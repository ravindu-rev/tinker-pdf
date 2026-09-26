//! Milestone 2: §9's basic shapes, every one of them an `Outline`.
//!
//! # Counted injection
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | `rect` defaults the absent radius to zero instead of the given one | 1 |
//! | `rect` does not clamp a radius to half the side | 1 |
//! | `rect` drops the element on a negative radius instead of degrading | 2 |
//! | `rect` draws a zero-area rectangle instead of nothing | 1 |
//! | `ellipse` uses its own quarter-turn constant, not `arc_to_curves` | 1 |
//! | `polygon` does not close | 1 |
//! | `polygon` drops the shape on an odd coordinate list | 1 |
//! | `shape` composes the ancestors' matrix but not the element's own | 1 |
//! | `shape` does not compose the matrix at all | 1 |
//! | **`viewport_element` keeps the outer viewport for a nested `<svg>`** | **1** |
//! | `Walk::spend` never refuses | 1 |
//! | `image` drops `preserveAspectRatio` | 1 |
//! | **`Walk::push` never refuses** | **0** |
//!
//! Two rows are in bold and they are the two worth reading.
//!
//! The nested viewport is **milestone 1's zero, closed**. `tests/document.rs`
//! recorded that injection as firing nothing, because a viewport a tenth of the
//! page still has an area and nothing at that milestone had coordinates. A
//! rectangle has coordinates.
//!
//! `Walk::push` is **this milestone's own zero**, and it is the same shape one
//! step later: until `<use>`, a node in the scene needs an element in the
//! document, so the element cap stands in front of the scene cap and nothing
//! here can reach the second. `tests/use.rs` is where an expansion makes the
//! two counts differ.

use tinker_pdf_svg::path::{Outline, Segment};
use tinker_pdf_svg::{Limits, Node, Refusal, Scene, Warning};

const SHAPES: &[u8] = include_bytes!("fixtures/shapes.svg");
const RADII: &[u8] = include_bytes!("fixtures/rect-radii.svg");
const NESTED: &[u8] = include_bytes!("fixtures/nested-scale.svg");
const IMAGE: &[u8] = include_bytes!("fixtures/placed-image.svg");

fn scene(bytes: &[u8], viewport: Option<(f64, f64)>) -> Scene {
    tinker_pdf_svg::read(bytes, viewport, &Limits::DEFAULT).expect("the fixture reads")
}

/// Every outline in a scene, in paint order.
fn outlines(scene: &Scene) -> Vec<Outline> {
    scene
        .nodes
        .iter()
        .filter_map(|node| match node {
            Node::Path { outline, .. } => Some(outline.clone()),
            _ => None,
        })
        .collect()
}

fn one(markup: &str) -> Outline {
    let scene = scene(markup.as_bytes(), Some((100.0, 100.0)));
    let mut outlines = outlines(&scene);
    assert_eq!(outlines.len(), 1, "one shape: {:?}", scene.nodes);
    outlines.remove(0)
}

/// Within a tenth of a millionth of a user unit — far below what a rasterizer
/// resolves and far above `f64`'s noise over these magnitudes.
fn near(left: f64, right: f64, what: &str) {
    assert!((left - right).abs() < 1e-7, "{what}: {left} is not {right}");
}

fn point(left: [f64; 2], right: [f64; 2], what: &str) {
    near(left[0], right[0], what);
    near(left[1], right[1], what);
}

/// Every point an outline visits, for a bounding-box assertion.
fn bounds(outline: &Outline) -> [f64; 4] {
    let mut out = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut see = |p: [f64; 2]| {
        out[0] = out[0].min(p[0]);
        out[1] = out[1].min(p[1]);
        out[2] = out[2].max(p[0]);
        out[3] = out[3].max(p[1]);
    };
    for segment in &outline.segments {
        match *segment {
            Segment::Move(p) | Segment::Line(p) => see(p),
            Segment::Cubic(a, b, c) => {
                see(a);
                see(b);
                see(c);
            }
            Segment::Close => {}
            _ => {}
        }
    }
    out
}

