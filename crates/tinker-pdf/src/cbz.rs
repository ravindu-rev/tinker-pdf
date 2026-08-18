//! A comic archive becomes a document (gap 29, milestone 5).
//!
//! A CBZ is a ZIP of page images, one file per page. This module is where the
//! archive stops being a bag of names and becomes a `Document`: which entries
//! are pages, in what order, at what size, and what a page that cannot be
//! built looks like.
//!
//! # Why this is in the facade and not below it
//!
//! Ruling 8 keeps the leaves PDF-free — `tinker-pdf-zip` turns bytes into
//! names and byte ranges and has no opinion about what an entry is *for* — and
//! deciding what a page *is* needs document types. It cannot live in
//! `tinker-pdf-cos` either without teaching the object model about a container
//! no PDF stream will ever hold. So it lives here, beside `resources.rs`,
//! which already makes the other boundary decisions for the same reason.
//!
//! # Synthesis, not an enum
//!
//! [`crate::Document::cos`] returns `&CosDocument` — a **borrow, and not an
//! `Option`** — and that one signature decides the whole shape of this. A CBZ
//! has no `CosDocument` to lend, so one is *made*: the pages are built through
//! [`DocumentBuilder`], serialised by `finish`, and parsed straight back by
//! `CosDocument::open`. The synthesised document goes out through the writer
//! and in through the reader, so it is parsed by the same parser as every real
//! PDF and cannot take a shortcut around it — and every capability the engine
//! already has (cancellation, warnings, `at_dpi`, bounded painting) arrives for
//! nothing rather than being wired a second time.
//!
//! # The image bytes are not touched
//!
//! Synthesis happens whole at open, so whatever a page holds is held for the
//! document's life. A JPEG is placed verbatim ([`ImageData::Jpeg`]) and a PNG
//! goes through [`png_image`], whose default route copies the IDAT into a
//! `/FlateDecode` stream with `/Predictor 15` and never builds a raster.
//! Decoding every page instead would cost *w x h x 3* each — about 3.6 GB for
//! a 200-page archive at 2000 x 3000 — and the failure would arrive only at
//! the size that matters.
//!
//! # Two levels of refusal, and the difference is the feature
//!
//! **Archive level** ([`ArchiveRefusal`], via [`crate::OpenError`]): not a ZIP,
//! damaged past recovery, every entry encrypted, multi-disk, a Zip64 value the
//! archive cannot contain, a bound spent, and a valid archive with no image
//! entries at all. That last one matters most: opening it as a zero-page
//! document is `NotAPdf`'s failure wearing a success costume — a host asks for
//! the page count, gets 0, and shows an empty reader with no error to display.
//!
//! **Page level** ([`PageDefect`]): an entry that is recognisably an image and
//! cannot be turned into a real page **still becomes a page**, of the right
//! size, carrying the neutral placeholder and a named warning. Dropping it
//! would produce a document that looks complete — 99 pages where the archive
//! holds 100, every page correct, and the story jumping in the middle with
//! nothing anywhere saying so.
//!
//! Entries that are not images at all — `ComicInfo.xml`, `Thumbs.db`,
//! `__MACOSX/`, directory records — are neither pages nor warnings. Skipping
//! them is correct rather than lenient, and warning about them would bury the
//! warnings that matter.

use std::borrow::Cow;
use std::cmp::Ordering;

use tinker_pdf_cos::{png_image, DocumentBuilder, ImageData, PngImageData};
use tinker_pdf_filters::Limits as FilterLimits;
use tinker_pdf_zip::{Archive, ArchiveError, Entry};

pub use tinker_pdf_zip::{
    limits as zip_limits, EntryError as ZipEntryError, InflateWarning, Limits as ZipLimits,
    Warning as ZipWarning,
};

// ---- Bounds -----------------------------------------------------------------

/// The most pages one archive may synthesise.
///
/// Gap 18a milestone 8 found `MAX_JPX_WORK` set *above* the most its own inputs
/// could ask for, so it could never fire. That failure decides this number in
/// one direction: [`zip_limits::MAX_ZIP_ENTRIES`] is 16 384, so a cap at or
/// above that could never be the thing that stopped anything, and this one has
/// to sit below it to be a cap at all.
///
/// | | Pages |
/// | --- | --- |
/// | The most any fixture in this repository spends | 4 096 (the archive built *past* this cap holds 4 097 entries; nothing else here holds more than 13) |
/// | A 200-page comic | 200 |
/// | **This cap** | **4 096** |
///
/// A page costs an object graph as well as an archive entry, which is why the
/// archive's entry cap is not this cap and why `cos::limits::MAX_PAGES` — 2^21
/// — is not either: that is a bound on a page tree a real document may have,
/// and it is four hundred times more pages than any comic is.
///
/// Reachable: `a_page_count_past_the_cap_is_refused_by_name` builds the real
/// 4 097-entry archive rather than lowering the constant, because a cap proved
/// only against a lowered copy of itself has not been proved to fire.
pub const MAX_CBZ_PAGES: usize = 4_096;

