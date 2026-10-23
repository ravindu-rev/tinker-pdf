# Capability tier — security

[The roadmap](../ROADMAP.md) says how a step runs, what comes next, and which dependencies cross tiers. Each row below keeps its text as it was planned — Item, Today, Exit criterion, Doc, Depends on — and is cut into steps, each one workflow run.

Parity here means every standard security handler and crypt filter read
and written (the legacy ones behind an explicit option), the PDF 2.0
extensions for authenticated encryption and integrity, public-key
encryption in every recipient shape, and the audits a security reviewer
reaches for — hidden information, active content, disarm and
reconstruction — each a typed report before it is a removal.

## SE-01 — Encryption on save below R6

**Encryption on save below R6** — R4 with AES-128, RC4 at 40 and 128 bits — for readers that stop at 1.6, behind an explicit legacy option with a typed weakness warning

**Today.** R6 only (`features/encryption.md:224, 227`); a decision row until 9 October 2026, whose argument was that the readers that need it are the whole reason

**Exit criterion.** each revision's `/O` and `/U` entries, computed with a corpus file's own parameters and known password (`corpus/passwords.tsv`), equal that file's bytes; outputs reopen through the reader

**Doc:** — · **Depends on:** H-18

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-02 — AES-256-GCM crypt filters

**AES-256-GCM crypt filters** (ISO/TS 32003), read and written

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'gcm|ghash' crates tools` finds nothing

**Exit criterion.** NIST GCM test vectors; GHASH constant-time

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-03 — Integrity protection for encrypted documents

**Integrity protection for encrypted documents** (ISO/TS 32004): the MAC written and verified

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'ts.?32004|integrity_mac' crates tools` finds nothing

**Exit criterion.** the MAC recomputed in the test from the extension's definition; a tampered byte is reported

**Doc:** — · **Depends on:** SE-02

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-04 — Crypt-filter breadth on write and one reader gap

**Crypt-filter breadth on write and one reader gap**: embedded-files-only (`/EFF`), named crypt filters, `/Identity`, `/EncryptMetadata false`; `/EncryptMetadata` read from the crypt filter as well as from `/Encrypt`

**Today.** the writer emits AESV3 for everything; `/EncryptMetadata` is read from `/Encrypt` and not from the crypt filter, where Table 27 puts the public-key handler's — a reader gap found when public-key writing landed on 2 October 2026 and recorded in [design/pubsec.md](../design/pubsec.md)'s Risks

**Exit criterion.** fixtures per configuration reopen with the expected streams in clear; the reader honours a crypt filter's `/EncryptMetadata`

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-05 — Unencrypted wrapper documents

**Unencrypted wrapper documents** (2.0 7.6.7): a cover document carrying an `/EncryptedPayload` collection item, created and opened

**Today.** an associated file whose relationship is `EncryptedPayload` is refused (`features/document-model.md:428`); an `EncryptedPayload` written needs an `/EP` dictionary no writer here makes

**Exit criterion.** a wrapper built and reopened yields its payload byte for byte

**Doc:** — · **Depends on:** ED-21

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-06 — Password robustness and introspection

**Password robustness and introspection**: a password tried in alternative encodings for R2–R4, the user password recovered from the owner password for R2–R4, open by a raw file key, report the computed key, which password matched and the crypt parameters, and copy encryption parameters from another document

**Today.** SASLprep for R5/R6; 34 corpus passwords measured (`corpus/passwords.tsv`)

**Exit criterion.** fixtures for each route; R2–R4 recovery recomputed from Algorithm 7 in the test

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-07 — Public-key handler breadth

**Public-key handler breadth**: AES-192 and RC2 content ciphers; key-agreement (ECDH), key-encryption-key and password recipients; RSA-OAEP key transport; sealing to EC keys; per-recipient permissions; applying an envelope's `/P`; a password and recipients together; a sealed incremental save

**Today.** **The content-cipher gap is closed**: `des-ede3-cbc` decrypts — the tables read twice from FIPS 46-3 and adjudicated by 500 NIST CAVP known answers — so the cipher an outside toolkit still picks by default for older recipients now opens rather than being named (`des.rs` in `tinker-pdf-crypto`). AES-192-CBC and RC2, which that toolkit can also be asked for and which no PDF has ever defaulted to, remain refused by name (`features/encryption.md:225`); only key-transport recipients are read (`EnvelopedError::UnsupportedRecipientKind`), and other recipient shapes were a decision of the roadmap until 9 October 2026; sealing to non-RSA keys and incremental sealing are refused (`features/encryption.md:219-224`). **The risk the old row kept open is the derivation**: the 7.6.5 key derivation is held only to a second implementation by the same author, and that risk closes the day a real public-key-encrypted document arrives. Writing landed on 2 October 2026 and the outside toolkit opens what this engine seals, which adds interop evidence for the envelope and the cipher but not for the derivation. Re-measured 15 September 2026, and the clause survives — `/Adobe.PubSec` and `/Recipients` are each **zero across all 5 605 PDFs** under `corpus/files`; ten files match `pubsec` case-insensitively and every one of the ten is `/PubSec` inside a signature's `/Prop_Build`, a different key entirely ([features/encryption.md](../features/encryption.md); [design/pubsec.md](../design/pubsec.md), Risks)

