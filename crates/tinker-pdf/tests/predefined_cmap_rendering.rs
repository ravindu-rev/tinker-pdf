//! A document naming a predefined CMap draws the glyphs the registry chooses
//! (gap 03, 9.7.5.2).
//!
//! `/Encoding /90ms-RKSJ-H` is how a Japanese file usually says which encoding
//! its codes are in — a name, not an embedded stream, because the CMap is one
//! Adobe publishes and every reader is expected to have. This engine had
//! fourteen registry prefixes and a two-byte identity stub instead, so the
//! name resolved to something plausible and wrong: the string split at the
//! wrong byte boundaries, each fragment became a CID the file never named,
//! and because the CID also drives `/W`, the advances went with it.
//!
//! These are pixels rather than parsed ranges, which gap 02 is what makes
//! possible: before it a composite font's CID was computed and discarded, so
//! no CMap could reach a glyph. The measurement is the ink bounding box, gap
//! 01's technique — the glyphs are boxes of different sizes, so what the page
//! painted says *which* glyph ran, where counting inked pixels would pass
//! with any glyph at all.
//!
//! Unlike `cmap_inheritance.rs`, nothing here is hand-built. Every code, CID
//! and codespace below is quoted from
//! `crates/tinker-pdf-font/data/cmap-resources/Adobe-Japan1-7/CMap/`.
//!
//! The tables are behind `cmap-predefined`, so this whole file is. What a
//! build without them does instead is
//! `tinker-pdf-font/tests/predefined_cmaps_absent.rs`.
#![cfg(feature = "cmap-predefined")]

use tinker_pdf::{Bitmap, Document, RenderOptions, RenderWarning};

// ---------------------------------------------------------------------------
// A TrueType face of boxes.
// ---------------------------------------------------------------------------

/// The glyphs, glyph 1 upward. Far enough apart that no tolerance could
/// confuse one with another.
const SIZES: [i16; 3] = [300, 900, 600];

/// One glyph's `glyf` entry: a box `size` font units on a side.
fn box_glyph(size: i16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&1i16.to_be_bytes()); // one contour
    out.extend_from_slice(&0i16.to_be_bytes()); // xMin
    out.extend_from_slice(&0i16.to_be_bytes()); // yMin
    out.extend_from_slice(&size.to_be_bytes()); // xMax
    out.extend_from_slice(&size.to_be_bytes()); // yMax
    out.extend_from_slice(&3u16.to_be_bytes()); // last point of the contour
    out.extend_from_slice(&0u16.to_be_bytes()); // no hinting instructions
    out.extend_from_slice(&[1, 1, 1, 1]); // on-curve, 16-bit deltas
    for dx in [0i16, size, 0, -size] {
        out.extend_from_slice(&dx.to_be_bytes());
    }
    for dy in [0i16, 0, size, 0] {
        out.extend_from_slice(&dy.to_be_bytes());
    }
    while out.len() % 4 != 0 {
        out.push(0);
    }
    out
}

/// A face whose glyph `g`, counting from 1, is a box `SIZES[g - 1]` units on a
/// side. Glyph 0 is `.notdef` and draws nothing.
///
/// No `cmap` table: 9.7.4.2 addresses a descendant font by CID and never
/// consults the face's own character map, which gap 02 proved with a decoy.
fn boxes_font() -> Vec<u8> {
    let glyphs = SIZES.len() as u16 + 1;

    let mut glyf: Vec<u8> = Vec::new();
    let mut loca: Vec<u32> = vec![0, 0]; // glyph 0 is an empty `loca` range
    for size in SIZES {
        glyf.extend_from_slice(&box_glyph(size));
        loca.push(glyf.len() as u32);
    }
    let loca_bytes: Vec<u8> = loca.iter().flat_map(|o| o.to_be_bytes()).collect();

    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // unitsPerEm
    head[50..52].copy_from_slice(&1i16.to_be_bytes()); // long `loca` offsets

    let mut maxp = vec![0u8; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&glyphs.to_be_bytes());

    let mut hhea = vec![0u8; 36];
    hhea[34..36].copy_from_slice(&glyphs.to_be_bytes()); // numberOfHMetrics

    let mut hmtx = Vec::new();
    for _ in 0..glyphs {
        hmtx.extend_from_slice(&600u16.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }

    let tables: Vec<(&[u8; 4], &[u8])> = vec![
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca_bytes),
        (b"maxp", &maxp),
    ];

    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]); // search hints, unread

    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // checksum
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

// ---------------------------------------------------------------------------
// The CIDs the registry gives, and the glyphs they reach.
// ---------------------------------------------------------------------------

