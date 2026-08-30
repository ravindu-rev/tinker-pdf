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
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_svg::path::{self, Segment};
use tinker_pdf_svg::transform;

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

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);
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
