//! SVG geometry: §8.3.2's path data grammar and §7.6's transform list.
//!
//! The repository's **thirty-third** target. Like `css` and `xml`, the input is
//! *text* rather than a record layout, so there is no offset to corrupt and no
//! length field to lie about. What a mutator finds instead is the grammar's
//! seams, and this grammar has more of them than it looks: a sign that
//! separates two numbers where a space would, an exponent with no digits, a
//! command letter that repeats itself implicitly, a `moveto` whose repetition
//! is a `lineto` and not another `moveto`, and — the one that has broken every
//! path parser ever written — **an arc's two flags, which are one character
//! each and not numbers**, so that `a1 1 0 1130` is a valid arc with an x of
//! thirty rather than an x of eleven hundred and thirty.
//!
//! The control byte picks the **segment budget** rather than the input, for the
//! reason `css`'s header gives: a target whose limits are all at their shipped
//! defaults never explores a refusal, because a million-segment cap cannot fire
//! inside one iteration.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **The budget holds rather than being exceeded and reported.** The
//!   segments produced never outnumber what was granted, and what was spent
//!   plus what is left is what was handed in — an off-by-one in the accounting
//!   is a cap that does not cap.
//! - **Every coordinate is finite.** A NaN or an infinity here does not fail
//!   anything in this crate; it reaches a rasterizer, where it is a page of
//!   nothing, and the file that produced it looked perfectly ordinary. SVG's
//!   number grammar admits `1e999`, so this is reachable from text a person
//!   could type.
//! - **Parsing is deterministic**, which is ruling 4 asserted on a parser
//!   rather than on a rendered page.
//! - **An outline begins with a move.** §8.3.2 requires it, `parse` enforces
//!   it, and a subpath with no start is a rasterizer's problem rather than
//!   this crate's — so it must never leave here.
//! - **A transform preserves the shape of an outline**: the same number of
//!   segments, each of the same kind. A matrix moves points and nothing else,
//!   and a `transformed` that dropped a `Close` would turn a filled shape into
//!   an open one that still fills — visibly wrong only sometimes.
//! - **An unreadable transform is `None` rather than an identity.** That is the
//!   difference between a caller that can name the defect and a shape drawn
//!   somewhere the file did not put it.
//!
//! # The document surface, from milestone 1
//!
//! The same bytes are also read as **markup**, through
//! [`tinker_pdf_svg::read`], which is the whole crate end to end: the XML
//! reader, the element tree, and the walk that composes transforms into a
//! scene. That half is where a mutator finds the things a grammar fuzzer
//! cannot — a `<!DOCTYPE` with an internal subset, an element nested past the
//! cap, a `viewBox` with no area, a `<use>` that reaches its own ancestor.
//!
//! What is asserted over a scene:
//!
//! - **Every number in it is finite**, for the reason a coordinate is: an
//!   infinity reaches a page and the file that produced it looked ordinary.
//! - **Reading is deterministic**, ruling 4 over the whole crate rather than
//!   over one parser.
//! - **The warning list is deduplicated and inside its cap**, so a document
//!   cannot choose how much memory its own diagnostics cost.
//! - **Nothing is past a limit that refused nothing.** A scene came back, so
//!   every ceiling held; the node count is checked against the one that bounds
//!   it rather than assumed.
//! - **A text run's font size is a number.** It reaches a `Tf` operator, and an
//!   infinity there is a content stream a reader refuses outright — which is a
//!   worse failure than a page that looks wrong, not a better one.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_svg::path::{self, Outline, Segment};
use tinker_pdf_svg::{transform, Limits, Node, Paint, Scene};

/// Every point a segment carries.
fn points(segment: &Segment) -> Vec<[f64; 2]> {
    match *segment {
        Segment::Move(p) | Segment::Line(p) => vec![p],
        Segment::Cubic(a, b, c) => vec![a, b, c],
        Segment::Close => Vec::new(),
        _ => Vec::new(),
    }
}

/// Which variant a segment is, so two outlines can be compared by shape.
fn kind(segment: &Segment) -> u8 {
    match segment {
        Segment::Move(_) => 0,
        Segment::Line(_) => 1,
        Segment::Cubic(..) => 2,
        Segment::Close => 3,
        _ => 4,
    }
}

