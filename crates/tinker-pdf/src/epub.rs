//! An EPUB is told from a comic, and becomes a book with pages (gap 31,
//! milestones 3 and 4).
//!
//! `PK\x03\x04` is one signature over five formats. Gap 29 read it as a comic
//! archive; gap 30 taught [`crate::Document::open`] to recognise an XPS first,
//! by ECMA-388 E.3's three steps, and named the rest of the list in the same
//! sentence — *"CBZ, XPS, EPUB, ODF, OOXML and every JAR ever built"*. This
//! module is the entry nobody went back for.
//!
//! # The route, and why gap 30's is untouched
//!
//! E.3's step 2 asks whether the archive holds `[Content_Types].xml` **and**
//! `_rels/.rels`. Gap 31's milestone 1 measured twenty-six real books and
//! **none** of them holds either, so an EPUB fails E.3 at step 2's first check
//! having read nothing, and the comic fallthrough is exactly what E.3's own
//! text asks for. [`ArchiveRefusal::UnreadablePackage`] is therefore
//! unreachable for an EPUB. **Gap 30 is not wrong; EPUB is a different question
//! it did not ask**, and the order in [`crate::Document::open`] says so: XPS
//! first, then this, then the comic path.
//!
//! # What the signature is, and what it deliberately is not
//!
//! **The signature is `META-INF/container.xml`.** OCF 3.3 §4.2.6.3 makes it the
//! one file every container must hold, and it is what tells this format from
//! every other ZIP: an ODF has `META-INF/manifest.xml` and no `container.xml`,
//! a JAR has `META-INF/MANIFEST.MF`, and a comic archive that happens to carry
//! a `META-INF/` directory record — which one of milestone 1's two producers
//! writes into every book — has no file in it at all.
//!
//! **The signature is not the `mimetype` rule.** §4.3.2 requires an entry
//! named `mimetype`, first in the archive, uncompressed and with no extra
//! field, holding exactly `application/epub+zip`. That is a `MUST` and this
//! engine degrades rather than fails (ruling 2), so a book that breaks it is
//! **warned about and read anyway**: refusing one would lose a book over a ZIP
//! field that changes nothing about its contents. The asymmetry — one clause
//! decides the format and the other only reports on it — is decided here rather
//! than by whoever writes the next layer.
//!
//! The comparison is byte-exact and case-sensitive, because §4.2.3 says
//! container paths are compared case-sensitively. `META-INF/Container.xml` is
//! not `META-INF/container.xml`, and a build that folded case would route a
//! comic archive holding the first into this module.
//!
//! # Where the pages come from, and why they are the caller's business
//!
//! **A PDF page is fixed and a reflowable book is not.** A comic's page size is
//! its image's pixels and an XPS states its own in 1/96 inch; a book states
//! nothing at all, so somebody has to choose — and the choice decides how many
//! pages there are. Three facts in this repository forced the shape:
//! `RenderOptions` is a parameter of `Page::render`, so it arrives *after*
//! pagination; [`crate::Document::with_fonts`] is a builder that arrives
//! *after* `open`; and line breaking needs advance widths, so a book laid out
//! at `open` with no provider is laid out with metrics the render then does not
//! use.
//!
//! So the page box and the base font size arrive at `open`, through
//! [`crate::OpenOptions`], and reach this module as a [`BookLayout`]. **For a
//! reflowable book the page count is a function of that number and is not a
//! property of the file**, which is stated on the type rather than left to be
//! discovered. `mutool draw`'s `-W`, `-H` and `-S` are the same three fields
//! from the implementation gap 28 is removing, which is evidence the seam is in
//! the right place rather than a coincidence worth hiding.
//!
//! # What is on those pages, since milestone 8
//!
//! **The book.** A spine item is read into an element tree by
//! [`xhtml`], cascaded against [`read::UA_STYLESHEET`] and the book's own
//! stylesheets by `tinker-pdf-css`, laid out and fragmented by
//! `tinker-pdf-layout`, and drawn by [`paint`]. A chapter takes as many pages
//! as its text needs, so **the page count is no longer the spine item count**
//! — which is milestone 4's own sentence read forwards: for a reflowable book
//! the page count is a function of the box the caller stated.
//!
//! What is still a placeholder is what a placeholder was always for: a spine
//! item this build cannot resolve, and an SVG content document, which is a
//! named non-goal. [`SpineDefect`] is the list, and it no longer has a variant
//! meaning *this build has no layout engine* — milestone 4 wrote that one down
//! in order to delete it here.
//!
//! # What the caller is told, and why the census is the number
//!
//! Every property a book asked for that this build does not implement reaches
//! the report as [`ArchiveWarning::UnimplementedProperty`], **counted by the
//! elements it affected** rather than by the declarations that asked. That is
//! the figure gap 31 says an `As built` is judged on: it is how much of
//! somebody else's format this build actually reads, measured on the book in
//! front of it rather than on a list of specifications.

pub mod nav;
pub mod ocf;
pub mod package;
pub mod paint;
pub mod read;
pub mod xhtml;

use tinker_pdf_cos::build::{OutlineEntry, Target};
use tinker_pdf_cos::dest::is_writable_uri;
use tinker_pdf_cos::DocumentBuilder;
use tinker_pdf_css::cascade::ComputedStyle;
use tinker_pdf_css::media::MediaContext;
use tinker_pdf_css::parser::{parse as css_parse, Stylesheet};
use tinker_pdf_css::{Budget as CssBudget, Limits as CssLimits, NoImports};
use tinker_pdf_layout::{
    layout_with, Budget as LayoutBudget, Limits as LayoutLimits, Options as LayoutOptions,
    Page as LayoutPage, Warning as LayoutWarning,
};
use tinker_pdf_xml::Limits as XmlLimits;
use tinker_pdf_zip::{limits as zip_limits, Archive};

