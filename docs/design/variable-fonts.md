# Variable fonts

A variable face carries one design and the arithmetic that moves it: `fvar`
names the axes, `avar` bends their scales, and the delta stores — `gvar` and
`cvar` for a `glyf` face, `HVAR`, `VVAR` and `MVAR` for its metrics, and the
`blend` operands of a `CFF2` table — say how far each value moves at each
position. This document settles how the engine reads the design space,
evaluates both delta stores, interprets `CFF2`, writes a static instance of
either flavour, and who asks for one — with one property the rest rests on:
**every value at an instance is computed by one integer model, transcribed
from the OpenType specification's own rounding rules, which the float-free
shaper can call as it stands.**

Every rule below is transcribed from OpenType 1.9.1 (published by Microsoft),
cited by chapter and section, and from W3C's CSS Fonts Module Level 4 for the
books.

## Scope

- **The design space**: `fvar`'s axes and named instances, user coordinates
  normalised against each axis's minimum, default and maximum and then
  through `avar`, with the precision and rounding the overview chapter
  requires.
- **The tuple variation store** `gvar` and `cvar` share: shared and embedded
  peak tuples, intermediate regions, private and shared point numbers, packed
  deltas, and each tuple's scalar; `cvar` giving the `cvt ` values at an
  instance; `gvar` applied to `glyf` outlines, untouched points inferred,
  composite offsets and phantom points moved.
- **The item variation store** and its delta-set index maps, and its clients:
  `HVAR`, `VVAR`, `MVAR` and `CFF2`'s blends.
- **CFF2**: its header, DICTs, INDEXes, Font DICTs, FDSelect in all three
  formats and its charstrings, through the one Type 2 interpreter
  parameterised by flavour; `blend` and `vsindex` in charstrings and Private
  DICTs.
- **The static instancer** for both flavours: a face at an instance written as
  a face with no variation tables.
- **Who asks for an instance**: the document builder, and EPUB and HTML
  through css-fonts-4's font variation resolution.
- **The limits** a hostile face meets, each refused by name.

## Non-goals

- **`FeatureVariations` and GSUB/GPOS variation indices.** Substituting
  features by region and moving GPOS values, anchors and ligature carets by
  `GDEF`'s item variation store are FT-07a's, which depends on this row and
  reuses its numeric model and its item variation store as they stand.
- **Multiple-master Type 1** (FT-10). Its blend design is not OpenType's;
  [type42-and-multiple-master](type42-and-multiple-master.md) is its design.
- **An instance chosen for a host-supplied face from its request's weight.**
  A substituted face drawn bolder is FT-14's weight emulation.
- **`STAT`.** It names styles for a menu; nothing drawn or written reads it.
- **`avar` beyond version 1.0**, which is the only version the 1.9.1 chapter
  defines; another major version is refused by name.
- **Drawing a PDF's embedded variable face at another instance.** A PDF names
  no instance, so an embedded variable face is drawn at its default, and an
  instance reaches a page only as a static face this engine or another
  producer wrote.
- **Hinting a variable face at an instance.** No page draws one: a PDF names
  no instance, and an instance reaches a page as a static face. So the
  bytecode interpreter (`docs/design/hinting.md`) hints a static face, or a
  variable face at its default, and its `GETINFO` answers that variations are
  not supported; this design's part of hinting is the static instance's
  varied `cvt ` and its `GETVARIATION` IDEF, for whichever reader hints it.
- **Variations inside tables this design does not name**: `COLR`'s are
  FT-06.20's; `BASE` and `JSTF` keep their default values in a static
  instance.

## Design

### The numeric model

The overview chapter's 'Coordinate Scales and Normalization' fixes how a
coordinate is computed and stored, and its 'Algorithm for Interpolation of
Instance Values' leaves the representation of scalars and sums to the
implementation, asking for at least 16 fractional bits and rounding only
where a value is used. The model below fills that freedom one way, in
integers, so that the font crate and the float-free shaper compute the same
number on every target (ruling 4). The variation module carries
`#![deny(clippy::float_arithmetic)]`, as `tinker-pdf-shape` does; the only
floats in it are the two conversions at its edge, allowed by name.

1. **A user coordinate is a 16.16 `Fixed`** (overview, step 1 of the
   precision list). A caller's `f64` reaches it by the chapter's conversion:
   the fractional part times 65 536 rounded to the nearest integer, a half
   rounding up, in the low word, and the two's complement of the integer part
   in the high word — which is `floor(x · 65536 + 0.5)`, saturated to the
   32-bit range. A NaN names no position and leaves its axis at the default.
