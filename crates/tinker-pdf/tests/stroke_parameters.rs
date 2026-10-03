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
///
/// And it scales it **across the line**, not by one number: a horizontal
/// line two units wide is `2 × sy` rows of ink in every column it covers,
/// whatever `sx` is. This test used to assert that `4 0 0 1 cm` thickened
/// the line by half again — the one-width defect, read as the feature —
/// when by the clause an `x` scale only lengthens it.
#[test]
fn the_current_transform_scales_the_line_width() {
    let density = |bitmap: &tinker_pdf::Bitmap| -> f64 {
        let on = columns(bitmap).iter().filter(|c| **c).count().max(1);
        inked(bitmap) as f64 / on as f64
    };
    let plain = stroked("2 w");
    assert_eq!(density(&plain), 2.0, "two rows at 1:1");
    // `10 20 m 90 20 l` under these is drawn at y 20 × sy, so the thick one
    // is moved down to stay on the 40-point page.
    let wide = stroked("q 4 0 0 1 0 0 cm 2 w");
    assert_eq!(
        density(&wide),
        2.0,
        "an x scale lengthens, it does not thicken"
    );
    let tall = stroked("q 1 0 0 4 0 -60 cm 2 w");
    assert_eq!(
        density(&tall),
        8.0,
        "a y scale of 4 makes two units eight rows"
    );
}

/// Renders `content` on a `size`-point square page at one pixel a point, and
/// hands back each pixel's ink, 0 to 1, for a black stroke on white.
fn ink(size: u32, content: &str) -> impl Fn(u32, u32) -> f64 {
    let mut builder = DocumentBuilder::new();
    builder.add_page(f64::from(size), f64::from(size), |page| {
        page.raw(content.as_bytes());
    });
    let bitmap = Document::open(builder.finish())
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    move |x, y| {
        let at = y as usize * bitmap.stride + x as usize * bitmap.components();
        f64::from(255 - bitmap.data[at]) / 255.0
    }
}

/// **A stroke under a transform that is not a similarity** (8.4.3.2). The pen
/// is a disc in user space, so under `scale(1, 3)` a circle of radius 10
/// stroked two wide is the user-space annulus between radii 9 and 11 carried
/// through the scale: at its top it spans `y` from `3 × 9` to `3 × 11` above
/// the centre — **six** device units — and at its side `x` from 9 to 11 —
/// **two**. The renderer used to stroke in device space at one width, the
/// user width times `√|det|`, which is `2√3 ≈ 3.46` both ways.
///
/// The centre is at `(50.5, 50.5)` on a 100-point page, so pixel column 50
/// and pixel row 49 straddle its axes and the ink summed down the column
/// above the centre is the band's height, and along the row right of it the
/// band's width. The circle is four Béziers with their ends on the axes, so
/// those are exactly where the band is measured; the slack is the
/// flattener's, a few hundredths.
#[test]
fn a_circle_under_scale_1_3_is_six_wide_at_its_top_and_two_at_its_side() {
    let k = 0.552_284_75 * 10.0;
    let circle = format!(
        "0 0 0 RG q 1 0 0 3 50.5 50.5 cm 2 w \
         10 0 m 10 {k} {k} 10 0 10 c -{k} 10 -10 {k} -10 0 c \
         -10 -{k} -{k} -10 0 -10 c {k} -10 10 -{k} 10 0 c h S Q"
    );
    let ink = ink(100, &circle);
    // Device row 49 is page y 50 to 51; the top band is rows 16 to 23.
    let top: f64 = (0..49).map(|y| ink(50, y)).sum();
    assert!(
        (top - 6.0).abs() < 0.1,
        "the top of the circle is 3 x 2 = 6 rows of ink: {top:.3}"
    );
    let side: f64 = (51..100).map(|x| ink(x, 49)).sum();
    assert!(
        (side - 2.0).abs() < 0.1,
        "its side is 1 x 2 = 2 columns of ink: {side:.3}"
    );
}

/// **A dash on a sheared line is sheared.** 8.4.3.6 measures a dash along
/// the path in user space and 8.4.3.3's butt cap cuts it square there, so
/// under `1 0 1 1 0 50 cm` (`x' = x + y`) the cut `x = 10` of a line along
/// `y = 0` becomes the diagonal `x' = 10 + y`. A pen six wide covers page
/// `y` 47 to 53, which is device rows 47 to 52, and in device row `r` —
/// user `y = 49.5 − r` at its middle — the first dash's ink runs from
/// `x' = 10 + y` to `20 + y`: ten long, centred at `64.5 − r`. Cut square in
/// device space, as it was, it sat at 15 in every row.
#[test]
fn a_dash_on_a_sheared_line_is_sheared() {
    let ink = ink(
        100,
        "0 0 0 RG q 1 0 1 1 0 50 cm 6 w [10 20] 0 d 10 0 m 80 0 l S Q",
    );
    for r in 47..53 {
        // The second dash starts at user x = 40, past x' = 37 in every row.
        let row: Vec<f64> = (0..32).map(|x| ink(x, r)).collect();
        let total: f64 = row.iter().sum();
        let centre: f64 = row
            .iter()
            .enumerate()
            .map(|(x, v)| (x as f64 + 0.5) * v)
            .sum::<f64>()
            / total;
        assert!((total - 10.0).abs() < 0.05, "row {r}: ten long, {total:.3}");
        let want = 64.5 - f64::from(r);
        assert!(
            (centre - want).abs() < 0.05,
            "row {r}: the dash is centred at {want}, sheared, not {centre:.3}"
        );
    }
}
