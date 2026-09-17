# Creation

`DocumentBuilder` is a thin authoring layer over the [writer](writing.md):
pages, content, fonts, images, graphics state, patterns, shadings, links,
metadata and an outline. It deliberately does no layout — placing what a
caller already positioned is its business, composing paragraphs is not. The
container formats use it: a CBZ, XPS or EPUB becomes a PDF through exactly
this API, which is why it carries what those formats needed (CID fonts,
ExtGStates, patterns, links, outlines) and nothing speculative.

## What it does

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
that already measured every run (EPUB) sets text. `glyph_run` on the
builder records which glyphs a document draws, so that `set_subset_fonts`
can embed only those.

**Fonts.** `add_base_font` names one of the standard 14; `add_named_font`
declares a non-embedded face by name and encoding; `add_embedded_font`
embeds a TrueType program as a simple font; `add_cid_font` embeds it as a
Type 0 / CIDFontType2 with `/Identity-H`, so any glyph index is addressable
(9.7.4). With `set_subset_fonts(true)` an embedded TrueType face is cut to
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
only — 8.6.6.3 forbids an `/Indexed` over `/Indexed`, and this writer emits
no CIE, `/Separation` or `/DeviceN` space. It **does** write an `/ICCBased`
space: `add_icc_color_space` registers one as an indirect object with `/N`
checked against Table 66, `set_fill_icc` and `set_stroke_icc` name it on the
page, and `ImageColorSpace::Icc` names it on an image (8.9.5.4) — so one
embedded profile serves a page's operators and its pictures alike.

**The operand count comes from the space rather than from the caller.** 8.6.5.5's
`/N` says how many operands `scn` takes, so four values against a three-channel
profile lose the fourth and one value gains two zeros. A content stream whose
arity disagrees with its own space is one every reader has to guess about, and
the guesses differ.

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

**Navigation.** `link(x0, y0, x1, y1, &Target)` adds a link annotation
(12.5.6.5); `set_outline(Vec<OutlineEntry>)` writes the document outline
(12.3.3). `Target` is `Page { index, view: DestKind }` or `Uri(String)`, and
exactly one of `/Dest` or `/A` is written, decided by the variant — the
malformed both-at-once shape is not expressible (ruling 6,
[rulings.md](../rulings.md)). A `Target::Page` past the last page writes
*no* destination rather than a link to whichever page was last. An
`OutlineEntry` with `target: None` is a real shape — a part title above
three chapters — and `open` is written as the sign of `/Count` exactly as
12.3.3 spells it, only for entries that have children.

**Metadata.** `set_info(key, value)` writes the `/Info` dictionary (14.3.3).

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

`DocumentBuilder`, `PageBuilder`, `ImageData`, `DeviceSpace`, `ExtGState`,
`TransparencyGroup`, `FormXObject`, `Function`, `Shading`, `ShadingPattern`,
`TilingPattern`, `TilingType`, `Glyph`, `PlacedGlyph`, `BlendMode`, `MaskKind`,
`StateMask`,
`Target`, `OutlineEntry` and `WriteOptions` are re-exported from the facade.
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

**The archival profile.** `DocumentBuilder::archival` takes an ISO 19005
profile and turns this whole surface into one that says no: the standard 14,
transparency under part 1, a device colour the output intent cannot reproduce
and an `/Info` entry part 4 has no room for are all refused where the caller
asks for them, and the finished document carries an output intent, a
byte-deterministic XMP packet and the header version its part requires.
[features/pdfa.md](pdfa.md) is the whole of it.

## Refused by name

| What | How it shows | Why | See |
| --- | --- | --- | --- |
| Any image encoder but deflate | `Rgb8`/`Gray8` are deflated; JPEG and PNG-IDAT pass through; nothing is encoded to JPEG, CCITT, JBIG2 or JPX | **The reason has now changed twice and the refusal has not.** It used to be "the engine decodes those codecs; it does not write them"; on 15 September 2026 `tinker-pdf-filters` gained CCITT G4 and JBIG2 generic regions, and on 16 September a baseline JPEG encoder, so three of the four codecs named here have a writer in the leaf crate and this builder calls none of them. What is missing is not a coder. It is the decision of *when* a raster is better off lossy, or as a fax coding, than as deflate — and, per codec, the framing: for JBIG2 D.3's embedded-stream assembly, which the filter crate deliberately does not write, and for JPEG the `/DCTDecode` XObject's own colour and `/Decode` agreement. JPX has no encoder at all | [filters](filters.md), [ROADMAP](../ROADMAP.md) |
| Text shaping in `text` and `glyphs` | `text` is one byte per character; `glyphs` takes glyph indices the caller positioned | neither runs GSUB or GPOS and neither will: `glyph_run` is the shaped entry point, through `tinker-pdf-shape` | [fonts](fonts.md), [design/shaping.md](../design/shaping.md) |
| CIE, `/Separation` and `/DeviceN` on write | `DeviceSpace` and `/ICCBased` on the fill, stroke and image setters | a `/Separation` or `/DeviceN` space is a tint transform into an alternate space, and this writer emits no function for one. **ICC is not in this row**: `add_icc_color_space` registers a space, `set_fill_icc` and `set_stroke_icc` name it, and `ImageColorSpace::Icc` puts it on an image | [ROADMAP](../ROADMAP.md) |
| A `Target::Uri` outside 7-bit ASCII | `link` returns `false` | 12.6.4.7's `/URI` is ASCII; an unwritable target writes nothing rather than a plausible-and-wrong action | — |
| Layout | none — positions are the caller's | by design; [epub](epub.md)'s layout engine is a *consumer* of this API | — |
| Everything an `ArchivalProfile` forbids | the call returns `false` and pushes a typed `ArchivalRefusal` naming its clause; `finish_archival` returns `Err` for what only a finished document can be judged on | a builder that emitted what the validator rejects would make the validator the last line of defence rather than the second | [pdfa](pdfa.md) |

## Verified

- `crates/tinker-pdf-cos/src/build.rs` carries its unit tests beside the
  code; `crates/tinker-pdf/tests/writer_graphics.rs` and
  `writer_navigation.rs` exercise ExtGStates, patterns, shadings, forms,
  links and outlines through the facade and read the result back through
  the [document model](document-model.md).
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
