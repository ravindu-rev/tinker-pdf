# Signature fixtures

`certificates.tsv` is documented in its own header: expected values for the
X.509 certificates the fetched corpora carry, produced once by OpenSSL and
committed as a dated measurement.

## The ECDSA-signed documents

`ecdsa-p256.pdf`, `ecdsa-p256-root.der`, `ecdsa-p384.pdf` and
`ecdsa-p384-root.der` were built once, on **14 September 2026**, by
`ecdsa-fixtures.py` and **OpenSSL 3.5.5 (27 Jan 2026)**. They exist because
nothing else here can produce an ECDSA-signed PDF: of the 34 `SignerInfo`s and
71 certificates in the corpora `corpus/corpora.lock` pins, **not one uses
ECDSA** — every signer is RSASSA-PKCS1-v1_5 and every certificate is signed
with RSA. `crates/tinker-pdf/tests/cms_census.rs` measures that, and the
roadmap's Signatures row names it as the reason the verdict path had no ECDSA
arm until these files arrived.

Each PDF is one page, one invisible signature field, and one
`/SubFilter /adbe.pkcs7.detached` signature whose `/ByteRange` covers every
byte but the `/Contents` gap. Each root `.der` is the self-signed certificate
that issued that document's signer, and is what a test hands to
`TrustAnchors::add` — the engine ships no root store and never will, so the
anchor has to come from somewhere, and here it comes from the fixture.

### Who wrote which half, and what each half is worth

**OpenSSL wrote the cryptography.** The two key pairs, the two certificate
chains, the `SignedData`, the `signedAttributes` and the ECDSA signature over
their DER are all its work. Reading them exercises `tinker-pdf-pki` and
`tinker-pdf-crypto` against structures a second implementation produced, which
is real interop: RFC 3279 §2.2.3's `SEQUENCE { r INTEGER, s INTEGER }`, RFC
5480 §2.1.1's named curve and uncompressed point, RFC 5652 §5.4's re-encoded
`SET OF Attribute`, and an ECDSA certificate signature over a stored
`TBSCertificate`.

**`ecdsa-fixtures.py` wrote the PDF around it**, and that half is weaker
evidence. The script lays the objects out, computes the four `/ByteRange`
numbers, extracts the covered bytes and hands them to OpenSSL to digest;
`crates/tinker-pdf/src/signature.rs` recomputes the same spans when the file is
read back. Both are the same author's reading of ISO 32000-1 12.8.1, so a
matching `messageDigest` catches a transcription slip between them and cannot
catch a misreading of the clause. `crates/tinker-pdf/tests/pubsec.rs` says the
same about its own generator and for the same reason.

The script adjudicates nothing (ruling 13). What adjudicates is
`crates/tinker-pdf/tests/ecdsa_verdict.rs`, which opens the committed bytes
with this engine and asserts all four questions — and, for every one of the six
ways the file can be spoiled, that the answer is not `Verified`.

### Regenerating

```sh
python crates/tinker-pdf/tests/signature_support/ecdsa-fixtures.py \
    crates/tinker-pdf/tests/signature_support /tmp/ecdsa-work
```

**A regeneration does not reproduce these bytes**, and that is not a defect:
the script generates fresh key pairs and OpenSSL stamps its own `signingTime`,
so every run produces a different document that is equally valid. The committed
bytes are the fixture; `ecdsa_verdict.rs` hard-codes one fact about them (the
P-256 signature's `r` and `s` are both 32 octets, so they can be exchanged in
place without moving a length), and a regeneration has roughly a one-in-four
chance of needing that test adjusted. Nothing re-runs the script, and
`cargo xtask oracles` would refuse a test that tried.

## Signature shapes the corpus lacks: `signature-fixtures.py`

`signature-fixtures.py` builds the signed documents `tests/signature_shapes.rs`
holds the verdict to, one per shape the fetched corpora have too few of. It
was run once per fixture with **OpenSSL 3.0.13 (30 Jan 2024)**, on the date
given for each, and nothing re-runs it — `cargo xtask oracles` would refuse a
test that tried. It lays out the same one-page, one-field document
`ecdsa-fixtures.py` does, computes the `/ByteRange`, and splices OpenSSL's CMS
into the reservation; what each half of the result is worth is the same split
the ECDSA section above makes, and `signature_shapes.rs` states it again at the
top. A regeneration produces different bytes, because the keys are fresh each
time.

```sh
python3 crates/tinker-pdf/tests/signature_support/signature-fixtures.py \
    crates/tinker-pdf/tests/signature_support /tmp/sig-work <fixture>...
```

