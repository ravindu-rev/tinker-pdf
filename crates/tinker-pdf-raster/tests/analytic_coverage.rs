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
    fill, fill_hard, fill_stripes, flatten, stroke, FillRule, HardEdge, LineCap, LineJoin, Mask,
    Path, StripeMask, StrokeStyle, SubpixelOrder, Transform,
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

// ---- the centre rule (RC-11.3) ----------------------------------------------

/// Whether the rectangle `[x0, x1) × [y0, y1)` holds pixel `(column, row)`'s
/// centre on the grid: the centre is unit `256·column + 128` of sub-scanline
/// `16·row + 8`, and the rectangle holds it when that sub-scanline is among
/// the ones its two horizontal edges bound, `⌈16·y0⌉` to `⌈16·y1⌉`, and the
/// unit among the ones its two vertical edges bound, each taken to the nearest
/// 1/256 — the grid's own two bounds, stated from the corners
/// (`docs/design/anti-aliasing.md`).
fn rect_holds_centre(column: i64, row: i64, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> bool {
    let sub = row * SAMPLES + SAMPLES / 2;
    let centre = column * UNITS + UNITS / 2;
    let down = |y: f64| (y * SAMPLES as f64).ceil() as i64;
    let across = |x: f64| (x * UNITS as f64).round() as i64;
    down(y0) <= sub && sub < down(y1) && across(x0) <= centre && centre < across(x1)
}

/// One edge of a flattened outline: its ends ordered by `y`, and its winding,
/// +1 where the path took it downward.
struct Line {
    top: (f64, f64),
    bottom: (f64, f64),
    winding: i32,
}

/// The edges a fill makes of `path`: each subpath flattened at the tolerance
/// the fill is given — `flatten` is the fill's own flattening — closed on its
/// first point, and every segment kept that reaches a sub-scanline, from
/// `⌈16·y⌉` of its upper end to `⌈16·y⌉` of its lower.
fn lines_of(path: &Path, tolerance: f64) -> Vec<Line> {
    let mut lines = Vec::new();
    for poly in flatten(path, tolerance) {
        let (Some(first), Some(last)) = (poly.first(), poly.last()) else {
            continue;
        };
        let closing = (first != last).then(|| [*last, *first]);
        for pair in poly
            .windows(2)
            .chain(closing.as_ref().map(|pair| &pair[..]))
        {
            let (a, b) = (pair[0], pair[1]);
            let (top, bottom, winding) = if a.y < b.y { (a, b, 1) } else { (b, a, -1) };
            if (top.y * SAMPLES as f64).ceil() >= (bottom.y * SAMPLES as f64).ceil() {
                continue;
            }
            lines.push(Line {
                top: (top.x, top.y),
                bottom: (bottom.x, bottom.y),
                winding,
            });
        }
    }
    lines
}

/// How near half a unit a crossing may lie before its equation alone cannot
/// say which unit the grid takes it to: the fill carries an edge in sixteen
/// bits under the unit, and a sixty-fourth of a unit is far past what those
/// can drift over a region this size.
const NEAR: f64 = 1.0 / 64.0;

/// Whether the outline `lines` holds pixel `(column, row)`'s centre under
/// `rule`, from the lines' own equations: each line that reaches sub-scanline
/// `16·row + 8` meets `y = row + ½` at the `x` its equation gives, taken to
/// the nearest 1/256, a half going up; the crossings at or left of the
/// centre's unit `256·column + 128` sum to the winding the rule reads there,
/// since a crossing at a unit begins or ends a span that holds that unit.
///
/// `None` where a crossing lies within [`NEAR`] of half a unit and the two
/// units it could be taken to lie either side of the centre: there the
/// equation does not decide the pixel, and nothing is asserted of it.
fn holds_centre(lines: &[Line], column: i64, row: i64, rule: FillRule) -> Option<bool> {
    let sub = row * SAMPLES + SAMPLES / 2;
    let y = sub as f64 / SAMPLES as f64;
    let centre = column * UNITS + UNITS / 2;
    let mut winding = 0i32;
    for line in lines {
        let first = (line.top.1 * SAMPLES as f64).ceil() as i64;
        let last = (line.bottom.1 * SAMPLES as f64).ceil() as i64;
        if sub < first || sub >= last {
            continue;
        }
        let x = line.top.0
            + (y - line.top.1) * (line.bottom.0 - line.top.0) / (line.bottom.1 - line.top.1);
        let at = x * UNITS as f64 + 0.5;
        let (low, high) = ((at - NEAR).floor() as i64, (at + NEAR).floor() as i64);
        if low != high && low == centre {
            return None;
        }
        if at.floor() as i64 <= centre {
            winding += line.winding;
        }
    }
    Some(match rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => winding % 2 != 0,
    })
}

