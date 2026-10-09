# SVG

When this is done, `crates/tinker-pdf-svg/` reads an SVG 1.1 document — markup,
not just a `d` attribute — into a display list, and `crates/tinker-pdf/src/epub.rs`
draws that list onto a page instead of filling a grey rectangle and calling it a
chapter. `SpineDefect::SvgContentDocument` stops existing, the six SVG spine
items in the fetched corpus become six pages that carry ink, and what still
refuses refuses for a **narrower named reason** than "no SVG renderer": a filter,
a mask, an animation, a `<foreignObject>`, a `<pattern>` used as a paint (the
mask and the pattern have since been drawn; see *As built*). The
refusal row in [features/epub.md](../features/epub.md) is narrowed to those and
kept, because deleting a row whose exit criterion is not green is how a
refusal table stops meaning anything.

## Scope

- **§8.3's path data**, already built: the twenty commands, the arc's
  endpoint-to-centre conversion, exact quadratic raising. Everything becomes a
  move, a line, a cubic or a close.
- **§7's coordinate systems**: the `transform` list, `viewBox`,
  `preserveAspectRatio`, nested viewports, and §4.2's `<length>` grammar with
  §7.10's absolute units — **ninety user units to the inch**, which is SVG's
  number and not CSS's ninety-six.
- **§5's document structure**: `<svg>`, `<g>`, `<defs>`, `<use>`, `<title>`,
  `<desc>`, `<a>` as a container, and `display`/`visibility`.
- **§9's basic shapes**: `<path>`, `<rect>` with `rx`/`ry`, `<circle>`,
  `<ellipse>`, `<line>`, `<polyline>`, `<polygon>` — every one of them an
  `Outline`, through the path machinery that already exists.
- **§11's painting**: `fill`, `stroke`, `fill-rule`, `stroke-width`,
  `stroke-linecap`, `stroke-linejoin`, `stroke-miterlimit`, `stroke-dasharray`,
  `stroke-dashoffset`, `opacity`, `fill-opacity`, `stroke-opacity`, and the
  three ways a property is stated: a presentation attribute, a `style=""`
  attribute and a `<style>` element — whose `@media`, and whose own `media`
  attribute, are asked about paper,
  whose `@import` is fetched through the caller and whose `@font-face` is
  handed to it (see *As built*).
- **§13's gradients**: `<linearGradient>`, `<radialGradient>`, `<stop>`,
  `gradientUnits`, `gradientTransform`, `spreadMethod`, and `xlink:href`
  inheritance between paint servers — which is how every Illustrator file
  states a gradient it uses twice.
- **§14.3's clipping**: `<clipPath>` and `clip-path`, as a path a consumer
  intersects with — and, for a clip that holds text, as a mask (see *As
  built*).
- **§10's text**, through the shaping path that already sets an EPUB's prose:
  `<text>`, `<tspan>`, `x`/`y`/`dx`/`dy`, `text-anchor`, and the font
  properties a run needs.
- **Bounds**: depth, elements and scene nodes, path segments across the whole
  document, `<use>` expansions, and distinct warnings — each a field of
  `Limits` with a `Refusal` that names it.

## Non-goals

Each is refused **by name**, with a typed `Warning` and a test that reaches it.
A picture that quietly drops one of these looks finished.

- **Filters** (§15). `<filter>`, `filter=`, and the whole `fe*` family. A
  filter is a raster pipeline over a rendered subregion, and this crate
  produces geometry. PDF has no filter either, so drawing one would mean
  rasterising the element inside the writer — at a resolution the book does
  not state, with the vector text under it lost to extraction. The element is
  drawn unfiltered. `Warning::FilterUnsupported`.
  *No longer a non-goal, 9 October 2026*: [ROADMAP](../ROADMAP.md) row CD-10, exact
  vector mappings first.
- ~~**Masks** (§14.4)~~ — **drawn since the milestones**; see *As built*. A
  mask is a rendered alpha channel, and PDF has one: 11.6.5.2's soft mask.
- **SMIL animation** (§19) and **scripting** (§18). A static rendering is the
  document's initial state, and saying so is the point.
  `Warning::AnimationIgnored`, `Warning::ScriptIgnored`.
  *Scripting is no longer a non-goal, 9 October 2026*: [ROADMAP](../ROADMAP.md) row
  CD-12. SMIL stays a limit: a page is static.
- **`<foreignObject>`** (§23). Its content is a different document language;
  reading it here would be a second XHTML reader.
  `Warning::ForeignObjectUnsupported`.
  *No longer a non-goal, 9 October 2026*: [ROADMAP](../ROADMAP.md) row CD-11, laid
  out by the facade so this crate stays a leaf.
