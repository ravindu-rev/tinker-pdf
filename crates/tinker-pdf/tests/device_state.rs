//! The device-dependent graphics state, applied
//! (`docs/design/device-dependent-state.md`): each key a `gs` stores, drawn
//! as its clause says, every effect computed here from the clause rather
//! than read back from the engine.
//!
//! Every fixture draws pixel-aligned shapes at 72 dpi, so each object covers
//! whole pixels and none partly: the clause's per-point rules (11.7.5.2)
//! then have one answer at every pixel, which these tests compute.

use tinker_pdf::{Bitmap, Document, PixelFormat, PixelRegion, RenderOptions};

mod render_support;

/// A one-page document `width` by `height` points, its resources `resources`
/// (object 4) and its content `content` (object 5), with `extra` as objects 6
/// onward for the resources to name.
fn page(width: u32, height: u32, resources: &str, content: &str, extra: &[String]) -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources 4 0 R \
             /Contents 5 0 R >>"
        ),
        resources.to_string(),
        stream("", content),
    ];
    objects.extend(extra.iter().cloned());
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A stream object of `dict`'s entries and `body`.
fn stream(dict: &str, body: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{body}\nendstream",
        body.len()
    )
}

/// A type 4 (PostScript calculator) function of one input over `[0 1]`.
fn calculator(program: &str) -> String {
    stream("/FunctionType 4 /Domain [0 1] /Range [0 1]", program)
}

fn render(bytes: Vec<u8>, options: &RenderOptions) -> Bitmap {
    Document::open(bytes)
        .expect("the document opens")
        .page(0)
        .expect("a page")
        .render(options)
}

fn options(format: PixelFormat) -> RenderOptions {
    RenderOptions {
        format,
        allow_cmyk: format == PixelFormat::CmykA8,
        ..RenderOptions::default()
    }
}

/// A curve of one input as the engine samples a transfer function: 256
/// entries, each the function at `i / 255` clamped to `[0 1]`, rounded.
fn table(curve: fn(f64) -> f64) -> [u8; 256] {
    core::array::from_fn(|i| {
        let x = f64::from(i as u8) / 255.0;
        let v = curve(x);
        let v = if v.is_finite() { v } else { x };
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    })
}

// The four curves, each a calculator whose arithmetic is IEEE's own, so the
// test computes exactly what the function returns.
const INVERT: &str = "{ 1 exch sub }";
fn invert(x: f64) -> f64 {
    1.0 - x
}
const SQUARE: &str = "{ dup mul }";
fn square(x: f64) -> f64 {
    x * x
}
const HALF: &str = "{ 0.5 mul 0.25 add }";
fn half(x: f64) -> f64 {
    x * 0.5 + 0.25
}
const DOUBLE: &str = "{ 2 mul 1 min }";
fn double(x: f64) -> f64 {
    (x * 2.0).min(1.0)
}

/// Objects 6 to 9: the four curves.
fn curves() -> Vec<String> {
    [INVERT, SQUARE, HALF, DOUBLE].map(calculator).to_vec()
}

/// The pixel rectangle `[x0, x1) × [y0, y1)` a PDF rectangle of `x`, `y`,
/// `w`, `h` points covers on a page `height` points tall at 72 dpi.
fn pixels(x: u32, y: u32, w: u32, h: u32, height: u32) -> (u32, u32, u32, u32) {
    (x, height - y - h, x + w, height - y)
}

fn inside(rect: (u32, u32, u32, u32), x: u32, y: u32) -> bool {
    x >= rect.0 && x < rect.2 && y >= rect.1 && y < rect.3
}

/// `without`, with every pixel `mapped` says is covered passed through `map`.
fn expected(
    without: &Bitmap,
    mapped: impl Fn(u32, u32) -> bool,
    map: impl Fn(&mut [u8]),
) -> Vec<u8> {
    let n = without.components();
    let mut data = without.data.clone();
    for y in 0..without.height {
        for x in 0..without.width {
            if mapped(x, y) {
                let at = y as usize * without.stride + x as usize * n;
                map(&mut data[at..at + n]);
            }
        }
    }
    data
}

