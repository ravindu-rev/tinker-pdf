//! The spine, in markup order, with the boxes a fixed page states (gap 30,
//! milestone 4).
//!
//! Milestone 3 landed the spine a milestone early, because milestone 1's
//! `an_xps_with_a_raster_resource_is_not_a_one_page_comic` calls `.expect` on
//! `Document::open` and then asks the first page its size — a criterion no
//! refusal can satisfy. What that milestone left to this one is written into
//! its own progress note: **markup order**, `/CropBox` and `/BleedBox` from
//! `ContentBox` and `BleedBox`, resolving the payload parts by **media type**
//! rather than by root element, and qpdf on the produced file. The qpdf half is
//! `tests/xps_qpdf.rs`; the rest is here.
//!
//! # Why the fixtures are built rather than borrowed
//!
//! The corpus cannot see any of this, and saying so is the point rather than an
//! apology:
//!
//! - **Every real package names its pages `1.fpage`, `2.fpage`, `3.fpage` and
//!   references them in that order**, so markup order and filename order agree
//!   in all eight and a reader that sorted by name would pass every one of
//!   them. That is gap 29's `p10, p1, p2` lesson in a second format — that
//!   gap's natural-sort defect survived every fixture whose names were
//!   zero-padded, because a padded name sorts the same under both orders.
//! - **No fixed page in the corpus carries a `ContentBox` or a `BleedBox`.**
//!   Both serialisers omit them, so the only measurement available is the
//!   arithmetic, and the arithmetic is three conversions stacked — a unit, an
//!   origin and a rectangle's own form — each of which is right on a box at the
//!   page's own origin and wrong everywhere else.
//! - **Every real payload part's extension agrees with its media type**, so
//!   routing by extension and routing by 7.2.3.5 give the same answer on all
//!   fifty-two of them.
//!
//! So the package a test wants here is one no producer would ever write, which
//! is the shape `tests/xps_opc.rs` already argues for at length.

mod xps_support;

use tinker_pdf::xps::opc::PartName;
use tinker_pdf::{ArchiveRefusal, ArchiveWarning, Bitmap, Document, RenderOptions, XpsPageDefect};
use xps_support::{
    archive, before_content_types, binary_part, content_types_with, distinct_pixels, document,
    fixed_page, fixed_page_with, one_page_package, open, opened, part, refusal, rgb_png, sequence,
    with, Part,
};

// ---- helpers ------------------------------------------------------------

/// Every page's `(name, size)`, in the order the document puts them.
fn page_order(document: &Document) -> Vec<(String, (f64, f64))> {
    let report = document.archive().expect("a synthesised document");
    report
        .pages()
        .iter()
        .enumerate()
        .map(|(at, origin)| {
            let page = document
                .page(at as u32)
                .unwrap_or_else(|| panic!("page {at}"));
            (origin.name.clone(), page.size())
        })
        .collect()
}

fn warnings(document: &Document) -> Vec<ArchiveWarning> {
    document
        .archive()
        .expect("a synthesised document")
        .warnings()
        .to_vec()
}

fn pixel(bitmap: &Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[0, 0, 0]);
    (p[0], p[1], p[2])
}

/// A three-page document whose markup order agrees with **no** order over its
/// filenames, and whose pages are three different sizes so the order can be
/// read off the geometry as well as off the report.
///
/// Four orders, all distinct, which is what makes the fixture worth building:
///
/// | order | pages |
/// | --- | --- |
/// | markup, which is the answer | `p10`, `p1`, `p2` |
/// | natural, gap 29's comic order | `p1`, `p2`, `p10` |
/// | lexicographic | `p1`, `p10`, `p2` |
/// | the archive's own storage order | `p2`, `p10`, `p1` |
fn out_of_order_package() -> Vec<Part> {
    vec![
        part("_rels/.rels", &package_rels()),
        part(
            "FixedDocumentSequence.fdseq",
            &sequence(&["Documents/1/FixedDocument.fdoc"]),
        ),
        part(
            "Documents/1/FixedDocument.fdoc",
            // Markup order, which 12.3.1 makes the document's order.
            &document(&["Pages/p10.fpage", "Pages/p1.fpage", "Pages/p2.fpage"]),
        ),
        // Stored in a third order again, so an implementation that walked the
        // central directory would produce a fourth answer.
        part("Documents/1/Pages/p2.fpage", &fixed_page("200", "300")),
        part("Documents/1/Pages/p10.fpage", &fixed_page("816", "1056")),
        part("Documents/1/Pages/p1.fpage", &fixed_page("400", "600")),
        part("[Content_Types].xml", xps_support::CONTENT_TYPES),
    ]
}

