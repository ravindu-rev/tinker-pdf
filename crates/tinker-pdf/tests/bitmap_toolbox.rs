//! The bitmap toolbox held to its closed forms (`docs/design/bitmap-toolbox.md`).
//!
//! Every reviewed raster golden in `tests/goldens/` and `tests/goldens/surfaces/`
//! is read through `golden_support`'s PPM reader — the goldens are never
//! rewritten here (H-133) — and laid out in each of the eight pixel formats,
//! and, where the format has an alpha, with an alpha plane this file lays on,
//! straight and premultiplied. Each map, each reduction, the scale and the
//! subsample run over every one of those, and every byte and every answer is
//! held to the closed form the design states, computed here from the formula
//! and the canvas's own colour relations (`tinker_pdf_raster::canvas`), never
//! from the toolbox.
//!
//! Gamma is taken at exponents of one half, one and two, whose powers this
//! file computes exactly in integers: `floor(255 * pow(v / 255, g) + 0.5)` is
//! `round(sqrt(255 v))`, `v` and `(v^2 + 127) / 255` there, and no value of
//! either lies within a rounding of `pow` of a tie. A scale's average is
//! computed from its definition with rationals in `u128`: each pixel's
//! rectangle of the source, measured in fractions of a sample, against each
//! sample's.

mod golden_support;

use std::collections::BTreeSet;
use std::path::Path;

use tinker_pdf::{Bitmap, Color, PixelFormat, RenderWarning};
use tinker_pdf_raster::canvas::{cmyk_to_rgb, lab_bytes, lab_to_rgb, rgb_to_cmyk};

const FORMATS: [PixelFormat; 8] = [
    PixelFormat::Gray8,
    PixelFormat::GrayA8,
    PixelFormat::Rgb8,
    PixelFormat::Rgba8,
    PixelFormat::CmykA8,
    PixelFormat::LabA8,
    PixelFormat::Bgr8,
    PixelFormat::Bgra8,
];

// ---- the inputs -------------------------------------------------------------------

/// Every reviewed raster golden, by its name, as the `Rgb8` picture it holds.
fn goldens() -> Vec<(String, Bitmap)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens");
    let mut paths = Vec::new();
    for dir in [root.clone(), root.join("surfaces")] {
        for entry in std::fs::read_dir(&dir).expect("the goldens are committed") {
            let path = entry.expect("a directory entry").path();
            if path.extension().is_some_and(|extension| extension == "ppm") {
                paths.push(path);
            }
        }
    }
    paths.sort();
    assert!(
        paths.len() >= 9,
        "the nine operator families' goldens at least: {paths:?}"
    );
    paths
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(&path).expect("a committed golden");
            let name = path.display().to_string();
            let bitmap = golden_support::ppm_bitmap(&bytes)
                .unwrap_or_else(|| panic!("{name} is a raster golden"));
            (name, bitmap)
        })
        .collect()
}

/// Rec.601 luma in thousandths, rounded: `Color::luma`'s closed form.
fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114 + 500) / 1000) as u8
}

/// The alpha plane laid on a golden: every level, zero and opaque among
/// them, in a pattern no golden's content lines up with.
fn alpha(x: u32, y: u32) -> u8 {
    ((x * 37 + y * 101 + (x * y) % 7) % 256) as u8
}

/// How many of a format's bytes are colour.
fn colours(format: PixelFormat) -> usize {
    match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 => 1,
        PixelFormat::Rgb8
        | PixelFormat::Rgba8
        | PixelFormat::Bgr8
        | PixelFormat::Bgra8
        | PixelFormat::LabA8 => 3,
        PixelFormat::CmykA8 => 4,
    }
}

/// `round(s * a / 255)`.
fn premultiply(s: u8, a: u8) -> u8 {
    ((u32::from(s) * u32::from(a) + 127) / 255) as u8
}

/// `round(c * 255 / a)`, clamped, 0 where `a` is.
fn straighten(c: u8, a: u8) -> u8 {
    if a == 0 {
        return 0;
    }
    ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
}

