//! A host-supplied face puts text on the page (plans 05, 15).
//!
//! This is the gap that stands between the engine and drawing text when no
//! fonts are embedded: a document that names Helvetica and embeds nothing
//! extracts its text perfectly and draws none of it, because the engine
//! bundles no faces and the host supplies them.
//! The seam that closes it is [`tinker_pdf::FontProvider`], and this proves
//! the whole path — provider to glyph outline to ink on the page.
//!
//! The face is synthesised here rather than read from the system, so the test
//! is identical on every platform and the repository carries no font anyone
//! has to licence.

mod written_support;

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
    built("document", builder)
}

fn ink(bitmap: &tinker_pdf::Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|p| p[0] < 250)
        .count()
}

/// Without a provider **and without the bundled faces**, which is what this
/// crate's default build is. `cfg`-gated rather than deleted: it is still the
/// claim a default build makes, and a `bundled-fonts` build makes the opposite
/// one three tests below.
#[test]
#[cfg(not(feature = "bundled-fonts"))]
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
///
/// Not true of a `bundled-fonts` build, and deliberately so: there, a
/// declining provider falls through to the faces this build carries, which is
/// the whole point of the chain.
#[test]
#[cfg(not(feature = "bundled-fonts"))]
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
    let doc = Document::open(built("substitute", builder))
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

/// Adobe's stream cipher, encrypting, so a test can build a Type 1 program
/// the way a font tool would.
fn t1_encrypt(plain: &[u8], key: u16, pad: usize) -> Vec<u8> {
    const C1: u16 = 52_845;
    const C2: u16 = 22_719;

    let mut r = key;
    let mut out = Vec::new();
    let mut input = vec![0x55u8; pad];
    input.extend_from_slice(plain);

    for byte in input {
        let cipher = byte ^ (r >> 8) as u8;
        r = (u16::from(cipher).wrapping_add(r))
            .wrapping_mul(C1)
            .wrapping_add(C2);
        out.push(cipher);
    }
    out
}

/// A Type 1 program whose glyph `A` is a filled 600-unit box.
fn type1_program() -> Vec<u8> {
    let num = |v: i32, out: &mut Vec<u8>| {
        if (-107..=107).contains(&v) {
            out.push((v + 139) as u8);
        } else if (108..=1131).contains(&v) {
            let v = v - 108;
            out.push((v / 256 + 247) as u8);
            out.push((v % 256) as u8);
        } else {
            out.push(255);
            out.extend_from_slice(&v.to_be_bytes());
        }
    };

    let mut cs = Vec::new();
    num(0, &mut cs);
    num(700, &mut cs);
    cs.push(13); // hsbw: no side bearing, 700 wide
    num(0, &mut cs);
    num(0, &mut cs);
    cs.push(21); // rmoveto
    num(600, &mut cs);
    cs.push(6); // hlineto
    num(600, &mut cs);
    cs.push(7); // vlineto
    num(-600, &mut cs);
    cs.push(6); // hlineto
    cs.push(9); // closepath
    cs.push(14); // endchar

    let encrypted = t1_encrypt(&cs, 4330, 4);

    let mut private = Vec::new();
    private.extend_from_slice(
        b"dup /Private 8 dict dup begin
/lenIV 4 def
",
    );
    private.extend_from_slice(
        b"/CharStrings 1 dict dup begin
",
    );
    private.extend_from_slice(format!("/A {} RD ", encrypted.len()).as_bytes());
    private.extend_from_slice(&encrypted);
    private.extend_from_slice(
        b" ND
end
end
",
    );

    let mut out = Vec::new();
    out.extend_from_slice(
        b"%!PS-AdobeFont-1.0: Boxy
",
    );
    out.extend_from_slice(
        b"/FontMatrix [0.001 0 0 0.001 0 0] readonly def
",
    );
    out.extend_from_slice(
        b"/Encoding 256 array
dup 65 /A put
readonly def
",
    );
    out.extend_from_slice(
        b"currentfile eexec
",
    );
    out.extend_from_slice(&t1_encrypt(&private, 55_665, 4));
    out
}

/// 9.9: `/FontFile` is a Type 1 program. Before this existed its bytes went to
/// the sfnt and CFF parsers, both of which declined them correctly — so an
/// embedded Type 1 font drew nothing at all and reported only that some font
/// was unreadable.
#[test]
fn an_embedded_type1_font_draws() {
    let program = type1_program();

    let mut bytes = Vec::new();
    bytes.extend_from_slice(
        b"%PDF-1.7
",
    );
    bytes.extend_from_slice(
        b"1 0 obj
<< /Type /Catalog /Pages 2 0 R >>
endobj
",
    );
    bytes.extend_from_slice(
        b"2 0 obj
<< /Type /Pages /Count 1 /Kids [3 0 R] >>
endobj
",
    );
    bytes.extend_from_slice(
        b"3 0 obj
<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 60]
          /Resources << /Font << /F0 4 0 R >> >> /Contents 6 0 R >>
endobj
",
    );
    bytes.extend_from_slice(
        b"4 0 obj
<< /Type /Font /Subtype /Type1 /BaseFont /Boxy
          /FirstChar 65 /LastChar 65 /Widths [700] /FontDescriptor 5 0 R >>
endobj
",
    );
    bytes.extend_from_slice(
        b"5 0 obj
<< /Type /FontDescriptor /FontName /Boxy /Flags 4 /FontFile 7 0 R >>
endobj
",
    );
    let content = b"BT /F0 40 Tf 10 10 Td (A) Tj ET
