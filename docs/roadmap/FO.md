# Capability tier — formats out and print production

[The roadmap](../ROADMAP.md) says how a step runs, what comes next, and which dependencies cross tiers. Each row below keeps its text as it was planned — Item, Today, Exit criterion, Doc, Depends on — and is cut into steps, each one workflow run.

Parity here means every raster and vector output a host asks for, the print
streams a printer takes, the container and office formats a reader takes
away, and the print-production work — flattening, separations, trapping,
preflight — that the standards tier's conversions need.

## FO-01 — Netpbm output

**Netpbm output**: PBM (halftoned 1-bit), PGM, PPM, PAM with alpha and CMYK, and PKM (1-bit CMYK)

**Today.** PNG only (`Bitmap::to_png`)

**Exit criterion.** each file's samples equal the bitmap's; headers held to the format description

**Doc:** — · **Depends on:** RC-06

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-02 — JPEG raster output

**JPEG raster output** and image-to-buffer helpers (data URIs)

**Today.** absent — the owner's parity decision of 9 October 2026; `Bitmap` offers `to_png` only (`crates/tinker-pdf/src/lib.rs:831`); the baseline JPEG encoder exists and only image recompression calls it (`crates/tinker-pdf/src/recode.rs:1277`)

**Exit criterion.** outputs decode through the in-tree decoder within the bound computed from the caller's tables

**Doc:** — · **Depends on:** ED-28

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-03 — TIFF output

**TIFF output**: multi-page; none, PackBits, LZW, Deflate, JPEG, G3 1D and 2D, G4; CMYK and one plane per ink; BigTIFF; resolution and ICC tags; fax class F

**Today.** a TIFF reader and a G4 encoder exist

**Exit criterion.** every file re-read by the in-tree TIFF reader yields the samples written; a class F file carries the tags and values TIFF Class F's description requires

**Doc:** — · **Depends on:** RC-04

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-04 — PSD output

**PSD output** with spot channels

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bpsd\b' crates tools` finds nothing

**Exit criterion.** channels equal the separations rendered (RC-04)

**Doc:** — · **Depends on:** RC-04

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-05 — PNG options

**PNG options**: 16-bit, an embedded ICC profile, physical resolution, the fast level (P-04)

**Today.** 8-bit PNG without colour or resolution chunks

**Exit criterion.** each chunk re-read; 16-bit samples equal the RC-07 render

**Doc:** — · **Depends on:** P-04, RC-07

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-06 — JPEG 2000 raster output

**JPEG 2000 raster output**

**Today.** absent — the owner's parity decision of 9 October 2026; no JPEG 2000 encoder exists (ED-26)

**Exit criterion.** outputs decode within the bound (lossless exactly)

**Doc:** — · **Depends on:** ED-26

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-07a — A WebP encoder

**A WebP encoder**, lossless and lossy

**Today.** absent — the owner's parity decision of 9 October 2026; WebP is read only (`features/filters.md`)

**Exit criterion.** lossless outputs decode through the in-tree decoder to the samples written; lossy within a bound computed in the test

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-07b — An AVIF encoder

**An AVIF encoder** (AV1 intra)

**Today.** absent — the owner's parity decision of 9 October 2026; AVIF is not even read yet (FI-08)

**Exit criterion.** outputs decode through FI-08's decoder within a bound computed in the test, lossless exactly

**Doc:** `design/avif.md` (owed) · **Depends on:** FI-08

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-07c — A JPEG XL encoder

**A JPEG XL encoder**, modular (lossless) and VarDCT

**Today.** absent — the owner's parity decision of 9 October 2026; JPEG XL is not read yet (FI-07a)

**Exit criterion.** lossless outputs decode through FI-07a's decoder to the samples written; VarDCT within a bound computed in the test

**Doc:** `design/jpeg-xl.md` (owed) · **Depends on:** FI-07b

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-08a — Raster PostScript output

**Raster PostScript output**, banded