/// The most bytes the synthesiser may hand [`tinker_pdf_cos::CosDocument`].
///
/// Charged **before each page is built**, on what that page has undertaken to
/// contribute: its image bytes, its soft mask, and [`PAGE_OVERHEAD`] for the
/// objects it costs. That is `tinker-pdf-zip`'s `Budget` posture — a permit is
/// what has been promised, not what happened to arrive — and it is what keeps
/// the *work* bounded rather than only the answer.
///
/// The archive's own caps do not bound this on their own, which is the reason
/// it exists: [`ZipLimits::max_inflated_total`] bounds the image data and
/// nothing bounds the object graph, so thousands of one-byte entries are a page
/// tree far larger than their input.
///
/// | | Bytes |
/// | --- | --- |
/// | The most any fixture in this repository spends | ~70 KB |
/// | A 200-page comic at 2000 x 3000, 1.5 MB a page | ~300 MB |
/// | **This cap** | **512 MiB** |
///
/// A margin of 1.8x, and deliberately not more. The synthesiser holds two
/// copies of this in flight — the object set and the serialised output — and
/// `usize` is 32 bits on `wasm32-unknown-unknown` and `wasm32-wasip1`, both of
/// which this engine builds for, where a gigabyte held twice is more than half
/// the address space and the allocator would fire before the cap did.
pub const MAX_SYNTHESISED_PDF: usize = 512 << 20;

/// What one page costs in objects, charged against [`MAX_SYNTHESISED_PDF`]
/// on top of its image data.
///
/// A page dictionary, a content stream, an image dictionary, their three
/// cross-reference entries and this page's slot in `/Kids`. Measured rather
/// than guessed: `the_synthesised_document_fits_inside_what_was_charged_for_it`
/// builds documents from 1 to 200 pages and asserts the finished length is
/// inside the charge, so a change to the writer that made a page cost more
/// would fail rather than quietly overrun the cap.
pub const PAGE_OVERHEAD: usize = 1_024;

/// What the catalog, the page tree, the header and the trailer cost, charged
/// once against [`MAX_SYNTHESISED_PDF`] before the first page.
pub const DOCUMENT_OVERHEAD: usize = 4_096;

/// The neutral grey a placeholder page is filled with.
///
/// The same 0xBF the renderer paints over an image it could not decode, so a
/// page that is missing looks the same whichever level lost it. Grey rather
/// than a diagonal-cross convention because it never reads as content.
///
/// Shared with [`crate::xps`] rather than copied, so a fixed page whose markup
/// is not painted yet and a comic page whose image would not decode look the
/// same — which they should, because they are the same statement.
pub(crate) const PLACEHOLDER_GREY: f64 = 191.0 / 255.0;

/// The page size a placeholder falls back to when the book never says.
///
/// US Letter, and it is only ever reached by an archive in which **no** entry
/// produced a real image — one of nothing but GIFs, say. A placeholder in an
/// ordinary archive takes the size of the first real page instead, because a
/// comic's pages are one size and a placeholder that matches its neighbours is
/// the one a reader can page through.
const FALLBACK_PAGE: (f64, f64) = (612.0, 792.0);

/// Resource ceilings for synthesising a document from an archive.
///
/// Separate from [`ZipLimits`] and carrying one, because they bound different
/// things: the archive's caps bound what comes *out* of the container and these
/// bound what is built *from* it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Pages synthesised. See [`MAX_CBZ_PAGES`].
    pub max_pages: usize,
    /// Bytes of the document handed to the parser. See [`MAX_SYNTHESISED_PDF`].
    pub max_synthesised: usize,
    /// What the archive reader is allowed to spend.
    pub zip: ZipLimits,
}