- **`<textPath>`, `<tref>` and `<altGlyph>`** (§10.13, §10.10, §10.14).
  Text on a path places each glyph by its advance along the curve and turns
  it to the tangent there, and an advance is a font metric this crate does
  not have (ruling 8) — so it would be a second text-layout seam, through the
  caller, beside the one §10.4's lists already use. `<tref>` and `<altGlyph>`
  are SVG 1.1 features SVG 2 removed. `Warning::TextLayoutUnsupported`; the
  element's content is not drawn. **Unscheduled, not decided**: none of the
  three has a file behind it here, and ruling 3 makes that a reason to wait
  for a count rather than a reason to refuse for good, so they stay a row in
  the roadmap and not one of its named non-goals (*corrected on review,
  3 October 2026*); since 9 October 2026 that row is [ROADMAP](../ROADMAP.md) CD-08.
- ~~**`<pattern>` as a paint** (§13.3)~~ — **drawn since the milestones**;
  see *As built*. A tiling paint server is 8.7.3's tiling pattern, cell for
  cell.
- ~~**A bounding-box effect on text**~~ — **measured by the caller since 4
  October 2026**; see *As built*. What is left of it: a `mask` or `clip-path`
  in `objectBoundingBox` units on a `<tspan>`, which SVG 2 §11.2 resolves
  against the box of the whole `<text>` — not known while the `<tspan>`'s
  group is built. It is drawn unmasked or unclipped and named
  `Warning::TextBoxUnmeasured`, as every such effect on text was before
  (`what_cannot_be_placed_stays_named`). So is any effect through
  `tinker_pdf_svg::read`, which has no measurer. *Text whose first run is
  hidden left this list on 9 October 2026*: a hidden run is now laid out (see
  *Text* under *Design*), so it opens its chunk and is in the box.
- ~~**`<marker>`** (§11.6)~~ — **drawn since the milestones**; see *As built*.
  Arrowheads on a path's vertices, thirty-two of them in the fetched corpus,
  all on paths that also fill.
- **`<image>` inside an SVG that the facade cannot resolve.** The crate carries
  the `href` unresolved — it has no container, no filesystem and no network,
  which is what makes it a leaf. Whether the reference resolves is the
  caller's answer and is named there.
- **`switch`/`requiredFeatures` conditional processing** (§5.8). Every branch
  of a `<switch>` in a book is a language variant, and choosing one is a
  reading-system policy this build does not have.
  *No longer a non-goal, 9 October 2026*: [ROADMAP](../ROADMAP.md) row CD-09.
- **Writing an SVG.** This is a reader. The writer is the facade's
  (`crates/tinker-pdf/src/svg_out.rs`, `Page::to_svg`), a `Device` held to
  this crate by reading every file it makes back through it; it never emits
  the elements refused above, and names what would have needed one
  ([features/rendering.md](../features/rendering.md)).

## Design

### The boundary is a value, not a trait

`tinker_pdf_svg::read(bytes, viewport, limits) -> Result<Scene, Refusal>`.
A `Scene` is `size`, a `Vec<Node>` in paint order, and a `Vec<Warning>`.

The obvious alternative is a `Device` trait the crate calls back into, which is
how `tinker-pdf-content` drives the interpreter. It is the wrong shape here and
`lib.rs` says why in its own words: ruling 7's seam exists so that
*interpretation* happens once and consumers differ, and there is exactly one
consumer of an SVG in this repository. A trait would put the facade's vocabulary
into this crate's signatures, which is what ruling 8 forbids, in exchange for a
generality nothing wants.

**Ruling 7 still binds the facade, in the direction it is about.** The scene
does not reach a rasterizer: it is written into a PDF content stream through
`tinker_pdf_cos::build::PageBuilder`, exactly as an EPUB's text and boxes
already are, and that document is then read back through the interpreter and
the `Device` seam like any other. Nothing reaches around the interpreter,
because nothing here is interpreting a content stream.

### Document order is paint order, and transforms are composed once

§3.3 paints elements in document order and there is no `z-index`. So the walk is
depth-first and pushes as it goes, and every point in a finished `Scene` is
already in the scene's own space — `viewBox`, every ancestor's `transform` and
every nested viewport multiplied in. A tree of nodes each carrying a matrix
would move that multiplication to the consumer, and this repository would then
hold two of them: this crate's for its own tests, and the facade's for the page.

### Property resolution: three sources, one specificity

