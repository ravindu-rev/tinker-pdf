# OCR — the last implementation tier

[The roadmap](../ROADMAP.md) says how a step is worked and what comes next.

OCR is in scope, and last. Everything here is in-tree: preprocessing, layout analysis,
the recogniser and its models — built and trained here from data with known
licences, or vendored as data with provenance where the owner admits it
(H-39). Accuracy is never adjudicated by another recogniser (ruling 13):
first-party synthetic pages — known text rendered and degraded
deterministically — give a machine-checkable error rate in `cargo test`,
and real scans need ground truth a person transcribed (H-71). The import
seam (OC-01) is not recognition and may run ahead of the tier.

## OC-01 — A text-layer import seam

**A text-layer import seam**: hOCR, ALTO and PAGE XML from an outside recogniser written as an invisible text layer (render mode 3) in a glyph-less font fitted to the word boxes, with `/ToUnicode`

**Exit criterion.** imported layers extract as the imported text in the imported order; every word's box equals the file's; the page renders unchanged

**Doc:** `design/ocr.md` (owed) · **Depends on:** FT-24

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-01.1 | design | Create `docs/design/ocr.md`: the tier's in-tree pipeline, the import seam, searchable output, preprocessing, geometry, orientation and script; the glyph-less font fitted to word boxes; first-party synthetic pages as the adjudicator under ruling 13, real scans only with ground truth a person transcribed (H-71); milestones with exit tests. | `docs/design/ocr.md` (new) | The doc exists in the design-doc order, names OC-01.2 and OC-01.3's exit tests. | — |
| OC-01.2 | build | hOCR, ALTO and PAGE XML read from an outside recogniser into FT-24's word-box model: words, boxes, lines, blocks, order and confidence. | `crates/tinker-pdf/src/ocr/import.rs` (new), `crates/tinker-pdf/src/ocr/mod.rs` (new), `crates/tinker-pdf/src/lib.rs`, `crates/tinker-pdf/tests/ocr_import.rs` (new) | `ocr_import.rs` (new): a hand-written file of each format reads to the words, boxes and order it states. | OC-01.1, FT-24.3 |
| OC-01.3 | build | The imported words written as an invisible text layer, render mode 3, in a glyph-less font whose advances are fitted to each word's box, with `/ToUnicode`. | `crates/tinker-pdf/src/ocr/layer.rs` (new), `crates/tinker-pdf/tests/ocr_import.rs` (new) | The layer extracts as the imported text in the imported order, every word's box equals the file's, and the page renders to the same pixels as before. | OC-01.2 |
| OC-01.4 | build | Create the crate `tinker-pdf-ocr`: a workspace member with a `[workspace.dependencies]` entry and `#![forbid(unsafe_code)]`, declared in xtask's `ALLOWED` with the edges `docs/design/ocr.md` gives it, added to the libm check's `PIXEL_PATHS` since its arithmetic is pinned, and `docs/architecture.md` amended (the crate count, the graph, the per-crate map). *Note:* As FJ-01.2 creates `tinker-pdf-script`. The crate serves the tier, not this row's exit; every step that adds a file to it waits on this one. | `Cargo.toml`, `crates/tinker-pdf-ocr/Cargo.toml` (new), `crates/tinker-pdf-ocr/src/lib.rs` (new), `xtask/src/main.rs`, `docs/architecture.md` | `cargo xtask check` is green (dag, libm, versions) with the crate in it, and `cargo test -p tinker-pdf-ocr` runs. | OC-01.1 |

## OC-02 — Searchable-PDF output

**Searchable-PDF output**: the original image untouched, the recognised text layered per page or region, PDF/A output on request

**Exit criterion.** the layer's words fall inside their image boxes (counted in the test); PDF/A output passes SD-01

**Doc:** `design/ocr.md` (owed) · **Depends on:** OC-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-02.1 | design | Add searchable-PDF output to `docs/design/ocr.md`: the image kept byte for byte, the layer per page or per region, PDF/A on request. | `docs/design/ocr.md` (new) | The section exists, names OC-02.2 and OC-02.3's exit tests. | OC-01.1 |
| OC-02.2 | build | Searchable output: the recognised or imported words layered over the original image, per page or region, through OC-01.3's layer. | `crates/tinker-pdf/src/ocr/layer.rs` (new), `crates/tinker-pdf/tests/ocr_searchable.rs` (new), `docs/features/ocr.md` (new), `docs/README.md` | `ocr_searchable.rs` (new): every word falls inside its image box, counted in the test, and the image stream is byte-identical to the source's. | OC-02.1, OC-01.3 |
| OC-02.3 | build | PDF/A output on request, the layer carried under the archival profile. | `crates/tinker-pdf/src/ocr/layer.rs` (new), `crates/tinker-pdf/tests/ocr_searchable.rs` (new) | Searchable PDF/A output passes the PDF/A validator at its level with zero findings. | OC-02.2 |

## OC-03 — Preprocessing

**Preprocessing**: binarisation (global, Sauvola, adaptive), despeckling, border and background removal, colour dropout, oversampling — integer pipelines, deterministic

**Exit criterion.** each operation held to its formula computed in the test over generated images; determinism on four targets