fn package_rels() -> String {
    xps_support::package_rels("/FixedDocumentSequence.fdseq")
}

fn corpus(name: &str) -> Vec<u8> {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/xps")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

// ---- markup order -------------------------------------------------------

/// **The criterion.** Pages arrive in markup order and in no order over names.
///
/// 12.3.1 gives a `FixedDocument`'s pages in the order its `PageContent`
/// elements appear and defines no other, so the archive's storage order and
/// every sort over the part names are four different wrong answers. Gap 29 paid
/// for this once already: its comic path sorts by name because a CBZ has
/// nothing else, and the natural-order defect it fixed survived every fixture
/// whose names were zero-padded — `page001` and `page010` sort the same way
/// under both orders, so a padded fixture proves nothing at all.
///
/// Read twice, independently. `ArchiveReport::pages` names the part each page
/// came from, and `Page::size` is the size that page's own markup states — so a
/// build that ordered the report one way and the pages another would fail here
/// rather than agreeing with itself.
#[test]
fn pages_come_in_markup_order_and_in_no_order_over_their_names() {
    let bytes = archive(out_of_order_package());
    let document = open(&bytes).expect("an XPS");
    assert_eq!(document.page_count(), 3);

    let markup = vec![
        ("/Documents/1/Pages/p10.fpage".to_string(), (612.0, 792.0)),
        ("/Documents/1/Pages/p1.fpage".to_string(), (300.0, 450.0)),
        ("/Documents/1/Pages/p2.fpage".to_string(), (150.0, 225.0)),
    ];
    assert_eq!(page_order(&document), markup);

    // And the fixture really does distinguish them: sorted by name, the same
    // three pages come out in a different order under either sort, so neither
    // could have produced the answer above by accident.
    let mut lexicographic = markup.clone();
    lexicographic.sort_by(|a, b| a.0.cmp(&b.0));
    assert_ne!(
        lexicographic, markup,
        "the fixture's names must not already be in markup order, or it proves \
         nothing — this is gap 29's zero-padded-fixture trap"
    );
    assert_eq!(
        lexicographic
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [
            "/Documents/1/Pages/p1.fpage",
            "/Documents/1/Pages/p10.fpage",
            "/Documents/1/Pages/p2.fpage"
        ]
    );
}

/// The multi-page package milestone 1 committed, counted against its own
/// markup.
///
/// `page_count()` equals the number of `PageContent` elements, and the number
/// of `PageContent` elements is read out of the part rather than written down
/// here — so a package regenerated with a different number of pages would move
/// both sides together and a reader that lost one would move only the first.
#[test]
fn the_real_multi_page_package_has_one_page_per_page_content() {
    let bytes = corpus("wpf-three-pages.xps");
    let mut opened = opened(&bytes);
    let fdoc = PartName::from_item("Documents/1/FixedDocument.fdoc").expect("a part name");
    let markup = opened
        .read_part(&fdoc)
        .expect("the fixed document")
        .to_vec();
    let text = String::from_utf8(markup).expect("WPF writes UTF-8");
    let references = text.matches("<PageContent").count();
    assert_eq!(references, 3, "the committed package has three references");

    let document = Document::open(bytes).expect("a fixed document");
    assert_eq!(document.page_count() as usize, references);
    assert_eq!(
        page_order(&document)
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>(),
        [
            "/Documents/1/Pages/1.fpage",
            "/Documents/1/Pages/2.fpage",
            "/Documents/1/Pages/3.fpage",
        ]
    );
}

/// A `PageContent` that does not resolve keeps **its place**, not just a place.
///
/// `tests/xps_opc.rs` already holds the page count against dropping one. This
/// is the half that only an out-of-order document can see: the placeholder has
/// to sit where the markup put it, and a build that dropped the reference and
/// appended a page at the end would keep the count and still renumber
/// everything after it.
#[test]
fn an_unresolved_page_keeps_its_place_in_markup_order() {
    let bytes = archive(with(
        out_of_order_package(),
        "Documents/1/FixedDocument.fdoc",
        &document(&["Pages/p10.fpage", "Pages/gone.fpage", "Pages/p2.fpage"]),
    ));
    let document = open(&bytes).expect("an XPS");
    assert_eq!(document.page_count(), 3);
    assert_eq!(
        page_order(&document),
        [
            ("/Documents/1/Pages/p10.fpage".to_string(), (612.0, 792.0)),
            // The book's own first size, so a reader can page through it.
            ("/Documents/1/Pages/gone.fpage".to_string(), (612.0, 792.0)),
            ("/Documents/1/Pages/p2.fpage".to_string(), (150.0, 225.0)),
        ]
    );
    assert!(warnings(&document).contains(&ArchiveWarning::XpsPage {
        page: 1,
        defect: XpsPageDefect::SourceUnresolved,
    }));
}

// ---- 10.3's two rectangles ----------------------------------------------

/// `ContentBox` becomes `/CropBox` and `BleedBox` becomes `/BleedBox`.
///
/// The numbers are the whole test. A fixed page is 816 x 1056 in units of 1/96
/// inch, which is 612 x 792 pt; `ContentBox="96,48,192,96"` is `x,y,width,height`
/// with the origin at the **top** left and y increasing downward, so:
///
/// ```text
/// x0 = 96 * 0.75                    =  72
/// x1 = (96 + 192) * 0.75            = 216
/// y1 = (1056 - 48) * 0.75           = 756
/// y0 = (1056 - 48 - 96) * 0.75      = 684
/// ```
///
/// Every plausible mistake lands somewhere else and none of them lands
/// nowhere: no flip gives `[72, 36, 216, 108]`, no scale gives
/// `[96, 912, 288, 1008]`, the scale applied twice gives `[54, 513, 162, 567]`,
/// and reading the four numbers as PDF's `x0 y0 x1 y1` gives `[72, 720, 144,
/// 756]`. That is why the fixture's box is neither square nor at the origin.
#[test]
fn a_content_box_and_a_bleed_box_reach_the_pdf_scaled_once_and_flipped_once() {
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        &fixed_page_with(
            "816",
            "1056",
            r#"ContentBox="96,48,192,96" BleedBox="48,24,720,1008" "#,
        ),
    ));
    let document = open(&bytes).expect("an XPS");
    let page = document.page(0).expect("a page");

    assert_eq!(
        page.media_box(),
        (0.0, 0.0, 612.0, 792.0),
        "the page's own size is untouched by either box"
    );
    assert_eq!(page.crop_box(), (72.0, 684.0, 216.0, 756.0));

    // `/BleedBox` is not a rectangle this engine reads back through `Page`, so
    // it is read out of the object the writer wrote — which is also the only
    // way to tell "absent" from "equal to the media box".
    assert_eq!(
        bleed_box(&document, 0),
        Some(vec![36.0, 18.0, 576.0, 774.0])
    );

    // **And the consequence, asserted rather than left to be discovered.**
    // 7.7.3.3 makes `/CropBox` the region a viewer lays out, and this engine's
    // `Page::size` is that rectangle, so a fixed page that states a `ContentBox`
    // reports the content box's size and renders at it. The page is not lost —
    // `media_box` above still carries the whole of it — but a host asking "how
    // big is this page" gets the answer PDF gives rather than the answer XPS
    // would.
    assert_eq!(page.size(), (144.0, 72.0));
}

