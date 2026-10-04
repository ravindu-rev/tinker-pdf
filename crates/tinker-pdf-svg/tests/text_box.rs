//! A run's box, measured by the caller (the roadmap's SVG-in-the-spine row).
//!
//! §7.11's box of text is its glyph cells — each glyph's advance by the font's
//! ascent and descent (SVG 2 §8.10) — and both are a font's, which this crate
//! does not have (ruling 8). [`tinker_pdf_svg::read`] still has no box for
//! text, and a bounding-box effect on it is drawn without the effect and named
//! `TextBoxUnmeasured` (`masks.rs`, `gradients.rs`, `patterns.rs`); a caller
//! that has the metrics hands them in through
//! [`tinker_pdf_svg::Context::with_measure`], and the box is then the cells of
//! the runs **where the caller sets them**: `text-anchor`, and a run that
//! continues where the one before it ended, are applied as the facade's
//! `place_text` applies them, so the box is where the ink will be.
//!
//! The measurer here is arithmetic anyone can check: every character is half
//! an em wide, and the ascent and descent are eight and two tenths of one. A
//! twenty-unit run of four characters at (10, 50) is 40 wide, and its cells
//! span (10, 34) to (50, 54).

use tinker_pdf_svg::path::{Outline, Segment};
use tinker_pdf_svg::{
    Context, Limits, MeasureText, Node, Paint, RunMetrics, Scene, TextStyle, Warning,
};

/// Half an em a character, eight tenths of an em above the baseline and two
/// below.
struct Halves;

impl MeasureText for Halves {
    fn measure(&self, text: &str, font: &TextStyle) -> RunMetrics {
        #[allow(clippy::cast_precision_loss)]
        let count = text.chars().count() as f64;
        RunMetrics {
            advance: 0.5 * font.size * count,
            ascent: 0.8 * font.size,
            descent: 0.2 * font.size,
        }
    }
}

/// A measurer with nothing to say: a face with no metrics.
struct Nothing;

impl MeasureText for Nothing {
    fn measure(&self, _: &str, _: &TextStyle) -> RunMetrics {
        RunMetrics {
            advance: f64::NAN,
            ascent: 0.0,
            descent: 0.0,
        }
    }
}

/// An advance at the edge of a double's range, for a run that is scaled past
/// it.
struct Vast;

impl MeasureText for Vast {
    fn measure(&self, _: &str, _: &TextStyle) -> RunMetrics {
        RunMetrics {
            advance: 1.5e308,
            ascent: 1.0,
            descent: 1.0,
        }
    }
}

const STOPS: &str =
    "<stop offset=\"0\" stop-color=\"red\"/><stop offset=\"1\" stop-color=\"blue\"/>";

fn document(body: &str) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"100\" height=\"100\">{body}</svg>")
}

fn measured(body: &str, measure: &dyn MeasureText) -> Scene {
    tinker_pdf_svg::read_with(
        document(body).as_bytes(),
        None,
        &Limits::DEFAULT,
        &Context::NONE.with_measure(measure),
    )
    .expect("reads")
}

fn unmeasured(body: &str) -> Scene {
    tinker_pdf_svg::read(document(body).as_bytes(), None, &Limits::DEFAULT).expect("reads")
}

fn near(left: f64, right: f64, what: &str) {
    assert!((left - right).abs() < 1e-9, "{what}: {left} is not {right}");
}

fn near_all(left: &[f64], right: &[f64], what: &str) {
    assert_eq!(left.len(), right.len(), "{what}");
    for (l, r) in left.iter().zip(right) {
        near(*l, *r, what);
    }
}

/// The box an outline's points span, as `[min_x, min_y, max_x, max_y]`.
fn span(outline: &Outline) -> [f64; 4] {
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

/// Every run, in drawing order, groups looked through.
fn runs(nodes: &[Node]) -> Vec<&Node> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            Node::Text { .. } => out.push(node),
            Node::Group { nodes, .. } => out.extend(runs(nodes)),
            _ => {}
        }
    }
    out
}