**Doc:** `design/ocr.md` (owed) · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-03.1 | design | Add preprocessing to `docs/design/ocr.md`: each operation's formula in integer arithmetic, the synthetic page generator the tier tests against, and determinism across targets. | `docs/design/ocr.md` (new) | The section exists, names OC-03.2 to OC-03.6's exit tests. | OC-01.1 |
| OC-03.2 | test | The synthetic page generator: known text and layouts rendered through the engine and degraded deterministically, with noise, blur, skew, warp and ink spread, from a seed. | `crates/tinker-pdf/src/ocr/synth.rs` (new), `crates/tinker-pdf/tests/ocr_synth.rs` (new) | `ocr_synth.rs` (new): a page from a seed is byte-identical across runs, and its ground truth lists every word with its box; in the default and `bundled-fonts` builds. | OC-03.1, OC-01.2 |
| OC-03.3 | build | Binarisation in integer arithmetic: global, Sauvola and adaptive. | `crates/tinker-pdf-ocr/src/preprocess.rs` (new), `crates/tinker-pdf-ocr/src/lib.rs` (new), `crates/tinker-pdf-ocr/tests/preprocess.rs` (new) | `preprocess.rs` (new): each method's output equals its formula computed in the test over generated images. | OC-03.2, OC-01.4 |
| OC-03.4 | build | Despeckling, border and background removal, and colour dropout. | `crates/tinker-pdf-ocr/src/preprocess.rs` (new), `crates/tinker-pdf-ocr/tests/preprocess.rs` (new) | Each operation's output equals its formula computed in the test over generated images. | OC-03.3 |
| OC-03.5 | build | Oversampling in integer arithmetic. | `crates/tinker-pdf-ocr/src/preprocess.rs` (new), `crates/tinker-pdf-ocr/tests/preprocess.rs` (new) | The output equals the formula computed in the test. | OC-03.3 |
| OC-03.6 | ci | The preprocessing outputs hashed on four targets in CI. | `.github/workflows/ci.yml`, `crates/tinker-pdf-ocr/tests/preprocess.rs` (new) | On a CI run on `develop` every operation's output hash is identical on the four targets. | OC-03.4, OC-03.5, MS-11 |

## OC-04 — Skew and geometry

**Skew and geometry**: skew detection and deskew with a border policy; document detection in photographs and perspective warp

**Exit criterion.** generated pages rotated by known angles recover them within 0.1°; warped quadrilaterals map back within a pixel

**Doc:** `design/ocr.md` (owed) · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-04.1 | design | Add skew and geometry to `docs/design/ocr.md`: the skew estimator, the border policy, document detection in photographs and the perspective warp. | `docs/design/ocr.md` (new) | The section exists, names OC-04.2 and OC-04.3's exit tests. | OC-01.1 |
| OC-04.2 | build | Skew detection and deskew with a border policy. | `crates/tinker-pdf-ocr/src/geometry.rs` (new), `crates/tinker-pdf-ocr/tests/geometry.rs` (new) | `geometry.rs` (new): generated pages rotated by known angles recover them within 0.1°, the border filled as the policy says. | OC-04.1, OC-03.2, OC-01.4 |
| OC-04.3 | build | Document detection in photographs and the perspective warp back to a flat page. | `crates/tinker-pdf-ocr/src/geometry.rs` (new), `crates/tinker-pdf-ocr/tests/geometry.rs` (new) | Generated warped quadrilaterals map back within a pixel. | OC-04.2 |

## OC-05 — Orientation and script detection

**Orientation and script detection** with an auto-rotate threshold

**Exit criterion.** generated pages in each orientation and script classified correctly at the rate the design states

**Doc:** `design/ocr.md` (owed) · **Depends on:** OC-07

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-05.1 | design | Add orientation and script detection to `docs/design/ocr.md`: the classifier, its model, the rates it must reach, and the auto-rotate threshold. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `docs/design/ocr.md` (new) | The section exists, states the rates, names OC-05.2 and OC-05.3's exit tests. | OC-01.1 |
| OC-05.2 | build | Orientation detection on the recognition runtime, with an auto-rotate threshold, its model trained by `cargo xtask train`. | `crates/tinker-pdf-ocr/src/orientation.rs` (new), `crates/tinker-pdf-ocr/tests/orientation.rs` (new) | `orientation.rs` (new): generated pages in each orientation are classified at the rate the design states; below the threshold nothing rotates. | OC-05.1, OC-07, OC-08a.2 |
| OC-05.3 | build | Script detection per page and block over the scripts the models cover. | `crates/tinker-pdf-ocr/src/orientation.rs` (new), `crates/tinker-pdf-ocr/tests/orientation.rs` (new) | Generated pages in each script are classified at the rate the design states. | OC-05.2 |

## OC-06 — Layout analysis for scans

**Layout analysis for scans**: blocks, columns, lines, words, tables and figures with reading order; single-column, single-block, single-line, single-word and sparse modes

**Exit criterion.** generated multi-column scan pages segment to their generated layout above the floors the design states

**Doc:** `design/scan-layout.md` (owed) · **Depends on:** FT-30, FT-31, OC-03

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-06.1 | design | Create `docs/design/scan-layout.md`: segmentation of binarised pages into blocks, columns, lines and words; table and figure regions; reading order through FT-30's model; the single-column, single-block, single-line, single-word and sparse modes; generated pages and the floors they must clear; milestones. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `docs/design/scan-layout.md` (new) | The doc exists in the design-doc order, states the floors, names OC-06.2 to OC-06.6's exit tests. | — |
| OC-06.2 | build | Connected components and the line finder on binarised pages. | `crates/tinker-pdf-ocr/src/layout.rs` (new), `crates/tinker-pdf-ocr/tests/scan_layout.rs` (new) | `scan_layout.rs` (new): generated pages' lines are found with the boxes the generator records. | OC-06.1, OC-03.3 |
| OC-06.3 | build | Words and lines grouped into blocks and columns, ordered through FT-30's reading-order inference. | `crates/tinker-pdf-ocr/src/layout.rs` (new), `crates/tinker-pdf-ocr/tests/scan_layout.rs` (new) | Generated multi-column pages segment to their blocks and order above the stated floor. | OC-06.2, FT-30 |
| OC-06.4 | build | Table and figure regions told apart from text. | `crates/tinker-pdf-ocr/src/layout.rs` (new), `crates/tinker-pdf-ocr/tests/scan_layout.rs` (new) | Generated pages' table and figure regions are found above the stated floor. | OC-06.3, FT-31 |
| OC-06.5 | build | The segmentation modes: single column, single block, single line, single word, and sparse text. | `crates/tinker-pdf-ocr/src/layout.rs` (new), `crates/tinker-pdf-ocr/tests/scan_layout.rs` (new) | Each mode segments its generated pages as the design states. | OC-06.3 |
| OC-06.6 | test | The generated multi-column scan set run whole against the design's floors. | `crates/tinker-pdf-ocr/tests/scan_layout.rs` (new) | Every generated page segments to its generated layout above the floors, recorded as a ratchet. | OC-06.4, OC-06.5 |

