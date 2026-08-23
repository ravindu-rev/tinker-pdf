# Text shaping

When this is done, every place this engine *produces* text — a
`DocumentBuilder` page, a form-fill appearance stream, an EPUB paginated
into a book — sets Arabic, Indic and ligature-dependent scripts the way a
reader of those scripts expects, from a new leaf crate that takes face
bytes and a paragraph of text and returns positioned glyph runs,
bit-identically on every target; and the docs stop stating shaping as a
permanent non-goal, because the reasoning behind that non-goal — recorded
below, since it was right — only ever covered the consuming half of the
engine.

## Scope

- **The overturn, recorded.** Plan 05 (fonts, now
  [features/fonts.md](../features/fonts.md)) refused shaping ("PDF content
  streams position pre-shaped glyphs"), plan 06 (content and text, now
  [features/content-and-text.md](../features/content-and-text.md)) refused
  full UAX #9, and `crates/tinker-pdf-layout/src/metrics.rs` states "No
  kerning and no shaping" on the `Metrics` trait by design. That reasoning
  **holds** for rendering existing PDFs — the producer positioned every
  glyph, and re-shaping them would be wrong — and fails wherever *this
  engine is the producer*: `DocumentBuilder` text, the appearance streams
  `text_appearance` builds in `crates/tinker-pdf-cos/src/fill.rs`, and
  EPUB layout, where `Metrics::advance` is per character and a sum over a
  run. An Arabic or Devanagari EPUB cannot paginate legibly today. The
  [ROADMAP](../ROADMAP.md) Tier 3 entry overturns the non-goal; this doc
  is its design.
- **A new leaf crate, `tinker-pdf-shape`**, under ruling 8
  ([rulings.md](../rulings.md)): face bytes and plain text in, positioned
  glyph runs out; no COS types, no PDF or CSS vocabulary in its API.
- **OpenType Layout** (ISO/IEC 14496-22): `GDEF`, `GSUB` lookup types 1–8,
  `GPOS` lookup types 1–9, coverage and class-definition tables, extension
  and chaining-context lookups, feature and script lists.
- **UAX #9** bidirectional algorithm: paragraph-level embedding-level
  resolution in the crate; per-line reordering (the L rules) exposed as a
  pure function layout calls after line breaking, since only layout knows
  where lines end.
