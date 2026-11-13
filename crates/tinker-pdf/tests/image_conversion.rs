//! The converter an image's samples become RGB by (`docs/design/icc.md`,
//! P-22, "The per-image converter"), chosen once an image by its space, its
//! depth and its `/Decode`, counted through MS-01's tally
//! (`Stats::image_colours_converted`): what it asks of the colour space's
//! conversion, `ColorSpace::to_rgb`, does not grow with the image's pixels,
//! what it allocates does not grow with its colours, and each device arm
//! draws what `to_rgb` gives each of its inputs.
//!
//! Every page here is the image's own size in points and draws it over the
//! whole of itself at 72 dpi, one sample a pixel, so a pixel of the render is
//! the colour of the sample under it, byte for byte. Each count expected is
//! derived beside it from what the image holds.
//!
//! No tally sees an allocation, so this binary installs MS-01's A10 counter,
//! the guard suite's counting allocator (`guard_support::Counting`), which
//! counts each allocation against the thread that made it; each render here
//! runs on its test's thread, as `RenderOptions::default()` asks.

mod guard_support;
mod icc_support;
mod render_support;

use guard_support::{allocations_of, object, pdf, stream, Counting};
use tinker_pdf::{Bitmap, Document, RenderOptions, Stats};
use tinker_pdf_color::ColorSpace;

#[global_allocator]
static COUNTING: Counting = Counting;

/// A page `width` by `height` points drawing the image object 5 over the
/// whole of itself, `extra` numbered from 6.
fn page(width: u32, height: u32, image: Vec<u8>, extra: Vec<Vec<u8>>) -> Vec<u8> {
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] \
             /Resources << /XObject << /Im 5 0 R >> >> /Contents 4 0 R >>"
        )),
        stream(
            "",
            format!("q {width} 0 0 {height} 0 0 cm /Im Do Q").as_bytes(),
        ),
        image,
    ];
    objects.extend(extra);
    pdf(&objects)
}

/// An image XObject of `width` by `height` samples, `keys` beside its size,
/// holding `samples` uncompressed.
fn image(width: u32, height: u32, keys: &str, samples: &[u8]) -> Vec<u8> {
    stream(
        &format!("/Type /XObject /Subtype /Image /Width {width} /Height {height} {keys}"),
        samples,
    )
}

/// `bytes`' page rendered at 72 dpi, counting.
fn render(bytes: Vec<u8>) -> (Bitmap, Stats) {
    let bitmap = Document::open(bytes)
        .expect("the page opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions {
            stats: true,
            ..RenderOptions::default()
        });
    let stats = bitmap.stats.expect("a counted render hands its stats back");
    (bitmap, stats)
}

/// The colours `bytes`' image asked of its space's conversion.
fn conversions(bytes: Vec<u8>) -> u64 {
    render(bytes).1.image_colours_converted
}

/// The colour of `bitmap`'s pixel at `(x, y)`.
fn pixel(bitmap: &Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = y as usize * bitmap.stride + x as usize * bitmap.components();
    (bitmap.data[at], bitmap.data[at + 1], bitmap.data[at + 2])
}

/// Samples of `bits` bits, packed most significant first, each row starting
/// on a byte (8.9.3): `width` of them a row, `value(x, y)` each.
fn packed(width: u32, height: u32, bits: u32, value: impl Fn(u32, u32) -> u32) -> Vec<u8> {
    let row_bytes = (width * bits).div_ceil(8) as usize;
    let mut out = vec![0u8; row_bytes * height as usize];
    for y in 0..height {
        for x in 0..width {
            let at = y as usize * row_bytes * 8 + (x * bits) as usize;
            let v = value(x, y) & ((1 << bits) - 1);
            for bit in 0..bits {
                if (v >> (bits - 1 - bit)) & 1 == 1 {
                    let index = at + bit as usize;
                    out[index / 8] |= 0x80 >> (index % 8);
                }
            }
        }
    }
    out
}

/// 8-bit samples, `n` a pixel, `value(x, y, c)` each.
fn bytes_of(width: u32, height: u32, n: u32, value: impl Fn(u32, u32, u32) -> u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((width * height * n) as usize);
    for y in 0..height {
        for x in 0..width {
            for c in 0..n {
                out.push((value(x, y, c) & 0xFF) as u8);
            }
        }
    }
    out
}

/// A 256-entry palette over DeviceRGB, each entry a colour of its own, as a
/// hexadecimal string.
fn palette() -> String {
    (0..256u32)
        .map(|i| format!("{:02X}{:02X}{:02X}", i, (i * 37) % 256, 255 - i))
        .collect()
}

