//! Milestone 5: §5.6's `<use>`, and the bomb it is.
//!
//! # Counted injection
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | the cycle guard is removed | 3 |
//! | the cycle guard sees only the innermost expansion | 1 |
//! | `max_uses` never fires | 1 |
//! | `x`/`y` are applied **before** the element's own transform | 1 |
//! | the instance inherits from the definition, not from the `<use>` | 1 |
//! | a `<symbol>` is not sized by the `<use>` that instances it | 1 |
//! | a `<symbol>` draws where it stands | 1 |
//! | a `<use>` naming nothing here is silent | 1 |
//! | a `<use>` into another document is silent | 1 |
//! | the depth cap reads `Node::depth` again instead of the walk's | 1 |
//! | the scene-node cap never fires | 1 |
//!
//! Eleven injections, no zeros — after three were found and fixed. The two
//! `UseUnresolved` arms were asserted together, so either could go silent with
//! the pair still passing; and the guard-weakening injection had to be written
//! as *"see only the innermost expansion"* rather than as an ancestor test,
//! because `use_element` does not take the `<use>`'s own index at all. That it
//! cannot is the shape of the argument: the stack is the guard, and an
//! ancestor test is a special case of it that this code has no way to spell.

use tinker_pdf_svg::path::Segment;
use tinker_pdf_svg::{Colour, Limits, Node, Paint, Refusal, Scene, Warning};

const REUSE: &[u8] = include_bytes!("fixtures/reuse.svg");
const BOMB: &[u8] = include_bytes!("fixtures/use-bomb.svg");

fn scene(bytes: &[u8]) -> Scene {
    tinker_pdf_svg::read(bytes, Some((100.0, 100.0)), &Limits::DEFAULT).expect("the fixture reads")
}

