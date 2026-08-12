//! Reading the fixture's fonts.
//!
//! `simple-text.pdf` names Helvetica with WinAnsiEncoding and supplies no
//! `/Widths`, no `/ToUnicode` and no embedded font program — the case that
//! forces built-in metrics and encoding tables to be right, since there is
//! nothing else to fall back on.
//!
//! The composite fonts at the end are hand-built instead, because there is no
//! CJK fixture anywhere in this repository and `testdata/` is four
//! self-authored PDFs that are not to be modified. Every byte of them is in
//! this file, which is what makes "the right CID" and "the right glyph"
//! checkable by arithmetic rather than by trust.

use std::path::PathBuf;

use tinker_pdf_cos::{font, pages, CosDocument, FontKind, ObjRef, Object};

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

// ---------------------------------------------------------------------------
// Composite fonts: code to CID to GID (gap 02, 9.7.4).
// ---------------------------------------------------------------------------

/// A document holding nothing but a catalog, an empty page tree, and the
/// numbered objects given — object 3 upward, so the font dictionary is always
/// `3 0 R`.
fn document(objects: &[Vec<u8>]) -> CosDocument {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n");
    for (i, object) in objects.iter().enumerate() {
        out.extend_from_slice(format!("{} 0 obj\n", i + 3).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
            objects.len() + 3
        )
        .as_bytes(),
    );
    CosDocument::open(out).expect("the hand-built document opens")
}

/// An uncompressed stream object with the bytes given.
fn stream(data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A `/CIDToGIDMap` stream: two big-endian bytes per CID from zero up
/// (9.7.4.2).
fn cid_to_gid_stream(map: &[u16]) -> Vec<u8> {
    let bytes: Vec<u8> = map.iter().flat_map(|g| g.to_be_bytes()).collect();
    stream(&bytes)
}

/// A Type 0 font over a CIDFontType2 descendant, as `3 0 R`.
///
/// `encoding` is written after `/Encoding` verbatim, so it may be a predefined
/// name or a reference to a CMap stream; `cid_to_gid` likewise, so it may be
/// `/Identity` or a reference to a stream.
fn composite_font(encoding: &str, widths: &str, cid_to_gid: &str, rest: &[Vec<u8>]) -> CosDocument {
    let mut objects = vec![
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /Fixture /Encoding {encoding}\n\
             /DescendantFonts [4 0 R] >>"
        )
        .into_bytes(),
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Fixture /DW 250 {widths}\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>\n\
             /CIDToGIDMap {cid_to_gid} >>"
        )
        .into_bytes(),
    ];
    objects.extend_from_slice(rest);
    document(&objects)
}

fn font_at(doc: &CosDocument, num: u32) -> font::Font {
    font::at(doc, ObjRef::new(num, 0)).expect("the font dictionary reads")
}

/// Milestone 1's exit criterion. `Identity-H` says the CID *is* the code, and
/// `/Identity` says the glyph is the CID, so a file using both — which is the
/// overwhelmingly common composite font — cannot move when the CID starts
/// deciding the glyph. This is what makes the fix safe to ship.
#[test]
fn an_identity_h_code_is_its_own_cid_and_its_own_glyph() {
    let doc = composite_font("/Identity-H", "", "/Identity", &[]);
    let font = font_at(&doc, 3);
    assert_eq!(font.kind(), FontKind::Type0);
    assert!(
        !font.has_cid_to_gid_map(),
        "`/Identity` as a name is not a table"
    );

    for code in [0x0000u16, 0x0001, 0x0041, 0x1234, 0x7FFF, 0xFFFF] {
        let decoded = font.decode(&code.to_be_bytes());
        assert_eq!(decoded.len(), 1, "two bytes are one code");
        let one = &decoded[0];
        assert_eq!(one.code, u32::from(code));
        assert_eq!(one.bytes, 2);
        assert_eq!(one.cid, one.code, "Identity-H makes the CID the code");
        assert_eq!(
            font.gid_for_cid(one.cid),
            code,
            "and /Identity makes the glyph the CID"
        );
    }
}

