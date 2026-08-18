//! An XPS package opens as the fixed document it is (gap 30, milestones 3, 4
//! and 6).
//!
//! # The defect this module exists to remove
//!
//! Gap 29 taught [`crate::Document::open`] to sniff `PK\x03\x04`, and **one
//! signature covers CBZ, XPS, EPUB, ODF, OOXML and every JAR ever built**.
//! Before this module, a real `.xps` opened as a comic: milestone 1 measured
//! `wpf-image-and-text.xps` — one 816 x 1056 fixed page, US Letter to the point
//! — reporting `page_count()` 1 and `Page::size()` `(32.0, 32.0)`, because the
//! "page" was a PNG *resource* at its own pixel size. The markup, the text, the
//! obfuscated font and the page size were discarded and **nothing warned**,
//! because from `cbz.rs`'s point of view nothing had gone wrong: it found one
//! image entry and paged it.
//!
//! It was worse than one file. The page count was the count of *raster parts*,
//! so a three-page document was refused as [`ArchiveRefusal::NoImages`] and a
//! one-page document whose two brushes shared a PNG reported one page for a
//! reason that had nothing to do with the document. Five of milestone 1's eight
//! packages refused, three opened as one-page comics, and **not one was read as
//! the document it is**.
//!
//! That is gap 17's blank-page-as-success arriving in the facade, and it is why
//! this milestone is early: it is the only one in gap 30 that improves matters
//! on its own.
//!
//! # The discrimination is ECMA-388 E.3's, and the order is the point
//!
//! E.3's recipe is informative in the standard and exactly right here:
//!
//! 1. the bytes are a ZIP — already true, by the offset-zero signature;
//! 2. an item named `[Content_Types].xml` exists **and** a package
//!    relationships part `_rels/.rels` exists;
//! 3. `_rels/.rels` parses and carries a relationship of either dialect's
//!    fixed-representation type whose target resolves to a part whose media
//!    type is the FixedDocumentSequence one.
//!
//! **A comic archive that happens to carry a `[Content_Types].xml` is still a
//! comic archive.** That case decides the order, and it is a fixture rather
//! than a remark: step 2 wants both items, so a CBZ with a content-types part
//! and no relationships part never reaches a read.
//!
//! One refinement of E.3, made here deliberately and recorded so it is not read
//! as a slip. When step 2 holds and step 3 fails, the two failures are not the
//! same:
//!
//! - **No fixed representation at all** — a `.docx`, an `.odf`, anything else
//!   OPC — is "not an XPS", and falls through to the comic path exactly as the
//!   plan says. That direction is unchanged.
//! - **A fixed representation that is there and will not resolve** — a
//!   content-types item that is not well-formed XML, a target naming a part the
//!   package does not hold, a media type that is not the sequence's — is
//!   [`ArchiveRefusal::UnreadablePackage`]. This *is* an XPS and it is broken,
//!   and paging its images would be the original defect wearing a smaller hat.
//!
//! # Two dialects, one reader
//!
//! ECMA-388 Table D-2 gives OpenXPS's namespace as
//! `http://schemas.openxps.org/oxps/v1.0` and every Microsoft serialiser writes
//! XPS 1.0's `http://schemas.microsoft.com/xps/2005/06`. Both are accepted, on
//! elements **and** on relationship types, and a package mixing them is
//! accepted too, because nothing in either specification makes that an error.
//!
//! **The content type does not discriminate the dialect**, and milestone 1
//! settled it by measurement rather than by reading: Windows' OpenXPS output
//! carries ECMA-388 Table D-4's `xps-` strings, byte-identical to XPS 1.0's, and
//! the substring `oxps-` appears in no content-types item in the corpus at all.
//! So the obvious sniff is the wrong one, and the namespace is the only
//! discriminator there is.
//!
//! # What a page is, and what it is not
//!
//! The spine is read — `FixedDocumentSequence` to `FixedDocument` to
//! `FixedPage` — and each page is synthesised **at the size its own markup
//! states**. ECMA-388 18.1 puts one XPS unit at 1/96 inch against PDF's 1/72,
//! so `Width="816" Height="1056"` is 612 x 792 pt, which is US Letter to the
//! point.
//!
//! **Milestone 6 is where a page first draws.** Paths, brushes, transforms,
//! clips, canvases and resource dictionaries are [`paint`], [`geometry`],
//! [`brush`] and [`markup`]; the page's content stream opens with one `cm`
//! carrying 18.1's unit *and* its top-left origin, so the numbers in the stream
//! are the numbers in the markup. Two of milestone 1's eight real packages now
//! render with nothing owed at all.
//!
//! What is owed is owed **by name, at the element**: a `Glyphs` run is
//! milestone 7's and an `ImageBrush` is milestone 8's, and each says so through
//! [`XpsElementDefect`] rather than leaving a page that quietly drew most of
//! itself. [`XpsPageDefect`] is now what it says: a page that is a
//! **placeholder** rather than a page that failed to be finished. Milestones 3
//! to 5 gave every page `NotDrawn`; that variant is deleted rather than kept as
//! a shape nothing produces, which is the argument milestone 3 used one level
//! up when it deleted the test that recorded the old behaviour.
//!
//! # Three things milestone 4 decided, each of which has a wrong answer that
//! looks right
//!
//! **Pages come in markup order.** 12.3.1 gives a `FixedDocument`'s pages in
//! the order its `PageContent` elements appear and defines no other order at
//! all, so the archive's own storage order and any sort over the part names are
//! three more answers that are wrong on any document whose producer did not
//! happen to agree with them. Gap 29's comic path sorts by name because a CBZ
//! has nothing else to sort by; this one never sorts. No package in milestone
//! 1's corpus can tell the difference — all eight name their pages `1`, `2`,
//! `3` and reference them in that order — which is why `tests/xps_spine.rs`
//! builds one that can.
//!
//! **The payload is resolved by media type.** A `DocumentReference` and a
//! `PageContent` each name a part, and what makes that part a fixed document or
//! a fixed page is OPC 7.2.3.5's answer over `[Content_Types].xml`, never the
//! extension its name happens to end in — which is what Table D-4 is for. A
//! reference to the wrong kind of part is a page that keeps its number and
//! carries [`XpsPageDefect::MediaTypeMismatch`], because a `PageContent`
//! pointing at a PNG is a broken document and not a page that failed to draw.
//! Milestone 3 resolved these by root element alone and recorded it as owed.
//!
//! **10.3's two rectangles survive.** `ContentBox` becomes `/CropBox` and
//! `BleedBox` becomes `/BleedBox`, each scaled by [`UNITS_TO_POINTS`] exactly
//! once and flipped from XPS's top-left origin exactly once. They are the only
//! statement a fixed page makes about where its content is, and a synthesised
//! document that dropped them would lose it for good. `page_box`'s own comment
//! carries the arithmetic and what each of the plausible mistakes produces
//! instead.

pub mod brush;
pub mod geometry;
pub mod markup;
pub mod opc;
pub mod paint;

use std::collections::HashMap;

use tinker_pdf_cos::DocumentBuilder;
use tinker_pdf_xml::{Event, Limits as XmlLimits, Source};
use tinker_pdf_zip::{limits as zip_limits, Archive};

use crate::cbz::{
    ArchiveRefusal, ArchiveReport, ArchiveWarning, PageOrigin, DOCUMENT_OVERHEAD,
    MAX_SYNTHESISED_PDF, PAGE_OVERHEAD, PLACEHOLDER_GREY,
};
use markup::{Budget, Trouble};
use opc::{Package, PackageProblem, PartName};
use paint::{Drawn, Painter};