// ---- the straight-sided shapes ----------------------------------------------

/// §9.5's `<line>`, §9.6's `<polyline>` and §9.7's `<polygon>`, exactly.
///
/// The three are one family and the only difference between the last two is a
/// `Z`, which is a thing a build gets wrong in exactly one direction: a
/// polygon drawn open still fills, because a fill closes every subpath — so
/// only the *stroke* would be wrong, and only along one edge.
#[test]
fn the_straight_sided_shapes_are_the_points_they_state() {
    let shapes = scene(SHAPES, Some((200.0, 200.0)));
    let drawn = outlines(&shapes);
    assert_eq!(drawn.len(), 8, "eight shapes: {:?}", shapes.nodes);

    assert_eq!(
        drawn[3].segments,
        vec![Segment::Move([1.0, 2.0]), Segment::Line([3.0, 4.0])],
        "<line>"
    );
    let open = vec![
        Segment::Move([0.0, 0.0]),
        Segment::Line([10.0, 0.0]),
        Segment::Line([10.0, 10.0]),
    ];
    assert_eq!(drawn[4].segments, open, "<polyline>");
    let mut shut = open.clone();
    shut.push(Segment::Close);
    assert_eq!(
        drawn[5].segments, shut,
        "<polygon> closes and a polyline does not"
    );
    assert_eq!(
        drawn[6].segments,
        vec![Segment::Move([5.0, 5.0]), Segment::Line([15.0, 5.0])],
        "<path> is the path parser's own answer"
    );
    assert_eq!(
        drawn[7].segments,
        vec![Segment::Move([0.0, 0.0]), Segment::Line([10.0, 0.0])],
        "§9.7's error rule: the two complete pairs are rendered and the \
         dangling fifth coordinate is dropped"
    );
}

/// §9.2's plain `<rect>`: four corners, one close, in the order §9.2 states.
#[test]
fn a_rectangle_is_its_four_corners() {
    let shapes = scene(SHAPES, Some((200.0, 200.0)));
    assert_eq!(
        outlines(&shapes)[0].segments,
        vec![
            Segment::Move([10.0, 20.0]),
            Segment::Line([40.0, 20.0]),
            Segment::Line([40.0, 60.0]),
            Segment::Line([10.0, 60.0]),
            Segment::Close,
        ]
    );
}

// ---- §9.2's radii -----------------------------------------------------------

/// §9.2's three `rx`/`ry` rules, each against a rectangle it must equal.
///
/// Comparing outlines rather than asserting coordinates is deliberate: the
/// claim is that two spellings are the *same shape*, and a recorded coordinate
/// list would prove only that neither had changed.
#[test]
fn the_rect_radius_rules_are_three_equalities() {
    let scene = scene(RADII, Some((100.0, 100.0)));
    let drawn = outlines(&scene);
    let [both, one, other, huge, half, square, negative] = &drawn[..] else {
        panic!("seven rectangles: {:?}", drawn.len());
    };

    assert_eq!(one, both, "`rx` alone gives `ry` its value");
    assert_eq!(other, both, "and the other way round");
    assert_eq!(
        huge, half,
        "a radius past half the side clamps to half, so the corners meet \
         rather than crossing"
    );
    assert_eq!(
        negative, square,
        "§9.2 calls a negative radius an error; ruling 2 draws the rectangle \
         the file is about rather than losing it"
    );
    assert!(
        scene.warnings.contains(&Warning::ValueUnreadable {
            attribute: "rx".to_owned()
        }),
        "and names it: {:?}",
        scene.warnings
    );
    // The rounded rectangle stays inside the rectangle it rounds.
    let box_ = bounds(both);
    near(box_[0], 0.0, "left");
    near(box_[1], 0.0, "top");
    near(box_[2], 40.0, "right");
    near(box_[3], 20.0, "bottom");
    assert_ne!(both, square, "and a rounded rectangle is not a square one");
}

