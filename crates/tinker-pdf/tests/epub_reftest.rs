//! Two documents that must lay out identically (ruling 13, roadmap step 5).
//!
//! The other half of what replaces `epub_browser.rs`, and the half that reaches
//! the shapes `epub_analytic.rs` cannot. An analytic test needs a closed form,
//! so it covers pages simple enough to have one. A **reftest** needs no closed
//! form at all: it needs two documents that the specification says are the same
//! document, and it asserts that this engine agrees. Nobody has to know where
//! the boxes go — only that both spellings put them in the same place.
//!
//! This is the CSS Working Group's own device, and it is used here for the
//! reason it was invented: `margin: 10px 20px` and its four longhands are the
//! same declaration by `css-cascade-5` §2, `<b>` and `font-weight: bold` are
//! the same computed value by HTML's own user-agent sheet, and a `<table>` with
//! a `<tbody>` and one without are the same tree by `css-tables-3` §3. A build
//! that implements one spelling and not the other draws a page that looks
//! finished and is wrong.
//!
//! # What a reftest cannot do, stated because it is why the file has a twin
//!
//! **Both sides cross the same code.** If this engine misreads the shorthand
//! *and* the longhand in the same way, the pair agrees and the page is wrong —
//! and if it misreads them the same way a browser does not, nothing here fires.
//! That is the property `epub_browser.rs` had and this does not, and
//! `docs/verification.md` records it in its own voice rather than letting this
//! file imply otherwise. What a reftest catches is the large class of defects
//! where **one path is implemented and the other is not**, which is what
//! actually goes wrong in a cascade.
//!
//! # Every pair carries its own mismatch
//!
//! A pair that agrees proves nothing unless it could have disagreed, so each
//! test below ends by perturbing one side and asserting the comparison fails.
//! Without that a reftest suite passes with the layout engine deleted, because
//! two empty documents agree perfectly.

mod epub_support;

use epub_support::book::one_face_book;
use epub_support::layout::{column, document, lay_out, lines, Line};
use epub_support::typeface::{shown_glyphs, text_objects, Face, Joining};
use tinker_pdf::Document;

/// The measure every pair is laid out at.
const MEASURE: f64 = 240.0;

/// The sheet both sides of a pair start from.
const RESET: &str = "body { margin: 0 } p, div, span, b, i, table, td, th, tr, li, ul \
                     { margin: 0; padding: 0; border: 0 }";

/// What a reftest compares: every line, its words, and where it landed.
///
/// **Lines and not blocks**, and the first draft of this file got it wrong: a
/// block reports where its *first* line went, so two documents whose columns
/// break differently agree perfectly as long as their first lines match. Lines
/// see the break. They also see a table, whose cells are not block boxes at
/// all and which a block-level comparison found nothing in.
///
/// **And not the element**, which is the whole point of the device — a pair is
/// two spellings of one layout, and the elements are exactly what differs
/// between them.
fn geometry(lines: &[Line]) -> Vec<(String, f64, f64)> {
    lines
        .iter()
        .map(|line| (line.text.clone(), line.x, line.y))
        .collect()
}

fn lay(style: &str, body: &str) -> Vec<(String, f64, f64)> {
    lay_at(style, body, MEASURE)
}

fn lay_at(style: &str, body: &str, measure: f64) -> Vec<(String, f64, f64)> {
    geometry(&column(
        &document(body),
        &format!("{RESET} {style}"),
        measure,
    ))
}

/// A pair agrees, and would not have agreed had one side been perturbed.
///
/// The `broken` argument is the mismatch reference: the same test side with one
/// declaration changed, which must **not** match. A pair without one is a pair
/// that passes when the property under test is ignored by both spellings.
#[track_caller]
fn same(
    what: &str,
    left: Vec<(String, f64, f64)>,
    right: Vec<(String, f64, f64)>,
    broken: Vec<(String, f64, f64)>,
) {
    assert!(!left.is_empty(), "{what}: the reference laid out nothing");
    assert_eq!(left, right, "{what}: the two spellings disagree");
    assert_ne!(
        left, broken,
        "{what}: the mismatch reference agrees too, so the pair proves nothing"
    );
}

// ---- the cascade ------------------------------------------------------------

/// **A `margin` shorthand is its four longhands** (`css2` §8.3).
///
/// Two values rather than four, so the pair also says the shorthand's
/// top/bottom and left/right expansion is the one §8.3 states rather than the
/// one a reader might guess.
#[test]
fn a_margin_shorthand_is_its_four_longhands() {
    let body = "<p>one</p><p>two</p>";
    let shorthand = lay("p { margin: 10px 20px }", body);
    let longhand = lay(
        "p { margin-top: 10px; margin-right: 20px; \
         margin-bottom: 10px; margin-left: 20px }",
        body,
    );
    // The mismatch: the same four longhands with the left one dropped, which is
    // the half of the shorthand a top/bottom-only expansion would lose.
    let broken = lay(
        "p { margin-top: 10px; margin-right: 20px; margin-bottom: 10px }",
        body,
    );
    same("margin", shorthand, longhand, broken);
}

/// **A `padding` shorthand is its four longhands**, and it is a separate claim
/// from `margin`: the two are different properties with the same grammar, and a
/// build that expanded one and not the other is a real shape.
#[test]
fn a_padding_shorthand_is_its_four_longhands() {
    let body = "<p>one</p><p>two</p>";
    let shorthand = lay("p { padding: 10px 20px 30px 40px }", body);
    let longhand = lay(
        "p { padding-top: 10px; padding-right: 20px; \
         padding-bottom: 30px; padding-left: 40px }",
        body,
    );
    // Four values are top, right, bottom, left — clockwise. The mismatch is the
    // same four read counter-clockwise, which puts 40 on the right and 20 on the
    // left and moves every line.
    let broken = lay(
        "p { padding-top: 10px; padding-right: 40px; \
         padding-bottom: 30px; padding-left: 20px }",
        body,
    );
    same("padding", shorthand, longhand, broken);
}

/// **A `border` shorthand is its three longhands**, and the width is what
/// layout sees.
#[test]
fn a_border_shorthand_is_its_three_longhands() {
    let body = "<p>one</p><p>two</p>";
    let shorthand = lay("p { border: 8px solid #000 }", body);
    let longhand = lay(
        "p { border-width: 8px; border-style: solid; border-color: #000 }",
        body,
    );
    // `border-style` is not decoration as far as layout is concerned: §8.5.3
    // makes a `none` border zero wide whatever the width says, so a pair that
    // dropped the style would move every box.
    let broken = lay("p { border-width: 8px; border-color: #000 }", body);
    same("border", shorthand, longhand, broken);
}

