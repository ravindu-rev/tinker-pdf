//! A standalone SVG, a bare image and a loose XHTML file, each opened as a
//! document by `Document::open` (tier 5's formats row).
//!
//! Each of the three had a reader in the tree and was refused as not-a-PDF.
//! What this file holds is the routing and the claim the module comment of
//! `tinker_pdf::standalone` makes about it: **a loose document is laid out by
//! the code that lays out the larger document it would be one part of.** So
//! the strongest test here is not a picture of the right answer but an
//! equality — a loose XHTML file renders to exactly the pixels the same file
//! does as the one chapter of an EPUB, and a bare PNG to exactly the pixels it
//! does as the one page of a comic archive. Pictures with known pixels are
//! there too, because two blank pages are equal.

mod cbz_support;
mod epub_support;
mod render_support;

use std::sync::Arc;

use cbz_support::{broken_png, distinct_pixels, grey_jpeg, rgb_png, zip, Damage, ZipFile};
use epub_support::{ocf_zip, OcfEntry};
use render_support::{curvy_font, ink};
use tinker_pdf::cbz::{ImageDefect, ImageFormat, PageDefect};
use tinker_pdf::epub::read::PX_TO_PT;
use tinker_pdf::epub::{SpineDefect, DEFAULT_PAGE};
use tinker_pdf::{
    ArchiveWarning, Bitmap, Document, OpenError, OpenOptions, RenderOptions, ShreddedSource,
    SimpleFontProvider, SliceSource, Standalone,
};

// ---- helpers -----------------------------------------------------------------

fn open(bytes: &[u8]) -> Document {
    Document::open(bytes.to_vec()).expect("the document opens")
}

fn render(document: &Document, page: u32) -> Bitmap {
    document
        .page(page)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// `document` with a face to draw its text in.
///
/// A document that embeds no face draws **none** of its text without one — it
/// extracts perfectly and renders `UnreadableFont` and a blank page — so two
/// pages compared without it are two blank pages, equal whatever was laid out
/// on them. The face is `render_support`'s synthetic one, attached after
/// pagination: the line breaks are the ones `open` made from the built-in
/// metrics, on both sides of a comparison, and only the glyphs are its.
fn drawn(document: Document) -> Document {
    document.with_fonts(Arc::new(SimpleFontProvider::new(curvy_font())))
}

/// Pixels that are not white: what says a comparison compared something.
const LEAST_INK: usize = 200;

fn pixel(bitmap: &Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[0, 0, 0]);
    (p[0], p[1], p[2])
}

fn warnings(document: &Document) -> Vec<ArchiveWarning> {
    document
        .archive()
        .expect("a synthesised document has a report")
        .warnings()
        .to_vec()
}

fn page_text(document: &Document, page: u32) -> String {
    document
        .page(page)
        .expect("a page")
        .text()
        .plain_text()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// RFC 4648 §4's base64, written out here so a `data:` URL can carry a
/// fixture; the decoder under test is held to the RFC's own vectors in
/// `standalone.rs`'s unit tests, not to this.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let group = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((group >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A whole XHTML document around `body`, with `head` inside its `<head>`.
fn xhtml(head: &str, body: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head>{head}</head>"#,
            r#"<body>{body}</body></html>"#
        ),
        head = head,
        body = body
    )
}

/// An EPUB whose one chapter is `chapter`, byte for byte.
fn book_of(chapter: &str) -> Vec<u8> {
    let container = concat!(
        r#"<?xml version="1.0"?><container version="1.0" "#,
        r#"xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles>"#,
        r#"<rootfile full-path="content.opf" media-type="application/oebps-package+xml"/>"#,
        r#"</rootfiles></container>"#
    );
    let package = concat!(
        r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" "#,
        r#"version="3.0" unique-identifier="id"><metadata "#,
        r#"xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier>"#,
        r#"<dc:title>t</dc:title><dc:language>en</dc:language></metadata><manifest>"#,
        r#"<item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest>"#,
        r#"<spine><itemref idref="c"/></spine></package>"#
    );
    let entries = [
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", container.as_bytes()),
        OcfEntry::deflated("content.opf", package.as_bytes()),
        OcfEntry::deflated("c.xhtml", chapter.as_bytes()),
    ];
    ocf_zip(&entries, &[0, 1, 2, 3])
}