impl Limits {
    /// The constants above, which is what [`Default`] hands back.
    pub const DEFAULT: Self = Self {
        max_pages: MAX_CBZ_PAGES,
        max_synthesised: MAX_SYNTHESISED_PDF,
        zip: ZipLimits::DEFAULT,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

// ---- The sniff --------------------------------------------------------------

/// A container format recognised at the head of a file.
///
/// Recognising the three this build does not read is the point rather than an
/// afterthought: "this is a CBR and I do not read CBR" is a different sentence
/// from "this is not a PDF", and a host shows different things for them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Container {
    /// `PK\x03\x04` — a ZIP local file header, which is what a CBZ is.
    Zip,
    /// `Rar!\x1A\x07` — RAR, which a CBR is. Not read here.
    Rar,
    /// `7z\xBC\xAF\x27\x1C` — 7z, which a CB7 is. Not read here.
    SevenZip,
    /// POSIX `ustar` — tar, which a CBT is. Not read here.
    Tar,
}

/// Which container the bytes **begin** with, if any.
///
/// Every signature here is tested at a fixed position and nowhere else, which
/// is what keeps a PDF that happens to contain `PK\x03\x04` inside a stream —
/// an attachment, a compressed object, a font program — an ordinary PDF.
/// Searching for these would turn a legal document into a refused archive.
///
/// tar's `ustar` is at offset 257 because that is where POSIX 1003.1 puts the
/// magic field of the first header block. It is still a fixed position rather
/// than a search.
#[must_use]
pub fn container(bytes: &[u8]) -> Option<Container> {
    if bytes.starts_with(b"PK\x03\x04") {
        return Some(Container::Zip);
    }
    if bytes.starts_with(b"Rar!\x1A\x07") {
        return Some(Container::Rar);
    }
    if bytes.starts_with(b"7z\xBC\xAF\x27\x1C") {
        return Some(Container::SevenZip);
    }
    if bytes.get(257..262) == Some(b"ustar".as_slice()) {
        return Some(Container::Tar);
    }
    None
}

// ---- Refusals and reports ---------------------------------------------------

/// Why a container this build recognises could not be opened as a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ArchiveRefusal {
    /// A container this build does not read: RAR, 7z, tar.
    NotAZip,
    /// The central directory could not be located and no local header was
    /// found either.
    Damaged,
    /// Every entry that could have been a page is encrypted, so there is
    /// nothing to page. Neither ZipCrypto nor the AES extensions are in scope.
    Encrypted,
    /// Spanned or multi-disk. The fragment that happens to be here is not the
    /// archive.
    MultiDisk,
    /// A Zip64 field declares a size or offset the archive cannot contain.
    Zip64OutOfBounds,
    /// A valid archive holding not one image entry.
    ///
    /// Refused rather than opened as zero pages: a host that asks for the page
    /// count, gets 0 and has no error to show has been handed a failure
    /// dressed as a success.
    ///
    /// **This is a sentence about a comic archive and never about an XPS.**
    /// Before gap 30's milestone 3 it was what five of milestone 1's eight real
    /// packages came back as, including a three-page document, because the
    /// comic path counted raster parts and a fixed document has none.
    NoImages,
    /// A bound was spent: [`MAX_CBZ_PAGES`], [`MAX_SYNTHESISED_PDF`],
    /// [`crate::xps::MAX_XPS_PARTS`], [`crate::xps::MAX_XPS_PAGES`], or one of
    /// the archive reader's own.
    TooLarge,
    /// An OPC package that claims to be an XPS and whose own structure will not
    /// read (gap 30, milestone 3).
    ///
    /// ECMA-388 E.3's second step held — the archive carries both
    /// `[Content_Types].xml` and `_rels/.rels` — and its third did not: a
    /// content-types item or a package relationships part that is not
    /// well-formed XML, a fixed-representation target naming a part the package
    /// does not hold, or one whose media type is not the fixed document
    /// sequence's.
    ///
    /// Refused rather than fallen through to the comic path, which is a
    /// deliberate refinement of E.3 and is argued in [`crate::xps`]'s header: a
    /// package that carries OPC's own two parts is not a comic, and paging its
    /// images would be gap 30's headline defect wearing a smaller hat. A ZIP
    /// carrying a `[Content_Types].xml` and **no** relationships part is still
    /// a comic archive, and so is an OPC package that names no fixed
    /// representation at all.
    UnreadablePackage,
    /// An interleaved package: OPC 7.2.4's `…/[0].piece` items.
    ///
    /// Recognised and refused by name rather than half-assembled. Reassembling
    /// them is a second addressing model layered on the first, and no package
    /// in gap 30 milestone 1's corpus uses one — so the refusal is tied to
    /// evidence rather than to taste, and the plan says what would change it.
    Interleaved,
    /// A package holding an item that is not a part name (OPC 6.2.2.2) and is
    /// not the content-types item.
    InvalidPartName,
    /// A package holding two part names equal under OPC 6.2.2.3's ASCII case
    /// folding, or one name derivable from another — `/a` beside `/A`, or
    /// `/segment1` beside `/segment1/segment2`.
    ///
    /// Refused rather than resolved to whichever came first, because "whichever
    /// came first" is directory order and directory order is whatever the
    /// producing tool walked.
    AmbiguousPartNames,
    /// An XPS whose fixed payload names no page: a `FixedDocumentSequence` that
    /// will not read or names no document, or a document set that names no
    /// `PageContent` between them.
    ///
    /// There is no page to degrade *to*, which is the one thing that separates
    /// this from every page-level defect in [`crate::xps::XpsPageDefect`].
    NoFixedPages,
}

impl core::fmt::Display for ArchiveRefusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            ArchiveRefusal::NotAZip => "a container this build does not read",
            ArchiveRefusal::Damaged => "the archive is damaged past recovery",
            ArchiveRefusal::Encrypted => "every entry that could be a page is encrypted",
            ArchiveRefusal::MultiDisk => "a spanned or multi-disk archive",
            ArchiveRefusal::Zip64OutOfBounds => "a Zip64 value the archive cannot contain",
            ArchiveRefusal::NoImages => "a valid archive with no image entries",
            ArchiveRefusal::TooLarge => "the archive is past a bound this build enforces",
            ArchiveRefusal::UnreadablePackage => {
                "an XPS package whose own structure could not be read"
            }
            ArchiveRefusal::Interleaved => "an interleaved package, which is not reassembled here",
            ArchiveRefusal::InvalidPartName => "an item that is not a part name",
            ArchiveRefusal::AmbiguousPartNames => "two part names one package may not both hold",
            ArchiveRefusal::NoFixedPages => "a fixed payload that names no page",
        })
    }
}

