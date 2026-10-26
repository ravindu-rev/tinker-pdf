# Digital signatures

Opening a signed PDF reports what every signature actually proves — which bytes it covers,
whether the digest over them holds, whether the CMS signature verifies against the signer's
certificate, how far the certificate chain gets toward a host-supplied trust anchor, and what
changed after signing, classified against `/DocMDP` and `/FieldMDP` rules — and
`DocumentEditor` can produce a signature of its own on an incremental save, with the private
key held by a caller-supplied signer callback so key material never enters the engine.
`Document::signatures()` finds every signature and classifies what its `/ByteRange` covers;
`tinker-pdf-pki` reads DER, X.509 and CMS `SignedData`; `tinker-pdf-crypto` verifies RSA —
PKCS#1 v1.5 and PSS — and ECDSA, gated on published vectors; `Document::verify_signatures()`
assembles the four answers; `Signature::modifications()` measures later revisions against
`/DocMDP`; and `DocumentEditor::save_signed` produces signatures of its own, certifying and
locking fields, with the key held by the caller. What holds the two ends together is that the
writer and the reader call one `digest_spans`, so what is signed and what is checked cannot
drift.

`tests/signature_support/certificates.tsv` records serial, validity, SubjectPublicKeyInfo
digest, self-issued flag and both common names for corpus certificates, produced by OpenSSL
and committed with the commands and the version in its header; `tests/certificates.rs` holds
the parser to it. Ruling 13 permits exactly that — a third-party program may supply data.
Producing the values with `tinker-pdf-pki` would be the parser agreeing with itself, which is
the one thing a sidecar must not be.

Some corpus serials have their top bit set with no leading zero, so the `INTEGER` *is*
negative in DER — RFC 5280 §4.1.2.2 says a serial "MUST be a positive integer" and those
issuers emitted one that is not. OpenSSL reads the number as negative; so does this crate. The
sidecar records the octets instead, because that is what `issuerAndSerialNumber` matching
compares — the certificate's identity rather than either side's rendering of its value.

## Where a signature is found, and what its bytes are

**A signature is not always the `/V` of a field.** 12.7.4.5 says it is, but some signatures are
reachable only through the catalog's `/Perms` (12.8.4): a file can carry a `/UR3` usage-rights
signature and no signature field at all, or a `/FT /Sig` annotation in a document with no
`/AcroForm`. So the inventory walks both roots and every entry records which one found it — a
usage-rights signature grants a reader capabilities and makes no claim about the document's
content, and reporting the two the same way would say that it does.

**Producers disagree about whether `/Contents`' angle brackets are inside the gap.** Some put
`<` and `>` inside the two `/ByteRange` spans' gap; others put them outside, so the signature
covers its own delimiters. Both are read, and the second is named by a warning
(`a_delimited_gap_and_a_tight_one_both_decode_and_only_one_is_silent`), because the difference
is two bytes and two bytes inside or outside a digest is a different digest.

**The `%%EOF` end-of-line marker straddles the revision boundary.** `Revision::byte_range` stops
just past `%%EOF`; a signer who included 7.5.5's trailing EOL covers one byte more. Without a
tolerance of exactly those EOL bytes, and no others, an ordinary sign-then-fill document would
read as suspicious.

**An encrypted document's object streams do not decompress until the file key exists**, so a
signature inside one is invisible until the caller authenticates. `Anchor::MergedField` — a
producer merging the signature dictionary into the field object — is a defensive branch held
up by a fixture (`a_field_carrying_the_signature_entries_itself_is_still_a_signature`).

## What the CMS reader reaches

**`Signature::contents` is the gap the `/ByteRange` spans leave**, which is right — the bytes a
signature covers are the ones the file says it covers. A signature whose `/ByteRange` does not
bracket its `/Contents` (a `Coverage::Suspicious` one) hands the CMS parser nothing, even where
the document carries a readable blob.

**`/Contents` is the reservation, not the blob.** A signer reserves space before layout and
cannot shrink it, so the stored string is DER followed by zero fill, and a DER parser refuses
the whole of it as trailing bytes — correctly. `Signature::cms()` trims the fill, and **only
zeros**: bytes after the declared length that are anything else are not fill, and trimming
whatever follows a signed structure would be a second reading of it.

