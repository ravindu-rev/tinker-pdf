# Table reconstruction from geometry

When this is done, a caller who opts in gets, for a page, the tables this
engine *inferred* from what the page draws — cells with a row, a column, a
span, their text and their quads — reconstructed from the rules and fills the
recording device keeps beside the glyphs, and from the alignment of lines
where no rule was drawn. The answer is a type of its own, `TableSource::Inferred`,
never the default and never a structure element, so that a grid this engine
guessed is not mistaken for one the producer tagged. The roadmap row says
"opt-in, same discipline, held to the `/Table` elements the corpus carries",
and this document measures those elements first, because they are the
adjudicator.

## Scope

- **`Page::tables(TableSource)`** on the facade. `Stated` walks the
  structure tree's `Table`, `TR`, `TH` and `TD` elements and gives each cell
  the characters `StructureTree::text_for_page` already claims for it, with
  the enclosing quad — a reader of what the producer said, and the half of
  this design that needs no inference. `Inferred` returns `InferredTable`s.
- **`InferredTable`**: rows and columns as counts, cells as `{ row, col,
  row_span, col_span, chars, quad, header: HeaderEvidence }`, the table's
  bounds, the **evidence** it was built on (`Ruled`, `Aligned`), the
  permutation of its characters from stream order, and `TableWarning`s.
- **Ruled tables first**: rules from stroked lines and thin filled
  rectangles, snapped to a lattice, cells as the rectangles the lattice
  bounds, spans where an interior rule is missing, text assigned by quad.
- **Aligned tables second**, labelled `Aligned`: columns from left edges
  that repeat across at least three lines with a consistent gap, when no rule
  was drawn.
- **Header evidence**, never a header: `HeaderEvidence::None`, `FillBeneath`,
  `RuleBeneath`, `FirstRow` — what the page shows, not what a `TH` means.
- **The measurement**: `table_census.rs` over every tagged file that carries
  a `Table` element, inferring with the tree hidden and scoring grid and
  cell assignment against it, ratcheted in `corpus/ratchet.json`.
- **Surfacing**: `tpdf text --tables`.

## Non-goals

- **Tables in images.** A scanned table is pixels; OCR is a host seam the
  roadmap keeps as a decision, and nothing here reads a raster.
- **Tables across pages.** A table continued on the next page is two tables
  here, each named `TableWarning::MayContinue` when its last rule is the
  page's bottom margin. Joining them is a document-level question the sibling
  design's cross-page pass would own, and it is not designed here.
- **Nested tables.** A lattice inside a cell is refused by name
  (`TableWarning::NestedLattice`) and the outer table is returned. Real
  documents have them; the first delivery does not guess at them.
- **Writing a `/Table`.** No inferred grid enters a structure tree, a written
  document, or a PDF/UA verdict — the same line
  [design/reading-order.md](reading-order.md) draws for orders.
- **Semantics.** `Scope`, `/Headers`, a caption's relation to its table, a
  merged cell's meaning: a producer states them or nobody does.
- **A probability.** Evidence is named (`Ruled`, `Aligned`) and warnings are
  typed; no score pretends to a calibration nobody has.
- **Serialisation.** CSV and JSON are the structured-text-serialisation
  row's; `InferredTable` is shaped so that row can write it.

## What the corpus carries, measured

A scratch walk over `corpus/files` on 16 September 2026 — 5 605 PDFs, 5 597
opened, no passwords supplied, qpdf's whole tree rather than its `subdir` —
bound every structure tree this engine could read and counted the table
family by `standard_type` after the role map:

| Corpus | files with a `Table` | `Table` | `TR` | `TH` | `TD` | files with a `TH` |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| SafeDocs | 145 | 1 836 | 14 880 | 3 052 | 73 698 | 44 |
| veraPDF | 49 | 49 | 175 | 204 | 274 | 49 |
| pdfjs | 13 | 23 | 180 | 29 | 502 | 5 |
| qpdf, pdfa-examples | 0 | 0 | 0 | 0 | 0 | 0 |
| **total** | **207** | **1 908** | **15 235** | **3 285** | **74 474** | **98** |

Three things in that table decide the design.

