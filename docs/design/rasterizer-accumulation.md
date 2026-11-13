# Sort-free coverage accumulation

*For [ROADMAP](../ROADMAP.md) row P-07.* `fill` computes a coverage mask by
sampling each pixel row at sixteen sub-scanlines and measuring, on each, the
horizontal spans the fill rule calls inside in exact 1/256-pixel units
([rasterizer](../features/rasterizer.md)). The sorted sweep that defines
those numbers — edges sorted by their tops, an active-edge list per
sub-scanline, each sub-scanline's crossings sorted by `x` and swept into a `u16`
accumulator one span at a time — is kept, test-only, as `fill_reference.rs`.
This document fixes how the same numbers are computed without either sort: from
integer cells, one row at a time, prefix-summed along the row. It also fixes
where the fill's memory lives, the two fast paths that skip work the cells show
is constant, how the stroker hands its outline to the cells directly, how
dashes are cut without collecting them, and how the hot loops are written for
the `+simd128` build.

## Scope

- **The contract**: the bytes today's sweep produces, under both fill rules,
  for every path, region and tolerance, including windings past one and
  several crossings in one pixel on one sub-scanline.
- **Cells and lanes** (P-07.3): the deposit, the row's prefix sum and its
  conversion to bytes, a lane holding several crossings, and a sub-scanline
  whose crossings do not close.
- **The band and memory plan, and the stop hook's partial mask.**
- **Scratch** (P-07.4): what a caller owns and every fill reuses.
- **Solid spans** (P-07.5) and **the opaque Normal composite** (P-07.6).
- **The stroker's sink** (P-07.7) and **dash cutting** (P-07.8).
- **The `+simd128` loops** (P-07.9) and the benchmark operations that carry
  the relation.
- **The counted guard** of every step, each a test-only counter read by a
  unit test.

## Non-goals

- **Changing `SAMPLES` or any coverage value.** Every fingerprint, golden and
  `analytic_coverage.rs` assertion holds as it stands; the design is a
  different way to the same integers, never a different integer.
- **Floating-point area coverage.** An area integrator is not bit-identical
  across targets (ruling 4), and it is a different sampling model.
- **`unsafe` and SIMD intrinsics.** H-16's answer: autovectorisable integer
  loops over slices, nothing else.
- **The SVG writer's `dash`**, which keeps its callback, its tolerance and its
  100 000-step bound per segment, and **the pattern fill's seam**: a pattern
  paints through the coverage mask of its area, and a pattern stroke's mask
  comes from the same cells a solid stroke's does.
- **The image run and the mesh's colour walk.** Both take their coverage from
  `fill` and move with it; neither is otherwise touched.

## Design

### What the sweep computes

A crossing is an edge meeting a sub-scanline: its position is
`x = (dxdy·(sub − top) + x_top + 2^15) >> 16` in 1/256-pixel units, in the page
frame, with every operation saturating, and its winding is the edge's ±1.
Write `W_s(k)` for the sum of the windings of the crossings on sub-scanline `s`
at or left of unit `k`, and `I` for the rule: `I(w) = (w ≠ 0)` for non-zero,
`I(w) = w & 1` for even-odd. The sweep adds a span `[a, b)` when a crossing
takes the rule from outside to inside at `a` and the next that takes it back
outside is at `b`. **When a sub-scanline's crossings close** — their windings
sum to a value the rule calls outside, which is true of every sub-scanline of
every closed path — every span it opens it closes, and a pixel `p` of the region
receives on `s` exactly the count of units `k` of `p` with `I(W_s(k)) = 1`,
clipped to the region. Spans cannot overlap within one sub-scanline, so a pixel
receives at most 256 a sub-scanline and 4 096 a row, and its byte is
`min(total / 16, 255)`. That count is a function of the crossings alone and not
of their order: equal positions give zero-length spans.

Every subpath is closed before it is filled (8.5.3.1), so a sub-scanline fails
to close only where `build_edges` drops an edge that covers one: a segment with
a point past the finite, a `y` whose sixteenfold is not finite, a `dy` under
`f64::EPSILON` across a sub-scanline, or a slope or first crossing that is not
finite. A segment
whose ends round to one sub-scanline covers none, and dropping it closes
everything it would have closed.

### Cells and lanes

The fill works **one pixel row at a time**. A row is a line of **cells**, one
per pixel of the region's width, and a cell has sixteen **lanes**, one per
sub-scanline of the row:

