//! §11.6's markers: a referenced subtree drawn again at every vertex.
//!
//! Every expected point here is arithmetic from §11.6.2's transform — the
//! vertex, the turn `orient` asks for, the `markerUnits` scale, and the
//! reference point taken through the view box — written beside the assertion,
//! never a recorded output.
//!
//! # Counted injection
//!
//! Counted over this file and `crates/tinker-pdf/tests/epub_svg.rs`, whose
//! `a_marker_is_drawn_at_the_end_of_its_line` is the one that reaches paper.
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | `marker-mid` is drawn at the first and last vertices too | 4 |
//! | the bisector is the incoming direction alone | 3 |
//! | a turn past 180° is not wrapped back | 1 |
//! | a turn past -180° is not wrapped back | 1 |
//! | `markerUnits="strokeWidth"` does not scale | 2 |
//! | the reference point is not taken through the view box | 1 |
//! | every cubic of an arc is a vertex | 1 |
//! | a closed subpath's first vertex has no incoming direction | 1 |
//! | the marker's viewport clip is never made | 1 |
//! | the marker's style inherits from the shape instead of its ancestry | 10 |
//! | the marker cycle guard is removed | 1 |
//! | a marker naming nothing is silent | 1 |
//! | `auto-start-reverse` is read as `auto` | 1 |
//! | markers are painted before the shape | 11 |
//!
//! Fourteen injections, no zeros — after one was found. The wrap that keeps a
//! bisector on the short side of a turn is two rules, one per direction, and
//! the first fixture turned only one way, so disabling the other fired
//! nothing; the zigzag now turns through 180° in both.

use tinker_pdf_svg::path::Segment;
use tinker_pdf_svg::{Limits, Node, Paint, Refusal, Scene, Warning};

fn scene(markup: &str) -> Scene {
    tinker_pdf_svg::read(markup.as_bytes(), Some((100.0, 100.0)), &Limits::DEFAULT)
        .expect("the document reads")
}

fn near(left: f64, right: f64, what: &str) {
    assert!((left - right).abs() < 1e-9, "{what}: {left} is not {right}");
}

fn point(left: [f64; 2], right: [f64; 2], what: &str) {
    near(left[0], right[0], &format!("{what}, x"));
    near(left[1], right[1], &format!("{what}, y"));
}

/// The first two points of a path node's outline.
fn first_two(node: &Node) -> ([f64; 2], [f64; 2]) {
    let Node::Path { outline, .. } = node else {
        panic!("a path: {node:?}");
    };
    let points: Vec<[f64; 2]> = outline
        .segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::Move(p) | Segment::Line(p) => Some(*p),
            _ => None,
        })
        .collect();
    (points[0], points[1])
}

/// The marker instances after the shape: every node but the first, each
/// unwrapped from its viewport clip where it has one.
fn instances(scene: &Scene) -> Vec<&Node> {
    scene
        .nodes
        .iter()
        .skip(1)
        .flat_map(|node| match node {
            Node::Group { nodes, .. } => nodes.iter().collect::<Vec<_>>(),
            other => vec![other],
        })
        .collect()
}

const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\">";

/// A marker that is a line five long along its own x axis, unclipped and
/// unscaled, so each instance's first two points are its origin and its
/// heading.
fn arrow(orient: &str) -> String {
    format!(
        "<marker id=\"m\" orient=\"{orient}\" markerUnits=\"userSpaceOnUse\" \
         overflow=\"visible\"><path d=\"M0 0 L5 0\"/></marker>"
    )
}

/// §11.6.2: start at the first vertex, mid at every other, end at the last —
/// and each instance's origin is its vertex.
#[test]
fn start_mid_and_end_land_on_their_vertices() {
    let markup = format!(
        "{SVG}<defs>\
         <marker id=\"s\" overflow=\"visible\" markerUnits=\"userSpaceOnUse\"><path d=\"M0 0 L1 0\"/></marker>\
         <marker id=\"i\" overflow=\"visible\" markerUnits=\"userSpaceOnUse\"><path d=\"M0 0 L2 0\"/></marker>\
         <marker id=\"e\" overflow=\"visible\" markerUnits=\"userSpaceOnUse\"><path d=\"M0 0 L3 0\"/></marker>\
         </defs><polyline points=\"10,10 30,10 30,30 50,30\" fill=\"none\" stroke=\"black\" \
         marker-start=\"url(#s)\" marker-mid=\"url(#i)\" marker-end=\"url(#e)\"/></svg>"
    );
    let scene = scene(&markup);
    let drawn = instances(&scene);
    let found: Vec<([f64; 2], f64)> = drawn
        .iter()
        .map(|node| {
            let (origin, along) = first_two(node);
            (origin, along[0] - origin[0])
        })
        .collect();
    // `orient` is the initial angle zero, so each line runs along +x and its
    // length says which marker it is.
    assert_eq!(found.len(), 4, "{found:?}");
    let expected = [
        ([10.0, 10.0], 1.0),
        ([30.0, 10.0], 2.0),
        ([30.0, 30.0], 2.0),
        ([50.0, 30.0], 3.0),
    ];
    for ((origin, length), (want, want_length)) in found.iter().zip(expected) {
        point(*origin, want, "the vertex");
        near(*length, want_length, "which marker");
    }
}

