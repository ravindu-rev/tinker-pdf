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
//! **exact horizontal spans in units of 1/256 of a pixel**, each edge taken to
//! the **nearest** unit. Both numbers are `fill.rs`'s own constants and both
//! are part of ruling 4's contract — the result is fixed-point and identical on
//! every target, which a floating-point area integrator would not be.
//!
//! The *nearest* is newer than the rest of this file. An edge used to be
//! truncated toward zero, which is not the same operation on the two sides of
//! the origin, so a shape quantised differently once it was moved across it —
//! which ruling 5's tile guard (`crates/tinker-pdf/tests/render_regions.rs`)
//! measured as a one-level difference between a tile and the page under it.
//! Rounding is also the unbiased choice: truncation moved every edge of every
//! page a half-unit toward zero.
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
//! # Which half of this is adjudication and which is agreement
//!
//! Said plainly, because the two look identical in a passing run and only one
//! of them is evidence.
//!
//! **No third-party data adjudicates anything in this file.** There is no
//! published vector set for a coverage grid; the grid is `fill.rs`'s own choice
//! under ruling 4. What this file can do, and does, is hold the implementation
//! to the *stated model* by a route that does not pass through the
//! implementation — and the two halves of the September 2026 change sit on
//! opposite sides of that line:
//!
//! - **The slope is adjudicated.** [`a_shallow_edge_keeps_the_slope_the_line_states`]
//!   computes the parallelogram's left edge as `8 + 14·y/256` — the line's own
//!   equation, evaluated per sub-scanline — and never asks how an edge is
//!   stored. `fill.rs` used to keep a slope as a whole 1/256 unit per
//!   sub-scanline and this fixture is 0.875 of one, so the old arithmetic
//!   disagreed with the line by fourteen pixels. That is a real finding: the
//!   expectation could not have been written to match it.
//! - **The rounding is agreement, not adjudication.** [`units`] takes an
//!   edge to the *nearest* 1/256 unit, and `fill.rs` was changed to do the same
//!   in the same commit. An oracle changed alongside the code it checks proves
//!   nothing on its own, and this one is written down as such. The argument for
//!   it is in [`units`] and is about the geometry rather than about `fill.rs`:
//!   the overlap is **linear** in each edge's position, so the nearest unit is
//!   the unit whose overlap is nearest the real overlap, and truncation is the
//!   further of the two on every edge. What carries independent weight is the
//!   *size* of the disagreement — measured at **at most one level of 255**, on
//!   fewer than 3% of the pixels of any reviewed golden — which is the shape a
//!   quantisation rule has and not the shape a placement bug has.
//!
//! # What this cannot reach
//!
//! Pixels. It says a rectangle covers the pixels geometry says it covers; it
//! says nothing about whether the page that rectangle came from is the page a
//! producer meant. That is the gap `verification.md` names, and no fixture here
//! narrows it.