// ---- Bounds -----------------------------------------------------------------

/// The most parts one package may hold.
///
/// Gap 18a milestone 8 found `MAX_JPX_WORK` set *above* the most its own inputs
/// could ask for, so it could never fire. That decides this number in one
/// direction: [`zip_limits::MAX_ZIP_ENTRIES`] is 16 384, so a cap at or above
/// that could never be the thing that stopped anything.
///
/// | | Parts |
/// | --- | --- |
/// | The most any fixture in this repository spends | 8 192 (the package built *past* this cap holds 8 193; the largest real package here holds 7) |
/// | A 200-page fixed document | 505 |
/// | **This cap** | **8 192** |
///
/// The yardstick is gap 30's own, named in its bounds section: a 200-page fixed
/// document at roughly 2 000 drawable elements and 40 000 path segments a page.
/// Its parts are one sequence, one document, two hundred pages, two hundred
/// page relationships parts, the package relationships part and about a hundred
/// fonts and images — 505, against a cap sixteen times that.
///
/// Reachable: `a_package_past_the_part_cap_is_refused_by_name` builds the real
/// 8 193-part package rather than lowering the constant, because a cap proved
/// only against a lowered copy of itself has not been proved to fire.
pub const MAX_XPS_PARTS: usize = 8_192;

/// The most fixed pages one package may synthesise.
///
/// | | Pages |
/// | --- | --- |
/// | The most any fixture in this repository spends | 4 096 (the document built *past* this cap names 4 097; nothing else here names more than 3) |
/// | A 200-page fixed document | 200 |
/// | **This cap** | **4 096** |
///
/// A page costs an object graph as well as a `PageContent` element, which is
/// why [`MAX_XPS_PARTS`] is not this cap: two hundred `PageContent` elements
/// may name **one** part between them, so the page count is not bounded by the
/// part count at all. What bounds it before this is
/// `tinker_pdf_xml::limits::MAX_XML_TOKENS` — one fixed document part may
/// produce a million events, so a million pages — and this sits four hundred
/// times below that.
///
/// Reachable: `a_page_count_past_the_xps_cap_is_refused_by_name` builds a fixed
/// document naming 4 097 pages.
pub const MAX_XPS_PAGES: usize = 4_096;

/// The most drawable markup elements one package's pages may cost.
///
/// **A total, and this is the row that says why one is needed.** A per-page cap
/// times a page count the file chooses is not a bound — `5adf502`'s finding in
/// a third format, and gap 30's own sentence about it. Both halves of the work
/// are charged against this one number: an element materialised into a
/// property-element value, and an element the drawing walk visits.
///
/// | | Elements |
/// | --- | --- |
/// | The most any fixture in this repository spends | 1 048 576 (the page built *past* this cap holds one more; the largest real package here holds 6 on a page) |
/// | A 200-page fixed document | 400 000 |
/// | **This cap** | **1 048 576** |
///
/// The yardstick is gap 30's own: a 200-page fixed document at roughly 2 000
/// drawable elements a page, every page a distinct part, is 400 000 — against a
/// cap two and a half times that. What stands in front of it is much larger
/// still: [`MAX_XPS_PARTS`] parts each producing up to
/// `tinker_pdf_xml::limits::MAX_XML_TOKENS` events, of which every second one
/// can be a start tag, so the ceiling is in the billions and a cap here can
/// always be the thing that stopped something.
///
/// Reachable: `a_page_past_the_element_cap_is_refused_by_name` builds the real
/// page rather than lowering the constant.
pub const MAX_XPS_ELEMENTS: usize = 1 << 20;

/// The most path segments one package's geometry may produce.
///
/// **A total, for [`MAX_XPS_ELEMENTS`]'s reason and one of its own**: `L 0,0`
/// is six bytes, so one `Data` attribute in a part this build already admits —
/// 128 MiB, [`zip_limits::MAX_ZIP_ENTRY_BYTES`] — is twenty-two million
/// segments, and a per-path cap does not see it.
///
/// | | Segments |
/// | --- | --- |
/// | The most any fixture in this repository spends | 8 388 608 (the geometry built *past* this cap holds one more) |
/// | A 200-page fixed document | 8 000 000 |
/// | **This cap** | **8 388 608** |
///
/// The margin over the yardstick is deliberately narrow, and the reason is
/// that **this cap is also the peak-memory bound**. A segment is materialised
/// before it is written — it has to be, because a geometry's bounding box is
/// what decides a transparency group's `/BBox` and a canvas's overlap — so the
/// worst case is one geometry holding the whole total, which at 56 bytes a
/// segment is about 470 MiB. That sits under `MAX_ZIP_INFLATED`'s 1 GiB, which
/// this build already admits, and under what the same segments would become as
/// operator bytes against [`MAX_SYNTHESISED_PDF`]. A larger cap would buy
/// headroom over a yardstick nothing has ever reached and pay for it in the
/// one number an attacker can drive.
///
/// Reachable: `a_geometry_past_the_segment_cap_is_refused_by_name`.
pub const MAX_XPS_SEGMENTS: usize = 1 << 23;

/// How long a chain of `{StaticResource}` aliases may be.
///
/// 14.2.5 resolves a reference in the nearest enclosing `ResourceDictionary`
/// and then outward, and a dictionary entry may itself name another — so the
/// chain's length is the file's to choose and **a cycle is two lines of
/// markup**. ECMA-388 18.2 recommends sixteen as a consumer limit and [M11.5]
/// makes refusing past a consumer's own limit conformant.
///
/// | | Aliases deep |
/// | --- | --- |
/// | The most any fixture in this repository spends | 16 |
/// | A 200-page fixed document | 2 |
/// | **This cap** | **16** |
///
/// **The depth cap and the cycle guard are two rules and not one.** A chain of
/// twenty distinct keys is not a cycle, and a cycle of two keys is not deep;
/// deleting either leaves the other passing every test written for it, which
/// is the shape gap 30's milestones 2 to 5 each found once. They answer under
/// different names — [`XpsElementDefect::BrushTooDeep`] and
/// [`XpsElementDefect::BrushCyclic`] — so a report can tell them apart.
///
/// Reachable: `a_static_resource_chain_past_the_depth_cap_is_named`, against
/// `tinker_pdf_xml::limits::MAX_XML_TOKENS` entries one dictionary could hold.
pub const MAX_XPS_RESOURCE_DEPTH: usize = 16;

/// The two relations, in `const` blocks so a build that broke either **does not
/// compile**.
///
/// Gap 29's milestone 5 established the rung: a relation between constants is
/// checked where the constants are, not in a test that might not be run.
/// `MAX_XPS_PAGES < MAX_XPS_PARTS < MAX_ZIP_ENTRIES` is written the same way
/// and for the same reason — a cap at or above the one in front of it can never
/// be the thing that stopped anything.
const _: () = {
    assert!(
        MAX_XPS_PAGES < MAX_XPS_PARTS,
        "the page cap is at or above the part cap"
    );
    assert!(
        MAX_XPS_PARTS < zip_limits::MAX_ZIP_ENTRIES,
        "the part cap is at or above the archive reader's own entry cap, so it could never fire"
    );
};