use crate::cbz::{
    ArchiveRefusal, ArchiveReport, ArchiveWarning, PageOrigin, DOCUMENT_OVERHEAD,
    MAX_SYNTHESISED_PDF, PAGE_OVERHEAD, PLACEHOLDER_GREY,
};
use ocf::resolve_reference;
use package::{FallbackDefect, Package};
use paint::{draw_page, page_target, run_rect, BookMetrics, Fonts, Frame};

// ---- Bounds -----------------------------------------------------------------

/// The most bytes one container path may be.
///
/// OCF 3.3 §4.2.3's own number, taken rather than invented: *"content paths
/// MUST NOT exceed 65535 bytes"*. It is a bound in ruling 1's sense as well as
/// a validity rule, because a path is a `String` this engine builds out of an
/// attacker-chosen XML attribute value — and an attribute value may be as long
/// as the part that holds it.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends | 65 535 (the container built *past* this cap names a path of 65 536; the longest path in any real book here is 39) |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **65 535** |
///
/// Charged against the reference **before** it is merged with a base and before
/// its dot segments are removed, which is `tinker-pdf-zip`'s own posture: a
/// permit is what has been promised, not what happened to arrive. Charging
/// afterwards would mean allocating the 128 MiB path first and refusing it
/// second.
///
/// **Not** a cap on a container path's *depth*, and that absence is argued
/// rather than overlooked: nothing here touches a filesystem, so depth bounds
/// no allocation and no recursion — `remove_dot_segments` is a loop over a
/// `Vec` — and length is what costs. It is `tinker-pdf-zip`'s stated reason for
/// having no such cap either.
///
/// **Not** a cap on one *segment*, and that is a different absence with a
/// different argument. §4.2.3 also says a file name must not exceed 255 bytes,
/// and that clause is enforced by nothing here: it is an interoperability rule
/// about somebody else's file system, this engine never writes a file, and
/// refusing a path that names an entry the container actually holds would lose
/// a book over it. See [`ocf::resolve_reference`].
///
/// Reachable: `a_content_path_past_the_length_cap_is_refused_by_name` builds
/// the real 65 536-byte reference rather than lowering the constant, because a
/// cap proved only against a lowered copy of itself has not been proved to
/// fire.
pub const MAX_OCF_PATH_LEN: usize = 65_535;

/// Manifest items admitted from one package document (§5.6).
///
/// | | Items |
/// | --- | --- |
/// | The most any fixture in this repository spends | 8 192 (the manifest built *past* this cap; the largest real book here has 96) |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **8 192** |
///
/// A per-item cap and **not** a work cap, which is the distinction gap 31's
/// bounds table asks each of these to state: one manifest belongs to one
/// package document, one package document belongs to one book, and the count
/// here is the count in that one file. What the *product* of this and something
/// else would cost is milestone 6's `MAX_SELECTOR_MATCHES`, and it is named
/// there rather than pretended to be here.
///
/// Reachable: an `<item/>` is fourteen bytes, so a package document inside
/// `MAX_ZIP_ENTRY_BYTES` can name nine million of them.
pub const MAX_EPUB_MANIFEST_ITEMS: usize = 8_192;

/// Spine itemrefs admitted (§5.7), which at this milestone is the page count.
///
/// | | Itemrefs |
/// | --- | --- |
/// | The most any fixture in this repository spends | 4 096 (the spine built *past* this cap; the longest real spine here is 94) |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **4 096** |
///
/// **The manifest cap does not bound this**, and that is worth stating because
/// the `const` relation below puts them in order and could be read as claiming
/// it does. `epubcheck` reports two itemrefs naming one manifest item as
/// `OPF-034`, which is an error and not a well-formedness failure — so a
/// hostile package document may name one item four thousand times, exactly as
/// gap 30 found four thousand `PageContent` elements naming one part. The
/// ordering is a policy, the cap is the bound.
///
/// Reachable: an `<itemref/>` is twelve bytes.
pub const MAX_EPUB_SPINE_ITEMS: usize = 4_096;

/// How far §3.5.1's manifest fallback chain is followed.
///
/// | | Links |
/// | --- | --- |
/// | The most any fixture in this repository spends | 17 (the chain built *past* this cap; **no real book in either corpus carries a `fallback` attribute at all**) |
/// | A 200-page comic | 0 |
/// | A 200-page fixed document | 0 |
/// | **This cap** | **16** |
///
/// **The depth cap and the cycle guard are two rules and not one**, and both
/// are in [`Package::content_document`] under different names. A chain of
/// twenty distinct ids is not a cycle and a cycle of two is not deep; a build
/// with only the guard walks every chain to its end and a build with only the
/// cap reports a cycle as depth. Gap 30's `MAX_XPS_RESOURCE_DEPTH` found the
/// same pair one format over.
///
/// Reachable: the longest acyclic chain is one link per manifest item, so
/// [`MAX_EPUB_MANIFEST_ITEMS`] is the ceiling in front of it.
pub const MAX_EPUB_FALLBACK_DEPTH: usize = 16;

