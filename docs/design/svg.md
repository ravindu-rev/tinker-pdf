# SVG

`crates/tinker-pdf-svg/` reads an SVG 1.1 document — markup, not just a `d`
attribute — into a display list, and `crates/tinker-pdf/src/epub/svg.rs` draws
that list onto a page. What it does not draw it refuses for a **named
reason** — a filter, an animation or a script, a `<foreignObject>`, text on a
path — and the refusal rows in [features/epub.md](../features/epub.md) list
them.

## Scope

- **§8.3's path data**: the twenty commands, the arc's endpoint-to-centre
  conversion, exact quadratic raising. Everything becomes a move, a line, a
  cubic or a close.
- **§7's coordinate systems**: the `transform` list, `viewBox`,
  `preserveAspectRatio`, nested viewports, and §4.2's `<length>` grammar with
  §7.10's absolute units — **ninety user units to the inch**, which is SVG's
  number and not CSS's ninety-six.
- **§5's document structure**: `<svg>`, `<g>`, `<defs>`, `<use>`, `<title>`,
  `<desc>`, `<a>` as a container, and `display`/`visibility`.
- **§9's basic shapes**: `<path>`, `<rect>` with `rx`/`ry`, `<circle>`,
  `<ellipse>`, `<line>`, `<polyline>`, `<polygon>` — every one of them an
  `Outline`, through the one path machinery.
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
- **§10's text**, through the shaping path that sets an EPUB's prose:
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
  drawn unfiltered. `Warning::FilterUnsupported`. Filters are
  [ROADMAP](../ROADMAP.md) row CD-10, exact vector mappings first.
- **SMIL animation** (§19) and **scripting** (§18). A static rendering is the
  document's initial state, and saying so is the point.
  `Warning::AnimationIgnored`, `Warning::ScriptIgnored`. Scripting is
  [ROADMAP](../ROADMAP.md) row CD-12; SMIL stays a limit: a page is static.
- **`<foreignObject>`** (§23). Its content is a different document language;
  reading it here would be a second XHTML reader.
  `Warning::ForeignObjectUnsupported`. [ROADMAP](../ROADMAP.md) row CD-11,
  laid out by the facade so this crate stays a leaf.
- **`<textPath>`, `<tref>` and `<altGlyph>`** (§10.13, §10.10, §10.14).
  Text on a path places each glyph by its advance along the curve and turns
  it to the tangent there, and an advance is a font metric this crate does
  not have (ruling 8) — so it would be a second text-layout seam, through the
  caller, beside the one §10.4's lists use. `<tref>` and `<altGlyph>` are SVG
  1.1 features SVG 2 removed. `Warning::TextLayoutUnsupported`; the element's
  content is not drawn. They are [ROADMAP](../ROADMAP.md) row CD-08.
- **A bounding-box mask or clip on a `<tspan>`.** A `mask` or `clip-path` in
  `objectBoundingBox` units on a `<tspan>` resolves, by SVG 2 §11.2, against
  the box of the whole `<text>` — not known while the `<tspan>`'s group is
  built. It is drawn unmasked or unclipped and named
  `Warning::TextBoxUnmeasured` (`what_cannot_be_placed_stays_named`). So is
  any bounding-box effect on text read through `tinker_pdf_svg::read`, which
  has no measurer.
- **`<image>` inside an SVG that the facade cannot resolve.** The crate carries
  the `href` unresolved — it has no container, no filesystem and no network,
  which is what makes it a leaf. Whether the reference resolves is the
  caller's answer and is named there.
- **`switch`/`requiredFeatures` conditional processing** (§5.8). Every branch
  of a `<switch>` in a book is a language variant, and choosing one is a
  reading-system policy this build does not have. [ROADMAP](../ROADMAP.md)
  row CD-09.
- **Writing an SVG.** This is a reader. The writer is the facade's
  (`crates/tinker-pdf/src/svg_out.rs`, `Page::to_svg`), a `Device` held to
  this crate by reading every file it makes back through it; it never emits
  the elements refused above, and names what would have needed one
  ([features/rendering.md](../features/rendering.md)).

## Design

### The boundary is a value, not a trait

