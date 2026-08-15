//! A JBIG2 scan reaches the page, or is refused by name (7.4.7; ITU-T T.88).
//!
//! The filter crate proves the *decoder* against T.88 Annex H.1's own
//! datastream. This file proves the two things that live on the other side of
//! the crate boundary and that no unit test can see.
//!
//! **Polarity.** T.88 6.2.2 codes 1 for black; a 1-bit DeviceGray sample is 0
//! for black. The decoder deliberately returns JBIG2's own sense and the
//! inversion happens once, at the boundary, where `/Decode`, `/ImageMask` and
//! `/ColorSpace` are also read. A build that inverted twice, or not at all,
//! renders every scanned archive in the world as its own negative and every
//! one of the filter crate's tests still passes. So these assert named
//! pixels: this one is black, that one is white.
//!
//! **The refusal.** A stream whose regions this build cannot decode — the
//! symbol dictionary plus text region an OCR pipeline emits, which is most
//! JBIG2 in circulation — must draw the neutral placeholder and say so. It
//! must **not** return the blank page it was composited onto, because that is
//! indistinguishable from a correct decode of a blank scan and is strictly
//! worse than the placeholder it replaced. That risk is at its highest right
//! now, because until this gap landed the decoder never returned a page at
//! all; a decoder that succeeds *sometimes* is one that can start succeeding
//! when it should not.

use tinker_pdf::{Document, RenderOptions, RenderWarning};

/// ITU-T T.88 Annex H.1's second page: the page information segment and the
/// arithmetically coded generic region that draws a 54 by 44 frame at (4, 11)
/// on a 64 by 56 page, with the four segments around them that this build
/// refuses.
///
/// Bytes 400 to 681 of the annex's datastream, in the embedded organisation of
/// Annex D.3 — which is what a PDF carries: no file header, no page count.
#[rustfmt::skip]
const ANNEX_H_PAGE_2: [u8; 282] = [
    0x00, 0x00, 0x00, 0x08, 0x30, 0x00, 0x02, 0x00, 0x00, 0x00, 0x13, 0x00,
    0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x38, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x00, 0x01,
    0x02, 0x00, 0x00, 0x00, 0x1B, 0x08, 0x00, 0x02, 0xFF, 0x00, 0x00, 0x00,
    0x02, 0x00, 0x00, 0x00, 0x02, 0x4F, 0xE7, 0x8C, 0x20, 0x0E, 0x1D, 0xC7,
    0xCF, 0x01, 0x11, 0xC4, 0xB2, 0x6F, 0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0A,
    0x07, 0x40, 0x00, 0x09, 0x02, 0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00,
    0x25, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x0C, 0x08, 0x00, 0x00, 0x00, 0x05, 0x8D, 0x6E, 0x5A, 0x12,
    0x40, 0x85, 0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0B, 0x27, 0x00, 0x02, 0x00,
    0x00, 0x00, 0x23, 0x00, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x2C, 0x00,
    0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x0B, 0x00, 0x08, 0x03, 0xFF, 0xFD,
    0xFF, 0x02, 0xFE, 0xFE, 0xFE, 0x04, 0xEE, 0xED, 0x87, 0xFB, 0xCB, 0x2B,
    0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0C, 0x10, 0x01, 0x02, 0x00, 0x00, 0x00,
    0x1C, 0x06, 0x04, 0x04, 0x00, 0x00, 0x00, 0x0F, 0x90, 0x71, 0x6B, 0x6D,
    0x99, 0xA7, 0xAA, 0x49, 0x7D, 0xF2, 0xE5, 0x48, 0x1F, 0xDC, 0x68, 0xBC,
    0x6E, 0x40, 0xBB, 0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0D, 0x17, 0x20, 0x0C,
    0x02, 0x00, 0x00, 0x00, 0x3E, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00,
    0x24, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x0F, 0x00, 0x02, 0x00,
    0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x87, 0xCB, 0x82, 0x1E, 0x66,
    0xA4, 0x14, 0xEB, 0x3C, 0x4A, 0x15, 0xFA, 0xCC, 0xD6, 0xF3, 0xB1, 0x6F,
    0x4C, 0xED, 0xBF, 0xA7, 0xBF, 0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0E, 0x31,
    0x00, 0x02, 0x00, 0x00, 0x00, 0x00,
];