/// An image format recognised by its magic bytes.
///
/// Classification is **never** by extension: a `.jpg` that is a PNG is routine,
/// and an extension is a claim where the first bytes of a file are a fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageFormat {
    /// JFIF/EXIF JPEG. Read.
    Jpeg,
    /// PNG. Read.
    Png,
    /// GIF. Not read here.
    Gif,
    /// WebP. Not read here.
    WebP,
    /// Windows bitmap. Not read here.
    Bmp,
    /// TIFF, either byte order. Not read here.
    Tiff,
    /// AVIF. Not read here.
    Avif,
    /// JPEG 2000, in the JP2 wrapper or as a bare codestream. Not read here —
    /// this engine has a JPX decoder for PDF streams and no route from a
    /// container entry to it.
    Jpeg2000,
}

/// Why a page is a placeholder rather than the picture the archive holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PageDefect {
    /// An image format this build does not read, named rather than collapsed.
    UnsupportedFormat(ImageFormat),
    /// The archive would not hand the entry's bytes over: encrypted, a
    /// checksum failure, a truncation, a compression method not read here.
    EntryRefused(ZipEntryError),
    /// The bytes are a JPEG or a PNG and could not be made into an image —
    /// an unreadable header, a colour type outside Table 11.1, a raster past
    /// the ceiling.
    Undecodable,
}

/// What synthesising a document from an archive tolerated (ruling 10).
///
/// Recorded at open, because open is where the knowledge is: by the time a page
/// is rendered the entry it came from is gone.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ArchiveWarning {
    /// What the archive reader tolerated, carried verbatim.
    Zip(ZipWarning),
    /// An entry that is an image became a placeholder page. **The page count
    /// and every page number after it are unchanged**, which is the whole
    /// reason the entry was not dropped.
    PlaceholderPage {
        /// Zero-based page index.
        page: u32,
        /// Why.
        defect: PageDefect,
    },
    /// The image reached the page and something about it was degraded: a
    /// raster short by whole rows, a dropped ancillary chunk, a file that
    /// stopped before its end marker.
    DegradedImage {
        /// Zero-based page index.
        page: u32,
    },
    /// A synthesised **fixed page** is not the picture the package holds (gap
    /// 30, milestone 3).
    ///
    /// Every page of every XPS carries one of these while gap 30 is being
    /// built, and [`crate::xps::XpsPageDefect::NotDrawn`] is the honest reason:
    /// the spine is read, the page is at the size its own markup states, and
    /// the markup is not painted yet. A page that came back silent would be
    /// gap 17's blank page reported as success.
    XpsPage {
        /// Zero-based page index.
        page: u32,
        /// Why.
        defect: crate::xps::XpsPageDefect,
    },
}

/// Where one page came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageOrigin {
    /// The archive entry's stored path, as the archive spelled it.
    pub name: String,
    /// `None` when the page is the entry's own picture.
    pub defect: Option<PageDefect>,
}

/// What a document synthesised from a container cost, and where its pages came
/// from.
///
/// Reachable through [`crate::Document::archive`], and `None` for an ordinary
/// PDF — which is how a caller tells the two apart without guessing from the
/// bytes it handed in.
#[derive(Clone, Debug)]
pub struct ArchiveReport {
    warnings: Vec<ArchiveWarning>,
    pages: Vec<PageOrigin>,
    synthesised_bytes: usize,
    dialect: Option<crate::xps::Dialect>,
    parsed_parts: usize,
}

impl ArchiveReport {
    /// Builds a report. `pub(crate)` because the two synthesisers are the only
    /// things that can honestly fill one in.
    pub(crate) fn synthesised(
        warnings: Vec<ArchiveWarning>,
        pages: Vec<PageOrigin>,
        synthesised_bytes: usize,
        dialect: Option<crate::xps::Dialect>,
        parsed_parts: usize,
    ) -> ArchiveReport {
        ArchiveReport {
            warnings,
            pages,
            synthesised_bytes,
            dialect,
            parsed_parts,
        }
    }

    /// Everything tolerated, in the order it happened.
    #[must_use]
    pub fn warnings(&self) -> &[ArchiveWarning] {
        &self.warnings
    }

    /// Which spelling of XPS this document was, or `None` for a comic archive.
    ///
    /// Public because gap 30's decision 3 is a claim a caller should be able to
    /// check rather than take on trust: the two dialects carry **byte-identical
    /// content types**, which milestone 1 measured, so the namespace is the
    /// only discriminator there is and this is where the answer it reached
    /// comes out.
    #[must_use]
    pub fn xps_dialect(&self) -> Option<crate::xps::Dialect> {
        self.dialect
    }

