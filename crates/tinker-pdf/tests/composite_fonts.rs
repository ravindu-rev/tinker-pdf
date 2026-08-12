//! A composite font draws the glyph its CID names (gap 02, plan 05).
//!
//! A Type 0 font's code is a byte sequence, and 9.7.4 makes the CID that its
//! encoding CMap produces — not the code — the thing that selects both the
//! advance and the glyph. Until August 2026 only the advance was told. The
//! glyph lookup was handed the raw code, which went through `/ToUnicode` to a
//! character and the character through the font program's own `cmap`, so a CJK
//! document laid out at exactly the right spacing and drew the wrong
//! characters. It reads as a font substitution rather than an engine bug,
//! which is why it survived.
//!
//! `/CIDToGIDMap` (9.7.4.2) was read nowhere at all. A subsetter renumbers
//! glyphs and the CIDs must not move with them, so almost every embedded CJK
//! font in the wild carries the stream form — and a CID indexing a table that
//! is not the identity, without the table, draws a *different* wrong glyph.
//!
//! Every byte of every font here is built in this file. There is no CJK
//! fixture anywhere in this repository, and `testdata/` is four self-authored
//! PDFs that are not to be modified. What makes a hand-built fixture worth
//! more than a real font for this question is that **the code, the CID and the
//! glyph are three distinct numbers**: 0x41, 7 and 3. Where any two of them
//! coincide — which is what `Identity-H` over `/Identity` does, and what every
//! file that works today is made of — a reader that discards the CID and one
//! that uses it are indistinguishable.

use tinker_pdf::{Bitmap, Document, RenderOptions, RenderWarning};

// ---------------------------------------------------------------------------
// Building a TrueType font program.
// ---------------------------------------------------------------------------

/// One glyph's `glyf` entry: a box `size` font units on a side, from the
/// origin.
///
/// Boxes of different sizes rather than letter shapes, because the size is
/// what the rendered ink can be measured against. The bounding box of what a
/// page painted says *which* glyph ran, which is the whole question here — a
/// count of inked pixels would pass with any glyph at all.
fn box_glyph(size: i16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&1i16.to_be_bytes()); // one contour
    out.extend_from_slice(&0i16.to_be_bytes()); // xMin
    out.extend_from_slice(&0i16.to_be_bytes()); // yMin
    out.extend_from_slice(&size.to_be_bytes()); // xMax
    out.extend_from_slice(&size.to_be_bytes()); // yMax
    out.extend_from_slice(&3u16.to_be_bytes()); // last point of the contour
    out.extend_from_slice(&0u16.to_be_bytes()); // no hinting instructions

    // Every point is on-curve and every delta is a signed 16-bit word: none
    // of the short-coordinate or repeat flag bits are set, which is larger
    // than a real font would write and far easier to read.
    out.extend_from_slice(&[1, 1, 1, 1]);
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

/// A `cmap` subtable in format 4 mapping each code to one glyph, one segment
/// apiece with a delta and no glyph array.
///
/// `entries` must be in ascending code order, which the format requires and a
/// reader relies on: the search stops at the first segment whose end is not
/// below the code, so an unsorted table answers `.notdef` for everything past
/// the first segment. That is a fixture that passes tests by not working.
fn cmap_table(entries: &[(u16, u16)]) -> Vec<u8> {
    assert!(
        entries.windows(2).all(|p| p[0].0 < p[1].0),
        "format 4 segments are ordered by code"
    );
    let segments = entries.len() as u16 + 1; // plus the 0xFFFF terminator.

    let mut sub = Vec::new();
    for value in [4u16, 0, 0, segments * 2, 0, 0, 0] {
        sub.extend_from_slice(&value.to_be_bytes());
    }
    for (code, _) in entries {
        sub.extend_from_slice(&code.to_be_bytes()); // endCode
    }
    sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
    sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    for (code, _) in entries {
        sub.extend_from_slice(&code.to_be_bytes()); // startCode
    }
    sub.extend_from_slice(&0xFFFFu16.to_be_bytes());
    for (code, glyph) in entries {
        sub.extend_from_slice(&glyph.wrapping_sub(*code).to_be_bytes()); // idDelta
    }
    sub.extend_from_slice(&1u16.to_be_bytes());
    for _ in 0..segments {
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset
    }

    let mut cmap = Vec::new();
    // One (3,1) Windows Unicode BMP subtable, the one a reader prefers.
    for value in [0u16, 1, 3, 1] {
        cmap.extend_from_slice(&value.to_be_bytes());
    }
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&sub);
    cmap
}

