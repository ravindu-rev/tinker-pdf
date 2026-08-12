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

use tinker_pdf_cos::warn::WarningKind;
use tinker_pdf_cos::{font, pages, CosDocument, FontKind, ObjRef, Object};
use tinker_pdf_font::cmap::{Section as CMapSection, Warning as CMapWarning};

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
    stream_with("", data)
}

/// The same, with `prefix` written into the stream dictionary verbatim — a
/// `/UseCMap` entry, in the tests that need one (Table 120).
fn stream_with(prefix: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {prefix} /Length {} >>\nstream\n", data.len()).into_bytes();
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

// ---------------------------------------------------------------------------
// Vertical metrics: `/W2` and `/DW2` (gap 05, 9.7.4.3).
// ---------------------------------------------------------------------------

/// The vertical fixture's metrics, and its decoy.
///
/// Three things are deliberate here.
///
/// **The CIDs are non-adjacent** — 5 and 6, then 9, then 14. `/W2` carries
/// *three* numbers per CID where `/W` carries one, and reading it with `/W`'s
/// stride does not fail: it puts each CID's metrics on some other CID and
/// keeps going. Entries for CIDs that touch could not show that, because a
/// shift of one lands on a CID the fixture also names. The gaps between them
/// are asserted to hold the default, which is the shift showing up directly.
///
/// **The two forms alternate**, array then range then array. A wrong stride in
/// the range form desynchronises everything after it, so CID 14 reading back
/// correctly is evidence about how many elements the range form consumed.
///
/// **`/W` is the decoy.** Every CID `/W2` names is also given a horizontal
/// width, and none of them resembles its vertical displacement: CID 5 is 1000
/// wide and advances -400 downward. A reader that reached for the horizontal
/// number — which is what this engine did until August 2026 — cannot produce
/// any of the numbers below.
const VERTICAL_METRICS: &str = "/W [5 [1000 1000] 9 [1000] 14 [1000] 20 [500]]\n\
     /W2 [5 [-400 250 700 -410 255 705] 9 9 -600 300 800 14 [-900 350 900]]";

/// Milestone 1's exit criterion, first half: a fixture's per-CID vertical
/// displacements read back exactly, in both of Table 117's forms.
#[test]
fn per_cid_vertical_metrics_read_back_in_both_forms() {
    let doc = composite_font("/Identity-H", VERTICAL_METRICS, "/Identity", &[]);
    let font = font_at(&doc, 3);

    // (v_x, v_y, w1_y) — the position vector, then the displacement.
    assert_eq!(
        font.vertical_metrics(5),
        (250.0, 700.0, -400.0),
        "array form"
    );
    assert_eq!(
        font.vertical_metrics(6),
        (255.0, 705.0, -410.0),
        "the second triple of the same array, one CID along"
    );
    assert_eq!(
        font.vertical_metrics(9),
        (300.0, 800.0, -600.0),
        "range form"
    );
    assert_eq!(
        font.vertical_metrics(14),
        (350.0, 900.0, -900.0),
        "an array entry after a range one, so the range consumed five \
         elements and not three"
    );

    for cid in [5, 6, 9, 14] {
        assert!(font.has_vertical_metrics(cid), "/W2 names CID {cid}");
    }
}

/// The stride risk, stated as an assertion. `/W2`'s three-number entries read
/// with `/W`'s one-number stride would leave metrics on CIDs the file never
/// named — so the CIDs *between* the fixture's entries are checked to be
/// untouched, which is where a shift lands.
#[test]
fn a_cid_between_two_w2_entries_is_not_given_its_neighbour_s_metrics() {
    let doc = composite_font("/Identity-H", VERTICAL_METRICS, "/Identity", &[]);
    let font = font_at(&doc, 3);

    for cid in [4, 7, 8, 10, 11, 12, 13, 15, 16] {
        assert!(
            !font.has_vertical_metrics(cid),
            "CID {cid} sits in a gap /W2 does not name, and something put \
             metrics on it: {:?}",
            font.vertical_metrics(cid)
        );
        let (_, v_y, w1_y) = font.vertical_metrics(cid);
        assert_eq!((v_y, w1_y), (880.0, -1000.0), "CID {cid} takes the default");
    }
}

/// Milestone 1's exit criterion, second half: a font without `/DW2` reports
/// 9.7.4.3's `[880 -1000]`, not the horizontal fallback.
///
/// The fixture's `/DW` is 250 and its `/W` is 1000, so neither of those
/// numbers nor their negations can be mistaken for the default.
#[test]
fn a_font_without_dw2_reports_the_spec_default() {
    let doc = composite_font("/Identity-H", VERTICAL_METRICS, "/Identity", &[]);
    let font = font_at(&doc, 3);

    let (_, v_y, w1_y) = font.vertical_metrics(99);
    assert_eq!(v_y, 880.0, "/DW2's default position vector y");
    assert_eq!(
        w1_y, -1000.0,
        "and its default displacement, which is downward"
    );
    assert!(!font.has_vertical_metrics(99));

    // A font with no vertical metrics of any kind still has these.
    let bare = composite_font("/Identity-H", "", "/Identity", &[]);
    assert_eq!(font_at(&bare, 3).vertical_metrics(1).1, 880.0);
    assert_eq!(font_at(&bare, 3).vertical_metrics(1).2, -1000.0);
}

/// `/DW2` when the file gives one, which must beat the default and must be
/// read in the order Table 116 writes it: position vector *y* first,
/// displacement second. Swapping them would make the glyph advance 1234 units
/// **upward**, which is a column running the wrong way.
#[test]
fn dw2_overrides_the_default_in_the_order_the_table_gives() {
    let doc = composite_font("/Identity-H", "/DW2 [123 -1234]", "/Identity", &[]);
    let font = font_at(&doc, 3);
    let (_, v_y, w1_y) = font.vertical_metrics(1);
    assert_eq!(v_y, 123.0, "the first element is the position vector's y");
    assert_eq!(w1_y, -1234.0, "the second is the displacement");
}

/// 9.7.4.3 derives `v_x` from the glyph's own horizontal width rather than
/// defaulting it, so the horizontal metrics decide where a vertical glyph sits
/// sideways in its em box even when the file says nothing vertical at all.
#[test]
fn v_x_is_half_the_horizontal_width_when_w2_gives_none() {
    let doc = composite_font("/Identity-H", VERTICAL_METRICS, "/Identity", &[]);
    let font = font_at(&doc, 3);

    assert_eq!(font.width_of(20).0, 500.0, "the /W entry this derives from");
    assert_eq!(font.vertical_metrics(20).0, 250.0, "half of it");

    // A CID /W does not name falls back to /DW, and v_x follows it there.
    assert_eq!(font.width_of(99).0, 250.0, "/DW");
    assert_eq!(font.vertical_metrics(99).0, 125.0, "half of /DW");

    // Where /W2 *does* give one, it is used as written and not derived: CID 5
    // is 1000 wide, so a derived v_x would be 500 rather than 250.
    assert_eq!(font.width_of(5).0, 1000.0);
    assert_eq!(font.vertical_metrics(5).0, 250.0, "/W2's own v1x");
}

/// Every shape of `/W2` and `/DW2` a hostile or broken file can write. None
/// may panic (ruling 1), none may hang, and none may leave a CID with metrics
/// the file did not give it.
#[test]
fn a_malformed_w2_degrades() {
    for entry in [
        "/W2 [5]",
        "/W2 [5 []]",
        "/W2 [5 [-400]]",
        "/W2 [5 [-400 250]]",
        "/W2 [5 [-400 250 700 -410]]",
        "/W2 [5 5]",
        "/W2 [5 9 -600]",
        "/W2 [/Name [1 2 3]]",
        "/W2 [5 [(text) (more) (still)]]",
        "/W2 5",
        "/W2 << /A /B >>",
        "/W2 [0 2147483647 -1000 0 880]",
        "/W2 [-5 [-400 250 700]]",
        "/DW2 [880]",
        "/DW2 880",
        "/DW2 [(a) (b)]",
        "/DW2 [880 -1000 -1000]",
    ] {
        let doc = composite_font("/Identity-H", entry, "/Identity", &[]);
        let font = font_at(&doc, 3);
        // Reachable for every CID, whatever the file said.
        let _ = font.vertical_metrics(0);
        let _ = font.vertical_metrics(5);
        let _ = font.vertical_metrics(u32::MAX);
        assert!(
            !font.has_vertical_metrics(u32::MAX),
            "{entry} put metrics on a CID no file can address"
        );
    }

    // The partial triple is dropped rather than guessed at, and the whole
    // entry before it survives.
    let doc = composite_font(
        "/Identity-H",
        "/W2 [5 [-400 250 700 -410]]",
        "/Identity",
        &[],
    );
    let font = font_at(&doc, 3);
    assert_eq!(font.vertical_metrics(5), (250.0, 700.0, -400.0));
    assert!(!font.has_vertical_metrics(6), "a lone -410 is not a triple");
}

/// A simple font has no `/W2` to read and never writes vertically, but the
/// accessor is on `Font` and a caller may still reach it. It answers the
/// spec's default rather than zeros, which is the least wrong thing available.
#[test]
fn a_simple_font_answers_the_default_vertical_metrics() {
    let doc = open("simple-text.pdf");
    let fonts = first_page_fonts(&doc);
    let font = fonts.first().expect("a font");
    assert_eq!(font.vertical_metrics(u32::from(b'T')).1, 880.0);
    assert_eq!(font.vertical_metrics(u32::from(b'T')).2, -1000.0);
    assert!(!font.has_vertical_metrics(u32::from(b'T')));
}

// ---------------------------------------------------------------------------
// `usecmap` and `/UseCMap` (gap 04, 9.7.5.3).
// ---------------------------------------------------------------------------

/// The parent these tests inherit from, written out here.
///
/// **Not a predefined CMap, and nothing below is evidence about one.** Gap 03
/// is what makes `90ms-RKSJ-H` and its family real; until it lands,
/// `CMap::predefined` answers those names with a two-byte identity stub, so a
/// fixture inheriting from one would be measuring the stub and would keep
/// passing when the stub was replaced. What these prove is that a parent,
/// whatever it turns out to contain, reaches the child intact.
///
/// Its `cidrange` is the **decoy**: it covers the codes the child covers and
/// answers them with different CIDs, so a merge that let the parent win could
/// not pass by accident.
const PARENT_CMAP: &[u8] = b"/CIDInit /ProcSet findresource begin\n\
     begincmap\n\
     /CMapName /TestParent def\n\
     1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
     1 begincidrange <0041> <005A> 100 endcidrange\n\
     1 begincidchar <0061> 900 endcidchar\n\
     endcmap end";

/// A child declaring one `cidrange` and nothing else — no codespace, so
/// without inheritance its two-byte codes split into single bytes.
const CHILD_CMAP: &[u8] = b"/CIDInit /ProcSet findresource begin\n\
     begincmap\n\
     1 begincidrange <0041> <005A> 200 endcidrange\n\
     endcmap end";

/// Milestone 2's exit criterion, name form: a CMap inheriting through the
/// stream dictionary resolves identically to one inheriting in the body.
///
/// The parent is `Identity-H` because that is the only parent *both*
/// spellings can reach. A body `usecmap` names a CMap, and a name has no
/// address in a document — 9.7.5.2's predefined set is the only place one can
/// come from — while the dictionary form is the one that may point at a
/// stream. So the two spellings are compared over the parent they share.
#[test]
fn the_body_and_the_dictionary_spellings_of_usecmap_agree() {
    let in_body = composite_font(
        "5 0 R",
        "",
        "/Identity",
        &[stream(
            b"/Identity-H usecmap\n1 begincidchar <0041> 7 endcidchar",
        )],
    );
    let in_dict = composite_font(
        "5 0 R",
        "",
        "/Identity",
        &[stream_with(
            "/UseCMap /Identity-H",
            b"1 begincidchar <0041> 7 endcidchar",
        )],
    );

    let body = font_at(&in_body, 3);
    let dict = font_at(&in_dict, 3);
    for bytes in [
        [0x00u8, 0x41].as_slice(),
        &[0x00, 0x42],
        &[0x12, 0x34],
        &[0x00, 0x41, 0x00, 0x42],
        &[0x41],
    ] {
        assert_eq!(
            body.decode(bytes),
            dict.decode(bytes),
            "the two spellings disagree on {bytes:02X?}"
        );
    }
    assert_eq!(
        body.decode(&[0x00, 0x41])[0].cid,
        7,
        "the child's own entry"
    );
    assert_eq!(
        body.decode(&[0x00, 0x42])[0].cid,
        0x42,
        "and the parent's identity where the child says nothing"
    );
}

/// Milestone 2's exit criterion, stream form. This is the path the resolver
/// closure exists for: the parent is a document stream, and the leaf crate
/// that walks the chain cannot fetch one (ruling 8).
///
/// The child declares no codespace at all, so a two-byte string splits into
/// single bytes without the parent's — which is the reported defect.
#[test]
fn a_parent_stream_named_by_the_dictionary_is_inherited() {
    let alone = composite_font("5 0 R", "", "/Identity", &[stream(CHILD_CMAP)]);
    let decoded = font_at(&alone, 3).decode(&[0x00, 0x41]);
    assert_eq!(
        decoded.iter().map(|d| d.bytes).collect::<Vec<_>>(),
        vec![1, 1],
        "with no parent the child has no codespace, so it splits one byte at \
         a time — the defect, stated as an assertion"
    );

    let inherited = composite_font(
        "5 0 R",
        "",
        "/Identity",
        &[
            stream_with("/UseCMap 6 0 R", CHILD_CMAP),
            stream(PARENT_CMAP),
        ],
    );
    let decoded = font_at(&inherited, 3).decode(&[0x00, 0x41]);
    assert_eq!(
        decoded.len(),
        1,
        "the parent's codespace makes this one code"
    );
    assert_eq!(decoded[0].bytes, 2);
    assert_eq!(decoded[0].code, 0x41);
}

/// The risk this gap's own table calls out: merge order silently wrong, with
/// the parent overriding the child. Both name `<0041>` to `<005A>` and they
/// disagree, so no accident produces the right answer.
#[test]
fn the_child_s_cid_range_beats_the_parent_s_through_a_document() {
    let parent_only = composite_font("5 0 R", "", "/Identity", &[stream(PARENT_CMAP)]);
    assert_eq!(
        font_at(&parent_only, 3).decode(&[0x00, 0x41])[0].cid,
        100,
        "what the parent alone says, which is the decoy"
    );

    let doc = composite_font(
        "5 0 R",
        "",
        "/Identity",
        &[
            stream_with("/UseCMap 6 0 R", CHILD_CMAP),
            stream(PARENT_CMAP),
        ],
    );
    let font = font_at(&doc, 3);
    assert_eq!(
        font.decode(&[0x00, 0x41])[0].cid,
        200,
        "the child's base, not the parent's"
    );
    assert_eq!(font.decode(&[0x00, 0x42])[0].cid, 201);
    assert_eq!(
        font.decode(&[0x00, 0x61])[0].cid,
        900,
        "and the parent survives where the child says nothing"
    );
}

/// A parent may itself inherit. The grandparent supplies the codespace, the
/// parent one mapping and the child another, and all three have to arrive.
#[test]
fn a_chain_two_deep_through_the_document_is_followed() {
    let doc = composite_font(
        "5 0 R",
        "",
        "/Identity",
        &[
            stream_with(
                "/UseCMap 6 0 R",
                b"1 begincidrange <0041> <0041> 200 endcidrange",
            ),
            stream_with("/UseCMap 7 0 R", b"1 begincidchar <0042> 300 endcidchar"),
            stream(
                b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
                  1 begincidchar <0043> 400 endcidchar",
            ),
        ],
    );
    let font = font_at(&doc, 3);
    assert_eq!(
        font.decode(&[0x00, 0x41])[0].bytes,
        2,
        "the grandparent's codespace"
    );
    assert_eq!(
        font.decode(&[0x00, 0x41])[0].cid,
        200,
        "the child's mapping"
    );
    assert_eq!(font.decode(&[0x00, 0x42])[0].cid, 300, "the parent's");
    assert_eq!(font.decode(&[0x00, 0x43])[0].cid, 400, "the grandparent's");
}

/// A `/UseCMap` pointing at its own stream must terminate (ruling 1), and it
/// must say so rather than reading as a CMap that never wanted a parent.
#[test]
fn a_use_cmap_pointing_at_its_own_stream_terminates() {
    let doc = composite_font(
        "5 0 R",
        "",
        "/Identity",
        &[stream_with(
            "/UseCMap 5 0 R",
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
              1 begincidchar <0041> 7 endcidchar",
        )],
    );
    let font = font_at(&doc, 3);
    assert_eq!(
        font.decode(&[0x00, 0x41])[0].cid,
        7,
        "its own work survives"
    );
    assert!(
        doc.warnings()
            .iter()
            .any(|w| w.kind == WarningKind::CMap(CMapWarning::ParentCycle)),
        "{:?}",
        doc.warnings()
    );
}

/// Two streams that use each other close the loop one link later, which a
/// guard keyed on the name of the link would not notice.
#[test]
fn two_streams_that_use_each_other_terminate() {
    let doc = composite_font(
        "5 0 R",
        "",
        "/Identity",
        &[
            stream_with("/UseCMap 6 0 R", b"1 begincidchar <0041> 7 endcidchar"),
            stream_with("/UseCMap 5 0 R", b"1 begincidchar <0042> 8 endcidchar"),
        ],
    );
    let font = font_at(&doc, 3);
    assert_eq!(font.decode(&[0x41])[0].cid, 7);
    assert!(doc
        .warnings()
        .iter()
        .any(|w| w.kind == WarningKind::CMap(CMapWarning::ParentCycle)));
}

/// Milestone 3's provenance half (ruling 10): the warning names the section
/// that stopped early *and* the stream it was in, so a document with two
/// CMaps says which one is damaged.
#[test]
fn a_truncated_section_warns_against_the_stream_it_was_in() {
    let doc = composite_font(
        "5 0 R",
        "",
        "/Identity",
        &[stream(
            b"1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
              1 begincidrange <0041> <005A>",
        )],
    );
    let _ = font_at(&doc, 3);

    let warnings = doc.warnings();
    let mine: Vec<_> = warnings
        .iter()
        .filter(|w| matches!(w.kind, WarningKind::CMap(_)))
        .collect();
    assert_eq!(mine.len(), 1, "{warnings:?}");
    assert_eq!(
        mine[0].kind,
        WarningKind::CMap(CMapWarning::SectionUnterminated(CMapSection::CidRange))
    );
    assert_eq!(
        mine[0].object,
        Some(ObjRef::new(5, 0)),
        "attributed to the CMap stream, not to the font that used it"
    );
}

/// A `/UseCMap` naming nothing usable degrades to the child alone and reports
/// it, rather than reading as a CMap that never wanted a parent.
#[test]
fn an_unresolvable_use_cmap_is_reported() {
    for entry in ["/UseCMap /NotACMapAnybodyHas", "/UseCMap 99 0 R"] {
        let doc = composite_font(
            "5 0 R",
            "",
            "/Identity",
            &[stream_with(entry, b"1 begincidchar <0041> 7 endcidchar")],
        );
        let font = font_at(&doc, 3);
        assert_eq!(font.decode(&[0x41])[0].cid, 7, "{entry}");
        assert!(
            doc.warnings()
                .iter()
                .any(|w| w.kind == WarningKind::CMap(CMapWarning::ParentUnresolved)),
            "{entry}: {:?}",
            doc.warnings()
        );
    }
}

/// A `/ToUnicode` CMap goes through the same reader, so it inherits too.
#[test]
fn a_to_unicode_cmap_inherits_as_well() {
    let doc = document(&[
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 4 0 R >>".to_vec(),
        stream_with("/UseCMap 5 0 R", b"1 beginbfchar <41> <0061> endbfchar"),
        stream(b"1 beginbfrange <41> <5A> <0391> endbfrange"),
    ]);
    let font = font_at(&doc, 3);
    assert_eq!(font.text_of(0x41), "a", "the child's own entry wins");
    assert_eq!(
        font.text_of(0x42),
        "\u{392}",
        "and the parent covers what the child did not restate"
    );
}

/// Every shape of `/UseCMap` a hostile file can write. None may panic
/// (ruling 1), and none may leave the font unusable.
#[test]
fn a_malformed_use_cmap_degrades() {
    // The last two point at the font dictionary and at the descendant, which
    // are objects that exist and are not CMap streams.
    for entry in [
        "/UseCMap 42",
        "/UseCMap [1 2 3]",
        "/UseCMap << /A /B >>",
        "/UseCMap null",
        "/UseCMap 3 0 R",
        "/UseCMap 4 0 R",
    ] {
        let doc = composite_font(
            "5 0 R",
            "",
            "/Identity",
            &[stream_with(entry, b"1 begincidchar <0041> 7 endcidchar")],
        );
        let font = font_at(&doc, 3);
        assert_eq!(font.decode(&[0x41])[0].cid, 7, "{entry}");
    }
}

// ---------------------------------------------------------------------------
// Predefined CMaps, surfaced (gap 03 milestone 5, 9.7.5.2).
// ---------------------------------------------------------------------------

/// `Font::encoding_is_approximate` is where `CMap::is_approximate` finally
/// has a production caller, which it did not from the day it was written.
///
/// The flag means one thing: the code-to-CID mapping is assumed rather than
/// known. In a default build nothing is assumed, because the whole registry
/// is compiled in — so this asserts *false*, which is the more useful half.
/// With `cmap-predefined` off the same font reports true and the engine
/// warns; the two are the same expression here so that the assertion cannot
/// quietly become vacuous on either side.
#[test]
fn a_composite_font_says_whether_its_cid_mapping_is_assumed() {
    for name in ["/90ms-RKSJ-H", "/UniJIS-UCS2-H", "/ETen-B5-H"] {
        let doc = composite_font(name, "", "/Identity", &[]);
        let font = font_at(&doc, 3);
        assert_eq!(
            font.encoding_is_approximate(),
            cfg!(not(feature = "cmap-predefined")),
            "{name}"
        );
    }

    // Identity is a rule rather than a table, so it is exact in every build.
    let identity = composite_font("/Identity-H", "", "/Identity", &[]);
    assert!(!font_at(&identity, 3).encoding_is_approximate());

    // And so is a simple font, which has no CMap to be approximate about.
    let simple = document(&[b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec()]);
    assert!(!font_at(&simple, 3).encoding_is_approximate());
}

/// The registry reaches the CID through a document, not only through the leaf
/// crate's own tests: `<8140>` is CID 633 in `90ms-RKSJ-H`, and the string
/// splits one byte then two.
#[cfg(feature = "cmap-predefined")]
#[test]
fn a_predefined_encoding_name_decodes_through_the_registry() {
    let doc = composite_font("/90ms-RKSJ-H", "", "/Identity", &[]);
    let font = font_at(&doc, 3);
    let codes = font.decode(&[0x41, 0x81, 0x40]);
    assert_eq!(codes.len(), 2, "one one-byte code and one two-byte one");
    assert_eq!(
        (codes[0].code, codes[0].bytes, codes[0].cid),
        (0x41, 1, 264)
    );
    assert_eq!(
        (codes[1].code, codes[1].bytes, codes[1].cid),
        (0x8140, 2, 633)
    );
}

/// A name outside 9.7.5.2's set is warned about, by name, and nothing is
/// substituted for it. `WarningKind` carries an interned `Name` because a
/// warning that cannot say *which* CMap is missing is not actionable.
#[test]
fn an_unknown_predefined_name_is_warned_about_by_name() {
    let doc = composite_font("/90ms-RKSJ-Q", "", "/Identity", &[]);
    let font = font_at(&doc, 3);
    let named: Vec<String> = doc
        .warnings()
        .iter()
        .filter_map(|w| match w.kind {
            WarningKind::PredefinedCMapUnknown(n) => doc.name_bytes(n),
            _ => None,
        })
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .collect();
    assert_eq!(named, vec!["90ms-RKSJ-Q".to_string()]);

    // And with no CMap the font falls back to one byte per code rather than
    // inventing a codespace, so the codes are honest fragments instead of
    // plausible CIDs.
    assert_eq!(font.decode(&[0x41, 0x81, 0x40]).len(), 3);
}