";
    bytes.extend_from_slice(
        format!(
            "6 0 obj
<< /Length {} >>
stream
",
            content.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(content);
    bytes.extend_from_slice(
        b"endstream
endobj
",
    );
    bytes.extend_from_slice(
        format!(
            "7 0 obj
<< /Length {} /Length1 {} >>
stream
",
            program.len(),
            program.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(&program);
    bytes.extend_from_slice(
        b"
endstream
endobj
",
    );
    bytes.extend_from_slice(
        b"trailer
<< /Size 8 /Root 1 0 R >>
%%EOF
",
    );

    let doc = Document::open(bytes).expect("it opens");
    let page = doc.page(0).expect("a page");
    let bitmap = page.render(&RenderOptions::default());

    assert!(
        ink(&bitmap) > 0,
        "the Type 1 glyph drew nothing, warnings: {:?}",
        bitmap.warnings
    );
    assert!(
        !bitmap
            .warnings
            .contains(&tinker_pdf::RenderWarning::UnreadableFont),
        "and the font is no longer unreadable"
    );
}

// ---- the bundled faces (roadmap Tier 1) ----------------------------------
//
// Everything above is about the seam: a face the *host* supplies. These are
// about the other answer, for the host that has none — the twelve Liberation
// faces a `bundled-fonts` build carries. The tests are `cfg`-gated rather than
// skipped at run time, so a build without the feature does not compile them
// and cannot report them as passing.

/// **A base-14 document draws its text with no provider installed.**
///
/// The exact document the first test in this file renders blank. That pair is
/// the whole claim of the feature, and it is why the two live in one file: the
/// gap and its closing are the same fixture, one `--features` apart.
#[test]
#[cfg(feature = "bundled-fonts")]
fn a_bundled_build_draws_base_fourteen_text_with_no_provider() {
    let doc = Document::open(document()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let bitmap = page.render(&RenderOptions::default());

    assert!(
        ink(&bitmap) > 0,
        "nothing was drawn, warnings: {:?}",
        bitmap.warnings
    );
    assert!(
        !bitmap
            .warnings
            .contains(&tinker_pdf::RenderWarning::UnreadableFont),
        "and the font is not reported unreadable: {:?}",
        bitmap.warnings
    );
}

/// **A host provider still wins**, and the bundled set answers only what it
/// declines.
///
/// Turning the feature on must not take a face away from anybody. The check is
/// per *request* rather than per document: this provider answers for the
/// regular face and declines symbolic fonts, and both halves have to hold at
/// once.
#[test]
#[cfg(feature = "bundled-fonts")]
fn a_host_provider_is_asked_first_and_the_bundled_faces_only_after() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counting(AtomicUsize);
    impl FontProvider for Counting {
        fn substitute(&self, _: &FontRequest) -> Option<Arc<Vec<u8>>> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Some(Arc::new(boxy_font()))
        }
    }

    let provider = Arc::new(Counting(AtomicUsize::new(0)));
    let doc = Document::open(document())
        .expect("it opens")
        .with_fonts(provider.clone());
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());

    assert!(provider.0.load(Ordering::Relaxed) > 0, "the host was asked");
    assert!(ink(&bitmap) > 0, "and its face drew: {:?}", bitmap.warnings);

    // The host's face is the boxy one, so every glyph is a filled square and
    // the page is far darker than Liberation would make it. Ink count is the
    // cheapest way to tell the two apart without a fingerprint.
    let bundled = Document::open(document())
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    assert!(
        ink(&bitmap) > ink(&bundled),
        "the host's boxes ({}) should out-ink Liberation ({})",
        ink(&bitmap),
        ink(&bundled)
    );
}

/// **A symbolic font is still declined**, feature or no feature.
///
/// Symbol and ZapfDingbats are two of the standard 14 and neither has a
/// Liberation equivalent. Drawing a text face for one puts letters where the
/// document meant arrows: legible, plausible and wrong, which is worse than
/// the gap it replaces. So this is the one row of the standard 14 the feature
/// deliberately does not close, and it is asserted rather than assumed.
#[test]
#[cfg(feature = "bundled-fonts")]
fn a_symbolic_font_is_declined_by_the_bundled_faces_too() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Symbol");
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F0", 24.0, 20.0, 40.0, "abgd");
    });

    let doc = Document::open(built(
        "a symbolic font is declined by the bundled faces too",
        builder,
    ))
    .expect("it opens");
    let bitmap = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    assert_eq!(ink(&bitmap), 0, "a symbolic font draws nothing");
    assert!(
        bitmap
            .warnings
            .contains(&tinker_pdf::RenderWarning::UnreadableFont),
        "and says so: {:?}",
        bitmap.warnings
    );
}

/// What `builder` writes, held to the validate-on-save helper as a document
/// this engine wrote from nothing it did not write.
#[track_caller]
fn built(label: &str, builder: tinker_pdf::DocumentBuilder) -> Vec<u8> {
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}

