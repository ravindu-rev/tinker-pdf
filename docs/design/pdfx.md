# PDF/X validation and writing

When this is done, the engine answers "does this file conform to PDF/X, and
if not, which clause of ISO 15930 did it break?" with the typed findings
`Document::validate_pdfa` already gives for ISO 19005, and
`DocumentBuilder::archival` gains a print flavour that either writes a file
this engine's own validator finds nothing wrong with, or refuses at build time
naming the clause. The roadmap row reads "an ISO 15930 rule group; the
archival profile grows a PDF/X flavour", and this document exists because that
row is L-sized and an L-sized row is not scheduled without one.

**The first thing this document has to settle is not the design but the
adjudicator, because the honest answer is different from PDF/A's.** ISO 19005
has a 2 371-file annotated corpus in this tree, and every rule in
`crates/tinker-pdf/src/pdfa/` is held to it. ISO 15930 has **no annotated
conformance corpus anywhere that this project could find**, and the
requirement bodies of every part are behind ISO's paywall. What exists is
written down below, with what each piece can and cannot decide, so that the
roadmap schedules a validator knowing it would have a false-positive bar and
no false-negative bar — which is a different proposition from the one
[design/pdfa.md](pdfa.md) delivered.

## Scope

- **A validator** over the opened `CosDocument`, one rule group per
  machinery exactly as `pdfa/` is organised, keyed by the flavours ISO 15930
  defines for complete exchange: PDF/X-1a:2001 (ISO 15930-1:2001),
  PDF/X-3:2002 (15930-3:2002), PDF/X-1a:2003 (15930-4:2003), PDF/X-3:2003
  (15930-6:2003), PDF/X-4 and PDF/X-4p (15930-7:2010), and PDF/X-6, X-6p and
  X-6n (15930-9:2020, on ISO 32000-2). Verdicts are the `Verdict { flavour,
  findings, coverage }` shape `pdfa.rs` already has; a finding names the
  clause as the part numbers it and the object it is about (ruling 10,
  [rulings.md](../rulings.md)).
- **Flavour detection** from where ISO 15930 puts the claim, which is not
  where ISO 19005 puts it: the `GTS_PDFXVersion` key of the `/Info`
  dictionary, plus `GTS_PDFXConformance` for the 2001 levels, and for X-4 and
  later the identification schema in the XMP packet as well (15930-7 6.10
  "Metadata and document identification" and 6.11 "PDF/X-4 file
  identification"; 15930-9 6.11.2 "Namespaces and prefixes" and 6.11.3
  "Version and conformance level identification"). 15930-4 clause 5 is
  explicit that nothing else counts: *"Neither the version number in the
  header of a PDF file, nor the value of the Version key in the Catalog of a
  PDF file shall be used in determining whether a file is in accordance with
  this part of this International Standard."*
- **The evidence base**, pinned in `corpus/corpora.lock` like every other
  corpus: the Ghent PDF Output Suite 5.0 and the Altona Test Suite 1.2
  "Online Version" — conforming files, and only conforming files. What they
  can decide is argued below.
- **A writer flavour** on `ArchivalProfile` (`crates/tinker-pdf-cos/src/build.rs`):
  the `/Info` identification keys, `/Trapped`, an output intent with
  `/S /GTS_PDFX`, a `/TrimBox` on every page, and every buildable-but-
  nonconforming request refused at the call that makes it, in the style
  `ArchivalRefusal` already has.
- **Surfacing**: `Document::validate_pdfx`, and `--pdfx` on `tpdf check`.

## Non-goals

- **PDF/X-2 and PDF/X-5.** ISO 15930-5 (PDF/X-2:2003, withdrawn) and
  15930-8 (PDF/X-5) define *partial* exchange: the file references graphical
  content or an ICC profile held elsewhere. A blind validator cannot settle
  whether the referenced thing exists, and the CGATS application notes say of
  X-2 that "communication must occur between the sender and receiver" — the
  opposite of what a rule engine over one file can check. X-4p and X-6p, which
  reference only an external *profile*, are in scope because the reference
  itself is checkable.