/// A rectangle with no area draws nothing — §9.2's own answer, and not an
/// error.
#[test]
fn a_rectangle_of_no_area_draws_nothing() {
    for attributes in ["width=\"0\" height=\"10\"", "width=\"10\" height=\"0\""] {
        let markup =
            format!("<svg xmlns=\"http://www.w3.org/2000/svg\"><rect {attributes}/></svg>");
        let scene = scene(markup.as_bytes(), Some((100.0, 100.0)));
        assert!(scene.nodes.is_empty(), "{attributes}: {:?}", scene.nodes);
        assert!(scene.warnings.is_empty(), "and says nothing about it");
    }
}

// ---- the curved shapes ------------------------------------------------------

/// A `<circle>` and the `<path>` that spells the same two arcs are the **same
/// segments**, not merely the same shape.
///
/// This is the module header's claim made checkable. Two quarter-turn
/// approximations in one crate would differ in the fourth decimal, and the
/// difference would be visible only where a rounded corner met a circle drawn
/// to the same radius — which is a defect nobody finds by looking.
#[test]
fn a_circle_and_the_path_that_spells_it_are_the_same_segments() {
    let circle =
        one("<svg xmlns=\"http://www.w3.org/2000/svg\"><circle cx=\"0\" cy=\"0\" r=\"50\"/></svg>");
    let path = one(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M 50 0 \
         A 50 50 0 0 1 -50 0 A 50 50 0 0 1 50 0 Z\"/></svg>",
    );
    assert_eq!(circle.segments, path.segments);
}

/// A circle passes through the four points its radius names, and an ellipse
/// through its own two pairs.
///
/// The endpoints are exact by construction — [`tinker_pdf_svg::path::arc_to_curves`]
/// forces the last one to the command's own — so this is an equality rather
/// than a tolerance on the ones it can be.
#[test]
fn a_circle_and_an_ellipse_reach_the_points_their_radii_name() {
    let shapes = scene(SHAPES, Some((200.0, 200.0)));
    let drawn = outlines(&shapes);

    let round = bounds(&drawn[1]);
    near(round[0], 50.0, "the circle's left");
    near(round[2], 150.0, "and its right");
    let Segment::Move(start) = drawn[1].segments[0] else {
        panic!("an outline begins with a move");
    };
    point(start, [150.0, 100.0], "the circle starts at three o'clock");

    let oval = bounds(&drawn[2]);
    near(oval[0], 50.0, "the ellipse's left");
    near(oval[2], 150.0, "and its right");
    // A cubic's control points sit outside the curve, so the vertical bound is
    // the control hull's rather than the ellipse's — but it is bounded by the
    // hull of a quarter arc, which for ry = 25 is 25 · (1 + 4/3 · tan(π/8)).
    let hull = 25.0 * (1.0 + 4.0 / 3.0 * (std::f64::consts::PI / 8.0).tan());
    assert!(
        oval[3] <= 100.0 + hull + 1e-7 && oval[3] >= 100.0 + 25.0,
        "the ellipse is half as tall as it is wide: {oval:?}"
    );
}

/// A radius of zero disables rendering; §9.3 and §9.4 both say so, and neither
/// calls it an error.
#[test]
fn a_radius_of_zero_draws_nothing() {
    for element in [
        "<circle r=\"0\"/>",
        "<circle/>",
        "<ellipse rx=\"10\" ry=\"0\"/>",
        "<ellipse/>",
    ] {
        let markup = format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{element}</svg>");
        let scene = scene(markup.as_bytes(), Some((100.0, 100.0)));
        assert!(scene.nodes.is_empty(), "{element}: {:?}", scene.nodes);
    }
}

// ---- transforms, composed once ----------------------------------------------

