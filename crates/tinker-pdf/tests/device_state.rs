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

// ---- flatness (10.7.2) --------------------------------------------------------

/// A cubic in device pixels, as four points.
type Cubic = [(f64, f64); 4];

/// The flattener's control-polygon rule (`tinker-pdf-raster`'s `geom.rs`),
/// written out here: the control polygon's length, one segment within the
/// tolerance, and otherwise `ceil(1.5 × √(polygon / tolerance))` steps, one to
/// 512, each point the Bernstein form at `i / steps` in the same order of
/// operations. Appends the points after the first.
fn flatten_cubic(p: Cubic, tolerance: f64, out: &mut Vec<(f64, f64)>) {
    let distance = |a: (f64, f64), b: (f64, f64)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        (dx * dx + dy * dy).sqrt()
    };
    let polygon = distance(p[0], p[1]) + distance(p[1], p[2]) + distance(p[2], p[3]);
    if !polygon.is_finite() || polygon <= tolerance {
        out.push(p[3]);
        return;
    }
    let steps = ((polygon / tolerance).sqrt() * 1.5)
        .ceil()
        .clamp(1.0, 512.0) as u32;
    for i in 1..=steps {
        let t = f64::from(i) / f64::from(steps);
        let u = 1.0 - t;
        let x = u * u * u * p[0].0
            + 3.0 * u * u * t * p[1].0
            + 3.0 * u * t * t * p[2].0
            + t * t * t * p[3].0;
        let y = u * u * u * p[0].1
            + 3.0 * u * u * t * p[1].1
            + 3.0 * u * t * t * p[2].1
            + t * t * t * p[3].1;
        out.push((x, y));
    }
}

/// An ellipse about `(cx, cy)` with radii `rx`, `ry` as four cubics in PDF
/// space, its handles `9/16` of each radius: every coordinate a short dyadic
/// fraction, so the page transform's `64 − y` is exact and the device points
/// are the ones the test computes.
fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> [Cubic; 4] {
    let (hx, hy) = (rx * 0.5625, ry * 0.5625);
    [
        [
            (cx + rx, cy),
            (cx + rx, cy + hy),
            (cx + hx, cy + ry),
            (cx, cy + ry),
        ],
        [
            (cx, cy + ry),
            (cx - hx, cy + ry),
            (cx - rx, cy + hy),
            (cx - rx, cy),
        ],
        [
            (cx - rx, cy),
            (cx - rx, cy - hy),
            (cx - hx, cy - ry),
            (cx, cy - ry),
        ],
        [
            (cx, cy - ry),
            (cx + hx, cy - ry),
            (cx + rx, cy - hy),
            (cx + rx, cy),
        ],
    ]
}

/// The content drawing `curves` as one closed path.
fn curve_operators(curves: &[Cubic]) -> String {
    let mut out = format!("{} {} m", curves[0][0].0, curves[0][0].1);
    for c in curves {
        out.push_str(&format!(
            " {} {} {} {} {} {} c",
            c[1].0, c[1].1, c[2].0, c[2].1, c[3].0, c[3].1
        ));
    }
    out.push_str(" h");
    out
}

/// The two shapes every flatness page draws: a circle and an ellipse.
fn flatness_shapes() -> [[Cubic; 4]; 2] {
    [
        ellipse(32.0, 32.0, 24.0, 24.0),
        ellipse(96.0, 32.0, 24.0, 12.0),
    ]
}

/// A 128 by 64 page drawing `body` after `/F gs`, `/F` setting `state`.
fn flatness_page(state: &str, body: &str) -> Vec<u8> {
    page(
        128,
        64,
        &format!("<< /ExtGState << /F << {state} >> >> >>"),
        &format!("/F gs {body}"),
        &[],
    )
}

/// Both shapes filled, `0.2 0.4 0.8 rg`.
fn filled_shapes() -> String {
    let [circle, oval] = flatness_shapes();
    format!(
        "0.2 0.4 0.8 rg {} f {} f",
        curve_operators(&circle),
        curve_operators(&oval)
    )
}

