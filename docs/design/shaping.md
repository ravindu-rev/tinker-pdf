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
- **Vertical shaping** (FT-07b): a top-to-bottom direction; each character's
  orientation by UAX #50's `Vertical_Orientation` in the itemizer; the
  vertical feature plan for an upright run; vertical metrics from `vhea`,
  `vmtx` and `VORG`; a sideways run — Mongolian among them — shaped along a
  line and marked for rotation; `GPOS` down a column; and a form field's
  column moved onto that plan — *Vertical shaping* below.

## Non-goals

- **Shaping while rendering existing PDFs.** `TJ` arrays are honored as
  written. Nothing in `tinker-pdf-render` calls this crate. Reordering is not
  shaping: ruling 14 puts an extracted right-to-left line into logical order
  with this crate's UAX #9 (`bidi::logical_order`), called from the facade,
  so `tinker-pdf-content` has no edge here and no glyph is re-shaped.
- **AAT (`morx`) and Graphite tables.** OpenType Layout only; a face
  carrying only `morx` shapes as if unshaped. [ROADMAP](../ROADMAP.md) rows
  FT-07c, whose design is the *AAT shaping* section below, and FT-07d.
- **Variation-aware shaping.** `fvar`/`avar`/`HVAR` deltas are not applied
  to GPOS values. [ROADMAP](../ROADMAP.md) row FT-07a, with variable fonts
  themselves FT-05.
- **Laying a line out vertically.** `writing-mode` and vertical lines in
  books are [ROADMAP](../ROADMAP.md) row CD-01's, and nothing here lays a
  line out vertically: breaking a column, the progression of columns across a
  page, where a sideways run's baseline stands across its column (a face's
  `BASE` or `vhea`), and `text-combine-upright` are layout's. The shaper
  shapes the runs of a column and says which way each stands.
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
`IndicPositionalCategory.txt`, `VerticalOrientation.txt` (*Vertical
shaping*, below), and the conformance files `BidiTest.txt` and
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

### Vertical shaping

**Two ways to stand in a column.** A vertical paragraph is set top to
bottom, and each of its characters stands in the column one of two ways:
**upright**, in the orientation the code charts show, or **sideways**,
rotated 90° clockwise. UAX #50's `Vertical_Orientation` gives each
character's default, from `VerticalOrientation.txt` at the pinned Unicode
version beside the shaper's other UCD files. Its four values are UAX #50's
table 1: `U` upright, `R` sideways, and `Tu` and `Tr` characters a face is
expected to give a vertical form, with an upright fallback and a rotated one.
`U`, `Tu` and `Tr` are set upright and `R` sideways, which is CSS Writing
Modes 3 §5.1.2's rule for mixed orientation. A `Tr` character whose face has
no vertical form is still set upright: §5.1.1 says a layout engine "is not
expected to" synthesize the rotated fallback, and the itemizer, which sees
text and no face, could not.