/// An RGB image of two by two samples, object 10, and an axial shading,
/// object 11, for the fill, image and shading page.
fn image_and_shading() -> Vec<String> {
    vec![
        stream(
            "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /ASCIIHexDecode",
            "C83214 1478DC 50A03C F0C850>",
        ),
        "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [42 0 58 0] \
         /Function << /FunctionType 2 /Domain [0 1] /C0 [0.1 0.8 0.3] /C1 [0.9 0.2 0.6] /N 1 >> \
         /Extend [true true] >>"
            .to_string(),
    ]
}

/// A fill, an image and a shading, side by side on a 60 by 40 page, after
/// `/T gs`.
const OBJECTS: &str = "/T gs 0.2 0.6 0.9 rg 2 2 16 16 re f \
                       q 16 0 0 16 22 2 cm /Im0 Do Q \
                       q 42 2 16 16 re W n /Sh0 sh Q";

/// The three objects' pixels.
fn objects_cover(x: u32, y: u32) -> bool {
    [2, 22, 42]
        .iter()
        .any(|x0| inside(pixels(*x0, 2, 16, 16, 40), x, y))
}

fn objects_page(transfer: &str) -> Vec<u8> {
    let mut extra = curves();
    extra.extend(image_and_shading());
    page(
        60,
        40,
        &format!(
            "<< /ExtGState << /T << {transfer} >> >> /XObject << /Im0 10 0 R >> \
             /Shading << /Sh0 11 0 R >> >>"
        ),
        OBJECTS,
        &extra,
    )
}

/// **A transfer function maps each component of what is painted through its
/// curve** (10.5): a fill, an image and a shading under one function and
/// under four render as the page without them with each covered pixel passed
/// through the curves; `/TR2` wins over `/TR`; `/Identity` and `/Default` move
/// no byte; a light page uses the first three functions, a grey one the
/// fourth, and an ink page all four in additive form; and the white the
/// objects leave is never painted, so it keeps the page's default.
#[test]
fn a_transfer_function_maps_each_component_through_its_curve() {
    let [inv, sq, hf, db] = [table(invert), table(square), table(half), table(double)];
    for format in [PixelFormat::Rgb8, PixelFormat::Gray8, PixelFormat::CmykA8] {
        let options = options(format);
        let without = render(objects_page(""), &options);

        let one = render(objects_page("/TR 6 0 R"), &options);
        let four = render(objects_page("/TR [6 0 R 7 0 R 8 0 R 9 0 R]"), &options);
        let second_wins = render(objects_page("/TR 7 0 R /TR2 6 0 R"), &options);
        let (want_one, want_four) = match format {
            PixelFormat::Rgb8 => (
                expected(&without, objects_cover, |px| {
                    for c in px.iter_mut() {
                        *c = inv[usize::from(*c)];
                    }
                }),
                expected(&without, objects_cover, |px| {
                    for (c, t) in px.iter_mut().zip([&inv, &sq, &hf]) {
                        *c = t[usize::from(*c)];
                    }
                }),
            ),
            PixelFormat::Gray8 => (
                expected(&without, objects_cover, |px| {
                    px[0] = inv[usize::from(px[0])]
                }),
                expected(&without, objects_cover, |px| px[0] = db[usize::from(px[0])]),
            ),
            _ => (
                expected(&without, objects_cover, |px| {
                    for c in px.iter_mut().take(4) {
                        *c = 255 - inv[usize::from(255 - *c)];
                    }
                }),
                expected(&without, objects_cover, |px| {
                    for (c, t) in px.iter_mut().zip([&inv, &sq, &hf, &db]) {
                        *c = 255 - t[usize::from(255 - *c)];
                    }
                }),
            ),
        };
        assert_ne!(
            one.data, without.data,
            "{format:?}: the transfer moved nothing"
        );
        assert_eq!(one.data, want_one, "{format:?}: one function");
        assert_eq!(four.data, want_four, "{format:?}: four functions");
        assert_eq!(second_wins.data, want_one, "{format:?}: /TR2 over /TR");
        for device in ["/TR /Identity", "/TR2 /Default", "/TR 6 0 R /TR2 /Default"] {
            assert_eq!(
                render(objects_page(device), &options).data,
                without.data,
                "{format:?}: {device} is the device's own"
            );
        }
    }
}

