//! Minification whose right answer is arithmetic (ruling 13, roadmap step 6).
//!
//! Drawing an image smaller than itself has a closed form, and it is the one
//! everybody agrees on: **the value of a destination pixel is the average of
//! the source samples it covers, each weighted by how much of it the pixel
//! covers.** That is a definition rather than a convention, so it can be
//! evaluated here from the geometry and compared against what the rasterizer
//! produced — the tier `docs/design/render-verification.md` calls the only one
//! that answers to mathematics.
//!
//! [`box_average`] below is that definition, written from it. It shares no
//! code with `tinker_pdf_raster::image`: no pyramid, no fixed point, no
//! filter, just overlaps in `f64`. A test that reached for the engine's own
//! sampler would be asking the answer to check itself.
//!
//! # What this found
//!
//! The engine used to interpolate on the way down — four taps weighted by
//! distance, whatever the ratio — which is exact only when the ratio is a
//! power of two, because then the box-filter pyramid has already done all the
//! work and the interpolation has nothing left to do. Between the powers of
//! two it kept whichever samples the grid landed near and threw the rest away.
//! Mean absolute error per channel, out of 255, against the definition, on a
//! 512-square pseudo-random source:
//!
//! | downscale | interpolating | averaging |
//! | ---: | ---: | ---: |
//! | 1.5 : 1 | 43.26 | 0.16 |
//! | 2 : 1 | 0.23 | 0.23 |
//! | 3 : 1 | 26.33 | 0.25 |
//! | 4 : 1 | 0.50 | 0.40 |
//! | 6 : 1 | 9.58 | 0.32 |
//!
//! A tenth of full scale, on most images on most pages, because a page scale
//! is rarely a power of two. Nothing caught it: the fingerprints in
//! `determinism.rs` pin the engine against *itself*, and every one of them was
//! reproducing the same wrong answer on every target, which is exactly the
//! blind spot ruling 13 says a first-party suite has to be built to cover.
//!
//! The residual error is the pyramid's, and it is bounded by how coarse the
//! reduced level is: reducing to within 4:1 rather than 2:1 leaves the average
//! four times more resolution to place the destination pixel edges in, which
//! is the difference between 9.88 and 0.25 at a 3:1 downscale. Reducing to
//! within 8:1 leaves it at 0.25, so 4 is where it stops.
//!
//! # The edge, which is the other closed form
//!
//! The tests from [`an_image_edge_is_covered_in_proportion_to_its_area`] down
//! ask a different question of the same draw: not what colour a pixel takes
//! but *how much* of it the image reaches. That has a closed form too — the
//! area of the pixel the image's transformed unit square covers — and it is
//! the one `analytic_coverage.rs` already states for paths, restated here in
//! [`quantised`] from `fill.rs`'s own two grid constants.
//!
//! It is checked through the canvas rather than through a mask, because the
//! claim is about what an image *paints*. Black on white under `Normal` makes
//! that arithmetic trivial and exact: the composite reduces to
//! `255 - coverage`, so a half-covered edge pixel is 127 and a quarter-covered
//! corner is 191. `docs/design/image-edges.md` records what the change these
//! pin traded away, which is that abutting strips now seam.

use tinker_pdf_raster::blend::BlendMode;
use tinker_pdf_raster::canvas::{Canvas, Color, PixelFormat};
use tinker_pdf_raster::fragments::Fragments;
use tinker_pdf_raster::image::{
    accumulate_image, draw_image, ImageDraw, ImageSource, Pyramid, Transform,
};

/// A pseudo-random source: high spatial frequency with no period a sampling
/// grid can lock on to, which is the content that separates an average from an
/// interpolation. A smooth gradient does not — every filter gets that right.
fn noise(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let mixed = x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(2_246_822_519);
            let value = if (mixed >> 13) & 1 == 0 { 20u8 } else { 235 };
            out.extend_from_slice(&[value, value, value]);
        }
    }
    out
}

