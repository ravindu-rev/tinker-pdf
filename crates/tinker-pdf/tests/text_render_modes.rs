//! `Tr` decides fill, stroke and clip independently (9.3.6, Table 106).
//!
//! `show_glyph` checked only whether the mode painted *anything* and then
//! filled, so mode 1 (stroke only) came out solid in the fill colour, mode 2
//! never stroked, and modes 4–7 never clipped. `TextRenderMode::clips` existed,
//! was documented and unit-tested, and was called by nothing outside its own
//! test.

use std::sync::Arc;

use tinker_pdf::{Document, RenderOptions, SimpleFontProvider};
use tinker_pdf_cos::DocumentBuilder;

/// A face whose every glyph from 32 up is a filled 700-unit box, so "did this
/// glyph paint, and in what colour" has an unambiguous answer.
fn boxy_font() -> Vec<u8> {
    let mut glyph = Vec::new();
    glyph.extend_from_slice(&1i16.to_be_bytes());
    glyph.extend_from_slice(&0i16.to_be_bytes());
    glyph.extend_from_slice(&0i16.to_be_bytes());
    glyph.extend_from_slice(&700i16.to_be_bytes());
    glyph.extend_from_slice(&700i16.to_be_bytes());
    glyph.extend_from_slice(&3u16.to_be_bytes());
    glyph.extend_from_slice(&0u16.to_be_bytes());
    glyph.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]);
    for dx in [0i16, 700, 0, -700] {
        glyph.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, 700, 0] {
        glyph.extend_from_slice(&dy.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());

    const FIRST: usize = 32;
    const LAST: usize = 255;
    let size = glyph.len() as u32;
    let mut glyf = Vec::new();
    for _ in FIRST..=LAST {
        glyf.extend_from_slice(&glyph);
    }
    let mut loca = Vec::new();
    for index in 0..=LAST + 1 {
        loca.extend_from_slice(&((index.saturating_sub(FIRST)) as u32 * size).to_be_bytes());
    }

    let tables: [(&[u8; 4], &[u8]); 3] = [(b"head", &head), (b"loca", &loca), (b"glyf", &glyf)];
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

fn render(content: &str) -> tinker_pdf::Bitmap {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(60.0, 60.0, |page| {
        page.raw(content.as_bytes());
    });

    Document::open(builder.finish())
        .expect("it opens")
        .with_fonts(Arc::new(SimpleFontProvider::new(boxy_font())))
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn pixel(bitmap: &tinker_pdf::Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[0, 0, 0]);
    (p[0], p[1], p[2])
}

fn inked(bitmap: &tinker_pdf::Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|p| !(p[0] > 240 && p[1] > 240 && p[2] > 240))
        .count()
}

/// Mode 0 fills in the fill colour: the baseline the rest are compared to.
#[test]
fn mode_zero_fills() {
    let bitmap = render("BT 1 0 0 rg 0 0 1 RG /F0 40 Tf 5 10 Td 0 Tr (A) Tj ET");
    let (r, g, b) = pixel(&bitmap, 20, 30);
    assert!(
        r > 200 && g < 60 && b < 60,
        "filled red, got ({r}, {g}, {b})"
    );
}

/// Mode 1 strokes the outline and leaves the interior alone. Filling it —
/// which is what happened — paints a solid box in the wrong colour entirely.
#[test]
fn mode_one_strokes_without_filling() {
    let outlined = render("BT 1 0 0 rg 0 0 1 RG /F0 40 Tf 5 10 Td 2 w 1 Tr (A) Tj ET");

    let interior = pixel(&outlined, 20, 30);
    assert!(
        interior.0 > 200 && interior.1 > 200 && interior.2 > 200,
        "the interior stays blank, got {interior:?}"
    );
    assert!(inked(&outlined) > 0, "and the outline is drawn");

    let filled = render("BT 1 0 0 rg 0 0 1 RG /F0 40 Tf 5 10 Td 0 Tr (A) Tj ET");
    assert!(
        inked(&outlined) < inked(&filled),
        "an outline covers less than a fill: {} against {}",
        inked(&outlined),
        inked(&filled)
    );
}

/// Mode 2 does both, so it covers at least as much as a fill alone.
#[test]
fn mode_two_fills_and_strokes() {
    let both = render("BT 1 0 0 rg 0 0 1 RG /F0 40 Tf 5 10 Td 2 w 2 Tr (A) Tj ET");
    let fill_only = render("BT 1 0 0 rg 0 0 1 RG /F0 40 Tf 5 10 Td 0 Tr (A) Tj ET");
    assert!(
        inked(&both) >= inked(&fill_only),
        "{} against {}",
        inked(&both),
        inked(&fill_only)
    );
}

/// Mode 3 paints nothing: the invisible layer a scanned page puts its OCR
/// text in.
#[test]
fn mode_three_paints_nothing() {
    let bitmap = render("BT 1 0 0 rg /F0 40 Tf 5 10 Td 3 Tr (A) Tj ET");
    assert_eq!(inked(&bitmap), 0);
}

/// Mode 7 adds the glyphs to the clip and paints nothing itself, so a fill
/// after `ET` shows only through the glyph shapes.
#[test]
fn mode_seven_clips_what_follows() {
    let clipped = render("q BT /F0 40 Tf 5 10 Td 7 Tr (A) Tj ET 0 1 0 rg 0 0 60 60 re f Q");
    let unclipped = render("q 0 1 0 rg 0 0 60 60 re f Q");

    assert!(inked(&clipped) > 0, "the glyph area is painted");
    assert!(
        inked(&clipped) < inked(&unclipped) / 2,
        "but only the glyph area: {} against a whole page at {}",
        inked(&clipped),
        inked(&unclipped)
    );
}

/// Mode 4 fills *and* clips, so the glyph is painted and later content is
/// still restricted to it.
#[test]
fn mode_four_fills_and_clips() {
    let bitmap = render("q BT 1 0 0 rg /F0 40 Tf 5 10 Td 4 Tr (A) Tj ET 0 1 0 rg 0 0 60 60 re f Q");

    let (r, g, b) = pixel(&bitmap, 20, 30);
    assert!(
        g > 200 && r < 80 && b < 80,
        "green shows through the glyph, got ({r}, {g}, {b})"
    );

    let corner = pixel(&bitmap, 58, 2);
    assert!(
        corner.0 > 240 && corner.1 > 240,
        "and the rest of the page is untouched, got {corner:?}"
    );
}

/// The clip takes effect at `ET`, not per glyph. Applying it glyph by glyph
/// would leave the second clipped to the first, and the result empty.
#[test]
fn several_clipped_glyphs_union_rather_than_intersect() {
    let one = render("q BT /F0 30 Tf 3 15 Td 7 Tr (A) Tj ET 0 0 0 rg 0 0 60 60 re f Q");
    let two = render("q BT /F0 30 Tf 3 15 Td 7 Tr (AA) Tj ET 0 0 0 rg 0 0 60 60 re f Q");
    assert!(
        inked(&two) > inked(&one),
        "two glyphs clip a larger area than one: {} against {}",
        inked(&two),
        inked(&one)
    );
}