**Today.** absent — the owner's parity decision of 9 October 2026; a decision row of the roadmap until 9 October 2026, on the argument that print pipelines are the only consumer

**Exit criterion.** the output interpreted by FI-17a's interpreter reproduces the bitmap

**Doc:** — · **Depends on:** FI-17a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-08b — Vector PostScript Level 2 and 3 and EPS

**Vector PostScript Level 2 and 3 and EPS**: DSC-conforming; fonts as Type 42, Type 1, CFF and CID with CMaps; smooth shading with a Level 2 fallback; masked images; separations; `setpagedevice` paper and duplex; OPI comments for images

**Today.** as FO-08a

**Exit criterion.** every fingerprint fixture written and interpreted by FI-17b's interpreter renders byte-identically to the PDF; DSC and OPI comments held to the conventions' text

**Doc:** `design/postscript-output.md` (owed) · **Depends on:** FO-10, FI-17b

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-09a — PCL raster output

**PCL raster output**, monochrome and colour, with the printer presets and options a driver needs

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bpcl\b' crates tools` finds nothing

**Exit criterion.** the byte stream re-read by FI-18a's interpreter reproduces the bitmap

**Doc:** — · **Depends on:** FI-18a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-09b — PCLm output

**PCLm output** (compression, strip height)

**Today.** absent — the owner's parity decision of 9 October 2026; PCLm appears only as two corpus files' names in raster comments

**Exit criterion.** the output re-opened as PDF yields the bitmap strip by strip

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-09c — PWG raster output

**PWG raster output** with every header field settable

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\bpwg\b' crates tools` finds nothing

**Exit criterion.** headers and pixels re-read by a first-party parser in the test

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-09d — URF raster output

**URF raster output**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\burf\b' crates tools` finds nothing

**Exit criterion.** as FO-09c

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-09e — PCL XL vector output

**PCL XL vector output**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'pclxl|pcl.xl' crates tools` finds nothing

**Exit criterion.** re-read by FI-18b's interpreter to the same pixels

**Doc:** — · **Depends on:** FI-18b

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-10 — Transparency flattening

**Transparency flattening**: atomic regions, complex areas rasterised at a set resolution, vector text kept where possible, an outline-text option, overprint-aware; a transparency-free PDF out

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'flatten_transparency|transparency_flatten' crates tools` finds nothing

**Exit criterion.** flattened pages render within a stated tolerance of the original (computed per pixel in the test) and contain no transparency; the PDF/A-1 and PDF/X-1a transparency rules pass

**Doc:** `design/flattening.md` (owed) · **Depends on:** RC-03

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-11 — A CBZ writer

**A CBZ writer** (pages rasterised into a comic archive) and the ZIP writer under it

**Today.** container writing was a decision item of the roadmap until 9 October 2026 — CBZ, XPS and EPUB were read-only conversions; `tinker-pdf-zip` reads only

**Exit criterion.** the written archive re-opens through the CBZ reader to the same pixels

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-12a — An XPS writer

**An XPS writer**

**Today.** absent — the owner's parity decision of 9 October 2026; the same decision item as FO-11

**Exit criterion.** written packages re-open through the XPS reader and render byte-identically to the source

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-12b — An EPUB writer

**An EPUB writer** from structured or tagged content

**Today.** absent — the owner's parity decision of 9 October 2026; the same decision item as FO-11 (tagged EPUB is read and written into PDF, never out of it)

**Exit criterion.** a tagged committed document written as EPUB re-opens through the EPUB reader with its text, order and images

**Doc:** `design/epub-writer.md` (owed) · **Depends on:** FT-30

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-12c — tar and 7z writers

**tar and 7z writers** for archive output

**Today.** a non-goal of the archive design until 9 October 2026 (`design/comic-archives.md:49-50`)

**Exit criterion.** archives re-open through the readers byte for byte

**Doc:** — · **Depends on:** FO-11

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-13 — SVG output, complete

**SVG output, complete**: soft masks, non-Normal blend modes and knockout groups expressed rather than refused; shadings and patterns as SVG gradients and patterns where exact; text as text or as paths; images reused

