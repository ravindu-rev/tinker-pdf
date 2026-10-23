# OCR — the last implementation tier

[The roadmap](../ROADMAP.md) says how a step runs, what comes next, and which dependencies cross tiers. Each row below keeps its text as it was planned — Item, Today, Exit criterion, Doc, Depends on — and is cut into steps, each one workflow run.

The owner's instruction puts OCR in scope and last, as a later
implementation. Everything here is in-tree: preprocessing, layout analysis,
the recogniser and its models — built and trained here from data with known
licences, or vendored as data with provenance where the owner admits it
(H-39). Accuracy is never adjudicated by another recogniser (ruling 13):
first-party synthetic pages — known text rendered and degraded
deterministically — give a machine-checkable error rate in `cargo test`,
and real scans need ground truth a person transcribed (H-71). The import
seam (OC-01) is not recognition and may run ahead of the tier.

## OC-01 — A text-layer import seam

**A text-layer import seam**: hOCR, ALTO and PAGE XML from an outside recogniser written as an invisible text layer (render mode 3) in a glyph-less font fitted to the word boxes, with `/ToUnicode`

**Today.** absent — the owner's parity decision of 9 October 2026; until 9 October 2026 OCR was a decision item of the roadmap — "if ever, as a host seam like `FontProvider` and not an engine" — and "a scanned table is pixels" in the table design (`design/table-reconstruction.md:40-43`)

**Exit criterion.** imported layers extract as the imported text in the imported order; every word's box equals the file's; the page renders unchanged

**Doc:** `design/ocr.md` (owed) · **Depends on:** FT-24

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-02 — Searchable-PDF output

**Searchable-PDF output**: the original image untouched, the recognised text layered per page or region, PDF/A output on request

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** the layer's words fall inside their image boxes (counted in the test); PDF/A output passes SD-01

**Doc:** `design/ocr.md` (owed) · **Depends on:** OC-01

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-03 — Preprocessing

**Preprocessing**: binarisation (global, Sauvola, adaptive), despeckling, border and background removal, colour dropout, oversampling — integer pipelines, deterministic

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'sauvola|binari[sz]' crates tools` finds nothing

**Exit criterion.** each operation held to its formula computed in the test over generated images; determinism on four targets

**Doc:** `design/ocr.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-04 — Skew and geometry

**Skew and geometry**: skew detection and deskew with a border policy; document detection in photographs and perspective warp

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'deskew|skew_angle' crates tools` finds nothing

**Exit criterion.** generated pages rotated by known angles recover them within 0.1°; warped quadrilaterals map back within a pixel

**Doc:** `design/ocr.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-05 — Orientation and script detection

**Orientation and script detection** with an auto-rotate threshold

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'orientation_detect|script_detect' crates tools` finds nothing

**Exit criterion.** generated pages in each orientation and script classified correctly at the rate the design states

**Doc:** `design/ocr.md` (owed) · **Depends on:** OC-07

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-06 — Layout analysis for scans

**Layout analysis for scans**: blocks, columns, lines, words, tables and figures with reading order; single-column, single-block, single-line, single-word and sparse modes

**Today.** the born-digital reading-order and table inference exist (FT-30, FT-31)

**Exit criterion.** generated multi-column scan pages segment to their generated layout above the floors the design states

**Doc:** `design/scan-layout.md` (owed) · **Depends on:** FT-30, FT-31, OC-03

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-07 — The recognition runtime

**The recognition runtime**: deterministic inference in pure Rust (a line recogniser with sequence decoding), in fixed-point or pinned arithmetic so results are byte-identical on four targets; a versioned model format

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -iw 'ocr' crates` finds comments about other producers' OCR layers, and UCD data, and no recogniser

**Exit criterion.** a committed small model run over generated lines gives byte-identical outputs on four targets (K-07); inference counted-linear in line length (MS-01)

**Doc:** `design/ocr-inference.md` (owed) · **Depends on:** —

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-08a — Models, milestone 1 — Latin printed text

**Models, milestone 1 — Latin printed text**, and the reproducible training pipeline (`cargo xtask train`)

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** character error rate on first-party synthetic pages below the target the owner approves (H-71), ratcheted in `cargo test`; the training run reproduces the committed model's bytes

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-07, H-39, H-71

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-08b — Models, milestone 2 — Cyrillic and Greek

**Models, milestone 2 — Cyrillic and Greek**

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** as OC-08a per script

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-08c — Models, milestone 3 — Arabic and Hebrew

