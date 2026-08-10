//! A from-scratch, pure-Rust PDF engine.
//!
//! This facade is the only crate users depend on; everything below it is an
//! implementation detail that may be reshaped without notice until the API
//! freezes at 0.1.0.
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let bytes = std::fs::read("document.pdf")?;
//! let doc = tinker_pdf::Document::open(bytes)?;
//! println!("{} pages", doc.page_count());
//! for page in doc.pages() {
//!     println!("{}", page.text().plain_text());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! Scope, design and exit criteria: `docs/plans/00-architecture.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod annots;
pub mod fonts;
pub mod redact;
mod resources;

use std::sync::Arc;

use tinker_pdf_content::{interpret, Matrix, TextDevice};
use tinker_pdf_cos::{outline as cos_outline, pages as cos_pages, CosDocument};

pub use fonts::{FontProvider, FontRequest, SimpleFontProvider};
pub use tinker_pdf_content::{Quad, TextBlock, TextChar, TextLine, TextPage, WritingMode};
pub use tinker_pdf_cos::{
    Action, AuthError, AuthLevel, DestKind, Destination, Field, FieldKind, FieldValue, LadderLevel,
    Link, Metadata, OutlineItem, Warning, WarningKind,
};
pub use tinker_pdf_crypto::Permissions;
pub use tinker_pdf_raster::canvas::PixelFormat;
pub use tinker_pdf_render::{CancelToken, RenderWarning};

/// The engine's version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// What this build is and what it can do.
///
/// Reachable on purpose: the engine this replaces blocklisted its own version
/// constant from its bindings, so nothing could report which build a bug
/// report came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildInfo {
    /// The crate version.
    pub version: &'static str,
    /// The PDF specification this engine targets.
    pub spec: &'static str,
}

/// Describes this build.
#[must_use]
pub fn build_info() -> BuildInfo {
    BuildInfo {
        version: VERSION,
        spec: "ISO 32000-1 (PDF 1.7)",
    }
}

/// How a page should be rendered.
#[derive(Clone, Debug)]
pub struct RenderOptions {
    /// Pixels per PDF point. 1.0 renders at 72 dpi.
    pub scale: f64,
    /// How the bitmap stores its pixels.
    pub format: PixelFormat,
    /// Lets a caller stop a long render.
    pub cancel: Option<CancelToken>,
    /// Whether to draw annotation appearance streams over the page.
    ///
    /// On by default, because that is what a reader sees: a page rendered
    /// without its annotations is missing its highlights, its stamps and its
    /// filled-in form fields, and looks convincingly complete without them.
    pub annotations: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            scale: 1.0,
            format: PixelFormat::Rgb8,
            cancel: None,
            annotations: true,
        }
    }
}

impl RenderOptions {
    /// Options rendering at a resolution in dots per inch.
    #[must_use]
    pub fn at_dpi(dpi: f64) -> RenderOptions {
        RenderOptions {
            scale: dpi / 72.0,
            ..RenderOptions::default()
        }
    }
}

/// A rendered page.
#[derive(Clone, Debug)]
pub struct Bitmap {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// How pixels are stored.
    pub format: PixelFormat,
    /// Bytes per row.
    pub stride: usize,
    /// The pixels.
    pub data: Vec<u8>,
    /// What the renderer could not do exactly (ruling 2).
    pub warnings: Vec<RenderWarning>,
}

impl Bitmap {
    /// How many bytes each pixel occupies.
    #[must_use]
    pub fn components(&self) -> usize {
        self.format.components()
    }
}

/// Why a document could not be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// Nothing in the bytes resembles a PDF: not one indirect object could be
    /// found, even after a full rescan.
    NotAPdf,
}

impl core::fmt::Display for OpenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            OpenError::NotAPdf => f.write_str("not a PDF: no indirect objects found"),
        }
    }
}

impl std::error::Error for OpenError {}

/// An open document.
///
/// Cheap to clone: the bytes and the object store are shared.
#[derive(Clone)]
pub struct Document {
    inner: Arc<CosDocument>,
    /// Where glyphs come from for fonts the document does not embed.
    ///
    /// None by default: the engine bundles no faces and reads no font
    /// directories, so a host that wants text drawn for such documents says
    /// where to find it. See [`fonts`].
    fonts: Option<Arc<dyn FontProvider>>,
}

impl Document {
    /// Opens a document from bytes.
    ///
    /// Bytes rather than a path because `wasm32-unknown-unknown` has no
    /// filesystem and is a first-class target; native callers read the file
    /// themselves, or memory-map it and pass the mapping.
    ///
    /// A damaged file opens anyway wherever anything can be recovered — see
    /// [`Document::ladder_level`] and [`Document::warnings`] for what that
    /// cost.
    pub fn open(bytes: impl Into<Arc<[u8]>>) -> Result<Document, OpenError> {
        let inner = CosDocument::open(bytes).map_err(|_| OpenError::NotAPdf)?;
        Ok(Document {
            inner: Arc::new(inner),
            fonts: None,
        })
    }

