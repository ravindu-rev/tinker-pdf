//! The spine becomes pages, at a box the caller states (gap 31, milestone 4).
//!
//! `src/epub/tests.rs` holds the package document's own arithmetic as pure
//! functions over markup. Everything here needs a real container and a real
//! `Document`, because the claims are about what a **caller** gets: how many
//! pages, how large they are, what colour they are, what the report says about
//! each, and what happens when a host passes numbers no book asked for.
//!
//! # The pairs this file exists for
//!
//! Nine milestones of this programme have found the same rule: *when a thing
//! has two independent consequences, a test for one of them is not a test.*
//! Four pairs are milestone 4's, and each is two assertions here rather than
//! one:
//!
//! - **a page count and a page box** are two claims about `OpenOptions`, and a
//!   build that paginated correctly into the wrong box would pass either test
//!   alone;
//! - **a spine item that does not resolve must produce a page *and* keep its
//!   position**, and dropping it renumbers a chapter rather than a page;
//! - **the placeholder must be grey *and* named** — gap 30's milestone 6 found
//!   that *"a page that will not read is grey, a page that draws nothing is
//!   white"* is two rules;
//! - and **`with_fonts` must warn on a book *and* stay silent on everything
//!   else**, because a warning every document trips is not a warning.

mod epub_support;
mod xps_support;

use std::sync::Arc;

use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::epub::package::{PackageDefect, Property};
use tinker_pdf::epub::{
    BookOptionDefect, SpineDefect, DEFAULT_FONT_SIZE, DEFAULT_PAGE, MAX_PAGE_SIDE,
};
use tinker_pdf::{
    ArchiveRefusal, ArchiveWarning, Document, DocumentBuilder, OpenError, OpenOptions,
    RenderOptions, SimpleFontProvider,
};

// ---- fixtures ---------------------------------------------------------------

const CONTAINER_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
    r#"</rootfiles></container>"#
);

fn chapter(title: &str, body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>{title}</title></head><body><p>{body}</p></body></html>"#
    )
}

/// A book whose package document is whatever is handed in, with three content
/// documents beside it.
fn book_with(package: &str) -> Vec<u8> {
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", package.as_bytes()),
        OcfEntry::deflated(
            "EPUB/text/ch1.xhtml",
            chapter("One", "The first chapter.").as_bytes(),
        ),
        OcfEntry::deflated(
            "EPUB/text/ch2.xhtml",
            chapter("Two", "The second chapter.").as_bytes(),
        ),
        OcfEntry::deflated(
            "EPUB/text/ch3.xhtml",
            chapter("Three", "The third chapter.").as_bytes(),
        ),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// A package document with §5.5.3.1's three required elements around a manifest
/// and a spine.
fn package(manifest: &str, spine: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-00000000000a</dc:identifier>"#,
            r#"<dc:title>A Book Of Three Chapters</dc:title>"#,
            r#"<dc:language>en</dc:language>"#,
            r#"<dc:creator>The tinker-pdf authors</dc:creator>"#,
            r#"</metadata>"#,
            r#"<manifest>{}</manifest><spine>{}</spine></package>"#
        ),
        manifest, spine
    )
}

/// Three chapters, three itemrefs: the ordinary book every test below varies.
fn three_chapters() -> Vec<u8> {
    book_with(&package(
        concat!(
            r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<item id="c3" href="text/ch3.xhtml" media-type="application/xhtml+xml"/>"#
        ),
        concat!(
            r#"<itemref idref="c1"/>"#,
            r#"<itemref idref="c2"/>"#,
            r#"<itemref idref="c3"/>"#
        ),
    ))
}

fn corpus_book(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Every `SpinePage` warning a document carries, in page order.
fn spine_defects(doc: &Document) -> Vec<(u32, SpineDefect)> {
    doc.archive()
        .expect("a book carries a report")
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::SpinePage { page, defect } => Some((*page, *defect)),
            _ => None,
        })
        .collect()
}

// ---- the page count, and the box it is a function of ------------------------

