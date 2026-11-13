# Colour fonts

*For [ROADMAP](../ROADMAP.md) row FT-06.* An OpenType face can carry its
glyphs in colour five ways: `COLR` version 0 stacks outline glyphs, each
filled with a `CPAL` palette entry; `COLR` version 1 describes a glyph as a
graph of paints — fills, gradients, transforms, clips and composites; `sbix`
and `CBLC`/`CBDT` carry a bitmap per glyph per strike; and `SVG ` carries an
SVG document per glyph range. This document fixes the plain values
`tinker-pdf-font` hands back for each (ruling 8), the limits a hostile table
meets (ruling 1), the PDF object a colour glyph is written as and the
construct each paint becomes, what that construct cannot say exactly, and
how the tests that hold each later step compare pixels. Every rule cites the
OpenType 1.9.1 chapter and section it comes from, and every PDF construct
the ISO 32000-2 clause that defines it.

## Scope

- **The interpreter's Type 3 text** (FT-06.2): a Type 3 glyph reaches the
  text device, so a colour glyph written as Type 3 keeps the text a CID run
  extracts.
- **The readers** (FT-06.3 to FT-06.8, FT-06.21): `CPAL` versions 0 and 1,
  `COLR` version 0, `COLR` version 1's paint graph, `sbix`, `CBLC` with
  `CBDT`, the `SVG ` document index with its gzip-encoded documents, each
  bounds-checked and handing back plain values.
- **An SVG element read by its id** (FT-06.9), so one glyph is drawn out of a
  document that holds many.
- **The writer** (FT-06.10 to FT-06.16, FT-06.18 to FT-06.20): colour glyphs
  written into a PDF by `DocumentBuilder`, their text extractable, each paint
  as the construct below, at the default instance and at a variable
  instance.
- **The price** (FT-06.17a, FT-06.17b), which H-141 rules with.
- **The reading side's answer**: whether a PDF this engine reads ever draws
  the colour tables of a font program it embeds. It does not (*Embedded
  colour tables are never drawn*, below), so the row owes no reading work.

## Non-goals

- **Drawing an embedded program's colour tables when a PDF is read.** ISO
  32000-2 9.9.1 finds a glyph's description in an embedded program's `glyf`
  or `CFF ` table only, and 9.3.6 makes the text rendering mode, not the
  program, decide how it is painted; reading a page draws exactly that.
- **Animation.** The `SVG ` chapter's *Animations* section makes a static
  rendering the document with no animation run; that is what is drawn.
- **Hinted outlines inside a colour glyph.** A `PaintGlyph` or a version 0
  layer is an outline, drawn unhinted as every outline is (FT-04a to FT-04d
  hold hinting).
- **Colour fonts in vertical runs beyond the run writer's.** A colour glyph
  is placed wherever the run writer places a glyph; the bitmaps' vertical
  metrics (`vertBearingX`, `vertBearingY`, `vertAdvance`) are read and
  handed back, and placed by a vertical run when the run writer sets one.

## Design

### Embedded colour tables are never drawn

ISO 32000-2 9.9.1, after Table 124, states the whole process by which a PDF
processor finds a glyph description in an OpenType program: a Type 1 font's
glyphs through the `CFF ` table (9.6.5.2), a TrueType font's through `glyf`
(9.6.5.4), and a CIDFont's through `CFF ` or `glyf` (9.7.4.2). No colour
table appears in it, and the same clause makes the fill-or-stroke decision
"entirely determined by the PDF text rendering mode parameter" (9.3.6). A
PDF that embeds a colour face therefore draws its outlines in the text's
colour, which is what this engine does, and no row is owed. A colour glyph
is drawn in colour only when it is *written* as content, which is what the
rest of this document is about.

### What the font crate hands back

Five readers, each over its table's own bytes as `Sfnt::table` gives them,
each answering `None` for what it cannot read rather than guessing:

| Module | Type | Answers |
| --- | --- | --- |
| `cpal.rs` | `Cpal` | palette and entry counts; `colour(palette, entry)` as `Rgba` (sRGB, not premultiplied); each palette's type flags and label name ID; each entry's label name ID; `choose(PaletteChoice)` |
| `colr.rs` | `Colr` | version 0: `layers(glyph, &Cpal, palette)` as `Layer { glyph, colour }` bottom first; version 1: `paint_graph(glyph)` as a `PaintGraph`, `clip_box(glyph)` |
| `sbix.rs` | `Sbix` | strikes (`ppem`, `ppi`); `glyph(strike, glyph)` as origin offsets, graphic type and bytes; `choose_strike(glyph, StrikeChoice)`; whether outlines are drawn over |
| `cbdt.rs` | `ColourBitmaps` | strikes (`ppemX`, `ppemY`, bit depth, glyph range); `glyph(strike, glyph)` as metrics and PNG bytes; `choose_strike(glyph, StrikeChoice)` |
| `svg_table.rs` | `SvgTable` | `document(glyph)` as the document's bytes, inflated when gzip-encoded, and the element id `glyph<id>` |