    /// Supplies fonts for documents that do not embed their own.
    ///
    /// Without one, such a document extracts its text perfectly — the
    /// standard-14 metrics are built in — and draws none of it, reporting
    /// [`RenderWarning::UnreadableFont`]. The engine bundles no faces and
    /// reads no font directories: bundling is a licensing decision and
    /// reading a directory is an operating-system dependency, and
    /// `wasm32-unknown-unknown` has neither.
    #[must_use]
    pub fn with_fonts(mut self, provider: Arc<dyn FontProvider>) -> Document {
        self.fonts = Some(provider);
        self
    }

    /// Whether the bytes look like a PDF, without opening them.
    #[must_use]
    pub fn sniff(bytes: &[u8]) -> bool {
        let window = bytes.get(..1024.min(bytes.len())).unwrap_or_default();
        window.windows(5).any(|w| w == b"%PDF-")
    }

    /// How much repair opening the document needed.
    #[must_use]
    pub fn ladder_level(&self) -> LadderLevel {
        self.inner.ladder_level()
    }

    /// Everything the engine tolerated, in the order it happened.
    #[must_use]
    pub fn warnings(&self) -> Vec<Warning> {
        self.inner.warnings()
    }

    /// Whether the document is encrypted.
    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        self.inner.is_encrypted()
    }

    /// How far the accepted password got.
    #[must_use]
    pub fn auth_level(&self) -> AuthLevel {
        self.inner.auth_level()
    }

    /// The document's permission flags, respecting the authentication level.
    ///
    /// An unencrypted document and one opened with the owner password both
    /// permit everything. Note that PDF permissions are advisory: a document
    /// that says printing is denied is asking, not enforcing.
    #[must_use]
    pub fn permissions(&self) -> Permissions {
        self.inner.permissions()
    }

    /// Tries a password.
    ///
    /// Reports **which** password matched, which the engine this replaces
    /// could not: its binding collapsed the C API's bitmask to a boolean, so
    /// "the owner password lifts every restriction" was unimplementable.
    ///
    /// Takes `&self`, so it works on a document that has already been shared —
    /// cloned, or had a page taken from it, which every caller does. Requiring
    /// unique ownership made the most ordinary sequence in the API
    /// (look at a page, then supply the password) report an encrypted document
    /// as unencrypted.
    pub fn authenticate(&self, password: &str) -> Result<AuthLevel, AuthError> {
        self.inner.authenticate(password)
    }

    /// The number of pages.
    #[must_use]
    pub fn page_count(&self) -> u32 {
        cos_pages::count(&self.inner)
    }

    /// Every page, in document order.
    #[must_use]
    pub fn pages(&self) -> Vec<Page> {
        cos_pages::collect(&self.inner)
            .into_iter()
            .map(|inner| Page {
                doc: self.inner.clone(),
                inner,
                fonts: self.fonts.clone(),
            })
            .collect()
    }

    /// One page by zero-based index.
    #[must_use]
    pub fn page(&self, index: u32) -> Option<Page> {
        self.pages().into_iter().nth(index as usize)
    }

    /// The document's information dictionary.
    #[must_use]
    pub fn metadata(&self) -> Metadata {
        cos_outline::metadata(&self.inner)
    }

    /// The version, as "PDF 1.7".
    #[must_use]
    pub fn pdf_version(&self) -> Option<String> {
        cos_outline::version_string(&self.inner)
    }

    /// The outline tree; empty when the document has none.
    #[must_use]
    pub fn outline(&self) -> Vec<OutlineItem> {
        cos_outline::outline(&self.inner)
    }

    /// Page labels, when the document defines them.
    #[must_use]
    pub fn page_labels(&self) -> Vec<String> {
        cos_outline::page_labels(&self.inner, self.page_count())
    }

    /// The document's interactive form fields (12.7).
    ///
    /// Empty when the document has no `/AcroForm`, which is most documents.
    #[must_use]
    pub fn form_fields(&self) -> Vec<Field> {
        tinker_pdf_cos::fields(&self.inner)
    }

    /// The underlying object model, for callers that need raw access.
    #[must_use]
    pub fn cos(&self) -> &CosDocument {
        &self.inner
    }
}

impl core::fmt::Debug for Document {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Document")
            .field("pages", &self.page_count())
            .field("encrypted", &self.is_encrypted())
            .field("ladder", &self.ladder_level())
            .finish()
    }
}

/// One page.
#[derive(Clone)]
pub struct Page {
    doc: Arc<CosDocument>,
    inner: cos_pages::Page,
    fonts: Option<Arc<dyn FontProvider>>,
}

