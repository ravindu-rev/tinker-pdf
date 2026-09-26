//! Milestone 4: §13.2's gradients and §14.3's clipping.
//!
//! # Counted injection
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | a backward stop offset is sorted rather than adjusted | 1 |
//! | `gradientUnits` is baked into the coordinates | 1 |
//! | the `xlink:href` chain is not followed for stops | 5 |
//! | the `xlink:href` chain is not followed for attributes | 1 |
//! | the reference-chain cap is raised from ten to a hundred | 1 |
//! | `fx`/`fy` default to the initial centre, not the resolved one | 1 |
//! | a gradient with no stops falls through to the paint's fallback | 2 |
//! | a single stop is not doubled into a ramp | 1 |
//! | an unsupported `spreadMethod` is silent | 1 |
//! | `stop-color` inherits from an ancestor | 2 |
//! | `clip-path` inherits | 1 |
//! | a clip path intersects its children rather than unioning them | 1 |
//! | `clipPathUnits` is ignored | 1 |
//! | an empty `<clipPath>` is no clip at all | 1 |
//! | a `clip-path` naming nothing is silent | 1 |
//! | `clip-rule` reads `fill-rule` | 1 |
//! | a `<clipPath>` outside `<defs>` is drawn where it stands | 6 |
//!
//! Seventeen injections and **no zeros — after three were found and fixed**,
//! which is the whole reason the matrix is run rather than reasoned about:
//!
//! - Every gradient in the fixture stated its own geometry, so the chain walk
//!   for *attributes* was never exercised. `#twice-removed` states nothing at
//!   all and is two hops from its stops.
//! - Every `<clipPath>` was inside a `<defs>`, which is not walked into for its
//!   own reason — so "a `<clipPath>` draws nothing where it stands" was being
//!   proved by `<defs>`. One of them is now at the top level.
//! - The `xlink:href` cycle guard was **two rules**: a `contains` check and a
//!   length cap. Removing the first fired nothing, because the second already
//!   terminates the walk and the repeated entries resolve identically. The
//!   `contains` check is gone and the cap is asserted directly.

use tinker_pdf_svg::{Clip, Colour, FillRule, Limits, Node, Paint, Scene, Stop, Warning};

const GRADIENTS: &[u8] = include_bytes!("fixtures/gradients.svg");
const CLIPPING: &[u8] = include_bytes!("fixtures/clipping.svg");

fn scene(bytes: &[u8]) -> Scene {
    tinker_pdf_svg::read(bytes, Some((100.0, 100.0)), &Limits::DEFAULT).expect("the fixture reads")
}

fn fill(scene: &Scene, at: usize) -> Paint {
    match &scene.nodes[at] {
        Node::Path { fill, .. } => fill.clone(),
        other => panic!("{other:?}"),
    }
}

fn clip(scene: &Scene, at: usize) -> Option<Clip> {
    match &scene.nodes[at] {
        Node::Path { clip, .. } => clip.clone(),
        other => panic!("{other:?}"),
    }
}

fn near(left: f64, right: f64, what: &str) {
    assert!((left - right).abs() < 1e-9, "{what}: {left} is not {right}");
}

fn rgb(r: u8, g: u8, b: u8) -> Colour {
    Colour {
        rgb: [
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
        ],
    }
}

// ---- §13.2's ramp -------------------------------------------------------------

/// §13.2.4's stops, including the offset rule a first build sorts instead.
///
/// *"Each gradient offset value is required to be equal to or greater than the
/// previous"*, and a smaller one is **adjusted to be equal** — not moved. A
/// build that sorted the list would put the blue stop between the red and the
/// green, which is a different ramp and a plausible-looking one.
#[test]
fn the_stops_are_read_and_a_backward_offset_is_adjusted_not_sorted() {
    let scene = scene(GRADIENTS);
    let Paint::Linear { stops, .. } = fill(&scene, 0) else {
        panic!("a linear gradient: {:?}", fill(&scene, 0));
    };
    assert_eq!(stops.len(), 3);
    assert_eq!(stops[0].colour, rgb(255, 0, 0));
    near(stops[0].offset, 0.0, "the first offset");
    assert_eq!(stops[1].colour, rgb(0, 255, 0));
    near(stops[1].offset, 0.5, "the second");
    near(stops[1].opacity, 0.25, "and its `stop-opacity`");
    assert_eq!(
        stops[2].colour,
        rgb(0, 0, 255),
        "the blue stop keeps its place in the list"
    );
    near(
        stops[2].offset,
        0.5,
        "and its offset is raised to the previous",
    );
}

