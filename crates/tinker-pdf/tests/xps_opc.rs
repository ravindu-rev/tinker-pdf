//! The package layer and the discrimination, against packages built for it
//! (gap 30, milestone 3).
//!
//! `tests/xps.rs` is the corpus this repository did not write, and it is what
//! says the reader works on real files. This file is its opposite half and it
//! exists because of a lesson gap 29's milestone 5 wrote down at some cost: a
//! suite of positive assertions **cannot catch a weakened check**. That
//! milestone truncated PNG's eight-byte signature to four and JPEG's three to
//! two, and every test asserting "a real PNG is recognised" passed both times,
//! because a prefix of a signature still matches every file that carries the
//! whole one.
//!
//! For a discrimination that means the fixtures worth having are the **near
//! misses**, and they are the ones no producer will ever hand you:
//!
//! - a comic archive carrying a `[Content_Types].xml`, which is still a comic;
//! - a comic archive carrying **both** of OPC's items and no XPS relationship,
//!   which is a `.docx` in shape and is still a comic;
//! - an XPS whose content-types item is damaged, which is not a comic and must
//!   not be paged as one;
//! - a fixed representation whose target is missing, external, or a part whose
//!   media type is something else;
//! - a ZIP that is neither, which is refused exactly as it was before.
//!
//! The bounds fire here too, at their shipped values, against real packages
//! built past them rather than against lowered constants — `MAX_XPS_PARTS`
//! through an 8 193-part package and `MAX_XPS_PAGES` through a fixed document
//! naming 4 097 pages.

mod cbz_support;

use cbz_support::{distinct_pixels, rgb_png, zip, Damage, ZipFile};
use tinker_pdf::cbz;
use tinker_pdf::xps::{
    self,
    opc::{self, Item, PartName},
};
use tinker_pdf::{ArchiveRefusal, ArchiveWarning, Dialect, Document, OpenError, XpsPageDefect};
use tinker_pdf_zip::{limits as zip_limits, Limits as ZipLimits, Warning as ZipWarning};

// ---- building a package -------------------------------------------------

const CONTENT_TYPES: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
    r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" />"#,
    r#"<Default Extension="fdseq" ContentType="application/vnd.ms-package.xps-fixeddocumentsequence+xml" />"#,
    r#"<Default Extension="fdoc" ContentType="application/vnd.ms-package.xps-fixeddocument+xml" />"#,
    r#"<Default Extension="fpage" ContentType="application/vnd.ms-package.xps-fixedpage+xml" />"#,
    r#"<Default Extension="png" ContentType="image/png" />"#,
    r#"</Types>"#
);

const XPS_NS: &str = "http://schemas.microsoft.com/xps/2005/06";

fn package_rels(target: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="{XPS_NS}/fixedrepresentation" Target="{target}" Id="R0" /></Relationships>"#
    )
}