/// **Flatness sets the tolerance a curve is flattened to** (10.7.2): a circle
/// and an ellipse filled under `/FL` 1, 5 and 20 are byte for byte the
/// polygons the test derives from the flattener's rule at that tolerance,
/// filled through the same rasterizer onto a white canvas; `5 i` draws as
/// `/FL 5` filled, stroked and as a clip; 0, a negative value and the
/// device's own 0.2 draw as no flatness at all; a value past 100 draws as 100
/// and one finer than 1/256 of a pixel as 1/256; and a coarse flatness moves a
/// stroke, a clip and a glyph, filled and as a text clip, so each of those
/// paths takes it too.
#[test]
fn flatness_sets_the_tolerance_a_curve_is_flattened_to() {
    use tinker_pdf_raster::{fill, Canvas, Color, FillRule, Path};
    let options = RenderOptions::default();
    let none = render(flatness_page("", &filled_shapes()), &options);

    for flatness in [1.0, 5.0, 20.0] {
        let page = render(
            flatness_page(&format!("/FL {flatness}"), &filled_shapes()),
            &options,
        );
        let mut twin = Canvas::new(128, 64, tinker_pdf_raster::PixelFormat::Rgb8, Color::WHITE);
        for shape in flatness_shapes() {
            // Device space: x as it is, y counted down from the top.
            let device = shape.map(|c| c.map(|(x, y)| (x, 64.0 - y)));
            let mut points = vec![device[0][0]];
            for cubic in device {
                flatten_cubic(cubic, flatness, &mut points);
            }
            let mut path = Path::new();
            path.move_to(points[0].0, points[0].1);
            for (x, y) in &points[1..] {
                path.line_to(*x, *y);
            }
            path.close();
            let mask = fill(&path, FillRule::NonZero, 0, 0, 128, 64, flatness, None);
            twin.fill_mask(&mask, Color::rgb(51, 102, 204), 1.0);
        }
        assert_eq!(
            page.data, twin.data,
            "/FL {flatness}: not the derived polygons"
        );
        if flatness >= 5.0 {
            assert_ne!(page.data, none.data, "/FL {flatness} moved nothing");
        }
    }

    // The operator's twin, and the readings of what Table 56 does not allow.
    let [circle, _] = flatness_shapes();
    let stroked = format!("0 0 1 RG 3 w {} S", curve_operators(&circle));
    let clipped = format!(
        "q {} W n 0.9 0.1 0.1 rg 0 0 128 64 re f Q",
        curve_operators(&circle)
    );
    for body in [filled_shapes(), stroked.clone(), clipped.clone()] {
        let gs = render(flatness_page("/FL 5", &body), &options);
        let operator = render(flatness_page("", &format!("5 i {body}")), &options);
        assert_eq!(gs.data, operator.data, "5 i against /FL 5: {body}");
        let without = render(flatness_page("", &body), &options);
        for device in ["/FL 0.2", "/FL 0", "/FL -3"] {
            assert_eq!(
                render(flatness_page(device, &body), &options).data,
                without.data,
                "{device} is the device's own: {body}"
            );
        }
        assert_eq!(
            render(flatness_page("/FL 1000", &body), &options).data,
            render(flatness_page("/FL 100", &body), &options).data,
            "past 100 is 100: {body}"
        );
        assert_eq!(
            render(flatness_page("/FL 0.000000001", &body), &options).data,
            render(flatness_page("/FL 0.00390625", &body), &options).data,
            "finer than 1/256 is 1/256: {body}"
        );
    }
    for body in [stroked, clipped] {
        assert_ne!(
            render(flatness_page("/FL 40", &body), &options).data,
            render(flatness_page("", &body), &options).data,
            "a coarse flatness moved nothing: {body}"
        );
    }

    // A glyph of the curved face, filled and as a text clip, under `i`.
    let glyph = |flatness: &str, mode: u8| {
        let mut builder = tinker_pdf::DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(b"F0", b"Curvy", &render_support::curvy_font()));
        builder.add_page(96.0, 48.0, |page| {
            page.raw(flatness.as_bytes());
            page.raw(format!("{mode} Tr").as_bytes());
            page.text(b"F0", 30.0, 4.0, 10.0, "0123");
            // What the clip lets through; a filled glyph is the page itself.
            if mode == 7 {
                page.raw(b"0.9 0.1 0.1 rg 0 0 96 48 re f");
            }
        });
        render(builder.finish(), &options).data
    };
    for mode in [0, 7] {
        assert_eq!(glyph("0.2 i", mode), glyph("", mode), "mode {mode}: 0.2 i");
        assert_eq!(glyph("0 i", mode), glyph("", mode), "mode {mode}: 0 i");
        assert_ne!(
            glyph("30 i", mode),
            glyph("", mode),
            "mode {mode}: 30 i moved nothing"
        );
    }
}

