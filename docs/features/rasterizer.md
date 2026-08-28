# Rasterizer

`tinker-pdf-raster` is a pure 2D scanline rasterizer with zero PDF knowledge
([ruling 8](../rulings.md)): paths, pixels and plain parameters in, coverage
and pixels out, with no COS type or spec vocabulary in its public API. Its
output is deterministic by construction — fixed-point coverage accumulation,
integer blending, and no platform transcendental on any pixel path — so the
same input produces bit-identical pixels on every supported target
([ruling 4](../rulings.md), [determinism](determinism.md)). Every paint costs
what it covers, not what the page measures, and a long operation stops
mid-sweep when it is cancelled.

## What it does

**Paths and flattening.** A `Path` is a sequence of `Verb`s over `f64`
points: `MoveTo`, `LineTo`, `QuadTo`, `CurveTo`, `Close`. Quadratics are
first class — every TrueType glyph outline is quadratic, and raising them to
cubics would run on the hottest path in the engine for a control point the
curve does not have. The quadratic arm of the flattener takes its step count
from the cubic the curve is exactly equal to, so the same curve breaks into
the same pieces whichever verb describes it, and a typeface that mixes
`glyf` and `CFF` outlines keeps one smoothness character. Flattening
subdivides by a fixed count derived from the control polygon — never by a
floating-point termination test, which is exactly the class of cross-target
divergence ruling 4 exists to forbid. Single-point subpaths are kept, because
stroking draws them as dots under a round or square cap (8.4.3.3). Non-finite
points are dropped at construction, so nothing downstream ever sees a NaN.

**Filling.** `fill` rasterizes a path into a coverage `Mask` under the
non-zero winding (8.5.3.3.2) or even-odd (8.5.3.3.3) rule. Coverage is
accumulated at sixteen sub-scanlines per pixel row with exact horizontal
spans in 1/256-pixel fixed point, entirely in integer arithmetic — the
choice ruling 4 pays for over analytic exact-area coverage, and the
difference stays below one 8-bit level at every edge angle. The sweep keeps
an active-edge list and visits only the rows a shape reaches, so a glyph-tall
fill on a page-tall region does the shape's work, not the paper's.

**Stroking.** `stroke` expands a path under a `StrokeStyle` pen into an
outline filled with the non-zero rule, so a stroked edge anti-aliases
identically to a filled one. Caps are butt, round and square (8.4.3.3,
Table 54); joins are miter with the 8.4.3.5 limit, round and bevel (8.4.3.4,
Table 55); dashes follow 8.4.3.6, with an empty or zero-sum array meaning a
solid line, and a width of zero drawing the thinnest device line — one
pixel (8.4.3.2).

**Clipping.** A clip is a `Mask` like any other, and a clip stack composes by
multiplication: `Mask::intersect_in_place` multiplies coverages, which is
what keeps a clipped anti-aliased edge looking right rather than doubly
hard. `Mask::uniform` carries a soft mask's value outside the region its
group covered, where the answer is the luminosity of `/BC` alone (11.6.5.2)
— which is not zero unless `/BC` is black.

**Compositing.** A `Canvas` stores pixels in one of five formats (`Gray8`,
`GrayA8`, `Rgb8`, `Rgba8`, `CmykA8`) and composites in integer arithmetic
throughout. `CmykA8` is the one **subtractive** format — its components are
quantities of ink — and it exists for transparency groups that declare
`/DeviceCMYK`, whose blends the specification says happen over ink rather than
over light ([rendering](rendering.md)). It is not a format a page comes back
in. The two device relations of 8.6.4.4 live here rather than in
`tinker-pdf-color`, for the reason `Color::luma`'s coefficients do — a
rasterizer turns stored components into light, and a leaf takes bytes and plain
values in; a test holds the copy to the original so the two cannot drift. The
inverse takes maximum undercolour removal, which makes it **exact**: every one
of the sixteen million colours survives the round trip.
`fill_mask_with` blends a colour through a mask under any of the sixteen
`BlendMode`s — the twelve separable modes of 11.3.5.2 and the four
non-separable ones of 11.3.5.3 — with the whole operation scaled by an
alpha, which is what the graphics state's `ca` and `CA` do (8.6.4.4).
`Canvas::composite` blits one canvas onto another (11.3.6), bounded by the
source's rectangle, and a canvas can carry the initial backdrop of a
non-isolated transparency group (11.4.4) so that 11.4.7.2's removal step has
the alpha it needs.

