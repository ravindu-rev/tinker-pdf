# EPUB layout

`crates/tinker-pdf-layout/` turns a tree of boxes and a page box into pages.
It is a leaf crate: its input is a caller-built `BoxNode` tree carrying an
already-cascaded `ComputedStyle`, and its output is `Page`s of positioned
fragments. It holds no parser, no decoder, no font file and no idea what a
JPEG is. `crates/tinker-pdf/src/epub/` is its caller — it builds the tree
from XHTML and the cascade, and paints the fragments into a PDF — and
`DocumentBuilder::from_html` reaches it through the same passes.

## Scope

- **CSS 2.2's box model**: margins with §8.3.1's three collapsing cases,
  borders, padding, `box-sizing`, `width`/`height` with §10.4's and §10.7's
  min/max constraints, and `position: relative`'s §9.4.3 offset carried to
  paint rather than to flow.
- **§9's formatting contexts**: block and inline, §9.2.2's atomic inline-level
  boxes, and all nine of §9.5's float rules.
- **§3.1's replaced elements**: `Content::Replaced`, sized by §10.3.2 and
  §10.6.2 with §10.4's eleven-row constraint table, emitted as
  `ReplacedFragment`s a caller draws a picture into.
- **§17's table model**: §17.2.1's anonymous-box generation, §17.5.2.2's
  two-pass automatic width, §17.6.2.1's five-rule border conflict resolution,
  `rowspan` clamped.
- **`css-flexbox-1`'s line algorithm**: grow/shrink freeze-and-redistribute,
  `order`.
- **`css-multicol-1`'s columns**, as one `Abreast` shape beside bands and flex
  lines.
- **§13.3's fragmentation**: rules A to D with the specification's own escape,
  `page-break-*`, `orphans`, `widows`, and `Options::paginate` for the flow
  that is one page however tall it comes to — EPUB RS 3.3 §8.1's one page per
  spine itemref.
- **Line breaking by UAX #14** over vendored Unicode 17.0.0, checked against
  every case of Unicode's own `LineBreakTest.txt`.
- **Bounds**: every recursion and every collection has a cap in `Limits` with a
  `Refusal` that names it.

## Non-goals

Each is refused **by name**, with a typed `Warning` and a fixture that reaches
it; [features/epub.md](../features/epub.md) lists them. A layout that quietly
drops one of these looks finished, which is the whole argument for the table.

- **An arbiter.** Ruling 13: no browser, no reference implementation, no
  screenshot comparison. What replaces them is arithmetic — `epub_analytic.rs`
  sets every document in a fixed-pitch face where the line breaker is a
  division and computes every expected number in the test — and
  `epub_reftest.rs`, which compares two spellings of the same document against
  each other rather than against anything outside this repository.
- **`object-fit`, `object-position`.** A replaced box's content fills its
  content box exactly, which is what CSS says happens when the property that
  would say otherwise is absent. An author who states a `width` and a `height`
  that disagree with the picture's proportions gets a stretched picture,
  asserted rather than assumed. [ROADMAP](../ROADMAP.md) row CD-15 would
  change this.
- **A second layout pass.** `::first-line` selects part of an already
  laid-out box; honouring it means laying the box out twice. Parsed, no box,
  named. `::first-letter` needs no second pass: its letter is found while the
  box tree is built. `::first-line`'s second pass is [ROADMAP](../ROADMAP.md)
  row CD-15.
- **`max-height` shorter than its content, on a box that does not clip.** The
  flow is one column whose `y` never goes backwards, so a box already emitted
  cannot be shortened while what it held is still drawn. `MaxHeightAsAuto`
  says so. A box that clips its block axis is the exception: what is past its
  padding box is not drawn, so it leaves the column — laid out and not
  painted, its text still conserved — and the box is the height it says.
  [ROADMAP](../ROADMAP.md) row CD-02 would change this.