/// A page that states neither box carries neither, which is the half a writer
/// gets wrong by defaulting them.
///
/// 7.7.3.3 makes an absent `/CropBox` mean the media box, so a build that wrote
/// one on every page would be indistinguishable through `Page::crop_box` and
/// would still have turned "this page said nothing" into "this page said all of
/// it". Every package in milestone 1's corpus is in this state, so this is the
/// case the real files do cover.
#[test]
fn a_page_that_states_no_box_carries_none() {
    let bytes = archive(one_page_package());
    let document = open(&bytes).expect("an XPS");
    assert_eq!(
        document.page(0).map(|p| p.crop_box()),
        Some((0.0, 0.0, 612.0, 792.0))
    );
    assert_eq!(
        crop_box(&document, 0),
        None,
        "and not because one was written"
    );
    assert_eq!(bleed_box(&document, 0), None);

    let real = Document::open(corpus("wpf-image-and-text.xps")).expect("a fixed document");
    assert_eq!(crop_box(&real, 0), None, "no real package states one");
    assert_eq!(bleed_box(&real, 0), None);
}

/// 14.11.2: a box outside the media box is reduced to its intersection with it.
///
/// An XPS `BleedBox` is *meant* to extend past the page — that is what bleed is
/// — and PDF has nowhere to put one, so the intersection is what any consumer
/// would compute anyway. Written down as a test because "it clamps" and "it
/// silently dropped the attribute" look identical from outside.
#[test]
fn a_bleed_box_that_runs_off_the_page_is_reduced_to_the_page() {
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        &fixed_page_with("816", "1056", r#"BleedBox="-24,-24,864,1104" "#),
    ));
    let document = open(&bytes).expect("an XPS");
    assert_eq!(bleed_box(&document, 0), Some(vec![0.0, 0.0, 612.0, 792.0]));
    // It is a statement, not a defect: nothing warns about a box that clamped.
    assert!(!warnings(&document).contains(&ArchiveWarning::XpsPage {
        page: 0,
        defect: XpsPageDefect::PageBoxUnusable,
    }));
}