fn sequence(documents: &[&str]) -> String {
    let refs: String = documents
        .iter()
        .map(|source| format!(r#"<DocumentReference Source="{source}" />"#))
        .collect();
    format!(r#"<FixedDocumentSequence xmlns="{XPS_NS}">{refs}</FixedDocumentSequence>"#)
}

fn document(pages: &[&str]) -> String {
    let refs: String = pages
        .iter()
        .map(|source| format!(r#"<PageContent Source="{source}" />"#))
        .collect();
    format!(r#"<FixedDocument xmlns="{XPS_NS}">{refs}</FixedDocument>"#)
}

fn fixed_page(width: &str, height: &str) -> String {
    format!(r#"<FixedPage xmlns="{XPS_NS}" Width="{width}" Height="{height}" />"#)
}

/// One item of a package to build.
struct Part {
    name: String,
    data: Vec<u8>,
}

fn part(name: &str, data: &str) -> Part {
    Part {
        name: name.to_string(),
        data: data.as_bytes().to_vec(),
    }
}

/// Builds an archive out of parts, deflated, with no damage.
///
/// Deflated rather than stored, and it matters for one test: `Archive::inflated`
/// is the only public measure of the archive's budget and a **stored** entry
/// spends none of it, so a read-once cache checked over a stored part asserts
/// `0 == 0` and passes with the cache deleted. Milestone 1 measured both — WPF
/// stores `_rels/.rels` and the XPS object model deflates it — so either would
/// have been a defensible fixture and only one of them can see the defect.
fn archive(parts: Vec<Part>) -> Vec<u8> {
    let files: Vec<ZipFile> = parts
        .iter()
        .map(|p| {
            // A directory record holds no bytes, and deflating none of them is
            // not what an archiver writes.
            if p.data.is_empty() {
                ZipFile::stored(&p.name, &p.data)
            } else {
                ZipFile::deflated(&p.name, &p.data)
            }
        })
        .collect();
    zip(&files, Damage::None)
}

/// The smallest package that is an XPS: one document, one page, 816 x 1056.
///
/// `[Content_Types].xml` goes **last**, as it does in all eight of milestone
/// 1's real packages — OPC 7.3.7 leaves its position unconstrained and a reader
/// that assumed it was first would be wrong on the first real file.
fn one_page_package() -> Vec<Part> {
    vec![
        part("_rels/.rels", &package_rels("/FixedDocumentSequence.fdseq")),
        part(
            "FixedDocumentSequence.fdseq",
            &sequence(&["Documents/1/FixedDocument.fdoc"]),
        ),
        part(
            "Documents/1/FixedDocument.fdoc",
            &document(&["Pages/1.fpage"]),
        ),
        part("Documents/1/Pages/1.fpage", &fixed_page("816", "1056")),
        part("[Content_Types].xml", CONTENT_TYPES),
    ]
}

/// Replaces one part's bytes in place, keeping its position.
fn with(mut parts: Vec<Part>, name: &str, data: &str) -> Vec<Part> {
    for slot in &mut parts {
        if slot.name == name {
            slot.data = data.as_bytes().to_vec();
            return parts;
        }
    }
    parts.push(part(name, data));
    parts
}

fn open(bytes: &[u8]) -> Result<Document, OpenError> {
    Document::open(bytes.to_vec())
}

fn refusal(bytes: &[u8]) -> Option<ArchiveRefusal> {
    match Document::open(bytes.to_vec()) {
        Err(OpenError::UnsupportedArchive(why)) => Some(why),
        _ => None,
    }
}

/// A package read through the OPC layer directly, for the tests that are about
/// the layer rather than about the facade.
fn opened(bytes: &[u8]) -> opc::Package<'_> {
    let archive =
        cbz::open_archive(bytes, &ZipLimits::DEFAULT).expect("the fixture is a readable ZIP");
    opc::Package::open(
        archive,
        zip_limits::MAX_ZIP_NAME_LEN,
        tinker_pdf_xml::Limits::DEFAULT,
    )
}

// ---- the happy path, so the near misses mean something ------------------

#[test]
fn a_hand_built_package_opens_as_a_fixed_document() {
    let bytes = archive(one_page_package());
    let document = open(&bytes).expect("an XPS");
    assert_eq!(document.page_count(), 1);
    assert_eq!(document.page(0).map(|p| p.size()), Some((612.0, 792.0)));
    let report = document.archive().expect("a synthesised document");
    assert_eq!(report.xps_dialect(), Some(Dialect::Xps1));
    assert_eq!(
        report
            .pages()
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["/Documents/1/Pages/1.fpage"]
    );
    assert!(report.warnings().contains(&ArchiveWarning::XpsPage {
        page: 0,
        defect: XpsPageDefect::NotDrawn,
    }));
}

// ---- the near misses ----------------------------------------------------

/// **The case that decides the order of the test.**
///
/// A comic archive that happens to carry a `[Content_Types].xml` is still a
/// comic archive, and a discrimination that was "does it contain this item
/// name" would refuse it or read it as an empty package. E.3's step 2 wants
/// *both* of OPC's items, so this one never reaches a read at all.
#[test]
fn a_comic_carrying_a_content_types_part_is_still_a_comic() {
    let png = rgb_png(4, 6, &distinct_pixels(4, 6));
    let bytes = zip(
        &[
            ZipFile::stored("001.png", &png),
            ZipFile::deflated("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ],
        Damage::None,
    );
    let document = open(&bytes).expect("a comic archive");
    assert_eq!(document.page_count(), 1);
    assert_eq!(
        document.page(0).map(|p| p.size()),
        Some((4.0, 6.0)),
        "the page is the PNG at its own pixel size, which is what a comic is"
    );
    let report = document.archive().expect("a report");
    assert_eq!(report.xps_dialect(), None, "it is not an XPS");
}

/// And with **both** of OPC's items, which is a `.docx` in shape.
///
/// The relationship is a real one — OOXML's `officeDocument` — so the package
/// is genuinely OPC and genuinely not an XPS. The plan is explicit that this
/// direction stays the fallthrough: "an XPS mis-routed to CBZ is today's
/// defect, and a CBZ mis-routed to XPS is a refusal where a document used to
/// open".
#[test]
fn an_opc_package_that_names_no_fixed_representation_is_still_a_comic() {
    let png = rgb_png(4, 6, &distinct_pixels(4, 6));
    let rels = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Target="word/document.xml" "#,
        r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" />"#,
        r#"</Relationships>"#
    );
    let bytes = zip(
        &[
            ZipFile::stored("word/media/image1.png", &png),
            ZipFile::deflated("_rels/.rels", rels.as_bytes()),
            ZipFile::deflated("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ],
        Damage::None,
    );
    let document = open(&bytes).expect("the comic path, unchanged");
    assert_eq!(document.page_count(), 1);
    assert_eq!(document.page(0).map(|p| p.size()), Some((4.0, 6.0)));
}

/// A ZIP that is neither is refused exactly as it was before this milestone.
#[test]
fn a_zip_that_is_neither_is_still_refused_as_having_no_images() {
    let bytes = zip(
        &[
            ZipFile::stored("readme.txt", b"nothing here is a page"),
            ZipFile::stored("notes.txt", b"nor here"),
        ],
        Damage::None,
    );
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::NoImages));
}

/// **The near miss the whole milestone turns on.** A damaged XPS is not a
/// comic.
///
/// Both of OPC's items are present and the fixed representation is there and
/// names the sequence, so this package is an XPS by every test that matters —
/// and its content-types item will not parse. Falling through to the comic path
/// would page whatever raster parts it holds, which is gap 30's headline defect
/// wearing a smaller hat.
#[test]
fn an_xps_whose_content_types_item_is_damaged_is_refused_by_name() {
    let png = rgb_png(4, 6, &distinct_pixels(4, 6));
    let mut parts = with(
        one_page_package(),
        "[Content_Types].xml",
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default",
    );
    parts.insert(
        0,
        Part {
            name: "Resources/one.png".to_string(),
            data: png,
        },
    );
    let bytes = archive(parts);
    assert_eq!(
        refusal(&bytes),
        Some(ArchiveRefusal::UnreadablePackage),
        "a package with a raster part and a broken content-types item must not \
         come back as a one-page comic"
    );
}

/// The other of OPC's two items, damaged.
#[test]
fn an_xps_whose_package_relationships_part_is_damaged_is_refused_by_name() {
    let bytes = archive(with(
        one_page_package(),
        "_rels/.rels",
        "<Relationships><Relationship",
    ));
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::UnreadablePackage));
}

/// A relationships part in the wrong namespace is not a relationships part.
#[test]
fn a_relationships_part_in_the_wrong_namespace_is_refused_by_name() {
    let bytes = archive(with(
        one_page_package(),
        "_rels/.rels",
        r#"<Relationships xmlns="http://example.invalid/rels"><Relationship Id="R0" Type="t" Target="/x" /></Relationships>"#,
    ));
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::UnreadablePackage));
}

#[test]
fn a_fixed_representation_naming_a_missing_part_is_refused_by_name() {
    let bytes = archive(with(
        one_page_package(),
        "_rels/.rels",
        &package_rels("/NotHere.fdseq"),
    ));
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::UnreadablePackage));
}

/// The target resolves to a part that exists and whose media type is something
/// else — a fixed *page*, not a sequence. E.3's third step is about the media
/// type and not about the extension, and this is the fixture that says so.
#[test]
fn a_fixed_representation_naming_the_wrong_media_type_is_refused_by_name() {
    let bytes = archive(with(
        one_page_package(),
        "_rels/.rels",
        &package_rels("/Documents/1/Pages/1.fpage"),
    ));
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::UnreadablePackage));
}

