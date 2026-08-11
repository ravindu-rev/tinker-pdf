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