An SVG states a property three ways, and all three are live in the corpus —
Illustrator writes a `<style>` element of `.st0 { fill: … }` classes, Inkscape
writes `style=""`, and a hand-written file writes presentation attributes.
`css-cascade-5` §6.1 already orders them, and `tinker-pdf-css` already
implements that order, so the resolution is: presentation attributes are the
**lowest** author-level declarations (SVG 1.1 §6.4 makes them so), then
`<style>` rules by selector specificity and source order, then `style=""`.

What is **not** taken from `tinker-pdf-css` is `ComputedStyle`: its properties
are HTML's and an SVG wants a different dozen. The edge buys the tokenizer, the
selector matcher, `Specificity`, and the `<color>` grammar — so `rebeccapurple`
and `rgb(1 2 3 / 40%)` are read once, in the crate that already knows how.

### Text: the seam is a resolved run, and the facade shapes it

`<text>` is the one part of SVG that needs a font, and ruling 8 forbids this
crate from growing font plumbing. So the leaf crate does everything that is
*about the document* — reads `<text>` and `<tspan>`, resolves `x`/`y`/`dx`/`dy`
into an absolute anchor, composes the matrix, resolves `font-family`,
`font-size`, `font-weight`, `font-style`, `text-anchor` and the fill through the
same property machinery every other element uses — and emits a `Node::Text`
carrying the string, the anchor, the matrix and that resolved style.

The facade does everything that is *about a font*: `epub/paint.rs`'s `choose`
picks a face per character (`css-fonts-4` §5.3), `tinker-pdf-shape` shapes an
embedded one, and `DocumentBuilder::glyph_run` writes it. That is the same path
a paragraph of the book takes, which is the property worth having: SVG text and
XHTML text in one book cannot be set in two different faces by two different
matchers. A run is painted as a shape is — its fill and its stroke each a
colour, a gradient or a pattern, through the same pattern resources — and
9.3.6's rendering modes are SVG's four combinations of the two exactly: a fill
alone is mode 0, a stroke alone 1, both 2, and neither 3, invisible text a
reader still extracts. *Corrected 4 October 2026*: `draw_text` set a run's
solid fill and nothing else, so a run painted with a gradient, a pattern
or `none` drew in black, and a stroke never drew.

A run under `visibility: hidden` is **laid out and not painted**, which is
§11.5 read with SVG 2's *Controlling visibility*: a hidden element still
affects text layout and counts in a bounding box. The leaf emits it as a
`Node::Text` with `hidden` set, no fill and no stroke; `place_text` moves the
pen past it and opens a chunk where it begins one, the box of its `<text>`
holds its cells, and the writer draws nothing for it — not 9.3.6's invisible
text, which a reader would extract, and not a silhouette in a clip, which
§14.3.5 says a hidden child is not. *Corrected 9 October 2026, on the review
of the formats lane*: the run was not emitted at all, so the text after it
was set where the hidden text began, a hidden first run's text at the pen's
zero rather than at its `x`, and a bounding-box paint on such a `<text>` was
`TextBoxUnmeasured` (`a_hidden_run_is_laid_out_and_not_painted`,
`a_hidden_run_is_in_its_texts_box`, `a_hidden_run_in_a_clip_is_no_silhouette`,
`a_hidden_run_moves_the_pen_and_draws_nothing`). A hidden *shape* is still
left out: it moves nothing.

This is `Node::Image`'s seam read a second time. The crate carries what the
document said; the caller resolves it against what it has. Where the document
needs a font's number back — a bounding-box effect on text takes a fraction of
its glyph cells — the caller lends it one through `Context::with_measure`
rather than the leaf learning what a font is (see *As built*).

### Bounds