/// `TargetMode="External"` names something outside the package, and this engine
/// performs no I/O of any kind.
#[test]
fn an_external_fixed_representation_is_refused_by_name() {
    let external = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="{XPS_NS}/fixedrepresentation" Target="http://example.invalid/x.fdseq" TargetMode="External" Id="R0" /></Relationships>"#
    );
    let bytes = archive(with(one_page_package(), "_rels/.rels", &external));
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::UnreadablePackage));
}

/// The sniff costs **no read at all** for every comic archive there has ever
/// been.
///
/// This is the other half of "opens the archive once": one `Archive` serves
/// both the discrimination and the read, and a ZIP with neither of OPC's items
/// comes straight back with the archive's inflation budget untouched.
#[test]
fn recognising_a_comic_archive_costs_no_read() {
    let png = rgb_png(4, 6, &distinct_pixels(4, 6));
    let bytes = zip(&[ZipFile::deflated("001.png", &png)], Damage::None);
    let opened = cbz::open_archive(&bytes, &ZipLimits::DEFAULT).expect("a ZIP");
    assert_eq!(opened.inflated(), 0, "nothing is read at open");
    match xps::route(opened, &xps::Limits::DEFAULT) {
        xps::Routing::NotXps(archive) => assert_eq!(
            archive.inflated(),
            0,
            "the discrimination read nothing, and the same archive comes back"
        ),
        _ => panic!("a comic archive is not an XPS"),
    }
}

// ---- part names ---------------------------------------------------------

/// OPC 6.2.2.2, one prohibition at a time.
#[test]
fn a_name_that_is_not_a_part_name_is_not_read_as_one() {
    // The content-types item, whose brackets 7.3.7 chose *because* they violate
    // the grammar.
    assert!(PartName::from_item("[Content_Types].xml").is_none());
    // A directory record.
    assert!(PartName::from_item("Documents/").is_none());
    // An empty segment.
    assert!(PartName::from_item("Documents//1.fpage").is_none());
    // A segment ending in a dot.
    assert!(PartName::from_item("Documents./1.fpage").is_none());
    // A backslash, which APPNOTE 4.4.17.1 does not use as a separator either.
    assert!(PartName::from_item("Documents\\1.fpage").is_none());
    // A percent-encoded unreserved character: `%41` is `A`.
    assert!(PartName::from_item("Resources/%41.png").is_none());
    // A percent-encoded forward slash, and a backslash.
    assert!(PartName::from_item("Resources/a%2Fb.png").is_none());
    assert!(PartName::from_item("Resources/a%5Cb.png").is_none());
    // An absolute item name is not one: 7.3.4 removes the leading slash.
    assert!(PartName::from_item("/Documents/1.fpage").is_none());
    // And nothing at all.
    assert!(PartName::from_item("").is_none());

    // The ones that *are* part names, so the sweep above is not just a
    // validator that says no to everything.
    for item in [
        "_rels/.rels",
        "Documents/1/Pages/_rels/1.fpage.rels",
        "Resources/595c31af-dbe8-48a5-a032-c677a052f501.ODTTF",
        "a%20b/c.png",
    ] {
        assert!(PartName::from_item(item).is_some(), "{item}");
    }
}

/// 7.3.4 and 7.3.5 over a name that actually needs the percent-encoding.
///
/// RFC 3987 §3.2 decodes only what stands for a character outside US-ASCII, and
/// this is the fixture that separates the two rules: `%C3%A9` becomes `é` and
/// `%20` stays `%20`, so the round trip is exact in both directions.
#[test]
fn a_non_ascii_part_name_round_trips_through_percent_encoding() {
    let item = "Resources/caf%C3%A9%20one.png";
    let part = PartName::from_item(item).expect("a part name");
    assert_eq!(part.as_str(), "/Resources/café%20one.png");
    assert_eq!(part.item_name(), item, "item -> part -> item");
    assert_eq!(part.extension(), Some("png"));
}

/// 6.5.2's derived name, and the off-by-one-segment it is easy to get wrong.
#[test]
fn a_relationships_part_name_is_derived_by_inserting_a_segment() {
    let page = PartName::from_item("Documents/1/Pages/1.fpage").expect("a part name");
    assert_eq!(
        page.relationships_part().map(|p| p.as_str().to_string()),
        Some("/Documents/1/Pages/_rels/1.fpage.rels".to_string())
    );
    let root = PartName::from_item("FixedDocumentSequence.fdseq").expect("a part name");
    assert_eq!(
        root.relationships_part().map(|p| p.as_str().to_string()),
        Some("/_rels/FixedDocumentSequence.fdseq.rels".to_string())
    );
}

