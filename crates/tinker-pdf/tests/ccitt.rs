//! A fax reaches the page (7.4.6; ITU-T T.4, T.6).
//!
//! Nothing in this repository decoded a CCITT stream end to end. The filter
//! crate had unit tests for its run tables and its EOL handling, and
//! `resources.rs` had a branch that turned the result into pixels, and no test
//! anywhere ran the two together. That matters more here than for most
//! decoders, because the contract between them is a *shape* — how many bytes
//! per pixel, and which way round black and white are — and a change to either
//! half that keeps its own tests green renders every scanned document in the
//! world as a negative at one eighth width.
//!
//! So these assert positions and colours: this pixel is black, that one is
//! white. A fax that came out inverted, mirrored, or eight times too wide is a
//! perfectly plausible image, and only a named pixel tells the difference.

use tinker_pdf::{Document, RenderOptions};

fn render(bytes: Vec<u8>) -> tinker_pdf::Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

fn pixel(bitmap: &tinker_pdf::Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let at = (y as usize) * bitmap.stride + (x as usize) * bitmap.components();
    let p = bitmap.data.get(at..at + 3).unwrap_or(&[0, 0, 0]);
    (p[0], p[1], p[2])
}

fn is_black(p: (u8, u8, u8)) -> bool {
    p.0 < 60 && p.1 < 60 && p.2 < 60
}

fn is_white(p: (u8, u8, u8)) -> bool {
    p.0 > 200 && p.1 > 200 && p.2 > 200
}

/// Packs a pattern of `0` and `1` into bytes, most significant bit first.
///
/// Anything that is not a bit is ignored, so the codes below can be spaced
/// the way T.4's tables print them. A partial final byte is zero-filled, which
/// is what an encoder's own padding looks like.
fn bits(pattern: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut byte = 0u8;
    let mut count = 0u32;
    for c in pattern.chars().filter(|c| *c == '0' || *c == '1') {
        byte = (byte << 1) | u8::from(c == '1');
        count += 1;
        if count % 8 == 0 {
            out.push(byte);
            byte = 0;
        }
    }
    if count % 8 != 0 {
        out.push(byte << (8 - count % 8));
    }
    out
}

/// A one-page document whose only content is one fax image filling it.
///
/// `extra` goes into the image dictionary, so a caller can add `/ImageMask`,
/// a `/Decode` array or a different colour space to the same coded bytes.
fn fax_page(coded: &[u8], parms: &str, extra: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    out.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
          /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
    );
    out.extend_from_slice(b"4 0 obj\n<< /Length 27 >>\nstream\n");
    out.extend_from_slice(b"q 20 0 0 20 0 0 cm /Im0 Do Q");
    out.extend_from_slice(b"\nendstream\nendobj\n");
    out.extend_from_slice(
        format!(
            "5 0 obj\n<< /Type /XObject /Subtype /Image /Width 8 /Height 4\n\
             {extra}\n/Filter /CCITTFaxDecode /DecodeParms << {parms} >>\n\
             /Length {} >>\nstream\n",
            coded.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(coded);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    out.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");
    out
}

/// The same page and the same coded bytes, as an inline image (8.9.7).
///
/// Table 93's short spellings throughout, because that is what a producer
/// writes and because the abbreviation rewrite is part of what is under test.
fn inline_fax_page(coded: &[u8], parms: &str, extra: &str) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend_from_slice(b"q 20 0 0 20 0 0 cm BI /W 8 /H 4 ");
    content.extend_from_slice(extra.as_bytes());
    content.extend_from_slice(format!(" /F /CCF /DP << {parms} >> ID ").as_bytes());
    content.extend_from_slice(coded);
    content.extend_from_slice(b" EI Q");

    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    out.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
          /Resources << >> /Contents 4 0 R >>\nendobj\n",
    );
    out.extend_from_slice(format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes());
    out.extend_from_slice(&content);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    out.extend_from_slice(b"trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n");
    out
}

