# Every paint costs a whole page

Drawing a comma allocates and scans a full-page coverage mask. So does every
other glyph, every clip, every fill. A page of ten thousand glyphs at 300 dpi
does ten thousand full-page passes. Rendering works and is far slower than it
should be, and the slowness scales with the page size rather than with what is
drawn. When this is done, a paint costs what it covers. (M)

## What is wrong

Three layers, each independently O(canvas).

**The mask is always page-sized.** All four `fill()` call sites in
`crates/tinker-pdf-render/src/lib.rs` — `fill_with_pattern`, `paint`,
`clip_path`, `end_text` — pass `0, 0, canvas.width, canvas.height`.
`Path::bounds` exists in the raster crate and is called nowhere outside its
own unit test.

`paint` is reached from `draw_image_placeholder`, `fill_path`, `stroke_path`,
and twice from `show_glyph` — fill and stroke. So one full-page mask per
glyph, twice for stroked text.

**Consumers ignore the mask's extent even when it has one.** `Mask` carries
`x0`, `y0`, `width`, `height` and `Mask::at` returns 0 outside — but
`Canvas::fill_mask_with` walks `0..self.height × 0..self.width` regardless,
`Mask::intersect` allocates a second full-size mask and iterates all of it,
and `Mask::empty` allocates `width × height`.

**`fill` itself scans the whole height.** Its row loop runs `0..height`
whatever the path covers, and inside it every sample tests every edge with
only a per-edge y-range check — no active-edge list, no sort by top. Cost is
`height × 16 × edges`.

## Scope

- Compute a device-space bounding box per path and pass it to `fill`.
- Intersect that with the current clip's bounds before allocating.
- `Canvas::fill_mask_with` iterates the mask's rectangle.
- `Mask::intersect` produces a mask bounded by the intersection, and
  `intersect_in_place` where the destination can be reused.
- An active-edge list in `fill`, or at minimum edges sorted by top so the
  per-row scan can start and stop.

## Non-goals

- **Changing the coverage arithmetic.** Sixteen sub-scanline samples,
  integer accumulation, deterministic (ruling 4). Bounding changes *where* the
  work happens, not what it computes. Output must be identical.
- **Tiling or threading.** Plan 07's tile contract says a tile renders through
  the same path with a translated viewport; that stays true and is not this.

## Design

The invariant that makes this safe: **bounding must not change a single
pixel.** A mask smaller than the canvas reports 0 outside itself, which is
what it would have contained anyway. So the whole change is verifiable by
running the existing suite unchanged — and by the determinism fingerprints,
which will not move if this is right and will move loudly if it is not.

*Amended, August 2026.* That last sentence was worth nothing when it was
written. The `text` fixture in `tests/determinism.rs` named Helvetica and
embedded no font program, and the engine bundles no faces — so every glyph
resolved to nothing, the page rendered blank, and its committed fingerprint
was the hash of an empty 200x100 canvas. The glyph path this plan is about
("one full-page mask per glyph") was the one path the fingerprints did not
cover, so milestone 1's exit criterion would have been satisfied by a change
that broke every glyph on the page. The fixture now embeds a synthetic
TrueType face of curves, diagonals and a hole and paints 1486 pixels, and each
fixture asserts a floor on its ink before it is hashed. Take the criterion at
face value from here; anything measured against the old `text` fingerprint was
measured against a blank page.

Order matters for the payoff. Bounding the *mask* without bounding
`fill_mask_with` keeps the full-page walk. Bounding both without touching
`fill`'s row loop keeps the `height × 16 × edges` scan. The three land
together or the win does not appear.

`Path::bounds` already exists and is already correct; it has simply never been
called.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Path bounds passed to `fill`, clipped to the clip's extent | Every existing test passes unchanged; `tests/determinism.rs` fingerprints do not move | S |
| 2 | `fill_mask_with` and `Mask::intersect` bounded | Same, and a benchmark of a small glyph on a large page shows the drop | S |
| 3 | Active-edge list in `fill` | Same again; a path with many edges over a tall page no longer scales with page height | M |
| 4 | A guard against regression | A test asserting a small fill on a large canvas touches a bounded number of pixels, so a future full-canvas call fails rather than merely being slow | S |

## Dependencies

**Needs first:** nothing.

**Unblocks:** [09](09-tiling-patterns.md) and [10](10-mesh-shadings.md) become
practical rather than merely possible — a mesh is thousands of small fills and
a pattern is thousands of small composites.

## Risks

| Risk | Mitigation |
| --- | --- |
| An off-by-one in the bounds clips a pixel of the shape | The determinism fingerprints and the whole existing suite are the check; this is the rare change where "no test moved" is exactly the right outcome |
| Bounds computed before the transform, or after, inconsistently | `Path` in the raster crate is already in device space by the time it reaches `fill`; bounds are computed there and nowhere else |
| The active-edge list changes coverage at edges through a different accumulation order | Integer accumulation is order-independent by construction; the fingerprints prove it |

## As built — August 2026

Four commits, one per milestone. All three layers were still O(canvas)
exactly as described, and all three had to land before the win appeared.

**The measurement, which is what this plan asked for.** US Letter (612 x 792
pt) at 300 dpi — 2550 x 3301 px — with 600 glyphs of embedded 9 pt text,
release build, `Document::open` plus `render`:

| | total | per glyph |
| --- | --- | --- |
| before | 5997.7 ms | 9.996 ms |
| after M1 (bounded masks) | 51.9 ms | 0.087 ms |
| after M2 (bounded consumers) | 26.1 ms | 0.044 ms |
| after M3 (active-edge list) | 18.8 ms | 0.031 ms |

**318 times**, and the plan's insistence that the three land together is
right: M1 alone still walked the page to composite, and M1 with M2 still
scanned every row of the region they were given. The mask for one 9 pt glyph
went from 8 417 550 bytes to 252.

Milestone 3's own criterion, a 500-edge zigzag confined to 200 px of height
filled into a 600 x H region, with the pre-gap-14 `fill` linked into the same
process so the comparison is one machine and one cache state:

| H | before | after |
| --- | --- | --- |
| 500 | 8.0 ms | 6.9 ms |
| 1000 | 11.8 ms | 6.9 ms |
| 2000 | 19.5 ms | 6.9 ms |
| 4000 | 35.6 ms | 6.8 ms |
| 8000 | 66.0 ms | 6.8 ms |

Sixteen times the paper cost 8.3 times as much before and 0.99 times as much
now. The two masks are byte-identical at every one of those heights.

**One pixel of slack on each side, and it is measured rather than assumed.**
The obvious `floor`/`ceil` box is one pixel too small. A rasterised edge can
deposit coverage in the pixel *outside* the box its own control points
describe: the flattener evaluates a Bezier as a float sum that may round an
ulp past the extreme control point, and the scanline walks a crossing as
`(x * 256) as i64` stepped by a slope truncated to 1/256 px. Both are
sub-pixel, and both cross an integer boundary when the shape's extreme lands
exactly on one — which for glyphs, rectangles and form bounding boxes is the
common case. Two million random paths over a 20 x 20 canvas, each compared
pixel for pixel against its full-canvas mask: the exact box lost 24 pixels,
worst 16 levels of 255; with one pixel of slack, none.

**The order-independence claim in the risk table holds, and the fingerprints
were not what proved it.** Within a sub-scanline, crossings at the same x can
be swept in either order and the two sweeps can decompose one interval
differently — `[a, x] + [x, b]` against `[a, b]`. `add_span` measures each
pixel's overlap in exact 1/256 units of `i64` and adds them into a `u16` that
cannot reach its ceiling (sixteen sub-scanlines of at most 256 units), so the
two decompositions come to the same integer. Checked three ways: a unit test
filling the same subpaths in both orders under both rules; byte-identity at
five page heights above; and 200 000 random paths of up to four subpaths
through the exhaustive scan and the active list under both rules — 400 000
mask comparisons, zero differences.

**What the fingerprints could not see.** This plan says "no test moved" is the
right outcome, and it is, but it is not the same as "the tests would have
noticed". Injecting a bound one pixel short on each edge in turn, the six
fingerprints caught **none of the four**: every fixture either draws at
fractional coordinates, where the slack is not load-bearing, or draws a
full-page rectangle, where the clamp to the canvas absorbs the error. What
caught each of the four, distinctly, was one ink-bounding-box assertion in
`an_oc_entry_on_a_form_xobject_hides_the_form` — `(0, 0, 9, 39)` became
`(1, 0, 9, 39)`, `(0, 0, 8, 39)`, `(0, 1, 9, 39)` and `(0, 0, 9, 38)`. Cutting
two pixels, one inside the geometry, is caught by 11 to 13 tests including the
fingerprints.

`intersect_in_place` stopping a row short was caught by **nothing at all**
until `intersecting_in_place_reaches_the_last_row_and_column` was written for
it: a paint's rectangle is already the clip's rectangle, and a rectangular
clip is opaque in the middle of itself, so the only place a dropped row shows
is the rectangle's own edge — which is exactly where the slack row sits.

**Milestone 4 is an instrument, not an assertion about pixels.**
`a_small_fill_on_a_large_page_stays_small` draws through each of the four
`fill` call sites on a 2000 x 2000 page and asserts the mask pixels asked for:
484 for a 20 x 20 fill, 132 for a stroked line, 968 for a clip and the fill
inside it, 1444 for a 40 pt glyph, 2368 for one filled and stroked. A
full-canvas paint is 4 000 000. Injecting exactly that — `paint_region`
returning the whole canvas — is caught by this test and by no assertion about
pixels anywhere, because the pixels are identical, which is the whole premise
of the gap.

**Two things outside the plan's list, same defect.** `fill_with_pattern`
evaluated its inverse transform and its shading function over every pixel of
the canvas, and `sh` swept the page while reading a clip that bounds it
(8.7.4.2). Both take the mask's rectangle now. `blit` was left alone: an image
is already bounded by its own placement.

**One live defect.** `a_degenerate_bounding_box_does_not_panic` caught a
`/BBox [0 0 1e9999 20]` saturating `as i64` to `i64::MAX`, where adding the
slack overflowed. Saturating now; ruling 1 covers a page that draws nothing,
not one that panics.

The six fingerprints did not move, on x86_64 Windows or on `wasm32-wasip1`
under wasmtime 47.0.3. 1181 tests to 1190.
