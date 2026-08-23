//! Coverage whose right answer is arithmetic (ruling 13, roadmap step 6).
//!
//! The tier `docs/design/render-verification.md` calls *"the only tier that
//! answers to mathematics"*, and the reason it is worth more than a second
//! renderer where it applies: an expected value computed from the geometry has
//! no opinion to be wrong about, where a second engine has one and it may
//! differ from this one for reasons neither is wrong about.
//!
//! # The sampling grid, stated once
//!
//! `fill` samples **sixteen sub-scanlines per pixel row** and accumulates
//! **exact horizontal spans in units of 1/256 of a pixel**. Both numbers are
//! `fill.rs`'s own constants and both are part of ruling 4's contract — the
//! result is fixed-point and identical on every target, which a floating-point
//! area integrator would not be.
//!
//! Given that grid the coverage of any pixel is a closed form:
//!
//! ```text
//! coverage = min(255, n * hx / 16)
//! ```
//!
//! where `n` is how many of the sixteen sub-scanlines the shape covers and `hx`
//! is the horizontal overlap in 1/256 units on each of them. Every expectation
//! in this file is that expression evaluated by hand or by [`quantised`], which
//! is written from the grid rather than from `fill.rs`.
//!
//! **`255` and not `256` is the whole of the interesting arithmetic**: a fully
//! covered pixel accumulates 16 × 256 units, which divided by sixteen is 256,
//! and 256 does not fit in a byte. The clamp is what makes full coverage
//! opaque, and it is why `128` — not `127` and not `129` — is half.
//!
//! # The injections that were counted
//!
//! | Injected into `fill.rs` | Caught by the workspace | Of which here |
//! | --- | ---: | ---: |
//! | the nonzero rule filling like the even-odd one | 5 | 3 |
//! | the accumulator rounding instead of truncating | 1 | **1** |
//! | a sub-scanline taken at its floor rather than its ceiling | 1 | **1** |
//!
//! The last two rows are why
//! [`coverage_follows_the_grid_between_its_own_gradations`] exists. On the
//! first pass both were caught by **nothing but the determinism fingerprint** —
//! which notices that a pixel moved and has no opinion about whether it should
//! have — because every offset in this file landed on a sixteenth, where the
//! two roundings the grid performs are invisible. A fixture that cannot see a
//! rounding is not a fixture for a rasterizer.
//!
//! # What this cannot reach
//!
//! Pixels. It says a rectangle covers the pixels geometry says it covers; it
//! says nothing about whether the page that rectangle came from is the page a
//! producer meant. That is the gap `verification.md` names, and no fixture here
//! narrows it.

use tinker_pdf_raster::{fill, stroke, FillRule, LineCap, LineJoin, Mask, Path, StrokeStyle};

/// The region every fixture rasterises into.
const W: u32 = 8;
const H: u32 = 8;

/// Sub-scanlines per pixel row, and horizontal units per pixel — `fill.rs`'s
/// own two constants, restated here because every expectation below is written
/// in terms of them.
const SAMPLES: i64 = 16;
const UNITS: i64 = 256;

fn rect_mask(x: f64, y: f64, w: f64, h: f64, rule: FillRule) -> Mask {
    let mut path = Path::new();
    path.rect(x, y, w, h);
    fill(&path, rule, 0, 0, W, H, 0.1, None)
}

fn at(mask: &Mask, x: u32, y: u32) -> u8 {
    mask.data[(y * mask.width + x) as usize]
}

/// The grid's own answer for a pixel the shape covers `hx` units across on `n`
/// of its sixteen sub-scanlines.
///
/// Written from the grid and not from `fill.rs`: this is the one function the
/// whole file rests on, so it states the arithmetic in one place where a reader
/// can check it against the two constants above.
fn quantised(sub_scanlines: i64, units: i64) -> u8 {
    u8::try_from((sub_scanlines * units / SAMPLES).min(255)).unwrap_or(255)
}

/// How many of a row's sixteen sub-scanlines a span of `y` covers.
///
/// A sub-scanline is included when its own `k/16` lies at or after the top edge
/// and before the bottom one, which is `fill.rs`'s `ceil(y * 16)` bound stated
/// as a count.
fn sub_scanlines(row: u32, y0: f64, y1: f64) -> i64 {
    let first = (y0 * SAMPLES as f64).ceil() as i64;
    let last = (y1 * SAMPLES as f64).ceil() as i64;
    let row_top = i64::from(row) * SAMPLES;
    let row_bottom = row_top + SAMPLES;
    (last.min(row_bottom) - first.max(row_top)).max(0)
}