`Limits` carries `max_depth`, `max_nodes`, `max_segments`, `max_uses` and
`max_warnings`, each with a `Refusal` naming it. `max_uses` is the one that is
not obvious: a `<use>` that references an ancestor is the classic SVG bomb, and
it is refused **structurally** — the walk refuses to enter a subtree it is
already inside — rather than by a depth counter that happens to trip.
`max_warnings` exists because warnings are deduplicated by value, and a document
of half a million distinct unknown element names would otherwise choose how much
memory its own diagnostics cost.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
|---|---|---|---|
| 1 | The document: `tinker-pdf-xml` to an element tree, the tree to a `Scene`; `<svg>`, `viewBox`, `preserveAspectRatio`, `<g>`, structural attributes | The SVG 1.1 doctype reads and an internal subset is `Refusal::Unreadable`; a non-`<svg>` root is `Refusal::NotAnSvg`; §7.10's ninety-to-the-inch asserted by number; every named non-goal is a `Warning` naming itself, asserted as a set **with its length**; depth, node and warning caps each fire by name; the fuzz target drives `read` and asserts finiteness, determinism, deduplication and the caps; counted injection table in the test module | M |
| 2 | Shapes: `<path>`, `<rect>` (`rx`/`ry`), `<circle>`, `<ellipse>`, `<line>`, `<polyline>`, `<polygon>` | Every shape is an `Outline` through `path::parse` or the same `Segment` vocabulary — no second geometry type; §9.2's `rx`/`ry` auto-and-clamp rules asserted by number; a nested viewport's *scale* asserted, which is milestone 1's zero-injection row closing; `max_segments` fires by name | M |
| 3 | Paint: `fill`, `stroke` and their eleven relatives; presentation attribute vs `style=""` vs `<style>` | §6.4's precedence asserted in all three directions on one element; specificity comes from `tinker-pdf-css` and a two-class selector beats a one-class one; `currentColor` and inheritance; an unreadable value is `ValueUnreadable` and the inherited value stands | L |
| 4 | Gradients and clipping | A two-stop `objectBoundingBox` gradient resolves to the right two points in user space; `spreadMethod`; `xlink:href` stop inheritance between paint servers; `<clipPath>` becomes an `Outline` and `ClipPathUnsupported` leaves the warning list | L |
| 5 | `<use>`, with the bomb refused by name | A `<use>` of an ancestor is `Refusal::TooManyUses` rather than a stack overflow or a depth cap; `max_uses` fires; a `<use>` naming nothing is `Warning::UseUnresolved`; `<use>` of an `<svg>` or a `<symbol>` takes its `width`/`height` | M |
| 6 | `<text>` through the facade's shaping path | The leaf crate emits `Node::Text` with a resolved style and no font vocabulary in its signature; the facade draws it with the same `choose`/`face_runs` a paragraph uses; `text-anchor` asserted in all three values | M |
| 7 | The spine wiring | `SpineDefect::SvgContentDocument` deleted; the six SVG spine items in the fetched corpus draw, with the banner reading `RAN`; `docs/features/epub.md`'s row **narrowed, not deleted**, to the non-goals above; the Tier 4 EPUB entry narrowed in the same commit; `determinism.rs` green | L |

## Dependencies

- `tinker-pdf-xml`, `tinker-pdf-css` and `tinker-pdf-math`, all three already
  declared and argued in `crates/tinker-pdf-svg/Cargo.toml`. **No fourth edge.**
- `crates/tinker-pdf/src/epub.rs` and a new `crates/tinker-pdf/src/epub/svg.rs`,
  in milestone 7 and nowhere earlier.
- `fuzz/fuzz_targets/svg.rs` and `fuzz/corpus/svg/`, extended at every milestone
  that grows the surface.
- `xtask`'s `PIXEL_PATHS`, which gains `tinker-pdf-svg` at milestone 1 — the
  crate's own manifest already argued ruling 4 and nothing was checking it.

## Risks

| Risk | Mitigation |
|---|---|
| **A wrong SVG looks like a picture.** Unlike a codec, where a misread bit is visible noise, a dropped `transform` or a misresolved percentage draws a clean shape in the wrong place | Every expected value in the tests is arithmetic written out beside the assertion or a number the clause fixes, never a recorded output. The counted-injection table per milestone is the second half: a check that catches nothing when its defect returns is recorded as catching nothing |
| **No oracle, by ruling 13.** Nothing outside this repository may say whether a rendering is right | Accepted and named. What replaces it is that the geometry is checkable *arithmetically* — an arc ends where the command says, a quadratic raised to a cubic passes through the same midpoint, a rotation about a point fixes that point — and those are identities rather than comparisons. What it cannot reach is whether the whole page is what the author saw. **Not closed** |
| Illustrator's `<style>`-element idiom means a build with no CSS is a build that draws every shape black | `tinker-pdf-css` is an edge for exactly this, and `cover.svg` in the fetched corpus is fifty-eight `.stN` classes. Milestone 3's exit criterion is the three sources in all three directions |
| `<use>` is a bomb: an expansion referencing an ancestor grows without bound and a recursive walk overflows a stack before any cap fires | Refused structurally — the walk will not enter a subtree it is already inside — with `Refusal::TooManyUses` and a fixture that is the bomb. The depth counter is the second line and not the first |
| Three of the six corpus SVGs contain **nothing but an `<image>`**, so "six pages that draw" is not reachable from geometry alone | Milestone 7 resolves an SVG `<image>` against the container and embeds it through the same `ImageData` path `cbz.rs` uses. If that does not land, three pages draw nothing and the refusal row says `<image>` by name rather than being deleted |
| Scope is large enough that a partial landing is likely | Milestones are commit boundaries and each ends at a testable claim. A build that reads geometry and refuses paint by name is a legitimate stopping point; one that draws shapes nothing checks is not |