/// A box that will not read is **named**, and the page is otherwise unchanged.
///
/// Six spellings, each of which a defaulting reader would turn into "the whole
/// page" — which is a plausible answer, and a plausible answer silently
/// substituted for a statement the file made is what gap 07's solid-black
/// stroke was. The page keeps its size, its number and its place; only the box
/// is missing, and the report says so.
#[test]
fn a_page_box_that_will_not_read_is_named_rather_than_defaulted() {
    for attributes in [
        r#"ContentBox="96,48,192" "#,         // three numbers
        r#"ContentBox="96,48,192,96,12" "#,   // five
        r#"ContentBox="" "#,                  // none
        r#"ContentBox="96,48,nonsense,96" "#, // not a number
        r#"ContentBox="96,48,0,96" "#,        // no width
        r#"ContentBox="96,48,-192,96" "#,     // negative width
    ] {
        let bytes = archive(with(
            one_page_package(),
            "Documents/1/Pages/1.fpage",
            &fixed_page_with("816", "1056", attributes),
        ));
        let document = open(&bytes).unwrap_or_else(|e| panic!("{attributes}: {e:?}"));
        assert_eq!(document.page_count(), 1, "{attributes}");
        assert_eq!(
            document.page(0).map(|p| p.size()),
            Some((612.0, 792.0)),
            "{attributes}: the page is the size its markup states"
        );
        assert_eq!(crop_box(&document, 0), None, "{attributes}: no box written");
        let seen = warnings(&document);
        assert!(
            seen.contains(&ArchiveWarning::XpsPage {
                page: 0,
                defect: XpsPageDefect::PageBoxUnusable,
            }),
            "{attributes}: {seen:?}"
        );
        // And the page still says the one thing every page in this milestone
        // says, so the box warning is an addition rather than a replacement.
        assert!(seen.contains(&ArchiveWarning::XpsPage {
            page: 0,
            defect: XpsPageDefect::NotDrawn,
        }));
    }
}

/// A box on a page whose own size will not read is not arithmetic worth doing.
///
/// 10.3's rectangles are stated in the coordinate space of a page this one did
/// not state, and flipping one against a height the reader invented would put
/// the box somewhere the file never mentioned. `SizeUnusable` already says the
/// geometry is not what the file said.
#[test]
fn a_box_on_a_page_with_no_usable_size_is_not_written() {
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        &fixed_page_with("816", "", r#"ContentBox="96,48,192,96" "#),
    ));
    let document = open(&bytes).expect("an XPS");
    assert_eq!(crop_box(&document, 0), None);
    assert!(warnings(&document).contains(&ArchiveWarning::XpsPage {
        page: 0,
        defect: XpsPageDefect::SizeUnusable,
    }));
}

// ---- 7.2.3.5 routes the payload -----------------------------------------

/// **A fixed page is one because its media type says so**, not its name.
///
/// Both directions in one test, because they are the two halves of one rule and
/// a suite with only the first would pass on a reader that never looked at the
/// content-types item at all: every real payload part's extension agrees with
/// its media type, so extension-routing and 7.2.3.5 give the same answer on all
/// fifty-two parts of the corpus.
#[test]
fn a_fixed_page_is_routed_by_media_type_and_not_by_its_name() {
    // A part named `.txt`, declared a fixed page by an `Override`. It is a page.
    let types = content_types_with(
        r#"<Override PartName="/Documents/1/Pages/1.txt" ContentType="application/vnd.ms-package.xps-fixedpage+xml" />"#,
    );
    let parts = with(
        with(
            with(
                one_page_package(),
                "Documents/1/FixedDocument.fdoc",
                &document(&["Pages/1.txt"]),
            ),
            "Documents/1/Pages/1.txt",
            &fixed_page("816", "1056"),
        ),
        "[Content_Types].xml",
        &types,
    );
    let document = open(&archive(parts)).expect("an XPS whose page is named `.txt`");
    assert_eq!(document.page(0).map(|p| p.size()), Some((612.0, 792.0)));
    assert!(warnings(&document).contains(&ArchiveWarning::XpsPage {
        page: 0,
        defect: XpsPageDefect::NotDrawn,
    }));

    // And a part named `.fpage` that an `Override` says is something else. It
    // is not a page, and the extension does not rescue it.
    let types = content_types_with(
        r#"<Override PartName="/Documents/1/Pages/1.fpage" ContentType="text/plain" />"#,
    );
    let bytes = archive(with(one_page_package(), "[Content_Types].xml", &types));
    let document = open(&bytes).expect("the page still exists");
    assert_eq!(document.page_count(), 1);
    assert!(warnings(&document).contains(&ArchiveWarning::XpsPage {
        page: 0,
        defect: XpsPageDefect::MediaTypeMismatch,
    }));
}

