//! Milestone 1: an SVG document, read into a tree and walked into a scene.
//!
//! Every fixture beside this file is first-party SVG written for one clause and
//! cited by its number. Nothing here is a recorded output: each expected value
//! is either a number the clause fixes (ninety user units to the inch) or
//! arithmetic written out beside the assertion.
//!
//! # Counted injection
//!
//! Each defect below was reintroduced into the shipped source, `cargo test -p
//! tinker-pdf-svg` run, and the failing tests counted. A check that catches
//! nothing when its defect comes back is a check that is not there, and the
//! only way to know is to put the defect back.
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | `document::read` uses `Doctype::Refuse` instead of `SkipExternalId` | 1 |
//! | `document::read` accepts any root element (drops the `NotAnSvg` check) | 1 |
//! | `document::read` stops recording `previous`/`next` sibling links | 1 |
//! | `Node::href` reads only `href`, not `xlink:href` | 1 |
//! | the depth cap compares `>` instead of `>=` | 1 |
//! | the node cap never fires | 1 |
//! | the truncation arm returns `Err` instead of keeping the partial tree | 1 |
//! | `document::length` uses 96 user units to the inch | 1 |
//! | `document::length` ignores the percentage basis | 2 |
//! | `Walk::element` walks into a `display="none"` subtree | 1 |
//! | `view_box_of` maps a degenerate box with the identity | 1 |
//! | `viewport_element` enters a viewport with no area | 1 |
//! | `Walk::warn` stops deduplicating | 1 |
//! | `Walk::warn` drops the `max_warnings` cap | 1 |
//! | `matrix_of` returns the identity for an unreadable transform | 1 |
//! | **`viewport_element` keeps the outer viewport for a nested `<svg>`** | **0** |
//!
//! The zero is the row worth reading, and it is written down rather than
//! quietly dropped. §7.9's rebasing changes the *scale* inside a nested
//! viewport, and at this milestone nothing has coordinates — a viewport a tenth
//! of the page still has an area, so every probe available here answers the
//! same either way. `tests/shapes.rs` is where it becomes a number.

use tinker_pdf_svg::document::{self, Child, Node};
use tinker_pdf_svg::{Limits, Refusal, Warning};

const DOCTYPE: &[u8] = include_bytes!("fixtures/doctype-svg11.svg");
const INTERNAL_SUBSET: &[u8] = include_bytes!("fixtures/internal-subset.svg");
const STRUCTURE: &[u8] = include_bytes!("fixtures/structure.svg");
const NON_GOALS: &[u8] = include_bytes!("fixtures/non-goals.svg");
const DISABLED: &[u8] = include_bytes!("fixtures/disabled.svg");
const UNITS: &[u8] = include_bytes!("fixtures/units.svg");

fn tree(bytes: &[u8]) -> document::Tree {
    document::read(bytes, &Limits::DEFAULT).expect("the fixture reads")
}

fn scene(bytes: &[u8], viewport: Option<(f64, f64)>) -> tinker_pdf_svg::Scene {
    tinker_pdf_svg::read(bytes, viewport, &Limits::DEFAULT).expect("the fixture reads")
}

fn named(node: &Node) -> &str {
    &node.name
}

// ---- the doctype question ---------------------------------------------------

/// SVG 1.1 Appendix A's declaration is read; an internal subset is refused
/// with nothing inside it parsed.
///
/// The pair is the point. A reader that refused both would pass an assertion
/// about the second and lose every file Illustrator has ever written; one that
/// accepted both would parse an entity table out of a stranger's file.
#[test]
fn the_svg_11_doctype_is_read_and_an_internal_subset_is_refused() {
    let tree = tree(DOCTYPE);
    assert_eq!(named(&tree.nodes[0]), "svg");
    assert_eq!(
        tree.nodes.len(),
        2,
        "the root and one <g>: {:?}",
        tree.nodes
    );

    assert_eq!(
        document::read(INTERNAL_SUBSET, &Limits::DEFAULT),
        Err(Refusal::Unreadable),
        "billion laughs in the internal subset is refused before it expands"
    );
}

/// A document whose root is not an `<svg>` is refused by its own name, not
/// read as an empty picture.
#[test]
fn a_root_that_is_not_an_svg_is_refused_by_name() {
    let html = b"<html xmlns=\"http://www.w3.org/1999/xhtml\"><body/></html>";
    assert_eq!(
        document::read(html, &Limits::DEFAULT),
        Err(Refusal::NotAnSvg)
    );
    // An element named `svg` in somebody else's namespace is not one either.
    let wrong = b"<svg xmlns=\"http://example.invalid/svg\"/>";
    assert_eq!(
        document::read(wrong, &Limits::DEFAULT),
        Err(Refusal::NotAnSvg)
    );
    // And no element at all is a different name again.
    assert_eq!(
        document::read(b"<!-- nothing -->", &Limits::DEFAULT),
        Err(Refusal::Unreadable)
    );
}

