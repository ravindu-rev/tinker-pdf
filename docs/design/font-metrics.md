# Font measurement

The facade reads font programs to draw them, and until this answers nothing
about one to a caller: not how wide a glyph is, not which characters a face
has, not what its name for a glyph is. This design settles the facade's font
measurement API — `FontMetrics`, built from a program's bytes or from a
document's embedded program — and the definition, the unit and the source
table of each answer for each format the font crate reads. Each definition is
a public commitment that the 0.1.0 freeze (H-114) makes permanent, which is
why each is written down here, against its specification, before any of it is
built.

The rules are read from ISO 32000-2: 9.2.4 (glyph space, the displacement and
the glyph bounding box: "the smallest rectangle … that just encloses the
entire glyph shape"), 9.8.2's Table 121 (Table 123 of ISO 32000-1: the font
flags and what the Symbolic and Nonsymbolic flags mean) and Annex D (Table
D.2's Standard Latin character set, D.5's Symbol set and D.6's ZapfDingbats
set). From the formats: OpenType 1.9.1's `head`, `hhea`, `hmtx`, `maxp`,
`loca`, `glyf`, `cmap`, `post` and `OS/2` chapters, with the IBM font family
classes `sFamilyClass` holds and PANOSE 2.0's digits; Adobe's TN #5176, *The
Compact Font Format Specification*, section 9's Table 9 (Top DICT) and
section 15's Table 23 (Private DICT); TN #5177, *The Type 2 Charstring
Format*; *Adobe Type 1 Font Format*, chapter 2 (the font dictionary and
`/FontInfo`), section 4.1 (points at extremes) and chapter 5 (the Private
dictionary's `/StdVW` and `/ForceBold`); TN #5012 for Type 42; and the Adobe
Glyph List specification's algorithmic names.

## Scope

- **The type and its constructors** (FT-16.4): `FontMetrics`, from a
  program's bytes in each format the font crate reads, a collection's face by
  its index, and a `DocumentFont`'s embedded program.
- **Its answers** (FT-16.4): the glyph count and units per em; a glyph's
  advance and box, the font's box, in font units and scaled to a size;
  `has_glyph` and the glyph a character draws; every code point the face
  maps; glyph names both ways; 9.8.2's flags; and a text's length at a size.
- **What the font crate reads for them and does not today** (FT-16.2 and
  FT-16.3): every scalar an sfnt's `cmap` maps, its `post` names forwards, its
  glyph and font boxes, its style facts, a collection's other faces; a CFF's
  Top DICT and Private DICT facts and a Type 1 program's `/FontBBox`,
  `/FontInfo` and Private facts.
- **What it reports**: nothing, on `Document::warnings` or anywhere.
- **Its proof**: each answer held to an independent reading in the test
  (FT-16.4), its fingerprint on every target (FT-16.5) and its speed row
  (FT-16.6).

## Non-goals

- **Hinting** (FT-04b to FT-04d). Every answer is the outline the program
  states, unhinted.
- **Variation instances** (FT-05). A variable face is measured at its default
  instance: `hmtx`'s advances, `glyf`'s points without `gvar`, a `CFF2`
  table's charstrings with every blend at the default.
- **Colour glyphs** (FT-06). A glyph's box is its outline's, whatever a colour
  table draws for it.
- **Shaping.** A text's length is one glyph a character with no substitution
  or positioning; a shaped run's advances are `tinker-pdf-shape`'s, which the
  facade's shaping writes from (*Text length*).
- **Binding projections**, which follow the facade (ruling 11): the C ABI and
  the bindings owe the type to SU-12.22, beside the font listing.

## Design

### The type and its constructors

```rust
pub struct FontMetrics { /* owned */ }

impl FontMetrics {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<FontMetrics, FontMetricsError>;
    pub fn from_collection(bytes: Vec<u8>, face: u32) -> Result<FontMetrics, FontMetricsError>;
}
impl DocumentFont {
    pub fn metrics(&self) -> Result<FontMetrics, FontMetricsError>;
}

#[non_exhaustive]
pub enum FontFormat { TrueType, OpenTypeCff, OpenTypeCff2, Cff, CidKeyedCff, Type1 }

#[non_exhaustive]
pub enum FontMetricsError {
    /// No reader of this engine takes the bytes as a font program.
    NotAFont,
    /// A collection has no face of the index asked for.
    NoSuchFace { faces: u32 },
    /// A WOFF or WOFF 2.0 container that does not decode within its limit.
    Container,
    /// The document embeds no program for the font.
    NotEmbedded,
    /// The document's program stream does not decode.
    Undecodable,
}
```

`FontMetrics` owns its bytes and is `Clone`, `Send` and `Sync`, so a caller
holds one as long as it likes and asks from any thread. `from_bytes` takes,
in this order of recognition:

| Bytes | Read as | `format()` |
| --- | --- | --- |
| `wOFF`, `wOF2` | the sfnt the container wraps, decoded through `woff::decode` within 16 MiB (the EPUB reader's bound), then as below | the wrapped face's |
| A Type 42 program (`Type42::is_type42`) | its reassembled face, as below; its glyph names `/CharStrings`'s | its face's |
| `0x00010000`, `true`, `OTTO`, `ttcf` | an sfnt: a collection's first face, as a document embedding one means it | `TrueType` with `glyf`, `OpenTypeCff` with `CFF `, `OpenTypeCff2` with `CFF2` |
| A CFF header | a bare CFF, its first font | `CidKeyedCff` with `ROS` (TN #5176 Table 10), else `Cff` |
| `eexec` in the cleartext | a Type 1 program: PFA, PFB, or as a PDF embeds it; a multiple-master program at its `/WeightVector` | `Type1` |

An sfnt with no outline table this crate reads is `TrueType`, and answers its
tables without boxes. `from_collection` takes a collection's face by its index
in the `ttcf` header (`numFonts` bounded by what the header holds), whether
the bytes are the collection or a WOFF 2.0 container of one, and a plain sfnt
as face 0 alone. `DocumentFont::metrics` decodes the program the listing found
(`DocumentFont::program`) and reads it as `from_bytes` does.

### Units and scale

Each answer is in the face's **font units**, and each `_at` answer in **text
space at a size**, the size the unit a caller's size is in:

| Format | Font units | Scale `s` of a glyph, font units to the em |
| --- | --- | --- |
| sfnt, all outlines | `head`'s `unitsPerEm` (1000 where it is absent or below 16, as `Sfnt` reads it) | `1 / unitsPerEm` |
| Bare CFF | the charstrings' | the glyph's font matrix's `a` (`Cff::font_matrix_for`: a CID-keyed font's Font DICT matrix composed with the Top DICT's) |
| Type 1 | the charstrings' | `/FontMatrix`'s `a` |

A matrix whose `a` is zero or not finite scales by 1/1000, glyph space's own
ratio (9.2.4). An sfnt's `CFF ` and `CFF2` charstrings are taken in the face's
units, as OpenType writes them and as the font crate's vertical origins read
them, so every answer of an sfnt shares `head`'s em; the table's own
FontMatrix is not read. `units_per_em()` is `1 / s` of the font as a whole:
`head`'s for an sfnt, the Top DICT's matrix's for a CFF, `/FontMatrix`'s for a
Type 1 program.

A value `v` at size `z` is `v × s × z`, evaluated left to right; a box's
corners are ordered after scaling, so a negative size or matrix gives an
ordered box.

### Glyph count

An sfnt's is `maxp`'s `numGlyphs`, or, with no `maxp` long enough to state it,
what its outline table holds: `loca`'s entries less one, or the CFF's
CharStrings INDEX count. A bare CFF's is its CharStrings INDEX count; a Type 1
program's, the `/CharStrings` entries read, numbered in the order the
dictionary lists them — which is the glyph index every answer here takes. A
glyph at or past the count has no advance, box or name.

### Advance

The glyph's horizontal displacement (9.2.4), in font units:

- **sfnt**: `hmtx`, through `hhea`'s `numberOfHMetrics`, the last advance
  repeating past it (`Sfnt::advance`), for an OpenType CFF face too, whose
  charstring widths OpenType sets aside for `hmtx`.
- **Bare CFF**: the charstring's width — its first stack-clearing operator's
  extra operand plus the Private DICT's `nominalWidthX`, or `defaultWidthX`
  where it has none (TN #5176 section 15) — `Cff::advance`.
- **Type 1**: its `hsbw` or `sbw` width (`Type1::advance`).

### Glyph boxes

- **A `glyf` glyph**: the four `FWORD`s its header states (`glyf`'s
  `xMin`, `yMin`, `xMax`, `yMax`, the minimum and maximum of its coordinate
  data), read through `loca`, as written: the box the face states, which the
  `vmtx` clause and every TrueType reader read. A glyph whose `loca` range is
  empty — a space — is bounded at its origin, all four zero.
- **A CFF, `CFF2` or Type 1 glyph**, whose box no table states: **9.2.4's
  box, the smallest rectangle enclosing the shape the charstring draws.** Over
  each contour that draws something — a `moveto` followed by no drawing
  operator draws nothing and bounds nothing — the box holds the contour's
  start, every segment's end point, and each curve's extremes inside it: for a
  cubic of control values `p₀ … p₃` on an axis, with `d₀ = p₁ − p₀`, `d₁ = p₂ −
  p₁`, `d₂ = p₃ − p₂`, the roots in `(0, 1)` of `A t² + B t + C`, `A = d₀ − 2d₁ +
  d₂`, `B = 2(d₁ − d₀)`, `C = d₀` — `t = −C / B` where `A` is zero, else `t =
  (−B ± √(B² − 4AC)) / 2A` where the discriminant is not negative — each
  evaluated by de Casteljau's construction, `q = p + (p′ − p) × t` three times
  over. Control points off the shape do not widen it. A glyph that draws
  nothing is bounded at its origin. The Type 1 book's section 4.1 asks a font
  to put a point at most extremes, and where it does the box is its points';
  a curve whose extreme falls between them — a curved serif's tip — is
  bounded at its extreme.

The arithmetic is IEEE 754's four operations and `sqrt`, so a box is the same
bits on every target (ruling 4). FT-14's heights read a CFF or Type 1 glyph's
top from this box ([metric substitution](metric-substitution.md)).

### Font box

- **sfnt**: `head`'s `xMin`, `yMin`, `xMax` and `yMax`.
- **Bare CFF**: the Top DICT's `FontBBox` (5); absent, or with other than four
  operands, TN #5176 Table 9's default, `0 0 0 0`.
- **Type 1**: `/FontBBox`, an array or a procedure of four numbers in the
  cleartext; absent, `0 0 0 0` — the Type 1 book, which makes the entry
  required, states no default, and section 2.3 allows all zeros; Table 9
  gives the same key that default.

### Characters

`glyph_for_char(c)` is the glyph a character draws; `has_glyph(c)` whether
there is one; `code_points()` every character there is one for, in ascending
order. A character "has a glyph" when the face maps it to a glyph other than
glyph 0 and below the glyph count.

- **sfnt**: the `cmap` subtable `Sfnt::glyph_for_char` chooses — (3,10), then
  (3,1), then platform 0, then (3,0), then (1,0): 9.6.6.4's order, with the
  subtables past the Basic Multilingual Plane first — read exactly as that
  lookup reads it: a
  (3,0) subtable answers a character below U+0100 at `0xF000` plus it, and its
  `F0xx` codes as themselves; a Macintosh subtable answers no character past
  U+007F; each format `lookup_cmap` reads (0, 2, 4, 6, 12, 13) is read as it
  is there, a subtable out of its format's order as written, the first entry
  that holds a code answering it. `code_points` is every scalar value that
  lookup answers with a glyph, read once (`Sfnt::char_map`, FT-16.2).
- **A Type 42 program**: its face's `cmap`.
- **A name-keyed CFF or Type 1 program**: each glyph's name stands for the
  character its name gives, the lowest glyph answering a character two names
  give. A name stands for a character when, from its first period on dropped
  and holding no underscore (which names a ligature of several), it is a name
  of Table D.2's Standard Latin set — the character D.2 gives it — or of
  D.5's Symbol set or D.6's ZapfDingbats set, the character their tables give
  (`BuiltInEncoding::unicode_of`, which also holds the fourteen names the ITC
  Zapf Dingbats Glyph List gives that D.6 encodes none of); or the Adobe Glyph
  List specification's algorithmic forms, `uni` and exactly four upper-case
  hexadecimal digits, or `u` and four to six, naming a scalar value that is
  not a surrogate. `.notdef` stands for none, as does every other name.
- **A CID-keyed CFF** maps no character: its glyphs are numbered by CID in a
  collection, and the collection's CMaps, not the program, say which
  characters they are.

### Glyph names

`glyph_name(glyph)` and `glyph_for_name(name)`, the lowest glyph a name is
given to:

- **A `glyf` face** names its glyphs by `post`: version 1.0 is the 258
  standard Macintosh names in their order, one a glyph, up to `maxp`'s count;
  2.0 gives each glyph an index, below 258 into that order and from 258 into
  the table's own Pascal strings; 2.5 gives each glyph a signed offset into
  the order; 3.0 and any other version name nothing.
- **An OpenType CFF face** by its `CFF ` table's charset, as a bare CFF; a
  `CFF2` face, whose table names nothing, by `post`.
- **A bare CFF** by its charset and string INDEX (TN #5176 sections 10 and
  13); a CID-keyed one names nothing.
- **A Type 1 program** by its `/CharStrings` keys; **a Type 42 program** by
  its `/CharStrings` keys, a glyph that several keys give taking the first
  written, and by its face's `post` where `/CharStrings` gives none.

A name that is not UTF-8 is none.

### Flags

`flags()` answers 9.8.2's Table 121 bits, each from the table that states it,
as a `FontFlags` of bit positions 1 to 32:

| Bit | Flag | sfnt | Bare CFF | Type 1 |
| --- | --- | --- | --- | --- |
| 1 | FixedPitch | `post` `isFixedPitch` not zero | Top DICT `isFixedPitch` (12 1) not zero; Table 9's default false | `/FontInfo` `/isFixedPitch true` |
| 2 | Serif | `OS/2` `sFamilyClass`'s class 1 to 5 or 7; with class 0, PANOSE's family kind 2 (Latin Text) and serif style 2 to 10 (Cove to Triangle) | — | — |
| 3 | Symbolic | the complement of Nonsymbolic | | |
| 4 | Script | `sFamilyClass`'s class 10; with class 0, PANOSE's family kind 3 (Latin Hand Written) | — | — |
| 6 | Nonsymbolic | the chosen subtable is not (3,0), and the face maps at least one character and only characters of D.2's set | not CID-keyed, and it names at least one glyph and every name but `.notdef` is a D.2 name | every name but `.notdef` is a D.2 name, and there is one |
| 7 | Italic | `post` `italicAngle` not zero, or `fsSelection` `ITALIC` (bit 0) or `OBLIQUE` (bit 9), or `macStyle` italic (bit 1) | Top DICT `ItalicAngle` (12 2) not zero; Table 9's default 0 | `/FontInfo` `/ItalicAngle` not zero |
| 17 | AllCap | — | — | — |
| 18 | SmallCap | — | — | — |
| 19 | ForceBold | an OpenType CFF face's Private DICT, as a bare CFF's | the Private DICT's `ForceBold` (12 14), a CID-keyed font's first Font DICT's; Table 23's default false | the Private dictionary's `/ForceBold true` (section 5.10) |

A dash is a flag no table of the format states, and it is clear. **Symbolic
and Nonsymbolic are 9.8.2's definitions read off the program**: a font whose
character set is not a subset of the Standard Latin set, or does not use its
standard names, is symbolic, so a face of Latin, Greek and Cyrillic is
symbolic though it draws Latin text, and exactly one of the two is set. **No
table states AllCap or SmallCap**, and a face's repertoire does not show them
— an all-capitals face maps its lowercase letters to capitals, a small-capitals
face maps them to small capitals — so both are clear. A Type 42 program's
flags are its face's. The style facts each column reads are FT-16.2's and
FT-16.3's readers, and FT-14's synthesis reads the same ones.

### Text length

`text_length(text, size)` is one glyph a character with no shaping: each
character's glyph as `glyph_for_char` answers it, or, where there is none, the
face's `.notdef` — glyph 0 of an sfnt or a CFF, the glyph a Type 1 program
names `.notdef`, nothing where it names none — whose advance a renderer draws
for a character the face lacks; the advances in font units times each glyph's
scale, summed left to right from zero, times `size`. No ligature, no kerning,
no `GPOS`, no bidirectional reordering: what a content stream's `Tj` places
for the string when the font's widths are the face's.

### What a measurement reads and reports

**Nothing is reported, and nothing is added to `Document::warnings`.**
`FontMetrics::from_bytes` holds no document. `DocumentFont::metrics` decodes
its program through `CosDocument::stream_decoded_unrecorded` (FT-16.4), which
runs the stream's filters and finds its extent as `stream_decoded` does and
drops what they tolerated rather than recording it, and keeps no extent it
found, so the render that reads the stream later records what it would have:
measuring a font neither adds a warning nor takes one from a later read, as
listing the fonts does not (`fontlist.rs`'s `listing_fonts_adds_no_warnings`).

### Bounds

Every count a program carries is held to what its bytes hold (ruling 1): the
table directory's 512 records and a `cmap`'s 64; a format 4 subtable's
segments, read once in order, and at most 65 536 scalars; a format 12 or 13
subtable's groups, at most 2²⁰ and what it holds, walked once against the
scalars already answered, so the walk is linear in the groups and the scalars
it answers; a format 2 subtable's 65 536 codes; `post`'s strings, read once, no
more than the table's bytes; a collection's `numFonts`, no more than its
header holds; a CFF's INDEX counts, a charstring's work (`Cff`'s budget) and a
Type 1 program's (`Type1`'s operation cap); a WOFF's output, 16 MiB. A box is
computed for the glyph asked for, and a CFF's are read 256 glyphs at a time,
one parse a block, so no answer runs every charstring of a face; the names and
the character map are read once, at construction, linear in the program.

### The independent reading the test holds each answer to

FT-16.4's `each_answer_is_the_faces_own_tables` holds every answer, over every
committed face — `crates/tinker-pdf-font/data/liberation`,
`crates/tinker-pdf-shape/data/text-rendering-tests/fonts`,
`crates/tinker-pdf-shape/data/aots/fonts`, and the CFF and Type 1 programs
under `fuzz/corpus` — to a reader written in the test that shares no code
with `tinker-pdf-font`:

| Answer | sfnt | Bare CFF | Type 1 |
| --- | --- | --- | --- |
| Glyph count, units | the test's directory reader: `maxp`, `head` | its INDEX reader: CharStrings, the Top DICT's `FontMatrix` | its eexec decryption and `/CharStrings` reader, `/FontMatrix` |
| Advance | `hhea` and `hmtx` | its Type 2 interpreter's width | its Type 1 interpreter's `hsbw` or `sbw` |
| Glyph box | `loca` and the `glyf` header; for `CFF `, its Type 2 interpreter's outline and the box above | its Type 2 interpreter's outline | its Type 1 interpreter's outline, `seac` and the blend othersubrs at `/WeightVector` included |
| Font box | `head` | the Top DICT's `FontBBox` | the cleartext's `/FontBBox` |
| Characters | its `cmap` reader of the subtable choice and the formats the faces use | the charset and the standard strings, transcribed from TN #5176 Appendix A, and Table D.2's names and the algorithmic forms, transcribed from Annex D and the Glyph List specification | the `/CharStrings` keys and the same names |
| Names | `post` 1.0, 2.0, 2.5 and 3.0, and the `CFF ` charset | the charset | the `/CharStrings` keys |
| Flags | `post`, `OS/2`, `head` and the chosen subtable | the Top DICT and Private DICT | `/FontInfo` and the Private dictionary |
| Text length | the advances above, summed | the same | the same |

A box edge a point gives is held exactly, and one a curve's extreme gives
within 10⁻⁹ of a font unit, the rounding of the same stated arithmetic. A
`DocumentFont` embedding a Liberation face answers as its bytes do, and the
document's warning count is the same after as before.

## Milestones

1. **FT-16.2, what `Sfnt` reads.** `Sfnt::char_map` and `code_points`,
   `glyph_name` for `post` 1.0, 2.0, 2.5 and 3.0, `glyph_box` through `loca`,
   `font_box`, `style` for the facts of `OS/2`, `post` and `head` the flags
   read, and `Sfnt::parse_face` and `faces` for a collection. Exit: `sfnt.rs`'s
   `code_points_are_every_scalar_the_subtable_maps`,
   `post_names_glyphs_in_formats_1_2_and_3`, `a_glyph_box_is_its_glyf_header`,
   `the_font_box_is_heads` and `the_style_facts_are_os2_posts_and_heads`, over
   hand-built tables whose values the test writes; a proptest that none of
   them panics on arbitrary bytes; the `sfnt` fuzz target calls each.
2. **FT-16.3, a CFF's and a Type 1 program's facts.** `Cff::style` (the Top
   DICT's `FontBBox`, `isFixedPitch`, `ItalicAngle`, `UnderlinePosition`,
   `UnderlineThickness` and `Weight`, the Private DICT's `ForceBold` and
   `StdVW`) and `Type1::style` (`/FontBBox`, `/FontInfo`'s same entries, the
   Private dictionary's `/ForceBold` and `/StdVW`), an absent entry its
   default. Exit: `cff.rs`'s `the_top_dicts_box_and_style_entries_are_read`
   over a hand-built Top DICT and `fuzz/corpus/cff/three_glyphs.cff`, and
   `type1.rs`'s `font_bbox_and_font_info_are_read` over
   `fuzz/corpus/type1/square.pfb`; the `cff` and `type1` fuzz targets call
   them.
3. **FT-16.4, the API.** `crates/tinker-pdf/src/font_metrics.rs`,
   `DocumentFont::metrics`, and `CosDocument::stream_decoded_unrecorded`.
   Exit: `tests/font_metrics.rs`'s `each_answer_is_the_faces_own_tables` with
   its `RAN` line counting the faces; a `DocumentFont` embedding a Liberation
   face answering as its bytes do with the warning count unchanged; a
   proptest feeding the constructors arbitrary bytes; default and
   `bundled-fonts` builds.
4. **FT-16.5, the fingerprint.** Exit: `determinism.rs`'s
   `font_measurements_are_stable_across_targets`, hashing every answer over a
   committed Liberation face, text lengths at fractional sizes among them,
   entered in K-07's roster, holding its hash on Linux, Windows and
   `wasm32-wasip1`.
5. **FT-16.6, the speed row.** Exit: `xtask/src/bench.rs`'s
   `the_committed_baseline_agrees_with_the_tree` with the operation that asks
   every answer for every glyph and code point of each committed Liberation
   face, and `cargo bench -p tinker-pdf` reporting its time.

## Risks

| Risk | Mitigation |
| --- | --- |
| A definition turns out wrong after 0.1.0 freezes it | Each is written here against its clause and pinned by the test and the fingerprint before the freeze |
| A CFF table's FontMatrix and `head`'s em disagree, as test faces' do | Stated: an sfnt is measured in `head`'s units, one em for every table; the renderer draws such a face's outline at its matrix, and the two agree wherever the face is well formed |
| A last-resort face maps every scalar value | The map is one entry a scalar, at most 1 114 112, read once and bisected |
| A hostile CFF makes every charstring run to its budget | No answer runs more than a block of 256; a text's length runs no charstring for a TrueType face and one per distinct glyph block otherwise |
| A Type 1 or CFF glyph's names stand for characters the engine's text reading reads otherwise | Stated: the API reads Annex D's sets and the specification's forms, strictly; the reader of a document's `/Differences` keeps its own leniencies |
| Symbolic surprises a caller who measures a Latin face with Cyrillic | It is 9.8.2's own definition, its NOTE 1 says so, and the table above gives the reading |
| A measurement adds a warning through the program's stream | `stream_decoded_unrecorded` records nothing and keeps no extent; the test asserts the count |