**The certificate parser** (`x509.rs`) reads subject, issuer, validity, SPKI and extensions,
gated on RFC 5280's appendix examples, and the CMS census runs it over every certificate in the
corpora. A `CertificateChoices [1]` extended certificate is kept apart from the X.509 ones
(`a_certificate_set_keeps_other_choices_apart_from_x509_ones`).

**RFC 5652 §5.4 is adjudicated by data rather than by reading.** `crates/tinker-pdf/tests/cms_census.rs`
verifies real signatures from independent producers under this engine's own RSA when the stored
`[0]` IMPLICIT tag is replaced by a universal `SET OF` tag before digesting, and asserts that
**not one of them verifies without it**. That is the strongest evidence available for the rule
most likely to be implemented subtly wrong, and it is a `cargo test` assertion rather than a
note.

**What fixtures hold up.** A signer identified by `subjectKeyIdentifier`, embedded CRLs, the
`[1]` certificate choice and the refusal of a BER `signedAttrs` are held up by fixtures in
`cms.rs`. The ECDSA and RSASSA-PSS arms are held up by
published vectors and by OpenSSL's CMS around this repository's own `/ByteRange`;
`crates/tinker-pdf/tests/ecdsa_verdict.rs` states which link is which, and the split is the
same one `pubsec.rs` makes, for the same reason.

## Reading BER without widening what is signed

ISO 32000-1 12.8.3.3.1 calls a signature's `/Contents` a DER-encoded object, and real signers
write BER: indefinite lengths from the outermost SEQUENCE down, `30 80 … A0 80 30 80`, from
Acrobat Distiller, Adobe LiveCycle Designer and LibreOffice. RFC 5652 §5.1 permits it. DER
admits one encoding per value, and a second reading of a signed structure is a signature bypass
rather than a leniency, so BER is read under rules that keep what is signed DER.

**The rule is exactly one bit wide.** `der::Limits` carries `allow_indefinite_lengths`, false in
`Limits::new`, false in `Limits::CERTIFICATE`, and true in `Limits::CMS` and nowhere else. So
`ContentInfo::parse` reads X.690 §8.1.3.6's form and every other caller of `tinker-pdf-pki`
reads DER — including `Certificate::parse`, which runs under `Limits::CERTIFICATE` however it
was reached, so a certificate that arrives inside a BER message is still held to RFC 5280 §4.1's
DER. It is a property of a whole parse rather than a parameter on a reader, because a structure
half-read under one rule and half under the other is the differential the whole exercise is
trying not to have.

**What is signed is not widened.** RFC 5652 §5.4 digests the DER of `signedAttrs`, so
`Attributes::parse` sweeps that subtree — the `[0]` node and everything under it, including
attribute values this crate has no decoder for — and an indefinite length anywhere in it is
`CmsError::IndefiniteSignedAttributes`. The sweep is separate from the walk because the walk
locates an unrecognised attribute's values without descending into them, so a BER encoding
three levels inside one would otherwise be invisible. `unsignedAttrs` is deliberately *not*
held to the rule: nothing digests it.

**The terminator is walked to, never searched for.** `00 00` occurs constantly inside real
content — an INTEGER, a digest, a modulus — so searching for the pair rather than walking the
nodes in between does not fail to parse. It produces a *different, well-formed reading of the
same bytes*, which is the parser differential a signature bypass is made of. A verifying
signature is the evidence the scan is right: a parser that found the wrong end-of-contents pair
would still produce a structure, but not a `signedAttrs` whose digest matches a signature a real
signer made with a real key. The CMS census makes that check over real BER blobs — somebody
else's bytes against this engine's own RSA (ruling 13).

**An indefinite length moves the depth ceiling from descent time to read time.** A
definite-length node nested too deeply reads fine and refuses when a caller descends; an
indefinite one cannot say where it ends without walking what is inside it, so the whole node is
`DepthExceeded`. Forty terminated levels fit in 160 octets, and
`fuzz/corpus/pki_der/indefinite-deep-nesting` is that input.

