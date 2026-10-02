"""Builds the signed-PDF fixtures beside this file that `ecdsa-fixtures.py` does not.

Run once, on 2 October 2026, with OpenSSL 3.0.13; the outputs are committed and
nothing re-runs this. Each fixture is one signature shape a published vector
cannot carry into a whole document. The aid lays out the objects, computes the
four `/ByteRange` numbers and splices OpenSSL's CMS into the reservation -- it
adjudicates nothing (ruling 13). README.md beside it says what each half of the
result is worth as evidence; a regeneration produces different bytes, because
the keys are fresh each time.

    python3 signature-fixtures.py <out-dir> <work-dir> <fixture>...

where each <fixture> is one of the names in BUILDERS at the bottom.
"""

import hashlib
import os
import subprocess
import sys

OUT = sys.argv[1]
WORK = sys.argv[2]
WANTED = sys.argv[3:]
os.makedirs(WORK, exist_ok=True)
os.makedirs(OUT, exist_ok=True)

DAYS = "36500"


def run(*args, stdin=None):
    result = subprocess.run(args, capture_output=True, input=stdin)
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


def save(name, data):
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(data)
    print(name, len(data), hashlib.sha256(data).hexdigest())


def der_of(pem):
    return run("openssl", "x509", "-in", pem, "-outform", "DER")


PLACEHOLDER = "[0000000000 0000000000 0000000000 0000000000]"


def build_pdf(reserve, sub_filter, reason, name, sig_type="Sig"):
    """A one-page document with one signature field whose `/ByteRange` covers
    every byte but the `/Contents` gap: the layout `ecdsa-fixtures.py` builds,
    with the `/SubFilter` and `/Type` as parameters."""
    count = 6
    fixed = [
        (1, "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] /SigFlags 3 >> >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 6 0 R >>"),
        (4, "<< /Type /Annot /Subtype /Widget /FT /Sig /T (Signature1) /V 5 0 R "
            "/Rect [0 0 0 0] /F 4 /P 3 0 R >>"),
    ]
    stream = b"0.2 0.6 0.3 rg 20 20 160 160 re f\n"
    entries = (
        "/Filter /Adobe.PPKLite /SubFilter /" + sub_filter + " "
        "/M (D:20261002120000Z) /Reason (" + reason + ") /Name (" + name + ") "
        "/ByteRange " + PLACEHOLDER + " /Contents <"
    )

    out = bytearray(b"%PDF-1.7\n")
    offsets = [0] * (count + 1)
    for num, body in fixed:
        offsets[num] = len(out)
        out += (str(num) + " 0 obj\n" + body + "\nendobj\n").encode()

    offsets[5] = len(out)
    head = "5 0 obj\n<< /Type /" + sig_type + " " + entries
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

    covered = bytearray()
    for start, length in [(0, gap_start), (gap_end, tail_len)]:
        covered += out[start:start + length]
    return out, contents_at, bytes(covered)


def splice(out, contents_at, reserve, der):
    assert len(der) <= reserve, (len(der), reserve)
    hexed = der.hex().upper() + "0" * ((reserve - len(der)) * 2)
    out[contents_at:contents_at + reserve * 2] = hexed.encode()
    return bytes(out)


def rsa_root(tag, subject, sigopts=()):
    """A self-signed RSA root on a 2048-bit `rsaEncryption` key."""
    run("openssl", "genpkey", "-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048",
        "-out", w(tag + "-root.key"))
    run("openssl", "req", "-x509", "-new", "-key", w(tag + "-root.key"), "-sha256",
        *sigopts, "-set_serial", "1", "-days", DAYS, "-subj", subject,
        "-out", w(tag + "-root.pem"))


def leaf(tag, subject, keygen, sigopts=(), extfile=None):
    """A leaf the root issues, on a key `keygen` describes."""
    run("openssl", "genpkey", *keygen, "-out", w(tag + "-leaf.key"))
    run("openssl", "req", "-new", "-key", w(tag + "-leaf.key"), "-subj", subject,
        "-out", w(tag + "-leaf.csr"))
    extra = ["-extfile", extfile] if extfile else []
    run("openssl", "x509", "-req", "-in", w(tag + "-leaf.csr"),
        "-CA", w(tag + "-root.pem"), "-CAkey", w(tag + "-root.key"),
        "-set_serial", "2", "-sha256", *sigopts, "-days", DAYS, *extra,
        "-out", w(tag + "-leaf.pem"))


PSS_SIGOPTS = ("-sigopt", "rsa_padding_mode:pss", "-sigopt", "rsa_pss_saltlen:32")


def rsa_pss():
    """RSASSA-PSS end to end: the root signs the leaf's certificate with PSS,
    the leaf's own key is an `id-RSASSA-PSS` key restricted to SHA-256 and a
    salt of at least 32, and the CMS signature is PSS under it."""
    tag = "pss"
    rsa_root(tag, "/CN=Tinker PDF RSASSA-PSS Test Root/O=tinker-pdf test fixture",
             PSS_SIGOPTS)
    leaf(tag, "/CN=Tinker PDF RSASSA-PSS Test Signer/O=tinker-pdf test fixture",
         ("-algorithm", "RSA-PSS", "-pkeyopt", "rsa_keygen_bits:2048",
          "-pkeyopt", "rsa_pss_keygen_md:sha256", "-pkeyopt", "rsa_pss_keygen_mgf1_md:sha256",
          "-pkeyopt", "rsa_pss_keygen_saltlen:32"),
         PSS_SIGOPTS)
    reserve = 4000
    out, contents_at, covered = build_pdf(
        reserve, "adbe.pkcs7.detached", "An RSASSA-PSS signature",
        "Tinker PDF RSASSA-PSS Test Signer")
    with open(w(tag + "-covered.bin"), "wb") as f:
        f.write(covered)
    run("openssl", "cms", "-sign", "-binary", "-in", w(tag + "-covered.bin"),
        "-signer", w(tag + "-leaf.pem"), "-inkey", w(tag + "-leaf.key"),
        "-certfile", w(tag + "-root.pem"), "-md", "sha256",
        "-keyopt", "rsa_padding_mode:pss", "-keyopt", "rsa_pss_saltlen:32",
        "-outform", "DER", "-nosmimecap", "-out", w(tag + "-cms.der"))
    with open(w(tag + "-cms.der"), "rb") as f:
        der = f.read()
    save("rsa-pss.pdf", splice(out, contents_at, reserve, der))
    save("rsa-pss-root.der", der_of(w(tag + "-root.pem")))


BUILDERS = {
    "rsa-pss": rsa_pss,
}

for wanted in WANTED:
    BUILDERS[wanted]()