    /// One entry per page, in page order.
    #[must_use]
    pub fn pages(&self) -> &[PageOrigin] {
        &self.pages
    }

    /// How many bytes the synthesised document is.
    ///
    /// Public because a bound nobody can measure is a bound nobody can check:
    /// this is what the [`MAX_SYNTHESISED_PDF`] ledger's first column is read
    /// from, rather than asserted about in prose.
    #[must_use]
    pub fn synthesised_bytes(&self) -> usize {
        self.synthesised_bytes
    }

    /// How many parts' markup was parsed while the document was synthesised
    /// (gap 30, milestone 4).
    ///
    /// **Zero for a comic archive**, and that is a real answer rather than a
    /// placeholder: the comic path reads magic bytes and image headers and
    /// parses no markup at all — `ComicInfo.xml` is deliberately nobody's scope.
    ///
    /// For an XPS this is the count of **distinct parts**, not of references to
    /// them, and it is published for the same reason
    /// [`ArchiveReport::synthesised_bytes`] is. `crate::xps::Payload` caches a
    /// parse per part because `Source::new` walks the whole part before it
    /// yields an event, so a fixed document naming one page four thousand times
    /// would otherwise scan it four thousand times — and nothing else about the
    /// document would change, so a test with no counter to read could not tell
    /// the cache from its own absence.
    #[must_use]
    pub fn parsed_parts(&self) -> usize {
        self.parsed_parts
    }
}

// ---- Ordering ---------------------------------------------------------------

/// Natural order over stored paths, so `page2` precedes `page10`.
///
/// Lexicographic is the obvious wrong answer and it fails invisibly: an archive
/// of `1.jpg` through `12.jpg` reads as 1, 10, 11, 12, 2, 3 … — every page
/// present, every page rendered correctly, and the comic unreadable. A
/// zero-padded archive (`page001.jpg`) sorts identically under both orders, so
/// a fixture with padded names proves **nothing** about this and must say so.
/// The ZIP format is no help: APPNOTE puts no ordering requirement on the
/// central directory.
///
/// Defined in byte arithmetic so ruling 4 holds — a locale-aware or
/// Unicode-collating comparison is a per-platform answer and this engine may
/// not have one.
///
/// - The **full stored path**, not the basename, so directories group.
/// - Split into maximal runs of ASCII digits and maximal runs of everything
///   else, compared run by run.
/// - Two digit runs compare by **length after leading zeros are trimmed, then
///   by those bytes** — never by parsing an integer, because a forty-digit run
///   is a legal filename and `u64::from_str` is not a total function over one.
/// - Any other pairing compares as text: byte by byte with ASCII letters
///   case-folded, then by the unfolded bytes to break the tie.
/// - Runs exhausted first sorts first, and two names that are still equal
///   (`p01` against `p1`) fall back to their raw bytes, so the order is a total
///   one before the caller's own tie-break is reached.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0usize, 0usize);

    while i < a.len() && j < b.len() {
        // `i < a.len()` and `j < b.len()`, so both index in bounds.
        let a_digit = a[i].is_ascii_digit();
        let b_digit = b[j].is_ascii_digit();
        let next_a = run_end(a, i, a_digit);
        let next_b = run_end(b, j, b_digit);
        let (ra, rb) = (&a[i..next_a], &b[j..next_b]);

        let ord = if a_digit && b_digit {
            digit_cmp(ra, rb)
        } else {
            text_cmp(ra, rb)
        };
        if ord != Ordering::Equal {
            return ord;
        }
        i = next_a;
        j = next_b;
    }

    // One of them ran out of runs: the shorter remainder sorts first.
    (a.len() - i)
        .cmp(&(b.len() - j))
        // Equal run by run and still not the same string — `p01` against `p1`,
        // which the digit rule calls equal on purpose. Raw bytes settle it so
        // the comparison is total rather than leaving two names unordered.
        .then_with(|| a.cmp(b))
}

/// Where the run starting at `at` ends: maximal digits, or maximal non-digits.
fn run_end(s: &[u8], at: usize, digits: bool) -> usize {
    let mut end = at;
    while end < s.len() && s[end].is_ascii_digit() == digits {
        end += 1;
    }
    end
}

/// A digit run: magnitude first, then the digits themselves.
fn digit_cmp(a: &[u8], b: &[u8]) -> Ordering {
    let ta = trim_zeros(a);
    let tb = trim_zeros(b);
    ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb))
}

/// Leading zeros removed. All-zero trims to nothing, which is the right
/// magnitude for it.
fn trim_zeros(s: &[u8]) -> &[u8] {
    let mut at = 0usize;
    while at < s.len() && s[at] == b'0' {
        at += 1;
    }
    &s[at..]
}

/// A text run: ASCII case folded, then the unfolded bytes.
fn text_cmp(a: &[u8], b: &[u8]) -> Ordering {
    a.iter()
        .map(u8::to_ascii_lowercase)
        .cmp(b.iter().map(u8::to_ascii_lowercase))
        .then_with(|| a.cmp(b))
}

