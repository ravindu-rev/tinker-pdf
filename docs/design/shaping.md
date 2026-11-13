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
- **Graphite shaping** (FT-07d): a face whose only layout tables are a
  Graphite description, shaped by its own passes — its glyph attributes,
  features and languages, the rules its finite-state machines find, the stack
  machine, positioning, collision avoidance and justification — *Graphite
  shaping* below.
- **Justification** (FT-07e): `JSTF`'s priorities and kashida applied to a
  run by a justify call in integer font units, chosen by CSS Text 3's
  `text-justify`, offered a line's slack by layout and drawn as measured —
  *Justification* below.
- **Person-written expectations** (FT-09): the case format a reader of a
  script writes by hand, the provenance it carries, the bar it is wired into,
  how a script leaves the shaped-and-unverified list, and the rules of the
  two mechanisms refused until a case exists — *Person-written expectations*
  below.

## Non-goals

- **Shaping while rendering existing PDFs.** `TJ` arrays are honored as
  written. Nothing in `tinker-pdf-render` calls this crate. Reordering is not
  shaping: ruling 14 puts an extracted right-to-left line into logical order
  with this crate's UAX #9 (`bidi::logical_order`), called from the facade,
  so `tinker-pdf-content` has no edge here and no glyph is re-shaped.
- **AAT (`morx`) tables.** OpenType Layout only; a face carrying only
  `morx` shapes as if unshaped. [ROADMAP](../ROADMAP.md) row FT-07c, whose
  design is the *AAT shaping* section below.
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
- **Justification beyond what *Justification* below designs.** Shrinkage in
  line breaking, spacing between the letters of scripts that do not join,
  and calligraphic elongation are that section's non-goals.

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
The third bar is an expectation a reader of the script writes by hand, in the
format *Person-written expectations* below fixes.

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
   done. The price of canonical ordering, which H-145 reads, is measured
   before anything applies it (FT-08.2): `text_rendering.rs`'s `#[ignore]`d
   `canonical_ordering_price` puts each runnable case's text in
   `Canonical_Combining_Class` order — field 3 of the vendored
   `UnicodeData.txt`, every adjacent pair whose first class is greater than
   the second and the second not 0 exchanged — shapes it through the suite's
   own `shaped`, and prints `RAN canonical_ordering_price` and each section's
   cases gained and lost against the text as typed, by id. Ordering the text
   before `cmap` stands in for ordering it after decomposition, because this
   crate decomposes nothing. Its figures, with its time from `cargo test`'s
   own `finished in` line in release and debug, are what H-145 reads; the
   eight passing cases that type a class-230 mark just before `SAKOT`
   (U+1A60, class 9) type the pair the ordering exchanges.
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
  glyphs and `C` and `D` its last in layout order, the span running from the
  leftmost of its two marks to the rightmost. A span with fewer glyphs than
  its verb names is left as it is. Clusters across a rearranged span are
  merged as `Buffer::reorder` merges them, so they stay monotonic; a verb that
  leaves every glyph where it was merges nothing.
- **Contextual (1).** The entry's `markIndex` and `currentIndex` name
  per-glyph lookup tables — through a list of 32-bit offsets at the
  subtable's `substitutionTable` — for the marked glyph and the current one,
  `0xFFFF` naming none; `setMark` makes the current glyph the marked one
  "after the substitutions are made". The marked glyph is substituted wherever
  the machine has moved since marking it, the marked glyph before the current
  one, so both indices on one glyph chain (`MORX-22`). The chapter gives the
  offsets no count; the array ends where the first table it names begins at
  the latest, so an index whose offset would lie at or past that point — or
  that cannot be read — substitutes nothing and is recorded
  (`Warning::SubstitutionTableMissing`).
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
(`MORX-19`) and an insertion goes beside it (`MORX-32`) — and a rearrangement
with no glyph marked last has no span, its verbs moving nothing (`MORX-3`). The
end-of-text entry is taken once (`MORX-10`, `MORX-11`, `MORX-17`, `MORX-20`),
its current glyph the last one the machine met: `markLast` there extends a
span to it (`MORX-11`) and a contextual `currentIndex` substitutes it
(`MORX-20`). A rearrangement's marks stay where they were after its verb, so a
later verb acts on the same positions (`MORX-6`). A machine runs only over a
run holding a glyph its class table gives a class other than out of bounds:
the chapter lets a subtable none of whose glyphs a run uses be skipped
("Subtable Glyph Coverage Tables"), a version-2 table's class table is the only
statement of which glyphs a machine uses, and a machine skipped takes no
end-of-text transition either (`MORX-20`'s `EE` and `E` keep their glyphs
though its end-of-text entry substitutes). A machine that never advances stops
at its ceiling (`MORX-24`, `MORX-36`), and one that inserts without bound at
the glyph ceiling (`MORX-34`).

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

Every refusal is a typed `Warning` recorded once a run (rulings 2 and 10),
each naming where in its table it was asked, the table, chain and subtable
(`aat::Site`): a `mort` table not run (and, until FT-07c.10, `morx` itself —
FT-07c.2, `Warning::TableNotRun`); a subtable type reserved or unknown, or of
a machine not built yet (`Warning::SubtableTypeNotRun`); a chain whose header
or feature entries cannot be read or whose length runs past its table, which
ends the walk, a chain's length being the only way to the next
(`Warning::MalformedChain`); a lookup format unknown
(`Warning::UnknownLookupFormat`); a subtable that cannot be read, or a machine
stopped at a state, an entry or a class column its tables do not hold
(`Warning::MalformedAatSubtable`), or at its ceiling
(`Warning::TransitionBudgetExceeded`); a contextual index past its table; a
component or kerning stack full or empty; a glyph insertion past
`Limits::max_glyphs`; a `kerx` variation tuple or vertical subtable refused; a
control-point action on a CFF face; an OpenType tag no AAT feature answers
(`Warning::FeatureNotInAat`). `Limits::max_transitions` is the ceiling on one
machine's transitions in one subtable — `max(4 096, 32 × (glyphs + 1))`,
scaled as `Limits::for_glyphs` scales the others — past which the subtable
stops, leaving the run as it stood. Every count a table states that a run
spends is bounded the same way: `Limits::max_subtables` (4 096) on the chains
and subtables of one table read and run, each counting one
(`Warning::SubtableBudgetExceeded`), and `Limits::max_feature_entries` (4 096)
on the feature entries of one chain read to compute its flags
(`Warning::FeatureEntryBudgetExceeded`), both a face's ceilings that
`for_glyphs` leaves alone. The stacks are the chapters' sizes, 16 components
and 8 kerning glyphs; every read is `read::Bytes`', so a truncated table or an
offset past its end answers `None` (ruling 1).

#### The manual's worked examples

By chapter and figure, for each of the six state machines:

| Machine | Worked examples the manual gives | Transcribed by |
| --- | --- | --- |
| `morx` type 0, rearrangement | **None.** The verb table defines each verb's effect and is held by `every_rearrangement_verb_moves_what_it_names`; it shows no machine | H-142 |
| `morx` type 1, contextual | (1) `morx` chapter, *Examples*, "1. A contextual substitution table": glyphs 50, 51, 201 and 202 after glyph 80 become 600, 601, 602 and 900, every byte from offset 0 given but three, each corrected: its `nClasses`, printed 5, is read as the 6 its state array's lines hold, printed twelve bytes apart at 56, 68 and 80; its substitution tables' offset at 16, printed `xxx`, as 116, where the example starts them; and its per-glyph lookup's units, printed at offsets 132, 136, 144 and 148 with the guard at 152, as the contiguous units they must be, 132 to 147 with the guard at 148. (2) *AAT special tables*, "State Tables": the capitalising machine of figure FF30, its four entries in figure FF31 and its state array in figure FF32 — a word's first letter upper case and the rest lower, an out-of-bounds glyph starting a word again — encoded as a contextual subtable with `UPPER` and `LOWER` as current-glyph substitution tables. Figures FF27 and FF29 draw earlier forms of the machine and give no table | FT-07c.7 |
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
| The manual's worked examples carry misprints, and a transcription that copies one tests the misprint | Each correction is named here and in its test: the contextual example's class count, substitution-table offset and unit offsets, the ligature example's third segment, the `trak` example's offset, the `feat` example's count, the noncontextual example's comment |
| A behaviour the manual leaves open is read one way where a face expects another | The default mark, the end-of-text entry and an insertion's advance are pinned by named test sections; the rest are stated here as this engine's readings |
| A rearrangement under a reverse traversal reads its span the other way | Spans are layout positions whatever the traversal; no test section rearranges in reverse, so `MORX-37` to `MORX-40` hold the traversal and FT-07c.6's verb test the spans |
| `kerx` format 2's two sentences on where a value lies disagree | The `kern` chapter's statement of the same layout decides; a face built the other way kerns by the wrong cell, never out of bounds |
| A cross-stream shift is cumulative in some faces' intent | Read per glyph, the reading the `kerx` chapter's text supports; the `kern` example that steps the baseline is that table's convention |
| The OpenType-to-AAT mapping is by meaning, not stated | Only `ss01` to `ss20` and `hojo` are stated by the registry; the rest follow its descriptions, and a tag with no row is recorded, never guessed |
| A face carrying both kinds of table is shaped otherwise than today | It is not: OpenType Layout wins wherever it is present, and a different precedence is the owner's to take |
| A hostile chain loops or grows without bound | The transition ceiling, the glyph ceiling, the stacks' sizes and `read::Bytes`; the `shape` fuzz target drives every table (FT-07c.17) |
| Four machines have no worked example, so nothing the manual states holds them | H-142's person-written expectations, counted beside the manual's by FT-07c.20 |

## Graphite shaping

A face whose layout is a Graphite description — the tables of SIL's Graphite
Table Format: `Silf`, `Glat`, `Gloc`, `Feat` and `Sill` — is shaped by its own
rules: the passes its finite-state machines find rules in, the stack-machine
code each rule runs, and its positioning, collision avoidance and
justification. [ROADMAP](../ROADMAP.md) row FT-07d; until its shaping step
lands, such a face is refused by name (*Warnings*, below).

### Graphite scope

- **The tables, at the versions read.** `Silf` at major versions 1 to 5, any
  minor, plain or compressed; `Glat` 1, 2 and 3, with or without octaboxes,
  plain or compressed; `Gloc` with short or long offsets, with or without
  attribute ids; `Feat` 1 and 2; `Sill` 1. Any other version, and any
  compression scheme but 0 and 1, is refused by name with the table and the
  number it states.
- **The sources** (ruling 21; `THIRDPARTY.md` names each with its revision):
  the Graphite Table Format (the *GTF*) in its two published editions — the
  one in the Graphite project's documentation, which carries the version-5
  notes, and the 2019 edition describing table format 5 — and the stack
  machine's command list in both of its editions, the GTF's appendix and the
  earlier standalone document; the *Graphite Description Language* manual,
  version 4.000 (the *GDL*), for what a pass, a rule, a scan position and each
  attribute mean; the project documentation's font and feature topics, for
  guard space and a language's default feature values; the Graphite
  compiler's documentation of its debug files, for its worked class map and
  the jammed state; the TrueType Reference Manual's `feat` chapter, which the
  GTF names as `Feat`'s basis; and the LZ4 Block Format Description, which the
  GTF's compression scheme 1 names. No engine's code is read.
- **The mechanisms.** The slot stream and its attributes; glyph attributes
  and octaboxes; features and languages; pseudo-glyphs and class maps; the
  finite-state machine and its rule order; the stack machine; line-break,
  substitution, bidi, justification and positioning passes; attachment,
  shifting, advances and kerning; collision avoidance and its kerning;
  justification levels.
- **Where it applies.** `Shaper::shape`, so a book on the EPUB path, a
  `DocumentBuilder` run and a form field all reach it through the shaper they
  already make.

### Graphite non-goals