/// ECMA-388 18.1: one XPS unit is 1/96 inch and one PDF unit is 1/72, so a
/// fixed page's `Width` and `Height` are scaled by exactly three quarters.
///
/// 816 x 1056 becomes 612 x 792, which is US Letter to the point — verified
/// against every package in milestone 1's corpus, in both dialects.
pub const UNITS_TO_POINTS: f64 = 0.75;

/// The size a page falls back to when the book never states a usable one.
///
/// US Letter, and only ever reached by a document in which **no** fixed page
/// stated a size this reader could use — every ordinary page takes the first
/// usable size in the book instead, so a placeholder matches its neighbours and
/// a reader can page through it.
const FALLBACK_PAGE: (f64, f64) = (612.0, 792.0);

/// The largest `Width` or `Height`, in XPS units, a fixed page may state.
///
/// **Not a resource bound and deliberately not in the ledger**: it allocates
/// nothing and refuses nothing that a document could want — 10 416 inches is
/// about a sixth of a mile — it is a sanity check on a number the file chooses,
/// and a page past it degrades to the book's own size with
/// [`XpsPageDefect::SizeUnusable`] rather than refusing. Writing down why a cap
/// is *not* a cap is the cheaper half of ruling 1's discipline.
const MAX_PAGE_UNITS: f64 = 1.0e6;

// ---- The two dialects -------------------------------------------------------

/// XPS 1.0's namespace, which is what every Microsoft serialiser writes.
pub const XPS_1_0_NAMESPACE: &str = "http://schemas.microsoft.com/xps/2005/06";

/// ECMA-388 Table D-2's OpenXPS namespace.
pub const OPENXPS_NAMESPACE: &str = "http://schemas.openxps.org/oxps/v1.0";

/// XPS 1.0's fixed-representation relationship type.
pub const XPS_1_0_FIXED_REPRESENTATION: &str =
    "http://schemas.microsoft.com/xps/2005/06/fixedrepresentation";

/// OpenXPS's fixed-representation relationship type.
pub const OPENXPS_FIXED_REPRESENTATION: &str =
    "http://schemas.openxps.org/oxps/v1.0/fixedrepresentation";

/// XPS 1.0's required-resource relationship type.
pub const XPS_1_0_REQUIRED_RESOURCE: &str =
    "http://schemas.microsoft.com/xps/2005/06/required-resource";

/// OpenXPS's required-resource relationship type.
pub const OPENXPS_REQUIRED_RESOURCE: &str =
    "http://schemas.openxps.org/oxps/v1.0/required-resource";

/// ECMA-388 Table D-4's media type for a `FixedDocumentSequence` part.
///
/// **Both dialects carry this string.** Milestone 1 measured it: the sources
/// reporting an `oxps-`-prefixed twin are wrong about what Windows writes, and
/// no package in the corpus contains the substring `oxps-` in its content-types
/// item at all.
pub const FIXED_DOCUMENT_SEQUENCE_MEDIA_TYPE: &str =
    "application/vnd.ms-package.xps-fixeddocumentsequence+xml";

/// Table D-4's media type for a `FixedDocument` part, in both dialects.
pub const FIXED_DOCUMENT_MEDIA_TYPE: &str = "application/vnd.ms-package.xps-fixeddocument+xml";

/// Table D-4's media type for a `FixedPage` part, in both dialects.
pub const FIXED_PAGE_MEDIA_TYPE: &str = "application/vnd.ms-package.xps-fixedpage+xml";

/// Which spelling of the one vocabulary a package used.
///
/// Read from the **namespace** and never from the content type, for the reason
/// in this module's header. Kept rather than discarded because a report that
/// cannot say which dialect a file was cannot help anybody debug one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dialect {
    /// `http://schemas.microsoft.com/xps/2005/06`.
    Xps1,
    /// `http://schemas.openxps.org/oxps/v1.0`.
    OpenXps,
}

impl Dialect {
    /// The element namespace this dialect uses.
    #[must_use]
    pub fn namespace(self) -> &'static str {
        match self {
            Dialect::Xps1 => XPS_1_0_NAMESPACE,
            Dialect::OpenXps => OPENXPS_NAMESPACE,
        }
    }

    /// The fixed-representation relationship type this dialect uses.
    #[must_use]
    pub fn fixed_representation(self) -> &'static str {
        match self {
            Dialect::Xps1 => XPS_1_0_FIXED_REPRESENTATION,
            Dialect::OpenXps => OPENXPS_FIXED_REPRESENTATION,
        }
    }

    /// The required-resource relationship type this dialect uses.
    #[must_use]
    pub fn required_resource(self) -> &'static str {
        match self {
            Dialect::Xps1 => XPS_1_0_REQUIRED_RESOURCE,
            Dialect::OpenXps => OPENXPS_REQUIRED_RESOURCE,
        }
    }
}

/// Whether an element namespace is one of the two dialects.
#[must_use]
pub fn dialect_of(namespace: Option<&str>) -> Option<Dialect> {
    match namespace {
        Some(XPS_1_0_NAMESPACE) => Some(Dialect::Xps1),
        Some(OPENXPS_NAMESPACE) => Some(Dialect::OpenXps),
        _ => None,
    }
}

/// Whether a relationship type is one of the two fixed-representation types.
#[must_use]
pub fn fixed_representation_dialect(kind: &str) -> Option<Dialect> {
    match kind {
        XPS_1_0_FIXED_REPRESENTATION => Some(Dialect::Xps1),
        OPENXPS_FIXED_REPRESENTATION => Some(Dialect::OpenXps),
        _ => None,
    }
}

// ---- Limits -----------------------------------------------------------------

/// Resource ceilings for reading a package and synthesising a document from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Parts admitted from one package. See [`MAX_XPS_PARTS`].
    pub max_parts: usize,
    /// Fixed pages synthesised. See [`MAX_XPS_PAGES`].
    pub max_pages: usize,
    /// Drawable markup elements across the document. See
    /// [`MAX_XPS_ELEMENTS`].
    pub max_elements: usize,
    /// Path segments across the document. See [`MAX_XPS_SEGMENTS`].
    pub max_segments: usize,
    /// Bytes of the document handed to the parser. Shared with the comic path
    /// rather than duplicated — see [`MAX_SYNTHESISED_PDF`], whose argument is
    /// about the writer and not about the container.
    pub max_synthesised: usize,
    /// What the markup reader is allowed to spend, per part.
    pub xml: XmlLimits,
    /// The archive reader's own name cap.
    ///
    /// The archive arrives already open — that is the whole point of routing
    /// once — so this is the one number from `tinker_pdf_zip::Limits` the
    /// package layer still needs, and it needs it for exactly one thing: a
    /// `Warning::NameTruncated` is recorded once per archive, and attributing
    /// it to an entry takes the cap it was measured against. See
    /// [`opc::Package::open`].
    pub zip_name_len: usize,
}

