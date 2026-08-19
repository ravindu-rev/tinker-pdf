//! The package document: what a book says it is made of (EPUB 3.3 §5).
//!
//! [`super::ocf`] answered *which file is the package document*. This answers
//! *what the book is*: its version (§5.4.1), the three Dublin Core elements
//! §5.5.3.1 requires, the manifest (§5.6) with §5.6.2's `properties` and
//! §3.5.1's fallbacks, and the spine (§5.7) — which is the list milestone 4
//! turns into pages.
//!
//! # A pure function of bytes and a base path
//!
//! Nothing here touches an [`tinker_pdf_zip::Archive`]. [`parse`] takes the
//! package document's bytes and **the container path it was read from**, and
//! hands back structure; resolving an `href` to an archive entry is the
//! synthesiser's, one layer up. That split is what lets every rule below be
//! tested against a string rather than against a ZIP, which is where
//! `src/epub/tests.rs` puts them.
//!
//! The base matters and it is the *referring document*, which is the general
//! rule [`super::ocf::resolve_reference`] was built for and — as milestone 3
//! recorded — had no caller for. `container.xml`'s `full-path` is the one
//! reference in this format resolved against the container root instead. A
//! manifest `href` is not: milestone 1 measured one producer putting its
//! package document at `content.opf` and another at `EPUB/content.opf`, and
//! **199 of 412 hrefs in the two corpora name a directory**, so a build that
//! resolved against the root reads every flat book and loses every nested one.
//!
//! # EPUB 2.0 is a compatibility surface and not a second parser
//!
//! §5.4.1's `version` is `2.0` or `3.x` and anything else is refused by name.
//! The two are read by **one** parser, because the parts milestone 4 needs are
//! the parts the two versions agree about: OPF 2.0.1 §2.2 requires the same
//! three Dublin Core elements, §2.3's `<manifest>`/`<item>` carries the same
//! `id`, `href`, `media-type` and `fallback`, and §2.4's `<spine>` is the same
//! ordered list of `idref`s. What EPUB 2 adds — `opf:role`, `opf:file-as`,
//! `<guide>`, `<dc-metadata>` — is metadata refinement this milestone does not
//! read, and what EPUB 3 adds — `properties`, `refines`, the navigation
//! document — is absent from a 2.0 package rather than spelled differently.
//!
//! So the compatibility surface is: **a 2.0 package paginates exactly as a 3.0
//! one does**, and [`Package::version`] is where a caller can see which it was.
//! Four of milestone 1's twenty-six books are 2.0 and two of those are
//! committed, so the claim is checked against real files rather than a fixture.
//!
//! # What is refused here and what is warned about
//!
//! Refused, because there is no book without it: markup that is not well
//! formed, a root that is not `package` in [`OPF_NAMESPACE`], a version that is
//! neither 2.0 nor 3.x, no `<manifest>` or no `<spine>` element, a spine with
//! no `<itemref>`, and either bound spent.
//!
//! Warned about, because ruling 2 degrades rather than fails: every one of
//! §5.5.3.1's three required elements, separately; a `unique-identifier` naming
//! no `dc:identifier`; and a manifest holding two items with one `id`. A book
//! missing its `dc:language` is still a book, and refusing it would lose one
//! over a line of metadata no page depends on.

use tinker_pdf_xml::{Doctype, Event, Source};

use super::{Limits, MAX_EPUB_FALLBACK_DEPTH};
use crate::cbz::ArchiveRefusal;

// ---- The names the package document fixes -----------------------------------

/// §5.4's namespace, which every element of a package document is in.
pub const OPF_NAMESPACE: &str = "http://www.idpf.org/2007/opf";

/// Dublin Core's, which §5.5.3.1's three required elements are in.
///
/// Both producers declare it on `<metadata>` rather than on the root, and one
/// declares it on both. The reader resolves namespaces, so where it is declared
/// is not a fact this module can see — which is the reason it is read through
/// [`tinker_pdf_xml`] rather than by matching `dc:` as a prefix.
pub const DC_NAMESPACE: &str = "http://purl.org/dc/elements/1.1/";

// ---- Versions ---------------------------------------------------------------