/// A page showing two CIDs in `font`, a Type 0 font of `encoding` over a
/// CIDFontType0 descendant whose `/CIDSystemInfo` is `info` and which embeds
/// no program: the shape of an unembedded CJK font. Written by hand, because
/// the builder embeds every font it writes.
fn composite_page(encoding: &str, info: &str, content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
         /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /KozMinPr6N-Regular \
             /Encoding {encoding} /DescendantFonts [6 0 R] >>"
        ),
        format!(
            "<< /Type /Font /Subtype /CIDFontType0 /BaseFont /KozMinPr6N-Regular \
             {info} /FontDescriptor 7 0 R /DW 1000 >>"
        ),
        "<< /Type /FontDescriptor /FontName /KozMinPr6N-Regular /Flags 4 \
         /FontBBox [0 -120 1000 880] /ItalicAngle 0 /Ascent 880 /Descent -120 \
         /CapHeight 740 /StemV 80 >>"
            .to_string(),
    ];
    let mut out = String::from("%PDF-1.7\n");
    for (index, object) in objects.iter().enumerate() {
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

/// **The provider is told the character collection** (FT-01.2): an
/// unembedded Adobe-Japan1 font's request carries the registry, the
/// ordering and the supplement its descendant names, so a provider can
/// choose a face by the characters its CIDs stand for. A recording provider
/// is the host's, asked before any bundled face, so the same holds in the
/// default and the `bundled-fonts` builds.
#[test]
fn the_provider_is_told_the_character_collection() {
    use std::sync::Mutex;
    use tinker_pdf::CharacterCollection;

    struct Records(Mutex<Vec<FontRequest>>);
    impl FontProvider for Records {
        fn substitute(&self, request: &FontRequest) -> Option<Arc<Vec<u8>>> {
            if let Ok(mut seen) = self.0.lock() {
                seen.push(request.clone());
            }
            None
        }
    }

    let asked = |info: &str| {
        let records = Arc::new(Records(Mutex::new(Vec::new())));
        let doc = Document::open(composite_page(
            "/Identity-H",
            info,
            "BT /F0 24 Tf 20 40 Td <00410042> Tj ET",
        ))
        .expect("it opens")
        .with_fonts(records.clone());
        let _ = doc
            .page(0)
            .expect("a page")
            .render(&RenderOptions::default());
        let seen = records.0.lock().expect("not poisoned").clone();
        assert_eq!(seen.len(), 1, "asked once for the font, got: {seen:?}");
        seen.into_iter().next().expect("one request")
    };

    let request = asked("/CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 6 >>");
    assert_eq!(request.base_font, "KozMinPr6N-Regular");
    assert_eq!(
        request.character_collection,
        Some(CharacterCollection {
            registry: "Adobe".to_string(),
            ordering: "Japan1".to_string(),
            supplement: Some(6),
        })
    );

    // Leniently, as rendering reads it: names for strings, no supplement.
    let request = asked("/CIDSystemInfo << /Registry /Adobe /Ordering /Japan1 >>");
    assert_eq!(
        request.character_collection,
        Some(CharacterCollection {
            registry: "Adobe".to_string(),
            ordering: "Japan1".to_string(),
            supplement: None,
        })
    );

    // A descendant that names no collection carries none.
    assert_eq!(asked("").character_collection, None);
}

/// A simple font's request carries no character collection.
#[test]
fn a_simple_fonts_request_names_no_character_collection() {
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
    let records = Arc::new(Records(Mutex::new(Vec::new())));
    let doc = Document::open(document())
        .expect("it opens")
        .with_fonts(records.clone());
    let _ = doc
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default());
    let seen = records.0.lock().expect("not poisoned").clone();
    assert!(!seen.is_empty(), "the base-14 font is asked for");
    assert!(seen
        .iter()
        .all(|request| request.character_collection.is_none()));
}

/// A page drawing one code in `/F0`, the font dictionary object 5, whose
/// descriptor is object 6 — or, for a composite font, whose descendant is
/// object 7 and the descendant's descriptor object 6. Written by hand,
/// because the builder embeds every font it describes.
fn described_page(
    font: &str,
    descriptor: &str,
    descendant: Option<&str>,
    content: &str,
) -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
         /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        font.to_string(),
        descriptor.to_string(),
    ];
    if let Some(descendant) = descendant {
        objects.push(descendant.to_string());
    }
    let mut out = String::from("%PDF-1.7\n");
    for (index, object) in objects.iter().enumerate() {
        out.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
        objects.len() + 1
    ));
    out.into_bytes()
}

/// **The provider is told the PANOSE** (FT-12.2): a font's request carries
/// the family class and the ten PANOSE bytes its descriptor's `/Style`
/// states (9.8.3.2) — a simple font's own descriptor, and a composite font's
/// descendant's — and nothing where the descriptor states none or states it
/// malformed. A recording provider is the host's, asked before any bundled
/// face, so this holds in the default and the `bundled-fonts` builds.
#[test]
fn the_provider_is_told_the_panose() {
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

    let asked = |bytes: Vec<u8>| {
        let records = Arc::new(Records(Mutex::new(Vec::new())));
        let doc = Document::open(bytes)
            .expect("it opens")
            .with_fonts(records.clone());
        let _ = doc
            .page(0)
            .expect("a page")
            .render(&RenderOptions::default());
        let seen = records.0.lock().expect("not poisoned").clone();
        assert_eq!(seen.len(), 1, "asked once for the font, got: {seen:?}");
        seen.into_iter().next().expect("one request")
    };

    // A sans face of class 8, subclass 1.
    let simple = asked(described_page(
        "<< /Type /Font /Subtype /TrueType /BaseFont /Sans /FirstChar 65 \
         /LastChar 65 /Widths [600] /FontDescriptor 6 0 R >>",
        "<< /Type /FontDescriptor /FontName /Sans /Flags 32 \
         /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 /Descent -200 \
         /CapHeight 700 /StemV 80 /Style << /Panose <0801020B0604020202020204> >> >>",
        None,
        "BT /F0 24 Tf 20 40 Td (A) Tj ET",
    ));
    assert_eq!(simple.base_font, "Sans");
    assert_eq!(simple.family_class, Some(0x0801));
    assert_eq!(simple.panose, Some([2, 0x0B, 6, 4, 2, 2, 2, 2, 2, 4]));

    // A composite font's classification is its descendant's, 9.8.3.2's own
    // example.
    let composite = asked(described_page(
        "<< /Type /Font /Subtype /Type0 /BaseFont /Mincho /Encoding /Identity-H \
         /DescendantFonts [7 0 R] >>",
        "<< /Type /FontDescriptor /FontName /Mincho /Flags 6 \
         /FontBBox [0 -120 1000 880] /ItalicAngle 0 /Ascent 880 /Descent -120 \
         /CapHeight 740 /StemV 80 /Style << /Panose <01 05 02 02 03 00 00 00 00 00 00 00> >> >>",
        Some(
            "<< /Type /Font /Subtype /CIDFontType0 /BaseFont /Mincho \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 6 >> \
             /FontDescriptor 6 0 R /DW 1000 >>",
        ),
        "BT /F0 24 Tf 20 40 Td <0041> Tj ET",
    ));
    assert_eq!(composite.family_class, Some(0x0105));
    assert_eq!(composite.panose, Some([2, 2, 3, 0, 0, 0, 0, 0, 0, 0]));

    // Eleven bytes state nothing, and nothing is made of them.
    let short = asked(described_page(
        "<< /Type /Font /Subtype /TrueType /BaseFont /Sans /FirstChar 65 \
         /LastChar 65 /Widths [600] /FontDescriptor 6 0 R >>",
        "<< /Type /FontDescriptor /FontName /Sans /Flags 32 \
         /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 /Descent -200 \
         /CapHeight 700 /StemV 80 /Style << /Panose <0801020B06040202020202> >> >>",
        None,
        "BT /F0 24 Tf 20 40 Td (A) Tj ET",
    ));
    assert_eq!((short.family_class, short.panose), (None, None));

    // And a standard-14 font, which has no descriptor, states none either.
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Times-Roman");
    builder.add_page(100.0, 50.0, |page| {
        page.text(b"F0", 12.0, 5.0, 20.0, "x");
    });
    let standard = asked(built("panose, a standard font", builder));
    assert_eq!((standard.family_class, standard.panose), (None, None));
}

