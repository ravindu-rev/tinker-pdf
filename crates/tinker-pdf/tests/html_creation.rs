//! HTML and CSS to PDF as a creation API: `DocumentBuilder::from_html`
//! (tier 5's formats row).
//!
//! The row asks for the API **held by the EPUB reftests**, and this file does
//! that in the two ways the claim can be made:
//!
//! - **The creation call is the book.** Markup and a stylesheet handed to
//!   `from_html` render to exactly the pixels of an EPUB whose one chapter is
//!   the same markup with the stylesheet linked at the top of its `<head>`,
//!   at the same page box. One cascade, one layout and one painter, or this
//!   fails the first time a second copy of any of them drifts.
//! - **The reftest pairs hold through it.** The pairs `epub_reftest.rs` lays
//!   out through the reader — a shorthand against its longhands, `1.5em`
//!   against `24px`, `50%` against `120px`, an implied row group against an
//!   explicit one, padding against border — are laid out here through
//!   `from_html` and read back out of the finished PDF, line by line, each
//!   with the mismatch reference that must not agree. A reftest suite that
//!   passed with the layout engine deleted would be one where two empty
//!   documents agree, and the mismatch is what stops that.

mod epub_support;
mod render_support;

use std::sync::Arc;

use epub_support::{ocf_zip, OcfEntry};
use render_support::{curvy_font, ink};
use tinker_pdf::epub::read::PX_TO_PT;
use tinker_pdf::epub::BookOptionDefect;
use tinker_pdf::{
    ArchiveWarning, Bitmap, Document, DocumentBuilder, FromHtml, HtmlError, OpenOptions, PageBox,
    RenderOptions, SimpleFontProvider,
};

/// The face every pair is set in: Courier's 600/1000 advance, so a line
/// breaks where arithmetic says and a pair can disagree by a whole character.
const MONO: &str = "* { font-family: monospace !important } html { font-size: 16px !important }";

/// The reftest suite's own reset.
const RESET: &str = "body { margin: 0 } p, div, span, b, i, table, td, th, tr, li, ul \
                     { margin: 0; padding: 0; border: 0 }";

/// The reftest suite's measure, 240 CSS pixels, as a page with no margin.
fn column_page() -> PageBox {
    PageBox::new(240.0 * PX_TO_PT, 3_000.0).with_margin(0.0)
}

fn document(body: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head>"#,
            r#"<title>t</title></head><body>{body}</body></html>"#
        ),
        body = body
    )
}

/// The finished document a `from_html` call made.
fn made(markup: &str, stylesheet: &str, page: PageBox) -> Document {
    let (builder, _) =
        DocumentBuilder::from_html(markup, stylesheet, page).expect("the markup lays out");
    Document::open(builder.finish()).expect("the finished document opens")
}

/// Every line on the first page, its text and where it starts, read back out of
/// the PDF — what the reftest suite compares, one layer further out.
fn lines(document: &Document) -> Vec<(String, i64, i64)> {
    let text = document.page(0).expect("a page").text();
    text.lines()
        .iter()
        .map(|line| {
            (
                line.text.clone(),
                (line.quad.ll.0 * 1000.0).round() as i64,
                (line.quad.ll.1 * 1000.0).round() as i64,
            )
        })
        .collect()
}

fn lay(style: &str, body: &str) -> Vec<(String, i64, i64)> {
    lines(&made(
        &document(body),
        &format!("{RESET} {MONO} {style}"),
        column_page(),
    ))
}

