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

/// `text` as UTF-16, in the byte order asked, with or without its byte order
/// mark.
fn utf16(text: &str, big_endian: bool, mark: bool) -> Vec<u8> {
    let units = mark.then_some('\u{FEFF}').into_iter().chain(text.chars());
    let mut out = Vec::new();
    for c in units {
        let mut pair = [0u16; 2];
        for unit in c.encode_utf16(&mut pair) {
            out.extend_from_slice(&if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
    }
    out
}

/// **A UTF-16 SVG, XHTML file or FB2 is sniffed as what it is**, and opens as
/// the same document its UTF-8 bytes do.
///
/// `tinker-pdf-xml` decodes UTF-16 in both byte orders, marked and in Appendix
/// F's unmarked `3C 00` / `00 3C` shape, and every reader behind the sniff is
/// built on it; a sniff that walked the prolog only as UTF-8 sent all three to
/// the PDF parser and `NotAPdf`, while `fb2::to_xhtml` translated the same
/// bytes.
#[test]
fn a_utf_16_document_is_sniffed_as_what_it_is() {
    let fb2 = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?>",
        "<FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\">",
        "<description><title-info><book-title>Т</book-title></title-info></description>",
        "<body><section><p>hello from a book</p></section></body></FictionBook>"
    );
    let page = xhtml("<title>t</title>", "<p>hello from a page</p>")
        .replace("encoding=\"utf-8\"", "encoding=\"UTF-16\"");
    let svg = format!("<?xml version=\"1.0\" encoding=\"UTF-16\"?>{RED_SQUARE}");
    for (kind, source) in [
        (Standalone::Fb2, fb2.to_owned()),
        (Standalone::Html, page),
        (Standalone::Svg, svg),
    ] {
        let utf8 = open(source.as_bytes());
        for (big_endian, mark) in [(false, true), (true, true), (false, false), (true, false)] {
            let what = format!("{kind:?}, big-endian {big_endian}, marked {mark}");
            let bytes = utf16(&source, big_endian, mark);
            assert_eq!(tinker_pdf::standalone::sniff(&bytes), Some(kind), "{what}");
            let document = open(&bytes);
            assert_eq!(document.page_count(), utf8.page_count(), "{what}");
            assert_eq!(
                document.page(0).expect("a page").size(),
                utf8.page(0).expect("a page").size(),
                "{what}"
            );
            assert_eq!(page_text(&document, 0), page_text(&utf8, 0), "{what}");
            assert!(!page_text(&document, 0).is_empty(), "{what}");
            assert!(
                !warnings(&document)
                    .iter()
                    .any(|w| matches!(w, ArchiveWarning::Markup { .. })),
                "{what}: {:?}",
                warnings(&document)
            );
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

/// **Tag soup opens as the tree HTML's parser builds, pixel for pixel**
/// (tier 5's formats row). Each soup below is HTML the XML reader stops at in
/// its first lines — an unclosed `<p>` and `<li>`, an unquoted attribute, a
/// `&nbsp` with no semicolon, a misnested `<b><i>`, a cell with no row — and
/// beside it the XHTML of the tree §13.2.6 builds from it, written out by hand
/// from the standard and the way html5lib's suite writes such trees. The
/// soup's pages are the twin's pages, byte for byte, and the report says the
/// soup was not XML and nothing else.
#[test]
fn tag_soup_opens_as_the_tree_html_builds_pixel_for_pixel() {
    let cases: [(&str, &str); 3] = [
        (
            "<!DOCTYPE html><html><body><p>first</p><p>unclosed<br><p>after</body></html>",
            "<p>first</p><p>unclosed<br/></p><p>after</p>",
        ),
        (
            "<title>Soup</title><h1 class=big>Heading</h1><ul><li>one<li>two &amp three\
             &nbsp;four</ul><p><b>bold <i>both</b> italic</i> plain",
            "<h1 class=\"big\">Heading</h1><ul><li>one</li><li>two &amp; three\u{A0}four</li></ul>\
             <p><b>bold <i>both</i></b><i> italic</i> plain</p>",
        ),
        (
            "<table><td>cell<td>next</table><p>after the table",
            "<table><tbody><tr><td>cell</td><td>next</td></tr></tbody></table><p>after the table</p>",
        ),
    ];
    for (soup, tree) in cases {
        let opened = drawn(open(soup.as_bytes()));
        let twin = drawn(open(
            format!(
                "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head></head><body>{tree}</body></html>"
            )
            .as_bytes(),
        ));
        assert_eq!(opened.page_count(), twin.page_count(), "{soup}");
        let (a, b) = (render(&opened, 0), render(&twin, 0));
        assert!(ink(&a) >= LEAST_INK, "{soup}: nothing was drawn");
        assert!(a.data == b.data, "{soup}: the soup is not the tree's page");
        assert_eq!(page_text(&opened, 0), page_text(&twin, 0), "{soup}");
        let soup_warnings: Vec<ArchiveWarning> = warnings(&opened)
            .into_iter()
            .filter(|w| !matches!(w, ArchiveWarning::FontsAttachedAfterPagination))
            .collect();
        assert_eq!(
            soup_warnings,
            [ArchiveWarning::Markup {
                item: String::new(),
                defect: tinker_pdf::epub::xhtml::MarkupDefect::NotXml
            }],
            "{soup}"
        );
    }
    // The title of a document that is not XML is its title still.
    let titled = open(b"<title>Soup</title><p>x");
    assert_eq!(titled.metadata().title.as_deref(), Some("Soup"));
}

/// **The tree HTML builds is held to the XML reader's depth**: every reader
/// after this one was written against `MAX_XML_DEPTH` standing in front of it,
/// and HTML's adoption agency can nest clones deeper than its own stack. An
/// element past the cap is not made; its text is kept in the deepest element
/// the cap allows, and `TooDeep` says so.
#[test]
fn an_html_tree_deeper_than_the_cap_keeps_its_text_and_says_so() {
    use tinker_pdf::epub::xhtml::{from_html, MarkupDefect};
    let limits = tinker_pdf_xml::Limits {
        max_depth: 4,
        ..tinker_pdf_xml::Limits::DEFAULT
    };
    let parsed = tinker_pdf_xml::html::parse(
        "<div><div><div><span>deep <b>deeper</b></span></div></div></div><p>after",
        &tinker_pdf_xml::Limits::DEFAULT,
    );
    let dom = from_html(&parsed, &limits);
    assert!(
        dom.defects.contains(&MarkupDefect::TooDeep),
        "{:?}",
        dom.defects
    );
    for (at, node) in dom.nodes.iter().enumerate() {
        let mut depth = 1;
        let mut up = node.parent;
        while let Some(parent) = up {
            depth += 1;
            up = dom.nodes[parent].parent;
        }
        assert!(depth <= 4, "node {at} ({}) is {depth} deep", node.name);
    }
    let text: String = dom
        .nodes
        .iter()
        .flat_map(|n| &n.children)
        .filter_map(|c| match c {
            tinker_pdf::epub::xhtml::Child::Text(t) => Some(t.as_str()),
            tinker_pdf::epub::xhtml::Child::Element(_) => None,
        })
        .collect();
    assert_eq!(
        text, "deep deeperafter",
        "the text past the cap is kept, in order"
    );
    // Within the cap nothing is said.
    let shallow = from_html(&parsed, &tinker_pdf_xml::Limits::DEFAULT);
    assert_eq!(shallow.defects, [MarkupDefect::NotXml]);
}

/// **A well-formed XHTML file is still read as XML**, so it is not reported
/// as anything else; and **HTML in windows-1252** — not UTF-8, and saying
/// nothing about its encoding — is decoded by HTML's default rather than
/// lost.
#[test]
fn xml_is_still_xml_and_an_undeclared_eight_bit_page_is_windows_1252() {
    let xhtml = open(
        b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>t</title></head>\
          <body><p>fine</p></body></html>",
    );
    assert!(
        warnings(&xhtml).is_empty(),
        "well-formed XHTML: {:?}",
        warnings(&xhtml)
    );
    let latin = open(b"<!DOCTYPE html><p>\x93caf\xe9\x94 & cr\xe8me");
    assert_eq!(page_text(&latin, 0), "\u{201C}café\u{201D} & crème");
    assert!(warnings(&latin).contains(&ArchiveWarning::Markup {
        item: String::new(),
        defect: tinker_pdf::epub::xhtml::MarkupDefect::NotXml
    }));
}

/// **An XHTML file in the single-byte encoding its declaration names is read
/// in that encoding**, as XML when it is well-formed and as HTML's characters
/// when it is not. The review of the formats lane found the well-formed one
/// refused for its encoding, handed to HTML's parser — which reads no XML
/// declaration — and set as `Ïðèâåò`, windows-1252's letters for the bytes of
/// `Привет`, with a warning saying only that it was not XML.
#[test]
fn a_loose_xhtml_file_is_read_in_the_single_byte_encoding_it_declares() {
    let privet = [0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2];
    let file = |encoding: &str, body: &[u8]| {
        let mut bytes = format!(
            "<?xml version=\"1.0\" encoding=\"{encoding}\"?>\
             <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>t</title></head><body>"
        )
        .into_bytes();
        bytes.extend_from_slice(body);
        bytes
    };

    let mut well_formed = b"<p>".to_vec();
    well_formed.extend_from_slice(&privet);
    well_formed.extend_from_slice(b"</p></body></html>");
    let document = open(&file("windows-1251", &well_formed));
    assert_eq!(page_text(&document, 0), "Привет");
    // Nothing about the markup: it is XML. (The build's faces cover no
    // Cyrillic, which `UncoveredCharacters` says, and which is not this.)
    assert!(
        !warnings(&document)
            .iter()
            .any(|w| matches!(w, ArchiveWarning::Markup { .. })),
        "well-formed XHTML in a declared encoding: {:?}",
        warnings(&document)
    );

    // An unclosed `<br>` and no end tags: HTML, over the declared encoding's
    // characters rather than over bytes for it to guess at.
    let mut soup = b"<p>".to_vec();
    soup.extend_from_slice(&privet);
    soup.extend_from_slice(b"<br>");
    let document = open(&file("windows-1251", &soup));
    assert_eq!(page_text(&document, 0), "Привет");
    assert!(warnings(&document).contains(&ArchiveWarning::Markup {
        item: String::new(),
        defect: tinker_pdf::epub::xhtml::MarkupDefect::NotXml
    }));

    // A byte the declared table leaves unmapped is U+FFFD, and said.
    let mut hole = b"<p>".to_vec();
    hole.extend_from_slice(&[0xE2, 0xAA, 0xE3]);
    hole.extend_from_slice(b"<br>");
    let document = open(&file("windows-1253", &hole));
    let text = page_text(&document, 0);
    assert!(text.starts_with('β') && text.ends_with('γ'), "{text}");
    assert!(warnings(&document).contains(&ArchiveWarning::Markup {
        item: String::new(),
        defect: tinker_pdf::epub::xhtml::MarkupDefect::Undecodable
    }));
}

/// **A loose file XML cannot read is still read in the encoding it says it is
/// in.** The review of the lane's fixes found the single-byte fix held only
/// while the XML reader could decode the file: a declared windows-1251 with a
/// form feed in it — no character to XML — went to HTML's parser as bytes and
/// was set as `Ïðèâåò`; a UTF-16 file with no byte order mark that is not
/// well-formed, decoded as UTF-16 and thrown away, was read again as UTF-8;
/// and a well-formed file declaring Shift_JIS was set in windows-1252's
/// letters saying only `NotXml`, where a `<meta charset>` naming the same
/// encoding says `EncodingNotDecoded`.
#[test]
fn a_loose_file_xml_cannot_read_is_read_in_the_encoding_it_names() {
    use tinker_pdf::epub::xhtml::MarkupDefect;
    let has = |document: &Document, defect: MarkupDefect| {
        warnings(document).contains(&ArchiveWarning::Markup {
            item: String::new(),
            defect,
        })
    };
    let privet: &[u8] = b"\xcf\xf0\xe8\xe2\xe5\xf2";

    // A C0 control the declared table decodes and XML does not admit.
    let mut controlled = b"<?xml version=\"1.0\" encoding=\"windows-1251\"?>\
        <html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>"
        .to_vec();
    controlled.extend_from_slice(privet);
    controlled.extend_from_slice(b" \x0c ");
    controlled.extend_from_slice(privet);
    controlled.extend_from_slice(b"</p></body></html>");
    let document = open(&controlled);
    assert_eq!(page_text(&document, 0), "Привет Привет");
    assert!(has(&document, MarkupDefect::NotXml));

    // UTF-16LE with no byte order mark, a declaration, and a `<br>` left
    // open; and the same with no declaration, in Appendix F's unmarked shape.
    for markup in [
        "<?xml version=\"1.0\" encoding=\"UTF-16\"?><html><body><p>Привет<br></body></html>",
        "<html><body><p>Привет<br></body></html>",
    ] {
        let wide: Vec<u8> = markup.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let document = open(&wide);
        assert_eq!(page_text(&document, 0), "Привет", "{markup}");
        assert!(has(&document, MarkupDefect::NotXml), "{markup}");
        // Refused for its syntax, and not for an encoding.
        assert!(
            !has(&document, MarkupDefect::EncodingNotDecoded),
            "{markup}"
        );
    }

    // An encoding this build does not decode, declared by a well-formed file:
    // the guess, and the same defect a `<meta>` naming it gets.
    let japanese = open(
        b"<?xml version=\"1.0\" encoding=\"Shift_JIS\"?>\
          <html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>\x93\xfa\x96\x7b</p></body></html>",
    );
    assert!(has(&japanese, MarkupDefect::EncodingNotDecoded));
    let meta = open(b"<html><head><meta charset=shift_jis></head><body><p>\x93\xfa\x96\x7b<br>");
    assert!(has(&meta, MarkupDefect::EncodingNotDecoded));
    assert_eq!(page_text(&japanese, 0), page_text(&meta, 0));

    // The same declaration over bytes that are UTF-8 — ASCII, its Japanese
    // written as character references — which the XML reader decodes and
    // then refuses at the declaration: the text is right, and the report
    // says why a well-formed file is not XML. Then the same with a byte order
    // mark, which decides the encoding before the declaration is read.
    let declared = "<?xml version=\"1.0\" encoding=\"Shift_JIS\"?>\
        <html xmlns=\"http://www.w3.org/1999/xhtml\"><body><p>&#x65E5;&#x672C; Japan</p></body></html>";
    let marked: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain(declared.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    for bytes in [declared.as_bytes(), &marked] {
        let document = open(bytes);
        assert_eq!(page_text(&document, 0), "日本 Japan");
        assert!(has(&document, MarkupDefect::NotXml));
        assert!(
            has(&document, MarkupDefect::EncodingNotDecoded),
            "{:?}",
            warnings(&document)
        );
    }
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

/// One of the pictures `tinker-pdf-filters/tests/images/` holds its decoders
/// to, pixel for pixel; provenance is that directory's README.
fn image_fixture(path: &str) -> Vec<u8> {
    let full = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tinker-pdf-filters/tests/images")
        .join(path);
    std::fs::read(&full).unwrap_or_else(|e| panic!("{}: {e}", full.display()))
}

/// **A bare picture is the comic of that one picture**, in every format the
/// comic path pages: the same page count, each page the same size and the same
/// pixels, the same warnings and the same per-page defects.
///
/// The module comment's claim is that a bare image is "built by the code that
/// builds the larger document", and an equality over every format is what
/// holds it: a copy of the comic path's per-format plan had already drifted
/// from it, so a GIF and a WebP that a one-entry CBZ decoded and drew were
/// placeholders bare, and a multi-page TIFF the CBZ paged was one page.
#[test]
fn a_bare_picture_is_the_comic_of_that_one_picture() {
    let png = rgb_png(12, 9, &distinct_pixels(12, 9));
    for (what, bytes, pages) in [
        ("a PNG", png, 1),
        ("a JPEG", grey_jpeg(16, 8), 1),
        ("a G4 TIFF", G4_ONE_STRIP.to_vec(), 1),
        ("a GIF", image_fixture("gif/pillow-palette-13x7.gif"), 1),
        (
            "a lossless WebP",
            image_fixture("webp/pillow-lossless-4colour-13x7.webp"),
            1,
        ),
        (
            "a lossy WebP",
            image_fixture("webp/pillow-lossy-rgb-61x45.webp"),
            1,
        ),
        (
            "a TIFF of three pages",
            image_fixture("tiff/tifffile-multipage.tif"),
            3,
        ),
    ] {
        let bare = open(&bytes);
        let comic = open(&zip(&[ZipFile::stored("picture", &bytes)], Damage::None));
        assert_eq!(bare.page_count(), pages, "{what}");
        assert_eq!(comic.page_count(), pages, "{what}, as a comic");
        for page in 0..pages {
            let size = bare.page(page).expect("a page").size();
            assert_eq!(size, comic.page(page).expect("a page").size(), "{what}");
            let (a, b) = (render(&bare, page), render(&comic, page));
            assert!(
                a.data == b.data,
                "{what}: page {page} differs from the comic's"
            );
            assert!(ink(&a) > 0, "{what}: page {page} is blank");
        }
        assert_eq!(warnings(&bare), warnings(&comic), "{what}");
        assert!(warnings(&bare).is_empty(), "{what}: {:?}", warnings(&bare));
        let defects = |document: &Document| -> Vec<Option<PageDefect>> {
            let report = document.archive().expect("a report");
            report.pages().iter().map(|page| page.defect).collect()
        };
        assert_eq!(defects(&bare), defects(&comic), "{what}");
    }
}

/// **A picture this build does not decode is a placeholder naming why**, the
/// page a comic archive holding it alone produces: a format recognised and not
/// read is named by its format, and bytes that will not decode are
/// `Undecodable`.
#[test]
fn an_image_this_build_does_not_read_is_a_placeholder_naming_why() {
    for (bytes, defect) in [
        (
            b"\x00\x00\x00\x14ftypavif\x00\x00\x00\x00".to_vec(),
            PageDefect::UnsupportedFormat(ImageFormat::Avif),
        ),
        (broken_png(), PageDefect::Undecodable),
        // A GIF header with no image in it: read, and nothing to draw.
        (
            b"GIF89a\x01\x00\x01\x00\x00\x00\x00;".to_vec(),
            PageDefect::Undecodable,
        ),
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
        let comic = open(&zip(&[ZipFile::stored("picture", &bytes)], Damage::None));
        assert_eq!(warnings(&comic), warnings(&document));
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
