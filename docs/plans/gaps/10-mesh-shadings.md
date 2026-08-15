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

---

## As built — August 2026

Five milestones, five commits, in the table's order.

### Why this was built, said plainly

**This was built on an explicit decision, not on a hit rate.** The corpus ran
before it and put mesh shadings at **10 files of 4 525 — 0.2 per cent**, the
lowest of the three deferred capabilities, against JBIG2 at 2.3 per cent and
JPX at 0.4 per cent, with all ten in `pdfjs` and none in `verapdf` or `qpdf`.
Ruling 3 schedules a deferred capability by that number, and by that number
this was not next.

The reasoning that overrode it is the amendment's, and it is a real argument
rather than a courtesy: the three corpora pinned here are a browser's
regression suite, a conformance suite and a writer's test suite, and not one
of them samples design or CAD output, which is exactly where gradient meshes
live. 0.2 per cent is a fact about these corpora. It is not a fact about the
world, and nothing in the repository currently measures the world for this
construct. Whether that judgement was right is a question a corpus of
Illustrator and AutoCAD exports would settle, and adding one is gap 23's
lock file rather than this plan's.

Nobody reading this should take the work below as evidence that the corpus
asked for it. It did not.

### What was actually missing

Verified at `5adf502`, and the plan's "nothing is subtly wrong" held exactly.
`read_shading` handled `/ShadingType` 1, 2 and 3, returned `Err(kind)` for
4 through 7, and that surfaced as `RenderWarning::UnsupportedShading { kind }`
with the area left blank — through `sh` and through a `PatternType 2` alike,
where the pattern reported itself as `PatternPaint::Unsupported`. The
degradation was correct, complete and reported. This is a missing feature, and
the only thing wrong with the old behaviour was that it was not the feature.

### The seam, and how the absence of one is proved

The design constraint the plan is most emphatic about is one buffer, and it is
right to be. Filling a mesh triangle by triangle through the ordinary path
anti-aliases every *shared* edge against the backdrop: two neighbours each
contribute about half coverage there, source-over composites the two to about
three quarters, and the mesh acquires a lattice of pale lines that reads as a
bug in the blend rather than in the shading.

`tinker_pdf_raster::mesh::draw_mesh` rasterises a whole mesh into one buffer:

- **coverage** from a single `fill` over one path holding every triangle, under
  the non-zero rule. That fill accumulates *signed* area, so two triangles
  meeting along an edge contribute half a pixel each and the pixel comes out
  whole. This is the mechanism, and it is the existing, already-deterministic
  scanline fill rather than a second rasteriser;
- **colour** from a scanline walk per triangle that interpolates the vertex
  values barycentrically. Overlapping writes along a shared edge are harmless
  because linear interpolation along an edge depends only on the two vertices
  both triangles share, so the two triangles agree there by construction.

The silhouette keeps its anti-aliasing, which one-buffer meshes usually give
up. A pixel on it can have partial coverage and contain no triangle's centre,
so the colour buffer is spread two rounds of four-neighbour into pixels the
coverage mask reaches and no interior did — from a snapshot each round, so the
answer does not depend on which way the scan runs. A triangle thinner than a
pixel contains no centre at all and drops its centroid colour into one pixel,
so the spread has something to work from.

**The proof is an injection, and it is a measurement.** One anti-aliased fill
per triangle, composited source-over, was substituted for the single fill, and
the workspace was re-run:

| What | Correct | Per-triangle fill |
| --- | --- | --- |
| Coverage along a shared edge | 255 | **192** |
| A flat mid-grey square, on white, at the diagonal | 128 | **159** |
| A flat Coons patch against the two triangles it equals | within 2 levels | **79 levels apart** |

The third row is the one that shows why this matters at scale: a patch is
subdivided into hundreds of small triangles, so the seams are not one line but
a lattice across the whole surface. Ten assertions caught it, including both
dedicated no-seam tests and the fingerprint.

### How the subdivision is target-stable

Gap 13's finding is the one that shaped this. A recursive subdivider that
terminates on a floating-point comparison changes the *number* of segments on
a 32-bit target rather than their positions, and a different number of
segments is a different mesh rather than a rounded one. A patch is that trap
one dimension up, and it has a second edge: the count has to come from
**device** space, because a patch subdivided in shading space is far too
coarse at 300 dpi and far too fine at thumbnail size — which is why `Mesh`
holds patches and `Mesh::tessellate` takes the matrix rather than the reader
baking in a grid.

`patch_steps` is a fixed step count from a fixed-point measure, exactly as the
plan's risk row prescribes:

