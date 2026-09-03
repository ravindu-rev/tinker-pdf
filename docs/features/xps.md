# XPS

An `.xps` or `.oxps` package opens as a `Document` whose pages are its fixed
pages, at the size their markup states. The package is turned into a real
PDF at `open` through [`DocumentBuilder`](creation.md), so nothing below the
facade is XPS-shaped and every capability the engine has — rendering, text
extraction, editing, saving — arrives with it. Both dialects are read: XPS
1.0 (`schemas.microsoft.com/xps/2005/06`) and OpenXPS (ECMA-388,
`schemas.openxps.org/oxps/v1.0`), through one reader.

## What it does

**Telling an XPS from every other ZIP.** `PK\x03\x04` at offset zero covers
CBZ, XPS, EPUB, ODF, OOXML and every JAR ever built. ECMA-388 E.3's
three-step test decides: the bytes are a ZIP; `[Content_Types].xml` *and*
`_rels/.rels` both exist; `_rels/.rels` parses and carries a
fixed-representation relationship of either dialect whose target resolves
to a part whose media type is the FixedDocumentSequence one. A comic archive
that happens to carry a content-types part is still a comic archive — step
2 wants both items. One refinement is deliberate: a package with no fixed
representation at all (a `.docx`, an `.odt`) is "not an XPS" and falls
through to the [CBZ](cbz.md) path as E.3 says; a fixed representation that
is *there and will not resolve* is `ArchiveRefusal::UnreadablePackage`,
because paging a broken XPS's images as a comic would be the original
defect in a smaller hat. The content type does not discriminate the dialect
— measured: Windows' OpenXPS output carries Table D-4's `xps-` strings byte
for byte — so the namespace is the only discriminator, on elements and on
relationship types, and a package mixing the two is accepted.

**The package layer** (`opc`): OPC part naming with 7.2.3.5's
case-insensitive comparison (`.ODTTF` against `odttf` is real), content
types by default and override, relationships parsed per source part,
relative references resolved against the referring part's name — on markup
attributes (`ImageSource`, `FontUri`) as well as relationship targets,
because XPS 1.0 writes them absolute and OpenXPS writes them relative. The
spine is `FixedDocumentSequence → FixedDocument → FixedPage`, pages in
markup order (12.3.1 defines no other), each payload resolved by media type
rather than by extension.

**The XML parser** is a leaf crate, `tinker-pdf-xml`: a pull parser that
refuses `<!DOCTYPE` with an internal subset *by name* before reading one
byte past it — entity expansion is a refusal, not a budget — with a
two-valued doctype mode that [EPUB](epub.md) uses for `<!DOCTYPE html>`.
Every parse is bounded by `Limits` (depth, attribute count, text length).

**Markup to content stream** (`markup`, `geometry`, `brush`, `paint`). One
XPS unit is 1/96 inch (18.1) against PDF's 1/72, so a page opens with one
`cm` carrying the unit and the top-left origin, and the numbers in the
stream are the numbers in the markup. 11.2.3's abbreviated geometry syntax
in both spellings real producers emit (`M0,0L200,0` and `M 0,0 L 200,0`);
`Path`, `Canvas`, `RenderTransform`, `Clip`, `Opacity`, resource
dictionaries with `{StaticResource}` lookup bounded against cycles and
depth — in the page and in 14.2.4's **separate part**, whose `Source`
chain is bounded the same two ways and read in a pass before the drawing
walk; 15.2.5's `ContextColor`, whose ICC profile is embedded **verbatim**
as an `/ICCBased` colour space and whose components reach the content
stream unchanged, so the reader does the colour management and this build
converts nothing; section 15's brushes — `SolidColorBrush`, `LinearGradientBrush`,
`RadialGradientBrush`, `ImageBrush` and `VisualBrush` with `TileMode`
(through a PDF tiling pattern, whose cell is a picture for the one and a
**drawing** for the other) — with colours in both the eight- and
six-digit spellings. A
gradient strokes and sets text through a `/PatternType 2` shading pattern,
because 8.7.4.1's `sh` floods a clip and neither a stroke nor a glyph
outline is one. 14.3's `OpacityMask` is a brush used as an **alpha
channel**, and the three brushes it can be are three constructions: a
uniform alpha is 11.6.4.4's `/ca`, a gradient whose stops' alphas differ is
painted as a grey for a `/Luminosity` soft mask, and a picture's own alpha
is read by an `/Alpha` one. `ContentBox` and `BleedBox` (10.3) survive as
`/CropBox` and `/BleedBox`.