**Images.** `draw_image` maps every destination pixel backwards through the
inverse transform into the samples — a forward map leaves seams and
double-writes. Three things decide what a pixel gets: how much of it the image
covers, which samples it reads, and whether it is composited alone.

*Coverage.* The image occupies the unit square of its transform (8.9.5.2), and
that square is rasterised through the same `fill` every path uses — same
sixteen sub-scanlines a row, same exact spans in 1/256 of a pixel, same
cancellation. So an image edge anti-aliases exactly as a filled edge does, and
a pixel the image half covers takes half the paint. The quad's corners are
snapped to that 1/256 grid first, because a rectangle placed by a translation
lands on a grid line far more often than not and `a + (e - t)` need not equal
`(a + e) - t` to the last bit; without the snap a cropped page and the page
under it disagreed on 240 pixels.

*Sampling.* Decided per draw by `sampling_for` from the ratio alone, with
`/Interpolate` (Table 89) the only thing the caller supplies. Above 1:1 the
file gets what it asked for — `/Interpolate true` smooths four taps
(`Bilinear`), false keeps hard samples — and below it, the average of every
sample the pixel covers, weighted by how much of it the pixel covers —
integrated over the destination pixel's **true source rectangle**, which is
what makes a render at twice the scale, box-filtered down, the same picture.

Past 128:1 that gives way to exact 2x2 box-filter averaging into a `Pyramid` of
halved levels until the residual is within 4:1, which bounds the cost of the
one case exact integration does not: the same image drawn many times at
extreme minification, where the levels are built once and reused. The
threshold is measured — the worst downscale any draw in the pdfjs corpus asks
for is 72:1 — and the trade it makes is written up in
[design/image-edges.md](../design/image-edges.md), because a pyramid is
quantised to powers of two and therefore *scale-dependent*, which is what the
`dpi` metamorphic relation had been reporting on strip-built scans.

Hard samples are `Area` over the pixel's own footprint rather than one nearest
tap, and above 1:1 that footprint is *smaller than a sample*: a pixel inside a
sample reads that sample and nothing else, and only a pixel straddling two
mixes them. That is the sample's edge anti-aliased, not the image blurred —
`/Interpolate false` still gets its hard pixels, and a 1:1 blit is still
byte-preserving. A nearest tap instead quantised every internal sample boundary
to whole device pixels, which is the same defect as the outer edge one level
down: a render at twice the scale resolves those boundaries twice as finely, so
box-filtering it back down could not reproduce the coarser render. Measured on
a 512-square source, share of pixels disagreeing between a render and the
box-filtered render at twice the scale: **49.9 % before, 0.00 % after**, at
every ratio tried.

*Compositing, and the artefact anti-aliasing creates.* Two images that share an
edge each cover the pixels along it partly, and compositing them one after the
other lets the page through in between — a half-and-half boundary keeps a
quarter of the backdrop, because source-over of two half-covered draws is not
the average of them. That is conflation, and it is what every scan assembled
from strips is made of. So a *run* of consecutive image draws accumulates in
`fragments.rs` first — coverage adding rather than compositing, premultiplied,
four bytes a pixel, integers throughout — and reaches the canvas once. Two
strips each covering half a boundary pixel accumulate to one whole pixel of
their average, which is what a render at twice the scale box-filters down to.

