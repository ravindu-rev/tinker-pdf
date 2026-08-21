//! The crate's own tests, by the specification each is about.
//!
//! # The metrics are one point per point on purpose
//!
//! Every number below is arithmetic a reader can check: a character of a
//! ten-point font is ten points wide, so a hundred-point line holds ten of
//! them, and a line box is twelve points tall because `line-height: normal` is
//! 1.2. A test whose expected number came out of running the code is a
//! regression test and not a test of the specification, and this file is meant
//! to be the second kind.

use tinker_pdf_css::cascade::ComputedStyle;
use tinker_pdf_css::property::{
    AlignContent, AlignItems, AlignSelf, BorderStyle, BoxSizing, Clear, Color, Display,
    FlexDirection, FlexWrap, Float, JustifyContent, LengthPercentage, LineHeight, ListStyleType,
    MarginValue, OverflowWrap, PageBreak, PageBreakInside, Side, Sides, Size, TextAlign,
    Visibility, WhiteSpace,
};

use crate::flex;
use crate::flow::marker_text;
use crate::metrics::FixedPitch;
use crate::table;
use crate::{
    layout, layout_with, BoxNode, Budget, Content, Layout, Limits, Options, Refusal, Warning,
};

/// One point of advance per point of font size.
const METRICS: FixedPitch = FixedPitch {
    advance: 1.0,
    ascent: 0.8,
    descent: 0.2,
};

fn base() -> ComputedStyle {
    let mut style = ComputedStyle::initial();
    style.font_size = 10.0;
    style
}

fn block() -> ComputedStyle {
    let mut style = base();
    style.display = Display::Block;
    style
}

fn px(value: f64) -> MarginValue {
    MarginValue::Length(LengthPercentage::Px(value))
}

fn text(body: &str) -> BoxNode {
    BoxNode::text(base(), body)
}

fn para(body: &str) -> BoxNode {
    BoxNode::element(block(), vec![text(body)])
}

fn run(tree: &BoxNode, width: f64, height: f64) -> Layout {
    layout(
        tree,
        &METRICS,
        &Options::new(width, height),
        &Limits::DEFAULT,
    )
    .expect("the fixture is under every cap")
}

/// The baselines on one page, which is where a block ended up.
fn baselines(laid: &Layout, page: usize) -> Vec<f64> {
    laid.pages[page].runs.iter().map(|run| run.y).collect()
}

fn page_text(laid: &Layout, page: usize) -> String {
    laid.pages[page]
        .runs
        .iter()
        .filter(|run| !run.generated)
        .map(|run| run.text.as_str())
        .collect()
}

/// The stream text conservation is about: every non-whitespace character, in
/// order.
fn conservable(body: &str) -> String {
    body.chars().filter(|c| !c.is_whitespace()).collect()
}

// ---- css-box-3, the box model ----------------------------------------------

/// An `auto` width fills the containing block, less this box's own margins,
/// borders and padding.
#[test]
fn an_auto_width_block_fills_what_is_left_of_its_containing_block() {
    let mut style = block();
    style.margin.left = px(10.0);
    style.margin.right = px(20.0);
    style.background_color = Color {
        r: 1,
        g: 2,
        b: 3,
        a: 255,
    };
    let tree = BoxNode::element(block(), vec![BoxNode::element(style, vec![text("x")])]);
    let laid = run(&tree, 200.0, 400.0);
    let fragment = &laid.pages[0].boxes[0];
    assert_eq!(fragment.x, 10.0);
    assert_eq!(fragment.width, 170.0);
}

/// `box-sizing` decides whether `width` is the content or the border box, and
/// the difference is only visible on a box that **has** a border and a padding.
///
/// A fixture with neither gets the same answer from both values, which is why
/// this one has both — the same shape as gap 30's ordering fixture and gap 31
/// milestone 3's two containers.
#[test]
fn box_sizing_is_the_difference_between_a_hundred_and_a_hundred_and_thirty() {
    let make = |sizing: BoxSizing| {
        let mut style = block();
        style.width = Size::Length(LengthPercentage::Px(100.0));
        style.box_sizing = sizing;
        style.padding = Sides::all(LengthPercentage::Px(10.0));
        style.border_width = Sides::all(5.0);
        style.border_style = Sides::all(BorderStyle::Solid);
        let tree = BoxNode::element(block(), vec![BoxNode::element(style, vec![text("x")])]);
        let laid = run(&tree, 400.0, 400.0);
        laid.pages[0].boxes[0].width
    };
    assert_eq!(make(BoxSizing::BorderBox), 100.0);
    assert_eq!(make(BoxSizing::ContentBox), 130.0);
}

/// §10.3.3: two `auto` margins centre a box of specified width.
#[test]
fn two_auto_margins_centre_a_block() {
    let mut style = block();
    style.width = Size::Length(LengthPercentage::Px(100.0));
    style.margin.left = MarginValue::Auto;
    style.margin.right = MarginValue::Auto;
    style.background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    let tree = BoxNode::element(block(), vec![BoxNode::element(style, vec![text("x")])]);
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(laid.pages[0].boxes[0].x, 100.0);
}

/// §8.3: a percentage margin is a percentage of the containing block's
/// **width**, on all four sides — including the top, which is the one that
/// surprises.
#[test]
fn a_percentage_margin_is_of_the_width_even_at_the_top() {
    let mut style = block();
    style.margin.top = MarginValue::Length(LengthPercentage::Percent(10.0));
    let tree = BoxNode::element(block(), vec![BoxNode::element(style, vec![text("x")])]);
    let laid = run(&tree, 200.0, 400.0);
    // Ten per cent of two hundred is twenty, plus the nine-point ascent-and-
    // leading of the line box. Ten per cent of the *height* would be forty.
    assert_eq!(baselines(&laid, 0), vec![29.0]);
}

/// A border whose style is `none` has a used width of zero, whatever
/// `border-width` says (§8.5.3).
#[test]
fn a_border_width_with_no_border_style_moves_nothing() {
    let mut style = block();
    style.border_width = Sides::all(20.0);
    style.border_style = Sides::all(BorderStyle::None);
    let tree = BoxNode::element(block(), vec![BoxNode::element(style, vec![text("x")])]);
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(baselines(&laid, 0), vec![9.0]);
}

// ---- CSS 2.2 section 8.3.1, collapsing margins ------------------------------
//
// Three cases, three tests, and they are separate on purpose. A single fixture
// with a parent, a first child and an empty sibling exercises all three and
// passes if any two of them are right.

/// Case 1: a box's bottom margin and the next box's top margin collapse.
#[test]
fn margins_collapse_between_adjacent_siblings() {
    let mut first = block();
    first.margin.bottom = px(20.0);
    let mut second = block();
    second.margin.top = px(30.0);
    let tree = BoxNode::element(
        block(),
        vec![
            BoxNode::element(first, vec![text("a")]),
            BoxNode::element(second, vec![text("b")]),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    // 12 for the first line box, then max(20, 30) rather than 20 + 30.
    assert_eq!(baselines(&laid, 0), vec![9.0, 51.0]);
}

/// Case 2: a parent's top margin and its first child's collapse, when nothing
/// separates them.
#[test]
fn margins_collapse_between_a_parent_and_its_first_child() {
    let mut parent = block();
    parent.margin.top = px(20.0);
    let mut child = block();
    child.margin.top = px(30.0);
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(
            parent,
            vec![BoxNode::element(child, vec![text("a")])],
        )],
    );
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(baselines(&laid, 0), vec![39.0]);
}

/// Case 3: a box with nothing in it collapses **through** itself, so its own
/// two margins and the two around it are all one margin.
#[test]
fn an_empty_boxs_margins_collapse_through_it() {
    let mut empty = block();
    empty.margin.top = px(20.0);
    empty.margin.bottom = px(30.0);
    let tree = BoxNode::element(
        block(),
        vec![para("a"), BoxNode::element(empty, Vec::new()), para("b")],
    );
    let laid = run(&tree, 200.0, 400.0);
    // 12 for the first line, then max(20, 30) — not 20 + 30, and not 20 or 30
    // taken alone with the other lost.
    assert_eq!(baselines(&laid, 0), vec![9.0, 51.0]);
}

/// A border between a parent and its first child stops case 2, which is the
/// clause that says *when* they collapse rather than *that* they do.
#[test]
fn a_border_stops_a_parent_collapsing_with_its_first_child() {
    let mut parent = block();
    parent.margin.top = px(20.0);
    parent.border_width = Sides::all(5.0);
    parent.border_style = Sides::all(BorderStyle::Solid);
    let mut child = block();
    child.margin.top = px(30.0);
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(
            parent,
            vec![BoxNode::element(child, vec![text("a")])],
        )],
    );
    let laid = run(&tree, 200.0, 400.0);
    // 20 of parent margin, 5 of border, 30 of child margin, 9 of ascent.
    assert_eq!(baselines(&laid, 0), vec![64.0]);
}

