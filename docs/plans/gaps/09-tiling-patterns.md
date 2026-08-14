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

---

## As built — August 2026

All five milestones, five commits. 1276 tests to 1304 — thirty-three of them
about tiling patterns, a net twenty-eight because two of the old ones were
rewritten rather than added to. The full gate green on every commit, including
the `wasm32-wasip1` determinism leg under wasmtime 47.0.3.

| # | Commit | What landed |
| --- | --- | --- |
| 1 | `8cf7b72` | `PatternPaint::Tiling`, `TilingPattern`, `TileRequest`, `Tile`, `GlyphSource::tile`, `PageResources::tile`, the lattice loop, and the two rewritten "stays blank" tests |
| 2 | `451c362` | The lattice's own evidence: steps against the box, the off-by-one, spill, the path and the clip |
| 3 | `3edc4b8` | Anchoring under a scaled *and* a translated CTM, on the fill path and the stroke path, plus two shapes sharing one lattice |
| 4 | `9e4c629` | `PaintType 2`, `Canvas::recolor`, the stroke slot's colour, and the ninth determinism fingerprint |
| 5 | `634e04b` | The three budgets exercised, recursion termination, and a pattern's own `/Resources` |

### The shape of it, and what the plan did not say

The design section's headline — rasterize once, blit across the lattice — is
what was built and it was the right call. Three things around it were not in
the plan.

**The seam had to split in two.** A cell is a *content stream*: running one
needs an interpreter and a resource dictionary, and `tinker-pdf-render` has
neither and must not grow either (ruling 8). So `GlyphSource::pattern` returns
the cell's *geometry* — `/Matrix`, `/BBox`, `/XStep`, `/YStep`, `/PaintType` —
and a second method, `GlyphSource::tile`, returns one rasterized cell for a
buffer the renderer has already sized. That split is not tidiness: it is what
lets every budget be checked from six numbers **before** anything is
rasterized, so a pathological `/XStep` costs a division rather than a cell.
The renderer keeps all the geometry, the resource layer keeps all the PDF, and
neither has half of the other's job.

**The form-recursion cap does not bound pattern recursion.** The risk table
says to reuse it. It cannot: a cell is run by a fresh `interpret` call, so the
interpreter's depth counter starts at zero every time and a pattern that fills
with itself recurses until the stack is gone. A separate `MAX_PATTERN_DEPTH`
is threaded through `TileRequest`, and it is **4** rather than the form cap's
16, because a form nested `n` deep costs `n` renders while a pattern nested
`n` deep costs the *product* of the lattice counts.

**One budget was not enough.** The plan says "cap the tile count". The count
alone does not bound the work, because `/XStep` may legally be much smaller
than `/BBox` — that is how an overlapping weave is drawn — and then every cell
costs its own area rather than its step's. A cell as large as the page stepped
a hundredth of a point is about 24 000 positions, comfortably inside a 65 536
cap, and 39 megapixels of compositing for one fill. Three numbers, each with a
distinct job:

| Bound | Value | What it stops |
| --- | --- | --- |
| `MAX_TILES` | 65 536 | The lattice itself. A 10 pt hatch over A4 at scale 1 is about 5 000 cells; a 2 pt one about 125 000, which is over |
| `MAX_TILE_AREA` | 16.7 Mpx | The cell's buffer, which is allocated. A memory bound rather than a time one |
| `MAX_TILE_WORK` | 33.5 Mpx | The product, over one fill |

Past any of the three the fill paints nothing and names the pattern.
Painting *part* of a lattice was rejected: a hatch covering the top third of a
shape reads as a rendering artefact, where an unpainted area reads as the gap
it is.

**A pattern's `/Resources` are honoured** — read into a `PageResources` of its
own, falling back to the page's when the key is absent, with the host's
`FontProvider` carried across so a cell is not the one place in a document
where substitute faces stop applying, and with the nested view's warnings
merged back into the page's (ruling 10). Gap 11's finding that a *form*
XObject's own `/Resources` are consulted nowhere is unchanged and is not this
gap's: a cell is a self-contained drawing where a form is usually written
beside the page that invokes it, so the pattern case had to be built and the
form case is a separate, still-open finding.

**Uncoloured cells are recoloured after the fact**, by a new
`Canvas::recolor` that replaces each pixel's ink and keeps its alpha. The
alternative — intercepting colour at every paint inside the cell — is four
call sites that can drift apart and a fifth the day something else learns to
paint. Flattening afterwards makes 8.7.3.3's "colour operators inside it are
ignored" true by construction, including for operators not written yet.