/// Which version of the format a package document declares (§5.4.1).
///
/// Two values, because those are the two this build reads. Anything else is
/// [`ArchiveRefusal::UnsupportedPackageVersion`] — refused rather than guessed
/// at, because guessing 3.0 for an EPUB 4 is how a reader silently mis-reads a
/// vocabulary it has never seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PackageVersion {
    /// `2.0`: OPF 2.0.1, read as the compatibility surface this module's header
    /// describes.
    Epub2,
    /// `3.0` and any other `3.x`. EPUB 3.2 and 3.3 both write `3.0`, and 3.1 —
    /// withdrawn — wrote `3.1`, so the major version is what decides.
    Epub3,
}

impl PackageVersion {
    /// The version an attribute value names, or `None` for one this build does
    /// not read.
    ///
    /// The major version decides, and the rest must be digits and dots: `3.0`
    /// and `3.1` are both EPUB 3, `2.0` and `2.0.1` are both EPUB 2, and `1.0`,
    /// `4.0`, `3.0beta` and an absent attribute are all `None`.
    #[must_use]
    pub fn from_attribute(value: &str) -> Option<PackageVersion> {
        let (major, rest) = value.split_once('.')?;
        if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return None;
        }
        match major {
            "2" => Some(PackageVersion::Epub2),
            "3" => Some(PackageVersion::Epub3),
            _ => None,
        }
    }
}

// ---- Core media types (§3.2) ------------------------------------------------

/// §3.2's core media types: the closed set every reading system must support.
///
/// The set is what §3.5.1's fallback rule is written against — a manifest item
/// whose media type is **not** one of these is a *foreign resource* and must
/// name a `fallback` — so it is a type here rather than a string comparison at
/// the one call site. A media type outside the set is [`Option::None`], which
/// is a fact about the publication and not an omission in this list.
///
/// Two of them are EPUB **content documents** (§6): [`CoreMediaType::Xhtml`]
/// and [`CoreMediaType::Svg`]. Those are the only two a spine item may be, and
/// [`CoreMediaType::content_document`] is the question §5.7.2 asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CoreMediaType {
    /// `application/xhtml+xml` — an XHTML content document (§6.1).
    Xhtml,
    /// `image/svg+xml` — an SVG content document, which may also be an image.
    Svg,
    /// `text/css`.
    Css,
    /// `image/gif`.
    Gif,
    /// `image/jpeg`.
    Jpeg,
    /// `image/png`.
    Png,
    /// `image/webp`, added to the set in EPUB 3.3.
    WebP,
    /// `audio/mpeg`.
    Mp3,
    /// `audio/mp4`.
    Mp4Audio,
    /// `audio/ogg` with the Opus codec.
    OggOpus,
    /// `font/ttf`, and `application/font-sfnt` which 3.0 spelled it.
    TrueType,
    /// `font/otf`, and `application/vnd.ms-opentype`.
    OpenType,
    /// `font/woff`, and `application/font-woff`.
    Woff,
    /// `font/woff2`.
    Woff2,
    /// `application/javascript`, `text/javascript` and
    /// `application/ecmascript`.
    JavaScript,
    /// `application/x-dtbncx+xml` — the NCX, in the set for EPUB 2 compatibility
    /// and for no other reason.
    Ncx,
    /// `application/smil+xml` — media overlays.
    Smil,
    /// `application/pls+xml` — a pronunciation lexicon.
    Pls,
}

impl CoreMediaType {
    /// The core media type a manifest `media-type` names, or `None`.
    ///
    /// Compared with ASCII case folded and with parameters discarded, per RFC
    /// 2045 §5.1: a media type's type and subtype are case-insensitive, and
    /// `text/css; charset=utf-8` is `text/css`. The one entry in §3.2 that
    /// *has* a parameter is `audio/ogg; codecs=opus`, and dropping it here
    /// means an Ogg with another codec is admitted as core — recorded rather
    /// than papered over, because the alternative is parsing a parameter list
    /// to reject a container nothing in this engine decodes either way.
    #[must_use]
    pub fn from_media_type(media_type: &str) -> Option<CoreMediaType> {
        let essence = media_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        Some(match essence.as_str() {
            "application/xhtml+xml" => CoreMediaType::Xhtml,
            "image/svg+xml" => CoreMediaType::Svg,
            "text/css" => CoreMediaType::Css,
            "image/gif" => CoreMediaType::Gif,
            "image/jpeg" => CoreMediaType::Jpeg,
            "image/png" => CoreMediaType::Png,
            "image/webp" => CoreMediaType::WebP,
            "audio/mpeg" => CoreMediaType::Mp3,
            "audio/mp4" => CoreMediaType::Mp4Audio,
            "audio/ogg" => CoreMediaType::OggOpus,
            "font/ttf" | "application/font-sfnt" => CoreMediaType::TrueType,
            "font/otf" | "application/vnd.ms-opentype" => CoreMediaType::OpenType,
            "font/woff" | "application/font-woff" => CoreMediaType::Woff,
            "font/woff2" => CoreMediaType::Woff2,
            "application/javascript" | "text/javascript" | "application/ecmascript" => {
                CoreMediaType::JavaScript
            }
            "application/x-dtbncx+xml" => CoreMediaType::Ncx,
            "application/smil+xml" => CoreMediaType::Smil,
            "application/pls+xml" => CoreMediaType::Pls,
            _ => return None,
        })
    }