/// §8.3.1's arithmetic: the **maximum of the positive** margins plus the
/// **minimum of the negative** ones, which is not the maximum of the signed
/// values.
#[test]
fn a_negative_margin_is_added_rather_than_beaten() {
    let mut first = block();
    first.margin.bottom = px(-10.0);
    let mut second = block();
    second.margin.top = px(30.0);
    let tree = BoxNode::element(
        block(),
        vec![
            BoxNode::element(first, vec![text("a")]),
            BoxNode::element(second, vec![text("b")]),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    // 12 + (30 - 10) + 9. A build that took max() over the signed values would
    // put it at 12 + 30 + 9 = 51.
    assert_eq!(baselines(&laid, 0), vec![9.0, 41.0]);
}

// ---- CSS 2.2 section 9.4.1 and 9.4.2 ---------------------------------------

/// §9.2.1.1: block-level and inline-level siblings, so the inline runs are
/// wrapped in anonymous block boxes.
#[test]
fn inline_siblings_of_a_block_get_an_anonymous_block_each() {
    let tree = BoxNode::element(block(), vec![text("before"), para("middle"), text("after")]);
    let laid = run(&tree, 200.0, 400.0);
    // Three line boxes in document order, not one line with the block's text
    // spliced into it.
    assert_eq!(baselines(&laid, 0), vec![9.0, 21.0, 33.0]);
    assert_eq!(page_text(&laid, 0), "beforemiddleafter");
}

/// §10.8.1's strut: a line box is at least as tall as its block container's
/// own font and `line-height`, and as tall as the tallest inline on it.
#[test]
fn a_line_box_takes_the_taller_of_the_strut_and_its_content() {
    let mut big = base();
    big.font_size = 20.0;
    let tree = BoxNode::element(block(), vec![BoxNode::text(big, "x")]);
    let laid = run(&tree, 400.0, 400.0);
    // 20-point text: line-height 24, ascent 16, half-leading 2, so the
    // baseline is 18 rather than the strut's 9.
    assert_eq!(baselines(&laid, 0), vec![18.0]);
}

/// A `line-height` in points is used as written; a number is re-multiplied by
/// each element's own font size.
#[test]
fn a_line_height_number_and_a_length_are_different_things() {
    let mut number = block();
    number.line_height = LineHeight::Number(2.0);
    let laid = run(&BoxNode::element(number, vec![text("x")]), 400.0, 400.0);
    // 20 tall, ascent 8, half-leading 5, baseline 13.
    assert_eq!(baselines(&laid, 0), vec![13.0]);

    let mut length = block();
    length.line_height = LineHeight::Px(30.0);
    let laid = run(&BoxNode::element(length, vec![text("x")]), 400.0, 400.0);
    assert_eq!(baselines(&laid, 0), vec![18.0]);
}

/// `visibility: hidden` is laid out and not painted; `display: none` is
/// neither.
#[test]
fn hidden_is_laid_out_and_none_is_not() {
    let mut hidden = block();
    hidden.visibility = Visibility::Hidden;
    // `visibility` inherits, and the cascade is what does the inheriting: a
    // fixture that builds computed styles by hand has to do it itself, which
    // is why the text node carries the value as well as the box.
    let mut hidden_text = base();
    hidden_text.visibility = Visibility::Hidden;
    let tree = BoxNode::element(
        block(),
        vec![
            BoxNode::element(hidden, vec![BoxNode::text(hidden_text, "a")]),
            para("b"),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(baselines(&laid, 0), vec![9.0, 21.0]);
    assert!(!laid.pages[0].runs[0].painted);
    assert!(laid.pages[0].runs[1].painted);

    let mut gone = block();
    gone.display = Display::None;
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(gone, vec![text("a")]), para("b")],
    );
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(baselines(&laid, 0), vec![9.0]);
    assert_eq!(page_text(&laid, 0), "b");
}

// ---- css-text-3 section 4.1, white-space processing -------------------------

/// Phase I, §4.1.1, against a source indented the way milestone 1's real books
/// are.
#[test]
fn phase_one_collapses_an_indented_source() {
    // Exactly the shape pandoc and calibre write: a newline and eight spaces
    // between elements, and two spaces after a full stop.
    let source = "\n        The sea.  The sea,\n        and the ships.\n    ";
    let laid = run(&para(source), 4_000.0, 400.0);
    assert_eq!(page_text(&laid, 0), "The sea. The sea, and the ships.");
}

/// Phase I again, across an element boundary — §4.1.1's *"even one outside the
/// boundary of the inline containing that space"*.
///
/// This is the failure that is **not** visible: two spaces between two
/// emphasised words look like one space to anybody reading the page, and the
/// line breaks move.
#[test]
fn phase_one_collapses_across_an_element_boundary() {
    let both_sides = BoxNode::element(
        block(),
        vec![
            BoxNode::element(base(), vec![text("a ")]),
            BoxNode::element(base(), vec![text(" b")]),
        ],
    );
    assert_eq!(page_text(&run(&both_sides, 400.0, 400.0), 0), "a b");

    // **The half that tells a shared collapser from a per-element one**, and
    // the survivor of the first injection pass. In the fixture above the first
    // run *ends* with a space, so a fresh collapser per element removes the
    // second run's leading space as a leading space and answers `a b` too — the
    // right answer for the wrong reason, and a test that cannot fail.
    //
    // Here the first run ends with a letter. A collapser carried across the
    // boundary sees no pending space and keeps the one that follows; a fresh
    // one per element treats it as the start of a context and deletes it,
    // which welds two words into `ab`. §4.1.1's *"even one outside the
    // boundary of the inline containing that space"* is exactly this case.
    let only_the_second = BoxNode::element(
        block(),
        vec![
            BoxNode::element(base(), vec![text("a")]),
            BoxNode::element(base(), vec![text(" b")]),
        ],
    );
    assert_eq!(page_text(&run(&only_the_second, 400.0, 400.0), 0), "a b");
}

/// Phase II, §4.1.2, at the ends of a wrapped line — a **different** phase at a
/// different time, and a build with only phase I sets a space at the start of
/// every line after the first.
#[test]
fn phase_two_trims_the_space_a_line_broke_at() {
    // Ten characters a line: "aaaa bbbb" is nine, so the break is after the
    // space and "bbbb" starts the second line at x = 0.
    let laid = run(&para("aaaaaa bbbb cccc"), 60.0, 400.0);
    assert_eq!(page_text(&laid, 0), "aaaaaabbbbcccc");
    let xs: Vec<f64> = laid.pages[0].runs.iter().map(|r| r.x).collect();
    assert_eq!(xs, vec![0.0, 0.0, 0.0]);
    // And the trailing space of the first line was hung rather than set: the
    // line is six characters wide, not seven.
    assert_eq!(laid.pages[0].runs[0].width, 60.0);
}

/// `white-space: pre` preserves both the spaces and the segment breaks;
/// `pre-line` preserves the breaks and collapses the spaces.
#[test]
fn pre_and_pre_line_differ_about_the_spaces_and_agree_about_the_breaks() {
    let source = "a   b\nc";
    let mut pre = block();
    pre.white_space = WhiteSpace::Pre;
    let laid = run(
        &BoxNode::element(pre.clone(), vec![BoxNode::text(pre, source)]),
        400.0,
        400.0,
    );
    assert_eq!(page_text(&laid, 0), "a   bc");
    assert_eq!(baselines(&laid, 0), vec![9.0, 21.0]);

    let mut pre_line = block();
    pre_line.white_space = WhiteSpace::PreLine;
    let laid = run(
        &BoxNode::element(pre_line.clone(), vec![BoxNode::text(pre_line, source)]),
        400.0,
        400.0,
    );
    assert_eq!(page_text(&laid, 0), "a bc");
    assert_eq!(baselines(&laid, 0), vec![9.0, 21.0]);
}

/// `white-space: nowrap` collapses like `normal` and does not wrap, and the
/// two facts are separate: a build that made it preserve spaces would still
/// pass a test that only looked at the line count.
#[test]
fn nowrap_collapses_and_does_not_wrap() {
    let mut style = block();
    style.white_space = WhiteSpace::NoWrap;
    let laid = run(
        &BoxNode::element(style.clone(), vec![BoxNode::text(style, "aa  bb cc")]),
        30.0,
        400.0,
    );
    assert_eq!(page_text(&laid, 0), "aa bb cc");
    assert_eq!(baselines(&laid, 0).len(), 1);
}

// ---- css-text-3 section 5.4, overflow-wrap ---------------------------------

/// A word longer than the line overflows under `overflow-wrap: normal` and is
/// broken under `break-word`.
#[test]
fn overflow_wrap_decides_what_happens_to_a_word_longer_than_the_line() {
    let laid = run(&para("aaaaaaaaaa"), 40.0, 400.0);
    assert_eq!(baselines(&laid, 0).len(), 1);
    assert!(laid
        .warnings
        .iter()
        .any(|(w, _)| *w == Warning::LineOverflowed));

    let mut style = block();
    style.overflow_wrap = OverflowWrap::BreakWord;
    let mut inner = base();
    inner.overflow_wrap = OverflowWrap::BreakWord;
    let laid = run(
        &BoxNode::element(style, vec![BoxNode::text(inner, "aaaaaaaaaa")]),
        40.0,
        400.0,
    );
    assert_eq!(baselines(&laid, 0).len(), 3);
    assert_eq!(page_text(&laid, 0), "aaaaaaaaaa");
}

// ---- css-text-3 section 6, alignment and justification ----------------------

/// The three alignments that move a line, and the one that stretches it.
#[test]
fn the_alignments_put_a_line_where_they_say() {
    let at = |align: TextAlign| {
        let mut style = block();
        style.text_align = align;
        let laid = run(&BoxNode::element(style, vec![text("abcd")]), 100.0, 400.0);
        laid.pages[0].runs[0].x
    };
    assert_eq!(at(TextAlign::Left), 0.0);
    assert_eq!(at(TextAlign::Right), 60.0);
    assert_eq!(at(TextAlign::Center), 30.0);
}

/// Justification distributes the slack over the spaces, and **not over the
/// last line of a paragraph**.
#[test]
fn justification_stretches_every_line_but_the_last() {
    let mut style = block();
    style.text_align = TextAlign::Justify;
    // Ten characters a line, and the **last line has a space in it** — which is
    // the whole of what makes this a test. With `aa bb cc dd` the last line is
    // one word, so there is no gap to stretch and a build that justified it
    // anyway gives the same answer; that fixture survived the injection pass.
    let laid = run(
        &BoxNode::element(style, vec![text("aa bb cc dd ee")]),
        100.0,
        400.0,
    );
    assert_eq!(baselines(&laid, 0).len(), 2);
    let first = &laid.pages[0].runs[0];
    // Eight characters of ink and two spaces, in a hundred points: each space
    // takes an extra (100 - 80) / 2 = 10.
    assert_eq!(first.word_spacing, 10.0);
    assert_eq!(first.width, 100.0);
    let last = &laid.pages[0].runs[1];
    assert_eq!(
        last.word_spacing, 0.0,
        "the last line of a paragraph was justified"
    );
    assert_eq!(last.width, 50.0);
}

/// `text-indent` applies to the **first** line of a block and to no other.
#[test]
fn text_indent_is_the_first_line_only() {
    let mut style = block();
    style.text_indent = LengthPercentage::Px(20.0);
    // Six characters a line, twenty points of indent. The first line has forty
    // points to fill and the second has sixty, so the second holds two words
    // where the first holds one — and a build that subtracted the indent from
    // every line's measure sets three lines instead of two. `aaaa bbbb` cannot
    // tell those apart, which is why it is not the fixture: both answers put
    // one word on each line.
    let laid = run(
        &BoxNode::element(style, vec![text("aa bb cc")]),
        60.0,
        400.0,
    );
    let xs: Vec<f64> = laid.pages[0].runs.iter().map(|r| r.x).collect();
    assert_eq!(xs, vec![20.0, 0.0]);
    assert_eq!(page_text(&laid, 0), "aabb cc");
}

// ---- CSS 2.2 section 12.5, list markers -------------------------------------

/// The five counting styles, at the boundaries a table would get wrong.
#[test]
fn the_marker_counters_are_computed_rather_than_tabled() {
    assert_eq!(marker_text(ListStyleType::Decimal, 42), "42.");
    assert_eq!(marker_text(ListStyleType::LowerAlpha, 1), "a.");
    // Bijective base 26: after `z` comes `aa`, not `ba`, and there is no digit
    // for zero. A table of twenty-six entries gets this wrong silently on the
    // twenty-seventh item of a list.
    assert_eq!(marker_text(ListStyleType::LowerAlpha, 26), "z.");
    assert_eq!(marker_text(ListStyleType::LowerAlpha, 27), "aa.");
    assert_eq!(marker_text(ListStyleType::UpperAlpha, 52), "AZ.");
    assert_eq!(marker_text(ListStyleType::UpperRoman, 4), "IV.");
    assert_eq!(marker_text(ListStyleType::UpperRoman, 1990), "MCMXC.");
    assert_eq!(marker_text(ListStyleType::LowerRoman, 9), "ix.");
    assert_eq!(marker_text(ListStyleType::None, 3), "");
}

/// A marker is generated content: it reaches the page and is **not** part of
/// the conserved character stream.
#[test]
fn a_marker_is_on_the_page_and_out_of_the_conserved_stream() {
    let mut item = block();
    item.display = Display::ListItem;
    item.list_style_type = ListStyleType::Decimal;
    let tree = BoxNode::element(
        block(),
        vec![
            BoxNode::element(item.clone(), vec![text("first")]),
            BoxNode::element(item, vec![text("second")]),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let markers: Vec<&str> = laid.pages[0]
        .runs
        .iter()
        .filter(|r| r.generated)
        .map(|r| r.text.as_str())
        .collect();
    assert_eq!(markers, vec!["1.", "2."]);
    assert_eq!(laid.text(), "firstsecond");
    // And it sits outside the content box, which is `list-style-position:
    // outside`'s initial value.
    assert!(laid.pages[0].runs[0].x < 0.0);
}

// ---- CSS 2.2 section 13.3, fragmentation ------------------------------------

/// §13.3.1: `page-break-before: always` starts a new page whether or not the
/// current one is full.
#[test]
fn a_forced_break_starts_a_page_with_room_to_spare() {
    let mut second = block();
    second.page_break_before = PageBreak::Always;
    let tree = BoxNode::element(
        block(),
        vec![para("a"), BoxNode::element(second, vec![text("b")])],
    );
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(laid.pages.len(), 2);
    assert_eq!(page_text(&laid, 0), "a");
    assert_eq!(page_text(&laid, 1), "b");
}

/// `page-break-after: always` is the same fact from the other side, and a
/// build that read only one of the two properties passes a test that only has
/// the other.
#[test]
fn page_break_after_forces_one_too() {
    let mut first = block();
    first.page_break_after = PageBreak::Always;
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(first, vec![text("a")]), para("b")],
    );
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(laid.pages.len(), 2);
    assert_eq!(page_text(&laid, 1), "b");
}

/// Rule A: a break at a margin is refused when any element meeting there says
/// `avoid` — and the test has to show the break moving, because the answer
/// where it does not move is the same as the answer with no rule at all.
#[test]
fn rule_a_moves_the_break_up_a_block() {
    let with = |value: PageBreak| {
        let mut third = block();
        third.page_break_before = value;
        let tree = BoxNode::element(
            block(),
            vec![
                para("a"),
                para("b"),
                BoxNode::element(third, vec![text("c")]),
            ],
        );
        // Two line boxes fit; the third does not.
        let laid = run(&tree, 200.0, 30.0);
        page_text(&laid, 0)
    };
    assert_eq!(with(PageBreak::Auto), "ab");
    assert_eq!(with(PageBreak::Avoid), "a");
}

/// Rule A's other half: a forced break **beats** an `avoid` at the same
/// margin, because §13.3.3 says *"at least one of them has the value always"*
/// rather than *"none of them has the value avoid"*.
#[test]
fn a_forced_break_beats_an_avoid_at_the_same_margin() {
    let mut first = block();
    first.page_break_after = PageBreak::Avoid;
    let mut second = block();
    second.page_break_before = PageBreak::Always;
    let tree = BoxNode::element(
        block(),
        vec![
            BoxNode::element(first, vec![text("a")]),
            BoxNode::element(second, vec![text("b")]),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(laid.pages.len(), 2);
}

/// Rule B: a margin whose elements share a `page-break-inside: avoid`
/// ancestor is not a break position.
#[test]
fn rule_b_refuses_a_margin_inside_an_avoiding_ancestor() {
    let with = |value: PageBreakInside| {
        let mut parent = block();
        parent.page_break_inside = value;
        let tree = BoxNode::element(
            block(),
            vec![
                para("a"),
                BoxNode::element(parent, vec![para("b"), para("c")]),
            ],
        );
        let laid = run(&tree, 200.0, 30.0);
        page_text(&laid, 0)
    };
    assert_eq!(with(PageBreakInside::Auto), "ab");
    assert_eq!(with(PageBreakInside::Avoid), "a");
}

/// Rule B is about a **common** ancestor, and the margin between an ordinary
/// paragraph and the first child of an avoiding box has none.
///
/// A build that recorded "either side is inside something that avoids" would
/// refuse a break at the one margin that is the natural place for one — the
/// margin *before* the figure — and would push the whole figure onto the next
/// page for no reason anybody could see.
#[test]
fn rule_b_is_about_a_common_ancestor_and_not_about_either_side() {
    let mut figure = block();
    figure.page_break_inside = PageBreakInside::Avoid;
    let tree = BoxNode::element(
        block(),
        vec![
            para("a"),
            BoxNode::element(figure, vec![para("b"), para("c")]),
        ],
    );
    // Room for one line. The margin between `a` and the figure is a legal
    // break: `a` is not inside the figure, so there is no common ancestor that
    // avoids anything.
    let laid = run(&tree, 200.0, 12.0);
    assert_eq!(page_text(&laid, 0), "a");
    assert_eq!(page_text(&laid, 1), "b");
}

/// Rule C is two constraints and they interact: the same three-line paragraph
/// breaks in three different places for three pairs of values.
#[test]
fn orphans_and_widows_are_two_constraints_over_one_paragraph() {
    let with = |orphans: u16, widows: u16| {
        let mut style = block();
        style.orphans = orphans;
        style.widows = widows;
        let mut inner = base();
        inner.orphans = orphans;
        inner.widows = widows;
        // Ten characters a line, three lines.
        let tree = BoxNode::element(
            block(),
            vec![BoxNode::element(
                style,
                vec![BoxNode::text(inner, "aaaa bbbb cccc dddd eeee")],
            )],
        );
        // Two line boxes fit on a page.
        let laid = run(&tree, 100.0, 30.0);
        (
            laid.pages.iter().map(|p| p.runs.len()).collect::<Vec<_>>(),
            laid.warnings
                .iter()
                .any(|(w, _)| *w == Warning::BreakForcedPastTheRules),
        )
    };
    // One line left behind is enough and one carried forward is enough, so the
    // page takes both lines it has room for.
    assert_eq!(with(1, 1), (vec![2, 1], false));
    // Two carried forward: the break moves **up**, leaving one line behind.
    assert_eq!(with(1, 2), (vec![1, 2], false));
    // Two behind and two forward cannot both hold in a paragraph of three, so
    // §13.3.3's escape has to drop rule C — and says so.
    assert_eq!(with(2, 2), (vec![2, 1], true));
}

/// Rule D: `page-break-inside: avoid` refuses a break between the line boxes
/// of a block, and the break moves to the margin above it.
#[test]
fn rule_d_refuses_a_break_between_the_lines_of_an_avoiding_block() {
    let with = |value: PageBreakInside| {
        let mut style = block();
        style.page_break_inside = value;
        // One line either side is enough, so rule C is out of the way and rule
        // D is the only thing this fixture is about.
        style.orphans = 1;
        style.widows = 1;
        let mut inner = base();
        inner.orphans = 1;
        inner.widows = 1;
        let tree = BoxNode::element(
            block(),
            vec![
                para("a"),
                BoxNode::element(style, vec![BoxNode::text(inner, "bbbb cccc dddd")]),
            ],
        );
        // Five characters a line, so the paragraph is three lines; room for
        // three line boxes on a page.
        let laid = run(&tree, 50.0, 40.0);
        page_text(&laid, 0)
    };
    // Without it the page fills: `a`, then two of the paragraph's three lines.
    assert_eq!(with(PageBreakInside::Auto), "abbbbcccc");
    // With it, the only permitted position is the margin above the paragraph.
    assert_eq!(with(PageBreakInside::Avoid), "a");
}

/// §13.3.3's rules are dropped **one pair at a time**, and the middle tier is
/// where the break actually lands.
///
/// A survivor of the first pass, and the reason is worth keeping: with rules B
/// and D still standing, `choose` finds nothing and falls through to its last
/// resort, which cuts at the overflowing item. That is *also* where the third
/// tier would cut in most fixtures, so a build with only the first tier answers
/// identically. It answers differently exactly when the highest permitted
/// position is **below** the overflow — here, because `widows: 2` forbids
/// breaking before the last line — and then the middle tier keeps two lines on
/// the page where the last resort keeps three.
#[test]
fn the_rules_are_dropped_one_pair_at_a_time() {
    let mut style = block();
    style.page_break_inside = PageBreakInside::Avoid;
    style.orphans = 1;
    style.widows = 2;
    let mut inner = base();
    inner.orphans = 1;
    inner.widows = 2;
    // Five characters a line, four lines; room for three line boxes.
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(
            style,
            vec![BoxNode::text(inner, "aaaa bbbb cccc dddd")],
        )],
    );
    let laid = run(&tree, 50.0, 40.0);
    assert_eq!(page_text(&laid, 0), "aaaabbbb");
    // Rule D was dropped and rule C was not, so this is the middle tier and not
    // the last resort — which is the difference the warning records.
    assert!(!laid
        .warnings
        .iter()
        .any(|(w, _)| *w == Warning::BreakForcedPastTheRules));
}

/// A page break happens **in** the margin, so the margin does not reappear at
/// the top of the next page.
///
/// A survivor of the first pass, and it survived for a flat reason: every
/// fragmentation fixture in this file had zero-height margins, so a build that
/// carried the margin over to the next page put nothing there. CSS 2.2's own
/// model is that the break is *in* the vertical margin between two boxes, and a
/// twenty-point margin at the top of a chapter page is visible in any book.
#[test]
fn a_margin_a_page_breaks_in_does_not_reappear_at_the_top_of_the_next() {
    let mut first = block();
    first.margin.bottom = px(20.0);
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(first, vec![text("a")]), para("b")],
    );
    // Room for exactly one line box, so the break is taken in the margin.
    let laid = run(&tree, 200.0, 12.0);
    assert_eq!(laid.pages.len(), 2);
    assert_eq!(page_text(&laid, 1), "b");
    assert_eq!(baselines(&laid, 1), vec![9.0]);
}

/// A line box taller than a whole page is still emitted, and says so.
///
/// Dropping it would lose a line of the book, which is the one outcome
/// fragmentation must never have.
#[test]
fn a_line_taller_than_the_page_is_kept_and_reported() {
    let mut style = block();
    style.line_height = LineHeight::Px(100.0);
    let laid = run(&BoxNode::element(style, vec![text("a")]), 200.0, 20.0);
    assert_eq!(laid.text(), "a");
    assert!(laid
        .warnings
        .iter()
        .any(|(w, _)| *w == Warning::BreakForcedPastTheRules));
}

// ---- text conservation ------------------------------------------------------

/// Every character in, every character out, in order, across many pages.
///
/// Milestone 4 built this harness against thirteen grey placeholders so that
/// this milestone would inherit it. This is the first build that could violate
/// it.
#[test]
fn text_is_conserved_across_every_page_break() {
    let source: String = (0..40)
        .map(|n| format!("Chapter {n} begins here and runs on for a while. "))
        .collect();
    let tree = BoxNode::element(block(), vec![para(&source), para(&source)]);
    let laid = run(&tree, 120.0, 60.0);
    assert!(laid.pages.len() > 8, "{} pages", laid.pages.len());
    let expected = conservable(&tree.source_text());
    assert_eq!(conservable(&laid.text()), expected);
}

/// `display: none` removes exactly its own subtree's text and nothing else,
/// which is the pair of failures conservation is for: honouring it where it
/// should not be is missing text, and not honouring it is extra text.
#[test]
fn display_none_removes_exactly_its_own_text() {
    let mut gone = block();
    gone.display = Display::None;
    let tree = BoxNode::element(
        block(),
        vec![
            para("keep"),
            BoxNode::element(gone, vec![text("drop"), para("drop")]),
            para("keep"),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(laid.text(), "keepkeep");
}

// ---- CSS 2.2 9.5, floats ----------------------------------------------------

/// A line box is twelve points tall at ten-point text and its baseline is nine
/// of them down: `line-height: normal` is 1.2, the face is 0.8 above the
/// baseline and 0.2 below, and the half-leading is one point either side.
const BASELINE: f64 = 9.0;

fn floated(side: Float, width: f64) -> ComputedStyle {
    let mut style = block();
    style.float = side;
    style.width = Size::Length(LengthPercentage::Px(width));
    style
}

/// A floated box of a stated width, holding one run of text.
fn float_box(side: Float, width: f64, body: &str) -> BoxNode {
    BoxNode::element(floated(side, width), vec![text(body)])
}

/// Where the run reading `body` ended up: its left edge, and the **top** of
/// the line box it is on rather than its baseline.
fn placed(laid: &Layout, body: &str) -> (f64, f64) {
    for page in &laid.pages {
        for run in &page.runs {
            if run.text == body {
                return (run.x, run.y - BASELINE);
            }
        }
    }
    panic!("no run reads {body:?}");
}

/// **Every float fixture asserts this**, because a lost float is a lost
/// paragraph and every other assertion in this file is about a position.
fn conserved(tree: &BoxNode, laid: &Layout) {
    assert_eq!(
        conservable(&laid.text()),
        conservable(&tree.source_text()),
        "text conservation"
    );
}

/// §9.5.1 rule 1: a float's outer edge does not leave its containing block —
/// the **left** edge of a left float and the **right** edge of a right one.
///
/// The containing block is inset from the page on both sides, so a build that
/// clamped to the page rather than to the block fails here and nowhere else:
/// every other fixture in this section is laid out in a containing block that
/// starts at the page's own left edge.
#[test]
fn rule_1_a_float_does_not_leave_its_containing_block() {
    let mut cb = block();
    cb.margin.left = px(50.0);
    cb.margin.right = px(40.0);
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(
            cb,
            vec![
                float_box(Float::Left, 20.0, "L"),
                float_box(Float::Right, 30.0, "R"),
            ],
        )],
    );
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(placed(&laid, "L"), (50.0, 0.0));
    // 300 less the 40-point right margin, less the float's own 30.
    assert_eq!(placed(&laid, "R"), (230.0, 0.0));
    conserved(&tree, &laid);
}

/// Rule 2: a left float is to the right of every earlier left float it is
/// beside, or below it.
#[test]
fn rule_2_a_float_does_not_overlap_an_earlier_float_on_its_own_side() {
    let tree = BoxNode::element(
        block(),
        vec![
            float_box(Float::Left, 40.0, "A"),
            float_box(Float::Left, 40.0, "B"),
        ],
    );
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(placed(&laid, "A"), (0.0, 0.0));
    assert_eq!(placed(&laid, "B"), (40.0, 0.0));
    conserved(&tree, &laid);

    // **And mirrored, which is a different line of code.** Every fixture in
    // this section was built out of left floats until the injection campaign
    // pointed out that the right-hand half of this rule is not exercised by any
    // of them: a rule with two mirrored halves needs both asserted or half of
    // it is untested.
    let mirrored = BoxNode::element(
        block(),
        vec![
            float_box(Float::Right, 40.0, "C"),
            float_box(Float::Right, 40.0, "D"),
        ],
    );
    let laid = run(&mirrored, 300.0, 400.0);
    assert_eq!(placed(&laid, "C"), (260.0, 0.0));
    assert_eq!(placed(&laid, "D"), (220.0, 0.0));
    conserved(&mirrored, &laid);
}

/// Rule 3: a left float's right edge does not cross an earlier **right**
/// float's left edge, and where it would the left float goes below it.
///
/// Not rule 7: nothing here is stacked on its own side, and the containing
/// block is wide enough for either float alone.
#[test]
fn rule_3_a_float_does_not_cross_an_earlier_float_on_the_other_side() {
    let tree = BoxNode::element(
        block(),
        vec![
            float_box(Float::Right, 200.0, "R"),
            float_box(Float::Left, 150.0, "L"),
        ],
    );
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(placed(&laid, "R"), (100.0, 0.0));
    assert_eq!(placed(&laid, "L"), (0.0, 12.0));
    conserved(&tree, &laid);

    // And mirrored: a right float that cannot get past an earlier left one.
    let mirrored = BoxNode::element(
        block(),
        vec![
            float_box(Float::Left, 200.0, "M"),
            float_box(Float::Right, 150.0, "N"),
        ],
    );
    let laid = run(&mirrored, 300.0, 400.0);
    assert_eq!(placed(&laid, "M"), (0.0, 0.0));
    assert_eq!(placed(&laid, "N"), (150.0, 12.0));
    conserved(&mirrored, &laid);
}

/// Rule 4: a float's outer top is not above the top of its containing block.
///
/// **Every fixture for rules 4, 5 and 6 is built out of negative margins**, and
/// this is the one that says why: with positive margins the static position is
/// already below all three ceilings and none of the three does anything, so a
/// build with all three deleted passes. Here the paragraph's own top margin
/// pulls it thirty points above the containing block's content edge and its
/// bottom margin pulls the float thirty further, so the float's static position
/// is above the block it is in — and rules 5 and 6, which are about that
/// paragraph, are ten points *higher* than rule 4 and cannot be what stopped
/// it.
#[test]
fn rule_4_a_float_does_not_rise_above_its_containing_block() {
    let mut cb = block();
    cb.padding.top = LengthPercentage::Px(50.0);
    let mut early = block();
    early.margin.top = px(-40.0);
    early.margin.bottom = px(-30.0);
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(
            cb,
            vec![
                BoxNode::element(early, vec![text("aaa")]),
                float_box(Float::Left, 20.0, "F"),
            ],
        )],
    );
    let laid = run(&tree, 300.0, 400.0);
    // The paragraph's border box is at 10 and its line with it; the containing
    // block's content edge is at 50, and that is where the float stops.
    assert_eq!(placed(&laid, "aaa"), (0.0, 10.0));
    assert_eq!(placed(&laid, "F"), (0.0, 50.0));
    conserved(&tree, &laid);
}

/// Rule 5: nor above the border-box top of any box an earlier element made.
///
/// The earlier box here is an empty spacer with a top margin, so its own top
/// (42) is below every line box in the document (0) — which is what makes this
/// rule 5's fixture and not rule 6's, and the containing block's top is 0,
/// which is what stops rule 4 from covering for it.
#[test]
fn rule_5_a_float_does_not_rise_above_an_earlier_box() {
    let mut spacer = block();
    spacer.margin.top = px(30.0);
    spacer.margin.bottom = px(-1000.0);
    spacer.height = Size::Length(LengthPercentage::Px(10.0));
    let tree = BoxNode::element(
        block(),
        vec![
            para("aaa"),
            BoxNode::element(spacer, Vec::new()),
            float_box(Float::Left, 20.0, "F"),
        ],
    );
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(placed(&laid, "aaa"), (0.0, 0.0));
    assert_eq!(placed(&laid, "F"), (0.0, 42.0));
    conserved(&tree, &laid);
}