/// The definition: every source sample the destination pixel covers, weighted
/// by the area of the overlap.
///
/// Written from the statement of the problem rather than from the engine, and
/// deliberately in `f64` with no fixed point — the engine's arithmetic is
/// integer for ruling 4's reasons, and a reference sharing that choice could
/// share its mistakes.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn box_average(source: &[u8], width: u32, ratio: f64, x: u32, y: u32) -> f64 {
    let (sx0, sx1) = (f64::from(x) * ratio, f64::from(x + 1) * ratio);
    let (sy0, sy1) = (f64::from(y) * ratio, f64::from(y + 1) * ratio);
    let (mut sum, mut weight) = (0.0f64, 0.0f64);
    for sy in (sy0.floor() as u32)..(sy1.ceil() as u32) {
        for sx in (sx0.floor() as u32)..(sx1.ceil() as u32) {
            let across = (sx1.min(f64::from(sx) + 1.0) - sx0.max(f64::from(sx))).max(0.0);
            let down = (sy1.min(f64::from(sy) + 1.0) - sy0.max(f64::from(sy))).max(0.0);
            let area = across * down;
            if area <= 0.0 {
                continue;
            }
            let at = ((sy * width + sx) * 3) as usize;
            sum += area * f64::from(source.get(at).copied().unwrap_or(0));
            weight += area;
        }
    }
    if weight > 0.0 {
        sum / weight
    } else {
        0.0
    }
}

/// Draws `source` over a square canvas of `side` pixels and returns the mean
/// absolute error against [`box_average`], and the worst single pixel.
///
/// The canvas is square and its side is a whole number of pixels, so the image
/// spans it exactly and the two sampling grids are the same grid. A fractional
/// page would leave the engine and this reference disagreeing about where the
/// pixels are, which on noise is a large error and nothing to do with the
/// filter — that mistake cost an afternoon before it was noticed.
///
/// The edges are excluded, and only the edges: a destination pixel hanging off
/// the image covers fewer samples, and how a sampler treats that is a separate
/// question.
fn error(source: &[u8], size: u32, side: u32) -> (f64, f64) {
    let image = ImageSource {
        width: size,
        height: size,
        rgb: source,
        alpha: &[],
    };
    // The unit square onto the whole canvas, y flipped, which is the placement
    // a page hands the rasterizer.
    let placement = Transform {
        a: f64::from(side),
        b: 0.0,
        c: 0.0,
        d: -f64::from(side),
        e: 0.0,
        f: f64::from(side),
    };
    let mut canvas = Canvas::new(side, side, PixelFormat::Rgb8, Color::WHITE);
    draw_image(
        &mut canvas,
        &ImageDraw::new(image, placement),
        &mut Pyramid::new(),
    );

    let ratio = f64::from(size) / f64::from(side);
    let (mut total, mut worst, mut counted) = (0.0f64, 0.0f64, 0u32);
    for y in 1..side - 1 {
        for x in 1..side - 1 {
            let at = (y as usize) * canvas.stride + (x as usize) * 3;
            let got = f64::from(canvas.data.get(at).copied().unwrap_or(0));
            let want = box_average(source, size, ratio, x, y);
            let error = (got - want).abs();
            total += error;
            worst = worst.max(error);
            counted += 1;
        }
    }
    (total / f64::from(counted.max(1)), worst)
}

