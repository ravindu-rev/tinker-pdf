//! §14.4's masks: a second picture whose luminance is the first one's alpha.
//!
//! Every expected number here is arithmetic from §14.4's units — the region's
//! −10%/−10%/120%/120% of the masked element's box, or lengths in its user
//! space — written beside the assertion.
//!
//! # Counted injection
//!
//! Counted over this file and `crates/tinker-pdf/tests/epub_svg.rs`, whose
//! five mask tests are the ones that reach paper.
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | the region's default is the box itself rather than 10% around it | 1 |
//! | `maskUnits="userSpaceOnUse"` is read as a fraction | 2 |
//! | `maskContentUnits="objectBoundingBox"` is ignored | 1 |
//! | the mask's content inherits from the masked element | 1 |
//! | a mask naming nothing is silent | 1 |
//! | the mask cycle guard is removed | 1 |
//! | a region with no area is no mask at all | 1 |
//! | `mask` is inherited | 1 |
//! | an image's `clip-path` is dropped, as it was | 1 |
//!
//! And for the box a mask is measured against (the review of lane 5C), counted
//! over every suite of this crate, `patterns.rs` and `gradients.rs` among them:
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | text's box is read as one of no area, as it was | 5 |
//! | text left out of a box is not named | 5 |
//! | a marked shape's mask is measured over its markers, as it was | 1 |
//! | `maskContentUnits` is not asked whether it needs the box | 1 |

use tinker_pdf_svg::path::Segment;
use tinker_pdf_svg::{Colour, Limits, Mask, Node, Paint, Refusal, Scene, Warning};

fn scene(markup: &str) -> Scene {
    tinker_pdf_svg::read(
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\">{markup}</svg>"
        )
        .as_bytes(),
        Some((100.0, 100.0)),
        &Limits::DEFAULT,
    )
    .expect("the document reads")
}

fn near(left: f64, right: f64, what: &str) {
    assert!((left - right).abs() < 1e-9, "{what}: {left} is not {right}");
}

/// The box an outline's points span, as `[min_x, min_y, max_x, max_y]`.
fn span(outline: &tinker_pdf_svg::path::Outline) -> [f64; 4] {
    let mut out = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    for segment in &outline.segments {
        if let Segment::Move(p) | Segment::Line(p) = segment {
            out = [
                out[0].min(p[0]),
                out[1].min(p[1]),
                out[2].max(p[0]),
                out[3].max(p[1]),
            ];
        }
    }
    out
}

/// The one masked group a scene holds.
fn masked(scene: &Scene) -> (&[Node], &Mask) {
    match &scene.nodes[..] {
        [Node::Group {
            nodes,
            mask: Some(mask),
            ..
        }] => (nodes, mask),
        other => panic!("one masked group: {other:?}"),
    }
}

/// §14.4's initial region: −10%, −10%, 120% and 120% of the masked element's
/// bounding box. The rectangle is 20 by 10 at (10, 10), so the region runs
/// from (8, 9) to (32, 21).
#[test]
fn the_initial_region_is_ten_percent_around_the_box() {
    let scene = scene(
        "<mask id=\"m\"><rect width=\"100\" height=\"100\" fill=\"white\"/></mask>\
         <rect x=\"10\" y=\"10\" width=\"20\" height=\"10\" mask=\"url(#m)\"/>",
    );
    let (nodes, mask) = masked(&scene);
    assert_eq!(nodes.len(), 1, "the rectangle, inside the group");
    let [x0, y0, x1, y1] = span(mask.region.as_ref().expect("a <mask> has a region"));
    near(x0, 8.0, "left");
    near(y0, 9.0, "top");
    near(x1, 32.0, "right");
    near(y1, 21.0, "bottom");
    assert_eq!(mask.nodes.len(), 1, "the mask's white rectangle");
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
}

/// `maskUnits="userSpaceOnUse"` makes the region lengths in the masked
/// element's user space, and `maskContentUnits="objectBoundingBox"` makes the
/// content a fraction of its box: a unit square of content is the box.
#[test]
fn the_two_unit_attributes_mean_what_they_say() {
    let scene = scene(
        "<mask id=\"m\" maskUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"50\" height=\"40\" \
         maskContentUnits=\"objectBoundingBox\"><rect width=\"0.5\" height=\"1\" fill=\"white\"/></mask>\
         <rect x=\"10\" y=\"10\" width=\"20\" height=\"10\" mask=\"url(#m)\"/>",
    );
    let (_, mask) = masked(&scene);
    let [x0, y0, x1, y1] = span(mask.region.as_ref().expect("a <mask> has a region"));
    assert_eq!(
        [x0, y0, x1, y1],
        [0.0, 0.0, 50.0, 40.0],
        "lengths, as written"
    );
    let Node::Path { outline, .. } = &mask.nodes[0] else {
        panic!("the mask's rectangle");
    };
    let [a, b, c, d] = span(outline);
    assert_eq!(
        [a, b, c, d],
        [10.0, 10.0, 20.0, 20.0],
        "half the box's width, its whole height, at its corner"
    );
}

