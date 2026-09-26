//! An XHTML `<img>` as a replaced box, end to end: from a container entry to
//! an `/XObject` a reader can resolve.
//!
//! `crates/tinker-pdf-layout/src/tests.rs` proves CSS 2.2 §10.3.2, §10.6.2 and
//! §10.4's constraint table against intrinsic sizes a fixture states. This file
//! proves the other half, which no leaf crate can: that a *picture in a book*
//! reaches the page at the size **its own header** says it is, that the
//! `/XObject` it names is one the document actually holds, and that an `<img>`
//! which does not reach the page says so by name rather than leaving a hole the
//! surrounding text closes over.
//!
//! # Why the sizes here are read out of the fixture rather than written down
//!
//! A replaced box is the one box whose size comes from outside CSS, so a test
//! that hard-coded 48 by 18 points would pass against a build that had simply
//! memorised the fixture. Every size assertion below is computed from the
//! PNG's own `IHDR` or the JPEG's own `SOF`, times `PX_TO_PT` — so the claim is
//! *the picture's dimensions reached the page*, which is the claim the roadmap
//! row makes.
//!
//! # Counted injection, over `cargo test --no-fail-fast -p tinker-pdf`
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | **the replaced box painted nowhere — the bug this row closes** | 8 |
//! | the replaced box laid out but never emitted as a fragment | 9 |
//! | an undecodable picture produces no warning | 8 |
//! | EPUB RS §8.1's one page per itemref given up | 9 |
//! | every picture registered under one name | 2 |
//! | the classifier reads the file extension, not the magic bytes | 2 |
//! | a floating replaced box sent through shrink-to-fit | 2 |
//! | `width` stated and `height` auto handled as though both were stated | 1 |
//! | the picture drawn at the border box's corner, not the content box's | 1 |
//! | every page draws the first picture rather than its own | 1 |
//! | a refused `<img>` becomes a 300 × 150 box of nothing | 1 |
//!
//! **Three rows here caught nothing the first time**, and each was a claim no
//! fixture made: that a refused `<img>` generates *no box* rather than an empty
//! one (HTML §4.8.4.4 decides it, and nothing asserted it); that a picture is
//! classified by its magic bytes and never by its name (every fixture named its
//! pictures correctly); and, in the layout crate, that a picture is drawn once
//! per *box* rather than once per fragment. All three have fixtures now. The
//! layout crate's own table is at the head of its replaced-box section in
//! `crates/tinker-pdf-layout/src/tests.rs`.

mod cbz_support;
mod epub_support;

use std::collections::BTreeSet;

use cbz_support::{distinct_pixels, grey_jpeg, rgb_png};
use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::cbz::{ImageDefect, ImageFormat};
use tinker_pdf::epub::read::PX_TO_PT;
use tinker_pdf::epub::{DEFAULT_PAGE, PAGE_MARGIN};
use tinker_pdf::{ArchiveWarning, Document, OpenOptions, RenderOptions};

// ---- fixtures ---------------------------------------------------------------

const CONTAINER: &str = concat!(
    r##"<?xml version="1.0" encoding="utf-8"?>"##,
    r##"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"##,
    r##"<rootfiles><rootfile full-path="EPUB/content.opf" "##,
    r##"media-type="application/oebps-package+xml"/></rootfiles></container>"##
);