/// The relations, in a `const` block so a build that broke one **does not
/// compile**.
///
/// Gap 29's milestone 5 established the rung and gap 30's milestone 3 copied
/// it: a relation between constants is checked where the constants are, not in
/// a test that might not be run.
///
/// **`MAX_EPUB_PAGES` is deliberately absent from this list and from this
/// build.** Gap 31's bounds table names it and argues that it cannot be bounded
/// by the spine item count, because one spine item of 128 MiB of text fragments
/// into as many pages as its length allows. That argument is right and it is
/// about a build that fragments. This one does not: milestone 4 puts exactly
/// one page on each itemref, so a page cap above [`MAX_EPUB_SPINE_ITEMS`] could
/// never fire and a cap below it would be the spine cap wearing another name.
/// That is gap 18a milestone 8's failure reached from the direction that writes
/// the constant first, and `bounds_ledger.rs` already records the same argument
/// for `MAX_XPS_VISUAL_DEPTH`: **the bound arrives with the fragmentation or
/// not at all**, which is milestone 7.
const _: () = {
    assert!(
        MAX_EPUB_FALLBACK_DEPTH < MAX_EPUB_MANIFEST_ITEMS,
        "a fallback chain cannot be longer than the manifest it walks, so a \
         deeper cap could never fire"
    );
    assert!(
        MAX_EPUB_SPINE_ITEMS < MAX_EPUB_MANIFEST_ITEMS,
        "the spine cap is at or above the manifest cap"
    );
    assert!(
        MAX_EPUB_MANIFEST_ITEMS < zip_limits::MAX_ZIP_ENTRIES,
        "the manifest cap is at or above the archive reader's own entry cap, so \
         a book that reached it would have been refused first"
    );
};

/// Resource ceilings for reading an OCF container and its package document.
///
/// Separate from [`crate::cbz::Limits`] and from [`crate::xps::Limits`] for
/// their stated reason: they bound different things, and a caller that lowered
/// one should not silently lower another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Bytes of one container path. See [`MAX_OCF_PATH_LEN`].
    pub max_path_len: usize,
    /// Manifest items. See [`MAX_EPUB_MANIFEST_ITEMS`].
    pub max_manifest_items: usize,
    /// Spine itemrefs. See [`MAX_EPUB_SPINE_ITEMS`].
    pub max_spine_items: usize,
    /// Bytes of the document handed to the parser. See
    /// [`crate::cbz::MAX_SYNTHESISED_PDF`], reused rather than duplicated
    /// because it bounds the same thing for the same reason.
    pub max_synthesised: usize,
    /// What the markup reader is allowed to spend, per file.
    pub xml: XmlLimits,
    /// What the cascade is allowed to spend, per book.
    ///
    /// Held whole rather than flattened into fields here, because
    /// `tinker_pdf_css::Limits` already says what each of its numbers bounds
    /// and a copy of that reasoning in a second struct is a copy that goes out
    /// of date. The same argument [`XmlLimits`] is carried under, one crate
    /// over.
    pub css: CssLimits,
    /// What layout and fragmentation are allowed to spend, per book.
    pub layout: LayoutLimits,
}

impl Limits {
    /// The constants above, which is what [`Default`] hands back.
    pub const DEFAULT: Self = Self {
        max_path_len: MAX_OCF_PATH_LEN,
        max_manifest_items: MAX_EPUB_MANIFEST_ITEMS,
        max_spine_items: MAX_EPUB_SPINE_ITEMS,
        max_synthesised: MAX_SYNTHESISED_PDF,
        xml: XmlLimits::DEFAULT,
        css: CssLimits::DEFAULT,
        layout: LayoutLimits::DEFAULT,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// ---- The page box a book has no opinion about -------------------------------

/// The page box a reflowable book is laid out into when the caller states none:
/// **432 × 648 points, six inches by nine.**
///
/// A number has to be chosen and defended, exactly as gap 29 had to defend one
/// image pixel to one PDF point:
///
/// - It is the size the medium is printed at. A trade paperback is 6 × 9
///   inches, and an EPUB is a book rather than a report; US Letter would set
///   ninety-character lines no typographer would.
/// - `Page::size()` then reports a number a host can size a viewport with, and
///   `RenderOptions::default()` at 72 dpi renders it at 432 × 648 pixels.
/// - And it is **stated**, here, with the sentence that has to be said:
///   **for a reflowable book the page count is a function of this number and is
///   not a property of the file.** Two hosts that pass different boxes get
///   different books, on purpose.
pub const DEFAULT_PAGE: (f64, f64) = (432.0, 648.0);

/// The margin between the page box and the text, in points: **36, half an
/// inch**.
///
/// `tinker-pdf-layout`'s `Options` is a **content area** and not a page: that
/// crate says in as many words that page margins are the caller's, *"because a
/// reading system's margins are a presentation choice and this crate has no
/// opinion about them"*. This is the caller, and this is the opinion.
///
/// Half an inch is the smallest margin a trade paperback is set with, and the
/// number matters more than it looks: at 432 points wide it leaves 360 points
/// of measure, which at 12 point Times is about 66 characters a line — inside
/// the 45-to-75 band every typographic manual gives. A page with no margin at
/// all would set 79, and one with an inch would set 53 and add pages to every
/// book.
///
/// **It is not the `body` margin and does not replace it.** HTML's own
/// `body { margin: 8px }` is in `ua.css` and cascades like any other rule, so
/// a book that sets `body { margin: 0 }` gets its text 36 points from the page
/// edge rather than at it.
pub const PAGE_MARGIN: f64 = 36.0;

/// The base font size a reflowable book is laid out at, in points, when the
/// caller states none.
///
/// Twelve, which is what `1rem` and an unstyled paragraph resolve to. It has no
/// consumer at this milestone — line breaking is milestone 7 — and it is on the
/// type now rather than later because adding it later would change a page count
/// that callers had already calibrated against.
pub const DEFAULT_FONT_SIZE: f64 = 12.0;

/// The largest page side this build will lay a book into, in points.
///
/// PDF 1.7 Annex C.2 fixes the maximum coordinate of a page at 14 400 units —
/// two hundred inches — so a caller asking for more is asking for a page no
/// conforming reader will display. Clamped rather than refused, because it is
/// the *caller's* number rather than the file's and a host that passes nonsense
/// should still get its book.
pub const MAX_PAGE_SIDE: f64 = 14_400.0;

/// What a reflowable book needs decided before it has any pages.
///
/// Built from [`crate::OpenOptions`] by [`BookLayout::sanitised`], which is
/// where a caller's number stops being arbitrary. It is a plain `Copy` struct
/// and holds no font provider: a provider decides *metrics*, which is milestone
/// 7's business, and keeping it in the facade is what keeps this module off
/// `crate::fonts`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BookLayout {
    /// The page box, in points.
    pub page: (f64, f64),
    /// The base font size, in points.
    pub font_size: f64,
}

impl Default for BookLayout {
    fn default() -> Self {
        BookLayout {
            page: DEFAULT_PAGE,
            font_size: DEFAULT_FONT_SIZE,
        }
    }
}

/// Which of a caller's numbers could not be used.
///
/// Three names and not one, because a caller that passed a bad width and a bad
/// font size has two bugs, and a report that said "the options were unusable"
/// tells it neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BookOptionDefect {
    /// The page width is not finite, is not positive, or is past
    /// [`MAX_PAGE_SIDE`].
    PageWidth,
    /// The page height, likewise.
    PageHeight,
    /// The base font size is not finite, is not positive, or is larger than the
    /// page it would be set on.
    FontSize,
}