2. **Clamp** to the axis's minimum and maximum (overview, step 2 of the
   normalisation; `fvar`, 'Variation Instance Selection').
3. **Default normalisation in 16.16** (overview, steps 2 and 3 of the
   precision list): `(user − default) / (default − min)` below the default,
   `(user − default) / (max − default)` above it, zero at it. A 16.16 quotient
   is the exact quotient rounded to the nearest 1/65 536, **a half rounding
   toward +∞** — the one rounding the chapter states, its conversion's,
   applied to every 16.16 operation. The result is clamped to [−1, +1].
4. **`avar` in 16.16** (overview, step 4; `avar`, 'Table formats'). The first
   kept map whose `fromCoordinate` is at least the value is `endSeg`; equal,
   its `toCoordinate` is the answer; greater, the map before it is `startSeg`,
   `ratio = (v − start.from) / (end.from − start.from)` rounded as in 3, and
   the answer `start.to + ratio × (end.to − start.to)`, the product rounded
   the same way. The maps' F2DOT14 values enter 16.16 times four, exactly.
   The result is clamped to [−1, +1].
5. **To 2.14** (overview, step 5): add 2 and shift right by 2, sign
   extending. This is the coordinate the specification says must be stored
   and returned — the one `GETVARIATION` pushes and a `FeatureVariations`
   condition compares — and it is what a `Location` holds: one `i16` per
   axis, in `fvar`'s order.
6. **Region scalars, in 16.16.** A per-axis scalar is the overview's
   pseudo-code read on the 2.14 coordinates: 1 where the region is invalid
   for the axis or its peak is zero, 0 outside `[start, end]`, 1 at the peak,
   and otherwise `(c − start) / (peak − start)` or `(end − c) / (end − peak)`,
   the exact ratio rounded as in 3. The region's scalar starts at 1 and is
   multiplied by each axis's in `fvar`'s order, each product rounded as in 3.
   A scalar is a value in [0, 1] with 16 fractional bits.
7. **Scaled deltas.** A delta that is an integer — every `cvar`, `HVAR`,
   `VVAR` and `MVAR` delta, and every delta `gvar` states — times a scalar is
   an exact 16.16 value. A delta with a fraction — a `gvar` delta inferred for
   an untouched point — times a scalar is rounded to 16.16 as in 3.
8. **Accumulation.** The net adjustment and the default it is added to are
   summed exactly in a 64-bit 16.16 value, saturating where the sum would
   leave it, which the overview permits ("saturation arithmetic … may be
   used") and which no font reaches: a 16-bit default plus 4 095 tuples of
   16-bit deltas needs 44 bits.
9. **Use.** An outline reads the 16.16 value as an `f64`, which holds it
   exactly. A static instance and the shaper round it to whole font units, a
   half rounding toward +∞ — `floor(v + ½)`.

**CFF2 is carried in `f64`** because the Type 2 machine's operand stack is,
as `cff.rs`'s is for CFF. Every CFF2 charstring operand is a 16.16 number, the
encodings giving nothing else and CFF2 having no arithmetic operators. A
`blend` adds to each default its deltas times their regions' scalars — the
model's 16.16 scalars, exact as `f64` — in region order, and rounds the
result to 16.16 as in 3, so a blended operand is again a number a Type 2
charstring can carry and a static instance writes it exactly. For integer
deltas the sum is exact before that rounding and the result is the model's.
A DICT `blend` is the same over the DICT's operands.

### The design space (`variation.rs`, FT-05.2)

`DesignSpace::read(&Sfnt)` reads `fvar` and `avar`. No `fvar`, or an `fvar`
whose `axisCount` is zero, gives a space with no axes: every coordinate asked
of it is the default, and every variation table is ignored, as the `fvar`
chapter says of a zero count.

- **Axes**: tag, minimum, default and maximum as 16.16, flags (`HIDDEN_AXIS`)
  and name ID. Records are read `axisSize` apart, so a larger size is a
  compatible extension (`fvar`, 'Table Formats'); a size under 20, an array
  past the table's end, more than `MAX_AXES` (64) axes, or an axis whose
  minimum exceeds its default or whose default exceeds its maximum is refused
  by name. A refused design space draws the default.
- **Named instances**: subfamily name ID, flags, user coordinates, and the
  PostScript name ID where `instanceSize` holds it (`axisCount × 4 + 6` or
  more); a size under `axisCount × 4 + 4` or records past the end are refused.
- **`avar`**: one segment map per axis, version 1, `axisCount` equal to
  `fvar`'s, every map inside the table, or the space is refused by name. A
  map whose `fromCoordinate` does not exceed the last kept one's, or whose
  `toCoordinate` falls below it, is ignored, as the chapter allows; an axis
  whose kept maps lack any of −1→−1, 0→0 and 1→1 is not modified.
- **`Instance`**, the one argument every caller names an instance with:
  `Named(index)` into `fvar`'s instances, or `User(coordinates)`, each a tag
  and an `f64` user value. Tags the face lacks are ignored, a tag given twice
  takes its last value, and an axis given none stays at its default
  (`fvar`, 'Variation Instance Selection'). `DesignSpace::location` turns an
  `Instance` into a `Location`; a named index past the instances is refused.
  `Instance::default()` is the default instance.

### Region scalars

`RegionAxis { start, peak, end }` in 2.14 and the per-axis and overall
scalar functions of the model's step 6 are the module's own, shared by both
stores and by the shaper. A tuple that is not intermediate has its start and
end implied: from zero to the peak, on the peak's side (overview,
'Algorithm for Interpolation of Instance Values'). A region read against a
`Location` with fewer coordinates than it has axes reads the missing ones at
zero, the default.