/// An 8x4 checkerboard, two-dimensionally coded (T.6).
///
/// Rows 0 and 1 are four black then four white; rows 2 and 3 are the reverse.
/// The asymmetry is the point: a negative, a horizontal mirror and a vertical
/// mirror each produce a different one of the four quadrants, so no single
/// mistake leaves the assertions below satisfied.
///
/// Row 0 has no real reference line, so it is coded horizontally (`001`, T.6
/// 2.2.2) as a white run of zero and a black run of four. Its last changing
/// element sits at the row's end and is coded V(0). Row 1 repeats row 0
/// exactly, so all three of its changing elements are V(0) — the mode that
/// makes G4 small. Rows 2 and 3 are the same pair one colour over.
fn checkerboard_g4() -> Vec<u8> {
    bits(concat!(
        // Row 0: horizontal, white 0 (00110101), black 4 (011), then V(0).
        "001 00110101 011 1 ",
        // Row 1: V(0) three times.
        "111 ",
        // Row 2: horizontal, white 4 (1011), black 4 (011).
        "001 1011 011 ",
        // Row 3: V(0) twice.
        "11 ",
        // EOFB (T.6 2.2.3): two EOLs.
        "000000000001 000000000001",
    ))
}

/// The whole point of the file: a fax decodes to the pixels its codes name,
/// in the places they name, the right way round.
#[test]
fn a_group_4_fax_renders_to_known_pixels() {
    let bitmap = render(fax_page(
        &checkerboard_g4(),
        "/K -1 /Columns 8 /Rows 4",
        "/ColorSpace /DeviceGray /BitsPerComponent 1",
    ));

    let top_left = pixel(&bitmap, 4, 4);
    let top_right = pixel(&bitmap, 15, 4);
    let bottom_left = pixel(&bitmap, 4, 15);
    let bottom_right = pixel(&bitmap, 15, 15);

    assert!(
        is_black(top_left),
        "rows 0-1 start black, got {top_left:?} — white here is a negative"
    );
    assert!(
        is_white(top_right),
        "and end white, got {top_right:?} — black here is a mirror"
    );
    assert!(
        is_white(bottom_left),
        "rows 2-3 start white, got {bottom_left:?} — black here is a vertical flip"
    );
    assert!(
        is_black(bottom_right),
        "and end black, got {bottom_right:?}"
    );
    assert!(
        bitmap.warnings.is_empty(),
        "a well-formed fax needs no excuses: {:?}",
        bitmap.warnings
    );
}

/// 8.9.7 permits a fax inline, and it decodes there to the same four
/// quadrants — through the same `ccitt_samples`, not a second call site.
///
/// It used to hit the inline path's catch-all, report `CCITTFaxDecode` as a
/// codec this build does not have, and paint the grey placeholder over it.
///
/// The quadrants are asserted by position for the reason the file's header
/// gives, and doubly so here: gap 16 changed what the decoder returns from one
/// byte per pixel to packed bits without changing its signature, so a second
/// caller written against the old contract would compile, run, and draw a
/// negative at one eighth width. That fills the left-hand eighth of this
/// image, which is where `top_left` and `bottom_left` are read — and it is the
/// *right*-hand pair that catches it.
#[test]
fn an_inline_group_4_fax_renders_to_the_same_pixels_as_the_xobject() {
    let coded = checkerboard_g4();
    let parms = "/K -1 /Columns 8 /Rows 4";
    let extra = "/CS /G /BPC 1";

    let inline = render(inline_fax_page(&coded, parms, extra));
    let xobject = render(fax_page(
        &coded,
        parms,
        "/ColorSpace /DeviceGray /BitsPerComponent 1",
    ));

    for (bitmap, what) in [(&xobject, "as an XObject"), (&inline, "inline")] {
        let top_left = pixel(bitmap, 4, 4);
        let top_right = pixel(bitmap, 15, 4);
        let bottom_left = pixel(bitmap, 4, 15);
        let bottom_right = pixel(bitmap, 15, 15);
        assert!(
            is_black(top_left),
            "{what}: rows 0-1 start black: {top_left:?}"
        );
        assert!(
            is_white(top_right),
            "{what}: and end white: {top_right:?} — black at one eighth width \
             is a caller reading the old byte-per-pixel contract"
        );
        assert!(
            is_white(bottom_left),
            "{what}: rows 2-3 start white: {bottom_left:?}"
        );
        assert!(
            is_black(bottom_right),
            "{what}: and end black: {bottom_right:?}"
        );
    }

    assert_eq!(
        inline.data, xobject.data,
        "the same coded bytes must produce the same page either way round"
    );
    assert!(
        inline.warnings.is_empty(),
        "a well-formed inline fax reports no unsupported codec and needs no \
         other excuse: {:?}",
        inline.warnings
    );
}

