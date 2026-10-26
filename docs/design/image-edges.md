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
grid, exact 1/256 spans, integer accumulation, the active-edge sweep and the
cancellation contract — five properties a second edge walker would have to earn
again, and could then disagree with `fill` about.

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