/// A `/Separation` space's tint transform, white to a blue, object 6.
fn separation_tint() -> Vec<u8> {
    object("<< /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [0.2 0.4 0.9] /N 1 >>")
}

/// A two-colorant `/DeviceN` space's tint transform, a type 4 program taking
/// `a b` to `a b (a + b) / 2`, object 6.
fn device_n_tint() -> Vec<u8> {
    stream(
        "/FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1]",
        b"{ 1 index 1 index add 2 div }",
    )
}

/// The DeviceCMYK samples of every pair of an ink byte and a black byte, a
/// row a black byte: C the column, M its complement and Y seven times it,
/// which seven, being odd, takes through every byte, and K the row.
fn every_cmyk_pair(x: u32, y: u32, c: u32) -> u32 {
    match c {
        0 => x,
        1 => 255 - x,
        2 => (x * 7) % 256,
        _ => y,
    }
}

/// **A converter is chosen once an image**, so what an image asks of its
/// space's conversion does not grow with its pixels: an 8-bit DeviceRGB image
/// is its own colour and an 8-bit ICC one converts by its transform's row
/// path, so neither asks one colour; an 8-bit DeviceCMYK image asks at most
/// one colour for each of the 65 536 pairs of an ink byte and a black byte —
/// all of them here, since the 256 by 256 image holds every pair and each
/// pixel one, and no more for the image twice as tall that holds each twice,
/// and a 4 by 4 one of sixteen pairs no more than its pixels; an image of one
/// component asks at most `2^bpc`, one for each value it holds — 16 for a
/// 4-bit grey under `/Decode [1 0]`, which no copy stands for, 256 for an
/// 8-bit `/Separation` and for an 8-bit `/Indexed` image, 2 for a one-bit
/// grey; and an image no table stands for asks once a colour its remembered
/// colours do not hold, once for a uniform `/DeviceN` image however large.
/// Each figure is the same at both sizes, the second four times the pixels.
#[test]
fn a_converter_is_chosen_once_per_image() {
    let profile = || stream("/N 3", &icc_support::rgb_profile());
    let grey_profile = || stream("/N 1", &icc_support::grey_profile());
    for side in [16u32, 32, 64] {
        let rgb = image(
            side,
            side,
            "/ColorSpace /DeviceRGB /BitsPerComponent 8",
            &bytes_of(side, side, 3, |x, y, c| x * 7 + y * 3 + c * 50),
        );
        assert_eq!(
            conversions(page(side, side, rgb, vec![])),
            0,
            "DeviceRGB at {side}"
        );

        let icc = image(
            side,
            side,
            "/ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 8",
            &bytes_of(side, side, 3, |x, y, c| x * 5 + y * 9 + c * 70),
        );
        assert_eq!(
            conversions(page(side, side, icc, vec![profile()])),
            0,
            "an ICC RGB image at {side}"
        );
        let icc_grey = image(
            side,
            side,
            "/ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 8",
            &bytes_of(side, side, 1, |x, y, _| x * 16 + y),
        );
        assert_eq!(
            conversions(page(side, side, icc_grey, vec![grey_profile()])),
            0,
            "an ICC grey image at {side}"
        );
    }

    // C, M and Y alike, so a pixel's three inks ask the one entry of its ink
    // byte against its black byte, a row holding each ink byte once: every
    // pixel of the first 256 rows asks a colour, and no pixel after them.
    for height in [256u32, 512] {
        let cmyk = image(
            256,
            height,
            "/ColorSpace /DeviceCMYK /BitsPerComponent 8",
            &bytes_of(256, height, 4, |x, y, c| if c == 3 { y % 256 } else { x }),
        );
        assert_eq!(
            conversions(page(256, height, cmyk, vec![])),
            65_536,
            "DeviceCMYK, every pair, {height} rows"
        );
    }
    let few = image(
        4,
        4,
        "/ColorSpace /DeviceCMYK /BitsPerComponent 8",
        &bytes_of(4, 4, 4, |x, y, c| (x * 4 + y) * 9 + c),
    );
    assert_eq!(
        conversions(page(4, 4, few, vec![])),
        16,
        "a small DeviceCMYK image asks no more than its pixels"
    );

    for side in [64u32, 128] {
        let grey = image(
            side,
            side,
            "/ColorSpace /DeviceGray /BitsPerComponent 4 /Decode [1 0]",
            &packed(side, side, 4, |x, y| x + y),
        );
        assert_eq!(
            conversions(page(side, side, grey, vec![])),
            16,
            "4-bit grey at {side}"
        );
    }
    for side in [16u32, 32] {
        let spot = image(
            side,
            side,
            "/ColorSpace [/Separation /Spot /DeviceRGB 6 0 R] /BitsPerComponent 8",
            &bytes_of(side, side, 1, |x, y, _| (y * side + x) % 256),
        );
        assert_eq!(
            conversions(page(side, side, spot, vec![separation_tint()])),
            256,
            "a /Separation at {side}"
        );
        let indexed = image(
            side,
            side,
            &format!(
                "/ColorSpace [/Indexed /DeviceRGB 255 <{}>] /BitsPerComponent 8",
                palette()
            ),
            &bytes_of(side, side, 1, |x, y, _| (y * side + x) % 256),
        );
        assert_eq!(
            conversions(page(side, side, indexed, vec![])),
            256,
            "an /Indexed image at {side}"
        );
        let bilevel = image(
            side,
            side,
            "/ColorSpace /DeviceGray /BitsPerComponent 1",
            &packed(side, side, 1, |x, y| x ^ y),
        );
        assert_eq!(
            conversions(page(side, side, bilevel, vec![])),
            2,
            "one bit at {side}"
        );
        let uniform = image(
            side,
            side,
            "/ColorSpace [/DeviceN [/A /B] /DeviceRGB 6 0 R] /BitsPerComponent 8",
            &bytes_of(side, side, 2, |_, _, c| 60 + c * 100),
        );
        assert_eq!(
            conversions(page(side, side, uniform, vec![device_n_tint()])),
            1,
            "a uniform /DeviceN image at {side}"
        );
    }
}