## OC-07 — The recognition runtime

**The recognition runtime**: deterministic inference in pure Rust (a line recogniser with sequence decoding), in fixed-point or pinned arithmetic so results are byte-identical on four targets; a versioned model format

**Exit criterion.** a committed small model run over generated lines gives byte-identical outputs on four targets (K-07); inference counted-linear in line length (MS-01)

**Doc:** `design/ocr-inference.md` (owed) · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-07.1 | design | Create `docs/design/ocr-inference.md`: the line recogniser and its sequence decoding, fixed-point or pinned arithmetic so outputs are byte-identical on four targets, the versioned model format, inference counted-linear in line length, and milestones with exit tests. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `docs/design/ocr-inference.md` (new) | The doc exists in the design-doc order, names OC-07.2 to OC-07.8's exit tests. | — |
| OC-07.2 | build | The versioned model format: tensors, quantisation scales and metadata, read within bounds, an unknown version refused. | `crates/tinker-pdf-ocr/src/model.rs` (new), `crates/tinker-pdf-ocr/tests/model.rs` (new), `crates/tinker-pdf/tests/bounds_ledger.rs` | `model.rs` (new): a model written and read back is equal; truncated and oversized models are refused by name; the bounds are ledger rows. | OC-07.1, OC-01.4 |
| OC-07.3 | build | The kernels in the design's arithmetic: convolution, matrix product, activations through `tinker-pdf-math`'s functions, and normalisation. | `crates/tinker-pdf-ocr/src/kernels.rs` (new), `crates/tinker-pdf-ocr/tests/kernels.rs` (new) | `kernels.rs` (new): each kernel's output equals the value computed in the test, bit for bit. | OC-07.2 |
| OC-07.4 | build | The sequence layers the design names, on those kernels. | `crates/tinker-pdf-ocr/src/sequence.rs` (new), `crates/tinker-pdf-ocr/tests/kernels.rs` (new) | Each layer's output on fixed inputs equals the value computed in the test. | OC-07.3 |
| OC-07.5 | build | Sequence decoding, greedy and beam, with deterministic tie-breaking, to characters with positions and confidences. | `crates/tinker-pdf-ocr/src/decode.rs` (new), `crates/tinker-pdf-ocr/tests/decode.rs` (new) | `decode.rs` (new): decoded outputs on fixed score matrices equal those computed in the test, ties resolved as the design states. | OC-07.4 |
| OC-07.6 | build | Line images normalised into the model's input: height, padding and polarity. | `crates/tinker-pdf-ocr/src/normalise.rs` (new), `crates/tinker-pdf-ocr/tests/normalise.rs` (new) | Normalised inputs equal those computed in the test from generated lines. | OC-07.3 |
| OC-07.7 | ci | A committed small model run over generated lines on four targets, under K-07. | `crates/tinker-pdf-ocr/models` (new), `crates/tinker-pdf-ocr/tests/decode.rs` (new), `.github/workflows/ci.yml` | On a CI run on `develop` the outputs are byte-identical on the four targets. | OC-07.5, OC-07.6, K-07.1, MS-11 |
| OC-07.8 | measure | Inference work counted against line length through MS-01's tally. | `crates/tinker-pdf-ocr/tests/decode.rs` (new), `docs/design/ocr-inference.md` (new) | The counted work grows linearly in line length over generated lines, asserted with the slope recorded. | OC-07.5, MS-01.8 |

## OC-08a — Models, milestone 1 — Latin printed text

**Models, milestone 1 — Latin printed text**, and the reproducible training pipeline (`cargo xtask train`)