// ---- the tree ---------------------------------------------------------------

/// Parents precede their children and siblings are linked both ways.
///
/// The order is what `tinker_pdf_css::cascade` requires of a slice and the
/// links are what `selectors-4`'s `+` and `~` walk; a tree that had either
/// wrong would style a document subtly rather than visibly.
#[test]
fn the_tree_is_in_document_order_with_siblings_linked_both_ways() {
    let tree = tree(STRUCTURE);
    let names: Vec<&str> = tree.nodes.iter().map(named).collect();
    assert_eq!(
        names,
        ["svg", "title", "desc", "defs", "g", "g", "g", "use", "use"],
        "document order, parents first"
    );
    for (index, node) in tree.nodes.iter().enumerate() {
        if let Some(parent) = node.parent {
            assert!(parent < index, "{} sits before its child", names[parent]);
        }
        if let Some(next) = node.next {
            assert_eq!(
                tree.nodes[next].previous,
                Some(index),
                "the sibling links agree in both directions"
            );
        }
    }
    let outer = tree.by_id("outer").expect("#outer");
    let inner = tree.by_id("inner").expect("#inner");
    assert!(tree.contains(outer, inner), "#inner is under #outer");
    assert!(!tree.contains(inner, outer), "and not the other way round");
    assert_eq!(
        tree.nodes[outer].previous.map(|at| named(&tree.nodes[at])),
        Some("defs"),
        "the previous *element* sibling, text between them notwithstanding"
    );
}

/// `id`, `class` and `style` are read out of the attribute list once, and the
/// list keeps every attribute under the name the source spelled.
#[test]
fn the_three_names_the_cascade_asks_for_are_read_once() {
    let tree = tree(STRUCTURE);
    let reused = &tree.nodes[tree.by_id("reused").expect("#reused")];
    assert_eq!(reused.id.as_deref(), Some("reused"));
    assert_eq!(reused.classes, ["tool", "shared"]);
    assert_eq!(reused.style.as_deref(), Some("fill: red"));
    assert_eq!(
        reused.attr("class"),
        Some("tool shared"),
        "and the attribute is still in the list under its own name"
    );
}

/// §5.6's `xlink:href` and SVG 2's `href` are one reference.
///
/// Both spellings are in the wild — Illustrator writes the first and every
/// hand-written file writes the second — and a build that read one would fail
/// to resolve half the `<use>` elements there are.
#[test]
fn xlink_href_and_href_name_the_same_thing() {
    let tree = tree(STRUCTURE);
    let uses: Vec<&str> = tree
        .nodes
        .iter()
        .filter(|node| node.name == "use")
        .map(|node| node.href().expect("a <use> has a reference"))
        .collect();
    assert_eq!(uses, ["#reused", "#reused"], "both spellings resolved");
}

/// Character data reaches the element it is inside, interleaved with the
/// elements rather than concatenated after them.
#[test]
fn character_data_is_kept_where_the_source_put_it() {
    let tree = tree(STRUCTURE);
    let title = tree
        .nodes
        .iter()
        .find(|node| node.name == "title")
        .expect("<title>");
    assert_eq!(title.text().trim(), "Structure");
    let root = &tree.nodes[0];
    let elements = root
        .children
        .iter()
        .filter(|child| matches!(child, Child::Element(_)))
        .count();
    assert_eq!(elements, 5, "five element children: {:?}", root.children);
}

/// Past [`Limits::max_depth`] is a refusal by that name, and one below it is
/// a document.
///
/// Both directions, because a cap asserted only from the refusing side passes
/// for a build whose cap is off by one in the direction that refuses real
/// files.
#[test]
fn nesting_past_the_depth_cap_is_refused_by_name() {
    // `Limits` is `#[non_exhaustive]`, so a caller outside the crate tunes it
    // by assigning to the field rather than by a struct expression — which is
    // exactly what the facade does.
    let mut limits = Limits::DEFAULT;
    limits.max_depth = 8;
    let build = |depth: usize| {
        let mut out = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
        for _ in 0..depth {
            out.push_str("<g>");
        }
        for _ in 0..depth {
            out.push_str("</g>");
        }
        out.push_str("</svg>");
        out.into_bytes()
    };
    // The root is depth zero, so seven groups under it is the deepest legal
    // document at a cap of eight.
    assert!(document::read(&build(7), &limits).is_ok(), "seven is legal");
    assert_eq!(
        document::read(&build(8), &limits),
        Err(Refusal::TooDeep),
        "eight is not"
    );
}

