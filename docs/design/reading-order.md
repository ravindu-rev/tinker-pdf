# Inferred reading order for untagged pages

A caller who asks for it — and only a caller who asks — gets a reading order
for an untagged page that this engine *inferred* from geometry: columns
found, running heads and page numbers set aside, footnotes placed after the
body they annotate, ruled tables read as one block. The answer is a type of
its own, `ReadingOrder::Inferred`, never the default and never merged into
`TextPage`, so that a guess cannot be mistaken for the file's own statement
of its order. "Labelled as inferred" is the design constraint everything
below serves.

**What no design can promise is "the order a human would choose."** There is
no conformance corpus for that: no published set of pages with the reading
order a person chose, annotated, that ruling 13 ([rulings.md](../rulings.md))
could admit as data. This document says so first and then says what is
measured instead — an order that *other people's producers* wrote into
tagged files, scored against this engine's inference with the tree hidden —
and what that measurement can and cannot stand for.

## The stream order, stated precisely

`Page::text()` is `TextDevice` (`crates/tinker-pdf-content/src/text.rs`)
assembling glyph events into lines and blocks. **It sorts nothing.** A glyph
continues the current line when it runs the same way, sits within half an em
of the line's baseline, is not more than three ems behind the previous
glyph, and — after an `ET` — begins within half an em of where the pen
stopped; otherwise it starts a new line, which is pushed after the last. A
block is consecutive lines whose vertical gap is at most 1.5 line heights.
`plain_text()` walks blocks then lines in that order. So the order a caller
gets is the **content stream's**, with geometry deciding only whether two
glyphs are one line and two lines one block (and, within a line holding a
right-to-left character, ruling 14's logical order). A two-column page whose
producer wrote column one then column two reads correctly, and one whose
producer wrote line by line across both columns reads interleaved — that is
the whole of the untagged story, and it is the stream's, not geometry's.

A tagged document is different: `Document::structure()` and
`StructureTree::text_for_page` give 14.8's structure order over the
**same** `TextPage`, and content-and-text.md is explicit that "nothing is
inferred" for the untagged majority. This design does not touch that path.

## Scope

- **`ReadingOrder`** on the facade, three variants and an explicit request:
  `Stream` (what `plain_text()` gives, named), `Stated` (the structure
  tree's, `None` when there is none — the join, wrapped), and `Inferred`,
  which is opt-in on `Page` and returns an `InferredOrder`.
  `Page::text_in(ReadingOrder)` answers with an `OrderedText` labelled by the
  order it is, so a request for `Inferred` on a tagged page comes back
  `Stated`.
- **`InferredOrder`**: the page's blocks in inferred order, each block a run
  of `TextChar`s taken from the same `TextPage` — the discipline
  `text_for_page` established, never a second extractor — with a **role**
  (`Body`, `RunningHead`, `RunningFoot`, `PageNumber`, `Footnote`, `Table`,
  `Unplaced`), the column it was assigned to, and the permutation from stream
  order, so a caller can undo the inference character for character.
- **Three inferences**: columns from whitespace between line quads; running
  heads, feet and page numbers from position and cross-page repetition;
  footnotes from position, size, separator rules and raised reference marks.
  Ruled tables are handed off to the table inference first.
- **Warnings with provenance** (ruling 10): `InferenceWarning::
  ColumnsAmbiguous`, `PageTooSparse`, `RotatedText`, `VerticalWriting`,
  `TableSuspected`, `NoBodyText`, `NoCrossPageEvidence`, `TreePresent` — and
  `Declined { reason }` when the inference will not guess at all
  (`DeclineReason::TreePresent`, `VerticalWriting`, `RotatedText`, `Table`).
- **The measurement** (the section below): `reading_order_census.rs`, its
  floors in `INFERRED_FLOORS` ([ROADMAP](../ROADMAP.md) row FT-30).
- **Surfacing**: `tpdf text --order stream|stated|inferred`.

## Non-goals

- **A human-judged corpus.** None exists and none will be made here: an
  order this project's authors chose and committed is this project agreeing
  with itself, which is the thing ruling 13 says proves nothing.
- **Auto-tagging.** An inferred order never becomes a structure tree, never
  enters a written document, and never reaches
  [design/pdfua.md](pdfua.md)'s verdicts: content-and-text.md refuses "a tree
  guessed from geometry presented as the file's own statement". A tree
  written into a new document as that document's statement, never presented
  as the input's own, is [ROADMAP](../ROADMAP.md) row SD-07.
- **Changing the default.** `plain_text()`, `search()` and selection quads
  do not change by a byte; the fingerprint suite pins that.
- **Bidi reordering inside a line.** That is ruling 14's, applied by
  `Page::text` whatever order is asked for. What this design does with
  `TextLine::rtl` is choose which way columns run.
- **Table cells.** A ruled table the table inference finds is one unit here,
  ordered as a whole; its cells are
  [design/table-reconstruction.md](table-reconstruction.md)'s.
- **A probability.** No calibration data exists to make a number honest, so
  confidence is a set of named warnings, not a score.
- **Learned weights.** Every threshold is a constant with a stated unit and
  an arithmetic fixture, because a threshold nobody can derive is a
  threshold nobody can defend when a real page moves it.

## Design

### The label is a type, and the failure mode is the type's

A wrong inference is not a crash and not a warning: it is a page read in an
order a person would not read it, silently. The defence is that the wrong
answer can only ever be **asked for**. `ReadingOrder::Inferred` is a
different value from `Stream`, carries `InferenceWarning`s, and carries the
permutation back to stream order; a consumer that stores an inferred order
stores the label with it, and one that finds it wrong has the stream order
one index lookup away. `TextPage` is untouched, so anything that reads the
page without asking — search, selection, redaction, the EPUB conservation
census — cannot see the inference exist. That is the whole of "labelled as
inferred": not a flag on the answer but a separate answer.
`crates/tinker-pdf/src/reading_order.rs` holds the types, the constants and
the inference.

### One interpretation into a tee

The inference reads the page through **one interpretation into a tee**
(`crates/tinker-pdf/src/observe.rs`): a device that hands `TextDevice`
exactly the four calls it implements and answers the interpreter's three
questions with `TextDevice`'s own answers, so the page it builds is
`Page::text()`'s character for character — held over every committed
untagged document by `the_inference_reads_the_same_page_text_reads` — and
keeps the paths and clips for itself, which is where the footnote separators
and the tables' rules come from. With the tree hidden it reads `/Artifact`
scopes as content, because an inference measured against a tree has to find
a running head without the producer's artifact mark telling it to. Line and
block assembly stay `TextDevice`'s — one assembler, or the structured and
flat views drift — and anything across pages is read page by page.

### The inferences

**Tables first.** The page's ruled tables — the sibling design's
`Page::inferred_tables`, over the same one interpretation — claim their
characters before anything else is decided, and each is read as one block
of `Role::Table`: its lines a cell at a time, row by row in the table's
direction, in exactly the order of the table's own permutation. The page
says so (`InferenceWarning::TableSuspected { tables }`). A table's lines are
not looked at for columns, running heads or footnotes: its frame stands over
the column finder's coverage as a block of text would — the whole frame,
rules and the white of its cells, because a column holding a paragraph and a
table is not open space — and it is placed in the column that holds its
frame, or across the columns as a spanner where its frame crosses a gap,
heading the band under it. Its own rules are no footnote separator, and a
block of the text device's that runs from a line just above a table to one
just below is cut where the table stands. **Only ruled tables are handed
off**: an aligned table is weaker evidence — nothing the page drew bounds
it — and its lines are read as lines. A page more than half of whose
characters are in tables is declined (`DeclineReason::Table`), as a page
mostly of vertical or rotated lines is: its order is the tables', not a
reading order's.

**Columns.** The inference cuts each line into fragments at internal
whitespace of at least a column gap, cuts the body into elementary intervals
at the fragments' edges, and finds every gap in one pass (three columns are
two gaps, not a recursion): a run of intervals free of text over
`COLUMN_FREE_SHARE` (60 %) of the body's height, at least `COLUMN_GAP_EMS`
wide, with a column of text at least `COLUMN_MIN_WIDTH_EMS` wide on both
sides. A line that runs across a gap without touching it — a producer
drawing both columns' lines at once — is cut in two; a line whose glyph
boxes hold the cut is a spanner, and a run of spanners ends one band of the
page and heads the next (`InferredBlock::section`), so a heading over two
columns reads before them and a paragraph set across the page between two
column sets reads between them. Inside a column, the text device's blocks
are kept as units, ordered by their tops, with the stream's order wherever
two stand level (`ROW_TOLERANCE_EMS`). Columns run right to left when most
lines carry `TextLine::rtl`. Rotated and vertical lines go last as
`Unplaced`, and a page that is mostly either is declined. Fewer than
`MIN_COLUMN_LINES` lines of body text and no columns are looked for
(`PageTooSparse`); a gap that came within a factor of two of separating
columns and did not is `ColumnsAmbiguous`. The constants are ems of the
page's median line size, so a page set at 7 pt and one at 12 pt meet the same
rule.

`COLUMN_GAP_EMS` is **0.8**: `css-multicol-1`'s `column-gap: normal` is 1em
and LaTeX's `\columnsep` is 10 pt in a 10 pt document, a justified column
ends exactly at its edge, and at exactly 1 em the gap would be met to the
last bit of a sum of advances (`a_two_column_book_reads_down_its_columns`).
`COLUMN_MIN_WIDTH_EMS` (8) keeps labels beside their values one column,
because reading every label before any value is the one order nobody wants
(`labels_beside_their_values_are_one_column`). A whitespace run that reaches
the outermost edge of the text is a margin, never a gap, so a ragged right
edge is not a near miss (`a_ragged_edge_is_a_margin_and_not_a_gap`).

**Running heads, feet and page numbers.** A line lying wholly in the top or
foot `MARGIN_BAND` (12 % of the page's height) is a page number when a
neighbouring page carries, in the same band at the same height to within an
em, the numeral its own value plus the offset between the two pages —
decimal or canonical roman, with dashes, bars, brackets and a full stop
around it — and a running head or foot when `RUNNING_REPEATS` (two)
neighbours carry its text, digits masked so "Page 3 of 9" recurs, at the same
place to within an em. Both are taken out of the body before columns are
looked for, so a head across the page is never a spanner, and are read first
and last with their roles; a caller that wants them gone filters on the
role. The window is the pages around the page: `RUNNING_WINDOW` (16) pages,
half before and half after, because page 300's running head names its
chapter and is not on pages 1 to 16. `Document::inferred_orders` reads each
page's margins once for a whole range; `Page::inferred_order`,
`Document::inferred_order` and it give the same answer. With fewer than two
other pages to compare — a one-page document — a block wholly in a band is
`Unplaced` where it stands and `InferenceWarning::NoCrossPageEvidence` says
why; nothing is called a running head on no evidence. Each neighbour is
asked as a sweep along the lines' heights, the lines within an em held in an
ordered set by edge, and not line against line, so the work stays bounded
however many margin lines a stream draws
(`margin_lines_are_judged_in_bounded_work`, in
`crates/tinker-pdf/src/reading_order.rs`).

**Footnotes.** In the page's last band, the run of blocks at the foot of
each column — and below the columns, across the page — whose every line is
set at most `FOOTNOTE_SIZE_RATIO` (0.9) of the body's median size is a run
of notes when a horizontal rule at least `SEPARATOR_SHARE` (a third) of the
column wide stands between it and the body above, or when it opens with a
raised glyph. A glyph is raised when its origin stands `RAISE_EMS` (a fifth)
of its line's size above the line's baseline, which the text device's
`TextChar::origin` keeps — `Glyph::baseline` joins a raised glyph to its
line, and the risen origin is where the ink is. A run is cut into one note
at every line that opens with a raised marker; each note's marker is its
leading raised glyphs, or a short numeral or note symbol on its baseline.
Notes are read after the body, before the running feet, in the order of
their reference marks — the raised glyph with the same text in the body, by
its place in the order read — and in the order they stand where no mark was
found. They leave the body before the margin bands are judged, so a note in
the foot band is a note and not an unplaced margin block. A `/Note`
structure element is the tagged form of this, and is the ground truth below.

`Role` has no caption: a caption needs the figure the tee does not record.
Every rule is arithmetic on quads and sizes; nothing here needs a
transcendental, so the determinism contract (ruling 4) costs no
`tinker-pdf-math` call, and the same page gives the same order on every
target.

`tpdf text --order stream|stated|inferred` prints the order asked for, and
on standard error which order each page got and what an inference
tolerated; `--order stated` of an untagged document is refused.

## What is measured instead of a human, and what it stands for

**The adjudicator is the producer.** Each tagged file of the fetched
corpora has a structure order that somebody other than this project wrote,
and 14.8 makes that order the document's statement of how it reads. Hide the
tree, infer over the same `TextPage`, and compare — that is the roadmap row's
exit criterion, and it is the only third-party statement about reading order
that exists at this scale.

**The population is chosen, not summed.** veraPDF's tagged files are
conformance fixtures, a page with one paragraph on it, where every order
agrees with every other; scoring them would report agreement the inference
did nothing to earn. The ratchet population is the tagged files of pdfjs and
SafeDocs (`RATCHET_POPULATION`), and veraPDF's tagged fixtures are the set
where the inference must not *move* anything (`NOTHING_MAY_MOVE`), because
there is nothing to move.

Four scores, each printed per corpus:

1. **Pair agreement.** For every pair of characters that both the tree and
   the inference order, the fraction ordered the same way, reported for
   `Stream` beside the inference, so that "the inference improved on the
   stream" is a number and not a hope.
2. **Column crossings.** Walking the tree's order, the sequence of inferred
   column indices should never return to a column it left. Each return is a
   crossing, counted per page. This is the one score that measures the
   column inference and nothing else, because a producer that tagged column
   one then column two has said where the columns are without saying the
   word.
3. **Running-head precision.** Of the blocks the inference called a running
   head, foot or page number, the fraction the producer drew as an artifact
   in a margin band — by position, because the device seam carries a
   property list's `/MCID` and 14.9's entries but not an artifact's
   `/Type`, so `/Pagination` cannot be told from `/Layout` by name. Recall is
   printed beside it — producers under-mark — but precision is what matters,
   because calling body text a running head is the mistake that drops a
   paragraph.
4. **Footnote precision**, likewise, against the producer's `/Note`
   elements: of the blocks called footnotes, the fraction the producer tagged
   as one.

The two precisions are scored over every page of a file that marks any, not
only the pages that do. The harness is
`crates/tinker-pdf/tests/reading_order_support/mod.rs`: pair agreement
counted as inversions by a merge sort (held to arithmetic by
`the_pair_score_counts_inversions`), column crossings, and the role scores;
characters are matched across the two extractions by origin and text, the
k-th repeat to the k-th, and `the_score_sees_a_shuffled_page` holds the
instrument seeing disorder. `reading_order_census.rs` is the corpus census,
`#[ignore]`d, in `corpus.yml`'s census step, printing `RAN`/`SKIPPED`; it
asserts what holds whatever the corpus holds — every inferred order a
permutation, a floor only on the ratchet population, every floor in
`INFERRED_FLOORS` met by cross-multiplication as `corpus/ratchet.json`'s
bars are, and **zero characters moved on veraPDF**.

**The one first-party construction that is admissible, and why.** This
engine's EPUB pipeline lays out real producers' books into multi-column
pages (`css-multicol-1`, laid out as one `Abreast` shape) and floats, and the
XHTML's document order is the *author's* statement of the reading order,
made by a third party. `epub_float_order.rs` asserts that order survives
layout. A two-column reftest whose inferred order equals the spine's order
is an arithmetic fixture with a known answer, and it is admissible because
the answer is the book's, not this project's. What it cannot show is stated:
the layout engine that placed the columns and the inference that finds them
share every threshold their author has.

**First-party fixtures** (`crates/tinker-pdf/tests/reading_order.rs`). The
content stream agrees with the tree on every pair of every committed EPUB
book — the EPUB writer draws in the order it tags — so the books are in the
set where nothing may move. A two-column fixture drawn line by line across
both columns and tagged column by column, a spanning fixture, three columns
and right-to-left columns each read every pair the tree's way with no
crossings; every one-column fixture, every committed `testdata` page and
every page of the committed books moves no character; the two-column EPUB
book is found as two columns on every page set in two, and the same book
redrawn as a line-by-line producer would draw it reads in the author's order
(`the_two_column_book_redrawn_across_the_page_reads_in_the_authors_order`).
A six-page builder book with a verso and a recto head, a numbered foot and
two interleaved columns between them, its furniture drawn as `/Artifact
/Pagination` and its stream drawing the number first and the head last,
reads head first, number last and body between; a same word set at a
different place on every page's top band stays body; a `Page # of #` head
and a roman page number are found. No EPUB fixture carries running heads:
the EPUB path draws none (no `css-page-3` margin boxes). A page of body text
with two raised reference marks and two notes under a separator rule, drawn
note 2, body, note 1 and tagged body then `/Note`s, reads every pair the
tree's way; a rule over notes marked on the baseline, and raised markers
without a rule, each make two notes; note 2 set above note 1 still reads
after it, and with no marks in the body the notes read in the order they
stand; two notes drawn as one block of the text device's are cut at their
markers; and a rise of zero everywhere changes nothing. Paragraphs around a
ruled table, drawn baseline by baseline across the page and tagged cell by
cell, read with the table one `Table` block equal to the table inference's
permutation and nothing outside it moved, in one column, in the left column
of two, and across two column sets; a page mostly table is declined with
nothing moved; a small source line under a table's bottom rule is body, not
a footnote; a book's bordered table moves nothing and is one `Table` block.
Every `InferenceWarning` variant is reached by a fixture — `VerticalWriting`
and `Declined { VerticalWriting }` by a hand-assembled `/Identity-V` page,
since the document builder writes no vertical font.

**What none of this measures.** Whether the producer's order is the one a
person would choose. A tagging tool that emits structure in stream order —
and the `Stream` score says how many do — is a statement about nothing, and
agreement with it is agreement with the stream. The `Stream` score is what
tells the ratchet population from the noise: files where `Stream` scores 1.0
against the tree carry no information about columns, and the
column-crossings score is computed over the rest.

## Milestones

1. `ReadingOrder`, `OrderedText` and `InferredOrder`; `Stream` and `Stated`
   wrapping the stream and structure orders. Exit: `plain_text()`
   byte-identical over the fingerprint suite; `tinker_parity.rs` unchanged;
   `tpdf text --order stated` on a tagged fixture equals
   `structured_text().plain_text()`.
2. The measurement harness: `reading_order_census.rs` scoring `Stream`
   against the tree over the tagged files, with the population split above,
   `RAN`/`SKIPPED`. Exit: a pair agreement per corpus; the count of files
   where `Stream` agrees exactly, printed.
3. Columns and block ordering. Exit: column crossings over the multi-page
   ratchet population fall from the stream's; pair agreement does not fall
   on any corpus; veraPDF's tagged fixtures: zero characters moved; the
   two-column EPUB reftest recovers the spine's order exactly.
4. Running heads, feet and page numbers across the pages around a page.
   Exit: precision over the producers' margin artifacts at a recorded floor;
   recall printed; a single-page document yields `Unplaced` with a warning
   and no role; the builder book with a running head places it first.
5. Footnotes and reference marks. Exit: precision over `/Note` elements at a
   recorded floor; a builder fixture with two footnotes and their raised
   markers orders them after the body in mark order; a rise of zero
   everywhere changes nothing.
6. The surface and the table handoff: warnings, `Declined` for vertical,
   rotated and table pages, `tpdf text --order inferred`. Exit: every
   `InferenceWarning` variant reached by a fixture; the `TableSuspected`
   handoff asserted on a ruled-table page.

## Dependencies

- **`TextDevice` and `TextPage`** — the one assembler.
- **The tee in `observe.rs`** — one interpretation per page, shared with
  table reconstruction.
- **`StructureTree::text_for_page`** — the claimed-character join the
  scores are computed over.
- **The census step of `corpus.yml`** — where `reading_order_census.rs`
  runs.
- **The EPUB pipeline's multi-column layout and `epub_float_order.rs`.**
- **[design/table-reconstruction.md](table-reconstruction.md)** — the
  `TableSuspected` handoff; tables first, then order, on a page that has
  both.

## Risks

| Risk | Mitigation |
| --- | --- |
| **There is no ground truth, and the substitute is a producer's opinion.** A tagging tool that tags in stream order makes agreement meaningless | The `Stream` score is measured beside the inference and the files it already matches are excluded from the column score; the EPUB reftests carry an author's order that is not a tagger's |
| A wrong order looks like a page. Nothing crashes; a paragraph is read in the wrong place | The answer is opt-in, labelled by type, and carries its own undo; `plain_text()` cannot change; every threshold has an arithmetic fixture beside it |
| The veraPDF fixtures dominate the tagged count and inflate any average | They are the set where nothing may move, never the ratchet population, and the split is asserted by corpus name in the census |
| Thresholds tuned to the corpora are wrong for the world | Every constant is in ems of the page's own median size and named in one place; a real page that moves one is a fixture with its number, not a tweak |
| Cross-page repetition needs pages the caller has not asked for | `RUNNING_WINDOW` bounds the pages read, so an inferred order costs at most that many other pages' text whatever the document's length; with too few, margin blocks are `Unplaced` with `NoCrossPageEvidence` |
| The inference runs on a tagged page because a caller asked for `Inferred` without checking `Stated` | `Page::text_in(ReadingOrder::Inferred)` on a tagged page answers `Stated`, and `Page::inferred_order` declines with `DeclineReason::TreePresent` unless `InferenceOptions::hide_structure` is set — a guess is never preferred to a statement |