/// `orient="auto"` turns a marker to the **bisector** at a corner, and to the
/// one direction at an end.
///
/// The polyline goes right and then down — 0° then 90° in SVG's y-down space —
/// so the corner's marker points at 45°, the start's at 0° and the end's at
/// 90°: a heading of five along (cos θ, sin θ).
#[test]
fn auto_orientation_is_the_bisector_at_a_corner() {
    let markup = format!(
        "{SVG}<defs>{}</defs><polyline points=\"0,0 10,0 10,10\" fill=\"none\" \
         marker=\"url(#m)\"/></svg>",
        arrow("auto")
    );
    let scene = scene(&markup);
    let drawn = instances(&scene);
    assert_eq!(drawn.len(), 3);
    let half = 5.0 * core::f64::consts::FRAC_1_SQRT_2;
    let heads = [
        ([0.0, 0.0], [5.0, 0.0]),
        ([10.0, 0.0], [10.0 + half, half]),
        ([10.0, 10.0], [10.0, 15.0]),
    ];
    for (node, (origin, head)) in drawn.iter().zip(heads) {
        let (at, along) = first_two(node);
        point(at, origin, "the vertex");
        point(along, head, "the heading");
    }
}

/// The bisector is taken **the short way round**.
///
/// Right, then back up-and-left at 170°: the turn is 170°, and half of it is
/// 85°, which a plain mean of the two angles also gets. The next two corners
/// are the ones it does not: 170° to -170° is a turn of 20° **through** 180°,
/// and -170° to 170° is a turn of -20° through it — each bisected at 180°,
/// where the plain mean of the two angles is zero. One corner turns each way,
/// because the wrap is two rules and each has its own direction.
#[test]
fn the_bisector_is_taken_the_short_way_round() {
    let c = |degrees: f64| tinker_pdf_math::cos(tinker_pdf_math::to_radians(degrees));
    let s = |degrees: f64| tinker_pdf_math::sin(tinker_pdf_math::to_radians(degrees));
    let a = [10.0, 0.0];
    let b = [a[0] + 10.0 * c(170.0), a[1] + 10.0 * s(170.0)];
    let d = [b[0] + 10.0 * c(-170.0), b[1] + 10.0 * s(-170.0)];
    let end = [d[0] + 10.0 * c(170.0), d[1] + 10.0 * s(170.0)];
    let markup = format!(
        "{SVG}<defs>{}</defs><polyline points=\"0,0 {},{} {},{} {},{} {},{}\" \
         marker-mid=\"url(#m)\"/></svg>",
        arrow("auto"),
        a[0],
        a[1],
        b[0],
        b[1],
        d[0],
        d[1],
        end[0],
        end[1]
    );
    let scene = scene(&markup);
    let drawn = instances(&scene);
    assert_eq!(drawn.len(), 3, "three interior vertices");
    for (which, node) in ["the second", "the third"].iter().zip(&drawn[1..]) {
        let (at, along) = first_two(node);
        let heading = [along[0] - at[0], along[1] - at[1]];
        assert!(
            (heading[0] + 5.0).abs() < 1e-6 && heading[1].abs() < 1e-6,
            "{which} corner's marker points at 180 degrees: {heading:?}"
        );
    }
}

/// `markerUnits="strokeWidth"`, the initial value, scales the marker by the
/// **shape's** `stroke-width`; `userSpaceOnUse` does not.
#[test]
fn stroke_width_units_scale_by_the_shapes_stroke() {
    let markup = format!(
        "{SVG}<defs>\
         <marker id=\"w\" overflow=\"visible\"><path d=\"M0 0 L5 0\"/></marker>\
         <marker id=\"u\" overflow=\"visible\" markerUnits=\"userSpaceOnUse\"><path d=\"M0 0 L5 0\"/></marker>\
         </defs><line x1=\"0\" y1=\"0\" x2=\"10\" y2=\"0\" stroke=\"black\" stroke-width=\"3\" \
         marker-start=\"url(#w)\" marker-end=\"url(#u)\"/></svg>"
    );
    let scene = scene(&markup);
    let drawn = instances(&scene);
    let (start, start_head) = first_two(drawn[0]);
    let (end, end_head) = first_two(drawn[1]);
    near(
        start_head[0] - start[0],
        15.0,
        "five, times a stroke three wide",
    );
    near(end_head[0] - end[0], 5.0, "and five in user space");
}

