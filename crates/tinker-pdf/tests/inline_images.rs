//! An inline image decodes exactly as the same image would in an XObject
//! (8.9.7).
//!
//! `BI … ID … EI` used to run a filter chain of its own, three files away from
//! the one every other image uses. It passed no predictor, hardcoded
//! `/EarlyChange`, and refused DCT and CCITT outright — so the ordinary way to
//! compress image samples produced noise with nothing said about it, and the
//! two codecs 8.9.7 explicitly permits inline drew a grey placeholder.
//!
//! The shape of every test here is the same and it is deliberate: the *same
//! bytes* go through both paths and the two bitmaps must be equal. A parity
//! assertion is the only one that cannot be satisfied by both paths agreeing
//! on the wrong answer for different reasons — and it is also the guard on the
//! other direction, since the chain builder is now shared and breaking the
//! XObject path to fix the inline one would show up here first. The pixel
//! assertions beside it are what stops both paths being wrong together.

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

/// A one-page document whose content is `content`, verbatim.
fn page_with_content(content: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"%PDF-1.7\n");
    bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    bytes.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
          /Resources << >> /Contents 4 0 R >>\nendobj\n",
    );
    bytes.extend_from_slice(
        format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes(),
    );
    bytes.extend_from_slice(content);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(b"trailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n");
    bytes
}

/// The same page, drawing one image XObject over the whole of it.
///
/// `extra` is spliced into the image dictionary, so the caller writes the same
/// `/Filter` and `/DecodeParms` it wrote inline.
fn page_with_xobject(extra: &str, stream: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"%PDF-1.7\n");
    bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    bytes.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
          /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
    );
    bytes.extend_from_slice(b"4 0 obj\n<< /Length 27 >>\nstream\n");
    bytes.extend_from_slice(b"q 20 0 0 20 0 0 cm /Im0 Do Q");
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(
        format!(
            "5 0 obj\n<< /Type /XObject /Subtype /Image\n{extra}\n/Length {} >>\nstream\n",
            stream.len()
        )
        .as_bytes(),
    );
    bytes.extend_from_slice(stream);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    bytes.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");
    bytes
}

/// The same page, drawing one inline image over the whole of it.
fn page_with_inline(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend_from_slice(b"q 20 0 0 20 0 0 cm BI ");
    content.extend_from_slice(dict.as_bytes());
    content.extend_from_slice(b" ID ");
    content.extend_from_slice(data);
    content.extend_from_slice(b" EI Q");
    page_with_content(&content)
}

/// Four rows of four RGB pixels: red, red, blue, blue.
///
/// Every row is identical, which is what makes the PNG `Up` predictor worth
/// testing with — rows 1 to 3 encode to nothing but zeroes, so a decoder that
/// skips the un-predicting step sees three rows of black and a first row
/// shifted one byte by the filter tag. That is a wrong image no assertion on
/// "did anything draw" would catch.
fn stripe_rows() -> Vec<Vec<u8>> {
    let row: Vec<u8> = [[255u8, 0, 0], [255, 0, 0], [0, 0, 255], [0, 0, 255]].concat();
    vec![row; 4]
}

/// PNG `Up`-filters rows and prefixes each with its tag (RFC 2083, 6.4).
fn png_up(rows: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev = vec![0u8; rows.first().map_or(0, Vec::len)];
    for row in rows {
        out.push(2);
        for (i, byte) in row.iter().enumerate() {
            out.push(byte.wrapping_sub(prev.get(i).copied().unwrap_or(0)));
        }
        prev = row.clone();
    }
    out
}

fn hex(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for byte in bytes {
        out.extend_from_slice(format!("{byte:02X}").as_bytes());
    }
    out.push(b'>');
    out
}

/// Both halves of the stripe are where they belong, in the colours they
/// belong in.
///
/// Asserted on both paths, not only on the parity between them: a shared chain
/// builder that broke *both* would satisfy an equality assertion perfectly.
fn assert_stripe(bitmap: &tinker_pdf::Bitmap, what: &str) {
    let left = pixel(bitmap, 4, 10);
    let right = pixel(bitmap, 15, 10);
    let low = pixel(bitmap, 4, 17);
    assert!(
        left.0 > 200 && left.2 < 60,
        "{what}: the left half is red, got {left:?}"
    );
    assert!(
        right.2 > 200 && right.0 < 60,
        "{what}: the right half is blue, got {right:?}"
    );
    assert!(
        low.0 > 200 && low.2 < 60,
        "{what}: the last row repeats the first — black here is the predictor \
         never having run, got {low:?}"
    );
}