/// A TrueType face whose glyph `g`, counting from 1, is a box `sizes[g - 1]`
/// units on a side. Glyph 0 is `.notdef` and draws nothing.
///
/// `cmap` is written verbatim when given and the table is omitted when it is
/// not — a subset font whose `cmap` a producer dropped is a real file, and it
/// is the case the last-resort branch of the simple-font lookup exists for.
fn boxes_font(sizes: &[i16], cmap: Option<Vec<u8>>) -> Vec<u8> {
    let glyphs = sizes.len() as u16 + 1;

    let mut glyf: Vec<u8> = Vec::new();
    let mut loca: Vec<u32> = vec![0, 0]; // glyph 0 is an empty `loca` range.
    for size in sizes {
        glyf.extend_from_slice(&box_glyph(*size));
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
        hmtx.extend_from_slice(&600u16.to_be_bytes()); // advance
        hmtx.extend_from_slice(&0i16.to_be_bytes()); // left side bearing
    }

    let cmap = cmap.unwrap_or_default();
    let mut tables: Vec<(&[u8; 4], &[u8])> = vec![
        (b"glyf", &glyf),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca_bytes),
        (b"maxp", &maxp),
    ];
    if !cmap.is_empty() {
        tables.insert(0, (b"cmap", &cmap));
    }

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

/// The four glyphs every fixture here uses, glyph 1 upward.
///
/// Glyph 3 is what the composite fixtures should draw and glyph 2 is what a
/// reader still going through the font's `cmap` would draw, so the two are
/// far enough apart that no tolerance could confuse them.
const SIZES: [i16; 4] = [300, 900, 600, 450];

/// The decoy: the font's own `cmap` says `A` is glyph 2, the 900-unit box.
///
/// A composite font's code is not a character, so 9.7.4.2 never consults this
/// table — but the code used to reach it through `/ToUnicode`, and code 0x41
/// means `A` to the standard encoding. Every composite test below asserts a
/// box that is *not* 900 units, which is what proves the `cmap` is no longer
/// asked a question it cannot answer.
fn decoy_cmap() -> Vec<u8> {
    cmap_table(&[(0x03, 2), (0x41, 2), (0x42, 2)])
}

// ---------------------------------------------------------------------------
// Building the document around it.
// ---------------------------------------------------------------------------

/// Assembles a one-page PDF from numbered objects, starting at object 5.
fn document(objects: &[Vec<u8>], content: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    out.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100]\n\
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

/// A stream object holding raw bytes.
fn stream(prefix: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {prefix} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A `/CIDToGIDMap` stream: two big-endian bytes per CID from zero up.
fn cid_to_gid_stream(map: &[u16]) -> Vec<u8> {
    let bytes: Vec<u8> = map.iter().flat_map(|g| g.to_be_bytes()).collect();
    stream("", &bytes)
}

/// The encoding CMap that makes the code and the CID different numbers:
/// code 0x41 is CID 7, and nothing else is mapped.
fn cid_cmap() -> Vec<u8> {
    stream(
        "/Type /CMap",
        b"/CIDInit /ProcSet findresource begin\n\
          1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
          1 begincidchar <0041> 7 endcidchar\n\
          end",
    )
}

/// A composite font over a TrueType program, drawing `codes` at 48 point.
///
/// `encoding` and `cid_to_gid` are written into the dictionaries verbatim, so
/// each may be a predefined name or a reference to the stream that follows.
fn composite_document(
    program: &[u8],
    encoding: &str,
    widths: &str,
    cid_to_gid: &str,
    extra: &[Vec<u8>],
    codes: &[u16],
) -> Vec<u8> {
    let hex: String = codes.iter().map(|c| format!("{c:04X}")).collect();
    let mut objects = vec![
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /Fixture /Encoding {encoding}\n\
             /DescendantFonts [8 0 R] >>"
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /Fixture /Flags 4 /FontFile2 7 0 R >>".to_vec(),
        stream("", program),
        format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Fixture /DW 250 {widths}\n\
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>\n\
             /CIDToGIDMap {cid_to_gid} /FontDescriptor 6 0 R >>"
        )
        .into_bytes(),
    ];
    objects.extend_from_slice(extra);
    document(&objects, &format!("BT /F0 48 Tf 20 30 Td <{hex}> Tj ET"))
}

