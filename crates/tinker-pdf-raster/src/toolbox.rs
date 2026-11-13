//! The bitmap toolbox's arithmetic (`docs/design/bitmap-toolbox.md`): the
//! per-pixel maps and the reductions, over a buffer of pixels in any
//! [`PixelFormat`] and stride.
//!
//! On `PIXEL_PATHS`, so gamma's table is built through `tinker_pdf_math::pow`
//! and no transcendental of the platform's reaches a pixel (ruling 4); every
//! other form is integer arithmetic. `tinker_pdf::Bitmap`'s methods are one
//! call each into here with the bitmap's own fields, and add nothing.
//!
//! **Each form is the design's, as its test computes it.** A pixel is decoded
//! to sRGB as [`crate::Canvas::pixel`] decodes one and a colour encoded as a
//! canvas is filled with one; `the_decoding_and_encoding_are_the_canvass` holds
//! the two copies here to the canvas's so they cannot drift.

use std::collections::BTreeSet;

use crate::canvas::{cmyk_to_rgb, lab_bytes, lab_to_rgb, rgb_to_cmyk, Color, PixelFormat};

/// What a buffer of pixels is: its format, its size, how far apart its rows
/// start, and whether its colour is premultiplied by its alpha — the fields
/// `tinker_pdf::Bitmap` carries beside its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// How each pixel is stored.
    pub format: PixelFormat,
    /// Pixels across.
    pub width: u32,
    /// Rows.
    pub height: u32,
    /// Bytes from one row's start to the next.
    pub stride: usize,
    /// Whether each colour component has been multiplied by its pixel's
    /// alpha. Meaningless, and ignored, for a format with no alpha.
    pub premultiplied: bool,
}

impl Layout {
    /// The bytes one row's pixels take, `width * components`.
    fn row_bytes(&self) -> Option<usize> {
        (self.width as usize).checked_mul(self.format.components())
    }

    /// Whether `len` bytes laid out so are a picture: a width and a height
    /// that are not zero, a stride that holds a row, and every row the other
    /// fields promise inside the bytes — `Bitmap::to_png`'s condition. A map
    /// on anything else changes nothing and a reduction answers `None`.
    #[must_use]
    pub fn is_picture(&self, len: usize) -> bool {
        let Some(row) = self.row_bytes() else {
            return false;
        };
        if self.width == 0 || self.height == 0 || self.stride < row {
            return false;
        }
        (self.height as usize - 1)
            .checked_mul(self.stride)
            .and_then(|start| start.checked_add(row))
            .is_some_and(|end| end <= len)
    }

    /// Whether the colour is premultiplied in fact: asked for, and a format
    /// with an alpha to have been multiplied by.
    fn premultiplied(&self) -> bool {
        self.premultiplied && self.format.has_alpha()
    }
}

/// How many of a pixel's bytes are colour: all of them, or all but the alpha.
fn colours(format: PixelFormat) -> usize {
    match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 => 1,
        PixelFormat::Rgb8 | PixelFormat::Rgba8 | PixelFormat::LabA8 => 3,
        PixelFormat::CmykA8 => 4,
    }
}

/// Every pixel of a picture, each the `components` bytes of it and nothing
/// of a row's padding. `false`, with nothing done, for a buffer that is not a
/// picture.
fn each_pixel_mut(layout: &Layout, data: &mut [u8], mut f: impl FnMut(&mut [u8])) -> bool {
    if !layout.is_picture(data.len()) {
        return false;
    }
    let components = layout.format.components();
    let row = layout.width as usize * components;
    for y in 0..layout.height as usize {
        let start = y * layout.stride;
        if let Some(pixels) = data.get_mut(start..start + row) {
            for pixel in pixels.chunks_exact_mut(components) {
                f(pixel);
            }
        }
    }
    true
}

