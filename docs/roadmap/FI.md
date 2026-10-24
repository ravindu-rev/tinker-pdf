# Capability tier — formats in

[The roadmap](../ROADMAP.md) says how a step runs, what comes next, and which dependencies cross tiers. Each row below keeps its text as it was planned — Item, Today, Exit criterion, Doc, Depends on — and is cut into steps, each one workflow run.

Parity here means every input a widely deployed engine opens, and the
field's print-stream and archive formats besides. Each is ordered inside the
tier by corpus and producer evidence (ruling 3 as amended); none waits for
it.

## FI-01 — MOBI, PRC, PDB and AZW3/KF8

**MOBI, PRC, PDB and AZW3/KF8** (no DRM): PalmDOC, uncompressed and HUFF/CDIC records, the KF8 section, laid out by the XHTML path

**Today.** absent — the owner's parity decision of 9 October 2026; a decision row of the roadmap until 9 October 2026 ("MOBI; DOCX")

**Exit criterion.** books generated once by a producer (dated, bytes only, provenance recorded) open and draw as their XHTML twins pixel for pixel; HUFF/CDIC decoding held to the format's published description

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-01.1 | build | PDB containers and MOBI and EXTH headers read, with PalmDOC-compressed and uncompressed text records. | `crates/tinker-pdf/src/mobi.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/mobi.rs` (new) | `mobi.rs` (new): records built from the format description decode as it states; malformed records are typed errors. | — |
| FI-01.2 | build | HUFF/CDIC records decompressed. | `crates/tinker-pdf/src/mobi.rs` (new), `crates/tinker-pdf/tests/mobi.rs` (new) | Records built from the published description decode as it states. | FI-01.1 |
| FI-01.3 | build | MOBI 6 markup turned into XHTML (`filepos` anchors, image records) and laid out by the EPUB path; a book with DRM refused by name. | `crates/tinker-pdf/src/mobi.rs` (new), `crates/tinker-pdf/tests/mobi.rs` (new), `docs/features/epub.md` | Books generated once by a producer (dated, bytes only, provenance recorded) open and draw as their XHTML twins pixel for pixel. | FI-01.2, H-106 |
| FI-01.4 | build | KF8 (AZW3): the KF8 boundary, skeleton and fragment reassembly, fonts, CSS and images, and the NCX as the outline. | `crates/tinker-pdf/src/mobi.rs` (new), `crates/tinker-pdf/tests/mobi.rs` (new) | KF8 books generated once by a producer draw as their XHTML twins pixel for pixel; the outline matches the NCX. | FI-01.3 |
| FI-01.5 | build | MOBI, PRC, PDB and AZW3 sniffed and opened like the other formats, with a speed row and a fingerprint. | `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/benches/engine.rs`, `crates/tinker-pdf/tests/determinism.rs` | `Document::open` opens each kind; the speed row is in the benchmark set; a book's render is fingerprinted on every K-07 target. | FI-01.4, MS-04.8, K-07 |

**Closes the row:** FI-01.3 and FI-01.4 draw producer-made books as their XHTML twins pixel for pixel and FI-01.2 holds HUFF/CDIC to its description, which is the row's exit.

## FI-02a — Office documents as text and structure

**Office documents as text and structure**: DOCX, XLSX, PPTX, ODF and HWPX translated to XHTML (paragraphs, headings, lists, tables, images, links) and laid out by the EPUB path

**Today.** absent — the owner's parity decision of 9 October 2026; the decision row this replaces declined it because "a DOCX layout engine is a word processor" — which is why the office rows are split into milestones (FI-02b…f)

**Exit criterion.** generated documents (built in the test from the ECMA-376 and ODF schemas) open with the expected text, structure and images

**Doc:** — · **Depends on:** FI-21

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-02a.1 | build | DOCX to XHTML: paragraphs, headings by outline level, lists, tables, images through the package's relationships, and links, laid out by the EPUB path. | `crates/tinker-pdf/src/office/mod.rs` (new), `crates/tinker-pdf/src/office/docx.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/office_text.rs` (new) | `office_text.rs` (new): documents built in the test from ECMA-376's schemas open with the expected text, structure and images. | FI-21, H-102 |
| FI-02a.2 | build | XLSX to XHTML: each sheet a table of its cached values, shared and inline strings. | `crates/tinker-pdf/src/office/xlsx.rs` (new), `crates/tinker-pdf/tests/office_text.rs` (new) | Generated workbooks open with each sheet's values in place. | FI-02a.1 |
| FI-02a.3 | build | PPTX to XHTML: each slide a section with its text and images. | `crates/tinker-pdf/src/office/pptx.rs` (new), `crates/tinker-pdf/tests/office_text.rs` (new) | Generated presentations open with each slide's text and images in order. | FI-02a.1 |
| FI-02a.4 | build | ODF text, spreadsheet and presentation documents to XHTML. | `crates/tinker-pdf/src/office/odf.rs` (new), `crates/tinker-pdf/tests/office_text.rs` (new) | Documents built from the ODF schemas open with the expected text, structure and images. | FI-02a.1 |
| FI-02a.5 | build | HWPX to XHTML. | `crates/tinker-pdf/src/office/hwpx.rs` (new), `crates/tinker-pdf/tests/office_text.rs` (new) | Generated HWPX documents open with the expected text, structure and images. | FI-02a.1 |
| FI-02a.6 | build | Office packages sniffed by their content types or `mimetype`, retiring XPS's refusal of a `.docx`, with a feature doc of their own. | `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/src/xps.rs`, `docs/features/office.md` (new), `docs/README.md` | `Document::open` opens each format; a `.docx` no longer reaches the XPS refusal; the feature doc is indexed. | FI-02a.2, FI-02a.3, FI-02a.4, FI-02a.5 |

**Closes the row:** FI-02a.1 to FI-02a.5 open documents built from the ECMA-376 and ODF schemas with the expected text, structure and images, which is the row's exit.

## FI-02b — DOCX layout, milestone 1

**DOCX layout, milestone 1**: paragraphs, runs, styles and their inheritance, numbering, sections and page setup

**Today.** absent — the owner's parity decision of 9 October 2026; a `.docx` is recognised only to be refused as no XPS (`crates/tinker-pdf/src/xps.rs:47`)

**Exit criterion.** analytic fixtures built from ECMA-376's rules (box positions computed in the test); determinism on four targets; visual review of producer documents is H-61

**Doc:** `design/office-layout.md` (owed) · **Depends on:** FI-02a

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-02b.1 | design | Create `docs/design/office-layout.md` for FI-02b to FI-02f: why office layout is its own model and not CSS (line-spacing rules, tab stops, widow and keep rules, sections), where it lives beside the layout crate, analytic fixtures from ECMA-376's and ODF's rules, determinism, and each milestone. | `docs/design/office-layout.md` (new) | The doc exists in the design-doc order, names this row's exit tests and the milestones FI-02c to FI-02f extend, and is reviewed. | FI-02a, H-102 |
| FI-02b.2 | build | Styles and their inheritance resolved: document defaults, style chains and direct formatting of paragraphs and runs. | `crates/tinker-pdf/src/office/styles.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | `office_layout.rs` (new): resolved properties equal ECMA-376's rules applied in the test. | FI-02b.1 |
| FI-02b.3 | build | Runs and paragraphs laid out: fonts and sizes, spacing, line-spacing rules, indentation, alignment and tab stops. | `crates/tinker-pdf/src/office/layout.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Analytic fixtures place every box where the test computes from ECMA-376's rules. | FI-02b.2 |
| FI-02b.4 | build | Numbering: abstract and concrete numbering, levels, level text and restarts. | `crates/tinker-pdf/src/office/numbering.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Analytic fixtures' list labels and positions equal the test's computation. | FI-02b.3 |
| FI-02b.5 | build | Sections and page setup: page size, margins, orientation, columns and section breaks, with widow and orphan control and the keep and page-break rules. | `crates/tinker-pdf/src/office/layout.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Analytic fixtures paginate where the test computes. | FI-02b.3 |
| FI-02b.6 | test | Determinism on the four targets. *Note:* The visual review of producer documents is H-61's, an input a person reads and never a check; it is recorded when it is made, outside this exit. | `crates/tinker-pdf/tests/determinism.rs`, `docs/features/office.md` (new) | A laid-out document's render is fingerprinted on every K-07 target. | FI-02b.4, FI-02b.5, K-07 |

**Closes the row:** FI-02b.2 to FI-02b.5 place analytic fixtures' boxes as ECMA-376's rules compute, and FI-02b.6 fingerprints them on four targets, which is the row's exit; the visual review of producer documents is H-61's, outside this exit.

## FI-02c — DOCX layout, milestone 2

**DOCX layout, milestone 2**: tables (grid, spans, borders, headers repeated), headers and footers, fields, footnotes and endnotes

**Today.** absent — the owner's parity decision of 9 October 2026; as FI-02b

**Exit criterion.** as FI-02b

**Doc:** `design/office-layout.md` (owed) · **Depends on:** FI-02b

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-02c.1 | design | Add milestone 2 to `docs/design/office-layout.md`: tables, headers and footers, fields, footnotes and endnotes. | `docs/design/office-layout.md` | The section names FI-02c.2 to FI-02c.6's exit tests and is reviewed. | FI-02b.1 |
| FI-02c.2 | build | Tables: the grid, cell spans and vertical merges, border conflicts, cell margins, row heights and repeated header rows. | `crates/tinker-pdf/src/office/tables.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Analytic table fixtures place every cell and border where the test computes. | FI-02c.1, FI-02b |
| FI-02c.3 | build | Headers and footers: default, first and even per section. | `crates/tinker-pdf/src/office/layout.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Analytic fixtures place each variant on the pages the test computes. | FI-02c.2 |
| FI-02c.4 | build | Fields, simple and complex: page numbers and counts, references, hyperlinks and tables of contents, computed or taken from their cached results as ECMA-376 states. | `crates/tinker-pdf/src/office/fields.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Each field kind's result equals the test's computation. | FI-02c.3 |
| FI-02c.5 | build | Footnotes and endnotes: placement, separators and numbering formats. | `crates/tinker-pdf/src/office/notes.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Analytic fixtures place each note where the test computes. | FI-02c.3 |
| FI-02c.6 | test | Determinism on the four targets for milestone 2's fixtures. | `crates/tinker-pdf/tests/determinism.rs` | A table-and-notes document's render is fingerprinted on every K-07 target. | FI-02c.4, FI-02c.5, K-07 |

**Closes the row:** FI-02c.2 to FI-02c.5 place analytic fixtures as computed and FI-02c.6 holds them on four targets, as FI-02b's exit states.

## FI-02d — DOCX layout, milestone 3

**DOCX layout, milestone 3**: floating and anchored objects, text wrapping, DrawingML shapes, charts drawn from their cached data

**Today.** absent — the owner's parity decision of 9 October 2026; as FI-02b

**Exit criterion.** as FI-02b

**Doc:** `design/office-layout.md` (owed) · **Depends on:** FI-02c, FI-36

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-02d.1 | design | Add milestone 3 to `docs/design/office-layout.md`: anchored objects, wrapping, DrawingML preset geometry, and charts from cached data. | `docs/design/office-layout.md` | The section names FI-02d.2 to FI-02d.7's exit tests and is reviewed. | FI-02b.1 |
| FI-02d.2 | build | Inline and anchored drawings positioned relative to page, margin, paragraph or character. | `crates/tinker-pdf/src/office/drawing.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Analytic fixtures place each object where the test computes. | FI-02d.1, FI-02c |
| FI-02d.3 | build | Text wrapping around anchored objects: square, tight, through, top and bottom, behind and in front. | `crates/tinker-pdf/src/office/layout.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Analytic fixtures' line boxes avoid each object as the test computes for each mode. | FI-02d.2 |
| FI-02d.4 | build | DrawingML preset geometries evaluated from their guide formulas, with fills, lines and text in shapes. | `crates/tinker-pdf/src/office/geometry.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Each preset's path equals its formulas evaluated in the test. | FI-02d.1 |
| FI-02d.5 | build | Charts drawn from their cached data: bar, line, pie, area and scatter, with axes and legends. | `crates/tinker-pdf/src/office/charts.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Each chart's geometry equals the test's computation from the cached values. | FI-02d.1 |
| FI-02d.6 | build | EMF and WMF pictures drawn through FI-36. | `crates/tinker-pdf/src/office/drawing.rs` (new), `crates/tinker-pdf/tests/office_layout.rs` (new) | Documents holding metafile pictures draw them as FI-36 renders them alone. | FI-02d.2, FI-36 |
| FI-02d.7 | test | Determinism on the four targets for milestone 3's fixtures. | `crates/tinker-pdf/tests/determinism.rs` | A document with anchored shapes and a chart is fingerprinted on every K-07 target. | FI-02d.3, FI-02d.4, FI-02d.5, FI-02d.6, K-07 |

**Closes the row:** FI-02d.2 to FI-02d.6 place analytic fixtures as computed and FI-02d.7 holds them on four targets, as FI-02b's exit states.

## FI-02e — Spreadsheet layout

**Spreadsheet layout** (XLSX, ODS): cells, number formats, merged cells, print areas, page breaks, charts from cached data

**Today.** absent — the owner's parity decision of 9 October 2026; no spreadsheet is read; `rg -i 'spreadsheetml' crates tools` finds only an associated file's media type in a test (`crates/tinker-pdf/tests/pdf20.rs:345`)