/// Every point in a scene is in the scene's space: the element's own matrix,
/// every ancestor's, and the root's `viewBox` mapping, all multiplied in.
///
/// The three are asserted separately because a build can get any one of them
/// and miss the others, and each miss draws a clean shape somewhere wrong.
#[test]
fn every_transform_on_the_way_down_is_composed_in() {
    // The element's own.
    let own = one(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect transform=\"translate(5,7)\" \
         width=\"1\" height=\"1\"/></svg>",
    );
    point(
        match own.segments[0] {
            Segment::Move(p) => p,
            _ => panic!("a move"),
        },
        [5.0, 7.0],
        "the element's own transform",
    );

    // An ancestor's, and the element's, in the right order: §7.6 makes a
    // child's own matrix apply first, in its parent's space.
    let both = one(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><g transform=\"scale(2)\">\
         <rect transform=\"translate(5,7)\" width=\"1\" height=\"1\"/></g></svg>",
    );
    point(
        match both.segments[0] {
            Segment::Move(p) => p,
            _ => panic!("a move"),
        },
        [10.0, 14.0],
        "translate then scale, and not scale then translate",
    );

    // The root's `viewBox`, which is a scale of ten here: a 10-unit box shown
    // in a 100-unit viewport.
    let mapped = one(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\" \
         viewBox=\"0 0 10 10\"><rect x=\"1\" y=\"2\" width=\"1\" height=\"1\"/></svg>",
    );
    point(
        match mapped.segments[0] {
            Segment::Move(p) => p,
            _ => panic!("a move"),
        },
        [10.0, 20.0],
        "the viewBox mapping",
    );
}

/// §7.9's rebasing, as a number.
///
/// **Milestone 1's zero-injection row, closed.** The inner viewport is a tenth
/// of the page, so a `width="50%"` inside it is five user units and not fifty.
/// A build that kept the outer viewport draws the same rectangle ten times too
/// wide, which is a picture that looks entirely plausible.
#[test]
fn a_percentage_inside_a_nested_viewport_is_of_that_viewport() {
    let outline = one(&String::from_utf8_lossy(NESTED));
    let box_ = bounds(&outline);
    near(box_[2] - box_[0], 5.0, "fifty per cent of ten");
    near(box_[3] - box_[1], 5.0, "on both axes");
}

/// Document order is paint order (§3.3), and the scene is already in it.
#[test]
fn the_scene_is_in_document_order() {
    let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <rect id=\"a\" x=\"1\" width=\"1\" height=\"1\"/>\
        <g><rect id=\"b\" x=\"2\" width=\"1\" height=\"1\"/></g>\
        <rect id=\"c\" x=\"3\" width=\"1\" height=\"1\"/></svg>";
    let scene = scene(markup.as_bytes(), Some((100.0, 100.0)));
    let firsts: Vec<f64> = outlines(&scene)
        .iter()
        .map(|outline| match outline.segments[0] {
            Segment::Move(p) => p[0],
            _ => panic!("a move"),
        })
        .collect();
    assert_eq!(firsts, [1.0, 2.0, 3.0]);
}

// ---- the bounds -------------------------------------------------------------

/// One document cannot be a million segments, however it spells them.
///
/// Charged across the **whole document** rather than per path, which is the
/// property this asserts: two paths of six segments each cross a budget of ten
/// and neither would cross it alone.
#[test]
fn segments_are_charged_across_the_whole_document() {
    let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <path d=\"M0 0 L1 0 L2 0 L3 0 L4 0 L5 0\"/>\
        <path d=\"M0 0 L1 0 L2 0 L3 0 L4 0 L5 0\"/></svg>";
    let mut limits = Limits::DEFAULT;
    limits.max_segments = 10;
    assert_eq!(
        tinker_pdf_svg::read(markup.as_bytes(), None, &limits),
        Err(Refusal::TooManySegments)
    );
    limits.max_segments = 12;
    assert!(
        tinker_pdf_svg::read(markup.as_bytes(), None, &limits).is_ok(),
        "and twelve is enough for two sixes"
    );
}

