//! The document a comic archive is synthesised into, held to ISO 32000.
//!
//! This replaces `cbz_qpdf.rs`, which asked qpdf whether the synthesised
//! document was a valid PDF and what its pages said. Ruling 13 retires that
//! question in that form; what stands in its place is the strict validator for
//! the structure, and raw dictionary reads for the values.
//!
//! **What is lost is worth naming.** qpdf was a reader nobody here wrote, and
//! its acceptance was evidence about the world. Nothing below is: a clause this
//! project misreads in the synthesiser and in the validator agrees with itself.
//! `docs/verification.md` states that in its own voice.
//!
//! What is *not* lost is the part gap 29's milestone 5 actually caught things
//! with: the page tree here is walked from the catalog's own `/Kids` and every
//! value is read out of a dictionary as the file spells it, so the tolerant
//! reader's defaults — a `/MediaBox` it would have invented, a `/Rect` it would
//! have normalised — cannot hide a writer's omission.

mod cbz_support;
mod validated_support;

use cbz_support::{distinct_pixels, grey_jpeg, rgb_png, zip, Damage, ZipFile};
use tinker_pdf::{cbz, Container, CosDocument, Defect, Document, WriteMode, WriteOptions};
use validated_support::{category, flat, numbers, pages, value};

/// Four pages whose stored order, lexicographic order and natural order all
/// disagree, and whose third page this build cannot decode.
fn comic() -> Vec<u8> {
    zip(
        &[
            ZipFile::stored("ComicInfo.xml", b"<?xml version=\"1.0\"?><ComicInfo/>"),
            ZipFile::stored("p10.png", &rgb_png(6, 4, &distinct_pixels(6, 4))),
            ZipFile::stored("p1.jpg", &grey_jpeg(16, 8)),
            ZipFile::deflated("p2.png", &rgb_png(5, 9, &distinct_pixels(5, 9))),
            ZipFile::stored("p3.gif", b"GIF89a\x04\x00\x04\x00\x00\x00\x00;"),
        ],
        Damage::None,
    )
}

#[track_caller]
fn valid(label: &str, bytes: &[u8]) -> CosDocument {
    let doc = CosDocument::open(bytes.to_vec()).expect("the document opens");
    let defects = tinker_pdf_cos::validate(&doc);
    assert!(
        defects.is_empty(),
        "{label}: {:?}",
        defects.iter().map(Defect::to_string).collect::<Vec<_>>()
    );
    doc
}

/// The document the synthesiser hands the parser is a valid PDF.
#[test]
fn a_synthesised_comic_validates() {
    let (pdf, report) =
        cbz::synthesise(Container::Zip, &comic(), &cbz::Limits::DEFAULT).expect("it synthesises");
    assert_eq!(report.pages().len(), 4);
    valid("the synthesised document", &pdf);
}

/// And so does the same document saved back through the writer, in both
/// layouts.
///
/// This is the exit criterion gap 29 wrote in as many words — `cos()` returns
/// the synthesised document, and saving it produces a file that reads clean.
#[test]
fn a_saved_synthesised_comic_validates() {
    let document = Document::open(comic()).expect("the archive opens");
    assert!(document.archive().is_some(), "it was synthesised");

    for linearize in [false, true] {
        let saved = document.editor().save(&WriteOptions {
            mode: WriteMode::Rewrite,
            linearize,
            ..WriteOptions::default()
        });
        let doc = valid(if linearize { "linearized" } else { "rewritten" }, &saved);
        if linearize {
            let first = doc
                .catalog()
                .map(|catalog| catalog.len())
                .expect("a catalog");
            assert!(first > 0, "the catalog survived the layout");
            assert!(
                doc.bytes().windows(11).any(|w| w == b"/Linearized"),
                "and the layout is the one that was asked for"
            );
        }
    }
}

/// The pages are there, in the order this build put them, at the sizes the
/// images are.
///
/// The structural check above reads whether the tree is walkable; this reads
/// what it *says*. A page tree that walks perfectly whose media boxes are not
/// the images' own pixel sizes, or whose pages are in the order the archive
/// stored them rather than the order a reader wants, passes that check and is
/// still the wrong document.
///
/// Both halves are read from the file: the `/MediaBox` from the page object,
/// and the dimensions from the image XObject's own `/Width` and `/Height`. They
/// are independent — a build that wrote a correct `/MediaBox` over an image
/// whose `/Width` disagreed would satisfy one and not the other, which is the
/// failure gap 29's milestone 4 found in the mask.
#[test]
fn the_pages_are_at_the_sizes_the_images_are() {
    let (pdf, _) =
        cbz::synthesise(Container::Zip, &comic(), &cbz::Limits::DEFAULT).expect("it synthesises");
    let doc = valid("the synthesised document", &pdf);

    let pages = pages(&doc);
    assert_eq!(pages.len(), 4, "four pages");

    // Natural order, which is the whole point of the fixture's names: stored
    // as p10, p1, p2, p3 and paged as p1, p2, p3, p10. Lexicographic order
    // would put the 6 x 4 page first.
    //
    // The placeholder takes the first real page's size, which is what a
    // comic's own shape asks for: pages of one size, and a page that cannot be
    // decoded should not make the book jump.
    let wanted = [
        [0.0, 0.0, 16.0, 8.0],
        [0.0, 0.0, 5.0, 9.0],
        [0.0, 0.0, 16.0, 8.0],
        [0.0, 0.0, 6.0, 4.0],
    ];
    for (index, (reference, page)) in pages.iter().enumerate() {
        let box_ = numbers(&doc, page, b"MediaBox")
            .unwrap_or_else(|| panic!("page {index} ({reference}) states no /MediaBox"));
        assert_eq!(box_, wanted[index], "page {index}'s /MediaBox");
    }

    // Three of the four carry an image, and the placeholder carries none —
    // which is what tells a placeholder from a page that merely rendered
    // badly.
    let sizes: Vec<(i64, i64)> = pages
        .iter()
        .flat_map(|(_, page)| category(&doc, page, b"XObject"))
        .filter_map(|(_, object)| {
            let dict = object.as_dict()?;
            Some((
                value(&doc, dict, b"Width").as_int()?,
                value(&doc, dict, b"Height").as_int()?,
            ))
        })
        .collect();
    assert_eq!(
        sizes,
        vec![(16, 8), (5, 9), (6, 4)],
        "the JPEG, the deflated PNG and the stored PNG, in page order"
    );

    // And the page that carries none is the third, the GIF this build cannot
    // decode.
    let placeholder = &pages[2].1;
    assert!(
        category(&doc, placeholder, b"XObject").is_empty(),
        "the placeholder page carries no image: {}",
        flat(&doc, &value(&doc, placeholder, b"Resources"))
    );
}
