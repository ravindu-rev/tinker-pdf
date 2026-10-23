# Capability tier — signatures (private-key signing in the engine)

[The roadmap](../ROADMAP.md) says how a step runs, what comes next, and which dependencies cross tiers. Each row below keeps its text as it was planned — Item, Today, Exit criterion, Doc, Depends on — and is cut into steps, each one workflow run.

The owner's instruction puts signing with a private key in the engine. That
reverses what [design/signatures.md](../design/signatures.md),
[features/signatures.md](../features/signatures.md),
[design/pubsec.md](../design/pubsec.md) and `SECURITY.md` stated as a
non-goal — "no key parsing … no RSA/ECDSA signing arithmetic", "a boundary
a future signing feature would have to cross deliberately, not drift
across" — and each of them now points here; the ruling that records the
crossing is the owner's (H-09). `bignum` today skips constant-time
discipline because every value it sees is public; from SG-01 on, every
secret operand takes the constant-time path. The engine still links no
random number generator: entropy is the caller's, and ECDSA nonces are
deterministic by default.

## SG-01 — RSA signing in the engine

**RSA signing in the engine**: PKCS#1 v1.5 and RSASSA-PSS over SHA-256, SHA-384, SHA-512, SHA-512/256 and (with SG-04) SHA3-256/384/512, the digest the caller's choice; CRT with blinding, constant-time fixed-window Montgomery exponentiation with no secret-dependent branch or table index, key material zeroised on drop

**Today.** verify-only (`crates/tinker-pdf-crypto/src/rsa.rs`, `crates/tinker-pdf-crypto/src/bignum.rs`); RSA-2048 private operation 6.0 ms without CRT (dated)