/// Codes and their registry CIDs, quoted from the vendored files.
///
/// From `90ms-RKSJ-H`:
///
/// ```text
/// <20> <7d>      231      so <41> is 231 + 0x21 = 264
/// <8140> <817e>  633      so <8140> is 633
/// <a0> <df>      326      so <b1> is 326 + 0x11 = 343
/// ```
///
/// From `90ms-RKSJ-V`, which is `/90ms-RKSJ-H usecmap` plus 78 overrides:
///
/// ```text
/// <8141> <8142> 7887      where the parent would say 634
/// ```
const ASCII_A: u32 = 264;
const KANJI: u32 = 633;
const KATAKANA: u32 = 343;
const ROTATED: u32 = 7887;

/// `/CIDToGIDMap` as a stream: CID to a two-byte glyph index, and every CID it
/// does not name is `.notdef`.
///
/// A stream rather than `/Identity` for the reason gap 02 gives — with the
/// identity the CID *is* the glyph index, so a face would need 7 888 glyphs
/// and a wrong CID would still land on some glyph. Here only four CIDs reach
/// anything, so a mis-split string draws a blank page rather than a different
/// picture, and blank is unmistakable.
fn cid_to_gid() -> Vec<u8> {
    let mut out = vec![0u8; (ROTATED as usize + 1) * 2];
    for (cid, gid) in [
        (ASCII_A, 1u16), // the 300-unit box
        (KANJI, 2),      // the 900-unit box
        (KATAKANA, 3),   // the 600-unit box
        (ROTATED, 3),    // the 600-unit box again
    ] {
        out[cid as usize * 2..cid as usize * 2 + 2].copy_from_slice(&gid.to_be_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// Building the document.
// ---------------------------------------------------------------------------

fn stream(prefix: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {prefix} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A page drawing `bytes` at 48 point through a Type 0 font whose `/Encoding`
/// is the predefined CMap `encoding` names.
///
/// `/W` gives each of the four live CIDs a 1000-unit advance, so the ink of a
/// two-code string spans one advance plus the second glyph's box and the sum
/// is arithmetic a reader can check.
fn page(encoding: &str, bytes: &[u8]) -> Vec<u8> {
    let objects: Vec<Vec<u8>> = vec![
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /Fixture /Encoding /{encoding}\n\
             /DescendantFonts [6 0 R] >>"
        )
        .into_bytes(),
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Fixture /DW 1000\n\
             /W [{ASCII_A} [1000] {KANJI} [1000] {KATAKANA} [1000] {ROTATED} [1000]]\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 2 >>\n\
             /CIDToGIDMap 8 0 R /FontDescriptor 7 0 R >>"
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /Fixture /Flags 4 /FontFile2 9 0 R >>".to_vec(),
        stream("", &cid_to_gid()),
        stream("", &boxes_font()),
    ];

    let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect();
    let content = format!("BT /F0 48 Tf 20 30 Td <{hex}> Tj ET");

    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    out.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100]\n\
          /Resources << /Font << /F0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
    );
    out.extend_from_slice(format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes());
    out.extend_from_slice(content.as_bytes());
    out.extend_from_slice(b"\nendstream\nendobj\n");
    for (i, object) in objects.iter().enumerate() {
        out.extend_from_slice(format!("{} 0 obj\n", i + 5).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
            objects.len() + 5
        )
        .as_bytes(),
    );
    out
}

// ---------------------------------------------------------------------------
// Measuring what came out.
// ---------------------------------------------------------------------------

fn render(bytes: Vec<u8>) -> Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn ink(bitmap: &Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|p| p.iter().any(|v| *v != 255))
        .count()
}

