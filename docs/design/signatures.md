# Digital signatures

When this is done, opening a signed PDF reports what every signature actually proves — which
bytes it covers, whether the digest over them holds, whether the CMS signature verifies against
the signer's certificate, how far the certificate chain gets toward a host-supplied trust
anchor, and what changed after signing, classified against `/DocMDP` and `/FieldMDP` rules —
and `DocumentEditor` can produce a signature of its own on an incremental save, with the
private key held by a caller-supplied signer callback so key material never enters the engine.
Milestone 1 has landed: `Document::signatures()` (`crates/tinker-pdf/src/signature.rs`) finds
every signature, classifies what its `/ByteRange` covers against the file, and digests the
covered spans. Nothing verifies anything yet. `SignaturePlaceholder` in
`crates/tinker-pdf-cos/src/write.rs` is still a dead struct with no producer or consumer, and
the CMS blob is still an opaque byte string.

## What milestone 1 measured, which changed this document

Three things the corpus said that this design had not.

**A signature is not always the `/V` of a field.** 12.7.4.5 says it is, and a reader that
believes only that finds 13 of the 18 signatures in the fetched corpora. The other five are
reachable only through the catalog's `/Perms` (12.8.4): three files carry a `/UR3` usage-rights
signature and no signature field at all, and one has a `/FT /Sig` annotation in a document with
no `/AcroForm`. So the inventory walks both roots and every entry records which one found it —
a usage-rights signature grants a reader capabilities and makes no claim about the document's
content, and reporting the two the same way would say that it does.

**Producers disagree about whether `/Contents`' angle brackets are inside the gap.** Seven of
the nine well-formed signed files put `<` and `>` inside the two `/ByteRange` spans' gap; one
puts them outside, so the signature covers its own delimiters. Both are read, and the second is
named by a warning, because the difference is two bytes and two bytes inside or outside a
digest is a different digest.

**The `%%EOF` end-of-line marker straddles the revision boundary.** `Revision::byte_range` stops
just past `%%EOF`; a signer who included 7.5.5's trailing EOL covers one byte more.
`prefilled_f1040.pdf` covers to 299 340 where its revision ends at 299 339, and without a
tolerance of exactly those EOL bytes an ordinary sign-then-fill document reads as suspicious.
It was the only `Coverage::Revision` in the corpus, so the tolerance is the difference between
that classification being exercised by real data and not at all.

One trap, recorded because it produced a wrong reading before it produced a right one: an
encrypted document's object streams do not decompress until the file key exists, so a signature
inside one is invisible until the caller authenticates. Two corpus files behave that way, and
the first reading of one of them looked exactly like a producer merging the signature dictionary
into the field object. It does not; it was simply unread. `Anchor::MergedField` survives as a
defensive branch that **no corpus file needs**, held up by a fixture and by that sentence.

## Scope

- **Read: byte-range digesting (12.8.1).** Parse the signature dictionary — `/ByteRange`,
  `/Contents`, `/SubFilter`, `/M`, `/Reason` — from every populated `FieldKind::Signature`
  field **and from the catalog's `/Perms`**, which is the only route to five of the eighteen
  (see the measurement above). Digest the named ranges over the *stored* bytes (the
  `stream_raw_encrypted` forensic tier and the raw file buffer, per
  [opening](../features/opening.md)), and classify coverage: whole file minus the `/Contents`
  gap, or a prefix with later revisions on top. **Done.**
- **Read: CMS and X.509 parsing.** DER (X.690) walker; CMS `SignedData` (RFC 5652) including
  signed attributes and `messageDigest`; X.509 certificates (RFC 5280). `SubFilter`s
  `adbe.pkcs7.detached` and `adbe.pkcs7.sha1` (ISO 32000-1 12.8.3.3) plus
  `ETSI.CAdES.detached` (ISO 32000-2, CAdES subfilter clause); `adbe.x509.rsa_sha1` parsed
  and reported, verified only if the corpus says it still matters (ruling 3).