- **`Sile` and `Sild`.** The GTF says `Sile`, a description kept apart from
  its face, "is not currently being used", and that `Sild` "has not been
  implemented". A face carrying either is read as though it did not.
- **Line-break weights reaching layout.** Layout breaks a line by UAX #14,
  and nothing in the `Shaper` seam carries a weight. A face's line-break
  passes still run, and a later pass reads what they set.
- **The line-break pseudo-glyph.** A run is shaped before the line it ends up
  on is known, so no line-break glyph is put in the stream, and a rule with
  `#` in its context never matches. Neither is cross-line contextualization
  (`maxPreContext`, `maxPostContext`) applied.
- **Header fields with no reader here.**
  `extraAscent` and `extraDescent` are a line height's, which is layout's;
  critical features are given no meaning by the GTF; `attrSkipPasses` marks
  the glyphs without which a pass cannot match, so skipping such a pass is
  speed and never output, and nothing here takes it up.
- **Ligature components' boxes.** The GDL marks them unimplemented; a
  component's slot reference is kept and read, and no caret inside a ligature
  is answered.
- **A Graphite column.** The GDL: "Vertical scripts are currently not
  supported". An upright run of a Graphite face is shaped as an upright run of
  a face with no layout tables is today (*Vertical shaping*), and a sideways
  run through its passes, along a line.

### Graphite design

**Precedence.** A face's Graphite tables run for a run whose script its
OpenType Layout tables declare no lookup for: neither its `GSUB` nor its
`GPOS` has a script record for the run's script tag, nor a `DFLT` one, so no
feature could name a lookup there. Wherever either table's script list holds
the run's script or `DFLT`, the run shapes through OpenType exactly as it
does today; a face carrying `morx` takes the path *AAT shaping* above sets
for it; and a face with no readable `GSUB` or `GPOS` takes its Graphite
tables for every run. A run whose script the face's OpenType tables cover
therefore takes exactly today's path, and what moves is only a run that is
drawn unshaped today: a script a face carrying both covers in Graphite alone.

The case that decided it is the vendored Awami Nastaliq. It is
Graphite-first and carries a 70-byte `GPOS`: one `kern` feature for script
`deva` alone, one single-adjustment lookup moving glyph 1 by one unit. Read
as "any `GSUB` or `GPOS` sends the face down the OpenType path", that table
would draw every Urdu run in the face unshaped — no `arab` record and no
`DFLT`, so no lookup — even after its passes run. Under the precedence
above an Urdu run takes the face's Graphite passes and a Devanagari run its
`GPOS`; Tagmukay, whose `GSUB` holds `DFLT`, `latn` and `tfng`, keeps the
OpenType path for every run; and Dai Banna SIL, which carries neither, takes
its passes for every run. FT-07d.12 lands the precedence in `Shaper::shape`;
until it does, every face carrying a readable `GSUB` or `GPOS` takes the
OpenType path, and the tests reach a description's passes through
`graphite`'s own types.

**What is decompressed.** From `Silf` 5 and `Glat` 3 a table may be
compressed: its version, then a word whose top five bits are the scheme and
whose low twenty-seven are the uncompressed size, then the scheme's data.
Scheme 0 is the table as it stands. Scheme 1 is one LZ4 block — no frame and
no checksum — decoding to the whole table again, its version included and its
scheme 0. It is decoded once a face, into bytes the face's Graphite view
owns, under `Limits::max_table_bytes` (16 MiB); a stated size past it, a
block that decodes to more or fewer bytes than it states, a match offset of
zero or reaching before the start, and a block that ends inside a sequence
are each refused by name.

**Glyph attributes.** `Gloc` is a version, flags — bit 0 long offsets, bit 1
an attribute-id array at the end — the attribute count, and one offset into
`Glat` per glyph and one more. The number of glyphs is read from `Gloc`'s
own length: the offsets fill the table up to the id array, or to its end.
`Silf`'s `maxGlyphID` does not bound it: the vendored Awami Nastaliq's
`Silf` states a smaller maximum than its `Gloc` indexes and than its own
pseudo-glyph map names, and a test holds the reader to that face. `Glat` 1
writes each run as a byte attribute number and a byte count, `Glat` 2 and 3
as two `USHORT`s, each followed by that many `SHORT` values; a glyph's block
is its runs, and an attribute no run holds is 0, the compiler documentation's
"any unlisted attribute can be assumed to have the value 0". `Glat` 3's
header word carries the scheme in its top five bits and the octabox flag in
its lowest: the GTF's table gives the word fields of 5, 28 and 1 bits, which
cannot all fit, and the vendored face sets bit 0. Where the flag is set each
block opens with an octabox — a `USHORT` bitmap of the 4 × 4 cells that have
sub-boxes, bit (y × 4) + x, four diagonal bytes, and eight bytes a cell — and
its runs follow. The four bytes are, in order, the least and greatest x + y
and the least and greatest x − y over the glyph's outline, each scaled to
0–255 across the bounding box's width plus its height: the order the project
documentation's edition names them in (`diag_neg_min`, `diag_neg_max`,
`diag_pos_min`, `diag_pos_max`), where the 2019 edition lists the pairs the
other way round, and the vendored face's outlines decide it, in a test. A
sub-box's eight bytes are its left, right, bottom and top and its four
diagonals in the same order, read as stored; what they are scaled to is
settled where collision avoidance first reads them, against the face's
outlines as the four header bytes are. Answers are integers: a glyph and an
attribute number give an `i16`, the first run that holds the attribute
winning; a glyph past the offsets, a block that ends before it starts or past
`Glat`'s end, or a run reaching past its block answers `None`, and so does a
block of more than `Limits::max_attribute_runs` (256) runs, refused by name.

**Features.** `Feat` 1 is the TrueType `feat` table's layout, a 16-bit
feature type and twelve bytes a feature, which is what the vendored Dai Banna
SIL carries; 2 widens the id to 32 bits and inserts a reserved `USHORT`,
sixteen bytes a feature. A setting is a `SHORT` value and a name index. An id
is four characters, or a number where its top byte is 0; this crate asks with
an OpenType `Tag`, read as the id its four bytes spell. A feature's default
is the TrueType chapter's: its setting at index 0, or, where the flags set
both 0x8000 and 0x4000, the setting their low byte names. A feature with no
settings is the GDL's boolean, default 0, values 0 and 1. Flag bit 0 marks
an alias, the GDL's hidden id: it keeps a value of its own, because the GDL
tests the public and the hidden feature separately.

**Languages.** `Sill` is its header, one entry a language and one more — a
four-byte code padded with NULs, a count and an offset from the table's start
— and the settings they point at: a 32-bit feature id, a `SHORT` value and two
bytes of padding. The first entry whose code matches is the language's; the
last entry is the GTF's bogus one and is never matched.

**Feature values, from the shaper's settings.** In this order, each over the
last: every feature's default; the language's `Sill` settings, where
`Shaper::with_language`'s tag names one of its codes — the tag's letters
lowercased and its trailing spaces read as NULs, so `urd ` and `URD ` both
select `urd`, while an OpenType language tag whose letters are not the ISO
639 code (`TRK ` for `tur`) selects nothing; every feature whose id a tag of
`Shaper::with_features` spells, set to 1, every other keeping its value,
because a Graphite feature's 0 is one of the face's settings rather than an
off and its passes run whatever the features; and `Shaper::with_settings`,
each tag the id it spells, the last setting of one tag winning. A tag or a
`Sill` setting naming no feature the face declares, and a value the feature
does not declare, is ignored and named. The values are one `i16` a feature,
in `Feat`'s order, and they travel on every slot; `SetFeat` changes one
slot's, clipped to the largest value the feature declares. `PushFeat`'s
operand is the feature's index in `Feat`'s order: the vendored Tagmukay's
second pass tests its features 0 and 1, which are its `ss01` and `ss02`.

