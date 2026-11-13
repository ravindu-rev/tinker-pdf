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
and `texel` drop it. Bilinear's four taps and its `u32` weights do not change;
only where they come from does.

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
a third coverage value — so a draw that reduces expands the image to RGB and
samples it as before. The form is produced where a sample's colour and alpha
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
samples, so no reduced grid is drawn magnified.

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