/// **`page_count()` is the spine's length**, and it is the spine's length
/// rather than the manifest's, the archive's or the XHTML entries'.
///
/// The fixture makes those four numbers different on purpose: six archive
/// entries, three manifest items, three content documents and a spine of
/// **two**, with the third chapter in the manifest and out of the reading
/// order. A build counting any of the other three would report three.
#[test]
fn the_page_count_is_the_spine_and_not_the_manifest_or_the_archive() {
    let bytes = book_with(&package(
        concat!(
            r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<item id="c3" href="text/ch3.xhtml" media-type="application/xhtml+xml"/>"#
        ),
        concat!(r#"<itemref idref="c1"/>"#, r#"<itemref idref="c3"/>"#),
    ));
    let doc = Document::open(bytes).expect("a book");
    assert_eq!(doc.page_count(), 2);

    // And the two pages are the two the spine names, in the spine's order —
    // which is not the manifest's order and not the archive's.
    let report = doc.archive().expect("a report");
    let origins: Vec<&str> = report.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(origins, ["EPUB/text/ch1.xhtml", "EPUB/text/ch3.xhtml"]);
}

/// **A page count and a page box are two claims about `OpenOptions`.**
///
/// Both are asserted at three different boxes: a build that put the right
/// number of pages at a fixed size, and one that put the caller's size on the
/// wrong number of pages, each pass one half of this.
#[test]
fn the_page_box_is_the_one_open_options_states_and_the_default_is_six_by_nine() {
    let bytes = three_chapters();

    // The default, which `open` uses and which is stated on the type.
    let doc = Document::open(bytes.clone()).expect("a book");
    assert_eq!(doc.page_count(), 3);
    for page in 0..doc.page_count() {
        assert_eq!(doc.page(page).expect("a page").size(), (432.0, 648.0));
    }
    assert_eq!(DEFAULT_PAGE, (432.0, 648.0));

    // And two boxes a caller states. The page count still does not move for
    // *this* fixture, and since milestone 8 that is a fact about the fixture
    // rather than about the build: three chapters of one sentence each fit on
    // one page at every box here. `a_chapter_takes_as_many_pages_as_its_text
    // _needs` in `tests/epub_reading.rs` is where the other half is asserted,
    // on a chapter long enough to fragment.
    for (width, height) in [(600.0, 800.0), (200.0, 1_000.0)] {
        let doc = Document::open_with(bytes.clone(), &OpenOptions::at_page(width, height))
            .expect("a book");
        assert_eq!(doc.page_count(), 3, "at {width}x{height}");
        for page in 0..doc.page_count() {
            assert_eq!(
                doc.page(page).expect("a page").size(),
                (width, height),
                "page {page} at {width}x{height}"
            );
        }
        let layout = doc
            .archive()
            .expect("a report")
            .layout()
            .expect("a reflowable document says what it was laid out at");
        assert_eq!(layout.page, (width, height));
        assert_eq!(layout.font_size, DEFAULT_FONT_SIZE);
    }
}

/// `open(bytes)` takes bytes and nothing else, and `open_with` is the same
/// function with the defaults filled in.
///
/// The plan's exit criterion says `open`'s signature is *"unchanged and
/// asserted so"*, and the assertion that can carry that is this one: the two
/// calls produce **byte-identical synthesised documents**. A build that gave
/// `open` its own defaults could drift from `OpenOptions::default()` and
/// nothing else in the suite would see it.
#[test]
fn open_takes_bytes_and_nothing_else_and_is_open_with_at_the_defaults() {
    let bytes = three_chapters();

    // `open` accepts what it always accepted: anything that becomes an
    // `Arc<[u8]>`, by value, with no second argument.
    let from_vec = Document::open(bytes.clone()).expect("a Vec");
    let from_arc = Document::open(Arc::<[u8]>::from(bytes.clone())).expect("an Arc");
    let from_slice_owned = Document::open(bytes.as_slice().to_vec()).expect("a slice's Vec");
    assert_eq!(from_vec.page_count(), from_arc.page_count());
    assert_eq!(from_vec.page_count(), from_slice_owned.page_count());

    let defaulted = Document::open_with(bytes, &OpenOptions::default()).expect("a book");
    assert_eq!(
        from_vec.archive().expect("a report").synthesised_bytes(),
        defaulted.archive().expect("a report").synthesised_bytes(),
        "open is not open_with at the defaults"
    );
    assert_eq!(
        from_vec.archive().expect("a report").layout(),
        defaulted.archive().expect("a report").layout()
    );
}

/// A page box a host could not have meant is replaced with the default and
/// **named**, and the book still opens.
///
/// Not an [`ArchiveRefusal`], because the number is the caller's rather than
/// the file's, and refusing would blame the book for the host's bug.
#[test]
fn an_unusable_page_box_is_replaced_and_named_rather_than_refused() {
    let mut options = OpenOptions::default();
    options.page = (f64::NAN, MAX_PAGE_SIDE * 2.0);
    options.font_size = -3.0;
    let doc = Document::open_with(three_chapters(), &options).expect("still a book");

    assert_eq!(doc.page_count(), 3);
    assert_eq!(doc.page(0).expect("a page").size(), DEFAULT_PAGE);

    let named: Vec<BookOptionDefect> = doc
        .archive()
        .expect("a report")
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::UnusableOption(defect) => Some(*defect),
            _ => None,
        })
        .collect();
    assert_eq!(
        named,
        [
            BookOptionDefect::PageWidth,
            BookOptionDefect::PageHeight,
            BookOptionDefect::FontSize
        ],
        "three bad numbers are three named defects"
    );
}

