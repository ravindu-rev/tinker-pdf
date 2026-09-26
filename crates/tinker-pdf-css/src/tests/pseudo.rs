//! `::before` and `::after`: the cascade half.
//!
//! `crates/tinker-pdf/tests/epub_pseudo.rs` is the other half — the box on the
//! page. This file is about the door between them, [`StyleTree::pseudo`], and
//! about the three things that are easy to get wrong on this side of it:
//!
//! * **what the box inherits from.** CSS 2.1 §12.1 says the originating
//!   element, not its parent, and the two differ on every element that sets an
//!   inherited property. A build that inherited from the parent is right on
//!   every fixture where the element sets nothing.
//! * **when a box exists at all.** §12.2 makes `content` the condition. A
//!   `p::before { color: red }` with no `content` generates nothing, and a
//!   build that generated an empty box would put an empty inline box in every
//!   paragraph of a book that styles `::before` without filling it.
//! * **that the originating element is still not styled.** The old behaviour
//!   was "matches nothing"; the new one is "matches a box that is not the
//!   element", and the failure mode between them is the same paragraph turning
//!   red.
//!
//! # Counted injections
//!
//! Recorded in `crates/tinker-pdf/tests/epub_pseudo.rs`, where the whole
//! matrix is run across both halves at once. Splitting the counts across two
//! files would report each defect twice and make the total meaningless.

use super::{sheet, tree, Node};
use crate::cascade::{cascade, ComputedStyle, Origin, StyleTree};
use crate::property::{Color, Display};
use crate::selector::PseudoElement;
use crate::{Budget, Limits};

