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

/// One image draw: the pixels, where they go, and how they compose.
pub struct ImageDraw<'a> {
    /// The samples.
    pub image: ImageSource<'a>,
    /// Maps the image's unit square — `(0, 0)` at its bottom-left corner and
    /// `(1, 1)` at its top-right — to device space.
    pub unit_to_device: Transform,
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

            let Some((r, g, b, coverage)) = nearest(image, u, v) else {
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
