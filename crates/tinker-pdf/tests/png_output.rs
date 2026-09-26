//! `Bitmap::to_png` over all six pixel formats, and what a rendered page does
//! when it goes out through it.
//!
//! # Why every format
//!
//! `tinker_pdf_render::page_format` keeps `CmykA8` off a page unless the caller
//! opts in with `RenderOptions::allow_cmyk`, and nothing in this engine hands a
//! caller a `LabA8` page at all. But `Bitmap`'s fields are
//! **public**, a transparency group compositing over ink (11.6.6) is a real
//! buffer of exactly that shape, and a caller who has one is entitled to write
//! it out. PNG has colour types 0, 2, 3, 4 and 6 and **no CMYK and no Lab**, so
//! the two have to be converted, and the conversion is the thing most likely to
//! be written as "drop the extra byte and hope": four CMYK components under
//! colour type 6 is a perfectly valid PNG that renders cyan as red.
//!
//! So the assertions here name the colours rather than comparing two of our own
//! buffers. Pure cyan is `(0, 255, 255)` because 8.6.4.4 says `R = (1 - C)`,
//! not because this engine's rasterizer says so, and a test that only asked
//! whether the bytes survived would pass on ink written under an RGB label.
//!
//! # What this file is not
//!
//! It is not the encoder's verification. A picture written by this repository
//! and read back by this repository says the pair is self-consistent and
//! nothing about ISO/IEC 15948 — the standing lesson from a JPEG 2000 plane
//! count that was right only for the code-blocks an in-tree encoder happened to
//! emit. The adjudication is `tinker-pdf-filters`' `tests/png_suite.rs`, which
//! runs the encoder over 176 files produced by an encoder nobody here wrote and
//! unfilters its output with a transcription of 9.2 that lives in the test. What
//! this file owns is the **format mapping**, which PngSuite cannot see: PngSuite
//! has no `Bitmap` and no `PixelFormat`.

use tinker_pdf::{Bitmap, Document, DocumentBuilder, PixelFormat, RenderOptions};
use tinker_pdf_filters::{png_decode, png_scan, Limits, PngColour};

const CAP: Limits = Limits::new(1 << 24);

/// A bitmap over a tightly packed buffer.
fn packed(width: u32, height: u32, format: PixelFormat, data: Vec<u8>) -> Bitmap {
    Bitmap {
        width,
        height,
        format,
        stride: width as usize * format.components(),
        data,
        warnings: Vec::new(),
    }
}

fn decode(bitmap: &Bitmap) -> tinker_pdf_filters::PngImage {
    let bytes = bitmap.to_png().expect("a picture");
    png_decode(&bytes, &CAP).expect("this engine's own PNG reads")
}

/// The whole mapping table, one row at a time: the colour type in IHDR, the
/// component count that follows from it, and the dimensions.
#[test]
fn every_pixel_format_maps_onto_a_colour_type_png_actually_has() {
    const MAPPING: [(PixelFormat, u8, PngColour); 6] = [
        (PixelFormat::Gray8, 0, PngColour::Grey),
        (PixelFormat::Rgb8, 2, PngColour::Rgb),
        (PixelFormat::GrayA8, 4, PngColour::GreyAlpha),
        (PixelFormat::Rgba8, 6, PngColour::Rgba),
        // The two PNG has no type for. Both keep their alpha, so both land on
        // 6 rather than on 2.
        (PixelFormat::CmykA8, 6, PngColour::Rgba),
        (PixelFormat::LabA8, 6, PngColour::Rgba),
    ];
    for (format, colour_type, colour) in MAPPING {
        let bitmap = packed(3, 2, format, vec![0x40; 6 * format.components()]);
        let bytes = bitmap.to_png().expect("a picture");
        let scan = png_scan(&bytes).expect("a readable file");
        assert_eq!(scan.header.colour_type, colour_type, "{format:?}");
        assert_eq!(scan.header.bit_depth, 8, "{format:?}");
        assert_eq!(
            (scan.header.width, scan.header.height),
            (3, 2),
            "{format:?}"
        );

        let image = decode(&bitmap);
        assert_eq!(image.colour, colour, "{format:?}");
        assert_eq!(image.data.len(), 3 * 2 * colour.components() as usize);
        // Ruling 10: a file this engine wrote takes no leniency from this
        // engine's reader.
        assert!(
            image.warnings.is_empty(),
            "{format:?}: {:?}",
            image.warnings
        );
        assert!(image.complete, "{format:?}");
    }
}

/// The four formats PNG already has a layout for are written **byte for byte**.
#[test]
fn the_four_formats_png_already_has_are_not_touched() {
    for format in [
        PixelFormat::Gray8,
        PixelFormat::GrayA8,
        PixelFormat::Rgb8,
        PixelFormat::Rgba8,
    ] {
        let n = format.components();
        let data: Vec<u8> = (0..(4 * 3 * n) as u32)
            .map(|i| (i.wrapping_mul(53) ^ 0x5A) as u8)
            .collect();
        let bitmap = packed(4, 3, format, data.clone());
        assert_eq!(decode(&bitmap).data, data, "{format:?}");
    }
}

