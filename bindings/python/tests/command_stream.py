"""The command stream through the Python binding (RC-18.6).

Run against a wheel that has been `pip install`ed, from anywhere in the
repository's checkout, which holds the face and the profile the page embeds:

    python bindings/python/tests/command_stream.py

The page holds every kind of record a page's stream is written with -- the
C ABI's test page (`crates/tinker-pdf-ffi/src/commands/tests.rs`,
`every_kind_page`), built again here -- and a stream pushed by hand holds the
three a page reaches only past a budget or through a recording that
composites colorants. Between the two, every kind but the end.

What is held:

- the stream decoded into `CommandRecord`s, each field a `CommandField` of its
  tag, and written back through the binding's own types -- each field rebuilt
  from its tag and its Python value, each record from its kind and fields --
  is the facade's bytes, byte for byte;
- a recording device receives every record, every kind among them, in the
  order the facade's decoder gives them, and a device that names only some
  methods hears only those kinds;
- the replay through the engine draws what a render of the page draws;
- bytes that are no stream are refused with the reader's sentence.

The script prints `COMMAND-STREAM: RAN` on success, because a script that
imports nothing and exits 0 looks exactly like a passing one.
"""

import pathlib
import sys

import tinker_pdf

ROOT = pathlib.Path(__file__).resolve().parents[3]
SERIF = ROOT / "crates/tinker-pdf-font/data/liberation/LiberationSerif-Regular.ttf"
RGB_PROFILE = ROOT / "fuzz/corpus/icc_profile/rgb-gamma-curve.icc"

# The tags in their numbers' order on the wire (`docs/design/command-stream.md`,
# "The form").
TAGS = ["nothing", "unsigned", "signed", "real", "bytes", "reals", "naturals", "strings"]


def stream_object(dictionary, data):
    """A stream object of `dictionary` holding `data`."""
    head = f"<< {dictionary} /Length {len(data)} >>\nstream\n".encode()
    return head + data + b"\nendstream"


def pdf(objects):
    """A file of `objects`, numbered from 1, the first the catalog, with a
    classic cross-reference table."""
    out = bytearray(b"%PDF-1.7\n")
    offsets = []
    for number, body in enumerate(objects, 1):
        offsets.append(len(out))
        out += f"{number} 0 obj\n".encode() + body + b"\nendobj\n"
    xref = len(out)
    out += f"xref\n0 {len(objects) + 1}\n0000000000 65535 f \n".encode()
    for offset in offsets:
        out += f"{offset:010} 00000 n \n".encode()
    out += (
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\n"
        f"startxref\n{xref}\n%%EOF\n"
    ).encode()
    return bytes(out)