/// A `PageContent` naming a part that is a real part and not a fixed page.
///
/// The page is kept and keeps its number, for the reason every other page-level
/// degradation here is kept: three references and two pages is a book with no
/// gap anywhere in it.
#[test]
fn a_page_content_naming_something_that_is_not_a_page_keeps_its_number() {
    let png = rgb_png(4, 6, &distinct_pixels(4, 6));
    let parts = before_content_types(
        with(
            one_page_package(),
            "Documents/1/FixedDocument.fdoc",
            &document(&["Pages/1.fpage", "/Resources/one.png", "Pages/1.fpage"]),
        ),
        binary_part("Resources/one.png", png),
    );
    let document = open(&archive(parts)).expect("an XPS");
    assert_eq!(document.page_count(), 3);
    assert_eq!(
        document
            .archive()
            .expect("a report")
            .pages()
            .get(1)
            .map(|p| p.name.as_str()),
        Some("/Resources/one.png")
    );
    assert!(warnings(&document).contains(&ArchiveWarning::XpsPage {
        page: 1,
        defect: XpsPageDefect::MediaTypeMismatch,
    }));
    // And the page it did not become is not the PNG paged as a comic would page
    // it: it is a placeholder at the book's own size, which is the difference
    // this whole gap is about.
    assert_eq!(document.page(1).map(|p| p.size()), Some((612.0, 792.0)));
}

/// And the same rule one level up, for a `DocumentReference`.
#[test]
fn a_document_reference_naming_something_that_is_not_a_document_is_named() {
    let bytes = archive(with(
        one_page_package(),
        "FixedDocumentSequence.fdseq",
        &sequence(&[
            "Documents/1/FixedDocument.fdoc",
            "Documents/1/Pages/1.fpage",
        ]),
    ));
    let document = open(&bytes).expect("an XPS");
    assert_eq!(document.page_count(), 2);
    assert!(warnings(&document).contains(&ArchiveWarning::XpsPage {
        page: 1,
        defect: XpsPageDefect::MediaTypeMismatch,
    }));
}

/// A payload part with no media type at all is not read as one either.
///
/// 7.2.3.5's third step is that a part may simply have no media type, which is
/// a real answer rather than a failure — and a part that never said what it is
/// is not a fixed page on the strength of having been pointed at.
#[test]
fn a_payload_part_with_no_media_type_is_not_a_page() {
    let bare = xps_support::CONTENT_TYPES.replace(
        r#"<Default Extension="fpage" ContentType="application/vnd.ms-package.xps-fixedpage+xml" />"#,
        "",
    );
    assert!(!bare.contains("fpage"), "the fixture really removed it");
    let bytes = archive(with(one_page_package(), "[Content_Types].xml", &bare));
    let document = open(&bytes).expect("the page still exists");
    assert!(warnings(&document).contains(&ArchiveWarning::XpsPage {
        page: 0,
        defect: XpsPageDefect::MediaTypeMismatch,
    }));
}

// ---- the placeholder is grey --------------------------------------------

/// Every page is the neutral 0xBF, and **not white**.
///
/// Row 4 asks for the placeholder rather than a blank page, and the two are
/// indistinguishable from every other test in this file: a page carrying no
/// content stream at all would have the right size, the right number, the right
/// warning and a white bitmap. Gap 17 is the whole of why that is not good
/// enough — a blank page returned as success is the failure this engine names
/// after itself — so the pixels are read.
///
/// 0xBF is the same grey the renderer paints over an image it could not decode
/// and the same one a comic placeholder carries, which is deliberate: a page
/// that is missing looks the same whichever level lost it.
#[test]
fn every_page_is_the_neutral_grey_rather_than_white() {
    // Small on purpose: 16 x 24 XPS units is 12 x 18 pt, so every pixel of the
    // page can be read rather than a sample of them.
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/Pages/1.fpage",
        &fixed_page("16", "24"),
    ));
    let document = open(&bytes).expect("an XPS");
    let bitmap = document
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    assert_eq!((bitmap.width, bitmap.height), (12, 18));
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            assert_eq!(pixel(&bitmap, x, y), (0xBF, 0xBF, 0xBF), "({x}, {y})");
        }
    }

    // And on a package this repository did not write, at a few points, because
    // a 612 x 792 render is 1.4 MB and reading all of it proves nothing more.
    let real = Document::open(corpus("wpf-three-pages.xps")).expect("a fixed document");
    let bitmap = real
        .page(2)
        .expect("the third page")
        .render(&RenderOptions::default());
    assert_eq!((bitmap.width, bitmap.height), (612, 792));
    for (x, y) in [(0, 0), (611, 0), (0, 791), (611, 791), (306, 396)] {
        assert_eq!(pixel(&bitmap, x, y), (0xBF, 0xBF, 0xBF), "({x}, {y})");
    }
}

