# Capability tier — security

[The roadmap](../ROADMAP.md) says how a step is worked and what comes next.

Parity here means every standard security handler and crypt filter read
and written (the legacy ones behind an explicit option), the PDF 2.0
extensions for authenticated encryption and integrity, public-key
encryption in every recipient shape, and the audits a security reviewer
reaches for — hidden information, active content, disarm and
reconstruction — each a typed report before it is a removal.

## SE-01 — Encryption on save below R6

**Encryption on save below R6** — R4 with AES-128, RC4 at 40 and 128 bits — for readers that stop at 1.6, behind an explicit legacy option with a typed weakness warning

**Exit criterion.** each revision's `/O` and `/U` entries, computed with a corpus file's own parameters and known password (`corpus/passwords.tsv`), equal that file's bytes; outputs reopen through the reader

**Doc:** — · **Depends on:** H-18

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-01.1 | build | Encryption on save below R6, behind an explicit legacy option with a typed weakness warning: R4 with AES-128 and RC4 at 40 and 128 bits, `/O` and `/U` by Algorithms 3 to 5. | `crates/tinker-pdf-cos/src/security.rs`, `crates/tinker-pdf/src/write.rs`, `crates/tinker-pdf/tests/legacy_encryption.rs` (new), `docs/features/encryption.md` | `legacy_encryption.rs` (new): outputs of each revision reopen through the reader with the expected streams in clear, and the warning is raised. | H-18 |
| SE-01.2 | test | A corpus check: for each file `corpus/passwords.tsv` lists at R2 to R4, `/O` and `/U` computed from the file's own parameters and known password and compared with its bytes. | `crates/tinker-pdf/tests/legacy_encryption_census.rs` (new), `.github/workflows/corpus.yml` | The check prints `RAN` and passes for every listed file on a dispatched run on `develop`. | SE-01.1, MS-11 |

## SE-02 — AES-256-GCM crypt filters

**AES-256-GCM crypt filters** (ISO/TS 32003), read and written

**Exit criterion.** NIST GCM test vectors; GHASH constant-time

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-02.1 | build | GCM in the crypto crate: GHASH constant-time with no table indexed by a secret. | `crates/tinker-pdf-crypto/src/gcm.rs` (new), `crates/tinker-pdf-crypto/src/lib.rs`, `crates/tinker-pdf-crypto/tests/gcm.rs` (new) | `gcm.rs` (new): NIST's GCM vectors pass; an operation-count test shows GHASH's counts independent of the key and data. | — |
| SE-02.2 | build | AES-256-GCM crypt filters read as ISO/TS 32003 defines them, each object's nonce and tag checked. | `crates/tinker-pdf-cos/src/decrypt.rs`, `crates/tinker-pdf/tests/gcm_encryption.rs` (new), `docs/features/encryption.md` | `gcm_encryption.rs` (new): fixtures decrypt; a changed tag is a typed error. | SE-02.1 |
| SE-02.3 | build | AES-256-GCM crypt filters written with TS 32003's nonce rule. | `crates/tinker-pdf-cos/src/security.rs`, `crates/tinker-pdf/tests/gcm_encryption.rs` (new), `crates/tinker-pdf/tests/determinism.rs` | Outputs reopen through SE-02.2; no two objects share a nonce; a written document's bytes are fingerprinted on every K-07 target. | SE-02.2, K-07 |

## SE-03 — Integrity protection for encrypted documents

**Integrity protection for encrypted documents** (ISO/TS 32004): the MAC written and verified

**Exit criterion.** the MAC recomputed in the test from the extension's definition; a tampered byte is reported

**Doc:** — · **Depends on:** SE-02

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-03.1 | build | ISO/TS 32004's integrity MAC written and verified for encrypted documents. | `crates/tinker-pdf-cos/src/security.rs`, `crates/tinker-pdf-cos/src/decrypt.rs`, `crates/tinker-pdf/tests/integrity_mac.rs` (new), `docs/features/encryption.md` | `integrity_mac.rs` (new): the MAC equals the one the test recomputes from the extension's definition; a tampered byte is reported. | SE-02 |

