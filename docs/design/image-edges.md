# Image edges, sampling and conflation

An image draw meets the device pixel grid in three places: the outer edge of
the image's rectangle, every boundary between two of its samples, and the edge
it shares with an abutting image. None of the three is quantised to whole
device pixels, and this document describes how each is built.

## The three edges

**1. The outer edge.** Painting a destination pixel in full or not at all, by
whether the image's transformed unit square contains its centre, quantises the
edge to whole device pixels. Every filled path is anti-aliased, and an image
edge is too.

**2. Every internal sample boundary.** Above 1:1, one nearest tap per pixel
assigns each sample's footprint to whole device pixels: the same quantisation
as the outer edge, one level down, at as many boundaries as there are samples.
A render then disagrees with the box-filtered render at twice the scale.
Minification by area is scale-coherent.

**3. Conflation, which anti-aliasing an image edge creates.** Two images
sharing an edge each cover the boundary pixel partly, and compositing them one
after the other lets the page through between them: a half-and-half boundary
keeps a quarter of the backdrop. A quantised edge has no seam, because the
pixel belongs wholly to whichever strip holds its centre, so partial coverage
on its own makes a page built of abutting strips worse. A PCLM page is
hundreds of them.

## Scope

- Geometric coverage for the image's own rectangle, from the crate's own path
  rasterizer rather than a second implementation.
- A sampling rule that does not change filter *family* across 1:1, so two
  renders at different scales agree.
- Accumulation across a run of image draws, so abutting ones do not conflate.
- Analytic tests for all three, every expectation computed from the geometry.

## Non-goals

- Removing conflation between an image and a *path*, or between images
  separated by other drawing. A run ends at anything that paints, and it must:
  fragments are added, and z-order is not negotiable.
- Smoothing a magnified image. `/Interpolate false` means hard samples and
  gets them; only a sample's *edge* is anti-aliased rather than snapped.
- Making the `dpi` relation hold on every image file.

## Design

**Coverage is the quad, filled.** `draw_image` builds the unit square's four
corners through the transform and fills them with the non-zero rule over the
rectangle the draw already visits. Reusing `fill` buys the sixteen-sub-scanline
grid, exact 1/256 spans, integer accumulation, the row walk over cells that
sorts nothing, and the cancellation contract — five properties a second edge
walker would have to earn again, and could then disagree with `fill` about.

The corners are **snapped to that 1/256 grid** before filling. An image
rectangle placed by a translation lands on a grid line far more often than not,
and `a + (e - t)` need not equal `(a + e) - t` to the last bit, so an ulp
either side of a grid line falls on the wrong side of it. `fill` takes each
crossing to the nearest 1/256 unit, but the quad's corners come from an affine
evaluated at one magnitude and are compared against one evaluated at another,
and no rounding rule inside `fill` can reconcile two inputs that already
differ. A cropped page must be the sub-rectangle of the whole page *exactly*,
as a tile must under ruling 5. `round` is IEEE-exact, so the snap costs no
determinism.

**Sampling keeps one family across 1:1.** Hard samples are `Area` over the
pixel's own footprint, which above 1:1 is smaller than one sample: a pixel
inside a sample reads that sample alone, and only a pixel straddling two mixes
them. Hard pixels survive, a 1:1 blit stays byte-preserving, and magnification
is as scale-coherent as minification.

**Minification integrates exactly; the pyramid is a threshold.** An area
filter that integrates the destination pixel's true source rectangle agrees
with itself at another scale by construction: one pixel covers exactly the
union of the four that replace it when the device scale doubles. A box-filter
pyramid does not. It averages source-aligned blocks of two, so the support it
integrates is quantised to powers of two, and which blocks a pixel lands on
moves with the scale. The error is small in the mean and large in the count of
pixels that differ at all, which is what the `dpi` relation counts. Only a
fractional ratio separates the two: at an integer ratio on a full-canvas draw
the pyramid's blocks sit exactly under the destination pixels, so a synthetic
test at 2:1 or 4:1 cannot tell them apart.

Exact integration visits each source pixel once per draw, which is what
building a pyramid costs too. What it does not bound is the *same* image drawn
many times at extreme minification, where a pyramid is built once and every
repeat then costs the destination rather than the source. So the pyramid stays
for that, and engages only past 128:1 (`PYRAMID_ABOVE`).

**A run of image draws accumulates before it composites.** `fragments.rs` holds
premultiplied colour and coverage, four bytes a pixel, added rather than
composited. Two strips each covering half a boundary pixel accumulate to one
whole pixel of their average — which is exactly what the doubled render
box-filters down to.

What ends a run: anything that paints, any change of alpha, blend mode, clip or
soft mask, a knockout group (11.4.5 gives each element its own shape), the page
itself, and a draw that would land on coverage the run already holds — adding
an overlap would show both pictures at once. What does *not* end a run is `q`,
`Q` or a form boundary, none of which paints. That distinction is the whole
difference between the feature working and not: real content brackets every
image in `q`/`Q`, and flushing there would leave no run holding two of
anything.