/// The width and height of everything the page painted, in pixels.
fn ink_box(bitmap: &Bitmap) -> (f64, f64) {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let at = (y as usize * bitmap.stride) + x as usize * bitmap.components();
            let pixel = &bitmap.data[at..at + bitmap.components()];
            if pixel.iter().any(|v| *v != 255) {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x0 > x1 {
        return (0.0, 0.0);
    }
    (f64::from(x1 - x0 + 1), f64::from(y1 - y0 + 1))
}

/// Asserts the page's ink spans `units` font units wide and `tall` high.
#[track_caller]
fn spans(bitmap: &Bitmap, units: f64, tall: f64, what: &str) {
    let (w, h) = ink_box(bitmap);
    let (want_w, want_h) = (units / 1000.0 * 48.0, tall / 1000.0 * 48.0);
    assert!(
        (w - want_w).abs() <= 2.0 && (h - want_h).abs() <= 2.0,
        "{what}: {w} by {h} pixels, not the {want_w} by {want_h} expected. \
         Warnings: {:?}",
        bitmap.warnings,
    );
    assert!(
        !bitmap.warnings.contains(&RenderWarning::UnreadableFont),
        "{what}: {:?}",
        bitmap.warnings
    );
}

// ---------------------------------------------------------------------------
// The tests.
// ---------------------------------------------------------------------------

/// Each code on its own, so the mixed-width test below is measuring a
/// combination of things already known rather than one unexplained number.
#[test]
fn each_registry_code_reaches_the_glyph_its_cid_names() {
    spans(&render(page("90ms-RKSJ-H", &[0x41])), 300.0, 300.0, "<41>");
    spans(
        &render(page("90ms-RKSJ-H", &[0x81, 0x40])),
        900.0,
        900.0,
        "<8140>",
    );
    spans(
        &render(page("90ms-RKSJ-H", &[0xB1])),
        600.0,
        600.0,
        "<b1>, half-width katakana",
    );
}

/// Milestone 3's exit criterion, on the page: a one-byte and a two-byte code
/// in the same string, split by the registry's own codespace declarations.
///
/// `41 81 40` is `A` followed by one two-byte kanji. Correctly split it is two
/// codes, CID 264 and CID 633, so the ink runs from the left of the 300-unit
/// box across one 1000-unit advance to the right edge of the 900-unit box:
/// 1900 units, and 900 tall.
///
/// Every wrong split draws a blank page, because `/CIDToGIDMap` names four
/// CIDs and nothing else. That is not a lucky property of this fixture; it is
/// why the fixture is built this way, since a blank page cannot be mistaken
/// for a slightly different one.
#[test]
fn a_one_byte_and_a_two_byte_code_split_where_the_registry_says() {
    let bitmap = render(page("90ms-RKSJ-H", &[0x41, 0x81, 0x40]));
    spans(&bitmap, 1900.0, 900.0, "<41> then <8140>");
}

/// The defect, stated as an assertion, so the test above measures a change.
///
/// `Identity-H` is what the fourteen-prefix stub amounted to: one blanket
/// two-byte codespace and code-is-CID. The same three bytes become 0x4181 and
/// a truncated 0x40, neither of which any `/CIDToGIDMap` entry names, so the
/// page comes out blank.
///
/// It does warn, and the warning is worth reading carefully because it is the
/// argument for milestone 5. `UnreadableFont` is gap 02's — a stream
/// `/CIDToGIDMap` can tell that a CID ran off its end, where `/Identity`
/// cannot. It says *this font has no glyph for that CID*. It does not say
/// *that CID came out of a CMap nobody wrote*, and on a real CJK document,
/// where the font does have a glyph for almost every CID, it would not fire
/// at all.
#[test]
fn the_blanket_two_byte_codespace_draws_nothing() {
    let bitmap = render(page("Identity-H", &[0x41, 0x81, 0x40]));
    assert_eq!(ink(&bitmap), 0, "nothing was drawn");
    assert!(
        bitmap.warnings.contains(&RenderWarning::UnreadableFont),
        "gap 02's warning, about the glyph rather than about the CMap: {:?}",
        bitmap.warnings
    );
}

/// A name outside the registry is refused rather than guessed, and the page
/// that results is honestly empty rather than dishonestly full.
///
/// `90ms-RKSJ-Q` does not exist. Under the prefix list it matched `90ms` and
/// produced the same stub `Identity-H` gives above — a page of plausible
/// glyphs from a CMap nobody wrote. `CMap::predefined` now answers `None`,
/// which leaves the font with no encoding CMap and nothing to invent from.
/// Milestone 5 is what makes the document *say* the name it could not find.
#[test]
fn a_name_the_registry_does_not_define_draws_nothing() {
    let bitmap = render(page("90ms-RKSJ-Q", &[0x41, 0x81, 0x40]));
    assert_eq!(ink(&bitmap), 0);
}

/// `90ms-RKSJ-V` against `90ms-RKSJ-H`, on the page, with the registry
/// supplying its own decoy.
///
/// Both files map `<8141>`: the horizontal one to 634 through
/// `<8140> <817e> 633`, the vertical one to 7887 through its own
/// `<8141> <8142> 7887`. `/CIDToGIDMap` sends 7887 to the 600-unit box and
/// leaves 634 unmapped, so the vertical CMap draws a box and the horizontal
/// one draws nothing. Neither answer can arrive by the fixture failing.
///
/// This is also the first evidence in the repository that a *predefined*
/// parent is inherited at all. Gap 04 built the merge and could only test it
/// with hand-made parents, and said so in every fixture: until this gap
/// landed, `90ms-RKSJ-H` was a stub and a test inheriting from it would have
/// been measuring the stub.
#[test]
fn the_vertical_cmap_overrides_its_parent_on_the_page() {
    let vertical = render(page("90ms-RKSJ-V", &[0x81, 0x41]));
    spans(&vertical, 600.0, 600.0, "<8141> through 90ms-RKSJ-V");

    let horizontal = render(page("90ms-RKSJ-H", &[0x81, 0x41]));
    assert_eq!(
        ink(&horizontal),
        0,
        "the same code through the parent is CID 634, which this face lacks"
    );

    // And what the child did *not* restate still comes from the parent, which
    // is the half a replacement rather than a merge would lose.
    spans(
        &render(page("90ms-RKSJ-V", &[0x81, 0x40])),
        900.0,
        900.0,
        "<8140> inherited from 90ms-RKSJ-H",
    );
}