A colour reference is `ColourIndex::Palette(entry)` or
`ColourIndex::Foreground` — 0xFFFF, which COLR's *Colors and solid color
fills* says "must not be treated as an actual index" — and a version 0 layer
comes resolved, `LayerColour::Colour(Rgba)` or `LayerColour::Foreground`.
Numbers come back as the table writes them: an `F2DOT14` as its `i16`, a
`Fixed` as its `i32`, an `FWORD` as its `i16`, with conversions beside them
(`f2dot14`, `fixed`) that divide by 16 384 and 65 536 exactly. Nothing is
interpolated, clamped or resolved against a palette in the font crate past
version 0's layers; that is the writer's, which knows the palette and the
text colour.

**Sorted arrays.** COLR's `BaseGlyph` records and `BaseGlyphPaintRecord`s,
its `Clip` records, the `SVG ` document records and the `IndexSubtableRecord`s
of `EBLC` are each required in increasing glyph order. Each array's order is
checked once when the table is parsed: in order, a lookup bisects; out of
order, it scans and the first record whose glyph or range holds the glyph
answers — the rule `Sfnt`'s `cmap` lookup already keeps for a subtable out of
its format's order — and the table records a typed leniency saying which
array was out of order (ruling 10). Overlapping ranges, which the `SVG `
chapter's *SVG table formats* and COLR's ClipList forbid, are read the same
way: the first in table order wins.

**Versions.** `CPAL` versions 0 and 1 (*Palette table header*), `COLR`
versions 0 and 1 (*COLR header*), `sbix` version 1 (*Header*), `CBLC` and
`CBDT` major version 3 (*CblcHeader*, *CbdtHeader*) and `SVG ` version 0
(*SVG table formats*) are read; any other version answers `None`, because a
version this design has not read is a layout it cannot vouch for. A `COLR`
present with no `CPAL` is ignored, as the COLR chapter's *Overview* says.

### CPAL and the palette

`CPAL`'s *Palette entries and color records*: an entry is
`colorRecordIndices[palette] + entry` into one array of BGRA records. Each
lookup checks the palette against `numPalettes`, the entry against
`numPaletteEntries`, the sum against `numColorRecords` and the record against
the table's end, and answers `None` past any of them — so a palette whose
first index leaves too few records answers for the entries it has and `None`
after. Version 1's three arrays are read only when their offset is non-zero
and the array lies inside the table; a label of 0xFFFF is "no string"
(*Palette labels array*, *Palette entry label array*) and answers `None`.

**Which palette.** `PaletteChoice` is the writer's parameter:

- `Default` — palette 0, *Palette table header*'s default palette.
- `Index(n)` — palette `n`, or palette 0 where the face has fewer.
- `LightBackground` and `DarkBackground` — the first palette whose type has
  `USABLE_WITH_LIGHT_BACKGROUND` or `USABLE_WITH_DARK_BACKGROUND` set
  (*Palette types array*), or palette 0 where none has; a version 0 table has
  no types and answers palette 0.

The `SVG ` chapter's *Color palettes* makes the same choice for an SVG
glyph's `var(--colorN)`: the chosen palette's entry `N`, its alpha multiplied
into the opacity property of the element it colours.

**The foreground** is the text's own fill colour where the glyph is shown.
A Type 3 glyph procedure inherits the graphics state of the text-showing
operator but for the CTM (ISO 32000-2 9.6.4), so a foreground layer is
filled without setting a colour and paints in whatever colour the text was
set in; an SVG glyph's `currentColor` is the same colour, as the `SVG `
chapter's *Colors* recommends.

### COLR version 0

`layers(glyph, &Cpal, palette)` finds the glyph's `BaseGlyph` record and
answers its `numLayers` layer records from `firstLayerIndex`, bottom first
(*BaseGlyph and Layer records*), each palette entry looked up in the chosen
palette. It answers `None` — the glyph is then not a colour glyph — where
the slice runs past `numLayerRecords` or the table, or where any layer's
entry is neither 0xFFFF nor an entry the palette has: a glyph drawn with a
layer missing is a different picture, and its outline is the honest one.

### COLR version 1: the plain graph

`paint_graph(glyph)` answers the graph under the glyph's
`BaseGlyphPaintRecord`, or `None` where it has none or where its root paint
cannot be read. *Mixing version 0 and version 1 formats* gives version 1 the
preference, so a glyph with both records is read through its graph.

A `PaintGraph` is an arena of nodes, each paint table one node, its children
node indices, and the root an index. Every one of the thirty-two formats of
*Paint tables* reads to a `Paint` variant holding each field as written:

| Formats | Node |
| --- | --- |
| 1 | `Layers` — the `numLayers` paints of the LayerList from `firstLayerIndex` |
| 2, 3 | `Solid` — colour reference, alpha |
| 4, 5 | `LinearGradient` — colour line, `p0`, `p1`, `p2` |
| 6, 7 | `RadialGradient` — colour line, two centres and radii |
| 8, 9 | `SweepGradient` — colour line, centre, start and end angles |
| 10 | `Glyph` — outline glyph id, child |
| 11 | `ColrGlyph` — base glyph id, the root of that glyph's graph |
| 12, 13 | `Transform` — the six `Affine2x3` values, child |
| 14, 15 | `Translate` — `dx`, `dy`, child |
| 16–19 | `Scale` — `scaleX`, `scaleY`, centre for 18 and 19, child |
| 20–23 | `ScaleUniform` — `scale`, centre for 22 and 23, child |
| 24–27 | `Rotate` — angle, centre for 26 and 27, child |
| 28–31 | `Skew` — x and y angles, centre for 30 and 31, child |
| 32 | `Composite` — source, mode, backdrop |