Fragments are *added*, so a run holds only draws that do not overlap: a picture
laid over another ends the run and composites, and so does any change of alpha,
blend mode, clip or soft mask, and any other drawing operation. A run survives
`q`, `Q` and a form boundary, because none of those paints — which matters,
since real content brackets every image in `q`/`Q` and a run that ended there
would never hold two of anything. Past `MAX_IMAGE_RUN_PIXELS`, a quarter of
`MAX_PAGE_PIXELS`, images composite one at a time as before and abutting ones
conflate again; that is bounded memory rather than a page that will not render.
A knockout group composites image by image too, because 11.4.5 gives every
element its own shape.

*Determinism, throughout.* The level count branches on 16.16 fixed-point
integers, because `log2` is a transcendental and a count one different is not a
rounding difference — it is a different image. The averaging weights are
integers for the same reason: an area is a product of two overlaps, and in
floats the accumulation order would decide the last bit. So is the run's
accumulation, and so is the coverage the quad contributes. The only floats left
on the path are `sqrt`, division and `round`, which IEEE 754 pins exactly
(ruling 4). The pyramid belongs to the caller, so an image's lifetime is
decided where it is known. A stencil's PDF name stayed behind as
`ImageDraw::tint` (8.9.6.2): the image says where, the caller says what.

The trade is written down in [design/image-edges.md](../design/image-edges.md).

**Meshes.** `draw_mesh` rasterizes a whole Gouraud-shaded triangle mesh into
one `MeshBuffer` — coverage from a single non-zero fill over every triangle,
colour from a barycentric walk that interpolates the vertex inputs and only
then asks the caller's closure what colour they are. One buffer rather than
one fill per triangle is what removes the lattice of pale seams that
per-triangle compositing paints along every shared edge.

**Cost and cancellation.** A paint costs what it covers: the caller asks for
a region per shape clipped to the clip's rectangle, every consumer walks the
mask's rectangle rather than the canvas, and the fill sweeps only the rows a
shape reaches. Measured on 600 glyphs on US Letter at 300 dpi: 5 997.7 ms to
18.8 ms (one-time measurement, August 2026). Long operations take a `stop`
predicate — the render layer's `CancelToken` — asked every sixteenth row of
a fill or composite and every 1 024 steps of a dash expansion, and what
comes back on a stop is the partial result, because a half-drawn shape is a
better progressive frame than a missing one. The predicate decides only
whether the sweep continues, never what a continued row computes, so the
check interval cannot change a pixel.

## API

Most hosts reach the rasterizer through the facade: `Page::render` takes a
`RenderOptions` — scale, `PixelFormat`, an optional `CancelToken`, whether
to draw annotations — and returns a `Bitmap`. The crate itself is a leaf,
usable directly with no PDF anywhere near it:

```rust
use tinker_pdf_raster::{fill, Canvas, Color, FillRule, Path, PixelFormat};

let mut path = Path::new();
path.move_to(10.0, 10.0);
path.quad_to(50.0, 90.0, 90.0, 10.0);
path.close();

// A mask over only the region the shape can touch, then one composite.
let mask = fill(&path, FillRule::NonZero, 0, 0, 100, 100, 0.1, None);
let mut canvas = Canvas::new(100, 100, PixelFormat::Rgb8, Color::WHITE);
canvas.fill_mask(&mask, Color::BLACK, 1.0);
```

The crate root re-exports the working set: `Path`, `Verb`, `Point`,
`FillRule`, `flatten`; `fill`, `Mask`; `stroke`, `StrokeStyle`, `LineCap`,
`LineJoin`; `Canvas`, `Color`, `PixelFormat`, `MaskKind`; `BlendMode` (in
`blend`); `draw_image`, `ImageDraw`, `ImageSource`, `Transform`, `Filter`,
`Sampling`, `Pyramid`; `draw_mesh`, `MeshDraw`, `MeshBuffer`. The bridge
from PDF vocabulary — `/BM` names to `BlendMode`, content-stream operators
to paths — lives one layer up, in [rendering](rendering.md).

