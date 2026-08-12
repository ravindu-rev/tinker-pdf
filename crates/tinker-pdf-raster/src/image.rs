//! Image sampling.
//!
//! An image arriving here is bytes, two dimensions and a transform. There is
//! no colour space, no filter chain and no dictionary — ruling 8, and the
//! reason the sampling policy can be tested against numbers instead of against
//! documents.
//!
//! Sampling maps *backwards*: every destination pixel is walked through the
//! inverse transform into the image, rather than every source sample being
//! pushed forward. A forward map leaves seams where it lands short and
//! double-writes where it overlaps, and both depend on the transform, so
//! neither is fixable by rounding differently.
//!
//! The policy this file implements is `docs/plans/07-rasterizer.md`.

use crate::blend::BlendMode;
use crate::canvas::{Canvas, Color};
use crate::fill::Mask;

/// A decoded image: its samples, and how many of them there are.
///
/// Three bytes of colour per sample and an optional byte of coverage. What
/// produced them — a codec, a colour conversion, a mask — is the caller's
/// business and is deliberately not expressible here.
#[derive(Clone, Copy, Debug)]
pub struct ImageSource<'a> {
    /// Samples across.
    pub width: u32,
    /// Samples down.
    pub height: u32,
    /// Three bytes per sample, row-major from the **top** row.
    pub rgb: &'a [u8],
    /// One byte of coverage per sample; empty means every sample is opaque.
    pub alpha: &'a [u8],
}

impl ImageSource<'_> {
    /// The sample at `(x, y)`, or `None` where the buffer stops short.
    ///
    /// Every index is checked rather than saturated. `usize` is 32 bits on
    /// wasm32 and 64 on the other three targets, and a sample count that fits
    /// one and not the other is exactly the divergence ruling 4 exists to
    /// catch: a wrapping multiply would read a different sample on one target
    /// and the same document would render differently there.
    fn texel(&self, x: u32, y: u32) -> Option<(u8, u8, u8, u8)> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = (y as usize)
            .checked_mul(self.width as usize)?
            .checked_add(x as usize)?;
        let start = index.checked_mul(3)?;
        let rgb = self.rgb.get(start..start.checked_add(3)?)?;
        let alpha = self.alpha.get(index).copied().unwrap_or(255);
        Some((rgb[0], rgb[1], rgb[2], alpha))
    }
}

/// A 2x3 affine map, in the `[a b c d e f]` order every graphics API writes
/// one.
///
/// The rasterizer's fills and strokes take paths that are already in device
/// space, so this is the only transform the crate holds. An image cannot be
/// pre-transformed the same way — the mapping is what decides which sample a
/// destination pixel reads — so it arrives as a matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// x scale.
    pub a: f64,
    /// y shear.
    pub b: f64,
    /// x shear.
    pub c: f64,
    /// y scale.
    pub d: f64,
    /// x translation.
    pub e: f64,
    /// y translation.
    pub f: f64,
}

impl Transform {
    /// The identity.
    pub const IDENTITY: Transform = Transform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Maps a point.
    #[must_use]
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// The inverse, or `None` when the map collapses.
    ///
    /// A determinant this small maps the image to a sliver of nothing, and
    /// inverting it would produce coordinates whose magnitude has no useful
    /// bits left.
    #[must_use]
    pub fn invert(&self) -> Option<Transform> {
        let determinant = self.a * self.d - self.b * self.c;
        if !determinant.is_finite() || determinant.abs() < 1e-12 {
            return None;
        }
        let inverse = 1.0 / determinant;
        Some(Transform {
            a: self.d * inverse,
            b: -self.b * inverse,
            c: -self.c * inverse,
            d: self.a * inverse,
            e: (self.c * self.f - self.d * self.e) * inverse,
            f: (self.b * self.e - self.a * self.f) * inverse,
        })
    }
}

/// How a destination pixel takes its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// One truncating tap: the sample the pixel's centre lands in.
    Nearest,
    /// Four taps around the pixel's centre, weighted by how near each is.
    Bilinear,
}

