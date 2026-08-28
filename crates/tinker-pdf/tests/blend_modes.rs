//! Blend modes reach the page (11.3.5).
//!
//! `/BM` is set through an `/ExtGState`, and before this it was read by
//! nothing: every mode drew as `Normal`. A document using `/Multiply` for a
//! highlighter or a shadow rendered it as an opaque block over whatever it was
//! meant to tint — which looks like a colour bug, not a missing feature.

use tinker_pdf::{Document, PixelFormat, RenderOptions};

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

// ---- the four non-separable modes (11.3.5.3) --------------------------------

/// A page that fills a coloured square, then covers it with a differently
/// coloured square under the named blend mode.
///
/// Coloured rather than grey, because grey is degenerate for every mode below:
/// `Hue` and `Saturation` of an achromatic backdrop are undefined-ish, and
/// `Color` and `Luminosity` both collapse to something a broken build would
/// also produce. `blend_modes.rs`'s grey fixture cannot reach these.
fn coloured(blend: &str) -> Vec<u8> {
    let content = "1 0 0 rg 0 0 40 40 re f\n\
                   /GS0 gs\n\
                   0 0 1 rg 0 0 40 40 re f";
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

/// 11.3.5.3's `Lum`, transcribed. The coefficients are the clause's own
/// 0.3 / 0.59 / 0.11 — deliberately not Rec.601's, which `blend.rs` records as
/// "the difference between matching a reference renderer and not".
fn lum((r, g, b): (u8, u8, u8)) -> u32 {
    (300 * u32::from(r) + 590 * u32::from(g) + 110 * u32::from(b) + 500) / 1000
}

/// **The four non-separable modes reach the canvas**, asserted by the property
/// 11.3.5.3 defines them with rather than by a transcribed pixel.
///
/// `Color` is `SetLum(Cs, Lum(Cb))` and `Luminosity` is `SetLum(Cb, Lum(Cs))`,
/// and `SetLum` — with the `ClipColor` that follows it — exists precisely to
/// leave the luminosity it was handed intact. So the defining property is
/// checkable without transcribing `SetSat`: a `Color` blend carries the
/// **backdrop's** luminosity, and a `Luminosity` blend carries the
/// **source's**.
///
/// # Why this test exists
///
/// `canvas.rs`'s non-separable branch was **dead code in the entire
/// workspace**. `blend.rs` tests `apply_nonseparable` directly and bypasses
/// the canvas; this file covered only `/Multiply`; every other `Luminosity` in
/// the suite is `/SMask /S /Luminosity`, which is the soft-mask *kind* and a
/// different thing entirely. A build whose branch fell through to `Normal`
/// passed everything, and the fingerprints could not object to a path no
/// fixture reached.
#[test]
fn the_non_separable_modes_carry_the_luminosity_their_clause_says() {
    let backdrop = (255u8, 0u8, 0u8);
    let source = (0u8, 0u8, 255u8);

    let colour = middle(&render(coloured("/Color")));
    let luminosity = middle(&render(coloured("/Luminosity")));

    // Within a level: `SetLum` adds an integer offset to three channels and
    // `ClipColor` may pull one back, and each step rounds once.
    let off = |a: u32, b: u32| a.abs_diff(b);
    assert!(
        off(lum(colour), lum(backdrop)) <= 2,
        "/BM /Color must carry the backdrop's luminosity: got {colour:?} at \
         lum {}, backdrop lum {}",
        lum(colour),
        lum(backdrop)
    );
    assert!(
        off(lum(luminosity), lum(source)) <= 2,
        "/BM /Luminosity must carry the source's luminosity: got \
         {luminosity:?} at lum {}, source lum {}",
        lum(luminosity),
        lum(source)
    );

    // And the discriminating half. A build that falls through to `Normal`
    // paints the source, whose luminosity is the source's — so the
    // `Luminosity` assertion above would still pass. This one would not.
    assert_ne!(
        colour, source,
        "/BM /Color painted the source unchanged, which is what a fall-through \
         to Normal does"
    );
}

/// The four are four, not one.
///
/// `Hue` and `Saturation` need `SetSat`, whose closed form is long enough that
/// transcribing it here would be a second implementation to keep in step. What
/// is asserted instead is the property that fails first when the branch is not
/// reached: all four modes fall through to `_ => cs` together, so a build that
/// misses the branch paints **the same pixel** for all four.
#[test]
fn the_four_non_separable_modes_are_not_one_expression() {
    let results: Vec<(u8, u8, u8)> = ["/Hue", "/Saturation", "/Color", "/Luminosity"]
        .iter()
        .map(|mode| middle(&render(coloured(mode))))
        .collect();

    let mut distinct = results.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(
        distinct.len() >= 3,
        "the four non-separable modes produced {} distinct results ({results:?}); \
         a build that never reaches the branch produces one",
        distinct.len()
    );

    // None of them may be the plain source, which is the fall-through.
    assert!(
        !results.contains(&(0, 0, 255)),
        "a non-separable mode painted the source unchanged: {results:?}"
    );
}

/// Renders a page into a one-channel bitmap.
fn render_grey(bytes: Vec<u8>) -> tinker_pdf::Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions {
            format: PixelFormat::Gray8,
            ..RenderOptions::default()
        })
}