use tinker_pdf_raster::{
    fill, fill_stripes, stroke, FillRule, LineCap, LineJoin, Mask, Path, StripeMask, StrokeStyle,
    SubpixelOrder, Transform,
};

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
///
/// **To the nearest unit, not toward zero**, which is the grid's own rule and
/// was `as i64` here until ruling 5's tile guard landed.
///
/// The argument is about the geometry and not about `fill.rs`, which is the
/// only reason this file is allowed to state it: the overlap this function
/// returns is **linear** in each edge's position, so the unit nearest the real
/// edge is the unit whose overlap is nearest the real overlap. A left edge at
/// `x = 2.51` is 642.56 units along; rounded it is 643 and the column-2 overlap
/// comes out 125, truncated it is 642 and the overlap comes out 126, and the
/// real overlap is 125.44. Truncation is the further of the two, on every edge,
/// and it is not even the same operation on the two sides of the origin — so a
/// shape quantised differently once it moved across one, which is what ruling
/// 5's tile guard (`crates/tinker-pdf/tests/render_regions.rs`) measured as a
/// one-level difference between a tile and the page under it.
fn units(column: u32, x0: f64, x1: f64) -> i64 {
    let left = (x0 * UNITS as f64).round() as i64;
    let right = (x1 * UNITS as f64).round() as i64;
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

/// **A slope below one unit of `x` per sub-scanline is carried, not dropped.**
///
/// The fixture above runs along `x = y`, where the shape moves exactly one
/// pixel of `x` per pixel of `y` — sixteen 1/256 units per sub-scanline, a
/// whole number. Every slope in this file was a whole number, and an edge's
/// slope was **stored** as a whole number of 1/256 units per sub-scanline, so
/// the file could not see the quantisation at all. What it hid:
///
/// > any edge shallower than one pixel of `x` per sixteen of `y` had its slope
/// > truncated to **zero** and was rasterised vertical.
///
/// This parallelogram is two pixels wide, 256 tall and drifts fourteen across,
/// which is 0.875 of a unit per sub-scanline — under one, and so exactly the
/// case. Under the old arithmetic it came out a straight vertical bar and was
/// fourteen pixels wrong at the bottom; every pixel below the first few rows
/// fails against the expectation here.
///
/// The expectation is the same closed form as every other fixture in this file:
/// on each of the sixteen sub-scanlines of a row the shape runs from `x(y)` to
/// `x(y) + 2`, where `x(y) = 8 + 14·y/256` is the parallelogram's own left
/// edge, and the row's coverage is those sixteen overlaps summed and divided by
/// sixteen. Nothing here reads `fill.rs`.
#[test]
fn a_shallow_edge_keeps_the_slope_the_line_states() {
    // Wide enough for the drift, tall enough for a sub-unit slope to matter,
    // and both ends on whole pixel rows so every row has all sixteen of its
    // sub-scanlines inside the shape.
    const WIDE: u32 = 32;
    const TALL: u32 = 256;
    const LEFT: f64 = 8.0;
    const DRIFT: f64 = 14.0;
    const THICK: f64 = 2.0;

    let left_at = |y: f64| LEFT + DRIFT * y / f64::from(TALL);

    let mut path = Path::new();
    path.move_to(LEFT, 0.0);
    path.line_to(LEFT + THICK, 0.0);
    path.line_to(left_at(f64::from(TALL)) + THICK, f64::from(TALL));
    path.line_to(left_at(f64::from(TALL)), f64::from(TALL));
    path.close();
    let mask = fill(&path, FillRule::NonZero, 0, 0, WIDE, TALL, 0.1, None);

    let mut wanted = Vec::with_capacity((WIDE * TALL) as usize);
    for row in 0..TALL {
        for column in 0..WIDE {
            let mut total = 0i64;
            for sample in 0..SAMPLES {
                let y = (i64::from(row) * SAMPLES + sample) as f64 / SAMPLES as f64;
                total += units(column, left_at(y), left_at(y) + THICK);
            }
            wanted.push(quantised(1, total));
        }
    }
    assert_eq!(
        mask.data, wanted,
        "the shallow edge is not where the line says it is"
    );

    // And it really does drift, so the fixture is not a vertical bar agreeing
    // with a vertical expectation: the ink is at one end of the region at the
    // top and at the other at the bottom.
    let leftmost = |row: u32| (0..WIDE).find(|column| at(&mask, *column, row) > 0);
    assert_eq!(leftmost(0), Some(8), "the ink starts at x = 8");
    assert_eq!(
        leftmost(TALL - 1),
        Some(21),
        "and ends thirteen columns over: the left edge has travelled the \
         fourteen pixels the line states, which a truncated slope threw away \
         entirely"
    );
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

// ---- stripes ----------------------------------------------------------------

/// The filter's five taps and their sum, restated from
/// `docs/design/lcd-text.md` because every stripe expectation below is
/// written in terms of them.
const TAPS: [i64; 5] = [1, 2, 3, 2, 1];
const TAP_SUM: i64 = 9;

/// The map a shape's stripes are measured under: three times across, about
/// the page frame's `x = 0`.
const THREE_ACROSS: Transform = Transform {
    a: 3.0,
    b: 0.0,
    c: 0.0,
    d: 1.0,
    e: 0.0,
    f: 0.0,
};

/// A row of stripes filtered here into `pixels` pixels' three coverages in
/// `order`. `stripes` starts a whole pixel — three stripes — left of the
/// region and ends one right of it, so the region's stripe `j` is
/// `stripes[j + 3]` and the five it is filtered from are `stripes[j + 1]` to
/// `stripes[j + 5]`, each weighted by its tap and the sum divided by nine,
/// rounded half up.
fn filtered_row(stripes: &[u8], pixels: usize, order: SubpixelOrder) -> Vec<[u8; 3]> {
    let filter = |j: usize| -> u8 {
        let sum: i64 = TAPS
            .iter()
            .enumerate()
            .map(|(t, tap)| tap * i64::from(stripes[j + 1 + t]))
            .sum();
        u8::try_from((sum + TAP_SUM / 2) / TAP_SUM).unwrap_or(255)
    };
    (0..pixels)
        .map(|p| {
            let [left, middle, right] = [filter(3 * p), filter(3 * p + 1), filter(3 * p + 2)];
            match order {
                SubpixelOrder::Rgb => [left, middle, right],
                SubpixelOrder::Bgr => [right, middle, left],
            }
        })
        .collect()
}

/// A mask's row, a pixel's three coverages at a time.
fn stripe_row(mask: &StripeMask, row: u32) -> Vec<[u8; 3]> {
    (0..mask.width)
        .map(|x| mask.at(mask.x0 + x as i32, mask.y0 + row as i32))
        .collect()
}

/// **Stripe coverage is three times the horizontal coverage, filtered**
/// (RC-10.2).
///
/// Rectangles whose left edges stand at every third of a pixel from one
/// pixel to three, of every width in thirds from a third to two pixels, over
/// whole rows, over rows a sixteenth apart, and between the sub-scanlines:
/// each stripe's coverage is the grid's closed form for a pixel a third as
/// wide — sixteen sub-scanlines, the span to the nearest 1/256 of a stripe,
/// computed here from the rectangle's corners mapped three times across —
/// and the pixel's three coverages are those stripes filtered here with the
/// design's taps, in both orders. A rectangle at thirds covers a stripe whole
/// or not at all across, so these stripes are 0 or the row's vertical share,
/// and what the filter makes of an edge is the whole of the difference.
///
/// A diagonal, a quadratic outline and an even-odd pair of rectangles are
/// held to `fill` of the path mapped three times across, its stripes
/// filtered here: the same fill a shape's stripes are defined by, so these
/// hold the mapping, the region's widening, the filter and the order to the
/// design rather than the grid to itself.
#[test]
fn stripe_coverage_is_three_times_horizontal_coverage_filtered() {
    const WIDE: u32 = 8;
    let orders = [SubpixelOrder::Rgb, SubpixelOrder::Bgr];

    for left in 3..=9u32 {
        for span in 1..=6u32 {
            for (y0, height) in [(2.0, 3.0), (1.25, 3.25), (2.0 + 1.0 / 32.0, 0.94)] {
                let x0 = f64::from(left) / 3.0;
                let width = f64::from(span) / 3.0;
                let mut path = Path::new();
                path.rect(x0, y0, width, height);
                // `rect` writes its far corners as these sums.
                let (x1, y1) = (x0 + width, y0 + height);
                for order in orders {
                    let mask =
                        fill_stripes(&path, FillRule::NonZero, 0, 0, WIDE, H, 0.1, order, None);
                    for row in 0..H {
                        let down = sub_scanlines(row, y0, y1);
                        let stripes: Vec<u8> = (-3..3 * i64::from(WIDE) + 3)
                            .map(|k| match u32::try_from(k) {
                                Ok(k) => quantised(down, units(k, 3.0 * x0, 3.0 * x1)),
                                Err(_) => 0,
                            })
                            .collect();
                        assert_eq!(
                            stripe_row(&mask, row),
                            filtered_row(&stripes, WIDE as usize, order),
                            "a rectangle from {left}/3 across {span}/3, {y0} down {height}, row {row}, {order:?}"
                        );
                    }
                }
            }
        }
    }

    let mut diagonal = Path::new();
    diagonal.move_to(1.0, 0.5);
    diagonal.line_to(6.5, 7.5);
    diagonal.line_to(1.0, 7.5);
    diagonal.close();
    let mut curve = Path::new();
    curve.move_to(1.2, 6.8);
    curve.quad_to(4.0, -2.0, 6.9, 6.8);
    curve.close();
    let mut pair = Path::new();
    pair.rect(1.3, 1.0, 4.0, 4.0);
    pair.rect(3.6, 2.5, 4.0, 4.0);
    for (name, path, rule) in [
        ("the diagonal", &diagonal, FillRule::NonZero),
        ("the quadratic", &curve, FillRule::NonZero),
        ("the even-odd pair", &pair, FillRule::EvenOdd),
    ] {
        let tripled = fill(
            &path.mapped(&THREE_ACROSS),
            rule,
            -3,
            0,
            3 * WIDE + 6,
            H,
            0.1,
            None,
        );
        assert!(
            tripled.data.iter().any(|&c| c > 0 && c < 255),
            "{name} has partial stripes"
        );
        for order in orders {
            let mask = fill_stripes(path, rule, 0, 0, WIDE, H, 0.1, order, None);
            for row in 0..H {
                let span = tripled.width as usize;
                let line = &tripled.data[row as usize * span..(row as usize + 1) * span];
                assert_eq!(
                    stripe_row(&mask, row),
                    filtered_row(line, WIDE as usize, order),
                    "{name}, row {row}, {order:?}"
                );
            }
        }
    }
}

/// **A stripe mask over a tile is the page's** (ruling 5, RC-10.2).
///
/// A pixel's three coverages read the stripes of the pixels beside it, so a
/// tile's edge pixel needs stripes the tile does not hold. They are geometry
/// in the page frame: over shapes whose edges fall on and within the
/// filter's reach of the tile edges — rectangles at thirds, one a third of a
/// pixel wide, a diagonal and a quadratic outline — every tile of every
/// width from one pixel to four, at every column and over two bands of rows,
/// holds the page's three coverages at every pixel, in both orders.
#[test]
fn a_stripe_mask_over_a_tile_is_the_page_s() {
    const WIDE: u32 = 12;
    let mut shapes = Vec::new();
    let mut thirds = Path::new();
    thirds.rect(2.0 + 1.0 / 3.0, 1.0, 4.0 + 1.0 / 3.0, 5.5);
    thirds.rect(9.0 + 2.0 / 3.0, 0.25, 1.0 / 3.0, 7.0);
    shapes.push(thirds);
    let mut diagonal = Path::new();
    diagonal.move_to(0.4, 0.2);
    diagonal.line_to(11.6, 7.4);
    diagonal.line_to(0.4, 7.4);
    diagonal.close();
    shapes.push(diagonal);
    let mut curve = Path::new();
    curve.move_to(1.1, 7.6);
    curve.quad_to(6.0, -4.0, 10.9, 7.6);
    curve.close();
    shapes.push(curve);
    for (index, path) in shapes.iter().enumerate() {
        for order in [SubpixelOrder::Rgb, SubpixelOrder::Bgr] {
            let page = fill_stripes(path, FillRule::NonZero, 0, 0, WIDE, H, 0.1, order, None);
            assert!(
                page.data.iter().any(|&[r, g, b]| r != g || g != b),
                "shape {index} has edges in stripes"
            );
            for x0 in 0..WIDE {
                for width in 1..=(WIDE - x0).min(4) {
                    for (y0, height) in [(0, H), (2, 3)] {
                        let tile = fill_stripes(
                            path,
                            FillRule::NonZero,
                            x0 as i32,
                            y0,
                            width,
                            height,
                            0.1,
                            order,
                            None,
                        );
                        for y in y0..y0 + height as i32 {
                            for x in x0 as i32..(x0 + width) as i32 {
                                assert_eq!(
                                    tile.at(x, y),
                                    page.at(x, y),
                                    "shape {index}, {order:?}, the tile at ({x0}, {y0}) {width} wide, pixel ({x}, {y})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