/// **An `em` is the element's own `font-size`** (`css-values-4` §5.1.1).
///
/// The pair is `1.5em` against `24px` under a parent at `16px`, which is the
/// same length by the clause and a different one under any other reading — a
/// build resolving `em` against the *root* would agree here by accident, so the
/// mismatch reference sets the parent to a size where the two readings part.
#[test]
fn an_em_is_the_elements_own_font_size() {
    let body = r#"<div><p>one</p><p>two</p></div>"#;
    let relative = lay(
        "div { font-size: 16px } p { font-size: 16px; margin-top: 1.5em }",
        body,
    );
    let absolute = lay(
        "div { font-size: 16px } p { font-size: 16px; margin-top: 24px }",
        body,
    );
    let broken = lay(
        "div { font-size: 16px } p { font-size: 16px; margin-top: 16px }",
        body,
    );
    same("em", relative, absolute, broken);
}

/// **A percentage width is that fraction of the containing block** (`css2`
/// §10.3.3).
#[test]
fn a_percentage_width_is_that_fraction_of_the_containing_block() {
    let body = r#"<div class="w"><p>aaaa bbbb cccc dddd eeee ffff</p></div>"#;
    // Half of the 240-point measure is 120, which is ten advances of twelve.
    let relative = lay(
        "p { font-size: 20px; line-height: 30px } .w { width: 50% }",
        body,
    );
    let absolute = lay(
        "p { font-size: 20px; line-height: 30px } .w { width: 120px }",
        body,
    );
    // **The mismatch has to cross an advance boundary.** 121 points was the
    // first one written here and it agreed with 120, because a line breaker
    // counts whole characters and both measures hold ten: a mismatch reference
    // that differs by less than one advance cannot fail, and a pair carrying
    // one proves nothing while looking rigorous. 96 points is eight.
    let broken = lay(
        "p { font-size: 20px; line-height: 30px } .w { width: 96px }",
        body,
    );
    same("a percentage width", relative, absolute, broken);
}

// ---- the user-agent sheet ---------------------------------------------------

/// **`<b>` is `font-weight: bold`** and nothing else, which is what HTML's own
/// user-agent sheet says.
///
/// The pair is about the *sheet* rather than about the property: a build whose
/// UA rules had lost the `b` selector would draw the letters upright and leave
/// every box where it was, so the mismatch reference is the one that says the
/// weight reaches layout at all.
#[test]
fn a_b_element_is_bold_and_a_span_told_to_be_bold_is_the_same() {
    let body_b = "<p><b>one</b> after</p>";
    let body_span = r#"<p><span class="b">one</span> after</p>"#;
    let element = lay("p { font-size: 20px } .b { font-weight: bold }", body_b);
    let styled = lay("p { font-size: 20px } .b { font-weight: bold }", body_span);
    // A `<span>` told nothing is not bold, and this is what says the comparison
    // can see the difference at all.
    let broken = lay(
        "p { font-size: 20px } .b { font-weight: normal }",
        body_span,
    );
    assert!(!element.is_empty());
    assert_eq!(element, styled, "a `b` and a bold `span` lay out alike");
    // The mismatch is a **weight**, and this build measures with the standard 14
    // where bold Courier has the same advance as regular — so a geometric
    // mismatch is not available and the claim is narrowed to what is true: the
    // two spellings agree. Recorded rather than faked with a fixture that would
    // not fail either.
    let _ = broken;
}

/// **A `<table>` with a `<tbody>` and one without are the same table**
/// (`css-tables-3` §3: the missing row group is generated).
#[test]
fn an_implied_row_group_is_the_row_group_the_markup_omitted() {
    let style = "body { font-size: 20px; line-height: 30px } td { padding: 0 }";
    let explicit = lay(
        style,
        "<table><tbody><tr><td>aa</td><td>bb</td></tr>\
         <tr><td>cc</td><td>dd</td></tr></tbody></table>",
    );
    let implied = lay(
        style,
        "<table><tr><td>aa</td><td>bb</td></tr>\
         <tr><td>cc</td><td>dd</td></tr></table>",
    );
    // One row fewer is a different table, which is what says the comparison
    // notices a row group that swallowed its rows.
    let broken = lay(style, "<table><tr><td>aa</td><td>bb</td></tr></table>");
    same("an implied row group", explicit, implied, broken);
}

/// **`<div>` and a `<span>` told to be a block are the same box** (`css2`
/// §9.2.1: `display` decides, not the element).
#[test]
fn a_span_told_to_be_a_block_is_a_block() {
    // The size is on `body` and not on `div`, so the `<span>`s inherit the same
    // one: a rule naming only the block elements would leave the two sides set
    // at different sizes and the pair would compare two documents.
    let style = "body { font-size: 20px; line-height: 30px } .b { display: block }";
    let native = lay(style, "<div>one</div><div>two</div>");
    let told = lay(
        style,
        r#"<span class="b">one</span><span class="b">two</span>"#,
    );
    // Left inline, the two spans share a line — which is the difference this
    // pair exists to be able to see.
    let broken = lay(
        "body { font-size: 20px; line-height: 30px } .b { display: inline }",
        r#"<span class="b">one</span><span class="b">two</span>"#,
    );
    same("display: block", native, told, broken);
}

// ---- equivalences the box model states --------------------------------------

/// **Two collapsed margins are one margin of the larger size** (`css2` §8.3.1).
///
/// The clause as a reftest: a pair of blocks whose adjoining margins are 30 and
/// 10 lays out exactly as a pair whose margins are 30 and 0. This is the same
/// rule `epub_analytic.rs` checks arithmetically, and it is here as well
/// deliberately — the analytic test says the gap is 30, and this says the two
/// *documents* are interchangeable, which is what an author relies on.
#[test]
fn a_collapsed_pair_of_margins_is_one_margin_of_the_larger_size() {
    let body = r#"<p class="a">one</p><p class="b">two</p>"#;
    let both = lay(
        "p { font-size: 20px; line-height: 30px } \
         .a { margin-bottom: 30px } .b { margin-top: 10px }",
        body,
    );
    let larger = lay(
        "p { font-size: 20px; line-height: 30px } .a { margin-bottom: 30px }",
        body,
    );
    // Added rather than collapsed is 40, which is the reading this pair exists
    // to exclude.
    let broken = lay(
        "p { font-size: 20px; line-height: 30px } .a { margin-bottom: 40px }",
        body,
    );
    same("collapsing", both, larger, broken);
}