/// Past [`Limits::max_nodes`] is a refusal by that name.
#[test]
fn more_elements_than_the_node_cap_is_refused_by_name() {
    let mut limits = Limits::DEFAULT;
    limits.max_nodes = 16;
    let mut wide = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
    for _ in 0..32 {
        wide.push_str("<g/>");
    }
    wide.push_str("</svg>");
    assert_eq!(
        document::read(wide.as_bytes(), &limits),
        Err(Refusal::TooManyNodes)
    );
}

/// Ruling 2: a document that stops half way keeps the elements it read.
///
/// An exporter that truncated a file should lose the bottom of the drawing
/// rather than all of it.
#[test]
fn a_truncated_document_keeps_the_elements_it_read() {
    let cut = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><g id=\"kept\"/><g id=\"al";
    let tree = document::read(cut, &Limits::DEFAULT).expect("the prefix is a tree");
    assert!(tree.truncated, "and it says the document did not finish");
    assert!(tree.by_id("kept").is_some(), "the element before the cut");
}

// ---- §4.2's lengths ---------------------------------------------------------

/// §7.10 fixes one inch at **ninety** user units, not CSS 2.1's ninety-six.
///
/// Two inches by seventy-two points is a hundred and eighty by ninety, and a
/// build on the wrong table produces a hundred and ninety-two by ninety-six —
/// which is the same shape and the wrong size, so only a number catches it.
#[test]
fn absolute_units_are_ninety_to_the_inch() {
    let scene = scene(UNITS, Some((640.0, 480.0)));
    assert_eq!(scene.size, (180.0, 90.0));
    for (text, expected) in [
        ("1in", 90.0),
        ("72pt", 90.0),
        ("6pc", 90.0),
        ("2.54cm", 90.0),
        ("25.4mm", 90.0),
        ("90", 90.0),
        ("90px", 90.0),
        ("-.5in", -45.0),
        ("1e1", 10.0),
    ] {
        let got = document::length(text, None).unwrap_or_else(|| panic!("{text} is a length"));
        assert!(
            (got - expected).abs() < 1e-9,
            "{text}: {got} is not {expected}"
        );
    }
    assert!(document::length("1parsec", None).is_none(), "not a unit");
    assert!(document::length("", None).is_none(), "not a number");
    assert!(document::length("1e999", None).is_none(), "not a double");
}

/// A percentage on the root is of the viewport the caller placed it in.
///
/// `width="50%"` in a 640-unit box is 320 and in a 300-unit box is 150, and a
/// build that read the number and dropped the sign would report 50 for both.
#[test]
fn a_percentage_is_of_the_viewport_the_caller_passed() {
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"50%\" height=\"25%\"/>";
    assert_eq!(scene(markup, Some((640.0, 480.0))).size, (320.0, 120.0));
    assert_eq!(scene(markup, Some((300.0, 200.0))).size, (150.0, 50.0));
    // No basis at all is a percentage with no meaning, refused rather than
    // guessed at.
    assert!(document::length("50%", None).is_none());
}

/// A root that states no size is the viewport it was placed in, and a caller
/// that passed none gets CSS 2.1 §10.3.2's replaced-element default.
#[test]
fn a_root_with_no_size_takes_the_viewport_it_was_given() {
    let bare = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
    assert_eq!(scene(bare, Some((432.0, 648.0))).size, (432.0, 648.0));
    assert_eq!(
        scene(bare, None).size,
        tinker_pdf_svg::scene::DEFAULT_VIEWPORT
    );
    // A viewport that is not a box is the caller's mistake, not the file's, so
    // it is replaced rather than refused.
    assert_eq!(
        scene(bare, Some((0.0, f64::NAN))).size,
        tinker_pdf_svg::scene::DEFAULT_VIEWPORT
    );
}

// ---- what the walk says -----------------------------------------------------

