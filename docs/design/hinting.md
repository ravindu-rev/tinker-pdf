# Hinting and small text

*For [ROADMAP](../ROADMAP.md) rows FT-04a to FT-04d.* At eight to sixteen
pixels an em a glyph's stems are a pixel or two wide, and where an edge falls
between two pixels the stem draws as two grey columns rather than one black
one. A font may carry its own answer — TrueType bytecode, or the stem and zone
hints of a CFF or Type 1 program — that moves the outline onto the pixel grid
at each size before it is filled; a face that carries none can be fitted by an
autohinter; and a renderer can thicken a glyph a little at small sizes so its
stems keep their weight. This document lays down the seam all four plug into —
a per-size outline asked of the glyph source, placed on a whole pixel of the
page frame, then darkened, then filled — and FT-04b's interpreter for
TrueType bytecode. Stem darkening's section (FT-04a) follows; FT-04c's and
FT-04d's sections are added by their design steps. Every treatment is off by
default, and whether a page looks better with one is H-65's judgement: no
other renderer's output is an expected answer here (ruling 13).

## Scope

- **The seam** every small-text treatment shares: `RenderOptions::hinting`,
  when a glyph is asked for at a size, the per-size request and its answer,
  where the answer is placed, the cache keys, and what stays unhinted.
- **FT-04b's interpreter** (FT-04b.2 to FT-04b.10): the instruction set
  transcribed from its published specifications; its numbers, zones and
  graphics state; the `fpgm`, `prep` and glyph-program pipeline; composites;
  `gasp` and `head`; what a program asks of its engine (`GETINFO`, `MPPEM`,
  `MPS`, `INSTCTRL`); the bounds that keep hostile bytecode finite; failure.
- **Drawing** (FT-04b.11): the option reaching the renderer, and the hinted
  outline filled anti-aliased or in monochrome.
- **Monochrome dropout control** (FT-04b.12): the scan-converter rules a
  hinted glyph's programs select.
- **Proof** (FT-04b.13 to FT-04b.16): the fuzz target, the fingerprints on
  every target, the speed rows, and the example H-65's reviewer reads.
- **What FT-04c and FT-04d share**, and whether `tinker-pdf-font` joins
  `cargo xtask libm`'s pixel paths.

## Non-goals

- **Variations.** `GETVARIATION`, `cvar` and a variable face's hinting are
  FT-05's; `GETINFO` answers that variations are not supported.
- **ClearType's modes.** Sub-pixel positioned hinting and the
  backward-compatibility interpreter a ClearType rasterizer runs are not
  built: `GETINFO` answers no ClearType, and `INSTCTRL`'s third selector is
  kept and changes nothing. LCD text (RC-10) stripes whatever outline it is
  given, hinted or not.
- **A rotated, skewed, mirrored or stretched glyph.** It is drawn unhinted
  (*When a glyph is hinted*).
- **Layout by hinted advances.** A PDF places each glyph by `/Widths` and the
  text matrix; the phantom points carry a hinted advance a program may move,
  and nothing reads it but the program.
- **Device-metrics tables.** `hdmx`, `LTSH` and `VDMX` describe or cache
  advances and heights, which PDF layout does not take from the face.
- **Hinted output anywhere but the raster.** `to_svg`, text extraction and the
  display list's recorded geometry keep the outline the font states.
- **The picture.** The tests hold each instruction to its specified effect and
  each property the rows name; whether small text reads better is H-65's.

## Design

### The option

```rust
pub enum Hinting {
    /// Today's drawing: every glyph from the outline its font states.
    Off,
    /// The face's own hints: a TrueType glyph's bytecode (FT-04b), a CFF or
    /// Type 1 glyph's stem and zone hints (FT-04c). A glyph of a face that
    /// carries none draws unhinted.
    Font,
    /// `Font`, and the autohinter (FT-04d) for a glyph of a face that
    /// carries no hints, as FT-04d's section defines one.
    FontOrAuto,
}

// tinker_pdf::RenderOptions
pub hinting: Hinting,   // Hinting::Off by default
```