Bounded at `MAX_IMAGE_RUN_PIXELS`, a quarter of `MAX_PAGE_PIXELS`: past it,
images composite one at a time and abutting ones conflate again, which is a
quality loss rather than a page that will not render.

The analytic tests are in `crates/tinker-pdf-raster/tests/analytic_sampling.rs`
and the unit tests of `crates/tinker-pdf-raster/src/image.rs`.

## Milestones

1. Coverage from the filled quad, snapped to the sweep grid: a half-covered
   edge is half the paint on either axis at all fifteen sixteenths; a corner is
   the product of its fractions; a 1:1 integer-aligned draw is
   byte-preserving.
2. One filter family across 1:1: a render and the box-filtered render at twice
   the scale agree at every ratio tried; `/Interpolate false` reproduces whole
   samples.
3. Runs, and what ends one: two abutting strips leave 63 composited singly and
   0 as a run; a shared boundary is the average of the two draws; runs survive
   `q`/`Q`.
4. Fingerprints and docs: the image-bearing determinism fingerprints reproduce
   on `wasm32-wasip1`; `features/rasterizer.md` carries the trade.

## Risks

| Risk | Mitigation |
|---|---|
| A run composited under the wrong clip paints through it | A run records the identity of the clip and soft mask in force and ends when either changes; every operation that swaps the canvas flushes outright |
| Fragments are added, so an overlap would show both pictures | Asked before every draw, over the band where the new draw meets what the run already covered rather than over either whole; a real overlap ends the run |
| Memory: a run buffers the canvas | Bounded to a quarter of `MAX_PAGE_PIXELS`, past which the per-draw path runs |
| Anti-aliased image edges add transposition noise, which `rotate` measures | Not mitigated: an anti-aliased edge at a fractional offset does not transpose exactly, as a quantised one would. It is a cost of the anti-aliased edge, paid in the `rotate` relation's budget |

## P-08 — Image drawing

The sampler above decides *what* a pixel reads. This section is about what
reading it costs, and it changes no byte: every mechanism below is shown equal
to the sampler and the composition it replaces, so no fingerprint and no golden
moves, and each is held to that by a test against the form it replaces.

### Scope

- A separable integer resampler for draws whose map has no shear or rotation
  term: the source span and overlaps of each destination column and row,
  planned once per draw, and every pixel's sums in `u64`.
- Image composition without a libm rounding, in the direct draw, in a run's
  composite and in `Canvas::blend_pixel_inked`.
- The facade's two sample loops: an identity path for 8-bit DeviceGray and
  DeviceRGB, and a 256-entry table for one-bit images and image masks.
- A one-channel coverage form for bilevel images, sampled as one channel and
  held at one byte a sample.
- The drawing half of the decode-to-target-size handshake.

### Non-goals

- Colour conversion beyond the two device identities: ICC-based, CMYK and
  Indexed throughput, and converting an image once at its decoded size, are
  P-22's.
- The decoders' own speed, which is P-09's, P-10's and P-11's.
- The pyramid's policy: `PYRAMID_ABOVE` and `PYRAMID_TARGET` stand.
- `Page::images`, which keeps its own samples
  (`crates/tinker-pdf/src/images.rs`).

### Design

**Which draws are planned.** `Transform::invert` maps a device pixel centre
`(x, y)` back to the unit square as `u = a*x + c*y + e` and
`v = b*x + d*y + f`. When the inverse's `b` and `c` are both zero — the draw
has no shear and no rotation, though it may be flipped on either axis — `c*y`
and `b*x` are signed zeros at every finite centre, so `u` is the same number on
every row and `v` the same number in every column, up to the sign of a zero,
which no sampler reads: the area filter truncates `u * w * 65536` to an integer
and bilinear subtracts a half. Such a draw plans its columns and rows; every
other draw keeps the per-pixel map. `sampling_for` answers `Area` or `Bilinear`
and never `Nearest`, so those two are what a plan serves.

**A column's plan and a row's.** For `Area` a column's centre is
`cx = (u * w * 65536) as i64`, its source span `[cx - h, cx + h)` with `h` half
the 16.16 footprint, and its plan the texels the span touches with their
non-zero overlaps in 1/65536ths — `span` and `overlap` exactly as the
per-pixel filter calls them. A row's is the same with `1 - v` and the image's
height in place of `u` and its width. For
`Bilinear` a column's plan is `split`'s sample and fraction and the two taps
clamped into the image, and a row's likewise. Both are computed once per draw
from the **page-frame** pixel centres `px + 0.5` and `py + 0.5` the per-pixel
map uses, over the columns and rows `device_bounds` gives, so a tile plans the
page's numbers (ruling 5).

