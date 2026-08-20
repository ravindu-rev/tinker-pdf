//! `css-cascade-5` §6.1's sorting order, a fixture per criterion, and §7.2's
//! inheritance against the quadratic alternative.

use super::{sheet, tree, Node};
use crate::cascade::{cascade, rank, resolve_lazily, ComputedStyle, Origin};
use crate::property::{
    Color, Display, Float, FontStyle, LengthPercentage, LineHeight, MarginValue, Side, Size,
    TextAlign, Visibility,
};
use crate::{Budget, Limits, Refusal, Stylesheet};

/// Cascades one author sheet over a tree.
pub fn styles(source: &str, nodes: &[Node]) -> Vec<ComputedStyle> {
    let parsed = sheet(source);
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    cascade(&[(Origin::Author, &parsed)], nodes, &limits, &mut budget)
        .expect("the fixture is under every cap")
        .styles
}

/// Cascades several sheets, each at its own origin.
fn styles_from(sheets: &[(Origin, &Stylesheet)], nodes: &[Node]) -> Vec<ComputedStyle> {
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    cascade(sheets, nodes, &limits, &mut budget)
        .expect("the fixture is under every cap")
        .styles
}

fn one(name: &str) -> Vec<Node> {
    tree(&[(name, None)])
}

fn red() -> Color {
    Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    }
}

fn blue() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 255,
        a: 255,
    }
}

/// §6.1 criterion 1, the ordinary half: author beats user beats UA.
#[test]
fn criterion_one_normal_origins_run_author_user_ua() {
    let ua = sheet("p { color: red }");
    let user = sheet("p { color: blue }");
    let author = sheet("p { color: #00ff00 }");
    let nodes = one("p");
    let green = Color {
        r: 0,
        g: 255,
        b: 0,
        a: 255,
    };
    assert_eq!(
        styles_from(
            &[
                (Origin::UserAgent, &ua),
                (Origin::User, &user),
                (Origin::Author, &author)
            ],
            &nodes
        )[0]
        .color,
        green
    );
    // And with the author's removed, the user's wins over the UA's — which is
    // the half a test that only checked the author would not reach.
    assert_eq!(
        styles_from(&[(Origin::UserAgent, &ua), (Origin::User, &user)], &nodes)[0].color,
        blue()
    );
    // The sheets are given in a deliberately unhelpful order here: origin
    // decides, not the order the caller handed them over in.
    assert_eq!(
        styles_from(
            &[(Origin::Author, &author), (Origin::UserAgent, &ua)],
            &nodes
        )[0]
        .color,
        green
    );
}

/// §6.1 criterion 1, **the reversal**: an `!important` author rule **loses** to
/// an `!important` UA rule.
///
/// This is the clause a first implementation drops, and it is invisible in
/// every ordinary book: the normal case runs author-over-UA and the important
/// case runs UA-over-author, so a build that treated `!important` as extra
/// weight on top of the normal order is right everywhere except here. It is
/// asserted from **both** directions — the important UA winning, and the
/// important author winning once the UA's `!important` is taken away — because
/// one of those alone is also what a build that always prefers the UA does.
#[test]
fn criterion_one_important_reverses_the_origin_order() {
    let nodes = one("p");
    let ua_important = sheet("p { color: red !important }");
    let author_important = sheet("p { color: blue !important }");
    let author_normal = sheet("p { color: blue }");

    assert_eq!(
        styles_from(
            &[
                (Origin::UserAgent, &ua_important),
                (Origin::Author, &author_important)
            ],
            &nodes
        )[0]
        .color,
        red(),
        "important UA beats important author — the reversal"
    );
    assert_eq!(
        styles_from(
            &[
                (Origin::UserAgent, &sheet("p { color: red }")),
                (Origin::Author, &author_important)
            ],
            &nodes
        )[0]
        .color,
        blue(),
        "important author beats normal UA, so this is not 'the UA always wins'"
    );
    assert_eq!(
        styles_from(
            &[
                (Origin::UserAgent, &sheet("p { color: red }")),
                (Origin::Author, &author_normal)
            ],
            &nodes
        )[0]
        .color,
        blue(),
        "normal author beats normal UA, which is the unreversed direction"
    );
    // The user origin sits between them in both halves, and it is the row a
    // three-value implementation drops entirely.
    assert_eq!(
        styles_from(
            &[
                (Origin::User, &sheet("p { color: red !important }")),
                (Origin::Author, &author_important)
            ],
            &nodes
        )[0]
        .color,
        red(),
        "important user beats important author"
    );
    assert_eq!(
        styles_from(
            &[
                (Origin::User, &sheet("p { color: red }")),
                (Origin::Author, &author_normal)
            ],
            &nodes
        )[0]
        .color,
        blue(),
        "normal author beats normal user"
    );
}