One option for all three hinters, so FT-04c and FT-04d plug into the seam
FT-04b builds without a second one. It is handed through `render_layer_with`
to the `Renderer` as `antialias` is, and `Page::render`, `render_form`,
`render_annotation` and `DisplayList::render` all honour it, since each draws
through the renderer.

### When a glyph is hinted

A glyph is asked for at its size when every one of these holds, and drawn
from its em outline exactly as today otherwise:

| Condition | Why |
| --- | --- |
| `RenderOptions::hinting` is not `Off` | Off is today |
| Text render mode 0, a fill and nothing else (9.3.6) | A stroke and a clip take the outline the font states, so a glyph's fill, its stroke and its clip never disagree about its shape |
| Not a Type 3 glyph | Its procedure paints graphics, which have no hints |
| The em-to-device 2×2 is axis-aligned and unstretched: `b` and `c` zero and `d` equal to `a` or to its negation, each read at the 1/256-pixel-an-em steps the glyph mask's placement keeps (`glyph::Placement::matrix`), with `a` above zero | The instruction set's distances are measured along axes and scaled by one size; a quarter turn, a skew, a mirror or a horizontal scaling (`Tz`) would need the CVT scaled per direction, and is drawn unhinted instead. The decision is the placement's, so a tile and the page decide alike |
| `a`, the pixels per em, is at least 1 and at most 1 024 | Below a pixel there is no grid to fit; above 1 024 the glyph takes the path route a large glyph already takes |
| For a TrueType face, the `gasp` range covering the size asks for grid-fitting, or the face has no `gasp` (*`gasp` and `head`*) | The face says where its hints help |
| The face's `fpgm` ran, and its `prep` ran at this size | A face or a size whose setup failed draws unhinted (*Failure*) |

`GETINFO`'s rotation and stretch bits are therefore always 0: a rotated or
stretched glyph never reaches a program.

### The per-size request

The renderer asks its glyph source through a new call beside `outline`:

```rust
pub struct GlyphSize {
    /// Pixels per em, in 26.6: `a` of the em-to-device 2×2, times 64,
    /// rounded to the nearest unit.
    pub ppem: i32,
    /// The em's extent in user space, in points and 26.6: the square root of
    /// the glyph transform's 2×2 determinant, times 64, rounded. `MPS`.
    pub points: i32,
    /// Whether text is drawn whole or empty (`text_bits` 0, or anti-aliasing
    /// off): `GETINFO`'s greyscale bit, and dropout control.
    pub monochrome: bool,
    /// `Font` or `FontOrAuto`; never `Off`, which never asks.
    pub hinting: Hinting,
}

pub struct SizedOutline {
    /// The fitted outline in device pixels about the glyph's origin, y up as
    /// glyph space is: each point a multiple of 1/64.
    pub outline: Outline,
    /// For a monochrome glyph, the scan control its programs left
    /// (*Monochrome dropout control*); `None` keeps today's hardening.
    pub dropout: Option<Dropout>,
}

// tinker_pdf_render::GlyphSource
fn sized_outline(&self, font_id: u64, code: u32, size: &GlyphSize) -> Option<SizedOutline> {
    None
}
```

`None` is the answer for a glyph not hinted at that size — a face without
hints under `Font`, a `gasp` that declines, a program that failed — and the
renderer then draws the em outline as today. The facade answers it in
`resources.rs` by taking the glyph `selection` takes, then the face's hinter,
and keeps the answer by font, glyph and `GlyphSize` beside `Caches::outlines`,
so one glyph at one size is hinted once a scope. The call is new rather than
an edit of `extract_outline`, which FT-01 changes.

### Placement: the origin on a whole pixel of the page frame

A hinted outline is fitted to a grid whose lines are whole pixels about the
glyph's origin, so the origin has to stand on a whole pixel too. It is placed
at the glyph's origin rounded to the nearest pixel, `floor(e + 1/2)` and
`floor(f + 1/2)` of the em-to-device transform's translation, and mapped by
the 2×2's signs alone: device `x` is the origin's plus the outline's `x`, and
device `y` the origin's minus the outline's `y` where `d` is negative, as the
page frame's `y` runs down.