**A pixel's sum.** `sum over ty of oy * (sum over tx of ox * t)` is
`sum over ty and tx of (ox * oy) * t` exactly in integers, and the weight
`sum over ty of oy * (sum over tx of ox)` over the samples present is the
per-pixel filter's weight, so the quotient `(S + W / 2) / W` is the same
number. A sample a short buffer does not hold drops out of both sums, as `area`
and `texel` drop it. A span's ends saturate at `i64`'s range, which only an
axis of one sample squashed past 10^14 samples a pixel reaches, and there the
one sample is read whole. Bilinear's four taps and its `u32` weights do not
change; only where they come from does.

**The bound that keeps the sums in `u64`.** An overlap is at most 2^16. Along
one axis a pixel's overlaps sum to at most its footprint, which is at most 128
texels to 128:1, where nothing is reduced: 2^23. Past 128:1 the pyramid holds
the residual within 4 texels (2^18), or stops at a level of at most two samples
(2^17), or stops after sixteen halvings at a level of at most 2^16 samples of a
32-bit dimension (2^32); the facade's dimensions, at most 2^16, are one sample
there. A channel sum is at most `255 * Wx * Wy`: under 2^54 to 128:1 and
wherever neither axis was halved sixteen times, and under 2^63 where one was.
Both axes would have to be halved sixteen times and still be thousands of
samples wide — an image of 2^56 samples, whose first pyramid level no address
space holds — before a sum could reach 2^64. So no sum needs `u128`, and none
is left.

**No per-pixel `floor`.** `split`'s floor becomes a truncating cast and a
correction: `t = v as i64`, less one where `t as f64 > v`. That is `floor` for
every `|v| < 2^52` and for NaN, and `v` is `u * w - 0.5` with `u` clamped into
`[0, 1]`. The fraction is `v - t` as before: the same subtraction of the same
two numbers. A rotated or sheared draw keeps the per-pixel map with this split
and the `u64` sums, so no draw multiplies in `u128` or calls `floor` per pixel.

**Composition without a libm rounding.** A directly drawn pixel's opacity is
`alpha * c / 255 * o / 255 * k / 255` in `f64`, in that order — `alpha` the
draw's, `c` the quad's coverage, `o` the sample's alpha, `k` the clip — a
run's is `alpha * c / 255 * k / 255`, and `blend_pixel_inked` makes either a
level as `round(clamp(x, 0, 1) * 255)`. Three replacements, each exact:

- `nearest_level(x)` is `t`, plus one where `x - t >= 0.5`, with
  `t = x as u32`. `x - t` is exact — `t <= x < 2 * t` for `x >= 1`, which is
  Sterbenz's condition, and `x` itself below one — so this is `f64::round` for
  every `x` in `[0, 2^32)`. `blend_pixel_inked` keeps its `f64` for shadings
  and meshes and rounds with it; image pixels reach the canvas by their level
  instead.
- For an alpha of exactly 1 the level is `(c * o * k + 32512) / 65025`, and a
  run's `(c * k + 127) / 255`. The exact value `N / 65025`, `N = c * o * k`, is
  never a half-integer — `2 * N` is even and 65025 odd — so it stands at least
  1/130050 from one, while the `f64` expression makes six roundings of relative
  size 2^-53 on a value at most 255 and so lies within 2e-13 of the exact
  value. Both round to the same integer, `floor((N + 32512.5) / 65025)`,
  which is `(N + 32512) / 65025` in integers because no multiple of 65025 lies
  above `N + 32512` and at or below `N + 32512.5`. A run's two factors stand
  1/510 from a half-integer, against four roundings.
- For any other alpha the expression is evaluated in today's order and rounded
  by `nearest_level`. No integer form can stand in for it: the alpha carries 53
  bits, and for a decimal alpha such as 0.37 the exact value against an integer
  product can lie within 1e-15 of a half-integer, where only the expression's
  own roundings say which way it goes. The level of a fully covered, opaque,
  unclipped pixel — most pixels — is computed once per draw.

The draw's alpha is clamped and classified once per draw, and no image pixel
calls `round`.

**The identity path.** An 8-bit DeviceGray or DeviceRGB image whose `/Decode`
pairs are each `[0 1]` or absent, with no colour key and no `/ImageMask`,
copies its samples into RGB a row at a time, a grey replicated to three. The
sample loop reads a sample `v` as `raw = v / 255.0`, a `[0 1]` pair maps it to
`0 + raw * 1`, which is `raw` exactly, and `to_rgb` answers
`round(raw * 255)`, which is within 1e-13 of `v` before it is rounded and so is
`v`. A sample the stream stops short of reads as
zero, as `read_bits` reads it.

