//! Colour spaces and patterns produce the right pixels (8.6, 8.7).
//!
//! Four separate defects lived here, and each one rendered plausibly wrong
//! rather than obviously broken, which is why none had a test:
//!
//! - a `/Function` array was truncated to its first element, so an RGB
//!   gradient became a red ramp;
//! - a `/Separation` tint transform was hardcoded to the identity, so spot
//!   colours came out as whatever the alternate space made of a raw tint;
//! - `/Lab` was aliased to `/DeviceRGB`, which clamps L (0..100) into 0..1 and
//!   renders nearly the whole space black;
//! - a pattern fill painted `/Pattern`'s nominal black, so every gradient fill
//!   from a real design tool became an opaque black rectangle.

use tinker_pdf::{Document, RenderOptions};

/// A one-page document whose content and resources are given verbatim.
fn page(resources: &str, content: &str) -> tinker_pdf::Bitmap {
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << {resources} >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
trailer\n<< /Size 9 /Root 1 0 R >>\n%%EOF\n",
        content.len() + 1
    );

    Document::open(bytes.into_bytes())
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

/// Three one-output functions, one per component, must all be read. Taking
/// only the first leaves green and blue at zero for the whole ramp.
#[test]
fn a_function_array_supplies_every_component() {
    let bitmap = page(
        "/Shading << /S0 << /ShadingType 2 /ColorSpace /DeviceRGB \
           /Coords [0 0 40 0] /Extend [true true] \
           /Function [ \
             << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> \
             << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> \
             << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> \
           ] >> >>",
        "q 0 0 40 40 re W n /S0 sh Q",
    );

    // At the far end every component is 1, so the ramp ends white — not red.
    let (r, g, b) = pixel(&bitmap, 38, 20);
    assert!(r > 200, "red reaches full: {r}");
    assert!(
        g > 200 && b > 200,
        "and so do green and blue: {g}, {b} — a truncated array leaves them at 0"
    );
}

/// A one-ink Separation over DeviceCMYK. Full tint through the transform is
/// black; the identity would feed the tint in as cyan.
#[test]
fn a_separation_runs_its_tint_transform() {
    let bitmap = page(
        "/ColorSpace << /Spot [ /Separation /Black /DeviceCMYK \
           << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0 0 1] /N 1 >> ] >>",
        "/Spot cs 1 scn 0 0 40 40 re f",
    );

    let (r, g, b) = pixel(&bitmap, 20, 20);
    assert!(
        r < 40 && g < 40 && b < 40,
        "full tint of a black ink is black, got ({r}, {g}, {b}) — \
         cyan means the transform was skipped"
    );
}

/// The same space at zero tint is white, which proves the transform is being
/// evaluated rather than the answer being black regardless.
#[test]
fn a_separation_at_zero_tint_is_blank() {
    let bitmap = page(
        "/ColorSpace << /Spot [ /Separation /Black /DeviceCMYK \
           << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 0 0 1] /N 1 >> ] >>",
        "/Spot cs 0 scn 0 0 40 40 re f",
    );

    let (r, g, b) = pixel(&bitmap, 20, 20);
    assert!(r > 220 && g > 220 && b > 220, "got ({r}, {g}, {b})");
}

/// L* of 100 with no chroma is white. Read as RGB it clamps to (1, 0, 0) after
/// the 0..1 clamp — and a mid-grey L* of 50 clamps to black.
#[test]
fn lab_lightness_is_not_clamped_into_zero_to_one() {
    let white = page(
        "/ColorSpace << /Lb [ /Lab << /WhitePoint [0.9642 1 0.8249] \
           /Range [-100 100 -100 100] >> ] >>",
        "/Lb cs 100 0 0 scn 0 0 40 40 re f",
    );
    let (r, g, b) = pixel(&white, 20, 20);
    assert!(
        r > 220 && g > 220 && b > 220,
        "L*=100 is white, got ({r}, {g}, {b})"
    );

    let mid = page(
        "/ColorSpace << /Lb [ /Lab << /WhitePoint [0.9642 1 0.8249] \
           /Range [-100 100 -100 100] >> ] >>",
        "/Lb cs 50 0 0 scn 0 0 40 40 re f",
    );
    let (r, g, b) = pixel(&mid, 20, 20);
    assert!(
        (90..=180).contains(&r) && (90..=180).contains(&g) && (90..=180).contains(&b),
        "L*=50 is a mid grey, got ({r}, {g}, {b})"
    );
}

/// A shading pattern fill paints the gradient, not black.
#[test]
fn a_shading_pattern_paints_its_shading() {
    let bitmap = page(
        "/Pattern << /P0 << /PatternType 2 /Shading \
           << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 40 0] \
              /Extend [true true] \
              /Function << /FunctionType 2 /Domain [0 1] \
                           /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >> >>",
        "/Pattern cs /P0 scn 0 0 40 40 re f",
    );

    let left = pixel(&bitmap, 2, 20);
    let right = pixel(&bitmap, 37, 20);
    assert!(
        left.0 > 180 && left.2 < 80,
        "the left end is red, got {left:?}"
    );
    assert!(
        right.2 > 180 && right.0 < 80,
        "the right end is blue, got {right:?} — black means the pattern was ignored"
    );
}

/// A tiling pattern is not painted, and says so. Filling it with black would
/// hide the gap behind something that reads as content.
#[test]
fn a_tiling_pattern_is_reported_rather_than_blacked_out() {
    let bitmap = page(
        "/Pattern << /P0 << /PatternType 1 /PaintType 1 /TilingType 1 \
           /BBox [0 0 8 8] /XStep 8 /YStep 8 /Resources << >> /Length 0 >> >>",
        "/Pattern cs /P0 scn 0 0 40 40 re f",
    );

    let (r, g, b) = pixel(&bitmap, 20, 20);
    assert!(
        r > 220 && g > 220 && b > 220,
        "the area is left alone, got ({r}, {g}, {b})"
    );
    assert!(
        bitmap
            .warnings
            .iter()
            .any(|w| matches!(w, tinker_pdf::RenderWarning::UnsupportedPattern { .. })),
        "and the gap is reported: {:?}",
        bitmap.warnings
    );
}

/// Choosing an ordinary colour after a pattern has to clear it, or every
/// later fill keeps painting the gradient.
#[test]
fn setting_a_colour_clears_the_pattern() {
    let bitmap = page(
        "/Pattern << /P0 << /PatternType 2 /Shading \
           << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 40 0] \
              /Extend [true true] \
              /Function << /FunctionType 2 /Domain [0 1] \
                           /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >> >>",
        "/Pattern cs /P0 scn 0 0 40 20 re f 0 1 0 rg 0 20 40 20 re f",
    );

    let patterned = pixel(&bitmap, 2, 30);
    let plain = pixel(&bitmap, 2, 10);
    assert!(
        patterned.0 > 180,
        "the lower half is the gradient's red end"
    );
    assert!(
        plain.1 > 180 && plain.0 < 80,
        "the upper half is plain green, got {plain:?}"
    );
}
