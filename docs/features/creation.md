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
no CIE, `/Separation` or `/DeviceN` space.

**Graphics.** `add_ext_gstate` (`ExtGState`: blend mode, alphas, soft mask
with `MaskKind` and `StateMask`), `add_form` (`FormXObject`, optionally a
`TransparencyGroup`), `add_shading` (`Shading`: axial and radial with a
`Function`), `add_tiling_pattern` (`TilingPattern`, all three `TilingType`
values of Table 75); the page side applies them with `set_ext_gstate`, `form`,
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
`TransparencyGroup`, `FormXObject`, `Function`, `Shading`, `TilingPattern`,
`TilingType`, `Glyph`, `PlacedGlyph`, `BlendMode`, `MaskKind`, `StateMask`,
`Target`, `OutlineEntry` and `WriteOptions` are re-exported from the facade.
`ImageData` and `Target` are `#[non_exhaustive]`: the next shape is an
addition, not a break.

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
| CFF / OpenType-CFF subsetting | a CFF face embeds whole | charstring subsetting with subroutine renumbering is not written | [ROADMAP.md](../ROADMAP.md) |
| Any image encoder but deflate | `Rgb8`/`Gray8` are deflated; JPEG and PNG-IDAT pass through; nothing is encoded to JPEG, CCITT, JBIG2 or JPX | the engine decodes those codecs; it does not write them | [filters](filters.md) |
| Text shaping | `text` is one byte per character; `glyphs` takes glyph indices the caller positioned | no GSUB/GPOS; a stated non-goal now reopened | [ROADMAP.md](../ROADMAP.md), [fonts](fonts.md) |
| Non-device colour spaces on write | `DeviceSpace` only (`/DeviceGray`, `/DeviceRGB`, `/DeviceCMYK`) | no CIE, ICC, `/Separation` or `/DeviceN` writer | — |
| A `Target::Uri` outside 7-bit ASCII | `link` returns `false` | 12.6.4.7's `/URI` is ASCII; an unwritable target writes nothing rather than a plausible-and-wrong action | — |
| `ImageData::Compressed` from outside the workspace | `CompressedImage`, `ImageColorSpace`, `ImageFilter` and `SoftMask` are not re-exported by the facade, so the variant cannot be constructed by an external caller | the container formats use it internally; the facade re-export is owed and not yet on the roadmap | — |
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
