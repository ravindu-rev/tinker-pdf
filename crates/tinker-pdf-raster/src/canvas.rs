//! Pixels and compositing.
//!
//! Colour is stored per pixel in the format the caller asked for, and blending
//! is integer arithmetic throughout — a `u32` intermediate with rounded
//! division, never a float — so a composite is bit-identical everywhere
//! (ruling 4).

use crate::blend::BlendMode;

use crate::fill::Mask;

/// How pixels are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// One byte of grey per pixel.
    Gray8,
    /// Grey and alpha.
    GrayA8,
    /// Red, green and blue.
    Rgb8,
    /// Red, green, blue and alpha.
    Rgba8,
}

impl PixelFormat {
    /// Bytes per pixel.
    #[must_use]
    pub fn components(self) -> usize {
        match self {
            PixelFormat::Gray8 => 1,
            PixelFormat::GrayA8 => 2,
            PixelFormat::Rgb8 => 3,
            PixelFormat::Rgba8 => 4,
        }
    }

    /// Whether the format carries alpha.
    #[must_use]
    pub fn has_alpha(self) -> bool {
        matches!(self, PixelFormat::GrayA8 | PixelFormat::Rgba8)
    }
}

/// A colour in 8-bit sRGB with alpha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha, where 255 is opaque.
    pub a: u8,
}

impl Color {
    /// An opaque colour.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// Opaque black.
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    /// Opaque white.
    pub const WHITE: Color = Color::rgb(255, 255, 255);
    /// Nothing at all: the background a group or tile buffer starts from.
    pub const TRANSPARENT: Color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// The grey this colour reads as, by the usual luma weights.
    #[must_use]
    pub fn luma(self) -> u8 {
        // Integer weights summing to 1000, rounded: no float, no drift.
        let value =
            (u32::from(self.r) * 299 + u32::from(self.g) * 587 + u32::from(self.b) * 114 + 500)
                / 1000;
        value.min(255) as u8
    }
}

/// A rectangular grid of pixels.
#[derive(Clone, Debug)]
pub struct Canvas {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Storage format.
    pub format: PixelFormat,
    /// Bytes per row.
    pub stride: usize,
    /// The pixels.
    pub data: Vec<u8>,
}

impl Canvas {
    /// A canvas filled with `background`.
    ///
    /// A format without alpha must start opaque — a transparent tile is what
    /// a caller sees when a clipped render forgets this, and it is invisible
    /// until someone composites the tile over something dark.
    #[must_use]
    pub fn new(width: u32, height: u32, format: PixelFormat, background: Color) -> Canvas {
        let components = format.components();
        let stride = (width as usize).saturating_mul(components);
        let len = stride.saturating_mul(height as usize);

        let mut canvas = Canvas {
            width,
            height,
            format,
            stride,
            data: vec![0; len],
        };
        canvas.clear(background);
        canvas
    }

    /// Repaints every pixel.
    pub fn clear(&mut self, color: Color) {
        let pixel = self.encode(color);
        for chunk in self.data.chunks_exact_mut(self.format.components()) {
            chunk.copy_from_slice(&pixel[..chunk.len()]);
        }
    }

    fn encode(&self, color: Color) -> [u8; 4] {
        match self.format {
            PixelFormat::Gray8 => [color.luma(), 0, 0, 0],
            PixelFormat::GrayA8 => [color.luma(), color.a, 0, 0],
            PixelFormat::Rgb8 => [color.r, color.g, color.b, 0],
            PixelFormat::Rgba8 => [color.r, color.g, color.b, color.a],
        }
    }

    /// Composites `color` through `mask` onto the canvas.
    ///
    /// `alpha` scales the whole operation, which is what the graphics state's
    /// `ca` and `CA` do (8.6.4.4).
    pub fn fill_mask(&mut self, mask: &Mask, color: Color, alpha: f64) {
        self.fill_mask_with(mask, color, alpha, BlendMode::Normal);
    }

    /// As [`Canvas::fill_mask`], with a blend mode (11.3.5).
    pub fn fill_mask_with(&mut self, mask: &Mask, color: Color, alpha: f64, mode: BlendMode) {
        let alpha = if alpha.is_finite() {
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u32
        } else {
            255
        };
        if alpha == 0 {
            return;
        }

        let components = self.format.components();
        let source = self.encode(color);
        let color_alpha = u32::from(color.a);

        for row in 0..self.height {
            for col in 0..self.width {
                let coverage = u32::from(mask.at(col as i32, row as i32));
                if coverage == 0 {
                    continue;
                }
                // Coverage, the colour's own alpha, and the state's alpha all
                // multiply; rounding at each step keeps 255 mapping to 255.
                let effective = mul255(mul255(coverage, color_alpha), alpha);
                if effective == 0 {
                    continue;
                }

                let base = (row as usize) * self.stride + (col as usize) * components;
                blend(
                    self.data.get_mut(base..base + components),
                    &source,
                    effective,
                    self.format,
                    mode,
                );
            }
        }
    }