/// One in 16.16 fixed point.
const ONE: u64 = 1 << 16;

/// Source samples per device pixel along each axis, in 16.16 fixed point.
///
/// The transform's columns are the device vectors of the image's own axes, so
/// their lengths are how far the whole image reaches; divided by the samples
/// along that axis they are the size of one sample, and the reciprocal is how
/// many samples a device pixel spans. One or less is magnification.
///
/// **Fixed point, because a policy that branches on a float branches
/// differently.** `sqrt` and division are correctly rounded, so the value
/// arriving here is the same on every target — but it is turned into an
/// integer before anything compares it, so the comparison cannot be the thing
/// that diverges. Gap 13 rejected the plan's recursive flattener for the
/// mirror image of this: a float termination test that changes the *number* of
/// segments rather than their positions.
fn sample_ratios(image: &ImageSource<'_>, t: &Transform) -> (u64, u64) {
    let across = (t.a * t.a + t.b * t.b).sqrt();
    let down = (t.c * t.c + t.d * t.d).sqrt();
    (
        fixed_ratio(f64::from(image.width), across),
        fixed_ratio(f64::from(image.height), down),
    )
}

/// `samples / device_length` in 16.16, saturating.
///
/// A zero-length axis means the image is squashed to a line, which is an
/// unbounded downscale; `u64::MAX` is that, and the saturating `as` cast says
/// the same thing about an overflow.
fn fixed_ratio(samples: f64, device: f64) -> u64 {
    // `<= 0.0` rather than a negated `>`: NaN answers false to both, and the
    // check below catches it.
    if device <= 0.0 || !device.is_finite() || !samples.is_finite() {
        return u64::MAX;
    }
    let ratio = samples / device;
    if !ratio.is_finite() {
        return u64::MAX;
    }
    // Multiplying by a power of two is exact until it overflows, and the cast
    // saturates rather than wrapping.
    (ratio * 65536.0) as u64
}

/// Plan 07's sampling policy.
///
/// | Condition | Filter |
/// | --- | --- |
/// | Scale >= 1 per axis, `interpolate` true | Bilinear |
/// | Scale >= 1 per axis, `interpolate` false | Nearest |
/// | Downscale | Bilinear |
///
/// The debatable row is nearest at or above 1:1 without the flag, and it is
/// deliberate. `/Interpolate` is defined as opt-*in* smoothing for magnified
/// images: false means the author wanted hard pixels — a barcode, a
/// screenshot, an upscaled one-bit mask — and blurring them is a change the
/// file asked not to have. It also keeps a 1:1 blit byte-preserving, which is
/// a property the tile contract leans on.
///
/// **The flag has no say on the way down.** It is defined for magnification,
/// and a shrunk image that takes one tap per destination pixel does not keep
/// hard edges — it keeps whichever of the source pixels the sampling grid
/// happened to land on and throws the rest away. That is not the sharpness the
/// author asked for, it is aliasing, and it is a difference against every
/// other renderer rather than a matter of taste.
#[must_use]
pub fn sampling_for(image: &ImageSource<'_>, t: &Transform, interpolate: bool) -> Filter {
    let (across, down) = sample_ratios(image, t);
    // At or above 1:1 on both axes.
    if across <= ONE && down <= ONE {
        return if interpolate {
            Filter::Bilinear
        } else {
            Filter::Nearest
        };
    }
    Filter::Bilinear
}