1. the measure is the longest control-polygon leg-sum among the patch's four
   boundary curves — the same quantity gap 13's flattener takes its step count
   from — in the **Manhattan** metric, so no square root is involved at all;
2. it is built in `f64` from nothing but subtraction, addition and `abs`, all
   of which IEEE 754 pins exactly;
3. it is cast **once** into 16.16 fixed point, and every comparison after that
   is between two `i64`;
4. the loop **doubles its threshold** rather than halving the measure, because
   shifting truncates and a measure a hair over the threshold would come out
   one doubling short — gap 12's finding in the image pyramid, in the same
   shape.

The count is therefore always a power of two, capped at 32 steps (2 048
triangles for one patch), and `the_subdivision_count_is_a_power_of_two_from_a_fixed_point_measure`
walks 200 sizes asserting both properties. Nothing on the path calls a
transcendental, which `cargo xtask libm` enforces.

Both targets agree on the mesh fingerprint, which is what turns that reasoning
into evidence: the fixture contains a Coons patch and a tensor patch, and
halving the step count moves its hash.

### Colour interpolation, and how it is proved

8.7.4.5.5's rule is that vertex values are interpolated and *then* converted.
The raster crate makes that structural rather than a convention: it
interpolates opaque numbers it cannot interpret and asks a `&dyn Fn(&[f64]) ->
Color` what they paint, per pixel, after interpolation. Ruling 8 is what forced
that shape — no colour space may cross into a leaf crate — and it is the same
seam `ImageDraw::stop` has used since gap 12.

With a `/Function` the vertex carries one parametric value and the component
count is **1** rather than the colour space's. Reading the space's count
instead cuts the stream up wrongly from the first vertex on, which is a
different failure from getting the colour wrong.

The assertion that carries this is a `/Separation` whose tint transform is
cubic: tint 0 is white, tint 1 is black, and halfway across the ramp the tint
is 0.508, whose cube is 0.131, so the correct grey is about **221**.
Converting the vertices to RGB first and interpolating those gives **125**.
Both are plausible gradients, and the ends agree either way, which is why the
assertion is on the middle. The determinism fixture's Coons patch sits in the
same space for the same reason.

### The work budget

Three ceilings at decode, and one at rasterisation. The last is the one the
fuzz campaign's finding at `5adf502` argues for: a depth or per-item cap is
not a work cap once the structure branches, and a patch branches.

| Bound | Value | What it stops |
| --- | --- | --- |
| `MAX_MESH_VERTICES` | 262 144 | `/BitsPerCoordinate 1` with one one-bit component packs a vertex into five bits, so a megabyte of stream is 1.6 million. Checked against the stream's **length before anything is reserved**, and again as vertices are appended |
| `MAX_MESH_PATCHES` | 16 384 | A patch costs far more than a vertex, because it subdivides |
| `MAX_MESH_TRIANGLES` | 131 072 | A **total**, not a per-patch limit: 16 384 patches each subdividing to 2 048 triangles is 33 million inside a per-patch cap that never fires |
| `MAX_MESH_WORK` | 67 108 864 | Charged over **both** rasterisation passes, because they run away for different reasons. The colour pass costs a triangle's clamped bounding box; the coverage pass costs three edges per sub-scanline crossed, so forty thousand page-tall slivers cost almost nothing in boxes and a great deal in crossings. A budget counting only boxes would not have seen them |

Every term is measured over bounding boxes already **clamped to the paint
region**, so a mesh written far off the page costs nothing, and the same mesh
drawn into a four-row clip fits where into a full page it does not. Past any
of them nothing is painted and the type is named — the answer `fill_with_tiles`
already gives a lattice that will not fit, and for the same reason: a fragment
reads as an artefact where a gap reads as a gap.

### Defect injection

Each applied alone, the whole workspace re-run with `--no-fail-fast`. Three of
the seven found blind spots, and the tests that close them exist because of
this exercise.

| Injected | Caught by |
| --- | --- |
| Colour converted at the vertices and interpolated in RGB | 2 — `colour_is_interpolated_in_the_shadings_own_space`, and the fingerprint |
| Edge flag 2 read as flag 1 | 2, then **3**. The end-to-end test only ever wrote flag 1, so swapping the reader's two arms moved no pixel on it. It now writes a flag 2 vertex placed where the two readings cover different pixels |
| `/Decode` x and y ranges swapped | 5 |
| `/BitsPerCoordinate` read from `/BitsPerComponent` | 1, then **2**. The fingerprint alone. Every end-to-end fixture wrote eight bits for all three widths, so a reader taking the wrong key was right by accident; one fixture now writes 16, 8 and 4 |
| One anti-aliased fill per triangle instead of one buffer | 10, with the numbers in the seam table above |
| The winding normalisation deleted | **0**, then 1 |
| The Coons interior replaced by a bilinear corner blend | 1, then **2**. The fingerprint alone, because the flat-patch unit test cannot separate them — a flat patch's interior is the flat grid under either rule. What separates them is a **bowed** boundary |
| The subdivision step count halved | 3, including the fingerprint |

