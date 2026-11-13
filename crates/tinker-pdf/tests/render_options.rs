//! The `RenderOptions` fields that change what a page's bytes *are* rather than
//! which page or how large: ink instead of light, what a pixel means when it
//! is not fully painted, and a page drawn in two colours by its coverage.
//!
//! # Why a file of its own, and why these hashes are not in `determinism.rs`
//!
//! `determinism.rs` pins twenty pages rendered with `RenderOptions::default()`
//! and nothing else: its `Fixture` has a builder, a way to open and an ink floor,
//! and no options, and the table it checks is the one thing in the repository a
//! determinism bug would show up in. Growing an options column there would
//! change that file's shape to add a row, and a row that means "this page, but
//! differently" beside twenty that mean "this page" is how a reader of that
//! table gets the wrong idea about what moved.
//!
//! So each option carries its own pinned hash here, computed the way that file
//! computes one — SHA-256 over the width and height, big-endian, and the pixel
//! bytes — over pages `render_support` builds, which are the same bytes that
//! file's `analytic_*` rows hash. And each option has the claim that matters
//! most beside it: **the default is unchanged**. That is held two ways: the
//! default render of the blend grid still hashes to the value `determinism.rs`
//! pins for `analytic_blend`, copied here verbatim, and the option spelled out
//! at its default renders the same bytes as not mentioning it.
//!
//! A mismatch prints the replacement line, as `determinism.rs` does. The same
//! rule applies: two targets disagreeing is a determinism bug and the table is
//! not to be edited to make it go away.

mod render_support;
mod written_support;

use render_support::{blend_grid_page, curvy_font};
use tinker_pdf::{
    Bitmap, BlendMode, Color, DeviceSpace, Document, DocumentBuilder, ExtGState, ForcedColours,
    FormXObject, HardEdge, ImageData, MaskKind, PixelFormat, RenderOptions, StateMask,
    TransparencyGroup,
};
use tinker_pdf_raster::canvas::rgb_to_cmyk;

/// `determinism.rs`'s `analytic_blend` row, copied: the blend grid rendered
/// with the default options. If this and that file ever disagree, one of them
/// was edited without the other.
const ANALYTIC_BLEND_DEFAULT: &str =
    "e1d056a15ae8cb7f18fd1524dfcc419893f4a5283c2f56b3f7307380aee43560";

/// The hash `determinism.rs` takes: dimensions, then bytes.
fn fingerprint(bitmap: &Bitmap) -> String {
    let mut input = Vec::with_capacity(bitmap.data.len() + 8);
    input.extend_from_slice(&bitmap.width.to_be_bytes());
    input.extend_from_slice(&bitmap.height.to_be_bytes());
    input.extend_from_slice(&bitmap.data);
    tinker_pdf_crypto::sha2::sha256(&input)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn render(bytes: Vec<u8>, options: &RenderOptions) -> Bitmap {
    Document::open(bytes)
        .expect("the document opens")
        .page(0)
        .expect("a page")
        .render(options)
}

/// Pixels something was painted on. The background is white (no ink, for
/// `CmykA8`) and opaque, or — on a transparent page — nothing at all.
fn painted(bitmap: &Bitmap) -> usize {
    let components = bitmap.components();
    let alpha_at = bitmap.format.has_alpha().then_some(components - 1);
    bitmap
        .data
        .chunks_exact(components)
        .filter(|pixel| {
            let alpha = alpha_at.map_or(255, |at| pixel[at]);
            if alpha == 0 {
                return false;
            }
            let colours = &pixel[..alpha_at.unwrap_or(components)];
            alpha != 255
                || match bitmap.format {
                    PixelFormat::CmykA8 => colours.iter().any(|v| *v != 0),
                    _ => colours.iter().any(|v| *v != 255),
                }
        })
        .count()
}

/// Asserts a pinned hash, printing the line to paste if it moved — after
/// asserting the page painted at least `least` pixels, because a hash of a
/// page that stopped drawing is as stable as any other (`determinism.rs`'s
/// header tells the story of the fixture that was blank for months).
fn pinned(name: &str, bitmap: &Bitmap, least: usize, want: &str) {
    let drawn = painted(bitmap);
    assert!(
        drawn >= least,
        "the `{name}` render painted {drawn} pixels, fewer than {least}: it is \
         measuring less than it claims. Warnings: {:?}",
        bitmap.warnings
    );
    let actual = fingerprint(bitmap);
    assert_eq!(
        actual, want,
        "the `{name}` render moved.\n\n\
         If two targets disagree, this is a determinism bug: find the \
         arithmetic that is not target-stable and do not update the value.\n\
         If this is a deliberate rendering change, say what moved in the \
         commit and paste:\n\n    \"{actual}\""
    );
}

/// Text in an embedded face, a DeviceCMYK fill, a rich black, a pure-K fill,
/// and a translucent `/Multiply` band laid over all three: the page whose ink
/// is worth looking at.
fn ink_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    assert!(builder.add_ext_gstate(
        b"G0",
        &ExtGState {
            fill_alpha: Some(0.5),
            blend_mode: Some(BlendMode::Multiply),
            ..ExtGState::default()
        }
    ));
    builder.add_page(96.0, 48.0, |page| {
        page.text(b"F0", 12.0, 4.0, 30.0, "ink 0123");
        page.raw(b"1 0 0 0 k 4 4 20 20 re f");
        page.raw(b"1 1 1 1 k 28 4 20 20 re f");
        page.raw(b"0 0 0 1 k 52 4 20 20 re f");
        page.raw(b"q");
        assert!(page.set_ext_gstate(b"G0"));
        page.raw(b"0 1 0 0 k 14 10 50 14 re f");
        page.raw(b"Q");
    });
    built("ink page", builder)
}

// ---- CMYK page output --------------------------------------------------------

/// Asking for ink and saying so gets ink: five bytes a pixel, the canvas
/// starting with none, and each fill's own components — `1 0 0 0 k` is
/// cyan, `0 0 0 1 k` black ink, and a rich black `1 1 1 1 k` all four inks.
///
/// **The rich black is the flip.** Until October 2026 every colour was
/// flattened to light where it was read, and an ink page took the light back
/// through 8.6.4.4 inverted with maximum undercolour removal, which has one
/// ink for each colour: the rich black came back as the pure K of the same
/// shade, and this test pinned that as the limitation it was. The components
/// now travel beside the light (`GraphicsState::fill_ink`), and an ink canvas
/// composites them.
#[test]
fn a_page_asked_for_in_ink_with_the_opt_in_comes_back_in_ink() {
    let ink = render(
        ink_page(),
        &RenderOptions {
            format: PixelFormat::CmykA8,
            allow_cmyk: true,
            ..RenderOptions::default()
        },
    );
    assert_eq!(ink.format, PixelFormat::CmykA8);
    assert_eq!(ink.components(), 5);
    assert_eq!(ink.stride, ink.width as usize * 5);

    let at = |x: u32, y: u32| -> [u8; 5] {
        let i = y as usize * ink.stride + x as usize * 5;
        ink.data[i..i + 5].try_into().expect("five bytes")
    };
    // Page space is 48 points tall at one pixel a point, so y flips.
    assert_eq!(
        at(1, 1),
        [0, 0, 0, 0, 255],
        "no ink where nothing is painted"
    );
    assert_eq!(at(14, 40), [255, 0, 0, 0, 255], "cyan is cyan");
    assert_eq!(at(62, 40), [0, 0, 0, 255, 255], "black is K");
    assert_eq!(
        at(38, 40),
        [255, 255, 255, 255, 255],
        "a rich black arrives as all four inks: the file's own components, \
         not light turned back into ink"
    );
}

/// **A DeviceCMYK colour's components reach an ink page unchanged**, however
/// the content stream names the space, for a fill, a stroke and a glyph —
/// `round(255 × component)` each, which is 8.6.4.4's components as bytes and
/// nothing else — and a colour that is not DeviceCMYK still becomes ink as it
/// did: 8.6.4.4's light, inverted with maximum undercolour removal.
///
/// Every square is whole pixels on a one-point-a-pixel page, so a sampled
/// pixel is wholly covered and its bytes are the colour's.
#[test]
fn a_device_cmyk_colour_reaches_an_ink_page_unchanged() {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    builder.add_page(120.0, 40.0, |page| {
        // `k`, the operator.
        page.raw(b"0.2 0.4 0.6 0.1 k 0 30 10 10 re f");
        // `cs` naming the device space, then `sc`.
        page.raw(b"/DeviceCMYK cs 0.1 0.9 0.3 0.7 sc 10 30 10 10 re f");
        // `cs` alone: 8.6.8 resets DeviceCMYK to `0 0 0 1`, black ink.
        page.raw(b"/DeviceCMYK cs 20 30 10 10 re f");
        // A stroke, `K`: a six-wide line along y = 25 from x 30 to 60.
        page.raw(b"1 0.5 0 0 K 6 w 30 25 m 60 25 l S");
        // A colour that is not DeviceCMYK: red, light, becomes ink as before.
        page.raw(b"1 0 0 rg 70 30 10 10 re f");
        // A glyph filled in rich black, drawn large.
        page.raw(b"BT /F0 30 Tf 1 1 1 1 k 85 5 Td (A) Tj ET");
    });
    let ink = render(
        built(
            "a device cmyk colour reaches an ink page unchanged",
            builder,
        ),
        &RenderOptions {
            format: PixelFormat::CmykA8,
            allow_cmyk: true,
            ..RenderOptions::default()
        },
    );
    let at = |x: u32, y: u32| -> [u8; 5] {
        let i = y as usize * ink.stride + x as usize * 5;
        ink.data[i..i + 5].try_into().expect("five bytes")
    };
    let byte = |v: f64| (v * 255.0_f64).round() as u8;
    // Page y 30 to 40 is pixel rows 0 to 9.
    assert_eq!(
        at(5, 5),
        [byte(0.2), byte(0.4), byte(0.6), byte(0.1), 255],
        "`k`"
    );
    assert_eq!(
        at(15, 5),
        [byte(0.1), byte(0.9), byte(0.3), byte(0.7), 255],
        "`/DeviceCMYK cs ... sc`"
    );
    assert_eq!(at(25, 5), [0, 0, 0, 255, 255], "`cs` resets to black ink");
    // Page y 22 to 28 is pixel rows 12 to 17.
    assert_eq!(at(45, 15), [255, byte(0.5), 0, 0, 255], "`K` on a stroke");
    // Red is (255, 0, 0) light, which maximum undercolour removal makes
    // `0 1 1 0` — the conversion every non-CMYK colour still takes.
    assert_eq!(at(75, 5), [0, 255, 255, 0, 255], "light still converts");
    // The glyph: every wholly covered pixel of it is all four inks.
    let glyph: Vec<[u8; 5]> = (85..120)
        .flat_map(|x| (0..40).map(move |y| (x, y)))
        .map(|(x, y)| at(x, y))
        .filter(|pixel| pixel[4] == 255 && pixel[..4].iter().any(|v| *v != 0))
        .collect();
    assert!(
        glyph.contains(&[255, 255, 255, 255, 255]),
        "the glyph is drawn, and wholly covered pixels are rich black"
    );
    assert!(
        glyph
            .iter()
            .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2] && pixel[2] == pixel[3]),
        "every glyph pixel, edges included, is the four inks in step: {:?}",
        glyph
            .iter()
            .find(|pixel| !(pixel[0] == pixel[1] && pixel[1] == pixel[2] && pixel[2] == pixel[3]))
    );

    // And in light, `cs` alone is black, as 8.6.8 says, where an all-zeros
    // reset made it white.
    let light = render(
        {
            let mut builder = DocumentBuilder::new();
            builder.add_page(30.0, 10.0, |page| {
                page.raw(b"/DeviceCMYK cs 0 0 10 10 re f");
            });
            built(
                "a device cmyk colour reaches an ink page unchanged",
                builder,
            )
        },
        &RenderOptions::default(),
    );
    assert_eq!(&light.data[..3], &[0, 0, 0], "`/DeviceCMYK cs` is black");
}