/// A golden laid out in `format`: a grey of its luma, its RGB, red first or
/// blue first, an ink of its complement over a black ink that runs across it
/// so rich and plain inks both occur, its RGB bytes read as Lab bytes — any
/// bytes are a Lab bitmap — and the alpha plane where the format has one,
/// premultiplied when asked.
fn variant(golden: &Bitmap, format: PixelFormat, premultiplied: bool) -> Bitmap {
    let (width, height) = (golden.width, golden.height);
    let mut data = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let at = ((y * width + x) * 3) as usize;
            let (r, g, b) = (golden.data[at], golden.data[at + 1], golden.data[at + 2]);
            let a = alpha(x, y);
            let mut pixel = match format {
                PixelFormat::Gray8 => vec![luma(r, g, b)],
                PixelFormat::GrayA8 => vec![luma(r, g, b), a],
                PixelFormat::Rgb8 => vec![r, g, b],
                PixelFormat::Rgba8 | PixelFormat::LabA8 => vec![r, g, b, a],
                PixelFormat::Bgr8 => vec![b, g, r],
                PixelFormat::Bgra8 => vec![b, g, r, a],
                PixelFormat::CmykA8 => {
                    vec![255 - r, 255 - g, 255 - b, ((x + 3 * y) % 256) as u8, a]
                }
            };
            if premultiplied && format.has_alpha() {
                let colours = colours(format);
                for c in pixel.iter_mut().take(colours) {
                    *c = premultiply(*c, a);
                }
            }
            data.extend_from_slice(&pixel);
        }
    }
    Bitmap {
        width,
        height,
        format,
        stride: width as usize * format.components(),
        data,
        warnings: Vec::new(),
        premultiplied: premultiplied && format.has_alpha(),
        stats: None,
    }
}

/// Every variant of a golden: each format straight, and each format with
/// an alpha premultiplied too.
fn variants(golden: &Bitmap) -> Vec<Bitmap> {
    let mut out = Vec::new();
    for format in FORMATS {
        out.push(variant(golden, format, false));
        if format.has_alpha() {
            out.push(variant(golden, format, true));
        }
    }
    out
}

// ---- the closed forms, computed here ---------------------------------------------

/// A pixel's sRGB as the canvas reads its format.
fn srgb(format: PixelFormat, px: &[u8]) -> (u8, u8, u8) {
    match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 => (px[0], px[0], px[0]),
        PixelFormat::Rgb8 | PixelFormat::Rgba8 => (px[0], px[1], px[2]),
        PixelFormat::Bgr8 | PixelFormat::Bgra8 => (px[2], px[1], px[0]),
        PixelFormat::CmykA8 => cmyk_to_rgb(px[0], px[1], px[2], px[3]),
        PixelFormat::LabA8 => lab_to_rgb(px[0], px[1], px[2]),
    }
}

/// A colour's components written over a pixel as the canvas encodes it.
fn write(format: PixelFormat, px: &mut [u8], (r, g, b): (u8, u8, u8)) {
    match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 => px[0] = luma(r, g, b),
        PixelFormat::Rgb8 | PixelFormat::Rgba8 => px[..3].copy_from_slice(&[r, g, b]),
        PixelFormat::Bgr8 | PixelFormat::Bgra8 => px[..3].copy_from_slice(&[b, g, r]),
        PixelFormat::CmykA8 => {
            let (c, m, y, k) = rgb_to_cmyk(r, g, b);
            px[..4].copy_from_slice(&[c, m, y, k]);
        }
        PixelFormat::LabA8 => {
            let (l, a, bb) = lab_bytes(Color::rgb(r, g, b));
            px[..3].copy_from_slice(&[l, a, bb]);
        }
    }
}

/// What a map should make of `bitmap`: `map` over each pixel's straight
/// colour, a premultiplied pixel straightened first and premultiplied after,
/// the alpha kept.
fn expected(bitmap: &Bitmap, map: &dyn Fn(PixelFormat, &mut [u8])) -> Vec<u8> {
    let format = bitmap.format;
    let colours = colours(format);
    let premultiplied = bitmap.premultiplied && format.has_alpha();
    let mut data = bitmap.data.clone();
    for pixel in data.chunks_exact_mut(format.components()) {
        let a = if format.has_alpha() {
            pixel[colours]
        } else {
            255
        };
        if premultiplied {
            for c in &mut pixel[..colours] {
                *c = straighten(*c, a);
            }
        }
        map(format, pixel);
        if premultiplied {
            for c in &mut pixel[..colours] {
                *c = premultiply(*c, a);
            }
        }
        if format.has_alpha() {
            pixel[colours] = a;
        }
    }
    data
}

/// Gamma's table at an exponent this file computes exactly.
fn gamma_table(gamma: f64) -> [u8; 256] {
    let mut table = [0u8; 256];
    for (v, slot) in table.iter_mut().enumerate() {
        let v = v as u32;
        let level: u32 = if gamma == 1.0 {
            v
        } else if gamma == 2.0 {
            (v * v + 127) / 255
        } else {
            // One half: the integer nearest sqrt(255 v), which is never a tie
            // because (2t + 1)^2 is odd and 4 * 255 v even.
            let n = 255 * v;
            let mut t = 0u32;
            while (t + 1) * (t + 1) <= n {
                t += 1;
            }
            if 4 * n >= (2 * t + 1) * (2 * t + 1) {
                t + 1
            } else {
                t
            }
        };
        *slot = level as u8;
    }
    table
}