// ---- the placeholder, which is grey and named -------------------------------

/// **A page that will not read is grey and says why; a page that reads is
/// not.**
///
/// Two claims, and gap 30's milestone 6 is why they are two: *"a page that will
/// not read is grey, a page that draws nothing is white"*. A build that
/// paginated a book and left the pages blank passes every count in this file
/// and produces a book of blank paper reported as a success — which is the
/// failure gap 17 spent itself on.
///
/// **Amended at milestone 8**, which is where the second claim acquired a
/// second side. Until this milestone every page was grey and the test could
/// not tell a working placeholder from a build that greyed everything; now the
/// same fixture has one page of each, and asserting only the grey one would
/// pass on a build that never laid anything out.
#[test]
fn a_page_that_will_not_read_is_grey_and_a_page_that_reads_is_not() {
    // Two itemrefs: one an SVG content document, which is a named non-goal and
    // stays a placeholder at every milestone, and one an ordinary chapter.
    let doc = Document::open(book_with(&package(
        concat!(
            r#"<item id="drawing" href="text/ch2.xhtml" media-type="image/svg+xml"/>"#,
            r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>"#
        ),
        concat!(r#"<itemref idref="drawing"/>"#, r#"<itemref idref="c1"/>"#),
    )))
    .expect("a book");
    assert_eq!(doc.page_count(), 2);

    // 0xBF is the same grey the renderer paints over an image it could not
    // decode, so a page that is missing looks the same whichever level lost it.
    let greys = |page: u32| {
        let bitmap = doc
            .page(page)
            .expect("a page")
            .render(&RenderOptions::default());
        let components = bitmap.components();
        let mut seen: Vec<u8> = bitmap
            .data
            .chunks_exact(components)
            .map(|pixel| pixel[0])
            .collect();
        seen.sort_unstable();
        seen.dedup();
        seen
    };
    assert_eq!(
        greys(0),
        [0xBF],
        "the SVG page is not one flat placeholder grey"
    );
    assert_ne!(
        greys(1),
        vec![0xBF],
        "a chapter that laid out is still being painted as a placeholder"
    );
    assert!(
        !doc.page(1).expect("a page").text().plain_text().is_empty(),
        "a chapter that laid out has text on its page"
    );

    // And named, once, for the page that is a placeholder and not for the one
    // that is not.
    assert_eq!(
        spine_defects(&doc),
        [(0, SpineDefect::SvgContentDocument)],
        "a placeholder is named and a chapter is not"
    );
}

/// **A spine item that does not resolve still produces a page and keeps its
/// position.**
///
/// Two claims. The fixture puts the broken itemref in the **middle**, so a
/// build that dropped it would report two pages *and* would move the third
/// chapter to page two — and a build that kept the count while reordering
/// would pass a count-only test. Dropping it does not renumber a page, it
/// removes a chapter.
///
/// Five ways an itemref fails to resolve, each with its own name, because a
/// caller acts differently on "the package document is wrong" and "the
/// container is missing a file".
#[test]
fn an_unresolved_spine_item_still_makes_a_page_and_keeps_its_place() {
    let manifest = concat!(
        r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>"#,
        r#"<item id="c3" href="text/ch3.xhtml" media-type="application/xhtml+xml"/>"#,
        // Names an entry the container does not hold.
        r#"<item id="gone" href="text/missing.xhtml" media-type="application/xhtml+xml"/>"#,
        // Not a container path at all.
        r#"<item id="remote" href="http://example.invalid/a.xhtml" media-type="application/xhtml+xml"/>"#,
        // A core media type that is not a content document, with no fallback.
        r#"<item id="picture" href="text/ch2.xhtml" media-type="image/png"/>"#,
        // An SVG content document: a named non-goal rather than a defect.
        r#"<item id="drawing" href="text/ch2.xhtml" media-type="image/svg+xml"/>"#
    );

    for (idref, want) in [
        ("nobody", SpineDefect::IdrefUnresolved),
        ("gone", SpineDefect::ResourceMissing),
        ("remote", SpineDefect::HrefUnusable),
        (
            "picture",
            SpineDefect::Fallback(tinker_pdf::epub::package::FallbackDefect::NoFallback),
        ),
        ("drawing", SpineDefect::SvgContentDocument),
    ] {
        let spine =
            format!(r#"<itemref idref="c1"/><itemref idref="{idref}"/><itemref idref="c3"/>"#);
        let doc = Document::open(book_with(&package(manifest, &spine))).expect("a book");

        assert_eq!(doc.page_count(), 3, "{idref} lost a page");
        // **Named at page 1 and nowhere else**, which since milestone 8 is two
        // claims rather than one: the broken itemref is still a page and still
        // in the middle, and the two chapters either side of it are no longer
        // placeholders at all. A build that greyed the whole book would have
        // passed this test at milestone 4 and fails it here.
        assert_eq!(
            spine_defects(&doc),
            [(1, want)],
            "{idref} did not keep its place"
        );

        // The third chapter is still the third page, which is the half a page
        // count cannot see.
        let report = doc.archive().expect("a report");
        assert_eq!(report.pages()[2].name, "EPUB/text/ch3.xhtml", "{idref}");
        // And the broken page is a page of the book's own box rather than a
        // page of nothing.
        assert_eq!(doc.page(1).expect("a page").size(), DEFAULT_PAGE);
    }
}

/// A `<spine>` naming one manifest item three times is three pages of it.
///
/// The shape gap 30 found for `PageContent`, one format over: `epubcheck`
/// reports it as `OPF-034` and it is an error rather than a well-formedness
/// failure, so a package document may do it and a reader has to answer. Three
/// pages, because the spine is the reading order and the reading order says
/// three.
#[test]
fn one_manifest_item_named_three_times_is_three_pages() {
    let doc = Document::open(book_with(&package(
        r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>"#,
        r#"<itemref idref="c1"/><itemref idref="c1"/><itemref idref="c1"/>"#,
    )))
    .expect("a book");
    assert_eq!(doc.page_count(), 3);
    let report = doc.archive().expect("a report");
    assert!(report
        .pages()
        .iter()
        .all(|p| p.name == "EPUB/text/ch1.xhtml"));
}

// ---- what the report carries -------------------------------------------------

/// **Milestone 3's §4.3.2 warnings now have somewhere to go.**
///
/// That milestone could compute every one of the five clauses and had no report
/// to put them in, because a refused book carries none. The container here
/// breaks four of them at once — deflated, second, with an extra field and
/// holding the wrong bytes — and is read to exactly the page a conforming one
/// reaches.
#[test]
fn the_mimetype_clauses_reach_a_callers_report_now_that_a_book_has_one() {
    use tinker_pdf::epub::ocf::MimetypeDefect;

    let entries = vec![
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("mimetype", b"application/zip").with_extra(&[0x55, 0x54, 0x01, 0x00, 7]),
        OcfEntry::deflated(
            "EPUB/content.opf",
            package(
                r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>"#,
                r#"<itemref idref="c1"/>"#,
            )
            .as_bytes(),
        ),
        OcfEntry::deflated(
            "EPUB/text/ch1.xhtml",
            chapter("One", "The first chapter.").as_bytes(),
        ),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    let doc = Document::open(ocf_zip(&entries, &directory)).expect("still a book");
    assert_eq!(doc.page_count(), 1);

    let clauses: Vec<MimetypeDefect> = doc
        .archive()
        .expect("a report")
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::Ocf(tinker_pdf::epub::ocf::OcfWarning::Mimetype(defect)) => {
                Some(*defect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        clauses,
        [
            MimetypeDefect::NotFirst,
            MimetypeDefect::Compressed,
            MimetypeDefect::ExtraField,
            MimetypeDefect::WrongMediaType
        ]
    );
}

/// §5.5.3.1's metadata reaches the document it describes.
///
/// A `dc:title` that is parsed and thrown away is the failure this whole plan
/// is organised around, one level up from a CSS property. `/Title` and
/// `/Author` are where it goes, through the writer, and a caller reads it back
/// through the ordinary metadata surface rather than through anything this
/// milestone added.
#[test]
fn the_books_metadata_reaches_the_synthesised_document() {
    let doc = Document::open(three_chapters()).expect("a book");
    let metadata = doc.metadata();
    assert_eq!(metadata.title.as_deref(), Some("A Book Of Three Chapters"));
    assert_eq!(metadata.author.as_deref(), Some("The tinker-pdf authors"));

    // And a real book's, from a producer that writes both.
    let doc = Document::open(corpus_book("pandoc-book-cover.epub")).expect("a book");
    assert_eq!(
        doc.metadata().title.as_deref(),
        Some("A Short Account of Containers")
    );
    assert_eq!(
        doc.metadata().author.as_deref(),
        Some("The tinker-pdf authors")
    );
}

/// A book missing a required Dublin Core element still opens, and says which.
#[test]
fn a_missing_required_metadata_element_warns_rather_than_refuses() {
    let without_language = package(
        r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>"#,
        r#"<itemref idref="c1"/>"#,
    )
    .replace(r#"<dc:language>en</dc:language>"#, "");
    let doc = Document::open(book_with(&without_language)).expect("still a book");
    assert_eq!(doc.page_count(), 1);

    let defects: Vec<PackageDefect> = doc
        .archive()
        .expect("a report")
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::Package(defect) => Some(*defect),
            _ => None,
        })
        .collect();
    assert_eq!(defects, [PackageDefect::NoLanguage]);
}

/// §5.6.2's properties this build does not honour are counted once per book.
///
/// Deduplicated with a count rather than one warning per item, which is gap
/// 31's third honesty device: the fetched corpus holds a book with seventy-one
/// `mathml` items, and seventy-one identical warnings would destroy the
/// distinction between "it opened" and "it opened cleanly".
#[test]
fn an_unimplemented_manifest_property_is_named_once_with_its_count() {
    let doc = Document::open(book_with(&package(
        concat!(
            r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml" properties="mathml"/>"#,
            r#"<item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml" properties="mathml scripted"/>"#,
            r#"<item id="c3" href="text/ch3.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#
        ),
        concat!(
            r#"<itemref idref="c1"/>"#,
            r#"<itemref idref="c2"/>"#,
            r#"<itemref idref="c3"/>"#
        ),
    )))
    .expect("a book");

    let counted: Vec<(Property, u32)> = doc
        .archive()
        .expect("a report")
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::UnimplementedFeature { property, items } => Some((*property, *items)),
            _ => None,
        })
        .collect();
    assert_eq!(
        counted,
        [(Property::MathMl, 2), (Property::Scripted, 1)],
        "`nav` names a resource this build finds rather than behaviour it lacks"
    );
}

// ---- fonts, early and late ---------------------------------------------------

/// **`with_fonts` on a reflowable document warns by name, and on nothing else
/// does it warn at all.**
///
/// Two claims, and the second is what stops the warning from being noise: a
/// provider attached to a PDF or a comic archive can move no page, so saying so
/// there would be a sentence that is not true. On a book it cannot re-paginate
/// — the document was synthesised at `open` — and silently accepting a provider
/// that changes nothing is precisely the invisible partial implementation this
/// plan exists to prevent.
#[test]
fn a_late_font_provider_warns_on_a_book_and_on_nothing_else() {
    let provider = Arc::new(SimpleFontProvider::new(Vec::new()));

    let doc = Document::open(three_chapters())
        .expect("a book")
        .with_fonts(provider.clone());
    assert!(
        doc.archive()
            .expect("a report")
            .warnings()
            .contains(&ArchiveWarning::FontsAttachedAfterPagination),
        "a provider attached after pagination said nothing"
    );

    // A comic archive: the same call, and no warning, because nothing about it
    // was decided by a font.
    let comic = comic_archive();
    let doc = Document::open(comic).expect("a comic").with_fonts(provider);
    assert!(
        !doc.archive()
            .expect("a report")
            .warnings()
            .contains(&ArchiveWarning::FontsAttachedAfterPagination),
        "a comic warned about a provider that could not have changed it"
    );
    // And its report says it is not reflowable, which is what the warning keys
    // on.
    assert_eq!(doc.archive().expect("a report").layout(), None);
}

/// `OpenOptions::fonts` is not dropped on the floor.
///
/// It cannot yet change a book's pagination — nothing lays out text until
/// milestone 8 — so the claim that *can* be made today is that a provider
/// passed at `open` does everything `with_fonts` does. A PDF naming a font it
/// does not embed is where that is visible: without a provider it draws
/// nothing and reports `UnreadableFont`, and with one it draws.
///
/// Without this the field would be accepted, stored and ignored, which is the
/// failure the whole `OpenOptions` seam was built to prevent.
#[test]
fn a_font_provider_passed_at_open_reaches_the_render() {
    let pdf = pdf_naming_a_font_it_does_not_embed();

    let bare = Document::open(pdf.clone()).expect("a PDF");
    let bitmap = bare
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    assert!(
        bitmap
            .warnings
            .contains(&tinker_pdf::RenderWarning::UnreadableFont),
        "the control is not a document with a missing face"
    );

    let options = OpenOptions::default().with_fonts(Arc::new(SimpleFontProvider::new(boxy_font())));
    let supplied = Document::open_with(pdf, &options).expect("a PDF");
    let bitmap = supplied
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    assert!(
        !bitmap
            .warnings
            .contains(&tinker_pdf::RenderWarning::UnreadableFont),
        "the provider passed at open never reached the page: {:?}",
        bitmap.warnings
    );
    // And nothing warns, because this is the route that is on time.
    assert!(supplied.archive().is_none(), "a PDF is not synthesised");
}

// ---- a real book -------------------------------------------------------------

/// The corpus, through the door a host uses, with the page box asserted beside
/// the count — **and the count moving when the box does.**
///
/// This is the sentence [`tinker_pdf::epub::DEFAULT_PAGE`] has carried since
/// milestone 4 and could not demonstrate until milestone 8: *for a reflowable
/// book the page count is a function of that number and is not a property of
/// the file*. Three claims, and each fails on its own:
///
/// - the **spine** does not move, at either box: a chapter that fragmented adds
///   pages and adds no origin;
/// - the **page count** does move, and upward, for every book whose text needs
///   more than one page at the smaller box;
/// - and every page is the size the caller asked for.
///
/// A build that ignored `OpenOptions::page` in layout and honoured it only in
/// `/MediaBox` would pass the first and third and fail the second, which is
/// exactly the shape a page box wired to the wrong place takes.
#[test]
fn every_committed_book_paginates_to_its_spine_at_the_box_it_was_given() {
    for (name, spine, wide, narrow) in [
        ("calibre-book-cover.epub", 5u32, 5u32, 8u32),
        ("calibre-book-nocover.epub", 4, 4, 7),
        ("pandoc-book-cover.epub", 5, 7, 8),
        ("pandoc-book-epub2.epub", 5, 7, 8),
        ("pandoc-book-nocover.epub", 4, 6, 7),
        ("pandoc-plates.epub", 5, 5, 5),
    ] {
        let bytes = corpus_book(name);
        let spine_of = |doc: &Document| {
            let mut out: Vec<String> = Vec::new();
            for origin in doc.archive().expect("a report").pages() {
                if out.last() != Some(&origin.name) {
                    out.push(origin.name.clone());
                }
            }
            out
        };

        let big = Document::open(bytes.clone()).expect(name);
        assert_eq!(big.page_count(), wide, "{name} at the default box");
        assert_eq!(spine_of(&big).len(), spine as usize, "{name}");

        let small = Document::open_with(bytes, &OpenOptions::at_page(300.0, 500.0)).expect(name);
        assert_eq!(small.page_count(), narrow, "{name} at another box");
        assert_eq!(
            spine_of(&small),
            spine_of(&big),
            "{name} read a different spine at a different box"
        );
        assert!(narrow >= wide, "{name} needs fewer pages in a smaller box");
        for page in 0..small.page_count() {
            assert_eq!(
                small.page(page).expect("a page").size(),
                (300.0, 500.0),
                "{name} page {page}"
            );
        }
    }
}

/// A book this build recognises and cannot read is still refused by name, and
/// the names milestone 4 added are among them.
#[test]
fn a_book_whose_package_document_is_wrong_is_refused_by_name() {
    for (what, package_text, want) in [
        (
            "not markup",
            "<package".to_owned(),
            ArchiveRefusal::UnreadablePackageDocument,
        ),
        (
            "a version this build does not read",
            package(
                r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>"#,
                r#"<itemref idref="c1"/>"#,
            )
            .replace(r#"version="3.0""#, r#"version="4.0""#),
            ArchiveRefusal::UnsupportedPackageVersion,
        ),
        (
            "a spine naming nothing",
            package(
                r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>"#,
                "",
            ),
            ArchiveRefusal::EmptySpine,
        ),
    ] {
        assert_eq!(
            Document::open(book_with(&package_text)).err(),
            Some(OpenError::UnsupportedArchive(want)),
            "{what}"
        );
    }
}

// ---- fixtures that are not books --------------------------------------------

/// A two-page comic archive, for the half of the `with_fonts` pair that must
/// stay silent.
fn comic_archive() -> Vec<u8> {
    let png = xps_support::rgb_png(4, 6, &xps_support::distinct_pixels(4, 6));
    let entries = vec![
        OcfEntry::stored("page1.png", &png),
        OcfEntry::stored("page2.png", &png),
    ];
    ocf_zip(&entries, &[0, 1])
}

/// A one-page PDF drawing text in a base font it does not embed.
fn pdf_naming_a_font_it_does_not_embed() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F1", 24.0, 20.0, 40.0, "HELLO");
    });
    builder.finish()
}