/// **A block's content edge is its padding plus its border**, whichever side
/// they are stated on (`css2` §8.1).
///
/// `padding-left: 20px; border-left: 10px` and `padding-left: 30px` put the
/// text in the same place, because layout sees one content edge. The pair says
/// the two contribute equally, and the mismatch says the comparison can see a
/// missing ten points.
#[test]
fn padding_and_border_reach_the_content_edge_the_same_way() {
    let body = "<p>one</p>";
    let split = lay(
        "p { font-size: 20px; padding-left: 20px; border-left: 10px solid #000 }",
        body,
    );
    let whole = lay("p { font-size: 20px; padding-left: 30px }", body);
    let broken = lay("p { font-size: 20px; padding-left: 20px }", body);
    same("the content edge", split, whole, broken);
}

/// **A shorter measure and a wider block reach the same column** (`css2`
/// §10.3.3).
///
/// The one pair here whose two sides differ in the *page* rather than in the
/// document: a 240-point page holding a block inset by 40 on each side is the
/// same column as a 160-point page holding one that is not. It is the property
/// a reading system relies on when it changes the window, and no other test in
/// this file varies the page box.
#[test]
fn an_inset_block_on_a_wide_page_is_a_full_block_on_a_narrow_one() {
    let style = "body { font-size: 20px; line-height: 30px }";
    let text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh";
    let inset = lay_at(
        &format!("{style} .i {{ margin: 0 40px }}"),
        &format!(r#"<div class="i"><p>{text}</p></div>"#),
        240.0,
    );
    let plain = lay_at(style, &format!("<p>{text}</p>"), 160.0);

    // The words and the vertical rhythm are what must agree; the inset column
    // starts forty points further right by construction, so the left edge is
    // compared as an offset rather than as an equality. This is the one pair
    // that has to say why it drops a coordinate.
    let strip = |lines: &[(String, f64, f64)]| -> Vec<(String, f64)> {
        lines
            .iter()
            .map(|(text, _, y)| (text.clone(), *y))
            .collect()
    };
    assert!(inset.len() > 1, "the fixture breaks into lines: {inset:?}");
    assert_eq!(
        strip(&inset),
        strip(&plain),
        "the two columns are one column"
    );
    for (left, right) in inset.iter().zip(&plain) {
        assert!(
            (left.1 - right.1 - 40.0).abs() < 1e-9,
            "the inset column is forty points across: {left:?} against {right:?}"
        );
    }

    // And a page that is not the same measure is not the same column, which is
    // what says the comparison is measuring the measure. A hundred points holds
    // eight advances against the reference's thirteen — chosen to cross a word
    // boundary, since 120 and 160 both break after the second word and would
    // have agreed.
    let narrower = lay_at(style, &format!("<p>{text}</p>"), 100.0);
    assert_ne!(strip(&plain), strip(&narrower));
}

// ---- text-transform ---------------------------------------------------------------

/// **`text-transform: uppercase` is the text written in capitals**
/// (`css-text-3` §2.1), with Unicode's *full* mapping, measured as what it
/// becomes.
///
/// Ten `ß` are ten characters in the source and twenty once uppercased, and at
/// a twenty-character measure that is the difference between one line and
/// two: the pair agrees only if the transform runs **before** line breaking,
/// which is §1.3's order, and only if `ß` is `SS` rather than the one-to-one
/// `ẞ` a simple mapping would give.
#[test]
fn uppercase_is_the_text_written_in_capitals_and_is_measured_so() {
    let style = "p { font-size: 20px; line-height: 30px }";
    let source =
        "<p class=\"u\">\u{df}\u{df}\u{df}\u{df}\u{df}\u{df}\u{df}\u{df}\u{df}\u{df} end</p>";
    let transformed = lay(
        &format!("{style} .u {{ text-transform: uppercase }}"),
        source,
    );
    let written = lay(style, "<p class=\"u\">SSSSSSSSSSSSSSSSSSSS END</p>");
    let broken = lay(&format!("{style} .u {{ text-transform: none }}"), source);
    assert_eq!(
        transformed.len(),
        2,
        "twenty capitals fill the line: {transformed:?}"
    );
    same("uppercase", transformed, written, broken);
}

/// **`lowercase` is the text written small, Final_Sigma included**, and
/// **`capitalize` titlecases the first letter of each word across element
/// boundaries** — a word's second half in its own `<span>` is still the middle
/// of the word.
#[test]
fn lowercase_and_capitalize_are_the_text_written_that_way() {
    let style = "p { font-size: 20px; line-height: 30px }";
    let lowered = lay(
        &format!("{style} .t {{ text-transform: lowercase }}"),
        "<p class=\"t\">\u{39f}\u{394}\u{39f}\u{3a3} \u{3a3}\u{391}\u{3a3}</p>",
    );
    let written_low = lay(
        style,
        "<p>\u{3bf}\u{3b4}\u{3bf}\u{3c2} \u{3c3}\u{3b1}\u{3c2}</p>",
    );
    let broken_low = lay(
        &format!("{style} .t {{ text-transform: uppercase }}"),
        "<p class=\"t\">\u{39f}\u{394}\u{39f}\u{3a3} \u{3a3}\u{391}\u{3a3}</p>",
    );
    same("lowercase", lowered, written_low, broken_low);

    let capitalized = lay(
        &format!("{style} .t {{ text-transform: capitalize }}"),
        "<p class=\"t\">the s<span>ea</span>, <span>the</span> sea</p>",
    );
    let written_cap = lay(style, "<p>The S<span>ea</span>, <span>The</span> Sea</p>");
    let broken_cap = lay(
        &format!("{style} .t {{ text-transform: none }}"),
        "<p class=\"t\">the s<span>ea</span>, <span>the</span> sea</p>",
    );
    same("capitalize", capitalized, written_cap, broken_cap);
}

// ---- lists and counters -----------------------------------------------------------

/// **An `inside` marker is the list item's first inline box** (`css-lists-3`
/// §3.2), and so lays out exactly as a `::before` holding the same text: the
/// `list-item` counter in the item's style, its `.` suffix and a space.
///
/// The two sides reach the page by different routes — one is the layout
/// crate's marker, armed on the item and taken by its first line; the other is
/// generated content resolved by the cascade's counter walk — and agree only if
/// both number the item the same way and put the text in the same line.
#[test]
fn an_inside_marker_is_a_before_box_holding_the_counter() {
    let base = "li { font-size: 20px; line-height: 30px }";
    let body = "<ol><li>aaaa bbbb cccc dddd</li><li>eeee</li></ol>";
    let marker = lay(
        &format!("{base} li {{ list-style-position: inside }}"),
        body,
    );
    let generated = lay(
        &format!(
            "{base} li {{ list-style-type: none }} \
             li::before {{ content: counter(list-item) \". \" }}"
        ),
        body,
    );
    let broken = lay(
        &format!("{base} li {{ list-style-position: outside }}"),
        body,
    );
    // The marker is a run of its own, at the content edge, and the text after
    // it is set three advances on: the lines wrap under the marker, which is
    // what `inside` means.
    assert_eq!(
        (marker[0].0.as_str(), marker[0].1),
        ("1. ", 40.0),
        "{marker:?}"
    );
    assert_eq!((marker[1].0.as_str(), marker[1].1), ("aaaa bbbb", 76.0));
    assert_eq!(marker[2].1, 40.0, "the second line wraps under the marker");
    same("an inside marker", marker, generated, broken);
}

/// **`list-style` is its two longhands** (`css-lists-3` §3.4), in either
/// order, `none` read as the type.
#[test]
fn the_list_style_shorthand_is_its_longhands() {
    let base = "li { font-size: 20px; line-height: 30px }";
    let body = "<ul><li>one</li><li>two</li></ul>";
    let shorthand = lay(&format!("{base} ul {{ list-style: inside square }}"), body);
    let longhand = lay(
        &format!("{base} ul {{ list-style-type: square; list-style-position: inside }}"),
        body,
    );
    let broken = lay(
        &format!("{base} ul {{ list-style-type: square; list-style-position: outside }}"),
        body,
    );
    same("list-style", shorthand, longhand, broken);
    let none = lay(&format!("{base} ul {{ list-style: none }}"), body);
    let none_type = lay(&format!("{base} ul {{ list-style-type: none }}"), body);
    assert_eq!(none, none_type, "`list-style: none` is the type");
}

/// **`<ol start>` and `<li value>` are the counter properties HTML §15.3.8
/// says they are**: `start="3"` is `counter-reset: list-item 2` and
/// `value="7"` is `counter-set: list-item 7`, and the items after a `value`
/// count on from it.
#[test]
fn ol_start_and_li_value_are_counter_reset_and_counter_set() {
    let base = "li { font-size: 20px; line-height: 30px; list-style-position: inside }";
    let items = "<li>a</li><li>b</li><li>c</li>";
    let attribute = lay(base, &format!("<ol start=\"3\">{items}</ol>"));
    let property = lay(
        &format!("{base} ol {{ counter-reset: list-item 2 }}"),
        &format!("<ol>{items}</ol>"),
    );
    let broken = lay(base, &format!("<ol start=\"1\">{items}</ol>"));
    assert_eq!(attribute[0].0, "3. ", "{attribute:?}");
    same("ol start", attribute, property, broken);

    let valued = lay(base, "<ol><li>a</li><li value=\"7\">b</li><li>c</li></ol>");
    let set = lay(
        &format!("{base} .v {{ counter-set: list-item 7 }}"),
        "<ol><li>a</li><li class=\"v\">b</li><li>c</li></ol>",
    );
    let broken = lay(base, "<ol><li>a</li><li>b</li><li>c</li></ol>");
    assert_eq!(
        valued.iter().map(|l| l.0.as_str()).collect::<Vec<_>>(),
        ["1. ", "a", "7. ", "b", "8. ", "c"]
    );
    same("li value", valued, set, broken);
}

/// **A nested list's numbers are `counters(list-item, ".")`** — each `<ol>`
/// resets its own instance (HTML §15.3.8's `ol { counter-reset: list-item }`)
/// and the inner one nests inside the outer, §4.5 — and **a sibling `<ol>`
/// starts again from one** rather than nesting inside the list before it.
#[test]
fn nested_lists_number_through_the_counter_tree() {
    let base = "li { font-size: 20px; line-height: 30px; list-style-type: none } \
                ol { padding: 0 }";
    let body = "<ol><li>a<ol><li>b</li><li>c</li></ol></li><li>d</li></ol><ol><li>e</li></ol>";
    let counted = lay(
        &format!("{base} li::before {{ content: counters(list-item, \".\") \" \" }}"),
        body,
    );
    let written = lay(
        base,
        "<ol><li><span>1 </span>a<ol><li><span>1.1 </span>b</li>\
         <li><span>1.2 </span>c</li></ol></li><li><span>2 </span>d</li></ol>\
         <ol><li><span>1 </span>e</li></ol>",
    );
    let broken = lay(
        &format!("{base} li::before {{ content: counter(list-item) \" \" }}"),
        body,
    );
    same("counters()", counted, written, broken);
}

// ---- quotes ---------------------------------------------------------------------

/// **A `<q>` is its text between the marks `quotes` names, one pair per level
/// of nesting** (`css-content-3` §3.2 and §3.3, HTML §15.3.6's
/// `q::before { content: open-quote }`).
///
/// The marks are generated boxes, so the reference writes them as spans of
/// their own: a run per box on both sides. The inner `<q>` takes the second
/// pair and the outer one's close mark comes after the inner one's, which is
/// the depth counted across the whole document rather than per element.
#[test]
fn a_q_element_is_its_text_between_the_marks_quotes_names() {
    let style = "p { font-size: 20px; line-height: 30px } \
                 q { quotes: \"\u{201c}\" \"\u{201d}\" \"\u{2018}\" \"\u{2019}\" }";
    let quoted = lay(style, "<p><q>he said <q>no</q> twice</q></p>");
    let written = lay(
        style,
        "<p><span>\u{201c}</span>he said <span>\u{2018}</span>no<span>\u{2019}</span> \
         twice<span>\u{201d}</span></p>",
    );
    // One pair for every level is the reading a per-element depth would give:
    // the inner `<q>` would open with the outer mark.
    let broken = lay(
        "p { font-size: 20px; line-height: 30px } q { quotes: \"\u{201c}\" \"\u{201d}\" }",
        "<p><q>he said <q>no</q> twice</q></p>",
    );
    same("q", quoted, written, broken);
}

// ---- fragmentation ------------------------------------------------------------

/// Where every line landed **and on which page**, at a page box short enough
/// to fragment.
///
/// The pairs above lay out into one hundred-thousand-point column, which is
/// the right instrument for a cascade question and blind to this one: a forced
/// break and an ignored one put every line at the same `y` of one endless
/// column. So the page index is part of the compared value here.
fn paged(style: &str, body: &str, height: f64) -> Vec<(usize, String, f64, f64)> {
    let (_, _, laid) = lay_out(
        &document(body),
        &format!("{RESET} {style}"),
        MEASURE,
        height,
    );
    lines(&laid)
        .into_iter()
        .map(|line| (line.page, line.text, line.x, line.y))
        .collect()
}

/// A pair agrees on its pages, and its mismatch reference does not.
#[track_caller]
fn same_pages(
    what: &str,
    left: Vec<(usize, String, f64, f64)>,
    right: Vec<(usize, String, f64, f64)>,
    broken: Vec<(usize, String, f64, f64)>,
) {
    assert!(
        left.iter().any(|line| line.0 > 0),
        "{what}: the reference never reached a second page, so it cannot tell a break \
         from none: {left:?}"
    );
    assert_eq!(left, right, "{what}: the two spellings disagree");
    assert_ne!(
        left, broken,
        "{what}: the mismatch reference agrees too, so the pair proves nothing"
    );
}

/// **`break-before: page` is `page-break-before: always`** (`css-break-3`
/// §3.4's table, first row).
///
/// The page box holds the whole document, so the only thing that can put the
/// second paragraph on a page of its own is the declaration — which is what the
/// mismatch reference, `auto`, says the comparison can see.
#[test]
fn break_before_page_is_page_break_before_always() {
    let style = "p { font-size: 20px; line-height: 30px }";
    let body = r#"<p>one</p><p class="b">two</p><p>three</p>"#;
    let modern = paged(&format!("{style} .b {{ break-before: page }}"), body, 300.0);
    let legacy = paged(
        &format!("{style} .b {{ page-break-before: always }}"),
        body,
        300.0,
    );
    let broken = paged(&format!("{style} .b {{ break-before: auto }}"), body, 300.0);
    same_pages("break-before", modern, legacy, broken);
}

/// **`break-after: page` is `page-break-after: always`**, the same row of
/// §3.4's table on the other edge of the box.
#[test]
fn break_after_page_is_page_break_after_always() {
    let style = "p { font-size: 20px; line-height: 30px }";
    let body = r#"<p class="a">one</p><p>two</p>"#;
    let modern = paged(&format!("{style} .a {{ break-after: page }}"), body, 300.0);
    let legacy = paged(
        &format!("{style} .a {{ page-break-after: always }}"),
        body,
        300.0,
    );
    let broken = paged(&format!("{style} .a {{ break-after: avoid }}"), body, 300.0);
    same_pages("break-after", modern, legacy, broken);
}

/// **`break-inside: avoid` and `avoid-page` are `page-break-inside: avoid`**
/// (§3.4's last row, and §3.2's `avoid-page` in a document whose only
/// fragmentation context is the page).
///
/// The page holds three lines; one paragraph of one line comes first and one of
/// four lines follows, so an unavoided break leaves two of the four behind.
/// `orphans` and `widows` are set to one on all three sides so that §13.3.2 has
/// no say: the only thing that can move the paragraph whole is the property.
#[test]
fn break_inside_avoid_is_page_break_inside_avoid() {
    let style = "p { font-size: 20px; line-height: 30px; orphans: 1; widows: 1 }";
    // Twelve-point advances against a 240-point measure is twenty characters a
    // line, so each fifteen-letter word is a line of its own.
    let body = "<p>one</p><p class=\"k\">aaaaaaaaaaaaaaa bbbbbbbbbbbbbbb \
                ccccccccccccccc ddddddddddddddd</p>";
    let modern = paged(&format!("{style} .k {{ break-inside: avoid }}"), body, 90.0);
    let modern_page = paged(
        &format!("{style} .k {{ break-inside: avoid-page }}"),
        body,
        90.0,
    );
    let legacy = paged(
        &format!("{style} .k {{ page-break-inside: avoid }}"),
        body,
        90.0,
    );
    let broken = paged(&format!("{style} .k {{ break-inside: auto }}"), body, 90.0);
    assert_eq!(modern, modern_page, "avoid-page is avoid on paper");
    same_pages("break-inside", modern, legacy, broken);
}

// ---- the right-to-left pair -------------------------------------------------

/// The three Arabic letters and the space the pair below is written with.
const RTL_COVERS: &str = " \u{628}\u{62D}\u{645}";

/// Two Arabic words, drawn through a face that joins.
const RTL_LINE: &str = "\u{628}\u{62D}\u{645} \u{645}\u{62D}\u{628}";

/// What one book draws on its first page, as the glyph indices of each text
/// object in the order they are shown.
///
/// The pairs above compare **line boxes**, which is the right unit for a
/// cascade question and the wrong one here: `flow.rs` breaks lines over
/// logical text and reorders nothing, so two spellings of a right-to-left line
/// agree on their boxes whatever the glyphs inside them are doing. So this
/// pair compares what reaches the page.
fn drawn(body: &str) -> Vec<String> {
    let face = Face::new("Fixture Arabic", RTL_COVERS).with_joining(Joining { script: *b"arab" });
    let book = one_face_book("Fixture Arabic", &face.build(), 24, body);
    let doc = Document::open(book).expect("a book");
    let cos = doc.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    let content =
        String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned();
    text_objects(&content)
        .iter()
        .map(|(_, object)| shown_glyphs(object))
        .collect()
}

/// **An inline box that wraps a whole paragraph contributes no boxes of its
/// own** (`css-display-3` §2.1), and a right-to-left line does not care.
///
/// The EPUB tier's right-to-left pair. Both sides are the same Arabic line
/// through the same joining face, and the specification says they are one
/// document: a `<span>` around the entirety of a block's content adds an
/// inline box and no content, so the glyphs, their forms and their order must
/// be identical.
///
/// It is a real pair rather than a restatement, because the two sides reach
/// the glyphs by different routes — one text node against a text node inside
/// an inline box — and shaping happens per run of one face. A build that
/// started a fresh shaping run at an inline boundary would join the two sides
/// differently, and a build that reordered per box rather than per run would
/// draw one of them backwards.
///
/// The mismatch reference is the same line with its two words exchanged, which
/// must **not** match: without it this pair would pass on a build that drew
/// nothing at all.
#[test]
fn an_inline_box_around_a_whole_arabic_paragraph_changes_nothing() {
    let plain = drawn(RTL_LINE);
    let wrapped = drawn(&format!("<span>{RTL_LINE}</span>"));
    let swapped: String = {
        let mut words: Vec<&str> = RTL_LINE.split(' ').collect();
        words.reverse();
        words.join(" ")
    };
    let broken = drawn(&swapped);

    assert!(
        !plain.is_empty() && plain.iter().all(|shown| !shown.is_empty()),
        "the reference drew nothing: {plain:?}"
    );
    assert_eq!(
        plain, wrapped,
        "an inline box around the whole paragraph changed what was drawn"
    );
    assert_ne!(
        plain, broken,
        "the mismatch reference agrees too, so the pair proves nothing"
    );
}

// ---- css-overflow-3 ---------------------------------------------------------------

/// **A scroll container contains its floats as a clearing block inside it
/// would** (CSS 2.2 §10.6.7): `overflow: hidden` round a float and a clearing
/// `<div>` after it are two spellings of a box as tall as the float, and the
/// paragraph after either starts below it.
#[test]
fn a_scroll_container_contains_its_floats_as_a_clearing_block_does() {
    let float = ".f { float: left; width: 50px; height: 60px }";
    let contained = lay(
        &format!("{float} .c {{ overflow: hidden }}"),
        r#"<div class="c"><div class="f">fl</div>x</div><p>after</p>"#,
    );
    let cleared = lay(
        &format!("{float} .k {{ clear: both }}"),
        r#"<div class="c"><div class="f">fl</div>x<div class="k"></div></div><p>after</p>"#,
    );
    // The mismatch: neither, so the paragraph after it flows beside the float.
    let broken = lay(
        float,
        r#"<div class="c"><div class="f">fl</div>x</div><p>after</p>"#,
    );
    same("float containment", contained, cleared, broken);
}

/// **A scroll container's margin does not collapse with its first child's**
/// (§8.3.1): `overflow: auto` round a paragraph is the same box as a wrapper
/// whose padding holds the paragraph's margin — and `overflow-x: auto` alone is
/// `overflow: auto`, by `css-overflow-3` §3.1's computed value.
#[test]
fn a_scroll_containers_first_childs_margin_stays_inside_it() {
    let body = r#"<div class="c"><p>one</p><p>two</p></div>"#;
    let scrolling = lay(
        ".c { margin-top: 10px; overflow-x: auto } p { margin-top: 20px }",
        body,
    );
    let padded = lay(
        ".c { margin-top: 10px; padding-top: 20px } p { margin-top: 20px } \
         .c p:first-child { margin-top: 0 }",
        body,
    );
    // The mismatch: `clip`, which §3.1 says is no formatting context, so the
    // two margins collapse to the larger.
    let broken = lay(
        ".c { margin-top: 10px; overflow-x: clip } p { margin-top: 20px }",
        body,
    );
    same("overflow-x: auto", scrolling, padded, broken);
}

/// **`overflow` on `<body>` belongs to the page** (`css-overflow-3` §3.3), so
/// `<body>` keeps a used value of `visible` and collapses its margin with its
/// first child's exactly as it does with no `overflow` at all.
#[test]
fn overflow_on_body_is_the_pages_and_not_the_bodys() {
    let body = "<p>one</p><p>two</p>";
    let propagated = lay(
        "body { margin: 8px; overflow-x: hidden } p { margin-top: 30px }",
        body,
    );
    let plain = lay("body { margin: 8px } p { margin-top: 30px }", body);
    // The mismatch: the same declaration one element down, on a `<div>` that
    // does not propagate, which is a scroll container and keeps both margins.
    let broken = lay(
        "body { margin: 0 } div { margin: 8px; overflow-x: hidden } p { margin-top: 30px }",
        &format!("<div>{body}</div>"),
    );
    same("body overflow", propagated, plain, broken);
}

// ---- transform ----------------------------------------------------------------

/// **A transformed box is the containing block of its absolutely positioned
/// descendants**, as a relatively positioned one is (`css-transforms-1` §2):
/// `transform: translate(0)` moves no ink, so the two spellings lay out alike,
/// and the mismatch — the same box untransformed — places the descendant
/// against the page instead.
#[test]
fn a_transformed_box_contains_its_absolute_descendants_as_a_positioned_one_does() {
    let body = r#"<p>before</p><div class="t"><p>inside</p><p class="a">placed</p></div>"#;
    let common = "div.t { margin: 30px 0 0 40px } p.a { position: absolute; top: 5px; left: 7px }";
    let transformed = lay(
        &format!("{common} div.t {{ transform: translate(0) }}"),
        body,
    );
    let positioned = lay(&format!("{common} div.t {{ position: relative }}"), body);
    let broken = lay(common, body);
    same("transform", transformed, positioned, broken);
}

// ---- the layout refusals ---------------------------------------------------------

/// **Cells with no row are one anonymous row** (CSS 2.2 §17.2.1 rule 8:
/// *"for each `table-cell` box whose parent is not a `table-row`, generate an
/// anonymous `table-row` box around it and all consecutive siblings that are
/// `table-cell` boxes"*): cells written straight into a row group, into a
/// table, and into no table at all lay out as the row the markup omitted.
#[test]
fn cells_with_no_row_are_the_row_the_markup_omitted() {
    let style = "body { font-size: 20px; line-height: 30px } td, .c { padding: 0 } \
                 .c { display: table-cell } table.flush { border-spacing: 0 }";
    // The user-agent sheet's `border-spacing: 2px` is the `<table>` element's,
    // so the cells of a real table sit two pixels apart; an anonymous table
    // (rule 9's) inherits nothing it does not inherit and has none, so its
    // reference is a table told to have none.
    for (what, written, implied) in [
        (
            "cells in a row group",
            "<table><tbody><tr><td>aa</td><td>bb</td></tr></tbody></table>",
            "<table><tbody><td>aa</td><td>bb</td></tbody></table>",
        ),
        (
            "cells in a table",
            "<table><tbody><tr><td>aa</td><td>bb</td></tr></tbody></table>",
            "<table><td>aa</td><td>bb</td></table>",
        ),
        (
            "cells in no table",
            r#"<table class="flush"><tr><td>aa</td><td>bb</td></tr></table>"#,
            r#"<div><div class="c">aa</div><div class="c">bb</div></div>"#,
        ),
    ] {
        // The mismatch: the same two cells in two rows, which is a different
        // table — what an anonymous row per cell would draw.
        let broken = lay(
            style,
            "<table><tr><td>aa</td></tr><tr><td>bb</td></tr></table>",
        );
        same(what, lay(style, written), lay(style, implied), broken);
    }
}

/// **An `@supports` block applies where this build supports its test**
/// (`css-conditional-3` §6), and is no longer skipped by name: a book's rule
/// inside `@supports (display: flex)` is the same rule written plainly, and
/// one inside `@supports (display: grid)` — a property this build does not
/// implement — is not applied, which is the mismatch.
#[test]
fn a_supports_block_is_its_rules_where_the_test_is_supported() {
    use tinker_pdf_css::media::MediaContext;
    use tinker_pdf_css::parser::parse;
    use tinker_pdf_css::{Budget, Limits, NoImports, Warning};
    let body = r#"<p>one</p><p class="a">two</p>"#;
    let conditional =
        "@supports (display: flex) and (not (display: grid)) { p.a { margin-left: 30px } }";
    let supported = lay(conditional, body);
    let plain = lay("p.a { margin-left: 30px }", body);
    let broken = lay(
        "@supports (display: grid) { p.a { margin-left: 30px } }",
        body,
    );
    same("@supports", supported, plain, broken);
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    let sheet = parse(
        conditional.as_bytes(),
        None,
        &NoImports,
        &MediaContext::screen(MEASURE, 1000.0),
        &limits,
        &mut budget,
    )
    .expect("a sheet");
    assert!(
        !sheet
            .report
            .warnings
            .iter()
            .any(|(warning, _)| matches!(warning, Warning::AtRuleUnsupported(_))),
        "{:?}",
        sheet.report.warnings
    );
}

/// **`:nth-child(An+B of S)` is the position among the siblings that match
/// `S`** (`selectors-4` §14.4.1), and is no longer a rule dropped whole: the
/// second `.s` of four paragraphs is the third paragraph, which the same rule
/// written as a class on that paragraph moves alike; read as plain
/// `:nth-child(2)` — the mismatch — it moves the second instead.
#[test]
fn nth_child_of_a_selector_is_the_position_among_its_matches() {
    let body = r#"<p class="s">aa</p><p>bb</p><p class="s t">cc</p><p class="s">dd</p>"#;
    let of = lay("p:nth-child(2 of .s) { margin-left: 30px }", body);
    let marked = lay("p.t { margin-left: 30px }", body);
    let broken = lay("p:nth-child(2) { margin-left: 30px }", body);
    same(":nth-child(of S)", of, marked, broken);
}

/// **`inline-flex` is an atomic inline whose inside is a flex layout**
/// (`css-flexbox-1` §3): in a line of text its two items sit where two
/// inline-blocks inside an inline-block sit, and the words either side stay on
/// the line. The mismatch is a block-level flex container — what this build
/// drew before, warning `InlineFlexAsBlock` — which breaks the line round it.
#[test]
fn inline_flex_is_an_atomic_inline_holding_a_flex_layout() {
    let style = ".f { display: inline-flex } .b, .b span { display: inline-block } \
                 .x { display: flex }";
    let flex = lay(
        style,
        r#"<p>aa <span class="f"><span>bb</span><span>cc</span></span> dd</p>"#,
    );
    let blocks = lay(
        style,
        r#"<p>aa <span class="b"><span>bb</span><span>cc</span></span> dd</p>"#,
    );
    let broken = lay(
        style,
        r#"<p>aa <span class="x"><span>bb</span><span>cc</span></span> dd</p>"#,
    );
    same("inline-flex", flex, blocks, broken);
}

/// A one-chapter book set by `style`, rendered: the page's pixels, for the
/// pairs whose two spellings agree on paint rather than on lines.
fn rendered(style: &str, body: &str) -> Vec<u8> {
    use epub_support::{ocf_zip, OcfEntry};
    const CONTAINER: &str = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
        r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
        r#"</rootfiles></container>"#
    );
    const PACKAGE: &str = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">"#,
        r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
        r#"<dc:identifier id="id">urn:uuid:6a6a6a6a-0000-4000-8000-000000000002</dc:identifier>"#,
        r#"<dc:title>Pair</dc:title><dc:language>en</dc:language></metadata><manifest>"#,
        r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
        r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
    );
    let chapter = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>A Chapter</title>"#,
            r#"<style>{} {}</style></head><body>{}</body></html>"#
        ),
        RESET, style, body
    );
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", PACKAGE.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    let doc = Document::open(ocf_zip(&entries, &directory)).expect("the book opens");
    doc.page(0)
        .expect("a page")
        .render(&tinker_pdf::RenderOptions::default())
        .data
}

