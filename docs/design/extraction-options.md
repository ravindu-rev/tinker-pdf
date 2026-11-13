# Extraction options

`Page::text()` reads a page one way. The options here read it the other ways
a caller may ask for — eleven of them, each opt-in, each a field of a value
the read already takes, and each with a default that is today's extraction to
the byte (ROADMAP row FT-17). Several read from 9.4.4, where a glyph stands
and how far the pen moves after it, and from 14.9.4, what an `/ActualText`
replaces; each definition below is the one the implementing step builds and
its test holds.

## Scope

- **The eleven options**, each with its definition, its default, the type,
  field and function that carry it, and the fixture and known answer its
  step's test holds it to: keep ligatures (1), keep whitespace (2), no
  inserted spaces (3), ignore `/ActualText` (4), code or glyph-id fallback
  for unmapped characters (5), outline-accurate boxes (6), ascenders and side
  bearings (7), clip to the page or a rectangle (8), paragraph breaks by
  first-line indent (9), image blocks in the structured output (10) and spans
  kept unmerged (11).
- **The report the fallback builds on** (FT-17.2): a glyph whose code maps to
  no text is reported, whichever option is in force.
- **The pin** (FT-17.3): today's default extraction of a fixed set of
  committed documents, digested a page at a time, so that each later step
  shows its default unchanged.
- **The `TextChar` fields** this row adds, and the ones rows FT-18 and FT-19
  add or read beside them.
- **The fingerprint and the speed row** (FT-17.13).

## Non-goals

- **Shaping while reading**, refused by name in
  [content-and-text](../features/content-and-text.md): a ligature split here
  divides one glyph's box among the letters its text names, and re-shapes
  nothing.
- **Any change to `Page::text()`'s default output**, which ruling 14 and the
  parity suite pin. Every default below is today's output, and FT-17.3's
  table holds it to the byte.
- **A character a page does not state.** The fallback writes the code or the
  glyph index into the private-use planes and never guesses a letter; reading
  a glyph's outline for its character is row FT-35's, and recognition OC's.
- **The plain-text and search options.** `PlainTextOptions` and
  `SearchOptions` exist and are unchanged; they read the characters these
  options make.
- **Reading order.** Ruling 14 and `Page::text_in` own it, and no option here
  reorders a line or a block.

## Design

### How the options travel

`TextOptions` derives `Clone, Copy, Debug, Default, PartialEq, Eq`, and keeps
all six. Every field this row adds is a `bool` or an enum of `Copy` and `Eq`
values whose `Default` is today's behaviour, so `TextOptions::default()`
still reads as `Page::text()` reads. The one value with numbers in it is the
clip rectangle, `TextRect`: it holds its four coordinates and compares them by
their bits (`f64::to_bits`), so it is `Copy` and `Eq`, and two rectangles are
equal exactly when each coordinate is the same number — `0` and `-0` apart, a
NaN equal to itself. `TextRect::new(x0, y0, x1, y1)` takes two corners in
either order and keeps each axis's smaller and larger.

Every struct literal of `TextOptions` in the tree names a base —
`..Default::default()`, `..TextOptions::default()` or `..*options` — so a
field added breaks none of them; a caller outside the tree that writes the
struct out whole meets a compile error at the new field, which is how a Rust
struct says it grew. `Document::text_pages` hands each page the options it is
given with `parallelism` serial, as it does now.

| Option | `TextOptions` field | Type, its default first |
| --- | --- | --- |
| 1. keep ligatures | `split_ligatures` | `bool`, `false` |
| 2. keep whitespace | `normalize_whitespace` | `bool`, `false` |
| 3. no inserted spaces | `insert_spaces` | `bool`, `false` |
| 4. ignore `/ActualText` | `ignore_actual_text` | `bool`, `false` |
| 5. code or glyph-id fallback | `unmapped` | `Unmapped`: `Drop`, `Code`, `Glyph` |
| 6, 7. boxes | `boxes` | `CharBoxes`: `EmBox`, `FontMetrics`, `Outline` |
| 8. clip | `clip` | `TextClip`: `None`, `CropBox`, `Rectangle(TextRect)` |
| 9. paragraph breaks | `indent_breaks` | `bool`, `false` |