**Ruling 5.** `Renderer::base` is never translated for a tile, so the
translation is the page frame's in a tile, a group's buffer and the page, and
the rounding is the same number in each — as `Placement::of` takes its phase
in the page frame. A hinted glyph's mask is made at phase `(0, 0)` and placed
by a whole pixel, so a tile reads the page's mask.

The origin moves by up to half a pixel on each axis. Spacing still follows
`/Widths`: each glyph's own origin rounds on its own, and no advance moves.

### The cache keys

Both keys a glyph's mask is kept under — the renderer's own
`glyph_cache::Key` and the source's `MaskKey` — gain the hinting option and,
with FT-04a, whether the glyph is darkened, so a mask made one way is never
drawn for a glyph asked for another. The monochrome flag is already there as
the coverage level and the hard-edge rule.

### What stays unhinted

`to_svg`, text extraction, the display list's recorded geometry, a stroked or
clipping glyph and a Type 3 glyph read the outline the font states. The
display list records glyphs rather than outlines, so `DisplayList::render`
hints as `Page::render` does.

### The interpreter's specification

Each instruction's semantics are transcribed from the instruction set's
published specifications, and from nothing else (ruling 21): the OpenType
specification's TrueType chapters — *TrueType Instruction Set*, *Instructing
TrueType Glyphs*, *Graphics State Summary* and the `cvt `, `fpgm`, `prep`,
`gasp`, `maxp`, `head` and `glyf` tables (version 1.9.1) — read beside Apple's
*TrueType Reference Manual*, chapters 2 (*Font Engine*), 3 (*Instructing
Fonts*), 4 (*Graphics State*) and 5 (*Instruction Set*). The OpenType text
governs where the two differ: its four phantom points, its `GETINFO` bits and
its `SCANTYPE` rules 5 and 6. Where both leave a choice to the engine — how
`MUL` rounds, what the twilight zone's original points are — this document
makes it and says so, and a test holds the engine to it as this document's,
never as the specification's.

The interpreter is `tinker-pdf-font`'s `hint` module, a leaf like the rest of
the crate (ruling 8): `hint/mod.rs` the face, size and glyph entry points,
`hint/vm.rs` the stack, decoding and dispatch, then a file a family —
`flow.rs`, `storage.rs`, `state.rs`, `moves.rs`, `shift.rs`, `iup.rs`,
`glyph.rs` — and `hint/tests/` a file a family.

### Numbers

- **The stack holds 32-bit two's-complement values.** A coordinate, a
  distance and a CVT value are F26Dot6 (26.6); a vector component is F2Dot14
  (2.14); a count, an index, a flag word and a selector are integers. The
  instruction tables label some stack elements `uint32`, a width; every
  comparison (`LT` to `NEQ`, `MAX`, `MIN`) is signed, as a distance is.
- **`ADD`, `SUB` and `NEG` wrap**: the specification gives these instructions
  no overflow protection, and wrapping is the two's-complement answer on every
  target. `ABS` of the most negative value is itself.
- **`MUL`** is `n1 · n2 / 64` in 64 bits, rounded half away from zero, then
  wrapped to 32 — this engine's choice, the specification stating no rounding.
  **`DIV`** is `n2 · 64 / n1` in 64 bits, truncated toward zero as the
  specification states, then wrapped; a zero divisor is an error.
- **`FLOOR`** is `n & !63` and **`CEILING`** `(n + 63) & !63`, wrapping.
- **A unit vector** set from two points (`SPVTL`, `SFVTL`, `SDPVTL`) is the
  difference `(dx, dy)` in 26.6 scaled to length 16 384: each component
  `round(16 384 · d / sqrt(dx² + dy²))`, the sum and the root in `f64` on
  integers below 2^53, which IEEE 754 rounds the same way on every target. A
  difference of zero sets the vector to the x axis, this engine's choice.
  `SPVFS` and `SFVFS` take their two components as given.