/// A simple TrueType font over the same program, for the paths this gap must
/// leave exactly as they were.
fn simple_document(program: &[u8], text: &[u8]) -> Vec<u8> {
    let literal: String = text.iter().map(|b| format!("\\{b:03o}")).collect();
    document(
        &[
            b"<< /Type /Font /Subtype /TrueType /BaseFont /Fixture /FirstChar 0\n\
              /LastChar 255 /FontDescriptor 6 0 R >>"
                .to_vec(),
            b"<< /Type /FontDescriptor /FontName /Fixture /Flags 4 /FontFile2 7 0 R >>".to_vec(),
            stream("", program),
        ],
        &format!("BT /F0 48 Tf 20 30 Td ({literal}) Tj ET"),
    )
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

/// The pixels a box of `size` font units covers at 48 point.
fn pixels(size: f64) -> f64 {
    size / 1000.0 * 48.0
}

/// Asserts the page drew one box of `size` font units, and nothing else.
#[track_caller]
fn drew_box(bitmap: &Bitmap, size: f64, what: &str) {
    let expected = pixels(size);
    let (w, h) = ink_box(bitmap);
    assert!(
        (w - expected).abs() <= 2.0 && (h - expected).abs() <= 2.0,
        "{what}: drew a {w} by {h} pixel shape, not the {expected} square the \
         {size}-unit glyph makes. Warnings: {:?}",
        bitmap.warnings,
    );
}

/// The font dictionary the page's `/F0` names, read back out of the document
/// so that the advance and the glyph are measured from the same bytes.
fn font_of(bytes: &[u8]) -> std::sync::Arc<tinker_pdf_cos::Font> {
    let doc = Document::open(bytes.to_vec()).expect("it opens");
    let cos = doc.cos();
    let page = tinker_pdf_cos::pages::collect(cos);
    let resources = page
        .first()
        .and_then(|p| p.resources.clone())
        .expect("the page has resources");
    tinker_pdf_cos::font::from_resources(cos, &resources)
        .into_values()
        .next()
        .expect("the page has one font")
}

// ---------------------------------------------------------------------------
// The tests.
// ---------------------------------------------------------------------------

/// Milestone 3's exit criterion, and the mitigation this gap exists for: one
/// fixture, a non-identity CMap and a non-identity `/CIDToGIDMap`, and the
/// advance and the glyph asserted together.
///
/// Three distinct numbers. The code is 0x41; the encoding CMap makes it CID
/// 7; `/CIDToGIDMap` makes CID 7 glyph 3, which is the 600-unit box. `/W`
/// gives CID 7 an advance of 800. A reader using the code as the glyph would
/// find nothing at glyph 65 and draw a blank page; one going through the
/// font's `cmap`, as this engine did, would draw glyph 2's 900-unit box.
#[test]
fn a_subsetted_composite_font_draws_the_glyph_its_map_names() {
    let program = boxes_font(&SIZES, Some(decoy_cmap()));
    let bytes = composite_document(
        &program,
        "9 0 R",
        "/W [7 [800]]",
        "10 0 R",
        &[cid_cmap(), cid_to_gid_stream(&[0, 0, 0, 0, 0, 0, 0, 3])],
        &[0x0041],
    );

    // The advance, from the CID.
    let font = font_of(&bytes);
    let decoded = font.decode(&[0x00, 0x41]);
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].code, 0x41, "the code the string held");
    assert_eq!(decoded[0].cid, 7, "the CID the encoding CMap names");
    assert_eq!(decoded[0].width, 800.0, "the advance /W gives that CID");

    // The glyph, from the same CID.
    let page = render(bytes);
    drew_box(&page, 600.0, "CID 7 through a non-identity /CIDToGIDMap");
    assert!(!page.warnings.contains(&RenderWarning::UnreadableFont));
}

