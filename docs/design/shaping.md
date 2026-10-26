# Text shaping

Every place this engine *produces* text — a `DocumentBuilder` page, a
form-fill appearance stream, an EPUB paginated into a book — sets Arabic,
Indic and ligature-dependent scripts the way a reader of those scripts
expects, through a leaf crate that takes face bytes and a paragraph of text
and returns positioned glyph runs, bit-identically on every target. Shaping
covers the producing half of the engine only.

## Scope

- **Where shaping applies.** PDF content streams position pre-shaped
  glyphs, and `crates/tinker-pdf-layout/src/metrics.rs` states "No kerning
  and no shaping" on the `Metrics` trait by design. That reasoning holds for
  rendering existing PDFs — the producer positioned every glyph, and
  re-shaping them would be wrong — and fails wherever *this engine is the
  producer*: `DocumentBuilder` text, the appearance streams
  `text_appearance` builds in `crates/tinker-pdf-cos/src/fill.rs`, and EPUB
  layout, where `Metrics::advance` is per character and a sum over a run.
  The Universal Shaping Engine's remainder is [ROADMAP](../ROADMAP.md) row
  FT-08.
- **A leaf crate, `tinker-pdf-shape`**, under ruling 8
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
- **Consumers**: `tinker-pdf-layout` (a `Shaper` seam beside the `Metrics`
  trait), `DocumentBuilder::glyph_run` (`crates/tinker-pdf-cos/src/build.rs`,
  which takes caller-positioned glyphs and a `Glyph` that pairs a glyph
  index with the text it stands for — the ligature-to-`/ToUnicode` case is
  documented on that type), and `fill.rs` form appearances.
- **Determinism as a contract** (ruling 4): all shaping arithmetic in
  integer font design units — `FWORD`s in, `i32` positions out — so output
  is bit-identical across linux, windows, macos and wasm with no float in
  the crate at all.

## Non-goals

- **Shaping while rendering existing PDFs.** `TJ` arrays are honored as
  written. Nothing in `tinker-pdf-render` calls this crate. Reordering is not
  shaping: ruling 14 puts an extracted right-to-left line into logical order
  with this crate's UAX #9 (`bidi::logical_order`), called from the facade,
  so `tinker-pdf-content` has no edge here and no glyph is re-shaped.
- **AAT (`morx`) and Graphite tables.** OpenType Layout only; a face
  carrying only `morx` shapes as if unshaped. [ROADMAP](../ROADMAP.md) rows
  FT-07c and FT-07d.
- **Variation-aware shaping.** `fvar`/`avar`/`HVAR` deltas are not applied
  to GPOS values. [ROADMAP](../ROADMAP.md) row FT-07a, with variable fonts
  themselves FT-05.
- **Vertical text shaping** (`vhea`/`vmtx`, `vert`/`vrt2`).
  [ROADMAP](../ROADMAP.md) row FT-07b.
- **Font selection and fallback.** Which face a run gets stays
  `css-fonts-4` §5 matching above the layout crate; the shaper takes one
  face and never picks another.
- **Justification beyond space adjustment.** No `JSTF`, no kashida
  elongation. [ROADMAP](../ROADMAP.md) row FT-07e.

## Design

**The crate boundary, and the reuse decision.** `tinker_pdf_font::Sfnt`
(`crates/tinker-pdf-font/src/sfnt.rs`) parses the table directory, `cmap`
subtables (formats 0, 4, 6, 12) and `hmtx`. `tinker-pdf-shape` depends on
`tinker-pdf-font` for exactly that — a leaf-to-leaf edge, which ruling 8
says does not weaken the ruling, and one sfnt parser in the tree rather than
two. The OpenType Layout tables themselves (`GDEF`, `GSUB`, `GPOS`) are
parsed inside `tinker-pdf-shape`, not added to `tinker-pdf-font`, because
the font crate's charter is "the tables metrics need" and lookups are not
metrics; it also puts the lookup fuzz target in the crate whose code it
exercises.