- **A projection** of a 26.6 difference on a 2.14 vector is
  `(dx · vx + dy · vy) / 16 384` in 64 bits, rounded half away from zero.
- **A move** of a point by a distance `d` measured along the projection
  vector `p` is along the freedom vector `f`: the point moves by
  `f · d · 16 384 / (f · p)`, each component rounded half away from zero, so
  that its projection on `p` changes by `d`. A freedom vector orthogonal to
  the projection vector at a move — `f · p` of zero — is an error (*Instructing
  TrueType Glyphs*: "it is not valid").

### Zones, points and the graphics state

**Zone 1** holds the glyph: its `n` contour points and four phantom points,
each with its current position, its original position (the scaled, unhinted
point), its on-curve flag and a touched flag for each axis, and the contours'
end points. **Zone 0**, the twilight zone, holds `maxTwilightPoints` points
(`maxp`), all at the origin, their original positions the origin too until a
move sets them (`MIAP` and `MIRP` in zone 0 set the original position with the
current, this engine's choice where the specification is silent). A point
index past its zone, a contour index past the glyph and a zone number other
than 0 and 1 are errors.

**The phantom points** follow the contour points, as the OpenType text places
them: `n` at the horizontal origin, `xMin` less the left side bearing; `n+1`
the advance width past it; `n+2` the vertical origin, `yMax` plus the top
side bearing; `n+3` the advance height below it. Horizontal metrics are
`hmtx`'s; vertical ones `vmtx`'s, or `hhea`'s ascender and descender where the
face has none, this engine's choice. Each is scaled as the points are.

**The graphics state** starts each program at the defaults the *Graphics State
Summary* gives: `auto_flip` on, `control_value_cut_in` 17/16 pixel,
`delta_base` 9, `delta_shift` 3, the projection, freedom and dual vectors the
x axis, `instruct_control` 0, `loop` 1, `minimum_distance` one pixel,
`round_state` to the grid, `rp0` to `rp2` 0, `scan_control` off,
`single_width_cut_in` and `single_width_value` 0, and `zp0` to `zp2` zone 1.
What `prep` leaves of the settings — `auto_flip`, the two cut-ins, the single
width, `delta_base`, `delta_shift`, `instruct_control`, `minimum_distance`,
`round_state` and the scan control — becomes the default every glyph of the
size starts from; the vectors, the zone pointers, the reference points and the
loop start every glyph at the summary's defaults, as they describe a position
in one glyph rather than a setting of the font.

**Engine compensation** for `ROUND`, `NROUND`, `MDRP` and `MIRP`'s grey, black
and white distances is zero for all three, this engine's choice; distance type
3, which fonts must not use, is read as grey.

### The pipeline

**A face** is read once: `maxp`'s limits, `head`'s units per em and flags,
`cvt `, `fpgm`, `prep` and `gasp`. `fpgm` runs once, defining the functions
and instruction definitions every later program may call; a face whose `fpgm`
fails is unhinted at every size.

**A size** is set up once a face and `GlyphSize`: the CVT's FWORDs scaled to
26.6 at the size, `round(v · ppem / unitsPerEm)` in 64 bits, the storage area
cleared to zeros, the twilight zone set to the origin, and `prep` run over
them. The CVT, the storage area, the twilight zone and the graphics-state
defaults `prep` leaves are kept for the size; a size whose `prep` fails is
unhinted.

**A glyph** starts from the size's kept state — a copy of its CVT, storage
area and twilight zone, so no glyph's program can change another's and a glyph
hints the same whichever glyphs were hinted before it, which is what lets the
facade cache by glyph. Its points are scaled to 26.6 as the CVT is, its
phantom points added, and its program run; the hinted points become an
`Outline` through `glyf.rs`'s own contour reading, the implied on-curve
midpoints included, in pixels. A glyph with no program is its scaled points.

`INSTCTRL`'s first selector, set in `prep`, makes every glyph of the size its
scaled points with no program run; its second makes glyph programs start from
the summary's defaults rather than `prep`'s. In a glyph program both are
ignored, as the specification says. The third selector is kept and changes
nothing (*Non-goals*). A selector past 3 is ignored.

**Composites** (`glyf`, FT-04b.10) hint each component in turn at the size with
its own program, then place it: its offset scaled, rounded to the grid on each
axis where `ROUND_XY_TO_GRID` is set, or matched point to point on the hinted
points where `ARGS_ARE_XY_VALUES` is clear; `USE_MY_METRICS` takes the
component's phantom points as the composite's; then the composite's own
program, where `WE_HAVE_INSTRUCTIONS` gives one, runs over the assembled points
and four phantom points of its own. A component under a scale or a 2×2 is
assembled from its scaled points without its own program, which measured an
untransformed outline — this engine's choice. `glyf.rs`'s depth of 8 and
budget of 256 components bound the walk as they bound the unhinted one.

### `gasp` and `head`

**`gasp`**, versions 0 and 1: the range a size takes is the first whose
`rangeMaxPPEM` is at least the size's whole pixels per em (`ppem` rounded to
the nearest pixel), and the glyph is hinted where that range's
`GASP_GRIDFIT` (bit 0) is set. `GASP_DOGRAY` and the two ClearType bits are
not read: the page's text coverage decides grey or monochrome. A face with no
`gasp` is hinted at every size; a size past the last range, which a table cut
short leaves, is not.