/// **DeviceGray on an ink page takes the grey function alone, on black**
/// (10.5's special case): `0.3 g` and `0.2 0.6 0.9 rg` under four functions
/// on the ink page — the grey fill's cyan, magenta and yellow are left as the
/// conversion made them, nothing, where the RGB fill's pass through theirs.
#[test]
fn a_device_gray_colour_on_ink_takes_the_grey_function_alone() {
    let options = options(PixelFormat::CmykA8);
    let content = "/T gs 0.3 g 2 2 16 16 re f 0.2 0.6 0.9 rg 22 2 16 16 re f";
    let doc = |transfer: &str| {
        page(
            40,
            20,
            &format!("<< /ExtGState << /T << {transfer} >> >> >>"),
            content,
            &curves(),
        )
    };
    let without = render(doc(""), &options);
    let four = render(doc("/TR [6 0 R 7 0 R 8 0 R 9 0 R]"), &options);
    let tables = [table(invert), table(square), table(half), table(double)];
    let grey = pixels(2, 2, 16, 16, 20);
    let colour = pixels(22, 2, 16, 16, 20);
    let mut want = without.data.clone();
    let n = without.components();
    for y in 0..without.height {
        for x in 0..without.width {
            let at = y as usize * without.stride + x as usize * n;
            let px = &mut want[at..at + n];
            if inside(grey, x, y) {
                assert_eq!(&px[..3], &[0, 0, 0], "the grey fill is black ink alone");
                px[3] = 255 - tables[3][usize::from(255 - px[3])];
            } else if inside(colour, x, y) {
                for (c, t) in px.iter_mut().zip(&tables) {
                    *c = 255 - t[usize::from(255 - *c)];
                }
            }
        }
    }
    assert_eq!(four.data, want);
}

/// **The topmost fully opaque object's transfer, at every point**
/// (11.7.5.2): an opaque fill under `1 − x` covers the page, and over its
/// middle each case below lays an object that is not fully opaque — at
/// `ca 0.5`, under a soft mask, inside a group invoked at `ca 0.5`, an image
/// with a soft mask — or one that is, under the device's own transfer. Where
/// that object lies the page's default applies, so those pixels are the
/// page without any transfer; elsewhere the fill's own.
#[test]
fn a_transfer_function_is_the_topmost_fully_opaque_object_s() {
    let options = options(PixelFormat::Rgb8);
    let inv = table(invert);
    // Object 10: a luminosity mask white over the left half of the middle,
    // black over the right; object 11: a group drawing a fill; object 12: an
    // image whose /SMask is object 13.
    let mut extra = curves();
    let mask = "1 g 20 0 10 40 re f 0 g 30 0 10 40 re f";
    extra.push(stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 60 40] \
         /Group << /S /Transparency /CS /DeviceGray >>",
        mask,
    ));
    extra.push(stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 60 40] /Group << /S /Transparency >>",
        "0.9 0.2 0.1 rg 20 2 20 16 re f",
    ));
    extra.push(stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8 /SMask 13 0 R /Filter /ASCIIHexDecode",
        "30C060>",
    ));
    extra.push(stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray \
         /BitsPerComponent 8 /Filter /ASCIIHexDecode",
        "FF>",
    ));
    let cases = [
        (
            "a translucent fill",
            "/Half gs 0.9 0.2 0.1 rg 20 2 20 16 re f",
        ),
        (
            "a soft-masked fill",
            "/Masked gs 0.9 0.2 0.1 rg 20 2 20 16 re f",
        ),
        ("a group at ca 0.5", "/Half gs /Fm0 Do"),
        ("an image with a soft mask", "q 20 0 0 16 20 2 cm /Im0 Do Q"),
        (
            "an opaque fill under the identity",
            "/Plain gs 0.9 0.2 0.1 rg 20 2 20 16 re f",
        ),
    ];
    let middle = pixels(20, 2, 20, 16, 40);
    for (what, case) in cases {
        let doc = |transfer: &str| {
            page(
                60,
                40,
                &format!(
                    "<< /ExtGState << /T << {transfer} >> /Half << /ca 0.5 >> \
                     /Masked << /SMask << /S /Luminosity /G 10 0 R >> >> \
                     /Plain << /TR /Identity >> >> \
                     /XObject << /Fm0 11 0 R /Im0 12 0 R >> >>"
                ),
                &format!("q /T gs 0.2 0.6 0.9 rg 2 2 56 16 re f {case} Q"),
                &extra,
            )
        };
        let without = render(doc(""), &options);
        let with = render(doc("/TR 6 0 R"), &options);
        let fill = pixels(2, 2, 56, 16, 40);
        let want = expected(
            &without,
            |x, y| inside(fill, x, y) && !inside(middle, x, y),
            |px| {
                for c in px.iter_mut() {
                    *c = inv[usize::from(*c)];
                }
            },
        );
        assert_eq!(with.data, want, "{what}");
    }
}