- **PDF/X-1:2001.** The 2001 level that admitted OPI and encryption; the
  application notes call it deprecated and every later part dropped it. A file
  claiming it is reported as claiming a level this build does not validate,
  which is a finding rather than a silence.
- **The ICC characterization registry.** 15930's output intent may name a
  registered printing condition instead of embedding a profile, and the
  application notes (2.16.2) expect preflight tools to "be shipped with a list
  of the characterizations included in the ICC registry". That list is data
  with a date on it, and vendoring it is a decision to take when a corpus file
  turns on it; until then a `RegistryName` is checked for shape and its
  identifier is not looked up.
- **Trapping, screening, proofing.** `/Trapped` is a flag this build reads
  and writes; it traps nothing. Colour-managed rendering with the output
  intent's profile is the renderer's row, not this one.
- **Conversion.** No "make this file PDF/X". Validation reports; the builder
  conforms; nothing rewrites colour or fonts — the same line
  [design/pdfa.md](pdfa.md) draws.
- **Receiver restrictions ("PDF/X Plus").** The notes describe publishers
  forbidding JPEG or TrueType on top of the standard (2.19). That is a policy
  layer a caller writes over the findings, not a rule here.

## What third-party material exists, and what each piece decides

Ruling 13 admits data and refuses programs. For PDF/A the data was decisive:
2 896 files each annotated `pass` or `fail` by the people who wrote the
conformance suite. For PDF/X the search was made on 16 September 2026 and this
is everything it found.

**The corpora in this tree carry no annotated PDF/X fixture.** The veraPDF
corpus has 2 907 files in sixteen directories — PDF/A parts 1 to 4, PDF/UA-1
and -2, Isartor, TWG, two ISO 32000 sets and `Undefined` — and none is a
PDF/X test; its own README names ISO 19005, ISO 14289, ISO 32000-1 and
ISO 32000-2 and no other standard. Across all five fetched corpora, **23
files carry a `GTS_PDFXVersion` key** at all — 5 in pdfjs, 18 in SafeDocs —
and 25 carry the string `/GTS_PDFX` anywhere (counted by a raw byte search,
which misses an `/Info` dictionary inside an object stream, so these are
floors). The version strings that search could read are `PDF/X-4`,
`PDF/X-3:2002`, `PDF/X-1:2001` and one that is empty. None of the 23 is
annotated by anybody; they are real producers' claims, and they are the
false-positive check outside any suite, the role the fifteen real PDF/A-1a
files play for the level A rules.

**The published suites are all conforming files.** Three exist, and each was
built to test an *output device*, not a validator:

| Suite | Flavour | Contents | Terms, as read on 16 September 2026 |
| --- | --- | --- | --- |
| Ghent PDF Output Suite 5.0 (gwg.org/gos5) | PDF/X-4 (ISO 15930-7) | a test-page package of six pages with 48 assembled patches, and an individual-patch package, in three categories the suite calls CMYK, SPOT and CMS, the last being "ICC-based objects allowed in PDF/X-4"; per-patch documentation "distributed along with the patch" | free download; **no licence text on either page fetched** |
| Altona Test Suite 1.2 Online Version (eci.org) | PDF/X-3 (ISO 15930-3) | three A3 pages — Altona Measure, Altona Visual, Altona Technical, the last "864 carefully structured patches" for overprinting and font formats | its documentation: "freeware. However nobody is allowed to redistribute, change or modify … without prior permission in writing from European Color Initiative (ECI)" |
| Altona Test Suite 2.0 Application Kit | adds PDF/X-4 | ten reference print series | sold; not obtainable as data |