/// **A stencil mask paints the fill colour's own ink** (8.9.6.2): it has no
/// colour of its own and is painted with the current fill colour, which is a
/// flat colour like any fill's — so a DeviceCMYK one reaches an ink page as
/// its components, not as its light turned back into ink. Two abutting
/// stencils of one colour, a third of another beside them (abutting stencils
/// are held back and composited once, so the third must not join the first
/// two's ink), and a fourth in light red, which still converts. Each is a
/// whole ten-pixel square, so a sampled pixel is wholly covered.
#[test]
fn a_stencil_mask_paints_the_fill_colour_s_own_ink() {
    let stencil = |x: u32| {
        let mut content =
            format!("q 10 0 0 10 {x} 30 cm BI /IM true /W 1 /H 1 /BPC 1 ID ").into_bytes();
        content.extend_from_slice(b"\x00 EI Q");
        content
    };
    let mut builder = DocumentBuilder::new();
    builder.add_page(50.0, 40.0, |page| {
        page.raw(b"0.2 0.4 0.6 0.1 k");
        page.raw(&stencil(0));
        page.raw(&stencil(10));
        page.raw(b"0.1 0.9 0.3 0.7 k");
        page.raw(&stencil(20));
        page.raw(b"1 0 0 rg");
        page.raw(&stencil(30));
    });
    let ink = render(
        built("a stencil mask paints the fill colour s own ink", builder),
        &RenderOptions {
            format: PixelFormat::CmykA8,
            allow_cmyk: true,
            ..RenderOptions::default()
        },
    );
    let at = |x: u32, y: u32| -> [u8; 5] {
        let i = y as usize * ink.stride + x as usize * 5;
        ink.data[i..i + 5].try_into().expect("five bytes")
    };
    let byte = |v: f64| (v * 255.0_f64).round() as u8;
    let first = [byte(0.2), byte(0.4), byte(0.6), byte(0.1), 255];
    assert_eq!(at(5, 5), first, "a stencil in `k`'s ink");
    assert_eq!(at(15, 5), first, "the one beside it, in the same run");
    assert_eq!(
        at(25, 5),
        [byte(0.1), byte(0.9), byte(0.3), byte(0.7), 255],
        "a stencil of another ink, not the run's"
    );
    assert_eq!(at(35, 5), [0, 255, 255, 0, 255], "light still converts");
}

/// **A pattern or a group keeps a DeviceCMYK colour's ink where it is drawn
/// on ink, and nowhere else.** A coloured tiling pattern's cell (8.7.3.3's
/// `/PaintType 1`) is drawn into a buffer of the page's own format and
/// composited component for component, so a `k` inside it reaches the page
/// as its components, exactly as on the page, and an `rg` inside it as its
/// light turned to ink. An uncoloured one (`/PaintType 2`) paints in the
/// colour its `scn` operands gave, which is light even over a DeviceCMYK
/// base: `1 1 1 1` arrives as the pure K of the same shade. A form is drawn
/// on the page; a group that names `/CS /DeviceRGB` composites in light
/// (11.6.6), so its rich black is pure K too.
///
/// Until the third review of lane 8A, `RenderOptions::allow_cmyk`'s doc and
/// `rendering.md` said a pattern's cell was light, which was true of the
/// uncoloured pattern only, and nothing here held either case.
#[test]
fn a_pattern_or_group_keeps_a_device_cmyk_colour_s_ink_where_it_is_drawn_on_ink() {
    // A coloured cell: rich black below, red above, ten by ten.
    let coloured = "1 1 1 1 k 0 0 10 5 re f 1 0 0 rg 0 5 10 5 re f";
    // An uncoloured cell: a shape, in whatever colour `scn` gives it.
    let uncoloured = "0 0 10 10 re f";
    let isolated_rgb = "1 1 1 1 k 40 0 10 20 re f";
    let plain_form = "1 1 1 1 k 50 0 10 20 re f";
    let content = "/Pattern cs /P0 scn 0 0 20 20 re f \
                   /Cs1 cs 1 1 1 1 /P1 scn 20 0 20 20 re f \
                   /Fm0 Do /Fm1 Do";
    let cell = |keys: &str, body: &str| {
        format!(
            "<< /Type /Pattern /PatternType 1 /TilingType 1 /BBox [0 0 10 10] \
             /XStep 10 /YStep 10 /Resources << >> {keys} /Length {} >>\n\
             stream\n{body}\nendstream",
            body.len() + 1
        )
    };
    let form = |keys: &str, body: &str| {
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 60 20] {keys} \
             /Length {} >>\nstream\n{body}\nendstream",
            body.len() + 1
        )
    };
    let bytes = format!(
        "%PDF-1.7\n\
         1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
         2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
         3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 60 20] \
         /Resources << /Pattern << /P0 5 0 R /P1 6 0 R >> \
         /ColorSpace << /Cs1 [/Pattern /DeviceCMYK] >> \
         /XObject << /Fm0 7 0 R /Fm1 8 0 R >> >> /Contents 4 0 R >>\nendobj\n\
         4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
         5 0 obj\n{}\nendobj\n\
         6 0 obj\n{}\nendobj\n\
         7 0 obj\n{}\nendobj\n\
         8 0 obj\n{}\nendobj\n\
         trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 1,
        cell("/PaintType 1", coloured),
        cell("/PaintType 2", uncoloured),
        form(
            "/Group << /S /Transparency /I true /CS /DeviceRGB >>",
            isolated_rgb
        ),
        form("", plain_form),
    );
    let ink = render(
        bytes.into_bytes(),
        &RenderOptions {
            format: PixelFormat::CmykA8,
            allow_cmyk: true,
            ..RenderOptions::default()
        },
    );
    assert_eq!(ink.format, PixelFormat::CmykA8);
    let at = |x: u32, y: u32| -> [u8; 5] {
        let i = y as usize * ink.stride + x as usize * 5;
        ink.data[i..i + 5].try_into().expect("five bytes")
    };
    // Page y 0 to 5 is pixel rows 15 to 19, and y 5 to 10 rows 10 to 14; the
    // lattice repeats ten up, so rows 5 to 9 are the next cell's black.
    for (x, y) in [(5, 17), (15, 17), (5, 7), (15, 7)] {
        assert_eq!(
            at(x, y),
            [255, 255, 255, 255, 255],
            "({x}, {y}): a `k` in a coloured cell is the document's own ink"
        );
    }
    assert_eq!(
        at(5, 12),
        [0, 255, 255, 0, 255],
        "an `rg` in a coloured cell is light turned to ink, as on the page"
    );
    assert_eq!(
        at(30, 10),
        [0, 0, 0, 255, 255],
        "an uncoloured pattern's colour is light, over a DeviceCMYK base too"
    );
    assert_eq!(
        at(45, 10),
        [0, 0, 0, 255, 255],
        "a group in `/DeviceRGB` composites light"
    );
    assert_eq!(
        at(55, 10),
        [255, 255, 255, 255, 255],
        "a form is drawn on the page, and keeps the ink"
    );
}

/// **Over ink, a rich black's edge is darker than over light**, and that is
/// the clause's arithmetic rather than a disagreement. A pixel half covered
/// by `1 1 1 1 k` composites to half of every ink, `128` four times, and
/// 8.6.4.4 makes that `(1 − ½)(1 − ½) = ¼` of white: about 64. Over light
/// the same pixel is half black on white, about 128. Compositing is linear
/// in whichever components the canvas holds and 8.6.4.4 is a product, so
/// the two orders part wherever more than one ink is partly there — which
/// is why `to_png` of an ink page is the light render only for pages that
/// never put two inks and black on a partial pixel.
#[test]
fn a_rich_black_edge_composited_over_ink_is_not_the_light_edge() {
    let page = || {
        let mut builder = DocumentBuilder::new();
        builder.add_page(20.0, 20.0, |page| {
            // Half a pixel off the lattice on the left: column 4 is half
            // covered, columns 5 to 13 wholly.
            page.raw(b"1 1 1 1 k 4.5 5 10 10 re f");
        });
        built(
            "a rich black edge composited over ink is not the light edge",
            builder,
        )
    };
    let ink = render(
        page(),
        &RenderOptions {
            format: PixelFormat::CmykA8,
            allow_cmyk: true,
            ..RenderOptions::default()
        },
    );
    let light = render(page(), &RenderOptions::default());
    let row = 10usize;
    let ink_edge: [u8; 5] = ink.data[row * ink.stride + 4 * 5..][..5]
        .try_into()
        .expect("five bytes");
    assert_eq!(ink_edge, [128, 128, 128, 128, 255], "half of every ink");
    let png = Bitmap::from_png(&ink.to_png().expect("a picture")).expect("it reads");
    let png_edge = png.data[row * png.stride + 4 * png.components()];
    let light_edge = light.data[row * light.stride + 4 * light.components()];
    // (1 - 128/255)^2 of 255 is 63.5; 1 - 128/255 of it, 127.
    assert!(
        (63..=64).contains(&png_edge),
        "over ink: a quarter of white, {png_edge}"
    );
    assert!(
        (127..=128).contains(&light_edge),
        "over light: half of white, {light_edge}"
    );
    // Wholly covered, the two agree: all four inks are black either way.
    let png_inside = png.data[row * png.stride + 8 * png.components()];
    let light_inside = light.data[row * light.stride + 8 * light.components()];
    assert_eq!((png_inside, light_inside), (0, 0));
}

/// Without the opt-in, ink is still light — which is what every caller that
/// asked for `CmykA8` before this switch existed got, and still gets.
#[test]
fn without_the_opt_in_a_page_asked_for_in_ink_is_still_light() {
    let light = render(
        ink_page(),
        &RenderOptions {
            format: PixelFormat::CmykA8,
            ..RenderOptions::default()
        },
    );
    assert_eq!(light.format, PixelFormat::Rgba8);
}

/// The opt-in changes nothing for any format but ink.
#[test]
fn the_opt_in_changes_nothing_for_a_format_that_is_not_ink() {
    for format in [
        PixelFormat::Gray8,
        PixelFormat::GrayA8,
        PixelFormat::Rgb8,
        PixelFormat::Rgba8,
        PixelFormat::LabA8,
    ] {
        let plain = render(
            ink_page(),
            &RenderOptions {
                format,
                ..RenderOptions::default()
            },
        );
        let opted = render(
            ink_page(),
            &RenderOptions {
                format,
                allow_cmyk: true,
                ..RenderOptions::default()
            },
        );
        assert_eq!(opted.format, plain.format, "{format:?}");
        assert_eq!(opted.data, plain.data, "{format:?}");
    }
}

/// **Plates are not a page format**: a page asked for in
/// `PixelFormat::Plates` comes back in `Rgba8`, with or without the ink
/// opt-in, whose bytes are the ink page's light — what a `CmykA8` request
/// without the opt-in returns — however many spots the format names, since a
/// page whose paints name none leaves every spot channel without ink.
#[test]
fn a_page_asked_for_in_plates_comes_back_as_its_light() {
    let light = render(
        ink_page(),
        &RenderOptions {
            format: PixelFormat::CmykA8,
            ..RenderOptions::default()
        },
    );
    assert_eq!(light.format, PixelFormat::Rgba8);
    for spots in [0, 2, tinker_pdf::MAX_SPOT_PLATES] {
        let format = PixelFormat::Plates {
            spots: tinker_pdf::Spots::new(spots).expect("under the cap"),
        };
        for allow_cmyk in [false, true] {
            let plates = render(
                ink_page(),
                &RenderOptions {
                    format,
                    allow_cmyk,
                    ..RenderOptions::default()
                },
            );
            assert_eq!(plates.format, PixelFormat::Rgba8, "{spots} spots");
            assert_eq!(
                plates.data, light.data,
                "{spots} spots, opt-in {allow_cmyk}"
            );
        }
    }
}

/// `to_png` writes an ink page as the light it stands for, and that light is
/// byte for byte what the same render returns without the switch: one
/// conversion, 8.6.4.4's, reached two ways.
///
/// For these two pages, whose every partly covered or blended pixel is one
/// ink and black. Not for every page since ink pages carry the document's own
/// components: see
/// `a_rich_black_edge_composited_over_ink_is_not_the_light_edge`.
#[test]
fn an_ink_page_written_as_png_is_the_light_the_switch_would_have_returned() {
    for page in [ink_page(), blend_grid_page()] {
        let ink = render(
            page.clone(),
            &RenderOptions {
                format: PixelFormat::CmykA8,
                allow_cmyk: true,
                ..RenderOptions::default()
            },
        );
        let light = render(
            page,
            &RenderOptions {
                format: PixelFormat::CmykA8,
                ..RenderOptions::default()
            },
        );
        let read = Bitmap::from_png(&ink.to_png().expect("a picture")).expect("it reads");
        assert_eq!(read.format, PixelFormat::Rgba8, "PNG has no CMYK");
        assert_eq!(read.data, light.data);
    }
}

