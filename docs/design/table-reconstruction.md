# Table reconstruction from geometry

A caller who opts in gets, for a page, the tables this engine *inferred*
from what the page draws — cells with a row, a column, a span, their text and
their quads — reconstructed from the rules and fills the page draws beside
the glyphs, and from the alignment of lines where no rule was drawn. The
answer is a type of its own, `TableSource::Inferred`, never the default and
never a structure element, so that a grid this engine guessed is not mistaken
for one the producer tagged. The tables a producer stated are read as well,
and the `/Table` elements the corpus carries are the adjudicator
([ROADMAP](../ROADMAP.md) row FT-31).

## Scope

- **`Page::tables(TableSource)`** on the facade. `Stated` walks the
  structure tree's `Table`, `TR`, `TH` and `TD` elements and gives each cell
  the characters `StructureTree::text_for_page` claims for it, with the
  enclosing quad — a reader of what the producer said, and the half of this
  design that needs no inference. `Inferred` returns `InferredTable`s.
- **`InferredTable`**: rows and columns as counts, cells as `{ row, col,
  row_span, col_span, chars, quad, header: HeaderEvidence }`, the table's
  bounds, the **evidence** it was built on (`Ruled`, `Aligned`), the
  permutation of its characters from stream order, and `TableWarning`s.
- **Ruled tables first**: rules from stroked lines and thin filled
  rectangles, merged into a lattice, cells as the rectangles the lattice
  bounds, spans where an interior rule is missing, text assigned by quad.
- **Aligned tables second**, labelled `Aligned`: columns from left edges
  that repeat across at least three lines, when no rule was drawn.
- **Header evidence**, never a header: `HeaderEvidence::None`, `FillBeneath`,
  `RuleBeneath`, `FirstRow` — what the page shows, not what a `TH` means.
- **The measurement**: `table_census.rs` over every tagged file that carries
  a `Table` element, inferring with the tree hidden and scoring found, grid
  and cell assignment against it.
- **Surfacing**: `tpdf text --tables`.

## Non-goals

- **Tables in images.** A scanned table is pixels, and nothing here reads a
  raster. Tables in images are [ROADMAP](../ROADMAP.md) row OC-15a.
- **Tables across pages.** A table continued on the next page is two tables
  here, each named `TableWarning::MayContinue` when its last rule lies in the
  page's foot band. Joining them is [ROADMAP](../ROADMAP.md) row FT-32.
- **Nested tables.** A lattice inside another's frame is refused by name
  (`TableWarning::NestedLattice`) and the outer table is returned.
  [ROADMAP](../ROADMAP.md) row FT-32.
- **Writing a `/Table`.** No inferred grid enters a structure tree, a written
  document, or a PDF/UA verdict — the same line
  [design/reading-order.md](reading-order.md) draws for orders. Auto-tagging
  a new document is [ROADMAP](../ROADMAP.md) row SD-07; no inferred grid ever
  reaches a verdict about its input.
- **Semantics.** `Scope`, `/Headers`, a caption's relation to its table, a
  merged cell's meaning: a producer states them or nobody does.
- **A probability.** Evidence is named (`Ruled`, `Aligned`) and warnings are
  typed; no score pretends to a calibration nobody has.
- **Serialisation.** CSV and JSON are [ROADMAP](../ROADMAP.md) row FT-32's;
  `InferredTable` is shaped so that row can write it.

## What the corpus carries

**The population is SafeDocs**: documents a crawler found on the open web,
tagged by whatever produced them. That is the adjudicator: a third party's
statement of where the rows and cells are, at a scale no fixture set
reaches, with every mistake a real producer makes — layout tables, tables
tagged in the wrong order, a `TD` with no text — included. The veraPDF table
files are one table each, under PDF/UA's table clauses (7.5, the 7.2
table-model fixtures, 8.2.5.26), fully and deliberately tagged; they are the
false-positive set, because a table inferred there that the fixture did not
tag is wrong on a page whose author tagged everything.

