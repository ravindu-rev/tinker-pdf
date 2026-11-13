# Tier 1 — prove correctness

[The roadmap](../ROADMAP.md) says how a step is worked and what comes next.

These come before any new feature, with tier 0. Ruling 13 says the engine
agreeing with itself is the only kind of proof this repository will have,
which raises the bar on what the checks must be: answers computable in
closed form, bitstreams transcribed from the standards' own annexes,
published conformance data, and thousands of documents nobody here
authored. What this tier cannot contain is stated once: the four properties
ruling 13 retires ([verification.md](../verification.md), "What does not
come back") are not rows (NG-04).

The nine reviewed goldens are a person's reading, H-60 in [H.md](H.md);
K-08 extends the mechanism to every new rendered surface.

## K-02 — Default colour spaces

**Default colour spaces** (8.6.5.6): `DefaultGray`, `DefaultRGB`, `DefaultCMYK` in a page's or form's `/ColorSpace` resources substitute for device spaces when rendering and extracting colour

**Exit criterion.** fixtures with each default space and their twins drawn in the substituted space directly, byte-identical; a corpus census of files carrying one, recorded

**Doc:** [design/icc.md](../design/icc.md) (amend) · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| K-02.5 | measure | Run the census locally over the fetched corpora, read it to its `RAN` line, and commit its first figures as the census's floors and in `docs/features/rendering.md`, so the row's census is recorded. *Note:* After K-02.2, so the same run measures the substituted rendering across the corpus; a bar that falls is a finding, not a re-recorded bar (H-78). `default_space_census.rs` is K-02.4's file. | `crates/tinker-pdf/tests/default_space_census.rs`, `docs/features/rendering.md` | The local run is green and prints `default-space-census: RAN`, recorded here; the counts it printed are committed as floors in `default_space_census.rs` and quoted in `docs/features/rendering.md`. | MS-11 |

## K-03 — Validate on save, every writer path

**Validate on save, every writer path** — every test that writes a document holds it to the strict validator and any profile it claims; the object grammar at the declared version (SD-20) joins the same helper in SD-20's own commit

**Exit criterion.** a shared test helper every writing test file calls; `cargo xtask check` fails a writing test that does not

**Doc:** [design/validate-on-save.md](../design/validate-on-save.md) · **Depends on:** —

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| K-03.6 | test | Migrate the last two facade units that change and save a document: `image_recode.rs` and `redact.rs`'s tests, each rewrite of a fixture held to structure clean (an incremental update: no worse than its input, and clean past the input's last byte) and semantics no worse than its input; each leaves the pending list. *Note:* The helper surfaces one finding in each file's campaign over damaged inputs, so by K-03.4's rule both stay pending until it is settled. `image_recode.rs`: the rewrite of one damaged page holds a `repaired-while-reading` (`non-hex byte skipped`) its input's own reading does not report, though the rewrite copied the damaged hex image as it found it, so a rewrite of a damaged file can fail `rewritten` for damage it did not make. `redact.rs`: a real the reader clamped to `f64::MAX` is written as the digits of `f64::MAX`, which the reader's real builder takes past it and clamps again (`object-repaired`, `real-overflow-clamped`). The editor's, the signer's, the sealer's and encryption's tests, and the `write.rs` and `subset.rs` test modules, hold what they write. `written.rs` is K-03.3's file. | `crates/tinker-pdf/tests/image_recode.rs`, `crates/tinker-pdf/src/redact.rs`, `xtask/src/written.rs` | `cargo test -p tinker-pdf` passes in the default and `bundled-fonts` builds with both files calling the helper, and `cargo xtask check` passes with them off the pending list. | K-03.3 |
| K-03.7 | test | Migrate every other facade test file and `src` test module still on the pending list — the render, text, analysis, signature-reading and census tests whose fixtures are built with `DocumentBuilder` or rewritten — so every document they write is held to the helper, and each file leaves the pending list. *Note:* Sixty files by the row finding's search, each a mechanical change; the `#[ignore]`d corpus censuses among them run only over locally fetched corpora. Same rule as K-03.4 for a defect the helper surfaces. `written.rs` is K-03.3's file. | `crates/tinker-pdf/tests`, `crates/tinker-pdf/src`, `xtask/src/written.rs` (new) | `cargo test -p tinker-pdf` passes in the default and `bundled-fonts` builds, and `cargo xtask check` passes with no file under `crates/tinker-pdf` left on the pending list. | K-03.3 |
| K-03.8 | test | Migrate the `tinker-pdf-cos` writing tests and its `src` test modules, the FFI's `src` test modules and the CLI's writing test modules, then empty and delete the pending list, so `cargo xtask check` fails any test that writes a document without the helper. *Note:* `README.md` and `docs/verification.md`'s strict-validator section then say the rule holds by test, not file by file. `strict_validator.rs` writes damage on purpose and uses the expected-defects form. Same rule as K-03.4 for a defect the helper surfaces. | `crates/tinker-pdf-cos/tests`, `crates/tinker-pdf-cos/src`, `crates/tinker-pdf-ffi/src`, `tools/tpdf/src`, `xtask/src/written.rs` (new), `README.md`, `docs/verification.md` | `written.rs` holds no pending list and its unit test `a_new_writing_test_without_the_helper_fails` passes; `cargo xtask check` passes on the tree; `cargo test --workspace` passes, and `cargo test -p tinker-pdf-cos` and `-p tinker-pdf` with `--no-default-features` pass. | K-03.4, K-03.5, K-03.6, K-03.7 |