/// A circle of four cubics about `(cx, cy)`.
fn circle(path: &mut Path, (cx, cy): (f64, f64), r: f64) {
    const K: f64 = 0.552_284_75;
    path.move_to(cx + r, cy);
    path.curve_to(cx + r, cy + K * r, cx + K * r, cy + r, cx, cy + r);
    path.curve_to(cx - K * r, cy + r, cx - r, cy + K * r, cx - r, cy);
    path.curve_to(cx - r, cy - K * r, cx - K * r, cy - r, cx, cy - r);
    path.curve_to(cx + K * r, cy - r, cx + r, cy - K * r, cx + r, cy);
    path.close();
}

/// The centre rule's mask of `path` over the fixtures' region.
fn centres(path: &Path, rule: FillRule) -> Mask {
    fill_hard(path, rule, 0, 0, W, H, 0.1, None, HardEdge::Centre)
}

/// Every pixel of `mask` against `expected`, which may decline one: the mask
/// whole or empty everywhere, and each pixel `expected` decides 255 where it
/// says the centre is held and 0 where it says not. Returns how many pixels
/// were decided and how many of those are held.
fn assert_centres(
    mask: &Mask,
    name: &str,
    expected: impl Fn(i64, i64) -> Option<bool>,
) -> (u32, u32) {
    assert!(
        mask.data.iter().all(|&v| v == 0 || v == 255),
        "{name}: every pixel whole or empty"
    );
    let (mut decided, mut held) = (0, 0);
    for row in 0..H {
        for column in 0..W {
            let Some(inside) = expected(i64::from(column), i64::from(row)) else {
                continue;
            };
            decided += 1;
            held += u32::from(inside);
            assert_eq!(
                at(mask, column, row),
                if inside { 255 } else { 0 },
                "{name}: pixel ({column}, {row})"
            );
        }
    }
    (decided, held)
}