/// The archive's entries in reading order.
///
/// Ties break on the entry's position in the central directory, which makes the
/// order total: two entries in a ZIP may legally have the same name.
fn reading_order(entries: &[Entry]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by(|&x, &y| {
        // Both indices come from the range above, so neither `get` can miss;
        // the fallbacks keep the comparison total rather than panicking.
        let (a, b) = (entries.get(x), entries.get(y));
        match (a, b) {
            (Some(a), Some(b)) => natural_cmp(&a.name, &b.name).then_with(|| a.index.cmp(&b.index)),
            _ => x.cmp(&y),
        }
    });
    order
}

// ---- Classification ---------------------------------------------------------

/// What the first bytes of an entry say it is.
///
/// Magic bytes and nothing else. An entry matching none of these is not an
/// image, is not a page and is not a warning — `ComicInfo.xml`, `Thumbs.db`,
/// `.DS_Store` and the `__MACOSX/` AppleDouble files all land here, and warning
/// about each of them would bury the warnings that matter.
#[must_use]
pub fn image_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(ImageFormat::Jpeg);
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(ImageFormat::Png);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(ImageFormat::Gif);
    }
    // RIFF container with a WEBP form type; the four bytes between are the
    // chunk length and carry no signature.
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
        return Some(ImageFormat::WebP);
    }
    // ISO base media: a `ftyp` box whose major brand is one of AVIF's two.
    if matches!(bytes.get(4..12), Some(b"ftypavif" | b"ftypavis")) {
        return Some(ImageFormat::Avif);
    }
    if bytes.starts_with(b"II\x2A\x00") || bytes.starts_with(b"MM\x00\x2A") {
        return Some(ImageFormat::Tiff);
    }
    // 15444-1 I.5.1's twelve-byte JP2 signature box, and the bare SOC/SIZ pair
    // a `.j2k` codestream starts with.
    if bytes.starts_with(&[
        0, 0, 0, 0x0C, b'j', b'P', b' ', b' ', 0x0D, 0x0A, 0x87, 0x0A,
    ]) || bytes.starts_with(&[0xFF, 0x4F, 0xFF, 0x51])
    {
        return Some(ImageFormat::Jpeg2000);
    }
    // Two bytes is a thin signature, so it is checked last and only for a file
    // long enough to carry a BITMAPFILEHEADER and the smallest DIB header.
    if bytes.starts_with(b"BM") && bytes.len() >= 26 {
        return Some(ImageFormat::Bmp);
    }
    None
}

/// Whether a name claims to be an image, for an entry whose bytes cannot be
/// read at all.
///
/// This is the **one** place an extension is consulted, and only because there
/// is nothing else: an encrypted entry, or one whose checksum failed, has no
/// first bytes to classify. The question here is narrower than "what is this" —
/// it is "was this meant to be a page", and getting it wrong in the direction
/// of making a page is the safe direction, because a spurious placeholder is
/// visible where a missing page renumbers every page after it.
fn extension_claims_image(name: &str) -> bool {
    // 4.4.17.1: separators are `/`, so the last segment is the file name.
    let file = name.rsplit('/').next().unwrap_or(name);
    let Some((stem, extension)) = file.rsplit_once('.') else {
        return false;
    };
    // A name that is nothing but a dot and a suffix is a hidden file rather
    // than a page — `.DS_Store` is the one every archiver leaves behind.
    if stem.is_empty() {
        return false;
    }
    let lower = extension.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "jpg"
            | "jpeg"
            | "jpe"
            | "jfif"
            | "png"
            | "gif"
            | "webp"
            | "bmp"
            | "dib"
            | "tif"
            | "tiff"
            | "avif"
            | "jp2"
            | "j2k"
            | "jpf"
            | "jpx"
    )
}

// ---- Synthesis --------------------------------------------------------------

/// One page, decided but not yet written.
enum Content<'a> {
    /// JPEG bytes, placed exactly as the archive holds them.
    Jpeg(Cow<'a, [u8]>),
    /// A PNG, through gap 29 milestone 4's chooser.
    Png(Box<PngImageData>),
    /// Nothing usable; the page is the neutral placeholder.
    Placeholder,
}

/// A page's plan: where it came from, how big it is, and what goes on it.
struct Plan<'a> {
    name: String,
    /// `None` when the size is not known and has to be borrowed from the book.
    size: Option<(f64, f64)>,
    content: Content<'a>,
    defect: Option<PageDefect>,
    degraded: bool,
    /// Bytes this page has undertaken to contribute to the finished document.
    charge: usize,
}