/// A pixel column's horizontal overlap with `[x0, x1]`, in 1/256 units.
fn units(column: u32, x0: f64, x1: f64) -> i64 {
    let left = (x0 * UNITS as f64) as i64;
    let right = (x1 * UNITS as f64) as i64;
    let cell = i64::from(column) * UNITS;
    (right.min(cell + UNITS) - left.max(cell)).max(0)
}

/// The whole expected mask of an axis-aligned rectangle.
fn expected_rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<u8> {
    let mut out = Vec::with_capacity((W * H) as usize);
    for row in 0..H {
        for column in 0..W {
            out.push(quantised(sub_scanlines(row, y0, y1), units(column, x0, x1)));
        }
    }
    out
}

// ---- rectangles -------------------------------------------------------------

/// **A rectangle on integer coordinates is exactly covered, and nothing else
/// is touched.**
///
/// The simplest closed form there is, and the one a build with an off-by-one in
/// either bound fails: a rectangle from 2 to 5 covers columns 2, 3 and 4 and
/// not column 5.
#[test]
fn a_rectangle_on_integer_coordinates_is_exactly_covered() {
    let mask = rect_mask(2.0, 3.0, 3.0, 2.0, FillRule::NonZero);
    for row in 0..H {
        for column in 0..W {
            let inside = (2..5).contains(&column) && (3..5).contains(&row);
            assert_eq!(
                at(&mask, column, row),
                if inside { 255 } else { 0 },
                "pixel ({column}, {row})"
            );
        }
    }
}

/// **Half a pixel is 128, and it is 128 on either axis.**
///
/// The symmetry is the assertion. Sixteen sub-scanlines and 1/256 horizontal
/// units are different mechanisms with different resolutions, and a build in
/// which they disagreed about what half means would draw a rectangle whose
/// vertical edges were softer than its horizontal ones — which looks like
/// anti-aliasing rather than like a defect.
#[test]
fn half_a_pixel_is_the_same_coverage_on_either_axis() {
    let across = rect_mask(1.5, 1.0, 2.0, 2.0, FillRule::NonZero);
    let down = rect_mask(1.0, 1.5, 2.0, 2.0, FillRule::NonZero);
    assert_eq!(at(&across, 1, 1), 128, "half a pixel across");
    assert_eq!(at(&down, 1, 1), 128, "half a pixel down");
    assert_eq!(at(&across, 3, 1), 128, "and the far edge too");
    assert_eq!(at(&down, 1, 3), 128);
    // And the interior is still whole on both.
    assert_eq!(at(&across, 2, 1), 255);
    assert_eq!(at(&down, 1, 2), 255);
}

/// **A pixel cut on both axes is the product of the two fractions.**
///
/// The corner of a rectangle offset by a half on each axis is a quarter of a
/// pixel, and a quarter of 256 is 64. A build that added the two fractions
/// rather than multiplying them would put 0 there and a build that took the
/// smaller would put 128 — both are shapes a rasterizer has had.
#[test]
fn a_corner_cut_on_both_axes_is_the_product_of_its_fractions() {
    let mask = rect_mask(1.5, 1.5, 2.0, 2.0, FillRule::NonZero);
    assert_eq!(at(&mask, 1, 1), 64, "a quarter of a pixel");
    assert_eq!(at(&mask, 1, 2), 128, "half, cut across only");
    assert_eq!(at(&mask, 2, 1), 128, "half, cut down only");
    assert_eq!(at(&mask, 2, 2), 255, "and the middle is whole");
}

