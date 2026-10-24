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

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-01.1 | design | Create `docs/design/signing.md`, amending `docs/design/signatures.md`'s non-goals: the constant-time path for every secret operand, CRT with blinding from caller entropy, zeroisation, RSA signing, ECDSA, EdDSA, key import, CMS `SignedData`, PAdES levels, and the ruling H-09 asks the owner to add, drafted. | `docs/design/signing.md` (new), `docs/design/signatures.md` | The doc exists in the design-doc order, names this row's exit tests and the milestones SG-02, SG-03, SG-05, SG-06 and SG-07 extend, and is reviewed. | H-32, H-09 |
| SG-01.2 | build | The bignum's constant-time path: fixed-window Montgomery exponentiation with every table entry read, no branch or index on a secret. | `crates/tinker-pdf-crypto/src/bignum.rs` | An operation-count test (counters under `cfg(test)`) shows branch and lookup counts independent of the exponent's bits; failing first on today's exponentiation. | SG-01.1 |
| SG-01.3 | test | The signature-generation vectors vendored with provenance: NIST CAVP's PKCS#1 v1.5 and PSS vectors and the Wycheproof RSA signature vectors (Apache-2.0), with their licence records. | `crates/tinker-pdf-crypto/tests/data` (new), `THIRDPARTY.md`, `.gitattributes` | The vectors load in a test that prints their counts; their licence is recorded. | SG-01.1, H-92 |
| SG-01.4 | build | The RSA private operation by CRT with blinding from caller entropy, and key material zeroised on drop. | `crates/tinker-pdf-crypto/src/rsa.rs`, `crates/tinker-pdf-crypto/src/bignum.rs` | Unit tests: the CRT result equals the plain private operation on every vector key; a blinded run equals an unblinded one; a key's limbs read zero after drop in a test harness. | SG-01.2 |
| SG-01.5 | build | PKCS#1 v1.5 signature generation over SHA-256, SHA-384, SHA-512 and SHA-512/256, the digest the caller's choice. | `crates/tinker-pdf-crypto/src/rsa.rs`, `crates/tinker-pdf-crypto/tests/rsa_sign.rs` (new) | `rsa_sign.rs` (new): CAVP's PKCS#1 v1.5 generation vectors and Wycheproof's signature vectors pass for every SHA-2 digest they cover. | SG-01.3, SG-01.4 |
| SG-01.6 | build | RSASSA-PSS generation, the salt fixed for vectors or drawn from caller entropy. | `crates/tinker-pdf-crypto/src/rsa.rs`, `crates/tinker-pdf-pki/src/pss.rs`, `crates/tinker-pdf-crypto/tests/rsa_sign.rs` (new) | CAVP's PSS vectors with a fixed salt pass for every SHA-2 digest. | SG-01.5 |
| SG-01.7 | test | The SHA-3 pairs of both schemes once SG-04's digests exist. | `crates/tinker-pdf-crypto/tests/rsa_sign.rs` (new) | The SHA-3 vectors pass for PKCS#1 v1.5 and PSS. | SG-01.6, SG-04 |
| SG-01.8 | watch | The private-key path's independent side-channel review. | `docs/features/signatures.md` | H-67's review is recorded, dated, in the feature doc. | SG-01.7, H-67 |

**Closes the row:** SG-01.5 to SG-01.7 pass CAVP's and Wycheproof's vectors, SG-01.2's operation-count test shows no secret-dependent count, and SG-01.8 records H-67's review, which is the row's exit; the speed is P-13's.

## SG-02 — ECDSA signing on P-256, P-384, P-521 and Brainpool P256r1, P384r1 and P512r1

**ECDSA signing on P-256, P-384, P-521 and Brainpool P256r1, P384r1 and P512r1**, over SHA-256, SHA-384, SHA-512 and (with SG-04) SHA-3, the digest the caller's choice, with RFC 6979 deterministic nonces by default and caller entropy as an option (hedged), constant-time scalar multiplication on complete formulas; P-521, the Brainpool curves and compressed points verified too