/// **The fingerprint.** Two pages in ink: the blend grid, whose twelve modes
/// are 11.3.5's formulas over complemented components here rather than over
/// light, and the ink page.
#[test]
fn ink_output_is_pinned() {
    let options = RenderOptions {
        format: PixelFormat::CmykA8,
        allow_cmyk: true,
        ..RenderOptions::default()
    };
    // 1 656 of 1 728 pixels and 1 562 of 4 608 today; each floor is about
    // half, as `determinism.rs`'s are.
    pinned(
        "blend grid, in ink",
        &render(blend_grid_page(), &options),
        800,
        "a8c768d45da8dbf2a9f9457f6c10e3f112576def13c28502a3a0b5fa6c03a418",
    );
    // Moved in October 2026, deliberately, when DeviceCMYK components began
    // to reach an ink page: the 400 pixels of the rich-black square (x 28 to
    // 47, rows 24 to 43) went from pure K to all four inks — 120 of them
    // `0 0 0 255` to `255 255 255 255`, and the 280 under the half-alpha
    // `/Multiply` magenta band `0 128 0 255` to the same, since 11.3.5's
    // multiply over complements keeps a full ink full — and no other byte.
    // It was "cd3c60cc88514aaa0ac5a0ce2eea1d7b26cc8b0d6412f2c9fc824c0a42e8788b".
    pinned(
        "ink page, in ink",
        &render(ink_page(), &options),
        780,
        "d8ca10789bb0cec54eb2d16e5b9d43432be855235b5a56e1137148c1f72afd75",
    );
}

/// **The default is unchanged.** The blend grid rendered with the default
/// options is still `determinism.rs`'s `analytic_blend`; the hard-edge page's
/// default render — every kind of edge this file hardens, anti-aliased — is
/// pinned beside it; and every switch spelled out at its default renders the
/// same bytes as not mentioning it.
///
/// The twenty fingerprints in `determinism.rs` are the rest of this claim,
/// and the stronger half: they render with the defaults and did not move.
#[test]
fn the_default_is_unchanged() {
    let spelled = RenderOptions {
        allow_cmyk: false,
        antialias: true,
        graphics_bits: 8,
        text_bits: 8,
        hard_edge: HardEdge::Half,
        thinnest_line: None,
        transparent: false,
        premultiplied: false,
        forced_colours: None,
        ..RenderOptions::default()
    };
    for (name, page, least, want) in [
        ("blend grid", blend_grid_page(), 800, ANALYTIC_BLEND_DEFAULT),
        (
            "hard-edge page",
            hard_edge_page(),
            1100,
            "03dbe46995a5a1bdf69376e57a5e67de9ea24c8be8f3d19b41b2265bb679812c",
        ),
    ] {
        let default = render(page.clone(), &RenderOptions::default());
        pinned(&format!("{name}, default"), &default, least, want);
        let explicit = render(page, &spelled);
        assert_eq!(explicit.data, default.data, "{name}");
        assert_eq!(explicit.format, PixelFormat::Rgb8, "{name}");
    }
}

// ---- the blue-first output orders --------------------------------------------

/// `pixels`, `from` components a pixel, as `to` components blue first: red
/// and blue exchanged, alpha kept when `to` has room for it and dropped when
/// it has not.
fn blue_first(pixels: &[u8], from: usize, to: usize) -> Vec<u8> {
    pixels
        .chunks_exact(from)
        .flat_map(|pixel| {
            let mut out = vec![pixel[2], pixel[1], pixel[0]];
            out.extend_from_slice(&pixel[3..to]);
            out
        })
        .collect()
}

/// **`Bgr8` and `Bgra8` are the `Rgba8` render reordered**, byte for byte:
/// on the blend grid — twelve modes, four of them non-separable, whose
/// formulas see the channel order — and on a text page whose grey text and
/// translucent shape put partial pixels everywhere, with `transparent` and
/// `premultiplied` each on and off. `Bgra8` is the `Rgba8` render under the
/// same options with red and blue exchanged; `Bgr8` has no alpha, so like
/// `Rgb8` it is on white whatever `transparent` says, and it is the opaque
/// `Rgba8` render reordered with its alpha dropped. `to_png` of each writes the
/// `Rgb8` or `Rgba8` render's file, and a display list's render is the page's.
#[test]
fn bgr_and_bgra_are_the_rgba8_render_reordered() {
    for (name, page) in [
        ("blend grid", blend_grid_page()),
        ("text page", layer_page()),
    ] {
        let doc = Document::open(page).expect("the page opens");
        let page = doc.page(0).expect("a page");
        let list = page.display_list();
        for transparent in [false, true] {
            for premultiplied in [false, true] {
                let options = |format: PixelFormat| RenderOptions {
                    format,
                    transparent,
                    premultiplied,
                    ..RenderOptions::default()
                };
                let what =
                    format!("{name}, transparent {transparent}, premultiplied {premultiplied}");
                let rgba = page.render(&options(PixelFormat::Rgba8));
                let opaque = page.render(&RenderOptions {
                    format: PixelFormat::Rgba8,
                    ..RenderOptions::default()
                });
                let rgb = page.render(&options(PixelFormat::Rgb8));

                let bgra = page.render(&options(PixelFormat::Bgra8));
                assert_eq!(bgra.format, PixelFormat::Bgra8, "{what}");
                assert_eq!(
                    (bgra.width, bgra.height),
                    (rgba.width, rgba.height),
                    "{what}"
                );
                assert_eq!(bgra.premultiplied, rgba.premultiplied, "{what}");
                assert_eq!(bgra.data, blue_first(&rgba.data, 4, 4), "{what}: Bgra8");
                assert_eq!(bgra.to_png(), rgba.to_png(), "{what}: Bgra8's file");

                let bgr = page.render(&options(PixelFormat::Bgr8));
                assert_eq!(bgr.format, PixelFormat::Bgr8, "{what}");
                assert!(
                    !bgr.premultiplied,
                    "{what}: Bgr8 has no alpha to multiply by"
                );
                assert_eq!(bgr.data, blue_first(&opaque.data, 4, 3), "{what}: Bgr8");
                assert_eq!(
                    bgr.data,
                    blue_first(&rgb.data, 3, 3),
                    "{what}: Bgr8 against Rgb8"
                );
                assert_eq!(bgr.to_png(), rgb.to_png(), "{what}: Bgr8's file");

                for format in [PixelFormat::Bgr8, PixelFormat::Bgra8] {
                    assert_eq!(
                        list.render(&options(format)).data,
                        page.render(&options(format)).data,
                        "{what}: a display list's {format:?} is the page's"
                    );
                }
            }
        }
        // The orders really are different bytes: a page with colour on it is
        // not its own reordering.
        let rgb = page.render(&RenderOptions::default());
        let bgr = page.render(&RenderOptions {
            format: PixelFormat::Bgr8,
            ..RenderOptions::default()
        });
        assert_ne!(bgr.data, rgb.data, "{name}: the order did not change");
    }
}

// ---- a page with nothing under it, and premultiplied alpha -------------------

/// One pixel of a four- or two-component bitmap.
fn pixel(bitmap: &Bitmap, x: u32, y: u32) -> &[u8] {
    let n = bitmap.components();
    let at = y as usize * bitmap.stride + x as usize * n;
    &bitmap.data[at..at + n]
}

/// A page for premultiplication to be *arithmetic* on: text in a grey and a
/// curved shape in an orange at 37 % opacity, neither of them a pure colour.
///
/// The ink page's colours are all 0 or 255 in every channel, and `c·a/255` of
/// those is `0` or `a` exactly — so on it a premultiplication that truncated
/// instead of rounding came out byte-identical, and a campaign injecting that
/// defect was caught only by a pinned hash. Here every anti-aliased edge and
/// every pixel of the translucent shape rounds.
fn layer_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    assert!(builder.add_ext_gstate(
        b"G0",
        &ExtGState {
            fill_alpha: Some(0.37),
            ..ExtGState::default()
        }
    ));
    builder.add_page(96.0, 48.0, |page| {
        page.raw(b"0.3 g");
        page.text(b"F0", 15.0, 3.5, 26.0, "0123 fox");
        page.raw(b"q");
        assert!(page.set_ext_gstate(b"G0"));
        page.raw(b"0.6 0.3 0.1 rg 40 4 m 80 2 90 30 60 22 c 45 30 l h f");
        page.raw(b"Q");
    });
    built("layer page", builder)
}

fn layer(format: PixelFormat, premultiplied: bool) -> RenderOptions {
    RenderOptions {
        format,
        transparent: true,
        premultiplied,
        ..RenderOptions::default()
    }
}

/// With `transparent`, nothing painted is nothing: `(0, 0, 0, 0)`. What is
/// painted opaquely is its colour at full alpha, and the half-opaque
/// `/Multiply` band where it lies over nothing is its own colour at half
/// alpha — 11.3.6 with a backdrop alpha of zero, where the blend mode has
/// nothing to blend with.
#[test]
fn a_transparent_page_is_nothing_where_nothing_is_painted() {
    let page = render(ink_page(), &layer(PixelFormat::Rgba8, false));
    assert_eq!(page.format, PixelFormat::Rgba8);
    assert!(!page.premultiplied);
    assert_eq!(pixel(&page, 1, 1), [0, 0, 0, 0], "nothing is transparent");
    assert_eq!(pixel(&page, 14, 40), [0, 255, 255, 255], "cyan, opaque");
    // x 25 is in the gap between the first two squares, y 31 inside the band.
    assert_eq!(
        pixel(&page, 25, 31),
        [255, 0, 255, 128],
        "magenta at the band's own alpha, over nothing"
    );

    let grey = render(ink_page(), &layer(PixelFormat::GrayA8, false));
    assert_eq!(pixel(&grey, 1, 1), [0, 0]);
    assert_eq!(pixel(&grey, 62, 40), [0, 255], "K is black, opaque");
}

/// A format without alpha cannot hold nothing, so `transparent` changes no
/// byte of it — a page asked for in `Rgb8` is on white, as it always was.
#[test]
fn a_format_without_alpha_is_on_white_whatever_it_is_asked() {
    for format in [PixelFormat::Gray8, PixelFormat::Rgb8] {
        let plain = render(
            ink_page(),
            &RenderOptions {
                format,
                ..RenderOptions::default()
            },
        );
        let asked = render(ink_page(), &layer(format, true));
        assert_eq!(asked.data, plain.data, "{format:?}");
        assert!(
            !asked.premultiplied,
            "{format:?} has no alpha to multiply by"
        );
    }
}

/// **Premultiplied is straight times alpha**, at every pixel of every format
/// with alpha, computed here in floating point from the straight render rather
/// than by the code under test — on [`layer_page`], whose partial alpha falls
/// on colours that are not 0 or 255, so the rounding is exercised and not only
/// the multiplication.
#[test]
fn premultiplied_is_straight_colour_times_alpha() {
    for format in [PixelFormat::GrayA8, PixelFormat::Rgba8] {
        let straight = render(layer_page(), &layer(format, false));
        let pre = render(layer_page(), &layer(format, true));
        assert!(pre.premultiplied && !straight.premultiplied);
        let n = format.components();
        let mut partial = 0;
        for (s, p) in straight.data.chunks_exact(n).zip(pre.data.chunks_exact(n)) {
            let a = s[n - 1];
            assert_eq!(p[n - 1], a, "the alpha itself is untouched");
            if a != 0 && a != 255 {
                partial += 1;
            }
            for (c, got) in s[..n - 1].iter().zip(&p[..n - 1]) {
                let want = (f64::from(*c) * f64::from(a) / 255.0).round() as u8;
                assert_eq!(*got, want, "{format:?}: {c} at alpha {a}");
            }
        }
        assert!(
            partial > 150,
            "{format:?}: only {partial} partly transparent pixels, too few to \
             say anything"
        );
    }
}

/// On a page over white every pixel is opaque, and an opaque pixel is the
/// same in both conventions: the switch changes no byte, and says it applied.
#[test]
fn premultiplied_changes_no_byte_of_an_opaque_page() {
    let straight = render(
        ink_page(),
        &RenderOptions {
            format: PixelFormat::Rgba8,
            ..RenderOptions::default()
        },
    );
    let pre = render(
        ink_page(),
        &RenderOptions {
            format: PixelFormat::Rgba8,
            premultiplied: true,
            ..RenderOptions::default()
        },
    );
    assert_eq!(pre.data, straight.data);
    assert!(pre.premultiplied);
}

