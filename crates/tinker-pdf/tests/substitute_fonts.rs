//! A host-supplied face puts text on the page (plans 05, 15).
//!
//! This is the gap that stands between the engine and replacing MuPDF inside
//! Tinker: a document that names Helvetica and embeds nothing extracts its
//! text perfectly and draws none of it, because the engine bundles no faces.
//! The seam that closes it is [`tinker_pdf::FontProvider`], and this proves
//! the whole path — provider to glyph outline to ink on the page.
//!
//! The face is synthesised here rather than read from the system, so the test
//! is identical on every platform and the repository carries no font anyone
//! has to licence.

use std::sync::Arc;

use tinker_pdf::{Document, FontProvider, FontRequest, RenderOptions, SimpleFontProvider};
use tinker_pdf_cos::DocumentBuilder;

/// A TrueType font whose every glyph from 32 upward is the same filled box.
///
/// Enough to answer "did a glyph get drawn", which is the question. Real
/// shapes are the font crate's business and are tested there.
fn boxy_font() -> Vec<u8> {
    // A filled square, as one contour of four on-curve points.
    let mut glyph = Vec::new();
    glyph.extend_from_slice(&1i16.to_be_bytes()); // one contour
    glyph.extend_from_slice(&0i16.to_be_bytes()); // xMin
    glyph.extend_from_slice(&0i16.to_be_bytes()); // yMin
    glyph.extend_from_slice(&700i16.to_be_bytes()); // xMax
    glyph.extend_from_slice(&700i16.to_be_bytes()); // yMax
    glyph.extend_from_slice(&3u16.to_be_bytes()); // last point of contour 0
    glyph.extend_from_slice(&0u16.to_be_bytes()); // no instructions
    glyph.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]); // on-curve, word deltas
    for dx in [0i16, 700, 0, -700] {
        glyph.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, 700, 0] {
        glyph.extend_from_slice(&dy.to_be_bytes());
    }

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long loca

    // Glyphs below 32 are empty; every one from 32 up is the box.
    //
    // `loca` gives each glyph its own slice of `glyf`, so the shape has to be
    // repeated once per glyph rather than pointed at repeatedly. Offsets that
    // merely repeat make every glyph *empty* — a glyph is empty precisely
    // when its entry equals the next — which draws nothing at all and looks
    // exactly like a font that failed to load.
    const FIRST: usize = 32;
    const LAST: usize = 255;
    let size = glyph.len() as u32;

    let mut glyf = Vec::with_capacity(glyph.len() * (LAST + 1 - FIRST));
    for _ in FIRST..=LAST {
        glyf.extend_from_slice(&glyph);
    }

    let mut loca = Vec::new();
    for index in 0..=LAST + 1 {
        let offset = (index.saturating_sub(FIRST)) as u32 * size;
        loca.extend_from_slice(&offset.to_be_bytes());
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

/// A page of base-14 text with no embedded font: the shape of most simple
/// documents, and the one the engine could not draw.
fn document() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F0", 24.0, 20.0, 40.0, "HELLO");
    });
    builder.finish()
}

fn ink(bitmap: &tinker_pdf::Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|p| p[0] < 250)
        .count()
}

#[test]
fn without_a_provider_the_text_is_extracted_but_not_drawn() {
    let doc = Document::open(document()).expect("it opens");
    let page = doc.page(0).expect("a page");

    assert!(
        page.text().plain_text().contains("HELLO"),
        "the metrics are built in, so extraction works regardless"
    );

    let bitmap = page.render(&RenderOptions::default());
    assert_eq!(ink(&bitmap), 0, "and nothing is drawn");
    assert!(
        bitmap
            .warnings
            .contains(&tinker_pdf::RenderWarning::UnreadableFont),
        "the gap is reported rather than silent: {:?}",
        bitmap.warnings
    );
}

#[test]
fn a_supplied_face_puts_the_text_on_the_page() {
    let provider = Arc::new(SimpleFontProvider::new(boxy_font()));
    let doc = Document::open(document())
        .expect("it opens")
        .with_fonts(provider);

    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());

    assert!(
        ink(&bitmap) > 0,
        "the substitute drew nothing, warnings: {:?}",
        bitmap.warnings
    );
    assert!(
        !bitmap
            .warnings
            .contains(&tinker_pdf::RenderWarning::UnreadableFont),
        "and the font is no longer unreadable"
    );
}

