# Vertical text advances by horizontal widths

A Japanese document set vertically detects that it is vertical and moves the
pen downward — by the horizontal width of each glyph. Columns come out with
the wrong line rhythm, and characters that should be shifted sideways within
their em box are not. When this is done, vertical text uses the vertical
metrics the file supplies. (M)

## What is wrong

Detection works. `crates/tinker-pdf-font/src/cmap.rs` sets `vertical` from a
`-V` name suffix or `/WMode 1`, `crates/tinker-pdf-cos/src/font.rs` copies it
onto the `Font`, and the pen moves down rather than across.

The metrics do not. `/W2` and `/DW2` — 9.7.4.3, the vertical counterparts of
`/W` and `/DW` — are not parsed anywhere. `width_of` returns the horizontal
advance whatever the writing mode, so every vertical advance is a horizontal
number that happens to be applied downward.

Two consequences beyond the advance itself. `/W2` also carries the **position
vector** `v`, which shifts a glyph within its em box for vertical setting —
a comma belongs in a different corner vertically than horizontally. And
`/DW2`'s default is `[880 -1000]`, so a font that omits `/W2` entirely still
needs the default applied rather than the horizontal fallback.

## Scope

- Parse `/DW2` (a two-element array: position vector *y*, then displacement)
  and `/W2` (the per-CID form, 9.7.4.3 Table 117) on the descendant font.
- A `vertical_metrics(cid) -> (v_x, v_y, w1_y)` accessor beside `width_of`.
- Apply the vertical displacement to the pen in the text state machine when
  the writing mode is vertical.
- Apply the position vector to the glyph's placement transform.
- Default to `[880 -1000]` when `/DW2` is absent, and derive `v_x` as half the
  horizontal width when the metrics do not give one — which is what 9.7.4.3
  specifies rather than a guess.

## Non-goals

- **Vertical substitution.** `vert`/`vrt2` OpenType features rotate brackets
  and swap punctuation forms; that is shaping, not metrics, and the engine has
  no shaping layer by design.
- **Ruby, tate-chu-yoko, or any layout above the glyph.** A PDF states glyph
  positions; the engine draws where it is told.

## Design

`/W2`'s per-CID form is more complex than `/W`'s: each entry carries three
numbers — the displacement and a two-component position vector — where `/W`
carries one. The array form `c [w1y v1x v1y w2y v2x v2y …]` and the range form
`c_first c_last w1y v1x v1y` both exist.

The accessor returns all three components rather than just the advance,
because the position vector is where the second half of the bug lives and a
signature that returns only the advance would invite exactly the same
omission again.

Where it applies: the text state machine already branches on writing mode to
decide which axis the pen moves along. The same branch chooses the metric.

## Milestones

| # | Deliverable | Exit criteria (concrete, testable) | Size |
| --- | --- | --- | --- |
| 1 | `/DW2` and `/W2` parsed, both array and range forms | A fixture's per-CID vertical displacements read back exactly; a font without `/DW2` reports the `[880 -1000]` default | S |
| 2 | Vertical displacement applied to the pen | A two-glyph vertical run advances by the `/W2` values, not the `/W` ones — asserted on the text quads, so it is checkable without rendering | M |
| 3 | Position vector applied to placement | A glyph with a non-default `v` draws offset within its em box; a rendered fixture differs from the same run without the vector | S |

## Dependencies

**Needs first:** [02](02-cid-to-gid.md) — the metrics are per *CID*, so this
is only meaningful once the CID is being used properly. [03](03-predefined-cmaps.md)
for any file using a `-V` registry CMap rather than `Identity-V`.

**Unblocks:** vertical CJK laying out correctly, which is most of the
non-Latin corpus that is not already covered by horizontal text.

## Risks

| Risk | Mitigation |
| --- | --- |
| The sign convention on the vertical displacement is easy to invert, and an inverted one still produces a plausible-looking column running the wrong way | The fixture asserts absolute pen positions after two glyphs, not deltas, so a sign error changes the number rather than the shape |
| `/W2`'s three-number entries are easy to misparse as `/W`'s one-number entries, silently shifting every subsequent CID | A fixture with entries for three non-adjacent CIDs; a stride error puts the metrics on the wrong ones |