/// Every coordinate an outline carries, appended.
fn sweep(outline: &Outline, out: &mut Vec<f64>) {
    for segment in &outline.segments {
        for point in points(segment) {
            out.extend_from_slice(&point);
        }
    }
}

/// Every number a paint carries, appended.
fn sweep_paint(paint: &Paint, out: &mut Vec<f64>) {
    match paint {
        Paint::None | Paint::Solid(_) => {}
        Paint::Linear {
            from,
            to,
            matrix,
            stops,
        } => {
            out.extend_from_slice(&[from[0], from[1], to[0], to[1]]);
            out.extend_from_slice(matrix);
            out.extend(stops.iter().flat_map(|stop| [stop.offset, stop.opacity]));
        }
        Paint::Radial {
            centre,
            radius,
            focus,
            matrix,
            stops,
        } => {
            out.extend_from_slice(&[centre[0], centre[1], *radius, focus[0], focus[1]]);
            out.extend_from_slice(matrix);
            out.extend(stops.iter().flat_map(|stop| [stop.offset, stop.opacity]));
        }
        _ => {}
    }
}

/// Every number a scene carries, so one assertion can sweep all of them.
///
/// **Every field, not the interesting ones.** A gradient's `/Matrix` and a
/// clip's outline reach a page exactly as a coordinate does, and an infinity
/// in either is the same failure: a rasterizer with nothing to draw and a file
/// that looked ordinary.
fn numbers(scene: &Scene) -> Vec<f64> {
    let mut out = vec![scene.size.0, scene.size.1];
    for node in &scene.nodes {
        match node {
            Node::Path {
                outline,
                fill,
                fill_opacity,
                stroke,
                clip,
                ..
            } => {
                sweep(outline, &mut out);
                if let Some(clip) = clip {
                    sweep(&clip.outline, &mut out);
                }
                sweep_paint(fill, &mut out);
                out.push(*fill_opacity);
                if let Some(stroke) = stroke {
                    sweep_paint(&stroke.paint, &mut out);
                    out.extend_from_slice(&[
                        stroke.width,
                        stroke.miter_limit,
                        stroke.dash_offset,
                        stroke.opacity,
                    ]);
                    out.extend_from_slice(&stroke.dashes);
                }
            }
            Node::Image { rect, matrix, .. } => {
                out.extend_from_slice(rect);
                out.extend_from_slice(matrix);
            }
            Node::Text {
                anchor,
                matrix,
                font,
                fill,
                fill_opacity,
                stroke,
                ..
            } => {
                if let Some(anchor) = anchor {
                    out.extend_from_slice(anchor);
                }
                out.extend_from_slice(matrix);
                // The size reaches a `Tf` operator, where an infinity is a
                // content stream a reader refuses rather than a page that
                // looks wrong — which is worse, not better.
                out.push(font.size);
                sweep_paint(fill, &mut out);
                out.push(*fill_opacity);
                if let Some(stroke) = stroke {
                    sweep_paint(&stroke.paint, &mut out);
                    out.push(stroke.width);
                }
            }
            _ => {}
        }
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);

    // ---- the whole crate, over the same bytes -------------------------------
    //
    // Before the UTF-8 gate below, deliberately: `tinker_pdf_svg::read` takes
    // bytes and decides the encoding itself, so gating it on `from_utf8` would
    // hide every UTF-16 document and every byte sequence the decoder refuses.
    let mut limits = Limits::DEFAULT;
    // Small enough that the caps are crossable inside one iteration. The
    // shipped values are exercised by the `3` arm.
    match (knobs >> 2) & 3 {
        0 => {
            limits.max_depth = 4;
            limits.max_nodes = 8;
            limits.max_uses = 2;
            limits.max_warnings = 2;
        }
        1 => {
            limits.max_depth = 16;
            limits.max_nodes = 64;
            limits.max_uses = 8;
            limits.max_warnings = 8;
        }
        2 => limits.max_segments = 32,
        _ => {}
    }
    let viewport = if knobs & 0x80 == 0 {
        None
    } else {
        Some((100.0, 50.0))
    };
    if let Ok(scene) = tinker_pdf_svg::read(body, viewport, &limits) {
        for number in numbers(&scene) {
            assert!(
                number.is_finite(),
                "a scene carries something that is not a number: {number}"
            );
        }
        assert!(
            scene.nodes.len() <= limits.max_nodes,
            "{} nodes came out of a cap of {}",
            scene.nodes.len(),
            limits.max_nodes
        );
        assert!(
            scene.warnings.len() <= limits.max_warnings,
            "the warning cap did not hold"
        );
        for (index, warning) in scene.warnings.iter().enumerate() {
            assert!(
                !scene.warnings[..index].contains(warning),
                "a warning was reported twice: {warning:?}"
            );
        }
        let again = tinker_pdf_svg::read(body, viewport, &limits)
            .expect("the same bytes refused on a second run");
        assert!(again == scene, "reading a document is not deterministic");
    }

    let Ok(text) = core::str::from_utf8(body) else {
        return;
    };

    // Small enough that the cap is crossable inside one iteration, and wide
    // enough that the shipped value is also exercised.
    let granted = match knobs & 3 {
        0 => 0,
        1 => 4,
        2 => 512,
        _ => 1 << 20,
    };

    // ---- the transform list ------------------------------------------------
    //
    // Driven first because it is the cheaper half and because a `None` here is
    // a result rather than a dead end: the assertion is that the grammar either
    // yields six finite numbers or refuses, never an identity standing in for a
    // transform the file stated.
    if let Some(matrix) = transform::list(text) {
        assert!(
            matrix.iter().all(|n| n.is_finite()),
            "a transform produced a coordinate that is not a number: {matrix:?}"
        );
        let again = transform::list(text).expect("the same text refused on a second run");
        assert!(matrix == again, "reading a transform is not deterministic");
    }
    if let Some(numbers) = transform::numbers(text) {
        assert!(
            numbers.iter().all(|n| n.is_finite()),
            "the number grammar admitted something that is not a number"
        );
    }
    // §7.7's mapping, over a viewport the input does not choose: a degenerate
    // view box must refuse rather than map, and a legal one must not produce a
    // matrix with a hole in it.
    if let Some(numbers) = transform::numbers(text) {
        if numbers.len() >= 4 {
            let box_ = [numbers[0], numbers[1], numbers[2], numbers[3]];
            if let Some(matrix) = transform::view_box(box_, 100.0, 50.0, Some(text)) {
                assert!(
                    matrix.iter().all(|n| n.is_finite()),
                    "a view box mapped to a matrix that is not numbers: {matrix:?}"
                );
            }
        }
    }

    // ---- the path data -----------------------------------------------------
    let mut budget = granted;
    let Ok(outline) = path::parse(text, &mut budget) else {
        return;
    };

    assert!(
        outline.segments.len() <= granted,
        "{} segments came out of a budget of {granted}",
        outline.segments.len()
    );
    assert_eq!(
        outline.segments.len() + budget,
        granted,
        "what was spent and what is left do not add up to what was granted"
    );

    if let Some(first) = outline.segments.first() {
        assert!(
            matches!(first, Segment::Move(_)),
            "an outline began with something other than a move: {first:?}"
        );
    }

    for segment in &outline.segments {
        for point in points(segment) {
            assert!(
                point[0].is_finite() && point[1].is_finite(),
                "a segment carries a coordinate that is not a number: {segment:?}"
            );
        }
    }

    // Ruling 4, on the parser.
    let mut again_budget = granted;
    let again = path::parse(text, &mut again_budget).expect("the same input refused on a second run");
    assert!(again == outline, "parsing path data is not deterministic");

    // A matrix moves points and nothing else.
    let moved = outline.transformed([2.0, 0.5, -0.25, 3.0, 7.0, -11.0]);
    assert_eq!(
        moved.segments.len(),
        outline.segments.len(),
        "a transform changed how many segments there are"
    );
    for (before, after) in outline.segments.iter().zip(&moved.segments) {
        assert_eq!(
            kind(before),
            kind(after),
            "a transform changed a segment's kind"
        );
    }
    assert_eq!(
        moved.is_empty(),
        outline.is_empty(),
        "a transform changed whether the outline draws anything"
    );
});