/// Every named non-goal reaches the caller as a warning that says which.
///
/// Asserted as a **set with a length**, so an element quietly added to the
/// refused list without a warning fails here rather than passing a test about
/// the ones that are left.
///
/// **`<clipPath>` left this list at milestone 4** and the fixture still holds
/// one, which is the point: it is now reached by reference like a `<defs>`
/// child and draws nothing where it stands, so an element that produced a
/// warning here again would mean the walk had started rendering it.
#[test]
fn every_named_non_goal_is_a_warning_that_says_which() {
    let scene = scene(NON_GOALS, Some((100.0, 100.0)));
    let expected = [
        Warning::FilterUnsupported,
        Warning::MaskUnsupported,
        Warning::PatternUnsupported,
        Warning::ForeignObjectUnsupported,
        Warning::AnimationIgnored,
        Warning::ScriptIgnored,
        Warning::MarkerUnsupported,
        Warning::ElementUnknown("nonsuch".to_owned()),
    ];
    for warning in &expected {
        assert!(
            scene.warnings.contains(warning),
            "{warning:?} is missing from {:?}",
            scene.warnings
        );
    }
    assert_eq!(
        scene.warnings.len(),
        expected.len(),
        "and nothing else: {:?}",
        scene.warnings
    );
    // `<animate>`, `<animateTransform>` and `<set>` are three elements and one
    // fact, which is what deduplication is for.
    assert_eq!(
        scene
            .warnings
            .iter()
            .filter(|w| **w == Warning::AnimationIgnored)
            .count(),
        1
    );
}

/// The two ways a subtree leaves the rendering tree, each observed by whether
/// the walk entered it.
///
/// §11.5's `display: none` takes the element **and its children**; §7.7's
/// degenerate `viewBox` *"disables rendering of the element"*, which is a
/// different answer from mapping it with an identity. A refused element inside
/// each is the probe: if the walk went in, its warning is in the list.
#[test]
fn a_disabled_subtree_is_not_walked_at_all() {
    let scene = scene(DISABLED, Some((100.0, 100.0)));
    assert!(
        !scene.warnings.contains(&Warning::FilterUnsupported),
        "display:none kept the walk out: {:?}",
        scene.warnings
    );
    assert!(
        !scene.warnings.contains(&Warning::MaskUnsupported),
        "a degenerate viewBox kept the walk out: {:?}",
        scene.warnings
    );
    assert!(
        scene.warnings.contains(&Warning::ScriptIgnored),
        "and the walk did run: {:?}",
        scene.warnings
    );
}

/// A `transform` that is not §7.6's grammar is **named**, not silently an
/// identity — a shape drawn where the file did not put it looks right.
#[test]
fn an_unreadable_transform_is_named_rather_than_ignored() {
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><g transform=\"wobble(3)\"/></svg>";
    let scene = scene(markup, Some((10.0, 10.0)));
    assert_eq!(
        scene.warnings,
        [Warning::ValueUnreadable {
            attribute: "transform".to_owned()
        }]
    );
}

/// Warnings are deduplicated and the list is capped.
///
/// Both halves matter: without the first a document with four hundred
/// `<animate>` elements reports four hundred times, and without the second a
/// document of distinct unknown element names chooses how much memory its own
/// diagnostics cost.
#[test]
fn warnings_are_deduplicated_and_capped() {
    let mut many = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
    for index in 0..64 {
        many.push_str(&format!("<unknown{index}/>"));
    }
    many.push_str("</svg>");

    let all =
        tinker_pdf_svg::read(many.as_bytes(), None, &Limits::DEFAULT).expect("the document reads");
    assert_eq!(all.warnings.len(), 64, "sixty-four distinct names");

    let mut tight = Limits::DEFAULT;
    tight.max_warnings = 4;
    let capped =
        tinker_pdf_svg::read(many.as_bytes(), None, &tight).expect("the document still reads");
    assert_eq!(capped.warnings.len(), 4, "and the cap holds");
}

/// §7.9: a nested `<svg>` establishes a viewport, and a viewport with no area
/// draws nothing — itself or anything inside it.
///
/// **What is deliberately not asserted here is the rebasing**: that a
/// percentage inside a nested viewport is of *that* viewport rather than of the
/// page. Nothing at this milestone can see it — a percentage is proportional,
/// so a viewport that is a tenth of the page still has an area either way, and
/// the injection that keeps the outer viewport fires **zero** assertions in
/// this file. It becomes visible the moment a shape has coordinates, and
/// `tests/shapes.rs` is where it is held.
#[test]
fn a_nested_viewport_with_no_area_draws_nothing() {
    // The probe is a `<mask>` inside the innermost viewport: it is reached
    // only if the walk went in at all.
    let markup = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
      <svg width="10%" height="10%">
        <svg width="5%" height="5%"><mask id="deep"/></svg>
      </svg>
    </svg>"#;
    let nested = scene(markup, Some((100.0, 100.0)));
    assert!(
        nested.warnings.contains(&Warning::MaskUnsupported),
        "every viewport on the way down has an area: {:?}",
        nested.warnings
    );

    let empty = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
      <svg width="0%" height="10%"><mask id="deep"/></svg>
    </svg>"#;
    assert!(
        !scene(empty, Some((100.0, 100.0)))
            .warnings
            .contains(&Warning::MaskUnsupported),
        "a viewport of no width is not entered"
    );
}