def every_kind_page():
    """The 200 x 160 page whose stream holds every kind a page's stream is
    written with: a state with stored parameters, a black generation, a
    transfer function and a halftone; a clip and an axial shading in an ICC
    space; a fill in a /Separation space and one in a Lab space whose range is
    written backwards; text in an embedded face and in a font no name
    resolves; an image XObject and an inline image; marked content, a group
    under a luminosity soft mask then cleared; a tiling pattern, a form with
    resources of its own, a form that draws itself, and a square annotation
    with an appearance."""
    face = SERIF.read_bytes()
    profile = RGB_PROFILE.read_bytes()
    widths = " ".join(["500"] * 95)
    content = (
        b"q /Fancy gs 0 0 1 RG 2 w 10 10 m 100 10 l S 1 0 0 rg 10 20 30 20 re f Q\n"
        b"q 10 40 40 30 re W n /Sh0 sh Q\n"
        b"q /CS0 cs 0.7 scn 60 20 20 20 re f Q\n"
        b"q /Lab0 cs 50 10 10 sc 90 20 10 10 re f Q\n"
        b"BT /F0 12 Tf 10 120 Td (Glyphs) Tj ET\n"
        b"BT /Nope 12 Tf 10 140 Td (x) Tj ET\n"
        b"q 20 0 0 20 120 20 cm /Im0 Do Q\n"
        b"q 10 0 0 10 150 20 cm BI /W 2 /H 2 /CS /G /BPC 8 ID "
        + bytes([0x00, 0x55, 0xAA, 0xFF])
        + b" EI Q\n"
        b"/Span << /MCID 0 >> BDC 0 g 120 60 5 5 re f EMC\n"
        b"q /Masked gs 1 0 0 1 120 80 cm /G Do /NoMask gs 0 1 0 rg 0 0 5 5 re f Q\n"
        b"q /Pattern cs /P0 scn 150 60 30 30 re f Q\n"
        b"q 1 0 0 1 150 100 cm /Fm0 Do Q\n"
        b"/Rec Do\n"
    )
    rgb = bytes([255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0])
    return pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 160] /Resources 4 0 R "
            b"/Contents 5 0 R /Annots [17 0 R] >>",
            b"<< /Font << /F0 6 0 R >> "
            b"/XObject << /Im0 9 0 R /G 10 0 R /Fm0 11 0 R /Rec 12 0 R >> "
            b"/Shading << /Sh0 13 0 R >> /Pattern << /P0 15 0 R >> "
            b"/ColorSpace << "
            b"/CS0 [/Separation /Spot /DeviceRGB "
            b"<< /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [1 0 0] /N 1 >>] "
            b"/Lab0 [/Lab << /WhitePoint [0.9505 1 1.089] /Range [100 -100 -100 100] >>] >> "
            b"/ExtGState << "
            b"/Fancy << /RI /Saturation "
            b"/BG << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> "
            b"/TR << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 2 >> "
            b"/HT << /Type /Halftone /HalftoneType 1 /Frequency 60 /Angle 45 "
            b"/SpotFunction /Round >> >> "
            b"/Masked << /SMask << /Type /Mask /S /Luminosity /G 16 0 R >> >> "
            b"/NoMask << /SMask /None >> >> >>",
            stream_object("", content),
            (
                "<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSerif /FirstChar 32 "
                f"/LastChar 126 /Widths [{widths}] /FontDescriptor 7 0 R "
                "/Encoding /WinAnsiEncoding >>"
            ).encode(),
            b"<< /Type /FontDescriptor /FontName /LiberationSerif /Flags 32 "
            b"/FontBBox [-200 -300 1200 1000] /ItalicAngle 0 /Ascent 900 /Descent -200 "
            b"/CapHeight 700 /StemV 80 /FontFile2 8 0 R >>",
            stream_object(f"/Length1 {len(face)}", face),
            stream_object(
                "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB "
                "/BitsPerComponent 8",
                rgb,
            ),
            stream_object(
                "/Type /XObject /Subtype /Form /BBox [0 0 20 20] /Group << /S /Transparency >>",
                b"0 0.8 0 rg 0 0 20 20 re f",
            ),
            stream_object(
                "/Type /XObject /Subtype /Form /BBox [0 0 40 40] "
                "/Resources << /XObject << /ImF 9 0 R >> >>",
                b"q 10 0 0 10 0 0 cm /ImF Do Q",
            ),
            stream_object(
                "/Type /XObject /Subtype /Form /BBox [0 0 10 10] "
                "/Resources << /XObject << /Rec 12 0 R >> >>",
                b"0 0 1 rg 0 0 2 2 re f /Rec Do",
            ),
            b"<< /ShadingType 2 /ColorSpace [/ICCBased 14 0 R] /Coords [10 0 50 0] "
            b"/Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> "
            b"/Extend [true true] >>",
            stream_object("/N 3", profile),
            stream_object(
                "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 "
                "/YStep 10 /Resources << >>",
                b"1 0 0 rg 0 0 5 5 re f",
            ),
            stream_object(
                "/Type /XObject /Subtype /Form /BBox [0 0 200 160] "
                "/Group << /S /Transparency /CS /DeviceGray >>",
                b"0.5 g 0 0 200 160 re f",
            ),
            b"<< /Type /Annot /Subtype /Square /Rect [150 120 190 150] /AP << /N 18 0 R >> >>",
            stream_object(
                "/Type /XObject /Subtype /Form /BBox [0 0 40 30]",
                b"0 0 1 RG 2 w 1 1 38 28 re S",
            ),
        ]
    )