/// Rule 6: nor above the top of any line box holding earlier content.
///
/// The earlier paragraph's border box is at 0 and its **line** is at 40,
/// because of a padding rather than a margin — so rule 5's ceiling is 0 here
/// and only rule 6 can be what held the float at 40.
#[test]
fn rule_6_a_float_does_not_rise_above_an_earlier_line_box() {
    let mut early = block();
    early.padding.top = LengthPercentage::Px(40.0);
    early.margin.bottom = px(-1000.0);
    let tree = BoxNode::element(
        block(),
        vec![
            BoxNode::element(early, vec![text("aaa")]),
            float_box(Float::Left, 20.0, "F"),
        ],
    );
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(placed(&laid, "aaa"), (0.0, 40.0));
    assert_eq!(placed(&laid, "F"), (0.0, 40.0));
    conserved(&tree, &laid);
}

/// Rule 7: two floats that do not fit side by side, and one that is wider than
/// everything and does not move.
///
/// **The second half is the point.** Rule 7 is written *"a left-floating box
/// that has another left-floating box to its left"*, and a build that reads it
/// as *"a float that does not fit its containing block goes below"* passes the
/// first assertion and fails the second: a lone float wider than its container
/// overflows it, exactly where it was written, and going down a line would not
/// help it fit.
#[test]
fn rule_7_two_floats_that_do_not_fit_side_by_side_stack() {
    let tree = BoxNode::element(
        block(),
        vec![
            float_box(Float::Left, 200.0, "A"),
            float_box(Float::Left, 150.0, "B"),
        ],
    );
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(placed(&laid, "A"), (0.0, 0.0));
    assert_eq!(placed(&laid, "B"), (0.0, 12.0));
    conserved(&tree, &laid);

    let alone = BoxNode::element(block(), vec![float_box(Float::Left, 400.0, "W")]);
    let laid = run(&alone, 300.0, 400.0);
    assert_eq!(placed(&laid, "W"), (0.0, 0.0));
    conserved(&alone, &laid);

    // The same float, with something on the *other* side of it, which is what
    // separates rule 7 from the clamp it looks like. Rule 3 sends `W` below
    // `E`; a build that clamped every float to its containing block's far edge
    // rejects `W` at every height there is and puts it back at the top,
    // straight through `E`. The two answers are the same in every arrangement
    // without an opposite-side float in it, which is why this assertion is
    // here and why the injection campaign is what found that out.
    let opposed = BoxNode::element(
        block(),
        vec![
            float_box(Float::Right, 100.0, "E"),
            float_box(Float::Left, 400.0, "F"),
        ],
    );
    let laid = run(&opposed, 300.0, 400.0);
    assert_eq!(placed(&laid, "E"), (200.0, 0.0));
    assert_eq!(placed(&laid, "F"), (0.0, 12.0));
    conserved(&opposed, &laid);

    // And mirrored, where the clamp is against the containing block's **left**
    // edge: without it the second right float is placed at -50, which is off
    // the page and outside any block on it.
    let mirrored = BoxNode::element(
        block(),
        vec![
            float_box(Float::Right, 200.0, "C"),
            float_box(Float::Right, 150.0, "D"),
        ],
    );
    let laid = run(&mirrored, 300.0, 400.0);
    assert_eq!(placed(&laid, "C"), (100.0, 0.0));
    assert_eq!(placed(&laid, "D"), (150.0, 12.0));
    conserved(&mirrored, &laid);
}

/// Rule 8: as high as possible — which means going back **up** for the third
/// float, not carrying on down the page.
///
/// `B` did not fit beside `A` and went below it. `C` fits beside `B`, and a
/// build that placed each float under the last one puts it at 24 instead: every
/// other rule here is satisfied by that answer.
#[test]
fn rule_8_a_float_is_placed_as_high_as_it_fits() {
    let tree = BoxNode::element(
        block(),
        vec![
            float_box(Float::Left, 200.0, "A"),
            float_box(Float::Left, 150.0, "B"),
            float_box(Float::Left, 50.0, "C"),
        ],
    );
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(placed(&laid, "A"), (0.0, 0.0));
    assert_eq!(placed(&laid, "B"), (0.0, 12.0));
    assert_eq!(placed(&laid, "C"), (150.0, 12.0));
    conserved(&tree, &laid);
}

/// Rule 9: and then as far to the left, or to the right, as possible.
///
/// Both ends in one fixture, because the rule has two halves and a build that
/// implemented one of them would put every float at the same edge.
#[test]
fn rule_9_a_float_goes_to_the_far_end_of_the_room_it_has() {
    let tree = BoxNode::element(
        block(),
        vec![
            float_box(Float::Left, 50.0, "L"),
            float_box(Float::Right, 50.0, "R"),
        ],
    );
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(placed(&laid, "L"), (0.0, 0.0));
    assert_eq!(placed(&laid, "R"), (250.0, 0.0));
    conserved(&tree, &laid);
}

/// §9.5.2: `clear` moves a box below the floats on the sides it names, and
/// **only** those sides.
#[test]
fn clearance_moves_a_box_below_the_floats_it_names() {
    let cleared = |value: Clear| {
        let mut style = block();
        style.clear = value;
        let tree = BoxNode::element(
            block(),
            vec![
                float_box(Float::Left, 100.0, "F"),
                BoxNode::element(style, vec![text("x")]),
            ],
        );
        let laid = run(&tree, 300.0, 400.0);
        conserved(&tree, &laid);
        placed(&laid, "x")
    };
    assert_eq!(cleared(Clear::Left), (0.0, 12.0));
    assert_eq!(cleared(Clear::Both), (0.0, 12.0));
    // A left float is not a right one. Both of these leave the paragraph
    // beside the float, shortened — which is the answer `clear: right` is
    // supposed to give and the answer a build that treated every value as
    // `both` never gives.
    assert_eq!(cleared(Clear::Right), (100.0, 0.0));
    assert_eq!(cleared(Clear::None), (100.0, 0.0));
}

/// Clearance is the distance a box **still needs**, not the float's bottom.
///
/// The paragraph's own thirty-point top margin already takes it past the
/// twelve-point float, so its clearance is zero and it sits at 30. A build that
/// moved the box to the float's bottom and then applied the margin puts it at
/// 42, and every fixture whose cleared box has no margin agrees with both.
#[test]
fn clearance_is_what_is_still_needed_and_not_the_floats_bottom() {
    let mut style = block();
    style.clear = Clear::Left;
    style.margin.top = px(30.0);
    let tree = BoxNode::element(
        block(),
        vec![
            float_box(Float::Left, 100.0, "F"),
            BoxNode::element(style, vec![text("x")]),
        ],
    );
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(placed(&laid, "x"), (0.0, 30.0));
    conserved(&tree, &laid);
}

/// Clearance also **stops the margins either side of it collapsing** (§8.3.1),
/// which is a second job and not a consequence of the first.
///
/// The paragraph above has a 20-point bottom margin, the cleared one a
/// 10-point top margin, and the float ends at 48. Introduce clearance without
/// separating the two margins and they collapse to 20, the six points of
/// clearance are added to that instead of to their sum, and the cleared
/// paragraph lands at 38 — **above the float it was supposed to clear**. Every
/// other fixture here has a float for a predecessor, and a float contributes no
/// adjoining margin at all, so all of them agree with the broken build.
#[test]
fn clearance_does_not_let_the_margins_it_sits_between_collapse() {
    let mut early = block();
    early.margin.bottom = px(20.0);
    let mut cleared = block();
    cleared.clear = Clear::Left;
    cleared.margin.top = px(10.0);
    let tree = BoxNode::element(
        block(),
        vec![
            float_box(Float::Left, 40.0, "aa bb cc dd"),
            BoxNode::element(early, vec![text("a")]),
            BoxNode::element(cleared, vec![text("x")]),
        ],
    );
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(placed(&laid, "a"), (40.0, 0.0));
    assert_eq!(placed(&laid, "x"), (0.0, 48.0));
    conserved(&tree, &laid);
}

/// §9.5: a line box is shortened beside a float and **restored below it**.
///
/// The second half is what a build that only narrows the first line gets
/// wrong, and it is the half a reader notices: forty points of the measure come
/// back the moment the figure ends.
#[test]
fn a_line_box_is_shortened_beside_a_float_and_restored_below_it() {
    let tree = BoxNode::element(
        block(),
        vec![float_box(Float::Left, 40.0, "F"), text("aa bb cc dd ee ff")],
    );
    let laid = run(&tree, 100.0, 400.0);
    let runs = &laid.pages[0].runs;
    assert_eq!(runs[0].text, "F");
    // Sixty points of measure beside the float: five characters of it.
    assert_eq!((runs[1].x, runs[1].text.as_str()), (40.0, "aa bb"));
    // And a hundred below it: eight.
    assert_eq!((runs[2].x, runs[2].text.as_str()), (0.0, "cc dd ee"));
    assert_eq!(runs[2].y, 12.0 + BASELINE);
    conserved(&tree, &laid);

    // **The other side is a different line of code**, and the line beside a
    // right float does not move: it is the measure that changes, so the
    // assertion has to be about where the line *broke*. A build that ignored
    // right floats sets eight characters on the first line instead of five and
    // puts every one of them where a reader would expect them.
    let mirrored = BoxNode::element(
        block(),
        vec![
            float_box(Float::Right, 40.0, "F"),
            text("aa bb cc dd ee ff"),
        ],
    );
    let laid = run(&mirrored, 100.0, 400.0);
    let runs = &laid.pages[0].runs;
    assert_eq!(placed(&laid, "F"), (60.0, 0.0));
    assert_eq!((runs[1].x, runs[1].text.as_str()), (0.0, "aa bb"));
    assert_eq!((runs[2].x, runs[2].text.as_str()), (0.0, "cc dd ee"));
    conserved(&mirrored, &laid);
}

/// A float taller than its containing block goes on shortening the line boxes
/// of the blocks after it.
///
/// The containing block is twenty points tall and the float is thirty-six, so
/// two of the next block's lines are beside a float that its own container
/// finished with — which is what *"a float can overflow its containing block"*
/// means and what a build that reset the float context per block loses.
#[test]
fn a_float_taller_than_its_containing_block_goes_on_shortening_lines() {
    let mut cb = block();
    cb.height = Size::Length(LengthPercentage::Px(20.0));
    let tree = BoxNode::element(
        block(),
        vec![
            BoxNode::element(cb, vec![float_box(Float::Left, 40.0, "aa bb cc")]),
            BoxNode::element(block(), vec![text("dd ee ff gg hh")]),
        ],
    );
    let laid = run(&tree, 100.0, 400.0);
    assert_eq!(placed(&laid, "aa"), (0.0, 0.0));
    assert_eq!(placed(&laid, "cc"), (0.0, 24.0));
    assert_eq!(placed(&laid, "dd ee"), (40.0, 20.0));
    assert_eq!(placed(&laid, "ff gg"), (40.0, 32.0));
    assert_eq!(placed(&laid, "hh"), (0.0, 44.0));
    conserved(&tree, &laid);
}

/// A float inside a paragraph does not break the paragraph in two.
///
/// The words either side of it are one inline formatting context and set as one
/// line, and the float's own text reads **between** them — which is document
/// order and not emission order, and the reason [`crate::TextRun::order`]
/// exists.
#[test]
fn a_float_inside_a_paragraph_keeps_the_paragraph_whole() {
    let mut style = floated(Float::Left, 20.0);
    style.display = Display::Inline;
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(
            block(),
            vec![
                text("aa "),
                BoxNode::element(style, vec![text("F")]),
                text("bb cc"),
            ],
        )],
    );
    let laid = run(&tree, 100.0, 400.0);
    let runs = &laid.pages[0].runs;
    let read: Vec<&str> = runs.iter().map(|run| run.text.as_str()).collect();
    assert_eq!(read, vec!["aa ", "F", "bb cc"]);
    // One line, at the eighty points the float left of the measure.
    assert_eq!((runs[0].x, runs[0].y), (20.0, BASELINE));
    assert_eq!((runs[2].x, runs[2].y), (50.0, BASELINE));
    conserved(&tree, &laid);
}

/// **A float that would fall off the page bottom is pushed to the next page**,
/// whole, because nothing of it has been drawn yet.
///
/// This is the fragmentation interaction and the one that loses text: a build
/// that drew the float where the column put it would put it eight points past
/// the foot of the page, and a build that stopped paginating when the column
/// ran out would not draw it at all.
#[test]
fn a_float_that_would_fall_off_the_page_bottom_is_pushed_whole() {
    let tree = BoxNode::element(
        block(),
        vec![
            para("aaaa bbbb cccc dddd eeee ffff"),
            float_box(Float::Left, 40.0, "xx"),
            para("gg hh ii"),
        ],
    );
    let laid = run(&tree, 100.0, 40.0);
    assert_eq!(laid.pages.len(), 2);
    assert!(
        !laid.pages[0].runs.iter().any(|run| run.text == "xx"),
        "the float was drawn on the page it did not fit"
    );
    assert!(laid.pages[1].runs.iter().any(|run| run.text == "xx"));
    // **Pushed, not broken**, which is the whole difference and is a warning
    // rather than a position: a build that drew the float's zero-height top
    // margin on the first page and the rest of it on the second puts every run
    // in the same place as this one and is doing something else.
    assert!(
        !laid
            .warnings
            .iter()
            .any(|(w, _)| *w == Warning::FloatBrokenAcrossPages),
        "the float was broken where it should have been pushed"
    );
    for (at, page) in laid.pages.iter().enumerate() {
        for run in &page.runs {
            assert!(
                run.y - BASELINE >= -1e-9 && run.y + 3.0 <= 40.0 + 1e-9,
                "page {at} draws {:?} at {} on a page 40 points tall",
                run.text,
                run.y
            );
        }
    }
    conserved(&tree, &laid);
}

/// A float too tall for the page it **began** on is broken across the boundary
/// and says so.
///
/// Pushing is not available once part of it is drawn — the lines beside it were
/// shortened for a float in that position — so what is left of it continues at
/// the top of the next page, and [`Warning::FloatBrokenAcrossPages`] names the
/// gap between that and `css-break-3`.
#[test]
fn a_float_broken_across_a_page_keeps_all_of_itself_and_says_so() {
    let tree = BoxNode::element(
        block(),
        vec![
            float_box(Float::Left, 40.0, "aa bb cc dd"),
            text("one two three four five six seven"),
        ],
    );
    let laid = run(&tree, 100.0, 40.0);
    assert_eq!(laid.pages.len(), 2);
    assert!(laid
        .warnings
        .iter()
        .any(|(w, _)| *w == Warning::FloatBrokenAcrossPages));
    let page_one: Vec<&str> = laid.pages[0]
        .runs
        .iter()
        .map(|run| run.text.as_str())
        .collect();
    assert_eq!(page_one[..3], ["aa", "bb", "cc"]);
    assert!(laid.pages[1].runs.iter().any(|run| run.text == "dd"));
    for page in &laid.pages {
        for run in &page.runs {
            assert!(run.y - BASELINE >= -1e-9 && run.y + 3.0 <= 40.0 + 1e-9);
        }
    }
    conserved(&tree, &laid);
}

/// §9.5's second sentence: a line box with no room left beside a float is
/// **shifted down** until it has some.
///
/// The float leaves ten points of measure, which is a third of the first word,
/// so the line goes under the float and gets the whole hundred. A build with
/// only the first sentence sets that paragraph one character per line down a
/// ten-point gutter and every word of it is still there, which is why text
/// conservation cannot be what catches this.
#[test]
fn a_line_with_no_room_beside_a_float_goes_under_it() {
    let tree = BoxNode::element(
        block(),
        vec![float_box(Float::Left, 90.0, "x"), text("aaa bbb")],
    );
    let laid = run(&tree, 100.0, 400.0);
    assert_eq!(placed(&laid, "aaa bbb"), (0.0, 12.0));
    conserved(&tree, &laid);

    // **Under the next float down, not under the last one.** Two floats at two
    // heights: the first leaves ten points and the second leaves eighty, and
    // the line belongs beside the second. A build that went to the bottom of
    // every float it could see loses the eighty points and sets the paragraph
    // a line lower than it belongs, which is a page that looks perfectly
    // ordinary.
    let stepped = BoxNode::element(
        block(),
        vec![
            float_box(Float::Left, 90.0, "x"),
            float_box(Float::Left, 20.0, "y"),
            text("aaa bbb"),
        ],
    );
    let laid = run(&stepped, 100.0, 400.0);
    assert_eq!(placed(&laid, "y"), (0.0, 12.0));
    assert_eq!(placed(&laid, "aaa bbb"), (20.0, 12.0));
    conserved(&stepped, &laid);

    // And a word that would not fit the **full** measure either is not a
    // reason to go looking below anything: that line overflows wherever it is
    // put, and moving it down loses the float's own gutter for nothing.
    let long = BoxNode::element(
        block(),
        vec![float_box(Float::Left, 90.0, "x"), text("aaaaaaaaaaaaaaa")],
    );
    let laid = run(&long, 100.0, 400.0);
    assert_eq!(placed(&laid, "aaaaaaaaaaaaaaa"), (90.0, 0.0));
    conserved(&long, &laid);
}

/// A float that outlives the column it was written in still gets a page.
///
/// The book is one line long and the figure beside it is four. The column runs
/// out on the first page and the float does not, and a build that stopped
/// paginating when the column ran out simply never draws the rest of it —
/// which is a lost paragraph that renders beautifully, and the exact defect
/// milestone 4 built text conservation against.
#[test]
fn a_float_outliving_the_column_still_gets_a_page() {
    let tree = BoxNode::element(
        block(),
        vec![float_box(Float::Left, 40.0, "aa bb cc dd"), text("z")],
    );
    let laid = run(&tree, 100.0, 40.0);
    assert_eq!(laid.pages.len(), 2);
    assert!(laid.pages[1].runs.iter().any(|run| run.text == "dd"));
    conserved(&tree, &laid);
}

/// A float continuing onto a page starts at the **top** of it.
///
/// The column and the float are cut at different heights — the float has a
/// six-point top margin and the text has none — so the float's next item
/// belongs six points above the page the column broke at. Drawn where the
/// column would put it, it is six points off the top of the page; drawn at the
/// top of the page, it is beside the text it was written beside. Every fixture
/// whose two cuts happened to coincide agrees with both.
#[test]
fn a_float_continuing_onto_a_page_starts_at_the_top_of_it() {
    let mut style = floated(Float::Left, 40.0);
    style.margin.top = px(6.0);
    let tree = BoxNode::element(
        block(),
        vec![
            para("aa"),
            BoxNode::element(style, vec![text("bb cc dd")]),
            text("ee ff gg hh ii jj kk ll mm nn"),
        ],
    );
    let laid = run(&tree, 100.0, 40.0);
    assert_eq!(laid.pages.len(), 2);
    assert!(laid.pages[0].runs.iter().any(|run| run.text == "bb"));
    let continued = laid.pages[1]
        .runs
        .iter()
        .find(|run| run.text == "cc")
        .expect("the float continues on the second page");
    assert_eq!((continued.x, continued.y), (0.0, BASELINE));
    conserved(&tree, &laid);
}

/// §10.3.5: a float with no stated width is shrunk to fit its content.
///
/// `min(max(preferred minimum, available), preferred)`, and all three are
/// visible here: the preferred width of `"aa bb"` is fifty points, which is
/// less than the hundred available, so the float is fifty wide and the
/// paragraph beside it gets the other fifty. A build that gave an `auto` float
/// the whole containing block — which is what `width: auto` means for every
/// *other* block — leaves the paragraph nothing.
#[test]
fn a_float_with_no_width_is_shrunk_to_fit() {
    let mut style = block();
    style.float = Float::Left;
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(style, vec![text("aa bb")]), text("cc dd")],
    );
    let laid = run(&tree, 100.0, 400.0);
    assert_eq!(placed(&laid, "aa bb"), (0.0, 0.0));
    assert_eq!(placed(&laid, "cc dd"), (50.0, 0.0));
    conserved(&tree, &laid);
}