**Exit criterion.** as FI-02b, against ECMA-376 SpreadsheetML and ODF

**Doc:** `design/office-layout.md` (owed) · **Depends on:** FI-02a

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-02e.1 | design | Add spreadsheet layout to `docs/design/office-layout.md`: cell geometry, number formats, merges, print areas and breaks, and charts. | `docs/design/office-layout.md` | The section names FI-02e.2 to FI-02e.7's exit tests and is reviewed. | FI-02b.1 |
| FI-02e.2 | build | Cells laid out: column widths from character units, row heights, fonts, alignment, borders and fills. | `crates/tinker-pdf/src/office/sheets.rs` (new), `crates/tinker-pdf/tests/office_sheets.rs` (new) | `office_sheets.rs` (new): analytic fixtures place each cell where the test computes from ECMA-376's rules. | FI-02e.1, FI-02a |
| FI-02e.3 | build | Number formats: sections, dates and times, fractions, scientific and conditional codes. | `crates/tinker-pdf/src/office/numfmt.rs` (new), `crates/tinker-pdf/tests/office_sheets.rs` (new) | Each format code formats the specification's examples as stated. | FI-02e.2 |
| FI-02e.4 | build | Merged cells, print areas, manual and automatic page breaks, scaling to fit and repeated print titles. | `crates/tinker-pdf/src/office/sheets.rs` (new), `crates/tinker-pdf/tests/office_sheets.rs` (new) | Analytic fixtures paginate and place merges where the test computes. | FI-02e.2 |
| FI-02e.5 | build | Spreadsheet charts drawn through FI-02d.5's chart drawing. | `crates/tinker-pdf/src/office/sheets.rs` (new), `crates/tinker-pdf/tests/office_sheets.rs` (new) | A sheet's chart geometry equals the computation from its cached values. | FI-02e.2, FI-02d.5 |
| FI-02e.6 | build | ODF spreadsheets onto the same layout. | `crates/tinker-pdf/src/office/odf.rs` (new), `crates/tinker-pdf/tests/office_sheets.rs` (new) | Analytic ODS fixtures place as their XLSX twins. | FI-02e.4 |
| FI-02e.7 | test | Determinism on the four targets for the spreadsheet fixtures. | `crates/tinker-pdf/tests/determinism.rs` | A laid-out sheet is fingerprinted on every K-07 target. | FI-02e.3, FI-02e.5, FI-02e.6, K-07 |

**Closes the row:** FI-02e.2 to FI-02e.6 place analytic fixtures as SpreadsheetML's and ODF's rules compute and FI-02e.7 holds them on four targets, as FI-02b's exit states.

## FI-02f — Presentation layout

**Presentation layout** (PPTX, ODP): slides, layouts and masters, text boxes, shapes, images

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'pptx|presentationml|\bodp\b' crates tools` finds nothing

**Exit criterion.** as FI-02b, against PresentationML and ODF

**Doc:** `design/office-layout.md` (owed) · **Depends on:** FI-02a, FI-36

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-02f.1 | design | Add presentation layout to `docs/design/office-layout.md`: slide size, master, layout and slide inheritance, text boxes and autofit, shapes and images. | `docs/design/office-layout.md` | The section names FI-02f.2 to FI-02f.6's exit tests and is reviewed. | FI-02b.1 |
| FI-02f.2 | build | Slides: size, and placeholders and text styles inherited from master and layout. | `crates/tinker-pdf/src/office/slides.rs` (new), `crates/tinker-pdf/tests/office_slides.rs` (new) | `office_slides.rs` (new): analytic fixtures resolve each placeholder's position and style as the test computes. | FI-02f.1, FI-02a |
| FI-02f.3 | build | Text boxes: insets, autofit scaling, bullets and paragraphs. | `crates/tinker-pdf/src/office/slides.rs` (new), `crates/tinker-pdf/tests/office_slides.rs` (new) | Analytic fixtures place every line where the test computes. | FI-02f.2 |
| FI-02f.4 | build | Shapes through FI-02d.4's preset geometry, images with cropping, and metafiles through FI-36. | `crates/tinker-pdf/src/office/slides.rs` (new), `crates/tinker-pdf/tests/office_slides.rs` (new) | Each shape's path and image's placement equal the test's computation. | FI-02f.2, FI-02d.4, FI-36 |
| FI-02f.5 | build | ODF presentations onto the same layout. | `crates/tinker-pdf/src/office/odf.rs` (new), `crates/tinker-pdf/tests/office_slides.rs` (new) | Analytic ODP fixtures place as their PPTX twins. | FI-02f.3, FI-02f.4 |
| FI-02f.6 | test | Determinism on the four targets for the presentation fixtures. | `crates/tinker-pdf/tests/determinism.rs` | A laid-out slide is fingerprinted on every K-07 target. | FI-02f.5, K-07 |

**Closes the row:** FI-02f.2 to FI-02f.5 place analytic fixtures as PresentationML's and ODF's rules compute and FI-02f.6 holds them on four targets, as FI-02b's exit states.

## FI-02g — RTF

**RTF** read and laid out

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\brtf\b' crates tools` finds nothing

**Exit criterion.** generated documents per control-word family

**Doc:** — · **Depends on:** FI-02a, FI-36

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-02g.1 | build | RTF read: groups and destinations, code pages and `\u` escapes with `\uc`, and the font, colour, style and list tables. | `crates/tinker-pdf/src/office/rtf.rs` (new), `crates/tinker-pdf/tests/rtf.rs` (new) | `rtf.rs` (new): documents generated per control-word family parse to the expected properties. | FI-02a, H-102 |
| FI-02g.2 | build | RTF laid out through FI-02a's XHTML path: paragraphs and runs, tables, lists, fields, and pictures (metafiles through FI-36, PNG and JPEG). | `crates/tinker-pdf/src/office/rtf.rs` (new), `crates/tinker-pdf/tests/rtf.rs` (new) | Generated documents per control-word family render with the expected text, structure and pictures. | FI-02g.1, FI-36 |
| FI-02g.3 | build | RTF sniffed and opened, and documented. | `crates/tinker-pdf/src/standalone.rs`, `docs/features/office.md` (new) | `Document::open` opens RTF; the feature doc states what is read. | FI-02g.2 |

**Closes the row:** FI-02g.1 and FI-02g.2 hold generated documents per control-word family, which is the row's exit.

## FI-03 — Plain text documents

**Plain text documents** (`.txt`, `.log`) with encoding detection and box-drawing characters

**Today.** absent — the owner's parity decision of 9 October 2026; no opener takes plain text (`rg -i 'open_text|plain_text_document' crates` finds nothing; `plain_text` is extraction's output)

**Exit criterion.** fixtures in each detectable encoding open to their text, monospaced, one page per form feed

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-03.1 | build | Plain text opened as a document: encoding detected from byte-order marks, UTF-8 validity and a single-byte fallback; set monospaced, one page per form feed, box-drawing characters drawn. | `crates/tinker-pdf/src/plain_text.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/plain_text_documents.rs` (new), `docs/features/opening.md` | `plain_text_documents.rs` (new): fixtures in each detectable encoding open to their text, monospaced, one page per form feed, in both builds. | — |
| FI-03.2 | build | Plain text sniffed and opened by `Document::open`, and a fingerprint of one page. | `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/tests/determinism.rs` | `Document::open` opens a `.txt` fixture; the fingerprint holds on every K-07 target. | FI-03.1, K-07 |

**Closes the row:** FI-03.1 opens each encoding's fixture to its text, monospaced, a page per form feed, which is the row's exit.

## FI-04 — Gzip-wrapped documents

