//! Images reach the page (8.9).
//!
//! There was no test anywhere that an image XObject actually renders. That
//! became apparent when a refactor moved `GlyphSource::image` out of its trait
//! impl and into an inherent one: the trait's default — which returns nothing
//! — took over, every image in every document silently stopped drawing, and
//! the whole suite stayed green. Only a dead-code warning gave it away.
//!
//! These are the tests that would have failed.

use tinker_pdf::{Document, RenderOptions};
use tinker_pdf_cos::{DocumentBuilder, ImageData};

fn render(bytes: Vec<u8>) -> tinker_pdf::Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn pixel(bitmap: &tinker_pdf::Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[0, 0, 0]);
    (p[0], p[1], p[2])
}

/// A solid red 2×2 image drawn over the whole page must make the page red.
#[test]
fn an_rgb_image_is_drawn() {
    let mut builder = DocumentBuilder::new();
    let red = [255u8, 0, 0].repeat(4);
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 2,
            height: 2,
            data: &red,
        }
    ));
    builder.add_page(20.0, 20.0, |page| {
        page.image(b"Im0", 0.0, 0.0, 20.0, 20.0);
    });

    let bitmap = render(builder.finish());
    let (r, g, b) = pixel(&bitmap, 10, 10);
    assert!(
        r > 200 && g < 60 && b < 60,
        "the image painted red, got ({r}, {g}, {b}) — white means it never drew"
    );
}

/// A greyscale image, to exercise the one-component path separately.
#[test]
fn a_grey_image_is_drawn() {
    let mut builder = DocumentBuilder::new();
    let grey = [0x40u8; 4];
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Gray8 {
            width: 2,
            height: 2,
            data: &grey,
        }
    ));
    builder.add_page(20.0, 20.0, |page| {
        page.image(b"Im0", 0.0, 0.0, 20.0, 20.0);
    });

    let bitmap = render(builder.finish());
    let (r, g, b) = pixel(&bitmap, 10, 10);
    assert!(
        (0x30..=0x50).contains(&r) && r == g && g == b,
        "a dark grey, got ({r}, {g}, {b})"
    );
}

/// The image occupies the unit square of the current transform, so it lands
/// where the `cm` puts it and nowhere else.
#[test]
fn an_image_lands_where_its_transform_puts_it() {
    let mut builder = DocumentBuilder::new();
    let red = [255u8, 0, 0].repeat(4);
    builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 2,
            height: 2,
            data: &red,
        },
    );
    builder.add_page(40.0, 40.0, |page| {
        // The left half only.
        page.image(b"Im0", 0.0, 0.0, 20.0, 40.0);
    });

    let bitmap = render(builder.finish());
    let left = pixel(&bitmap, 5, 20);
    let right = pixel(&bitmap, 35, 20);
    assert!(
        left.0 > 200 && left.1 < 60,
        "the left half is red: {left:?}"
    );
    assert!(
        right.0 > 200 && right.1 > 200 && right.2 > 200,
        "the right half is untouched: {right:?}"
    );
}

/// An image resource that names a codec this build has no decoder for is
/// reported *and* leaves a placeholder, rather than silently occupying no
/// space (ruling 2).
#[test]
fn an_undecodable_image_leaves_a_placeholder() {
    let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 44 >>\nstream\n\
q 20 0 0 20 0 0 cm /Im0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /JPXDecode /Length 4 >>\n\
stream\n\x00\x00\x00\x00\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n";

    let bitmap = render(bytes.to_vec());
    let (r, g, b) = pixel(&bitmap, 10, 10);
    assert!(
        r < 240 && r == g && g == b,
        "a neutral placeholder, got ({r}, {g}, {b})"
    );
    assert!(
        bitmap
            .warnings
            .iter()
            .any(|w| matches!(w, tinker_pdf::RenderWarning::UnsupportedImage { .. })),
        "and it says which codec was missing: {:?}",
        bitmap.warnings
    );
}

/// 8.9.5.2: `/Decode [1 0]` inverts the samples. Ignoring it renders the
/// image as its own negative, which looks like a decoder bug rather than a
/// missing feature.
#[test]
fn a_decode_array_inverts_the_samples() {
    let image = |decode: &str| -> tinker_pdf::Bitmap {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 28 >>\nstream\n\
q 20 0 0 20 0 0 cm /Im0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 {decode} /Length 4 >>\n\
stream\n\x00\x00\x00\x00\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n"
        );
        render(bytes.into_bytes())
    };

    let plain = pixel(&image(""), 10, 10);
    assert!(plain.0 < 40, "all-zero grey samples are black: {plain:?}");

    let inverted = pixel(&image("/Decode [1 0]"), 10, 10);
    assert!(
        inverted.0 > 200,
        "and inverted they are white: {inverted:?}"
    );
}

/// 11.6.5.3: an `/SMask` supplies per-sample opacity. Without it every
/// soft-masked image paints as an opaque rectangle.
#[test]
fn a_soft_mask_makes_an_image_transparent() {
    let with_mask = |smask: &str| -> tinker_pdf::Bitmap {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 28 >>\nstream\n\
q 20 0 0 20 0 0 cm /Im0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 {smask} /Length 4 >>\n\
stream\n\x00\x00\x00\x00\nendstream\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>\n\
stream\n\x00\x00\x00\x00\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n"
        );
        render(bytes.into_bytes())
    };

    // Object 6 is an all-zero mask: fully transparent everywhere.
    let opaque = pixel(&with_mask(""), 10, 10);
    assert!(
        opaque.0 < 40,
        "unmasked, the black image paints: {opaque:?}"
    );

    let masked = pixel(&with_mask("/SMask 6 0 R"), 10, 10);
    assert!(
        masked.0 > 200,
        "a zero mask hides it entirely, leaving the page: {masked:?}"
    );
}

/// A stencil mask paints in the current fill colour, not in black
/// (8.9.6.2). Baking black in at decode time paints every stencil black
/// whatever the page asked for.
#[test]
fn a_stencil_mask_paints_in_the_fill_colour() {
    let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 42 >>\nstream\n\
q 1 0 0 rg 20 0 0 20 0 0 cm /Im0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ImageMask true /BitsPerComponent 1 /Length 2 >>\n\
stream\n\x00\x00\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n";

    let bitmap = render(bytes.to_vec());
    let (r, g, b) = pixel(&bitmap, 10, 10);
    assert!(
        r > 200 && g < 60 && b < 60,
        "the stencil paints red, got ({r}, {g}, {b}) — black means the \
         colour was baked in at decode time"
    );
}
