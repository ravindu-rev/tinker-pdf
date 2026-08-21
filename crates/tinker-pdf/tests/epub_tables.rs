//! Tables, end to end (gap 31, milestone 11).
//!
//! `tinker-pdf-layout`'s own tests are where CSS 2.2 §17 is asserted against
//! its own arithmetic; this file is where the *reader* is asserted — the
//! attributes, the user-agent sheet, the cascade, the box tree, the pages, and
//! text conservation across every one of them.
//!
//! # `colspan` and `rowspan` are the one thing no stylesheet can say
//!
//! There is no CSS property behind either, so the cascade cannot carry them and
//! `tinker_pdf_layout::style::consume`'s compile-time device — which is about
//! computed styles — has nothing to say about them. They come off the markup in
//! `epub::read` and arrive on `BoxNode::span`, and this file is the only place
//! that path is asserted end to end.
//!
//! **HTML's own parsing rules, not `str::parse`.** `colspan="2 "` is two
//! columns and `colspan="x"` is one; `rowspan="0"` is *to the end of the row
//! group* and is the one case where zero is a value rather than a mistake.
//! Every one of those is a different number for a different reason and each has
//! its own case below.
//!
//! # Conservation across every table fixture
//!
//! Milestone 10 found that reading order stops being emission order the moment
//! a float exists. A table is the same problem in two dimensions and then a
//! third: a row's cells sit beside each other, a `<tfoot>` written first is
//! drawn last, and a table that crosses a page boundary finishes on the page
//! after. None of the three loses a character and all three would fail an
//! *ordered* comparison against the source, which is what text conservation is.

mod epub_support;

use tinker_pdf::epub::paint::BookMetrics;
use tinker_pdf::epub::read::{box_tree, PX_TO_PT, UA_STYLESHEET};
use tinker_pdf::epub::{xhtml, DEFAULT_FONT_SIZE};
use tinker_pdf::{Document, OpenOptions};
use tinker_pdf_css::cascade::{cascade_from, ComputedStyle, Origin, StyleTree};
use tinker_pdf_css::media::MediaContext;
use tinker_pdf_css::parser::parse as css_parse;
use tinker_pdf_css::property::Display;
use tinker_pdf_css::{Budget as CssBudget, Limits as CssLimits, NoImports};
use tinker_pdf_layout::{layout, Layout, Limits as LayoutLimits, Options, TextRun};

use epub_support::conservation::conservation;
use epub_support::{ocf_zip, OcfEntry};

// ---- the fixtures ------------------------------------------------------------

const CONTAINER_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" "#,
    r#"media-type="application/oebps-package+xml"/></rootfiles></container>"#
);

const PACKAGE: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
    r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
    r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-0000000000b1</dc:identifier>"#,
    r#"<dc:title>A Book Of Tables</dc:title><dc:language>en</dc:language>"#,
    r#"</metadata><manifest>"#,
    r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
);

/// A content document whose `<body>` is `body`.
fn chapter(body: &str) -> String {
    format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Tables</title>"#,
            r#"</head><body>{}</body></html>"#
        ),
        body
    )
}

/// A whole container holding one chapter.
fn book(body: &str) -> Vec<u8> {
    let chapter = chapter(body);
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", PACKAGE.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// The markup, the user-agent sheet, the cascade and layout, at a stated page.
///
/// The same path `epub_browser.rs` uses, and for the same reason: a fixture
/// that asked the reader for its pages could see *whether* the text arrived and
/// not *where*, and where is the whole of §17.
fn engine(body: &str, width_px: f64, height_px: f64) -> (xhtml::Dom, StyleTree, Layout) {
    let document = chapter(body);
    let dom = xhtml::read(document.as_bytes(), &tinker_pdf_xml::Limits::DEFAULT).expect("markup");
    // The cascade and the layout below are `layout_document`'s, kept separate
    // only because these fixtures need the tree and the styles back as well.
    let media = MediaContext::screen(width_px, height_px);
    let limits = CssLimits::DEFAULT;
    let mut budget = CssBudget::new(&limits);
    let ua = css_parse(
        UA_STYLESHEET.as_bytes(),
        None,
        &NoImports,
        &media,
        &limits,
        &mut budget,
    )
    .expect("the committed sheet");
    let mut initial = ComputedStyle::initial();
    initial.font_size = DEFAULT_FONT_SIZE / PX_TO_PT;
    let styles = cascade_from(
        &[(Origin::UserAgent, &ua)],
        &dom.nodes,
        &limits,
        &mut budget,
        &initial,
    )
    .expect("a cascade");
    let laid = layout(
        &box_tree(&dom, &styles),
        &BookMetrics::STANDARD,
        &Options::new(width_px, height_px),
        &LayoutLimits::DEFAULT,
    )
    .expect("a layout");
    (dom, styles, laid)
}

/// Every run of the first page, in reading order.
fn runs(laid: &Layout) -> Vec<&TextRun> {
    laid.pages[0].runs.iter().filter(|r| !r.generated).collect()
}

/// The run whose text is exactly `needle`.
fn run_of<'a>(laid: &'a Layout, needle: &str) -> &'a TextRun {
    runs(laid)
        .into_iter()
        .find(|run| run.text.trim() == needle)
        .unwrap_or_else(|| {
            panic!(
                "no run reads {needle:?}; the page has {:?}",
                runs(laid).iter().map(|r| &r.text).collect::<Vec<_>>()
            )
        })
}