impl BookLayout {
    /// A usable layout, and what had to be replaced to get one.
    ///
    /// Each field is checked on its own and replaced with its default on its
    /// own, so a caller that passed a good box and a bad font size keeps the
    /// box. Nothing here refuses: a page box is the caller's number and a
    /// refusal would be an `OpenError` about the *file*, which would be a lie.
    #[must_use]
    pub fn sanitised(page: (f64, f64), font_size: f64) -> (BookLayout, Vec<BookOptionDefect>) {
        let usable = |value: f64| value.is_finite() && value > 0.0 && value <= MAX_PAGE_SIDE;
        let mut defects = Vec::new();
        let width = if usable(page.0) {
            page.0
        } else {
            defects.push(BookOptionDefect::PageWidth);
            DEFAULT_PAGE.0
        };
        let height = if usable(page.1) {
            page.1
        } else {
            defects.push(BookOptionDefect::PageHeight);
            DEFAULT_PAGE.1
        };
        // A font larger than the page it is set on paginates into one glyph per
        // page, or into none, and either way it is not a book. The bound is the
        // page rather than a constant, because a 14 400-point page can honestly
        // carry a 400-point heading.
        let font_size = if font_size.is_finite() && font_size > 0.0 && font_size <= height {
            font_size
        } else {
            defects.push(BookOptionDefect::FontSize);
            DEFAULT_FONT_SIZE
        };
        (
            BookLayout {
                page: (width, height),
                font_size,
            },
            defects,
        )
    }
}

// ---- What one spine item became ---------------------------------------------

/// Why a page is the neutral placeholder rather than the chapter it stands for.
///
/// **Every page carries one at this milestone**, which is the point: a page
/// that draws nothing and says nothing is a book of blank paper reported as a
/// success. [`SpineDefect::NotLaidOut`] is the one milestone 8 removes; the
/// rest are what a book can be wrong about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SpineDefect {
    /// The `idref` names no manifest item — or the attribute is absent, which
    /// §5.7.2 forbids.
    IdrefUnresolved,
    /// The manifest item's `href` is not a container path under §4.2.3.
    HrefUnusable,
    /// The `href` resolves to a container path the archive does not hold.
    ///
    /// Distinct from [`SpineDefect::HrefUnusable`] because the two blame
    /// different halves of the book: a reference that is not a path is the
    /// package document being wrong, and a path naming no entry is the
    /// container missing a file.
    ResourceMissing,
    /// The item is an SVG content document (§6.2).
    ///
    /// A named non-goal rather than a defect in the book: this build reads no
    /// SVG, and six spine items in the fetched corpus are one. Named separately
    /// from [`SpineDefect::NotLaidOut`] because milestone 8 removes that one and
    /// leaves this one standing.
    SvgContentDocument,
    /// §3.5.1's fallback chain does not reach an EPUB content document.
    Fallback(FallbackDefect),
    /// The cascade refused the document: one of `tinker-pdf-css`'s caps fired.
    ///
    /// **Named apart from [`SpineDefect::NotFragmented`]** because the two are
    /// different halves of the reader and a caller can act on the difference:
    /// a document with a million selectors is refused here and a document with
    /// a million line boxes is refused there, and a build that reported both as
    /// "the chapter did not lay out" would have merged the two things a bound
    /// exists to tell apart.
    NotStyled,
    /// Layout or fragmentation refused the document: one of
    /// `tinker-pdf-layout`'s caps fired, or the page box has no usable area.
    NotFragmented,
}

// ---- Routing ----------------------------------------------------------------

/// What one already-open archive turned out to be.
///
/// The archive travels **inside** this enum on the way out rather than being
/// opened twice, which is gap 30 milestone 3's exit criterion inherited whole:
/// opening it to sniff and again to read doubles the central-directory walk and
/// creates a window in which the two reads could disagree about the same bytes.
#[non_exhaustive]
pub enum Routing<'a> {
    /// An OCF container that paginated: the synthesised PDF and what it cost.
    Document(Vec<u8>, ArchiveReport),
    /// An OCF container this build will not read. **Refused by name**, which is
    /// the feature rather than the absence of one.
    Refused(ArchiveRefusal),
    /// Not an EPUB. The archive is handed back **unread** for the comic path.
    NotEpub(Archive<'a>),
}

/// Decides whether an open archive is an EPUB, and lays it out if it is.
///
/// The cheap half runs first and costs no read at all: a ZIP with no
/// `META-INF/container.xml` item is not an OCF container, which is every comic
/// archive there has ever been, and it comes straight back as
/// [`Routing::NotEpub`] having spent nothing.
#[must_use]
pub fn route<'a>(archive: Archive<'a>, limits: &Limits, layout: &BookLayout) -> Routing<'a> {
    if !ocf::is_container(&archive) {
        return Routing::NotEpub(archive);
    }
    let mut book = ocf::Ocf::open(archive, limits);
    match read(&mut book, limits, layout) {
        Ok((pdf, report)) => Routing::Document(pdf, report),
        Err(why) => Routing::Refused(why),
    }
}