    /// Composites one pixel, for callers that sample rather than rasterize.
    ///
    /// An image is drawn by mapping device pixels back into its samples, so
    /// there is no coverage mask to go through — each pixel is decided
    /// individually and blended here.
    pub fn blend_pixel(&mut self, x: u32, y: u32, color: Color, alpha: f64) {
        self.blend_pixel_with(x, y, color, alpha, BlendMode::Normal);
    }

    /// As [`Canvas::blend_pixel`], with a blend mode (11.3.5).
    pub fn blend_pixel_with(&mut self, x: u32, y: u32, color: Color, alpha: f64, mode: BlendMode) {
        if x >= self.width || y >= self.height {
            return;
        }
        let alpha = if alpha.is_finite() {
            (alpha.clamp(0.0, 1.0) * 255.0).round() as u32
        } else {
            return;
        };
        let effective = mul255(alpha, u32::from(color.a));
        if effective == 0 {
            return;
        }

        let components = self.format.components();
        let source = self.encode(color);
        let base = (y as usize) * self.stride + (x as usize) * components;
        blend(
            self.data.get_mut(base..base + components),
            &source,
            effective,
            self.format,
            mode,
        );
    }

    /// The pixel at `(x, y)` as a colour, for tests and readback.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Color> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let components = self.format.components();
        let base = (y as usize) * self.stride + (x as usize) * components;
        let px = self.data.get(base..base + components)?;
        Some(match self.format {
            PixelFormat::Gray8 => {
                let v = *px.first()?;
                Color::rgb(v, v, v)
            }
            PixelFormat::GrayA8 => {
                let v = *px.first()?;
                Color {
                    r: v,
                    g: v,
                    b: v,
                    a: *px.get(1)?,
                }
            }
            PixelFormat::Rgb8 => Color::rgb(*px.first()?, *px.get(1)?, *px.get(2)?),
            PixelFormat::Rgba8 => Color {
                r: *px.first()?,
                g: *px.get(1)?,
                b: *px.get(2)?,
                a: *px.get(3)?,
            },
        })
    }
}

/// `a * b / 255`, rounded, in integers.
fn mul255(a: u32, b: u32) -> u32 {
    let product = a * b + 128;
    (product + (product >> 8)) >> 8
}

/// How many colour channels a format carries, before any alpha.
fn color_channels(format: PixelFormat) -> usize {
    match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 => 1,
        PixelFormat::Rgb8 | PixelFormat::Rgba8 => 3,
    }
}