/// **Every sixteenth of a pixel is the coverage the grid states**, on both
/// axes, at fifteen offsets each.
///
/// The sweep the three fixtures above are special cases of. Sixteenths because
/// that is the vertical resolution: a finer offset cannot be represented down
/// the page, and asserting one would be asserting the grid is finer than it is.
#[test]
fn coverage_follows_the_grid_at_every_sixteenth_of_a_pixel() {
    for step in 1..16 {
        let offset = f64::from(step) / 16.0;

        let across = rect_mask(2.0 + offset, 2.0, 3.0, 3.0, FillRule::NonZero);
        assert_eq!(
            across.data,
            expected_rect(2.0 + offset, 2.0, 5.0 + offset, 5.0),
            "offset {offset} across"
        );

        let down = rect_mask(2.0, 2.0 + offset, 3.0, 3.0, FillRule::NonZero);
        assert_eq!(
            down.data,
            expected_rect(2.0, 2.0 + offset, 5.0, 5.0 + offset),
            "offset {offset} down"
        );

        // And the two agree with each other on the edge pixel, which is the
        // symmetry above generalised.
        assert_eq!(
            at(&across, 2, 3),
            at(&down, 3, 2),
            "offset {offset}: the two axes disagree about the same fraction"
        );
    }
}

/// **And between the gradations, where the two mechanisms round.**
///
/// The sweep above lands on the grid: every offset is a whole sixteenth, so
/// `ceil(y × 16)` and `floor(y × 16)` are the same number and every
/// accumulator total is a multiple of sixteen. That makes it blind to both of
/// the roundings the grid actually performs — **measured, by injecting each and
/// watching all ten fixtures here stay green**:
///
/// - a sub-scanline is taken at its **ceiling**, so an edge at `1.03` covers
///   one fewer than an edge at `1.0`. On a sixteenth boundary the two readings
///   coincide.
/// - the accumulator **truncates** its division by sixteen. A total that is a
///   multiple of sixteen rounds and truncates alike, and a full-height
///   rectangle always produces one.
///
/// The offsets below are chosen to be neither: thirds and odd 1/256ths across,
/// and hundredths down, so `n` is partial and the totals are odd multiples of
/// eight. Both injections fail here.
#[test]
fn coverage_follows_the_grid_between_its_own_gradations() {
    // Across: odd numbers of 1/256, so a partial column's total is not a
    // multiple of sixteen. Down: not a sixteenth, so the ceiling matters.
    for across in [1.0 / 256.0, 5.0 / 256.0, 1.0 / 3.0, 0.51] {
        for down in [0.03, 0.31, 0.5 + 1.0 / 32.0, 0.97] {
            let (x0, y0) = (2.0 + across, 2.0 + down);
            let (x1, y1) = (x0 + 3.0, y0 + 3.0);
            let mut path = Path::new();
            path.rect(x0, y0, 3.0, 3.0);
            let mask = fill(&path, FillRule::NonZero, 0, 0, W, H, 0.1, None);
            assert_eq!(
                mask.data,
                expected_rect(x0, y0, x1, y1),
                "offset {across} across and {down} down"
            );
        }
    }
}

// ---- fill rules -------------------------------------------------------------

/// **Nonzero minus even-odd is exactly the intersection** (8.5.3.3).
///
/// Two overlapping subpaths wound the same way: the nonzero rule fills the
/// union and the even-odd rule fills the union less the overlap, so subtracting
/// one mask from the other leaves the overlap and nothing else — and the
/// overlap is a rectangle whose coverage this test computes on its own.
///
/// Three numbers, each computed a different way, which is what makes this
/// sharper than either mask alone: a build whose rules were both wrong in the
/// same direction still has to make the difference come out as the intersection.
#[test]
fn nonzero_minus_even_odd_is_exactly_the_overlap() {
    let mut path = Path::new();
    path.rect(1.0, 1.0, 4.0, 4.0);
    path.rect(3.0, 2.0, 4.0, 2.0);
    let nonzero = fill(&path, FillRule::NonZero, 0, 0, W, H, 0.1, None);
    let even_odd = fill(&path, FillRule::EvenOdd, 0, 0, W, H, 0.1, None);

    // The overlap of the two rectangles, computed here from their corners.
    let overlap = expected_rect(3.0, 2.0, 5.0, 4.0);

    let mut difference = Vec::with_capacity(overlap.len());
    for (union, ring) in nonzero.data.iter().zip(&even_odd.data) {
        difference.push(union - ring);
    }
    assert_eq!(difference, overlap);

    // And the two rules really did differ, so the subtraction is not comparing
    // a mask with itself.
    assert_ne!(nonzero.data, even_odd.data);
    assert!(overlap.iter().any(|&value| value > 0));
}

