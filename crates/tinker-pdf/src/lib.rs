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
//! Feature documentation: `docs/architecture.md`, and one doc per feature
//! under `docs/features/`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod annots;
pub mod cbz;
pub mod epub;
pub mod fonts;
pub mod mdp;
mod optional;
pub mod pdfa;
pub mod redact;
mod resources;
pub mod shaping;
pub mod signature;
pub mod structure;
pub mod verdict;
pub mod xps;

use std::sync::Arc;

use tinker_pdf_content::{interpret, Matrix, TextDevice};
use tinker_pdf_cos::{outline as cos_outline, pages as cos_pages};

/// Comic archives: what [`Document::open`] does with a `PK\x03\x04` at offset
/// zero, and what it refuses by name.
pub use cbz::{
    ArchiveRefusal, ArchiveReport, ArchiveWarning, ComicInfo, ComicInfoDefect, Container,
    PageDefect, PageOrigin,
};
pub use fonts::{FontProvider, FontRequest, SimpleFontProvider};
/// Digital signatures, read (12.8), behind [`Document::signatures`].
pub use mdp::{Change, Modification, Modifications, Touched};
/// PDF/A conformance (ISO 19005), behind [`Document::validate_pdfa`].
pub use pdfa::{
    Clause, ConformanceFinding, Coverage as PdfACoverage, FindingKind, Flavour, Level, Part,
    RuleGroup as PdfARuleGroup, StagedRule, Verdict as PdfAVerdict, STAGED as PDFA_STAGED,
};
pub use signature::{Anchor, Coverage, CoverageDefect, Signature, SignatureWarning, SubFilter};
/// Tagged PDF: the logical structure tree, and the reading-order view over it
/// (14.7, 14.8).
pub use structure::{
    StructElement, StructKid, StructureTree, StructureWarning, StructuredNode, StructuredText,
    TextSource,
};
pub use tinker_pdf_content::{
    MarkedProps, Quad, TextBlock, TextChar, TextLine, TextPage, TextWarning, WritingMode,
};
/// The strict validator's verdict (ruling 13), behind [`Document::validate`].
///
/// `kind_counts` and `tier_counts` are how a report says *which* rules a file
/// broke without carrying every instance of them.
pub use tinker_pdf_cos::{kind_counts, tier_counts, Defect, DefectKind, Tier};
pub use tinker_pdf_cos::{
    Action, Attachment, AuthError, AuthLevel, Date, DestKind, Destination, DocumentScript, Field,
    FieldKind, FieldScripts, FieldValue, LadderLevel, Link, Metadata, OutlineItem, Script,
    ScriptSummary, Trapped, Warning, WarningKind,
};
pub use tinker_pdf_cos::{
    BlendMode, DeviceSpace, ExtGState, FormXObject, Function, Glyph, MaskKind, PlacedGlyph,
    Shading, ShadingPattern, StateMask, TilingPattern, TilingType, TransparencyGroup,
};
/// Streaming open: where a document's bytes come from when they are not all
/// in hand (`docs/design/streaming-open.md`).
///
/// [`ByteSource`] is the seam a host implements to supply ranges — a mapping,
/// a file handle, an HTTP server answering range requests. It is on this
/// facade rather than left in the crate underneath because ruling 11 makes
/// this the only public surface: a host that cannot name the trait cannot
/// implement it. [`SliceSource`] is the degenerate case every existing caller
/// already uses without knowing it, which is why [`Document::open`] keeps its
/// exact signature.
pub use tinker_pdf_cos::{
    ByteSource, CountingSource, ShreddedSource, SliceSource, SourceMiss, CHUNK_SIZE,
};

