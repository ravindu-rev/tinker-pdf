//! Reading the fixture's fonts.
//!
//! `simple-text.pdf` names Helvetica with WinAnsiEncoding and supplies no
//! `/Widths`, no `/ToUnicode` and no embedded font program — the case that
//! forces built-in metrics and encoding tables to be right, since there is
//! nothing else to fall back on.

use std::path::PathBuf;

use tinker_pdf_cos::{font, pages, CosDocument, FontKind, Object};

fn open(name: &str) -> CosDocument {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    CosDocument::open(bytes).expect("the fixture opens")
}

/// The fonts of the first page, by resource name.
fn first_page_fonts(doc: &CosDocument) -> Vec<std::sync::Arc<font::Font>> {
    let pages = pages::collect(doc);
    let page = pages.first().expect("a first page");
    let resources = page.resources.as_ref().expect("the page has resources");
    font::from_resources(doc, resources).into_values().collect()
}

#[test]
fn the_fixture_font_is_a_standard_type1() {
    let doc = open("simple-text.pdf");
    let fonts = first_page_fonts(&doc);

    assert_eq!(fonts.len(), 1, "the fixture uses one font");
    let font = fonts.first().expect("a font");
    assert_eq!(font.kind(), FontKind::Type1);
    assert_eq!(font.base_font(), "Helvetica");
    assert!(!font.is_vertical());
}

/// With no `/Widths`, the built-in Helvetica metrics are the only source of
/// advances, and a wrong one puts every glyph in the wrong place.
#[test]
fn widths_come_from_the_built_in_metrics() {
    let doc = open("simple-text.pdf");
    let fonts = first_page_fonts(&doc);
    let font = fonts.first().expect("a font");

    // The published Helvetica AFM advances.
    for (c, want) in [
        (b'T', 611.0),
        (b'i', 222.0),
        (b'n', 556.0),
        (b'k', 500.0),
        (b'e', 556.0),
        (b'r', 333.0),
        (b' ', 278.0),
    ] {
        let (width, exact) = font.width_of(u32::from(c));
        assert_eq!(width, want, "advance of {:?}", char::from(c));
        assert!(exact, "a published metric is exact");
    }
}

#[test]
fn codes_decode_to_the_text_they_stand_for() {
    let doc = open("simple-text.pdf");
    let fonts = first_page_fonts(&doc);
    let font = fonts.first().expect("a font");

    let decoded = font.decode(b"Tinker");
    let text: String = decoded.iter().map(|d| d.text.as_str()).collect();
    assert_eq!(text, "Tinker");

    // A simple font reads one byte at a time.
    assert!(decoded.iter().all(|d| d.bytes == 1));
    assert_eq!(decoded.len(), 6);
}

#[test]
fn win_ansi_high_codes_survive_the_round_trip() {
    let doc = open("simple-text.pdf");
    let fonts = first_page_fonts(&doc);
    let font = fonts.first().expect("a font");

    // 0x92 is a right single quote in WinAnsi, not a control character.
    assert_eq!(font.text_of(0x92), "’");
    assert_eq!(font.text_of(0xE9), "é");
    // An accented glyph borrows its base letter's advance, and says so.
    let (w, exact) = font.width_of(0xE9);
    assert_eq!(w, font.width_of(u32::from(b'e')).0);
    assert!(!exact, "an approximated width reports itself");
}

#[test]
fn the_text_of_a_page_can_be_assembled_from_its_font() {
    let doc = open("simple-text.pdf");
    let fonts = first_page_fonts(&doc);
    let font = fonts.first().expect("a font");

    // The content stream holds `(Tinker fixture, page 1 of 3) Tj`.
    let bytes = pages::content_bytes(&doc, pages::collect(&doc).first().expect("a page"));
    let content = String::from_utf8_lossy(&bytes);
    let start = content.find('(').expect("a string operand");
    let end = content.find(')').expect("its close");
    let literal = content
        .get(start + 1..end)
        .expect("the literal between them");

    let decoded: String = font
        .decode(literal.as_bytes())
        .iter()
        .map(|d| d.text.as_str())
        .collect();
    assert_eq!(decoded, "Tinker fixture, page 1 of 3");

    // And the advances sum to something sane for 18pt text on A4.
    let total: f64 = font
        .decode(literal.as_bytes())
        .iter()
        .map(|d| d.width)
        .sum::<f64>()
        / 1000.0
        * 18.0;
    assert!(
        (100.0..400.0).contains(&total),
        "27 characters of 18pt Helvetica should span 100..400 points, got {total}"
    );
}

#[test]
fn a_font_dictionary_of_nonsense_does_not_panic() {
    let doc = open("simple-text.pdf");
    let mut dict = tinker_pdf_cos::Dict::new();
    dict.insert(tinker_pdf_cos::Name::TYPE, Object::Int(7));
    let font = font::read(&doc, &dict);

    // No subtype means Type 1 by default, and everything else is empty.
    assert_eq!(font.kind(), FontKind::Type1);
    assert_eq!(font.base_font(), "");
    let _ = font.decode(&[0, 1, 255]);
    let _ = font.width_of(0);
    let _ = font.text_of(u32::MAX);
}