**`head`**'s flags bit 3 asks for integer pixels per em in all scaler
arithmetic: with it set, the size the points and the CVT are scaled to is
`ppem` rounded to a whole pixel, so a glyph asked for at 12.4 pixels an em is
hinted and drawn at 12. `unitsPerEm` is read as `Sfnt` reads it.

### What a program asks of its engine

- **`MPPEM`** answers the size's whole pixels per em, along whichever vector
  is the projection vector, since a hinted glyph's two axes are one size.
  **`MPS`** answers `GlyphSize::points`.
- **`GETINFO`**, by selector bit, as the OpenType text's table gives them:

| Selector bit | Result bits | Answer |
| --- | --- | --- |
| 0, version | 0 to 7 | 35: the version that answers greyscale (34 and later) and predates ClearType (36 and later), so a font's ClearType paths are not taken |
| 1, rotated | 8 | 0: a rotated glyph is never hinted |
| 2, stretched | 9 | 0: a stretched glyph is never hinted |
| 3, variations | 10 | 0 |
| 4, vertical phantom points | 11 | 1: the four phantom points are kept |
| 5, greyscale | 12 | 1 when `GlyphSize::monochrome` is false |
| 6 to 12, ClearType | 13 to 19 | 0 |

Selector bits are answered together, OR'd into one result, and a bit no row
names answers nothing.

### Bounds that keep hostile bytecode finite

Bytecode is untrusted input (ruling 1). Every one of these is an error, never
a panic and never a wait:

| Bound | Value |
| --- | --- |
| The stack | `maxStackElements` plus 32 entries, at least 256 and at most 65 536: a producer that under-declares still runs, and a hostile `maxp` holds 256 KiB |
| The storage area, the CVT, the function and instruction definitions, the twilight zone | `maxStorage`, the `cvt ` table's entries, `maxFunctionDefs`, `maxInstructionDefs` (an opcode is a byte, so at most 256), `maxTwilightPoints` capped at 16 384; an index past any of them |
| Instructions a run executes | 1 048 576 (2^20) for each run of `fpgm`, `prep`, a glyph's or a component's program, counting each instruction dispatched, each point a loop-counted instruction moves, and each call `LOOPCALL` makes |
| Call depth | 64: `CALL`, `LOOPCALL` and a dispatch to an `IDEF` nest no deeper |
| The instruction stream | a push that runs past its program's end, a jump outside it, an `IF` with no `EIF` or a definition with no `ENDF` |
| Where definitions may stand | `FDEF` and `IDEF` in a glyph program, and `ENDF` outside a definition |
| What instructions name | an undefined function, an opcode with no instruction and no `IDEF`, a zero `DIV`, an orthogonal freedom vector at a move, `IUP` in zone 0 |

