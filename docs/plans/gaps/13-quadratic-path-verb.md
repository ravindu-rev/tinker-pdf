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

## As built — August 2026

Two commits, one per milestone. Both defects were live exactly as described.

**The verb.** `Verb::QuadTo(Point, Point)`, `Path::quad_to`, arms in `push`
(so a non-finite control point drops the verb like any other) and in `bounds`
(so a control point still widens the box), and `subdivide_quadratic` beside
`subdivide` in the flattener. `MAX_VERBS` and the 512-step per-curve cap apply
unchanged. Only two places in the tree matched on `Verb` and both are in this
change; the fuzz targets, the bindings and the stroker all reach paths through
`flatten`, so a new variant did not disturb them.

**The Design paragraph is amended rather than followed, and so is plan 07's.**
Both describe recursive de Casteljau subdivision with a flatness test. The
cubic arm is not that and never was: it takes a fixed step count from the
control polygon's length and samples at evenly spaced parameters, because a
recursive subdivider terminates on a floating-point comparison and a
comparison that lands differently on a 32-bit target produces a different
*number* of segments, not a slightly different one. Ruling 4 overrides a phase
plan; the quadratic arm mirrors the cubic arm as it is.

**How the two arms are pinned together.** A quadratic's step count is measured
through the cubic it is equal to — two thirds of each leg plus a third of the
chord, which is the degree-elevated control polygon's length. Measuring the
quadratic's own two legs was the obvious alternative and would have broken the
same glyph curve into up to 22 per cent more pieces when a font described it
as a quadratic than when it described it as a cubic, which is this table's
second risk. `a_quadratic_flattens_like_its_exact_cubic_equivalent` raises
seven curves by the two-thirds rule and flattens both forms at five
tolerances: same point count, and the worst pair 3.6e-12 apart — about 4e-16
relative, two units in the last place — against a 1e-9 bound that is itself
four orders below the rasteriser's 1/256 snap.

**The latent bug was real, and was demonstrated before it was fixed.** It is
not reachable from a `glyf` outline, because every contour there begins with a
`MoveTo`; it is reachable through `GlyphSource`, which is a public trait any
host can implement. `ClosedThenCurved` returns an outline whose second contour
opens with a quadratic straight after a `Close`. Against the old code, device
pixel (4, 28) was white — the fallback had collapsed both control points onto
`end + 2/3(control - end)` and leant the arch the other way — the glyph
painted 414 pixels where the correct shape paints 457, and **138 of 1600
pixels differed**, 8.6 per cent of the canvas. After the change the same glyph
is byte-identical to the same curve drawn as an explicit cubic in a content
stream.

**The `text` fingerprint moved, and only that one.** Four of the six shapes in
`determinism.rs`'s synthetic face carry off-curve points, so every one of them
used to be raised before it was flattened. Measured across all six fixtures,
before against after: `curves`, `shading`, `blend`, `pattern` and `optional`
are byte-identical, and `text` differs in **7 of 20 000 pixels — 0.035 per
cent — each by exactly one level of 255 on all three channels**, with the ink
count (1486) and the ink bounding box ((10, 30)–(182, 59)) unchanged. That is
the arithmetic-only move this table's first risk predicted, and it is inside
any perceptual budget. `98c3e73c…` becomes `b0bc9383…`.

There is no golden re-baseline to land alongside: the risk row's mitigation
refers to Tinker's MuPDF goldens in another repository. The rule here is the
opposite one, and `wasm32-wasip1` produced the same new hash as native
Windows x86_64 before the table was touched — 32-bit against 64-bit, which is
what says this is a rendering change rather than a determinism bug.

**Injection.** Three defects, each with `cargo test --workspace
--no-fail-fast`:

| Injected | Caught by |
| --- | --- |
| The quadratic arm given `tolerance * 2.0` | `a_quadratic_flattens_like_its_exact_cubic_equivalent`, `a_quadratic_after_a_close_starts_where_the_subpath_did`, `a_quadratic_glyph_matches_the_same_curve_drawn_as_a_cubic`, `rendering_is_stable_across_targets` |
| The midpoint weight moved from 2.0 to 1.0 | the same four |
| The up-conversion restored verbatim, current-point lookup and all | `a_glyph_quadratic_after_a_close_starts_at_the_subpath_start`, `a_quadratic_glyph_matches_the_same_curve_drawn_as_a_cubic`, `rendering_is_stable_across_targets` |

Nothing else in the workspace moved for any of the three. The two flattener
defects are invisible to `a_glyph_quadratic_after_a_close_starts_at_the_subpath_start`,
whose probe has four pixels of margin, and the after-close defect is invisible
to every geometry test, because it never lived in the geometry. Neither test
covers the other, which is why both exist.

One methodology note. The comparison against an explicitly drawn cubic caught
a defect in its own fixture before it caught anything else: the triangle that
exists only to put a `Close` in front of the curve was wound against the arch,
so under the non-zero rule it punched a hole worth 5 pixels at up to 203
levels. Written the other way it contributes nothing, which is what the
fixture needs it to do. A tolerance-based assertion would have absorbed that
and the test would have looked fine.

**Non-goals held.** `Point` is still `f64` and `PathSegment` still has no
quadratic. Plan 07 is amended in place on both counts — the verb set now
matches it, the `f32` line does not, and the plan says so rather than staying
half-true.