/// `to_png` writes straight alpha, which is the only kind PNG has, and the
/// round trip loses exactly what premultiplying lost and nothing else:
/// multiplying the PNG's samples by their alpha again gives back the
/// premultiplied bytes, every one of them.
#[test]
fn a_premultiplied_page_writes_a_png_that_multiplies_back_exactly() {
    for format in [PixelFormat::GrayA8, PixelFormat::Rgba8] {
        let pre = render(layer_page(), &layer(format, true));
        let read = Bitmap::from_png(&pre.to_png().expect("a picture")).expect("it reads");
        assert!(!read.premultiplied, "PNG's alpha is straight");
        let n = format.components();
        for (r, p) in read.data.chunks_exact(n).zip(pre.data.chunks_exact(n)) {
            let a = r[n - 1];
            assert_eq!(a, p[n - 1]);
            for (c, want) in r[..n - 1].iter().zip(&p[..n - 1]) {
                let again = (f64::from(*c) * f64::from(a) / 255.0).round() as u8;
                assert_eq!(again, *want, "{format:?}: {c} at alpha {a}");
            }
        }
    }
}

/// Ink is premultiplied like light: the band's magenta over nothing is half
/// its ink at half its alpha.
#[test]
fn ink_is_premultiplied_too() {
    let ink = render(
        ink_page(),
        &RenderOptions {
            format: PixelFormat::CmykA8,
            allow_cmyk: true,
            transparent: true,
            premultiplied: true,
            ..RenderOptions::default()
        },
    );
    assert_eq!(ink.format, PixelFormat::CmykA8);
    assert!(ink.premultiplied);
    assert_eq!(pixel(&ink, 1, 1), [0, 0, 0, 0, 0]);
    assert_eq!(pixel(&ink, 25, 31), [0, 128, 0, 0, 128]);
}

/// Ruling 5 holds for a page with nothing under it, premultiplied: a tile is
/// the page under it, byte for byte.
#[test]
fn a_transparent_premultiplied_tile_is_the_page_under_it() {
    let doc = Document::open(ink_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let options = layer(PixelFormat::Rgba8, true);
    let whole = page.render(&options);
    for (x, y, w, h) in [(0, 0, 37, 23), (37, 23, 37, 25), (60, 11, 36, 30)] {
        let tile = page.render(&RenderOptions {
            region: Some(tinker_pdf::PixelRegion::new(x, y, w, h)),
            ..options.clone()
        });
        assert!(tile.premultiplied);
        for row in 0..h {
            let from = (y + row) as usize * whole.stride + x as usize * 4;
            let want = &whole.data[from..from + w as usize * 4];
            let got = &tile.data[row as usize * tile.stride..][..w as usize * 4];
            assert_eq!(got, want, "tile ({x}, {y}) row {row}");
        }
    }
}

/// **The fingerprint.** The ink page as a layer, straight and premultiplied,
/// and the hard-edge page's anti-aliased edges as a premultiplied layer, where
/// every partial pixel is partial in alpha rather than in colour.
#[test]
fn transparent_and_premultiplied_output_is_pinned() {
    // 1 562 and 2 220 pixels painted today; the floors are about half.
    pinned(
        "ink page as a layer, straight",
        &render(ink_page(), &layer(PixelFormat::Rgba8, false)),
        780,
        "dc8ad1fb83b335e6dae243b9c4deca910e6c0fe8d1ed709585542b7aaff9ad30",
    );
    pinned(
        "ink page as a layer, premultiplied",
        &render(ink_page(), &layer(PixelFormat::Rgba8, true)),
        780,
        "732b177c00dda1fb2755b569a6f27225bd7e5cead07bc58f164f227cfb013b31",
    );
    pinned(
        "hard-edge page as a layer, premultiplied",
        &render(hard_edge_page(), &layer(PixelFormat::Rgba8, true)),
        1100,
        "30569c270e26bb77f4597fbb04e7f9239554a05d6bb2132da6f9ffaa081cbce8",
    );
}

// ---- the anti-aliasing switch -------------------------------------------------

/// The image the hard-edge page places: two by two samples of one green, so
/// every pixel it covers is exactly that green whatever the sampler does
/// inside it, and a pixel of any other colour is an edge.
const GREEN: [u8; 3] = [0, 160, 40];

/// Text, a diagonal fill, a stroked curve, a hairline and a rotated image's
/// edge, each in its own opaque colour and none touching another — the
/// fixture the switch's exit criterion names.
///
/// Every colour is pure and every element opaque, so on a hard-edged render a
/// pixel has exactly six possible values: the page's white or one element's
/// colour. Anything else is a pixel something covered partly.
fn hard_edge_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    let samples = GREEN.repeat(4);
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 2,
            height: 2,
            data: &samples,
        }
    ));
    builder.add_page(160.0, 100.0, |page| {
        // Text, in black: glyph coverage, the densest source of partial
        // pixels there is.
        page.text(b"F0", 14.0, 4.0, 78.0, "fox 0123");
        // A diagonal fill, in red. Closed with `h` rather than left open:
        // this engine does not yet close an open subpath before filling or
        // clipping (8.5.3.3), and a fixture about edges should not lean on it.
        page.raw(b"1 0 0 rg 100 45 m 150 75 l 140 40 l h f");
        // A stroked curve, in blue.
        page.raw(b"0 0 1 RG 2.3 w 10 10 m 30 45 60 5 80 30 c S");
        // A hairline at a shallow angle, in black: the thinnest stroke there
        // is, which a hard edge must not lose.
        page.raw(b"0 0 0 RG 0 w 5 97 m 155 94 l S");
        // An image's edge, rotated so none of it is axis-aligned, in green.
        page.raw(b"q 20 12 -12 20 95 5 cm /Im0 Do Q");
    });
    built("hard edge page", builder)
}

/// The colours a hard-edged render of [`hard_edge_page`] may hold.
const HARD_COLOURS: [[u8; 3]; 5] = [[255, 255, 255], [0, 0, 0], [255, 0, 0], [0, 0, 255], GREEN];

/// Pixels whose colour is not one of `HARD_COLOURS` — each one a pixel
/// something covered partly.
fn partial_pixels(bitmap: &Bitmap) -> Vec<(u32, u32, [u8; 3])> {
    let mut out = Vec::new();
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let i = y as usize * bitmap.stride + x as usize * 3;
            let pixel = [bitmap.data[i], bitmap.data[i + 1], bitmap.data[i + 2]];
            if !HARD_COLOURS.contains(&pixel) {
                out.push((x, y, pixel));
            }
        }
    }
    out
}

fn count(bitmap: &Bitmap, colour: [u8; 3]) -> usize {
    bitmap
        .data
        .chunks_exact(3)
        .filter(|pixel| **pixel == colour)
        .count()
}

/// **The exit criterion.** Every pixel of an anti-aliasing-off render of text,
/// a diagonal fill, a stroked curve, a hairline and an image edge is fully
/// covered or fully uncovered — and the same page with anti-aliasing on is
/// not, so the fixture has edges in it to harden.
///
/// Each element is also required to be *present*, by its own colour, so that
/// a switch that hardened by painting nothing would fail here rather than
/// pass: an empty page is fully uncovered everywhere.
#[test]
fn with_anti_aliasing_off_every_pixel_is_wholly_covered_or_not_at_all() {
    for scale in [1.0, 1.7, 3.0] {
        let soft = render(
            hard_edge_page(),
            &RenderOptions {
                scale,
                ..RenderOptions::default()
            },
        );
        let hard = render(
            hard_edge_page(),
            &RenderOptions {
                scale,
                antialias: false,
                ..RenderOptions::default()
            },
        );
        assert!(
            soft.warnings.is_empty() && hard.warnings.is_empty(),
            "{:?} {:?}",
            soft.warnings,
            hard.warnings
        );
        assert!(
            partial_pixels(&soft).len() > 100,
            "at {scale}x the anti-aliased page has partial pixels to take away"
        );
        let partial = partial_pixels(&hard);
        assert!(
            partial.is_empty(),
            "at {scale}x, {} pixels of the hard-edged render are partly \
             covered; the first few: {:?}",
            partial.len(),
            &partial[..partial.len().min(8)]
        );
        // Each element painted, by its own colour. The floors are about half
        // of what each paints at 1x, scaled by area.
        let area = scale * scale;
        for (what, colour, least) in [
            ("text and hairline", [0, 0, 0], 180.0),
            ("the diagonal fill", [255, 0, 0], 300.0),
            ("the stroked curve", [0, 0, 255], 90.0),
            ("the image", GREEN, 270.0),
        ] {
            let painted = count(&hard, colour);
            assert!(
                painted as f64 >= least * area,
                "at {scale}x {what} painted {painted} pixels"
            );
        }
    }
}

/// The hairline survives: a zero-width stroke is at least a pixel wide with
/// anti-aliasing off, so a shallow line across the page leaves no column it
/// crosses empty — which a line eight tenths of a pixel wide, split across two
/// rows, would.
#[test]
fn a_hard_edged_hairline_leaves_no_column_it_crosses_empty() {
    // Slopes from level to one in one, each starting on a different phase.
    for (index, rise) in [0.0, 0.37, 1.5, 3.0, 11.0, 37.0, 60.0, 99.5]
        .iter()
        .enumerate()
    {
        let y0 = 20.3 + index as f64 * 0.13;
        let content = format!("0 w 10 {y0} m 110 {} l S", y0 + rise);
        let page = format!(
            "%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
             2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
             3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 140] \
             /Contents 4 0 R >>\nendobj\n\
             4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
             trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
            content.len()
        );
        let hard = render(
            page.into_bytes(),
            &RenderOptions {
                antialias: false,
                ..RenderOptions::default()
            },
        );
        for x in 11..109u32 {
            let inked = (0..hard.height).any(|y| {
                let i = y as usize * hard.stride + x as usize * 3;
                hard.data[i] == 0
            });
            assert!(inked, "rise {rise}: column {x} of the hairline is empty");
        }
    }
}

/// Ruling 5 holds with the switch off: a tile of a hard-edged page is the
/// hard-edged page under it, byte for byte, because the threshold is a
/// function of one pixel's coverage and the coverage already agreed.
#[test]
fn a_hard_edged_tile_is_the_hard_edged_page_under_it() {
    let doc = Document::open(hard_edge_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let options = RenderOptions {
        antialias: false,
        ..RenderOptions::default()
    };
    let whole = page.render(&options);
    for (x, y, w, h) in [
        (0, 0, 37, 23),
        (37, 23, 37, 23),
        (90, 60, 70, 40),
        (3, 71, 64, 29),
    ] {
        let tile = page.render(&RenderOptions {
            region: Some(tinker_pdf::PixelRegion::new(x, y, w, h)),
            ..options.clone()
        });
        for row in 0..h {
            let from = (y + row) as usize * whole.stride + x as usize * 3;
            let want = &whole.data[from..from + w as usize * 3];
            let got = &tile.data[row as usize * tile.stride..][..w as usize * 3];
            assert_eq!(got, want, "tile ({x}, {y}) row {row}");
        }
    }
}

/// The shading paths and the tiling one, each in one flat colour so that any
/// pixel of any other colour is an edge: a type 4 mesh whose vertices all
/// carry the same colour, `sh` of an axial shading whose two ends agree
/// painted through a diagonal clip, the same shading as a pattern filling a
/// curved shape, and a tiling pattern whose cell is a diagonal band.
///
/// A mesh is rasterised by its own path (`draw_mesh`, one coverage buffer
/// for the whole mesh), `sh` paints whatever the clip covers, a shading
/// pattern paints whatever its shape covers, and a tiling cell is drawn by a
/// renderer of its own before it is composited — four different places a
/// coverage value is produced, which is why each gets a flat colour here
/// rather than the gradient that would hide an edge inside a ramp.
fn hard_edge_shadings_page() -> Vec<u8> {
    shadings_page(
        "q 90 10 m 150 30 l 110 60 l h W n /Sh1 sh Q\n\
         /Pattern cs /P0 scn 95 75 m 115 99 145 90 150 70 c 120 62 l h f\n\
         /P1 scn 5 65 65 30 re f",
    )
}

/// [`hard_edge_shadings_page`]'s document drawing `content` after the mesh,
/// with its shadings `/Sh0` (the mesh) and `/Sh1` and its patterns `/P0` (a
/// shading) and `/P1` (a tiling cell) in its resources.
fn shadings_page(content: &str) -> Vec<u8> {
    // One vertex a line: flag, x, y, r, g, b, eight bits each, so the stream
    // is byte-aligned and needs no packing. `/Decode` maps the coordinates
    // onto the page and the colour straight through.
    let vertices: [(u8, u8); 3] = [(15, 20), (110, 45), (45, 150)];
    let mut mesh = String::new();
    for (x, y) in vertices {
        for byte in [0u8, x, y, 0, 128, 255] {
            mesh.push_str(&format!("{byte:02x}"));
        }
    }
    mesh.push('>');
    let axial = "/ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 160 100] \
                 /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0.5 0] \
                 /C1 [1 0.5 0] /N 1 >>";
    let cell = "0.5 0 0.5 rg 0 0 m 10 6 l 10 10 l 4 10 l h f";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 160 100]\n\
   /Resources << /Shading << /Sh0 5 0 R /Sh1 << {axial} >> >>\n\
   /Pattern << /P0 << /PatternType 2 /Shading << {axial} >> >> /P1 6 0 R >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n/Sh0 sh\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 \
/BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 160 0 100 0 1 0 1 0 1] \
/Filter /ASCIIHexDecode /Length {} >>\nstream\n{mesh}\nendstream\nendobj\n\
6 0 obj\n<< /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 \
/YStep 10 /Resources << >> /Length {} >>\nstream\n{cell}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 8,
        mesh.len() + 1,
        cell.len() + 1,
    )
    .into_bytes()
}