**A grapheme cluster's orientation.** UAX #50 §3.2.1 orients a grapheme
cluster as a whole: its first character's value, or `U` where the cluster
holds an enclosing mark. The cluster is drawn from what the crate vendors: a
character continues the one before it when its `General_Category` is a mark
(`Mn`, `Mc`, `Me`, from `UnicodeData.txt`'s field 2) or its `Script` is
`Inherited`, which takes in `ZWJ`, `ZWNJ` and the variation selectors. That
is a base and the marks after it, the legacy grapheme cluster's core; the
rest of UAX #29 is not computed (*Risks*).

**Itemization.** `shape::itemize_vertical` cuts a vertical paragraph into
runs of one script, one embedding level and one orientation, resolving
`Common` and `Inherited` scripts as `itemize` does; `itemize`, a line's, is
unchanged. A `Run` carries its `Orientation` — `Horizontal` along a line,
`Upright` or `Sideways` in a column — and an upright run's direction is
`Direction::TopToBottom` whatever its level, while a sideways run's is its
level's, because it is shaped along a line. A run's fields are public, so a
caller that draws every glyph upright sets every run upright itself (*A form
field's column*, below).

**The upright plan.** An upright run takes the plan its text takes along a
line, changed where the OpenType feature registry and CSS say so:

- **`GSUB`.** `vert` joins the last stage: the registry has it "active by
  default in vertical writing mode". The default set stays on — `locl`,
  `ccmp`, `rlig`, `liga`, `clig` and `calt`, none of which the registry
  restricts to horizontal text. A run whose text joins takes the default
  plan rather than the joining one, because §5.1.1 shapes a cursive script
  set upright in its isolated forms; a Brahmic run keeps the Universal
  Shaping Engine's stages, `vert` in the last.
- **`vert` and `vrt2`.** The registry gives the two to two kinds of layout
  engine: `vert` (with `vrtr` for sideways runs) to one that rotates sideways
  runs itself, as this one does, and `vrt2` "in lieu of" both to one that
  depends on the face to supply pre-rotated glyphs. `vert` is "a subset" of
  `vrt2`, which "overrides" it, and the two are never applied together. So
  `vrt2` is off; turned on through `with_settings`, it takes `vert`'s place
  where the face carries `vrt2` for the run's script and language, and where
  the face does not, `vert` stays, because nothing overrides it. CSS §5.1.2
  does not use `vrt2` at all.
- **`GPOS`.** `vkrn` in place of `kern` — the registry's `kern` "is not
  intended for use in vertical text layout", and `vkrn` "should be active by
  default" in most of it — and `valt`, which the registry has "active by
  default in vertical-setting contexts"; `abvm`, `blwm`, `dist`, `curs`,
  `mark` and `mkmk` as along a line. `vhal`, `vpal`, `vapk`, `vchw` and
  `vkna` are off, as the registry has them, and are turned on through
  `with_settings`. `vchw` is the one the registry makes the caller's — on
  wherever the layout engine does no CJK text layout of its own — which a
  leaf crate cannot know of its caller; it is off for the reason `chws` is
  off along a line.

The registry's interactions among them are applied. The glyph-height
features `valt`, `vhal`, `vpal` and `vchw` exclude one another, so a caller
who turns on one of the last three withdraws `valt`. Where `valt` applies —
the face carries it for the run's script and language — or `vhal` or `vchw`
does, `vkrn` and `vapk` are withdrawn for the run unless the caller turned
them on; `vpal` keeps them. One interaction is not applied: `vkrn` standing
off, by default, from the glyphs a face's `vpal` covers where `vpal` is off
and the face has no `vapk`. That is the exception `kern` has with `palt`,
which a line does not apply either, and
[features/fonts.md](../features/fonts.md) names both. A caller who names its
own features (`with_features`) is taken at its word in a column as along a
line, and no vertical feature is added; `with_settings` applies over the
upright plan as over any other.

**The sideways plan.** A sideways run is shaped along a line — §5.1.1's
"horizontal metrics and composition", where "vertical typesetting features
are not used" — by the plan its text takes there, Mongolian's joining included,
with `vrtr` added to the last stage (the registry: "always active by default
for sideways runs in vertical writing mode"). `vert` and `vrt2` are never
applied to it, whatever the settings: its glyphs are turned by its consumer,
and a vertical form would be turned twice. Its advances are `hmtx`'s, its
direction is its level's, and it is marked `Orientation::Sideways`: its
consumer turns it 90° clockwise to stand in the column, so its pen runs down
the column and its glyphs' tops face the column's right. Where its baseline
stands across the column is layout's.

**Vertical metrics.** `tinker_pdf_font::Sfnt` reads them, since the font
crate's charter is the tables metrics need: `vhea`'s `numOfLongVerMetrics`,
`vmtx`'s advance heights and top side bearings — the last advance repeating
past the count, as `hmtx`'s does — and `VORG`'s default and per-glyph
origins. The vertical origin's y is the `vmtx` clause's: the glyph's top
side bearing plus the top of its bounding box.

- **TrueType outlines** (a `glyf` table): the box is the glyph header's
  `yMax`, read by `glyf::bounds`; a glyph with no outline has the origin
  point for its box, so its vertical origin is its top side bearing. A `VORG`
  beside `glyf` is ignored, as the `VORG` clause requires.
- **CFF outlines** (a `CFF` table): `VORG`'s record for the glyph, or its default,
  which is the clause's own shortcut; without a `VORG`, the top of the
  glyph's outline as its charstring draws it, control points included and
  rounded to a unit — the bounding box wherever the outline has a point at
  its topmost extreme, which is where a face's designer puts one.

A face without them answers `None`: no `vhea` or no `vmtx`, a count of zero,
or a read past the table's end. The shaper then synthesizes the glyph's
metrics, which §5.1.1 requires of a layout engine and leaves the heuristic
of: one em down, the vertical origin `880/1000` of an em above the baseline
and half the advance width in — the ideographic em box, and the default a
PDF CIDFont's `/DW2` states (ISO 32000-2 9.7.4.3) — and the run records
`Warning::VerticalMetricsSynthesized` once.

**The convention.** Every number is still a font design unit with y up, and
a glyph is still drawn at the pen plus its offset.

- **Advances.** Down a column the pen moves by `y_advance`, the negative of
  the advance height, and `x_advance` is zero. `ShapedRun::advance` is the
  run's length along its own axis: the sum of `x_advance` along a line, the
  sum of `y_advance` negated down a column.
- **Offsets.** A glyph's offset is where its *horizontal* origin — the origin
  its outline, its anchors and every `GPOS` placement are written in — stands
  from the pen: `(−origin_x, −origin_y)`, so that the vertical origin is at
  the pen. `origin_x` is half the glyph's advance width, rounded down: the
  `vmtx` clause leaves the vertical origin's x unspecified, and 9.7.4.3 puts
  a PDF glyph's at half its width. A consumer that draws each glyph at the
  pen plus its offset draws a column without knowing what a vertical origin
  is, and `Shaper::vertical_metrics` reports the advance and origin a glyph
  was given, for a consumer that places glyphs by metrics of its own.
- **Value records.** A placement adds to the offset in every direction: the
  `GPOS` clause writes placements in the glyph's own coordinates "regardless
  of the writing direction". `YAdvance` lengthens the advance — "for a
  top-to-bottom feature, to increase the advance height of a glyph by 100,
  the YAdvance = 100" — so a column subtracts it from `y_advance`; `XAdvance`,
  "only used for horizontal layout", is not applied in a column. Along a line
  nothing changes: aots's `gpos1_1_simple_t4` states a `YAdvance` in a
  horizontal run as a y displacement its deltas accumulate, which is what a
  line does with one.
- **Attachment.** A mark meets its base by the arithmetic
  `Buffer::propagate_attachments` does for a run drawn in buffer order —
  `offset(child) = offset(parent) + pen(parent) − pen(child) +
  anchor(parent) − anchor(child)`, on both axes — and because each offset
  carries its own glyph's origin, the mark's anchor lands on its base's. A
  cursive join is a line's with the axes exchanged, the column being its
  third case: along the column the first glyph's advance closes the gap,
  across it one glyph's placement does, `RIGHT_TO_LEFT` choosing which, and
  a chain inherits its parent's placement across. A mark's advances are
  zeroed in a column as along a line.

**A form field's column.** A PDF vertical font draws every glyph upright at
its own `/W2` position (9.7.4.3) and turns nothing, so a glyph that should
stand sideways has to come from the face pre-rotated: the registry's case for
`vrt2`. So `fill.rs` sets every run of a column upright itself, rather than
splitting it by UAX #50, turns `vrt2` on, and places each glyph by its `/W2`
as before, writing only what `GPOS` changed on top: the advance and the
placement down the column as `TJ` numbers, a positive one moving the pen down
(9.4.3), and a placement across the column by moving the text matrix. A
column whose face has no `GPOS` writes no number.

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
11. Vertical metrics in `Sfnt` (FT-07b.2): `vhea`, `vmtx`, `VORG` and the
    `glyf` bounds an origin is computed from. Exit, `sfnt.rs`'s unit tests:
    `vertical_metrics_read_from_vhea_vmtx_and_vorg` — every advance, side
    bearing and origin equals what the synthetic tables were written with,
    and a read out of range answers `None`;
    `a_glyph_past_the_long_metrics_repeats_the_last_advance_height`;
    `a_face_without_vertical_metrics_answers_none`; and beside them a `VORG`
    beside `glyf` ignored, a CFF face's origin taken from its `VORG` and,
    without one, from its outline, and a `vmtx` shorter than `vhea` claims
    answering `None` past its end. The `sfnt` fuzz target asks for each new
    metric.
12. Orientation (FT-07b.3): `VerticalOrientation.txt` vendored and compiled
    into `unicode::vertical_orientation`, and `shape::itemize_vertical`.
    Exit: `ucd_version.rs` holds the file to the pin and to its tree's file
    list; `vertical.rs`'s `vertical_orientation_matches_the_vendored_file`
    checks every code point against the file, and
    `itemize_splits_a_vertical_paragraph_by_orientation` pins the runs of a
    paragraph mixing CJK, Latin and Mongolian, an enclosing mark and a
    combining one among them; a horizontal paragraph itemizes as before.
13. The column (FT-07b.4): `Direction::TopToBottom`, `Orientation`, the
    upright plan and its metrics, the sideways plan. Exit, `vertical.rs`
    against synthetic faces whose expected glyphs and positions the test
    computes from their tables: `an_upright_run_takes_vert_and_its_vmtx_advances`,
    `vrt2_is_applied_as_the_registry_says`,
    `a_sideways_run_keeps_the_horizontal_plan` and
    `a_mongolian_run_in_a_vertical_paragraph_joins`; `aots.rs`,
    `text_rendering.rs` and `fingerprints.rs` unchanged. The `shape_text` fuzz
    target shapes each paragraph vertically too.
14. `GPOS` down a column (FT-07b.5): value records, the interactions,
    attachment. Exit, `vertical.rs`: `vkrn_adjusts_a_pair_down_the_column`,
    `vhal_and_vpal_take_their_adjustments_when_switched_on` and
    `a_mark_attaches_to_its_base_in_a_column`, with a cursive join down a
    column and `valt`'s withdrawal of `vkrn` beside them; every horizontal
    suite unchanged.
15. The form field's column (FT-07b.6). Exit, `shaped_forms.rs`:
    `a_column_takes_its_faces_vertical_kerning` writes a `vkrn` pair with the
    `TJ` number the test computes;
    `a_column_runs_the_vertical_features_and_not_the_horizontal_ones` holds
    the column to the upright plan; a column whose face carries no feature
    beyond `locl`, `ccmp`, `vert` and `vrt2` writes the bytes it wrote before
    the column moved onto the plan.
16. A variable face's column (FT-07b.7): its advances and origins at the
    coordinates FT-07a threads through the shaper, from `VVAR`. Exit,
    `vertical.rs`'s `a_variable_column_takes_its_instances_advances`; at the
    default instance it shapes as milestones 13 and 14 left it.
17. The fingerprint (FT-07b.8): vertical paragraphs of mixed orientation
    through synthetic faces built by `tests/column_face`, which `vertical.rs`
    shares, `fingerprints.rs`'s `vertical_shaping_is_stable_across_targets`,
    on Linux, Windows and `wasm32-wasip1`, after checking how much it shaped,
    its `VERTICAL` a run's value; `SHAPING` and `BIDI` unchanged, and
    `xtask/src/determinism.rs`'s roster names the test.
18. The speed row (FT-07b.9a, FT-07b.9b): `shape a vertical paragraph` in
    MS-04's fonts group, held to one list with `baseline.json` by
    `xtask/src/bench.rs`'s `the_committed_baseline_agrees_with_the_tree`, its
    figure taken in the timing phase.
19. Person-written expectations (FT-07b.10, with H-64): `vertical.rs`'s
    `person_written_vertical_expectations_hold` passes every fixture for
    `vert`, `vrt2`, `vkrn`, `vhal`, vertical positioning and a Mongolian run
    set sideways in a column, the count per feature and per script asserted.

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
- For vertical shaping: UAX #50 and its `VerticalOrientation.txt` at the
  pinned Unicode version; CSS Writing Modes 3 §5.1 for how its values are
  applied; the OpenType feature registry's vertical entries, and the `vhea`,
  `vmtx`, `VORG` and `GPOS` clauses, for the plan, the metrics and the
  convention. Until H-64's person-written fixtures arrive (milestone 19),
  every vertical test is a synthetic face whose expected output the test
  computes from its own tables, which shows the reading is applied and not
  that it is the reading.

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
| A `vmtx` shorter than `vhea` claims | Every read is bounds-checked and one past the table's end answers `None`, so a glyph whose entry is missing takes synthesized metrics and the run records `VerticalMetricsSynthesized`; a unit test reads a truncated `vmtx` |
| A `VORG` beside `glyf` | The `VORG` clause's own rule: ignored where the face has TrueType outlines, the origin read from `glyf`'s `yMax` and `vmtx`; a unit test gives such a face a `VORG` that disagrees |
| A face with vertical features but no vertical metrics | Its `vert` and `vkrn` still run — a feature is the face's statement — and its metrics are synthesized from the em box, with `VerticalMetricsSynthesized` recorded, so the column is set and says it was set on a synthesis |
| A hostile `VORG`: a count past its table, records out of order | The count is bounded by what the table holds; the order is checked once per face, a sorted array bisected and one out of order scanned as written, the first record winning, as `cmap`'s are |
| A CFF face without `VORG` interprets a charstring for every origin | The `CFF` table is parsed once per `Sfnt::vertical_origins` view, which the shaper makes once per run, and the charstring interpreter's own budget bounds each glyph; a face with `VORG`, which a CFF face built for vertical text carries, never reaches it |
| The grapheme cluster is approximated | A base and the marks and inherited characters after it; UAX #29's other extensions are not computed. Of what that leaves out only an emoji tag sequence — its tag characters `R` beside an upright base — splits; Hangul syllable sequences and regional-indicator pairs are upright on both sides. Named in [features/fonts.md](../features/fonts.md) |
| `Tr`'s rotated fallback is never taken | CSS §5.1.1 does not expect it of a layout engine and the itemizer sees no face; a `Tr` character with no vertical form stands upright, named in [features/fonts.md](../features/fonts.md) |

## AAT shaping

*For ROADMAP row FT-07c.* A face built for Apple's layout carries its
substitutions in `morx`, its kerning in `kerx` and its tracking in `trak`,
and may carry no `GSUB` or `GPOS` at all; today such a face shapes as if
unshaped. This section reads the four tables, clause by clause, from Apple's
*TrueType Reference Manual* (chapter 6, `morx`, `kerx`, `trak`, `feat` and
`ankr`, with its *AAT special tables* chapter for lookup and state tables, and
appendix F, the *Font Feature Registry*), and from nothing else (ruling 21);
settles where they run beside OpenType Layout, how a caller's features reach
them, how `trak` gets a size, and what clusters, warnings and ceilings they
keep; lists the manual's worked examples for each state machine; and names
the text-rendering-tests sections the suite will run.

### Scope

- **`morx`** (FT-07c.4 to FT-07c.10): the chain, its feature entries and
  sub-feature flags, subtable coverage, and the five subtable types —
  rearrangement (0), contextual (1), ligature (2), noncontextual (4) and
  insertion (5) — over the shared lookup tables and extended state tables.
- **`kerx`** (FT-07c.11 to FT-07c.14): its pair formats 0, 2 and 6, its
  state-machine formats 1 (contextual kerning) and 4 (attachment), with
  `ankr` and a glyph's TrueType points for format 4.
- **`trak`** (FT-07c.15, FT-07c.16): the normal track at the size the
  consumer passes.
- **`feat`**: which feature settings are exclusive, read for the chain.
- **The test sections** (FT-07c.3) and the proof: fuzzing, the fingerprint,
  the speed row and the count of worked examples (FT-07c.17 to FT-07c.20).

### Non-goals

- **`mort`**, the predecessor table, which no row names: a face carrying
  `mort` and no `morx` is shaped as if unshaped and the run says so by name,
  as the manual has a face with both ignore `mort` ("If a font has both a
  'mort' table and a 'morx' (extended metamorphosis) table, the 'mort' table
  is ignored").
- **`kerx` variation tuples** — a subtable whose `tupleCount` is above 0 —
  are FT-07a's, and **vertical** `kerx` subtables and `trak`'s vertical data
  FT-07b's; until those rows land each is refused by name where a run meets
  it.
- **The `kern` table**, TrueType's original kerning table, whose format 1 is a
  state machine of its own encoding; no row here reads it.
- **`ltag` and the Language Tag feature** (type 39): no run states a language
  to the chain, so its selector stays 0, "no change".
- **`just`, `opbd`, `lcar`, `prop`, `bsln` and the other AAT tables**: none
  shapes a glyph run; justification is FT-07e's.
- **A precedence that changes a face carrying OpenType Layout**: a price for
  the owner, never chosen here (*Precedence*).

### Design

#### Precedence

**A face carrying `GSUB` or `GPOS` is shaped through OpenType Layout exactly
as today, whatever AAT tables it carries beside them; a face carrying neither
is shaped through its AAT tables** — `morx` for substitution, `kerx` for
positioning and `trak` for tracking, each where it has it. So `GSUB` runs where
a face has it and `morx` only where it has none, and `GPOS` likewise before
`kerx`; and a face with `GSUB` and `kerx` but no `GPOS` is not kerned by
`kerx`, which would change a face that carries OpenType Layout. No committed
face carries an AAT table, so no committed digest moves (FT-07c.10).

#### Lookup tables and state tables

**Lookup tables** (*AAT special tables*, "Lookup Tables") come in formats 0
(simple array), 2 (segment single), 4 (segment array, its value a 16-bit
offset "from the start of the table"), 6 (single table), 8 (trimmed array) and
10 (extended trimmed array, `unitSize` 1, 2, 4 or 8). A binary-search header's
`searchRange`, `entrySelector` and `rangeShift` "are no longer used", and a
unit whose glyph is `0xFFFF` is the search's guard, no entry; a format-0 array
is as long as its table allows. A glyph a class lookup does not cover is class
1, *out of bounds*. In `morx`, "a value of 0 returned from a table lookup [is]
to mean 'do nothing' or 'make no substitution'".

**Extended state tables** ("Extended State Tables"): an `STXHeader` of
`nClasses` and the offsets of the class table — a lookup table of 16-bit
classes — the state array, `nClasses` 16-bit entry indices a state, and the
entry table, whose entries' size is the subtable type's. Classes 0 to 3 are
predefined: 0 *end of text*, 1 *out of bounds*, 2 *deleted glyph* (glyph
`0xFFFF`) and 3 *end of line*. One driver runs every machine: from state 0,
*start of text*, it reads the class of the glyph at the current position,
takes that state's entry, acts, moves to the entry's state, and advances unless
the entry's `dontAdvance` flag is set; past the last glyph it takes the
current state's *end of text* entry once, its `dontAdvance` meaning nothing
there. State 1, *start of line*, is never the initial state: a run is not a
line.

#### `morx`

**The chain** ("Table Format"): a `morx` of version 2 or 3 holds chains, each
a `defaultFlags`, its feature entries and its subtables; version 3's subtable
glyph coverage arrays are an optimisation the manual makes optional ("the
subtable may be skipped") and are not read, so a wrong one costs no glyph.
The sub-feature flags start at `defaultFlags`; each feature entry whose
feature and setting are requested sets `flags = (flags & disableFlags) |
enableFlags`, entries in table order, "Entries that are located later in the
feature table take precedence over earlier entries"; and a subtable runs where
its `subFeatureFlags` and the flags share a bit.

**Requests from a caller** (FT-07c.5). On the AAT path `Shaper::with_settings`
requests, for each tag set above 0, its feature's on-setting, and for each tag
set to 0 the off-setting of a non-exclusive feature — the on-setting plus one,
the registry's pairing — and nothing for an exclusive one, whose chain
defaults then stand. A feature `feat` marks exclusive (`featureFlags` bit 15)
takes the last setting requested of it, its siblings not requested.
`Shaper::with_features` replaces the plan: the flags start at 0, not at
`defaultFlags`, and each named tag requests its on-setting, so
`with_features(&[], &[])` runs no subtable. The crate's default OpenType
features are not requested on the AAT path: the chain's `defaultFlags` are the
face's own statement of its defaults. A tag with no row below is requested of
nothing and recorded once a run.

**The mapping** is this engine's, read by meaning from two published
registries: Apple's *Font Feature Registry* (appendix F), whose *Stylistic
Alternatives* entry states itself "the AAT equivalent of the OpenType 'ss01'
through 'ss20' features" and whose *Character Shape* entry names `hojo`, and
the OpenType Layout tag registry's description of each tag. A setting is
`type/selector`, the registry's on-setting; the off-setting of a non-exclusive
one is its selector plus one.

| OpenType tags | AAT feature (type), exclusive? | Selector |
| --- | --- | --- |
| `rlig`, `liga`, `dlig`, `clig`, `hlig` | Ligatures (1), no | 0, 2, 4, 18, 20 |
| `calt`, `swsh`, `cswh` | Contextual Alternates (36), no | 0, 2, 4 |
| `smcp`, `pcap` | Lower Case (37), yes | 1, 2 |
| `c2sc`, `c2pc` | Upper Case (38), yes | 1, 2 |
| `onum`, `lnum` | Number Case (21), yes | 0, 1 |
| `tnum`, `pnum` | Number Spacing (6), yes | 0, 1 |
| `afrc`, `frac` | Fractions (11), yes | 1, 2 |
| `sups`, `subs`, `ordn`, `sinf` | Vertical Position (10), yes | 1, 2, 3, 4 |
| `zero` | Typographic Extras (14), no | 4 (`kSlashedZeroOnSelector`, whose row the registry titles "Unslashed Zero") |
| `mgrk` | Mathematical Extras (15), no | 10 |
| `case`, `cpsp` | Case Sensitive Layout (33), no | 0, 2 |
| `ss01` to `ss20` | Stylistic Alternatives (35), no | 2 to 40, the `n`th set at `2n` |
| `salt` | Character Alternatives (17), yes | 1 |
| `titl` | Style Options (19), yes | 4 |
| `pwid`, `fwid`, `hwid`, `twid`, `qwid` | Text Spacing (22), yes | 0, 1, 2, 3, 4 |
| `trad`, `smpl`, `jp78`, `jp83`, `jp90`, `expt`, `jp04`, `hojo`, `nlck`, `tnam` | Character Shape (20), yes | 0, 1, 2, 3, 4, 10, 11, 12, 13, 14 |
| `ruby` | Ruby Kana (28), no | 2 |
| `ital` | Italic CJK Roman (32), no | 2 |
| `hkna`, `vkna` | Alternate Kana (34), no | 0, 2 |
| `vert`, `vrt2` | Vertical Substitution (4), no | 0 |
| `kern` | not a `morx` feature: set to 0, it turns `kerx` off (`css-fonts-4` §6.4) | — |

**Coverage** ("Metamorphosis Subtables"): bit `0x80000000` set, the subtable
is for vertical text, clear for horizontal; `0x20000000` set, for both. A
subtable whose orientation is not the run's — an upright run's is vertical,
a line's and a sideways run's horizontal — is skipped. The glyphs a subtable
processes are the run's **in layout order** — "Normally, when a 'morx' table
is processed, the Unicode bidi algorithm has already been run and the glyph
stream is in layout order" — so the chain runs over a right-to-left run
reversed and hands it back reversed again; bits 30 and 28 then choose the
traversal: layout order (both clear), reverse layout order (30), logical order
(28: layout order for a left-to-right run, its reverse for a right-to-left
one) and reverse logical order (both). Marks, the rearrangement verbs' `A` to
`D` and "to the left of" are positions in layout order whichever way the
machine traverses.

**Subtable types.** Type 3 is reserved and a type above 5 unknown: each is
skipped and recorded.

- **Rearrangement (0).** `markFirst` and `markLast` make the current glyph the
  first or the last of the span; the verb then rearranges the span's glyphs as
  the verb table states (`Ax ⇒ xA` to `ABxCD ⇒ DCxBA`), `A` and `B` its first
  glyphs and `C` and `D` its last in layout order. A span with fewer glyphs
  than its verb names is left as it is. Clusters across a rearranged span are
  merged as `Buffer::reorder` merges them, so they stay monotonic.
- **Contextual (1).** The entry's `markIndex` and `currentIndex` name
  per-glyph lookup tables — through a list of 32-bit offsets at the
  subtable's `substitutionTable` — for the marked glyph and the current one,
  `0xFFFF` naming none; `setMark` makes the current glyph the marked one
  "after the substitutions are made". The marked glyph is substituted wherever
  the machine has moved since marking it. An index past the offsets the table
  holds substitutes nothing and is recorded.
- **Ligature (2).** `setComponent` pushes the current glyph on the component
  stack and `performAction` runs the actions from `ligActionIndex`: each pops a
  component and adds its glyph index to the action's 30-bit offset, sign
  extended, to index the component table, whose values sum to an index into
  the ligature list. As the `mort` chapter reads the machine morx inherits, a
  popped component whose action has neither `last` nor `store` set becomes a
  deleted glyph, and one whose action has either set is replaced by the
  ligature at the sum so far; the stack holds 16 components, a push past them
  refused and recorded; after the actions the ligature's position is pushed
  again, "so that the ligatures can be used as components of other
  ligatures". An action popping an empty stack ends its list there, what it
  formed standing, and is recorded.
- **Noncontextual (4).** One lookup table: each glyph it covers with a value
  other than 0 becomes that value.
- **Insertion (5).** `currentInsertIndex` and `markedInsertIndex` start a
  list of `currentInsertCount` and `markedInsertCount` glyphs (each at most
  31) in the insertion action table, inserted before the current or marked
  glyph where `currentInsertBefore` or `markedInsertBefore` is set and after it
  otherwise; `setMark` marks the current glyph. With `dontAdvance` "the next
  glyph processed would in fact be the first one inserted" downstream of the
  current glyph; without it the machine moves past the current glyph and past
  the glyphs inserted after it, so an insertion is not read again. A mark
  keeps naming its glyph when glyphs are inserted before it. Growth past
  `Limits::max_glyphs` is refused, as `GSUB`'s is, and recorded.

**Readings the manual leaves open, pinned by the test sections.** Where no
glyph has been marked, the mark is the first glyph the machine met — a
rearrangement's span starts there (`MORX-7`), a contextual mark substitutes it
(`MORX-19`) and an insertion goes beside it (`MORX-32`). The end-of-text entry
is taken once (`MORX-10`, `MORX-11`, `MORX-17`, `MORX-20`). A machine that never
advances stops at its ceiling (`MORX-24`, `MORX-36`), and one that inserts
without bound at the glyph ceiling (`MORX-34`).

**After the chain** the deleted glyphs are removed — within it they are class
2 to every later subtable — and the glyphs' advances are read from `hmtx`, as
after `GSUB`.

**Clusters.** A ligature takes the first cluster of its components, the one
`shaping::cluster_texts` rebuilds `/ToUnicode` from. An inserted glyph takes
the cluster of the glyph it is inserted at; the manual makes a split-vowel-like
insertion (its `currentIsKashidaLike` or `markedIsKashidaLike` flag clear)
"associated with the same character offset" as the other glyph of its action,
so for one the clusters from the inserted glyph to that glyph are merged, as a
rearranged span's are. Whether each inserted glyph was inserted kashida-like
is kept on the buffer for FT-07e, and changes no output here.

#### `kerx`

**The header** ("The 'kerx' Table"): version 2, 3 or 4, its subtables each a
length, a coverage and a `tupleCount`. Coverage `0x80000000` is vertical,
`0x40000000` cross-stream, `0x20000000` variation, `0x10000000` the
processing direction of formats 1 and 4, and the low byte the format. A
subtable with a `tupleCount` above 0 in a version-4 table is refused by name
(FT-07a); a vertical subtable runs in no line and is refused by name in an
upright run (FT-07b); a format the chapter does not define is skipped and
recorded. Subtables are additive and run in order, over the run in layout
order as `morx`'s do.

**Values.** A kerning value is in font units, "If this value is greater than
zero, the glyphs are moved apart"; it is added to the advance of the left
glyph of its pair, or of the glyph a format-1 value pops. In a cross-stream
subtable it is added to the `y_offset` of the right glyph of its pair, or of
the glyph popped — "If adjustment values are positive, the text will be
moved up" — that glyph alone, which is this engine's reading where the chapter
does not say whether a shift carries to the glyphs after it.

- **Format 0**, ordered pairs, binary-searched by the 32-bit key the two
  glyphs make, a final `0xFFFF 0xFFFF 0` entry a sentinel.
- **Format 2**, a class array: the left glyph's value from the left lookup
  table and the right glyph's from the right one, both defaulting to 0; the
  value is read at the subtable's start plus the two, the left values holding
  their row "offset by the offset of the array" as the `kern` chapter's format
  2 states the same layout — the `kerx` chapter's last sentence, which adds
  them to the array's address instead, is read as the subtable's (*Risks*).
- **Format 6**, an index array: row and column indices from two lookup
  tables, the row's pre-multiplied by `columnCount`, summed into the kerning
  array, two-byte values or four where `valuesAreLong` (bit 0) is set.
- **Format 1**, contextual kerning: an entry's `push` (`0x8000`) pushes the
  current glyph on the kerning stack of "up to eight glyphs", `dontAdvance`
  (`0x4000`) holds the position, `reset` (`0x2000`) clears the stack, and its
  value index, `0xFFFF` for none, starts a list of values in the value table,
  each popping one glyph and kerning it, ended by `0xFFFF`. A push onto a full
  stack is refused and recorded, and a value with no glyph to pop ends its
  list.
- **Format 4**, attachment: `mark` (`0x8000`) remembers the current glyph and
  an action index, `0xFFFF` for none, names an action of the type the
  subtable's flags give: control points (type 0), `ankr` anchor points (1) or
  coordinates (2). "The current glyph is positioned so that the two
  control/anchor points overlap": its offset is the marked glyph's point,
  where the marked glyph stands in the run, less the current glyph's point.
  Control points are a TrueType glyph's points as stored (FT-07c.13): a simple
  glyph's in point-number order, a composite's its components' in component
  order, each moved by its offset and 2×2 in integer arithmetic, rounded half
  away from zero — the numbering a composite's instructions see. A CFF face has
  no such points, and a control-point action on one is refused by name.
  Anchor points are `ankr`'s: a lookup of 16-bit offsets into its glyph data
  table, each glyph's entry a count and that many `x`, `y` pairs. An
  attachment is resolved by `Buffer::propagate_attachments`, as a `GPOS` mark's
  is.

#### `trak`

The normal track — the entry whose `track` value is 0 — gives the tracking:
"If the point size and track number required are stored, these values are
used. If the point size and track number required are not found in the
tracking table, interpolation or extrapolation is performed." At a size equal
to a listed size its value is used; between two, it is interpolated linearly;
past the first or the last, extrapolated linearly from the nearest two; with
one size listed, that value holds at every size. The arithmetic is 16.16
integers in `i64`, the result rounded half away from zero to a whole font unit
and added to every glyph's advance — "The same adjustment is applied to all
glyphs in the font" — after `kerx`. A face with no track 0, or no horizontal
data, is not tracked.

**The size** is the consumer's: `Shaper::with_size` takes the run's size in
points as a 16.16 integer, so no float enters the crate, and every consumer
that shapes passes it (FT-07c.16). A run shaped with no size is not tracked,
and keeps the advances the face gives.

#### `feat`

The feature name table's array of features, sorted by type, each with its
settings and `featureFlags`: bit 15 marks the feature's settings exclusive,
and bit 14 with bits 0 to 7 its default setting's index, the first setting
otherwise. The chain reads exclusivity from it (*Requests from a caller*), and
nothing else of it; a face without `feat` has every feature read as
non-exclusive except those the registry marks exclusive in the mapping above.

#### Warnings and ceilings

Every refusal is a typed `Warning` recorded once a run (rulings 2 and 10):
a `mort` table not run (and, until FT-07c.10, `morx` itself — FT-07c.2); a
subtable type reserved or unknown; a lookup format unknown; a machine stopped
at its ceiling; a contextual index past its table; a component or kerning stack
full or empty; a glyph insertion past `Limits::max_glyphs`; a `kerx` variation
tuple or vertical subtable refused; a control-point action on a CFF face; an
OpenType tag no AAT feature answers. `Limits` gains the ceiling on one
machine's transitions in one subtable — `max(4 096, 32 × (glyphs + 1))`, scaled
as `Limits::for_glyphs` scales the others — past which the subtable stops,
leaving the run as it stood. The stacks are the chapters' sizes, 16 components
and 8 kerning glyphs; every read is `read::Bytes`', so a truncated table or an
offset past its end answers `None` (ruling 1).

#### The manual's worked examples

By chapter and figure, for each of the six state machines:

| Machine | Worked examples the manual gives | Transcribed by |
| --- | --- | --- |
| `morx` type 0, rearrangement | **None.** The verb table defines each verb's effect and is held by `every_rearrangement_verb_moves_what_it_names`; it shows no machine | H-142 |
| `morx` type 1, contextual | (1) `morx` chapter, *Examples*, "1. A contextual substitution table": glyphs 50, 51, 201 and 202 after glyph 80 become 600, 601, 602 and 900, every byte from offset 0 given — its per-glyph lookup's units, printed at offsets 132, 136, 144 and 148 with the guard at 152, are read as the contiguous units they must be, 132 to 147 with the guard at 148. (2) *AAT special tables*, "State Tables": the capitalising machine of figure FF30, its four entries in figure FF31 and its state array in figure FF32 — a word's first letter upper case and the rest lower, an out-of-bounds glyph starting a word again — encoded as a contextual subtable with `UPPER` and `LOWER` as current-glyph substitution tables. Figures FF27 and FF29 draw earlier forms of the machine and give no table | FT-07c.7 |
| `morx` type 2, ligature | `morx` chapter, *Examples*, "2. A ligature table": the 24 ligatures of `a`, `b` or `c`, `d` or `e`, and `f` to `i` (glyphs 20 to 28) to glyphs 1000 to 1015 and 1500 to 1511, every byte at offsets 0 to 221 given — its class lookup's third segment, printed with the second's values `24, 23, 5`, is read as glyphs 25 to 28 in class 6, as its comment states. The `mort` chapter's figure FF38 works the same component sums in `mort`'s byte offsets and is not transcribed | FT-07c.8 |
| `morx` type 5, insertion | **None** | H-142 |
| `kerx` format 1, contextual kerning | **None** in the `kerx` chapter. The `kern` chapter's *Kerning Table Example* (figures FF48, FF78 and FF45) is a `kern` format-1 machine whose value lists end in an odd value and reset the cross-stream baseline with `0x8000`, conventions `kerx`'s value list does not share, and is not transcribed | H-142 |
| `kerx` format 4, attachment | **None** | H-142 |

Beside them, three worked examples that are no state machine are transcribed:
the `mort` chapter's *Example: Non-contextual Glyph Substitution*, whose
lookup (offsets 64 to 87: glyphs 11 and 12 to 135 and 136) is a `morx`
noncontextual subtable's whole body, carried into a `morx` chain with the
example's three feature entries and its coverage bit, vertical — the comment
calling it horizontal contradicts the bit and the example's purpose
(FT-07c.5); the `mort` chapter's *Hierarchical Ligature Tables* and
*Stand-Alone Ligature Tables* feature entries, worked examples of the flag
arithmetic (FT-07c.5); the `trak` chapter's *Tracking Table Example*, three
tracks at 12 and 24 points, its third track entry printed at offset 38 read at
36 (FT-07c.15); and the `feat` chapter's *Feature Name Table Example*, its
number-spacing feature read with the two settings it lists rather than the one
its count says (FT-07c.5). Each transcription names its corrections in its
test.

#### The test sections

text-rendering-tests at the vendored `26cfb96` holds 41 `morx` sections —
`MORX-1` to `MORX-42`, there being no `MORX-15` — and `MORT-1`; no `kerx` or
`trak` section. FT-07c.3 vendors all 42 with their faces and declines each
until FT-07c.10, which runs the `morx` chain, and declines `MORT-1` by name
for good:

| Sections | What they need |
| --- | --- |
| `MORX-1` | the noncontextual subtable |
| `MORX-2` to `MORX-14`, `MORX-16`, `MORX-17` | rearrangement: marking, verbs, underflow, end of text, `dontAdvance`, a loop |
| `MORX-18` to `MORX-26` | contextual substitution: marking, end of text, `setMark`, both indices, `dontAdvance`, a machine that never advances |
| `MORX-27`, `MORX-28`, `MORX-41` | ligatures: components, unmarked glyphs, stack underflow and overflow |
| `MORX-29` to `MORX-36` | insertion: before and after, the mark kept, `setMark` on the inserting transition, `dontAdvance`, unbounded growth |
| `MORX-37` to `MORX-40` | the four processing orders, over Latin and Hebrew |
| `MORX-42` | every subtable type in one chain |
| `MORT-1` | `mort`: declined, refused by name |

`KERN-1` and `KERN-2` test the `kern` table, which is not AAT's and which no
row here reads.

### Milestones

1. **FT-07c.2, the refusal.** Exit: `tests/aat.rs`'s
   `a_face_carrying_only_morx_is_named_and_shaped_unshaped`.
2. **FT-07c.3, the sections.** Exit: `text_rendering.rs`'s
   `the_corpus_is_the_one_that_was_vendored`,
   `the_suite_covers_the_sections_it_claims_to` and
   `the_sections_this_milestone_declines` over the 42 sections above;
   `every_section_this_crate_claims_is_whole` unchanged; `cargo xtask vendor`.
3. **FT-07c.4, the machinery.** Exit: `every_lookup_format_answers_the_glyphs_it_covers_and_no_other`,
   `a_state_table_reads_its_classes_states_and_entries`,
   `a_machine_that_never_advances_stops_at_its_ceiling`.
4. **FT-07c.5, the chain.** Exit: `a_chain_runs_the_subtables_its_flags_select`
   (with the two ligature-hierarchy examples' flags),
   `an_exclusive_feature_setting_turns_its_siblings_off` (with the `feat`
   example), `a_subtable_for_the_other_orientation_is_skipped`,
   `a_noncontextual_subtable_maps_every_glyph_it_covers` (with the
   noncontextual example).
5. **FT-07c.6, rearrangement.** Exit:
   `rearrangement_reproduces_the_manuals_worked_examples` (none, the count
   asserted), `every_rearrangement_verb_moves_what_it_names`.
6. **FT-07c.7, contextual.** Exit:
   `contextual_substitution_reproduces_the_manuals_worked_examples` (two),
   `a_marked_glyph_is_substituted_after_the_machine_has_moved_past_it`.
7. **FT-07c.8, ligatures.** Exit:
   `ligature_reproduces_the_manuals_worked_examples` (one),
   `a_ligature_keeps_its_first_components_cluster`,
   `a_component_stack_past_its_ceiling_is_refused_and_named`.
8. **FT-07c.9, insertion.** Exit:
   `insertion_reproduces_the_manuals_worked_examples` (none),
   `an_inserted_glyph_takes_the_cluster_it_is_inserted_at`,
   `an_insertion_past_the_glyph_ceiling_is_refused_and_named`.
9. **FT-07c.10, `morx` in `Shaper::shape`.** Exit:
   `a_face_carrying_only_morx_is_shaped_through_its_chain`,
   `a_face_carrying_both_takes_the_path_the_design_states`,
   `a_mort_only_face_is_named`; the 41 `MORX` sections leave `DECLINED` and
   pass whole, counted in `EXPECTED`, `PASSING` and `WHOLE`.
10. **FT-07c.11, `kerx` pairs.** Exit: `each_kerx_pair_format_kerns_its_pairs`,
    `a_cross_stream_subtable_moves_glyphs_off_the_baseline`,
    `kern_set_to_zero_turns_kerx_off`, `a_variation_tuple_is_refused_by_name`.
11. **FT-07c.12, contextual kerning.** Exit:
    `contextual_kerning_reproduces_the_manuals_worked_examples` (none),
    `a_kerning_stack_past_its_ceiling_is_refused_and_named`.
12. **FT-07c.13, a glyph's points.** Exit, `glyf.rs`:
    `points_are_a_simple_glyphs_stored_coordinates`,
    `a_composites_points_follow_its_components`.
13. **FT-07c.14, attachment.** Exit:
    `attachment_reproduces_the_manuals_worked_examples` (none),
    `each_point_source_attaches_where_its_points_say`,
    `a_control_point_action_on_a_cff_face_is_refused_by_name`.
14. **FT-07c.15, `trak`.** Exit: `tracking_is_the_tables_value_at_a_listed_size`
    and `tracking_between_two_sizes_is_interpolated` (with the `trak`
    example), `a_run_shaped_without_a_size_is_tracked_as_the_design_states`.
15. **FT-07c.16, the consumers.** Exit: `epub_shaped.rs`'s
    `an_aat_face_is_shaped_tracked_and_measured_as_drawn`,
    `shaped_creation.rs`'s `a_built_aat_run_is_tracked_at_its_size_and_extracts_as_typed`,
    `shaped_forms.rs`'s `an_aat_field_value_is_tracked_at_its_size`.
16. **FT-07c.17, the fuzzer.** Exit:
    `every_aat_fuzz_seed_shapes_without_panicking`.
17. **FT-07c.18, the fingerprint.** Exit: `fingerprints.rs`'s
    `aat_shaping_is_stable_across_targets`, on Linux, Windows and
    `wasm32-wasip1`.
18. **FT-07c.19, the speed row.** Exit: `xtask/src/bench.rs`'s
    `the_committed_baseline_agrees_with_the_tree` with the AAT paragraph in
    MS-04's fonts group.
19. **FT-07c.20, the count.** Exit:
    `every_state_machine_is_held_to_its_worked_examples` — two for
    contextual substitution, one for ligatures, and H-142's fixtures for
    rearrangement, insertion and `kerx` formats 1 and 4.

### Risks

| Risk | Mitigation |
| --- | --- |
| The manual's worked examples carry misprints, and a transcription that copies one tests the misprint | Each correction is named here and in its test: the contextual example's offset, the ligature example's third segment, the `trak` example's offset, the `feat` example's count, the noncontextual example's comment |
| A behaviour the manual leaves open is read one way where a face expects another | The default mark, the end-of-text entry and an insertion's advance are pinned by named test sections; the rest are stated here as this engine's readings |
| A rearrangement under a reverse traversal reads its span the other way | Spans are layout positions whatever the traversal; no test section rearranges in reverse, so `MORX-37` to `MORX-40` hold the traversal and FT-07c.6's verb test the spans |
| `kerx` format 2's two sentences on where a value lies disagree | The `kern` chapter's statement of the same layout decides; a face built the other way kerns by the wrong cell, never out of bounds |
| A cross-stream shift is cumulative in some faces' intent | Read per glyph, the reading the `kerx` chapter's text supports; the `kern` example that steps the baseline is that table's convention |
| The OpenType-to-AAT mapping is by meaning, not stated | Only `ss01` to `ss20` and `hojo` are stated by the registry; the rest follow its descriptions, and a tag with no row is recorded, never guessed |
| A face carrying both kinds of table is shaped otherwise than today | It is not: OpenType Layout wins wherever it is present, and a different precedence is the owner's to take |
| A hostile chain loops or grows without bound | The transition ceiling, the glyph ceiling, the stacks' sizes and `read::Bytes`; the `shape` fuzz target drives every table (FT-07c.17) |
| Four machines have no worked example, so nothing the manual states holds them | H-142's person-written expectations, counted beside the manual's by FT-07c.20 |