/// Three paragraphs long enough to need two pages at the default box.
fn long_body() -> String {
    let words = "lorem ipsum dolor sit amet consectetur adipiscing elit ".repeat(60);
    format!(
        "<h1>Heading</h1><p>{words}</p><p id=\"far\">second {words}</p>\
         <p><a href=\"#far\">to the second paragraph</a></p>"
    )
}

// ---- the sniff, at the door --------------------------------------------------

/// **What used to be refused as not-a-PDF opens, and what is not one of the
/// three still is not a document.**
#[test]
fn the_three_open_and_everything_else_is_still_refused() {
    assert_eq!(
        tinker_pdf::standalone::sniff(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
        Some(Standalone::Svg)
    );
    for bytes in [
        b"this is not a pdf at all".as_slice(),
        b"<FixedPage xmlns=\"http://schemas.microsoft.com/xps/2005/06\"/>",
        b"<?xml version=\"1.0\"?><!-- an svg that never starts",
        b"BM a two-byte signature, which is not sniffed",
    ] {
        assert_eq!(
            Document::open(bytes.to_vec()).err(),
            Some(OpenError::NotAPdf),
            "{:?}",
            String::from_utf8_lossy(bytes)
        );
    }
}

/// **A PDF with junk in front of it stays the PDF wherever the parser would
/// have found its header**, whatever the junk looks like.
///
/// The COS parser looks for `%PDF-` in the first
/// `tinker_pdf_cos::limits::MAX_HEADER_SCAN` bytes and opens what follows as
/// the PDF, the leading junk shifting every offset by its own length. A sniff
/// that looked in fewer bytes turned a PDF behind 1 500 bytes of junk that
/// began like a JPEG, an SVG or an HTML file into a synthesised placeholder,
/// a picture that would not read or a truncated page — so the window is the
/// parser's own, and this holds it at its far edge too, buffered and streamed.
#[test]
fn a_pdf_behind_junk_is_the_pdf_wherever_the_parser_finds_its_header() {
    let mut builder = tinker_pdf::DocumentBuilder::new();
    for _ in 0..2 {
        builder.add_page(200.0, 100.0, |_| {});
    }
    let pdf = builder.finish();
    let window = tinker_pdf_cos::limits::MAX_HEADER_SCAN;
    for prefix in [
        b"\xFF\xD8\xFF\xE0".as_slice(),
        b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\">",
        b"<html><body>",
        b"<!DOCTYPE html>",
    ] {
        // 1 500 bytes in, and as far in as a header that still ends inside the
        // window the parser searches.
        for junk in [1_500, window - 5] {
            let mut bytes = prefix.to_vec();
            bytes.resize(junk, b' ');
            bytes.extend_from_slice(&pdf);
            let what = format!("{:?} + {junk}", String::from_utf8_lossy(prefix));
            assert_eq!(tinker_pdf::standalone::sniff(&bytes), None, "{what}");
            let buffered = open(&bytes);
            assert!(buffered.archive().is_none(), "{what}: synthesised");
            assert_eq!(buffered.page_count(), 2, "{what}");
            let source = Arc::new(ShreddedSource::new(SliceSource::new(bytes)));
            let streamed = Document::open_streaming(source).expect("it opens streamed");
            assert!(
                streamed.archive().is_none(),
                "{what}: streamed, synthesised"
            );
            assert_eq!(streamed.page_count(), 2, "{what}: streamed");
        }
    }
}

// ---- a standalone SVG --------------------------------------------------------

const RED_SQUARE: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">"##,
    r##"<rect x="0" y="0" width="100" height="100" fill="#ff0000"/>"##,
    r##"<text x="110" y="60" font-family="monospace" font-size="20">Hello</text></svg>"##
);

/// **An SVG is one page, the size its root states**, with the picture on it
/// and its text in `Page::text()`.
///
/// 200 × 100 CSS pixels are 150 × 75 points (CSS 2.2 §4.3.2's 96 to 72). The
/// red square covers the left half; the right half is the page's white.
#[test]
fn an_svg_is_one_page_at_the_size_its_root_states() {
    let document = open(RED_SQUARE.as_bytes());
    assert_eq!(document.page_count(), 1);
    assert_eq!(
        document.page(0).expect("a page").size(),
        (200.0 * PX_TO_PT, 100.0 * PX_TO_PT)
    );
    let bitmap = render(&document, 0);
    assert_eq!(pixel(&bitmap, 30, 37), (255, 0, 0), "inside the square");
    assert_eq!(pixel(&bitmap, 140, 5), (255, 255, 255), "outside it");
    assert_eq!(page_text(&document, 0), "Hello");
    assert!(
        warnings(&document).is_empty(),
        "nothing was tolerated: {:?}",
        warnings(&document)
    );
}