**Gzip-wrapped documents** of any supported type

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'gzip|svgz' crates tools` finds nothing

**Exit criterion.** a gzipped copy of every committed fixture opens as the original

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-04.1 | build | Gzip-wrapped documents unwrapped by FT-06.7's member reader (RFC 1952), member after member, and the inner bytes opened as whatever they are, SVGZ included. | `crates/tinker-pdf-filters/src/gzip.rs` (new), `crates/tinker-pdf-filters/src/lib.rs`, `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/tests/gzip_documents.rs` (new), `docs/features/opening.md` | `gzip_documents.rs` (new): a gzipped copy of every committed fixture, made in the test by the in-tree deflate, opens as the original; a bad CRC is a typed error. | FT-06.7 |

**Closes the row:** FI-04.1 opens a gzipped copy of every committed fixture as the original, which is the row's exit.

## FI-05 — Netpbm (PBM, PGM, PPM, PAM, PFM, and PKM, the 1-bit CMYK variant) and PSD composite images

**Netpbm (PBM, PGM, PPM, PAM, PFM, and PKM, the 1-bit CMYK variant) and PSD composite images** as documents and inside containers

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -iw 'netpbm|pgm|pam|psd' crates/tinker-pdf-filters/src` finds nothing (its `PPM` is JPEG 2000's packed-packet marker)

**Exit criterion.** Netpbm held to its format description with fixtures generated in the test; PSD's composite image held to fixtures generated once (dated, bytes only)

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-05.1 | build | Netpbm decoded: PBM, PGM and PPM in plain and raw forms, PAM with its tuple types, PFM, and PKM's 1-bit CMYK. | `crates/tinker-pdf-filters/src/netpbm.rs` (new), `crates/tinker-pdf-filters/src/lib.rs` | Unit tests: fixtures generated in the test from the format description decode to their samples. | — |
| FI-05.2 | build | PSD's composite image decoded: raw and PackBits data in its colour modes and depths. | `crates/tinker-pdf-filters/src/psd.rs` (new), `crates/tinker-pdf-filters/src/lib.rs` | Unit tests: fixtures generated once (dated, bytes only) decode to their known samples. | H-106 |
| FI-05.3 | build | Netpbm and PSD opened as documents and recognised inside archives and books. | `crates/tinker-pdf/src/cbz.rs`, `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/tests/netpbm_psd.rs` (new), `docs/features/filters.md` | `netpbm_psd.rs` (new): each opens by `Document::open` and as a CBZ page. | FI-05.1, FI-05.2 |

**Closes the row:** FI-05.1 and FI-05.2 hold Netpbm and PSD to their descriptions and fixtures, and FI-05.3 opens them, which is the row's exit.

## FI-06 — Multi-image inputs

**Multi-image inputs**: multi-image Netpbm, BMP sub-images, stand-alone JBIG2 files, every frame of animated GIF and WebP as pages

**Today.** GIF and WebP open at their first frame (`features/filters.md:336-700`)

**Exit criterion.** each frame equals its frame decoded in the test; frame counts match the containers

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-06.1 | build | Every frame of an animated GIF and WebP as a page, composited with each frame's disposal and blending. | `crates/tinker-pdf-filters/src/gif.rs`, `crates/tinker-pdf-filters/src/webp.rs`, `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/tests/multi_image.rs` (new), `docs/features/filters.md` | `multi_image.rs` (new): each page equals its frame composited in the test; page counts match the containers' frame counts. | — |
| FI-06.2 | build | Multi-image Netpbm, BMP sub-images and stand-alone JBIG2 files as pages. | `crates/tinker-pdf-filters/src/netpbm.rs` (new), `crates/tinker-pdf-filters/src/bmp.rs`, `crates/tinker-pdf-filters/src/jbig2.rs`, `crates/tinker-pdf/tests/multi_image.rs` (new) | Each page equals its image decoded in the test; counts match the files'. | FI-06.1, FI-05 |

**Closes the row:** FI-06.1 and FI-06.2 make each frame or image a page equal to its decode, with matching counts, which is the row's exit.

## FI-07a — JPEG XL, milestone 1

**JPEG XL, milestone 1**: the container and the modular mode (lossless)

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'jxl|jpeg.?xl' crates tools` finds nothing

**Exit criterion.** the published conformance bitstreams for modular mode decode to their reference samples

**Doc:** `design/jpeg-xl.md` (owed) · **Depends on:** H-47

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-07a.1 | design | Create `docs/design/jpeg-xl.md` for FI-07a, FI-07b and the encoder FO-07c: the container and codestream, entropy coding, modular mode and its transforms, VarDCT and its filters, JPEG reconstruction, and the conformance data the decoder is held to. | `docs/design/jpeg-xl.md` (new) | The doc exists in the design-doc order, names this row's exit tests and the milestones FI-07b and FO-07c extend, and is reviewed. | H-47, H-102 |
| FI-07a.2 | build | The container and codestream headers: size, image metadata, colour encoding, frame headers and the table of contents. | `crates/tinker-pdf-filters/src/jxl/mod.rs` (new), `crates/tinker-pdf-filters/src/jxl/headers.rs` (new), `crates/tinker-pdf-filters/src/lib.rs` | Unit tests: the conformance bitstreams' headers parse to their stated values. | FI-07a.1 |
| FI-07a.3 | build | Entropy decoding: ANS and prefix codes, hybrid integer configurations, LZ77 and clustered histograms. | `crates/tinker-pdf-filters/src/jxl/entropy.rs` (new) | Unit tests hold each part to the specification's definitions; the conformance streams' entropy-coded sections decode. | FI-07a.2 |
| FI-07a.4 | build | Modular mode: meta-adaptive trees decoded and evaluated, the predictors including the weighted one, and channel decoding. | `crates/tinker-pdf-filters/src/jxl/modular.rs` (new) | Modular conformance bitstreams without transforms decode to their reference samples. | FI-07a.3 |
| FI-07a.5 | build | Modular transforms: RCT, palette with deltas, and squeeze. | `crates/tinker-pdf-filters/src/jxl/modular.rs` (new) | Conformance bitstreams with transforms decode to their reference samples. | FI-07a.4 |
| FI-07a.6 | build | Frames composed: blending, cropping and extra channels such as alpha. | `crates/tinker-pdf-filters/src/jxl/frame.rs` (new) | Every published conformance bitstream for modular mode decodes to its reference samples. | FI-07a.5 |
| FI-07a.7 | build | JPEG XL opened as a bare image and recognised in archives, books and HTML, with a fuzz target. | `crates/tinker-pdf/src/cbz.rs`, `crates/tinker-pdf/src/standalone.rs`, `fuzz/fuzz_targets/jxl.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md`, `docs/features/filters.md` | A modular file opens by `Document::open`; the fuzz target runs clean with the roster regenerated. | FI-07a.6 |

**Closes the row:** FI-07a.4 to FI-07a.6 decode the published modular-mode conformance bitstreams to their reference samples, which is the row's exit.

## FI-07b — JPEG XL, milestone 2

**JPEG XL, milestone 2**: VarDCT and lossless JPEG reconstruction

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'jxl|jpeg.?xl' crates tools` finds nothing

**Exit criterion.** the remaining conformance bitstreams; reconstructed JPEGs byte-equal to their originals

**Doc:** `design/jpeg-xl.md` (owed) · **Depends on:** FI-07a

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-07b.1 | design | Add milestone 2 to `docs/design/jpeg-xl.md`: VarDCT, its colour transform and filters, patches, splines and noise, and JPEG reconstruction. | `docs/design/jpeg-xl.md` | The section names FI-07b.2 to FI-07b.7's exit tests and is reviewed. | FI-07a.1 |
| FI-07b.2 | build | VarDCT coefficients: quantisation, every transform size and shape, and high-frequency coefficient decoding with its context model. | `crates/tinker-pdf-filters/src/jxl/vardct.rs` (new) | Unit tests hold each transform to its definition; VarDCT streams' coefficients decode. | FI-07b.1, FI-07a |
| FI-07b.3 | build | The low-frequency image, chroma from luma, adaptive quantisation and the XYB transform. | `crates/tinker-pdf-filters/src/jxl/vardct.rs` (new) | VarDCT streams without filters decode to their reference samples. | FI-07b.2 |
| FI-07b.4 | build | Restoration filters: Gaborish and the edge-preserving filter. | `crates/tinker-pdf-filters/src/jxl/filters.rs` (new) | Streams using each filter decode to their reference samples. | FI-07b.3 |
| FI-07b.5 | build | Patches, splines and noise synthesis. | `crates/tinker-pdf-filters/src/jxl/frame.rs` (new) | Streams using each feature decode to their reference samples. | FI-07b.3 |
| FI-07b.6 | build | Lossless JPEG reconstruction from the reconstruction data. | `crates/tinker-pdf-filters/src/jxl/jpeg_recon.rs` (new) | Reconstructed JPEGs are byte-equal to their originals. | FI-07b.2 |
| FI-07b.7 | test | Close the milestone: every remaining conformance bitstream, a decode fingerprint and the decoder's speed row. | `crates/tinker-pdf-filters/tests/jxl_conformance.rs` (new), `crates/tinker-pdf/tests/determinism.rs`, `crates/tinker-pdf/benches/engine.rs` | Every published conformance bitstream decodes to its reference; the fingerprint holds on every K-07 target; the speed row is in the benchmark set. | FI-07b.4, FI-07b.5, FI-07b.6, K-07, MS-04.8 |

**Closes the row:** FI-07b.7 decodes the remaining conformance bitstreams and FI-07b.6 reconstructs JPEGs byte-equal, which is the row's exit.

## FI-07c — JPEG XL as a PDF image filter, and HDR content

**JPEG XL as a PDF image filter, and HDR content**: the filter as its published PDF definition states it, and PQ and HLG transfer at high bit depth carried through rendering on the 16-bit pipeline and tone-mapped to the output space

**Today.** absent — the owner's parity decision of 9 October 2026; the filter's definition for PDF is not yet published (H-122)

**Exit criterion.** fixtures built from the published definition decode to FI-07a/b's decode of the same codestream as a bare file; the PQ and HLG transfers recomputed in the test from ITU-R BT.2100's equations

**Doc:** — · **Depends on:** FI-07b, RC-07, H-122

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-07c.1 | build | JPEG XL as a PDF image filter, by its published definition: name, parameters and colour handling. | `crates/tinker-pdf-filters/src/lib.rs`, `crates/tinker-pdf/src/images.rs`, `crates/tinker-pdf/tests/jxl_filter.rs` (new), `docs/features/filters.md` | `jxl_filter.rs` (new): fixtures built from the published definition decode to FI-07a and FI-07b's decode of the same codestream as a bare file. | FI-07b, H-122 |
| FI-07c.2 | build | HDR content: PQ and HLG transfer at high bit depth carried through RC-07's 16-bit pipeline and tone-mapped to the output space. | `crates/tinker-pdf-color/src/lib.rs`, `crates/tinker-pdf/tests/jxl_filter.rs` (new) | The PQ and HLG transfers equal BT.2100's equations recomputed in the test; tone mapping follows the stated operator. | FI-07c.1, RC-07.15 |

**Closes the row:** FI-07c.1 decodes filter fixtures as the bare codestream and FI-07c.2 holds PQ and HLG to BT.2100's equations, which is the row's exit.

## FI-08 — AVIF

**AVIF** (AV1 intra decoding) in archives, as a bare image and in EPUB and HTML

**Today.** AVIF needs a codec: `PageDefect::UnsupportedFormat(Avif)` (`features/cbz.md:431`; `features/opening.md:337`), priced L by the archives row this replaces. Every CBZ fixture is in the tree, so there is no corpus count. Everything else that row named has left it — Zstandard as ZIP method 93 (a hand-rolled RFC 8878 decoder held to Appendix A and to the format project's golden files), 7z's BCJ2 and BCJ filters and PPMd coder, bzip2 as ZIP method 12 and 7z coder `040202`, ZIP method 14, JPEG 2000 pages, BMP and GIF, and WebP lossless and lossy (the lossy planes held to the WebM project's published VP8 test vectors, fetched pinned by commit and SHA-256 and run by CI's `vp8-vectors` job) — each recorded with its tests in [features/cbz.md](../features/cbz.md) and [features/filters.md](../features/filters.md); what those decoders still refuse by name is FI-25's

**Exit criterion.** the AV1 conformance streams for intra-only profiles decode to their reference outputs; re-priced in the design doc

**Doc:** `design/avif.md` (owed) · **Depends on:** H-47

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-08.1 | design | Create `docs/design/avif.md` for FI-08 and the encoder FO-07b: the ISOBMFF container (shared with HEIF), AV1 intra decoding with its prediction, transforms and filters, the conformance streams, and the row's price stated again. | `docs/design/avif.md` (new) | The doc exists in the design-doc order, names this row's exit tests, restates the price, and is reviewed. | H-47 |
| FI-08.2 | build | An ISOBMFF box reader and the AVIF items: properties, grids and alpha auxiliary images. | `crates/tinker-pdf-filters/src/isobmff.rs` (new), `crates/tinker-pdf-filters/src/avif/mod.rs` (new), `crates/tinker-pdf-filters/src/lib.rs` | Unit tests: generated containers parse to their items and properties. | FI-08.1 |
| FI-08.3 | build | AV1's OBUs, sequence and frame headers for intra frames, and the symbol decoder with adapting CDFs. | `crates/tinker-pdf-filters/src/avif/obu.rs` (new), `crates/tinker-pdf-filters/src/avif/symbol.rs` (new) | The conformance streams' headers parse; the symbol decoder holds to unit tests from the specification. | FI-08.2 |
| FI-08.4 | build | Block partitioning and intra prediction: directional, smooth and Paeth modes, chroma from luma, palette and intra block copy. | `crates/tinker-pdf-filters/src/avif/intra.rs` (new) | Unit tests hold each predictor to the specification's equations. | FI-08.3 |
| FI-08.5 | build | Coefficient decoding, dequantisation and every inverse transform size and type. | `crates/tinker-pdf-filters/src/avif/transform.rs` (new) | Unit tests hold each transform to its definition; conformance streams decode to their pre-filter reconstruction. | FI-08.4 |
| FI-08.6 | build | The loop filter, CDEF, loop restoration, super-resolution and film grain. | `crates/tinker-pdf-filters/src/avif/postfilter.rs` (new) | The AV1 conformance streams for the intra-only profiles decode to their reference outputs. | FI-08.5 |
| FI-08.7 | build | AVIF opened as a bare image, in archives (retiring `UnsupportedFormat(Avif)`), and in books and HTML, with a fuzz target. | `crates/tinker-pdf/src/cbz.rs`, `crates/tinker-pdf/src/standalone.rs`, `fuzz/fuzz_targets/avif.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md`, `docs/features/cbz.md` | An AVIF opens everywhere an image does; the fuzz target runs clean with the roster regenerated; failing first on today's refusal. | FI-08.6 |

**Closes the row:** FI-08.6 decodes the intra-only AV1 conformance streams to their references, re-priced in FI-08.1, which is the row's exit.

## FI-09 — HEIC/HEIF

**HEIC/HEIF** (HEVC intra)

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'heic|heif|hevc' crates tools` finds nothing

**Exit criterion.** HEVC conformance streams for the still-picture profile

**Doc:** `design/heif.md` (owed) · **Depends on:** H-46 (patents)

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-09.1 | design | Create `docs/design/heif.md`: the HEIF container through FI-08.2's box reader, HEVC still-picture decoding (parameter sets, CABAC, intra prediction, transforms, deblocking and SAO), the conformance streams, and the patent question H-46 answers. | `docs/design/heif.md` (new) | The doc exists in the design-doc order, names FI-09.2 to FI-09.6's exit tests, and is reviewed. | H-46 |
| FI-09.2 | build | HEIF items through FI-08.2's box reader: HEVC configuration, grids, alpha and thumbnails. | `crates/tinker-pdf-filters/src/heif/mod.rs` (new) | Unit tests: generated containers parse to their items. | FI-09.1, FI-08.2 |
| FI-09.3 | build | HEVC NAL units, parameter sets, slice headers, and the CABAC engine with its context models. | `crates/tinker-pdf-filters/src/heif/hevc.rs` (new), `crates/tinker-pdf-filters/src/heif/cabac.rs` (new) | Unit tests hold CABAC to the specification; conformance streams' headers parse. | FI-09.2 |
| FI-09.4 | build | Coding trees, intra prediction, transforms and dequantisation. | `crates/tinker-pdf-filters/src/heif/hevc.rs` (new) | Conformance streams decode to their pre-filter reconstruction. | FI-09.3 |
| FI-09.5 | build | Deblocking and sample-adaptive offset. | `crates/tinker-pdf-filters/src/heif/hevc.rs` (new) | The HEVC conformance streams for the still-picture profile decode to their references. | FI-09.4 |
| FI-09.6 | build | HEIC opened as a bare image and in archives, with a fuzz target. | `crates/tinker-pdf/src/cbz.rs`, `crates/tinker-pdf/src/standalone.rs`, `fuzz/fuzz_targets/heif.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md` | A HEIC opens by `Document::open` and as a CBZ page; the fuzz target runs clean with the roster regenerated. | FI-09.5 |

**Closes the row:** FI-09.5 decodes the still-picture conformance streams to their references, which is the row's exit.

## FI-10 — JPEG lossless and hierarchical processes

**JPEG lossless and hierarchical processes** (SOF3, SOF7, SOF11, SOF15; SOF5, SOF6, SOF13, SOF14)

**Today.** `JpegError::Lossless`, `JpegError::Differential` (`features/filters.md:802-803`); zero in the corpus; untracked

**Exit criterion.** decoded samples equal T.81 Annex H's predictors and Annex J's differential reconstruction recomputed in the test over streams built from the annexes' tables

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-10.1 | build | The Huffman lossless process (SOF3): Annex H's predictors and point transform, retiring `JpegError::Lossless` for it. | `crates/tinker-pdf-filters/src/jpeg.rs`, `crates/tinker-pdf-filters/src/jpeg/lossless.rs` (new) | Unit tests: decoded samples equal Annex H's predictors recomputed in the test over streams built from the annex's tables; failing first on today's refusal. | — |
| FI-10.2 | build | The arithmetic lossless process (SOF11) and the differential lossless frames (SOF7, SOF15). | `crates/tinker-pdf-filters/src/jpeg/lossless.rs` (new) | Unit tests: decoded samples equal the annexes' computation. | FI-10.1 |
| FI-10.3 | build | The hierarchical DCT processes (SOF5, SOF6, SOF13, SOF14): Annex J's differential reconstruction, retiring `JpegError::Differential`. | `crates/tinker-pdf-filters/src/jpeg/hierarchical.rs` (new), `docs/features/filters.md` | Unit tests: decoded samples equal Annex J's reconstruction recomputed in the test; failing first on today's refusal. | FI-10.2 |

**Closes the row:** FI-10.1 to FI-10.3 decode every lossless and hierarchical process to the annexes' recomputed samples, which is the row's exit.

## FI-11 — Image-container refusals

**Image-container refusals**: TIFF `Compression` 6 (old-style JPEG), SGI LogLuv and ThunderScan, `PhotometricInterpretation` 6 outside compression 7, CIELab at other depths and sample formats, `InkSet` other than 1; BMP `BI_JPEG`, `BI_PNG`, the `BI_CMYK` family and OS/2 2.x Huffman and RLE24

**Today.** each a typed `TiffError`/`BmpError` (`features/filters.md:807-817`). TIFF is reached from XPS and CBZ. `Compression` 6, which Technical Note 2 replaced: a file whose `JPEGInterchangeFormat` (tag 513) points at a complete interchange-format stream is readable by the existing JPEG decoder without a guess; one that carries only TIFF 6.0 §22's per-component tags is not; no count recorded. Compression 6 was once moved to the permanent refusals with masks and colour-filter arrays and was brought back on review (2 October 2026): a permanent refusal with no count recorded was a scheduling decision ruling 3 does not allow. `PhotometricInterpretation` 8 (CIELab) at 8 and 16 bits, CMYK (5), `SampleFormat` 2 and 3, `Predictor` 3, BigTIFF, `Compression` 34712 and directories after the first have left ([features/filters.md](../features/filters.md)); 4 (a mask) and 32803 (a CFA) are permanent (NG-10)

**Exit criterion.** per refusal, a fixture built from the TIFF 6.0 or BMP description (or generated once, dated) decodes to its known samples

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-11.1 | build | TIFF `Compression` 6 whose `JPEGInterchangeFormat` points at a complete interchange-format stream, through the JPEG decoder; the §22 per-component-tag form refused by name. | `crates/tinker-pdf-filters/src/tiff.rs` | Unit tests: a fixture built from TIFF 6.0's description decodes to its known samples; the other form is its named refusal. | — |
| FI-11.2 | build | SGI LogLuv and ThunderScan. | `crates/tinker-pdf-filters/src/tiff.rs` | Unit tests: fixtures built from their descriptions decode to their known samples. | — |
| FI-11.3 | build | `PhotometricInterpretation` 6 outside compression 7, CIELab at other depths and sample formats, and `InkSet` other than 1. | `crates/tinker-pdf-filters/src/tiff.rs` | Unit tests: fixtures built from TIFF 6.0's description decode to their known samples. | — |
| FI-11.4 | build | BMP `BI_JPEG`, `BI_PNG`, the `BI_CMYK` family, and OS/2 2.x Huffman and RLE24. | `crates/tinker-pdf-filters/src/bmp.rs`, `docs/features/filters.md` | Unit tests: fixtures built from the BMP description or generated once (dated) decode to their known samples. | — |

**Closes the row:** FI-11.1 to FI-11.4 decode a fixture per refusal to its known samples, which is the row's exit.

## FI-12 — JPEG XR's named refusals

**JPEG XR's named refusals**: fixed-point, half-float and float formats; packed sub-byte depths; CMYK, CMYKDIRECT, NCOMPONENT and RGBE; unknown pixel-format GUIDs; the interleaved alpha plane; YUV420, YUV422 and YUVK; a windowed origin

**Today.** each a `JxrRefusal` reached by `crates/tinker-pdf-filters/src/jxr/tests/refusals.rs` ([features/filters.md](../features/filters.md)); the platform encoder cannot emit most of them, so a fixture per row needs a real encoder that can (H-106)

**Exit criterion.** a fixture from a real encoder per row (H-106) decodes within the lossless identity or the bound computed in the test

**Doc:** — · **Depends on:** H-106

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-12.1 | build | JPEG XR's fixed-point, half-float and float formats, converted to the pipeline's precision. | `crates/tinker-pdf-filters/src/jxr/container.rs`, `crates/tinker-pdf-filters/src/jxr/colour.rs` | Fixtures from a real encoder decode within the lossless identity or the bound computed in the test. | H-106 |
| FI-12.2 | build | Packed sub-byte depths, CMYK, CMYKDIRECT, NCOMPONENT and RGBE, and every pixel-format GUID the specification lists. | `crates/tinker-pdf-filters/src/jxr/container.rs`, `crates/tinker-pdf-filters/src/jxr/colour.rs`, `crates/tinker-pdf-filters/src/jxr/headers.rs` | Fixtures from a real encoder decode within the identity or the bound. | FI-12.1 |
| FI-12.3 | build | The interleaved alpha plane, YUV420, YUV422 and YUVK, and a windowed origin. | `crates/tinker-pdf-filters/src/jxr/mod.rs`, `crates/tinker-pdf-filters/src/jxr/colour.rs`, `crates/tinker-pdf-filters/src/jxr/tests/refusals.rs`, `docs/features/filters.md` | Fixtures from a real encoder decode within the identity or the bound; the refusal tests keep only what the specification leaves undefined. | FI-12.2 |

**Closes the row:** FI-12.1 to FI-12.3 decode a real encoder's fixture per refusal within the identity or the computed bound, which is the row's exit.

## FI-13 — JPEG 2000 Part 2

**JPEG 2000 Part 2** markers and the extended file format features PDF can carry

**Today.** a marker Table A.2 does not define is refused as unknown rather than measured past (`features/filters.md:797`); a named non-goal of the roadmap until 9 October 2026

**Exit criterion.** each extension held to T.801's text and its annex examples where published

**Doc:** `design/jpx-part2.md` (owed) · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-13.1 | design | Create `docs/design/jpx-part2.md`: which T.801 extensions and extended file-format features PDF can carry, each with the clause and annex example it is held to, and what stays refused by name. *Note:* The run takes this row after every other row of its tier: H-30's default orders it last. | `docs/design/jpx-part2.md` (new) | The doc exists in the design-doc order, names FI-13.2 to FI-13.8's exit tests, and is reviewed. | — |
| FI-13.2 | build | Part 2's markers parsed to their fields instead of refused as unknown. | `crates/tinker-pdf-filters/src/jpx/codestream.rs` | Unit tests: each marker parses to T.801's fields; failing first on today's refusal. | FI-13.1 |
| FI-13.3 | build | Variable DC offset and the non-linear point transform. | `crates/tinker-pdf-filters/src/jpx/colour.rs` | Unit tests hold both to T.801's text and its examples where published. | FI-13.2 |
| FI-13.4 | build | Arbitrary decomposition styles and arbitrary transformation kernels. | `crates/tinker-pdf-filters/src/jpx/wavelet.rs` | Unit tests hold both to T.801's text and its examples. | FI-13.2 |
| FI-13.5 | build | Multiple component transformations, array-based and wavelet-based. | `crates/tinker-pdf-filters/src/jpx/colour.rs` | Unit tests hold them to T.801's text and its examples. | FI-13.2 |
| FI-13.6 | build | Trellis-coded quantisation, precinct-dependent quantisation and the extended region of interest. | `crates/tinker-pdf-filters/src/jpx/tier1.rs`, `crates/tinker-pdf-filters/src/jpx/codestream.rs` | Unit tests hold each to T.801's text and its examples. | FI-13.2 |
| FI-13.7 | build | The extended file format's features PDF can carry: further colour specifications, opacity, and compositing reduced as PDF's rule states. | `crates/tinker-pdf-filters/src/jpx/boxes.rs` | Unit tests: files carrying each feature decode as T.801 and PDF state. | FI-13.2 |
| FI-13.8 | test | The published annex examples as a suite, and a Part 2 decode fingerprint. | `crates/tinker-pdf-filters/tests/jpx_part2.rs` (new), `crates/tinker-pdf/tests/determinism.rs`, `docs/features/filters.md` | Every published example passes; the fingerprint holds on every K-07 target. | FI-13.3, FI-13.4, FI-13.5, FI-13.6, FI-13.7, K-07 |

**Closes the row:** FI-13.2 to FI-13.8 hold each extension to T.801's text and its annex examples, which is the row's exit.

## FI-14 — Image orientation and resolution

**Image orientation and resolution**: EXIF and TIFF orientation applied to bare images and archive pages, JPEG XR's container orientation applied when converting, resolution reported

**Today.** orientation is not applied; JPEG XR's container orientation transform was a named non-goal of the roadmap until 9 October 2026, because T.832 leaves it to an application the leaf crate is not (ruling 8) — so it is applied in the facade when converting, never in the leaf ([design/jpeg-xr.md](../design/jpeg-xr.md))

**Exit criterion.** fixtures in all eight orientations draw upright; reported resolution equals the tags

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-14.1 | build | Orientation applied in the facade: EXIF and TIFF orientation for bare images and archive pages, and JPEG XR's container orientation when converting. | `crates/tinker-pdf/src/cbz.rs`, `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/tests/image_orientation.rs` (new), `docs/features/filters.md` | `image_orientation.rs` (new): fixtures in all eight orientations draw upright; failing first on today's unapplied orientation. | — |
| FI-14.2 | build | Resolution reported from EXIF, TIFF, PNG and JFIF. | `crates/tinker-pdf/src/images.rs`, `crates/tinker-pdf/tests/image_orientation.rs` (new) | Reported resolution equals the tags for each source. | — |

**Closes the row:** FI-14.1 draws every orientation upright and FI-14.2 reports resolution as tagged, which is the row's exit.

## FI-15 — The Brotli filter

**The Brotli filter** (`/BrotliDecode`) in PDF streams

**Today.** a Brotli decoder exists in `tinker-pdf-filters` (`crates/tinker-pdf-filters/src/brotli.rs`) for WOFF2; no PDF filter name reaches it

**Exit criterion.** RFC 7932's test vectors through the PDF filter path; an unknown-filter fixture naming `/BrotliDecode` now decodes

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-15.1 | build | `/BrotliDecode` in the PDF filter chain, reaching the existing Brotli decoder. | `crates/tinker-pdf-cos/src/streams.rs`, `crates/tinker-pdf-filters/src/brotli.rs`, `crates/tinker-pdf/tests/brotli_filter.rs` (new), `docs/features/filters.md` | `brotli_filter.rs` (new): RFC 7932's test vectors decode through the PDF filter path; the unknown-filter fixture naming `/BrotliDecode` now decodes. | — |

**Closes the row:** FI-15.1 decodes RFC 7932's vectors through the filter path and the named fixture, which is the row's exit.

## FI-16 — JBIG2 remainder

**JBIG2 remainder**: colour palette segments, the T.88 amendment features, random-access organisation

**Today.** non-goals of the design until 9 October 2026 (`design/jbig2-symbol-text.md:75-77, 100-102`)

**Exit criterion.** each held to T.88's text and the amendments' examples where published

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-16.1 | design | Amend `docs/design/jbig2-symbol-text.md` with the remainder: colour palette segments, the amendment features, and random-access organisation. | `docs/design/jbig2-symbol-text.md` | The amended doc names FI-16.2 to FI-16.4's exit tests and is reviewed. | — |
| FI-16.2 | build | Random-access organisation: every segment header first, then the data. | `crates/tinker-pdf-filters/src/jbig2.rs` | Unit tests: a stream in random-access organisation decodes as its sequential twin. | FI-16.1 |
| FI-16.3 | build | Colour palette and colour extension segments. | `crates/tinker-pdf-filters/src/jbig2.rs` | Unit tests hold them to T.88's text and the amendments' examples where published. | FI-16.1 |
| FI-16.4 | build | The other T.88 amendment features the design lists. | `crates/tinker-pdf-filters/src/jbig2.rs`, `docs/features/filters.md` | Unit tests hold each to the amendment's text and examples where published. | FI-16.3 |

**Closes the row:** FI-16.2 to FI-16.4 hold each feature to T.88's text and the amendments' examples, which is the row's exit.

## FI-17a — A PostScript interpreter, milestone 1 — the language

**A PostScript interpreter, milestone 1 — the language**: scanner, objects, virtual memory with save and restore, dictionaries and stacks, the operator set, error handling

**Today.** absent — the owner's parity decision of 9 October 2026; PostScript is read only as the type 4 calculator subset (`crates/tinker-pdf-color/src/function.rs`) and inside Type 1 font programs

**Exit criterion.** each operator held to the PostScript language reference's stated behaviour and examples, transcribed into tests; the interpreter fuzzed (K-04)

**Doc:** `design/postscript.md` (owed) · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-17a.1 | design | Create `docs/design/postscript.md` for FI-17a to FI-17c: the scanner, objects and virtual memory, stacks and dictionaries, the Level 3 operator set, errors, the graphics model and fonts, DSC and EPS, and how the reference's examples are transcribed as tests. | `docs/design/postscript.md` (new) | The doc exists in the design-doc order, names this row's exit tests and the milestones FI-17b and FI-17c extend, and is reviewed. | H-102 |
| FI-17a.2 | build | The scanner: numbers with radix, literal, hex and ASCII85 strings, names, procedures, binary tokens and binary object sequences. | `crates/tinker-pdf-ps/src/scan.rs` (new), `crates/tinker-pdf-ps/src/lib.rs` (new), `crates/tinker-pdf-ps/Cargo.toml` (new), `Cargo.toml`, `docs/architecture.md` | Unit tests: the reference's scanner examples tokenise as stated; the new crate is in `docs/architecture.md`. | FI-17a.1 |
| FI-17a.3 | build | Objects and virtual memory: composites, access attributes, local and global VM, and `save` and `restore`. | `crates/tinker-pdf-ps/src/vm.rs` (new) | Unit tests: the reference's `save`/`restore` examples hold. | FI-17a.2 |
| FI-17a.4 | build | The stacks and dictionaries: operand, dictionary and execution stacks, `systemdict`, `userdict` and `globaldict`. | `crates/tinker-pdf-ps/src/interp.rs` (new) | Unit tests: the reference's stack and dictionary operators hold to their stated behaviour. | FI-17a.3 |
| FI-17a.5 | build | Control and execution: `exec`, the conditionals and loops, `stopped`, `exit` and `bind`. | `crates/tinker-pdf-ps/src/interp.rs` (new) | Unit tests hold each operator to the reference's stated behaviour and examples. | FI-17a.4 |
| FI-17a.6 | build | Arithmetic, mathematical, relational, boolean and bitwise operators. | `crates/tinker-pdf-ps/src/ops/math.rs` (new) | Unit tests hold each to the reference's behaviour and examples. | FI-17a.4 |
| FI-17a.7 | build | Array, packed-array, dictionary and string operators. | `crates/tinker-pdf-ps/src/ops/composite.rs` (new) | Unit tests hold each to the reference's behaviour and examples. | FI-17a.4 |
| FI-17a.8 | build | Type, attribute and conversion operators. | `crates/tinker-pdf-ps/src/ops/convert.rs` (new) | Unit tests hold each to the reference's behaviour and examples. | FI-17a.4 |
| FI-17a.9 | build | File operators over in-memory data, with the standard filters through the filters crate. | `crates/tinker-pdf-ps/src/ops/file.rs` (new) | Unit tests hold each to the reference's behaviour; no operator reaches the file system. | FI-17a.4 |
| FI-17a.10 | build | Resources and error handling: `findresource` and `defineresource`, `errordict` and `$error`, each error typed. | `crates/tinker-pdf-ps/src/ops/resource.rs` (new), `crates/tinker-pdf-ps/src/error.rs` (new) | Unit tests: each error arises where the reference states and is reported typed. | FI-17a.5 |
| FI-17a.11 | build | The interpreter's limits as ledger rows, and a deterministic clock. | `crates/tinker-pdf-ps/src/interp.rs` (new), `crates/tinker-pdf/tests/bounds_ledger.rs` | An injection per limit fires as a typed error, never a panic; each limit is a ledger row. | FI-17a.10 |
| FI-17a.12 | test | The operator set's coverage and a fuzz target over the interpreter. | `crates/tinker-pdf-ps/tests/operators.rs` (new), `fuzz/fuzz_targets/postscript.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md` | Every language operator has a test of its stated behaviour; the fuzz target runs clean with the roster regenerated. | FI-17a.6, FI-17a.7, FI-17a.8, FI-17a.9, FI-17a.11, K-04 |

**Closes the row:** FI-17a.2 to FI-17a.11 hold each operator to the reference's behaviour and examples, and FI-17a.12 covers the set and fuzzes the interpreter, which is the row's exit.

## FI-17b — PostScript, milestone 2 — graphics and fonts

**PostScript, milestone 2 — graphics and fonts**: paths, painting, images, colour spaces, patterns and shading, halftones, `setpagedevice`, Type 1, 3, 42 and CID fonts

**Today.** absent — the owner's parity decision of 9 October 2026; as FI-17a

**Exit criterion.** analytic fixtures written in PostScript render to the same pixels as PDF twins this engine's builder makes from the same content

**Doc:** `design/postscript.md` (owed) · **Depends on:** FI-17a, FT-10

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-17b.1 | design | Add milestone 2 to `docs/design/postscript.md`: the graphics state, paths and painting, images and colour spaces, patterns, shading, halftones, page devices and fonts, each rendered onto PDF pages. | `docs/design/postscript.md` | The section names FI-17b.2 to FI-17b.8's exit tests and is reviewed. | FI-17a.1 |
| FI-17b.2 | build | The graphics state, coordinate system and matrix operators. | `crates/tinker-pdf-ps/src/ops/graphics.rs` (new), `crates/tinker-pdf/src/postscript.rs` (new), `crates/tinker-pdf/tests/postscript_graphics.rs` (new) | `postscript_graphics.rs` (new): analytic fixtures written in PostScript render to the same pixels as their builder-made PDF twins. | FI-17b.1, FI-17a |
| FI-17b.3 | build | Path construction, clipping and painting. | `crates/tinker-pdf-ps/src/ops/graphics.rs` (new), `crates/tinker-pdf/tests/postscript_graphics.rs` (new) | Analytic fixtures render as their PDF twins. | FI-17b.2 |
| FI-17b.4 | build | Images and image masks, and every colour space family. | `crates/tinker-pdf-ps/src/ops/image.rs` (new), `crates/tinker-pdf/tests/postscript_graphics.rs` (new) | Analytic fixtures render as their PDF twins. | FI-17b.3 |
| FI-17b.5 | build | Patterns and shading types 1 to 7. | `crates/tinker-pdf-ps/src/ops/pattern.rs` (new), `crates/tinker-pdf/tests/postscript_graphics.rs` (new) | Analytic fixtures render as their PDF twins. | FI-17b.4 |
| FI-17b.6 | build | Halftones and transfer functions stored, `setpagedevice`, and `showpage` producing PDF pages. | `crates/tinker-pdf-ps/src/ops/device.rs` (new), `crates/tinker-pdf/tests/postscript_graphics.rs` (new) | Multi-page fixtures with page-device changes produce the pages their twins have. | FI-17b.3 |
| FI-17b.7 | build | Fonts and the show family: Type 1, 3 and 42 and CID fonts through the font crate and FT-10. | `crates/tinker-pdf-ps/src/ops/font.rs` (new), `crates/tinker-pdf/tests/postscript_graphics.rs` (new) | Analytic text fixtures render as their PDF twins and extract their text, in both builds. | FI-17b.3, FT-10.3, FT-10.5 |
| FI-17b.8 | test | PostScript opened as a document, a determinism fingerprint and a speed row. | `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/tests/determinism.rs`, `crates/tinker-pdf/benches/engine.rs`, `docs/features/opening.md` | `Document::open` opens a PostScript file; the fingerprint holds on every K-07 target; the speed row is in the benchmark set. | FI-17b.5, FI-17b.6, FI-17b.7, K-07, MS-04.8 |

**Closes the row:** FI-17b.2 to FI-17b.7 render analytic PostScript fixtures to the same pixels as their PDF twins, which is the row's exit.

## FI-17c — PostScript, milestone 3

**PostScript, milestone 3**: DSC comments, EPS with preview headers, PostScript XObjects in PDF

**Today.** absent — the owner's parity decision of 9 October 2026; EPS and PostScript XObjects are named only by the PDF/A and PDF/X rules that refuse them

**Exit criterion.** EPS figures placed in documents draw as their stand-alone renders

**Doc:** `design/postscript.md` (owed) · **Depends on:** FI-17b

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-17c.1 | design | Add milestone 3 to `docs/design/postscript.md`: DSC comments, EPS with preview headers, and PostScript XObjects in PDF. | `docs/design/postscript.md` | The section names FI-17c.2 and FI-17c.3's exit tests and is reviewed. | FI-17a.1 |
| FI-17c.2 | build | DSC comments read, and EPS (with or without a preview header) placed by its bounding box. | `crates/tinker-pdf/src/postscript.rs` (new), `crates/tinker-pdf/tests/eps.rs` (new) | `eps.rs` (new): EPS figures placed in documents draw as their stand-alone renders. | FI-17c.1, FI-17b |
| FI-17c.3 | build | PostScript XObjects in PDF run by the interpreter when the page renders. | `crates/tinker-pdf/src/resources.rs`, `crates/tinker-pdf/tests/eps.rs` (new), `docs/features/rendering.md` | A page with a PostScript XObject renders as the XObject's stand-alone render placed there. | FI-17c.2 |

**Closes the row:** FI-17c.2 places EPS figures drawing as their stand-alone renders and FI-17c.3 runs PostScript XObjects, which is the row's exit.

## FI-18a — PCL 5 (5e and 5c) input

**PCL 5 (5e and 5c) input**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bpcl\b|pcl5' crates tools` finds nothing

**Exit criterion.** analytic fixtures from the PCL 5 technical reference render to PDF twins this engine's builder makes from the same content

**Doc:** `design/pcl.md` (owed) · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-18a.1 | design | Create `docs/design/pcl.md` for FI-18a to FI-18c: the escape-sequence parser, the page model and units, fonts and symbol sets with the engine's faces standing in, raster graphics, area fills, macros, PCL 5c colour, PCL XL and HP-GL/2. | `docs/design/pcl.md` (new) | The doc exists in the design-doc order, names this row's exit tests and the milestones FI-18b and FI-18c extend, and is reviewed. | H-102 |
| FI-18a.2 | build | The parser and job and page control: page size, orientation, margins, units of measure and copies. | `crates/tinker-pdf/src/pcl/mod.rs` (new), `crates/tinker-pdf/src/pcl/parse.rs` (new), `crates/tinker-pdf/tests/pcl.rs` (new) | `pcl.rs` (new): analytic fixtures from the PCL 5 technical reference place pages and margins where the test computes. | FI-18a.1 |
| FI-18a.3 | build | Cursor positioning and text: symbol sets, font selection by characteristics mapped to the engine's faces, and downloaded soft fonts. | `crates/tinker-pdf/src/pcl/text.rs` (new), `crates/tinker-pdf/tests/pcl.rs` (new) | Analytic text fixtures render to their builder-made PDF twins, in both builds. | FI-18a.2 |
| FI-18a.4 | build | Raster graphics with its compression modes. | `crates/tinker-pdf/src/pcl/raster.rs` (new), `crates/tinker-pdf/tests/pcl.rs` (new) | Analytic raster fixtures render to their PDF twins. | FI-18a.2 |
| FI-18a.5 | build | Rectangular area fills, patterns and the print model's transparency. | `crates/tinker-pdf/src/pcl/fill.rs` (new), `crates/tinker-pdf/tests/pcl.rs` (new) | Analytic fill fixtures render to their PDF twins. | FI-18a.2 |
| FI-18a.6 | build | Macros and overlays. | `crates/tinker-pdf/src/pcl/parse.rs` (new), `crates/tinker-pdf/tests/pcl.rs` (new) | Fixtures using macros render as their expanded twins. | FI-18a.2 |
| FI-18a.7 | build | PCL 5c colour: image data configuration, palettes, colour modes and colour raster. | `crates/tinker-pdf/src/pcl/colour.rs` (new), `crates/tinker-pdf/tests/pcl.rs` (new) | Analytic colour fixtures render to their PDF twins. | FI-18a.4 |
| FI-18a.8 | test | PCL opened as a document, a fuzz target and a determinism fingerprint. | `crates/tinker-pdf/src/standalone.rs`, `fuzz/fuzz_targets/pcl.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md`, `crates/tinker-pdf/tests/determinism.rs` | `Document::open` opens a PCL job; the fuzz target runs clean with the roster regenerated; the fingerprint holds on every K-07 target. | FI-18a.3, FI-18a.5, FI-18a.6, FI-18a.7, K-07 |

**Closes the row:** FI-18a.2 to FI-18a.7 render analytic fixtures from the PCL 5 reference to their PDF twins, which is the row's exit.

## FI-18b — PCL XL input

**PCL XL input**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'pclxl|pcl.xl|pcl6' crates tools` finds nothing

**Exit criterion.** as FI-18a, against the PCL XL protocol reference

**Doc:** `design/pcl.md` (owed) · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-18b.1 | design | Add PCL XL to `docs/design/pcl.md`: the stream header, encodings, attribute lists and operators, and each graphics family. | `docs/design/pcl.md` | The section names FI-18b.2 to FI-18b.7's exit tests and is reviewed. | FI-18a.1 |
| FI-18b.2 | build | The stream header, binary and ASCII encodings, attribute lists and operator parsing. | `crates/tinker-pdf/src/pcl/xl.rs` (new), `crates/tinker-pdf/tests/pcl_xl.rs` (new) | `pcl_xl.rs` (new): streams built from the protocol reference parse to their operators and attributes. | FI-18b.1, FI-18a.2 |
| FI-18b.3 | build | Page and graphics-state operators: pages, origin, scale, colour space, pen and brush. | `crates/tinker-pdf/src/pcl/xl.rs` (new), `crates/tinker-pdf/tests/pcl_xl.rs` (new) | Analytic fixtures render to their builder-made PDF twins. | FI-18b.2 |
| FI-18b.4 | build | Paths and painting: lines, Béziers, arcs, rectangles and ellipses. | `crates/tinker-pdf/src/pcl/xl.rs` (new), `crates/tinker-pdf/tests/pcl_xl.rs` (new) | Analytic fixtures render to their PDF twins. | FI-18b.3 |
| FI-18b.5 | build | Images and raster patterns with their compressions. | `crates/tinker-pdf/src/pcl/xl.rs` (new), `crates/tinker-pdf/tests/pcl_xl.rs` (new) | Analytic fixtures render to their PDF twins. | FI-18b.3 |
| FI-18b.6 | build | Downloaded fonts and text with escapements. | `crates/tinker-pdf/src/pcl/xl.rs` (new), `crates/tinker-pdf/tests/pcl_xl.rs` (new) | Analytic text fixtures render to their PDF twins and extract their text. | FI-18b.3 |
| FI-18b.7 | test | PCL XL opened as a document and fuzzed. | `crates/tinker-pdf/src/standalone.rs`, `fuzz/fuzz_targets/pcl_xl.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md` | `Document::open` opens a PCL XL job; the fuzz target runs clean with the roster regenerated. | FI-18b.4, FI-18b.5, FI-18b.6 |

**Closes the row:** FI-18b.3 to FI-18b.6 render analytic fixtures from the PCL XL reference to their PDF twins, which is the row's exit.

## FI-18c — HP-GL/2 input

**HP-GL/2 input**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'hpgl|hp-gl' crates tools` finds nothing

**Exit criterion.** as FI-18a

**Doc:** `design/pcl.md` (owed) · **Depends on:** FI-18a

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-18c.1 | design | Add HP-GL/2 to `docs/design/pcl.md`: its instructions inside PCL 5 and on their own. | `docs/design/pcl.md` | The section names FI-18c.2 and FI-18c.3's exit tests and is reviewed. | FI-18a.1 |
| FI-18c.2 | build | HP-GL/2's vector instructions, line types and attributes, and fill types, inside PCL 5 and stand-alone. | `crates/tinker-pdf/src/pcl/hpgl.rs` (new), `crates/tinker-pdf/tests/hpgl.rs` (new) | `hpgl.rs` (new): analytic fixtures render to their builder-made PDF twins. | FI-18c.1, FI-18a |
| FI-18c.3 | build | Labels and character sets, scaling, input windows, picture frames and rotation. | `crates/tinker-pdf/src/pcl/hpgl.rs` (new), `crates/tinker-pdf/tests/hpgl.rs` (new) | Analytic fixtures render to their PDF twins. | FI-18c.2 |

**Closes the row:** FI-18c.2 and FI-18c.3 render analytic fixtures to their PDF twins, as FI-18a's exit states.

## FI-19 — DjVu

**DjVu**: IW44 wavelets, JB2, BZZ, bundled and indirect documents, the text layer, annotations and outlines

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'djvu|iw44' crates tools` finds nothing

**Exit criterion.** JB2 and BZZ held exactly to the encoder's known input — bitmaps and text generated here, encoded once by an outside encoder (dated, bytes only) and committed beside their inputs; IW44 held to the specification's ZP-coder and wavelet arithmetic recomputed in the test, and an encoded photograph to a bound computed in the test from the encoder's input; the text layer, annotations and outlines to the values written

**Doc:** `design/djvu.md` (owed) · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-19.1 | design | Create `docs/design/djvu.md`: the IFF container and multi-page documents, the ZP coder, BZZ, JB2 and IW44, layer composition, the text layer, annotations and outlines, and the outside-encoded fixtures each coder is held to. | `docs/design/djvu.md` (new) | The doc exists in the design-doc order, names FI-19.2 to FI-19.7's exit tests, and is reviewed. | — |
| FI-19.2 | build | The IFF container: single pages, bundled documents and indirect ones through FI-27's provider. | `crates/tinker-pdf-filters/src/djvu/mod.rs` (new), `crates/tinker-pdf-filters/src/lib.rs` | Unit tests: generated containers parse to their chunks and pages. | FI-19.1, FI-27 |
| FI-19.3 | build | The ZP coder and BZZ decompression. | `crates/tinker-pdf-filters/src/djvu/zp.rs` (new), `crates/tinker-pdf-filters/src/djvu/bzz.rs` (new) | Unit tests: the ZP arithmetic equals the specification's recomputed in the test; BZZ data encoded once outside from committed text (dated, bytes only) decodes to that text exactly. | FI-19.2, H-106 |
| FI-19.4 | build | JB2 bilevel images with their symbol dictionaries. | `crates/tinker-pdf-filters/src/djvu/jb2.rs` (new) | Unit tests: bitmaps generated here, encoded once outside, decode to their inputs exactly. | FI-19.3 |
| FI-19.5 | build | IW44 wavelet images, progressive and in colour. | `crates/tinker-pdf-filters/src/djvu/iw44.rs` (new) | Unit tests: the wavelet arithmetic equals the specification's recomputed in the test; an encoded photograph decodes within the bound computed from the encoder's input. | FI-19.3 |
| FI-19.6 | build | Pages composed from mask, foreground and background, with the text layer, annotations and outlines. | `crates/tinker-pdf/src/djvu.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/djvu.rs` (new), `docs/features/opening.md` | `djvu.rs` (new): composed pages draw as the test composes them; the text layer, annotations and outlines equal the values written. | FI-19.4, FI-19.5 |
| FI-19.7 | build | DjVu sniffed and opened, fuzzed and fingerprinted. | `crates/tinker-pdf/src/standalone.rs`, `fuzz/fuzz_targets/djvu.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md`, `crates/tinker-pdf/tests/determinism.rs` | `Document::open` opens a DjVu document; the fuzz target runs clean with the roster regenerated; the fingerprint holds on every K-07 target. | FI-19.6, K-07 |

**Closes the row:** FI-19.3 to FI-19.6 hold BZZ and JB2 exactly to known inputs, IW44 to the recomputed arithmetic and its bound, and the text layer, annotations and outlines to the values written, which is the row's exit.

## FI-20a — OFD

**OFD** (GB/T 33190): the package, pages, text, graphics and images

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bofd\b' crates tools` finds nothing

**Exit criterion.** documents built in the test from GB/T 33190's schema, and documents a producer generated once (dated, bytes only) from content committed beside them, render to twins this engine's own builder makes from the same content

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-20a.1 | build | The OFD package read: its entry document, documents, pages, resources and templates. | `crates/tinker-pdf/src/ofd/mod.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/ofd.rs` (new) | `ofd.rs` (new): documents built in the test from GB/T 33190's schema parse to their pages and resources. | — |
| FI-20a.2 | build | Pages drawn: text objects with their glyph positions, path objects, images, colour spaces and clips, synthesised through the builder. | `crates/tinker-pdf/src/ofd/render.rs` (new), `crates/tinker-pdf/tests/ofd.rs` (new) | Documents built from the schema render to twins the builder makes from the same content. | FI-20a.1 |
| FI-20a.3 | build | Producer documents, and OFD sniffed and opened. | `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/tests/ofd.rs` (new), `docs/features/opening.md` | Documents a producer generated once (dated, bytes only) from committed content render to their builder twins; `Document::open` opens OFD. | FI-20a.2, H-106 |

**Closes the row:** FI-20a.2 and FI-20a.3 render schema-built and producer documents to their builder twins, which is the row's exit.

## FI-20b — SM2 and SM3

**SM2 and SM3** for OFD signatures

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bsm2\b|\bsm3\b' crates tools` finds nothing

**Exit criterion.** the published GM/T test vectors

**Doc:** — · **Depends on:** FI-20a

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-20b.1 | build | SM3. | `crates/tinker-pdf-crypto/src/sm3.rs` (new), `crates/tinker-pdf-crypto/src/lib.rs` | Unit tests: the published GM/T vectors pass. | — |
| FI-20b.2 | build | SM2 signature verification and constant-time signing. | `crates/tinker-pdf-crypto/src/sm2.rs` (new) | Unit tests: the published GM/T vectors pass; an operation-count test shows signing's counts independent of the key. | FI-20b.1 |
| FI-20b.3 | build | OFD signatures verified with SM2 and SM3. | `crates/tinker-pdf/src/ofd/signature.rs` (new), `crates/tinker-pdf/tests/ofd.rs` (new), `docs/features/signatures.md` | Signed OFDs generated in the test verify; a tampered one fails. | FI-20b.2, FI-20a |

**Closes the row:** FI-20b.1 and FI-20b.2 pass the published GM/T vectors and FI-20b.3 verifies OFD signatures with them, which is the row's exit.

## FI-21 — Compound files and mail

**Compound files and mail**: CFB/OLE2 archives, EML and MSG messages with attachments, MHTML and WARC pages, CHM help files

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bmhtml\b|\beml\b|\bwarc\b|\bchm\b|olecf' crates tools` finds nothing

**Exit criterion.** generated messages and archives open with every part; attachments carried as PDF attachments

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-21.1 | build | CFB/OLE2 compound files: sectors, allocation tables, the directory and mini streams. | `crates/tinker-pdf-archive/src/cfb.rs` (new), `crates/tinker-pdf-archive/src/lib.rs` | Unit tests: generated compound files open with every stream byte for byte. | — |
| FI-21.2 | build | EML messages: headers, MIME parts with their transfer encodings and charsets, the body laid out by the HTML path and attachments carried as PDF attachments. | `crates/tinker-pdf/src/mail.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/mail.rs` (new) | `mail.rs` (new): generated messages open with every part; each attachment is a PDF attachment byte for byte; in the default and `bundled-fonts` builds. | — |
| FI-21.3 | build | MSG messages from their compound file's properties, opened the same way. | `crates/tinker-pdf/src/mail.rs` (new), `crates/tinker-pdf/tests/mail.rs` (new) | Generated MSG files open with every part and attachment. | FI-21.1, FI-21.2 |
| FI-21.4 | build | MHTML and WARC pages through the HTML path with their resources from the archive. | `crates/tinker-pdf/src/mail.rs` (new), `crates/tinker-pdf/tests/mail.rs` (new) | Generated pages open with every resource drawn. | FI-21.2 |
| FI-21.5 | build | CHM help files: the container and its LZX compression, pages through the HTML path. | `crates/tinker-pdf-archive/src/chm.rs` (new), `crates/tinker-pdf-archive/src/lzx.rs` (new), `crates/tinker-pdf/tests/mail.rs` (new) | Generated help files open with every page; LZX holds to its published description. | FI-21.4 |
| FI-21.6 | build | Each format sniffed and opened, and documented. | `crates/tinker-pdf/src/standalone.rs`, `docs/features/opening.md` | `Document::open` opens each format; the feature doc names what is read. | FI-21.3, FI-21.5 |

**Closes the row:** FI-21.1 to FI-21.5 open generated messages and archives with every part and carry attachments as PDF attachments, which is the row's exit.

## FI-22 — Customizable `<select>`'s `<selectedcontent>`

**Customizable `<select>`'s `<selectedcontent>`**

**Today.** Loose HTML that is not XML is read by `tinker_pdf_xml::html`, a hand-written WHATWG §13.2.5 tokenizer and §13.2.6 tree builder held to html5lib's suite vendored at `9329e64` (MIT, THIRDPARTY.md): **1 779 of the 1 784 tree-construction tests** that run with scripting disabled (`tests/html5lib.rs`, the five that do not pass named as a list), **7 028 of 7 028 tokenizer runs** (`src/html/suite.rs`; four lone-surrogate runs a Rust string cannot hold not attempted), and since 9 October 2026 **82 of 82 encoding tests**. **What is left** is the four tests of customizable `<select>`'s `<selectedcontent>` (`webkit02.dat` #45–#48): the element is built empty (`features/opening.md:325`), where the DOM makes a copy of the selected option when the parser pops an `<option>`, which needs the option selectedness algorithm; `tests1.dat` #77 fails on the shared 1 024-byte name cap, which is kept. Not attempted, by name: html5lib's `#script-on` tests (CD-12), its `scripted/` directory, and the standard's new processing instructions, which the vendored suite predates

**Exit criterion.** the selectedness algorithm and the copy; those four tests pass

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-22.1 | fix | Customizable `<select>`'s selectedness algorithm and the copy into `<selectedcontent>` in the HTML tree builder. | `crates/tinker-pdf-xml/src/html`, `crates/tinker-pdf-xml/tests/html5lib.rs` | The four html5lib tree-construction tests that need it pass, and no other changes. | — |

**Closes the row:** FI-22.1 implements the algorithm and the copy so those four tests pass, which is the row's exit; a producer's document using the customizable `<select>` is H-107's evidence, outside this exit.

## FI-23 — Multi-byte legacy encodings

**Multi-byte legacy encodings** — GBK, gb18030, Big5, EUC-JP, ISO-2022-JP, Shift_JIS, EUC-KR — in FB2, loose HTML and XHTML

**Today.** An FB2 in an 8-bit encoding opens since 3 October 2026: a declaration naming any of the WHATWG Encoding Standard's twenty-eight single-byte encodings, by any of its labels that XML 1.0's `EncName` [81] admits, is decoded by that encoding's index — vendored verbatim from `whatwg/encoding` into `tinker-pdf-xml/data/encoding-indexes` under the licence's BSD-3-Clause grant for portions incorporated into source code (THIRDPARTY.md says why that reading, H-90) — held by `tests/fb2.rs`'s `an_fb2_in_an_eight_bit_encoding_is_the_book_its_utf8_twin_is`. **What is left**: the multi-byte legacy encodings, which still open an FB2 as an empty page with `Markup(Truncated)` and make loose HTML report `MarkupDefect::EncodingNotDecoded` (`features/opening.md:327, 332`), each a state machine over an index of thousands of rows; and a measurement, since this repository has no FB2 corpus to say which encodings real books use (H-107)

**Exit criterion.** the multi-byte decoders from the same repository's indexes, each held to the vendored index and to the encoding standard's own test files; an FB2 in each encoding equals its UTF-8 twin

**Doc:** — · **Depends on:** H-90 (index licence)

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-23.1 | build | GBK and gb18030 decoders from the vendored WHATWG indexes, gb18030's ranges included. *Note:* H-90 reads the WHATWG grant these indexes are published under, and H-116 lists multi-byte encoding indexes among the data licences it rules; the indexes are vendored once both have answered. | `crates/tinker-pdf-xml/src/encoding.rs` | Unit tests: each decoder holds to the vendored index and the encoding standard's test files. | H-90, H-116 |
| FI-23.2 | build | Big5 and EUC-KR. | `crates/tinker-pdf-xml/src/encoding.rs` | Unit tests: each holds to its index and the standard's test files. | FI-23.1 |
| FI-23.3 | build | EUC-JP, stateful ISO-2022-JP and Shift_JIS. | `crates/tinker-pdf-xml/src/encoding.rs` | Unit tests: each holds to its index and the standard's test files. | FI-23.1 |
| FI-23.4 | build | The decoders wired into FB2, loose HTML and XHTML by their declarations and labels. | `crates/tinker-pdf/src/fb2.rs`, `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/tests/legacy_encodings.rs` (new), `docs/features/opening.md` | `legacy_encodings.rs` (new): an FB2 in each encoding equals its UTF-8 twin, and so does a loose HTML page. | FI-23.2, FI-23.3 |

**Closes the row:** FI-23.1 to FI-23.3 hold each decoder to the vendored index and the standard's tests, and FI-23.4 makes each FB2 equal its UTF-8 twin, which is the row's exit; an FB2 corpus in these encodings is H-107's evidence, outside this exit.

## FI-24 — Sniffing refusals

**Sniffing refusals**: a bare BMP, an SVG or HTML root past byte 4 096

**Today.** `OpenError::NotAPdf` (`features/opening.md:335-336`)

**Exit criterion.** each opens; a PDF header in the first 4 096 bytes still wins

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-24.1 | fix | Sniffing: a bare BMP, and an SVG or HTML root found past byte 4 096 within a stated cap, a PDF header in the first 4 096 bytes still winning. | `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/sniffing.rs` (new), `docs/features/opening.md` | `sniffing.rs` (new): each case opens; a file with both a PDF header and a markup root opens as PDF; failing first on today's `NotAPdf`. | — |

**Closes the row:** FI-24.1 opens each case with a PDF header still winning, which is the row's exit.

## FI-25 — Archive breadth

**Archive breadth**: 7z coders not read, ZIP shrink, implode, XZ and PPMd, Zstandard dictionaries, randomised bzip2, sparse tar entries

**Today.** refused by name (`features/cbz.md:414, 418, 427-429`) — among them a Zstandard frame that names a dictionary, and bzip2's randomised blocks, unwritten since 1999

**Exit criterion.** each method held to its published description with fixtures generated once (dated, bytes only)

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-25.1 | build | The 7z coders the feature doc names as not read. | `crates/tinker-pdf-archive/src/sevenz.rs` | Unit tests: each coder holds to its published description; fixtures generated once (dated, bytes only) open. | H-106 |
| FI-25.2 | build | ZIP shrink and implode. | `crates/tinker-pdf-zip/src/lib.rs` | Unit tests: each method holds to its published description; fixtures generated once open. | H-106 |
| FI-25.3 | build | XZ and ZIP's PPMd. | `crates/tinker-pdf-archive/src/lzma.rs`, `crates/tinker-pdf-archive/src/ppmd.rs`, `crates/tinker-pdf-zip/src/lib.rs` | Unit tests: each holds to its description; fixtures generated once open. | H-106 |
| FI-25.4 | build | Zstandard frames naming a dictionary, the dictionary supplied by the host. | `crates/tinker-pdf-archive/src/zstd.rs` | Unit tests: a frame with its dictionary decodes as RFC 8878 states; without it, a typed refusal naming the dictionary. | H-106 |
| FI-25.5 | build | Randomised bzip2 blocks and sparse tar entries. | `crates/tinker-pdf-archive/src/bzip2.rs`, `crates/tinker-pdf-archive/src/tar.rs`, `docs/features/cbz.md` | Unit tests: each holds to its description; fixtures generated once open; the refusal rows go. | H-106 |

**Closes the row:** FI-25.1 to FI-25.5 hold each method to its published description with fixtures generated once, which is the row's exit.

## FI-26 — A picture past Annex C.2's 14 400 units

**A picture past Annex C.2's 14 400 units** kept inside the limit by `/UserUnit` or a scaled placement

**Today.** "not clamped, and owed" (`features/cbz.md:432`)

**Exit criterion.** an oversized picture opens as a page within 14 400 units and draws at a bounded cost

**Doc:** — · **Depends on:** ED-09

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-26.1 | fix | A picture past Annex C.2's 14 400 units opened as a page within the limit, by ED-09's `/UserUnit` or a scaled placement, and drawn at a bounded cost. | `crates/tinker-pdf/src/cbz.rs`, `crates/tinker-pdf/tests/oversized_pictures.rs` (new), `docs/features/cbz.md` | `oversized_pictures.rs` (new): an oversized picture opens as a page within 14 400 units and draws with counted work under the stated bound. | ED-09 |

**Closes the row:** FI-26.1 opens an oversized picture within the limit and draws it at a bounded cost, which is the row's exit.

## FI-27 — A host resource provider

**A host resource provider** for what a loose file names beside itself and for an EPUB's remote resources (the engine still does no I/O)

**Today.** `StylesheetUnresolved`, `ImageNotDrawn { Unresolved }` and their kin (`features/opening.md:338`); `remote-resources` tallied as `UnimplementedFeature` ([features/epub.md](../features/epub.md))

**Exit criterion.** a test provider supplies the resources and the page draws as its self-contained twin

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-27.1 | build | A host resource provider for what a loose file names beside itself and for an EPUB's remote resources: the engine asks, the host answers with bytes or nothing, and the engine performs no I/O. | `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/src/html.rs`, `crates/tinker-pdf/src/epub.rs`, `crates/tinker-pdf/tests/resource_provider.rs` (new), `docs/features/opening.md` | `resource_provider.rs` (new): a test provider supplies the resources and each page draws as its self-contained twin; without one, today's warnings remain. | — |
| FI-27.2 | doc | EPUB's `remote-resources` tally moved from unimplemented to provided-by-host in the feature doc and its census. | `docs/features/epub.md`, `crates/tinker-pdf/src/epub.rs` | The tally counts books whose remote resources the provider answered apart from those it could not. | FI-27.1 |

**Closes the row:** FI-27.1's test provider makes each page draw as its self-contained twin, which is the row's exit.

## FI-28 — Relayout of reflowable documents

**Relayout of reflowable documents**: re-paginate at a new page size, font size or margin, and after a font provider attached late, with locations (chapter, offset) that survive; layout accelerators saved and reloaded

**Today.** advances decide line breaks at open; "a font provider attached after open cannot re-paginate", since advances decided the line breaks at open (`ArchiveWarning::FontsAttachedAfterPagination`); a design question before it is work, the EPUB row this replaces said

**Exit criterion.** a book laid out at size A then B equals the book opened at B; a location resolves to the same text at both

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-28.1 | design | Amend `docs/design/epub-layout.md` with relayout: what is kept from the parse, how locations are named so they survive, how a late font provider re-paginates, and what the saved accelerators hold. | `docs/design/epub-layout.md` | The amended doc names FI-28.2 to FI-28.5's exit tests and is reviewed. | — |
| FI-28.2 | build | Re-pagination at a new page size, font size or margin from the parsed book. | `crates/tinker-pdf/src/epub.rs`, `crates/tinker-pdf/src/epub/package.rs`, `crates/tinker-pdf/tests/relayout.rs` (new) | `relayout.rs` (new): a book laid out at size A then B equals the book opened at B. | FI-28.1 |
| FI-28.3 | build | Locations (chapter and offset) that survive relayout. | `crates/tinker-pdf/src/epub.rs`, `crates/tinker-pdf/tests/relayout.rs` (new) | A location resolves to the same text at both sizes. | FI-28.2 |
| FI-28.4 | build | Relayout after a font provider attached late, retiring `ArchiveWarning::FontsAttachedAfterPagination`. | `crates/tinker-pdf/src/epub.rs`, `crates/tinker-pdf/tests/relayout.rs` (new) | A book given its provider after open equals the book opened with it; failing first on today's warning. | FI-28.2 |
| FI-28.5 | build | Layout accelerators saved as host-held bytes and reloaded. | `crates/tinker-pdf/src/epub.rs`, `crates/tinker-pdf/tests/relayout.rs` (new), `docs/features/epub.md` | Reloaded accelerators give identical pages; accelerators for other settings are ignored. | FI-28.3 |

**Closes the row:** FI-28.2 makes A-then-B equal B and FI-28.3 resolves a location to the same text at both, which is the row's exit.

## FI-29 — A reflowable view of fixed documents

**A reflowable view of fixed documents**: a PDF re-laid out from its structured text (and its tags where present) at any width

**Today.** absent — the owner's parity decision of 9 October 2026; re-layout exists only for EPUB's reflowable books (`crates/tinker-pdf/src/epub/package.rs:307-315`); no fixed page is re-laid out

**Exit criterion.** a tagged committed document's reflow extracts in the tree's order; an untagged one in the inferred order

**Doc:** — · **Depends on:** FT-30

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-29.1 | build | Structured text turned into a flow of blocks: the structure tree's order where tagged, FT-30's inferred order where not, with headings, lists, tables and images. | `crates/tinker-pdf/src/reflow.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/reflow.rs` (new) | `reflow.rs` (new): a tagged committed document's flow is in the tree's order and an untagged one's in the inferred order. | FT-30 |
| FI-29.2 | build | The flow laid out at any width through the HTML path. | `crates/tinker-pdf/src/reflow.rs` (new), `crates/tinker-pdf/tests/reflow.rs` (new) | Reflow at two widths extracts the same text in the same order. | FI-29.1 |
| FI-29.3 | build | `tpdf reflow --width` and a fingerprint of one reflowed page. | `tools/tpdf/src/main.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/content-and-text.md` | The command reflows `testdata/` fixtures to the facade's output; the fingerprint holds on every K-07 target. | FI-29.2, K-07 |

**Closes the row:** FI-29.1 and FI-29.2 reflow a tagged document in the tree's order and an untagged one in the inferred order, which is the row's exit.

## FI-30 — A custom document-handler seam

**A custom document-handler seam**: a host registers a format the engine does not read, synthesised through the builder

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'DocumentHandler|register_format' crates tools` finds nothing

**Exit criterion.** a test handler's format opens through `Document::open`

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-30.1 | build | A custom document-handler seam: a host registers a sniffing test and a synthesiser over the builder, consulted by `Document::open` after the built-in formats. | `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/tests/document_handlers.rs` (new), `docs/features/opening.md` | `document_handlers.rs` (new): a test handler's format opens through `Document::open`. | — |

**Closes the row:** FI-30.1 opens a test handler's format through `Document::open`, which is the row's exit.

## FI-31 — Files over 4 GiB

**Files over 4 GiB**: 64-bit offsets in `ByteSource`, wide cross-reference fields, streaming paths that never hold the file

**Today.** the facade takes bytes; wasm32's address space bounds a slice

**Exit criterion.** a generated 5 GiB file opens and renders its last page within the MS-05 band (native targets)

**Doc:** — · **Depends on:** P-05

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-31.1 | build | 64-bit offsets in `ByteSource` and the parser, and wide cross-reference stream fields. | `crates/tinker-pdf-cos/src/source.rs`, `crates/tinker-pdf-cos/src/xref.rs`, `crates/tinker-pdf-cos/tests/large_files.rs` (new) | `large_files.rs` (new): a synthetic source with offsets past 4 GiB parses its cross-reference and objects. | P-05 |
| FI-31.2 | build | Streaming paths that never hold the file: open, page lookup and rendering by ranges. | `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/large_files.rs` (new) | A generated 5 GiB file, produced lazily by a synthetic source and never held, opens and renders its last page within its MS-05 band on native targets. | FI-31.1, MS-05 |
| FI-31.3 | build | On wasm32, a file past the address space refused by a typed error. | `crates/tinker-pdf-cos/src/source.rs`, `docs/features/opening.md` | The wasm build's test returns the typed error and never panics. | FI-31.1 |

**Closes the row:** FI-31.2 opens a 5 GiB file and renders its last page within the MS-05 band, which is the row's exit.

## FI-32 — Container streaming and prefetch hints

**Container streaming and prefetch hints**: EPUB and archives opened from a `ByteSource`; a linearized file's hint tables offered as prefetch ranges

**Today.** streaming open for PDF (`features/opening.md:218-249`); containers need all bytes

**Exit criterion.** a streamed book's first page draws after fetching only its parts; the prefetch list equals the hint tables' ranges

**Doc:** — · **Depends on:** MS-08

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-32.1 | build | EPUB and archives opened from a `ByteSource`: the ZIP directory read from the end by ranges and only a page's parts fetched. | `crates/tinker-pdf-zip/src/lib.rs`, `crates/tinker-pdf/src/epub.rs`, `crates/tinker-pdf/tests/container_streaming.rs` (new), `docs/features/opening.md` | `container_streaming.rs` (new): a streamed book's first page draws after fetching only its parts, the fetched ranges counted. | MS-08 |
| FI-32.2 | build | A linearized file's hint tables offered as prefetch ranges. | `crates/tinker-pdf-cos/src/linearize.rs`, `crates/tinker-pdf/tests/container_streaming.rs` (new) | The prefetch list equals the hint tables' ranges for each page. | — |

**Closes the row:** FI-32.1 draws a streamed book's first page from its parts and FI-32.2's prefetch list equals the hint tables' ranges, which is the row's exit.

## FI-33 — XSL-FO input

**XSL-FO input**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'xsl-fo|fo:block' crates tools` finds nothing

**Exit criterion.** analytic fixtures from the XSL 1.1 recommendation render to their HTML-path twins

**Doc:** `design/xsl-fo.md` (owed) · **Depends on:** CD-16

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-33.1 | design | Create `docs/design/xsl-fo.md`: formatting objects and properties mapped onto the layout crate's box model and CD-16's paged media, what needs new layout, and the analytic fixtures from XSL 1.1 against their HTML-path twins. | `docs/design/xsl-fo.md` (new) | The doc exists in the design-doc order, names FI-33.2 to FI-33.8's exit tests, and is reviewed. | CD-16 |
| FI-33.2 | build | The formatting-object tree with property inheritance and expressions. | `crates/tinker-pdf/src/xslfo/mod.rs` (new), `crates/tinker-pdf/src/lib.rs` | Unit tests: property refinement follows XSL 1.1's rules for each case. | FI-33.1 |
| FI-33.3 | build | Page masters, page-sequence masters, regions and static content. | `crates/tinker-pdf/src/xslfo/pages.rs` (new), `crates/tinker-pdf/tests/xsl_fo.rs` (new) | `xsl_fo.rs` (new): analytic fixtures render to their HTML-path twins. | FI-33.2 |
| FI-33.4 | build | Blocks and inline formatting: blocks, inlines, characters, leaders and external graphics. | `crates/tinker-pdf/src/xslfo/blocks.rs` (new), `crates/tinker-pdf/tests/xsl_fo.rs` (new) | Analytic fixtures render to their HTML-path twins. | FI-33.3 |
| FI-33.5 | build | Lists and tables. | `crates/tinker-pdf/src/xslfo/blocks.rs` (new), `crates/tinker-pdf/tests/xsl_fo.rs` (new) | Analytic fixtures render to their HTML-path twins. | FI-33.4 |
| FI-33.6 | build | Footnotes, floats, markers and page-number citations. | `crates/tinker-pdf/src/xslfo/pages.rs` (new), `crates/tinker-pdf/tests/xsl_fo.rs` (new) | Analytic fixtures render to their HTML-path twins. | FI-33.4 |
| FI-33.7 | build | Basic links and the bookmark tree as PDF links and outline. | `crates/tinker-pdf/src/xslfo/blocks.rs` (new), `crates/tinker-pdf/tests/xsl_fo.rs` (new) | Links land on their targets and the outline equals the bookmark tree. | FI-33.4 |
| FI-33.8 | test | XSL-FO sniffed and opened, and fingerprinted. | `crates/tinker-pdf/src/standalone.rs`, `crates/tinker-pdf/tests/determinism.rs`, `docs/features/opening.md` | `Document::open` opens an FO document; the fingerprint holds on every K-07 target. | FI-33.5, FI-33.6, FI-33.7, K-07 |

**Closes the row:** FI-33.3 to FI-33.7 render analytic fixtures from XSL 1.1 to their HTML-path twins, which is the row's exit.

## FI-34 — A partly encrypted EPUB opens its unencrypted parts

**A partly encrypted EPUB opens its unencrypted parts** (ruling 2), each encrypted resource a typed warning

**Today.** the whole book is refused (`ArchiveRefusal::EncryptedResources`, `features/epub.md:604`)

**Exit criterion.** a generated book with one encrypted chapter opens with the rest drawn and one warning

**Doc:** — · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-34.1 | fix | A partly encrypted EPUB opens its unencrypted parts (ruling 2), each encrypted resource a typed warning and drawn as missing, narrowing `ArchiveRefusal::EncryptedResources` to books with nothing readable. | `crates/tinker-pdf/src/epub.rs`, `crates/tinker-pdf/tests/epub_partial_encryption.rs` (new), `docs/features/epub.md` | `epub_partial_encryption.rs` (new): a generated book with one encrypted chapter opens with the rest drawn and one warning; failing first on today's refusal. | — |

**Closes the row:** FI-34.1 opens a book with one encrypted chapter, the rest drawn and one warning, which is the row's exit.

## FI-35 — AFP and MO:DCA print streams

**AFP and MO:DCA print streams**

**Today.** absent — the owner's parity decision of 9 October 2026

**Exit criterion.** streams generated from the published MO:DCA reference render to PDF twins this engine's builder makes from the same content

**Doc:** `design/afp.md` (owed) · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-35.1 | design | Create `docs/design/afp.md`: MO:DCA's structured fields and document structure, the object architectures (text, graphics, images, barcodes, fonts), resource groups, page segments, overlays, form definitions and copy groups, and fixtures generated from the published reference against builder twins. | `docs/design/afp.md` (new) | The doc exists in the design-doc order, names FI-35.2 to FI-35.10's exit tests, and is reviewed. | — |
| FI-35.2 | build | Structured fields parsed, with the document, page and resource-group structure. | `crates/tinker-pdf/src/afp/mod.rs` (new), `crates/tinker-pdf/src/afp/fields.rs` (new), `crates/tinker-pdf/src/lib.rs` | Unit tests: streams generated from the reference parse to their structure. | FI-35.1 |
| FI-35.3 | build | Font resources: character sets, code pages and coded fonts, mapped to the engine's faces. | `crates/tinker-pdf/src/afp/fonts.rs` (new) | Unit tests: each code page maps to the characters the reference lists. | FI-35.2 |
| FI-35.4 | build | Presentation text with its control sequences. | `crates/tinker-pdf/src/afp/text.rs` (new), `crates/tinker-pdf/tests/afp.rs` (new) | `afp.rs` (new): analytic fixtures render to their builder-made PDF twins and extract their text. | FI-35.3 |
| FI-35.5 | build | Graphics drawing orders. | `crates/tinker-pdf/src/afp/graphics.rs` (new), `crates/tinker-pdf/tests/afp.rs` (new) | Analytic fixtures render to their PDF twins. | FI-35.2 |
| FI-35.6 | build | Image objects and their compressions. | `crates/tinker-pdf/src/afp/image.rs` (new), `crates/tinker-pdf/tests/afp.rs` (new) | Analytic fixtures render to their PDF twins. | FI-35.2 |
| FI-35.7 | build | Barcode objects drawn through ED-49's generators. | `crates/tinker-pdf/src/afp/barcode.rs` (new), `crates/tinker-pdf/tests/afp.rs` (new) | Analytic fixtures render to their PDF twins and the symbols decode through ED-50. | FI-35.2, ED-49, ED-50 |
| FI-35.8 | build | Page segments, overlays, form definitions with medium maps, and copy groups. | `crates/tinker-pdf/src/afp/pages.rs` (new), `crates/tinker-pdf/tests/afp.rs` (new) | Analytic fixtures paginate and overlay as their PDF twins. | FI-35.4, FI-35.5 |
| FI-35.9 | build | Resource groups inline and external, external ones through FI-27's provider. | `crates/tinker-pdf/src/afp/fields.rs` (new), `crates/tinker-pdf/tests/afp.rs` (new) | A stream whose resources are external renders as its inline twin. | FI-35.8, FI-27 |
| FI-35.10 | test | AFP sniffed and opened, fuzzed and fingerprinted. | `crates/tinker-pdf/src/standalone.rs`, `fuzz/fuzz_targets/afp.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md`, `crates/tinker-pdf/tests/determinism.rs` | `Document::open` opens an AFP stream; the fuzz target runs clean with the roster regenerated; the fingerprint holds on every K-07 target. | FI-35.6, FI-35.7, FI-35.9, K-07 |

**Closes the row:** FI-35.4 to FI-35.9 render streams generated from the published reference to their builder twins, which is the row's exit.

## FI-36 — EMF, EMF+ and WMF read

**EMF, EMF+ and WMF read** — as pictures inside office and RTF documents, and as documents

**Today.** absent — the owner's parity decision of 9 October 2026; FO-19 writes EMF and nothing reads it (`rg -i 'metafile' crates` finds one comment in `bmp.rs` and no reader)

**Exit criterion.** fixtures built from the record definitions of the published MS-EMF, MS-EMFPLUS and MS-WMF specifications render to PDF twins this engine's builder makes from the same drawing; every record type each specification defines is drawn or named by a typed warning

**Doc:** `design/metafiles.md` (owed) · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-36.1 | design | Create `docs/design/metafiles.md`: the GDI device context and its objects, mapping modes, world transforms and clipping, raster operations and which cannot be expressed, text and bitmaps, EMF+'s records and dual mode, WMF, and use as pictures and as documents. | `docs/design/metafiles.md` (new) | The doc exists in the design-doc order, names FI-36.2 to FI-36.7's exit tests, and is reviewed. | — |
| FI-36.2 | build | EMF records parsed and the device context's state machine. | `crates/tinker-pdf/src/metafile/mod.rs` (new), `crates/tinker-pdf/src/metafile/emf.rs` (new), `crates/tinker-pdf/src/lib.rs` | Unit tests: fixtures built from MS-EMF's record definitions parse to their records and states. | FI-36.1 |
| FI-36.3 | build | EMF drawing: paths, shapes, pens and brushes, mapping modes, world transform and clipping. | `crates/tinker-pdf/src/metafile/emf.rs` (new), `crates/tinker-pdf/tests/metafiles.rs` (new) | `metafiles.rs` (new): fixtures render to their builder-made PDF twins. | FI-36.2 |
| FI-36.4 | build | EMF text by `LOGFONT` mapped to the engine's faces, and bitmaps with the raster operations that can be expressed, the rest named by a typed warning. | `crates/tinker-pdf/src/metafile/emf.rs` (new), `crates/tinker-pdf/tests/metafiles.rs` (new) | Fixtures render to their PDF twins and extract their text; an inexpressible operation raises its warning. | FI-36.3 |
| FI-36.5 | build | WMF, with its placeable header, through the same drawing model. | `crates/tinker-pdf/src/metafile/wmf.rs` (new), `crates/tinker-pdf/tests/metafiles.rs` (new) | Fixtures built from MS-WMF's definitions render to their PDF twins. | FI-36.3 |
| FI-36.6 | build | EMF+ records: graphics state, brushes with gradients, pens, paths, images, text and transforms, and dual mode. | `crates/tinker-pdf/src/metafile/emfplus.rs` (new), `crates/tinker-pdf/tests/metafiles.rs` (new) | Fixtures built from MS-EMFPLUS's definitions render to their PDF twins. | FI-36.3 |
| FI-36.7 | test | Coverage of every record type each specification defines, drawn or named by a typed warning, and metafiles opened as documents. | `crates/tinker-pdf/tests/metafiles.rs` (new), `crates/tinker-pdf/src/standalone.rs`, `docs/features/opening.md` | A table test reaches every record type, each drawn or warned; `Document::open` opens EMF and WMF files. | FI-36.4, FI-36.5, FI-36.6 |

**Closes the row:** FI-36.3 to FI-36.6 render fixtures from the three specifications to their builder twins and FI-36.7 draws or names every record type, which is the row's exit.

## FI-37 — DXF drawings

**DXF drawings** as documents: entities, blocks, layers as optional content, line types, text and dimensions, and paper-space layouts as pages

**Today.** absent — the owner's parity decision of 9 October 2026; no CAD format is read

**Exit criterion.** fixtures built in the test from the published DXF reference's group codes render to PDF twins this engine's builder makes from the same geometry; each layer is an optional content group

**Doc:** `design/dxf.md` (owed) · **Depends on:** —

*Steps not yet reviewed; the review runs once all the work is done.*

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| FI-37.1 | design | Create `docs/design/dxf.md`: ASCII and binary group codes, sections and tables, each entity's drawing, blocks, line types and text styles mapped to the engine's faces, layers as optional content, and paper-space layouts as pages. | `docs/design/dxf.md` (new) | The doc exists in the design-doc order, names FI-37.2 to FI-37.7's exit tests, and is reviewed. | — |
| FI-37.2 | build | The group-code reader, ASCII and binary, with the sections and the layer, line-type, style and block-record tables. | `crates/tinker-pdf/src/dxf/mod.rs` (new), `crates/tinker-pdf/src/lib.rs` | Unit tests: fixtures built from the published reference's group codes parse to their tables and entities. | FI-37.1 |
| FI-37.3 | build | Geometric entities: lines, circles, arcs, ellipses, polylines with bulges, splines, solids and hatches. | `crates/tinker-pdf/src/dxf/entities.rs` (new), `crates/tinker-pdf/tests/dxf.rs` (new) | `dxf.rs` (new): fixtures render to their builder-made PDF twins. | FI-37.2 |
| FI-37.4 | build | Blocks and inserts, nested with attributes, line types and line weights. | `crates/tinker-pdf/src/dxf/entities.rs` (new), `crates/tinker-pdf/tests/dxf.rs` (new) | Fixtures render to their PDF twins. | FI-37.3 |
| FI-37.5 | build | Text, formatted multi-line text and dimensions. | `crates/tinker-pdf/src/dxf/text.rs` (new), `crates/tinker-pdf/tests/dxf.rs` (new) | Fixtures render to their PDF twins and extract their text, in both builds. | FI-37.3 |
| FI-37.6 | build | Layers as optional content groups, and paper-space layouts as pages with their viewports. | `crates/tinker-pdf/src/dxf/mod.rs` (new), `crates/tinker-pdf/tests/dxf.rs` (new) | Each layer is an optional content group; each layout is a page rendering as its twin. | FI-37.4, FI-37.5 |
| FI-37.7 | test | DXF sniffed and opened, fuzzed and fingerprinted. | `crates/tinker-pdf/src/standalone.rs`, `fuzz/fuzz_targets/dxf.rs` (new), `fuzz/Cargo.toml`, `docs/verification.md`, `crates/tinker-pdf/tests/determinism.rs` | `Document::open` opens a DXF drawing; the fuzz target runs clean with the roster regenerated; the fingerprint holds on every K-07 target. | FI-37.6, K-07 |

**Closes the row:** FI-37.3 to FI-37.6 render fixtures from the reference's group codes to their builder twins with each layer an optional content group, which is the row's exit.