/// Every non-whitespace character, which is the stream conservation is about.
fn conservable(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

// ---- §17.2.1's fixup, on the markup a real book writes -------------------------

/// **A `<table>` with no `<tbody>` in it lays out as a table**, which is the
/// markup every table in the committed corpus is written in.
///
/// XHTML has no tree-construction stage, so nothing puts the row group back:
/// §17.2.1's generation is the only thing between this document and a table
/// with no rows in it. Three consequences and all three are asserted — the
/// cells of a row share a baseline, the second row is under the first, and no
/// character is lost.
#[test]
fn a_table_with_no_tbody_lays_out_as_a_table() {
    let (_, _, laid) = engine(
        "<table><tr><td>one</td><td>two</td></tr>\
         <tr><td>three</td><td>four</td></tr></table>",
        400.0,
        800.0,
    );
    let one = run_of(&laid, "one");
    let two = run_of(&laid, "two");
    let three = run_of(&laid, "three");
    assert_eq!(
        one.y, two.y,
        "the first row's cells do not share a baseline"
    );
    assert!(
        two.x > one.x,
        "the second cell is not to the right of the first"
    );
    assert!(three.y > one.y, "the second row is not under the first");
    assert_eq!(conservable(&laid.text()), conservable("onetwothreefour"));
}

/// And the newline a producer writes between the tags is **not** a cell.
///
/// The same document, indented the way a producer indents it. Without
/// §17.2.1's rule 3 each newline becomes an anonymous cell and the table has
/// twice the columns it should.
#[test]
fn the_indentation_between_two_cells_is_not_a_cell() {
    let (_, _, laid) = engine(
        "<table>\n  <tr>\n    <td>one</td>\n    <td>two</td>\n  </tr>\n</table>",
        400.0,
        800.0,
    );
    assert_eq!(runs(&laid).len(), 2, "{:?}", runs(&laid));
    assert_eq!(conservable(&laid.text()), "onetwo");
}

/// A `<div>` a producer left between two `<td>`s is a cell of its own and not
/// a lost paragraph — §17.2.1's rule 7 on real markup.
#[test]
fn a_div_between_two_cells_is_a_cell_of_its_own() {
    let (_, _, laid) = engine(
        "<table><tr><td>one</td><div>middle</div><td>two</td></tr></table>",
        600.0,
        800.0,
    );
    assert_eq!(conservable(&laid.text()), "onemiddletwo");
    let middle = run_of(&laid, "middle");
    // Within a point of the real cells' baseline rather than exactly on it: an
    // anonymous box takes the **initial** value of every non-inherited
    // property (§17.2.1), and the user-agent sheet's `td { padding: 1px }` is
    // on the `<td>`s and not on the box this build generated. A point is the
    // padding; a line would be a row.
    assert!(
        (middle.y - run_of(&laid, "one").y).abs() < 2.0,
        "the stray div became a row rather than a cell: {} against {}",
        middle.y,
        run_of(&laid, "one").y
    );
}

/// A `<tr>` outside any `<table>` gets an anonymous table — §17.2.1's rule 9,
/// which is the ninth generation step and the one that runs a level up.
#[test]
fn a_row_outside_a_table_still_lays_out() {
    let (_, _, laid) = engine(
        "<div><tr><td>one</td><td>two</td></tr></div><p>after</p>",
        400.0,
        800.0,
    );
    assert_eq!(conservable(&laid.text()), "onetwoafter");
    assert_eq!(
        run_of(&laid, "one").y,
        run_of(&laid, "two").y,
        "the stray row's cells are not on one row"
    );
}

// ---- the attributes ------------------------------------------------------------

/// `colspan` reaches layout off the markup.
#[test]
fn colspan_reaches_layout_from_the_attribute() {
    let (_, _, laid) = engine(
        "<table style=\"table-layout: fixed; width: 300px\">\
         <tr><td colspan=\"2\">wide</td><td>narrow</td></tr>\
         <tr><td>a</td><td>b</td><td>c</td></tr></table>",
        400.0,
        800.0,
    );
    let wide = run_of(&laid, "wide");
    let narrow = run_of(&laid, "narrow");
    let c = run_of(&laid, "c");
    assert!(
        (narrow.x - c.x).abs() < 0.5,
        "the spanning cell did not take two columns: narrow at {} and c at {}",
        narrow.x,
        c.x
    );
    assert!(wide.x < narrow.x);
}

/// `rowspan` reaches layout off the markup, and the row under it starts one
/// column to the right.
#[test]
fn rowspan_reaches_layout_from_the_attribute() {
    let (_, _, laid) = engine(
        "<table><tr><td rowspan=\"2\">tall</td><td>first</td></tr>\
         <tr><td>second</td></tr></table>",
        400.0,
        800.0,
    );
    let first = run_of(&laid, "first");
    let second = run_of(&laid, "second");
    assert!(
        (first.x - second.x).abs() < 0.5,
        "the second row is not beside the spanning cell: {} and {}",
        first.x,
        second.x
    );
    assert!(second.y > first.y);
    assert_eq!(conservable(&laid.text()), "tallfirstsecond");
}

/// **HTML's own parsing rules for the two attributes, each case for its own
/// reason.**
///
/// A build using `str::parse` gives `colspan="2 "` one column, which is a table
/// with a hole in it; a build clamping `rowspan="0"` to one gives a book's
/// `rowspan="0"` a one-row cell with the rest of the column shifted up. The two
/// are different mistakes and this asserts against both.
#[test]
/// **`"2 "` is not the fixture this needs**, and the injection matrix said so:
/// `str::parse` on a trimmed string reads it perfectly well, so a build with
/// HTML's rule deleted passed. `"2x"` is the case the rule is *for* — leading
/// digits, then anything — and it is what a real attribute with a stray unit or
/// a typo in it looks like.
fn a_colspan_with_trailing_rubbish_is_its_leading_digits() {
    let (_, _, trailing) = engine(
        "<table style=\"table-layout: fixed; width: 300px\">\
         <tr><td colspan=\"2x\">wide</td><td>narrow</td></tr>\
         <tr><td>a</td><td>b</td><td>c</td></tr></table>",
        400.0,
        800.0,
    );
    assert!(
        (run_of(&trailing, "narrow").x - run_of(&trailing, "c").x).abs() < 0.5,
        "colspan=\"2x\" was not read as two"
    );
}

/// A `colspan` with no leading digit at all is one column, not a refusal and
/// not a zero.
#[test]
fn a_colspan_that_is_not_a_number_is_one_column() {
    // `"x"` is one, because there is no leading digit at all.
    let (_, _, rubbish) = engine(
        "<table style=\"table-layout: fixed; width: 300px\">\
         <tr><td colspan=\"x\">wide</td><td>narrow</td></tr>\
         <tr><td>a</td><td>b</td><td>c</td></tr></table>",
        400.0,
        800.0,
    );
    assert!(
        (run_of(&rubbish, "narrow").x - run_of(&rubbish, "b").x).abs() < 0.5,
        "colspan=\"x\" was not read as one"
    );
}

/// **`rowspan="0"` survives the clamp**, which is the one case where zero is a
/// value and not a mistake: HTML reads it as *to the end of this row group*.
///
/// Its own test and not a third assertion in another one, because the matrix
/// reports a test's name: a build that clamped it to one and a build that
/// misread `colspan="2 "` are two different mistakes in two different clamps
/// and a single failing name could not say which.
#[test]
fn a_rowspan_of_zero_reaches_the_end_of_its_row_group() {
    // `rowspan="0"` reaches the end of the row group, so **both** later rows
    // stand beside it.
    let (_, _, zero) = engine(
        "<table><tbody><tr><td rowspan=\"0\">tall</td><td>first</td></tr>\
         <tr><td>second</td></tr><tr><td>third</td></tr></tbody></table>",
        400.0,
        800.0,
    );
    let first = run_of(&zero, "first").x;
    assert!(
        (run_of(&zero, "second").x - first).abs() < 0.5
            && (run_of(&zero, "third").x - first).abs() < 0.5,
        "rowspan=\"0\" did not reach the end of its row group"
    );
}

// ---- the user-agent sheet ------------------------------------------------------

/// **The user-agent sheet's table rules are what make a `<table>` a table**,
/// and every one of them was `Unsupported` until this milestone.
#[test]
fn the_user_agent_sheet_gives_the_table_elements_their_display() {
    let (dom, styles, _) = engine(
        "<table><caption>cap</caption><colgroup><col/></colgroup>\
         <thead><tr><th>h</th></tr></thead><tfoot><tr><td>f</td></tr></tfoot>\
         <tbody><tr><td>b</td></tr></tbody></table>",
        400.0,
        800.0,
    );
    let display_of = |name: &str| {
        let at = dom
            .nodes
            .iter()
            .position(|node| node.name == name)
            .unwrap_or_else(|| panic!("no <{name}>"));
        styles.styles[at].display
    };
    assert_eq!(display_of("table"), Display::Table);
    assert_eq!(display_of("caption"), Display::TableCaption);
    assert_eq!(display_of("colgroup"), Display::TableColumnGroup);
    assert_eq!(display_of("col"), Display::TableColumn);
    assert_eq!(display_of("thead"), Display::TableHeaderGroup);
    assert_eq!(display_of("tfoot"), Display::TableFooterGroup);
    assert_eq!(display_of("tbody"), Display::TableRowGroup);
    assert_eq!(display_of("tr"), Display::TableRow);
    assert_eq!(display_of("td"), Display::TableCell);
    assert_eq!(display_of("th"), Display::TableCell);
}

/// And it carries HTML's two numbers, which are not decoration: without
/// `border-spacing: 2px` every unstyled table's cells touch, and without the
/// cell padding a bordered cell's text sits on its border.
#[test]
fn the_user_agent_sheet_carries_htmls_own_border_spacing() {
    let (_, styles, _) = engine("<table><tr><td>a</td></tr></table>", 400.0, 800.0);
    let table = styles
        .styles
        .iter()
        .find(|style| style.display == Display::Table)
        .expect("a table");
    assert_eq!(table.border_spacing.horizontal, 2.0);
    assert_eq!(table.border_spacing.vertical, 2.0);
    assert_eq!(
        table.border_collapse,
        tinker_pdf_css::property::BorderCollapse::Separate
    );
}

/// And §15.3.8's cell padding, which is a different declaration for a
/// different reason: without it a bordered cell's text sits on its border.
#[test]
fn the_user_agent_sheet_carries_htmls_own_cell_padding() {
    let (_, styles, _) = engine("<table><tr><td>a</td></tr></table>", 400.0, 800.0);
    let cell = styles
        .styles
        .iter()
        .find(|style| style.display == Display::TableCell)
        .expect("a cell");
    assert_eq!(
        cell.padding.top,
        tinker_pdf_css::property::LengthPercentage::Px(1.0)
    );
}

/// A `<tfoot>` written before its `<tbody>` — which HTML 4.01 required and real
/// books therefore contain — is **read** where it was written and **drawn**
/// under the body.
#[test]
fn a_footer_written_first_is_drawn_last_and_read_first() {
    let (_, _, laid) = engine(
        "<table><tfoot><tr><td>foot</td></tr></tfoot>\
         <tbody><tr><td>body</td></tr></tbody></table>",
        400.0,
        800.0,
    );
    assert_eq!(
        conservable(&laid.text()),
        "footbody",
        "the reading order is not the document's"
    );
    assert!(
        run_of(&laid, "foot").y > run_of(&laid, "body").y,
        "the footer group was drawn above the body"
    );
}

/// **`border-spacing` inherits, and it takes a `display: table` that is not a
/// `<table>` to see it.**
///
/// CSS 2.2 §17.6.1 says *inherited: yes*, and the injection matrix found that
/// nothing here observed it: deleting the inheritance changed no test at all.
/// The first fixture written for it did not work either, and **why** is the
/// finding. This build reads `border-spacing` off the *table* box, and the
/// user-agent sheet declares it on **every** `<table>` — HTML's own
/// `border-spacing: 2px` — so a `<table>` nested in a `<table>` has a
/// declaration of its own and a declared value beats an inherited one. Every
/// browser behaves the same way for the same reason.
///
/// What has no user-agent rule is a `display: table` that is not a `<table>`,
/// which is what a stylesheet writes on a `<div>` and what decision 5's whole
/// `display` family exists for. That box inherits, and this is the fixture.
#[test]
fn a_table_that_is_not_a_table_element_inherits_its_border_spacing() {
    let document = |spacing: &str| {
        format!(
            "<table style=\"border-spacing: {spacing}\"><tr><td>\
             <div style=\"display: table\"><div style=\"display: table-row\">\
             <div style=\"display: table-cell\">i</div>\
             <div style=\"display: table-cell\">j</div>\
             </div></div></td></tr></table>"
        )
    };
    let wide = engine(&document("20px"), 600.0, 800.0).2;
    let tight = engine(&document("0"), 600.0, 800.0).2;
    let gap = |laid: &Layout| run_of(laid, "j").x - run_of(laid, "i").x;
    assert!(
        gap(&wide) > gap(&tight) + 10.0,
        "the inner table did not inherit the outer one's border-spacing: \
         {} against {}",
        gap(&wide),
        gap(&tight)
    );
}

/// And so does `border-collapse`, which is a different property with a
/// different consequence: it decides whether the inner table's cells share
/// their borders or each draw their own.
#[test]
fn a_table_that_is_not_a_table_element_inherits_its_border_collapse() {
    let document = |collapse: &str| {
        format!(
            "<table style=\"border-collapse: {collapse}; border-spacing: 0\"><tr><td>\
             <div style=\"display: table\"><div style=\"display: table-row\">\
             <div style=\"display: table-cell; border: 8px solid #000000\">i</div>\
             <div style=\"display: table-cell; border: 8px solid #000000\">j</div>\
             </div></div></td></tr></table>"
        )
    };
    let collapsed = engine(&document("collapse"), 600.0, 800.0).2;
    let separate = engine(&document("separate"), 600.0, 800.0).2;
    let gap = |laid: &Layout| run_of(laid, "j").x - run_of(laid, "i").x;
    assert!(
        gap(&collapsed) < gap(&separate) - 4.0,
        "the inner table did not inherit the outer one's border-collapse: \
         {} against {}",
        gap(&collapsed),
        gap(&separate)
    );
}

// ---- conservation, across every table fixture -----------------------------------

/// **A whole book of tables conserves every character of its spine.**
///
/// The oracle is `epub_support::conservation`, which reads the spine out of the
/// container's bytes with its own scanners rather than asking this engine which
/// order it chose. A dropped cell is a dropped paragraph and this is its only
/// witness: a table with one column missing renders beautifully.
#[test]
fn a_book_of_tables_conserves_every_character() {
    for (name, body) in table_fixtures() {
        let bytes = book(body);
        let doc = Document::open(bytes.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let verdict = conservation(&bytes, &doc);
        assert!(
            verdict.holds(),
            "{name} does not conserve its text: {:?}",
            verdict.figure()
        );
    }
}

/// And it still conserves it when the table is cut in half by a page boundary.
///
/// A page box short enough that the table spans several pages, which is where a
/// fragmenter drops or repeats a row. The page count is asserted as well as the
/// text, because a book that lost the whole table would conserve nothing and
/// also produce one page.
#[test]
fn a_table_across_a_page_boundary_conserves_every_character() {
    let mut body = String::from("<table>");
    for row in 0..40 {
        body.push_str(&format!(
            "<tr><td>row{row}alpha</td><td>row{row}beta</td></tr>"
        ));
    }
    body.push_str("</table>");
    let bytes = book(&body);
    let doc = Document::open_with(bytes.clone(), &OpenOptions::at_page(288.0, 200.0))
        .expect("a book of one long table");
    assert!(
        doc.page_count() > 3,
        "the table did not cross a page boundary at all: {} pages",
        doc.page_count()
    );
    let verdict = conservation(&bytes, &doc);
    assert!(
        verdict.holds(),
        "a table across a page boundary lost or repeated text: {:?}",
        verdict.figure()
    );
}

/// The fixtures conservation is asserted over, one per shape §17 distinguishes.
///
/// A list rather than one large document, because a single fixture that
/// exercised all of them and passed would say nothing about which of them ran —
/// and because a failure names the shape.
fn table_fixtures() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "a table with no tbody",
            "<table><tr><td>one</td><td>two</td></tr>\
             <tr><td>three</td><td>four</td></tr></table>",
        ),
        (
            "a table indented the way a producer indents it",
            "<table>\n  <tbody>\n    <tr>\n      <td>one</td>\n      <td>two</td>\n \
             </tr>\n  </tbody>\n</table>",
        ),
        (
            "a caption, a header group and a footer group written first",
            "<table><caption>the caption</caption>\
             <tfoot><tr><td>foot one</td><td>foot two</td></tr></tfoot>\
             <thead><tr><th>head one</th><th>head two</th></tr></thead>\
             <tbody><tr><td>body one</td><td>body two</td></tr></tbody></table>",
        ),
        (
            "colspan and rowspan",
            "<table><tr><td colspan=\"2\">across</td><td>third</td></tr>\
             <tr><td rowspan=\"2\">down</td><td>beside</td><td>far</td></tr>\
             <tr><td>under</td><td>right</td></tr></table>",
        ),
        (
            "a nested table",
            "<table><tr><td><table><tr><td>inner one</td><td>inner two</td></tr></table></td>\
             <td>outer</td></tr></table>",
        ),
        (
            "a collapsing table with borders",
            "<table style=\"border-collapse: collapse\">\
             <tr><td style=\"border: 2px solid #333333\">left</td>\
             <td style=\"border: 1px dashed #666666\">right</td></tr></table>",
        ),
        (
            "a column group and columns",
            "<table><colgroup><col span=\"2\"/></colgroup>\
             <tr><td>one</td><td>two</td></tr></table>",
        ),
        (
            "a stray div in a row and a stray paragraph in a table",
            "<table><p>stray</p><tr><td>one</td><div>middle</div><td>two</td></tr></table>",
        ),
        (
            "a row outside any table",
            "<div><tr><td>one</td><td>two</td></tr></div><p>after</p>",
        ),
        (
            "a table with a paragraph on either side",
            "<p>before</p><table><tr><td>cell one</td><td>cell two</td></tr></table>\n             <p>after</p>",
        ),
    ]
}