// ---- what `cos()` lends -------------------------------------------------

/// `Document::cos` returns **the synthesised document**, page for page.
///
/// Gap 29 made that one signature the whole of its argument for synthesis over
/// an enum — `cos()` returns a borrow and not an `Option`, so a container that
/// had no object model to lend would have forced the one genuinely breaking
/// change this facade has. This is the assertion that the borrow is the same
/// document the facade is reporting on, rather than something beside it.
#[test]
fn the_object_model_cos_lends_is_the_document_that_was_synthesised() {
    let bytes = archive(out_of_order_package());
    let document = open(&bytes).expect("an XPS");
    let pages = tinker_pdf_cos::pages::collect(document.cos());
    assert_eq!(pages.len(), document.page_count() as usize);
    for (at, page) in pages.iter().enumerate() {
        let facade = document.page(at as u32).expect("a page");
        assert_eq!(
            (page.media_box.width(), page.media_box.height()),
            {
                let (x0, y0, x1, y1) = facade.media_box();
                (x1 - x0, y1 - y0)
            },
            "page {at}"
        );
    }
}

// ---- the markup is parsed once ------------------------------------------

/// Asking a part for its relationships twice **parses once**.
///
/// Milestone 3 cached the bytes and not the markup and recorded it as owed. The
/// counter exists because there is no other observable: `Archive::inflated` does
/// not move on a second parse of bytes it already handed over, so a test written
/// against the archive's budget would assert `n == n` and pass with the cache
/// deleted — which is gap 29's milestone-6 survivor exactly.
///
/// Both directions. The second ask parses nothing, and a **different** part
/// still does, so a counter that had simply stopped counting would fail here.
#[test]
fn asking_a_part_for_its_relationships_twice_parses_once() {
    let png = rgb_png(4, 6, &distinct_pixels(4, 6));
    let parts = before_content_types(
        before_content_types(
            one_page_package(),
            part(
                "Documents/1/Pages/_rels/1.fpage.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="R1" Type="http://schemas.microsoft.com/xps/2005/06/required-resource" Target="../../../Resources/one.png" /></Relationships>"#,
            ),
        ),
        binary_part("Resources/one.png", png),
    );
    let bytes = archive(parts);
    let mut package = opened(&bytes);
    let page = PartName::from_item("Documents/1/Pages/1.fpage").expect("a part name");

    assert_eq!(package.parses(), 0, "nothing is parsed at open");
    let first = package.relationships(Some(&page)).expect("the page's rels");
    assert_eq!(first.len(), 1);
    assert_eq!(package.parses(), 1);

    let again = package.relationships(Some(&page)).expect("the same rels");
    assert_eq!(first, again);
    assert_eq!(package.parses(), 1, "the second ask parses nothing");

    // A different part's relationships still cost a parse, so the counter is
    // measuring the cache rather than having stopped.
    package.relationships(None).expect("the package's rels");
    assert_eq!(package.parses(), 2);
    package.relationships(None).expect("and again");
    assert_eq!(package.parses(), 2);

    // The content-types item is the same story, and it is the one milestone 3
    // already cached — asserted here so the two caches are held to one rule.
    package.media_type(&page).expect("content types");
    assert_eq!(package.parses(), 3);
    for _ in 0..8 {
        package.media_type(&page).expect("content types");
    }
    assert_eq!(package.parses(), 3);
}

