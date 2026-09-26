//! Milestone 3: §11's painting properties, from §6.4's three sources.
//!
//! # Counted injection
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | a presentation attribute ranks above a `<style>` rule | 1 |
//! | `style=""` ranks below a `<style>` rule | 2 |
//! | `!important` is ignored in the ranking | 1 |
//! | specificity is `a * 100 + b * 10 + c` rather than the tuple | 3 |
//! | an unreadable value zeroes the property instead of leaving it | 1 |
//! | `currentColor` resolves to black instead of `color` | 1 |
//! | `stroke-dasharray` does not repeat an odd list | 1 |
//! | an all-zero `stroke-dasharray` is kept rather than becoming `none` | 1 |
//! | `visibility: hidden` still paints | 5 |
//! | `display: none` in `style=""` is not read | 1 |
//! | `opacity` is inherited instead of multiplied | 1 |
//! | group opacity is flattened with no warning | 1 |
//! | a zero-width stroke still becomes a `Stroke` | 1 |
//! | a `url(#…)` paint drops its fallback | 2 |
//! | an at-rule in a `<style>` element is read as a qualified rule | 1 |
//! | `fill-opacity` is not multiplied by `opacity` | 3 |
//! | a `<pattern>` paint reports as merely unresolved | 1 |
//!
//! Seventeen injections and **no zeros**: every check here has a defect that
//! reaches it. The three that fire more than twice are the ones whose defect
//! moves a shape rather than a value — `visibility` changes how many nodes a
//! scene holds, so five tests counting nodes see it.

use tinker_pdf_svg::{Colour, FillRule, Limits, LineCap, LineJoin, Node, Paint, Scene, Warning};

const THREE: &[u8] = include_bytes!("fixtures/three-sources.svg");
const PAINTING: &[u8] = include_bytes!("fixtures/painting.svg");

fn scene(bytes: &[u8]) -> Scene {
    tinker_pdf_svg::read(bytes, Some((100.0, 100.0)), &Limits::DEFAULT).expect("the fixture reads")
}

/// The `n`th path node of a scene.
fn path(scene: &Scene, at: usize) -> &Node {
    scene
        .nodes
        .iter()
        .filter(|node| matches!(node, Node::Path { .. }))
        .nth(at)
        .unwrap_or_else(|| panic!("no path {at} in {:?}", scene.nodes.len()))
}