/// The same fixture with two codes, so the advance is measured in pixels
/// rather than only asserted as a number. The two boxes together span one
/// advance plus one box; a wrong advance moves the second box and a wrong
/// glyph changes both.
#[test]
fn the_advance_and_the_glyph_agree_on_the_page() {
    let program = boxes_font(&SIZES, Some(decoy_cmap()));
    let page = render(composite_document(
        &program,
        "9 0 R",
        "/W [7 [800]]",
        "10 0 R",
        &[cid_cmap(), cid_to_gid_stream(&[0, 0, 0, 0, 0, 0, 0, 3])],
        &[0x0041, 0x0041],
    ));

    let (w, h) = ink_box(&page);
    let expected = pixels(800.0) + pixels(600.0);
    assert!(
        (w - expected).abs() <= 2.0,
        "two 600-unit boxes 800 units apart span {expected} pixels, not {w}"
    );
    assert!(
        (h - pixels(600.0)).abs() <= 2.0,
        "and they are one box tall, not {h}"
    );
}

/// Milestone 2's other exit criterion: a CID the map does not reach draws
/// `.notdef` and the page says so. Without the report it is a space, which is
/// the failure that looks like nothing happened.
///
/// Code 0x42 is not in the encoding CMap, so it is its own CID (9.7.4) — and
/// CID 0x42 is far past the end of an eight-entry table.
#[test]
fn a_cid_past_the_end_of_the_map_draws_notdef_and_says_so() {
    let program = boxes_font(&SIZES, Some(decoy_cmap()));
    let page = render(composite_document(
        &program,
        "9 0 R",
        "/W [7 [800]]",
        "10 0 R",
        &[cid_cmap(), cid_to_gid_stream(&[0, 0, 0, 0, 0, 0, 0, 3])],
        &[0x0042],
    ));

    assert_eq!(ink(&page), 0, "`.notdef` in this face draws nothing");
    assert!(
        page.warnings.contains(&RenderWarning::UnreadableFont),
        "and the page reports that a glyph could not be resolved: {:?}",
        page.warnings
    );
}

/// Milestone 1 at the rendering level. `Identity-H` makes the CID the code
/// and `/Identity` makes the glyph the CID, so the file a producer actually
/// embeds — a subset with no `cmap` of its own, because a composite font has
/// no use for one — cannot move: code 3 draws glyph 3, exactly as it did when
/// the code was used as the glyph index directly. This is the test that says
/// the fix is safe to ship, and it passes on both sides of the change.
#[test]
fn an_identity_h_font_over_an_identity_map_does_not_move() {
    let program = boxes_font(&SIZES, None);
    for (code, size) in [(1u16, 300.0), (2, 900.0), (3, 600.0), (4, 450.0)] {
        let page = render(composite_document(
            &program,
            "/Identity-H",
            "",
            "/Identity",
            &[],
            &[code],
        ));
        drew_box(&page, size, &format!("code {code} under Identity-H"));
        assert!(!page.warnings.contains(&RenderWarning::UnreadableFont));
    }
}

/// What *does* move, stated rather than glossed over. An `Identity-H` font
/// whose embedded face happens to carry a `cmap` used to select through it,
/// because the code went to a character and the character to that table. The
/// CID says glyph 3 and the `cmap` says glyph 2, and the CID is right: a
/// subset renumbers glyphs, so the surviving `cmap` of a face that was
/// subsetted for a composite font describes the original numbering and not
/// this one.
///
/// This is the only case in which a file that opened before renders
/// differently now, and it renders correctly.
#[test]
fn an_identity_h_face_with_a_cmap_now_follows_the_cid_not_the_character() {
    let program = boxes_font(&SIZES, Some(decoy_cmap()));
    let sfnt = tinker_pdf_font::Sfnt::parse(&program).expect("the fixture parses");
    assert_eq!(
        sfnt.glyph_for_char('\u{3}'),
        Some(2),
        "the `cmap` answers 2 for the character code 3 stands for"
    );

    let page = render(composite_document(
        &program,
        "/Identity-H",
        "",
        "/Identity",
        &[],
        &[0x0003],
    ));
    drew_box(&page, 600.0, "CID 3, not the `cmap`'s glyph 2");
}