/// **A subpath wound the other way is a hole under both rules.**
///
/// The pair the rule above needs: with opposite windings the nonzero rule
/// subtracts the inner shape too, so the two rules agree — and a build that
/// ignored winding direction entirely would pass the test above and fail this
/// one, because its nonzero mask would still be the union.
#[test]
fn an_oppositely_wound_subpath_is_a_hole_under_either_rule() {
    let mut path = Path::new();
    path.rect(1.0, 1.0, 6.0, 6.0);
    // The same inner square, wound backwards: right, down, left, up.
    path.move_to(3.0, 3.0);
    path.line_to(3.0, 5.0);
    path.line_to(5.0, 5.0);
    path.line_to(5.0, 3.0);
    path.close();

    let nonzero = fill(&path, FillRule::NonZero, 0, 0, W, H, 0.1, None);
    let even_odd = fill(&path, FillRule::EvenOdd, 0, 0, W, H, 0.1, None);
    assert_eq!(
        nonzero.data, even_odd.data,
        "an opposite winding is a hole under both rules"
    );
    assert_eq!(at(&nonzero, 4, 4), 0, "the hole is empty");
    assert_eq!(at(&nonzero, 2, 2), 255, "and the ring around it is not");
}

// ---- half-planes ------------------------------------------------------------

/// **A diagonal edge covers, per pixel, the area under it.**
///
/// The general case the rectangles are the degenerate form of, and the one
/// where the grid actually does work: a 45-degree edge crosses each of a
/// pixel's sixteen sub-scanlines at a different place, so the coverage is a sum
/// of sixteen different spans rather than a product of two fractions.
///
/// The expectation is that sum, computed here from the line's own equation —
/// sixteen evaluations of `x = f(y)` and sixteen exact overlaps — with no
/// reference to how `fill` walks its edges.
#[test]
fn a_diagonal_edge_covers_the_area_the_line_states() {
    // The triangle under `x = y` from (0,0) to (8,8), closed along the bottom.
    let mut path = Path::new();
    path.move_to(0.0, 0.0);
    path.line_to(8.0, 8.0);
    path.line_to(0.0, 8.0);
    path.close();
    let mask = fill(&path, FillRule::NonZero, 0, 0, W, H, 0.1, None);

    let mut wanted = Vec::with_capacity((W * H) as usize);
    for row in 0..H {
        for column in 0..W {
            // Sixteen sub-scanlines; on each, the shape runs from the left
            // edge of the region to `x = y`, which is the side the path winds
            // around.
            let mut total = 0i64;
            for sample in 0..SAMPLES {
                let y = (i64::from(row) * SAMPLES + sample) as f64 / SAMPLES as f64;
                total += units(column, 0.0, y);
            }
            wanted.push(quantised(1, total));
        }
    }
    assert_eq!(mask.data, wanted);

    // And the shape is a triangle rather than a rectangle, so the fixture is
    // not the integer case in disguise.
    assert_eq!(at(&mask, 7, 0), 0, "the top right corner is outside");
    assert_eq!(at(&mask, 0, 7), 255, "the bottom left is inside");
    // The diagonal pixel is 120 rather than 128, and the eight units are the
    // grid rather than a rounding error: a sub-scanline is sampled at its own
    // top edge, so the sixteen spans under `x = y` run 0, 1/16, … 15/16 of a
    // pixel and average to 15/32 rather than to a half.
    assert_eq!(at(&mask, 0, 0), 120);
}

// ---- strokes ----------------------------------------------------------------

/// The total ink of a mask, in whole pixels.
///
/// Coverage is 0..255 where a whole pixel is 255, so the sum divided by 255 is
/// an area — and the quantisation is what the tolerances below are stated in.
fn ink(mask: &Mask) -> f64 {
    mask.data.iter().map(|&v| f64::from(v)).sum::<f64>() / 255.0
}

fn stroked(path: &Path, style: &StrokeStyle) -> Mask {
    let outline = stroke(path, style, 0.01, None);
    fill(&outline, FillRule::NonZero, 0, 0, W, H, 0.01, None)
}

fn segment(x0: f64, y0: f64, x1: f64, y1: f64) -> Path {
    let mut path = Path::new();
    path.move_to(x0, y0);
    path.line_to(x1, y1);
    path
}