/// One image draw: the pixels, where they go, and how they compose.
pub struct ImageDraw<'a> {
    /// The samples.
    pub image: ImageSource<'a>,
    /// Maps the image's unit square — `(0, 0)` at its bottom-left corner and
    /// `(1, 1)` at its top-right — to device space.
    pub unit_to_device: Transform,
    /// Whether the image asked to be smoothed when it is magnified.
    ///
    /// The one bit of the policy the caller supplies; everything else is read
    /// off the transform.
    pub interpolate: bool,
    /// Scales the whole draw; clamped to 0..=1.
    pub alpha: f64,
    /// How the samples compose with what is already there.
    pub blend: BlendMode,
    /// Coverage the draw is confined to, if any.
    pub clip: Option<&'a Mask>,
    /// Paint this colour rather than the image's own, taking only coverage
    /// from the image.
    ///
    /// What a one-bit stencil needs, expressed without naming one: the image
    /// says *where*, the caller says *what*.
    pub tint: Option<Color>,
    /// Asked once per destination row; drawing stops as soon as it answers
    /// `true`.
    pub stop: Option<&'a dyn Fn() -> bool>,
}

impl<'a> ImageDraw<'a> {
    /// An opaque, unclipped, normally-blended draw.
    #[must_use]
    pub fn new(image: ImageSource<'a>, unit_to_device: Transform) -> ImageDraw<'a> {
        ImageDraw {
            image,
            unit_to_device,
            interpolate: false,
            alpha: 1.0,
            blend: BlendMode::Normal,
            clip: None,
            tint: None,
            stop: None,
        }
    }
}

/// Draws an image into the unit square of its transform.
///
/// Nothing outside the image's own placement is visited, so a stamp in a
/// corner costs the corner rather than the page.
pub fn draw_image(canvas: &mut Canvas, draw: &ImageDraw<'_>) {
    let image = &draw.image;
    if image.width == 0 || image.height == 0 {
        return;
    }
    let Some(inverse) = draw.unit_to_device.invert() else {
        return; // A degenerate transform maps the image to nothing.
    };

    let Some((x0, x1, y0, y1)) = device_bounds(&draw.unit_to_device, canvas.width, canvas.height)
    else {
        return;
    };

    let filter = sampling_for(image, &draw.unit_to_device, draw.interpolate);

    let alpha = draw.alpha.clamp(0.0, 1.0);
    for py in y0..y1 {
        if draw.stop.is_some_and(|stop| stop()) {
            return;
        }
        for px in x0..x1 {
            // Sample at the pixel's centre.
            let (u, v) = inverse.apply(f64::from(px) + 0.5, f64::from(py) + 0.5);
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }

            let sample = match filter {
                Filter::Nearest => nearest(image, u, v),
                Filter::Bilinear => bilinear(image, u, v),
            };
            let Some((r, g, b, coverage)) = sample else {
                continue;
            };
            if coverage == 0 {
                continue;
            }
            let clip = draw.clip.map_or(255, |mask| mask.at(px as i32, py as i32));
            if clip == 0 {
                continue;
            }

            let effective = alpha * f64::from(coverage) / 255.0 * f64::from(clip) / 255.0;
            let color = draw.tint.unwrap_or(Color::rgb(r, g, b));
            canvas.blend_pixel_with(px, py, color, effective, draw.blend);
        }
    }
}

/// The destination pixels the unit square can reach, clipped to the canvas.
fn device_bounds(t: &Transform, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let corners = [
        t.apply(0.0, 0.0),
        t.apply(1.0, 0.0),
        t.apply(0.0, 1.0),
        t.apply(1.0, 1.0),
    ];
    if corners
        .iter()
        .any(|(x, y)| !x.is_finite() || !y.is_finite())
    {
        return None;
    }
    let xs = corners.iter().map(|(x, _)| *x);
    let ys = corners.iter().map(|(_, y)| *y);
    let x0 = xs.clone().fold(f64::INFINITY, f64::min).floor().max(0.0) as u32;
    let x1 = (xs.fold(f64::NEG_INFINITY, f64::max).ceil().max(0.0) as u32).min(width);
    let y0 = ys.clone().fold(f64::INFINITY, f64::min).floor().max(0.0) as u32;
    let y1 = (ys.fold(f64::NEG_INFINITY, f64::max).ceil().max(0.0) as u32).min(height);
    Some((x0, x1, y0, y1))
}