Both free suites are admissible and both go into `corpora.lock` with
`redistribute = no`, which is this project's posture for every corpus
regardless of upstream's terms. **What they decide is one direction only**:
every file in them must produce zero findings under the flavour it claims,
which is the false-positive assertion `tests/pdfua.rs` already makes over the
195 conforming PDF/UA fixtures. What they cannot decide is whether a rule
*fires when it should*, because no file in either suite is wrong on purpose.
A rule with a typo that never fires passes both suites.

**The standards' own text is partly in hand.** ISO sells every part. The
preview samples a standards reseller publishes were fetched on 16 September
2026 — `WebFetch` could not read their text layers, and this engine's own
`tpdf text` could, which is the fourth time that pattern has held here — and
they give, verbatim, the foreword, the scope, the normative references, the
terms and the conformance clause of ISO 15930-1:2001, 15930-4:2003,
15930-7:2010 and 15930-9:2020, plus the full table of contents of each. What
that buys is the **clause numbering** every finding will cite:

| Part | Technical-requirement clauses in its table of contents |
| --- | --- |
| 15930-1:2001 | 6.3 to 6.21, with 6.5, 6.13 and 6.17 the three that separate PDF/X-1a from PDF/X-1; annexes A to D informative |
| 15930-4:2003 | 6.1 data structure, 6.2 colour, 6.3 fonts, 6.4 file specifications, 6.5 data compression, 6.6 trapping, 6.7 file identification, 6.8 bounding boxes, 6.9 extended graphics state, 6.10 PostScript XObjects and the `PS` operator, 6.11 the Encrypt dictionary, 6.12 alternate images, 6.13 annotations, 6.14 actions and JavaScripts, 6.15 `BX`/`EX`, 6.16 transparency, 6.17 viewer preferences |
| 15930-7:2010 | 6.1 to 6.27: general, non-print elements, complete exchange, colour, fonts, encoding of name objects, external and embedded files, stream filters, trapping, metadata and document identification, PDF/X-4 file identification, bounding boxes, extended graphics state, PostScript XObjects, encryption and access control, images, annotations, actions and JavaScripts, `BX`/`EX`, transparency, viewer preferences, alternate presentations, rendering intents, optional content, architectural limits, XFA forms, JPEG 2000 images; Annex A (normative) PDF/X-4p |
| 15930-9:2020 | 6.1 to 6.19: general, print and non-print elements, intended visual appearance, complete exchange, file structure (6.5.1 to 6.5.7), colour (6.6.1 to 6.6.3), graphics (6.7.1 to 6.7.5), fonts (6.8.1 to 6.8.5), bounding boxes, trapping, metadata and document identification (6.11.1 to 6.11.5), annotations, interactive forms, actions, optional content, viewer preferences, alternate presentations, document requirements, spectral data (CxF); annexes A and B for X-6p and X-6n |

What it does **not** buy is a single requirement body. Clause 6 of every part
is past the preview's last page. A rule written from a clause *title* is a
guess, and the Annex B and JPX experience in this tree — four wrong tables,
37 of 182 citations wrong when drafted from a text that was in hand — is the
measured cost of guessing from less.

**The one free restatement of the requirements is the CGATS application
notes.** *Application Notes for PDF/X Standards, Version 4* (September 2006,
NPES for CGATS SC6 TF1) is public: "permission to use, copy and distribute
them for any purpose is hereby granted without fee, provided that the contents
of these notes are not altered". It covers the 2003 levels only and defers the
2001 and 2002 ones to its Version 3, which was not found. It states in prose
what a conforming file carries, and it states its own standing — "If there is
a conflict between these Application Notes and any part of ISO 15930:2003,
the standard will always take precedence" — so a rule taken from it is a
transcription of a secondary source and is recorded as one. What it gives:

- **Identification** (2.3): a conforming PDF/X-1a:2003 file is "a PDF file"
  with `GTS_PDFXVersion` in the `/Info` dictionary equal to
  `(PDF/X-1a:2003)`; likewise `(PDF/X-3:2003)`. 15930-4 clause 5 adds that a
  2001-level file carries `(PDF/X-1:2001)` in `GTS_PDFXVersion` and
  `(PDF/X-1a:2001)` in `GTS_PDFXConformance`.
