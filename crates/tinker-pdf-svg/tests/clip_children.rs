//! §14.3.5's clipping-path content model past plain shapes: a `<use>` of a
//! shape, and `<text>` — directly or through a `<use>`.
//!
//! A `<use>` of a shape is that shape, placed by §5.6's rule, and joins the
//! clip's outline. Text cannot: a glyph's outline is a font's (ruling 8), so a
//! clip that holds text is drawn as the mask of its silhouettes — every shape
//! and run in white on a mask's black, the union §14.3.5 asks for.
//!
//! # Counted injection
//!
//! Counted over this file and `crates/tinker-pdf/tests/epub_svg.rs`.
//!
//! | Defect injected | Tests that failed |
//! | --- | ---: |
//! | a `<use>` in a clip is skipped, as it was | 4 |
//! | the `<use>`'s `x` and `y` are not applied | 3 |
//! | the referenced shape's own `transform` is not applied | 1 |
//! | a child the content model refuses is not named | 1 |
//! | a clip's text is skipped, as it was | 6 |
//! | a clip's silhouette keeps the run's own paint | 1 |
//! | a clip's text inherits from the clipped element | 2 |
//! | a `<use>` of text is walked from the `<clipPath>`'s style | 1 |
//! | the clip cycle guard is removed | 1 |
//! | a clip's segments are not spent | 1 |
//! | an element's own mask is lost under a text clip | 1 |
//! | the silhouette mask is bounded by an empty region | 3 |
//! | the clip's shapes are left out of the silhouette | 2 |

use tinker_pdf_svg::path::Segment;
use tinker_pdf_svg::{Colour, Limits, Mask, Node, Paint, Refusal, Scene, Warning};

fn read(markup: &str, limits: &Limits) -> Result<Scene, Refusal> {
    tinker_pdf_svg::read(
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" \
             xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"100\" height=\"100\">{markup}</svg>"
        )
        .as_bytes(),
        Some((100.0, 100.0)),
        limits,
    )
}