## As built

All seven milestones landed. `crates/tinker-pdf-svg/` is 4 900 lines over nine
files, `crates/tinker-pdf/src/epub/svg.rs` is the writer, and
`sample-svg-in-spine.epub`'s six spine items are six pages that draw.

**What the counted-injection matrices found that reading did not.** Eight
tables, 98 injections, and eleven of them fired **zero** the first time —
every one a hole in a fixture rather than in the code:

- §7.9's nested-viewport rebasing had no coordinates to move at milestone 1;
  `tests/shapes.rs` closed it.
- `Walk::push`'s scene-node cap could not fire until a `<use>` made a scene hold
  more nodes than the document has elements; `tests/reuse.rs` closed it.
- Every gradient in the fixture stated its own geometry, so the `xlink:href`
  chain walk for *attributes* was never exercised.
- Every `<clipPath>` sat inside a `<defs>`, so "a clipPath draws nothing where
  it stands" was being proved by `<defs>`.
- The `xlink:href` cycle guard was **two rules** — a `contains` check and a
  length cap — and removing the first fired nothing. It is one rule now.
- Both `UseUnresolved` arms were asserted together, so either could go silent.
- The only multi-word `font-family` in the text fixture was a *quoted* one,
  which is a single token.
- No `<tspan>` carried a `transform`, so composing one was proved by the
  `<text>` above it.
- The page-fit test had only a scene *wider* than the page, where both scales
  agree; the image-fit test sampled a row above the scene entirely, which
  nothing can ever ink; and `epub_svg.rs` had **no gradient test at all**, in a
  file about the format whose corpus is almost nothing but gradients. That last
  one needed a second correction: a *horizontal* gradient cannot see a y-flip,
  because the flip touches one coordinate.

**Two defects the tests found in the writer, both of which drew a plausible
page.** `DocumentBuilder::begin_page` snapshots the resource set, so patterns,
`/ExtGState`s and images registered *while drawing* were named by operators no
reader could resolve — every gradient and every transparency silently gone,
with the corpus test still green because that cover strokes its paths black.
`epub::svg::Registry` is now the ordering as a type. And `place_text` added the
running pen *and* a per-chunk offset, setting the second run of a chunk two
words along.

*Corrected 9 October 2026, on the review of the formats lane*: **a stroke is
as wide as its element's user space says.** A `Node::Path`'s outline has every
transform composed in and its `Stroke::width` none, and the writer set that
width under the page mapping alone — so `stroke-width="2"` inside `scale(3)`
drew two units wide rather than six, dashes likewise, and a root `viewBox`
mapping twenty units onto two hundred drew every stroke a tenth of its width.
`Stroke` now carries the element's matrix, and a stroked path under anything
but the identity is written under it: `q`, that matrix's `cm`, the outline
taken back through its inverse, the painting operator, `Q` — so 8.4.3.2 reads
`w` and `d` in the element's space, anisotropically under a non-uniform
matrix, as §11.4 strokes. A matrix with no inverse has no stroke
(`a_stroke_is_as_wide_as_its_elements_user_space_says`). Text needed nothing:
its stroke state is already read under the run's own `cm`.

**What is refused is what this document said it would be**, less one addition:
`<marker>` earned a warning of its own at milestone 1, because thirty-two of
them are in the fetched corpus and `ElementUnknown` would have called a real
SVG element a foreign vocabulary. The narrowed rows are in
[features/epub.md](../features/epub.md).

**After the milestones: §14.5's groups.** The scene was a flat list and
`opacity` was a product multiplied into every descendant — exact for one
shape painted once, too dark wherever two paints of one group overlapped,
which a fill and its own stroke always do. `Node::Group` is a list inside the
list: a container's `opacity` and `clip-path`, a `<use>`'s, a nested
`<svg>`'s, a `<text>`'s or a `<tspan>`'s, and a shape's own opacity where it
both fills and strokes. A group that changes nothing is not made, and a group
of one node that paints once folds into that node's alpha. The facade writes a
translucent group as an isolated transparency group form under a constant
alpha (11.6.6), and paints the form with the stream's default space in force —
the page mapping undone around the `Do` and put back as the form's first
operator — so that a gradient inside a form is anchored by the same pattern
matrix as one outside it under either reading of 8.7.3.1. The same change
found that a `clip-path` on a `<g>` had been **dropped without a word** since
milestone 4: `clip-path` does not inherit, and the container was not a shape,
so the clip went nowhere and `clip_path_does_not_inherit` asserted only the
half that held. The writer's own suite had recorded the symptom as a reader
limit (`a_clipped_image_is_clipped_in_page_space`); it asserts the clip now.