- **Script itemization** (UAX #24 `Scripts.txt`) and the mandatory feature
  set per script: `ccmp`/`liga`/`kern`/`mark`/`mkmk` for the default
  shaper; joining forms (`isol`/`init`/`medi`/`fina`), `rlig` and cursive
  attachment for Arabic-script text; the Universal Shaping Engine's
  cluster model for Indic and Southeast Asian scripts.
- **Consumers, staged in this order**: `tinker-pdf-layout` (a `Shaper`
  seam beside the existing `Metrics` trait), then
  `DocumentBuilder::glyph_run` (`crates/tinker-pdf-cos/src/build.rs`,
  which already takes caller-positioned glyphs and a `Glyph` that pairs a
  glyph index with the text it stands for — the ligature-to-`/ToUnicode`
  case is already documented on that type), then `fill.rs` form
  appearances.
- **Determinism as a contract** (ruling 4): all shaping arithmetic in
  integer font design units — `FWORD`s in, `i32` positions out — so output
  is bit-identical across linux, windows, macos and wasm with no float in
  the crate at all.

## Non-goals

- **Shaping while rendering existing PDFs.** The original non-goal's
  reasoning survives for the consuming half: `TJ` arrays are honored as
  written, forever. Nothing in `tinker-pdf-render` calls this crate.
- **AAT (`morx`) and Graphite tables.** OpenType Layout only; a face
  carrying only `morx` shapes as if unshaped, with a typed warning under
  ruling 10.
- **Variation-aware shaping.** `fvar`/`avar`/`HVAR` deltas applied to
  GPOS values are deferred until a corpus document demands them
  (ruling 3).
- **Vertical text shaping** (`vhea`/`vmtx`, `vert`/`vrt2`).
- **Font selection and fallback.** Which face a run gets stays
  `css-fonts-4` §5 matching above the layout crate; the shaper takes one
  face and never picks another.
- **Justification beyond space adjustment.** No `JSTF`, no kashida
  elongation; recorded here so it is a decision, not an omission.

## Design

**The crate boundary, and the reuse decision.** `tinker_pdf_font::Sfnt`
(`crates/tinker-pdf-font/src/sfnt.rs`) already parses the table directory,
`cmap` subtables (formats 0, 4, 6, 12) and `hmtx`. `tinker-pdf-shape` depends
on `tinker-pdf-font` for exactly that — a fourth leaf-to-leaf edge, which
ruling 8 says does not weaken the ruling, and one sfnt parser in the tree
rather than two. The OpenType Layout tables themselves (`GDEF`, `GSUB`,
`GPOS`) are parsed inside `tinker-pdf-shape`, not added to
`tinker-pdf-font`, because the font crate's charter is "the tables metrics
need" and lookups are not metrics; it also puts the lookup fuzz target in
the crate whose code it exercises.

**The pipeline, and where each stage lives.** Per paragraph, in this
order: (1) UAX #9 level resolution over the logical text; (2) itemization
into runs of one script, one level, one face; (3) shaping each run in
logical order — `cmap` mapping, script-specific preprocessing (Arabic
joining classes from `ArabicShaping.txt`, USE cluster formation), GSUB
substitution, GPOS positioning — emitting glyphs carrying a `cluster` byte
offset back into the text; (4) the caller breaks lines over the *logical*
text (layout's existing `uax14::opportunities`, called at
`flow.rs:2498`, is untouched); (5) per line, UAX #9's L rules reorder the
runs, via a pure `reorder(levels, range)` the crate exports. Each glyph is
flagged `safe_to_break`; a line broken at an unsafe offset re-shapes only
the two runs at the boundary, never the paragraph.

**The output type.** `ShapedRun { glyphs: Vec<ShapedGlyph>, direction }`,
where `ShapedGlyph { glyph: u16, cluster: u32, x_advance: i32, y_advance:
i32, x_offset: i32, y_offset: i32 }` in font design units, with
`units_per_em` beside it. Scaling to points happens in the consumer as
`units × size / upem` — multiplication and division are correctly rounded
by IEEE 754, which ruling 4 states is the deterministic side of the line.
Malformed lookups produce a typed warning and an unshaped fallback run
(rulings 1, 2, 10): never a panic, never a silent empty.

**The layout seam.** `tinker-pdf-layout` gains a `Shaper` trait next to
`Metrics` in `metrics.rs` — text and `FontRequest` in, shaped run and
total advance out — and an item measured through a `Shaper` is *never*
also measured through `Metrics::measure`, because metrics.rs itself
documents the two-paths-disagree failure and one path must own a run.
`FixedPitch` keeps serving the structured fuzz generator unchanged; the
facade's `BookMetrics` (`crates/tinker-pdf/src/epub/paint.rs`) implements
`Shaper` over the book's `FaceSet`. Layout stays a leaf: the trait is
plain structs, and `tinker-pdf-layout` gains no dependency edge.

**Creation and forms.** A shaped run maps 1:1 onto
`DocumentBuilder::glyph_run`'s `Glyph` (index plus the characters it
stands for, so `/ToUnicode` and text extraction survive ligatures — ISO
32000-1 9.4.3 positioning, exactly as build.rs already documents). Form
fill replaces `fill.rs`'s single-byte `width_of`/`Tj` path with the shaped
path for any value outside the single-byte range, writing a CID-keyed run
into the synthesized appearance (12.7's variable text), with the existing
quadding and comb logic (12.7.4.3) operating on shaped advances.

**Vendored data.** Beyond the UCD already in
`crates/tinker-pdf-layout/data/ucd` (`LineBreak.txt`,
`EastAsianWidth.txt`, `DerivedGeneralCategory.txt`, `emoji-data.txt`),
`tinker-pdf-shape/data/ucd` vendors, at the *same pinned Unicode version*:
`Scripts.txt`, `DerivedBidiClass.txt`, `BidiBrackets.txt`,
`BidiMirroring.txt`, `ArabicShaping.txt`, `IndicSyllabicCategory.txt`,
`IndicPositionalCategory.txt`, and the conformance files `BidiTest.txt`
and `BidiCharacterTest.txt` — run the way `LineBreakTest.txt`'s 19 338
cases already are in `tests/uax14_conformance.rs`. A version-skew test
compares the two crates' vendored file headers.

**Conformance bar.** Two published sources, both *data* and therefore
admissible under ruling 13: the **aots** corpus (adobe-type-tools' annotated
OpenType spec tests — tiny fonts with expected glyph sequences per GSUB/GPOS
lookup type) and Unicode's **text-rendering-tests** corpus (real fonts with
expected glyph names and positions per script), committed as fixtures with
their licences recorded in `THIRDPARTY.md`. Both carry their expected output
inside the fixture, which is what makes them gates rather than comparisons.

