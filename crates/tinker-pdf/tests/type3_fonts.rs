//! Type 3 glyphs are content streams, and they run (9.6.5).
//!
//! Every other font hands back an outline. A Type 3 glyph is a *procedure*: it
//! can fill, stroke, draw an image, do anything a page can. There was no path
//! for that at all, so a Type 3 font drew nothing — and, unlike a missing font
//! program, it did not even report `UnreadableFont`, because nothing was
//! missing. The document was complete and the glyphs were simply skipped.

use tinker_pdf::{Document, RenderOptions};

/// A one-page document with one Type 3 font, whose glyph `a` runs `proc`.
///
/// `matrix` is the `/FontMatrix`; a Type 3 font picks its own glyph-space
/// units, which is why the entry is required rather than assumed to be 1/1000.
fn document(proc_bytes: &str, matrix: &str, content: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"%PDF-1.7\n");
    bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    bytes.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100]\n\
          /Resources << /Font << /F0 4 0 R >> >> /Contents 7 0 R >>\nendobj\n",
    );
    bytes.extend_from_slice(
        format!(
            "4 0 obj\n<< /Type /Font /Subtype /Type3 /FontBBox [0 0 1000 1000]\n\
             /FontMatrix {matrix}\n\
             /CharProcs << /a 5 0 R >>\n\
             /Encoding << /Type /Encoding /Differences [97 /a] >>\n\
             /FirstChar 97 /LastChar 97 /Widths [1000] /Resources << >> >>\nendobj\n"
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(
        format!(
            "5 0 obj\n<< /Length {} >>\nstream\n{proc_bytes}\nendstream\nendobj\n",
            proc_bytes.len() + 1
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(
        format!(
            "7 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
            content.len() + 1
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(b"trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");
    bytes
}

fn render(bytes: Vec<u8>) -> tinker_pdf::Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn inked(bitmap: &tinker_pdf::Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|p| p[0] < 200)
        .count()
}

fn pixel(bitmap: &tinker_pdf::Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[255, 255, 255]);
    (p[0], p[1], p[2])
}

/// The headline: a glyph whose procedure fills a box actually fills one.
#[test]
fn a_glyph_procedure_runs() {
    // `d0` declares the width; the box is the whole glyph square.
    let bitmap = render(document(
        "1000 0 d0 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 50 Tf 10 20 Td (a) Tj ET",
    ));

    assert!(
        inked(&bitmap) > 500,
        "the procedure drew nothing: {} inked pixels",
        inked(&bitmap)
    );
}

/// The `/FontMatrix` maps glyph space to text space, so halving it halves the
/// glyph. Ignoring it would draw both at the same size — or, at 1/1000
/// assumed, draw the second one a thousand times too large.
#[test]
fn the_font_matrix_scales_the_glyph() {
    let big = render(document(
        "1000 0 d0 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 50 Tf 10 20 Td (a) Tj ET",
    ));
    let small = render(document(
        "1000 0 d0 0 0 1000 1000 re f",
        "[0.0005 0 0 0.0005 0 0]",
        "BT /F0 50 Tf 10 20 Td (a) Tj ET",
    ));

    let (big, small) = (inked(&big), inked(&small));
    let ratio = big as f64 / small.max(1) as f64;
    assert!(
        (3.0..5.0).contains(&ratio),
        "half the matrix is a quarter the area: {big} against {small}"
    );
}

/// The glyph lands where the text is, not at the page origin.
#[test]
fn a_glyph_lands_at_the_text_position() {
    let bitmap = render(document(
        "1000 0 d0 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 20 Tf 60 70 Td (a) Tj ET",
    ));

    // Text at (60, 70) with a 20-unit em: the glyph occupies roughly
    // x 60..80, y 70..90 in user space — the upper right, and the raster
    // counts y downward.
    let inside = pixel(&bitmap, 70, 100 - 80);
    assert!(
        inside.0 < 200,
        "the glyph is up and to the right: {inside:?}"
    );

    let origin = pixel(&bitmap, 5, 95);
    assert!(
        origin.0 > 200,
        "and nothing was drawn at the page origin: {origin:?}"
    );
}

/// A procedure may use the whole graphics language, including colour — this
/// is the point of Type 3 and what separates it from an outline font.
#[test]
fn a_procedure_may_set_its_own_colour() {
    let bitmap = render(document(
        "1000 0 d0 1 0 0 rg 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 50 Tf 10 20 Td (a) Tj ET",
    ));

    let (r, g, b) = pixel(&bitmap, 30, 55);
    assert!(
        r > 200 && g < 60 && b < 60,
        "the glyph painted its own red, got ({r}, {g}, {b})"
    );
}

/// A procedure must not leak its state onto the page. `q`/`Q` inside a glyph
/// is the caller's business, but the glyph's own transform and colour are not.
#[test]
fn a_procedure_does_not_leak_its_state() {
    let bitmap = render(document(
        "1000 0 d0 1 0 0 rg 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 20 Tf 10 70 Td (a) Tj ET 0 0 10 10 re f",
    ));

    // The rectangle after `ET` is drawn in the page's colour, which is still
    // black — the glyph's red must not have escaped.
    let (r, g, b) = pixel(&bitmap, 5, 95);
    assert!(
        r < 60 && g < 60 && b < 60,
        "the page kept its own colour, got ({r}, {g}, {b})"
    );
}

/// Two glyphs in a row advance by `/Widths` through the font matrix, not by
/// the 1/1000 every other font uses.
#[test]
fn glyphs_advance_through_the_font_matrix() {
    let one = render(document(
        "1000 0 d0 0 0 400 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 20 Tf 10 40 Td (a) Tj ET",
    ));
    let two = render(document(
        "1000 0 d0 0 0 400 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 20 Tf 10 40 Td (aa) Tj ET",
    ));

    let (one, two) = (inked(&one), inked(&two));
    assert!(
        two > one + one / 2,
        "the second glyph landed beside the first rather than on it: \
         {two} against {one}"
    );
}

/// A code with no entry in `/Differences` names no procedure, so nothing is
/// drawn and nothing panics.
#[test]
fn an_unmapped_code_draws_nothing() {
    let bitmap = render(document(
        "1000 0 d0 0 0 1000 1000 re f",
        "[0.001 0 0 0.001 0 0]",
        "BT /F0 50 Tf 10 20 Td (b) Tj ET",
    ));
    assert_eq!(inked(&bitmap), 0);
}