/// **The centre rule paints exactly the pixels whose centres the shape holds**
/// (RC-11.3).
///
/// Each pixel's answer is computed here from the shape's own equation on the
/// grid, as this file's other expectations are, and never from `fill.rs`:
///
/// - **rectangles at every sixteenth of a pixel**, every one of the sixteen
///   offsets on each of the four edges — `x.5` and `y.5` among them, where an
///   edge runs through a column or a row of centres and the shape takes them
///   on its left and top and gives them up on its right and bottom — and
///   between the gradations, where the two bounds round: thirds and
///   hundredths across, hundredths about the middle down. Each is
///   [`rect_holds_centre`], the closed form from the corners; a rectangle
///   narrower than a pixel standing between two centres holds none.
/// - **triangles**, wound both ways and one a sliver, whose corners stand at
///   sixteenths and whose sides have slopes with odd denominators — so that
///   no crossing of a row's middle lands on half a unit — each pixel by
///   [`holds_centre`] over its three sides' equations.
/// - **a circle** and **an even-odd ring** of two circles, by
///   [`holds_centre`] over the flattened outline the fill fills, the ring
///   under both rules: even-odd leaves its hole, non-zero fills it.
#[test]
fn the_centre_rule_paints_exactly_the_pixels_whose_centres_the_shape_holds() {
    let rectangle = |(x0, y0, x1, y1): (f64, f64, f64, f64)| {
        let mut path = Path::new();
        path.rect(x0, y0, x1 - x0, y1 - y0);
        // `rect` writes its far corners as these sums.
        let far = (x0 + (x1 - x0), y0 + (y1 - y0));
        (path, (x0, y0, far.0, far.1))
    };
    for a in 0..16 {
        for b in 0..16 {
            let (fa, fb) = (f64::from(a) / 16.0, f64::from(b) / 16.0);
            for corners in [
                (1.0 + fa, 1.0 + fb, 5.0 + fb, 5.0 + fa),
                (1.0 + fb, 2.0 + fa, 6.0 + fa, 3.0 + fb),
            ] {
                let (path, corners) = rectangle(corners);
                let (decided, _) = assert_centres(
                    &centres(&path, FillRule::NonZero),
                    &format!("the rectangle {corners:?}"),
                    |column, row| Some(rect_holds_centre(column, row, corners)),
                );
                assert_eq!(decided, W * H);
            }
        }
    }
    for across in [1.0 / 3.0, 0.49, 0.51, 2.0 / 3.0] {
        for down in [0.03, 0.47, 0.53, 0.97] {
            let (path, corners) = rectangle((1.0 + across, 1.0 + down, 6.0 - across, 6.5 - down));
            let (_, held) = assert_centres(
                &centres(&path, FillRule::NonZero),
                &format!("the rectangle {corners:?}"),
                |column, row| Some(rect_holds_centre(column, row, corners)),
            );
            assert!(held > 0, "{corners:?} holds centres");
        }
    }
    let (path, corners) = rectangle((2.6, 1.0, 3.4, 6.0));
    let (_, held) = assert_centres(
        &centres(&path, FillRule::NonZero),
        "a rectangle between two columns of centres",
        |column, row| Some(rect_holds_centre(column, row, corners)),
    );
    assert_eq!(held, 0, "it is narrower than the gap between centres");

    let triangles: [[(f64, f64); 3]; 3] = [
        [(1.0, 1.0), (7.0, 2.0), (3.0, 7.0)],
        [(0.5, 0.75), (7.5, 3.25), (2.25, 7.5)],
        [(1.0, 1.0), (7.0, 7.0), (1.75, 1.25)],
    ];
    for corners in triangles {
        for reversed in [false, true] {
            let mut order = corners;
            if reversed {
                order.reverse();
            }
            let mut path = Path::new();
            path.move_to(order[0].0, order[0].1);
            path.line_to(order[1].0, order[1].1);
            path.line_to(order[2].0, order[2].1);
            path.close();
            let lines = lines_of(&path, 0.1);
            let (decided, held) = assert_centres(
                &centres(&path, FillRule::NonZero),
                &format!("the triangle {order:?}"),
                |column, row| holds_centre(&lines, column, row, FillRule::NonZero),
            );
            assert_eq!(
                decided,
                W * H,
                "{order:?}: no crossing lies near half a unit"
            );
            assert!(held > 0, "{order:?} holds centres");
        }
    }

    let mut disc = Path::new();
    circle(&mut disc, (4.1, 3.9), 3.3);
    let lines = lines_of(&disc, 0.1);
    let (decided, held) = assert_centres(
        &centres(&disc, FillRule::NonZero),
        "the circle",
        |column, row| holds_centre(&lines, column, row, FillRule::NonZero),
    );
    assert!(decided + 4 >= W * H, "{decided} pixels decided");
    assert!(held > 20, "the circle holds {held} centres");

    let mut ring = Path::new();
    circle(&mut ring, (4.05, 4.05), 3.6);
    circle(&mut ring, (4.05, 4.05), 1.7);
    let lines = lines_of(&ring, 0.1);
    for rule in [FillRule::EvenOdd, FillRule::NonZero] {
        let mask = centres(&ring, rule);
        let (decided, held) =
            assert_centres(&mask, &format!("the ring under {rule:?}"), |column, row| {
                holds_centre(&lines, column, row, rule)
            });
        assert!(decided + 4 >= W * H, "{rule:?}: {decided} pixels decided");
        assert!(held > 20, "{rule:?}: {held} centres");
        let hole = at(&mask, 4, 4);
        match rule {
            FillRule::EvenOdd => assert_eq!(hole, 0, "even-odd leaves the hole"),
            FillRule::NonZero => assert_eq!(hole, 255, "non-zero fills it"),
        }
    }
}

