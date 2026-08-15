# Mesh shadings warn and skip

ShadingTypes 4 through 7 — free-form and lattice-form Gouraud triangles, and
Coons and tensor patches. Illustrator gradient meshes and most CAD colour maps
use them. The engine warns and leaves the area blank. When this is done, a
mesh shading paints. (M)

## What is wrong

Nothing is subtly wrong. `read_shading` handles types 1, 2 and 3 and returns
`None` for the rest, which surfaces as `RenderWarning::UnsupportedShading`
carrying the type number. The degradation is correct and reported.

This is a missing feature, and **ruling 3 says it should be scheduled by
evidence**: a deferred capability is built when the corpus hit-rate report
says real documents need it, not before. That report does not exist
([23](23-corpus-runner.md)). This document exists so the work is ready when
the evidence arrives — not so it can be started without it.

## Scope

- Type 4: free-form Gouraud triangles, with the per-vertex edge flag.
- Type 5: lattice-form, with `/VerticesPerRow`.
- Types 6 and 7: Coons and tensor patches, with the same edge-flag continuation
  scheme.
- The packed vertex stream: `/BitsPerCoordinate`, `/BitsPerComponent`,
  `/BitsPerFlag`, `/Decode`.
- `/Function` when present, which turns one parametric value per vertex into a
  colour instead of carrying components directly.

## Non-goals

- **Exact patch subdivision matching another renderer.** Coons patches are
  subdivided to a tolerance; different tolerances give different pixels. The
  bar is perceptual, against the corpus budget.

## Design

**One buffer, not one anti-aliased fill per triangle.** A mesh is thousands of
adjacent triangles. Filling each through the ordinary path anti-aliases every
shared edge against the background, producing a visible seam lattice — and
costs a full-canvas mask per triangle ([14](14-bounded-painting.md)
notwithstanding). Rasterise the whole mesh into one buffer with a scanline
walk, interpolating colour per pixel, then composite that buffer once.

**Patches subdivide to triangles.** Coons and tensor patches are bicubic
surfaces; subdivide adaptively to a flatness tolerance and hand the triangles
to the same rasteriser. That keeps types 6 and 7 a front-end on types 4 and 5
rather than a second implementation.

**Colour interpolation is per-vertex, in the shading's colour space**, then
converted — not converted at the vertices and interpolated in RGB, which
shifts the midpoints of any non-linear space.

**Determinism.** The subdivision tolerance and the interpolation arithmetic
must be target-stable (ruling 4). Fixed-point interpolation, or `f64` with no
transcendental calls — `tinker-pdf-math` exists for the latter.

## Where a half-implementation is worse than none

Types 4 and 5 without 6 and 7. A document mixing them — common, because a
gradient mesh exports as patches while its background exports as triangles —
would render half its shading and warn about the other half, which looks like
a corrupt file rather than a partial capability. If only half ships, the
warning must name the type, as it already does.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Packed vertex reader: coordinates, components, flags, `/Decode` | A hand-built stream at 8, 16 and 32 bits per coordinate reads back the vertices exactly | S |
| 2 | Triangle mesh rasteriser, one buffer | A two-triangle type 4 shading interpolates colour across the shared edge with no seam | M |
| 3 | Types 4 and 5 end to end | A lattice-form fixture renders; `/Function` and direct-component forms agree where they should | M |
| 4 | Patch subdivision; types 6 and 7 | A flat Coons patch renders identically to the equivalent two triangles | M |
| 5 | Determinism | The mesh fixtures join `tests/determinism.rs` and hash identically across targets | S |

## Dependencies

**Needs first:** [23](23-corpus-runner.md), for the evidence that schedules it
(ruling 3). Technically nothing.

**Unblocks:** nothing.

## Risks

| Risk | Mitigation |
| --- | --- |
| Adaptive subdivision that depends on floating-point comparisons can differ across targets | Fixed depth chosen from a fixed-point flatness measure; the determinism fixtures are milestone 5 and not optional |
| Per-triangle anti-aliasing produces a seam lattice that looks like a rendering bug and is very hard to attribute | One buffer, stated as milestone 2's exit criterion rather than left as an implementation note |
| Vertex streams are attacker-sized | The existing `Limits` ceiling; vertex count bounded before allocation, as the CCITT and JBIG2 paths do |

## Amendment — August 2026: the corpus evidence ruling 3 asked for

[23](23-corpus-runner.md) has run. Across 4 525 documents from pdf.js,
veraPDF, qpdf's qtest and the PDF Association:

**Mesh shadings: 10 files, 0.2 %** — the lowest of the three deferred
capabilities, against JBIG2 at 2.3 % and JPX at 0.4 %.

All ten are in `pdfjs`; `verapdf` and `qpdf` contain none. This document is
sized M and is five milestones of triangle rasterisation, patch subdivision and
packed vertex decoding — for ten files in the corpora this project pins.

Ruling 3 says a deferred capability is built when the corpus says real
documents need it. **The corpus does not say that yet.** What it says is that
the honest degradation this plan replaces — a named `UnsupportedShading`
warning carrying the type number — is what 0.2 % of files get, and that is
working as designed.

The counter-argument, which is real: the corpora pinned here are a browser's
regression suite, a conformance suite and a writer's test suite. None is a
sample of design or CAD output, which is exactly where gradient meshes live. A
corpus of Illustrator or AutoCAD exports would very likely move this number a
long way. Adding one is [23](23-corpus-runner.md)'s lock file, not this plan.

So: the work below stays ready, and the trigger is a corpus that represents the
documents this capability exists for — not the count above.