#[track_caller]
fn same(
    what: &str,
    left: Vec<(String, i64, i64)>,
    right: Vec<(String, i64, i64)>,
    broken: Vec<(String, i64, i64)>,
) {
    assert!(!left.is_empty(), "{what}: the reference laid out nothing");
    assert_eq!(left, right, "{what}: the two spellings disagree");
    assert_ne!(
        left, broken,
        "{what}: the mismatch reference agrees too, so the pair proves nothing"
    );
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

fn render(document: &Document, page: u32) -> Bitmap {
    document
        .page(page)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// An EPUB whose one chapter is `chapter`, with `sheet` as `style.css` beside
/// it.
fn book_of(chapter: &str, sheet: &str) -> Vec<u8> {
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
        r#"<item id="c" href="c.xhtml" media-type="application/xhtml+xml"/>"#,
        r#"<item id="s" href="style.css" media-type="text/css"/></manifest>"#,
        r#"<spine><itemref idref="c"/></spine></package>"#
    );
    let entries = [
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", container.as_bytes()),
        OcfEntry::deflated("content.opf", package.as_bytes()),
        OcfEntry::deflated("c.xhtml", chapter.as_bytes()),
        OcfEntry::deflated("style.css", sheet.as_bytes()),
    ];
    ocf_zip(&entries, &[0, 1, 2, 3, 4])
}

// ---- the creation call is the book -------------------------------------------

/// **Markup and a stylesheet handed to `from_html` are the pages of a book whose
/// chapter links the same sheet first**, pixel for pixel, at two page boxes.
///
/// The book's chapter carries the `<link>` at the top of its `<head>` and its
/// own `<style>` after it; the creation call is handed the sheet separately
/// and the same `<head>` without the `<link>`. `css-cascade-5` §6.1's order of
/// appearance is the one thing that could tell the two apart, and the
/// document's `<style>` is written to tie with the sheet so that it does.
#[test]
fn a_document_made_from_html_is_the_book_of_the_same_chapter() {
    let sheet = "h1 { font-size: 30px; color: #036 } p { text-indent: 2em; color: #a00 } \
                 li { margin-left: 1em }";
    let words = "the quick brown fox jumps over the lazy dog ".repeat(40);
    let body = format!(
        "<h1>A heading</h1><p>{words}</p><ul><li>one</li><li>two</li></ul>\
         <table><tr><td>a</td><td>b</td></tr></table><p>{words}</p>"
    );
    // The document's own rule ties with the sheet's on `p { color }` and must
    // win, as it would after a `<link>`.
    let style = "<style>p { color: #060 }</style>";
    let made_head = format!("<title>t</title>{style}");
    let book_head = format!(r#"<title>t</title><link rel="stylesheet" href="style.css"/>{style}"#);
    let wrap = |head: &str| {
        format!(
            concat!(
                r#"<?xml version="1.0" encoding="utf-8"?>"#,
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><head>{head}</head>"#,
                r#"<body>{body}</body></html>"#
            ),
            head = head,
            body = body
        )
    };
    for (width, height) in [(432.0, 648.0), (300.0, 420.0)] {
        let page = PageBox::new(width, height);
        let (builder, report) =
            DocumentBuilder::from_html(wrap(&made_head), sheet, page).expect("the markup lays out");
        let made = drawn(Document::open(builder.finish()).expect("it opens"));
        let book = drawn(
            Document::open_with(
                book_of(&wrap(&book_head), sheet),
                &OpenOptions::at_page(width, height),
            )
            .expect("the book opens"),
        );
        assert!(made.page_count() >= 2, "the body needs two pages");
        assert_eq!(made.page_count() as usize, report.pages());
        assert_eq!(
            made.page_count(),
            book.page_count(),
            "at {width} x {height}"
        );
        for at in 0..made.page_count() {
            assert_eq!(made.page(at).expect("a page").size(), (width, height));
            let (a, b) = (render(&made, at), render(&book, at));
            assert!(ink(&a) >= LEAST_INK, "page {at} drew no text to compare");
            assert!(
                a.data == b.data,
                "page {at} at {width} x {height} differs from the book's"
            );
        }
    }
}

/// **The caller's sheet comes first and the document's own rules win a tie**,
/// and an `!important` in the caller's sheet beats the document's normal rule
/// — `css-cascade-5` §6.1, both halves, read back as the colour on the page.
#[test]
fn the_callers_sheet_is_applied_ahead_of_the_documents_own() {
    let markup = |style: &str| {
        format!(
            concat!(
                r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>{style}</style>"#,
                r#"</head><body><div style="width: 100px; height: 100px"></div></body></html>"#
            ),
            style = style
        )
    };
    let colour = |stylesheet: &str| {
        let document = made(
            &markup("div { background-color: #0000ff }"),
            stylesheet,
            PageBox::new(200.0, 200.0).with_margin(0.0),
        );
        let bitmap = render(&document, 0);
        let at = 30 * bitmap.stride + 30 * bitmap.components();
        (bitmap.data[at], bitmap.data[at + 1], bitmap.data[at + 2])
    };
    assert_eq!(
        colour("div { background-color: #ff0000 }"),
        (0, 0, 255),
        "the document's rule loses a tie to the caller's"
    );
    assert_eq!(
        colour("div { background-color: #ff0000 !important }"),
        (255, 0, 0),
        "an important declaration in the caller's sheet does not win"
    );
}

/// **The page box is the caller's**: the size is every page's, the margin is
/// where the content starts, and the `<title>` is the `/Title`.
#[test]
fn the_page_box_and_margin_are_the_callers() {
    let document = made(
        &document("<p>text</p>"),
        &format!("{MONO} body {{ margin: 0 }} p {{ margin: 0 }}"),
        PageBox::new(300.0, 200.0).with_margin(20.0),
    );
    assert_eq!(document.page(0).expect("a page").size(), (300.0, 200.0));
    let first = lines(&document);
    assert_eq!(first.first().map(|line| line.1), Some(20_000), "{first:?}");
    assert_eq!(document.metadata().title.as_deref(), Some("t"));
}

/// **A number the caller passed that cannot be used is replaced and named**:
/// a page width that is not a number, and a margin that leaves no content area.
#[test]
fn an_unusable_page_box_is_replaced_and_named() {
    let (_, report) = DocumentBuilder::from_html(
        document("<p>x</p>"),
        "",
        PageBox::new(f64::NAN, 400.0).with_margin(500.0),
    )
    .expect("it lays out");
    assert_eq!(
        &report.warnings()[..2],
        [
            ArchiveWarning::UnusableOption(BookOptionDefect::PageWidth),
            ArchiveWarning::UnusableOption(BookOptionDefect::Margin),
        ]
    );
    assert_eq!(report.layout().page, (432.0, 400.0));
    assert!(report.margin() * 2.0 < 400.0);
}

/// **A document the cascade refuses is an error, not a placeholder page**: a
/// creation call has no page count to keep. `MAX_DOM_NODES` is the cap.
#[test]
fn a_document_past_a_cascade_cap_is_refused_by_name() {
    let body = "<i/>".repeat(tinker_pdf_css::limits::MAX_DOM_NODES + 1);
    let refused = DocumentBuilder::from_html(document(&body), "", PageBox::new(400.0, 400.0));
    assert_eq!(refused.err(), Some(HtmlError::StyleRefused));
}

/// **The caller's own stylesheet refused at a cap refuses the document**, as
/// the markup's would: a creation call that laid the markup out without the
/// sheet it was handed would return pages its caller did not ask for and a
/// report saying nothing. The two caps a sheet can reach on its own — its rule
/// count and its length — each refuse it.
#[test]
fn a_stylesheet_past_a_cascade_cap_is_refused_by_name() {
    use tinker_pdf_css::limits::{MAX_CSS_BYTES, MAX_CSS_RULES};
    let mut rules: String = (0..=MAX_CSS_RULES)
        .map(|n| format!(".c{n}{{color:red}}"))
        .collect();
    rules.push_str("p{color:#00ff00}");
    let mut long = String::from("p{color:#00ff00}");
    long.push_str(&" ".repeat(MAX_CSS_BYTES));
    for (what, sheet) in [("rules", rules), ("bytes", long)] {
        let refused =
            DocumentBuilder::from_html(document("<p>x</p>"), &sheet, PageBox::new(300.0, 300.0));
        assert_eq!(
            refused.err(),
            Some(HtmlError::StyleRefused),
            "a sheet past the {what} cap"
        );
    }
    // And a sheet of half as many rules is a document. (The rule cap is the
    // whole document's, so the user-agent sheet's rules count against it too.)
    let within: String = (0..MAX_CSS_RULES / 2)
        .map(|n| format!(".c{n}{{color:red}}"))
        .collect();
    assert!(
        DocumentBuilder::from_html(document("<p>x</p>"), &within, PageBox::new(300.0, 300.0))
            .is_ok()
    );
}

/// **What the markup names that nothing answers is named**, and a provider
/// handed to `from_html_with` answers it — through the same seam a book's
/// container does.
#[test]
fn a_provider_answers_what_the_markup_names() {
    use tinker_pdf::epub::read::{Resources, Unavailable};
    use tinker_pdf::epub::Limits;

    struct Sheet;
    impl Resources for Sheet {
        fn fetch(
            &mut self,
            _: &str,
            reference: &str,
            _: &Limits,
        ) -> Result<(String, Vec<u8>), Unavailable> {
            if reference == "site.css" {
                Ok((
                    "site.css".to_owned(),
                    b"div { background-color: #00ff00 }".to_vec(),
                ))
            } else {
                Err(Unavailable::Missing)
            }
        }
    }
    let markup = concat!(
        r#"<html xmlns="http://www.w3.org/1999/xhtml"><head>"#,
        r#"<link rel="stylesheet" href="site.css"/></head>"#,
        r#"<body><div style="width: 100px; height: 100px"></div></body></html>"#
    );
    let page = PageBox::new(200.0, 200.0).with_margin(0.0);

    let (_, alone) = DocumentBuilder::from_html(markup, "", page).expect("it lays out");
    assert!(
        alone
            .warnings()
            .contains(&ArchiveWarning::StylesheetUnresolved {
                item: String::new(),
                sheets: 1
            }),
        "{:?}",
        alone.warnings()
    );

    let (builder, answered) =
        DocumentBuilder::from_html_with(markup, "", page, &mut Sheet).expect("it lays out");
    assert!(answered.warnings().is_empty(), "{:?}", answered.warnings());
    let document = Document::open(builder.finish()).expect("it opens");
    let bitmap = render(&document, 0);
    let at = 30 * bitmap.stride + 30 * bitmap.components();
    assert_eq!(
        &bitmap.data[at..at + 3],
        [0, 255, 0],
        "the sheet did not apply"
    );
}

// ---- the reftest pairs, through the creation call ------------------------------

#[test]
fn a_margin_shorthand_is_its_four_longhands() {
    let body = "<p>one</p><p>two</p>";
    same(
        "margin",
        lay("p { margin: 10px 20px }", body),
        lay(
            "p { margin-top: 10px; margin-right: 20px; margin-bottom: 10px; margin-left: 20px }",
            body,
        ),
        lay(
            "p { margin-top: 10px; margin-right: 20px; margin-bottom: 10px }",
            body,
        ),
    );
}

#[test]
fn a_padding_shorthand_is_its_four_longhands() {
    let body = "<p>one</p><p>two</p>";
    same(
        "padding",
        lay("p { padding: 10px 20px 30px 40px }", body),
        lay(
            "p { padding-top: 10px; padding-right: 20px; padding-bottom: 30px; padding-left: 40px }",
            body,
        ),
        lay(
            "p { padding-top: 10px; padding-right: 40px; padding-bottom: 30px; padding-left: 20px }",
            body,
        ),
    );
}

#[test]
fn a_border_shorthand_is_its_three_longhands() {
    let body = "<p>one</p><p>two</p>";
    same(
        "border",
        lay("p { border: 8px solid #000 }", body),
        lay(
            "p { border-width: 8px; border-style: solid; border-color: #000 }",
            body,
        ),
        lay("p { border-width: 8px; border-color: #000 }", body),
    );
}

#[test]
fn an_em_is_the_elements_own_font_size() {
    let body = "<div><p>one</p><p>two</p></div>";
    same(
        "em",
        lay(
            "div { font-size: 16px } p { font-size: 16px; margin-top: 1.5em }",
            body,
        ),
        lay(
            "div { font-size: 16px } p { font-size: 16px; margin-top: 24px }",
            body,
        ),
        lay(
            "div { font-size: 16px } p { font-size: 16px; margin-top: 16px }",
            body,
        ),
    );
}

#[test]
fn a_percentage_width_is_that_fraction_of_the_containing_block() {
    let body = r#"<div class="w"><p>aaaa bbbb cccc dddd eeee ffff</p></div>"#;
    same(
        "a percentage width",
        lay(
            "p { font-size: 20px; line-height: 30px } .w { width: 50% }",
            body,
        ),
        lay(
            "p { font-size: 20px; line-height: 30px } .w { width: 120px }",
            body,
        ),
        lay(
            "p { font-size: 20px; line-height: 30px } .w { width: 96px }",
            body,
        ),
    );
}

#[test]
fn an_implied_row_group_is_the_row_group_the_markup_omitted() {
    let style = "body { font-size: 20px; line-height: 30px } td { padding: 0 }";
    same(
        "an implied row group",
        lay(
            style,
            "<table><tbody><tr><td>aa</td><td>bb</td></tr>\
             <tr><td>cc</td><td>dd</td></tr></tbody></table>",
        ),
        lay(
            style,
            "<table><tr><td>aa</td><td>bb</td></tr><tr><td>cc</td><td>dd</td></tr></table>",
        ),
        lay(style, "<table><tr><td>aa</td><td>bb</td></tr></table>"),
    );
}

#[test]
fn a_span_told_to_be_a_block_is_a_block() {
    let style = "body { font-size: 20px; line-height: 30px } .b { display: block }";
    same(
        "display: block",
        lay(style, "<div>one</div><div>two</div>"),
        lay(
            style,
            r#"<span class="b">one</span><span class="b">two</span>"#,
        ),
        lay(
            "body { font-size: 20px; line-height: 30px } .b { display: inline }",
            r#"<span class="b">one</span><span class="b">two</span>"#,
        ),
    );
}

#[test]
fn a_collapsed_pair_of_margins_is_one_margin_of_the_larger_size() {
    let body = r#"<p class="a">one</p><p class="b">two</p>"#;
    same(
        "collapsing",
        lay(
            "p { font-size: 20px; line-height: 30px } .a { margin-bottom: 30px } .b { margin-top: 10px }",
            body,
        ),
        lay(
            "p { font-size: 20px; line-height: 30px } .a { margin-bottom: 30px }",
            body,
        ),
        lay(
            "p { font-size: 20px; line-height: 30px } .a { margin-bottom: 40px }",
            body,
        ),
    );
}

#[test]
fn padding_and_border_reach_the_content_edge_the_same_way() {
    let body = "<p>one</p>";
    same(
        "the content edge",
        lay(
            "p { font-size: 20px; padding-left: 20px; border-left: 10px solid #000 }",
            body,
        ),
        lay("p { font-size: 20px; padding-left: 30px }", body),
        lay("p { font-size: 20px; padding-left: 20px }", body),
    );
}