impl Limits {
    /// The constants above, which is what [`Default`] hands back.
    pub const DEFAULT: Self = Self {
        max_parts: MAX_XPS_PARTS,
        max_pages: MAX_XPS_PAGES,
        max_elements: MAX_XPS_ELEMENTS,
        max_segments: MAX_XPS_SEGMENTS,
        max_synthesised: MAX_SYNTHESISED_PDF,
        xml: XmlLimits::DEFAULT,
        zip_name_len: zip_limits::MAX_ZIP_NAME_LEN,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// ---- Page-level degradation -------------------------------------------------

/// Why a synthesised fixed page is not the picture the package holds.
///
/// **A page that draws carries none of these**, which is milestone 6's whole
/// change: milestones 3 to 5 gave every page `NotDrawn` because the spine was
/// read and the markup was not painted, and that variant is **deleted** here
/// rather than kept as a shape nothing produces. A record of old behaviour
/// that does not break when the behaviour changes is not a record — which is
/// the argument milestone 3 used to delete
/// `today_an_xps_opens_as_a_comic_and_this_is_what_it_reports`, one level up.
///
/// What a page can still be silent about is nothing: an element that could not
/// be drawn reports through [`XpsElementDefect`] instead, so a page whose
/// markup read and whose `Glyphs` did not is honest about exactly that much.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum XpsPageDefect {
    /// A `PageContent` whose `Source` did not resolve to a part in the package.
    ///
    /// The page is kept anyway and **keeps its number**: a `FixedDocument` that
    /// references ten pages and resolves nine would otherwise produce a
    /// nine-page document with no gap anywhere in it.
    SourceUnresolved,
    /// A `DocumentReference` whose `Source` did not resolve, or whose document
    /// part would not read. One placeholder page stands for it, for the same
    /// reason: a document that vanished silently takes an unknown number of
    /// pages with it.
    DocumentUnresolved,
    /// The fixed page part will not read, or its root element is not a
    /// `FixedPage` in either dialect's namespace.
    Unreadable,
    /// The root element read and states a size, and the markup **under** it
    /// does not read (gap 30, milestone 6).
    ///
    /// Distinct from [`XpsPageDefect::Unreadable`] because the page keeps the
    /// size the file stated and its neighbours' shape, where a part that is
    /// not a `FixedPage` at all has nothing to state one with. The page is the
    /// neutral placeholder.
    ContentUnreadable,
    /// `Width` or `Height` is absent, unparseable or outside what a page can
    /// be, so the page took the book's own size instead of a size the file
    /// stated.
    SizeUnusable,
    /// The reference named a part the package holds and **its media type is
    /// not the one Table D-4 fixes for it** (gap 30, milestone 4).
    ///
    /// A `PageContent` naming an image part, or a `DocumentReference` naming a
    /// fixed page, is a broken document rather than a page that happens not to
    /// draw — and it is not read as a fixed page on the strength of what its
    /// name ends in, which is the whole reason 7.2.3.5 is an ordered algorithm
    /// over the content-types item rather than a table of extensions. The page
    /// is kept and keeps its number, for [`XpsPageDefect::SourceUnresolved`]'s
    /// reason.
    MediaTypeMismatch,
    /// A `ContentBox` or `BleedBox` was stated and would not read as four
    /// usable numbers, so the page carries no `/CropBox` or `/BleedBox` (gap
    /// 30, milestone 4).
    ///
    /// Named rather than defaulted, because 7.7.3.3 makes an absent `/CropBox`
    /// mean "the whole media box" — which is a plausible answer, and a
    /// plausible answer silently substituted for a statement the file made is
    /// this gap's own failure mode. The page is otherwise unaffected.
    PageBoxUnusable,
}

impl core::fmt::Display for XpsPageDefect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            XpsPageDefect::ContentUnreadable => "the fixed page's markup will not read",
            XpsPageDefect::SourceUnresolved => "a `PageContent` naming no part",
            XpsPageDefect::DocumentUnresolved => "a `DocumentReference` naming no readable part",
            XpsPageDefect::Unreadable => "the fixed page part is not a readable `FixedPage`",
            XpsPageDefect::SizeUnusable => "the fixed page states no usable size",
            XpsPageDefect::MediaTypeMismatch => {
                "a reference naming a part whose media type is not the payload's"
            }
            XpsPageDefect::PageBoxUnusable => "a `ContentBox` or `BleedBox` that will not read",
        })
    }
}

/// Why one element of a fixed page is not the picture the markup describes
/// (gap 30, milestone 6).
///
/// **Three answers, and which one an element gets is decided by gap 30's
/// design section rather than by whoever wrote the code.** The asymmetry is
/// the point:
///
/// - *geometry unreadable* — the element is **not painted**, because a missing
///   shape is visible as a missing shape;
/// - *paint unreadable* — the element **is painted, in the neutral placeholder
///   grey**, because the shape and its position are known and only the colour
///   is not, and gap 07's headline defect was a gradient-stroked rule painting
///   solid black silently;
/// - *transform, clip or opacity unreadable* — the element **and everything
///   under it is refused**, because the identity matrix, the absent clip and
///   full opacity each draw the right content in the wrong place at the right
///   size, which reads as a layout bug in the producer.
///
/// The names are separate for the reason gap 29's milestone 2 recorded about
/// refusals: "this archive promised 5 000 bytes and produced 4 000" and
/// "checksum wrong" both refuse, and a report that could not tell them apart
/// was no use to anybody. An `ImageBrush` this build does not draw yet, a
/// `{StaticResource}` that names nothing and a colour string that is not one
/// are three different things about a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum XpsElementDefect {
    /// A `Data` that is absent or will not parse as 11.2's geometry. **Not
    /// painted.**
    GeometryUnreadable,
    /// A `RenderTransform` or `Transform` that is not 14.4.1's six numbers.
    /// **The element and its descendants are refused.**
    TransformUnreadable,
    /// A `Clip` that will not parse. **Refused** — the same parser as
    /// `GeometryUnreadable` and the opposite consequence, because a shape that
    /// failed to be a clip draws everything the clip was there to hide.
    ClipUnreadable,
    /// An `Opacity` that is not a number in `[0, 1]`. **Refused.**
    OpacityUnreadable,
    /// An `OpacityMask`, which this build does not apply (14.3). **Refused**,
    /// for `OpacityUnreadable`'s reason: a mask ignored draws a whole shape
    /// where a sliver was meant.
    OpacityMaskUnsupported,
    /// A `{StaticResource}` naming a key no dictionary in scope holds.
    /// **Painted grey.**
    BrushUnresolved,
    /// A `{StaticResource}` chain that returns to a key it already passed
    /// through. **Painted grey**, and refused rather than recursed.
    BrushCyclic,
    /// A `{StaticResource}` chain longer than [`MAX_XPS_RESOURCE_DEPTH`].
    /// **Painted grey.** Not the same as a cycle and never reported as one.
    BrushTooDeep,
    /// A brush this milestone does not paint: an `ImageBrush` or a
    /// `VisualBrush` (gap 30, milestone 8), a `ContextColor` naming an ICC
    /// profile, or a gradient asked to stroke. **Painted grey.**
    BrushUnsupported,
    /// A colour or a gradient that is not 15's syntax. **Painted grey.**
    BrushUnreadable,
    /// The brush reached the page and not exactly: gradient stops whose alphas
    /// differ from each other, which one constant alpha cannot express, or a
    /// `ColorInterpolationMode` this build does not interpolate in.
    BrushApproximated,
    /// A `ResourceDictionary` with a `Source`, naming another part (14.2.4).
    /// Gap 30's milestone 8; every key in it is unresolvable until then, and
    /// saying so once beats one `BrushUnresolved` per use.
    ResourceDictionaryRemote,
    /// A `Glyphs` run, which is gap 30's milestone 7. **Not painted**, and
    /// named — a page whose words are missing and whose report says nothing is
    /// gap 17's blank page one element down.
    GlyphsNotDrawn,
    /// Markup this build does not draw: an element from another vocabulary, a
    /// property element nothing here reads, a dictionary entry with no
    /// `x:Key`.
    ElementUnknown,
}

