# Creation

`DocumentBuilder` is a thin authoring layer over the [writer](writing.md):
pages, content, fonts, images, graphics state, patterns, shadings, links,
metadata and an outline. It deliberately does no layout — placing what a
caller already positioned is its business, composing paragraphs is not. The
container formats use it: a CBZ, XPS or EPUB becomes a PDF through exactly
this API, which is why it carries what those formats needed (CID fonts,
ExtGStates, patterns, links, outlines) and nothing speculative.

## What it does

**From HTML and CSS** (tier 5's formats row). The builder itself still
places only what a caller positioned; `tinker_pdf::FromHtml`, implemented for
it in the facade, is the one constructor that composes. With the trait in
scope, `DocumentBuilder::from_html(markup, stylesheet, PageBox)` reads the
markup into the EPUB reader's tree — as XML, and when it is not XML by HTML's
own parser (WHATWG §13.2, `tinker_pdf_xml::html`), so tag soup makes the
document its tree is and the report says `MarkupDefect::NotXml`
(`tag_soup_makes_the_document_its_tree_is`) — applies `stylesheet` as an author
sheet **ahead of** every sheet the markup links (so the markup's own `<style>`
wins a tie, as after a `<link>` at the top of `<head>`), lays it out into
pages of the `PageBox` — a size, a margin inside it (default half an inch)
and a base font size — and hands back a builder holding the pages, every face
the layout used registered and the `<title>` as `/Title`, so a caller can add
information or pages and `finish` as usual. It is the EPUB path with the book
taken away: `epub::lay_out_one`, the same cascade, layout and painter, which
is why `tests/html_creation.rs` holds a document made this way **pixel for
pixel** to the same markup as the one chapter of an EPUB at the same box. The
`HtmlReport` beside the builder speaks a book's `ArchiveWarning` vocabulary —
unimplemented properties counted by element, pictures not drawn, sheets that
did not resolve, characters no face covers, and an `UnusableOption` for a box,
margin or size the caller passed that could not be used and was replaced. A
document the cascade or the layout refuses at one of its caps — the caller's
own stylesheet included — is `HtmlError::{StyleRefused, LayoutRefused}`
rather than a placeholder page, because a creation call has no page count to
keep. References: a `data:` URL
carries its own bytes; anything else is missing and named under `from_html`,
and asked of the caller's `epub::read::Resources` under `from_html_with`.
`from_markdown(text, stylesheet, PageBox)` is the same call for Markdown:
`tinker_pdf::markdown` translates CommonMark 0.31.2 into an XHTML document
and `from_html` lays it out, with what the translation did — raw HTML set as
text, a container past the nesting cap — first in the report as
`ArchiveWarning::Translation` ([opening](opening.md)).

**Pages.** `add_page(width, height, |page| ...)` hands a `PageBuilder` to a
closure; the page's `/MediaBox` is the given size, and `set_crop_box` /
`set_bleed_box` add the two 14.11.2 boxes a fixed-layout source may state.
Content is emitted as operators into the page's stream in the order the
closure calls are made.

**Text.** `text(font, size, x, y, str)` sets a string in a simple font
resource, writing the string's UTF-8 bytes — right for a font whose
encoding agrees with ASCII and wrong the moment a character does not (`é`
in a `WinAnsiEncoding` font is one byte; `text` would write two), which is
what `encoded_text` is for: it takes the codes the caller chose and,
separately, the characters they stand for, so subsetting still sees what
the page drew; `glyphs(font, size, x, y,
&[Glyph])` places glyphs by index with explicit advances through a CID font,
which is how a format that addresses glyphs by index (XPS) or a layout engine
that already measured every run (EPUB) sets text; `text_pieces(size, y,
spacing, &[TextPiece])` writes several such strings, each in its own font at
its own position along one baseline, as **one** text object, moving between
them with `Td` — what a mark drawn back over its letter, and the glyph after
it, need to stay one line to a reader that ends a line where a text object
ends. `glyph_run` on the
builder records which glyphs a document draws, so that `set_subset_fonts`
can embed only those.

**Fonts.** `add_base_font` names one of the standard 14; `add_named_font`
declares a non-embedded face by name and encoding; `add_embedded_font`
embeds a TrueType program as a simple font; `add_cid_font` embeds it as a
Type 0 / CIDFontType2 with `/Identity-H`, so any glyph index is addressable
(9.7.4). A CFF program — bare, or the `CFF ` table of an `OpenType/CFF`
face — goes under a CIDFontType0 instead, as 9.9 Table 126 requires, and a
CID-keyed one is written glyph by glyph as the CID its charset gives each
index, in the string, `/W` and `/ToUnicode` alike
([fonts](fonts.md)). With `set_subset_fonts(true)` an embedded TrueType face is cut to
the glyphs the document draws — `glyf`/`loca` rebuilt, composite components
followed, the 9.6.4 subset name tag written — so a line of Latin text set in
a CJK face costs kilobytes rather than the whole face.

**Images.** `add_image(resource, &ImageData)` with four shapes: `Jpeg`
(bytes placed verbatim, never re-encoded — recompression is generational
loss the caller cannot undo), `Rgb8` and `Gray8` (raw samples, deflated by
the writer), and `Compressed(CompressedImage)` — bytes already in the
encoding their dictionary declares. The last is what makes a 200-page comic
cost a multiple of its own size rather than *w × h × 3* a page: a
non-interlaced PNG's IDAT *is* a `/FlateDecode` stream with `/Predictor 15`,
byte for byte (PDF's predictor is PNG 9.2's row filter adopted wholesale),
and the writer never re-encodes a stream that already declares a `/Filter`.
`tinker_pdf_cos::build::jpeg_shape(bytes)` reads a JPEG's dimensions and
component count from its SOF marker so the caller need not. Indexed images take a `DeviceSpace` base
only — 8.6.6.3 forbids an `/Indexed` over `/Indexed`, and this writer offers
no other space as a base. It **does** write an `/ICCBased`
space: `add_icc_color_space` registers one as an indirect object with `/N`
checked against Table 66, `set_fill_icc` and `set_stroke_icc` name it on the
page, and `ImageColorSpace::Icc` puts it on an image — so one embedded
profile serves a page's operators and its pictures alike; an image whose
`components` is not the `/N` the space was registered with is refused, since
its rows would be the wrong width. The image's
`/ColorSpace` is a reference to the space's own array, not the resource name:
Table 89 makes it a colour space, and only a content stream's `cs` looks a
name up in `/Resources` (8.6.3). The name was written until September 2026,
and this reader drew those samples as grey.

**CIE-based spaces.** `add_cie_color_space(resource, &CieSpace)` registers a
`/CalGray`, `/CalRGB` or `/Lab` array (ISO 32000-1 8.6.5.2–8.6.5.4) as an
indirect object: the family name and a dictionary carrying `/WhitePoint`
always and every other parameter — `/BlackPoint`, `/Gamma`, `/Matrix`,
`/Range` — only where it differs from Tables 63–65's default, as the ICC
writer leaves `/Alternate` to its default. `CieSpace::is_valid` holds the
tables: a white point with `X` and `Z` positive and `Y` exactly 1, a black
point of non-negative numbers, positive gammas, a finite matrix written column
by column as Table 64 writes it, and a `/Range` whose minimums are below its
maximums — each held as the file will state it, since a real is written to
six places, so a white `X` or a gamma under half a millionth (written `0`) or
a `/Range` pair closer than that (written as one number) is refused; anything
else registers nothing. `set_fill_cie` and
`set_stroke_cie` write `/Name cs c1 … cn sc` with the space's own operand
count, each component clamped to the space's own range — 0..1 for `/CalGray`
and `/CalRGB`, and for `/Lab` `L*` to 0..100 and `a*`, `b*` to its `/Range`,
which is what a `/Lab` colour is and why it cannot share the 0..1 clamp — and
`ImageColorSpace::Cie { resource, components }` puts the space on an image,
refused when `components` is not its count; and `ImageColorSpace::Lab`, which
needs no registration, writes an inline `/Lab` over a D50 white whose `/Range`
is `[-128, 128 − 256/2^bits]`, so an offset-binary `a*` or `b*` sample is its
own value under Table 90's default decode — the encoding the TIFF door hands
a CIE L\*a\*b\* file over in. No `ArchivalProfile` refuses one:
a CIE-based space is device-independent, and 6.2.3.3 is about device colour.
`writer_cie.rs` reads each back through this reader — the image's
`ImageSpace` is the space written — and holds what the renderer draws to the
clauses' arithmetic, computed in the test: neutral colours to IEC 61966-2-1's
transfer of `Y`, and a chromatic `/Lab` colour through the published
Bradford-adapted sRGB matrix for a D50 white. Two reader defects were found on
the way and fixed with it: a `/Lab` image with no `/Decode` read its samples
as fractions of one rather than Table 90's `0..100` and `/Range`, so it was all
but black; and a `/ColorSpace` resource named `/G`, `/RGB` or `/CMYK` was
read as the inline-image abbreviation for the device space it spells, so a
`/CalGray` registered as `/G` was drawn in DeviceGray.

**Spot colours.** `add_separation_color_space(resource, colorant, alternate,
&tint)` registers `[/Separation /colorant /Alternate tint]` (8.6.6.4) and
`add_device_n_color_space(resource, &colorants, alternate, &tint,
attributes)` registers `[/DeviceN [...] /Alternate tint]` (8.6.6.5), each an
indirect array with the tint transform its own object. A `/Separation` takes
any one-input function — a type 2 ramp from no ink to full strength is the
ordinary one — and a `/DeviceN` takes a `Function::Calculator`, a type 4
PostScript calculator whose program is a `Vec<CalculatorOp>` value rather than
text: the builder checks that every operator is Table 42's, that none is
reached short of operands, that an `if` leaves the stack where it found it
and an `ifelse`'s arms agree, and that the program ends with one value per
alternate component, since a reader takes the last *n* and a program leaving
another count writes outputs nobody meant. A type 0 sampled function,
`Function::Sampled` — sixteen-bit samples and 7.10.2's multilinear
interpolation — is offered too, of any number of inputs: it is how XPS writes
a gradient blended in linear light and an `nCLR` profile's `/DeviceN` tint
transform. It was not offered while this reader evaluated a table along its
first input only, which it no longer does (3 October 2026). `DeviceNAttributes`
writes Table 71's `/Colorants` from separations registered earlier.
Colorant names are unique but for `/None`, at most 32 (Annex C), and never
`/All`, which 8.6.6.5 reserves for a `/Separation`.
`set_fill_tint` and `set_stroke_tint` write `/Name cs t1 … tn scn`, and
`ImageColorSpace::Tint { resource, components }` puts such a space on an
image — refused when `components` is not the space's colorant count. Under an
`ArchivalProfile` the alternate is the device colour a reader without the ink
paints, and is refused where the destination profile would refuse it. Under
parts 2 to 4, ISO 19005-2 6.2.4.4 binds two more: a `/DeviceN` naming a spot
colour — any colorant but `/None` and `/Cyan`, `/Magenta`, `/Yellow`, `/Black`
— that its `/Colorants` does not describe is refused
(`ArchivalRefusal::UndescribedColorant`), and so is a `/Separation` for a
colorant an earlier one named with another alternate or tint transform
(`InconsistentSeparation`), compared as the written objects and remembered
across a reused resource name, because a page begun before still draws with
the first.

**The operand count comes from the space rather than from the caller.** 8.6.5.5's
`/N` says how many operands `scn` takes, so four values against a three-channel
profile lose the fourth and one value gains two zeros; a tint space takes one
tint a colorant, and a missing tint is written as zero, which is no ink. A
content stream whose arity disagrees with its own space is one every reader has
to guess about, and the guesses differ.

**Graphics.** `add_ext_gstate` (`ExtGState`: blend mode, alphas, soft mask
with `MaskKind` and `StateMask`), `add_form` (`FormXObject`, optionally a
`TransparencyGroup`), `add_shading` (`Shading`: axial and radial with a
`Function`), `add_tiling_pattern` (`TilingPattern`, all three `TilingType`
values of Table 75), `add_shading_pattern` (`ShadingPattern`, `/PatternType 2`);
the page side applies them with `set_ext_gstate`, `form`,
`shading`, `set_fill_pattern` / `set_stroke_pattern`, plus `fill_rect`,
`set_fill_rgb` / `set_stroke_rgb`, `image`, and `raw(operators)` for
anything else. Each page-side call returns `false` when the named resource
was never added — a typo is a `false`, never a dangling `/Resources` entry.

**A resource name may be any bytes.** Every operator that names one — `Tf`,
`Do`, `gs`, `sh`, `cs`/`CS` and the pattern `scn`/`SCN` — writes it through
the same 7.3.5 escaper the dictionary keys go through, so a delimiter, white
space, `#` or a byte outside `!`..`~` is a `#xx` escape on both sides. Until
September 2026 the content-stream side wrote the bytes raw: `form(b"Fm B")`
wrote `/Fm B Do`, which is the name `/Fm` and a stray operand, and the page
drew nothing while its `/Resources` held the form under the whole name.

**Layers.** `add_layer(name, visible)` writes an optional content group
(8.11.2.1) and returns a `LayerId`; `optional(layer, |page| ...)` draws
inside `/OC /OCn BDC … EMC` (8.11.3.2) with the group in the page's
`/Properties`; `finish` writes `/OCProperties` — `/OCGs`, and a default
configuration whose `/Order` is the order the layers were added and whose
`/OFF` names the hidden ones. A layer inside a `tagged` element splits the
element's sequence around itself, so each `EMC` closes the scope it was
written for: without that, a child element's close would end the layer and
the child would draw in plain view with every byte balanced. For the same
reason the layer's closure is a scope for `open_tag`/`close_tag`, as
`tagged_with`'s is: `close_tag` inside it cannot close an element opened
outside it, and an element opened inside it and left open is closed when it
returns. A layer a page
cannot name — registered after the page was begun, or on another builder —
or one nested past `MAX_NEST_DEPTH`, is refused and its closure not run:
content drawn outside the layer it was meant for shows when the layer is
hidden. A `LayerId` carries which builder made it as well as its position,
because every builder's first layer is its zeroth and a position alone would
let another builder's first layer draw into this one's. The builder's number
is never written, so the bytes do not depend on it. Part 1 of ISO 19005 forbids optional content, and the
archival profile refuses a layer by that clause.

**Navigation.** `link(x0, y0, x1, y1, &Target)` adds a link annotation
(12.5.6.5); `set_outline(Vec<OutlineEntry>)` writes the document outline
(12.3.3). `Target` is `Page { index, view: DestKind }`, `Named(bytes)` or
`Uri(String)`, and exactly one of `/Dest` or `/A` is written, decided by the
variant — the malformed both-at-once shape is not expressible (ruling 6,
[rulings.md](../rulings.md)). A `Target::Page` past the last page writes
*no* destination rather than a link to whichever page was last.
`add_named_destination(name, index, view)` registers a name, and `finish`
writes the catalog's `/Names /Dests` tree (12.3.2.3) through the same tree
writer the editor uses; a `Target::Named` keeps the name in `/Dest` as a byte
string and reads back as `Destination::Named`, never flattened to the array
it stands for. A name unregistered at `finish`, or registered for a page
that never arrived, is dangling and refused — the link is not written, the
outline entry becomes a heading — and `dangling_destinations()` names each
one before the document is finished. An
`OutlineEntry` with `target: None` is a real shape — a part title above
three chapters — and `open` is written as the sign of `/Count` exactly as
12.3.3 spells it, only for entries that have children.

**Metadata.** `set_info(key, value)` writes the `/Info` dictionary (14.3.3).
Its values and the outline's titles are text strings, written by
`encode_text_string` for the version the document declares
([document-model](document-model.md)): PDFDocEncoding where it carries the
text, UTF-16BE where it does not, and UTF-8 instead of UTF-16BE in a
document made with `DocumentBuilder::with_version(2, 0)` or later. They were
written as bare UTF-8 bytes until September 2026, which 7.9.2.2 has every
reader decode as PDFDocEncoding — "Ä" read back as "Ã—", in this engine too.

**Version.** `DocumentBuilder::new()` declares 1.7; `with_version(major,
minor)` declares another, fixed at construction because text is encoded for
it as it arrives; an archival profile declares its part's version instead.
Declaring 2.0 is not conforming to it: what 2.0 deprecates (`/Info` beyond
the two dates, an unembedded standard font) is still written when asked for.

**Finish.** `finish()` serialises through the writer and returns the bytes
— the same writer every edited document goes through, so a built document
gets object streams, compression and every other writer property for free.

## API

```rust
use tinker_pdf::{DocumentBuilder, ImageData};

let mut b = DocumentBuilder::new();
b.add_base_font(b"F1", b"Helvetica");
b.add_embedded_font(b"F2", b"MyFace", &ttf_bytes);
b.set_subset_fonts(true);
b.add_image(b"Im0", &ImageData::Jpeg(&jpeg_bytes));

b.add_page(612.0, 792.0, |page| {
    page.text(b"F1", 12.0, 72.0, 720.0, "Hello");
    page.image(b"Im0", 72.0, 400.0, 200.0, 150.0);
    page.fill_rect(72.0, 72.0, 100.0, 20.0, 0.5);
});

b.set_info(b"Title", "Built");
let pdf: Vec<u8> = b.finish();
```

`DocumentBuilder`, `PageBuilder`, `ImageData`, `DeviceSpace`, `CieSpace`, `ExtGState`,
`TransparencyGroup`, `FormXObject`, `Function`, `CalculatorOp`,
`DeviceNAttributes`, `LayerId`, `Shading`, `ShadingPattern`,
`TilingPattern`, `TilingType`, `Glyph`, `PlacedGlyph`, `BlendMode`, `MaskKind`,
`StateMask`,
`Target`, `OutlineEntry` and `WriteOptions` are re-exported from the facade.
The HTML half is `FromHtml` (`from_html`, `from_html_with`, `from_markdown`), `PageBox`
(`new`, `with_margin`, `with_font_size`; `#[non_exhaustive]`), `HtmlReport`
(`warnings`, `pages`, `layout`, `margin`, `cost`) and `HtmlError`, all on the
facade:

```rust
use tinker_pdf::{DocumentBuilder, FromHtml, PageBox};

let (mut b, report) = DocumentBuilder::from_html(
    "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><h1>Invoice</h1></body></html>",
    "h1 { font-size: 20pt }",
    PageBox::new(612.0, 792.0).with_margin(54.0),
)?;
b.set_info(b"Author", "Accounts");
let pdf = b.finish();
```
`ImageData` and `Target` are `#[non_exhaustive]`: the next shape is an
addition, not a break.

**`ImageData::Compressed` is constructible from outside the workspace**, which
it was not: the enum crossed the facade and its payload did not, so the variant
was documented and unreachable — a shape worse than an absent one, because it
reads as a capability. `CompressedImage`, `ImageColorSpace`, `ImageFilter`,
`SoftMask` and `CcittParams` are re-exported now. The fifth is the one a grep
over the writer's own names does not find: `ImageFilter::CcittFax` carries
7.4.6's parameters as the *filters* crate's struct rather than a second
spelling of `/K`, so without it that variant could be neither built nor matched
on. The doctest on the re-export names all five through the facade and nothing
else, so deleting any one of them fails `cargo test --doc` rather than quietly
reopening the gap.

**`ImageFilter::Jpx` places a JPEG 2000 file as `/JPXDecode`**, bare
codestream or JP2 alike (7.4.9), and it is the one filter whose image
dictionary carries **no `/BitsPerComponent` and no `/ColorSpace`**: Table 89
makes both optional for it alone, the codestream states both, and a
`/ColorSpace` would override a JP2's own `colr` box. The two
`CompressedImage` fields are still filled in and checked — as a description
of what a decode produces — and are not written; a colour-key mask with it is
refused, having no stated depth to be ranges over. `ImageFilter` is not
`#[non_exhaustive]`, so the variant was a break, and the workspace's one
exhaustive match on it (the writer's own `/Filter` name) took the arm.

**The archival profile.** `DocumentBuilder::archival` takes an ISO 19005
profile and turns this whole surface into one that says no: the standard 14,
transparency under part 1, a device colour the output intent cannot reproduce
and an `/Info` entry part 4 has no room for are all refused where the caller
asks for them, and the finished document carries an output intent, a
byte-deterministic XMP packet and the header version its part requires.
[features/pdfa.md](pdfa.md) is the whole of it.

**PDF 2.0's page-level output intents.** In a document made with
`DocumentBuilder::with_version(2, 0)`, `PageBuilder::output_intent(NewOutputIntent)`
gives a page its own `/OutputIntents` entry — `/S`, the required
`/OutputConditionIdentifier`, `/OutputCondition`, `/RegistryName`, `/Info` and
an ICC `/DestOutputProfile` whose `/N` comes from the `DeviceSpace` declared
beside it — and pages naming one profile share one stream. It returns `false`
below 2.0, under an archival profile (which writes the catalog's intent and
judges every device colour against that one), and for an empty `/S`,
identifier or profile. `Page::output_intents` reads it back
([document-model](document-model.md)).

**Associated files** (ISO 32000-2 14.13). `DocumentBuilder::associate_file`,
`PageBuilder::associate_file` and `Tag::associated_file` take a
`NewAssociatedFile` — filename, MIME type, `FileRelationship`, optional
description, bytes — and write it into the holder's `/AF`: an embedded file
stream with `/Subtype` and `/Params` (`/Size`, MD5 `/CheckSum`), under an
indirect file specification carrying `/F`, `/UF`, `/Desc`, `/EF` and
`/AFRelationship`. They are written in a document declaring 2.0, or under
ISO 19005-3, which carried them on 1.7 first — a part 3 document carrying one
validates with no finding (`a_part_3_document_carries_an_associated_file_and_validates`).
Other parts record `ArchivalRefusal::AssociatedFile`. A structure element
holding nothing but an associated file is kept, as one holding any other
property is.

Four more holders, October 2026, each from the Arlington model's list.
`PageBuilder::associate_file_with_link(index, file)` gives the page's
`index`th accepted link its `/AF`; `DocumentBuilder::add_form_with_files`
is `add_form` with an `/AF` on the form, refusing the whole registration
when a file cannot be written, since a form without the files asked for is
not the form asked for; `DocumentBuilder::associate_file_with_structure`
puts one on the structure tree root, which is written only with the tree — a
document that tags nothing has no root, and writing one would claim a
structure (`/MarkInfo /Marked true` comes with it) the document does not
have. `PageBuilder::with_associated_files(files, draw)` draws a
marked-content sequence associated with files: `/AF /AFn BDC … EMC` with a
direct `<< /MCAF [...] >>` under `/Properties /AFn`, the shape of the
errata's 14.13.5 EXAMPLE (Table 409a), nesting with structure elements and
layers as `optional` does — the two now share one writer, byte for byte —
and writing nothing for a closure that drew nothing. Where the document may
not carry associated files, or a file is refused, the closure still draws,
outside any `/AF` sequence, and the call answers `false`
(`the_new_holders_are_refused_where_an_associated_file_cannot_be_written`).

## Refused by name

| What | How it shows | Why | See |
| --- | --- | --- | --- |
| Any image encoder but deflate | `Rgb8`/`Gray8` are deflated; JPEG and PNG-IDAT pass through; nothing is encoded to JPEG, CCITT, JBIG2 or JPX | **The reason has now changed twice and the refusal has not.** It used to be "the engine decodes those codecs; it does not write them"; on 15 September 2026 `tinker-pdf-filters` gained CCITT G4 and JBIG2 generic regions, and on 16 September a baseline JPEG encoder, so three of the four codecs named here have a writer in the leaf crate and this builder calls none of them. What is missing is not a coder. It is the decision of *when* a raster is better off lossy, or as a fax coding, than as deflate — and, per codec, the framing: for JBIG2 D.3's embedded-stream assembly, which the filter crate deliberately does not write, and for JPEG the `/DCTDecode` XObject's own colour and `/Decode` agreement. JPX has no encoder at all | [filters](filters.md), [ROADMAP](../ROADMAP.md) |
| Text shaping in `text` and `glyphs` | `text` is one byte per character; `glyphs` takes glyph indices the caller positioned | neither runs GSUB or GPOS: `glyph_run` is the shaped entry point, through `tinker-pdf-shape`, and shaped text in one call is a roadmap row | [fonts](fonts.md), [design/shaping.md](../design/shaping.md), [ROADMAP](../ROADMAP.md) ED-45 |
| A `Target::Uri` outside 7-bit ASCII | `link` returns `false` | 12.6.4.7's `/URI` is ASCII; an unwritable target writes nothing rather than a plausible-and-wrong action | [ROADMAP](../ROADMAP.md) ED-47 |
| Layout on the builder's own methods | none — positions are the caller's | a programmatic layout API is [ROADMAP](../ROADMAP.md) ED-44 since 9 October 2026; [epub](epub.md)'s layout engine is a *consumer* of this API, and `FromHtml` is where a caller reaches it: a constructor in the facade, because ruling 8 keeps the layout engine out of `tinker-pdf-cos` | — |
| A cascade or layout cap spent | `HtmlError::{StyleRefused, LayoutRefused}` | a book keeps the page as a placeholder; a creation call has no page count to keep, so it is refused by which half refused it | [epub](epub.md) |
| Everything an `ArchivalProfile` forbids | the call returns `false` and pushes a typed `ArchivalRefusal` naming its clause; `finish_archival` returns `Err` for what only a finished document can be judged on | a builder that emitted what the validator rejects would make the validator the last line of defence rather than the second | [pdfa](pdfa.md) |

## Verified

- `crates/tinker-pdf-cos/src/build.rs` carries its unit tests beside the
  code; `crates/tinker-pdf/tests/writer_graphics.rs` and
  `writer_navigation.rs` exercise ExtGStates, patterns, shadings, forms,
  links and outlines through the facade and read the result back through
  the [document model](document-model.md).
- `crates/tinker-pdf/tests/writer_layers.rs` reads every written layer back
  through `Document::layers()` with its name and default, renders a hidden
  layer unpainted until the editor shows it, renders an element in a hidden
  layer inside a visible element hidden while its parent's pieces are not,
  reads nested tagged and optional content in structure order with nothing
  orphaned, and holds a layered level A document to the PDF/A validator;
  `build.rs`'s `layer_tests` pin the split sequences byte for byte.
- `crates/tinker-pdf/tests/writer_tints.rs` renders `/Separation` and
  `/DeviceN` fills, strokes and images and holds every sampled pixel to the
  tint transform's own arithmetic — `c0 + t (c1 − c0)` for a ramp, the
  program's formula for a calculator — through `round(v × 255)`, exactly,
  with the strict validator clean, and holds the archival refusals — the
  alternate the output intent cannot reproduce, and ISO 19005-2 6.2.4.4's
  undescribed spot colour and disagreeing `/Separation`, each admitted once
  the document says what it should; `build.rs`'s `tint_tests` hold the
  arrays, the type 4 stream, each calculator refusal, `/All` in a `/DeviceN`
  and an ICC image whose count is not the space's `/N`.
- `crates/tinker-pdf/tests/writer_names.rs` holds each name-taking operator
  to one awkward name — a space, a `#`, a `/` and a byte past 0x7F: the
  stream carries the escaped token, the resource dictionary the name's own
  bytes, and the page draws what the same document draws under a plain name.
- `crates/tinker-pdf/tests/html_creation.rs` holds `from_html` **by the EPUB
  reftests**, two ways: a document made from markup and a stylesheet renders
  pixel for pixel to the EPUB whose chapter is the same markup with the sheet
  linked first, at two page boxes and over two pages; and nine of
  `epub_reftest.rs`'s pairs — the `margin`, `padding` and `border` shorthands
  against their longhands, `1.5em` against `24px`, `50%` against `120px`, an
  implied row group, `display: block`, a collapsed margin pair and padding
  against border — are laid out through `from_html` and read back out of the
  finished PDF line by line, each with the mismatch reference that must not
  agree. Beside them: the caller's sheet ahead of the document's and
  `!important` beating it, the page box and margin, an unusable box and margin
  named, a cascade cap refused as `StyleRefused` — the markup's element count,
  and the caller's own sheet past its rule or byte cap, which before the
  lane's review was dropped with no warning — and a provider answering a
  `<link>`. `hostile_input.rs` runs `from_html` over its damaged markup.
- `crates/tinker-pdf/tests/png_passthrough.rs` asserts a PNG's IDAT reaches
  the page untouched and decodes identically.
- `crates/tinker-pdf-cos/tests/page_operations.rs` covers embedded and
  subset fonts: subsetting is asserted by glyph count
  (`the_glyphs_that_were_drawn_survive_subsetting`), by the 9.6.4 tag
  (`a_subset_font_name_carries_its_tag`), by `/Length1` describing the
  subset rather than the original, and by the same document subsetting to
  the same bytes.
- Three determinism byte-hashes pin builder output: a synthesised PDF, a
  synthesised fixed document and a synthesised book hash as *bytes*
  ([determinism](determinism.md)), so object numbering, key order and
  stream framing cannot drift silently.
- Every built document is held to the strict validator through the
  container-format suites (`cbz_validated.rs`, `xps_validated.rs`,
  `epub_validated.rs`), since those
  formats produce their PDFs through this API.