/// Reads a book as far as this build goes, and synthesises a document from it.
///
/// The order is `container.xml`, then `encryption.xml`, then the package
/// document, and it is a decision rather than an accident. `container.xml` is
/// the file §4.2.6.3 **requires**, and a container whose required file will not
/// read has failed before the question of what is encrypted arises;
/// `encryption.xml` comes before the package document because a book whose
/// resources this engine holds no key for is refused rather than paginated into
/// a complete-looking book of nothing.
fn read(
    book: &mut ocf::Ocf<'_>,
    limits: &Limits,
    layout: &BookLayout,
) -> Result<(Vec<u8>, ArchiveReport), ArchiveRefusal> {
    // The rootfile is resolved as well as parsed: a container naming a package
    // document the book does not hold is a different sentence from one whose
    // markup will not read, and the two have different names. Which of them a
    // build reports is also the only observable difference between resolving
    // `full-path` against the container root and resolving it against
    // `META-INF/` — see `ocf::Ocf::default_rendition`.
    let rendition = book.default_rendition()?;
    let (path, index) = (rendition.path.clone(), rendition.index);
    // Read far enough to know whether the two font obfuscations are all that is
    // going on. Anything else is real encryption, this engine holds no key, and
    // a book whose resources it cannot read is refused by that name.
    book.encryption()?;

    let package = match book.read(index) {
        Ok(bytes) => package::parse(bytes, &path, limits)?,
        Err(_) => return Err(ArchiveRefusal::UnreadablePackageDocument),
    };
    synthesise(book, &package, limits, layout)
}

/// One spine item, read as far as it went.
struct Chapter {
    /// The container path the content document was read from, or the `idref`
    /// or `href` that named nothing.
    name: String,
    /// The container path, when there is one, for resolving a reference
    /// written **in** this document.
    path: Option<String>,
    /// Why this chapter is a placeholder, or `None` when it is a chapter.
    defect: Option<SpineDefect>,
    /// The element tree and its computed styles.
    reading: Option<read::Reading>,
    /// The pages this chapter's text needed, or empty for a placeholder.
    pages: Vec<LayoutPage>,
    /// The index of this chapter's first page in the whole document.
    first_page: usize,
}

impl Chapter {
    /// How many pages this chapter contributes.
    ///
    /// **A placeholder still contributes one**, which is the rule milestone 4
    /// established and this milestone does not get to relax: dropping a spine
    /// item does not renumber a page, it removes a chapter.
    fn page_count(&self) -> usize {
        self.pages.len().max(1)
    }

    /// The first page carrying text from `element` or from anything under it.
    ///
    /// Falls back to the chapter's own first page, which is the honest answer
    /// for a destination naming an element with no text in it at all: the
    /// chapter is where the reference points and the exact line is not
    /// knowable. Writing the page of whichever element happened to be nearest
    /// would be the plausible-and-wrong answer.
    fn page_of(&self, element: usize) -> usize {
        let Some(reading) = &self.reading else {
            return self.first_page;
        };
        for (offset, page) in self.pages.iter().enumerate() {
            for run in &page.runs {
                let Some(anchor) = run.anchor else {
                    continue;
                };
                if reading.dom.contains(element, anchor as usize) {
                    return self.first_page + offset;
                }
            }
        }
        self.first_page
    }
}

/// The user-agent stylesheet, parsed once per book.
///
/// Once per book and not once per spine item: a thirteen-chapter book would
/// otherwise tokenize the same four kilobytes thirteen times, and the sheet
/// cannot differ between them.
///
/// **The caller's base font size is not in here**, and the first draft of this
/// milestone put it here as a generated `html { font-size: Npt }` rule. That is
/// wrong in a way one of the two measured producers demonstrates: pandoc writes
/// `html, body, div, … { font-size: 100% }` as a reset on every book, an author
/// declaration that beats any user-agent rule — so a reading system's base size
/// would be silently discarded by two thirds of this corpus. A percentage on
/// the root resolves against the **initial** value, and the initial value is
/// the user's; [`tinker_pdf_css::cascade::cascade_from`] is where it goes.
fn ua_sheet(media: &MediaContext, limits: &CssLimits, budget: &mut CssBudget) -> Vec<Stylesheet> {
    match css_parse(
        read::UA_STYLESHEET.as_bytes(),
        None,
        &NoImports,
        media,
        limits,
        budget,
    ) {
        Ok(sheet) => vec![sheet],
        // Unreachable for the committed file, which
        // `the_committed_sheet_is_what_a_book_is_set_with` parses on every run;
        // reachable only if a caller lowered `Limits::css` below what four
        // kilobytes of CSS costs, and a book with no user-agent sheet is a book
        // with no block structure rather than a refusal.
        Err(_) => Vec::new(),
    }
}

/// The root element's initial values, carrying the caller's base font size.
///
/// `ComputedStyle::initial()` at [`crate::OpenOptions::font_size`], converted
/// from points to the CSS pixels the cascade computes in. Everything else is
/// the specification's own initial value.
fn initial_style(font_size: f64) -> ComputedStyle {
    let mut initial = ComputedStyle::initial();
    initial.font_size = font_size / read::PX_TO_PT;
    initial
}