**`Silf`.** A version; from 3 a word holding the compiler version (from 5,
under the scheme's five bits); the description count, two reserved bytes and
an offset a description. A description (*subtable*) is laid out as the GTF's
table gives it, the version notes deciding which fields exist: from 3 the
rule version, `passOffset` and `pseudosOffset`; from 2 the mirroring and
skip-pass attributes, the justification levels, a third reserved byte, the
critical features and the reserved byte after them; and before 4 a class
offset is a `USHORT` where from 4 it is a `ULONG`. `passOffset` and
`pseudosOffset` are read and not relied on: the reader walks the fields to
where the layout puts them, and every vendored face's offsets agree with the
walk — `pseudosOffset` naming `numPseudo`, the field before the mappings,
where the GTF says the mappings themselves. A run takes the first description
whose script tags hold its OpenType script tag, and the first description
where none does.

**Pseudo-glyphs and classes.** The pseudo-glyph map pairs a code point with a
glyph, in code-point order; it is consulted before `cmap`, so a character
`cmap` lacks still reaches its rules — the vendored Dai Banna SIL maps its
combining accents only there — and a pseudo-glyph left in the stream becomes,
at the end, the real glyph its `attrPseudo` attribute names. The class map is
the class count, the count of linear classes and one offset a class and one
more, from the class map's start. A linear class is its glyphs in order, so
an index gives a glyph, and a glyph is found by reading them in order — the
GTF's "very slow, but stored very simply". A lookup class is four header
words and `(glyph, index)` pairs sorted by glyph, so a glyph gives its index
by bisection, and an index is found by reading the pairs. A class past the
count, an index past the class, and a glyph in neither answer `None`. A map
or a pair array out of order is read as written, the first match winning.

**Passes.** Indices from 0 to `iSubst` are line-break passes, from `iSubst`
to `iJust` substitution, from `iJust` to `iPos` justification and from
`iPos` on positioning; the bidi pass runs before pass `iBidi`, and 255 is
none. A pass is its flags — bits 0 to 2 the collision runs, 3 and 4 the
kerning, 5 a reversed pass — its rule-loop, context and backup limits, its
rule count, the offsets of its pass constraint, rule constraints and actions
from the description's start, and its machine: the state, transitional,
success and column counts, the ranges that give each glyph a column, the
rule lists of the success states, the shortest and longest pre-context and
the start state for each, a sort key and a pre-context a rule, the collision
threshold, the pass constraint's length, each rule's constraint and action
offsets, and the transitions. A pass of no columns has no machine and runs
only its collision fixing. Before 2 a pass has no `fsmOffset`, pass
constraint offset, collision threshold, constraint length or the reserved
byte, as the notes say. A pass whose counts disagree — more success or
transitional states than states, or states that the order the GTF gives
cannot hold — is refused by name and does not run; so is every pass past
`Limits::max_passes` (64).

**The machine that finds a rule.** A glyph's column is the range that holds
it, the ranges bisected where they are sorted and disjoint and read in order,
the first winning, where they are not; a glyph in no range jams the machine.
At a scan position `p`, with `maxPre` and `minPre` the pass's longest and
shortest pre-contexts: where `p ≥ maxPre` the machine reads from slot
`p − maxPre` in `startStates[0]`; where `p < maxPre` it reads from slot 0 in
`startStates[maxPre − p]`, the GTF's skipped transitions; where `p < minPre`
no rule matches. Each slot read takes the transition its column gives. A
transition to 0 jams, as the compiler documentation's "a value of zero means
that no match occurred" puts it; one to a state past the table is refused by
name and ends the walk; a state at or past `numRows − numSuccess` accepts,
and its rules join the candidates; a state at or past the transitional count
is final and ends the walk. No more than `Limits::max_context_length` (64)
slots are read; the walk that would read more is refused by name. A rule
whose own pre-context is longer than the slots before `p` is dropped: the
skipped transitions stand for slots that do not exist, and only the rule's
count can say it needed them. The candidates are tried by sort key, highest
first, and by rule number among equal keys — the GTF's "primarily on the
length of the rule and secondarily on its original position", and the
compiled faces' keys are their rules' lengths — at most
`Limits::max_rule_candidates` (1 024) at one position, refused by name past
it; a rule number past the pass's count is refused by name. Each candidate's
constraint decides whether it fires (*The stack machine*).

**The slot stream.** One slot a glyph: its glyph and the real glyph a
pseudo-glyph stands for; the first and last characters it is associated with;
its attributes, initialised from its glyph — advance from the face's metrics,
`breakweight` and `directionality` from the attributes the description names,
collision and sequence attributes from the run starting at `attCollisions`,
justification from each level's — and set by rules: shift, advance and
attachment (to, at, with, level), insert, component references, the user
attributes the description counts, justification width, the bidi level; and
the feature values. A slot stores only what a rule set; every other
attribute reads as its glyph's, so a glyph a rule puts in a slot brings its
own advance and an attribute a rule set stays set. The collision and sequence
defaults are the run of glyph attributes from `attCollisions`, where the
description's flags mark a collision pass: in order `collision.flags`,
`min.x`, `min.y`, `max.x`, `max.y`, `margin` and `marginweight`, then
`sequence.class`, `proxClass` and `order`, then the sequence weights. The
vendored Awami Nastaliq bears the order out — its glyph table's
`collision { margin = STDMARGIN; marginweight = STDMARGINWT }` is 150 and 400
at `attCollisions` + 5 and + 6, its nuqtas' `sequence { class =
ORDER_UNUQTAS; order = ORDER_LEFTDOWN; … valign { height = 275m; weight =
500 } }` 1, 0, 1, …, 275 and 500 from + 7 — and no vendored glyph table sets
the four limits, whose order follows the slot attributes' numbers. A slot
reference — `attach.to`, a component's — names a slot by an id no other slot
of the run has had, so that it survives insertions and deletions, and reads
as the offset from the reading slot to the slot it names, 0 where it names
none or one no longer there. A slot's cluster is the byte offset of the first
character it is associated with. An `Assoc` gives the slot the span of the
input slots it names; an inserted slot takes the characters of the slot after
it, or of the slot before it at the stream's end, the GDL's cursor rule for an
unassociated insertion; a deleted slot's characters join the slot before it,
or the one after it at the start — when its action ends, and only where no
`Assoc` of the action named it, because the GDL's default is for a deletion
"which has not been associated" and the compiled code of its own example
`ZWJ clsCons > _ clsConsJoin:(2 3)` deletes the joiner before it associates
it. At the end a span of slots whose characters overlap or go backwards is
merged to its least, as `Buffer::reorder` merges a reordered span, so
clusters stay monotonic. Every insertion is charged against
`Limits::max_glyphs`, and one past it is refused by name.

**Acting on the stream.** An action edits the stream in place, and its slot
operands count slots of the stream as the action found it, the GTF's "the
glyph that was in the input in the given slot": from the input slot the
current slot is, and for a slot the action inserted from the input slot
before the one it was inserted ahead of. The compiled faces settle that last
point. Awami Nastaliq's `_ cMark1l > @M:M _ / ^ _ cMark2l_1234u
cMark2l_1234u? _=M` inserts at the match's start and copies and associates
the mark three input slots on, `Insert PutCopy 3 Assoc 3`; its
`gAlefMaddaAbove cDiacriticUpper _ > gAlef:1 @2:2 gMaddaAbove:1` inserts at
the match's end and associates the inserted madda with the first slot,
`Assoc −1`, one input slot back from the diacritic it follows. So each
action keeps a journal of what it inserted and deleted and a copy of each
input slot as it was before the action first changed it: `PutCopy`, `PutSubs`
and `Assoc` read those copies, and the computing instructions read each slot
as it stands, a deleted one as it was found. `PutCopy` copies the input
slot's glyph and what rules set on it, keeping the current slot's characters
for an `Assoc` to set; `Insert` puts in a copy of the slot it goes before,
attached to nothing; `CopyNext` leaves the current slot as it stands, which
in place is the input's, and moves on; `Delete` makes the slot before
current, so the slot pointer may stand before the stream's first; `SetFeat`
sets the feature of the slot at its offset, clipped to the largest value the
feature declares. An action that names a slot outside the stream or its
input, a class that gives no glyph, or an insertion past the glyph ceiling
is refused by name at its instruction and ends there; what it did until then
stands. A slot's position and a metric over an attachment cluster are
positioning's to answer (*Positioning*).

**The stack machine.** Code is checked before it ever runs:
every opcode the GTF's table lists and none it marks unimplemented (`NextN`,
`PushIGlyphAttr`, `PushProcState`, `PutSubs2`, `PutSubs3`), every operand
inside the block, every `ContextItem` skip landing on an instruction boundary
inside it and inside any `ContextItem` block around it, each `ContextItem`'s
block one test — it leaves exactly one value — and the stack's depth,
computed from each opcode's pops and pushes, never below empty and never
above `Limits::max_stack_depth`. A constraint holds no instruction that acts
on the stream and an action no `ContextItem`, which has no slot under test
there. Operands are held to the description and the face: a class to the
class map's count, a feature to `Feat`'s, a user attribute to the
description's count, an attachment level and an index to zero and up, and a
slot attribute or metric to the numbers vendored code reveals (*The numbering
an instruction names*), each used only by the instructions that can take it
— an indexed attribute by the indexed instructions, a slot reference by
`AttrSetSlot` and `IAttrSetSlot`, a read-only one never set. Code that fails
is refused by name and never runs: a rule whose constraint or action fails
its check never fires. Operands are the GTF appendix's, which gives
`PushFeat` and `SetFeat` their slot offset and adds the 16-bit class and
attribute forms and the bit operations that the earlier standalone document
lacks; the opcode range the appendix's text states (0x00 to 0x3E) ends four
opcodes before its table does (0x42), and the table is read. Where the table
leaves a reading open it is taken as its notation states: `PushShortU` pushes
its operand unsigned, as its name and braces say, where the appendix's text
calls it signed; `SetBits`' mask and value are signed sixteen-bit, sign
extended; `Trunc8` and `Trunc16` keep the low eight and sixteen bits; `Cond`'s
first popped value, the top, chooses the second popped where it is true and
the third where it is false; `Div` truncates toward zero; and `PushVersion`
pushes 0x00050000, `Silf` 5's stack-machine language version, the latest the
GTF defines. Values are `i32`; arithmetic wraps, as two's complement does on
every target, and a division by zero, or of `i32::MIN` by −1, is refused by
name and ends the code. No instruction jumps backwards, so a block cannot
loop; every instruction executed is charged against
`Limits::max_instructions`, past which the run's remaining code is refused by
name.

**A rule's constraint** is run once for each slot of its match, from its
first pre-context slot to the slot that brought the machine to the state that
accepted it, with that slot the one `ContextItem` compares its offset to —
the GTF's "the slot currently being tested" — and each run starts with true on
the stack. Every other slot offset in a constraint counts from the slot under
test: Awami Nastaliq's pass 14 reads the attached slot's `att.to` inside its
`ContextItem` block at offset 0. A `ContextItem` block skipped for the slot
under test is a test that holds — the GTF's "a test that is irrelevant for
this slot" — so the skip leaves a true in place of the one value the block
would have left. The compiled faces show why both are needed: a constraint
on several slots is their tests joined by one `And` fewer than there are
tests, as in Awami Nastaliq's pass 14, rule 0, whose
`_=D {att.to==0}  _=H {att.to==0}` is two blocks and one `And`. Without a
true for each skipped block a slot under test that is neither would pop past
the stack's bottom; and the true beneath answers for a block with nothing
after it, and for a constraint that is empty. All 431 `ContextItem` blocks in
the vendored faces leave one value, none nests in another, and none is in an
action. A run's answer is the value `PopRet` returns, or the top of the stack
where the code ends without one; the rule fires where every run answers true.
A pass constraint is run once, with nothing under test: its offsets count
from the stream's first slot, and every `ContextItem` in it is skipped. An
action starts with an empty stack and answers the scan offset (*Passes
running*).

**The numbering an instruction names.** The GTF and the GDL name the slot
attributes and glyph metrics an instruction reads and sets but number
neither. Their numbers are read off compiled code: the vendored faces'
`Silf`, against the GDL sources the faces' authors publish beside them, each
number recorded where the step that runs it lands, with the rule it was read
from, and held by a test against the face's code. The slot attributes, each
with the compiled rule it is read from: 0 `advance.x` (Awami Nastaliq's pass
7, rule 0, `seqWidth = @P.advance.x + @P.seqWidth`); 2 `attach.to`, a slot
reference (the same rule's `AttrSetSlot`, and pass 14, rule 0's
`{att.to==0}`); 3 and 4 `attach.at.x` and `.y`, set from the point on the
glyph attached to, and 8 and 9 `attach.with.x` and `.y`, from the point on
the slot's own (pass 7, rule 0); 14 `breakweight` (Dai Banna SIL's
line-break pass); 15 `component.X.reference`, indexed (Tagmukay's pass 0);
16 `directionality` (Awami Nastaliq's pass 6, rule 24,
`_ {directionality == 1}`); 17 `insert` (pass 7, rule 0: cleared on the slot
it attaches, `{insert=1}` on the one after); 19 `position.y`, read only (pass
12, rule 22, `{position.y + bb.top > SHORTFINAL2}`); 20 and 21 `shift.x` and
`.y` (pass 17, rule 17, `shift { x = 300m; y = 350m; }`); 55 `userX`,
indexed by X − 1 (pass 7, rule 0 sets `seqWidth`, its `user5`, as index 4);
57 `collision.flags`, 58 `min.x`, 59 `min.y`, 60 `max.x` and 61 `max.y`, 69
`sequence.class`, 70 `proxClass` and 71 `order` (pass 17, rule 17); 62
`collision.fix.x`, read only (pass 19, rule 16, `advance.x += max(0,
@N.collision.fix.x) + 300m`); 64 `collision.margin` and 65 `marginweight`
(pass 17, rule 69, `margin = 300m; marginweight = 500`); and 66
`collision.exclude.glyph`, 67 `exclude.offset.x` and 68 `exclude.offset.y`
(pass 9, rule 73). The one glyph metric any of their code reads is 2,
`bb.top`, in that pass 12 rule. An attribute or metric no vendored code
reveals is refused by name, which is how the GDL's own constraint example
`_ {bb.height > 1000m}` (4.1.3) is held: no vendored face numbers
`bb.height`, so a block naming any metric but `bb.top` fails its check, and
the comparison the example makes is run through `bb.top`.

**Passes running.** Passes run in the description's order over the whole
run, each over the stream the last left; a reversed pass runs over the stream
turned end to end, which is turned back after it. The substitution stage is
the line-break and substitution passes, from the first to the first
justification pass; the justification passes run when layout's justify call
asks for them (*Justification*), and the positioning passes after. A pass
constraint that is false skips the pass, and so does one refused, by its
check or as it runs. A rule's blocks are checked the first time its pass asks
for them in a run, and kept for the rest of the pass. At each scan position
the first candidate whose constraint holds for every slot of its match fires;
a candidate whose constraint or action fails its check never fires, and the
next is tried. The fired action leaves a slot pointer where its `Next`s put
it, and the scan position becomes that pointer plus the returned offset,
held between the stream's start and end; an action refused as it runs is
named, what it did stands, and the position advances by one. Where no rule
fires the position advances by one. The GDL's safety net: once `maxRuleLoop`
rules have fired without the position passing the furthest it has reached,
it is advanced past it before another rule is tried; and a position may move
back no further than `maxBackup` slots behind that furthest, past which it is
left where it stood when the rule fired, the GDL's "the stream position will
not be set before the consonant, but will be left before the vowel". A positioning pass may
substitute one glyph for one (`PutGlyph`, `PutSubs`) and copy a slot onto
itself (`CopyNext`, `PutCopy` of the current slot); an action of one that
would insert, delete or reorder a slot is refused by name, the GDL's "only do
a one-to-one substitution".

**Bidi.** This crate resolves UAX #9 over the paragraph before itemizing it,
so every slot of a run is at the run's level and the bidi pass reorders
nothing — the GDL's "ABC 123 DEF" is three runs here, each passed in logical
order, and the consumer's per-line reorder draws "FED 123 CBA". Where a
description has a bidi pass and a `Silf` of 4 or later, a slot of a run at an
odd level takes its glyph's mirrored glyph from the mirroring attribute,
whatever `mirror.isEncoded` says, because this crate's shaper mirrors no
character itself; a description with no bidi pass mirrors nothing, the GDL's
"when Bidi is off, the mirroring attributes are considered undefined". The
mirroring is placed where the bidi pass is: before pass `iBidi` where that is
a line-break or substitution pass, and at the substitution stage's end where
it is a justification or positioning pass — `iBidi` is at most `iPos`, and
the GDL puts the bidi pass "at end of the substitution table".

**Positioning.** In the run's direction, as the GDL states it: an advance
moves the slots after it; a shift moves a slot and what is attached to it
and nothing else; `kern` is a shift and an advance at once; an attached slot
puts its `attach.with` point on its parent's `attach.at` point, its shift
added after, and moves the pen not at all; a cluster grows by the guard space
the project documentation's font topics describe, so a diacritic with an
advance, or with its origin before its base's, widens its base's cluster; and
a positive shift or advance moves "further along" the run, so in a
right-to-left run it moves left. The guard space is the font topics': a
member pushes its cluster's advance out to where its own advance ends, where
that advance is not 0; and it pushes the cluster's origin out to its own,
the base moving right, where its origin is left of the base's and, as the
topics put it, "the origin of the diacritic is to the left of where it is
attached or the advance is non-zero" — so of the topics' four tildes over a
dotless i, the one drawn like a base pushes both sides, the one drawn left of
its origin with no advance neither, the one drawn right of its origin with no
advance the left alone, and the one finishing at its origin with an advance
the right alone. Shifts move no pen and widen no cluster. Attachment chains
are resolved from the root, at most `Limits::max_attachment_depth` deep, and
a cycle is refused by name with the slots left where their pens put them;
the slot past the depth ceiling is placed the same way. While the
positioning passes run, `position.y` reads a slot's height so far — its
chain's `attach.at.y − attach.with.y` and every shift along it, its own
included — and a glyph metric at a level of 1 or more reads the cluster of
the slot and every slot attached to it: `attach.level`, which no vendored
code numbers, is refused by name, so every attachment is at the GDL's default
level 1 and the levels past 0 coincide. A run comes out as the crate's
convention has it: one `ShapedGlyph` a slot in logical order, an advance and
an offset from the pen, x to the right, a cluster's root carrying the
cluster's advance and its members none; a right-to-left run is drawn by
walking its glyphs backwards, as every run of this crate is, so the pen before
a glyph is the advances of the glyphs after it.

**Collision avoidance.** The GDL states what the search weighs and not how
it searches: the distance moved, a weight of 1; intrusion into a
neighbour's margin, weighted by `marginweight`; the sequence regions; each
fixable slot kept inside its movement rectangle, whose x limits are right and
left whatever the run's direction; a cluster bounded by the start and end
flags; kerning that adds space after a kerned slot so the next cluster clears
its margin, counting the width of an intervening space and ignoring the y
limits. This design's search is its own and integer. A glyph's shape is its
box from `glyf` cut by its octabox's four diagonals, read as FT-07d.4 holds
them, and the shape of the exclusion glyph a slot names at its offset. The
sub-boxes are not used: their bytes match the vendored Awami Nastaliq's
outlines neither as 255ths of the glyph's box nor as 255ths of their cell, so
their scale is not settled and a guessed one would move glyphs silently. A
range is the slots from one flagged to start a range to one flagged to end
it, a run with neither one range; a slot flagged to ignore is neither moved
nor in the way, and a glyph with no outline has no shape. For each slot
flagged to move, in stream order, the candidates are its place before any
fix, the fix it has, and the places inside its rectangle at which an edge of
its shape meets an edge of a neighbour's in its range, widened by the
neighbour's margin, along x and along y. Each is weighed by the distance
moved, a weight of 1; the ink it would overlap, at a weight past any move a
rectangle allows; the neighbour's margin it would intrude on, weighted by its
own `marginweight`; `NOABOVE` and `NOBELOW` against its proximate class's
glyphs, kept as ink is; and its alignment with a glyph of its own class, the
dearer by `sequence.valign.weight` the nearer their centres are than
`valign.height`. The diagonal sequence regions of `LEFTDOWN` are not weighed:
the GDL draws them in a figure whose geometry its text does not fix. The
least-cost candidate wins, the nearest to its start breaking a tie; a slot no
candidate clears of ink keeps its place and is named; a move smaller than
the pass's collision threshold is not made; and the ranges are run over as
many times as the pass's flags give. A kerned slot's advance then changes by
the least that leaves the sequence after it — up to the next slot kerned,
past any space — clear by its margin and the width of the spaces between,
along x alone and wherever the two overlap in height, inside its x limits; a
sequence nothing overlaps in height is kerned in as far as `min.x` allows. A
slot's fix is read back as `collision.fix.x` and moves where positioning
places it. Every slot pair compared is charged against
`Limits::max_collision_work`, past which the slots not yet fixed or kerned
keep their places, and that is named.

**Justification.** A description's levels name, for each, the glyph
attributes of its stretch, shrink, step and weight; layout's justify call
(FT-07e) hands a run the slack its line wants, which is spread over the
slots of the first level that can take it, by weight, within each slot's
stretch or shrink and in its steps, into each slot's `justify.width`; the
justification passes then run, and the positioning passes after them. A
description with no levels takes the GDL's level 0: white space stretched to
100 times its width and shrunk to 75 %.

**The output.** Each slot's glyph, a pseudo-glyph's real one, its cluster,
advance and offset; the run's warnings, each once (ruling 10).

**Ceilings.** Every count a hostile description can spend is bounded, and the
work past each bound is refused by name.

| Bound | Default | What it bounds |
| --- | --- | --- |
| `Limits::max_table_bytes` | 16 MiB | a compressed table's stated size |
| `Limits::max_attribute_runs` | 256 | the runs one glyph's `Glat` block is read through |
| `Limits::max_passes` | 64 | the passes a description runs |
| `Limits::max_context_length` | 64 | the slots one rule's machine reads (the OpenType bound, shared) |
| `Limits::max_rule_candidates` | 1 024 | the rules one position tries |
| `Limits::max_stack_depth` | 256 | one block of code's stack |
| `Limits::max_instructions` | 4 000 000, or 64 000 a glyph where that is more | the instructions a run executes |
| `Limits::max_glyphs` | as today | the slots insertions may grow the stream to |
| `Limits::max_attachment_depth` | 64 | an attachment chain |
| `Limits::max_collision_work` | 4 000 000, or 64 000 a glyph where that is more | the slot pairs collision fixing compares |

Rules, states and transitions are bounded by their tables' own sizes, which
the walk reads no further than its context bound; a pass's own `maxRuleLoop`
and `maxBackup` are the face's bounds on itself.

**Warnings.** Until shaping lands, a run over a face whose only layout tables
are Graphite's carries `Warning::TableNotRun { table: Silf }` once. The
readers' refusals are a `graphite::Refusal` each, naming its table, glyph,
pass, rule, state or feature as the case has it; once the passes run they
reach a run's warnings as `Warning::Graphite`, each once.

**The examples each mechanism is held to.** Every example is transcribed as
table bytes the test assembles — the GDL rules hand-compiled into the
tables, machines and code the GTF lays out — because no Graphite compiler is
on this machine and no outside tool makes an input. Where the source's
example is a rule, the test states the rule beside the bytes.

| Mechanism | Examples | Count | Step |
| --- | --- | ---: | --- |
| Compression | The LZ4 Block Format's three literal lengths (48 as 15, 33; 280 as 15, 255, 10; 15 as 15, 0) and its offset of 1 repeating the last byte | 4 | FT-07d.4 |
| `Glat` and `Gloc` | The GTF's sub-box bitmap: bit 3 the lower right cell, bit 12 the upper left | 1 | FT-07d.4 |
| `Feat` | The `feat` chapter's flags 0xC001, the default setting index 1; the GDL's `ligatures` feature of four settings, default `std`; the GDL's `eng` with its hidden `Engs` | 3 | FT-07d.5 |
| `Sill` | The GTF's `en` as the bytes 101, 110, 0, 0; the GDL's language table of `sindhi`, `kurdish` and `urdu` | 2 | FT-07d.5 |
| Class maps | The compiler documentation's linear class 6 and indexed class 10 | 1 | FT-07d.6 |
| Finding a rule | The GDL's `X > Y / A _ A` over `A X A X A X A` (4.1.1); `A B C > X Y Z` before `A > D / _ B` (4.1.7); `B > Y / W A? _ C` and `C > D / B _ E` over `WABC` and over `WBCE` (4.1.7.1); `A > B / W _` and `A > C / _ X Y` over `WAXY` (4.1.7.2) | 5 | FT-07d.6 |
| Computing | The compiler documentation's 11509 pushed as 44 and −11; the GDL's `_ {ligatures == all}` (4.1.2), `_ {bb.height > 1000m}` (4.1.3) and the default test of a public and a hidden feature (6.9.2.1) | 4 | FT-07d.7 |
| Acting on the stream | The compiler documentation's `PutSubs 0 10 6`; the GDL's selection from a class (4.4.1), reordering (4.4.2), association (4.4.3), the dependent I moved by insertion and deletion, the joiner deleted into its consonant and the Thai split vowel (4.4.4), the ligature of three (4.4.5) and a user attribute set in one table and read in another (6.3) | 9 | FT-07d.8 |
| Substitution passes | The outputs of 4.1.1 (`A Y A Y A Y A`), 4.1.7.1 (`WAYC`, `WYCE`) and 4.1.7.2 (`WCXY`); the example file's dotless i, tone and modifier reordered, pitch letters as numbers, and raised glyphs (5.1); the alternating `A1 A2` chain backed up over (6.4.1) | 9 | FT-07d.9 |
| Bidi | The GDL's `ABC 123 DEF` (4.1.4.2); a parenthesis mirrored at an odd level (6.6) | 2 | FT-07d.9 |
| Positioning | Diacritics stacked through `^` (4.1.1); a shift (4.6.1); an advance (4.6.2); `kern.x` as a shift and an advance (4.6.2); composite metrics at levels 0, 1 and 2 (4.6.3); the lam-alef ligature's diacritics (4.6.8.1); the dotless i with its tilde (4.6.8.2); the four guard-space cases of the font topics | 11 | FT-07d.10 |
| Collision | The GDL's Nastaliq example: nuqtas fixed inside spaces' starts and ends (its pass 2), diacritics fixed and finals kerned (its pass 4), each held to what its text states — the collision removed, every glyph inside its rectangle, kerning along x alone | 2 | FT-07d.11 |
| Justification | The GDL's stretched and shrunk space (6.5.4); the five kashidas' stretch and step (6.5.4); the wide glyph substituted and the rest kerned (6.5.5); kashidas inserted while width remains (6.5.6.1); the ligature undone to stretch (6.5.6.2); level 0's white space (6.5.2) | 6 | FT-07d.13 |

Fifty-nine in all. The specification gives no worked example for `Silf`'s
header or a pass's layout; those are held to the vendored faces, whose
readings the tests state.

**The fixture faces.** Three faces under
`crates/tinker-pdf-shape/data/graphite`, each whole and unmodified — the OFL
reserves their names to the original versions, and cutting a table out would
make a modified one — each with its upstream licence:

| Face | What it carries | What H-64's person writes for |
| --- | --- | --- |
| Dai Banna SIL 4.000, Regular | `Silf` 4.0 with one line-break pass and one substitution pass, `Glat` 1, short `Gloc`, `Feat` 1 and an empty `Sill`; no OpenType Layout table, so it is the face the refusal and the Graphite path run on | the Latin letters its substitution pass composes with a combining accent from the pseudo-glyph map (`e` U+0301, `C` U+0327, `n` U+0303, `o` U+0308, `a` U+030A, `s` U+030C) and an accent after a letter it does not compose (`b` U+0301); New Tai Lue syllables with a prevowel, `ᦶᦂ` among them |
| Tagmukay 2.000, Regular | `Silf` 2.0 with two substitution passes of ligatures and component references, `Glat` 1, `Feat` 2 with `ss01` and `ss02`, and a `GSUB`, so the whole face takes the OpenType path | Tifinagh with its consonant ligatures, with each of `ss01` and `ss02` on and off |
| Awami Nastaliq 3.500, Regular | `Silf` 5.1 and `Glat` 3 compressed, octaboxes, long `Gloc` with 214 attributes, `Feat` 2, a `Sill` of five languages, twenty-one passes with pass constraints, a backed-up pass, a reversed pass, collision passes and kerning; and the one-lookup `GPOS` the precedence turns on | Urdu words through every pass, with the `ks` and `kas` languages' settings and the `hehk` and `lamv` features on and off |

H-64's person writes the expected glyphs and positions in the case format
FT-09.1 sets for every person-written expectation, the first of the formats
the person-written rows land, and FT-07d.17 reads them through it.

### Graphite milestones

1. **The refusal** (FT-07d.2): `Warning::TableNotRun`. Exit,
   `tests/graphite.rs`:
   `a_face_carrying_only_graphite_tables_is_named_and_shaped_unshaped`.
2. **The faces** (FT-07d.3). Exit: `cargo xtask vendor` green;
   `the_fixture_faces_are_the_ones_vendored`.
3. **Glyph attributes** (FT-07d.4): `graphite/mod.rs`, `graphite/lz4.rs`,
   `graphite/glat.rs`. Exit:
   `every_glat_version_answers_the_attributes_it_stores`,
   `an_offset_past_the_glat_answers_none`, the compression and `Glat`
   examples counted.
4. **Features and languages** (FT-07d.5): `graphite/feat.rs`. Exit:
   `a_feature_keeps_its_default_unless_a_setting_names_it`,
   `a_language_takes_its_sill_settings`,
   `a_setting_the_face_does_not_declare_is_ignored_and_named`, the `Feat`
   and `Sill` examples counted.
5. **`Silf` and its machines** (FT-07d.6): `graphite/silf.rs`. Exit:
   `a_pass_finds_the_rule_its_machine_selects`,
   `a_class_map_answers_both_kinds_of_class`,
   `a_transition_past_its_table_is_refused_and_named`.
6. **Computing** (FT-07d.7): `graphite/machine.rs`. Exit:
   `each_computing_instruction_does_what_the_specification_states`,
   `code_that_fails_its_check_never_runs`,
   `a_program_past_its_budget_is_refused_and_named`.
7. **Acting** (FT-07d.8): `graphite/slot.rs`. Exit:
   `each_action_instruction_rewrites_the_stream_as_specified`,
   `an_inserted_slot_takes_the_cluster_the_design_states`,
   `an_insertion_past_the_glyph_ceiling_is_refused_and_named`.
8. **Substitution passes** (FT-07d.9): `graphite/pass.rs`. Exit:
   `substitution_passes_reproduce_the_specifications_examples`,
   `a_rule_that_never_moves_the_pointer_stops_at_the_loop_ceiling`,
   `a_right_to_left_run_is_passed_in_the_order_the_design_states`.
9. **Positioning** (FT-07d.10): `graphite/position.rs`. Exit:
   `positioning_passes_reproduce_the_specifications_examples`,
   `an_attached_slot_is_placed_at_its_attachment_point`,
   `an_attachment_cycle_is_refused_and_named`.
10. **Collision** (FT-07d.11): `graphite/collision.rs`. Exit:
    `collision_avoidance_reproduces_the_specifications_examples`,
    `two_colliding_slots_are_moved_apart_within_their_limits`,
    `a_slot_with_no_room_keeps_its_place_and_is_named`.
11. **Shaping** (FT-07d.12): `Shaper::shape` runs the passes and the refusal
    retires. Exit: `a_graphite_only_face_is_shaped_through_its_passes`,
    `a_face_carrying_both_takes_the_path_the_design_states`;
    `epub_shaped.rs`'s
    `a_book_in_a_graphite_face_is_shaped_and_measured_as_drawn`, in the
    default and `bundled-fonts` builds.
12. **Justification** (FT-07d.13): `graphite/justify.rs`. Exit:
    `a_line_is_justified_by_its_faces_levels`; `epub_shaped.rs`'s
    `a_justified_line_in_a_graphite_face_takes_its_levels`.
13. **The fuzzer** (FT-07d.14). Exit:
    `every_graphite_fuzz_seed_shapes_without_panicking`; `cargo xtask check`.
14. **The fingerprint** (FT-07d.15). Exit: `fingerprints.rs`'s
    `graphite_shaping_is_stable_across_targets`, on Linux, Windows and
    `wasm32-wasip1`.
15. **The speed row** (FT-07d.16): `shape a Graphite paragraph` in MS-04's
    fonts group, through Awami Nastaliq's Graphite tables. Exit:
    `the_committed_baseline_agrees_with_the_tree`; `cargo bench -p tinker-pdf`.
16. **Person-written expectations** (FT-07d.17, with H-64). Exit:
    `every_person_written_graphite_fixture_is_reproduced`,
    `every_specification_example_is_transcribed`.

### Graphite risks

| Risk | Mitigation |
|---|---|
| The specification numbers no slot attribute and no glyph metric, and a number read wrong moves the wrong thing silently | Each number is read off a vendored face's compiled code against its published source and held by a test against that code; an attribute no vendored code reveals is refused by name rather than guessed; H-64's fixtures run the faces whole |
| The two GTF editions disagree (the octabox diagonals' order, a lookup class's search range, `PushFeat`'s operands, the stack machine's opcode range) and `pseudosOffset` names a field the text does not | Each disagreement is settled in *Graphite design* by the vendored faces' bytes, and a test holds the reading to them |
| Collision avoidance's search is described by its costs, not by its steps, so a face tuned to another search can come out units apart | The search is integer and deterministic, inside every limit the GDL states; H-64's fixtures for Awami Nastaliq are what hold its positions |
| A Graphite-first face carries a stub `GPOS` for another script, as Awami Nastaliq does | The precedence asks whether the run's script has a lookup to take, not whether a table is present, so its Urdu takes its passes; a run whose script the OpenType tables cover keeps today's path |
| A hostile description: offsets anywhere, counts that lie, a machine that never jams, code that loops or divides by zero, attachment cycles | Every read through `read::Bytes`; every count bounded (*Ceilings*); code checked before it runs; the `shape` fuzz target drives each table directly and through the sfnt directory (FT-07d.14) |
| A compressed table names a size it does not decode to, or a size meant to exhaust memory | The stated size is bounded before a byte is decoded, and the decoded length must equal it |
| An inserted or reordered slot breaks cluster monotonicity | Clusters are merged as `Buffer::reorder` merges them, and the fuzz target's assertion that clusters never go backwards carries over |
| The faces' licences | OFL-1.1, which `deny.toml` admits (ruling 18); each face vendored whole with its licence beside it |

## Justification

A justified line today takes all of its slack in its spaces
(`crates/tinker-pdf-layout/src/flow.rs`, the alignment pass of a line). Two
more ways to fill a line are written down, and this section designs both: a
face's own justification suggestions, which ISO/IEC 14496-22 stores in its
`JSTF` table — lookups to enable or disable, and lookups whose values are a
maximum adjustment — and **kashida**, the elongation of the join between two
letters of a cursive script, written here as extender glyphs inserted at the
join. CSS Text 3's `text-justify` says which of them a line may use.

The `JSTF` rules below are transcribed from the table's definition as
OpenType 1.9.1 publishes it (Microsoft's `JSTF` chapter), the text
ISO/IEC 14496-22 holds for the table; `text-justify` from CSS Text Module
Level 3, §6.4 and its subsections; and the kashida ranking from two published
sources on Arabic justification — W3C's *Text Layout Requirements for the
Arabic Script*, §7.2.5, and Elyaakoubi and Lazrek, "Justify Just or Just
Justify", *Journal of Electronic Publishing* 13(1), 2010, which states the
calligraphic rules of Benatia, Elyaakoubi and Lazrek, "Arabic text
justification", *TUGboat* 27(2), 2006.

### Scope

- **`text-justify`** (`css-text-3` §6.4): `auto`, `none`, `inter-word`,
  `inter-character` and the legacy `distribute`, which computes to
  `inter-character`, parsed, cascaded and inherited into layout's style, each
  value given the method this build has for it.
- **`JSTF` read** (FT-07e.2): its script records, each script's extender
  glyphs, default and per-language `JstfLangSys` tables, each language
  system's priorities, each priority's eight `JstfModList`s — the `GSUB` and
  `GPOS` lookups to enable and to disable, for shrinkage and for extension —
  and its two `JstfMax` tables of `GPOS`-type lookups; carried on `Layout`
  beside `GDEF`, `GSUB` and `GPOS`.
- **The shaper's justify call** (FT-07e.3, FT-07e.4): a shaped run and a
  width to gain, in integer font units, in; the run with the width taken by
  `JSTF`'s priorities and then by kashida, and how much was taken, out.
- **The layout seam** (FT-07e.6): a justified line offers its runs their
  share of its slack through `metrics::Shaper`, records what each run's
  shaper took on its `TextRun`, and spreads the rest over the spaces as
  before.
- **The painter** (FT-07e.7): each justified run drawn with the glyphs the
  call gave layout, inserted glyphs included, and extracted as the text the
  book holds.
- **The price of the default** (FT-07e.8): a switch that sets `auto` on a
  joining run as `inter-character` is set, for H-143's figures.

### Non-goals

- **Shrinkage in line breaking.** A line is broken at its natural widths,
  first fit, as today, and justification only ever *adds* to a line: layout
  never asks for less than the natural width, so `JSTF`'s shrinkage lists and
  shrinkage `JstfMax` are read and applied by the justify call when it is
  asked for a negative width, and no line asks for one. A breaker that let a
  line hold more text by shrinking it would measure every candidate line
  twice, and §6.4.1 makes negative space the user agent's option, not a
  requirement.
- **Spacing between the letters of scripts that do not join.**
  `inter-character`'s own method for Latin, CJK or Thai text — the slack
  spread between every pair of typographic character units — is not built
  here; such a run under `inter-character` is justified over its spaces and
  counted (*What `text-justify` asks*, below).
- **Calligraphic elongation.** Curvilinear kashida, stretched letter
  variants (`jalt`), swashes and compressed ligatures are not chosen; an
  elongation is extender glyphs on the baseline. A run whose joins `curs`
  attached takes no kashida, since a flat extender between glyphs joined off
  the baseline breaks the join.
- **Paragraph-wide optimisation.** No optimum-fit line breaking, no penalty
  for elongations stacked in consecutive lines; each line is justified alone.
- **Columns.** Justification of vertical text (FT-07b's columns) is not
  designed here.
- **Variations.** A `JstfMax` lookup's values at an instance of a variable
  face are FT-07a's, as every `GPOS` value's are; here they are read at the
  default.

### Design

#### What `text-justify` asks, and what this build gives each value

`text-justify` (§6.4) is `auto | none | inter-word | inter-character`,
initial `auto`, inherited, applying to text; `distribute` computes to
`inter-character` for legacy reasons. A word separator's justification
opportunity is its parent's value's, and an opportunity between two
typographic character units is their nearest common ancestor's (§6.4.1), so
layout reads the value **run by run**: a space belongs to the run that holds
it, and kashida is offered only between two letters of one run, whose nearest
common ancestor is that run's element. A join across a span boundary takes
no kashida.

| Value | Method in this build |
| --- | --- |
| `auto` | the slack over the line's spaces, as before, in every script — H-143's answer |
| `inter-word` | the same: word separators only, §6.4's own description |
| `none` | no justification opportunity: the run's spaces take nothing, and a line none of whose runs offers one is set at its start, as §6.4.3 sets a line that cannot be stretched under `text-align-last: auto` |
| `inter-character`, `distribute` | in a run that joins, the run's share of the slack is offered to its shaper's justify call — `JSTF`'s priorities, then kashida, §6.4.4's "cursive elongation" — and what the call does not take goes to the spaces; in any other run, the spaces |

**The value that selects kashida and `JSTF` is `inter-character`.** §6.4.4
forbids gaps between the joined letters of a cursive script and lets a user
agent "translate space distributed to justification opportunities within a
run of such typographic letter units into some form of cursive elongation";
`inter-character` is the value whose opportunities fall within such a run,
and `inter-word` puts none there. H-143 keeps `auto` on the spaces, so a book
asks for elongation by naming `inter-character`. A run "joins" as the
Arabic shaper decides it does (`arabic::joins`): it holds a letter whose
joining type joins.

**What is refused, and when.** FT-07e.5 reads all five keywords into a
`TextJustify` of four values, and refuses `inter-character` and
`distribute` by value — `Declaration::Unsupported`, counted against
`text-justify` by element, as `hyphens: auto` is — because until FT-07e.7's
provider answers the justify call nothing in a book honours them, and a
value parsed and not honoured is the failure the property table exists to
prevent. FT-07e.6's layout tests set the value on a computed style directly.
FT-07e.7 lifts the refusal in the same commit that makes the call answer,
and the facade then counts `text-justify` against each element whose
`inter-character` run does not join, at paint time where a run's text is
known, as it counts `font-kerning` against the standard 14: that is the half
of `inter-character` this row does not build.

#### `JSTF`, read (FT-07e.2)

`jstf::Jstf` is a view over the table's bytes, every read through
`read::Bytes` (ruling 1), every offset measured from the table that holds
it, a NULL offset absent:

- **The header**: `majorVersion` 1 — another is not read — and
  `jstfScriptCount` `JstfScriptRecord`s, each a tag and an offset. A script
  is found by its tag, the records scanned in their order; the tag is the one
  the run's `GSUB` script resolved to.
- **`JstfScript`**: the `ExtenderGlyph` table — a count and that many glyph
  ids, in the table's order — the default `JstfLangSys` and
  `jstfLangSysCount` records of a tag and an offset. The run's language
  selects a record; with none, or none for its tag, the default.
- **`JstfLangSys`**: `jstfPriorityCount` offsets to `JstfPriority` tables, in
  priority order, the first the least obtrusive suggestion.
- **`JstfPriority`**: ten offsets, in the chapter's order —
  `gsubShrinkageEnable`, `gsubShrinkageDisable`, `gposShrinkageEnable`,
  `gposShrinkageDisable`, `shrinkageJstfMax`, `gsubExtensionEnable`,
  `gsubExtensionDisable`, `gposExtensionEnable`, `gposExtensionDisable`,
  `extensionJstfMax`.
- **`JstfModList`**: a count and that many lookup indices into the `GSUB` or
  `GPOS` `LookupList`.
- **`JstfMax`**: a count and that many offsets to lookups of the `GPOS` form,
  in design order, read by the `GPOS` lookup reader over the `JstfMax`
  table's own bytes; a contextual positioning lookup there, which the
  chapter excludes, is skipped with a warning.

An offset past the table, a count past its bytes, or a version other than 1
answers `None` for what it leads to, never a panic. `Layout` carries the
table as `Layout::jstf`: `Layout::parse` reads it from the face, and
`Layout::with_jstf` adds it to a `Layout::from_tables`, whose signature
stays as it is. The `shape` fuzz target reads every structure of it.

#### The justify call (FT-07e.3)

`Shaper::justify(text, run, shaped, width)` takes the run as `Shaper::shape`
shaped it and the width to gain, in the face's integer units, positive to
extend; it returns the justified run and the units it took, never more than
`width`, and which priority and how many inserted glyphs took them.

1. **The language system** is the run's script's `JstfScript` and its
   language's `JstfLangSys`. A face with no `JSTF`, or none for the run,
   skips to kashida.
2. **Each priority stands alone**, the chapter says, "its suggestions are
   not added to the previous levels", and a suggestion that modifies a lookup
   already applied is applied "to an unmodified version of the glyph string".
   So priority `k` re-shapes the run from its text with its plan changed at
   the lookup level: the extension-enable lookups added and the
   extension-disable lookups removed, `GSUB`'s and `GPOS`'s each, the list
   reassembled in `LookupList` order. Its gain is the re-shaped run's advance
   less the original's.
3. **`JstfMax`.** The priority's extension `JstfMax` lookups run over the
   re-shaped run, each value record at its full value, through
   `apply::Runner` over their own lookup list. That is the most the priority
   may add; "a text-processing client may choose to adjust a glyph's
   positioning by any amount from zero to the specified maximum", so when the
   width left is less, each glyph's adjustment is scaled to it in integers —
   `adjustment × left ÷ maximum`, rounded toward zero, the units the rounding
   lost given one at a time to the adjusted glyphs in logical order.
4. **The choice.** A priority whose lookup changes alone gain more than the
   width overshoots and is not taken. Of the rest, the first whose gain and
   `JstfMax` together meet the width ends the search; with none, the one that
   took the most, the earlier on a tie. The search stops at the first that
   meets it, so a priority is re-shaped at most once.
5. **Kashida**, below, takes what is left, where the run joins.

Shrinkage is the same with the four shrinkage lists and the shrinkage
`JstfMax`, for a negative width, taking no more than it is asked for.

#### Kashida: opportunities and their ranking (FT-07e.4)

**An opportunity** is a join between two letters of one word of the run:
the first letter in a form that joins forward (initial or medial,
`arabic::forms`), the next one joining backward (medial or final), the two in
different glyphs — a ligature's components share one and have no join
between them to elongate — and no cursive attachment between them. The
inserted glyphs go after every glyph of the first letter's cluster, its marks
included, so a mark stays on its letter. A word is a stretch of the run
between characters that do not join.

**What is never elongated**: a word of two letters ("strictly prohibited",
JEP 2010), and the joins the same source prohibits for the letter Beh —
Beh followed by Ain, Seen, Feh, Qaf or Kaf — read for every letter of Beh's
skeleton by Unicode's `Joining_Group`: a letter of group `BEH`, `NOON` or
`YEH` followed by one of `AIN`, `SEEN`, `SAD`, `FEH`, `QAF` or `KAF`.
`ArabicShaping.txt` is vendored for its `Joining_Group` field at the pinned
Unicode version beside `DerivedJoiningType.txt`.

**The ranking**, each opportunity a key compared field by field, the
smallest first:

1. *Approval*, from the same rules: 0 for a join JEP 2010 approves — Beh's
   skeleton followed by `TAH`; 2 for one it authorises without approving —
   Beh's skeleton followed by `ALEF`, `HAH`, `DAL`, `REH` or `LAM`; 1 for
   every other.
2. *The word's length*: 0 for four letters or more, 1 for three — "usually
   kashida is not used in a word composed of three letters", so only where
   no longer word offers one.
3. *The place in the word*: the distance from the join after the second
   letter, "it is preferable to use kashida in the second letter".
4. *The place in the run*, the word nearest the run's logical end first:
   calligraphers elongate near the end of the line, and the run's end stands
   for the line's, which is where layout offers first.
5. *The cluster*, the earlier first, so the order is total.

**The fill.** One elongation a word — W3C's §7.2.5, "avoiding multiple
kashidas in a single word" — at the word's best opportunity, the words taken
in their best opportunities' order. An elongation takes extender glyphs
while the next one keeps its own length within the face's em and the run's
gain within the width, and the next word is elongated only when the one
before is full: so a line is elongated once where one elongation fills it,
JEP 2010's convention of one elongation to a line, and more only where the
slack is longer than an em. The extender glyphs are the `ExtenderGlyph`
table's for the run's script where the face has one — the longest that fits
first, the lower glyph id on a tie — and otherwise the face's glyph for
U+0640 TATWEEL, which joins on both sides (`arabic.rs`'s
`tatweel_joins_on_both_sides`), so no joining form changes; a face with
neither inserts nothing. Each inserted glyph takes its `hmtx` advance and no
offset, carries the **cluster of the letter before it**, and is marked
inserted on the `ShapedGlyph`, which is how the painter writes it with no
text. The glyph ceiling (`Limits::max_glyphs`) bounds the insertions; the
first glyph past it is not inserted and the run records a warning naming it.

#### The layout seam (FT-07e.6)

`metrics::Shaper` gains `justify(text, font, rtl, context, width)`, the
width in points, answering the points the provider's shaper took — at most
`width` and never negative — and, by default, zero: a provider that does not
justify declines, and its lines are justified over their spaces exactly as
before. A justified line (`text-align: justify`, not its last line, not a
line ended by a preserved newline) with slack:

1. offers its runs whose `text-justify` is `inter-character`, and which the
   shaper measured, the slack still left, **from the line's logical end
   toward its start**, each in the context it was measured in;
2. adds what a run took to its width and records the offer and the take on
   the run (`TextRun::justify`), so that the painter asks the same question
   and gets the same answer;
3. spreads the rest over the spaces of the runs whose `text-justify` is not
   `none`, as `extra_per_space` does today; with none, sets the line at its
   start.

`flow.rs` asks for the shaper in its one place and keeps to it, so a run is
measured, justified and drawn by one path (`tests/shaper.rs`'s
`a_justified_run_is_measured_by_one_path`: a provider whose `advance`
panics still justifies). The layout crate gains no dependency edge.

#### The painter, and extracted text (FT-07e.7)

`BookMetrics` answers the call through `tinker-pdf-shape`: the width in
points is the face's units `round(width × units_per_em ÷ size)`, the call
runs, and what it took comes back as `units × size ÷ units_per_em`, each one
multiply and one divide. The painter shapes a justified run and calls the
same justify with the offer layout recorded, in the same context, so the
glyphs it draws are the glyphs layout measured.

**An inserted glyph stays out of the text.** `PageBuilder::glyphs` and
`PageBuilder::glyph_run` record a glyph's `/ToUnicode` text by the first
non-empty text it is drawn with, one text a code. An inserted extender is
written with **empty text**, so it never sets its glyph's entry; but the
extender is the face's tatweel glyph, and a book that also *types* a tatweel
gives that glyph the text "ـ", which every inserted copy would then extract
as. So a run holding an inserted glyph is drawn inside a stated span —
`PageBuilder::begin_stated`, a `/Span` whose `/ActualText` is the run's
text as the book holds it (14.9.4) — and a reader reads the typed text in
place of the glyphs, whatever the extender's `/ToUnicode` says. The cluster
an inserted glyph carries is its letter's, so the run's glyph-to-text
correspondence is unchanged everywhere the span is not read.

#### The price of the default (FT-07e.8, H-143)

H-143 rules that `auto` is justified over its spaces. FT-07e.8a measures what
the other answer would cost with **the `auto-elongation` cargo feature** of
`tinker-pdf-layout`, forwarded by the facade and off by default: with it on,
`auto` on a run that joins is set as `inter-character` is. The feature
changes nothing else, and no build the repository ships turns it on.

#### What H-64's person writes for (FT-07e.10)

- **Kashida**, over **Noto Naskh Arabic Regular** vendored into
  `crates/tinker-pdf-shape/data/arabic/` under `OFL-1.1`, which `deny.toml`
  admits, declared in `THIRDPARTY.md`. Six lines — بسم الله الرحمن الرحيم,
  محمد رسول الله, كتب الطالب الدرس, سلام عليكم, في البيت (a word of two
  letters beside a longer one) and تطبيق (Beh before Tah) — each justified
  through the call at its natural width plus a quarter, a half and one em,
  and at two em, which one elongation cannot fill. The person writes each
  line's glyphs and positions with the extenders where a reader of the
  script places them.
- **`JSTF`**, over a face `tests/justify.rs` assembles, since no vendored
  face carries the table: Latin glyphs with an `ffi` ligature lookup, a kerning
  lookup, and a `JSTF` of three priorities for `latn` — the ligature disabled
  for extension, the kerning disabled, and a `JstfMax` that widens the space
  by up to 360 units — and the strings "office affinity" and "waffle iron",
  each at widths that the first, the second and the third priority meets and
  one that none meets.

#### Bounds

Ruling 1 and the resource-exhaustion design hold the call to the shaper's
budgets: at most one re-shape a priority, and at most
`Limits::max_jstf_priorities` (64) priorities a call, the rest not tried and
a warning recorded once; `JstfMax` lookups run under the run's `Limits` like
any lookup; insertions under `Limits::max_glyphs`. A `JstfModList` naming a
lookup past its `LookupList` changes nothing.

### Milestones

1. **FT-07e.2, `JSTF` read.** `crates/tinker-pdf-shape/tests/justify.rs`:
   `a_jstf_table_reads_back_as_assembled` (every structure of a table the
   test assembles), `an_offset_past_the_jstf_answers_none`,
   `every_jstf_fuzz_seed_shapes_without_panicking`; `cargo xtask check`.
2. **FT-07e.3, the priorities.** `tests/justify.rs`:
   `a_priority_extends_a_run_by_enabling_its_lookups`,
   `justification_stops_at_the_first_priority_that_fits`,
   `a_jstf_max_lookup_bounds_the_extension`,
   `a_face_without_jstf_returns_the_run_unchanged`, with the `cfg(test)`
   count of one re-shape a priority.
3. **FT-07e.4, kashida.** `tests/justify.rs`:
   `kashidas_fill_the_width_at_the_ranked_opportunities`,
   `an_inserted_kashida_carries_its_letters_cluster`,
   `a_word_with_no_opportunity_is_left_to_the_spaces`,
   `a_kashida_past_the_glyph_ceiling_is_refused_and_named`; `fingerprints.rs`
   `justified_shaping_is_stable_across_targets`.
4. **FT-07e.5, the property.** `tinker-pdf-css`'s `src/tests/parser.rs`
   `text_justify_parses_the_values_the_design_reads` and `src/tests/cascade.rs`
   `text_justify_cascades_as_the_design_reads`, `inter-character` and
   `distribute` refused by value and counted.
5. **FT-07e.6, the seam.** `crates/tinker-pdf-layout/tests/shaper.rs`:
   `a_justified_line_offers_its_slack_to_the_shaper_first`,
   `a_shaper_that_declines_leaves_the_line_justified_by_its_spaces`,
   `a_justified_run_is_measured_by_one_path`; every older layout test passes.
6. **FT-07e.7, books.** `crates/tinker-pdf/tests/epub_shaped.rs`:
   `a_justified_arabic_line_is_elongated_and_extracts_as_typed`,
   `a_typed_tatweel_beside_an_inserted_kashida_extracts_once`,
   `a_justified_line_is_measured_as_drawn`, under `inter-character`, which
   the parser takes from this step; an `inter-character` run that does not
   join counted; default and `bundled-fonts` builds.
7. **FT-07e.8a, the price.** With `auto-elongation` and without it, the
   figures committed here: the books whose pages move, the cases that change,
   time and bytes in release and debug, in both builds; FT-07e.8b's
   wall-clock figure in the timing phase.
8. **FT-07e.9, H-143 landed.** `epub_shaped.rs`
   `a_justified_arabic_paragraph_under_auto_is_set_as_ruled`: over its
   spaces.
9. **FT-07e.10, the person's fixtures.** `tests/justify.rs`
   `every_person_written_justification_fixture_is_reproduced`, the kashida
   and the `JSTF` sets each counted.
10. **FT-07e.11, the speed row.** A justified Arabic paragraph through the
    call in MS-04's fonts group, held by `xtask/src/bench.rs`'s
    `the_committed_baseline_agrees_with_the_tree`.

### Risks

| Risk | Mitigation |
| --- | --- |
| Kashida placement is a reader's judgement, and ruling 13 lets no other engine settle it | The ranking is stated with its sources and adjudicated by H-64's person-written fixtures (FT-07e.10); until they land, every test is a face whose expected glyphs the test computes from the ranking, which shows the ranking applied and not that it is right |
| A line measured with one set of glyphs and drawn with another | One call, asked the same offer in the same context by layout and painter; `a_justified_line_is_measured_as_drawn` |
| An inserted extender extracts as text, or a typed one is lost | Inserted glyphs carry empty text and their run is a stated span with `/ActualText`; `a_typed_tatweel_beside_an_inserted_kashida_extracts_once` |
| A hostile `JSTF` makes justification the denial of service | One re-shape a priority, 64 priorities a call, every lookup under the run's `Limits`, insertions under the glyph ceiling |
| A committed book's pages move | `auto` keeps its spaces (H-143), `inter-character` is refused until FT-07e.7 and no committed book declares `text-justify`; `epub_fallback.rs` and `epub_conservation.rs` hold the books |
| Flat extenders break joins off the baseline | A join `curs` attached takes no kashida |
| `inter-character` half built | The half not built is named here and counted by element in the books |

## Person-written expectations

*For ROADMAP row FT-09.* For a script neither published corpus covers, the
only expected answer ruling 13 admits is one a person wrote: text-rendering-tests
and aots are admissible because people wrote their expectations, and another
shaper's recorded output never is. H-64 is the person's act — a reader of the
script writing, from the face's own tables, the glyphs and positions the
script needs. This section fixes the format that person writes in, the
provenance every case carries, where the cases and faces live, the bar they are
wired into, how a script leaves [features/fonts.md](../features/fonts.md)'s
shaped-and-unverified row, and the rules of the two mechanisms refused until a
case exists: Syriac's Alaph and the topographical features of a joining script
the Universal Shaping Engine shapes.

The Alaph rules are read from the Unicode Standard, version 17.0.0, §9.3.2
*Syriac Shaping*, and from Microsoft's *Developing OpenType Fonts for Syriac
Script*, *Shaping features* and *Features*; the topographical stage from
Microsoft's *Creating and supporting OpenType fonts for the Universal Shaping
Engine*, *How the Universal Shaping Engine works* and *OpenType feature
application II*.

### Scope

- **The case format** (FT-09.1): a text file a person writes by hand —
  the text as code points, the face, its feature settings, language and
  direction, and per glyph its name or index, its cluster, its advance and its
  offsets — readable without reading this crate. It is the format every
  person-written row reads: FT-07a.8, FT-07b.10, FT-07d.17 and FT-07e.10 each
  add the fields their harness needs in their own section, by the extension
  rule below.
- **Provenance**: the author, the date, the author's standing as a reader of
  the script, and the face's source and SPDX identifier, whose absence refuses
  a case.
- **The bar** (FT-09.2): `crates/tinker-pdf-shape/tests/person_written.rs`,
  shaping each case as a consumer does and comparing it glyph by glyph, with a
  per-script ratchet and a `RAN` line.
- **Leaving the list** (FT-09.3), one delivery at a time.
- **Two mechanisms' rules**: Alaph's three forms (FT-09.4) and the
  topographical features under the Universal Shaping Engine (FT-09.5).

### Non-goals

- **Writing an expectation.** Nothing here, and no step, writes a case on a
  person's behalf. The bar's own check transcribes a published case,
  `SHARAN-1/1`, which people wrote; every delivered case is H-64's.
- **Another shaper's output, or a tolerance.** No expected value comes from a
  run of any program, and every number is compared exactly in the case's own
  units.
- **Pixels.** A case states glyphs and positions, not a picture: the outlines
  are the face's, and a picture would put the rasterizer between the reader's
  statement and the shaper's answer.
- **A face the allowlist does not admit.** It is not vendored (ruling 18); its
  script stays listed until its licence is the owner's to admit.
- **Partial credit.** A script leaves the list when every one of its cases
  passes, never on a share of them.
- **The other rows' harnesses.** Variations, columns, Graphite and
  justification read this format through their own tests and add their own
  fields; the bar here runs scripts along a line.

### Design

#### The case file

A case file is UTF-8 text with LF line ends and the extension `.cases`. Each
line is one statement, a keyword and its value separated by the first run of
spaces; a line whose first character is `#` is a comment, and a blank line is
nothing. A **header** comes first, each of its keywords once; then one or more
**cases**, each opened by a `case` line and holding the statements after it up
to the next `case` line.

