# Type 42 and multiple-master Type 1 instances

Two PostScript font formats reach PDFs that ISO 32000 does not give a key or
a form to. A **Type 42** program is a TrueType face wrapped in a PostScript
font dictionary; 9.9's Table 124 has no entry for one, so a page that embeds
one embeds it under a key meant for another format. A **multiple-master** Type
1 program holds two to sixteen master designs and the arithmetic that blends
them; 9.6.2.3 has an `/MMType1` font embed a *snapshot*, an ordinary Type 1
program of one instance, so a page that embeds the whole multiple-master
program has the instance to draw only in the font's `/BaseFont`. This design
settles how each is recognised, read and drawn, what is reported, and — since
a multiple-master font converts its design vector with PostScript procedures
this engine does not run (FI-17a is the PostScript interpreter's row) — the
arithmetic that stands in for them.

The rules are transcribed from Adobe's published documentation of the two
formats: Technical Note #5012, *The Type 42 Font Format Specification* (31
July 1998); Technical Note #5015, *Type 1 Font Format Supplement* (15 May
1994), section 3, *Multiple Master Font Extensions*, and its appendices C and
D; *Adobe Type 1 Font Format*, chapters 6 and 8, for `callothersubr` and
`pop`; and Technical Note #5088, *Font Naming Issues*, section 5, for the
design coordinates a name carries. From ISO 32000: 9.2.4 (glyph space),
9.6.2.3 (multiple master fonts), 9.6.5.2 and 9.6.5.4 (encodings) and 9.9
(embedded font programs), whose numbers are the same in ISO 32000-1 and
ISO 32000-2.

## Scope

- **Type 42 read** (FT-10.2): `tinker_pdf_font::type42`, the wrapper TN #5012
  section 2 defines — `/FontType 42`, `/FontMatrix`, `/Encoding`,
  `/CharStrings` and the `/sfnts` strings, hexadecimal and binary — and the
  face the strings reassemble, handed to `Sfnt::parse`; damaged input
  declined by name; a fuzz target of its own.
- **Type 42 drawn** (FT-10.3): a program recognised under any of 9.9's three
  keys drawn as its wrapped TrueType face, each glyph chosen as below, in
  rendering, `to_svg` and the glyph-usage walk alike, reported once a font.
- **Multiple-master instances** (FT-10.4): `Type1` reads a program's blend
  design and weight vector, computes the weight vector for a design vector
  by the supplement's arithmetic, and runs the blend othersubrs, 14 to 18, as
  blends of the masters' operands.
- **An `/MMType1` font's instance** (FT-10.5): the design coordinates its
  `/BaseFont` carries, read by TN #5088's convention, asked of `Type1` in
  rendering, `to_svg` and the glyph-usage walk alike.
- **The speed rows** (FT-10.6), in MS-04's fonts group.

## Non-goals

- **Running PostScript.** No procedure of either format runs: not a Type 42
  glyph whose `/CharStrings` value is a procedure, not
  `NormalizeDesignVector`, `ConvertDesignVector`, `$Blend`,
  `makeblendedfont` or an `/OtherSubrs` entry. Where a font's own procedures
  would compute something the arithmetic below does not, the font draws what
  the design says it draws, and FI-17a's interpreter is the row that would
  run them.
- **CID-keyed Type 42**: a PostScript `CIDFontType 2` CIDFont (TN #5012
  section 5), its `/CIDMap`, `/GDBytes`, `/GlyphDirectory` and
  `/MetricsCount`. It is refused by name; a `/FontType 42` program whose
  `/sfnts` carries a `gdir` table, whose glyphs live in a `/GlyphDirectory`
  rather than in `glyf`, is refused the same way.
