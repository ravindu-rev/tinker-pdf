//! The discrimination, and the OCF container behind it (gap 31, milestone 3).
//!
//! `src/epub/tests.rs` holds the arithmetic — paths, `container.xml`,
//! `encryption.xml` — as pure functions. Everything here needs a real ZIP,
//! because the claims are about what a **caller** gets: which of three formats
//! a `PK\x03\x04` is read as, what §4.3.2 warns about without refusing, which
//! refusal a broken book comes back with, and what the archive's inflation
//! budget was spent on.
//!
//! # The near misses this file is mostly made of
//!
//! Milestone 1 measured twenty-six real books and every one of them is a
//! *positive* example: `mimetype` first in both physical and directory order,
//! `META-INF` holding nothing but reserved names and one producer's extra file,
//! and a `full-path` that resolves. Not one of them can tell a correct
//! implementation from three different wrong ones. So the fixtures here are the
//! archives no producer will ever write:
//!
//! - a comic archive carrying a `META-INF/` directory record, which one of the
//!   two committed producers writes into every book it makes;
//! - a container whose physical and directory orders **disagree**, in both
//!   directions, because `header_offset == 0` and `index == 0` are two claims
//!   and a fixture that separates them one way still passes the other build;
//! - a book that holds both `EPUB/content.opf` **and**
//!   `META-INF/EPUB/content.opf`, so resolving `full-path` against the wrong
//!   base succeeds with the wrong answer rather than failing visibly;
//! - and a conforming XPS package carrying `META-INF/container.xml`, which is
//!   the only thing that can see the order of the two tests.

mod epub_support;
mod xps_support;

use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::epub::ocf::{
    is_container, Item, MimetypeDefect, Obfuscation, Ocf, OcfWarning, Reserved, CONTAINER_ITEM,
    ENCRYPTION_ITEM, MIMETYPE_ITEM, OCF_MEDIA_TYPE,
};
use tinker_pdf::epub::{self, MAX_OCF_PATH_LEN};
use tinker_pdf::{ArchiveRefusal, Document, OpenError};
use tinker_pdf_zip::{Archive, Limits as ZipLimits};

// ---- fixtures ---------------------------------------------------------------

/// The smallest package document that is a **book**: three required Dublin
/// Core elements, one manifest item and one spine itemref.
///
/// *Amended at milestone 4.* Milestone 3's version had an empty `<manifest/>`
/// and an empty `<spine/>`, because nothing then read either. That fixture is
/// now `ArchiveRefusal::EmptySpine` — a refusal that is true of it and useless
/// as a control — so every container test below would have been asserting the
/// wrong sentence about a container that is fine. The `href` is relative, which
/// is what makes the fixtures exercise §4.2.5's merge against the package
/// document's own path rather than against the container root.
const PACKAGE_OPF: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">"#,
    r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
    r#"<dc:identifier id="id">urn:uuid:0d1f3f2e-0000-4000-8000-000000000001</dc:identifier>"#,
    r#"<dc:title>The Smallest Book</dc:title><dc:language>en</dc:language>"#,
    r#"</metadata>"#,
    r#"<manifest><item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest>"#,
    r#"<spine><itemref idref="ch1"/></spine></package>"#
);

/// The one content document [`PACKAGE_OPF`] names.
const CHAPTER_XHTML: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>One</title></head>"#,
    r#"<body><p>One paragraph.</p></body></html>"#
);

/// Where [`CHAPTER_XHTML`] sits, given where the package document sits: the
/// `href` is relative, so it is a sibling.
fn chapter_path(full_path: &str) -> String {
    match full_path.rfind('/') {
        Some(at) => format!("{}ch1.xhtml", &full_path[..=at]),
        None => "ch1.xhtml".to_owned(),
    }
}

fn container_xml(full_path: &str) -> String {
    container_xml_with(&format!(
        r#"<rootfile full-path="{full_path}" media-type="application/oebps-package+xml"/>"#
    ))
}

fn container_xml_with(rootfiles: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles>{rootfiles}</rootfiles></container>"#
    )
}

/// The four entries of the smallest conforming container, in physical order.
///
/// `container.xml` is **deflated**, which is what both committed producers do
/// and is load-bearing for one test: `Archive::inflated` is the only public
/// measure of the archive's budget and a stored entry spends none of it, so a
/// read-once cache checked over a stored file asserts `0 == 0` and passes with
/// the cache deleted. Gap 30's `xps_support::archive` records the same reason.
fn book_entries(full_path: &str) -> Vec<OcfEntry> {
    vec![
        OcfEntry::stored(MIMETYPE_ITEM, OCF_MEDIA_TYPE),
        OcfEntry::deflated(CONTAINER_ITEM, container_xml(full_path).as_bytes()),
        OcfEntry::deflated(full_path, PACKAGE_OPF.as_bytes()),
        OcfEntry::deflated(&chapter_path(full_path), CHAPTER_XHTML.as_bytes()),
    ]
}