/// RFC 3986 §5.2 against a part name, in the shapes a package actually uses.
#[test]
fn a_relative_target_resolves_against_the_source_part() {
    let page = PartName::from_item("Documents/1/Pages/1.fpage").expect("a part name");
    for (reference, want) in [
        ("../../../Resources/x.png", "/Resources/x.png"),
        ("/Resources/x.png", "/Resources/x.png"),
        ("2.fpage", "/Documents/1/Pages/2.fpage"),
        ("./2.fpage", "/Documents/1/Pages/2.fpage"),
        ("../Other/2.fpage", "/Documents/1/Other/2.fpage"),
    ] {
        assert_eq!(
            page.resolve(reference).map(|p| p.as_str().to_string()),
            Some(want.to_string()),
            "{reference}"
        );
    }

    // A reference that would climb past the root cannot: §5.2.4 discards the
    // extra `..`, so the result is still inside the package rather than
    // somewhere a filesystem would have gone.
    assert_eq!(
        page.resolve("../../../../../../etc/passwd")
            .map(|p| p.as_str().to_string()),
        Some("/etc/passwd".to_string())
    );

    // And a reference that is not a part of this package at all.
    assert!(page.resolve("http://example.invalid/x.png").is_none());
    assert!(page.resolve("//example.invalid/x.png").is_none());
    assert!(page.resolve("x.png?v=2").is_none());
}

/// The package relationships part resolves against the **root**, not against
/// itself. Milestone 1 measured the XPS object model writing the target
/// relative there, where WPF writes it absolute.
#[test]
fn a_relative_package_relationship_target_resolves_against_the_root() {
    let bytes = archive(with(
        one_page_package(),
        "_rels/.rels",
        &package_rels("FixedDocumentSequence.fdseq"),
    ));
    let document = open(&bytes).expect("an XPS");
    assert_eq!(document.page_count(), 1);
}

/// 6.2.2.3, both halves, each refused by its own name.
#[test]
fn two_part_names_one_package_may_not_both_hold_are_refused_by_name() {
    // `/a` beside `/A`: one name, spelled twice.
    let equivalent = archive(with(
        with(
            one_page_package(),
            "Resources/one.txt",
            "the same name, folded",
        ),
        "Resources/ONE.txt",
        "the same name, folded",
    ));
    assert_eq!(
        refusal(&equivalent),
        Some(ArchiveRefusal::AmbiguousPartNames)
    );

    // `/segment1` beside `/segment1/segment2`: one derivable from the other.
    let derivable = archive(with(
        with(one_page_package(), "Resources/one", "a part"),
        "Resources/one/two.txt",
        "and a part derivable from it",
    ));
    assert_eq!(
        refusal(&derivable),
        Some(ArchiveRefusal::AmbiguousPartNames)
    );

    // Case-insensitively, too, which is the half a byte comparison misses.
    let folded = archive(with(
        with(one_page_package(), "Resources/One", "a part"),
        "resources/ONE/two.txt",
        "derivable under ASCII case folding",
    ));
    assert_eq!(refusal(&folded), Some(ArchiveRefusal::AmbiguousPartNames));
}

/// **The survivor the injection matrix found**, and the test that closes it.
///
/// 6.2.2.3's equivalence has two consequences and a package layer needs both: a
/// package may not hold `/a` beside `/A`, which the test above covers through
/// `validate`, and a *lookup* must find the part however the reference spells
/// it. Nothing covered the second, because `validate` compares folded `&str`s
/// in a set and never compares two `PartName`s at all — so deleting the case
/// fold from `PartName`'s `PartialEq` failed **nothing in the workspace**. That
/// is gap 29's milestone-5 lesson arriving from a third direction: every
/// assertion here was positive, and a weakened check still satisfies them all.
#[test]
fn a_part_resolves_however_a_reference_spells_its_case() {
    let bytes = archive(one_page_package());
    let package = opened(&bytes);
    for spelling in [
        "Documents/1/Pages/1.fpage",
        "documents/1/pages/1.FPAGE",
        "DOCUMENTS/1/PAGES/1.FPAGE",
    ] {
        let name = PartName::from_item(spelling).expect("a part name");
        assert!(package.has(&name), "{spelling}");
    }
    // And it is equivalence rather than leniency: a different name is still a
    // different name.
    let other = PartName::from_item("Documents/1/Pages/2.fpage").expect("a part name");
    assert!(!package.has(&other));

    // Through the facade, because that is where it costs a page: a `Source`
    // spelled in another case names the part, so what the page is missing is
    // its markup rather than its part.
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/FixedDocument.fdoc",
        &document(&["PAGES/1.FPAGE"]),
    ));
    let document = open(&bytes).expect("an XPS");
    assert!(
        document
            .archive()
            .expect("a report")
            .warnings()
            .contains(&ArchiveWarning::XpsPage {
                page: 0,
                defect: XpsPageDefect::NotDrawn,
            }),
        "the part resolved under a different spelling of its own case"
    );
}

/// A near miss for the derivability rule: `/a.txt` and `/a/b` are **not** a
/// derivable pair, because the prefix does not end at a segment boundary.
#[test]
fn a_shared_prefix_that_is_not_a_segment_boundary_is_not_derivable() {
    let bytes = archive(with(
        with(one_page_package(), "Resources/one.txt", "a part"),
        "Resources/one/two.txt",
        "a part whose parent is `one`, not `one.txt`",
    ));
    assert!(
        open(&bytes).is_ok(),
        "`/Resources/one.txt` is not derivable from `/Resources/one/two.txt`"
    );
}

#[test]
fn an_item_that_is_not_a_part_name_is_refused_by_name() {
    let bytes = archive(with(
        one_page_package(),
        "Resources/a%41b.png",
        "a percent-encoded unreserved character",
    ));
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::InvalidPartName));
}