/// The decoy is really a decoy: the font's own `cmap` maps `A` to glyph 2,
/// and 9.7.4.2 never asks it. Without this, "the composite path does not
/// consult the `cmap`" would be a claim about a table that might have been
/// empty all along.
#[test]
fn the_font_s_own_cmap_is_not_consulted_for_a_composite_font() {
    let program = boxes_font(&SIZES, Some(decoy_cmap()));
    let sfnt = tinker_pdf_font::Sfnt::parse(&program).expect("the fixture parses");
    assert_eq!(
        sfnt.glyph_for_char('A'),
        Some(2),
        "the `cmap` answers 2 for `A`, which is the 900-unit box"
    );

    // Code 0x41 means `A` to the standard encoding, so the old path reached
    // that table through `/ToUnicode` and drew glyph 2. The CID says 3.
    let page = render(composite_document(
        &program,
        "9 0 R",
        "",
        "10 0 R",
        &[cid_cmap(), cid_to_gid_stream(&[0, 0, 0, 0, 0, 0, 0, 3])],
        &[0x0041],
    ));
    drew_box(&page, 600.0, "the CID, not the character");
}

/// A simple font keeps the path it had: the code *is* what the encoding maps,
/// and the font's own `cmap` is the right table to ask. Code 0x41 draws glyph
/// 2 here — the same table, the same answer, and the opposite of what the
/// composite fixture above wants.
#[test]
fn a_simple_font_still_selects_through_the_font_s_cmap() {
    let program = boxes_font(&SIZES, Some(decoy_cmap()));
    let page = render(simple_document(&program, b"A"));
    drew_box(&page, 900.0, "a simple font's code through its `cmap`");
}

/// The last resort, which gap 01 left in place and this gap narrows. A subset
/// whose `cmap` was dropped has no table to ask, so the code is the only
/// glyph number on offer and every reader guesses it. That is now the only
/// case it serves: the composite reading it used to double as is handled by
/// CID before this line is reached.
#[test]
fn a_simple_font_with_no_cmap_still_falls_back_to_the_code() {
    let program = boxes_font(&SIZES, None);
    assert!(
        tinker_pdf_font::Sfnt::parse(&program)
            .expect("the fixture parses")
            .glyph_for_char('A')
            .is_none(),
        "this face has no `cmap` at all"
    );

    // Code 3, so the guess lands on the 600-unit box.
    let page = render(simple_document(&program, &[3]));
    drew_box(&page, 600.0, "the last resort");
}

/// Writes the seed `fuzz/corpus/render_page/` carries for this path, so the
/// seed and the fixture above cannot drift apart.
///
/// Worth having because nothing in any corpus had a composite font before it:
/// `/CIDToGIDMap` is a document-controlled table that this engine now indexes
/// per glyph, and no fuzz target could reach the code that indexes it.
///
/// Run with `--ignored` when the fixture changes; the corpus is committed,
/// and a run that rewrites it is a diff to look at rather than apply blindly.
#[test]
#[ignore = "writes into fuzz/corpus/render_page, which is committed"]
fn write_the_fuzz_seed() {
    let base =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/render_page");
    // Two codes: one the CMap maps and the map keeps, and one it does not, so
    // the seed reaches both the hit and the `.notdef` path before a single
    // byte is mutated.
    let bytes = composite_document(
        &boxes_font(&SIZES, Some(decoy_cmap())),
        "9 0 R",
        "/W [7 [800]]",
        "10 0 R",
        &[cid_cmap(), cid_to_gid_stream(&[0, 0, 0, 0, 0, 0, 0, 3])],
        &[0x0041, 0x0042],
    );
    std::fs::write(base.join("composite-font.pdf"), bytes).expect("the corpus directory is there");
}

/// A hostile `/CIDToGIDMap` is bytes like any other. None of these may panic
/// (ruling 1), and none may draw a glyph the document did not name.
#[test]
fn a_malformed_cid_to_gid_map_degrades() {
    let program = boxes_font(&SIZES, Some(decoy_cmap()));
    for entry in ["/Identity", "/Nonsense", "42", "99 0 R", "9 0 R"] {
        // The last case points the map at the CMap stream, which is a stream
        // of the wrong thing entirely.
        let page = render(composite_document(
            &program,
            "/Identity-H",
            "",
            entry,
            &[cid_cmap()],
            &[0x0003],
        ));
        let (w, h) = ink_box(&page);
        assert!(
            w <= pixels(900.0) + 2.0 && h <= pixels(900.0) + 2.0,
            "{entry} drew {w} by {h}, which is no glyph in this face"
        );
    }
}