// ---- the any-part rule (RC-11.4) --------------------------------------------

/// Whether the rectangle `[x0, x1) × [y0, y1)` meets pixel `(column, row)`'s
/// open square on the grid (`docs/design/anti-aliasing.md`), from its corners:
/// a unit of the pixel inside on one of its sixteen sub-scanlines — the
/// sub-scanlines `⌈16·y0⌉` to `⌈16·y1⌉` against the row's, the units
/// `round(256·x0)` to `round(256·x1)` against the pixel's — or a side through
/// the square: a vertical side, in each row whose open interval its span of
/// `y` meets, at a unit strictly inside the pixel's; a horizontal side, in the
/// row whose open interval holds its `y`, over units reaching into the pixel's
/// open interval.
fn rect_meets(column: i64, row: i64, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> bool {
    let (left, right) = (column * UNITS, column * UNITS + UNITS);
    let (top, bottom) = (row * SAMPLES, row * SAMPLES + SAMPLES);
    let down = |y: f64| (y * SAMPLES as f64).ceil() as i64;
    let across = |x: f64| (x * UNITS as f64).round() as i64;
    let (first, last) = (down(y0), down(y1));
    let (from, to) = (across(x0), across(x1));
    let sampled = first.max(top) < last.min(bottom) && from.max(left) < to.min(right);
    let (upper, lower) = (row as f64, (row + 1) as f64);
    let vertical = y0 < lower && y1 > upper && [from, to].iter().any(|&x| left < x && x < right);
    let horizontal = [y0, y1].iter().any(|&y| upper < y && y < lower) && from < right && to > left;
    sampled || vertical || horizontal
}

/// One segment of a flattened outline, its two ends in path order.
type Segment = ((f64, f64), (f64, f64));

/// Every segment the any-part rule reads of `path`'s outline: each subpath
/// flattened at the tolerance the fill is given, closed on its first point,
/// every segment kept — horizontal ones and ones lying between two
/// sub-scanlines too — and a subpath of one point a segment of no length.
fn segments_of(path: &Path, tolerance: f64) -> Vec<Segment> {
    let mut segments = Vec::new();
    for poly in flatten(path, tolerance) {
        let (Some(first), Some(last)) = (poly.first(), poly.last()) else {
            continue;
        };
        for pair in poly.windows(2) {
            segments.push(((pair[0].x, pair[0].y), (pair[1].x, pair[1].y)));
        }
        if first != last {
            segments.push(((last.x, last.y), (first.x, first.y)));
        }
        if poly.len() == 1 {
            segments.push(((first.x, first.y), (first.x, first.y)));
        }
    }
    segments
}

/// `x` across to the nearest unit, a half going up, and whether it lies
/// within [`NEAR`] of half a unit where either unit it could be taken to is
/// at or inside the pixel spanning units `left` to `right`, so that the
/// equation alone does not decide the pixel.
fn across_near(x: f64, left: i64, right: i64) -> (i64, bool) {
    let at = x * UNITS as f64 + 0.5;
    let (low, high) = ((at - NEAR).floor() as i64, (at + NEAR).floor() as i64);
    (
        at.floor() as i64,
        low != high && low >= left - 1 && low <= right,
    )
}

fn inside_under(rule: FillRule, winding: i32) -> bool {
    match rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => winding % 2 != 0,
    }
}