/// §13.2.3's `objectBoundingBox`, which is the initial value.
///
/// The rectangle is ten by twenty at (10, 20), so the unit square maps onto it
/// with a matrix that is neither a translation nor a uniform scale — which is
/// what tells a build that baked the units into the coordinates from one that
/// carried them.
#[test]
fn a_bounding_box_gradient_maps_the_unit_square_onto_the_shape() {
    let scene = scene(GRADIENTS);
    let Paint::Linear {
        from, to, matrix, ..
    } = fill(&scene, 0)
    else {
        panic!("a linear gradient");
    };
    // §13.2.3's initial geometry: (0,0) to (100%,0), which is (1,0) here.
    near(from[0], 0.0, "x1");
    near(to[0], 1.0, "x2 is 100% of the box");
    near(to[1], 0.0, "y2");
    // And the matrix is what makes those two the shape's left and right edges.
    let start = tinker_pdf_svg::transform::apply(matrix, from);
    let end = tinker_pdf_svg::transform::apply(matrix, to);
    near(start[0], 10.0, "the box's left edge");
    near(start[1], 20.0, "and its top");
    near(end[0], 20.0, "the box's right edge");
    near(end[1], 20.0, "at the same height");
}

/// §13.2.3's `xlink:href`: geometry from one element, stops from another.
///
/// This is how every gradient in the fetched corpus is written, so a build
/// that read only the referencing element would draw all sixteen of
/// `cover.svg`'s gradients as nothing.
#[test]
fn a_paint_server_takes_its_stops_from_the_one_it_references() {
    let scene = scene(GRADIENTS);
    let Paint::Linear {
        from,
        to,
        matrix,
        stops,
    } = fill(&scene, 1)
    else {
        panic!("a linear gradient: {:?}", fill(&scene, 1));
    };
    assert_eq!(stops.len(), 3, "the stops came from `#ramp`");
    near(from[0], 2.0, "and the geometry from `#borrowed`");
    near(from[1], 3.0, "y1");
    near(to[0], 4.0, "x2");
    near(to[1], 5.0, "y2");
    // `userSpaceOnUse` means the element's own space, which here is the
    // identity — so the matrix moves nothing.
    let start = tinker_pdf_svg::transform::apply(matrix, from);
    near(start[0], 2.0, "user space is the element's own");
    near(start[1], 3.0, "on both axes");
}

/// Every attribute is looked up along the **same** chain, however many hops.
///
/// `#twice-removed` states nothing at all: its geometry and its
/// `gradientUnits` come from `#borrowed` one hop away and its stops from
/// `#ramp` two hops away. A build that read only the referenced element's
/// *stops* — which is the half a first implementation does, because that is
/// the half that is visibly missing — would place this gradient at §13.2.3's
/// initial geometry in bounding-box units, which is a ramp across the shape
/// instead of the two-user-unit one the file describes.
#[test]
fn every_attribute_is_looked_up_along_the_same_chain() {
    let scene = scene(GRADIENTS);
    let Paint::Linear {
        from, to, stops, ..
    } = fill(&scene, 7)
    else {
        panic!("a linear gradient: {:?}", fill(&scene, 7));
    };
    assert_eq!(stops.len(), 3, "the stops, two hops away");
    near(from[0], 2.0, "x1, one hop away");
    near(from[1], 3.0, "y1");
    near(to[0], 4.0, "x2");
    near(to[1], 5.0, "y2");
}