/// Directory records are not parts, and a package carrying them opens.
///
/// Neither Microsoft serialiser writes one, so **the corpus cannot see this**
/// and it is the sort of thing a general-purpose archiver does on every file it
/// makes: a reader that took `Documents/` for a part name would refuse it as
/// `InvalidPartName` — the last segment is empty — and every package repacked
/// through an ordinary ZIP tool would stop opening. Named here for that reason
/// rather than left to the eight files that happen not to have any.
#[test]
fn directory_records_are_not_parts() {
    let mut parts = one_page_package();
    for name in ["Documents/", "Documents/1/", "Documents/1/Pages/", "_rels/"] {
        parts.insert(0, part(name, ""));
    }
    let bytes = archive(parts);
    let package = opened(&bytes);
    assert_eq!(
        package
            .items()
            .iter()
            .filter(|item| **item == Item::Directory)
            .count(),
        4
    );
    // And `/Documents` is not derivable from `/Documents/1/Pages/1.fpage`,
    // because the directory record is not a part in the first place.
    let document = open(&bytes).expect("directory records are not parts");
    assert_eq!(document.page_count(), 1);
}

/// OPC 7.2.4's interleaving, recognised and refused rather than half-assembled.
///
/// A reader that did nothing would see parts whose names end in `.piece` and no
/// part with the name they belong to — which is a document silently missing
/// whatever the pieces carried.
#[test]
fn an_interleaved_package_is_refused_by_name() {
    for piece in [
        "Documents/1/Pages/1.fpage/[0].piece",
        "Big.bin/[3].last.piece",
    ] {
        let bytes = archive(with(one_page_package(), piece, "a piece of a part"));
        assert_eq!(
            refusal(&bytes),
            Some(ArchiveRefusal::Interleaved),
            "{piece}"
        );
    }
}

// ---- media types --------------------------------------------------------

/// 7.2.3.5's ordered algorithm, in both of its steps and both case rules.
#[test]
fn an_override_beats_a_default_and_both_compare_case_insensitively() {
    let types = concat!(
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" />"#,
        r#"<Default Extension="fdseq" ContentType="application/vnd.ms-package.xps-fixeddocumentsequence+xml" />"#,
        r#"<Default Extension="fdoc" ContentType="application/vnd.ms-package.xps-fixeddocument+xml" />"#,
        r#"<Default Extension="fpage" ContentType="application/vnd.ms-package.xps-fixedpage+xml" />"#,
        r#"<Default Extension="ODTTF" ContentType="application/vnd.ms-package.obfuscated-opentype" />"#,
        r#"<Override PartName="/Resources/PLAIN.ODTTF" ContentType="font/otf" />"#,
        r#"<Override PartName="/Documents/1/Pages/1.fpage" ContentType="application/xml" />"#,
        r#"</Types>"#
    );
    let parts = with(
        with(
            with(one_page_package(), "[Content_Types].xml", types),
            "Resources/a.odttf",
            "lower case against an upper-case Default",
        ),
        "Resources/plain.odttf",
        "lower case against a mixed-case Override",
    );
    let bytes = archive(parts);
    let mut package = opened(&bytes);

    let odttf = PartName::from_item("Resources/a.odttf").expect("a part name");
    assert_eq!(
        package.media_type(&odttf).expect("content types"),
        Some("application/vnd.ms-package.obfuscated-opentype"),
        "step 2: `Extension=\"ODTTF\"` matches `.odttf`"
    );

    let overridden = PartName::from_item("Resources/plain.odttf").expect("a part name");
    assert_eq!(
        package.media_type(&overridden).expect("content types"),
        Some("font/otf"),
        "step 1 beats step 2, and `PartName=\"/Resources/PLAIN.ODTTF\"` matches"
    );

    let page = PartName::from_item("Documents/1/Pages/1.fpage").expect("a part name");
    assert_eq!(
        package.media_type(&page).expect("content types"),
        Some("application/xml"),
        "an Override beats the Default that would otherwise apply"
    );

    // Step 3: a part whose last segment carries no dot has no media type, and
    // that is an answer rather than a failure.
    let bare = PartName::from_item("Resources/no-extension").expect("a part name");
    assert_eq!(package.media_type(&bare).expect("content types"), None);
}

/// 7.2.3.2 forbids two `Default`s for one extension and two `Override`s for one
/// part. Refused rather than resolved to whichever came first, and **both
/// halves**, because they are two rules that share a sentence — deleting either
/// leaves the other passing every test written for it.
#[test]
fn a_content_types_item_that_declares_one_thing_twice_is_refused_by_name() {
    let twice_default = concat!(
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="fdseq" ContentType="application/vnd.ms-package.xps-fixeddocumentsequence+xml" />"#,
        r#"<Default Extension="FDSEQ" ContentType="text/plain" />"#,
        r#"</Types>"#
    );
    let bytes = archive(with(
        one_page_package(),
        "[Content_Types].xml",
        twice_default,
    ));
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::UnreadablePackage));

    let twice_override = concat!(
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="fdseq" ContentType="application/vnd.ms-package.xps-fixeddocumentsequence+xml" />"#,
        r#"<Override PartName="/Documents/1/Pages/1.fpage" ContentType="application/xml" />"#,
        r#"<Override PartName="/DOCUMENTS/1/PAGES/1.FPAGE" ContentType="text/plain" />"#,
        r#"</Types>"#
    );
    let bytes = archive(with(
        one_page_package(),
        "[Content_Types].xml",
        twice_override,
    ));
    assert_eq!(
        refusal(&bytes),
        Some(ArchiveRefusal::UnreadablePackage),
        "and the duplicate is found under 6.2.2.3's case fold, not by spelling"
    );
}