/// `refX`/`refY` are in the view box's coordinates, taken through the view
/// box onto the vertex.
///
/// A view box of 0 0 10 10 into a 5 by 5 marker halves everything, so the
/// reference point (10, 0) is (5, 0) in the viewport — and that is the point
/// that lands on the vertex, which puts the content's origin five to its left.
#[test]
fn the_reference_point_is_taken_through_the_view_box() {
    let markup = format!(
        "{SVG}<defs><marker id=\"v\" overflow=\"visible\" markerUnits=\"userSpaceOnUse\" \
         markerWidth=\"5\" markerHeight=\"5\" viewBox=\"0 0 10 10\" refX=\"10\" refY=\"0\">\
         <path d=\"M0 0 L10 0\"/></marker></defs>\
         <line x1=\"40\" y1=\"40\" x2=\"60\" y2=\"40\" marker-start=\"url(#v)\"/></svg>"
    );
    let scene = scene(&markup);
    let (origin, along) = first_two(instances(&scene)[0]);
    point(origin, [35.0, 40.0], "the content's origin");
    point(
        along,
        [40.0, 40.0],
        "and its (10, 0), the reference, on the vertex",
    );
}

/// A vertex is where a **command** ends: an arc is several cubics here and
/// one vertex there.
#[test]
fn an_arc_is_one_vertex_and_not_one_per_cubic() {
    let markup = format!(
        "{SVG}<defs>{}</defs><path d=\"M10 50 A40 40 0 0 1 90 50 L90 90\" \
         marker-mid=\"url(#m)\"/></svg>",
        arrow("0")
    );
    let scene = scene(&markup);
    let Node::Path { outline, .. } = &scene.nodes[0] else {
        panic!("the path");
    };
    assert!(
        outline.segments.len() > 3,
        "the half circle is more than one cubic: {}",
        outline.segments.len()
    );
    let drawn = instances(&scene);
    assert_eq!(drawn.len(), 1, "one interior vertex, at the arc's end");
    point(first_two(drawn[0]).0, [90.0, 50.0], "where the arc ends");
}

/// A closed subpath's first vertex arrives along the closing segment, so a
/// corner marker there bisects the corner.
///
/// The square runs right, down, left and up, closing upward into (0, 0) and
/// leaving rightward — so the bisector of -90° and 0° is -45°.
#[test]
fn a_closed_subpaths_first_vertex_bisects_its_corner() {
    let markup = format!(
        "{SVG}<defs>{}</defs><path d=\"M0 0 L10 0 L10 10 L0 10 Z\" \
         marker-start=\"url(#m)\"/></svg>",
        arrow("auto")
    );
    let scene = scene(&markup);
    let (at, along) = first_two(instances(&scene)[0]);
    let half = 5.0 * core::f64::consts::FRAC_1_SQRT_2;
    point(at, [0.0, 0.0], "the start");
    point(along, [half, -half], "turned to -45 degrees");
}

/// The user agent's `marker { overflow: hidden }`: an instance is clipped to
/// its viewport unless the marker says `overflow="visible"`.
#[test]
fn a_marker_is_clipped_to_its_viewport_by_default() {
    let markup = format!(
        "{SVG}<defs>\
         <marker id=\"c\" markerUnits=\"userSpaceOnUse\" markerWidth=\"4\" markerHeight=\"2\">\
         <path d=\"M0 0 L9 0\"/></marker>\
         <marker id=\"o\" markerUnits=\"userSpaceOnUse\" overflow=\"visible\"><path d=\"M0 0 L9 0\"/></marker>\
         </defs><line x1=\"20\" y1=\"20\" x2=\"60\" y2=\"20\" \
         marker-start=\"url(#c)\" marker-end=\"url(#o)\"/></svg>"
    );
    let scene = scene(&markup);
    let Some(Node::Group {
        clip: Some(clip), ..
    }) = scene.nodes.get(1)
    else {
        panic!("a clipped instance: {:?}", scene.nodes.get(1));
    };
    let corners: Vec<[f64; 2]> = clip
        .outline
        .segments
        .iter()
        .filter_map(|segment| match segment {
            Segment::Move(p) | Segment::Line(p) => Some(*p),
            _ => None,
        })
        .collect();
    point(
        corners[0],
        [20.0, 20.0],
        "the viewport's corner on the vertex",
    );
    point(corners[2], [24.0, 22.0], "four wide and two high");
    assert!(
        matches!(scene.nodes.get(2), Some(Node::Path { .. })),
        "and the visible one is drawn as it is: {:?}",
        scene.nodes.get(2)
    );
}

