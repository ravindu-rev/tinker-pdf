//! `J j M d` reach the rasterizer (8.4.3).
//!
//! The rasterizer has implemented caps, joins, miter limits and dashes from
//! the beginning, with its own tests. The interpreter discarded all four
//! operators, so every stroke in every document came out solid, butt-capped
//! and miter-joined — a whole tested feature that no document could reach.
//!
//! These drive it from PDF content rather than from `StrokeStyle`, which is
//! the part that was missing.

use tinker_pdf::{Document, RenderOptions};
use tinker_pdf_cos::DocumentBuilder;

/// A page with one horizontal stroke drawn through the given operators.
fn stroked(setup: &str) -> tinker_pdf::Bitmap {
    let mut builder = DocumentBuilder::new();
    builder.add_page(100.0, 40.0, |page| {
        page.raw(format!("0 0 0 RG\n{setup}\n10 20 m 90 20 l S\n").as_bytes());
    });

    Document::open(builder.finish())
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn inked(bitmap: &tinker_pdf::Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|p| p[0] < 128)
        .count()
}

/// Whether a column has any ink, left to right.
fn columns(bitmap: &tinker_pdf::Bitmap) -> Vec<bool> {
    let components = bitmap.components();
    (0..bitmap.width)
        .map(|x| {
            (0..bitmap.height).any(|y| {
                let at = (y as usize) * bitmap.stride + (x as usize) * components;
                bitmap.data.get(at).is_some_and(|v| *v < 128)
            })
        })
        .collect()
}

/// The headline: a dashed line has gaps, a solid one does not.
#[test]
fn a_dash_pattern_breaks_the_line() {
    let solid = stroked("4 w");
    let dashed = stroked("4 w [6 6] 0 d");

    let solid_gaps = columns(&solid).windows(2).filter(|w| w[0] && !w[1]).count();
    assert_eq!(solid_gaps, 1, "a solid line ends exactly once");

    let dashed_columns = columns(&dashed);
    let runs = dashed_columns.windows(2).filter(|w| w[0] && !w[1]).count();
    assert!(
        runs >= 4,
        "a 6-on 6-off pattern over 80 units leaves several dashes, got {runs}"
    );
    assert!(
        inked(&dashed) < inked(&solid),
        "and draws less ink than the solid line"
    );
}

/// A phase starts the pattern part-way in, so the first dash is shorter.
#[test]
fn a_dash_phase_shifts_the_pattern() {
    let unshifted = stroked("4 w [10 10] 0 d");
    let shifted = stroked("4 w [10 10] 5 d");

    let first_run = |bitmap: &tinker_pdf::Bitmap| -> usize {
        columns(bitmap)
            .iter()
            .skip_while(|on| !**on)
            .take_while(|on| **on)
            .count()
    };
    assert!(
        first_run(&shifted) < first_run(&unshifted),
        "the phase eats into the first dash: {} against {}",
        first_run(&shifted),
        first_run(&unshifted)
    );
}

/// An all-zero dash array is invalid. Treating it literally makes the stroke
/// vanish, which reads as missing content rather than as a malformed file.
#[test]
fn a_degenerate_dash_array_draws_a_solid_line() {
    let solid = stroked("4 w");
    let degenerate = stroked("4 w [0 0] 0 d");
    let ratio = inked(&degenerate) as f64 / inked(&solid) as f64;
    assert!(
        (0.9..=1.1).contains(&ratio),
        "expected a solid line, drew {ratio:.2} of one"
    );
}

/// Round and square caps extend past the endpoints; butt caps do not.
#[test]
fn caps_extend_the_line() {
    let butt = stroked("10 w 0 J");
    let round = stroked("10 w 1 J");
    let square = stroked("10 w 2 J");

    let extent =
        |bitmap: &tinker_pdf::Bitmap| -> usize { columns(bitmap).iter().filter(|on| **on).count() };
    assert!(
        extent(&round) > extent(&butt),
        "a round cap reaches past the endpoint: {} against {}",
        extent(&round),
        extent(&butt)
    );
    assert!(
        extent(&square) > extent(&butt),
        "and so does a square one: {} against {}",
        extent(&square),
        extent(&butt)
    );
    assert!(
        inked(&square) >= inked(&round),
        "a square cap covers at least as much as a round one"
    );
}

/// A miter join spikes past the corner; a bevel cuts it off. The join operator
/// has to reach the rasterizer for the two to differ at all.
#[test]
fn joins_change_the_corner() {
    let corner = |setup: &str| -> tinker_pdf::Bitmap {
        let mut builder = DocumentBuilder::new();
        builder.add_page(60.0, 60.0, |page| {
            page.raw(format!("0 0 0 RG\n{setup}\n10 10 m 30 45 l 50 10 l S\n").as_bytes());
        });
        Document::open(builder.finish())
            .expect("it opens")
            .page(0)
            .expect("a page")
            .render(&RenderOptions::default())
    };

    let miter = corner("8 w 0 j");
    let bevel = corner("8 w 2 j");
    assert!(
        inked(&miter) > inked(&bevel),
        "a miter spike covers more than a bevel: {} against {}",
        inked(&miter),
        inked(&bevel)
    );

    // And the miter limit turns a miter into a bevel without changing `j`.
    let limited = corner("8 w 0 j 1 M");
    assert!(
        inked(&limited) < inked(&miter),
        "the miter limit clips the spike: {} against {}",
        inked(&limited),
        inked(&miter)
    );
}

/// 8.4.3.2: the line width is in *user* space, so the current transform
/// scales it. Only the page transform was applied before, so a `cm` scale
/// left every stroke at the wrong weight.
#[test]
fn the_current_transform_scales_the_line_width() {
    let plain = stroked("2 w");
    let scaled = stroked("q 4 0 0 1 0 0 cm 2 w");

    // The x scale of 4 both lengthens the line and thickens it; the thickness
    // is what was missing, so compare the ink per covered column.
    let density = |bitmap: &tinker_pdf::Bitmap| -> f64 {
        let on = columns(bitmap).iter().filter(|c| **c).count().max(1);
        inked(bitmap) as f64 / on as f64
    };
    assert!(
        density(&scaled) > density(&plain) * 1.5,
        "the stroke thickened with the transform: {:.2} against {:.2}",
        density(&scaled),
        density(&plain)
    );
}