/// **And every fixture is genuinely a table**, which is what stops the
/// conservation sweep above from passing on a build with no table model at all.
///
/// Conservation is an ordering of characters and says nothing about geometry: a
/// build that set every cell as inline text would conserve all ten fixtures.
/// This is the second consequence — in every fixture that has two cells in one
/// row, those two cells share a baseline and stand side by side.
#[test]
fn every_conservation_fixture_is_a_table_and_not_a_paragraph() {
    for (name, body) in table_fixtures() {
        let (_, _, laid) = engine(body, 500.0, 4000.0);
        assert!(
            abreast(&laid) >= 2,
            "{name} has no two cells side by side, so it is not laid out as a table"
        );
    }
}

// ---- the corpus ----------------------------------------------------------------

/// **The committed corpus's own tables lay out as tables**, and the reading
/// path this file asserts on synthetic markup is the one they take.
///
/// **And it records which of §17.2.1's steps a real book actually needs**,
/// which is not the one this milestone was written expecting.
///
/// The plan's row says the fixup a real book needs is the missing `<tbody>`.
/// It is not, for these two producers: both pandoc and calibre write
/// `<thead>` and `<tbody>` in full, and this fixture asserts that rather than
/// the opposite, because a claim measured to be false is worth more written
/// down than deleted. What every one of them **does** write is indentation
/// between the tags, so §17.2.1's rule 3 — white space between two proper table
/// children — fires on every table in the corpus and is the step that would
/// otherwise put an empty anonymous cell between every pair of real ones. The
/// bare-`<tr>` table is still the markup hand-written and legacy HTML uses, and
/// it has its own fixtures above; it is simply not what these two producers
/// emit.
///
/// Three claims, because a table with one row would satisfy the first alone.
#[test]
fn a_real_books_table_puts_its_cells_side_by_side() {
    for name in ["pandoc-book-cover.epub", "calibre-book-cover.epub"] {
        let bytes = corpus_book(name);
        let document = tabled_document(&bytes)
            .unwrap_or_else(|| panic!("{name} has no content document with a table in it"));
        assert!(
            document.contains("<tbody"),
            "{name}'s table has no tbody, which is not what this corpus was \
             measured to contain"
        );
        assert!(
            document.contains(">\n<td") || document.contains(">\n  <td"),
            "{name}'s table is not indented, so §17.2.1's rule 3 never fires on it"
        );
        assert!(
            document.matches("<td").count() >= 6,
            "{name}'s table has fewer than six cells"
        );
        let laid = layout_document(&document, 400.0, 8000.0);
        assert!(
            abreast(&laid) >= 3,
            "{name}'s widest line carries {} runs abreast, so its table's cells \
             are not side by side",
            abreast(&laid)
        );
    }
}