| Header keyword | Value | Required |
| --- | --- | --- |
| `format` | `1` | yes |
| `script` | the ISO 15924 code of the script the file's cases are in, as Unicode's `PropertyValueAliases.txt` gives it for `Scripts.txt`'s values: `Syrc`, `Nkoo`, `Mong` | yes |
| `face` | the face's file name, in the case file's own directory | yes |
| `author` | who wrote the file's expectations | provenance |
| `standing` | the author's standing as a reader of the script | provenance |
| `date` | the day they were written, `YYYY-MM-DD` | provenance |
| `face-source` | where the face comes from: its publisher, its release or version, and where it was fetched | provenance |
| `face-licence` | the face's SPDX identifier | provenance |
| `units-per-em` | the em the file's numbers are written in, 16 to 16 384; absent, the face's own `unitsPerEm` | no |

| Case keyword | Value | Required |
| --- | --- | --- |
| `case` | the case's name, unique among its script's cases | yes |
| `text` | the text as code points, separated by spaces: `U+0712 U+0710` | yes |
| `direction` | the paragraph's direction: `ltr`, `rtl`, or `auto` for UAX #9's P2 and P3 | yes |
| `features` | feature settings over the default plan, as css-fonts-4 §6.12's `font-feature-settings` sets them: `liga=0 ss01=1`, each tag four letters or digits and each value a decimal number, `0` off; absent, the default plan | no |
| `language` | the OpenType language system tag, padded with spaces to four characters: `SYR`; absent, the script's default language system | no |
| `glyph` | one glyph, as below; one line a glyph, in drawn order | one at least |