## SE-04 — Crypt-filter breadth on write and one reader gap

**Crypt-filter breadth on write and one reader gap**: embedded-files-only (`/EFF`), named crypt filters, `/Identity`, `/EncryptMetadata false`; `/EncryptMetadata` read from the crypt filter as well as from `/Encrypt`

**Exit criterion.** fixtures per configuration reopen with the expected streams in clear; the reader honours a crypt filter's `/EncryptMetadata`

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-04.1 | fix | The reader takes `/EncryptMetadata` from a crypt filter as well as from `/Encrypt`, as Table 27 puts the public-key handler's. | `crates/tinker-pdf-cos/src/decrypt.rs`, `crates/tinker-pdf/tests/crypt_filters.rs` (new) | `crypt_filters.rs` (new): a public-key document whose crypt filter says `/EncryptMetadata false` reads its metadata in clear. | — |
| SE-04.2 | build | Crypt filters on write: embedded files only (`/EFF`), named filters, `/Identity`, and `/EncryptMetadata false`. | `crates/tinker-pdf-cos/src/security.rs`, `crates/tinker-pdf/tests/crypt_filters.rs` (new), `docs/features/encryption.md` | Fixtures per configuration reopen with exactly the expected streams in clear. | SE-04.1 |

## SE-05 — Unencrypted wrapper documents

**Unencrypted wrapper documents** (2.0 7.6.7): a cover document carrying an `/EncryptedPayload` collection item, created and opened

**Exit criterion.** a wrapper built and reopened yields its payload byte for byte

**Doc:** — · **Depends on:** ED-21

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-05.1 | build | Unencrypted wrappers (2.0 7.6.7): a cover document with an `/EncryptedPayload` collection item and its `/EP` dictionary created, and on open the payload found and offered, retiring the associated-file refusal. | `crates/tinker-pdf/src/wrapper.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/encrypted_wrapper.rs` (new), `docs/features/document-model.md` | `encrypted_wrapper.rs` (new): a wrapper built and reopened yields its payload byte for byte. | ED-21 |

## SE-06 — Password robustness and introspection

**Password robustness and introspection**: a password tried in alternative encodings for R2–R4, the user password recovered from the owner password for R2–R4, open by a raw file key, report the computed key, which password matched and the crypt parameters, and copy encryption parameters from another document

**Exit criterion.** fixtures for each route; R2–R4 recovery recomputed from Algorithm 7 in the test

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-06.1 | build | Passwords tried in alternative encodings for R2 to R4, opening by a raw file key, and a report of the computed key, which password matched and the crypt parameters. | `crates/tinker-pdf-cos/src/decrypt.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/password_routes.rs` (new), `docs/features/encryption.md` | `password_routes.rs` (new): a fixture per route opens and reports as stated. | — |
| SE-06.2 | build | The user password recovered from the owner password for R2 to R4, and encryption parameters copied from another document. | `crates/tinker-pdf-cos/src/decrypt.rs`, `crates/tinker-pdf-cos/src/security.rs`, `crates/tinker-pdf/tests/password_routes.rs` (new) | Recovery equals Algorithm 7 recomputed in the test; a document saved with copied parameters opens with the source's passwords. | SE-06.1 |

## SE-07 — Public-key handler breadth

**Public-key handler breadth**: AES-192 and RC2 content ciphers; key-agreement (ECDH), key-encryption-key and password recipients; RSA-OAEP key transport; sealing to EC keys; per-recipient permissions; applying an envelope's `/P`; a password and recipients together; a sealed incremental save

**Exit criterion.** RFC 5652/5753 structures transcribed; NIST vectors for each primitive; envelopes generated once by an outside tool (dated, bytes only) open; a real public-key-encrypted document from a producer (H-107) closes the derivation risk