## RSASSA-PSS, and the two checks no vector reaches

`tinker_pdf_pki::pss` reads RFC 4055 §3.1's `RSASSA-PSS-params` out of a signer's
`signatureAlgorithm`, a certificate's, or an `id-RSASSA-PSS` key's own restrictions;
`RsaPublicKey::verify_pss` is RFC 8017 §8.1.2 with EMSA-PSS-VERIFY and MGF1; the verdict's PSS
arm and the chain walk's PSS link take every length from the parameters and enforce RFC 4056
§3's key restrictions. The evidence is the same three-way split the ECDSA arm has: CAVP
`SigVerPSS` vectors and RSA Laboratories' for the arithmetic — the second set is there for its
1 025- to 1 031-bit keys, where `emLen` is one octet short of the signature and NIST's
whole-octet moduli never go — OpenSSL 3.0.13's CMS and certificates in
`tests/signature_support/rsa-pss.pdf`, and this repository's `/ByteRange` on both sides.

**Two checks no published vector reaches.** A signer following EMSA-PSS-ENCODE cannot produce a
block that breaks step 6 (the bits above `emBits`) or step 4 (the `0xbc` trailer) while keeping
`H` right. Neither is a forgery on its own; both are a verifier accepting an encoding the
standard does not have.
`a_block_a_signer_could_not_have_written_is_refused_even_where_h_matches` recovers RSA
Laboratories' example 3 `EM` with the public key and spoils it both ways.

**The mask hash.** NIST's and RSA Laboratories' vectors, and the OpenSSL fixture, all mask with
the message hash, so step 8's choice of hash is exercised by `pss-mgf1.txt` in
`tinker-pdf-crypto`'s test data alone: four signatures OpenSSL 3.0.13 made with the two hashes
apart, in both directions of output length; each must verify under its own parameters and be
refused under the message hash as the mask hash.

## The verdict's other shapes

**RFC 5652 §5.4's other case.** A signer with no signed attributes is verified over the
content's own digest — the covered bytes, for a detached signature — and the verdict says the
two questions are then one (`Unchecked::NoSignedAttributes` is the digest's reason only when the
signature failed).

**`adbe.pkcs7.sha1`.** The document digest is two links — the encapsulated twenty octets against
the covered bytes' SHA-1, and the signer's `messageDigest` against those octets — and
`a_document_and_its_encapsulated_digest_replaced_together_are_caught` is the test the second
link exists for. `ContentNotEncapsulated` names the one refusal, a message under the subfilter
that is detached.

**`GeneralNames`.** `tinker_pdf_pki::general_name` reads RFC 5280 §4.2.1.6's choice, decoded on
request in the alternative-name extensions, `authorityCertIssuer` and an ESS `issuerSerial`,
against RFC 5280 C.2's mailbox and an OpenSSL CAdES fixture carrying eight of the nine
alternatives. The one thing easy to get wrong is that `[4]` is explicit — `Name` is a `CHOICE`,
and X.680 §31.2.7 makes a tag on one explicit whatever the module says. `subjectAltName` stays
out of what `unrecognised_critical` counts as recognised: decoding a name is not identifying a
subject by it, and nothing here does the latter.

**RFC 3161 signature timestamps.** `tinker_pdf_pki::tsp` reads a `TimeStampToken` — the envelope
through `cms`, the `TSTInfo` with RFC 3161 §2.4.2's fractional `genTime` and its explicitly
tagged `tsa` `GeneralName` — and `cms` reads RFC 2634's first-version `signingCertificate`, whose
`ESSCertID`s are SHA-1 with no algorithm field, because that is what RFC 3161 §2.4.1 names and
what OpenSSL's TSA writes by default. The verdict asks five things of each token: the imprint
against the signature octets, the token's signature, its `messageDigest` against the `TSTInfo`
(a time rewritten under an intact signature fails here and nowhere else), §2.3's critical
`timeStamping`-only extended key usage with §2.4.1's ESS binding, and the chain, judged at
`genTime`. The evidence is one token OpenSSL 3.0.13's TSA made.