/// Composites `source` onto `dst` with the given alpha and blend mode.
///
/// # The convention, which was not written down and was not consistent
///
/// Every channel here is **straight** — not premultiplied. `clear` and
/// `encode` write straight colour and `pixel` reads it back, so that is the
/// only convention the type can be said to have; but the compositing below
/// used to compute `Co = Cs·as + Cb·(1−as)`, which is the *premultiplied*
/// result. The two agree exactly when the backdrop is opaque and disagree
/// everywhere else.
///
/// That was invisible while the only canvas was a page with an opaque
/// background — which is every canvas the engine made until something needed
/// to draw into a transparent buffer. A tile rendered once and blitted, or a
/// transparency group, starts fully transparent, and there the old formula
/// darkens everything toward the uninitialised black underneath.
///
/// So: straight alpha, `Co = (Cs·as + Cb·ab·(1−as)) / ao` with
/// `ao = as + ab·(1−as)` (11.3.6). The opaque case is kept as a fast path
/// because it is still the overwhelmingly common one and it is exactly the
/// old arithmetic, so nothing about page rendering changes.
fn blend(
    dst: Option<&mut [u8]>,
    source: &[u8; 4],
    alpha: u32,
    format: PixelFormat,
    mode: BlendMode,
) {
    let Some(dst) = dst else { return };
    if alpha == 0 {
        return;
    }
    let inverse = 255 - alpha;
    let channels = color_channels(format);

    // A format without an alpha channel is opaque by construction.
    let backdrop_alpha = if format.has_alpha() {
        dst.get(channels).map_or(255, |a| u32::from(*a))
    } else {
        255
    };

    // 11.3.6: the blend function sees the backdrop only to the extent the
    // backdrop is there. Where it is absent the source passes through
    // unblended, which is what keeps `Multiply` from turning a transparent
    // buffer black.
    let mut blended = [0u32; 3];
    for (i, out) in blended.iter_mut().enumerate().take(channels) {
        let (Some(slot), Some(src)) = (dst.get(i), source.get(i)) else {
            continue;
        };
        let cs = u32::from(*src);
        let cb = u32::from(*slot);
        let mixed = mode.apply(cb, cs);
        *out = mul255(255 - backdrop_alpha, cs) + mul255(backdrop_alpha, mixed);
    }
    if mode.is_nonseparable() && channels == 3 {
        let cb = [
            dst.first().map_or(0, |v| u32::from(*v)),
            dst.get(1).map_or(0, |v| u32::from(*v)),
            dst.get(2).map_or(0, |v| u32::from(*v)),
        ];
        let cs = [
            u32::from(source[0]),
            u32::from(source[1]),
            u32::from(source[2]),
        ];
        let mixed = mode.apply_nonseparable(cb, cs);
        for (i, out) in blended.iter_mut().enumerate() {
            *out = mul255(255 - backdrop_alpha, cs[i]) + mul255(backdrop_alpha, mixed[i]);
        }
    }

    let out_alpha = alpha + mul255(backdrop_alpha, inverse);

    for (slot, mixed) in dst.iter_mut().zip(blended.iter()).take(channels) {
        let cb = u32::from(*slot);
        *slot = if backdrop_alpha == 255 {
            // The fast path, and the old arithmetic exactly.
            (mul255(*mixed, alpha) + mul255(cb, inverse)).min(255) as u8
        } else if out_alpha == 0 {
            0
        } else {
            let weighted = mul255(*mixed, alpha) + mul255(mul255(cb, backdrop_alpha), inverse);
            ((weighted * 255 + out_alpha / 2) / out_alpha).min(255) as u8
        };
    }

    if format.has_alpha() {
        if let Some(slot) = dst.get_mut(channels) {
            *slot = out_alpha.min(255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::fill;
    use crate::geom::{FillRule, Path};

    fn square_mask(x: f64, y: f64, w: f64, h: f64, size: u32) -> Mask {
        let mut path = Path::new();
        path.rect(x, y, w, h);
        fill(&path, FillRule::NonZero, 0, 0, size, size, 0.05)
    }

    #[test]
    fn a_canvas_starts_at_its_background() {
        let canvas = Canvas::new(4, 3, PixelFormat::Rgb8, Color::WHITE);
        assert_eq!(canvas.data.len(), 4 * 3 * 3);
        for y in 0..3 {
            for x in 0..4 {
                assert_eq!(canvas.pixel(x, y), Some(Color::WHITE));
            }
        }
    }

    #[test]
    fn full_coverage_replaces_the_pixel() {
        let mut canvas = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        canvas.fill_mask(&square_mask(2.0, 2.0, 4.0, 4.0, 8), Color::BLACK, 1.0);

        assert_eq!(canvas.pixel(3, 3), Some(Color::BLACK), "inside");
        assert_eq!(canvas.pixel(0, 0), Some(Color::WHITE), "outside");
    }

    #[test]
    fn half_alpha_lands_halfway() {
        let mut canvas = Canvas::new(4, 4, PixelFormat::Rgb8, Color::WHITE);
        canvas.fill_mask(&square_mask(0.0, 0.0, 4.0, 4.0, 4), Color::BLACK, 0.5);

        let px = canvas.pixel(1, 1).expect("a pixel");
        assert!(
            (126..=130).contains(&px.r),
            "black at half alpha over white is mid grey, got {}",
            px.r
        );
    }

    #[test]
    fn grayscale_stores_one_component() {
        let mut canvas = Canvas::new(4, 4, PixelFormat::Gray8, Color::WHITE);
        assert_eq!(canvas.data.len(), 16);
        canvas.fill_mask(&square_mask(0.0, 0.0, 4.0, 4.0, 4), Color::BLACK, 1.0);
        assert_eq!(canvas.pixel(2, 2).map(|c| c.r), Some(0));
    }

    #[test]
    fn alpha_formats_accumulate_opacity() {
        let mut canvas = Canvas::new(
            4,
            4,
            PixelFormat::Rgba8,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            },
        );
        assert_eq!(canvas.pixel(1, 1).map(|c| c.a), Some(0), "starts clear");

        canvas.fill_mask(&square_mask(0.0, 0.0, 4.0, 4.0, 4), Color::WHITE, 1.0);
        assert_eq!(canvas.pixel(1, 1).map(|c| c.a), Some(255), "becomes opaque");
    }

    #[test]
    fn compositing_is_bit_identical_across_runs() {
        let mask = square_mask(0.5, 0.5, 3.25, 2.75, 8);
        let paint = |c: &mut Canvas| c.fill_mask(&mask, Color::rgb(10, 200, 30), 0.37);

        let mut a = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        let mut b = Canvas::new(8, 8, PixelFormat::Rgb8, Color::WHITE);
        paint(&mut a);
        paint(&mut b);
        assert_eq!(a.data, b.data, "ruling 4");
    }

    /// Compositing onto a *transparent* backdrop.
    ///
    /// This is the case the old arithmetic got wrong, and it was invisible
    /// because every canvas the engine made had an opaque background. The old
    /// formula computed the premultiplied result and stored it as straight
    /// colour: half-covering a transparent buffer with red gave (128, 0, 0)
    /// at alpha 128 — a dark red that is really red-over-black — instead of
    /// (255, 0, 0) at alpha 128, which is red seen through half coverage.
    ///
    /// A tile rendered once and blitted, or a transparency group, starts
    /// fully transparent. Every one of them would have darkened toward the
    /// uninitialised black underneath.
    #[test]
    fn half_covering_a_transparent_buffer_keeps_the_colour() {
        let mut canvas = Canvas::new(1, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        canvas.blend_pixel(0, 0, Color::rgb(255, 0, 0), 0.5);

        let px = canvas.pixel(0, 0).expect("a pixel");
        assert!(
            px.r > 250,
            "the colour stays red rather than darkening to premultiplied: {px:?}"
        );
        assert!(
            (120..=136).contains(&px.a),
            "and the coverage lands in the alpha: {px:?}"
        );
    }

    /// The opaque path must be untouched by that fix, or every existing
    /// golden moves.
    #[test]
    fn compositing_onto_an_opaque_backdrop_is_unchanged() {
        let mut canvas = Canvas::new(1, 1, PixelFormat::Rgb8, Color::WHITE);
        canvas.blend_pixel(0, 0, Color::rgb(0, 0, 0), 0.5);

        let px = canvas.pixel(0, 0).expect("a pixel");
        assert!(
            (120..=136).contains(&px.r),
            "half black over white is mid grey: {px:?}"
        );
    }

    /// Two half-covering paints must reach the same place as one, whichever
    /// order they arrive in — the property that makes a group buffer usable
    /// at all.
    #[test]
    fn repeated_compositing_converges_rather_than_drifting() {
        let mut canvas = Canvas::new(1, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        for _ in 0..8 {
            canvas.blend_pixel(0, 0, Color::rgb(255, 0, 0), 0.5);
        }
        let px = canvas.pixel(0, 0).expect("a pixel");
        assert!(px.r > 250, "still red, not drifting dark: {px:?}");
        assert!(px.a > 250, "and now essentially opaque: {px:?}");
    }

    /// A blend mode must see a transparent backdrop as *absent*, not as
    /// black. `Multiply` against black is black, so getting this wrong turns
    /// every multiplied group into a silhouette.
    #[test]
    fn a_blend_mode_ignores_a_backdrop_that_is_not_there() {
        let mut canvas = Canvas::new(1, 1, PixelFormat::Rgba8, Color::TRANSPARENT);
        canvas.blend_pixel_with(0, 0, Color::rgb(255, 0, 0), 1.0, BlendMode::Multiply);

        let px = canvas.pixel(0, 0).expect("a pixel");
        assert!(
            px.r > 250,
            "multiplying against nothing leaves the source: {px:?}"
        );
    }

    /// And where the backdrop *is* there, the mode applies.
    #[test]
    fn a_blend_mode_applies_against_a_real_backdrop() {
        let mut canvas = Canvas::new(1, 1, PixelFormat::Rgb8, Color::rgb(128, 128, 128));
        canvas.blend_pixel_with(0, 0, Color::rgb(128, 128, 128), 1.0, BlendMode::Multiply);

        let px = canvas.pixel(0, 0).expect("a pixel");
        assert!(px.r < 80, "mid grey multiplied by itself is darker: {px:?}");
    }

    #[test]
    fn mul255_keeps_its_endpoints() {
        assert_eq!(mul255(255, 255), 255, "opaque over opaque stays opaque");
        assert_eq!(mul255(0, 255), 0);
        assert_eq!(mul255(255, 0), 0);
        assert_eq!(mul255(128, 255), 128);
    }

    #[test]
    fn degenerate_canvases_and_alphas_are_harmless() {
        let mut empty = Canvas::new(0, 0, PixelFormat::Rgb8, Color::WHITE);
        empty.fill_mask(&square_mask(0.0, 0.0, 1.0, 1.0, 1), Color::BLACK, 1.0);
        assert!(empty.data.is_empty());
        assert_eq!(empty.pixel(0, 0), None);

        let mut canvas = Canvas::new(2, 2, PixelFormat::Rgb8, Color::WHITE);
        for alpha in [0.0, -1.0, 2.0, f64::NAN] {
            canvas.fill_mask(&square_mask(0.0, 0.0, 2.0, 2.0, 2), Color::BLACK, alpha);
        }
    }
}