**Glyphs** (`glyphs`, `font`). A `<Glyphs>` run carries `Indices` (12.1.3)
— glyph index, advance and offsets per cluster, including the empty-index
form `",53"` — and a `FontUri`. Fonts are ODTTF-obfuscated (9.1.7.3): the
key is the part's GUID, sixteen bytes reversed, XORed over the first 32
bytes; the de-obfuscated program is asserted byte-for-byte against a real
file's known sfnt header. Because XPS addresses every glyph by index, the
writer gained Type 0 / CIDFontType2 with `/Identity-H` for this format.
12.1's `IsSideways` turns the run a quarter turn and runs its baseline
**down** the page, which is 18.1's own text matrix with its two axes
exchanged — the glyphs, their advances and their order are unchanged, and
the box the run occupies turns with it. An odd `BidiLevel` is a
right-to-left run: the text goes through `tinker-pdf-shape`'s UAX #9 with
the level's parity as the base direction, and the **clusters** are laid in
the visual order that comes back, with the origin at the run's right edge.
The algorithm rather than a reversal, because European digits inside a
Hebrew or Arabic run rise to an even level of their own and are drawn left
to right inside a run drawn right to left — a reversal renders every price
backwards. Bold and italic simulation (`StyleSimulations`) is reported,
not applied.

**Images** pass through. A PNG or JPEG resource part reaches the page as
the bytes it is ([creation](creation.md), `ImageData::Compressed`); a page
whose decoded raster would be 17.2 MB costs 0.2 MB above baseline to
synthesise, measured rather than argued. **9.1.5's TIFF joins them**, and
mostly passes through as well: four of TIFF 6.0's codings already have a
`/Filter` name, so a single-strip file of any of them is placed rather than
decoded ([filters](filters.md)). A resolution stated in the directory's
`XResolution`, `YResolution` and `ResolutionUnit` becomes the part's dpi,
where 13.4.1's 96 stands in when the file states none — which is the first
consumer those three tags have had.

**Everything owed is owed at the element.** A page that cannot be read is a
placeholder page that keeps its number, carrying an `XpsPageDefect`; an
element the reader cannot honour is drawn around and named in an
`XpsElementDefect` — never a page that quietly drew most of itself.

## API

```rust
let doc = tinker_pdf::Document::open(xps_bytes)?;   // routed by ECMA-388 E.3
let report = doc.archive().expect("a container format");
for warning in report.warnings() { /* ArchiveWarning::XpsPage / XpsElement */ }
let page = doc.page(0).unwrap();
let bitmap = page.render(&tinker_pdf::RenderOptions::at_dpi(96.0));
let text = page.text().plain_text();                 // from the Glyphs runs
let pdf = doc.editor().save(&Default::default());    // the synthesised PDF
```

`Document::open` / `open_with` route automatically; `Document::archive()`
returns the `ArchiveReport` with per-page origins and every warning.
`tinker_pdf::xps` exposes `Dialect`, `Limits`, `XpsPageDefect`,
`XpsElementDefect` and the `opc`, `markup`, `geometry`, `brush`, `glyphs`,
`paint` and `font` modules for callers that want the package layer without
the page synthesis.

## Refused by name