// ---- a substituted face read by character (FT-01.3) -------------------------

mod render_support;

use render_support::{
    composite_fonts_page, fallback_face, substituted_cjk_page, to_unicode_body, FaceBox,
    UnembeddedFont, CJK_PENS, CJK_SIZE, JAPAN1, LOWER_LEFT, NOTDEF_BOX, UPPER_RIGHT,
};

/// The centre of a box in the face's 1000-unit em.
fn centre([x0, y0, x1, y1]: FaceBox) -> (f64, f64) {
    (
        f64::from(i32::from(x0) + i32::from(x1)) / 2.0,
        f64::from(i32::from(y0) + i32::from(y1)) / 2.0,
    )
}

/// A point of the em inside no character's box, the `.notdef`'s or the
/// vertical form's: only a decoy, the whole em, inks it.
const DECOY_ONLY: (f64, f64) = (950.0, 500.0);

/// Whether the pixel under the point `at` of a glyph's 1000-unit em is
/// inked, the glyph's origin at `origin` and its size `size` in points, on a
/// page `height` points tall drawn at 1x.
fn inked(
    bitmap: &tinker_pdf::Bitmap,
    height: f64,
    origin: (f64, f64),
    size: f64,
    at: (f64, f64),
) -> bool {
    let x = origin.0 + at.0 * size / 1000.0;
    let y = height - (origin.1 + at.1 * size / 1000.0);
    let (x, y) = (x.floor() as usize, y.floor() as usize);
    let at = y * bitmap.stride + x * bitmap.components();
    bitmap.data.get(at).is_some_and(|value| *value < 128)
}

/// A host provider answering every request with `face`, symbolic ones
/// included, as a CJK font's descriptor says it is.
fn provider_of(face: Vec<u8>) -> Arc<SimpleFontProvider> {
    Arc::new(SimpleFontProvider::new(face).substituting_symbolic())
}

/// `bytes` rendered at 1x with `face` supplied.
fn drawn_with(bytes: Vec<u8>, face: Vec<u8>) -> tinker_pdf::Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .with_fonts(provider_of(face))
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// Whether `bitmap` reports a font it could not draw.
fn reports(bitmap: &tinker_pdf::Bitmap) -> bool {
    bitmap
        .warnings
        .contains(&tinker_pdf::RenderWarning::UnreadableFont)
}

/// The resource name every one-character page's font is given.
const FONT: &str = "F";

/// A one-character page in `font` at [`CJK_SIZE`], its pen at `(20, 20)` on a
/// 120-point-square page.
fn one_glyph_page(font: UnembeddedFont<'_>, code: &str) -> Vec<u8> {
    composite_fonts_page(
        120.0,
        120.0,
        &[font],
        &format!("BT /{FONT} {CJK_SIZE} Tf 20 20 Td <{code}> Tj ET"),
    )
}

/// **A substituted face draws a composite font by character, not by CID**
/// (`docs/design/fallback-faces.md`). An unembedded Adobe-Japan1 font under
/// `UniJIS-UTF16-H` shows `日本語A`, and a provider's face draws it whose
/// `cmap` gives each character its own quarter of the em and whose glyph at
/// each CID's number is a decoy filling the whole em: every character inks
/// its own quarter, and nothing else of its em — not another character's
/// quarter, not the `.notdef`'s centre, and not the corner only the decoy
/// covers. In a vertical font under `UniJIS-UTF16-V`, `、` is CID 7887, the
/// vertical comma, reached through `UniJIS-UTF32-V` as `、` and drawn in the
/// face's `vert` form. Nothing is reported.
#[cfg(feature = "cmap-predefined")]
#[test]
fn a_substituted_face_draws_a_composite_font_by_character_not_by_cid() {
    use render_support::{
        CJK_VERTICAL_PEN, FALLBACK_CHARACTERS, LOWER_RIGHT, UPPER_LEFT, VERTICAL_COMMA,
    };

    let bitmap = drawn_with(substituted_cjk_page(), fallback_face(true, true));
    assert!(!reports(&bitmap), "{:?}", bitmap.warnings);
    let quarters = [LOWER_LEFT, LOWER_RIGHT, UPPER_LEFT, UPPER_RIGHT];
    for (slot, (character, _, own)) in FALLBACK_CHARACTERS.iter().take(4).enumerate() {
        let pen = CJK_PENS[slot];
        assert!(
            inked(&bitmap, 120.0, pen, CJK_SIZE, centre(*own)),
            "{character} inks its own box"
        );
        for other in quarters.iter().filter(|quarter| *quarter != own) {
            assert!(
                !inked(&bitmap, 120.0, pen, CJK_SIZE, centre(*other)),
                "{character} inks no other character's box"
            );
        }
        for (point, what) in [(centre(NOTDEF_BOX), ".notdef"), (DECOY_ONLY, "the decoy")] {
            assert!(
                !inked(&bitmap, 120.0, pen, CJK_SIZE, point),
                "{character} draws not {what}"
            );
        }
    }
    // 9.7.4.3: the vertical origin stands `(w0 / 2, 880)` units above and
    // right of the glyph's own, `/DW 1000` and `/DW2`'s default.
    let (x, y) = CJK_VERTICAL_PEN;
    let origin = (x - 0.5 * CJK_SIZE, y - 0.88 * CJK_SIZE);
    assert!(
        inked(&bitmap, 120.0, origin, CJK_SIZE, centre(VERTICAL_COMMA.1)),
        "the vertical comma is drawn in the face's vertical form"
    );
    assert!(
        !inked(&bitmap, 120.0, origin, CJK_SIZE, centre(LOWER_LEFT)),
        "and not as the horizontal comma"
    );
}