Two consequences are followed rather than smoothed over. A region whose
start, peak and end are out of order on an axis, or whose start and end
cross zero around a non-zero peak, ignores that axis — scalar 1, as the
pseudo-code says, not 0. And a region whose peak is zero on every axis has
scalar 1 everywhere, the default instance included: a face carrying one
moves its default by those deltas, which contradicts `fvar`'s definition of
the default instance, and the engine computes what the algorithm computes.

### The tuple variation store (`variation/tuple.rs`, FT-05.3)

`TupleStore::parse` reads the header the common-formats chapter gives — a
`GlyphVariationData` header, or `cvar`'s after its version — and yields each
tuple: its peak, embedded or from `gvar`'s shared tuples; its intermediate
start and end; its point numbers, private or shared; and its packed deltas
(common formats, 'Tuple Variation Store Header', 'TupleVariationHeader',
'Serialized Data').

- **Point numbers** (common formats, 'Packed "Point" Numbers'): a count of
  one byte, or two with the high bit set; zero means every point — every
  `cvt ` entry, or a glyph's points and its four phantom points. Runs of
  bytes or words, each a difference from the number before it, the first from
  zero. A number past 65 535, or runs reading more numbers than the count or
  past the tuple's data, are refused by name.
- **Deltas** (common formats, 'Packed Deltas'): runs of zeros, bytes or
  words, one logical stream of the tuple's delta count — the point count for
  `cvar`, twice it for `gvar`, its X deltas then its Y deltas — a run allowed
  to cross the X/Y boundary. A run reaching past the count or past the
  tuple's `variationDataSize` is refused.
- **Bounds**: 4 095 tuples, by the format's twelve bits; `MAX_AXES` axes a
  tuple record; a delta count of at most twice 65 539. A shared tuple index
  past `sharedTupleCount` — every index, in `cvar`, which has none and
  requires embedded peaks — is refused.

