//! `css-lists-3` §4's counters and `css-counter-styles-3`'s predefined styles:
//! the grammar, the walk, and the representations.
//!
//! The page half — a marker and a `::before` agreeing on the same number — is
//! `crates/tinker-pdf/tests/epub_reftest.rs`'s; this file is about the tree the
//! numbers come from, which has two rules a first implementation gets wrong:
//! a counter reset on an element is visible to its **following siblings**, and
//! two sibling resets of one name **replace** rather than nest.

use super::{sheet, tree, Node};
use crate::cascade::{cascade, Origin, StyleTree};
use crate::counter::{marker_text, represent};
use crate::property::{CounterChange, Declaration, ListStylePosition, ListStyleType, Property};
use crate::selector::PseudoElement;
use crate::{Budget, Limits};

fn styled(source: &str, nodes: &[Node]) -> StyleTree {
    let parsed = sheet(source);
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    cascade(&[(Origin::Author, &parsed)], nodes, &limits, &mut budget)
        .expect("the fixture is under every cap")
}

fn declared(source: &str) -> Vec<Declaration> {
    sheet(source)
        .rules
        .iter()
        .flat_map(|rule| rule.declarations.iter().map(|d| d.declaration.clone()))
        .collect()
}

fn change(name: &str, value: i32) -> CounterChange {
    CounterChange {
        name: name.to_owned(),
        value,
    }
}

/// The markers of every list item in the tree, in document order.
fn markers(styles: &StyleTree, count: usize) -> Vec<Option<String>> {
    (0..count)
        .map(|at| styles.marker(at).map(str::to_owned))
        .collect()
}

// ---- the grammar ---------------------------------------------------------------

/// `none | [ <counter-name> <integer>? ]+`, with each property's own default
/// integer, and the shapes outside it discarded as the author's.
#[test]
fn the_counter_properties_read_names_and_integers_with_their_own_defaults() {
    assert_eq!(
        declared("p { counter-reset: a b 3 c -2 }"),
        [Declaration::Known(Property::CounterReset(vec![
            change("a", 0),
            change("b", 3),
            change("c", -2),
        ]))]
    );
    assert_eq!(
        declared("p { counter-increment: a b 1 }"),
        [Declaration::Known(Property::CounterIncrement(vec![
            change("a", 1),
            change("b", 1),
        ]))]
    );
    assert_eq!(
        declared("p { counter-set: list-item 7 }"),
        [Declaration::Known(Property::CounterSet(vec![change(
            "list-item",
            7
        )]))]
    );
    assert_eq!(
        declared("p { counter-reset: none }"),
        [Declaration::Known(Property::CounterReset(Vec::new()))]
    );
    // Two integers for one name, an integer with no name, `none` as a name,
    // a non-integer: none of these is the grammar.
    for value in ["a 1 1", "3", "a none", "a 1.5", "\"a\""] {
        assert!(
            declared(&format!("p {{ counter-increment: {value} }}")).is_empty(),
            "counter-increment: {value} is not CSS"
        );
    }
    // `reversed()` is, and it is this build's gap.
    assert_eq!(
        declared("ol { counter-reset: reversed(list-item) }"),
        [Declaration::Unsupported {
            property: "counter-reset",
            value: "reversed(list-item)".to_owned(),
        }]
    );
}

/// `list-style` is `<position> || <image> || <type>`, both implemented
/// longhands emitted, and an image refused by value.
#[test]
fn the_list_style_shorthand_sets_both_longhands_and_refuses_an_image() {
    let both = |kind, position| {
        vec![
            Declaration::Known(Property::ListStyleType(kind)),
            Declaration::Known(Property::ListStylePosition(position)),
        ]
    };
    assert_eq!(
        declared("ul { list-style: inside square }"),
        both(ListStyleType::Square, ListStylePosition::Inside)
    );
    assert_eq!(
        declared("ul { list-style: lower-roman }"),
        both(ListStyleType::LowerRoman, ListStylePosition::Outside)
    );
    assert_eq!(
        declared("ul { list-style: none }"),
        both(ListStyleType::None, ListStylePosition::Outside)
    );
    assert_eq!(
        declared("ul { list-style: none inside }"),
        both(ListStyleType::None, ListStylePosition::Inside)
    );
    assert_eq!(
        declared("ul { list-style: url(dot.png) disc }"),
        [Declaration::Unsupported {
            property: "list-style",
            value: "url(dot.png) disc".to_owned(),
        }]
    );
    assert!(declared("ul { list-style: disc square }").is_empty());
}