/// **A CID no route reaches draws `.notdef` and reports.** `字`, which the
/// face has no character for, under `UniJIS-UTF16-H`; `<FFFF>`, which that
/// CMap maps to no CID; and an `Adobe-Identity` CIDFontType2 with no
/// `/ToUnicode` showing CID 1, which is no character of any collection —
/// drawn `.notdef` and reported even though glyph 1 of this face is a real
/// glyph, which is the price ruling 23 takes. Each draws the face's
/// `.notdef` and not the decoy at its CID's number. The same Identity font
/// with a `/ToUnicode` naming `日` draws `日`'s box and reports nothing.
#[test]
fn an_unreached_cid_draws_notdef_and_reports() {
    let japan1 = |code: &str| {
        one_glyph_page(
            UnembeddedFont {
                name: FONT,
                encoding: "/UniJIS-UTF16-H",
                subtype: "CIDFontType0",
                info: JAPAN1,
                to_unicode: None,
            },
            code,
        )
    };
    let identity = "/CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>";
    let to_unicode = to_unicode_body(&[(0x0001, '\u{65e5}')]);
    let unreached = [
        ("a character the face does not have", japan1("5B57")),
        ("a code the CMap maps to no CID", japan1("FFFF")),
        (
            "an Identity font with no /ToUnicode",
            one_glyph_page(
                UnembeddedFont {
                    name: FONT,
                    encoding: "/Identity-H",
                    subtype: "CIDFontType2",
                    info: identity,
                    to_unicode: None,
                },
                "0001",
            ),
        ),
    ];
    for (what, page) in unreached {
        let bitmap = drawn_with(page, fallback_face(true, false));
        assert!(reports(&bitmap), "{what}: reported, {:?}", bitmap.warnings);
        assert!(
            inked(&bitmap, 120.0, (20.0, 20.0), CJK_SIZE, centre(NOTDEF_BOX)),
            "{what}: the face's .notdef"
        );
        for (point, never) in [(DECOY_ONLY, "a decoy"), (centre(LOWER_LEFT), "glyph 1")] {
            assert!(
                !inked(&bitmap, 120.0, (20.0, 20.0), CJK_SIZE, point),
                "{what}: never {never}"
            );
        }
    }

    let mapped = one_glyph_page(
        UnembeddedFont {
            name: FONT,
            encoding: "/Identity-H",
            subtype: "CIDFontType2",
            info: identity,
            to_unicode: Some(&to_unicode),
        },
        "0001",
    );
    let bitmap = drawn_with(mapped, fallback_face(true, false));
    assert!(!reports(&bitmap), "{:?}", bitmap.warnings);
    assert!(inked(
        &bitmap,
        120.0,
        (20.0, 20.0),
        CJK_SIZE,
        centre(LOWER_LEFT)
    ));
    assert!(!inked(
        &bitmap,
        120.0,
        (20.0, 20.0),
        CJK_SIZE,
        centre(NOTDEF_BOX)
    ));
}

/// **A vertical font draws the face's vertical form**, and a face with no
/// `vert` draws the character upright: `、` under `UniJIS-UTF16-V` is the
/// vertical comma's CID, reached through `UniJIS-UTF32-V` as `、`.
#[cfg(feature = "cmap-predefined")]
#[test]
fn a_vertical_font_draws_the_faces_vertical_form() {
    use render_support::VERTICAL_COMMA;

    let page = || {
        composite_fonts_page(
            120.0,
            120.0,
            &[UnembeddedFont {
                name: FONT,
                encoding: "/UniJIS-UTF16-V",
                subtype: "CIDFontType0",
                info: JAPAN1,
                to_unicode: None,
            }],
            &format!("BT /{FONT} {CJK_SIZE} Tf 60 100 Td <3001> Tj ET"),
        )
    };
    let origin = (60.0 - 0.5 * CJK_SIZE, 100.0 - 0.88 * CJK_SIZE);
    for (with_vert, drawn, other) in [
        (true, VERTICAL_COMMA.1, LOWER_LEFT),
        (false, LOWER_LEFT, VERTICAL_COMMA.1),
    ] {
        let bitmap = drawn_with(page(), fallback_face(true, with_vert));
        assert!(!reports(&bitmap), "{:?}", bitmap.warnings);
        assert!(
            inked(&bitmap, 120.0, origin, CJK_SIZE, centre(drawn)),
            "with vert {with_vert}"
        );
        assert!(
            !inked(&bitmap, 120.0, origin, CJK_SIZE, centre(other)),
            "with vert {with_vert}"
        );
    }
}

/// **A face with no `cmap` keeps the index reading**: with no characters to
/// be read by — the corpus's synthetic face, the test faces — an Adobe-Japan1
/// CID is the face's glyph number, here the decoy at 3284, and nothing is
/// reported.
#[cfg(feature = "cmap-predefined")]
#[test]
fn a_face_with_no_cmap_keeps_the_index_reading() {
    let page = one_glyph_page(
        UnembeddedFont {
            name: FONT,
            encoding: "/UniJIS-UTF16-H",
            subtype: "CIDFontType0",
            info: JAPAN1,
            to_unicode: None,
        },
        "65E5",
    );
    let bitmap = drawn_with(page, fallback_face(false, false));
    assert!(!reports(&bitmap), "{:?}", bitmap.warnings);
    assert!(inked(&bitmap, 120.0, (20.0, 20.0), CJK_SIZE, DECOY_ONLY));
}

