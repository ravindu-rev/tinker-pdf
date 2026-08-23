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
depth; section 15's brushes — `SolidColorBrush`, `LinearGradientBrush`,
`RadialGradientBrush`, `ImageBrush` with `TileMode` (through a PDF tiling
pattern) — with colours in both the eight- and six-digit spellings.
`ContentBox` and `BleedBox` (10.3) survive as `/CropBox` and `/BleedBox`.

**Glyphs** (`glyphs`, `font`). A `<Glyphs>` run carries `Indices` (12.1.3)
— glyph index, advance and offsets per cluster, including the empty-index
form `",53"` — and a `FontUri`. Fonts are ODTTF-obfuscated (9.1.7.3): the
key is the part's GUID, sixteen bytes reversed, XORed over the first 32
bytes; the de-obfuscated program is asserted byte-for-byte against a real
file's known sfnt header. Because XPS addresses every glyph by index, the
writer gained Type 0 / CIDFontType2 with `/Identity-H` for this format.
Bold and italic simulation (`StyleSimulations`) is reported, not applied.

**Images** pass through. A PNG or JPEG resource part reaches the page as
the bytes it is ([creation](creation.md), `ImageData::Compressed`); a page
whose decoded raster would be 17.2 MB costs 0.2 MB above baseline to
synthesise, measured rather than argued.

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
| `VisualBrush`, a `ContextColor` naming an ICC profile, a gradient used to stroke | `XpsElementDefect::BrushUnsupported` | a brush whose content is arbitrary markup is a nested page; painted grey and named | [ROADMAP.md](../ROADMAP.md) |
| `OpacityMask` | `XpsElementDefect::OpacityMaskUnsupported` | not mapped to a PDF soft mask yet | [ROADMAP.md](../ROADMAP.md) |
| Remote resource dictionary (`Source=` a separate part) | `XpsElementDefect::ResourceDictionaryRemote` | only in-page dictionaries are resolved | [ROADMAP.md](../ROADMAP.md) |
| TIFF, JPEG XR, any non-PNG/JPEG image part | `XpsElementDefect::ImageFormatUnsupported` | no decoder for either; named at the element | [ROADMAP.md](../ROADMAP.md) |
| `{ColorConvertedBitmap …}` naming an ICC profile | `XpsElementDefect::ImageProfileUnsupported` | no ICC pipeline, and the syntax has nowhere to put an sRGB fallback, so the picture is refused rather than drawn in colours the file did not ask for | [rendering](rendering.md) |
| `IsSideways` glyph runs | `XpsElementDefect::GlyphsSidewaysUnsupported` | vertical-rotated runs not laid out | — |
| Odd `BidiLevel` (right-to-left runs) | `XpsElementDefect::GlyphsBidiUnsupported` | no bidi reordering | [ROADMAP.md](../ROADMAP.md) (shaping) |
| `StyleSimulations` | `XpsElementDefect::GlyphsStyleSimulated` | reported; glyphs drawn unsimulated | — |
| Gradient stops with differing alphas; a `ColorInterpolationMode` this build does not interpolate in | `XpsElementDefect::BrushApproximated` | the brush reached the page and not exactly — one constant alpha cannot express per-stop alphas — and the approximation is named | — |
| Unknown element | `XpsElementDefect::ElementUnknown` | drawn around, never silently skipped | — |
| Broken fixed representation, interleaved parts, invalid or ambiguous part names, no fixed pages | `ArchiveRefusal::{UnreadablePackage, Interleaved, InvalidPartName, AmbiguousPartNames, NoFixedPages}` | a package that *is* an XPS and is broken is refused, not paged as a comic | [cbz](cbz.md) |
| Page-level defects | `XpsPageDefect::{SourceUnresolved, DocumentUnresolved, Unreadable, ContentUnreadable, SizeUnusable, MediaTypeMismatch, PageBoxUnusable}` | the page becomes a placeholder that keeps its number | — |
| Signatures, print tickets, 3D, story fragments | not read | parts the spine does not reach are ignored | — |

**Corpus caveat, stated**: every committed package was written by one of
two Microsoft serialisers on one machine. No non-Windows producer was found
(none installed could emit XPS), so the corpus is a single vendor's idea of
the format, and the doc says so ([ROADMAP.md](../ROADMAP.md) Tier 4).

## Verified

- **Eight genuine packages** in `crates/tinker-pdf/tests/xps/` — six WPF
  `.xps` (`System.Windows.Xps.Packaging`, .NET Framework 4.8) and two
  XPSOM `.oxps` (`XpsServices.dll` 10.0.26100) — obtained *before* any
  reader existed so the reader could be measured against files it did not
  write. All eight render with nothing owed. `INVENTORY.tsv` is recomputed
  through `tinker-pdf-zip` on every `cargo test`, so .NET's ZIP reader and
  this one must agree about all 52 rows. Thirteen things real files did
  that ECMA-388 does not say are recorded in that README.
- `tests/xps.rs`, `xps_spine.rs` (a package whose storage, name and markup
  orders differ), `xps_opc.rs`, `xps_markup.rs`, `xps_glyphs.rs`
  (de-obfuscation asserted byte-for-byte), `xps_images.rs`,
  `xps_memory.rs` (synthesis cost measured), `xml_real_packages.rs`.
- `xps_validated.rs`: every synthesised PDF is held to the strict validator,
  in every write mode, and its gradients, transparency groups, boxes and
  composite fonts are read back out of the dictionaries — the entries this
  engine's own typed readers supply a default for. It replaces the qpdf oracle
  ruling 13 retired; what left with it is that a reader nobody here wrote
  accepts the file. `xps_mutool.rs`: rendered output compared against an
  external reader, in a job that goes red if it is missing. **It leaves under
  ruling 13 too**, and it is the costlier of the two: what it proves — that
  two independent programs read one package and agree — is the thing
  conservation assertions cannot reproduce, so after that milestone a
  consistent misreading of ECMA-388 survives ([ROADMAP](../ROADMAP.md)).
- The `xps` determinism fingerprint and the fixed-document byte-hash
  ([determinism](determinism.md)) — the first fixture whose input this
  repository did not author.
- Fuzz targets `xml` and `zip_archive`; the `bounds_ledger.rs` rows for
  every XPS cap, measured against the real packages.