**The one-bit table.** A one-component, one-bit image — a fax, a JBIG2 region,
a one-bit Flate or uncompressed stream, an `/ImageMask` — has two possible
samples, and everything the sample loop makes of a sample is a function of its
bit: the component through `/Decode` or Table 90's default by the loop's own
expression, the colour through `to_rgb`, a mask's paint as that component below
one half (8.9.6.2), and a colour key's test, which is on the raw bit (8.9.6.4).
So the two outcomes are computed once per image, and a 256-entry table maps a
byte to its eight samples' bytes. A row begins on a byte (8.9.3), so it reads
`ceil(w / 8)` bytes and keeps its first `w` samples, and a byte past the end of
the stream reads as zero. A one-bit image of more than one component, and every
other depth, keeps the general loop.

**The coverage form.** A one-bit, one-component image is two RGBA values, `X0`
and `X1`, chosen by each sample's bit, and both filters are linear in their
weights. The area filter's channel is `(X0 * W0 + X1 * W1 + W / 2) / W`, with
`W1` the weight on the 1-samples present and `W = W0 + W1`; bilinear's is
`(X0 * (65536 - B1) + X1 * B1 + 32768) >> 16`, with `B1` its weight on 1-taps
and a tap past a short buffer taking the anchor's bit as it takes the anchor's
colour. So a coverage image is sampled by summing one channel beside the
weight, and each of the four channels comes from the two sums exactly as the
average of the image expanded to RGB rounds it. A stencil is the same pair with
the paint opaque and the rest transparent, its colour the tint. Past 128:1 the
pyramid averages each channel with round-half-up, which is not linear in
coverage — a halved two-colour level holds each channel's rounded midpoint, not
a third coverage value — so a draw that reduces builds its first level from
the colour each sample chooses, which is the level the image expanded to RGB
halves to, and samples the RGB levels as before. The form is produced where a sample's colour and alpha
are a function of its bit alone: one component, one bit and no `/SMask`. A
colour key that selects one of the two raw values folds into that value's
alpha; one that selects both, and a soft mask, keep the RGB form. `to_svg`
expands the form to RGB for its PNG.

**The handshake's drawing half.** A decoder that answers a drawn size with
fewer samples — a JPEG at 1/2, 1/4 or 1/8 (P-09), a JPEG 2000 codestream with
resolution levels discarded (P-10) — hands back a smaller grid over the same
unit square, and the draw's transform does not change: `sample_ratios` reads
the new dimensions, so the policy, the plans and the quad are the reduced
grid's, and the image lands where the full decode does. A window is the
sub-square of the unit square it covers, drawn through the transform composed
with that sub-square's own map. The size asked for is, per image axis, the
device length of the unit square's side, which is what `sample_ratios` already
measures, and a decoder reduces an axis only while it keeps at least that many
samples, so no reduced grid is drawn magnified. `image_requests.rs` holds the
handshake end to end: a 203 by 157 RGB JPEG at 4:2:0 at each of the three
scales and a 99 by 61 grey one at two, and codestreams of both wavelets with
one, two and three levels dropped and with windows, drawn axis-aligned,
flipped, quarter-turned and sheared at the sizes that pick each answer. Each draw is
held to the same draw of its full decode — the image under a `/Mask` whose
colour-key ranges run backwards, which keeps the decode whole and masks
nothing — for the pixels it reaches and covers wholly, its edge's coverage,
and where its picture's weight lands; the answer each decode chose is read
from the render's counted work and the samples it leaves along each axis
from T.81's and T.800's size rules, never fewer than the drawn pixels; and a
page of the answers is byte-equal tile by tile.

### Milestones

1. **P-08.2: the `bilevel` fingerprint, before any change.** One-bit DeviceGray
   images plain and CCITT G4, one under `/Decode [1 0]`, a two-colour one-bit
   `/Indexed` image and an `/ImageMask` in a fill colour, each drawn magnified,
   at 1:1 and minified 2:1 and 3:1 at fractional offsets. Exit: the `bilevel`
   fingerprint joins `rendering_is_stable_across_targets` on Linux, Windows and
   `wasm32-wasip1`; `every_tile_is_byte_equal_to_the_page_under_it` holds on its
   page; `features/determinism.md` names it.
2. **P-08.3: the integer resampler.** Exit: `image.rs`
   `the_column_plan_is_the_per_pixel_sampler`, every filter from 8:1 magnified
   to past 128:1 minified, flipped axes, fractional offsets, short buffers,
   alpha and region rectangles, pixel for pixel equal to the per-pixel `walk`
   kept test-only; `a_draw_plans_its_weights_once_per_column_and_row`, plans
   counted at most columns plus rows; `analytic_sampling.rs`, the `image`,
   `bilevel` and `analytic_image` fingerprints, `image.ppm` and
   `render_regions.rs` unchanged.