/// A reflowable book of one chapter, with whatever resources a test needs
/// beside it.
///
/// `body` is markup rather than text so a caller can state the `<img>` itself,
/// which is the element every test here is about. The one rule the sheet
/// carries is `body { margin: 0 }`, so a fragment's coordinates are the frame's
/// and a test can compute where a picture must land instead of measuring where
/// it did.
fn book(body: &str, resources: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut items = String::new();
    for (at, (name, _)) in resources.iter().enumerate() {
        let media = match name.rsplit('.').next().unwrap_or("") {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            _ => "application/octet-stream",
        };
        items.push_str(&format!(
            r#"<item id="r{at}" href="{name}" media-type="{media}"/>"#
        ));
    }
    let package = format!(
        concat!(
            r##"<?xml version="1.0" encoding="utf-8"?>"##,
            r##"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" "##,
            r##"unique-identifier="id"><metadata "##,
            r##"xmlns:dc="http://purl.org/dc/elements/1.1/">"##,
            r##"<dc:identifier id="id">urn:uuid:1f0c2c1e-0000-4000-8000-00000000001d"##,
            r##"</dc:identifier><dc:title>A Picture Book</dc:title>"##,
            r##"<dc:language>en</dc:language></metadata><manifest>"##,
            r##"<item id="c" href="ch1.xhtml" media-type="application/xhtml+xml"/>"##,
            r##"{items}</manifest><spine><itemref idref="c"/></spine></package>"##
        ),
        items = items
    );
    let chapter = format!(
        concat!(
            r##"<?xml version="1.0" encoding="utf-8"?>"##,
            r##"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>One</title>"##,
            r##"<style>body {{ margin: 0 }} p {{ margin: 0 }}</style></head>"##,
            r##"<body>{body}</body></html>"##
        ),
        body = body
    );
    let mut entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", package.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()),
    ];
    for (name, bytes) in resources {
        entries.push(OcfEntry::deflated(&format!("EPUB/{name}"), bytes));
    }
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

fn open(body: &str, resources: &[(&str, Vec<u8>)]) -> Document {
    Document::open_with(book(body, resources), &OpenOptions::default()).expect("the book opens")
}

/// A picture whose every pixel differs from every other, so a page drawn from
/// the wrong one of two is visible.
fn plate(width: u32, height: u32) -> Vec<u8> {
    rgb_png(width, height, &distinct_pixels(width, height))
}

/// PNG 11.2.2: `IHDR` is the first chunk after the eight-byte signature, and
/// its data — length and type ahead of it — begins at offset 16.
///
/// **The fixture's own header and not the numbers it was built from**: this is
/// what makes every size assertion below a claim about the file rather than
/// about the test.
fn png_shape(png: &[u8]) -> (u32, u32) {
    assert_eq!(&png[12..16], b"IHDR", "the first chunk is not IHDR");
    let width = u32::from_be_bytes(png[16..20].try_into().expect("four bytes"));
    let height = u32::from_be_bytes(png[20..24].try_into().expect("four bytes"));
    (width, height)
}

/// A CSS-pixel pair as the points a page measures in.
fn points((width, height): (u32, u32)) -> (f64, f64) {
    (f64::from(width) * PX_TO_PT, f64::from(height) * PX_TO_PT)
}

// ---- reading a page ---------------------------------------------------------

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

/// Every image placement on a page: the `cm` matrix that maps 8.9.5.2's unit
/// square onto the picture's rectangle, and the resource name drawn through it.
///
/// Read out of the operators rather than off the rendered page, because the two
/// answer different questions and this file asks both: the operators say where
/// the painter put the picture, and only a render says whether the name it used
/// resolves to anything.
fn placements(content: &str) -> Vec<([f64; 6], String)> {
    let tokens: Vec<&str> = content.split_whitespace().collect();
    let mut out = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        if *token != "Do" {
            continue;
        }
        let name = tokens[at - 1].to_owned();
        assert_eq!(tokens[at - 2], "cm", "a `Do` with no placement before it");
        let mut matrix = [0.0f64; 6];
        for (i, slot) in matrix.iter_mut().enumerate() {
            slot.clone_from(&tokens[at - 8 + i].parse::<f64>().expect("a number"));
        }
        out.push((matrix, name));
    }
    out
}

/// The one image placement on a page.
fn only_placement(doc: &Document, at: usize) -> ([f64; 6], String) {
    let content = page_content(doc, at);
    let found = placements(&content);
    assert_eq!(found.len(), 1, "page {at}: {content}");
    found.into_iter().next().expect("one placement")
}