### The two rewritten tests

Both were pinning correct behaviour and both are now the anchoring assertion
the plan's own "worse than none" section asks for:

- `a_tiling_pattern_is_reported_rather_than_blacked_out` →
  `a_tiling_pattern_fill_does_not_move_with_the_transform`;
- `a_tiling_pattern_stroke_is_reported_rather_than_blacked_out` →
  `a_tiling_pattern_stroke_does_not_move_with_the_transform`.

Each renders one picture three ways — plainly, under a doubled CTM with halved
coordinates, and under a three-point translation — and asserts the three are
byte-identical. The scaled render catches a lattice whose *cells* follow the
transform; the translated one catches a lattice whose **phase** does, which is
the subtler half and the one this document says will never be reported. What
the old tests actually measured is kept as its own pair, on a pattern whose
cell cannot be read at all: that still warns and still paints nothing.

### Injections

Each defect was inserted deliberately and the whole workspace re-run with
`--no-fail-fast`.

| Defect | Caught by |
| --- | --- |
| Anchoring taken from the paint-time CTM | **Exactly 2** — the fill assertion and the stroke assertion. Nothing else in 1 304, including every "the hatch paints" and "the hatch repeats" test |
| The lattice range pairing the near edges rather than the opposite ones | 5 tests and the `tiling` fingerprint, led by the half-overlapping-cell one written for it |
| A cell spilling past its `/BBox` | 1 test and the fingerprint — **and nothing at all before that test was written** (see below) |
| `PaintType 2` reading the fill slot when stroking | 1 test and the fingerprint |
| All three budgets removed | The render crate's test binary **aborts** on a 160 GB allocation. With only the buffer bound restored it does not finish in four minutes, against 1.2 seconds with all three |
| The recolour skipped entirely | 3 tests and the fingerprint |

**The spill row is the useful one.** The first spill fixture — a cell drawing
four points past its box on every side, with a step of twice the box — does
*not* notice the `/BBox` clip being deleted, and cannot. With an axis-aligned
`/Matrix` the buffer **is** the box, so the overshoot falls off the buffer's
own edge and the plan's "impossible by construction" is literally true. Rotate
the matrix and it stops being impossible: the buffer is the axis-aligned hull
of a rotated square, half again as large, and the corners of that hull are
outside the cell. Without the clip they fill, and a lattice of diamonds comes
out as a grid of squares. `a_rotated_cell_is_still_clipped_to_its_box` was
written *because* the injection found nothing.

### The determinism fixture

`tiling`, 120x80, 4488 pixels of 9600, floor 2200. Gap 07's `pattern` fixture
is a `PatternType 2` shading — evaluated per pixel through an inverse
transform — and reaches nothing here. Six things in no other fixture: a
lattice rather than one cell, so the range arithmetic and its rounding to
device pixels are in the hash; a rotated pattern `/Matrix`; steps that are not
the box, one wider and one narrower, so the gap and the overlap are both
measured; a cell whose content overshoots its `/BBox` deliberately; a stroked
outline filled with a lattice under a non-identity paint-time CTM; and a
`PaintType 2` cell taking its colour from the **stroking** slot.
`wasm32-wasip1` and `x86_64-pc-windows-msvc` agree on the hash, and **none of
the eight existing fingerprints moved** on either target. This discharges gap
[25](25-wasm-determinism-leg.md)'s milestone 4 for this plan.

### Not done, and deliberately

- **Pattern-space caching across fills.** A non-goal in the plan and still
  one. Every fill rasterizes its own cell; two fills with one pattern
  rasterize it twice.
- **Sub-pixel lattice placement.** Positions round to whole device pixels, so
  a rotated or non-integral lattice carries up to half a pixel of placement
  error per cell. That is the price of rasterizing once, and
  [08](../08-rendering-device.md) is amended to say so — its two-strategy
  design was rejected as unaffordable in one branch and as two sets of
  rounding in the other.
- **`/TilingType`.** Table 75's spacing hint (constant, no distortion, faster
  tiling) is read nowhere. It selects between the same three renderings this
  build already collapses to one, and honouring it would mean the sub-pixel
  placement above.
- **A distinct warning for a budget refusal.** Over budget reports
  `UnsupportedPattern` with the pattern's name, the same as a cell that cannot
  be read. A caller's action is the same either way — this pattern was not
  painted, and here is which — and a second variant would have made
  `UnsupportedPattern` mean different things on two routes that share one
  function, which is exactly what gap 07 spent its effort undoing.
