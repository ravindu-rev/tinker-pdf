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
transcript is one interpretation of one page; and a replay. The retained-page
row's exit criterion is a replay of a transcript into a device, and "nothing
replays it yet". Until it does, the inference costs a second interpretation
of the page — `TextDevice` for the lines and `RecordingDevice` for the rest —
which ruling 4 makes identical and which is a cost, not a correctness
question. When the replay lands, one interpretation feeds both.

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
- **A replay of a transcript into a device** — does not exist; the
  retained-page row's deliverable. Two interpretations per page until then.
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

*Filled in as milestones land.* Nothing has landed; the roadmap row is not
scheduled. What the commit that added this document changed is the roadmap
row's evidence sentence and its tagged-file count, and the one sentence in
[features/content-and-text.md](../features/content-and-text.md) that called
today's order geometric.