**`cvar`** (`variation/cvar.rs`): the `cvt ` values at a `Location` are each
FWORD plus, for every tuple, its scalar times its delta for that index, in
16.16 (the model's 7 and 8). An index past the `cvt ` table's
`length / 2` entries moves nothing (`cvar` chapter, last paragraph). The
varied `cvt ` reaches a page only through a static instance, whose hinting is
FT-04b's.

**`gvar`** (`variation/gvar.rs`, FT-05.4): `glyf::outline_at` applies a
glyph's tuples to its **raw points** — the form FT-04b's interpreter works on
too, unscaled: a simple glyph's contour end indices, its flags with `REPEAT`
expanded, its absolute coordinates in font units as `i32`, and the four
phantom points appended where `docs/design/hinting.md`'s 'Zones, points and
the graphics state' places them, in the order the 'Phantom points' section
of *Instructing TrueType Glyphs* numbers them — the horizontal origin
`(xMin − lsb, 0)`, the advance width past it on the same line, the vertical
origin at `(0, yMax + tsb)` and the advance height below it, the vertical
metrics `vmtx`'s or, where the face has none, `hhea`'s ascender and
descender as that design chooses; `xMin` and `yMax` are the glyph header's,
zero for a glyph with no outline, whose phantom points are all it has. The
chapter gives the horizontal pair `y = 0` (its Skia example) and the
vertical pair no other coordinate, so they stand at `x = 0`. The varied
points are 16.16 values in font units, and the implied midpoints are
resolved after the deltas are added, through the one contour reading, so
`glyf::outline` keeps its output for every face. Untouched points take the
deltas the `gvar` chapter's 'Inferred deltas for un-referenced point numbers'
computes, contour by contour, from the default positions and the unscaled
deltas: the same delta where the two neighbours stand at one coordinate and
agree, none where they disagree, the nearer one's outside them, and between
them `ref.delta + proportion × (comp.delta − ref.delta)`, whose exact value
— the same whichever neighbour is the reference — is rounded to 16.16 once
and then scaled as the model's 7 says. A point number listed twice takes
both deltas; a phantom point is in no contour and is moved only where
listed. A composite's point numbers are its components and phantom points
('Point numbers and processing for composite glyphs'), nothing inferred,
the horizontal phantom points taking their X deltas and the vertical ones
their Y: a component with `ARGS_ARE_XY_VALUES` moves by its delta, its
offset as written plus the delta placed as the offset is — this reader's
composite does not scale an offset by its component's transform, so neither
is the delta; one placed by point numbers ignores its delta;
`USE_MY_METRICS` takes the component's phantom points, the last flagged
component's, as that component computes them. The phantom points give the
advance and side bearings where `HVAR` is absent. Every table is decoded at
every location, as `cvar`'s are, and an outline is charged a table times its
points for each glyph it draws, refused by name past `MAX_GLYPH_DELTA_WORK`
(2²²): a few bytes of point numbers can otherwise ask for a contour of
sixty-five thousand points to be inferred four thousand times.

### The item variation store (`variation/item_store.rs`, FT-05.5)

`ItemVariationStore::parse` reads format 1: the region list (`axisCount`,
`regionCount` under 32 768, each region an axis's start, peak and end), and
the item variation data subtables, a NULL offset meaning no variation for
that outer index (common formats, 'Item Variation Store Header and Item
Variation Data Subtables'). `region_scalars(&Location)` evaluates every region
once — the cache the chapter's last paragraph suggests — and `delta(index,
scalars)` sums one row: its deltas, `wordDeltaCount` of them long (16-bit, or
32-bit under `LONG_WORDS`) and the rest short (8-bit, or 16-bit), each times
the scalar of the region its column names. The index 0xFFFF/0xFFFF is no
variation. An outer index past the subtables, an inner one past `itemCount`,
a region index past the list, a `wordDeltaCount` above `regionIndexCount` or
rows past the subtable are refused by name; a region list whose `axisCount`
differs from `fvar`'s is refused where a client knows `fvar`'s.

**`DeltaSetIndexMap`** (common formats, 'Associating Target Items to Variation
Data'): formats 0 and 1, entries of one to four bytes split by
`INNER_INDEX_BIT_COUNT_MASK`, an item at or past `mapCount` taking the last
entry. A map with no entries, or entries past the table, is refused. `HVAR`
and `VVAR` use format 0 only, as their chapters state; a format 1 map there is
refused.

**The metric clients** (`variation/metrics.rs`), each at a `Location`:

- **`HVAR`**: the advance delta — through its map, or the glyph id as the
  inner index with outer index zero where there is none (`HVAR`,
  'Processing') — and the left and right side-bearing deltas through their
  maps, absent where the table has none. `Sfnt` gains `hmtx`'s left side
  bearing beside its advance.
- **`VVAR`**: the advance-height delta (implicit mapping likewise), the top
  and bottom side-bearing deltas, and the vertical origin's, applied to
  `VORG`'s `vertOriginY`. `Sfnt` gains `vhea` and `vmtx`'s advance height and
  top side bearing.
- **`MVAR`**: the value record of a tag, read `valueRecordSize` apart, its
  delta added to the field the tag names in `OS/2`, `hhea`, `vhea`, `post` or
  `gasp` ('Value Tags'). A tag the table does not hold is constant across the
  space; a tag the face's tables cannot answer has no default to vary.

**CFF2's store** is the same structure behind a 16-bit length (CFF2,
'VariationStore'), of which only each data subtable's region indices are read:
their count is the `k` a `blend` pops, and their regions' scalars the ones it
multiplies by.

### CFF2 (`cff2.rs`, FT-05.6 and FT-05.7)

**Structure** (CFF2 chapter): a five-byte header (version 2), a Top DICT that
is not in an INDEX, then the Global Subr INDEX; INDEXes count in 32 bits. The
Top DICT's `CharStringINDEXOffset` (17), `VariationStoreOffset` (24),
`FontDICTINDEXOffset` (12 36), `FontDICTSelectOffset` (12 37) and
`FontMatrix` (12 7, default 1/1000); each Font DICT's `PrivateDICTOffset`
(18), measured from the table; each Private DICT's `LocalSubrINDEXOffset`
(19), measured from the Private DICT, and its `vsindex` (22, default 0). CFF2
DICTs add `vsindex`, `blend` (23) and `VariationStoreOffset` as operators,
hold 513 operands, and take no 16.16 operand. FDSelect formats 0, 3 and 4; a
table with no FDSelect puts every glyph in Font DICT 0. `cff.rs`'s `Index`
and DICT tokeniser are reused with the count width and the operator range as
parameters, so CFF reads exactly as it does.

**One interpreter.** The Type 2 machine in `cff.rs` runs both flavours, so
FT-04c's hint recording and `cff_subset.rs`'s stem count stay shared. A CFF2
charstring differs from a Type 2 one in five ways (CFF2, 'CharString
operators'): no width, so no operator takes one; no `endchar` and no
`return` — a charstring and a subroutine end where their data ends, and the
two bytes are unrecognised operators, which clear the stack; no `put`,
`get` or arithmetic; a stack of 513 operands; and `vsindex` (15) and `blend`
(16). Stems, `hintmask` and `cntrmask` count exactly as CFF's do. Every
limit a CFF2 glyph passes refuses that glyph by name, where CFF keeps its
silent stops: the stack past 513, a `blend` with fewer operands than
`n + n·k + 1` or a non-integral `n`, subroutines nested past 10, the
64 000-operation budget, `vsindex` after a `blend` or twice, a `vsindex` past
the store's subtables, and `blend` in a table with no store.

**`blend`** pops `n`, then `n·k` deltas and `n` defaults, and pushes the `n`
results (CFF2, 'Variation operators'). `Cff2::outline` reads every `blend`'s
defaults and drops its deltas: the default instance's outline.
`Cff2::outline_at(glyph, &Location)` multiplies them by the scalars of the
data subtable the `vsindex` in force selects — the glyph's own, or its
Private DICT's — computed once per instance by `Cff2::blends_at`, as the
model's CFF2 paragraph states. `Cff2::private_dict` gives a Private DICT's
entries with its blends resolved, at the default or at an instance.

**Bounds**: an INDEX of more than 65 536 entries is refused before it is
read, since no glyph id, Font DICT number or biased subroutine number reaches
past it; each distinct Private DICT is read once, and the distinct ones,
with the Local Subr INDEX each reaches, may not total more bytes than the
table holds, which no font that does not overlap its own structures exceeds
— so the reading's work and memory are linear in the table.

**Glyph selection** (FT-05.13). A CFF2 program carries no charset, encoding
or glyph names. A simple font selects its glyphs through the face's `cmap`, as
9.6.6.4 has a TrueType font do; a composite font reads its CID as the glyph
index, as 9.7.4.2 reads a CFF that is not CID-keyed. The renderer routes an
sfnt with `CFF2` and neither `glyf` nor `CFF ` to this reader, at the default
instance; `selection` keeps agreeing with `extract_outline`; and a CFF2
program met on a rewrite is kept whole with its `SubsetRefusal`, since
`cff_subset.rs` rewrites CFF only.

### The static instancer (`instance.rs`, FT-05.10 and FT-05.11)

`instance::instantiate(program, &Instance)` writes an sfnt with no variation
tables, at the instance's `Location` (the overview's 'Dynamic Generation of
Static Instance Fonts').

| Table | In the static instance |
| --- | --- |
| `fvar`, `avar`, `gvar`, `cvar`, `HVAR`, `VVAR`, `MVAR`, `STAT` | dropped |
| `DSIG` | dropped: it signs bytes that changed |
| `hdmx`, `VDMX` | dropped: variable fonts do not use them, and they describe no instance |
| `glyf`, `loca` | each glyph's varied points rounded to font units (the model's 9), composite offsets varied, each bounding box recomputed from the points the glyph holds, a matched component placed by its points as the `glyf` chapter places it; instructions, on-curve flags, `OVERLAP_SIMPLE` and transforms kept; `loca`'s format chosen by the new size |
| `hmtx`, `vmtx` | the varied advances (`HVAR`/`VVAR`, else the phantom points) rounded; each left side bearing the glyph's new `xMin`, and each top side bearing its vertical origin, the third phantom point, less its new `yMax`; `hhea` and `vhea` the long-metric counts and the extents these give |
| `OS/2`, `hhea`, `vhea`, `post`, `gasp` | each `MVAR` value rounded and held to its field, `gasp`'s last range keeping 0xFFFF; the three fields `MVAR` leaves to the axes ('Value Tags', note): `usWeightClass` the `wght` value rounded into 1 to 1 000, `usWidthClass` the `wdth` value interpolated between the classes of the `OS/2` chapter's table and rounded into 1 to 9, as the `wdth` registration says, and `italicAngle` the `slnt` value, both measured counter-clockwise; and `xAvgCharWidth`, from `OS/2` version 3 on, the instance's mean non-zero advance |
| `cvt ` | the `cvar` values rounded |
| `prep`, `maxp` | where the face carries hinting — `fpgm`, `prep` or a glyph's program — an IDEF for `GETVARIATION` (0x91) appended to `prep` that pushes the instance's normalised coordinates, and `maxp`'s `maxInstructionDefs` and `maxStackElements` raised to cover it, as the *TrueType Instruction Set*'s `GETVARIATION` notes recommend for an instance font |
| `head` | the font bounding box the glyphs' union, `checkSumAdjustment` rewritten |
| `VORG` | the `VVAR` origins rounded |
| `CFF2` | replaced by `CFF ` (FT-05.11, below) |
| everything else — `cmap`, `name`, `fpgm`, GSUB, GPOS, GDEF, `BASE`, `JSTF`, `MATH`, `COLR`, `CPAL`, `kern`, and tables this reader does not know | copied as they are; GSUB, GPOS and GDEF keep their variation data unapplied, which is FT-07a's |

The table directory's checksums are rewritten. The instance at the default
location keeps every default outline and metric: a face with no region whose
peak is zero everywhere instances to its own default values. A face that does
not vary is its own instance and is returned as it is, unless its outlines are
in `CFF2`: that face is written with a `CFF ` table all the same, so the
instance of every face is one a CFF reader reads; a table a reader
refuses refuses the instance by name, since a writer has no default to fall
back to that is not a different face.

**CFF2 to CFF** (FT-05.11). Each glyph's charstring is re-encoded as Type 2:
its blends resolved, its subroutines inlined — a CFF2 subroutine blends under
its caller's `vsindex`, so one subroutine can blend two ways for two glyphs —
its width written first against a `nominalWidthX` of zero where the width is
not zero, `endchar` added, and each operator whose operands pass Type 2's 48
split where its operands repeat, as the overview's paragraph on converting
CFF2 charstrings asks; stems and masks are kept for FT-04c. The machine
records each operator it runs with the operands on its stack, and each is
written with the operands it consumed: a stem operator its pairs, its
continuation's first edge restated from zero because each stem operator's
edges are relative from zero; an implicit `hintmask` stem's overflow as
`vstemhm` before it; an alternating line or curve operator's continuation as
the other operator where the piece starts on the other axis; `hhcurveto`'s and
`vvcurveto`'s odd first operand in the first piece and `hvcurveto`'s fifth in
the last; `rcurveline`'s and `rlinecurve`'s overflow as `rrcurveto` and
`rlineto` before them. Each Private DICT is written with its blended values,
the operators a CFF Private DICT shares with a CFF2 one, and no `Subrs`. The
CFF is **CID-keyed**, ROS `Adobe-Identity-0` with each CID its glyph id,
when the CFF2 has more than one Font DICT or `post` names no glyphs; with one
Font DICT and `post` names every glyph apart it is **name-keyed**, its charset
those names — the overview's rule. `cff_subset.rs`'s INDEX and DICT writers are
reused, and the CFF is read back through `Cff` before it is returned, every
glyph held to the outline the CFF2 draws at the instance and to its width; a
CFF2 face's top side bearings are measured from its `VORG` origin as `VVAR`
moves it, or from the origin the default instance gives it. More than 256
Font DICTs, which a CFF's FDSelect cannot name, more than 65 535 glyphs, an
operand no Type 2 charstring carries, and a CFF that does not read back are
refused by name.

### Who asks for an instance

- **The builder** (FT-05.12). `DocumentBuilder::add_cid_font_at` and
  `add_embedded_font_at` take the program and an `&Instance`, re-exported on
  the facade as `FontInstance` (ruling 11: an argument type that cannot be
  named is a method that cannot be called), with `UserCoordinate` as
  `FontCoordinate`, and embed the static instance before subsetting. The
  methods without an instance embed a `glyf` face as it is, byte for byte —
  the XPS packages' variable face, the synthesised fixed document's bytes and
  the `xps` fingerprint rest on it — and a face whose outlines are only in
  `CFF2`, which no `/FontFile` stream carries, as its default instance, a
  CFF. A bare CFF is its own instance at any user coordinates and has no
  named one.
- **EPUB and HTML** (FT-05.14, FT-05.15). The CSS crate reads
  `font-variation-settings` (`normal`, or comma-separated pairs of a
  four-character tag of U+0020 to U+007E and a number; a tag given twice
  keeps its last value; css-fonts-4 §8.2) and `font-optical-sizing` (`auto` or
  `none`, §8.1), both inherited. A run's instance follows §7.2's precedence,
  lowest first: `font-weight` sets `wght`, held to the range the
  `@font-face` weight descriptor declares (§4.4); `font-style: italic` sets
  `ital` to 1 and `oblique` sets `slnt` to the negation of §2.4's fourteen
  degrees — CSS measures clockwise and `slnt` counter-clockwise — since the
  cascade and the descriptor both read past an angle; the `@font-face`
  `font-variation-settings` descriptor, read by the property's grammar;
  `font-optical-sizing: auto` setting `opsz` to the used font size in CSS
  pixels; then the property. Values outside the face's axes are clamped by
  the normalisation. `wdth` follows `font-stretch` once CD-01 lands the
  property, and stays at the face's default until then. The face set
  (`epub::typeface::FaceSet`) instances each face once per `Location`, the
  first time a run asks for it, and a run whose location is the face's
  default draws in the face itself; the instances are bounded at 64 a book
  and 64 MiB between them, and a run past either, or whose instance or
  design space a reader refuses, draws at the face's default and is named
  (`FaceDefect::InstancesSpent`, `FaceDefect::InstanceRefused`).
- **A PDF being drawn**: never. Its embedded variable face is drawn at the
  default (FT-05.13 for CFF2; a `glyf` face already is).

## Milestones

1. **FT-05.2, the design space.** `crates/tinker-pdf-font/tests/variations.rs`
   over faces `tests/variable_support/mod.rs` builds byte by byte:
   `axes_and_named_instances_are_read`,
   `user_coordinates_normalise_against_min_default_and_max`,
   `avar_maps_normalised_coordinates_through_its_segments` (at and between
   its points), `coordinates_outside_an_axis_are_clamped`,
   `a_named_instance_resolves_to_its_coordinates`, each expected value
   computed in the test by the specification's formulas;
   `TestGPOSFour.ttf`'s two axes and 36 named instances read as a second
   reader in the test decodes them; and the refusals of an axis count, an
   instance size and a segment count past the table's end.
2. **FT-05.3, the tuple store and `cvar`.** `variations.rs`:
   `a_tuples_scalar_is_the_product_of_its_axis_factors` (peak, intermediate,
   out-of-region and zero-peak axes),
   `packed_point_numbers_and_deltas_decode_in_every_run_form`,
   `the_all_points_form_covers_every_point`,
   `the_cvt_at_an_instance_is_the_default_plus_the_scaled_deltas` at named
   and arbitrary coordinates.
3. **FT-05.4, `gvar`.** `variations.rs`:
   `instance_outlines_equal_the_deltas_applied_in_the_test` at every named
   instance and at arbitrary coordinates (simple, composite and
   untouched-point glyphs), `the_default_instance_is_the_static_outline`,
   `phantom_points_give_the_advance_without_hvar`, and every glyph of
   `TestGPOSFour.ttf` at its named instances equal to a second reader's.
4. **FT-05.5, the item store and the metrics.** `variations.rs`:
   `region_scalars_and_delta_sets_decode_in_every_delta_width`,
   `a_delta_set_index_map_maps_every_glyph`,
   `hvar_advances_at_an_instance_equal_the_deltas_applied_in_the_test`,
   `vvar_metrics_likewise`, `mvar_values_by_tag_likewise`.
5. **FT-05.6, CFF2 read.** `variations.rs`: `a_cff2_program_outlines_its_glyphs`
   (local and global subroutines, an FDSelect of each format, a hint mask
   counted), `a_cff2_default_instance_ignores_its_blends`,
   `garbage_cff2_is_refused_without_panicking`; every `cff.rs` and
   `cff_subset/tests.rs` test unchanged.
6. **FT-05.7, CFF2 blends.** `variations.rs`:
   `cff2_blends_equal_the_deltas_applied_in_the_test` (several regions,
   `vsindex` switching data, blends inside subroutines),
   `a_private_dict_blend_resolves_at_the_instance`,
   `the_default_instance_is_the_unblended_outline`.
7. **FT-05.8, the committed faces.** `tests/variable_fixtures.rs`:
   `the_committed_faces_are_the_builders_bytes`,
   `the_provenance_record_names_every_file_and_every_row_is_true`,
   `each_face_carries_the_tables_it_claims`.
8. **FT-05.9, fuzzing.** `cargo xtask fuzz` and `cargo xtask check` green;
   `fuzz_seeds.rs`'s `the_cff2_seeds_reach_the_parser`, and
   `the_truetype_seeds_reach_the_outlines` counting the variable seed.
9. **FT-05.10, the `glyf` instancer.** `variations.rs`:
   `a_static_instance_re_reads_to_the_instance`,
   `the_instance_carries_no_variation_table`,
   `the_default_instance_keeps_the_default_outlines`.
10. **FT-05.11, the CFF2 instancer.** `variations.rs`:
    `a_cff2_instance_re_reads_as_cff_to_the_blended_outlines`,
    `a_cff2_instance_carries_no_variation_table`,
    `subset_cff_cuts_the_instance_like_any_cff`.
11. **FT-05.12, the builder.** `crates/tinker-pdf-cos/tests/variable_embedding.rs`:
    `an_instanced_embedding_re_reads_to_the_same_outlines`,
    `the_written_program_carries_no_variation_table`,
    `a_cff2_face_is_embedded_not_refused`.
12. **FT-05.13, drawing CFF2.** `crates/tinker-pdf/tests/variable_fonts.rs`:
    `an_embedded_cff2_face_draws_its_default_instance`,
    `a_cff2_glyph_is_chosen_in_a_simple_and_a_composite_font`,
    `an_embedded_gvar_face_draws_its_default_instance`.
13. **FT-05.14, the properties.** `tinker-pdf-css`'s `tests/cascade.rs`:
    `font_variation_settings_parse_cascade_and_inherit`,
    `font_optical_sizing_is_auto_unless_none`; `IMPLEMENTED_NAMES` and
    `UNSUPPORTED_PROPERTIES` still disjoint.
14. **FT-05.15, books.** `crates/tinker-pdf/tests/epub_fonts.rs`:
    `a_variable_face_draws_each_weight_at_its_own_instance`,
    `font_variation_settings_reach_the_instance`,
    `a_book_instances_each_face_once_per_coordinates`.
15. **FT-05.16, the fingerprint.** `determinism.rs`'s
    `rendering_is_stable_across_targets` with the `variable` fixtures' hashes,
    in both builds, natively and on `wasm32-wasip1`.
16. **FT-05.17a, the speed rows, unmeasured.** In MS-04's fonts group, every
    outline of the committed `glyf` face and of the CFF2 face at a named
    instance, and writing a static instance of each; `xtask/src/bench.rs`'s
    `the_committed_baseline_agrees_with_the_tree`. FT-05.17b takes their
    figures in the timing phase.

## Risks

| Risk | Mitigation |
| --- | --- |
| Two implementations of the arithmetic disagree in the last bit — the font crate's and the shaper's, or an outline's and a static instance's | One integer model, its every rounding stated above and transcribed from the overview; the shaper calls the font crate's store evaluator rather than a twin, and the module denies float arithmetic |
| A test that computes its expectation with the code under test proves nothing | The tests compute every expected value from the specification's formulas in their own integer arithmetic, over faces whose every byte the builder states |
| A hostile face spends without bound: axes, tuples, point runs, regions, delta rows, a CFF2 INDEX, blend operands, subroutine depth | Each is bounded — 64 axes, the format's 4 095 tuples and 15-bit point counts, an outline's 2²² point deltas across the glyphs it draws, fewer than 32 768 regions, a 65 536-entry INDEX, a 513-operand stack, ten levels of subroutines, the 64 000-operation budget, each Private DICT read once — and refused by name past its bound (ruling 1); FT-05.9 fuzzes them |
| A region that is zero on every axis moves the default instance | The algorithm is followed and the case recorded above; no font producing one was seen, and the instance at the default of a face without one is its default |
| Changing the shared Type 2 interpreter moves a CFF outline | The flavour gates every CFF2 difference, CFF's silent stops are untouched, and every `cff.rs` and `cff_subset/tests.rs` test is held unchanged |
| A static CFF exceeds Type 2's limits | Operators are split at their operand groups and subroutines inlined; FT-05.11 re-reads every instance through `Cff` |
| The raw-point form drifts between this design and the hinting design | One form: the points this design varies are the ones `docs/design/hinting.md` scales into zone 1, its phantom points placed as that design places them, and FT-05.4 lands after it |
| A book's variable face moves a committed book's pages | The committed books carry static faces only; FT-05.15 holds their page counts, conservation results and fingerprints |