/// Milestone 1's exit criterion. A Flate-with-PNG-predictor inline image
/// decodes to the same samples as the identical XObject image.
///
/// `/DP` was in the abbreviation table, so the key reached the re-parsed
/// dictionary intact and was then never read: the inline chain passed `None`
/// where the predictor goes. The array form is used on both sides because
/// `[/AHx /Fl]` is the shape that proves the *positional* match between filter
/// and parameters, which the single-dictionary form cannot.
#[test]
fn an_inline_image_with_a_png_predictor_matches_the_same_xobject() {
    let coded = hex(&tinker_pdf_filters::zlib_compress(&png_up(&stripe_rows())));
    let parms = "/DecodeParms [null << /Predictor 15 /Colors 3 \
                 /BitsPerComponent 8 /Columns 4 >>]";

    let inline = render(page_with_inline(
        "/W 4 /H 4 /CS /RGB /BPC 8 /F [/AHx /Fl] \
         /DP [null << /Predictor 15 /Colors 3 /BitsPerComponent 8 /Columns 4 >>]",
        &coded,
    ));
    let xobject = render(page_with_xobject(
        &format!(
            "/Width 4 /Height 4 /ColorSpace /DeviceRGB /BitsPerComponent 8\n\
             /Filter [/ASCIIHexDecode /FlateDecode] {parms}"
        ),
        &coded,
    ));

    assert_stripe(&xobject, "as an XObject");
    assert_stripe(&inline, "inline");
    assert_eq!(
        inline.data, xobject.data,
        "the same bytes through both paths must produce the same pixels"
    );
    assert!(
        inline.warnings.is_empty(),
        "and no excuses: {:?}",
        inline.warnings
    );
}

/// The single-dictionary `/DP` form, written the way a compact producer writes
/// it — no space between a name and whatever follows it.
///
/// 7.2.2 makes `/` a delimiter, so `/F/Fl/DP<<…>>` is ordinary PDF. The
/// abbreviation rewrite searched for `"/Fl "` and found nothing in it, which
/// left the whole `/Filter` entry a two-letter name no branch matched — the
/// deflate bytes were then sampled as if they were pixels.
#[test]
fn a_compact_inline_dictionary_still_finds_its_filter_and_parameters() {
    let coded = hex(&tinker_pdf_filters::zlib_compress(&png_up(&stripe_rows())));

    let bitmap = render(page_with_inline(
        "/W 4/H 4/CS/RGB/BPC 8/F[/AHx/Fl]/DP[null<</Predictor 15/Colors 3\
         /BitsPerComponent 8/Columns 4>>]",
        &coded,
    ));

    assert_stripe(&bitmap, "compact");
    assert!(
        bitmap.warnings.is_empty(),
        "and no excuses: {:?}",
        bitmap.warnings
    );
}

/// Encodes bytes as one LZW literal code each, growing the code width where
/// `early_change` says it grows (7.4.4.3).
///
/// Not a compressor: every byte becomes its own code, so the output is larger
/// than the input. That is the whole point — the only variable under test is
/// *when* the width goes from nine bits to ten, and the two settings put that
/// one code apart. Everything decoded after that point differs.
fn lzw_literals(input: &[u8], early_change: bool) -> Vec<u8> {
    let early = u32::from(early_change);
    let mut out = Vec::new();
    let mut acc = 0u32;
    let mut nbits = 0u32;
    let emit = |code: u32, width: u32, out: &mut Vec<u8>, acc: &mut u32, nbits: &mut u32| {
        *acc = (*acc << width) | code;
        *nbits += width;
        while *nbits >= 8 {
            out.push((*acc >> (*nbits - 8)) as u8);
            *nbits -= 8;
        }
    };

    let mut width = 9u32;
    // 256 is Clear, 257 is EOD, and the table starts filling at 258.
    emit(256, width, &mut out, &mut acc, &mut nbits);
    let mut next = 258u32;
    for (i, byte) in input.iter().enumerate() {
        emit(u32::from(*byte), width, &mut out, &mut acc, &mut nbits);
        // The decoder can only define an entry once it has a previous code,
        // so the first code after a Clear adds nothing.
        if i > 0 {
            next += 1;
            if width < 12 && next + early >= 1 << width {
                width += 1;
            }
        }
    }
    emit(257, width, &mut out, &mut acc, &mut nbits);
    if nbits > 0 {
        out.push((acc << (8 - nbits)) as u8);
    }
    out
}

/// 32 rows of 32 greyscale samples: the left half black, the right half white.
fn half_and_half() -> Vec<u8> {
    let row: Vec<u8> = (0..32u32)
        .map(|x| if x < 16 { 0x00 } else { 0xFF })
        .collect();
    row.repeat(32)
}

/// `/EarlyChange 0` is honoured rather than assumed.
///
/// The inline path passed `true` unconditionally, so a stream encoded the
/// other way desynchronised at the code where the width grows and decoded to
/// noise from there on. The image is 1024 samples and the divergence lands at
/// code 255, so the top of the picture is right either way — which is exactly
/// why the assertion is on the bottom of it, and on parity with the XObject.
#[test]
fn an_inline_image_honours_early_change() {
    let coded = hex(&lzw_literals(&half_and_half(), false));

    let inline = render(page_with_inline(
        "/W 32 /H 32 /CS /G /BPC 8 /F [/AHx /LZW] /DP [null << /EarlyChange 0 >>]",
        &coded,
    ));
    let xobject = render(page_with_xobject(
        "/Width 32 /Height 32 /ColorSpace /DeviceGray /BitsPerComponent 8\n\
         /Filter [/ASCIIHexDecode /LZWDecode] /DecodeParms [null << /EarlyChange 0 >>]",
        &coded,
    ));

    for (bitmap, what) in [(&xobject, "as an XObject"), (&inline, "inline")] {
        let bottom_left = pixel(bitmap, 4, 17);
        let bottom_right = pixel(bitmap, 15, 17);
        assert!(
            bottom_left.0 < 60,
            "{what}: the bottom-left quarter is black, got {bottom_left:?} — \
             this is past the code where the width grows"
        );
        assert!(
            bottom_right.0 > 200,
            "{what}: and the bottom-right quarter is white, got {bottom_right:?}"
        );
    }
    assert_eq!(
        inline.data, xobject.data,
        "the same LZW stream through both paths must produce the same pixels"
    );
}