Options 10 and 11 are the writer's: `WriterOptions { unmerged_spans,
image_blocks }`, both `bool`, `false` by default, `Copy`, `Eq` and `Default`,
taken by `TextWriter::with_options(format, options)`; `TextWriter::new` is
`with_options(format, WriterOptions::default())`. The facade re-exports
`WriterOptions` beside `TextWriter`, and `Unmapped`, `CharBoxes`, `TextClip`
and `TextRect` beside `TextOptions`.

The facade turns `TextOptions` into what `tinker-pdf-content` reads,
`ExtractionOptions`: `unmapped`, `boxes`, `insert_spaces`, `indent_breaks`,
`split_ligatures`, `normalize_whitespace`, and `clip` as the rectangle itself,
`Option<(f64, f64, f64, f64)>`, `TextClip::CropBox` resolved to
`Page::crop_box`. It is `Copy` and `PartialEq` and not `Eq`, since nothing
compares two. `TextDevice::with_options(&ExtractionOptions)` reads the first
five; `text::apply_character_options(&mut TextPage, &ExtractionOptions)`
reads the two character options. `TextDevice::new()` is `with_options` of the default, so
every device built today is unchanged.

### What the interpreter is asked, and when

The default extraction parses no font program: the widths come from the font
dictionary (`Page::text_in_content_order`'s comment). Two options need more —
the glyph-id fallback the glyph index a code selects, and the box options the
font's ascent and descent and a glyph's outline bounds — and each is asked
only where the device wants it:

- `Device::glyph_queries(&self) -> GlyphQueries` is asked once, as an
  interpretation starts, as `Device::composites_colorants` is; the default
  asks nothing. `GlyphQueries { unmapped_glyph_ids, extents }`, both `bool`.
  A wrapping device (`Culled`) forwards its inner device's answer.
- `FontSource` gains three questions, each answering `None` by default:
  `glyph_id(font, code) -> Option<u16>`, `font_extent(font) -> Option<(f64,
  f64)>` (the ascent and the descent) and `glyph_ink(font, code) ->
  Option<(f64, f64, f64, f64)>` (the outline's bounds), every number in the
  glyph's own space, where an em is one unit.
- `Glyph` gains `glyph_id: Option<u16>` and `extent: Option<GlyphExtent>`,
  `GlyphExtent { ascent_descent, ink }` holding the two answers. Both are
  `None` wherever the device did not ask, which is every glyph of every
  device but the text device under these options.
- The interpreter asks `glyph_id` for a glyph whose text is empty and for no
  other, and `font_extent` once for each text-showing operator and
  `glyph_ink` once for each glyph it shows, where `extents` is asked. A Type
  3 glyph is asked nothing: its shape is a procedure.
- The facade's `PageResources` answers through the glyph selection a render
  and the subsetter make (`selection`, `extract_outline`, 9.6.6.4, 9.7.4.2),
  and reports nothing as it does: a text extraction's operation reports no
  unresolved glyph (`Operation::quiet_glyphs`), so no option adds an
  `UnknownFont` warning a render would add for a glyph its font does not
  carry. What it reads is counted, as a render's reading is: a program
  decoded and an outline read appear in `Page::text_counted`'s `Stats`.

### The em box, and what every decision reads

A character's quad today is its **em box**: the advance wide, 0.8 of an em
above the baseline and 0.2 below, through the glyph's transform
(`TextDevice::show_glyph`). Every decision extraction makes from geometry
reads the em box, under every option: which glyphs are one line and which
lines one block (the device), and ruling 14's joins, visual order, marks and
stated text (`text_order`). So a box option changes the quads a caller reads
and nothing else, the text and its order included.

The mechanism: each character carries its glyph's place in the content
stream's order, `TextChar::glyph` (below). Under a box option the device
keeps the box the option asks for beside the page, by that place, and the
union of each `/ActualText` span's boxes by the span; `TextDevice::finish_held`
returns the page and `HeldBoxes`, and `TextDevice::finish` is `finish_held`
with the boxes put straight in. The facade puts them in once ruling 14 has
read the page (`HeldBoxes::apply`): each character with a glyph takes its
glyph's box, a character an `/ActualText` placed on its span's box rather than
on a glyph (`text_order::stated_chars`) takes its span's union, and the lines'
and blocks' quads are made again from the characters', as `finish` makes
them. Under `EmBox` nothing is held and `apply` does nothing.

### 1. Keep ligatures

`split_ligatures: false` keeps every character as its glyph's text names it,
as today. With `true`, every character whose text is two pieces or more, or is
one ligature presentation form, is replaced by one character a piece:

- **A piece** is a character with every character after it whose UAX #29
  `Word_Break` is `Extend`, `Format` or `ZWJ`, the classes the device already
  treats as riding on what precedes them (`text.rs`'s `strong`): a letter keeps
  its marks, and `é` written as `e` and U+0301 is one piece.
- **A ligature presentation form** is a character of the Alphabetic
  Presentation Forms block (U+FB00 to U+FB4F) or of the Arabic Presentation
  Forms-A and -B blocks (U+FB50 to U+FDFF, U+FE70 to U+FEFF) whose
  decomposition in `UnicodeData.txt` is tagged `<compat>`, `<isolated>`,
  `<initial>`, `<medial>` or `<final>` and names, as written, two pieces or
  more. Its pieces are the decomposition's: U+FB01 is `f` `i`, U+FB03 `f` `f`
  `i`, U+FB4F `א` `ל`, U+FEFB `ل` `ا`, and U+FB05 `ſ` `t`, the letters as the
  UCD spells the ligature, not decomposed again. A form whose decomposition is
  one piece — a mark on a space — or untagged or `<font>` — a letter with its
  point, a wide Hebrew letter — is no ligature, and is kept. The table is
  compiled from the vendored `UnicodeData.txt` by `tinker-pdf-content`'s
  `build.rs`, beside the diacritic folding.
- **Each piece takes an equal share of the quad along its baseline**: the
  lower edge, from `ll` to `lr`, and the upper, from `ul` to `ur`, each in as
  many equal parts as there are pieces. The first piece takes the part at the
  quad's start where the text holds no right-to-left character
  (`reads_right_to_left`), and the part at its end where it holds one, so a
  Latin ligature reads left to right and an Arabic one from the right, as each
  is drawn. Each piece's origin is the character's moved along the baseline by
  its part's offset from the quad's start. A vertical line's shares run down
  its column, the first at the top. Nothing on the page says how wide each
  letter of one glyph is, so the shares are equal.
- **Each piece keeps the character's other fields** — size, `mcid`, `stream`,
  `font`, `stated`, `artifact` and `glyph`, and those later rows add — and the
  line's text is made again from its characters.
- **When**: after ruling 14 has read the page, its stated text placed and its
  held boxes put in, so every character is split, the ones an `/ActualText`
  states among them, and a share is of the box the caller reads. Words, search
  and the serialisations read the pieces, so a match inside a ligature is
  boxed inside it.
- **Fixture and known answer** (FT-17.6): a built page with a glyph mapped to
  `ffi` and one mapped to U+FB01, on a left-to-right line and on a
  right-to-left one, comes back as three and two characters, in the order
  above, whose quads tile the glyph's.

### 2. Keep whitespace

`normalize_whitespace: false` keeps every character's white space as its
glyph's text names it, as today. With `true`, in the same pass and after the
split, every character of a character's text that has Unicode's `White_Space`
property (`PropList.txt`; `char::is_whitespace` reads the same table) other
than U+0020 SPACE is written U+0020: U+0009 to U+000D, U+0085, U+00A0,
U+1680, U+2000 to U+200A, U+2028, U+2029, U+202F, U+205F and U+3000. Each
becomes one space, a run is not collapsed, and the character's box and every
other field stay. U+200B ZERO WIDTH SPACE and U+FEFF are `Format` and not white
space, and are kept. The `/ActualText` strings a page records
(`TextPage::stated`) are the page's statement and are kept as written; the
characters that stand for them are normalised. Fixture and known answer
(FT-17.6): a tab, a no-break space and an ideographic space each read as one
U+0020, each with its own quad.

### 3. No inserted spaces

No extraction path inserts a space today, and `insert_spaces: false` keeps
that: the row's "no inserted spaces" is the default. With `true`, as each
glyph arrives on a horizontal line the device is collecting, where neither it
nor the line's last glyph is white space and the gap between their em boxes
along the line's baseline — this glyph's near edge past the last one's far
edge, or, for a pen moving left, the last one's near edge past this glyph's
far edge — is more than **0.15 of an em** of the larger of their two sizes,
the device adds a character before this glyph:

- its text U+0020; its quad the gap, from the one box's edge to the other's
  along the baseline and as tall as the last glyph's em box; its origin the
  last glyph's far edge on its baseline;
- the last glyph's size, `mcid`, `stream`, `font`, `stated` and `artifact`;
  `glyph` and `show` `None`, since no glyph and no operator drew it;
- `TextChar::inserted` `true`, which is `false` on every character a glyph
  draws.

Nothing is inserted where a line breaks, on a vertical line, or beside a glyph
an artifact scope drops. The threshold is under the narrowest word space of
the common text faces — a quarter to a third of an em — and over a kern or a
tracking adjustment, which stays under a tenth of an em, so a producer that
positions its words with `TJ` reads as words and its kerning does not split
them. A right-to-left line takes its space between the same two glyphs, and
ruling 14 then reads it in its place. Fixture and known answer (FT-17.7):
`[(Hello) -300 (World)] TJ` at a known size reads `Hello World`, the space's
quad spanning the gap; `-100` and a drawn space add nothing.

### 4. Ignore `/ActualText`

`ignore_actual_text: false` reads an `/ActualText` on a marked-content property
list where its glyphs stand (14.9.4, ruling 14's *What a page says*), as
today. With `true`, `Page::text_with` reads the glyphs instead: the device
still records each span (`TextPage::stated`, `TextChar::stated`), and the
facade places no stated text — `text_order`'s placement in content order and
in logical order reads every unit as drawn. `StructureTree::text_for_page_with
(index, page, &TextOptions)` reads the option too: with it, an element's
`/ActualText` replaces nothing either, and every run reports
`TextSource::Glyphs`; `text_for_page` is `text_for_page_with` of the default,
and keeps its signature. `matched`, `orphans` and `unmarked` count characters
by their `/MCID`, which reading the glyphs leaves as it was. Fixture and known
answer (FT-17.12): the pages of `tagged_pdf.rs`'s
`actual_text_replaces_what_it_encloses_without_orphaning_it` and
`actual_text_on_a_property_list_replaces_its_own_sequence` give the
replacements by default and the drawn glyphs with the option, the three counts
alike both ways.

### 5. Code or glyph-id fallback

A glyph whose code maps to no text — `Glyph::text` empty: no `/ToUnicode`
entry and no encoding that names it — is reported under every mode (FT-17.2,
below). Then:

- **`Unmapped::Drop`**, the default, drops it, as today.
- **`Unmapped::Code`** keeps it as one character standing for its code,
  `Glyph::code` (the byte of a simple font, the CMap code of a composite one):
  U+F0000 plus the code, in Supplementary Private Use Area-A, for a code up to
  0xFFFD, and U+FFFD for a larger one.
- **`Unmapped::Glyph`** keeps it as one character standing for the glyph
  index its font program selects for the code (`Glyph::glyph_id`): U+100000
  plus the index, in Supplementary Private Use Area-B, for an index up to
  0xFFFD, and U+FFFD for a larger one and for a glyph whose font cannot say —
  a font with nothing embedded, a Type 1 program, which addresses its glyphs by
  name, and a Type 3 font.

Private use, because Unicode reserves those planes for private agreement and a
producer's text means none of them, so a fallback is never read as a letter;
the two areas tell the modes apart; and the code or index is the character's
value less its area's first code point. A kept glyph is a character as any
other is — it joins lines, words and search, with the em box — and its
`Bidi_Class` is `L`, as every private-use character's is, which ruling 14
reads as it reads any letter. The index is `PageResources::selection`'s, the
glyph a render draws: the `cmap` through the code's character or the code, a
`/CIDToGIDMap` through the CID, a CFF charset through a name, and 9.6.6.4's
closing guess. Fixture and known answer (FT-17.4): a page embedding a
Liberation face as a composite font with no `/ToUnicode`, whose
`/CIDToGIDMap` sends two drawn codes to the glyphs of two characters the test
reads from the face's `cmap`, gives the two private-use characters of each
mode at the glyphs' quads, and nothing by default.

### 6 and 7. Outline-accurate boxes; ascenders and side bearings

`boxes` chooses a character's quad. In the glyph's own space, where an em is
one unit along each axis and the glyph's transform takes it to the page:

- **`CharBoxes::EmBox`**, the default: `x` from 0 to the advance (at least a
  thousandth of an em) and `y` from −0.2 to 0.8; a vertical glyph's `x` from 0
  to 1 and its `y` from −0.2 to 0.8 of its advance. Today's box.
- **`CharBoxes::FontMetrics`**, ascenders and side bearings: a horizontal
  glyph's `x` from where its outline's bounds start to where they end — its
  left and right side bearings taken off its advance — or from 0 to the
  advance where it draws nothing, and its `y` from the font's descent to its
  ascent, or from −0.2 to 0.8 where neither is known. A vertical glyph keeps
  the em box: ascent and descent are horizontal metrics.
- **`CharBoxes::Outline`**: the outline's bounds, `x` and `y`, where the
  glyph draws something, and the `FontMetrics` box where it draws nothing (a
  space).

The ascent and descent are, in this order: an embedded sfnt program's `hhea`
ascender and descender over its `unitsPerEm`, where they are not both zero;
the font descriptor's `/Ascent` and `/Descent` over 1000 (9.8.1, Table 120),
where they are not both zero; the descriptor's `/FontBBox` lower and upper `y`
over 1000, where the lower is below the upper; and otherwise none. A
composite font's descriptor is its descendant's. The program comes first
because it is what is drawn, and a writer's descriptor may state numbers of
its own. The outline's bounds are `Outline::bounds` — its points' box, control
points included, which for a `glyf` glyph is its header's — of the outline
`PageResources::extract_outline` reads, at the instance a render draws, in an
em's units; there are none for a glyph with no outline, a font with no
program, and a Type 3 glyph, which keeps the em box under every option.
Lines', blocks', words' and search's quads are built from characters' and
follow. Fixture and known answer (FT-17.5): a page built with an embedded
Liberation face at a known size, one line of it turned, whose quads in each
mode are the ones the test computes from the face's `hhea`, `hmtx` side
bearings and `glyf` boxes.

### 8. Clip to the page or a rectangle

`TextClip::None`, the default, keeps every glyph, as today. Under
`TextClip::CropBox` (`Page::crop_box`) or `TextClip::Rectangle`, in default user
space — the space text is reported in — the device leaves out each glyph, as
it arrives, whose character's centre is outside the rectangle.

**The rectangle rule**, stated here once: a character is inside a rectangle
when the centre of its quad's enclosing rectangle (`Quad::bounds`) is, edges
included — `x0 ≤ cx ≤ x1` and `y0 ≤ cy ≤ y1` — and no character is inside a
rectangle with a coordinate that is not a number. It is the rule `tables.rs`
assigns a character to a cell by, and the one FT-19.3's region crop and FT-23's
selection by rectangle cite. The quad is the one the character is reported
with, under the box option in force.

A glyph left out is not a character: it joins no line, moves no pen and adds
no inserted space, so the lines and blocks are made of what is kept and their
quads cover only that. Fixture and known answer (FT-17.8): a page with words
drawn past its crop box and a word straddling a caller's rectangle returns the
characters the rule keeps, with their quads, and the default returns them all.

### 9. Paragraph breaks by first-line indent

`indent_breaks: false` groups lines into blocks by their vertical gap alone,
as today (`TextDevice::finish`: a line joins the block before it where the gap
is at most one and a half line heights). With `true`, each block that rule
makes whose lines are all horizontal and unturned — their baselines along the
page's `x` axis, as the device's `Frame::Page` measures them — is cut further:

- a line's **start** is its quad's left edge where the device counts it left
  to right (`TextLine::rtl` false), and its right edge where it counts it
  right to left;
- a line is **indented** where its start stands at least one em of its own
  size (`TextLine::size`) in from the block's edge on its side: the leftmost
  left edge of the block's left-to-right lines, or the rightmost right edge of
  its right-to-left ones;
- a line **follows** the line before it where its first character's origin
  is below that line's first character's by more than half the larger of their
  sizes, so two pieces of one row are never a paragraph break;
- where the block's lines that are not indented outnumber the ones that are —
  paragraphs set with a first-line indent — the block is cut before every
  indented line that follows a line that is not; otherwise — a hanging
  indent, whose indented lines outnumber the others, centred lines, a block
  with as many of each — it is kept whole.

Each block so cut has its quad made again from its lines'. The threshold is
one em because a first-line indent is one to two ems in book typography, and a
ragged-right line starts at the margin whatever its length. Fixture and known
answer (FT-17.9): two paragraphs at even leading, the second's first line
indented two ems, are two blocks with the option and one without; a
right-to-left paragraph indented from the right is cut the same way; a hanging
indent is one block either way.

### 10. Image blocks in the structured output

`WriterOptions::image_blocks: false` writes text alone, as today. With `true`,
`TextWriter::page_with_images(frame, page, images: &[ImageBlock])` writes each
of `images` as a block of its own beside the page's text blocks;
`TextWriter::page` is `page_with_images` with none, and with the option off
`page_with_images` writes what `page` writes, to the byte.

- **The block**: `ImageBlock { bbox, matrix, width, height }`, plain numbers,
  so the writer stays free of image types: `matrix` the placement's `[a b c d
  e f]`, `bbox` the enclosing rectangle of the unit square through it (8.9.5.2)
  in default user space, `width` and `height` the image's in samples.
- **Where they come from**: `Page::image_blocks()`, one block for each
  placement of each image `Page::images()` lists, in that list's order and
  each image's placements in content order, read from the same walk with no
  samples decoded.
- **Where they are written**: JSON, a page's `images` array after `blocks`
  and before `warnings`, each `{"bbox":[…],"matrix":[…],"width":…,"height":…}`;
  XML, an `<image bbox="…" matrix="…" width="…" height="…"/>` element for each
  after the page's blocks and before its warnings; HTML, a `<div
  class="image" data-width="…" data-height="…">` positioned at its box after
  the page's blocks. Numbers are written as every number of the format is.
- **`TEXT_FORMAT_VERSION` stays 1.** It moves when a field changes meaning or
  goes away; an image block is a new field, written only where the writer was
  asked for it, and `blocks` still holds text blocks alone.

Fixture and known answer (FT-17.11): a built page with text and one image
drawn under two matrices writes two image blocks whose boxes are the matrices'
unit squares, in all three formats, and none by default.

### 11. Spans kept unmerged

A span is a run of a line's consecutive characters that share a font name and
a size (`serialize.rs`'s `spans`), and `WriterOptions::unmerged_spans: false`
keeps that, as today. Each character records which text-showing operator drew
it, `TextChar::show`: the operator's place among the `Tj`, `TJ`, `'` and `"`
the interpreter runs on the page, a form's and a Type 3 procedure's among
them, counted from 0 in the order it runs them (`Glyph::show`). A Type 3
glyph reported after its procedure carries the operator that showed it, and a
character no operator drew — an inserted space, a stated character on its
span's box — has none. With `true`, a span is also cut where `show` changes,
a character with none continuing the span before it, so a span is what one
operator drew. Fixture and known answer (FT-17.10): a line drawn by three `Tj`
in one font is one span by default and three with the option, each with its
own box and text, in all three formats.