**Doc:** [design/pubsec.md](../design/pubsec.md) (amend) · **Depends on:** SG-05

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-07.1 | design | Amend `docs/design/pubsec.md` with every recipient shape and content cipher, permissions per recipient, envelope `/P`, password with recipients, and sealed incremental saves. | `docs/design/pubsec.md` | The amended doc names SE-07.2 to SE-07.6's exit tests. | SG-05 |
| SE-07.2 | build | AES-192 and RC2 content ciphers in enveloped data. | `crates/tinker-pdf-pki/src/enveloped.rs`, `crates/tinker-pdf-crypto/src/aes.rs` | NIST's AES-192 vectors and RFC 2268's RC2 vectors pass; envelopes generated once outside (dated, bytes only) open. | SE-07.1, H-106 |
| SE-07.3 | build | Key-agreement recipients (ECDH, RFC 5753) and sealing to EC keys. *Note:* Added with the signing work, as H-29's default has it: the recipient's private scalar is multiplied on SG-02.3's constant-time path. | `crates/tinker-pdf-pki/src/enveloped.rs`, `crates/tinker-pdf-pki/src/seal.rs` | RFC 5753's structures, transcribed, open and are written; envelopes generated once outside open. | SE-07.2, SG-02.3 |
| SE-07.4 | build | Key-encryption-key and password recipients with AES key wrap, and RSA-OAEP key transport. | `crates/tinker-pdf-pki/src/enveloped.rs`, `crates/tinker-pdf-crypto/src/aes.rs` | RFC 5652's structures open and are written; NIST's key-wrap and OAEP vectors pass. | SE-07.2 |
| SE-07.5 | build | Per-recipient permissions, an envelope's `/P` applied, a password and recipients together, and a sealed incremental save. | `crates/tinker-pdf-pki/src/seal.rs`, `crates/tinker-pdf-cos/src/pubsec.rs`, `crates/tinker-pdf/tests/pubsec_breadth.rs` (new), `docs/features/encryption.md` | `pubsec_breadth.rs` (new): each recipient opens with its own permissions; the combined document opens by either route; the incremental save stays sealed. | SE-07.3, SE-07.4 |

## SE-08 — A host security-handler seam

**A host security-handler seam** for a vendor's own `/Filter`: the host supplies the key or decrypts per object

**Exit criterion.** a test handler opens a fixture encrypted under a private filter name

**Doc:** — · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-08.1 | build | A host security-handler seam for a vendor's own `/Filter`: the host supplies the file key or decrypts per object, and `AuthError::UnsupportedHandler` remains only when no host handler answers. | `crates/tinker-pdf-cos/src/decrypt.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/host_security_handler.rs` (new), `docs/features/encryption.md` | `host_security_handler.rs` (new): a test handler opens a fixture encrypted under a private filter name, by key and by per-object decryption. | — |

## SE-09 — Hidden-information audit and removal

**Hidden-information audit and removal**: report, then remove, private `/Info`, XMP and `/PieceInfo` data, attachments, comments, fields, hidden text (invisible but not OCR, covered by opaque objects, white on white, outside the crop box, microscopic), hidden layers, content outside the boxes, search indexes, outlines, links, actions and scripts, earlier revisions, thumbnails and unreferenced objects

**Exit criterion.** a fixture planting each kind is reported, and is clean after removal (text and raw-stream search)

**Doc:** — · **Depends on:** RC-17, ED-31

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-09.1 | build | The audit's document-level report: private `/Info`, XMP and `/PieceInfo` data, attachments, comments, fields, outlines, links, actions and scripts, search indexes, thumbnails, unreferenced objects and earlier revisions (ED-31). | `crates/tinker-pdf/src/audit.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/hidden_information.rs` (new) | `hidden_information.rs` (new): a fixture planting each kind reports it. | ED-31 |
| SE-09.2 | build | Hidden text and content detected: invisible text not over an image, text covered by opaque objects, white on white, outside the crop box, microscopic, hidden layers, and content outside the boxes, through RC-17's analysis. | `crates/tinker-pdf/src/audit.rs` (new), `crates/tinker-pdf/tests/hidden_information.rs` (new) | A fixture per kind reports exactly the planted text; an OCR layer over its scan is not reported. | SE-09.1, RC-17.3, RC-17.4, RC-17.5 |
| SE-09.3 | build | Removal of each reported kind, through the redaction, trim and sanitise paths. | `crates/tinker-pdf/src/audit.rs` (new), `crates/tinker-pdf/tests/hidden_information.rs` (new), `docs/features/editing.md` | After removal each fixture is clean by text search and by raw-stream search. | SE-09.2, ED-36f, ED-07 |