/// Turns a spine into pages that read.
///
/// Three passes, and there are three because each needs the whole of the one
/// before it:
///
/// 1. **Every spine item is read, cascaded and laid out.** Nothing is written
///    yet, because the page a chapter starts on is not known until every
///    chapter before it has been fragmented.
/// 2. **Every face and every character is collected**, because a PDF's font
///    resources belong to the document rather than to a page, and a character
///    outside `WinAnsiEncoding` needs a code chosen before anything is drawn.
/// 3. **The document is written**: pages, link annotations and the outline, all
///    of which name page indices only the first pass could produce.
///
/// # Errors
/// [`ArchiveRefusal::TooLarge`] when the synthesised document is past
/// [`Limits::max_synthesised`].
pub fn synthesise(
    book: &mut ocf::Ocf<'_>,
    package: &Package,
    limits: &Limits,
    layout: &BookLayout,
) -> Result<(Vec<u8>, ArchiveReport), ArchiveRefusal> {
    // Charged before anything is built, on what the pages have undertaken to
    // contribute — `tinker-pdf-zip`'s `Budget` posture, where a permit is what
    // has been promised rather than what happened to arrive.
    DOCUMENT_OVERHEAD
        .checked_add(package.spine().len().saturating_mul(PAGE_OVERHEAD))
        .filter(|total| *total <= limits.max_synthesised)
        .ok_or(ArchiveRefusal::TooLarge)?;

    let mut warnings: Vec<ArchiveWarning> = Vec::new();
    // §4.3.2's clauses, which milestone 3 could only report through
    // `epub::ocf::Ocf` because a refused book carries no report at all. This is
    // the commit that gives a book one.
    for warning in book.warnings() {
        warnings.push(ArchiveWarning::Ocf(*warning));
    }
    for defect in package.defects() {
        warnings.push(ArchiveWarning::Package(*defect));
    }
    for (property, items) in package.unimplemented() {
        warnings.push(ArchiveWarning::UnimplementedFeature { property, items });
    }

    let frame = Frame {
        page: layout.page,
        margin: PAGE_MARGIN,
    };
    let (content_width, content_height) = frame.content_px();
    let media = MediaContext::screen(content_width, content_height);
    let options = LayoutOptions::new(content_width, content_height);
    let mut css_budget = CssBudget::new(&limits.css);
    let mut layout_budget = LayoutBudget::new(&limits.layout);
    let ua = ua_sheet(&media, &limits.css, &mut css_budget);
    let initial = initial_style(layout.font_size);
    let context = read::Context {
        ua: &ua,
        limits,
        css_limits: &limits.css,
        media: &media,
        initial: &initial,
    };

    // ---- pass 1: read, cascade, lay out ------------------------------------
    let mut census = read::Census::default();
    let mut chapters: Vec<Chapter> = Vec::with_capacity(package.spine().len());
    let mut layout_warnings: Vec<(LayoutWarning, usize)> = Vec::new();
    for itemref in package.spine() {
        let (name, path, mut defect) = plan_page(book, package, &itemref.idref);
        let mut reading = None;
        let mut pages = Vec::new();
        if defect.is_none() {
            let source = path.clone().unwrap_or_default();
            let bytes = book
                .index_of(&source)
                .and_then(|index| book.read(index).ok().map(<[u8]>::to_vec));
            match bytes {
                // Unreachable through `plan_page`, which already resolved the
                // path to an entry; kept because an entry that will not
                // *inflate* is a different failure from one that is not there,
                // and reporting it as the second would be a lie about the book.
                None => defect = Some(SpineDefect::ResourceMissing),
                Some(bytes) => {
                    match read::read_document(book, &source, &bytes, &context, &mut css_budget) {
                        Err(_) => defect = Some(SpineDefect::NotStyled),
                        Ok(document) => {
                            for markup in &document.dom.defects {
                                warnings.push(ArchiveWarning::Markup {
                                    item: name.clone(),
                                    defect: *markup,
                                });
                            }
                            census.absorb(&document.census);
                            match layout_with(
                                &document.tree,
                                &BookMetrics,
                                &options,
                                &limits.layout,
                                &mut layout_budget,
                            ) {
                                Err(_) => defect = Some(SpineDefect::NotFragmented),
                                Ok(laid) => {
                                    for (warning, count) in laid.warnings {
                                        match layout_warnings
                                            .iter_mut()
                                            .find(|(w, _)| *w == warning)
                                        {
                                            Some(slot) => slot.1 += count,
                                            None => layout_warnings.push((warning, count)),
                                        }
                                    }
                                    pages = laid.pages;
                                }
                            }
                            reading = Some(document);
                        }
                    }
                }
            }
        }
        chapters.push(Chapter {
            name,
            path,
            defect,
            reading,
            pages,
            first_page: 0,
        });
    }

    let mut at = 0usize;
    for chapter in &mut chapters {
        chapter.first_page = at;
        at += chapter.page_count();
    }
    let total_pages = at;

    // ---- pass 2: every face, and every character that needs a code ---------
    let mut fonts = Fonts::new();
    for chapter in &chapters {
        for page in &chapter.pages {
            for run in &page.runs {
                if run.painted {
                    fonts.note(run);
                }
            }
        }
    }

    // ---- what the caller is told, before any of it is drawn ----------------
    for (property, elements) in census.ranked() {
        warnings.push(ArchiveWarning::UnimplementedProperty { property, elements });
    }
    for (warning, count) in &layout_warnings {
        warnings.push(ArchiveWarning::Layout {
            warning: warning.clone(),
            count: *count,
        });
    }
    if fonts.unrepresented() > 0 {
        warnings.push(ArchiveWarning::UnrepresentedCharacters {
            characters: fonts.unrepresented(),
        });
    }

    // ---- pass 3: write it --------------------------------------------------
    let (width, height) = layout.page;
    let mut builder = DocumentBuilder::new();
    // §5.5.3.1's metadata, reaching the document it describes. Without this the
    // three required elements would be parsed and thrown away, which is exactly
    // the failure gap 31 is organised around — one level up from a CSS property.
    if let Some(title) = package.title() {
        builder.set_info(b"Title", title);
    }
    if let Some(creator) = package.creator() {
        builder.set_info(b"Author", creator);
    }
    fonts.register(&mut builder);

    let links = cross_references(&chapters, &frame, limits, total_pages);

    let mut pages: Vec<PageOrigin> = Vec::with_capacity(total_pages);
    for chapter in &chapters {
        if let Some(defect) = chapter.defect {
            let page = u32::try_from(chapter.first_page).unwrap_or(u32::MAX);
            warnings.push(ArchiveWarning::SpinePage { page, defect });
            builder.add_page(width, height, |page| {
                page.fill_rect(0.0, 0.0, width, height, PLACEHOLDER_GREY);
            });
            pages.push(PageOrigin {
                name: chapter.name.clone(),
                defect: None,
            });
            continue;
        }
        // A content document whose text fitted nowhere — a cover whose only
        // content is an SVG, which four of the six committed books have — still
        // gets its page, for the same reason a placeholder does.
        if chapter.pages.is_empty() {
            builder.add_page(width, height, |_| {});
            pages.push(PageOrigin {
                name: chapter.name.clone(),
                defect: None,
            });
            continue;
        }
        for (offset, laid) in chapter.pages.iter().enumerate() {
            let index = chapter.first_page + offset;
            let on_page = links.get(index).map_or(&[][..], Vec::as_slice);
            builder.add_page(width, height, |page| {
                draw_page(page, laid, &frame, &fonts);
                for (rect, target) in on_page {
                    page.link(rect.0, rect.1, rect.2, rect.3, target);
                }
            });
            pages.push(PageOrigin {
                name: chapter.name.clone(),
                defect: None,
            });
        }
    }

    let entries = outline(book, package, &chapters, limits, total_pages);
    if !entries.is_empty() && !builder.set_outline(entries) {
        warnings.push(ArchiveWarning::OutlineUnwritable);
    }

    // Taken after every read, because reading is what most of them come from.
    for warning in book.archive().warnings() {
        warnings.push(ArchiveWarning::Zip(*warning));
    }

    let pdf = builder.finish();
    if pdf.len() > limits.max_synthesised {
        return Err(ArchiveRefusal::TooLarge);
    }
    let synthesised_bytes = pdf.len();
    Ok((
        pdf,
        ArchiveReport::book(warnings, pages, synthesised_bytes, *layout),
    ))
}

