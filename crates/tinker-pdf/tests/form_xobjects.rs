//! Form XObjects are clipped to their bounding box (8.10.2).
//!
//! Forms are how most producers place repeated content — a letterhead, a
//! stamp, a chart drawn once and invoked per page — so a form that paints
//! outside its box paints over the rest of the page, every time it appears.
//! It reads as a defect in the page rather than in the form, which is what
//! makes it expensive to find.
//!
//! This is the same defect as the annotation one, a layer down: an appearance
//! stream *is* a form XObject, and the two were both unclipped.

use tinker_pdf::{Document, RenderOptions};

fn render(bytes: Vec<u8>) -> tinker_pdf::Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn pixel(bitmap: &tinker_pdf::Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[255, 255, 255]);
    (p[0], p[1], p[2])
}

/// The middle of the page's lower-left quarter, in device pixels.
///
/// The page is forty points and the bitmap is whatever the default resolution
/// makes of that, so quarters have to be named as fractions of the bitmap.
/// Reading device pixels as if they were points is how the first version of
/// these tests sampled the wrong quarter entirely.
fn lower_left(bitmap: &tinker_pdf::Bitmap) -> (u32, u32) {
    (bitmap.width / 4, bitmap.height * 3 / 4)
}

/// The middle of the page's upper-right quarter. Device space puts y = 0 at
/// the top, so this is a *small* y.
fn upper_right(bitmap: &tinker_pdf::Bitmap) -> (u32, u32) {
    (bitmap.width * 3 / 4, bitmap.height / 4)
}

/// A 40×40 page invoking a form whose content and box are given.
fn document(bbox: &str, matrix: &str, form_content: &str) -> Vec<u8> {
    let page_content = "q /Fm0 Do Q";
    let page = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << /XObject << /Fm0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{page_content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form {bbox} {matrix}\n\
   /Length {} >>\nstream\n{form_content}\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        page_content.len(),
        form_content.len()
    );
    page.into_bytes()
}

/// The headline: a form that fills the whole page is cut to its box.
#[test]
fn a_form_is_clipped_to_its_bounding_box() {
    let bytes = document("/BBox [0 0 20 20]", "", "1 0 0 rg 0 0 40 40 re f");
    let bitmap = render(bytes);

    let (x, y) = lower_left(&bitmap);
    let inside = pixel(&bitmap, x, y);
    assert!(
        inside.0 > 200 && inside.1 < 60,
        "the form draws inside its box, got {inside:?}"
    );

    let (x, y) = upper_right(&bitmap);
    let outside = pixel(&bitmap, x, y);
    assert!(
        outside.0 > 200 && outside.1 > 200 && outside.2 > 200,
        "and nothing spilled outside it, got {outside:?}"
    );
}

/// The clip is in form space, so `/Matrix` moves it with the content. A clip
/// applied in the wrong space is the subtle version: the form renders, just
/// with the wrong part of it showing.
#[test]
fn the_box_moves_with_the_forms_matrix() {
    // The box covers form-space 0..20, and the matrix shifts everything up
    // and right by 20, so the visible quarter is the top-right one.
    let bytes = document(
        "/BBox [0 0 20 20]",
        "/Matrix [1 0 0 1 20 20]",
        "1 0 0 rg 0 0 40 40 re f",
    );
    let bitmap = render(bytes);

    let (x, y) = upper_right(&bitmap);
    let inside = pixel(&bitmap, x, y);
    assert!(
        inside.0 > 200 && inside.1 < 60,
        "the box moved with the matrix, got {inside:?}"
    );

    let (x, y) = lower_left(&bitmap);
    let outside = pixel(&bitmap, x, y);
    assert!(
        outside.0 > 200 && outside.1 > 200 && outside.2 > 200,
        "and the original quarter is now empty, got {outside:?}"
    );
}

/// The clip must not survive the form. A form clipping to its own box and
/// then leaking that clip would erase whatever the page drew afterwards.
#[test]
fn the_clip_does_not_outlive_the_form() {
    let page_content = "q /Fm0 Do Q 0 0 1 rg 20 20 20 20 re f";
    let form_content = "1 0 0 rg 0 0 40 40 re f";
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << /XObject << /Fm0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{page_content}\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 20 20]\n\
   /Length {} >>\nstream\n{form_content}\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n",
        page_content.len(),
        form_content.len()
    )
    .into_bytes();

    let bitmap = render(bytes);

    // The blue square is drawn after the form, in the quarter the form's clip
    // excluded. If the clip leaked, it would be missing.
    let (x, y) = upper_right(&bitmap);
    let after = pixel(&bitmap, x, y);
    assert!(
        after.2 > 200 && after.0 < 60,
        "what the page drew after the form is still there, got {after:?}"
    );
}

/// 8.10.2 makes the box required, but a file that omits it gets an unclipped
/// form rather than no form (ruling 2).
#[test]
fn a_form_without_a_box_still_draws() {
    let bytes = document("", "", "1 0 0 rg 0 0 20 20 re f");
    let bitmap = render(bytes);

    let (x, y) = lower_left(&bitmap);
    let there = pixel(&bitmap, x, y);
    assert!(
        there.0 > 200 && there.1 < 60,
        "it draws where its matrix puts it, got {there:?}"
    );
}

/// 7.9.5: a rectangle's corners may be given in either order, and a box
/// written backwards must clip to the same area rather than to nothing.
#[test]
fn a_backwards_bounding_box_clips_the_same_area() {
    let bytes = document("/BBox [20 20 0 0]", "", "1 0 0 rg 0 0 40 40 re f");
    let bitmap = render(bytes);

    let (x, y) = lower_left(&bitmap);
    let inside = pixel(&bitmap, x, y);
    assert!(
        inside.0 > 200 && inside.1 < 60,
        "normalised rather than treated as empty, got {inside:?}"
    );
    let (x, y) = upper_right(&bitmap);
    let outside = pixel(&bitmap, x, y);
    assert!(
        outside.0 > 200 && outside.1 > 200 && outside.2 > 200,
        "and it still clips, got {outside:?}"
    );
}

/// A degenerate box must not clip everything away silently, nor panic.
#[test]
fn a_degenerate_bounding_box_does_not_panic() {
    for bbox in [
        "/BBox [0 0 0 0]",
        "/BBox [0 0 1e9999 20]",
        "/BBox [1 2 3]",
        "/BBox 7",
    ] {
        let bytes = document(bbox, "", "1 0 0 rg 0 0 20 20 re f");
        let bitmap = render(bytes);
        assert!(!bitmap.data.is_empty(), "for {bbox}");
    }
}