/// The ranks themselves, as a table, so the reversal is asserted as an order
/// and not only through six fixtures.
#[test]
fn the_six_reachable_ranks_are_in_the_specifications_order() {
    let ladder = [
        rank(Origin::UserAgent, false),
        rank(Origin::User, false),
        rank(Origin::Author, false),
        rank(Origin::Author, true),
        rank(Origin::User, true),
        rank(Origin::UserAgent, true),
    ];
    for pair in ladder.windows(2) {
        assert!(pair[0] < pair[1], "{ladder:?} is not §6.1's order");
    }
    // The two gaps are deliberate: 4 is animation and 8 is transition, and
    // neither has a source in this engine. Asserting them keeps a later
    // renumbering from quietly closing them up — which is the edit that would
    // move `!important` back the wrong way round.
    assert_eq!(rank(Origin::Author, false), 3);
    assert_eq!(rank(Origin::Author, true), 5);
    assert_eq!(rank(Origin::UserAgent, true), 7);
}

/// §6.1 criterion 3: an element-attached declaration beats every selector at
/// the same origin and importance, however specific.
#[test]
fn criterion_three_an_inline_declaration_beats_every_selector() {
    let mut nodes = one("p");
    nodes[0].id = Some("top".to_string());
    nodes[0].style = Some("color: blue".to_string());
    assert_eq!(styles("#top { color: red }", &nodes)[0].color, blue());
    // And it still loses to an `!important` rule, which is criterion 1 sorting
    // above criterion 3 — the pair that says this is an order and not a
    // special case.
    assert_eq!(
        styles("#top { color: red !important }", &nodes)[0].color,
        red()
    );
    // An `!important` inline declaration wins that back.
    nodes[0].style = Some("color: blue !important".to_string());
    assert_eq!(
        styles("#top { color: red !important }", &nodes)[0].color,
        blue()
    );
}

/// §6.1 criterion 5: specificity, when origin and attachment tie.
#[test]
fn criterion_five_specificity_decides_a_tie() {
    let mut nodes = one("p");
    nodes[0].classes = vec!["lead".to_string()];
    nodes[0].id = Some("top".to_string());
    // Written weakest-last on purpose, so a build that took the last rule
    // regardless of specificity would fail rather than pass by accident.
    assert_eq!(
        styles("#top { color: red } p { color: blue }", &nodes)[0].color,
        red()
    );
    assert_eq!(
        styles(".lead { color: red } p { color: blue }", &nodes)[0].color,
        red()
    );
    // `:not(.lead)` cannot match, but `:not(.other)` has `.lead`'s specificity
    // — so this pair is decided by order, not by specificity, and a build that
    // gave `:not()` a weight of its own would get it the other way round.
    assert_eq!(
        styles(":not(.other) { color: red } .lead { color: blue }", &nodes)[0].color,
        blue()
    );
}

