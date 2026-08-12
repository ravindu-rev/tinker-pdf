//! A CFF font draws the glyphs the document names (gap 01, plan 05).
//!
//! The character code is not a glyph index. Until August 2026 this engine
//! used it as one for every CFF program it met — bare `/FontFile3 /Subtype
//! /Type1C`, the `CFF ` table of an OpenType face, and the CID-keyed programs
//! composite fonts embed — and what that looked like depended entirely on how
//! many glyphs the font had:
//!
//! - a **subset** font, which is what a producer embeds, has a few dozen
//!   glyphs, so code 65 selected a glyph that did not exist and the page came
//!   out blank where the text should be;
//! - a **full** font has hundreds, so code 65 selected a real glyph and the
//!   page drew the wrong letter at the right width, silently.
//!
//! Both fixtures are below, and both now draw the letter that was asked for.
//! Every byte of every font here is built in this file: `testdata/` is four
//! self-authored PDFs and none of them embeds a CFF.

use tinker_pdf::{Bitmap, Document, RenderOptions, RenderWarning};

// ---------------------------------------------------------------------------
// Building a CFF font program.
// ---------------------------------------------------------------------------

/// Wraps `items` as a CFF INDEX with four-byte offsets.
fn index(items: &[Vec<u8>]) -> Vec<u8> {
    if items.is_empty() {
        return vec![0, 0];
    }
    let mut out = (items.len() as u16).to_be_bytes().to_vec();
    out.push(4);
    let mut offset = 1u32;
    out.extend_from_slice(&offset.to_be_bytes());
    for item in items {
        offset += item.len() as u32;
        out.extend_from_slice(&offset.to_be_bytes());
    }
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

/// One DICT entry: operands in the fixed five-byte integer form, then the
/// operator. Fixed width is what lets the fixture compute its own offsets.
fn entry(op: u16, operands: &[i32]) -> Vec<u8> {
    let mut out = Vec::new();
    for value in operands {
        out.push(29);
        out.extend_from_slice(&value.to_be_bytes());
    }
    if op > 0xFF {
        out.push(12);
        out.push((op & 0xFF) as u8);
    } else {
        out.push(op as u8);
    }
    out
}

/// A charstring drawing a box `size` units on a side, from the origin.
fn box_glyph(size: i16) -> Vec<u8> {
    let n = |v: i16| {
        let mut out = vec![28u8];
        out.extend_from_slice(&v.to_be_bytes());
        out
    };
    let mut out = Vec::new();
    out.extend(n(0));
    out.extend(n(0));
    out.push(21); // rmoveto
    for (dx, dy) in [(size, 0), (0, size), (-size, 0)] {
        out.extend(n(dx));
        out.extend(n(dy));
        out.push(5); // rlineto
    }
    out.push(14); // endchar
    out
}

/// A CFF font program built from its parts.
#[derive(Default)]
struct Program {
    /// Strings past the standard 391, numbered from SID 391 upward.
    strings: Vec<Vec<u8>>,
    /// The SID — or, when `cid` is set, the CID — of each glyph from 1 up.
    charset: Vec<u16>,
    /// Each glyph's charstring, `.notdef` first.
    charstrings: Vec<Vec<u8>>,
    /// The font's own encoding table, when it carries one.
    encoding: Option<Vec<u8>>,
    /// `ROS`, an FDArray and an FDSelect: a CID-keyed program.
    cid: bool,
}

impl Program {
    fn top_dict(&self, at: &[i32; 6]) -> Vec<u8> {
        let mut out = Vec::new();
        if self.cid {
            out.extend(entry(0x0C1E, &[391, 392, 0]));
        }
        out.extend(entry(15, &[at[0]]));
        if !self.cid {
            out.extend(entry(16, &[at[1]]));
        }
        out.extend(entry(17, &[at[2]]));
        out.extend(entry(18, &[at[3], at[4]]));
        if self.cid {
            out.extend(entry(0x0C24, &[at[5]]));
            // FDSelect sits immediately after the charset, and every glyph is
            // in Font DICT 0: format 3, one range, then the sentinel.
            out.extend(entry(0x0C25, &[at[0] + self.charset_bytes().len() as i32]));
        }
        out
    }

    /// The charset in format 0, which lists a SID per glyph after `.notdef`.
    fn charset_bytes(&self) -> Vec<u8> {
        let mut out = vec![0u8];
        for sid in &self.charset {
            out.extend_from_slice(&sid.to_be_bytes());
        }
        out
    }

    fn build(&self) -> Vec<u8> {
        // defaultWidthX and nominalWidthX, both 600.
        let private = {
            let mut out = entry(20, &[600]);
            out.extend(entry(21, &[600]));
            out
        };

        let header = [1u8, 0, 4, 4];
        let names = index(&[b"Fixture".to_vec()]);
        let strings = index(&self.strings);
        let gsubrs = index(&[]);
        let charstrings = index(&self.charstrings);
        let charset = self.charset_bytes();
        let encoding = self.encoding.clone().unwrap_or_default();
        // One range covering every glyph, and the sentinel that ends it.
        let mut fd_select = vec![3u8, 0, 1, 0, 0, 0];
        fd_select.extend_from_slice(&(self.charstrings.len() as u16).to_be_bytes());
        let fd_select = if self.cid { fd_select } else { Vec::new() };

        // The Top DICT's length cannot change when the offsets in it do, so
        // one pass with zeroes measures it and the second fills it in.
        let top_len = self.top_dict(&[0; 6]).len();
        let mut cursor =
            header.len() + names.len() + (2 + 1 + 8 + top_len) + strings.len() + gsubrs.len();

        let charset_at = cursor;
        cursor += charset.len() + fd_select.len();
        let encoding_at = cursor;
        cursor += encoding.len();
        let charstrings_at = cursor;
        cursor += charstrings.len();
        let private_at = cursor;
        cursor += private.len();
        let fd_private_at = cursor;
        if self.cid {
            cursor += private.len();
        }
        let fd_array_at = cursor;

        let at = [
            charset_at as i32,
            if self.encoding.is_some() {
                encoding_at as i32
            } else {
                0
            },
            charstrings_at as i32,
            private.len() as i32,
            private_at as i32,
            fd_array_at as i32,
        ];

        let mut out = header.to_vec();
        out.extend_from_slice(&names);
        out.extend_from_slice(&index(&[self.top_dict(&at)]));
        out.extend_from_slice(&strings);
        out.extend_from_slice(&gsubrs);
        assert_eq!(out.len(), charset_at, "the charset lands where it was put");
        out.extend_from_slice(&charset);
        out.extend_from_slice(&fd_select);
        assert_eq!(out.len(), encoding_at);
        out.extend_from_slice(&encoding);
        assert_eq!(out.len(), charstrings_at);
        out.extend_from_slice(&charstrings);
        assert_eq!(out.len(), private_at);
        out.extend_from_slice(&private);
        if self.cid {
            assert_eq!(out.len(), fd_private_at);
            out.extend_from_slice(&private);
            assert_eq!(out.len(), fd_array_at);
            let font_dict = entry(18, &[private.len() as i32, fd_private_at as i32]);
            out.extend_from_slice(&index(&[font_dict]));
        }
        out
    }
}

/// The three-letter face every simple-font fixture here uses: `A` as a box
/// 600 units on a side, `B` as one of 300, and `C` as one of 450.
///
/// Different sizes rather than different shapes because the size is what the
/// rendered ink can be measured against — the box a glyph draws says which
/// glyph ran, which is the whole question.
fn abc(encoding: Option<Vec<u8>>) -> Vec<u8> {
    Program {
        // SIDs 34, 35 and 36 are `A`, `B` and `C` among the standard strings.
        charset: vec![34, 35, 36],
        charstrings: vec![vec![14], box_glyph(600), box_glyph(300), box_glyph(450)],
        encoding,
        ..Program::default()
    }
    .build()
}

/// The em fraction a box of `size` font units covers, in pixels at 48 point.
fn expected_box(size: f64) -> f64 {
    size / 1000.0 * 48.0
}

// ---------------------------------------------------------------------------
// Building the document around it.
// ---------------------------------------------------------------------------

/// Assembles a one-page PDF from numbered objects.
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

/// A stream object holding a font program.
fn font_file(subtype: &str, program: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "<< /Subtype /{subtype} /Length {} >>\nstream\n",
        program.len()
    )
    .into_bytes();
    out.extend_from_slice(program);
    out.extend_from_slice(b"\nendstream");
    out
}