/// §14.4: a mask's content inherits from **the mask's** ancestors, not from
/// the element it masks.
#[test]
fn a_masks_content_inherits_from_its_own_ancestry() {
    let scene = scene(
        "<g fill=\"#00ff00\"><mask id=\"m\"><rect width=\"1\" height=\"1\"/></mask></g>\
         <rect width=\"10\" height=\"10\" fill=\"#ff0000\" mask=\"url(#m)\"/>",
    );
    let (_, mask) = masked(&scene);
    let Node::Path { fill, .. } = &mask.nodes[0] else {
        panic!("the mask's rectangle");
    };
    assert_eq!(
        *fill,
        Paint::Solid(Colour {
            rgb: [0.0, 1.0, 0.0]
        }),
        "the mask's ancestor's green"
    );
}

/// A `mask` naming nothing draws the element unmasked and says so.
#[test]
fn a_mask_naming_nothing_is_named_and_draws_unmasked() {
    let scene = scene("<rect width=\"10\" height=\"10\" mask=\"url(#nothing)\"/>");
    assert!(
        matches!(&scene.nodes[..], [Node::Path { .. }]),
        "the rectangle, alone: {:?}",
        scene.nodes
    );
    assert_eq!(scene.warnings, [Warning::MaskUnresolved]);
}

/// A mask whose content wears the same mask is the `<use>` bomb's third
/// spelling, refused by the same rule.
#[test]
fn a_mask_that_reaches_itself_is_refused() {
    let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <mask id=\"m\"><rect width=\"1\" height=\"1\" mask=\"url(#m)\"/></mask>\
        <rect width=\"10\" height=\"10\" mask=\"url(#m)\"/></svg>";
    assert_eq!(
        tinker_pdf_svg::read(markup.as_bytes(), None, &Limits::DEFAULT),
        Err(Refusal::TooManyUses)
    );
}

/// A region with no area **masks everything away** — §14.4's answer, and not
/// the absence of a mask, which would draw the element whole.
#[test]
fn a_region_with_no_area_masks_everything() {
    let scene = scene(
        "<mask id=\"m\" width=\"0\"><rect width=\"100\" height=\"100\" fill=\"white\"/></mask>\
         <rect width=\"10\" height=\"10\" mask=\"url(#m)\"/>",
    );
    let (_, mask) = masked(&scene);
    assert_eq!(mask.region, Some(Default::default()), "{:?}", mask.region);
    assert!(mask.nodes.is_empty(), "and nothing to read luminance from");
}

/// `mask` masks the element that states it, and is not inherited: a masked
/// `<g>` is one masked group whose children carry no mask of their own.
#[test]
fn a_groups_mask_is_not_inherited() {
    let scene = scene(
        "<mask id=\"m\"><rect width=\"100\" height=\"100\" fill=\"white\"/></mask>\
         <g mask=\"url(#m)\" opacity=\"0.5\"><rect width=\"10\" height=\"10\"/>\
         <rect x=\"5\" width=\"10\" height=\"10\"/></g>",
    );
    let [Node::Group {
        nodes,
        opacity,
        mask: Some(_),
        ..
    }] = &scene.nodes[..]
    else {
        panic!("one masked group: {:?}", scene.nodes);
    };
    near(*opacity, 0.5, "and its opacity with it");
    assert_eq!(nodes.len(), 2);
    assert!(
        nodes.iter().all(|node| matches!(node, Node::Path { .. })),
        "the children are plain shapes: {nodes:?}"
    );
}

/// An `<image>`'s `clip-path` reaches the scene, as a group around the
/// picture — it used to be dropped, because an image node has no clip of its
/// own and nothing put one around it.
#[test]
fn an_images_clip_path_is_a_group_around_it() {
    let scene = scene(
        "<clipPath id=\"c\"><rect width=\"5\" height=\"5\"/></clipPath>\
         <image href=\"a.png\" width=\"10\" height=\"10\" clip-path=\"url(#c)\"/>",
    );
    let [Node::Group {
        nodes,
        clip: Some(_),
        ..
    }] = &scene.nodes[..]
    else {
        panic!("a clipped group: {:?}", scene.nodes);
    };
    assert!(matches!(&nodes[..], [Node::Image { .. }]));
}