/// Turns an archive into the bytes of a PDF, and a report of what that cost.
///
/// This is what [`crate::Document::open`] does with a container, and it is
/// public for two reasons. Converting a comic archive into a PDF is a thing a
/// host wants for its own sake — the bytes here are a complete document, and
/// [`tinker_pdf_cos::CosDocument::open`] is what the facade does with them
/// next. And a bound that can only ever be exercised at its shipped value is a
/// bound no test can prove fires: `limits` is how
/// `a_synthesis_past_the_byte_cap_is_refused_by_name` reaches
/// [`MAX_SYNTHESISED_PDF`]'s branch without building half a gigabyte.
///
/// # Errors
/// [`ArchiveRefusal`], one variant per refusal, each of them by name.
///
/// # This door is the comic path and nothing else
///
/// It opens its own archive and pages whatever it finds, so handing it an XPS
/// gets a comic made of that package's resources — which is precisely gap 30's
/// headline defect, and is why [`crate::Document::open`] does **not** come
/// through here. That route opens the archive once and asks
/// [`crate::xps::route`] what it is first.
pub fn synthesise(
    what: Container,
    bytes: &[u8],
    limits: &Limits,
) -> Result<(Vec<u8>, ArchiveReport), ArchiveRefusal> {
    if what != Container::Zip {
        // Recognised and refused. RAR, 7z and tar are three more
        // decompressors, two of them encumbered, and none of them a page.
        return Err(ArchiveRefusal::NotAZip);
    }
    let archive = open_archive(bytes, &limits.zip)?;
    pages_from_archive(archive, limits)
}

/// Opens the archive, once, mapping the reader's refusals onto this one's.
///
/// Split out of [`synthesise`] so `Document::open` can open a ZIP **once** and
/// then decide what it is: milestone 3's exit criterion is that the sniff and
/// the read share one [`Archive`], because opening it twice doubles the work
/// and creates a window in which the two reads could disagree.
///
/// # Errors
/// [`ArchiveRefusal`], one variant per refusal the archive reader made.
pub fn open_archive<'a>(
    bytes: &'a [u8],
    limits: &ZipLimits,
) -> Result<Archive<'a>, ArchiveRefusal> {
    Archive::open(bytes, limits).map_err(|e| match e {
        ArchiveError::NotAZip => ArchiveRefusal::NotAZip,
        ArchiveError::Damaged => ArchiveRefusal::Damaged,
        ArchiveError::MultiDisk => ArchiveRefusal::MultiDisk,
        ArchiveError::Zip64OutOfBounds => ArchiveRefusal::Zip64OutOfBounds,
        ArchiveError::TooManyEntries => ArchiveRefusal::TooLarge,
    })
}

/// Pages an already-open archive as a comic.
///
/// # Errors
/// [`ArchiveRefusal`], one variant per refusal, each of them by name.
pub fn pages_from_archive(
    mut archive: Archive<'_>,
    limits: &Limits,
) -> Result<(Vec<u8>, ArchiveReport), ArchiveRefusal> {
    let order = reading_order(archive.entries());
    let mut plans: Vec<Plan<'_>> = Vec::new();
    let mut spent = DOCUMENT_OVERHEAD;

    for index in order {
        let Some(entry) = archive.entries().get(index).cloned() else {
            continue;
        };
        // 4.4.17.1: a stored path ending in `/` is a directory record, which
        // holds no bytes and is not a page.
        if entry.is_directory() {
            continue;
        }
        let Some(plan) = plan_entry(&mut archive, index, &entry, limits) else {
            continue;
        };

        if plans.len() >= limits.max_pages {
            return Err(ArchiveRefusal::TooLarge);
        }
        spent = spent
            .checked_add(plan.charge)
            .filter(|&total| total <= limits.max_synthesised)
            .ok_or(ArchiveRefusal::TooLarge)?;
        plans.push(plan);
    }

    if plans.is_empty() {
        return Err(ArchiveRefusal::NoImages);
    }
    // Archive level, and only when there is nothing else: an archive of a
    // hundred JPEGs and one encrypted entry keeps its hundred readable pages.
    if plans
        .iter()
        .all(|p| p.defect == Some(PageDefect::EntryRefused(ZipEntryError::Encrypted)))
    {
        return Err(ArchiveRefusal::Encrypted);
    }

    // A placeholder has no size of its own. It takes the first real page's,
    // because a comic's pages are one size and a placeholder that matches its
    // neighbours is one a reader can page through; paper is the answer only
    // for a book that never states one.
    let fallback = plans.iter().find_map(|p| p.size).unwrap_or(FALLBACK_PAGE);

    let mut builder = DocumentBuilder::new();
    let mut warnings: Vec<ArchiveWarning> = Vec::new();
    let mut pages: Vec<PageOrigin> = Vec::with_capacity(plans.len());

    for (number, plan) in plans.iter().enumerate() {
        // Each page names its own image and no other. Without this every page
        // would inherit every image registered before it, and a 200-page
        // archive would carry 20 100 resource entries for the 200 it uses.
        builder.clear_image_resources();

        let drawn = match &plan.content {
            Content::Jpeg(data) => builder.add_image(IMAGE_RESOURCE, &ImageData::Jpeg(data)),
            Content::Png(png) => builder.add_image(IMAGE_RESOURCE, &png.image()),
            Content::Placeholder => false,
        };

        let (width, height) = plan.size.unwrap_or(fallback);
        builder.add_page(width, height, |page| {
            if drawn {
                // 8.9.5.2: an image occupies the unit square, so one image
                // pixel is one PDF point exactly when the transform is the
                // page's own size.
                page.image(IMAGE_RESOURCE, 0.0, 0.0, width, height);
            } else {
                page.fill_rect(0.0, 0.0, width, height, PLACEHOLDER_GREY);
            }
        });

        // A plan that said "real image" and whose image the writer refused is
        // a placeholder too, and the report has to say so rather than claiming
        // a picture that is not there.
        let defect = plan.defect.or(match plan.content {
            Content::Placeholder => Some(PageDefect::Undecodable),
            _ if drawn => None,
            _ => Some(PageDefect::Undecodable),
        });
        let page = number as u32;
        if let Some(defect) = defect {
            warnings.push(ArchiveWarning::PlaceholderPage { page, defect });
        } else if plan.degraded {
            warnings.push(ArchiveWarning::DegradedImage { page });
        }
        pages.push(PageOrigin {
            name: plan.name.clone(),
            defect,
        });
    }

    // Taken after every read, because reading is what most of them come from.
    for w in archive.warnings() {
        warnings.push(ArchiveWarning::Zip(*w));
    }

    let pdf = builder.finish();
    let synthesised_bytes = pdf.len();
    // No markup: the comic path reads magic bytes and image headers, and
    // `ComicInfo.xml` is gap 29's named non-goal rather than an omission.
    Ok((
        pdf,
        ArchiveReport::synthesised(warnings, pages, synthesised_bytes, None, 0),
    ))
}