    /// Whether this is an EPUB content document (§6), which is the only thing a
    /// spine item may be.
    #[must_use]
    pub fn content_document(self) -> bool {
        matches!(self, CoreMediaType::Xhtml | CoreMediaType::Svg)
    }
}

// ---- Manifest item properties (§5.6.2) --------------------------------------

/// §5.6.2's manifest item properties, which is a closed vocabulary of seven.
///
/// A `properties` value outside it — calibre writes `calibre:title-page` — is
/// kept as written in [`Item::properties`] and matches none of these, which is
/// the honest answer: an unprefixed name this build does not know would be a
/// specification it has not read, and a prefixed one is somebody's extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Property {
    /// `cover-image`: the publication's cover.
    CoverImage,
    /// `mathml`: the resource contains MathML.
    MathMl,
    /// `nav`: the resource is the EPUB navigation document (§6.3).
    Nav,
    /// `remote-resources`: the resource references something outside the
    /// container.
    RemoteResources,
    /// `scripted`: the resource contains scripting.
    Scripted,
    /// `svg`: the resource embeds SVG markup.
    Svg,
    /// `switch`: the resource uses `epub:switch`.
    Switch,
}

impl Property {
    /// The property a `properties` token names, or `None`.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Property> {
        Some(match token {
            "cover-image" => Property::CoverImage,
            "mathml" => Property::MathMl,
            "nav" => Property::Nav,
            "remote-resources" => Property::RemoteResources,
            "scripted" => Property::Scripted,
            "svg" => Property::Svg,
            "switch" => Property::Switch,
            _ => return None,
        })
    }
}

// ---- What the package document said -----------------------------------------

/// One `<item>` of the manifest (§5.6.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The `id`, which is what a spine `idref` and a `fallback` name.
    pub id: String,
    /// The `href`, exactly as the package document wrote it.
    pub href: String,
    /// What that resolves to against the package document's own path, or
    /// `None` when it is not a container path at all (§4.2.3).
    ///
    /// Two facts and not one: a reference that is not a path and a path that
    /// names no entry are different defects with different names, and only the
    /// first of them can be decided without an archive.
    pub path: Option<String>,
    /// The `media-type`, as written.
    pub media_type: String,
    /// Which of §3.2's core media types that is, or `None` for a foreign
    /// resource — the one §3.5.1 requires a `fallback` for.
    pub core: Option<CoreMediaType>,
    /// The `properties` tokens, in document order and as written.
    pub properties: Vec<String>,
    /// The `fallback`, naming another item's `id` (§3.5.1).
    pub fallback: Option<String>,
}

impl Item {
    /// Whether this item carries a §5.6.2 property.
    #[must_use]
    pub fn has(&self, property: Property) -> bool {
        self.properties
            .iter()
            .any(|token| Property::from_token(token) == Some(property))
    }
}

/// One `<itemref>` of the spine (§5.7.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpineItem {
    /// The `idref`, naming a manifest item. Empty when the attribute is absent,
    /// which §5.7.2 forbids and which still produces a page.
    pub idref: String,
    /// `linear="no"`: auxiliary content, outside the primary reading order.
    ///
    /// **It still becomes a page**, and that is a decision rather than an
    /// oversight: §5.7.3 says a reading system *should* provide access to
    /// non-linear content, and a build that dropped it would renumber every
    /// page after it — gap 29's renumbering hazard, at book scale. Two books in
    /// the fetched corpus carry three between them.
    pub linear: bool,
    /// The itemref's own `properties`, which override the manifest item's
    /// rendition properties. Recorded; milestone 12's fixed-layout renditions
    /// are what read them.
    pub properties: Vec<String>,
}