/// Every run's fill, in drawing order.
fn fills(nodes: &[Node]) -> Vec<&Paint> {
    runs(nodes)
        .into_iter()
        .filter_map(|node| match node {
            Node::Text { fill, .. } => Some(fill),
            _ => None,
        })
        .collect()
}

/// A linear gradient's matrix, which for `objectBoundingBox` units is the box
/// as `[width, 0, 0, height, x, y]`.
fn gradient_box(paint: &Paint) -> [f64; 6] {
    match paint {
        Paint::Linear { matrix, .. } => *matrix,
        other => panic!("a gradient: {other:?}"),
    }
}

/// Every colour a scene's runs are painted with is one a document can state:
/// a paint that waited for its box never reaches the caller as its mark.
fn colours_are_colours(nodes: &[Node]) {
    for paint in fills(nodes) {
        if let Paint::Solid(colour) = paint {
            assert!(
                colour.rgb.iter().all(|c| (0.0..=1.0).contains(c)),
                "a colour no document can state: {colour:?}"
            );
        }
    }
}

/// **A bounding-box gradient on text spans the measured cells**, under every
/// anchor: `start` from the anchor, `middle` across it, `end` to it.
#[test]
fn a_gradient_on_text_spans_the_runs_measured_box() {
    for (anchor, left) in [("start", 10.0), ("middle", -10.0), ("end", -30.0)] {
        let scene = measured(
            &format!(
                "<linearGradient id=\"g\">{STOPS}</linearGradient>\
                 <text x=\"10\" y=\"50\" font-size=\"20\" text-anchor=\"{anchor}\" \
                 fill=\"url(#g) #00ff00\">ABCD</text>"
            ),
            &Halves,
        );
        assert!(scene.warnings.is_empty(), "{anchor}: {:?}", scene.warnings);
        let [fill] = fills(&scene.nodes)[..] else {
            panic!("one run: {:?}", scene.nodes);
        };
        near_all(
            &gradient_box(fill),
            &[40.0, 0.0, 0.0, 20.0, left, 34.0],
            anchor,
        );
    }
}

/// **A `<tspan>`'s paint takes the `<text>`'s box** — SVG 2 §11.2: *"relative
/// to the entire 'text' element in all cases, even when different effects are
/// applied to different 'tspan' … elements"* — so the gradient runs across the
/// whole line and its second half is the second half of it, not a gradient
/// of its own. A stroke is resolved the same way.
#[test]
fn a_tspans_paint_spans_the_whole_text() {
    let scene = measured(
        &format!(
            "<linearGradient id=\"g\">{STOPS}</linearGradient>\
             <text x=\"10\" y=\"50\" font-size=\"20\">AB<tspan fill=\"url(#g)\" \
             stroke=\"url(#g)\">CD</tspan></text>"
        ),
        &Halves,
    );
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let [_, Node::Text { fill, stroke, .. }] = runs(&scene.nodes)[..] else {
        panic!("two runs: {:?}", scene.nodes);
    };
    let whole = [40.0, 0.0, 0.0, 20.0, 10.0, 34.0];
    near_all(&gradient_box(fill), &whole, "fill");
    let stroke = stroke.as_ref().expect("the stroke is drawn");
    near_all(&gradient_box(&stroke.paint), &whole, "stroke");
}