/// **A minified image is the average of what each pixel covers**, at every
/// ratio and not only at the powers of two.
///
/// The sides are chosen to straddle the pyramid's own decisions: 341 and 171
/// need no reduction, 85 needs one and 43 needs two, so a mistake in how the
/// residual footprint is derived from the ratio shows on one side of a
/// threshold and not the other.
#[test]
fn a_minified_image_is_the_average_of_what_each_pixel_covers() {
    const SIZE: u32 = 512;
    let source = noise(SIZE, SIZE);

    // A mean error under one level out of 255. Not a tolerance chosen to fit:
    // the exact answer is unreachable through a pyramid, because a reduced
    // level has already averaged blocks whose edges do not line up with the
    // destination pixel edges, and one level is what that costs. The
    // interpolating filter this replaced scored 43 on the first row.
    for side in [341u32, 256, 171, 128, 85, 43] {
        let (mean, worst) = error(&source, SIZE, side);
        let ratio = f64::from(SIZE) / f64::from(side);
        assert!(
            mean < 1.0,
            "{ratio:.2}:1 has a mean error of {mean:.2} out of 255 (worst {worst:.0})"
        );
        // And no single pixel is wildly wrong, which a mean can hide.
        assert!(
            worst < 24.0,
            "{ratio:.2}:1 has a pixel {worst:.0} out of 255 from its average"
        );
    }
}

/// **A power-of-two downscale is the box filter exactly**, because the pyramid
/// has already computed the answer and the average has one whole sample to
/// take.
///
/// Separated from the test above because it is a stronger claim and the bar is
/// different: here the arithmetic is the box filter's own and the only error
/// available is rounding. It is also the row that was already right before the
/// filter changed, which is why nothing had noticed the other rows.
#[test]
fn a_power_of_two_downscale_is_the_box_filter_exactly() {
    const SIZE: u32 = 512;
    let source = noise(SIZE, SIZE);
    for side in [256u32, 128, 64, 32] {
        let (mean, worst) = error(&source, SIZE, side);
        assert!(
            mean < 1.0 && worst < 2.0,
            "{}:1 is mean {mean:.2}, worst {worst:.2}",
            SIZE / side
        );
    }
}

/// **A magnified image is not averaged**, and that is a boundary the policy
/// draws rather than an accident of the arithmetic.
///
/// `/Interpolate` is opt-in smoothing for magnification (8.9.5.1): false means
/// the file asked for hard pixels — a barcode, a screenshot, a one-bit mask —
/// and an area filter that blurred them would be a change the file asked not
/// to have. So a magnified draw reproduces whole source samples, exactly.
#[test]
fn a_magnified_image_keeps_its_samples_whole() {
    let source = noise(4, 4);
    let image = ImageSource {
        width: 4,
        height: 4,
        rgb: &source,
        alpha: &[],
    };
    let placement = Transform {
        a: 16.0,
        b: 0.0,
        c: 0.0,
        d: -16.0,
        e: 0.0,
        f: 16.0,
    };
    let mut canvas = Canvas::new(16, 16, PixelFormat::Rgb8, Color::WHITE);
    draw_image(
        &mut canvas,
        &ImageDraw::new(image, placement),
        &mut Pyramid::new(),
    );
    for y in 0..16u32 {
        for x in 0..16u32 {
            let at = (y as usize) * canvas.stride + (x as usize) * 3;
            let got = canvas.data.get(at).copied().unwrap_or(0);
            let want = source[(((y / 4) * 4 + (x / 4)) * 3) as usize];
            assert_eq!(
                got, want,
                "({x}, {y}) is {got} and its source sample is {want}: a 4:1 \
                 magnification repeats samples, it does not average them"
            );
        }
    }
}

// ---- edges ------------------------------------------------------------------

/// The canvas every edge fixture below draws on.
const SIDE: u32 = 8;

/// Sub-scanlines per pixel row, and horizontal units per pixel — `fill.rs`'s
/// own two constants, restated because every expectation below is written in
/// terms of them, exactly as `analytic_coverage.rs` restates them.
const SAMPLES: i64 = 16;
const UNITS: i64 = 256;

/// The grid's answer for a pixel the shape covers `units` across on
/// `sub_scanlines` of its sixteen rows.
fn quantised(sub_scanlines: i64, units: i64) -> u8 {
    u8::try_from((sub_scanlines * units / SAMPLES).min(255)).unwrap_or(255)
}