- **Hints.** A Type 42 glyph draws as a `/FontFile2` glyph draws; a
  multiple-master glyph draws unhinted ([hinting](hinting.md)'s non-goals).
  The supplement's blended Private dictionary values — `BlueValues`, `StdHW`
  and the rest — are not read.
- **Intermediate masters.** A blend design with a master off the corners of
  its blend space is drawn at its default instance (below), because its
  `ConvertDesignVector` is the font's own and is not the supplement's simple
  linear weighting.
- **Writing either format.** A Type 42 program is written whole on save, as
  every program this engine does not subset is; an instance is not written
  out as a snapshot.

## Design

### Type 42

#### Which keys carry one, and how it is recognised

9.9's Table 124 gives `/FontFile` to Type 1, `/FontFile2` to TrueType and
`/FontFile3` to CFF and OpenType, and none to Type 42. A Type 42 program in a
PDF therefore always stands under a key meant for another format, and it is
recognised **by its content, under any of the three keys**: a program is a
Type 42 program where, after leading white space, it begins with
`%!PS-TrueTypeFont` — the portion of TN #5012 section 2.1's first line it
requires — or where its dictionary sets `/FontType 42` (Table 1). The facade
asks this before it tries the Type 1 parser, which finds `eexec` anywhere in
its input, and a binary `/sfnts` string can hold those five bytes.

**It is a leniency, and it is named.** Drawing it is the engine reading a
program under a key whose format it is not, so each font whose program is read
as Type 42 is reported once, on the document's warnings, against the font's
dictionary — `WarningKind::Type42Program` naming the descriptor key, through
`CosDocument::warn_once` — so a program met at every glyph is named once
(ruling 10).

#### The wrapper

`Type42::parse` reads the font dictionary TN #5012 section 2.2 lays out,
structurally — the program is PostScript, and this is no interpreter, so the
reader looks for the definitions section 7's example makes and decides
nothing by running anything:

- **`/FontType 42`** (Table 1, required).
- **`/FontMatrix`**, a literal array of six numbers (Table 1). Section 2.3
  makes it the identity, the face's em mapping to one unit. It is read and
  does not move the drawing: 9.2.4 fixes glyph space at a thousandth of text
  space for every font but Type 3, and the face is drawn as `/FontFile2`
  draws one (below).
- **`/Encoding`** (Table 2): `StandardEncoding`, an array built by `dup
  <code> /<name> put`, or a literal array of names.
- **`/CharStrings`** (Table 3): a dictionary of glyph name to glyph index,
  written `<n> dict dup begin … end` or `<< … >>`, each entry `/<name>
  <index> def`. An entry whose value is a procedure (section 4.3) names a
  glyph no outline reaches here, and is passed over.
- **`/sfnts`** (Table 3, section 4.2): an array of strings, each hexadecimal
  — `<…>`, white space ignored, an odd final digit followed by 0 as
  PostScript reads one — or binary, `<n> RD` or `<n> -|` and one space
  followed by exactly `n` bytes (section 4.2's own representation, the one
  Type 1 charstrings use). **A string of odd length ends in the padding byte
  section 4.2 adds for interpreters before version 2013, which is not part of
  the face, and loses it**; a string of even length is all face. The strings,
  so trimmed, are concatenated in order, and the bytes are the face, handed
  to `Sfnt::parse`.

Each structure must close: a dictionary without its `end` or `>>`, an array
without its `]`, a hexadecimal string without its `>` and a binary string with
fewer bytes than it declares are declined, as is a program with no
`/FontType 42`, no `/sfnts`, no `/CharStrings`, or a face `Sfnt::parse`
declines. Each refusal is a `Type42Refusal` variant naming the structure.
`/PaintType`, `/FontBBox`, `/FontInfo`, `/Metrics` and the unique
identifiers are not read: 9.9 makes the render mode decide filling, and the
face's own metrics and outlines are what the page draws.

#### Which glyph a code chooses

TN #5012 section 4.5 is the PostScript interpreter's route: the code's name
through `/Encoding`, the name's index through `/CharStrings`, the index into
`loca` and `hmtx`, and the face's `cmap` unread — "it need not be
downloaded", and many wrappers drop it. The PDF font dictionary decides which
route a page takes, as it does for every program:

| Font dictionary | Glyph |
| --- | --- |
| `/Type1` (or `/MMType1`) | 9.6.5.2: the code's name by the dictionary's `/Encoding` over the program's own (`/Encoding`, the built-in encoding), the name's index through `/CharStrings`; a name `/CharStrings` lacks takes its `.notdef` entry, or glyph 0 |
| `/TrueType` | 9.6.5.4, over the wrapped face's `cmap`, exactly as `/FontFile2` would; where the face has no `cmap`, or 9.6.5.4 finds no subtable to read, the code's name through `/CharStrings`, standing in for the `post` names a TrueType face would otherwise give |
| `/CIDFontType2` | 9.7.4.2's `/CIDToGIDMap` over the wrapped face's glyph indices, exactly as `/FontFile2` would |

Every other dictionary over a Type 42 program draws as it draws today: no
route reaches a glyph, and the font is unreadable.

#### Drawing, and saving

The chosen glyph is drawn from the wrapped face as `/FontFile2`'s glyphs are
drawn — the `glyf` outline in design units over the face's `unitsPerEm`,
widths from the font dictionary — so a page embedding a Type 42 program
renders byte for byte as its twin embedding the same face under `/FontFile2`
(FT-10.3's exit). On save the program is written whole, under the key and the
name it came with: the subsetter rewrites programs it can parse as a format it
writes, and a Type 42 wrapper is not one.

### Multiple-master Type 1

#### What a program carries

Section 3 of the supplement adds, in the cleartext half of the program:

- **`/WeightVector`** in the font dictionary (section 3.4): `k` numbers, one
  a master, the contribution of each to the instance the dictionary is. The
  program as shipped is its **default instance** (section 3.2, "It is
  recommended that it be set so the default font instance will be the normal
  roman design").
- In `/FontInfo` (section 3.5, all required): **`/BlendAxisTypes`**, `n` axis
  names; **`/BlendDesignPositions`**, `k` arrays of `n` numbers, each master's
  place in the blend space, every coordinate from 0 to 1; and
  **`/BlendDesignMap`**, `n` arrays of two to twelve `[D B]` pairs, each
  axis's piecewise-linear map from design coordinates `D` to blend
  coordinates `B`.
- One to four axes and two to sixteen masters (section 3.2).
- Charstrings holding every master's operands (section 3.11), each blended
  operand written as the first master's value and the other masters' deltas
  from it, and blended by `callothersubr` 14 to 18 (section 3.12).

The `/Blend` dictionary's per-master `/FontBBox`, `/Private` and `/FontInfo`
values are not read: nothing drawn reads a bounding box or a blue zone here.

#### The instances this engine computes

The supplement's **standard form** is the one its own sections define the
arithmetic for: section 3.9's "simple linear weighting", and Appendix D's
worked weights, apply to masters at the corners of the blend space. So this
engine instances a program whose blend design is:

- one to four axes, `n` the length of `/BlendAxisTypes`, of
  `/BlendDesignMap` and of every position;
- `k = 2^n` masters, `k` the length of `/WeightVector` and of
  `/BlendDesignPositions`, every position's coordinates each 0 or 1, and each
  corner held by exactly one master — in whatever order the program lists
  them, which section 3.5 leaves open;
- each axis's map two to twelve points, its design coordinates rising
  strictly, every blend coordinate from 0 to 1.

**Any other design draws its default instance**, its `/WeightVector`, and
says so (below): a master between the corners (section 3.2's intermediate
designs), a master count that is not `2^n`, a corner held twice or not at all,
a map out of order. And a program whose `/WeightVector` is not two to sixteen
numbers is no multiple-master program this engine blends: its blend
othersubrs are othersubrs it does not know, which chapter 8 of the Type 1
book has the interpreter answer by handing `pop` the arguments, so the
charstring draws its first master — exactly what such a program draws today.

#### From a design vector to a weight vector

Two steps, each section's rule and nothing of the font's own procedures:

1. **Normalising** (section 3.8, Appendix C): each axis's design coordinate
   `d` is mapped through its axis's map. On the segment `D_i ≤ d ≤ D_{i+1}`,
   `b = B_i + (d − D_i) × (B_{i+1} − B_i) ÷ (D_{i+1} − D_i)`, the operations
   in that order; Appendix C's Minion procedure is this formula, segment by
   segment. A coordinate below the map's first point takes its first blend
   coordinate, and one above its last its last: section 3.1 defines blend
   coordinates from 0 to 1 and the map's ends as "the lower limits" and the
   upper ones, and a coordinate past them is reported as clamped.
2. **Weighting** (section 3.9, Appendix D): master `i` at corner `p_i` weighs
   `V_i = Π_j (p_ij = 1 ? b_j : 1 − b_j)`, the product taken over the axes in
   their order. That is section 3.9's three properties — one at its own
   corner, nothing at another's, all equal at the centre — and Appendix D's
   four Myriad weights, `(1−b1)(1−b2)`, `b1(1−b2)`, `(1−b1)b2` and `b1b2`.

Only additions, subtractions, multiplications and divisions of `f64`, which
IEEE 754 rounds exactly and alike on every target, in a fixed order: an
instance is the same bits on every target (ruling 4).

#### What each blend othersubr returns

Section 3.12 numbers the five othersubrs and what they take; section 3.4
gives the `$Blend` they call; chapter 6 of the Type 1 book has
`callothersubr` push its arguments reversed and `pop` take each result off the
top. For `k` masters and weights `W_0 … W_(k−1)`:

| Othersubr | Arguments | Results |
| --- | --- | --- |
| 14 | `k`: `a1`, then `a2 − a1 … ak − a1` | 1 |
| 15 | `2k`: `a1 b1`, then each quantity's `k − 1` deltas in turn | 2 |
| 16 | `3k` | 3 |
| 17 | `4k` | 4 |
| 18 | `6k` | 6 |

Each result is **`a1 + ((Δ2 × W_1 + Δ3 × W_2) + … + Δk × W_(k−1))`**, its
first master's value plus each other master's delta times that master's
weight, the products added from the second master on and the sum added to the
first master's value last — `$Blend`'s own order of operations, `W_0` unused
as it is there. The results are left so that successive `pop`s take them in
the order the arguments gave the quantities, `a` first, which is what the
charstrings of section 3.11 then use (`… 15 callsubr rmoveto` moves by `a b`).
A call whose argument count is not `k` times its result count is an othersubr
this engine does not know, its arguments handed to `pop` as chapter 8 says;
so is every call of a program that is no multiple-master program here. `hsbw`
and `sbw` take blended operands like any other command, so an instance's
advances are its blended widths.

#### The default instance, and the instance a name gives

`Type1::outline` and `Type1::advance` draw the program at the weights it is
set to, its `/WeightVector` until `Type1::instance` sets others — so a
multiple-master program under a plain `/Type1` dictionary draws its default
instance. `Type1::instance(design)` computes the weights for a design vector,
one coordinate an axis, and keeps the default with the reason where the design
is not the standard form or the vector has the wrong length.

An `/MMType1` font names its instance in `/BaseFont`. 9.6.2.3 writes the
PostScript name with its spaces turned to underscores — `"MinionMM 366 465 11
"` becomes `/MinionMM_366_465_11_` — and calls the coordinates in the name a
convention of TN #5088 rather than a requirement. Section 5 of TN #5088 is the
convention: the family and style, an underscore, then each axis's design
coordinate as decimal digits, "any number of digits" and no decimal point,
the coordinates separated by labels of non-digits (`_367wd_450wt`, `_700 wt
470 wd 24 op`). So the facade reads `/BaseFont`, past any subset tag, as: the
name up to its first underscore, then every run of digits after it, in order,
each a coordinate. Where the runs are as many as the program's axes, they are
the design vector; where there are none, or another number, or a run longer
than nine digits, the name gives no instance and the default is drawn.

`FontKind` keeps folding `/MMType1` into Type 1: the facade reads the
dictionary's `/Subtype` only where a program it read has a weight vector,
which is the only place an instance can differ. An instance moves outlines and
advances and never which glyph a code chooses, so rendering and `to_svg`, which
draw through one outline path, ask for it, and the glyph-usage walk, which asks
which glyph, answers alike at every instance. An `/MMType1` font whose
embedded program is a snapshot — the conforming case — has none, and draws
exactly as today; so does every program without a weight vector.

**What is reported.** An embedded multiple-master program is itself a
leniency — 9.6.2.3 asks for the snapshot — so each font whose program blends is
reported once, on the document's warnings against the font's dictionary,
`WarningKind::MultipleMasterInstance`, naming which instance was drawn:
`Design`, the coordinates its name gives; `Clamped`, those coordinates with
one past its map, taken at the map's end; `DefaultType1Font`, the program's
own weights, because its dictionary is `/Type1`; `DefaultNoCoordinates`, its
name carries none; `DefaultAxisCount`, its name carries another number than
the program's axes; and `DefaultBlendDesign`, a blend design this engine does
not instance.

#### Bounds

Every count a program spends is bounded and refused by name: `/WeightVector`
at sixteen numbers and `/BlendDesignPositions` at sixteen masters of four
coordinates, `/BlendDesignMap` at four axes of twelve points, past which the
array is not read; the arrays nested no deeper than their three levels; an
othersubr's blend at the operand stack the interpreter already caps; the
charstring machine at its existing depth and operator caps; a Type 42
wrapper's strings at the input's length, `/CharStrings` at 65 536 entries,
and a nesting of procedures and arrays the wrapper's reader skips at 64
levels. Nothing is allocated from a count a program states before the bytes
behind it are read.

## Milestones

1. **FT-10.1, this document.**
2. **FT-10.2, the Type 42 reader.** `crates/tinker-pdf-font/src/type42.rs`'s
   tests: `the_sfnts_strings_reassemble_the_face_they_wrap` (a face wrapped in
   the test as hexadecimal and as binary strings, split at a table boundary and
   inside a table, padded as section 4.2 pads, reads back byte-identical),
   `charstrings_map_each_name_to_its_glyph`, `every_truncation_is_declined`;
   the `type42` fuzz target, its seeds written by the test's own wrapper, held
   by `cargo xtask fuzz`.
3. **FT-10.3, a Type 42 page.** `crates/tinker-pdf/tests/type42_fonts.rs`:
   `a_type42_face_draws_as_its_embedded_truetype` — a page embedding a Type 42
   program renders byte-identical to its twin embedding the same face under
   `/FontFile2`, and its `to_svg` and extracted text equal the twin's;
   `a_damaged_type42_program_reports_unreadable_font`; `determinism.rs`'s
   `rendering_is_stable_across_targets` holds a Type 42 page; default and
   `bundled-fonts` builds.
4. **FT-10.4, instancing.** `crates/tinker-pdf-font/src/type1.rs`:
   `an_instance_is_its_masters_blended_at_the_design_weights` — a synthetic
   two-axis, four-master program built in the test, instanced at several
   design vectors, gives outline points and advances equal to the masters'
   blended with weights the test computes from the blend design;
   `the_weight_vector_is_the_default_instance`; the `type1` fuzz target
   instances at three vectors.
5. **FT-10.5, an `/MMType1` page.** `crates/tinker-pdf/tests/multiple_master.rs`:
   `an_mmtype1_font_draws_the_instance_its_name_gives` — the page renders
   byte-identical to a plain Type 1 twin whose one master is that instance's
   outlines computed in the test from the blend design;
   `an_mmtype1_font_without_coordinates_draws_its_default_instance`;
   `determinism.rs` holds a multiple-master page; default and `bundled-fonts`
   builds.
6. **FT-10.6, the speed rows.** `crates/tinker-pdf/benches/fonts.rs`: `parse a
   Type 42 program and outline every glyph` and `instance a multiple-master
   Type 1 program and outline every glyph`, each on a generated program sized
   into MS-04's window; `xtask/src/bench.rs`'s
   `the_committed_baseline_agrees_with_the_tree` holds them and
   `baseline.json` to one list.

## Risks

| Risk | Mitigation |
| --- | --- |
| A font's own `NormalizeDesignVector` or `ConvertDesignVector` computes another weight vector than the supplement's arithmetic | Only the standard form is instanced, the form those sections define the arithmetic for; a font whose procedures depart from its own map is not detected without running them, and is FI-17a's |
| An instance differs in its last bits from a PostScript interpreter's, which blends in single-precision reals | No outside program adjudicates (ruling 13); the arithmetic is stated here, its order fixed, and the same on every target |
| A binary `/sfnts` string holds `eexec` and the Type 1 parser claims the program | Type 42 is recognised first, by its first line or its `/FontType 42` |
| A wrapper splits the face at an odd offset with no padding byte, and loses a byte of it | Section 4.2 makes every string's face data even and its padding the one odd byte; a face so cut fails `Sfnt::parse` or its tables' own checks, and is declined rather than drawn wrong |
| A name's digits are not its coordinates — a family name with digits after an underscore | Only as many runs as the program has axes are taken as coordinates; any other count draws the default, reported |
| A hostile program claims a huge weight vector, design or dictionary | Each array is bounded at the supplement's own limits and refused past them; every string at the input's length |
| A multiple-master program that draws its first master today draws its default instance | That is the program's own instance, the one a PostScript interpreter draws; a program with no weight vector, and every ordinary Type 1 program, draws exactly as before, and no committed fixture carries a multiple-master program |
