//! An embedded CMap that inherits from a parent draws what the pair of them
//! say (gap 04, 9.7.5.3).
//!
//! `usecmap` — and its dictionary spelling `/UseCMap` — is how a CMap says
//! "everything the parent defines, plus these changes". A reader that ignores
//! it gets only the changes: no codespace ranges, and whichever CID ranges the
//! child happened to restate. The codespaces are the part that hurts, because
//! they decide where one code ends and the next begins: a two-byte string read
//! by a CMap with no codespace splits into single bytes, and every code after
//! the first is a fragment of the one before.
//!
//! These are pixels rather than parsed ranges. Gap 02 is what makes that
//! possible — before it, a composite font's CID was computed and discarded, so
//! no CMap change could reach a glyph at all. The measurement is the ink
//! bounding box, gap 01's technique: the fixture's glyphs are boxes of
//! different sizes, so what the page painted says *which* glyph ran, where a
//! count of inked pixels would pass with any glyph at all.
//!
//! **The parent here is hand-built and is not a predefined CMap.** Gap 03 is
//! what makes `90ms-RKSJ-H` and its family real; until it lands,
//! `CMap::predefined` answers those names with a two-byte identity stub. A
//! fixture inheriting from one would be measuring the stub, and would keep
//! passing when the stub was replaced by something with different contents.
//! Nothing in this file is evidence that real predefined inheritance works.

use tinker_pdf::{Bitmap, Document, RenderOptions, RenderWarning};

// ---------------------------------------------------------------------------
// A TrueType face of boxes.
// ---------------------------------------------------------------------------