/// Whether the outline meets pixel `(column, row)`'s open square under
/// `rule`, from its own equations, as the design defines meeting on the grid:
///
/// - on each of the row's sixteen sub-scanlines, each line reaching it
///   crosses `y = sub/16` at the `x` its equation gives, to the nearest unit;
///   the crossings, sorted here, open and close spans under the rule, and a
///   span holding a unit of the pixel's 256 meets it;
/// - each segment's part inside the open row `(row, row + 1)`, from where it
///   crosses the row's top or begins inside it to where it crosses the bottom
///   or ends, has both ends' `x` to the nearest unit, and an extent reaching
///   into the pixel's open interval of units meets it.
///
/// `None` where a crossing lies within [`NEAR`] of half a unit at or inside
/// the pixel.
fn meets(
    lines: &[Line],
    segments: &[Segment],
    column: i64,
    row: i64,
    rule: FillRule,
) -> Option<bool> {
    let (left, right) = (column * UNITS, column * UNITS + UNITS);
    let mut undecided = false;
    let mut met = false;
    for k in 0..SAMPLES {
        let sub = row * SAMPLES + k;
        let y = sub as f64 / SAMPLES as f64;
        let mut crossings: Vec<(i64, i32)> = Vec::new();
        for line in lines {
            let first = (line.top.1 * SAMPLES as f64).ceil() as i64;
            let last = (line.bottom.1 * SAMPLES as f64).ceil() as i64;
            if sub < first || sub >= last {
                continue;
            }
            let x = line.top.0
                + (y - line.top.1) * (line.bottom.0 - line.top.0) / (line.bottom.1 - line.top.1);
            let (unit, near) = across_near(x, left, right);
            undecided |= near;
            crossings.push((unit, line.winding));
        }
        crossings.sort_unstable();
        let (mut winding, mut start) = (0i32, 0i64);
        for (x, w) in crossings {
            let was = inside_under(rule, winding);
            winding += w;
            let is = inside_under(rule, winding);
            if !was && is {
                start = x;
            } else if was && !is && start < x && start < right && x > left {
                met = true;
            }
        }
    }
    let (upper_row, lower_row) = (row as f64, (row + 1) as f64);
    for &(a, b) in segments {
        let (upper, lower) = if a.1 <= b.1 { (a, b) } else { (b, a) };
        if !(upper.1 < lower_row && lower.1 > upper_row) {
            continue;
        }
        let ends = if upper.1 == lower.1 {
            [a.0, b.0]
        } else {
            let at = |y: f64| {
                if y <= upper.1 {
                    upper.0
                } else if y >= lower.1 {
                    lower.0
                } else {
                    upper.0 + (y - upper.1) * (lower.0 - upper.0) / (lower.1 - upper.1)
                }
            };
            [at(upper.1.max(upper_row)), at(lower.1.min(lower_row))]
        };
        let (u, near_u) = across_near(ends[0], left, right);
        let (v, near_v) = across_near(ends[1], left, right);
        undecided |= near_u || near_v;
        if u.min(v) < right && u.max(v) > left {
            met = true;
        }
    }
    (!undecided).then_some(met)
}

/// The any-part rule's mask of `path` over the fixtures' region.
fn touched(path: &Path, rule: FillRule) -> Mask {
    fill_hard(path, rule, 0, 0, W, H, 0.1, None, HardEdge::AnyPart)
}

/// Every pixel of a polygon's any-part mask against [`meets`] over its
/// lines and segments: returns how many pixels were decided and how many of
/// those the shape meets.
fn assert_touched(path: &Path, rule: FillRule, name: &str) -> (u32, u32) {
    let (lines, segments) = (lines_of(path, 0.1), segments_of(path, 0.1));
    assert_centres(&touched(path, rule), name, |column, row| {
        meets(&lines, &segments, column, row, rule)
    })
}