**What no shaping engine here adjudicates.** Ruling 13 rules out running
another shaper and diffing. Those two corpora between them cover a large
part of Latin, Arabic and the Indic lookups, and nothing covers the rest:
for a script with no conformance fixture, this design can show that shaping
ran and that it is deterministic, not that it is right. That remainder is
named per script in [features/fonts.md](../features/fonts.md) as the
capability lands, rather than implied by a percentage.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size (S/M/L/XL) |
|---|---|---|---|
| 1 | `tinker-pdf-shape` crate: `GDEF`/`GSUB`/`GPOS` parsing, coverage/classdef, extension and chaining-context lookups | aots fixture suite green per lookup type in `tests/aots.rs`; `fuzz_shape` (arbitrary face bytes + text) in the nightly fuzz job with zero crashes; `#![deny(clippy::float_arithmetic)]` compiles | L |
| 2 | Default shaper: `cmap` via `tinker_pdf_font::Sfnt`, `ccmp`/`liga`, GPOS `kern`/`mark`/`mkmk`, cluster mapping | text-rendering-tests CMAP/GSUB/GPOS sections pass as committed fixtures; shaping fingerprints committed and reproduced by the determinism CI legs ([features/determinism.md](../features/determinism.md)) on all four targets | M |
| 3 | UAX #9: level resolution, bracket pairs, per-line `reorder`, mirroring | `BidiTest.txt` and `BidiCharacterTest.txt` conformance tests green through the same entry point consumers call, in the `uax14_conformance.rs` pattern | M |
| 4 | Arabic-script shaper: joining forms, `rlig`, cursive attachment | Every text-rendering-tests Arabic section passes, with the count of sections asserted so a shrinking suite cannot read as a passing one; injected wrong joining-class and wrong-anchor defects each caught by a counted assertion | L |
| 5 | USE shaper for Indic and Southeast Asian scripts | Every text-rendering-tests USE section passes, counted; scripts with no fixture are listed by name in the feature doc as shaped-but-unverified rather than counted as done | XL |
| 6 | Layout consumer: `Shaper` trait, itemize→shape→break→reorder wiring, `BookMetrics` implements it | An Arabic EPUB fixture paginates with joined forms and RTL line order, pinned by a render fingerprint; the reftest pairs of [render-verification](render-verification.md)'s EPUB tier gain an RTL pair; the one-path-owns-a-run rule asserted by test | L |
| 7 | Creation consumer: shaped runs through `DocumentBuilder::glyph_run` | Round-trip test: Arabic string built into a PDF, text extraction returns the original string; the strict structural validator clean on the output; render fingerprint committed | M |
| 8 | Forms consumer: `fill.rs` `text_appearance` shapes non-Latin values | Fill an Arabic value into a text field: appearance renders joined (fingerprint), `/V` round-trips through extraction, the strict validator clean; `fill.rs`'s `escape` no longer writes `?` for characters above the single-byte range, and [features/forms.md](../features/forms.md) records the change | M |

## Dependencies

- `tinker_pdf_font::Sfnt` table directory and `cmap` (exists; the new
  leaf-to-leaf edge is the one addition).
- `DocumentBuilder::glyph_run` and `Glyph` in
  `crates/tinker-pdf-cos/src/build.rs` (exist; milestone 7 is wiring, not
  new machinery).
- The determinism fingerprint suite and CI legs
  ([features/determinism.md](../features/determinism.md)) — milestone 2
  extends them, the macOS/wasm legs must be observed first (Tier 1).
- The conformance-fixture discipline of
  [verification.md](../verification.md): a fixture carries its own expected
  output, and a suite whose case count can shrink silently is not a suite —
  so the counts are asserted.
- EPUB `@font-face` currently refuses WOFF/WOFF2 by name
  ([features/epub.md](../features/epub.md)); real Arabic EPUBs often ship
  WOFF, so milestone 6's fixture uses a raw sfnt face until that Tier 4
  row closes — the two items are independent but meet in the demo.

## Risks

| Risk | Mitigation |
|---|---|
| Indic/USE correctness is effectively unbounded — the reason this is the roadmap's largest single item | USE's data-driven cluster model rather than per-script shapers; the Unicode and aots conformance files gate what they cover; scripts scheduled by corpus evidence under ruling 3, not completeness |
| **Ruling 13 leaves scripts without a conformance fixture unadjudicated.** Shaping can be deterministic, plausible and wrong, and nothing fires | Those scripts are listed by name in the feature doc as shaped-but-unverified; the number that matters is how many sections pass, asserted with its count, not a percentage against another engine. Not closed |
| Two measurement paths disagree — the exact failure `metrics.rs` warns about, now with three paths (`Metrics`, `Shaper`, the renderer) | One path owns a run, asserted by test (milestone 6); creation writes the shaper's own advances through `glyph_run`, so what was measured is what is drawn |
| Re-shaping at unsafe line breaks goes quadratic or breaks Arabic joining across lines | `safe_to_break` flags per glyph; re-shape only the boundary runs; a pinned test breaks inside a joined word under `word-break: break-all` and asserts forms and cost |
| Float creep silently breaks the cross-target contract | Integer font units end to end; `clippy::float_arithmetic` denied in the crate; fingerprints on all four targets from milestone 2, divergence is build-stopping |
| Unicode version skew between the two crates' vendored UCD | One pinned version, asserted by a header-comparison test; upgrades touch both `data/ucd` trees in one commit |
| Malformed `GSUB`/`GPOS` from untrusted faces (cycles, out-of-range offsets) | Ruling 1 via the fuzz target from milestone 1; lookup recursion depth and total-substitution budgets in the crate's `limits`, typed warnings on refusal per rulings 2 and 10 |