## K-04 — Structure-aware and metamorphic fuzzing

**Structure-aware and metamorphic fuzzing** — generators for COS syntax, content streams, CFF and `glyf`, JPX codestreams, CSS, HTML and XML; metamorphic targets asserting render equals rewrite-then-render, tile equals page, and native equals wasm on the same input

**Exit criterion.** the new targets in `fuzz/fuzz_targets/`, each run locally for 600 seconds with its coverage figure recorded in `fuzz/coverage.tsv`; every metamorphic disagreement a failure

**Doc:** [design/structured-fuzzing.md](../design/structured-fuzzing.md) · **Depends on:** MS-11

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| K-04.7 | build | Markup generators over one generated element tree, as two targets: well-formed and near-well-formed XML (namespaces, CDATA, entity references, a document type declaration the reader must refuse to parse) and HTML tag soup aimed at the tree builder's insertion modes (foster parenting, the adoption agency, table and template modes). | `fuzz/fuzz_targets/xml_structured.rs` (new), `fuzz/fuzz_targets/html_structured.rs` (new), `fuzz/Cargo.toml`, `fuzz/corpus/xml_structured` (new), `fuzz/corpus/html_structured` (new), `xtask/src/fuzzaudit.rs`, `fuzz/README.md` | Both targets (new) build, are in the roster, and replay their seeds clean in a local twenty-second `cargo fuzz run`; for every well-formed XML document generated the reader's element count equals the generator's, and every HTML input builds a tree whose parent and child links agree, inside `MAX_DOM_NODES`. | K-04.2 |
| K-04.8 | build | A CSS generator: rule sets, at-rules (`@media`, `@import` cycles, `@font-face`, `@layer`), selectors of every combinator and pseudo-class `selectors-4` defines, and declarations of known and unknown properties, cascaded over a generated tree as the existing `css` target does. *Note:* The index-against-brute-force assertion is the existing `css` target's strongest (`fuzz/README.md`, its row); this target reaches it with well-formed stylesheets. | `fuzz/fuzz_targets/css_structured.rs` (new), `fuzz/Cargo.toml`, `fuzz/corpus/css_structured` (new), `xtask/src/fuzzaudit.rs`, `fuzz/README.md` | `css_structured` (new) builds, is in the roster, and replays its seeds clean in a local twenty-second `cargo fuzz run`, asserting on every input that indexed matching and brute-force matching give the same cascade. | K-04.2 |
| K-04.9 | build | A metamorphic target asserting render equals rewrite-then-render: a document from the COS and content generators that opens without repair is saved, reopened and rendered again, and any byte that differs fails the run. | `fuzz/fuzz_targets/render_rewrite.rs` (new), `fuzz/Cargo.toml`, `fuzz/corpus/render_rewrite` (new), `xtask/src/fuzzaudit.rs`, `fuzz/README.md` | `render_rewrite` (new) builds, is in the roster, and replays its seeds clean in a local twenty-second `cargo fuzz run`; every page of every clean input renders identically before and after `save`. | K-04.3 |
| K-04.10 | build | A metamorphic target asserting tile equals page: a generated page rendered whole and as a generated region at a generated scale agrees byte for byte over the region (ruling 5), with RC-21's image run the one shape excluded, as H-15's default keeps it. *Note:* When RC-21 lands, its exclusion is deleted here in the same commit. | `fuzz/fuzz_targets/render_tile.rs` (new), `fuzz/Cargo.toml`, `fuzz/corpus/render_tile` (new), `xtask/src/fuzzaudit.rs`, `fuzz/README.md` | `render_tile` (new) builds, is in the roster, and replays its seeds clean in a local twenty-second `cargo fuzz run`; every region it asks for equals the crop of the whole render. | K-04.3 |
| K-04.11a | test | Native equals wasm on the same input: a replay test hashes the render of each file in a directory and prints the hashes with a `RAN` banner, run locally natively and on `wasm32-wasip1` over minimised `render_page`, `content_stream` and `render_rewrite` corpora from local fuzz sessions, any hash that differs between the two a failure. *Note:* Builds toward H-136's default. The wasm runtime hosts the test binary and judges nothing (ruling 13), as in the local `wasm32-wasip1` fingerprint runs; it reaches no file unless granted the corpus directory, so the run must grant it or the wasm side reads nothing and its banner check fails. | `crates/tinker-pdf/tests/wasm_parity.rs` (new) | `wasm_parity.rs` (new) `print_the_render_hash_of_every_input` prints `wasm-parity: RAN` with a non-zero count natively and on `wasm32-wasip1`, or `SKIPPED` with no directory; a local run over the three corpora prints the same two lists, recorded here. | K-04.3, K-04.9, MS-11, H-136 |
| K-04.11b | test | The corpus half of K-04.11a's exit: a local run of `wasm_parity.rs` over the three minimised corpora K-04.12's sessions leave prints the same two lists natively and on `wasm32-wasip1`, recorded here. *Note:* Split from K-04.11 by the owner's choice to run long proofs after the list; K-04.11a lands the test and its `SKIPPED` path. | `crates/tinker-pdf/tests/wasm_parity.rs` | The two lists agree over all three corpora, recorded here. | K-04.11a, K-04.12 |
| K-04.12 | measure | Run every new target locally on Linux for its full 600 s (`cargo fuzz run <target> fuzz/corpus/<target> -- -max_total_time=600`), and record each new target's coverage figure — the `cov:` and `ft:` of its session's final `DONE` line — in `fuzz/coverage.tsv`. *Note:* `-print_final_stats` prints the `stat::` lines, not coverage; the edge and feature counts are the `DONE` status line's. | `fuzz/coverage.tsv` (new) | Every session finds nothing and K-04.11's native-against-wasm run agrees, recorded here; `fuzz/coverage.tsv` carries each new target's `cov` and `ft`. | K-04.4, K-04.5, K-04.6, K-04.7, K-04.8, K-04.10, K-04.11a, MS-11 |