/// Gamma's closed form for one format's pixel.
fn gamma_pixel(table: &[u8; 256], format: PixelFormat, px: &mut [u8]) {
    let t = |v: u8| table[usize::from(v)];
    match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 | PixelFormat::LabA8 => px[0] = t(px[0]),
        PixelFormat::Rgb8 | PixelFormat::Rgba8 | PixelFormat::Bgr8 | PixelFormat::Bgra8 => {
            for v in &mut px[..3] {
                *v = t(*v);
            }
        }
        PixelFormat::CmykA8 => {
            for v in &mut px[..4] {
                *v = 255 - t(255 - *v);
            }
        }
    }
}

/// Invert's closed form: every colour byte complemented.
fn invert_pixel(format: PixelFormat, px: &mut [u8]) {
    for v in &mut px[..colours(format)] {
        *v = 255 - *v;
    }
}

/// Invert by luminance's closed form.
fn invert_luminance_pixel(format: PixelFormat, px: &mut [u8]) {
    match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 | PixelFormat::LabA8 => px[0] = 255 - px[0],
        PixelFormat::Rgb8
        | PixelFormat::Rgba8
        | PixelFormat::Bgr8
        | PixelFormat::Bgra8
        | PixelFormat::CmykA8 => {
            let (r, g, b) = srgb(format, px);
            let y = i32::from(luma(r, g, b));
            let turn = |c: u8| (i32::from(c) + 255 - 2 * y).clamp(0, 255) as u8;
            write(format, px, (turn(r), turn(g), turn(b)));
        }
    }
}

/// The two ends of the test's tint, neither grey.
const DARK: Color = Color::rgb(20, 40, 160);
const LIGHT: Color = Color::rgb(250, 230, 200);

/// Tint's closed form, from `DARK` to `LIGHT`.
fn tint_pixel(format: PixelFormat, px: &mut [u8]) {
    let (r, g, b) = srgb(format, px);
    let y = u32::from(luma(r, g, b));
    let ramp = |dark: u8, light: u8| {
        ((u32::from(dark) * (255 - y) + u32::from(light) * y + 127) / 255) as u8
    };
    write(
        format,
        px,
        (
            ramp(DARK.r, LIGHT.r),
            ramp(DARK.g, LIGHT.g),
            ramp(DARK.b, LIGHT.b),
        ),
    );
}

/// Grayscale's closed form.
fn grayscale_pixel(format: PixelFormat, px: &mut [u8]) {
    match format {
        PixelFormat::Gray8 | PixelFormat::GrayA8 => {}
        PixelFormat::Rgb8
        | PixelFormat::Rgba8
        | PixelFormat::Bgr8
        | PixelFormat::Bgra8
        | PixelFormat::CmykA8 => {
            let (r, g, b) = srgb(format, px);
            let y = luma(r, g, b);
            write(format, px, (y, y, y));
        }
        PixelFormat::LabA8 => {
            px[1] = 128;
            px[2] = 128;
        }
    }
}

/// Every map the toolbox has, by name: what it does to a bitmap through the
/// facade, and its closed form.
type Map = (
    &'static str,
    fn(&mut Bitmap),
    Box<dyn Fn(PixelFormat, &mut [u8])>,
);

