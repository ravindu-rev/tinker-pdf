//! Temporary probe: is a codec reachable behind another filter?

use tinker_pdf::{Document, RenderOptions};

fn pixel(b: &tinker_pdf::Bitmap, x: u32, y: u32) -> (u8, u8, u8) {
    let c = b.components();
    let at = (y as usize) * b.stride + (x as usize) * c;
    (b.data[at], b.data[at + 1], b.data[at + 2])
}

/// Same fixture as tests/images.rs: one 8x8 DC-only progressive block that
/// decodes to a dark grey near 32.
fn progressive_jpeg() -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    let mut quant = [1u8; 64];
    quant[0] = 255;
    out.extend_from_slice(&quant);
    out.extend_from_slice(&[0xFF, 0xC2, 0x00, 0x0B, 0x08]);
    out.extend_from_slice(&[0x00, 0x08, 0x00, 0x08, 0x01, 0x01, 0x11, 0x00]);
    let mut counts = [0u8; 16];
    counts[1] = 1;
    let mut dht = vec![0x00];
    dht.extend_from_slice(&counts);
    dht.push(0x02);
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&dht);
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01]);
    out.push(0b0001_1111);
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x00, 0x10]);
    out.extend_from_slice(&[0xFF, 0x00]);
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

/// Builds a one-page file whose only content is an image XObject with the
/// given /Filter value and stream payload.
fn page_with_image(filter: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    out.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n",
    );
    out.extend_from_slice(
        b"4 0 obj\n<< /Length 28 >>\nstream\nq 20 0 0 20 0 0 cm /Im0 Do Q\nendstream\nendobj\n",
    );
    out.extend_from_slice(
        format!(
            "5 0 obj\n<< /Type /XObject /Subtype /Image /Width 8 /Height 8\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter {} /Length {} >>\nstream\n",
            filter,
            payload.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(payload);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    out.extend_from_slice(b"trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");
    out
}

fn report(label: &str, bytes: Vec<u8>) {
    let doc = Document::open(bytes).expect("opens");
    let page = doc.page(0).expect("page");
    let bm = page.render(&RenderOptions::default());
    println!(
        "{label}: centre = {:?}   warnings = {:?}",
        pixel(&bm, 10, 10),
        bm.warnings
    );
}

#[test]
fn probe_codec_behind_a_filter() {
    let jpeg = progressive_jpeg();

    // Control: the codec is the only filter.
    report("DCT alone          ", page_with_image("/DCTDecode", &jpeg));

    // The same JPEG behind ASCIIHexDecode.
    let mut hex: Vec<u8> = jpeg.iter().flat_map(|b| format!("{b:02X}").into_bytes()).collect();
    hex.push(b'>');
    report(
        "[/ASCIIHex /DCT]   ",
        page_with_image("[/ASCIIHexDecode /DCTDecode]", &hex),
    );

    // And behind FlateDecode, the other common wrapper.
    let flate = tinker_pdf_filters::zlib_compress(&jpeg);
    report(
        "[/Flate /DCT]      ",
        page_with_image("[/FlateDecode /DCTDecode]", &flate),
    );

    // CCITT: an all-white 8x8 G4 image is not easy to hand-code, so this only
    // shows what the wrapped case does relative to the unwrapped one.
    let ccitt_raw: Vec<u8> = vec![0x26, 0xA0, 0x26, 0xA0, 0x26, 0xA0, 0x26, 0xA0];
    report(
        "CCITT alone        ",
        page_with_image("/CCITTFaxDecode", &ccitt_raw),
    );
    let mut ccitt_hex: Vec<u8> = ccitt_raw
        .iter()
        .flat_map(|b| format!("{b:02X}").into_bytes())
        .collect();
    ccitt_hex.push(b'>');
    report(
        "[/ASCIIHex /CCITT] ",
        page_with_image("[/ASCIIHexDecode /CCITTFaxDecode]", &ccitt_hex),
    );
}
