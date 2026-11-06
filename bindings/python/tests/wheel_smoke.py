"""Proves an installed wheel is the engine, not merely an importable module.

Run against a wheel that has been `pip install`ed, never against the source
tree:

    python bindings/python/tests/wheel_smoke.py testdata/simple-text.pdf FACE.ttf

The trap here is PRE-A's, one ecosystem out. `testdata/simple-text.pdf` names
Helvetica and embeds no font program, and this engine bundles no faces and
reads no font directories -- so a render of it returns a correctly sized,
entirely blank bitmap. Every assertion of the shape "a bitmap came back with
the right dimensions" passes on that, and it would pass equally on a wheel
whose renderer had been replaced with a memset.

So the render is asserted twice: **blank without a face, inked with one.**
The pair is what says the pixels came from the engine, and it exercises the
`set_fonts` seam at the same time -- which is the call a Python host must
make for any document that does not embed its fonts, and therefore the one
most worth proving crosses the binding.

The script prints `WHEEL-SMOKE: RAN` on success. CI greps for that, because a
Python script that imports nothing and exits 0 looks exactly like a passing
one.
"""

import pathlib
import sys
import tempfile

import tinker_pdf


"""One face per platform the release matrix builds on.

`auto` resolves through this list. It is a list rather than a fallback to
"render without a face" on purpose: a missing face would otherwise turn the
inked half of this test into a skip, and a skip exits 0 and reads exactly like
a pass -- the failure mode every RAN / SKIPPED banner in this repository is
written to avoid.
"""
FACES = [
    # windows-latest
    "C:/Windows/Fonts/arial.ttf",
    # macos runners
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    # ubuntu-latest, after `apt-get install fonts-dejavu-core`
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
]


def resolve_face(argument):
    if argument != "auto":
        return pathlib.Path(argument)
    for candidate in FACES:
        path = pathlib.Path(candidate)
        if path.is_file():
            return path
    raise SystemExit(
        "no font face found. This test needs one: the engine bundles no faces, "
        "so without one the inked half cannot run, and a version of it that "
        "quietly skipped would pass on a wheel that renders nothing at all.\n"
        "Looked in:\n  " + "\n  ".join(FACES)
    )


def ink(bitmap):
    """Pixels whose first channel is not the white background."""
    pixels = bitmap.data
    return sum(1 for i in range(0, len(pixels), bitmap.components) if pixels[i] != 0xFF)


def pdf(bodies):
    """A PDF of `bodies`, numbered from 1, the first its catalog, with a
    classic cross-reference table."""
    out = bytearray(b"%PDF-1.7\n")
    offsets = []
    for number, body in enumerate(bodies, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % number + body + b"\nendobj\n"
    table = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(bodies) + 1)
    for offset in offsets:
        out += b"%010d 00000 n \n" % offset
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (
        len(bodies) + 1,
        table,
    )
    return bytes(out)