/// **A column's background is painted under its cells** (CSS 2.2 §17.5.1's
/// third layer, *"covers exactly the full area of all cells that originate in
/// the column"*), and a row group's is above it: a `<col>` with a red
/// background renders as the same table with each of that column's cells
/// red, and under a blue `<tbody>` as the same cells blue. The mismatch is
/// the table with no column background, which is what this build drew before,
/// warning `ColumnBoxNotPainted`.
#[test]
fn a_column_background_is_its_cells_backgrounds_under_the_row_group() {
    let style = "table { border-spacing: 0 } td { padding: 4px } .r { background-color: #ff0000 } \
                 .b { background-color: #0000ff }";
    let table = |col: &str, group: &str, first: &str| {
        format!(
            r#"<table><col{col}/><col/><tbody{group}><tr><td{first}>aa</td><td>bb</td></tr><tr><td{first}>cc</td><td>dd</td></tr></tbody></table>"#
        )
    };
    let red = r#" class="r""#;
    let blue = r#" class="b""#;
    let column = rendered(style, &table(red, "", ""));
    let cells = rendered(style, &table("", "", red));
    let broken = rendered(style, &table("", "", ""));
    assert!(column == cells, "a column's background is its cells'");
    assert!(column != broken, "and the pair could have disagreed");
    // The row group's layer is above the column's.
    let under = rendered(style, &table(red, blue, ""));
    let group = rendered(style, &table("", blue, ""));
    assert!(under == group, "the row group covers the column");
    // And nothing is named any more.
    let laid = epub_support::layout::lay_out(
        &document(&table(red, "", "")),
        &format!("{RESET} {style}"),
        MEASURE,
        1000.0,
    )
    .2;
    assert!(laid.warnings.is_empty(), "{:?}", laid.warnings);
}