/// **A run that continues a chunk is placed where the previous one ended**,
/// and a `dy` on it moves its cells: the box of `AB`, then `CD` ten units
/// lower, is (10, 34) to (50, 64).
#[test]
fn a_continuing_run_is_measured_where_the_pen_is() {
    let scene = measured(
        &format!(
            "<linearGradient id=\"g\">{STOPS}</linearGradient>\
             <text x=\"10\" y=\"50\" font-size=\"20\" fill=\"url(#g)\">AB<tspan dy=\"10\">CD</tspan></text>"
        ),
        &Halves,
    );
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    // The second run's matrix carries the `dy`, so its paint is the same box
    // seen from ten units lower: one gradient across both, in the scene.
    let fills = fills(&scene.nodes);
    assert_eq!(fills.len(), 2, "{:?}", scene.nodes);
    for (fill, which) in fills.iter().zip(["first", "second"]) {
        near_all(
            &gradient_box(fill),
            &[40.0, 0.0, 0.0, 30.0, 10.0, 34.0],
            which,
        );
    }
}

/// **A rotated glyph's cell turns with it.** `rotate="90"` turns the glyph
/// clockwise about its origin in SVG's downward space: its advance runs down
/// the page and its ascent to the right. One `A` at twenty units is ten wide,
/// sixteen up and four down, so its cell at (10, 50) spans (6, 50) to
/// (26, 60).
#[test]
fn a_rotated_glyphs_cell_turns_with_it() {
    let scene = measured(
        &format!(
            "<linearGradient id=\"g\">{STOPS}</linearGradient>\
             <text x=\"10\" y=\"50\" font-size=\"20\" rotate=\"90\" fill=\"url(#g)\">A</text>"
        ),
        &Halves,
    );
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let [fill] = fills(&scene.nodes)[..] else {
        panic!("one run: {:?}", scene.nodes);
    };
    near_all(
        &gradient_box(fill),
        &[20.0, 0.0, 0.0, 10.0, 6.0, 50.0],
        "rotated",
    );
}

/// **A bounding-box mask and clip on text are resolved against the cells.**
/// The mask's initial region is −10% to 120% of the box, so (6, 32) to
/// (54, 56); the clip's left half is (10, 34) to (30, 54), on the `<text>`
/// itself and on a `<g>` around it.
#[test]
fn a_mask_and_a_clip_on_text_take_its_box() {
    let scene = measured(
        "<mask id=\"m\"><rect width=\"100\" height=\"100\" fill=\"white\"/></mask>\
         <text x=\"10\" y=\"50\" font-size=\"20\" mask=\"url(#m)\">ABCD</text>",
        &Halves,
    );
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let [Node::Group {
        mask: Some(mask), ..
    }] = &scene.nodes[..]
    else {
        panic!("one masked group: {:?}", scene.nodes);
    };
    near_all(
        &span(mask.region.as_ref().expect("a region")),
        &[6.0, 32.0, 54.0, 56.0],
        "mask region",
    );

    let clip = "<clipPath id=\"half\" clipPathUnits=\"objectBoundingBox\">\
                <rect width=\"0.5\" height=\"1\"/></clipPath>";
    for body in [
        format!("{clip}<text x=\"10\" y=\"50\" font-size=\"20\" clip-path=\"url(#half)\">ABCD</text>"),
        format!(
            "{clip}<g clip-path=\"url(#half)\"><text x=\"10\" y=\"50\" font-size=\"20\">ABCD</text></g>"
        ),
    ] {
        let scene = measured(&body, &Halves);
        assert!(scene.warnings.is_empty(), "{body}: {:?}", scene.warnings);
        let [Node::Group {
            clip: Some(clip), ..
        }] = &scene.nodes[..]
        else {
            panic!("one clipped group: {:?}", scene.nodes);
        };
        near_all(&span(&clip.outline), &[10.0, 34.0, 30.0, 54.0], &body);
    }
}

/// **A bounding-box pattern on text tiles its box**: a tile half the box each
/// way is 20 by 10 from (10, 34).
#[test]
fn a_pattern_on_text_tiles_its_box() {
    let scene = measured(
        "<pattern id=\"p\" width=\"0.5\" height=\"0.5\">\
         <rect width=\"5\" height=\"5\"/></pattern>\
         <text x=\"10\" y=\"50\" font-size=\"20\" fill=\"url(#p) #00ff00\">ABCD</text>",
        &Halves,
    );
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let [Paint::Pattern(tile)] = fills(&scene.nodes)[..] else {
        panic!("a pattern: {:?}", scene.nodes);
    };
    near_all(&tile.cell, &[10.0, 34.0, 20.0, 10.0], "cell");
}

