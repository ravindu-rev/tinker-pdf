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

/// A pixel two renders disagree about: `(x, y, left, right)`.
type Difference = (u32, u32, (u8, u8, u8), (u8, u8, u8));

/// Where two renders of the same size first disagree, and what they say
/// there.
///
/// `assert_eq!` on the buffers is the assertion these tests want, but its
/// failure message is several thousand bytes of decimal with the interesting
/// pixel somewhere inside it. This says which pixel moved, which is the
/// question the person reading the failure has.
fn first_difference(a: &tinker_pdf::Bitmap, b: &tinker_pdf::Bitmap) -> Option<Difference> {
    if (a.width, a.height) != (b.width, b.height) {
        return Some((0, 0, pixel(a, 0, 0), pixel(b, 0, 0)));
    }
    for y in 0..a.height {
        for x in 0..a.width {
            let (left, right) = (pixel(a, x, y), pixel(b, x, y));
            if left != right {
                return Some((x, y, left, right));
            }
        }
    }
    None
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

/// The gradient this file's stroke tests paint with: red at x=0, blue at
/// x=40, extended past both ends so every pixel of a 40 pt page has a colour.
const GRADIENT: &str = "/Pattern << /P0 << /PatternType 2 /Shading \
       << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 40 0] \
          /Extend [true true] \
          /Function << /FunctionType 2 /Domain [0 1] \
                       /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >> >>";

/// A tiling pattern, which this build reports rather than paints.
const HATCH: &str = "/Pattern << /P0 << /PatternType 1 /PaintType 1 \
       /TilingType 1 /BBox [0 0 8 8] /XStep 8 /YStep 8 \
       /Resources << >> /Length 0 >> >>";

/// A shading pattern named by `SCN` paints the stroke, not `/Pattern`'s
/// nominal black.
#[test]
fn a_shading_pattern_strokes_with_its_shading() {
    let bitmap = page(GRADIENT, "/Pattern CS /P0 SCN 8 w 0 20 m 40 20 l S");

    let left = pixel(&bitmap, 2, 20);
    let right = pixel(&bitmap, 37, 20);
    assert!(
        left.0 > 180 && left.2 < 80,
        "the left end of the rule is red, got {left:?} — \
         black means the stroke ignored the pattern"
    );
    assert!(
        right.2 > 180 && right.0 < 80,
        "the right end is blue, got {right:?}"
    );

    // The rule is 8 pt of a 40 pt page, so everything outside it is untouched.
    // Without this a pattern painted over the whole canvas would pass above.
    let above = pixel(&bitmap, 20, 4);
    assert_eq!(
        above,
        (255, 255, 255),
        "outside the rule nothing is painted"
    );
}

/// The same band filled and stroked must come out the same. A stroke is a
/// fill of its outline, so anything that makes the two disagree — a different
/// fill rule, a different anchoring, a different alpha — shows up here and
/// nowhere else.
#[test]
fn a_pattern_fills_and_strokes_the_same_band_identically() {
    let filled = page(GRADIENT, "/Pattern cs /P0 scn 0 16 40 8 re f");
    let stroked = page(GRADIENT, "/Pattern CS /P0 SCN 8 w 0 20 m 40 20 l S");

    // Proof the comparison is about something: the band is painted and it is
    // not one flat colour.
    let left = pixel(&stroked, 2, 20);
    let right = pixel(&stroked, 37, 20);
    assert!(
        left.0 > 180 && right.2 > 180,
        "the band carries the gradient"
    );

    assert_eq!(
        first_difference(&filled, &stroked),
        None,
        "the same 8 pt band filled and stroked paints the same pixels"
    );
}

/// A tiling pattern on a *stroke* must degrade exactly as it does on a fill:
/// nothing painted, and a warning saying which pattern. Painting `/Pattern`'s
/// nominal black instead turns a known gap into what reads as content.
#[test]
fn a_tiling_pattern_stroke_is_reported_rather_than_blacked_out() {
    let bitmap = page(HATCH, "/Pattern CS /P0 SCN 8 w 0 20 m 40 20 l S");

    let (r, g, b) = pixel(&bitmap, 20, 20);
    assert!(
        r > 220 && g > 220 && b > 220,
        "the rule is left alone, got ({r}, {g}, {b}) — black is the defect"
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

/// 8.4.5: painting has two alphas, and a stroke uses `CA`. Reaching the
/// pattern paint through the fill path is the obvious way to build this, and
/// the obvious way carries `ca` with it — so a page that sets them apart is
/// the only thing that can tell the two apart.
#[test]
fn a_pattern_stroke_takes_the_stroking_alpha() {
    let resources = format!("{GRADIENT} /ExtGState << /GS0 << /ca 1 /CA 0.4 >> >>");
    let bitmap = page(
        &resources,
        "/GS0 gs /Pattern CS /P0 SCN 8 w 0 20 m 40 20 l S",
    );

    // The ramp is red at this end, so 40 % of it over white leaves green and
    // blue at about 153. Full opacity — which is what `ca` would give — leaves
    // them at 0.
    let (r, g, b) = pixel(&bitmap, 2, 20);
    assert!(r > 220, "red stays saturated over white, got {r}");
    assert!(
        (120..=185).contains(&g) && (120..=185).contains(&b),
        "the stroke is 40 % opaque, got ({r}, {g}, {b}) — \
         near 0 means it took `ca` instead of `CA`"
    );
}

/// 8.7.3.1: a pattern's matrix maps pattern space to the *default* space of
/// the page, so the CTM in force when the stroke happens is not part of it.
///
/// Both pages draw the same 8 pt rule across the same pixels; the second gets
/// there through a doubled CTM and halved coordinates. Anchoring the pattern
/// to the CTM would stretch the second page's gradient by two — its right end
/// would reach only the middle of the ramp — so the bytes are the assertion.
#[test]
fn a_stroked_pattern_does_not_move_with_the_transform() {
    let plain = page(GRADIENT, "/Pattern CS /P0 SCN 8 w 4 20 m 36 20 l S");
    let scaled = page(
        GRADIENT,
        "/Pattern CS /P0 SCN q 2 0 0 2 0 0 cm 4 w 2 10 m 18 10 l S Q",
    );

    // The ramp has to actually run across the rule, or two blank pages would
    // agree and prove nothing.
    let left = pixel(&plain, 5, 20);
    let right = pixel(&plain, 34, 20);
    assert!(
        left.0 > 180 && left.2 < 80,
        "the left end is red, got {left:?}"
    );
    assert!(
        right.2 > 150 && right.0 < 110,
        "the right end is blue, got {right:?}"
    );

    assert_eq!(
        first_difference(&plain, &scaled),
        None,
        "the gradient sits in the same place under both transforms — \
         a difference here is the pattern following the CTM"
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