## K-05 — Resource exhaustion counts as a failure

**Resource exhaustion counts as a failure** — per-input timeouts, a resident-memory limit and stack exhaustion fail a fuzz run; every parser has a size-scaling complexity test

**Exit criterion.** the local fuzz sessions set explicit memory and per-allocation limits derived from `bounds_ledger.rs` and fail on either; a size-scaling complexity test per parser family (COS, content, fonts, each codec, XML/HTML/CSS, archives) in `cargo test`

**Doc:** [design/resource-exhaustion.md](../design/resource-exhaustion.md) · **Depends on:** MS-01

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| K-05.3b | ci | The session half of K-05.3: a local session of every target, each started as `docs/verification.md` gives it, under its `fuzz/limits.tsv` row's `-rss_limit_mb` and `-malloc_limit_mb`, on the stack budget, with the quarantine and page release pinned. *Note:* Split from K-05.3 by the owner's choice to run long proofs after the list; K-05.3a landed the command, for the seed replay and the session alike. A target that fails under its derived limit is a finding fixed by bounding its parser, never a raised limit, and a resident failure is reproduced from its saved input, or from the inputs the session added before it, before it is fixed. | `docs/verification.md` | A local session of every target finds nothing, each session's log showing its limits, recorded here. | K-05.3a, MS-11 |
| K-05.6b | test | Size-scaling complexity tests for the transform codecs in `tinker-pdf-filters`: JPEG (baseline, progressive, arithmetic), JPX, WebP and JPEG XR, each with a counted sibling of its door that returns the steps its decode took, as K-05.6a gave the others. *Note:* Split from K-05.6, which turned out bigger than one change; K-05.6a landed the counted doors of inflate, LZW, ASCII85, ASCIIHex, RunLength, the predictors, brotli, CCITT, PNG, TIFF, GIF and BMP and their tests in `complexity.rs`, with JBIG2's through `jbig2_decode_measured`. Arithmetic-coded JPEG shapes are built with `qm.rs`'s test-only QM encoder from repeated hand-derived MCUs, as `arith.rs`'s tests drive the coder: an interval coded once, committed beside the test and held to that encoder's output by a unit test, repeated between restart markers. JPEG XR, which this repository cannot encode, is read from the committed lossless pictures whose areas are four times one another. | `crates/tinker-pdf-filters/tests/complexity.rs`, `crates/tinker-pdf-filters/src/jpeg.rs`, `crates/tinker-pdf-filters/src/jpx/mod.rs`, `crates/tinker-pdf-filters/src/webp.rs`, `crates/tinker-pdf-filters/src/jxr/mod.rs` | `complexity.rs` in `tinker-pdf-filters`, in `cargo test`: one test each for JPEG, JPX, WebP and JPEG XR, each holding work at k times n to K-05.1's bound times work at n over that codec's shapes. | K-05.6a |
| K-05.7 | test | Size-scaling complexity tests for the markup and style parsers: XML (depth, attributes, namespaces, character references), HTML tree construction (formatting elements, foster parenting, tables), and CSS tokenising, parsing, selector matching and the cascade. *Note:* HTML tree construction already counts its steps, under `cfg(test)` only (`step` and `STEPS`, `crates/tinker-pdf-xml/src/html/mod.rs:326-343`; `assert_linear`, `src/html/tree.rs:3312`), where an integration test cannot see them: its shapes read MS-01's tally from that one place, never a second count beside it, and `moving_many_children_is_linear` stays. | `crates/tinker-pdf-xml/tests/complexity.rs` (new), `crates/tinker-pdf-css/tests/complexity.rs` (new), `crates/tinker-pdf-xml/src/html/mod.rs` | `complexity.rs` (new) in each crate, in `cargo test`: for every shape, work at k times n is at most K-05.1's bound times work at n; the HTML tree builder's `assert_linear` tests pass unchanged. | K-05.1 |
| K-05.8 | test | Size-scaling complexity tests for the archives: ZIP by its central directory and its local-header scan, tar, 7z and its coders (LZMA, BCJ, BCJ2, PPMd, bzip2, zstd), and RAR. | `crates/tinker-pdf-zip/tests/complexity.rs` (new), `crates/tinker-pdf-archive/tests/complexity.rs` (new) | `complexity.rs` (new) in each crate, in `cargo test`: for every shape, work at k times n is at most K-05.1's bound times work at n. | K-05.1 |

