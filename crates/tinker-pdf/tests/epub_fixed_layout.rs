//! Fixed-layout renditions: EPUB 3.3 §8.2 and EPUB RS 3.3 §8.1 (gap 31,
//! milestone 12).
//!
//! # Two pagination rules, and both have to hold
//!
//! Milestone 4 built this reader on one premise, written into
//! [`tinker_pdf::OpenOptions`]'s own documentation: *"for a reflowable EPUB the
//! page count is a function of these numbers and is not a property of the
//! file"*. EPUB Reading Systems 3.3 §8.1 states the other one: a pre-paginated
//! content document is **"exactly one page per spine itemref"**, whatever box
//! the caller passed.
//!
//! Those are not the same rule and neither is a special case of the other, so
//! this file asserts each on its own and then asserts them **against each
//! other**: the same three content documents, opened twice at two different
//! page boxes, reflowable and pre-paginated. The reflowable book's page count
//! changes with the box and the fixed one's does not.
//!
//! # What §8.2.2.6 puts where
//!
//! The `rendition:layout` declaration is in the **package document** and the
//! dimensions are in the **content document**, one `<meta name="viewport">` per
//! spine item. Two items of one book may therefore be two different sizes,
//! which is why a page box became a field of a chapter rather than of a book.
//!
//! # And what §8.1.2 clips
//!
//! The viewport is the initial containing block and what falls outside it is
//! clipped. Vertically that is pagination's answer — the second page a
//! reflowable document would get does not exist — and horizontally it is a clip
//! path in the content stream, because nothing about pagination can see a box
//! that is too wide. The two are asserted separately for that reason.

mod epub_support;

use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::epub::ocf::{CONTAINER_ITEM, MIMETYPE_ITEM, OCF_MEDIA_TYPE};
use tinker_pdf::epub::package::{self, RenditionLayout};
use tinker_pdf::epub::{Limits, DEFAULT_PAGE};
use tinker_pdf::{ArchiveWarning, Document, OpenOptions};

// ---- fixtures ---------------------------------------------------------------

const CONTAINER: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" "#,
    r#"media-type="application/oebps-package+xml"/></rootfiles></container>"#
);