/// How many bytes are read to decide whether a source holds a container.
///
/// `cbz::container` tests fixed positions and reads no further than byte 262;
/// one kilobyte is that with room, and it is one head read either way.
const CONTAINER_SNIFF: u64 = 1024;
/// Writing: creation, editing and saving.
///
/// Without these on the facade a caller depending only on this crate could
/// read a document and never produce one, which is half a library.
///
/// The second block is gap 30 milestone 5's — the graphics state, transparency
/// groups, gradients, tiling patterns and glyphs addressed by index. Every one
/// of them is an *argument* to a [`DocumentBuilder`] method rather than
/// something a method hands back, and an argument type that cannot be named is
/// a method that cannot be called: without `ExtGState` on the facade,
/// `add_ext_gstate` is unreachable from outside this workspace. `DeviceSpace`
/// comes with them because `TransparencyGroup` and `Shading` are built from
/// one; it was already needed by `ImageColorSpace::Indexed` and already
/// missing, which is a gap 29 omission this closes as a side effect rather
/// than a policy this milestone changed.
///
/// `Target` is gap 31 milestone 5's, and it arrives for the same reason: it is
/// the argument to `PageBuilder::link` and the field an `OutlineEntry` carries,
/// so without it neither is callable from outside. Its view type is
/// [`DestKind`] — the **reader's** own, already on this facade — rather than a
/// second vocabulary for one concept, which is what makes a write followed by
/// a read an equality rather than a translation.
///
/// `EmbeddedWhole` and `SubsetRefusal` arrive with CFF subsetting and are the
/// return type of `DocumentBuilder::finish_reporting`, so without them on this
/// facade the method is callable and its answer is unnameable. They exist
/// because ruling 10 makes "the face went in whole" a reported fact rather
/// than an absence: a producer-made subset that this subsetter cannot beat is
/// a different event from a format it declines, and the caller is told which.
/// `EditCheckpoint` is gap 32 milestone 1's, and it is here for the reason
/// the paragraph above gives for `ExtGState` and `Target`: it is the return
/// type of [`DocumentEditor::checkpoint`] and the argument to
/// [`DocumentEditor::restore`], so without it on this facade neither method
/// can be named from outside this workspace. It is the closure-free spelling
/// of [`DocumentEditor::transaction`] — the same two functions, reachable from
/// a language that has no closures to hand across a boundary (ruling 11,
/// `docs/design/bindings-write.md`).
/// `ArchivalProfile` and the three types it is built from are milestone 6 of
/// `docs/design/pdfa.md`, and they are on this facade for the reason
/// `ExtGState` is: `DocumentBuilder::archival` cannot be called without naming
/// its argument. `ArchivalRefusal` comes with them because it is what
/// `finish_archival` returns and what `refusals` hands back — a refusal a
/// caller cannot name is a refusal they cannot match on, which is the thing
/// this whole surface exists to avoid.
pub use tinker_pdf_cos::{
    ArchivalLevel, ArchivalPart, ArchivalProfile, ArchivalRefusal, DocumentBuilder, DocumentEditor,
    EditCheckpoint, EmbeddedWhole, Encryption, FillError, FillRejection, ImageData, OutlineEntry,
    PageBuilder, SkippedWidget, SubsetRefusal, Target, WidgetDefect, WriteMode, WriteOptions,
};
/// Form calculations: running the `/AA` calculate actions a form carries.
///
/// The interpreter itself is [`tinker_pdf_cos::script`]; these are the types a
/// caller of [`DocumentEditor::recalculate`] handles.
///
/// [`ScriptPolicy`] and [`Trigger`] are here for the reason `ExtGState` is:
/// they are the argument to [`DocumentEditor::recalculate_under`], so without
/// them on this facade the method is callable by nobody outside this
/// workspace (ruling 11). The same argument brings [`Keystroke`], which is
/// what [`DocumentEditor::keystroke`] takes, and [`EventVerdict`],
/// [`DisplayString`], [`ScriptScope`] and [`ScriptBudget`], which are what
/// the methods hand back or what a host builds to ask a question with.
///
/// [`DisplayString`] is worth one more sentence, because its whole job is to
/// be a type: it is what a format action produces, it has no `Deref` and no
/// `Into<String>`, and none of the write doors will take one — which is how
/// 12.7.3.3 stopped being a convention and became a guarantee.
pub use tinker_pdf_cos::{
    CalcError, DisplayString, EventVerdict, Keystroke, Recalculation, ScriptBudget, ScriptError,
    ScriptPolicy, ScriptScope, Trigger,
};
/// Signing on an incremental save (12.8.1), behind
/// [`DocumentEditor::save_signed`].
///
/// The key never crosses this boundary: a [`Signer`] receives a digest and
/// returns finished CMS bytes. [`DigestAlgorithm`] is shared with the reading
/// side on purpose — one definition of what a `/ByteRange` covers, so what is
/// signed and what is checked cannot drift apart.
pub use tinker_pdf_cos::{
    Certification, DigestAlgorithm, FieldLock, SignError, SignRefused, Signer, SigningRequest,
    SigningTarget,
};
/// The object model behind [`Document::cos`].
///
/// The escape hatch is only an escape hatch if the types it hands back can be
/// named without depending on the crate underneath, so they are re-exported
/// here rather than left for a caller to find.
pub use tinker_pdf_cos::{
    CosDocument, CosError, Dict, Name, ObjRef, Object, PdfString, Revision, StreamObj, XrefEntry,
    XrefTable,
};
pub use tinker_pdf_cos::{PubSecError, Recipient};
pub use tinker_pdf_crypto::Permissions;
pub use tinker_pdf_raster::canvas::PixelFormat;
pub use tinker_pdf_render::{CancelToken, RenderWarning};
/// Signature verdicts (12.8), behind [`Document::verify_signatures`].
pub use verdict::{
    Chain, CmsState, DocumentDigest, SignatureCheck, SignerDescription, TrustAnchors, Unchecked,
    Verdict, Weakness,
};
/// Fixed documents: the other thing a `PK\x03\x04` can be (gap 30).
pub use xps::{Dialect, XpsElementDefect, XpsPageDefect};

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
///
/// **`#[non_exhaustive]`**, from gap 29 milestone 4, which is what let
/// milestone 5 add [`OpenError::UnsupportedArchive`] without a second break.
/// Gaps 30 and 31 expect their own. `Copy`, `PartialEq` and `Eq` are kept,
/// because `tests/tinker_parity.rs` compares values of this type by ruling 12.
/// `Clone` but not `Copy`: [`OpenError::SourceUnavailable`] carries the range
/// that was wanted, and a range is not `Copy`. Naming the bytes is worth more
/// than the convenience — a host told only "a range was missing" has to guess
/// which one to fetch, which is the whole question it needed answered.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenError {
    /// Nothing in the bytes resembles a PDF: not one indirect object could be
    /// found, even after a full rescan.
    ///
    /// Failing to be a PDF is not the same as failing to be a *document*: a
    /// container this build recognises and cannot page gets its own answer
    /// rather than being collapsed into this one.
    NotAPdf,
    /// The bytes are empty.
    ///
    /// Distinguished from [`OpenError::NotAPdf`] because it is almost always a
    /// caller's bug — a path that did not exist, a stream not read to the end —
    /// rather than a bad document, and telling the two apart is the difference
    /// between checking the file and checking the code.
    Empty,
    /// The bytes are a container this build recognises and cannot open as a
    /// document.
    ///
    /// The reason is named rather than collapsed into [`OpenError::NotAPdf`],
    /// because "I do not read CBR" and "this is not a document" are different
    /// answers and a host shows different things for them. See
    /// [`ArchiveRefusal`].
    UnsupportedArchive(ArchiveRefusal),
    /// A [`ByteSource`] could not supply the bytes an open must have.
    ///
    /// Only [`Document::open_streaming`] produces it, and the right response
    /// is the opposite of [`OpenError::NotAPdf`]'s: fetch the range the
    /// [`SourceMiss`] names and call again. Collapsing the two — which this
    /// enum did until the C ABI needed to tell a host which one had happened —
    /// makes the seam unusable for the thing it exists for, because "not a
    /// PDF" is a reason to stop.
    ///
    /// Only the head window produces it. A miss further in has the rescan
    /// ladder underneath it and degrades (ruling 2) rather than failing.
    SourceUnavailable(SourceMiss),
}