## K-06 — The nightly `jbig2` fuzz failure

**The nightly `jbig2` fuzz failure** — reproduce on `develop`, fix or show it fixed; ruling 1 makes it a release blocker

**Exit criterion.** every target clean in seven local sessions of 600 seconds each (`cargo fuzz run <target> fuzz/corpus/<target> -- -max_total_time=600`), recorded here

**Doc:** — · **Depends on:** MS-11

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| K-06.2 | test | Close the row on local sessions: no scheduled run's `crash-jbig2` artifact survives, so the nightly's input cannot be shown; every fuzz target runs clean in seven local sessions of 600 seconds each (`cargo fuzz run <target> fuzz/corpus/<target> -- -max_total_time=600`, a scratch directory first so no new input lands in the committed seeds), and `verification.md`'s `jbig2` row says what the sessions show. *Note:* A session that finds an input is a `fix` before this step closes, its input committed as a seed pinned by its refusal. | `docs/verification.md`, `docs/roadmap/K.md` | Every target's seven sessions finish clean, recorded here, and K-06 leaves the roadmap. | — |

## K-08 — Reviewed-golden families for every new rendered surface

**Reviewed-golden families for every new rendered surface** — the mechanism of `render_goldens.rs` extended: synthesised annotation appearances, visible signature appearances, fields under formatting scripts, OCR text-layer alignment, sample EPUB/HTML/Markdown/FB2 pages, SVG and XPS pages, `to_svg` output; each family's header names its clause and enters `UNREVIEWED`