`IDEF` affects only undefined opcodes and is ignored for a defined one, as
the specification says. A run also stops when the renderer's stop predicate
answers yes, asked every 4 096 instructions, and leaves its glyph unhinted.
The instructions a run executes are its steps (`docs/design/resource-exhaustion.md`,
"The counters"): the hinter returns them as a value, and the facade adds them
to the render's work.

### Failure

An error in a glyph's program leaves that glyph drawn from its em outline, as
it is drawn today; one in `prep` leaves its size so, and one in `fpgm` the
whole face. The font crate returns the error as a `hint::Error` naming which
bound or rule it broke, and the facade reports a `RenderWarning` naming the
font, once a render, so a page drawn partly unhinted says so (ruling 10).
Nothing is retried leniently: a program that breaks a rule another engine
forgives is drawn as its font states, which is never wrong, only unhinted.

### Monochrome dropout control

A hinted glyph drawn monochrome is made whole-or-empty by the scan converter's
rules (*TrueType Instruction Set*, `SCANTYPE`; *Font Engine*, "Using the scan
converters to control dropouts"), not by `RenderOptions::hard_edge`: the
glyph's programs chose the rules for the size. Rules 1 and 2 — a pixel whose
centre the outline holds, or touches, is on — are `HardEdge::Centre`, read on
the filler's own grid. Dropout control adds the pixels rules 3 to 6 turn on
when a scan line between two adjacent pixel centres, across or down, is
crossed by an on-transition and an off-transition and neither pixel is on:

| `SCANTYPE` | Rules | Which pixel |
| --- | --- | --- |
| 0 | 1, 2, 3 | the left-most across, the bottom-most down, stubs included |
| 1 | 1, 2, 4 | as 0, but not for a stub: the crossings must continue to the scan lines either side |
| 2, 3, 6, 7 | 1, 2 | none: dropout control off |
| 4 | 1, 2, 5 | the pixel whose centre is nearer the midpoint of the two crossings, stubs included |
| 5 | 1, 2, 6 | as 4, but not for a stub |

"Bottom-most" is in the glyph's y-up frame, which the page frame's y-down
turns into the lower pixel on the page. Dropout control is on for a size
where `SCANCTRL`'s flags say so at its whole pixels per em: bit 8 with the
threshold in bits 0 to 7 (`0xFF` every size, `0` none), bit 11 off above the
threshold; bits 9, 10, 12 and 13 ask about rotation and stretching, which a
hinted glyph never has. The state the size's programs leave travels with the
outline as `SizedOutline::dropout`, and `tinker-pdf-raster`'s hardening of a
glyph's coverage applies it; every other shape hardens as before.

### What FT-04c and FT-04d share

- **The option and the request.** A CFF or Type 1 glyph (FT-04c) and a glyph
  of an unhinted face (FT-04d) answer `GlyphSource::sized_outline` with their
  fitted outline at the same `GlyphSize`; the facade routes each glyph to its
  face's hinter and `FontOrAuto` alone to the autohinter.
- **The conditions, the placement and the keys**: the axis-aligned rule, the
  origin on a whole pixel of the page frame, the cache keys and the darkening
  after; `gasp` is TrueType's and decides nothing for the other two.
- **The outline type and the arithmetic**: `SizedOutline` in pixels with its
  points on 1/64, and `hint::fixed`'s 26.6 operations — `round`, `floor`,
  `ceil`, `mul`, `div` and the half-away-from-zero rounding above — which the
  fitter and the autohinter use rather than writing their own.
- **Failure**: a fitter that cannot fit a glyph returns `None`, and the glyph
  draws unhinted with the same warning.

### `cargo xtask libm`

`tinker-pdf-font` joins `PIXEL_PATHS` in `xtask/src/main.rs` with FT-04b.2,
the commit that brings the first arithmetic a pixel depends on into the crate.
The interpreter's arithmetic is integer, but for the vector normalisation's
`sqrt`, which IEEE 754 rounds correctly; no transcendental is needed, and the
crate calls none today, so it joins clean and stays so.

## Milestones

1. **FT-04b.2, the core.** `hint/mod.rs`, `hint/vm.rs`: the bounded stack,
   decoding with inline push data, the instruction budget, typed errors; the
   push, stack, logic, arithmetic and `DEBUG` instructions; and
   `tinker-pdf-font` in `PIXEL_PATHS`. Exit: `hint/tests/stack.rs`'s
   `every_push_instruction_pushes_its_operands`,
   `every_stack_instruction_moves_the_stack_as_specified`,
   `every_logic_and_arithmetic_instruction_computes_its_specified_value`
   (`MUL` and `DIV` in 26.6, wrapping, and a zero divisor) and
   `underflow_overflow_a_truncated_push_and_an_unknown_opcode_are_errors`.
2. **FT-04b.3, control flow and definitions.** Exit: `hint/tests/flow.rs`'s
   `if_else_eif_take_the_specified_branch_at_any_nesting`,
   `jumps_move_by_their_signed_offsets` (relative to the jump itself),
   `fdef_call_and_loopcall_run_their_bodies_as_specified`,
   `idef_defines_an_unused_opcode`,
   `a_program_that_never_ends_stops_at_its_budget`,
   `recursion_past_the_call_depth_is_an_error`.
3. **FT-04b.4, storage, the CVT and the engine's answers.** Exit:
   `hint/tests/storage.rs`'s `storage_reads_back_what_was_written`,
   `the_cvt_is_scaled_to_the_size_and_written_in_pixels_or_font_units`,
   `mppem_and_mps_report_the_size`,
   `getinfo_answers_each_selector_as_specified` (the table above),
   `instctrl_is_honoured_only_where_the_specification_allows`,
   `an_index_out_of_range_is_an_error`.
4. **FT-04b.5, the graphics state.** Exit: `hint/tests/state.rs`'s
   `every_vector_instruction_sets_its_specified_unit_vector`,
   `reference_points_zones_and_the_loop_are_set_as_specified`,
   `every_round_state_rounds_as_specified` (`SROUND`'s and `S45ROUND`'s period,
   phase and threshold included),
   `odd_and_even_answer_as_specified`,
   `the_distance_and_delta_settings_are_stored_as_specified`,
   `scanctrl_and_scantype_set_the_dropout_state`.
5. **FT-04b.6, measuring and moving.** Exit: `hint/tests/moves.rs`'s
   `gc_scfs_and_md_measure_along_the_projection_vector`, and `mdap_`, `miap_`,
   `mdrp_`, `mirp_` and `msirp_moves_as_specified` over a grid of vectors,
   round states and flag bits.
6. **FT-04b.7, the other point instructions.** Exit: `hint/tests/shift.rs`'s
   `shp_shc_shz_and_shpix_shift_as_specified`,
   `alignrp_and_alignpts_align_as_specified`, `ip_isect_and_utp_as_specified`,
   `the_flip_instructions_toggle_on_curve_as_specified`.
7. **FT-04b.8, interpolation and exceptions.** Exit: `hint/tests/iup.rs`'s
   `iup_interpolates_and_shifts_untouched_points_as_specified`,
   `deltap_moves_a_point_only_at_its_size`,
   `deltac_changes_a_cvt_entry_only_at_its_size`.
8. **FT-04b.9, a simple glyph end to end.** Exit: `hint/tests/glyph.rs`'s
   `a_glyph_with_no_program_hints_to_its_points_scaled_to_a_64th`,
   `a_hand_assembled_program_moves_its_glyph_as_computed_in_the_test`,
   `phantom_points_carry_the_hinted_advance`,
   `fpgm_runs_once_per_face_and_prep_once_per_size` (a `cfg(test)` count),
   `an_erring_program_leaves_the_glyph_unhinted_and_reports_it`; `glyf.rs`'s
   unhinted outlines unchanged for every glyph of the committed faces.
9. **FT-04b.10, composites and the faces' fingerprints.** Exit:
   `hint/tests/glyph.rs`'s `a_composite_hints_its_components_then_itself` and
   `point_matching_places_a_component_as_specified`; `hinting_fingerprints.rs`'s
   `hinted_outlines_of_the_committed_faces_are_stable_across_targets`, every
   glyph of each of the twelve Liberation faces hinted at each whole size from
   8 to 16 pixels an em and at 24, 32, 48 and 72, hashed per face, each face
   asserting how many glyphs ran a program.
10. **FT-04b.11, drawing.** `RenderOptions::hinting`, `GlyphSource::sized_outline`,
    the placement and the keys above, and the warning. Exit: `hinting.rs`'s
    `a_hinted_glyph_draws_its_hinted_outline`,
    `a_rotated_glyph_is_drawn_unhinted`, `hinting_off_changes_no_pixel`,
    `hinted_tiles_are_byte_equal_to_the_page` (ruling 5),
    `monochrome_hinted_text_draws`, in the default and `bundled-fonts` builds.
11. **FT-04b.12, dropout control.** Exit: `fill.rs`'s
    `dropout_control_keeps_a_stem_narrower_than_a_pixel`,
    `each_scantype_mode_turns_on_the_pixels_its_rule_names`,
    `stubs_are_kept_only_where_scantype_asks`; `hinting.rs`'s
    `a_thin_monochrome_stem_survives_with_dropout_control`;
    `hardened_shapes_sharing_an_edge_partition_its_pixels` unchanged.
12. **FT-04b.13, the fuzz target.** Exit: `fuzz_seeds.rs`'s
    `the_truetype_hinting_seeds_reach_the_interpreter`: every seed hints or is
    refused by name, one at least runs each instruction family, every
    single-byte flip and prefix of each reaches an answer.
13. **FT-04b.14, every target.** Exit: `hinting_fingerprints.rs` in K-07's
    roster, and `determinism.rs`'s hinted monochrome text page, passing on
    Linux, Windows and `wasm32-wasip1`.
14. **FT-04b.15, the speed rows.** Exit: `xtask/src/bench.rs`'s
    `the_committed_baseline_agrees_with_the_tree` with the two operations —
    a Liberation face's every glyph hinted at 12 pixels an em, and the text
    page at 72 dpi with `hinting` on.
15. **FT-04b.16, the example.** Exit: `cargo run -p tinker-pdf --example
    small_text` writes one PNG per size from 8 to 16 pixels, hinting off and
    on, anti-aliased and monochrome, and the gates' examples gate finds its
    line.

## Risks

| Risk | Mitigation |
| --- | --- |
| A font whose bytecode breaks a rule another engine forgives draws unhinted here | Unhinted is the outline the font states, never a wrong shape, and the warning names the font; the fingerprints count the committed faces' glyphs that ran a program, so a rule that refuses them shows |
| Hostile bytecode loops, recurses or allocates | The bounds above, every one an error, and FT-04b.13's fuzz target replayed on stable |
| A glyph hints differently depending on which glyphs came before it | Every glyph starts from a copy of its size's state; `fpgm_runs_once_per_face_and_prep_once_per_size` and the fingerprints, which hint every glyph in order, hold it |
| Rounding the origin moves text by up to half a pixel | Off by default; each glyph rounds its own origin, so spacing still follows `/Widths`; the picture is H-65's |
| A font's ClearType paths assume stripes this hinting does not draw | `GETINFO` answers version 35 and no ClearType, so a font takes its greyscale and monochrome paths |
| A tile and the page hint a glyph differently | The size comes from the 2×2 and the origin from the page frame's translation, neither of which a tile changes; `hinted_tiles_are_byte_equal_to_the_page` |
| Hinting is slow on a page of many sizes | One glyph at one size is hinted once a scope, and FT-04b.15 measures it |