/// Every pixel of a picture, read, until `f` answers `false`. `false`, with
/// nothing read, for a buffer that is not a picture.
fn each_pixel(layout: &Layout, data: &[u8], mut f: impl FnMut(&[u8]) -> bool) -> bool {
    if !layout.is_picture(data.len()) {
        return false;
    }
    let components = layout.format.components();
    let row = layout.width as usize * components;
    for y in 0..layout.height as usize {
        let start = y * layout.stride;
        if let Some(pixels) = data.get(start..start + row) {
            for pixel in pixels.chunks_exact(components) {
                if !f(pixel) {
                    return true;
                }
            }
        }
    }
    true
}

/// `round(c * 255 / a)`, clamped to 255 for a component the premultiplied
/// convention does not allow, and 0 where the alpha is: `Bitmap::straightened`'s
/// arithmetic. `c * 255` is an integer and `a / 2` rounds the division half up.
fn straighten(c: u8, a: u8) -> u8 {
    let a = u32::from(a);
    (u32::from(c) * 255 + a / 2)
        .checked_div(a)
        .map_or(0, |v| v.min(255)) as u8
}

/// `round(s * a / 255)`, `Bitmap::premultiply`'s arithmetic: `s * a` is an
/// integer and 255 is odd, so the fraction is never a half.
fn premultiply(s: u8, a: u8) -> u8 {
    ((u32::from(s) * u32::from(a) + 127) / 255) as u8
}

/// Applies `f` to every pixel's straight colour: a premultiplied pixel is
/// straightened first and premultiplied again after, so `f` is written once,
/// for straight colour, and the alpha is never its to change.
fn map_straight(layout: &Layout, data: &mut [u8], mut f: impl FnMut(&mut [u8])) -> bool {
    let format = layout.format;
    let colours = colours(format);
    let premultiplied = layout.premultiplied();
    each_pixel_mut(layout, data, |pixel| {
        let alpha = pixel.get(colours).copied().unwrap_or(255);
        if premultiplied {
            for c in pixel.iter_mut().take(colours) {
                *c = straighten(*c, alpha);
            }
        }
        f(pixel);
        if premultiplied {
            for c in pixel.iter_mut().take(colours) {
                *c = premultiply(*c, alpha);
            }
        }
        // The alpha byte is the format's, written back whatever `f` did.
        if let Some(a) = pixel.get_mut(colours) {
            *a = alpha;
        }
    })
}

/// A pixel's colour as sRGB, and its alpha — 255 for a format with none —
/// decoded exactly as [`crate::Canvas::pixel`] decodes one: a grey replicated,
/// RGB as it is, ink through 8.6.4.4's relation, Lab through `tinker-pdf-color`.
#[must_use]
pub fn decode(format: PixelFormat, px: &[u8]) -> Option<Color> {
    Some(match format {
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
        PixelFormat::CmykA8 => {
            let (r, g, b) = cmyk_to_rgb(*px.first()?, *px.get(1)?, *px.get(2)?, *px.get(3)?);
            Color {
                r,
                g,
                b,
                a: *px.get(4)?,
            }
        }
        PixelFormat::LabA8 => {
            let (r, g, b) = lab_to_rgb(*px.first()?, *px.get(1)?, *px.get(2)?);
            Color {
                r,
                g,
                b,
                a: *px.get(3)?,
            }
        }
    })
}

/// A colour's components in a format's bytes, its alpha left out, exactly as
/// a canvas of that format is filled with it: grey by [`Color::luma`], ink by
/// the maximum-undercolour-removal inverse, Lab by [`lab_bytes`].
#[must_use]
pub fn encode(format: PixelFormat, color: Color) -> ([u8; 4], usize) {
    match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 => ([color.luma(), 0, 0, 0], 1),
        PixelFormat::Rgb8 | PixelFormat::Rgba8 => ([color.r, color.g, color.b, 0], 3),
        PixelFormat::CmykA8 => {
            let (c, m, y, k) = rgb_to_cmyk(color.r, color.g, color.b);
            ([c, m, y, k], 4)
        }
        PixelFormat::LabA8 => {
            let (l, a, b) = lab_bytes(color);
            ([l, a, b, 0], 3)
        }
    }
}

