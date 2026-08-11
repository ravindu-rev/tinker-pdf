# Tiling patterns are reported, not painted

A hatch, a cross-hatch, a repeating logo watermark — `PatternType 1`. The
engine recognises it, warns, and leaves the area blank. That is honest, and it
is a hole in every technical drawing and every document with a patterned fill.
When this is done, a tiling pattern paints. (M)

## What is wrong

`PageResources::pattern` returns `PatternPaint::Unsupported` for anything that
is not `PatternType 2`, and `fill_with_pattern` turns that into
`RenderWarning::UnsupportedPattern` and leaves the area alone.

Nothing here is subtly wrong — the degradation is correct and documented. This
is a missing feature, not a defect.

## Scope

- `PatternType 1`, both `PaintType 1` (coloured) and `PaintType 2`
  (uncoloured, taking the current fill colour).
- `/BBox`, `/XStep`, `/YStep`, `/Matrix`, `/Resources`.
- Fills, and — with [07](07-stroked-patterns.md) — strokes.
- A bounded tile count, so a pathological `/XStep` cannot hang a render.

## Non-goals

- **Pattern-space caching across pages.** A tile rasterised once per fill is
  already the whole win; caching across fills is an optimisation with a
  lifetime question attached.

## Design

**Rasterise the tile once, then blit it across the lattice.** This is the only
part of the design that matters, and it is why this document depends on
[11](11-transparency-groups.md).

The obvious implementation — clip to the filled path, then replay the tile's
content stream once per lattice position with a translated CTM — is correct
and pathologically slow. Each tile needs its own bounding-box clip, every clip
goes through `save_state`, and `save_state` clones a page-sized mask. A hatch
with a 10 pt step over A4 is roughly 4,800 tiles and several gigabytes of
memcpy. Dropping the per-tile clip to avoid that is the trap described below.

So: render the tile into a small offscreen `Canvas` sized to its `/BBox` in
device space, then `Canvas::composite` it at each lattice position, clipped to
the filled path. `Canvas::composite` is milestone 1 of
[11](11-transparency-groups.md).

**Anchoring.** 8.7.3.1 anchors pattern space to the *parent stream's default
space*, not to the CTM in force when the pattern is used. The shading-pattern
path already does this correctly and the same `base` transform applies here.

**Uncoloured patterns** ignore colour operators in their content and take the
colour from the `SCN` operands that named the pattern.

**Bounding the work.** The lattice range comes from the filled path's device
bounding box mapped back into pattern space. Cap the tile count; past the cap,
warn and fill with nothing rather than hanging.

## Where a half-implementation is worse than none

**Anchoring to the paint-time CTM.** The lattice then slides under every
transform. It reads as a small offset rather than as a defect — the pattern is
*there*, just not where it should be — so it will not be reported as a bug.
This turns a tested, documented, honest degradation into a false capability,
which is strictly worse than the blank area it replaces.

The existing test that asserts a tiling-pattern fill stays blank must be
*rewritten deliberately*, not deleted. It is currently pinning correct
behaviour.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Tile rasterised to an offscreen canvas | A one-tile pattern fill matches the same content drawn directly at that position | S |
| 2 | Lattice blitting, clipped to the fill path | A hatch over a rectangle repeats at `/XStep`/`/YStep`; nothing paints outside the path | M |
| 3 | Anchoring to the parent's default space | The same pattern filled under two different CTMs puts the lattice in the same place | S |
| 4 | `PaintType 2` | An uncoloured pattern takes the `SCN` colour; colour operators inside it are ignored | S |
| 5 | Tile cap | A pattern with a one-unit step over a large page warns and terminates rather than hanging | S |

## Dependencies

**Needs first:** [11](11-transparency-groups.md) milestone 1 —
`Canvas::composite`. Attempting this first produces something that will be
rewritten.

**Unblocks:** [07](07-stroked-patterns.md)'s tiling half.

## Risks

| Risk | Mitigation |
| --- | --- |
| A tile whose `/BBox` is smaller than its step leaves gaps that must not be filled by the neighbour spilling | The tile canvas is `/BBox`-sized, so spill is impossible by construction rather than by clipping |
| A tile containing a form XObject or another pattern recurses | Reuse the existing form-recursion depth cap; a pattern that fills with itself terminates |
| The existing "stays blank" test gets deleted rather than rewritten, losing the anchoring assertion with it | Rewrite it into the anchoring test in milestone 3, in the same commit that makes it fail |
