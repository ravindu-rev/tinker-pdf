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
    BorderStyle, BoxSizing, Clear, Color, Display, Float, LengthPercentage, LineHeight,
    ListStyleType, MarginValue, OverflowWrap, PageBreak, PageBreakInside, Side, Sides, Size,
    TextAlign, Visibility, WhiteSpace,
};

use crate::flow::marker_text;
use crate::metrics::FixedPitch;
use crate::{layout, BoxNode, Content, Layout, Limits, Options, Refusal, Warning};

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

// ---- warnings ---------------------------------------------------------------

/// `float` and `clear` are named rather than laid out, and are **not** mapped
/// onto their nearest implemented neighbour.
#[test]
fn an_unimplemented_property_is_named_rather_than_approximated() {
    let mut style = block();
    style.float = Float::Left;
    style.clear = Clear::Both;
    let tree = BoxNode::element(block(), vec![BoxNode::element(style, vec![text("a")])]);
    let laid = run(&tree, 200.0, 400.0);
    assert!(laid
        .warnings
        .iter()
        .any(|(w, _)| *w == Warning::FloatInFlow(Float::Left)));
    assert!(laid
        .warnings
        .iter()
        .any(|(w, _)| *w == Warning::ClearIgnored(Clear::Both)));
}

/// A warning is counted, not repeated — ruling 10's shape and
/// `tinker_pdf_css::parser::Report`'s.
#[test]
fn warnings_are_deduplicated_with_a_count() {
    let mut style = block();
    style.float = Float::Left;
    let children: Vec<BoxNode> = (0..5)
        .map(|_| BoxNode::element(style.clone(), vec![text("a")]))
        .collect();
    let laid = run(&BoxNode::element(block(), children), 200.0, 400.0);
    let entry = laid
        .warnings
        .iter()
        .find(|(w, _)| *w == Warning::FloatInFlow(Float::Left))
        .expect("the float was named");
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