- **Compression** (2.8): any lossless filter "other than LZW"; JPEG the only
  lossy one; "JBIG2 compression may not be used".
- **Boxes** (2.10): `/MediaBox` required; "each PDF/X page shall include
  either an ArtBox or TrimBox, but not both"; a `/BleedBox` is optional and
  neither ArtBox nor TrimBox may extend beyond it; the same for `/CropBox`.
- **Encryption** (2.11): "None of the PDF/X standards covered by these
  application notes permit the use of PDF-based encryption."
- **Output intent** (2.16, 3.3, 5.1): required, `/S /GTS_PDFX`; a
  characterization in the ICC registry may be named by
  `OutputConditionIdentifier` plus `RegistryName`, otherwise
  `DestOutputProfile` is required; under X-3, device-independent colour
  anywhere makes the embedded profile mandatory; under a CMYK intent
  `DeviceRGB` is not allowed and must go through a `DefaultRGB`; the rules
  apply to the alternate spaces of `Separation`, `DeviceN`, `Indexed` and
  `Pattern`. Table 2 of the notes gives the four dictionary shapes verbatim.
- **Trapping** (2.17): `/Trapped` required, "a name object — /True or
  /False — and not a boolean"; `/Unknown` not permitted.
- **Fonts** (2.18): every font used is embedded; subsets neither required nor
  prohibited.
- **Transparency** (2.25) prohibited in the 2003 levels; **PostScript
  XObjects and the `PS` operator** (2.26) prohibited; **annotations** (2.28)
  "must fall entirely outside the BleedBox", `PrinterMark` inside the bleed
  but outside the trim or art box; **private `/Info` keys** text strings
  (2.29).
- **Colour under X-1a** (3.1.1): "An ICCBased color space must not be used
  for printing elements in PDF/X-1:2001 and PDF/X-1a:2001 files."

That is enough to write the 2003 syntax and colour groups as transcriptions
with a named source. It is **not** enough for X-4 and X-6, whose requirements
the notes predate: transparency permitted under restrictions, optional
content, JPEG 2000, `BX`/`EX`, architectural limits, the XMP identification
schema. Those groups are not written until the part is in hand.

**One lead worth following came from ISO 19005-2 rather than from any PDF/X
source.** Its normative references, read from the preview with `tpdf text`,
list *ISO 15930-7:2010 … (PDF/X-4)* as indispensable — the only PDF/X part any
ISO 19005 part cites. A file can claim PDF/A-2 and PDF/X-4 at once, and
`pdfa/colour.rs` already collects every PDF/A output intent's
`DestOutputProfile` and reports `OutputIntentsDisagree` when two differ; it
skips an intent whose `/S` is not `GTS_PDFA1` before collecting, with the
comment that judging it "would be enforcing a standard the file did not
claim". Whether 19005-2's rule about a file carrying more than one output
intent is stated over PDF/A intents only or over every entry that carries a
profile is a question the preview does not answer and no corpus fixture under
`PDF_A-2b/6.2.2` asks. The PDF/X group is where that gets read and settled,
because it is the group that will put a second intent into a document this
engine writes.

**So the adjudicator, stated plainly.** For the 2003 levels: the CGATS notes
as the rule source; the two published suites as the false-positive bar; 23
real producers' claims as the false-positive bar outside any suite; fixtures
built here, each with a near-miss twin, as the only thing that ever makes a
rule *fire* — and every one of those twins is this project's reading of a
secondary source, so a clause read wrongly is read wrongly in both directions
and the pair agrees with itself. That is the limit [design/pdfa.md](pdfa.md)
names for its writer, applied to a whole validator. For X-4 and X-6: nothing
yet, and the milestones below say so rather than pricing them.