// ---- the walk --------------------------------------------------------------------

/// **Sibling lists each start again**, a nested list nests, and an element
/// that generates no box changes no counter, its own `counter-increment`
/// included (§4.3).
#[test]
fn list_items_count_through_the_counter_tree() {
    // 0 body, 1 ol, 2-4 li (3 is display: none), 5 ol nested in 4, 6-7 li,
    // 8 ol (sibling of 1), 9 li.
    let nodes = tree(&[
        ("body", None),
        ("ol", Some(0)),
        ("li", Some(1)),
        ("li", Some(1)),
        ("li", Some(1)),
        ("ol", Some(4)),
        ("li", Some(5)),
        ("li", Some(5)),
        ("ol", Some(0)),
        ("li", Some(8)),
    ]);
    let mut nodes = nodes;
    nodes[3].classes.push("gone".into());
    let styles = styled(
        "ol { display: block; counter-reset: list-item; list-style-type: decimal } \
         li { display: list-item } .gone { display: none; counter-increment: list-item 5 }",
        &nodes,
    );
    let m = |text: &str| Some(text.to_owned());
    assert_eq!(
        markers(&styles, nodes.len()),
        [
            None,
            None,
            m("1."),
            None,
            m("2."),
            None,
            m("1."),
            m("2."),
            None,
            m("1."),
        ]
    );
}

/// **A reset is visible to the following siblings and their descendants and
/// ends with its parent; a sibling's reset of the same name replaces it, and a
/// descendant's nests inside it** (§4.5). `counters()` joins what is in scope.
#[test]
fn a_counter_reset_reaches_following_siblings_and_ends_with_its_parent() {
    // 0 body; 1 h1, 2 p, 3 section, 6 p, 7 h1, 8 p under it; the section holds
    // 4 h2 and 5 p.
    let nodes = tree(&[
        ("body", None),
        ("h1", Some(0)),
        ("p", Some(0)),
        ("section", Some(0)),
        ("h2", Some(3)),
        ("p", Some(3)),
        ("p", Some(0)),
        ("h1", Some(0)),
        ("p", Some(0)),
    ]);
    let styles = styled(
        "h1 { counter-reset: n 4 } h2 { counter-reset: n } \
         p { counter-increment: n } \
         p::before { content: counters(n, \".\", lower-alpha) }",
        &nodes,
    );
    let before = |at: usize| {
        styles
            .pseudo(at, PseudoElement::Before)
            .map(|b| b.text.clone())
            .unwrap_or_default()
    };
    // The h1's reset reaches the p after it; the h2's, a level down, nests
    // inside it; once the section closes, the outer one counts on.
    assert_eq!(before(2), "e");
    assert_eq!(before(5), "e.a");
    assert_eq!(before(6), "f");
    // The second h1 is a sibling of the first: its reset **replaces** the
    // instance rather than nesting a second `n` inside it, so this is `e` and
    // not `f.e`.
    assert_eq!(before(8), "e");
}

/// **A presentational hint loses to every author rule**, which is where HTML
/// §15.1 puts it.
#[test]
fn a_presentational_hint_is_the_weakest_author_declaration() {
    let mut nodes = tree(&[("ol", None), ("li", Some(0))]);
    nodes[0]
        .attributes
        .push(("hint".into(), "counter-reset: list-item 4".into()));
    let base = "ol { list-style-type: decimal } li { display: list-item }";
    let hinted = styled(base, &nodes);
    assert_eq!(hinted.marker(1), Some("5."));
    // `:where()` has no specificity, so this rule ties the hint on everything
    // but position — the case "at the start of the author style sheet" decides.
    let overridden = styled(
        &format!("{base} :where(ol) {{ counter-reset: list-item }}"),
        &nodes,
    );
    assert_eq!(overridden.marker(1), Some("1."));
}

// ---- the styles ------------------------------------------------------------------

