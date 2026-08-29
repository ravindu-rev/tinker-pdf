"""Write testdata/form-fields.pdf.

Layout aid only: every byte of every object below is written out by hand here;
all this program computes is where each one landed, so the cross-reference
table (7.5.4) names real offsets. Ruling 13's line is between supplying and
adjudicating -- nothing here says whether the result is right. What says that
is `the_committed_form_fixture_is_the_document_these_tests_assume` in
crates/tinker-pdf-ffi/src/lib.rs, which opens the committed bytes with this
engine and checks the ladder level, the strict validator's verdict and the
field shapes.

The document: an /AcroForm with a text field, a checkbox whose on state is not
/Yes, and a radio pair -- the shapes edit.rs's own inline fixture uses -- plus
one deliberate defect. The text field "name" has two widget kids and the
second carries no /Rect, so 12.5.2 Table 164 is broken for that widget and
filling the field reports exactly one SkippedWidget. That widget is left out
of the page's /Annots on purpose: the strict validator walks /Annots, so a
defective widget listed there would make every artefact saved from this
fixture carry a defect, and the parity gate could then never demand a clean
one. A widget a field claims and a page does not show is a real damaged-form
shape, and it is the one that isolates the defect under test.
"""

import sys
from pathlib import Path

OBJECTS = {}

OBJECTS[1] = (
    b"<< /Type /Catalog /Pages 2 0 R\n"
    b"   /AcroForm << /Fields [5 0 R 8 0 R 11 0 R]\n"
    b"                /NeedAppearances true\n"
    b"                /DA (/Helv 0 Tf 0 g)\n"
    b"                /DR << /Font << /Helv 4 0 R >> >> >> >>"
)

OBJECTS[2] = b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>"

OBJECTS[3] = (
    b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200]\n"
    b"   /Resources << /Font << /Helv 4 0 R >> >>\n"
    b"   /Contents 14 0 R\n"
    b"   /Annots [6 0 R 8 0 R 12 0 R 13 0 R] >>"
)

OBJECTS[4] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"

# The text field. Two widget kids, and the second has no /Rect.
OBJECTS[5] = b"<< /FT /Tx /T (name) /Ff 0 /MaxLen 32 /Kids [6 0 R 7 0 R] >>"

OBJECTS[6] = (
    b"<< /Type /Annot /Subtype /Widget /Parent 5 0 R\n"
    b"   /Rect [20 140 280 160] /F 4 >>"
)

# 12.5.2 Table 164 makes /Rect required. This widget has none, which is the
# whole reason this fixture exists: filling "name" must report it rather than
# claim the field was drawn.
OBJECTS[7] = b"<< /Type /Annot /Subtype /Widget /Parent 5 0 R /F 4 >>"

# A checkbox merged with its own widget (12.7.3.3), whose on state is /On
# rather than /Yes.
OBJECTS[8] = (
    b"<< /Type /Annot /Subtype /Widget /FT /Btn /T (agree)\n"
    b"   /Rect [20 105 40 125] /F 4 /AS /Off\n"
    b"   /AP << /N << /On 9 0 R /Off 10 0 R >> >> >>"
)

APPEARANCE_ON = b"0 0 1 rg 2 2 16 16 re f\n"
APPEARANCE_OFF = b"0.8 0.8 0.8 rg 2 2 16 16 re f\n"

OBJECTS[9] = (
    b"<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length "
    + str(len(APPEARANCE_ON)).encode()
    + b" >>\nstream\n"
    + APPEARANCE_ON
    + b"endstream"
)

OBJECTS[10] = (
    b"<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length "
    + str(len(APPEARANCE_OFF)).encode()
    + b" >>\nstream\n"
    + APPEARANCE_OFF
    + b"endstream"
)

# A radio group: /Ff 32768 is Radio (12.7.4.2 Table 227).
OBJECTS[11] = b"<< /FT /Btn /Ff 32768 /T (colour) /V /Off /Kids [12 0 R 13 0 R] >>"

OBJECTS[12] = (
    b"<< /Type /Annot /Subtype /Widget /Parent 11 0 R\n"
    b"   /Rect [20 60 40 80] /F 4 /AS /Off\n"
    b"   /AP << /N << /red 9 0 R /Off 10 0 R >> >> >>"
)

OBJECTS[13] = (
    b"<< /Type /Annot /Subtype /Widget /Parent 11 0 R\n"
    b"   /Rect [60 60 80 80] /F 4 /AS /Off\n"
    b"   /AP << /N << /blue 9 0 R /Off 10 0 R >> >> >>"
)

CONTENT = (
    b"BT /Helv 12 Tf 20 175 Td (Tinker form fixture) Tj ET\n"
    b"0.5 w 20 140 260 20 re S\n"
)

OBJECTS[14] = (
    b"<< /Length " + str(len(CONTENT)).encode() + b" >>\nstream\n" + CONTENT + b"endstream"
)

# Two 16-byte identifiers, fixed rather than random: this file is committed, so
# it has to be the same bytes every time anybody regenerates it (ruling 4).
FILE_ID = b"<0123456789abcdef0123456789abcdef>"


def build():
    out = bytearray()
    out += b"%PDF-1.7\n"
    # 7.5.2: four bytes above 127, so transfer software does not treat the
    # file as text. Its absence is `binary-comment-missing` in this engine's
    # own validator, which is how it was noticed.
    out += b"%\xe2\xe3\xcf\xd3\n"

    offsets = {}
    for number in sorted(OBJECTS):
        offsets[number] = len(out)
        out += str(number).encode() + b" 0 obj\n"
        out += OBJECTS[number]
        out += b"\nendobj\n"

    startxref = len(out)
    highest = max(OBJECTS)
    out += b"xref\n"
    out += b"0 " + str(highest + 1).encode() + b"\n"
    out += b"0000000000 65535 f \n"
    for number in range(1, highest + 1):
        out += ("%010d 00000 n \n" % offsets[number]).encode()
    out += b"trailer\n"
    out += (
        b"<< /Size "
        + str(highest + 1).encode()
        + b" /Root 1 0 R /ID ["
        + FILE_ID
        + b" "
        + FILE_ID
        + b"] >>\n"
    )
    out += b"startxref\n"
    out += str(startxref).encode() + b"\n"
    out += b"%%EOF\n"
    return bytes(out)


def main():
    if len(sys.argv) != 2:
        print("usage: gen_form_fixture.py <out.pdf>", file=sys.stderr)
        return 2
    data = build()
    Path(sys.argv[1]).write_bytes(data)
    import hashlib

    print("wrote %d bytes sha256=%s" % (len(data), hashlib.sha256(data).hexdigest()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