fn maps() -> Vec<Map> {
    let mut maps: Vec<Map> = vec![
        ("invert", |b| b.invert(), Box::new(invert_pixel)),
        (
            "invert by luminance",
            |b| b.invert_luminance(),
            Box::new(invert_luminance_pixel),
        ),
        ("tint", |b| b.tint(DARK, LIGHT), Box::new(tint_pixel)),
        ("grayscale", |b| b.grayscale(), Box::new(grayscale_pixel)),
    ];
    let gammas: [(&'static str, fn(&mut Bitmap), f64); 3] = [
        ("gamma 1/2", |b| b.gamma(0.5), 0.5),
        ("gamma 1", |b| b.gamma(1.0), 1.0),
        ("gamma 2", |b| b.gamma(2.0), 2.0),
    ];
    for (name, apply, g) in gammas {
        let table = gamma_table(g);
        maps.push((
            name,
            apply,
            Box::new(move |format: PixelFormat, px: &mut [u8]| {
                gamma_pixel(&table, format, px);
            }),
        ));
    }
    maps
}

// ---- the tests ----------------------------------------------------------------------

/// **Each map is its closed form over a committed render**: every golden, in
/// every format, straight and premultiplied, every byte of every map the
/// design's formula — and the bitmap's other fields untouched.
#[test]
fn each_map_is_its_closed_form_over_a_committed_render() {
    let maps = maps();
    let mut held = 0usize;
    for (name, golden) in goldens() {
        for bitmap in variants(&golden) {
            for (map, apply, form) in &maps {
                let want = expected(&bitmap, form.as_ref());
                let mut got = bitmap.clone();
                apply(&mut got);
                let what = format!(
                    "{name}, {map}, {:?}{}",
                    bitmap.format,
                    if bitmap.premultiplied {
                        " premultiplied"
                    } else {
                        ""
                    }
                );
                assert_eq!(
                    (
                        got.width,
                        got.height,
                        got.format,
                        got.stride,
                        got.premultiplied
                    ),
                    (
                        bitmap.width,
                        bitmap.height,
                        bitmap.format,
                        bitmap.stride,
                        bitmap.premultiplied
                    ),
                    "{what}: a map changes the bytes alone"
                );
                if got.data != want {
                    let at = got
                        .data
                        .iter()
                        .zip(&want)
                        .position(|(a, b)| a != b)
                        .unwrap_or(0);
                    panic!(
                        "{what}: byte {at} is {} where the closed form is {}",
                        got.data[at], want[at]
                    );
                }
                held += 1;
            }
        }
    }
    println!("bitmap-toolbox maps: RAN, {held} bitmaps held to their closed forms");
}

/// A gamma that is no exponent changes nothing, and gamma 1 is the identity.
#[test]
fn a_gamma_that_is_no_exponent_changes_nothing() {
    let (_, golden) = goldens().remove(0);
    for bitmap in variants(&golden) {
        for g in [0.0, -2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut got = bitmap.clone();
            got.gamma(g);
            assert_eq!(got.data, bitmap.data, "{g}, {:?}", bitmap.format);
        }
        if !bitmap.premultiplied {
            let mut identity = bitmap.clone();
            identity.gamma(1.0);
            assert_eq!(identity.data, bitmap.data, "{:?}", bitmap.format);
        }
    }
}

/// The distinct pixels of a bitmap, every byte compared.
fn distinct(bitmap: &Bitmap) -> BTreeSet<Vec<u8>> {
    let row = bitmap.width as usize * bitmap.components();
    (0..bitmap.height as usize)
        .flat_map(|y| {
            bitmap.data[y * bitmap.stride..y * bitmap.stride + row]
                .chunks_exact(bitmap.components())
                .map(<[u8]>::to_vec)
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Whether every pixel is opaque black or opaque white by the canvas's
/// reading of its format.
fn monochrome(bitmap: &Bitmap) -> bool {
    let format = bitmap.format;
    let colours = colours(format);
    let row = bitmap.width as usize * bitmap.components();
    (0..bitmap.height as usize).all(|y| {
        bitmap.data[y * bitmap.stride..y * bitmap.stride + row]
            .chunks_exact(bitmap.components())
            .all(|px| {
                let opaque = !format.has_alpha() || px[colours] == 255;
                opaque && matches!(srgb(format, px), (0, 0, 0) | (255, 255, 255))
            })
    })
}

/// A golden turned to black and white by its luma, laid out in `format`
/// with every alpha opaque: the shapes on which the monochrome test answers
/// yes. Its ink is rich black where the luma is odd, black ink alone where it
/// is even, which are both black.
fn bilevel(golden: &Bitmap, format: PixelFormat) -> Bitmap {
    let mut out = variant(golden, format, false);
    let components = format.components();
    for (index, pixel) in out.data.chunks_exact_mut(components).enumerate() {
        let at = index * 3;
        let y = luma(golden.data[at], golden.data[at + 1], golden.data[at + 2]);
        let black = y < 128;
        let bytes: Vec<u8> = match (format, black) {
            (PixelFormat::Gray8, _) => vec![if black { 0 } else { 255 }],
            (PixelFormat::GrayA8, _) => vec![if black { 0 } else { 255 }, 255],
            (PixelFormat::Rgb8, true) => vec![0, 0, 0],
            (PixelFormat::Rgb8, false) => vec![255, 255, 255],
            (PixelFormat::Rgba8, true) => vec![0, 0, 0, 255],
            (PixelFormat::Rgba8, false) => vec![255, 255, 255, 255],
            (PixelFormat::Bgr8, true) => vec![0, 0, 0],
            (PixelFormat::Bgr8, false) => vec![255, 255, 255],
            (PixelFormat::Bgra8, true) => vec![0, 0, 0, 255],
            (PixelFormat::Bgra8, false) => vec![255, 255, 255, 255],
            (PixelFormat::CmykA8, true) if y % 2 == 1 => vec![255, 255, 255, 255, 255],
            (PixelFormat::CmykA8, true) => vec![0, 0, 0, 255, 255],
            (PixelFormat::CmykA8, false) => vec![0, 0, 0, 0, 255],
            (PixelFormat::LabA8, _) => {
                let (l, a, b) = lab_bytes(if black { Color::BLACK } else { Color::WHITE });
                vec![l, a, b, 255]
            }
        };
        pixel.copy_from_slice(&bytes);
    }
    out
}

/// **Each count and test is its closed form over a committed render**: the
/// colour count at several limits, the unicolour test and the monochrome
/// test, over every variant of every golden, over each golden made black and
/// white in every format — with and without one pixel spoiling it — and over
/// a flat bitmap of each.
#[test]
fn each_count_and_test_is_its_closed_form_over_a_committed_render() {
    let mut held = 0usize;
    for (name, golden) in goldens() {
        let mut bitmaps = variants(&golden);
        for format in FORMATS {
            let black_and_white = bilevel(&golden, format);
            // One pixel a little short of white, or of opaque: no longer
            // black and white.
            let mut spoiled = black_and_white.clone();
            let components = format.components();
            let last = spoiled.data.len() - components;
            if format.has_alpha() {
                spoiled.data[last + components - 1] = 254;
            } else {
                spoiled.data[last] = 128;
            }
            let mut flat = variant(&golden, format, false);
            let first: Vec<u8> = flat.data[..components].to_vec();
            for pixel in flat.data.chunks_exact_mut(components) {
                pixel.copy_from_slice(&first);
            }
            bitmaps.extend([black_and_white, spoiled, flat]);
        }
        for bitmap in &bitmaps {
            let what = format!(
                "{name}, {:?}{}",
                bitmap.format,
                if bitmap.premultiplied {
                    " premultiplied"
                } else {
                    ""
                }
            );
            let colours = distinct(bitmap);
            for limit in [0, 1, 2, 17, 1_000, usize::MAX] {
                assert_eq!(
                    bitmap.colour_count(limit),
                    Some(colours.len().min(limit)),
                    "{what}: the colour count to {limit}"
                );
            }
            assert_eq!(
                bitmap.is_unicolour(),
                Some(colours.len() <= 1),
                "{what}: unicolour"
            );
            assert_eq!(
                bitmap.is_monochrome(),
                Some(monochrome(bitmap)),
                "{what}: monochrome"
            );
            held += 1;
        }
    }
    println!("bitmap-toolbox reductions: RAN, {held} bitmaps held to their closed forms");
}

/// Every operation's answer on the bitmaps the counts and tests must say yes
/// to, so neither branch of either test goes unexercised. Lab's black and
/// white are held by the closed form above rather than here, since whether
/// its encoding of white reads back as exactly white is `tinker-pdf-color`'s
/// precision and not the toolbox's.
#[test]
fn the_tests_say_yes_where_they_should() {
    let (_, golden) = goldens().remove(0);
    for format in FORMATS {
        if format != PixelFormat::LabA8 {
            assert_eq!(
                bilevel(&golden, format).is_monochrome(),
                Some(true),
                "{format:?}"
            );
        }
        let mut flat = variant(&golden, format, false);
        let components = format.components();
        let first: Vec<u8> = flat.data[..components].to_vec();
        for pixel in flat.data.chunks_exact_mut(components) {
            pixel.copy_from_slice(&first);
        }
        assert_eq!(flat.is_unicolour(), Some(true), "{format:?}");
        assert_eq!(flat.colour_count(usize::MAX), Some(1), "{format:?}");
    }
}

/// `bitmap` with every row padded by `padding` bytes of a pattern no pixel
/// holds in that place.
fn padded(bitmap: &Bitmap, padding: usize) -> Bitmap {
    let row = bitmap.width as usize * bitmap.components();
    let mut data = Vec::new();
    for y in 0..bitmap.height as usize {
        data.extend_from_slice(&bitmap.data[y * bitmap.stride..y * bitmap.stride + row]);
        data.extend((0..padding).map(|k| (0xA5 ^ (y * 31 + k * 7)) as u8));
    }
    Bitmap {
        stride: row + padding,
        data,
        ..bitmap.clone()
    }
}

/// The pixels of a padded bitmap, its padding left out.
fn unpadded(bitmap: &Bitmap) -> Vec<u8> {
    let row = bitmap.width as usize * bitmap.components();
    (0..bitmap.height as usize)
        .flat_map(|y| bitmap.data[y * bitmap.stride..y * bitmap.stride + row].to_vec())
        .collect()
}

/// **A padded stride is neither read nor written**: every map over a bitmap
/// whose rows are padded writes the bytes it writes unpadded and leaves the
/// padding as it was, and every reduction answers what it answers unpadded,
/// though the padding holds values that would change each answer if read.
#[test]
fn a_padded_stride_is_neither_read_nor_written() {
    let maps = maps();
    let (_, golden) = goldens().remove(0);
    let mut cases = variants(&golden);
    for format in FORMATS {
        cases.push(bilevel(&golden, format));
    }
    for bitmap in cases {
        let wide = padded(&bitmap, 7);
        for (map, apply, _) in &maps {
            let mut plain = bitmap.clone();
            apply(&mut plain);
            let mut got = wide.clone();
            apply(&mut got);
            assert_eq!(unpadded(&got), plain.data, "{map}, {:?}", bitmap.format);
            let row = bitmap.width as usize * bitmap.components();
            for y in 0..bitmap.height as usize {
                let padding = y * wide.stride + row..(y + 1) * wide.stride;
                assert_eq!(
                    &got.data[padding.clone()],
                    &wide.data[padding],
                    "{map}, {:?}: row {y}'s padding was written",
                    bitmap.format
                );
            }
        }
        assert_eq!(
            wide.colour_count(usize::MAX),
            bitmap.colour_count(usize::MAX)
        );
        assert_eq!(wide.is_unicolour(), bitmap.is_unicolour());
        assert_eq!(wide.is_monochrome(), bitmap.is_monochrome());
    }
}

/// A bitmap that is not a picture — no width, no height, a stride narrower
/// than a row, a buffer short of its last row — is refused whole: no map
/// writes a byte of it and every reduction answers `None`.
#[test]
fn a_bitmap_that_is_not_a_picture_is_refused_whole() {
    let maps = maps();
    let (_, golden) = goldens().remove(0);
    let base = variant(&golden, PixelFormat::Rgba8, false);
    let mut short = base.clone();
    short.data.pop();
    let cases = [
        Bitmap {
            width: 0,
            ..base.clone()
        },
        Bitmap {
            height: 0,
            ..base.clone()
        },
        Bitmap {
            stride: base.stride - 1,
            ..base.clone()
        },
        short,
    ];
    for bitmap in cases {
        for (map, apply, _) in &maps {
            let mut got = bitmap.clone();
            apply(&mut got);
            assert_eq!(
                got.data, bitmap.data,
                "{map} wrote a bitmap that is no picture"
            );
        }
        assert_eq!(bitmap.colour_count(10), None);
        assert_eq!(bitmap.is_unicolour(), None);
        assert_eq!(bitmap.is_monochrome(), None);
    }
}

/// Black goes to the tint's dark end and white to its light end, in every
/// format that can say so exactly; and a premultiplied pixel of no alpha
/// stays nothing under every map.
#[test]
fn the_ends_of_every_map_are_where_the_design_puts_them() {
    let pixel = |format: PixelFormat, bytes: &[u8], premultiplied: bool| Bitmap {
        width: 1,
        height: 1,
        format,
        stride: bytes.len(),
        data: bytes.to_vec(),
        warnings: Vec::new(),
        premultiplied,
        stats: None,
    };
    let mut black = pixel(PixelFormat::Rgb8, &[0, 0, 0], false);
    black.tint(DARK, LIGHT);
    assert_eq!(black.data, [DARK.r, DARK.g, DARK.b]);
    let mut white = pixel(PixelFormat::Rgb8, &[255, 255, 255], false);
    white.tint(DARK, LIGHT);
    assert_eq!(white.data, [LIGHT.r, LIGHT.g, LIGHT.b]);
    let mut white = pixel(PixelFormat::Rgb8, &[255, 255, 255], false);
    white.invert_luminance();
    assert_eq!(white.data, [0, 0, 0], "white turned over is black");
    let mut rich = pixel(PixelFormat::CmykA8, &[255, 255, 255, 255, 255], false);
    rich.grayscale();
    assert_eq!(
        rich.data,
        [0, 0, 0, 255, 255],
        "a rich black is black ink alone"
    );
    for format in [
        PixelFormat::GrayA8,
        PixelFormat::Rgba8,
        PixelFormat::CmykA8,
        PixelFormat::LabA8,
        PixelFormat::Bgra8,
    ] {
        let nothing = vec![0u8; format.components()];
        for (map, apply, _) in &maps() {
            let mut got = pixel(format, &nothing, true);
            apply(&mut got);
            assert_eq!(
                got.data, nothing,
                "{map}, {format:?}: a transparent pixel stays nothing"
            );
        }
    }
}

// ---- scale and subsample ----------------------------------------------------------

/// The overlaps of line `d` of `count` lines over an axis of `extent`
/// samples the lines cover exactly, in `1 / count` of a sample: the line
/// covers `[d * extent, (d + 1) * extent)` and sample `i` covers
/// `[i * count, (i + 1) * count)`. The definition, with nothing quantised.
fn overlaps(d: u32, count: u32, extent: u32) -> Vec<(usize, u128)> {
    let (lo, hi) = (
        u128::from(d) * u128::from(extent),
        u128::from(d + 1) * u128::from(extent),
    );
    (0..extent)
        .filter_map(|i| {
            let start = u128::from(i) * u128::from(count);
            let covered = hi
                .min(start + u128::from(count))
                .saturating_sub(lo.max(start));
            (covered > 0).then_some((i as usize, covered))
        })
        .collect()
}

/// What a scale should make of a packed `bitmap`: every byte the area average
/// of the rectangle each pixel covers, every sample under it weighted by the
/// product of its two overlaps and the sum rounded half up — the colour
/// premultiplied for the average where the bitmap is straight and
/// straightened after, the alpha averaged as a channel.
fn scaled_expected(bitmap: &Bitmap, width: u32, height: u32) -> Vec<u8> {
    let (sw, sh) = (bitmap.width, bitmap.height);
    let format = bitmap.format;
    let components = format.components();
    let colours = colours(format);
    let alpha = format.has_alpha();
    let straight = !bitmap.premultiplied;
    let columns: Vec<_> = (0..width).map(|x| overlaps(x, width, sw)).collect();
    let rows: Vec<_> = (0..height).map(|y| overlaps(y, height, sh)).collect();
    let mut out = Vec::with_capacity(width as usize * height as usize * components);
    for down in &rows {
        for across in &columns {
            let mut sums = [0u128; 5];
            let mut weight = 0u128;
            for &(ty, oy) in down {
                for &(tx, ox) in across {
                    let at = (ty * sw as usize + tx) * components;
                    let px = &bitmap.data[at..at + components];
                    let a = if alpha { px[colours] } else { 255 };
                    let w = ox * oy;
                    weight += w;
                    for c in 0..colours {
                        let v = if straight {
                            premultiply(px[c], a)
                        } else {
                            px[c]
                        };
                        sums[c] += w * u128::from(v);
                    }
                    if alpha {
                        sums[colours] += w * u128::from(a);
                    }
                }
            }
            let mut pixel = [0u8; 5];
            for (slot, sum) in pixel.iter_mut().zip(sums).take(components) {
                *slot = ((2 * sum + weight) / (2 * weight)) as u8;
            }
            if straight && alpha {
                let a = pixel[colours];
                for c in &mut pixel[..colours] {
                    *c = straighten(*c, a);
                }
            }
            out.extend_from_slice(&pixel[..components]);
        }
    }
    out
}

/// The sizes a golden is scaled to: whole and fractional ratios down and up,
/// the two axes going opposite ways, a hair either side of its own size, and
/// one pixel, which is the whole picture's average.
fn targets(width: u32, height: u32) -> Vec<(&'static str, u32, u32)> {
    vec![
        ("a half", width / 2, height / 2),
        ("a third", width / 3, height / 3),
        ("two thirds", width * 2 / 3, height * 2 / 3),
        ("five sevenths", width * 5 / 7, height * 5 / 7),
        ("double", width * 2, height * 2),
        ("one and a half", width * 3 / 2, height * 3 / 2),
        ("half across, double down", width / 2, height * 2),
        ("one less and one more", width - 1, height + 1),
        ("one pixel", 1, 1),
    ]
}

/// What a bitmap a toolbox operation made should carry besides its bytes:
/// its format, its size, a packed stride, the source's premultiplied flag
/// and warnings, and no stats.
fn assert_carried(got: &Bitmap, source: &Bitmap, width: u32, height: u32, what: &str) {
    assert_eq!(
        (
            got.width,
            got.height,
            got.format,
            got.stride,
            got.premultiplied
        ),
        (
            width,
            height,
            source.format,
            width as usize * source.components(),
            source.premultiplied
        ),
        "{what}: the result's fields"
    );
    assert_eq!(got.warnings, source.warnings, "{what}: the warnings travel");
    assert!(
        got.stats.is_none(),
        "{what}: a scaled bitmap counted nothing"
    );
}

/// The bitmap as its variants are, with a warning on it to see it carried.
fn warned(mut bitmap: Bitmap) -> Bitmap {
    bitmap.warnings = vec![RenderWarning::Cancelled];
    bitmap
}

/// **A scaled render is the area average the test computes**: every golden,
/// in every format, straight and premultiplied, at each of [`targets`] —
/// whole and fractional ratios, down and up, and the two axes apart — every
/// byte the average of the rectangle its pixel covers, computed here from the
/// definition.
#[test]
fn a_scaled_render_is_the_area_average_the_test_computes() {
    let mut held = 0usize;
    for (name, golden) in goldens() {
        for bitmap in variants(&golden).into_iter().map(warned) {
            for (ratio, width, height) in targets(bitmap.width, bitmap.height) {
                let what = format!(
                    "{name}, {:?}{}, {ratio} ({width}x{height})",
                    bitmap.format,
                    if bitmap.premultiplied {
                        " premultiplied"
                    } else {
                        ""
                    }
                );
                let got = bitmap
                    .scaled(width, height)
                    .unwrap_or_else(|| panic!("{what}: a picture scales"));
                assert_carried(&got, &bitmap, width, height, &what);
                let want = scaled_expected(&bitmap, width, height);
                if got.data != want {
                    let at = got
                        .data
                        .iter()
                        .zip(&want)
                        .position(|(a, b)| a != b)
                        .unwrap_or(0);
                    panic!(
                        "{what}: byte {at} is {} where the average is {}",
                        got.data[at], want[at]
                    );
                }
                held += 1;
            }
        }
    }
    println!("bitmap-toolbox scale: RAN, {held} scales held to the area average");
}

/// **A scale to the same size returns the same bytes**: every variant, and a
/// padded one, comes back as it went in, stride and padding included.
#[test]
fn a_scale_to_the_same_size_returns_the_same_bytes() {
    let (_, golden) = goldens().remove(0);
    for bitmap in variants(&golden) {
        for source in [bitmap.clone(), padded(&bitmap, 5)] {
            let same = source
                .scaled(source.width, source.height)
                .expect("a picture");
            assert_eq!(
                (same.width, same.height, same.stride, same.premultiplied),
                (
                    source.width,
                    source.height,
                    source.stride,
                    source.premultiplied
                ),
            );
            assert_eq!(same.data, source.data, "{:?}", source.format);
        }
    }
}

/// The samples a subsample should keep: the first of each `x` by `y` block.
fn subsampled_expected(bitmap: &Bitmap, x: u32, y: u32) -> Vec<u8> {
    let components = bitmap.components();
    let mut out = Vec::new();
    for row in (0..bitmap.height).step_by(y as usize) {
        for column in (0..bitmap.width).step_by(x as usize) {
            let at = row as usize * bitmap.stride + column as usize * components;
            out.extend_from_slice(&bitmap.data[at..at + components]);
        }
    }
    out
}

/// **A subsampled render is the samples the test takes**: every golden, in
/// every format, straight and premultiplied, by whole factors that divide it
/// and that do not, unequal on the two axes and past its extent, every byte
/// the first sample of its block as stored; a factor of one on both axes is
/// the bitmap.
#[test]
fn a_subsampled_render_is_the_samples_the_test_takes() {
    let mut held = 0usize;
    for (name, golden) in goldens() {
        for bitmap in variants(&golden).into_iter().map(warned) {
            let factors = [
                (2, 2),
                (3, 2),
                (2, 5),
                (7, 3),
                (bitmap.width, bitmap.height),
                (bitmap.width + 3, 1),
                (1, bitmap.height + 1),
            ];
            for (x, y) in factors {
                let what = format!("{name}, {:?}, by {x}x{y}", bitmap.format);
                let got = bitmap
                    .subsampled(x, y)
                    .unwrap_or_else(|| panic!("{what}: a picture subsamples"));
                let (width, height) = (bitmap.width.div_ceil(x), bitmap.height.div_ceil(y));
                assert_carried(&got, &bitmap, width, height, &what);
                assert_eq!(got.data, subsampled_expected(&bitmap, x, y), "{what}");
                held += 1;
            }
            let same = bitmap.subsampled(1, 1).expect("a picture");
            assert_eq!(same.data, bitmap.data, "{name}: by one is the bitmap");
            assert_eq!(same.stride, bitmap.stride);
        }
    }
    println!("bitmap-toolbox subsample: RAN, {held} subsamples held to their samples");
}

/// A padded stride is not read: a padded bitmap scales and subsamples to the
/// bytes its packed twin does.
#[test]
fn a_padded_stride_scales_and_subsamples_as_its_packed_twin() {
    let (_, golden) = goldens().remove(0);
    for bitmap in variants(&golden) {
        let wide = padded(&bitmap, 7);
        for (_, width, height) in targets(bitmap.width, bitmap.height) {
            assert_eq!(
                wide.scaled(width, height).map(|b| b.data),
                bitmap.scaled(width, height).map(|b| b.data),
                "{:?} to {width}x{height}",
                bitmap.format
            );
        }
        for (x, y) in [(2, 2), (3, 5)] {
            assert_eq!(
                wide.subsampled(x, y).map(|b| b.data),
                bitmap.subsampled(x, y).map(|b| b.data),
                "{:?} by {x}x{y}",
                bitmap.format
            );
        }
    }
}

/// Scale and subsample refuse a bitmap that is not a picture, a size or a
/// factor of zero, and a result too large to hold, rather than handing back
/// part of one.
#[test]
fn scale_and_subsample_refuse_what_is_no_picture() {
    let (_, golden) = goldens().remove(0);
    let base = variant(&golden, PixelFormat::Rgba8, false);
    let mut short = base.clone();
    short.data.pop();
    for bitmap in [
        Bitmap {
            width: 0,
            ..base.clone()
        },
        Bitmap {
            height: 0,
            ..base.clone()
        },
        Bitmap {
            stride: base.stride - 1,
            ..base.clone()
        },
        short,
    ] {
        assert!(bitmap.scaled(10, 10).is_none());
        assert!(bitmap.subsampled(2, 2).is_none());
    }
    assert!(base.scaled(0, 10).is_none());
    assert!(base.scaled(10, 0).is_none());
    assert!(base.subsampled(0, 1).is_none());
    assert!(base.subsampled(1, 0).is_none());
    // More bytes than an address space holds: refused, not attempted.
    assert!(base.scaled(u32::MAX, u32::MAX).is_none());
}
