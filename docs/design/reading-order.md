# Inferred reading order for untagged pages

When this is done, a caller who asks for it — and only a caller who asks —
gets a reading order for an untagged page that this engine *inferred* from
geometry: columns found, running heads and page numbers set aside, footnotes
placed after the body they annotate. The answer is a type of its own,
`ReadingOrder::Inferred`, never the default and never merged into
`TextPage`, so that a guess cannot be mistaken for the file's own statement
of its order. The roadmap row says "labelled as inferred", and that label is
the design constraint everything below serves.

**What no design can promise is "the order a human would choose."** There is
no conformance corpus for that: no published set of pages with the reading
order a person chose, annotated, that ruling 13 ([rulings.md](../rulings.md))
could admit as data. This document says so first and then says what is
measured instead — an order that *other people's producers* wrote into 1 078
tagged files, scored against this engine's inference with the tree hidden —
and what that measurement can and cannot stand for.

## What exists today, stated precisely

`Page::text()` is `TextDevice` (`crates/tinker-pdf-content/src/text.rs`)
assembling glyph events into lines and blocks. **It sorts nothing.** A glyph
continues the current line when it runs the same way, sits within half an em
of the line's baseline, is not more than three ems behind the previous
glyph, and — after an `ET` — begins within half an em of where the pen
stopped; otherwise it starts a new line, which is pushed after the last. A
block is consecutive lines whose vertical gap is at most 1.5 line heights.
`plain_text()` walks blocks then lines in that order. So the order a caller
gets is the **content stream's**, with geometry deciding only whether two
glyphs are one line and two lines one block.

Two documents say otherwise. [features/content-and-text.md](../features/content-and-text.md)'s
refusal table says `plain_text()` "orders lines and blocks geometrically and
always has", and the roadmap row's evidence column says "geometric line and
block order". Neither is what the code does; the order is the producer's, and
both sentences are corrected in the commit that adds this document rather
than built on. A two-column page whose producer wrote column one then column two
already reads correctly today, and one whose producer wrote line by line
across both columns reads interleaved — that is the whole of the untagged
story, and it is the stream's, not geometry's.

A tagged document is different and stays different: `Document::structure()`
and `StructureTree::text_for_page` give 14.8's structure order over the
**same** `TextPage`, and content-and-text.md is explicit that "nothing is
inferred" for the untagged majority. This design does not touch that path.

## Scope