/// How many of a row's sixteen sub-scanlines the span `[y0, y1)` covers.
fn sub_scanlines(row: u32, y0: f64, y1: f64) -> i64 {
    let first = (y0 * SAMPLES as f64).ceil() as i64;
    let last = (y1 * SAMPLES as f64).ceil() as i64;
    let top = i64::from(row) * SAMPLES;
    (last.min(top + SAMPLES) - first.max(top)).max(0)
}

/// A pixel column's horizontal overlap with `[x0, x1)`, in 1/256 units.
fn units(column: u32, x0: f64, x1: f64) -> i64 {
    let left = (x0 * UNITS as f64) as i64;
    let right = (x1 * UNITS as f64) as i64;
    let cell = i64::from(column) * UNITS;
    (right.min(cell + UNITS) - left.max(cell)).max(0)
}

/// Draws a solid black image over the device rectangle `[x0, x1] x [y0, y1]`
/// onto a white canvas.
///
/// Solid, so no filter has an opinion and the only thing left to measure is
/// the geometry. The transform is the placement a page hands the rasterizer:
/// the unit square's `(0, 0)` is the image's bottom-left, so `d` is negative
/// and `f` is the bottom edge.
fn black_over_white(x0: f64, y0: f64, x1: f64, y1: f64) -> Canvas {
    let rgb = [0u8; 12];
    let image = ImageSource {
        width: 2,
        height: 2,
        rgb: &rgb,
        alpha: &[],
    };
    let placement = Transform {
        a: x1 - x0,
        b: 0.0,
        c: 0.0,
        d: -(y1 - y0),
        e: x0,
        f: y1,
    };
    let mut canvas = Canvas::new(SIDE, SIDE, PixelFormat::Rgb8, Color::WHITE);
    draw_image(
        &mut canvas,
        &ImageDraw::new(image, placement),
        &mut Pyramid::new(),
    );
    canvas
}

/// Black over white under `Normal` is `255 - coverage`, per pixel.
///
/// `mul255(0, a) + mul255(255, 255 - a)` is `255 - a`, and the draw's alpha is
/// the coverage byte, so the canvas *is* the mask read as a photographic
/// negative. Stated here once rather than at each assertion.
fn expected(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<u8> {
    let mut out = Vec::with_capacity((SIDE * SIDE) as usize);
    for row in 0..SIDE {
        for column in 0..SIDE {
            out.push(255 - quantised(sub_scanlines(row, y0, y1), units(column, x0, x1)));
        }
    }
    out
}

/// The index of a pixel in a [`channel`] readout.
fn pixel(row: u32, column: u32) -> usize {
    (row * SIDE + column) as usize
}

/// The first colour channel of every pixel, which for a grey draw is all of it.
fn channel(canvas: &Canvas) -> Vec<u8> {
    let mut out = Vec::with_capacity((SIDE * SIDE) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let at = (y as usize) * canvas.stride + (x as usize) * 3;
            out.push(canvas.data.get(at).copied().unwrap_or(0));
        }
    }
    out
}

/// **An image edge is covered in proportion to its area**, on either axis, at
/// every sixteenth of a pixel.
///
/// The defect this closes painted a destination pixel in full or not at all,
/// by whether its *centre* fell inside the image — so an image at a fractional
/// offset had a jagged edge while every filled path beside it was
/// anti-aliased, and, worse, abutting strips tiled differently at different
/// scales because which strip owned a boundary pixel depended on the scale.
/// That is what the `dpi` corpus relation was breaking on.
///
/// Sixteenths because that is the vertical resolution of the grid: asserting a
/// finer offset down the page would be asserting the grid is finer than it is.
#[test]
fn an_image_edge_is_covered_in_proportion_to_its_area() {
    for step in 1..16 {
        let offset = f64::from(step) / 16.0;

        let across = black_over_white(2.0 + offset, 2.0, 5.0 + offset, 5.0);
        assert_eq!(
            channel(&across),
            expected(2.0 + offset, 2.0, 5.0 + offset, 5.0),
            "offset {offset} across"
        );

        let down = black_over_white(2.0, 2.0 + offset, 5.0, 5.0 + offset);
        assert_eq!(
            channel(&down),
            expected(2.0, 2.0 + offset, 5.0, 5.0 + offset),
            "offset {offset} down"
        );
    }
}