/// How many distinct colours a rendered page has. One is a blank page.
fn colours(doc: &Document, at: u32) -> usize {
    let bitmap = doc
        .page(at)
        .expect("a page")
        .render(&RenderOptions::default());
    let components = bitmap.components();
    let mut seen: BTreeSet<&[u8]> = BTreeSet::new();
    for y in 0..bitmap.height as usize {
        for x in 0..bitmap.width as usize {
            let start = y * bitmap.stride + x * components;
            if let Some(pixel) = bitmap.data.get(start..start + components) {
                seen.insert(pixel);
            }
        }
    }
    seen.len()
}

// ---- the intrinsic size -----------------------------------------------------

/// **A reflowable `<img>` is a replaced box at the picture's own size**, which
/// is the roadmap row's second exit criterion.
///
/// CSS 2.2 §10.3.2's first case and §10.6.2's first: neither `width` nor
/// `height` is stated, so the used size is the intrinsic size — and
/// `css-images-3` §4.1 makes a raster's intrinsic size its pixel dimensions,
/// since nothing in an EPUB sets a density.
///
/// The expected numbers come from the fixture's own `IHDR`. A build that read
/// the manifest's `media-type` instead of the bytes, or that took the box from
/// the containing block, cannot produce them by accident.
#[test]
fn a_reflowable_img_is_a_replaced_box_at_the_pictures_own_size() {
    let png = plate(64, 24);
    let (width, height) = points(png_shape(&png));
    let doc = open(
        r#"<img src="pic.png" style="display: block"/>"#,
        &[("pic.png", png.clone())],
    );
    let (matrix, name) = only_placement(&doc, 0);
    assert_eq!((matrix[0], matrix[3]), (width, height));
    // A picture at the top left of a body with no margin: §8.1's content box is
    // the frame's own origin, and 8.9.5.2's unit square is mapped by its
    // **bottom** left corner, so the y is the content top less the height.
    assert_eq!(matrix[4], PAGE_MARGIN);
    assert_eq!(matrix[5], DEFAULT_PAGE.1 - PAGE_MARGIN - height);
    // The shear terms are zero: a picture is placed, never rotated or skewed.
    assert_eq!((matrix[1], matrix[2]), (0.0, 0.0));
    assert!(name.starts_with("/Im"), "{name}");
}

/// The same for a JPEG, whose intrinsic size comes from its `SOF` rather than
/// from an `IHDR`.
///
/// Two formats and not one, because the two take different readers — the PNG
/// goes through `png_image` and the JPEG through `jpeg_shape` — and a build
/// that sized one of them from the containing block would still pass the
/// fixture above.
#[test]
fn a_jpeg_img_is_a_replaced_box_at_the_jpegs_own_size() {
    // `grey_jpeg` needs multiples of eight; 56 by 40 is not square, so a build
    // that transposed the two axes cannot pass.
    let jpeg = grey_jpeg(56, 40);
    let (width, height) = points((56, 40));
    let doc = open(
        r#"<img src="pic.jpg" style="display: block"/>"#,
        &[("pic.jpg", jpeg)],
    );
    let (matrix, _) = only_placement(&doc, 0);
    assert_eq!((matrix[0], matrix[3]), (width, height));
}

/// A stated `width` scales the height by the picture's **own** ratio, §10.6.2's
/// second case, all the way through a book.
///
/// Half the picture's width, so half its height — and not the intrinsic height,
/// which is what a build that read a stated `width` as though both dimensions
/// were stated would draw.
#[test]
fn a_stated_width_scales_the_height_by_the_pictures_own_ratio() {
    let png = plate(64, 24);
    let (width, height) = points(png_shape(&png));
    let doc = open(
        &format!(
            r#"<img src="pic.png" style="display: block; width: {}px"/>"#,
            64 / 2
        ),
        &[("pic.png", png)],
    );
    let (matrix, _) = only_placement(&doc, 0);
    assert_eq!((matrix[0], matrix[3]), (width / 2.0, height / 2.0));
}