| Field | What it holds |
| --- | --- |
| `lanes: u16` | the sub-scanlines with at least one crossing in this pixel |
| `multi: u16` | the sub-scanlines with two or more |
| `w: [i8; 16]`, `off: [u8; 16]` | a lane's first crossing: its winding and its offset in the pixel, `x & 255` |
| `more: u32` | the pixel's block of sixteen list heads, one a lane, for the crossings after a lane's first; none until a lane holds two |

A crossing is placed by integer arithmetic alone: `rel = x − 256·x0`; a
negative `rel` adds its winding to the row's **left lanes**, the sixteen
windings of the crossings left of the region; a `rel` at or past `256·width`
is dropped, since nothing in the region lies right of it; anything else lands
in cell `rel >> 8`, lane `s`, at offset `rel & 255`.

**The prefix sum** is the left-to-right walk of the row's cells carrying
`W[16]`, the winding of each lane at the walk's position, from the left lanes,
and `inside`, the count of lanes `I` calls inside. At a cell with no crossing
`W` does not change, and the cell's byte is `min(16·inside, 255)`: every lane
inside covers its 256 units. At a cell with crossings, the total starts at
`256·inside` and each lane holding one crossing at offset `o` with winding `w`
adds `(256 − o)·(I(W + w) − I(W))` as it moves `W` and `inside` — the units
from the crossing to the cell's right edge change sides together, and the
lanes are independent, so the order the lanes are taken in decides nothing.
The byte is `min(total / 16, 255)`. That total is the sweep's, unit for unit:
the winding at a cell's left edge is the sum of every crossing left of it on
its lane, which is what the sweep reaches there by sorting.

### A lane holding several crossings

The order matters only inside one lane of one cell, where the units between
two crossings take the winding of whichever comes first. Such a lane is
**bucketed by offset, never compared**: its crossings' windings are added into
a 256-entry table at their offsets and a 256-bit map marks which offsets are
taken; the map is read upward by `trailing_zeros`, and each offset applies
`(256 − o)·(I(W + d) − I(W))` with `d` the winding summed there. Several
crossings at one offset sum before they apply, which is the sweep's
zero-length span. Each entry read is zeroed, so the table is clean for the
next lane. The work is the lane's crossings plus four map words, whatever their
number — a mesh of a thousand triangles in one pixel costs no more per
crossing than a vertex does.

**So a fill orders nothing.** No edge is compared with another, no crossing
with another, and no list is sorted, at a triangle's vertex or anywhere else.
The counter of comparisons the guard reads (below) stays at zero on every
closed path.

### A sub-scanline that does not close

A fill whose `build_edges` dropped an edge that covers a sub-scanline is
marked. Its rows are read once before their cells are filled: each lane's
windings over every edge that covers it are summed, and a row any of whose
lanes sums to a value the rule calls inside is swept as the sweep sweeps it —
that row's crossings gathered with their edge's top and its index in the path,
sorted by position, then top, then index (the order the stable sorts of the
sweep put them in), and walked into a `u16` accumulator span by span, a span
added only where it closes. Every other row of the fill takes the cells. The
row's sort is the only comparison a fill can make, it is counted, and no closed
path reaches it.

### Edges, rows and the stop hook

`build_edges` keeps the edges in the order the path gives them and buckets
each by the row of the region it starts in — `top` as a row, clamped to the
first active row — as a list through a `next` index per edge. **A row**:

1. asks `stop` when it is the 0th, 16th, 32nd … row of the active range,
   before any of its cells are filled, and returns the mask as it stands when
   it answers yes;
2. appends its bucket to the active list;
3. has every active edge deposit its crossings on the row's sixteen
   sub-scanlines it covers;
4. walks its cells into the mask row, clearing each as it goes;
5. drops the edges that end inside it from the active list, by swapping the
   last into the hole — the order of the active list decides nothing.

The rows are the sweep's own: the first row any edge reaches to the last,
clamped to the region, and a sub-scanline is `row·16 + s` with the sweep's
saturating arithmetic. **A stopped fill keeps every row it had finished and
nothing below**: the question comes before a row's deposit, so no row is ever
half-converted, and the cells are clear whenever the question is asked.

### Memory

One row of cells at a time: forty bytes a pixel of the region's width, reused
for each row and cleared as it is converted. The edges, a `next` and a bucket
head per active row, the active list, the lists of a lane's later crossings
(eight bytes each, reset each row), the 256-entry table and its map, and for a
marked fill the row's gathered crossings and a `u16` accumulator the width of
the region. Nothing is sized by the region's height but the bucket heads. A
region is the shape's bounds clipped to the clip's rectangle and the canvas, so
the cells are as wide as what is drawn.

