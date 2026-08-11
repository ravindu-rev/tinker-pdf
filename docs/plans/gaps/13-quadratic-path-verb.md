# TrueType quadratics are up-converted to cubics

Every TrueType glyph outline is quadratic. The path model has no quadratic
verb, so every one is converted to a cubic before it is flattened — extra
arithmetic and extra error on the single hottest path in the engine, for
nothing. Plan 07 says explicitly that this must not happen. When this is done,
quadratics are first-class. (S)

## What is wrong

`crates/tinker-pdf-raster/src/geom.rs`:

```rust
pub enum Verb { MoveTo, LineTo, CurveTo(Point, Point, Point), Close }
```

Plan 07 (`07-rasterizer.md:17-20`) says the opposite:

> Path model: `MoveTo` / `LineTo` / `QuadTo` / `CubicTo` / `Close` over `f32`
> points. Quadratics are first-class, not up-converted — TrueType glyph
> outlines are quadratic and up-conversion adds float work and error for
> nothing.

The conversion is in `Renderer::show_glyph`, raising each quadratic to a cubic
with the two-thirds rule. It is the only up-conversion in the tree.

It also carries a latent bug: the helper that finds the current point returns
`None` after a `Close`, so a quadratic immediately following a close uses its
own endpoint as the current point. That produces a degenerate curve rather
than the intended one. Rare, since a contour rarely opens with a quadratic
straight after a close — but it is wrong, and a first-class verb removes the
helper along with the bug.

(Points are `f64` rather than plan 07's `f32`. That divergence is deliberate
and is not in scope here — `f64` is what the rest of the geometry uses.)

## Scope

- `Verb::QuadTo(Point, Point)` on the path model.
- `Path::quad_to`.
- Quadratic subdivision in the flattener, beside the cubic one.
- `show_glyph` emits quadratics directly.
- Delete the up-conversion and the current-point helper with it.

## Non-goals

- **Changing `Point` to `f32`.** A separate decision with its own consequences
  for the fill accumulator.
- **A quadratic in `PathSegment`.** PDF content streams have no quadratic
  operator, so the content-level path model does not need one. Only the raster
  path model does.

## Design

Quadratic subdivision is de Casteljau at the midpoint, exactly as the cubic
case already is — one fewer control point and one fewer level of interpolation.
The flatness test is the distance from the control point to the chord, which
is cheaper than the cubic's two-point test.

The existing `MAX_VERBS` cap and per-curve point cap apply unchanged.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `QuadTo` verb, constructor, flattener arm | A quadratic and its exact cubic equivalent flatten to the same polyline within tolerance | S |
| 2 | `show_glyph` emits quadratics; up-conversion deleted | Glyph rendering is unchanged within the perceptual budget; a glyph whose contour opens with a quadratic after a close renders correctly, where it previously did not | S |

## Dependencies

**Needs first:** nothing.

**Unblocks:** nothing, but it is on the path every glyph takes, so it lands
best alongside the other pixel-moving work rather than on its own.

## Risks

| Risk | Mitigation |
| --- | --- |
| Glyph goldens move slightly, since the arithmetic changes even where the shape does not | Land with the other rasteriser work and re-baseline once |
| A quadratic flattener with a subtly different tolerance from the cubic one gives glyphs two different smoothness characters | Milestone 1's exit criterion compares a quadratic against its exact cubic equivalent, which is what pins them together |