/// Cascades one author sheet over a tree and hands back the whole tree, which
/// is what a `pseudo` lookup needs and `tests::cascade::styles` throws away.
fn styled(source: &str, nodes: &[Node]) -> StyleTree {
    let parsed = sheet(source);
    let limits = Limits::DEFAULT;
    let mut budget = Budget::new(&limits);
    cascade(&[(Origin::Author, &parsed)], nodes, &limits, &mut budget)
        .expect("the fixture is under every cap")
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

/// A `div` with a `p` inside it, which is the smallest tree that can tell
/// "inherits from the originating element" from "inherits from its parent".
fn pair() -> Vec<Node> {
    tree(&[("div", None), ("p", Some(0))])
}

// ---- when a box exists -------------------------------------------------------

/// **§12.2: `content` is what makes the box exist.**
///
/// Four cases, and the last two are the ones a first implementation gets
/// wrong: a rule that styles `::before` without filling it, and one that fills
/// it with `none`.
#[test]
fn a_box_exists_exactly_when_content_does() {
    let nodes = pair();

    let with = styled("p::before { content: \"x\" }", &nodes);
    assert!(with.pseudo(1, PseudoElement::Before).is_some());
    assert_eq!(with.pseudo(1, PseudoElement::Before).unwrap().text, "x");

    // No rule at all.
    let without = styled("p { color: red }", &nodes);
    assert!(without.pseudo(1, PseudoElement::Before).is_none());

    // Styled and not filled.
    let styled_only = styled("p::before { color: red }", &nodes);
    assert!(
        styled_only.pseudo(1, PseudoElement::Before).is_none(),
        "a `::before` with no content generates no box"
    );

    // Filled with nothing. `none` and `normal` are the same answer here and
    // both are asserted, because §2.1 says so and a build that read only one
    // of them would generate a box for the other.
    for keyword in ["none", "normal"] {
        let empty = styled(&format!("p::before {{ content: {keyword} }}"), &nodes);
        assert!(
            empty.pseudo(1, PseudoElement::Before).is_none(),
            "content: {keyword} generates a box"
        );
    }
}

/// `::after` is its own box, and the two do not share.
#[test]
fn before_and_after_are_two_boxes() {
    let nodes = pair();
    let both = styled(
        "p::before { content: \"[\" } p::after { content: \"]\" }",
        &nodes,
    );
    assert_eq!(both.pseudo(1, PseudoElement::Before).unwrap().text, "[");
    assert_eq!(both.pseudo(1, PseudoElement::After).unwrap().text, "]");

    let one = styled("p::before { content: \"[\" }", &nodes);
    assert!(one.pseudo(1, PseudoElement::After).is_none());
}

/// The two that generate nothing here return `None` from the same door, rather
/// than being absent from the enum.
#[test]
fn first_line_and_first_letter_generate_no_box() {
    let nodes = pair();
    let styles = styled(
        "p::first-line { content: \"x\" } p::first-letter { content: \"y\" }",
        &nodes,
    );
    assert!(styles.pseudo(1, PseudoElement::FirstLine).is_none());
    assert!(styles.pseudo(1, PseudoElement::FirstLetter).is_none());
}

// ---- what it inherits from ---------------------------------------------------

/// **§12.1: the box inherits from its originating element, not from that
/// element's parent.**
///
/// The fixture sets `color` on both the `div` and the `p` so the two answers
/// are different colours. A build that inherited from the parent gets the
/// `div`'s red; the right answer is the `p`'s green.
#[test]
fn a_generated_box_inherits_from_its_originating_element() {
    let nodes = pair();
    let styles = styled(
        "div { color: #ff0000 } p { color: #00ff00 } p::before { content: \"x\" }",
        &nodes,
    );
    let generated = styles.pseudo(1, PseudoElement::Before).expect("a box");
    assert_eq!(
        generated.style.color,
        rgb(0, 255, 0),
        "the originating element's colour, not its parent's"
    );
    // And the fixture is worthless unless the two really differ.
    assert_eq!(styles.styles[0].color, rgb(255, 0, 0));
}

/// A non-inherited property starts at its initial value, exactly as it does on
/// an element — the generated box is not a copy of the originating element.
#[test]
fn a_generated_box_is_not_a_copy_of_its_originating_element() {
    let nodes = pair();
    let styles = styled(
        "p { border-top-width: 7px } p::before { content: \"x\" }",
        &nodes,
    );
    let generated = styles.pseudo(1, PseudoElement::Before).expect("a box");
    assert_eq!(
        generated.style.border_width.top,
        ComputedStyle::initial().border_width.top,
        "`border-top-width` does not inherit, so the box starts at initial"
    );
    assert_eq!(
        styles.styles[1].border_width.top, 7.0,
        "and the element has it"
    );
}

/// The box takes its own declarations, and `inherit` on it reads the
/// originating element too.
#[test]
fn the_box_takes_its_own_declarations() {
    let nodes = pair();
    let styles = styled(
        "p { border-top-width: 7px } \
         p::before { content: \"x\"; color: #0000ff; border-top-width: inherit; display: block }",
        &nodes,
    );
    let generated = styles.pseudo(1, PseudoElement::Before).expect("a box");
    assert_eq!(generated.style.color, rgb(0, 0, 255));
    assert_eq!(generated.style.display, Display::Block);
    assert_eq!(
        generated.style.border_width.top, 7.0,
        "`inherit` on a generated box reads the originating element"
    );
}

// ---- the originating element is not styled ----------------------------------

/// **The failure the old behaviour existed to prevent, still prevented.**
///
/// `p::before { color: red }` must not colour the paragraph. It was true when
/// the rule matched nothing; it has to stay true now that it matches something
/// else.
#[test]
fn the_originating_element_is_not_styled_by_the_rule() {
    let nodes = pair();
    let styles = styled("p::before { content: \"x\"; color: #ff0000 }", &nodes);
    assert_eq!(
        styles.styles[1].color,
        ComputedStyle::initial().color,
        "the paragraph took the generated box's colour"
    );
    assert_eq!(
        styles.pseudo(1, PseudoElement::Before).unwrap().style.color,
        rgb(255, 0, 0),
        "and the box did not"
    );
}

// ---- the cascade applies -----------------------------------------------------

/// A generated box is cascaded, not merely matched: specificity, order and
/// `!important` all decide between two rules for the same box.
#[test]
fn two_rules_for_one_box_cascade() {
    let nodes = pair();

    // Order, at equal specificity.
    let later = styled(
        "p::before { content: \"a\" } p::before { content: \"b\" }",
        &nodes,
    );
    assert_eq!(later.pseudo(1, PseudoElement::Before).unwrap().text, "b");

    // Specificity beats order.
    let specific = styled(
        "p::before { content: \"b\" } :root p::before { content: \"a\" }",
        &nodes,
    );
    assert_eq!(specific.pseudo(1, PseudoElement::Before).unwrap().text, "a");

    // And `!important` beats specificity.
    let important = styled(
        "p::before { content: \"b\" !important } :root p::before { content: \"a\" }",
        &nodes,
    );
    assert_eq!(
        important.pseudo(1, PseudoElement::Before).unwrap().text,
        "b"
    );
}

/// A rule that does not match the originating element generates nothing.
#[test]
fn a_rule_whose_subject_does_not_match_generates_nothing() {
    let nodes = pair();
    let styles = styled("span::before { content: \"x\" }", &nodes);
    for at in 0..nodes.len() {
        assert!(styles.pseudo(at, PseudoElement::Before).is_none());
    }
}

// ---- attr() ------------------------------------------------------------------

/// `attr()` reads the **originating element's** attribute, and §2.4 makes a
/// missing one the empty string rather than an error.
#[test]
fn attr_reads_the_originating_element() {
    let mut nodes = pair();
    nodes[1]
        .attributes
        .push(("data-label".to_owned(), "Note".to_owned()));

    let styles = styled("p::before { content: attr(data-label) }", &nodes);
    assert_eq!(
        styles.pseudo(1, PseudoElement::Before).unwrap().text,
        "Note"
    );

    // Missing: the empty string, and therefore a box that exists and draws
    // nothing — which is §2.4's answer and not the same as no box at all.
    let missing = styled("p::before { content: attr(data-missing) }", &nodes);
    let box_ = missing.pseudo(1, PseudoElement::Before).expect("a box");
    assert_eq!(box_.text, "");
}

/// Strings and `attr()` concatenate, in the order written.
#[test]
fn content_concatenates_in_order() {
    let mut nodes = pair();
    nodes[1]
        .attributes
        .push(("data-n".to_owned(), "7".to_owned()));
    let styles = styled("p::before { content: \"[\" attr(data-n) \"] \" }", &nodes);
    assert_eq!(
        styles.pseudo(1, PseudoElement::Before).unwrap().text,
        "[7] "
    );
}

// ---- the door itself ---------------------------------------------------------

/// Every element has an entry, so the consumer can ask about any of them
/// without a bounds check of its own — and an index past the end is `None`
/// rather than a panic (ruling 1).
#[test]
fn the_door_answers_for_every_element_and_past_the_end() {
    let nodes = pair();
    let styles = styled("p::before { content: \"x\" }", &nodes);
    assert_eq!(styles.generated.len(), nodes.len());
    assert!(styles.pseudo(0, PseudoElement::Before).is_none());
    assert!(styles.pseudo(1, PseudoElement::Before).is_some());
    assert!(styles.pseudo(9_999, PseudoElement::Before).is_none());
    assert!(styles.pseudo(usize::MAX, PseudoElement::After).is_none());
}