/// 6.5.2 makes a relationship `Id` unique within its part.
#[test]
fn a_relationships_part_that_repeats_an_id_is_refused_by_name() {
    let twice = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Type="{XPS_NS}/fixedrepresentation" Target="/FixedDocumentSequence.fdseq" Id="R0" /><Relationship Type="{XPS_NS}/required-resource" Target="/Resources/x.png" Id="R0" /></Relationships>"#
    );
    let bytes = archive(with(one_page_package(), "_rels/.rels", &twice));
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::UnreadablePackage));
}

// ---- reads happen once --------------------------------------------------

/// The read-once cache, measured through the archive's own budget.
///
/// `Archive::read` charges [`tinker_pdf_zip::limits::MAX_ZIP_INFLATED`] on
/// every call and never refunds it, so reading `[Content_Types].xml` once to
/// decide the format and again to use it would spend it twice. This is a
/// correctness requirement rather than an optimisation, and it is checkable
/// because gap 29 made `inflated()` public for exactly this.
#[test]
fn resolving_a_part_twice_does_not_inflate_it_twice() {
    let bytes = archive(one_page_package());
    let mut package = opened(&bytes);
    let page = PartName::from_item("Documents/1/Pages/1.fpage").expect("a part name");

    assert_eq!(package.archive().inflated(), 0);
    let first = package.read_part(&page).expect("the page part").len();
    let after_one = package.archive().inflated();
    assert!(after_one > 0, "a deflated part costs its inflated length");

    let second = package.read_part(&page).expect("the page part again").len();
    assert_eq!(first, second);
    assert_eq!(
        package.archive().inflated(),
        after_one,
        "the second resolution of the same part spends nothing"
    );

    // And the whole open does the same: the content-types item is read to
    // decide the format and used again to resolve four media types.
    let mut whole = opened(&bytes);
    let types = PartName::from_item("FixedDocumentSequence.fdseq").expect("a part name");
    whole.media_type(&types).expect("content types");
    let once = whole.archive().inflated();
    for _ in 0..8 {
        whole.media_type(&types).expect("content types");
    }
    assert_eq!(whole.archive().inflated(), once);
}

// ---- a truncated name, in both directions -------------------------------

/// `Warning::NameTruncated` makes **that** part unresolvable, and leaves every
/// other part alone.
///
/// Both directions, and the second is the one that matters: gap 29's own
/// milestone-6 survivor was an `Archive::warnings` loop that nothing asserted
/// was there, so deleting it failed nothing in the workspace. A test that only
/// checked "the long name is unresolvable" would pass on a package layer that
/// resolved *nothing*.
#[test]
fn a_truncated_part_name_is_unresolvable_and_an_untruncated_one_is_not() {
    let long = format!("Resources/{}.bin", "a".repeat(2_000));
    let parts = with(
        with(one_page_package(), &long, "a name past the archive's cap"),
        "Resources/short.bin",
        "an ordinary name in the same package",
    );
    let bytes = archive(parts);

    let package = opened(&bytes);
    assert!(
        package
            .archive()
            .warnings()
            .contains(&ZipWarning::NameTruncated),
        "the archive reader truncated the name and said so"
    );

    // Direction one: the name that came back is not the name the package holds,
    // so nothing resolves to it — not the truncated spelling and not the
    // original.
    let truncated = package
        .archive()
        .entries()
        .iter()
        .find(|e| e.name.starts_with("Resources/aaa"))
        .map(|e| e.name.clone())
        .expect("the long entry survived, truncated");
    assert!(truncated.len() <= zip_limits::MAX_ZIP_NAME_LEN);
    assert!(truncated.len() < long.len(), "it really was truncated");
    for spelling in [truncated.as_str(), long.as_str()] {
        let name = PartName::from_item(spelling).expect("still a part name in shape");
        assert!(
            !package.has(&name),
            "a truncated name must resolve to nothing: {spelling}"
        );
    }
    assert!(package.items().contains(&Item::Unresolvable));

    // Direction two: every other part in the same package still resolves.
    for item in [
        "Resources/short.bin",
        "Documents/1/Pages/1.fpage",
        "_rels/.rels",
    ] {
        let name = PartName::from_item(item).expect("a part name");
        assert!(package.has(&name), "{item} should still resolve");
    }

    // And the warning reaches the report, rather than being read into a loop
    // nothing checks.
    let document = open(&bytes).expect("the rest of the package is fine");
    let report = document.archive().expect("a report");
    assert!(
        report
            .warnings()
            .contains(&ArchiveWarning::Zip(ZipWarning::NameTruncated)),
        "the report carries what the archive reader tolerated"
    );
}

/// The other half of the same rule: a package the archive reader did **not**
/// truncate resolves every name, including one at the cap.
#[test]
fn an_untruncated_package_resolves_every_name() {
    let at_cap = format!(
        "Resources/{}.bin",
        "a".repeat(zip_limits::MAX_ZIP_NAME_LEN - "Resources/.bin".len())
    );
    assert_eq!(at_cap.len(), zip_limits::MAX_ZIP_NAME_LEN);
    let bytes = archive(with(one_page_package(), &at_cap, "exactly at the cap"));
    let package = opened(&bytes);
    assert_eq!(
        package.archive().warnings(),
        &[],
        "a name of exactly the cap is not truncated"
    );
    let name = PartName::from_item(&at_cap).expect("a part name");
    assert!(package.has(&name), "and it resolves");
}

// ---- the spine degrades rather than dropping pages ----------------------