| What | Typed variant | Why | See |
| --- | --- | --- | --- |
| A `ContextColor` whose profile takes a channel count `/ICCBased` cannot state | `XpsElementDefect::ColourProfileChannels` | Table 66 permits **1, 3 or 4** components and ICC.1's `nCLR` family runs to fifteen; `/DeviceN` would need a tint transform only *evaluating* the profile could supply, which is the colour engine this build does not have. **Painted grey**, rather than in a colour picked by dropping components | [colour](colour.md) |
| A `ContextColor` whose profile part is missing, is not a profile, or names a data space ICC.1 does not | `XpsElementDefect::ColourProfileUnresolved` | **Still painted**, in 8.6.5.5's default-`/Alternate` reading of the components — one channel grey, three RGB, four CMYK. Not an invention: that is what a reader does with an `/ICCBased` stream it cannot use, and the numbers are the file's | [colour](colour.md) |
| A `ContextColor` in a **gradient stop** | `XpsElementDefect::BrushApproximated` | 8.7.4.5's shading names one colour space for the whole function, so a stop cannot carry one of its own; it takes the same alternate reading and the brush says it reached the page and not exactly | — |
| An image part no rule identifies | `XpsElementDefect::ImageFormatUnsupported` | a part neither the content type nor the magic bytes name is not one to guess at. **JPEG XR left this row**: 9.1.5.1's format now decodes and draws, so all four of 9.1.5's formats reach the page and the pre-emptive refusal loop that used to sit in front of both identification rules is gone | [filters](filters.md) |
| A content type and magic bytes that disagree about two formats this build draws | *(none — the bytes win, unnamed)* | a decoder reads bytes, so the bytes decide; `Images::get` returns a `Result`, so the only channel out is a refusal and a leniency has nowhere to go. Ruling 10 wants it named and this does not name it — pinned by `a_content_type_that_disagrees_with_the_bytes_draws_the_bytes_and_says_nothing`. **Wiring JPEG XR made this worse rather than better**: with all four formats drawn there are now twelve ordered pairs that take the arm where there were two. Closing it needs a leniency variant on `XpsElementDefect` and a push into `paint.rs`'s `defects` | [rulings](../rulings.md) ruling 10 |
| `{ColorConvertedBitmap …}` naming an ICC profile | `XpsElementDefect::ImageProfileUnsupported` | *not* for want of an ICC pipeline — a `ContextColor` embeds its profile as an `/ICCBased` space — but because 8.9.5's image dictionary takes a `/ColorSpace` and this writer's `add_image` states only the three device families, so there is nowhere to put the profile. The picture is refused rather than drawn in colours the file did not ask for | [colour](colour.md) |
| `StyleSimulations` | `XpsElementDefect::GlyphsStyleSimulated` | reported; glyphs drawn unsimulated | — |
| Gradient stops with differing alphas; a `ColorInterpolationMode` this build does not interpolate in | `XpsElementDefect::BrushApproximated` | the brush reached the page and not exactly — one constant alpha cannot express per-stop alphas — and the approximation is named | — |
| Unknown element | `XpsElementDefect::ElementUnknown` | drawn around, never silently skipped | — |
| Broken fixed representation, interleaved parts, invalid or ambiguous part names, no fixed pages | `ArchiveRefusal::{UnreadablePackage, Interleaved, InvalidPartName, AmbiguousPartNames, NoFixedPages}` | a package that *is* an XPS and is broken is refused, not paged as a comic | [cbz](cbz.md) |
| Page-level defects | `XpsPageDefect::{SourceUnresolved, DocumentUnresolved, Unreadable, ContentUnreadable, SizeUnusable, MediaTypeMismatch, PageBoxUnusable}` | the page becomes a placeholder that keeps its number | — |
| Signatures, print tickets, 3D, story fragments | not read | parts the spine does not reach are ignored | — |

### Decoded but unadjudicated

A refusal is a decision and every row above is reached by a test. This is the
other list: configurations that **decode** and that nothing here checks the
*correctness* of. Named rather than counted as covered, the treatment
[fonts.md](fonts.md) gives shaped-but-unverified scripts.

All of them are JPEG XR, because it is the only one of 9.1.5's four formats
with no second implementation available to this repository. Ruling 13 is why
that matters and why the list will not shrink by testing harder: a third party
may generate an input and may never adjudicate an output, so "decode it with
something else and compare" is not a check this project can make. The full
reasoning, and the evidence that *is* available, is in
[design/jpeg-xr.md](../design/jpeg-xr.md).

| What decodes unchecked | Why nothing checks it |
| --- | --- |
| **The quantised lossy path, in general** | The lossless identity pins ITU-T T.832 9.8's dequantization at QP 1 and says nothing about `QuantMap( )` at any other quantizer. The seam property is a *relative* comparison within one image, so a filter wrong by a constant everywhere passes it. The transform round trip is blind to an error mirrored into both directions. Monotonicity only orders three error totals. A decoder exactly right losslessly and wrong at every other quantizer passes everything here |
| The first-level overlap filter across a **soft tile boundary** | Needs a multi-tile image at `OVERLAP_MODE` 2; both tiled fixtures were encoded at mode 1. It is also the one path where 9.9.3.2's text disagrees with its own geometry, and where this build follows the geometry |
| `HARD_TILING_FLAG` | The Windows encoder does not expose it, so both settings cannot be produced on this machine and only the soft-tile path has a fixture |
| `SHIFT_BITS`, `TRIM_FLEXBITS`, more than one QP per tile | Every fixture carries the value at which each of these does nothing, so the code paths run only in their degenerate form |
| A damaged codestream's **values** | `fuzz_jxr` proves a damaged file does not panic and that a dropped tile is reported; it does not check that what survives is what a conformant decoder would produce |