### The unmapped-code report

`TextDevice::show_glyph` drops a glyph whose text is empty, and nothing has
said so: `TextWarning::UnmappedCode` exists in the model and the writers and
is constructed nowhere. The device now reports each such glyph, before it drops
it or keeps it as a fallback, with the glyph's code. The report is
deduplicated as `TextDevice::warn` deduplicates, one warning a code, and
bounded apart: at most 64 distinct codes a page, kept beside the 64
`UnknownFont` warnings `warn` keeps, so a page with many unmapped codes still
names every font it could not resolve. Only `TextPage::warnings` gains
entries; the text, the lines and the quads are unchanged.

### The pin of the default

FT-17.3's suite, `crates/tinker-pdf/tests/text_defaults.rs`, digests the
default extraction of every committed document of this set that opens without
a key:

| Documents | Kind |
| --- | --- |
| `testdata/*.pdf` | the reference fixtures |
| `crates/tinker-pdf/tests/signature_support/*.pdf`, `crates/tinker-pdf/tests/pubsec_support/*.pdf`, `crates/tinker-pdf/tests/xps/source/*.pdf` | PDFs committed for other suites |
| `fuzz/corpus/cos_document/*.pdf`, `fuzz/corpus/render_page/*.pdf` | the fuzz targets' seed PDFs, their fonts of each kind among them |
| `crates/tinker-pdf/tests/epub/*.epub` | the committed books, laid out by this engine |
| `crates/tinker-pdf/tests/xps/*.xps` and `*.oxps`, `crates/tinker-pdf/tests/xps_rows/*.xps`, `crates/tinker-pdf/tests/xps_interleaved/*.xps`, `fuzz/corpus/render_page/*.xps` and `*.oxps` | the committed XPS documents, converted by this engine |