## SE-10 — Pattern-based redaction

**Pattern-based redaction**: detectors for card numbers (with the Luhn check), IBANs (with the checksum), national identifiers, phone numbers by locale, e-mail, dates and IP addresses; mark, review, apply

**Exit criterion.** planted values found with no false positive on the committed fixtures; `tpdf redact --pattern` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** FT-20, ED-36d

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-10.1 | build | Detectors over extracted text with their quads: card numbers with the Luhn check, IBANs with their checksum, the national identifiers the feature doc lists with their check digits, phone numbers by locale, e-mail, dates and IP addresses. | `crates/tinker-pdf/src/patterns.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/pattern_redaction.rs` (new), `docs/features/editing.md` | `pattern_redaction.rs` (new): planted values are found, and none is found on the committed fixtures that plant none. | FT-20.4, FT-20.5 |
| SE-10.2 | build | Mark, review and apply through ED-36d's marks, and `tpdf redact --pattern`. | `crates/tinker-pdf/src/patterns.rs` (new), `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/pattern_redaction.rs` (new) | Applied patterns leave none of the values by text search; `tpdf redact --pattern` runs over `testdata/` fixtures. | SE-10.1, ED-36d |

## SE-11 — An active-content risk report

**An active-content risk report**: every script (document, field, page, annotation, open action), launch action, URL with its domain, remote go-to target, embedded executable by magic number, rich media, XFA script, submission target and external stream; counts of the keys security triage watches

**Exit criterion.** a fixture planting each item is listed exactly once

**Doc:** — · **Depends on:** AN-20

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-11.1 | build | An active-content report: every script by place, launch action, URL with its domain, remote go-to target, embedded executable by magic number, rich media, XFA script, submission target and external stream, with counts of the keys triage watches. *Note:* The report joins SE-09.1's audit in `audit.rs`. | `crates/tinker-pdf/src/audit.rs` (new), `crates/tinker-pdf/tests/active_content.rs` (new), `docs/features/editing.md` | `active_content.rs` (new): a fixture planting each item lists it exactly once. | AN-20, SE-09.1 |
| SE-11.2 | build | `tpdf audit` printing the report as text and JSON. | `tools/tpdf/src/main.rs` | CLI tests print the planted fixture's report as the facade gives it. | SE-11.1 |

## SE-12 — Content disarm and reconstruction

**Content disarm and reconstruction**: a preset that rebuilds a document from whitelisted content — streams regenerated, objects normalised, revisions dropped, trailing data after `%%EOF` and polyglot payloads (PDF with ZIP, HTML or script) detected and removed, duplicate headers removed

**Exit criterion.** generated polyglots and trailing payloads are detected and absent from the output; the render is unchanged