**Exit criterion.** NIST CAVP signature-generation vectors (PKCS#1 v1.5, and PSS with a fixed salt) for every SHA-2 digest they cover and the Wycheproof RSA signature vectors (Apache-2.0) committed and passing, the SHA-3 pairs once SG-04 lands; an operation-count test shows no secret-dependent branch counts; the speed is P-13's; the side-channel review is H-67

**Doc:** `design/signing.md` (owed; amends [design/signatures.md](../design/signatures.md)) · **Depends on:** H-32

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-02 — ECDSA signing on P-256, P-384, P-521 and Brainpool P256r1, P384r1 and P512r1

**ECDSA signing on P-256, P-384, P-521 and Brainpool P256r1, P384r1 and P512r1**, over SHA-256, SHA-384, SHA-512 and (with SG-04) SHA-3, the digest the caller's choice, with RFC 6979 deterministic nonces by default and caller entropy as an option (hedged), constant-time scalar multiplication on complete formulas; P-521, the Brainpool curves and compressed points verified too

**Today.** verify-only on P-256 and P-384; P-521, the Brainpool curves and compressed points refused (`Unchecked::UnsupportedKey`, naming the curve's OID, `features/signatures.md:318-319`)

**Exit criterion.** RFC 6979 Appendix A.2.5–A.2.7 signatures reproduced byte for byte for every digest the appendix lists; NIST CAVP signature-generation and Wycheproof ECDSA vectors pass on the NIST curves; Wycheproof's Brainpool vectors pass on the verify side, every deterministic Brainpool signature the signer makes over RFC 6979's messages verifies there, and the curve parameters are checked against RFC 5639; a signed document's bytes identical on four targets (K-07); **a P-256 signature ≤ 0.5 ms, P-384 ≤ 1.2 ms, P-521 ≤ 2.5 ms, Brainpool P-256 ≤ 1 ms, constant-time** (tier P, calibrated)

**Doc:** `design/signing.md` (owed) · **Depends on:** SG-01, SG-04, H-33

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-03 — Ed25519 and Ed448

**Ed25519 and Ed448** signing and verification, carried in CMS by RFC 8419 (ISO/TS 32002)

**Today.** absent — the owner's parity decision of 9 October 2026; an EdDSA key met while verifying is `Unchecked::UnsupportedKey`, naming its OID (`features/signatures.md:318`)

**Exit criterion.** RFC 8032's test vectors; Wycheproof EdDSA vectors

**Doc:** `design/signing.md` (owed) · **Depends on:** SG-05

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-04 — Hash and curve breadth

**Hash and curve breadth**: SHA-3-256/384/512 and SHAKE256, SHA-512/256 (ISO/TS 32001); Brainpool P256r1, P384r1 and P512r1 (SG-02 signs with them); DSA verification for legacy signatures; RSA moduli up to 16 384 bits

**Today.** SHA-1, SHA-2 and MD5 only (`tinker-pdf-crypto/src`); a Brainpool key is `Unchecked::UnsupportedKey`, naming the curve's OID (`features/signatures.md:318`)

**Exit criterion.** NIST SHA3VS vectors; the curves' parameters checked against RFC 5639 and Wycheproof's Brainpool vectors; CAVP DSA verification vectors

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-05 — Key import

**Key import**: PKCS#8 plain and encrypted (PBES2 with PBKDF2-HMAC-SHA-256 or SHA-1 and AES-CBC; PBES1), PKCS#12 (PBES2, and the legacy SHA-1 triple-DES and 40-bit RC2 schemes; the MAC by the PKCS#12 key derivation with HMAC-SHA-1 or SHA-256), PKCS#1 `RSAPrivateKey`, SEC 1 `ECPrivateKey`, PEM armour, certificate chains extracted; passphrases as bytes, never logged, zeroised

**Today.** "Key material of any kind" was a non-goal until 9 October 2026 (`design/pubsec.md:62-66`); `tinker-pdf-pki` has no PKCS#8 or PKCS#12 reader

**Exit criterion.** RFC 6070's PBKDF2 vectors; RFC 7292 structures; key files generated once by an outside tool with known passphrases committed with provenance (bytes only, dated), each held to its certificate's public key in the test; malformed containers are typed errors under fuzzing

**Doc:** `design/signing.md` (owed) · **Depends on:** H-32, H-34

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-06 — CMS `SignedData` built in the engine

**CMS `SignedData` built in the engine**: signed attributes (content type, message digest, ESS signing-certificate-v2, optional signing time), the certificate chain, unsigned attributes (timestamp token), DER's canonical `SET OF` ordering; sub-filters `adbe.pkcs7.detached` and `ETSI.CAdES.detached`; the existing `Signer` (finished CMS) kept, and a `RawSigner` seam beside it for keys held in hardware or a remote service (the host returns a raw signature over the to-be-signed attributes)

**Today.** `save_signed` reserves `/Contents`, patches `/ByteRange` and hands the digest to a host `Signer` that returns finished CMS (`features/signatures.md:158-166`)

**Exit criterion.** every signed output verifies through the engine's own verdict with the digest, signature, chain and attributes all `Ok`; the DER checked against X.690's canonical rules by the in-tree DER validator; structures transcribed from RFC 5652; the interoperability record is a dated measurement (H-68)

**Doc:** `design/signing.md` (owed) · **Depends on:** SG-01, SG-02, SG-05

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-07 — PAdES baseline levels B-B, B-T, B-LT and B-LTA

**PAdES baseline levels B-B, B-T, B-LT and B-LTA**: orchestration (signing-certificate-v2 mandatory, `/M` rather than a signing time, a signature timestamp through the host `Timestamper` for B-T, DSS and VRI with the chain's revocation data for B-LT, a document timestamp for B-LTA) and a level detector on the validation side

**Today.** the parts exist (DSS writer, document timestamp, host timestamper); "PAdES-LTA conformance levels are out" was a non-goal of the design until 9 October 2026 (`design/signatures.md:487-490`)

**Exit criterion.** the level detector, its rules transcribed clause by clause from EN 319 142-1 (free) with a fixture and twin per rule, classifies the engine's own output at each level and the corpus's signatures

**Doc:** `design/signing.md` (owed) · **Depends on:** SG-06, SG-10

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-08 — `tpdf sign`, `verify`, `timestamp` and `ltv`

**`tpdf sign`, `verify`, `timestamp` and `ltv`**: sign with a PKCS#12, or a PKCS#8 key and chain, the passphrase from a file, the environment or a prompt (never the command line); a field by name or a new visible field; certification level; PAdES level; a two-phase file workflow for timestamp tokens and revocation data the CLI does not fetch

**Today.** `sign` was left out of the CLI "while signing keys are a non-goal": signing is a private-key operation, keys in the engine were a non-goal of [design/signatures.md](../design/signatures.md) until 9 October 2026, and the `Signer` seam takes finished CMS bytes, which a command line cannot produce without the signing arithmetic that non-goal declined; there is no `verify` subcommand

**Exit criterion.** `tools/tpdf` tests sign the `testdata/` fixtures with committed test keys; outputs verify through the engine's verdict and reopen through the facade; the passphrase never appears in a process listing or log

**Doc:** — · **Depends on:** SG-07

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-09 — Signing across the C ABI and every binding

**Signing across the C ABI and every binding**: in-engine keys need no callback (key bytes and passphrase in, signed bytes out); two-phase deferred signing (a placeholder and digest out with resumable state, then CMS or a raw signature in, without re-serialising) for keys the engine never sees; and a signing callback across the C ABI, with timestamper and entropy callbacks beside it

**Today.** `save_signed`, `Signer`, `Timestamper` and `EntropySource` are not projected: "no `tpdf_*` entry point takes a callback" (`features/bindings.md:1046, 1049`; `design/bindings-write.md:51-57`)

**Exit criterion.** `bindings-parity` prints the hash of a deterministically signed document (PKCS#1 v1.5 and RFC 6979), identical through C, Python, JavaScript, .NET, Go, Ruby and Java, by each of the three routes

**Doc:** [design/bindings-write.md](../design/bindings-write.md) (amend) · **Depends on:** SG-06, H-28, H-34

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-10 — Revocation, parsed and evaluated

**Revocation, parsed and evaluated**: CRLs (RFC 5280 §5 and §6.3 — issuing distribution points, delta CRLs, reasons, indirect CRLs) and OCSP responses (RFC 6960 — responder authorisation, `id-pkix-ocsp-nocheck`, `thisUpdate`/`nextUpdate` against the validation time, nonces); OCSP request bytes built for the host to send; AIA and distribution-point URLs extracted; `adbe-revocationInfoArchival` read and written; the verdict consults the DSS. Fetching stays the host's

**Today.** "nothing parses a CRL or OCSP response, and the verdict does not consult the store" (`features/signatures.md:182-201`); fetching a non-goal until 9 October 2026 (`design/signatures.md:469-481`); the engine still performs no I/O (NG-08)

**Exit criterion.** the NIST public-key interoperability test suite's CRL cases pass (public test data); OCSP structures transcribed from RFC 6960 with responses generated once (dated, bytes only); the corpus's embedded revocation data evaluated and recorded

**Doc:** `design/signature-validation.md` (owed) · **Depends on:** SG-11

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-11 — Certification path validation, complete

**Certification path validation, complete** (RFC 5280 §6): validity at the validation time, key usage and extended key usage, basic and name constraints, policy processing, revocation; caller-supplied anchors

**Today.** chain building to caller-supplied anchors at a caller-supplied instant (`features/signatures.md:97-104`); expiry and key-usage judgements partial

**Exit criterion.** the NIST public-key interoperability test suite's path-validation cases pass, the count ratcheted

**Doc:** `design/signature-validation.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-12 — Validation per EN 319 102-1

**Validation per EN 319 102-1**: basic, with-time and long-term validation; indications passed, failed and indeterminate with their sub-indications; proof of existence from timestamps; algorithm-expiry constraints (TS 119 312); a report shaped as TS 119 102-2; the clock a parameter (ruling 4)

**Today.** a four-question verdict with no boolean

**Exit criterion.** each sub-indication reached by a fixture built for it; the report validates against the published XML schema in the test

**Doc:** `design/signature-validation.md` (owed) · **Depends on:** SG-10, SG-11, SG-18

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-13 — XML signatures and Exclusive XML Canonicalisation

**XML signatures and Exclusive XML Canonicalisation**, in-tree, verified and created: trust lists verified; XFA signatures, EPUB `signatures.xml` and XPS signatures verified and created with SG-01's and SG-02's in-engine keys

**Today.** absent — the owner's parity decision of 9 October 2026; no XML canonicalisation or `ds:Signature` reader exists (`rg -i 'c14n|xmldsig|SignedInfo' crates` finds nothing)

**Exit criterion.** the W3C canonicalisation test cases and XML-signature interoperability examples pass on the verify side; where an example publishes its private key, the engine's signature over the example's input reproduces the example's `SignatureValue` byte for byte (PKCS#1 v1.5 is deterministic); every signature the engine creates verifies through its own verifier, its canonical `SignedInfo` equal to the one computed in the test

**Doc:** — · **Depends on:** SG-01, SG-02 (creation)

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-14 — Trust lists

**Trust lists**: the EU list of lists and national trusted lists (TS 119 612, signed), a vendor's approved-trust-list format, PEM bundles — ingested from bytes the host supplies; "qualified at time T" answered from service status history

**Today.** no bundled root store by policy (`features/signatures.md:315`); no list format read

**Exit criterion.** published lists' structures parsed and their signatures verified (SG-13); service-status history held to fixtures with known transitions

**Doc:** `design/trust-lists.md` (owed) · **Depends on:** SG-13, H-27

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-15 — Seed values and locks

**Seed values and locks**: `/SV` constraints honoured and reported (filter, sub-filter, digest, certificate constraints, reasons, MDP, timestamp, legal attestation, revocation, lock document, appearance filter); `/Lock` applied when a field is signed; MDP version 2 `/P`; a query for the fields a given signature locks and whether a field is locked, per revision

**Today.** locks on certification exist; `/SV` is not read (no source hit for `SV`)

**Exit criterion.** each constraint a fixture whose signing request is refused or adjusted as 12.8.1.3 states; the lock query answers fixtures signed with `All`, `Include` and `Exclude` locks field by field; `tpdf forms lock` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** SG-06

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-16 — Signature appearances, complete

**Signature appearances, complete**: legacy layered `n0`/`n2` appearances, configurable layouts, a handwritten signature image with alpha, a vector logo, localised dates, signer names in any script through a CID font and shaping, and a preview before signing

**Today.** the visible seal sets Helvetica in `WinAnsiEncoding` and draws a character above the single-byte range as a substitute (`features/signatures.md:210-211`)

**Exit criterion.** a reviewed golden family per layout (K-08, H-73); non-Latin names extract as written

**Doc:** — · **Depends on:** ED-45

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-17 — Clear a signature

**Clear a signature**: revert a signed field to unsigned in a new revision

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'clear_signature|remove_signature' crates tools` finds nothing

**Exit criterion.** the cleared field reports unsigned and the earlier revision still verifies

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-18 — Revision analysis and attack detection, on by default

**Revision analysis and attack detection, on by default**: enumerate revisions; extract the signed revision; diff later revisions by value and classify each change as allowed or not under DocMDP and FieldMDP; detect shadow attacks (hide, replace, hide-and-replace through overlays, optional content, annotations, font swaps), incremental-saving attacks, signature wrapping and `/ByteRange` tricks; a visual diff of signed against final

**Today.** `modifications()` compares cross-reference entries by object (`features/signatures.md:150-157`)

**Exit criterion.** a fixture per published attack class, built here from the class's description, is flagged; every benign incremental edit in the corpus is not

**Doc:** `design/revision-analysis.md` (owed) · **Depends on:** ED-31, ED-23

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-19 — Usage-rights signatures

**Usage-rights signatures**: `/Perms /UR3` verified with its transform parameters, the granted rights reported, stripped safely when an edit would invalidate them

**Today.** `/Perms` signatures are found and classified (`features/signatures.md:25-63`)

**Exit criterion.** fixtures with each rights flag; stripping leaves a document that validates and keeps every other signature valid

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-20 — Edit pre-flight for signed documents

**Edit pre-flight for signed documents**: "will this edit invalidate signature X, or break DocMDP P=2?" answered before saving, and incremental mode forced

**Today.** `check_save` answers encryption questions only

**Exit criterion.** a matrix of edits over certified fixtures predicted correctly against the verdict after saving

**Doc:** — · **Depends on:** SG-18

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-21 — Detached signatures outside the PDF

**Detached signatures outside the PDF**: CAdES and ASiC sidecars over a PDF or its attachments; `.p7s` verification

**Today.** absent — the owner's parity decision of 9 October 2026; `rg '\.p7s|ASiC' crates tools` finds nothing

**Exit criterion.** sidecars generated by SG-06's builder verify; structures transcribed from the ETSI standards

**Doc:** — · **Depends on:** SG-06, SG-13

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-22 — Signature policies and attributes

**Signature policies and attributes**: policy identifiers, commitment types, claimed and certified signer roles, content timestamps, counter-signatures

**Today.** absent — the owner's parity decision of 9 October 2026; the CMS reader names a countersignature only to say it is not covered (`crates/tinker-pdf-pki/src/cms.rs:1121`); no policy, commitment-type or role attribute is read or written

**Exit criterion.** each attribute written, re-read and reported by the verdict

**Doc:** — · **Depends on:** SG-06

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-23 — Keys and certificates made here

**Keys and certificates made here**: RSA and EC key generation from caller entropy, self-signed certificates, PKCS#10 requests, PKCS#12 export — so signing fixtures can be made in the tree

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'generate_key|CertificationRequest' crates tools` finds nothing, and `keygen` only as an HTML element name

**Exit criterion.** generated keys sign and verify; the exported PKCS#12 re-imports through SG-05; certificate fields held to RFC 5280 structures

**Doc:** — · **Depends on:** SG-05, H-33

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-24 — Remote and batch signing preparation

**Remote and batch signing preparation**: to-be-signed hashes for remote signing services, batched over many documents under one authorisation

**Today.** the two-phase route (SG-09) is the engine side

**Exit criterion.** a batch of 100 documents prepared, signed by a test host and completed verifies document by document

**Doc:** — · **Depends on:** SG-09

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-25 — Post-quantum signatures and recipients

**Post-quantum signatures and recipients**: ML-DSA and SLH-DSA in CMS; ML-KEM recipients for public-key encryption; composite signatures

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'ml.dsa|slh.dsa|ml.kem|dilithium|kyber' crates tools` finds nothing

**Exit criterion.** the NIST post-quantum test vectors pass; signed documents verify through the verdict

**Doc:** `design/post-quantum.md` (owed) · **Depends on:** SG-06, SE-07

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-26 — The signatures corpus measurement

**The signatures corpus measurement**

**Today.** **Every named shape has left; one corpus measurement is owed.** RSASSA-PSS has left — verified against 360 CAVP `SigVerPSS` vectors, RSA Laboratories' 60 and a document an outside toolkit signed once (`signature_shapes.rs`). A signature with no signed attributes has left too — checked over the covered bytes' own digest, against a no-attributes fixture the same toolkit made; the corpus's one such signer (`bug854315.pdf`) is owed its first measured verdict, which `verdicts.rs` prints and tallies apart. `adbe.pkcs7.sha1` has left — both links of its encapsulated digest checked, against two such fixtures; its one corpus file is a fuzzer's mutation whose coverage does not hold up, so no corpus count moves. `GeneralNames` have left — decoded in the alternative-name extensions, `authorityCertIssuer` and an ESS `issuerSerial`, against RFC 5280 C.2 and an outside CAdES signature. RFC 3161 signature timestamps have left — imprint, signature, `TSTInfo` digest, the authority certificate's EKU and ESS binding, and its chain at `genTime`, against a token an outside timestamp authority made once. **What is left is measurement, not work**, none of it taken because the corpora could not be fetched where these landed: `verdicts.rs`'s pin for `bug854315.pdf`'s unattributed signer — its assertion that the signature is no longer `NotChecked` is an expectation nobody has measured, and would fail on an MD5 digest, unusable coverage or a missing certificate; what the verdict makes of the corpus's seven timestamp tokens; and a re-run of `cms_census.rs`, whose pins (27 blobs parsed, 7 nested tokens) predate the strict decoding of RFC 2634's first-version `signingCertificate`, which fails a whole `ContentInfo` on a malformed one as the second version always did, and which RFC 3161 tokens normally carry ([features/signatures.md](../features/signatures.md); [design/signatures.md](../design/signatures.md))

**Exit criterion.** the census's first run with these, recorded and pinned

**Doc:** — · **Depends on:** MS-13

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-27 — A PKCS#11 signing provider

**A PKCS#11 signing provider** for keys held in tokens, smart cards and HSMs: a `RawSigner` for `tpdf sign` and the bindings that loads the PKCS#11 module a caller names, finds a key by label or id, and signs SG-06's to-be-signed attributes — kept outside the engine crates, as FT-13's font provider is, because loading a module needs the platform's loader and `unsafe`

**Today.** absent — the owner's parity decision of 9 October 2026; SG-06 adds the `RawSigner` seam and nothing implements it for a token (`rg -i 'pkcs#\?11|cryptoki' crates tools` finds nothing)

**Exit criterion.** CI signs `testdata/` fixtures through a software token that holds a generated key (an outside program hosts the key and executes the operation; it judges nothing) and the engine's own verdict verifies every output; a refused operation, a wrong PIN and a missing key are typed errors; `tpdf sign --pkcs11` runs it; real hardware is H-117's confirmation

**Doc:** — · **Depends on:** SG-06, SG-08, H-34

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SG-28 — Operating-system key stores as signing providers

**Operating-system key stores as signing providers**: Windows CNG and its certificate store, the macOS keychain, and the Android Keystore through the Android binding — each a `RawSigner` outside the engine crates, the key never leaving the store

**Today.** absent — the owner's parity decision of 9 October 2026; no platform key store is reached (`rg -i 'keychain|keystore|\bcng\b' crates tools bindings` finds nothing)

**Exit criterion.** CI legs on Windows and macOS generate a non-exportable key into the runner's own store, sign `testdata/` fixtures through it, and the engine's verdict verifies the outputs; the Android provider signs in the AAR's emulator test; only a signature value crosses from the store

**Doc:** — · **Depends on:** SG-27, SU-15, H-34

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*