impl core::fmt::Display for XpsElementDefect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            XpsElementDefect::GeometryUnreadable => "a `Data` that is not 11.2's geometry",
            XpsElementDefect::TransformUnreadable => "a transform that is not six numbers",
            XpsElementDefect::ClipUnreadable => "a `Clip` that is not 11.2's geometry",
            XpsElementDefect::OpacityUnreadable => "an `Opacity` that is not a number in [0, 1]",
            XpsElementDefect::OpacityMaskUnsupported => "an `OpacityMask` this build cannot apply",
            XpsElementDefect::BrushUnresolved => "a `{StaticResource}` naming no resource",
            XpsElementDefect::BrushCyclic => "a `{StaticResource}` chain that returns to itself",
            XpsElementDefect::BrushTooDeep => "a `{StaticResource}` chain past the depth cap",
            XpsElementDefect::BrushUnsupported => "a brush this build does not paint",
            XpsElementDefect::BrushUnreadable => "a colour or gradient that is not 15's syntax",
            XpsElementDefect::BrushApproximated => "a brush that reached the page approximately",
            XpsElementDefect::ResourceDictionaryRemote => "a `ResourceDictionary` in another part",
            XpsElementDefect::GlyphsNotDrawn => "a `Glyphs` run, which is not drawn yet",
            XpsElementDefect::ElementUnknown => "markup this build does not draw",
        })
    }
}

// ---- Routing ----------------------------------------------------------------

/// What one already-open archive turned out to be.
///
/// The archive travels **inside** this enum on the way out rather than being
/// opened twice, which is milestone 3's own exit criterion: opening it to sniff
/// and again to read doubles the work and creates a window in which the two
/// reads could disagree.
pub enum Routing<'a> {
    /// E.3's three steps held and the package was read: the synthesised PDF and
    /// what it cost.
    Document(Vec<u8>, ArchiveReport),
    /// E.3's three steps held and the package could not be read. Refused **by
    /// name**, which is the feature rather than the absence of one.
    Refused(ArchiveRefusal),
    /// Not an XPS. The archive is handed back **unread** for the comic path.
    NotXps(Archive<'a>),
}

/// Decides what an open archive is, and reads it if it is an XPS.
///
/// The cheap half runs first and costs no read at all: a ZIP with no
/// `[Content_Types].xml` item and no `_rels/.rels` item is not an OPC package,
/// which is every comic archive there has ever been, and it comes straight back
/// as [`Routing::NotXps`] having spent nothing.
#[must_use]
pub fn route<'a>(archive: Archive<'a>, limits: &Limits) -> Routing<'a> {
    let mut package = Package::open(archive, limits.zip_name_len, limits.xml);
    match recognise(&mut package) {
        Recognition::NotXps => Routing::NotXps(package.into_archive()),
        Recognition::Broken(why) => Routing::Refused(why),
        Recognition::Xps { sequence, dialect } => {
            match synthesise(&mut package, &sequence, dialect, limits) {
                Ok((pdf, report)) => Routing::Document(pdf, report),
                Err(why) => Routing::Refused(why),
            }
        }
    }
}

/// What E.3's three steps decided.
enum Recognition {
    /// An XPS, with its fixed document sequence part and the dialect the
    /// relationship type was spelled in.
    Xps {
        sequence: PartName,
        dialect: Dialect,
    },
    /// Not an XPS: the comic path takes it, unchanged.
    NotXps,
    /// An XPS whose own structure will not read.
    Broken(ArchiveRefusal),
}

/// ECMA-388 E.3, step by step.
fn recognise(package: &mut Package<'_>) -> Recognition {
    // Step 1 is the caller's: the bytes began with a local file header and
    // opened as a ZIP.
    //
    // Step 2. Both items, by the names OPC fixes for them, and **no read**: a
    // comic archive stops here having cost nothing.
    if package.item_index(opc::CONTENT_TYPES_ITEM).is_none() {
        return Recognition::NotXps;
    }
    if package
        .item_index(opc::PACKAGE_RELATIONSHIPS_ITEM)
        .is_none()
    {
        return Recognition::NotXps;
    }

    // Step 3. From here the archive carries OPC's own two parts, so a failure
    // is a broken package rather than a comic — see this module's header for
    // why that refinement of E.3 is taken.
    let relationships = match package.relationships(None) {
        Ok(relationships) => relationships,
        Err(_) => return Recognition::Broken(ArchiveRefusal::UnreadablePackage),
    };
    let Some((relationship, dialect)) = relationships.iter().find_map(|relationship| {
        fixed_representation_dialect(&relationship.kind).map(|dialect| (relationship, dialect))
    }) else {
        // OPC, and no fixed representation: a `.docx` or an `.odf`. Not an XPS,
        // and the comic path is unchanged for it.
        return Recognition::NotXps;
    };
    // `TargetMode="External"` names something outside the package, and this
    // engine performs no I/O of any kind. It is an XPS that claims its own
    // payload is somewhere else, which is a broken package rather than a comic.
    if relationship.external {
        return Recognition::Broken(ArchiveRefusal::UnreadablePackage);
    }
    let target = relationship.target.clone();

    // 6.5.2: a relative target in the *package* relationships part resolves
    // against the package root. Milestone 1 measured both spellings in one
    // corpus — WPF writes `/FixedDocumentSequence.fdseq` and the XPS object
    // model writes `FixedDocumentSequence.fdseq` — so a reader that handled
    // only one refuses one of the two producers.
    let Some(sequence) = opc::resolve_reference(opc::PACKAGE_ROOT, &target)
        .as_deref()
        .and_then(PartName::from_absolute)
    else {
        return Recognition::Broken(ArchiveRefusal::UnreadablePackage);
    };
    if !package.has(&sequence) {
        return Recognition::Broken(ArchiveRefusal::UnreadablePackage);
    }
    match package.media_type(&sequence) {
        Ok(Some(media)) if media == FIXED_DOCUMENT_SEQUENCE_MEDIA_TYPE => {
            Recognition::Xps { sequence, dialect }
        }
        _ => Recognition::Broken(ArchiveRefusal::UnreadablePackage),
    }
}

// ---- Synthesis --------------------------------------------------------------

/// One page, decided but not yet written.
struct Plan {
    /// The part name, or the reference that did not resolve to one, for the
    /// report.
    name: String,
    /// The fixed page part to paint, when the reference resolved to one whose
    /// media type says it is a page. `None` is a placeholder, and a
    /// placeholder has no markup to draw.
    part: Option<PartName>,
    size: Option<(f64, f64)>,
    /// `/CropBox` and `/BleedBox`, in PDF points, from 10.3's two attributes.
    boxes: PageBoxes,
    /// Why this page is a placeholder, or `None` for a page that draws its own
    /// markup — which is what milestone 6 made possible and what deleting
    /// `NotDrawn` records.
    defect: Option<XpsPageDefect>,
    /// A second warning this page owes beside `defect`. A page has one reason
    /// for being a placeholder and may separately have said something about its
    /// own boxes that would not read, and collapsing the two would lose
    /// whichever came second.
    extra: Option<XpsPageDefect>,
}

/// The rectangles ECMA-388 10.3 lets a fixed page state, in **PDF points**.
///
/// 10.3.1's `ContentBox` becomes `/CropBox` and 10.3.2's `BleedBox` becomes
/// `/BleedBox`. Neither is written when the page did not state it: 7.7.3.3
/// makes an absent `/CropBox` mean the whole media box, so writing one anyway
/// would turn a page that said nothing into a page that said everything.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct PageBoxes {
    crop: Option<[f64; 4]>,
    bleed: Option<[f64; 4]>,
}

