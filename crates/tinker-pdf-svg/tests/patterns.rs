//! §13.3's `<pattern>` as a paint: a tile of nodes, repeated.
//!
//! Every expected number is arithmetic from §13.3's units — the tile in the
//! element's user space or as a fraction of its box, its content's origin at
//! the tile's corner — written beside the assertion.
//!
//! # Counted injection
//!
//! Counted over this file and `crates/tinker-pdf/tests/epub_svg.rs`.
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | the tile is taken in user space under `objectBoundingBox` | 2 |
//! | the content's origin is pattern space's, not the tile's corner | 2 |
//! | `patternContentUnits="objectBoundingBox"` is ignored | 1 |
//! | `patternTransform` is not composed | 1 |
//! | the `xlink:href` chain is not followed | 1 |
//! | the pattern cycle guard is removed | 1 |
//! | the tile's content inherits from the painted element | 6 |
//! | the writer's tile does not repeat at its own size | 1 |

use tinker_pdf_svg::path::Segment;
use tinker_pdf_svg::{Colour, Limits, Node, Paint, Refusal, Scene, Tile};

fn scene(markup: &str) -> Scene {
    tinker_pdf_svg::read(
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" \
             xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"100\" height=\"100\">{markup}</svg>"
        )
        .as_bytes(),
        Some((100.0, 100.0)),
        &Limits::DEFAULT,
    )
    .expect("the document reads")
}

/// The tile the first path of a scene is filled with.
fn tile(scene: &Scene) -> &Tile {
    match scene.nodes.first() {
        Some(Node::Path {
            fill: Paint::Pattern(tile),
            ..
        }) => tile,
        other => panic!("a pattern fill: {other:?}"),
    }
}

/// The box a tile's first node's outline spans.
fn first_span(tile: &Tile) -> [f64; 4] {
    let Some(Node::Path { outline, .. }) = tile.nodes.first() else {
        panic!("a path in the tile: {:?}", tile.nodes);
    };
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

/// `patternUnits="userSpaceOnUse"`: the tile is where its lengths say, and
/// the content's origin is the tile's corner (§13.3: *"the new coordinate
/// system has its origin at (x, y)"*).
#[test]
fn a_user_space_tile_is_where_its_lengths_say() {
    let scene = scene(
        "<pattern id=\"p\" patternUnits=\"userSpaceOnUse\" x=\"5\" y=\"5\" width=\"10\" \
         height=\"20\"><rect width=\"5\" height=\"5\"/></pattern>\
         <rect width=\"100\" height=\"100\" fill=\"url(#p)\"/>",
    );
    let tile = tile(&scene);
    assert_eq!(tile.cell, [5.0, 5.0, 10.0, 20.0]);
    assert_eq!(
        first_span(tile),
        [5.0, 5.0, 10.0, 10.0],
        "at the tile's corner"
    );
    assert_eq!(tile.matrix, tinker_pdf_svg::transform::IDENTITY);
}

/// §13.3's initial `patternUnits` is `objectBoundingBox`: the tile is a
/// fraction of the painted element's box. A rectangle 20 by 40 at (10, 10)
/// with a tile of a half by a quarter is a tile ten by ten at its corner.
#[test]
fn a_bounding_box_tile_is_a_fraction_of_the_box() {
    let scene = scene(
        "<pattern id=\"p\" width=\"0.5\" height=\"0.25\"><rect width=\"2\" height=\"2\"/></pattern>\
         <rect x=\"10\" y=\"10\" width=\"20\" height=\"40\" fill=\"url(#p)\"/>",
    );
    let tile = tile(&scene);
    assert_eq!(tile.cell, [10.0, 10.0, 10.0, 10.0]);
    assert_eq!(
        first_span(tile),
        [10.0, 10.0, 12.0, 12.0],
        "and `userSpaceOnUse` content, from the tile's corner"
    );
}

/// `patternContentUnits="objectBoundingBox"` scales the content by the box.
#[test]
fn bounding_box_content_is_scaled_by_the_box() {
    let scene = scene(
        "<pattern id=\"p\" width=\"1\" height=\"1\" patternContentUnits=\"objectBoundingBox\">\
         <rect width=\"0.5\" height=\"0.25\"/></pattern>\
         <rect x=\"10\" y=\"10\" width=\"20\" height=\"40\" fill=\"url(#p)\"/>",
    );
    assert_eq!(
        first_span(tile(&scene)),
        [10.0, 10.0, 20.0, 20.0],
        "a half of twenty and a quarter of forty"
    );
}

/// `patternTransform` composes into the tile's matrix, inside the element's.
#[test]
fn the_pattern_transform_is_the_tiles_matrix() {
    let scene = scene(
        "<pattern id=\"p\" patternUnits=\"userSpaceOnUse\" width=\"10\" height=\"10\" \
         patternTransform=\"translate(3, 4)\"><rect width=\"1\" height=\"1\"/></pattern>\
         <g transform=\"scale(2)\"><rect width=\"10\" height=\"10\" fill=\"url(#p)\"/></g>",
    );
    assert_eq!(
        tile(&scene).matrix,
        [2.0, 0.0, 0.0, 2.0, 6.0, 8.0],
        "the translate, then the group's scale"
    );
}

/// §13.3's `xlink:href`: attributes the referencing pattern does not state, and
/// the content when it has none, come from the one it references.
#[test]
fn a_pattern_inherits_along_its_reference() {
    let scene = scene(
        "<pattern id=\"base\" patternUnits=\"userSpaceOnUse\" width=\"7\" height=\"9\">\
         <rect width=\"1\" height=\"1\"/></pattern>\
         <pattern id=\"p\" xlink:href=\"#base\" x=\"2\"/>\
         <rect width=\"100\" height=\"100\" fill=\"url(#p)\"/>",
    );
    let tile = tile(&scene);
    assert_eq!(
        tile.cell,
        [2.0, 0.0, 7.0, 9.0],
        "its own x, the rest inherited"
    );
    assert_eq!(tile.nodes.len(), 1, "and the content");
}

/// A pattern whose tile is painted with itself is refused like the `<use>`
/// bomb.
#[test]
fn a_pattern_that_paints_itself_is_refused() {
    let markup = "<svg xmlns=\"http://www.w3.org/2000/svg\">\
        <pattern id=\"p\" patternUnits=\"userSpaceOnUse\" width=\"10\" height=\"10\">\
        <rect width=\"5\" height=\"5\" fill=\"url(#p)\"/></pattern>\
        <rect width=\"10\" height=\"10\" fill=\"url(#p)\"/></svg>";
    assert_eq!(
        tinker_pdf_svg::read(markup.as_bytes(), None, &Limits::DEFAULT),
        Err(Refusal::TooManyUses)
    );
}

/// §13.3: properties inherit into a `<pattern>` from its ancestors, not from
/// the element it paints.
#[test]
fn a_tiles_content_inherits_from_its_own_ancestry() {
    let scene = scene(
        "<g fill=\"#0000ff\"><pattern id=\"p\" patternUnits=\"userSpaceOnUse\" width=\"10\" \
         height=\"10\"><rect width=\"5\" height=\"5\"/></pattern></g>\
         <rect width=\"10\" height=\"10\" fill=\"url(#p)\" stroke=\"red\"/>",
    );
    let Some(Node::Path { fill, stroke, .. }) = tile(&scene).nodes.first() else {
        panic!("the tile's rectangle");
    };
    assert_eq!(
        *fill,
        Paint::Solid(Colour {
            rgb: [0.0, 0.0, 1.0]
        })
    );
    assert!(stroke.is_none(), "and no stroke from the painted element");
}