/// §6.1 criterion 6: order of appearance, when everything else ties — and
/// across sheets, not only within one.
#[test]
fn criterion_six_order_decides_the_last_tie() {
    let nodes = one("p");
    assert_eq!(
        styles("p { color: red } p { color: blue }", &nodes)[0].color,
        blue()
    );
    assert_eq!(
        styles("p { color: blue } p { color: red }", &nodes)[0].color,
        red()
    );
    // Two declarations of one property inside one rule: the later wins, which
    // is the same criterion one level down.
    assert_eq!(
        styles("p { color: red; color: blue }", &nodes)[0].color,
        blue()
    );
    // And across two sheets at the same origin.
    let first = sheet("p { color: red }");
    let second = sheet("p { color: blue }");
    assert_eq!(
        styles_from(
            &[(Origin::Author, &first), (Origin::Author, &second)],
            &nodes
        )[0]
        .color,
        blue()
    );
}

/// §7.2: an inherited property comes from the parent's **computed** value and
/// a non-inherited one goes back to its initial value.
#[test]
fn inheritance_carries_the_computed_value_and_resets_the_rest() {
    let nodes = tree(&[("div", None), ("p", Some(0)), ("em", Some(1))]);
    let styled = styles(
        "div { color: red; margin-top: 5px; font-style: italic; display: block }",
        &nodes,
    );
    assert_eq!(styled[2].color, red(), "colour inherits two levels down");
    assert_eq!(styled[2].font_style, FontStyle::Italic);
    assert_eq!(
        styled[1].margin.top,
        MarginValue::Length(LengthPercentage::Px(0.0)),
        "margin does not inherit"
    );
    assert_eq!(
        styled[1].display,
        Display::Inline,
        "display does not inherit, and its initial value is inline"
    );
    assert_eq!(styled[0].display, Display::Block);
}

/// The single top-down pass and a lazy resolution agree, on a tree deep enough
/// and with enough relative units for them to be able to disagree.
///
/// The lazy one is **quadratic in tree depth** — it recomputes every ancestor
/// for every element, which is what "resolve inheritance on demand" costs once
/// `em` is in play — and that is the whole reason the shipped route is one
/// pass. Its agreeing is what makes the choice a performance decision rather
/// than a correctness one.
#[test]
fn a_lazy_resolution_and_the_single_pass_agree() {
    let mut spec: Vec<(&str, Option<usize>)> = vec![("html", None)];
    for depth in 1..40 {
        spec.push(("div", Some(depth - 1)));
    }
    let nodes = tree(&spec);
    let source = "
        html { font-size: 20px; color: red; line-height: 1.5 }
        div { font-size: 1.1em; margin-top: 0.5em; text-indent: 2rem }
        div div div { color: blue; letter-spacing: normal }
    ";
    let parsed = sheet(source);
    let sheets = [(Origin::Author, &parsed)];
    let single = styles_from(&sheets, &nodes);

    let limits = Limits::DEFAULT;
    for (at, expected) in single.iter().enumerate() {
        let mut budget = Budget::new(&limits);
        let lazy = resolve_lazily(&sheets, &nodes, at, &mut budget).expect("under every cap");
        assert_eq!(&lazy, expected, "element {at}");
    }
    // The font sizes actually compound, so "they agree" is a claim about
    // something rather than about forty copies of the initial value.
    assert!(single[39].font_size > single[0].font_size * 10.0);
}

/// `font-size` is applied before everything that is relative to it, whatever
/// order the cascade sorted the two declarations into.
///
/// A single pass in cascade order resolves `2em` against whatever the parent
/// had whenever `font-size` happens to sort later — right about half the time
/// and silently wrong the rest.
#[test]
fn font_size_is_resolved_before_anything_relative_to_it() {
    let nodes = one("p");
    // `text-indent` is written *first* and at a lower specificity, so it sorts
    // before `font-size` on both criteria a build might use.
    let styled = styles("p { text-indent: 2em } p#x, p { font-size: 40px }", &nodes);
    assert_eq!(styled[0].font_size, 40.0);
    assert_eq!(
        styled[0].text_indent,
        LengthPercentage::Px(80.0),
        "two of this element's ems, not two of its parent's"
    );
}