**Models, milestone 3 — Arabic and Hebrew** (right-to-left, joining, ruling 14's logical order)

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** as OC-08a; recognised text extracts in logical order

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-08d — Models, milestone 4 — Indic scripts

**Models, milestone 4 — Indic scripts**

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** as OC-08a

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-08e — Models, milestone 5 — Chinese, Japanese and Korean

**Models, milestone 5 — Chinese, Japanese and Korean**, horizontal and vertical

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** as OC-08a

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-09 — Language resources

**Language resources**: dictionaries, user words and patterns, per-word confidence

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** a dictionary corrects planted near-misses on synthetic pages; confidence calibrated on them

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-10 — OCR output formats

**OCR output formats**: hOCR, ALTO, PAGE XML, TSV, text, and the engine's structured-text formats

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'hocr|\balto\b|page_xml' crates tools` finds nothing

**Exit criterion.** each validated against its published schema or definition (shared writers with FT-24)

**Doc:** — · **Depends on:** FT-24, OC-08a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-11 — Workflow modes

**Workflow modes**: skip pages with text, force, redo (replace an old OCR layer and keep born-digital text — ED-36f), strip; page selection; a sidecar text file; a policy for signed documents; OCR of the image regions only of a born-digital page, its text runs kept; per-page budgets; parallel pages; `tpdf ocr`

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** each mode held to mixed born-digital and scanned fixtures, the image-regions mode leaving every born-digital run unchanged; a signed document refused or annotated as the policy states; `tpdf ocr` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** OC-02, P-15

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-12 — Scan compression

**Scan compression**: mixed raster content (mask, foreground and background layers), JPEG 2000 backgrounds, palette quantisation, lossy JBIG2 symbol coding with refinement

**Today.** lossless JBIG2 symbol coding is ED-27; absent otherwise

**Exit criterion.** compressed scans render within the tolerance the design states of their sources; text masks stay lossless

**Doc:** `design/mrc.md` (owed) · **Depends on:** ED-26, ED-27

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-13 — Glyph-level recognition for unmapped fonts

**Glyph-level recognition for unmapped fonts**, writing a corrected `/ToUnicode`

**Today.** `TextWarning::UnmappedCode`; FT-35 handles faces with known matches

**Exit criterion.** fixtures with stripped `cmap`s and unknown faces recover their text

**Doc:** — · **Depends on:** OC-08a, FT-35

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-14 — Vectorised scan text

**Vectorised scan text**: recognised words redrawn in synthesised vector glyphs matching their shapes

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** rendered output within the stated tolerance of the scan; text extracts as recognised

**Doc:** `design/ocr-vector.md` (owed) · **Depends on:** OC-08a

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-15a — Tables in images

**Tables in images**: table structure recovered from scanned pages

**Today.** "a scanned table is pixels" (`design/table-reconstruction.md:40-43`)

**Exit criterion.** generated scanned tables recover their grids

**Doc:** `design/scan-layout.md` (owed) · **Depends on:** OC-06

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-15b — Marks on scanned forms

**Marks on scanned forms**: checkbox and bubble states, signature presence, patch-code and barcode separators

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** generated forms with known marks read correctly

**Doc:** — · **Depends on:** OC-06, ED-50

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-15c — Handwriting recognition

**Handwriting recognition**

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** first-party synthetic handwriting and person-transcribed samples (H-71) below the approved error rate

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a, H-71

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-15d — Mathematics recognition

**Mathematics recognition** to MathML and LaTeX

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists (OC-07)

**Exit criterion.** generated formula images recover their MathML

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a, FT-34

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-16 — Alternate-text suggestions

**Alternate-text suggestions** for figures, on-device, flagged for a person's review

**Today.** absent — the owner's parity decision of 9 October 2026; `rg -i 'alt_text_suggest|caption_model' crates tools` finds nothing

**Exit criterion.** suggestions produced deterministically for the committed figures and never written as final text without a caller's assertion

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-07

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*

## OC-17 — The OCR accuracy and speed ratchet

**The OCR accuracy and speed ratchet**: character and word error rates per script on first-party synthetic pages, and pages per second per script, in the benchmark set

**Today.** absent — the owner's parity decision of 9 October 2026; no recogniser exists to measure

**Exit criterion.** the rates and speeds recorded in `cargo test` and `baseline.json` and held as floors; **on clean synthetic 300-dpi Latin pages a character error rate ≤ 1 % and a word error rate ≤ 3 %, ≤ 3 % and ≤ 8 % on the degraded set, and a page in ≤ 1.5 s on one thread; Chinese pages ≤ 2 % characters wrong clean and ≤ 4 s a page**; each other script's numbers stated in its own milestone row (OC-08b…OC-08e) when it lands; the error targets stand until H-71 re-states them, the speeds gate as calibrated (tier P)

**Doc:** — · **Depends on:** OC-08a, MS-04

*Not yet cut into steps. It is cut before work on it starts ([How a step runs](../ROADMAP.md#how-a-step-runs), 1).*