A **glyph** line is `glyph <id> <cluster> <x-advance> <y-advance> <x-offset>
<y-offset>`:

- `<id>` is `gid` and a decimal glyph index, or the glyph's name as the face's
  own `post` table or `CFF ` charset gives it. A name the face does not give
  refuses the case, because a name resolved any other way is a guess about
  which glyph the person meant.
- `<cluster>` is the index, counted from 0 in the case's `text`, of the first
  code point of the characters the glyph stands for; an inserted glyph takes
  the cluster of the character that produced it. A case that states no
  clusters writes `-` in every glyph line, and one that states them writes
  them in every line.
- The advances and offsets are integers in the file's units: the pen moves by
  the advance after the glyph is drawn, and the glyph is drawn at the pen plus
  its offset, `y` upward. The last glyph's two advances may each be `-` in a
  case that does not state where its line ends.

The glyphs are listed in **drawn order**: left to right, as a reader reads
them off a proof, so a right-to-left word lists its last letter's glyphs
first. That is the order text-rendering-tests writes its glyphs in, and the
one a person can check against a printed line.

**The extension rule.** A keyword this format does not define refuses the case
in every harness that does not read it, so nothing a person writes is quietly
ignored. A row whose harness needs more — FT-07a's instance coordinates,
FT-07b's column, FT-07d's Graphite language, FT-07e's width to justify —
defines its keyword in its own section, and its harness reads the rest of the
file as here.