def heavy(pages, size):
    """`pages` pages of a line of Helvetica text and about `size` bytes of
    drawing each: few objects and many bytes, as a scan or a drawing is."""
    font = 3 + 2 * pages
    kids = b" ".join(b"%d 0 R" % (3 + 2 * at) for at in range(pages))
    bodies = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count %d /Kids [%s] >>" % (pages, kids),
    ]
    drawing = b"0 0 1 1 re f\n" * (size // 13)
    for at in range(pages):
        bodies.append(
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]"
            b" /Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>" % (font, 4 + 2 * at)
        )
        content = b"BT /F1 12 Tf 10 50 Td (page %d on demand) Tj ET\n" % (at + 1) + drawing
        bodies.append(b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream")
    bodies.append(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    return pdf(bodies)


def damaged():
    """A catalog and an empty tree whose table's second entry points into the
    first object, which an open of the bytes patches."""
    data = bytearray(pdf([b"<< /Type /Catalog /Pages 2 0 R >>", b"<< /Type /Pages /Count 0 /Kids [] >>"]))
    entry = data.index(b"xref\n0 3\n") + 9 + 2 * 20
    data[entry : entry + 10] = b"0000000004"
    return bytes(data)


def updated():
    """Three pages saved linearized, then a title appended as an update, which
    leaves the length the linearization states behind."""
    linearized = tinker_pdf.Document(heavy(3, 0)).editor().save(mode="rewrite", linearize=True)
    editor = tinker_pdf.Document(linearized).editor()
    editor.set_title("updated")
    return editor.save(mode="incremental")


def warned(document):
    return [(w.kind, w.offset, w.object) for w in document.warnings()]


def on_demand():
    """`Document.open(path)` reads on demand by the rule: a file whose bytes
    open the same streamed stays streamed and answers as its bytes do, and a
    damaged file and a linearized one updated since are read whole and warn
    what their bytes warn."""
    with tempfile.TemporaryDirectory() as directory:
        data = heavy(4, 1 << 20)
        path = pathlib.Path(directory) / "heavy.pdf"
        path.write_bytes(data)
        by_path = tinker_pdf.Document.open(str(path))
        by_bytes = tinker_pdf.Document(data)
        assert by_path.page_count == by_bytes.page_count == 4, (by_path.page_count, by_bytes.page_count)
        assert by_path.page_text(0) == by_bytes.page_text(0), (by_path.page_text(0), by_bytes.page_text(0))
        assert "page 1 on demand" in by_path.page_text(0), by_path.page_text(0)
        assert by_path.is_streamed, "a file its bytes answer for stays on demand"
        assert not by_bytes.is_streamed
        print(f"on demand: {len(data)} bytes, {by_path.page_count} pages, streamed")
        for name, data in [("damaged", damaged()), ("updated", updated())]:
            path = pathlib.Path(directory) / f"{name}.pdf"
            path.write_bytes(data)
            by_path = tinker_pdf.Document.open(str(path))
            by_bytes = tinker_pdf.Document(data)
            assert warned(by_path) == warned(by_bytes), (name, warned(by_path), warned(by_bytes))
            assert not by_path.is_streamed, f"{name}: read whole"
            print(f"{name}: read whole, {len(warned(by_path))} warnings, as its bytes")
        del by_path, by_bytes


def main(pdf_path, face_path):
    pdf = pathlib.Path(pdf_path).read_bytes()
    face_file = resolve_face(face_path)
    face = face_file.read_bytes()
    print(f"face {face_file}")

    print(
        f"interpreter {sys.version_info.major}.{sys.version_info.minor}, "
        f"tinker_pdf {tinker_pdf.__version__}"
    )

    document = tinker_pdf.Document(pdf)
    print(
        f"pages={document.page_count} encrypted={document.is_encrypted} "
        f"size={document.page_size(0)}"
    )
    assert document.page_count == 3, document.page_count
    assert not document.is_encrypted

    text = document.page_text(0)
    assert "Tinker fixture" in text, text
    print(f"text={text.strip()[:40]!r}")

    bare = document.render(0, dpi=72.0)
    assert ink(bare) == 0, (
        "this fixture embeds no font program, so a build that bundles no faces "
        "must draw nothing -- if it drew something, this test is measuring the "
        "wrong thing"
    )

    document.set_fonts(face)
    drawn = document.render(0, dpi=72.0)
    painted = ink(drawn)
    print(
        f"bitmap {drawn.width}x{drawn.height} stride={drawn.stride} "
        f"comps={drawn.components} bytes={len(drawn.data)} ink={painted}"
    )
    assert painted > 100, f"only {painted} pixels of ink with a face supplied"
    assert len(drawn.data) == drawn.stride * drawn.height, "the pixel buffer is the wrong size"

    # The buffer protocol plan 13 names, which is how numpy and Pillow ingest
    # pixels without a copy.
    view = memoryview(drawn.data)
    assert view.nbytes == len(drawn.data)

    on_demand()

    print("WHEEL-SMOKE: RAN, rendered, inked and opened on demand")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(__doc__)
        raise SystemExit(2)
    main(sys.argv[1], sys.argv[2])