/// The distinct colours a render holds, with how many pixels each.
fn palette(bitmap: &Bitmap) -> Vec<([u8; 3], usize)> {
    let mut out: Vec<([u8; 3], usize)> = Vec::new();
    for pixel in bitmap.data.chunks_exact(3) {
        let colour = [pixel[0], pixel[1], pixel[2]];
        match out.iter_mut().find(|(c, _)| *c == colour) {
            Some((_, n)) => *n += 1,
            None => out.push((colour, 1)),
        }
    }
    out
}

/// **The switch reaches every shading path, and the tiling one**: with
/// anti-aliasing off, the mesh, the clipped `sh`, the shading pattern and the
/// tiling pattern leave exactly four colours on the page — the white and three
/// flat colours — where the same page anti-aliased has dozens along the
/// silhouettes and inside the tiling cells.
#[test]
fn with_anti_aliasing_off_a_shading_is_wholly_covered_or_not_at_all() {
    for scale in [1.0, 2.3] {
        let soft = render(
            hard_edge_shadings_page(),
            &RenderOptions {
                scale,
                ..RenderOptions::default()
            },
        );
        let hard = render(
            hard_edge_shadings_page(),
            &RenderOptions {
                scale,
                antialias: false,
                ..RenderOptions::default()
            },
        );
        assert!(
            soft.warnings.is_empty() && hard.warnings.is_empty(),
            "{:?} {:?}",
            soft.warnings,
            hard.warnings
        );
        assert!(
            palette(&soft).len() > 20,
            "at {scale}x the anti-aliased silhouettes have partial pixels"
        );
        let colours = palette(&hard);
        assert_eq!(
            colours.len(),
            4,
            "at {scale}x a hard-edged page of three flat paints holds white \
             and three colours, not {colours:?}"
        );
        // Every paint, and each with real area behind it: at 1x the mesh is
        // 1 416 pixels, the clip and the shading pattern together 2 274 and
        // the tiling pattern 1 002, and each floor is about half.
        let area = scale * scale;
        for (colour, least) in [
            ([0u8, 128, 255], 700.0),
            ([255, 128, 0], 1100.0),
            ([128, 0, 128], 500.0),
        ] {
            let painted = colours
                .iter()
                .find(|(c, _)| *c == colour)
                .map_or(0, |(_, n)| *n);
            assert!(
                painted as f64 >= least * area,
                "at {scale}x {colour:?} painted {painted} pixels: {colours:?}"
            );
        }
    }
}

/// **The fingerprint.** The hard-edge page with anti-aliasing off, at 1x and
/// at 1.7x, where every edge lands on a different phase of the pixel grid.
#[test]
fn hard_edged_output_is_pinned() {
    // 1 780 and 4 983 pixels painted today; the floors are about half.
    for (scale, least, want) in [
        (
            1.0,
            890,
            "f407710585c1a5218877c3fb1f8689204f24d8835340c72ef5f313ea874c9561",
        ),
        (
            1.7,
            2490,
            "39d5aba7acb9e32ea92f05d49797e42499118e63cd66e9a7a6a19152b222401c",
        ),
    ] {
        pinned(
            &format!("hard-edge page at {scale}x, anti-aliasing off"),
            &render(
                hard_edge_page(),
                &RenderOptions {
                    scale,
                    antialias: false,
                    ..RenderOptions::default()
                },
            ),
            least,
            want,
        );
    }
}

// ---- forced colours ----------------------------------------------------------

/// Text in an embedded face, a translucent `/Multiply` fill and dashed
/// stroke, a turned image, a knockout group declaring ink and a fill under a
/// luminosity soft mask that shows its left half: everything forced colours
/// do not apply, beside everything they draw by coverage. `dark` draws the
/// page as forced colours should see it — every paint black, opaque and
/// `Normal`, the group's fills drawn straight and the masked one unmasked.
fn forced_page(dark: bool) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    assert!(builder.add_ext_gstate(
        b"Half",
        &ExtGState {
            fill_alpha: Some(0.5),
            stroke_alpha: Some(0.3),
            blend_mode: Some(BlendMode::Multiply),
            ..ExtGState::default()
        }
    ));
    assert!(builder.add_form(
        b"Knock",
        &FormXObject {
            bbox: [96.0, 4.0, 156.0, 56.0],
            matrix: None,
            group: Some(TransparencyGroup {
                color_space: DeviceSpace::Cmyk,
                isolated: true,
                knockout: true,
            }),
            content: b"0.2 0.6 0.9 rg 100 8 40 30 re f 0.9 0.2 0.1 rg 112 20 40 30 re f",
        }
    ));
    assert!(builder.add_form(
        b"Left",
        &FormXObject {
            bbox: [0.0, 0.0, 160.0, 100.0],
            matrix: None,
            group: Some(TransparencyGroup {
                color_space: DeviceSpace::Gray,
                isolated: true,
                knockout: false,
            }),
            content: b"1 g 0 0 80 100 re f",
        }
    ));
    assert!(builder.add_ext_gstate(
        b"Masked",
        &ExtGState {
            soft_mask: Some(StateMask::Group {
                kind: MaskKind::Luminosity,
                form: b"Left",
                backdrop: None,
            }),
            ..ExtGState::default()
        }
    ));
    let samples = [250u8, 20, 20, 20, 200, 40, 30, 30, 220, 240, 240, 60];
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 2,
            height: 2,
            data: &samples,
        }
    ));
    builder.add_page(160.0, 100.0, |page| {
        page.raw(if dark { b"0 g" } else { b"0.8 0.1 0.1 rg" });
        page.text(b"F0", 14.0, 4.0, 78.0, "fox 0123");
        page.raw(b"q");
        if dark {
            page.raw(b"0 g 0 G");
        } else {
            assert!(page.set_ext_gstate(b"Half"));
            page.raw(b"0 0.5 0 rg 0 0 1 RG");
        }
        page.raw(b"10 10 60 30 re f 2 w [4 2] 0 d 10 50 m 70 60 l S Q");
        page.raw(b"q 20 12 -12 20 76 62 cm /Im0 Do Q");
        if dark {
            page.raw(b"0 g 100 8 40 30 re f 112 20 40 30 re f");
        } else {
            assert!(page.form(b"Knock"));
        }
        page.raw(b"q");
        if dark {
            page.raw(b"0 g");
        } else {
            assert!(page.set_ext_gstate(b"Masked"));
            page.raw(b"0.3 0.3 0.3 rg");
        }
        page.raw(b"40 20 100 30 re f Q");
    });
    built("forced page", builder)
}

/// Two squares a Type 3 glyph's procedure fills (9.6.5) — or, `as_page`, the
/// same two squares filled by the page itself — and a filled triangle beside
/// them on the page.
fn type3_page(as_page: bool) -> Vec<u8> {
    let glyph = "1000 0 0 0 1000 1000 d1 100 100 600 600 re f";
    let shown = if as_page {
        "12 12 12 12 re f 32 12 12 12 re f"
    } else {
        "BT /T3 20 Tf 10 10 Td (AA) Tj ET"
    };
    let content = format!("{shown} 60 10 m 90 40 l 60 40 l h f");
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 50] \
/Resources << /Font << /T3 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] \
/FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /sq 6 0 R >> \
/Encoding << /Type /Encoding /Differences [65 /sq] >> /FirstChar 65 \
/LastChar 65 /Widths [1000] /Resources << >> >>\nendobj\n\
6 0 obj\n<< /Length {} >>\nstream\n{glyph}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 1,
        glyph.len() + 1,
    )
    .into_bytes()
}

/// Every page this section draws in forced colours, by name.
fn forced_fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("forced page", forced_page(false)),
        ("shadings page", hard_edge_shadings_page()),
        ("type 3 page", type3_page(false)),
    ]
}

/// Forced colours, black on white, with or without fill-to-stroke.
fn forced(fill_to_stroke: bool) -> ForcedColours {
    ForcedColours {
        fill_to_stroke,
        ..ForcedColours::default()
    }
}

/// `options` drawing in `colours`.
fn forced_options(scale: f64, format: PixelFormat, colours: ForcedColours) -> RenderOptions {
    RenderOptions {
        scale,
        format,
        allow_cmyk: format == PixelFormat::CmykA8,
        forced_colours: Some(colours),
        ..RenderOptions::default()
    }
}

/// Rec.601 luma in thousandths, rounded: `Color::luma`'s closed form, how a
/// grey canvas holds a colour.
fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((u32::from(r) * 299 + u32::from(g) * 587 + u32::from(b) * 114 + 500) / 1000) as u8
}

/// What a forced-colour render in `colours` should hold, computed from the
/// black-on-white render of the same page: every pixel the background plus
/// its coverage times the difference, `(fg (255 - v) + bg v + 127) / 255` a
/// channel for the grey `v`, written as a canvas of `format` is filled, and
/// opaque.
fn tinted(coverage: &Bitmap, colours: ForcedColours, format: PixelFormat) -> Vec<u8> {
    assert_eq!(coverage.format, PixelFormat::Gray8);
    let (fg, bg) = (colours.foreground, colours.background);
    let mut out = Vec::new();
    for y in 0..coverage.height as usize {
        for x in 0..coverage.width as usize {
            let v = u32::from(coverage.data[y * coverage.stride + x]);
            let ramp = |dark: u8, light: u8| {
                ((u32::from(dark) * (255 - v) + u32::from(light) * v + 127) / 255) as u8
            };
            let (r, g, b) = (ramp(fg.r, bg.r), ramp(fg.g, bg.g), ramp(fg.b, bg.b));
            match format {
                PixelFormat::Gray8 => out.push(luma(r, g, b)),
                PixelFormat::GrayA8 => out.extend([luma(r, g, b), 255]),
                PixelFormat::Rgb8 => out.extend([r, g, b]),
                PixelFormat::Rgba8 => out.extend([r, g, b, 255]),
                PixelFormat::Bgr8 => out.extend([b, g, r]),
                PixelFormat::Bgra8 => out.extend([b, g, r, 255]),
                PixelFormat::CmykA8 => {
                    let (c, m, yellow, k) = rgb_to_cmyk(r, g, b);
                    out.extend([c, m, yellow, k, 255]);
                }
                PixelFormat::LabA8 => panic!("Lab is not a page format"),
                PixelFormat::Plates { .. } => panic!("plates are not a page format"),
            }
        }
    }
    out
}

/// The page formats a forced render is asked for in.
const FORCED_FORMATS: [PixelFormat; 7] = [
    PixelFormat::Gray8,
    PixelFormat::GrayA8,
    PixelFormat::Rgb8,
    PixelFormat::Rgba8,
    PixelFormat::Bgr8,
    PixelFormat::Bgra8,
    PixelFormat::CmykA8,
];