The variable format of each pair carries its `varIndexBase` as written
(`Some`), the static one `None`; a colour line is a `ColourLine` of its
extend and its stops, a variable line's stops each with their own base. The
deltas are FT-06.20's (*COLR table and OpenType Font Variations*), applied
through FT-05's item variation store.

**What the clause leaves to the reader.** An extend value the *Extend
enumeration* does not define reads as `EXTEND_PAD`, as *Color references,
ColorStop and ColorLine* says, and a `CompositeMode` the enumeration does not
define reads as `COMPOSITE_CLEAR`, as *Format 32: PaintComposite* says, each
with a typed leniency carrying the value. A paint format the clause does not
define, a paint table that runs past the table, a `PaintColrLayers` slice past
the LayerList, a `PaintColrGlyph` naming a glyph with no
`BaseGlyphPaintRecord`, and a child offset of zero — a `PaintGlyph` "on its
own does not add content" (*Filling shapes*) — each read as a
`Paint::Refused` node with its reason and a typed warning: *Color glyphs as a
directed acyclic graph* has "the paint table at which the error occurs …
ignored, that sub-graph … not rendered, and that node … considered to be
visually bounded", and the rest of the graph read. A refused root answers
`None`, because nothing of the glyph is left to draw.

**The limits a hostile graph meets** (ruling 1):

- **Cycles.** `PaintColrLayers` and `PaintColrGlyph` reach paints through
  indices rather than forward offsets, so a graph can name itself; *Color
  glyphs as a directed acyclic graph* requires it acyclic and gives the check:
  paint tables tracked along the path. A paint table is identified by its
  offset in `COLR`, and an edge to a table already on the path is a
  `Paint::Refused(Cycle)` node with a warning naming the glyph — the cycle
  refused by name, the rest drawn.
- **Sharing.** A paint table reached again by another path is the node it
  already is: the arena holds each table once, so a graph that fans one table
  out to many parents costs its tables, not its paths. The writer writes a
  node with more than one parent once, as a form XObject each parent draws,
  so the written bytes are linear in the nodes too.
- **Depth.** `MAX_PAINT_DEPTH`, 64 paints from the root to the deepest leaf.
  A table is read at the depth of the first path that reaches it; an edge
  whose path would pass the limit, by reading a new table or by reaching a
  table already read with its height below it, is `Paint::Refused(TooDeep)`
  with a warning. Every path a drawing recurses down is then at most 64
  long. The deepest graph a real face states is a handful of levels:
  `PaintColrLayers`, `PaintGlyph`, a transform or two, a gradient.
- **Size.** `MAX_PAINT_NODES`, 16 384 nodes a glyph. A table met past it
  is `Paint::Refused(TooLarge)`, warned once. A `PaintColrLayers` slice is at
  most 255 layers (`numLayers` is a `uint8`) and a version 0 slice at most
  65 535, each held to the array it indexes; colour stops are held to their
  table, at least six bytes each, so every vector the reader builds is bounded
  by the table.

### sbix

*Strikes*: each strike is a `ppem`, a `ppi` and `numGlyphs + 1` offsets, the
glyph count from `maxp` (*Table dependencies*); glyph `N`'s data is the bytes
between offsets `N` and `N + 1`, none when they are equal, and the eight-byte
header (*Glyph data*) is two `int16` origin offsets and a graphic tag. Every
offset is checked against the strike's place in the table and the table's
end, and an offset array that runs past the table, or a pair that runs
backwards, answers `None` for the glyphs it would place.

- **Graphic types.** `png `, `jpg ` and `tiff` are handed back as bytes with
  their type, undecoded (decoding is the drawing step's,
  `tinker_pdf_filters::png_decode` among them). `mask` and `pdf `, which
  *Glyph data* names as "not supported in the OpenType specification", and
  every other tag are refused by name: `SbixRefusal::GraphicType(tag)`.
- **`dupe`.** The data is a big-endian glyph id whose bitmap "must be used for
  the current glyph" (*Glyph data*). It is resolved once, in the same strike,
  and the glyph answers with the named glyph's graphic and **the named
  glyph's** origin offsets, since the bitmap is placed as its own record
  places it; a `dupe` naming a `dupe` — the clause states no chain — is
  refused (`SbixRefusal::DupeChain`), and so is one naming a glyph past the
  count or with no data (`SbixRefusal::DupeTarget`).
- **Origin offsets.** *Glyph data* places the graphic's left and bottom edges
  `originOffsetX` and `originOffsetY` "units" from the glyph's design origin
  without saying whether a unit is a font unit or one of the strike's pixels.
  This design reads them as **the strike's pixels**: the graphic they place is
  measured in that strike's pixels, `ppem` of them to the em, and the offsets
  of one glyph differ from strike to strike only if they scale with it.
  FT-06.15's fixture uses offsets whose two readings land apart.
