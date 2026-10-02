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


def rsa_chain(tag, what):
    """A root and a leaf it issues, both on 2048-bit `rsaEncryption` keys and
    both signed with PKCS#1 v1.5 and SHA-256: the ordinary chain, so that the
    shape under test is the signature's and nothing else's."""
    rsa_root(tag, "/CN=Tinker PDF " + what + " Test Root/O=tinker-pdf test fixture")
    leaf(tag, "/CN=Tinker PDF " + what + " Test Signer/O=tinker-pdf test fixture",
         ("-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048"))


def cms_sign(tag, content, *options):
    """`openssl cms -sign` over the bytes in `content`, the root included, as DER."""
    with open(w(tag + "-content.bin"), "wb") as f:
        f.write(content)
    run("openssl", "cms", "-sign", "-binary", "-in", w(tag + "-content.bin"),
        "-signer", w(tag + "-leaf.pem"), "-inkey", w(tag + "-leaf.key"),
        "-certfile", w(tag + "-root.pem"), "-outform", "DER", *options,
        "-out", w(tag + "-cms.der"))
    with open(w(tag + "-cms.der"), "rb") as f:
        return f.read()


def pkcs7_sha1(attributes):
    """12.8.3.3.1's `adbe.pkcs7.sha1`: the SHA-1 digest of the covered bytes is
    the *encapsulated content* (`-nodetach`), and the signer digests that
    content with SHA-256 -- two digests, so a reader that confused them is
    caught. With `attributes` false the signer carries none (`-noattr`), and
    the signature is over the twenty octets directly."""
    tag = "sha1" if attributes else "sha1-noattr"
    rsa_chain(tag, "adbe.pkcs7.sha1")
    reserve = 4000
    out, contents_at, covered = build_pdf(
        reserve, "adbe.pkcs7.sha1", "An adbe.pkcs7.sha1 signature",
        "Tinker PDF adbe.pkcs7.sha1 Test Signer")
    options = ["-nodetach", "-md", "sha256"]
    options += ["-nosmimecap"] if attributes else ["-noattr"]
    der = cms_sign(tag, hashlib.sha1(covered).digest(), *options)
    name = "pkcs7-sha1" if attributes else "pkcs7-sha1-no-attributes"
    save(name + ".pdf", splice(out, contents_at, reserve, der))
    save(name + "-root.der", der_of(w(tag + "-root.pem")))


def no_signed_attributes():
    """A detached `adbe.pkcs7.detached` signature with no signed attributes
    (`-noattr`): RFC 5652 §5.4's other case, where the signature is over the
    digest of the content itself -- here, the covered bytes."""
    tag = "noattr"
    rsa_chain(tag, "No Signed Attributes")
    reserve = 4000
    out, contents_at, covered = build_pdf(
        reserve, "adbe.pkcs7.detached", "A signature with no signed attributes",
        "Tinker PDF No Signed Attributes Test Signer")
    der = cms_sign(tag, covered, "-md", "sha256", "-noattr")
    save("no-signed-attributes.pdf", splice(out, contents_at, reserve, der))
    save("no-signed-attributes-root.der", der_of(w(tag + "-root.pem")))


GENERAL_NAMES_EXTENSIONS = """\
basicConstraints = CA:FALSE
keyUsage = critical, digitalSignature, nonRepudiation
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always, issuer:always
subjectAltName = email:signer@example.com, DNS:signer.example.com, URI:https://example.com/signer, IP:192.0.2.7, IP:2001:db8::7, RID:1.2.3.4, otherName:1.3.6.1.4.1.311.20.2.3;UTF8:signer@example.com, dirName:directory
issuerAltName = URI:https://example.com/root

[directory]
CN = Tinker PDF Directory Name
O = tinker-pdf test fixture
"""