### Scratch

The cells, the edges, the buckets, the active list, the later crossings, the
table and the row buffers live in a **`FillScratch` the caller owns**, grown
when a fill needs more and never shrunk; `fill_with(&mut scratch, …)` fills
through it, and `fill(…)` is a fill through a scratch of its own. The flattener
hands its points straight to `build_edges`, so a path is never collected into
polylines on its way in. `tinker-pdf-render`'s `Renderer` holds one scratch and
threads it through every path, clip, glyph, image-quad and mesh fill, so a
group's buffer, a soft mask's and a tile's canvas share the renderer's; a
tiling cell, drawn by a renderer of its own, has its own. **Nothing left in the
scratch reaches a pixel**: a fill begins by emptying the edges, the active list
and the later crossings, the cells and the table are clear between rows by
construction, and a stop leaves them clear.

### Solid spans

In a row, the cells between two cells holding crossings share one byte, the
one `inside` gives. The row keeps a **bitmap of the cells holding a crossing**,
set by the deposit and read word by word with `trailing_zeros`; the walk jumps
from one set cell to the next, and the run between them is converted once and
written as one slice fill — 255 where every lane is inside, `16·inside` where
some are, and nothing at all where none is, the mask being zero already. A run
ends at the first cell holding a crossing on any lane, so a pixel that reaches
255 only by the clamp still goes through its cell.

### The opaque Normal composite

`Canvas::fill_mask_inked` takes a fast path when the blend is `Normal`, the
colour's alpha and the state's alpha are both 255, the canvas carries no
initial backdrop and its format is not `CmykA8`. **A destination is opaque**
when its format carries no alpha (`Gray8`, `Rgb8`) or its pixel's alpha byte is
255. Over such a pixel `blend`'s arithmetic collapses: the union alpha is 255,
the separable mix is the source channel, and the result is
`min(cs·c/255 + cb·(255 − c)/255, 255)` per colour channel (`mul255`, rounded),
the alpha staying 255. So a mask row is read as a slice. Where the format
carries no alpha, a run of 255 is written as the encoded colour in one slice
copy, from a row of it the canvas keeps; over an alpha-carrying destination
each pixel of a run whose alpha byte is 255 takes the encoded colour. A partial
pixel over an opaque destination takes the opaque-backdrop arithmetic alone.
Any pixel over a destination that is not opaque, and every case outside the
conditions — the ink canvas, a non-isolated group's backdrop, any alpha below
255, any other mode — keeps `blend`, whose rounding a non-opaque destination
needs.

### The stroker's sink

