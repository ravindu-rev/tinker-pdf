# Fonts

Every font format a PDF can carry is parsed in `crates/tinker-pdf-font` —
bytes in, metrics and outlines out, no PDF types on its API (ruling 8,
[rulings](../rulings.md)) — and the facade binds those programs to font
dictionaries, encodings and CMaps. The engine reads no font directories, by
policy: that is an operating-system dependency `wasm32-unknown-unknown` does
not have. It bundles no font programs **by default** either — a face is a
licensing decision, and the host that has one supplies it — but the
`bundled-fonts` feature carries twelve Liberation faces for the host that has
none. A document that embeds its fonts needs nothing else; one that does not
draws no text without a host-supplied [`FontProvider`](#api), and the gap is
reported, never silent.

## What it does

**TrueType** (9.6.6, 9.9). `Sfnt` parses the table directory and `glyf.rs`
reads outlines from `glyf`/`loca`, including the implied on-curve midpoints
between consecutive off-curve points. Composite glyphs are followed, bounded
two ways: a nesting depth of 8 and a total budget of 256 components, because a
depth cap alone leaves a 64-component self-referencing glyph asking for 64^8
outlines (ruling 1). Character lookup goes through the font's `cmap`,
including the 9.6.6.4 route for symbolic fonts whose (3,0) subtable maps codes
into the private-use block. A symbolic font asks its `cmap` for the code
first and for the character its `/ToUnicode` names after, a non-symbolic font
the other way round: a symbolic font's table is keyed by its codes, and an
office suite's subset numbers its glyphs in the order the document used them,
so its space can be code 3 while U+0020, read as a code of the same table,
reaches a letter. The validators read a glyph's advance through the same
selection, so such a font's widths are judged against the glyphs it draws.
Hinting is deliberately not interpreted; outlines render unhinted with
anti-aliasing.

**CFF / Type 2** (9.6.6). `Cff` parses the INDEX and DICT structures and runs
Type 2 charstrings — implicit width on the first stack-clearing operator,
`endchar` among them, where `seac`'s four operands are not a width and a
fifth before them is (Type 2 charstring specification, appendix C),
`hintmask` operand counting (hints are parsed exactly far enough to skip),
local and global subroutines with the count-dependent bias, which is resolved
once per INDEX when the face is parsed. A caller reading many glyphs keeps one
`cff::Machine` and one `Outline` and runs each glyph through them
(`Cff::outline_into`): the operand stack and the segment buffer are allocated
once, while the stack is cleared and the operation budget, the subroutine
depth cap and the transient array start afresh at every glyph, so a glyph
draws the same segments whichever glyphs came before it; `Cff::outline` is
that call with a fresh pair. `Cff::advance` runs a charstring only as far as
the operator that settles its width and builds no outline. Glyph selection is
the charset's job, never the character code's: all three written charset
formats (0, 1, 2) plus the three predefined ones (ISOAdobe, Expert,
ExpertSubset), names resolved through the 391 standard strings and the font's
own string INDEX, and the font's built-in encoding with its supplements. A
CID-keyed font (`ROS` in the Top DICT) selects glyphs by CID through the
charset, with `FDSelect` routing each glyph to its own Font DICT's Private
DICT, local subroutines and — where one is stated — font matrix
(`Cff::font_matrix_for` is per glyph for exactly this reason).

**OpenType/CFF** (the `OTTO` tag). `Sfnt::parse` accepts an `OTTO` face, and
a face with no `glyf` routes its `CFF ` table into the CFF path. Without that
routing such a font would parse, find no `glyf`, and draw every glyph as an
empty outline that reads as a legitimate space.

**Vertical metrics** (the `vhea`, `vmtx` and `VORG` clauses).
`Sfnt::vertical_advance` and `Sfnt::top_side_bearing` read `vmtx` the way
`hmtx` is read: `vhea`'s `numOfLongVerMetrics` long entries, kept from the
parse as `hhea`'s count is, the last advance repeating past them, and a side
bearing each for the glyphs after them from the array that follows, up to
`maxp`'s glyph count. `Sfnt::vorg_origin` reads `VORG`'s record for a glyph
or its default, its count bounded by what the table holds and its records
bisected where they are in order and scanned as written where they are not.
`Sfnt::vertical_origin` is the `vmtx` clause's: the top side bearing plus the
top of the glyph's box — the `yMax` of its `glyf` header (`glyf::bounds`) for
TrueType outlines, beside which a `VORG` is ignored as its own clause
requires; for CFF outlines `VORG`'s origin, or without one the top of the
outline the charstring draws, control points included. A caller asking for
many glyphs makes one `Sfnt::vertical_origins` view, which decides the source
once and parses the `CFF ` table once where it is needed; `Sfnt` itself keeps
nothing that borrows its bytes, so a face stays covariant in its lifetime. A
glyph with no outline is bounded at its origin, so its origin is its side
bearing; a `CFF2` face, which the CFF path does not read, has `VORG`'s origin
or none. A face without the tables, a count of zero, or a
read past a table's end answers `None`, and what a shaper sets then is
[design/shaping.md](../design/shaping.md)'s *Vertical shaping*.

**Colour tables** (OpenType 1.9.1's CPAL and COLR chapters,
[design](../design/colour-fonts.md)). `Cpal` reads both versions of the
palette table: every palette's entries as `Rgba` — sRGB, not premultiplied,
the table's blue-green-red-alpha order named by field — and version 1's
palette types (both flags, a reserved bit kept), palette labels and entry
labels as `name` IDs, 0xFFFF reading as none. `Cpal::choose` takes a
`PaletteChoice` — palette 0, an index, or the first palette flagged for a
light or a dark background — and always answers one of the table's
palettes, or 0. `Colr` reads version 0's `BaseGlyph` and layer records, in a
version 0 or version 1 header: `Colr::layers` answers a glyph's layers bottom
first, each outline glyph with its palette entry's colour or, for 0xFFFF,
`LayerColour::Foreground`, the text's own colour; `Colr::layer_records` the
same before the palette is consulted. The records are bisected where they are
in increasing glyph order and scanned, the first record answering, where they
are not, with `ColrWarning::BaseGlyphsOutOfOrder`. Every offset and count is
held to the table: a palette or entry past its count, a record past
`numColorRecords` or the table's end, a layer slice past `numLayerRecords`,
and a layer naming an entry the palette lacks each answer `None` — the glyph
is then drawn as its outline — and a header that does not fit, or a version
other than 0 or 1, is no table. Nothing here draws a colour glyph; a PDF's
embedded program is drawn from its outlines (ISO 32000-2 9.9.1).

Version 1's paint graph: `Colr::paint_graph` reads a glyph's
`BaseGlyphPaintRecord` into a `PaintGraph`, an arena with one `Paint` node a
paint table, however many parents reach it. All thirty-two formats read, each
field as written — `F2DOT14`s and `Fixed`s as their integers, with `f2dot14`
and `fixed` to divide them exactly, and a variable format's `varIndexBase` as
`Some` — and each `ColorLine` or `VarColorLine` with its extend and stops;
`Colr::clip_box` reads the `ClipList`'s format 1 and 2 boxes by glyph range.
What the chapter calls not well formed reads as a `Paint::Refused` node with a
typed `ColrRefusal`, the rest of the graph read: a table reached again on its
own path (`Cycle`, through a `PaintColrLayers` slice, a `PaintColrGlyph` or an
offset of zero), a path past `MAX_PAINT_DEPTH` (64) paints (`TooDeep`), a graph
past `MAX_PAINT_NODES` (16 384) nodes (`TooLarge`), a format the chapter does
not define, a table that runs past `COLR` or names no child, and a
`PaintColrGlyph` naming a glyph with no record. A refused root answers `None`.
An extend value or a composite mode the enumerations do not define reads as
`EXTEND_PAD` or `COMPOSITE_CLEAR`, as the chapter says, each warned with its
value; records out of order are scanned and warned, as version 0's are.

Bitmap strikes (the sbix chapter). `Sbix::parse` takes the table and the
face's glyph count — `Sfnt::glyph_count`, `maxp`'s — and reads every strike's
`ppem`, `ppi` and `numGlyphs + 1` offsets, refusing the table where any of
them runs past it. `Sbix::glyph` answers a glyph's bitmap in a strike: its
origin offsets, its graphic type (`png `, `jpg ` or `tiff`) and its bytes,
undecoded; no data where its offsets are equal; a `dupe` resolved once, in
the same strike, to the named glyph's record, which `SbixGlyph::from` names.
What it refuses it names (`SbixRefusal`): a graphic type the chapter does not
define — `mask` and `pdf ` among them — a `dupe` of a `dupe`, a `dupe` naming
a glyph past the count or with no data, and data that runs backwards, past
the table or short of its header. `Sbix::draws_outlines` reads bit 1 of the
flags. `Sbix::choose_strike` picks the strike a `StrikeChoice` names among
those holding the glyph: `Largest`, `AtLeast(ppem)` (the smallest at least
that large, else the largest), `Multiple(ppem)` (the smallest whole multiple,
else `AtLeast`'s) and `Nearest(ppem)` (the larger on a tie), strikes of one
size taken in table order.

Colour bitmap strikes in `CBLC` and `CBDT` (their chapters, over EBLC's
layout). `ColourBitmaps::parse` takes both tables, each at major version 3,
and reads every `BitmapSize` record: `ppemX`, `ppemY`, the bit depth, the
flags and the glyph range. `ColourBitmaps::glyph` finds the index subtable
whose range holds a glyph and reads it in whichever of the five index formats
it is — four- and two-byte offsets per glyph (1, 3), one size and one set of
metrics for a range (2), sparse offsets (4), and sparse glyphs of one size
(5) — then the glyph's data in `CBDT`'s format 17, 18 or 19: the PNG,
undecoded, with its strike's pixels per em and its `BitmapMetrics` in pixels —
small metrics for the direction the strike's flags name, big metrics for
both, format 19's from the index subtable. Every index entry is held to its
list's `indexSubtableListSize` and to `CBLC`, and every glyph's data to its
slot and to `CBDT`; what does not fit, offsets that run backwards and format
19 under an index format with no metrics are `CbdtRefusal::Malformed`, an
index format the chapter does not define is `IndexFormat`, and the formats
CBDT inherits from EBDT — 1 to 9, uncompressed and component bitmaps — are
`DataFormat`, refused by name (the table below).
`ColourBitmaps::choose_strike` takes the same `StrikeChoice` as `sbix`,
comparing `ppemY`.

**Type 1** (9.9, `/FontFile`). `Type1::parse` reads PFB, PFA and the bare
bytes a PDF embeds: eexec decryption, the second decryption layer around each
charstring with the font's own `/lenIV`, and Type 1 charstrings — `hsbw`'s
side bearing, `seac` accent composition, and `flex` through the
`callothersubr`/`pop` callback protocol. Glyphs are addressed by name, via
the encoding the font dictionary specifies or the program's built-in one.

**Type 3** (9.6.5). A Type 3 glyph is a content stream, not an outline: it
can fill, stroke, draw images. The content interpreter fetches the procedure
named by `/Encoding /Differences` out of `/CharProcs` and runs it recursively
under the font's `/FontMatrix` — which is required, because a Type 3 font
chooses its own glyph-space units rather than the 1/1000 convention. A
resource scope reads the font's `/Differences` once, on its first glyph, into
a table of each code's glyph name — the first name an array gives a code is
the one it keeps, every item resolved, an integer or a real setting the code
as the interpreter always read it — and keeps it beside `/CharProcs` and the
matrix, so a glyph is one probe of the table and one lookup of its
procedure. What the glyph stands for — its `/ToUnicode` text, or the
character its `/Differences` name gives — is extracted and searched where the
glyph stands, as a shown glyph's is: the interpreter reports the glyph to the
text device once its procedure has run, and a procedure that showed text of
its own is read by that text alone ([content and
text](content-and-text.md)).

**Composite fonts** (9.7.4). The CID the encoding CMap produces — not the
code — selects both the glyph and the advance. The advance is `/W`'s, read in
both of 9.7.4.3's forms, `c [w1 … wn]` and `c_first c_last w`, mixed freely
and each entry from its own start, and `/DW`'s (1000 when absent) for a CID no
entry names; a malformed entry ends the array, and every entry before it
stands. For a CIDFontType2,
`/CIDToGIDMap` in both its forms (`/Identity`, or a stream of two-byte GIDs,
9.7.4.2) turns CID into glyph index; the font program's own `cmap` is not
consulted on this path, because a composite font's code is not a character.
For a CIDFontType0 the CFF charset answers. A CID the font does not carry
draws `.notdef` **and** reports — glyph 0 is usually blank, and without the
report the failure looks like nothing happened.

**CMaps** (9.7.5, 9.10.3). One scanner reads both encoding CMaps and
`/ToUnicode`. Codespace ranges are matched **per byte** as 9.7.6.2 requires —
`90ms-RKSJ-H`'s `<8140> <9FFC>` is one interval per byte position, not one
integer interval — and an undefined code takes its byte length from its lead
byte (9.7.6.3), so a damaged string stays in step. Differential CMaps are
followed through both spellings, the `usecmap` operator and the stream
dictionary's `/UseCMap` (Table 120): the child's mappings stay in front of
the parent's wherever they overlap, the chain is capped at
`MAX_USECMAP_DEPTH` (4) links, and a CMap that uses itself — directly or
around a chain — is refused by source comparison, not by name. All 202 CMaps
of Adobe's registry (9.7.5.2) are vendored
([THIRDPARTY](../../THIRDPARTY.md)) and compiled by `tinker-pdf-font`'s
`build.rs` into delta-encoded, deflated tables behind the `cmap-predefined`
feature, default on; with it off, the codespaces still ship so strings split
correctly, and `CMap::is_approximate` says the CIDs are guesses. A name
outside the registry answers `None` and is warned about, never approximated
silently.

**Vertical writing** (9.7.4.3). `/W2` in both of Table 117's forms and
`/DW2` with its `[880 -1000]` default supply the displacement and the
position vector, keyed by the same CID that selects the glyph; the position
vector places the glyph within its em box and the advance moves the pen down
by 9.4.4's `ty` (with `Tz` correctly not applied to a vertical advance).
`/WMode` and the `-V` CMap suffix both set the mode, and 9.7.5.3's rule that
the child's `/WMode` wins over an inherited one is kept.

**Standard-14 metrics** (9.6.2.2, Annex D.5). A simple font naming one of the
standard faces may omit `/Widths`, so `Standard14` carries Adobe's AFM
advances for the printable ASCII range exactly, with the aliases every viewer
honours (Arial for Helvetica, TimesNewRoman for Times). Above 0x7F a width is
approximated from the glyph's base letter, and the approximation is reported
by the flag `Standard14::advance` returns beside the width, never silently.

**The symbolic fonts' built-in encodings** (Annex D.5, D.6). Symbol and
ZapfDingbats each have an encoding of their own, and `BuiltInEncoding` holds
both as Annex D tabulates them: Table D.5's 189 Symbol codes and Table D.6's
188 ZapfDingbats codes, each with the glyph name its table gives it and the
Unicode value the Adobe Glyph List — or, for ZapfDingbats' `a` names, the ITC
Zapf Dingbats Glyph List — gives the name, and the fourteen names that list
holds and Table D.6 does not encode
([design](../design/symbol-faces.md); the sources are in
[THIRDPARTY.md](../../THIRDPARTY.md)). `substitute_glyph` is the route from
one of their codes to a substitute face's glyph: the face's own glyph names
first — `Sfnt::glyph_for_name`, which reads every version of `post` over the
258 standard Macintosh names in the order Apple's TrueType Reference Manual
gives, or a CFF charset — then the name's Unicode value through the face's
Unicode subtable (`Sfnt::glyph_for_unicode`), then the code through a (3,0)
subtable at `0xF000` (`Sfnt::glyph_for_symbol_code`), glyph 0 being no answer
at any step. The facade does not ask it: a substituted Symbol or ZapfDingbats
face is read by its `cmap` alone (*The host seam*, below).
`crates/tinker-pdf-font/tests/symbol_census.rs` counts, for H-94, what each
candidate face the design names reaches by it.

**The host seam.** `FontRequest::read` distils a font dictionary and its
descriptor (9.8.2 Table 123 flags, the 9.6.4 subset tag stripped, weight and
slope read from the name too because plenty of producers set no flags) into a
request a `FontProvider` answers with a TrueType or bare CFF program — or
declines. `SimpleFontProvider` covers the common case: up to four faces
chosen by weight and slope, falling back to whatever is set, and declining
symbolic fonts unless `substituting_symbolic()` was called, because a text
face standing in for a symbol font draws confidently wrong glyphs. A
composite font's request also carries its **character collection**,
`FontRequest::character_collection` — the descendant's `/CIDSystemInfo`
(9.7.3) as `tinker-pdf-cos`'s `Font::character_collection` reads it,
leniently as rendering reads a font rather than as PDF/A judges one: a
registry and an ordering each a string or a name, and a supplement where it
is an integer from zero up — so a provider can choose a face by the
characters the font's CIDs stand for (`docs/design/fallback-faces.md`). A
simple font's request, and one whose descendant names no collection
readably, carries `None`; `SimpleFontProvider` ignores the field.
`CharacterCollection` is re-exported from the facade.

**A substituted face is read by character** (`docs/design/fallback-faces.md`).
A face a provider substitutes numbers its glyphs in an order of its own, so
the facade keeps whether a font's program was embedded or substituted, and a
substituted one is never read by the document's glyph numbers. A composite
font's code reaches a glyph only through the CID its encoding CMap maps it
to — a code the CMap maps to none, or a build without the predefined tables,
reaches nothing — by the first of four routes that reaches one: the face's
own charset, where the face is a CID-keyed CFF whose `ROS` is the font's
collection (`Cff::registry_ordering`); the collection's `Uni*-UTF32` CMaps
inverted once a process per collection and writing mode
(`tinker_pdf_font::cmap::collection_character`), to a character and the
face's `cmap`, a radical or a compatibility ideograph tried after the other
characters reaching the same CID, and in the face's `vrt2` or `vert` form
where the font writes vertically or the CID is a vertical variant;
`/ToUnicode`, where it names one character (`Font::to_unicode_text`); and
otherwise `.notdef` with `UnreadableFont`. So an `Adobe-Identity` font with
no `/ToUnicode` draws `.notdef` and reports even where the host's face is the
document's own (ruling 23). A simple font's code takes the face's `cmap` by
its character and by the code, symbolic fonts code first, and a code neither
reaches draws `.notdef` and reports; a CFF-flavoured face is read by the names
the encoding gives, as an embedded CFF is. A face with no `cmap` and no `ROS`
— the synthetic face the corpus measures with, the test faces — has no
characters to be read by and keeps the reading an embedded program gets.
Subsetting asks about embedded programs only.
`crates/tinker-pdf-font/tests/cjk_coverage.rs` counts, for H-116 and H-04,
the CIDs of each collection and supplement that each candidate face the
design names reaches by the first two routes, in either writing mode, and
what each candidate costs to store and to make ready.

**Writing** (9.9). `DocumentBuilder::add_embedded_font` and `add_cid_font`
embed a TrueType, an `OpenType/CFF` face or a bare CFF program, with widths
taken from the program's own `hmtx` — or, for a bare CFF, from its charstrings
through its `FontMatrix`, which need not be the usual 1/1000. Each gets the
descriptor entry 9.9 Table 126 gives it: `/FontFile2`, `/FontFile3
/Subtype /OpenType`, or `/FontFile3 /Subtype /Type1C` (`/CIDFontType0C` under a
composite font).

Under a composite font the **descendant** is chosen by the outlines, not by
the wrapper: a TrueType program is a CIDFontType2 with `/CIDToGIDMap
/Identity`, and a CFF — bare, or the `CFF ` table of an `OpenType/CFF` face —
is a CIDFontType0, with no `/CIDToGIDMap`, because 9.7.4.1 makes a
CIDFontType0 the CFF-based one and Table 126's note on `/Subtype /OpenType`
admits that wrapper under a CIDFontType2 only when it carries `glyf`
(`a_composite_font_over_a_cff_face_is_subsetted`).

A **CID-keyed** CFF — bare, or in an OpenType wrapper — goes down the
composite path too. 9.7.4.2 reads a CIDFontType0's CID through the program's
charset when the program is CID-keyed, and `/Identity-H` makes the code the
CID, so the code a glyph is written as is **the CID its charset gives it**:
`add_cid_font` reads the charset once (`Cff::cid_for_gid`), and
`PageBuilder::glyphs` and `DocumentBuilder::glyph_run` write each glyph's CID
rather than its index. `/W` and `/ToUnicode` are keyed by that code and
measured at the glyph it selects, and a run's pen advances by the same width,
so the `TJ` adjustments and `/W` still cannot disagree. A glyph past the end of
the font goes out as CID 0, `.notdef`, rather than as its own number, which
the charset may give to a glyph that exists. A charset that is **not
one-to-one** — two glyphs claiming one CID, a glyph after `.notdef` claiming
CID 0, a charset short of the glyph count — is refused whole, because a glyph
whose CID another glyph also claims is one no code reaches
(`a_cid_keyed_cff_in_an_opentype_wrapper_writes_each_glyph_s_cid`,
`a_cid_keyed_program_the_writer_embedded_draws_the_glyph_it_was_given`).

`set_subset_fonts` (on by default) cuts each program down to the glyphs the
pages drew, and **glyph identifiers are never renumbered** — which is what
lets `/Widths`, `/W`, `cmap`, `/CIDToGIDMap` and `/ToUnicode` stay as written.
For TrueType that means `glyf`/`loca` rebuilt with composite closures followed
and a dropped glyph left as a zero-length `loca` entry. For CFF it means the
CharStrings INDEX, both subroutine INDEXes, the Private DICTs and the Top DICT
rebuilt together, with a dropped glyph left as a single `endchar`; the charset,
the encoding, the String INDEX and `FDSelect` are copied through byte for byte,
because nothing moved and re-encoding them would only be a chance to map a
glyph to the wrong name. A `callsubr` operand is recomputed from the new index
and the **new bias** rather than adjusted, since subsetting can move an INDEX
across the 1 240 and 33 900 thresholds. A global subroutine reached from two
Font DICTs is written twice, because the local subroutines it calls are the
caller's. The subsetter checks its own output: `verify` outlines every
retained glyph in both programs and compares the segments, and a subset that
disagrees is not used.

The 9.6.4 six-letter tag is prefixed to `/BaseFont`. A face that is not cut
down is embedded whole — larger and correct (ruling 2) — and
`DocumentBuilder::finish_reporting` returns an `EmbeddedWhole` naming the
resource and one of three `SubsetRefusal` reasons: the font claimed none of the
text drawn with it, the program could not be rebuilt, or the rebuild came out
no smaller than the face. That last one is common: a producer-made subset has
nothing left to remove.

**Rewriting** (9.6.4, 9.9). The paragraph above is the *builder*, which knows
every glyph it placed because it placed them. A rewrite of somebody else's
document knows nothing of the kind, so `tinker_pdf::subset::apply` runs the
same `tinker_pdf_font::subset` over a glyph set the interpreter collects —
pages, form XObjects at any depth, Type 3 glyph procedures and every state of
every annotation `/AP` — and leaves whole, by name, any program it cannot
bound. The encoding needs no repair because glyph identifiers are never
renumbered; only `/BaseFont`, the descendant's `/BaseFont` and the
descriptor's `/FontName` move, all three to the same 9.6.4 name, from the same
`subset_tag` this page's builder uses. A Type 3 font, which has no program, has
the procedures nothing shown runs emptied in place instead, its dictionary
untouched. It is an editing operation and lives with the rest of them: see
[editing](editing.md).

**And it is arranged rather than asked for.** `tinker_pdf::write::save`, the
facade's save door, runs the pass by default — `SaveOptions::fonts` is a
`FontPolicy` whose default is `Subset` — so the common case does not depend on
a caller remembering. `DocumentEditor::save` is `tinker-pdf-cos`'s: it writes
every program through as it arrived, and it carries no font switch **and must
not**: the pass is driven by the interpreter, which sits above that crate, so a
flag there would be one the crate carrying it cannot act on.
[writing](writing.md) argues the boundary; the part that belongs on this page
is what it buys — a redacted document's embedded face does not keep the
removed letters' outlines unless somebody asked it to.

**A collection is rebuilt as the face it was read as.** `Sfnt::parse` takes a
`ttcf` by its first member, since a PDF embedding a collection has no way to
say which member it means, and `subset` declares `Sfnt::version`, the
directory the tables actually came from, rather than the file's leading tag: a
single flat table directory declaring `ttcf` is one this crate and every other
refuses to read. Nothing in this repository writes a collection, so the corpus
census (`crates/tinker-pdf/tests/cff_subset_census.rs`) is what reaches one.

**WOFF 1.0 and WOFF 2.0** ([W3C REC 2012], [W3C REC 2024]). `woff.rs` unpacks
both to the sfnt inside them; nothing else in the crate knows they exist, and
`Sfnt::parse` is what reads what comes out. The two are not variations on each
other and the code does not pretend they are.

WOFF 1.0 is a **repackaging**: each table zlib-compressed on its own, the
directory recording the original length and the original checksum. Undoing it
is inflate — `tinker-pdf-filters`' own, not a second one — plus a directory
rebuilt in ascending tag order, which §5 requires in as many words. The tables
themselves go back in the **physical order the container recorded**, which is
the only surviving record of the order the original font had them in, and is
the difference between reproducing the producer's file and producing an
equivalent one. `origChecksum` is verified for every table even though §5 puts
that on the producer: it is the one end-to-end integrity check either container
carries, it costs one pass over bytes already in cache, and a face that fails
it is one this engine would rather decline by name. `head` is checksummed with
`checkSumAdjustment` taken as zero, which is the sfnt rule neither WOFF
restates and which every producer follows because it copies the value out of a
real directory.

WOFF 2.0 is a **re-encoding**. One Brotli stream carries every table
concatenated; `glyf` is taken apart into seven substreams and its contours
re-encoded as triplets; `loca` is not stored at all and falls out of the `glyf`
reconstruction; `hmtx` may have had its left side bearings deleted on the
grounds that they equal the glyph bounding boxes. All of that is reversed here,
along with §3.1's two variable-width integer codings — `255UInt16`, whose
encoding is deliberately not unique, and `UIntBase128`, whose two forbidden
spellings are both refused — §4.1's table of 63 known tags, and §4.2's font
collections. §5 says the result "may produce binary results that are different
from the original data", so byte identity is not the property claimed for it.

**Every transform the Recommendation defines is reversed**, against the text
of the Recommendation of 8 August 2024 ([W3C REC 2024]). Clause 5 defines three
transforms: `glyf` version 0 (§5.1, the `overlapSimpleBitmap` included),
`loca` version 0 (§5.3) and `hmtx` version 1 (§5.4). §4.1 makes version 3 the
null transform for `glyf` and `loca` and version 0 the null transform for
everything else. That is the whole of `transform_kind`. Any other pair —
`glyf` 1 or 2, `hmtx` 2 or 3, any version but 0 for any other table — is one
§4.1 answers itself: "If a decoder encounters a table entry that specifies an
unknown transformation version number the entire font MUST be rejected". So
`WoffError::UnknownTransform` is that rejection and not a transform this
build lacks. §5.3's "both glyf and loca tables must either be present in their
transformed format or with null transform applied to both tables" is kept as
well: a transformed `glyf` beside a null-transformed `loca`, whose offsets
index the `glyf` the encoder was handed and not the one §5.1 rebuilds, or
beside no `loca` at all, is refused by name
(`a_transformed_glyf_needs_its_loca_transformed_with_it`).

Both are bounded by a caller-supplied ceiling that is **not advisory**: a WOFF2
directory states its lengths in `UIntBase128`, which reaches 2^32 − 1 in five
bytes, so a forty-byte file can ask for four gigabytes. Metadata and private
data blocks are located and skipped, never parsed — the metadata block is XML,
and reading it would give this crate an opinion about markup that ruling 8 says
it may not have.

[W3C REC 2012]: https://www.w3.org/TR/WOFF/
[W3C REC 2024]: https://www.w3.org/TR/WOFF2/

**Variable fonts** (OpenType 1.9.1; [design](../design/variable-fonts.md)).
`tinker_pdf_font::variation` reads a face's design space.
`DesignSpace::read` takes `fvar`'s axes — tag, minimum, default, maximum,
flags and name — and its named instances, each record read `axisSize` or
`instanceSize` from the last so that a longer record is a compatible
extension, and `avar`'s segment maps, a record out of order ignored and an
axis whose maps lack −1→−1, 0→0 and 1→1 left unmodified, as the `avar`
chapter has it. `DesignSpace::location` turns an `Instance` — a named
instance's index, or user coordinates by axis tag, the last value of a tag
winning and an unknown tag ignored — into a `Location`: one normalised 2.14
coordinate an axis, computed as the overview's 'Coordinate Scales and
Normalization' requires. The user value is clamped to its axis, the default
normalisation and `avar` run in 16.16 with every quotient and product rounded
to the nearest 1/65 536 and a half rounding up, and the result becomes 2.14 by
adding 2 and shifting right by 2. The module is integer throughout
(`#![deny(clippy::float_arithmetic)]`), so the shaper can call it; an `f64`
user value reaches 16.16 through `Fixed::from_f64`, the overview's own
conversion. A face with no `fvar`, or one whose axis count is zero, has no
axes, and every coordinate asked of it is the default. A hostile table is
refused by name (`VariationError`): more than 64 axes, an axis record under
twenty bytes, axes, named instances or a segment map past the table's end, an
instance record too small for its coordinates, an axis whose default lies
outside its range, an `avar` whose axis count is not `fvar`'s, and a version
neither chapter defines.

`variation::tuple` reads the tuple variation store `gvar` and `cvar` share:
each table's peak, embedded or one of `gvar`'s shared tuples, its
intermediate start and end, its private or the store's shared point numbers —
a first byte of zero meaning every item — and its packed deltas, one stream
of zeros, bytes and words. A table's scalar is the product of its axes'
scalars in `fvar`'s order, each a 16.16 value the overview's pseudo-code
gives — one for an axis whose peak is zero or whose region is not valid for
it, zero outside its start and end — and each product rounded as the
normalisation rounds. `variation::cvar::cvt_at` gives a face's `cvt ` values
at a `Location` in 16.16: each FWORD plus every table's scalar times its delta
for that index, an index past the `cvt ` moving nothing; a face with no
`cvar`, or one that does not vary, keeps its values. Every table's deltas are
decoded wherever its scalar is zero too, so a store is refused or read the
same at every location. Refused by name: a header or a table's data past the
store, a shared tuple index past the shared tuples — every index in `cvar`,
which must embed its peaks — a table with no point numbers of its own in a
store with none shared, and point numbers or deltas that end early, run past
their count or add up past 65 535. The varied `cvt ` reaches no page by
itself.

`variation::item_store` reads the item variation store — the region list and
its item variation data subtables, rows of 8- and 16-bit deltas or, under
`LONG_WORDS`, 16- and 32-bit ones, a NULL offset and the index 0xFFFF/0xFFFF
meaning no variation — and the delta-set index maps of formats 0 and 1, an
item past the count taking the last entry. `region_scalars` evaluates every
region once at a location, and `delta` sums one row exactly in 16.16, so the
shaper's `GDEF` variations and `CFF2`'s blends read the same store under the
same integers. `variation::metrics` holds its three clients, each at a
`Location`: `Hvar` — the advance through its map or, with none, the glyph id
as the inner index of the first subtable, and the side bearings through
theirs; `Vvar` — the advance height, the side bearings and the vertical
origin `VORG` records; and `Mvar` — a value record by tag, its delta added to
the field `METRIC_FIELDS` names in `OS/2`, `hhea`, `vhea`, `post` or `gasp`,
a tag no record names constant. `Sfnt::left_side_bearing` gives the default
the left side-bearing delta applies to, beside `Sfnt::advance`, and the
vertical deltas apply to `Sfnt::vertical_advance` and `Sfnt::vorg_origin`. Refused by name: a store of another format, a header,
region list, subtable or map past its bytes, 32 768 regions or more, a region
or delta-set index past what it indexes, more long deltas than columns, a map
with no entries or an outer index past sixteen bits, a map of format 1 in
`HVAR` or `VVAR`, which use format 0 only, a table with no store, an `MVAR`
record under eight bytes, and a region list whose axis count is not `fvar`'s.

**CFF2** (OpenType 1.9.1's CFF2 chapter). `Cff2::parse` reads the five-byte
header, the Top DICT that is not in an INDEX, the Global Subr INDEX after it,
the CharStrings and Font DICT INDEXes — INDEXes counting in 32 bits — each
Font DICT's Private DICT, measured from the table, and its local subroutines,
measured from the Private DICT, FDSelect in formats 0, 3 and 4, a table with
none putting every glyph in Font DICT 0, and the variation store behind its
16-bit length. CFF2 DICTs are read by the same tokeniser as CFF's with the
operators running to 24, so that `vsindex`, `blend` and
`VariationStoreOffset` are operators, and a stack of 513. The charstrings run
through `cff.rs`'s one Type 2 machine, parameterised by flavour: no width, no
`endchar` and no `return` — a charstring and a subroutine end where their
data does — no `put`, `get` or arithmetic, and `vsindex` and `blend`; stems
and hint masks count as CFF's do, so `cff_subset.rs`'s count still agrees,
and a CFF charstring runs exactly as before. `Cff2::outline` is the default
instance: every `blend` — `n` defaults, `n·k` deltas and `n`, `k` the region
count of the item variation data the `vsindex` in force selects, the
glyph's own or its Private DICT's — keeps its defaults, and so does
`Cff2::private_dict`'s reading of a Private DICT. A CFF2 program has no
charset, encoding or glyph names. Each distinct Private DICT is read once,
and the distinct ones with the local subroutines each reaches may not read
more bytes than the table holds. Refused by name (`Cff2Error`), where CFF's
reader stops silently: a version other than 2, a header, Top DICT, INDEX or
Private DICT past the table, an INDEX of more than 65 536 entries before its
offsets are read, Private DICTs past that budget, an FDSelect of another
format, cut short, out of order or short of the glyphs, a glyph naming a
Font DICT that is not there, a stack past 513, a `blend` short of its
operands or with a fractional `n`, subroutines nested past ten, the 64 000
operation budget, `vsindex` after a `blend`, twice, without an operand or
past the store's subtables, and `blend` or `vsindex` in a table with no
store.

At an instance, `Cff2::blends_at` gathers the store's region scalars at a
`Location` — the design's 16.16 values — for each item variation data
subtable in its column order, once, and `Cff2::outline_at`, or
`Cff2::outline_with` over blends a caller keeps, draws a glyph through them:
each `blend` adds to its defaults their deltas times the scalars of the
subtable the `vsindex` in force selects, in region order, rounded to 16.16,
so a blended operand is again one a Type 2 charstring can carry; for integer
deltas that is the numeric model's exact sum. `Cff2::private_dict_at` reads
a Private DICT's blends the same way. `Cff2::blends_in` holds the store's
axis count to a design space's, refusing a mismatch by name. At the default
location every scalar is zero, so a face whose regions all peak away from
zero draws there exactly what `Cff2::outline` draws.

## API

Everything reaches callers through the facade (ruling 11). Reading:
`Document::open`, then `Document::with_fonts` (or `OpenOptions::with_fonts`
via `Document::open_with`, which for reflowable formats arrives early enough
to affect pagination) installs a provider; `Page::render` reports
`RenderWarning::UnreadableFont` on `Bitmap::warnings` when glyphs went
undrawn, and `Page::text` — which needs no outlines and consults no provider
— reports `TextWarning::UnknownFont` and `TextWarning::UnmappedCode` on
`TextPage::warnings`. Parse-time CMap leniencies land on
`Document::warnings()` as typed `WarningKind` values when a font is read — by
a page's render or text the first time its content selects the font, so a
font a page names and never uses raises none of them there. Writing:
`DocumentBuilder::add_base_font`, `add_named_font`, `add_embedded_font`,
`add_cid_font`, `set_subset_fonts`, and `PageBuilder::text` or
`PageBuilder::glyphs` (glyphs addressed by index, for a composite font).

`tinker-pdf-font`'s parsers each have a counted sibling that returns its
uncounted sibling's answer and the steps it took
([resource exhaustion](../design/resource-exhaustion.md), "The counters"), a
count in the crate's own vocabulary that a caller can add to an operation's
tally: `Sfnt::parse_counted` (a table record read, a record compared) and
`Sfnt::glyph_for_char_counted` (an encoding record weighed and the subtable's
order checked, at a face's first lookup; an entry examined, at each);
`glyf::outline_counted` (a glyph appended, a component read, each contour end,
flag, coordinate and emitted point); `Cff::parse_counted` (each INDEX offset,
charset, encoding and FDSelect entry and Font DICT) and `Cff::outline_counted`
(each operand and operator the Type 2 machine read, its subroutines' included);
`Type1::parse_counted` (each byte of the `eexec` section and of each
charstring decrypted, each byte of the private section walked) and
`Type1::outline_counted` (each operand and operator run, both glyphs of a
`seac` included); `woff::decode_counted` (each directory entry, each byte the
tables decompress to and each byte of the font assembled); and for a CMap,
`cmap::parse_counted` and `parse_embedded_counted` (each token, every CMap of
the chain's), `CMap::decode_codes_counted` (each codespace range a code is
tried against), `CMap::to_unicode_counted` and `CMap::cid_counted` (each
range a lookup examined).

```rust
use std::sync::Arc;
use tinker_pdf::{Document, SimpleFontProvider};

let face = std::fs::read("DejaVuSans.ttf")?;
let doc = Document::open(bytes)?
    .with_fonts(Arc::new(SimpleFontProvider::new(face)));
// A document that embeds nothing now draws text; without the provider it
// would extract perfectly and render none of it, reporting UnreadableFont.
```

## Listing what a document carries

`Document::fonts()` answers "which faces is this file made of" without
rendering anything. Each `DocumentFont` carries the `/Subtype` family
(`FontKind`), `/BaseFont` exactly as the file spelled it, that name with
9.6.4's subset tag stripped, the tag itself, whether a program is embedded and
under which `/FontFile*` key (`ProgramKey`), and the resource names the font
answered to. A font a scope's graphics state names by reference (8.4.5's
`/Font [font size]`) is listed too, since an interpreter binds it as `Tf`
binds a name; its resource names are the ones `/Font` gives it, none for a
font only a state names. `tpdf fonts <file>` prints that list; `tpdf fonts <file> --out
DIR` additionally writes each embedded program out, naming each file from the
program's own first bytes rather than from the descriptor key, since
`/FontFile3` holds a bare CFF or an OpenType wrapper and the listing does not
read the stream's `/Subtype` to tell them apart.

**The bytes are lazy and the listing is not a parse.** `DocumentFont` holds
the *address* of the program; `program_bytes()` decodes the stream, and
nothing else does. The listing reads `/Subtype`, `/BaseFont` and the
descriptor and never a CMap — which is what keeps it cheap, and also what
keeps it a *read*: `cos::font::read` absorbs its leniencies into the
document's warnings, so a listing built on it would make `warnings()` depend
on whether anyone had asked for the fonts first. A test asserts the warning
count is unchanged across a call.

Each face appears once however many pages and resource names reach it, and a
composite font is one entry rather than two: 9.7.4 makes the descendant
CIDFont part of the Type 0 font, so listing both would report every CJK face
twice. Reachability is the pages' `/Resources` plus every scope their content
can enter — form XObjects (8.10.1), tiling patterns (8.7.3), Type 3 glyph
procedures (9.6.5), every state of every annotation appearance (12.5.5) — and
the form's `/DR` (12.7.3.3).

**`/DR` is part of reachability.** A variable-text field's `/DA` names its font
in `/DR`, which no page has to mention: in
`verapdf/Isartor test files/PDFA-1b/6.9 Interactive Forms/isartor-6-9-t01-fail-a.pdf`
the page's whole `/Resources` is a `/ProcSet`, and the one embedded face in
the file is reachable only that way.

### The corpus census

`crates/tinker-pdf/tests/annotation_census.rs`'s font half, `#[ignore]`d and
run with `-- --ignored --nocapture`, lists the fonts of every fetched corpus
file. A file that names `/FontFile` somewhere in its raw bytes and yields no
embedded program is printed by name rather than asserted about, because the
bytes can be in an unreferenced object, on a page outside the tree, or in a
descriptor key whose value is not a stream — the byte scan is a hint, not a
claim.

A `/FontFile3` that resolves to nothing — `null` written as the descriptor's
value outright, or an object that is itself `null` — is not an embedded
program. veraPDF's font-embedding fixtures state the expectation in their
outlines, and it is ours: `6-2-11-4-1-t01-fail-a.pdf` says *"Type1 font that
is used for rendering is not embedded"*, `6-2-10-4-1-t01-pass-a.pdf` says the
same with *"the text rendering mode is 3"* after it, and `is_embedded()`
answers `false` for the face in both.

## What the missing faces cost

This engine bundles no font programs by default, so a document that names
Helvetica and embeds nothing extracts its text perfectly and draws none of it.
The corpus run keeps that policy's cost apart from the engine's own defects
with three bars, in three files, and `corpus-run` refuses to compare any of
them against another: `corpus/ratchet.json` with no faces;
`corpus/ratchet-fonts.json` with the face `cargo xtask synth-face` writes,
whose every glyph from 32 up is a filled box, so it answers *was a face
available* and nothing else; and `corpus/ratchet-bundled.json` with the
bundled faces. The synthetic face exists so the measurement can be reproduced
anywhere: no licence, no download, the same bytes on every machine, and no
dependence on the fonts a machine happens to have installed.

The bundled bar counts a symbolic font the engine could not read — an
embedded face with `/Flags` bit 3 set — as undrawn, where the synthetic face
answers it with squares. That is the bundled set being right: it declines a
symbolic font rather than drawing letters in its place. Neither bar says the
text was *set* correctly; both say whether a glyph was drawn at all.

The metamorphic relations that rewrite a document reopen the rewritten copy
with the same provider: a comparison between two renders is only a comparison
if both are made under the same conditions.

9.6.2.2 requires the base 14 to be available, so without a face a conforming
file that names one and embeds nothing cannot be drawn — a conformance gap
rather than only a policy. The `bundled-fonts` feature below is the answer.

## The bundled faces

`bundled-fonts`, **off by default**, embeds the twelve Liberation faces —
Sans, Serif and Mono, four styles each. They are metric-compatible with Arial,
Times New Roman and Courier New, which are in turn what every reader
substitutes for Helvetica, Times and Courier, so the advance widths match what
the document's own `/Widths` array already says. That is twelve of the standard
14; Symbol and ZapfDingbats are the other two and are declined rather than
approximated.

They stay declined because **no face carries their glyphs and metrics under a
licence `deny.toml` admits**. The gate allows OFL-1.1 and the permissive
licences, and the faces built to stand in for these two — URW's Standard
Symbols PS and D050000L, from the base-35 set Ghostscript ships — are
AGPL-3.0 with a font exception. Liberation, the OFL
family that answers the other twelve, has neither repertoire. A text face is
not a fallback either: it puts letters where the document meant arrows and
check marks. A host holding a face it may use supplies it through
`FontProvider`, which answers before the bundled set does.

```toml
tinker-pdf = { version = "0.0.1", features = ["bundled-fonts"] }
```

Three things about the shape of it:

- **The bundled set is asked last.** A host provider is consulted first and the
  bundled faces answer only what it declines — per *request*, not per document,
  so a host with a face for the body text and none for the monospaced code
  sample gets both right. Turning the feature on can therefore only add
  answers; it cannot take one away.
- **Symbolic fonts are declined by name as well as by flag.** A base-14 font
  dictionary carries no `/FontDescriptor`, so `/Flags` is zero and the symbolic
  bit is absent for exactly the two faces that need it. Without the name check
  a `/BaseFont /Symbol` page would render its text as Latin letters — legible,
  plausible and wrong.
- **The family comes from the name before the flags**, for the same reason:
  producers set the serif bit wrongly all the time, and a document that embeds
  nothing names `Helvetica`, `Times-Roman` or `Courier` exactly. The names nest
  — `sans-serif` contains `serif`, `DejaVu Sans Mono` contains `sans` — so the
  narrowest claim is tested first, and each of those sentences is a test.

Off by default because a desktop application, a server with a font package or a
web page with a face already loaded all have better faces than these and a way
to hand them over, and none of them should carry ours. `FontProvider` remains
the seam either way. Provenance and the OFL text are in
[THIRDPARTY.md](../../THIRDPARTY.md).

## Shaping, and what it is allowed to claim

`crates/tinker-pdf-shape` is a leaf crate: face bytes and text in, positioned
glyph runs out, in integer font design units, with no PDF or CSS vocabulary on
its API. Nothing in `tinker-pdf-render` calls it and nothing ever will — see
the first row of "Refused by name" below. Its design is
[design/shaping.md](../design/shaping.md).

What it does:

- **OpenType Layout.** `GDEF`, `GSUB` types 1–8 and `GPOS` types 1–9,
  coverage and class definitions, extension and chaining-context lookups.
- **The default shaper.** `cmap` through `tinker_pdf_font::Sfnt`, plus `cmap`
  format 14 read here because a variation selector is consumed by a shaper and
  never reaches a renderer; script itemization; `locl`/`ccmp`/`rlig`/`liga`/
  `clig`/`calt`, then `kern`/`dist`/`curs`/`mark`/`mkmk`; a cluster on every
  glyph that is a byte offset into the caller's own text.
- **UAX #9.** Level resolution per paragraph, bracket pairs, mirroring, and
  L1/L2 per *line*, as a function the caller applies after breaking — because
  only the caller knows where a line ends.
- **Cursive joining.** `Joining_Type` from the UCD, the four forms from the
  Unicode Standard's own rule, and a feature mask per glyph so `init` reaches
  the first letter of a word and no other. Seven `GSUB` stages instead of one,
  because a face's `medi` lookup is written expecting `init` not to have run.
- **The Universal Shaping Engine, in part.** `Indic_Syllabic_Category` and
  `Indic_Positional_Category`, a syllable per Brahmic cluster, every `GSUB`
  feature applied inside one syllable and never across two, USE's feature
  stages, the canonical decomposition of a Brahmic character that has one, and
  one reordering pause that moves a pre-base glyph to the front of its
  syllable — over the glyphs, through a category each one carries from the
  character it came from, so a conjunct formed before the pause is still
  reordered. A `ZWJ` or `ZWNJ` survives the whole of `GSUB` — blocking a
  ligature is the whole of what one is for — and is deleted at the end of it,
  before any advance is filled, because a face may give one an outline and a
  width. `rphf` is offered only where the syllable has a base for the repha to
  sit on, so a word-final `RA` and halant is a dead consonant and not a reph.
  The table below says how much of it is adjudicated.
- **Plans and coverage indexes kept per face.** A caller that keeps a
  `tinker_pdf_shape::Plans` beside a face and makes its shapers with it
  (`Shaper::with_plans`) has each stage's lookups resolved once per
  `(table, script, language, features)` and, for each planned lookup, a bitmap
  of the glyphs each subtable's first coverage holds, built the first time a
  plan names the lookup. The runner still runs every lookup over the whole
  run in list order; at a position no subtable of a lookup holds, it charges
  the operation budget what a probe would have cost and moves on without
  resolving or probing a subtable, so the glyphs, positions and warnings are
  the ones shaping without a `Plans` gives. A lookup whose probe warns
  whatever the glyph has no index, and an index is set aside for a run whose
  `Limits` would make a probe warn (`max_extension_depth`,
  `max_context_length`). What a `Plans` holds and reads is bounded by
  `Limits::max_index_bytes`, past which it falls back to probing; it starts
  again when handed another face's tables; and it counts what it built
  (`plans_built`, `indexes_built`). A shaper with no `Plans` resolves and
  probes as it always has. The engine's own callers keep one per face for as
  long as they keep its bytes: `tinker_pdf::shaping::Run` carries the shaper
  its caller made, so a document build writes every run in a face through
  that face's `Plans`; each of a book's embedded faces keeps its own
  (`EmbeddedFace::plans`), which measuring, painting and SVG text share, and
  `BookCost::shaping_plans` and `shaping_indexes` count what a book's faces
  built; and a form field's shaped appearance keeps one beside the program it
  decoded, for every line of the value.
- **A column's runs.** `shape::itemize_vertical` cuts a vertical paragraph
  into runs of one script, one level and one `Orientation`, by UAX #50's
  `Vertical_Orientation` (UCD 17.0.0's `VerticalOrientation.txt`, compiled
  with its `@missing` default): `U`, `Tu` and `Tr` stand `Upright` and `R`
  `Sideways`, CSS Writing Modes 3 §5.1.2's rule, each grapheme cluster as its
  first character does, or upright where it holds an enclosing mark (UAX #50
  §3.2.1). A line's `itemize` is unchanged and every run of it is
  `Orientation::Horizontal`.
- **A column, shaped.** `Shaper::shape` sets an `Upright` run top to bottom
  (`Direction::TopToBottom`): the plan its text takes along a line, except
  that it never joins — a cursive script set upright takes its isolated forms
  (CSS Writing Modes 3 §5.1.1) — with `vert` added to its last stage, and
  `vrt2` in `vert`'s place where a setting turns it on and the face carries
  it. Each glyph's pen moves down by its `vmtx` advance (`y_advance` is its
  negative, `x_advance` zero) and its offsets put its vertical origin at the
  pen: half its advance width in, and the `vmtx` clause's origin down. A face
  without vertical metrics is set on a synthesized em box — one em down, the
  origin 880/1000 of an em above the baseline — and the run records
  `Warning::VerticalMetricsSynthesized` once. Its `GPOS` features are
  `shape::VERTICAL_GPOS_FEATURES` — `vkrn` in place of `kern`, `valt` beside
  `abvm`, `blwm`, `dist`, `curs`, `mark` and `mkmk` — with `vhal`, `vpal`,
  `vapk` and `vchw` turned on through settings and the registry's
  interactions applied: one of `vhal`, `vpal`, `vchw` turned on withdraws
  `valt`, and `valt` where the face carries it, `vhal` or `vchw` withdraws
  `vkrn` and `vapk` unless a setting puts them back. Down a column a
  `YAdvance` lengthens the advance height and an `XAdvance` is not applied;
  a placement adds to the offset as along a line; a mark meets its base by
  the line's forward arithmetic on both axes and its advance is zeroed; and
  a cursive join closes the gap along the column and makes the anchors meet
  across it. A `Sideways` run is shaped
  along a line, Mongolian's joining included, with `vrtr` added and `vert`
  and `vrt2` kept out whatever the settings, and `ShapedRun::orientation`
  marks it for its consumer to turn 90° clockwise. `ShapedRun::advance` is a
  run's length along its own axis, `Shaper::shape_text_vertical` shapes a
  paragraph as a column at once, and `Shaper::vertical_metrics` reports what
  a glyph stands on.

- **A layout seam, and one path owning a run.** `Shaper` sits beside `Metrics`
  in `crates/tinker-pdf-layout/src/metrics.rs` as plain structs and `f64`, so
  the layout crate gains no dependency; `Metrics::shaper()` is asked once per
  provider, in one place in `flow.rs`, and a run measured through the shaper is
  never also measured through `Metrics::measure`.
  `crates/tinker-pdf-layout/tests/shaper.rs` drives a provider whose `advance`
  **panics**, so a second measurement path is a failure and not a discrepancy.
  `BookMetrics` implements it over a book's own `@font-face` faces.
- **Shaped runs into a document.** `tinker_pdf::shaping` turns a run's clusters
  back into the text each glyph stands for and writes it through
  `DocumentBuilder::glyph_run`. `write_run` draws its text as one line in
  visual order — rule L2 over the bidi runs, a right-to-left run from its last
  cluster to its first. An Arabic string built that way extracts back to
  itself, in reading order, through the `/ToUnicode` the writer wrote, across
  a ligature, and the document is clean under the strict structural validator.

- **Shaped values into a form field.** `tinker_pdf_cos::Font::program` walks
  `/DescendantFonts` → `/FontDescriptor` → `/FontFile2` (or `/FontFile3`, or
  `/FontFile`) and returns the stream's *address*, so `fill.rs`, which reaches
  its font through the AcroForm `/DR`, has an outline to shape against and not
  only a width. Where the `/DA` font is composite, horizontal, and embeds an
  sfnt, a field's value is shaped and written as a `TJ` run in visual order —
  under `/Identity-H`, under an embedded CMap stream, and under a predefined
  registry CMap where this build compiled its table in, because
  `CMap::code_for_cid` inverts the encoding and verifies each candidate
  forwards before answering. Three more fonts shape: a **vertical** CMap,
  shaped as a column by the shaper's vertical plan with every run upright
  and `vrt2` turned on, and written at each CID's own `/W2` displacement with
  what `GPOS` changed — `vkrn` among it — written over that as `TJ` numbers
  down the column and a text matrix across it; a **bare CFF** under a
  `CIDFontType0`, wrapped per line in a
  synthesised sfnt whose `cmap` is the font's `/ToUnicode` read backwards and
  checked forwards (`CMap::code_for_unicode`) and whose `hmtx` is `/W`; and a
  **simple TrueType** font, each glyph written as the lowest byte its encoding
  reaches it by, so `GPOS` reaches the field. Everywhere else the single-byte
  path stands and every character it could not write is named by
  `WarningKind::FieldCharacterUnrepresentable`, against the field's own
  object; a registry CMap in a `cmap-predefined`-off build is refused with
  `WarningKind::PredefinedCMapApproximate` against the field first, so the
  missing table is not mistaken for a missing glyph — see
  [forms.md](forms.md).

- **A paginated Arabic book.** Drawing walks the same segments measurement
  does (`paint::face_runs`, `css-fonts-4` §5.3 resolved **before** shaping,
  because a glyph index means nothing outside its own face), and an embedded
  face's segment is shaped whole, so a paragraph is measured through the
  shaper and drawn through it too — two measurement paths disagreeing is what
  `metrics.rs` warns about. `crates/tinker-pdf/tests/epub_shaped.rs` pins it:
  joined forms, the line drawn from its last letter, a render fingerprint, and
  a page-level assertion that the line was measured the way it is drawn.
  `epub_reftest.rs` carries the EPUB tier's right-to-left pair.

- **Rule L2 is applied at two levels, not one**: inside a face segment, and
  across the segments of a right-to-left run, which are drawn in reverse with
  each keeping the glyph order its own shaping gave it, so a right-to-left
  line whose characters need two faces is not drawn as two left-to-right
  pieces. `epub_shaped.rs` carries a two-face fixture for it and a
  left-to-right control beside it, because a build that reversed every
  multi-face run would pass the first and set every English sentence with a
  fallback character in it backwards. A **standard-14** segment has no sfnt to
  shape against and is written a code at a time: `paint::coded_order` puts a
  segment holding a right-to-left character in L2's order first
  (`epub_fallback.rs`); one holding none is written as typed at any level, a
  right-to-left `inside` marker's `1. ` among them. That leaves the `.,`
  between two right-to-left words drawn as typed and read back reversed, a
  known limit. A letter and the nonspacing marks after it are one unit, the
  marks drawn after the letter in either direction. With no `GPOS` to
  position a mark, the painter states where it stands: a mark has no advance
  (the overflow font measures one at zero, `paint::standard_width`, in layout
  and in its `/Widths`; the Liberation stand-in's have none), takes no
  `letter-spacing` of its own (a letter and its marks are one typographic
  character unit, `css-text-3` §10.2, in layout and every painter alike), and
  is drawn inside its letter's box, its own box ending a millionth of an em
  past where layout measured the letter to end — a stand-in's letter carrying
  a mark is drawn where layout put it, a piece of its own — in one text object
  with the rest of its slice (`PageBuilder::text_pieces`), where ruling 14
  reads it with its letter at every size and spacing [epub.md](epub.md) lists,
  four to a letter as well. The overflow font is written at no word spacing, a
  text object of its own at `0 Tw` and a piece of one as
  `PieceText::UnspacedCodes`: its code 32 is a character of the book's, and
  9.3.3 moves every glyph after a single-byte code 32 by `Tw`, so written at
  the run's word spacing it would draw them a word spacing off where layout
  put them, over the next glyph under `word-spacing` or justification, with
  marks or none ([epub.md](epub.md)). A run that ends on a mark
  is cut past half an em and a millionth of spacing; after a simple font's
  letter alone, past half an em or at it by a last place, and after a
  stand-in's, within half a thousandth of an em of it by its `/W`'s rounding.

  Two classes of limit are named with their arithmetic, measured on synthetic
  books and pinned in [epub.md](epub.md). **A**, at a spacing within a
  thousandth of an em of half an em: there a run boundary is cut or not by the
  last place of a reader's sum and a painter's, marks or none
  (`<p dir="rtl">كَتَبَ كتب …</p>` at `0.5em` reads as its mark-free twin
  does). **B**: a pointed paragraph breaks its lines where the unpointed
  paragraph does, and a line so broken can fall into one of ruling 14's named
  limits, as the unpointed paragraph's does.

  **And a third level: the visual line.** `flow.rs` breaks lines over logical
  text and resolves no levels, so a right-to-left line made of two styled
  spans arrives as two runs laid left to right in the order written.
  `paint::visual_lines` resolves UAX #9 over each visual line's whole text
  after layout and lays its runs out again in L2's order, each at its own
  measured width, so the line's extent and alignment do not move; drawing,
  links and tags all read the one placement. A line is consecutive runs whose
  ends meet on one baseline, which is how `flow.rs` places a line; a line with
  no right-to-left character is not touched. Inside a run, the pieces
  `word-spacing` cuts it into (every justified line but the last) are laid in
  L2's order too, each paying its space where the space is drawn.

- **Shaping across a span.** A styled span is a run of its own, and a run
  shaped alone sees nothing either side of it: a word with a coloured letter
  would be drawn as isolated letters, and a glyph its neighbour positions — a
  mark, the second glyph of a pair — would lose the offset. The painter shapes
  each run against up to eight characters of its logical neighbours where
  they **touch it on its line** and resolve to the same embedded face
  (`Fonts::set_contexts`), and draws only its own glyphs, placed relative to
  the first of them. A context in the other direction, which could put its
  glyphs between the run's own and overprint the neighbour, is not taken:
  such a run is shaped alone.

  **And layout measures each run in that context**, so a context that changes
  an advance — a joined form wider than the isolated one, a pair that kerns —
  is seen by the line breaker rather than left between the run and the next.
  The `Shaper` seam in `tinker-pdf-layout` takes a context
  (`Shaper::shape_in`, with a default that ignores it for every provider
  that has none): each run's painted neighbours on its line, text and face
  request, the line's ends and atomic boxes and generated content stopping
  it, and the slices the line breaker measures between break opportunities
  take the rest of their own span as theirs. `BookMetrics` applies a
  neighbour only where both sides of the boundary resolve to one embedded
  face and shapes the run with up to eight of its characters either side —
  `one_embedded_face` and `CONTEXT_CHARS` are the painter's own — keeping its
  own glyphs. `epub_shaped.rs` holds a kerned pair across a span (`V` 5.4 pt
  after `A`, a 500-unit advance less 200 at 18 pt), a joined form wider than
  the isolated one (12.6 pt each, not 9), the line breaker setting four
  kerned words on a measure the unkerned ones overflow, and a mixed-direction
  line cut and measured in one context; `tinker-pdf-layout/tests/shaper.rs`
  holds the seam with a provider whose answer depends on its neighbour.

  **A run that mixes directions is cut at its line's level boundaries**
  (`paint::split_at_levels`). A run is one element's text on one line, and
  `a ب<span>ح</span>م b` is three runs of which two mix directions. The line's
  whole text is resolved, every run is cut wherever the level changes inside
  it, and each piece takes its share of the run's measured width — the run
  shaped once, as layout shaped it, each glyph's advance to the piece its
  cluster starts in, the last piece taking what the others leave — so the
  line's extent does not move. A character X9 removes takes the level of the
  one before it, so a joiner does not cut its word. A slice shaped with a
  context is shaped in its **own** paragraph direction, so ` b` after an
  Arabic word its line draws before it is not shaped as part of a
  right-to-left paragraph. `epub_shaped.rs` holds the positions against the
  face's `hmtx` and UAX #9's levels worked out by hand.

  **The levels are the paragraph's, not the line's.** A visual line resolved
  as a paragraph of its own would resolve a weak or neutral character at its
  start or end against `sos` or `eos` rather than the strong character on the
  line before or after, and where a line wrapped would change its order:
  `abc (de` in a right-to-left paragraph would draw `de(` on its second line
  where unwrapped it draws `(de`. Layout numbers the bidi paragraph every run
  is set in (`TextRun::paragraph`: a block's inline content up to a forced
  break of `Bidi_Class` `B` — a preserved newline, CR, NEL or U+2029, and not
  a U+2028 line separator, which ends the line and not the paragraph,
  `css-writing-modes-3` §2.4), and the painter resolves each paragraph once
  over every line of it, across pages, and takes each line's levels from
  `Paragraph::line` — X1 to I2 the paragraph's, L1 and L2 the line's
  (`a_wrapped_line_is_ordered_by_its_paragraphs_levels`). The white space a
  line's end hangs is in no run and so not in the resolved text; it is a
  neutral that L1 resets anyway. A line whose runs are not all of one
  paragraph — an inline block's own text touching the line it sits in — is
  resolved by itself.

  **The paragraph's level is the block's `direction`, and an inline box's
  `unicode-bidi` opens one** (`css-writing-modes-3` §2), so a left-to-right
  paragraph whose line begins with an Arabic word is not laid out right to
  left. A run carries its paragraph's direction and the embeddings its inline
  ancestors open (`TextRun::paragraph_rtl`, `TextRun::embeddings`), and the
  line is resolved with those as `LRE`/`RLE`/`LRI`/`RLI`/`FSI` …
  `PDF`/`PDI` written into the text UAX #9 reads and nowhere else, two
  sibling boxes' isolates told apart by the box that opened each. A cut piece
  remembers its level (`TextRun::bidi_level`) and is drawn and shaped in
  that level's direction: a run of neutrals has no strong character to say
  which way it reads, and read by its own text the space and `!` ending a
  right-to-left paragraph would be drawn ` !`. A `plaintext` block's
  paragraphs ask the metrics provider for their first strong character
  (`Metrics::first_strong`), which `BookMetrics` answers from the vendored
  `Bidi_Class`.

- **A book's feature settings reach the shaper.**
  `tinker_pdf_shape::Shaper::with_settings` switches features on or off
  **over** the plan a run gets, where `with_features` replaces it: a feature
  set to `0` leaves every stage it is in and the positioning list, and one set
  on that the plan lacks joins the last substitution stage and the
  positioning list — both, a tag saying nothing about which table holds its
  lookups — so a joining run asked for `smcp` keeps its staged forms. The last
  setting of a tag wins. The EPUB path hands it `font-kerning: none` as `kern`
  off and `font-feature-settings` after it, `css-fonts-4` §7.2's order,
  through the run's `FontRequest`, so layout measures with the settings the
  painter draws with. A setting above one is an alternate index this crate's
  alternate substitution does not take, and is refused before it arrives.

- **`GPOS` offsets reach the page.** `PageBuilder::glyphs` writes one hex
  string at one origin, which would put a mark where its advance put it
  rather than where its anchor does, so the EPUB painter draws through
  `DocumentBuilder::glyph_run` — 9.4.3's `TJ` adjustment per glyph and `Ts`
  for a vertical offset, against the same `/W`-rounded advances the reader
  will use — and `letter-spacing` is folded into those positions rather than
  left to `Tc`. `Tc` is applied per glyph and `flow.rs` measures per
  character, so a ligature or a joined word would otherwise be drawn narrower
  than it was measured.

  A page with no mark draws the same bytes whether offsets are carried or
  dropped, so `epub_shaped.rs` carries a `GPOS` `SinglePos` face whose one
  displaced glyph makes the transport visible to its fingerprint; the anchor
  arithmetic itself is adjudicated by aots and by the `GPOS-3` and `GPOS-4`
  sections.

  In extraction a glyph carries its **baseline** origin — the same transform
  with 9.4.3's rise taken out, computed only when there is a rise — and
  `TextDevice`'s line tests ask that rather than where its ink is, so a rise
  does not start a new line and a base–mark–base sequence stays on one. The
  reported quads and origins are the inked ones.

**What no shaping engine here adjudicates.** Ruling 13 rules out running
another shaper and diffing, so the claim for a script is exactly as strong as
the fixture behind it, and the scripts divide in five:

| Script | What is behind it |
|---|---|
| Latin, Ethiopic | text-rendering-tests sections `CMAP-1`, `CMAP-2`, `GSUB-1`, `GSUB-2`, `GPOS-1`–`GPOS-4`: 48 cases, 38 of them discriminating against an implementation with no shaper at all |
| Hebrew, Arabic and every other bidirectional script, for **direction only** | `BidiTest.txt` and `BidiCharacterTest.txt` in full — 861 948 resolutions — both again through `bidi::order_units` (the drawing direction for a line given as units), and every visual order `BidiCharacterTest.txt` states read back through `bidi::logical_order`, the entry point text extraction calls (ruling 14). This says the levels and the visual order are right; it says nothing about the glyphs |
| Arabic *shaping* | `SHARAN-1`: six words of Urdu in Nasta‘līq, reproduced glyph for glyph and position for position. It is the corpus's only Arabic-script section, so joining, `rlig` and cursive attachment are adjudicated **for one face of one style of one language**. Naskh, and the vowelled Arabic of a Qur'an, have no fixture here |
| Balinese, Kannada, Tai Tham | `SHBALI`, `SHKNDA`, `SHLANA`: 333 cases. `crates/tinker-pdf-shape/tests/text_rendering.rs`'s `PASSING` holds the number reproduced per section and is a ratchet — it may rise and may not fall — and its `TRIAGE` says of each remaining failure whether the glyph *set*, their *order* or only a *position* is wrong |
| Every other Brahmic and Southeast Asian script — Devanagari, Bengali, Gujarati, Gurmukhi, Malayalam, Odia, Sinhala, Tamil, Telugu, Myanmar, Khmer, Lao, Thai, Javanese, Sundanese, Tibetan, Tagalog and the rest — and Syriac, N'Ko, Mongolian, Adlam, Thaana, Mandaic, Hanifi Rohingya, Phags-pa | **shaped, and unverified.** The cluster model runs over them because it is driven by the Unicode properties rather than by a list of scripts — and so does the canonical decomposition, which reaches every two-part vowel in Devanagari, Bengali, Oriya, Tamil, Telugu, Malayalam and Sinhala. No fixture in either vendored corpus contains a face for any of them, and none in their upstreams or in HarfBuzz's suite is one ruling 13 admits (below). What that produces is deterministic and plausible; nothing in this repository says it is right |

How the cluster model orders a syllable, each rule held by the sections
above:

- **The halant does not move the reordering insertion point.** A pre-base
  vowel goes in front of the whole conjunct, not in front of the consonant it
  attaches to: `SHBALI-2/1` — `KA ADEG-ADEG PA TALING` — expects the taling
  first.
- **The reordering permutation is computed over the glyphs**, through a USE
  category carried on each one from the character it came from, so it holds
  in the clusters where a substitution has changed their number — exactly the
  clusters where reordering matters.
- **Canonical decomposition**, of Brahmic characters that have one, from
  `UnicodeData.txt` field 5 fully expanded at build time.
- **Two pre-base characters in one syllable come out in the reverse of the
  order they were typed.** Tai Tham `SHLANA-6/2` and `SHLANA-6/4` each have
  `U+1A55 CONSONANT SIGN MEDIAL RA` beside a pre-base vowel, and keeping their
  order costs cases across two sections, so the reversal stands on evidence
  rather than on a default.

What it does **not** do, each named so it is a backlog and not a mystery:

- **The Indic shaper's base-finding.** Kannada, Devanagari and their seven
  relatives form conjuncts by a different model from USE's, in which `rphf`,
  `half` and `blwf` apply at *one position* of a syllable rather than to the
  whole of it. This crate applies them to the syllable.

  In `SHKNDA-2` that is one cause. A Kannada matra has to be next to its base
  before the presentation features run: `NA` + `AA` rewrites the base and
  `NA` + `E` ligates, and neither matches with a subjoined consonant in
  between. Moving every dependent mark back onto its base gains those cases
  and loses more across `SHBALI` and `SHLANA`, because Tai Tham's
  `SHLANA-2/6` expects base, subjoined consonant, mark and Kannada's
  `SHKNDA-2/1` expects base, mark, subjoined consonant for the same shape.
  **No property this crate reads separates them**: both matras are
  `Vowel_Dependent` and `Right`. What separates them is which shaping engine
  the script belongs to, and choosing one per script is a second cluster
  model. `crates/tinker-pdf-shape/src/universal.rs` holds the per-section
  numbers.
- **`rphf` asks whether the syllable has a base for the repha**, which is the
  one piece of that model this crate does implement: a word-final `RA` +
  halant is a dead consonant and not a reph (`SHKNDA-2/7`).
- **A reph is moved to the end of its syllable.** `SHKNDA-2/12` is the one
  case in either corpus with a reph in it, and what it says is that the reph
  goes last, so that `haln` can reach the consonant and virama it left behind.
  The Indic model gives a face several positions to choose between and reads
  which from the face's own tables; this implements one of them and reads
  nothing.
- **Canonical ordering.** What is done above is decomposition and not NFD: the
  `Canonical_Combining_Class` sort that would follow it is not applied. No
  case in the corpus is known to need it, so it is a gap rather than a cause.
- **Hangul.** Its decomposition is arithmetic rather than tabulated (UAX #15
  §3.12) and `UnicodeData.txt` lists none, so a Hangul syllable is not
  decomposed. Hangul does not reach the Brahmic plan anyway; the row is here
  so the absence is a decision.
- **Tai Tham's residue is mostly one glyph.** `TestShapeLana`'s gid311
  (`uni1A78`) is expected where this crate produces gid314, the glyph `cmap`
  gives U+1A7B. It is **not** a feature this crate fails to ask for:
  requesting all twenty-four the face declares still produces gid314. It is
  not a `cmap` difference either; the face has one subtable and it reads
  monotonically across the block. What is left is a lookup that does not match
  the glyph sequence this crate hands it, which is inside USE's cluster
  grammar. `TRIAGE` in `crates/tinker-pdf-shape/tests/text_rendering.rs` holds
  the case list and both refutations.
- **The per-syllable `GSUB` confinement stays**, though it gains nothing in
  this corpus and costs cases. What those cases say is that this crate's
  syllable *boundaries* are in the wrong place, not that confining a lookup to
  a cluster is wrong; `Buffer::set_syllable` has the measurement.
- **Dotted circles.** USE inserts one into a cluster that its grammar calls
  broken. This crate never inserts a glyph the text did not ask for, so a
  malformed cluster renders as its parts.
- **`Default_Ignorable_Code_Point`.** Only `ZWJ` and `ZWNJ` are deleted, by
  `Indic_Syllabic_Category`, which this crate already parses. The wider
  property — soft hyphen, word joiner, the Mongolian free variation selectors
  and some seventy more — is not vendored, because no case in either corpus
  reaches a default-ignorable character that is not one of those two, and a
  table nothing here could adjudicate is worth less than a predicate named as
  narrow. The variation selectors are the exception and are already handled:
  `Shaper::map` *consumes* one, because a selector chooses a glyph.
- **UAX #29 in a column.** The grapheme cluster UAX #50 orients is read as a
  base and the characters after it that are marks (`General_Category` `Mn`,
  `Mc`, `Me`) or of the `Inherited` script — the joiners and the variation
  selectors among them. The rest of UAX #29 is not computed: of what that
  leaves out, an emoji tag sequence is cut, its tag characters (`R`) standing
  sideways after their upright base, while a Hangul syllable sequence and a
  regional-indicator pair are upright on both sides and are not.
- **`Tr`'s rotated fallback.** A `Tr` character whose face has no vertical
  form stands upright with its horizontal glyph. CSS Writing Modes 3 §5.1.1
  does not expect a layout engine to synthesize the fallback, and the itemizer
  sees text and no face.
- **Kerning's two exceptions for proportional metrics.** The registry asks
  that `vkrn` stand off, by default, from the glyphs a face's `vpal` covers
  where `vpal` is off and the face has no `vapk`, and `kern` likewise from the
  glyphs `palt` covers. Neither is applied: `vkrn` runs over a whole column
  and `kern` over a whole line, and a caller switches either off.
- **Vertical shaping is unverified.** No face in either vendored corpus
  carries a vertical feature or vertical metrics, so every vertical test is a
  synthetic face whose expected glyphs and positions the test computes from
  the tables it wrote: it shows the reading in `design/shaping.md`'s
  *Vertical shaping* is the one applied, not that the reading is right. H-64's
  person-written fixtures are what would say so.

**A `cmap` subtable in its format's order is bisected, and one out of order
is read as written.** `glyph_for_char` chooses its subtable at a face's first
lookup and keeps the choice, with whether the subtable's entries are in the
order the format requires: format 4's end codes ascending, format 12's and 13's
groups ascending, disjoint and each well formed, and every entry readable. In
that order the entry a scan from the start stops at is the one a bisection
finds, so a lookup bisects and gives the same glyph for every code; a subtable
a broken producer wrote out of order is scanned, so its first matching entry
still wins. An embedded CMap's `bfrange` and `cidrange` ranges are read the
same way, checked after the CMap is parsed and again after it inherits a
parent's: in ascending, disjoint order a lookup bisects, and otherwise the
first range written that holds the code wins, as the precedence of a single
over a range and of the child's ranges over the parent's requires. So mapping
every character of a long text through a subtable of many segments costs work
in proportion to the text and the logarithm of the subtable, where a scan of
every segment for every character was their product.

**A `cmap` of format 13 is read**, the many-to-one range format: the same
groups as format 12 and the same glyph for every code in the range rather than
an offset into a run. `CMAP-4` is what adjudicates it: it is a capability built
against a published conformance fixture rather than against measured demand,
on a decision the [roadmap](../ROADMAP.md) records, and
`crates/tinker-pdf/tests/cmap_census.rs` counts what the corpora carry.

**A Macintosh subtable is not indexed by Unicode, and is not read as though it
were.** Platform 1 is the classic Mac OS and its subtables are byte maps in a
legacy encoding named by the subtable's `language` field. Below U+0080 every
one of those encodings agrees with ASCII; above it they do not, so
`glyph_for_char` returns `None` there rather than the wrong glyph. That is a
refusal a caller can fall back from, where a silent mis-mapping would not be.

What would lift the refusal is the conversion table for the encoding the
language field names, and this repository does not carry one: Apple's published
mapping files disclaim warranty and grant no redistribution rights, so they
have no SPDX identifier `deny.toml` allows and `cargo xtask vendor` would
refuse them. That is the limit the ICC's own profile files meet too (NG-03,
where the engine's sRGB profile is computed from IEC 61966-2-1 instead), and
it is recorded in the [roadmap](../ROADMAP.md)'s named non-goals rather than
left as owed work.

**A `cmap` of format 2 is read**, the high-byte mapping the legacy CJK
encodings use: a 256-entry key array says, for each first byte, which subheader
reads the byte after it, with key 0 meaning the byte stands alone. That is how
a mixed one- and two-byte encoding is written as one table.

What a format 2 subtable means depends on its platform. On a legacy one —
(3, 3) Windows PRC, (1, 25) Macintosh Chinese simplified, (1, 0) Macintosh
Roman — it is a byte map, and reaching it from a `char` needs the same
conversion table the Macintosh row above does not have. On a Unicode one —
(3, 1) Windows Unicode BMP and (0, 3) Unicode BMP — it is indexed by the
scalar value directly: the high byte selects the subheader and the low byte
indexes within it, which works for the BMP as well as for GBK, and
`glyph_for_char` scores (3, 1) and (0, 3) above everything but (3, 10).

Two more are refused inside `tinker-pdf-shape`, for ruling 13's reason rather
than for want of code: **Syriac's Alaph**, which selects `fin2`, `fin3` and
`med2` in place of `fina` by its `Joining_Group`, and the **topographical
features of a joining Brahmic script**, which would need the four joining masks
on a run that computes syllables instead. Neither has a fixture in either
vendored corpus, so implementing either would be adding behavior nothing here
could show was right.

**Nothing outside the vendored corpora is admissible.** Unicode's
text-rendering-tests at `26cfb96` — the commit vendored here — has no shaping
section beyond those already vendored (`SHARAN`, `SHBALI`, `SHKNDA`,
`SHLANA`); every other section tests a font format (`AVAR`, `CFF`, `GVAR`,
`MORX` and the like). HarfBuzz's suite at `3c4d303` has in-house cases in
eighteen of the scripts this page lists as unverified — one of them Syriac,
none Sundanese, N'Ko, Thaana, Mandaic, Hanifi Rohingya or Tagalog — and none
of them can be a fixture here, for two reasons that each suffice. Their
expected glyphs and positions are **recorded by running `hb-shape`**
(`test/shape/record-test.sh`), so adopting them would make another shaper's
output the expected answer, which ruling 13 forbids by name;
text-rendering-tests and aots are admissible because a person wrote their
expectations. And the faces carry no licence to vendor under: of the ones
those cases use, two state an open licence in their `name` table, and several
are macOS system faces the HarfBuzz repository does not contain either.
HarfBuzz's own `aots` and `text-rendering-tests` directories are the two
corpora already vendored here. So every name stays on the list.

## Refused by name

| What | Typed variant | Why (one line) | See |
|---|---|---|---|
| Shaping **while reading a PDF**: `TJ` arrays are honored as written | none — the producer positioned every glyph and re-shaping them would be wrong | Permanent; shaping to *produce* a document is `tinker-pdf-shape`, above | [shaping](../design/shaping.md) |
| A CFF whose `callsubr` operand is not the token before the call, or that calls a subroutine it does not carry, or whose subroutine calls itself, or that declares `CharstringType 1` | `SubsetRefusal::ProgramNotRebuildable`; the whole face is embedded | Each needs the subsetter to invent what the font meant, and a broken subset renders *almost* right | this page |
| A CFF subset that comes out no smaller than the face | `SubsetRefusal::SubsetNotSmaller`; the whole face is embedded | A producer's own subset has nothing left to remove, and the face is also the one it tested | this page |
| A **CID-keyed** CFF under `add_cid_font` whose charset is not one-to-one | `add_cid_font` returns false | 9.7.4.2 reads a CID through the charset, which answers with the first glyph claiming it, so a glyph whose CID another also claims is one no code reaches. A one-to-one charset is accepted and each glyph written as its CID | this page |
| EPUB text past 224 characters outside `WinAnsiEncoding` in one standard face, in a build **without** `bundled-fonts` | `ArchiveWarning::UnrepresentedCharacters` | a simple font has 256 codes and that build carries no face to key a composite font to; with the feature, the Liberation stand-in is embedded as an `/Identity-H` composite font for every character it covers | [epub](epub.md) |
| Symbol and ZapfDingbats when nothing embeds them | `RenderWarning::UnreadableFont`, in a `bundled-fonts` build too | No face with their repertoire and metrics is under a licence `deny.toml` admits — URW's base-35 stand-ins are AGPL-3.0 with a font exception — and a text face drawn for a symbolic font puts letters where the document meant arrows. A host's `FontProvider` may supply one | this page |
| A CID the descendant font does not carry | `.notdef` drawn + `RenderWarning::UnreadableFont`; extraction: `TextWarning::UnknownFont` | Drawing whichever glyph the code happens to number is the invisible failure | this page |
| A predefined CMap name outside Adobe's registry | `WarningKind::PredefinedCMapUnknown` | A guessed codespace mis-splits the string, so glyphs *and* advances go wrong silently | [rulings](../rulings.md) ruling 10 |
| Registry CID tables in a `cmap-predefined`-off build | `WarningKind::PredefinedCMapApproximate`, `CMap::is_approximate` | Codespaces still ship so strings split right; the CIDs are admitted guesses | this page |
| A `usecmap` parent that cannot be resolved | `WarningKind::CMap(cmap::Warning::ParentUnresolved)` | The child keeps what it declared itself rather than inheriting from nothing | 9.7.5.3 |
| A `usecmap` chain past 4 links, or one that revisits a source | `cmap::Warning::ParentChainCapped`, `cmap::Warning::ParentCycle` | The names come out of the document; being finite is the property that matters (ruling 1) | [rulings](../rulings.md) |
| A truncated CMap mapping section | `cmap::Warning::SectionUnterminated(Section)` | What parsed is kept and the section is named, so partial coverage is visible | [rulings](../rulings.md) ruling 10 |
| An `sbix` graphic of type `mask` or `pdf `, or of any type but `png `, `jpg ` and `tiff` | `SbixRefusal::GraphicType` | The sbix chapter's *Glyph data* names `mask` and `pdf ` as not supported in OpenType, and defines no other | [design](../design/colour-fonts.md) |
| CBDT's uncompressed and component glyph data, the EBDT formats 1 to 9 it inherits | `CbdtRefusal::DataFormat(n)` | Read and drawn by their own step; the PNG formats 17 to 19 are what colour fonts carry | [ROADMAP](../ROADMAP.md) FT-06.21 |
| TrueType and Type 2 hinting | none — outlines are unhinted, not degraded | The bytecode interpreter makes small text differently wrong; subset output keeps `cvt `/`fpgm`/`prep` for readers that disagree. Stem darkening, the bytecode interpreter, CFF hints and an autohinter are roadmap rows, the small-text judgement a person's | [ROADMAP](../ROADMAP.md) FT-04a…FT-04d |

## Verified

- `crates/tinker-pdf-font/tests/complexity.rs` — the parsers held to
  [resource exhaustion](../design/resource-exhaustion.md)'s size-scaling law
  through their counted siblings, in both feature builds: the work of a font
  at four times a size is at most 4.4 times the work at that size — for a
  directory of more tables, a simple glyph of more points and a composite of
  more components, a CFF of more glyphs, a longer charstring and more
  subroutine calls, a longer `eexec` section and a Type 1 font of more
  charstrings, WOFF and WOFF 2.0 files of more tables, and an embedded CMap of
  more `bfchar` mappings — and, where a lookup bisects, at most 4.4 times the
  logarithm's growth more, for format 4 and format 12 subtables of more
  segments and an embedded CMap of more `cidrange` ranges, every code of each
  mapped. Each read gives back what was written, alike through the uncounted
  doors, and a subtable and a CMap written out of order still match the entry
  written first.
- `crates/tinker-pdf-font/src/sfnt.rs`, `glyf.rs` and `cff.rs` — vertical
  metrics against synthetic tables: every advance height, side bearing and
  `VORG` origin reads back as written, the last advance repeating past the
  long metrics; a face without `vhea` or `vmtx`, with a count of zero, a
  `vhea` too short for its count or a `VORG` of another major version answers
  `None`; a `vmtx` shorter than `vhea` claims answers `None` past its end; a
  `VORG` beside `glyf` is ignored for the header's `yMax`; a CFF face takes
  `VORG`'s origin, and without one its charstring's top; `VORG` records out of
  order or overcounted are read as written; and `glyf::bounds` reads the
  header as written, bounds an empty glyph at its origin and a missing one
  not at all. The `sfnt` fuzz target asks for each of them.
- `crates/tinker-pdf-shape/tests/vertical.rs` — vertical shaping. The
  compiled `Vertical_Orientation` against the vendored file at every code
  point; a column of CJK, Latin and Mongolian cut by orientation, clusters
  with enclosing and combining marks, joiners and selectors among them; and,
  against synthetic faces whose expected glyphs and positions the test
  computes from the tables it wrote, an upright run's `vert` and `vmtx`
  metrics, a synthesized em box and its warning, `vrt2` in `vert`'s place,
  a sideways run's line plan and `vrtr`, Mongolian joined sideways and
  isolated upright, `vkrn` down a column, `vhal`, `vpal` and `valt` with
  their interactions, a mark on its base and a cursive join down a column,
  each anchor pair drawn at one point. The `shape_text` fuzz target shapes
  every paragraph as a column too.
- `crates/tinker-pdf/src/fontlist.rs` — unit tests beside the listing:
  9.6.4's subset-tag shape in both directions (seven strings that are not a
  tag, each for its own reason), a descriptor deciding embedding, a
  `/FontFile2` naming no stream reported **not** embedded, a composite font
  as one entry carrying its descendant's program, one face reached three ways
  listed once with both its resource names, a form XObject's and an
  annotation appearance's fonts reached, a resource cycle that terminates,
  the form's `/DR` reached, the listing agreeing with `cos::font::read` about
  family and embedding (self-consistency, named as such), and listing adding
  no warnings.
- `crates/tinker-pdf/tests/annotation_census.rs`'s font half — the census
  above, `#[ignore]`d, printing `RAN`/`SKIPPED`. It asserts every subset tag it
  meets against 9.6.4's shape, that a font with a program is `is_embedded()`
  and one without is not, and that all four families are present.
- `crates/tinker-pdf-font/tests/woff_fixtures.rs` — **the WOFF decoders against
  the committed files** in `crates/tinker-pdf-font/tests/woff/`, written by
  `make-fixtures.py` from `cargo xtask synth-face` — a face this project owns,
  which is why they can exist at all: OFL-1.1 reserves the name of every face
  the corpus vendors, and no producer in the corpus tooling emits a web font.
  Five packings by **three encoders with no code in common** — fontTools
  4.63.0, `ttf2woff` 3.0.0, and `wawoff2` 2.0.1, which is Google's reference
  C++ encoder built to WebAssembly. fontTools **generated** these files and
  adjudicates nothing (ruling 13): no program runs at test time, and every
  assertion compares this build against the source face committed beside the
  containers. `tests/woff/PROVENANCE.tsv` records all of that per file —
  producer, version, the face it was made from, the day, and ruling 13's two
  halves — and a test holds it to the directory in both directions, because a
  record nothing checks stops being true the first time a fixture is
  regenerated. The claim is **identical glyph outlines through `Sfnt`** over
  all 263 glyphs — 230 that draw and 33 that do not, asserted by number — plus
  identical `cmap` answers, identical advances, and a `head` whose
  `checkSumAdjustment` this build recomputed correctly. For WOFF 1.0 from the
  producer that preserved the table order it is **byte identity** with the
  source face.
- `crates/tinker-pdf-font/src/woff/tests.rs` — the parts no committed file
  reaches: §3.1's three legal spellings of 506, `UIntBase128`'s two forbidden
  ones, the known-tag table, an unknown tag carried through, the three tables
  whose legal transform versions differ, a transformed `glyf` refused without
  its transformed `loca` (fontTools' file taken apart and rebuilt three ways),
  and the ceiling refused before a byte is decompressed — and one deliberate
  **non**-refusal: WOFF 2.0 §3.2 says a decoder "MUST NOT reject" a file for a
  non-zero reserved field or a `totalSfntSize` that disagrees, where WOFF 1.0
  §3 and §4 say it MUST reject both.
- `crates/tinker-pdf-font/tests/woff_seeds.rs` and
  `fuzz/fuzz_targets/woff.rs` — the committed seeds, replayed on stable
  because a seed corpus nothing reads stops describing the parser. Which must
  decode and which must be refused are both asserted, and prefixes and
  single-byte flips of them reach an answer rather than a panic (ruling 1).
- `crates/tinker-pdf-font/tests/variations.rs` — variable faces built byte by
  byte by `tests/variable_support/mod.rs`, every expected value computed in
  the test by `variable_support::spec`, the overview's formulas written a
  second time in exact rationals and never by the code under test: axes and
  named instances, extended record sizes, user coordinates normalised against
  minimum, default and maximum and clamped outside them, `avar` at and between
  its points, a named instance resolved, tags given twice or naming nothing,
  and every refusal by name, with every truncation and single-byte change of
  the two tables read or refused without a panic. `TestGPOSFour.ttf`, from the
  vendored text-rendering tests, is read against a second reader in the test
  that decodes its directory, `fvar` and `avar` from the chapters' field lists:
  two axes and thirty-six named instances, each instance's location as the
  specification computes it. `src/variation.rs`'s unit tests hold the
  conversion, each rounding and every branch of the region scalar's
  pseudo-code, and the overview's own normalisation and `avar` examples. The
  tuple store's tests: each table's scalar against the specification's over a
  grid of locations (peak, intermediate, out-of-region and zero-peak axes),
  point numbers and deltas in every run form a packer in the test writes and
  the chapter's own examples, the all-points form, private and shared points,
  `gvar`'s shared tuples by index, the `cvt ` at every named instance and at
  coordinates no instance names equal to the defaults plus the deltas the
  test scales, and every refusal by name. The item store's: region scalars
  over a grid of locations, rows in every delta width read back as written,
  each delta set's adjustment equal to the specification's sum, maps in every
  entry size and both formats with an item past the count, `HVAR`'s advances
  with a map and with the implicit one, its side bearings, `VVAR`'s advance
  heights, side bearings and `VORG` origins, and every `MVAR` tag's value at
  every named instance and at coordinates no instance names — each the
  default the test wrote plus the adjustment the test computed — with every
  refusal by name and every truncation and single-byte change of the three
  tables read or refused without a panic. CFF2's: faces the builder writes
  byte by byte, each glyph's outline equal to the segments its charstring
  states — local and global subroutines, a global subroutine's local call
  taking the caller's Font DICT, an FDSelect of each format and none, a hint
  mask over nine stems one of them implicit, the curve and flex operators —
  through a fresh machine and a reused one; the default instance keeping
  every blend's defaults in charstrings, subroutines and Private DICTs under
  both a glyph's and a Private DICT's `vsindex`; every refusal by name; and
  every truncation and single-byte change of two faces read or refused
  without a panic. Every `cff.rs` and `cff_subset/tests.rs` test holds
  unchanged. At an instance, at every named instance and at coordinates no
  instance names: each subtable's scalars, each glyph's outline — two values
  blended at once, `vsindex` switching the subtable in a charstring and in a
  Private DICT, blends inside local and global subroutines — and each Private
  DICT's blended values equal to the default plus the deltas times the
  scalars the specification computes in the test; the default location
  drawing the unblended outline; and a design space of another axis count
  refused.
- `crates/tinker-pdf-font/tests/colour_tables.rs` — the colour tables over
  bytes the test writes from the OpenType chapters' layouts: COLR version 0's
  layers bottom first with each palette's colours, a slice two glyphs share,
  records out of order scanned and named, the foreground index never a
  palette entry; CPAL version 1's types, labels and entry labels, each
  `PaletteChoice`; every index past its count and every array past the
  table's end answering `None`; COLR version 1's thirty-two paint formats
  each to its node, field for field, every composite mode and an unknown one,
  an unknown extend, clip boxes by range in order and out of it, a
  `PaintColrGlyph` cycle and a self-naming slice refused by name, a shared
  table one node, the depth and size limits met exactly, an overrunning
  colour line, an unknown format, a missing glyph and a slice past the list;
  `sbix`'s strikes and every graphic type, a `dupe` resolved and its chains
  and bad targets refused, no data, refused types and short, backward and
  overrunning data, and each `StrikeChoice` over strikes of repeated sizes;
  `CBLC`/`CBDT` through all five index formats and the three PNG formats,
  small metrics in each direction the flags name, the inherited and unknown
  formats refused, and ranges, lists, subtables and data that overrun or run
  backwards; and every prefix and flipped byte of the tables answering
  without a panic. The `sfnt` fuzz target's `colr-v0.ttf`, `colr-v1.ttf`,
  `sbix.ttf` and `cbdt.ttf` seeds are written by the same builders, and read
  back through their directories.
- `crates/tinker-pdf/tests/cff_fonts.rs` — CFF glyph selection: charset over
  code, string INDEX, built-in encodings, CID-keyed `ROS`/FDArray/FDSelect,
  and a CID-keyed program this engine's writer embedded drawing the glyph it
  was given, bare and in an `OTTO` wrapper.
- `crates/tinker-pdf-font/src/cff_subset/tests.rs` — fonts built byte by
  byte: local and global subroutine renumbering, a global subroutine reached
  from two Font DICTs, `hintmask` counting stems a subroutine declared, `seac`
  components kept, and the four refusals. Two of them cross a **bias
  threshold**: 1 300 local subroutines cut to one (1 131 → 107), and 34 000 cut
  to 1 300 (32 768 → 1 131), the second keeping a non-prefix, non-contiguous
  range so the renumbering is the identity nowhere and every operand form
  appears on the new side and none on the old. That test pins the whole
  rewritten charstring against a byte string derived from the renumbering rule
  rather than read back from the subsetter. A test that only uses small fonts
  never crosses a threshold, so these two, and the corpus census, are what
  hold the bias.
- `crates/tinker-pdf-cos/tests/cff_subsetting.rs` — the writer end: the 9.6.4
  tag, the Table 126 descriptor entry for each of the three shapes, the
  CIDFontType0 descendant over a `CFF ` table, `/W` from the original
  program, each `SubsetRefusal` reported by name, and a CID-keyed program's
  glyphs written as their CIDs — in the string, `/W` (sorted by CID over a
  charset that runs backwards) and `/ToUnicode` — with a charset two glyphs
  share refused.
- `crates/tinker-pdf/tests/cff_subset_census.rs` — every CFF face in the
  fetched corpora's PDFs, a document laid out from another format left out,
  cut to nine glyphs, and every retained glyph's outline,
  advance and font matrix, every glyph's name, every CID's glyph and every one
  of the 256 codes compared with the original's.
- `crates/tinker-pdf/tests/composite_fonts.rs` — the CID selects glyph and
  advance together; `/CIDToGIDMap` in both forms; `.notdef` plus report for a
  CID the font does not carry.
- `crates/tinker-pdf/tests/cmap_inheritance.rs` — `usecmap` and `/UseCMap`
  merge order, the depth cap, and cycle refusal, at the rendered-page level.
- `crates/tinker-pdf/tests/predefined_cmap_rendering.rs` and
  `predefined_cmap_warnings.rs` — registry CMaps choose the glyphs, and a
  name outside the set is warned about in both spellings.
- `crates/tinker-pdf-font/tests/predefined_cmaps.rs` — 805 632 range bounds
  and 474 codespace bounds (all 237 declared codespaces) checked against
  Adobe's own text through a second, deliberately different reader, because a
  round trip through the engine's encoder proves only self-agreement;
  `predefined_cmaps_absent.rs` pins the `cmap-predefined`-off behaviour.
- `crates/tinker-pdf/tests/type3_fonts.rs` — glyph procedures run as content
  under `/FontMatrix`, and their text extracted and found where the glyphs
  stand, once where a procedure shows text of its own, in invisible text and
  inside a form, with nothing drawn changed by it; `vertical_metrics.rs` —
  `/W2` both forms, the `/DW2` default, and the position vector;
  `substitute_fonts.rs` — the `FontProvider` seam, including declining
  symbolic fonts, and a composite
  font's request carrying its character collection
  (`the_provider_is_told_the_character_collection`), which
  `crates/tinker-pdf-cos/tests/fonts.rs`'s
  `a_composite_fonts_character_collection_is_read` reads leniently; and a
  substituted face read by character over a face whose glyph at each CID's
  number is a decoy — `a_substituted_face_draws_a_composite_font_by_character_not_by_cid`,
  `an_unreached_cid_draws_notdef_and_reports`,
  `a_vertical_font_draws_the_faces_vertical_form`,
  `a_face_with_no_cmap_keeps_the_index_reading`,
  `a_simple_font_takes_only_the_readings_a_cmap_admits` and, without the
  tables, `without_the_tables_unembedded_cjk_text_is_reported`; the inverted
  tables in `tinker-pdf-font`'s `predefined.rs`, read from the vendored CMap
  text, inverted once (`the_collection_is_inverted_once`), and reached by
  nothing without `cmap-predefined`.
- `crates/tinker-pdf-font/tests/cjk_coverage.rs` — the CJK census above,
  `#[ignore]`d, printing `RAN`/`SKIPPED`. Its instruments run in every
  `cargo test`: each collection's supplements against the `/CIDCount`s the
  vendored CMap files state, the routes and their tallies over faces built in
  the test, a collection file's faces read one by one, the storage forms over
  the committed WOFF fixtures, the `name` reading, and the candidate list
  against the design's table.
- Fuzzing: the `cff`, `cff_subset`, `cmap`, `sfnt`, `truetype`, `type1` and
  `woff` targets exercise this feature (ruling 1).
- Determinism: the `text` render fingerprint in
  `crates/tinker-pdf/tests/determinism.rs` embeds a synthetic six-glyph face
  built in the test itself and pins glyph rasterisation bit-for-bit across
  targets, asserting a least-ink floor so a face that stops drawing cannot
  read as a pass ([determinism](determinism.md)); the `epub` fixture renders
  through `SimpleFontProvider`, covering the provider path.
- The corpus run (`cargo xtask corpus-run`) exercises real embedded fonts of
  every kind here. See [verification](../verification.md).