/// A package document with a stated `rendition:layout`, three spine items, and
/// per-itemref `properties` the caller chooses.
fn package_opf(layout: Option<&str>, itemref_properties: [&str; 3]) -> String {
    let meta = match layout {
        Some(value) => {
            format!(r#"<meta property="rendition:layout">{value}</meta>"#)
        }
        None => String::new(),
    };
    let mut manifest = String::new();
    let mut spine = String::new();
    for (at, properties) in itemref_properties.iter().enumerate() {
        let id = at + 1;
        manifest.push_str(&format!(
            r#"<item id="ch{id}" href="ch{id}.xhtml" media-type="application/xhtml+xml"/>"#
        ));
        let attribute = if properties.is_empty() {
            String::new()
        } else {
            format!(r#" properties="{properties}""#)
        };
        spine.push_str(&format!(r#"<itemref idref="ch{id}"{attribute}/>"#));
    }
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" "#,
            r#"unique-identifier="id" prefix="rendition: http://www.idpf.org/vocab/rendition/#">"#,
            r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
            r#"<dc:identifier id="id">urn:uuid:0d1f3f2e-0000-4000-8000-00000000000c</dc:identifier>"#,
            r#"<dc:title>A Fixed Book</dc:title><dc:language>en</dc:language>"#,
            "{meta}</metadata><manifest>{manifest}</manifest><spine>{spine}</spine></package>"
        ),
        meta = meta,
        manifest = manifest,
        spine = spine
    )
}

/// A content document with a §8.2.2.6 viewport and a body the caller chooses.
fn chapter(viewport: Option<(u32, u32)>, body: &str) -> String {
    let meta = match viewport {
        Some((width, height)) => {
            format!(r#"<meta name="viewport" content="width={width}, height={height}"/>"#)
        }
        None => String::new(),
    };
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Fixed</title>"#,
            "{meta}</head><body>{body}</body></html>"
        ),
        meta = meta,
        body = body
    )
}

/// Enough prose that a 432 × 648 page cannot hold it, so a reflowable
/// pagination and a fixed one give visibly different page counts.
fn long_body() -> String {
    let mut out = String::new();
    for at in 0..40 {
        out.push_str(&format!(
            "<p>Paragraph {at} of a chapter written to be longer than one page of \
             any reasonable size, so that the two pagination rules disagree about \
             it rather than happening to agree.</p>"
        ));
    }
    out
}

/// A book of three chapters, at a stated layout and viewport.
fn book(layout: Option<&str>, properties: [&str; 3], viewport: Option<(u32, u32)>) -> Vec<u8> {
    book_with_bodies(layout, properties, viewport, [&long_body(); 3])
}

fn book_with_bodies(
    layout: Option<&str>,
    properties: [&str; 3],
    viewport: Option<(u32, u32)>,
    bodies: [&str; 3],
) -> Vec<u8> {
    let mut entries = vec![
        OcfEntry::stored(MIMETYPE_ITEM, OCF_MEDIA_TYPE),
        OcfEntry::deflated(CONTAINER_ITEM, CONTAINER.as_bytes()),
        OcfEntry::deflated(
            "EPUB/content.opf",
            package_opf(layout, properties).as_bytes(),
        ),
    ];
    for (at, body) in bodies.iter().enumerate() {
        entries.push(OcfEntry::deflated(
            &format!("EPUB/ch{}.xhtml", at + 1),
            chapter(viewport, body).as_bytes(),
        ));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

fn open_at(bytes: &[u8], page: (f64, f64)) -> Document {
    let mut options = OpenOptions::default();
    options.page = page;
    Document::open_with(bytes.to_vec(), &options).expect("the fixture opens")
}

/// One page's content stream, decoded, through this repository's own reader.
fn page_content(doc: &Document, at: usize) -> String {
    let cos = doc.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.get(at).expect("that page exists");
    String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned()
}

fn warnings(doc: &Document) -> Vec<ArchiveWarning> {
    doc.archive()
        .expect("a synthesised book carries a report")
        .warnings()
        .to_vec()
}

// ---- §8.2: which pagination a spine item goes through ------------------------

/// `rendition:layout` is read out of the package's metadata expression, and a
/// book that says nothing is reflowable by §8.2.1's own default.
#[test]
fn the_metadata_expression_says_which_layout_the_book_is() {
    let parsed = |bytes: &[u8]| {
        let opf = epub_support::read(bytes, "EPUB/content.opf").expect("the package document");
        package::parse(&opf, "EPUB/content.opf", &Limits::DEFAULT).expect("it parses")
    };
    assert_eq!(
        parsed(&book(Some("pre-paginated"), ["", "", ""], None)).rendition_layout(),
        RenditionLayout::PrePaginated
    );
    assert_eq!(
        parsed(&book(Some("reflowable"), ["", "", ""], None)).rendition_layout(),
        RenditionLayout::Reflowable
    );
    assert_eq!(
        parsed(&book(None, ["", "", ""], None)).rendition_layout(),
        RenditionLayout::Reflowable,
        "a book that declares nothing is reflowable and not undecided"
    );
}

/// A `rendition:layout` outside the vocabulary is **named** and the book is
/// reflowed, which is §8.2.1's default rather than a guess.
///
/// A misspelt `pre-paginated` would otherwise be a fixed-layout book silently
/// reflowed — a complete-looking book of the wrong shape, which is the failure
/// this whole plan is organised around.
#[test]
fn an_unknown_rendition_layout_is_named_and_reflowed() {
    let bytes = book(Some("pre-pagenated"), ["", "", ""], Some((600, 800)));
    let opf = epub_support::read(&bytes, "EPUB/content.opf").expect("the package document");
    let parsed = package::parse(&opf, "EPUB/content.opf", &Limits::DEFAULT).expect("it parses");
    assert_eq!(parsed.rendition_layout(), RenditionLayout::Reflowable);
    assert!(
        parsed
            .defects()
            .contains(&package::PackageDefect::UnknownRenditionLayout),
        "{:?}",
        parsed.defects()
    );
    let doc = open_at(&bytes, DEFAULT_PAGE);
    assert!(
        doc.page_count() > 3,
        "and it was paginated by the box: {}",
        doc.page_count()
    );
}

/// §8.2.2: an `<itemref>`'s own property beats the book's declaration, in
/// **both** directions.
///
/// One assertion for each direction, because a build that read the itemref only
/// when the book said `reflowable` would honour every fixed-layout override and
/// silently ignore every reflowable one — and a reflowable chapter inside a
/// fixed-layout book is what a fixed-layout book's own colophon is.
#[test]
fn an_itemrefs_property_overrides_the_books_declaration_both_ways() {
    let reflowable_book = book(
        None,
        ["", "rendition:layout-pre-paginated", ""],
        Some((600, 800)),
    );
    let doc = open_at(&reflowable_book, DEFAULT_PAGE);
    let sizes: Vec<(f64, f64)> = (0..doc.page_count())
        .map(|at| doc.page(at).expect("a page").size())
        .collect();
    let fixed_pages = sizes.iter().filter(|size| size.0 == 450.0).count();
    assert_eq!(
        fixed_pages, 1,
        "exactly the overridden itemref is one fixed page: {sizes:?}"
    );

    let fixed_book = book(
        Some("pre-paginated"),
        ["", "rendition:layout-reflowable", ""],
        Some((600, 800)),
    );
    let doc = open_at(&fixed_book, DEFAULT_PAGE);
    let sizes: Vec<(f64, f64)> = (0..doc.page_count())
        .map(|at| doc.page(at).expect("a page").size())
        .collect();
    assert_eq!(
        sizes.iter().filter(|size| size.0 == 450.0).count(),
        2,
        "the two that stayed fixed: {sizes:?}"
    );
    assert!(
        sizes.iter().filter(|size| size.0 == DEFAULT_PAGE.0).count() > 1,
        "and the reflowable one paginated by the box: {sizes:?}"
    );
}

// ---- RS §8.1: the two pagination rules ---------------------------------------

/// **The whole of milestone 12's structural claim, in one test.**
///
/// The same three content documents, opened four times: reflowable and
/// pre-paginated, at two page boxes each. The reflowable book's page count is a
/// function of the box; the fixed book's is a function of the spine and is
/// three either way.
#[test]
fn reflowable_paginates_by_the_box_and_pre_paginated_by_the_spine() {
    let reflowable = book(None, ["", "", ""], Some((600, 800)));
    let fixed = book(Some("pre-paginated"), ["", "", ""], Some((600, 800)));

    let small = open_at(&reflowable, (300.0, 400.0)).page_count();
    let large = open_at(&reflowable, (900.0, 1200.0)).page_count();
    assert!(
        small > large,
        "a reflowable book's page count follows the box: {small} at 300x400, \
         {large} at 900x1200"
    );

    assert_eq!(
        open_at(&fixed, (300.0, 400.0)).page_count(),
        3,
        "exactly one page per spine itemref"
    );
    assert_eq!(
        open_at(&fixed, (900.0, 1200.0)).page_count(),
        3,
        "and the caller's box does not change it"
    );
}

/// §8.2.2.6: a pre-paginated page is the **viewport's** size, in points, and
/// not the caller's box.
#[test]
fn a_fixed_page_is_the_size_the_viewport_meta_states() {
    let bytes = book(Some("pre-paginated"), ["", "", ""], Some((1200, 1600)));
    let doc = open_at(&bytes, DEFAULT_PAGE);
    // 1 200 CSS pixels at 0.75 points a pixel is 900 points.
    for at in 0..doc.page_count() {
        assert_eq!(doc.page(at).expect("a page").size(), (900.0, 1200.0));
    }
}

/// Two spine items of one book may be two different sizes, which is what
/// putting the dimensions in the *content* document means.
#[test]
fn two_fixed_chapters_may_be_two_different_page_sizes() {
    let mut entries = vec![
        OcfEntry::stored(MIMETYPE_ITEM, OCF_MEDIA_TYPE),
        OcfEntry::deflated(CONTAINER_ITEM, CONTAINER.as_bytes()),
        OcfEntry::deflated(
            "EPUB/content.opf",
            package_opf(Some("pre-paginated"), ["", "", ""]).as_bytes(),
        ),
    ];
    for (at, size) in [(400, 600), (800, 400), (1200, 1600)].iter().enumerate() {
        entries.push(OcfEntry::deflated(
            &format!("EPUB/ch{}.xhtml", at + 1),
            chapter(Some(*size), "<p>One line.</p>").as_bytes(),
        ));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    let doc = open_at(&ocf_zip(&entries, &directory), DEFAULT_PAGE);
    let sizes: Vec<(f64, f64)> = (0..doc.page_count())
        .map(|at| doc.page(at).expect("a page").size())
        .collect();
    assert_eq!(sizes, vec![(300.0, 450.0), (600.0, 300.0), (900.0, 1200.0)]);
}

/// A pre-paginated item with **no** viewport is still one page and says what is
/// missing.
///
/// Two assertions, because the two facts are separable: the spine rule holds
/// whatever the document says about its size, and the size is the caller's box
/// rather than one this build invented.
#[test]
fn a_fixed_item_with_no_viewport_is_named_and_still_one_page() {
    let bytes = book(Some("pre-paginated"), ["", "", ""], None);
    let doc = open_at(&bytes, DEFAULT_PAGE);
    assert_eq!(
        doc.page_count(),
        3,
        "the spine rule holds without a viewport"
    );
    assert_eq!(doc.page(0).expect("a page").size(), DEFAULT_PAGE);
    assert_eq!(
        warnings(&doc)
            .iter()
            .filter(|w| matches!(w, ArchiveWarning::FixedLayoutWithoutViewport { .. }))
            .count(),
        3,
        "one per item, each naming the item: {:?}",
        warnings(&doc)
    );
}

/// §8.2.2.6's grammar is two **numbers**. `width=device-width` is valid HTML,
/// is not a number, and a reading system with no device cannot resolve it — so
/// it is the same answer as no viewport at all rather than a guessed size.
#[test]
fn device_width_is_not_a_viewport() {
    let mut entries = vec![
        OcfEntry::stored(MIMETYPE_ITEM, OCF_MEDIA_TYPE),
        OcfEntry::deflated(CONTAINER_ITEM, CONTAINER.as_bytes()),
        OcfEntry::deflated(
            "EPUB/content.opf",
            package_opf(Some("pre-paginated"), ["", "", ""]).as_bytes(),
        ),
    ];
    for at in 1..=3 {
        let document = concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Fixed</title>"#,
            r#"<meta name="viewport" content="width=device-width, height=device-height"/>"#,
            r#"</head><body><p>One line.</p></body></html>"#
        );
        entries.push(OcfEntry::deflated(
            &format!("EPUB/ch{at}.xhtml"),
            document.as_bytes(),
        ));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    let doc = open_at(&ocf_zip(&entries, &directory), DEFAULT_PAGE);
    assert_eq!(doc.page(0).expect("a page").size(), DEFAULT_PAGE);
    assert!(
        warnings(&doc)
            .iter()
            .any(|w| matches!(w, ArchiveWarning::FixedLayoutWithoutViewport { .. })),
        "{:?}",
        warnings(&doc)
    );
}

// ---- RS §8.1.2: the initial containing block ---------------------------------

/// Content below the initial containing block is **clipped**, counted, and
/// named — and the same content in a reflowable book is not.
///
/// The control is the half that matters: a build with no fixed-layout path at
/// all would pass the first assertion by paginating normally and losing
/// nothing, and the page count is what says the text went somewhere.
#[test]
fn content_below_the_initial_containing_block_is_clipped_and_counted() {
    let bytes = book(Some("pre-paginated"), ["", "", ""], Some((400, 200)));
    let doc = open_at(&bytes, DEFAULT_PAGE);
    assert_eq!(doc.page_count(), 3);
    let clipped: Vec<usize> = warnings(&doc)
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::FixedLayoutContentClipped { characters, .. } => Some(*characters),
            _ => None,
        })
        .collect();
    assert_eq!(clipped.len(), 3, "{:?}", warnings(&doc));
    assert!(
        clipped.iter().all(|count| *count > 1_000),
        "most of a forty-paragraph chapter did not fit 400 by 200: {clipped:?}"
    );

    // The control: the same documents, reflowed, lose nothing and say nothing.
    let reflowed = open_at(&book(None, ["", "", ""], Some((400, 200))), DEFAULT_PAGE);
    assert!(
        !warnings(&reflowed)
            .iter()
            .any(|w| matches!(w, ArchiveWarning::FixedLayoutContentClipped { .. })),
        "{:?}",
        warnings(&reflowed)
    );
}

/// A pre-paginated page carries §8.1.2's clip path, and a reflowable one does
/// not.
///
/// **Pagination cannot see this half.** A box wider than the viewport is on the
/// page it belongs to at an `x` past its right edge; no page count and no text
/// comparison notices, and the clip is what stops it being drawn. Read out of
/// the content stream rather than inferred, because the operators are the whole
/// claim.
#[test]
fn a_fixed_page_clips_to_its_initial_containing_block() {
    let wide = r#"<div style="width: 900px; background-color: #ff0000">Wide</div>"#;
    let bytes = book_with_bodies(
        Some("pre-paginated"),
        ["", "", ""],
        Some((400, 300)),
        [wide, wide, wide],
    );
    let doc = open_at(&bytes, DEFAULT_PAGE);
    let text = page_content(&doc, 0);
    assert!(
        text.contains("0 0 300 225 re W n"),
        "the initial containing block, as a clip: {text}"
    );
    assert!(
        text.trim_end().ends_with('Q'),
        "and it is closed, so the clip does not leak into the next page's          graphics state: {text}"
    );

    let reflowed = open_at(
        &book_with_bodies(None, ["", "", ""], None, [wide; 3]),
        DEFAULT_PAGE,
    );
    let text = page_content(&reflowed, 0);
    assert!(
        !text.contains(" re W n"),
        "a reflowable page has no initial containing block to clip to: {text}"
    );
}

/// A pre-paginated document is cascaded against **its own** viewport, not
/// against the caller's page box.
///
/// §8.2.2.6 makes the viewport the initial containing block, so
/// `@media (max-width: …)` in a fixed-layout book is a question about the
/// document's own dimensions. A build that evaluated it against
/// `OpenOptions::page` gives every host a different book.
#[test]
fn media_queries_in_a_fixed_document_are_about_its_viewport() {
    let body = concat!(
        r#"<style>@media (max-width: 500px) { p { display: none } }</style>"#,
        r#"<p>Only visible on a wide viewport.</p>"#
    );
    let narrow = book_with_bodies(
        Some("pre-paginated"),
        ["", "", ""],
        Some((400, 600)),
        [body; 3],
    );
    let wide = book_with_bodies(
        Some("pre-paginated"),
        ["", "", ""],
        Some((900, 600)),
        [body; 3],
    );
    // The caller's box is the same for both and is wider than 500 CSS pixels,
    // so a build reading `OpenOptions::page` would show the paragraph twice.
    let seen = |bytes: &[u8]| {
        let doc = open_at(bytes, (800.0, 1000.0));
        doc.page(0)
            .expect("a page")
            .text()
            .plain_text()
            .contains("Only")
    };
    assert!(!seen(&narrow), "the 400-pixel viewport hides it");
    assert!(seen(&wide), "the 900-pixel viewport shows it");
}

/// **A fixed-layout page has no reading-system margin**, which is a different
/// fact from being the right size.
///
/// EPUB RS 3.3 §8.1.2 makes the viewport the initial containing block, and a
/// margin inside it would be a page the author did not design: everything on it
/// would be shifted and the design's own edge-to-edge elements would stop
/// reaching the edge. The injection matrix asked for this — putting
/// `epub::PAGE_MARGIN` back changed no page count, no page size and no warning,
/// and moved every mark on every fixed page by half an inch.
#[test]
fn a_fixed_page_has_no_reading_system_margin() {
    let ink = |bytes: &[u8]| {
        let doc = open_at(bytes, DEFAULT_PAGE);
        let text = page_content(&doc, 0);
        // The `Td` that positions the first text object, which is where the
        // first line of the chapter starts.
        let at = text.find(" Td").expect("a positioned text object");
        let words: Vec<&str> = text[..at].split_whitespace().collect();
        let x: f64 = words[words.len() - 2].parse().expect("an x");
        x
    };
    let fixed = ink(&book_with_bodies(
        Some("pre-paginated"),
        ["", "", ""],
        Some((600, 800)),
        ["<p>One line.</p>"; 3],
    ));
    let reflowed = ink(&book_with_bodies(
        None,
        ["", "", ""],
        Some((600, 800)),
        ["<p>One line.</p>"; 3],
    ));
    // The user-agent sheet's `body { margin: 8px }` is 6 points and applies to
    // both; the reading system's 36 applies only to the reflowable one.
    assert!(
        fixed < 10.0,
        "the fixed page starts at its own edge: {fixed}"
    );
    assert!(
        reflowed > 36.0,
        "and the reflowable one inside the reading system's margin: {reflowed}"
    );
}

/// **A fixed chapter is laid out into its own viewport**, which is a different
/// fact from the page being that size.
///
/// The injection matrix asked for this: laying every chapter into the caller's
/// box and then drawing it on a page the size of the viewport produces pages of
/// exactly the right dimensions with the text set to the wrong measure, and
/// nothing above notices — the page count is the spine's either way, the sizes
/// are the viewport's either way, and the clip is there either way.
///
/// Two books whose only difference is the viewport **width**, at one caller box
/// wider than both: the narrow one sets the same paragraph in more lines.
#[test]
fn a_fixed_chapter_is_set_at_its_viewports_measure() {
    let body = "<p>A paragraph long enough that the measure it is set at decides \
                how many lines it takes, which is the whole of what laying it out \
                into the viewport rather than into the caller's page box means.</p>";
    let lines = |width: u32| {
        let bytes = book_with_bodies(
            Some("pre-paginated"),
            ["", "", ""],
            Some((width, 2000)),
            [body; 3],
        );
        let doc = open_at(&bytes, (900.0, 1400.0));
        page_content(&doc, 0).matches(" Td").count()
    };
    let narrow = lines(200);
    let wide = lines(880);
    assert!(
        narrow > wide,
        "the 200-pixel viewport takes more lines than the 880-pixel one: \
         {narrow} against {wide}"
    );
}

/// Every page of a fixed-layout book still has its chapter's name in the
/// report, so a host can say which item a page came from.
#[test]
fn a_fixed_page_still_names_the_item_it_came_from() {
    let bytes = book(Some("pre-paginated"), ["", "", ""], Some((600, 800)));
    let doc = open_at(&bytes, DEFAULT_PAGE);
    let report = doc.archive().expect("a report");
    let names: Vec<&str> = report
        .pages()
        .iter()
        .map(|origin| origin.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["EPUB/ch1.xhtml", "EPUB/ch2.xhtml", "EPUB/ch3.xhtml"]
    );
}

// ---- the real book -----------------------------------------------------------
//
// Everything above is a fixture this repository wrote, and every one of them
// was built from §8.2's own sentences. `kcc-fixed-layout.epub` is the first
// pre-paginated book here that a producer wrote, and `tests/epub/README.md`
// records what it does that none of the fixtures did. These are the assertions
// behind that list.
//
// The producer is Kindle Comic Converter 11.0.1 and the pictures are the four
// PNGs `make-corpus.ps1` writes byte by byte from the PNG specification, so the
// book is our input through their tool in exactly the sense the other eight
// are.

/// The committed pre-paginated book.
fn real_book() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
        .join("kcc-fixed-layout.epub");
    std::fs::read(&path).expect("kcc-fixed-layout.epub is committed beside the books")
}

fn real_package() -> package::Package {
    let bytes = real_book();
    let opf =
        epub_support::read(&bytes, "OEBPS/content.opf").expect("the package document is in there");
    package::parse(&opf, "OEBPS/content.opf", &Limits::DEFAULT).expect("it parses")
}

/// **A real producer declares `rendition:layout` with no `prefix` attribute at
/// all**, and every fixture above declares one.
///
/// EPUB 3.3 §5.4.3 makes `rendition:` a *reserved* prefix — a package document
/// may use it without declaring it, and KCC does. `package_opf` above writes
/// `prefix="rendition: http://www.idpf.org/vocab/rendition/#"` on every fixture
/// because that is what the specification's own example shows, so a build that
/// required the declaration would have passed all thirteen tests in this file
/// and reflowed every fixed-layout book in circulation.
///
/// The assertion is in two halves for that reason: the package really does omit
/// the declaration, and it is really read as pre-paginated anyway. Either alone
/// would pass on a build with the bug.
#[test]
fn the_real_book_declares_the_layout_with_no_prefix_declaration() {
    let bytes = real_book();
    let opf = epub_support::read(&bytes, "OEBPS/content.opf").expect("the package document");
    let text = String::from_utf8_lossy(&opf);
    assert!(
        !text.contains("prefix="),
        "the real book grew a prefix declaration, so this test stopped saying anything"
    );
    assert!(
        text.contains(r#"<meta property="rendition:layout">pre-paginated</meta>"#),
        "the spelling this test is about is no longer in the book"
    );
    assert_eq!(
        real_package().rendition_layout(),
        RenditionLayout::PrePaginated
    );
}

/// **The layout is on the package and on no itemref**, and what the itemrefs
/// carry instead is a *different* rendition property in the same attribute.
///
/// Every fixture above that exercises §8.2.2's override puts
/// `rendition:layout-pre-paginated` in `itemref/@properties`. The real book
/// puts `page-spread-left` and `page-spread-right` there — six of them, one per
/// item, alternating — and nothing about the layout. So a build that read that
/// attribute by prefix match, or that treated an unrecognised token there as a
/// defect, meets six unrecognised tokens on the first real book it opens.
///
/// `SpineItem::layout` is asked directly rather than inferred from page sizes,
/// because the answer this is about is *per item* and a page size is the
/// viewport's.
#[test]
fn the_real_books_itemref_properties_are_spreads_and_not_layouts() {
    let package = real_package();
    let spine = package.spine();
    assert_eq!(spine.len(), 6, "six itemrefs");

    let properties: Vec<&str> = spine
        .iter()
        .flat_map(|item| item.properties.iter().map(String::as_str))
        .collect();
    assert_eq!(
        properties,
        [
            "page-spread-right",
            "page-spread-left",
            "page-spread-right",
            "page-spread-left",
            "page-spread-right",
            "page-spread-left",
        ],
        "the properties the real book writes on its itemrefs changed"
    );
    assert!(
        properties
            .iter()
            .all(|token| RenditionLayout::from_itemref_property(token).is_none()),
        "a spread token was read as a layout: {properties:?}"
    );

    // And every item is still pre-paginated, because the book said so once.
    for item in spine {
        assert_eq!(
            item.layout(package.rendition_layout()),
            RenditionLayout::PrePaginated,
            "{} is not pre-paginated",
            item.idref
        );
    }
    assert_eq!(
        package.defects(),
        [],
        "the real book's package document reports a defect"
    );
}

/// **Six viewports, six different sizes, and not one of them is the size of the
/// picture that produced it.**
///
/// `two_fixed_chapters_may_be_two_different_page_sizes` above makes this claim
/// on three hand-built documents. This is the same claim on a book nobody here
/// wrote — and it is a stronger version of it, because the producer chose the
/// numbers: KCC resized the cover from 120 × 180 to 117 × 177 to fit its device
/// profile's aspect ratio, and **split the two landscape plates in half**, so
/// four pictures became six pages at six sizes.
///
/// The sizes are points: a CSS pixel is three quarters of a point, so
/// `width=117` is 87.75.
#[test]
fn the_real_book_is_one_page_per_itemref_at_six_different_sizes() {
    let bytes = real_book();
    let doc = open_at(&bytes, DEFAULT_PAGE);
    assert_eq!(doc.page_count(), 6, "one page per spine itemref");

    let sizes: Vec<(f64, f64)> = (0..doc.page_count())
        .map(|at| doc.page(at).expect("a page").size())
        .collect();
    assert_eq!(
        sizes,
        vec![
            (87.75, 132.75),
            (22.5, 30.0),
            (21.75, 27.0),
            (30.0, 37.5),
            (29.25, 33.75),
            (30.0, 66.0),
        ]
    );

    // Six distinct sizes, which is what makes the per-item viewport visible
    // rather than merely present. A build that read one viewport and used it
    // for the book would give six equal pages and the count above would not
    // notice.
    let mut distinct = sizes.clone();
    distinct.sort_unstable_by(|a, b| a.partial_cmp(b).expect("no NaN in a page size"));
    distinct.dedup();
    assert_eq!(distinct.len(), 6, "two pages came out the same size");

    // And the caller's box changes none of it, which is RS §8.1's rule over a
    // real file rather than over a fixture built to state it.
    let wide = open_at(&bytes, (900.0, 1200.0));
    let wide_sizes: Vec<(f64, f64)> = (0..wide.page_count())
        .map(|at| wide.page(at).expect("a page").size())
        .collect();
    assert_eq!(wide_sizes, sizes);
}

/// **A real producer's book trips neither warning**, which is the half a
/// corpus of fixtures cannot supply.
///
/// Every fixture that produces `FixedLayoutWithoutViewport` or
/// `FixedLayoutContentClipped` above was built to produce it. The question those
/// tests cannot answer is whether an ordinary book trips them anyway — a
/// viewport grammar read too strictly, or an initial containing block computed
/// a fraction of a point too small, would warn on every page of every real book
/// and every fixture here would still pass.
#[test]
fn the_real_book_trips_neither_fixed_layout_warning() {
    let doc = open_at(&real_book(), DEFAULT_PAGE);
    let reported = warnings(&doc);
    assert!(
        !reported.iter().any(|w| matches!(
            w,
            ArchiveWarning::FixedLayoutWithoutViewport { .. }
                | ArchiveWarning::FixedLayoutContentClipped { .. }
        )),
        "a real producer's fixed-layout book tripped a fixed-layout warning: {reported:?}"
    );
    // And nothing else either. Stated as the whole list rather than as the two
    // variants above, because the useful fact is that this build has *nothing*
    // to say about an ordinary pre-paginated book, and a warning added later
    // that fires on every comic should come here first.
    assert_eq!(reported, [], "the real fixed-layout book reports something");
}

/// **Every page of the real book is its clip and nothing else**, and that is
/// this build's largest gap in this format written down as a test.
///
/// A fixed-layout comic is six pictures. This build paginates it correctly, at
/// the right count, at the right six sizes, with §8.1.2's clip on each — and
/// **draws none of the pictures**, because no path in `epub::` turns an `<img>`
/// into a box. Six blank pages of exactly the right dimensions is a
/// complete-looking book of the wrong contents, which is the failure this whole
/// gap is organised around, and it took a real fixed-layout book to make it
/// visible: the other eight books are text, and text arrives.
///
/// Written as an assertion rather than a comment so that the milestone which
/// paints a replaced element **has to come here and delete it**. `q … re W n …
/// Q` and nothing between is the whole content stream today.
#[test]
fn today_every_page_of_the_real_book_is_empty_inside_its_clip() {
    let doc = open_at(&real_book(), DEFAULT_PAGE);
    for at in 0..doc.page_count() {
        let content = page_content(&doc, at as usize);
        let (width, height) = doc.page(at).expect("a page").size();
        let expected = format!("q 0 0 {width} {height} re W n Q");
        assert_eq!(
            content.split_whitespace().collect::<Vec<_>>().join(" "),
            expected,
            "page {at} draws something, so this test is out of date and the \
             README's finding about it needs re-measuring"
        );
    }
    // The pictures really are in the container, so the emptiness is the
    // painter's and not the archive's.
    let bytes = real_book();
    for name in [
        "OEBPS/Images/kcc-0001-kcc-x.jpg",
        "OEBPS/Images/kcc-0004-kcc-x.jpg",
    ] {
        let picture = epub_support::read(&bytes, name).unwrap_or_else(|| panic!("{name}"));
        assert!(
            picture.starts_with(&[0xFF, 0xD8]),
            "{name} is not a JPEG in the container"
        );
    }
}

/// **The real book's pages carry no text either, and the six characters it does
/// have are ones it means to hide.**
///
/// `CONSERVATION.tsv` records `0` of `6` conserved for this book, which is the
/// only row in that file where the two numbers differ, and it would read as a
/// text-conservation failure without this. KCC writes
/// `<div style="display:none;">.</div>` into every one of its six content
/// documents — a Kindle workaround for a page with no text at all — and a full
/// stop that is `display: none` is a character this build is right to drop.
///
/// So the conservation record's exception is a producer's habit rather than a
/// defect, and this is where that is established rather than asserted in a
/// comment.
#[test]
fn the_real_books_six_lost_characters_are_six_hidden_full_stops() {
    let bytes = real_book();
    let mut hidden = 0usize;
    for at in 1..=4u32 {
        for suffix in ["x", "b", "c"] {
            let name = format!("OEBPS/Text/kcc-000{at}-kcc-{suffix}.xhtml");
            let Some(document) = epub_support::read(&bytes, &name) else {
                continue;
            };
            let text = String::from_utf8_lossy(&document).into_owned();
            assert!(
                text.contains(r#"<div style="display:none;">.</div>"#),
                "{name} does not carry the hidden full stop this test is about"
            );
            hidden += 1;
        }
    }
    assert_eq!(hidden, 6, "six content documents, six hidden full stops");

    let doc = open_at(&bytes, DEFAULT_PAGE);
    for at in 0..doc.page_count() {
        assert_eq!(
            doc.page(at).expect("a page").text().plain_text().trim(),
            "",
            "page {at} shows the character the book hid"
        );
    }
}