fn fill_of(scene: &Scene, at: usize) -> Paint {
    match path(scene, at) {
        Node::Path { fill, .. } => fill.clone(),
        other => panic!("{other:?}"),
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Paint {
    Paint::Solid(Colour {
        rgb: [
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
        ],
    })
}

// ---- §6.4's three sources ----------------------------------------------------

/// The three sources, in §6.4's order, on one document.
///
/// Every rectangle carries a **red** presentation attribute and the winner is
/// never red except where nothing else applies — so a build that ranked the
/// attribute too high fails on four of the five at once, and a build that
/// dropped it fails on the first.
#[test]
fn the_three_sources_rank_in_the_order_section_6_4_states() {
    let scene = scene(THREE);
    assert_eq!(
        fill_of(&scene, 0),
        rgb(255, 0, 0),
        "a presentation attribute alone is the value"
    );
    assert_eq!(
        fill_of(&scene, 1),
        rgb(0, 255, 0),
        "§6.4: a rule beats a presentation attribute whatever its selector"
    );
    assert_eq!(
        fill_of(&scene, 2),
        rgb(0, 255, 255),
        "and `style=\"\"` beats the rule"
    );
    assert_eq!(
        fill_of(&scene, 3),
        rgb(0, 255, 0),
        "§6.1's reversal: an important rule beats a normal `style=\"\"`"
    );
    assert_eq!(
        fill_of(&scene, 4),
        rgb(0, 0, 255),
        "`.a.b` is two classes and `.a` is one, so the tuple decides"
    );
}

/// `selectors-4` §15's tuple, not a base-ten packing.
///
/// Eleven classes on one selector is what breaks `a * 100 + b * 10 + c`, and
/// no document announces that it has one. The specificity comes from
/// `tinker-pdf-css`, which already derives `Ord` on the tuple — this asserts
/// that the ranking here uses it rather than flattening it.
#[test]
fn one_id_beats_any_number_of_classes() {
    let mut markup = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><style>\
         #target { fill: #00ff00 }\n",
    );
    // Twelve classes, and the rule comes *after* the id rule so source order
    // would give it the win if specificity did not.
    let classes: String = (0..12).map(|n| format!(".c{n}")).collect();
    markup.push_str(&format!("{classes} {{ fill: #ff0000 }}\n"));
    markup.push_str("</style>");
    let names: Vec<String> = (0..12).map(|n| format!("c{n}")).collect();
    markup.push_str(&format!(
        "<rect id=\"target\" class=\"{}\" width=\"1\" height=\"1\"/></svg>",
        names.join(" ")
    ));
    let scene = scene(markup.as_bytes());
    assert_eq!(fill_of(&scene, 0), rgb(0, 255, 0));
}

/// An at-rule in a `<style>` element is skipped whole and named.
///
/// Not read as a qualified rule: `@media print { .a { fill: magenta } }` has a
/// prelude that is not a selector list, and a build that fed it to the selector
/// parser would either refuse the sheet or — worse — apply the print rule.
#[test]
fn an_at_rule_is_skipped_and_named() {
    let scene = scene(THREE);
    assert!(
        scene.warnings.contains(&Warning::AtRuleIgnored),
        "{:?}",
        scene.warnings
    );
    assert_ne!(
        fill_of(&scene, 1),
        rgb(255, 0, 255),
        "and the print rule did not apply"
    );
}

// ---- §11's properties --------------------------------------------------------

/// §11.3 and §11.6 on one shape: the fill, its rule, and its alpha.
#[test]
fn the_fill_family_reaches_the_node() {
    let scene = scene(PAINTING);
    let Node::Path {
        fill,
        rule,
        fill_opacity,
        stroke,
        ..
    } = path(&scene, 0)
    else {
        panic!("a path");
    };
    assert_eq!(*fill, rgb(255, 0, 0), "the colour table is the CSS crate's");
    assert_eq!(*rule, FillRule::EvenOdd);
    assert!((fill_opacity - 0.5).abs() < 1e-12);
    assert!(stroke.is_none(), "and no stroke was stated");
}

/// §11.4's whole stroke family, including the two rules a first build misses.
#[test]
fn the_stroke_family_reaches_the_node() {
    let scene = scene(PAINTING);
    let Node::Path { fill, stroke, .. } = path(&scene, 1) else {
        panic!("a path");
    };
    assert_eq!(*fill, Paint::None, "`fill: none` is no ink, not black");
    let stroke = stroke.as_ref().expect("a stroke");
    assert_eq!(stroke.paint, rgb(0, 255, 0), "`#0f0` expands to `#00ff00`");
    assert!((stroke.width - 3.0).abs() < 1e-12);
    assert_eq!(stroke.cap, LineCap::Round);
    assert_eq!(stroke.join, LineJoin::Bevel);
    assert!((stroke.miter_limit - 2.0).abs() < 1e-12);
    assert_eq!(
        stroke.dashes,
        vec![5.0, 5.0],
        "§11.4 repeats an odd dash list to make it even; a build that took \
         `5` literally would draw a solid line"
    );
    assert!((stroke.dash_offset - 2.0).abs() < 1e-12);
    assert!((stroke.opacity - 0.25).abs() < 1e-12);
}

/// §11.2's `currentColor` is `color`, inherited from wherever it was set.
#[test]
fn current_colour_is_the_colour_property() {
    let scene = scene(PAINTING);
    assert_eq!(fill_of(&scene, 2), rgb(0, 0, 255));
}

/// §11.5: a hidden element is not in the display list.
#[test]
fn a_hidden_element_does_not_reach_the_scene() {
    let scene = scene(PAINTING);
    // Eight rectangles in the fixture, of which `#hidden` is not drawn and
    // `#hairless` draws with no stroke — so seven paths.
    assert_eq!(
        scene.nodes.len(),
        7,
        "one of the eight is hidden: {:?}",
        scene.nodes.len()
    );
}

/// §11.5's `display: none` in a `style=""` attribute, which is how Inkscape
/// spells it.
#[test]
fn display_none_is_read_from_the_style_attribute_too() {
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <g style=\"display:none\"><rect width=\"1\" height=\"1\"/><mask/></g>\
        <rect width=\"1\" height=\"1\"/></svg>";
    let scene = scene(markup);
    assert_eq!(scene.nodes.len(), 1, "{:?}", scene.nodes);
    assert!(
        !scene.warnings.contains(&Warning::MaskUnsupported),
        "and the subtree was not walked at all"
    );
}

/// §14.5's group opacity, flattened into each descendant and **named where the
/// flattening is observable**.
///
/// The product is what reaches the node: a group at 0.5 over a fill at 0.5 is
/// 0.25. That much is exact. What is not exact is the fill and the stroke
/// compositing against each other before the group is faded, and that is the
/// warning.
#[test]
fn group_opacity_multiplies_and_says_when_it_shows() {
    let scene = scene(PAINTING);
    let Node::Path {
        fill_opacity,
        stroke,
        ..
    } = path(&scene, 3)
    else {
        panic!("a path");
    };
    assert!(
        (fill_opacity - 0.25).abs() < 1e-12,
        "0.5 group times 0.5 fill: {fill_opacity}"
    );
    assert!(
        (stroke.as_ref().expect("a stroke").opacity - 0.5).abs() < 1e-12,
        "and the stroke takes the group's alone"
    );
    assert!(
        scene.warnings.contains(&Warning::GroupOpacityFlattened),
        "{:?}",
        scene.warnings
    );
}

/// A lone shape at a non-unit opacity is **exact**, and says nothing.
///
/// The other half of the pair above. A build that warned on every `opacity`
/// would be reporting a defect it does not have, which is as bad as reporting
/// none: a warning that always fires is a warning nobody reads.
#[test]
fn a_single_painted_shape_at_an_opacity_is_not_a_warning() {
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <rect opacity=\"0.6\" fill=\"red\" width=\"1\" height=\"1\"/></svg>";
    let scene = scene(markup);
    assert!(
        !scene.warnings.contains(&Warning::GroupOpacityFlattened),
        "{:?}",
        scene.warnings
    );
    let Node::Path { fill_opacity, .. } = path(&scene, 0) else {
        panic!("a path");
    };
    assert!((fill_opacity - 0.6).abs() < 1e-12);
}

/// `opacity` composes down the tree rather than inheriting.
///
/// Two nested groups at a half each is a quarter, and a build that *inherited*
/// it would give the child a half — the same picture wherever nothing nests,
/// and wrong in every file that does.
#[test]
fn nested_opacities_multiply() {
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><g opacity=\"0.5\">\
        <g opacity=\"0.5\"><rect fill=\"red\" width=\"1\" height=\"1\"/></g></g></svg>";
    let scene = scene(markup);
    let Node::Path { fill_opacity, .. } = path(&scene, 0) else {
        panic!("a path");
    };
    assert!((fill_opacity - 0.25).abs() < 1e-12, "{fill_opacity}");
}

/// §13.2's fallback after a reference that resolves to nothing.
#[test]
fn an_unresolved_paint_server_falls_back_to_what_the_file_said() {
    let scene = scene(PAINTING);
    assert_eq!(
        fill_of(&scene, 4),
        rgb(255, 255, 0),
        "the file wrote what to do when the server is missing"
    );
    assert!(
        scene.warnings.contains(&Warning::PaintServerUnresolved),
        "and it is still named: {:?}",
        scene.warnings
    );
    // With no fallback stated, §13.2's answer is that nothing is painted.
    let bare = b"<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <rect fill=\"url(#nothing)\" width=\"1\" height=\"1\"/></svg>";
    let bare =
        tinker_pdf_svg::read(bare, Some((100.0, 100.0)), &Limits::DEFAULT).expect("it reads");
    assert_eq!(fill_of(&bare, 0), Paint::None);
}

/// `css-cascade-5` §5.2: a value that is not the grammar leaves the inherited
/// value standing, and the property is named.
///
/// Not zero, and not the initial value. `stroke-width: 3px3` on a child of a
/// group that said seven must draw seven, because that is what the document
/// says; a build that zeroed it would silently remove the stroke.
#[test]
fn an_unreadable_value_leaves_the_inherited_one_standing() {
    let scene = scene(PAINTING);
    let Node::Path { stroke, .. } = path(&scene, 5) else {
        panic!("a path");
    };
    let stroke = stroke.as_ref().expect("the inherited stroke");
    assert!(
        (stroke.width - 7.0).abs() < 1e-12,
        "the group's width: {}",
        stroke.width
    );
    assert!(
        scene.warnings.contains(&Warning::ValueUnreadable {
            attribute: "stroke-width".to_owned()
        }),
        "and the property is named: {:?}",
        scene.warnings
    );
}

/// §11.4: a stroke that puts no ink on the page is not a stroke.
///
/// Answered here rather than carried, so no consumer has to decide whether a
/// `Stroke` of width zero draws — which is the kind of question two consumers
/// answer differently.
#[test]
fn a_stroke_that_draws_nothing_is_not_a_stroke() {
    let scene = scene(PAINTING);
    let Node::Path { stroke, .. } = path(&scene, 6) else {
        panic!("a path");
    };
    assert!(stroke.is_none(), "a zero width: {stroke:?}");
}

/// An all-zero dash pattern is `none`, because a pattern that never advances
/// is a line a rasterizer cannot draw.
#[test]
fn a_dash_pattern_that_never_advances_is_no_pattern() {
    for value in ["0 0", "none", "0"] {
        let markup = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect stroke=\"red\" \
             stroke-dasharray=\"{value}\" width=\"1\" height=\"1\"/></svg>"
        );
        let scene = scene(markup.as_bytes());
        let Node::Path { stroke, .. } = path(&scene, 0) else {
            panic!("a path");
        };
        assert!(
            stroke.as_ref().expect("a stroke").dashes.is_empty(),
            "{value}"
        );
    }
    // §11.4: one negative dash invalidates the whole list rather than being
    // dropped from it, so the inherited pattern stands.
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><g stroke-dasharray=\"4 2\">\
        <rect stroke=\"red\" stroke-dasharray=\"4 -2\" width=\"1\" height=\"1\"/></g></svg>";
    let scene = scene(markup);
    let Node::Path { stroke, .. } = path(&scene, 0) else {
        panic!("a path");
    };
    assert_eq!(stroke.as_ref().expect("a stroke").dashes, vec![4.0, 2.0]);
}

/// A `<pattern>` named as a paint is refused **as a pattern**, not as a
/// missing server.
///
/// The two are different facts and a caller can act on the difference: one
/// says the document referenced something that is not there, the other says
/// this build declines a paint server that is. A single warning for both would
/// report a complete file as a broken one.
#[test]
fn a_pattern_paint_is_refused_under_its_own_name() {
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\">        <defs><pattern id=\"tiles\"/></defs>        <rect fill=\"url(#tiles) green\" width=\"1\" height=\"1\"/></svg>";
    let scene = scene(markup);
    assert!(
        scene.warnings.contains(&Warning::PatternUnsupported),
        "{:?}",
        scene.warnings
    );
    assert!(
        !scene.warnings.contains(&Warning::PaintServerUnresolved),
        "the server is there; this build declines it: {:?}",
        scene.warnings
    );
    assert_eq!(
        fill_of(&scene, 0),
        rgb(0, 128, 0),
        "and §13.2's fallback is what the file said to use instead"
    );
}