**Today.** `SvgWarning::SoftMaskRefused`, `BlendModeRefused`, `KnockoutRefused`, `Rasterised` (`features/rendering.md:737-738`)

**Exit criterion.** the SVG re-opened through the in-tree SVG reader renders within the stated tolerance of the page for every fingerprint fixture

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-14a — DOCX export

**DOCX export**: paragraphs and styles, tables, headers and footers, images, links

**Today.** absent — the owner's parity decision of 9 October 2026; as FI-02b; nothing writes an office format

**Exit criterion.** exported documents re-read by FI-02a yield the source's text, structure and images

**Doc:** `design/office-export.md` (owed) · **Depends on:** FT-30, FT-31, FI-02a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-14b — ODT export

**ODT export**

**Today.** absent — the owner's parity decision of 9 October 2026; ODF appears only as the EPUB container's namespace (`crates/tinker-pdf/src/epub/ocf.rs:105`)

**Exit criterion.** as FO-14a

**Doc:** `design/office-export.md` (owed) · **Depends on:** FO-14a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-14c — XLSX export

**XLSX export** with typed cells and tables stitched across pages

**Today.** absent — the owner's parity decision of 9 October 2026; nothing writes an office format (FO-14a); a spreadsheet's media type appears only in a test's associated file (`crates/tinker-pdf/tests/pdf20.rs:345`)

**Exit criterion.** exported tables re-read to the reconstructed grid

**Doc:** `design/office-export.md` (owed) · **Depends on:** FT-32

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-14d — PPTX export

**PPTX export**, a text box per block per slide

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'pptx|presentationml' crates tools` finds nothing

**Exit criterion.** re-read to the same blocks

**Doc:** `design/office-export.md` (owed) · **Depends on:** FO-14a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-14e — RTF export

**RTF export**

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i '\brtf\b' crates tools` finds nothing

**Exit criterion.** re-read by FI-02g

**Doc:** — · **Depends on:** FI-02g

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-15a — High-fidelity HTML

**High-fidelity HTML**: absolutely positioned selectable text in embedded web fonts (WOFF and WOFF2 written with corrected `cmap`s), vector art as SVG or background images, per page or one file, the outline as navigation

**Today.** structured text as HTML (`TextWriter`)

**Exit criterion.** the HTML re-laid out by the engine's own HTML path renders within the stated tolerance of the page; every glyph's text extracts

**Doc:** `design/html-export.md` (owed) · **Depends on:** FO-13

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-15b — Accessible HTML from tagged PDF

**Accessible HTML from tagged PDF**: standard structure types mapped to HTML, attributes to CSS, alternate text, MathML, table headers

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'accessible_html|structure_to_html' crates tools` finds nothing

**Exit criterion.** each committed tagged document's HTML re-read yields the structure tree's order, headings, lists and tables

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-15c — Hybrid HTML

**Hybrid HTML**: graphics as a background image, text over it

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'hybrid_html' crates tools` finds nothing

**Exit criterion.** text positions equal the glyph boxes

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-16 — Device trace and diagnostics output

**Device trace and diagnostics output**: every device call with parameters as XML, a bounding-box log, glyph-detail XML

**Today.** `display_list` in Rust only

**Exit criterion.** the trace re-parsed equals the recorded events; `tpdf trace` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-17 — A uniform document-writer API and option grammar

**A uniform document-writer API and option grammar**: one page-by-page writer chosen by format, with typed options and an option-string grammar for the CLI and bindings (unknown options reported)

**Today.** each output has its own call

**Exit criterion.** every output format reachable through one writer; option strings round-trip to typed options; an unknown option is a typed error; `tpdf convert` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-18 — A Brotli encoder

**A Brotli encoder** for `/BrotliDecode` streams on write

**Today.** absent — the owner's parity decision of 9 October 2026; the Brotli module decodes only (`crates/tinker-pdf-filters/src/brotli.rs` has no `compress` or `encode`)