/// **A part named by a thousand references is parsed once.**
///
/// Nothing makes a reference and a part the same thing, and 12.3 lets one
/// `FixedDocument` name one `PageContent` `Source` as often as it likes.
/// `tinker_pdf_xml::Source::new` decodes the part and walks every character of
/// it against XML 1.0's `Char` production **before it yields an event**, so it
/// is O(part) whatever the caller stops at — and milestone 3 called it once per
/// reference. Four thousand pages naming one 128 MiB part is 512 GiB of
/// scanning out of a package that deflates to a few hundred kilobytes, which is
/// `5adf502`'s *"depth is not work once the recursion branches"* in a third
/// format.
///
/// Read as a count rather than as a clock, which is ruling 1's own rule about
/// how a bound is proved: `Archive::inflated` cannot see this, because the
/// **bytes** were already cached a layer down and it is the **parse** that
/// repeated.
///
/// Both directions. A thousand references to one part is three parses — the
/// sequence, the document, the page — and three references to three *distinct*
/// parts is five, so a counter that had simply stopped counting fails here.
#[test]
fn a_part_named_by_a_thousand_references_is_parsed_once() {
    let names: Vec<&str> = vec!["Pages/1.fpage"; 1_000];
    let bytes = archive(with(
        one_page_package(),
        "Documents/1/FixedDocument.fdoc",
        &document(&names),
    ));
    let document = open(&bytes).expect("an XPS");
    assert_eq!(document.page_count(), 1_000);
    let report = document.archive().expect("a report");
    assert_eq!(
        report.parsed_parts(),
        3,
        "the sequence, the fixed document and the one page part"
    );

    // Three distinct pages cost two more, so the count is the number of parts
    // and not a constant.
    let bytes = archive(out_of_order_package());
    let document = open(&bytes).expect("an XPS");
    assert_eq!(document.page_count(), 3);
    assert_eq!(
        document.archive().expect("a report").parsed_parts(),
        5,
        "the sequence, the fixed document and three distinct page parts"
    );

    // **The survivor the injection matrix found, and the leg that closes it.**
    //
    // There are two caches and they are two pieces of code: one for a part's
    // children and one for a fixed page's geometry. Both blocks above name one
    // `FixedDocument` **once**, so `Payload::children` was called once per
    // distinct part whether or not it cached — and deleting its cache failed
    // nothing in the workspace while `a_part_named_by_a_thousand_references_is_parsed_once`
    // was already written and passing. That is gap 29's milestone-5 lesson for
    // the third time in this gap: a positive assertion cannot catch a weakened
    // check, and one test cannot stand for two rules that share a sentence.
    //
    // 12.3.1 lets a `FixedDocumentSequence` name one `FixedDocument` as often as
    // it likes, and each reference is a whole document's worth of pages. Three
    // references to one document that holds one page is three pages, and three
    // parses: the sequence, the document once, and the page once.
    let bytes = archive(with(
        one_page_package(),
        "FixedDocumentSequence.fdseq",
        &sequence(&[
            "Documents/1/FixedDocument.fdoc",
            "Documents/1/FixedDocument.fdoc",
            "Documents/1/FixedDocument.fdoc",
        ]),
    ));
    let document = open(&bytes).expect("an XPS");
    assert_eq!(document.page_count(), 3, "one document shown three times");
    assert_eq!(
        document.archive().expect("a report").parsed_parts(),
        3,
        "the sequence, the one fixed document and the one page part"
    );
}

/// A `FixedDocumentSequence` naming more documents than the page cap allows is
/// refused, and one naming exactly the cap is not.
///
/// The page cap does not bound this loop on its own, which is the point: a
/// `DocumentReference` whose document holds no `PageContent` pushes no plan and
/// charges nothing, and `MAX_XML_TOKENS` lets one sequence hold a million of
/// them. Each costs a reference resolution and a lookup over every item in the
/// package, so a million of them is work a twenty-kilobyte part bought.
#[test]
fn a_sequence_naming_more_documents_than_the_page_cap_is_refused_by_name() {
    let empty = format!(
        r#"<FixedDocument xmlns="{}"></FixedDocument>"#,
        xps_support::XPS_NS
    );
    let refs: Vec<&str> = vec!["Documents/1/Empty.fdoc"; tinker_pdf::xps::MAX_XPS_PAGES + 1];
    let bytes = archive(with(
        with(
            one_page_package(),
            "FixedDocumentSequence.fdseq",
            &sequence(&refs),
        ),
        "Documents/1/Empty.fdoc",
        &empty,
    ));
    assert_eq!(refusal(&bytes), Some(ArchiveRefusal::TooLarge));

    // And one fewer is not refused for being large -- it holds no page, so it
    // reaches the refusal that is true about it instead.
    let refs: Vec<&str> = vec!["Documents/1/Empty.fdoc"; tinker_pdf::xps::MAX_XPS_PAGES];
    let bytes = archive(with(
        with(
            one_page_package(),
            "FixedDocumentSequence.fdseq",
            &sequence(&refs),
        ),
        "Documents/1/Empty.fdoc",
        &empty,
    ));
    assert_eq!(
        refusal(&bytes),
        Some(ArchiveRefusal::NoFixedPages),
        "exactly at the cap is walked, and every document in it holds no page"
    );
}

