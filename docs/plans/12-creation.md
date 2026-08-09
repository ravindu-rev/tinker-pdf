# Phase 12 — Creation

When this phase is done, tinker-pdf can produce documents, not just transform
them: a `DocumentBuilder` that assembles pages, draws content, embeds and
subsets fonts, places images without recompressing them, writes outlines that
round-trip, and hands the result to phase [09](09-writing.md) for
serialization — optionally encrypted. The shape is deliberate: creation is a
thin authoring layer over machinery that already exists by now (fonts from
[05](05-fonts.md), subsetting from [10](10-editing.md), serialization and
encryption from [09](09-writing.md) and [03](03-encryption.md)), not a second
engine. Its acceptance bar is concrete and external: regenerate Tinker's
fixture corpus and pass tinker-pdf's own parity tests against the
self-generated files.

## Scope

- `DocumentBuilder`: new document, `add_page(Size)`, page-level defaults
  (MediaBox; CropBox/Rotate optional), /Info metadata write, XMP metadata
  stream write (raw bytes passthrough, mirroring the read side in
  [04](04-document-semantics.md)).
- Content-stream builder: path construction and painting operators (m/l/c/re,
  S/f/B and variants, w/J/j/M/d, cs/CS + sc/scn for the color spaces the
  builder exposes), graphics-state save/restore, transformation.
- Text: `text()` runs with font, size, position, leading; automatic font
  embedding — TrueType and CFF programs embedded with correct
  FontDescriptor/Widths (or CIDFont structure when the text needs it), and
  **subset by default** via the used-glyph collection from
  [10-editing](10-editing.md); ToUnicode generated so extraction round-trips.
- Images: `add_image()` with **JPEG byte passthrough — DCT data is placed as-is
  with `/Filter /DCTDecode`, never decoded and recompressed**; raw RGB/Gray
  bitmaps go in flate-compressed; alpha becomes /SMask.
- Outline write using the destination enum from
  [04-document-semantics](04-document-semantics.md): `Explicit { page, kind }`,
  `Named`, `Uri` each serialize to their correct COS form and survive a
  read-back — ruling 6 in [99-consistency](99-consistency.md). This is the
  writer-side kill of MuPDF limitation #6, where `set_outlines` silently
  percent-encoded a URI into a dead named destination.
- Encryption on save and incremental/rewrite choice: delegated entirely to
  [09-writing](09-writing.md); the builder only carries the options through.
- Fixture self-hosting: a `gen-fixtures` example in the facade crate that
  reproduces Tinker's four fixtures (read
  `Tinker/crates/tinker-core/examples/gen-fixtures.rs` for the exact
  content): `simple-text.pdf` (3 A4 pages, one Helvetica line each),
  `outline-3level.pdf` (6 pages, 3-level outline with real explicit
  destinations), `encrypted-aes256.pdf` (user `open-sesame`, owner
  `owner-secret`, all permissions), `permissions-noprint.pdf`
  (`/P -2056`, printing denied). From this phase on the corpus is
  self-hosted and Tinker's MuPDF-based generator can be retired at
  integration ([15](15-tinker-integration.md)).

## Non-goals

- Layout. No paragraphs, no line breaking, no tables, no styles — Tinker's
  typst pipeline and conversion plans own document *composition*; this phase
  owns placing what a caller already positioned.
- PDF/A or PDF/X conformance output. Tracked as a later capability; Tinker
  delegates PDF/A to Ghostscript today and nothing here changes that.
- Content editing of existing pages — that is [10-editing](10-editing.md);
  the builder only creates.
- Type1 font *embedding* (reading Type1 stays in [05](05-fonts.md); new
  documents embed TrueType/CFF only).

## Design