/// The resource name every synthesised page gives its own image.
///
/// The same name on every page is not a collision: each page's `/Resources`
/// holds exactly one `/XObject`, and they are different objects.
const IMAGE_RESOURCE: &[u8] = b"Im";

/// Decides what one entry becomes, or `None` when it is not a page at all.
fn plan_entry<'a>(
    archive: &mut Archive<'a>,
    index: usize,
    entry: &Entry,
    limits: &Limits,
) -> Option<Plan<'a>> {
    let placeholder = |defect: PageDefect| Plan {
        name: entry.name.clone(),
        size: None,
        content: Content::Placeholder,
        defect: Some(defect),
        degraded: false,
        charge: PAGE_OVERHEAD,
    };

    let data = match archive.read(index) {
        Ok(data) => data,
        Err(e) => {
            // No bytes, so no magic. See `extension_claims_image` for why the
            // name is allowed to decide this one case and nothing else.
            return extension_claims_image(&entry.name)
                .then(|| placeholder(PageDefect::EntryRefused(e)));
        }
    };

    match image_format(&data)? {
        ImageFormat::Jpeg => {
            // The same reader `add_image` uses, so the `/MediaBox` and the
            // `/Width` cannot disagree.
            let Some((width, height, _)) = tinker_pdf_cos::jpeg_shape(&data) else {
                return Some(placeholder(PageDefect::Undecodable));
            };
            if width == 0 || height == 0 {
                return Some(placeholder(PageDefect::Undecodable));
            }
            Some(Plan {
                name: entry.name.clone(),
                size: Some((f64::from(width), f64::from(height))),
                charge: PAGE_OVERHEAD.saturating_add(data.len()),
                content: Content::Jpeg(data),
                defect: None,
                degraded: false,
            })
        }
        ImageFormat::Png => {
            // The decoded route's ceiling is the largest entry this build will
            // read out of an archive: a page whose raster is bigger than the
            // biggest file the archive may hold is not a comic page. It is the
            // *caller's* number, which is what `ExceedsOutputLimit` carries,
            // so a host that lowered it can tell its decision from the
            // decoder's own `MAX_PNG_SAMPLES`.
            let Ok(png) = png_image(&data, &FilterLimits::new(limits.zip.max_entry_bytes)) else {
                return Some(placeholder(PageDefect::Undecodable));
            };
            if png.width() == 0 || png.height() == 0 {
                return Some(placeholder(PageDefect::Undecodable));
            }
            let size = (f64::from(png.width()), f64::from(png.height()));
            let degraded = !png.complete();
            let charge = PAGE_OVERHEAD.saturating_add(embedded_len(&png));
            Some(Plan {
                name: entry.name.clone(),
                size: Some(size),
                content: Content::Png(Box::new(png)),
                defect: None,
                degraded,
                charge,
            })
        }
        // Recognised, named, and refused at the page level rather than the
        // archive's: an archive of a hundred JPEGs and one GIF keeps its
        // hundred readable pages, and the GIF keeps its page number.
        other => Some(placeholder(PageDefect::UnsupportedFormat(other))),
    }
}

/// How many bytes a prepared PNG will put in the file.
fn embedded_len(png: &PngImageData) -> usize {
    match png.image() {
        ImageData::Compressed(image) => image
            .data
            .len()
            .saturating_add(image.soft_mask.map_or(0, |m| m.data.len())),
        // `png_image` builds nothing else, and a shape that costs nothing to
        // charge for is one that has not been written yet.
        _ => 0,
    }
}

#[cfg(test)]
mod tests;