/// A `PageContent` whose `Source` does not resolve **still produces a page**.
///
/// Dropping it would produce a document with no gap anywhere in it: three
/// references, two resolved, and a two-page book that looks complete. That is
/// gap 29's renumbering hazard arriving through markup rather than filenames.
#[test]
fn a_page_content_that_does_not_resolve_keeps_its_number() {
    let bytes = archive(with(
        with(
            one_page_package(),
            "Documents/1/FixedDocument.fdoc",
            &document(&["Pages/1.fpage", "Pages/missing.fpage", "Pages/3.fpage"]),
        ),
        "Documents/1/Pages/3.fpage",
        &fixed_page("816", "1056"),
    ));
    let document = open(&bytes).expect("an XPS");
    assert_eq!(document.page_count(), 3, "every reference produces a page");
    let report = document.archive().expect("a report");
    assert!(report.warnings().contains(&ArchiveWarning::XpsPage {
        page: 1,
        defect: XpsPageDefect::SourceUnresolved,
    }));
    // And the page that could not be found takes the book's own size rather
    // than paper, so a reader can page through it.
    assert_eq!(document.page(1).map(|p| p.size()), Some((612.0, 792.0)));
    assert_eq!(
        report.pages().get(1).map(|p| p.name.as_str()),
        Some("/Documents/1/Pages/missing.fpage")
    );
}

/// Only a **direct child** of a `FixedDocument` is a page.
///
/// 12.3.2's `PageContent.LinkTargets` is a child of a `PageContent`, so a
/// reader that counted every `PageContent` descendant rather than every child
/// would find one page too many in a document that uses link targets — and
/// would find it as an extra page at the end, which is the shape that looks
/// like the producer's fault.
#[test]
fn only_a_direct_child_of_a_fixed_document_is_a_page() {
    let nested = format!(
        r#"<FixedDocument xmlns="{XPS_NS}"><PageContent Source="Pages/1.fpage"><PageContent.LinkTargets><PageContent Source="Pages/1.fpage" /></PageContent.LinkTargets></PageContent></FixedDocument>"#
    );
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/FixedDocument.fdoc",
        &nested,
    ));
    let document = open(&bytes).expect("an XPS");
    assert_eq!(
        document.page_count(),
        1,
        "the nested reference is not a page"
    );
}

/// A `DocumentReference` that does not resolve leaves one visible page behind
/// rather than nothing, because it takes an unknown number of pages with it.
#[test]
fn a_document_reference_that_does_not_resolve_leaves_a_page() {
    let bytes = archive(with(
        one_page_package(),
        "FixedDocumentSequence.fdseq",
        &sequence(&["Documents/1/FixedDocument.fdoc", "Documents/2/Gone.fdoc"]),
    ));
    let document = open(&bytes).expect("an XPS");
    assert_eq!(document.page_count(), 2);
    let report = document.archive().expect("a report");
    assert!(report.warnings().contains(&ArchiveWarning::XpsPage {
        page: 1,
        defect: XpsPageDefect::DocumentUnresolved,
    }));
}

/// A fixed page with no usable size is a page at the book's size, named.
#[test]
fn a_fixed_page_with_no_usable_size_takes_the_books_size() {
    // The last is `MAX_PAGE_UNITS`, which is a sanity check on a number the file
    // chose rather than a resource bound — it allocates nothing and a page past
    // it degrades rather than refusing, which is why it is here and not in
    // `bounds_ledger.rs`.
    for (width, height) in [
        ("", "1056"),
        ("nonsense", "1056"),
        ("816", "-3"),
        ("0", "0"),
        ("1e9", "1056"),
    ] {
        let bytes = archive(with(
            with(
                one_page_package(),
                "Documents/1/FixedDocument.fdoc",
                &document(&["Pages/1.fpage", "Pages/2.fpage"]),
            ),
            "Documents/1/Pages/2.fpage",
            &fixed_page(width, height),
        ));
        let document = open(&bytes).unwrap_or_else(|e| panic!("{width}x{height}: {e:?}"));
        assert_eq!(document.page_count(), 2);
        assert_eq!(
            document.page(1).map(|p| p.size()),
            Some((612.0, 792.0)),
            "{width}x{height}: the book's own size"
        );
        let report = document.archive().expect("a report");
        assert!(
            report.warnings().contains(&ArchiveWarning::XpsPage {
                page: 1,
                defect: XpsPageDefect::SizeUnusable,
            }),
            "{width}x{height}: named rather than silently defaulted"
        );
    }
}

/// A fixed page part that is not a `FixedPage` is a page, named.
#[test]
fn a_fixed_page_part_that_is_not_one_is_a_named_placeholder() {
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        r#"<NotAPage xmlns="http://schemas.microsoft.com/xps/2005/06" />"#,
    ));
    let document = open(&bytes).expect("an XPS with one unreadable page");
    assert_eq!(document.page_count(), 1);
    let report = document.archive().expect("a report");
    assert!(report.warnings().contains(&ArchiveWarning::XpsPage {
        page: 0,
        defect: XpsPageDefect::Unreadable,
    }));
    assert_eq!(
        document.page(0).map(|p| p.size()),
        Some((612.0, 792.0)),
        "US Letter, because no page in this book stated a size"
    );
}

/// A sequence that names no document has no page to degrade *to*.
#[test]
fn a_fixed_document_sequence_naming_no_document_is_refused_by_name() {
    let empty = archive(with(
        one_page_package(),
        "FixedDocumentSequence.fdseq",
        &sequence(&[]),
    ));
    assert_eq!(refusal(&empty), Some(ArchiveRefusal::NoFixedPages));

    let broken = archive(with(
        one_page_package(),
        "FixedDocumentSequence.fdseq",
        "<FixedDocumentSequence",
    ));
    assert_eq!(refusal(&broken), Some(ArchiveRefusal::NoFixedPages));

    let no_pages = archive(with(
        one_page_package(),
        "Documents/1/FixedDocument.fdoc",
        &document(&[]),
    ));
    assert_eq!(refusal(&no_pages), Some(ArchiveRefusal::NoFixedPages));
}