/// A TrueType face whose every glyph from 32 upward is one filled box.
///
/// Copied in shape from `tests/substitute_fonts.rs`, which records why it is
/// synthesised rather than read from the system: the test is then identical on
/// every platform and the repository carries no font anybody has to licence.
fn boxy_font() -> Vec<u8> {
    let mut glyph = Vec::new();
    glyph.extend_from_slice(&1i16.to_be_bytes());
    glyph.extend_from_slice(&0i16.to_be_bytes());
    glyph.extend_from_slice(&0i16.to_be_bytes());
    glyph.extend_from_slice(&700i16.to_be_bytes());
    glyph.extend_from_slice(&700i16.to_be_bytes());
    glyph.extend_from_slice(&3u16.to_be_bytes());
    glyph.extend_from_slice(&0u16.to_be_bytes());
    glyph.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]);
    for dx in [0i16, 700, 0, -700] {
        glyph.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, 700, 0] {
        glyph.extend_from_slice(&dy.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());

    const FIRST: usize = 32;
    const LAST: usize = 255;
    let size = glyph.len() as u32;

    let mut glyf = Vec::with_capacity(glyph.len() * (LAST + 1 - FIRST));
    for _ in FIRST..=LAST {
        glyf.extend_from_slice(&glyph);
    }
    let mut loca = Vec::new();
    for index in 0..=LAST + 1 {
        let offset = (index.saturating_sub(FIRST)) as u32 * size;
        loca.extend_from_slice(&offset.to_be_bytes());
    }

    let tables: [(&[u8; 4], &[u8]); 3] = [(b"head", &head), (b"loca", &loca), (b"glyf", &glyf)];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);

    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}