3. **P-08.4: composition without a libm rounding.** Exit: `canvas.rs`
   `an_image_pixel_composes_as_the_float_expression_rounded`, exhaustive over
   coverage, sample alpha and clip at a sweep of alphas;
   `an_image_draw_rounds_no_float_per_pixel`, `f64::round` calls on the image
   path counted at most one per draw; `blend_modes.rs`,
   `analytic_sampling.rs`, every fingerprint and golden unchanged.
4. **P-08.5: the identity path.** Exit: `resources.rs`
   `an_eight_bit_device_image_converts_by_identity`, all 256 values through
   both spaces equal to `to_rgb`'s answer and the loop's `to_rgb` calls counted
   at zero; `images.rs`'s `an_rgb_image_is_drawn`, `a_grey_image_is_drawn` and
   `a_decode_array_inverts_the_samples`, `inline_images.rs` and the `image`
   fingerprint unchanged.
5. **P-08.6: the one-bit table.** Exit: `resources.rs`
   `one_bit_samples_expand_through_the_table`, every byte value, both
   polarities, `/Decode [1 0]`, widths not a multiple of eight and a colour key
   equal to the bit loop, and `read_bits` calls counted at zero; `ccitt.rs`'s
   `a_group_4_fax_renders_to_known_pixels`, `jbig2.rs`, the `jbig2` and
   `bilevel` fingerprints unchanged.
6. **P-08.7: the coverage form in the raster crate.** Exit: `image.rs`
   `a_two_colour_image_draws_as_its_rgb_expansion`, every filter, ratio, offset
   and colour pair and a tint equal to the RGB expansion;
   `a_coverage_image_sums_one_channel`, channel sums per tap counted at one;
   P-08.3's differential test and `analytic_sampling.rs` unchanged.
7. **P-08.8: bilevel images carried as coverage.** Exit: `resources.rs`
   `a_bilevel_image_is_held_at_one_byte_a_sample` for a JBIG2, a G4 and a
   one-bit Flate image; the `bilevel`, `jbig2` and `image` fingerprints,
   `images.rs`'s `a_stencil_mask_paints_in_the_fill_colour`, `jbig2.rs`'s
   `an_image_mask_paints_a_jbig2_region_in_the_fill_colour`, `svg_output.rs`
   and `render_regions.rs` unchanged.
8. **P-08.9: the handshake, end to end.** Exit: `image_requests.rs`
   `every_reduced_answer_lands_where_the_full_decode_does` and
   `a_reduced_answer_is_never_magnified`, guard A7, and every tile equal to its
   page.
9. **P-08.10: the row closes.** Exit: a local MS-02 paired run green with the
   four floors — the half-size image replay at 150 dpi 8.75x, the half-size G4
   scan page at 150 dpi 12.8x, the half-size JBIG2 scan page at 150 dpi 10.9x
   and the quarter-size Flate RGB image page at 72 dpi 4.01x; the row's counted
   guards green on Linux, Windows and `wasm32-wasip1`; the `image`, `bilevel`,
   `jbig2`, `jpx` and `cbz` fingerprints holding on the four targets.

### Risks

| Risk | Mitigation |
|---|---|
| A plan made in a tile's own frame rounds apart from the page's (ruling 5) | Columns and rows are planned from page-frame pixel centres, the numbers the per-pixel map uses, so a tile's plan is the page's restricted to the tile; `render_regions.rs` tiles the `bilevel` page with the rest |
| The run path: `accumulate_image` and `draw_image` share `walk`, and a plan written for one would leave runs — every page of strips — on the per-pixel map, or give the two consumers different samplers | One `walk` plans for both emitters; the differential test drives `walk` itself, and `analytic_sampling.rs`'s conflation pair holds the run |
| A sum past `u64` panics in a debug build and wraps in a release one | The bound above, and the differential test past 128:1 |
| For an alpha other than 0 and 1 the answer is the `f64` expression's own roundings | The expression kept in its order, and held exhaustively over coverage, sample alpha and clip at a sweep of alphas |
| The pyramid is not linear in coverage | A bilevel draw that reduces expands to RGB |
| A table that forgets a `/Decode`, a polarity or a colour key | The table's two outcomes come from the loop's own expressions, and a test holds every byte value against the bit loop |

## RC-21 — A run decided in the page's frame

Ruling 5 draws a tile through the page's one frame, so every number a tile
computes is the number the page computes. One decision is still taken on the
canvas: whether an image draw *overlaps* what the run already holds, which
ends the run (`Renderer::run_would_overlap`). The run's fragments cover the
canvas and nothing else, so where the page finds an overlap outside a tile the
tile keeps a run the page ended, and a seam inside the tile conflates on the
page and not in the tile — ruling 5's named exception. This section states
the class of pages that exception covers, three ways of answering the
question in the page's frame, what each costs a tile and which whole-page
renders each moves, and how they are priced before H-15 chooses one.

### Scope

- The run-membership question answered in the page's frame, so that every
  shape of the class tiles byte-equal.