// ---- warnings ---------------------------------------------------------------

/// `display: inline-block` is named rather than laid out, and is **not**
/// mapped onto its nearest implemented neighbour.
///
/// Until milestone 10 this fixture was about `float`, which is now laid out
/// where it was named. What it is about has changed; what it says has not: a
/// property this build cannot honour is reported by name, because a page that
/// is plausible and wrong is the failure this whole plan is organised against.
#[test]
fn an_unimplemented_property_is_named_rather_than_approximated() {
    let mut style = block();
    style.display = Display::InlineBlock;
    let tree = BoxNode::element(block(), vec![BoxNode::element(style, vec![text("a")])]);
    let laid = run(&tree, 200.0, 400.0);
    assert!(laid
        .warnings
        .iter()
        .any(|(w, _)| *w == Warning::InlineBlockAsInline));
}

/// A warning is counted, not repeated — ruling 10's shape and
/// `tinker_pdf_css::parser::Report`'s.
#[test]
fn warnings_are_deduplicated_with_a_count() {
    let mut style = block();
    style.display = Display::InlineBlock;
    let children: Vec<BoxNode> = (0..5)
        .map(|_| BoxNode::element(style.clone(), vec![text("a")]))
        .collect();
    let laid = run(&BoxNode::element(block(), children), 200.0, 400.0);
    let entry = laid
        .warnings
        .iter()
        .find(|(w, _)| *w == Warning::InlineBlockAsInline)
        .expect("the display value was named");
    assert_eq!(entry.1, 5);
}

// ---- bounds -----------------------------------------------------------------