It is the set `text_logical_order.rs`'s `no_left_to_right_page_moves` reads —
`testdata` and the books — widened to every committed PDF and XPS document.
The comic archives under `crates/tinker-pdf/tests/cbz` are not in it: every
page of one is an image, with no text to pin. A document that does not open —
an encrypted one, whose key is not this question — is counted and passed over.

A page's **record** is text, one item a line: each block's quad's bounds;
each line's bounds, writing mode, `rtl`, size and text; each character's text,
quad, origin, font and size; each warning, by its kind and its value; and
`plain_text()`. Every number is written to the thousandth (`{:.3}`, a negative
zero as `0.000`), every text with `\`, the C0 controls and DEL escaped as
`\u{…}` and everything else as it is, so the record is the same bytes on every
target and every toolchain. Named fields alone are read, so a field another
row adds to `TextChar` moves no digest. The digest is the record's SHA-256.

The table, `crates/tinker-pdf/tests/text_defaults.tsv`, is a header of `#`
lines naming the command that writes it, then one row a page: `build`,
`document` (its path from the repository root), `page` (from 0), `digest`. A
book or an XPS document is laid out or converted by this engine with the faces
its build bundles, so every row is kept per build: the build is `default` or
`bundled-fonts`, the two builds the gates test the facade in, or another
build's name of the features that change a reading (`cmap-predefined`,
`bundled-fonts`, `hyphenation`). The writer, the `#[ignore]`d
`write_text_defaults`, run by name in one build, writes that build's rows and
keeps the others'. The check, `default_extraction_of_every_committed_fixture_is_pinned`,
fails on every page whose digest moved, naming the document and the page, on
every page of the `default` and `bundled-fonts` builds with no row, as
unrecorded, and on a row naming a page that is not there; in another build with
no rows it prints a `SKIPPED` line. It prints a `RAN` line with the documents
and pages read. A digest that moves is a finding put to the owner, and never
re-pinned to match.