def cades_general_names():
    """`GeneralNames` in a real signature: a CAdES signer (`-cades`, so the
    signed attributes carry RFC 5035's `signingCertificateV2` with an
    `issuerSerial` naming the signer's issuer as a directory name) under a leaf
    whose `subjectAltName` uses eight of the nine alternatives, whose
    `issuerAltName` is a URI, and whose `authorityKeyIdentifier` names the
    issuer's issuer and serial (`issuer:always`)."""
    tag = "names"
    rsa_root(tag, "/CN=Tinker PDF GeneralNames Test Root/O=tinker-pdf test fixture")
    with open(w(tag + "-ext.cnf"), "w") as f:
        f.write(GENERAL_NAMES_EXTENSIONS)
    leaf(tag, "/CN=Tinker PDF GeneralNames Test Signer/O=tinker-pdf test fixture",
         ("-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048"),
         extfile=w(tag + "-ext.cnf"))
    reserve = 5000
    out, contents_at, covered = build_pdf(
        reserve, "ETSI.CAdES.detached", "A CAdES signature naming its certificate",
        "Tinker PDF GeneralNames Test Signer")
    der = cms_sign(tag, covered, "-md", "sha256", "-cades", "-nosmimecap")
    save("cades-general-names.pdf", splice(out, contents_at, reserve, der))
    save("cades-general-names-root.der", der_of(w(tag + "-root.pem")))


# ---- RFC 3161: a timestamping authority, and DER surgery to carry its token --

TSA_EXTENSIONS = """\
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature, nonRepudiation
extendedKeyUsage = critical, timeStamping
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always
"""


def tsa(tag, ess_algorithm):
    """A timestamping authority: its own RSA root, and a leaf whose only
    extended key usage is `timeStamping`, critical (RFC 3161 §2.3).
    `ess_algorithm` picks the ESS attribute the token carries: `sha1` is RFC
    2634's `signingCertificate`, anything else RFC 5816's
    `signingCertificateV2` under that digest."""
    rsa_root(tag + "-tsa", "/CN=Tinker PDF Timestamp Root/O=tinker-pdf test fixture")
    with open(w(tag + "-tsa-ext.cnf"), "w") as f:
        f.write(TSA_EXTENSIONS)
    leaf(tag + "-tsa", "/CN=Tinker PDF Timestamping Authority/O=tinker-pdf test fixture",
         ("-algorithm", "RSA", "-pkeyopt", "rsa_keygen_bits:2048"),
         extfile=w(tag + "-tsa-ext.cnf"))
    with open(w(tag + "-tsa-serial"), "w") as f:
        f.write("2026100201\n")
    with open(w(tag + "-tsa.cnf"), "w") as f:
        f.write("\n".join([
            "[ tsa_config ]",
            "serial = " + w(tag + "-tsa-serial"),
            "signer_cert = " + w(tag + "-tsa-leaf.pem"),
            "signer_key = " + w(tag + "-tsa-leaf.key"),
            "certs = " + w(tag + "-tsa-root.pem"),
            "signer_digest = sha256",
            "default_policy = 1.3.6.1.4.1.55555.1.1",
            "digests = sha1, sha256, sha384, sha512",
            "accuracy = secs:1, millisecs:500, microsecs:100",
            "clock_precision_digits = 0",
            "ordering = yes",
            "tsa_name = yes",
            "ess_cert_id_chain = no",
            "ess_cert_id_alg = " + ess_algorithm,
            "",
        ]))


def timestamp(tag, digest):
    """An RFC 3161 `TimeStampToken` over the SHA-256 `digest`, from `tsa(tag)`:
    a query asking for the certificate, and a reply written as the bare token."""
    run("openssl", "ts", "-query", "-digest", digest.hex(), "-sha256", "-cert",
        "-out", w(tag + "-query.tsq"))
    run("openssl", "ts", "-reply", "-config", w(tag + "-tsa.cnf"), "-section", "tsa_config",
        "-queryfile", w(tag + "-query.tsq"), "-token_out", "-out", w(tag + "-token.der"))
    with open(w(tag + "-token.der"), "rb") as f:
        return f.read()


def header(data, at):
    """A DER node's tag, content length and header length at `at`."""
    tag = data[at]
    first = data[at + 1]
    if first < 0x80:
        return tag, first, 2
    count = first & 0x7F
    return tag, int.from_bytes(data[at + 2:at + 2 + count], "big"), 2 + count