/// An archive whose central-directory order is its physical order, which is
/// what every real producer writes.
fn in_order(entries: &[OcfEntry]) -> Vec<u8> {
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(entries, &directory)
}

/// The smallest conforming book.
fn smallest_book() -> Vec<u8> {
    in_order(&book_entries("EPUB/content.opf"))
}

/// The container layer over some bytes, with the routing decision asserted on
/// the way past — a fixture that stopped being a container would otherwise be
/// measured as one.
fn ocf(bytes: &[u8]) -> Ocf<'_> {
    let archive = Archive::open(bytes, &ZipLimits::DEFAULT).expect("the fixture is a ZIP");
    assert!(
        is_container(&archive),
        "the fixture is not an OCF container"
    );
    Ocf::open(archive, &epub::Limits::DEFAULT)
}

/// What `Document::open` said, as a refusal or as a page count.
fn opened(bytes: &[u8]) -> Result<u32, ArchiveRefusal> {
    match Document::open(bytes.to_vec()) {
        Ok(doc) => Ok(doc.page_count()),
        Err(OpenError::UnsupportedArchive(why)) => Err(why),
        Err(other) => panic!("opened in a new way: {other:?}"),
    }
}

fn corpus_book(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Every committed book, the number of `<itemref>`s its spine holds, and the
/// number of pages its text needs at the default box.
///
/// Read out of the package documents by hand and written down, rather than
/// computed here: a test that recomputes the number it is checking agrees with
/// itself whatever the reader does.
///
/// **The two numbers are different since milestone 8** and both are here for
/// that reason. The spine is a property of the file; the page count is a
/// function of the box, and `pandoc-book-cover.epub`'s five itemrefs need seven
/// pages at 432 x 648. A test that kept only the second could not tell a
/// chapter that fragmented from a chapter that was duplicated.
const COMMITTED_SPINES: &[(&str, u32, u32)] = &[
    ("calibre-book-cover.epub", 5, 5),
    ("calibre-book-nocover.epub", 4, 4),
    ("pandoc-book-cover.epub", 5, 7),
    ("pandoc-book-epub2.epub", 5, 7),
    ("pandoc-book-nocover.epub", 4, 6),
    ("pandoc-plates.epub", 5, 5),
];

// ---- the discrimination -----------------------------------------------------

/// Every committed book is recognised as a book, and opens as one.
///
/// This is the defect gap 31 exists to fix, closed twice over. Before milestone
/// 3 each of these opened as however many images it happened to hold, with
/// `warnings()` empty and nothing anywhere saying a book had been lost; after
/// it, each came back as `UnpaginatedBook`. Now each is **its spine's worth of
/// pages**, and the numbers are the ones the package documents state rather
/// than a count this test would accept whatever it was.
///
/// The counts are here rather than derived from the package document, so a
/// build that stopped reading a spine and started counting something else —
/// manifest items, XHTML entries — would fail rather than agree with itself.
#[test]
fn every_committed_book_is_read_as_the_book_it_is() {
    for (name, spine, pages) in COMMITTED_SPINES {
        let bytes = corpus_book(name);
        assert_eq!(
            opened(&bytes),
            Ok(*pages),
            "{name} is not its own page count"
        );

        // And the spine is still the spine: the distinct consecutive origins of
        // those pages are the itemrefs, in the itemrefs' order. A chapter that
        // fragmented adds pages and adds no origin; a chapter that vanished
        // takes one away.
        let doc = tinker_pdf::Document::open(bytes).expect("a book");
        let report = doc.archive().expect("a report");
        let mut origins: Vec<&str> = Vec::new();
        for origin in report.pages() {
            if origins.last() != Some(&origin.name.as_str()) {
                origins.push(origin.name.as_str());
            }
        }
        assert_eq!(
            origins.len(),
            *spine as usize,
            "{name} is not its own spine: {origins:?}"
        );
    }
}

/// **A comic archive that carries a `META-INF/` directory is still a comic.**
///
/// Not hypothetical: one of milestone 1's two producers writes a deflated,
/// zero-byte `META-INF/` directory record into every book it makes, so an
/// archiver that preserves empty directories — or a repack of a book's images —
/// produces exactly this. The signature is a *file* at
/// `META-INF/container.xml`, and a discrimination written as
/// `name.starts_with("META-INF")` would take this for a book and refuse it.
#[test]
fn a_comic_that_carries_a_meta_inf_directory_is_still_a_comic() {
    let png = xps_support::rgb_png(4, 6, &xps_support::distinct_pixels(4, 6));
    let files = [
        // Deflated and empty, exactly as calibre writes it.
        OcfEntry::deflated("META-INF/", b""),
        OcfEntry::stored("page1.png", &png),
        OcfEntry::stored("page2.png", &png),
    ];
    let bytes = in_order(&files);
    assert_eq!(opened(&bytes), Ok(2), "a comic was read as something else");

    let archive = Archive::open(&bytes, &ZipLimits::DEFAULT).expect("a ZIP");
    assert!(!is_container(&archive));
}

/// And a comic carrying `META-INF` **files** that are not the container is
/// still a comic, which is a different rule from the one above.
///
/// The case fold is §4.2.3's and it is the near miss that decides it:
/// `META-INF/Container.xml` is not `META-INF/container.xml`. OPC 6.2.2.3, one
/// module over, folds ASCII case for exactly this comparison — so a reader that
/// borrowed `PartName`'s equality would route this archive into the OCF layer
/// and refuse a comic as a broken book.
#[test]
fn a_comic_carrying_meta_inf_files_that_are_not_the_container_is_still_a_comic() {
    let png = xps_support::rgb_png(4, 6, &xps_support::distinct_pixels(4, 6));
    for name in [
        "META-INF/Container.xml",
        "META-INF/CONTAINER.XML",
        "META-INF/manifest.xml",
        "META-INF/com.apple.ibooks.display-options.xml",
        "META-INF/sub/container.xml",
        "meta-inf/container.xml",
        "container.xml",
    ] {
        let files = [
            OcfEntry::deflated(name, container_xml("EPUB/content.opf").as_bytes()),
            OcfEntry::stored("page1.png", &png),
        ];
        let bytes = in_order(&files);
        assert_eq!(opened(&bytes), Ok(1), "{name} turned a comic into a book");
    }
}

/// The other direction of the same pair: a container holding **no image at
/// all** is a book, not `NoImages`.
///
/// `ArchiveRefusal::NoImages` reads *"a valid archive with no image entries"*,
/// which is a sentence about a comic archive, and milestone 1 measured it being
/// said about a novel of four chapters. Two of the six committed books have no
/// picture in them; so does this fixture, and it holds nothing but the three
/// entries OCF requires.
#[test]
fn a_container_with_no_image_at_all_is_a_book_and_not_an_archive_with_no_images() {
    assert_eq!(opened(&smallest_book()), Ok(1));
    for name in ["pandoc-book-nocover.epub", "calibre-book-nocover.epub"] {
        let pages = COMMITTED_SPINES
            .iter()
            .find(|(book, _, _)| book == &name)
            .map(|(_, _, pages)| *pages)
            .expect("a recorded page count");
        assert_eq!(opened(&corpus_book(name)), Ok(pages), "{name}");
    }
}

/// **XPS is tested first, and this is the only fixture that can see it.**
///
/// A conforming one-page package, plus `META-INF/container.xml` naming a
/// rootfile the package does not hold. Under the shipped order it is an XPS and
/// opens as one page; under the opposite order it would be an OCF container
/// whose rootfile resolves to nothing, and would come back as
/// `RootfileMissing`.
///
/// Nothing in OCF forbids a container from also carrying OPC's two items, so
/// the order is a decision rather than a consequence: ECMA-388 E.3 is a
/// specification's own recipe for recognising its format and OCF has no such
/// clause, so the format that describes how to recognise itself goes first.
#[test]
fn a_package_that_is_both_an_xps_and_a_container_is_read_as_the_xps() {
    // A page size no fallback shares: `xps.rs`'s own is US Letter, which 816 x
    // 1056 XPS units at 96 to the inch happens to be exactly, so the obvious
    // fixture would pass with the markup never read at all.
    let parts = xps_support::with(
        xps_support::one_page_package(),
        "Documents/1/Pages/1.fpage",
        &xps_support::fixed_page("400", "500"),
    );
    let parts = xps_support::before_content_types(
        parts,
        xps_support::part(CONTAINER_ITEM, &container_xml("EPUB/content.opf")),
    );
    let bytes = xps_support::archive(parts);

    // It really is both, or the test below proves nothing about an order.
    let archive = Archive::open(&bytes, &ZipLimits::DEFAULT).expect("a ZIP");
    assert!(is_container(&archive), "the fixture lost its container.xml");

    let doc = Document::open(bytes).expect("an XPS opens");
    assert_eq!(doc.page_count(), 1);
    // 400 x 500 XPS units at 96 to the inch, which is 300 x 375 points: the
    // page came out of the fixed-page markup and not out of any default.
    assert_eq!(doc.page(0).expect("page 0").size(), (300.0, 375.0));
    assert!(
        doc.archive().and_then(|r| r.xps_dialect()).is_some(),
        "it was not read as an XPS"
    );
}

// ---- OCF 3.3 section 4.3.2 --------------------------------------------------

fn mimetype_defects(bytes: &[u8]) -> Vec<MimetypeDefect> {
    ocf(bytes)
        .warnings()
        .iter()
        .filter_map(|w| match w {
            OcfWarning::Mimetype(defect) => Some(*defect),
            // `OcfWarning` is `#[non_exhaustive]` because milestone 4 adds the
            // spine's. This helper is about §4.3.2 and says so in its name; the
            // test that a conforming book warns about **nothing at all** reads
            // `warnings()` whole rather than through here.
            _ => None,
        })
        .collect()
}

/// **`header_offset == 0`, and not `index == 0`, proved in both directions.**
///
/// All twenty-six books milestone 1 measured put `mimetype` first in *both*
/// orders, so no real file can tell the two checks apart — APPNOTE puts no
/// ordering requirement on the central directory, and the writer orders that
/// table as it pleases. One fixture is not enough either: an archive where the
/// physical order is wrong and the directory order is right catches a build
/// checking `index`, and an archive where they are the other way round catches
/// a build checking `index` **in the opposite direction**, by warning about a
/// container that conforms. Both are here, and the pair is the test.
#[test]
fn the_mimetype_rule_is_physical_order_and_not_directory_order() {
    let entries = book_entries("EPUB/content.opf");

    // Physically second, first in the directory. `index == 0` says nothing is
    // wrong; §4.3.2 says the media type is not at offset 38.
    let wrong_physically = ocf_zip(
        &[
            OcfEntry::deflated(CONTAINER_ITEM, container_xml("EPUB/content.opf").as_bytes()),
            OcfEntry::stored(MIMETYPE_ITEM, OCF_MEDIA_TYPE),
            OcfEntry::deflated("EPUB/content.opf", PACKAGE_OPF.as_bytes()),
        ],
        &[1, 0, 2],
    );
    let archive = Archive::open(&wrong_physically, &ZipLimits::DEFAULT).expect("a ZIP");
    let mimetype = archive
        .entries()
        .iter()
        .find(|e| e.name == MIMETYPE_ITEM)
        .expect("a mimetype entry");
    assert_eq!(mimetype.index, 0, "the fixture does not separate the two");
    assert_ne!(mimetype.header_offset, 0);
    assert_eq!(
        mimetype_defects(&wrong_physically),
        vec![MimetypeDefect::NotFirst]
    );

    // Physically first, last in the directory. `index == 0` says the container
    // is broken; it is not.
    let wrong_in_the_directory = ocf_zip(&entries, &[1, 2, 0]);
    let archive = Archive::open(&wrong_in_the_directory, &ZipLimits::DEFAULT).expect("a ZIP");
    let mimetype = archive
        .entries()
        .iter()
        .find(|e| e.name == MIMETYPE_ITEM)
        .expect("a mimetype entry");
    assert_eq!(mimetype.index, 2, "the fixture does not separate the two");
    assert_eq!(mimetype.header_offset, 0);
    assert_eq!(mimetype_defects(&wrong_in_the_directory), vec![]);
}

/// Each of §4.3.2's five clauses warns **on its own**.
///
/// Five names rather than one boolean, for milestone 1's reason: a book whose
/// `mimetype` is deflated and a book whose `mimetype` is second are two
/// different bugs in two different producers. A single `is_valid` would make
/// four of these five untestable.
#[test]
fn each_of_the_mimetype_clauses_warns_on_its_own() {
    let opf = "EPUB/content.opf";

    // The control, first: a conforming container warns about nothing, so every
    // warning below is the one line its fixture changed.
    assert_eq!(mimetype_defects(&smallest_book()), vec![]);

    let missing = in_order(&[
        OcfEntry::deflated(CONTAINER_ITEM, container_xml(opf).as_bytes()),
        OcfEntry::deflated(opf, PACKAGE_OPF.as_bytes()),
    ]);
    assert_eq!(mimetype_defects(&missing), vec![MimetypeDefect::Missing]);

    let deflated = in_order(&[
        OcfEntry::deflated(MIMETYPE_ITEM, OCF_MEDIA_TYPE),
        OcfEntry::deflated(CONTAINER_ITEM, container_xml(opf).as_bytes()),
        OcfEntry::deflated(opf, PACKAGE_OPF.as_bytes()),
    ]);
    assert_eq!(
        mimetype_defects(&deflated),
        vec![MimetypeDefect::Compressed]
    );

    // The clause that had no accessor at all until this milestone. The area is
    // written on the **local** header only, which 4.4.11 allows: a check
    // reading the central directory's copy would find nothing here.
    let mut entries = book_entries(opf);
    entries[0] = OcfEntry::stored(MIMETYPE_ITEM, OCF_MEDIA_TYPE)
        .with_extra(&[0x55, 0x54, 0x03, 0x00, 1, 2, 3]);
    let extra = in_order(&entries);
    assert_eq!(mimetype_defects(&extra), vec![MimetypeDefect::ExtraField]);

    let wrong_bytes = in_order(&[
        OcfEntry::stored(MIMETYPE_ITEM, b"application/epub+zip\n"),
        OcfEntry::deflated(CONTAINER_ITEM, container_xml(opf).as_bytes()),
        OcfEntry::deflated(opf, PACKAGE_OPF.as_bytes()),
    ]);
    assert_eq!(
        mimetype_defects(&wrong_bytes),
        vec![MimetypeDefect::WrongMediaType]
    );
}

/// And every clause broken at once is **still a book**.
///
/// This is the asymmetry the module header argues for, asserted rather than
/// described: §4.3.2 is a `MUST` and ruling 2 degrades rather than fails, so a
/// container that breaks all of it is warned about four times and read anyway.
/// A build that refused would lose a book over a ZIP field that changes nothing
/// about its contents — and the four warnings are what stops that leniency from
/// being silence.
#[test]
fn a_container_that_breaks_every_mimetype_clause_is_still_read_as_a_book() {
    let opf = "EPUB/content.opf";
    let bytes = ocf_zip(
        &[
            OcfEntry::deflated(CONTAINER_ITEM, container_xml(opf).as_bytes()),
            OcfEntry::deflated(MIMETYPE_ITEM, b"application/zip")
                .with_extra(&[0x55, 0x54, 0x01, 0x00, 7]),
            OcfEntry::deflated(opf, PACKAGE_OPF.as_bytes()),
        ],
        &[0, 1, 2],
    );
    assert_eq!(
        mimetype_defects(&bytes),
        vec![
            MimetypeDefect::NotFirst,
            MimetypeDefect::Compressed,
            MimetypeDefect::ExtraField,
            MimetypeDefect::WrongMediaType,
        ],
        "the four clauses are not reported in the order the section states them"
    );
    // And it is still the book it is: a container that breaks all four clauses
    // is warned about four times and paginates to exactly the page a
    // conforming one does.
    assert_eq!(opened(&bytes), Ok(1));
}

// ---- container.xml, and the base its full-path resolves against -------------

/// Both producers' shapes resolve, from the real files.
///
/// One puts its package document under a directory and the other at the archive
/// root, which milestone 1 named as the reason §4.2.5's resolution is
/// load-bearing from the first real file rather than from an exotic one.
#[test]
fn the_rootfile_resolves_for_both_producers_shapes() {
    for (name, want) in [
        ("pandoc-book-cover.epub", "EPUB/content.opf"),
        ("calibre-book-cover.epub", "content.opf"),
    ] {
        let bytes = corpus_book(name);
        let mut book = ocf(&bytes);
        let rootfile = book
            .default_rendition()
            .expect("a package document")
            .clone();
        assert_eq!(rootfile.path, want, "{name}");
        assert_eq!(rootfile.declared, want, "{name}");
        assert_eq!(
            rootfile.media_type, "application/oebps-package+xml",
            "{name}"
        );
        let index = rootfile.index;
        assert_eq!(book.archive().entries()[index].name, want, "{name}");
    }
}

/// **The base is the container root, and this fixture is the only one that can
/// tell.**
///
/// A book holding `EPUB/content.opf` *and* `META-INF/EPUB/content.opf`. A build
/// resolving `full-path` against `META-INF/container.xml` — which is what every
/// other reference in the format does — finds the second, resolves happily and
/// hands back the wrong package document. On a book that holds only the first
/// it would fail visibly as `RootfileMissing`, which is why both halves are
/// asserted: the second is what a passing test looks like when the fixture
/// cannot see the defect.
#[test]
fn a_rootfile_resolves_against_the_root_even_when_the_wrong_base_would_also_resolve() {
    let opf = "EPUB/content.opf";
    let decoy = "META-INF/EPUB/content.opf";
    let bytes = in_order(&[
        OcfEntry::stored(MIMETYPE_ITEM, OCF_MEDIA_TYPE),
        OcfEntry::deflated(CONTAINER_ITEM, container_xml(opf).as_bytes()),
        OcfEntry::deflated(opf, PACKAGE_OPF.as_bytes()),
        OcfEntry::deflated(decoy, PACKAGE_OPF.as_bytes()),
    ]);
    let mut book = ocf(&bytes);
    let rootfile = book
        .default_rendition()
        .expect("a package document")
        .clone();
    assert_eq!(rootfile.path, opf);
    assert_eq!(book.archive().entries()[rootfile.index].name, opf);
    // And the decoy is genuinely there, or the fixture is the ordinary case
    // wearing a longer name.
    assert!(book.index_of(decoy).is_some());

    // The same container without the decoy: the wrong base names nothing.
    let plain = smallest_book();
    let mut book = ocf(&plain);
    assert_eq!(
        book.default_rendition().map(|r| r.path.clone()),
        Ok(opf.to_owned())
    );
    assert!(book.index_of("META-INF/EPUB/content.opf").is_none());
}

/// The default rendition is the first `<rootfile>` **whose media type is a
/// package document's**, not simply the first one.
///
/// §4.2.6.3.1 permits multiple renditions and says which of them is the default
/// in exactly those words. Every book in both corpora names one rootfile, so no
/// real file can tell "the first" from "the first package document" — and a
/// build that took the first would follow a fixed-layout rendition's manifest,
/// or a media overlay's, into a file that is not a package document at all.
#[test]
fn the_default_rendition_is_the_first_rootfile_that_is_a_package_document() {
    let opf = "EPUB/content.opf";
    let other = "other/thing.xml";
    let container = container_xml_with(&format!(
        r#"<rootfile full-path="{other}" media-type="application/x-other"/>{}"#,
        format_args!(r#"<rootfile full-path="{opf}" media-type="application/oebps-package+xml"/>"#)
    ));
    let bytes = in_order(&[
        OcfEntry::stored(MIMETYPE_ITEM, OCF_MEDIA_TYPE),
        OcfEntry::deflated(CONTAINER_ITEM, container.as_bytes()),
        // Both are present, so taking the wrong one resolves rather than
        // failing — which is the only way this fixture can catch anything.
        OcfEntry::deflated(other, b"<thing/>"),
        OcfEntry::deflated(opf, PACKAGE_OPF.as_bytes()),
    ]);
    let mut book = ocf(&bytes);
    let rootfile = book
        .default_rendition()
        .expect("a package document")
        .clone();
    assert_eq!(rootfile.path, opf);
    assert_eq!(rootfile.media_type, "application/oebps-package+xml");
    assert!(
        book.index_of(other).is_some(),
        "the decoy is not in the book"
    );
}

// ---- META-INF: recognised, and acted on ------------------------------------

/// Recognising a reserved name and **acting** on it are two rules, and this is
/// the pair that separates them.
///
/// The same unreadable bytes are put at each of the six names in turn.
/// `container.xml` and `encryption.xml` are the two this build parses, so those
/// two refuse; the other four are recognised and ignored, so those four do not.
/// A build that parsed all six would refuse a book over a `signatures.xml` it
/// has no opinion about; a build that parsed none would read ciphertext as a
/// font. Only asserting both halves can tell the two apart.
#[test]
fn the_two_reserved_names_this_build_acts_on_are_the_only_two_that_can_refuse() {
    let opf = "EPUB/content.opf";
    for (item, reserved, refuses) in [
        (CONTAINER_ITEM, Reserved::Container, true),
        (ENCRYPTION_ITEM, Reserved::Encryption, true),
        ("META-INF/manifest.xml", Reserved::Manifest, false),
        ("META-INF/metadata.xml", Reserved::Metadata, false),
        ("META-INF/rights.xml", Reserved::Rights, false),
        ("META-INF/signatures.xml", Reserved::Signatures, false),
    ] {
        let mut entries = book_entries(opf);
        if item == CONTAINER_ITEM {
            entries[1] = OcfEntry::deflated(item, b"this is not markup");
        } else {
            entries.push(OcfEntry::deflated(item, b"this is not markup"));
        }
        let bytes = in_order(&entries);
        let book = ocf(&bytes);
        assert!(
            book.items().contains(&Item::Reserved(reserved)),
            "{item} was not recognised"
        );
        let want = if refuses {
            Err(ArchiveRefusal::UnreadableContainer)
        } else {
            Ok(1)
        };
        assert_eq!(opened(&bytes), want, "{item}");
    }
}

/// **Two entries at one path: milestone 3's owed question, decided.**
///
/// §4.2.3 forbids a container from holding two files at one path. Milestone 3
/// recorded that `Ocf::index_of` takes the first and that *"milestone 4 decides
/// it, with the manifest in front of it"*. With the manifest in front of it the
/// decision is a **warning**, and the three halves of it are asserted
/// separately because a build could get any one of them without the others:
///
/// - the container **says so**, where before it said nothing;
/// - the book still **opens**, because refusing would lose it over a ZIP quirk
///   that changes nothing about the resource anybody meant;
/// - and the first entry is the one that resolves, which is the behaviour the
///   warning is *about* — a caller that does nothing with the warning gets the
///   same book it got before.
#[test]
fn two_entries_at_one_path_are_warned_about_and_the_first_one_wins() {
    let opf = "EPUB/content.opf";
    let mut entries = book_entries(opf);
    // A second chapter at the same name, holding different bytes. Legal ZIP,
    // illegal OCF, and no real book in either corpus does it.
    entries.push(OcfEntry::deflated(
        &chapter_path(opf),
        br#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Two</title></head><body><p>The impostor.</p></body></html>"#,
    ));
    let bytes = in_order(&entries);

    let book = ocf(&bytes);
    assert!(
        book.warnings().contains(&OcfWarning::DuplicatePath),
        "a container holding one path twice said nothing: {:?}",
        book.warnings()
    );
    // One warning for the container and not one per copy.
    assert_eq!(
        book.warnings()
            .iter()
            .filter(|w| **w == OcfWarning::DuplicatePath)
            .count(),
        1
    );
    let first = book.index_of(&chapter_path(opf)).expect("the chapter");
    assert_eq!(
        first, 3,
        "the first entry of that name is the one that wins"
    );

    // And it is still a book, with its own page.
    assert_eq!(opened(&bytes), Ok(1));
    let doc = Document::open(bytes.clone()).expect("a book");
    assert!(
        doc.archive()
            .expect("a report")
            .warnings()
            .contains(&tinker_pdf::ArchiveWarning::Ocf(OcfWarning::DuplicatePath)),
        "the container's warning did not reach the caller's report"
    );

    // The control: the same book without the second copy says nothing at all.
    assert!(ocf(&smallest_book()).warnings().is_empty());
}

/// A seventh file in `META-INF` is neither refused nor warned about.
///
/// Every book one of the two committed producers writes carries
/// `META-INF/com.apple.ibooks.display-options.xml`, and **zero** of the twenty
/// fetched books carries anything unreserved there at all — so a build that
/// refused an unrecognised name would refuse every pandoc book while passing
/// the entire downloaded corpus. That is milestone 1's single best argument for
/// having commissioned a corpus as well as fetched one, and this is the line it
/// bought.
#[test]
fn a_seventh_file_in_meta_inf_is_ignored_rather_than_refused() {
    let mut entries = book_entries("EPUB/content.opf");
    entries.push(OcfEntry::deflated(
        "META-INF/com.apple.ibooks.display-options.xml",
        b"<display_options/>",
    ));
    let bytes = in_order(&entries);
    let book = ocf(&bytes);
    assert!(book.items().contains(&Item::UnreservedMetaInf));
    assert!(book.warnings().is_empty(), "an ordinary book warned");
    assert_eq!(opened(&bytes), Ok(1));
}

/// The real `encryption.xml` in the fetched corpus is the shape this build had
/// to be right about, and it is not the shape a fixture would have guessed.
///
/// Both obfuscated samples redeclare the **default** namespace on
/// `<EncryptedData xmlns="http://www.w3.org/2001/04/xmlenc#">` rather than
/// binding a prefix, so a reader that looked for an `enc:` prefix — which is
/// how every published example of this file is written — would find nothing at
/// all and call the book unencrypted.
#[test]
fn an_encryption_file_written_with_a_default_namespace_is_read() {
    let opf = "EPUB/wasteland.opf";
    let encryption = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
        r#"<EncryptedData xmlns="http://www.w3.org/2001/04/xmlenc#">"#,
        r#"<EncryptionMethod Algorithm="http://www.idpf.org/2008/embedding"/>"#,
        r#"<CipherData><CipherReference URI="EPUB/OldStandard-Bold.obf.otf"/></CipherData>"#,
        r#"</EncryptedData></encryption>"#
    );
    let mut entries = book_entries(opf);
    entries.push(OcfEntry::deflated(ENCRYPTION_ITEM, encryption.as_bytes()));
    let bytes = in_order(&entries);
    let mut book = ocf(&bytes);
    let read = book.encryption().expect("obfuscation only").clone();
    assert_eq!(read.entries().len(), 1);
    assert_eq!(read.entries()[0].path, "EPUB/OldStandard-Bold.obf.otf");
    assert_eq!(read.entries()[0].algorithm, Obfuscation::Idpf);
    // An obfuscated font is not encryption, so the book opens.
    assert_eq!(opened(&bytes), Ok(1));
}

// ---- every book-level refusal, from a fixture built for it ------------------

/// One fixture per refusal, and the refusal each comes back with.
///
/// The sweep is the point: gap 31's plan says every book-level
/// `ArchiveRefusal` gets its own variant, and a variant nothing returns is a
/// name nobody can act on. The last row is the whole milestone — a conforming
/// book, refused because there is no layout engine yet, and refused by a
/// sentence that is true of it.
#[test]
fn every_book_level_refusal_is_returned_by_a_fixture_built_for_it() {
    let opf = "EPUB/content.opf";

    let unreadable = {
        let mut entries = book_entries(opf);
        entries[1] = OcfEntry::deflated(CONTAINER_ITEM, b"<container");
        in_order(&entries)
    };
    let no_rootfile = {
        let mut entries = book_entries(opf);
        entries[1] = OcfEntry::deflated(
            CONTAINER_ITEM,
            br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles/></container>"#,
        );
        in_order(&entries)
    };
    let missing_rootfile = {
        let mut entries = book_entries(opf);
        entries[1] = OcfEntry::deflated(CONTAINER_ITEM, container_xml("EPUB/nope.opf").as_bytes());
        in_order(&entries)
    };
    let encrypted = {
        let mut entries = book_entries(opf);
        entries.push(OcfEntry::deflated(
            ENCRYPTION_ITEM,
            concat!(
                r#"<encryption xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
                r#"<EncryptedData xmlns="http://www.w3.org/2001/04/xmlenc#">"#,
                r#"<EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes256-cbc"/>"#,
                r#"<CipherData><CipherReference URI="EPUB/text/chapter1.xhtml"/></CipherData>"#,
                r#"</EncryptedData></encryption>"#
            )
            .as_bytes(),
        ));
        in_order(&entries)
    };
    // Milestone 4's three, each from a package document built for it.
    let unreadable_package = with_package(opf, "<package");
    let wrong_version = with_package(
        opf,
        &PACKAGE_OPF.replace(r#"version="3.0""#, r#"version="4.0""#),
    );
    let empty_spine = with_package(
        opf,
        &PACKAGE_OPF.replace(r#"<spine><itemref idref="ch1"/></spine>"#, "<spine/>"),
    );

    for (what, bytes, want) in [
        (
            "markup that will not read",
            unreadable,
            ArchiveRefusal::UnreadableContainer,
        ),
        (
            "a container naming no package document",
            no_rootfile,
            ArchiveRefusal::UnreadableContainer,
        ),
        (
            "a rootfile the book does not hold",
            missing_rootfile,
            ArchiveRefusal::RootfileMissing,
        ),
        (
            "resources encrypted rather than obfuscated",
            encrypted,
            ArchiveRefusal::EncryptedResources,
        ),
        (
            "a package document that is not markup",
            unreadable_package,
            ArchiveRefusal::UnreadablePackageDocument,
        ),
        (
            "a package version this build does not read",
            wrong_version,
            ArchiveRefusal::UnsupportedPackageVersion,
        ),
        (
            "a spine that names no content",
            empty_spine,
            ArchiveRefusal::EmptySpine,
        ),
    ] {
        assert_eq!(opened(&bytes), Err(want), "{what}");
        // Each of them is a sentence a host can show about a book, which is the
        // difference this milestone exists to make.
        assert!(!format!("{want}").is_empty());
    }

    // And the control the list used to hold: a conforming book, which is now a
    // page rather than a refusal. Without it every row above would still pass
    // on a build that refused every EPUB there is.
    assert_eq!(opened(&smallest_book()), Ok(1));
}

/// The smallest book with its package document replaced.
fn with_package(opf: &str, package: &str) -> Vec<u8> {
    let mut entries = book_entries(opf);
    entries[2] = OcfEntry::deflated(opf, package.as_bytes());
    in_order(&entries)
}

/// [`MAX_OCF_PATH_LEN`] fires, by its own refusal, on the real length.
///
/// The `full-path` is built one byte past the cap rather than the cap being
/// lowered to meet a shorter fixture: a bound proved only against a lowered
/// copy of itself has not been proved to fire. It is charged before the merge,
/// so what refuses here is the attribute value and not the path it would have
/// become.
#[test]
fn a_content_path_past_the_length_cap_is_refused_by_name() {
    let opf = "EPUB/content.opf";

    let long = format!("{}/content.opf", "a".repeat(MAX_OCF_PATH_LEN - 12));
    assert_eq!(long.len(), MAX_OCF_PATH_LEN);
    let mut entries = book_entries(opf);
    entries[1] = OcfEntry::deflated(CONTAINER_ITEM, container_xml(&long).as_bytes());
    // At the cap it is a path, and it simply names nothing.
    assert_eq!(
        opened(&in_order(&entries)),
        Err(ArchiveRefusal::RootfileMissing)
    );

    let past = format!("a{long}");
    assert_eq!(past.len(), MAX_OCF_PATH_LEN + 1);
    let mut entries = book_entries(opf);
    entries[1] = OcfEntry::deflated(CONTAINER_ITEM, container_xml(&past).as_bytes());
    assert_eq!(opened(&in_order(&entries)), Err(ArchiveRefusal::TooLarge));
}

// ---- the archive's budget ---------------------------------------------------

/// Resolving the same file twice inflates it **once**.
///
/// `Archive::read` charges `Limits::max_inflated_total` on every call and never
/// refunds it, so a layer that read `container.xml` to route the file and again
/// to resolve its rootfile would spend it twice — and a book whose files were
/// large enough would be refused for a bound it never exceeded.
///
/// The assertion that makes this able to fail is the first one: the fixture's
/// `container.xml` is **deflated**, so the budget moves at all. Over a stored
/// file every figure here is zero and the test passes with the cache deleted,
/// which is the shape gap 30 recorded for the same measurement one format over.
#[test]
fn resolving_a_file_twice_does_not_inflate_it_twice() {
    let bytes = smallest_book();
    let mut book = ocf(&bytes);
    let before = book.archive().inflated();

    let first = book
        .default_rendition()
        .expect("a package document")
        .clone();
    let once = book.archive().inflated();
    assert!(
        once > before,
        "the fixture spends nothing, so this test asserts nothing"
    );

    let again = book
        .default_rendition()
        .expect("a package document")
        .clone();
    assert_eq!(again, first);
    assert_eq!(
        book.archive().inflated(),
        once,
        "container.xml inflated twice"
    );

    // And the cache is the entry's, not the answer's: reading the same entry
    // straight through spends nothing more either.
    let index = book.index_of(CONTAINER_ITEM).expect("a container");
    let read = book.read(index).expect("the container's bytes").len();
    assert!(read > 0);
    assert_eq!(book.archive().inflated(), once);

    // The package document has not been read at all yet, so the budget still
    // has room for exactly one more file — which is what says the figures above
    // are a spend and not a constant.
    let index = book
        .index_of("EPUB/content.opf")
        .expect("a package document");
    book.read(index).expect("the package document's bytes");
    assert!(book.archive().inflated() > once);
}