/// **A text clip's glyphs are each flattened at the flatness they were shown
/// at**: a clipping text object whose glyphs are all shown at one coarse
/// flatness clips as it does with that flatness set before `BT`, and one
/// whose second glyph is shown after `i` changed it differs from both
/// readings that hold one flatness for the whole object.
#[test]
fn a_text_clip_flattens_each_glyph_at_its_own_flatness() {
    let options = RenderOptions::default();
    let clip = |before: &str, between: &str| {
        let mut builder = tinker_pdf::DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(b"F0", b"Curvy", &render_support::curvy_font()));
        builder.add_page(96.0, 48.0, |page| {
            // Shown invisibly, which binds the face to the page.
            page.raw(b"3 Tr");
            page.text(b"F0", 30.0, 4.0, 10.0, "0123");
            page.raw(before.as_bytes());
            // One text object: its glyphs clip together at its `ET`.
            page.raw(format!("BT /F0 30 Tf 7 Tr 4 10 Td (01) Tj {between} (23) Tj ET").as_bytes());
            page.raw(b"0 Tr 0.9 0.1 0.1 rg 0 0 96 48 re f");
        });
        render(builder.finish(), &options).data
    };
    let coarse = clip("30 i", "");
    assert_eq!(clip("30 i", "30 i"), coarse, "one flatness, set twice");
    let mixed = clip("30 i", "0.2 i");
    assert_ne!(mixed, coarse, "the second glyphs took the first's flatness");
    assert_ne!(
        mixed,
        clip("", ""),
        "the first glyphs took the second's flatness"
    );
}

/// **A glyph's mask is kept per flatness**: within one render, the same four
/// glyphs at the same phases, shown once at the device's flatness and once at
/// a coarse one, in either order, each draw as they do on a page that shows
/// them alone — so neither run is drawn from the other's masks — and the
/// coarse run is not the device's.
#[test]
fn a_glyph_mask_is_kept_per_flatness() {
    let options = RenderOptions::default();
    // "0123" at x 4 and at x 100, a whole 96 pixels apart, so each glyph of
    // the second run stands at its twin's phase; `None` leaves a run out.
    let shown = |left: Option<&str>, right: Option<&str>| {
        let mut builder = tinker_pdf::DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(b"F0", b"Curvy", &render_support::curvy_font()));
        builder.add_page(192.0, 48.0, |page| {
            for (flatness, x) in [(left, 4.0), (right, 100.0)] {
                if let Some(flatness) = flatness {
                    page.raw(format!("q {flatness}").as_bytes());
                    page.text(b"F0", 30.0, x, 10.0, "0123");
                    page.raw(b"Q");
                }
            }
        });
        render(builder.finish(), &options)
    };
    // The left or the right 96 columns, row after row.
    let half = |bitmap: &Bitmap, right: bool| -> Vec<u8> {
        let from = if right { 96 * 3 } else { 0 };
        (0..bitmap.height as usize)
            .flat_map(|row| bitmap.data[row * bitmap.stride + from..][..96 * 3].to_vec())
            .collect()
    };
    for (first, second) in [("", "30 i"), ("30 i", "")] {
        let both = shown(Some(first), Some(second));
        assert_eq!(
            half(&both, false),
            half(&shown(Some(first), None), false),
            "{first:?} then {second:?}: the first run"
        );
        assert_eq!(
            half(&both, true),
            half(&shown(None, Some(second)), true),
            "{first:?} then {second:?}: the second run"
        );
    }
    assert_ne!(
        half(&shown(None, Some("30 i")), true),
        half(&shown(None, Some("")), true),
        "30 i moved nothing"
    );
}

/// **A flattened tile is the page under it** (ruling 5): the tolerance is a
/// device-pixel measure of device geometry, so a tile flattens the page's
/// curves and is its rectangle of the page.
#[test]
fn a_flattened_tile_is_the_page_under_it() {
    let [circle, _] = flatness_shapes();
    let body = format!(
        "{} 0 0 1 RG 2 w {} S",
        filled_shapes(),
        curve_operators(&circle)
    );
    let doc = Document::open(flatness_page("/FL 6", &body)).expect("it opens");
    let page = doc.page(0).expect("a page");
    let options = RenderOptions::default();
    let whole = page.render(&options);
    for (x, y, w, h) in [(0, 0, 37, 23), (37, 23, 64, 41), (90, 5, 38, 59)] {
        let tile = page.render(&RenderOptions {
            region: Some(PixelRegion::new(x, y, w, h)),
            ..options.clone()
        });
        for row in 0..h {
            let from = (y + row) as usize * whole.stride + x as usize * 3;
            assert_eq!(
                &tile.data[row as usize * tile.stride..][..w as usize * 3],
                &whole.data[from..from + w as usize * 3],
                "tile ({x}, {y}) row {row}"
            );
        }
    }
}