/// What the decoder forgives is named, wherever the fax was written.
///
/// Ruling 10. The XObject path collects the decoder's warnings per image and
/// reports them through `damaged_images`; an inline fax reaches the same
/// function, so it reports the same way rather than drawing a half-decoded
/// page in silence.
#[test]
fn a_damaged_inline_fax_row_is_reported_the_way_an_xobject_one_is() {
    let coded = bits(concat!("001 00110101 011 1 ", "111 ", "00000001"));
    let parms = "/K -1 /Columns 8 /Rows 4";

    let inline = render(inline_fax_page(&coded, parms, "/CS /G /BPC 1"));
    let xobject = render(fax_page(
        &coded,
        parms,
        "/ColorSpace /DeviceGray /BitsPerComponent 1",
    ));

    let damaged = |bitmap: &tinker_pdf::Bitmap| -> Vec<String> {
        bitmap
            .warnings
            .iter()
            .filter_map(|w| match w {
                tinker_pdf::RenderWarning::DamagedImage { reason, .. } => Some(reason.clone()),
                _ => None,
            })
            .collect()
    };

    assert!(
        !damaged(&inline).is_empty(),
        "the inline fax says what it forgave: {:?}",
        inline.warnings
    );
    assert_eq!(
        damaged(&inline),
        damaged(&xobject),
        "and says the same thing the XObject one does"
    );
    assert!(
        is_black(pixel(&inline, 4, 12)),
        "the damaged row still carries the row above it: {:?}",
        pixel(&inline, 4, 12)
    );
}

/// A row that will not decode is one row, not the rest of the page.
///
/// Fax practice, and what plan 02 specifies: repeat the row above, warn,
/// stop. The behaviour predates this file and has to survive the change to a
/// packed representation, so it is pinned by position rather than by the
/// warning alone — the recovered row must carry the *previous* row's pixels,
/// not white, and the rows after it must be white rather than more copies.
#[test]
fn a_damaged_fax_row_repeats_the_row_above() {
    let coded = bits(concat!(
        // Rows 0 and 1: four black, four white, exactly as above.
        "001 00110101 011 1 ",
        "111 ",
        // Row 2 opens with a code no table holds and that is too short to be
        // an EOL: seven zeros and a one.
        "00000001",
    ));

    let bitmap = render(fax_page(
        &coded,
        "/K -1 /Columns 8 /Rows 4",
        "/ColorSpace /DeviceGray /BitsPerComponent 1",
    ));

    // Rows 0 and 1 occupy the top half, row 2 the third quarter, row 3 the
    // last.
    assert!(
        is_black(pixel(&bitmap, 4, 4)),
        "the rows that did decode are unaffected"
    );
    assert!(
        is_black(pixel(&bitmap, 4, 12)),
        "the damaged row carries the row above it, not white: {:?}",
        pixel(&bitmap, 4, 12)
    );
    assert!(
        is_white(pixel(&bitmap, 15, 12)),
        "including its white half — a solid black row is not a repeat: {:?}",
        pixel(&bitmap, 15, 12)
    );
    assert!(
        is_white(pixel(&bitmap, 4, 17)),
        "and the rows after it are white rather than further copies: {:?}",
        pixel(&bitmap, 4, 17)
    );
}