/// What a fixed page's root element said about its own geometry.
#[derive(Clone, Copy)]
struct PageGeometry {
    /// `None` when `Width` or `Height` is absent or unusable.
    size: Option<(f64, f64)>,
    boxes: PageBoxes,
    /// A box attribute was there and would not read.
    box_unusable: bool,
}

/// The payload markup this synthesis has already read, parsed **once per
/// part** (gap 30, milestone 4).
///
/// # The multiplier this removes, which is ruling 1's own sentence
///
/// Nothing makes a reference and a part the same thing. A `FixedDocument` may
/// name one `PageContent` `Source` four thousand times, and a
/// `FixedDocumentSequence` may name one `FixedDocument` a million times — 12.3
/// forbids neither, and milestone 3's own `MAX_XPS_PAGES` note is about exactly
/// that asymmetry: *"four thousand `PageContent` elements may name **one** part
/// between them"*.
///
/// Reading is cheap — [`opc::Package`] caches the bytes — and **parsing was
/// not**. `tinker_pdf_xml::Source::new` decodes the part and then walks every
/// character of it against XML 1.0's `Char` production before a single event is
/// produced, so it is O(part) whatever the caller stops at. Milestone 3's
/// reader called it once per *reference*, which multiplies a part this build
/// already admits (128 MiB, [`zip_limits::MAX_ZIP_ENTRY_BYTES`]) by a count the
/// file chooses: 4 096 pages naming one part is 512 GiB of scanning from a
/// package that deflates to a few hundred kilobytes, and a sequence naming a
/// million documents that hold no page at all spends no page budget while it
/// does it.
///
/// That is `5adf502`'s finding in a third format — *"depth is not work once the
/// recursion branches"* — and the plan's own version of it: **a per-item cap is
/// not a total once the file chooses how many items there are.**
///
/// # Why the answer is a cache and not a cap
///
/// A work cap here would have to refuse something a conforming document may do:
/// a fixed document that shows one page part four thousand times is legal XPS
/// and costs one part. Cached, the total markup this synthesis parses is the
/// sum of the **distinct** parts it read, which the archive reader has already
/// bounded from both sides — a deflated part against
/// [`zip_limits::MAX_ZIP_INFLATED`] and a stored one against the length of the
/// file itself. So a constant over it could never be the thing that stopped
/// anything, which is gap 18a milestone 8's failure, and gap 30 gains no new
/// row in `bounds_ledger.rs` for it. What the sequence *does* gain is a count:
/// see [`synthesise`], where a sequence naming more documents than the page cap
/// allows is refused before any of them is resolved.
#[derive(Default)]
struct Payload {
    /// `(part, root element)` to that part's children's `Source` attributes.
    children: HashMap<(PartName, &'static str), Option<Vec<Option<String>>>>,
    /// One fixed page part's geometry.
    pages: HashMap<PartName, Option<PageGeometry>>,
    /// How many parts' markup has been parsed.
    ///
    /// Published through [`ArchiveReport::parsed_parts`] for the reason
    /// `synthesised_bytes` is: a cache with no observable is a cache a test
    /// cannot tell from its own absence, and `Archive::inflated` cannot stand
    /// in for it because the bytes were already cached a layer down.
    parses: usize,
}

impl Payload {
    /// Every `child` element's `Source` in `part`, in markup order, read once.
    fn children(
        &mut self,
        package: &mut Package<'_>,
        part: &PartName,
        root: &'static str,
        child: &'static str,
        limits: &Limits,
    ) -> Option<Vec<Option<String>>> {
        let key = (part.clone(), root);
        if let Some(found) = self.children.get(&key) {
            return found.clone();
        }
        let parsed = match package.read_part(part) {
            Ok(bytes) => child_sources(bytes, root, child, limits),
            Err(_) => None,
        };
        self.parses = self.parses.saturating_add(1);
        self.children.insert(key, parsed.clone());
        parsed
    }