**What *is* checked, so that the list above is read in proportion**: fifteen
committed fixtures — eight pixel formats, all three overlap modes, both tiled
layouts, both frequency-mode layouts and both alpha formats — decode
**bit-for-bit** to rasters this repository authored, through a Windows encoder
that supplied bytes and never judged them.

**What the corpus is**: thirteen committed packages from **three serialisers
and two vendors** — six WPF `.xps`, two XPSOM `.oxps`, and five written by
Ghostscript 10.07.1's `xpswrite` device over PDFs this repository's own
`DocumentBuilder` wrote. The third producer shares no code, no vendor and no
lineage with the first two, and it earned its place: fifteen of the thirty
things recorded in `crates/tinker-pdf/tests/xps/README.md` came from it, and
three of them contradict something the eight Microsoft files had made look like
a rule — `[Content_Types].xml` is not always last, colours come in three
spellings and not two, and abbreviated geometry does too. One of its packages,
`gs-images.xps`, is refused at two elements by the table above and is the
fixture the TIFF row should start from: its image parts are `image/tiff`,
reached through `{ColorConvertedBitmap …}` naming an ICC profile part.

## Verified

- **Thirteen genuine packages** in `crates/tinker-pdf/tests/xps/` — six WPF
  `.xps` (`System.Windows.Xps.Packaging`, .NET Framework 4.8), two XPSOM
  `.oxps` (`XpsServices.dll` 10.0.26100) and five from Ghostscript 10.07.1's
  `xpswrite` device. The first eight were obtained *before* any reader existed
  so the reader could be measured against files it did not write; the five came
  later, from a second vendor, over source PDFs `DocumentBuilder` wrote.
  Twelve render with nothing owed and the thirteenth is refused at exactly two
  elements, by name. `INVENTORY.tsv` is recomputed through `tinker-pdf-zip` on
  every `cargo test`, so .NET's ZIP reader and this one must agree about all 82
  rows. **Thirty** things real files did that ECMA-388 does not say are
  recorded in that README.
- **The Ghostscript half regenerates byte for byte**, which nothing else in this
  repository's fixture corpora does: `xpswrite` stamps a fixed ZIP timestamp,
  stores every entry, and derives its one relationship `Id` from the content, so
  a deterministic writer on the way in gives an auditable corpus on the way out.
  `tests/xps_corpus_source.rs` holds the input half of that with a test that
  runs on every `cargo test`.
- `tests/xps.rs`, `xps_spine.rs` (a package whose storage, name and markup
  orders differ), `xps_opc.rs`, `xps_markup.rs`, `xps_glyphs.rs`
  (de-obfuscation asserted byte-for-byte), `xps_images.rs`,
  `xps_memory.rs` (synthesis cost measured), `xml_real_packages.rs`.
- `xps_validated.rs`: every synthesised PDF is held to the strict validator,
  in every write mode, and its gradients, transparency groups, boxes and
  composite fonts are read back out of the dictionaries — the entries this
  engine's own typed readers supply a default for. It replaces the qpdf oracle
  ruling 13 retired; what left with it is that a reader nobody here wrote
  accepts the file.
- `xps_conservation.rs`: what the markup states is what the document has, in
  order, at the place 18.1 puts it — page sizes, fills and their colours,
  gradient geometry and stops, image pixel counts and rectangles, tiling
  copies, and glyph runs with their origins, sizes, text and the advances
  `Indices` overrides. The markup side is walked by the test's own scanners,
  which reach for `tinker-pdf-zip` and nothing above it: not the XML parser
  this reader parses with, not the image decoders that give a picture its pixel
  count, not `geometry`'s reader of 11.2.3, and not 18.1's scale and flip,
  written out from the clause. Twelve of the thirteen packages conserve every
  fact and the census is recorded in `tests/xps/CONSERVATION.tsv`; the
  thirteenth, `gs-images.xps`, states two pictures this build refuses at the
  element, so its divergence is pinned by a test of its own rather than
  censused. It replaces
  `xps_mutool.rs`, which compared a second reader's device trace of the package
  against its trace of the document. **What left with that oracle is that two
  independent programs read one package and agree**, and no assertion here
  reproduces it: reader, harness and reviewer share one reading of ECMA-388, so
  a consistent misreading of it now survives
  ([verification](../verification.md), [ROADMAP](../ROADMAP.md)).
- The `xps` determinism fingerprint and the fixed-document byte-hash
  ([determinism](determinism.md)) — the first fixture whose input this
  repository did not author.
- Fuzz targets `xml` and `zip_archive`; the `bounds_ledger.rs` rows for
  every XPS cap, measured against the real packages.