/// **A simple font's code takes only the readings a `cmap` admits.** An
/// unembedded WinAnsi TrueType font shows `A` and `B`: `A` reaches its box
/// through the face's `cmap`; `B`, which the face has no character for, draws
/// `.notdef` and reports, not glyph 66, the decoy its code would number. The
/// same face with no `cmap` keeps the code as the glyph number, and draws the
/// decoy for `B`.
#[test]
fn a_simple_font_takes_only_the_readings_a_cmap_admits() {
    let content =
        format!("BT /F {CJK_SIZE} Tf 20 20 Td (A) Tj ET BT /F {CJK_SIZE} Tf 120 20 Td (B) Tj ET");
    let page = simple_font_page(&content);
    let bitmap = drawn_with(page.clone(), fallback_face(true, false));
    assert!(reports(&bitmap), "B is reported: {:?}", bitmap.warnings);
    assert!(
        inked(&bitmap, 120.0, (20.0, 20.0), CJK_SIZE, centre(UPPER_RIGHT)),
        "A"
    );
    assert!(
        !inked(&bitmap, 120.0, (20.0, 20.0), CJK_SIZE, DECOY_ONLY),
        "A, not a decoy"
    );
    assert!(
        inked(&bitmap, 120.0, (120.0, 20.0), CJK_SIZE, centre(NOTDEF_BOX)),
        "B, .notdef"
    );
    assert!(
        !inked(&bitmap, 120.0, (120.0, 20.0), CJK_SIZE, DECOY_ONLY),
        "B, not glyph 66"
    );

    let bitmap = drawn_with(page, fallback_face(false, false));
    assert!(
        inked(&bitmap, 120.0, (120.0, 20.0), CJK_SIZE, DECOY_ONLY),
        "B as glyph 66"
    );
}

/// A 240 by 120 page showing `content` in `/F`, an unembedded, non-symbolic
/// WinAnsi TrueType font whose two codes `A` and `B` are each a full em wide.
fn simple_font_page(content: &str) -> Vec<u8> {
    render_support::raw_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 240 120] \
          /Resources << /Font << /F 5 0 R >> >> /Contents 4 0 R >>"
            .to_vec(),
        render_support::raw_stream("", content.as_bytes()),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /Fallback-Simple \
          /Encoding /WinAnsiEncoding /FirstChar 65 /LastChar 66 /Widths [1000 1000] \
          /FontDescriptor 6 0 R >>"
            .to_vec(),
        b"<< /Type /FontDescriptor /FontName /Fallback-Simple /Flags 32 \
          /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 1000 /Descent 0 \
          /CapHeight 700 /StemV 80 >>"
            .to_vec(),
    ])
}

/// **Without the tables, unembedded CJK text is reported**, never guessed: a
/// build without `cmap-predefined` maps no `UniJIS-UTF16-H` code to a CID and
/// inverts no collection, so each character of `日本語A` draws `.notdef` and
/// the page reports, the decoys at the codes' numbers undrawn.
#[cfg(not(feature = "cmap-predefined"))]
#[test]
fn without_the_tables_unembedded_cjk_text_is_reported() {
    let bitmap = drawn_with(substituted_cjk_page(), fallback_face(true, true));
    assert!(reports(&bitmap), "{:?}", bitmap.warnings);
    for pen in CJK_PENS {
        assert!(inked(&bitmap, 120.0, pen, CJK_SIZE, centre(NOTDEF_BOX)));
        assert!(!inked(&bitmap, 120.0, pen, CJK_SIZE, DECOY_ONLY));
    }
}

// ---- Symbol and ZapfDingbats drawn by glyph name (FT-03.4) -------------------

use render_support::{
    symbol_box, symbol_draws, symbol_face, symbol_glyph, symbol_pen, symbols_page,
    symbols_page_size, SYMBOL_CELL, SYMBOL_COLUMNS, SYMBOL_SIZE,
};

/// The device-pixel box `[x0, y0, x1, y1)` the dark pixels of cell `at` of a
/// [`symbols_page`]-sized page cover, `y` down; `None` for a blank cell.
fn symbol_ink(bitmap: &tinker_pdf::Bitmap, at: usize) -> Option<[usize; 4]> {
    let (width, height) = (SYMBOL_CELL.0 as usize, SYMBOL_CELL.1 as usize);
    let (left, top) = (
        (at % SYMBOL_COLUMNS) * width,
        (at / SYMBOL_COLUMNS) * height,
    );
    let mut found: Option<[usize; 4]> = None;
    for y in top..top + height {
        for x in left..left + width {
            let pixel = y * bitmap.stride + x * bitmap.components();
            if bitmap.data.get(pixel).is_some_and(|value| *value < 128) {
                found = Some(match found {
                    None => [x, y, x + 1, y + 1],
                    Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)],
                });
            }
        }
    }
    found
}

/// The device-pixel box [`symbol_face`]'s glyph `glyph` covers drawn at the
/// pen of cell `at`, `y` down: every edge on a pixel's edge at
/// [`SYMBOL_SIZE`].
fn symbol_expected(at: usize, glyph: u16) -> [usize; 4] {
    let (x, y) = symbol_pen(at);
    let (_, height) = symbols_page_size();
    let [x0, y0, x1, y1] = symbol_box(glyph);
    let unit = SYMBOL_SIZE / 1000.0;
    let pixel = |value: f64| value.round() as usize;
    [
        pixel(x + f64::from(x0) * unit),
        pixel(height - (y + f64::from(y1) * unit)),
        pixel(x + f64::from(x1) * unit),
        pixel(height - (y + f64::from(y0) * unit)),
    ]
}