**Exit criterion.** each family committed with `unreviewed` headers and held byte for byte; the reading is H-73

**Doc:** — · **Depends on:** the row that adds the surface

| Step | Kind | What | Where | Exit | Depends on |
| --- | --- | --- | --- | --- | --- |
| K-08.5 | test | A golden family for fields under formatting scripts: a page of text fields whose appearances show the values `AFNumber_Format`, `AFPercent_Format`, `AFDate_Format` and `AFSpecial_Format` produce, its header naming its clause, entering `UNREVIEWED`, once a formatted value reaches a field's appearance. *Note:* No appearance draws a formatted value today: `formatted_value` returns a `DisplayString` no writer takes (`docs/features/forms.md`). FJ-07's formatting after a committed change is taken as the row that adds the surface. | `crates/tinker-pdf/tests/surface_goldens.rs` (new), `crates/tinker-pdf/tests/goldens/surfaces/formatted-fields.ppm` (new) | `every_surface_golden_is_what_this_build_draws` holds `formatted-fields` byte for byte above its ink floor, `unreviewed` and listed in `UNREVIEWED`, in the default and `bundled-fonts` builds. | K-08.1, FJ-07 |
| K-08.6 | test | A golden family for OCR text-layer alignment: a scanned-page fixture with its imported invisible text layer drawn visibly over it (each word's box and baseline), so a reviewer sees the layer sit on the words; its header naming its clause, entering `UNREVIEWED`. *Note:* The layer is OC-01's import seam's; the recogniser's own layer (OC-02) joins the family in that row. | `crates/tinker-pdf/tests/surface_goldens.rs` (new), `crates/tinker-pdf/tests/goldens/surfaces/ocr-layer-alignment.ppm` (new) | `every_surface_golden_is_what_this_build_draws` holds `ocr-layer-alignment` byte for byte above its ink floor, `unreviewed` and listed in `UNREVIEWED`, in the default and `bundled-fonts` builds. | K-08.1, OC-01.3 |
