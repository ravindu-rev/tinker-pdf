//! The `RenderOptions` fields that change what a page's bytes *are* rather than
//! which page or how large: ink instead of light, and what a pixel means when
//! it is not fully painted.
//!
//! # Why a file of its own, and why these hashes are not in `determinism.rs`
//!
//! `determinism.rs` pins nineteen pages rendered with `RenderOptions::default()`
//! and nothing else: its `Fixture` has a builder, a way to open and an ink floor,
//! and no options, and the table it checks is the one thing in the repository a
//! determinism bug would show up in. Growing an options column there would
//! change that file's shape to add a row, and a row that means "this page, but
//! differently" beside nineteen that mean "this page" is how a reader of that
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

use render_support::{blend_grid_page, curvy_font};
use tinker_pdf::{
    Bitmap, BlendMode, Document, DocumentBuilder, ExtGState, PixelFormat, RenderOptions,
};

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

/// Pixels something was painted on: any colour channel off the background,
/// which is no ink for `CmykA8` and white for every other format.
fn painted(bitmap: &Bitmap) -> usize {
    let components = bitmap.components();
    bitmap
        .data
        .chunks_exact(components)
        .filter(|pixel| match bitmap.format {
            PixelFormat::CmykA8 => pixel[..4].iter().any(|v| *v != 0),
            PixelFormat::Gray8 | PixelFormat::Rgb8 => pixel.iter().any(|v| *v != 255),
            _ => pixel[..components - 1].iter().any(|v| *v != 255),
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
    builder.finish()
}

// ---- CMYK page output --------------------------------------------------------

/// Asking for ink and saying so gets ink: five bytes a pixel, the canvas
/// starting with none, and each fill's components named — `1 0 0 0 k` is
/// exactly cyan because 8.6.4.4 relates it to `(0, 255, 255)` and maximum
/// undercolour removal relates that straight back.
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
    // **The limitation, pinned so it cannot be forgotten**: a rich black is
    // light converted back, and comes back as the pure K of the same colour.
    assert_eq!(
        at(38, 40),
        [0, 0, 0, 255, 255],
        "a rich black arrives as pure K: the ink is light turned back into \
         ink, not the file's own components"
    );
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

/// `to_png` writes an ink page as the light it stands for, and that light is
/// byte for byte what the same render returns without the switch: one
/// conversion, 8.6.4.4's, reached two ways.
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
    pinned(
        "ink page, in ink",
        &render(ink_page(), &options),
        780,
        "cd3c60cc88514aaa0ac5a0ce2eea1d7b26cc8b0d6412f2c9fc824c0a42e8788b",
    );
}

/// **The default is unchanged.** The blend grid rendered with the default
/// options is still `determinism.rs`'s `analytic_blend`, and the switch spelled
/// out at its default renders the same bytes as not mentioning it.
#[test]
fn the_default_is_unchanged() {
    let default = render(blend_grid_page(), &RenderOptions::default());
    pinned("blend grid, default", &default, 800, ANALYTIC_BLEND_DEFAULT);

    let spelled = render(
        blend_grid_page(),
        &RenderOptions {
            allow_cmyk: false,
            ..RenderOptions::default()
        },
    );
    assert_eq!(spelled.data, default.data);
    assert_eq!(spelled.format, PixelFormat::Rgb8);
}
