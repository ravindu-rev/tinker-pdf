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
- **Plans and coverage indexes, kept per face** (tier P's P-26): the lookups
  each stage resolves, and for each lookup the glyphs its subtables'
  coverage holds, built once per face and feature set and kept beside the
  face's bytes, so a paragraph pays for its table walks once rather than
  once per run and per glyph — *Lookup plans and coverage indexes* below.

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

### Lookup plans and coverage indexes

**What is asked per glyph.** `Shaper::shape` resolves each stage's lookups
through `LayoutTable::lookups_for_masked` — a walk of the script,
language-system and feature lists — once per run and stage, and the runner
then visits every planned lookup at every position its skipper admits:
`Runner::apply_at` charges the operation budget, follows each subtable's
extension indirection (`resolve`) and asks the subtable's first coverage
table, by binary search, whether the glyph is one it covers. Over a
1 000-character paragraph that question is asked per glyph per lookup, and
in a Latin, Arabic or Kannada face the answer is no at most positions.

**A plan** is one `(table, script, language, stage)` resolution: the
stage's requested `(feature, mask)` list, the script tag and language
`Shaper::shape` asks with, and the `(lookup, mask)` pairs
`lookups_for_masked` returns for them. It is a pure function of the table's
bytes and that key, so a plan built once answers every later run that asks
with the same key — and the stage list a shaper's settings make
(`with_settings`, `with_features`) is part of the key, so one face's plans
serve every feature set its callers ask for.

**An index** is kept for each planned lookup, from its subtables: per
subtable, the glyphs of the coverage table that subtable reads first at a
position, as its kind reads it (the table below); and their union for the
lookup. Each set is a superset of what that coverage's binary search can
find — every glyph a format 1 array lists, every range a format 2 table
states, read as far as the table's bytes go and sorted and merged where the
table is not, never trusted to be. Its form is a bitmap over the 64-glyph
blocks from its lowest glyph to its highest: a subtraction, a load and a
shift to ask, and at most 1 024 words for the whole 16-bit glyph space. A
lookup of one subtable keeps the union alone.

| Subtable, extensions followed | The coverage it reads first |
| --- | --- |
| `GSUB` 1; `GPOS` 1 and 2 | the `Offset16` at 2 |
| `GSUB` 2, 3, 4 and 8; `GPOS` 3, 4, 5 and 6 | the `Offset16` at 2, in format 1; another format never applies |
| Context (`GSUB` 5, `GPOS` 7) and chained context (`GSUB` 6, `GPOS` 8), formats 1 and 2 | the `Offset16` at 2 |
| The same, format 3 | the first input coverage, with the input count kept |
| An unreadable subtable, or a format no reader takes | none: it never applies |

**What the runner does with it.** Application order does not change: each
lookup still runs over the whole buffer, in list order, and every position
its skipper admits is still visited; the index says only where a subtable
cannot apply. At a position whose glyph no subtable of the lookup holds, the
runner charges the operation budget what `apply_at` would have charged there
— one per subtable, at least one — and moves on, resolving and probing
nothing. At a position the union holds it tries the subtables in order, as
`apply_at` does, with the same charge before each, and resolves and probes
only those whose own set holds the glyph. Every output is the one probing
gives: the glyphs, the positions and the warnings, the operation budget
reached at the same place.

**Where the index cannot answer.** A probe that warns whatever the glyph is
never passed over. A lookup with a subtable of a type this crate does not
implement (`UnknownLookupType`), or whose extension warns at every probe
(`ExtensionChained`, `MalformedSubtable`), has no index and is probed at
every position. Two warnings depend on the run's `Limits`, so an index keeps
what decides them and is set aside for a run whose limits would fire them:
the extension indirections its subtables take (`ExtensionTooDeep`, past
`max_extension_depth`) and its longest format 3 input (`ContextTooLong`, past
`max_context_length`). A lookup a contextual rule runs at one position
(`Runner::recurse`) is probed as before.

**Where they live.** `Shaper` stays `Copy` and is still made afresh at every
call site. What is kept is a `Plans`, owned beside the face's bytes by
whoever owns them, which every shaper made over the face borrows
(`Shaper::with_plans`). A `Plans` holds no borrow — lookup indices, masks and
bitmaps — so it can sit in the same struct as the bytes it describes, where a
kept `Shaper`, which borrows its face, could not. Entries are built when
first asked: a plan when a run first asks for its key, and with it the index
of each of its lookups not yet indexed — once per face, shared by every plan
that names the lookup. The store sits behind a `Mutex`, so `Plans` and
`Shaper` stay `Send` and `Sync`, and each entry is an `Arc` handed out under
the lock and read outside it. A `Plans` answers for the face it was first
used with: it keeps the address and length of that face's `GSUB` and `GPOS`
and starts again if a shaper hands it other tables, so one used with the
wrong face costs time and never output. A shaper with no `Plans` resolves
each stage per run and probes every position, as it always has; `Plans`
counts what it built (`plans_built`, `indexes_built`), the counted value the
exits read.

**The bound** (ruling 1). Everything a `Plans` holds, and every coverage
byte it reads to build an index, is charged against `Limits::max_index_bytes`
(8 MiB by default): a plan its pairs and its key, an index each subtable's
set, the union and a fixed cost per subtable, a coverage its entries as read
— two bytes a format 1 glyph, six a format 2 range. Past the bound a plan is
resolved per run and a lookup probed at every position, exactly as with no
`Plans`, so a hostile face that names one 65 535-range coverage from every
subtable of every lookup costs at most the bound's worth of reading, once per
face. The bound charged is the building shaper's `Limits`, so a face's
`Plans` holds no more than the largest bound any shaper over it carried.

**Why this reverses `common.rs`.** `common.rs` builds no map at parse,
because a map costs an allocation per face for a question most callers ask
twice. A plan and an index answer a question asked per run and per glyph: a
paragraph asks the same few hundred bytes of tables thousands of times, and
the bitmap answers in the time the binary search takes to read its first
entry. They are still not built at parse: a face nobody shapes through, and
a lookup no plan names, cost nothing.

**The callers.** Each keeps one `Plans` per face for as long as it keeps the
face's bytes, and makes its shapers with it.

- `crates/tinker-pdf/src/shaping.rs`: `Run` carries the shaper its caller
  made, in place of the face, so a document build that writes many runs in
  one face makes each with that face's `Plans`; the EPUB SVG painter passes
  its embedded face's.
- `crates/tinker-pdf/src/epub/paint.rs`: each `EmbeddedFace` keeps its
  `Plans`, so measuring a book (`BookMetrics`), painting it and its SVG text
  share one per face; `BookCost` counts the plans and indexes the book's
  faces built (`shaping_plans`, `shaping_indexes`).
- `crates/tinker-pdf-cos/src/fill.rs`: `Shapeable` keeps one beside the
  program it decoded for the appearance, so every line of a value reuses it.

## Milestones

1. The `tinker-pdf-shape` crate: `GDEF`/`GSUB`/`GPOS` parsing,
   coverage/classdef, extension and chaining-context lookups. Exit: the aots
   fixture suite per lookup type in `tests/aots.rs`; the `shape` fuzz target
   (arbitrary face bytes and text), run under `cargo fuzz` on Linux as a
   change's local proof; `#![deny(clippy::float_arithmetic)]` compiles.
2. The default shaper: `cmap` via `tinker_pdf_font::Sfnt`, `ccmp`/`liga`,
   GPOS `kern`/`mark`/`mkmk`, cluster mapping. Exit: the text-rendering-tests
   CMAP/GSUB/GPOS sections as committed fixtures; shaping fingerprints
   reproduced on every measured target
   ([features/determinism.md](../features/determinism.md)): Linux and Windows
   in CI's tests, `wasm32-wasip1` under wasmtime locally.
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
9. Plans and coverage indexes (P-26.3): `Plans`, `Shaper::with_plans`,
   `Limits::max_index_bytes`, and the runner passing the positions an index
   does not hold. Exit, in `apply.rs`:
   `a_glyph_no_subtable_covers_is_never_probed` — a 1 000-character paragraph
   shaped through Liberation Serif, `TestShapeAran.ttf` and
   `NotoSansKannada-Regular.ttf`, each with one `Plans`: every planned lookup
   has an index, the subtables resolved and probed at each position
   (test-only counters) are at most the index entries holding its glyph and
   none where none holds it, and the output is the one shaping with no
   `Plans` gives; `a_plan_is_built_once_per_face_and_feature_set` — 100 runs
   through one `Plans` build each plan and index once, and a second feature
   set builds its own plans and no lookup's index twice; the index holds
   every glyph a probe applies or warns at, for each subtable kind, and is
   set aside under the limits that would fire its warnings; `fingerprints.rs`'s
   `shaping_twice_gives_the_same_answer` runs through one kept `Plans` per
   face. `aots.rs`, `synthetic.rs`, `text_rendering.rs` and `fingerprints.rs`
   are unchanged (`cargo test -p tinker-pdf-shape`).
10. The callers keep one per face (P-26.4): `shaping.rs`'s `Run` carries a
    shaper, `EmbeddedFace` and `fill.rs`'s `Shapeable` keep a `Plans`, and
    `BookCost` counts what a book's faces built. Exit: `epub_shaped.rs`'s
    `a_face_is_indexed_once_per_book` — paginating a generated three-chapter
    Arabic book in one embedded face builds plans and indexes, and the same
    number as the book's first chapter alone, so none is built again per
    chapter, page or line; `epub_shaped.rs`, `shaped_creation.rs`,
    `shaped_forms.rs` and the `epub` fingerprint unchanged, in the default
    build and with `--features bundled-fonts`.

## Dependencies

- `tinker_pdf_font::Sfnt` table directory and `cmap` (the leaf-to-leaf edge
  is the one addition).
- `DocumentBuilder::glyph_run` and `Glyph` in
  `crates/tinker-pdf-cos/src/build.rs` (milestone 7 is wiring, not new
  machinery).
- The determinism fingerprint suite and the targets it is measured on
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
| Float creep silently breaks the cross-target contract | Integer font units end to end; `clippy::float_arithmetic` denied in the crate; fingerprints on every measured target from milestone 2 — Linux and Windows in CI's tests, `wasm32-wasip1` locally — and a divergence fails them |
| Unicode version skew between the crates' vendored UCD | One pinned version, asserted by a header-comparison test; upgrades touch every `data/ucd` tree in one commit |
| Malformed `GSUB`/`GPOS` from untrusted faces (cycles, out-of-range offsets) | Ruling 1 via the fuzz target from milestone 1; lookup recursion depth and total-substitution budgets in the crate's `limits`, typed warnings on refusal per rulings 2 and 10 |
| An index says a subtable cannot apply where a probe would have applied it or warned, and shaping changes with nothing to say so | Each set is a superset of what its subtable's first read finds, by the table in the design; a probe that warns whatever the glyph is never passed over, and the two warnings the limits decide set the index aside; a passed position is charged what its probe would have cost. Unit tests hold the index to the probe for every subtable kind, the `shape_text` fuzz target holds a kept `Plans` to none on arbitrary faces, and the shaping fingerprints do not move |
| Building the index is the denial of service a hostile face aims at | `Limits::max_index_bytes` bounds what a `Plans` holds and reads, and past it a lookup is probed as it always was; nothing is built at parse, and nothing for a lookup no plan names |
| A `Plans` kept with one face is handed another, and answers with the first face's lookups | It keeps the address and length of its face's `GSUB` and `GPOS` and starts again on any other, so a misplaced `Plans` costs time and never output |