/// **A picture is classified by its bytes and never by its name.**
///
/// A `.jpg` that is a PNG is routine — every producer that renamed a file has
/// written one — and an extension is a claim where the first bytes of a file
/// are a fact. The manifest's `media-type` is the same kind of claim, which is
/// why neither is consulted: this book declares `image/png` in its manifest,
/// names the entry `pic.png`, and fills it with a JPEG.
///
/// The size is the one that says which reader ran. A build that believed the
/// name would put the bytes through `png_image`, which refuses them, and the
/// picture would be `Undecodable` instead of drawn.
///
/// **Found by a counted injection that caught nothing.** Making the classifier
/// read the extension left every fixture here green, in a file whose every
/// other fixture names its pictures correctly.
#[test]
fn a_picture_is_classified_by_its_bytes_and_never_by_its_name() {
    let doc = open(
        r#"<img src="pic.png" style="display: block"/>"#,
        &[("pic.png", grey_jpeg(56, 40))],
    );
    assert_eq!(not_drawn(&doc), []);
    let (matrix, _) = only_placement(&doc, 0);
    assert_eq!((matrix[0], matrix[3]), points((56, 40)));
}

/// The same the other way: a `.png` holding a format this build does not read
/// is named by **that** format, not by the one its name claims.
#[test]
fn a_misnamed_unsupported_picture_is_named_by_the_format_it_really_is() {
    let mut gif = Vec::from(*b"GIF89a");
    gif.extend_from_slice(&[8, 0, 8, 0, 0x80, 0, 0]);
    let doc = open(r#"<p>a<img src="pic.png"/>b</p>"#, &[("pic.png", gif)]);
    assert_eq!(
        not_drawn(&doc),
        [(ImageDefect::UnsupportedFormat(ImageFormat::Gif), 1)],
        "a GIF named .png was classified by its name"
    );
}

/// §9.2.2's atomic inline-level box: an `<img>` with no `display` declaration
/// sits **on the line** beside its text, not on a line of its own.
///
/// The picture is 16 by 12 and the paragraph around it is ordinary prose, so a
/// build that gave the picture a block of its own would put its placement at
/// the left margin. This asserts it is past it.
#[test]
fn an_inline_img_sits_on_the_line_beside_its_text() {
    let png = plate(16, 12);
    let doc = open(
        r#"<p>before <img src="pic.png"/> after</p>"#,
        &[("pic.png", png)],
    );
    let (matrix, _) = only_placement(&doc, 0);
    assert!(
        matrix[4] > PAGE_MARGIN,
        "the picture is at the margin rather than on the line: {matrix:?}"
    );
}

// ---- the picture is really in the document ----------------------------------

/// **The page is more than one colour**, which is the claim no content stream
/// can make.
///
/// `DocumentBuilder::begin_page` snapshots the document's resource set, so an
/// `/XObject` registered after the page begins leaves a `Do` whose name
/// resolves to nothing — a content stream that looks exactly right over a page
/// that is still blank. Only a render tells those two apart, which is why this
/// measurement is in pixels.
///
/// Measured 15 September 2026: **865** colours over a 64 by 24 plate whose
/// every pixel differs from every other, against **1** for the same book with
/// the painter's `draw_replaced` removed.
#[test]
fn the_page_a_reflowable_img_is_on_is_more_than_one_colour() {
    let doc = open(
        r#"<img src="pic.png" style="display: block"/>"#,
        &[("pic.png", plate(64, 24))],
    );
    let count = colours(&doc, 0);
    assert!(count > 1, "a page of one colour is a blank page: {count}");
}

/// Two pictures in one document are two `/XObject`s, and each page draws **its
/// own**.
///
/// One name per picture across the document rather than `cbz.rs`'s single `/Im`
/// per page: a book's page may hold several pictures and two of them sharing a
/// name would make the second overwrite the first. Two differently-sized plates
/// so the placements say which is which.
#[test]
fn two_pictures_in_one_document_are_two_resources() {
    let first = plate(64, 24);
    let second = plate(32, 40);
    let doc = open(
        concat!(
            r#"<img src="a.png" style="display: block"/>"#,
            r#"<img src="b.png" style="display: block"/>"#
        ),
        &[("a.png", first.clone()), ("b.png", second.clone())],
    );
    let content = page_content(&doc, 0);
    let found = placements(&content);
    assert_eq!(found.len(), 2, "{content}");
    let names: BTreeSet<&str> = found.iter().map(|(_, name)| name.as_str()).collect();
    assert_eq!(names.len(), 2, "two pictures share one name: {content}");
    assert_eq!(
        (found[0].0[0], found[0].0[3]),
        points(png_shape(&first)),
        "{content}"
    );
    assert_eq!(
        (found[1].0[0], found[1].0[3]),
        points(png_shape(&second)),
        "{content}"
    );
}

/// A picture is drawn **inside** its border and padding, CSS 2.2 §8.1.
///
/// The one number nothing else here pins: a build that drew at the border box's
/// corner puts every bordered figure up and to the left of where it belongs, by
/// an amount no size assertion can see.
#[test]
fn a_picture_is_drawn_inside_its_border_and_padding() {
    let png = plate(64, 24);
    let (_, height) = points(png_shape(&png));
    let doc = open(
        r#"<img src="pic.png" style="display: block; border: 8px solid #000; padding: 4px"/>"#,
        &[("pic.png", png)],
    );
    let (matrix, _) = only_placement(&doc, 0);
    let inset = 12.0 * PX_TO_PT;
    assert_eq!(matrix[4], PAGE_MARGIN + inset);
    assert_eq!(matrix[5], DEFAULT_PAGE.1 - PAGE_MARGIN - inset - height);
}

// ---- ruling 10: the `<img>` that did not reach the page ---------------------

/// Every [`ArchiveWarning::ImageNotDrawn`] a book reported, as the pair a test
/// cares about.
fn not_drawn(doc: &Document) -> Vec<(ImageDefect, usize)> {
    warnings(doc)
        .into_iter()
        .filter_map(|warning| match warning {
            ArchiveWarning::ImageNotDrawn { defect, images, .. } => Some((defect, images)),
            _ => None,
        })
        .collect()
}

/// An `<img>` whose `src` names nothing in the container is **named**, and
/// generates no box.
///
/// HTML §4.8.4.4 makes an element *"expected to be treated as a replaced
/// element"* only when the image is available, so an unavailable one generates
/// no box and the text around it closes over the gap — nothing about the page
/// says a picture was meant to be there. That invisibility is precisely why
/// this warning exists, and it is the ruling 10 gap the roadmap row names:
/// `SvgImageUnresolved` is an SVG `<image>` and could never say this.
#[test]
fn an_img_whose_src_resolves_to_nothing_is_named() {
    let doc = open(r#"<p>a<img src="missing.png"/>b</p>"#, &[]);
    assert_eq!(not_drawn(&doc), [(ImageDefect::Unresolved, 1)]);
    assert_eq!(placements(&page_content(&doc, 0)), []);
}

/// An `<img>` with no `src` at all is the same defect: there is no reference to
/// resolve.
#[test]
fn an_img_with_no_src_at_all_is_unresolved() {
    let doc = open(r#"<p>a<img/>b</p>"#, &[]);
    assert_eq!(not_drawn(&doc), [(ImageDefect::Unresolved, 1)]);
}

/// A **core media type** this build has no decoder for is named by its format,
/// not collapsed into "unresolved".
///
/// EPUB 3.3 §3.2 makes GIF and WebP core image media types a conforming book
/// may use with no fallback, so a reader meeting one has met a legal book it
/// cannot draw — a different sentence from a broken reference, and a host acts
/// on the two differently.
#[test]
fn a_core_media_type_with_no_decoder_here_is_named_by_its_format() {
    let mut gif = Vec::from(*b"GIF89a");
    gif.extend_from_slice(&[8, 0, 8, 0, 0x80, 0, 0]);
    let doc = open(r#"<p>a<img src="pic.gif"/>b</p>"#, &[("pic.gif", gif)]);
    assert_eq!(
        not_drawn(&doc),
        [(ImageDefect::UnsupportedFormat(ImageFormat::Gif), 1)]
    );
}

/// An SVG in an `<img>` lands in `Unknown`, and that is where it belongs.
///
/// An SVG is XML and carries no magic number, so the classifier that refuses to
/// read an extension as a fact cannot recognise one. `<img src="cover.svg">` is
/// a core media type this build places only as a **spine item**, never as a
/// replaced box, and the warning says the bytes were unrecognisable rather than
/// claiming a format it did not identify.
#[test]
fn an_svg_in_an_img_is_unknown_rather_than_a_format() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"/>"#.to_vec();
    let doc = open(r#"<p>a<img src="pic.svg"/>b</p>"#, &[("pic.svg", svg)]);
    assert_eq!(not_drawn(&doc), [(ImageDefect::Unknown, 1)]);
}

/// **A picture this build recognises and cannot decode is named `Undecodable`**
/// — the fourth arm, and the one that is this build's fault or the bytes'.
///
/// A PNG signature with no `IHDR` behind it: `image_format` says PNG, and the
/// reader then refuses it. A build that reported nothing here would draw a page
/// with a hole in it and tell a host the book was read.
#[test]
fn a_png_that_will_not_decode_is_named_undecodable() {
    let doc = open(
        r#"<p>a<img src="pic.png"/>b</p>"#,
        &[("pic.png", cbz_support::broken_png())],
    );
    assert_eq!(not_drawn(&doc), [(ImageDefect::Undecodable, 1)]);
    assert_eq!(placements(&page_content(&doc, 0)), []);
}

/// A JPEG whose `SOF` cannot be found is the same.
#[test]
fn a_jpeg_that_will_not_decode_is_named_undecodable() {
    let doc = open(
        r#"<p>a<img src="pic.jpg"/>b</p>"#,
        &[("pic.jpg", vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0, 0])],
    );
    assert_eq!(not_drawn(&doc), [(ImageDefect::Undecodable, 1)]);
}

/// Every `<img>` that failed the same way in one document is **one** warning
/// with a count, not one warning each.
///
/// `UnimplementedProperty`'s own reason: a comic whose forty pictures are all
/// WebP is one sentence a host can act on, and forty identical warnings is not.
/// Two defects in one document so the grouping is by defect and not merely by
/// item.
#[test]
fn images_that_failed_the_same_way_are_one_warning_with_a_count() {
    let doc = open(
        concat!(
            r#"<p><img src="a.png"/><img src="b.png"/><img src="c.png"/>"#,
            r#"<img src="gone.png"/></p>"#
        ),
        &[
            ("a.png", cbz_support::broken_png()),
            ("b.png", cbz_support::broken_png()),
            ("c.png", cbz_support::broken_png()),
        ],
    );
    let mut reported = not_drawn(&doc);
    reported.sort_by_key(|(_, images)| *images);
    assert_eq!(
        reported,
        [(ImageDefect::Unresolved, 1), (ImageDefect::Undecodable, 3)]
    );
}

/// A book whose every `<img>` drew reports **nothing**, which is what makes
/// every assertion above a measurement rather than a warning that always fires.
#[test]
fn a_book_whose_pictures_all_drew_reports_nothing_about_them() {
    let doc = open(
        r#"<img src="pic.png" style="display: block"/>"#,
        &[("pic.png", plate(64, 24))],
    );
    assert_eq!(not_drawn(&doc), []);
}

/// The warning names the **content document** the `<img>` is in, so a host with
/// a two-hundred-item spine is told which file to look at.
#[test]
fn the_warning_names_the_content_document() {
    let doc = open(r#"<p>a<img src="missing.png"/>b</p>"#, &[]);
    let items: Vec<String> = warnings(&doc)
        .into_iter()
        .filter_map(|warning| match warning {
            ArchiveWarning::ImageNotDrawn { item, .. } => Some(item),
            _ => None,
        })
        .collect();
    assert_eq!(items, ["EPUB/ch1.xhtml"]);
}

/// **A refused `<img>` generates no box at all**, and that is HTML §4.8.4.4
/// rather than a shortcut.
///
/// This is the settlement the row asks for in so many words: *a box of the
/// right size with nothing in it is a different degradation from no box*. The
/// specification decides it — an element is *"expected to be treated as a
/// replaced element"* **only when the image is available** — and an
/// unavailable one is therefore an ordinary empty inline, which generates no
/// box and costs no space. The alternative, CSS 2.2 §10.3.2's 300 by 150
/// default, would put a blank rectangle the size of a postcard into the middle
/// of a paragraph for a reference that was merely misspelled.
///
/// It also keeps text conservation an equality: an empty box that carried
/// `alt` would put characters on the page that the spine's markup does not
/// contain, one per refused image, with no source character to answer them.
///
/// Asserted as an **identity against an empty inline element**, which is what
/// §4.8.4.4 says the refused `<img>` has become. Byte for byte, because a
/// weaker claim is one a 300 by 150 box also satisfies: an empty rectangle
/// draws nothing either, and only its *geometry* gives it away.
///
/// **Found by a counted injection that caught nothing.** Making a refused
/// `<img>` a `300 × 150` replaced box left every fixture here green.
#[test]
fn a_refused_img_generates_no_box_and_costs_no_space() {
    let refused = open(r#"<p>ab<img src="missing.png"/>cd</p>"#, &[]);
    let empty = open(r#"<p>ab<span></span>cd</p>"#, &[]);
    assert_eq!(page_content(&refused, 0), page_content(&empty, 0));
    // And the text is still one contiguous word, so the two boxes agree about
    // nothing rather than about the same wrong thing: an element boundary
    // splits the run into two text objects, and the second begins exactly where
    // the first left off.
    let whole = open(r#"<p>abcd</p>"#, &[]);
    assert_eq!(refused.page_count(), whole.page_count());
    assert_eq!(
        refused.page(0).expect("a page").text().plain_text(),
        whole.page(0).expect("a page").text().plain_text()
    );
    // The `<img>` really was refused, so this is a book that met one.
    assert_eq!(not_drawn(&refused), [(ImageDefect::Unresolved, 1)]);
    assert_eq!(not_drawn(&empty), []);
}

// ---- ruling 2: degrade, do not fail ----------------------------------------

/// **A book of nothing but broken pictures is still a book.** Ruling 2.
///
/// Every `<img>` refused, every warning reported, and the text around them
/// still set on a page — which is the degradation this build chooses over a
/// refusal, and the one HTML §4.8.4.4 describes: an unavailable image is not a
/// replaced element, so the line closes over it.
#[test]
fn a_book_whose_pictures_all_failed_still_opens_and_sets_its_text() {
    let doc = open(
        r#"<p>before<img src="missing.png"/>after</p>"#,
        &[("pic.png", cbz_support::broken_png())],
    );
    assert_eq!(doc.page_count(), 1);
    let text = doc.page(0).expect("a page").text().plain_text();
    assert!(text.contains("before") && text.contains("after"), "{text}");
}
