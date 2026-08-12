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

## As built — August 2026

Two commits, and the defect was still live: `/W2` and `/DW2` appeared nowhere
in the tree outside these plan files, so `width_of` returned the horizontal
advance whatever the writing mode. Detection was exactly as described —
`cmap.rs` set `vertical`, `font.rs` copied it, the pen moved down — and the
number it moved by was `/W`'s.

**What was built, against the milestones.**

1. `/DW2` and `/W2` are read on the descendant font in
   `crates/tinker-pdf-cos/src/font.rs`, beside `/W` and for the same reason
   `/CIDToGIDMap` is read there (ruling 8): they are entries in a PDF font
   dictionary, and the leaf crate has no PDF vocabulary. Both of Table 117's
   forms, both bounded as `/W`'s range form already was.
   `vertical_metrics(cid) -> (v_x, v_y, w1_y)` sits beside `width_of`, with
   `has_vertical_metrics` alongside it so "the file said this" and "the
   default happens to match" stay distinguishable — the same argument
   `has_cid_to_gid_map` was added under.
2. The displacement reaches the pen in `Interpreter::show`, by 9.4.4's `ty`
   rather than by a negated `tx`.
3. The position vector reaches `Glyph::transform`, inside the size scaling so
   that it goes through `Trm` like any other glyph-space coordinate.

**Three things this plan did not say.**

*`TJ` moved a vertical pen sideways.* 9.4.3 subtracts the adjustment from "the
current horizontal or vertical coordinate, depending on the writing mode", and
`show_array` put it on x unconditionally. A vertical run with any kerning in it
therefore drifted out of its own column, one adjustment at a time, and the
column spacing never changed. Plan [06](../06-content-and-text.md) already
carried the correct formula; the code did not. Fixed here because it is the
same branch and the same sentence of the spec, and plan 06 is amended to say
so out loud.

*`Th` was applied to the vertical advance.* 9.4.4's `ty` has no `Th` in it —
horizontal scaling scales horizontal motion — and the shared advance
expression multiplied by it before the axis was chosen. Invisible until a
vertical run met a `Tz`, and then it stretched the line spacing.

*`Glyph::advance` changed meaning, and had to.* It is now the signed
displacement along the writing direction, so vertically it is negative. The
text device already took `.abs()` of it, which is why nothing broke; the field
is documented rather than left for the next caller to discover.

**How the two risks were proved rather than assumed.** Each was injected into
the working tree and the suite re-run:

| Injected defect | Caught by |
| --- | --- |
| The vertical displacement negated | 7 integration tests, 3 unit tests |
| `/W2`'s array form read with `/W`'s one-number stride | 3 unit tests in `tinker-pdf-cos`, 6 integration tests |
| `/W2`'s range form advancing 3 elements instead of 5 | 1 unit test, 3 integration tests |
| The position vector dropped | 4 unit tests, 8 integration tests |
| The original defect restored — the horizontal advance applied downward | 5 integration tests |

The range-form stride is the one worth noting: exactly **one** assertion
catches it, and it is the one this plan's risk table asked for. The fixture
writes an array entry, then a range entry, then a second array entry, so a
range form that consumed the wrong number of elements desynchronises what
follows and CID 14 reads back as something else. Without the third entry the
error is invisible.

**The decoy.** `/W` gives every CID in the fixtures 1000 units, against
vertical displacements of -400, -600 and -900. No test can reach its expected
number through the horizontal path.

**One existing fixture moved, and it is a rendering change rather than a
regression.** `predefined_cmap_rendering.rs` showed its vertical glyph at
(20, 30) on a 300x100 page. With the position vector applied — and no `/W2` in
that fixture, so `v` is the derived `(w0/2, 880)` — the glyph is drawn 24
points left and 42 down of the pen, which put it over the page's corner where
the clip decided its ink box. The pen moved to (40, 60); the CIDs, the glyphs
and the inheritance it tests are untouched.

**No determinism fingerprint moved.** All four fixtures in
`crates/tinker-pdf/tests/determinism.rs` are horizontal, and they reproduce
byte-for-byte on native Windows and on `wasm32-wasip1` under wasmtime 47.0.3.

**Tests: 1093 to 1117.** Seven in `tinker-pdf-cos/tests/fonts.rs` for the
parsing, four unit tests in `interpret.rs` for the state-machine arithmetic
against hand-computed positions, and thirteen in
`crates/tinker-pdf/tests/vertical_metrics.rs` — the text-quad half needing no
rasteriser, the position-vector half rendering.

**What is still outstanding in vertical text**, none of it in this plan's
scope. Vertical substitution (`vert`/`vrt2`) remains a non-goal and always
will be — it is shaping. `Tw` is skipped for every vertical run rather than
for every multi-byte code, which is right for every font that exists and is
not what 9.3.3 says; it predates this work and is left alone deliberately. The
redaction pen in `crates/tinker-pdf/src/redact.rs` computes its own advances
and knows only about `x`, so redacting a vertical run measures the wrong axis
— it is a separate seam from the interpreter, and no plan currently owns it.
And no corpus has been run: 9.4.3's rule that a positive `TJ` number moves a
vertical pen *further down* is implemented as the spec states it, and whether
producers write it that way is a question only real files can answer.
