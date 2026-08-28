# Digital signatures

When this is done, opening a signed PDF reports what every signature actually proves — which
bytes it covers, whether the digest over them holds, whether the CMS signature verifies against
the signer's certificate, how far the certificate chain gets toward a host-supplied trust
anchor, and what changed after signing, classified against `/DocMDP` and `/FieldMDP` rules —
and `DocumentEditor` can produce a signature of its own on an incremental save, with the
private key held by a caller-supplied signer callback so key material never enters the engine.
**All of that now happens.** Milestones 1 and 3 through 8 have landed, with milestone 2's
crate under them. `Document::signatures()` finds every signature and classifies what its
`/ByteRange` covers; `tinker-pdf-pki` reads DER, X.509 and CMS `SignedData`;
`tinker-pdf-crypto` verifies RSA and ECDSA against 504 published vectors;
`Document::verify_signatures()` assembles the four answers; `Signature::modifications()`
measures later revisions against `/DocMDP`; and `DocumentEditor::save_signed` produces
signatures of its own, certifying and locking fields, with the key held by the caller. What
holds the two ends together is that the writer and the reader call one `digest_spans`, so
what is signed and what is checked cannot drift.

Milestone 2's sidecar now exists too: `tests/signature_support/certificates.tsv` records
serial, validity, SubjectPublicKeyInfo digest, self-issued flag and both common names for 24
corpus certificates, produced **once by OpenSSL** and committed with the commands, the version
and the date in its header. Ruling 13 permits exactly that — a third-party program may supply
data, and the committed output of a tool run once is a dated measurement rather than a check.
Transcribing twenty-four certificates by hand does not scale, and producing the values with
`tinker-pdf-pki` would be the parser agreeing with itself, which is the one thing a sidecar
must not be.

Fifteen of the twenty-four are reachable and all fifteen match. **Both numbers are asserted**,
and the mechanism has already earned itself: it was thirteen until BER indefinite lengths were
read, and the test failed the moment that refusal lifted. The nine still out of reach sit in
blobs whose `/ByteRange` does not bracket their `/Contents` — readable by a scanner that
ignores the coverage classifier, which is exactly what this build declines to be.

One disagreement came out of it, about a real certificate. Three corpus serials have their top
bit set with no leading zero, so the `INTEGER` *is* negative in DER — RFC 5280 §4.1.2.2 says a
serial "MUST be a positive integer" and those issuers emitted one that is not. OpenSSL reads
the number and prints `-0603E746C0C547A1789F`; so does this crate. The sidecar records the
octets `F9FC18B93F3AB85E8761` instead, because that is what `issuerAndSerialNumber` matching
compares — the certificate's identity rather than either side's rendering of its value.

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

## What milestone 3 measured, which changed this document again

The CMS parser landed and was pointed at every blob in the fetched corpora
(`crates/tinker-pdf/tests/cms_census.rs`). Five things came back that this design had not
said, and the first is a scope change rather than a detail.

**A fifth of the corpus is BER, and this engine refuses it.** ISO 32000-1 12.8.3.3.1 calls a
signature's `/Contents` a DER-encoded object. **Four of the eighteen CMS blobs are not**:
`160F-2019.pdf`, `issue16553.pdf`, `prefilled_f1040.pdf` and the second signature of
`xfa_filled_imm1344e.pdf` open `30 80 … A0 80 30 80` — indefinite lengths from the outermost
SEQUENCE down, which is legal BER. **It is not one vendor's quirk**: those documents name
Acrobat Distiller 5.0.5, Adobe LiveCycle Designer ES 8.2 and 10.0, and LibreOffice 7.5 as
their producers. (A document's producer is not necessarily its signer's software, but it is
the evidence the files carry, and it spans two independent lineages either way.)
`tinker-pdf-pki` refuses all four, by name and on the argument `der.rs`'s header makes: DER
admits one encoding per value, and a second reading of a signed structure is a signature
bypass rather than a leniency. **Nothing verifies those four files today, and the design does
not currently say how it ever will.** Reading them means teaching the walker to reconstitute
definite lengths from an end-of-contents pair, which changes the rule every parser above it
rests on; it is its own milestone, with its own argument about what two readings of one
structure would cost, and it was deliberately not done quietly inside a milestone about
`SignedData`. The number is asserted so it cannot drift unremarked.