/// A progressive greyscale 8x8 JPEG, DC-only.
///
/// The same fixture `images.rs` uses for the XObject path, so the two tests
/// below compare the two paths on bytes that are known to decode.
fn progressive_jpeg() -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];

    // DQT: a large DC quantiser, so the one bit the second scan adds is a
    // visible difference rather than a rounding one.
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    let mut quant = [1u8; 64];
    quant[0] = 255;
    out.extend_from_slice(&quant);

    // SOF2: progressive, 8-bit, 8x8, one component, no subsampling.
    out.extend_from_slice(&[0xFF, 0xC2, 0x00, 0x0B, 0x08]);
    out.extend_from_slice(&[0x00, 0x08, 0x00, 0x08, 0x01, 0x01, 0x11, 0x00]);

    // DHT, DC table 0: a single two-bit code "00" meaning magnitude size 2.
    let mut counts = [0u8; 16];
    counts[1] = 1;
    let mut dht = vec![0x00];
    dht.extend_from_slice(&counts);
    dht.push(0x02);
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&dht);

    // First DC scan, band 0..0, Ah 0 Al 1.
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01]);
    out.push(0b0001_1111);

    // Refining DC scan, Ah 1 Al 0, with the stuffed zero a real encoder writes.
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x00, 0x10]);
    out.extend_from_slice(&[0xFF, 0x00]);

    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

/// 8.9.7 permits DCT inline, and it now decodes there.
///
/// It used to reach the catch-all and be reported as a codec this build does
/// not have — which it has had since the XObject path was fixed. Hex-armoured
/// so the chain is `[/AHx /DCT]`: a codec behind another filter is the shape
/// that used to hand the JPEG decoder deflate output on the XObject side, and
/// there was nothing at all testing it inline.
#[test]
fn an_inline_jpeg_decodes_rather_than_reporting_a_codec() {
    let coded = hex(&progressive_jpeg());

    let inline = render(page_with_inline(
        "/W 8 /H 8 /CS /G /BPC 8 /F [/AHx /DCT]",
        &coded,
    ));
    let xobject = render(page_with_xobject(
        "/Width 8 /Height 8 /ColorSpace /DeviceGray /BitsPerComponent 8\n\
         /Filter [/ASCIIHexDecode /DCTDecode]",
        &coded,
    ));

    let (r, g, b) = pixel(&inline, 10, 10);
    assert_eq!((r, g, b), (r, r, r), "grey, got ({r}, {g}, {b})");
    assert!(
        (20..=45).contains(&r),
        "the JPEG decoded to its own dark grey rather than to a placeholder: {r}"
    );
    assert!(
        !inline
            .warnings
            .iter()
            .any(|w| matches!(w, tinker_pdf::RenderWarning::UnsupportedImage { .. })),
        "and reports no unsupported codec: {:?}",
        inline.warnings
    );
    assert_eq!(
        inline.data, xobject.data,
        "the same JPEG through both paths must produce the same page"
    );
}

/// 8.9.5.2's `/Decode [1 0]` reaches an inline JPEG, which returns its own
/// pixels and so never joins the sample loop that reads the array.
#[test]
fn a_decode_array_inverts_an_inline_jpeg() {
    let coded = hex(&progressive_jpeg());
    let at = |dict: &str| pixel(&render(page_with_inline(dict, &coded)), 10, 10);

    let plain = at("/W 8 /H 8 /CS /G /BPC 8 /F [/AHx /DCT]");
    let inverted = at("/W 8 /H 8 /CS /G /BPC 8 /D [1 0] /F [/AHx /DCT]");

    assert!(plain.0 < 60, "the plain image is dark: {plain:?}");
    assert!(
        inverted.0 > 195,
        "and the inverted one is light: {inverted:?}"
    );
}

/// A filter name no build knows ends the chain, and is reported rather than
/// sampled.
#[test]
fn an_inline_image_with_an_unknown_filter_is_reported() {
    let bitmap = render(page_with_inline(
        "/W 2 /H 2 /CS /G /BPC 8 /F /Whirlpool",
        &[0x00, 0x00, 0x00, 0x00],
    ));

    assert!(
        bitmap.warnings.iter().any(|w| matches!(
            w,
            tinker_pdf::RenderWarning::UnsupportedImage { codec } if codec == "Whirlpool"
        )),
        "got {:?}",
        bitmap.warnings
    );
}
