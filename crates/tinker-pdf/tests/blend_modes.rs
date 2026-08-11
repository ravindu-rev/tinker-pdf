//! Blend modes reach the page (11.3.5).
//!
//! `/BM` is set through an `/ExtGState`, and before this it was read by
//! nothing: every mode drew as `Normal`. A document using `/Multiply` for a
//! highlighter or a shadow rendered it as an opaque block over whatever it was
//! meant to tint — which looks like a colour bug, not a missing feature.

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

/// A page that fills a mid-grey square, then covers it with a mid-grey square
/// under the named blend mode.
fn document(blend: &str) -> Vec<u8> {
    let content = "0.5 0.5 0.5 rg 0 0 40 40 re f\n\
                   /GS0 gs\n\
                   0.5 0.5 0.5 rg 0 0 40 40 re f";
    let page = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << /ExtGState << /GS0 << /BM {blend} >> >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    );
    page.into_bytes()
}

fn middle(bitmap: &tinker_pdf::Bitmap) -> (u8, u8, u8) {
    pixel(bitmap, bitmap.width / 2, bitmap.height / 2)
}

/// The headline. Mid grey over mid grey is mid grey under `Normal` and
/// distinctly darker under `Multiply`; if `/BM` were still ignored the two
/// would be identical.
#[test]
fn multiply_darkens_where_normal_does_not() {
    let normal = middle(&render(document("/Normal")));
    let multiply = middle(&render(document("/Multiply")));

    assert!(
        (120..=136).contains(&normal.0),
        "normal leaves mid grey alone: {normal:?}"
    );
    assert!(
        multiply.0 < 80,
        "multiply darkens it: {multiply:?} against {normal:?}"
    );
}

#[test]
fn screen_lightens() {
    let screen = middle(&render(document("/Screen")));
    assert!(screen.0 > 180, "mid over mid screens brighter: {screen:?}");
}

#[test]
fn darken_and_lighten_go_opposite_ways() {
    let darken = middle(&render(document("/Darken")));
    let lighten = middle(&render(document("/Lighten")));
    // Both squares are the same grey, so both modes return it — the point is
    // that neither inverts, which a swapped min/max would.
    assert!((120..=136).contains(&darken.0), "{darken:?}");
    assert!((120..=136).contains(&lighten.0), "{lighten:?}");
}

#[test]
fn difference_of_a_colour_with_itself_is_black() {
    let difference = middle(&render(document("/Difference")));
    assert!(
        difference.0 < 12 && difference.1 < 12 && difference.2 < 12,
        "a colour differenced with itself cancels: {difference:?}"
    );
}

/// 11.3.5: `/Compatible` is a synonym for `/Normal`, and a mode from a later
/// specification must render as `Normal` rather than refusing to draw.
#[test]
fn an_unknown_mode_falls_back_to_normal() {
    let normal = middle(&render(document("/Normal")));
    for name in ["/Compatible", "/SomeFutureMode"] {
        let other = middle(&render(document(name)));
        assert_eq!(other, normal, "{name} renders as Normal");
    }
}

/// The array form offers fallbacks, and the first *recognised* entry wins —
/// taking the first entry regardless would select a mode nobody implements
/// and silently render Normal.
#[test]
fn the_array_form_takes_the_first_mode_it_knows() {
    let multiply = middle(&render(document("/Multiply")));
    let array = middle(&render(document("[/NotAMode /Multiply /Screen]")));
    assert_eq!(array, multiply, "it skipped past the unknown name");
}

/// A `gs` that sets only `/ca` must not reset the blend mode to Normal —
/// which is why the seam reports "unchanged" separately from "Normal".
#[test]
fn setting_only_the_alpha_leaves_the_blend_mode_alone() {
    let content = "0.5 0.5 0.5 rg 0 0 40 40 re f\n\
                   /GS0 gs\n\
                   /GS1 gs\n\
                   0.5 0.5 0.5 rg 0 0 40 40 re f";
    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 40]\n\
   /Resources << /ExtGState << /GS0 << /BM /Multiply >>\n\
                              /GS1 << /ca 1 >> >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes();

    let after = middle(&render(bytes));
    assert!(
        after.0 < 80,
        "the multiply survived a gs that touched only the alpha: {after:?}"
    );
}