/// The most runs any one baseline of any page carries, counting only runs that
/// stand apart from each other in `x`.
///
/// Two cells of one row share a baseline and are a point apart at least; two
/// lines of one paragraph do not share a baseline at all. It is therefore the
/// smallest measurement that can tell a table from a column of text, which is
/// what every fixture in this file needs and what a conservation check cannot
/// see.
fn abreast(laid: &Layout) -> usize {
    let mut most = 0usize;
    for page in &laid.pages {
        let runs: Vec<&TextRun> = page.runs.iter().filter(|run| !run.generated).collect();
        for run in &runs {
            // **Within four points of each other, not exactly equal.** Two
            // cells of one row are set from their own content tops (§17.5.3),
            // so a cell with a two-point border beside one with a one-point
            // border has its first baseline a point lower — and an exact
            // comparison called that a column of text. Four points is a fifth
            // of a line here, so it cannot reach the row above or below.
            let mut columns: Vec<f64> = runs
                .iter()
                .filter(|other| (other.y - run.y).abs() < 4.0)
                .map(|other| other.x)
                .collect();
            columns.sort_by(f64::total_cmp);
            columns.dedup_by(|a, b| (*a - *b).abs() <= 1.0);
            most = most.max(columns.len());
        }
    }
    most
}

/// The first content document in a book that has a `<table>` in it.
fn tabled_document(bytes: &[u8]) -> Option<String> {
    let mut archive =
        tinker_pdf_zip::Archive::open(bytes, &tinker_pdf_zip::Limits::DEFAULT).ok()?;
    let names: Vec<String> = archive
        .entries()
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    for (at, name) in names.iter().enumerate() {
        if !(name.ends_with(".xhtml") || name.ends_with(".html")) {
            continue;
        }
        let Ok(data) = archive.read(at) else {
            continue;
        };
        let text = String::from_utf8_lossy(&data).into_owned();
        if text.contains("<table") {
            return Some(text);
        }
    }
    None
}

/// One whole content document, through the reading path, at a stated page.
fn layout_document(document: &str, width_px: f64, height_px: f64) -> Layout {
    let dom = xhtml::read(document.as_bytes(), &tinker_pdf_xml::Limits::DEFAULT).expect("markup");
    let media = MediaContext::screen(width_px, height_px);
    let limits = CssLimits::DEFAULT;
    let mut budget = CssBudget::new(&limits);
    let ua = css_parse(
        UA_STYLESHEET.as_bytes(),
        None,
        &NoImports,
        &media,
        &limits,
        &mut budget,
    )
    .expect("the committed sheet");
    let mut initial = ComputedStyle::initial();
    initial.font_size = DEFAULT_FONT_SIZE / PX_TO_PT;
    let styles = cascade_from(
        &[(Origin::UserAgent, &ua)],
        &dom.nodes,
        &limits,
        &mut budget,
        &initial,
    )
    .expect("a cascade");
    layout(
        &box_tree(&dom, &styles),
        &BookMetrics::STANDARD,
        &Options::new(width_px, height_px),
        &LayoutLimits::DEFAULT,
    )
    .expect("a layout")
}

fn corpus_book(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}