- The class, one fixture per shape, pinned as it stands until the answer
  moves (RC-21.2).
- Three mechanisms, each shadowed beside today's answer by a test-only
  implementation that decides nothing, and priced in one table (RC-21.3);
  the one H-15 takes lands (RC-21.4) and a bench holds its price (RC-21.5).

### Non-goals

- What else ends a run. A change of alpha, blend mode, clip, soft mask or
  ink, a knockout group, a frame past `MAX_IMAGE_RUN_PIXELS` and every call
  `ends_image_run` names are decided from the graphics state and the frame's
  area, which a tile and the page share already.
- What a run is: fragments added and held to 255, `SLACK`, and the budget.
- Banded renders. `DisplayList::render` cuts its bands where no run stands,
  so a band never meets the exception, and its planner is unchanged.
- `RunReach` and the patch that grows its rectangle by it (`patch.rs`).
  Under a mechanism that answers in the page's frame a region still reports
  what it could not see, and what it reports is still true.
- The choice, which is H-15's.

### Design

**The question as the canvas asks it.** A draw that could join the run — no
change of state, not drawn directly — takes its rectangle on the canvas
(`image_bounds_in` against `device()`), meets it with the rectangle the run
has put coverage in on the canvas (`ImageRun::covered`), fills its unit
square over that band (`image_coverage`) and asks the fragments whether any
pixel of the band would then hold more than `255 + SLACK` levels
(`Fragments::would_overlap`). The fragments hold at each pixel the members'
edge coverage times their samples' alpha, added and held to 255
(`accumulate_image`). A draw with no rectangle on the canvas reads nothing,
is answered no, and joins. What the question reads is the band: each of its
pixels once for the square and once for the fragments.

**Where a tile parts from the page.** The page asks the same question over
its own band: the draw's bounds within the frame met with the hull of the
members' bounds within the frame, which `ImageRun::framed` already keeps for
`RunReach`. A tile's band is the page's band cut at the canvas, and while the
two runs hold the same members the tile's fragments are the page's at every
pixel the tile holds. So the first answer that parts is always one where the
page finds an overlap outside the canvas and the tile, reading only the
canvas, answers no. After it the tile's run holds members the page already
composited, and later answers can part either way.

**The class.** Each way a page can put the page's overlap outside a tile, on
a 100 by 100 page of one-sample images, the abutting seam at a half pixel so
that conflation shows:

1. *Overlap outside the tile in y*: `A` at `40.5 0 0 40 0 60 cm`, `B` at
   `40 0 0 40 40.5 0 cm`, `C` at `40 0 0 70 40.5 30 cm`, in that order. `A`
   and `C` abut along column 40 in rows 0 to 39; `C` overlaps `B` in rows 60
   to 69, below every tile that holds the seam's rows and not those.
2. *Overlap outside the tile in x*: the same page transposed, `A` at
   `40 0 0 40.5 0 59.5 cm`, `B` at `40 0 0 40 60 19.5 cm`, `C` at
   `70 0 0 40 0 19.5 cm`: the seam along row 40, the overlap in columns 60
   to 69.
3. *A draw wholly outside the tile that overlaps a member*: `A` at
   `40.5 0 0 100 0 0 cm`, `B` at `40 0 0 30 20 0 cm`, overlapping `A` in
   rows 70 to 99, `C` at `40 0 0 40 40.5 60 cm`, abutting `A`. On the page
   `B` ends `A`'s run and `C` joins `B`'s; in a tile above row 70 `B` has no
   rectangle, joins, and `C` joins `A`.
4. *Soft-masked images*: the first page's three images each with a
   one-sample `/SMask` of 0.8, so the band holds coverage times an alpha the
   samples set, and a mechanism that reads squares alone reads the wrong
   number.
5. *Stencil images*: the first page's three as `/ImageMask` stencils in one
   fill colour, `B` two samples tall with only its upper sample painting, so
   the overlap is in its painted half and the stencil route carries the ink.
6. *A group's canvas*: the first page's three drawn inside a transparency
   group's form whose `/BBox` is `[5 5 95 95]`, a buffer that stands inside
   the page and, in a tile, inside the tile, with a frame of its own.
7. *A soft mask's canvas*: the first page's three drawn white in a
   luminosity soft mask's group, the page filled through the mask: the seam
   is in the mask's luminosity and shows in the fill.
8. *Turned*: the first page under `/Rotate 90`.
9. *Cropped*: the first page under the crop box `[7 3 93 97]`, offset from
   the media box on both axes.
10. *Cropped and turned*: the same crop under `/Rotate 270`.
11. *An annotation's appearance*: the first page's three drawn by a
    `/Square` annotation's appearance over an empty content stream, after
    the content and in the annotation's scope.