/// **The exit criterion: forced colours are coverage over the two colours.**
/// Every pixel of a forced-colour render, in every page format, in colours
/// dark on light, light on dark and two hues, is the background plus its
/// coverage times the difference — coverage read from the black-on-white
/// render of the same page and the tint computed here — at two scales, with
/// and without fill-to-stroke; and that black-on-white render holds black,
/// white and coverage between, so the closed form is held on edges.
#[test]
fn forced_colours_are_coverage_over_the_two_colours() {
    let pairs = [
        (Color::BLACK, Color::WHITE),
        (Color::WHITE, Color::BLACK),
        (Color::rgb(255, 230, 0), Color::rgb(10, 20, 90)),
        (Color::rgb(0, 90, 40), Color::rgb(250, 245, 230)),
    ];
    let mut held = 0usize;
    for (name, bytes) in forced_fixtures() {
        for scale in [1.0, 1.7] {
            for fill_to_stroke in [false, true] {
                let what = format!("{name} at {scale}x, fill-to-stroke {fill_to_stroke}");
                let coverage = render(
                    bytes.clone(),
                    &forced_options(scale, PixelFormat::Gray8, forced(fill_to_stroke)),
                );
                assert!(
                    coverage.data.contains(&0)
                        && coverage.data.contains(&255)
                        && coverage.data.iter().any(|v| (1..255).contains(v)),
                    "{what}: the coverage holds black, white and edges between"
                );
                for (foreground, background) in pairs {
                    let colours = ForcedColours {
                        background,
                        foreground,
                        fill_to_stroke,
                    };
                    for format in FORCED_FORMATS {
                        let got = render(bytes.clone(), &forced_options(scale, format, colours));
                        assert_eq!(
                            (got.width, got.height, got.format, got.stride),
                            (
                                coverage.width,
                                coverage.height,
                                format,
                                coverage.width as usize * format.components()
                            ),
                            "{what}, {format:?}"
                        );
                        assert!(!got.premultiplied);
                        let want = tinted(&coverage, colours, format);
                        if got.data != want {
                            let at = got.data.iter().zip(&want).position(|(a, b)| a != b);
                            panic!(
                                "{what}, {format:?}, {foreground:?} on {background:?}: byte \
                                 {at:?} is not the tint of its coverage"
                            );
                        }
                        held += 1;
                    }
                }
            }
        }
    }
    println!("forced colours: RAN, {held} renders held to the tint of their coverage");
}

/// **What forced colours do not apply.** The black-on-white forced render of
/// a page asking for colour, constant alpha, `/Multiply`, a knockout group in
/// ink and a soft mask is the ordinary grey render of its twin that asks for
/// none of it — every paint black, opaque and `Normal`, the group's fills
/// drawn straight and the masked fill unmasked — byte for byte; the image is
/// its lightness in both. At the scales where every edge the group draws is
/// whole, so its composite has no partial pixel to round.
#[test]
fn forced_colours_apply_no_alpha_blend_mask_knockout_or_group_space() {
    for scale in [1.0, 2.0] {
        let forced_render = render(
            forced_page(false),
            &forced_options(scale, PixelFormat::Gray8, forced(false)),
        );
        let twin = render(
            forced_page(true),
            &RenderOptions {
                scale,
                format: PixelFormat::Gray8,
                ..RenderOptions::default()
            },
        );
        assert_eq!(forced_render.data, twin.data, "at {scale}x");
        // And the twin is not the page drawn as it asks: the mask and the
        // alphas change pixels there.
        let ordinary = render(
            forced_page(false),
            &RenderOptions {
                scale,
                format: PixelFormat::Gray8,
                ..RenderOptions::default()
            },
        );
        assert_ne!(ordinary.data, twin.data, "at {scale}x");
    }
}

/// A shading pattern and a tiling pattern fill paint their shapes black by
/// coverage, as every fill does, and `sh` and a mesh paint their lightness:
/// the forced black-on-white render of the shadings page is the ordinary grey
/// render of the page with its two pattern fills made black fills.
#[test]
fn a_pattern_fill_paints_its_shape_and_a_shading_its_lightness() {
    let twin = shadings_page(
        "q 90 10 m 150 30 l 110 60 l h W n /Sh1 sh Q\n\
         0 g 95 75 m 115 99 145 90 150 70 c 120 62 l h f\n\
         5 65 65 30 re f",
    );
    for scale in [1.0, 1.7] {
        let forced_render = render(
            hard_edge_shadings_page(),
            &forced_options(scale, PixelFormat::Gray8, forced(false)),
        );
        let plain = render(
            twin.clone(),
            &RenderOptions {
                scale,
                format: PixelFormat::Gray8,
                ..RenderOptions::default()
            },
        );
        assert_eq!(forced_render.data, plain.data, "at {scale}x");
        assert!(
            forced_render.warnings.is_empty(),
            "{:?}",
            forced_render.warnings
        );
    }
}

/// `fills` with every `re f`, `f*` and `B` written as `fill` asks, under a
/// wide dashed pen and a clip.
fn fills_page(content: &str) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(120.0, 90.0, |page| {
        page.raw(b"5 w [3 2] 1 d 1 J 1 j");
        page.raw(content.as_bytes());
    });
    built("fills page", builder)
}

/// Each fill of [`fill_to_stroke_draws_each_fill_as_its_stroke`]'s page, as a
/// path and the operator that fills it.
const FILLS: [(&str, &str); 5] = [
    ("10 10 30 20 re", "f"),
    ("50 10 m 80 40 l 60 45 l h", "f"),
    ("90 10 m 110 20 l 95 35 l 100 10 l 115 30 l h", "f*"),
    ("15 50 m 25 85 45 85 55 50 c h", "f"),
    ("70 55 30 25 re", "B"),
];

/// **The exit criterion: fill-to-stroke draws each fill as its stroke.** A
/// page of fills in a colour under a wide dashed pen with round caps and
/// joins — a rectangle, a triangle, an even-odd star, a curve, a
/// fill-and-stroke and one under a clip — renders in forced colours with
/// fill-to-stroke byte for byte as its twin that strokes the same paths with
/// `0 w [] 0 d`, in forced colours, at scales a hairline lands on
/// differently, anti-aliased and not; and differs from the same page with its
/// fills filled.
#[test]
fn fill_to_stroke_draws_each_fill_as_its_stroke() {
    let mut fills = String::from("q 0 0 120 90 re W n 0.2 0.8 0.3 rg ");
    let mut strokes = String::from("q 0 0 120 90 re W n 0.2 0.8 0.3 rg ");
    for (path, op) in FILLS {
        fills.push_str(&format!("{path} {op}\n"));
        strokes.push_str(&format!("q 0 w [] 0 d {path} S Q\n"));
        if op == "B" {
            // The fill-and-stroke's stroke is its own, at the page's pen.
            strokes.push_str(&format!("{path} S\n"));
        }
    }
    fills.push_str("Q q 30 40 40 40 re W n 20 45 60 30 re f Q");
    strokes.push_str("Q q 30 40 40 40 re W n q 0 w [] 0 d 20 45 60 30 re S Q Q");
    let (fills, strokes) = (fills_page(&fills), fills_page(&strokes));
    for scale in [1.0, 1.3, 2.5] {
        for antialias in [true, false] {
            let options = RenderOptions {
                antialias,
                ..forced_options(scale, PixelFormat::Rgb8, forced(true))
            };
            let drawn = render(fills.clone(), &options);
            let twin = render(strokes.clone(), &options);
            assert_eq!(
                drawn.data, twin.data,
                "at {scale}x, anti-aliased {antialias}"
            );
            let filled = render(
                fills.clone(),
                &RenderOptions {
                    antialias,
                    ..forced_options(scale, PixelFormat::Rgb8, forced(false))
                },
            );
            assert_ne!(drawn.data, filled.data, "the interiors are not painted");
        }
    }
}

/// A Type 3 glyph's fills are a glyph's, and a glyph stays filled: the type 3
/// page renders the same with fill-to-stroke and without, in its glyphs,
/// while the same squares filled by the page become their outlines.
#[test]
fn a_type3_glyph_stays_filled_under_fill_to_stroke() {
    for scale in [1.0, 2.0] {
        let glyphs = |fill_to_stroke: bool, as_page: bool| {
            render(
                type3_page(as_page),
                &forced_options(scale, PixelFormat::Gray8, forced(fill_to_stroke)),
            )
        };
        // The triangle the page fills is outlined either way, so compare the
        // rows the squares are on, below it.
        let squares = |bitmap: &Bitmap| -> Vec<u8> {
            let rows = (f64::from(bitmap.height) * 0.4) as usize;
            bitmap.data[bitmap.stride * (bitmap.height as usize - rows)..]
                .chunks(bitmap.stride)
                .flat_map(|row| row[..(bitmap.width as usize / 2)].to_vec())
                .collect()
        };
        let (filled, outlined) = (glyphs(false, false), glyphs(true, false));
        assert!(squares(&filled).contains(&0), "the glyphs are drawn");
        assert_eq!(squares(&filled), squares(&outlined), "at {scale}x");
        assert_ne!(
            filled.data, outlined.data,
            "the page's own triangle is outlined"
        );
        let (page_filled, page_outlined) = (glyphs(false, true), glyphs(true, true));
        assert_ne!(
            squares(&page_filled),
            squares(&page_outlined),
            "at {scale}x, the page's squares are outlined"
        );
    }
}

/// A forced render is opaque whatever `transparent` says, and premultiplying
/// an opaque page changes no byte of it.
#[test]
fn a_forced_render_is_opaque_and_its_options_mean_what_they_mean() {
    let colours = ForcedColours {
        background: Color::rgb(20, 20, 60),
        foreground: Color::rgb(250, 250, 180),
        fill_to_stroke: false,
    };
    for format in [PixelFormat::GrayA8, PixelFormat::Rgba8, PixelFormat::Bgra8] {
        let plain = forced_options(1.0, format, colours);
        let opaque = render(forced_page(false), &plain);
        let alpha_at = format.components() - 1;
        assert!(opaque
            .data
            .chunks_exact(format.components())
            .all(|pixel| pixel[alpha_at] == 255));
        for (transparent, premultiplied) in [(true, false), (false, true), (true, true)] {
            let asked = RenderOptions {
                transparent,
                premultiplied,
                ..plain.clone()
            };
            let got = render(forced_page(false), &asked);
            assert_eq!(
                got.data, opaque.data,
                "{format:?}, {transparent}, {premultiplied}"
            );
            assert_eq!(got.premultiplied, premultiplied);
        }
    }
    // The default forced colours are black on white with fills filled.
    assert_eq!(
        ForcedColours::default(),
        ForcedColours {
            background: Color::WHITE,
            foreground: Color::BLACK,
            fill_to_stroke: false,
        }
    );
}

/// Two greys and a red filled on a page, under a transfer function that turns
/// every component over (10.5) where `transfer` asks, and under none where it
/// does not: the function a type 4 calculator, `{ 1 exch sub }`.
fn transfer_page(transfer: bool) -> Vec<u8> {
    let content = format!(
        "{} 0.25 g 10 10 50 30 re f 0.8 g 20 25 40 20 re f 1 0 0 rg 70 5 20 40 re f",
        if transfer { "/T gs" } else { "" }
    );
    let function = "{ 1 exch sub }";
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 50] \
/Resources << /ExtGState << /T << /TR 5 0 R >> >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
5 0 obj\n<< /FunctionType 4 /Domain [0 1] /Range [0 1] /Length {} >>\nstream\n\
{function}\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        content.len(),
        function.len(),
    )
    .into_bytes()
}

/// **No transfer function applies to a forced render** (10.5): the page's
/// colours are what forced colours replace, and the tint is the final colour.
/// A page under a function that turns every component over renders in forced
/// colours, in every page format, with fill-to-stroke and without, exactly as
/// the same page under none — though drawn as it asks, the function moves its
/// pixels.
#[test]
fn forced_colours_apply_no_transfer_function() {
    let asked = render(transfer_page(true), &RenderOptions::default());
    let plain = render(transfer_page(false), &RenderOptions::default());
    assert_ne!(asked.data, plain.data, "an ordinary render applies it");
    for format in FORCED_FORMATS {
        for fill_to_stroke in [false, true] {
            let colours = ForcedColours {
                background: Color::rgb(250, 250, 235),
                foreground: Color::rgb(10, 30, 90),
                fill_to_stroke,
            };
            let options = forced_options(1.0, format, colours);
            let with = render(transfer_page(true), &options);
            let without = render(transfer_page(false), &options);
            assert_eq!(with.data, without.data, "{format:?}, {fill_to_stroke}");
            assert!(with.warnings.is_empty(), "{:?}", with.warnings);
        }
    }
}

/// A display list draws a page in forced colours as the page draws itself:
/// the same bytes and the same warnings, with fill-to-stroke and without, at
/// two scales — the recording made for an ordinary render, whose answers a
/// forced renderer gives too.
#[test]
fn a_display_list_draws_forced_colours_as_the_page_does() {
    for (name, bytes) in forced_fixtures() {
        let doc = Document::open(bytes).expect("the fixture opens");
        let page = doc.page(0).expect("a page");
        let list = page.display_list();
        assert!(list.is_retained(), "{name}");
        for scale in [1.0, 1.7] {
            for fill_to_stroke in [false, true] {
                let options = forced_options(scale, PixelFormat::Rgb8, forced(fill_to_stroke));
                let direct = page.render(&options);
                let replayed = list.render(&options);
                assert_eq!(
                    replayed.data, direct.data,
                    "{name} at {scale}x, {fill_to_stroke}"
                );
                assert_eq!(replayed.warnings, direct.warnings, "{name} at {scale}x");
            }
        }
    }
}