/// A segment header (T.88 7.2), short form.
fn segment(number: u32, kind: u8, page: u8, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&number.to_be_bytes());
    out.push(kind & 0x3F);
    out.push(0);
    out.push(page);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(data);
    out
}

/// A page information segment's nineteen bytes (T.88 7.4.8).
fn page_information(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.push(0x01);
    out.extend_from_slice(&0u16.to_be_bytes());
    out
}

/// A one-page document whose only content is one JBIG2 image filling it.
///
/// `extra` goes into the image dictionary, so a caller can add `/ImageMask` or
/// a `/Decode` array to the same coded bytes. `globals` becomes a
/// `/JBIG2Globals` stream in `/DecodeParms` when it is not empty.
fn jbig2_page(coded: &[u8], width: u32, height: u32, extra: &str, globals: &[u8]) -> Vec<u8> {
    let parms = if globals.is_empty() {
        String::new()
    } else {
        "/DecodeParms << /JBIG2Globals 6 0 R >>".to_string()
    };

    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    out.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 64 56]\n\
          /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
    );
    out.extend_from_slice(b"4 0 obj\n<< /Length 27 >>\nstream\n");
    out.extend_from_slice(b"q 64 0 0 56 0 0 cm /Im0 Do Q");
    out.extend_from_slice(b"\nendstream\nendobj\n");
    out.extend_from_slice(
        format!(
            "5 0 obj\n<< /Type /XObject /Subtype /Image /Width {width} /Height {height}\n\
             {extra}\n/Filter /JBIG2Decode {parms}\n/Length {} >>\nstream\n",
            coded.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(coded);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    if !globals.is_empty() {
        out.extend_from_slice(
            format!("6 0 obj\n<< /Length {} >>\nstream\n", globals.len()).as_bytes(),
        );
        out.extend_from_slice(globals);
        out.extend_from_slice(b"\nendstream\nendobj\n");
    }
    out.extend_from_slice(b"trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n");
    out
}

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

/// The page is 64 by 56 at one unit per pixel and the image fills it, so an
/// image coordinate is a device coordinate.
///
/// Not a flip, deliberately: 8.9.5.2 maps an image's *first* row to the top of
/// the unit square, so a row index does not invert the way a PDF y coordinate
/// does. Getting that backwards was the first thing this file caught, and it
/// caught it because the frame is asymmetric about its own middle only at the
/// corners.
fn image_pixel(bitmap: &tinker_pdf::Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    pixel(bitmap, x, y)
}

/// **The polarity, proved where it matters.** A 1 in the JBIG2 datastream is
/// black ink on the rendered page.
///
/// Annex H.1's region is a frame two pixels thick, so the corners are 1 and
/// the middle is 0 — a picture that survives being inverted as a *picture*,
/// which is exactly why the assertions name coordinates rather than counting
/// ink. An inverted build draws a page that is black everywhere except a
/// rectangle, and that reads as a plausible scan.
#[test]
fn a_jbig2_one_is_black_ink_on_the_page() {
    let bitmap = render(jbig2_page(&ANNEX_H_PAGE_2, 64, 56, "", &[]));

    // The frame's own edges: (4, 11) is its top left corner, (57, 54) its
    // bottom right, and both rows and both columns are two pixels thick.
    for (x, y) in [(4, 11), (57, 11), (4, 54), (57, 54), (5, 12), (30, 12)] {
        assert!(
            is_black(image_pixel(&bitmap, x, y)),
            "({x}, {y}) is a 1 in the datastream and must be black ink; \
             got {:?}",
            image_pixel(&bitmap, x, y)
        );
    }

    // Inside the frame, and outside it. Both are 0 and both must be white.
    for (x, y) in [(30, 30), (10, 20), (2, 30), (30, 5), (62, 30)] {
        assert!(
            is_white(image_pixel(&bitmap, x, y)),
            "({x}, {y}) is a 0 in the datastream and must be white; got {:?}",
            image_pixel(&bitmap, x, y)
        );
    }
}

/// `/Decode [1 0]` inverts the samples, which is only meaningful because the
/// samples reach the generic path rather than being turned into pixels by the
/// codec.
///
/// This is the assertion that says the routing decision held. A JBIG2 image
/// that returned its own RGB would ignore `/Decode` entirely and pass every
/// other test in this file.
#[test]
fn decode_arrays_apply_to_a_jbig2_image_because_it_arrives_as_samples() {
    let bitmap = render(jbig2_page(&ANNEX_H_PAGE_2, 64, 56, "/Decode [1 0]", &[]));
    assert!(
        is_white(image_pixel(&bitmap, 4, 11)),
        "/Decode [1 0] must turn the frame white"
    );
    assert!(
        is_black(image_pixel(&bitmap, 30, 30)),
        "and the ground black"
    );
}

/// `/ImageMask true` paints the fill colour through the 0 samples (8.9.6.2),
/// which for a JBIG2 scan means the ink paints and the paper is transparent.
#[test]
fn an_image_mask_paints_a_jbig2_region_in_the_fill_colour() {
    let masked = render(jbig2_page(&ANNEX_H_PAGE_2, 64, 56, "/ImageMask true", &[]));
    assert!(
        is_black(image_pixel(&masked, 4, 11)),
        "the frame paints in the fill colour, which defaults to black"
    );
    assert!(
        is_white(image_pixel(&masked, 30, 30)),
        "and the paper does not paint at all"
    );
}

/// **The refusal, end to end, now that regions actually decode.**
///
/// A symbol dictionary and a text region — what `jbig2enc` and OCRmyPDF emit,
/// and the bulk of the JBIG2 gap 23 found in the corpora. No generic region,
/// so no region was composited, so the page is refused and the placeholder
/// stands.
#[test]
fn a_symbol_dictionary_file_still_draws_the_placeholder() {
    let mut coded = segment(0, 48, 1, &page_information(64, 56));
    coded.extend(segment(1, 0, 1, &[0u8; 8])); // symbol dictionary
    coded.extend(segment(2, 6, 1, &[0u8; 8])); // immediate text region

    let bitmap = render(jbig2_page(&coded, 64, 56, "", &[]));
    assert!(
        bitmap.warnings.contains(&RenderWarning::UnsupportedImage {
            codec: "JBIG2Decode".to_string()
        }),
        "the codec must be named, not silently skipped: {:?}",
        bitmap.warnings
    );
    assert!(
        !is_white(image_pixel(&bitmap, 32, 28)),
        "a blank white page reported as success is the failure this whole \
         gap exists to prevent; the neutral placeholder must be drawn instead"
    );
}

/// Just the two segments of Annex H.1's page 2 that this build decodes: the
/// page information segment, and the generic region.
///
/// Sliced out of [`ANNEX_H_PAGE_2`] rather than transcribed a second time, so
/// there is one copy of the annex here and these offsets are checkable by
/// walking clause 7.2's headers through it.
const PAGE_INFORMATION_SEGMENT: std::ops::Range<usize> = 0..30;
const GENERIC_REGION_SEGMENT: std::ops::Range<usize> = 112..158;

/// The decoder's own stable identifiers for what an image tolerated
/// (ruling 10).
fn reasons(bitmap: &tinker_pdf::Bitmap) -> Vec<String> {
    bitmap
        .warnings
        .iter()
        .filter_map(|w| match w {
            RenderWarning::DamagedImage { reason, .. } => Some(reason.clone()),
            _ => None,
        })
        .collect()
}

/// **`/JBIG2Globals` is read, and this is how you can tell.**
///
/// The same two clean segments decode twice: once alone, once with a symbol
/// dictionary in a globals stream. The stream itself has nothing in it to
/// warn about, so the only thing that can produce `jbig2-segment-skipped` in
/// the second render is a segment that came out of the globals.
///
/// The obvious version of this test asserts only the second half — and passes
/// on a build that never opens the globals stream at all, because the image's
/// own text region was producing the warning. Injecting exactly that, a
/// `/JBIG2Globals` that always resolves to nothing, was caught by **nothing**
/// until the clean baseline was put beside it.
#[test]
fn a_symbol_dictionary_reaches_the_decoder_through_the_globals_stream() {
    let mut coded = ANNEX_H_PAGE_2[PAGE_INFORMATION_SEGMENT].to_vec();
    coded.extend_from_slice(&ANNEX_H_PAGE_2[GENERIC_REGION_SEGMENT]);

    let alone = render(jbig2_page(&coded, 64, 56, "", &[]));
    assert!(
        is_black(image_pixel(&alone, 4, 11)),
        "the region draws on its own"
    );
    assert!(
        reasons(&alone).is_empty(),
        "this stream has nothing to warn about, and the half below depends \
         on that: {:?}",
        reasons(&alone)
    );

    let globals = segment(0, 0, 0, &[0u8; 8]); // symbol dictionary, page 0
    let with_globals = render(jbig2_page(&coded, 64, 56, "", &globals));
    assert!(
        is_black(image_pixel(&with_globals, 4, 11)),
        "and still draws with globals attached"
    );
    assert!(
        reasons(&with_globals).contains(&"jbig2-segment-skipped".to_string()),
        "the only segment that can have been skipped is the one in the \
         globals stream: {:?}",
        reasons(&with_globals)
    );
}

/// A file whose whole payload is a shared symbol dictionary: refused, and
/// named.
///
/// This is the shape a scanned book takes — one globals object holding the
/// dictionary, one image object per page holding only text regions — so it is
/// the common case in the corpus rather than a corner.
#[test]
fn a_globals_only_symbol_dictionary_is_refused_and_named() {
    let globals = segment(0, 0, 0, &[0u8; 8]); // symbol dictionary, page 0
    let mut coded = segment(1, 48, 1, &page_information(64, 56));
    coded.extend(segment(2, 6, 1, &[0u8; 8]));

    let bitmap = render(jbig2_page(&coded, 64, 56, "", &globals));
    assert!(
        bitmap.warnings.contains(&RenderWarning::UnsupportedImage {
            codec: "JBIG2Decode".to_string()
        }),
        "{:?}",
        bitmap.warnings
    );
    assert!(
        reasons(&bitmap).contains(&"jbig2-segment-skipped".to_string()),
        "the refusal itself carries no detail, so this warning is the only \
         thing that can say why it refused: {:?}",
        reasons(&bitmap)
    );
}

/// **The case the refusal has to get right in both directions.** A file with
/// a generic region *and* a symbol dictionary this build cannot decode.
///
/// The region draws — refusing it would throw away a picture that decoded
/// perfectly — and the missing lineage is still named, so "the scan decoded"
/// and "the scan decoded whole" stay apart (ruling 10). This is Annex H.1's
/// second page exactly: one generic region, plus a symbol dictionary, a text
/// region, a pattern dictionary and a halftone region that are all absent
/// from the result.
#[test]
fn a_generic_region_beside_an_undecodable_symbol_dictionary_draws_and_reports() {
    let bitmap = render(jbig2_page(&ANNEX_H_PAGE_2, 64, 56, "", &[]));

    assert!(
        !bitmap.warnings.contains(&RenderWarning::UnsupportedImage {
            codec: "JBIG2Decode".to_string()
        }),
        "a page that decoded a region is not a refusal: {:?}",
        bitmap.warnings
    );
    assert!(
        is_black(image_pixel(&bitmap, 4, 11)),
        "the generic region drew"
    );

    assert!(
        reasons(&bitmap).contains(&"jbig2-segment-skipped".to_string()),
        "the text and halftone regions are missing from this page and \
         ruling 10 wants that said: {:?}",
        reasons(&bitmap)
    );
}

/// A stream that is not JBIG2 at all is refused rather than sampled.
#[test]
fn arbitrary_bytes_under_a_jbig2_filter_draw_the_placeholder() {
    let bitmap = render(jbig2_page(b"not a jbig2 stream at all", 64, 56, "", &[]));
    assert!(
        bitmap.warnings.contains(&RenderWarning::UnsupportedImage {
            codec: "JBIG2Decode".to_string()
        }),
        "{:?}",
        bitmap.warnings
    );
}