/// `color`'s components written over a pixel's colour bytes, its alpha byte
/// left alone.
fn write_colour(format: PixelFormat, pixel: &mut [u8], color: Color) {
    let (bytes, count) = encode(format, color);
    for (slot, byte) in pixel.iter_mut().zip(bytes.iter().take(count)) {
        *slot = *byte;
    }
}

/// A pixel's lightness: [`Color::luma`] of its sRGB.
fn lightness(format: PixelFormat, pixel: &[u8]) -> u8 {
    decode(format, pixel).map_or(0, Color::luma)
}

// ---- the maps -----------------------------------------------------------------

/// Gamma's table for `gamma`: `T[v] = floor(255 * pow(v / 255, gamma) + 0.5)`,
/// through `tinker_pdf_math::pow`, so every target builds the same 256 bytes.
/// `None` for a `gamma` that is not finite and greater than zero.
#[must_use]
pub fn gamma_table(gamma: f64) -> Option<[u8; 256]> {
    if !(gamma.is_finite() && gamma > 0.0) {
        return None;
    }
    let mut table = [0u8; 256];
    for (v, slot) in table.iter_mut().enumerate() {
        let x = 255.0 * tinker_pdf_math::pow(v as f64 / 255.0, gamma) + 0.5;
        // `floor` is correctly rounded by IEEE 754, so it is the same on
        // every target (ruling 4); a power of a value in 0..=1 to a positive
        // exponent is in 0..=1, and the clamp guards the cast all the same.
        *slot = x.floor().clamp(0.0, 255.0) as u8;
    }
    Some(table)
}

/// Every tone component through gamma's table: a grey or an RGB component
/// becomes `T[v]`, an ink `255 - T[255 - v]`, Lab's `L*` byte `T[v]` with
/// `a*` and `b*` kept. Changes nothing, and answers `false`, for a buffer
/// that is not a picture or a `gamma` that is not finite and positive.
pub fn gamma(layout: &Layout, data: &mut [u8], gamma: f64) -> bool {
    let Some(table) = gamma_table(gamma) else {
        return false;
    };
    let format = layout.format;
    let tone = |v: u8| table[usize::from(v)];
    map_straight(layout, data, |pixel| match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 => {
            if let Some(v) = pixel.first_mut() {
                *v = tone(*v);
            }
        }
        PixelFormat::Rgb8 | PixelFormat::Rgba8 => {
            for v in pixel.iter_mut().take(3) {
                *v = tone(*v);
            }
        }
        PixelFormat::CmykA8 => {
            for v in pixel.iter_mut().take(4) {
                *v = 255 - tone(255 - *v);
            }
        }
        PixelFormat::LabA8 => {
            if let Some(l) = pixel.first_mut() {
                *l = tone(*l);
            }
        }
    })
}

/// Every colour byte `v` becomes `255 - v`, in every format; the alpha is
/// kept. `false`, with nothing done, for a buffer that is not a picture.
pub fn invert(layout: &Layout, data: &mut [u8]) -> bool {
    let colours = colours(layout.format);
    map_straight(layout, data, |pixel| {
        for v in pixel.iter_mut().take(colours) {
            *v = 255 - *v;
        }
    })
}

/// Lightness inverted and chroma kept: in sRGB each component `c` becomes
/// `clamp(c + 255 - 2Y, 0, 255)` for the pixel's lightness `Y`; a grey
/// becomes `255 - v`; Lab's `L*` byte `255 - L` with `a*` and `b*` kept; ink
/// is decoded to sRGB, mapped, and encoded back. `false`, with nothing done,
/// for a buffer that is not a picture.
pub fn invert_luminance(layout: &Layout, data: &mut [u8]) -> bool {
    let format = layout.format;
    map_straight(layout, data, |pixel| match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 | PixelFormat::LabA8 => {
            if let Some(v) = pixel.first_mut() {
                *v = 255 - *v;
            }
        }
        PixelFormat::Rgb8 | PixelFormat::Rgba8 | PixelFormat::CmykA8 => {
            let Some(color) = decode(format, pixel) else {
                return;
            };
            let shift = 255 - 2 * i32::from(color.luma());
            let turn = |c: u8| (i32::from(c) + shift).clamp(0, 255) as u8;
            write_colour(
                format,
                pixel,
                Color::rgb(turn(color.r), turn(color.g), turn(color.b)),
            );
        }
    })
}