/// **The any-part rule paints exactly the pixels the shape touches**
/// (RC-11.4).
///
/// Each pixel's answer is the test's own intersection of the shape with the
/// pixel's open square, on the grid as the design defines meeting, computed
/// from the shape's equations and never from `fill.rs`:
///
/// - **rectangles at every sixteenth of a pixel** on each of their four edges
///   and between the gradations, by [`rect_meets`] from the corners: a
///   rectangle on whole pixels paints its own pixels and no more, an edge
///   strictly inside a pixel paints it whatever share it covers, and a
///   rectangle whose sides stand between sub-scanlines or between units is
///   painted where its sides pass;
/// - **slivers thinner than a sub-scanline** — between two sub-scanlines,
///   across a row's edge, of no height on a row's edge, and slanted across a
///   sub-scanline — which only their sides reach;
/// - **triangles**, one with a horizontal side, whose corners stand at
///   sixteenths and whose sloped sides have odd denominators, so that no
///   crossing lands on half a unit, and **a line traced there and back**,
///   which encloses nothing and meets the pixels it passes through; a
///   **point** paints the pixel under it, and a point on a pixel's edge none;
/// - **a circle**, by its flattened outline, the polygon the fill fills.
#[test]
fn the_any_part_rule_paints_exactly_the_pixels_the_shape_touches() {
    let rectangle = |(x0, y0, x1, y1): (f64, f64, f64, f64)| {
        let mut path = Path::new();
        path.rect(x0, y0, x1 - x0, y1 - y0);
        let far = (x0 + (x1 - x0), y0 + (y1 - y0));
        (path, (x0, y0, far.0, far.1))
    };
    let assert_rect = |corners: (f64, f64, f64, f64)| {
        let (path, corners) = rectangle(corners);
        let (decided, held) = assert_centres(
            &touched(&path, FillRule::NonZero),
            &format!("the rectangle {corners:?}"),
            |column, row| Some(rect_meets(column, row, corners)),
        );
        assert_eq!(decided, W * H);
        held
    };
    for a in 0..16 {
        for b in 0..16 {
            let (fa, fb) = (f64::from(a) / 16.0, f64::from(b) / 16.0);
            assert_rect((1.0 + fa, 1.0 + fb, 5.0 + fb, 5.0 + fa));
            assert_rect((1.0 + fb, 2.0 + fa, 6.0 + fa, 3.0 + fb));
        }
    }
    for across in [1.0 / 3.0, 0.49, 0.51, 2.0 / 3.0] {
        for down in [0.03, 0.47, 0.53, 0.97] {
            assert_rect((1.0 + across, 1.0 + down, 6.0 - across, 6.5 - down));
        }
    }
    assert_eq!(
        assert_rect((2.0, 3.0, 5.0, 5.0)),
        6,
        "on whole pixels, its own six and no more"
    );

    // Slivers: between sub-scanlines 40 and 41, across the edge of rows 3
    // and 4, of no height on that edge, and slanted across sub-scanline 89.
    assert_eq!(
        assert_rect((1.25, 2.5 + 1.0 / 64.0, 5.75, 2.5 + 3.0 / 64.0)),
        5
    );
    assert_eq!(
        assert_rect((2.25, 4.0 - 1.0 / 64.0, 5.75, 4.0 + 1.0 / 64.0)),
        8
    );
    assert_eq!(
        assert_rect((2.25, 4.0, 5.75, 4.0)),
        0,
        "a side on a row's edge"
    );
    let mut slanted = Path::new();
    slanted.move_to(1.0, 5.5 + 1.0 / 64.0);
    slanted.line_to(7.0, 5.5 + 5.0 / 64.0);
    slanted.line_to(7.0, 5.5 + 7.0 / 64.0);
    slanted.line_to(1.0, 5.5 + 3.0 / 64.0);
    slanted.close();
    let (decided, held) = assert_touched(&slanted, FillRule::NonZero, "the slanted sliver");
    assert_eq!((decided, held), (W * H, 6), "row 5, columns 1 to 6");

    let triangles: [[(f64, f64); 3]; 4] = [
        [(1.0, 1.0), (7.0, 2.0), (3.0, 7.0)],
        [(0.5, 0.75), (7.5, 3.25), (2.25, 7.5)],
        [(1.0, 1.0), (7.0, 7.0), (1.75, 1.25)],
        [(1.0, 2.25), (7.0, 2.25), (3.5, 6.5)],
    ];
    for corners in triangles {
        for reversed in [false, true] {
            let mut order = corners;
            if reversed {
                order.reverse();
            }
            let mut path = Path::new();
            path.move_to(order[0].0, order[0].1);
            path.line_to(order[1].0, order[1].1);
            path.line_to(order[2].0, order[2].1);
            path.close();
            let (decided, held) =
                assert_touched(&path, FillRule::NonZero, &format!("the triangle {order:?}"));
            assert_eq!(
                decided,
                W * H,
                "{order:?}: no crossing lies near half a unit"
            );
            assert!(held > 0);
        }
    }

    let mut there_and_back = Path::new();
    there_and_back.move_to(1.25, 3.75);
    there_and_back.line_to(6.75, 5.0);
    let (decided, held) =
        assert_touched(&there_and_back, FillRule::NonZero, "a line there and back");
    assert_eq!(decided, W * H);
    assert!(held >= 6, "it passes through {held} pixels");

    let mut points = Path::new();
    points.move_to(3.4, 2.7);
    points.move_to(5.0, 4.5);
    points.move_to(6.5, 6.0);
    let mask = touched(&points, FillRule::NonZero);
    for row in 0..H {
        for column in 0..W {
            assert_eq!(
                at(&mask, column, row),
                if (column, row) == (3, 2) { 255 } else { 0 },
                "the points: pixel ({column}, {row})"
            );
        }
    }

    let mut disc = Path::new();
    circle(&mut disc, (4.1, 3.9), 3.3);
    let (decided, held) = assert_touched(&disc, FillRule::NonZero, "the circle");
    assert!(decided + 8 >= W * H, "{decided} pixels decided");
    assert!(held > 30, "the circle meets {held} pixels");
}