**The population is SafeDocs.** 145 of the 207 files and 1 836 of the 1 908
tables are documents a crawler found on the open web, tagged by whatever
produced them. That is the adjudicator: a third party's statement of where
the rows and cells are, at a scale no fixture set reaches, with every
mistake a real producer makes — layout tables, tables tagged in the wrong
order, a `TD` with no text — included. The veraPDF 49 are one table each,
under PDF/UA's table clauses (7.5, the 7.2 table-model fixtures, 8.2.5.26),
fully and deliberately tagged; they are the false-positive set, because a
table inferred there that the fixture did not tag is wrong on a page whose
author tagged everything.

**Headers are rare.** 98 files carry a `TH` at all, 44 of them in SafeDocs.
So "did the inference find the header row" has ground truth on a fifth of
the population and `HeaderEvidence` is named evidence rather than a verdict
for that reason.

**Spans have eight fixtures.** The PDF/UA files that state `RowSpan` and
`ColSpan` in their own outlines — `7.2-t15-pass-a`, `7.2-t41-fail-a`,
`7.2-t42-fail-a`, `7.2-t43-fail-a`, `8.2.5.26-t01-pass-a`, `-t03-fail-a`,
`-t03-fail-b`, `-t04-fail-a` — are the only files whose spans are stated in
words; SafeDocs' spans are in `/A` attributes this engine does not read yet
(below). Two of the eight are annotated `pass` and six `fail`, the six
because their spans are *inconsistent* with the grid, which makes them the
fixtures for the inference refusing to build a lattice from a producer's
arithmetic.

**Nothing here reaches the structure reader's attributes.**
`crates/tinker-pdf/src/structure.rs` reads no `/A` dictionary: `/RowSpan`,
`/ColSpan`, `/Headers`, `/Scope` are not fields of `StructElement`, and a
search for either key in that file finds nothing. `Stated` tables therefore
start with a grid whose spans are unknown, and milestone 1 adds the reader —
the same one [design/pdfua.md](pdfua.md) needs for table regularity and
header association, built once.

## Design

### The label is a type, and a wrong table is the caller's to undo

Everything [design/reading-order.md](reading-order.md) says about
`ReadingOrder::Inferred` holds here with the noun changed. `TableSource::
Inferred` is a different request from `Stated`; an `InferredTable` names its
evidence and its warnings, carries the permutation from stream order for
every character it placed, and leaves `TextPage` untouched. A table
reconstructed where none was — the failure that matters, because a paragraph
of aligned numbers reads like a grid — is a table the caller asked for, can
see the evidence of, and can discard without losing a character. `Inferred`
on a page whose structure tree has a `Table` returns the stated one with
`TableWarning::TreePresent` and infers nothing.

### What the recording device gives this design, and what it does not

`record.rs` names table reconstruction as the consumer that "wants the rules
and fills a page draws beside the glyphs, which no text-only device keeps".
Precisely:

- **`FillPath` and `StrokePath` events with the path in device space** as
  `Vec<PathSegment>`, and `ClipPath` likewise. A ruled table is stroked lines
  or filled rectangles a fraction of a point tall; both arrive as segments.
- **The graphics state at the call**, when `Capture::state` is on: the line
  width that tells a 0.5 pt rule from a 12 pt bar, and the fill colour that
  tells a cell's shading from a rule. This design needs it, so its capture is
  `Capture { text: true, paths: true, images: false, structure: true, state:
  true }` — the expensive one, boxed per event, and `event_stays_small` pins
  what that costs.
- **`SaveState`, `RestoreState` and `ClipPath` in order.** The recorder
  keeps no accumulated clip — "nothing is derived" — so a rule drawn outside
  the current clip is in the transcript and not on the page. The consumer
  accumulates rectangular clips from those three events and refuses a
  non-rectangular one (`TableWarning::ClipNotRectangular`) rather than
  reading a rule through it.
- **Glyphs**, for the same reason the reading-order design wants them: a
  `Glyph::font_id` per event tells a bold header row from a body row, which
  `TextChar` cannot.