/// One channel of the ramp from `dark` to `light` at lightness `y`:
/// `(dark * (255 - y) + light * y + 127) / 255`.
fn ramp(dark: u8, light: u8, y: u8) -> u8 {
    let y = u32::from(y);
    ((u32::from(dark) * (255 - y) + u32::from(light) * y + 127) / 255) as u8
}

/// Every pixel's lightness on the ramp from `dark` to `light`, each sRGB
/// channel [`ramp`]'s, the ramp's colour encoded into the format; the
/// alphas of `dark` and `light` are not read. `false`, with nothing done, for
/// a buffer that is not a picture.
pub fn tint(layout: &Layout, data: &mut [u8], dark: Color, light: Color) -> bool {
    let format = layout.format;
    map_straight(layout, data, |pixel| {
        let y = lightness(format, pixel);
        let colour = Color::rgb(
            ramp(dark.r, light.r, y),
            ramp(dark.g, light.g, y),
            ramp(dark.b, light.b, y),
        );
        write_colour(format, pixel, colour);
    })
}

/// Every pixel its own lightness, in its own format: a grey unchanged, RGB
/// `(Y, Y, Y)`, ink `rgb_to_cmyk(Y, Y, Y)`, Lab its `L*` with `a*` and `b*`
/// at the encoded zero, 128. `false`, with nothing done, for a buffer that is
/// not a picture.
pub fn grayscale(layout: &Layout, data: &mut [u8]) -> bool {
    let format = layout.format;
    map_straight(layout, data, |pixel| match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 => {}
        PixelFormat::Rgb8 | PixelFormat::Rgba8 | PixelFormat::CmykA8 => {
            let y = lightness(format, pixel);
            write_colour(format, pixel, Color::rgb(y, y, y));
        }
        PixelFormat::LabA8 => {
            for v in pixel.iter_mut().skip(1).take(2) {
                *v = 128;
            }
        }
    })
}

// ---- the reductions -------------------------------------------------------------

/// A pixel's bytes as one number, so two pixels of one format are one number
/// exactly when every byte of them is equal.
fn packed(pixel: &[u8]) -> u64 {
    pixel
        .iter()
        .fold(0u64, |word, &byte| (word << 8) | u64::from(byte))
}

/// The number of distinct pixels, every byte compared as stored, or `limit`
/// once that many are found: counting stops there, so what is held is at most
/// `limit` pixels. `None` for a buffer that is not a picture.
#[must_use]
pub fn colour_count(layout: &Layout, data: &[u8], limit: usize) -> Option<usize> {
    if !layout.is_picture(data.len()) {
        return None;
    }
    if limit == 0 {
        return Some(0);
    }
    let mut seen = BTreeSet::new();
    each_pixel(layout, data, |pixel| {
        seen.insert(packed(pixel));
        seen.len() < limit
    });
    Some(seen.len().min(limit))
}

/// Whether every pixel is the first, every byte compared as stored. `None`
/// for a buffer that is not a picture.
#[must_use]
pub fn is_unicolour(layout: &Layout, data: &[u8]) -> Option<bool> {
    let mut first: Option<u64> = None;
    let mut same = true;
    let picture = each_pixel(layout, data, |pixel| {
        let this = packed(pixel);
        match first {
            None => first = Some(this),
            Some(first) if first != this => same = false,
            Some(_) => {}
        }
        same
    });
    picture.then_some(same)
}