/// `css-counter-styles-3` §6's predefined styles, at the edges of their ranges.
#[test]
fn each_predefined_style_falls_back_to_decimal_outside_its_range() {
    assert_eq!(represent(-3, ListStyleType::Decimal), "-3");
    assert_eq!(represent(28, ListStyleType::LowerAlpha), "ab");
    assert_eq!(represent(26, ListStyleType::UpperAlpha), "Z");
    // No zero and no negatives in a bijective numeration: §2.2's fallback.
    assert_eq!(represent(0, ListStyleType::LowerAlpha), "0");
    assert_eq!(represent(-1, ListStyleType::UpperAlpha), "-1");
    assert_eq!(represent(3_999, ListStyleType::UpperRoman), "MMMCMXCIX");
    assert_eq!(represent(4_000, ListStyleType::LowerRoman), "4000");
    assert_eq!(represent(0, ListStyleType::LowerRoman), "0");
    assert_eq!(represent(-9, ListStyleType::Disc), "\u{2022}");
    assert_eq!(represent(5, ListStyleType::None), "");
    assert_eq!(marker_text(ListStyleType::LowerRoman, 4), "iv.");
    assert_eq!(marker_text(ListStyleType::Square, 4), "\u{25aa}");
    assert_eq!(marker_text(ListStyleType::None, 4), "");
}

// ---- quotes ------------------------------------------------------------------------

/// `quotes`' grammar: pairs of strings, `none`, `auto`, and `match-parent`
/// refused by value.
#[test]
fn quotes_reads_pairs_none_and_auto() {
    use crate::property::Quotes;
    assert_eq!(
        declared("q { quotes: \"<\" \">\" \"(\" \")\" }"),
        [Declaration::Known(Property::Quotes(Quotes::Pairs(vec![
            ("<".to_owned(), ">".to_owned()),
            ("(".to_owned(), ")".to_owned()),
        ])))]
    );
    assert_eq!(
        declared("q { quotes: none }"),
        [Declaration::Known(Property::Quotes(Quotes::None))]
    );
    assert_eq!(
        declared("q { quotes: auto }"),
        [Declaration::Known(Property::Quotes(Quotes::Auto))]
    );
    assert_eq!(
        declared("q { quotes: match-parent }"),
        [Declaration::Unsupported {
            property: "quotes",
            value: "match-parent".to_owned(),
        }]
    );
    for value in ["\"<\"", "\"<\" \">\" \"(\"", "\"<\" 3", "bold"] {
        assert!(
            declared(&format!("q {{ quotes: {value} }}")).is_empty(),
            "quotes: {value} is not CSS"
        );
    }
}

/// **The quote depth is the document's**: an open mark is the pair at the
/// current depth, the last pair past the list; a close lowers it first; a
/// close at zero produces nothing and does not go negative; `no-open-quote`
/// moves the depth without a mark; and under `auto` nothing is drawn and each
/// box that asked is counted against `quotes`.
#[test]
fn quote_keywords_walk_one_depth_across_the_document() {
    // 0 body; 1 p (close at zero), 2 q, 3 q inside 2, 4 q inside 3, 5 p
    // (no-open-quote), 6 q after it, 7 em (auto).
    let nodes = tree(&[
        ("body", None),
        ("p", Some(0)),
        ("q", Some(0)),
        ("q", Some(2)),
        ("q", Some(3)),
        ("p", Some(0)),
        ("q", Some(0)),
        ("em", Some(0)),
    ]);
    let mut nodes = nodes;
    nodes[5].classes.push("n".into());
    let styles = styled(
        "body { quotes: \"[\" \"]\" \"(\" \")\" } \
         p::before { content: close-quote \"|\" } \
         q::before { content: open-quote } q::after { content: close-quote } \
         .n::after { content: no-open-quote } \
         em { quotes: auto } em::before { content: open-quote \"x\" }",
        &nodes,
    );
    let text = |at: usize, which: PseudoElement| {
        styles
            .pseudo(at, which)
            .map(|b| b.text.clone())
            .unwrap_or_default()
    };
    assert_eq!(text(1, PseudoElement::Before), "|", "a close at depth zero");
    assert_eq!(text(2, PseudoElement::Before), "[");
    assert_eq!(text(3, PseudoElement::Before), "(");
    assert_eq!(
        text(4, PseudoElement::Before),
        "(",
        "past the list, the last pair"
    );
    assert_eq!(text(4, PseudoElement::After), ")");
    assert_eq!(text(3, PseudoElement::After), ")");
    assert_eq!(text(2, PseudoElement::After), "]");
    // The second p's ::after opened a level with no mark, so the next q is at
    // depth one.
    assert_eq!(text(6, PseudoElement::Before), "(");
    assert_eq!(text(7, PseudoElement::Before), "x", "auto draws no mark");
    assert_eq!(
        styles
            .report
            .unsupported
            .iter()
            .find(|(name, _)| *name == "quotes")
            .map(|(_, count)| *count),
        Some(1),
        "one box met `auto`"
    );
}