/// Every cap fires **by its own refusal**, and each is built by handing the
/// real constant the input it bounds rather than by lowering it.
/// The depth cap is enforced in **two** places — the block walk and the inline
/// gather — and each is reached by a different tree.
///
/// A survivor of the first injection pass, and this session's named failure
/// mode: deleting the check in `block` changed no answer, because the fixture
/// was a chain of blocks ending in *text*, and the text is gathered one level
/// deeper than the last block. The gather's own check caught it, so the block's
/// was never the reason.
#[test]
fn a_tree_of_blocks_past_the_depth_cap_is_refused_by_name() {
    // No text anywhere, so the inline gather never runs and this is the block
    // walk's check or nothing.
    let mut node = BoxNode::element(block(), Vec::new());
    for _ in 0..(Limits::DEFAULT.max_depth + 2) {
        node = BoxNode::element(block(), vec![node]);
    }
    let refusal = layout(
        &node,
        &METRICS,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect_err("a tree of blocks past the depth cap");
    assert!(matches!(refusal, Refusal::TooDeep { .. }));
}

#[test]
fn a_tree_of_inlines_past_the_depth_cap_is_refused_by_name() {
    // One block and then nothing but inlines, so the block walk never recurses
    // and this is the gather's check or nothing.
    let mut node = text("a");
    for _ in 0..(Limits::DEFAULT.max_depth + 2) {
        node = BoxNode::element(base(), vec![node]);
    }
    let refusal = layout(
        &BoxNode::element(block(), vec![node]),
        &METRICS,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect_err("a tree of inlines past the depth cap");
    assert!(matches!(refusal, Refusal::TooDeep { .. }));
}

/// Each of the three below builds the **real** constant's own input rather
/// than lowering the constant, which is gap 29's discipline: *a cap proved only
/// against a lowered copy of itself has not been proved to fire.*
#[test]
fn a_tree_past_the_box_cap_is_refused_by_name() {
    let children: Vec<BoxNode> = (0..=Limits::DEFAULT.max_boxes).map(|_| text("a")).collect();
    let refusal = layout(
        &BoxNode::element(block(), children),
        &METRICS,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect_err("a tree past the box cap");
    assert!(matches!(refusal, Refusal::TooManyBoxes { .. }));
}

#[test]
fn a_paragraph_past_the_line_break_budget_is_refused_by_name() {
    // One character over the cap, and it is refused **before** the class table
    // allocates a unit per character — which is what keeps a four-million-
    // character fixture four megabytes rather than a hundred and fifty.
    let long = "a".repeat(Limits::DEFAULT.max_break_work + 1);
    let refusal = layout(
        &para(&long),
        &METRICS,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect_err("a paragraph past the break budget");
    assert!(matches!(refusal, Refusal::TooMuchLineBreaking { .. }));
    // And one character under it is read, so the cap is a boundary rather than
    // a floor.
    let short = "a".repeat(Limits::DEFAULT.max_break_work - 1);
    assert!(layout(
        &para(&short),
        &METRICS,
        &Options::new(2_000_000.0, 400.0),
        &Limits::DEFAULT,
    )
    .is_ok());
}

/// The break budget is a **total**, spent across every formatting context in
/// a book and never refunded.
///
/// A survivor of the first pass: the firing test above is one paragraph over
/// the cap, so a build that assigned the count instead of adding it answered
/// identically. This one is three paragraphs, each comfortably under the cap
/// and over it together — `tinker-pdf-css`'s
/// `the_token_total_is_spent_across_sheets_and_not_per_sheet`, one crate up.
/// The cap is lowered here on purpose and the real constant is fired above:
/// what is being tested is the arithmetic, not the number.
#[test]
fn the_break_total_is_spent_across_paragraphs_and_not_per_paragraph() {
    let limits = Limits {
        max_break_work: 10,
        ..Limits::DEFAULT
    };
    let one = BoxNode::element(block(), vec![para("aaaa")]);
    assert!(layout(&one, &METRICS, &Options::new(200.0, 400.0), &limits).is_ok());
    let three = BoxNode::element(block(), vec![para("aaaa"), para("aaaa"), para("aaaa")]);
    let refusal = layout(&three, &METRICS, &Options::new(200.0, 400.0), &limits)
        .expect_err("three paragraphs past the total");
    assert!(matches!(refusal, Refusal::TooMuchLineBreaking { .. }));
}

#[test]
fn a_book_past_the_page_cap_is_refused_by_name() {
    // A preserved newline is a line box, and a page twelve points tall holds
    // exactly one twelve-point line — so the flow is one page per character.
    let mut style = block();
    style.white_space = WhiteSpace::Pre;
    let mut inner = base();
    inner.white_space = WhiteSpace::Pre;
    let breaks = "\n".repeat(Limits::DEFAULT.max_pages + 1);
    let refusal = layout(
        &BoxNode::element(style, vec![BoxNode::text(inner, breaks)]),
        &METRICS,
        &Options::new(200.0, 12.0),
        &Limits::DEFAULT,
    )
    .expect_err("a book past the page cap");
    assert!(matches!(refusal, Refusal::TooManyPages { .. }));
}

/// **The float total fires by its own refusal**, and it is reached by floats
/// rather than by lowering it.
///
/// A thousand-odd figures in one chapter, each examined against the ones before
/// it: the work is the square of the float count while every other cap is
/// satisfied — three thousand floats is twelve thousand boxes against a cap of
/// 2 097 152 and three thousand characters against a break total of four
/// million.
/// That is the shape [`crate::limits::MAX_LAYOUT_WORK`] exists for and the one
/// neither of the other two work caps can see, because the work is their
/// **product** rather than either of them.
#[test]
fn a_book_past_the_float_work_total_is_refused_by_name() {
    // 17 992 500 examinations, measured rather than derived: milestone 13
    // raised the cap fourfold and every fixture here was re-measured against
    // the new one with `Budget::layout()` at an unreachable ceiling.
    let floats: Vec<BoxNode> = (0..3_000)
        .map(|_| float_box(Float::Left, 40.0, "x"))
        .collect();
    let refusal = layout(
        &BoxNode::element(block(), floats),
        &METRICS,
        &Options::new(100.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect_err("a book past the float total");
    assert!(
        matches!(refusal, Refusal::TooMuchLayoutWork { .. }),
        "{refusal:?}"
    );
}

/// **And it fires the other way too**, because the total is charged in two
/// places and a total charged in two places has two reachable halves.
///
/// Twelve hundred figures is 3 597 000 examinations to place — a long way
/// under the cap — and the three thousand six hundred line boxes after them ask
/// all twelve hundred for their measure, which is the other 15 120 000. Both
/// halves are **individually** under the cap and together they are over it:
/// delete either charge and this book lays out; delete neither and it is
/// refused by name.
#[test]
fn a_book_past_the_float_work_total_through_its_line_boxes_is_refused_by_name() {
    let mut children: Vec<BoxNode> = (0..1_200)
        .map(|_| float_box(Float::Left, 60.0, "x"))
        .collect();
    children.push(para(&"aaaa bbbb ".repeat(3_600)));
    let refusal = layout(
        &BoxNode::element(block(), children),
        &METRICS,
        &Options::new(100.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect_err("a book past the float total");
    assert!(
        matches!(refusal, Refusal::TooMuchLayoutWork { .. }),
        "{refusal:?}"
    );
}

/// And a third time, through `clear`.
///
/// §9.5.2 asks the same question of the same list — *how far down do the floats
/// on these sides reach* — and a book may ask it once per block. A thousand
/// figures and fourteen thousand cleared blocks is fourteen million
/// examinations that neither of the other two fixtures makes, and the scan
/// behind it is the same quadratic: blocks times floats, with both bounded only
/// by the box cap.
#[test]
fn a_book_past_the_float_work_total_through_its_clearances_is_refused_by_name() {
    let mut cleared = block();
    cleared.clear = Clear::Left;
    let mut children: Vec<BoxNode> = (0..1_000)
        .map(|_| float_box(Float::Left, 60.0, "x"))
        .collect();
    children.extend((0..14_000).map(|_| BoxNode::element(cleared.clone(), Vec::new())));
    let refusal = layout(
        &BoxNode::element(block(), children),
        &METRICS,
        &Options::new(100.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect_err("a book past the float total");
    assert!(
        matches!(refusal, Refusal::TooMuchLayoutWork { .. }),
        "{refusal:?}"
    );
}

/// And it is not reached by a book that is merely long: the same thousand
/// figures spread over the forty spine items of the plan's own yardstick spend
/// a fraction of it.
///
/// A cap that a real book reaches is a cap that refuses real books, which is
/// the failure gap 18a's milestone 8 records in the other direction.
#[test]
fn an_illustrated_chapter_is_nowhere_near_the_float_work_total() {
    let mut children: Vec<BoxNode> = Vec::new();
    for _ in 0..25 {
        children.push(float_box(Float::Left, 40.0, "x"));
        children.push(para("aa bb cc dd ee ff gg hh ii jj kk ll mm nn oo pp"));
    }
    let mut budget = Budget::new(&Limits::DEFAULT);
    layout_with(
        &BoxNode::element(block(), children),
        &METRICS,
        &Options::new(300.0, 400.0),
        &Limits::DEFAULT,
        &mut budget,
    )
    .expect("a chapter with a figure every other paragraph");
    assert!(
        budget.layout() * 100 < Limits::DEFAULT.max_layout_work,
        "a chapter with 25 figures spent {} of {}",
        budget.layout(),
        Limits::DEFAULT.max_layout_work
    );
}

/// A page with no area is refused by name rather than paginated for ever.
#[test]
fn a_page_with_no_room_is_refused_by_name() {
    for (width, height) in [(0.0, 100.0), (100.0, 0.0), (f64::NAN, 100.0)] {
        let refusal = layout(
            &para("a"),
            &METRICS,
            &Options::new(width, height),
            &Limits::DEFAULT,
        )
        .expect_err("a page with no area");
        assert!(matches!(refusal, Refusal::PageTooSmall { .. }));
    }
}

/// A book with nothing in it is one empty page rather than none.
#[test]
fn an_empty_tree_is_one_empty_page() {
    let laid = run(&BoxNode::element(block(), Vec::new()), 200.0, 400.0);
    assert_eq!(laid.pages.len(), 1);
    assert!(laid.pages[0].runs.is_empty());
}

/// The same tree twice is the same layout — ruling 4, on the one thing here
/// that could have been ordered by a hash map.
#[test]
fn laying_the_same_tree_out_twice_agrees() {
    let mut style = block();
    style.float = Float::Left;
    style.clear = Clear::Left;
    let tree = BoxNode::element(
        block(),
        vec![
            BoxNode::element(style, vec![text("a")]),
            para("bbbb cccc dddd"),
        ],
    );
    let first = run(&tree, 100.0, 40.0);
    let second = run(&tree, 100.0, 40.0);
    assert_eq!(first.warnings, second.warnings);
    assert_eq!(first.text(), second.text());
    assert_eq!(
        first.pages.iter().map(|p| p.runs.len()).collect::<Vec<_>>(),
        second
            .pages
            .iter()
            .map(|p| p.runs.len())
            .collect::<Vec<_>>()
    );
}

/// A `Content::Text` directly on a block is the same as one wrapped in an
/// inline, which is what keeps the caller from having to build a wrapper.
#[test]
fn text_directly_on_a_block_lays_out() {
    let laid = run(&BoxNode::text(block(), "hello"), 200.0, 400.0);
    assert_eq!(page_text(&laid, 0), "hello");
}

/// A box background reaches the page and covers the lines inside it.
#[test]
fn a_background_covers_the_lines_it_holds() {
    let mut style = block();
    style.background_color = Color {
        r: 9,
        g: 8,
        b: 7,
        a: 255,
    };
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(style, vec![text("aaaa bbbb")])],
    );
    let laid = run(&tree, 60.0, 400.0);
    let fragment = &laid.pages[0].boxes[0];
    assert_eq!(fragment.y, 0.0);
    assert_eq!(fragment.height, 24.0);
    assert_eq!(fragment.background.r, 9);
}

/// `padding` is inside the border box and pushes the content in.
#[test]
fn padding_moves_the_content_and_not_the_box() {
    let mut style = block();
    style.padding = Sides::all(LengthPercentage::Px(10.0));
    style.background_color = Color {
        r: 1,
        g: 1,
        b: 1,
        a: 255,
    };
    let tree = BoxNode::element(block(), vec![BoxNode::element(style, vec![text("a")])]);
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(laid.pages[0].boxes[0].x, 0.0);
    assert_eq!(laid.pages[0].boxes[0].width, 200.0);
    assert_eq!(laid.pages[0].runs[0].x, 10.0);
    assert_eq!(baselines(&laid, 0), vec![19.0]);
}

/// `Sides::get` is exercised on all four edges, so a build that read the top
/// for the bottom fails.
#[test]
fn each_edge_is_its_own() {
    let mut style = block();
    style.margin = Sides {
        top: px(1.0),
        right: px(2.0),
        bottom: px(4.0),
        left: px(8.0),
    };
    let tree = BoxNode::element(
        block(),
        vec![BoxNode::element(style, vec![text("a")]), para("b")],
    );
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(laid.pages[0].runs[0].x, 8.0);
    // 1 of top margin, 9 of ascent; then 12 of line, 4 of bottom margin, 9.
    assert_eq!(baselines(&laid, 0), vec![10.0, 26.0]);
    assert_eq!(
        laid.pages[0].runs[0].width + 8.0,
        10.0 + 8.0,
        "the left margin moved the run and the right margin did not"
    );
    let _ = Side::Right;
    assert!(matches!(Content::Text(String::new()), Content::Text(_)));
}

// ---- CSS 2.2 §17, tables ----------------------------------------------------
//
// Milestone 11. Every number below is arithmetic a reader can check at the
// file's own metrics: a character is as wide as the font is tall, so "abc" at
// ten points is thirty points of max-content and, being one word, thirty of
// min-content too.

fn styled(display: Display) -> ComputedStyle {
    let mut style = base();
    style.display = display;
    style
}

fn table_of(children: Vec<BoxNode>) -> BoxNode {
    BoxNode::element(styled(Display::Table), children)
}

fn row_of(cells: Vec<BoxNode>) -> BoxNode {
    BoxNode::element(styled(Display::TableRow), cells)
}

fn cell_of(body: &str) -> BoxNode {
    BoxNode::element(styled(Display::TableCell), vec![text(body)])
}

fn group_of(display: Display, rows: Vec<BoxNode>) -> BoxNode {
    BoxNode::element(styled(display), rows)
}

/// A whitespace-only anonymous inline box, which is what a producer's newline
/// between two tags becomes.
fn gap() -> BoxNode {
    text("\n  ")
}

/// The `x` of every text run on a page, in the order the runs read.
fn xs(laid: &Layout, page: usize) -> Vec<f64> {
    laid.pages[page].runs.iter().map(|run| run.x).collect()
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-9
}

// ---- §17.2.1, the anonymous table objects ----------------------------------

/// Step 1: a `table-column` box's children generate nothing.
///
/// Two consequences and both are asserted: the step is counted, **and** the
/// text inside the `<col>` does not reach the page. A test for the counter
/// alone would pass on a build that counted it and set the text anyway.
#[test]
fn a_columns_children_generate_no_boxes() {
    let column = BoxNode::element(styled(Display::TableColumn), vec![text("lost")]);
    let tree = table_of(vec![column, row_of(vec![cell_of("kept")])]);
    let generated = table::generate(&tree).generated;
    assert_eq!(generated.count(table::Step::DropColumnChildren), 1);
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(laid.text(), "kept");
}

/// Step 2: a `table-column-group`'s child that is not a `table-column`
/// generates nothing.
///
/// **Not step 1 under another name.** Step 1 is about a column's children and
/// this is about a column group's non-column children, and the fixture keeps
/// them apart by having no `table-column` in it at all — so a build with only
/// step 1 sets the `<div>`'s text.
#[test]
fn a_column_groups_non_column_child_generates_no_box() {
    let group = BoxNode::element(
        styled(Display::TableColumnGroup),
        vec![BoxNode::element(block(), vec![text("lost")])],
    );
    let tree = table_of(vec![group, row_of(vec![cell_of("kept")])]);
    let built = table::generate(&tree);
    assert_eq!(
        built
            .generated
            .count(table::Step::DropNonColumnFromColumnGroup),
        1
    );
    assert_eq!(built.generated.count(table::Step::DropColumnChildren), 0);
    // The second consequence, and the one a counter alone would miss: the
    // `<div>` did not become a **column**. A build that took every child of a
    // column group for a column would give the table a column described by a
    // box that is not one, and §17.5.2.1 would read its `width`.
    assert_eq!(
        built.columns.len(),
        1,
        "the stray child became a column: {:?}",
        built.columns.len()
    );
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(laid.text(), "kept");
}

/// Step 3: the newline a producer writes between `</tr>` and `<tr>` is not a
/// cell.
///
/// The second consequence is the one that matters: without this step the table
/// has an extra anonymous row in it, so the fixture asserts the **number of
/// rows** as well as the counter.
#[test]
fn whitespace_between_two_rows_is_not_a_row_of_its_own() {
    let tree = table_of(vec![
        gap(),
        row_of(vec![cell_of("a")]),
        gap(),
        row_of(vec![cell_of("b")]),
        gap(),
    ]);
    let tree_box = table::generate(&tree);
    assert_eq!(
        tree_box
            .generated
            .count(table::Step::DropWhitespaceInContainer),
        3
    );
    assert_eq!(tree_box.generated.count(table::Step::RowForTableChild), 0);
    assert_eq!(tree_box.visual_rows().len(), 2);
    // **And the *"if any"* half of rule 3**, which is a separate clause and a
    // separate defect: a whitespace-only child with no sibling on either side
    // is removed too. A build that required a neighbour on both sides gives a
    // table of nothing but indentation one empty row, which is a table with a
    // blank line in it.
    let only = table_of(vec![gap()]);
    assert!(table::generate(&only).visual_rows().is_empty());
}

/// Step 4: white space between two internal table siblings, in a parent that
/// is not a table at all.
///
/// Step 3's parent is a tabular container and this one's is the `<div>` step 9
/// is about, which is why they are two rules. A build with only step 3 ends the
/// misparented run at the newline and wraps the two cells in **two** anonymous
/// tables, side by side, which looks like a table.
#[test]
fn whitespace_between_two_misparented_cells_does_not_end_the_run() {
    let children = vec![cell_of("a"), gap(), cell_of("b")];
    assert!(table::is_whitespace_between_table_boxes(&children, 1));
    assert_eq!(table::misparented_run(&children, 0), 3);
    // And it is genuinely about white space: a paragraph there ends the run.
    let broken = vec![cell_of("a"), para("x"), cell_of("b")];
    assert!(!table::is_whitespace_between_table_boxes(&broken, 1));
    assert_eq!(table::misparented_run(&broken, 0), 1);
}

/// Step 5: a table's child that is not a proper table child is wrapped in an
/// anonymous row holding an anonymous cell.
#[test]
fn a_tables_stray_child_gets_an_anonymous_row() {
    let tree = table_of(vec![para("stray"), row_of(vec![cell_of("real")])]);
    let generated = table::generate(&tree).generated;
    assert_eq!(generated.count(table::Step::RowForTableChild), 1);
    assert_eq!(generated.count(table::Step::CellForRowChild), 1);
    let laid = run(&tree, 200.0, 400.0);
    // Two rows, one under the other, and the stray text is not lost.
    assert_eq!(laid.text(), "strayreal");
    // A line box is twelve points and its baseline is one of leading plus
    // eight of ascent.
    assert_eq!(baselines(&laid, 0), vec![9.0, 21.0]);
}

/// Step 6: a row group's child that is not a row is wrapped in an anonymous
/// row.
///
/// **Not step 5.** The parent here is a `<tbody>`, and the fixture has a real
/// row group in it so a build with only step 5 leaves the paragraph unwrapped
/// and loses it.
#[test]
fn a_row_groups_stray_child_gets_an_anonymous_row() {
    let group = group_of(
        Display::TableRowGroup,
        vec![para("stray"), row_of(vec![cell_of("real")])],
    );
    let tree = table_of(vec![group]);
    let generated = table::generate(&tree).generated;
    assert_eq!(generated.count(table::Step::RowForRowGroupChild), 1);
    assert_eq!(generated.count(table::Step::RowForTableChild), 0);
    assert_eq!(run(&tree, 200.0, 400.0).text(), "strayreal");
}

/// Step 7: a row's child that is not a cell is wrapped in an anonymous cell.
///
/// A `<div>` between two `<td>`s is what a producer writes, and without this
/// step it generates no box at all — a lost paragraph, which is the failure
/// text conservation exists to name.
#[test]
fn a_rows_stray_child_gets_an_anonymous_cell() {
    let row = row_of(vec![cell_of("a"), para("stray"), cell_of("b")]);
    let tree = table_of(vec![row]);
    let generated = table::generate(&tree).generated;
    assert_eq!(generated.count(table::Step::CellForRowChild), 1);
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(laid.text(), "astrayb");
    // Three cells side by side, which is what says the stray became a *cell*
    // and not a row.
    assert_eq!(baselines(&laid, 0), vec![9.0, 9.0, 9.0]);
}

/// Step 8: **the `<tbody>` a real book leaves out.**
///
/// XHTML has no tree-construction stage, so a `<table>` of bare `<tr>`s arrives
/// with no row group at all. Consecutive bare rows share **one** anonymous
/// group rather than getting one each, which is the half of this step a build
/// gets wrong without noticing: one group per row lays the table out
/// identically and every `<tfoot>` after it is then in the wrong place.
#[test]
fn a_table_of_bare_rows_gets_the_row_group_the_book_left_out() {
    let tree = table_of(vec![
        row_of(vec![cell_of("a")]),
        row_of(vec![cell_of("b")]),
        row_of(vec![cell_of("c")]),
    ]);
    let generated = table::generate(&tree);
    assert_eq!(generated.generated.count(table::Step::GroupForBareRows), 1);
    assert_eq!(generated.groups.len(), 1);
    assert_eq!(generated.groups[0].rows.len(), 3);
    assert!(generated.groups[0].node.is_none());
}

/// Step 9: an internal table box whose parent is not a table.
///
/// It is [`table::misparented_run`] rather than one of [`table::generate`]'s
/// steps because its input is a block container's child list, and a counter for
/// it inside `Generated` could never be bumped. Both consequences are asserted:
/// the run is found, and the two cells end up **side by side**, which only
/// happens if an anonymous table was generated around them.
#[test]
fn a_stray_cell_outside_a_table_gets_an_anonymous_table() {
    let tree = BoxNode::element(block(), vec![cell_of("a"), cell_of("b"), para("after")]);
    let children = match &tree.content {
        Content::Children(children) => children,
        Content::Text(_) => unreachable!(),
    };
    assert_eq!(table::misparented_run(children, 0), 2);
    let laid = run(&tree, 300.0, 400.0);
    assert_eq!(laid.text(), "abafter");
    let baselines = baselines(&laid, 0);
    assert_eq!(
        baselines[0], baselines[1],
        "the two stray cells are on one row, so an anonymous table wrapped them"
    );
    assert!(
        baselines[2] > baselines[1],
        "and the paragraph after them is not in it"
    );
}

/// The eight steps [`table::generate`] performs are the eight [`table::Step`]
/// names, and a ninth added without a place in the order fails here.
#[test]
fn the_generation_steps_are_a_sequence() {
    assert_eq!(table::Step::ALL.len(), 8);
    let mut sorted = table::Step::ALL.to_vec();
    sorted.sort_unstable();
    assert_eq!(
        sorted.as_slice(),
        table::Step::ALL.as_slice(),
        "the array is not in the order the variants declare"
    );
    let tree = table_of(vec![row_of(vec![cell_of("a")])]);
    assert_eq!(
        table::generate(&tree).generated.fired(),
        vec![table::Step::GroupForBareRows],
        "an ordinary table of bare rows needs exactly one step"
    );
}

// ---- §17.5, the grid, colspan and rowspan ----------------------------------

/// `colspan` takes the slots it says, and the cells after it start past them.
#[test]
fn a_colspan_takes_the_slots_it_says() {
    // A stated width, because an `auto` table shrinks to its content
    // (§17.5.2.2) and five one-character cells would make every column ten
    // points wide — a fixture whose numbers are about the text rather than
    // about the grid.
    let mut style = styled(Display::Table);
    style.width = Size::Length(LengthPercentage::Px(300.0));
    let tree = BoxNode::element(
        style,
        vec![
            row_of(vec![cell_of("a").with_span(2, 1), cell_of("b")]),
            row_of(vec![cell_of("c"), cell_of("d"), cell_of("e")]),
        ],
    );
    let laid = run(&tree, 300.0, 400.0);
    let x = xs(&laid, 0);
    assert_eq!(x.len(), 5);
    // Three equal columns of a hundred points: the spanning cell is at zero and
    // the one after it is at two hundred.
    assert!(close(x[0], 0.0) && close(x[1], 200.0), "{x:?}");
    assert!(
        close(x[2], 0.0) && close(x[3], 100.0) && close(x[4], 200.0),
        "{x:?}"
    );
}

/// `rowspan` occupies the slot below, so the next row's first cell starts one
/// column to the right.
#[test]
fn a_rowspan_pushes_the_next_rows_cells_right() {
    let tree = table_of(vec![
        row_of(vec![cell_of("a").with_span(1, 2), cell_of("b")]),
        row_of(vec![cell_of("c")]),
    ]);
    let laid = run(&tree, 200.0, 400.0);
    let x = xs(&laid, 0);
    assert!(close(x[0], 0.0) && x[1] > x[0], "{x:?}");
    assert!(
        close(x[2], x[1]),
        "the second row's only cell is under `b` and not under `a`: {x:?}"
    );
}

/// `rowspan="0"` is HTML's *"to the end of this row group"*, which is the one
/// place zero is a value and not a mistake.
#[test]
fn a_rowspan_of_zero_reaches_the_end_of_its_row_group() {
    let tree = table_of(vec![group_of(
        Display::TableRowGroup,
        vec![
            row_of(vec![cell_of("a").with_span(1, 0), cell_of("b")]),
            row_of(vec![cell_of("c")]),
            row_of(vec![cell_of("d")]),
        ],
    )]);
    let laid = run(&tree, 200.0, 400.0);
    let x = xs(&laid, 0);
    assert!(
        close(x[2], x[1]) && close(x[3], x[1]) && x[1] > x[0],
        "both later rows are beside the spanning cell: {x:?}"
    );
    assert!(
        laid.warnings.is_empty(),
        "a span to the end of the group is not a clamp: {:?}",
        laid.warnings
    );
}

/// A `rowspan` past the last row of its row group is clamped **to the group**
/// and says so — CSS 2.2 §17.5's own rule, and not "to the table".
#[test]
fn a_rowspan_past_its_row_group_is_clamped_and_says_so() {
    let tree = table_of(vec![
        group_of(
            Display::TableRowGroup,
            vec![row_of(vec![cell_of("a").with_span(1, 4), cell_of("b")])],
        ),
        group_of(Display::TableRowGroup, vec![row_of(vec![cell_of("c")])]),
    ]);
    let laid = run(&tree, 200.0, 400.0);
    assert!(
        laid.warnings
            .iter()
            .any(|(warning, _)| *warning == Warning::RowspanPastTheRowGroup),
        "{:?}",
        laid.warnings
    );
    let x = xs(&laid, 0);
    assert!(
        close(x[2], x[0]),
        "the second group's row starts at the left edge, so the span stopped at \
         its own group: {x:?}"
    );
}

// ---- §17.5.2.2, the automatic algorithm, which is two passes ---------------

/// **The whole of this milestone's width claim, and it is about the algorithm
/// rather than about a table that came out looking plausible.**
///
/// §17.5.2.2 computes a minimum and a maximum content width per column
/// *first*, and distributes the table's width over them *second*. A one-pass
/// approximation that gives each column a share of the available width in
/// proportion to its content is a perfectly ordinary thing to write, and it
/// agrees with this everywhere except where a column's minimum is greater than
/// its proportional share.
///
/// So the fixture is one where they differ: column B is one unbreakable word of
/// fifty points and column A is nine short ones. The one-pass answer gives B
/// twenty-five points — **below its own minimum**, which is a table whose text
/// overflows its cell — and the two-pass answer gives it fifty.
#[test]
fn the_automatic_algorithm_is_two_pass_and_a_one_pass_answer_differs() {
    let cells = [
        table::CellWidths {
            left: 0,
            columns: 1,
            min: 10.0,
            max: 170.0,
            specified: None,
        },
        table::CellWidths {
            left: 1,
            columns: 1,
            min: 50.0,
            max: 50.0,
            specified: None,
        },
    ];
    // The first pass, asserted on its own. It is a value and not an
    // intermediate a debugger could see, which is what makes this an assertion.
    let constraints = table::constraints(2, &cells, &[None, None]);
    assert_eq!(constraints.min, vec![10.0, 50.0]);
    assert_eq!(constraints.max, vec![170.0, 50.0]);
    assert_eq!(constraints.total_min(), 60.0);
    assert_eq!(constraints.total_max(), 220.0);

    // The second pass.
    let widths = table::distribute(&constraints, 110.0);
    assert_eq!(widths, vec![60.0, 50.0]);

    // And the one-pass answer, computed here so the difference is a number in
    // this file rather than a claim in a comment.
    let one_pass: Vec<f64> = cells
        .iter()
        .map(|cell| 110.0 * cell.max / constraints.total_max())
        .collect();
    assert_eq!(one_pass, vec![85.0, 25.0]);
    assert!(
        one_pass[1] < constraints.min[1],
        "the fixture does not distinguish the two algorithms"
    );
    assert_ne!(widths, one_pass);
}

/// And the same table, laid out, so the algorithm reaches the page.
#[test]
fn the_two_pass_widths_are_the_widths_the_cells_get() {
    let mut painted = styled(Display::TableCell);
    painted.background_color = Color {
        r: 1,
        g: 2,
        b: 3,
        a: 255,
    };
    let wide = BoxNode::element(painted.clone(), vec![text("a a a a a a a a a")]);
    let narrow = BoxNode::element(painted, vec![text("bbbbb")]);
    let tree = table_of(vec![row_of(vec![wide, narrow])]);
    let laid = run(&tree, 110.0, 400.0);
    let boxes = &laid.pages[0].boxes;
    assert_eq!(boxes.len(), 2);
    assert!(close(boxes[0].width, 60.0), "{:?}", boxes[0]);
    assert!(close(boxes[1].width, 50.0), "{:?}", boxes[1]);
    assert!(close(boxes[1].x, 60.0), "{:?}", boxes[1]);
}

/// §17.5.2.2 applies a spanning cell **after** every single-column one, and
/// the order is the algorithm rather than a convenience.
///
/// A spanning cell met first pushes its whole minimum into the columns it
/// touches, and the single-column cells that follow cannot take it back — so
/// the table is wider than it needs to be, which looks like a table.
#[test]
fn a_spanning_cell_raises_its_columns_after_the_single_ones() {
    let cells = [
        table::CellWidths {
            left: 0,
            columns: 2,
            min: 100.0,
            max: 100.0,
            specified: None,
        },
        table::CellWidths {
            left: 0,
            columns: 1,
            min: 80.0,
            max: 80.0,
            specified: None,
        },
        table::CellWidths {
            left: 1,
            columns: 1,
            min: 10.0,
            max: 10.0,
            specified: None,
        },
    ];
    let constraints = table::constraints(2, &cells, &[None, None]);
    // The single-column cells already sum to ninety, so the spanning cell needs
    // ten more and not a hundred.
    assert_eq!(constraints.total_min(), 100.0);
    assert!(
        constraints.min[0] > 80.0 && constraints.min[1] > 10.0,
        "the deficit went to both columns: {constraints:?}"
    );
}

/// §17.5.2.2's `auto` width: a table narrower than its containing block takes
/// its own maximum and does not fill the page.
#[test]
fn an_auto_width_table_takes_its_maximum_and_not_the_measure() {
    let constraints = table::Constraints {
        min: vec![10.0, 10.0],
        max: vec![40.0, 40.0],
    };
    assert_eq!(table::automatic_width(&constraints, 400.0, 0.0), 80.0);
    // And one wider than the measure takes the measure, down to its minimum.
    assert_eq!(table::automatic_width(&constraints, 50.0, 0.0), 50.0);
    let wide = table::Constraints {
        min: vec![100.0, 100.0],
        max: vec![400.0, 400.0],
    };
    assert_eq!(
        table::automatic_width(&wide, 50.0, 0.0),
        200.0,
        "a table cannot be narrower than the sum of its minimums"
    );
}

// ---- §17.5.2.1, the fixed algorithm ----------------------------------------

/// §17.5.2.1's own first sentence: **`width: auto` means use the automatic
/// algorithm**, whatever `table-layout` says.
///
/// The two answers differ here by construction: the fixed algorithm would
/// divide two hundred points into two columns of a hundred, and the automatic
/// one gives the wide column its content and the narrow one its own.
#[test]
fn table_layout_fixed_with_an_auto_width_uses_the_automatic_algorithm() {
    let mut style = styled(Display::Table);
    style.table_layout = tinker_pdf_css::property::TableLayout::Fixed;
    let tree = BoxNode::element(
        style,
        vec![row_of(vec![cell_of("a a a a a a a a a"), cell_of("bbbbb")])],
    );
    let laid = run(&tree, 110.0, 400.0);
    let x = xs(&laid, 0);
    // The wide cell wraps into several lines and the narrow one is the last
    // run, at the second column's left edge: sixty under the automatic
    // algorithm and fifty-five under the fixed one's even division.
    assert!(
        close(*x.last().expect("a run"), 60.0),
        "an auto-width table took the fixed algorithm's even columns: {x:?}"
    );
}

/// And with a stated width it is the fixed algorithm, which reads the **first
/// row** and nothing else.
///
/// **The obvious fixture cannot fail**, and the injection matrix is what said
/// so. Putting a stated width on the *same* column in both rows makes the two
/// builds agree by accident: the first row is walked first either way, and
/// §17.5.2.1's *"a column already given a width keeps it"* then discards the
/// second row's. So the second row states a width on a column the first row
/// left `auto`, which is the only arrangement in which the two answers differ —
/// first row only gives the auto column all 160 points that are left, and
/// every row gives it 150 and shares the surplus.
#[test]
fn the_fixed_algorithm_reads_the_first_row_and_ignores_the_rest() {
    let mut style = styled(Display::Table);
    style.table_layout = tinker_pdf_css::property::TableLayout::Fixed;
    style.width = Size::Length(LengthPercentage::Px(200.0));
    let mut first = styled(Display::TableCell);
    first.width = Size::Length(LengthPercentage::Px(40.0));
    let mut second = styled(Display::TableCell);
    second.width = Size::Length(LengthPercentage::Px(150.0));
    let tree = BoxNode::element(
        style,
        vec![
            row_of(vec![BoxNode::element(first, vec![text("a")]), cell_of("b")]),
            row_of(vec![
                cell_of("c"),
                BoxNode::element(second, vec![text("d")]),
            ]),
        ],
    );
    let laid = run(&tree, 300.0, 400.0);
    let x = xs(&laid, 0);
    assert!(
        close(x[1], 40.0),
        "the first row's forty-point cell set the first column: {x:?}"
    );
    assert!(
        close(x[2], 0.0) && close(x[3], 40.0),
        "the second row's hundred-and-fifty-point cell changed nothing: {x:?}"
    );
}

/// A `<col>`'s width beats a first-row cell's, which is the order §17.5.2.1
/// states its three sources in.
#[test]
fn a_columns_width_beats_the_first_rows_cell() {
    let declared = [Some(30.0), None];
    let first = [table::CellWidths {
        left: 0,
        columns: 1,
        min: 0.0,
        max: 0.0,
        specified: Some(90.0),
    }];
    assert_eq!(table::fixed(2, &declared, &first, 100.0), vec![30.0, 70.0]);
    // And with no column box the cell decides, which is what says the
    // assertion above is about the order and not about the number.
    assert_eq!(
        table::fixed(2, &[None, None], &first, 100.0),
        vec![90.0, 10.0]
    );
}

// ---- §17.6.1, the separated border model -----------------------------------

/// `border-spacing` goes between every pair of cells **and** at the four
/// edges, which is the half a build leaves out.
#[test]
fn border_spacing_is_between_the_cells_and_at_the_edges() {
    let mut style = styled(Display::Table);
    style.table_layout = tinker_pdf_css::property::TableLayout::Fixed;
    style.width = Size::Length(LengthPercentage::Px(200.0));
    style.border_spacing = tinker_pdf_css::property::BorderSpacing {
        horizontal: 6.0,
        vertical: 0.0,
    };
    let tree = BoxNode::element(style, vec![row_of(vec![cell_of("a"), cell_of("b")])]);
    let laid = run(&tree, 300.0, 400.0);
    let x = xs(&laid, 0);
    // 200 less three gaps of six is 182, so each column is 91.
    assert!(close(x[0], 6.0), "the left edge has a gap too: {x:?}");
    assert!(close(x[1], 103.0), "{x:?}");
}

/// It takes **two** lengths and they are two directions.
///
/// A build that kept one number is right about every fixture written with one
/// value and wrong about every book that writes two.
#[test]
fn border_spacing_is_two_directions() {
    let mut style = styled(Display::Table);
    style.border_spacing = tinker_pdf_css::property::BorderSpacing {
        horizontal: 0.0,
        vertical: 20.0,
    };
    let tree = BoxNode::element(
        style,
        vec![
            row_of(vec![cell_of("a"), cell_of("b")]),
            row_of(vec![cell_of("c"), cell_of("d")]),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(
        xs(&laid, 0)[1],
        10.0,
        "the horizontal spacing is zero, so the second column starts where the \
         first one's ten points of text end"
    );
    let baselines = baselines(&laid, 0);
    // 20 of spacing, 1 of leading, 8 of ascent; then 12 of line, 20 of
    // spacing, 9.
    assert_eq!(baselines, vec![29.0, 29.0, 61.0, 61.0]);
}

// ---- §17.6.2.1, the five ordered rules -------------------------------------

fn edge(style: BorderStyle, width: f64, origin: table::Origin) -> table::Edge {
    table::Edge {
        style,
        width,
        color: Color::BLACK,
        origin,
    }
}

/// Rule 1: `hidden` beats everything, **including a wider border**.
///
/// It is the only rule that can make a border disappear, and it has to be first
/// for exactly that reason: reached after rule 3 the wide solid border has
/// already won and the author's `border-style: hidden` draws a line.
#[test]
fn a_hidden_border_beats_a_wider_one() {
    let won = table::resolve(
        edge(BorderStyle::Hidden, 1.0, table::Origin::Row),
        edge(BorderStyle::Solid, 8.0, table::Origin::Cell),
    );
    assert_eq!(won.style, BorderStyle::Hidden);
    assert_eq!(won.used_width(), 0.0);
    // Both ways round, because the two arguments are not symmetric anywhere
    // else in this function.
    let other = table::resolve(
        edge(BorderStyle::Solid, 8.0, table::Origin::Cell),
        edge(BorderStyle::Hidden, 1.0, table::Origin::Row),
    );
    assert_eq!(other.style, BorderStyle::Hidden);
}

/// Rule 2: `none` has the lowest priority, **including against a narrower
/// border**.
///
/// Written as its own arm and not left to rule 3, because a `border-style: none`
/// with a stated `border-width` is a real declaration and would win on width.
#[test]
fn a_none_border_loses_to_a_narrower_one() {
    let won = table::resolve(
        edge(BorderStyle::None, 8.0, table::Origin::Cell),
        edge(BorderStyle::Solid, 1.0, table::Origin::Table),
    );
    assert_eq!(won.style, BorderStyle::Solid);
    assert_eq!(won.width, 1.0);
    assert_eq!(
        table::resolve(
            edge(BorderStyle::None, 8.0, table::Origin::Cell),
            edge(BorderStyle::None, 1.0, table::Origin::Table),
        )
        .used_width(),
        0.0
    );
}

/// Rule 3: at different widths the wider wins, whatever the style order and
/// whatever the box says.
#[test]
fn the_wider_border_wins() {
    let won = table::resolve(
        // A `double`, which outranks `solid` at rule 4, and on a cell, which
        // outranks a table at rule 5 — so only rule 3 can decide this.
        edge(BorderStyle::Double, 1.0, table::Origin::Cell),
        edge(BorderStyle::Solid, 4.0, table::Origin::Table),
    );
    assert_eq!(won.style, BorderStyle::Solid);
    assert_eq!(won.width, 4.0);
}

/// Rule 4: at equal widths the style order decides, and it is the
/// specification's order rather than alphabetical or declaration order.
#[test]
fn at_equal_widths_the_style_order_decides() {
    // `double` over `solid`, and the `solid` is on the cell so rule 5 would
    // decide the other way.
    let won = table::resolve(
        edge(BorderStyle::Double, 2.0, table::Origin::Table),
        edge(BorderStyle::Solid, 2.0, table::Origin::Cell),
    );
    assert_eq!(won.style, BorderStyle::Double);
    // And the whole of the order this build's `BorderStyle` can express.
    for (better, worse) in [
        (BorderStyle::Double, BorderStyle::Solid),
        (BorderStyle::Solid, BorderStyle::Dashed),
        (BorderStyle::Dashed, BorderStyle::Dotted),
    ] {
        assert_eq!(
            table::resolve(
                edge(worse, 2.0, table::Origin::Cell),
                edge(better, 2.0, table::Origin::Table),
            )
            .style,
            better,
            "{better:?} does not beat {worse:?}"
        );
    }
}

/// Rule 5: at equal widths and equal styles the box decides, cell first and
/// table last.
#[test]
fn at_equal_widths_and_styles_the_box_decides() {
    let order = [
        table::Origin::Table,
        table::Origin::ColumnGroup,
        table::Origin::Column,
        table::Origin::RowGroup,
        table::Origin::Row,
        table::Origin::Cell,
    ];
    for window in order.windows(2) {
        let (worse, better) = (window[0], window[1]);
        let won = table::resolve(
            edge(BorderStyle::Solid, 2.0, worse),
            edge(BorderStyle::Solid, 2.0, better),
        );
        assert_eq!(won.origin, better, "{better:?} does not beat {worse:?}");
        let other = table::resolve(
            edge(BorderStyle::Solid, 2.0, better),
            edge(BorderStyle::Solid, 2.0, worse),
        );
        assert_eq!(other.origin, better, "and not in the other order either");
    }
}

/// The collapsing model ignores `border-spacing`, §17.6.2's own first
/// sentence.
///
/// It is zeroed at [`crate::style::consume`] rather than at each reader, so a
/// build cannot forget it in one of the four places it is read.
#[test]
fn border_collapse_ignores_border_spacing() {
    let mut style = styled(Display::Table);
    style.border_collapse = tinker_pdf_css::property::BorderCollapse::Collapse;
    style.border_spacing = tinker_pdf_css::property::BorderSpacing {
        horizontal: 30.0,
        vertical: 30.0,
    };
    let tree = BoxNode::element(style, vec![row_of(vec![cell_of("a"), cell_of("b")])]);
    let laid = run(&tree, 200.0, 400.0);
    let x = xs(&laid, 0);
    // With the thirty points honoured the first cell would start at thirty and
    // the second at seventy.
    assert!(close(x[0], 0.0) && close(x[1], 10.0), "{x:?}");
}

/// A collapsed border is half a width on each side of an **inner** grid line
/// and the whole of it at an outer one.
#[test]
fn a_collapsed_border_is_shared_between_the_cells_beside_it() {
    let mut table_style = styled(Display::Table);
    table_style.border_collapse = tinker_pdf_css::property::BorderCollapse::Collapse;
    table_style.table_layout = tinker_pdf_css::property::TableLayout::Fixed;
    table_style.width = Size::Length(LengthPercentage::Px(200.0));
    let mut cell = styled(Display::TableCell);
    cell.border_style = Sides::all(BorderStyle::Solid);
    cell.border_width = Sides::all(4.0);
    let tree = BoxNode::element(
        table_style,
        vec![row_of(vec![
            BoxNode::element(cell.clone(), vec![text("a")]),
            BoxNode::element(cell, vec![text("b")]),
        ])],
    );
    let laid = run(&tree, 300.0, 400.0);
    let x = xs(&laid, 0);
    assert!(
        close(x[0], 4.0),
        "the table's outer edge keeps the whole width: {x:?}"
    );
    assert!(close(x[1], 102.0), "and the shared edge is halved: {x:?}");
}

/// A `hidden` border removes the line it collapsed with, on the page and not
/// only in [`table::resolve`].
///
/// §17.6.2.1's rule 1 is the only rule that can make a border *disappear*, and
/// [`table::Edge::used_width`] is the second half of it: a build that resolved
/// the conflict correctly and then drew the winner's stated width would draw
/// the border the author hid. Two halves, two places, and this is the fixture
/// for the second.
#[test]
fn a_hidden_border_leaves_no_ink_where_it_won() {
    let mut table_style = styled(Display::Table);
    table_style.border_collapse = tinker_pdf_css::property::BorderCollapse::Collapse;
    table_style.table_layout = tinker_pdf_css::property::TableLayout::Fixed;
    table_style.width = Size::Length(LengthPercentage::Px(200.0));
    let mut left = styled(Display::TableCell);
    left.border_style = Sides::all(BorderStyle::Solid);
    left.border_width = Sides::all(8.0);
    let mut right = styled(Display::TableCell);
    right.border_style = Sides::all(BorderStyle::Hidden);
    right.border_width = Sides::all(4.0);
    let tree = BoxNode::element(
        table_style,
        vec![row_of(vec![
            BoxNode::element(left, vec![text("a")]),
            BoxNode::element(right, vec![text("b")]),
        ])],
    );
    let laid = run(&tree, 300.0, 400.0);
    let x = xs(&laid, 0);
    assert!(
        close(x[1], 100.0),
        "the hidden border was drawn at the shared edge: {x:?}"
    );
    assert!(
        close(x[0], 8.0),
        "and the solid border on the table's own edge still is: {x:?}"
    );
}

/// The whole of §17.6.2's point: two adjacent one-point borders are **one**
/// line and not two.
#[test]
fn two_adjacent_borders_collapse_into_one() {
    let mut cell = styled(Display::TableCell);
    cell.border_style = Sides::all(BorderStyle::Solid);
    cell.border_width = Sides::all(2.0);
    let build = |collapse: bool| {
        let mut style = styled(Display::Table);
        style.table_layout = tinker_pdf_css::property::TableLayout::Fixed;
        style.width = Size::Length(LengthPercentage::Px(200.0));
        if collapse {
            style.border_collapse = tinker_pdf_css::property::BorderCollapse::Collapse;
        }
        let tree = BoxNode::element(
            style,
            vec![row_of(vec![
                BoxNode::element(cell.clone(), vec![text("a")]),
                BoxNode::element(cell.clone(), vec![text("b")]),
            ])],
        );
        xs(&run(&tree, 300.0, 400.0), 0)
    };
    let separated = build(false);
    let collapsed = build(true);
    assert!(close(separated[1], 102.0), "{separated:?}");
    assert!(close(collapsed[1], 101.0), "{collapsed:?}");
}

// ---- fragmentation ---------------------------------------------------------

/// **A table breaks between its rows**, which is where §13.3.3 puts a break
/// position and is what a real book's table needs.
#[test]
fn a_table_breaks_between_its_rows() {
    let rows: Vec<BoxNode> = (0..10).map(|_| row_of(vec![cell_of("x")])).collect();
    let tree = table_of(rows);
    // Twelve points a row, so a page of fifty holds four.
    let laid = run(&tree, 200.0, 50.0);
    assert!(laid.pages.len() > 1, "the table did not break at all");
    // **And it breaks where §13.3.3 permits one**, which is the half a page
    // count cannot see: with no break position between the bands the
    // fragmenter drops rules A to D and cuts anyway, producing the same page
    // count and one warning. Asserting the warning is *absent* is the only
    // thing that separates the two builds — milestone 10's finding, in a
    // second place.
    assert!(
        !laid
            .warnings
            .iter()
            .any(|(warning, _)| *warning == Warning::BreakForcedPastTheRules),
        "the table broke where no rule permits it: {:?}",
        laid.warnings
    );
    assert_eq!(laid.text(), "x".repeat(10));
    assert_eq!(
        laid.pages.iter().map(|page| page.runs.len()).sum::<usize>(),
        10,
        "a row was drawn twice or not at all"
    );
}

/// And it does **not** break across a `rowspan`, because there is no break
/// position inside a cell that spans two rows.
#[test]
fn a_rowspan_keeps_its_rows_on_one_page() {
    let tree = table_of(vec![
        row_of(vec![cell_of("a")]),
        row_of(vec![cell_of("b").with_span(1, 2), cell_of("c")]),
        row_of(vec![cell_of("d")]),
        row_of(vec![cell_of("e")]),
    ]);
    // Three rows fit a page of thirty-six points; the band of two must move
    // whole to the next page rather than being cut in half.
    let laid = run(&tree, 200.0, 30.0);
    let pages: Vec<String> = (0..laid.pages.len())
        .map(|at| page_text(&laid, at))
        .collect();
    assert!(
        pages
            .iter()
            .any(|page| page.contains('b') && page.contains('d')),
        "the spanning cell and the row it spans are not on one page: {pages:?}"
    );
    assert_eq!(laid.text(), "abcde");
}

/// A band taller than a page is drawn where it is and **says so**, which is
/// the staged half of table fragmentation named rather than left silent.
#[test]
fn a_row_taller_than_a_page_is_drawn_and_says_so() {
    let tall = cell_of("a b c d e f g h i j k l m n o p q r s t u v w x y z");
    let tree = table_of(vec![row_of(vec![tall])]);
    let laid = run(&tree, 40.0, 30.0);
    assert!(
        laid.warnings
            .iter()
            .any(|(warning, _)| *warning == Warning::TableRowTallerThanPage),
        "{:?}",
        laid.warnings
    );
    assert_eq!(
        conservable(&laid.text()),
        conservable("a b c d e f g h i j k l m n o p q r s t u v w x y z"),
        "and nothing was lost by overflowing"
    );
}

// ---- nesting, and the work cap it multiplies -------------------------------

/// A table inside a cell is laid out inside it.
#[test]
fn a_nested_table_is_laid_out_inside_its_cell() {
    let inner = table_of(vec![row_of(vec![cell_of("i"), cell_of("j")])]);
    let outer = table_of(vec![row_of(vec![
        BoxNode::element(styled(Display::TableCell), vec![inner]),
        cell_of("k"),
    ])]);
    let laid = run(&outer, 200.0, 400.0);
    let x = xs(&laid, 0);
    assert_eq!(laid.text(), "ijk");
    assert!(
        x[0] < x[1] && x[1] < x[2],
        "the inner table's two cells are inside the outer one's first: {x:?}"
    );
    assert!(x[1] < 100.0, "the nested table overflowed its cell: {x:?}");
}

/// **A hostile `colspan` is refused by the layout total**, which is the first
/// of the three places tables charge it.
///
/// A `colspan` is a number in the file and the slots it claims are the work, so
/// five boxes can ask for five and a half million slots — a quantity neither the
/// box cap nor the line-break cap can see, because it is neither a box nor a
/// character.
#[test]
fn a_hostile_colspan_is_refused_by_the_work_total() {
    let rows: Vec<BoxNode> = (0..5)
        .map(|_| row_of(vec![cell_of("x").with_span(1_100_000, 1)]))
        .collect();
    let refusal = layout(
        &table_of(rows),
        &METRICS,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect_err("a table past the layout total");
    assert!(
        matches!(refusal, Refusal::TooMuchLayoutWork { .. }),
        "{refusal:?}"
    );
}

/// **The second of the three places**, and it has its own fixture because a
/// total charged in three places has three reachable halves.
///
/// The grid is rows by columns and neither factor bounds the other. Four
/// thousand one hundred rows whose first one holds a cell spanning as many
/// columns is 8 199 slots to *place* — a long way under the total — and
/// 16 810 000 to *hold*. Delete the grid charge and this book lays out; delete
/// the placement charge and it still refuses.
#[test]
fn a_grid_of_many_rows_and_many_columns_is_refused_by_the_work_total() {
    let mut rows = vec![row_of(vec![cell_of("x").with_span(4_100, 1)])];
    rows.extend((0..4_099).map(|_| row_of(vec![cell_of("y")])));
    let refusal = layout(
        &table_of(rows),
        &METRICS,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect_err("a grid past the layout total");
    assert!(
        matches!(refusal, Refusal::TooMuchLayoutWork { .. }),
        "{refusal:?}"
    );
}

/// **And the third**: §17.5.2.2 spreads every spanning cell over every column
/// it touches, and that is a third quantity again.
///
/// One row of 4 200 000 columns is 4 200 000 slots to place and 4 200 000 to
/// hold — 8 400 000 together, under the total — and 8 400 000 more to
/// distribute. It refuses only with all three charged, which is what makes this
/// the fixture for the third rather than a second copy of the first.
#[test]
fn the_width_distribution_is_charged_as_well_as_the_grid() {
    let refusal = layout(
        &table_of(vec![row_of(vec![cell_of("x").with_span(4_200_000, 1)])]),
        &METRICS,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect_err("a distribution past the layout total");
    assert!(
        matches!(refusal, Refusal::TooMuchLayoutWork { .. }),
        "{refusal:?}"
    );
}

/// **And a nested table multiplies it**, which is why gap 31's bounds table
/// named a nested table beside the two-pass algorithm in the same sentence.
///
/// The two fixtures hold the *same* inner table and differ only in whether it
/// is inside a cell. Alone it is under the total; wrapped, the outer table
/// lays its cell out three times — twice to measure it and once to set it —
/// and the same work is charged three times over.
#[test]
fn a_nested_table_multiplies_the_work_total() {
    let inner = |span: u32| {
        table_of(vec![row_of(vec![
            cell_of("x").with_span(span, 1),
            cell_of("y"),
        ])])
    };
    // 8 000 004 alone and 24 000 016 nested, either side of a cap of
    // 16 000 000: the pair only proves the multiplication while both are true.
    let span = 2_000_000;
    let alone = layout(
        &inner(span),
        &METRICS,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    );
    assert!(
        alone.is_ok(),
        "the inner table alone is already past the total, so the pair proves \
         nothing: {alone:?}"
    );
    let nested = table_of(vec![row_of(vec![BoxNode::element(
        styled(Display::TableCell),
        vec![inner(span)],
    )])]);
    let refusal = layout(
        &nested,
        &METRICS,
        &Options::new(200.0, 400.0),
        &Limits::DEFAULT,
    )
    .expect_err("the same table, nested, is past it");
    assert!(
        matches!(refusal, Refusal::TooMuchLayoutWork { .. }),
        "{refusal:?}"
    );
}

// ---- conservation and reading order ----------------------------------------

/// **Every character of a table survives it**, which is a lost cell's only
/// witness: a table with one column missing renders beautifully.
#[test]
fn every_character_of_a_table_survives_it() {
    let source = table_of(vec![
        BoxNode::element(styled(Display::TableCaption), vec![text("caption")]),
        group_of(
            Display::TableHeaderGroup,
            vec![row_of(vec![cell_of("h1"), cell_of("h2")])],
        ),
        gap(),
        group_of(
            Display::TableRowGroup,
            vec![
                row_of(vec![cell_of("a1"), cell_of("a2")]),
                row_of(vec![
                    cell_of("b1").with_span(2, 1),
                    BoxNode::element(styled(Display::TableCell), vec![para("nested")]),
                ]),
            ],
        ),
    ]);
    let laid = run(&source, 200.0, 400.0);
    assert_eq!(
        conservable(&laid.text()),
        conservable(&source.source_text()),
        "a cell was lost or repeated"
    );
}

/// **A `<tfoot>` written first is read first and drawn last**, which is
/// milestone 10's finding in two dimensions.
///
/// HTML 4.01 required `<tfoot>` before `<tbody>` and real books therefore
/// contain it. §17.2 renders it below, and a build whose reading order is its
/// emission order reports the footer's text in the middle of the table — which
/// fails an *ordered* text conservation on a book that lost nothing at all.
#[test]
fn a_footer_group_written_first_is_read_first_and_drawn_last() {
    let source = table_of(vec![
        group_of(
            Display::TableFooterGroup,
            vec![row_of(vec![cell_of("foot")])],
        ),
        group_of(Display::TableRowGroup, vec![row_of(vec![cell_of("body")])]),
    ]);
    let laid = run(&source, 200.0, 400.0);
    // Read in document order: the footer first.
    assert_eq!(laid.text(), "footbody");
    assert_eq!(
        conservable(&laid.text()),
        conservable(&source.source_text())
    );
    // Drawn in visual order: the footer last. The page's own vector is the ink
    // order and is sorted by the stamp, so the *baselines* are what says which
    // was drawn where.
    let runs = &laid.pages[0].runs;
    let foot = runs
        .iter()
        .find(|run| run.text == "foot")
        .expect("the foot");
    let body = runs
        .iter()
        .find(|run| run.text == "body")
        .expect("the body");
    assert!(
        foot.y > body.y,
        "the footer group was drawn above the body: {} vs {}",
        foot.y,
        body.y
    );
}

/// And it survives a page boundary, which is where a fragmenter loses a row.
#[test]
fn a_tables_text_is_conserved_across_a_page_boundary() {
    let rows: Vec<BoxNode> = (0..20)
        .map(|at| row_of(vec![cell_of(&format!("r{at}")), cell_of("z")]))
        .collect();
    let source = table_of(rows);
    let laid = run(&source, 200.0, 40.0);
    assert!(laid.pages.len() > 4, "{} pages", laid.pages.len());
    assert_eq!(
        conservable(&laid.text()),
        conservable(&source.source_text())
    );
}

// ---- §17.4 and §17.5.3, the rest of the model ------------------------------

/// §17.4: a caption is set above its table.
#[test]
fn a_caption_is_set_above_its_table() {
    let tree = table_of(vec![
        BoxNode::element(styled(Display::TableCaption), vec![text("cap")]),
        row_of(vec![cell_of("a")]),
    ]);
    let laid = run(&tree, 200.0, 400.0);
    assert_eq!(laid.text(), "capa");
    let baselines = baselines(&laid, 0);
    assert!(baselines[0] < baselines[1], "{baselines:?}");
}

/// §17.5.3: a cell's box is its row's height, whatever its own content came
/// to.
///
/// A one-line cell beside a five-line one is painted five lines tall, and a
/// build without the rule draws a table with ragged backgrounds.
#[test]
fn a_cells_background_fills_its_whole_row() {
    let mut painted = styled(Display::TableCell);
    painted.background_color = Color {
        r: 9,
        g: 9,
        b: 9,
        a: 255,
    };
    let tree = table_of(vec![row_of(vec![
        BoxNode::element(painted.clone(), vec![text("one")]),
        BoxNode::element(painted, vec![text("a a a a a a a a a a")]),
    ])]);
    let laid = run(&tree, 100.0, 400.0);
    let boxes = &laid.pages[0].boxes;
    assert_eq!(boxes.len(), 2);
    assert!(
        close(boxes[0].height, boxes[1].height),
        "the short cell is {} tall and the tall one {}",
        boxes[0].height,
        boxes[1].height
    );
    assert!(boxes[0].height > 12.0, "the row is more than one line tall");
}

/// An empty cell still takes its column, so the cells after it are where the
/// grid says.
#[test]
fn an_empty_cell_still_takes_its_column() {
    let empty = BoxNode::element(styled(Display::TableCell), Vec::new());
    // A fixed layout, because that is the algorithm that gives an empty column
    // a width at all: §17.5.2.2 would give it its content's, which is nothing.
    let mut style = styled(Display::Table);
    style.table_layout = tinker_pdf_css::property::TableLayout::Fixed;
    style.width = Size::Length(LengthPercentage::Px(300.0));
    let tree = BoxNode::element(style, vec![row_of(vec![cell_of("a"), empty, cell_of("c")])]);
    let laid = run(&tree, 300.0, 400.0);
    let x = xs(&laid, 0);
    assert_eq!(x.len(), 2);
    assert!(
        close(x[1], 200.0),
        "the third cell is in the third column: {x:?}"
    );
}

/// A row group's background is painted under its rows' and its cells'.
#[test]
fn a_row_group_paints_under_its_cells() {
    let mut group = styled(Display::TableRowGroup);
    group.background_color = Color {
        r: 1,
        g: 1,
        b: 1,
        a: 255,
    };
    let mut cell = styled(Display::TableCell);
    cell.background_color = Color {
        r: 2,
        g: 2,
        b: 2,
        a: 255,
    };
    let tree = table_of(vec![BoxNode::element(
        group,
        vec![row_of(vec![BoxNode::element(cell, vec![text("a")])])],
    )]);
    let laid = run(&tree, 200.0, 400.0);
    let boxes = &laid.pages[0].boxes;
    assert_eq!(boxes.len(), 2);
    assert_eq!(boxes[0].background.r, 1, "the group is not painted first");
    assert_eq!(boxes[1].background.r, 2);
}

// ---- `css-flexbox-1`, milestone 12 -----------------------------------------

/// A flex container at a stated direction and wrap, with everything else at its
/// initial value.
fn flex_container(direction: FlexDirection, wrap: FlexWrap) -> ComputedStyle {
    let mut style = base();
    style.display = Display::Flex;
    style.flex_direction = direction;
    style.flex_wrap = wrap;
    style
}

/// One flex item at a stated `flex` shorthand, holding one word.
fn flex_item(body: &str, grow: f64, shrink: f64, basis: Size) -> BoxNode {
    let mut style = block();
    style.flex_grow = grow;
    style.flex_shrink = shrink;
    style.flex_basis = basis;
    BoxNode::element(style, vec![text(body)])
}

fn basis(value: f64) -> Size {
    Size::Length(LengthPercentage::Px(value))
}

/// The left edge of every run on a page, which is where a row container put its
/// items.
fn lefts(laid: &Layout) -> Vec<f64> {
    xs(laid, 0)
}

/// `display: flex` lays its children out **beside** one another, which is the
/// difference between a flex container and every other block container in this
/// crate.
#[test]
fn a_flex_container_puts_its_items_on_one_line() {
    let tree = BoxNode::element(
        flex_container(FlexDirection::Row, FlexWrap::NoWrap),
        vec![
            flex_item("aa", 0.0, 1.0, Size::Auto),
            flex_item("bb", 0.0, 1.0, Size::Auto),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let x = lefts(&laid);
    let y = baselines(&laid, 0);
    assert_eq!(x.len(), 2, "{x:?}");
    assert!(x[1] > x[0], "the second item is beside the first: {x:?}");
    assert!(close(y[0], y[1]), "and on the same line: {y:?}");
    assert_eq!(laid.text(), "aabb");
}

/// `display: inline-flex` is the same layout inside and a **warning** about the
/// outside, `css-flexbox-1` §3.
///
/// Two assertions and not one: the layout is a flex layout, *and* the fact that
/// this build makes the box block-level is said out loud. A build that laid it
/// out as a flex container and said nothing would be a silent partial
/// implementation of the kind this whole plan exists to prevent.
#[test]
fn inline_flex_lays_out_as_flex_and_says_it_is_block_level() {
    let mut style = flex_container(FlexDirection::Row, FlexWrap::NoWrap);
    style.display = Display::InlineFlex;
    let tree = BoxNode::element(
        style,
        vec![
            flex_item("aa", 0.0, 1.0, Size::Auto),
            flex_item("bb", 0.0, 1.0, Size::Auto),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let x = lefts(&laid);
    assert!(x[1] > x[0], "it is still a flex layout: {x:?}");
    assert_eq!(laid.warnings, vec![(Warning::InlineFlexAsBlock, 1)]);
}

/// `flex-direction: column` stacks the items and `row` sets them side by side,
/// and the same fixture says which is which.
#[test]
fn flex_direction_decides_which_axis_is_the_main_one() {
    let items = || {
        vec![
            flex_item("aa", 0.0, 1.0, Size::Auto),
            flex_item("bb", 0.0, 1.0, Size::Auto),
        ]
    };
    let across = run(
        &BoxNode::element(
            flex_container(FlexDirection::Row, FlexWrap::NoWrap),
            items(),
        ),
        200.0,
        400.0,
    );
    let down = run(
        &BoxNode::element(
            flex_container(FlexDirection::Column, FlexWrap::NoWrap),
            items(),
        ),
        200.0,
        400.0,
    );
    let (ax, ay) = (lefts(&across), baselines(&across, 0));
    let (dx, dy) = (lefts(&down), baselines(&down, 0));
    assert!(ax[1] > ax[0] && close(ay[0], ay[1]), "row: {ax:?} {ay:?}");
    assert!(
        close(dx[0], dx[1]) && dy[1] > dy[0],
        "column: {dx:?} {dy:?}"
    );
}

/// `row-reverse` is not `row` read backwards by the caller: main-start is the
/// **right** edge, so the first item in the document is the rightmost one.
#[test]
fn row_reverse_puts_the_first_item_last() {
    let tree = BoxNode::element(
        flex_container(FlexDirection::RowReverse, FlexWrap::NoWrap),
        vec![
            flex_item("aa", 0.0, 1.0, basis(50.0)),
            flex_item("bb", 0.0, 1.0, basis(50.0)),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let x = lefts(&laid);
    assert!(x[0] > x[1], "the first item is on the right: {x:?}");
    assert!(close(x[0], 150.0) && close(x[1], 100.0), "{x:?}");
    // And the text still reads in document order, which is §5.1's own limit:
    // the boxes move and the words do not.
    assert_eq!(laid.text(), "aabb");
}

/// `flex-wrap: nowrap` overflows and `wrap` starts a second line, over the same
/// three items that do not fit.
#[test]
fn flex_wrap_is_the_difference_between_overflowing_and_a_second_line() {
    let items = || {
        vec![
            flex_item("a", 0.0, 0.0, basis(80.0)),
            flex_item("b", 0.0, 0.0, basis(80.0)),
            flex_item("c", 0.0, 0.0, basis(80.0)),
        ]
    };
    let one = run(
        &BoxNode::element(
            flex_container(FlexDirection::Row, FlexWrap::NoWrap),
            items(),
        ),
        200.0,
        400.0,
    );
    let two = run(
        &BoxNode::element(flex_container(FlexDirection::Row, FlexWrap::Wrap), items()),
        200.0,
        400.0,
    );
    let (ox, oy) = (lefts(&one), baselines(&one, 0));
    let (wx, wy) = (lefts(&two), baselines(&two, 0));
    assert!(
        close(oy[0], oy[1]) && close(oy[1], oy[2]) && close(ox[2], 160.0),
        "nowrap keeps one line and overflows: {ox:?} {oy:?}"
    );
    assert!(
        close(wy[0], wy[1]) && wy[2] > wy[1] && close(wx[2], 0.0),
        "wrap starts a second line: {wx:?} {wy:?}"
    );
}

/// `wrap-reverse` stacks the lines the other way, which is a different fact
/// from wrapping at all.
#[test]
fn wrap_reverse_stacks_the_lines_upwards() {
    let tree = BoxNode::element(
        flex_container(FlexDirection::Row, FlexWrap::WrapReverse),
        vec![
            flex_item("a", 0.0, 0.0, basis(120.0)),
            flex_item("b", 0.0, 0.0, basis(120.0)),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let y = baselines(&laid, 0);
    assert!(y[0] > y[1], "the first line is below the second: {y:?}");
    assert_eq!(laid.text(), "ab");
}

/// `flex-grow` shares the free space out in proportion to the factor.
#[test]
fn flex_grow_shares_the_free_space_in_proportion() {
    let tree = BoxNode::element(
        flex_container(FlexDirection::Row, FlexWrap::NoWrap),
        vec![
            flex_item("a", 1.0, 1.0, basis(0.0)),
            flex_item("b", 3.0, 1.0, basis(0.0)),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let x = lefts(&laid);
    // 200 in the ratio 1:3 is 50 and 150, so the second item starts at 50.
    assert!(close(x[0], 0.0) && close(x[1], 50.0), "{x:?}");
}

/// `flex-shrink` takes the overflow back in proportion to the factor **times
/// the base size**, which is not the same as in proportion to the factor.
///
/// The two answers differ here on purpose: one item of 300 and one of 100 in a
/// container of 200 overflow by 200. Scaled by the base size the shares are
/// 3:1, so the wide one loses 150 and the narrow one 50 — 150 and 50. In
/// proportion to the raw factor they would lose 100 each, which is 200 and 0
/// and puts the narrow item at zero width.
#[test]
fn flex_shrink_is_scaled_by_the_base_size() {
    let tree = BoxNode::element(
        flex_container(FlexDirection::Row, FlexWrap::NoWrap),
        vec![
            flex_item("a", 0.0, 1.0, basis(300.0)),
            flex_item("b", 0.0, 1.0, basis(100.0)),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let x = lefts(&laid);
    assert!(close(x[1], 150.0), "the second item starts at 150: {x:?}");
}

/// `flex-basis` is read **before** `width`, which is the whole point of the
/// property.
#[test]
fn flex_basis_beats_the_width_property() {
    let mut with_width = block();
    with_width.width = Size::Length(LengthPercentage::Px(150.0));
    with_width.flex_basis = basis(50.0);
    let tree = BoxNode::element(
        flex_container(FlexDirection::Row, FlexWrap::NoWrap),
        vec![
            BoxNode::element(with_width, vec![text("a")]),
            flex_item("b", 0.0, 0.0, basis(20.0)),
        ],
    );
    let laid = run(&tree, 400.0, 400.0);
    let x = lefts(&laid);
    assert!(
        close(x[1], 50.0),
        "the first item is 50 wide and not 150: {x:?}"
    );
}

/// `justify-content` moves the items along the **main** axis, which a column of
/// y-offsets cannot see.
#[test]
fn justify_content_puts_the_line_where_it_says() {
    let at = |kind| {
        let mut style = flex_container(FlexDirection::Row, FlexWrap::NoWrap);
        style.justify_content = kind;
        let laid = run(
            &BoxNode::element(
                style,
                vec![
                    flex_item("a", 0.0, 0.0, basis(50.0)),
                    flex_item("b", 0.0, 0.0, basis(50.0)),
                ],
            ),
            200.0,
            400.0,
        );
        lefts(&laid)
    };
    let start = at(JustifyContent::FlexStart);
    assert!(close(start[0], 0.0) && close(start[1], 50.0), "{start:?}");
    let end = at(JustifyContent::FlexEnd);
    assert!(close(end[0], 100.0) && close(end[1], 150.0), "{end:?}");
    let centre = at(JustifyContent::Center);
    assert!(
        close(centre[0], 50.0) && close(centre[1], 100.0),
        "{centre:?}"
    );
    let between = at(JustifyContent::SpaceBetween);
    assert!(
        close(between[0], 0.0) && close(between[1], 150.0),
        "{between:?}"
    );
    let around = at(JustifyContent::SpaceAround);
    assert!(
        close(around[0], 25.0) && close(around[1], 125.0),
        "{around:?}"
    );
    let evenly = at(JustifyContent::SpaceEvenly);
    assert!(
        close(evenly[0], 100.0 / 3.0) && close(evenly[1], 200.0 / 3.0 + 50.0),
        "{evenly:?}"
    );
}

/// `align-items` moves every item along the **cross** axis, and `stretch` moves
/// none of them because it resizes them instead.
#[test]
fn align_items_puts_a_short_item_where_it_says() {
    let at = |kind| {
        let mut style = flex_container(FlexDirection::Row, FlexWrap::NoWrap);
        style.align_items = kind;
        let mut tall = block();
        tall.height = Size::Length(LengthPercentage::Px(60.0));
        let laid = run(
            &BoxNode::element(
                style,
                vec![
                    BoxNode::element(tall, vec![text("a")]),
                    flex_item("b", 0.0, 0.0, basis(50.0)),
                ],
            ),
            200.0,
            400.0,
        );
        baselines(&laid, 0)
    };
    let start = at(AlignItems::FlexStart);
    assert!(close(start[0], start[1]), "both at the top: {start:?}");
    let end = at(AlignItems::FlexEnd);
    assert!(end[1] > end[0], "the short one at the bottom: {end:?}");
    let centre = at(AlignItems::Center);
    let (top, bottom) = (at(AlignItems::FlexStart), at(AlignItems::FlexEnd));
    assert!(
        centre[1] > top[1] && centre[1] < bottom[1],
        "and between the two: {centre:?}"
    );
}

/// `align-self` is one item's answer and overrides the container's, which is a
/// different fact from `align-items` working at all.
#[test]
fn align_self_overrides_the_containers_align_items() {
    let mut style = flex_container(FlexDirection::Row, FlexWrap::NoWrap);
    style.align_items = AlignItems::FlexStart;
    let mut tall = block();
    tall.height = Size::Length(LengthPercentage::Px(60.0));
    let mut moved = block();
    moved.flex_basis = basis(50.0);
    moved.align_self = AlignSelf::FlexEnd;
    let tree = BoxNode::element(
        style,
        vec![
            BoxNode::element(tall, vec![text("a")]),
            BoxNode::element(moved, vec![text("b")]),
            flex_item("c", 0.0, 0.0, basis(50.0)),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let y = baselines(&laid, 0);
    assert!(
        y[1] > y[0] && close(y[2], y[0]),
        "only the item that asked for it moved: {y:?}"
    );
}

/// `align-content` moves the **lines** and has no effect on a single-line
/// container, which is §8.4's own first sentence.
#[test]
fn align_content_moves_the_lines_and_only_when_there_are_two() {
    let at = |kind, wrap| {
        let mut style = flex_container(FlexDirection::Row, wrap);
        style.align_content = kind;
        style.height = Size::Length(LengthPercentage::Px(200.0));
        let laid = run(
            &BoxNode::element(
                style,
                vec![
                    flex_item("a", 0.0, 0.0, basis(120.0)),
                    flex_item("b", 0.0, 0.0, basis(120.0)),
                ],
            ),
            200.0,
            400.0,
        );
        baselines(&laid, 0)
    };
    let start = at(AlignContent::FlexStart, FlexWrap::Wrap);
    let end = at(AlignContent::FlexEnd, FlexWrap::Wrap);
    assert!(
        end[0] > start[0],
        "two lines move to the bottom: {start:?} {end:?}"
    );
    // And the same declaration over one line moves nothing at all.
    let one_start = at(AlignContent::FlexStart, FlexWrap::NoWrap);
    let one_end = at(AlignContent::FlexEnd, FlexWrap::NoWrap);
    assert_eq!(
        one_start, one_end,
        "align-content has no effect on a single-line container"
    );
}

/// `order` moves the boxes and does **not** move the words, which is §5.4's own
/// note and the reason text conservation survives it.
#[test]
fn order_moves_the_boxes_and_not_the_reading_order() {
    let mut second = block();
    second.flex_basis = basis(50.0);
    second.order = -1;
    let tree = BoxNode::element(
        flex_container(FlexDirection::Row, FlexWrap::NoWrap),
        vec![
            flex_item("aa", 0.0, 0.0, basis(50.0)),
            BoxNode::element(second, vec![text("bb")]),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let runs = &laid.pages[0].runs;
    let placed: Vec<(f64, &str)> = runs.iter().map(|r| (r.x, r.text.as_str())).collect();
    assert!(
        placed.iter().find(|(_, t)| *t == "bb").expect("bb").0 < 1e-9,
        "the ordered item is first on the line: {placed:?}"
    );
    assert_eq!(
        laid.text(),
        "aabb",
        "and the words still read in document order"
    );
}

/// §9.7's loop, rather than one division: an item that cannot shrink past its
/// content gives the space back and the **other** item absorbs it.
#[test]
fn the_flexible_length_resolution_redistributes_after_a_minimum_bites() {
    let items = [
        flex::Item {
            grow: 0.0,
            shrink: 1.0,
            base: 100.0,
            hypothetical: 100.0,
            min: 90.0,
            extra: 0.0,
        },
        flex::Item {
            grow: 0.0,
            shrink: 1.0,
            base: 100.0,
            hypothetical: 100.0,
            min: 0.0,
            extra: 0.0,
        },
    ];
    let used = flex::resolve(&items, 100.0);
    // One pass would give 50 and 50, then clamp the first to 90 and stop --
    // 140 in a container of 100. The loop freezes the first at 90 and gives the
    // rest of the deficit to the second.
    assert!(close(used[0], 90.0) && close(used[1], 10.0), "{used:?}");
}

/// §9.7 step 4b's `< 1` clause: flex factors summing to less than one
/// distribute only that fraction of the free space.
#[test]
fn flex_factors_below_one_leave_the_rest_of_the_space_empty() {
    let items = [flex::Item {
        grow: 0.5,
        shrink: 1.0,
        base: 0.0,
        hypothetical: 0.0,
        min: 0.0,
        extra: 0.0,
    }];
    let used = flex::resolve(&items, 200.0);
    assert!(
        close(used[0], 100.0),
        "half the room and not all of it: {used:?}"
    );
}

/// §9.3's own clause: a line takes at least one item however wide it is.
#[test]
fn a_flex_line_always_takes_one_item() {
    let items = [
        flex::Item {
            grow: 0.0,
            shrink: 0.0,
            base: 500.0,
            hypothetical: 500.0,
            min: 0.0,
            extra: 0.0,
        },
        flex::Item {
            grow: 0.0,
            shrink: 0.0,
            base: 500.0,
            hypothetical: 500.0,
            min: 0.0,
            extra: 0.0,
        },
    ];
    assert_eq!(
        flex::lines(&items, 100.0, FlexWrap::Wrap),
        vec![(0, 1), (1, 2)]
    );
    assert_eq!(
        flex::lines(&items, 100.0, FlexWrap::NoWrap),
        vec![(0, 2)],
        "and `nowrap` is one line whatever it costs"
    );
}

/// §5.4's stability: two items in the same ordinal group keep document order.
#[test]
fn the_order_sort_is_stable() {
    assert_eq!(flex::ordered(&[2, 1, 2, 1]), vec![1, 3, 0, 2]);
}

/// A flex item's text is conserved through wrapping, ordering and alignment,
/// which is the invariant a box-moving milestone is most likely to break.
#[test]
fn a_flex_container_conserves_its_text() {
    let mut style = flex_container(FlexDirection::RowReverse, FlexWrap::Wrap);
    style.justify_content = JustifyContent::SpaceBetween;
    style.align_items = AlignItems::Center;
    let mut ordered = block();
    ordered.order = -2;
    ordered.flex_basis = basis(90.0);
    let tree = BoxNode::element(
        style,
        vec![
            flex_item("one", 0.0, 0.0, basis(90.0)),
            BoxNode::element(ordered, vec![text("two")]),
            flex_item("three", 0.0, 0.0, basis(90.0)),
            flex_item("four", 0.0, 0.0, basis(90.0)),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    // Each item is its own block, so the words meet with nothing between them:
    // what is asserted is that every character survived reversing the
    // direction, wrapping the line, reordering one item and centring them all.
    assert_eq!(laid.text(), "onetwothreefour");
}

/// A flex container taller than a page is drawn where it is and **says so**,
/// which is the same staged half a table row has.
#[test]
fn a_flex_line_taller_than_a_page_is_named() {
    let tree = BoxNode::element(
        flex_container(FlexDirection::Column, FlexWrap::NoWrap),
        vec![
            para("a"),
            para("b"),
            para("c"),
            para("d"),
            para("e"),
            para("f"),
        ],
    );
    let laid = run(&tree, 200.0, 30.0);
    assert!(
        laid.warnings
            .contains(&(Warning::FlexLineTallerThanPage, 1)),
        "{:?}",
        laid.warnings
    );
    assert_eq!(laid.text(), "abcdef");
}

/// A row container of several lines **can** be broken between two of them,
/// which is the other half of the sentence above.
#[test]
fn a_row_container_breaks_between_its_lines() {
    let tree = BoxNode::element(
        flex_container(FlexDirection::Row, FlexWrap::Wrap),
        vec![
            flex_item("a", 0.0, 0.0, basis(120.0)),
            flex_item("b", 0.0, 0.0, basis(120.0)),
            flex_item("c", 0.0, 0.0, basis(120.0)),
        ],
    );
    let laid = run(&tree, 200.0, 26.0);
    assert!(laid.pages.len() > 1, "one page holds every line");
    assert!(
        !laid
            .warnings
            .iter()
            .any(|(w, _)| *w == Warning::FlexLineTallerThanPage),
        "a line that fits is not a line that overflows: {:?}",
        laid.warnings
    );
    assert_eq!(laid.text(), "abc");
}

/// §4: a run of child text becomes an anonymous flex item, and a run that is
/// all white space does not.
#[test]
fn a_run_of_child_text_is_an_anonymous_flex_item() {
    let tree = BoxNode::element(
        flex_container(FlexDirection::Row, FlexWrap::NoWrap),
        vec![text("aa"), flex_item("bb", 0.0, 0.0, basis(50.0))],
    );
    let laid = run(&tree, 200.0, 400.0);
    let x = lefts(&laid);
    assert_eq!(x.len(), 2, "the bare text got a box of its own: {x:?}");
    assert!(x[1] > x[0], "{x:?}");
    assert_eq!(laid.text(), "aabb");

    // And the white space every producer writes between two elements does not
    // become a third item.
    let spaced = BoxNode::element(
        flex_container(FlexDirection::Row, FlexWrap::NoWrap),
        vec![
            flex_item("aa", 0.0, 0.0, basis(50.0)),
            text("\n  "),
            flex_item("bb", 0.0, 0.0, basis(50.0)),
        ],
    );
    let laid = run(&spaced, 200.0, 400.0);
    assert_eq!(lefts(&laid).len(), 2, "the white space is not an item");
}

/// §3: `float` does not apply to a flex item, and the failure it prevents is a
/// figure taken out of the line and put against the container's edge.
#[test]
fn a_float_declaration_on_a_flex_item_does_nothing() {
    let mut floated = block();
    floated.float = Float::Right;
    floated.flex_basis = basis(50.0);
    let tree = BoxNode::element(
        flex_container(FlexDirection::Row, FlexWrap::NoWrap),
        vec![
            flex_item("a", 0.0, 0.0, basis(50.0)),
            BoxNode::element(floated, vec![text("b")]),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let x = lefts(&laid);
    assert!(
        close(x[1], 50.0),
        "the item is where §9 put it and not where §9.5 would: {x:?}"
    );
}

/// §9.4 step 11: `stretch` makes an item as tall as its line, which is a size
/// and not a position — so it is the item's painted box that says so.
#[test]
fn stretch_makes_an_item_as_tall_as_its_line() {
    let mut style = flex_container(FlexDirection::Row, FlexWrap::NoWrap);
    style.align_items = AlignItems::Stretch;
    let mut tall = block();
    tall.height = Size::Length(LengthPercentage::Px(60.0));
    let mut short = block();
    short.flex_basis = basis(50.0);
    short.background_color = Color {
        r: 9,
        g: 9,
        b: 9,
        a: 255,
    };
    let tree = BoxNode::element(
        style,
        vec![
            BoxNode::element(tall, vec![text("a")]),
            BoxNode::element(short, vec![text("b")]),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let painted = laid.pages[0]
        .boxes
        .iter()
        .find(|b| b.background.r == 9)
        .expect("the short item is painted");
    assert!(
        close(painted.height, 60.0),
        "the short item's box is the line's height: {painted:?}"
    );
}

// ---- what the injection matrix asked for -----------------------------------

/// `css-align-3` §9.3's **fallback alignment**: with negative free space,
/// `space-between` behaves as `flex-start` and the other two as `center`.
///
/// The matrix asked for this. Every `justify-content` fixture above has room to
/// spare, and a build with no fallback pulls an overflowing line apart
/// *backwards* — `free / (count - 1)` is negative, so the second item is drawn
/// to the **left** of the first and the line reads in reverse.
#[test]
fn an_overflowing_line_falls_back_to_a_different_alignment() {
    let at = |kind| {
        let mut style = flex_container(FlexDirection::Row, FlexWrap::NoWrap);
        style.justify_content = kind;
        let laid = run(
            &BoxNode::element(
                style,
                vec![
                    flex_item("a", 0.0, 0.0, basis(150.0)),
                    flex_item("b", 0.0, 0.0, basis(150.0)),
                ],
            ),
            200.0,
            400.0,
        );
        lefts(&laid)
    };
    let between = at(JustifyContent::SpaceBetween);
    assert!(
        close(between[0], 0.0) && close(between[1], 150.0),
        "space-between falls back to flex-start: {between:?}"
    );
    let around = at(JustifyContent::SpaceAround);
    assert!(
        close(around[0], -50.0) && close(around[1], 100.0),
        "and space-around to center, which overflows both ends equally: \
         {around:?}"
    );
    let evenly = at(JustifyContent::SpaceEvenly);
    assert_eq!(around, evenly, "space-evenly falls back the same way");
}

/// `css-flexbox-1` §8.3's `baseline`: the items' **first baselines** are
/// aligned, which is not the same as their tops and not the same as their
/// centres.
///
/// The matrix asked for this too. Two items whose first lines sit at different
/// heights inside their own boxes — one has a padding above its text — are
/// aligned so that the two lines share a baseline, which moves the item with
/// *less* padding down.
#[test]
fn align_items_baseline_lines_the_text_up() {
    let mut style = flex_container(FlexDirection::Row, FlexWrap::NoWrap);
    style.align_items = AlignItems::Baseline;
    let mut padded = block();
    padded.flex_basis = basis(60.0);
    padded.padding = Sides::all(LengthPercentage::Px(0.0));
    padded.padding.top = LengthPercentage::Px(20.0);
    let tree = BoxNode::element(
        style,
        vec![
            BoxNode::element(padded, vec![text("a")]),
            flex_item("b", 0.0, 0.0, basis(60.0)),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    let y = baselines(&laid, 0);
    assert!(
        close(y[0], y[1]),
        "the two first baselines are the same line: {y:?}"
    );
    // **And where** the shared line is, which the assertion above cannot see:
    // the line's baseline is the **largest** of the items' own, so it is the
    // padded item's 29 and not the unpadded item's 9. The injection matrix
    // found this -- a build that took the smallest, or zero, aligns the two
    // items with each other perfectly and draws them both above the line box.
    assert!(
        close(y[0], 29.0),
        "the line's baseline is the deepest item's: {y:?}"
    );
    // And the control: the same document aligned to the top puts them 20 apart,
    // which is what says the assertion above is about §8.3 and not about two
    // boxes that happened to agree.
    let mut top = flex_container(FlexDirection::Row, FlexWrap::NoWrap);
    top.align_items = AlignItems::FlexStart;
    let mut padded = block();
    padded.flex_basis = basis(60.0);
    padded.padding = Sides::all(LengthPercentage::Px(0.0));
    padded.padding.top = LengthPercentage::Px(20.0);
    let laid = run(
        &BoxNode::element(
            top,
            vec![
                BoxNode::element(padded, vec![text("a")]),
                flex_item("b", 0.0, 0.0, basis(60.0)),
            ],
        ),
        200.0,
        400.0,
    );
    let y = baselines(&laid, 0);
    assert!(close(y[0] - y[1], 20.0), "{y:?}");
}

/// §8.4's initial value is `stretch` and it makes the **lines** taller, which
/// then moves every item inside them.
///
/// The matrix asked for this. Every `align-content` fixture above states a
/// value other than the initial one, so a build that mapped `stretch` onto
/// `flex-start` — which moves no line at all — passed every one of them.
#[test]
fn align_content_stretch_makes_the_lines_taller() {
    let container = |kind| {
        let mut style = flex_container(FlexDirection::Row, FlexWrap::Wrap);
        style.align_content = kind;
        style.align_items = AlignItems::FlexEnd;
        style.height = Size::Length(LengthPercentage::Px(200.0));
        let laid = run(
            &BoxNode::element(
                style,
                vec![
                    flex_item("a", 0.0, 0.0, basis(120.0)),
                    flex_item("b", 0.0, 0.0, basis(120.0)),
                ],
            ),
            200.0,
            400.0,
        );
        baselines(&laid, 0)
    };
    // Two lines of one item each, in a container of 200. Stretched, each line
    // is 100 tall and the items sit at their bottoms; unstretched, each line is
    // its item's height and both are at the top of the container.
    let stretched = container(AlignContent::Stretch);
    let packed = container(AlignContent::FlexStart);
    assert!(
        stretched[0] > packed[0] && stretched[1] > packed[1],
        "stretching the lines moved both items down: {stretched:?} against \
         {packed:?}"
    );
    assert!(
        stretched[1] - stretched[0] > packed[1] - packed[0],
        "and moved them apart: {stretched:?}"
    );
}

/// `box-sizing: border-box` measures `flex-basis` from the **border box**, and
/// every size in §9 is a content one.
///
/// The matrix asked for this. A build that skipped the conversion makes a
/// `flex-basis: 100px` item with 10 points of padding either side 120 wide
/// instead of 100, which is a row that overflows by the padding of every item
/// in it.
#[test]
fn box_sizing_border_box_shrinks_a_flex_basis_by_its_padding() {
    let at = |sizing| {
        let mut item = block();
        item.flex_basis = basis(100.0);
        item.flex_grow = 0.0;
        item.flex_shrink = 0.0;
        item.box_sizing = sizing;
        item.padding = Sides::all(LengthPercentage::Px(10.0));
        let laid = run(
            &BoxNode::element(
                flex_container(FlexDirection::Row, FlexWrap::NoWrap),
                vec![
                    BoxNode::element(item, vec![text("a")]),
                    flex_item("b", 0.0, 0.0, basis(50.0)),
                ],
            ),
            300.0,
            400.0,
        );
        lefts(&laid)
    };
    let content = at(BoxSizing::ContentBox);
    assert!(
        close(content[1], 120.0),
        "a content-box basis of 100 with 10 of padding is 120 wide: {content:?}"
    );
    let border = at(BoxSizing::BorderBox);
    assert!(
        close(border[1], 100.0),
        "and a border-box one is 100: {border:?}"
    );
}

/// §4.5: the automatic minimum main size is *"further clamped by"* the item's
/// own stated size.
///
/// The matrix asked for this. Without the clamp a `width: 40px` item holding
/// one unbreakable eighty-point word cannot be made narrower than the word, so
/// the row overflows — and the author's declaration says it should not.
#[test]
fn the_automatic_minimum_is_clamped_by_a_stated_size() {
    let item = |width: Option<f64>| {
        let mut style = block();
        style.flex_shrink = 1.0;
        style.flex_grow = 0.0;
        if let Some(width) = width {
            style.width = Size::Length(LengthPercentage::Px(width));
        }
        BoxNode::element(style, vec![text("wwwwwwww")])
    };
    let laid = run(
        &BoxNode::element(
            flex_container(FlexDirection::Row, FlexWrap::NoWrap),
            vec![item(Some(40.0)), flex_item("b", 0.0, 0.0, basis(100.0))],
        ),
        100.0,
        400.0,
    );
    let x = lefts(&laid);
    assert!(
        x[1] <= 40.0 + 1e-9,
        "the stated width is the floor and the eighty-point word is not: {x:?}"
    );
    // The control: the same item with no stated width cannot go below its own
    // longest word, which is what says the assertion above is about the clamp.
    let laid = run(
        &BoxNode::element(
            flex_container(FlexDirection::Row, FlexWrap::NoWrap),
            vec![item(None), flex_item("b", 0.0, 0.0, basis(100.0))],
        ),
        100.0,
        400.0,
    );
    let x = lefts(&laid);
    assert!(close(x[1], 80.0), "{x:?}");
}

/// §9.2 step 4: the hypothetical main size is the base size **clamped by the
/// used minimum**, and §9.3 collects the lines from those sizes.
///
/// The matrix asked for this. A build that used the raw base size puts two
/// `flex-basis: 0` items on one line however long their words are, and then
/// discovers at §9.7 that neither can be made that narrow — a line that
/// overflows where the specification wraps.
#[test]
fn the_hypothetical_size_decides_where_a_line_wraps() {
    let item = |body: &str| {
        let mut style = block();
        style.flex_basis = basis(0.0);
        style.flex_grow = 0.0;
        style.flex_shrink = 0.0;
        BoxNode::element(style, vec![text(body)])
    };
    let laid = run(
        &BoxNode::element(
            flex_container(FlexDirection::Row, FlexWrap::Wrap),
            vec![item("wwwwwwwwww"), item("wwwwwwwwww")],
        ),
        150.0,
        400.0,
    );
    let y = baselines(&laid, 0);
    assert!(
        y[1] > y[0],
        "two hundred-point words do not share a hundred-and-fifty-point line: \
         {y:?}"
    );
}

/// §4's white-space exception, asserted where it is **observable**.
///
/// The matrix asked for this: counting the runs cannot see the difference,
/// because an anonymous item holding one newline collapses to no text and
/// therefore to no run. `space-around` can: it divides the free space by the
/// number of items, so a phantom third item moves the two real ones.
#[test]
fn a_whitespace_run_is_not_an_item_and_the_spacing_says_so() {
    let container = |children| {
        let mut style = flex_container(FlexDirection::Row, FlexWrap::NoWrap);
        style.justify_content = JustifyContent::SpaceAround;
        let laid = run(&BoxNode::element(style, children), 200.0, 400.0);
        lefts(&laid)
    };
    let spaced = container(vec![
        flex_item("a", 0.0, 0.0, basis(50.0)),
        text("\n  "),
        flex_item("b", 0.0, 0.0, basis(50.0)),
    ]);
    // Two items in 200 with 100 to spare: a quarter share at each end and a
    // half share between, so 25 and 125. A third, empty, item would make the
    // shares thirds and put them at 16.67 and 133.33.
    assert!(
        close(spaced[0], 25.0) && close(spaced[1], 125.0),
        "{spaced:?}"
    );
    let bare = container(vec![
        flex_item("a", 0.0, 0.0, basis(50.0)),
        flex_item("b", 0.0, 0.0, basis(50.0)),
    ]);
    assert_eq!(spaced, bare, "the white space changed nothing at all");
}

/// §4's blockification, at the one arm of it this build can observe: an
/// `inline-flex` **item** is a flex container whose outside §4 has already made
/// block-level, so it does not raise the warning about being laid out as one.
///
/// The matrix asked for this. The `inline` and `inline-block` arms are
/// unobservable here and are recorded as such where they are written: this
/// build's [`crate::flow`] reads `display` to ask four questions and an inline
/// item answers all four the way a block one does.
#[test]
fn an_inline_flex_item_is_blockified_and_does_not_warn() {
    let mut inner = flex_container(FlexDirection::Row, FlexWrap::NoWrap);
    inner.display = Display::InlineFlex;
    inner.flex_basis = basis(100.0);
    let tree = BoxNode::element(
        flex_container(FlexDirection::Row, FlexWrap::NoWrap),
        vec![
            BoxNode::element(inner, vec![flex_item("a", 0.0, 0.0, basis(40.0))]),
            flex_item("b", 0.0, 0.0, basis(50.0)),
        ],
    );
    let laid = run(&tree, 200.0, 400.0);
    assert!(
        laid.warnings.is_empty(),
        "an inline-flex item is already block-level: {:?}",
        laid.warnings
    );
    assert_eq!(laid.text(), "ab");
}

/// §9.7 step 2's freeze is **not** an optimisation, and step 4b's
/// factors-below-one clause is what makes the difference visible.
///
/// The injection matrix asked for this twice. Deleting step 2's freeze leaves
/// the answers unchanged in every ordinary arrangement, because step 4's loop
/// clamps by the same minimum and reaches the same fixed point — so the first
/// pass of the matrix reported both halves as survivors and the argument for
/// calling them equivalent mutants was already written down. It is wrong.
/// `initial_free` is computed **once**, in step 3, out of the frozen/unfrozen
/// split as it stood, and step 4b multiplies *that* number by the flex factors
/// when they sum to less than one. An item frozen in step 2 contributes its
/// hypothetical size to it and an unfrozen one contributes its base size, and
/// those differ exactly when a minimum bit.
#[test]
fn step_two_freezes_before_step_three_measures_the_free_space() {
    // Growing. A `flex: 0 0 0` item whose content is fifty wide: its base size
    // is zero, its automatic minimum is fifty, and step 2 freezes it at fifty
    // because its factor is zero. The half-factor item beside it then grows by
    // half of what is left of the *hundred and fifty*, not by half of the two
    // hundred an unfrozen first item would have left.
    let grown = flex::resolve(
        &[
            flex::Item {
                grow: 0.0,
                shrink: 0.0,
                base: 0.0,
                hypothetical: 50.0,
                min: 50.0,
                extra: 0.0,
            },
            flex::Item {
                grow: 0.5,
                shrink: 1.0,
                base: 0.0,
                hypothetical: 0.0,
                min: 0.0,
                extra: 0.0,
            },
        ],
        200.0,
    );
    assert!(
        close(grown[0], 50.0) && close(grown[1], 75.0),
        "half of a hundred and fifty, not half of two hundred: {grown:?}"
    );

    // Shrinking, and the other half of step 2: an item whose base size is
    // already **below** its minimum is on the wrong side of its hypothetical
    // size and takes no part in the sums.
    let shrunk = flex::resolve(
        &[
            flex::Item {
                grow: 0.0,
                shrink: 0.25,
                base: 50.0,
                hypothetical: 90.0,
                min: 90.0,
                extra: 0.0,
            },
            flex::Item {
                grow: 0.0,
                shrink: 0.25,
                base: 100.0,
                hypothetical: 100.0,
                min: 0.0,
                extra: 0.0,
            },
        ],
        100.0,
    );
    assert!(
        close(shrunk[0], 90.0) && close(shrunk[1], 77.5),
        "a quarter of the ninety-point deficit, not of the fifty-point one: \
         {shrunk:?}"
    );
}
