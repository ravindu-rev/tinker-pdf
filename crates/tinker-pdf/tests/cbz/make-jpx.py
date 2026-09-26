# Writes python-jpx.cbz: T.800 Annex J.10's codestream as two comic pages.
#
# J.10 is the one JPEG 2000 file whose decoded samples a standard publishes
# (J.10.5: 101, 103, 104, 105, 96, 97, 96, 102, 109, one component, 1 x 9), so
# a comic page made of it has an expected picture no decoder produced. The
# 100 bytes are read out of `tinker-pdf-filters/tests/jpx_annex_j.rs`, where
# they are transcribed field by field and checked against J.10's own octal
# offsets, rather than transcribed a second time here.
#
#   page1.j2k   the bare codestream, exactly J.10's 100 bytes
#   page2.jp2   the same codestream inside T.800 Annex I's JP2 boxes: the
#               signature box (I.5.1), `ftyp` naming `jp2 ` (I.5.2), a `jp2h`
#               holding `ihdr` (I.5.3.1: 9 high, 1 wide, one component of
#               8 unsigned bits, compression type 7) and `colr` (I.5.3.3:
#               method 1, EnumCS 17, greyscale), then `jp2c` (I.5.4)
#
# The boxes are this repository's, written from Annex I; the codestream inside
# them is the standard's. Both entries are stored, so the archive's CRC-32 is
# over the very bytes the page's `/JPXDecode` stream will carry.
#
#   cd crates/tinker-pdf/tests/cbz && python3 make-jpx.py
#
# One fixed timestamp, so a rerun under the same CPython writes the same bytes.
# Not run by any test: the committed archive is the record (ruling 13).

import re
import struct
import zipfile

SOURCE = "../../../tinker-pdf-filters/tests/jpx_annex_j.rs"
STAMP = (2026, 9, 26, 0, 0, 0)


def annex_j10():
    text = open(SOURCE).read()
    body = re.search(r"const ANNEX_J10: &\[u8\] = &\[(.*?)\];", text, re.S).group(1)
    body = re.sub(r"//[^\n]*", "", body)
    data = bytes(int(h, 16) for h in re.findall(r"0x([0-9A-Fa-f]{2})", body))
    assert len(data) == 100 and data[:4] == b"\xff\x4f\xff\x51" and data[-2:] == b"\xff\xd9"
    return data


def box(kind, payload):
    return struct.pack(">I", 8 + len(payload)) + kind + payload


def jp2(codestream, width, height):
    signature = box(b"jP  ", b"\x0d\x0a\x87\x0a")
    ftyp = box(b"ftyp", b"jp2 " + struct.pack(">I", 0) + b"jp2 ")
    # HEIGHT, WIDTH, NC, BPC (precision - 1, unsigned), C = 7, UnkC, IPR.
    ihdr = box(b"ihdr", struct.pack(">IIHBBBB", height, width, 1, 7, 7, 0, 0))
    # METH 1 (enumerated), PREC, APPROX, EnumCS 17 (greyscale).
    colr = box(b"colr", struct.pack(">BBBI", 1, 0, 0, 17))
    return signature + ftyp + box(b"jp2h", ihdr + colr) + box(b"jp2c", codestream)


codestream = annex_j10()
pages = [("page1.j2k", codestream), ("page2.jp2", jp2(codestream, 1, 9))]

with zipfile.ZipFile("python-jpx.cbz", "w") as z:
    for name, data in pages:
        info = zipfile.ZipInfo(name, date_time=STAMP)
        info.compress_type = zipfile.ZIP_STORED
        z.writestr(info, data)