- **`ReadingOrder`** on the facade, three variants and an explicit request:
  `Stream` (what `plain_text()` gives today, named), `Stated` (the structure
  tree's, `None` when there is none — the existing join, wrapped), and
  `Inferred`, which is opt-in on `Page` and returns an `InferredOrder`.
- **`InferredOrder`**: the page's blocks in inferred order, each block a run
  of `TextChar`s taken from the same `TextPage` — the discipline
  `text_for_page` established, never a second extractor — with a **role**
  (`Body`, `RunningHead`, `RunningFoot`, `PageNumber`, `Footnote`,
  `Caption`, `Unplaced`), the column it was assigned to, and the permutation
  from stream order, so a caller can undo the inference character for
  character.
- **Three inferences**, each a milestone: columns from whitespace between
  line quads; running heads, feet and page numbers from position and
  cross-page repetition; footnotes from position, size and the reference
  marks the recording device already distinguishes.
- **Warnings with provenance** (ruling 10): `InferenceWarning::
  ColumnsAmbiguous`, `PageTooSparse`, `RotatedText`, `VerticalWriting`,
  `TableSuspected`, `NoBodyText`, `TreePresent` — and `Declined { reason }`
  when the inference will not guess at all, which is the answer for a
  vertical-mode page and for a page the sibling design claims as a table.
- **The measurement** (the section below), ratcheted in
  `corpus/ratchet.json` as a sixth axis beside `tagged`.
- **Surfacing**: `tpdf text --order stream|stated|inferred`.

## Non-goals

- **A human-judged corpus.** None exists and none will be made here: an
  order this project's authors chose and committed is this project agreeing
  with itself, which is the thing ruling 13 says proves nothing.
- **Auto-tagging.** An inferred order never becomes a structure tree, never
  enters a written document, and never reaches
  [design/pdfua.md](pdfua.md)'s verdicts. content-and-text.md refuses "a tree
  guessed from geometry presented as the file's own statement", and the
  refusal stands.
  *Narrowed, 9 October 2026*: auto-tagging is [ROADMAP](../ROADMAP.md) row SD-07 — a
  tree written into a new document as that document's statement, never
  presented as the input's own.
- **Changing the default.** `plain_text()`, `search()` and selection quads
  do not change by a byte; the fingerprint suite pins that, and every
  milestone's exit repeats the pin.
- **Bidi reordering inside a line.** A right-to-left line is still reported
  in stream order with `TextLine::rtl` set; that is the tier 3 row
  "Reading order of a right-to-left line in text extraction" and it stays
  there. What this design does with `rtl` is choose which way columns run.
- **Tables.** A block the table design recognises is one unit here, ordered
  as a whole; its cells are [design/table-reconstruction.md](table-reconstruction.md)'s.
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

### What the recording device gives this design, and what it does not

`crates/tinker-pdf-content/src/record.rs` names inferred reading order as one
of six consumers it was promoted for. Precisely what it supplies:

- **Glyph events with a font identity.** `Glyph::font_id` is stable within
  an interpretation; `TextChar` carries text, quad, size, origin and `/MCID`
  and **no font id**, so "the footnote is set in a different face from the
  body" is a question only the recorder's events answer.
- **The rise.** `Glyph::baseline` is `Some` exactly when 9.4.3's `Ts` is in
  force, and its own doc says why a rise is not a line: a superscript
  footnote marker is on the line it interrupts. A reference mark in the body
  and a marker at the head of a footnote block are the pair the footnote
  inference joins, and the recorder is what tells a raised `1` from a `1` on
  the baseline.
- **Marked-content nesting.** `scopes_at(index)` gives every scope open at
  a glyph, and a scope tagged `Artifact` with a `/Pagination` type and a
  `Header` or `Footer` subtype (ISO 32000-1 14.8.2.2, Table 330) is a
  producer saying "this is a running head" — which is the ground truth the
  running-head inference is scored against, below.
- **Paths and images.** A horizontal `StrokePath` above a block of small
  text is the footnote separator every typesetter draws; a `DrawImage` is a
  figure that interrupts a column and has a caption under it. Neither is in
  `TextPage`.

The capture this needs is `Capture { text: true, paths: true, images: true,
structure: true, state: false }`. **That is not `Capture::GLYPHS`**, and
`record.rs`'s module doc says both that this consumer "wants glyphs and the
marked-content nesting" and that it runs under `GLYPHS`, whose `structure:
false` gates every `begin_marked_content` off and makes `hidden_at` answer
`false` everywhere. The doc contradicts itself on this consumer and should be
corrected when milestone 3 lands; the design here follows the first sentence.

What the recorder does **not** give: line and block assembly, which stays
`TextDevice`'s — one assembler, or the structured and flat views drift; page
boxes and page count, which are the page's; anything across pages, since a
transcript is one interpretation of one page; and — until September 2026 — a
replay. The retained-page row landed then, and `tinker_pdf_content::replay`
hands a transcript to any device, the `TextDevice` included. Before it did,
the inference was costed at a second interpretation of the page —
`TextDevice` for the lines and `RecordingDevice` for the rest — which ruling
4 makes identical and which is a cost, not a correctness question. Now one
interpretation feeds both.

### The three inferences

**Columns.** Project every line's quad onto the page's x axis, over the body
band (the page with the top and bottom margin bands removed). A vertical
whitespace gap at least 1.5 body-ems wide that no line crosses for at least
60 % of the band's height is a column boundary; the cut recurses to a bounded
depth (two, which is what a page of prose has), and a page whose gaps are
narrower or shorter than that is one column with `ColumnsAmbiguous` when a
gap came within a factor of two of the threshold. Columns run left to right,
or right to left when the majority of the page's lines carry `TextLine::rtl`.
Blocks are ordered top to bottom inside a column, and a block that crosses a
column boundary — a full-width heading over two columns — is placed before
the columns it spans. The constants are ems of the page's median line size,
so a page set at 7 pt and one at 12 pt meet the same rule.

**Running heads, feet and page numbers.** Over the first *K* pages of the
document (K = 16, bounded, and documented), a block is a candidate when it
lies in the top or bottom 12 % of the media box; it is a running head or foot
when a block with the same text and the same position to within one em
appears on at least two other pages, and a page number when its text is a
numeral alone and increments across consecutive pages. A single-page
document has no cross-page evidence and gets `Unplaced` for its margin
blocks with a warning, never a guess. These blocks are ordered first and
last, with their roles, and a caller that wants them gone filters on the
role.

**Footnotes.** A block below the last body block in its column, set at most
0.9 of the body's median size, either beneath a horizontal stroke that spans
at least a third of the column or beginning with a glyph whose `baseline` is
`Some` — a raised marker — is a footnote; its reference mark is the nearest
raised glyph with the same text in the body above. Footnotes are ordered
after the body of their page, in the order of their reference marks where
those were found and in position order where not. A `/Note` structure
element is the tagged form of this, and is the ground truth below.

Every rule is arithmetic on quads and sizes; nothing here needs a
transcendental, so the determinism contract (ruling 4) costs no `tinker-pdf-math`
call, and the same page gives the same order on every target.

## What is measured instead of a human, and what it stands for

**The adjudicator is the producer.** The fetched corpora carry 1 078 tagged
files — pdfjs 90, veraPDF 589, qpdf 37, pdfa-examples 1, SafeDocs 361, from
`corpus/ratchet.json`'s `tagged.files`. Each has a structure order that
somebody other than this project wrote, and 14.8 makes that order the
document's statement of how it reads. Hide the tree, infer over the same
`TextPage`, and compare — that is the roadmap row's exit criterion, and it is
the only third-party statement about reading order that exists at this scale.

**The roadmap's number is stale, and the stale number is exact.** The row
says 717 tagged files. 90 + 589 + 37 + 1 is 717: the four corpora that
existed before the fifth was pinned on 5–6 September 2026. With SafeDocs the
ratchet says 1 078, and a scratch walk on 16 September that supplied no
passwords and took qpdf's whole tree found 1 073 of them, **288 with more
than one page** — 262 of those in SafeDocs, 15 in pdfjs, 7 in veraPDF, 4 in
qpdf. The row is corrected in the same commit as this document.

**And the population has to be chosen, not summed.** 589 of the 1 078 are
veraPDF fixtures, and a conformance fixture is a page with one paragraph on
it, where every order agrees with every other. Scoring them would report
agreement the inference did nothing to earn. The ratchet population is the
tagged files of pdfjs and SafeDocs — 451 files, 277 of them multi-page — and
veraPDF's tagged fixtures are the false-positive set: pages where the
inference must not *move* anything, because there is nothing to move.

Four scores, each printed per corpus and the first three ratcheted:

1. **Pair agreement.** For every pair of characters that both the tree and
   the inference order, the fraction ordered the same way. Reported for
   `Stream` first — the baseline, recorded before the inference exists, so
   that "the inference improved on the stream" is a number and not a hope.
2. **Column crossings.** Walking the tree's order, the sequence of inferred
   column indices should never return to a column it left. Each return is a
   crossing, counted per page. This is the one score that measures the
   column inference and nothing else, because a producer that tagged column
   one then column two has said where the columns are without saying the
   word.
3. **Running-head precision.** Over files whose producers marked
   `/Artifact /Pagination` scopes: of the blocks the inference called a
   running head or foot, the fraction the producer marked as one. Recall is
   printed and not ratcheted — producers under-mark — but precision is a
   floor, because calling body text a running head is the mistake that
   drops a paragraph.
4. **Footnote precision**, likewise, over files whose producers wrote
   `/Note` elements: of the blocks called footnotes, the fraction the
   producer tagged as one.

**The one first-party construction that is admissible, and why.** This
engine's EPUB pipeline lays out real producers' books — nine in the tree —
into multi-column pages (`css-multicol-1`, laid out as one `Abreast` shape)
and floats, and the XHTML's document order is the *author's* statement of the
reading order, made by a third party. `epub_float_order.rs` already asserts
that order survives layout. A two-column reftest whose inferred order equals
the spine's order is an arithmetic fixture with a known answer, and it is
admissible because the answer is the book's, not this project's. What it
cannot show is stated: the layout engine that placed the columns and the
inference that finds them share every threshold their author has.

**What none of this measures.** Whether the producer's order is the one a
person would choose. A tagging tool that emits structure in stream order —
and the baseline score will say how many do — is a statement about nothing,
and agreement with it is agreement with the stream. The milestone-2 baseline
is what tells the ratchet population from the noise: files where `Stream`
already scores 1.0 against the tree carry no information about columns, and
the column-crossings score is computed over the rest.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `ReadingOrder` and `InferredOrder` types; `Stream` and `Stated` wrapping what exists; `Inferred` returning `Declined { NotImplemented }` | `plain_text()` byte-identical over the fingerprint suite; `tinker_parity.rs` unchanged; `tpdf text --order stated` on a tagged fixture equals `structured_text().plain_text()` | S |
| 2 | The measurement harness before the inference: `reading_order_census.rs` scoring **`Stream`** against the tree over the tagged files, with the population split above, `RAN`/`SKIPPED` | A baseline pair-agreement per corpus committed to `corpus/ratchet.json`; the count of files where `Stream` already agrees exactly, printed; a seeded injection — lines shuffled — drops the baseline and fails `--check` | S |
| 3 | Columns and block ordering over the recorder (`structure: true`, the second interpretation until a replay exists) | Column crossings over the multi-page ratchet population fall from the baseline by a recorded margin; pair agreement does not fall on any corpus; veraPDF's tagged fixtures: zero characters moved; the two-column EPUB reftest recovers the spine's order exactly; counted injections with the zeros printed | M |
| 4 | Running heads, feet and page numbers across the first K pages | Precision over `/Artifact /Pagination` marks at a recorded floor; recall printed; a single-page document yields `Unplaced` with a warning and no role; the reftest book with a running head places it first | M |
| 5 | Footnotes and reference marks, using `Glyph::baseline` | Precision over `/Note` elements at a recorded floor; a builder fixture with two footnotes and their raised markers orders them after the body in mark order; a rise of zero everywhere changes nothing (the monotone property the recorder's doc states, asserted here) | M |
| 6 | The surface: warnings, `Declined` for vertical and table pages, `tpdf text --order inferred`; the feature doc's row | Every `InferenceWarning` variant reached by a fixture; the sibling design's `TableSuspected` handoff asserted on a ruled-table page; the roadmap row deleted | S |

## Dependencies

- **`TextDevice` and `TextPage`** — the one assembler. Exists; unchanged.
- **`RecordingDevice`** — exists; needs `structure: true`, not `GLYPHS`,
  and its module doc corrected to say so.
- **A replay of a transcript into a device** — exists since September
  2026 (`tinker_pdf_content::replay`, the retained-page row's deliverable);
  one interpretation per page.
- **`StructureTree::text_for_page`** — the claimed-character join the
  scores are computed over. Exists.
- **`corpus/ratchet.json` and `xtask/src/ratchet.rs`** — a sixth axis with
  the same integer cross-multiplication the other five use. Exists; grows a
  bar.
- **The EPUB pipeline's multi-column layout and `epub_float_order.rs`** —
  exist.
- **[design/table-reconstruction.md](table-reconstruction.md)** — the
  `TableSuspected` handoff; tables first, then order, on a page that has
  both.

## Risks

| Risk | Mitigation |
| --- | --- |
| **There is no ground truth, and the substitute is a producer's opinion.** A tagging tool that tags in stream order makes agreement meaningless | Named, not closed. The `Stream` baseline is measured first and the files it already matches are excluded from the column score; the EPUB reftests carry an author's order that is not a tagger's |
| A wrong order looks like a page. Nothing crashes; a paragraph is read in the wrong place | The answer is opt-in, labelled by type, and carries its own undo; `plain_text()` cannot change; every threshold has an arithmetic fixture beside it |
| The veraPDF fixtures dominate the tagged count and inflate any average | They are the false-positive set, never the ratchet population, and the split is asserted by corpus name in the census |
| Thresholds tuned to 451 files are wrong for the world | Every constant is in ems of the page's own median size and named in one place; a real page that moves one is a fixture with its number, not a tweak |
| The second interpretation doubles the cost of an inferred read on a large page | Bounded by the same limits the interpreter already has; the replay row removes it, and the cost is measured in the census rather than assumed |
| Cross-page repetition needs pages the caller has not asked for | K is bounded and documented; a `Page`-level call with no document context yields `Unplaced` rather than fetching the document |
| The inference runs on a tagged page because a caller asked for `Inferred` without checking `Stated` | `Inferred` on a page with a structure tree returns the tree's order with `InferenceWarning::TreePresent` and does nothing — a guess is never preferred to a statement |

## As built

**Milestones 1 and 2 (3 October 2026): the names and the instrument, before
the guess.** `crates/tinker-pdf/src/reading_order.rs` holds `ReadingOrder`
(`Stream`, `Stated`, `Inferred`), `OrderedText` — the answer labelled by the
order it *is*, so a request for `Inferred` on a tagged page comes back
`Stated` — `Page::text_in(ReadingOrder)`, and `InferredOrder` with its
permutation from stream order, its `InferredBlock`s and their `Role`s.
`Page::inferred_order` and `Document::inferred_order` decline:
`DeclineReason::TreePresent` on a tagged document unless
`InferenceOptions::hide_structure` is set, and `DeclineReason::NotImplemented`
everywhere else, with the stream's blocks unmoved and every one `Unplaced`.
`Page::text()`, `plain_text()` and `search()` are not touched: the new
surface is a sibling of `text_with`, never a field on it.

The inference reads the page through **one interpretation into a tee**
(`crates/tinker-pdf/src/observe.rs`): a device that hands
`TextDevice` exactly the four calls it implements and answers the
interpreter's three questions with `TextDevice`'s own answers, so the page it
builds is `Page::text()`'s character for character — held over every
committed untagged document by `the_inference_reads_the_same_page_text_reads`
rather than argued. With the tree hidden it reads `/Artifact` scopes as
content, because an inference measured against a tree has to find a running
head without the producer's artifact mark telling it to.

The harness is `crates/tinker-pdf/tests/reading_order_support/mod.rs`: pair
agreement counted as inversions by a merge sort (held to arithmetic by
`the_pair_score_counts_inversions`), column crossings, and a role score for
milestones 4 and 5; characters are matched across the two extractions by
origin and text, the k-th repeat to the k-th. `reading_order_census.rs` is
the corpus census, `#[ignore]`d, in `corpus.yml`'s census step, printing
`RAN`/`SKIPPED`; it splits the population by corpus name (pdfjs and SafeDocs
the ratchet population, veraPDF where nothing may move) and asserts what
holds whatever the corpus holds — every inferred order a permutation, a
floor only on the ratchet population, and, since the review of this lane,
**zero characters moved on veraPDF**, milestone 3's exit, which the census
printed and did not hold until then; running-head and footnote precision are
scored over every page of a file that marks any, not only the pages that
do. **The corpus baseline is owed:** the fetched
corpora were not reachable where this landed, so `INFERRED_FLOORS` is empty
and no `ratchet.json` axis was added; the first nightly run's figures are
the floors.

What was measured, first-party: the content stream agrees with the tree on
every pair of every committed EPUB book (11 439 397 pairs over nine books —
the EPUB writer draws in the order it tags), so the books are in the set
where nothing may move; the headline two-column fixture, drawn line by line
across both columns and tagged column by column, scores 0.7603 for the
stream (2 036 440 of 2 678 455 pairs); and a seeded shuffle of a one-column
page drops the stream from 1 to 0.4139, which is the instrument seeing
disorder (`the_score_sees_a_shuffled_page`).

**Milestone 3 (3 October 2026): columns and block order.** The inference
cuts each line into fragments at internal whitespace of at least a column gap,
cuts the body into elementary intervals at the fragments' edges, and finds
every gap in one pass (three columns are two gaps, not a recursion): a run of
intervals covered by text over at most 40 % of the body's height, at least
`COLUMN_GAP_EMS` wide, with a column of text at least `COLUMN_MIN_WIDTH_EMS`
wide on both sides. A line that runs across a gap without touching it — a
producer drawing both columns' lines at once — is cut in two; a line whose
glyph boxes hold the cut is a spanner, and a run of spanners ends one band
of the page and heads the next (`InferredBlock::section`), so a heading over
two columns reads before them and a paragraph set across the page between two
column sets reads between them. Inside a column, the text device's blocks are
kept as units, ordered by their tops, with the stream's order wherever two
stand level (`ROW_TOLERANCE_EMS`). Columns run right to left when most lines
carry `TextLine::rtl`. Rotated and vertical lines go last as `Unplaced`, and a
page that is mostly either is declined. The reading is still the one
interpretation into the tee, not the recorder the milestone named.

**Three departures from the design, each with its fixture.** (1)
`COLUMN_GAP_EMS` is **0.8**, not 1.5: `css-multicol-1`'s `column-gap: normal`
is 1em and LaTeX's `\columnsep` is 10 pt in a 10 pt document, a justified
column ends exactly at its edge, and at 1.5 em — or at exactly 1 em, where the
gap is met to the last bit of a sum of advances — a justified two-column book
reads as one column, interleaved (`a_two_column_book_reads_down_its_columns`;
both injections fire). (2) `COLUMN_MIN_WIDTH_EMS` (8) is new: labels beside
their values are a gap by width and height, and reading every label before
any value is the one order nobody wants
(`labels_beside_their_values_are_one_column`). (3) A whitespace run that
reaches the outermost edge of the text is a margin, never a gap, so a ragged
right edge is not a near miss (`a_ragged_edge_is_a_margin_and_not_a_gap`).

Measured, first-party (`crates/tinker-pdf/tests/reading_order.rs`, 23
tests): the headline two-column fixture, the spanning fixture, three columns
and right-to-left columns each score 1 against the tree with zero crossings
where the stream scores 0.7603, 0.5068, below 1 and 0.4995; every one-column
fixture, every committed `testdata` page and every page of the nine committed
books moves no character; the two-column EPUB book is found as two columns on
every page set in two, with no crossings; and the same book **redrawn as a
line-by-line producer would draw it** — every line where the layout put it,
drawn down the page by baseline across both columns, tagged with its place in
the book's own tree — scores 0.7573 for the stream and 1.0000 for the
inference over 3 915 671 pairs
(`the_two_column_book_redrawn_across_the_page_reads_in_the_authors_order`).
The census now prints column crossings over the files the stream does not
already read; **the corpus figures — crossings against the baseline, pair
agreement per corpus, and veraPDF's moved count — are owed**, as above.

**Milestone 4 (3 October 2026): running heads, running feet and page
numbers.** A line lying wholly in the top or foot `MARGIN_BAND` (12 % of the
crop box) is a page number when a neighbouring page carries, in the same band
at the same height to within an em, the numeral its own value plus the
offset between the two pages — decimal or canonical roman, with dashes,
bars, brackets and a full stop around it — and a running head or foot when
`RUNNING_REPEATS` (two) neighbours carry its text, digits masked so "Page 3
of 9" recurs, at the same place to within an em. Both are taken out of the
body before columns are looked for, so a head across the page is never a
spanner, and are read first and last with their roles. **The window is the
pages around the page, not the first K of the document**: `RUNNING_WINDOW`
(16) pages, half before and half after, because page 300's running head names
its chapter and is not on pages 1 to 16. `Document::inferred_orders` reads
each page's margins once for a whole range; `Page::inferred_order`,
`Document::inferred_order` and it give the same answer, asserted role for
role. With fewer than two other pages to compare — a one-page document — a
block wholly in a band is `Unplaced` where it stands and
`InferenceWarning::NoCrossPageEvidence` says why; nothing is called a running
head on no evidence. Each neighbour is asked as a sweep along the lines'
heights, the lines within an em held in an ordered set by edge, and not line
against line: the review of this lane found every margin line compared with
every margin line of sixteen neighbours, quadratic in lines a stream draws
for a few bytes each, and a page of sixteen thousand one-glyph lines beside
sixteen neighbours of the same went from 73 seconds unoptimised to under one
(`margin_lines_are_judged_in_bounded_work`, in `reading_order.rs`).

Measured, first-party: a six-page builder book with a verso and a recto head,
a numbered foot and two interleaved columns between them, its furniture
drawn as `/Artifact /Pagination` and its stream drawing the number first and
the head last — every page reads head first, number last and body between
at full pair agreement, and against the producer's artifact marks, read with
the tree hidden, 12 of 12 blocks called furniture were marked and 129 of 129
marked characters were found, where the stream calls nothing furniture. A
same word set at a different place on every page's top band stays body; a
`Page # of #` head and a roman page number are found. **No EPUB fixture
carries this milestone**: the EPUB path draws no running head (no
`css-page-3` margin boxes), so the design's "reftest book with a running
head" is the builder book. The census prints running-head precision and
recall per corpus over the artifacts each producer drew in the margin bands
— by position, because the device seam carries a property list's `/MCID` and
14.9's entries but not an artifact's `/Type`, so `/Pagination` cannot be told
from `/Layout` by name — and **the corpus figure is owed**.

**Milestone 5 (3 October 2026): footnotes.** In the page's last band, the
run of blocks at the foot of each column — and below the columns, across the
page — whose every line is set at most `FOOTNOTE_SIZE_RATIO` (0.9) of the
body's median size is a run of notes when a horizontal rule at least
`SEPARATOR_SHARE` (a third) of the column wide stands between it and the body
above, or when it opens with a raised glyph. A glyph is raised when its origin
stands `RAISE_EMS` (a fifth) of its line's size above the line's baseline,
which the text device's `TextChar::origin` keeps — `Glyph::baseline` joins a
raised glyph to its line, and the risen origin is where the ink is — and the
rules come from the same one interpretation the tables read. A run is cut into
one note at every line that opens with a raised marker; each note's marker is
its leading raised glyphs, or a short numeral or note symbol on its baseline.
Notes are read after the body, before the running feet, in the order of their
reference marks — the raised glyph with the same text in the body, by its
place in the order read — and in the order they stand where no mark was
found. They leave the body before the margin bands are judged, so a note in
the foot band is a note and not an unplaced margin block. The design's
`Capture { structure: true }` recorder is not needed: the tee carries both.

Measured, first-party (`crates/tinker-pdf/tests/reading_order.rs`): a page of
body text with two raised reference marks and two notes under a separator
rule, drawn note 2, body, note 1 and tagged body then `/Note`s, reads every
pair the tree's way (the stream 0.9407); against the producer's `/Note`
elements, two of two blocks called footnotes were notes and 102 of 102 note
characters were found, where the stream finds none; a rule over notes marked
on the baseline, and raised markers without a rule, each make two notes, and
neither makes none; note 2 set above note 1 still reads after it, and with no
marks in the body the notes read in the order they stand; two notes drawn as
one block of the text device's are cut at their markers; and **a rise of zero
everywhere changes nothing** — the same page with nothing raised and no rule
calls nothing a footnote and reads its foot where the columns put it. The
census scores footnote precision against `/Note` per corpus; **that figure is
owed**.

**Milestone 6 (3 October 2026): the surface, and the table handoff.** Tables
first, then order: the page's ruled tables — the sibling design's
`Page::inferred_tables`, over the same one interpretation — claim their
characters before anything else is decided, and each is read as one block
of a role the design did not list, `Role::Table`: its lines a cell at a time,
row by row in the table's direction, in exactly the order of the table's own
permutation. The page says so (`InferenceWarning::TableSuspected { tables }`).
A table's lines are not looked at for columns, running heads or footnotes:
its frame stands over the column finder's coverage as a block of text would
— the whole frame, rules and the white of its cells, because a column holding
a paragraph and a table is not open space — and it is placed in the column
that holds its frame, or across the columns as a spanner where its frame
crosses a gap, heading the band under it. Its own rules are no footnote
separator, and a block of the text device's that runs from a line just above
a table to one just below is cut where the table stands. **Only ruled tables
are handed off**: an aligned table is weaker evidence — nothing the page drew
bounds it — and its lines are read as lines (the labels-beside-values fixture
finds one and keeps its `ColumnsAmbiguous` reading). A page more than half of
whose characters are in tables is declined (`DeclineReason::Table`), as a
page mostly of vertical or rotated lines is: its order is the tables', not a
reading order's. `Role::Caption`, which the design listed and nothing
produced — a caption needs the figure the observer does not record — was
taken out rather than left as a promise; the enum is non-exhaustive, so it
can come back with the inference that produces it. `tpdf text --order
stream|stated|inferred` prints the order asked for, and on standard error
which order each page got and what an inference tolerated; `--order stated`
of an untagged document is refused.

Measured, first-party (`crates/tinker-pdf/tests/reading_order.rs`): a page of
two paragraphs around a ruled table whose first-column cells hold two lines,
drawn baseline by baseline across the page and tagged cell by cell, reads
every pair the tree's way where the stream does not (0.9971 over 96 580
pairs — the paragraphs' pairs dominate; the table is where it is wrong),
with the table one `Table` block equal to the table inference's permutation
and nothing outside it moved, whether the paragraphs stand two lines off the
table or one; the same set in the left column of two reads 1.0000 where the
stream reads 0.8512, the table in column 0; a table across two column sets
reads between them (0.8899 streamed, 1.0000 inferred), a spanner heading the
second band; a page mostly table is declined with nothing moved; a small
source line under a table's bottom rule is body, not a footnote; and a book's
bordered table, which the EPUB writer draws in the order it tags, moves
nothing and is one `Table` block. Every `InferenceWarning` variant is reached
by a fixture — `VerticalWriting` and `Declined { VerticalWriting }` by a
hand-assembled `/Identity-V` page, since the document builder writes no
vertical font. **The roadmap row stays, narrowed**: its exit is the corpus
census, which has not run where this was written; the corpus agreement,
column crossings, veraPDF moved count and running-head and footnote
precision are owed, and no ratchet axis was added for a figure not measured.