- **Read: signature verification, hand-rolled, verify-only.** RSASSA-PKCS1-v1_5 (RFC 8017)
  and ECDSA over P-256/P-384 (FIPS 186-4) verification in `tinker-pdf-crypto`, gated on
  published vectors exactly as the existing AES/SHA code is. Digests are the crate's existing
  `sha256`/`sha384`/`sha512` (RFC 5754 names them for CMS); SHA-1 accepted for legacy
  signatures and flagged as weak in the verdict.
- **Read: modification detection (12.8.2.2, 12.8.2.4).** A signature covering revision *N*
  plus later revisions from `CosDocument::revisions()` yields the set of objects the later
  revisions touched; classify them against the `/DocMDP` `/P` level and `/FieldMDP` field
  lists, producing a typed answer, not a boolean.
- **Write: sign on incremental save.** `DocumentEditor::save` in `WriteMode::Incremental`
  grows the seam plan 09 sketched: reserve `/Contents` and `/ByteRange` (revive
  `SignaturePlaceholder` as the producer's record), patch `/ByteRange` after layout, hand the
  range digest to a caller-supplied `Signer`, hex-patch the returned CMS into the gap.
- **Trust model.** The engine verifies chain *shape* — signatures along the chain, validity
  windows, basicConstraints, keyUsage — up to anchors the host supplies as DER bytes, the
  same inversion as `FontProvider` (`crates/tinker-pdf/src/fonts.rs`): the engine computes,
  the host decides what to trust. No bundled root store.
- **Facade.** `Document::signatures()` returning per-signature verdict structs, projected 1:1
  through the FFI per ruling 11; typed warnings with provenance per ruling 10.

## Non-goals

- **Signing keys in the engine.** No key parsing (PKCS#8, PKCS#12), no key generation, no
  RSA/ECDSA *signing* arithmetic. The `Signer` callback returns finished CMS bytes.
- **Revocation fetching.** No CRL or OCSP network traffic — the engine performs no I/O.
  Embedded revocation data (PAdES DSS/VRI, ISO 32000-2 12.8.4.3) is parsed and surfaced;
  evaluating freshness is the host's call.
- **Timestamp validation.** RFC 3161 tokens inside CMS are parsed and reported (present,
  TSA name, time); validating the TSA's own chain is a later tier, not this design.
- **Long-term validation profiles.** PAdES-LTA conformance levels are out; the verdict
  reports what is embedded, nothing more.
- **The public-key security handler** (`/Filter /Adobe.PPKLite` encryption, 7.6.5) — related
  ASN.1, different feature.
- **Visible signature appearance generation.** A signed field keeps whatever appearance the
  caller set via the existing forms path; drawing seals is not signature work.

## Design

**What already exists, and is load-bearing.** Three seams built earlier were built for this.
First: `incremental_update` (`crates/tinker-pdf-cos/src/write.rs`) appends after the
original bytes, adding a newline only when the base lacks one, and the test
`an_incremental_save_keeps_the_original_bytes` (`crates/tinker-pdf-cos/src/edit.rs`) asserts
`starts_with(original)` — "the signable prefix must survive an edit" is already a committed
assertion, so the byte-identical prefix a signature needs is not new work. A second, older
assertion of the same invariant sits in `write.rs` as
`an_incremental_update_preserves_the_original_bytes_exactly`, whose doc comment already reads
"The invariant phase 10's signing depends on"; milestone 1's revision-coverage fixture is now
a third, and the only one that checks what the *reader* makes of the result. Second: each
`Revision` (`crates/tinker-pdf-cos/src/xref.rs`) carries the byte range a signature
covers, recorded at open per 7.5.6 — MDP analysis is a consumer of existing data, not new
parsing. Third: `tinker-pdf-crypto` already holds SHA-256/384/512 and the published-vector
merge gate, and its `EntropySource` trait is the precedent for hosts supplying what the
engine refuses to own.

**A new leaf crate: `tinker-pdf-pki`.** DER, X.509 and CMS parsing go in a new leaf crate —
bytes in, values out — not into `tinker-pdf-crypto`. The argument: the crypto crate is
*arithmetic* whose failure mode is a wrong number caught by published vectors; ASN.1 parsing
is *untrusted-input structure walking* whose failure mode is a panic or an overread on
malformed bytes, exactly the class ruling 1's per-format fuzzers exist for. Merging them
would put a large attack surface inside the crate whose review story is "small, vector-gated
arithmetic", and would leave DER parsing fuzzable only through crypto's API. As its own leaf,
`tinker-pdf-pki` gets a dedicated fuzz target over raw DER, its own test corpus of
certificates, and independent publishability. It has no PDF vocabulary (X.509 and CMS are not
PDF concepts), satisfying ruling 8's definition, and its one edge — `pki → crypto` for digest
and signature-verify primitives — is a leaf-to-leaf edge like `font → filters`, which ruling 8
explicitly permits. The alternative (parsing in the facade) fails ruling 8's spirit in the
other direction: it would weld protocol parsing to COS types for no gain. The crate exposes:
a DER walker (definite lengths only — DER forbids indefinite; depth-capped; never panics),
`Certificate` (subject/issuer as compared-by-DER blobs plus decoded fields, SPKI, validity,
extensions), `SignedData` (digest/signature algorithm identifiers, signed attributes with
their exact DER for re-digesting, certificates, signer identifier), and chain-shape checking
against caller-passed anchor certificates. Unknown algorithm OIDs return typed refusals.

**Verify math in `tinker-pdf-crypto`.** A constant-size big-unsigned type (stack arrays, no
allocation in hot paths) with the operations verification needs: modular exponentiation for
RSA, and P-256/P-384 field/scalar arithmetic with Jacobian point math for ECDSA. Verify-only
keeps it honest: no private-key operations means no constant-time obligations beyond what the
existing password comparison already set, though modular ops are written branchless-by-value
anyway. Gates: FIPS 186-4 / NIST CAVP known-answer vectors and RFC 8017's worked examples as
merge requirements, mirroring the FIPS 197 / RFC 6229 gates the crate's existing code cites.
EMSA-PKCS1-v1_5 decoding compares the full expected encoding byte-for-byte against the
decrypted block — the strict comparison that forecloses Bleichenbacher-style forgery under
low exponents — and ECDSA rejects `r`, `s` outside `[1, n-1]` before any curve math.

**The verdict is a report, not a bool.** Per signature, the facade returns: coverage
(`WholeFile` — the two ranges bracket exactly the `/Contents` gap and reach EOF — or
`Revision { index }`, or `Suspicious` with a typed reason when ranges overlap the gap
wrongly, the territory 12.8.1's byte-range notes warn about); digest match; CMS signature validity including the signed
`messageDigest` attribute cross-check; chain result (`AnchoredTo { anchor }`, `SelfSigned`,
`Incomplete`, `Expired`, per-link); weak-algorithm flags; and the MDP classification below.
Nothing is collapsed: "digest holds but the chain reaches no anchor" is the everyday case
and the host renders it, exactly as `Bitmap.warnings` consumers do under ruling 2. Anything
unparseable or unsupported becomes a typed verdict variant with provenance (rulings 2, 10),
never an `Err`, never a panic (ruling 1) — a signed file must still open, render, and extract.

**MDP over revisions.** For a signature covering `Revision` *k*: reparse the document
prefix `&bytes[..revisions()[k].byte_range.end]` as its own `CosDocument` (the machinery
already accepts arbitrary buffers), then diff object numbers written by revisions after *k*
against the prefix. `/DocMDP` `/P` (12.8.2.2): level 1 permits nothing; 2 permits form fill
and signing; 3 adds annotations — classify each touched object as form-field value, signature
field, annotation, or other, reusing `FieldKind` classification from `form.rs`. `/FieldMDP`
(12.8.2.4) narrows to the named fields under its `/Action`. The output is
`Unmodified | PermittedChanges(list) | DisallowedChanges(list)` with object provenance.

**Signing.** `WriteOptions` grows a signing request: the field to sign (or a new invisible
one), the signature dictionary values, a gap size, and a `Signer`. The trait is two calls:
`digest_alg()` naming the digest, and `sign(&[u8]) -> Result<Vec<u8>, SignRefused>` taking
the byte-range digest and returning DER CMS. `incremental_update` gains the reserve-and-patch
step plan 09 specified — `0`-filled hex gap, fixed-width space-padded `/ByteRange` slots
patched after layout, `SignaturePlaceholder` finally earning its fields as the internal
record of where to patch. A CMS larger than the gap is a typed refusal, not a truncation.
Output stays deterministic given the signer's bytes: same inputs, same file, byte-identical
(the determinism contract, ruling 4, extended to written bytes as plan 09 already treats it).

**Verification, and the one place ruling 13 costs the most.** The primitives are gated by
published test vectors, which is data and the strongest evidence available: NIST CAVP RSA and
ECDSA verify vectors, RFC 8017's worked example, RFC 5652 fixture DER. Read-side ground truth
is a committed corpus of signed fixtures — valid, tampered-after-signing, expired-chain —
each with an expected-verdict sidecar asserted in `cargo test`. Fuzzers: raw DER into
`tinker-pdf-pki`, and whole signed PDFs into the verdict path. Written signatures are checked
by the strict structural validator, and every signing test asserts the
`starts_with(original)` prefix invariant and independently re-digests the returned
`/ByteRange` spans.

**What none of that establishes:** that anyone else accepts the signature. Under ruling 13 no
CI job may ask a validator, so:

> A signature everything in-tree accepts may still be rejected by real validators.

Interop is therefore a *dated, recorded, one-time measurement* performed outside CI and
written into this document when the capability lands — the same class of evidence as the
committed JPEG 2000 reference decodes: a measurement, not a check. It does not run again, it
does not gate a merge, and it goes stale. Nothing here hides that, because a signing feature
whose interop claim is unverified and unstated is worse than one that says so.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
|---|-------------|-------------------------------------|-----------------|
| 1 | Signature inventory: `/ByteRange`/`/Contents` parsing, range digesting, coverage classification | `Document::signatures()` lists every signature in the fixture corpus with correct coverage; a flipped byte inside a covered range flips the digest verdict in a unit test; fuzzer on the parse path runs crash-free in CI | M |
| 2 | `tinker-pdf-pki` DER walker + X.509 | Parses every certificate in the fixture corpus to the subject/issuer/validity/SPKI values committed in its sidecar, transcribed once from the certificate's own DER and reviewed; RFC 5280's own example certificates parse; dedicated fuzz target in the fuzz workspace; depth-capped, zero panics | M |
| 3 | CMS `SignedData` parsing incl. signed attributes | RFC 5652 fixture set round-trips to expected values; `messageDigest` attribute extracted and re-digestable from exact DER; unknown OIDs yield typed refusals asserted by test | M |
| 4 | Big-unsigned + RSASSA-PKCS1-v1_5 verify in `tinker-pdf-crypto` | NIST CAVP RSA verify vectors (2048/3072/4096, SHA-256/384/512) pass as `cargo test` merge gate; forged-padding vectors rejected; RFC 8017 worked example passes | M |
| 5 | ECDSA P-256/P-384 verify | CAVP ECDSA verify vectors pass, including invalid-`r`/`s` and wrong-curve rejections; point-not-on-curve certificates refused with typed verdict | M |
| 6 | End-to-end verdicts + trust anchors | Corpus of signed fixtures (valid, tampered, expired, self-signed) each matches its committed expected-verdict sidecar; anchor supplied → `AnchoredTo`, withheld → `SelfSigned`/`Incomplete`, asserted per fixture | M |
| 7 | `/DocMDP` + `/FieldMDP` via `revisions()` | Fixtures: form-fill after certification level 2 → `PermittedChanges`; page edit after level 1 → `DisallowedChanges` naming the object; `/FieldMDP`-locked field edit detected; all as `cargo test` assertions | M |
| 8 | Sign on incremental save: seam + `Signer` callback | Every signing test asserts `starts_with(original)`; independently re-digesting the returned `/ByteRange` spans matches the digest handed to the `Signer`; the signed file re-opens and verifies through this engine's own read side, and passes the strict structural validator; oversized CMS → typed refusal test | L |
| 9 | Facade + FFI projection, warnings, docs | Verdict types exposed 1:1 through `tinker-pdf-ffi` (ruling 11) with parity tests; typed warnings carry object provenance (ruling 10) pinned by fixture; [features/forms.md](../features/forms.md) gains a signature-fields section; roadmap row closed against [ROADMAP.md](../ROADMAP.md) | M |

## Dependencies

- **Existing:** `incremental_update` and the `starts_with(original)` invariant
  (`tinker-pdf-cos/src/write.rs`, `edit.rs`); `Revision.byte_range` from open
  (`xref.rs`, [opening](../features/opening.md)); `FieldKind::Signature` and field
  classification (`form.rs`); SHA-2 digests and the published-vector convention
  (`tinker-pdf-crypto`); the strict structural validator
  ([verification](../verification.md)); rulings 1, 2, 8, 10, 11, 13
  ([rulings](../rulings.md)).
- **New:** `tinker-pdf-pki` leaf crate (milestones 2–3) before verdict assembly (6);
  crypto verify math (4–5) before 6; milestone 1 is independent and can land first;
  the write side (8) depends only on 1 and the existing incremental writer, so it can
  proceed in parallel with 4–7.
- **Published data, not programs (ruling 13):** NIST CAVP verify vectors, RFC 8017 and
  RFC 5652 examples, RFC 5280 sample certificates; signed-fixture corpus committed to the
  test tree with expected-verdict sidecars.

## Risks

| Risk | Mitigation |
|------|------------|
| Hand-rolled RSA/ECDSA verify accepts a forgery (padding laxity, missing range checks) | Verify-only scope; CAVP negative vectors and forged-padding cases as merge gates, mirroring the crate's FIPS 197/RFC 6229 precedent; full-encoding comparison for EMSA-PKCS1-v1_5 rather than a prefix match. The negative vectors carry this alone under ruling 13, which is why every published invalid case is a gate rather than a sample |
| ASN.1 parser panics or overreads on malformed DER (largest new untrusted surface) | Own leaf crate with dedicated fuzz target from milestone 2's first commit; depth caps and definite-length-only parsing; ruling 1 makes a fuzz crash a release blocker |
| A verdict rendered as a single "valid" boolean misleads hosts into overtrusting | The API has no boolean: coverage, digest, chain, and MDP are separate typed fields, and the docs state the engine proves integrity, not identity — anchors are the host's assertion |
| Byte-range trickery: ranges that skip more than the `/Contents` gap make a "valid" signature over chosen bytes | Coverage classification is computed from the ranges, never trusted from them; anything but exact-gap bracketing to EOF or a clean revision boundary is `Suspicious` with a typed reason, fixture-pinned |
| MDP misclassification calls a benign form fill a disallowed change (or the reverse) | Classification reuses the tested `form.rs` field machinery rather than re-deriving object roles; fixtures for each `/P` level in both directions, each fixture's expected verdict written from the clause rather than from a run |
| Signing seam drifts from the prefix invariant and silently invalidates what it signs | `starts_with(original)` asserted in every signing test without exception (already the incremental writer's rule); the `/ByteRange` digest re-computed independently in tests |
| **Nobody outside this repository ever validates a signature it produced** (ruling 13), so this engine can sign confidently and wrongly | The primitives are pinned by published vectors, which is the part most likely to be subtly wrong. Assembly and placement are not adjudicated by anyone; interop is a dated one-time measurement recorded in this document, and the sentence about real validators stands in the feature doc as a permanent limit. **Not closed** |