/// A page of [`symbols_page`]'s size showing each of `draws`, a font resource
/// and a code, at the pen of its place among them: the fonts, each a resource
/// name and its dictionary written verbatim, are objects 5 on, and `more`
/// follows them.
fn symbolic_page(fonts: &[(&str, &str)], draws: &[(&str, u8)], more: &[Vec<u8>]) -> Vec<u8> {
    let (width, height) = symbols_page_size();
    let mut content = String::new();
    for (at, (font, code)) in draws.iter().enumerate() {
        let (x, y) = symbol_pen(at);
        content.push_str(&format!(
            "BT /{font} {SYMBOL_SIZE} Tf {x} {y} Td <{code:02X}> Tj ET\n"
        ));
    }
    let resources: String = fonts
        .iter()
        .enumerate()
        .map(|(index, (name, _))| format!("/{name} {} 0 R ", index + 5))
        .collect();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] \
             /Resources << /Font << {resources}>> >> /Contents 4 0 R >>"
        )
        .into_bytes(),
        render_support::raw_stream("", content.as_bytes()),
    ];
    objects.extend(fonts.iter().map(|(_, dict)| dict.as_bytes().to_vec()));
    objects.extend(more.iter().cloned());
    render_support::raw_pdf(&objects)
}

/// The unembedded Symbol font of the pages below, as the standard 14 are
/// written: no `/Widths`, no `/Encoding`, no descriptor.
const SYMBOL: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Symbol >>";

/// **Every code of both built-in encodings draws its named glyph**
/// (`docs/design/symbol-faces.md`, FT-03.4). A host face naming all 377 of
/// its glyphs in its `post` table, each a box of its own size, draws the 189
/// codes of Symbol's built-in encoding and the 188 of ZapfDingbats', each
/// code alone at its own pen in an unembedded font: every code inks exactly
/// the box of the glyph its built-in name names, `space` the one glyph both
/// encodings name, and nothing is reported.
#[test]
fn every_code_of_both_built_in_encodings_draws_its_named_glyph() {
    let bitmap = drawn_with(symbols_page(), symbol_face(true, &[]));
    assert!(
        !reports(&bitmap),
        "every code reached its glyph: {:?}",
        bitmap.warnings
    );
    let draws = symbol_draws();
    assert_eq!(draws.len(), 189 + 188, "both encodings' codes");
    for (at, (font, code, name)) in draws.iter().enumerate() {
        assert_eq!(
            symbol_ink(&bitmap, at),
            Some(symbol_expected(at, symbol_glyph(name))),
            "/{font} {code:#04x} draws {name}'s box"
        );
    }
}

/// **A `/Differences` name wins over the built-in encoding**, and so does a
/// base encoding the dictionary names. Symbol's `0x41` is `Alpha` and its
/// `0x42` `Beta`; renamed `heart` and `a1` — a ZapfDingbats name, which the
/// face holds too — they draw those, while `0x43`, renamed nothing, keeps
/// `Chi`. ZapfDingbats encodes nothing at `0x95`, and under a named
/// `/WinAnsiEncoding` base it is `bullet`, while `0x61` there is `a`, which
/// the face neither names nor maps: `.notdef`, reported.
#[test]
fn a_differences_name_wins_over_the_built_in_encoding() {
    let page = symbolic_page(
        &[
            (
                "R",
                "<< /Type /Font /Subtype /Type1 /BaseFont /Symbol \
                 /Encoding << /Differences [65 /heart /a1] >> >>",
            ),
            (
                "B",
                "<< /Type /Font /Subtype /Type1 /BaseFont /ZapfDingbats \
                 /Encoding << /BaseEncoding /WinAnsiEncoding >> >>",
            ),
        ],
        &[
            ("R", 0x41),
            ("R", 0x42),
            ("R", 0x43),
            ("B", 0x95),
            ("B", 0x61),
        ],
        &[],
    );
    let bitmap = drawn_with(page, symbol_face(true, &[]));
    for (at, name) in [(0, "heart"), (1, "a1"), (2, "Chi"), (3, "bullet")] {
        assert_eq!(
            symbol_ink(&bitmap, at),
            Some(symbol_expected(at, symbol_glyph(name))),
            "draw {at} is {name}"
        );
    }
    assert_eq!(
        symbol_ink(&bitmap, 4),
        Some(symbol_expected(4, 0)),
        "a WinAnsi `a` reaches nothing and draws .notdef"
    );
    assert!(reports(&bitmap), "and reports it: {:?}", bitmap.warnings);
}

/// **An embedded Symbol program keeps its own encoding.** A TrueType font
/// named Symbol embedding a face whose `(3,0)` `cmap` maps `0x41` to the
/// glyph named `heart` draws the heart, by its own encoding, though a host
/// face is on offer; the same face substituted for the same font with
/// nothing embedded draws `Alpha`, the built-in encoding's name, which the
/// route asks the face's names for first.
#[test]
fn an_embedded_symbol_program_keeps_its_own_encoding() {
    let heart = symbol_glyph("heart");
    let program = symbol_face(true, &[(0x41, heart)]);
    let descriptor = |more: &str| {
        format!(
            "<< /Type /FontDescriptor /FontName /Symbol /Flags 4 \
             /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 1000 /Descent 0 \
             /CapHeight 700 /StemV 80{more} >>"
        )
        .into_bytes()
    };
    let font = "<< /Type /Font /Subtype /TrueType /BaseFont /Symbol /FontDescriptor 6 0 R >>";
    let embedded = symbolic_page(
        &[("E", font)],
        &[("E", 0x41)],
        &[
            descriptor(" /FontFile2 7 0 R"),
            render_support::raw_stream("", &program),
        ],
    );
    let bitmap = drawn_with(embedded, symbol_face(true, &[]));
    assert!(!reports(&bitmap), "{:?}", bitmap.warnings);
    assert_eq!(
        symbol_ink(&bitmap, 0),
        Some(symbol_expected(0, heart)),
        "the program's own (3,0) reading"
    );

    let substituted = symbolic_page(&[("E", font)], &[("E", 0x41)], &[descriptor("")]);
    let bitmap = drawn_with(substituted, program);
    assert!(!reports(&bitmap), "{:?}", bitmap.warnings);
    assert_eq!(
        symbol_ink(&bitmap, 0),
        Some(symbol_expected(0, symbol_glyph("Alpha"))),
        "the face substituted is read by the code's name first"
    );
}

