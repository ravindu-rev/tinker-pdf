# Human verification and owner decisions

[The roadmap](../ROADMAP.md) holds the machine work; these are the acts only a person can do.

Every manual verification and every decision that needs a person, in one
file. A row whose only remainder is a person's act is here,
and its machine half stays in its tier; every row
or step whose exit needs a person's act names the H row in its Depends on.
Each row says what the person does, what it blocks, and — where the work can
build toward something meanwhile — the default it builds toward until the
person acts, so the machine work in the tiers never stalls on a decision. A
decision or a review also says why no machine can make it; an access,
purchase, secret, machine or release row needs no such column, because each
is a person's for one reason — it takes an account, a payment, a signature,
a legal act or a physical machine. Three groups, ordered by what each
blocks, and the standing duties last. A decision the owner has taken leaves
this file for [ruling 23](../rulings.md) or the ruling that states it.

## Decisions with a default the work already builds toward

| ID | Decision | Why the owner | Blocks | Default until decided |
| --- | --- | --- | --- | --- |
| H-15 | Ruling 5's image-run trade: a tile pays the page's image work to be byte-equal in the one pinned shape | a speed-against-exactness trade, and speed now has targets | RC-21 | keep the pinned exception; measure the cost first |
| H-46 | HEIC decoding given the HEVC patent pool | legal | FI-09 | not scheduled until ruled |
| H-47 | AVIF and JPEG XL conformance data licences | licence | FI-07a, FI-08 | fetch locally only, never commit, until ruled |
| H-141 | Rule, with FT-06.17's figures in hand, on what drawing colour glyphs costs: (a) whether books and Markdown pages draw their colour faces in colour, at the bytes a page grows by and the time to open and paginate it in release and debug against today's plain-outline or `.notdef` glyphs; (b) which bitmap strike an `sbix` or CBDT glyph is embedded at; (c) whether each paint FT-06.1 finds ISO 32000-2's transparency model cannot express is written as an image of the glyph at a stated resolution, or with the nearest construct and a typed warning. | Each option gives something up: bytes and time on books that convert today; resolution independence and the single renderer `crates/tinker-pdf/src/epub/svg.rs`'s module comment argues for; or, with (c)'s second option, the row's 'by the specification's formulas' for those paints. AGENTS.md leaves every price to the owner. | FT-06.18 and FT-06.19, and through FT-06.19 the row's exit. | The colour writer lands for `DocumentBuilder` callers (FT-06.10 to FT-06.16 and FT-06.20) with the strike a parameter and each inexpressible paint drawn source-over under a typed warning; books and Markdown pages draw their colour faces as they do today. |
| H-148 | Accept the measured price of running form scripts on the engine in place of the subset, which `docs/design/form-script-policy.md` keeps as the build only until FJ-01 lands: the form suites' time on each path in release and debug and each profile's wasm beside H-04's budget, both measured in FJ-01.38; the answers FJ-01.38's committed list shows changing, among them `length` and string comparison counting UTF-16 code units where the subset counts code points (`crates/tinker-pdf-cos/src/script.rs:1956`, `script.rs:2292-2299`); and the two decisions of form-script-policy.md that give way, decision 4 (recursion refused by name) in FJ-01.39 and decision 3 (document scope a table of functions) in FJ-01.40. | Each is something that works today (bytes, time, a refusal a test pins, an answer a form computes) given up for a complete language; only the owner accepts a price, and a design doc's decision is reversed by the owner, not by the session that wants it gone. | FJ-01.39 and FJ-01.40 | The engine lands in its own crate and beside the subset in `tinker-pdf-cos`, held to test262 and to the form suites' scripts; `recalculate`, `formatted_value`, keystroke and validate keep running on the subset, unchanged, until the owner rules. |
| H-164 | **Decided by rule** (ruling 14): taken where FT-37.2's figures read no case worse and more plain-PDF and conformance lines better than worse, whatever its time; a case read worse comes back to the owner here. FT-37: say whether the reader resolves a right-to-left paragraph across its lines, at what FT-37.2 measures it reads better and worse in every PDF. | Ruling 14 binds every output that reads `Page::text`; reading a paragraph across its lines amends it. | FT-37.3. | Each line is put in logical order by itself, as ruling 14 states; the line opening `25٪` stays in ROADMAP's Bugs. |

