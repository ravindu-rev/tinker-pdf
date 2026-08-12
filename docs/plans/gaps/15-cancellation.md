# A render cannot be stopped mid-operation

`RenderOptions.cancel` is documented as checked "between operations and
between scanline bands, so a cancelled render stops promptly". Three pixel
loops honour that; the rasteriser honours it nowhere. One fill of a
complicated path runs to completion however long it takes, and a UI that
cancels a render waits for it. When this is done, the documentation is true.
(S)

## What is wrong

Checked: per token in the interpreter, on entry to `fill_path`, `stroke_path`,
`show_glyph`, `draw_image`, `draw_shading` and `begin_form`, and per
destination row inside `fill_with_pattern`, `blit` and `draw_shading`'s pixel
loop.

Not checked:

- **`tinker_pdf_raster::fill`** takes no token. Its row loop is the single
  largest unit of work in a render — `height × 16 × edges`, with edges bounded
  only by `MAX_VERBS` times up to 512 flattened points per curve.
- **`stroke`** takes no token. Dash expansion of a long path with a fine dash
  array runs before any fill starts.
- **`flatten`** takes no token.
- **`Renderer::paint`** has no check at all, neither on entry nor internally —
  and `paint` is where the cost is. Its callers check on *their* entry, which
  is not the same thing.
- **`clip_path`** and **`end_text`** have no check even on entry, so they run
  a full-canvas fill plus an intersect even when the token was already set
  before the operator arrived.

There is also a reporting oddity: `finish()` pushes `RenderWarning::Cancelled`
whenever the token is set, whether or not any work was actually skipped.

## Scope

- Entry checks on `paint`, `clip_path` and `end_text`.
- A cancellation hook inside `fill`'s row loop.
- A hook inside `stroke`'s dash expansion.
- Make `Cancelled` mean work was skipped, not merely that the token was set.

## Non-goals

- **Cancelling mid-row.** A row is bounded by the canvas width; row
  granularity is prompt enough and is what the documentation promises.
- **Unwinding partial state.** A cancelled render returns whatever was drawn.
  That is already the contract and callers rely on it for progressive
  display.

## Design

The raster crate must not learn about `CancelToken` — it is a leaf and knows
nothing of the render layer (ruling 8). Pass a `&dyn Fn() -> bool`, or a small
`Cancel` trait local to the raster crate that the render layer implements.
`fill` and `stroke` take it as an `Option`, so every existing caller and every
test is unaffected.

`fill` returns a partial mask on cancellation rather than an empty one. A
half-drawn shape is a better progressive frame than a missing one, and the
caller is discarding the canvas anyway if the cancel was real.

For the warning: set a flag when a check actually fires, and push `Cancelled`
from that rather than from the token's state at `finish`.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Entry checks on `paint`, `clip_path`, `end_text` | A token set before a clip operator means the clip's fill never runs — asserted by timing or by a counting token | S |
| 2 | Cancellation inside `fill` and `stroke` | A single very large fill with a token set mid-flight returns early; a counting token shows the row loop stopped | S |
| 3 | `Cancelled` reports skipped work | A render that completes before the token is set does not report `Cancelled`; one that stops early does | S |

## Dependencies

**Needs first:** nothing. Lands well beside [14](14-bounded-painting.md),
which touches the same loops.

**Unblocks:** a responsive UI on large pages, which is the reason the token
exists.

## Risks

| Risk | Mitigation |
| --- | --- |
| A per-row branch in the hottest loop costs measurable time | Check every N rows rather than every row; N is a constant, so the cost is amortised and determinism is unaffected because the check does not alter output |
| A partial mask could be mistaken for a complete one by a caller that ignores the warning | The warning is the signal, and milestone 3 makes it truthful; a caller ignoring it was already accepting whatever was drawn |

## As built — August 2026

Two commits: milestone 1 alone, then milestones 2 and 3 together. The inventory
was as described, with two corrections owed to the gaps that landed
immediately before this one.

**What gap 14 changed about the argument.** This plan calls `fill`'s row loop
"the single largest unit of work in a render — `height x 16 x edges`". After
[14](14-bounded-painting.md) that loop is bounded to the rows the shape
actually reaches, so the cost is no longer proportional to the page. The hook
is still needed — a large shape on a large page is still a large sweep, and
dash expansion of a fine dash array still runs to completion before any fill
starts — but it is a promptness fix now rather than a rescue from an unbounded
loop.

**The seam was already chosen.** [12](12-image-sampling.md) threaded
cancellation into `ImageDraw` as `stop: &dyn Fn() -> bool` when it moved image
sampling into the raster crate. `fill` and `stroke` take the same shape as an
`Option`, so ruling 8 holds — the leaf crate never learns what a `CancelToken`
is — and there is one convention rather than two.

**`Cancelled` now means work was skipped.** `finish()` used to push the warning
whenever the token was set, so a caller whose timeout fired after the last
operator had run got a complete page reported as cancelled. An `AtomicBool` is
set by the checks themselves — including the predicate handed to the raster
crate, since a fill that stopped halfway skipped work as surely as an operator
that never began — and `finish()` reads the flag rather than the token.

**N is 16 rows**, counted in sweep iterations rather than row indices, so the
first row of every fill is checked whatever part of the region the shape starts
in. The cost was measured rather than assumed: 600 filled cubic paths on US
Letter at 300 dpi, release build, median of four runs, 26.9 ms with the
predicate against 25.1 ms without — inside the run-to-run spread of the same
build (26.6 to 28.9 ms), and against the 321x that gap 14 bought on a
comparable page.

**The number cannot move a pixel.** The predicate decides only whether the
sweep continues, never what a continued row computes, so `STOP_EVERY` is free
to be retuned without re-baselining anything. Two tests pin that directly — a
predicate that never answers yes changes no pixel and no outline — and none of
the seven determinism fingerprints moved, on native Windows or on
`wasm32-wasip1` under wasmtime 47.0.3.

**Tests use a counting token, never a timer.** A cancellation test paced by
wall-clock time is flaky by construction; every test here fires on the Nth
check, so the point at which work stops is exact and reproducible.

Injecting the milestone 3 defect — `finish()` reading the token rather than the
flag — fails exactly one assertion,
`cancelled_reports_skipped_work_rather_than_a_token_that_was_set`, which is the
test written for it.

**This closes the rasteriser lane.** Plan 07's `STATUS.md` row moves from
"see gap 15" to complete. What remains against that plan is recorded there as
divergence rather than absence: `Point` is `f64` where the plan says `f32`, and
the flattener is fixed-step where the plan says adaptive-recursive — the latter
deliberately, because a recursive subdivider terminates on a float comparison
and ruling 4 forbids that on a pixel path.