- **A tree whose depth is unbounded.** `Builder::block` recurses once per level
  of the document, so the frame of that one function is what the depth cap is
  measured in stack against — which is why its steps are methods rather than
  inline code, each saying so: the whole horizontal half of §10.3 is out of
  it, and `note_overflow` defers its refusal rather than returning it, for the
  room.

## Design

### The tree is the seam, and it carries no bytes

`BoxNode` is `{ style, content, anchor, span }`, and `Content` is `Text`,
`Children` or `Replaced(Intrinsic)`. `Intrinsic` is three `Option<f64>`s — a
width, a height, a ratio — and **nothing else**. What a picture *is* stays with
the caller and is reached through `BoxNode::anchor`, an opaque `u32` this crate
never reads and copies unchanged onto every fragment.

That is ruling 8 rather than a convenience. A `Content::Replaced(Vec<u8>)`
would have put a decoder, a container format and a media type inside a crate
whose whole claim is that it has none. The cost is real and is paid at the
facade: `epub::read::pictures` resolves and reads every `<img>` in a content
document **before** the box tree is built, because a replaced box's size is the
picture's size and the tree is built before anything is laid out.

### Three fields for an intrinsic size, not two

`css-images-3` §4 and CSS 2.2 §10.3.2 distinguish a source with an intrinsic
height and a ratio from one with an intrinsic height and no ratio, and §10.3.2
case 2 is written for exactly the first. A build deriving the ratio from the
dimensions can never reach that case. This build's only producer is a raster,
where `Intrinsic::raster` fills all three from one pair — but the cascade is
written against the general case, so the general case is what the type can
express, and an SVG with a `viewBox` and percentage dimensions is the shape
that will need it.

`Intrinsic::raster` refuses a degenerate side — zero, infinite, `NaN` — and
yields `Intrinsic::NONE` instead of a ratio of zero or of infinity, because
every one of §10.3.2's ratio cases multiplies by it and a size that is `NaN`
is a page nothing can draw.

### §10.4's table is the common path, not the exotic one

§10.4 has two algorithms. For every other box it is *"apply the rules again
with `max-width` as the width, then again with `min-width`"*. For a replaced
box **with an intrinsic ratio and both `width` and `height` auto** it is
instead a table of eleven constraint violations — and the difference is the
whole point of it: clamping the width alone leaves the height at its intrinsic
value and **stretches the picture**. `img { max-width: 100% }` is on almost
every reflowable book's stylesheet, so the table is what a real book takes.

`replaced_size` is one function and not two because §10.3.2 and §10.6.2 are
mutually recursive and the specification writes them that way. Which of the two
recurses depends on which dimension the author stated, so a `used_width` and a
`used_height` calling each other either loop or silently pick a winner. The
clamp re-derives only a width it actually moved, as §10.6.2 case 1 has it: an
intrinsic height beats a height derived through the ratio.

Fixtures in `crates/tinker-pdf-layout/src/tests.rs` hold the table row by
row — both maxima, both minima, each guard, the *h < min-height* row, and
§10.6.2 case 4's `min(w ÷ 2, 150)` — each stating its expected pair as
§10.4's own arithmetic: a 400 × 100 source under `min-height: 400px` is
1600 × 400, and the plausible wrong answer, raise the height and leave the
width alone, is 400 × 400. So the fixture holds the clause to its own words
rather than to this engine's output. The table is held in the crate that
implements it, against arithmetic: the books `tinker-pdf`'s suite opens meet
§10.4 through `max-width`, and a book's stylesheet could stop declaring the
rule.

