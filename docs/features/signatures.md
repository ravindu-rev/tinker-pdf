# Digital signatures

Both directions, with the cryptography hand-rolled and verify-only. Opening a
signed document reports what each signature actually proves — which bytes it
covers, whether those bytes still hash to what was signed, whether the
signature was made by the key in the signer's certificate, how far the
certificate chain reaches toward an anchor the *caller* supplied, and what
later revisions changed measured against the signature's own `/DocMDP`.
`DocumentEditor` produces signatures of its own on an incremental save, with
the private key held by a caller-supplied callback so key material never
enters the engine.

Every primitive is the project's own: DER, X.509 and CMS in the
`tinker-pdf-pki` leaf crate, and big-integer arithmetic, RSASSA-PKCS1-v1_5 and
ECDSA in `tinker-pdf-crypto` beside the ciphers ([encryption](encryption.md)).

**There is no boolean.** A verdict answers four questions separately, because
they come apart in practice: four documents in the fetched corpora carry a
signature that *verifies* over bytes the document no longer *has*, and no
single flag can say that.

## What it does

**Finding them.** `Document::signatures()` walks two roots. 12.7.4.5 says a
signature is the `/V` of a `/FT /Sig` field, and believing only that finds 13
of the 18 signatures in the fetched corpora; the other five are reachable only
through the catalog's `/Perms` (12.8.4). Each entry records which root found
it, because a `/UR3` usage-rights signature grants a reader capabilities and
makes no claim about the document's content.

**Coverage, classified rather than reported.** `/ByteRange` is four numbers a
producer wrote. Every span is checked against the file it claims to describe:
`WholeFile`, `Revision` for a signature a later incremental update was layered
on (7.5.6), or `Suspicious` with the reason named — a range past the end of
the file, spans that overlap, a gap that lands in an XMP packet rather than on
`/Contents`. Of the corpus's 18: **11 whole-file, 1 over a revision, 6
suspicious**, each of the six checked by hand against the file's bytes.

`/Contents` is read from the gap between the spans rather than from the object
model. That is the only reading that can disagree with a lying `/ByteRange` —
and in an encrypted document the object model returns a *decrypted* string
where a signature covers the bytes as stored.

**CMS and certificates.** `tinker-pdf-pki` reads RFC 5652 `SignedData`: both
`SignerIdentifier` shapes, signed and unsigned attributes, `contentType`,
`messageDigest`, `signingTime`, ESS `signingCertificateV2`, and RFC 3161
timestamp tokens — surfaced, never evaluated. RFC 5652 §5.4's re-encoding (the
stored `[0] IMPLICIT` tag replaced by `SET OF` before digesting) lives in one
function and is **adjudicated by data**: 19 real signatures from six producers
verify with the substitution and not one verifies without it.

`SignedData` is read as BER, which RFC 5652 §5.1 permits and a fifth of the
corpus's signed documents need — Acrobat Distiller, Adobe LiveCycle and
LibreOffice all emit indefinite lengths. The scan **walks** to the
end-of-contents pair rather than searching for `00 00`, because those two
bytes occur constantly inside real content and searching for them produces a
different well-formed reading of the same bytes. `signedAttrs` is held to DER
regardless, because that is what gets digested.

All 41 X.509 certificates in the corpus parse.

**The verdict.** `Document::verify_signatures(&anchors, at)` returns one
`Verdict` per signature: the coverage, whether the CMS could be read, whether
the `messageDigest` equals a digest recomputed over the covered bytes, whether
the signature verifies against the signer's key, how far the chain reached,
and the weaknesses accepted along the way. Every check that did not run says
*why* rather than reporting a failure — "we did not look" and "we looked and
it was wrong" are the two answers a caller must never confuse.

**Modification detection.** `Signature::modifications()` lists every object a
revision after the signed bytes wrote, classifies it, and marks it against the
signature's `/DocMDP` level (12.8.2.2) and `/FieldMDP` field lock (12.8.2.4).
It compares **cross-reference entries** rather than object values: 7.5.6 says
an update writes a new entry for exactly the objects it changed, so a
differing entry is a changed object — and it needs no decryption key, where a
value diff would report every object of an encrypted document as changed.

**Writing.** `DocumentEditor::save_signed` reserves the `/Contents` gap,
finishes the file, patches `/ByteRange` to describe what it finished, digests
the covered spans and hands the digest to a `Signer` that returns finished CMS
bytes. It can certify the document at any of 12.8.2.2's three levels and lock
fields per 12.8.2.4. The writer and the reader share one `digest_spans`, so
what is signed and what is checked cannot drift.

## API

```rust
let document = Document::open(bytes)?;

for signature in document.signatures() {
    signature.coverage;                 // WholeFile | Revision | Suspicious
    signature.field;                    // the field's qualified name, if any
    signature.anchor;                   // which root found it
    signature.certification;            // /DocMDP, if it certifies
    signature.cms();                    // the DER, with the reservation trimmed
    signature.digest(&document, alg);   // over the covered spans
    signature.modifications(&document); // what came after, classified
}

let mut anchors = TrustAnchors::new();
anchors.add(root_certificate_der)?;     // the caller says what it trusts
for verdict in document.verify_signatures(&anchors, Some(now)) {
    verdict.document_digest;            // Matches | Differs | NotChecked(why)
    verdict.signature;                  // Verified | Failed | NotChecked(why)
    verdict.chain;                      // AnchoredTo | SelfSigned | Incomplete | …
    verdict.weaknesses;                 // SHA-1, short keys, coverage
}
```

