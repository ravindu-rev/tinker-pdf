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
| 3 | Anchoring to the parent's default space | The same pattern filled under two different CTMs puts the lattice in the same place — **and the same pattern *stroked* under two different CTMs, per the amendment below** | S |
| 4 | `PaintType 2` | An uncoloured pattern takes the `SCN` colour; colour operators inside it are ignored | S |
| 5 | Tile cap | A pattern with a one-unit step over a large page warns and terminates rather than hanging | S |

*Amended, August 2026, by [07](07-stroked-patterns.md).* **Milestone 3 must
assert anchoring on the stroke path as well as the fill path, and milestone 4
on the stroke slot as well as the fill slot.** Neither is optional and neither
is covered by what 07 landed.

Gap 07 routed `Renderer::stroke_path` and the stroking half of
`show_glyph` through `fill_with_pattern`, so a tiling pattern on a stroke
already reaches the same `PatternPaint` and the same warning as a fill. When
this plan makes that paint, the stroke path starts painting tiles on the day
milestone 2 lands, with no separate wiring and therefore no separate test.

That is the problem. 07's anchoring test —
`a_stroked_pattern_does_not_move_with_the_transform` in
`crates/tinker-pdf/tests/colour_spaces.rs` — strokes the same pattern under two
different CTMs and asserts the two renders are byte-identical, but it can only
use a *shading* pattern, because a tiling pattern warns rather than paints
today. So the lattice-does-not-slide guarantee currently holds for shadings
only, and this document's own "worse than none" section names paint-time-CTM
anchoring as the silent failure mode that turns an honest gap into a false
capability. Without a tiling copy of that assertion the hole sits exactly where
this plan says the danger is.

The cheap discharge is to copy that test with `HATCH` in place of `GRADIENT`
once tiles paint, and to add its stroke-slot twin for `PaintType 2`: 07 stores
the `SCN` components for the stroke slot as well as the fill slot
(`an_uncoloured_pattern_keeps_the_colour_its_operands_gave` in
`interpret.rs`), but nothing has ever rendered them, so milestone 4 is the
first thing that can prove the stroke slot's colour is the one that paints.

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
| Tiles start painting on strokes for free, so nothing forces a stroke-path anchoring test to be written | The milestone 3 amendment above. There are **two** "stays blank" tests to rewrite — `a_tiling_pattern_is_reported_rather_than_blacked_out` and `a_tiling_pattern_stroke_is_reported_rather_than_blacked_out` — and the second is the one that will otherwise be deleted without a replacement |
