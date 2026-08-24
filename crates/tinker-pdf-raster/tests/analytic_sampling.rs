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

use tinker_pdf_raster::canvas::{Canvas, Color, PixelFormat};
use tinker_pdf_raster::image::{draw_image, ImageDraw, ImageSource, Pyramid, Transform};

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