**Exit criterion.** character error rate on first-party synthetic pages below the target the owner approves (H-71), ratcheted in `cargo test`; the training run reproduces the committed model's bytes

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-07, H-39, H-71

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-08a.1 | design | Create `docs/design/ocr-models.md`: training data rendered here or under licences H-39 admits, the faces used, the architecture on OC-07's runtime, the reproducible `cargo xtask train`, evaluation on first-party synthetic pages and on transcribed scans (H-71), and a milestone per script with OC-09's language resources and the handwriting, mathematics and alternate-text models. *Note:* Latin first, then Cyrillic and Greek, Arabic and Hebrew, Indic scripts and CJK, with handwriting last: H-38's default order. | `docs/design/ocr-models.md` (new) | The doc exists in the design-doc order, names OC-08a.2 to OC-08a.6's exit tests. | H-39 |
| OC-08a.2 | build | `cargo xtask train`: data from the synthetic page generator, a training loop in pure Rust in pinned arithmetic with a fixed order, checkpoints, and quantisation into the model format. | `xtask/src/train.rs` (new), `xtask/src/main.rs`, `crates/tinker-pdf-ocr/src/train.rs` (new) | A short training run from a seed reproduces its model's bytes on a second run. | OC-08a.1, OC-07.4, OC-03.2 |
| OC-08a.3 | test | Latin training and evaluation sets: the character set, faces under admitted licences, degradations, and line images with ground truth. | `crates/tinker-pdf/src/ocr/synth.rs` (new), `crates/tinker-pdf-ocr/tests/latin.rs` (new) | The sets regenerate byte-identically from their seeds, and every line's ground truth matches its rendered text. | OC-08a.2 |
| OC-08a.4 | measure | The Latin model trained and committed with its training manifest. *Note:* This is the one small bundled Latin model behind the `ocr` profile, H-40's default, with every other model host-supplied; it counts against the footprint budget, which MS-06 ratchets at the measured size until H-04 rules (H-04's default). | `crates/tinker-pdf-ocr/models` (new), `crates/tinker-pdf-ocr/tests/latin.rs` (new) | The training run reproduces the committed model's bytes. | OC-08a.3 |
| OC-08a.5 | measure | The character error rate on first-party synthetic pages, ratcheted in `cargo test`. | `crates/tinker-pdf-ocr/tests/latin.rs` (new) | The rate is below the target the owner approves (H-71) and held as a floor. | OC-08a.4, H-71, OC-07.5, OC-07.6 |
| OC-08a.6 | build | Page recognition end to end for Latin: preprocessing, segmentation, the line model, and the words into FT-24's model. | `crates/tinker-pdf/src/ocr/mod.rs` (new), `crates/tinker-pdf/tests/ocr_pages.rs` (new) | `ocr_pages.rs` (new): generated Latin pages recognise to their text and boxes, the character error rate measured here held as a floor, which OC-08a.5 tightens to the owner's target once H-71 sets it. | OC-08a.4, OC-06.6, OC-03.4, OC-01.2, OC-07.5, OC-07.6 |

## OC-08b — Models, milestone 2 — Cyrillic and Greek

**Models, milestone 2 — Cyrillic and Greek**

**Exit criterion.** as OC-08a per script

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-08b.1 | design | Add Cyrillic and Greek to `docs/design/ocr-models.md`: the character sets, the faces and the targets. *Note:* Second in H-38's default order. | `docs/design/ocr-models.md` (new) | The section exists, names OC-08b.2 and OC-08b.3's exit tests. | OC-08a.1 |
| OC-08b.2 | measure | Cyrillic and Greek sets generated and the models trained and committed. | `crates/tinker-pdf-ocr/models` (new), `crates/tinker-pdf-ocr/tests/cyrillic_greek.rs` (new) | The training run reproduces each committed model's bytes. | OC-08b.1, OC-08a.4 |
| OC-08b.3 | measure | Each script's character error rate on synthetic pages, ratcheted in `cargo test`. | `crates/tinker-pdf-ocr/tests/cyrillic_greek.rs` (new) | Each rate is below the target the owner approves and held as a floor. | OC-08b.2, H-71 |

## OC-08c — Models, milestone 3 — Arabic and Hebrew