/// Which of §5.5.3.1's three required elements a package document is missing,
/// and the other two things this milestone tolerates.
///
/// Five names and not one, because a book missing its language and a book whose
/// `unique-identifier` points at nothing are different defects, and milestone
/// 9's obfuscation key depends on exactly one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PackageDefect {
    /// §5.5.3.1: no `dc:title`.
    NoTitle,
    /// §5.5.3.1: no `dc:language`.
    NoLanguage,
    /// §5.5.3.1: no `dc:identifier` at all.
    NoIdentifier,
    /// §5.4.1's `unique-identifier` names no `dc:identifier` in the metadata —
    /// or the attribute is absent.
    ///
    /// **This is the one milestone 9 needs**, and it is separate from
    /// [`PackageDefect::NoIdentifier`] because a book may carry three
    /// identifiers and point at none of them. OCF §4.4.4's font obfuscation key
    /// is the SHA-1 of *the* unique identifier, so a book with this defect has
    /// no key even though it has identifiers.
    UniqueIdentifierUnresolved,
    /// §5.6.1 makes an item's `id` unique in the document. Two items with one
    /// `id` are read as the first, and this says so.
    DuplicateItemId,
}

/// A package document, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    version: PackageVersion,
    path: String,
    unique_identifier: Option<String>,
    title: Option<String>,
    language: Option<String>,
    creator: Option<String>,
    items: Vec<Item>,
    spine: Vec<SpineItem>,
    toc: Option<String>,
    right_to_left: bool,
    defects: Vec<PackageDefect>,
}

impl Package {
    /// Which version the package document declared.
    #[must_use]
    pub fn version(&self) -> PackageVersion {
        self.version
    }

    /// The container path the package document itself sits at, which is the
    /// base every `href` resolves against.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The value of the `dc:identifier` §5.4.1's `unique-identifier` names.
    ///
    /// `None` when there is no such element, which is
    /// [`PackageDefect::UniqueIdentifierUnresolved`]. **This is milestone 9's
    /// obfuscation key**: OCF §4.4.4 takes the SHA-1 of exactly this string,
    /// after §4.4.3's whitespace stripping — which is milestone 9's to do,
    /// because stripping here would hand back something no other caller asked
    /// for.
    #[must_use]
    pub fn unique_identifier(&self) -> Option<&str> {
        self.unique_identifier.as_deref()
    }

    /// The first `dc:title` in document order (§5.5.3.1).
    ///
    /// The first, because §5.5.3.2's `title-type` refinement is what picks
    /// among several and this milestone does not read refinements. Four of the
    /// twenty-six books carry more than one.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The first `dc:language` (§5.5.3.1).
    #[must_use]
    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    /// The first `dc:creator`, which is not required and is here because a
    /// synthesised document's `/Author` is a real consumer of it.
    #[must_use]
    pub fn creator(&self) -> Option<&str> {
        self.creator.as_deref()
    }

    /// Every manifest item, in document order.
    #[must_use]
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// Every spine itemref, in document order — which is reading order, and
    /// which is page order.
    #[must_use]
    pub fn spine(&self) -> &[SpineItem] {
        &self.spine
    }

    /// The `toc` attribute of `<spine>`: the `id` of the NCX, for EPUB 2.
    ///
    /// Recorded rather than read. Milestone 8's navigation is what uses it, and
    /// milestone 1 measured that one producer's EPUB 3 ships an NCX and no
    /// navigation document while the other ships both.
    #[must_use]
    pub fn toc(&self) -> Option<&str> {
        self.toc.as_deref()
    }

    /// Whether §5.7.1's `page-progression-direction` is `rtl`.
    #[must_use]
    pub fn right_to_left(&self) -> bool {
        self.right_to_left
    }

    /// Everything §5.5.3.1 and §5.6.1 tolerated.
    #[must_use]
    pub fn defects(&self) -> &[PackageDefect] {
        &self.defects
    }