#### What is compared

Each case is shaped as a consumer shapes a line: `Shaper::shape_text` over its
text and direction, with its feature settings through `Shaper::with_settings`
and its language through `Shaper::with_language`; the runs are put in drawn
order by UAX #9's rule L2 over their levels (`bidi::reorder`), and a
right-to-left run's glyphs are walked from its end. The pen starts at the
origin, and for each glyph in drawn order:

1. **the glyph** is the one the line states;
2. **its cluster**, where the case states clusters, is the code point index of
   the glyph's byte cluster;
3. **where it is drawn** — the pen plus its offset, on each axis — is where the
   line's own advances and offsets draw it.

And where the case states it, **the pen's end** — the line's advance on each
axis — is where the line's advances leave it. A position is compared rather
than the advance and offset it was written as, because two ways of reaching one
point draw one line: a reader sees where a mark stands, and whether the pen's
share of it is the mark's advance or the base's is a convention of a shaper,
which no reader can see and none should have to know.

In the face's own units every number is the shaper's integer, compared
exactly. A file written at another `units-per-em` has each of the shaper's
positions scaled once, `units × N ÷ unitsPerEm` rounded half away from zero —
the arithmetic `tests/text_rendering.rs` scales by — and compared exactly with
the sum the file's own numbers give.