**Doc:** — · **Depends on:** SE-09

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-12.1 | build | Payload detection: data after `%%EOF`, duplicate headers, and polyglots with ZIP, HTML or script found by their signatures. | `crates/tinker-pdf-cos/src/parse.rs`, `crates/tinker-pdf/src/audit.rs` (new), `crates/tinker-pdf/tests/disarm.rs` (new) | `disarm.rs` (new): generated polyglots and trailing payloads are each detected and named. | SE-09 |
| SE-12.2 | build | The disarm preset: the document rebuilt from whitelisted content, streams regenerated, objects normalised, revisions dropped and payloads left out. | `crates/tinker-pdf/src/audit.rs` (new), `crates/tinker-pdf/src/write.rs`, `crates/tinker-pdf/tests/disarm.rs` (new), `docs/features/editing.md` | Generated polyglots and trailing payloads are absent from the output, and the render is unchanged. | SE-12.1 |
| SE-12.3 | build | `tpdf disarm` and a fingerprint of one disarmed document. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs` | `tpdf disarm` runs over `testdata/` fixtures to the facade's output; the fingerprint holds on every K-07 target. | SE-12.2, K-07 |

## SE-13 — `sanitise` grows to the full scrub

**`sanitise` grows to the full scrub**: thumbnails, hidden text, links, field values reset, review replies, applied redactions, XMP alone

**Exit criterion.** each new flag held to a fixture

**Doc:** — · **Depends on:** SE-09

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-13.1 | build | `sanitise` grows the full scrub: thumbnails, hidden text, links, field values reset, review replies, applied redactions and XMP alone, each a flag. | `crates/tinker-pdf-cos/src/edit/sanitise.rs`, `crates/tinker-pdf/tests/sanitise_flags.rs` (new), `docs/features/editing.md` | `sanitise_flags.rs` (new): each new flag is held to a fixture. | SE-09 |

## SE-14 — Encrypted and multi-volume archives

**Encrypted and multi-volume archives**: ZipCrypto, AES in ZIP (AE-1 and AE-2), 7z AES-256, RAR 5 encryption of stored entries — through a password seam; multi-volume sets and sparse tar entries

**Exit criterion.** each scheme held to its published format description; fixtures generated once by outside archivers (dated, bytes only) open with their passwords

**Doc:** [design/comic-archives.md](../design/comic-archives.md) (amend) · **Depends on:** H-25

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-14.1 | design | Amend `docs/design/comic-archives.md` with each encryption scheme, the password seam H-25 decides and multi-volume sets. | `docs/design/comic-archives.md` | The amended doc names SE-14.2 to SE-14.5's exit tests. | H-25 |
| SE-14.2 | build | ZipCrypto and AES in ZIP (AE-1 and AE-2) through the password seam. | `crates/tinker-pdf-zip/src/lib.rs`, `crates/tinker-pdf-archive/src/lib.rs` | Each scheme holds to its published format description; fixtures generated once by an outside archiver (dated, bytes only) open with their passwords. | SE-14.1, H-106 |
| SE-14.3 | build | 7z AES-256. | `crates/tinker-pdf-archive/src/lib.rs` | The scheme holds to its format description; a fixture generated once outside opens with its password. | SE-14.2 |
| SE-14.4 | build | RAR 5 encryption of stored entries. | `crates/tinker-pdf-archive/src/lib.rs` | The scheme holds to its format description; a fixture generated once outside opens with its password. | SE-14.2 |
| SE-14.5 | build | Multi-volume sets, retiring `MultiDisk`; sparse tar entries are FI-25's. | `crates/tinker-pdf-archive/src/lib.rs`, `docs/features/cbz.md` | Multi-volume fixtures open. | SE-14.2 |

## SE-15 — EPUB resource encryption with caller keys and `signatures.xml`

**EPUB resource encryption with caller keys and `signatures.xml`**: W3C XML Encryption of resources decrypted with a key the caller supplies; `META-INF/signatures.xml` verified

**Exit criterion.** encrypted and signed books generated in the test open and verify; vendor DRM stays a limit (NG-12)

**Doc:** — · **Depends on:** SG-13

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| SE-15.1 | build | EPUB resources under W3C XML Encryption decrypted with a key the caller supplies, through `META-INF/encryption.xml`, retiring `ArchiveRefusal::EncryptedResources` for them; vendor DRM still refused by name (NG-12; H-26's default). | `crates/tinker-pdf/src/epub.rs`, `crates/tinker-pdf/tests/epub_encryption.rs` (new), `docs/features/epub.md` | `epub_encryption.rs` (new): encrypted books generated in the test open with the caller's key and are refused without it; a vendor-DRM book is refused by name. | SG-13 |
| SE-15.2 | test | Books both encrypted and signed, their `signatures.xml` verified through SG-13.4. | `crates/tinker-pdf/tests/epub_encryption.rs` (new) | Encrypted and signed books generated in the test open and verify. | SE-15.1 |
