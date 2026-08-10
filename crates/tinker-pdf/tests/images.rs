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
