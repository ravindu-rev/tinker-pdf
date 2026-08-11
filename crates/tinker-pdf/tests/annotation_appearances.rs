//! Annotation appearance streams reach the page correctly (12.5.5, 8.10.2).
//!
//! An appearance stream is a form XObject, and a form XObject is clipped to
//! its `/BBox`. Plenty of real appearance streams paint outside theirs —
//! the box is often the *intended* extent rather than the drawn one — so
//! without the clip an annotation spills across the page over content that
//! has nothing to do with it. It looks like a rendering bug in the page, not
//! in the annotation, which is why it takes so long to find.

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

/// A 40×40 page with one annotation in the lower-left 20×20, whose appearance
/// stream fills a rectangle four times the size of its own bounding box.
fn document(appearance_fill: &str, bbox: &str, rect: &str) -> Vec<u8> {
    let stream = format!("1 0 0 rg {appearance_fill}");
    let page = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Annots [5 0 R] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n\
5 0 obj\n<< /Type /Annot /Subtype /Square /Rect [{rect}]\n\
   /AP << /N 6 0 R >> >>\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [{bbox}]\n\
   /Length {} >>\nstream\n{stream}\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n",
        stream.len()
    );
    page.into_bytes()
}

/// The headline: paint outside the box, and none of it reaches the page.
///
/// The appearance fills 0..40 in form space while its box is 0..20. Fitting
/// maps the *box* onto `/Rect`, so without a clip the fill covers four times
/// the annotation's rectangle — the whole page, here.
#[test]
fn an_appearance_stream_is_clipped_to_its_bounding_box() {
    let bytes = document("0 0 40 40 re f", "0 0 20 20", "0 0 20 20");
    let bitmap = render(bytes);

    // Inside the annotation's rectangle, which is the lower-left quarter.
    // Device space puts y=0 at the top, so the lower-left of the page is the
    // bottom-left of the bitmap.
    let inside = pixel(&bitmap, 10, bitmap.height - 10);
    assert!(
        inside.0 > 200 && inside.1 < 60,
        "the annotation draws inside its rectangle, got {inside:?}"
    );

    // Outside it — the upper-right quarter — must be untouched.
    let outside = pixel(&bitmap, 30, 10);
    assert!(
        outside.0 > 200 && outside.1 > 200 && outside.2 > 200,
        "and nothing spilled outside, got {outside:?}"
    );
}

/// A stream that stays inside its box is unaffected, which is what says the
/// clip is a clip and not a crop of the annotation itself.
#[test]
fn an_appearance_stream_inside_its_box_is_untouched() {
    let bytes = document("0 0 20 20 re f", "0 0 20 20", "0 0 20 20");
    let bitmap = render(bytes);

    for (x, y) in [(2u32, 2u32), (10, 10), (17, 17)] {
        let there = pixel(&bitmap, x, bitmap.height - 1 - y);
        assert!(
            there.0 > 200 && there.1 < 60,
            "({x}, {y}) is inside the box and inside the rectangle: {there:?}"
        );
    }
}

/// The clip is in *form* space, so it has to survive the fitting transform
/// that maps the box onto a rectangle of a different size. Clipping in the
/// wrong space is the subtle version of this bug: the annotation renders,
/// just with the wrong part of it showing.
#[test]
fn the_clip_survives_being_fitted_to_a_different_rectangle() {
    // A 10-unit box scaled up onto a 20-unit rectangle.
    let bytes = document("0 0 40 40 re f", "0 0 10 10", "0 0 20 20");
    let bitmap = render(bytes);

    let inside = pixel(&bitmap, 10, bitmap.height - 10);
    assert!(
        inside.0 > 200 && inside.1 < 60,
        "the annotation still fills its rectangle after scaling, got {inside:?}"
    );

    let outside = pixel(&bitmap, 30, 10);
    assert!(
        outside.0 > 200 && outside.1 > 200 && outside.2 > 200,
        "and still does not spill, got {outside:?}"
    );
}

/// A form with no bounding box has nothing to clip to, and refusing to draw
/// it would lose annotations that render fine everywhere else (ruling 2).
#[test]
fn an_appearance_without_a_box_still_draws() {
    let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Annots [5 0 R] /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 0 >>\nstream\n\nendstream\nendobj\n\
5 0 obj\n<< /Type /Annot /Subtype /Square /Rect [0 0 20 20]\n\
   /AP << /N 6 0 R >> >>\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Form /Length 22 >>\n\
stream\n1 0 0 rg 0 0 20 20 re f\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n";

    let bitmap = render(bytes.to_vec());
    let there = pixel(&bitmap, 10, bitmap.height - 10);
    assert!(
        there.0 > 200 && there.1 < 60,
        "it draws where its own matrix puts it, got {there:?}"
    );
}

/// A bounding box holding an infinity or a NaN must not be written into a
/// content stream: `NaN` is not a number to a PDF lexer, it is a syntax error
/// that throws away everything after it.
#[test]
fn a_degenerate_bounding_box_does_not_corrupt_the_stream() {
    for bbox in ["0 0 1e9999 20", "0 0 20 20", "-1e9999 0 20 20"] {
        let bytes = document("0 0 20 20 re f", bbox, "0 0 20 20");
        let bitmap = render(bytes);
        assert_eq!(bitmap.data.len() % bitmap.stride, 0, "for /BBox [{bbox}]");
    }
}