/// **A half-covered image edge is half the paint**, said as one number so a
/// reader does not have to run the closed form in their head.
///
/// A pixel the image covers half of takes half the ink: 128 of 255 coverage
/// over white leaves 127. The old behaviour put 0 or 255 there depending on
/// which side of the centre the edge fell.
#[test]
fn a_half_covered_image_edge_is_half_the_paint() {
    let across = black_over_white(1.5, 1.0, 4.0, 4.0);
    assert_eq!(channel(&across)[pixel(1, 1)], 127, "half across");
    assert_eq!(channel(&across)[pixel(1, 2)], 0, "and inside");

    let down = black_over_white(1.0, 1.5, 4.0, 4.0);
    assert_eq!(channel(&down)[pixel(1, 1)], 127, "half down");
    assert_eq!(channel(&down)[pixel(2, 1)], 0, "and inside");
}

/// **A corner cut on both axes is the product of its two fractions**, not
/// their sum and not the smaller of them.
///
/// A quarter of a pixel is 64 of 255, which over white is 191. A build that
/// added the fractions would paint that corner solid and one that took the
/// smaller would paint it 127 — both are shapes an edge walker has had, and
/// neither is visible in a test that only offsets one axis.
#[test]
fn an_image_corner_is_the_product_of_its_two_fractions() {
    let canvas = channel(&black_over_white(1.5, 1.5, 4.0, 4.0));
    assert_eq!(canvas[pixel(1, 1)], 191, "a quarter of a pixel");
    assert_eq!(canvas[pixel(1, 2)], 127, "half, cut down only");
    assert_eq!(canvas[pixel(2, 1)], 127, "half, cut across only");
    assert_eq!(canvas[pixel(2, 2)], 0, "and the middle is whole");
}

/// **An integer-aligned draw at 1:1 is still byte-preserving**, which is the
/// property the coverage term must not have cost.
///
/// A quad on integer boundaries covers every pixel it touches whole, so the
/// coverage byte is 255 and the composite is the source sample unchanged. If
/// coverage were computed with an off-by-one, or if it double-counted against
/// the `area` filter's own edge handling, this fixture would grow a rim — a
/// darker or lighter border one pixel wide, which is exactly the artefact the
/// samplers were written to avoid and which no fingerprint would explain.
#[test]
fn an_integer_aligned_image_keeps_every_edge_pixel_whole() {
    let source = noise(SIDE, SIDE);
    let image = ImageSource {
        width: SIDE,
        height: SIDE,
        rgb: &source,
        alpha: &[],
    };
    let placement = Transform {
        a: f64::from(SIDE),
        b: 0.0,
        c: 0.0,
        d: -f64::from(SIDE),
        e: 0.0,
        f: f64::from(SIDE),
    };
    let mut canvas = Canvas::new(SIDE, SIDE, PixelFormat::Rgb8, Color::WHITE);
    draw_image(
        &mut canvas,
        &ImageDraw::new(image, placement),
        &mut Pyramid::new(),
    );
    for y in 0..SIDE {
        for x in 0..SIDE {
            let at = (y as usize) * canvas.stride + (x as usize) * 3;
            let got = canvas.data.get(at).copied().unwrap_or(0);
            let want = source[((y * SIDE + x) * 3) as usize];
            assert_eq!(got, want, "({x}, {y}) is {got}, its sample is {want}");
        }
    }
}

// ---- abutting draws ---------------------------------------------------------