// ---- the box a mask measures ------------------------------------------------

/// The white mask the tests below mask with: it keeps everything inside its
/// region, so only the region and the units are in question.
const WHITE: &str = "<mask id=\"m\"><rect width=\"100\" height=\"100\" fill=\"white\"/></mask>";

/// A `<text>`, or a group of only text, under a mask in the initial
/// `objectBoundingBox` units has **no box** this crate can measure — a run's
/// extent is a font metric (ruling 8). It is drawn unmasked and named, ruling
/// 2's answer; it used to be masked away by a region of no area, without a
/// word.
#[test]
fn text_under_a_bounding_box_mask_draws_unmasked_and_is_named() {
    for markup in [
        "<text x=\"10\" y=\"50\" mask=\"url(#m)\">Hi</text>",
        "<g mask=\"url(#m)\"><text x=\"10\" y=\"50\">Hi</text></g>",
        // The content's units need the box as much as the region's do.
        "<mask id=\"c\" maskUnits=\"userSpaceOnUse\" maskContentUnits=\"objectBoundingBox\">\
         <rect width=\"1\" height=\"1\" fill=\"white\"/></mask>\
         <text x=\"10\" y=\"50\" mask=\"url(#c)\">Hi</text>",
    ] {
        let scene = scene(&format!("{WHITE}{markup}"));
        assert!(
            matches!(&scene.nodes[..], [Node::Text { .. }]),
            "{markup}: the run, unmasked: {:?}",
            scene.nodes
        );
        assert_eq!(scene.warnings, [Warning::TextBoxUnmeasured], "{markup}");
    }
}

/// Lengths in user space need no box, so text under a `userSpaceOnUse` mask
/// is masked and nothing is named.
#[test]
fn text_under_a_user_space_mask_is_masked() {
    let scene = scene(
        "<mask id=\"m\" maskUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"50\" height=\"60\">\
         <rect width=\"100\" height=\"100\" fill=\"white\"/></mask>\
         <text x=\"10\" y=\"50\" mask=\"url(#m)\">Hi</text>",
    );
    let (nodes, mask) = masked(&scene);
    assert!(matches!(nodes, [Node::Text { .. }]), "{nodes:?}");
    let [x0, y0, x1, y1] = span(mask.region.as_ref().expect("a <mask> has a region"));
    assert_eq!([x0, y0, x1, y1], [0.0, 0.0, 50.0, 60.0]);
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
}

/// A group of a shape and text is masked by the box the shape spans — the
/// rectangle's 20 by 10 at (10, 10), so (8, 9) to (32, 21) — and the text
/// left out of it is named.
#[test]
fn a_group_of_text_and_a_shape_is_masked_by_the_shapes_box_and_named() {
    let scene = scene(&format!(
        "{WHITE}<g mask=\"url(#m)\"><rect x=\"10\" y=\"10\" width=\"20\" height=\"10\"/>\
         <text x=\"10\" y=\"50\">Hi</text></g>"
    ));
    let (nodes, mask) = masked(&scene);
    assert_eq!(nodes.len(), 2, "the rectangle and the run");
    let [x0, y0, x1, y1] = span(mask.region.as_ref().expect("a <mask> has a region"));
    near(x0, 8.0, "left");
    near(y0, 9.0, "top");
    near(x1, 32.0, "right");
    near(y1, 21.0, "bottom");
    assert_eq!(scene.warnings, [Warning::TextBoxUnmeasured]);
}

/// §7.11's object bounding box is the shape's geometry, and its markers are
/// not in it. A path from (10, 10) to (30, 20) with a forty-wide marker at its
/// end has the region (8, 9) to (32, 21), not one grown around the marker.
#[test]
fn a_marked_shapes_mask_region_is_its_own_box() {
    let scene = scene(&format!(
        "{WHITE}<marker id=\"a\" markerUnits=\"userSpaceOnUse\" markerWidth=\"40\" \
         markerHeight=\"40\" overflow=\"visible\"><rect width=\"40\" height=\"40\"/></marker>\
         <path d=\"M 10 10 L 30 10 L 30 20\" marker-end=\"url(#a)\" mask=\"url(#m)\"/>"
    ));
    let (nodes, mask) = masked(&scene);
    assert_eq!(nodes.len(), 2, "the path and its marker: {nodes:?}");
    let [x0, y0, x1, y1] = span(mask.region.as_ref().expect("a <mask> has a region"));
    near(x0, 8.0, "left");
    near(y0, 9.0, "top");
    near(x1, 32.0, "right");
    near(y1, 21.0, "bottom");
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
}