/// One glyph's `glyf` entry: a box `size` font units on a side, from the
/// origin. The size is what the rendered ink is measured against.
fn box_glyph(size: i16) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&1i16.to_be_bytes()); // one contour
    out.extend_from_slice(&0i16.to_be_bytes()); // xMin
    out.extend_from_slice(&0i16.to_be_bytes()); // yMin
    out.extend_from_slice(&size.to_be_bytes()); // xMax
    out.extend_from_slice(&size.to_be_bytes()); // yMax
    out.extend_from_slice(&3u16.to_be_bytes()); // last point of the contour
    out.extend_from_slice(&0u16.to_be_bytes()); // no hinting instructions

    out.extend_from_slice(&[1, 1, 1, 1]); // every point on-curve, 16-bit deltas
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
/// No `cmap` table, deliberately. A composite font addresses its descendant by
/// CID and 9.7.4.2 never consults the face's own character map, which gap 02
/// proved with a decoy table; the decoy this gap needs is a *parent CMap* that
/// answers the same codes differently, and that is built below.
fn boxes_font() -> Vec<u8> {
    let glyphs = SIZES.len() as u16 + 1;

    let mut glyf: Vec<u8> = Vec::new();
    let mut loca: Vec<u32> = vec![0, 0]; // glyph 0 is an empty `loca` range.
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
        hmtx.extend_from_slice(&600u16.to_be_bytes()); // advance
        hmtx.extend_from_slice(&0i16.to_be_bytes()); // left side bearing
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

/// The four glyphs, glyph 1 upward. Far enough apart that no tolerance could
/// confuse a 600 with a 900.
const SIZES: [i16; 4] = [300, 900, 600, 450];

// ---------------------------------------------------------------------------
// The CMaps.
// ---------------------------------------------------------------------------

/// The parent: the codespace the child does not declare, a **decoy** CID range
/// over the codes the child also claims, and one `cidchar` outside them.
///
/// The decoy is what makes merge order measurable. Code `<8140>` is CID 2 here
/// and CID 3 in the child, so the page draws a 900-unit box if the parent won
/// and a 600-unit box if the child did. Both are real glyphs in this face, so
/// neither answer can arrive by the fixture failing to work.
///
/// The codespace is Shift-JIS-shaped on purpose: two bytes, starting at 0x81,
/// so that a byte-at-a-time split produces codes — 0x81 and 0x40 — that name
/// no glyph at all. A test whose codes were below 0x100 could not tell a
/// two-byte read from a one-byte one, because both would compute the same
/// number.
const PARENT_CMAP: &[u8] = b"/CIDInit /ProcSet findresource begin\n\
     begincmap\n\
     /CMapName /FixtureParent def\n\
     1 begincodespacerange <8140> <9FFC> endcodespacerange\n\
     1 begincidrange <8140> <817E> 2 endcidrange\n\
     1 begincidchar <9F41> 4 endcidchar\n\
     endcmap end";

/// The child: one `cidrange` and nothing else. No codespace, which is the
/// whole defect — without the parent's it splits every string one byte at a
/// time.
const CHILD_CMAP: &[u8] = b"/CIDInit /ProcSet findresource begin\n\
     begincmap\n\
     1 begincidrange <8140> <817E> 3 endcidrange\n\
     endcmap end";

// ---------------------------------------------------------------------------
// Building the document.
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

/// A stream object, with `prefix` written into its dictionary verbatim.
fn stream(prefix: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {prefix} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A page drawing `codes` at 48 point through a Type 0 font whose `/Encoding`
/// is the CMap streams given: object 9 is the child, and objects past it are
/// whatever it references.
fn page(cmaps: &[Vec<u8>], codes: &[u16]) -> Vec<u8> {
    let mut objects = vec![
        b"<< /Type /Font /Subtype /Type0 /BaseFont /Fixture /Encoding 9 0 R\n\
          /DescendantFonts [8 0 R] >>"
            .to_vec(),
        b"<< /Type /FontDescriptor /FontName /Fixture /Flags 4 /FontFile2 7 0 R >>".to_vec(),
        stream("", &boxes_font()),
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /Fixture /DW 700\n\
          /CIDSystemInfo << /Registry (Adobe) /Ordering (Japan1) /Supplement 2 >>\n\
          /CIDToGIDMap /Identity /FontDescriptor 6 0 R >>"
            .to_vec(),
    ];
    objects.extend_from_slice(cmaps);
    let hex: String = codes.iter().map(|c| format!("{c:04X}")).collect();
    document(&objects, &format!("BT /F0 48 Tf 20 30 Td <{hex}> Tj ET"))
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

/// Asserts the page drew one box of `size` font units, and nothing else.
#[track_caller]
fn drew_box(bitmap: &Bitmap, size: f64, what: &str) {
    let expected = size / 1000.0 * 48.0;
    let (w, h) = ink_box(bitmap);
    assert!(
        (w - expected).abs() <= 2.0 && (h - expected).abs() <= 2.0,
        "{what}: drew a {w} by {h} pixel shape, not the {expected} square the \
         {size}-unit glyph makes. Warnings: {:?}",
        bitmap.warnings,
    );
}

// ---------------------------------------------------------------------------
// The tests.
// ---------------------------------------------------------------------------

/// The defect, stated as an assertion, so that the tests below are measuring
/// a change rather than a constant.
///
/// The same child CMap with no `/UseCMap` has no codespace of its own, so
/// `<8140>` splits into 0x81 and 0x40. Neither is inside the child's range, so
/// each is its own CID (9.7.4), and CIDs 129 and 64 name no glyph in a
/// four-glyph face: the page comes out blank.
///
/// Blank and *silent*, which is why this is worth a test rather than a
/// sentence. `/CIDToGIDMap /Identity` has no end — the CID is the glyph index
/// by definition — so nothing at that layer can say a CID ran off the top of
/// the face, and the only sign is a page that drew nothing. The stream form
/// does report, which is gap 02's `a_cid_past_the_end_of_the_map_draws_notdef
/// _and_says_so`.
#[test]
fn without_its_parent_the_child_splits_the_string_into_fragments() {
    let bitmap = render(page(&[stream("/Type /CMap", CHILD_CMAP)], &[0x8140]));
    assert_eq!(ink(&bitmap), 0, "nothing was drawn");
    assert!(
        !bitmap.warnings.contains(&RenderWarning::UnreadableFont),
        "and nothing was said about it: {:?}",
        bitmap.warnings
    );
}

/// Milestone 1's exit criterion at the rendering level: a child declaring only
/// one `cidrange` inherits the parent's codespaces, so the same two bytes are
/// now one code and select one glyph.
#[test]
fn a_child_inherits_its_parent_s_codespaces_and_draws_one_glyph() {
    let bitmap = render(page(
        &[
            stream("/Type /CMap /UseCMap 10 0 R", CHILD_CMAP),
            stream("/Type /CMap", PARENT_CMAP),
        ],
        &[0x8140],
    ));
    drew_box(&bitmap, 600.0, "one two-byte code, CID 3");
    assert!(!bitmap.warnings.contains(&RenderWarning::UnreadableFont));
}

/// The mitigation this gap's risk table asks for, in pixels: the parent and
/// the child both map `<8140>`, and they disagree. The child says CID 3, which
/// is the 600-unit box; the parent says CID 2, which is the 900-unit box.
///
/// A merge that let the parent win would draw a real glyph at a plausible
/// size, which is exactly the failure that would otherwise go unnoticed — so
/// the parent's answer is asserted separately first, to prove the decoy is
/// live and not an empty range.
#[test]
fn the_child_s_range_wins_over_the_parent_s_on_the_page() {
    let parent_alone = render(page(&[stream("/Type /CMap", PARENT_CMAP)], &[0x8140]));
    drew_box(&parent_alone, 900.0, "the parent alone, which is the decoy");

    let inherited = render(page(
        &[
            stream("/Type /CMap /UseCMap 10 0 R", CHILD_CMAP),
            stream("/Type /CMap", PARENT_CMAP),
        ],
        &[0x8140],
    ));
    drew_box(&inherited, 600.0, "the child's CID, not the parent's");
}

/// And the other half of "a merge, not a replacement": a code the child never
/// mentions still resolves through the parent.
#[test]
fn a_code_the_child_does_not_map_falls_through_to_the_parent() {
    let bitmap = render(page(
        &[
            stream("/Type /CMap /UseCMap 10 0 R", CHILD_CMAP),
            stream("/Type /CMap", PARENT_CMAP),
        ],
        &[0x9F41],
    ));
    drew_box(&bitmap, 450.0, "the parent's cidchar, CID 4");
}

/// The body spelling of the same statement, over the one parent both
/// spellings can reach. `Identity-H` supplies a two-byte codespace and
/// code-is-CID; the child overrides `<0004>` onto CID 2, the 900-unit box,
/// which is what says the merge ran rather than the parent replacing the
/// child outright.
///
/// The codespace half is measured with *two* codes rather than one, because
/// with one it is not measurable at all: a code below 0x100 has a zero high
/// byte, and the glyph a zero byte selects is `.notdef`, which draws nothing.
/// One code would therefore paint the same box whether the string was read as
/// one two-byte code or as two one-byte ones — a test that passes on both
/// sides of the change. Two codes separate them by advance: inherited, two
/// boxes one 700-unit advance apart; not inherited, four glyphs of which two
/// are blank, spanning three advances.
#[test]
fn a_body_usecmap_inherits_from_a_predefined_parent() {
    let cmap = b"/Identity-H usecmap\n1 begincidchar <0004> 2 endcidchar";

    let inherited = render(page(&[stream("/Type /CMap", cmap)], &[0x0003, 0x0003]));
    let (w, h) = ink_box(&inherited);
    let expected = (700.0 + 600.0) / 1000.0 * 48.0;
    assert!(
        (w - expected).abs() <= 2.0,
        "two 600-unit boxes one 700-unit advance apart span {expected} pixels, \
         not {w} — which is what four codes would give"
    );
    assert!(
        (h - 600.0 / 1000.0 * 48.0).abs() <= 2.0,
        "one box tall, not {h}"
    );

    let overridden = render(page(&[stream("/Type /CMap", cmap)], &[0x0004]));
    drew_box(&overridden, 900.0, "the child's own entry, CID 2");
}

/// Ruling 1 at the rendering level: a `/UseCMap` chain that closes on itself
/// must render rather than hang, and it must keep the child's own work.
#[test]
fn a_self_referential_use_cmap_still_renders() {
    let cmap = b"/CIDInit /ProcSet findresource begin\n\
                 1 begincodespacerange <8140> <9FFC> endcodespacerange\n\
                 1 begincidrange <8140> <817E> 3 endcidrange\n\
                 end";
    let bitmap = render(page(
        &[stream("/Type /CMap /UseCMap 9 0 R", cmap)],
        &[0x8140],
    ));
    drew_box(&bitmap, 600.0, "a CMap that uses itself");
}