/// **An SVG with no size of its own fills the caller's page box**, which is
/// what `width="100%"` asks for and what its absence defaults to (SVG 2
/// §8.2).
#[test]
fn an_svg_with_no_size_takes_the_callers_box() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg"><rect width="10" height="10"/></svg>"##;
    let options = OpenOptions::at_page(300.0, 200.0);
    let document = Document::open_with(svg.as_bytes().to_vec(), &options).expect("it opens");
    assert_eq!(document.page(0).expect("a page").size(), (300.0, 200.0));
}

/// **An SVG this build will not read is a page saying so**, the placeholder an
/// EPUB spine item gets for the same file, and never a refusal: the sniff
/// already decided what the bytes are.
///
/// The internal subset is what `tinker-pdf-xml` refuses by name, and the sniff
/// walks past it to find the root — so the two disagree here on purpose.
#[test]
fn an_svg_that_will_not_read_is_a_placeholder_naming_the_refusal() {
    let document =
        open(b"<!DOCTYPE svg [<!ENTITY a \"b\">]><svg xmlns=\"http://www.w3.org/2000/svg\"/>");
    assert_eq!(document.page_count(), 1);
    assert!(
        warnings(&document).iter().any(|w| matches!(
            w,
            ArchiveWarning::SpinePage {
                page: 0,
                defect: SpineDefect::SvgUnreadable(_)
            }
        )),
        "{:?}",
        warnings(&document)
    );
}

/// **An `<image>` carrying a `data:` URL is drawn, and one naming a file that
/// is not there is named.** A file opened from its bytes has nothing beside
/// it, and RFC 2397's URL is the one reference that carries its own bytes.
#[test]
fn an_svg_image_resolves_from_a_data_url_and_names_a_missing_file() {
    let png = rgb_png(4, 4, &[0, 0, 255].repeat(16));
    let svg = format!(
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" "##,
            r##"xmlns:xlink="http://www.w3.org/1999/xlink" width="80" height="40">"##,
            r##"<image x="0" y="0" width="40" height="40" xlink:href="data:image/png;base64,{}"/>"##,
            r##"<image x="40" y="0" width="40" height="40" xlink:href="beside.png"/></svg>"##
        ),
        base64(&png)
    );
    let document = open(svg.as_bytes());
    let bitmap = render(&document, 0);
    assert_eq!(pixel(&bitmap, 15, 15), (0, 0, 255), "the data: picture");
    assert_eq!(
        warnings(&document),
        [ArchiveWarning::SvgImageUnresolved {
            item: String::new(),
            images: 1
        }]
    );
}

// ---- a loose XHTML file -------------------------------------------------------

/// **A loose XHTML file is the one chapter of a book, pixel for pixel.**
///
/// The same bytes, opened alone and as an EPUB's only spine item, at the same
/// page box: every page renders to the same bitmap. That is the claim that
/// there is one reader, one layout and one painter for both — a second copy of
/// any of them would disagree here the first time either changed.
#[test]
fn a_loose_xhtml_file_is_the_one_chapter_of_a_book_pixel_for_pixel() {
    let page = xhtml("<title>Loose</title>", &long_body());
    let loose = drawn(open(page.as_bytes()));
    let book = drawn(open(&book_of(&page)));
    assert!(loose.page_count() >= 2, "the body needs two pages");
    assert_eq!(loose.page_count(), book.page_count());
    for at in 0..loose.page_count() as u32 {
        assert_eq!(
            loose.page(at).expect("a page").size(),
            DEFAULT_PAGE,
            "page {at} is the default box"
        );
        let (a, b) = (render(&loose, at), render(&book, at));
        assert!(ink(&a) >= LEAST_INK, "page {at} drew no text to compare");
        assert!(a.data == b.data, "page {at} differs from the book's");
    }
    assert!(page_text(&loose, 0).starts_with("Heading lorem ipsum"));
}

/// **The `<title>` is the document's `/Title`**, white space collapsed, the
/// way a book's `dc:title` is.
#[test]
fn the_title_element_is_the_documents_title() {
    let document = open(xhtml("<title>  A loose\n page </title>", "<p>x</p>").as_bytes());
    assert_eq!(document.metadata().title.as_deref(), Some("A loose page"));
}