**The pipeline, and where each stage lives.** Per paragraph, in this
order: (1) UAX #9 level resolution over the logical text; (2) itemization
into runs of one script, one level, one face; (3) shaping each run in
logical order — `cmap` mapping, script-specific preprocessing (Arabic
joining types from `DerivedJoiningType.txt`, USE cluster formation), GSUB
substitution, GPOS positioning — emitting glyphs carrying a `cluster` byte
offset back into the text; (4) the caller breaks lines over the *logical*
text (layout's `uax14::opportunities`); (5) per line, UAX #9's L rules
reorder the runs, via a pure `reorder(levels, range)` the crate exports.
Each glyph is flagged `safe_to_break`; a line broken at an unsafe offset
re-shapes only the two runs at the boundary, never the paragraph.

**The output type.** `ShapedRun { glyphs: Vec<ShapedGlyph>, direction }`,
where `ShapedGlyph { glyph: u16, cluster: u32, x_advance: i32, y_advance:
i32, x_offset: i32, y_offset: i32 }` in font design units, with
`units_per_em` beside it. Scaling to points happens in the consumer as
`units × size / upem` — multiplication and division are correctly rounded
by IEEE 754, which ruling 4 states is the deterministic side of the line.
Malformed lookups produce a typed warning and an unshaped fallback run
(rulings 1, 2, 10): never a panic, never a silent empty.

**The layout seam.** `tinker-pdf-layout` has a `Shaper` trait next to
`Metrics` in `metrics.rs` — text and `FontRequest` in, shaped run and
total advance out — and an item measured through a `Shaper` is *never*
also measured through `Metrics::measure`, because metrics.rs itself
documents the two-paths-disagree failure and one path must own a run.
`FixedPitch` serves the structured fuzz generator; the facade's
`BookMetrics` (`crates/tinker-pdf/src/epub/paint.rs`) implements `Shaper`
over the book's `FaceSet`. Layout stays a leaf: the trait is plain structs,
and `tinker-pdf-layout` has no dependency edge for it. The seam takes a
run's **context** — `Shaper::shape_in`, the text and face request of its
painted neighbours on its line — because a span is a run of its own and a
shaper's decisions cross it: measured alone, a joined form or a kerned pair
across a span would leave its difference between two runs. The default
ignores the context, so a provider with no shaping across runs measures
every run alone.

**Creation and forms.** A shaped run maps 1:1 onto
`DocumentBuilder::glyph_run`'s `Glyph` (index plus the characters it
stands for, so `/ToUnicode` and text extraction survive ligatures — ISO
32000-1 9.4.3 positioning, as build.rs documents). Form fill takes the
shaped path rather than `fill.rs`'s single-byte `width_of`/`Tj` path for a
value outside the single-byte range, writing a CID-keyed run into the
synthesized appearance (12.7's variable text), with the quadding and comb
logic (12.7.4.3) operating on shaped advances.

**Vendored data.** Beyond the UCD in `crates/tinker-pdf-layout/data/ucd`
(`LineBreak.txt`, `EastAsianWidth.txt`, `DerivedGeneralCategory.txt`,
`emoji-data.txt`), `tinker-pdf-shape/data/ucd` vendors, at the *same pinned
Unicode version*: `Scripts.txt`, `DerivedBidiClass.txt`, `BidiBrackets.txt`,
`BidiMirroring.txt`, `DerivedJoiningType.txt`, `IndicSyllabicCategory.txt`,
`IndicPositionalCategory.txt`, and the conformance files `BidiTest.txt` and
`BidiCharacterTest.txt` — run the way `LineBreakTest.txt` is in
`tests/uax14_conformance.rs`. A version-skew test compares the vendored
trees' file headers.

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
named per script in [features/fonts.md](../features/fonts.md), rather than
implied by a percentage.

## Milestones

1. The `tinker-pdf-shape` crate: `GDEF`/`GSUB`/`GPOS` parsing,
   coverage/classdef, extension and chaining-context lookups. Exit: the aots
   fixture suite per lookup type in `tests/aots.rs`; the `shape` fuzz target
   (arbitrary face bytes and text) in the nightly fuzz job;
   `#![deny(clippy::float_arithmetic)]` compiles.
2. The default shaper: `cmap` via `tinker_pdf_font::Sfnt`, `ccmp`/`liga`,
   GPOS `kern`/`mark`/`mkmk`, cluster mapping. Exit: the text-rendering-tests
   CMAP/GSUB/GPOS sections as committed fixtures; shaping fingerprints
   reproduced by the determinism CI legs
   ([features/determinism.md](../features/determinism.md)) on all four
   targets.
3. UAX #9: level resolution, bracket pairs, per-line `reorder`, mirroring.
   Exit: `BidiTest.txt` and `BidiCharacterTest.txt` conformance tests through
   the same entry point consumers call, in the `uax14_conformance.rs`
   pattern.
4. The Arabic-script shaper: joining forms, `rlig`, cursive attachment.
   Exit: every text-rendering-tests Arabic section, with the count of
   sections asserted so a shrinking suite cannot read as a passing one.
5. The USE shaper for Indic and Southeast Asian scripts. Exit: every
   text-rendering-tests USE section, counted; scripts with no fixture listed
   by name in the feature doc as shaped-but-unverified rather than counted as
   done.
6. The layout consumer: `Shaper` trait, itemize→shape→break→reorder wiring,
   `BookMetrics` implements it. Exit: an Arabic EPUB fixture paginates with
   joined forms and RTL line order, pinned by a render fingerprint; the
   reftest pairs of [render-verification](render-verification.md)'s EPUB
   tier include an RTL pair; the one-path-owns-a-run rule asserted by test.
7. The creation consumer: shaped runs through `DocumentBuilder::glyph_run`.
   Exit: an Arabic string built into a PDF extracts as the original string;
   the strict structural validator clean on the output; a render
   fingerprint.
8. The forms consumer: `fill.rs` `text_appearance` shapes non-Latin values.
   Exit: an Arabic value filled into a text field renders joined
   (fingerprint), `/V` round-trips through extraction, the strict validator
   clean.

## Dependencies

- `tinker_pdf_font::Sfnt` table directory and `cmap` (the leaf-to-leaf edge
  is the one addition).
- `DocumentBuilder::glyph_run` and `Glyph` in
  `crates/tinker-pdf-cos/src/build.rs` (milestone 7 is wiring, not new
  machinery).
- The determinism fingerprint suite and CI legs
  ([features/determinism.md](../features/determinism.md)), which milestone 2
  extends.
- The conformance-fixture discipline of
  [verification.md](../verification.md): a fixture carries its own expected
  output, and a suite whose case count can shrink silently is not a suite —
  so the counts are asserted.

## Risks

| Risk | Mitigation |
|---|---|
| Indic/USE correctness is effectively unbounded — the reason this is the roadmap's largest single item | USE's data-driven cluster model rather than per-script shapers; the Unicode and aots conformance files gate what they cover; scripts scheduled by corpus evidence under ruling 3, not completeness. The mitigation runs out where Kannada's `SHKNDA-2/1` and Tai Tham's `SHLANA-2/6` are the same cluster shape and want opposite glyph orders: no property this crate reads separates them, and a rule that satisfies one fails cases of the other — the per-section numbers are in `crates/tinker-pdf-shape/src/universal.rs` |
| **Ruling 13 leaves scripts without a conformance fixture unadjudicated.** Shaping can be deterministic, plausible and wrong, and nothing fires | Those scripts are listed by name in the feature doc as shaped-but-unverified; the number that matters is how many sections pass, asserted with its count, not a percentage against another engine |
| Two measurement paths disagree — the exact failure `metrics.rs` warns about, with three paths (`Metrics`, `Shaper`, the renderer) | One path owns a run, asserted by test (milestone 6); creation writes the shaper's own advances through `glyph_run`, so what was measured is what is drawn |
| Re-shaping at unsafe line breaks goes quadratic or breaks Arabic joining across lines | `safe_to_break` flags per glyph; re-shape only the boundary runs; a pinned test breaks inside a joined word under `word-break: break-all` and asserts forms and cost |
| Float creep silently breaks the cross-target contract | Integer font units end to end; `clippy::float_arithmetic` denied in the crate; fingerprints on all four targets from milestone 2, divergence is build-stopping |
| Unicode version skew between the crates' vendored UCD | One pinned version, asserted by a header-comparison test; upgrades touch every `data/ucd` tree in one commit |
| Malformed `GSUB`/`GPOS` from untrusted faces (cycles, out-of-range offsets) | Ruling 1 via the fuzz target from milestone 1; lookup recursion depth and total-substitution budgets in the crate's `limits`, typed warnings on refusal per rulings 2 and 10 |