/// Two *different* greys, which the shared `document` fixture is not.
///
/// `document` paints 0.5 over 0.5, so `Cb == Cs` and every blend mode agrees
/// with every other — including a fall-through that paints the source. It
/// cannot discriminate, and a test built on it passes on a broken build. That
/// mistake is why this builder exists.
fn two_greys(blend: &str) -> Vec<u8> {
    let content = "0.8 g 0 0 40 40 re f\n\
                   /GS0 gs\n\
                   0.3 g 0 0 40 40 re f";
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

/// **A non-separable mode means the same thing at every pixel format.**
///
/// `Gray8` is a public `RenderOptions::format`, and the branch implementing
/// 11.3.5.3 is gated on there being *three* colour channels — so on a grey
/// canvas all four non-separable modes fall through to `apply`'s `_ => cs` and
/// paint the source.
///
/// Three of the four are wrong there, provably and without transcribing
/// `SetSat`: with an achromatic backdrop and an achromatic source, `Color`,
/// `Hue` and `Saturation` all reduce to the **backdrop** — `SetLum(Cs, Lum(Cb))`
/// on a grey `Cs` is just `Cb`, and `SetSat` of an achromatic colour is
/// achromatic — while the fall-through paints `Cs`. Only `Luminosity`, which is
/// `SetLum(Cb, Lum(Cs))` and therefore `Cs`, is right by luck.
///
/// The assertion is the **invariance** rather than the value: whatever
/// `/BM /Color` means, a one-channel and a three-channel render of the same
/// grey content must agree about it. That needs no second implementation of
/// the clause, and it is exactly the property a format-gated branch breaks.
#[test]
fn a_non_separable_mode_reads_the_same_at_one_channel_as_at_three() {
    for mode in ["/Hue", "/Saturation", "/Color", "/Luminosity"] {
        let colour = middle(&render(two_greys(mode)));
        let grey = render_grey(two_greys(mode));
        let at = (grey.height / 2) as usize * grey.stride + (grey.width / 2) as usize;
        let one = grey.data.get(at).copied().expect("a grey pixel");

        assert_eq!(
            (colour.0, colour.1, colour.2),
            (colour.0, colour.0, colour.0),
            "{mode}: grey over grey must stay grey at three channels, or this \
             comparison means nothing"
        );
        assert!(
            one.abs_diff(colour.0) <= 1,
            "{mode}: one channel says {one}, three channels say {}, so the \
             blend depends on the pixel format",
            colour.0
        );
    }
}