/// **An intra-document link lands on the page its target is on**, through the
/// book's own cross-reference pass.
#[test]
fn a_fragment_link_lands_on_the_page_its_target_is_on() {
    use tinker_pdf_cos::dest::{Action, Destination};

    let document = open(xhtml("", &long_body()).as_bytes());
    let last = document.page_count() - 1;
    let links = document.page(last).expect("a page").links();
    assert_eq!(links.len(), 1, "{links:?}");
    let to = match &links[0].target {
        Some(Action::GoTo(Destination::Explicit { page_index, .. })) => *page_index,
        other => panic!("the link became {other:?}"),
    };
    let target = (0..document.page_count())
        .find(|&at| page_text(&document, at).contains("second lorem"))
        .expect("the second paragraph is on a page");
    assert_eq!(to, Some(target), "the link does not land on its target");
}

/// **What a loose file names beside itself is missing, and each is named**:
/// the stylesheet by `StylesheetUnresolved`, the picture by `ImageNotDrawn`.
/// The picture carried in a `data:` URL is drawn.
#[test]
fn what_a_loose_file_names_beside_itself_is_named_as_missing() {
    let png = rgb_png(8, 8, &[0, 160, 0].repeat(64));
    let page = xhtml(
        r#"<link rel="stylesheet" href="style.css"/>"#,
        &format!(
            r#"<p><img src="data:image/png;base64,{}"/><img src="photo.png"/></p>"#,
            base64(&png)
        ),
    );
    let document = open(page.as_bytes());
    let found = warnings(&document);
    assert!(
        found.contains(&ArchiveWarning::StylesheetUnresolved {
            item: String::new(),
            sheets: 1
        }),
        "{found:?}"
    );
    assert!(
        found.contains(&ArchiveWarning::ImageNotDrawn {
            item: String::new(),
            defect: ImageDefect::Unresolved,
            images: 1
        }),
        "{found:?}"
    );
    assert_eq!(
        found
            .iter()
            .filter(|w| matches!(w, ArchiveWarning::ImageNotDrawn { .. }))
            .count(),
        1,
        "the data: picture was refused too: {found:?}"
    );
    // The page has the green picture on it somewhere.
    let bitmap = render(&document, 0);
    assert!(
        bitmap
            .data
            .chunks(bitmap.components())
            .any(|p| p.get(..3) == Some(&[0, 160, 0][..])),
        "the data: picture is not on the page"
    );
}

/// **HTML that does not parse as XML is read as far as it does, and the report
/// says it stopped.** This build has no HTML5 tree builder — the narrowed half
/// of the row — so tag soup is never silently a complete document.
#[test]
fn tag_soup_is_read_as_far_as_it_parses_and_says_so() {
    let soup = "<!DOCTYPE html><html><body><p>first</p><p>unclosed<br><p>after</body></html>";
    let document = open(soup.as_bytes());
    assert!(page_text(&document, 0).contains("first"));
    assert!(
        warnings(&document).contains(&ArchiveWarning::Markup {
            item: String::new(),
            defect: tinker_pdf::epub::xhtml::MarkupDefect::Truncated
        }),
        "{:?}",
        warnings(&document)
    );
}

// ---- a bare image ------------------------------------------------------------

/// **A bare PNG is the one page of a comic, pixel for pixel**, at one image
/// pixel to one point, and the pixels are the file's own.
#[test]
fn a_bare_png_is_the_one_page_of_a_comic_pixel_for_pixel() {
    let (w, h) = (12, 9);
    let png = rgb_png(w, h, &distinct_pixels(w, h));
    let bare = open(&png);
    let comic = open(&zip(&[ZipFile::stored("p.png", &png)], Damage::None));
    assert_eq!(bare.page_count(), 1);
    assert_eq!(bare.page(0).expect("a page").size(), (12.0, 9.0));
    let (a, b) = (render(&bare, 0), render(&comic, 0));
    assert!(a.data == b.data, "the bare page differs from the comic's");
    let pixels = distinct_pixels(w, h);
    for (x, y) in [(0u32, 0u32), (11, 0), (5, 4), (11, 8)] {
        let at = ((y * w + x) * 3) as usize;
        assert_eq!(
            pixel(&a, x, y),
            (pixels[at], pixels[at + 1], pixels[at + 2]),
            "pixel ({x}, {y})"
        );
    }
    assert!(warnings(&bare).is_empty(), "{:?}", warnings(&bare));
}