### The `TextChar` fields, this row's and its neighbours'

`TextChar` has public fields and struct literals across the tree; each step
that adds a field writes it into every literal, and `TextPage`'s writers read
none of them unless their row says so.

| Field | Added by | What it is |
| --- | --- | --- |
| `glyph: Option<u32>` | FT-17.5 | the drawing glyph's place among every glyph the content showed the device on the page, in the content stream's order, from 0; the pieces of a split ligature share it; `None` for a character no glyph draws |
| `inserted: bool` | FT-17.7 | an inserted space (option 3) |
| `show: Option<u32>` | FT-17.10 | the text-showing operator that drew it (option 11) |
| per-character flags | FT-18.3 to FT-18.5, FT-18.9 | filled, stroked, synthetic and real bold, clipped, underline and strike-out, as FT-18.1 defines them; FT-18's inserted flag is `inserted`, read and not stored twice |
| language | FT-18.6 | the language 14.9.2 gives the character |

FT-19 adds none. Its grid places a character by `origin` and `size` and never
by its quad, so a box option moves no column, and its region crop is option 8's
rule through `clip`.

## Milestones

1. **The unmapped-code report** (FT-17.2). Exit: `text.rs`'s
   `a_code_nothing_maps_is_reported_once` — two glyphs of one unmapped code
   and one of another give two warnings and the same text — and
   `text_options.rs`'s `an_unmapped_code_is_reported` on a built page whose
   font maps a drawn code to nothing; every existing extraction test passes
   with its text unchanged, in the default and `bundled-fonts` builds.
