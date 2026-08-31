# SVG

When this is done, `crates/tinker-pdf-svg/` reads an SVG 1.1 document — markup,
not just a `d` attribute — into a display list, and `crates/tinker-pdf/src/epub.rs`
draws that list onto a page instead of filling a grey rectangle and calling it a
chapter. `SpineDefect::SvgContentDocument` stops existing, the six SVG spine
items in the fetched corpus become six pages that carry ink, and what still
refuses refuses for a **narrower named reason** than "no SVG renderer": a filter,
a mask, an animation, a `<foreignObject>`, a `<pattern>` used as a paint. The
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
  attribute and a `<style>` element.
- **§13's gradients**: `<linearGradient>`, `<radialGradient>`, `<stop>`,
  `gradientUnits`, `gradientTransform`, `spreadMethod`, and `xlink:href`
  inheritance between paint servers — which is how every Illustrator file
  states a gradient it uses twice.
- **§14.3's clipping**: `<clipPath>` and `clip-path`, as a path a consumer
  intersects with.
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
  produces geometry. `Warning::FilterUnsupported`.
- **Masks** (§14.4). `<mask>`, `mask=`. A mask is a rendered alpha channel,
  which is the same argument. `Warning::MaskUnsupported`.
- **SMIL animation** (§19) and **scripting** (§18). A static rendering is the
  document's initial state, and saying so is the point.
  `Warning::AnimationIgnored`, `Warning::ScriptIgnored`.
- **`<foreignObject>`** (§23). Its content is a different document language;
  reading it here would be a second XHTML reader.
  `Warning::ForeignObjectUnsupported`.
- **`<pattern>` as a paint** (§13.3). A tiling paint server is a form XObject
  and a `/Pattern` colour space, and it has no corpus behind it (ruling 3).
  `Warning::PatternUnsupported`.
- **`<marker>`** (§11.6). Arrowheads on a path's vertices. Thirty-two of them
  are in the fetched corpus, all on paths that also fill, so the drawing is
  there and the decorations are not. `Warning::MarkerUnsupported`.
- **`<image>` inside an SVG that the facade cannot resolve.** The crate carries
  the `href` unresolved — it has no container, no filesystem and no network,
  which is what makes it a leaf. Whether the reference resolves is the
  caller's answer and is named there.
- **`switch`/`requiredFeatures` conditional processing** (§5.8). Every branch
  of a `<switch>` in a book is a language variant, and choosing one is a
  reading-system policy this build does not have.
- **Writing an SVG.** This is a reader.

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
matchers.

This is `Node::Image`'s seam read a second time. The crate carries what the
document said; the caller resolves it against what it has.

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