**Shapes OpenSSL will not sign**, which `signature_shapes.rs` assembles with the committed
throwaway key, a control beside each. Signed attributes with no `messageDigest` (RFC 5652 §5.3
forbids it) bind nothing: a token's signature over them, an `adbe.pkcs7.sha1` message carrying
the right digest with nothing signing it, and the detached digest of such a signer are all
`NotChecked(NoMessageDigest)`. A detached `adbe.pkcs7.sha1` message with no signed attributes is
`ContentNotEncapsulated`. And RFC 4055 §1.2 restricts an `id-RSASSA-PSS` key to RSASSA-PSS, so
the OID is asked at each use: such a key verifies no PKCS#1 v1.5 signature or link, and nothing
is sealed to it.

## Document timestamps and the security store

**Document timestamps.** A `Timestamper` seam the shape of `Signer` — the engine hands over a
digest and receives a token, and never speaks to an authority itself — and
`DocumentEditor::save_timestamped`, which shares the reservation and the seal with `save_signed`
through one `seal_update`; `Reserved::build` and `Reserved::document_timestamp` share one
`finish`. The reader gives an `ETSI.RFC3161` dictionary the token path: the imprint against the
covered bytes, and `Verdict::is_trusted` requiring the document timestamp's own
`TimestampVerdict`.

**The document security store.** `add_validation_data` writes the catalog's `/DSS` — `/Certs`,
`/CRLs` and `/OCSPs` as arrays of streams, one `/VRI` entry per named signature keyed by the
SHA-1 of its `/Contents` (ETSI EN 319 142-1 §5.4.2.2) through one `validation_key` the reader
shares — extending a store already there rather than replacing it, never writing a stream twice,
and declaring `/ESIC` on a document below 2.0. The reader hands back references, not bytes, by
the attachments precedent: a store may name thousands of streams and listing them should not
cost decoding them. The evidence is a CRL and an OCSP response OpenSSL issued for a fixture's
signer, held to coming back byte for byte, filed under the right key, with the signature still
verifying over its revision. A member of the wrong *type* — an entry that is not an array, a
`/VRI` that is not a dictionary, an unreadable `/TU`, a `/DSS` that is not a dictionary — is a
typed `SecurityStoreWarning` (ruling 10), and a null `/DSS` — absent, or a reference to no
object, 7.3.10 — is `None`.

## Scope

- **Read: byte-range digesting (12.8.1).** Parse the signature dictionary — `/ByteRange`,
  `/Contents`, `/SubFilter`, `/M`, `/Reason` — from every populated `FieldKind::Signature`
  field **and from the catalog's `/Perms`** (see above). Digest the named ranges over the
  *stored* bytes (the `stream_raw_encrypted` forensic tier and the raw file buffer, per
  [opening](../features/opening.md)), and classify coverage: whole file minus the `/Contents`
  gap, or a prefix with later revisions on top.
- **Read: CMS and X.509 parsing.** DER (X.690) walker; CMS `SignedData` (RFC 5652) including
  signed attributes and `messageDigest`; X.509 certificates (RFC 5280). `SubFilter`s
  `adbe.pkcs7.detached` and `adbe.pkcs7.sha1` (ISO 32000-1 12.8.3.3) plus
  `ETSI.CAdES.detached` (ISO 32000-2, CAdES subfilter clause); `adbe.x509.rsa_sha1` recognised
  and reported, not verified.
- **Read: signature verification, hand-rolled, verify-only.** RSASSA-PKCS1-v1_5 and
  RSASSA-PSS (RFC 8017) and ECDSA over P-256/P-384 (FIPS 186-4) verification in
  `tinker-pdf-crypto`, gated on published vectors exactly as the AES/SHA code is. Digests are
  the crate's `sha256`/`sha384`/`sha512` (RFC 5754 names them for CMS); SHA-1 accepted for
  legacy signatures and flagged as weak in the verdict.
- **Read: modification detection (12.8.2.2, 12.8.2.4).** A signature covering revision *N*
  plus later revisions from `CosDocument::revisions()` yields the set of objects the later
  revisions touched; classify them against the `/DocMDP` `/P` level and `/FieldMDP` field
  lists, producing a typed answer, not a boolean. The writer writes `/DocMDP` and `/FieldMDP`
  too.