**After the milestones: §11.6's markers**, the one refusal with a corpus
count. A marker is expanded in the leaf crate, at every vertex — so the
consumer sees paths and groups and nothing marker-shaped, which is the
display-list rule again. Two decisions are worth having in writing. A vertex
is where a **command** ends (`path::parse_commands` reports the boundaries),
because path.rs cuts an arc into quarter-turn cubics and a `marker-mid` at
every cubic would decorate the inside of every arc. And a closed subpath's
first vertex arrives along its closing segment, which is SVG 2's
direction rule — SVG 1.1 is silent, and every renderer reads it this way. A
marker's style is resolved down its own ancestry (§11.6.2), the user agent's
`overflow: hidden` clips it to its viewport, and a marker that reaches itself
is `Refusal::TooManyUses`, the `<use>` bomb in another spelling. The counted
injection found one hole: the bisector's wrap is two rules, one per turning
direction, and the first fixture turned one way.

**After the milestones: §13.2.3's `spreadMethod`.** `Paint` carries the
method and the facade writes `reflect` and `repeat` as one 7.10.5 calculator
per shading — the parameter folded into one period (`t − ⌊t⌋`, or its
reflection about one) and the stops as a binary search, so a ramp of a
thousand stops nests ten deep and a period a hair wide costs what a wide one
does. The domain is derived rather than chosen: the visible box taken back
into gradient space and projected onto the axis, or for a radial gradient the
smallest circle of the family holding every corner. A focus on or past the
circle is drawn in to 99% of the radius, because the cone of circles from
such a focus never covers what lies behind it. Writing these found a reader
defect a long way from SVG: a shading's one `/Function`, when it was a
stream, was never read and painted as the identity (`shading_functions.rs`).

**After the milestones: §10.4's per-glyph lists.** Each character takes
each of `x`, `y`, `dx` and `dy` from the innermost `<text>` or `<tspan>` whose
list has a number at that character's place in it (§10.5), so an ancestor's
list reaches through a `<tspan>` that states none; `rotate`'s last number
goes on applying. A run is cut wherever a character moves or turns, so a line
with an `x` per character is a run per character and one with a single `x`
is one run. Two things were wrong before the lists rather than merely absent,
and their tests said the wrong thing: a `<tspan>` with a `y` and no `x` was
put back at the previous chunk's `x`, where §10.5's rule (b) continues from
the pen (`continues_x` carries that to the caller, which has the pen); and a
continuing run's `dx` shifted that run alone, so the text after a nudged word
slid back under it — the shift now accumulates until the next absolute `x`.
What is still the caller's is the advance: a chunk's width for `text-anchor`
does not include the `dx`s inside it.

**After the milestones: §14.4's masks.** `Mask` is a node list and a region,
and a masked element is a group carrying one. The facade writes the content as
an 11.6.5.2 `/Luminosity` soft mask — a transparency group form clipped to
the region, whose black backdrop masks everything outside it — under which
the masked group's own form is painted. The `gs` is set after the page
mapping is undone, because 11.6.5.2 places the mask's group in the space in
force at the `gs`. One decision is the luminance: SVG 1.1 asked for linearRGB
and CSS Masking 1, which every reading system follows, for the plain weighted
sum in sRGB; 11.6.5.3 derives a luminosity from an RGB group by weights of its
own, so every colour and stop in a mask's content is written as its CSS
luminance, a grey, on which the two cannot differ. What is not converted is a
picture inside a mask, which keeps 11.6.5.3's weights. Writing masks found a
renderer defect a long way from SVG — a group under a soft mask was masked
twice, through the form's `/BBox` clip and again at its composite
(`soft_mask_clips.rs`) — and that an `<image>`'s `clip-path`, like a `<g>`'s
before groups, was dropped without a word. *Corrected on review, 3 October
2026*: two boxes were wrong. A shape's mask was measured over everything its
group drew, so a marked path's region grew around its markers, which §7.11
leaves out; it is the shape's own geometry now
(`a_marked_shapes_mask_region_is_its_own_box`). A container's box still
measures the markers its paths drew, because its nodes do not say which
those were. And text under a bounding-box mask, which has no box this crate
can take, was masked away to nothing without a word — where before masks it
had been drawn and named; it is drawn unmasked and named again
(`Warning::TextBoxUnmeasured`, under *Non-goals*), as is a bounding-box clip
on it.