/// The pinned renders of [`forced_colour_output_is_pinned`], each its name
/// and its committed hash.
///
/// A value is a run's output: one left empty is recorded from the first
/// target's run, which prints the lines to paste, and held on every target
/// after it — Linux, Windows and `wasm32-wasip1`, in the default and
/// `bundled-fonts` builds, the page embedding its own face — and one two
/// targets disagree on is a determinism bug, never an edit of this table.
const FORCED_COLOUR_OUTPUT: [(&str, &str); 4] = [
    ("forced page, black on white, at 1x", ""),
    ("forced page, black on white, fill-to-stroke, at 1x", ""),
    ("forced page, yellow on navy, at 1.7x", ""),
    ("forced page, yellow on navy, fill-to-stroke, at 1.7x", ""),
];

/// **The forced-colour fingerprint** (RC-09.6): the forced page — text,
/// translucent and blended paint, a dashed stroke, a turned image, a knockout
/// ink group and a soft mask — drawn in forced colours with and without
/// fill-to-stroke, black on white at 1x and yellow on navy at 1.7x, where
/// every edge lands on another phase of the pixel grid, each hashed as
/// `determinism.rs` hashes a page to its committed value
/// ([`FORCED_COLOUR_OUTPUT`]). Each render's coverage, the black-on-white
/// grey of the same page, is floored by ink first, about half of what it
/// paints or less; every row is checked before any fails, and a failure
/// prints every line that moved.
#[test]
fn forced_colour_output_is_pinned() {
    let navy = ForcedColours {
        background: Color::rgb(10, 20, 90),
        foreground: Color::rgb(255, 230, 0),
        fill_to_stroke: false,
    };
    let cases = [
        (1.0, ForcedColours::default(), 2_500),
        (1.0, forced(true), 700),
        (1.7, navy, 7_000),
        (
            1.7,
            ForcedColours {
                fill_to_stroke: true,
                ..navy
            },
            1_500,
        ),
    ];
    let mut moved = Vec::new();
    for ((scale, colours, least), (name, want)) in cases.iter().zip(FORCED_COLOUR_OUTPUT) {
        let coverage = render(
            forced_page(false),
            &forced_options(*scale, PixelFormat::Gray8, forced(colours.fill_to_stroke)),
        );
        let drawn = coverage.data.iter().filter(|&&v| v != 255).count();
        assert!(
            drawn >= *least,
            "{name}: its coverage painted {drawn} pixels, fewer than {least}: it is \
             measuring less than it claims. Warnings: {:?}",
            coverage.warnings
        );
        let bitmap = render(
            forced_page(false),
            &forced_options(*scale, PixelFormat::Rgb8, *colours),
        );
        let actual = fingerprint(&bitmap);
        if actual != want {
            moved.push(format!("    (\"{name}\", \"{actual}\"),"));
        }
    }
    assert!(
        moved.is_empty(),
        "{} of the {} pinned forced-colour renders moved.\n\n\
         If two targets disagree, this is a determinism bug: find the arithmetic that \
         is not target-stable and do not update the values.\n\
         If the table was committed empty, or this is a deliberate rendering change \
         said in its commit, paste:\n\n{}",
        moved.len(),
        FORCED_COLOUR_OUTPUT.len(),
        moved.join("\n")
    );
}

// ---- levels and rules (RC-11.5) ---------------------------------------------

/// **`antialias` keeps its meaning** (RC-11.5): off, both levels read as 0
/// under the rule, whatever they say, so the hard-edge page with
/// anti-aliasing off renders the same bytes at any levels — under `Half`,
/// the bytes `hard_edged_output_is_pinned` pins.
#[test]
fn with_anti_aliasing_off_the_levels_read_as_zero() {
    let off = render(
        hard_edge_page(),
        &RenderOptions {
            antialias: false,
            ..RenderOptions::default()
        },
    );
    for (graphics_bits, text_bits) in [(8, 8), (3, 5), (0, 8), (1, 0), (200, 9)] {
        let other = render(
            hard_edge_page(),
            &RenderOptions {
                antialias: false,
                graphics_bits,
                text_bits,
                ..RenderOptions::default()
            },
        );
        assert_eq!(
            other.data, off.data,
            "graphics at {graphics_bits} bits and text at {text_bits}, anti-aliasing off"
        );
    }
}

/// The hard-edge page's elements, each alone and in black on white: the
/// twins whose 8-bit coverage `255 − pixel` is, so that a level's every pixel
/// is computable from it. Text, the diagonal fill, the stroked curve, the
/// hairline and the turned image, each as [`hard_edge_page`] draws it, the
/// first in the text class and the rest in the graphics class.
fn element_page(index: usize) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    let samples = [0u8; 12];
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 2,
            height: 2,
            data: &samples,
        }
    ));
    builder.add_page(160.0, 100.0, |page| match index {
        0 => page.text(b"F0", 14.0, 4.0, 78.0, "fox 0123"),
        1 => page.raw(b"0 g 100 45 m 150 75 l 140 40 l h f"),
        2 => page.raw(b"0 G 2.3 w 10 10 m 30 45 60 5 80 30 c S"),
        3 => page.raw(b"0 G 0 w 5 97 m 155 94 l S"),
        _ => page.raw(b"q 20 12 -12 20 95 5 cm /Im0 Do Q"),
    });
    built("hard edge element", builder)
}

/// The value `bits` bits of coverage take a coverage `c` to, computed here
/// from `docs/design/anti-aliasing.md`'s definition in its own integers: the
/// nearest of the `2^b` indices, `c·n / 255` rounded half up as
/// `(2·c·n + 255) / 510`, and that index's value, `q·255 / n` rounded as
/// `(2·q·255 + n) / (2n)`; 0 bits the half rule, 8 the coverage itself.
fn quantised_coverage(c: u8, bits: u8) -> u8 {
    match bits {
        0 => {
            if c >= 128 {
                255
            } else {
                0
            }
        }
        8.. => c,
        _ => {
            let n = (1u32 << bits) - 1;
            let q = (2 * u32::from(c) * n + 255) / 510;
            ((2 * q * 255 + n) / (2 * n)) as u8
        }
    }
}

/// **Coverage at each level is the 8-bit coverage quantised** (RC-11.5).
///
/// Each element of the hard-edge page, alone in black on white, rendered at
/// every level from 1 to 8 of its own class — text by
/// `RenderOptions::text_bits`, the fill, the curve, the hairline and the
/// image's edge by `RenderOptions::graphics_bits` — holds at every pixel
/// `255 − Q_b(255 − twin)`, the twin being the same element at the default
/// 8 bits and `Q_b` the level computed in the test. The other class's level
/// moves no pixel of it: graphics and text apart. At 1 bit every pixel is
/// whole or empty, the half rule's, and the twins have partial pixels to
/// quantise, so the comparison is not of white with white.
#[test]
fn coverage_at_each_level_is_the_eight_bit_coverage_quantised() {
    for index in 0..5 {
        let text = index == 0;
        // Every render of this test strokes at one floor, a whole pixel,
        // so that the hairline is one shape at every level and only its
        // coverage is quantised: the floors are
        // `a_hairline_at_the_minimum_width_drops_out_nowhere`'s.
        let floor = RenderOptions {
            thinnest_line: Some(1.0),
            ..RenderOptions::default()
        };
        let twin = render(element_page(index), &floor);
        assert!(
            twin.data.chunks_exact(3).any(|p| p[0] != 0 && p[0] != 255),
            "element {index} has partial pixels"
        );
        assert!(twin
            .data
            .chunks_exact(3)
            .all(|p| p[0] == p[1] && p[1] == p[2]));
        for bits in 1..=8u8 {
            let (graphics_bits, text_bits) = if text { (8, bits) } else { (bits, 8) };
            let options = RenderOptions {
                graphics_bits,
                text_bits,
                ..floor.clone()
            };
            let level = render(element_page(index), &options);
            for (at, (got, was)) in level
                .data
                .chunks_exact(3)
                .zip(twin.data.chunks_exact(3))
                .enumerate()
            {
                let want = 255 - quantised_coverage(255 - was[0], bits);
                assert_eq!(
                    got,
                    &[want, want, want][..],
                    "element {index} at {bits} bits, pixel {at}: the twin's {}",
                    was[0]
                );
            }
            // The other class's level reaches nothing of this element.
            let (graphics_bits, text_bits) = if text { (bits, 8) } else { (8, bits) };
            let other = render(
                element_page(index),
                &RenderOptions {
                    graphics_bits,
                    text_bits,
                    ..floor.clone()
                },
            );
            assert_eq!(
                other.data, twin.data,
                "element {index}: the other class at {bits} bits"
            );
        }
    }
}

/// One page of a single coverage site, written out: `resources` inside the
/// page's `/Resources`, `content` its stream, and `objects` from object 5 on,
/// each `(dictionary, stream)`.
fn site_page(resources: &str, content: &str, objects: &[(&str, &str)]) -> Vec<u8> {
    let mut pdf = String::from("%PDF-1.7\n");
    pdf.push_str("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    pdf.push_str("2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    pdf.push_str(&format!(
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 160 100] \
         /Resources << {resources} >> /Contents 4 0 R >>\nendobj\n"
    ));
    pdf.push_str(&format!(
        "4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
        content.len()
    ));
    for (number, (dictionary, stream)) in (5..).zip(objects) {
        if stream.is_empty() {
            pdf.push_str(&format!("{number} 0 obj\n{dictionary}\nendobj\n"));
        } else {
            pdf.push_str(&format!(
                "{number} 0 obj\n<< {dictionary} /Length {} >>\nstream\n{stream}\nendstream\nendobj\n",
                stream.len()
            ));
        }
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
        5 + objects.len()
    ));
    pdf.into_bytes()
}

/// The black pixels of a render: where its red channel is 0.
fn black(bitmap: &Bitmap) -> Vec<bool> {
    bitmap.data.chunks_exact(3).map(|p| p[0] == 0).collect()
}