/// One truncating tap: the sample the pixel's centre lands in.
///
/// Image rows run top-down and the unit square's `v` runs upward, which is the
/// flip in the second line.
fn nearest(image: &ImageSource<'_>, u: f64, v: f64) -> Option<(u8, u8, u8, u8)> {
    let sx = ((u * f64::from(image.width)) as u32).min(image.width - 1);
    let sy = (((1.0 - v) * f64::from(image.height)) as u32).min(image.height - 1);
    image.texel(sx, sy)
}

/// Four taps around the pixel's centre, weighted by how near each is.
///
/// Sample centres sit at half-integers, which is the `- 0.5` below: without
/// it the whole image shifts by half a sample and a 1:1 draw stops being
/// itself. Samples off the edge clamp to the edge rather than fading out,
/// because an image has no defined colour outside itself and fading to one
/// would put a dark rim on every magnified picture.
///
/// # Why the weights are integers
///
/// Ruling 4. The two fractions are the only float arithmetic here, and they
/// become 0..=255 immediately; everything after that is a `u32` multiply-add
/// with round-half-up, so the result cannot depend on how a target rounds. The
/// four weights sum to exactly 65 536, which is what keeps four identical
/// samples returning that sample rather than one level below it.
fn bilinear(image: &ImageSource<'_>, u: f64, v: f64) -> Option<(u8, u8, u8, u8)> {
    let x = u * f64::from(image.width) - 0.5;
    let y = (1.0 - v) * f64::from(image.height) - 0.5;

    let (x0, wx) = split(x);
    let (y0, wy) = split(y);
    let last_x = i64::from(image.width) - 1;
    let last_y = i64::from(image.height) - 1;
    let cx0 = x0.clamp(0, last_x) as u32;
    let cx1 = (x0 + 1).clamp(0, last_x) as u32;
    let cy0 = y0.clamp(0, last_y) as u32;
    let cy1 = (y0 + 1).clamp(0, last_y) as u32;

    // The nearer of the two rows and columns decides whether the pixel is on
    // the image at all; the other three fall back to it where the buffer is
    // shorter than the dimensions claim, so a truncated image blurs into its
    // own last sample rather than into black.
    let anchor = image.texel(cx0, cy0)?;
    let t10 = image.texel(cx1, cy0).unwrap_or(anchor);
    let t01 = image.texel(cx0, cy1).unwrap_or(anchor);
    let t11 = image.texel(cx1, cy1).unwrap_or(anchor);

    let w00 = (256 - wx) * (256 - wy);
    let w10 = wx * (256 - wy);
    let w01 = (256 - wx) * wy;
    let w11 = wx * wy;
    let mix = |a: u8, b: u8, c: u8, d: u8| {
        let total =
            u32::from(a) * w00 + u32::from(b) * w10 + u32::from(c) * w01 + u32::from(d) * w11;
        ((total + 32768) >> 16) as u8
    };

    Some((
        mix(anchor.0, t10.0, t01.0, t11.0),
        mix(anchor.1, t10.1, t01.1, t11.1),
        mix(anchor.2, t10.2, t01.2, t11.2),
        mix(anchor.3, t10.3, t01.3, t11.3),
    ))
}

