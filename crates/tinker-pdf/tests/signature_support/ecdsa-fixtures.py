"""Builds the ECDSA-signed PDF fixtures beside this file.

Run once, on 14 September 2026, with OpenSSL 3.5.5; the outputs are committed
and nothing re-runs this. The aid lays out the objects, computes the four
`/ByteRange` numbers and splices OpenSSL's detached CMS into the reservation --
it adjudicates nothing (ruling 13). README.md beside it says what each half of
the result is worth as evidence, and warns that a regeneration produces
different bytes because the keys are fresh each time.

    python ecdsa-fixtures.py <out-dir> <work-dir>
"""

import os
import subprocess
import sys

OUT = sys.argv[1]
WORK = sys.argv[2]
os.makedirs(WORK, exist_ok=True)
os.makedirs(OUT, exist_ok=True)

NOT_BEFORE = "20260101000000Z"
NOT_AFTER = "21260101000000Z"


def run(*args):
    result = subprocess.run(args, capture_output=True)
    if result.returncode != 0:
        raise SystemExit(
            " ".join(args)
            + "\n"
            + result.stdout.decode(errors="replace")
            + "\n"
            + result.stderr.decode(errors="replace")
        )
    return result.stdout


def w(name):
    return os.path.join(WORK, name)


def make_chain(tag, curve, md):
    """A self-signed root and a leaf it issues, both on `curve`, both signed with `md`."""
    run("openssl", "ecparam", "-name", curve, "-genkey", "-noout", "-out", w(tag + "-root.key"))
    run(
        "openssl", "req", "-x509", "-new", "-key", w(tag + "-root.key"), "-" + md,
        "-set_serial", "1",
        "-not_before", NOT_BEFORE, "-not_after", NOT_AFTER,
        "-subj", "/CN=Tinker PDF ECDSA " + tag.upper() + " Test Root/O=tinker-pdf test fixture",
        "-out", w(tag + "-root.pem"),
    )
    run("openssl", "ecparam", "-name", curve, "-genkey", "-noout", "-out", w(tag + "-leaf.key"))
    run(
        "openssl", "req", "-new", "-key", w(tag + "-leaf.key"),
        "-subj", "/CN=Tinker PDF ECDSA " + tag.upper() + " Test Signer/O=tinker-pdf test fixture",
        "-out", w(tag + "-leaf.csr"),
    )
    run(
        "openssl", "x509", "-req", "-in", w(tag + "-leaf.csr"),
        "-CA", w(tag + "-root.pem"), "-CAkey", w(tag + "-root.key"),
        "-set_serial", "2", "-" + md,
        "-not_before", NOT_BEFORE, "-not_after", NOT_AFTER,
        "-out", w(tag + "-leaf.pem"),
    )
    der = run("openssl", "x509", "-in", w(tag + "-root.pem"), "-outform", "DER")
    with open(os.path.join(OUT, "ecdsa-" + tag + "-root.der"), "wb") as f:
        f.write(der)


PLACEHOLDER = "[0000000000 0000000000 0000000000 0000000000]"
SIGNATURE_ENTRIES = (
    "/Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached "
    "/M (D:20260101120000Z) /Reason (An ECDSA signature, for the verdict path) "
    "/Name (Tinker PDF ECDSA Test Signer) "
    "/ByteRange " + PLACEHOLDER + " /Contents <"
)


def build_pdf(reserve):
    """A one-page document with one signature field whose `/ByteRange` covers
    every byte but the `/Contents` gap -- the layout the inline fixtures in
    crates/tinker-pdf/tests/signatures.rs build, written out so the covered
    bytes can be handed to a signer."""
    count = 6
    fixed = [
        (1, "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] /SigFlags 3 >> >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 6 0 R >>"),
        (4, "<< /Type /Annot /Subtype /Widget /FT /Sig /T (Signature1) /V 5 0 R "
            "/Rect [0 0 0 0] /F 4 /P 3 0 R >>"),
    ]
    stream = b"0.2 0.3 0.8 rg 20 20 160 160 re f\n"

    out = bytearray(b"%PDF-1.7\n")
    offsets = [0] * (count + 1)
    for num, body in fixed:
        offsets[num] = len(out)
        out += (str(num) + " 0 obj\n" + body + "\nendobj\n").encode()

    offsets[5] = len(out)
    head = "5 0 obj\n<< /Type /Sig " + SIGNATURE_ENTRIES
    byte_range_at = len(out) + head.index(PLACEHOLDER)
    out += head.encode()
    gap_start = len(out) - 1  # the `<` itself is inside the gap
    contents_at = len(out)
    out += ("0" * (reserve * 2)).encode()
    out += b">"
    gap_end = len(out)
    out += b" >>\nendobj\n"

    offsets[6] = len(out)
    out += ("6 0 obj\n<< /Length " + str(len(stream)) + " >>\nstream\n").encode()
    out += stream + b"endstream\nendobj\n"

    xref_at = len(out)
    out += ("xref\n0 " + str(count + 1) + "\n0000000000 65535 f \n").encode()
    for entry in offsets[1:count + 1]:
        out += ("%010d 00000 n \n" % entry).encode()
    out += ("trailer\n<< /Size " + str(count + 1) + " /Root 1 0 R >>\n"
            "startxref\n" + str(xref_at) + "\n%%EOF\n").encode()

    tail_len = len(out) - gap_end
    numbers = "[%010d %010d %010d %010d]" % (0, gap_start, gap_end, tail_len)
    assert len(numbers) == len(PLACEHOLDER), (numbers, PLACEHOLDER)
    out[byte_range_at:byte_range_at + len(numbers)] = numbers.encode()
    return out, contents_at, [(0, gap_start), (gap_end, tail_len)]


def sign(tag, md, reserve):
    out, contents_at, spans = build_pdf(reserve)

    covered = bytearray()
    for start, length in spans:
        covered += out[start:start + length]
    with open(w(tag + "-covered.bin"), "wb") as f:
        f.write(covered)

    run(
        "openssl", "cms", "-sign", "-binary", "-in", w(tag + "-covered.bin"),
        "-signer", w(tag + "-leaf.pem"), "-inkey", w(tag + "-leaf.key"),
        "-certfile", w(tag + "-root.pem"),
        "-md", md, "-outform", "DER", "-nosmimecap",
        "-out", w(tag + "-cms.der"),
    )
    with open(w(tag + "-cms.der"), "rb") as f:
        der = f.read()
    print(tag + ": covered " + str(len(covered)) + " bytes, CMS " + str(len(der))
          + " bytes, reserve " + str(reserve))
    assert len(der) <= reserve, tag

    hexed = der.hex().upper() + "0" * ((reserve - len(der)) * 2)
    assert len(hexed) == reserve * 2
    out[contents_at:contents_at + reserve * 2] = hexed.encode()

    path = os.path.join(OUT, "ecdsa-" + tag + ".pdf")
    with open(path, "wb") as f:
        f.write(out)
    return path


make_chain("p256", "prime256v1", "sha256")
make_chain("p384", "secp384r1", "sha384")
print(sign("p256", "sha256", 1600))
print(sign("p384", "sha384", 1800))