**Exit criterion.** encoder outputs decode through RFC 7932's decoder path to their input; savings recorded

**Doc:** — · **Depends on:** FI-15

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-19 — EMF and EMF+ output

**EMF and EMF+ output** (Windows metafiles)

**Today.** absent — the owner's parity decision of 9 October 2026; no metafile writer exists (`rg -i 'metafile' crates` finds one comment in `bmp.rs`)

**Exit criterion.** records held to the published specifications; every written file re-read through FI-36's reader to the same drawing

**Doc:** — · **Depends on:** FI-36

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-20 — A JPEG XR encoder

**A JPEG XR encoder**

**Today.** absent — the owner's parity decision of 9 October 2026; a named non-goal of the roadmap until 9 October 2026 — a scope choice under ruling 8 and never a limit, unlike JPEG XR's unadjudicated list (NG-06) ([design/jpeg-xr.md](../design/jpeg-xr.md))

**Exit criterion.** outputs decode through the in-tree decoder; lossless exactly

**Doc:** — · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-21 — A print-layout API

**A print-layout API**: scaling (none, fit, shrink, custom), auto-rotate and centre, tiling oversized pages, N-up and booklets, duplex and collation, ranges, annotations and comment summaries included or not, `/ViewerPreferences` print keys honoured, print-as-image fallback, reverse order

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'print_layout|PrintOptions|PrintParams' crates tools` finds nothing

**Exit criterion.** each setting's placement computed in the test; viewer-preference keys read and applied

**Doc:** — · **Depends on:** ED-08

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-22 — Print-intent rendering and printer marks

**Print-intent rendering and printer marks**: crop, bleed and registration marks, colour bars, page information, hairline fix-up

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'crop_marks|registration_marks|colou?r_bars' crates tools` finds nothing

**Exit criterion.** marks placed at the geometry computed from the boxes; hairlines at the stated minimum

**Doc:** — · **Depends on:** RC-13

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-23 — Ink coverage and total-area-coverage analysis

**Ink coverage and total-area-coverage analysis**: per page and plate, a heat map and its maximum, spot and overprint inventories

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'ink_coverage|total_area_coverage' crates tools` finds nothing

**Exit criterion.** coverage equals the plates' sums computed in the test

**Doc:** — · **Depends on:** RC-04

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-24 — Trapping

**Trapping**: raster trapping on separations and vector `/TrapNet` generation from presets

**Today.** `TrapNet` annotations are read; no trapping

**Exit criterion.** traps placed where the colour-pair rules computed in the test require

**Doc:** `design/trapping.md` (owed) · **Depends on:** RC-04

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-25 — Processing steps

**Processing steps** (ISO 19593-1) on optional content groups, read and written

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'processing_?steps|iso.?19593' crates tools` finds nothing

**Exit criterion.** fixtures per step type round-trip

**Doc:** — · **Depends on:** H-102

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-26 — Spectral spot-colour data

**Spectral spot-colour data** (CxF/X-4, ISO 17972-4), read, written and used for spot rendering

**Today.** absent — the owner's parity decision of 9 October 2026; "spectral" appears only as JPEG's spectral selection (`crates/tinker-pdf-filters/src/jpeg.rs`); `rg -i 'cxf' crates tools` finds nothing

**Exit criterion.** spectral-to-Lab computed in the test from the CIE tables

**Doc:** — · **Depends on:** RC-04, H-102

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## FO-27 — A preflight and fix-up framework

**A preflight and fix-up framework**: profiles of checks (PDF/A, PDF/X, PDF/UA, custom), fix-ups (convert colours, embed fonts, set boxes, remove objects, flatten, downsample), an inventory (fonts, images with effective resolution, colour spaces, overprint, transparency), and a report document with highlights

**Today.** validators report typed findings

**Exit criterion.** each fix-up removes the finding it targets on fixtures built to carry it, with no new finding

**Doc:** `design/preflight.md` (owed) · **Depends on:** SD-02, SD-12, ED-06b

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*
