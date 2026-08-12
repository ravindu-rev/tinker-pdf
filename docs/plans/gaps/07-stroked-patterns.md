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

## As built — August 2026

The defect was still live at `6f996cc`, exactly as described. A workspace grep
for `stroke_pattern` found four hits — the field, its initialisation, and the
two writes in `set_color`/`set_pattern` — and no reader anywhere.
`Renderer::stroke_path` and the stroking half of `show_glyph` both called
`stroke_color(state)`, and `ColorSpace::Pattern`'s `to_rgb` answers `(0, 0, 0)`
because 8.7.3 gives a pattern space no colour of its own.

Measured before the fix, on a 40 pt page with an 8 pt rule across it:

| Content | Rule pixel | Warnings |
| --- | --- | --- |
| `/Pattern cs /P0 scn ... re f` (shading) | the gradient | none |
| `/Pattern CS /P0 SCN ... S` (shading) | **(0, 0, 0)** | **none** |
| `/Pattern cs /P0 scn ... re f` (tiling) | untouched white | `UnsupportedPattern` |
| `/Pattern CS /P0 SCN ... S` (tiling) | **(0, 0, 0)** | **none** |

So the reporting half was real: [STATUS.md](../../STATUS.md)'s gap row said
tiling patterns are "reported with a warning rather than half-decoded", and on
the stroke path that was false. It is corrected in the same commit
(CONTRIBUTING rule 4), and [audit-2026-08.md](../../audit-2026-08.md)'s row
closes.

**What was built, against the milestones.** All four, together — the hazard
this plan names is the shading half landing without the tiling warning.

1. `stroke_path` calls `fill_with_pattern` with the outline `stroke()` already
   computed, instead of `paint` with `stroke_color`. No pattern machinery was
   added: a stroke is a fill of its outline, and the fill path was already
   right.
2. The same call routes `PatternPaint::Unsupported` into
   `RenderWarning::UnsupportedPattern`, because it is the same function. The
   stroke path and the fill path cannot now diverge in what they report,
   which is the point.
3. `show_glyph`'s stroking half routes identically.
4. `[/Pattern base]` (8.7.3.2) parses, and the components an `scn` supplies
   before a pattern name are resolved through `base` into the slot's colour.

**Four things this plan did not say.**

*The `scn` components were dropped for the fill slot too.* The design section
says the uncoloured case needs the components "stored for the stroke slot as
they are for the fill slot". They were not stored for either: `scn` collected
them, then returned early the moment it saw a trailing name. So milestone 4 is
a defect on both slots rather than a stroke-side omission, and the fix is in
the shared branch.

*`[/Pattern base]` did not parse at all.* `parse_space` had no `Pattern`
family arm, so the array form of the uncoloured pattern space resolved to
`None` — the components could not have been interpreted even if they had been
kept. `ColorSpace::Pattern` accordingly grew an optional underlying space,
which is also what makes `components()` answer 4 for a pattern over CMYK
instead of 1.

*`fill_with_pattern` used `state.fill_alpha` unconditionally.* Reusing it for
strokes carried `ca` onto a stroke, where 8.4.5 says `CA`. The alpha is now a
parameter — a signature change, not the refactor the non-goals rule out. A
test sets the two apart, because nothing else can tell them apart.

*The filling half of `show_glyph` had the same defect and is fixed too.* This
plan scopes only the stroking half. But a mode 2 glyph fills *and* strokes,
and routing one half would have painted a black body under a patterned edge —
which is this plan's own "worse than none" argument at the scale of a single
glyph. It is three lines at a call site already being edited; it adds no
machinery.

**Both risks were proved rather than assumed.** Each defect was injected into
the working tree and the whole suite re-run:

| Injected defect | Caught by |
| --- | --- |
| Anchoring: `matrix.then(&state.ctm).then(&base)` | **1** test — `a_stroked_pattern_does_not_move_with_the_transform`. All eleven others passed, including every "the gradient paints" assertion |
| Fill rule: the stroked outline filled even-odd | **1** test — `a_self_crossing_pattern_stroke_fills_its_overlap`. The twelve integration tests all passed; a straight rule's outline does not overlap itself |
| `show_glyph`'s stroking half left unrouted | 2 unit tests. No integration test caught it — those fixtures embed no font |
| The `scn` components dropped again | 1 unit test in `tinker-pdf-content`. Nothing in the render or facade crates caught it |
| `[/Pattern base]` losing its underlying space | 1 unit test in `resources.rs` |
| `state.fill_alpha` on the stroke route | 1 integration test |

The first two are the ones this plan's risk table asked for, and both turn out
to be caught by exactly one assertion each. Every other test in the file is
satisfied by a gradient that paints *somewhere*.

**A determinism fixture was added.** `fill_with_pattern` had no pixel baseline
at all: `determinism.rs`'s `shading` fixture goes through `sh`, which is a
different loop with a different inverse transform. The new `pattern` fixture
paints one shading pattern as a fill and as a stroke, under a rotated pattern
`/Matrix` and a non-identity paint-time CTM, so 8.7.3.1's anchoring is pinned
as bytes as well as as pixels. It reproduces byte-for-byte on
`wasm32-wasip1` against native Windows. **No existing fingerprint moved**,
which is what the change was expected to do — none of the four strokes with a
pattern.

**What gap 09 inherits, and why it is written into its plan.** The anchoring
test above can only assert the *shading* case here, because a tiling pattern
still warns rather than paints. Gap 09's own "worse than none" section names
paint-time-CTM anchoring as its silent failure mode, so the guarantee has a
hole exactly where that document says the danger is until the assertion is
re-run for tiling. [09](09-tiling-patterns.md) now carries that obligation as
a dated amendment; it is not enough to leave it here.