## Design

**Where it lives.** `crates/tinker-pdf/src/pdfx/`, a sibling of `pdfa/`,
sharing its kernel rather than copying it: `Raw`, `Machinery` and its counted
reaches, `RuleGroup`, `Coverage`, the content-stream walk in `pdfa/content.rs`
and the font and colour visitors over it. The kernel moves to a module both
standards use; the design doc for PDF/UA asks for the same move for the same
reason, and whichever lands first does it. `FindingKind` stays one closed
enum: a missing `/Trapped` is a new variant, an unembedded font is the variant
that exists.

**Flavour comes from `/Info`, and that changes the cost.** `pdfa.rs` reads
the claim out of an XMP packet because ISO 19005 puts it there, and that one
XML parse is the reach the laziness counter records on nearly every corpus
file. ISO 15930 puts the claim in the document information dictionary, so a
PDF/X flavour is one trailer-reachable dictionary read with no parser behind
it. For X-4 and later the XMP identification schema must agree with the
`/Info` key, which is a rule of its own and the first use of the metadata
group here. A file claiming both PDF/A and PDF/X is validated by both engines,
and each reports under its own clause numbers; nothing is merged.

**Clause tables per part.** `pdfa.rs` keys each rule to three numbers —
part 1, parts 2 and 3, part 4 — because the same defect is numbered
differently by each part. ISO 15930 needs the same shape with more columns:
the 2001, 2003, 2010 and 2020 parts each renumber. Encryption is 6.17 in
15930-1, 6.11 in 15930-4, 6.15 in 15930-7 and 6.5.6 ("Permissions") in
15930-9; the numbers above are from the tables of contents, which is exactly
what a clause table needs and all it needs. A rule whose part is not yet in
hand has no row for that part and does not run under it — the table is what
says which flavours a rule has been transcribed for.

**Rule groups, by machinery.**

1. **Syntax**: the identification keys and their agreement; `/Trapped` as a
   name in {`True`, `False`}; `/MediaBox` present, exactly one of `/ArtBox`
   and `/TrimBox`, containment inside `/BleedBox` and `/CropBox` when present;
   no `/Encrypt`; filters (LZW forbidden everywhere; JBIG2 forbidden under the
   2003 levels); no PostScript XObject and no `PS` operator (the walk already
   sees every operator); `BX`/`EX`; annotations' `/Rect` against the boxes,
   with `PrinterMark` and `TrapNet` on their own rule; `/Info` private keys
   as text strings; actions and JavaScript; alternate images; viewer
   preferences. All of it is the COS document and the existing walk.
2. **Colour**: an output intent with `/S /GTS_PDFX`, the
   `OutputConditionIdentifier` / `RegistryName` / `DestOutputProfile`
   arrangement per the notes' Table 2; device spaces admitted against the
   intent's space, with the alternate-space rule for `Separation`, `DeviceN`,
   `Indexed` and `Pattern`; `ICCBased` forbidden under X-1a; the profile read
   by `tinker-pdf-color`, which [design/icc.md](icc.md) already delivers for
   PDF/A.
3. **Fonts**: embedding, through the visitor `pdfa/fonts.rs` already runs
   over the walk, re-pointed at 15930's clause numbers.
4. **Metadata**: the XMP identification schema for X-4 and later, staged
   until the part is read.

**Transparency is where the tokenizer stops being enough.** `pdfa/content.rs`
tracks the text rendering mode and the selected font and nothing else, by
design. The 2003 levels forbid transparency outright, which is a dictionary
question (`/Group`, `/SMask`, `/BM`, `/CA`, `/ca` — `ArchivalRefusal::
Transparency` already names the five) and the walk answers it. X-4 *permits*
transparency under restrictions its 6.20 states and this document has not
read; if those restrictions turn on state — which blend mode is in force at
which operator — the rule needs `tinker_pdf_content::interpret` and a
`Device`, and the honest placement is the same the PDF/UA design gives its
real-content group: the recording device, not a second walk.