/// **A stroked segment's area is its length times its width, plus its caps**
/// (8.4.3.3).
///
/// Three cap styles and three statements, and only two of them are equalities.
/// Butt caps add nothing and square caps add one square of the width — half at
/// each end — and both are polygons, so both are exact.
///
/// **A round cap is not**, and the honest expectation says so rather than
/// carrying a tolerance wide enough to hide it: the cap is flattened to chords
/// before it is filled, so its area is strictly *below* the disc's. What is
/// asserted is therefore a pair of bounds that need no tuning — the cap
/// contains the square inscribed in the disc, of area `2r²`, and is contained
/// by the disc itself, of area `πr²`. A build drawing square caps for round
/// ones lands at `4r²` and fails the upper bound; one drawing nothing lands at
/// zero and fails the lower.
#[test]
fn a_stroked_segment_covers_its_length_times_its_width_plus_its_caps() {
    const LENGTH: f64 = 4.0;
    const WIDTH: f64 = 2.0;
    const RADIUS: f64 = WIDTH / 2.0;
    let path = segment(2.0, 4.0, 2.0 + LENGTH, 4.0);

    let with = |cap| {
        ink(&stroked(
            &path,
            &StrokeStyle {
                width: WIDTH,
                cap,
                ..StrokeStyle::default()
            },
        ))
    };

    let body = LENGTH * WIDTH;
    let butt = with(LineCap::Butt);
    assert!(
        (butt - body).abs() < 0.05,
        "butt caps add nothing: {butt} against {body}"
    );

    let square = with(LineCap::Square);
    assert!(
        (square - (body + WIDTH * WIDTH)).abs() < 0.05,
        "square caps add a square of the width: {square}"
    );

    let round = with(LineCap::Round);
    let inscribed = body + 2.0 * RADIUS * RADIUS;
    let disc = body + core::f64::consts::PI * RADIUS * RADIUS;
    assert!(
        round > inscribed && round < disc,
        "round caps lie between the inscribed square and the disc:          {inscribed} < {round} < {disc}"
    );
    // And the three are ordered, which is what says the cap style reaches the
    // stroker at all.
    assert!(butt < round && round < square, "{butt} {round} {square}");
}

/// **A stroke of twice the width covers twice the area**, and the ratio holds
/// at three widths.
///
/// The property a fixture at one width cannot state. `sqrt` is correctly
/// rounded and permitted on pixel paths under ruling 4, so nothing here needs a
/// tolerance for the arithmetic — only for the quantisation of a half-covered
/// pixel, which is what the 0.05 is.
#[test]
fn a_stroke_scales_with_its_width() {
    let path = segment(1.0, 4.0, 6.0, 4.0);
    for width in [1.0, 2.0, 3.0] {
        let mask = stroked(
            &path,
            &StrokeStyle {
                width,
                cap: LineCap::Butt,
                ..StrokeStyle::default()
            },
        );
        assert!(
            (ink(&mask) - 5.0 * width).abs() < 0.05,
            "a {width}-wide stroke of a 5-long segment: {}",
            ink(&mask)
        );
    }
}

/// **A miter join adds the corner a bevel leaves out** (8.4.3.5).
///
/// Two right-angled segments meeting at a corner: bevelled, the join is the
/// triangle between the two outer edges; mitred, it is the square that
/// completes the corner. The difference is a quarter of the width squared, and
/// it is computable because the corner is a right angle.
#[test]
fn a_miter_join_adds_the_corner_a_bevel_leaves_out() {
    const WIDTH: f64 = 2.0;
    let mut path = Path::new();
    path.move_to(2.0, 2.0);
    path.line_to(5.0, 2.0);
    path.line_to(5.0, 5.0);

    let join = |join: LineJoin| {
        ink(&stroked(
            &path,
            &StrokeStyle {
                width: WIDTH,
                cap: LineCap::Butt,
                join,
                miter_limit: 10.0,
                ..StrokeStyle::default()
            },
        ))
    };
    // The outer corner is a square of half the width on each side, of which the
    // bevel already covers half.
    let corner = (WIDTH / 2.0) * (WIDTH / 2.0) / 2.0;
    let difference = join(LineJoin::Miter) - join(LineJoin::Bevel);
    assert!(
        (difference - corner).abs() < 0.05,
        "a miter over a bevel: {difference} against {corner}"
    );
}