2. **The pin** (FT-17.3). Exit: `text_defaults.rs`'s
   `default_extraction_of_every_committed_fixture_is_pinned` passes at the tip
   after FT-17.2, prints its `RAN` line with the documents and pages read, and
   on a moved digest fails naming the document and the page.
3. **Code or glyph-id fallback** (FT-17.4). Exit: `text_options.rs`'s
   `an_unmapped_code_falls_back_to_its_code_or_its_glyph_when_asked`, in the
   default and `bundled-fonts` builds; `text_defaults.rs` passes.
4. **The boxes** (FT-17.5). Exit: `text_options.rs`'s
   `each_box_option_is_the_faces_own_metrics`, in both builds;
   `text_defaults.rs` passes.
5. **Ligatures and whitespace** (FT-17.6). Exit: `text_options.rs`'s
   `ligatures_split_into_letters_when_not_kept` and
   `whitespace_is_normalised_when_not_kept`, in both builds;
   `text_defaults.rs` passes.
6. **Inserted spaces** (FT-17.7). Exit: `text_options.rs`'s
   `a_gap_wider_than_the_threshold_reads_as_a_space_when_asked`, in both
   builds; `text_defaults.rs` passes with the option off.
7. **The clip** (FT-17.8). Exit: `text_options.rs`'s
   `text_outside_the_page_or_the_rectangle_is_left_out_when_asked`, in both
   builds; `text_defaults.rs` passes.