/// **A tile of a transferred page is the page under it** (ruling 5): the
/// plane is kept in device pixels of the page, so every tile of the fill,
/// image and shading page under four functions is its rectangle of the page.
#[test]
fn a_transferred_tile_is_the_page_under_it() {
    let doc = Document::open(objects_page("/TR [6 0 R 7 0 R 8 0 R 9 0 R]")).expect("it opens");
    let page = doc.page(0).expect("a page");
    for format in [PixelFormat::Rgb8, PixelFormat::CmykA8] {
        let options = options(format);
        let whole = page.render(&options);
        let n = whole.components();
        for (x, y, w, h) in [(0, 0, 23, 17), (23, 17, 37, 23), (11, 5, 31, 29)] {
            let tile = page.render(&RenderOptions {
                region: Some(PixelRegion::new(x, y, w, h)),
                ..options.clone()
            });
            for row in 0..h {
                let from = (y + row) as usize * whole.stride + x as usize * n;
                assert_eq!(
                    &tile.data[row as usize * tile.stride..][..w as usize * n],
                    &whole.data[from..from + w as usize * n],
                    "{format:?}: tile ({x}, {y}) row {row}"
                );
            }
        }
    }
}

/// A Type0 face of `render_support::curvy_font`'s shapes as objects `first`
/// to `first + 3`, its font dictionary object `first`: `/Identity-H` codes and
/// `/CIDToGIDMap /Identity`, so a two-byte code is the glyph it shows.
fn curvy_type0(first: usize) -> Vec<String> {
    let program = render_support::curvy_font();
    let hex: String = program.iter().map(|b| format!("{b:02X}")).collect();
    vec![
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /Curvy /Encoding /Identity-H \
             /DescendantFonts [{} 0 R] >>",
            first + 1
        ),
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Curvy \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /FontDescriptor {} 0 R /CIDToGIDMap /Identity /DW 640 >>",
            first + 2
        ),
        format!(
            "<< /Type /FontDescriptor /FontName /Curvy /Flags 4 /FontBBox [0 0 1000 1000] \
             /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 \
             /FontFile2 {} 0 R >>",
            first + 3
        ),
        stream(
            &format!("/Length1 {} /Filter /ASCIIHexDecode", program.len()),
            &format!("{hex}>"),
        ),
    ]
}