/// Why a document could not be *used* after it opened.
///
/// Separate from [`OpenError`]: a document that needs a password has opened
/// perfectly well, and a caller has to be able to tell that from bytes that
/// were never a PDF. Collapsing both into one variant meant every failure
/// looked like a corrupt file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentError {
    /// The document is encrypted and no password has been accepted yet.
    ///
    /// Call [`Document::authenticate`]; the empty string is the right first
    /// try, since a document encrypted only to restrict permissions has no
    /// user password.
    PasswordRequired,
    /// The document is encrypted with a handler this build does not implement.
    UnsupportedEncryption,
}

impl core::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DocumentError::PasswordRequired => f.write_str("a password is required"),
            DocumentError::UnsupportedEncryption => {
                f.write_str("the encryption handler is not supported")
            }
        }
    }
}

impl std::error::Error for DocumentError {}

impl core::fmt::Display for OpenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            OpenError::NotAPdf => f.write_str("not a PDF: no indirect objects found"),
            OpenError::Empty => f.write_str("no bytes to read"),
            OpenError::UnsupportedArchive(why) => write!(f, "not opened as a document: {why}"),
            OpenError::SourceUnavailable(miss) => write!(f, "the source could not supply {miss}"),
        }
    }
}

impl std::error::Error for OpenError {}

/// What a **reflowable** document needs decided before it has any pages.
///
/// Every field is ignored by a PDF, a CBZ and an XPS, which have their own page
/// geometry and always did — except [`OpenOptions::fonts`], which is where a
/// font provider goes for *any* document and is on this type because for a book
/// it decides more than glyphs.
///
/// # Why this exists at `open` and not later
///
/// A PDF page is fixed and a reflowable book is not, so somebody has to choose
/// the box a book is laid out into — and that choice decides how many pages
/// there are. Three facts made every later seam wrong:
///
/// 1. [`RenderOptions`] is a parameter of [`Page::render`], so it arrives
///    **after** pagination; its `scale` is a resolution and not a page box.
/// 2. [`Document::with_fonts`] is a builder that arrives **after** `open`.
/// 3. Line breaking needs advance widths, so a book laid out at `open` with no
///    provider is laid out with metrics the render then does not use.
///
/// A `with_fonts` that silently does not change the pagination is exactly the
/// invisible partial implementation gap 31 exists to prevent, so the late path
/// warns by name — [`ArchiveWarning::FontsAttachedAfterPagination`] — and this
/// is where a provider goes if it is to decide anything.
///
/// **For a reflowable EPUB the page count is a function of these numbers and is
/// not a property of the file.** Two hosts that pass different boxes get
/// different books, on purpose, and [`ArchiveReport::layout`] is where a caller
/// reads back which numbers produced the page count it is holding.
///
/// `mutool draw` takes `-W`, `-H` and `-S` for exactly these three fields,
/// separately from its render resolution. Agreeing with the implementation gap
/// 28 is removing is evidence the seam is in the right place.
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let bytes = std::fs::read("book.epub")?;
/// let mut options = tinker_pdf::OpenOptions::default();
/// options.page = (600.0, 800.0);
/// let doc = tinker_pdf::Document::open_with(bytes, &options)?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
#[non_exhaustive]
pub struct OpenOptions {
    /// The page box a reflowable document is laid out into, in points.
    ///
    /// [`epub::DEFAULT_PAGE`] — 432 × 648, six inches by nine — and that number
    /// is argued where it is declared rather than here.
    pub page: (f64, f64),
    /// The base font size, in points, that `1rem` and an unstyled paragraph
    /// resolve to. [`epub::DEFAULT_FONT_SIZE`].
    pub font_size: f64,
    /// Faces for text the document does not embed.
    ///
    /// Here rather than only on [`Document::with_fonts`], because a substituted
    /// face's advance widths decide where every line breaks and therefore how
    /// many pages a reflowable document has. For a PDF, a comic and a fixed
    /// document it does exactly what `with_fonts` does and arrives earlier.
    pub fonts: Option<Arc<dyn FontProvider>>,
}

impl Default for OpenOptions {
    fn default() -> Self {
        OpenOptions {
            page: epub::DEFAULT_PAGE,
            font_size: epub::DEFAULT_FONT_SIZE,
            fonts: None,
        }
    }
}

impl OpenOptions {
    /// Options laying a reflowable document into a page box, in points.
    #[must_use]
    pub fn at_page(width: f64, height: f64) -> OpenOptions {
        OpenOptions {
            page: (width, height),
            ..OpenOptions::default()
        }
    }

    /// The same options with a font provider attached.
    #[must_use]
    pub fn with_fonts(mut self, provider: Arc<dyn FontProvider>) -> OpenOptions {
        self.fonts = Some(provider);
        self
    }
}

impl core::fmt::Debug for OpenOptions {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OpenOptions")
            .field("page", &self.page)
            .field("font_size", &self.font_size)
            .field("fonts", &self.fonts.is_some())
            .finish()
    }
}

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
    /// Present only when this document was **synthesised** from a container
    /// rather than parsed from a PDF. See [`Document::archive`].
    archive: Option<Arc<ArchiveReport>>,
}