/// The glyphs must land where the text is, not merely somewhere. A substitute
/// drawn at the wrong scale or origin still puts ink on the page.
#[test]
fn the_substituted_glyphs_land_where_the_text_is() {
    let provider = Arc::new(SimpleFontProvider::new(boxy_font()));
    let doc = Document::open(document())
        .expect("it opens")
        .with_fonts(provider);
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());

    // The text starts at (20, 40) in a 200x100 page at 72 dpi, so in device
    // space it runs rightward from x=20 and upward from y=60 counting down.
    let mut min_x = u32::MAX;
    let mut max_y = 0u32;
    let components = bitmap.components();
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let at = (y as usize) * bitmap.stride + (x as usize) * components;
            if bitmap.data.get(at).is_some_and(|v| *v < 250) {
                min_x = min_x.min(x);
                max_y = max_y.max(y);
            }
        }
    }

    assert!(min_x != u32::MAX, "there is ink to locate");
    assert!(
        (18..=24).contains(&min_x),
        "the first glyph starts at the text origin, not at {min_x}"
    );
    assert!(
        (55..=65).contains(&max_y),
        "and sits on the baseline, not at {max_y}"
    );
}

/// A provider that declines leaves the page exactly as it was, rather than
/// drawing a placeholder nobody asked for.
#[test]
fn a_declining_provider_changes_nothing() {
    struct Declines;
    impl FontProvider for Declines {
        fn substitute(&self, _request: &FontRequest) -> Option<Arc<Vec<u8>>> {
            None
        }
    }

    let doc = Document::open(document())
        .expect("it opens")
        .with_fonts(Arc::new(Declines));
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());

    assert_eq!(ink(&bitmap), 0);
    assert!(bitmap
        .warnings
        .contains(&tinker_pdf::RenderWarning::UnreadableFont));
}

/// The request has to carry enough for a host to pick a face, and it has to
/// describe the font the document actually named.
#[test]
fn the_provider_is_told_which_font_is_wanted() {
    use std::sync::Mutex;

    struct Records(Mutex<Vec<FontRequest>>);
    impl FontProvider for Records {
        fn substitute(&self, request: &FontRequest) -> Option<Arc<Vec<u8>>> {
            if let Ok(mut seen) = self.0.lock() {
                seen.push(request.clone());
            }
            None
        }
    }

    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Times-BoldItalic");
    builder.add_page(100.0, 50.0, |page| {
        page.text(b"F0", 12.0, 5.0, 20.0, "x");
    });

    let records = Arc::new(Records(Mutex::new(Vec::new())));
    let doc = Document::open(builder.finish())
        .expect("it opens")
        .with_fonts(records.clone());
    let _ = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());

    let seen = records.0.lock().expect("not poisoned").clone();
    assert_eq!(seen.len(), 1, "asked once, got: {seen:?}");
    assert_eq!(seen[0].base_font, "Times-BoldItalic");
    assert!(seen[0].bold && seen[0].italic, "read from the name");
}

/// A document that embeds its own font must keep using it. A substitute is a
/// different face with different outlines, and preferring it would change how
/// a correctly embedded document looks.
#[test]
fn an_embedded_font_is_not_replaced() {
    struct Loud;
    impl FontProvider for Loud {
        fn substitute(&self, _request: &FontRequest) -> Option<Arc<Vec<u8>>> {
            panic!("a document that embeds its font must not ask for a substitute");
        }
    }

    // The fixture embeds nothing, so an embedded-font document is built by
    // hand: a font dictionary whose descriptor carries a FontFile2.
    let program = boxy_font();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"%PDF-1.7\n");
    bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    bytes.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
          /Resources << /Font << /F0 4 0 R >> >> /Contents 6 0 R >>\nendobj\n",
    );
    bytes.extend_from_slice(
        b"4 0 obj\n<< /Type /Font /Subtype /TrueType /BaseFont /Boxy\n\
          /FirstChar 32 /LastChar 126 /FontDescriptor 5 0 R >>\nendobj\n",
    );
    bytes.extend_from_slice(
        b"5 0 obj\n<< /Type /FontDescriptor /FontName /Boxy /Flags 32 /FontFile2 7 0 R >>\nendobj\n",
    );
    let content = b"BT /F0 24 Tf 20 40 Td (HELLO) Tj ET\n";
    bytes.extend_from_slice(
        format!("6 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes(),
    );
    bytes.extend_from_slice(content);
    bytes.extend_from_slice(b"endstream\nendobj\n");
    bytes.extend_from_slice(
        format!("7 0 obj\n<< /Length {} >>\nstream\n", program.len()).as_bytes(),
    );
    bytes.extend_from_slice(&program);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(b"trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n");

    let doc = Document::open(bytes)
        .expect("it opens")
        .with_fonts(Arc::new(Loud));
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());

    assert!(ink(&bitmap) > 0, "and the embedded font still draws");
}