**The two mixed rows have no arm, and that is a proof.** §10.4's
*(w < min-width) and (h > max-height)* row and its *(w > max-width) and
(h < min-height)* twin cannot change an answer. Reaching the first means
`under_w && over_h` with `over_w` and `under_h` both false, since every
earlier arm needs one of those two; the *w < min-width* row that then answers
reads `min(min-width × h ÷ w, max-height)` for its height, where
`min-width ÷ w > 1` puts the left term above `h` and `h > max-height` puts it
above the right — so the minimum **is** `max-height`, and the pair is
`(min-width, max-height)`, which is the row. The twin is the same argument with
every inequality turned round. A row that cannot change an answer is not a
guard, so neither is written; the proof sits beside the arms that remain, and
`a_minimum_on_one_axis_and_a_maximum_on_the_other_are_both_honoured` asserts
the pair §10.4 says it is.

### The picture is an inset on a record, and one fragment per picture

`BlockRecord::replaced` holds the picture as an inset from the box's own
border-box corner, not as absolute coordinates. A record is already the one
thing carried through every context a box can end up in — a float, a table
band, a flex item, a column, an atomic inline — and every one of those already
moves a record's `x`. An inset moves with the box for free; an absolute `x`
would be five more places to move it and five ways to forget.

`Page::replaced` is a third list beside `Page::boxes` and `Page::runs` because
a fragment and a picture answer different questions about the same box: a
`BoxFragment` is a border box and exists **once per page the box crosses**,
while a picture is drawn once, whole, from the page its box began on. A box cut
across a page boundary with a picture emitted per fragment is the same
photograph printed twice at two different heights. A box of one flow item is
on one page, where the two agree, so the fixture is a padded box: three items,
cut across three pages.

### `Options::paginate`, and why it is a local rather than a branch

EPUB RS 3.3 §8.1 makes a pre-paginated content document *"exactly one page per
spine itemref"*, and §8.1.2 makes the viewport the initial containing block,
**clipping** what falls outside it. Those are two sentences about the same box,
and a caller that expresses the second by paginating at the viewport and
dropping the pages after the first gets a third thing: content that overflows
by a hair is not clipped, it is *moved* to a page that then does not exist.
An inline picture in a viewport sized to the picture overflows by exactly the
strut's descender, CSS 2.2 §10.8.1.

So `paginate` is `false` for such a flow and `fragment::paginate` reads it as
an infinite fragmentainer, a single local. With an infinite height every
comparison in the cutter answers the way an unpaginated flow needs it to:
nothing overflows, no cut is chosen, no band is sliced, every float finishes on
the page it began on. There is one cutter and not two, so a rule added to it
cannot be added to only one of them. §13.3.1's forced break is the single place
the flag itself is read, because a forced break is a cut nothing overflowed
into and an infinite height cannot express it.

### An `<img>` that does not reach the page

The facade classifies an entry **by its magic bytes and never by its name or
its manifest `media-type`**: a `.jpg` that is a PNG is routine, and an
extension is a claim where the first bytes of a file are a fact. A JPEG called
`pic.png` and a BMP called `pic.png` are fixtures
(`a_picture_is_classified_by_its_bytes_and_never_by_its_name`,
`a_misnamed_unsupported_picture_is_named_by_the_format_it_really_is`).

An EPUB `<img>` reaches the page through EPUB 3.3 §3.2's core raster types:
JPEG and PNG pass through, and GIF and WebP are decoded. SVG is a core media
type this build places only as a *spine item*, and because it is XML with no
magic number it lands in `Unknown` — which is where it belongs, since the
classifier that will not read an extension cannot recognise one. Everything
else is a foreign resource a §3.2-conforming book may use only behind a
manifest fallback this build does not follow.

**A refused `<img>` generates no box at all**, and that is HTML §4.8.4.4
deciding it rather than a shortcut: an element is *"expected to be treated as a
replaced element"* **only when the image is available**. An unavailable one is
an ordinary empty inline. The alternative — §10.3.2's 300 by 150 default — puts
a blank rectangle the size of a postcard into a paragraph for a reference that
was merely misspelled, and carrying `alt` into it would put characters on the
page that the spine's markup does not contain, one per refused image, which is
exactly the quantity `epub_conservation.rs` compares. It is asserted as a
byte-for-byte identity against the same book with an empty `<span>` in the
`<img>`'s place (`a_refused_img_generates_no_box_and_costs_no_space`).