/// **Each rule reaches every coverage site** (RC-11.5).
///
/// Every place a render makes coverage, each alone on a page in black: a
/// path's fill and its stroke, a clip by `W n`, a glyph from its mask, a
/// stroked glyph and a glyph clipping mode (`7 Tr`), a Type 3 glyph's
/// procedure, an image's edge, a mesh's silhouette, a tiling pattern's cell
/// and `sh` through a clip. With anti-aliasing off each is whole or empty
/// under every rule; the any-part rule paints every pixel the half rule and
/// the centre rule paint and more, its edges meeting pixels whose centres
/// they miss — so each rule reached the site. And with anti-aliasing on, a
/// level of 0 for one class reaches that class's sites alone: the text
/// sites, the Type 3 glyph's among them, are hard at a text level of 0 and
/// soft at a graphics level of 0, and the graphics sites the other way
/// round.
#[test]
fn each_rule_reaches_every_coverage_site() {
    let font_page = |content: &[u8]| {
        let mut builder = DocumentBuilder::new();
        builder.set_subset_fonts(false);
        assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
        builder.add_page(160.0, 100.0, |page| page.raw(content));
        built("coverage site", builder)
    };
    let type3 = "/Font << /T3 << /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000] \
                 /FontMatrix [0.001 0 0 0.001 0 0] /CharProcs << /a 5 0 R >> \
                 /Encoding << /Type /Encoding /Differences [97 /a] >> \
                 /FirstChar 97 /LastChar 97 /Widths [1000] >> >>";
    let mesh_stream: String = [(15u8, 20u8), (110, 45), (45, 90)]
        .iter()
        .flat_map(|&(x, y)| [0u8, x, y, 0, 0, 0])
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        + ">";
    let flat = "/ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 160 100] \
                /Function << /FunctionType 2 /Domain [0 1] /C0 [0 0 0] /C1 [0 0 0] /N 1 >>";
    let sites: Vec<(&str, bool, Vec<u8>)> = vec![
        (
            "a path's fill",
            false,
            site_page("", "0 g 20.3 10.7 m 70.6 40.2 l 30.1 60.8 l h f", &[]),
        ),
        (
            "a path's stroke",
            false,
            site_page("", "0 G 1.7 w 10.2 80.3 m 80.6 70.1 l S", &[]),
        ),
        (
            "a clip",
            false,
            site_page(
                "",
                "20.4 20.6 m 90.3 32.2 l 60.8 70.4 l h W n 0 g 0 0 160 100 re f",
                &[],
            ),
        ),
        (
            "a glyph from its mask",
            true,
            font_page(b"BT /F0 14 Tf 4.3 30.6 Td (fox 0123) Tj ET"),
        ),
        (
            "a stroked glyph",
            true,
            font_page(b"BT /F0 14 Tf 1 Tr 0.6 w 0 G 4.3 60.6 Td (fox) Tj ET"),
        ),
        (
            "a glyph clip",
            true,
            font_page(b"BT /F0 30 Tf 7 Tr 10.3 20.6 Td (fox) Tj ET 0 g 0 0 160 100 re f"),
        ),
        (
            "a Type 3 glyph",
            true,
            site_page(
                type3,
                "BT /T3 60 Tf 20.3 10.6 Td (a) Tj ET",
                &[("", "1000 0 d0 0 g 100 100 m 900 250 l 450 950 l h f")],
            ),
        ),
        (
            "an image's edge",
            false,
            site_page(
                "/XObject << /Im0 5 0 R >>",
                "q 20 12 -12 20 95 5 cm /Im0 Do Q",
                &[(
                    "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceGray \
                /BitsPerComponent 8 /Filter /ASCIIHexDecode",
                    "00000000>",
                )],
            ),
        ),
        (
            "a mesh's silhouette",
            false,
            site_page(
                "/Shading << /Sh0 5 0 R >>",
                "/Sh0 sh",
                &[(
                    "/ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 \
                /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 160 0 100 0 1 0 1 0 1] \
                /Filter /ASCIIHexDecode",
                    mesh_stream.as_str(),
                )],
            ),
        ),
        (
            "a tiling cell",
            false,
            site_page(
                "/Pattern << /P0 5 0 R >>",
                "/Pattern cs /P0 scn 5 5 150 90 re f",
                &[(
                    "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 \
                /YStep 10 /Resources << >>",
                    "0 g 0 0 m 10 6 l 10 10 l 4 10 l h f",
                )],
            ),
        ),
        (
            "sh through a clip",
            false,
            site_page(
                &format!("/Shading << /Sh1 << {flat} >> >>"),
                "q 90.4 10.2 m 150.7 30.6 l 110.1 60.3 l h W n /Sh1 sh Q",
                &[],
            ),
        ),
    ];
    for (name, text, page) in &sites {
        let hard = |hard_edge: HardEdge| {
            let bitmap = render(
                page.clone(),
                &RenderOptions {
                    antialias: false,
                    hard_edge,
                    // One floor under every rule, so that a stroke is one
                    // shape whichever rule reads it.
                    thinnest_line: Some(1.0),
                    ..RenderOptions::default()
                },
            );
            assert!(bitmap.warnings.is_empty(), "{name}: {:?}", bitmap.warnings);
            assert!(
                bitmap
                    .data
                    .chunks_exact(3)
                    .all(|p| p == [0, 0, 0] || p == [255, 255, 255]),
                "{name} under {hard_edge:?}: whole or empty everywhere"
            );
            black(&bitmap)
        };
        let (half, centre, any) = (
            hard(HardEdge::Half),
            hard(HardEdge::Centre),
            hard(HardEdge::AnyPart),
        );
        assert!(
            centre.iter().any(|b| *b),
            "{name}: the centre rule paints it"
        );
        for (rule, pixels) in [("half", &half), ("centre", &centre)] {
            for (at, (painted, met)) in pixels.iter().zip(&any).enumerate() {
                assert!(
                    !painted || *met,
                    "{name}: pixel {at} under the {rule} rule is not met"
                );
            }
        }
        let more = any.iter().zip(&centre).filter(|(a, c)| **a && !**c).count();
        assert!(
            more > 0,
            "{name}: the any-part rule reaches pixels the centre rule does not"
        );

        let partial = |graphics_bits: u8, text_bits: u8| {
            render(
                page.clone(),
                &RenderOptions {
                    graphics_bits,
                    text_bits,
                    hard_edge: HardEdge::AnyPart,
                    thinnest_line: Some(1.0),
                    ..RenderOptions::default()
                },
            )
            .data
            .chunks_exact(3)
            .any(|p| p[0] != 0 && p[0] != 255)
        };
        let (own, other) = if *text {
            (partial(8, 0), partial(0, 8))
        } else {
            (partial(0, 8), partial(8, 0))
        };
        assert!(
            !own,
            "{name}: its own class at 0 bits leaves no partial pixel"
        );
        assert!(other, "{name}: the other class at 0 bits leaves it soft");
    }
}

// ---- the minimum line width (RC-11.6) ---------------------------------------

/// A 120 by 140 page holding one zero-width black stroke from `from` to `to`.
fn hairline_page(from: (f64, f64), to: (f64, f64)) -> Vec<u8> {
    let content = format!("0 w {} {} m {} {} l S", from.0, from.1, to.0, to.1);
    format!(
        "%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
         2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
         3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 140] \
         /Contents 4 0 R >>\nendobj\n\
         4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
         trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// Whether column `x` of a render holds ink in any row, and row `y` in any
/// column: a pixel short of white.
fn column_inked(bitmap: &Bitmap, x: u32) -> bool {
    (0..bitmap.height).any(|y| bitmap.data[y as usize * bitmap.stride + x as usize * 3] < 255)
}

fn row_inked(bitmap: &Bitmap, y: u32) -> bool {
    (0..bitmap.width).any(|x| bitmap.data[y as usize * bitmap.stride + x as usize * 3] < 255)
}

/// **A hairline at the minimum width drops out nowhere** (RC-11.6).
///
/// Zero-width strokes at the slopes
/// `a_hard_edged_hairline_leaves_no_column_it_crosses_empty` sweeps — level
/// to one in one, each from a phase of its own and at four more — across the
/// page, where every column a line crosses must hold ink, and the same slopes
/// turned down the page, steeper than one in one past the first, where every
/// row must: at 8 bits, at 7, 4, 2 and 1, at 0 bits under each rule with
/// anti-aliasing on, and with it off under each rule, each at the floor its
/// level and rule take by default (`docs/design/anti-aliasing.md`'s table:
/// 0.8 at 8 bits, 1 at 1 to 7 bits and under `Half`, 65/64 under `Centre`,
/// 1/256 under `AnyPart`, whose edges reach the pixels they pass through).
#[test]
fn a_hairline_at_the_minimum_width_drops_out_nowhere() {
    let mut settings: Vec<(String, RenderOptions)> = Vec::new();
    for bits in [8u8, 7, 4, 2, 1] {
        settings.push((
            format!("{bits} bits"),
            RenderOptions {
                graphics_bits: bits,
                ..RenderOptions::default()
            },
        ));
    }
    for hard_edge in [HardEdge::Half, HardEdge::Centre, HardEdge::AnyPart] {
        settings.push((
            format!("0 bits under {hard_edge:?}"),
            RenderOptions {
                graphics_bits: 0,
                hard_edge,
                ..RenderOptions::default()
            },
        ));
        settings.push((
            format!("anti-aliasing off under {hard_edge:?}"),
            RenderOptions {
                antialias: false,
                hard_edge,
                ..RenderOptions::default()
            },
        ));
    }
    let rises = [0.0, 0.37, 1.5, 3.0, 11.0, 37.0, 60.0, 99.5];
    for (name, options) in &settings {
        for (index, rise) in rises.iter().enumerate() {
            for phase in [0.0, 0.29, 0.5, 0.71, 0.83] {
                let shift = index as f64 * 0.13 + phase;
                let y0 = 20.3 + shift;
                let across = render(hairline_page((10.0, y0), (110.0, y0 + rise)), options);
                for x in 11..109u32 {
                    assert!(
                        column_inked(&across, x),
                        "{name}: rise {rise} at phase {phase}: column {x} of the line is empty"
                    );
                }
                let x0 = 5.3 + shift;
                let down = render(hairline_page((x0, 15.0), (x0 + rise, 115.0)), options);
                for y in 26..124u32 {
                    assert!(
                        row_inked(&down, y),
                        "{name}: drift {rise} at phase {phase}: row {y} of the line is empty"
                    );
                }
            }
        }
    }
}

/// The pinned renders of [`levels_rules_and_widths_are_pinned`], each its
/// name and its committed hash.
///
/// A value is a run's output: one left empty is recorded from the first
/// target's run, which prints the lines to paste, and held on every target
/// after it — Linux, Windows and `wasm32-wasip1` — and one two targets
/// disagree on is a determinism bug, never an edit of this table.
const LEVELS_RULES_AND_WIDTHS: [(&str, &str); 8] = [
    ("graphics at 2 bits and text at 4, at 1x", ""),
    ("graphics at 1 bit and text at 7, at 1.7x", ""),
    ("graphics at 0 bits under the centre rule, at 1x", ""),
    ("text at 0 bits under the any-part rule, at 1.7x", ""),
    ("anti-aliasing off under the centre rule, at 1x", ""),
    ("anti-aliasing off under the any-part rule, at 1.7x", ""),
    ("a thinnest line of 2.5 pixels, at 1x", ""),
    (
        "anti-aliasing off under the any-part rule, a thinnest line of a quarter pixel, at 1.7x",
        "",
    ),
];

/// **The levels, rules and widths are pinned** (RC-11.6): the hard-edge page
/// rendered at levels apart and together, at 0 bits under each rule with the
/// other class anti-aliased, with anti-aliasing off under the centre and
/// any-part rules, and at explicit thinnest lines, at 1x and at 1.7x — each
/// hashed as `determinism.rs` hashes a page, to its committed value
/// ([`LEVELS_RULES_AND_WIDTHS`]). Every row is checked before any fails, and
/// a failure prints every line that moved.
#[test]
fn levels_rules_and_widths_are_pinned() {
    let at = |scale: f64, options: RenderOptions| RenderOptions { scale, ..options };
    let cases = [
        at(
            1.0,
            RenderOptions {
                graphics_bits: 2,
                text_bits: 4,
                ..RenderOptions::default()
            },
        ),
        at(
            1.7,
            RenderOptions {
                graphics_bits: 1,
                text_bits: 7,
                ..RenderOptions::default()
            },
        ),
        at(
            1.0,
            RenderOptions {
                graphics_bits: 0,
                hard_edge: HardEdge::Centre,
                ..RenderOptions::default()
            },
        ),
        at(
            1.7,
            RenderOptions {
                text_bits: 0,
                hard_edge: HardEdge::AnyPart,
                ..RenderOptions::default()
            },
        ),
        at(
            1.0,
            RenderOptions {
                antialias: false,
                hard_edge: HardEdge::Centre,
                ..RenderOptions::default()
            },
        ),
        at(
            1.7,
            RenderOptions {
                antialias: false,
                hard_edge: HardEdge::AnyPart,
                ..RenderOptions::default()
            },
        ),
        at(
            1.0,
            RenderOptions {
                thinnest_line: Some(2.5),
                ..RenderOptions::default()
            },
        ),
        at(
            1.7,
            RenderOptions {
                antialias: false,
                hard_edge: HardEdge::AnyPart,
                thinnest_line: Some(0.25),
                ..RenderOptions::default()
            },
        ),
    ];
    let mut moved = Vec::new();
    for (options, (name, want)) in cases.iter().zip(LEVELS_RULES_AND_WIDTHS) {
        let bitmap = render(hard_edge_page(), options);
        assert!(bitmap.warnings.is_empty(), "{name}: {:?}", bitmap.warnings);
        // About 1 780 pixels at 1x and 4 983 at 1.7x anti-aliasing off; the
        // floor is under half.
        let least = (800.0 * options.scale * options.scale) as usize;
        let drawn = painted(&bitmap);
        assert!(
            drawn >= least,
            "{name} painted {drawn} pixels, fewer than {least}: it is measuring less than it claims"
        );
        let actual = fingerprint(&bitmap);
        if actual != want {
            moved.push(format!("    (\"{name}\", \"{actual}\"),"));
        }
    }
    assert!(
        moved.is_empty(),
        "{} of the {} pinned renders moved.\n\n\
         If two targets disagree, this is a determinism bug: find the arithmetic that \
         is not target-stable and do not update the values.\n\
         If the table was committed empty, or this is a deliberate rendering change \
         said in its commit, paste:\n\n{}",
        moved.len(),
        LEVELS_RULES_AND_WIDTHS.len(),
        moved.join("\n")
    );
}

/// What `builder` writes, held to the validate-on-save helper as a document
/// this engine wrote from nothing it did not write.
#[track_caller]
fn built(label: &str, builder: tinker_pdf::DocumentBuilder) -> Vec<u8> {
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}