**Models, milestone 3 — Arabic and Hebrew** (right-to-left, joining, ruling 14's logical order)

**Exit criterion.** as OC-08a; recognised text extracts in logical order

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-08c.1 | design | Add Arabic and Hebrew to `docs/design/ocr-models.md`: right-to-left lines, joining forms through the shaper, and output in logical order (ruling 14). *Note:* Third in H-38's default order. | `docs/design/ocr-models.md` (new) | The section exists, names OC-08c.2 to OC-08c.4's exit tests. | OC-08a.1 |
| OC-08c.2 | test | Arabic and Hebrew sets rendered through the shaper with their joining forms, ground truth in logical order. *Note:* The Arabic and Hebrew faces are FT-02.4's, vendored under H-116's ruling. | `crates/tinker-pdf/src/ocr/synth.rs` (new), `crates/tinker-pdf-ocr/tests/arabic_hebrew.rs` (new) | The sets regenerate byte-identically and each line's ground truth is the logical order of its rendered text. | OC-08c.1, OC-08a.3, FT-02.4 |
| OC-08c.3 | measure | The Arabic and Hebrew models trained and committed. | `crates/tinker-pdf-ocr/models` (new), `crates/tinker-pdf-ocr/tests/arabic_hebrew.rs` (new) | The training run reproduces each committed model's bytes. | OC-08c.2 |
| OC-08c.4 | measure | Each script's character error rate ratcheted, and recognised text extracting in logical order. | `crates/tinker-pdf-ocr/tests/arabic_hebrew.rs` (new), `crates/tinker-pdf/tests/ocr_pages.rs` (new) | Each rate is below the owner's target and held as a floor; a recognised page's extracted text is in logical order. | OC-08c.3, H-71 |

## OC-08d — Models, milestone 4 — Indic scripts

**Models, milestone 4 — Indic scripts**

**Exit criterion.** as OC-08a

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-08d.1 | design | Add Indic scripts to `docs/design/ocr-models.md`: which scripts, conjuncts and reordering through the shaper, and the targets. *Note:* Fourth in H-38's default order. | `docs/design/ocr-models.md` (new) | The section exists, names OC-08d.2 and OC-08d.3's exit tests. | OC-08a.1 |
| OC-08d.2 | measure | Indic sets rendered through the shaper and the models trained and committed. *Note:* The Indic faces are FT-02.4's, vendored under H-116's ruling. | `crates/tinker-pdf/src/ocr/synth.rs` (new), `crates/tinker-pdf-ocr/models` (new), `crates/tinker-pdf-ocr/tests/indic.rs` (new) | The sets regenerate byte-identically and the training run reproduces each committed model's bytes. | OC-08d.1, OC-08a.4, FT-02.4 |
| OC-08d.3 | measure | Each script's character error rate ratcheted in `cargo test`. | `crates/tinker-pdf-ocr/tests/indic.rs` (new) | Each rate is below the owner's target and held as a floor. | OC-08d.2, H-71 |

## OC-08e — Models, milestone 5 — Chinese, Japanese and Korean

**Models, milestone 5 — Chinese, Japanese and Korean**, horizontal and vertical

**Exit criterion.** as OC-08a

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-08e.1 | design | Add Chinese, Japanese and Korean to `docs/design/ocr-models.md`: the character sets, horizontal and vertical lines, and the targets. *Note:* Fifth in H-38's default order. | `docs/design/ocr-models.md` (new) | The section exists, names OC-08e.2 to OC-08e.5's exit tests. | OC-08a.1 |
| OC-08e.2 | test | CJK sets, horizontal and vertical, rendered with ground truth. *Note:* The CJK face is FT-01.8's, vendored under H-116's ruling. | `crates/tinker-pdf/src/ocr/synth.rs` (new), `crates/tinker-pdf-ocr/tests/cjk.rs` (new) | The sets regenerate byte-identically from their seeds. | OC-08e.1, OC-08a.3, FT-01.8 |
| OC-08e.3 | build | Vertical lines found by the layout analysis and fed to the recogniser. | `crates/tinker-pdf-ocr/src/layout.rs` (new), `crates/tinker-pdf-ocr/tests/cjk.rs` (new) | Generated vertical pages segment into their columns of text in order. | OC-08e.2, OC-06.6 |
| OC-08e.4 | measure | The Chinese, Japanese and Korean models trained and committed. | `crates/tinker-pdf-ocr/models` (new), `crates/tinker-pdf-ocr/tests/cjk.rs` (new) | The training run reproduces each committed model's bytes. | OC-08e.2, OC-08a.4 |
| OC-08e.5 | measure | Each language's character error rate, horizontal and vertical, ratcheted in `cargo test`. | `crates/tinker-pdf-ocr/tests/cjk.rs` (new) | Each rate is below the owner's target and held as a floor. | OC-08e.3, OC-08e.4, H-71 |

## OC-09 — Language resources

**Language resources**: dictionaries, user words and patterns, per-word confidence

**Exit criterion.** a dictionary corrects planted near-misses on synthetic pages; confidence calibrated on them

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-09.1 | design | Add language resources to `docs/design/ocr-models.md`: word lists under admitted licences, user words and patterns, dictionary-guided decoding, and confidence calibration. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `docs/design/ocr-models.md` (new) | The section exists, names OC-09.2 and OC-09.3's exit tests. | OC-08a.1 |
| OC-09.2 | build | Dictionary-guided decoding with user words and patterns, the word lists vendored under a licence the allowlist admits. | `crates/tinker-pdf-ocr/src/decode.rs` (new), `crates/tinker-pdf-ocr/data/words` (new), `THIRDPARTY.md`, `crates/tinker-pdf-ocr/tests/language.rs` (new) | `language.rs` (new): a dictionary corrects near-misses planted on synthetic pages, and a user word and a pattern each take precedence as the design states. | OC-09.1, OC-08a.6, OC-07.5 |
| OC-09.3 | build | Per-word confidence, calibrated on synthetic pages. | `crates/tinker-pdf-ocr/src/decode.rs` (new), `crates/tinker-pdf-ocr/tests/language.rs` (new) | The calibration error on held-out synthetic pages is within the design's bound. | OC-09.2 |

## OC-10 — OCR output formats

**OCR output formats**: hOCR, ALTO, PAGE XML, TSV, text, and the engine's structured-text formats

**Exit criterion.** each validated against its published schema or definition (shared writers with FT-24)

**Doc:** — · **Depends on:** FT-24, OC-08a

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-10.1 | build | Recognised pages written as hOCR, ALTO, PAGE XML, TSV and text through FT-24's shared writers, with per-word confidence, and through the engine's structured-text formats. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `crates/tinker-pdf/src/ocr/mod.rs` (new), `crates/tinker-pdf/tests/ocr_output.rs` (new) | `ocr_output.rs` (new): each format of a recognised page carries every word with its box and confidence. | OC-08a.6, FT-24.4, FT-24.5 |
| OC-10.2 | test | Each format validated against its published schema or definition through FT-24.2's harness. | `crates/tinker-pdf/tests/ocr_output.rs` (new), `crates/tinker-pdf/tests/word-box-schemas/fetch.sh` (new) | Every recognised page's ALTO and PAGE XML validate against their schemas where the harness has fetched them, and hOCR against its definition. | OC-10.1, FT-24.2 |

## OC-11 — Workflow modes

**Workflow modes**: skip pages with text, force, redo (replace an old OCR layer and keep born-digital text — ED-36f), strip; page selection; a sidecar text file; a policy for signed documents; OCR of the image regions only of a born-digital page, its text runs kept; per-page budgets; parallel pages; `tpdf ocr`

**Exit criterion.** each mode held to mixed born-digital and scanned fixtures, the image-regions mode leaving every born-digital run unchanged; a signed document refused or annotated as the policy states; `tpdf ocr` runs it on `testdata/` fixtures

**Doc:** — · **Depends on:** OC-02, P-15

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-11.1 | build | The workflow modes: skip pages with text, force, redo (an old OCR layer replaced through ED-36f, born-digital text kept), strip, and page selection. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `crates/tinker-pdf/src/ocr/workflow.rs` (new), `crates/tinker-pdf/tests/ocr_workflow.rs` (new) | `ocr_workflow.rs` (new): each mode is held to mixed born-digital and scanned fixtures. | OC-02.2, ED-36f |
| OC-11.2 | build | OCR of only the image regions of a born-digital page, its text runs kept, and a sidecar text file. | `crates/tinker-pdf/src/ocr/workflow.rs` (new), `crates/tinker-pdf/tests/ocr_workflow.rs` (new) | The image-regions mode leaves every born-digital run unchanged, compared run by run; the sidecar equals the recognised text. | OC-11.1 |
| OC-11.3 | build | A policy for signed documents, per-page budgets, and pages recognised in parallel through P-15's pool. | `crates/tinker-pdf/src/ocr/workflow.rs` (new), `crates/tinker-pdf/tests/ocr_workflow.rs` (new) | A signed document is refused or annotated as the policy states; a page past its budget stops with a typed warning; parallel output equals serial output byte for byte. | OC-11.1, P-15 |
| OC-11.4 | build | `tpdf ocr`, with the modes, the import seam and the output formats. | `tools/tpdf/src/main.rs`, `tools/tpdf/src/writing.rs`, `docs/features/ocr.md` (new) | The command runs on `testdata/` fixtures to the facade's output, its writes reopened and held to the strict validator. | OC-11.2, OC-11.3, OC-10.1 |

## OC-12 — Scan compression

**Scan compression**: mixed raster content (mask, foreground and background layers), JPEG 2000 backgrounds, palette quantisation, lossy JBIG2 symbol coding with refinement

**Exit criterion.** compressed scans render within the tolerance the design states of their sources; text masks stay lossless

**Doc:** `design/mrc.md` (owed) · **Depends on:** ED-26, ED-27

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-12.1 | design | Create `docs/design/mrc.md`: the mask, foreground and background layers, JPEG 2000 backgrounds, palette quantisation, lossy JBIG2 symbol coding with refinement, the tolerance compressed pages must hold, and masks kept lossless; milestones. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `docs/design/mrc.md` (new) | The doc exists in the design-doc order, states the tolerance, names OC-12.2 to OC-12.6's exit tests. | — |
| OC-12.2 | build | Layer separation: the text and line-art mask from the binarised page, a foreground colour layer, and the background with the masked areas filled. | `crates/tinker-pdf-ocr/src/mrc.rs` (new), `crates/tinker-pdf/tests/mrc.rs` (new) | `mrc.rs` (new): recombining the layers of a generated page gives its source within the tolerance, the mask pixel-exact. | OC-12.1, OC-03.3 |
| OC-12.3 | build | Mixed raster content written: the background through ED-26's JPEG 2000 encoder, the mask through ED-27's JBIG2, composited as the design states. | `crates/tinker-pdf/src/ocr/mrc_write.rs` (new), `crates/tinker-pdf/tests/mrc.rs` (new) | A written page renders within the tolerance of its source and passes the strict validator. | OC-12.2, ED-26.15, ED-27, OC-01.2 |
| OC-12.4 | build | Deterministic palette quantisation for foregrounds and few-colour pages. | `crates/tinker-pdf-ocr/src/mrc.rs` (new), `crates/tinker-pdf/tests/mrc.rs` (new) | The palette and the mapping equal those computed in the test. | OC-12.2 |
| OC-12.5 | build | Lossy JBIG2 symbol coding with refinement: symbols clustered by the design's similarity and the differences refined. | `crates/tinker-pdf-filters/src/jbig2_encode.rs` (new), `crates/tinker-pdf/tests/mrc.rs` (new) | A text-region page decodes within the tolerance, refinement bringing each symbol to the design's bound. | OC-12.1, ED-27 |
| OC-12.6 | test | Compressed scans held whole against their sources. | `crates/tinker-pdf/tests/mrc.rs` (new) | Every generated scan renders within the design's tolerance of its source and every text mask stays lossless. | OC-12.3, OC-12.4, OC-12.5 |

## OC-13 — Glyph-level recognition for unmapped fonts

**Glyph-level recognition for unmapped fonts**, writing a corrected `/ToUnicode`

**Exit criterion.** fixtures with stripped `cmap`s and unknown faces recover their text

**Doc:** — · **Depends on:** OC-08a, FT-35

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-13.1 | build | Glyphs of an unmapped font recognised: each code's glyph rendered alone and read by the Latin model, after FT-35's matching has had its turn. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `crates/tinker-pdf/src/ocr/glyphs.rs` (new), `crates/tinker-pdf/tests/ocr_glyphs.rs` (new) | `ocr_glyphs.rs` (new): fixtures with stripped `cmap`s and unknown faces map each code to its character. | OC-08a.4, FT-35.4, OC-01.2 |
| OC-13.2 | build | The recovered mapping written as a corrected `/ToUnicode` through FT-35.5's writer. | `crates/tinker-pdf/src/ocr/glyphs.rs` (new), `crates/tinker-pdf/tests/ocr_glyphs.rs` (new) | Those fixtures recover their text on extraction after the correction is saved. | OC-13.1, FT-35.5 |

## OC-14 — Vectorised scan text

**Vectorised scan text**: recognised words redrawn in synthesised vector glyphs matching their shapes

**Exit criterion.** rendered output within the stated tolerance of the scan; text extracts as recognised

**Doc:** `design/ocr-vector.md` (owed) · **Depends on:** OC-08a

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-14.1 | design | Create `docs/design/ocr-vector.md`: glyph images clustered per recognised character, outlines traced from each cluster, a synthesised font with `/ToUnicode`, placement, the tolerance, and milestones. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `docs/design/ocr-vector.md` (new) | The doc exists in the design-doc order, states the tolerance, names OC-14.2 to OC-14.5's exit tests. | — |
| OC-14.2 | build | Glyph images clustered per recognised character across a document. | `crates/tinker-pdf-ocr/src/vectorise.rs` (new), `crates/tinker-pdf/tests/ocr_vector.rs` (new) | `ocr_vector.rs` (new): generated pages' glyphs cluster by character as the generator records. | OC-14.1, OC-08a.6 |
| OC-14.3 | build | Each cluster's representative traced into outlines. | `crates/tinker-pdf-ocr/src/vectorise.rs` (new), `crates/tinker-pdf/tests/ocr_vector.rs` (new) | Each traced outline rasterises within the tolerance of its cluster's image. | OC-14.2 |
| OC-14.4 | build | A synthesised font with `/ToUnicode`, and the recognised words redrawn in it in place of the image text. | `crates/tinker-pdf/src/ocr/vector_write.rs` (new), `crates/tinker-pdf/tests/ocr_vector.rs` (new) | A redrawn page passes the strict validator and its text extracts as recognised. | OC-14.3 |
| OC-14.5 | test | Vectorised pages held against their scans. | `crates/tinker-pdf/tests/ocr_vector.rs` (new) | Every generated page renders within the stated tolerance of its scan and extracts as recognised. | OC-14.4 |

## OC-15a — Tables in images

**Tables in images**: table structure recovered from scanned pages

**Exit criterion.** generated scanned tables recover their grids

**Doc:** `design/scan-layout.md` (owed) · **Depends on:** OC-06

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-15a.1 | design | Add tables in images to `docs/design/scan-layout.md`: ruled and unruled tables, spans and headers, and the floors. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `docs/design/scan-layout.md` (new) | The section exists, names OC-15a.2 to OC-15a.5's exit tests. | OC-06.1 |
| OC-15a.2 | build | Ruled tables: rules found and the grid built from their intersections. | `crates/tinker-pdf-ocr/src/tables.rs` (new), `crates/tinker-pdf-ocr/tests/scan_tables.rs` (new) | `scan_tables.rs` (new): generated ruled tables recover their grids. | OC-15a.1, OC-06.4 |
| OC-15a.3 | build | Unruled tables from the alignment of recognised words, through FT-31's model. | `crates/tinker-pdf-ocr/src/tables.rs` (new), `crates/tinker-pdf-ocr/tests/scan_tables.rs` (new) | Generated unruled tables recover their grids above the floor. | OC-15a.2, OC-08a.6, FT-31 |
| OC-15a.4 | build | Spans and header cells, written into the table model the born-digital path uses. | `crates/tinker-pdf-ocr/src/tables.rs` (new), `crates/tinker-pdf-ocr/tests/scan_tables.rs` (new) | Generated tables with spans and headers recover them. | OC-15a.3 |
| OC-15a.5 | test | The generated scanned-table set run whole. | `crates/tinker-pdf-ocr/tests/scan_tables.rs` (new) | Every generated scanned table recovers its grid above the design's floor, held as a ratchet. | OC-15a.4 |

## OC-15b — Marks on scanned forms

**Marks on scanned forms**: checkbox and bubble states, signature presence, patch-code and barcode separators

**Exit criterion.** generated forms with known marks read correctly

**Doc:** — · **Depends on:** OC-06, ED-50

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-15b.1 | build | Checkbox and bubble states read from scanned forms. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `crates/tinker-pdf-ocr/src/marks.rs` (new), `crates/tinker-pdf-ocr/tests/marks.rs` (new) | `marks.rs` (new): generated forms' boxes and bubbles read with their known states. | OC-06.4 |
| OC-15b.2 | build | Signature presence in a stated region. | `crates/tinker-pdf-ocr/src/marks.rs` (new), `crates/tinker-pdf-ocr/tests/marks.rs` (new) | Generated forms with and without a signature read correctly. | OC-15b.1 |
| OC-15b.3 | build | Patch-code and barcode separators through ED-50.8's decoder, splitting a batch. | `crates/tinker-pdf-ocr/src/marks.rs` (new), `crates/tinker-pdf-ocr/tests/marks.rs` (new) | A generated batch with separators splits where they stand. | OC-15b.1, ED-50.8 |

## OC-15c — Handwriting recognition

**Handwriting recognition**

**Exit criterion.** first-party synthetic handwriting and person-transcribed samples (H-71) below the approved error rate

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a, H-71

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-15c.1 | design | Add handwriting to `docs/design/ocr-models.md`: synthetic handwriting from stroke models, data H-39 admits, the model, and the targets. *Note:* The run takes this row after every other row of its tier: H-38's default puts handwriting last. | `docs/design/ocr-models.md` (new) | The section exists, names OC-15c.2 to OC-15c.5's exit tests. | OC-08a.1 |
| OC-15c.2 | test | A synthetic handwriting generator: strokes rendered with seeded variation, with ground truth. | `crates/tinker-pdf/src/ocr/synth.rs` (new), `crates/tinker-pdf-ocr/tests/handwriting.rs` (new) | `handwriting.rs` (new): a line from a seed is byte-identical across runs and carries its text. | OC-15c.1, OC-03.2, OC-01.4 |
| OC-15c.3 | measure | The handwriting model trained and committed. | `crates/tinker-pdf-ocr/models` (new), `crates/tinker-pdf-ocr/tests/handwriting.rs` (new) | The training run reproduces the committed model's bytes. | OC-15c.2, OC-08a.2, H-39 |
| OC-15c.4 | measure | The error rate on synthetic handwriting, ratcheted in `cargo test`. | `crates/tinker-pdf-ocr/tests/handwriting.rs` (new) | The rate is below the approved target and held as a floor. | OC-15c.3, H-71 |
| OC-15c.5 | measure | The error rate on person-transcribed samples (H-71). | `crates/tinker-pdf-ocr/tests/handwriting.rs` (new), `docs/design/ocr-models.md` (new) | The rate is below the approved target, recorded with the samples' provenance. | OC-15c.4, H-71 |

## OC-15d — Mathematics recognition

**Mathematics recognition** to MathML and LaTeX

**Exit criterion.** generated formula images recover their MathML

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-08a, FT-34

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-15d.1 | design | Add mathematics recognition to `docs/design/ocr-models.md`: formula detection, symbol recognition, structure into MathML, LaTeX through FT-34's writer, and formula images generated from MathML through CD-04's layout. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `docs/design/ocr-models.md` (new) | The section exists, names OC-15d.2 to OC-15d.5's exit tests. | OC-08a.1 |
| OC-15d.2 | build | Formula regions found on scanned pages. | `crates/tinker-pdf-ocr/src/math.rs` (new), `crates/tinker-pdf-ocr/tests/math.rs` (new) | `math.rs` (new): generated pages' formula regions are found above the design's floor. | OC-15d.1, OC-06.4 |
| OC-15d.3 | measure | A symbol model trained on formula images CD-04 renders, and committed. | `crates/tinker-pdf-ocr/models` (new), `crates/tinker-pdf-ocr/tests/math.rs` (new) | The training run reproduces the committed model's bytes. | OC-15d.1, OC-08a.2, CD-04.6 |
| OC-15d.4 | build | The recognised symbols' spatial relations built into MathML, and LaTeX through FT-34's writer. | `crates/tinker-pdf-ocr/src/math.rs` (new), `crates/tinker-pdf-ocr/tests/math.rs` (new) | Fractions, scripts and radicals in generated images rebuild their MathML. | OC-15d.2, OC-15d.3, FT-34 |
| OC-15d.5 | test | Generated formula images held against their MathML. | `crates/tinker-pdf-ocr/tests/math.rs` (new) | Generated formula images recover their MathML at the design's rate, held as a ratchet. | OC-15d.4 |

## OC-16 — Alternate-text suggestions

**Alternate-text suggestions** for figures, on-device, flagged for a person's review

**Exit criterion.** suggestions produced deterministically for the committed figures and never written as final text without a caller's assertion

**Doc:** `design/ocr-models.md` (owed) · **Depends on:** OC-07

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-16.1 | design | Add alternate-text suggestions to `docs/design/ocr-models.md`: an on-device caption model on OC-07's runtime, data H-39 admits, determinism, and the rule that a suggestion is flagged for review and never written as final text without a caller's assertion. *Note:* Built toward H-38's default scope: printed text first, handwriting last. | `docs/design/ocr-models.md` (new) | The section exists, names OC-16.2 to OC-16.4's exit tests. | OC-08a.1 |
| OC-16.2 | build | The image-encoder and text-decoder layers the caption model needs, on the runtime's kernels. | `crates/tinker-pdf-ocr/src/sequence.rs` (new), `crates/tinker-pdf-ocr/tests/kernels.rs` (new) | Each layer's output on fixed inputs equals the value computed in the test. | OC-16.1, OC-07.4, OC-01.4 |
| OC-16.3 | measure | The caption model trained under H-39's terms and committed. | `crates/tinker-pdf-ocr/models` (new), `crates/tinker-pdf-ocr/tests/captions.rs` (new) | The training run reproduces the committed model's bytes. | OC-16.2, OC-08a.2, H-39 |
| OC-16.4 | build | Suggestions offered for figures, alongside SD-07's alternate-text placeholders, flagged for a person's review. | `crates/tinker-pdf/src/ocr/captions.rs` (new), `crates/tinker-pdf/tests/ocr_captions.rs` (new) | `ocr_captions.rs` (new): suggestions for the committed figures are byte-identical across runs, and none is written as final text without a caller's assertion, asserted. | OC-16.3, SD-07.5, OC-01.2 |

## OC-17 — The OCR accuracy and speed ratchet

**The OCR accuracy and speed ratchet**: character and word error rates per script on first-party synthetic pages, and pages per second per script, in the benchmark set

**Exit criterion.** the rates and speeds recorded in `cargo test` and `baseline.json` and held as floors; **on clean synthetic 300-dpi Latin pages a character error rate ≤ 1 % and a word error rate ≤ 3 %, ≤ 3 % and ≤ 8 % on the degraded set, and a page in ≤ 1.5 s on one thread; Chinese pages ≤ 2 % characters wrong clean and ≤ 4 s a page**; each other script's numbers stated in its own milestone row (OC-08b…OC-08e) when it lands; the error targets stand until H-71 re-states them, the speeds gate as calibrated (tier P)

**Doc:** — · **Depends on:** OC-08a, MS-04

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| OC-17.1 | measure | Character and word error rates per script on first-party synthetic pages, clean and degraded, recorded in `cargo test` as floors. | `crates/tinker-pdf-ocr/tests/latin.rs` (new), `docs/features/ocr.md` (new) | Latin on clean 300-dpi pages has at most 1 % of characters and 3 % of words wrong, and at most 3 % and 8 % on the degraded set, held as floors until H-71 re-states them. | OC-08a.5, OC-02.2, OC-01.4 |
| OC-17.2 | measure | Pages per second per script in the benchmark set, one thread. | `crates/tinker-pdf/benches/ocr.rs` (new), `crates/tinker-pdf/Cargo.toml`, `.github/workflows/bench.yml`, `crates/tinker-pdf/benches/baseline.json` | A Latin page recognises in at most 1.5 s on one thread, recorded in `baseline.json` and gated as tier P calibrates. | OC-08a.6, MS-04 |
| OC-17.3 | measure | Chinese pages' error rate and speed. | `crates/tinker-pdf-ocr/tests/cjk.rs` (new), `crates/tinker-pdf/benches/baseline.json` | Clean Chinese pages have at most 2 % of characters wrong and recognise in at most 4 s a page, both held as floors. | OC-08e.5, OC-17.2 |