/// **A box beside shapes is the union**: a rectangle at (0, 0) to (20, 10)
/// and the run's (10, 34) to (50, 54) make (0, 0) to (50, 54), and the mask's
/// region is −10% to 120% of that.
#[test]
fn text_beside_a_shape_is_measured_with_it() {
    let scene = measured(
        "<mask id=\"m\"><rect width=\"100\" height=\"100\" fill=\"white\"/></mask>\
         <g mask=\"url(#m)\"><rect width=\"20\" height=\"10\"/>\
         <text x=\"10\" y=\"50\" font-size=\"20\">ABCD</text></g>",
        &Halves,
    );
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let [Node::Group {
        mask: Some(mask), ..
    }] = &scene.nodes[..]
    else {
        panic!("one masked group: {:?}", scene.nodes);
    };
    near_all(
        &span(mask.region.as_ref().expect("a region")),
        &[-5.0, -5.4, 55.0, 59.4],
        "mask region",
    );
}

/// **What cannot be placed stays named**, and is drawn as it is without a
/// measurer: unmasked, or in the paint's fallback.
///
/// - A `<tspan>`'s mask takes the whole `<text>`'s box (SVG 2 §11.2), which
///   is not known while the `<tspan>` is being read; its own runs' box would
///   be a different picture.
/// - A `<text>` whose first characters are hidden begins where the text
///   before it left the pen, which is not in these nodes.
/// - A measurer with nothing to say gives no box.
/// - A cell scaled past a double's range is no box either.
#[test]
fn what_cannot_be_placed_stays_named() {
    // Continuing the line, and opening a chunk of its own: either way the
    // box is the whole `<text>`'s, and not the `<tspan>`'s runs'.
    for tspan in ["<tspan", "<tspan x=\"60\""] {
        let scene = measured(
            &format!(
                "<mask id=\"m\"><rect width=\"100\" height=\"100\" fill=\"white\"/></mask>\
                 <text x=\"10\" y=\"50\" font-size=\"20\">AB{tspan} mask=\"url(#m)\">CD</tspan></text>"
            ),
            &Halves,
        );
        assert_eq!(scene.warnings, [Warning::TextBoxUnmeasured], "{tspan}");
        assert!(
            scene.nodes.iter().all(|n| matches!(n, Node::Text { .. })),
            "{tspan}: the runs, unmasked: {:?}",
            scene.nodes
        );
    }

    let fallback = |scene: &Scene, what: &str| {
        assert_eq!(scene.warnings, [Warning::TextBoxUnmeasured], "{what}");
        colours_are_colours(&scene.nodes);
        for fill in fills(&scene.nodes) {
            assert_eq!(
                *fill,
                Paint::Solid(tinker_pdf_svg::Colour {
                    rgb: [0.0, 1.0, 0.0]
                }),
                "{what}: the fallback"
            );
        }
    };
    let gradient = format!("<linearGradient id=\"g\">{STOPS}</linearGradient>");
    let scene = measured(
        &format!(
            "{gradient}<text x=\"10\" y=\"50\" font-size=\"20\" fill=\"url(#g) #00ff00\">\
             <tspan visibility=\"hidden\">AB</tspan>CD</text>"
        ),
        &Halves,
    );
    assert_eq!(fills(&scene.nodes).len(), 1, "the visible run");
    fallback(&scene, "a hidden first run");

    let plain = format!(
        "{gradient}<text x=\"10\" y=\"50\" font-size=\"20\" fill=\"url(#g) #00ff00\">ABCD</text>"
    );
    fallback(&measured(&plain, &Nothing), "no metrics");

    let scaled = format!(
        "{gradient}<text x=\"10\" y=\"50\" transform=\"scale(10)\" \
         fill=\"url(#g) #00ff00\">ABCD</text>"
    );
    fallback(&measured(&scaled, &Vast), "past a double's range");
}