## Reviews and manual verification

| ID | What the person does | Why no machine can | Blocks |
| --- | --- | --- | --- |
| H-60 | **Read the nine reviewed goldens** against the clauses their headers name; put a name and the date in place of `unreviewed` in each header; empty `UNREVIEWED`. `crates/tinker-pdf/tests/render_goldens.rs` parses each `.ppm`'s header, refuses a field that is absent, blank or whitespace, re-renders every family and compares byte for byte, and holds a size ceiling so reviewing one stays a real act; `UNREVIEWED` lists all nine families. S, and it is a reading rather than work | the tier exists to answer "is this the picture the clause describes", which fixtures sharing the implementer's reading cannot; ideally the reviewer is not the implementer | tier 1's exit |
| H-61 | **A dated visual comparison against outside viewers** on a sample of corpus pages, recorded with tool, version, command and date — an input a person reads, never a check | ruling 13's amendment admits it only as a record | nothing formally; the only route back toward the first lost property |
| H-62 | **Judge PDF/UA's meaning clauses** for documents whose accessibility matters, above all ones this engine writes | an `/Alt` that lies is a well-formed `/Alt` (NG-13) | any claim that a written document conforms |
| H-63 | **Walk engine-written tagged documents through screen readers** (two desktop, one mobile): reading order, alternate text, tables, fields, language switches | the output is speech and navigation judged by a user | a credible accessibility claim; SD-09 |
| H-64 | **Write, or source from a person, expected shaping output** for every script shaped and unverified (the Brahmic and Southeast Asian list, Syriac's Alaph, N'Ko, Mongolian, Adlam, Thaana, Mandaic, Hanifi Rohingya, Phags-pa, joining Brahmic topographical features, Naskh and vowelled Arabic), and for the variation-aware, vertical, Graphite and justification fixtures | ruling 13 admits expectations a person wrote and forbids another shaper's recording; a reader of the script is needed | FT-09, FT-07a, FT-07b, FT-07d, FT-07e |
| H-65 | **Judge small text** with stem darkening, hinting and an autohinter | a fidelity question ruling 13 cannot adjudicate | FT-04a–d |
| H-66 | **Review the invented appearances** (`Text`'s note, `Caret`, `Stamp`, `FileAttachment`, `Sound`, captions, ¶, cloudy and beveled borders) and sign them as reviewed goldens | 12.5.6 gives no outline | AN-02, AN-04, AN-05 |
| H-67 | **An independent cryptographic review** of the private-key path: constant-time arithmetic, blinding, zeroisation, side channels in wasm | correctness vectors cannot show the absence of a timing leak | shipping SG-01…SG-09 |
| H-68 | **Signing interoperability and legal sign-off**: run signed output through outside validators and widely used viewers once, as a dated measurement; decide what the docs may claim (PAdES levels; nothing resembling a qualified signature without counsel); mark committed throwaway keys so secret scanning on a public repository knows them | outside programs may only be dated measurements; legal claims are the owner's | SG-07's public claims; also SG-06, whose steps wait on it |
| H-69 | **A security review of the script sandbox**: exhaustion, prototype pollution, host-API escape, regular-expression backtracking, the fuzz budget | fuzzing cannot show the absence of an escape | shipping FJ-01…FJ-18 |
| H-70 | **Write and review the object models' expected outputs** from the published references — the form object model (AForm rounding, separators, currency and negative styles; `util.printd` patterns; event order; every member FJ-18 lists), the XFA scripting object model and the 3D JavaScript API — and decide those references' admissibility | the object models have no conformance suite; ruling 13 admits person-written expectations | FJ-06, FJ-08…FJ-11, FJ-13, AN-11c, AN-30 |
| H-71 | **Transcribe OCR ground truth** for real scans (people other than the implementers) and approve character and word error targets per script | another recogniser may not adjudicate; the authors' own annotation proves nothing | OC-08a…e, OC-15c, OC-17 |
| H-72 | **Review OCR output** on a stratified sample (clean print, degraded scans, tables, columns, non-Latin) and sign it | quality judged by a reader | the OCR tier's claims |
| H-73 | **Review each new golden family** K-08 adds (annotation appearances, signature appearances, fields under formatting scripts, OCR layer alignment, EPUB/HTML/Markdown/FB2 pages, SVG and XPS pages, `to_svg`) | as H-60 | each family's `UNREVIEWED` entry; also SG-16, whose steps wait on it |
| H-74 | **Attempt recovery from redacted output** on a sample (optional) | redaction is a disclosure feature; an adversarial person is the review a suite cannot be | redaction completeness claims |
| H-75 | **Read the roadmap** before it is published: nothing names a compared engine, nothing reads as a comparison | a grep checks a list; only the owner knows the list is complete | publishing the roadmap; the completeness of `xtask/never-named.sha256` |
| H-76 | **Review the one-time fingerprint change** an optimisation that moves pixels makes — the glyph cache's sub-pixel phase quantisation (P-06), table-driven colour transforms (P-22) | a changed golden is reviewed before it is committed | P-06, P-22; also P-14, whose steps wait on it |
| H-78 | **Approve every lowered ratchet floor and widened benchmark band** (standing) | a session that both makes a change and widens the band that would catch it is self-approval | the commit that does it |
| H-79 | **Review XFA rendering of real government forms** against what their authors intended | no adjudicator exists for an author's intent | AN-29's claims |
| H-80 | **Print PostScript, PCL, PWG, URF and PCLm output on physical printers** | a printed page is judged by eye | FO-08a/b's and FO-09a–e's claims |
| H-81 | **Read the print-production visual suites' pages** (overprint, transparency, spot colour) | the suites are designed for human eyes | RC-03, RC-04's claims |
| H-82 | **Exercise script-heavy forms in widely used interactive viewers once**, as a dated measurement read by a person (never a check) | ruling 13: an outside program is a dated record at most | FJ-06…FJ-13's claims of behavioural parity |
| H-138 | Review the one-time change P-04.7 makes to written bytes: with document writing on the ratio level every compressed stream re-encodes, so `the_synthesised_document_is_the_same_bytes_on_every_target` (`crates/tinker-pdf/tests/determinism.rs:2406`), and any written-document hash beside it that holds such a stream, moves once while every page decodes and renders the same; approve re-recording them. | A moved determinism fingerprint is a finding by this repository's rule, and only a person accepts one as intended, as H-76 does for moved pixels. | P-04.7, and through it P-04.8 |
| H-142 | Write, or source from a person, the expected output of a fixture for each AAT state machine (`morx` rearrangement, contextual, ligature and insertion; `kerx` formats 1 and 4) for which FT-07c.1 finds the TrueType Reference Manual gives no worked example. | The row's exit holds each state machine to the specification's worked examples; where the manual has none, ruling 13 admits only an expectation a person wrote, never another shaper's recording, and what a machine should produce is not a machine's to say. | FT-07c.20 |
| H-144 | Review each move of a shaping fingerprint FT-08's steps make — `SHAPING` in `crates/tinker-pdf-shape/tests/fingerprints.rs`, and any facade fingerprint a shaping change reaches — against the cases the step recorded as moving it, and accept it or send it back as a determinism defect. | AGENTS.md forbids editing a determinism fingerprint to match new output — a moved hash is a finding — and only a person can rule that a deliberate change, and not a disagreement between targets, moved it. | FT-08.5, FT-08.6, FT-08.7, FT-08.8, FT-08.9, FT-08.10, FT-08.13, FT-08.14, FT-08.16, FT-08.17 |

## Access, purchases, secrets, machines and releases

| ID | What the person obtains or does | Blocks |
| --- | --- | --- |
| H-90 | Confirm the reading of the WHATWG grant as BSD-3-Clause for the two vendored data trees, or add CC-BY-4.0 to the allowlist, or rule it wrong | FI-23, the FB2 and HTML rows as landed |
| H-91 | Ratify the allowlist entries `W3C` and `OFL-1.1` | the allowlist as policy |
| H-93 | Establish the print-production output suite's licence terms (no licence text on the pages fetched) | SD-11 milestone 2 |
| H-94 | Rule on the licence walls: Symbol and ZapfDingbats stand-ins, the ICC profiles, the Mac encodings, the JPEG XR bitstreams, a shaping library's test faces, RAR | FT-03, NG-02, NG-03, NG-06 |
| H-96 | Purchase ISO 14289-1 and -2 | SD-04's second reading |
| H-97 | Purchase ISO 15930-4, -6, -7 and -9 | SD-11's X-4 and X-6 |
| H-98 | Obtain the T.83 compliance data. ITU-T T.83 (ISO/IEC 10918-2) is the compliance data published for exactly the JPEG encoder's purpose; its clause 4.4 ships the data on diskettes accompanying the document rather than inside it, the Recommendation is sold, and T.84's clause 4.2.1 names ISO and ITU as its source. Meanwhile the FDCT is held to A.3.3's equation recomputed in `f64` in the test — the standard's formula, not the standard's numbers — and what *is* adjudicated by published data is Figure A.6's zig-zag read twice, Tables K.1 and K.2 read twice, K.3.3's four DHT byte lists appearing verbatim in the output, and two whole entropy-coded segments derived from Tables K.3's and K.5's printed code words with no implementation in the middle. | the JPEG encoder's DCT adjudication (ED-28) |
| H-99 | Obtain the T.832 conformance bitstreams | NG-06 |
| H-100 | Purchase the colour test kit sold as print series | SD-11's X-4 evidence |
| H-101 | Find the CGATS application notes Version 3 | SD-11's 2001/2002 levels |
| H-102 | Purchase or obtain the standards the new rows need: ISO 21757-1 (ECMAScript for PDF), ISO 19444-1 (XFDF), ISO 24517-1 (PDF/E), ISO 16612-2/3 (PDF/VT), ISO 23504-1 (PDF/R), ISO 19593-1, ISO 17972-4, ISO 18619, ISO/IEC 18181 (JPEG XL), ISO 14739-1 (PRC); the free ones fetched (ECMA-363, ECMA-376, the XFA specification, the PostScript and PCL references, the ETSI standards) | FJ-18, AN-08, SD-13…15, FO-25, FO-26, RC-02, FI-07a/b, AN-11a, FI-02a–g, AN-27, FI-17a–c, FI-18a–c, SG-07, SG-12 |
| H-103 | A Swift toolchain (a person's Mac) to build and verify Swift | SU-13 |
| H-104 | Verify the .NET binding where an SDK exists | SU-12's .NET column |
| H-105 | Verify Java 22 and later (a person's run) | the "flagless from 22" claim |
| H-106 | Make fixtures only a particular machine or producer can make: JPEG XR files per refused format, a producer's XPS package per feature, signature and encryption fixtures generated once by an outside toolkit (dated) | FI-12, CD-13, SG-05, SE-07, SE-14; also SG-10, FI-01, FI-05, FI-19, FI-20a and FI-25, whose steps wait on it |
| H-107 | Find real documents (or authorise a crawl): a public-key-encrypted PDF, an EPUB declaring `@layer` and one carrying WOFF, a third party's PDF/X-1a:2003 or X-3:2003 file, an arithmetic-coded JPEG in a document, a customizable `<select>`, an FB2 corpus — and the new corpora: scans, script-heavy forms, signed documents across producers, XFA forms, PDF 2.0 files, MOBI and Office files, damaged files | SE-07, CD-07, SD-11, NG-07, FI-22, FI-23, the OCR and form tiers' evidence |
| H-108 | Watch a release run end to end: merge `develop` into `main` at a workspace version with no `v<version>` tag, and `release.yml` builds and publishes the artefacts, tags the version and makes the GitHub release | publishing; also SU-17 and SU-20, whose steps wait on it |
| H-110 | **Own a pinned benchmark machine** — fixed CPU and frequency, isolated cores, no co-tenants — on which MS-03's absolute figures are measured locally | MS-03's absolute check (tier P gates without it, through MS-02); also MS-09, whose steps wait on it |
| H-113 | **Publish packages**: confirm the `registries` environment's protection rules (not readable from a session); hold and rotate the four publishing secrets; decide and reserve package names; decide whether Go, Ruby, Java and Swift get packages | SU-17's publishing half; the maturity axis |
| H-114 | **The 0.1.0 freeze**: after H-10, H-19 and the new API surfaces (signing, scripts, OCR), or they become breaking changes | H-113; SU-18 |
| H-115 | Release engineering beyond today's registries: Maven Central (account and signing key), RubyGems, Go module tags, the Swift package index; code signing and notarisation of native libraries and wheels | SU-17, SU-15; also SU-20, whose steps wait on it |
| H-117 | A hardware key store session (PKCS#11 token, smart card or HSM) to confirm SG-27's provider on real hardware — SG-27.2's local run signs through a software token | SG-27's hardware claim |
| H-118 | A remote-signing test account, plug-test registration, trusted-list endpoints | SG-24, SG-12, SG-14 |
| H-119 | Apply to a public continuous-fuzzing service | K-10 |
| H-120 | Export-control notification for published cryptographic source; licence review of trust-list and model data | publishing with SG rows |
| H-121 | A security-advisory process: advisories and identifiers issued, a disclosure timeline | — |
| H-122 | Obtain the published definition of JPEG XL as a PDF image filter — its name, parameters and colour handling — once it is published | FI-07c |
| H-159 | Purchase or obtain the barcode symbology specifications ED-49 holds its symbols to: ISO/IEC 15417 (Code 128), 16388 (Code 39), 15420 (EAN/UPC), 16390 (ITF), 18004 (QR), 23941 (rMQR), 16022 (Data Matrix), 15438 (PDF417), 24728 (MicroPDF417), 24778 (Aztec), 16023 (MaxiCode) and 24724 (DataBar); the free ones fetched (the GS1 General Specifications, the postal and payment-profile specifications the design names). | ED-49.3 to ED-49.13, and through them ED-50's read-back tests. |
| H-160 | Purchase ISO 15930-8 (PDF/X-5g, X-5n and X-5pg) as reading for sessions, never committed. | SD-11.11, and through it SD-11.13's X-5 levels. |
| H-161 | Obtain EN 16931-1 and its syntax bindings, EN 16931-3-2 for UBL and -3-3 for CII, sold by national standards bodies, and confirm the terms of the published validation artefacts, the CII and UBL schemas, the business-rule artefacts and their example and negative invoices, and of the XRechnung test suite, before any is vendored or fetched. | SD-24b.2 to SD-24b.10. |

## Standing duties

| ID | Duty |
| --- | --- |
| H-130 | Triage private vulnerability reports; acknowledge within a few days; ship fixes in a patch release with credit |
| H-131 | Review the hand-rolled cryptography — mandatory once the private-key path exists (H-67) |
| H-132 | Follow the host application's test changes into `tinker_parity.rs` (ruling 12) |
| H-133 | Re-review any golden that changes before it is committed |