- **Outlines.** Bit 1 of `flags` asks for the outline drawn over the bitmap
  (*Header*); `draws_outlines()` answers it, and the writer honours it,
  filling the outline in the text's colour over the bitmap. *Glyph data*'s
  placement rule — the graphic's origin at the drawing position for a glyph
  with no contour, at the outline's `(xMin, yMin)` for one with contours — is
  the writer's, which has the outline.

### CBLC and CBDT

`CBLC` is `EBLC`'s layout at version 3 with `bitDepth` 32 added (*CblcHeader*,
*BitmapSize record*); its *IndexSubtableList* holds, per strike,
`IndexSubtableRecord`s of a glyph range and an offset from the list to an
index subtable. All five *IndexSubtable formats* are read: 1 and 3 an offset
per glyph of the range plus one (`lastGlyphIndex − firstGlyphIndex + 2`),
four-byte and two-byte; 2 one `imageSize` and one `BigGlyphMetrics` for the
whole range; 4 `numGlyphs + 1` `GlyphIdOffsetPair`s for a sparse range; 5 a
size, metrics and a sorted glyph array. A glyph's data is at
`imageDataOffset` plus its offset in `CBDT`, sized by the next offset (1, 3,
4) or by `imageSize` (2, 5); equal offsets are "a data size of zero", no
bitmap. Every count is multiplied out with checked arithmetic and held to
the record's range, the list's `indexSubtableListSize`, the table's end, and
— for the data — `CBDT`'s end.

*Glyph bitmap data formats*: format 17 is `SmallGlyphMetrics`, a `dataLen` and
PNG bytes; 18 is `BigGlyphMetrics` and the same; 19 is the length and bytes
alone, its metrics the index subtable's `bigMetrics` — so format 19 under an
index format that carries none (1, 3, 4) is refused, `CbdtRefusal::Malformed`,
as data whose length runs past its glyph's slot is. Small metrics carry
one direction, which the strike's `flags` names (*Bitmap flags*); a glyph comes
back with its strike's `ppemX` and `ppemY`, its `height`, `width`, bearings and
advance in pixels, and for big metrics the vertical three as well.

**The formats CBDT inherits.** *Uncompressed color bitmaps* admits every
`EBDT` data format at `bitDepth` 32 as premultiplied BGRA, and `EBDT`'s
component formats 8 and 9 assemble a glyph from others. Until FT-06.21 reads
them, a glyph in any of formats 1 to 9 is refused by name,
`CbdtRefusal::DataFormat(n)`.

### The strike a glyph is drawn at

*Strikes* in `sbix` makes a bitmap binding — "if there is bitmap data for a
glyph in *any* strike, then the glyph must be drawn using a bitmap from
*some* strike" — and leaves the choice to "the closest available larger
size, or the closest available integer-multiple larger size, or … some other
basis"; `CBDT`'s *Scaling behavior* recommends "the closest sized bitmap that
is larger than the desired end raster size". `StrikeChoice` is the writer's
parameter, and every option chooses among the strikes **that hold the
glyph** (a `dupe` holds it), comparing `sbix`'s `ppem` and `CBLC`'s `ppemY`,
the em's height:

- `Largest` — the most pixels per em.
- `AtLeast(ppem)` — the fewest pixels per em of at least `ppem`; the largest
  where none reaches it.
- `Multiple(ppem)` — the fewest pixels per em that are a whole multiple of
  `ppem`; `AtLeast(ppem)`'s strike where none is.
- `Nearest(ppem)` — the least distance from `ppem`, the larger on a tie.

Strikes of equal size are taken in table order, the first winning, so the
choice is a function of the table. Which option a book embeds at is H-141
(b); a `DocumentBuilder` caller names it.

### The SVG table

*SVG table formats*: a document index of glyph ranges, each with an offset
from the list and a length. A document whose first three bytes are 0x1F 0x8B
0x08 is gzip-encoded (*Document encoding and format*) and is inflated through
FT-06.7's RFC 1952 reader; the inflated document is capped at
`MAX_SVG_DOCUMENT`, 16 MiB, and one past the cap is refused by name, not cut
— a cut document is a different picture. The element is the one whose id is
`glyph` followed by the glyph id in decimal (*Glyph Identifiers*), drawn "as
though the given element and its content were specified in a `<defs>` tag and
then referenced", which is FT-06.9's read by id with what it references.

**The transform to the em.** *Coordinate systems and glyph metrics*: SVG's
user units are font units, its origin the glyph's origin, its y axis
downward; the initial viewport is the em square, `unitsPerEm` on a side, and a
`viewBox` or `width` and `height` on the root scale the user space within it;
the viewport is never clipped. So a document point `(x, y)` is the glyph-space
point the root's viewport mapping gives, with `y` negated:
`[1 0 0 −1 0 0]` composed after `tinker_pdf_svg::transform::view_box` from
the root's `viewBox` onto `0 0 unitsPerEm unitsPerEm`. The elements *SVG
capability requirements and restrictions* forbids are not drawn, and the
scene names each it passes over, as it names them in a book: the read is
handed the chapter's list as the elements its caller restricts, the chosen
palette as `var(--colorN)`'s values, and `currentColor` with no `color` set
left to the caller, which the scene writer draws by setting no colour. The
procedure is the book's scene writer's operators (`epub/svg.rs`), the one
place a scene becomes operators, under the flip; its form's box is the
scene's ink, strokes widened by their reach, since the chapter records no
glyph box and leaves the viewport unclipped. An `<image>` is a `data:` URL's
PNG or JPEG, the only source a document inside a font names.

