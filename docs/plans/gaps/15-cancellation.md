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