> *Answered.* The milestone this paragraph asked for was done; see
> [What reading BER measured](#what-reading-ber-measured-and-what-it-was-not-allowed-to-widen)
> below. The measurement above stands as the record of what milestone 3 found; the sentences
> about nothing verifying those four files, and about the design not saying how it ever
> would, no longer describe this engine.

**`Signature::contents` reaches twelve of the eighteen.** Milestone 1 takes the CMS bytes from
the gap the `/ByteRange` spans leave, which is right — the bytes a signature covers are the
ones the file says it covers. The consequence had not been stated: six signatures have a
`/ByteRange` that does not bracket their `/Contents` at all (the same six that classify as
`Coverage::Suspicious`), so the supported path hands the CMS parser nothing, even though each
of those documents does carry a readable blob. Milestone 6's verdict will have to say which of
the two it is reporting on, because "no CMS" and "a CMS the coverage classifier will not vouch
for" are different answers.

**The certificate parser works on certificates nobody here transcribed.** All 29 X.509
certificates across the parsed blobs read through `x509.rs` — subject, issuer, validity, SPKI,
extensions — with zero failures. Milestone 2 was gated on RFC 5280's appendix examples, which
are hand transcriptions of a specification; this is the first evidence from other people's
software. One `CertificateChoices [1]` extended certificate turned up inside `bug854315.pdf`'s
timestamp token, which is the only non-X.509 member in the corpus and the only real reason
that arm exists.

**RFC 5652 §5.4 is now adjudicated by data rather than by reading.** Fifteen real signatures,
from six independent producers, verify under this engine's own RSA when the stored `[0]`
IMPLICIT tag is replaced by a universal `SET OF` tag before digesting — and **not one of them
verifies without it**. That is the strongest evidence available for the rule most likely to be
implemented subtly wrong, and it is a `cargo test` assertion rather than a note.

**What the corpus does not contain, so that the gaps are named rather than assumed.** Zero
ECDSA and zero RSASSA-PSS signatures — every signer is RSASSA-PKCS1-v1_5, which means
milestone 5's ECDSA path will have no corpus evidence at all and must lean entirely on CAVP
vectors. Zero signers identified by `subjectKeyIdentifier`, zero embedded CRLs, and zero
`signingCertificateV2` attributes that this engine can reach — the corpus's only one is inside
`issue16553.pdf`, which is one of the four refused for BER. Those four code paths are held up
by fixtures in `cms.rs` and by nothing else, and each says so where it is defined.

> *One of those four has since closed.* Reading BER reached `issue16553.pdf`, so
> `signingCertificateV2` now has real evidence and the census asserts 1 rather than 0.
> `subjectKeyIdentifier`, embedded CRLs and the `[1]` certificate choice are still
> fixture-only — and the list gained a member: **no corpus blob has a BER `signedAttrs`**, so
> the RFC 5652 §5.4 rule that refuses one is adjudicated by fixtures alone. The injection
> matrix below is where that was found.

## What reading BER measured, and what it was not allowed to widen

Milestone 3's first finding said this needed its own piece of work with its own argument.
This is that argument and its measurements. The refused count is now **0 of 18**, and the
number it dropped from is the one milestone 3 asserted.

**The rule that changed is exactly one bit wide.** `der::Limits` grew
`allow_indefinite_lengths`, false in `Limits::new`, false in `Limits::CERTIFICATE`, and true
in `Limits::CMS` and nowhere else. So `ContentInfo::parse` reads X.690 §8.1.3.6's form and
every other caller of `tinker-pdf-pki` is unchanged — including `Certificate::parse`, which
runs under `Limits::CERTIFICATE` however it was reached, so the twelve certificates that
arrive inside a BER message are still held to RFC 5280 §4.1's DER. It is a property of a
whole parse rather than a parameter on a reader, because a structure half-read under one rule
and half under the other is the differential the whole exercise is trying not to have.

**What is signed did not widen at all.** RFC 5652 §5.4 digests the DER of `signedAttrs`, so
`Attributes::parse` sweeps that subtree — the `[0]` node and everything under it, including
attribute values this crate has no decoder for — and an indefinite length anywhere in it is
`CmsError::IndefiniteSignedAttributes`. The sweep is separate from the walk because the walk
locates an unrecognised attribute's values without descending into them, so a BER encoding
three levels inside one would otherwise be invisible. `unsignedAttrs` is deliberately *not*
held to the rule: nothing digests it.

**All four blobs write `signedAttrs` with definite lengths.** That is the measurement that
made this safe to do at all, and it is worth stating as a number rather than a hope: the
BER in these files is five structural nodes — the `ContentInfo`, its `[0]`, the `SignedData`,
the `EncapsulatedContentInfo` and the certificate set — and nothing below them. So the §5.4
rule costs this corpus nothing today, and `cms.rs` holds it up with a fixture rather than
with the corpus.

**Four more signatures verify, and that is the evidence the scan is right.** The census now
reports **18 of 18 blobs parsed, 0 refused**, 20 signers, 41 certificates all parsing, and
**19 signatures verifying over the §5.4 re-encoding — up from 15 — with 0 verifying over the
stored `[0]` bytes.** Each of the four BER blobs is among the four new ones. A parser that
found the wrong end-of-contents pair would still produce a structure; it would not produce a
`signedAttrs` whose digest matches a signature a real signer made with a real key. That is
the strongest available check on an end-of-contents scan, and it is somebody else's bytes
against this engine's own RSA (ruling 13).

**One code path stopped being fixture-only.** `issue16553.pdf` carries the corpus's only
`signingCertificateV2` attribute (RFC 5035 §3), and it was unreachable because that blob was
one of the four refused. The census assertion for it moves from 0 to 1, which is the first
real evidence under that decoder.

**One consequence of the encoding, recorded because it is a behaviour change.** An indefinite
length moves the depth ceiling from descent time to read time. A definite-length node nested
too deeply reads fine and refuses when a caller descends; an indefinite one cannot say where
it ends without walking what is inside it, so the whole node is `DepthExceeded`. Forty
terminated levels fit in 160 octets, and `fuzz/corpus/pki_der/indefinite-deep-nesting` is
that input.

### The injection matrix

Five defects were reintroduced one at a time and the suite re-run, over **3 205 workspace
tests plus the corpus census**. House practice
([verification.md](../verification.md)): a guard that catches nothing when its defect is
injected is not a guard.

| # | Defect reintroduced | Caught by | Census |
|---|---------------------|-----------|--------|
| 1 | `signedAttrs` no longer swept for definite lengths | 3 | **0** |
| 2 | depth bound dropped from the end-of-contents scan | 2 | 0 |
| 3 | a missing terminator treated as the end of the buffer | 5 | 0 |
| 4 | the terminator *searched* for as two bytes instead of walked to | 12 | caught |
| 5 | the terminator left out of `Tlv::raw`, so a range is two octets short | 5 | caught |

**The matrix found two holes, which is why it is run rather than reasoned about.**

*Injection 2 was caught by one assertion in the first draft*, out of 3 205. The depth bound
inside the scan was held up by a single unit test, and the only reason it fired at all is
that the test's input is *unterminated* — remove the bound and it fails for running out of
buffer instead, which is a different defect wearing the same error. The fix is
`indefinite-deep-nesting`, a **well-formed** forty-level input that must be `DepthExceeded`
under twelve levels and must parse under sixty-four; a truncated twin could not tell a
missing bound from a present one. Two catches now, in two files.

*Injection 1 is caught by nothing in the corpus*, and this one is not fixable by writing a
better test. No corpus blob has a BER `signedAttrs`, so the §5.4 rule that matters most is
adjudicated **entirely by fixtures** — the same standing of evidence as
`subjectKeyIdentifier`, embedded CRLs and the `[1]` certificate choice, and it is named here
for the same reason those are. The rule is what stops a BER structure from reaching a digest;
the corpus cannot currently say whether it is enforced.

Injection 4 is the one the whole design turns on: `00 00` occurs constantly inside real
content — an INTEGER, a digest, a modulus — so searching for the pair rather than walking the
nodes in between does not fail to parse. It produces a *different, well-formed reading of the
same bytes*, which is the parser differential a signature bypass is made of. Twelve tests and
the census catch it, which is the level of coverage that rule deserves.

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
  lists, producing a typed answer, not a boolean. **Done.** The writer grew `/DocMDP` and
  `/FieldMDP` with it, because no corpus file carries a `/DocMDP` reference at all — without a
  writer there would be nothing to read.
- **Write: sign on incremental save.** Reserve `/Contents` and `/ByteRange` (revive
  `SignaturePlaceholder` as the producer's record), patch `/ByteRange` after layout, hand the
  range digest to a caller-supplied `Signer`, hex-patch the returned CMS into the gap.
  **Done**, as `DocumentEditor::save_signed` rather than as an option on `save` — see the
  design section for why.
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
a DER walker (definite lengths by default; X.690 §8.1.3.6's indefinite length behind an
opt-in on `Limits` that only `Limits::CMS` sets, because RFC 5652 §5.1 permits BER and a
fifth of the corpus is; depth-capped; never panics),
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

**MDP over revisions. Done** — `crates/tinker-pdf/src/mdp.rs`, `Signature::modifications`.
The prefix is reparsed as its own `CosDocument`, as sketched, but what is diffed is **not**
object values: it is **cross-reference entries**. Comparing values has two faults that only
show up on real documents. An encrypted document's strings and streams come back decrypted
from the authenticated original and encrypted from a freshly opened prefix, so every object
compares unequal and the analysis reports a wholly rewritten document whenever it is merely
encrypted. And it means parsing every object of both documents to answer a question about
which bytes were written. 7.5.6 says an update writes a new entry for exactly the objects it
changed, so a differing entry *is* a changed object — no key needed, and one parse of each
object that actually moved. `/DocMDP` `/P` (12.8.2.2): level 1 permits nothing; 2 permits form fill
and signing; 3 adds annotations. `/FieldMDP` (12.8.2.4) narrows to the named fields under its
`/Action`, and overrides the level: a locked field may not change however permissive `/P` is.
The output is a list with each entry classified and marked `permitted`, rather than the
three-variant enum sketched here — the two shapes carry the same information and a list with
a flag has no variant that discards the list.

**Two things object granularity forced, both recorded rather than smoothed over.**

A cross-reference table's finest grain is an object, and two containers every document has are
rewritten by operations 12.8.2.2 *permits*. Adding a signature appends its widget to a page's
`/Annots`, and where that array is direct in the page dictionary — the common shape — the page
object changes; filling a field rewrites `/AcroForm`, and where that is direct in the catalog,
the catalog changes. Classified strictly, every legitimate fill and every legitimate
countersignature reports itself as a violation, which is a check nobody can use. So an altered
page or catalog whose change is *confined* to those keys is classified as plumbing, proved by
comparing the two dictionaries key by key. What that cannot see is inside the allowed key: an
annotation removed and one added look identical to it, and
`emptying_the_annotation_list_is_not_caught_and_this_is_the_limit` asserts exactly that, so the
day it stops being true is a failing test rather than an unnoticed improvement.

The key-by-key comparison hit a trap worth naming, because it is a trap for anything that
compares two documents. `Object::Name` holds an interned symbol valid only against the table
that issued it, so `/Type /Page` in one document and `/Type /Page` in another are unequal by
`PartialEq` whenever the two tables interned in different orders. The comparison is by name
*bytes*. One property falls out of it and is kept deliberately: a stream is answered "not the
same" without being read, and in an encrypted document unchanged strings compare unequal too —
so encryption makes this analysis stricter and never looser.

**Signing. Done** — `DocumentEditor::save_signed`, `crates/tinker-pdf-cos/src/sign.rs`. The
`Signer` trait is the two calls this section specified, `SignaturePlaceholder` finally earns
its four fields, and a CMS larger than the reservation is `SignError::ReserveTooSmall` rather
than a truncation. Three things came out differently from the sketch above, and each is a
correction rather than a shortcut.

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

One defect is recorded rather than quietly fixed, because it was invisible in exactly the way
this repository keeps finding things invisible. Adding the field and setting `/SigFlags` were
two passes, and both read the catalog from `self.doc` rather than through the editor's own
overlay — so the second discarded the first. The file that came out started with the original
bytes, passed the strict structural validator, and carried a signature dictionary **no field
pointed at**. Every structural check in the tree was green; only reading the signature back
found it. `DocumentEditor::acroform` now reads through the overlay and one update does both.

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

### The interop measurement, run once on 28 August 2026

**Tool:** OpenSSL 3.5.5 (27 Jan 2026), on `x86_64-pc-windows-msvc`. Run by hand, outside CI,
from a scratch directory. Nothing below runs again and no job depends on it.

A throwaway key and self-signed certificate were generated
(`openssl req -x509 -newkey rsa:2048 -days 3650 -nodes`), and
`testdata/simple-text.pdf` was signed by `DocumentEditor::save_signed` with an 8 192-byte
reservation and a placeholder blob. The `/ByteRange` this engine wrote covers 2 172 of the
resulting 18 558 bytes as `[0 1899 18285 273]`. Those covered bytes were handed to
`openssl cms -sign -binary -md sha256`, and the 1 435-byte detached `SignedData` OpenSSL
produced was patched into the reservation, leaving the file the same length.

**Direction A — does an outside program accept a signature in a file this engine wrote?**
A script re-read the finished PDF independently, taking `/ByteRange` and `/Contents` from the
bytes with no engine code involved, and:

```sh
openssl cms -verify -binary -inform DER -in blob.der -content covered.bin -CAfile cert.pem
# CMS Verification successful
```

**Direction B — does this engine accept a blob it did not make?** The same file, read through
`Document::verify_signatures` with the certificate as the only anchor:

```text
coverage       WholeFile
cms            Read { signers: 1 }
documentdigest Matches
signature      Verified
chain          AnchoredTo { anchor: "CN=One-time signer,O=tinker-pdf interop,C=GB", links: 0 }
TRUSTED        true
```

**Negative controls**, because "successful" and "did not look" are indistinguishable without
them. One bit flipped at offset 100 of the covered bytes:
`CMS_SignerInfo_verify_content:verification failure`. The same blob with no `-CAfile`:
`certificate verify error … self-signed certificate`. Both refused.

**What this establishes and what it does not.** It establishes that the `/ByteRange` this
writer computes is the range an independent implementation digests, that the reservation and
the patch produce a structurally valid CMS carrier, and that this engine's §5.4 re-encoding,
RSA verification, X.509 parsing and chain walk agree with OpenSSL on a blob OpenSSL made.

It does not establish that **Acrobat, or any PDF viewer, would show a green tick** — no PDF
validator was involved, only a CMS one, and the certificate was self-signed and trusted only
because it was named as the anchor. It is one file, one algorithm (RSA-2048 with SHA-256),
one day. The risk `verification.md` names remains: reader, writer, fixtures and reviewer
still share one author's reading of the specification.

## What milestone 6 measured, and the finding it produced

`Document::verify_signatures` asks four questions and answers each separately.
Over the eighteen corpus signatures, with no anchors supplied: **9 blobs parse,
8 signatures verify against the key in their own certificate, 0 fail, 4
document digests match — and 4 differ.**

The four that differ are veraPDF's `6.1.12 Permissions` and `6.1.11
Permissions` fixtures, and the cause is visible in the bytes rather than
inferred. Three of them — of 6 706, 7 207 and 11 516 bytes — carry the
**byte-identical** CMS blob. One signature cannot cover three different
documents, so the suite copied a signature between files, which it had no
reason not to do: those fixtures test a permissions rule and say nothing about
signature validity.

Each of the four reports a **verified signature over a changed document**. That
is the combination a single boolean cannot express, and it is why questions 2
and 3 are asked separately rather than multiplied together. The same shape is
pinned by a fixture: flipping one byte inside `digitally-signed.pdf`'s covered
range turns its digest to `Differs` and leaves its signature `Verified`.

Two defects found on the way, both of the kind that produce a plausible wrong
answer rather than a crash:

**`/Contents` is the reservation, not the blob.** A signer reserves space
before layout and cannot shrink it, so the stored string is DER followed by
zero fill, and a DER parser refuses the whole of it as trailing bytes —
correctly. Every CMS blob in the corpus read as unreadable until
`Signature::cms()` trimmed the fill. It trims **only zeros**: bytes after the
declared length that are anything else are not fill, and trimming whatever
follows a signed structure would be a second reading of it.

**A certificate's `signatureAlgorithm` is a signature OID, not a digest OID.**
Resolving `sha256WithRSAEncryption` through the digest table returns nothing
for every certificate ever issued, and every chain in the corpus read
`Chain::Broken`. Worth recording because of how it failed: the walk did not
crash and did not accept — it reported a path that is not a path, which is the
failure mode a chain walk should have.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
|---|-------------|-------------------------------------|-----------------|
| 1 **done** | Signature inventory: `/ByteRange`/`/Contents` parsing, range digesting, coverage classification | `Document::signatures()` lists every signature in the fixture corpus with correct coverage; a flipped byte inside a covered range flips the digest verdict in a unit test; fuzzer on the parse path runs crash-free in CI | M |
| 2 **done** | `tinker-pdf-pki` DER walker + X.509 | Parses every certificate in the fixture corpus to the subject/issuer/validity/SPKI values committed in its sidecar, transcribed once from the certificate's own DER and reviewed; RFC 5280's own example certificates parse; dedicated fuzz target in the fuzz workspace; depth-capped, zero panics | M |
| 3 **done** | CMS `SignedData` parsing incl. signed attributes | RFC 5652 fixture set round-trips to expected values; `messageDigest` attribute extracted and re-digestable from exact DER; unknown OIDs yield typed refusals asserted by test | M |
| 4 | Big-unsigned + RSASSA-PKCS1-v1_5 verify in `tinker-pdf-crypto` | NIST CAVP RSA verify vectors (2048/3072/4096, SHA-256/384/512) pass as `cargo test` merge gate; forged-padding vectors rejected; RFC 8017 worked example passes | M |
| 5 | ECDSA P-256/P-384 verify | CAVP ECDSA verify vectors pass, including invalid-`r`/`s` and wrong-curve rejections; point-not-on-curve certificates refused with typed verdict | M |
| 6 **done** | End-to-end verdicts + trust anchors | Corpus of signed fixtures (valid, tampered, expired, self-signed) each matches its committed expected-verdict sidecar; anchor supplied → `AnchoredTo`, withheld → `SelfSigned`/`Incomplete`, asserted per fixture | M |
| 7 **done** | `/DocMDP` + `/FieldMDP` via `revisions()` | Fixtures: form-fill after certification level 2 → `PermittedChanges`; page edit after level 1 → `DisallowedChanges` naming the object; `/FieldMDP`-locked field edit detected; all as `cargo test` assertions | M |
| 8 **done** | Sign on incremental save: seam + `Signer` callback | Every signing test asserts `starts_with(original)`; independently re-digesting the returned `/ByteRange` spans matches the digest handed to the `Signer`; the signed file re-opens and verifies through this engine's own read side, and passes the strict structural validator; oversized CMS → typed refusal test | L |
| 9 **done** | Facade + FFI projection, warnings, docs | Verdict types exposed 1:1 through `tinker-pdf-ffi` (ruling 11) with parity tests; typed warnings carry object provenance (ruling 10) pinned by fixture; [features/forms.md](../features/forms.md) gains a signature-fields section; roadmap row closed against [ROADMAP.md](../ROADMAP.md) | M |

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
| ASN.1 parser panics or overreads on malformed DER (largest new untrusted surface) | Own leaf crate with dedicated fuzz target from milestone 2's first commit; depth caps; definite-length parsing by default, with the indefinite form opt-in per parse, constructed-only, budget-and-depth-bounded during its end-of-contents scan, and refused outright inside what §5.4 digests; ruling 1 makes a fuzz crash a release blocker |
| A verdict rendered as a single "valid" boolean misleads hosts into overtrusting | The API has no boolean: coverage, digest, chain, and MDP are separate typed fields, and the docs state the engine proves integrity, not identity — anchors are the host's assertion |
| Byte-range trickery: ranges that skip more than the `/Contents` gap make a "valid" signature over chosen bytes | Coverage classification is computed from the ranges, never trusted from them; anything but exact-gap bracketing to EOF or a clean revision boundary is `Suspicious` with a typed reason, fixture-pinned |
| MDP misclassification calls a benign form fill a disallowed change (or the reverse) | Classification reuses the tested `form.rs` field machinery rather than re-deriving object roles; fixtures for each `/P` level in both directions, each fixture's expected verdict written from the clause rather than from a run |
| Signing seam drifts from the prefix invariant and silently invalidates what it signs | `starts_with(original)` asserted in every signing test without exception (already the incremental writer's rule); the `/ByteRange` digest re-computed independently in tests |
| **Nobody outside this repository ever validates a signature it produced** (ruling 13), so this engine can sign confidently and wrongly | The primitives are pinned by published vectors, which is the part most likely to be subtly wrong. Assembly and placement are not adjudicated by anyone; interop is a dated one-time measurement recorded in this document, and the sentence about real validators stands in the feature doc as a permanent limit. **Not closed** |