fn scene(markup: &str) -> Scene {
    read(markup, &Limits::DEFAULT).expect("the document reads")
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

/// The outline of the clip on a scene's one path.
fn clip_span(scene: &Scene) -> [f64; 4] {
    match &scene.nodes[..] {
        [Node::Path {
            clip: Some(clip), ..
        }] => span(&clip.outline),
        other => panic!("one clipped path: {other:?}"),
    }
}

/// The silhouette mask on a scene's one group, and what it masks.
fn silhouette(scene: &Scene) -> (&[Node], &Mask) {
    match &scene.nodes[..] {
        [Node::Group {
            nodes,
            mask: Some(mask),
            clip: None,
            ..
        }] => (nodes, mask),
        other => panic!("one masked group: {other:?}"),
    }
}

const WHITE: Paint = Paint::Solid(Colour { rgb: [1.0; 3] });

// ---- `<use>` -------------------------------------------------------------------

/// §5.6 places a `<use>` as its `transform`, then a translation by `x` and
/// `y`, then the referenced element's own `transform`. A ten-unit square
/// moved one along by its own transform is (1, 0)–(11, 10); the `<use>`'s
/// `x="5" y="7"` makes it (6, 7)–(16, 17), and its `scale(2)` (12, 14)–(32, 34).
#[test]
fn a_use_in_a_clip_is_the_shape_it_names_where_it_places_it() {
    let scene = scene(
        "<defs><rect id=\"r\" width=\"10\" height=\"10\" transform=\"translate(1, 0)\"/></defs>\
         <clipPath id=\"c\"><use xlink:href=\"#r\" x=\"5\" y=\"7\" transform=\"scale(2)\"/></clipPath>\
         <rect width=\"100\" height=\"100\" clip-path=\"url(#c)\"/>",
    );
    assert_eq!(clip_span(&scene), [12.0, 14.0, 32.0, 34.0]);
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
}

/// A `<use>` naming a group — an indirect reference, which §14.3.5 calls an
/// error — and a `<g>` child add nothing, and are named; the rest of the clip
/// still applies.
#[test]
fn a_child_the_content_model_refuses_adds_nothing_and_is_named() {
    let scene = scene(
        "<defs><g id=\"g\"><rect width=\"90\" height=\"90\"/></g></defs>\
         <clipPath id=\"c\"><rect width=\"5\" height=\"5\"/><use href=\"#g\"/>\
         <g><rect width=\"80\" height=\"80\"/></g></clipPath>\
         <rect width=\"100\" height=\"100\" clip-path=\"url(#c)\"/>",
    );
    assert_eq!(clip_span(&scene), [0.0, 0.0, 5.0, 5.0]);
    assert!(
        scene.warnings.contains(&Warning::ClipChildIgnored),
        "{:?}",
        scene.warnings
    );
    assert!(
        !scene.warnings.contains(&Warning::ClipPathUnsupported),
        "the clip was found: {:?}",
        scene.warnings
    );
}

/// A clip is rebuilt for every element that names it, and a `<use>` makes one
/// path count once per use — so its segments are spent from the document's
/// budget like any drawn path's. Three uses of a ten-segment path are thirty;
/// the clipped square spends five of its own.
#[test]
fn a_clips_segments_are_spent_from_the_documents_budget() {
    let markup =
        "<defs><path id=\"p\" d=\"M0 0 L1 0 L1 1 L2 1 L2 2 L3 2 L3 3 L4 3 L4 4 Z\"/></defs>\
         <clipPath id=\"c\"><use href=\"#p\"/><use href=\"#p\"/><use href=\"#p\"/></clipPath>\
         <rect width=\"100\" height=\"100\" clip-path=\"url(#c)\"/>";
    let mut limits = Limits::DEFAULT;
    limits.max_segments = 34;
    assert_eq!(read(markup, &limits), Err(Refusal::TooManySegments));
    limits.max_segments = 35;
    assert!(read(markup, &limits).is_ok());
}

// ---- `<text>` ------------------------------------------------------------------

/// A clip of text masks the element by the text's silhouette: the run, white,
/// opaque and unstroked whatever it was painted with, and a mask with no
/// region of its own. The clipped rectangle keeps its own paint and no clip.
#[test]
fn a_clip_of_text_is_a_mask_of_its_silhouette() {
    let scene = scene(
        "<clipPath id=\"c\"><text x=\"10\" y=\"20\" font-size=\"12\" fill=\"none\" \
         stroke=\"red\" fill-opacity=\"0.2\">Hi</text></clipPath>\
         <rect width=\"100\" height=\"100\" fill=\"blue\" clip-path=\"url(#c)\"/>",
    );
    let (nodes, mask) = silhouette(&scene);
    assert_eq!(mask.region, None, "bounded by the backdrop alone");
    let [Node::Text {
        text,
        anchor,
        fill,
        fill_opacity,
        stroke,
        ..
    }] = &mask.nodes[..]
    else {
        panic!("one run in the mask: {:?}", mask.nodes);
    };
    assert_eq!(text, "Hi");
    assert_eq!(*anchor, Some([10.0, 20.0]));
    assert_eq!(*fill, WHITE);
    assert_eq!(*fill_opacity, 1.0);
    assert!(stroke.is_none());
    let [Node::Path {
        fill: Paint::Solid(Colour { rgb }),
        clip: None,
        ..
    }] = nodes
    else {
        panic!("the rectangle, unclipped, under the mask: {nodes:?}");
    };
    assert_eq!(*rgb, [0.0, 0.0, 1.0]);
}

/// §14.3.5's clip is the **union** of its children: a clip of a shape and a
/// run is one mask of both, the shape's outline white beside the run.
#[test]
fn a_clip_of_shapes_and_text_masks_by_their_union() {
    let scene = scene(
        "<clipPath id=\"c\"><rect width=\"50\" height=\"100\"/><text x=\"60\" y=\"50\">A</text>\
         </clipPath><g clip-path=\"url(#c)\"><rect width=\"100\" height=\"100\"/></g>",
    );
    let (_, mask) = silhouette(&scene);
    let [Node::Path { outline, fill, .. }, Node::Text { .. }] = &mask.nodes[..] else {
        panic!("the shapes, then the run: {:?}", mask.nodes);
    };
    assert_eq!(span(outline), [0.0, 0.0, 50.0, 100.0]);
    assert_eq!(*fill, WHITE);
}

/// §14.3.5: a child *"made invisible by … visibility"* does not contribute to
/// the clip. A hidden run in a clip's text is laid out — the run after it is
/// placed past it — and is no silhouette: it stays hidden and unpainted.
#[test]
fn a_hidden_run_in_a_clip_is_no_silhouette() {
    let scene = scene(
        "<clipPath id=\"c\"><text x=\"10\" y=\"20\"><tspan visibility=\"hidden\">A</tspan>B\
         </text></clipPath><rect width=\"100\" height=\"100\" clip-path=\"url(#c)\"/>",
    );
    let (_, mask) = silhouette(&scene);
    let [Node::Text {
        text: first,
        hidden: true,
        fill: hidden_fill,
        ..
    }, Node::Text {
        text: second,
        hidden: false,
        fill,
        ..
    }] = &mask.nodes[..]
    else {
        panic!("the hidden run, then the drawn one: {:?}", mask.nodes);
    };
    assert_eq!((first.as_str(), second.as_str()), ("A", "B"));
    assert_eq!(*hidden_fill, Paint::None);
    assert_eq!(*fill, WHITE);
}

/// §14.3.5: *"properties inherit into the 'clipPath' element from its
/// ancestors; properties do not inherit from the element referencing the
/// 'clipPath' element"*.
#[test]
fn a_clips_text_inherits_from_the_clip_paths_ancestry() {
    let scene = scene(
        "<g font-size=\"40\"><clipPath id=\"c\"><text y=\"50\">A</text></clipPath></g>\
         <g font-size=\"7\"><rect width=\"10\" height=\"10\" clip-path=\"url(#c)\"/></g>",
    );
    let (_, mask) = silhouette(&scene);
    let [Node::Text { font, .. }] = &mask.nodes[..] else {
        panic!("one run: {:?}", mask.nodes);
    };
    assert_eq!(font.size, 40.0);
}

/// A `<use>` of text in a clip places the run by §5.6's rule and styles it
/// from the `<use>`, which is where the generated content sits.
#[test]
fn a_use_of_text_in_a_clip_is_placed_and_styled_by_the_use() {
    let scene = scene(
        "<defs><text id=\"t\" x=\"1\" y=\"2\">A</text></defs>\
         <clipPath id=\"c\"><use href=\"#t\" x=\"10\" y=\"20\" font-size=\"30\"/></clipPath>\
         <rect width=\"100\" height=\"100\" clip-path=\"url(#c)\"/>",
    );
    let (_, mask) = silhouette(&scene);
    let [Node::Text {
        anchor,
        matrix,
        font,
        ..
    }] = &mask.nodes[..]
    else {
        panic!("one run: {:?}", mask.nodes);
    };
    assert_eq!(*anchor, Some([1.0, 2.0]));
    assert_eq!(*matrix, [1.0, 0.0, 0.0, 1.0, 10.0, 20.0]);
    assert_eq!(font.size, 30.0);
}

/// An element with a text clip and a mask of its own is masked by both: a
/// group holds one mask, so the element's own is a group inside the
/// silhouette's.
#[test]
fn a_text_clip_and_a_mask_both_apply() {
    let scene = scene(
        "<mask id=\"m\"><rect width=\"100\" height=\"100\" fill=\"white\"/></mask>\
         <clipPath id=\"c\"><text y=\"50\">A</text></clipPath>\
         <rect width=\"100\" height=\"100\" clip-path=\"url(#c)\" mask=\"url(#m)\"/>",
    );
    let (nodes, outer) = silhouette(&scene);
    assert_eq!(outer.region, None);
    let [Node::Group {
        mask: Some(inner), ..
    }] = nodes
    else {
        panic!("the element's own mask inside: {nodes:?}");
    };
    assert!(inner.region.is_some(), "the `<mask>`'s, with its region");
}

/// A clip whose text wears the same clip would expand without end: refused,
/// as the `<use>` bomb is.
#[test]
fn a_clip_whose_text_wears_it_is_refused() {
    let markup = "<clipPath id=\"c\"><text y=\"50\" clip-path=\"url(#c)\">A</text></clipPath>\
         <rect width=\"10\" height=\"10\" clip-path=\"url(#c)\"/>";
    assert_eq!(read(markup, &Limits::DEFAULT), Err(Refusal::TooManyUses));
}