And *display-list replay*: each tile drawn from `Page::display_list`. A
replay delivers the calls the interpretation made, so its tiles are the
direct tiles, exception and all. The class is tiled at the guard's lattices
and scales: 64, 37 and 23 pixels at 1x, 53 at 0.5x, 0.75x, 1.5x, 2x, 3x and
4x, and one pixel at 0.22x.

**Mechanism A — the run's coverage held over the frame.** The row's own,
and the reference H-15 names. The run accumulates each member over its
bounds within the frame rather than the canvas, into fragments the frame's
size, and asks the question over the page's band. The tile's answer is the
page's by construction. Its price is the page's image work: a tile samples
every member over all of its bounds within the frame, however little of it
the tile shows, and holds the run's coverage over the frame, a byte a pixel
of it at the least beside the canvas's four. A tile of a 300-dpi page of
strips draws the page's strips whole. No whole-page render moves, because on
a whole page the canvas is the frame and this is today's run; every
fingerprint, golden and pin holds, and every shape of the class tiles
byte-equal. RC-21.4's exit, which forbids a tile the page's image work, rules
it out; it is priced as the bound the other two are read against.

**Mechanism B — the members outside the canvas recomputed over the band.**
The run keeps, for each member, what recomputes its coverage at a pixel: its
transform and the square its samples are cut at, its edge level and rule, its
bounds within the frame, and the samples that carry alpha — an image's soft
mask or colour key, a stencil's or a two-colour image's samples; none for an
opaque image, whose coverage is its square's. The question is asked over the
page's band. Inside the canvas the fragments answer as today; outside it,
each member whose bounds within the frame meet the band is sampled over the
part of the band outside the canvas only, its edge coverage times its
alpha exactly as `accumulate_image` adds it — a function of the page-frame
pixel alone (ruling 5) — summed and held to 255 as fragments are, and the new
square is filled over the whole band. A draw with no rectangle on the canvas
is asked over its band like any other. Every number the tile reads is the
page's number at that pixel, so its answer is the page's. Its price, per
question, is the members' bounds scanned and the band outside the canvas read
once for each member that meets it; it holds two transforms and a rectangle a
member and the alpha samples of members that have them. A page of abutting
strips meets each strip's predecessor in one row, so a tile reads, outside
itself, one row of the page a strip. No whole-page render moves: on the page
nothing is outside the canvas, and the question is today's. Every
fingerprint, golden and pin holds.

**Mechanism C — a rule from the page-frame geometry alone, shared by page
and tile.** A draw overlaps the run when its unit square, as four device
corners in the page's frame snapped to the 1/256 grid the coverage is snapped
to, meets some member's in an area of more than `SLACK / 255` of a pixel.
Nothing is read and no sample kept, and a page and a tile ask the same
arithmetic of the same numbers, so every tile is its page. Its price, per
question, is every member's square met with the draw's, a convex clip of two
quadrilaterals, and it holds sixty-four bytes a member. It moves whole-page
renders wherever coverage and geometry disagree, because the page then
answers by geometry too:

- *Alpha that leaves the overlap open*: a draw laid over a soft-masked,
  colour-keyed or stencil member, or a two-colour member's transparent
  colour, where the member's samples are transparent. Today it joins; under
  C it ends the run, and seams of that run conflate.
- *A sliver*: squares that overlap along an edge by less than `SLACK` in
  every pixel and by more than `SLACK / 255` of a pixel in all. Today
  abutment; under C an overlap.
- *Quantised edges*: at an edge level below eight bits or with anti-aliasing
  off, the fragments hold each member's edge quantised while the question's
  square is not, so strips meeting at a fractional pixel can overlap today
  and abut under C.

The fingerprints and goldens that may move are those of pages drawing a run
of two images or more, and which of them do is found by RC-21.4's run if
H-15 takes C, each a finding the owner reviews and never an edit.

**The bands.** A question's band on the page is the draw's bounds within the
frame met with the hull of the members' bounds within the frame. Under B a
tile reads, for one question, at most that band once and, outside its canvas,
the band once more for each member whose bounds within the frame meet it —
RC-21.4's bound on a tile's reads. Under A a tile reads every member's bounds
within the frame, which no band bounds.

**The counter.** `Renderer::run_pixels`, a `#[cfg(test)]` count beside
`mask_pixels`: for each question asked, the area of the band
`run_would_overlap` fills the square over and reads the fragments at. A draw
answered no without a band adds nothing.

**The shadows.** Each mechanism is shadowed by a test-only implementation
beside the canvas's answer, switched on per renderer. A shadow keeps its own
run as the page would hold it: ended where its own answer finds an overlap,
and wherever the real run ends for anything else — a change of state, a draw
made directly, any call that flushes. It answers every question the canvas is
asked and counts the pixels it read, the members it scanned, the bytes it
holds beyond today's run, and the answers that differ from the canvas's. It
decides nothing the render uses, so no pin moves while it is there.