**After the milestones: §13.3's patterns.** `Paint::Pattern` is a `Tile`: a
cell, a matrix and a node list, so a pattern is the display list again, one
level down. The leaf resolves the tile — `patternUnits` (initially
`objectBoundingBox`, a fraction of the painted element's box),
`patternContentUnits`, a `viewBox` fitted into the cell, `patternTransform`
composed inside the element's matrix, and every attribute and the content
inherited along `xlink:href` as a gradient's are — and walks the content into
nodes in pattern space, styled down the pattern's **own** ancestry (§13.3), so
a tile does not take the stroke of the shape it fills. The facade writes the
cell as an 8.7.3 tiling pattern, `/XStep` and `/YStep` the tile's size and
`/BBox` the tile, which is the clip §13.3's `overflow: hidden` asks for; the
cell's stream has no page mapping of its own, the pattern's `/Matrix` carrying
pattern space into the default space as a gradient's does. A tile with no
area paints nothing — not the fallback, because the server was found — and a
pattern that paints itself, directly or along its chain, is
`Refusal::TooManyUses`, the `<use>` bomb's fourth spelling. Text is the
exception to the first rule (*corrected on review, 3 October 2026*): a run
has no box this crate can measure, so a bounding-box tile on text is not a
tile of no area but a paint that cannot be resolved, and the fallback stands,
named `TextBoxUnmeasured` — it had painted `none` without a word, where
before patterns it drew in the fallback and was named. A gradient on text
takes the same answer, which it had never had. Each tile's nodes
are charged against `max_nodes` once per shape it fills, so a pattern painted
on a thousand shapes is a thousand tiles' worth of the budget, as the same
`<use>` a thousand times is. Text a tile or a mask draws had its face left
unregistered until both were checked for it — the registry noted the page's
runs and a group's, not a mask's or a tile's, so their `Tf` named a font the
file did not hold (`text_inside_a_tile_or_a_mask_names_a_font_the_file_has`).

**After the milestones: §14.3.5's `<use>` and `<text>` in a clip.** A
`<use>` there must name a shape or text directly, §14.3.5 says, and one that
names a shape is that shape, placed as §5.6 places it — the `<use>`'s
`transform`, its `x` and `y`, then the shape's own `transform` — joining the
outline. A clip is rebuilt for every element that names it, so its segments
are now spent from the document's budget, which a `<use>` in a clip would
otherwise multiply for free. Text is the case a clip cannot hold: a glyph's
outline is a font's, which this crate does not have, and the union of a path
and glyphs is not one PDF clip either — `W` intersects, and 9.3.6's clipping
text modes add glyphs only to each other. So a clip that holds text is a
`Mask` with no region of its own: the clip's shapes as one white outline,
under its fill rule, and every run walked as text styled down the
`<clipPath>`'s own ancestry (or the `<use>`'s that names it), then filled
white, opaque and unstroked whatever it was painted with. White keeps, a
mask's black backdrop removes, and the union is what the luminance is. An
element with a mask of its own as well is a group masked by one inside a
group masked by the other. A child the content model refuses — a `<g>`, an
`<image>`, a `<use>` of a group — adds nothing and is `ClipChildIgnored`.
**Both kinds of child had been skipped without a word** until now: the
shape loop passed over anything that was not a shape, and the warning the
refusal table cited for them was raised only for a reference naming no
`<clipPath>` at all.

**After the milestones: a `<style>` element's at-rules.** Until now every
at-rule in a `<style>` element was skipped and named `AtRuleIgnored`, the
three that change what a drawing looks like among them. `style::sheet_with`
reads them now, against a `style::Reach` the caller supplies through
`tinker_pdf_svg::read_with`'s `Context`: `@media` evaluated by
`tinker_pdf_css::media::evaluate`, the EPUB cascade's evaluator, as **print**
of the viewport — a book's sheets are written for the screen it is read on,
and its pages answer `screen`, but an SVG here is set on a page, and a drawing
that says `@media print` is saying what it looks like on one — and the
`<style>` element's own `media` attribute (SVG 2 §6.2) asked the same way of
the whole sheet, so `<style media="screen">` does not apply on paper, as
`@media screen { … }` does not; `@import`
fetched through `tinker_pdf_css::ImportResolver`, the trait the EPUB cascade's
imports go through, and read in place before every rule after it (one after a
rule is invalid, §3.3 of `css-cascade-5`), with `MAX_CSS_IMPORT_DEPTH`, the
ancestor chain against cycles, one `Budget` across every imported sheet's
tokens, and `MAX_CSS_BYTES` across their bytes together — an import is spliced
into the sheet that names it, and a comment is no tokens, so a sheet of one
comment imported a million times is what the second bound is for. The
`<style>` element's own text is the document's and is not spent, so a drawing
that read whole before reads whole now, and once either bound refuses, nothing
more is fetched; and `@font-face` read by
`tinker_pdf_css::font_face::parse_rule` into `Scene::font_faces`, a face's base
its own sheet's address or `None` for the element's, because a face is a font
program and this crate has no vocabulary for one. The facade's `read_svg`
supplies the container as the resolver — a `<style>` element's base is the
document's path, which the leaf does not have — and puts the faces in the list
`typeface::load` loads, beside a chapter's (`tests/at_rules.rs` in the leaf;
`an_svg_reaches_its_container_for_its_imports_and_faces` in `epub_svg.rs`).
An import the resolver lacks is `ImportUnresolved`, and `AtRuleIgnored` now
means what is left: `@keyframes`, `@page`, `@layer` and the rest, or one of
the three invalid or past a bound. Writing this found that the walk named
**every** `<style>` element `ElementUnknown("style")` — a loss reported in
every drawing that styles itself; §6.3 says it is not rendered, and it is
dispatched beside `<title>` now.