`tinker_pdf_svg::read(bytes, viewport, limits) -> Result<Scene, Refusal>`.
A `Scene` is `size`, a `Vec<Node>` in paint order, a `Vec<Warning>`, and the
`font_faces` its `<style>` elements declare.

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
are, and that document is then read back through the interpreter and
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
`css-cascade-5` §6.1 orders them, and `tinker-pdf-css` implements that order,
so the resolution is: presentation attributes are the **lowest** author-level
declarations (SVG 1.1 §6.4 makes them so), then `<style>` rules by selector
specificity and source order, then `style=""`.

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
reader still extracts.

A run under `visibility: hidden` is **laid out and not painted**, which is
§11.5 read with SVG 2's *Controlling visibility*: a hidden element still
affects text layout and counts in a bounding box. The leaf emits it as a
`Node::Text` with `hidden` set, no fill and no stroke; `place_text` moves the
pen past it and opens a chunk where it begins one, the box of its `<text>`
holds its cells, and the writer draws nothing for it — not 9.3.6's invisible
text, which a reader would extract, and not a silhouette in a clip, which
§14.3.5 says a hidden child is not (`a_hidden_run_is_laid_out_and_not_painted`,
`a_hidden_run_is_in_its_texts_box`, `a_hidden_run_in_a_clip_is_no_silhouette`,
`a_hidden_run_moves_the_pen_and_draws_nothing`). A hidden *shape* is left out:
it moves nothing.

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

1. The document: `tinker-pdf-xml` to an element tree, the tree to a `Scene`;
   `<svg>`, `viewBox`, `preserveAspectRatio`, `<g>`, structural attributes.
2. Shapes: `<path>`, `<rect>` (`rx`/`ry`), `<circle>`, `<ellipse>`, `<line>`,
   `<polyline>`, `<polygon>`.
3. Paint: `fill`, `stroke` and their eleven relatives; presentation attribute
   vs `style=""` vs `<style>`.
4. Gradients and clipping.
5. `<use>`, with the bomb refused by name.
6. `<text>` through the facade's shaping path.
7. The spine wiring.

## Dependencies

- `tinker-pdf-xml`, `tinker-pdf-css` and `tinker-pdf-math`, all three declared
  and argued in `crates/tinker-pdf-svg/Cargo.toml`. **No fourth edge.**
- `crates/tinker-pdf/src/epub.rs` and `crates/tinker-pdf/src/epub/svg.rs`.
- `fuzz/fuzz_targets/svg.rs` and `fuzz/corpus/svg/`.
- `xtask`'s `PIXEL_PATHS`, which lists `tinker-pdf-svg`, so the ruling 4 the
  crate's own manifest argues is checked.

## Risks

| Risk | Mitigation |
|---|---|
| **A wrong SVG looks like a picture.** Unlike a codec, where a misread bit is visible noise, a dropped `transform` or a misresolved percentage draws a clean shape in the wrong place | Every expected value in the tests is arithmetic written out beside the assertion or a number the clause fixes, never a recorded output |
| **No oracle, by ruling 13.** Nothing outside this repository may say whether a rendering is right | Accepted and named. What replaces it is that the geometry is checkable *arithmetically* — an arc ends where the command says, a quadratic raised to a cubic passes through the same midpoint, a rotation about a point fixes that point — and those are identities rather than comparisons. What it cannot reach is whether the whole page is what the author saw. **Not closed** |
| Illustrator's `<style>`-element idiom means a build with no CSS is a build that draws every shape black | `tinker-pdf-css` is an edge for exactly this, and `the_three_sources_rank_in_the_order_section_6_4_states` (`crates/tinker-pdf-svg/tests/paint.rs`) holds the three sources against each other |
| `<use>` is a bomb: an expansion referencing an ancestor grows without bound and a recursive walk overflows a stack before any cap fires | Refused structurally — the walk will not enter a subtree it is already inside — with `Refusal::TooManyUses` and a fixture that is the bomb. The depth counter is the second line and not the first |
| An SVG that holds nothing but an `<image>` draws nothing from geometry alone | The facade resolves an SVG `<image>` against the container and embeds it through the same `ImageData` path `cbz.rs` uses; one that does not resolve is named (`ArchiveWarning::SvgImageUnresolved`) |

## As built

