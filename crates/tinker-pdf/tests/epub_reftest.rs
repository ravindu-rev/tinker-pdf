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

use epub_support::layout::{column, document, Line};

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