```rust
pub struct DocumentBuilder { /* object store + page list + font registry */ }

impl DocumentBuilder {
    pub fn new() -> Self;
    pub fn metadata(&mut self) -> &mut MetadataBuilder;          // /Info + XMP
    pub fn add_page(&mut self, size: Size) -> PageBuilder<'_>;
    pub fn set_outline(&mut self, items: &[OutlineItem]);        // dest enum, ruling 6
    pub fn finish(self) -> Document;                             // ready for Document::save
}

pub struct PageBuilder<'b> { /* content ops buffer + resource dict */ }

impl PageBuilder<'_> {
    pub fn text(&mut self, font: &FontHandle, size: f32, at: Point, s: &str);
    pub fn path(&mut self) -> PathBuilder<'_>;                   // m/l/c/re + paint
    pub fn image(&mut self, img: &ImageHandle, rect: Rect);
    pub fn graphics_state(&mut self) -> GfxState<'_>;            // colors, line params, ctm
}

pub enum ImageData<'a> {
    Jpeg(&'a [u8]),               // passthrough, never recompressed
    Rgb8 { w: u32, h: u32, data: &'a [u8] },
    Gray8 { w: u32, h: u32, data: &'a [u8] },
}
```

Decisions and reasons:

- **The builder produces a `Document`, then [09](09-writing.md) saves it.**
  One serializer, one encryption path, one set of write options — creation
  gets incremental-save and linearization for free and cannot drift from the
  editor's output format.
- **Fonts are handles, registered once per document.** The registry
  deduplicates embeddings, accumulates used glyphs across all pages, and
  subsets at `finish()` — subsetting per page would embed the same font N
  times; subsetting eagerly would miss glyphs used later.
- **JPEG passthrough is a rule, not an optimization.** Recompression is
  generational quality loss the caller cannot undo; if a caller wants
  re-encoding they decode and re-add. The same rule protects the fixture
  regeneration exit criterion from drifting bytes.
- **Coordinates are PDF points, y-up, exactly as the spec has them.** No
  convenience flip: the facade's render side already documents the device
  transform, and two coordinate conventions in one API is how bugs breed.
- **ToUnicode always written.** A document this engine created must extract
  perfectly through its own text device — anything less fails the parity
  tests this phase is measured by.

Error policy: the builder is infallible until `finish()` except for caller
mistakes (unregistered handle, text with an empty font), which return typed
errors immediately at the call site — a creation API that defers all failures
to save time is miserable to debug.

## Milestones

| # | Deliverable | Exit criteria | Size |
| --- | --- | --- | --- |
| 12.1 | Builder core: pages, paths, graphics state, /Info | `tpdf info` on a built doc reports pages/sizes; `qpdf --check` green | S |
| 12.2 | Text with embedding + subsetting + ToUnicode | Built text extracts byte-identically through the phase 06 text device; subset font passes font fuzzers | M |
| 12.3 | Images (JPEG passthrough, RGB/Gray flate, /SMask) | Embedded JPEG bytes recovered exactly via `Cos::stream_raw`; renders match source images under `pdfcmp` | S |
| 12.4 | Outline write + metadata + encryption plumbing | Dest enum round-trips read→write→read; encrypted output opens at correct `AuthLevel` | S |
| 12.5 | Fixture self-hosting | All four Tinker fixtures regenerated; `tinker_parity.rs` passes against the self-generated files; oracle viewers open them | S |

## Dependencies

Needs [04](04-document-semantics.md) (dest model), [05](05-fonts.md) (font
parsing for embedding), [09](09-writing.md) (serialization, encryption),
[10](10-editing.md) (used-glyph subsetting). Post-integration OK: Tinker does
not block on this phase — but fixture self-hosting and Tinker's own creation
module (`docs/plans/05-pdf-creation.md`) both want it early after
[15](15-tinker-integration.md).

## Risks

| Risk | Mitigation |
| --- | --- |
| Subset fonts that Acrobat rejects (wrong cmap/name tables) | Validate subsets by reopening + extracting through our own stack *and* an oracle viewer render in CI; keep the unsubsetted-embedding switch as a debugging fallback |
| Fixture regeneration diverges semantically from the MuPDF-generated originals | Parity tests run against both old and new fixtures during the transition; the assertion set, not the bytes, is the contract |
| Builder API grows layout features by accretion | Non-goal stated above; layout requests route to Tinker's conversion/typst plans — ruling territory if it recurs |