    /// The manifest item with an `id`, or `None`.
    ///
    /// The **first**, which is what [`PackageDefect::DuplicateItemId`] reports
    /// on: §5.6.1 makes the `id` unique and a document that breaks it is read
    /// in document order rather than refused.
    #[must_use]
    pub fn item_by_id(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    /// The item carrying the `nav` property, which is the navigation document
    /// (§6.3).
    ///
    /// Recorded here; milestone 8's outline is its consumer.
    #[must_use]
    pub fn nav(&self) -> Option<&Item> {
        self.items.iter().find(|item| item.has(Property::Nav))
    }

    /// The item carrying the `cover-image` property.
    #[must_use]
    pub fn cover_image(&self) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.has(Property::CoverImage))
    }

    /// Every distinct §5.6.2 property in the manifest that this build does not
    /// honour, with the number of items carrying it.
    ///
    /// Deduplicated per book and counted, which is gap 31's third honesty
    /// device: *"a book with `float: left` on four hundred elements must produce
    /// **one** warning naming the property with a count"*. The same argument
    /// holds a level up — the fetched corpus has a book with seventy-one
    /// `mathml` items — and it is why this is a tally rather than a warning per
    /// page.
    ///
    /// `nav` and `cover-image` are absent from the answer because they are
    /// recorded rather than unimplemented: they name a resource this build
    /// finds and later milestones read. The other five name behaviour it does
    /// not have.
    #[must_use]
    pub fn unimplemented(&self) -> Vec<(Property, u32)> {
        let mut out: Vec<(Property, u32)> = Vec::new();
        for item in &self.items {
            for token in &item.properties {
                let Some(property) = Property::from_token(token) else {
                    continue;
                };
                if matches!(property, Property::Nav | Property::CoverImage) {
                    continue;
                }
                match out.iter_mut().find(|(p, _)| *p == property) {
                    Some((_, count)) => *count = count.saturating_add(1),
                    None => out.push((property, 1)),
                }
            }
        }
        out
    }

    /// §3.5.1's fallback chain, walked from a spine item to the content
    /// document that will actually be laid out.
    ///
    /// The chain exists because §3.5.1 lets a publication ship a resource no
    /// reading system is required to support as long as it names something that
    /// is. For a spine item the terminus must be an **EPUB content document**
    /// (§5.7.2), not merely a core media type: a PNG is core and is not a page.
    ///
    /// # The depth cap and the cycle guard are two rules
    ///
    /// A chain of twenty distinct ids is not a cycle, and a cycle of two ids is
    /// not deep; deleting either check leaves the other passing every test
    /// written for it. They answer under different names —
    /// [`FallbackDefect::TooDeep`] and [`FallbackDefect::Cyclic`] — which is
    /// gap 30's `{StaticResource}` finding applied to the one other place in
    /// this workspace where a document names a name that names a name.
    ///
    /// # Errors
    /// [`FallbackDefect`], one variant per way a chain fails to reach a content
    /// document.
    pub fn content_document<'a>(&'a self, start: &'a Item) -> Result<&'a Item, FallbackDefect> {
        let mut item = start;
        let mut seen: Vec<&str> = vec![&item.id];
        for _ in 0..MAX_EPUB_FALLBACK_DEPTH {
            if item.core.is_some_and(CoreMediaType::content_document) {
                return Ok(item);
            }
            let Some(next) = item.fallback.as_deref() else {
                return Err(FallbackDefect::NoFallback);
            };
            if seen.contains(&next) {
                return Err(FallbackDefect::Cyclic);
            }
            let Some(found) = self.item_by_id(next) else {
                return Err(FallbackDefect::Unresolved);
            };
            seen.push(&found.id);
            item = found;
        }
        Err(FallbackDefect::TooDeep)
    }
}

/// Why a §3.5.1 fallback chain did not reach an EPUB content document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FallbackDefect {
    /// The item is not a content document and names no `fallback`.
    NoFallback,
    /// A `fallback` names an `id` the manifest does not hold.
    Unresolved,
    /// The chain revisits an item it has already been through.
    Cyclic,
    /// The chain is longer than [`MAX_EPUB_FALLBACK_DEPTH`].
    TooDeep,
}

// ---- Parsing ----------------------------------------------------------------

/// Which element the reader is inside, at depth two of the package document.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    None,
    Metadata,
    Manifest,
    Spine,
}