`crates/tinker-pdf/src/epub/svg.rs` is the writer. The leaf's tests are in
`crates/tinker-pdf-svg/tests/`, the writer's in
`crates/tinker-pdf/tests/epub_svg.rs`.

**Resources are registered before the page that names them.**
`DocumentBuilder::begin_page` snapshots the resource set, so a pattern, an
`/ExtGState` or an image registered *while drawing* would be named by an
operator no reader could resolve, and the page would draw without it.
`epub::svg::Registry` is that ordering as a type: the content of every group,
text included, is written in `register`, and `draw`, handed a `&Registry` and
no builder, cannot register anything.

**A stroke is as wide as its element's user space says.** A `Node::Path`'s
outline has every transform composed in and its `Stroke::width` none, so
`Stroke` carries the element's matrix, and a stroked path under anything but
the identity is written under it: `q`, that matrix's `cm`, the outline taken
back through its inverse, the painting operator, `Q` — so 8.4.3.2 reads `w`
and `d` in the element's space, anisotropically under a non-uniform matrix, as
§11.4 strokes. A matrix with no inverse has no stroke
(`a_stroke_is_as_wide_as_its_elements_user_space_says`). Text's stroke state
is read under the run's own `cm`.

**§14.5's groups.** `Node::Group` is a list inside the list: a container's
`opacity` and `clip-path`, a `<use>`'s, a nested `<svg>`'s, a `<text>`'s or a
`<tspan>`'s, and a shape's own opacity where it both fills and strokes — so a
group's opacity applies once, to the group, and is not too dark wherever two
of its paints overlap, which a fill and its own stroke always do. A group that
changes nothing is not made, and a group of one node that paints once folds
into that node's alpha. The facade writes a translucent group as an isolated
transparency group form under a constant alpha (11.6.6), and paints the form
with the stream's default space in force — the page mapping undone around the
`Do` and put back as the form's first operator — so that a gradient inside a
form is anchored by the same pattern matrix as one outside it under either
reading of 8.7.3.1. `clip-path` does not inherit, and a container's clip is
its group's (`clip_path_does_not_inherit`; end to end,
`a_clipped_image_is_clipped_in_page_space` in the SVG writer's suite).

**§11.6's markers.** A marker is expanded in the leaf crate, at every vertex —
so the consumer sees paths and groups and nothing marker-shaped, which is the
display-list rule again. Two decisions are worth having in writing. A vertex
is where a **command** ends (`path::parse_commands` reports the boundaries),
because path.rs cuts an arc into quarter-turn cubics and a `marker-mid` at
every cubic would decorate the inside of every arc. And a closed subpath's
first vertex arrives along its closing segment, which is SVG 2's direction
rule — SVG 1.1 is silent, and every renderer reads it this way. A marker's
style is resolved down its own ancestry (§11.6.2), the user agent's
`overflow: hidden` clips it to its viewport, and a marker that reaches itself
is `Refusal::TooManyUses`, the `<use>` bomb in another spelling.

**§13.2.3's `spreadMethod`.** `Paint` carries the method and the facade writes
`reflect` and `repeat` as one 7.10.5 calculator per shading — the parameter
folded into one period (`t − ⌊t⌋`, or its reflection about one) and the stops
as a binary search, so a ramp of a thousand stops nests ten deep and a period
a hair wide costs what a wide one does. The domain is derived rather than
chosen: the visible box taken back into gradient space and projected onto the
axis, or for a radial gradient the smallest circle of the family holding every
corner. A focus on or past the circle is drawn in to 99% of the radius,
because the cone of circles from such a focus never covers what lies behind
it.

**§10.4's per-glyph lists.** Each character takes each of `x`, `y`, `dx` and
`dy` from the innermost `<text>` or `<tspan>` whose list has a number at that
character's place in it (§10.5), so an ancestor's list reaches through a
`<tspan>` that states none; `rotate`'s last number goes on applying. A run is
cut wherever a character moves or turns, so a line with an `x` per character
is a run per character and one with a single `x` is one run. A `<tspan>` with
a `y` and no `x` continues from the pen, §10.5's rule (b) (`continues_x`
carries that to the caller, which has the pen); and a continuing run's `dx`
accumulates until the next absolute `x`, so the text after a nudged word stays
shifted with it.