## Refused by name

| What | Typed variant / named bound | Why (one line) | See |
| --- | --- | --- | --- |
| A path past 1 048 576 verbs | `MAX_VERBS` (`geom.rs`); further verbs are dropped | A content stream can ask for unbounded geometry; a real page never does | [ruling 1](../rulings.md) |
| A verb with a non-finite point | dropped at `Path` construction | A NaN cannot be rasterized, and dropping the verb keeps the rest of the path usable | [ruling 1](../rulings.md) |
| A mesh costing more than 67 108 864 units of work | `MAX_MESH_WORK` (`mesh.rs`); `draw_mesh` returns `None`, reported as `RenderWarning::UnsupportedShading` | Half a mesh reads as an artefact where nothing reads as the gap it is | [rendering](rendering.md) |
| An image under a singular transform | `Transform::invert` returns `None`; the draw paints nothing | A determinant near zero maps the image to a sliver with no useful bits left | `image.rs` |
| A cancelled render | `RenderWarning::Cancelled` | The sweep stops at the next scanline band, keeping the rows already swept | [rendering](rendering.md) |

A dash expansion is additionally capped at 100 000 steps per segment, so a
tiny pattern over a long line cannot generate millions of pieces.

## Verified

- **95 unit tests inside the crate** (August 2026): `geom.rs` proves a
  quadratic flattens point-for-point like its exact cubic equivalent across
  seven curve shapes and five tolerances, and that flattening is
  deterministic; `fill.rs` covers both fill rules, mask algebra and the
  partial-mask stop; `stroke.rs` covers caps, joins, the miter limit and
  dash phase; `blend.rs` pins all sixteen modes in integers; `image.rs`
  measures the sampling policy against numbers — a magnified gradient's
  largest step, a 2:1 checkerboard averaging to uniform 128, and an image
  wholly off the canvas costing nothing; `mesh.rs`
  proves shared edges leave no seam; `canvas.rs` covers formats, backdrops
  and bounded compositing.
- **Determinism fingerprints** (`crates/tinker-pdf/tests/determinism.rs`):
  of the 15 committed render fingerprints, `text`, `curves`, `shading`,
  `blend`, `pattern`, `image`, `transparency`, `tiling` and `mesh` exercise
  this crate directly, each a pixel hash plus dimensions and an ink floor,
  reproduced byte-for-byte on three measured targets
  ([determinism](determinism.md)).
- **Analytic sampling and coverage**
  (`tinker-pdf-raster/tests/analytic_sampling.rs`): every expectation computed
  in the test from the geometry. Minification against an independent box
  average at six ratios; a half-covered edge against its area on either axis at
  all fifteen sixteenths, and a corner against the product of its two
  fractions; a 1:1 integer-aligned draw asserted byte-preserving so a coverage
  off-by-one would show as a rim; and the conflation pair — two abutting black
  strips leaving **63** composited one at a time and **0** as one run, which
  states the defect and the fix in the same test.
- **Integration tests** through the facade: `blend_modes.rs`,
  `stroke_parameters.rs`, `images.rs`, `inline_images.rs`,
  `mesh_shadings.rs` and `transparency_groups.rs` under
  `crates/tinker-pdf/tests/`.
- **Fuzzing**: the `render_page` target (one of the 24) drives whole
  documents through the interpreter into this rasterizer under ruling 1;
  the corpus run renders every page of 4 484 of 4 525 files with 0 crashes
  ([verification](../verification.md)).
- **Cost regression**: `a_small_fill_on_a_large_page_stays_small` (in
  `crates/tinker-pdf-render/src/lib.rs`) asserts on mask pixels *asked for*
  rather than pixels painted, so a paint that goes back to costing the whole
  canvas fails it while passing everything else.