**Headers are rare.** Few files carry a `TH` at all, so "did the inference
find the header row" has ground truth on a small part of the population, and
`HeaderEvidence` is named evidence rather than a verdict for that reason.

**Spans are stated in words in eight fixtures.** The PDF/UA files that state
`RowSpan` and `ColSpan` in their own outlines — `7.2-t15-pass-a`,
`7.2-t41-fail-a`, `7.2-t42-fail-a`, `7.2-t43-fail-a`, `8.2.5.26-t01-pass-a`,
`-t03-fail-a`, `-t03-fail-b`, `-t04-fail-a` — are the files whose spans are
stated in words. Two of the eight are annotated `pass` and six `fail`, the
six because their spans are *inconsistent* with the grid, which makes them
the fixtures for the inference refusing to build a lattice from a producer's
arithmetic.

## Design

### The label is a type, and a wrong table is the caller's to undo

Everything [design/reading-order.md](reading-order.md) says about
`ReadingOrder::Inferred` holds here with the noun changed. `TableSource::
Inferred` is a different request from `Stated`; an `InferredTable` names its
evidence and its warnings, carries the permutation from stream order for
every character it placed, and leaves `TextPage` untouched. A table
reconstructed where none was — the failure that matters, because a paragraph
of aligned numbers reads like a grid — is a table the caller asked for, can
see the evidence of, and can discard without losing a character.
`Page::tables(TableSource::Inferred)` on a page whose structure tree states a
table answers with the stated one, and `Page::inferred_tables` there infers
nothing and says so (`TableWarning::TreePresent`) unless
`TableOptions::hide_structure` is set.

### Stated tables