**The winding finding is the most interesting of the eight, because the
mitigation was right and the reasoning behind it was wrong.** Deleting the
normalisation moved no pixel anywhere in the workspace. Two triangles that
merely *share an edge* are disjoint, and their two crossings at that edge land
on exactly the same `x` — the edge is built from the same two points, so the
same slope and the same intercept come out whichever order the triangle names
them — and opposite signs there cancel each other out of the sweep and leave
the spans contiguous either way. The shared edge was never the case that
needed normalising. The case that does is an **overlap**: a clockwise triangle
lying inside an anticlockwise one sums to a winding of zero and punches a hole
its own shape, and a folded gradient mesh overlaps itself by design. The test
and the module documentation now say that instead.

### The fingerprint

The eleventh, `mesh`, 120 by 80 points, 9 311 pixels of ink out of 9 600,
floor 4 600. None of the other ten moved, and none should have: not one of
them draws a mesh, so every fingerprint report made while this gap was being
built was true and meant nothing until this one existed.

Seven things in it are in no other fixture: a type 4 strip carrying all three
edge flags; a type 5 lattice through a `/Function`, with an interior row off
the grid so no cell is a rectangle; a type 6 Coons patch with two bowed edges,
in a `/Separation` with a cubic tint; a type 7 tensor patch whose four
internal control points are dragged off the flat grid; the subdivision count,
which comes from the device transform; a shading pattern over a mesh under a
rotated `/Matrix`; and every packed width class the spec allows — 16 bits per
coordinate, 12 (not a whole number of bytes), 16 and 4 bits per component, and
flag widths of 8, 4 and 2.

Two of those choices are load-bearing rather than decorative, and both are gap
12's lesson applied ahead of time rather than after: the cubic tint is what
lets the fixture see RGB interpolation, because a linear space agrees with
itself under either order, and the bowed boundaries are what let it see the
Coons interior formula, because a flat patch's interior is the flat grid
either way.

`wasm32-wasip1` under wasmtime 47.0.3 produces the same hash as native Windows
x86_64 — 32-bit against 64-bit, which is where a `usize` assumption or an
unstable count would show. The page is also committed as a `render_page` fuzz
seed, so the campaign reaches the packed reader, the patch subdivider and the
mesh rasteriser rather than stopping at `read_shading`.

### Amendments

`07-rasterizer.md`'s non-goals said mesh shadings are "decomposed by 08 into
filled geometry with solid colors" and that a vertex-interpolated triangle
primitive would be "added *here* but scheduled *there*" if that banded. It
banded, and worse than banded — the failure is a seam lattice rather than
banding — so the primitive was added, and the plan now says so rather than
still offering the decomposition as the design.

`08-rendering-device.md` said in three places that mesh types 4 to 7 "hit the
capability path". They do not any more, and its milestone 6 row said mesh
fixtures degrade with a warning, which is now only true of a mesh that cannot
be read at all.

### Not done

- **`/Background` and `/BBox` on a shading dictionary** are read by nothing,
  for any shading type. That is a pre-existing gap this work did not touch and
  did not widen; plan 08's design paragraph names both.
- **T-junctions between patches of different sizes.** Two adjacent patches are
  tessellated independently, so where they share a *curved* boundary and are
  different enough in size to pick different step counts, the two polylines
  approximating that boundary differ and a sub-pixel sliver can appear. The
  step count comes from the patch's own extent, so patches of similar size
  agree, and a straight shared boundary is identical at any count. Every other
  renderer that subdivides patches independently has this; removing it needs
  the neighbour's count, which the edge-flag continuation does not carry.
- **The `/Function` is evaluated per pixel**, as it already is for axial and
  radial shadings. Plan 08 describes a 1 024-entry LUT for shading colour
  ramps; there is no LUT on any shading path in this engine, and building one
  here alone would have made the mesh path the odd one out.
- **The four-target claim is still two targets.** Linux and macOS come only
  from the CI matrix, as gap 25 records.