/// **A sliver between two sub-scanlines is not lost** (RC-11.4).
///
/// A rectangle a thirty-second of a pixel tall lying wholly between
/// sub-scanlines 40 and 41 — no sub-scanline crosses it — has no coverage at
/// any pixel and holds no centre, so the coverage, the half rule and the
/// centre rule leave every pixel empty. The any-part rule paints the five
/// pixels of row 2 its sides pass through, columns 1 to 5, and nothing else;
/// and a slanted one does the same in a tile as on the page.
#[test]
fn a_sliver_between_sub_scanlines_is_not_lost() {
    let mut sliver = Path::new();
    sliver.rect(1.25, 2.5 + 1.0 / 64.0, 4.5, 1.0 / 32.0);
    let soft = fill(&sliver, FillRule::NonZero, 0, 0, W, H, 0.1, None);
    assert!(soft.data.iter().all(|&v| v == 0), "no sub-scanline sees it");
    for edge in [HardEdge::Half, HardEdge::Centre] {
        let mask = fill_hard(&sliver, FillRule::NonZero, 0, 0, W, H, 0.1, None, edge);
        assert!(mask.data.iter().all(|&v| v == 0), "{edge:?} paints nothing");
    }
    let mask = touched(&sliver, FillRule::NonZero);
    for row in 0..H {
        for column in 0..W {
            let met = row == 2 && (1..=5).contains(&column);
            assert_eq!(
                at(&mask, column, row),
                if met { 255 } else { 0 },
                "pixel ({column}, {row})"
            );
        }
    }

    let mut slanted = Path::new();
    slanted.move_to(0.5, 3.5 + 1.0 / 64.0);
    slanted.line_to(7.5, 4.5 + 1.0 / 64.0);
    slanted.line_to(7.5, 4.5 + 2.0 / 64.0);
    slanted.line_to(0.5, 3.5 + 2.0 / 64.0);
    slanted.close();
    let page = touched(&slanted, FillRule::NonZero);
    assert!(page.data.iter().filter(|&&v| v == 255).count() >= 8);
    let tile = fill_hard(
        &slanted,
        FillRule::NonZero,
        3,
        3,
        3,
        2,
        0.1,
        None,
        HardEdge::AnyPart,
    );
    for y in 3..5 {
        for x in 3..6 {
            assert_eq!(tile.at(x, y), page.at(x, y), "the tile's pixel ({x}, {y})");
        }
    }
}
