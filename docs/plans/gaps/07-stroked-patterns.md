# A pattern-stroked line draws solid black

Set a gradient as the stroking colour and stroke a rule with it. The rule
comes out solid black, with an empty warnings list. The same pattern used as a
*fill* works correctly. When this is done, a stroke painted with a pattern
paints with that pattern, or says why not. (S)

## What is wrong

`crates/tinker-pdf-content/src/interpret.rs` records `gs.stroke_pattern` when
`SCN` names a pattern. Nothing reads it. A workspace grep for `stroke_pattern`
finds the write, the field declaration, and nothing else.

`Renderer::stroke_path` and the stroking half of `show_glyph` both call
`stroke_color(state)`, and `/Pattern` has no colour of its own — the colour
crate reports black for it. So the stroke paints black.

The comparison that makes this a reporting bug as well as a rendering one: a
*tiling* pattern used as a fill correctly produces `RenderWarning::UnsupportedPattern`
and leaves the area alone. The same tiling pattern used as a stroke paints
black and says nothing. STATUS's gap table claims tiling patterns are
"reported with a warning rather than half-decoded"; on the stroke path that is
not true.

## Scope

- `Renderer::stroke_path`: when `state.stroke_pattern` is set, route to the
  pattern paint rather than to `stroke_color`.
- The stroking half of `show_glyph`, which has the same defect for
  pattern-stroked text.
- Shading patterns paint; tiling patterns warn until
  [09](09-tiling-patterns.md) lands, and warn for the *stroke* path as they
  already do for the fill path.
- Uncoloured (`PaintType 2`) patterns take the colour the `SCN` operands
  supply, on the stroke path as on the fill path.

## Non-goals

- **Tiling patterns themselves.** [09](09-tiling-patterns.md).
- **Refactoring `fill_with_pattern`.** The stroke path can reuse it: a stroke
  is a fill of the stroked outline, and the outline is already computed.

## Design

The engine already turns a stroke into a fillable outline —
`stroke(&path, &style, tolerance)` — and then fills it. So the stroke path
does not need pattern machinery of its own: it needs to call the existing
`fill_with_pattern` with the stroked outline instead of `paint` with
`stroke_color`.

That is the whole change on the shading side. The uncoloured case needs the
`SCN` components stored for the stroke slot as they are for the fill slot.

## Where a half-implementation is worse than none

Painting shading patterns on strokes while leaving tiling patterns silently
black. The half that works makes the half that does not look like a rendering
bug in that particular file rather than a known gap. The warning is the part
that must not be skipped — it is what turns "this looks wrong" into "this is a
capability we do not have".

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | Shading patterns paint on `stroke_path` | A gradient-stroked rule shows the gradient; the same rule filled and stroked agree at their overlap | S |
| 2 | Tiling patterns warn on the stroke path | A tiling-pattern stroke produces `UnsupportedPattern` and leaves the area alone, matching the fill path exactly | S |
| 3 | Pattern-stroked text | `1 Tr` with a pattern stroke colour shows the pattern | S |
| 4 | Uncoloured patterns on strokes | A `PaintType 2` pattern stroke takes the `SCN` colour | S |

## Dependencies

**Needs first:** nothing.

**Unblocks:** nothing structurally, but it removes a silent-black failure that
would otherwise be blamed on whatever else is being worked on.

## Risks

| Risk | Mitigation |
| --- | --- |
| The stroked outline and the fill path could diverge in fill rule — a stroke outline is non-zero, always | Assert it explicitly in the test rather than relying on the default |
| Pattern anchoring on a stroke could pick up the stroke's transform rather than the parent stream's default space (8.7.3.1) | The same `base` the fill path uses; a test strokes the same pattern under two different CTMs and asserts the lattice does not move |