/// A comic archive parses no markup at all, which is a real answer.
///
/// The counter is on [`tinker_pdf::ArchiveReport`], which both synthesisers
/// fill in, so this is the assertion that the comic path did not quietly start
/// reading `ComicInfo.xml` — gap 29's named non-goal, and gap 30's second
/// correction says bringing an XML parser into the tree is not the same as
/// deciding the comic path should use it.
#[test]
fn a_comic_archive_parses_no_markup() {
    let png = rgb_png(4, 6, &distinct_pixels(4, 6));
    let bytes = xps_support::zip(
        &[
            xps_support::ZipFile::stored("001.png", &png),
            xps_support::ZipFile::stored(
                "ComicInfo.xml",
                br#"<?xml version="1.0"?><ComicInfo><Title>a</Title></ComicInfo>"#,
            ),
        ],
        xps_support::Damage::None,
    );
    let document = open(&bytes).expect("a comic archive");
    let report = document.archive().expect("a report");
    assert_eq!(report.xps_dialect(), None);
    assert_eq!(report.parsed_parts(), 0);
}

// ---- how narrow the invalid-name refusal is -----------------------------

/// **The decision milestone 3 left open, taken and pinned.**
///
/// Gap 30's design section says a package holding "a part name that is invalid,
/// duplicated or derivable from another" is refused at `open`, and milestone 3
/// flagged that as stricter than ruling 2 would be on its own: a lean reader
/// would leave the offending item unresolvable and open the document. It asked
/// row 4 to revisit it *"if a real file turns one up"*.
///
/// **No real file turns one up, so the refusal stands**, on the same footing as
/// the `.piece` refusal: tied to evidence rather than to taste. What this test
/// adds is the measurement that makes the decision defensible rather than
/// merely inherited — **the rule is much narrower than it looks**. The names a
/// general-purpose archiver leaves behind when a package is repacked are all
/// valid part names under 6.2.2.2, so none of them refuses anything:
/// `Thumbs.db`, `.DS_Store`, `__MACOSX/._x` and a percent-encoded space are
/// admitted, and the package opens.
///
/// What is refused is a name no conforming producer can write: a **raw** space,
/// a backslash, a percent-encoded unreserved character, a segment ending in a
/// dot. The raw space is the one worth naming, because it is the case a
/// third-party producer could plausibly reach — and it is the case that would
/// change this decision if one ever did.
#[test]
fn the_invalid_part_name_refusal_is_narrower_than_it_looks() {
    let harmless = [
        "Thumbs.db",
        "Documents/1/.DS_Store",
        "__MACOSX/._FixedDocumentSequence.fdseq",
        "Resources/a%20b.png",
    ];
    let mut parts = one_page_package();
    for name in harmless {
        parts = before_content_types(
            parts,
            part(name, "left behind by something that repacked it"),
        );
    }
    let document = open(&archive(parts)).unwrap_or_else(|e| {
        panic!("the names a repacking tool leaves behind are all part names: {e:?}")
    });
    assert_eq!(document.page_count(), 1);

    // And the refusal itself, one prohibition at a time, so "it refuses
    // everything" and "it refuses these" cannot be confused.
    for name in [
        "Resources/a b.png",    // a raw space: not a `pchar`
        "Resources/a\\b.png",   // a backslash, which is not a separator either
        "Resources/%41.png",    // a percent-encoded unreserved character
        "Resources/one./x.png", // a segment ending in a dot
    ] {
        let parts = before_content_types(one_page_package(), part(name, "not a part name"));
        assert_eq!(
            refusal(&archive(parts)),
            Some(ArchiveRefusal::InvalidPartName),
            "{name}"
        );
    }
}

// ---- reading a box back out of the written document ---------------------

fn page_rect(document: &Document, page: usize, key: &[u8]) -> Option<Vec<f64>> {
    let cos = document.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let reference = pages.get(page)?.reference;
    let object = cos.get(reference).ok()?;
    let dict = object.as_dict()?;
    let values = dict.get_array(cos.intern(key))?;
    Some(
        values
            .iter()
            .filter_map(tinker_pdf_cos::Object::as_number)
            .collect(),
    )
}

fn crop_box(document: &Document, page: usize) -> Option<Vec<f64>> {
    page_rect(document, page, b"CropBox")
}

fn bleed_box(document: &Document, page: usize) -> Option<Vec<f64>> {
    page_rect(document, page, b"BleedBox")
}