/// **Alpha is not dropped.** An `Rgba8` bitmap whose alpha varies across the
/// row comes back with the same alpha.
///
/// An encoder that wrote three components under colour type 6 would produce a
/// file whose rows are one byte short and whose pixels slide sideways; one that
/// wrote three components under colour type 2 would produce a perfectly valid,
/// perfectly opaque picture — which is the version nothing but this assertion
/// catches, because a page render is very largely opaque and a fixture built
/// from one would agree with it.
#[test]
fn alpha_survives_an_rgba_bitmap() {
    let data: Vec<u8> = vec![
        10, 20, 30, 0, // transparent
        40, 50, 60, 1, // very nearly so
        70, 80, 90, 128, // half
        100, 110, 120, 255, // opaque
    ];
    let bitmap = packed(4, 1, PixelFormat::Rgba8, data.clone());
    let image = decode(&bitmap);
    assert_eq!(image.colour, PngColour::Rgba);
    assert_eq!(image.data, data);
    let alphas: Vec<u8> = image.data.chunks_exact(4).map(|p| p[3]).collect();
    assert_eq!(alphas, vec![0, 1, 128, 255]);

    // And the same for the two-channel form.
    let grey = vec![10u8, 0, 20, 128, 30, 255];
    let bitmap = packed(3, 1, PixelFormat::GrayA8, grey.clone());
    assert_eq!(decode(&bitmap).data, grey);
}

/// **`CmykA8` is five components and becomes four, by 8.6.4.4 and not by
/// truncation.**
///
/// The expected colours are written out from the relation `R = (1 - C)(1 - K)`
/// rather than read back from `cmyk_to_rgb`, so an encoder that wrote the
/// first four bytes of each pixel straight through fails here: pure cyan would
/// come back as `(255, 0, 0)`, a saturated red under a label saying RGB.
#[test]
fn cmyk_becomes_light_rather_than_four_bytes_of_ink() {
    // (C, M, Y, K, A) and the (R, G, B) 8.6.4.4 gives it.
    const INKS: [([u8; 5], [u8; 3]); 6] = [
        // No ink at all is white; full black ink is black.
        ([0, 0, 0, 0, 255], [255, 255, 255]),
        ([0, 0, 0, 255, 255], [0, 0, 0]),
        // One pure ink each: the complement of its own channel.
        ([255, 0, 0, 0, 255], [0, 255, 255]),
        ([0, 255, 0, 0, 255], [255, 0, 255]),
        ([0, 0, 255, 0, 255], [255, 255, 0]),
        // Ink over black is still black, whatever the ink.
        ([255, 128, 64, 255, 32], [0, 0, 0]),
    ];
    let mut data = Vec::new();
    for (ink, _) in INKS {
        data.extend_from_slice(&ink);
    }
    let bitmap = packed(INKS.len() as u32, 1, PixelFormat::CmykA8, data);
    let image = decode(&bitmap);
    assert_eq!(image.colour, PngColour::Rgba);
    assert_eq!(image.data.len(), INKS.len() * 4);

    for (i, (ink, want)) in INKS.iter().enumerate() {
        let px = &image.data[i * 4..i * 4 + 4];
        assert_eq!(&px[..3], want, "{ink:?}: 8.6.4.4 gives {want:?}");
        assert_eq!(px[3], ink[4], "{ink:?}: the alpha is the fifth byte");
    }
}

/// **`LabA8` is four components and stays four, but they are not the same
/// four.** Its bytes are an encoding of `L*a*b*`, so writing them under colour
/// type 6 unchanged would label lightness as red.
///
/// The discriminating fixture is sRGB's own **green primary**, whose CIELAB
/// coordinates are `L* = 87.73, a* = -86.18, b* = 83.18` — a published value,
/// not one this repository chose. Through `PixelFormat::LabA8`'s encoding
/// (`L/100`, `(a + 128)/255`, `(b + 128)/255`) those are the bytes `224, 42,
/// 211`, which read as RGB are a pale mauve and read as Lab are pure green.
/// Nothing but the conversion parts the two.
#[test]
fn lab_is_decoded_back_to_srgb_and_not_relabelled() {
    // (L, a, b, alpha) in the encoding `PixelFormat::LabA8` documents: 128 is
    // the neutral axis, 0 is `L* = 0` and 255 is `L* = 100`.
    let data: Vec<u8> = vec![
        0, 128, 128, 255, // L* = 0, neutral: black
        255, 128, 128, 200, // L* = 100, neutral: white
        224, 42, 211, 64, // sRGB's green primary
    ];
    let bitmap = packed(3, 1, PixelFormat::LabA8, data.clone());
    let image = decode(&bitmap);
    assert_eq!(image.colour, PngColour::Rgba);

    let px: Vec<&[u8]> = image.data.chunks_exact(4).collect();
    assert_eq!(&px[0][..3], &[0, 0, 0], "L* = 0 is black");
    assert_eq!(px[0][3], 255);
    assert_eq!(&px[1][..3], &[255, 255, 255], "L* = 100 neutral is white");
    assert_eq!(px[1][3], 200, "the alpha is the fourth byte");
    assert_eq!(
        &px[2][..3],
        &[0, 255, 0],
        "the green primary comes back green"
    );
    assert_eq!(px[2][3], 64);
    // And it is emphatically not the bytes that went in.
    assert_ne!(&px[2][..3], &data[8..11]);

    // A neutral mid-grey is a grey — three equal channels — and **not** the
    // byte that encoded it: `L* = 50` is 119, because lightness is perceptual
    // and the channel is not. A version that passed the bytes through would
    // answer 128 here and would still be three equal channels.
    let grey = packed(1, 1, PixelFormat::LabA8, vec![128, 128, 128, 255]);
    let out = decode(&grey);
    assert_eq!(&out.data[..3], &[119, 119, 119]);
}