8. **Paragraph breaks** (FT-17.9). Exit: `text_options.rs`'s
   `an_indented_first_line_starts_a_block_when_asked`, in both builds;
   `text_defaults.rs` passes.
9. **Unmerged spans** (FT-17.10). Exit: `text_serialize.rs`'s
   `spans_are_one_per_showing_operator_when_unmerged`, in all three formats
   and both builds; `text_defaults.rs` passes.
10. **Image blocks** (FT-17.11). Exit: `text_serialize.rs`'s
    `image_blocks_are_written_when_kept`, in all three formats and both
    builds; `text_defaults.rs` passes.
11. **Ignoring `/ActualText`** (FT-17.12). Exit: `tagged_pdf.rs`'s
    `actual_text_is_ignored_when_asked`, in both builds.
12. **The fingerprint and the speed row** (FT-17.13). Exit: `determinism.rs`'s
    `extraction_options_are_stable_across_targets`, the JSON of a built page
    read with every option on (`Unmapped::Glyph`, `CharBoxes::Outline`,
    `TextClip::CropBox`, every `bool` true, and both writer options), entered
    in `xtask/src/determinism.rs`'s roster, holds its hash under `cargo test`
    on Linux and Windows and under `--target wasm32-wasip1`; `xtask/src/bench.rs`'s
    `the_committed_baseline_agrees_with_the_tree` holds MS-04's text group's
    two new operations, the 100 embedded-TrueType pages extracted with
    `CharBoxes::Outline` and by default, in the bench file and
    `baseline.json`.