/// Milestone 2. The stream form is what a subsetter writes, because
/// subsetting renumbers glyphs and the CIDs must not move with them.
#[test]
fn a_cid_to_gid_stream_names_the_glyph_and_runs_out_at_notdef() {
    // CID 7 is glyph 3 and CID 8 is glyph 1; every other CID in range names
    // no glyph, which is how a subset says it did not keep one.
    let doc = composite_font(
        "/Identity-H",
        "",
        "5 0 R",
        &[cid_to_gid_stream(&[0, 0, 0, 0, 0, 0, 0, 3, 1])],
    );
    let font = font_at(&doc, 3);
    assert!(font.has_cid_to_gid_map(), "the stream form was read");

    assert_eq!(font.gid_for_cid(7), 3);
    assert_eq!(font.gid_for_cid(8), 1);
    assert_eq!(font.gid_for_cid(2), 0, "a CID the map does not keep");
    assert_eq!(
        font.gid_for_cid(9),
        0,
        "one past the end of the table is `.notdef`, not a wrap onto entry 0"
    );
    assert_eq!(
        font.gid_for_cid(0x1_0000),
        0,
        "a CID beyond what the table can index is `.notdef`"
    );
    assert_eq!(
        font.gid_for_cid(u32::MAX),
        0,
        "and so is one whose byte offset would overflow"
    );
}

/// A table with a trailing half-entry is truncated data, and half a glyph
/// index is not a glyph index. Reading the high byte alone would name a glyph
/// 256 times too far along (ruling 1: never a panic either).
#[test]
fn a_half_entry_at_the_end_of_the_map_is_notdef() {
    let doc = composite_font("/Identity-H", "", "5 0 R", &[stream(&[0, 0, 0, 5, 0])]);
    let font = font_at(&doc, 3);
    assert_eq!(font.gid_for_cid(1), 5);
    assert_eq!(font.gid_for_cid(2), 0, "the last entry is one byte short");
}

/// The mitigation this gap exists for: the advance and the glyph are read
/// from the same CID, asserted from one fixture, so a later change that reads
/// one and not the other fails here rather than drifting.
///
/// Three distinct small numbers, which is what makes the fixture mean
/// anything: the code is 0x41, the CID is 7, and the glyph is 3. A fixture
/// where any two of those coincide would pass with the CID discarded.
#[test]
fn the_width_and_the_glyph_come_from_the_same_cid() {
    let cmap = b"/CIDInit /ProcSet findresource begin\n\
                 1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
                 1 begincidchar <0041> 7 endcidchar\n\
                 end";
    let doc = composite_font(
        "5 0 R",
        "/W [7 [800]]",
        "6 0 R",
        &[stream(cmap), cid_to_gid_stream(&[0, 0, 0, 0, 0, 0, 0, 3])],
    );
    let font = font_at(&doc, 3);

    let decoded = font.decode(&[0x00, 0x41]);
    assert_eq!(decoded.len(), 1);
    let one = &decoded[0];
    assert_eq!(one.code, 0x41, "the code the string held");
    assert_eq!(one.cid, 7, "the CID the encoding CMap names for it");
    assert_eq!(one.width, 800.0, "the advance /W gives that CID");
    assert!(one.width_is_exact);
    assert_eq!(
        font.gid_for_cid(one.cid),
        3,
        "and the glyph /CIDToGIDMap gives the same CID"
    );

    // The three numbers really are three numbers.
    assert_ne!(one.code, one.cid);
    assert_ne!(one.cid, u32::from(font.gid_for_cid(one.cid)));
    assert_ne!(one.code, u32::from(font.gid_for_cid(one.cid)));

    // A code the CMap does not map is its own CID (9.7.4), and that CID is
    // not in the table, so it draws `.notdef` rather than borrowing glyph 3.
    let other = font.decode(&[0x00, 0x42]);
    assert_eq!(other[0].cid, 0x42);
    assert_eq!(other[0].width, 250.0, "/DW, because /W does not name it");
    assert_eq!(font.gid_for_cid(other[0].cid), 0);
}

/// A simple font has no CID at all, so the field is the code and nothing
/// about the one-byte path changes. The code *is* what the encoding maps.
#[test]
fn a_simple_font_carries_its_code_as_its_cid() {
    let doc = open("simple-text.pdf");
    let fonts = first_page_fonts(&doc);
    let font = fonts.first().expect("a font");
    assert_eq!(font.kind(), FontKind::Type1);

    for one in font.decode(b"Tinker") {
        assert_eq!(one.cid, one.code);
        assert_eq!(one.bytes, 1);
    }
}

/// A `/CIDToGIDMap` that is neither a name nor a readable stream degrades to
/// the identity rather than to nothing: a font drawing its glyphs in order is
/// recoverable, and one drawing them by an unknown permutation is not.
#[test]
fn an_unreadable_cid_to_gid_map_falls_back_to_the_identity() {
    for entry in ["/Nonsense", "42", "99 0 R", "[1 2 3]"] {
        let doc = composite_font("/Identity-H", "", entry, &[]);
        let font = font_at(&doc, 3);
        assert!(!font.has_cid_to_gid_map(), "{entry} is not a table");
        assert_eq!(font.gid_for_cid(7), 7, "{entry} degrades to the identity");
    }
}
