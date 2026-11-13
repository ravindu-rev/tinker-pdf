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
TrueType bytecode. Stem darkening's section (FT-04a) follows, then CFF and
Type 1 hint application's (FT-04c) and the autohinter's (FT-04d). Every
treatment is off by
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
`hint/fixed.rs` the 26.6 arithmetic, `hint/vm.rs` the stack, decoding and
dispatch, then a file a family — `flow.rs`, `storage.rs`, `state.rs`,
`moves.rs`, `shift.rs`, `iup.rs`, `glyph.rs` — and `hint/tests/` a file a
family.

### Numbers

- **The stack holds 32-bit two's-complement values**, and each run of a
  program starts it empty. A coordinate, a distance and a CVT value are
  F26Dot6 (26.6); a vector component is F2Dot14 (2.14); a count, an index, a
  flag word and a selector are integers. The instruction tables label some
  stack elements `uint32`, a width; every comparison (`LT` to `NEQ`, `MAX`,
  `MIN`) is signed, as a distance is. A pushed byte is padded to 32 bits
  unsigned and a pushed word sign-extended, as *Pushing data onto the
  interpreter stack* states.
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
  difference of zero sets the vector to the x axis, this engine's choice. The
  vector points from the point popped first, in the zone `zp2` points at,
  toward the second, in `zp1`'s, as both specifications have it, and a flag
  of 1 turns it a quarter counter-clockwise, `(x, y)` to `(−y, x)`.
  `SDPVTL` sets the dual vector from the two points' original positions and
  the projection vector from their current ones; every other instruction
  that sets the projection vector sets the dual vector to it. `SPVFS` and
  `SFVFS` take their two components as given, each the low 16 bits of its
  value.
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
move sets them. A point `MIAP`, `MIRP`, `MSIRP` or `SCFS` moves in zone 0 takes
its new position as its original too, and `MIAP` and `MIRP` take such a
point's original measurement — the coordinate `MIAP`'s cut-in test compares,
the distance `MIRP`'s does — to be the CVT value's, since it has no outline to
measure. That is this engine's choice where the specifications say only that
these instructions "create" twilight points, and that a new reference to one
"will create a completely new position that has nothing to do with the
previous position of the point" (Apple's manual, chapter 2). A point index
past its zone, a contour index past the glyph and a zone number other than 0
and 1 are errors.

**The phantom points** follow the contour points, as the OpenType text places
them: `n` at the horizontal origin, `xMin` less the left side bearing; `n+1`
the advance width past it; `n+2` the vertical origin, `yMax` plus the top
side bearing; `n+3` the advance height below it. Horizontal metrics are
`hmtx`'s; vertical ones `vmtx`'s, or `hhea`'s ascender and descender where the
face has none, this engine's choice. The horizontal pair stands at `y = 0`
and the vertical pair at `x = 0`, the same choice, which
`glyf::phantom_points` makes and `gvar` moves
([variable fonts](variable-fonts.md)). Each is scaled as the points are.

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

**Rounding.** Each round state is a period, a phase and a threshold, the
predefined ones as Apple's manual tabulates them (chapter 2, table 19): `RTG`
1, 0, 1/2; `RTHG` 1, 1/2, 1/2; `RTDG` 1/2, 0, 1/4; `RDTG` 1, 0, 0; `RUTG` 1,
0, 63/64; `ROFF` no rounding. A value at or above zero rounds by *SROUND*'s
steps — subtract the phase, add the threshold, truncate to the next lowest
multiple of the period, add the phase back — and one that became negative
takes the positive round value closest to zero, the phase; a negative value
rounds as its magnitude does, negated, since "rounding is symmetric about
zero" (Apple's manual, `ROUND[ab]`). `SROUND` and `S45ROUND` decode their
value's low byte by the OpenType tables: the period gridPeriod/2, gridPeriod
or gridPeriod·2, the fourth code reserved and an error; the phase 0,
period/4, period/2 or gridPeriod·3/4, the last as the OpenType table gives it
where Apple's reads period·3/4 (the two agree at a period of one grid); the
threshold the period less 1/64 for code 0, "largest-number-smaller-than-
period", and the period times (t − 4)/8 for code t. `SROUND`'s gridPeriod is
one pixel; `S45ROUND`'s is √2/2 pixels, as the OpenType text states (Apple's
manual gives √2), so its periods are √2/4, √2/2 and √2 pixels, each to the
nearest 64th — 23, 45 and 91 — and its gridPeriod 45, every part taken half
away from zero to a 64th. `ODD` and `EVEN` test the whole pixel at or below
the rounded value.

### Measuring and moving