/// **Each device arm draws what `to_rgb` gives each of its inputs.** An
/// 8-bit DeviceGray image of every value and a DeviceRGB one of every value
/// in each channel, both copied; a 4-bit grey under `/Decode [1 0]` and an
/// 8-bit grey under `[0.2 0.9]`, both read through their tables; and an 8-bit
/// DeviceCMYK image of every pair of an ink byte and a black byte in each of
/// C, M and Y, read through its table — each pixel the colour
/// `ColorSpace::to_rgb` gives its sample's components, worked out here a
/// pixel at a time.
#[test]
fn each_device_arm_draws_what_to_rgb_gives_each_input() {
    let check = |what: &str,
                 width: u32,
                 height: u32,
                 bytes: Vec<u8>,
                 want: &dyn Fn(u32, u32) -> (u8, u8, u8)| {
        let (bitmap, stats) = render(bytes);
        assert!(bitmap.warnings.is_empty(), "{what}: {:?}", bitmap.warnings);
        assert_eq!((bitmap.width, bitmap.height), (width, height), "{what}");
        assert!(stats.image_colours_converted <= u64::from(width * height));
        for y in 0..height {
            for x in 0..width {
                assert_eq!(pixel(&bitmap, x, y), want(x, y), "{what}: ({x}, {y})");
            }
        }
    };
    let gray = ColorSpace::DeviceGray;
    let rgb = ColorSpace::DeviceRgb;
    let cmyk = ColorSpace::DeviceCmyk;
    let unit = |v: u32| f64::from(v) / 255.0;

    check(
        "8-bit grey",
        16,
        16,
        page(
            16,
            16,
            image(
                16,
                16,
                "/ColorSpace /DeviceGray /BitsPerComponent 8",
                &bytes_of(16, 16, 1, |x, y, _| y * 16 + x),
            ),
            vec![],
        ),
        &|x, y| gray.to_rgb(&[unit(y * 16 + x)]),
    );
    let channel = |x: u32, y: u32, c: u32| (y * 16 + x) * (2 * c + 1) % 256;
    check(
        "8-bit RGB",
        16,
        16,
        page(
            16,
            16,
            image(
                16,
                16,
                "/ColorSpace /DeviceRGB /BitsPerComponent 8",
                &bytes_of(16, 16, 3, channel),
            ),
            vec![],
        ),
        &|x, y| {
            rgb.to_rgb(&[
                unit(channel(x, y, 0)),
                unit(channel(x, y, 1)),
                unit(channel(x, y, 2)),
            ])
        },
    );
    check(
        "4-bit grey under [1 0]",
        16,
        16,
        page(
            16,
            16,
            image(
                16,
                16,
                "/ColorSpace /DeviceGray /BitsPerComponent 4 /Decode [1 0]",
                &packed(16, 16, 4, |x, y| x + y),
            ),
            vec![],
        ),
        &|x, y| gray.to_rgb(&[1.0 - f64::from((x + y) % 16) / 15.0]),
    );
    check(
        "8-bit grey under [0.2 0.9]",
        16,
        16,
        page(
            16,
            16,
            image(
                16,
                16,
                "/ColorSpace /DeviceGray /BitsPerComponent 8 /Decode [0.2 0.9]",
                &bytes_of(16, 16, 1, |x, y, _| y * 16 + x),
            ),
            vec![],
        ),
        &|x, y| gray.to_rgb(&[0.2 + unit(y * 16 + x) * (0.9 - 0.2)]),
    );
    check(
        "8-bit CMYK, every pair",
        256,
        256,
        page(
            256,
            256,
            image(
                256,
                256,
                "/ColorSpace /DeviceCMYK /BitsPerComponent 8",
                &bytes_of(256, 256, 4, every_cmyk_pair),
            ),
            vec![],
        ),
        &|x, y| cmyk.to_rgb(&[0, 1, 2, 3].map(|c| unit(every_cmyk_pair(x, y, c)))),
    );
}