/// **A glyph takes the device-dependent state as a path does**: four glyphs
/// in mode 0, each drawn from a mask of its own, with hard edges so that a
/// pixel is the fill colour or the page's own. Under `/TR` on a light page
/// each glyph pixel is its colour through the curve and the white the glyphs
/// leave is not, so the plane holds each glyph's shape; under `/BG2` and
/// `/UCR2` on the ink page each glyph pixel is the formula's ink of its light
/// and the paper keeps none.
#[test]
fn a_glyph_takes_the_transfer_and_the_ink_tables() {
    let mut extra = curves();
    extra.push(calculator(BLACK_HALF));
    extra.push(calculator(REMOVE_LESS));
    extra.extend(curvy_type0(12));
    let doc = |state: &str| {
        page(
            90,
            40,
            &format!("<< /ExtGState << /G << {state} >> >> /Font << /F0 12 0 R >> >>"),
            "/G gs 0.2 0.6 0.9 rg BT /F0 30 Tf 4 8 Td <0004000500060001> Tj ET",
            &extra,
        )
    };
    let hard = |format: PixelFormat| RenderOptions {
        antialias: false,
        ..options(format)
    };

    // On light: where the glyphs painted, and in what.
    let light = hard(PixelFormat::Rgb8);
    let without = render(doc(""), &light);
    let lit = |x: u32, y: u32| y as usize * without.stride + x as usize * 3;
    let mut glyph = Vec::new();
    for y in 0..without.height {
        for x in 0..without.width {
            if without.data[lit(x, y)..lit(x, y) + 3] != [255, 255, 255] {
                glyph.push((x, y));
            }
        }
    }
    assert!(!glyph.is_empty(), "the glyphs drew nothing");
    let first = lit(glyph[0].0, glyph[0].1);
    let colour = [
        without.data[first],
        without.data[first + 1],
        without.data[first + 2],
    ];
    let inv = table(invert);
    let mut want = without.data.clone();
    for &(x, y) in &glyph {
        let at = lit(x, y);
        assert_eq!(
            &without.data[at..at + 3],
            &colour[..],
            "a hard edge is the colour or the white"
        );
        for c in &mut want[at..at + 3] {
            *c = inv[usize::from(*c)];
        }
    }
    assert_eq!(render(doc("/TR 6 0 R"), &light).data, want, "under /TR");

    // On ink: the same pixels, their light through the formula.
    let ink_options = hard(PixelFormat::CmykA8);
    let without = render(doc(""), &ink_options);
    let with = render(doc("/BG2 10 0 R /UCR2 11 0 R"), &ink_options);
    let formula = ink(colour, black_half, remove_less);
    let mut want = without.data.clone();
    for &(x, y) in &glyph {
        let at = y as usize * without.stride + x as usize * 5;
        want[at..at + 4].copy_from_slice(&formula);
    }
    assert_ne!(with.data, without.data, "the ink tables moved nothing");
    assert_eq!(with.data, want, "under /BG2 and /UCR2");
}

/// 10.4.2.4 in bytes, from tables sampled as the engine samples them: the
/// black generation to `[0 1]` and the undercolour removal to `[−1 1]`.
fn ink(light: [u8; 3], black: fn(f64) -> f64, removal: fn(f64) -> f64) -> [u8; 4] {
    let k = 255 - light.iter().copied().max().unwrap_or(0);
    let x = f64::from(k) / 255.0;
    let take = (removal(x).clamp(-1.0, 1.0) * 255.0).round() as i32;
    let black = (black(x).clamp(0.0, 1.0) * 255.0).round() as u8;
    let c = |v: u8| (255 - i32::from(v) - take).clamp(0, 255) as u8;
    [c(light[0]), c(light[1]), c(light[2]), black]
}

const BLACK_HALF: &str = "{ 0.5 mul }";
fn black_half(x: f64) -> f64 {
    x * 0.5
}
const REMOVE_LESS: &str = "{ 0.25 mul 0.1 sub }";
fn remove_less(x: f64) -> f64 {
    x * 0.25 - 0.1
}
fn unchanged(x: f64) -> f64 {
    x
}