### Which table draws a glyph

A face can carry several. Each glyph is drawn by the first of these that
describes it, in this order:

1. `COLR` version 1 — its graph, where `paint_graph` answers.
2. `COLR` version 0 — its layers, where `layers` answers.
3. `SVG ` — its element, where the document holds it.
4. `sbix`, then `CBDT` — the strike the `StrikeChoice` picks, where one holds
   the glyph.
5. Its outline, in the text's colour, as a plain glyph.

Vectors come before bitmaps because a vector glyph is resolution independent
in a PDF that will be zoomed and printed; version 1 before version 0 is
*Mixing version 0 and version 1 formats*' preference; a glyph whose reading
fails at one step falls to the next. `sbix` before `CBDT` is a fixed order for
a face carrying both, so the choice is a function of the face.

### The PDF object

A colour face is written as **Type 3 fonts** (ISO 32000-2 9.6.4), one per
face, palette and `StrikeChoice`, as many as its codes need (a simple font has
256 codes, of which the writer gives 255, never 32, which 9.3.3 applies the
word spacing to; FT-06.10's run writer spills into the next). `/FontMatrix` is
`[0.001 0 0 0.001 0 0]`, which a file states exactly, and each glyph's form
takes the face's units into that glyph space by its `/Matrix`,
`1000/unitsPerEm`, so the form is written in font units: `1/unitsPerEm`
itself, a real written to six decimals, is `0.000488` for an em of 2048, and
would scale every glyph and every advance by its error. `/Widths` are
`hmtx`'s advances in those thousandths, which COLR's *Metrics and
boundedness* and the `SVG ` chapter's *Glyph semantics* keep the colour
glyph's; `/ToUnicode` maps each code to the text the run recorded for it,
which FT-06.2 makes Type 3 text extract and search as a CID run's does.

Each glyph procedure is `wx 0 d0` — 9.6.4's Table 111: "the glyph description
specifies both its shape and its colour" — and then draws a **form XObject
that is an isolated, non-knockout transparency group** (11.6.6, Table 145;
11.4.5), whose `/BBox` is the glyph's clip box or the bounds the writer
computes. The group is what COLR's *rendering algorithm* starts from, "an
initial drawing surface", and what makes the glyph one object to the text:
the graphics state's alpha, blend mode and soft mask apply to the group's
result as a whole (11.6.6), so translucent text fades a colour glyph without
its lower layers showing through its upper ones. A glyph written twice is one
form drawn twice. A glyph whose paints need no transparency is drawn through a
form with no `/Group` when a profile forbids transparency (below), which is
the same picture wherever the text's alpha is one, and a glyph that draws
nothing is its width alone.

**Where the form is named.** 9.6.5 has a reader find a procedure's names in
its font's `/Resources`, and this engine's renderer runs a procedure in the
scope that shows the text instead (`docs/design/icc.md`, the scopes). So a
glyph's form takes a name of the document's own — the face's resource name,
`.g` and the glyph's id, `C0.g12` — the first time a run draws the glyph,
and is named under it in its font's `/Resources` and in those of every page,
form and pattern registered after that run, the scopes a run can be shown
in. A glyph's own form is written under the document's resources less the
other glyphs' forms, which it never draws. A document with no colour face
names no such form.

### Each paint's construct

| Paint | Construct | Exact |
| --- | --- | --- |
| Version 0 layer; `PaintColrLayers` | Each layer drawn in order, bottom first, `Normal` blend: W3C Compositing Level 1 5.1's simple alpha compositing, which COLR's *Compositing and blending* names, is ISO 32000-2 11.3.3's formula with `B = Normal` | yes |
| `PaintSolid` | The clip region filled in the palette colour as `rg`, its alpha — the CPAL entry's alpha over 255 times the paint's — as `/ca` (11.6.4.4); a foreground fill sets no colour | yes |
| `PaintGlyph` | The outline as a clipping path in font units, `W n` under the nonzero rule `glyf` and `CFF ` outlines are filled by, then the child | yes |
| `PaintColrGlyph` | The named glyph's graph, drawn in place: "a continuation of the graph" (*Re-use using PaintColrGlyph*), so the named glyph's own clip box does not apply | yes |
| Transform family | `cm` with the matrix: `Affine2x3`'s `xx yx xy yy dx dy` as `a b c d e f`; a rotation by `θ` `[cos θ, sin θ, −sin θ, cos θ, 0, 0]`; a skew `[1, tan ψ, −tan φ, 1, 0, 0]` (*Formats 24 to 27*, *Formats 28 to 31*); each around-centre form between a translation to the centre and its inverse. Angles reach the matrix through `tinker_pdf_svg::transform`'s `tinker-pdf-math` helpers (ruling 4) | yes |
| `PaintLinearGradient` | An axial shading (8.7.4.5.3) from `p0` to `p1` projected onto the perpendicular to `p0p2` through `p0`, whose colour is constant along lines parallel to `p0p2`, as *Linear gradients* places it; its axis stretched across the clip box, so every point of the region has its own position on the line | yes |
| `PaintRadialGradient` | A radial shading (8.7.4.5.4), the same two-circle family *Radial gradients* defines, its two circles taken where the family reaches every point of the clip box — one unit past the stops for a padded line, the family's own triangle-inequality bound for one that repeats or reflects, never past a zero radius | yes |
| `PaintSweepGradient` | A function-based shading (8.7.4.5.2) over the clip box, its calculator taking the angle with `atan` and running the colour line from the start angle to the end, counter-clockwise, inverted when the start exceeds the end (*Sweep gradients*); a padded sweep over no angle, which the clause leaves unstated, is the first stop short of the angle and the last from it on, the limit of an end closing on its start counter-clockwise | yes |
| Colour line | A calculator function (7.10.5): the extend first — `EXTEND_PAD` the first stop below the first offset and the last at and past the last, so that of two stops at one offset the first stands below it and the second at it, and `EXTEND_REPEAT` and `EXTEND_REFLECT` folded into the line's interval by the fold `epub/svg.rs` folds a CSS ramp's period with — then the stops by the same bisection, each segment interpolating **linear-light, premultiplied** components and re-encoding them as sRGB in the calculator — the `CPAL` chapter's *Interpolation of colors*, transcribed; a stop's alpha below one makes a luminosity soft mask (11.6.5.1, `/S /Luminosity`, `/BC [0]`) whose group paints the same geometry in `DeviceGray` with the alphas as its ramp | yes |
| A colour line naming the foreground | Refused by name, the glyph drawn as its next table: a glyph's procedure is shared by every run that shows it and cannot state the colour a run is shown in, and interpolating that colour in linear light has no construct that leaves it unstated | no |
| `PaintComposite`, modes that mix no colour | `CLEAR` draws nothing; `SRC` the source and `DEST` the backdrop, drawn where the composite stands, since the result is painted source-over onto what is below it and source-over is associative; `SRC_IN` the source, in an isolated group of its own so that it is masked as one object, under an `/S /Alpha` soft mask of the backdrop's group, `DEST_IN` the reverse; `SRC_OUT` and `DEST_OUT` the same with `/TR` the function `1 − x` (Table 142) | yes |
| `PaintComposite`, `SRC_OVER`, `DEST_OVER` and the fifteen blend modes | `SRC_OVER` the backdrop then the source, `DEST_OVER` the two the other way round, where the composite stands; a blend mode an isolated group holding the backdrop and then the source's own isolated group under the `/BM` of the same name (11.3.5's Tables 134 and 135 are W3C Compositing Level 1's sections 10.1 and 10.2), so that the source blends with the backdrop alone and as one object | in the page's encoded components; see H-141 (d) |
| `PaintComposite`, `SRC_ATOP`, `DEST_ATOP`, `XOR`, `PLUS` | Not expressible: each sums two terms (W3C Compositing Level 1 9.1.10 to 9.1.13), and 11.3.3's formula only composites one over the other. Drawn source-over under a typed warning naming the mode until H-141 (c) rules | no |

**What this design does not write exactly**, the list FT-06.17a prices:

- **(c), inexpressible**: `COMPOSITE_SRC_ATOP`, `COMPOSITE_DEST_ATOP`,
  `COMPOSITE_XOR` and `COMPOSITE_PLUS`. Source-over agrees with the first
  three wherever an alpha is zero or one, and differs where both are
  fractional; nothing in 11.3 adds two results. H-141 (c) chooses between an
  image of the glyph — at FT-06.17a's candidate resolutions, 64, 128, 256 and
  512 pixels to the em — and source-over with the warning kept.
- **(d), the colour space of a composite**: *Format 32: PaintComposite* does
  its arithmetic "after applying the inverse transfer function to derive
  linear-light RGB values". ISO 32000-2 11.3.4 admits `CalRGB` as a blending
  colour space, so a composite's group with `/CS` a `CalRGB` of unit gamma and
  sRGB's primaries says exactly that; but this engine's renderer reduces a
  three-component group's space to its sRGB-encoded components
  (`crates/tinker-pdf/src/resources.rs`'s `group_space`), so it would not draw
  that construct as written, and an eight-bit linear buffer loses up to six
  levels in the darkest tones. Every other composition inside a colour glyph
  — layers, by *Compositing and blending*'s simple alpha compositing, which
  names no space — is in the page's encoded components. Until H-141 (d)
  rules, composites are written there too, and FT-06.14's formulas are
  evaluated over encoded components; FT-06.17a measures the largest
  difference against the linear-light reading for each mode that mixes
  colour.

**Boundedness.** COLR's *Metrics and boundedness of color glyphs using version
1 formats*: a glyph with a clip box is bounded and is clipped to it; one
without is drawn only if its graph is bounded — a fill under a `PaintGlyph`,
and a composite as the clause's list for its mode says — and an unbounded
glyph without a clip box is drawn as its outline with a typed warning.

**Profiles that forbid transparency.** ISO 19005-1 6.4 forbids a part 1
document any transparency, and `DocumentBuilder` refuses each parameter that
introduces some with `ArchivalRefusal::Transparency`. Under such a profile a
glyph whose graph needs a group, a soft mask, an alpha below one or a blend
mode other than `Normal` is refused by name and written as its outline; one
that needs none — opaque solids, layers, transforms, clips and opaque
gradients — is drawn through its form with no `/Group`.

### Bitmaps in a PDF

A strike's graphic becomes an image XObject drawn in the procedure, through
the one door every PNG and TIFF a document embeds goes through
(`tinker_pdf_cos::png_image`, `tiff_image`): a PNG with an alpha channel as
its colour channels and an `/SMask` of its alpha (Table 87, 11.6.5.2), one
whose `tRNS` names one colour or one run of indices passed through under the
`/Mask` that range test is (8.9.6.4), one with neither passed through as
`ImageData::Compressed` where K-03.9's rule admits it — the inflate verified
once, and a PNG whose data does not match its header decoded instead — a
`jpg ` passed through as `DCTDecode`, and a `tiff` placed or decoded as its
strips allow; a graphic decodes to at most `MAX_BITMAP_BYTES`, 64 MiB.
`CBDT`'s
*Compressed color bitmaps* says a glyph's PNG is sRGB whatever its chunks say,
so its samples are written as `DeviceRGB`, as every RGB colour is. The image
is placed by the strike's metrics scaled into the em by `unitsPerEm / ppem`:
`sbix` by its origin offsets from the origin *Glyph data* names, the graphic's
own pixels its size; `CBDT` by its bearings from the pen (`horiBearingY` to
the top edge), its metrics' `width` and `height` its size, `ppemX` across and
`ppemY` up. A `CBDT` glyph whose metrics carry no horizontal bearings, which a
horizontal run cannot place, falls to the glyph's next table and is named, as
is a graphic its decoder refuses. A bitmap used by several glyphs or several
runs — the same bytes, a `dupe` among them — is written once. Under a profile
that forbids transparency a graphic that needs an `/SMask` falls to the next
table and is named; one that needs none is drawn.

### How tests compare pixels

Every pixel test in FT-06.11 to FT-06.20 renders the written PDF with this
engine and compares, at sample points, with a colour the test computes from
the table's own definition — never another program's output (ruling 13).
A sample point stands at least two device pixels from every edge the test's
own geometry puts near it, so its coverage is whole; and each fixture stacks
at most two translucent paints over any sample point.

- **Solid fills, layers, clips, transforms, composites and SVG shapes**: each
  of R, G and B within **2** of 255 of the expected colour — one level for the
  eight-bit rounding of each translucent composite the point sees.
- **Gradients**, COLR's and SVG's: within **3** — ruling 22's one level for a
  shading the renderer steps or reads from a table, and the two above.
- **Bitmaps**: within **2**, at the centre of a source pixel whose eight
  neighbours share its colour, the image drawn at a whole number of device
  pixels to its pixel, against the pixel this engine's decoder gives for the
  graphic.

## Milestones

1. **FT-06.2, Type 3 text.** A `Device` method reporting each Type 3 glyph
   with its code, text, transform and advance; the text device implements it,
   every drawing device leaves it a no-op. Exit: `type3_fonts.rs`
   `a_glyph_procedures_text_extracts_and_is_found` and
   `a_procedure_that_shows_text_extracts_it_once`; `render_goldens.rs`'s
   `type3-glyph.ppm`, the page's `to_svg` and its `display_list` unchanged, and
   no unreadable font reported.
2. **FT-06.3, `CPAL` and `COLR` version 0.** Exit: `colour_tables.rs`
   `colr_v0_layers_come_in_order_with_their_palette_colours`,
   `cpal_v1_types_and_labels_read_as_written`,
   `the_foreground_index_is_the_text_colour` and
   `out_of_range_indices_and_overrunning_counts_answer_none`; `cargo xtask
   fuzz` green with `colr-v0.ttf`.
3. **FT-06.4, the version 1 graph.** Exit: `colour_tables.rs`
   `every_paint_format_reads_to_its_node`, `a_clip_box_is_read_for_its_glyph_range`,
   `a_colr_glyph_cycle_is_refused_by_name`,
   `a_graph_past_its_depth_stops_with_a_warning` and
   `a_colour_line_overrunning_its_table_answers_none`; `cargo xtask fuzz` green
   with `colr-v1.ttf`.
4. **FT-06.5, `sbix`.** Exit: `colour_tables.rs`
   `sbix_strikes_and_glyph_data_read_as_written`,
   `a_dupe_resolves_once_and_a_chain_is_refused`,
   `a_glyph_with_no_data_has_no_bitmap` and `each_strike_option_picks_its_strike`;
   `cargo xtask fuzz` green with `sbix.ttf`.
5. **FT-06.6, `CBLC` and `CBDT`.** Exit: `colour_tables.rs`
   `cbdt_glyphs_read_through_every_index_format`,
   `every_png_data_format_reads_its_metrics` and
   `a_glyph_range_overrunning_its_subtable_answers_none`; `cargo xtask fuzz`
   green with `cbdt.ttf`.
6. **FT-06.7, gzip.** Exit: `gzip.rs`
   `a_member_inflates_with_its_crc_and_size_checked`,
   `every_optional_header_field_is_skipped_as_its_flag_says`,
   `a_member_with_a_wrong_crc_or_size_is_refused` and
   `a_member_past_its_limit_is_refused`.
7. **FT-06.8, the `SVG ` index.** Exit: `colour_tables.rs`
   `svg_documents_are_found_by_glyph_range`,
   `a_gzipped_document_inflates_and_one_past_the_cap_is_refused` and
   `an_index_overrunning_its_table_answers_none`; `cargo xtask fuzz` green with
   `svg.ttf`.
8. **FT-06.9, an element by id.** Exit: `tinker-pdf-svg`'s `tests.rs`
   `an_element_is_read_by_id_with_what_it_references`,
   `an_id_naming_nothing_is_a_refusal` and `a_use_cycle_through_an_id_is_refused`.
9. **FT-06.10, the Type 3 writer and its run.** Exit: `colour_fonts.rs`
   `a_colour_glyph_run_round_trips_its_text_and_positions`,
   `the_written_colour_font_is_structurally_clean`, and a run past one font's
   codes spilling and round-tripping.
10. **FT-06.11, version 0 drawn.** Exit: `colour_fonts.rs`
    `colr_v0_layers_draw_their_palette_colours`.
11. **FT-06.12, version 1 solids, glyphs and transforms.** Exit:
    `colour_fonts.rs` `colr_v1_solids_glyphs_and_transforms_draw_by_the_formulas`
    and `a_composite_is_named_until_it_is_drawn`.
12. **FT-06.13, gradients.** `DocumentBuilder`'s `Shading` gains the
    function-based type a sweep needs. Exit: `colour_fonts.rs`
    `colr_v1_gradients_match_the_specification_formulas`.
13. **FT-06.14, composites.** A soft mask's `/TR` joins `StateMask::Group`.
    Exit: `colour_fonts.rs` `colr_v1_composites_match_their_formulas`, the
    rows above marked exact matching their formulas over encoded components
    and the inexpressible rows carrying the warning that names the mode.
14. **FT-06.15, bitmaps drawn.** Exit: `colour_fonts.rs`
    `bitmap_glyphs_draw_their_strike_where_its_metrics_put_it`, for each table
    and each `StrikeChoice`; `jpg ` and `tiff` glyphs alike; a bitmap used twice
    written once.
15. **FT-06.16, SVG glyphs drawn, and the fingerprint.** Exit:
    `colour_fonts.rs` `svg_glyphs_draw_their_document`; `determinism.rs`'s
    `GOLDEN` gaining a page of version 1, `sbix` and SVG glyphs that
    `rendering_is_stable_across_targets` reproduces.
16. **FT-06.17a and FT-06.17b, the price.** Exit: `colour_fonts.rs`
    `colour_glyph_costs`, and the bench operations `write a page of colour
    glyphs` and `render a page of colour glyphs at 150 dpi`, measured in the
    timing phase.
17. **FT-06.18, what H-141 rules for (c) and (d).** Exit: `colour_fonts.rs`
    `colr_v1_composites_match_their_formulas`'s remaining rows, by the formula
    at the image's resolution or within the bound recorded with the ruling.
18. **FT-06.19, books and Markdown pages.** Exit: `epub_fonts.rs`
    `a_books_colour_face_draws_its_emoji_in_colour` and `markdown.rs`
    `emoji_in_a_markdown_page_draw_in_colour`.
19. **FT-06.20, variable COLR.** Exit: `colour_tables.rs`
    `variable_paint_values_take_their_deltas` and `colour_fonts.rs`
    `a_variable_colour_glyph_draws_at_its_instance`.
20. **FT-06.21, `CBDT`'s inherited formats.** Exit: `colour_tables.rs`
    `every_inherited_cbdt_data_format_reads_its_pixels` and `colour_fonts.rs`
    `an_uncompressed_cbdt_glyph_draws_its_pixels`.

## Risks

| Risk | Mitigation |
| --- | --- |
| `sbix` offsets are font units in some face, not pixels | The clause leaves the unit unstated; FT-06.15's fixture puts the two readings apart, and FT-06.17a's census reads the committed books' faces for one that carries `sbix` |
| A real graph passes a limit | 64 levels and 16 384 nodes are far past any graph the chapter's examples need; each refusal is a typed warning naming the glyph, so a face that meets one says so |
| A graph that fans one table out to many parents writes exponentially many bytes | Each table is one node, and a node with several parents is one form XObject |
| Composites in encoded components differ from the linear-light formula | H-141 (d) with FT-06.17a's figures; the construct that is exact in PDF is named above |
| A strike choice embeds megabytes of bitmaps | The strike is the caller's parameter; H-141 (b) rules a book's |
| A colour glyph's text is lost | FT-06.2 lands first; FT-06.10's round trip extracts every glyph's text at its position |
| A hostile `SVG ` document inflates without bound | `MAX_SVG_DOCUMENT`, refused by name past it, under FT-06.7's limits |
| A transcendental creeps into a written matrix | Rotations and skews go through `tinker-pdf-math` (ruling 4); FT-06.16's fingerprint holds the bytes on four targets |