/// **An image's converter allocates nothing more for more colours** (A10):
/// each image rendered uniform and with every pixel a colour of its own, at
/// one size, allocates as many times — a `/DeviceN` image, whose colours
/// convert through its tint transform into a buffer the image keeps and are
/// remembered in a table of fixed size; a `/Separation` image and a
/// DeviceCMYK one, read through tables of fixed size; and an ICC image, read
/// through its transform's row path. Each page is rendered once first, so
/// what the first render of a process builds is not counted against the
/// uniform image.
#[test]
fn an_image_converts_in_allocations_flat_in_its_colours() {
    let side = 64u32;
    let cases: [(&str, u32, String, Vec<Vec<u8>>); 4] = [
        (
            "a /DeviceN image",
            2,
            "/ColorSpace [/DeviceN [/A /B] /DeviceRGB 6 0 R] /BitsPerComponent 8".to_string(),
            vec![device_n_tint()],
        ),
        (
            "a /Separation image",
            1,
            "/ColorSpace [/Separation /Spot /DeviceRGB 6 0 R] /BitsPerComponent 8".to_string(),
            vec![separation_tint()],
        ),
        (
            "a DeviceCMYK image",
            4,
            "/ColorSpace /DeviceCMYK /BitsPerComponent 8".to_string(),
            vec![],
        ),
        (
            "an ICC image",
            3,
            "/ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 8".to_string(),
            vec![stream("/N 3", &icc_support::rgb_profile())],
        ),
    ];
    for (what, n, keys, extra) in cases {
        let uniform = page(
            side,
            side,
            image(
                side,
                side,
                &keys,
                &bytes_of(side, side, n, |_, _, c| 40 + c * 50),
            ),
            extra.clone(),
        );
        let distinct = page(
            side,
            side,
            image(
                side,
                side,
                &keys,
                &bytes_of(side, side, n, |x, y, c| (x * 4 + c * 61) ^ (y * 4)),
            ),
            extra.clone(),
        );
        let allocations = |bytes: &Vec<u8>| {
            let document = Document::open(bytes.clone()).expect("the page opens");
            let page = document.page(0).expect("a page");
            drop(page.render(&RenderOptions::default()));
            allocations_of(|| drop(page.render(&RenderOptions::default())))
        };
        let (few, many) = (allocations(&uniform), allocations(&distinct));
        assert!(
            few > 0,
            "{what}: a render that allocates nothing measured nothing"
        );
        assert_eq!(
            few, many,
            "{what}: {few} allocations uniform and {many} with a colour a pixel"
        );
    }
}

/// **An inline image is converted by its converter too**: a 4-bit grey
/// drawn inline under `/Decode [1 0]` asks its sixteen values' colours at two
/// sizes, and draws what `to_rgb` gives each.
#[test]
fn an_inline_image_is_converted_by_its_converter() {
    for side in [16u32, 32] {
        let hex: String = packed(side, side, 4, |x, y| x + y)
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect();
        let content = format!(
            "q {side} 0 0 {side} 0 0 cm BI /W {side} /H {side} /CS /G /BPC 4 /D [1 0] \
             /F /AHx ID {hex}> EI Q"
        );
        let bytes = pdf(&[
            object("<< /Type /Catalog /Pages 2 0 R >>"),
            object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
            object(&format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {side} {side}] \
                 /Resources << >> /Contents 4 0 R >>"
            )),
            stream("", content.as_bytes()),
        ]);
        let (bitmap, stats) = render(bytes);
        assert!(bitmap.warnings.is_empty(), "{:?}", bitmap.warnings);
        assert_eq!(stats.image_colours_converted, 16, "at {side}");
        for y in 0..side {
            for x in 0..side {
                let want = ColorSpace::DeviceGray.to_rgb(&[1.0 - f64::from((x + y) % 16) / 15.0]);
                assert_eq!(pixel(&bitmap, x, y), want, "({x}, {y}) at {side}");
            }
        }
    }
}