/// Where one reference points, once the spine has been paginated.
///
/// The one function both an `<a href>` and a table-of-contents entry go
/// through, which is why it takes a reference and a referring path rather than
/// an element. A cross-reference and an outline entry naming the same chapter
/// and disagreeing about which page it is on would be two resolvers, and the
/// disagreement would be invisible in every rendered comparison there is.
fn resolve_target(
    chapters: &[Chapter],
    referring: Option<&str>,
    href: &str,
    limits: &Limits,
    total_pages: usize,
) -> Option<Target> {
    let trimmed = href.trim();
    if trimmed.is_empty() {
        return None;
    }
    // RFC 3986 §3.1: a scheme is a letter followed by letters, digits, `+`,
    // `-` and `.`, ending at the first `:` — and it is looked for only in the
    // reference's first segment, which is how `ocf::resolve_reference` decides
    // the same question. Asking it the same way in both places is what stops
    // `ch1.xhtml#a:b` from being read as a scheme.
    let scheme = trimmed
        .split('/')
        .next()
        .unwrap_or_default()
        .split_once(':')
        .map(|(scheme, _)| scheme)
        .filter(|scheme| {
            !scheme.is_empty()
                && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        });
    if scheme.is_some() {
        // 12.6.4.7's `/URI` is 7-bit ASCII; `is_writable_uri` is the writer's
        // own predicate, asked here so a refusal is a link this build did not
        // write rather than a `link()` that silently returned false.
        return is_writable_uri(trimmed).then(|| Target::Uri(trimmed.to_owned()));
    }

    let (path, fragment) = match trimmed.split_once('#') {
        Some((path, fragment)) => (path, Some(fragment)),
        None => (trimmed, None),
    };
    // An empty path is a same-document reference, which is what every
    // `href="#toc"` in this corpus is.
    let target = if path.is_empty() {
        referring?.to_owned()
    } else {
        resolve_reference(referring?, path, limits).ok()?
    };

    let chapter = chapters
        .iter()
        .find(|chapter| chapter.path.as_deref() == Some(target.as_str()))?;
    let page = match fragment.filter(|f| !f.is_empty()) {
        Some(fragment) => {
            let element = chapter
                .reading
                .as_ref()
                .and_then(|reading| reading.dom.by_id(fragment));
            match element {
                Some(element) => chapter.page_of(element),
                // A fragment naming nothing points at the chapter, which is
                // what a reading system does and is a different answer from
                // pointing nowhere: the reference is not broken, the anchor is.
                None => chapter.first_page,
            }
        }
        None => chapter.first_page,
    };
    (page < total_pages).then(|| page_target(u32::try_from(page).unwrap_or(u32::MAX)))
}

/// One page's link annotations: a rectangle in the page's own points, and
/// where it goes.
type PageLinks = Vec<((f64, f64, f64, f64), Target)>;