`stroke` and `stroke_mapped` write each piece of the outline — a segment's
quad, a join, a cap — through a sink: a `Path` for a caller that wants the
outline, or the scratch's edges for one that wants coverage. `emit`'s rule
moves into the sink's one method: a piece is mapped first where the pen has a
map, its orientation measured, and its points taken in the order that makes
it wind as every other piece does — or overlapping pieces cancel and a round
cap vanishes. A piece holds at most sixteen points (a circle's `ARC_STEPS`), so
the mapping and the reversal use a fixed array. The renderer strokes paths,
stroked glyphs and pattern strokes into the edges; the region is the scratch's
bounds of the points the pieces gave it — the bounds `Path::bounds` gives the
outline, which holds no curve — clipped as a path's is; and the sweep is the
fill's. The input path is flattened once, by the stroker, and the outline never
is. The text clip accumulates glyph fill paths and the SVG writer calls only
`dash`, so nothing in the renderer still needs an outline `Path`.

### Dash cutting

`each_dash` keeps the dash in hand in **one buffer**, reused, and hands it as a
slice to the stroker's outliner as soon as it ends, then clears it; so the
`Vec<Vec<Point>>` of pieces goes (`apply_dashes` with it) and no piece
allocates. `dash` keeps its callback, handing the same slice. The stop
predicate is still asked every 1 024 steps, and a stop outlines the piece in
hand and keeps every piece outlined before it.

### The `+simd128` loops

The loops that run per pixel are written as fixed-width integer loops over
slices, bounds checks hoisted out and no call inside, so that the `+simd128`
wasm build vectorises them and the plain one does not need to:

- **a touched cell's lanes**, the sixteen windings, offsets and insides as one
  16-wide loop of `i32` with no branch, the lane mask applied as a multiplier;
- **the solid span**, a slice fill;
- **the opaque composite**, the mask row and the canvas row walked in
  `chunks_exact` of the pixel's components, the run copy a slice copy;
- **the deposit of an edge whose crossings cannot saturate** — decided once
  per edge from its `x`, its slope and its height — as a plain wrapping loop
  over its sub-scanlines in the row; an edge that could saturate keeps the
  saturating form.

Integer lanes give the same bytes vectorised or not, so ruling 4 holds by
construction. **The relation** — fills at least 2× faster with `+simd128` than
without — is carried by `render/300 triangles at 150 dpi` (the 300 triangles,
`triangles()`, under the id MS-04.10 gives today's `render/fill 300
anti-aliased paths at 150 dpi`), `render/250 paths at 150 dpi` and
`render/100 paths at 300 dpi`, and committed as those three operations'
`+simd128` gain figures in `crates/tinker-pdf/benches/wasm.json`, which MS-10's
`bench-check --wasm` reads. `render/the 100 stroked curves alone at 150 dpi`
does not carry it: its time is the stroker's floating-point geometry, which
these loops do not reach.

### The counters

Each guard reads a counter compiled only under `cfg(test)` and kept per thread,
so tests running side by side cannot add into each other's: comparisons made
ordering edges or crossings (P-07.3), crossings that share a lane of a cell
(read beside it), allocations a fill makes besides its mask (P-07.4), cells
converted (P-07.5), `blend` calls (P-07.6), flatten passes (P-07.7), and dash
pieces held and allocated (P-07.8).

## Milestones

1. **P-07.2: the safety net**, before anything changes. Today's sweep kept as
   `fill_reference.rs`, registered `#[cfg(test)]` in `lib.rs`, the reference
   every later step's differential test calls. Exit: `fill_reference.rs`
   `fill_is_the_reference_sweep_on_generated_paths` passes against today's
   `fill` under both rules at random regions, over generated paths of seven
   classes — overlapping same- and opposite-wound subpaths, self-intersections,
   several crossings in one pixel on one sub-scanline, slivers under a
   sub-scanline, coordinates near 1e9, stroker outlines of every cap, join and
   dash, and subpaths with an edge `build_edges` drops. **Its counted guard**:
   the harness counts the cases each class ran and the crossings that shared a
   pixel on a sub-scanline, measured by the reference's own sweep, and fails if
   a class ran no case or the sharing class shared nothing, so it cannot pass
   by generating nothing. The `strokes` fingerprint — every cap and join, a
   miter past its limit, dashes with phases on a curve, an even-odd star —
   joins `rendering_is_stable_across_targets`, reproduced under `cargo test` on
   Linux and Windows and under `--target wasm32-wasip1`; its page tiles in
   `render_regions.rs`'s `every_tile_is_byte_equal_to_the_page_under_it`; and
   `determinism.md` names it.
2. **P-07.3: cells.** Exit: `fill.rs` `a_fill_sorts_nothing`. **Its counted
   guard**: the comparisons made ordering edges or crossings are zero on every
   closed generated path, generated triangles among them, and so at most the
   crossings sharing a pixel on one sub-scanline, counted beside them;
   `fill_is_the_reference_sweep_on_generated_paths`, `analytic_coverage.rs`,
   `a_stopped_fill_keeps_the_rows_it_had_already_swept`, every fingerprint,
   `every_golden_is_what_this_build_draws` and `render_regions.rs` unchanged,
   the facade's suite in the default build and with `bundled-fonts`, and the
   guard run under `--target wasm32-wasip1` (MS-01's leg).
3. **P-07.4: scratch reused.** Exit: `fill.rs`
   `a_fill_after_the_first_allocates_no_scratch`. **Its counted guard**: the
   allocations `fill_with` makes besides the mask it returns are zero from the
   second fill on when no fill is larger than the first; the harness run back
   to back over shapes of different sizes through one scratch; every
   fingerprint, golden and `render_regions.rs` unchanged, in both builds.
4. **P-07.5: solid spans.** Exit: `fill.rs` `an_interior_run_is_one_slice_fill`.
   **Its counted guard**: the cells converted in a row are at most the cells
   holding a crossing plus one for each run before, between and after them;
   the harness, every fingerprint, golden and `render_regions.rs` unchanged.
5. **P-07.6: the opaque Normal composite.** Exit: `canvas.rs`
   `the_opaque_path_is_the_general_blend_where_it_applies`, exhaustive over
   coverage, source and destination channel on every format whose conditions
   hold, byte-identical to `blend`; and `an_opaque_normal_fill_calls_no_general_blend`.
   **Its counted guard**: `blend` calls, zero for an opaque Normal fill onto an
   opaque canvas; `blend_modes.rs`, every fingerprint and golden unchanged.
6. **P-07.7: the stroker's sink.** Exit: `stroke.rs`
   `the_direct_outline_fills_as_the_path_outline_did`, the cells' mask equal
   to `fill` of the returned `Path` byte for byte over every cap, join, miter
   limit, dash, mapped pen and floor; and `a_stroke_is_flattened_once`. **Its
   counted guard**: flatten passes per stroke, one; `stroke_parameters.rs`,
   the `strokes` and `curves` fingerprints, `path-stroke.ppm` and
   `render_regions.rs` unchanged, in both builds.
7. **P-07.8: dash cutting.** Exit: `stroke.rs`
   `a_dashed_stroke_holds_one_piece_at_a_time`. **Its counted guard**: a gauge
   of pieces held at once, one, and allocations per piece, zero;
   `a_dash_pattern_removes_area`,
   `a_stopped_dash_expansion_keeps_the_pieces_it_had_measured`,
   `a_dash_of_no_length_costs_no_step_of_the_walk`, `stroke_parameters.rs`,
   `svg_output.rs`, `svg_dash_memory.rs` and the `strokes` fingerprint
   unchanged.
8. **P-07.9a: the loops**, as laid out above. Exit: MS-10's local run reports
   `+simd128` over plain at or above 2× on the three operations above,
   committed as their gain figures in `wasm.json`. **Its counted guard**: the
   guards of P-07.3 to P-07.8 read what they read before the reshaping, so no
   loop does more work for being vectorisable; the harness, every fingerprint
   on the four targets and every golden unchanged. **P-07.9b** takes the figure
   in the timing phase, and `cargo xtask baseline` holds it.
9. **P-07.10: the row's close** in MS-02's paired run: 250 paths at 150 dpi at
   2.94×, 100 paths at 300 dpi at 3.12×, the 250-path page's 100 stroked curves
   alone at 2.94× paired with the planning baseline's same curves, and 300
   triangles at 150 dpi at 2.2×, with MS-10's run holding the relation. **Its
   counted guard**: every counter above green in `cargo test` on Linux and
   Windows and under `--target wasm32-wasip1`, each guard in the determinism
   roster; `rendering_is_stable_across_targets` unchanged on the three, and
   the facade's suite in both builds.

## Risks

| Risk | Mitigation |
| --- | --- |
| A tile's cell disagrees with the page's (ruling 5) | **Cells are indexed from the page frame, never a tile's**: a crossing's position is computed in device units of the page, the cell is `(x >> 8) − x0` and the offset `x & 255`, an integer subtraction and a mask that do not depend on the region; the left lanes fold the crossings left of a region into the winding the page's prefix sum reaches at that column. `render_regions.rs` runs every step, the `strokes` page among its fixtures |
| The stop hook's partial mask changes | The question comes before a row's deposit, at the sweep's own cadence of every sixteenth active row, so a stop keeps the rows finished and zero below; `a_stopped_fill_keeps_the_rows_it_had_already_swept` and `a_predicate_that_never_answers_yes_changes_no_pixel` unchanged, and a stopped fill leaves the scratch clear |
| Windings past one, or several crossings in a lane, come out other than the sweep | The per-lane winding is exact in `i32`, the rule is applied per lane, and a crowded lane is ordered by its offsets; the harness's overlap, self-intersection, sharing and stroker classes compare every byte with the reference |
| A path whose sub-scanline does not close reads differently | The marked fill's open rows are the sweep itself, in the sweep's order; the harness's dropped-edge class compares them |
| A crowded pixel costs quadratic time | No lane is sorted; a lane costs its crossings and four map words |
| A wide region's cells cost more memory than its accumulator did | Forty bytes a pixel of one row, against two; the region is the shape's bounds clipped, and the row is reused, so the cells never exceed forty rows of the mask |
| Stale cells from a previous fill reach a pixel | Every cell is cleared as it is converted and the table as it is read; the harness runs back to back through one scratch |
| The opaque path rounds where `blend` does not | Exhaustive over coverage, source and destination on every eligible format |
| The direct outline winds differently from the `Path` it replaced | The sink applies `emit`'s orientation rule itself, and the direct outline is compared with `fill` of the `Path` over every cap, join, dash, map and floor |
| The `+simd128` relation is out of reach | Measured in the timing phase, never eased: an operation short of 2× is a finding against the loop that owns it, and only the owner re-states the figure (H-77, H-78) |