def length_octets(n):
    if n < 0x80:
        return bytes([n])
    body = n.to_bytes((n.bit_length() + 7) // 8, "big")
    return bytes([0x80 | len(body)]) + body


def children(data):
    """The (start, end) of each node directly inside `data`, a content."""
    at, out = 0, []
    while at < len(data):
        _, length, size = header(data, at)
        out.append((at, at + size + length))
        at += size + length
    return out


def append_to_last(node, depth, extra):
    """`node` with `extra` appended to the content of its last child `depth`
    levels down, every length on the way re-encoded. Works because the
    `SignerInfo` is the last node of every container that holds it, and its
    `unsignedAttrs` the last field of the `SignerInfo` -- so nothing after the
    insertion point moves."""
    tag, length, size = header(node, 0)
    content = node[size:size + length]
    if depth == 0:
        content = content + extra
    else:
        start, end = children(content)[-1]
        content = content[:start] + append_to_last(content[start:end], depth - 1, extra)
    return bytes([tag]) + length_octets(len(content)) + content


def last_signer(cms):
    """The `SignerInfo` of a `ContentInfo`: four last-child steps down."""
    node = cms
    for _ in range(4):
        tag, length, size = header(node, 0)
        content = node[size:size + length]
        start, end = children(content)[-1]
        node = content[start:end]
    return node


def signature_value(cms):
    """The `signature` OCTET STRING's content inside the `SignerInfo`."""
    signer = last_signer(cms)
    _, length, size = header(signer, 0)
    content = signer[size:size + length]
    for start, end in children(content):
        tag, inner, inner_size = header(content, start)
        if tag == 0x04:
            return content[start + inner_size:end]
    raise SystemExit("no signature value")


TIMESTAMP_TOKEN_OID = bytes.fromhex("060B2A864886F70D010910020E")


def with_timestamp(cms, token):
    """`cms` with `token` as its signer's `id-aa-timeStampToken` unsigned
    attribute (RFC 3161 Appendix A)."""
    values = bytes([0x31]) + length_octets(len(token)) + token
    attribute = TIMESTAMP_TOKEN_OID + values
    attribute = bytes([0x30]) + length_octets(len(attribute)) + attribute
    unsigned = bytes([0xA1]) + length_octets(len(attribute)) + attribute
    return append_to_last(cms, 4, unsigned)


def signature_timestamp():
    """A detached `adbe.pkcs7.detached` signature countersigned by an RFC 3161
    token in its unsigned attributes: the token's imprint is SHA-256 of the
    signer's `signature` octets (RFC 3161 Appendix A), and the authority writes
    RFC 2634's first-version `signingCertificate`."""
    tag = "sigts"
    rsa_chain(tag, "Timestamped Signature")
    tsa(tag, "sha1")
    reserve = 9000
    out, contents_at, covered = build_pdf(
        reserve, "adbe.pkcs7.detached", "A signature with an RFC 3161 timestamp",
        "Tinker PDF Timestamped Signature Test Signer")
    der = cms_sign(tag, covered, "-md", "sha256", "-nosmimecap")
    token = timestamp(tag, hashlib.sha256(signature_value(der)).digest())
    der = with_timestamp(der, token)
    save("signature-timestamp.pdf", splice(out, contents_at, reserve, der))
    save("signature-timestamp-root.der", der_of(w(tag + "-root.pem")))
    save("signature-timestamp-tsa-root.der", der_of(w(tag + "-tsa-root.pem")))


BUILDERS = {
    "rsa-pss": rsa_pss,
    "pkcs7-sha1": lambda: pkcs7_sha1(True),
    "pkcs7-sha1-no-attributes": lambda: pkcs7_sha1(False),
    "no-signed-attributes": no_signed_attributes,
    "cades-general-names": cades_general_names,
    "signature-timestamp": signature_timestamp,
}

for wanted in WANTED:
    BUILDERS[wanted]()