/// **A paint whose matrix overflows is named, not drawn**: the cells are
/// finite, but a `gradientTransform` of 10²⁰⁰ inside a run scaled by 10¹⁵⁰
/// is a gradient whose matrix is not. `push` names such a node and drops it;
/// a run's paint is resolved after the run was pushed, so the paint is `none`
/// and the run stays — its text still extracts.
#[test]
fn a_resolved_paint_past_a_doubles_range_is_named() {
    let scene = measured(
        &format!(
            "<linearGradient id=\"g\" gradientTransform=\"scale(1e200)\">{STOPS}</linearGradient>\
             <text x=\"10\" y=\"50\" font-size=\"20\" transform=\"scale(1e150)\" \
             fill=\"url(#g)\">ABCD</text>"
        ),
        &Halves,
    );
    assert_eq!(scene.warnings, [Warning::GeometryOverflow]);
    assert_eq!(fills(&scene.nodes), [&Paint::None]);
}

/// **A stroke whose server paints nothing is no stroke**, as `push_text`
/// makes one: a bounding-box gradient with no stops is §13.2.4's `none`.
#[test]
fn a_stroke_that_comes_to_none_is_removed() {
    let scene = measured(
        "<linearGradient id=\"empty\"/>\
         <text x=\"10\" y=\"50\" font-size=\"20\" stroke=\"url(#empty)\">ABCD</text>",
        &Halves,
    );
    assert!(scene.warnings.is_empty(), "{:?}", scene.warnings);
    let [Node::Text { fill, stroke, .. }] = runs(&scene.nodes)[..] else {
        panic!("one run: {:?}", scene.nodes);
    };
    assert_eq!(stroke, &None, "no stroke");
    assert_eq!(
        *fill,
        Paint::Solid(tinker_pdf_svg::Colour { rgb: [0.0; 3] }),
        "the initial black fill"
    );
}

/// **Nothing to measure reads the same.** Only a paint that would be
/// `TextBoxUnmeasured` waits for a box, so a document with no bounding-box
/// effect on its text — a user-space gradient or pattern on it, a
/// bounding-box one on a shape, a mask on a shape — is the same scene with a
/// measurer as without, warnings in the same order.
#[test]
fn a_document_with_nothing_to_measure_reads_the_same() {
    let body = format!(
        "<linearGradient id=\"u\" gradientUnits=\"userSpaceOnUse\" x2=\"50%\">{STOPS}</linearGradient>\
         <linearGradient id=\"b\">{STOPS}</linearGradient>\
         <pattern id=\"p\" patternUnits=\"userSpaceOnUse\" width=\"10\" height=\"10\">\
         <rect width=\"5\" height=\"5\"/><blink/></pattern>\
         <mask id=\"m\"><rect width=\"100\" height=\"100\" fill=\"white\"/></mask>\
         <rect width=\"20\" height=\"10\" fill=\"url(#b)\" mask=\"url(#m)\"/>\
         <text x=\"10\" y=\"50\" fill=\"url(#u)\" stroke=\"url(#p)\">AB<tspan dx=\"nonsense\" \
         fill=\"url(#p)\">CD</tspan></text>\
         <unknown/>"
    );
    let plain = unmeasured(&body);
    // The tile's own warning is raised where the run's stroke is resolved,
    // before the `<tspan>` after it is read: a paint that waited for the box
    // would have raised it after.
    assert_eq!(
        plain.warnings,
        [
            Warning::ElementUnknown("blink".into()),
            Warning::ValueUnreadable {
                attribute: "dx".into()
            },
            Warning::ElementUnknown("unknown".into()),
        ]
    );
    assert_eq!(measured(&body, &Halves), plain);
}