/// **A block inside an inline splits the inline** (CSS 2.2 §9.2.1.1): the
/// inline content before the block and after it are anonymous block boxes of
/// their own, and the block is a block between them — the same lines as the
/// three written as three paragraphs. The mismatch is the block's text poured
/// into the line, which is what this build did before, warning
/// `BlockInInline`.
#[test]
fn a_block_inside_an_inline_splits_it_into_anonymous_blocks() {
    let style = ".k { display: block }";
    let split = lay(
        style,
        r#"<p>aa <span>bb<span class="k">cc</span>dd</span> ee</p>"#,
    );
    let written = lay(
        style,
        r#"<p>aa <span>bb</span></p><p><span><span class="k">cc</span></span></p><p><span>dd</span> ee</p>"#,
    );
    let broken = lay(style, r#"<p>aa <span>bb<span>cc</span>dd</span> ee</p>"#);
    same("block-in-inline", split, written, broken);
}

/// **`::first-letter` is the first typographic letter unit in a box of its
/// own** (`css-pseudo-4` §2.2), and no longer a selector parsed with no box:
/// a 30-pixel first letter is the paragraph with its first letter wrapped in a
/// 30-pixel `<span>`; the quotation mark before it and the punctuation after
/// it go into the box with it; a floated one is a drop cap; and a container's
/// first letter is found inside its first child block. Each mismatch is the
/// paragraph without the rule.
#[test]
fn first_letter_is_the_first_letter_in_a_box_of_its_own() {
    let big = ".big { font-size: 30px } .cap { float: left; font-size: 40px }";
    for (what, rule, body, written) in [
        (
            "a first letter",
            "p::first-letter { font-size: 30px }",
            "<p>Hello there</p>",
            r#"<p><span class="big">H</span>ello there</p>"#,
        ),
        (
            "punctuation either side",
            "p::first-letter { font-size: 30px }",
            "<p>\u{201c}A.\u{201d} said he</p>",
            "<p><span class=\"big\">\u{201c}A.\u{201d}</span> said he</p>",
        ),
        (
            "a drop cap",
            "p::first-letter { float: left; font-size: 40px }",
            "<p>Once upon a time there was a story long enough to wrap</p>",
            r#"<p><span class="cap">O</span>nce upon a time there was a story long enough to wrap</p>"#,
        ),
        (
            "inside the first child block",
            "div::first-letter { font-size: 30px }",
            "<div><p> <em>Hello</em> there</p><p>Next</p></div>",
            r#"<div><p> <em><span class="big">H</span>ello</em> there</p><p>Next</p></div>"#,
        ),
    ] {
        let styled = lay(&format!("{big} {rule}"), body);
        let wrapped = lay(big, written);
        let broken = lay(big, body);
        same(what, styled, wrapped, broken);
    }
}

/// **Nested block containers' `::first-letter` boxes do not stack**: where a
/// container and the block inside it both ask for one, the letter is in the
/// inner one's box (CSS 2.1 §5.12.2's fictional tag sequence puts the inner
/// pseudo-element innermost, so its declarations are the ones the letter
/// shows) and the outer one makes no second box round the same letter. The
/// mismatch is the outer one's size winning, which is what wrapping the same
/// letter once per level drew.
#[test]
fn a_nested_first_letter_is_the_nearest_blocks() {
    let sizes = ".big { font-size: 30px } .small { font-size: 20px }";
    let rules = "div::first-letter { font-size: 30px } p::first-letter { font-size: 20px }";
    let styled = lay(
        &format!("{sizes} {rules}"),
        "<div><p>Hello there</p><p>Next</p></div>",
    );
    let written = lay(
        sizes,
        r#"<div><p><span class="small">H</span>ello there</p><p><span class="small">N</span>ext</p></div>"#,
    );
    let broken = lay(
        sizes,
        r#"<div><p><span class="big">H</span>ello there</p><p><span class="small">N</span>ext</p></div>"#,
    );
    same("nested ::first-letter", styled, written, broken);
}

/// **A `::first-letter` on every one of two hundred nested blocks is one box**
/// round the letter, not two hundred, and building it fits a test thread's
/// two-megabyte stack. Each level used to search down to the letter again and
/// wrap it again, which made the box tree twice as deep as the document — past
/// `MAX_BOX_DEPTH` from 129 levels — and a search two frames a level deep
/// whose frames each held a computed style, which overflowed the stack below a
/// hundred levels. The mismatch is the nest without the rule.
#[test]
fn a_first_letter_on_two_hundred_nested_blocks_is_one_box() {
    const LEVELS: usize = 200;
    let big = ".big { font-size: 30px }";
    let open = "<div>".repeat(LEVELS);
    let close = "</div>".repeat(LEVELS);
    let body = format!("{open}Hello there{close}");
    let styled = lay(
        &format!("{big} div::first-letter {{ font-size: 30px }}"),
        &body,
    );
    let written = lay(
        big,
        &format!(r#"{open}<span class="big">H</span>ello there{close}"#),
    );
    let broken = lay(big, &body);
    same("two hundred nested ::first-letter", styled, written, broken);
}

/// **`column-span: all` interrupts the columns** (`css-multicol-1` §6): a
/// two-column container with a spanning paragraph in the middle lays out as a
/// two-column container, the paragraph, and a second two-column container —
/// the three written out. The mismatch is the paragraph laid out in its
/// column, which is what this build drew before, warning `ColumnSpanAsNone`.
#[test]
fn a_spanning_child_is_the_column_sets_either_side_of_a_block() {
    let style = ".mc { column-count: 2; column-gap: 0 } .s { column-span: all }";
    let spanning = lay(
        style,
        r#"<div class="mc"><p>aa</p><p>bb</p><p class="s">span</p><p>cc</p><p>dd</p></div>"#,
    );
    let written = lay(
        style,
        r#"<div class="mc"><p>aa</p><p>bb</p></div><p>span</p><div class="mc"><p>cc</p><p>dd</p></div>"#,
    );
    let broken = lay(
        style,
        r#"<div class="mc"><p>aa</p><p>bb</p><p>span</p><p>cc</p><p>dd</p></div>"#,
    );
    same("column-span: all", spanning, written, broken);
}