### `rsa-pss.pdf` and `rsa-pss-root.der` — 2 October 2026

`python3 signature-fixtures.py <out> <work> rsa-pss`. A 2048-bit
`rsaEncryption` root, self-signed with RSASSA-PSS (`-sigopt
rsa_padding_mode:pss -sigopt rsa_pss_saltlen:32`); a leaf whose key is an
`id-RSASSA-PSS` key restricted to SHA-256, MGF1-SHA-256 and a salt of at least
32 (`genpkey -algorithm RSA-PSS` with the three `rsa_pss_keygen_*` options),
issued by the root with the same PSS options; and `openssl cms -sign -binary
-md sha256 -keyopt rsa_padding_mode:pss -keyopt rsa_pss_saltlen:32
-nosmimecap` over the covered bytes, the root included with `-certfile`.

SHA-256: `12bcac5f6ea4f3b005a6d5988d250f4527cd35b08cc5ff577ff4fe86cfd265a7`
(`rsa-pss.pdf`) and
`14f165359b23208b1f609f4de15692848a4e9ffbd7bd22d8fe1c06e815e13e7e`
(`rsa-pss-root.der`). The CMS blob inside the PDF, without its zero fill, is
also committed as the fuzz seed `fuzz/corpus/pki_cms/rsa-pss-signer`.

It exists because **no signature in the fetched corpora uses RSASSA-PSS**:
every `SignerInfo` is PKCS#1 v1.5. The PSS arithmetic is held to NIST CAVP's
and RSA Laboratories' published vectors in `tinker-pdf-crypto`; this file is
what carries a PSS signature, a PSS-restricted key and a PSS certificate
signature that a second implementation produced into a whole document.

### `no-signed-attributes.pdf` and `no-signed-attributes-root.der` — 2 October 2026

`python3 signature-fixtures.py <out> <work> no-signed-attributes`. A 2048-bit
RSA root and a leaf it issues, both PKCS#1 v1.5 with SHA-256, and `openssl cms
-sign -binary -md sha256 -noattr` over the covered bytes: a detached signer
with no `signedAttrs`, so RFC 5652 §5.4's signature is over SHA-256 of the
covered bytes themselves and its `signatureAlgorithm` is bare `rsaEncryption`.

SHA-256: `87800983dc430112cefa5060974a7172f51f37c1d9d07b2e5f04840f00b05f5d`
(`no-signed-attributes.pdf`) and
`d3df219c37f19604fe97cc19e80895f49a5fe9ae3b99cb641ca17934895359af`
(`no-signed-attributes-root.der`).

The corpus has exactly one signer of this shape — `bug854315.pdf`'s, per
`cms_census.rs` — and one is not enough to say the arm is right in both
directions; this file is the second, and the one whose negatives can be made.

## The visible-signature signer

`visible-signer-key.der` and `visible-signer.der` are a throwaway 2048-bit RSA
key (PKCS#1 `RSAPrivateKey`, DER) and the self-signed certificate over it,
generated once on **26 September 2026** by **OpenSSL 3.0.13 (30 Jan 2024)**:

```sh
openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem \
    -days 36500 -subj "/CN=tinker-pdf visible signature fixture/O=tinker-pdf/C=GB" \
    -set_serial 2026092601 -sha256
openssl rsa -in key.pem -traditional -outform DER -out visible-signer-key.der
openssl x509 -in cert.pem -outform DER -out visible-signer.der
```

SHA-256: `a14845674594223bfa2fd6429ca357b0cc119412a565623fe8b852ec5141d291`
(key) and `a4e5057fcbff906f61ef46dcc1c38251abaa68e4d362147a88302b2e07ed4c5e`
(certificate).

**Why a private key is committed at all.** `crates/tinker-pdf/tests/visible_signature.rs`
has to show that a signature over a document carrying a drawn seal *verifies*,
and the digest it signs exists only once this engine has laid the file out —
so no signature can be made ahead of time, and no test may spawn OpenSSL to
make one then (ruling 13). The test therefore signs in-process: it assembles
the CMS `SignedData` itself and raises the PKCS#1 v1.5 block to this key's
private exponent with `tinker_pdf_crypto::bignum`. The key signs nothing but
test documents and protects nothing; it is fixture data in the sense that a
committed JPEG is.

What OpenSSL's half is worth: the key pair and the certificate are a second
implementation's, so a certificate this engine could not read, or an `e` and
`d` that were not inverses, would fail the test rather than agree with it.
What it is not worth: the `SignedData` around the signature is this
repository's own reading of RFC 5652, checked by this repository's own
verifier.