impl Page {
    /// Zero-based position in the document.
    #[must_use]
    pub fn index(&self) -> u32 {
        self.inner.index
    }

    /// `/MediaBox`, as `(x0, y0, x1, y1)` in points.
    #[must_use]
    pub fn media_box(&self) -> (f64, f64, f64, f64) {
        let r = self.inner.media_box;
        (r.x0, r.y0, r.x1, r.y1)
    }

    /// `/CropBox`, clipped to the media box.
    #[must_use]
    pub fn crop_box(&self) -> (f64, f64, f64, f64) {
        let r = self.inner.crop_box;
        (r.x0, r.y0, r.x1, r.y1)
    }

    /// `/Rotate`, normalized to 0, 90, 180 or 270.
    #[must_use]
    pub fn rotation(&self) -> u16 {
        self.inner.rotation
    }

    /// The page's size in points after rotation, which is what a viewer lays
    /// out.
    #[must_use]
    pub fn size(&self) -> (f64, f64) {
        self.inner.display_size()
    }

    /// Renders the page to a bitmap.
    ///
    /// The bitmap's size is the page's *displayed* size — the crop box, with
    /// its axes swapped for a quarter-turn `/Rotate` — scaled and **rounded
    /// outward** so a page never loses its last row or column: A4 at 150 dpi
    /// is 1240×1755.
    #[must_use]
    pub fn render(&self, options: &RenderOptions) -> Bitmap {
        let (w, h) = self.size();
        let scale = if options.scale.is_finite() && options.scale > 0.0 {
            options.scale
        } else {
            1.0
        };

        let canvas = tinker_pdf_render::page_canvas(w, h, scale, options.format);
        // The rotation and the crop-box origin belong in the transform, not
        // only in the canvas size: sizing for a rotated page and then drawing
        // it upright fills a sideways canvas with clipped, upright content.
        let crop = self.crop_box();
        let base = tinker_pdf_render::page_view_transform(crop, self.rotation(), scale);

        let content = cos_pages::content_bytes(&self.doc, &self.inner);
        let resources =
            resources::PageResources::new(&self.doc, &self.inner, self.fonts.as_deref());

        let mut renderer = tinker_pdf_render::Renderer::new(canvas, base, &resources);
        if let Some(cancel) = &options.cancel {
            renderer = renderer.with_cancel(cancel.clone());
        }
        interpret(&content, Matrix::IDENTITY, &mut renderer, &resources);
        if options.annotations {
            // After the content, because an annotation sits on top of the
            // page rather than under it.
            annots::draw(&self.doc, &self.inner, self.fonts.as_deref(), &mut renderer);
        }
        let (canvas, warnings) = renderer.finish();

        Bitmap {
            width: canvas.width,
            height: canvas.height,
            format: canvas.format,
            stride: canvas.stride,
            data: canvas.data,
            warnings,
        }
    }

    /// The page's link annotations, in `/Annots` order (12.5.6.5).
    ///
    /// Only links. Every other annotation subtype belongs to the annotation
    /// model rather than to navigation.
    #[must_use]
    pub fn links(&self) -> Vec<Link> {
        tinker_pdf_cos::links(&self.doc, self.inner.reference)
    }

    /// The page's text.
    #[must_use]
    pub fn text(&self) -> TextPage {
        let content = cos_pages::content_bytes(&self.doc, &self.inner);
        // Text extraction needs no glyph outlines — the widths come from the
        // font dictionary — so no provider is consulted here.
        let resources = resources::PageResources::new(&self.doc, &self.inner, None);

        let mut device = TextDevice::new();
        // Text is reported in PDF user space, y upward, which is the space the
        // page's own boxes are in; a device transform is the renderer's job.
        interpret(&content, Matrix::IDENTITY, &mut device, &resources);
        device.finish()
    }
}

impl core::fmt::Debug for Page {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Page")
            .field("index", &self.index())
            .field("size", &self.size())
            .field("rotation", &self.rotation())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffing_recognizes_a_header() {
        assert!(Document::sniff(b"%PDF-1.7\n..."));
        assert!(
            Document::sniff(b"junk before the header %PDF-1.4"),
            "leading junk is routine"
        );
        assert!(!Document::sniff(b"not a pdf at all"));
        assert!(!Document::sniff(b""));
    }

    #[test]
    fn opening_nonsense_reports_it_rather_than_panicking() {
        assert_eq!(Document::open(b"".to_vec()).err(), Some(OpenError::NotAPdf));
        assert_eq!(
            Document::open(b"hello".to_vec()).err(),
            Some(OpenError::NotAPdf)
        );
    }

    #[test]
    fn build_info_is_reachable() {
        let info = build_info();
        assert_eq!(info.version, VERSION);
        assert!(info.spec.contains("32000"));
    }
}