/// Opens a recognised container **once** and routes it by what it holds.
///
/// One `Archive::open` for all three formats, which is gap 30 milestone 3's own
/// exit criterion rather than a nicety: opening it to sniff and again to read
/// doubles the central-directory walk and creates a window in which the two
/// reads could disagree about the same bytes. [`xps::route`] and [`epub::route`]
/// each hand the archive back **unread** when it is not theirs, and the cheap
/// half of both decisions costs no read at all.
///
/// # The order, and why each step is where it is
///
/// **XPS first, unchanged.** ECMA-388 E.3's three steps, all of them, exactly
/// as gap 30 wrote them. Its step 2 asks for `[Content_Types].xml` and
/// `_rels/.rels`, and gap 31's milestone 1 measured that **no** EPUB carries
/// either — 0 of 26 books — so an EPUB fails E.3 at step 2's first check having
/// read nothing, and this step is a comparison per entry for a book as it
/// already was for a comic.
///
/// **EPUB second**, by the presence of `META-INF/container.xml` (OCF 3.3
/// §4.2.6.3). It goes after XPS rather than before because E.3 is a
/// specification's own recipe and this is not: nothing in OCF says a container
/// may not also carry OPC's two items, and a file that satisfies both tests is
/// better read by the standard that describes how to recognise itself.
///
/// **The comic path is the fallthrough**, which is what E.3's own text asks
/// for. A ZIP that is neither is a bag of images, and that is the reading gap
/// 29 shipped.
///
/// `options` reaches only the EPUB step, because it is the only format here
/// whose page geometry its own file does not state. The two before it read
/// their sizes out of the package and ignore every field.
fn open_container(
    what: Container,
    bytes: &[u8],
    options: &OpenOptions,
) -> Result<(Vec<u8>, ArchiveReport), ArchiveRefusal> {
    if what != Container::Zip {
        // Recognised and refused. RAR, 7z and tar are three more
        // decompressors, two of them encumbered, and none of them a page.
        return Err(ArchiveRefusal::NotAZip);
    }
    let comic = cbz::Limits::DEFAULT;
    let archive = cbz::open_archive(bytes, &comic.zip)?;
    let archive = match xps::route(archive, &xps::Limits::DEFAULT) {
        xps::Routing::Document(pdf, report) => return Ok((pdf, report)),
        xps::Routing::Refused(why) => return Err(why),
        xps::Routing::NotXps(archive) => archive,
    };
    // A caller's number is checked once, here, before any book is read: the
    // defects belong in the report of the document that was laid out at the
    // replacement, and a book that turns out not to be one never sees them.
    let (layout, unusable) = epub::BookLayout::sanitised(options.page, options.font_size);
    let archive = match epub::route(archive, &epub::Limits::DEFAULT, &layout) {
        epub::Routing::Document(pdf, mut report) => {
            for defect in unusable {
                report.warn(ArchiveWarning::UnusableOption(defect));
            }
            return Ok((pdf, report));
        }
        epub::Routing::Refused(why) => return Err(why),
        epub::Routing::NotEpub(archive) => archive,
    };
    cbz::pages_from_archive(archive, &comic)
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
    ///
    /// # Containers
    ///
    /// Bytes that **begin** with a container signature are not read as a PDF.
    /// RAR, 7z and tar are refused by name. A ZIP is **one signature over five
    /// formats**, so it is opened once and then asked what it is: an XPS
    /// package becomes a document whose pages are its fixed pages (gap 30), an
    /// EPUB becomes a document whose pages are its spine (gap 31), and anything
    /// else is synthesised into a document whose pages are its images (gap 29).
    ///
    /// **A book paginates at a page box this call does not state**, so it gets
    /// the default: 432 × 648 points, from [`epub::DEFAULT_PAGE`]. See
    /// [`Document::open_with`] and [`OpenOptions`], where the page count stops
    /// being a property of the file.
    ///
    /// The order is not arbitrary and it is argued in [`open_container`]: the
    /// XPS test is ECMA-388 E.3's, all three steps of it, the EPUB test is
    /// OCF's `META-INF/container.xml`, and the comic path is the fallthrough —
    /// because an XPS or an EPUB mis-routed to the comic path is gap 30's and
    /// gap 31's headline defect, where a comic mis-routed either way would be a
    /// refusal where a document used to open.
    ///
    /// The signatures are tested at a fixed position and nowhere else, so a PDF
    /// that happens to carry `PK\x03\x04` inside a stream is unaffected — see
    /// [`cbz::container`].
    ///
    /// # Errors
    /// [`OpenError::Empty`] for no bytes, [`OpenError::UnsupportedArchive`] for
    /// a container this build recognises and cannot open as a document, and
    /// [`OpenError::NotAPdf`] when not one indirect object could be found.
    pub fn open(bytes: impl Into<Arc<[u8]>>) -> Result<Document, OpenError> {
        Document::open_with(bytes, &OpenOptions::default())
    }

    /// Opens a document, stating what a **reflowable** one needs decided before
    /// it has any pages.
    ///
    /// Identical to [`Document::open`] for a PDF, a comic archive and a fixed
    /// document, whose page geometry their own files state — except that
    /// [`OpenOptions::fonts`] is honoured for all of them, which is
    /// [`Document::with_fonts`] arriving early enough to matter.
    ///
    /// For an EPUB it is the difference between one book and another: **the
    /// page count is a function of [`OpenOptions::page`] and is not a property
    /// of the file.** See that type for why the seam is here rather than at
    /// `render` or at `with_fonts`.
    ///
    /// [`Document::open`]'s own signature is unchanged by this, deliberately:
    /// ruling 12's `tinker_parity.rs` compares it, and every existing caller
    /// keeps the defaults. `open(bytes)` *is* `open_with(bytes,
    /// &OpenOptions::default())`, which is written that way rather than
    /// duplicated so the two can never drift.
    ///
    /// # Errors
    /// The same three as [`Document::open`].
    pub fn open_with(
        bytes: impl Into<Arc<[u8]>>,
        options: &OpenOptions,
    ) -> Result<Document, OpenError> {
        let bytes: Arc<[u8]> = bytes.into();
        if bytes.is_empty() {
            return Err(OpenError::Empty);
        }

        if let Some(container) = cbz::container(&bytes) {
            let (pdf, report) = open_container(container, &bytes, options)
                .map_err(OpenError::UnsupportedArchive)?;
            // These bytes came out of this repository's own writer moments
            // ago, so a parse failure is a defect here rather than a claim
            // about the archive — but ruling 1 forbids asserting it, and
            // `Damaged` is the honest thing to say to a caller who cannot act
            // on the difference either way.
            let inner = CosDocument::open(pdf)
                .map_err(|_| OpenError::UnsupportedArchive(ArchiveRefusal::Damaged))?;
            return Ok(Document {
                inner: Arc::new(inner),
                fonts: fonts::effective(options.fonts.clone()),
                archive: Some(Arc::new(report)),
            });
        }

        let inner = CosDocument::open(bytes).map_err(|_| OpenError::NotAPdf)?;
        Ok(Document {
            inner: Arc::new(inner),
            fonts: fonts::effective(options.fonts.clone()),
            archive: None,
        })
    }

    /// Opens a document whose bytes are fetched from `source` as they are
    /// needed (`docs/design/streaming-open.md`).
    ///
    /// The same document as [`Document::open`] over the same bytes: the same
    /// pages, the same warnings, the same ladder level and the same pixels.
    /// A source is where bytes come from, never what they mean, and ruling 4
    /// makes that a contract rather than an intention -- the determinism
    /// fingerprints are run over a source that answers one byte at a time and
    /// must come out bit-identical.
    ///
    /// # Containers are whole-file, and say so
    ///
    /// A ZIP's central directory is at its end and synthesising a document
    /// from one rewrites the whole thing, so a recognised container fetches
    /// every byte before it is opened. That is declared rather than hidden:
    /// [`Document::whole_file_fetched`] answers true afterwards.
    ///
    /// # Errors
    /// The same three as [`Document::open`]. A source that cannot supply the
    /// bytes the open path needs is [`OpenError::NotAPdf`], because a document
    /// nobody could read is not one this call can return.
    pub fn open_streaming(source: Arc<dyn ByteSource>) -> Result<Document, OpenError> {
        Document::open_streaming_with(source, &OpenOptions::default())
    }

    /// [`Document::open_streaming`], stating what a reflowable document needs
    /// decided before it has any pages.
    ///
    /// # Errors
    /// The same three as [`Document::open`].
    pub fn open_streaming_with(
        source: Arc<dyn ByteSource>,
        options: &OpenOptions,
    ) -> Result<Document, OpenError> {
        if source.is_empty() {
            return Err(OpenError::Empty);
        }
        // The signatures are tested at a fixed position and nowhere else, so
        // one head window answers the question for every container this build
        // recognises -- `cbz::container` reads no further than byte 262.
        let head = source
            .read(0..CONTAINER_SNIFF)
            .map_err(OpenError::SourceUnavailable)?;
        if cbz::container(&head).is_some() {
            // Whole-file by contract, and the only honest way to read one.
            let mut bytes = Vec::with_capacity(source.len() as usize);
            let mut at = 0u64;
            while at < source.len() {
                let got = source
                    .read(at..source.len())
                    .map_err(OpenError::SourceUnavailable)?;
                if got.is_empty() {
                    return Err(OpenError::NotAPdf);
                }
                at += got.len() as u64;
                bytes.extend_from_slice(&got);
            }
            return Document::open_with(bytes, options);
        }

        let inner = CosDocument::open_source(source).map_err(|error| match error {
            tinker_pdf_cos::OpenError::SourceUnavailable(miss) => {
                OpenError::SourceUnavailable(miss)
            }
            tinker_pdf_cos::OpenError::NoObjects => OpenError::NotAPdf,
        })?;
        Ok(Document {
            inner: Arc::new(inner),
            fonts: fonts::effective(options.fonts.clone()),
            archive: None,
        })
    }

    /// Whether this document's bytes are fetched from a [`ByteSource`].
    #[must_use]
    pub fn is_streamed(&self) -> bool {
        self.inner.is_streamed()
    }

    /// Where the first page's objects end (Annex F `/E`), when this document
    /// was opened on the linearized fast path.
    ///
    /// `None` for every other document. A caller measuring what a page-one
    /// render cost asks this for where the tail starts, rather than assuming
    /// a fraction of the file.
    #[must_use]
    pub fn first_page_end(&self) -> Option<u64> {
        self.inner.first_page_end()
    }

    /// Whether every byte of the document has been fetched.
    ///
    /// Always true for one opened from a buffer. For a streamed one it is how
    /// a caller sees that a whole-file operation has happened.
    #[must_use]
    pub fn whole_file_fetched(&self) -> bool {
        self.inner.whole_file_fetched()
    }

    /// Where this document's pages came from, when it was synthesised from a
    /// container rather than parsed from a PDF.
    ///
    /// `None` for an ordinary PDF, which is how a caller tells the two apart.
    /// [`Document::cos`] answers the same for both, on purpose: the synthesised
    /// document is a real one, with a real catalog, a real page tree and real
    /// image XObjects, and it is what the renderer was given.
    #[must_use]
    pub fn archive(&self) -> Option<&ArchiveReport> {
        self.archive.as_deref()
    }

    /// Supplies fonts for documents that do not embed their own.
    ///
    /// Without one, such a document extracts its text perfectly — the
    /// standard-14 metrics are built in — and draws none of it, reporting
    /// [`RenderWarning::UnreadableFont`]. The engine bundles no faces and
    /// reads no font directories: bundling is a licensing decision and
    /// reading a directory is an operating-system dependency, and
    /// `wasm32-unknown-unknown` has neither.
    ///
    /// # On a reflowable document this is **too late**, and it says so
    ///
    /// A book's line breaks were decided when it was paginated, at `open`, from
    /// the metrics available then. A provider attached here still supplies
    /// glyphs; what it cannot do is change how many pages there are or which
    /// page a sentence is on. Silently accepting a provider that changes
    /// nothing is precisely the invisible partial implementation gap 31 exists
    /// to prevent, so this records
    /// [`ArchiveWarning::FontsAttachedAfterPagination`] on the document's own
    /// report — and [`OpenOptions::fonts`] is the route that works.
    ///
    /// Nothing is warned about for a PDF, a comic or a fixed document, whose
    /// pages this cannot move.
    #[must_use]
    pub fn with_fonts(mut self, provider: Arc<dyn FontProvider>) -> Document {
        // Through the same seam `open_with` uses, so the two cannot disagree
        // about whether a `bundled-fonts` build's own faces apply.
        self.fonts = fonts::effective(Some(provider));
        if let Some(report) = self.archive.as_mut() {
            if report.layout().is_some() {
                Arc::make_mut(report).warn(ArchiveWarning::FontsAttachedAfterPagination);
            }
        }
        self
    }

    /// Whether the bytes look like a PDF, without opening them.
    ///
    /// A PDF, specifically. [`Document::open`] also opens a comic archive, and
    /// this answers `false` for one — [`cbz::container`] is the question to ask
    /// about those.
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

    /// Reads the file again strictly, and reports what a tolerant read let
    /// through (ruling 13).
    ///
    /// [`Document::warnings`] says what the *reader* repaired. This says what
    /// is wrong with the file, which is a larger set: the reader's repairs
    /// plus every structure it never consults — the cross-reference sections
    /// as the bytes spell them, stream extents against `endstream`, the
    /// trailer against Table 15. An empty verdict is the strongest statement
    /// this repository makes about a file it wrote.
    ///
    /// Authenticate an encrypted document first: a stream that cannot be
    /// decrypted reads as one that does not decode.
    #[must_use]
    pub fn validate(&self) -> Vec<Defect> {
        tinker_pdf_cos::validate(&self.inner)
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

    /// Whether the document can be read, or what it wants first.
    ///
    /// A caller that opens a file and immediately asks for its text needs to
    /// know the difference between "this is not a PDF" and "this is a PDF and
    /// it wants a password". `open` answers the first; this answers the
    /// second, which it cannot, because a document that needs a password has
    /// opened perfectly well.
    ///
    /// # Errors
    /// [`DocumentError::PasswordRequired`] when the document is encrypted and
    /// nothing has been authenticated, and
    /// [`DocumentError::UnsupportedEncryption`] when the handler is one this
    /// build does not implement.
    pub fn readable(&self) -> Result<(), DocumentError> {
        if !self.is_encrypted() {
            return Ok(());
        }
        if self.auth_level() == AuthLevel::None {
            // An empty user password is the common case for a document
            // encrypted only to restrict permissions, and `open` does not try
            // it — trying it here would hide the distinction this exists for.
            return Err(DocumentError::PasswordRequired);
        }
        Ok(())
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
        // Through the bounded walk rather than `pages()`, which collects every
        // page: on a streamed document opened through Annex F's head-only
        // path, the rest of the page tree is the rest of the file, and asking
        // for page one would spend all of it.
        let inner = cos_pages::at(&self.inner, index)?;
        Some(Page {
            doc: Arc::clone(&self.inner),
            inner,
            fonts: self.fonts.clone(),
        })
    }

    /// The document's logical structure tree (14.7.2).
    ///
    /// `None` when the catalog has no `/StructTreeRoot`, which is what most
    /// documents are. **Nothing is inferred for them**: 14.7 describes
    /// structure a producer writes down, and a tree guessed from geometry
    /// would be this engine's opinion about reading order presented as the
    /// file's own statement of it.
    ///
    /// Bound on every call rather than cached. A structure tree is read by
    /// callers that want one, which is a small fraction of them, and a cache
    /// on a `Clone` handle over a shared object store is a lifetime question
    /// this does not need to answer to be correct.
    #[must_use]
    pub fn structure(&self) -> Option<StructureTree> {
        structure::bind(&self.inner)
    }

    /// The document's information dictionary.
    #[must_use]
    pub fn metadata(&self) -> Metadata {
        cos_outline::metadata(&self.inner)
    }

    /// The version, as "PDF 1.7".
    ///
    /// The later of the header's (7.5.2) and the catalog's (7.7.2), never
    /// absent: a document stating neither reports the 1.7 baseline and says
    /// so with a `HeaderMissing` warning.
    #[must_use]
    pub fn pdf_version(&self) -> String {
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

    /// Every file attached to the document (7.11.4).
    ///
    /// Attachments are how a PDF carries a spreadsheet next to the report made
    /// from it, and how some invoicing standards carry their machine-readable
    /// half. The bytes are not read here — listing what a document carries
    /// should not cost what extracting it costs — so each entry hands back the
    /// stream reference to read through [`Document::cos`].
    #[must_use]
    pub fn attachments(&self) -> Vec<Attachment> {
        tinker_pdf_cos::attachments(&self.inner)
    }

    /// The document's XMP metadata, unparsed (14.3.2).
    ///
    /// RDF/XML, handed over as bytes: parsing it needs an XML reader this
    /// engine does not have and should not grow, and a caller that wants it
    /// already has one.
    #[must_use]
    pub fn xmp_metadata(&self) -> Option<Vec<u8>> {
        tinker_pdf_cos::xmp_metadata(&self.inner)
    }

    /// The document's interactive form fields (12.7).
    ///
    /// Empty when the document has no `/AcroForm`, which is most documents.
    #[must_use]
    pub fn form_fields(&self) -> Vec<Field> {
        tinker_pdf_cos::fields(&self.inner)
    }

    /// Opens a public-key-encrypted document with the caller's key (7.6.5).
    ///
    /// `/Adobe.PubSec` seals the file key to certificates rather than to a
    /// password, so there is nothing to type: the caller implements
    /// [`Recipient`], is handed the sealed key and the identifier saying whose
    /// it is, and does the one private-key operation this engine refuses to be
    /// able to do. Returning `None` from it means "not addressed to me".
    ///
    /// # Errors
    /// [`PubSecError`], which tells "this document is somebody else's" apart
    /// from every other way it can fail.
    pub fn authenticate_with_recipient(
        &self,
        recipient: &dyn Recipient,
    ) -> Result<AuthLevel, PubSecError> {
        self.inner.authenticate_with_recipient(recipient)
    }

    /// The document's digital signatures (12.8), in field order.
    ///
    /// One entry per signature field that carries a `/V`; a signature field
    /// with no value is a place for a signature rather than a signature, and
    /// [`Document::form_fields`] already lists it.
    ///
    /// Nothing here is verified. Each entry says what the file claims and what
    /// checking that claim against the file established — in particular
    /// [`Signature::coverage`], which is the difference between a signature
    /// over this document and a signature over some of it.
    #[must_use]
    pub fn signatures(&self) -> Vec<Signature> {
        signature::signatures(self)
    }

    /// What every signature in this document turns out to prove (12.8).
    ///
    /// One [`Verdict`] per signature, in the order [`Document::signatures`]
    /// returns them. `anchors` are the certificates the *caller* trusts —
    /// with none, the chain result is [`Chain::NoAnchors`], which is honest:
    /// without something trusted to reach, a chain proves that a key signed
    /// something and not whose key it was.
    ///
    /// `at` is the instant to judge certificate validity at, in seconds since
    /// the Unix epoch. `None` reports the windows and judges nothing, which is
    /// the default because ruling 4 bans a clock from this engine and because
    /// "expired" is a claim about now.
    #[must_use]
    pub fn verify_signatures(&self, anchors: &TrustAnchors, at: Option<i64>) -> Vec<Verdict> {
        self.signatures()
            .iter()
            .map(|signature| verdict::verdict(self, signature, anchors, at))
            .collect()
    }

    /// What this build makes of the document's PDF/A claim (ISO 19005).
    ///
    /// A list of findings rather than a verdict: conformance is the list being
    /// empty **and** [`PdfACoverage::is_complete`] being true, and until the
    /// remaining rule groups land it is not. A caller reaching for a boolean
    /// should read both.
    #[must_use]
    pub fn validate_pdfa(&self) -> PdfAVerdict {
        pdfa::validate(self, PdfACoverage::IMPLEMENTED)
    }

    /// [`Document::validate_pdfa`], running only the rule groups in `groups`.
    ///
    /// The design doc requires that a syntax-only sweep over the whole veraPDF
    /// corpus never builds the machinery the other groups need, and a
    /// requirement nothing can ask for is a requirement nothing can check. This
    /// is how it is asked for: [`PdfACoverage::SYNTAX`] parses no XMP packet at
    /// all, and the verdict's own `coverage` reports back exactly which groups
    /// ran, so a caller cannot read a short sweep as a clean bill of health.
    ///
    /// Asking for a group this build has no rules for is not an error and does
    /// not make the verdict claim it ran — `coverage` is what actually
    /// happened, never what was requested.
    #[must_use]
    pub fn validate_pdfa_with(&self, groups: PdfACoverage) -> PdfAVerdict {
        pdfa::validate(self, groups)
    }

    /// The strictest certification any signature in this document declares
    /// (12.8.2.2), or `None` when none of them certifies it.
    ///
    /// A shortcut past `signatures()` for the common question "is this
    /// document certified, and how tightly".
    #[must_use]
    pub fn certification(&self) -> Option<Certification> {
        mdp::strictest(self)
    }

    /// What the form's calculations depend on, in the order they run
    /// (12.7.2, table 218).
    ///
    /// The references are field objects; match them against
    /// [`Field::reference`]. Empty for a form that declares no order, which
    /// includes forms whose fields calculate anyway.
    #[must_use]
    pub fn calculation_order(&self) -> Vec<ObjRef> {
        tinker_pdf_cos::calculation_order(&self.inner)
    }

    /// The document-level scripts, from `/Names /JavaScript` (7.7.4).
    #[must_use]
    pub fn document_scripts(&self) -> Vec<DocumentScript> {
        tinker_pdf_cos::document_scripts(&self.inner)
    }

    /// The catalog's `/AA` additional actions (12.6.3, table 200).
    #[must_use]
    pub fn catalog_scripts(&self) -> Vec<DocumentScript> {
        tinker_pdf_cos::catalog_scripts(&self.inner)
    }

    /// How much script this document carries, for a caller that has to say so
    /// before it fills anything.
    ///
    /// Reading a script runs nothing.
    #[must_use]
    pub fn script_summary(&self) -> ScriptSummary {
        tinker_pdf_cos::script_summary(&self.inner)
    }

    /// The underlying object model, for callers that need raw access.
    #[must_use]
    pub fn cos(&self) -> &CosDocument {
        &self.inner
    }

    /// An editor over this document, for callers that want to write it back.
    ///
    /// [`Document::cos`] *lends* the object model and [`DocumentEditor`] needs
    /// to *share* it, so without this a caller who opened a document through
    /// this facade could read it and never save it — including a document
    /// synthesised from a comic archive, which is the one thing gap 29 wanted a
    /// third party to be able to check.
    #[must_use]
    pub fn editor(&self) -> DocumentEditor {
        DocumentEditor::new(self.inner.clone())
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

        // The canvas is clamped for an enormous page; the transform has to be
        // clamped by the same factor or the content is drawn full-size onto a
        // smaller surface, which crops instead of scaling.
        let applied = tinker_pdf_render::page_scale(w, h, scale);

        // Ruling 2: the caller gets a whole page rather than a fragment, and
        // is told the resolution is not the one they asked for.
        let scaled_down = applied < scale;
        // The rotation and the crop-box origin belong in the transform, not
        // only in the canvas size: sizing for a rotated page and then drawing
        // it upright fills a sideways canvas with clipped, upright content.
        let crop = self.crop_box();
        let base = tinker_pdf_render::page_view_transform(crop, self.rotation(), applied);

        let content = cos_pages::content_bytes(&self.doc, &self.inner);
        let resources = resources::PageResources::new(&self.doc, &self.inner, self.fonts.as_ref());

        // 11.4.7: the page itself may declare a transparency group, and its
        // `/CS` is the space the *whole page* composites in. Nothing invokes
        // it, so it is read here rather than reaching the device through a
        // `Do` — and, unlike a form's group, it decides the format of the page
        // canvas itself rather than of a buffer over it. The bitmap is
        // converted back for the caller at the end; 11.4.7 says the page group
        // is composited and then converted to the output device's space, which
        // is exactly those two steps.
        let page_space = resources.page_group_space(&self.inner);
        let canvas_format = page_space
            .and_then(tinker_pdf_render::group_format)
            .unwrap_or(options.format);
        let canvas = tinker_pdf_render::page_canvas_in(w, h, applied, canvas_format);

        let mut renderer = tinker_pdf_render::Renderer::new(canvas, base, &resources);
        if let Some(cancel) = &options.cancel {
            renderer = renderer.with_cancel(cancel.clone());
        }
        if let Some(space) = page_space {
            renderer.note_page_group_space(space);
        }
        interpret(&content, Matrix::IDENTITY, &mut renderer, &resources);
        if options.annotations {
            // After the content, because an annotation sits on top of the
            // page rather than under it.
            annots::draw(&self.doc, &self.inner, self.fonts.as_ref(), &mut renderer);
        }
        let (canvas, mut warnings) = renderer.finish();
        // A glyph a font could not name is reported here rather than by the
        // renderer, which counts only the glyphs it was handed nothing for.
        // `.notdef` *is* an outline, so drawing it — which is what the spec
        // asks for — never reaches that counter, and the page would come out
        // blank in that spot with nothing said about why.
        if !resources.missing_fonts().is_empty()
            && !warnings.contains(&RenderWarning::UnreadableFont)
        {
            warnings.push(RenderWarning::UnreadableFont);
        }
        // Ruling 10: an image that decoded with a damaged row replicated, or
        // short, is drawn *and* named. The decoders have always returned these
        // and the image path has always thrown them away, so a scan missing
        // half its rows rendered identically to a scan of blank paper.
        for (name, reason) in resources.damaged_images() {
            let warning = RenderWarning::DamagedImage { name, reason };
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
        }
        if scaled_down {
            warnings.push(RenderWarning::PageScaledDown {
                requested: scale,
                applied,
            });
        }

        // Back to something a caller can read. A page group composited over
        // ink comes back as light, which is 11.4.7's own last step.
        let wanted = tinker_pdf_render::page_format(options.format);
        let canvas = if canvas.format == wanted {
            canvas
        } else {
            canvas.extract((0, 0), canvas.width, canvas.height, wanted)
        };

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

        // Ruling 2 applies to text as much as to pixels: extraction reports
        // what it had to tolerate rather than returning a shorter string and
        // leaving the caller to wonder.
        for name in resources.missing_fonts() {
            device.warn(TextWarning::UnknownFont { name });
        }
        device.finish()
    }

    /// The page's text in **structure order**, joined with the document's
    /// structure tree (14.7.4, 14.8).
    ///
    /// `None` when the document has no structure tree, which is the same
    /// answer [`Document::structure`] gives and for the same reason.
    ///
    /// This is a *second view* over the extraction [`Page::text`] already
    /// produces, not a second extractor: the same [`TextPage`] is built, and
    /// [`TextPage::plain_text`] on it is unchanged by a byte. What differs is
    /// the order the characters come back in, that `/ActualText` replaces what
    /// it encloses (14.9.4), that `/Alt` and `/E` surface, and that content
    /// the structure tree does not claim is **counted** rather than appended —
    /// see [`StructuredText::orphans`].
    #[must_use]
    pub fn structured_text(&self) -> Option<StructuredText> {
        let tree = structure::bind(&self.doc)?;
        Some(tree.text_for_page(self.index(), &self.text()))
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
        // Empty input is its own answer: it is a caller's bug far more often
        // than a bad document, and it used to be reported as `NotAPdf`, which
        // sent people to look at the file.
        assert_eq!(Document::open(b"".to_vec()).err(), Some(OpenError::Empty));
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