/// A padded `stride` is honoured: the bytes between the end of one row and the
/// start of the next are not pixels.
///
/// Nothing in this engine pads a `Bitmap` today — `Canvas` sizes its buffer
/// from the width — so an encoder that flattened the raster would be correct
/// for every picture this repository can produce and wrong for the first one a
/// caller builds. The fixture is here for that day.
#[test]
fn a_padded_bitmap_writes_only_its_pixels() {
    let mut data = Vec::new();
    for row in [[1u8, 2, 3], [4, 5, 6]] {
        data.extend_from_slice(&row);
        data.extend_from_slice(&[0xFE, 0xFD]);
    }
    let padded = Bitmap {
        width: 3,
        height: 2,
        format: PixelFormat::Gray8,
        stride: 5,
        data,
        warnings: Vec::new(),
    };
    assert_eq!(decode(&padded).data, vec![1, 2, 3, 4, 5, 6]);
    // And the file is the same file the unpadded twin produces, byte for byte.
    let tight = packed(3, 2, PixelFormat::Gray8, vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(padded.to_png(), tight.to_png());
}

/// `None`, and only for a bitmap that is not a picture.
#[test]
fn a_bitmap_that_is_not_a_picture_declines_to_be_one() {
    let mut zero = packed(0, 4, PixelFormat::Rgb8, Vec::new());
    assert_eq!(zero.to_png(), None, "a zero width");
    zero.width = 4;
    zero.height = 0;
    assert_eq!(zero.to_png(), None, "a zero height");

    let short = Bitmap {
        width: 4,
        height: 4,
        format: PixelFormat::Rgb8,
        stride: 12,
        // One byte short of the final pixel.
        data: vec![0; 47],
        warnings: Vec::new(),
    };
    assert_eq!(short.to_png(), None, "a raster that stops short");

    let overlapping = Bitmap {
        width: 4,
        height: 4,
        format: PixelFormat::Rgb8,
        stride: 11,
        data: vec![0; 64],
        warnings: Vec::new(),
    };
    assert_eq!(overlapping.to_png(), None, "a stride narrower than a row");

    // The converted formats answer the same way rather than panicking on a
    // short buffer, which is the path with the extra arithmetic in it.
    let short_cmyk = Bitmap {
        width: 4,
        height: 2,
        format: PixelFormat::CmykA8,
        stride: 20,
        data: vec![0; 30],
        warnings: Vec::new(),
    };
    assert_eq!(short_cmyk.to_png(), None);

    // And one pixel is a picture.
    assert!(packed(1, 1, PixelFormat::Gray8, vec![9]).to_png().is_some());
}

/// The same bitmap encodes to the same bytes every time (ruling 4), and a
/// rendered page does too.
#[test]
fn the_same_page_writes_the_same_png_twice() {
    let mut builder = DocumentBuilder::new();
    builder.add_page(72.0, 48.0, |page| {
        page.fill_rect(8.0, 8.0, 30.0, 20.0, 0.0);
        page.fill_rect(40.0, 20.0, 20.0, 20.0, 0.5);
    });
    let doc = Document::open(builder.finish()).expect("a document");
    let page = doc.page(0).expect("one page");

    let first = page.render(&RenderOptions::at_dpi(72.0));
    let second = page.render(&RenderOptions::at_dpi(72.0));
    let a = first.to_png().expect("a picture");
    let b = second.to_png().expect("a picture");
    assert_eq!(a, b);

    // And the pixels that come back out are the pixels that went in — the one
    // round trip here, and it is about the page rather than about the format.
    let image = png_decode(&a, &CAP).expect("valid");
    assert_eq!((image.width, image.height), (first.width, first.height));
    assert_eq!(image.data, first.data);
    assert_eq!(image.colour, PngColour::Rgb);

    // A page that drew nothing would satisfy every line above, so the picture
    // has to have ink in it.
    let dark = first.data.iter().filter(|&&b| b < 64).count();
    assert!(
        dark > 200,
        "only {dark} dark samples: the page drew nothing"
    );
}