/// The reference chain stops at its cap, and the cap is the cycle guard.
///
/// **This is the only guard**, and the injection matrix is why: a `contains`
/// check beside it fired nothing, because a cycle already terminates here and
/// the repeated entries resolve to the same attributes and the same stops. A
/// rule enforced twice hides the reachable half.
#[test]
fn a_reference_chain_stops_at_the_cap() {
    let mut markup =
        String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><defs>");
    // Twelve links, and only the last one has any stops.
    for index in 0..12 {
        markup.push_str(&format!(
            "<linearGradient id=\"g{index}\" xlink:href=\"#g{}\"/>",
            index + 1
        ));
    }
    markup.push_str(
        "<linearGradient id=\"g12\"><stop offset=\"0\" stop-color=\"red\"/></linearGradient>",
    );
    markup.push_str("</defs><rect fill=\"url(#g0)\" width=\"1\" height=\"1\"/></svg>");
    let long = scene(markup.as_bytes());
    assert_eq!(
        fill(&long, 0),
        Paint::None,
        "the stops are thirteen hops away and the cap is ten"
    );

    // And a chain inside the cap does reach them, so the cap is a cap rather
    // than a chain that never walks.
    let mut short = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"><defs>",
    );
    for index in 0..8 {
        short.push_str(&format!(
            "<linearGradient id=\"g{index}\" xlink:href=\"#g{}\"/>",
            index + 1
        ));
    }
    short.push_str(
        "<linearGradient id=\"g8\"><stop offset=\"0\" stop-color=\"red\"/></linearGradient>",
    );
    short.push_str("</defs><rect fill=\"url(#g0)\" width=\"1\" height=\"1\"/></svg>");
    let short = scene(short.as_bytes());
    assert!(
        matches!(fill(&short, 0), Paint::Linear { .. }),
        "nine hops is inside the cap"
    );
}

/// A reference cycle between two paint servers terminates.
///
/// `<linearGradient id="a" xlink:href="#b"/>` and the mirror of it is
/// well-formed XML, and a walk with no guard does not return. The guard is
/// structural — a chain that reaches somewhere it has been stops — rather than
/// a counter, so it fires on the first repeat rather than on the tenth.
#[test]
fn a_cycle_between_paint_servers_terminates() {
    let scene = scene(GRADIENTS);
    // Neither element has a stop, so §13.2.4's answer is `none` — and the
    // point of the test is that there is an answer at all.
    assert_eq!(fill(&scene, 3), Paint::None);
}

/// §13.2.3's radial geometry, with `fx`/`fy` defaulting to the **resolved**
/// centre rather than to the initial one.
#[test]
fn a_radial_gradient_focuses_on_its_own_centre_by_default() {
    let scene = scene(GRADIENTS);
    let Paint::Radial {
        centre,
        radius,
        focus,
        ..
    } = fill(&scene, 2)
    else {
        panic!("a radial gradient: {:?}", fill(&scene, 2));
    };
    near(centre[0], 0.25, "cx as a fraction of the box");
    near(centre[1], 0.75, "cy");
    near(radius, 0.5, "r");
    assert_eq!(
        focus, centre,
        "§13.2.3: `fx` and `fy` default to the centre that was resolved, not \
         to the one that was initial"
    );
}

/// §13.2.4's two degenerate ramps, each with its own answer.
#[test]
fn a_gradient_with_no_stops_and_one_with_a_single_stop() {
    let scene = scene(GRADIENTS);
    assert_eq!(
        fill(&scene, 5),
        Paint::None,
        "§13.2.4: no stops paints as if `none` were specified — which is not \
         the same as falling through to the `teal` fallback, because the \
         server was found"
    );
    let Paint::Linear { stops, .. } = fill(&scene, 6) else {
        panic!("a linear gradient: {:?}", fill(&scene, 6));
    };
    assert_eq!(stops.len(), 2, "one stop is a ramp from it to itself");
    assert_eq!(stops[0].colour, rgb(0x12, 0x34, 0x56));
    assert_eq!(stops[1].colour, stops[0].colour);
    near(stops[1].offset, 1.0, "and it reaches the end");
}

/// §13.2.3's `reflect` and `repeat` are refused by name and drawn as `pad`.
#[test]
fn an_unsupported_spread_method_is_named_and_padded() {
    let scene = scene(GRADIENTS);
    assert!(
        scene.warnings.contains(&Warning::SpreadMethodUnsupported),
        "{:?}",
        scene.warnings
    );
    assert!(
        matches!(fill(&scene, 4), Paint::Linear { .. }),
        "and the gradient still draws"
    );
}

/// A `<stop>` inherits its `stop-color` from nothing above the gradient.
///
/// `stop-color` is **not** an inherited property, so a `<g stop-color="red">`
/// around a shape must not colour a gradient's stops — which is what a build
/// whose `inherit()` cloned every field would do.
#[test]
fn stop_colour_does_not_inherit_from_an_ancestor() {
    let markup = br##"<svg xmlns="http://www.w3.org/2000/svg">
      <defs><linearGradient id="g"><stop offset="0"/><stop offset="1"/></linearGradient></defs>
      <g stop-color="#ff0000" stop-opacity="0.5">
        <rect fill="url(#g)" width="10" height="10"/>
      </g></svg>"##;
    let scene = scene(markup);
    let Paint::Linear { stops, .. } = fill(&scene, 0) else {
        panic!("a linear gradient: {:?}", fill(&scene, 0));
    };
    assert_eq!(
        stops[0],
        Stop {
            offset: 0.0,
            colour: rgb(0, 0, 0),
            opacity: 1.0,
        },
        "§13.2.4's initial `stop-color` is black at full opacity"
    );
}