/// **Black generation and undercolour removal make the ink the test
/// computes** (10.4.2.4), and only where DeviceRGB becomes DeviceCMYK
/// (11.7.5.3): on the ink page, under `/BG2 { 0.5 mul }` and
/// `/UCR2 { 0.25 mul 0.1 sub }` — negative for a pale colour, so it adds ink
/// — an `rg` fill, an RGB image, an RGB group and an uncoloured pattern of
/// `[/Pattern /DeviceRGB]` take their light into ink by the formula, the
/// group by the functions in force at its `Do`; a `g` fill
/// is 10.4.2.3's black alone and a `k` fill its own components, as without
/// the keys; with `/BG` alone the removal is `k` itself; inside a
/// `/DeviceCMYK` group on a light page the `rg` fill's ink is the formula's,
/// shown as its light; and without the keys every byte is today's.
#[test]
fn black_generation_and_undercolour_removal_make_the_ink_the_test_computes() {
    let mut extra = vec![calculator(BLACK_HALF), calculator(REMOVE_LESS)];
    extra.push(stream(
        "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB \
         /BitsPerComponent 8 /Filter /ASCIIHexDecode",
        "C83214 1478DC 50A03C F0C8F0>",
    ));
    extra.push(stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 80 20] \
         /Group << /S /Transparency /CS /DeviceRGB >>",
        "0.8 0.7 0.1 rg 62 2 16 16 re f",
    ));
    extra.push(stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 80 20] \
         /Group << /S /Transparency /CS /DeviceCMYK >>",
        "0.2 0.6 0.9 rg 2 2 16 16 re f",
    ));
    extra.push(stream(
        "/Type /Pattern /PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 4 4] \
         /XStep 4 /YStep 4 /Resources << >>",
        "0 0 4 4 re f",
    ));
    let content = "/G gs 0.2 0.6 0.9 rg 2 2 16 16 re f \
                   0.3 g 22 2 16 16 re f \
                   q 16 0 0 16 42 2 cm /Im0 Do Q \
                   /Fm0 Do \
                   0.1 0.2 0.3 0.4 k 82 2 16 16 re f \
                   /CS0 cs 0.7 0.3 0.5 /P0 scn 102 2 16 16 re f";
    let doc = |state: &str| {
        page(
            120,
            20,
            &format!(
                "<< /ExtGState << /G << {state} >> >> /XObject << /Im0 8 0 R /Fm0 9 0 R >> \
                 /ColorSpace << /CS0 [/Pattern /DeviceRGB] >> /Pattern << /P0 11 0 R >> >>"
            ),
            content,
            &extra,
        )
    };
    let ink_options = options(PixelFormat::CmykA8);
    let light_options = options(PixelFormat::Rgb8);
    let without = render(doc(""), &ink_options);
    let light = render(doc(""), &light_options);
    let rgb_fill = pixels(2, 2, 16, 16, 20);
    let image = pixels(42, 2, 16, 16, 20);
    let group = pixels(62, 2, 16, 16, 20);
    let pattern = pixels(102, 2, 16, 16, 20);
    let rgb = |x: u32, y: u32| {
        inside(rgb_fill, x, y)
            || inside(image, x, y)
            || inside(group, x, y)
            || inside(pattern, x, y)
    };
    let light_at = |x: u32, y: u32| {
        let at = y as usize * light.stride + x as usize * 3;
        [light.data[at], light.data[at + 1], light.data[at + 2]]
    };

    for (state, black, removal) in [
        (
            "/BG2 6 0 R /UCR2 7 0 R",
            black_half as fn(f64) -> f64,
            remove_less as fn(f64) -> f64,
        ),
        ("/BG 6 0 R", black_half, unchanged),
        ("/UCR 7 0 R", unchanged, remove_less),
    ] {
        let with = render(doc(state), &ink_options);
        let mut want = without.data.clone();
        for y in 0..without.height {
            for x in 0..without.width {
                if rgb(x, y) {
                    let at = y as usize * without.stride + x as usize * 5;
                    want[at..at + 4].copy_from_slice(&ink(light_at(x, y), black, removal));
                }
            }
        }
        assert_ne!(with.data, without.data, "{state}: nothing moved");
        assert_eq!(with.data, want, "{state}");
    }
    assert_eq!(
        render(doc("/BG2 /Default /UCR2 /Default"), &ink_options).data,
        without.data,
        "the device's own is maximum removal"
    );

    // A /DeviceCMYK group on a light page: the fill's ink is the formula's,
    // shown as 8.6.4.4's light.
    let cmyk_group = page(
        20,
        20,
        "<< /ExtGState << /G << /BG2 6 0 R /UCR2 7 0 R >> >> /XObject << /Fm0 10 0 R >> >>",
        "/G gs /Fm0 Do",
        &extra,
    );
    let shown = render(cmyk_group, &light_options);
    let [c, m, y, k] = ink([51, 153, 230], black_half, remove_less);
    let want = tinker_pdf_raster::canvas::cmyk_to_rgb(c, m, y, k);
    let at = 10 * shown.stride + 10 * 3;
    assert_eq!(
        (shown.data[at], shown.data[at + 1], shown.data[at + 2]),
        want,
        "inside the /DeviceCMYK group"
    );
}
