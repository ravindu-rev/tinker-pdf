//! `/Rotate` and `/CropBox` reach the pixels (7.7.3.3).
//!
//! Both were absent from the render transform while being present in the
//! canvas *size*, so a rotated page got a correctly sideways canvas filled
//! with upright, clipped content, and a page whose crop box did not start at
//! the origin had everything shifted. Neither had a test, and neither showed
//! up in the fixtures, all four of which are unrotated with a crop box at
//! (0, 0).
//!
//! The shape of every test here is the same: put a mark in one known corner,
//! render, and assert which corner the ink lands in. A transform that is
//! wrong in any of the three ways above moves it somewhere else.

use tinker_pdf::{Document, RenderOptions};
use tinker_pdf_cos::DocumentBuilder;

/// A 100×200 page — deliberately not square, so a quarter turn is visible in
/// the canvas dimensions — with a filled square in one corner.
///
/// `corner` picks which: it is the lower-left of the 20×20 mark in PDF user
/// space, where y counts up from the bottom.
fn page_with_mark(rotation: Option<i64>, crop: Option<(f64, f64, f64, f64)>) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(100.0, 200.0, |page| {
        // Bottom-left of the media box, in black.
        page.fill_rect(0.0, 0.0, 20.0, 20.0, 0.0);
    });
    let bytes = builder.finish();

    let Some(extra) = rotation
        .map(|r| format!("/Rotate {r}"))
        .or_else(|| crop.map(|(x0, y0, x1, y1)| format!("/CropBox [{x0} {y0} {x1} {y1}]")))
    else {
        return bytes;
    };

    // The builder writes no /Rotate or /CropBox, so they are spliced into the
    // page dictionary. Crude, and it keeps the fixture honest: the bytes are
    // what a producer would have written.
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let marker = "/Type /Page";
    let at = text.find(marker).expect("a page dictionary");
    let mut patched = text.clone();
    patched.insert_str(at + marker.len(), &format!(" {extra}"));
    patched.into_bytes()
}

/// Where the ink is, as (min_x, min_y, max_x, max_y) in pixels.
fn ink_bounds(bitmap: &tinker_pdf::Bitmap) -> Option<(u32, u32, u32, u32)> {
    let components = bitmap.components();
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    let mut any = false;

    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let at = (y as usize) * bitmap.stride + (x as usize) * components;
            if bitmap.data.get(at).is_some_and(|v| *v < 128) {
                any = true;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    any.then_some((x0, y0, x1, y1))
}

fn render(bytes: Vec<u8>) -> tinker_pdf::Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// The baseline: no rotation, crop box at the origin. The mark is at the
/// bottom-left in user space, so it is at the bottom-left of the raster too —
/// which counts down from the top, so near y = height.
#[test]
fn an_unrotated_page_puts_the_mark_at_the_bottom_left() {
    let bitmap = render(page_with_mark(None, None));
    assert_eq!((bitmap.width, bitmap.height), (100, 200));

    let (x0, y0, x1, y1) = ink_bounds(&bitmap).expect("there is ink");
    assert!(x0 <= 1 && x1 >= 18, "hugs the left edge: {x0}..{x1}");
    assert!(y1 >= bitmap.height - 2, "and the bottom: {y0}..{y1}");
}

/// A quarter turn clockwise swaps the canvas and carries the mark with it.
///
/// The bottom-left of an upright page becomes the **top-left** once the page
/// is turned 90° clockwise for display.
#[test]
fn a_quarter_turn_rotates_the_content_with_the_canvas() {
    let bitmap = render(page_with_mark(Some(90), None));
    assert_eq!(
        (bitmap.width, bitmap.height),
        (200, 100),
        "the canvas swaps for a quarter turn"
    );

    let (x0, y0, _x1, y1) = ink_bounds(&bitmap).expect("there is ink, not a blank page");
    assert!(x0 <= 2, "at the left: {x0}");
    assert!(y0 <= 2 && y1 <= 22, "and the top: {y0}..{y1}");
}

#[test]
fn a_half_turn_moves_the_mark_to_the_opposite_corner() {
    let bitmap = render(page_with_mark(Some(180), None));
    assert_eq!((bitmap.width, bitmap.height), (100, 200));

    let (x0, y0, x1, y1) = ink_bounds(&bitmap).expect("there is ink");
    assert!(x1 >= bitmap.width - 2, "at the right: {x0}..{x1}");
    assert!(y0 <= 2 && y1 <= 22, "and the top: {y0}..{y1}");
}

#[test]
fn three_quarter_turns_reach_the_remaining_corner() {
    let bitmap = render(page_with_mark(Some(270), None));
    assert_eq!((bitmap.width, bitmap.height), (200, 100));

    let (x0, y0, x1, y1) = ink_bounds(&bitmap).expect("there is ink");
    assert!(x1 >= bitmap.width - 2, "at the right: {x0}..{x1}");
    assert!(y1 >= bitmap.height - 2, "and the bottom: {y0}..{y1}");
}

/// Every rotation must draw the *same amount* of ink. A transform that clips
/// the content away still leaves some of it on the page, so counting is what
/// separates "rotated" from "rotated and half lost".
#[test]
fn rotation_never_loses_content() {
    let upright = render(page_with_mark(None, None));
    let expected = upright
        .data
        .chunks_exact(upright.components())
        .filter(|p| p[0] < 128)
        .count();
    assert!(expected > 300, "the baseline has a real mark: {expected}");

    for rotation in [90, 180, 270] {
        let bitmap = render(page_with_mark(Some(rotation), None));
        let found = bitmap
            .data
            .chunks_exact(bitmap.components())
            .filter(|p| p[0] < 128)
            .count();
        let drift = (found as i64 - expected as i64).abs();
        assert!(
            drift <= expected as i64 / 20,
            "/Rotate {rotation} drew {found} of {expected} inked pixels"
        );
    }
}

/// A crop box that does not start at the origin moves the visible area, and
/// its lower-left corner has to land on the canvas origin.
#[test]
fn a_crop_box_origin_shifts_the_visible_area() {
    // Crop to the middle: the mark at (0,0)–(20,20) falls outside it.
    let bitmap = render(page_with_mark(None, Some((30.0, 40.0, 90.0, 140.0))));
    assert_eq!(
        (bitmap.width, bitmap.height),
        (60, 100),
        "the canvas is the crop box, not the media box"
    );
    assert!(
        ink_bounds(&bitmap).is_none(),
        "and content outside the crop box is not drawn"
    );
}

/// The same crop box, moved to include the mark: it must now appear at the
/// crop box's own lower-left corner rather than at the media box's.
#[test]
fn content_inside_a_shifted_crop_box_lands_at_its_corner() {
    let bitmap = render(page_with_mark(None, Some((10.0, 10.0, 70.0, 110.0))));
    assert_eq!((bitmap.width, bitmap.height), (60, 100));

    let (x0, _y0, _x1, y1) = ink_bounds(&bitmap).expect("the mark is inside the crop box");
    // The mark spans user-space x 0..20; the crop box starts at 10, so 10
    // units of it are visible, hard against the left edge.
    assert!(x0 <= 1, "against the left edge: {x0}");
    assert!(y1 >= bitmap.height - 2, "and the bottom: {y1}");
}