What it does not give: **bounding boxes** (computed from segments; a path
with a curve is not a rule and is not one), **classification** (a rule is the
consumer's threshold — axis-aligned to within 0.5 pt over its length, at
least two ems long, at most 2 pt thick or a filled rectangle at most 2 pt in
one dimension), **line assembly** (`TextDevice`'s, as ever), and **a
replay** — the second interpretation per page until the retained-page row
lands, exactly as the reading-order design records.

### The lattice

1. **Collect rules.** Every `StrokePath` segment and every `FillPath` that
   is a rectangle, filtered by the thresholds above, clipped by the
   accumulated rectangular clip, snapped to a grid of 0.5 pt. Bounded:
   `MAX_RULES` rules per page with `TableWarning::TooManyRules`, because
   junction-finding is quadratic and a page of hatching is a denial of
   service with a table's name.
2. **Find junctions.** Every crossing or meeting of a horizontal and a
   vertical rule. A candidate table is a maximal set of rules whose junctions
   form at least a 2×2 lattice; anything smaller is a rule, not a table.
3. **Cells.** The minimal rectangles the lattice bounds. A cell whose
   interior contains no junction but whose boundary skips a lattice line is a
   span, and its `row_span` and `col_span` are the lines skipped plus one.
4. **Assign text.** Each `TextLine` from the same `TextPage` goes to the
   cell containing the centre of its quad; a line whose quad crosses a rule
   is split at the rule when its characters fall cleanly on either side and
   named `TableWarning::TextCrossesRule` when they do not.
5. **Order.** Rows top to bottom, cells left to right, or right to left when
   the majority of the table's lines carry `TextLine::rtl`. The permutation
   from stream order is recorded per character.
6. **Header evidence.** `FillBeneath` when the first row's cells were filled
   and the second's were not; `RuleBeneath` when the rule under the first
   row is thicker than the others or doubled; `FirstRow` otherwise, which is
   no evidence and says so.

**Aligned tables** repeat steps 3 to 6 over a lattice built from text
instead of ink: columns are left edges (right edges for `rtl`) that recur on
at least three lines within 0.25 em, rows are the lines' baselines, and the
result is labelled `Aligned` with `TableWarning::NoRules`. Every threshold
is in ems of the page's median line size and lives in one place.

## The adjudicator, and the scores

**Stated against inferred, with the tree hidden.** `table_census.rs` runs
over every tagged file with a `Table` — the 207 above, re-derived by the test
rather than carried from this document — and for each page with a stated
table:

1. **Found.** Whether an inferred table's bounds overlap the stated table's
   claimed-character quad by at least half. Recall over stated tables is a
   ratcheted floor per corpus.
2. **Grid agreement.** Whether the inferred row and column counts equal the
   stated ones, exact. A floor per corpus, and printed by evidence class, so
   that `Ruled` and `Aligned` are never averaged together.
3. **Cell assignment.** Of the characters the tree claims for cells of that
   table, the fraction the inference placed in the same `(row, col)`. A
   floor per corpus.
4. **Extra tables.** Inferred tables on pages where the tree claims none.
   Printed for SafeDocs and pdfjs, because producers under-tag and an
   untagged real table is not a false positive; **asserted zero over the
   veraPDF table fixtures**, where everything is tagged and an extra table is
   a mistake.
5. **Spans.** Over the eight span fixtures, the stated spans reproduced on
   the two `pass` files, and on the six `fail` files a
   `TableWarning::SpanInconsistent` rather than a lattice built to the
   producer's arithmetic.

The scores are the same shape as the metamorphic relations in
`corpus/ratchet.json`: compared by integer cross-multiplication, `compared`
recorded beside `held`, a relation that declines the hard pages not counted
as one that held.

**The first-party construction that is admissible.** This engine's EPUB
pipeline lays out real producers' XHTML tables through CSS 2.2 §17's model
and writes `Table`, `TR`, `TH` and `TD` elements for them —
`epub_structure.rs` asserts all four against the source markup. The XHTML
grid is the author's statement of the table, made by a third party, and a
book's table laid out here, then reconstructed here from the page's rules,
must recover that grid exactly. It is an arithmetic fixture with a known
answer, and its limit is the one the reading-order design states: the layout
engine that drew the rules and the inference that reads them share an author.

**What none of this measures.** Whether a producer's `Table` is a table. A
layout grid used for positioning is tagged `Table` by some producers and is
scored here as one; a real table tagged as paragraphs is invisible to the
census and counted as an extra. The extra count is printed for that reason,
and the ratchet floors are over what producers said, which is the honest
sentence.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `TableSource::Stated`: the tree walk, the attribute reader (`/A` with `/RowSpan`, `/ColSpan`, `/Headers`, `/Scope`, plus `/ID`) in `structure.rs`, cell quads from the claimed-character join; `table_census.rs` counting stated tables | The census prints `RAN` and counts at least the 207 files, 1 908 tables and 74 474 `TD`s recorded here, per corpus; the eight span fixtures read their spans as their outlines state them; `tagged.elements` in the ratchet unchanged; `hostile_input.rs` covers the attribute walk with zero panics | M |
| 2 | Rule extraction over `RecordingDevice` with rectangular clip accumulation | A builder-drawn 3×4 ruled grid yields twelve rules at the coordinates the test computes; a rule under a non-rectangular clip yields `ClipNotRectangular` and no rule; a page past `MAX_RULES` yields `TooManyRules`; counted injections with the zeros printed | M |
| 3 | Ruled-table reconstruction: junctions, lattice, cells, text assignment, order | Recall, grid agreement and cell assignment over SafeDocs' and pdfjs' ruled tables at recorded floors; **zero extra tables over the veraPDF table fixtures**; the EPUB table reftest recovers the XHTML grid exactly; `plain_text()` byte-identical over the fingerprint suite | M |
| 4 | Spans, header evidence, `rtl` ordering | The two `pass` span fixtures reproduced and the six `fail` ones refused with `SpanInconsistent`; a header row with a fill beneath yields `FillBeneath` on a builder fixture and `FirstRow` on its unfilled twin; an Arabic EPUB table orders cells right to left | S |
| 5 | Aligned tables, labelled | Recall rises over the tagged population by a recorded margin with `Aligned` scored separately from `Ruled`; the extra count printed; a paragraph of prose yields no table (the false positive that matters, as a fixture) | M |
| 6 | The surface: `tpdf text --tables`, the reading-order handoff (`TableSuspected` there, `TreePresent` here), the feature doc's row | Every `TableWarning` variant reached by a fixture; the roadmap row deleted | S |

## Dependencies

- **`RecordingDevice`** — exists; this consumer is the one that pays for
  `state: true`.
- **A replay of a transcript into a device** — does not exist (the
  retained-page row); two interpretations per page until it does.
- **`structure.rs`** — exists; **reads no attributes**, and milestone 1 is
  where the reader lands for both this design and
  [design/pdfua.md](pdfua.md).
- **`StructureTree::text_for_page`** — the claimed-character join. Exists.
- **`TextDevice`** — the line assembler. Exists; unchanged.
- **The ratchet machinery** — a table axis with `compared` beside `held`.
  Exists; grows a bar.
- **The EPUB table pipeline** (`crates/tinker-pdf-layout`'s §17 model,
  `epub_tables.rs`, `epub_structure.rs`) — exists; the reftest is new.
- **[design/reading-order.md](reading-order.md)** — the two designs share
  the recorder, the second-interpretation cost, the label discipline and the
  handoff.

## Risks

| Risk | Mitigation |
| --- | --- |
| **A wrong table looks like a table.** Aligned numbers in a paragraph, a form's boxes, a page's decorative rules | Opt-in and labelled; the veraPDF fixtures assert zero extras; a prose page is a fixture; the extra count over real documents is printed every run and read, not averaged |
| The adjudicator is a producer's tag, and producers tag layout grids as tables and real tables as nothing | Named. Recall and agreement are over what producers said; the extra count is printed rather than asserted on real documents; the EPUB reftest carries an author's grid that is not a tagger's |
| `state: true` makes the transcript large on a dense page | Bounded by the interpreter's limits and by `MAX_RULES`; the cost is measured in the census; the replay row removes the second interpretation |
| Junction finding is quadratic in rules | `MAX_RULES` with a typed refusal, sized from the census's largest ruled page and recorded in `bounds_ledger.rs` like every other cap |
| Spans built from a producer's inconsistent arithmetic corrupt the grid | The six `fail` fixtures refuse rather than repair, and `Stated` reports what the file said with a warning rather than what a repair would want |
| Two designs share the attribute reader and drift | One reader, in `structure.rs`, landed by whichever design schedules first; the other's milestone 1 shrinks to nothing and says so |

## As built

*Filled in as milestones land.* Nothing has landed; the roadmap row is not
scheduled. The census numbers above were produced by a scratch program that
is not in the tree, which is why milestone 1's first exit criterion is a test
that re-derives them.