**After the milestones: text's box.** A gradient, a pattern, a mask or a clip
in `objectBoundingBox` units is a fraction of §7.11's box, and the box of text
is the union of its glyph cells — each glyph's advance by the font's full
ascent and descent, SVG 2 §8.10 says — which are a font's (ruling 8). Until now
every such effect on text was drawn without it and named `TextBoxUnmeasured`.
The leaf takes a measurement now, through `read_with`'s
`Context::with_measure`: a `MeasureText` hands back a run's advance, ascent and
descent, and the facade's is the `BookMetrics` its pages are set with — the
same `Metrics::measure` its `place_text` and `draw_text` move the pen by, and
the `Metrics::vertical` of the face the run's request resolves to. The leaf
**replays `place_text`** with those numbers — a run with no position of its own
begins where the one before it ended, a chunk's `text-anchor` moves the whole
chunk by its whole width, a cell is turned by §10.5's `rotate` about the run's
origin and carried by the run's matrix — so the box is where the ink will be,
and is only if the caller places runs by the measurement it gave. A group's
box is its shapes' and its runs' cells together. A run's own paint is
different, because SVG 2 §11.2 resolves every `objectBoundingBox` effect on a
`<tspan>` against the box of the **whole** `<text>`, and that box is known only
once the `<text>`'s last run is placed: a paint server that takes a fraction of
the box waits as a mark in the run's place — a colour no document can state —
and is resolved when the `<text>` ends, against its box carried into each
run's space, and the mark replaced. Only such a paint waits, so a document with
no bounding-box effect on its text reads exactly as it did without a
measurer, warnings in the same order (`fuzz/fuzz_targets/svg.rs` and
`hostile_input.rs` assert it of every input, and that no mark reaches a
caller). A `<tspan>`'s own mask or clip cannot wait the same way — it is a
group around the `<tspan>`'s runs, built before the `<text>` ends — and stays
`TextBoxUnmeasured` rather than taking the `<tspan>`'s own box, which would be
a different picture. The facade reads an SVG before its faces are loaded, since
its `@font-face` rules are among what pass 2 loads, so a scene that named
`TextBoxUnmeasured` is read a second time once they are (`epub.rs`'s
`measure_svg`), and the report follows the second read; every other scene is
read once, as before (`tests/text_box.rs` in the leaf;
`a_bounding_box_gradient_on_svg_text_spans_the_text_it_paints`,
`a_bounding_box_mask_on_svg_text_masks_it` and
`the_report_is_the_measured_reads` in `epub_svg.rs`). Writing it found the run
painting `draw_text` had never done — a gradient's or a pattern's fill drawn
in black, a stroke never drawn (see *Text* under *Design*) — which would have
put every gradient this made possible on the page as black.

**What this cannot reach**, stated rather than absorbed: nothing outside this
repository adjudicates a rendering (ruling 13), so every expected value here is
arithmetic from a clause or an identity checkable without the code — an arc ends
where the command says, a quadratic raised to a cubic passes through the same
midpoint, four `M`s of Times-Roman are 4 × 0.889 em. Whether the whole page is
what the author saw is not something this suite asks.

One more limit worth naming: `bundled-fonts` is off by default, so the standard
14 have no outlines in a default build and base-14 text — a chapter's prose as
much as an SVG's label — renders as nothing. Text *placement* is therefore held
against the text matrix in the operators rather than against ink, which is a
stronger claim and not a test of that feature flag.