Signing takes a `Signer` the host implements — two calls, `digest_algorithm`
and `sign(&[u8]) -> Result<Vec<u8>, SignRefused>` — and a `SigningRequest`
naming where the signature goes, how much space to reserve, and what to
certify.

## Refused by name

| What | Typed variant | Why (one line) | See |
| --- | --- | --- | --- |
| Private-key operations of any kind | none offered — `Signer` returns finished CMS | no key parsing, no key generation, no signing arithmetic; the engine never holds key material | [design](../design/signatures.md) |
| A bundled root store | `Chain::NoAnchors` when the caller supplies none | which certificates to trust is a policy, and a library that ships one has made the caller's decision for them | this page |
| Deciding whether a certificate is expired, unasked | validity reported; judged only against a caller-supplied instant | ruling 4 bans a clock, and "expired" is a claim about *now* — a library that invents one answers differently on different days | [rulings](../rulings.md) ruling 4 |
| CRL and OCSP fetching | embedded revocation data surfaced, never evaluated | the engine performs no I/O; freshness is the host's call | [design](../design/signatures.md) |
| Validating an RFC 3161 timestamp | `SignerDescription::timestamped` says one is there | validating a token means validating the authority's own chain, which is a later tier | [design](../design/signatures.md) |
| ECDSA in a verdict | `Unchecked::UnsupportedAlgorithm` | implemented in `tinker-pdf-crypto` and gated on 120 CAVP vectors, but **zero corpus signatures use it**, and wiring an unexercised path into a verdict is worst here | [ROADMAP](../ROADMAP.md) |
| RSASSA-PSS | `SignatureAlgorithm::RsaPss`, named and not decoded | its parameters live in a structure this build does not read, so a caller meeting one knows what it is and knows nothing here has checked it | RFC 8017 |
| `adbe.pkcs7.sha1` (12.8.3.3.1) | `Unchecked::LegacySha1SubFilter` | deprecated in ISO 32000-2; one corpus file has it and that file is a fuzzer's output, so it is named rather than implemented on a sample of one | 12.8.3.3.1 |
| An indefinite length inside `signedAttrs` | `CmsError::IndefiniteSignedAttributes` | RFC 5652 §5.4 requires those bytes to be DER and they are what gets digested; BER is read everywhere else in a `SignedData`, and only here is it refused | RFC 5652 §5.4 |
| A signature with no signed attributes | `Unchecked::NoSignedAttributes` | the signature is then over the content directly, and guessing at what that content is would be a verdict about the wrong bytes | RFC 5652 §5.4 |
| Visible signature appearance generation | none — a signed field keeps whatever appearance the caller set | drawing seals is not signature work | [forms](forms.md) |
| The public-key **security handler** (`/Adobe.PubSec`, 7.6.5) | `AuthError::UnsupportedHandler` | related ASN.1, different feature — and zero corpus files ask for it | [encryption](encryption.md) |

## Verified

**Published vectors gate the arithmetic**, as they gate every other primitive
in this tree ([encryption](encryption.md)). **504 of them ran**: 360 NIST CAVP
`SigVer15` for RSA (60 valid and 300 that must be refused, moduli of 1 024 to
4 096 bits crossed with SHA-1/256/384/512, of which 150 are forged paddings),
120 CAVP ECDSA `SigVer` and 24 `PKV` for P-256 and P-384, and 16 from RFC
6979. CAVP's exponents are all large, so it never tests the low-exponent
forgery; hand-built negatives cover it by choosing a modulus that makes the
verifier recover any chosen block without a private key.

**One bug those vectors did not catch**, recorded because of how it hid:
Montgomery multiplication's conditional subtraction borrowed against the full
limb count while the carry borrowed against the used count, so every RSA key
below 4 096 bits came back correct in its low limbs and all-ones above them.
The inner loops read only low limbs, so it rode through an entire modular
exponentiation invisibly and would have surfaced as a *valid signature
reported invalid*, intermittently. All 360 CAVP vectors passed before the fix.

**The corpus adjudicates the parts it can.** Over the 18 signatures:
**12 CMS blobs parse, 11 signatures verify** against the key in their own
certificate, **0 fail**, 7 document digests match and **4 differ**. The four
that differ are veraPDF's permission fixtures, and the cause is in the bytes:
three of them, of different sizes, carry the byte-identical CMS blob. A
signature cannot cover three documents.

The six remaining are the ones whose `/ByteRange` does not bracket their
`/Contents`. They carry a readable blob and this build declines to hand it to
a parser, because reading a CMS the coverage classifier will not vouch for is
a verdict about the wrong bytes.

All 41 certificates across those blobs parse, and 15 of them are held to
values OpenSSL produced once and this repository committed
(`tests/signature_support/certificates.tsv`) — serial octets, validity window,
SubjectPublicKeyInfo digest and both common names.

Fixtures cover what the corpus cannot: a signature over a revision, a merged
field dictionary, both `/Contents` gap conventions, all four digest
algorithms, an oversized CMS refused rather than truncated, a signer that
declines, and — the shape the whole design exists for — one byte flipped
inside a signed range, which turns the document digest to `Differs` and leaves
the signature `Verified`.

**What this cannot establish, stated rather than absorbed.** Under ruling 13
nothing outside this repository has ever agreed that a signature this code
accepts is acceptable, or that one it rejects is not. The primitives are
gated on published vectors and RFC 5652 §5.4 is settled by real signatures
from six producers, but the assembly is this engine agreeing with itself.
**A signature everything in-tree accepts may still be rejected by a real
validator.** That is not closed, and the interop measurement the roadmap asks
for is a dated one-time record rather than a check.