**The writer flavour.** `ArchivalProfile` today is `{ part, level,
destination_profile, destination_space, output_condition, language }` and
every field is PDF/A's. The generalisation is a `Standard` the profile
carries — `PdfA { part, level }`, `PdfX(flavour)`, or both — with the shared
fields shared and three new ones: `trapped: bool`, the `/Info` identification
strings derived from the flavour rather than supplied, and the box the writer
puts on every page. `PageBuilder` has `set_crop_box` and `set_bleed_box` and
**no `set_trim_box` or `set_art_box`**; the flavour needs one of the two, and
the roadmap already lists trim and art boxes as neither read nor written on
the editor. Refusals, in `ArchivalRefusal`'s style: `DeviceRGB` and
`ICCBased` under X-1a, transparency under any 2003 level, a page with no trim
or art box, an annotation inside the trim box, `add_base_font` under any
flavour (the variant exists). Nothing is discovered at validation time that
the builder allowed.

**How the writing half is adjudicated, given ruling 13.** No third-party
program will ever say a file this writer produced conforms, and for PDF/X no
third-party *annotation* will either, because none exists for anybody's
files. What stands in has three legs, and the third is the one PDF/A does not
have:

- this build's own validator, with complete coverage, finds nothing;
- the strict structural validator in `tinker-pdf-cos`, which reads bytes and
  was written for the writer rather than for any standard, finds nothing;
- **every property the published conforming files state, this writer's
  output states the same way.** The Ghent and Altona files are conforming
  PDF/X documents made by the bodies that write the test suites, and their
  `/Info` keys, output intent dictionaries, box arrangements and `/Trapped`
  values are data. A test reads those properties out of a pinned suite file
  and out of a document this builder wrote under the same flavour, and asserts
  the shapes agree — the dictionary keys present, the names' spellings, the
  containment of boxes. That is a comparison against a third party's
  statement of what a conforming file looks like, and it is admissible where a
  verdict would not be.

None of the three says "it conforms", and the feature doc will say what
[features/pdfa.md](../features/pdfa.md) says: *this validator and the
structural one find nothing, and the shape matches the published files*.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Flavour detection, verdict types, `Document::validate_pdfx`, `tpdf check --pdfx`; the `pdfa` kernel shared rather than copied | Unit fixtures per identification form: each of the seven version strings, the 2001 conformance key, an empty string, a claim in XMP with none in `/Info`; a census over the fetched corpora prints `RAN` and the flavour of every one of the 23 files this document counted; `hostile_input.rs` calls the validator with zero panics; `Machinery`'s counters show a PDF/X flavour read costs no XML parse | S |
| 2 | The evidence base pinned: Ghent Output Suite 5.0 and Altona 1.2 Online in `corpora.lock`, licence recorded, `redistribute = no` | `cargo xtask corpus-fetch` verifies both by sha256; `pdfx_census.rs` opens every file, asserts each claims the flavour its suite says, prints `RAN`/`SKIPPED`. **Blocked** on establishing the Ghent package's licence terms, which neither page fetched states; a corpus with no recorded terms does not enter the lock | S, and a decision |
| 3 | Syntax group for the 2003 levels, transcribed from the CGATS notes with the note's section cited per rule | One fixture per rule with a near-miss twin, in `pdfx_syntax.rs`; **zero findings over every pinned suite file**, a hard assertion; zero findings over the 23 real claims or a ledger row per finding with a reason; counted injections with the zeros printed | M |
| 4 | Colour group for the 2003 levels: output intent shapes, device-space admission against the intent, alternate spaces, `ICCBased` under X-1a | Twins per rule; zero findings over the Altona pages (PDF/X-3) and the Ghent CMYK and SPOT patches; the 19005-2 multiple-intent question read and settled, with a fixture claiming both standards | M |
| 5 | Font group re-pointed at 15930's numbering | The embedding rule fires under a PDF/X claim with no PDF/A claim; zero on the suites; one injection: the clause table row removed, and the census names the fixture that stops being reported | S |
| 6 | Writer flavour: `Standard` on the profile, `set_trim_box`, `/Trapped`, the identification keys, the `GTS_PDFX` intent, refusals | Fixtures per flavour in `pdfx_writer.rs` judged three ways — this validator, the strict validator, and the shape comparison against a pinned suite file; a document claiming PDF/A-2b and PDF/X-4 together with zero findings from both engines; one refusal test per forbidden feature | M |
| 7 | X-4 and X-6 groups | **Not priced.** The requirement bodies are not in hand; the milestone opens when a part's text is, and its first exit criterion is the text read twice against the rules, which is what the JPX transcriptions needed | — |