/// Parses a package document (§5.4).
///
/// `path` is the container path the bytes were read from, which is the base
/// every `href` resolves against (§4.2.5).
///
/// The doctype mode is [`Doctype::Refuse`], and that is measured rather than
/// inherited: **zero of the twenty-six package documents in either corpus
/// carries a document type declaration**, where 100 % of one producer's XHTML
/// content documents do. Milestone 2's relaxation exists for content documents;
/// extending it to a file that has never needed it would widen the attack
/// surface for nothing, which is the same argument `container.xml` is read
/// under.
///
/// # Errors
/// [`ArchiveRefusal::UnreadablePackageDocument`] for markup that is not well
/// formed, a root that is not `package`, or a document with no manifest or no
/// spine; [`ArchiveRefusal::UnsupportedPackageVersion`] for a `version` that is
/// neither 2.0 nor 3.x; [`ArchiveRefusal::EmptySpine`] for a spine holding no
/// `itemref`; and [`ArchiveRefusal::TooLarge`] for either bound spent.
pub fn parse(bytes: &[u8], path: &str, limits: &Limits) -> Result<Package, ArchiveRefusal> {
    let source = Source::new(bytes).map_err(|_| ArchiveRefusal::UnreadablePackageDocument)?;
    let mut version: Option<PackageVersion> = None;
    let mut unique_id: Option<String> = None;
    let mut identifiers: Vec<(Option<String>, String)> = Vec::new();
    let mut titles: Vec<String> = Vec::new();
    let mut languages: Vec<String> = Vec::new();
    let mut creators: Vec<String> = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    let mut spine: Vec<SpineItem> = Vec::new();
    let mut toc: Option<String> = None;
    let mut right_to_left = false;
    let mut saw_manifest = false;
    let mut saw_spine = false;

    let mut depth = 0usize;
    let mut root = false;
    let mut section = Section::None;
    // The Dublin Core element whose characters are being gathered, and what has
    // been gathered. `Event::Text` arrives in as many pieces as the reader
    // chooses, and a `<dc:title>A &amp; B</dc:title>` is three of them.
    let mut capturing: Option<(DublinCore, Option<String>, String)> = None;

    for event in source.reader_with(&limits.xml, Doctype::Refuse) {
        match event.map_err(|_| ArchiveRefusal::UnreadablePackageDocument)? {
            Event::Text(text) | Event::Cdata(text) => {
                if let Some((_, _, gathered)) = &mut capturing {
                    gathered.push_str(&text);
                }
            }
            Event::End(_) => {
                if let Some((kind, id, gathered)) = capturing.take() {
                    let value = gathered.trim().to_owned();
                    match kind {
                        DublinCore::Identifier => identifiers.push((id, value)),
                        DublinCore::Title => titles.push(value),
                        DublinCore::Language => languages.push(value),
                        DublinCore::Creator => creators.push(value),
                    }
                }
                depth = depth.saturating_sub(1);
                if depth < 2 {
                    section = Section::None;
                }
            }
            Event::Start(element) => {
                depth += 1;
                match depth {
                    1 => {
                        root = element.namespace() == Some(OPF_NAMESPACE)
                            && element.local() == "package";
                        if !root {
                            return Err(ArchiveRefusal::UnreadablePackageDocument);
                        }
                        // §5.4.1 makes `version` required, and a package
                        // document that does not say which vocabulary it is
                        // written in is refused by the same name as one that
                        // says a vocabulary this build does not read. The
                        // consequence is identical — nothing here can know what
                        // the markup means — and guessing 3.0 is how a reader
                        // silently mis-reads a format nobody has seen.
                        version = element
                            .attribute(None, "version")
                            .and_then(PackageVersion::from_attribute);
                        if version.is_none() {
                            return Err(ArchiveRefusal::UnsupportedPackageVersion);
                        }
                        unique_id = element
                            .attribute(None, "unique-identifier")
                            .map(str::to_owned);
                    }
                    2 => {
                        section = match (element.namespace(), element.local()) {
                            (Some(OPF_NAMESPACE), "metadata") => Section::Metadata,
                            (Some(OPF_NAMESPACE), "manifest") => {
                                saw_manifest = true;
                                Section::Manifest
                            }
                            (Some(OPF_NAMESPACE), "spine") => {
                                saw_spine = true;
                                toc = element.attribute(None, "toc").map(str::to_owned);
                                right_to_left = element
                                    .attribute(None, "page-progression-direction")
                                    == Some("rtl");
                                Section::Spine
                            }
                            // `<guide>` in an EPUB 2 package, `<collection>` in
                            // an EPUB 3 one, and anything a later version adds.
                            // Ignored rather than refused, for `parse_container`'s
                            // reason: a reader that refuses what it has not been
                            // introduced to refuses the future.
                            _ => Section::None,
                        };
                    }
                    3 => match section {
                        Section::Metadata => {
                            if element.namespace() == Some(DC_NAMESPACE) {
                                if let Some(kind) = DublinCore::from_local(element.local()) {
                                    let id = element.attribute(None, "id").map(str::to_owned);
                                    capturing = Some((kind, id, String::new()));
                                }
                            }
                        }
                        Section::Manifest => {
                            if element.namespace() != Some(OPF_NAMESPACE)
                                || element.local() != "item"
                            {
                                continue;
                            }
                            if items.len() >= limits.max_manifest_items {
                                return Err(ArchiveRefusal::TooLarge);
                            }
                            let href = element.attribute(None, "href").unwrap_or_default();
                            let media_type =
                                element.attribute(None, "media-type").unwrap_or_default();
                            items.push(Item {
                                id: element.attribute(None, "id").unwrap_or_default().to_owned(),
                                href: href.to_owned(),
                                // §4.2.5, against the **referring document** —
                                // this package document — which is the general
                                // rule milestone 3 built and had no caller for.
                                path: super::ocf::resolve_reference(path, href, limits).ok(),
                                media_type: media_type.to_owned(),
                                core: CoreMediaType::from_media_type(media_type),
                                properties: tokens(element.attribute(None, "properties")),
                                fallback: element.attribute(None, "fallback").map(str::to_owned),
                            });
                        }
                        Section::Spine => {
                            if element.namespace() != Some(OPF_NAMESPACE)
                                || element.local() != "itemref"
                            {
                                continue;
                            }
                            if spine.len() >= limits.max_spine_items {
                                return Err(ArchiveRefusal::TooLarge);
                            }
                            spine.push(SpineItem {
                                idref: element
                                    .attribute(None, "idref")
                                    .unwrap_or_default()
                                    .to_owned(),
                                linear: element.attribute(None, "linear") != Some("no"),
                                properties: tokens(element.attribute(None, "properties")),
                            });
                        }
                        Section::None => {}
                    },
                    _ => {}
                }
            }
            _ => {}
        }
    }

    if !root || !saw_manifest || !saw_spine {
        return Err(ArchiveRefusal::UnreadablePackageDocument);
    }
    if spine.is_empty() {
        return Err(ArchiveRefusal::EmptySpine);
    }
    let version = version.ok_or(ArchiveRefusal::UnsupportedPackageVersion)?;

    let mut defects: Vec<PackageDefect> = Vec::new();

    if titles.is_empty() {
        defects.push(PackageDefect::NoTitle);
    }
    if languages.is_empty() {
        defects.push(PackageDefect::NoLanguage);
    }
    if identifiers.is_empty() {
        defects.push(PackageDefect::NoIdentifier);
    }
    let unique_identifier = unique_id.as_deref().and_then(|wanted| {
        identifiers
            .iter()
            .find(|(id, _)| id.as_deref() == Some(wanted))
            .map(|(_, value)| value.clone())
    });
    if unique_identifier.is_none() {
        defects.push(PackageDefect::UniqueIdentifierUnresolved);
    }
    if (1..items.len()).any(|at| items[..at].iter().any(|other| other.id == items[at].id)) {
        defects.push(PackageDefect::DuplicateItemId);
    }

    Ok(Package {
        version,
        path: path.to_owned(),
        unique_identifier,
        title: titles.into_iter().next(),
        language: languages.into_iter().next(),
        creator: creators.into_iter().next(),
        items,
        spine,
        toc,
        right_to_left,
        defects,
    })
}

/// The four Dublin Core elements this milestone reads.
#[derive(Clone, Copy, PartialEq, Eq)]
enum DublinCore {
    Identifier,
    Title,
    Language,
    Creator,
}

impl DublinCore {
    fn from_local(local: &str) -> Option<DublinCore> {
        Some(match local {
            "identifier" => DublinCore::Identifier,
            "title" => DublinCore::Title,
            "language" => DublinCore::Language,
            "creator" => DublinCore::Creator,
            _ => return None,
        })
    }
}

/// A space-separated attribute value, split into tokens.
///
/// §5.6.2 makes `properties` a space-separated list, and "space" there is XML's
/// white space — which is why this splits on [`char::is_whitespace`] rather
/// than on `' '`: pandoc writes its `properties` on one line and calibre wraps
/// nothing, but a producer that did would otherwise hand back a token with a
/// newline in it that matched nothing.
fn tokens(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}