/// Two `font-size` declarations do not compound: only the winner is applied.
///
/// A cascade that applied every matched declaration in order would resolve
/// `1.5em` and then `2em` against it, and 16px would become 48px instead of
/// 32px — which looks like a font that is merely a bit large.
#[test]
fn only_the_winning_font_size_is_applied() {
    let nodes = one("p");
    assert_eq!(
        styles("p { font-size: 1.5em } p { font-size: 2em }", &nodes)[0].font_size,
        32.0
    );
}

/// A `line-height` **number** inherits as the factor and is re-multiplied by
/// each descendant's own font size; a length inherits already resolved.
#[test]
fn a_line_height_number_inherits_as_a_factor() {
    let nodes = tree(&[("div", None), ("p", Some(0))]);
    let by_number = styles(
        "div { line-height: 1.5; font-size: 10px } p { font-size: 20px }",
        &nodes,
    );
    assert_eq!(by_number[1].line_height, LineHeight::Number(1.5));
    let by_length = styles(
        "div { line-height: 15px; font-size: 10px } p { font-size: 20px }",
        &nodes,
    );
    assert_eq!(by_length[1].line_height, LineHeight::Px(15.0));
    // A percentage computes to a factor of the element that wrote it and then
    // inherits as that length — `css-values-3`'s rule, and the reason the
    // percentage is not kept as one.
    let by_percent = styles("div { line-height: 150%; font-size: 10px }", &nodes);
    assert_eq!(by_percent[0].line_height, LineHeight::Number(1.5));
}

/// `font-weight: bolder` is `css-fonts-4` §2.2's table and not "add 100".
#[test]
fn bolder_and_lighter_follow_the_table() {
    let nodes = tree(&[("div", None), ("p", Some(0))]);
    assert_eq!(
        styles("div { font-weight: 400 } p { font-weight: bolder }", &nodes)[1].font_weight,
        700
    );
    assert_eq!(
        styles("div { font-weight: 700 } p { font-weight: bolder }", &nodes)[1].font_weight,
        900
    );
    assert_eq!(
        styles(
            "div { font-weight: 400 } p { font-weight: lighter }",
            &nodes
        )[1]
        .font_weight,
        100
    );
    assert_eq!(
        styles(
            "div { font-weight: bold } p { font-weight: lighter }",
            &nodes
        )[1]
        .font_weight,
        400
    );
}