/// A simple font dictionary with an embedded program, drawing `text` at 48
/// point. `encoding` is written into the dictionary verbatim, or omitted.
fn simple_document(program: &[u8], subtype: &str, encoding: Option<&str>, text: &str) -> Vec<u8> {
    let encoding = encoding.map_or(String::new(), |e| format!("/Encoding {e}\n"));
    document(
        &[
            format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /Fixture /FirstChar 65\n\
                 /LastChar 67 /Widths [600 600 600] {encoding}/FontDescriptor 6 0 R >>"
            )
            .into_bytes(),
            b"<< /Type /FontDescriptor /FontName /Fixture /Flags 32 /FontFile3 7 0 R >>".to_vec(),
            font_file(subtype, program),
        ],
        &format!("BT /F0 48 Tf 20 30 Td ({text}) Tj ET"),
    )
}

/// A composite font over a CID-keyed program, drawing one two-byte code.
///
/// `encoding` is written after `/Encoding` verbatim, so it may be a predefined
/// name or a reference to one of the `extra` objects, which are numbered from
/// 9 upward.
fn composite_document(program: &[u8], encoding: &str, extra: &[Vec<u8>], code: u16) -> Vec<u8> {
    let mut objects = vec![
        format!(
            "<< /Type /Font /Subtype /Type0 /BaseFont /Fixture /Encoding {encoding}\n\
             /DescendantFonts [8 0 R] >>"
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /Fixture /Flags 4 /FontFile3 7 0 R >>".to_vec(),
        font_file("CIDFontType0C", program),
        b"<< /Type /Font /Subtype /CIDFontType0 /BaseFont /Fixture /DW 600\n\
          /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >>\n\
          /FontDescriptor 6 0 R >>"
            .to_vec(),
    ];
    objects.extend_from_slice(extra);
    document(
        &objects,
        &format!("BT /F0 48 Tf 20 30 Td <{code:04X}> Tj ET"),
    )
}

/// An embedded CMap stream mapping one two-byte code to one CID (9.7.5.3).
fn cid_cmap(code: u16, cid: u16) -> Vec<u8> {
    let text = format!(
        "/CIDInit /ProcSet findresource begin\n\
         1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
         1 begincidchar <{code:04X}> {cid} endcidchar\n\
         end"
    );
    let mut out = format!("<< /Type /CMap /Length {} >>\nstream\n", text.len()).into_bytes();
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(b"\nendstream");
    out
}

/// Wraps a CFF program in an `OTTO` sfnt, which is how OpenType carries one.
fn otto(program: &[u8]) -> Vec<u8> {
    let tables: [(&[u8; 4], &[u8]); 2] = [(b"CFF ", program), (b"head", &[0u8; 54])];
    let mut out = Vec::new();
    out.extend_from_slice(b"OTTO");
    out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
    out.extend_from_slice(&[0; 6]); // search hints, unread

    let mut offset = 12 + tables.len() * 16;
    let mut body = Vec::new();
    for (tag, data) in tables {
        out.extend_from_slice(tag);
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
///
/// A count of inked pixels says a glyph was drawn; its bounding box says
/// *which* glyph, because every glyph in these fixtures is a box of its own
/// size. That is what separates "the text appeared" from "the right text
/// appeared", and the old behaviour would have passed the first.
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
    let expected = expected_box(size);
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

/// Milestone 2's exit criterion: a `/FontFile3 /Subtype /Type1C` fixture
/// renders `A` as `A`.
#[test]
fn a_type1c_font_draws_the_letter_the_document_named() {
    for (text, size) in [("A", 600.0), ("B", 300.0), ("C", 450.0)] {
        let page = render(simple_document(
            &abc(None),
            "Type1C",
            Some("/WinAnsiEncoding"),
            text,
        ));
        drew_box(&page, size, text);
        assert!(
            !page.warnings.contains(&RenderWarning::UnreadableFont),
            "{text} resolved, so nothing is reported"
        );
    }
}

/// The shape the defect actually took, which is not the shape the gap
/// document described. A subset font — the only kind a producer embeds —
/// has too few glyphs for the code to land on one at all, so the page came
/// out blank rather than wrong.
#[test]
fn a_subset_font_used_to_have_no_glyph_at_the_code_at_all() {
    let font = abc(None);
    let cff = tinker_pdf_font::Cff::parse(&font).expect("the fixture parses");
    assert_eq!(cff.glyph_count(), 4);
    assert!(
        cff.outline(65).is_none(),
        "there is no glyph 65: using the code as an index found nothing, \
         which is why the page was blank rather than wrong"
    );
    assert_eq!(cff.gid_for_name("A"), Some(1), "and the name finds glyph 1");

    let page = render(simple_document(
        &font,
        "Type1C",
        Some("/WinAnsiEncoding"),
        "ABC",
    ));
    assert!(ink(&page) > 0, "all three letters now draw");
}

/// The other shape it took. A font with more glyphs than the code has a
/// glyph at that index, so the page drew a real glyph — the wrong one, at the
/// right width, with nothing reported.
#[test]
fn a_full_font_used_to_draw_whichever_glyph_the_code_numbered() {
    // Seventy glyphs: `A` is glyph 1, and glyph 65 is a box of its own.
    let mut charset = vec![34u16];
    let mut charstrings = vec![vec![14u8], box_glyph(600)];
    let mut strings = Vec::new();
    for gid in 2..70u16 {
        strings.push(format!("filler{gid}").into_bytes());
        charset.push(391 + gid - 2);
        charstrings.push(box_glyph(if gid == 65 { 200 } else { 500 }));
    }
    let font = Program {
        strings,
        charset,
        charstrings,
        ..Program::default()
    }
    .build();

    let cff = tinker_pdf_font::Cff::parse(&font).expect("the fixture parses");
    assert_eq!(cff.glyph_count(), 70);
    assert_eq!(cff.gid_for_name("A"), Some(1));
    assert_eq!(
        cff.glyph_name(65),
        Some("filler65"),
        "glyph 65 is a glyph, and it is not `A` — this is the substitution \
         the gap document described, and it needed a font this size to happen"
    );

    let page = render(simple_document(
        &font,
        "Type1C",
        Some("/WinAnsiEncoding"),
        "A",
    ));
    drew_box(&page, 600.0, "A in a full font");
}

/// Milestone 2's other exit criterion: a fixture with no `/Encoding` resolves
/// through the font's own. The built-in encoding here disagrees with the
/// standard one on purpose — code 65 selects glyph 2 — so a reader that fell
/// through to the standard encoding would draw the 600-unit box instead.
#[test]
fn without_a_document_encoding_the_font_s_own_selects_the_glyph() {
    // Encoding format 0: three codes, for glyphs 1, 2 and 3 in order.
    let encoding = vec![0, 3, 0x43, 0x41, 0x42];
    let page = render(simple_document(&abc(Some(encoding)), "Type1C", None, "A"));
    drew_box(&page, 300.0, "the font's own encoding");
}

/// 9.6.6: the document's encoding wins where it has one. The same program as
/// above, whose built-in encoding says code 65 is the 300-unit glyph, now
/// draws the 600-unit one because the dictionary names WinAnsi.
#[test]
fn the_document_encoding_wins_over_the_font_s_own() {
    let encoding = vec![0, 3, 0x43, 0x41, 0x42];
    let page = render(simple_document(
        &abc(Some(encoding)),
        "Type1C",
        Some("/WinAnsiEncoding"),
        "A",
    ));
    drew_box(&page, 600.0, "the document's encoding");
}

/// `/Differences` beats a named base encoding, and it is how a subset font
/// says what its codes mean.
#[test]
fn differences_name_the_glyph_a_code_selects() {
    let page = render(simple_document(
        &abc(None),
        "Type1C",
        Some("<< /BaseEncoding /WinAnsiEncoding /Differences [65 /C] >>"),
        "A",
    ));
    drew_box(&page, 450.0, "a /Differences name");
}

/// Step 4 of the fallback order: nothing named the glyph, so `.notdef` is
/// drawn and the page reports. Without the report this is indistinguishable
/// from a space, which is what makes it worse than drawing the wrong glyph.
///
/// The font carries `B`, `C` and `D` and the page asks for `A`, which is what
/// a subset font meeting a code its producer did not subset for looks like.
/// Every step of the chain is exercised and every step declines: the
/// dictionary's WinAnsi names `A`, the font's built-in Standard encoding
/// names `A`, and the standard encoding names `A`, and this font has no `A`.
#[test]
fn a_code_nothing_names_draws_notdef_and_says_so() {
    let font = Program {
        charset: vec![35, 36, 37],
        charstrings: vec![vec![14], box_glyph(600), box_glyph(300), box_glyph(450)],
        ..Program::default()
    }
    .build();

    let page = render(simple_document(
        &font,
        "Type1C",
        Some("/WinAnsiEncoding"),
        "A",
    ));
    assert_eq!(ink(&page), 0, "`.notdef` in this font draws nothing");
    assert!(
        page.warnings.contains(&RenderWarning::UnreadableFont),
        "and the page says a glyph could not be resolved: {:?}",
        page.warnings
    );

    // The same font drawing a letter it does have reports nothing, so the
    // warning above is about the glyph and not about the font.
    let page = render(simple_document(
        &font,
        "Type1C",
        Some("/WinAnsiEncoding"),
        "B",
    ));
    drew_box(&page, 600.0, "B, which this font does have");
    assert!(!page.warnings.contains(&RenderWarning::UnreadableFont));
}

/// The second of the three sites that used the code as an index: an OpenType
/// face whose outlines are in a `CFF ` table rather than in `glyf`.
#[test]
fn an_opentype_cff_face_selects_by_name_too() {
    let page = render(simple_document(
        &otto(&abc(None)),
        "OpenType",
        Some("/WinAnsiEncoding"),
        "C",
    ));
    drew_box(&page, 450.0, "an OTTO face");
}

/// A composite font over a CID-keyed program. The CIDs are deliberately not
/// the glyph indices: CID 11 is glyph 2, so a reader using the code as an
/// index would draw glyph 11, which does not exist.
#[test]
fn a_cid_keyed_program_reaches_its_glyph_through_the_charset() {
    let font = Program {
        strings: vec![b"Adobe".to_vec(), b"Identity".to_vec()],
        charset: vec![10, 11, 12],
        charstrings: vec![vec![14], box_glyph(600), box_glyph(300), box_glyph(450)],
        cid: true,
        ..Program::default()
    }
    .build();

    let cff = tinker_pdf_font::Cff::parse(&font).expect("the fixture parses");
    assert!(cff.is_cid());
    assert_eq!(cff.gid_for_cid(11), Some(2));
    assert!(cff.outline(11).is_none(), "there is no glyph 11");

    let page = render(composite_document(&font, "/Identity-H", &[], 11));
    drew_box(&page, 300.0, "CID 11");
    assert!(!page.warnings.contains(&RenderWarning::UnreadableFont));
}

/// The same program under a CMap that is *not* the identity, which is the
/// check gap 02 owed this half rather than assumed.
///
/// The test above uses `Identity-H`, where the code and the CID are the same
/// number, so it cannot distinguish a reader that resolves the CID from one
/// that hands the code straight to the charset. Here they are three distinct
/// numbers: code 0x41 is CID 11 is glyph 2, the 300-unit box. A reader
/// skipping the CMap would ask the charset for CID 0x41, which this font does
/// not have, and draw `.notdef`.
#[test]
fn a_cid_keyed_program_follows_a_non_identity_cmap() {
    let font = Program {
        strings: vec![b"Adobe".to_vec(), b"Identity".to_vec()],
        charset: vec![10, 11, 12],
        charstrings: vec![vec![14], box_glyph(600), box_glyph(300), box_glyph(450)],
        cid: true,
        ..Program::default()
    }
    .build();

    let cff = tinker_pdf_font::Cff::parse(&font).expect("the fixture parses");
    assert_eq!(cff.gid_for_cid(11), Some(2));
    assert_eq!(cff.gid_for_cid(0x41), None, "this font has no CID 65");

    let page = render(composite_document(
        &font,
        "9 0 R",
        &[cid_cmap(0x0041, 11)],
        0x0041,
    ));
    drew_box(&page, 300.0, "code 0x41 through CID 11");
    assert!(!page.warnings.contains(&RenderWarning::UnreadableFont));
}