The one case where a box **is** laid out with nothing in it is the writer
refusing bytes that already sized a box: by then every line after it is placed
and the box cannot be unmade, so the honest answer is the geometry the page
would have had, plus `ArchiveWarning::ImageNotDrawn` saying the picture is not
in it. Ruling 2 both ways: degrade, and say so.

### Registration happens before the first page begins

`DocumentBuilder::begin_page` snapshots the document's resource set, so an
`/XObject` added after that call is invisible to the page that names it — the
`Do` is written, the reader cannot resolve the name, and the photograph is
silently gone while every word on the page still sets. `epub::svg::Registry`
makes that ordering a type; `register_pictures` is the same sentence one
element along, a pass of its own before the page loop, handing back names
rather than a builder anything could add to. It registers only pictures a
laid-out page actually drew, so an `<img>` under `display: none` costs the
caller's `max_synthesised` nothing.

**Only a render can tell a missing registration from a correct page.** A
content stream naming an `/XObject` looks identical either way, which is why
every claim about a picture reaching paper in this area is measured in pixels.

## Milestones

1. **The replaced box**: `Content::Replaced`, `Intrinsic`, `ReplacedFragment`,
   §10.3.2/§10.6.2/§10.4, the `<img>` facade, `ArchiveWarning::ImageNotDrawn`.
2. **The `background-*` image family**: `background-image`,
   `background-repeat`, `background-position`, `background-size`, with linear
   and radial gradients as axial and radial shadings in the same tiling path.
3. **`list-style-*`, `counter-reset`, `counter-increment`, `counters()`**:
   a scoped counter tree, `quotes` and the four quote keywords with it.
4. **`transform` and `opacity`.**
5. **`overflow`, `clip-path`, `border-radius`, `box-shadow`, `text-shadow`**,
   each a clip or an ink the painter writes, the geometry asserted
   arithmetically and never against a recorded bitmap.
6. **`writing-mode`, `direction`, `unicode-bidi` below the run**: bidi whose
   unit is the visual line rather than the `TextRun`.
7. **`DocumentBuilder::from_html`**: the cascade, the layout engine and the
   painter reachable without an OCF container, held by the EPUB reftests so
   the two callers cannot drift.

## Risks

| Risk | Mitigation |
|---|---|
| **A wrong layout looks like a page.** Unlike a codec, where a misread bit is visible noise, a dropped constraint or a misresolved percentage sets clean text in the wrong place | Every expected number in the tests is arithmetic written beside the assertion or a number the clause fixes |
| **A blank page satisfies every count.** Text conservation, page counts and warning sets are all satisfied by a book that laid everything out and painted nothing | Colour counts on rendered pages, asserted `> 1` per page, on a real producer's book. The number is the renderer's; the claim is the painter's |
| **No oracle, by ruling 13** | Named, and **not closed**. Arithmetic fixtures, reftest pairs between two spellings of one document, and real producers' books. What they cannot reach is whether the whole page is what the author saw |
| `Builder::block`'s frame is the depth cap's unit, so any edit that inlines work into it can overflow a stack the cap was sized against | The helpers carry that reason in their own doc comments, and `a_tree_of_blocks_past_the_depth_cap_is_refused_by_name` is the fixture that finds it every time |
| A picture's bytes are read at parse time to size its box, so a book of large plates costs its pictures in memory before a page exists | The pass-through path: a JPEG is held as its own bytes and a PNG through the reader that decides between passing its `IDAT` through and decoding it. `epub_memory.rs` bounds a synthesised document against the **picture** rather than against the ZIP, which is the stronger claim |
| Scope is large enough that a partial landing is likely | Each milestone ends at a testable claim. A build that lays a property out and refuses the rest by name is a legitimate stopping point; one that draws what nothing checks is not |