**§14.4's masks.** `Mask` is a node list and a region, and a masked element is
a group carrying one. The facade writes the content as an 11.6.5.2
`/Luminosity` soft mask — a transparency group form clipped to the region,
whose black backdrop masks everything outside it — under which the masked
group's own form is painted. The `gs` is set after the page mapping is undone,
because 11.6.5.2 places the mask's group in the space in force at the `gs`.
One decision is the luminance: SVG 1.1 asked for linearRGB and CSS Masking 1,
which every reading system follows, for the plain weighted sum in sRGB;
11.6.5.3 derives a luminosity from an RGB group by weights of its own, so
every colour and stop in a mask's content is written as its CSS luminance, a
grey, on which the two cannot differ. What is not converted is a picture
inside a mask, which keeps 11.6.5.3's weights. A shape's mask region is the
shape's own geometry, which leaves out its markers as §7.11 does
(`a_marked_shapes_mask_region_is_its_own_box`); a container's box still
measures the markers its paths drew, because its nodes do not say which those
were.

**§13.3's patterns.** `Paint::Pattern` is a `Tile`: a cell, a matrix and a
node list, so a pattern is the display list again, one level down. The leaf
resolves the tile — `patternUnits` (initially `objectBoundingBox`, a fraction
of the painted element's box), `patternContentUnits`, a `viewBox` fitted into
the cell, `patternTransform` composed inside the element's matrix, and every
attribute and the content inherited along `xlink:href` as a gradient's are —
and walks the content into nodes in pattern space, styled down the pattern's
**own** ancestry (§13.3), so a tile does not take the stroke of the shape it
fills. The facade writes the cell as an 8.7.3 tiling pattern, `/XStep` and
`/YStep` the tile's size and `/BBox` the tile, which is the clip §13.3's
`overflow: hidden` asks for; the cell's stream has no page mapping of its own,
the pattern's `/Matrix` carrying pattern space into the default space as a
gradient's does. A tile with no area paints nothing — not the fallback,
because the server was found — and a pattern that paints itself, directly or
along its chain, is `Refusal::TooManyUses`, the `<use>` bomb's fourth
spelling. Text with no measured box is the exception to the first rule: a
bounding-box tile on it is not a tile of no area but a paint that cannot be
resolved, so the fallback stands, named `TextBoxUnmeasured`, and a
bounding-box gradient on it takes the same answer. Each tile's nodes are
charged against `max_nodes` once per shape it fills, so a pattern painted on a
thousand shapes is a thousand tiles' worth of the budget, as the same `<use>`
a thousand times is. Text a tile or a mask draws has its face registered, as
the page's runs and a group's do
(`text_inside_a_tile_or_a_mask_names_a_font_the_file_has`).

**§14.3.5's `<use>` and `<text>` in a clip.** A `<use>` there must name a
shape or text directly, §14.3.5 says, and one that names a shape is that
shape, placed as §5.6 places it — the `<use>`'s `transform`, its `x` and `y`,
then the shape's own `transform` — joining the outline. A clip is rebuilt for
every element that names it, so its segments are spent from the document's
budget, which a `<use>` in a clip would otherwise multiply for free. Text is
the case a clip cannot hold: a glyph's outline is a font's, which this crate
does not have, and the union of a path and glyphs is not one PDF clip either —
`W` intersects, and 9.3.6's clipping text modes add glyphs only to each other.
So a clip that holds text is a `Mask` with no region of its own: the clip's
shapes as one white outline, under its fill rule, and every run walked as text
styled down the `<clipPath>`'s own ancestry (or the `<use>`'s that names it),
then filled white, opaque and unstroked whatever it was painted with. White
keeps, a mask's black backdrop removes, and the union is what the luminance
is. An element with a mask of its own as well is a group masked by one inside
a group masked by the other. A child the content model refuses — a `<g>`, an
`<image>`, a `<use>` of a group — adds nothing and is `ClipChildIgnored`.