`Page::stated_tables` (`crates/tinker-pdf/src/tables.rs`) finds every
`Table` element, its `TR` rows through `THead`, `TBody`, `TFoot` and any other
grouping element, and each row's `TH` and `TD` cells, never walking into a
`Table` inside a cell; each cell's characters are the ones the structure join
claims for its subtree over the **same** `TextPage`
(`StructureTree::element_runs`: the page's sequences grouped once a page,
every table's cells joined in one call). `StructElement::table` reads `/A`'s
`/Table`-owned `/Headers`, `/Scope`, `/Summary`, `/RowSpan` and `/ColSpan`,
and `StructElement::id` reads `/ID` — the reader [design/pdfua.md](pdfua.md)
uses too. Cells are placed as a table model places them — each in its row's
first column no span above still holds — and what the file's arithmetic does
not allow is named rather than repaired: `TableWarning::SpanInconsistent` for
a cell over a held slot, a row span past the last row, or a column span past
the most columns a table of that many cells can have (the clamp that keeps a
`/ColSpan` of four billion from being an allocation; each span is compared
with what is left of the table, never added to where it starts, which could
overflow on a 32-bit target), and `TableWarning::RaggedRows` for a row whose
width is not the table's. Placement asks a segment tree over the columns
where the next free one is, so a table whose every cell spans every row is a
logarithm per cell and not a walk.

### Rules, through the one interpretation

The tee device the reading-order inference reads text through
(`crates/tinker-pdf/src/observe.rs`) keeps the ink beside the glyphs: every
straight stroked segment axis-aligned to within 0.5 pt over its length and at
most 2 pt wide, every filled rectangle at most 2 pt in one dimension, as a
`TableRule` (`horizontal`, `at`, `from`, `to`, `width`) in default user
space; curves, diagonals, bars and specks are not rules, and a filled
rectangle that is not a rule is a fill, which is what a shaded header row is.
It accumulates the clip itself — `q`, `Q` and a form's bracket save and
restore it, each `W` intersects it while it stays a rectangle — cuts a rule
to it, and refuses and counts one drawn under a clip that is not a rectangle
(`TableWarning::ClipNotRectangular`). A rule in a hidden layer is not on the
page and is not kept. `MAX_TABLE_RULES` (16 384, a `bounds_ledger.rs` row)
bounds the page's rules and its fills: past it none is read and
`TableWarning::TooManyRules` (`TooManyFills`) says how many were drawn.
`Page::table_rules` exposes the rules as the evidence an inferred table is
built on. Sizing the cap from the corpus's largest ruled page is
[ROADMAP](../ROADMAP.md) row FT-31.

### The lattice

`Page::inferred_tables` (opt-in, `TableEvidence::Ruled`, never a structure
element) merges rules along one axis within `LATTICE_MERGE_EMS` (half an em)
into one lattice line — the halves of a collapsed CSS border, or the two
borders of cells with the default 2 px spacing between them — and collinear
pieces whose gaps are no wider into one segment; joins lines that reach each
other to within the same distance; and reads each maximal joined set as a
lattice whose lines bound the cells. A rule is at least `RULE_MIN_EMS` (one
em) long, because a one-line row with CSS's ordinary padding is about 1.75 em
tall, and a lattice is at least two cells, since one cell is a framed
paragraph. Each character goes to the cell holding its box's centre, so a
text-device line across two cells is split at the rule, and a glyph a rule
runs through the middle of is counted (`TextCrossesRule`). Rows read top to
bottom, cells left to right, a cell's lines top first; the permutation gives
every character's stream position.

Refused rather than guessed: a lattice of one cell (a box), one with text in
fewer than two cells or fewer than one in `LATTICE_TEXT_SHARE` (four) — a
form, or hatching, checked before any cell is made, so a lattice of forty
thousand cells around one word costs nothing — and a lattice inside
another's frame, in one of its cells or in one merged from several
(`NestedLattice` on the outer); two whose frames overlap with neither inside
the other are both refused (`LatticesCross`), so no character is in two
tables, and each lattice reads only the characters whose centres fall in its
frame, by binary search over two sorted copies. A table whose last rule lies
in the foot band is `MayContinue`.

**Spans and header evidence.** A grid cell whose boundary with a neighbour
no lattice segment rules — a segment at that line covering at least half the
boundary, within the lattice's reach — is one cell with it, and the shape the
missing rules leave is a span when it is a rectangle; when it is not (an L),
it stays grid cells and `TableWarning::SpanNotRectangular` says where, a
typed name of its own because `SpanInconsistent` is about a producer's stated
arithmetic, not about ink. `InferredTable::header` is `HeaderEvidence`:
`FillBeneath` when every first-row cell is shaded and no second-row cell is
(the observer keeps filled rectangles that are not rules, with whether they
inked anything); `RuleBeneath` when the rule under the first row is half
again as heavy as the table's other interior rules, or doubled where they are
single; `FirstRow` otherwise, which is no evidence and says so; `None` for
one row. Columns are counted, and cells read, right to left when most of the
table's characters are on right-to-left lines.

### Aligned tables

Where no ruled table stands, each text-device line is cut into fragments at
every gap of an em or more between glyphs that are not spaces
(`ALIGNED_GAP_EMS`; a word space is a quarter to a third of one, and a run of
space glyphs a producer pads columns with is the gap, not ink across it),
fragments sharing a baseline to within half an em are a row, and a run of at
least three consecutive rows (`ALIGNED_REPEATS`), each of two fragments or
more and no more than two lines apart, is a candidate. Its columns are the
left edges — right edges when most of its text is right to left — that three
rows share to within a quarter em (`ALIGNED_EDGE_EMS`); each fragment belongs
to the last column starting at or before it, which is what lets a header row
centred over its columns land in them. The result is `TableEvidence::Aligned`
with `TableWarning::NoRules`. Not a table: a run whose columns do not recur,
text in fewer than one cell in four (checked before any cell is made), and —
the false positive that matters — **two or more columns as wide as a column
of prose** (`COLUMN_MIN_WIDTH_EMS`, the reading-order inference's constant),
which is a page set in columns rather than a table. Every threshold is in ems
of the page's median line size and lives in one place.

### The surface and the handoff

`tpdf text --tables` prints each page's tables instead of its text — the
ones its structure tree states, or else the inferred ones labelled by their
evidence, with their header evidence and warnings — a line for each table and
one for each cell, a span written as a range, so the output is as long as the
cells and not as the grid. The reading-order handoff is the sibling design's
`TableSuspected`: the reading-order inference asks for the page's **ruled**
tables only (`ruled_tables`, the lattice half of the inference without the
aligned pass) and reads each as one block in the permutation's order.

## The adjudicator, and the scores

**Stated against inferred, with the tree hidden.** `table_census.rs`, in
`corpus.yml`'s census step, counts the table family per corpus by
`standard_type`, runs over every tagged file with a `Table`, and for each
page with a stated table:

1. **Found.** Whether an inferred table's bounds overlap the stated table's
   claimed-character quad by at least half. Recall over stated tables, per
   corpus.
2. **Grid agreement.** Whether the inferred row and column counts equal the
   stated ones, exact. Printed per corpus by evidence class, so that `Ruled`
   and `Aligned` are never averaged together.
3. **Cell assignment.** Of the characters the tree claims for cells of that
   table, the fraction the inference placed in the same cell — at its stated
   row and column with its stated spans — with the stated spans reproduced
   counted beside it.
4. **Extra tables.** Inferred tables on pages where the tree claims none.
   Printed for SafeDocs and pdfjs, because producers under-tag and an
   untagged real table is not a false positive; **asserted zero over the
   veraPDF table fixtures**, where everything is tagged and an extra table is
   a mistake.
5. **Spans.** Over the eight span fixtures, printed by name: the stated spans
   reproduced on the two `pass` files, and on the six `fail` files a
   `TableWarning::SpanInconsistent` rather than a lattice built to the
   producer's arithmetic.

It also prints the most rules any scored page draws. Holding the scores as
floors — compared by integer cross-multiplication, `compared` recorded
beside `held`, as `corpus/ratchet.json`'s relations are — is
[ROADMAP](../ROADMAP.md) row FT-31.

**The first-party construction that is admissible.** This engine's EPUB
pipeline lays out real producers' XHTML tables through CSS 2.2 §17's model
and writes `Table`, `TR`, `TH` and `TD` elements for them —
`epub_structure.rs` asserts all four against the source markup. The XHTML
grid is the author's statement of the table, made by a third party, and a
book's table laid out here, then reconstructed here from the page's rules,
must recover that grid exactly. It is an arithmetic fixture with a known
answer, and its limit is the one the reading-order design states: the layout
engine that drew the rules and the inference that reads them share an author.

**First-party fixtures** (`crates/tinker-pdf/tests/tables.rs`). An EPUB
table with a header row, a column span and a row span reads back from its
tree exactly as its markup states it, the header cells `TH` with `/Scope /Column`,
every cell's characters spelling its text. A grid of three rows of four
cells stroked as lines yields exactly its nine lines at the coordinates and
weight drawn; the same grid drawn as twelve cells' CSS-style borders yields
forty-eight rules, four a cell, centred on each border rectangle; a
rectangular clip cuts a rule and `Q` lifts it; a triangular clip refuses the
rule and names it; the cap plus one rules reads none. An untagged ruled grid
drawn a column at a time is read row by row, every cell its own text; tagged
row by row, with the tree hidden, it is found over the stated table with the
same grid and every character in its stated cell; an EPUB table with every
cell bordered is recovered exactly, spans included, in both of CSS 2.2
§17.6's border models, against the tree the EPUB writer made from the XHTML;
no table is inferred on any committed page whose tree states none, nor in a
framed paragraph, a form of sixteen boxes and two labels, or a page of
hatching. A missing rule is a column span of two and an L of missing rules
is named; a grey first row is `FillBeneath`, its unfilled twin `FirstRow`, a
striped table `FirstRow`, a rule three times as heavy or doubled
`RuleBeneath`, and an EPUB table whose `th` cells have a background
`FillBeneath`; a Hebrew ruled grid reads from its rightmost cell — a builder
grid whose glyphs are set in visual order, because the EPUB path does not lay
out `direction: rtl` on a table (a refused property). A committed book's
unruled twelve-cell table — a centred header row over three body columns at
recurring left edges — is found `Aligned` over the stated table with the
same grid; an unruled table tagged row by row and drawn column by column,
each row set up to a point off the others, is found; a report whose columns
are padded with spaces inside one string per row is cut into its cells; a
paragraph of prose, two columns of prose, two aligned rows and a Courier
listing whose every word starts under the one above are not tables. Every
`TableWarning` variant is reached by a fixture.

**What none of this measures.** Whether a producer's `Table` is a table. A
layout grid used for positioning is tagged `Table` by some producers and is
scored here as one; a real table tagged as paragraphs is invisible to the
census and counted as an extra. The extra count is printed for that reason,
and the scores are over what producers said, which is the honest sentence.

## Milestones

1. `TableSource::Stated`: the tree walk, the attribute reader (`/A` with
   `/RowSpan`, `/ColSpan`, `/Headers`, `/Scope`, plus `/ID`), cell quads from
   the claimed-character join; `table_census.rs` counting stated tables.
   Exit: the census prints `RAN` and counts the stated tables per corpus; the
   eight span fixtures read their spans as their outlines state them;
   `hostile_input.rs` covers the attribute walk with zero panics.
2. Rule extraction with rectangular clip accumulation. Exit: a builder-drawn
   3×4 ruled grid yields its rules at the coordinates the test computes; a
   rule under a non-rectangular clip yields `ClipNotRectangular` and no rule;
   a page past `MAX_TABLE_RULES` yields `TooManyRules`.
3. Ruled-table reconstruction: lattice, cells, text assignment, order. Exit:
   recall, grid agreement and cell assignment over SafeDocs' and pdfjs' ruled
   tables at recorded floors; **zero extra tables over the veraPDF table
   fixtures**; the EPUB table reftest recovers the XHTML grid exactly;
   `plain_text()` byte-identical over the fingerprint suite.
4. Spans, header evidence, `rtl` ordering. Exit: the two `pass` span
   fixtures reproduced and the six `fail` ones refused with
   `SpanInconsistent`; a header row with a fill beneath yields `FillBeneath`
   on a builder fixture and `FirstRow` on its unfilled twin; a right-to-left
   table orders its cells right to left.
5. Aligned tables, labelled. Exit: recall rises over the tagged population
   with `Aligned` scored separately from `Ruled`; the extra count printed; a
   paragraph of prose yields no table (the false positive that matters, as a
   fixture).
6. The surface: `tpdf text --tables`, the reading-order handoff
   (`TableSuspected` there, `TreePresent` here). Exit: every `TableWarning`
   variant reached by a fixture.

## Dependencies

- **The tee in `observe.rs`** — one interpretation per page, shared with the
  reading-order inference.
- **`structure.rs`** — `StructElement::table` and `StructElement::id`, the
  attribute reader this design and [design/pdfua.md](pdfua.md) share.
- **`StructureTree::text_for_page`** — the claimed-character join.
- **`TextDevice`** — the line assembler.
- **The EPUB table pipeline** (`crates/tinker-pdf-layout`'s §17 model,
  `epub_tables.rs`, `epub_structure.rs`).
- **[design/reading-order.md](reading-order.md)** — the two designs share
  the tee, the label discipline and the handoff.

## Risks

| Risk | Mitigation |
| --- | --- |
| **A wrong table looks like a table.** Aligned numbers in a paragraph, a form's boxes, a page's decorative rules | Opt-in and labelled; the veraPDF fixtures assert zero extras; a prose page is a fixture; the extra count over real documents is printed every run and read, not averaged |
| The adjudicator is a producer's tag, and producers tag layout grids as tables and real tables as nothing | Recall and agreement are over what producers said; the extra count is printed rather than asserted on real documents; the EPUB reftest carries an author's grid that is not a tagger's |
| Junction finding is quadratic in rules | `MAX_TABLE_RULES` with a typed refusal, recorded in `bounds_ledger.rs` like every other cap |
| Spans built from a producer's inconsistent arithmetic corrupt the grid | The six `fail` fixtures refuse rather than repair, and `Stated` reports what the file said with a warning rather than what a repair would want |
| Two designs share the attribute reader and drift | One reader, `StructElement::table` in `structure.rs`, serves both |