/// **A face with neither names nor a `cmap` keeps the index reading**, as it
/// does for a composite font: it has nothing to be read by name through, so
/// Symbol's `0x41` draws glyph 65, the code as a glyph number, and nothing is
/// reported.
#[test]
fn a_face_with_neither_names_nor_a_cmap_keeps_the_index_reading() {
    let page = symbolic_page(&[("S", SYMBOL)], &[("S", 0x41)], &[]);
    let bitmap = drawn_with(page, symbol_face(false, &[]));
    assert!(!reports(&bitmap), "{:?}", bitmap.warnings);
    assert_eq!(symbol_ink(&bitmap, 0), Some(symbol_expected(0, 65)));
}

/// **The `(3,0)` reading stays, last**: a face that names nothing and maps
/// Symbol's codes through a `(3,0)` subtable alone, as a face that draws a
/// Symbol page today may, still draws each code its subtable maps, and a code
/// it does not map draws `.notdef` and reports.
#[test]
fn a_face_read_by_its_symbol_subtable_alone_still_draws() {
    let beta = symbol_glyph("Beta");
    let page = symbolic_page(&[("S", SYMBOL)], &[("S", 0x41), ("S", 0x43)], &[]);
    let bitmap = drawn_with(page, symbol_face(false, &[(0x41, beta)]));
    assert_eq!(
        symbol_ink(&bitmap, 0),
        Some(symbol_expected(0, beta)),
        "what the subtable maps"
    );
    assert_eq!(symbol_ink(&bitmap, 1), Some(symbol_expected(1, 0)));
    assert!(reports(&bitmap), "0x43 is reported: {:?}", bitmap.warnings);
}

/// **Zapf Chancery is no ZapfDingbats.** A text face of that family is read
/// as a text face is: its `0x41` through the face's `cmap` by its character,
/// here the glyph the `(3,0)` subtable maps at `0xF041`, and not the glyph
/// ZapfDingbats' built-in encoding names at `0x41`.
#[test]
fn zapf_chancery_keeps_the_readings_a_text_face_gets() {
    let heart = symbol_glyph("heart");
    let page = symbolic_page(
        &[(
            "Z",
            "<< /Type /Font /Subtype /Type1 /BaseFont /ZapfChancery-MediumItalic >>",
        )],
        &[("Z", 0x41)],
        &[],
    );
    let bitmap = drawn_with(page, symbol_face(true, &[(0x41, heart)]));
    assert!(!reports(&bitmap), "{:?}", bitmap.warnings);
    assert_eq!(symbol_ink(&bitmap, 0), Some(symbol_expected(0, heart)));
}

/// **A code with no `/Widths` takes its name's advance**: each character's
/// box, as the page's text reads it, is the advance Adobe's Core 14 metrics
/// give the code's glyph name — Symbol's `Alpha` 722 and `Beta` 667,
/// ZapfDingbats' `a1` 974 and `a20` 846 — where it was a flat 600; a name
/// the metrics do not list, here `A` by `/Differences`, keeps 600; and a font
/// with `/Widths` takes its own.
#[test]
fn a_code_with_no_widths_takes_its_names_advance() {
    let page = render_support::raw_pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 400] \
          /Resources << /Font << /S 5 0 R /D 6 0 R /X 7 0 R /W 8 0 R >> >> \
          /Contents 4 0 R >>"
            .to_vec(),
        render_support::raw_stream(
            "",
            b"BT /S 10 Tf 20 300 Td <4142> Tj ET\n\
              BT /D 10 Tf 20 250 Td <2134> Tj ET\n\
              BT /X 10 Tf 20 200 Td <4142> Tj ET\n\
              BT /W 10 Tf 20 150 Td <4142> Tj ET\n",
        ),
        SYMBOL.as_bytes().to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /ZapfDingbats >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Symbol \
          /Encoding << /Differences [65 /A] >> >>"
            .to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Symbol \
          /FirstChar 65 /LastChar 66 /Widths [400 500] >>"
            .to_vec(),
    ]);
    let doc = Document::open(page).expect("it opens");
    let text = doc
        .page(0)
        .expect("a page")
        .text_with(&tinker_pdf::TextOptions {
            content_order: true,
            ..Default::default()
        });
    let lines: Vec<(String, Vec<f64>)> = text
        .lines()
        .iter()
        .map(|line| {
            let read: String = line.chars.iter().map(|c| c.text.as_str()).collect();
            let widths = line
                .chars
                .iter()
                .map(|c| {
                    let (x0, _, x1, _) = c.quad.bounds();
                    x1 - x0
                })
                .collect();
            (read, widths)
        })
        .collect();
    for (read, want, why) in [
        ("AB", [7.22, 6.67], "Symbol's Alpha and Beta"),
        ("!4", [9.74, 8.46], "ZapfDingbats' a1 and a20"),
        (
            "AB",
            [6.0, 6.67],
            "a name the metrics do not list, then Beta",
        ),
        ("AB", [4.0, 5.0], "the font's own /Widths"),
    ] {
        assert!(
            lines.iter().any(|(text, widths)| text == read
                && widths.len() == want.len()
                && widths
                    .iter()
                    .zip(want)
                    .all(|(width, want)| (width - want).abs() < 1e-6)),
            "{why}: {lines:?}"
        );
    }
}