Milestones 3 to 6 are what the roadmap row's "L" buys under the evidence
above. Milestone 7 is the part of the row that cannot be scheduled by this
document, and saying so is its job.

## Dependencies

- **`crates/tinker-pdf/src/pdfa/`** — the kernel, the content walk, the font
  and colour visitors, `Machinery`'s laziness counter. Exists; needs to be
  shared, not forked.
- **[design/icc.md](icc.md)** — profile parsing for the output intent's
  destination profile. Done through milestone 7 there; the staged case it
  records (a v4 profile with only an `mAB ` route leaves the intent's space
  unknown) applies here unchanged.
- **The corpus machinery** — `corpora.lock` already pins a `zip` archive with
  no upstream commit (SafeDocs), which is the shape both suites need.
- **`PageBuilder::set_trim_box` / `set_art_box`** — do not exist.
- **The recording device** (`crates/tinker-pdf-content/src/record.rs`) —
  only if X-4's transparency restrictions turn on graphics state; not needed
  for the 2003 levels.
- **The standards' text.** ISO 15930-4:2003 and 15930-6:2003 for the 2003
  groups' second reading; 15930-7:2010 and 15930-9:2020 before a line of
  milestone 7 is written. Purchase is a decision, not work, and ruling 13
  admits the text as data once it is here. The previews give the clause
  numbers and nothing below them.

## Risks

| Risk | Mitigation |
| --- | --- |
| **A rule that never fires passes every check this design has.** The suites are pass-only, the real claims are unannotated, and a fixture built here fires the rule its author expected | Named rather than closed. Every rule has a near-miss twin *and* a counted injection, and the census prints the count of rules that fired zero times over the suites — which for a pass-only suite should be every rule, so the number to watch is a rule that fires on a *suite* file, which is a false positive by definition |
| A rule transcribed from the application notes disagrees with the standard the notes defer to | The note's section is cited in the rule and in the finding's documentation; the ledger class is `secondary-source` rather than `reading`, so a later reading of the part knows which rows to revisit |
| The Ghent package's terms turn out to forbid fetching into a CI cache | Milestone 2 is blocked until the terms are read; Altona's terms are read and compatible with `redistribute = no`, so the PDF/X-3 half of the bar can land alone |
| Twenty-three real claims are too few to catch a rule that is too strict | Every `-pass-` PDF/A file in the veraPDF corpus is also a file with no PDF/X claim, and a PDF/X rule that fires on one of them is wrong twice; the census runs the PDF/X groups over all 2 907 and asserts silence on every file that claims nothing |
| The writer's shape comparison against a suite file is mistaken for a conformance verdict | The test's name and the feature doc say "shape", and the comparison is over dictionary keys and containment, never over a boolean the suite does not publish |
| X-4 rules get written from clause titles because the row says "L" and the 2003 groups landed | Milestone 7 has no size and one entry criterion, and `PDFX_STAGED` carries every X-4 and X-6 clause by name with "text not in hand" as the reason until it is |

## As built

*Filled in as milestones land.* Nothing has landed; the roadmap row is not
scheduled.