// ---- §14.3's clipping ---------------------------------------------------------

/// §14.3.5: a clipping path is the **union** of its children's silhouettes.
#[test]
fn a_clip_path_is_the_union_of_its_children() {
    let scene = scene(CLIPPING);
    let clip = clip(&scene, 0).expect("a clip");
    // Two rectangles, five segments each.
    assert_eq!(clip.outline.segments.len(), 10, "{:?}", clip.outline);
    assert_eq!(clip.rule, FillRule::NonZero);
}

/// §14.3.4's `objectBoundingBox` units, which are a fraction of the *clipped*
/// element's box.
#[test]
fn a_bounding_box_clip_is_a_fraction_of_the_element_it_clips() {
    let scene = scene(CLIPPING);
    let clip = clip(&scene, 1).expect("a clip");
    // The rectangle is ten by twenty at (10, 20), so `width="0.5"` is five
    // user units and the clip runs from x = 10 to x = 15.
    let box_ = bounds(&clip.outline);
    near(box_[0], 10.0, "the clip's left");
    near(box_[2], 15.0, "half the width");
    near(box_[1], 20.0, "the top");
    near(box_[3], 40.0, "the whole height");
}

/// §14.3.5: an **empty** `<clipPath>` clips everything away, and a `clip-path`
/// naming **nothing** is a reference this build could not use.
///
/// The two are different documents and this build gives them different
/// answers. Returning "no clip" for both would draw the first element
/// unclipped, which is the opposite of what §14.3.5 says.
#[test]
fn an_empty_clip_path_and_a_missing_one_are_different_answers() {
    let scene = scene(CLIPPING);
    let empty = clip(&scene, 2).expect("an empty clip is still a clip");
    assert!(
        empty.outline.segments.is_empty(),
        "and it clips everything away: {:?}",
        empty.outline
    );
    assert!(
        clip(&scene, 3).is_none(),
        "a reference to nothing is not a clip"
    );
    assert!(
        scene.warnings.contains(&Warning::ClipPathUnsupported),
        "and it is named: {:?}",
        scene.warnings
    );
}

/// `clip-rule` is its own property and reaches the clip.
#[test]
fn clip_rule_is_separate_from_fill_rule() {
    let scene = scene(CLIPPING);
    let clip = clip(&scene, 4).expect("a clip");
    assert_eq!(clip.rule, FillRule::EvenOdd);
    let Node::Path { rule, .. } = &scene.nodes[4] else {
        panic!("a path");
    };
    assert_eq!(
        *rule,
        FillRule::NonZero,
        "and the fill rule is untouched by it"
    );
}

/// A `<clipPath>` outside `<defs>` draws nothing where it stands.
///
/// §14.3 says a clipping path is never rendered directly, wherever it sits,
/// and putting one at the top level is legal and common. A build that walked
/// into it would draw its children as shapes — a circle over the drawing that
/// belongs to nothing in the picture.
#[test]
fn a_clip_path_outside_defs_still_draws_nothing() {
    let scene = scene(CLIPPING);
    // Six rectangles in the fixture and nothing else; the `<circle>` inside
    // `#loose` is not one of them.
    assert_eq!(scene.nodes.len(), 6, "{:?}", scene.nodes.len());
}

/// §14.3: `clip-path` is not inherited.
///
/// A child of a clipped group is clipped **by the group's own rendering**, not
/// by the same path applied again — and applying it again is the same picture
/// until the child moves, which is why only a test catches it.
#[test]
fn clip_path_does_not_inherit() {
    let scene = scene(CLIPPING);
    assert!(
        clip(&scene, 5).is_none(),
        "the child of a clipped group carries no clip of its own"
    );
}

/// Every point an outline visits.
fn bounds(outline: &tinker_pdf_svg::path::Outline) -> [f64; 4] {
    use tinker_pdf_svg::path::Segment;
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