/// Both dialects' namespaces are read on the markup as well as on the
/// relationship type, and a package mixing them is accepted.
///
/// Nothing in either specification makes mixing an error, and a package
/// assembled by merging two documents is how it would happen.
#[test]
fn a_package_mixing_the_two_dialects_is_accepted() {
    const OXPS: &str = "http://schemas.openxps.org/oxps/v1.0";
    let bytes = archive(with(
        with(
            one_page_package(),
            "Documents/1/FixedDocument.fdoc",
            &format!(
                r#"<FixedDocument xmlns="{OXPS}"><PageContent Source="Pages/1.fpage" /></FixedDocument>"#
            ),
        ),
        "Documents/1/Pages/1.fpage",
        &format!(r#"<FixedPage xmlns="{OXPS}" Width="816" Height="1056" />"#),
    ));
    let document = open(&bytes).expect("one vocabulary, two spellings");
    assert_eq!(document.page_count(), 1);
    assert_eq!(
        document.archive().and_then(|r| r.xps_dialect()),
        Some(Dialect::Xps1),
        "the dialect reported is the one the fixed representation was spelled in"
    );
}

/// Markup in neither dialect's namespace is not this format.
#[test]
fn markup_in_a_third_namespace_is_not_read_as_a_fixed_page() {
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        r#"<FixedPage xmlns="http://example.invalid/xps" Width="816" Height="1056" />"#,
    ));
    let document = open(&bytes).expect("the page still exists");
    let report = document.archive().expect("a report");
    assert!(report.warnings().contains(&ArchiveWarning::XpsPage {
        page: 0,
        defect: XpsPageDefect::Unreadable,
    }));
}

// ---- bounds -------------------------------------------------------------

/// `MAX_XPS_PARTS`, fired by its own refusal against a package built past it.
///
/// The real 8 193-part package rather than a lowered constant, because a cap
/// proved only against a lowered copy of itself has not been proved to fire —
/// gap 29's rule for `MAX_CBZ_PAGES`, and gap 18a milestone 8's failure is what
/// both of them are about.
#[test]
fn a_package_past_the_part_cap_is_refused_by_name() {
    let mut parts = one_page_package();
    let already = parts.len() - 1; // the content-types item is not a part
    for n in already..=xps::MAX_XPS_PARTS {
        parts.insert(parts.len() - 1, part(&format!("Filler/{n:06}.bin"), "x"));
    }
    let bytes = archive(parts);
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::TooLarge));

    // And one part fewer opens, so the cap is what stopped it rather than
    // something else about a large package.
    let mut parts = one_page_package();
    let already = parts.len() - 1;
    for n in already..xps::MAX_XPS_PARTS {
        parts.insert(parts.len() - 1, part(&format!("Filler/{n:06}.bin"), "x"));
    }
    let bytes = archive(parts);
    assert!(open(&bytes).is_ok(), "exactly at the cap is allowed");
}

/// `MAX_XPS_PAGES`, fired the same way.
///
/// The fixture is small on purpose: 4 097 `PageContent` elements naming **one**
/// part between them, which is why the page cap is not the part cap and why
/// this milestone carries both.
#[test]
fn a_page_count_past_the_xps_cap_is_refused_by_name() {
    let names: Vec<&str> = vec!["Pages/1.fpage"; xps::MAX_XPS_PAGES + 1];
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/FixedDocument.fdoc",
        &document(&names),
    ));
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::TooLarge));

    let names: Vec<&str> = vec!["Pages/1.fpage"; xps::MAX_XPS_PAGES];
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/FixedDocument.fdoc",
        &document(&names),
    ));
    let document = open(&bytes).expect("exactly at the cap is allowed");
    assert_eq!(document.page_count() as usize, xps::MAX_XPS_PAGES);
}

/// The relations that make both caps caps, checked where the constants are.
///
/// `const` blocks, so a build that broke either **does not compile** — gap 29's
/// milestone-5 rung, which is stronger than a test run. The test remains so the
/// relation has a name in the suite and so the reason travels with it.
#[test]
fn the_xps_caps_sit_below_the_caps_that_would_otherwise_stop_first() {
    const {
        assert!(
            xps::MAX_XPS_PAGES < xps::MAX_XPS_PARTS,
            "the page cap is at or above the part cap"
        );
        assert!(
            xps::MAX_XPS_PARTS < zip_limits::MAX_ZIP_ENTRIES,
            "the part cap is at or above the entry cap, so it could never fire"
        );
        // And above the yardstick, so neither refuses a real document: a
        // 200-page fixed document is 505 parts and 200 pages.
        assert!(xps::MAX_XPS_PARTS > 505);
        assert!(xps::MAX_XPS_PAGES > 200);
    }
}

/// The synthesised document fits inside what was charged for it.
///
/// `PAGE_OVERHEAD` is charged per page before anything is built, and a change
/// to the writer that made a page cost more would overrun `MAX_SYNTHESISED_PDF`
/// quietly. Measured rather than assumed, which is what gap 29's twin of this
/// test does for a comic.
#[test]
fn the_synthesised_document_fits_inside_what_was_charged_for_it() {
    for pages in [1usize, 3, 64] {
        let names: Vec<String> = (0..pages).map(|_| "Pages/1.fpage".to_string()).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let bytes = archive(with(
            one_page_package(),
            "Documents/1/FixedDocument.fdoc",
            &document(&refs),
        ));
        let document = open(&bytes).expect("an XPS");
        let report = document.archive().expect("a report");
        let charged = cbz::DOCUMENT_OVERHEAD + pages * cbz::PAGE_OVERHEAD;
        assert!(
            report.synthesised_bytes() <= charged,
            "{pages} pages: {} bytes against a charge of {charged}",
            report.synthesised_bytes()
        );
    }
}