/// Every `<a href>` in the book, as a rectangle on a page and a target.
///
/// Indexed by page, so the writer can hand one page its own annotations
/// without searching the whole book for them.
fn cross_references(
    chapters: &[Chapter],
    frame: &Frame,
    limits: &Limits,
    total_pages: usize,
) -> Vec<PageLinks> {
    let mut out: Vec<PageLinks> = vec![Vec::new(); total_pages];
    for chapter in chapters {
        let Some(reading) = &chapter.reading else {
            continue;
        };
        for (index, node) in reading.dom.nodes.iter().enumerate() {
            if !node.is_html() || node.name != "a" {
                continue;
            }
            let Some(href) = node.attr("href") else {
                continue;
            };
            let Some(target) =
                resolve_target(chapters, chapter.path.as_deref(), href, limits, total_pages)
            else {
                continue;
            };
            // A link's rectangles are its **runs'**, one each, and not one box
            // round the lot: an anchor broken across a line break occupies two
            // rectangles with the whole of the line between them, and a single
            // bounding box would make every word on that line part of the link.
            for (offset, page) in chapter.pages.iter().enumerate() {
                let at = chapter.first_page + offset;
                for run in &page.runs {
                    let Some(anchor) = run.anchor else {
                        continue;
                    };
                    if !reading.dom.contains(index, anchor as usize) {
                        continue;
                    }
                    if let Some(slot) = out.get_mut(at) {
                        slot.push((run_rect(run, frame), target.clone()));
                    }
                }
            }
        }
    }
    out
}

/// The document outline, from the navigation document or the NCX.
///
/// 5.4's navigation document is asked first because it is what EPUB 3
/// requires; the NCX is the fallback and not the other way round, so a book
/// carrying both -- which four of the six committed ones do -- gets the
/// outline its own version says is authoritative.
fn outline(
    book: &mut ocf::Ocf<'_>,
    package: &Package,
    chapters: &[Chapter],
    limits: &Limits,
    total_pages: usize,
) -> Vec<OutlineEntry> {
    let (entries, base) = navigation(book, package, limits);
    entries
        .iter()
        .map(|entry| convert(entry, base.as_deref(), chapters, limits, total_pages))
        .collect()
}

/// The table of contents as the book wrote it, and **the document it was
/// written in**.
///
/// The base comes back beside the entries rather than being folded into each
/// `href`, because it is one fact about the whole tree and folding it in would
/// mean rewriting every entry's reference into a spelling the book never used.
/// It matters: a navigation document is not always beside the content it names
/// -- pandoc puts `nav.xhtml` in `EPUB/` and the chapters in `EPUB/text/` -- so
/// a build that resolved a navigation `href` against the *chapter* would find
/// nothing, and one that resolved it against the container root would find
/// nothing for pandoc at all.
fn navigation(
    book: &mut ocf::Ocf<'_>,
    package: &Package,
    limits: &Limits,
) -> (Vec<nav::NavEntry>, Option<String>) {
    if let Some(path) = package.nav().and_then(|item| item.path.clone()) {
        if let Some(index) = book.index_of(&path) {
            if let Ok(bytes) = book.read(index).map(<[u8]>::to_vec) {
                if let Ok(dom) = xhtml::read(&bytes, &limits.xml) {
                    let entries = nav::from_navigation_document(&dom);
                    if !entries.is_empty() {
                        return (entries, Some(path));
                    }
                }
            }
        }
    }
    let ncx = package
        .toc()
        .and_then(|toc| package.item_by_id(toc))
        .and_then(|item| item.path.clone());
    let Some(path) = ncx else {
        return (Vec::new(), None);
    };
    let Some(index) = book.index_of(&path) else {
        return (Vec::new(), None);
    };
    let Ok(bytes) = book.read(index).map(<[u8]>::to_vec) else {
        return (Vec::new(), None);
    };
    (nav::from_ncx(&bytes, &limits.xml), Some(path))
}

/// One navigation entry, with its target resolved.
fn convert(
    entry: &nav::NavEntry,
    base: Option<&str>,
    chapters: &[Chapter],
    limits: &Limits,
    total_pages: usize,
) -> OutlineEntry {
    let target = entry
        .href
        .as_ref()
        .and_then(|href| resolve_target(chapters, base, href, limits, total_pages));
    OutlineEntry {
        title: entry.title.clone(),
        target,
        // 12.3.3 spells "shown expanded" as the sign of `/Count`, and a table
        // of contents a reader has to expand to see is a table of contents a
        // reader will not see.
        open: true,
        children: entry
            .children
            .iter()
            .map(|child| convert(child, base, chapters, limits, total_pages))
            .collect(),
    }
}

/// What one itemref becomes: the name its page reports, the container path it
/// was read from, and why that page is a placeholder rather than a chapter.
///
/// **A spine item that does not resolve still produces a page and keeps its
/// position**, which is why the defect is a value rather than an absence.
/// Dropping it would not renumber a page, it would remove a chapter — and a
/// book that jumps from chapter 4 to chapter 6 reads as a bad conversion rather
/// than as a bug.
fn plan_page(
    book: &ocf::Ocf<'_>,
    package: &Package,
    idref: &str,
) -> (String, Option<String>, Option<SpineDefect>) {
    let Some(item) = package.item_by_id(idref) else {
        return (idref.to_owned(), None, Some(SpineDefect::IdrefUnresolved));
    };
    // §3.5.1 before §4.2.5: which resource a spine item stands for is decided by
    // the fallback chain, and the *terminus* is the file that would be read.
    // Charging the href of an item the reading system was never going to open
    // would report the wrong name.
    let target = match package.content_document(item) {
        Ok(target) => target,
        Err(defect) => return (item.href.clone(), None, Some(SpineDefect::Fallback(defect))),
    };
    let Some(path) = target.path.as_deref() else {
        return (target.href.clone(), None, Some(SpineDefect::HrefUnusable));
    };
    if book.index_of(path).is_none() {
        return (path.to_owned(), None, Some(SpineDefect::ResourceMissing));
    }
    // An SVG content document (§6.2) is a named non-goal: this build reads no
    // SVG, six spine items in the fetched corpus are one, and laying one out as
    // though its markup were XHTML would set the `<title>` and the `<desc>` as
    // body text. It keeps its page and its position.
    let defect = matches!(target.core, Some(package::CoreMediaType::Svg))
        .then_some(SpineDefect::SvgContentDocument);
    (path.to_owned(), Some(path.to_owned()), defect)
}

#[cfg(test)]
mod tests;