**Today.** verify-only on P-256 and P-384; P-521, the Brainpool curves and compressed points refused (`Unchecked::UnsupportedKey`, naming the curve's OID, `features/signatures.md:318-319`)

**Exit criterion.** RFC 6979 Appendix A.2.5–A.2.7 signatures reproduced byte for byte for every digest the appendix lists; NIST CAVP signature-generation and Wycheproof ECDSA vectors pass on the NIST curves; Wycheproof's Brainpool vectors pass on the verify side, every deterministic Brainpool signature the signer makes over RFC 6979's messages verifies there, and the curve parameters are checked against RFC 5639; a signed document's bytes identical on four targets (K-07); **a P-256 signature ≤ 0.5 ms, P-384 ≤ 1.2 ms, P-521 ≤ 2.5 ms, Brainpool P-256 ≤ 1 ms, constant-time** (tier P, calibrated)

**Doc:** `design/signing.md` (owed) · **Depends on:** SG-01, SG-04, H-33

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-02.1 | design | Add ECDSA to `docs/design/signing.md`: constant-time field arithmetic per curve, complete addition formulas, scalar multiplication without secret-dependent branches, RFC 6979 and hedged nonces. | `docs/design/signing.md` | The section names SG-02.2 to SG-02.7's exit tests and is reviewed. | SG-01.1, H-33 |
| SG-02.2 | build | Constant-time field arithmetic for the P-256, P-384, P-521 and Brainpool primes on the bignum's constant-time path. | `crates/tinker-pdf-crypto/src/ecdsa.rs`, `crates/tinker-pdf-crypto/src/bignum.rs` | Unit tests hold each field's operations to values the test computes; an operation-count test shows counts independent of operands. | SG-02.1, SG-04 |
| SG-02.3 | build | Complete addition formulas and a constant-time scalar multiplication. | `crates/tinker-pdf-crypto/src/ecdsa.rs` | Unit tests: multiples of the generator equal the published values; an operation-count test shows counts independent of the scalar's bits. | SG-02.2 |
| SG-02.4 | build | Verification extended to P-521 and compressed points, retiring `Unchecked::UnsupportedKey` for them. | `crates/tinker-pdf-crypto/src/ecdsa.rs`, `crates/tinker-pdf-pki/src/cms.rs`, `docs/features/signatures.md` | Wycheproof's verification vectors pass on P-521 and with compressed points; failing first on today's refusal. | SG-02.3 |
| SG-02.5 | build | Signing with RFC 6979 deterministic nonces by default and caller entropy as a hedged option. | `crates/tinker-pdf-crypto/src/ecdsa.rs`, `crates/tinker-pdf-crypto/tests/ecdsa_sign.rs` (new) | `ecdsa_sign.rs` (new): RFC 6979 A.2.5 to A.2.7's signatures are reproduced byte for byte for every digest the appendix lists. | SG-02.3 |
| SG-02.6 | test | CAVP's generation and Wycheproof's ECDSA vectors on the NIST curves, Brainpool's deterministic signatures verified, and a signed document fingerprinted. | `crates/tinker-pdf-crypto/tests/ecdsa_sign.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | The vectors pass; every deterministic Brainpool signature over RFC 6979's messages verifies; the signed document's bytes are identical on every K-07 target. | SG-02.4, SG-02.5, K-07 |
| SG-02.7 | measure | The speed targets: a P-256 signature at most 0.5 ms, P-384 1.2 ms, P-521 2.5 ms and Brainpool P-256 1 ms, constant-time, as calibrated tier-P targets. | `crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/benches/baseline.json` | MS-02's paired job holds the four targets. | SG-02.6, MS-04, MS-02 |

**Closes the row:** SG-02.5 reproduces RFC 6979's signatures, SG-02.6 passes CAVP's and Wycheproof's vectors with a fingerprint on four targets, and SG-02.7 holds the speed targets, which is the row's exit.

## SG-03 — Ed25519 and Ed448

**Ed25519 and Ed448** signing and verification, carried in CMS by RFC 8419 (ISO/TS 32002)

**Today.** absent — the owner's parity decision of 9 October 2026; an EdDSA key met while verifying is `Unchecked::UnsupportedKey`, naming its OID (`features/signatures.md:318`)

**Exit criterion.** RFC 8032's test vectors; Wycheproof EdDSA vectors

**Doc:** `design/signing.md` (owed) · **Depends on:** SG-05

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-03.1 | design | Add EdDSA to `docs/design/signing.md`: Curve25519's and Edwards448's arithmetic on the constant-time path, and RFC 8419's carriage in CMS. | `docs/design/signing.md` | The section names SG-03.2 to SG-03.4's exit tests and is reviewed. | SG-01.1 |
| SG-03.2 | build | Ed25519 signing and verification. | `crates/tinker-pdf-crypto/src/eddsa.rs` (new), `crates/tinker-pdf-crypto/src/lib.rs`, `crates/tinker-pdf-crypto/tests/eddsa.rs` (new) | `eddsa.rs` (new): RFC 8032's Ed25519 vectors and Wycheproof's pass. | SG-03.1, SG-05 |
| SG-03.3 | build | Ed448 signing and verification over SHAKE256. | `crates/tinker-pdf-crypto/src/eddsa.rs` (new), `crates/tinker-pdf-crypto/tests/eddsa.rs` (new) | RFC 8032's Ed448 vectors and Wycheproof's pass. | SG-03.2, SG-04 |
| SG-03.4 | build | EdDSA in CMS by RFC 8419, signing and verifying, retiring `Unchecked::UnsupportedKey` for EdDSA keys. | `crates/tinker-pdf-pki/src/cms.rs`, `crates/tinker-pdf/tests/signatures_eddsa.rs` (new), `docs/features/signatures.md` | `signatures_eddsa.rs` (new): documents signed with each key verify through the engine's verdict; failing first on today's refusal. | SG-03.3, SG-06 |

**Closes the row:** SG-03.2 and SG-03.3 pass RFC 8032's and Wycheproof's vectors, and SG-03.4 carries them in CMS, which is the row's exit.

## SG-04 — Hash and curve breadth

**Hash and curve breadth**: SHA-3-256/384/512 and SHAKE256, SHA-512/256 (ISO/TS 32001); Brainpool P256r1, P384r1 and P512r1 (SG-02 signs with them); DSA verification for legacy signatures; RSA moduli up to 16 384 bits

**Today.** SHA-1, SHA-2 and MD5 only (`tinker-pdf-crypto/src`); a Brainpool key is `Unchecked::UnsupportedKey`, naming the curve's OID (`features/signatures.md:318`)

**Exit criterion.** NIST SHA3VS vectors; the curves' parameters checked against RFC 5639 and Wycheproof's Brainpool vectors; CAVP DSA verification vectors

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-04.1 | build | SHA-3-256, -384 and -512 and SHAKE256 on Keccak-f[1600]. | `crates/tinker-pdf-crypto/src/sha3.rs` (new), `crates/tinker-pdf-crypto/src/lib.rs`, `crates/tinker-pdf-crypto/tests/sha3.rs` (new) | `sha3.rs` (new): NIST SHA3VS vectors pass. | — |
| SG-04.2 | build | SHA-512/256. | `crates/tinker-pdf-crypto/src/sha2.rs` | CAVP's SHA-512/256 vectors pass. | — |
| SG-04.3 | build | Brainpool P256r1, P384r1 and P512r1 for verification, retiring `Unchecked::UnsupportedKey` for them. | `crates/tinker-pdf-crypto/src/ecdsa.rs`, `docs/features/signatures.md` | The parameters equal RFC 5639's; Wycheproof's Brainpool verification vectors pass; failing first on today's refusal. | — |
| SG-04.4 | build | DSA verification for legacy signatures. | `crates/tinker-pdf-crypto/src/dsa.rs` (new), `crates/tinker-pdf-pki/src/cms.rs` | CAVP's DSA verification vectors pass. | — |
| SG-04.5 | build | RSA moduli up to 16 384 bits, the limit a ledger row. | `crates/tinker-pdf-crypto/src/rsa.rs`, `crates/tinker-pdf/tests/bounds_ledger.rs` | 8 192- and 16 384-bit signatures verify; a larger modulus is a typed refusal; the limit is a ledger row. | — |

**Closes the row:** SG-04.1 to SG-04.4 pass SHA3VS, the SHA-512/256 vectors, RFC 5639's parameters with Wycheproof's Brainpool vectors and CAVP's DSA vectors, and SG-04.5 raises the modulus limit, which is the row's exit.

## SG-05 — Key import

**Key import**: PKCS#8 plain and encrypted (PBES2 with PBKDF2-HMAC-SHA-256 or SHA-1 and AES-CBC; PBES1), PKCS#12 (PBES2, and the legacy SHA-1 triple-DES and 40-bit RC2 schemes; the MAC by the PKCS#12 key derivation with HMAC-SHA-1 or SHA-256), PKCS#1 `RSAPrivateKey`, SEC 1 `ECPrivateKey`, PEM armour, certificate chains extracted; passphrases as bytes, never logged, zeroised

**Today.** "Key material of any kind" was a non-goal until 9 October 2026 (`design/pubsec.md:62-66`); `tinker-pdf-pki` has no PKCS#8 or PKCS#12 reader

**Exit criterion.** RFC 6070's PBKDF2 vectors; RFC 7292 structures; key files generated once by an outside tool with known passphrases committed with provenance (bytes only, dated), each held to its certificate's public key in the test; malformed containers are typed errors under fuzzing

**Doc:** `design/signing.md` (owed) · **Depends on:** H-32, H-34

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-05.1 | design | Add key import to `docs/design/signing.md`: each container and scheme, passphrases as bytes that are never logged and are zeroised, and the committed key files' provenance. | `docs/design/signing.md` | The section names SG-05.2 to SG-05.5's exit tests and is reviewed. | SG-01.1, H-32, H-34 |
| SG-05.2 | build | PBKDF2 with HMAC-SHA-256 and SHA-1, PBES2 with AES-CBC, and PBES1. | `crates/tinker-pdf-pki/src/pbe.rs` (new), `crates/tinker-pdf-pki/src/lib.rs` | Unit tests: RFC 6070's PBKDF2 vectors pass; PBES2 and PBES1 structures decrypt as their examples state. | SG-05.1 |
| SG-05.3 | build | PKCS#8 plain and encrypted, PKCS#1 `RSAPrivateKey`, SEC 1 `ECPrivateKey` and PEM armour. | `crates/tinker-pdf-pki/src/keys.rs` (new), `crates/tinker-pdf-pki/tests/keys.rs` (new) | `keys.rs` (new): each committed key file, generated once by an outside tool with a known passphrase and committed with dated provenance, reads to the key its certificate's public key matches. | SG-05.2, H-106 |
| SG-05.4 | build | PKCS#12: safe contents and bags, PBES2 and the legacy SHA-1 triple-DES and 40-bit RC2 schemes, the MAC by PKCS#12's key derivation with HMAC-SHA-1 or SHA-256, and certificate chains extracted. | `crates/tinker-pdf-pki/src/pkcs12.rs` (new), `crates/tinker-pdf-crypto/src/rc2.rs` (new), `crates/tinker-pdf-pki/tests/keys.rs` (new) | RFC 7292's structures read; each committed `.p12` yields its key and chain, held to the certificate's public key. | SG-05.3 |
| SG-05.5 | test | A fuzz target over every key container. | `fuzz/fuzz_targets/key_import.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md` | Malformed containers are typed errors under fuzzing; the target runs clean and the roster is regenerated. | SG-05.4 |

**Closes the row:** SG-05.2 to SG-05.4 pass RFC 6070's vectors and RFC 7292's structures and hold each committed key to its certificate, and SG-05.5 fuzzes the containers, which is the row's exit.

## SG-06 — CMS `SignedData` built in the engine

**CMS `SignedData` built in the engine**: signed attributes (content type, message digest, ESS signing-certificate-v2, optional signing time), the certificate chain, unsigned attributes (timestamp token), DER's canonical `SET OF` ordering; sub-filters `adbe.pkcs7.detached` and `ETSI.CAdES.detached`; the existing `Signer` (finished CMS) kept, and a `RawSigner` seam beside it for keys held in hardware or a remote service (the host returns a raw signature over the to-be-signed attributes)

**Today.** `save_signed` reserves `/Contents`, patches `/ByteRange` and hands the digest to a host `Signer` that returns finished CMS (`features/signatures.md:158-166`)

**Exit criterion.** every signed output verifies through the engine's own verdict with the digest, signature, chain and attributes all `Ok`; the DER checked against X.690's canonical rules by the in-tree DER validator; structures transcribed from RFC 5652; the interoperability record is a dated measurement (H-68)

**Doc:** `design/signing.md` (owed) · **Depends on:** SG-01, SG-02, SG-05

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-06.1 | design | Add CMS `SignedData` to `docs/design/signing.md`: the attributes, DER's canonical `SET OF` order, the sub-filters, and the `RawSigner` seam beside today's `Signer`. | `docs/design/signing.md` | The section names SG-06.2 to SG-06.7's exit tests and is reviewed. | SG-01.1 |
| SG-06.2 | build | A DER encoder with canonical `SET OF` ordering in the PKI crate. | `crates/tinker-pdf-pki/src/der.rs` | Unit tests: encoded structures pass the in-tree DER validator's X.690 canonical checks. | SG-06.1 |
| SG-06.3 | build | `SignedData` built in the engine: content type, message digest, ESS signing-certificate-v2 and an optional signing time, the chain, a signature by the engine's keys, under `adbe.pkcs7.detached` and `ETSI.CAdES.detached`. | `crates/tinker-pdf-pki/src/cms.rs`, `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/signing.rs`, `docs/features/signatures.md` | `signing.rs` `signed_outputs_verify`: every output verifies through the engine's verdict with digest, signature, chain and attributes all `Ok`. | SG-06.2, SG-01, SG-02, SG-05 |
| SG-06.4 | build | A signature timestamp through the host `Timestamper` as an unsigned attribute. | `crates/tinker-pdf-pki/src/cms.rs`, `crates/tinker-pdf-pki/src/tsp.rs`, `crates/tinker-pdf/tests/signing.rs` | `timestamped_signatures_verify`: the verdict reports the timestamp `Ok`. | SG-06.3 |
| SG-06.5 | build | The `RawSigner` seam: the host returns a raw signature over the to-be-signed attributes. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/signing.rs` | `raw_signer_outputs_verify`: a test host's raw signatures give outputs that verify, byte-identical to in-engine signing with the same deterministic key. | SG-06.3 |
| SG-06.6 | test | RFC 5652's structures transcribed as tests, and a deterministically signed document fingerprinted. | `crates/tinker-pdf-pki/tests/cms_structures.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | The transcribed structures encode and decode as the RFC states; the fingerprint holds on every K-07 target. | SG-06.4, SG-06.5, K-07 |
| SG-06.7 | watch | The interoperability record: signed output read by outside validators and viewers once, dated. | `docs/features/signatures.md` | H-68's dated measurement is recorded in the feature doc. | SG-06.6, H-68 |

**Closes the row:** SG-06.3 to SG-06.6 build `SignedData` that verifies with every check `Ok`, canonical DER and RFC 5652's structures, and SG-06.7 records the interoperability measurement, which is the row's exit.

## SG-07 — PAdES baseline levels B-B, B-T, B-LT and B-LTA

**PAdES baseline levels B-B, B-T, B-LT and B-LTA**: orchestration (signing-certificate-v2 mandatory, `/M` rather than a signing time, a signature timestamp through the host `Timestamper` for B-T, DSS and VRI with the chain's revocation data for B-LT, a document timestamp for B-LTA) and a level detector on the validation side

**Today.** the parts exist (DSS writer, document timestamp, host timestamper); "PAdES-LTA conformance levels are out" was a non-goal of the design until 9 October 2026 (`design/signatures.md:487-490`)

**Exit criterion.** the level detector, its rules transcribed clause by clause from EN 319 142-1 (free) with a fixture and twin per rule, classifies the engine's own output at each level and the corpus's signatures

**Doc:** `design/signing.md` (owed) · **Depends on:** SG-06, SG-10

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-07.1 | design | Add PAdES to `docs/design/signing.md`: each baseline level's orchestration and the level detector's rules, clause by clause from EN 319 142-1. | `docs/design/signing.md` | The section names SG-07.2 to SG-07.6's exit tests and is reviewed. | SG-01.1 |
| SG-07.2 | build | B-B and B-T: signing-certificate-v2, `/M` rather than a signing time, and a signature timestamp for B-T. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/pades.rs` (new) | `pades.rs` (new) `b_b_and_b_t_outputs_classify`: the detector classifies each output at its level. | SG-07.1, SG-06 |
| SG-07.3 | build | B-LT: the DSS and VRI with the chain's revocation data as SG-10 parses it. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/pades.rs` (new) | `b_lt_outputs_classify`: the detector classifies the output at B-LT. | SG-07.2, SG-10 |
| SG-07.4 | build | B-LTA: a document timestamp over the B-LT document. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/pades.rs` (new) | `b_lta_outputs_classify`: the detector classifies the output at B-LTA. | SG-07.3 |
| SG-07.5 | build | The level detector: EN 319 142-1's rules transcribed clause by clause, a fixture and twin per rule. | `crates/tinker-pdf/src/pades.rs` (new), `crates/tinker-pdf/tests/pades.rs` (new), `docs/features/signatures.md` | `each_rule_has_its_fixture_and_twin`: every rule passes its fixture and fails its twin. | SG-07.1 |
| SG-07.6 | watch | The corpus's signatures classified on a dispatched corpus run. | `.github/workflows/corpus.yml` | The run on `develop` prints each signature's level, read to the end. | SG-07.4, SG-07.5, MS-11, H-03 |

**Closes the row:** SG-07.5's detector, rule by rule, classifies the engine's own output at each level (SG-07.2 to SG-07.4) and the corpus's signatures (SG-07.6), which is the row's exit.

## SG-08 — `tpdf sign`, `verify`, `timestamp` and `ltv`

**`tpdf sign`, `verify`, `timestamp` and `ltv`**: sign with a PKCS#12, or a PKCS#8 key and chain, the passphrase from a file, the environment or a prompt (never the command line); a field by name or a new visible field; certification level; PAdES level; a two-phase file workflow for timestamp tokens and revocation data the CLI does not fetch

**Today.** `sign` was left out of the CLI "while signing keys are a non-goal": signing is a private-key operation, keys in the engine were a non-goal of [design/signatures.md](../design/signatures.md) until 9 October 2026, and the `Signer` seam takes finished CMS bytes, which a command line cannot produce without the signing arithmetic that non-goal declined; there is no `verify` subcommand

**Exit criterion.** `tools/tpdf` tests sign the `testdata/` fixtures with committed test keys; outputs verify through the engine's verdict and reopen through the facade; the passphrase never appears in a process listing or log

**Doc:** — · **Depends on:** SG-07

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-08.1 | build | `tpdf sign`: a PKCS#12, or a PKCS#8 key and chain, the passphrase from a file, the environment or a prompt and never an argument, a field by name or a new visible field, certification level and PAdES level. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs`, `docs/features/signatures.md` | CLI tests sign the `testdata/` fixtures with committed test keys; outputs verify through the engine's verdict and reopen; a passphrase argument is refused and no log carries the passphrase. | SG-07 |
| SG-08.2 | build | `tpdf verify`, the verdict as text and JSON. | `tools/tpdf/src/main.rs` | CLI tests print the verdict of signed fixtures as the facade gives it. | SG-08.1 |
| SG-08.3 | build | `tpdf timestamp` and `tpdf ltv`: a two-phase file workflow writing the requests and reading back the tokens and revocation data the CLI does not fetch. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs` | CLI tests run both phases with committed responses and the outputs classify at B-T and B-LT. | SG-08.1 |

**Closes the row:** SG-08.1 to SG-08.3 sign, verify, timestamp and add revocation data from the CLI, outputs verifying and the passphrase never exposed, which is the row's exit.

## SG-09 — Signing across the C ABI and every binding

**Signing across the C ABI and every binding**: in-engine keys need no callback (key bytes and passphrase in, signed bytes out); two-phase deferred signing (a placeholder and digest out with resumable state, then CMS or a raw signature in, without re-serialising) for keys the engine never sees; and a signing callback across the C ABI, with timestamper and entropy callbacks beside it

**Today.** `save_signed`, `Signer`, `Timestamper` and `EntropySource` are not projected: "no `tpdf_*` entry point takes a callback" (`features/bindings.md:1046, 1049`; `design/bindings-write.md:51-57`)

**Exit criterion.** `bindings-parity` prints the hash of a deterministically signed document (PKCS#1 v1.5 and RFC 6979), identical through C, Python, JavaScript, .NET, Go, Ruby and Java, by each of the three routes

**Doc:** [design/bindings-write.md](../design/bindings-write.md) (amend) · **Depends on:** SG-06, H-28, H-34

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-09.1 | design | Amend `docs/design/bindings-write.md` with signing: in-engine keys (bytes and passphrase in, signed bytes out), two-phase deferred signing with resumable state, and signing, timestamper and entropy callbacks as H-28 decides. | `docs/design/bindings-write.md` | The amended doc names SG-09.2 to SG-09.7's exit tests and is reviewed. | H-28, H-34 |
| SG-09.2 | build | The C ABI signs with in-engine keys: key bytes and passphrase in, signed bytes out. | `crates/tinker-pdf-ffi/src/lib.rs`, `crates/tinker-pdf-ffi/tests/signing.rs` (new) | `signing.rs` (new): a C-ABI signed document verifies and equals the facade's bytes. | SG-09.1, SG-06 |
| SG-09.3 | build | Two-phase deferred signing on the facade and the C ABI: a placeholder and digest out with resumable state, then CMS or a raw signature in, without re-serialising. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf-ffi/src/lib.rs`, `crates/tinker-pdf-ffi/tests/signing.rs` (new) | The finished bytes equal one-shot signing's with the same deterministic key. | SG-09.2 |
| SG-09.4 | build | Signing, timestamper and entropy callbacks across the C ABI. | `crates/tinker-pdf-ffi/src/lib.rs`, `crates/tinker-pdf-ffi/tests/signing.rs` (new) | A C test host's callbacks sign, timestamp and supply entropy, and the outputs verify. | SG-09.3 |
| SG-09.5 | build | Python and JavaScript project the three routes over the facade. | `bindings/python`, `bindings/js` | Each binding's test signs by each route and prints the document's hash. | SG-09.4 |
| SG-09.6 | build | .NET, Go, Ruby and Java project the three routes over the C ABI. | `bindings/dotnet`, `bindings/go`, `bindings/ruby`, `bindings/java` | Each binding's test signs by each route and prints the document's hash. | SG-09.4 |
| SG-09.7 | ci | `bindings-parity` compares the deterministically signed document's hash across every binding and route. | `.github/workflows/ci.yml` | `bindings-parity` prints one hash for C, Python, JavaScript, .NET, Go, Ruby and Java by each of the three routes (PKCS#1 v1.5 and RFC 6979), green on `develop`. | SG-09.5, SG-09.6, MS-11 |

**Closes the row:** SG-09.7's `bindings-parity` prints the same signed document's hash through every binding by each of the three routes, which is the row's exit.

## SG-10 — Revocation, parsed and evaluated

**Revocation, parsed and evaluated**: CRLs (RFC 5280 §5 and §6.3 — issuing distribution points, delta CRLs, reasons, indirect CRLs) and OCSP responses (RFC 6960 — responder authorisation, `id-pkix-ocsp-nocheck`, `thisUpdate`/`nextUpdate` against the validation time, nonces); OCSP request bytes built for the host to send; AIA and distribution-point URLs extracted; `adbe-revocationInfoArchival` read and written; the verdict consults the DSS. Fetching stays the host's

**Today.** "nothing parses a CRL or OCSP response, and the verdict does not consult the store" (`features/signatures.md:182-201`); fetching a non-goal until 9 October 2026 (`design/signatures.md:469-481`); the engine still performs no I/O (NG-08)

**Exit criterion.** the NIST public-key interoperability test suite's CRL cases pass (public test data); OCSP structures transcribed from RFC 6960 with responses generated once (dated, bytes only); the corpus's embedded revocation data evaluated and recorded

**Doc:** `design/signature-validation.md` (owed) · **Depends on:** SG-11

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-10.1 | design | Create `docs/design/signature-validation.md` for SG-10, SG-11 and SG-12: CRL and OCSP evaluation, RFC 5280 path validation with a revocation hook, EN 319 102-1's validation model, the clock as a parameter, fetching left to the host, and the public test data each is held to. | `docs/design/signature-validation.md` (new) | The doc exists in the design-doc order, names this row's exit tests and the milestones SG-11 and SG-12 extend, and is reviewed. | — |
| SG-10.2 | build | CRLs parsed (RFC 5280 §5) and evaluated (§6.3): issuing distribution points, delta CRLs, reasons and indirect CRLs, through SG-11's revocation hook. | `crates/tinker-pdf-pki/src/crl.rs` (new), `crates/tinker-pdf-pki/src/lib.rs`, `crates/tinker-pdf-pki/tests/pkits.rs` (new) | `pkits.rs` (new): the NIST interoperability suite's CRL cases pass. | SG-10.1, SG-11 |
| SG-10.3 | build | OCSP responses evaluated (RFC 6960): responder authorisation, `id-pkix-ocsp-nocheck`, `thisUpdate` and `nextUpdate` against the validation time, and nonces; request bytes built for the host to send. | `crates/tinker-pdf-pki/src/ocsp.rs` (new), `crates/tinker-pdf-pki/tests/ocsp.rs` (new) | `ocsp.rs` (new): RFC 6960's structures, transcribed, and responses generated once (dated, bytes only) evaluate as stated. | SG-10.2, H-106 |
| SG-10.4 | build | AIA and distribution-point URLs extracted, `adbe-revocationInfoArchival` read and written, and the verdict consulting the DSS. | `crates/tinker-pdf-pki/src/x509.rs`, `crates/tinker-pdf-pki/src/cms.rs`, `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/revocation.rs` (new), `docs/features/signatures.md` | `revocation.rs` (new): URLs extract as written; the archival attribute round-trips; a document whose DSS revokes the signer gets the revoked verdict; failing first on today's verdict. | SG-10.3 |
| SG-10.5 | watch | The corpus's embedded revocation data evaluated on a dispatched corpus run and recorded. | `.github/workflows/corpus.yml`, `docs/features/signatures.md` | The run on `develop` prints each embedded CRL's and response's evaluation, read to the end, and the feature doc records the figures. | SG-10.4, MS-11, H-03 |

**Closes the row:** SG-10.2 passes the NIST suite's CRL cases, SG-10.3 the transcribed OCSP structures, and SG-10.5 evaluates the corpus's revocation data, which is the row's exit.

## SG-11 — Certification path validation, complete

**Certification path validation, complete** (RFC 5280 §6): validity at the validation time, key usage and extended key usage, basic and name constraints, policy processing, revocation; caller-supplied anchors

**Today.** chain building to caller-supplied anchors at a caller-supplied instant (`features/signatures.md:97-104`); expiry and key-usage judgements partial

**Exit criterion.** the NIST public-key interoperability test suite's path-validation cases pass, the count ratcheted

**Doc:** `design/signature-validation.md` (owed) · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-11.1 | design | Add path validation to `docs/design/signature-validation.md`: RFC 5280 §6's state machine, caller-supplied anchors, and the revocation hook SG-10 fills. | `docs/design/signature-validation.md` | The section names SG-11.2 to SG-11.7's exit tests and is reviewed. | SG-10.1 |
| SG-11.2 | test | The NIST public-key interoperability test suite vendored as public test data, with a harness running each path-validation case and printing its count. | `crates/tinker-pdf-pki/tests/data/pkits` (new), `crates/tinker-pdf-pki/tests/pkits.rs` (new), `THIRDPARTY.md`, `.gitattributes` | The harness runs every case and prints the pass count; the data's terms are recorded. | SG-11.1, H-92 |
| SG-11.3 | build | Validity at the validation time, key usage and extended key usage, and basic constraints. | `crates/tinker-pdf-pki/src/path.rs` (new), `crates/tinker-pdf-pki/src/x509.rs` | The suite's sections for these checks pass. | SG-11.2 |
| SG-11.4 | build | Name constraints over every name form. | `crates/tinker-pdf-pki/src/path.rs` (new), `crates/tinker-pdf-pki/src/general_name.rs` | The suite's name-constraint cases pass. | SG-11.3 |
| SG-11.5 | build | Policy processing: the policy tree, explicit policy, inhibit any-policy and policy mappings. | `crates/tinker-pdf-pki/src/path.rs` (new) | The suite's policy cases pass. | SG-11.3 |
| SG-11.6 | build | The revocation hook: path validation asks a revocation checker about each certificate, a stub in the tests until SG-10's checker fills it. | `crates/tinker-pdf-pki/src/path.rs` (new), `docs/features/signatures.md` | A stub checker's revoked answer fails the path at that certificate as §6.1.3 states. | SG-11.3 |
| SG-11.7 | test | The path-validation pass count held as a ratcheted floor. | `crates/tinker-pdf-pki/tests/pkits.rs` (new) | The harness fails when the count falls below its committed floor, held at the count SG-11.3 to SG-11.6 reach. | SG-11.4, SG-11.5, SG-11.6 |

**Closes the row:** SG-11.3 to SG-11.6 pass the NIST suite's path-validation cases and SG-11.7 ratchets the count, which is the row's exit; revocation's cases close through SG-10.

## SG-12 — Validation per EN 319 102-1

**Validation per EN 319 102-1**: basic, with-time and long-term validation; indications passed, failed and indeterminate with their sub-indications; proof of existence from timestamps; algorithm-expiry constraints (TS 119 312); a report shaped as TS 119 102-2; the clock a parameter (ruling 4)

**Today.** a four-question verdict with no boolean

**Exit criterion.** each sub-indication reached by a fixture built for it; the report validates against the published XML schema in the test

**Doc:** `design/signature-validation.md` (owed) · **Depends on:** SG-10, SG-11, SG-18

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-12.1 | design | Add EN 319 102-1 to `docs/design/signature-validation.md`: basic, with-time and long-term validation, their indications and sub-indications, proof of existence, algorithm expiry from TS 119 312, and the TS 119 102-2 report. | `docs/design/signature-validation.md` | The section names SG-12.2 to SG-12.7's exit tests and is reviewed. | SG-10.1 |
| SG-12.2 | build | Basic validation with its indications and sub-indications. | `crates/tinker-pdf/src/validation.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/validation_model.rs` (new) | `validation_model.rs` (new): each basic sub-indication is reached by a fixture built for it. | SG-12.1, SG-10, SG-11 |
| SG-12.3 | build | Validation with time: proof of existence from signature timestamps and past signature validation. | `crates/tinker-pdf/src/validation.rs` (new), `crates/tinker-pdf/tests/validation_model.rs` (new) | Each with-time sub-indication is reached by a fixture built for it. | SG-12.2 |
| SG-12.4 | build | Long-term validation over archival timestamps, the DSS and chained proofs of existence. | `crates/tinker-pdf/src/validation.rs` (new), `crates/tinker-pdf/tests/validation_model.rs` (new) | Each long-term sub-indication is reached by a fixture built for it. | SG-12.3, SG-18 |
| SG-12.5 | build | Algorithm-expiry constraints from TS 119 312, the clock a parameter. | `crates/tinker-pdf/src/validation.rs` (new), `crates/tinker-pdf/tests/validation_model.rs` (new) | A signature with an algorithm expired at the given time gets the stated sub-indication; the same file at an earlier time does not. | SG-12.2 |
| SG-12.6 | build | An XML-schema checker in the XML crate for the constructs TS 119 102-2's published schema uses. | `crates/tinker-pdf-xml/src/schema.rs` (new), `crates/tinker-pdf-xml/src/lib.rs` | Unit tests: instances valid and invalid against small schemas of each construct are judged as the test states. | SG-12.1 |
| SG-12.7 | build | The validation report shaped as TS 119 102-2. | `crates/tinker-pdf/src/validation.rs` (new), `crates/tinker-pdf/tests/validation_model.rs` (new), `docs/features/signatures.md` | Every fixture's report validates against the published schema through SG-12.6 in the test. | SG-12.4, SG-12.5, SG-12.6 |

**Closes the row:** SG-12.2 to SG-12.5 reach each sub-indication by a fixture built for it and SG-12.7's report validates against the published schema, which is the row's exit.

## SG-13 — XML signatures and Exclusive XML Canonicalisation

**XML signatures and Exclusive XML Canonicalisation**, in-tree, verified and created: trust lists verified; XFA signatures, EPUB `signatures.xml` and XPS signatures verified and created with SG-01's and SG-02's in-engine keys

**Today.** absent — the owner's parity decision of 9 October 2026; no XML canonicalisation or `ds:Signature` reader exists (`rg -i 'c14n|xmldsig|SignedInfo' crates` finds nothing)

**Exit criterion.** the W3C canonicalisation test cases and XML-signature interoperability examples pass on the verify side; where an example publishes its private key, the engine's signature over the example's input reproduces the example's `SignatureValue` byte for byte (PKCS#1 v1.5 is deterministic); every signature the engine creates verifies through its own verifier, its canonical `SignedInfo` equal to the one computed in the test

**Doc:** — · **Depends on:** SG-01, SG-02 (creation)

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-13.1 | build | Canonical XML 1.0 and 1.1 and Exclusive XML Canonicalisation in the XML crate. | `crates/tinker-pdf-xml/src/c14n.rs` (new), `crates/tinker-pdf-xml/src/lib.rs`, `crates/tinker-pdf-xml/tests/c14n.rs` (new) | `c14n.rs` (new): the W3C canonicalisation test cases pass byte for byte. | H-92 |
| SG-13.2 | build | `ds:Signature` verified: `SignedInfo` canonicalised, references with their transforms and digests checked, `SignatureValue` verified with the key from `KeyInfo`. | `crates/tinker-pdf-pki/src/xmldsig.rs` (new), `crates/tinker-pdf-pki/src/lib.rs`, `crates/tinker-pdf-pki/tests/xmldsig.rs` (new) | `xmldsig.rs` (new): the W3C XML-signature interoperability examples pass on the verify side. | SG-13.1 |
| SG-13.3 | build | XML signatures created with SG-01's and SG-02's in-engine keys. | `crates/tinker-pdf-pki/src/xmldsig.rs` (new), `crates/tinker-pdf-pki/tests/xmldsig.rs` (new) | Where an example publishes its private key, the engine's PKCS#1 v1.5 signature over its input reproduces its `SignatureValue` byte for byte; every created signature verifies through SG-13.2 with its canonical `SignedInfo` equal to the test's. | SG-13.2, SG-01, SG-02 |
| SG-13.4 | build | EPUB `signatures.xml` and XPS package signatures verified and created. | `crates/tinker-pdf/src/epub.rs`, `crates/tinker-pdf/src/xps.rs`, `crates/tinker-pdf/tests/xml_signatures.rs` (new), `docs/features/signatures.md` | `xml_signatures.rs` (new): signed EPUB and XPS fixtures verify, and ones the engine signs verify through its own verifier. | SG-13.3 |

**Closes the row:** SG-13.1 and SG-13.2 pass the W3C cases and examples on the verify side, SG-13.3 reproduces a published `SignatureValue` and verifies what it creates, and SG-13.4 covers EPUB and XPS, which is the row's exit.

## SG-14 — Trust lists

**Trust lists**: the EU list of lists and national trusted lists (TS 119 612, signed), a vendor's approved-trust-list format, PEM bundles — ingested from bytes the host supplies; "qualified at time T" answered from service status history

**Today.** no bundled root store by policy (`features/signatures.md:315`); no list format read

**Exit criterion.** published lists' structures parsed and their signatures verified (SG-13); service-status history held to fixtures with known transitions

**Doc:** `design/trust-lists.md` (owed) · **Depends on:** SG-13, H-27

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-14.1 | design | Create `docs/design/trust-lists.md`: TS 119 612's lists and the list of lists, their signatures through SG-13, a vendor's approved-trust-list format and PEM bundles, ingestion from host bytes, and "qualified at time T" from status history. | `docs/design/trust-lists.md` (new) | The doc exists in the design-doc order, names SG-14.2 to SG-14.6's exit tests, and is reviewed. | H-27 |
| SG-14.2 | build | TS 119 612 lists parsed: scheme information, providers, services and their status history. | `crates/tinker-pdf-pki/src/tsl.rs` (new), `crates/tinker-pdf-pki/src/lib.rs`, `crates/tinker-pdf-pki/tests/trust_lists.rs` (new) | `trust_lists.rs` (new): published lists' structures parse as stated. | SG-14.1 |
| SG-14.3 | build | List signatures verified through SG-13, and national lists reached from the list of lists by its pointers. | `crates/tinker-pdf-pki/src/tsl.rs` (new), `crates/tinker-pdf-pki/tests/trust_lists.rs` (new) | Published lists' signatures verify; a tampered list fails. | SG-14.2, SG-13 |
| SG-14.4 | build | "Qualified at time T" answered from a service's status history. | `crates/tinker-pdf-pki/src/tsl.rs` (new), `crates/tinker-pdf-pki/tests/trust_lists.rs` (new) | Fixtures with known transitions answer at each time as the history states. | SG-14.2 |
| SG-14.5 | build | A vendor's approved-trust-list format and PEM bundles ingested as anchors. | `crates/tinker-pdf-pki/src/anchors.rs` (new), `crates/tinker-pdf-pki/tests/trust_lists.rs` (new) | Fixtures of each format yield their anchors. | SG-14.1 |
| SG-14.6 | build | Trust-list anchors used by the verdict when the caller selects lists. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/trust_lists_verdict.rs` (new), `docs/features/signatures.md` | `trust_lists_verdict.rs` (new): a signature chaining to a listed service verifies as trusted, and one chaining to a withdrawn service is reported at its time. | SG-14.3, SG-14.4, SG-14.5 |

**Closes the row:** SG-14.2 and SG-14.3 parse published lists and verify their signatures, and SG-14.4 holds status history to fixtures with known transitions, which is the row's exit.

## SG-15 — Seed values and locks

**Seed values and locks**: `/SV` constraints honoured and reported (filter, sub-filter, digest, certificate constraints, reasons, MDP, timestamp, legal attestation, revocation, lock document, appearance filter); `/Lock` applied when a field is signed; MDP version 2 `/P`; a query for the fields a given signature locks and whether a field is locked, per revision

**Today.** locks on certification exist; `/SV` is not read (no source hit for `SV`)

**Exit criterion.** each constraint a fixture whose signing request is refused or adjusted as 12.8.1.3 states; the lock query answers fixtures signed with `All`, `Include` and `Exclude` locks field by field; `tpdf forms lock` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** SG-06

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-15.1 | build | `/SV` seed values read, honoured on signing and reported: filter, sub-filter, digest, certificate constraints, reasons, MDP, timestamp, legal attestation, revocation, lock document and appearance filter. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/seed_values.rs` (new), `docs/features/signatures.md` | `seed_values.rs` (new): each constraint is a fixture whose signing request is refused or adjusted as 12.8.1.3 states. | SG-06 |
| SG-15.2 | build | `/Lock` applied when a field is signed, MDP version 2's `/P`, and a query for the fields a signature locks and whether a field is locked, per revision. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/src/mdp.rs`, `crates/tinker-pdf/tests/seed_values.rs` (new) | `lock_queries_answer_field_by_field`: fixtures signed with `All`, `Include` and `Exclude` locks answer field by field. | SG-15.1 |
| SG-15.3 | build | `tpdf forms lock`. | `tools/tpdf/src/main.rs` | The command runs over `testdata/` fixtures to the facade's output. | SG-15.2 |

**Closes the row:** SG-15.1 refuses or adjusts each constraint as 12.8.1.3 states, SG-15.2 answers lock queries field by field and SG-15.3 runs `tpdf forms lock`, which is the row's exit.

## SG-16 — Signature appearances, complete

**Signature appearances, complete**: legacy layered `n0`/`n2` appearances, configurable layouts, a handwritten signature image with alpha, a vector logo, localised dates, signer names in any script through a CID font and shaping, and a preview before signing

**Today.** the visible seal sets Helvetica in `WinAnsiEncoding` and draws a character above the single-byte range as a substitute (`features/signatures.md:210-211`)

**Exit criterion.** a reviewed golden family per layout (K-08, H-73); non-Latin names extract as written

**Doc:** — · **Depends on:** ED-45

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-16.1 | build | Configurable layouts (name, reason, location, date, logo and image placement) and legacy layered `n0`/`n2` appearances. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/signature_appearances.rs` (new), `docs/features/signatures.md` | `signature_appearances.rs` (new): each layout is a K-08 golden family awaiting H-73. | K-08 |
| SG-16.2 | build | A handwritten signature image with alpha, and a vector logo from an SVG or a page (ED-48). | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/signature_appearances.rs` (new) | Each is a K-08 golden awaiting H-73; the alpha becomes a soft mask. | SG-16.1, ED-48 |
| SG-16.3 | build | Localised dates and signer names in any script, through a CID font and shaping (ED-45). | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/signature_appearances.rs` (new) | Non-Latin names extract as written in both builds; dates follow the stated locale's pattern. | SG-16.1, ED-45 |
| SG-16.4 | build | A preview of the appearance before signing. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/signature_appearances.rs` (new) | The preview renders equal to the signed appearance. | SG-16.1 |
| SG-16.5 | watch | The appearance golden families reviewed by a person. | `crates/tinker-pdf/tests/render_goldens.rs` | Every family SG-16.1 to SG-16.3 added is signed as reviewed under H-73. | SG-16.2, SG-16.3, H-73 |

**Closes the row:** SG-16.1 to SG-16.3 give each layout a golden family and extract non-Latin names as written, and SG-16.5 records the review, which is the row's exit.

## SG-17 — Clear a signature

**Clear a signature**: revert a signed field to unsigned in a new revision

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'clear_signature|remove_signature' crates tools` finds nothing

**Exit criterion.** the cleared field reports unsigned and the earlier revision still verifies

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-17.1 | build | Clear a signature: a signed field reverted to unsigned in a new incremental revision, its value removed and its appearance reset. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/clear_signature.rs` (new), `docs/features/signatures.md` | `clear_signature.rs` (new): the cleared field reports unsigned and the earlier revision still verifies. | — |

**Closes the row:** SG-17.1 reports the cleared field unsigned with the earlier revision still verifying, which is the row's exit; AN-17.2 is its form-layer surface.

## SG-18 — Revision analysis and attack detection, on by default

**Revision analysis and attack detection, on by default**: enumerate revisions; extract the signed revision; diff later revisions by value and classify each change as allowed or not under DocMDP and FieldMDP; detect shadow attacks (hide, replace, hide-and-replace through overlays, optional content, annotations, font swaps), incremental-saving attacks, signature wrapping and `/ByteRange` tricks; a visual diff of signed against final

**Today.** `modifications()` compares cross-reference entries by object (`features/signatures.md:150-157`)

**Exit criterion.** a fixture per published attack class, built here from the class's description, is flagged; every benign incremental edit in the corpus is not

**Doc:** `design/revision-analysis.md` (owed) · **Depends on:** ED-31, ED-23

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-18.1 | design | Create `docs/design/revision-analysis.md`: revisions and the signed one (ED-31), value diffs, allowed changes under DocMDP and FieldMDP by 12.8.2.2, each published attack class and its fixture built from the class's description, the visual diff (ED-23), and the analysis on by default in the verdict. | `docs/design/revision-analysis.md` (new) | The doc exists in the design-doc order, names SG-18.2 to SG-18.7's exit tests, and is reviewed. | ED-31, ED-23 |
| SG-18.2 | build | Each later revision's changes classified as allowed or not under DocMDP's three permission levels and FieldMDP. | `crates/tinker-pdf/src/mdp.rs`, `crates/tinker-pdf/tests/revision_analysis.rs` (new) | `revision_analysis.rs` (new): fixtures at each level classify every change as 12.8.2.2 states. | SG-18.1 |
| SG-18.3 | build | Shadow attacks detected: hide, replace and hide-and-replace through overlays, optional content, annotations and font swaps. | `crates/tinker-pdf/src/revision_attacks.rs` (new), `crates/tinker-pdf/tests/revision_analysis.rs` (new) | A fixture per shadow class, built from its description, is flagged. | SG-18.2 |
| SG-18.4 | build | Incremental-saving attacks, signature wrapping and `/ByteRange` tricks detected. | `crates/tinker-pdf/src/revision_attacks.rs` (new), `crates/tinker-pdf/tests/revision_analysis.rs` (new) | A fixture per class is flagged. | SG-18.2 |
| SG-18.5 | build | A visual diff of the signed revision against the final one in the report, through ED-23. | `crates/tinker-pdf/src/revision_attacks.rs` (new), `crates/tinker-pdf/tests/revision_analysis.rs` (new) | A shadow fixture's report carries the regions ED-23 finds between the signed and final renders. | SG-18.3 |
| SG-18.6 | build | The analysis on by default in the verdict. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/revision_analysis.rs` (new), `docs/features/signatures.md` | The default verdict flags every attack fixture and none of the benign incremental-edit fixtures. | SG-18.4, SG-18.5 |
| SG-18.7 | watch | Every benign incremental edit in the corpus left unflagged, on a dispatched corpus run. | `.github/workflows/corpus.yml` | The run on `develop` flags no benign incremental edit, read to the end. | SG-18.6, MS-11, H-03 |

**Closes the row:** SG-18.3 and SG-18.4 flag a fixture per published attack class and SG-18.7 flags no benign corpus edit, which is the row's exit.

## SG-19 — Usage-rights signatures

**Usage-rights signatures**: `/Perms /UR3` verified with its transform parameters, the granted rights reported, stripped safely when an edit would invalidate them

**Today.** `/Perms` signatures are found and classified (`features/signatures.md:25-63`)

**Exit criterion.** fixtures with each rights flag; stripping leaves a document that validates and keeps every other signature valid

**Doc:** — · **Depends on:** —

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-19.1 | build | `/Perms /UR3` verified with its transform parameters, and the granted rights reported. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/usage_rights.rs` (new), `docs/features/signatures.md` | `usage_rights.rs` (new): fixtures with each rights flag verify and report their rights. | — |
| SG-19.2 | build | Usage rights stripped when an edit would invalidate them. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/usage_rights.rs` (new) | Stripping leaves a document that validates strictly and keeps every other signature valid. | SG-19.1 |

**Closes the row:** SG-19.1 verifies and reports each rights flag and SG-19.2 strips them safely, which is the row's exit.

## SG-20 — Edit pre-flight for signed documents

**Edit pre-flight for signed documents**: "will this edit invalidate signature X, or break DocMDP P=2?" answered before saving, and incremental mode forced

**Today.** `check_save` answers encryption questions only

**Exit criterion.** a matrix of edits over certified fixtures predicted correctly against the verdict after saving

**Doc:** — · **Depends on:** SG-18

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-20.1 | build | Edit pre-flight for signed documents: a proposed change set run as a trial incremental revision through SG-18's classification, answering whether it invalidates a signature or breaks DocMDP, and incremental mode forced for signed documents; `check_save` gains the signature questions. | `crates/tinker-pdf/src/mdp.rs`, `crates/tinker-pdf-cos/src/edit/save.rs`, `crates/tinker-pdf/tests/signed_preflight.rs` (new), `docs/features/signatures.md` | `signed_preflight.rs` (new) `edits_are_predicted_as_the_verdict_finds`: a matrix of edits over certified fixtures is predicted correctly against the verdict after saving. | SG-18 |

**Closes the row:** SG-20.1 predicts a matrix of edits over certified fixtures as the verdict finds them after saving, which is the row's exit.

## SG-21 — Detached signatures outside the PDF

**Detached signatures outside the PDF**: CAdES and ASiC sidecars over a PDF or its attachments; `.p7s` verification

**Today.** absent — the owner's parity decision of 9 October 2026; `rg '\.p7s|ASiC' crates tools` finds nothing

**Exit criterion.** sidecars generated by SG-06's builder verify; structures transcribed from the ETSI standards

**Doc:** — · **Depends on:** SG-06, SG-13

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-21.1 | build | Detached `.p7s` signatures over a PDF's bytes or an attachment's, created by SG-06's builder and verified. | `crates/tinker-pdf/src/detached.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/detached_signatures.rs` (new), `docs/features/signatures.md` | `detached_signatures.rs` (new): generated sidecars verify through the engine's verdict; a changed byte fails. | SG-06 |
| SG-21.2 | build | CAdES baseline attributes on sidecars, transcribed from EN 319 122. | `crates/tinker-pdf/src/detached.rs` (new), `crates/tinker-pdf/tests/detached_signatures.rs` (new) | The transcribed structures are produced and verify. | SG-21.1 |
| SG-21.3 | build | ASiC containers, simple and extended, with CMS or XML signatures (SG-13), through the ZIP crate. | `crates/tinker-pdf/src/detached.rs` (new), `crates/tinker-pdf/tests/detached_signatures.rs` (new) | Containers built to EN 319 162's transcribed structures verify; a changed member fails. | SG-21.2, SG-13 |
| SG-21.4 | build | `tpdf sign --detached` and `tpdf verify --detached`. | `tools/tpdf/src/main.rs` | CLI tests sign and verify sidecars over `testdata/` fixtures. | SG-21.3, SG-08 |

**Closes the row:** SG-21.1 to SG-21.3 verify sidecars SG-06's builder generates and produce the ETSI structures, which is the row's exit.

## SG-22 — Signature policies and attributes

**Signature policies and attributes**: policy identifiers, commitment types, claimed and certified signer roles, content timestamps, counter-signatures

**Today.** absent — the owner's parity decision of 9 October 2026; the CMS reader names a countersignature only to say it is not covered (`crates/tinker-pdf-pki/src/cms.rs:1121`); no policy, commitment-type or role attribute is read or written

**Exit criterion.** each attribute written, re-read and reported by the verdict

**Doc:** — · **Depends on:** SG-06

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-22.1 | build | Signature policy identifiers, commitment types, claimed and certified signer roles and content timestamps written, re-read and reported by the verdict. | `crates/tinker-pdf-pki/src/cms.rs`, `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/signature_attributes.rs` (new), `docs/features/signatures.md` | `signature_attributes.rs` (new): each attribute is written, re-read and reported. | SG-06 |
| SG-22.2 | build | Counter-signatures created and verified, retiring the reader's note that they are not covered (`cms.rs:1121`). | `crates/tinker-pdf-pki/src/cms.rs`, `crates/tinker-pdf/tests/signature_attributes.rs` (new) | A counter-signed signature reports both signatures verified; a broken counter-signature is reported. | SG-22.1 |

**Closes the row:** SG-22.1 and SG-22.2 write, re-read and report every attribute, which is the row's exit.

## SG-23 — Keys and certificates made here

**Keys and certificates made here**: RSA and EC key generation from caller entropy, self-signed certificates, PKCS#10 requests, PKCS#12 export — so signing fixtures can be made in the tree

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'generate_key|CertificationRequest' crates tools` finds nothing, and `keygen` only as an HTML element name

**Exit criterion.** generated keys sign and verify; the exported PKCS#12 re-imports through SG-05; certificate fields held to RFC 5280 structures

**Doc:** — · **Depends on:** SG-05, H-33

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-23.1 | build | RSA key generation from caller entropy: primes by Miller–Rabin, deterministic from a seed for tests. | `crates/tinker-pdf-crypto/src/keygen.rs` (new), `crates/tinker-pdf-crypto/src/lib.rs` | Unit tests: generated keys sign and verify; the same seed gives the same key. | SG-05, H-33 |
| SG-23.2 | build | EC key generation from caller entropy by rejection sampling. | `crates/tinker-pdf-crypto/src/keygen.rs` (new) | Unit tests: generated keys on each curve sign and verify. | SG-23.1 |
| SG-23.3 | build | Self-signed certificates and PKCS#10 requests. | `crates/tinker-pdf-pki/src/certgen.rs` (new), `crates/tinker-pdf-pki/src/lib.rs` | Unit tests: certificate fields hold to RFC 5280's structures and requests to RFC 2986's; certificates verify under their own key. | SG-23.2 |
| SG-23.4 | build | PKCS#12 export with PBES2 and a MAC. | `crates/tinker-pdf-pki/src/pkcs12.rs` (new) | An exported PKCS#12 re-imports through SG-05 to the same key and chain. | SG-23.3 |
| SG-23.5 | test | A fixture generator for the signing tests built on SG-23.1 to SG-23.4, so new signing fixtures are made in the tree. | `crates/tinker-pdf/tests/signing_support/mod.rs` (new), `crates/tinker-pdf/tests/signing.rs` | The signing tests make their keys and certificates through the generator and pass. | SG-23.4 |

**Closes the row:** SG-23.1 to SG-23.4 make keys that sign and verify, certificates held to RFC 5280 and a PKCS#12 that re-imports, which is the row's exit.

## SG-24 — Remote and batch signing preparation

**Remote and batch signing preparation**: to-be-signed hashes for remote signing services, batched over many documents under one authorisation

**Today.** the two-phase route (SG-09) is the engine side

**Exit criterion.** a batch of 100 documents prepared, signed by a test host and completed verifies document by document

**Doc:** — · **Depends on:** SG-09

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-24.1 | build | Batch preparation for remote signing: to-be-signed hashes for many documents under one authorisation, through SG-09's two-phase route, and completion from the returned signatures. | `crates/tinker-pdf/src/signature.rs`, `crates/tinker-pdf/tests/batch_signing.rs` (new), `docs/features/signatures.md` | `batch_signing.rs` (new): a batch of 100 documents prepared, signed by a test host and completed verifies document by document. | SG-09 |

**Closes the row:** SG-24.1 prepares, signs and completes a batch of 100 documents that verify one by one, which is the row's exit.

## SG-25 — Post-quantum signatures and recipients

**Post-quantum signatures and recipients**: ML-DSA and SLH-DSA in CMS; ML-KEM recipients for public-key encryption; composite signatures

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'ml.dsa|slh.dsa|ml.kem|dilithium|kyber' crates tools` finds nothing

**Exit criterion.** the NIST post-quantum test vectors pass; signed documents verify through the verdict

**Doc:** `design/post-quantum.md` (owed) · **Depends on:** SG-06, SE-07

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-25.1 | design | Create `docs/design/post-quantum.md`: ML-DSA (FIPS 204), SLH-DSA (FIPS 205) and ML-KEM (FIPS 203) on the constant-time path, their identifiers in CMS and in recipients, composite signatures and which specification is followed, and the NIST vectors each is held to. | `docs/design/post-quantum.md` (new) | The doc exists in the design-doc order, names SG-25.2 to SG-25.7's exit tests, and is reviewed. | SG-06 |
| SG-25.2 | build | ML-DSA key generation, signing and verification. | `crates/tinker-pdf-crypto/src/mldsa.rs` (new), `crates/tinker-pdf-crypto/src/lib.rs` | Unit tests: NIST's ML-DSA vectors pass. | SG-25.1, SG-04 |
| SG-25.3 | build | SLH-DSA signing and verification. | `crates/tinker-pdf-crypto/src/slhdsa.rs` (new) | Unit tests: NIST's SLH-DSA vectors pass. | SG-25.1, SG-04 |
| SG-25.4 | build | ML-KEM encapsulation and decapsulation. | `crates/tinker-pdf-crypto/src/mlkem.rs` (new) | Unit tests: NIST's ML-KEM vectors pass. | SG-25.1, SG-04 |
| SG-25.5 | build | ML-DSA and SLH-DSA signatures in CMS, signing and verifying documents. | `crates/tinker-pdf-pki/src/cms.rs`, `crates/tinker-pdf/tests/post_quantum.rs` (new) | `post_quantum.rs` (new): documents signed with each verify through the verdict. | SG-25.2, SG-25.3 |
| SG-25.6 | build | ML-KEM recipients for public-key encryption, on SE-07's recipient machinery. | `crates/tinker-pdf-pki/src/enveloped.rs`, `crates/tinker-pdf/tests/post_quantum.rs` (new) | A document encrypted to an ML-KEM recipient decrypts with its key and not with another. | SG-25.4, SE-07 |
| SG-25.7 | build | Composite signatures as the design's chosen specification defines them. | `crates/tinker-pdf-pki/src/cms.rs`, `crates/tinker-pdf/tests/post_quantum.rs` (new), `docs/features/signatures.md` | Composite-signed documents verify through the verdict; one broken component fails. | SG-25.5 |

**Closes the row:** SG-25.2 to SG-25.4 pass NIST's vectors and SG-25.5 to SG-25.7 sign and encrypt documents that the verdict accepts, which is the row's exit.

## SG-26 — The signatures corpus measurement

**The signatures corpus measurement**

**Today.** **Every named shape has left; one corpus measurement is owed.** RSASSA-PSS has left — verified against 360 CAVP `SigVerPSS` vectors, RSA Laboratories' 60 and a document an outside toolkit signed once (`signature_shapes.rs`). A signature with no signed attributes has left too — checked over the covered bytes' own digest, against a no-attributes fixture the same toolkit made; the corpus's one such signer (`bug854315.pdf`) is owed its first measured verdict, which `verdicts.rs` prints and tallies apart. `adbe.pkcs7.sha1` has left — both links of its encapsulated digest checked, against two such fixtures; its one corpus file is a fuzzer's mutation whose coverage does not hold up, so no corpus count moves. `GeneralNames` have left — decoded in the alternative-name extensions, `authorityCertIssuer` and an ESS `issuerSerial`, against RFC 5280 C.2 and an outside CAdES signature. RFC 3161 signature timestamps have left — imprint, signature, `TSTInfo` digest, the authority certificate's EKU and ESS binding, and its chain at `genTime`, against a token an outside timestamp authority made once. **What is left is measurement, not work**, none of it taken because the corpora could not be fetched where these landed: `verdicts.rs`'s pin for `bug854315.pdf`'s unattributed signer — its assertion that the signature is no longer `NotChecked` is an expectation nobody has measured, and would fail on an MD5 digest, unusable coverage or a missing certificate; what the verdict makes of the corpus's seven timestamp tokens; and a re-run of `cms_census.rs`, whose pins (27 blobs parsed, 7 nested tokens) predate the strict decoding of RFC 2634's first-version `signingCertificate`, which fails a whole `ContentInfo` on a malformed one as the second version always did, and which RFC 3161 tokens normally carry ([features/signatures.md](../features/signatures.md); [design/signatures.md](../design/signatures.md))

**Exit criterion.** the census's first run with these, recorded and pinned

**Doc:** — · **Depends on:** MS-13

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-26.1 | watch | Dispatch `corpus.yml` on `develop` and read the signature census and `verdicts.rs` to the end, the first measured verdict of the corpus's no-attributes signer among them. | `.github/workflows/corpus.yml`, `crates/tinker-pdf/tests/verdicts.rs` | The run prints every tally in a green run on `develop`; the run's id and figures are quoted in SG-26.2's commit. | MS-13, MS-11, H-03 |
| SG-26.2 | measure | The census's first figures recorded and pinned as floors. | `crates/tinker-pdf/tests/verdicts.rs`, `docs/features/signatures.md` | The census fails below its committed floors (a counted injection fires it) and passes at SG-26.1's figures; the feature doc quotes them with the run. | SG-26.1 |

**Closes the row:** SG-26.1 takes the census's first run and SG-26.2 records and pins it, which is the row's exit.

## SG-27 — A PKCS#11 signing provider

**A PKCS#11 signing provider** for keys held in tokens, smart cards and HSMs: a `RawSigner` for `tpdf sign` and the bindings that loads the PKCS#11 module a caller names, finds a key by label or id, and signs SG-06's to-be-signed attributes — kept outside the engine crates, as FT-13's font provider is, because loading a module needs the platform's loader and `unsafe`

**Today.** absent — the owner's parity decision of 9 October 2026; SG-06 adds the `RawSigner` seam and nothing implements it for a token (`rg -i 'pkcs#\?11|cryptoki' crates tools` finds nothing)

**Exit criterion.** CI signs `testdata/` fixtures through a software token that holds a generated key (an outside program hosts the key and executes the operation; it judges nothing) and the engine's own verdict verifies every output; a refused operation, a wrong PIN and a missing key are typed errors; `tpdf sign --pkcs11` runs it; real hardware is H-117's confirmation

**Doc:** — · **Depends on:** SG-06, SG-08, H-34

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-27.1 | build | A PKCS#11 `RawSigner` outside the engine crates: the module a caller names loaded, a key found by label or id, and SG-06's to-be-signed attributes signed; a refused operation, a wrong PIN and a missing key typed errors. | `tools/pkcs11-signer/Cargo.toml` (new), `tools/pkcs11-signer/src/lib.rs` (new), `Cargo.toml`, `docs/architecture.md` | Unit tests with a mock module return each typed error; the crate is outside the engine's `unsafe` ban as `docs/architecture.md` records. | SG-06, H-34 |
| SG-27.2 | ci | A CI job signing `testdata/` fixtures through a software token holding a generated key, the outside program only hosting the key and executing the operation. | `.github/workflows/ci.yml`, `tools/pkcs11-signer/tests/token.rs` (new) | The job signs every fixture through the token and the engine's verdict verifies each output, green on `develop`. | SG-27.1, MS-11 |
| SG-27.3 | build | `tpdf sign --pkcs11`, and the provider offered to the bindings. | `tools/tpdf/src/main.rs`, `tools/tpdf/Cargo.toml`, `bindings/python` | The CI job's token signs through the CLI and through Python, and the outputs verify. | SG-27.2, SG-08 |
| SG-27.4 | watch | Real hardware confirmed by a person's session with a token. | `docs/features/signatures.md` | H-117's dated confirmation is recorded in the feature doc. | SG-27.3, H-117 |

**Closes the row:** SG-27.2 signs fixtures through a software token with every output verifying, SG-27.1 types each failure and SG-27.3 runs `tpdf sign --pkcs11`, with real hardware confirmed in SG-27.4, which is the row's exit.

## SG-28 — Operating-system key stores as signing providers

**Operating-system key stores as signing providers**: Windows CNG and its certificate store, the macOS keychain, and the Android Keystore through the Android binding — each a `RawSigner` outside the engine crates, the key never leaving the store

**Today.** absent — the owner's parity decision of 9 October 2026; no platform key store is reached (`rg -i 'keychain|keystore|\bcng\b' crates tools bindings` finds nothing)

**Exit criterion.** CI legs on Windows and macOS generate a non-exportable key into the runner's own store, sign `testdata/` fixtures through it, and the engine's verdict verifies the outputs; the Android provider signs in the AAR's emulator test; only a signature value crosses from the store

**Doc:** — · **Depends on:** SG-27, SU-15, H-34

*Steps not yet reviewed: the run that takes this row's first step reviews them against the tree first.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SG-28.1 | build | A Windows CNG `RawSigner` over its certificate store, outside the engine crates, the key never leaving the store. | `tools/keystore-signer/Cargo.toml` (new), `tools/keystore-signer/src/windows.rs` (new), `tools/keystore-signer/src/lib.rs` (new), `Cargo.toml` | A Windows CI leg generates a non-exportable key into the runner's store, signs `testdata/` fixtures through it, and the verdict verifies the outputs. | SG-27, H-34 |
| SG-28.2 | build | A macOS keychain `RawSigner`. | `tools/keystore-signer/src/macos.rs` (new) | A macOS CI leg does the same through the runner's keychain. | SG-28.1 |
| SG-28.3 | build | An Android Keystore `RawSigner` through the Android binding. | `tools/keystore-signer/src/android.rs` (new), `bindings/java` | The AAR's emulator test signs through the Keystore and the verdict verifies. | SG-28.1, SU-15 |
| SG-28.4 | ci | The Windows, macOS and Android legs in CI. | `.github/workflows/ci.yml` | All three legs are green on `develop`, and only a signature value crosses from each store. | SG-28.2, SG-28.3, MS-11 |

**Closes the row:** SG-28.1 to SG-28.4 sign through each platform's store on CI with outputs verifying and only a signature value crossing, which is the row's exit.