**Exit criterion.** RFC 5652/5753 structures transcribed; NIST vectors for each primitive; envelopes generated once by an outside tool (dated, bytes only) open; a real public-key-encrypted document from a producer (H-107) closes the derivation risk

**Doc:** [design/pubsec.md](../design/pubsec.md) (amend) · **Depends on:** SG-05

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-08 — A host security-handler seam

**A host security-handler seam** for a vendor's own `/Filter`: the host supplies the key or decrypts per object

**Today.** `AuthError::UnsupportedHandler` (`features/encryption.md:217`)

**Exit criterion.** a test handler opens a fixture encrypted under a private filter name

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-09 — Hidden-information audit and removal

**Hidden-information audit and removal**: report, then remove, private `/Info`, XMP and `/PieceInfo` data, attachments, comments, fields, hidden text (invisible but not OCR, covered by opaque objects, white on white, outside the crop box, microscopic), hidden layers, content outside the boxes, search indexes, outlines, links, actions and scripts, earlier revisions, thumbnails and unreferenced objects

**Today.** `sanitise` removes JavaScript, actions, embedded files and metadata with a report (`features/editing.md:184-241`); nothing detects hidden text

**Exit criterion.** a fixture planting each kind is reported, and is clean after removal (text and raw-stream search)

**Doc:** — · **Depends on:** RC-17, ED-31

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-10 — Pattern-based redaction

**Pattern-based redaction**: detectors for card numbers (with the Luhn check), IBANs (with the checksum), national identifiers, phone numbers by locale, e-mail, dates and IP addresses; mark, review, apply

**Today.** redaction applies caller rectangles; no regex engine

**Exit criterion.** planted values found with no false positive on the committed fixtures; `tpdf redact --pattern` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** FT-20, ED-36d

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-11 — An active-content risk report

**An active-content risk report**: every script (document, field, page, annotation, open action), launch action, URL with its domain, remote go-to target, embedded executable by magic number, rich media, XFA script, submission target and external stream; counts of the keys security triage watches

**Today.** `sanitise` reports what it removes

**Exit criterion.** a fixture planting each item is listed exactly once

**Doc:** — · **Depends on:** AN-20

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-12 — Content disarm and reconstruction

**Content disarm and reconstruction**: a preset that rebuilds a document from whitelisted content — streams regenerated, objects normalised, revisions dropped, trailing data after `%%EOF` and polyglot payloads (PDF with ZIP, HTML or script) detected and removed, duplicate headers removed

**Today.** full rewrite and `sanitise`

**Exit criterion.** generated polyglots and trailing payloads are detected and absent from the output; the render is unchanged

**Doc:** — · **Depends on:** SE-09

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-13 — `sanitise` grows to the full scrub

**`sanitise` grows to the full scrub**: thumbnails, hidden text, links, field values reset, review replies, applied redactions, XMP alone

**Today.** four `Sanitise` flags and `ALL`

**Exit criterion.** each new flag held to a fixture

**Doc:** — · **Depends on:** SE-09

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-14 — Encrypted and multi-volume archives

**Encrypted and multi-volume archives**: ZipCrypto, AES in ZIP (AE-1 and AE-2), 7z AES-256, RAR 5 encryption of stored entries — through a password seam; multi-volume sets and sparse tar entries

**Today.** refused (`ArchiveRefusal::Encrypted`, `MultiDisk`; `TarEntryRefused`); a named non-goal of the roadmap until 9 October 2026 ([design/comic-archives.md](../design/comic-archives.md))

**Exit criterion.** each scheme held to its published format description; fixtures generated once by outside archivers (dated, bytes only) open with their passwords

**Doc:** [design/comic-archives.md](../design/comic-archives.md) (amend) · **Depends on:** H-25

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## SE-15 — EPUB resource encryption with caller keys and `signatures.xml`

**EPUB resource encryption with caller keys and `signatures.xml`**: W3C XML Encryption of resources decrypted with a key the caller supplies; `META-INF/signatures.xml` verified

**Today.** refused (`ArchiveRefusal::EncryptedResources`); a named non-goal of the roadmap until 9 October 2026, on the ground that this engine holds no key ([features/epub.md](../features/epub.md)) — a caller's key answers that

**Exit criterion.** encrypted and signed books generated in the test open and verify; vendor DRM stays a limit (NG-12)

**Doc:** — · **Depends on:** SG-13

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*