/// Every property this build implements reaches a computed style from a
/// stylesheet, which is the end-to-end shape of decision 5's first device.
#[test]
fn every_implemented_property_reaches_the_computed_style() {
    let nodes = one("p");
    let styled = &styles(
        "p {
            color: #112233;
            font-family: Georgia, serif;
            font-size: 20px;
            font-style: italic;
            font-variant: small-caps;
            font-weight: 700;
            line-height: 1.4;
            letter-spacing: 2px;
            word-spacing: 3px;
            text-align: justify;
            text-indent: 1em;
            text-decoration: underline;
            white-space: pre-wrap;
            list-style-type: square;
            visibility: hidden;
            display: inline-block;
            float: right;
            clear: both;
            box-sizing: border-box;
            width: 100px;
            height: 50%;
            margin: 1px 2px 3px 4px;
            padding: 5px;
            border: 2px dashed #445566;
            background-color: #778899;
            page-break-before: always;
            page-break-after: avoid;
            page-break-inside: avoid;
            orphans: 3;
            widows: 4;
            overflow-wrap: break-word;
            line-break: strict;
            word-break: keep-all;
         }",
        &nodes,
    )[0];
    assert_eq!(styled.font_size, 20.0);
    assert_eq!(styled.text_align, TextAlign::Justify);
    assert_eq!(styled.text_indent, LengthPercentage::Px(20.0));
    assert_eq!(styled.visibility, Visibility::Hidden);
    assert_eq!(styled.display, Display::InlineBlock);
    assert_eq!(styled.float, Float::Right);
    assert_eq!(styled.box_sizing, crate::property::BoxSizing::BorderBox);
    assert_eq!(styled.width, Size::Length(LengthPercentage::Px(100.0)));
    assert_eq!(styled.height, Size::Length(LengthPercentage::Percent(50.0)));
    assert_eq!(
        styled.margin.left,
        MarginValue::Length(LengthPercentage::Px(4.0))
    );
    assert_eq!(styled.padding.get(Side::Bottom), LengthPercentage::Px(5.0));
    assert_eq!(styled.border_width.top, 2.0);
    assert_eq!(styled.background_color.r, 0x77);
    assert_eq!(styled.page_break_before, crate::property::PageBreak::Always);
    assert_eq!(styled.page_break_after, crate::property::PageBreak::Avoid);
    assert_eq!(
        styled.page_break_inside,
        crate::property::PageBreakInside::Avoid
    );
    // Three and four rather than two and two: the initial value of both is 2,
    // so a build that ignored the declarations would still answer 2 and a test
    // asserting the initial value would pass without them.
    assert_eq!(styled.orphans, 3);
    assert_eq!(styled.widows, 4);
    assert_eq!(
        styled.overflow_wrap,
        crate::property::OverflowWrap::BreakWord
    );
    assert_eq!(
        styled.line_break,
        crate::property::LineBreakStrictness::Strict
    );
    assert_eq!(styled.word_break, crate::property::WordBreak::KeepAll);
}

/// An `Unsupported` declaration is counted where it **reached an element**,
/// not merely where it was parsed.
///
/// A `float: left` in a rule that matches nothing is not a gap this book
/// noticed, and counting it would inflate the one number the `As built` is
/// judged on.
#[test]
fn the_unsupported_census_counts_elements_reached() {
    let nodes = tree(&[("p", None), ("p", Some(0)), ("p", Some(0))]);
    let parsed = sheet("p { column-count: 2 } span { column-gap: 1em }");
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    let tree_styles = cascade(&[(Origin::Author, &parsed)], &nodes, &limits, &mut budget)
        .expect("under every cap");
    assert_eq!(tree_styles.report.unsupported, vec![("column-count", 3)]);
}

/// A caller that hands elements out of document order is refused **by name**.
///
/// The alternative is reading a computed style before it was written, which in
/// this shape means reading whatever the previous element left there — a book
/// styled by a neighbour, which looks like a book.
#[test]
fn a_tree_out_of_document_order_is_refused_by_name() {
    let mut nodes = tree(&[("div", None), ("p", Some(0))]);
    nodes[0].parent = Some(1);
    let parsed = sheet("p { color: red }");
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    assert_eq!(
        cascade(&[(Origin::Author, &parsed)], &nodes, &limits, &mut budget),
        Err(Refusal::NotInDocumentOrder { element: 0 })
    );
}

/// A rule that matches nothing changes nothing, which is the null case the
/// tests above all assume and none of them asserts.
#[test]
fn a_rule_that_matches_nothing_leaves_the_initial_value() {
    let nodes = one("p");
    assert_eq!(
        styles("span { color: red }", &nodes)[0].color,
        ComputedStyle::initial().color
    );
}

/// A property the winning rule does not set keeps its initial value even when
/// a losing rule set it — the cascade picks a winner per **property**, not per
/// rule.
#[test]
fn the_cascade_is_per_property_and_not_per_rule() {
    let mut nodes = one("p");
    nodes[0].id = Some("top".to_string());
    let styled = styles("p { color: red; float: left } #top { color: blue }", &nodes);
    assert_eq!(styled[0].color, blue(), "the id rule wins for colour");
    assert_eq!(
        styled[0].float,
        Float::Left,
        "and the type rule still wins for float, which the id rule never set"
    );
    assert_eq!(
        styled[0].display,
        Display::Inline,
        "and a property neither set is initial"
    );
}