/// Whether every pixel is opaque black or opaque white: its sRGB `(0, 0, 0)`
/// or `(255, 255, 255)` and its alpha, where the format has one, 255. `None`
/// for a buffer that is not a picture.
#[must_use]
pub fn is_monochrome(layout: &Layout, data: &[u8]) -> Option<bool> {
    let format = layout.format;
    let mut monochrome = true;
    let picture = each_pixel(layout, data, |pixel| {
        monochrome = decode(format, pixel)
            .is_some_and(|c| c.a == 255 && matches!((c.r, c.g, c.b), (0, 0, 0) | (255, 255, 255)));
        monochrome
    });
    picture.then_some(monochrome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Canvas;

    const FORMATS: [PixelFormat; 6] = [
        PixelFormat::Gray8,
        PixelFormat::GrayA8,
        PixelFormat::Rgb8,
        PixelFormat::Rgba8,
        PixelFormat::CmykA8,
        PixelFormat::LabA8,
    ];

    /// Colours over the cube: every corner and a lattice through it, at
    /// three alphas.
    fn colours_over_the_cube() -> Vec<Color> {
        let levels = [0u8, 1, 37, 64, 127, 128, 200, 254, 255];
        let mut out = Vec::new();
        for r in levels {
            for g in levels {
                for b in levels {
                    for a in [0u8, 128, 255] {
                        out.push(Color { r, g, b, a });
                    }
                }
            }
        }
        out
    }

    /// **The decoding and encoding are the canvas's.** A canvas filled with a
    /// colour holds the bytes [`encode`] writes, and a canvas's pixel reads
    /// back as [`decode`] reads its bytes, in every format.
    #[test]
    fn the_decoding_and_encoding_are_the_canvass() {
        for format in FORMATS {
            for color in colours_over_the_cube() {
                let canvas = Canvas::new(1, 1, format, color);
                let (bytes, count) = encode(format, color);
                assert_eq!(
                    &canvas.data[..count],
                    &bytes[..count],
                    "{format:?}: {color:?} encoded"
                );
                assert_eq!(
                    decode(format, &canvas.data),
                    canvas.pixel(0, 0),
                    "{format:?}: {color:?} decoded"
                );
            }
        }
    }

    /// Gamma's table is its closed form through the one `pow` every target
    /// shares, at exponents either side of one, and is refused for one that is
    /// not a finite positive number.
    #[test]
    fn gamma_s_table_is_its_closed_form() {
        for g in [0.45, 0.5, 1.0, 1.8, 2.0, 2.2, 3.0] {
            let table = gamma_table(g).expect("a positive exponent");
            for v in 0..=255u8 {
                let want = (255.0 * tinker_pdf_math::pow(f64::from(v) / 255.0, g) + 0.5).floor();
                assert_eq!(f64::from(table[usize::from(v)]), want, "g {g}, v {v}");
            }
            assert_eq!(
                (table[0], table[255]),
                (0, 255),
                "black and white are fixed"
            );
        }
        let identity = gamma_table(1.0).expect("one");
        assert!(identity
            .iter()
            .enumerate()
            .all(|(v, &t)| usize::from(t) == v));
        for g in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(gamma_table(g), None, "{g} is no exponent");
        }
    }

    /// A buffer is a picture only with a size, a stride that holds a row and
    /// every row inside it, and every operation refuses one that is not —
    /// changing nothing, answering `None`.
    #[test]
    fn a_buffer_that_is_not_a_picture_is_refused_whole() {
        let layout = |width, height, stride| Layout {
            format: PixelFormat::Rgb8,
            width,
            height,
            stride,
            premultiplied: false,
        };
        assert!(layout(2, 2, 6).is_picture(12));
        assert!(
            layout(2, 2, 8).is_picture(14),
            "the last row needs no padding"
        );
        assert!(!layout(2, 2, 8).is_picture(13), "the last row is cut");
        assert!(
            !layout(2, 2, 5).is_picture(100),
            "a stride narrower than a row"
        );
        assert!(!layout(0, 2, 0).is_picture(100), "no width");
        assert!(!layout(2, 0, 6).is_picture(100), "no height");
        assert!(!layout(u32::MAX, 2, usize::MAX).is_picture(usize::MAX));
        let short = layout(2, 2, 6);
        let mut data = vec![7u8; 11];
        let before = data.clone();
        assert!(!invert(&short, &mut data));
        assert!(!gamma(&short, &mut data, 2.0));
        assert!(!invert_luminance(&short, &mut data));
        assert!(!tint(&short, &mut data, Color::BLACK, Color::WHITE));
        assert!(!grayscale(&short, &mut data));
        assert_eq!(data, before, "nothing of a short buffer was written");
        assert_eq!(colour_count(&short, &data, 10), None);
        assert_eq!(is_unicolour(&short, &data), None);
        assert_eq!(is_monochrome(&short, &data), None);
    }

    /// The straightening and premultiplying the maps wrap themselves in are
    /// the facade's: an opaque pixel is untouched, a transparent one stays
    /// zero, and an impossible component clamps.
    #[test]
    fn premultiplied_colour_is_mapped_straight() {
        assert_eq!(straighten(100, 255), 100);
        assert_eq!(straighten(50, 0), 0);
        assert_eq!(straighten(200, 100), 255, "c > a clamps");
        assert_eq!(straighten(64, 128), 128);
        assert_eq!(premultiply(128, 128), 64);
        assert_eq!(premultiply(255, 0), 0);
        let layout = Layout {
            format: PixelFormat::Rgba8,
            width: 2,
            height: 1,
            stride: 8,
            premultiplied: true,
        };
        // An opaque pixel and one at half alpha holding half-dark grey.
        let mut data = vec![10, 20, 30, 255, 64, 64, 64, 128];
        assert!(invert(&layout, &mut data));
        assert_eq!(
            &data[..4],
            &[245, 235, 225, 255],
            "opaque: the straight map"
        );
        // Straight 128 inverted is 127, premultiplied by 128 is 64.
        assert_eq!(&data[4..], &[64, 64, 64, 128]);
    }

    /// The counting reductions stop where they say: the count at its limit,
    /// the unicolour test at the first pixel that differs.
    #[test]
    fn the_counts_stop_at_their_limit() {
        let layout = Layout {
            format: PixelFormat::Gray8,
            width: 256,
            height: 1,
            stride: 256,
            premultiplied: false,
        };
        let data: Vec<u8> = (0..=255).collect();
        assert_eq!(colour_count(&layout, &data, 0), Some(0));
        assert_eq!(colour_count(&layout, &data, 10), Some(10));
        assert_eq!(colour_count(&layout, &data, 256), Some(256));
        assert_eq!(colour_count(&layout, &data, 1_000), Some(256));
        assert_eq!(is_unicolour(&layout, &data), Some(false));
        let flat = vec![9u8; 256];
        assert_eq!(colour_count(&layout, &flat, 1_000), Some(1));
        assert_eq!(is_unicolour(&layout, &flat), Some(true));
        assert_eq!(is_monochrome(&layout, &flat), Some(false));
        let bilevel: Vec<u8> = (0..256).map(|x| if x % 3 == 0 { 0 } else { 255 }).collect();
        assert_eq!(is_monochrome(&layout, &bilevel), Some(true));
    }

    /// Ink is black by its light: all four inks at 255 is black, and so is
    /// `K` alone; a translucent black is not monochrome.
    #[test]
    fn ink_is_black_by_the_light_it_leaves() {
        let layout = Layout {
            format: PixelFormat::CmykA8,
            width: 3,
            height: 1,
            stride: 15,
            premultiplied: false,
        };
        let data = [
            255, 255, 255, 255, 255, //
            0, 0, 0, 255, 255, //
            0, 0, 0, 0, 255,
        ];
        assert_eq!(is_monochrome(&layout, &data), Some(true));
        let mut translucent = data;
        translucent[14] = 254;
        assert_eq!(is_monochrome(&layout, &translucent), Some(false));
    }
}