A case that does not reproduce fails with its first difference and a verdict:
the glyph **set** differs, their **order** differs, a **cluster** differs, or
only a **position** does — `text_rendering.rs`'s three, and the cluster.

#### Provenance, and what refuses a case

Every case carries its file's provenance. A file without one of `author`,
`standing`, `date`, `face-source` or `face-licence` refuses every case in it,
naming the field. A case is refused, each by name, as well for: a keyword the
format does not define; a header keyword after the first case or given twice;
a `format` other than `1`; a `date` that is no calendar day; a `face-licence`
with a character SPDX's identifier syntax does not allow; a `script` that is no
four-letter code; a character of the text in another script, `Common` and
`Inherited` aside, or a text with none in its own; a code point that is no
Unicode scalar value; a case without a `text`, a `direction` or a glyph; a
glyph name the face does not give, or an index past its glyphs; clusters stated
for some glyphs and not others; a `-` as any advance but the last glyph's; a
`units-per-em` outside its range; a malformed feature setting or language tag;
a case name its script has twice; and a face that is not in the directory or
that `Sfnt::parse` declines. A refusal fails the bar: it is a delivery to
mend, never a case to pass over.

#### Where cases and faces live

`crates/tinker-pdf-shape/data/person-written/` is one vendored tree: each
delivered face, whole and unmodified, its licence beside it as
`LICENSE-<face>`, and the `.cases` files that name it. `THIRDPARTY.md`
declares the tree with its faces' SPDX identifier, as it declares
`data/graphite`'s three faces, so `cargo xtask vendor` holds the licence to
`deny.toml`'s allowlist; the cases are this repository's own, contributed under
its licence as a test is. A face under an admitted licence other than the
tree's goes in a tree of its own beside it, `data/person-written-<name>`,
declared the same way, which the bar reads too. `.gitattributes` already gives
`crates/tinker-pdf-shape/data/**` `-text linguist-vendored`, so a delivered
face needs no attribute line.

