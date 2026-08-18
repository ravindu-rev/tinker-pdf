//! An XPS package opens as the fixed document it is (gap 30, milestone 3).
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
//! # What a page is in this milestone, and what it is not
//!
//! The spine is read — `FixedDocumentSequence` to `FixedDocument` to
//! `FixedPage` — and each page is synthesised **at the size its own markup
//! states**, carrying the neutral placeholder and a named warning. ECMA-388
//! 18.1 puts one XPS unit at 1/96 inch against PDF's 1/72, so `Width="816"
//! Height="1056"` is 612 x 792 pt, which is US Letter to the point.
//!
//! Nothing on the page is drawn yet. That is ruling 2's degradation and not a
//! blank page reported as success: every page carries
//! [`XpsPageDefect::NotDrawn`], so a host that asks what it got is told. Paths,
//! brushes, glyphs and images are gap 30's milestones 6 to 8, and the writer
//! work they need is milestone 5.

pub mod opc;

use tinker_pdf_cos::DocumentBuilder;
use tinker_pdf_xml::{Event, Limits as XmlLimits, Source};
use tinker_pdf_zip::{limits as zip_limits, Archive};

use crate::cbz::{
    ArchiveRefusal, ArchiveReport, ArchiveWarning, PageOrigin, DOCUMENT_OVERHEAD,
    MAX_SYNTHESISED_PDF, PAGE_OVERHEAD, PLACEHOLDER_GREY,
};
use opc::{Package, PackageProblem, PartName};

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
/// Every page of every package carries one of these in this milestone, and
/// [`XpsPageDefect::NotDrawn`] is the honest reason: the spine is read and the
/// markup is not painted yet. A page that came back silent would be gap 17's
/// blank page reported as success, which is the failure this whole gap is
/// about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum XpsPageDefect {
    /// The page is at the size its own markup states and **its content is not
    /// drawn**. Gap 30's milestones 6 to 8 are where the markup arrives, and
    /// milestone 5 is the writer work they need.
    NotDrawn,
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
    /// `Width` or `Height` is absent, unparseable or outside what a page can
    /// be, so the page took the book's own size instead of a size the file
    /// stated.
    SizeUnusable,
}

impl core::fmt::Display for XpsPageDefect {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            XpsPageDefect::NotDrawn => "the page's markup is not drawn yet",
            XpsPageDefect::SourceUnresolved => "a `PageContent` naming no part",
            XpsPageDefect::DocumentUnresolved => "a `DocumentReference` naming no readable part",
            XpsPageDefect::Unreadable => "the fixed page part is not a readable `FixedPage`",
            XpsPageDefect::SizeUnusable => "the fixed page states no usable size",
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
    match recognise(&mut package, limits) {
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
fn recognise(package: &mut Package<'_>, limits: &Limits) -> Recognition {
    // Step 1 is the caller's: the bytes began with a local file header and
    // opened as a ZIP.
    //
    // Step 2. Both items, by the names OPC fixes for them, and **no read**: a
    // comic archive stops here having cost nothing.
    if package.item_index(opc::CONTENT_TYPES_ITEM).is_none() {
        return Recognition::NotXps;
    }
    let Some(rels) = package.item_index(opc::PACKAGE_RELATIONSHIPS_ITEM) else {
        return Recognition::NotXps;
    };

    // Step 3. From here the archive carries OPC's own two parts, so a failure
    // is a broken package rather than a comic — see this module's header for
    // why that refinement of E.3 is taken.
    let relationships = match package
        .read(rels)
        .and_then(|bytes| opc::parse_relationships(bytes, &limits.xml))
    {
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
    size: Option<(f64, f64)>,
    defect: XpsPageDefect,
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

    let documents = match package.read_part(sequence) {
        Ok(bytes) => child_sources(bytes, "FixedDocumentSequence", "DocumentReference", limits),
        Err(_) => None,
    };
    // "A `FixedDocumentSequence` naming no documents at all" is a package-level
    // refusal in gap 30's design, and so is one that will not read: there is no
    // page to degrade *to*.
    let Some(documents) = documents.filter(|sources| !sources.is_empty()) else {
        return Err(ArchiveRefusal::NoFixedPages);
    };

    let mut plans: Vec<Plan> = Vec::new();
    for source in documents {
        let document = source
            .as_deref()
            .and_then(|reference| sequence.resolve(reference));
        let pages = match &document {
            Some(name) => match package.read_part(name) {
                Ok(bytes) => child_sources(bytes, "FixedDocument", "PageContent", limits),
                Err(_) => None,
            },
            None => None,
        };
        let Some(pages) = pages else {
            // A document that vanished takes an unknown number of pages with
            // it, so it leaves one visible page behind rather than nothing.
            push_plan(
                &mut plans,
                Plan {
                    name: reference_name(&document, source.as_deref()),
                    size: None,
                    defect: XpsPageDefect::DocumentUnresolved,
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
                plan_page(package, &base, page.as_deref(), limits),
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

    for (number, plan) in plans.iter().enumerate() {
        let (width, height) = plan.size.unwrap_or(fallback);
        builder.add_page(width, height, |page| {
            page.fill_rect(0.0, 0.0, width, height, PLACEHOLDER_GREY);
        });
        warnings.push(ArchiveWarning::XpsPage {
            page: u32::try_from(number).unwrap_or(u32::MAX),
            defect: plan.defect,
        });
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
        ArchiveReport::synthesised(warnings, pages, synthesised_bytes, Some(dialect)),
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
    base: &PartName,
    source: Option<&str>,
    limits: &Limits,
) -> Plan {
    let Some(page) = source.and_then(|reference| base.resolve(reference)) else {
        return Plan {
            name: source.unwrap_or("").to_string(),
            size: None,
            defect: XpsPageDefect::SourceUnresolved,
        };
    };
    let name = page.as_str().to_string();
    if !package.has(&page) {
        return Plan {
            name,
            size: None,
            defect: XpsPageDefect::SourceUnresolved,
        };
    }
    let stated = match package.read_part(&page) {
        Ok(bytes) => fixed_page_size(bytes, limits),
        Err(_) => None,
    };
    match stated {
        Some(Some(size)) => Plan {
            name,
            size: Some(size),
            defect: XpsPageDefect::NotDrawn,
        },
        // A `FixedPage` whose size is missing or unusable is still a page, at
        // the book's own size.
        Some(None) => Plan {
            name,
            size: None,
            defect: XpsPageDefect::SizeUnusable,
        },
        None => Plan {
            name,
            size: None,
            defect: XpsPageDefect::Unreadable,
        },
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

/// A fixed page's size in **PDF points**, from the `Width` and `Height` its
/// root element states.
///
/// `None` refuses the part; `Some(None)` is a `FixedPage` that stated no usable
/// size. The read stops at the root element, so a page of forty thousand path
/// segments costs the same here as an empty one.
fn fixed_page_size(bytes: &[u8], limits: &Limits) -> Option<Option<(f64, f64)>> {
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
        return Some(match (width, height) {
            (Some(width), Some(height)) => {
                Some((width * UNITS_TO_POINTS, height * UNITS_TO_POINTS))
            }
            _ => None,
        });
    }
    None
}

/// One coordinate, in XPS units, checked against what a page can be.
fn page_units(text: &str) -> Option<f64> {
    let value = text.trim().parse::<f64>().ok()?;
    (value.is_finite() && value > 0.0 && value <= MAX_PAGE_UNITS).then_some(value)
}