## Risks

| Risk | Mitigation |
| --- | --- |
| A producer's `/ToUnicode` names the private-use planes, and a fallback is read as its text | The supplementary planes are the ones a producer's text least uses, and every fallback is reported as an `UnmappedCode` warning whatever its mode, so a caller can tell a fallback from a stated character |
| A box option, reading font programs, makes extraction slower | Only the box options and the glyph-id fallback read a program, and only through `Device::glyph_queries`; the default asks nothing, and FT-17.13 times the outline mode beside the default |
| A box option changes which glyphs are a line, or ruling 14's order | Every decision reads the em box (*The em box, and what every decision reads*), and the option's boxes are put in after; `text_defaults.rs` holds the default, and each box test holds the text alike in every mode |
| Equal shares misplace a ligature's letters, `f` being narrower than `i` | Nothing on the page states the letters' widths; the shares tile the glyph, so search and selection stay inside it, which is what the option is for |
| A letter-spaced or tracked line reads with a space inside its words | The threshold is above a tenth of an em; tracking past 0.15 em is display setting, and the option is opt-in |
| A centred or ragged block is cut at every line by the indent rule | The rule cuts only where flush lines outnumber indented ones and only before an indented line that follows a flush one |
| A character straddling a clip's edge is kept on one side and lost on the other | The centre rule is one rule, stated once and cited by the region crop and the selection, so every reader of a rectangle agrees |
| A digest moves for a reason no step intended | It is a finding put to the owner, never re-pinned; the record names its fields, so a field added elsewhere cannot move it |