- **Write: sign on incremental save.** Reserve `/Contents` and `/ByteRange`
  (`SignaturePlaceholder` is the producer's record), patch `/ByteRange` after layout, hand the
  range digest to a caller-supplied `Signer`, hex-patch the returned CMS into the gap — as
  `DocumentEditor::save_signed`, beside `save` rather than an option on it (see Design).
- **Trust model.** The engine verifies chain *shape* — signatures along the chain and validity
  windows — up to anchors the host supplies as DER bytes, the same inversion as `FontProvider`
  (`crates/tinker-pdf/src/fonts.rs`): the engine computes, the host decides what to trust. No
  bundled root store.
- **Facade.** `Document::signatures()` and per-signature verdict structs, projected through the
  FFI per ruling 11; typed warnings with provenance per ruling 10.

## Non-goals

- **Signing keys in the engine.** No key parsing (PKCS#8, PKCS#12), no key generation, no
  RSA/ECDSA *signing* arithmetic: the `Signer` callback returns finished CMS bytes. In-engine
  signing with a private key is [ROADMAP](../ROADMAP.md) rows SG-01 to SG-09 and SG-23; the
  `Signer` seam serves keys the engine never sees.
- **Revocation fetching.** No CRL or OCSP network traffic — the engine performs no I/O.
  `Document::security_store` reads the document security store (PAdES DSS/VRI, ISO 32000-2
  12.8.4.3) back as references to its streams, with its `/VRI` entries keyed to signatures,
  `DocumentEditor::add_validation_data` writes one from host-supplied certificates, CRLs and
  OCSP responses, and a CMS blob's own CRLs are surfaced through `SignedData::crls`. Nothing
  parses a CRL or an OCSP response, and the verdict does not consult the store. Parsing and
  evaluating CRLs and OCSP responses, building the requests a host sends, and consulting the
  store are [ROADMAP](../ROADMAP.md) row SG-10; the fetching stays the host's, because the
  engine performs no I/O.
- **Long-term validation profiles.** PAdES-LTA conformance levels are out; the verdict reports
  what is embedded, nothing more. The PAdES baseline levels with a level detector are
  [ROADMAP](../ROADMAP.md) row SG-07, and validation per EN 319 102-1 is SG-12.
- **The public-key security handler** (7.6.5) — related ASN.1, a different feature
  ([pubsec](pubsec.md)).

## Design

**What it builds on.** Three seams carry it. First: `incremental_update`
(`crates/tinker-pdf-cos/src/write.rs`) appends after the original bytes, adding a newline only
when the base lacks one, and `an_incremental_save_keeps_the_original_bytes`
(`crates/tinker-pdf-cos/src/edit/tests.rs`) and
`an_incremental_update_preserves_the_original_bytes_exactly` (`write.rs`) assert
`starts_with(original)` — the byte-identical prefix a signature needs;
`a_signature_survived_by_an_incremental_update_covers_a_revision` checks what the *reader*
makes of the result. Second: each `Revision` (`crates/tinker-pdf-cos/src/xref.rs`) carries the
byte range a signature covers, recorded at open per 7.5.6 — MDP analysis is a consumer of
existing data, not new parsing. Third: `tinker-pdf-crypto` holds SHA-256/384/512 and the
published-vector merge gate, and its `EntropySource` trait is the precedent for hosts supplying
what the engine refuses to own.

**A leaf crate: `tinker-pdf-pki`.** DER, X.509 and CMS parsing live in a leaf crate — bytes in,
values out — not in `tinker-pdf-crypto`. The argument: the crypto crate is *arithmetic* whose
failure mode is a wrong number caught by published vectors; ASN.1 parsing is *untrusted-input
structure walking* whose failure mode is a panic or an overread on malformed bytes, exactly the
class ruling 1's per-format fuzzers exist for. Merging them would put a large attack surface
inside the crate whose review story is "small, vector-gated arithmetic", and would leave DER
parsing fuzzable only through crypto's API. As its own leaf, `tinker-pdf-pki` gets dedicated
fuzz targets over raw DER and CMS, its own test corpus of certificates, and independent
publishability. It has no PDF vocabulary (X.509 and CMS are not PDF concepts), satisfying
ruling 8's definition, and its one edge — `pki → crypto` for digest and signature-verify
primitives — is a leaf-to-leaf edge like `font → filters`, which ruling 8 explicitly permits.
The alternative (parsing in the facade) fails ruling 8's spirit in the other direction: it
would weld protocol parsing to COS types for no gain. The crate exposes: a DER walker (definite
lengths by default; X.690 §8.1.3.6's indefinite length behind an opt-in on `Limits` that only
`Limits::CMS` sets, because RFC 5652 §5.1 permits BER and real signers write it;
depth-capped; never panics), `Certificate` (subject/issuer as compared-by-DER blobs plus
decoded fields, SPKI, validity, extensions), and `SignedData` (digest/signature algorithm
identifiers, signed attributes with their exact DER for re-digesting, certificates, signer
identifier). It holds no trust anchor; the chain walk is the facade's. Unknown algorithm OIDs
return typed refusals.

**Verify math in `tinker-pdf-crypto`.** A constant-size big-unsigned type (`Uint<N>`, stack
arrays, no allocation in hot paths) with the operations verification needs: modular
exponentiation for RSA, and P-256/P-384 field/scalar arithmetic with Jacobian point math for
ECDSA. Verify-only keeps it honest: no private-key operations means no constant-time
obligations beyond what the password comparison already set, though modular ops are written
branchless-by-value anyway. Gates: NIST CAVP known-answer vectors and RSA Laboratories' PSS
vectors as merge requirements, mirroring the FIPS 197 / RFC 6229 gates the crate's cipher code
cites. EMSA-PKCS1-v1_5 decoding compares the full expected encoding byte-for-byte against the
decrypted block — the strict comparison that forecloses Bleichenbacher-style forgery under low
exponents — and ECDSA rejects `r`, `s` outside `[1, n-1]` before any curve math.

**The verdict is a report, not a bool.** Per signature, the facade returns: coverage
(`WholeFile` — the two ranges bracket exactly the `/Contents` gap and reach EOF — or
`Revision { index }`, or `Suspicious` with a typed reason when ranges overlap the gap
wrongly, the territory 12.8.1's byte-range notes warn about); digest match; CMS signature
validity including the signed `messageDigest` attribute cross-check; chain result
(`AnchoredTo`, `SelfSigned`, `Incomplete`, `Broken`, `NoAnchors`, `NoSignerCertificate`);
weaknesses, a certificate outside its validity window among them; and the MDP classification
below. Nothing is collapsed: "digest holds but the chain reaches no anchor" is the everyday case
and the host renders it, exactly as `Bitmap.warnings` consumers do under ruling 2. A verified
signature over a changed document is the combination a single boolean cannot express, and
`tests/verdicts.rs` pins it: flipping one byte inside `digitally-signed.pdf`'s covered range
turns its digest to `Differs` and leaves its signature `Verified`. Anything unparseable or
unsupported becomes a typed verdict variant with provenance (rulings 2, 10), never an `Err`,
never a panic (ruling 1) — a signed file must still open, render, and extract.

**MDP over revisions** — `crates/tinker-pdf/src/mdp.rs`, `Signature::modifications`. The prefix
is reparsed as its own `CosDocument`, but what is diffed is **not** object values: it is
**cross-reference entries**. Comparing values has two faults that only show up on real
documents. An encrypted document's strings and streams come back decrypted from the
authenticated original and encrypted from a freshly opened prefix, so every object would compare
unequal and the analysis would report a wholly rewritten document whenever it is merely
encrypted. And it would mean parsing every object of both documents to answer a question about
which bytes were written. 7.5.6 says an update writes a new entry for exactly the objects it
changed, so a differing entry *is* a changed object — no key needed, and one parse of each
object that actually moved. `/DocMDP` `/P` (12.8.2.2): level 1 permits nothing; 2 permits form
fill and signing; 3 adds annotations. `/FieldMDP` (12.8.2.4) narrows to the named fields under
its `/Action`, and overrides the level: a locked field may not change however permissive `/P`
is. The output is a list with each entry classified and marked `permitted`.

**What object granularity forces.**

A cross-reference table's finest grain is an object, and two containers every document has are
rewritten by operations 12.8.2.2 *permits*. Adding a signature appends its widget to a page's
`/Annots`, and where that array is direct in the page dictionary — the common shape — the page
object changes; filling a field rewrites `/AcroForm`, and where that is direct in the catalog,
the catalog changes. Classified strictly, every legitimate fill and every legitimate
countersignature would report itself as a violation, which is a check nobody can use. So an
altered page or catalog whose change is *confined* to those keys is classified as plumbing,
proved by comparing the two dictionaries key by key. What that cannot see is inside the allowed
key: an annotation removed and one added look identical to it, and
`emptying_the_annotation_list_is_not_caught_and_this_is_the_limit` (`tests/certification.rs`)
asserts exactly that, so the day it stops being true is a failing test rather than an unnoticed
improvement.

The key-by-key comparison is by name *bytes*, because `Object::Name` holds an interned symbol
valid only against the table that issued it, so `/Type /Page` in one document and `/Type /Page`
in another are unequal by `PartialEq` whenever the two tables interned in different orders —
a trap for anything that compares two documents. One property falls out of it and is kept
deliberately: a stream is answered "not the same" without being read, and in an encrypted
document unchanged strings compare unequal too — so encryption makes this analysis stricter and
never looser.

**Signing** — `DocumentEditor::save_signed` (`crates/tinker-pdf-cos/src/edit/signing.rs`), its
types in `crates/tinker-pdf-cos/src/sign.rs`. The `Signer` trait is two calls,
`SignaturePlaceholder` records the four offsets and lengths a patch needs, and a CMS larger than
the reservation is `SignError::ReserveTooSmall` rather than a truncation. Adding the field and
setting `/SigFlags` happen in one update, and `DocumentEditor::acroform` reads the catalog
through the editor's own overlay, so a second edit to the form in one save never starts from
the file's original catalog and discards the first.

**The request is a parameter, not a `WriteOptions` field.** A `&dyn Signer` needs a lifetime,
and giving `WriteOptions` one would change the type in every existing caller and in
`tinker_parity.rs`, which pins public signatures under ruling 12 — for a field that is
meaningless on every write mode but one. So `save_signed(&WriteOptions, &SigningRequest)`
sits beside `save`, and a rewrite asked to sign is `SignError::NotIncremental` rather than a
silently ignored option.

**The signature object is serialised by hand and appended outside the object set.** Two
independent reasons force it. `write_dict` cannot report *where* inside its output a value
landed, and the reservation is patched by offset — searching the finished file for a run of
zeros would find the first plausible match rather than the right one. And 7.6.2 exempts a
signature dictionary's `/Contents` from encryption, so it must not go through the inherited
cipher; writing it by hand makes skipping the cipher a decision rather than an omission.
`write::UpdatePlan` carries it, and `/Size` counts it explicitly, because an object nothing
else knows about is one a conforming reader would be required to ignore.

**`/M` is a parameter.** Ruling 4 bans a clock from this engine's output, and here that is
not only a determinism rule: a signing time the engine invented is a claim it is not entitled
to make.

**Verification, and the one place ruling 13 costs the most.** The primitives are gated by
published test vectors, which is data and the strongest evidence available: NIST CAVP RSA and
ECDSA verify vectors, RSA Laboratories' PSS vectors, RFC 5652 fixture DER. Read-side ground
truth is the signed fixtures committed in `tests/signature_support/`, made by OpenSSL as its
`README.md` records, and the corpus censuses. Fuzzers: raw DER and CMS into `tinker-pdf-pki`
(`pki_der`, `pki_cms`), and the signature reader over arbitrary bytes (`signatures`). Written
signatures are checked by the strict structural validator, and the signing suite
(`tests/signing.rs`) asserts the `starts_with(original)` prefix invariant and independently
re-digests the returned `/ByteRange` spans.

**What none of that establishes:** that anyone else accepts the signature. Under ruling 13 no
CI job may ask a validator, so:

> A signature everything in-tree accepts may still be rejected by real validators.

Nothing here hides that, because a signing feature whose interop claim is unverified and
unstated is worse than one that says so.

## Milestones

1. Signature inventory: `/ByteRange`/`/Contents` parsing, range digesting, coverage
   classification.
2. `tinker-pdf-pki`'s DER walker and X.509.
3. CMS `SignedData` parsing, signed attributes included.
4. Big-unsigned arithmetic and RSASSA-PKCS1-v1_5 verification in `tinker-pdf-crypto`.
5. ECDSA P-256/P-384 verification.
6. End-to-end verdicts and trust anchors.
7. `/DocMDP` and `/FieldMDP` over `revisions()`.
8. Signing on incremental save: the seam and the `Signer` callback.
9. The facade and its FFI projection, warnings, docs.

## Dependencies

- **Builds on:** `incremental_update` and the `starts_with(original)` invariant
  (`tinker-pdf-cos/src/write.rs`, `edit/save.rs`); `Revision.byte_range` from open
  (`xref.rs`, [opening](../features/opening.md)); `FieldKind::Signature` and field
  classification (`form.rs`); SHA-2 digests and the published-vector convention
  (`tinker-pdf-crypto`); the strict structural validator
  ([verification](../verification.md)); rulings 1, 2, 8, 10, 11, 13
  ([rulings](../rulings.md)).
- **Within this design:** verdict assembly (6) rests on `tinker-pdf-pki` (2–3) and the crypto
  verify math (4–5); the write side (8) rests only on the inventory (1) and the incremental
  writer.
- **Published data, not programs (ruling 13):** NIST CAVP verify vectors, RSA Laboratories' PSS
  vectors, RFC 5652 examples, RFC 5280 sample certificates; signed fixtures committed to the
  test tree.

## Risks

| Risk | Mitigation |
|------|------------|
| Hand-rolled RSA/ECDSA verify accepts a forgery (padding laxity, missing range checks) | Verify-only scope; CAVP negative vectors and forged-padding cases as merge gates, mirroring the crate's FIPS 197/RFC 6229 precedent; full-encoding comparison for EMSA-PKCS1-v1_5 rather than a prefix match. The negative vectors carry this alone under ruling 13, which is why every published invalid case is a gate rather than a sample |
| ASN.1 parser panics or overreads on malformed DER (largest new untrusted surface) | Own leaf crate with dedicated fuzz targets (`pki_der`, `pki_cms`); depth caps; definite-length parsing by default, with the indefinite form opt-in per parse, constructed-only, budget-and-depth-bounded during its end-of-contents scan, and refused outright inside what §5.4 digests; ruling 1 makes a fuzz crash a release blocker |
| A verdict rendered as a single "valid" boolean misleads hosts into overtrusting | The API has no boolean: coverage, digest, chain, and MDP are separate typed fields, and the docs state the engine proves integrity, not identity — anchors are the host's assertion |
| Byte-range trickery: ranges that skip more than the `/Contents` gap make a "valid" signature over chosen bytes | Coverage classification is computed from the ranges, never trusted from them; anything but exact-gap bracketing to EOF or a clean revision boundary is `Suspicious` with a typed reason, fixture-pinned |
| MDP misclassification calls a benign form fill a disallowed change (or the reverse) | Classification reuses the tested `form.rs` field machinery rather than re-deriving object roles; fixtures for each `/P` level in both directions, each fixture's expected verdict written from the clause rather than from a run |
| Signing seam drifts from the prefix invariant and silently invalidates what it signs | `starts_with(original)` asserted by the signing suite (the incremental writer's rule); the `/ByteRange` digest re-computed independently in tests |
| **Nobody outside this repository validates a signature it produced** (ruling 13), so this engine can sign confidently and wrongly | The primitives are pinned by published vectors, which is the part most likely to be subtly wrong. Assembly and placement are adjudicated by nobody, and the sentence about real validators stands in the feature doc as a permanent limit. **Not closed** |