def rebuilt(field):
    """The field, made again from its tag and its Python value through the
    binding's constructors."""
    if field.tag_name == "nothing":
        return tinker_pdf.CommandField.nothing()
    return getattr(tinker_pdf.CommandField, field.tag_name)(field.value)


def through_python(records):
    """`records`, each made again from its kind and its fields' values."""
    return [
        tinker_pdf.CommandRecord(record.kind, [rebuilt(field) for field in record.fields])
        for record in records
    ]


def recorder():
    """A device with a method for every kind this build knows and for an
    unknown one, each noting the kind's number it was handed."""

    class Recorder:
        def __init__(self):
            self.seen = []

    def method(number):
        def note(self, record):
            if record.kind != number:
                raise AssertionError(f"method for kind {number} handed a {record.kind}")
            self.seen.append(record.kind)

        return note

    for kind in tinker_pdf.CommandKind.all():
        setattr(Recorder, kind.method, method(kind.number))

    def unknown(self, record):
        self.seen.append(("unknown", record.kind))

    Recorder.unknown = unknown
    return Recorder()


def check(condition, what):
    if not condition:
        raise SystemExit(f"FAILED: {what}")


def main():
    page = every_kind_page()
    document = tinker_pdf.Document(page)
    written = document.command_stream(0)
    stream = tinker_pdf.CommandStream(written)
    check(stream.to_bytes() == written, "the stream writes back its own bytes")
    check(
        stream.version == (tinker_pdf.COMMAND_STREAM_MAJOR, tinker_pdf.COMMAND_STREAM_MINOR),
        f"the stream is this build's version: {stream.version}",
    )
    check(stream.page_index == 0, "the stream is page 0's")
    check(stream.size == document.page_size(0), "the stream's page is the page's size")
    check(stream.usage is None, "the stream is of no usage")

    records = stream.records
    check(len(records) == len(stream), "every record, counted")
    check(records[0].name == "Page", f"the page record leads: {records[0]!r}")
    for record in records:
        kind = record.command_kind
        check(kind is not None, f"kind {record.kind} is one this build knows")
        check(record.name == kind.name, f"{record!r} names its kind")
        check(
            len(record.fields) <= len(record.field_names),
            f"{record!r} holds no more fields than its kind names",
        )
        for at, field in enumerate(record.fields):
            check(field == rebuilt(field), f"{record!r}'s field {at} rebuilds")
            check(
                field.tag == TAGS.index(field.tag_name),
                f"{record!r}'s field {at} is tag {field.tag}, its {field.tag_name}",
            )
            if at < len(record.field_names):
                named = record.field(record.field_names[at])
                check(named == field, f"{record!r}'s field {at} is read by its name")
    check(
        tinker_pdf.CommandStream.from_records(through_python(records)).to_bytes() == written,
        "the stream decoded and written back through the binding's types is the "
        "facade's bytes",
    )

    on_the_page = {record.kind for record in records}
    by_hand = {"Colorants", "GroupBudgetSpent", "NestedBudgetSpent"}
    kinds = {kind.name: kind for kind in tinker_pdf.CommandKind.all()}
    for name, kind in kinds.items():
        if name != "End" and name not in by_hand:
            check(kind.number in on_the_page, f"the page's stream holds a {name} record")

    F = tinker_pdf.CommandField
    hand = [
        records[0],
        tinker_pdf.CommandRecord(
            kinds["Colorants"].number,
            [
                F.unsigned(1),
                F.strings([b"Spot", b"Other"]),
                F.bytes(bytes([128, 64])),
                F.naturals([1]),
                F.bytes(bytes([0, 0, 0, 255])),
                F.nothing(),
                F.bytes(bytes([10, 20, 30, 40])),
            ],
        ),
        tinker_pdf.CommandRecord(kinds["GroupBudgetSpent"].number, []),
        tinker_pdf.CommandRecord(kinds["NestedBudgetSpent"].number, []),
    ]
    pushed = tinker_pdf.CommandStream.from_records(hand)
    hand_bytes = pushed.to_bytes()
    reread = tinker_pdf.CommandStream(hand_bytes)
    check(reread.records == hand, "the records pushed by hand read back as pushed")
    check(
        tinker_pdf.CommandStream.from_records(through_python(reread.records)).to_bytes()
        == hand_bytes,
        "and write back through the binding's types",
    )

    for each in (stream, reread):
        device = recorder()
        each.replay(device)
        check(
            device.seen == [record.kind for record in each.records],
            "the device receives every record in the decoder's order",
        )
    seen = set(recorder_seen(stream)) | set(recorder_seen(reread))
    for name, kind in kinds.items():
        if name != "End":
            check(kind.number in seen, f"the device received a {name} record")

    class Glyphs:
        def __init__(self):
            self.runs = 0

        def glyph_run(self, record):
            check(record.name == "GlyphRun", "glyph_run is handed glyph runs")
            check(record.field("font id").tag_name == "unsigned", "a run names its font")
            self.runs += 1

    glyphs = Glyphs()
    stream.replay(glyphs)
    check(
        glyphs.runs == sum(1 for record in records if record.name == "GlyphRun"),
        "a device naming one method hears only that kind",
    )

    class Refusing:
        def begin_text(self, record):
            raise RuntimeError("the device refuses")

    try:
        stream.replay(Refusing())
    except RuntimeError as error:
        check("refuses" in str(error), "the device's exception is raised")
    else:
        raise SystemExit("FAILED: a device's exception ends the replay")

    drawn = stream.render(72.0)
    direct = document.render(0, 72.0)
    check(
        (drawn.width, drawn.height, drawn.data) == (direct.width, direct.height, direct.data),
        "the replay draws what a render of the page draws",
    )
    check(stream.pixel_size(72.0) == (drawn.width, drawn.height), "the replay's size")

    budgets = tinker_pdf.CommandBudgets(records=4)
    try:
        tinker_pdf.CommandStream.parse_with(written, budgets)
    except ValueError as error:
        check("records" in str(error), f"a stream past a budget names it: {error}")
    else:
        raise SystemExit("FAILED: a stream of more records than its budget is refused")
    check(
        tinker_pdf.CommandStream.parse_with(written, tinker_pdf.CommandBudgets()).to_bytes()
        == written,
        "the default budgets read the stream",
    )
    for refused in (b"", b"not a command stream", written[:-1]):
        try:
            tinker_pdf.CommandStream(refused)
        except ValueError:
            pass
        else:
            raise SystemExit(f"FAILED: {refused[:16]!r}... is no stream and was read")
    try:
        tinker_pdf.CommandStream(b"plainly not one")
    except ValueError as error:
        check("not a command stream" in str(error), f"the reader's sentence: {error}")

    glyph_run = tinker_pdf.CommandKind.from_number(17)
    check(glyph_run is not None and glyph_run.name == "GlyphRun", "kind 17 is a glyph run")
    check(glyph_run.method == "glyph_run" and glyph_run.is_command, "and a command")
    check(tinker_pdf.CommandKind.from_number(63) is None, "kind 63 is none this build knows")
    check(not kinds["Page"].is_codec and kinds["State"].is_codec, "the codec's kinds")

    print(f"records={len(records)} hand={len(hand)} kinds={len(kinds)}")
    print("COMMAND-STREAM: RAN")


def recorder_seen(stream):
    """The kinds a recording device hears from `stream`."""
    device = recorder()
    stream.replay(device)
    return device.seen


if __name__ == "__main__":
    sys.exit(main())