`GC[a]` and `MD[a]` measure the current outline on the projection vector for
`a = 0` and the original one on the dual projection vector for `a = 1`, as
both specifications' flag tables give it where their prose and figures
disagree; `MD`'s distance runs from the point popped second, in `zp0`'s zone,
to the one popped first, in `zp1`'s. Every instruction that moves a point
touches it on each axis the freedom vector has a component along, whether or
not the move is zero ("Whenever an instruction has the effect of moving a
point, that point is marked as touched", *Instructing TrueType Glyphs*), and
`MDAP[0]` touches without moving.

- **The cut-in test** uses the outline's measurement where it and the CVT
  value differ by more than the control value cut-in, and the CVT value
  otherwise, at equality too, as *Instructing TrueType Glyphs*' table of the
  cut-in shows. `MIAP` measures the point's original coordinate on the
  projection vector; `MIRP` the original distance from `rp0` on the dual.
- **The single width** replaces a distance whose magnitude is within the
  single width cut-in of it, keeping the distance's sign: `MDRP`'s original
  distance, `MIRP`'s CVT value.
- **Auto flip** makes `MIRP`'s CVT value take the original distance's sign,
  zero counting as positive. It touches no other instruction: the OpenType
  text's `FLIPON` and `FLIPOFF` name `MIRP` alone, and `MIAP`'s CVT value is
  a coordinate, whose sign means where it is.
- **`MIRP`** takes the CVT value, its single width, auto flip, then — where
  `c` is set — the cut-in test and the round state; **`MDRP`** the original
  distance, its single width, then the round state where `c` is set; each
  then keeps the minimum distance where `b` is set, in the distance's own
  direction, a distance of zero taking the original distance's, this
  engine's choice. `MSIRP` keeps its distance as popped.
- **The shifts.** `SHP`, `SHC` and `SHZ` move each of their points by how
  far their reference point — rp2 in `zp1`'s zone for `a = 0`, rp1 in `zp0`'s
  for `a = 1` — stands from its original position on the projection vector;
  `SHC` and `SHZ` leave the reference point where it is one of theirs, as
  *SHC* says and *SHZ*, "similar to SHC", follows, and `SHZ` moves every
  point of its zone, the phantom points among them. A contour exists only in
  the glyph's zone. `SHPIX` moves its points by its amount along the freedom
  vector alone, `amount · f / 16 384` to the nearest whole on each axis.
- **The alignments.** `ALIGNRP` brings each point's distance from rp0 to
  zero. `ALIGNPTS` pops p1, in `zp1`'s zone, then p2, in `zp0`'s, as the
  OpenType text has it (Apple's manual gives the zones the other way round),
  and moves both to the average of their coordinates on the projection
  vector, rounded half away from zero.
- **`IP`** moves each point so its distance from rp1 on the projection vector
  is its original distance from rp1 on the dual, times the references'
  current distance over their original one, to the nearest whole; an
  original distance of zero is an error.
- **`ISECT`** puts its point at the current lines' intersection, solved in
  128 bits and each coordinate rounded half away from zero, touched on both
  axes since it ignores the freedom vector; lines with no single
  intersection — parallel, or one of no length — take the four points'
  average, the OpenType text's middle.
- **`UTP`** untouches a point across for a freedom vector with no component
  up, up for one with none across, and both otherwise.
- **The flips** change only the on-curve flag and touch nothing; a
  `FLIPRGON` or `FLIPRGOFF` whose highest point is below its lowest names no
  point, this engine's reading. `AA` pops its point and does nothing.
- **The loop.** `SHP`, `SHPIX`, `IP`, `FLIPPT` and `ALIGNRP` repeat for the
  loop count, popping a point each time, and leave it at one.

### The pipeline

**A face** is read once: `maxp`'s limits, `head`'s units per em and flags,
`cvt `, `fpgm`, `prep` and `gasp`. `fpgm` runs once, defining the functions
and instruction definitions every later program may call; a face whose `fpgm`
fails is unhinted at every size.

**A size** is set up once a face and `GlyphSize`: the CVT's FWORDs scaled to
26.6 at the size, `round(v · ppem / unitsPerEm)` in 64 bits — as `WCVTF` and
`SSW` scale font units too, and to zero for a face that states no units per
em — the storage area cleared to zeros, the twilight zone set to the origin,
and `prep` run over them. A storage location never written reads zero, this
engine's answer where the specification calls the value "unpredictable"; a
storage location or a CVT entry past its table is an error. The CVT, the storage area, the twilight zone and the graphics-state
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
the summary's defaults rather than `prep`'s. The flag a selector chooses
takes the value's bit of the same weight, so 0 clears it and 1, 2 or 4 — the
values the specification lists for selectors 1, 2 and 3 — sets it. In a
glyph program and in `fpgm` the instruction pops its two values and changes
nothing, as "INSTCTRL[] can only be executed in the CVT program" (Apple's
manual) and "Flags 1 and 2 must be set in the CV program" have it. The third
selector is kept and changes nothing (*Non-goals*). A selector past 3, or
below 1, is ignored.

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

- **`MPPEM`** answers the size's whole pixels per em, `ppem` rounded to the
  nearest pixel, along whichever vector is the projection vector, since a
  hinted glyph's two axes are one size. **`MPS`** answers
  `GlyphSize::points`, in 26.6.
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
| Instructions a run executes | 1 048 576 (2^20) for each run of `fpgm`, `prep`, a glyph's or a component's program, counting each instruction dispatched, each instruction `IF`, `ELSE`, `FDEF` and `IDEF` pass over, each stack element `MINDEX` moves, each point a loop-counted instruction moves, each point `SHC`, `SHZ`, `FLIPRGON` and `FLIPRGOFF` act on, and each call `LOOPCALL` makes |
| Call depth | 64: `CALL`, `LOOPCALL` and a dispatch to an `IDEF` nest no deeper |
| The instruction stream | a push that runs past its program's end, a jump outside it, an `IF` with no `EIF` or a definition with no `ENDF` |
| Where definitions may stand | `FDEF` and `IDEF` in a glyph program or inside a definition, and `ENDF` outside one |
| What instructions name | an undefined function, an `IDEF` of a value that is no opcode, an opcode with no instruction and no `IDEF`, a zero `DIV`, a loop count below one ("Setting the loop variable to zero is an error"), `SROUND`'s reserved period, an orthogonal freedom vector at a move, a contour the zone lacks (the twilight zone has none), `IP` between reference points at one original position ("This instruction is not valid if rp1 and rp2 have the same position on the projection_vector"), `IUP` in zone 0 |

`IDEF` affects only undefined opcodes and is ignored for a defined one, as
the specification says: the 37 opcodes the instruction set summary assigns
to no instruction. `GETVARIATION`'s 0x91 is one of them here, since this
engine does not execute it (*Non-goals*), so the `IDEF` the OpenType text
requires a font that uses it to carry stands in for it. A definition's body
is the bytes from the instruction after its `FDEF` or `IDEF` to the first
`ENDF` after it, push data passed over; a jump inside a definition is
relative to it and may not leave it, and its end returns. A later
definition of the same number or opcode replaces an earlier one, and the
definitions `fpgm` and `prep` make last from run to run. A run also stops when the renderer's stop predicate
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
hinted glyph never has, so bits 12 and 13 turn it off; bits 14 and 15 are
reserved and read as nothing. A `SCANTYPE` mode outside 0, 1, 4 and 5 is
rules 1 and 2 alone. The state the size's programs leave travels with the
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

## Stem darkening

*For ROADMAP row FT-04a.* A stem a pixel wide whose edges fall between
pixels draws as two half-grey columns, and an anti-aliased page of small text
reads lighter than the same text printed. Darkening thickens every filled
glyph a little at small sizes, more across than down, fading to nothing as
the size grows: each contour of the glyph's device-space outline is moved
outward by a stated amount, so its stems gain that amount on each side and its
counters lose it.

### Scope

- **The amount** in device pixels, horizontal and vertical, as a function of
  a glyph's pixels per em, over 8 to 16 pixels and to where it fades to none.
- **The outline offset** that applies it (FT-04a.2), on `tinker-pdf-raster`'s
  device-space `Path`.
- **Its place in the seam** above, and the option that turns it on
  (FT-04a.3).
- **The tolerance** a stem-width measurement has on the rasterizer's grid.
- **Its proof**: the stem-width test, the fingerprint on every target and the
  speed row (FT-04a.3 to FT-04a.5).

### Non-goals

- **Darkening by a gamma or a contrast curve.** That is RC-10's text contrast,
  which maps coverage after the fill; darkening changes the shape before it.
- **Hinting.** Darkening thickens what hinting, or nothing, gives it; FT-04b,
  FT-04c and FT-04d fit the outline first.
- **Strokes, clips and Type 3 glyphs**, and every output that is not a raster:
  `to_svg`, text extraction and the display list's recorded geometry.
- **Matching another renderer's curve.** The amount is this design's
  statement, and the test holds the rasterizer to it, never to another
  program's output (ruling 13).

### Design

#### The amount

A glyph's pixels per em `p` is the square root of the absolute determinant of
its em-to-device 2×2 — the glyph's transform in the page frame, never a
tile's — which a rotation does not change. Its horizontal and vertical amounts,
each per side, in device pixels:

```text
across(p) = clamp(24 − p, 0, 16) / 64
down(p)   = clamp(24 − p, 0, 16) / 128
```

| Pixels per em | `across`, per side | `down`, per side | A vertical stem gains | A horizontal stem gains |
| ---: | ---: | ---: | ---: | ---: |
| 8 and below | 16/64 | 8/64 | 1/2 | 1/4 |
| 9 | 15/64 | 15/128 | 15/32 | 15/64 |
| 10 | 14/64 | 7/64 | 7/16 | 7/32 |
| 11 | 13/64 | 13/128 | 13/32 | 13/64 |
| 12 | 12/64 | 6/64 | 3/8 | 3/16 |
| 13 | 11/64 | 11/128 | 11/32 | 11/64 |
| 14 | 10/64 | 5/64 | 5/16 | 5/32 |
| 15 | 9/64 | 9/128 | 9/32 | 9/64 |
| 16 | 8/64 | 4/64 | 1/4 | 1/8 |
| 20 | 4/64 | 2/64 | 1/8 | 1/16 |
| 24 and above | 0 | 0 | 0 | 0 |

The amount is linear in `p` between 8 and 24, so a size between two rows lies
between them; every entry is a dyadic fraction, which `f64` holds exactly.
**Across is twice down**: a horizontal gain is taken from a glyph's side
bearings, where it changes no metric the page shows, while a vertical one
raises the x-height and the cap height by twice the amount, and a stem's
weight is read across the line far more than down it.

**Which axis is which.** For a glyph whose 2×2 is axis-aligned (`b` and `c`
zero) across is the page frame's `x` and down its `y`; for one turned a
quarter turn (`a` and `d` zero) the two swap, so the glyph's own vertical
stems still take `across`. A glyph at any other angle, or skewed, takes
`down` on both axes: the one offset that does not depend on the angle, and the
lighter of the two.

#### The offset

`Path::offset(&self, horizontal: f64, vertical: f64) -> Path`
(`crates/tinker-pdf-raster/src/geom.rs`) moves every point of every closed
contour, on and off the curve alike, so that each edge of the contour's
control polygon moves outward by `horizontal` along `x` and `vertical` along
`y` in proportion to its normal:

- **The outward side** is taken from the contour's signed area against the
  path's: a contour whose area has the sign of the path's total is an outer
  contour and grows; one of the other sign is a counter and shrinks, its points
  moving into it. Both are one handedness of normal: for a path of positive
  total area, the normal on the right of each edge's direction, `(ey, −ex)`
  normalised. A glyph drawn with either winding grows; a path whose total area
  is zero, or not finite, is returned as it came.
- **Each point moves along the bisector of its two edges' outward normals**,
  `n_in` and `n_out`, by the miter vector `m = (n_in + n_out) / (1 + n_in ·
  n_out)`, scaled per axis: the point moves by `(horizontal · m.x,
  vertical · m.y)`. On a straight run `m` is the edge's normal, and at an
  axis-aligned corner `m` is `(±1, ±1)`, so a rectangle grows by exactly
  twice each amount, one on each axis.
- **Bounded at a spike.** `m`'s length is `sqrt(2 / (1 + n_in · n_out))`; past
  4 — a turn sharper than about 151° — it is held at 4 along the bisector, and
  a turn back on itself, where the bisector vanishes, keeps its point.
- **A degenerate edge keeps its point.** A point's neighbours are the nearest
  points of its contour that differ from it, read once around the contour, so
  a zero-length edge between two equal points gives them both the move the
  edges either side of them give; a contour all of one point keeps it.
- **Finite in, finite out.** Every length is taken by scaling the difference
  by its largest component before the square root, so no square overflows; a
  point whose move is not finite keeps its place.
- **What a fill closes, this closes.** A subpath the path leaves open is
  offset as the closed contour the fill makes of it (8.5.3.1), and one that
  begins after a `Close` without a `MoveTo` gains one at its first point, so
  the offset path fills what the offset of the filled shape is.

Only `sqrt`, correctly rounded by IEEE 754, and the four operations are used,
so the offset is the same on every target, and nothing on the path calls the
platform's libm (`cargo xtask libm`, ruling 4).

#### Its place in the seam

`show_glyph` darkens a glyph after any hinting and before filling it: the
outline the seam above chooses — the hinted one at its rounded origin, or the
em outline at its placement — is offset by the amounts for the glyph's pixels
per em, in the frame it is filled in. That is the page frame's device pixels
on the path route, and the mask's own frame on the mask route, which differs
from the page frame by a whole pixel and a phase and is the same in a tile as
on the page; so a tile's glyph is the page's (ruling 5), and the tile's
origin enters neither the amount nor the offset.

It applies to a glyph whose render mode is 0 and nothing else, in any paint,
solid or pattern, as hinting does; a stroked or clipping glyph, a Type 3 glyph,
`to_svg`, text extraction and the display list's recorded geometry are
untouched, and `DisplayList::render` darkens as `Page::render` does, drawing
through the renderer. Both glyph mask keys gain whether the glyph is darkened;
the amount is a function of the 2×2 they already hold.

#### The option

```rust
// tinker_pdf::RenderOptions
pub stem_darkening: bool,   // false by default
```

handed through `render_layer_with` to the `Renderer` as `antialias` is. **Off
by default**, so no fingerprint or golden moves when it lands; whether it
turns on is H-65's judgement.

#### The tolerance of a stem-width measurement

FT-04a.3's test measures a stem's width from the bytes of a rendered row or
column, as the sum of the coverage it crosses divided by 255, and holds it to
the stem plus twice the amount. The rasterizer's grid
(`docs/features/rasterizer.md`, *Filling*) bounds how far that sum can be from
the geometry:

- **A vertical stem, across a row it fully covers: within 1/64 pixel.** Each
  edge is taken to the nearest 1/256 pixel, at most 1/256 for the two edges
  together, and a partly covered pixel's byte is its overlap in 1/256 units,
  read against 255 rather than 256, at most 255/65 280 of a pixel for each of
  the two edge pixels; the whole is under 0.012.
- **A horizontal stem, down a column it fully covers: within 9/128 pixel.**
  The sixteen sub-scanlines a pixel row samples at its sixteenths count an
  edge's position to a sixteenth, so the two edges' count differs from the
  stem's height by less than one sub-scanline, 1/16 pixel; a partly covered
  pixel's byte is sixteen a sub-scanline, read against 255, at most 15/4 080 of
  a pixel for each of the two; the whole is under 0.070.

Each bound is the grid's own, stated in advance, and the test takes the row
or column in the middle of the stem, where the other axis covers whole
pixels.

### Milestones

1. **FT-04a.2, the offset.** `Path::offset` in `geom.rs`. Exit: `geom.rs`'s
   `an_axis_aligned_square_grows_by_twice_the_amount_per_axis`,
   `a_counter_shrinks_as_its_outer_contour_grows`,
   `either_winding_grows_outward`,
   `a_zero_length_edge_or_collinear_point_gives_no_nan`, and a sweep of
   generated paths and amounts whose every point stays finite.
2. **FT-04a.3, darkened glyphs.** `RenderOptions::stem_darkening` to the
   renderer, the amounts above and the offset in `show_glyph` on both routes,
   and the keys. Exit: `stem_darkening.rs`'s
   `stem_width_at_8_to_16_px_is_the_designed_amount` — a face built in the
   test with one vertical and one horizontal stem of known width, rendered at
   each whole size from 8 to 16 pixels an em, each stem's width the stem plus
   twice the table's amount within the tolerances above —
   `darkening_off_changes_no_pixel` and
   `a_darkened_page_tiles_byte_equal_to_itself`, in the default and
   `bundled-fonts` builds.
3. **FT-04a.4, every target.** Exit: the stem-width test in K-07's roster, and
   `determinism.rs`'s darkened text page as a fingerprint, passing on Linux,
   Windows and `wasm32-wasip1`.
4. **FT-04a.5, the speed row.** Exit: `xtask/src/bench.rs`'s
   `the_committed_baseline_agrees_with_the_tree` with the operation that
   renders the text page with an embedded face at 150 dpi with
   `stem_darkening` on, beside the same page off.

### Risks

| Risk | Mitigation |
| --- | --- |
| A sharp corner throws a spike past the glyph | The miter is held at four times the amount, and a reversal keeps its point; at 8 pixels an em that is a pixel at most |
| A glyph whose separate outer contours wind opposite ways, which the non-zero rule fills alike, has those that wind against the total shrink | The signed areas decide, as every well-formed glyph's outer contours share a winding against its counters'; off by default, and the stem-width test draws the convention fonts keep |
| Text turned to an angle darkens less than upright text | Stated: such a glyph takes `down` on both axes, the lighter amount, so no direction grows more than upright text's |
| The offset moves a pixel with the option off | `darkening_off_changes_no_pixel`; nothing calls `Path::offset` unless the option is on |
| A tile and the page darken a glyph differently | The amount comes from the 2×2 and the offset from the path in a frame a tile shares; `a_darkened_page_tiles_byte_equal_to_itself` |

## CFF and Type 1 hint application

*For ROADMAP row FT-04c.* A CFF or Type 1 glyph carries no program: it
declares its stems — the ranges its horizontal and vertical strokes occupy —
and its font declares alignment zones and standard widths, and the
specifications leave the moving of the outline to the rasterizer ("A
declarative hint system depends on an intelligent rasterizing algorithm",
*Adobe Type 1 Font Format* 5.1). This section transcribes what the
specifications state about those hints at a device size, marks every rule it
adds where they leave the choice to the rasterizer, and settles one fitter in
`tinker-pdf-font` that the CFF reader and the Type 1 reader both feed and
FT-04b's per-size request reaches.

### Scope

- **The specifications' statements**, clause by clause: the Private
  dictionary's hint entries, stem and edge hints, hint replacement and hint
  masks, counter control, alignment zones and their overshoot, and flex at a
  device size.
- **The fitter** (FT-04c.2 to FT-04c.4): `stem_hints`, format-independent —
  a glyph's stems, edge hints, regions and counter groups and its font's
  zones and standard widths in, a fitted outline out — by one rule table.
- **The readers feeding it** (FT-04c.5, FT-04c.6): what the CFF reader and
  its Type 2 interpreter, and the Type 1 reader and its charstring
  interpreter, record for it, and flex decided at the size.
- **Answering the request** (FT-04c.7): `Hinting::Font` and
  `Hinting::FontOrAuto` reaching a CFF or Type 1 glyph through
  `GlyphSource::sized_outline`.
- **Proof** (FT-04c.8, FT-04c.9a): the fingerprint on every target and the
  speed row.

### Non-goals

- **Hinting a glyph that carries no hints.** A CFF or Type 1 glyph with no
  stem and no edge hint is FT-04d's, under `FontOrAuto`, and unhinted under
  `Font`.
- **Multiple-master blends** (FT-10): a Type 1 multiple-master font's
  blended hints, and the supplement's section 3, are read by no step here; a
  multiple-master glyph draws unhinted.
- **A CFF2 glyph's hints.** A CFF2 face is a variable face, drawn at its
  default (`docs/design/variable-fonts.md`); its Private DICT's entries may
  blend, and no step here reads them, so a CFF2 glyph draws unhinted under
  `Font`, as a glyph that carries no hints draws.
- **Stem darkening** (FT-04a), which thickens what the fitter gives it.
- **Hints in any output but the raster**, as for every hinter (*What stays
  unhinted*).
- **Matching another rasterizer.** The specifications state what the hints
  mean and leave how a stem is moved to the rasterizer; every rule this
  section adds is this engine's, held by a test to this document and never to
  another program's output (ruling 13).

### Design

#### The specifications

The rules are read from four documents, and from nothing else (ruling 21):
*Adobe Type 1 Font Format* (version 1.1), chapters 5 (*Private Dictionary*),
6 (*CharStrings Dictionary*, 6.4's hint commands and 6.5's *Character Level
Hints*) and 8 (*Using Subroutines*, 8.1 to 8.4); *Type 1 Font Format
Supplement* (Technical Note #5015, 15 May 1994), section 2 (*Counter Control
Hints*); *The Compact Font Format Specification* (Technical Note #5176,
version 1.0), section 15 and its Table 23; and *The Type 2 Charstring
Format* (Technical Note #5177, 16 March 2000), sections 2.1 (*Hints*), 2.2
(*The Flex Mechanism*), 4.1's flex operators, 4.3 (*Hint Operators*) and
Appendix B. In the rule table a rule is **Stated**, with its clause, where
the specifications say it, and **Engine** where they leave it to the
rasterizer and this document chooses; a test holds an Engine rule as this
document's.

Throughout, `s` is the size in pixels per character-space unit: the request's
pixels per em (`GlyphSize::ppem / 64`) times the glyph's `FontMatrix` scale —
the Type 1 font's, the CFF Top DICT's, and for a CID-keyed CFF glyph its Font
DICT's after the Top DICT's. BlueScale is stated in the same unit ("the number
of pixels tall that one character space unit will be", T1 5.6): its default
`0.039625` is one unit at 9.51 points and 300 dots per inch.

#### The fitter

`tinker-pdf-font`'s `stem_hints` module is a leaf like the rest of the crate
(ruling 8): numbers in, an outline out.

```rust
pub struct FontHints {                 // a Private dictionary's hint entries
    pub blue_values: Vec<f64>,         // pairs, as read (rule Z1)
    pub other_blues: Vec<f64>,
    pub family_blues: Vec<f64>,
    pub family_other_blues: Vec<f64>,
    pub blue_scale: f64,               // 0.039625 where absent
    pub blue_shift: f64,               // 7
    pub blue_fuzz: f64,                // 1
    pub std_hw: Option<f64>,
    pub std_vw: Option<f64>,
    pub stem_snap_h: Vec<f64>,
    pub stem_snap_v: Vec<f64>,
    pub force_bold: bool,              // false
    pub language_group: u8,            // 0 or 1 (rule C1)
    pub expansion_factor: f64,         // 0.06
}

pub enum StemKind { Stem, TopEdge, BottomEdge }   // top/right, bottom/left

pub struct Stem {
    pub vertical: bool,                // a vstem: an x range
    pub low: f64,                      // absolute, in character space
    pub high: f64,
    pub kind: StemKind,
}

pub struct GlyphHints {
    pub stems: Vec<Stem>,              // declaration order, at most 96
    pub regions: Vec<Region>,          // each: the stems active, the first point it holds
    pub counters: Vec<CounterGroup>,   // priority order: one axis, its stems ascending
}

pub fn fit(points: &[(f64, f64, usize)], hints: &GlyphHints, font: &FontHints,
           s: f64) -> Option<Vec<(i32, i32)>>
```

The reader hands the fitter every point of the glyph's outline — on and off
the curve, in the order its interpreter emitted them, each with the region
in force when it was emitted — and puts the fitted points back into the
outline's segments, in pixels about the glyph's origin, each on 1/64 pixel
(26.6). The arithmetic is `f64`'s four operations and `floor`, and
`hint::fixed`'s rounding where a value becomes 26.6 (FT-04b's section): no
transcendental (ruling 4). `None` is the answer for a glyph the fitter cannot
fit — `s` not a finite number above zero, a `FontMatrix` that is not one
uniform scale, no stem left after the bounds — and the glyph then draws
unhinted (*Failure*).

#### The rule table

**Zones.**

| Rule | What | Source |
| --- | --- | --- |
| Z1 | `BlueValues` is pairs; its first pair is a bottom zone, *baseline overshoot position and baseline*, and every later pair a top zone, *flat position and overshoot position*; `OtherBlues` is bottom zones only; `FamilyBlues` and `FamilyOtherBlues` are read alike. Up to seven pairs in `BlueValues` and `FamilyBlues`, five in the others. A CFF Private DICT gives each as a delta array, read as the running sums | Stated: T1 5.3 to 5.5; 5176 Table 23 and section 4 |
| Z2 | The flat edge of a bottom zone is its upper value and of a top zone its lower. A pair whose first value exceeds its second is dropped, an odd last value and the pairs past seven or five are ignored, and a value that is not finite drops its pair | Engine, the specifications requiring well-formed arrays |
| Z3 | Only a horizontal stem's edges are aligned: its top edge by a top zone, its bottom edge by a bottom zone — "that character must have an `hstem` hint specified at or in an alignment zone"; a vertical stem meets no zone | Stated: T1 5.2, 6.5 |
| Z4 | A zone captures an edge within it, or within `BlueFuzz` units outside it on either side ("act as if the stem top were actually within the zone"); `BlueFuzz` defaults to 1. Where two zones would capture one edge, the first in reading order (`BlueValues`, then `OtherBlues`) does | Stated: T1 5.8; 5176 Table 23. The tie: Engine |
| Z5 | Where `s` is below `BlueScale`, overshoot is suppressed: "All features falling in an alignment zone are rendered at the same pixel height" — every captured edge is placed at the zone's flat position, `round(flat · s)` | Stated: T1 5.6. The rounding: Engine |
| Z6 | A font whose `BlueScale` times its tallest zone's height is 1 or more breaks 5.6's restriction, "ensures that overshoot suppression will turn off before the overshoot reaches a full device pixel": suppression holds only where `s` is also below one over that height | Engine, keeping the restriction's purpose |
| Z7 | Where `s` is at or above `BlueScale`, an edge beyond its zone's flat position — above it for a top zone, below for a bottom zone — by at least `BlueShift` units (7 by default) overshoots, and one closer overshoots "only if their device space distance is at least one-half pixel" | Stated: T1 5.7; 5176 Table 23 |
| Z8 | An edge that overshoots is placed at the zone's rounded flat position moved outward by `round(d · s)` pixels and at least one, for `d` its distance from the flat position; one that does not, and one captured on the zone's flat side, at the rounded flat position | Engine |
| Z9 | A family zone replaces the font's own where "the difference between a font's alignment and its family's standard alignment is less than 1 pixel": the zone of `FamilyBlues` (`FamilyOtherBlues`) at the index of a zone of `BlueValues` (`OtherBlues`), its flat position less than one pixel from the font's at `s`, is used in its place at that size | Stated: T1 5.5. Pairing by index and comparing flat positions: Engine |

**Stems and edges.**

| Rule | What | Source |
| --- | --- | --- |
| S1 | Type 1: `y dy hstem` declares `y` to `y + dy` and `x dx vstem` `x` to `x + dx`, relative to the left side bearing point (`hsbw`, `sbw`). Type 2: `hstem`, `vstem`, `hstemhm`, `vstemhm` take pairs, the first value of the first pair relative to 0 and every later value relative to the edge before it; the stems are in ascending order of their first edge | Stated: T1 6.4; 5177 4.3 |
| S2 | Type 2 edge hints: a width of −20 is a top (right) edge, −21 a bottom (left) edge, "used to locate an edge rather than a stem that has two edges"; the edge is the pair's position for −20 and that position less 21 for −21 (5177's figure 4, `121 –21 400 –20 hstem` hinting the edges at 100 and 500). Another negative width "has undefined meaning": such a stem is dropped | Stated: 5177 2.1, 4.3. Dropping: Engine |
| S3 | Type 1 ghost stems "must be created with a stem height of 20 or 21 — either is acceptable" and lie inside the glyph's range: an `hstem` 20 or 21 units high whose top a top zone captures is a top edge hint, else one whose bottom a bottom zone captures a bottom edge hint, and any other is a stem | Stated: T1 6.5. Reading the edge off its zone: Engine |
| S4 | At most 96 stem hints a glyph, horizontal and vertical together; a stem past the 96th is dropped. Type 1 is held to the same number | Stated: 5177 Appendix B. Type 1: Engine |
| S5 | Stems of one direction must not overlap; a stem overlapping one declared before it in the same region is dropped from that region | Stated, as a constraint: T1 6.4; 5177 4.3 ("the results are undefined"). Dropping: Engine |
| S6 | A stem's width snaps to a standard width "when the difference between a standard stem width and a particular stem width is small": the nearest of `StdHW` and `StemSnapH` (horizontal stems) or of `StdVW` and `StemSnapV` (vertical ones), the smaller on a tie, is taken where it is less than half a pixel from the stem's width at `s` | Stated: T1 5.9. The half pixel and the tie: Engine |
| S7 | A stem is drawn a whole number of pixels wide, `floor(w · s + 1/2)` for its width `w` after S6, and at least one pixel | Engine |
| S8 | With `ForceBold` true, a stem S7 makes one pixel wide whose width at `s` is more than one pixel is drawn two: "in situations where character stems would normally be rendered at 1-pixel thick, a Type 1 font interpreter may thicken the stem"; false, or absent, nothing is thickened | Stated: T1 5.10; 5176 Table 23 (`false` by default). The rule: Engine |
| S9 | A stem with no captured edge keeps its centre as nearly as the grid allows: its lower edge at `round(c − w/2)` for its centre `c` in pixels and its fitted width `w`, its upper edge `w` above | Engine |
| S10 | A captured edge is placed by Z5 to Z8 and the stem's other edge at its fitted width from it; a stem both of whose edges are captured has each placed by its zone, and is held to at least one pixel by moving its bottom edge down | Engine |
| S11 | An edge hint is one edge: placed by its zone where one captures it, at the nearest whole pixel otherwise; the other number of its pair is no edge | Engine |
| S12 | Fitted edges keep their order on each axis: an edge that would pass the next one in design order is held at it | Engine |

**Regions.**

| Rule | What | Source |
| --- | --- | --- |
| R1 | Type 1 hint replacement, `subr# 1 3 callothersubr pop callsubr`, discards the stems in force and declares the ones subroutine `subr#` holds, for the outline that follows; othersubr 3 answers its argument, so the subroutine is called. `Subrs` 0 to 3 are the fixed flex and replacement entries | Stated: T1 8.1, 8.4 |
| R2 | Type 2 `hintmask` makes active the declared stems its bits name, the most significant bit of the first byte the first stem, for the outline that follows; "the number of data bytes is exactly the number needed" for the stems declared, and bits past them are ignored. `hstem` and `vstem` before a `hintmask` with no `vstem` operator between are its implied `vstem` | Stated: 5177 2.1, 4.3 |
| R3 | Before a glyph's first `hintmask` every declared stem is active; in Type 1, every stem declared so far | Engine, the specifications implying it where no mask is given |
| R4 | A point is fitted by the region in force when the operator that emitted it ran; the regions of one glyph are fitted apart | Engine |
| R5 | `dotsection` brackets a dot "for compatibility with the oldest PostScript interpreters", which hint replacement handles today; it changes nothing | Stated as obsolete: T1 8.2. Ignoring it: Engine |

**Counters.**

| Rule | What | Source |
| --- | --- | --- |
| C1 | `LanguageGroup` is 0 (Latin, Greek, Cyrillic and the like) or 1 (Chinese, Japanese, Korean); absent, or any other value, it is 0 | Stated: T1 5.11; 5176 Table 23 |
| C2 | Counter groups come from three places: `hstem3` and `vstem3`, three stems whose outer two are equally wide and whose centres are equally spaced, "these stems and the counters between them will all be controlled"; Type 2 `cntrmask`, each command one group of the stems its bits name, a group of each direction where it names both, earlier commands first; and the supplement's othersubrs 12 and 13 after `hsbw`, read in its data format — `#H HG1 … HGn #V VG1 … VGn`, each group's stems as the distance from the previous stem and a width, the last stem's distance to its far edge and its width negative, the groups in priority order | Stated: T1 6.4; 5015 2.2 to 2.4; 5177 2.1 |
| C3 | Under `LanguageGroup` 0 only stem-three groups are fitted: `hstem3`, `vstem3`, and a `cntrmask` naming exactly three stems of one direction, which "can be used in place of" them; any other group is read and not fitted. Under 1 every group is fitted | Stated: 5015 2 (counter control needs `LanguageGroup` 1); 5177 2.1 |
| C4 | Counters of one design width come out one number of pixels "providing there are a sufficient number of pixels available", and their proportions are kept "based on the constraints of the glyph's width" | Stated: 5015 2 |
| C5 | Groups are fitted in priority order, the first group's counters "the last to collapse" | Stated: 5015 2.4; 5177 2.1 |
| C6 | `ExpansionFactor`, 0.06 by default, bounds how far a group may change the glyph's extent, "a ± 6% change in the character bounding box", under `LanguageGroup` 1 | Stated: T1 5.14; 5015 2.5; 5176 Table 23 |
| C7 | A group is fitted after its stems' widths, in each region whose active stems hold all of its stems. Its span, from its first stem's lower edge to its last stem's upper edge, starts at `round(D · s)` pixels for its design span `D`; the white left after its stems is shared among its counters in proportion to their design widths, each taking the floor of its share and the pixels over going one at a time to the largest remainders. Where that gives counters of one design width different numbers, the span grows or shrinks by the fewest pixels that make them equal — within `ExpansionFactor` of `D · s` under `LanguageGroup` 1, by at most one pixel under 0 — and where no such span exists the remainders go leftmost first. Where the white is fewer pixels than the counters, the narrowest counters close first, and a later group's before an earlier one's (C5). The group's first stem is placed by S9 or S10 and the others follow at their counters; a stem a higher group has placed keeps its place and splits a later group there. A group whose stems overlap or are out of order is passed over | Engine |

**Flex and accents.**

| Rule | What | Source |
| --- | --- | --- |
| F1 | Type 2 `flex` draws its two curves as one straight line from the start to the end where the flex depth is less than `fd`/100 device pixels, and as curves otherwise; `hflex`, `hflex1` and `flex1` take `fd` 50 | Stated: 5177 2.2, 4.1 |
| F2 | Type 1 flex — `Subrs` 1, seven `rmoveto`s each followed by `Subrs` 2, then the height control, the end point and `Subrs` 0 — draws the curves "if the Flex height renders to" the control parameter's hundredths of a pixel "or more", and a straight line otherwise | Stated: T1 8.3, 8.4 |
| F3 | The depth is measured at `s`: for a flex more horizontal than vertical (its start-to-end `dx` larger than `dy`, 5177's `flex1` test) as the vertical distance from the joining point to the line through the start and end points at the joining point's `x`, and for one more vertical as the horizontal distance at its `y` | Stated: the comparison, 5177 4.1 and figure 3. The distance: Engine |
| F4 | An accented glyph (`seac`, or `endchar` with four arguments in Type 2) fits each component with its own hints at the size and places the accent at its offset rounded to whole pixels, so its fitted edges stay on the grid | Engine |

**Interpolation.**

| Rule | What | Source |
| --- | --- | --- |
| I1 | On each axis a region's fitted edges map its points: a point at an edge's design position goes where the edge went, one between two edges moves linearly by its design position between theirs, and one beyond the outermost edge moves as that edge moved; an off-curve point is mapped as an on-curve one is. An axis with no edge in a region is scaled and not moved | Engine |
| I2 | Each fitted point is rounded to 1/64 pixel, half away from zero, as `hint::fixed` rounds | Engine |

#### Feeding the fitter

**CFF** (FT-04c.5). `read_private` keeps every operator of Table 23 with the
defaults the table gives, per Font DICT for a CID-keyed font: `BlueValues` to
`FamilyOtherBlues` decoded from their deltas, `BlueScale`, `BlueShift`,
`BlueFuzz`, `StdHW`, `StdVW`, `StemSnapH`, `StemSnapV`, `ForceBold`,
`LanguageGroup` and `ExpansionFactor`. The Type 2 interpreter records each
stem where it now counts and discards them (`cff.rs`), with the count
unchanged so that it keeps agreeing with `cff_subset.rs`'s; each `hintmask`
starts a region, each `cntrmask` adds a group, and each emitted point carries
its region. `Cff::hinted_outline` interprets the glyph at the request's size,
so flex is decided there (F1, F3), and returns the fitted outline.

**Type 1** (FT-04c.6). The Private dictionary's keys are read with the same
defaults. `hstem` and `vstem` record stems relative to the side bearing point,
`hstem3` and `vstem3` record three stems and a group, othersubr 3 starts a
region with the stems its subroutine declares, othersubrs 12 and 13 pass
their groups, flex keeps its control parameter and is decided at the size
(F2, F3), and `dotsection` stays the no-op it is (R5). `Type1::hinted_outline`
returns the fitted outline; `Type1::outline` and `seac` composition keep
their output.

#### Answering the request

Under `Hinting::Font` and `Hinting::FontOrAuto` the facade's `extract_outline`
answers `GlyphSource::sized_outline` for a CFF glyph — bare, `OTTO` or
CID-keyed through its Font DICT — and a Type 1 glyph with the reader's
`hinted_outline` at the request's `ppem`, when FT-04b's conditions hold (*When
a glyph is hinted*): text render mode 0, an axis-aligned unstretched 2×2,
1 to 1 024 pixels an em. A glyph with no stem and no edge hint answers `None`
under `Font` and is FT-04d's under `FontOrAuto`. `GlyphSize::points` asks
nothing of these hints, and `monochrome` changes no fit: `SizedOutline::dropout`
is `None`, so a monochrome glyph keeps today's hardening. The placement on a
whole pixel of the page frame, the cache keys and the darkening after are
FT-04b's and FT-04a's, unchanged.

#### Failure

A fitter that cannot fit a glyph answers `None`, and the glyph draws from its
em outline as it does today, reported as FT-04b's *Failure* reports one.
Stems, groups and regions are bounded — 96 stems (S4), 96 groups, and one
region a `hintmask` or replacement — and a value that is not finite drops
the stem or zone holding it, so no hint set loops or allocates past the
outline it moves (ruling 1).

### Milestones

1. **FT-04c.2, the fitter.** `stem_hints` with S1 to S12, R3, R4 and I1, I2.
   Exit, `stem_hints/tests.rs`:
   `a_stems_edges_land_on_whole_pixels_at_its_rounded_width` (8 to 48 pixels
   an em, the one-pixel floor included),
   `a_width_near_a_standard_width_snaps_to_it` (inside and just outside S6's
   half pixel), `an_edge_hint_moves_only_its_edge`,
   `points_between_fitted_edges_are_interpolated`,
   `each_mask_region_is_fitted_with_its_own_stems`; every expected position
   computed in the test from this table.
2. **FT-04c.3, zones.** Z1 to Z9. Exit, `stem_hints/tests.rs`:
   `an_edge_in_a_zone_aligns_to_its_flat_edge`,
   `blue_fuzz_widens_a_zone_and_no_further`,
   `overshoot_is_suppressed_below_the_blue_scale_size_and_kept_above_it`,
   `blue_shift_decides_an_overshoot_above_that_size`,
   `a_family_zone_close_to_the_fonts_replaces_it`,
   `an_edge_in_no_zone_is_fitted_as_a_stem_edge`; each threshold size and
   aligned position computed in the test from Z5 to Z9.
3. **FT-04c.4, counters.** C1 to C7. Exit, `stem_hints/tests.rs`:
   `a_groups_counters_come_out_as_the_rule_states` (three vertical stems
   whose counters round apart, 8 to 24 pixels an em),
   `a_stem3_group_keeps_its_counters_equal`,
   `a_stem_in_no_group_is_fitted_alone`, and C1 and C3's three cases:
   `language_group_0_fits_only_stem3_groups`,
   `language_group_1_fits_every_group_within_its_expansion_factor`,
   `an_unrecognised_language_group_is_read_as_0`; each counter width computed
   in the test.
4. **FT-04c.5, the CFF reader.** Exit, `cff.rs`:
   `the_private_dict_hint_operators_are_read_with_their_defaults` (each
   operator, the delta arrays, a CID-keyed font's two Font DICTs),
   `stem_operators_record_absolute_edges`,
   `a_hintmask_starts_a_region_with_the_stems_its_bits_name`,
   `cntrmask_bits_make_counter_groups`,
   `a_flex_shallower_than_its_depth_is_a_line_at_that_size`,
   `a_hinted_cff_glyph_has_its_stems_and_zones_on_the_grid` (8 to 24 pixels
   an em); `cff.rs`'s existing tests and `cff_subset.rs`'s `hintmask` tests
   unchanged.
5. **FT-04c.6, the Type 1 reader.** Exit, `type1.rs`:
   `the_private_hint_keys_are_read`, `stems_are_relative_to_the_side_bearing`,
   `hint_replacement_starts_a_region_with_the_new_stems`,
   `a_stem3_hint_makes_a_counter_group`,
   `counter_control_othersubrs_make_groups` (the supplement's 2.6 example,
   its two calls),
   `a_flex_below_its_height_is_a_line_at_that_size`,
   `a_hinted_type1_glyph_has_its_stems_and_zones_on_the_grid`;
   `Type1::outline` and `seac` unchanged.
6. **FT-04c.7, the facade.** Exit, `cff_type1_hinting.rs`:
   `a_hinted_cff_stem_covers_whole_pixel_columns`,
   `a_hinted_type1_stem_covers_whole_pixel_columns` (9 to 16 pixels from an
   integral origin), `the_option_off_draws_what_it_drew_before`;
   `render_regions.rs`'s `hinted_cff_and_type1_text_tiles_byte_equal` at 0.5×
   to 4×; the default and `bundled-fonts` builds.
7. **FT-04c.8, every target.** Exit: `determinism.rs`'s
   `postscript_hinting_is_stable_across_targets`, a page of CFF and Type 1
   lines at 9 to 16 pixels with an ink floor, on Linux, Windows and
   `wasm32-wasip1`.
8. **FT-04c.9a, the speed row** (FT-04c.9b its figure, in the timing phase).
   Exit: `xtask/src/bench.rs`'s `the_committed_baseline_agrees_with_the_tree`
   with the operation that hints every outline of each committed CFF and
   Type 1 program at 12 pixels an em, beside its unhinted twin.

### Risks

| Risk | Mitigation |
| --- | --- |
| A rule of this section's is read as the specifications' | The table marks every rule Stated or Engine; a test of an Engine rule names this document, and the row's exit holds only the Stated ones as the specifications' |
| A real stem 20 or 21 units high at an alignment zone is taken for a Type 1 ghost (S3) and has one edge fitted | Its captured edge aligns as a ghost's would and its other edge is interpolated; Type 2 fonts state edge hints by their width and never meet the rule |
| A font breaking 5.6's restriction suppresses an overshoot of a pixel or more | Z6 ends suppression before an overshoot reaches a pixel, whatever `BlueScale` says |
| Snapping (S6) makes a stem drawn deliberately heavier draw at the standard width | Only within half a pixel, so the snap moves a stem at most to the next whole pixel, and only at sizes where half a pixel is a small part of the stem |
| Counter control widens a glyph past its advance (C7) | `ExpansionFactor` bounds it under `LanguageGroup` 1; under 0 only a stem-three group, by one pixel; the advance itself never moves (PDF places glyphs by `/Widths`) |
| An italic glyph's vertical stems are fitted as upright ranges | They are the ranges the font declares; italic fonts declare few, and `StemSnapV` "should be empty" for them (T1 5.9), so S6 snaps nothing |
| Flex decided at the size makes a glyph's outline differ between sizes | That is flex's purpose (T1 8.3); `Cff::outline` and `Type1::outline`, the unhinted reads, keep drawing the curves |
| A tile and the page fit a glyph differently | The fit is in glyph space at the 2×2's size, and FT-04b's placement puts the origin on the page frame's grid; `hinted_cff_and_type1_text_tiles_byte_equal` (ruling 5) |

## An autohinter

*For ROADMAP row FT-04d.* A face that carries no hints — a TrueType face
built with no programs, a CFF or Type 1 font whose stems were stripped — draws
its small text from the outline alone. The autohinter measures such a face
once — where its baseline, x-height and capitals stand, how wide its stems
are — from the face's own outlines, finds each glyph's stems and edges, and
fits them to the pixel grid as FT-04c's fitter fits declared ones. No
specification states an autohinter, so every rule and threshold below is this
engine's choice; the row's two measurable properties — stems on the pixel
grid, the x-height on a pixel boundary — are asserted in tests, and whether
the page looks better is H-65's judgement.

### Scope

- **Which glyphs are unhinted** in each format, and so the autohinter's.
- **Face analysis** (FT-04d.2): the writing systems, their reference
  characters, and the zones and standard stem widths measured from them.
- **Glyph analysis** (FT-04d.3): segments, the stems they pair into, and the
  zones their edges fall in.
- **Fitting** across the baseline (FT-04d.4) and along it (FT-04d.5): the
  x-height on a whole pixel, the zones, the stems, the points between, and the
  advance.
- **Answering the request** (FT-04d.6) under `Hinting::FontOrAuto`, and the
  proof: fuzzing, the fingerprint and the speed row (FT-04d.6 to FT-04d.8a).

### Non-goals

- **Hints a face carries**: TrueType programs (FT-04b) and CFF and Type 1
  stems (FT-04c). A face that carries hints is never autohinted, whether or not
  its hints ran at a size.
- **Stem darkening** (FT-04a), which thickens what the autohinter gives it.
- **Scripts beyond the three writing systems below.** A glyph of no listed
  writing system has its stems fitted and meets no zone.
- **Hinting by another engine's rules.** The rules are designed from the faces'
  own outlines and the measurable properties the row names, and are held to
  this document, never to another program's output (ruling 13).
- **The picture.** H-65's.

### Design

#### Which glyphs are unhinted

| Format | Unhinted, and so autohinted under `FontOrAuto` | Not autohinted |
| --- | --- | --- |
| TrueType (`glyf`) | A glyph with no instructions — a simple glyph whose `instructionLength` is 0, a composite without `WE_HAVE_INSTRUCTIONS` whose components have none — in a face with neither `fpgm` nor `prep` | Every glyph of a face carrying `fpgm` or `prep`, with or without a program of its own: the face hints (FT-04b), and a size or a program that fails draws unhinted (FT-04b's *Failure*) |
| CFF, CFF2, Type 1 | A glyph that declares no stem and no edge hint | A glyph that declares any: FT-04c fits it, or draws it unhinted where its fit fails or its format is one FT-04c does not fit (CFF2) |
| Type 3, bitmap glyphs | — | Never: a procedure or a bitmap has no outline to fit |

The test is the glyph's and its face's, never the size's: `gasp` decides
nothing here (*What FT-04c and FT-04d share*).

#### Face analysis

**Writing systems.** A glyph belongs to the writing system of the character
the face maps to it: through the face's `cmap` for a TrueType face, and
through its glyph names read as characters (`glyph_name_to_char`) for a CFF or
Type 1 face; where several characters reach one glyph, the lowest. Three are
measured, by Unicode block:

| System | Characters | Zones from (flat; round) | Vertical stem from | Horizontal stem from |
| --- | --- | --- | --- | --- |
| Latin | U+0000–U+024F, U+1E00–U+1EFF | baseline: bottoms of `x` `z` `H` `E`; `o` `O`. x-height: tops of `x` `z`; `o`. Cap height: tops of `H` `E` `Z`; `O`. Ascender: tops of `b` `d` `h` `k` `l`. Descender: bottoms of `p` `q` | `l`, else `I` | `z`, else `Z` |
| Greek | U+0370–U+03FF, U+1F00–U+1FFF | baseline: bottoms of `κ` `Η` `Ε`; `ο` `Ο`. x-height: tops of `κ` `π`; `ο`. Cap height: tops of `Η` `Ε` `Ζ`; `Ο`. Descender: bottoms of `ρ` `φ` | `ι`, else `Ι` | `π`, else `Ε` |
| Cyrillic | U+0400–U+052F | baseline: bottoms of `х` `н` `Н` `Е`; `о` `О`. x-height: tops of `х` `н`; `о`. Cap height: tops of `Н` `Е`; `О`. Descender: bottom of `р` | `н`, else `Н` | `т`, else `Т` |

A glyph no listed system's character reaches — another script, or a face
whose glyphs no `cmap` or name reaches — has no writing system.

**Zones.** A zone's flat position is the median of its flat characters'
extremes (the lower of the two middle values where they are even in number),
each the top (`yMax`) or bottom (`yMin`) of the character's outline in font
units; its overshoot is the median of its round characters' extremes where
any is in the face and lies outside the flat position (above it for a top
zone, below for a bottom one), and the zone is flat otherwise. A zone none of
whose flat characters the face has, or has with an empty outline, is not
measured, and a writing system with no x-height zone has none of its zones
fitted.

**Standard stems.** A writing system's standard vertical stem is the
narrowest vertical stem glyph analysis finds in its vertical stem character,
and its standard horizontal stem the narrowest horizontal stem in its
horizontal stem character; one the face lacks is not measured.

The analysis reads a few dozen outlines, once a face, and is kept beside the
face's outline cache for the resource scope (FT-04d.6).

#### Glyph analysis

All in font units, on the glyph's outline as the format states it — a
composite glyph's assembled from its components.

- **The polygon.** Each contour is read as the polygon through its points in
  order, on and off the curve: a well-made outline puts points at its
  extremes with their handles level, so a round's top is a level edge of the
  polygon.
- **Directions.** An edge of the polygon is horizontal where `14·|dy| ≤ |dx|`
  and vertical where `14·|dx| ≤ |dy|` — within about four degrees, decided by
  multiplication and no angle (ruling 4) — and neither otherwise.
- **Segments.** A maximal run of consecutive horizontal edges travelling one
  way (`+x` or `−x`), or of vertical edges travelling one way, is a segment
  where its extent along its direction is at least `upem / 50`. Its position
  is the length-weighted mean of its edges' midpoints across its direction.
  A segment holding an edge to or from an off-curve point is **round**. At
  most 256 segments a glyph; the rest are passed over (ruling 1).
- **Which side the ink is on.** The glyph's total signed area gives its
  winding: positive, ink lies left of travel. A horizontal segment with ink
  above it is a bottom edge and one with ink below it a top edge; a vertical
  segment with ink to its right is a left edge, to its left a right edge.
- **Stems.** A bottom edge and the top edge above it — or a left edge and the
  right edge right of it — pair into a stem where their extents overlap by at
  least half the shorter's, the distance between them is above zero and at
  most `upem / 4`, and each is the other's nearest such partner. An edge in
  no pair stays a single edge.
- **Zones.** A horizontal edge is linked to a zone of its glyph's writing
  system — a top edge to a top zone, a bottom edge to a bottom zone — where
  it lies within the zone or within `upem / 100` of it.

#### Fitting across the baseline

At a size of `s` pixels per font unit (`GlyphSize::ppem / 64 / upem`):

1. **The x-height on a whole pixel.** Where the glyph has a writing system
   with an x-height zone of flat position `xh`, the vertical scale is
   `s_y = round(xh · s) / xh`, so the x-height lands on a pixel boundary; it
   moves by at most half a pixel. Where `xh · s` rounds to 0, and for a
   glyph with no x-height, `s_y = s`.
2. **The zones.** Each zone's flat edge is placed at `round(flat · s_y)` —
   the baseline, at 0, stays at 0 — and its overshoot kept, as
   `max(1, round(d · s_y))` pixels beyond it for `d` the overshoot in font
   units, where `d · s_y` is at least half a pixel, and suppressed otherwise.
3. **Edges in zones** go to their zone's flat edge, or, for a round edge of a
   zone whose overshoot is kept, to its overshoot.
4. **Horizontal stems** are FT-04c's: a width snapped to the writing system's
   standard horizontal stem where within half a pixel of it (S6), rounded and
   at least one pixel (S7), placed by its zone-linked edge where it has one
   (S10) and otherwise by its centre (S9), the edges kept in order (S12).
5. **Points** move by FT-04c's interpolation between the fitted edges (I1,
   I2); a single edge in no zone is no fitted edge.

#### Fitting along the baseline

No zone runs across. **Vertical stems** take a width snapped to the
standard vertical stem within half a pixel, rounded and at least one pixel,
placed by their centre, in order (S6, S7, S9, S12), at `s`. **The advance is
not the autohinter's:** a PDF places each glyph by `/Widths`, so no advance is
read or moved, and the origin stays at 0; points left of the first vertical
edge and right of the last move as those edges move (I1), so a side bearing
changes by under a pixel. A glyph with no vertical stem keeps its horizontal
positions, scaled and unmoved.

#### Answering the request

Under `Hinting::FontOrAuto` — never under `Font` — the facade's
`extract_outline` answers `GlyphSource::sized_outline` for a glyph the table
above calls unhinted with the autohinter's outline at the request's `ppem`,
when FT-04b's conditions hold. The face is analysed once a resource scope and
the analysis kept beside `Caches::outlines`; a glyph is fitted once a size, as
FT-04b's answers are kept. `SizedOutline::dropout` is `None`, the placement on
a whole pixel of the page frame and the cache keys are FT-04b's, and a fit
that cannot be made — no segment, `s` not a finite number above zero — draws
the glyph unhinted.

### Milestones

1. **FT-04d.2, face analysis.** Exit, `autohint/tests.rs`, over faces built in
   the test with stated metrics: `the_x_height_is_measured_from_the_faces_own_x`,
   `each_zone_comes_from_its_reference_glyphs_with_its_overshoot`,
   `the_standard_stems_are_the_reference_glyphs_stems`,
   `a_face_missing_a_reference_character_gets_no_zone_from_it`, and one test a
   writing system: `latin_zones_and_stems_are_measured`,
   `greek_zones_and_stems_are_measured`,
   `cyrillic_zones_and_stems_are_measured`.
2. **FT-04d.3, glyph analysis.** Exit: `segments_are_found_where_the_test_drew_them`
   (quadratic and cubic contours, both windings),
   `overlapping_opposite_segments_pair_into_stems`,
   `an_edge_inside_a_zone_is_linked_to_it`,
   `a_diagonal_or_a_round_makes_no_stem`, `a_hostile_contour_is_bounded` (the
   256-segment cap).
3. **FT-04d.4, across the baseline.** Exit:
   `the_x_height_lands_on_a_pixel_boundary_at_every_size` (8 to 16 pixels an
   em and beyond, faces of 1 000 and 2 048 units an em) — **the row's second
   property** — `horizontal_stems_land_on_the_grid_with_rounded_widths`,
   `the_baseline_stays_on_the_grid`,
   `untouched_points_are_interpolated_between_fitted_edges`; each position
   computed in the test from the face's measured values. It reuses FT-04c.2's
   interpolation, and so follows it.
4. **FT-04d.5, along the baseline.** Exit:
   `vertical_stems_land_on_the_grid_with_rounded_widths` (8 to 16 pixels an em
   and beyond) — **the row's first property**, with FT-04d.4's horizontal
   stems — `the_advance_is_treated_as_the_design_states` (no point right of
   the last vertical edge moves by more than that edge, the origin unmoved),
   `a_glyph_with_no_vertical_stem_keeps_its_horizontal_positions`.
5. **FT-04d.6, the facade.** Exit, `autohinting.rs`, over a face built in the
   test with no glyph programs: `an_unhinted_faces_stems_cover_whole_pixels`,
   `its_x_height_ends_on_a_pixel_row` (8 to 16 pixels from an integral
   origin), `a_hinted_face_is_not_autohinted`,
   `the_face_is_analysed_once_per_page` (a `cfg(test)` count over 500
   glyphs), `the_option_off_draws_what_it_drew_before`; `render_regions.rs`'s
   `autohinted_text_tiles_byte_equal`; the `truetype`, `cff` and `type1` fuzz
   targets autohint at three sizes.
6. **FT-04d.7, every target.** Exit: `determinism.rs`'s
   `autohinted_text_is_stable_across_targets`, an unhinted face's text at 8 to
   16 pixels with an ink floor, on Linux, Windows and `wasm32-wasip1`.
7. **FT-04d.8a, the speed row** (FT-04d.8b its figure, in the timing phase).
   Exit: `xtask/src/bench.rs`'s `the_committed_baseline_agrees_with_the_tree`
   with the operation that autohints every outline of each committed TrueType
   face at 12 pixels an em, the face analysis inside the timed work, beside
   its unhinted twin.

H-65 judges the picture the examples draw; nothing above asserts one.

### Risks

| Risk | Mitigation |
| --- | --- |
| Features between zones — the bar of `e`, the crossbar of `f` — round to the grid on their own and can crowd the x-height | They are fitted as stems, a whole number of pixels wide, and held in order with the zone edges (S12); the picture is H-65's |
| A script with no x-height — Hebrew, Arabic, the CJK ideographs — gets no zone | Named: a glyph of no listed writing system has its stems fitted and its vertical scale unchanged, and a later writing system is a row of its own |
| An accent placed by a composite lands between pixels or collides with its base | The assembled outline is fitted as one glyph, so the accent's own edges are found and fitted with the base's; one whose edges the analysis does not find moves by interpolation, never off the outline |
| An edge fitted in glyph space lands on no pixel of the page | FT-04b's placement puts the glyph's origin on a whole pixel of the page frame (ruling 5), so a whole pixel about the origin is a whole pixel of the page; `autohinted_text_tiles_byte_equal` |
| Rescaling the vertical axis to the x-height changes a glyph's proportions | By at most half a pixel at the x-height; the horizontal scale and every advance are unchanged |
| A decorative or unusual face's reference glyphs give odd zones | The median of several characters rather than one; a zone whose characters the face lacks is not measured |
| A face's subset or symbolic `cmap` reaches no reference character | No zone is measured and stems are fitted alone; nothing is guessed |
| A round extreme or a serif is taken for a stem | The pairing asks for overlap, mutual nearness and at most a quarter of an em; a misread stem is still placed on the grid at its rounded width, never moved off the outline |
| Analysis cost on a page of many faces | Once a face a resource scope, a few dozen outlines; FT-04d.8a measures it |