/// §11.6.2: a marker's properties come from **its own ancestors**, not from
/// the shape that references it.
///
/// The shape fills red and the marker sits in a `<defs>` under a blue `<g>`,
/// so the marker's content is blue — and a build that inherited from the
/// shape would paint it red.
#[test]
fn a_markers_style_comes_from_its_own_ancestry() {
    let markup = format!(
        "{SVG}<g fill=\"#0000ff\"><defs>\
         <marker id=\"b\" overflow=\"visible\"><rect width=\"1\" height=\"1\"/></marker>\
         </defs></g><line x1=\"0\" y1=\"0\" x2=\"10\" y2=\"0\" fill=\"#ff0000\" \
         marker-end=\"url(#b)\"/></svg>"
    );
    let scene = scene(&markup);
    let Node::Path { fill, .. } = instances(&scene)[0] else {
        panic!("the marker's rectangle");
    };
    let Paint::Solid(colour) = fill else {
        panic!("a flat fill: {fill:?}");
    };
    assert_eq!(colour.rgb, [0.0, 0.0, 1.0], "the marker's ancestor's blue");
}

/// A marker whose content carries the same marker is the `<use>` bomb, and is
/// refused by the same rule.
#[test]
fn a_marker_that_reaches_itself_is_refused() {
    let markup = format!(
        "{SVG}<defs><marker id=\"r\"><line x1=\"0\" y1=\"0\" x2=\"1\" y2=\"0\" \
         marker-end=\"url(#r)\"/></marker></defs>\
         <line x1=\"0\" y1=\"0\" x2=\"10\" y2=\"0\" marker-end=\"url(#r)\"/></svg>"
    );
    assert_eq!(
        tinker_pdf_svg::read(markup.as_bytes(), None, &Limits::DEFAULT),
        Err(Refusal::TooManyUses)
    );
}

/// A marker reference naming nothing draws the shape without it, and says so.
#[test]
fn a_marker_naming_nothing_is_named() {
    let markup = format!(
        "{SVG}<line x1=\"0\" y1=\"0\" x2=\"10\" y2=\"0\" stroke=\"black\" \
         marker-end=\"url(#missing)\"/></svg>"
    );
    let scene = scene(&markup);
    assert_eq!(scene.nodes.len(), 1, "the line, alone");
    assert_eq!(scene.warnings, [Warning::MarkerUnresolved]);
}

/// SVG 2's `auto-start-reverse` turns the start marker half a turn, so one
/// arrowhead points outward at both ends of a line.
#[test]
fn auto_start_reverse_turns_the_start_around() {
    let markup = format!(
        "{SVG}<defs>{}</defs><line x1=\"20\" y1=\"20\" x2=\"60\" y2=\"20\" \
         marker-start=\"url(#m)\" marker-end=\"url(#m)\"/></svg>",
        arrow("auto-start-reverse")
    );
    let scene = scene(&markup);
    let drawn = instances(&scene);
    let (start, start_head) = first_two(drawn[0]);
    let (end, end_head) = first_two(drawn[1]);
    near(
        start_head[0] - start[0],
        -5.0,
        "the start points back along the line",
    );
    near(end_head[0] - end[0], 5.0, "the end points forward");
}

/// Markers are painted **after** the shape, and they are part of its
/// rendering: inherited from a `<g>`, and inside the shape's own opacity.
#[test]
fn markers_paint_after_the_shape_and_inside_its_opacity() {
    let markup = format!(
        "{SVG}<defs>{}</defs><g marker-end=\"url(#m)\">\
         <line x1=\"0\" y1=\"0\" x2=\"10\" y2=\"0\" stroke=\"black\" opacity=\"0.5\"/></g></svg>",
        arrow("0")
    );
    let scene = scene(&markup);
    let [Node::Group { nodes, opacity, .. }] = &scene.nodes[..] else {
        panic!("one group for the line and its marker: {:?}", scene.nodes);
    };
    near(*opacity, 0.5, "the line's own opacity, over both");
    assert_eq!(nodes.len(), 2, "{nodes:?}");
    let Node::Path { stroke, .. } = &nodes[0] else {
        panic!("the line first");
    };
    assert!(stroke.is_some(), "the stroked line is painted first");
    point(
        first_two(&nodes[1]).0,
        [10.0, 0.0],
        "then the marker at its end",
    );
}