/// **A bare JPEG passes through**: the page's image is the file's bytes,
/// placed at their own size.
#[test]
fn a_bare_jpeg_is_placed_at_its_own_size() {
    let document = open(&grey_jpeg(16, 8));
    assert_eq!(document.page(0).expect("a page").size(), (16.0, 8.0));
    let (r, g, b) = pixel(&render(&document, 0), 8, 4);
    assert!(
        r == g && g == b && r < 200,
        "not the dark grey: {r} {g} {b}"
    );
}

/// The ITU-T T.6 strip `tests/cbz.rs` builds its TIFF pages from, written out
/// again here: a 16 × 6 bilevel picture whose top row is eight white pixels and
/// eight black.
const G4_ONE_STRIP: [u8; 146] = [
    0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08, 0x00, 0x09, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x00, 0x00, 0x10, 0x01, 0x01, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x06, 0x01, 0x02, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x01, 0x03,
    0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x04, 0x00, 0x00, 0x01, 0x06, 0x00, 0x03, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x01, 0x11, 0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
    0x00, 0x7A, 0x01, 0x15, 0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x01, 0x16,
    0x00, 0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x01, 0x17, 0x00, 0x04, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x33, 0x14, 0xBB, 0x0C, 0x2C, 0x20,
    0xF0, 0x88, 0xB3, 0x65, 0x0E, 0x50, 0xE1, 0x11, 0xD1, 0x1D, 0x11, 0xD0, 0x20, 0x94, 0x44, 0x44,
    0x44, 0x58,
];

/// **A bare TIFF is a page of the picture it holds**: the top row's first
/// half white and second half black, at 16 × 6 points.
#[test]
fn a_bare_tiff_is_a_page_of_its_picture() {
    let document = open(&G4_ONE_STRIP);
    assert_eq!(document.page(0).expect("a page").size(), (16.0, 6.0));
    let bitmap = render(&document, 0);
    assert_eq!(pixel(&bitmap, 2, 0), (255, 255, 255));
    assert_eq!(pixel(&bitmap, 12, 0), (0, 0, 0));
}

/// **A picture this build does not decode is a placeholder naming why**, the
/// page a comic archive holding it alone has always produced: a format
/// recognised and not read is named by its format, and bytes that will not
/// decode are `Undecodable`.
#[test]
fn an_image_this_build_does_not_read_is_a_placeholder_naming_why() {
    for (bytes, defect) in [
        (
            b"GIF89a\x01\x00\x01\x00\x00\x00\x00;".to_vec(),
            PageDefect::UnsupportedFormat(ImageFormat::Gif),
        ),
        (broken_png(), PageDefect::Undecodable),
    ] {
        let document = open(&bytes);
        assert_eq!(document.page_count(), 1);
        assert_eq!(
            warnings(&document),
            [ArchiveWarning::PlaceholderPage { page: 0, defect }]
        );
        assert_eq!(
            document.archive().expect("a report").pages()[0].defect,
            Some(defect)
        );
    }
}

// ---- streamed ----------------------------------------------------------------

/// **A streamed open is the same document**, fetched whole as a container is,
/// and says so.
#[test]
fn a_streamed_standalone_document_is_the_same_document() {
    let png = rgb_png(5, 3, &distinct_pixels(5, 3));
    for bytes in [RED_SQUARE.as_bytes().to_vec(), png] {
        let buffered = open(&bytes);
        let source = Arc::new(ShreddedSource::new(SliceSource::new(bytes.clone())));
        let streamed = Document::open_streaming(source).expect("it opens streamed");
        assert!(streamed.whole_file_fetched());
        assert_eq!(streamed.page_count(), buffered.page_count());
        assert!(render(&streamed, 0).data == render(&buffered, 0).data);
    }
}

/// **A comic archive streamed from a source that answers in pieces opens**,
/// which it did not before this row: the container sniff was one `read`, a
/// `ShreddedSource` answers it with one byte, and one byte of `PK\x03\x04` is
/// no signature — so the archive went to the PDF parser and came back
/// `NotAPdf`. The sniff now fills its window, as the whole-file read after it
/// always did.
#[test]
fn a_comic_streamed_from_a_source_that_answers_in_pieces_opens() {
    let png = rgb_png(5, 3, &distinct_pixels(5, 3));
    let archive = zip(&[ZipFile::stored("p.png", &png)], Damage::None);
    let source = Arc::new(ShreddedSource::new(SliceSource::new(archive.clone())));
    let streamed = Document::open_streaming(source).expect("the archive opens streamed");
    assert_eq!(streamed.page_count(), 1);
    assert!(render(&streamed, 0).data == render(&open(&archive), 0).data);
}