**What RC-21.3 prices.** The class's eleven pages, drawn by the render crate's
own interpreter from the same content, the annotation's appearance as the form
it is; a page where a stencil is drawn over another's unpainted half, beside a
third that abuts the first, which C moves and A and B do not; and a generated
300-dpi page of abutting strips, 4 by 6 inches, 1200 by 1800 pixels, of
forty-one full-width strips each drawn from 1200 by 44 RGB samples and
meeting its neighbour at a fractional row. Each page whole, in four bands and
on a 64-pixel lattice; for each mechanism and today's, per tile, the
questions asked, the pixels read, the members scanned and the bytes held, the
answers and the tiles whose answer changes, and the time against today's, in
a release and a debug build. A time is a ratio to today's on one quiet
machine, never a baseline. The shadows and the probe are
`crates/tinker-pdf-render/src/run_shadows.rs`; the table is
`crates/tinker-pdf-render/data/run_prices.tsv`, whose head names its columns,
and each build's run rewrites that build's rows:

```bash
cargo test -p tinker-pdf-render --release -- --ignored price_of_deciding_runs_in_the_page_frame
cargo test -p tinker-pdf-render -- --ignored price_of_deciding_runs_in_the_page_frame
```

The shadows are held to what they stand for before anything is priced from
them: `the_shadows_answer_in_the_pages_frame` holds A's and B's answers, on
every tile of every lattice of the class and the open-overlap page, to the
page's question by question, and C's to its own answers on the page;
`every_page_of_the_class_parts_some_tile_from_the_page` holds each class page
to a tile whose canvas parts from the page; `geometry_ends_a_run_coverage_keeps`
is C's moved render on the open-overlap page; `the_shadows_change_no_pixel`
holds a render with every shadow on to the render with none; and
`two_quadrilaterals_share_their_overlap` holds C's area arithmetic.

### Milestones

1. **RC-21.2: the class, pinned.** Exit: `render_regions.rs`
   `every_shape_of_the_image_run_exception_is_pinned` passes in the default
   and `bundled-fonts` builds with one exact pin per shape and scale — the
   differing tiles' pixels, worst level, first pixel and columns or rows, or
   byte equality — and every tile of each shape drawn from
   `Page::display_list` equal to its direct tile; ruling 5 and
   `features/rendering.md` name the class.
2. **RC-21.3: the prices.** Exit: `tinker-pdf-render`
   `a_run_reads_only_the_band_where_a_draw_meets_it` pins today's counted
   reads on abutting strips; `price_of_deciding_runs_in_the_page_frame`
   (ignored, prints `RAN`) rewrites `run_prices.tsv` in release and debug,
   and H-15 is decided from it.
3. **RC-21.4: the mechanism H-15 takes.** Exit: the class joins `fixtures()`
   and passes `every_tile_is_byte_equal_to_the_page_under_it`, the scale and
   one-pixel lattices and `a_display_list_tiles_as_the_page_does` in both
   builds, failing on the code before;
   `an_image_run_that_overlaps_only_outside_a_tile_tiles_as_the_page_does`
   asserts equality; `a_tile_reads_only_the_bands_its_runs_meet` holds a
   tile's counted reads to the bands above; the probe re-run on the landed
   code rewrites the table.
4. **RC-21.5: the price held.** Exit: MS-04's render group gains "a 300-dpi
   page of abutting image strips in four bands", generated in the bench and
   sized 20–300 ms; `cargo xtask baseline` holds the bench file and
   `baseline.json` to one list; a local MS-02 paired run is green.

### Risks

| Risk | Mitigation |
|---|---|
| A recomputed member's coverage rounds apart from the one the page accumulated | It is the same walk over a smaller rectangle: the edge filled and the sampler planned from page-frame pixel centres, which ruling 5 makes the same numbers on any canvas; the class is tiled to byte equality |
| A shadow that answers wrongly prices a mechanism that does not exist | Mechanisms A and B answer as the page does by construction, so every tile whose answer each changes is a tile of the class; a shadow that changes an answer elsewhere is a finding about the shadow |
| A page's runs hold hundreds of members, and B and C scan them all | The scan is counted per question in the table; RC-21.4 may index members by row where the table says the scan dominates |
| The time of a debug build misleads | Both builds are priced, and a time is only ever a ratio to today's on the same machine |
| C moves pages that render correctly today | Named as its price above; under C a moved fingerprint is a finding the owner reviews before anything is committed |
| A soft mask's or a group's frame differs from the page's canvas | Every shadow asks in the frame of the canvas it is on, the rectangle that canvas would have on the whole page, and is reset where the canvas swaps |
| A decode that holds a window of its image (`SourceWindow`) has no samples outside it, so B resamples a stand-in alpha there | The window keeps every pixel of the square covered, so an opaque member is exact; for one whose alpha the window cuts, B asks the decode for the band's part, which RC-21.4 prices if H-15 takes B |