The tree comes into being with H-64's first delivery: git keeps no empty
directory, and `cargo xtask vendor` requires every tree under a crate's `data/`
to be declared with its licence, which a tree with no face cannot be. The bar
reads a missing tree as nothing delivered, and its ratchet — which counts each
script's cases — is what notices a tree that went missing.

#### The bar (FT-09.2)

`tests/person_written.rs` reads every `.cases` file in the trees, refuses as
above, shapes and compares each case, and holds `PERSON_WRITTEN` — per script,
its cases and how many reproduce — as `text_rendering.rs` holds `PASSING`: a
ratchet whose rows the run must equal, so a case delivered, lost or newly
failing changes the table in the commit that does it, and a count that falls
is a visible edit. It prints one `RAN` line naming each script and its count,
and says so when no script has a case. It runs as `cargo test -p
tinker-pdf-shape --test person_written`; the crate declares no
`bundled-fonts`, so there is one build.

#### Leaving the list (FT-09.3)

A script leaves `fonts.md`'s shaped-and-unverified row, and H-64's list, when
every one of its cases reproduces, each states its clusters and every advance
— a person's case is whole — and its face is vendored as above. The row names
each script with its ISO 15924 code, which is what FT-09.3's test reads. A
script with a failing case stays listed, and each failing case goes in a triage
table with its verdict; the failure is a finding for the shaper's own rows
(FT-08's cluster models among them), never a reason to edit the case.

#### Syriac's Alaph (FT-09.4)

§9.3.2 makes U+0710 SYRIAC LETTER ALAPH right-joining with `Joining_Group`
`Alaph`, and gives its word-final forms three rules of their own, read with the
Standard's visual terms — *to its right* is the character before it in logical
order, *to its left* the one after — and characters of `Joining_Type`
`Transparent` skipped on both sides, so the Syriac counterpart of the Arabic
rule R1 takes precedence:

- **S1**: a left-joining character to its right, and a non-joining character
  or the end of the text to its left: **Afj**.
- **S2**: a character to its right that does not join to its left, other than
  one of `Joining_Group` `Dalath_Rish`, and a non-joining character or the end
  of the text to its left: **Afn**.
- **S3**: a character of `Joining_Group` `Dalath_Rish` to its right, and a
  non-joining character or the end of the text to its left: **Afx**.

Table 9-15 names the three final glyph types; Table 9-19 draws Alaph's five
forms, the two of a letter that is not final among them, **Xn** and **Xr**;
Table 9-18 lists the right-joining letters with their groups, `Dalath_Rish`
among them. Microsoft's Syriac specification names the features: `fin2` for
Alaph at the end of a word "when the preceding base character cannot be joined
to" and is no Dalath, Rish or dotless Dalath-Rish, `fin3` after one of those,
and `med2` for Alaph "in the middle of Syriac words when the preceding base
character can be joined to"; its *Features* section keeps Alaph out of `medi`.
So, with the start of the text read as a character that does not join, as a
space before Alaph is:

| Alaph | Form | Feature |
| --- | --- | --- |
| last letter of its word, joined to the letter before it (S1) | Afj | `fina` |
| last letter, after a letter that does not join to it and is no `Dalath_Rish` (S2) | Afn | `fin2` |
| last letter, after a letter of `Joining_Group` `Dalath_Rish` (S3) | Afx | `fin3` |
| followed by a letter of its word, joined to the letter before it | Xr | `med2` |
| followed by a letter of its word, not joined | Xn | `isol` |

Alaph's last-letter test is S1 to S3's *non-joining character or end of the
text to its left*: the next character in logical order that is not
`Transparent` is of `Joining_Type` `Non_Joining`, or there is none. Its join to
the letter before is `crate::arabic::forms`'s answer, unchanged; the table
above only renames the form an Alaph takes. The stages are Microsoft's
*Shaping features* order — `isol`, `fina`, `fin2`, `fin3`, `medi`, `med2`,
`init` — so FT-09.4 puts `fin2` and `fin3` after `fina` and `med2` after `medi`
in `JOINING_GSUB_STAGES`, each its own stage, and gives Alaph's glyph the mask
of its one form. `Joining_Group` is field 3 of `ArabicShaping.txt`, the file
FT-07e.4 reads for kashida's groups; FT-09.4 vendors it at the pinned 17.0.0
wherever it is not yet in the tree and compiles the property in `build.rs`. A
run with no Alaph shapes exactly as before.

#### The topographical features under the Universal Shaping Engine (FT-09.5)

The engine's stages are character classification, split vowels, cluster
validation, *OpenType feature application I* (basic cluster formation), glyph
reordering, and *OpenType feature application II*, whose first group is the
**topographical features**, `isol`, `init`, `medi` and `fina`, applied "as
required features on a per cluster basis in order to invoke a particular form
based on non-joining or white space boundaries", the boundaries being where a
character's `Joining_Type` does not join on the side a join would take, and
`ZWJ` and `ZWNJ` making or breaking one. The standard typographic features
(`abvs`, `blws`, `haln`, `pres`, `psts` and the rest) follow, and positioning
after them.

So a run that both joins (`arabic::joins`) and holds a character the engine
categorises keeps its syllables, as `Plan::Universal` cuts them, and takes one
form a character from `arabic::forms` over the whole run, because a join
crosses clusters. The four features are one stage each, in the order they are
listed, after `USE_REORDER_AFTER`'s pause and before the presentation group,
each lookup masked per glyph by its form and matched within its cluster as the
engine's other per-cluster features are. A run with no joining character, and
every run of a script the engine does not shape — Arabic, Syriac — keeps the
plan it has today.

### Milestones

1. **FT-09.1, this section.** Its exit is the section.
2. **FT-09.2, the bar.** `tests/person_written.rs`:
   `every_person_written_case_reproduces` runs and prints its `RAN` line with
   nothing delivered; `a_case_without_its_provenance_is_refused`;
   `the_harness_compares_what_it_claims` writes `SHARAN-1/1` into the format
   inside the test, passes it, and fails it with one glyph or one offset
   changed. `features/fonts.md`'s table of what adjudicates each script gains
   the bar, empty.
3. **FT-09.3, a delivery**, one run of the step per script H-64 delivers:
   `every_person_written_case_reproduces` passes with the script;
   `the_unverified_list_names_every_script_without_a_whole_fixture` reads
   `features/fonts.md`'s row and fails while a script whose cases all pass is
   listed, or one whose cases do not is missing; `cargo xtask vendor` and
   `cargo xtask check` pass with the face declared.
4. **FT-09.4, Alaph.** `arabic.rs`'s
   `alaph_takes_the_form_its_neighbours_give_it` holds each row of the table
   above against the rule worked in the test; H-64's Syriac cases pass in
   `person_written.rs`; `ucd_version.rs`'s
   `each_tree_holds_the_files_its_algorithms_need` names `ArabicShaping.txt`
   at 17.0.0; `text_rendering.rs`'s `PASSING` and `aots.rs` unchanged;
   `fingerprints.rs` gains a Syriac run.
5. **FT-09.5, the topographical stage.** `shape.rs`'s
   `a_joining_brahmic_run_keeps_its_syllables_and_its_forms` holds syllable
   numbers and form masks on a run worked by hand; H-64's cases for the script
   pass in `person_written.rs`; `text_rendering.rs`'s `PASSING` and `TRIAGE`
   and every Arabic-script case unchanged; `fingerprints.rs` gains the run.

### Risks

| Risk | Mitigation |
| --- | --- |
| A person's numbers are wrong, and a shaper that matches them is wrong with them | The standing and the face's source travel with every case, so a case is answerable to a person; a failing case is triaged before anyone decides which side is wrong, and the shaper is changed only on the specification's text, never to match a number |
| A case is written against one release of a face and run against another | The face is vendored whole and unmodified, its release named in `face-source` and in `THIRDPARTY.md`; another release is another delivery, whose cases run against it in the commit that brings it |
| A position written as advance and offset in a split this crate does not make fails a case that draws right | What is drawn is compared, not the split, so any split that draws the line passes |
| A field the harness does not read is ignored, and a case passes that says more than was checked | A keyword the harness does not read refuses the case |
| A delivered tree is deleted and the bar reads it as nothing delivered | `PERSON_WRITTEN` counts each script's cases, so a lost tree fails the ratchet |
| The bar compares less than it says | `the_harness_compares_what_it_claims` passes a published case and fails it with one glyph and with one offset changed |
| The Standard's and Microsoft's descriptions of Alaph read apart — Microsoft's `med2` example puts Alaph after a Waw, which does not join to it | The forms are read from §9.3.2's rules and Table 9-19, the features from Microsoft's definitions; H-64's Syriac cases decide, and FT-09.4 lands only with them |
| A face's licence is not on the allowlist | It is not vendored and its script stays listed (ruling 18) |