    /// One fixed page part's geometry, read once.
    fn geometry(
        &mut self,
        package: &mut Package<'_>,
        part: &PartName,
        limits: &Limits,
    ) -> Option<PageGeometry> {
        if let Some(found) = self.pages.get(part) {
            return *found;
        }
        let parsed = match package.read_part(part) {
            Ok(bytes) => fixed_page_geometry(bytes, limits),
            Err(_) => None,
        };
        self.parses = self.parses.saturating_add(1);
        self.pages.insert(part.clone(), parsed);
        parsed
    }
}

/// Reads the fixed payload and builds a PDF whose pages are its fixed pages.
///
/// # Errors
/// [`ArchiveRefusal`], one variant per refusal, each of them by name.
fn synthesise(
    package: &mut Package<'_>,
    sequence: &PartName,
    dialect: Dialect,
    limits: &Limits,
) -> Result<(Vec<u8>, ArchiveReport), ArchiveRefusal> {
    // The strict half of OPC, and **only now**: every rule below would refuse a
    // comic archive that is doing nothing wrong, so none of them may run before
    // the format is decided.
    package
        .validate(limits.max_parts)
        .map_err(|problem| match problem {
            PackageProblem::Interleaved => ArchiveRefusal::Interleaved,
            PackageProblem::InvalidPartName => ArchiveRefusal::InvalidPartName,
            PackageProblem::AmbiguousPartNames => ArchiveRefusal::AmbiguousPartNames,
            PackageProblem::TooManyParts => ArchiveRefusal::TooLarge,
        })?;

    let mut payload = Payload::default();
    let documents = payload.children(
        package,
        sequence,
        "FixedDocumentSequence",
        "DocumentReference",
        limits,
    );
    // "A `FixedDocumentSequence` naming no documents at all" is a package-level
    // refusal in gap 30's design, and so is one that will not read: there is no
    // page to degrade *to*.
    let Some(documents) = documents.filter(|sources| !sources.is_empty()) else {
        return Err(ArchiveRefusal::NoFixedPages);
    };
    // A reference is not a page, so the page cap does not bound this loop on its
    // own: a `DocumentReference` naming a document that holds no `PageContent`
    // pushes no plan and charges nothing, and `MAX_XML_TOKENS` lets one sequence
    // hold a million of them. Each costs a name resolution and a lookup over
    // every item in the package, so the count is bounded here rather than left
    // to whatever the pages happen to cost — and it is bounded at the page cap
    // because a sequence naming more documents than this build will synthesise
    // pages cannot produce a document it would finish.
    if documents.len() > limits.max_pages {
        return Err(ArchiveRefusal::TooLarge);
    }

    let mut plans: Vec<Plan> = Vec::new();
    for source in documents {
        let document = source
            .as_deref()
            .and_then(|reference| sequence.resolve(reference));
        // Table D-4 fixes the media type of a `FixedDocument` part, and
        // 7.2.3.5 is how a part's media type is found — never its extension,
        // which nothing in this reader consults for the payload at all. A
        // reference to something that is not a fixed document is a document
        // that did not resolve, said more precisely.
        let typed = match &document {
            Some(name) => {
                package.has(name)
                    && matches!(package.media_type(name), Ok(Some(m)) if m == FIXED_DOCUMENT_MEDIA_TYPE)
            }
            None => false,
        };
        let pages = match (&document, typed) {
            (Some(name), true) => {
                payload.children(package, name, "FixedDocument", "PageContent", limits)
            }
            _ => None,
        };
        let Some(pages) = pages else {
            // A document that vanished takes an unknown number of pages with
            // it, so it leaves one visible page behind rather than nothing.
            let named = document.as_ref().is_some_and(|name| package.has(name));
            push_plan(
                &mut plans,
                Plan {
                    name: reference_name(&document, source.as_deref()),
                    part: None,
                    size: None,
                    boxes: PageBoxes::default(),
                    defect: Some(if named && !typed {
                        XpsPageDefect::MediaTypeMismatch
                    } else {
                        XpsPageDefect::DocumentUnresolved
                    }),
                    extra: None,
                },
                limits,
            )?;
            continue;
        };
        let Some(base) = document else {
            continue;
        };
        for page in pages {
            push_plan(
                &mut plans,
                plan_page(package, &mut payload, &base, page.as_deref(), limits),
                limits,
            )?;
        }
    }

    if plans.is_empty() {
        return Err(ArchiveRefusal::NoFixedPages);
    }

    // Charged before anything is built, on what the pages have undertaken to
    // contribute — `tinker-pdf-zip`'s `Budget` posture, where a permit is what
    // has been promised rather than what happened to arrive.
    DOCUMENT_OVERHEAD
        .checked_add(plans.len().saturating_mul(PAGE_OVERHEAD))
        .filter(|total| *total <= limits.max_synthesised)
        .ok_or(ArchiveRefusal::TooLarge)?;

    // A placeholder has no size of its own. It takes the first size the book
    // stated, because a document's pages are usually one size and a page that
    // matches its neighbours is one a reader can page through; paper is the
    // answer only for a book that never stated one.
    let fallback = plans
        .iter()
        .find_map(|plan| plan.size)
        .unwrap_or(FALLBACK_PAGE);

    let mut builder = DocumentBuilder::new();
    let mut warnings: Vec<ArchiveWarning> = Vec::new();
    let mut pages: Vec<PageOrigin> = Vec::with_capacity(plans.len());

    // The two work totals are the document's, spent and never refunded, and
    // one painter serves the whole document because a resource name has to be
    // unique across it: `add_page` copies the document's whole resource table
    // into every page, so two pages naming one `/GS0` would be one state.
    let mut budget = Budget::new(limits.max_elements, limits.max_segments);
    let mut painter = Painter::default();
    // Painted **once per part**, for the reason milestone 4 built its own
    // caches: a `FixedDocument` may show one page part four thousand times,
    // and `Source::new` walks every character of a part before it yields an
    // event. The bytes were already cached a layer down; this is the parse and
    // the drawing.
    let mut painted: HashMap<PartName, Option<Drawn>> = HashMap::new();

    for (number, plan) in plans.iter().enumerate() {
        let (width, height) = plan.size.unwrap_or(fallback);
        let boxes = plan.boxes;
        let mut defect = plan.defect;
        let mut content: Option<Vec<u8>> = None;
        let mut elements: Vec<XpsElementDefect> = Vec::new();

        if let Some(part) = &plan.part {
            if !painted.contains_key(part) {
                let drawn = match package.read_part(part) {
                    Ok(bytes) => painter.page(&mut builder, bytes, &limits.xml, &mut budget),
                    Err(_) => Err(Trouble::Markup),
                };
                match drawn {
                    Ok(drawn) => {
                        painted.insert(part.clone(), Some(drawn));
                    }
                    // A work total spent refuses the **package**. Truncating
                    // instead would produce a document with the right page
                    // count and the wrong pages, which is this gap's own
                    // failure mode wearing a smaller hat.
                    Err(Trouble::Exhausted) => return Err(ArchiveRefusal::TooLarge),
                    Err(Trouble::Markup) => {
                        painted.insert(part.clone(), None);
                    }
                }
            }
            match painted.get(part) {
                Some(Some(drawn)) => {
                    content = Some(drawn.content.clone());
                    elements = drawn.defects.clone();
                }
                _ => defect = Some(XpsPageDefect::ContentUnreadable),
            }
        }

        builder.add_page(width, height, |page| {
            match &content {
                // 18.1: one XPS unit is 1/96 inch against PDF's 1/72 and the
                // origin is the **top left** with y increasing downward. One
                // `cm` says both, so the numbers in the content stream are the
                // numbers in the markup — the property gap 30's geometry
                // section asks not to be thrown away by pre-multiplying, and
                // the one that makes a diff between the two readable.
                Some(drawn) => {
                    page.raw(
                        format!("q {UNITS_TO_POINTS} 0 0 -{UNITS_TO_POINTS} 0 {height} cm")
                            .as_bytes(),
                    );
                    page.raw(drawn);
                    page.raw(b"Q");
                }
                None => page.fill_rect(0.0, 0.0, width, height, PLACEHOLDER_GREY),
            }
            // 10.3.1 and 10.3.2 are the only statement a fixed page makes about
            // where its content is, and a synthesised document that dropped
            // them would lose it for good.
            if let Some([x0, y0, x1, y1]) = boxes.crop {
                page.set_crop_box(x0, y0, x1, y1);
            }
            if let Some([x0, y0, x1, y1]) = boxes.bleed {
                page.set_bleed_box(x0, y0, x1, y1);
            }
        });
        let number = u32::try_from(number).unwrap_or(u32::MAX);
        if let Some(defect) = defect {
            warnings.push(ArchiveWarning::XpsPage {
                page: number,
                defect,
            });
        }
        if let Some(defect) = plan.extra {
            warnings.push(ArchiveWarning::XpsPage {
                page: number,
                defect,
            });
        }
        for defect in elements {
            warnings.push(ArchiveWarning::XpsElement {
                page: number,
                defect,
            });
        }
        pages.push(PageOrigin {
            name: plan.name.clone(),
            defect: None,
        });
    }

    // Taken after every read, because reading is what most of them come from —
    // and asserted in both directions by
    // `a_truncated_part_name_is_unresolvable_and_an_untruncated_one_is_not`,
    // because gap 29's milestone-6 survivor was a warnings loop nothing checked
    // was there.
    for warning in package.archive().warnings() {
        warnings.push(ArchiveWarning::Zip(*warning));
    }

    let pdf = builder.finish();
    if pdf.len() > limits.max_synthesised {
        return Err(ArchiveRefusal::TooLarge);
    }
    let synthesised_bytes = pdf.len();
    Ok((
        pdf,
        ArchiveReport::synthesised(
            warnings,
            pages,
            synthesised_bytes,
            Some(dialect),
            payload.parses,
        ),
    ))
}

/// Adds one page's plan, or refuses because the page cap is spent.
fn push_plan(plans: &mut Vec<Plan>, plan: Plan, limits: &Limits) -> Result<(), ArchiveRefusal> {
    if plans.len() >= limits.max_pages {
        return Err(ArchiveRefusal::TooLarge);
    }
    plans.push(plan);
    Ok(())
}

/// What one `PageContent` becomes.
fn plan_page(
    package: &mut Package<'_>,
    payload: &mut Payload,
    base: &PartName,
    source: Option<&str>,
    limits: &Limits,
) -> Plan {
    let placeholder = |name: String, defect: XpsPageDefect| Plan {
        name,
        part: None,
        size: None,
        boxes: PageBoxes::default(),
        defect: Some(defect),
        extra: None,
    };

    let Some(page) = source.and_then(|reference| base.resolve(reference)) else {
        return placeholder(
            source.unwrap_or("").to_string(),
            XpsPageDefect::SourceUnresolved,
        );
    };
    let name = page.as_str().to_string();
    if !package.has(&page) {
        return placeholder(name, XpsPageDefect::SourceUnresolved);
    }
    // 7.2.3.5 over Table D-4's fixed page type. A `PageContent` naming a part
    // the package holds and whose media type is something else — an image, a
    // print ticket, a fixed *document* — is not read as a page on the strength
    // of what its name ends in. The part is not even read: a reference to the
    // wrong thing is answered before its bytes are.
    if !matches!(package.media_type(&page), Ok(Some(m)) if m == FIXED_PAGE_MEDIA_TYPE) {
        return placeholder(name, XpsPageDefect::MediaTypeMismatch);
    }
    let Some(geometry) = payload.geometry(package, &page, limits) else {
        return placeholder(name, XpsPageDefect::Unreadable);
    };
    let box_unusable = geometry
        .box_unusable
        .then_some(XpsPageDefect::PageBoxUnusable);
    match geometry.size {
        Some(size) => Plan {
            name,
            part: Some(page),
            size: Some(size),
            boxes: geometry.boxes,
            // **No defect.** Milestone 5 and everything before it gave every
            // page `NotDrawn`; this is the milestone in which a page that
            // reads is a page that draws, and a warning about it would be a
            // sentence that is no longer true.
            defect: None,
            extra: box_unusable,
        },
        // A `FixedPage` whose size is missing or unusable is still a page, at
        // the book's own size — and it carries no boxes, because 10.3's two
        // rectangles are stated in the coordinate space of a page this one did
        // not state. `SizeUnusable` already says that; a second warning about
        // the consequence would be the same sentence twice.
        None => placeholder(name, XpsPageDefect::SizeUnusable),
    }
}

/// The part name a plan reports, or the reference that did not become one.
fn reference_name(resolved: &Option<PartName>, source: Option<&str>) -> String {
    match resolved {
        Some(name) => name.as_str().to_string(),
        None => source.unwrap_or("").to_string(),
    }
}

/// Every `child` element's `Source`, in **markup order**, from a part whose
/// root is `root` in either dialect.
///
/// `None` refuses the part: the markup is not well formed, or its root is not
/// the element this part must hold. `Some` with a `None` inside is a child that
/// carried no `Source` at all, which is a page that cannot resolve rather than
/// a document that cannot be read.
fn child_sources(
    bytes: &[u8],
    root: &str,
    child: &str,
    limits: &Limits,
) -> Option<Vec<Option<String>>> {
    let source = Source::new(bytes).ok()?;
    let mut out: Vec<Option<String>> = Vec::new();
    let mut depth = 0usize;
    let mut seen_root = false;
    for event in source.reader(&limits.xml) {
        match event.ok()? {
            Event::Start(element) => {
                depth += 1;
                if depth == 1 {
                    if element.local() != root || dialect_of(element.namespace()).is_none() {
                        return None;
                    }
                    seen_root = true;
                } else if depth == 2
                    && element.local() == child
                    && dialect_of(element.namespace()).is_some()
                {
                    out.push(element.attribute(None, "Source").map(str::to_string));
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    seen_root.then_some(out)
}

/// A fixed page's geometry in **PDF points**, from its root element alone.
///
/// `None` refuses the part. The read stops at the root element, so a page of
/// forty thousand path segments costs the same here as an empty one.
fn fixed_page_geometry(bytes: &[u8], limits: &Limits) -> Option<PageGeometry> {
    let source = Source::new(bytes).ok()?;
    for event in source.reader(&limits.xml) {
        let Event::Start(element) = event.ok()? else {
            continue;
        };
        if element.local() != "FixedPage" || dialect_of(element.namespace()).is_none() {
            return None;
        }
        let width = element.attribute(None, "Width").and_then(page_units);
        let height = element.attribute(None, "Height").and_then(page_units);
        let size = match (width, height) {
            (Some(width), Some(height)) => {
                Some((width * UNITS_TO_POINTS, height * UNITS_TO_POINTS))
            }
            _ => None,
        };

        let mut boxes = PageBoxes::default();
        let mut box_unusable = false;
        if let Some(page) = size {
            // 10.3.1 to `/CropBox`, 10.3.2 to `/BleedBox`. Read only when the
            // page stated a size, because both are rectangles *in that page's*
            // coordinate space and flipping one against a size the file never
            // gave would be arithmetic over a number this reader invented.
            for (attribute, slot) in [
                ("ContentBox", &mut boxes.crop),
                ("BleedBox", &mut boxes.bleed),
            ] {
                let Some(text) = element.attribute(None, attribute) else {
                    continue;
                };
                match page_box(text, page) {
                    Some(rect) => *slot = Some(rect),
                    None => box_unusable = true,
                }
            }
        }
        return Some(PageGeometry {
            size,
            boxes,
            box_unusable,
        });
    }
    None
}

/// One coordinate, in XPS units, checked against what a page can be.
fn page_units(text: &str) -> Option<f64> {
    let value = text.trim().parse::<f64>().ok()?;
    (value.is_finite() && value > 0.0 && value <= MAX_PAGE_UNITS).then_some(value)
}

/// ECMA-388 10.3's `x,y,width,height`, as a PDF rectangle in points.
///
/// Three conversions in one place, and each is a defect somebody has shipped:
///
/// - **the unit**, 1/96 inch to 1/72, applied exactly once — the same
///   [`UNITS_TO_POINTS`] the page size uses, so a box and its page cannot
///   disagree about the scale;
/// - **the origin**, top-left with y down to bottom-left with y up, which is a
///   subtraction from the page height rather than a negation — a box that was
///   scaled and not flipped lands in the mirror-image half of the page and is
///   the right size, which is exactly the shape that reads as the producer's
///   fault;
/// - **the form**, `x,y,width,height` rather than PDF's `x0 y0 x1 y1`, which
///   are the same four numbers in a package whose box starts at the origin and
///   nowhere else.
///
/// The result is intersected with the page, because 14.11.2 says a consumer
/// reduces these boxes to their intersection with the media box anyway — so
/// writing the intersection is writing what any reader would compute, and a
/// bleed box that lies wholly outside the page is a statement with no PDF
/// spelling rather than one worth half-writing.
fn page_box(text: &str, page: (f64, f64)) -> Option<[f64; 4]> {
    let mut numbers = [0f64; 4];
    let mut seen = 0usize;
    // Commas with optional whitespace are 10.3's own form, and whitespace alone
    // is what the object model writes elsewhere in the same corpus. Accepting
    // both is parsing rather than guessing: neither spelling is ambiguous.
    for field in text.split([',', ' ', '\t', '\r', '\n']) {
        if field.is_empty() {
            continue;
        }
        let value = field.parse::<f64>().ok()?;
        if !value.is_finite() || value.abs() > MAX_PAGE_UNITS {
            return None;
        }
        *numbers.get_mut(seen)? = value;
        seen += 1;
    }
    if seen != 4 {
        return None;
    }
    let [x, y, width, height] = numbers;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let (page_width, page_height) = page;
    let x0 = (x * UNITS_TO_POINTS).max(0.0);
    let x1 = ((x + width) * UNITS_TO_POINTS).min(page_width);
    let y0 = (page_height - (y + height) * UNITS_TO_POINTS).max(0.0);
    let y1 = (page_height - y * UNITS_TO_POINTS).min(page_height);
    (x1 > x0 && y1 > y0).then_some([x0, y0, x1, y1])
}