**A `<style>` element's at-rules.** `style::sheet_with` reads them against a
`style::Reach` the caller supplies through `tinker_pdf_svg::read_with`'s
`Context`: `@media` evaluated by `tinker_pdf_css::media::evaluate`, the EPUB
cascade's evaluator, as **print** of the viewport — a book's sheets are
written for the screen it is read on, and its pages answer `screen`, but an
SVG here is set on a page, and a drawing that says `@media print` is saying
what it looks like on one — and the `<style>` element's own `media` attribute
(SVG 2 §6.2) asked the same way of the whole sheet, so `<style media="screen">`
does not apply on paper, as `@media screen { … }` does not; `@import` fetched
through `tinker_pdf_css::ImportResolver`, the trait the EPUB cascade's imports
go through, and read in place before every rule after it (one after a rule is
invalid, §3.3 of `css-cascade-5`), with `MAX_CSS_IMPORT_DEPTH`, the ancestor
chain against cycles, one `Budget` across every imported sheet's tokens, and
`MAX_CSS_BYTES` across their bytes together — an import is spliced into the
sheet that names it, and a comment is no tokens, so a sheet of one comment
imported a million times is what the second bound is for. The `<style>`
element's own text is the document's and is not spent, and once either bound
refuses, nothing more is fetched; and `@font-face` read by
`tinker_pdf_css::font_face::parse_rule` into `Scene::font_faces`, a face's
base its own sheet's address or `None` for the element's, because a face is a
font program and this crate has no vocabulary for one. The facade's
`read_svg` supplies the container as the resolver — a `<style>` element's base
is the document's path, which the leaf does not have — and puts the faces in
the list `typeface::load` loads, beside a chapter's (`tests/at_rules.rs` in the
leaf; `an_svg_reaches_its_container_for_its_imports_and_faces` in
`epub_svg.rs`). An import the resolver lacks is `ImportUnresolved`, and
`AtRuleIgnored` names what is left: `@keyframes`, `@page`, `@layer` and the
rest, or one of the three invalid or past a bound. A `<style>` element is not
rendered (§6.3); the walk dispatches it beside `<title>`.

**Text's box.** A gradient, a pattern, a mask or a clip in `objectBoundingBox`
units is a fraction of §7.11's box, and the box of text is the union of its
glyph cells — each glyph's advance by the font's full ascent and descent, SVG
2 §8.10 says — which are a font's (ruling 8). The leaf takes a measurement
through `read_with`'s `Context::with_measure`: a `MeasureText` hands back a
run's advance, ascent and descent, and the facade's is the `BookMetrics` its
pages are set with — the same `Metrics::measure` its `place_text` and
`draw_text` move the pen by, and the `Metrics::vertical` of the face the run's
request resolves to. The leaf **replays `place_text`** with those numbers — a
run with no position of its own begins where the one before it ended, a
chunk's `text-anchor` moves the whole chunk by its whole width, a cell is
turned by §10.5's `rotate` about the run's origin and carried by the run's
matrix — so the box is where the ink will be, and is only if the caller places
runs by the measurement it gave. A group's box is its shapes' and its runs'
cells together. A run's own paint is different, because SVG 2 §11.2 resolves
every `objectBoundingBox` effect on a `<tspan>` against the box of the
**whole** `<text>`, and that box is known only once the `<text>`'s last run is
placed: a paint server that takes a fraction of the box waits as a mark in the
run's place — a colour no document can state — and is resolved when the
`<text>` ends, against its box carried into each run's space, and the mark
replaced. Only such a paint waits, so a document with no bounding-box effect
on its text reads exactly as it would without a measurer, warnings in the same
order (`fuzz/fuzz_targets/svg.rs` and `hostile_input.rs` assert it of every
input, and that no mark reaches a caller). A `<tspan>`'s own mask or clip
cannot wait the same way — it is a group around the `<tspan>`'s runs, built
before the `<text>` ends — and stays `TextBoxUnmeasured` rather than taking
the `<tspan>`'s own box, which would be a different picture. The facade reads
an SVG before its faces are loaded, since its `@font-face` rules are among
what pass 2 loads, so a scene that named `TextBoxUnmeasured` is read a second
time once they are (`epub.rs`'s `measure_svg`), and the report follows the
second read; every other scene is read once (`tests/text_box.rs` in the leaf;
`a_bounding_box_gradient_on_svg_text_spans_the_text_it_paints`,
`a_bounding_box_mask_on_svg_text_masks_it` and
`the_report_is_the_measured_reads` in `epub_svg.rs`).

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