/// A coordinate as the sample below it and how far past it the point lies, in
/// 1/256ths.
fn split(value: f64) -> (i64, u32) {
    let floor = value.floor();
    // Saturating: a placement far outside the canvas still has to produce an
    // index, and the clamp above turns any of them into an edge sample.
    let index = floor as i64;
    let fraction = ((value - floor) * 256.0) as u32;
    (index, fraction.min(255))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::PixelFormat;

    /// A `size` x `size` image whose samples are all distinct.
    fn ramp(size: u32) -> Vec<u8> {
        let mut rgb = Vec::new();
        for y in 0..size {
            for x in 0..size {
                rgb.extend_from_slice(&[(x * 8) as u8, (y * 8) as u8, 0]);
            }
        }
        rgb
    }

    /// The transform that puts an image over the whole of a square canvas.
    ///
    /// Device y runs down and the unit square's v runs up, so the placement
    /// flips: `d` is negative and `f` is the far edge.
    fn over(size: f64) -> Transform {
        Transform {
            a: size,
            b: 0.0,
            c: 0.0,
            d: -size,
            e: 0.0,
            f: size,
        }
    }

    fn draw(canvas: &mut Canvas, image: &ImageSource<'_>, t: Transform) {
        draw_image(canvas, &ImageDraw::new(*image, t));
    }

    #[test]
    fn a_one_to_one_blit_preserves_every_byte() {
        let rgb = ramp(8);
        let image = ImageSource {
            width: 8,
            height: 8,
            rgb: &rgb,
            alpha: &[],
        };
        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        draw(&mut canvas, &image, over(8.0));

        // The image's top row is device row 0.
        for y in 0..8u32 {
            for x in 0..8u32 {
                let want = Color::rgb((x * 8) as u8, (y * 8) as u8, 0);
                assert_eq!(canvas.pixel(x, y), Some(want), "at {x},{y}");
            }
        }
    }

    #[test]
    fn a_degenerate_transform_draws_nothing() {
        let rgb = ramp(4);
        let image = ImageSource {
            width: 4,
            height: 4,
            rgb: &rgb,
            alpha: &[],
        };
        let mut canvas = Canvas::new(4, 4, PixelFormat::Rgb8, Color::WHITE);
        draw(
            &mut canvas,
            &image,
            Transform {
                a: 0.0,
                b: 0.0,
                c: 0.0,
                d: 0.0,
                e: 2.0,
                f: 2.0,
            },
        );
        assert!(canvas.data.iter().all(|b| *b == 255), "nothing was painted");
    }

    #[test]
    fn a_transparent_sample_leaves_the_page_alone() {
        let rgb = vec![0u8; 4 * 3];
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[0, 255, 0, 255],
        };
        let mut canvas = Canvas::new(4, 4, PixelFormat::Rgb8, Color::WHITE);
        draw(&mut canvas, &image, over(4.0));

        assert_eq!(canvas.pixel(0, 0), Some(Color::WHITE), "transparent");
        assert_eq!(canvas.pixel(3, 0), Some(Color::BLACK), "opaque");
    }

    #[test]
    fn a_tint_replaces_the_image_colour_and_keeps_its_coverage() {
        let rgb = vec![0u8; 4 * 3];
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[0, 255, 0, 255],
        };
        let mut canvas = Canvas::new(4, 4, PixelFormat::Rgb8, Color::WHITE);
        let mut draw = ImageDraw::new(image, over(4.0));
        draw.tint = Some(Color::rgb(255, 0, 0));
        draw_image(&mut canvas, &draw);

        assert_eq!(canvas.pixel(0, 0), Some(Color::WHITE), "still transparent");
        assert_eq!(canvas.pixel(3, 0), Some(Color::rgb(255, 0, 0)), "tinted");
    }

    #[test]
    fn a_short_buffer_skips_pixels_rather_than_panicking() {
        // Ruling 1: the dimensions claim sixteen samples and there are four.
        let rgb = vec![7u8; 4 * 3];
        let image = ImageSource {
            width: 4,
            height: 4,
            rgb: &rgb,
            alpha: &[],
        };
        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        draw(&mut canvas, &image, over(8.0));
        assert_eq!(
            canvas.pixel(0, 0),
            Some(Color::rgb(7, 7, 7)),
            "what is there"
        );
        assert_eq!(canvas.pixel(7, 7), Some(Color::WHITE), "what is not");
    }

    #[test]
    fn drawing_stops_when_asked() {
        let rgb = ramp(8);
        let image = ImageSource {
            width: 8,
            height: 8,
            rgb: &rgb,
            alpha: &[],
        };
        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        let mut draw = ImageDraw::new(image, over(8.0));
        let stop = || true;
        draw.stop = Some(&stop);
        draw_image(&mut canvas, &draw);
        assert!(canvas.data.iter().all(|b| *b == 255), "no row was drawn");
    }

    /// Plan 07's first two rows: the flag is the only thing that decides
    /// whether a magnified image is smoothed.
    #[test]
    fn magnification_smooths_only_when_the_file_asks() {
        let rgb = ramp(2);
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[],
        };
        // Eight device pixels per axis for two samples: 4x.
        assert_eq!(sampling_for(&image, &over(8.0), false), Filter::Nearest);
        assert_eq!(sampling_for(&image, &over(8.0), true), Filter::Bilinear);
        // And at exactly 1:1, which must stay byte-preserving without the flag.
        assert_eq!(sampling_for(&image, &over(2.0), false), Filter::Nearest);
    }

    #[test]
    fn a_magnified_image_gains_values_between_its_samples() {
        // Black and white on one diagonal, mid grey on the other.
        let rgb = vec![0, 0, 0, 255, 255, 255, 128, 128, 128, 128, 128, 128];
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[],
        };

        let distinct = |interpolate: bool| {
            let mut canvas = Canvas::new(16, 16, PixelFormat::Rgb8, Color::WHITE);
            let mut draw = ImageDraw::new(image, over(16.0));
            draw.interpolate = interpolate;
            draw_image(&mut canvas, &draw);
            let mut values: Vec<u8> = canvas.data.iter().step_by(3).copied().collect();
            values.sort_unstable();
            values.dedup();
            values.len()
        };

        assert_eq!(distinct(false), 3, "nearest keeps the three source values");
        assert!(
            distinct(true) > 20,
            "bilinear fills in between them: {} values",
            distinct(true)
        );
    }

    /// A 1:1 draw is byte-preserving under *either* filter: the sample centres
    /// line up, so all the weight lands on one tap.
    #[test]
    fn bilinear_at_one_to_one_is_still_byte_preserving() {
        let rgb = ramp(8);
        let image = ImageSource {
            width: 8,
            height: 8,
            rgb: &rgb,
            alpha: &[],
        };
        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        let mut draw = ImageDraw::new(image, over(8.0));
        draw.interpolate = true;
        draw_image(&mut canvas, &draw);

        for y in 0..8u32 {
            for x in 0..8u32 {
                let want = Color::rgb((x * 8) as u8, (y * 8) as u8, 0);
                assert_eq!(canvas.pixel(x, y), Some(want), "at {x},{y}");
            }
        }
    }

    /// Four identical samples must weigh out to that sample, not to one level
    /// below it — which is what the `+ 32768` rounding is for.
    #[test]
    fn bilinear_weights_sum_to_one() {
        let rgb = vec![255u8; 4 * 3];
        let image = ImageSource {
            width: 2,
            height: 2,
            rgb: &rgb,
            alpha: &[],
        };
        for step in 0..16 {
            let u = 0.5 / 16.0 + f64::from(step) / 16.0;
            let sample = bilinear(&image, u, u).expect("on the image");
            assert_eq!(sample, (255, 255, 255, 255), "at u = {u}");
        }
    }

    /// A horizontal ramp: `steps` samples one row tall, rising by `rise` a
    /// sample.
    fn gradient(steps: u32, rise: u8) -> Vec<u8> {
        let mut rgb = Vec::new();
        for x in 0..steps {
            let value = (x as u8).wrapping_mul(rise);
            rgb.extend_from_slice(&[value, value, value]);
        }
        rgb
    }

    /// The largest jump between neighbouring values.
    fn max_step(values: &[u8]) -> u8 {
        values
            .windows(2)
            .map(|pair| pair[1].abs_diff(pair[0]))
            .max()
            .unwrap_or(0)
    }

    /// Milestone 3: an upscaled gradient has no stair-steps.
    ///
    /// "No stair-steps" is a measurement, not an impression: the largest jump
    /// between neighbouring output pixels. Sixteen device pixels per source
    /// sample, so a filter that interpolates cannot step by more than one
    /// sixteenth of the source's own step, and one that does not must step by
    /// the whole of it.
    #[test]
    fn magnifying_a_gradient_leaves_no_stair_steps() {
        let rgb = gradient(8, 32);
        let image = ImageSource {
            width: 8,
            height: 1,
            rgb: &rgb,
            alpha: &[],
        };

        let row = |interpolate: bool| {
            let mut canvas = Canvas::new(128, 1, PixelFormat::Rgb8, Color::WHITE);
            let mut draw = ImageDraw::new(
                image,
                Transform {
                    a: 128.0,
                    b: 0.0,
                    c: 0.0,
                    d: -1.0,
                    e: 0.0,
                    f: 1.0,
                },
            );
            draw.interpolate = interpolate;
            draw_image(&mut canvas, &draw);
            canvas.data.iter().step_by(3).copied().collect::<Vec<u8>>()
        };

        assert_eq!(
            max_step(&row(false)),
            32,
            "nearest steps by the source's whole step: that is the stair"
        );
        assert_eq!(
            max_step(&row(true)),
            2,
            "bilinear steps by the source step over the magnification, 32/16"
        );
    }

    /// Milestone 3: a 2:1 downscale keeps its midtones.
    ///
    /// A one-sample checkerboard halved is the case that makes the point: its
    /// mean is 127.5 and every pixel of it is 0 or 255, so a sampler that
    /// takes one tap returns a page of whichever phase its grid landed on and
    /// reports the mean of *that*.
    #[test]
    fn a_two_to_one_downscale_keeps_its_midtones() {
        let mut rgb = Vec::new();
        for y in 0..16u32 {
            for x in 0..16u32 {
                let value = if (x + y) % 2 == 0 { 0 } else { 255 };
                rgb.extend_from_slice(&[value, value, value]);
            }
        }
        let image = ImageSource {
            width: 16,
            height: 16,
            rgb: &rgb,
            alpha: &[],
        };

        // 16 samples into 8 device pixels: exactly 2:1, the last row of the
        // policy that is still plain bilinear.
        assert_eq!(sampling_for(&image, &over(8.0), false), Filter::Bilinear);

        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        draw_image(&mut canvas, &ImageDraw::new(image, over(8.0)));
        let values: Vec<u8> = canvas.data.iter().step_by(3).copied().collect();

        let mean = values.iter().map(|v| u32::from(*v)).sum::<u32>() / values.len() as u32;
        assert_eq!(mean, 128, "the midtone survives: mean {mean}");
        let (low, high) = (
            *values.iter().min().expect("pixels"),
            *values.iter().max().expect("pixels"),
        );
        assert_eq!(
            (low, high),
            (128, 128),
            "and every pixel is that midtone rather than one phase of the \
             checkerboard: {low}..{high}"
        );
    }

    /// The flag is about magnification. On the way down there is no sharpness
    /// to preserve, only samples to drop, so the policy does not consult it.
    #[test]
    fn a_downscale_ignores_the_interpolate_flag() {
        let rgb = gradient(16, 16);
        let image = ImageSource {
            width: 16,
            height: 1,
            rgb: &rgb,
            alpha: &[],
        };
        let squash = Transform {
            a: 8.0,
            b: 0.0,
            c: 0.0,
            d: -1.0,
            e: 0.0,
            f: 1.0,
        };
        assert_eq!(sampling_for(&image, &squash, false), Filter::Bilinear);
        assert_eq!(sampling_for(&image, &squash, true), Filter::Bilinear);
    }

    #[test]
    fn a_transform_and_its_inverse_compose_to_the_identity() {
        let t = Transform {
            a: 2.0,
            b: 0.5,
            c: -1.0,
            d: 3.0,
            e: 7.0,
            f: -2.0,
        };
        let inverse = t.invert().expect("invertible");
        let (x, y) = t.apply(1.25, -0.5);
        let (back_x, back_y) = inverse.apply(x, y);
        assert!((back_x - 1.25).abs() < 1e-12, "{back_x}");
        assert!((back_y + 0.5).abs() < 1e-12, "{back_y}");
        assert!(Transform::IDENTITY.invert().is_some());
    }
}