fn start(scene: &Scene, at: usize) -> [f64; 2] {
    match &scene.nodes[at] {
        Node::Path { outline, .. } => match outline.segments[0] {
            Segment::Move(point) => point,
            ref other => panic!("an outline begins with a move, not {other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

fn near(left: f64, right: f64, what: &str) {
    assert!((left - right).abs() < 1e-9, "{what}: {left} is not {right}");
}

/// §5.6: `x` and `y` are an **additional** `translate`, applied after the
/// element's own transform.
///
/// The `<use>` scales by two and translates by (7, 9); §5.6's order makes that
/// a scale and then a move of seven, so the square lands at (7, 9) rather than
/// at (14, 18). The two orders differ everywhere except the origin, which is
/// why the fixture's translate is not zero.
#[test]
fn a_use_translates_after_its_own_transform() {
    let scene = scene(REUSE);
    let at = start(&scene, 0);
    near(
        at[0],
        14.0,
        "scale(2) then translate(7) in the scaled space",
    );
    near(at[1], 18.0, "and the same on y");
}

/// §5.6: the referenced content inherits from the `<use>`, not from where it
/// was written.
///
/// This is the whole reason a symbol library is useful: one `<rect>` in a
/// `<defs>` and a dozen instances of it in a dozen colours. A build that
/// inherited from the definition's own ancestors would draw all twelve black.
#[test]
fn the_instance_inherits_from_the_use_and_not_from_the_definition() {
    let scene = scene(REUSE);
    let Node::Path { fill, .. } = &scene.nodes[1] else {
        panic!("a path");
    };
    assert_eq!(
        *fill,
        Paint::Solid(Colour {
            rgb: [1.0, 0.0, 0.0]
        }),
        "the `<use>` said red and the definition said nothing"
    );
}

/// One `<use>` of a group is **several** nodes, which is the equality
/// milestone 2 said `<use>` would break.
///
/// A scene may now hold more nodes than the document has elements, and that is
/// what makes `Limits::max_nodes`'s scene-side half reachable at all.
#[test]
fn one_use_of_a_group_is_as_many_nodes_as_the_group_has_shapes() {
    let scene = scene(REUSE);
    // `#pair` holds two rectangles, at x = 0 and x = 5.
    near(start(&scene, 2)[0], 0.0, "the first of the pair");
    near(start(&scene, 3)[0], 5.0, "and the second");
}

/// §5.6: a `<use>` of a `<symbol>` sizes it, and the symbol's `viewBox` maps
/// into that size.
#[test]
fn a_symbol_is_sized_by_the_use_that_instances_it() {
    let scene = scene(REUSE);
    // The view box is ten wide, the instance is twenty, so the scale is two —
    // and the symbol's own ten-unit rectangle spans twenty user units.
    let Node::Path { outline, .. } = &scene.nodes[4] else {
        panic!("a path");
    };
    let far = outline
        .segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::Line(point) => Some(point[0]),
            _ => None,
        })
        .fold(f64::MIN, f64::max);
    near(far, 20.0, "ten units of view box at a scale of two");
}

/// A `<symbol>` draws nothing where it stands — only where it is instanced.
#[test]
fn a_symbol_draws_nothing_until_it_is_used() {
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <symbol id=\"s\"><rect width=\"1\" height=\"1\"/></symbol></svg>";
    let scene = scene(markup);
    assert!(scene.nodes.is_empty(), "{:?}", scene.nodes);
}

/// A reference that names nothing in this document is named rather than
/// silently drawing nothing.
#[test]
fn a_use_that_names_nothing_here_is_named() {
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <use href=\"#nowhere\"/></svg>";
    let scene = scene(markup);
    assert_eq!(scene.warnings, [Warning::UseUnresolved]);
    assert!(scene.nodes.is_empty());
}

/// A reference **into another document** is named too, and separately.
///
/// This crate has no container, no filesystem and no network, which is what
/// makes it a leaf — so `other.svg#thing` is a resource it cannot fetch rather
/// than one it failed to find. The two arms are asserted apart because they
/// are two `return`s, and a suite that only checked the pair together would
/// pass with either one silent.
#[test]
fn a_use_into_another_document_is_named_too() {
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <use href=\"elsewhere.svg#thing\"/></svg>";
    let scene = scene(markup);
    assert_eq!(scene.warnings, [Warning::UseUnresolved]);
    assert!(scene.nodes.is_empty());
}

/// The fixture's five instances draw and its two unresolved references do not.
#[test]
fn only_the_references_that_resolve_draw() {
    let scene = scene(REUSE);
    // The translated square, the red one, two from the pair, and the symbol.
    assert_eq!(scene.nodes.len(), 5, "{:?}", scene.nodes.len());
    assert!(scene.warnings.contains(&Warning::UseUnresolved));
}

// ---- the bomb ------------------------------------------------------------------

/// **A `<use>` inside the subtree it references is refused by name.**
///
/// Not by a stack that ran out, not by a depth counter that happened to trip
/// first, and not by a timeout: `Refusal::TooManyUses`, from a guard whose
/// whole job this is. The difference between those answers is the difference
/// between a defence and an accident — gap 30's argument for refusing
/// `<!DOCTYPE` by name rather than bounding entity expansion, one format over.
#[test]
fn a_use_that_reaches_its_own_ancestor_is_refused_by_name() {
    assert_eq!(
        tinker_pdf_svg::read(BOMB, Some((100.0, 100.0)), &Limits::DEFAULT),
        Err(Refusal::TooManyUses)
    );
}

/// An **indirect** cycle, which no ancestor test and no "is it the one I just
/// expanded?" test can see.
///
/// `#a` uses `#b`, `#b` uses `#a`, and neither contains the other. The obvious
/// guard — "is the target an ancestor of the `<use>`?" — answers no for both
/// and recurses for ever. The guard here is the stack of targets being
/// expanded, which catches the direct case as a special case of this one, so
/// there is one rule rather than two.
#[test]
fn an_indirect_cycle_is_refused_by_the_same_name() {
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <g id=\"a\"><use href=\"#b\"/></g>\
        <g id=\"b\"><use href=\"#a\"/></g></svg>";
    assert_eq!(
        tinker_pdf_svg::read(markup, Some((100.0, 100.0)), &Limits::DEFAULT),
        Err(Refusal::TooManyUses)
    );
}

/// A `<use>` of itself, which is the shortest cycle there is.
#[test]
fn a_use_of_itself_is_refused() {
    let markup = b"<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <use id=\"self\" href=\"#self\"/></svg>";
    assert_eq!(
        tinker_pdf_svg::read(markup, Some((100.0, 100.0)), &Limits::DEFAULT),
        Err(Refusal::TooManyUses)
    );
}

/// Past [`Limits::max_uses`] is a refusal by that name, and inside it is a
/// document.
///
/// Both directions, because a cap asserted only from the refusing side passes
/// for a build whose cap is off by one in the direction that refuses real
/// files — and a symbol library with three hundred instances is a real file.
#[test]
fn more_expansions_than_the_cap_is_refused_by_name() {
    let build = |count: usize| {
        let mut out = String::from(
            "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs>\
             <rect id=\"u\" width=\"1\" height=\"1\"/></defs>",
        );
        for _ in 0..count {
            out.push_str("<use href=\"#u\"/>");
        }
        out.push_str("</svg>");
        out.into_bytes()
    };
    let mut limits = Limits::DEFAULT;
    limits.max_uses = 4;
    assert!(
        tinker_pdf_svg::read(&build(4), None, &limits).is_ok(),
        "four expansions is four"
    );
    assert_eq!(
        tinker_pdf_svg::read(&build(5), None, &limits),
        Err(Refusal::TooManyUses)
    );
}

/// **`Limits::max_nodes`'s scene-side half, which milestone 2 recorded as
/// unreachable.**
///
/// That milestone's injection table has a zero against `Walk::push`: until
/// `<use>`, a node in the scene needed an element in the document, so the
/// element cap stood in front of the scene cap and nothing could reach the
/// second. Ten `<use>`s of one shape is ten scene nodes from thirteen
/// elements, and the two counts have come apart.
#[test]
fn the_scene_node_cap_is_reachable_once_a_use_expands() {
    let mut markup = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><defs>\
         <g id=\"u\"><rect width=\"1\" height=\"1\"/><rect width=\"1\" height=\"1\"/>\
         <rect width=\"1\" height=\"1\"/><rect width=\"1\" height=\"1\"/></g></defs>",
    );
    for _ in 0..6 {
        markup.push_str("<use href=\"#u\"/>");
    }
    markup.push_str("</svg>");
    // Twelve elements, twenty-four scene nodes.
    let mut limits = Limits::DEFAULT;
    limits.max_nodes = 16;
    assert_eq!(
        tinker_pdf_svg::read(markup.as_bytes(), None, &limits),
        Err(Refusal::TooManyNodes),
        "sixteen is above the element count and below the node count, which is \
         the window only a `<use>` opens"
    );
}

/// A chain of `<use>`s nests the **walk** even though every element is
/// shallow, and the depth cap sees it.
///
/// `Node::depth` is where an element is written and `Frame::depth` is where it
/// is being drawn. A build that capped on the first would let a chain of ten
/// thousand `<use>`s recurse while every element in it looked one deep.
#[test]
fn a_chain_of_uses_nests_the_walk_and_the_depth_cap_sees_it() {
    let mut markup = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\"><defs>");
    for index in 0..12 {
        markup.push_str(&format!(
            "<g id=\"g{index}\"><use href=\"#g{}\"/></g>",
            index + 1
        ));
    }
    markup.push_str("<rect id=\"g12\" width=\"1\" height=\"1\"/></defs>");
    markup.push_str("<use href=\"#g0\"/></svg>");
    let mut limits = Limits::DEFAULT;
    limits.max_depth = 8;
    assert_eq!(
        tinker_pdf_svg::read(markup.as_bytes(), None, &limits),
        Err(Refusal::TooDeep),
        "every element is two deep and the walk is twenty-five"
    );
    // The same document at a cap that admits it draws.
    limits.max_depth = 64;
    let scene =
        tinker_pdf_svg::read(markup.as_bytes(), None, &limits).expect("sixty-four is enough");
    assert_eq!(scene.nodes.len(), 1, "and it drew the rectangle at the end");
}