/// `max_nodes` bounds both halves, and **at this milestone only the first half
/// can fire**.
///
/// Until `<use>`, a node in the scene needs an element in the document, so the
/// element cap stands in front of the scene cap and the second is unreachable.
/// The injection that removes the scene-side check fires **zero** assertions,
/// and that is recorded rather than papered over — `tests/use.rs` is where an
/// expansion makes the two counts differ and the second half becomes
/// reachable. Asserting the reachable half here keeps the *number* honest:
/// eight rectangles under a cap of four refuse, and the refusal names itself.
#[test]
fn the_node_cap_refuses_by_name_on_the_half_that_can_fire() {
    let mut markup = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\">");
    for index in 0..8 {
        markup.push_str(&format!("<rect x=\"{index}\" width=\"1\" height=\"1\"/>"));
    }
    markup.push_str("</svg>");
    let mut limits = Limits::DEFAULT;
    limits.max_nodes = 4;
    assert_eq!(
        tinker_pdf_svg::read(markup.as_bytes(), None, &limits),
        Err(Refusal::TooManyNodes)
    );
    limits.max_nodes = 9;
    let scene = tinker_pdf_svg::read(markup.as_bytes(), None, &limits)
        .expect("nine is the root and its eight children");
    assert_eq!(
        scene.nodes.len(),
        8,
        "and eight elements produced eight nodes, which is the equality \
         `<use>` is about to break"
    );
}

// ---- §5.7's image -----------------------------------------------------------

/// An `<image>` reaches the caller **unresolved**: the reference, the
/// rectangle in its own space, the matrix out of it, and the aspect string.
///
/// Three of the six SVG spine items in the fetched corpus are exactly this and
/// nothing else, so what this node carries decides whether those pages draw.
#[test]
fn an_image_is_carried_with_everything_a_caller_needs_to_place_it() {
    let scene = scene(IMAGE, Some((550.0, 850.0)));
    let [node] = &scene.nodes[..] else {
        panic!("one node: {:?}", scene.nodes);
    };
    let Node::Image {
        href,
        rect,
        matrix,
        preserve,
    } = node
    else {
        panic!("an image: {node:?}");
    };
    assert_eq!(href, "flyer.jpg", "the reference, verbatim and unfetched");
    assert_eq!(*rect, [0.0, 0.0, 1100.0, 1700.0], "its own space");
    assert_eq!(
        preserve.as_deref(),
        Some("xMinYMin slice"),
        "the aspect string, for a caller that knows the intrinsic size"
    );
    // The viewBox is 1100 by 1700 into a 550-by-850 viewport, which is a half;
    // the `<g>` then translates by (100, 50) in the viewBox's own space, so the
    // origin lands at half of that.
    let placed = tinker_pdf_svg::transform::apply(*matrix, [0.0, 0.0]);
    point(placed, [50.0, 25.0], "the composed matrix");
    let far = tinker_pdf_svg::transform::apply(*matrix, [1100.0, 1700.0]);
    point(far, [600.0, 875.0], "and its far corner");
}

/// An `<image>` with no reference, or with no area, draws nothing.
#[test]
fn an_image_with_nothing_to_place_is_not_a_node() {
    for element in [
        "<image width=\"10\" height=\"10\"/>",
        "<image href=\"a.png\" width=\"0\" height=\"10\"/>",
        "<image href=\"a.png\"/>",
    ] {
        let markup = format!("<svg xmlns=\"http://www.w3.org/2000/svg\">{element}</svg>");
        let scene = scene(markup.as_bytes(), Some((100.0, 100.0)));
        assert!(scene.nodes.is_empty(), "{element}: {:?}", scene.nodes);
    }
}