/// One solid image over the device rectangle `[x0, x1] x [y0, y1]`.
fn strip(level: u8, x0: f64, y0: f64, x1: f64, y1: f64) -> (Vec<u8>, Transform) {
    (
        vec![level; 12],
        Transform {
            a: x1 - x0,
            b: 0.0,
            c: 0.0,
            d: -(y1 - y0),
            e: x0,
            f: y1,
        },
    )
}

/// Draws two abutting strips, either as two composites or as one run.
fn two_strips(a: u8, b: u8, conflation_free: bool) -> Vec<u8> {
    let (left, lt) = strip(a, 1.0, 1.0, 2.5, 6.0);
    let (right, rt) = strip(b, 2.5, 1.0, 4.0, 6.0);
    fn source(rgb: &[u8]) -> ImageSource<'_> {
        ImageSource {
            width: 2,
            height: 2,
            rgb,
            alpha: &[],
        }
    }
    let mut canvas = Canvas::new(SIDE, SIDE, PixelFormat::Rgb8, Color::WHITE);

    if conflation_free {
        let mut run = Fragments::new(0, 0, SIDE, SIDE);
        for (rgb, t) in [(&left, lt), (&right, rt)] {
            accumulate_image(
                &mut run,
                &ImageDraw::new(source(rgb), t),
                &mut Pyramid::new(),
                (SIDE, SIDE),
            );
        }
        run.composite(&mut canvas, 1.0, BlendMode::Normal, None, None);
    } else {
        for (rgb, t) in [(&left, lt), (&right, rt)] {
            draw_image(
                &mut canvas,
                &ImageDraw::new(source(rgb), t),
                &mut Pyramid::new(),
            );
        }
    }
    channel(&canvas)
}

/// **Two abutting images do not let the page through between them.**
///
/// This is the artefact anti-aliasing an image edge *creates*, and the reason
/// partial coverage alone made the corpus worse rather than better. Two black
/// strips meeting at a half pixel each cover that pixel half. Composited one
/// after the other over white, the first leaves 127 and the second leaves half
/// of that again — **63**, a pale seam down a solid black rectangle, at every
/// boundary of every scanned page assembled from strips.
///
/// Accumulated as one run the coverages add to a whole pixel, and the answer
/// is the 0 it should always have been. The two numbers are asserted together
/// so the test states the defect as well as the fix.
#[test]
fn two_abutting_images_do_not_leak_the_page_between_them() {
    let boundary = pixel(3, 2);

    let conflated = two_strips(0, 0, false);
    assert_eq!(
        conflated[boundary], 63,
        "compositing each strip in turn leaves a seam of backdrop"
    );

    let run = two_strips(0, 0, true);
    assert_eq!(
        run[boundary], 0,
        "accumulated as one run, two halves of a pixel are a whole pixel"
    );
    // And the strips themselves are unchanged either way.
    assert_eq!(run[pixel(3, 1)], 0, "inside the left strip");
    assert_eq!(run[pixel(3, 3)], 0, "inside the right strip");
}

/// **A boundary pixel is the area-weighted average of the two draws that
/// share it**, which is exactly what a render at twice the scale box-filters
/// down to — the property the `dpi` relation measures.
///
/// Black meeting white at a half pixel is 127 or 128 depending on which way
/// the two roundings fall, and asserting the pair rather than one of them is
/// the honest bar: the claim is that the pixel is *the average*, not that it
/// is a particular side of a half-level tie.
#[test]
fn a_shared_boundary_is_the_average_of_the_two_draws() {
    let run = two_strips(0, 255, true);
    let boundary = run[pixel(3, 2)];
    assert!(
        (127..=128).contains(&boundary),
        "black and white meeting at a half pixel average to {boundary}, not 127 or 128"
    );
    assert_eq!(run[pixel(3, 1)], 0, "the black strip is black");
    assert_eq!(run[pixel(3, 3)], 255, "the white one is white");
}
