//! Images reach the page (8.9).
//!
//! There was no test anywhere that an image XObject actually renders. That
//! became apparent when a refactor moved `GlyphSource::image` out of its trait
//! impl and into an inherent one: the trait's default — which returns nothing
//! — took over, every image in every document silently stopped drawing, and
//! the whole suite stayed green. Only a dead-code warning gave it away.
//!
//! These are the tests that would have failed.

use tinker_pdf::{Document, RenderOptions};
use tinker_pdf_cos::{DocumentBuilder, ImageData};

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

/// A solid red 2×2 image drawn over the whole page must make the page red.
#[test]
fn an_rgb_image_is_drawn() {
    let mut builder = DocumentBuilder::new();
    let red = [255u8, 0, 0].repeat(4);
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 2,
            height: 2,
            data: &red,
        }
    ));
    builder.add_page(20.0, 20.0, |page| {
        page.image(b"Im0", 0.0, 0.0, 20.0, 20.0);
    });

    let bitmap = render(builder.finish());
    let (r, g, b) = pixel(&bitmap, 10, 10);
    assert!(
        r > 200 && g < 60 && b < 60,
        "the image painted red, got ({r}, {g}, {b}) — white means it never drew"
    );
}

/// A greyscale image, to exercise the one-component path separately.
#[test]
fn a_grey_image_is_drawn() {
    let mut builder = DocumentBuilder::new();
    let grey = [0x40u8; 4];
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Gray8 {
            width: 2,
            height: 2,
            data: &grey,
        }
    ));
    builder.add_page(20.0, 20.0, |page| {
        page.image(b"Im0", 0.0, 0.0, 20.0, 20.0);
    });

    let bitmap = render(builder.finish());
    let (r, g, b) = pixel(&bitmap, 10, 10);
    assert!(
        (0x30..=0x50).contains(&r) && r == g && g == b,
        "a dark grey, got ({r}, {g}, {b})"
    );
}

/// The image occupies the unit square of the current transform, so it lands
/// where the `cm` puts it and nowhere else.
#[test]
fn an_image_lands_where_its_transform_puts_it() {
    let mut builder = DocumentBuilder::new();
    let red = [255u8, 0, 0].repeat(4);
    builder.add_image(
        b"Im0",
        &ImageData::Rgb8 {
            width: 2,
            height: 2,
            data: &red,
        },
    );
    builder.add_page(40.0, 40.0, |page| {
        // The left half only.
        page.image(b"Im0", 0.0, 0.0, 20.0, 40.0);
    });

    let bitmap = render(builder.finish());
    let left = pixel(&bitmap, 5, 20);
    let right = pixel(&bitmap, 35, 20);
    assert!(
        left.0 > 200 && left.1 < 60,
        "the left half is red: {left:?}"
    );
    assert!(
        right.0 > 200 && right.1 > 200 && right.2 > 200,
        "the right half is untouched: {right:?}"
    );
}

/// A progressive JPEG, which appeared in half of the first real-world files
/// tested and until now rendered as a grey placeholder.
///
/// One 8×8 block, DC only, sent across two scans: the first carries the value
/// with its low bit removed and the second supplies that bit. The second scan
/// is also what makes this a progressive file rather than a sequential one
/// wearing an SOF2 marker, so a decoder that quietly ignored later scans would
/// land two quantisation steps away and fail the assertion below.
fn progressive_jpeg() -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];

    // DQT: a large DC quantiser, so the one bit the second scan adds is a
    // visible difference rather than a rounding one.
    out.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, 0x00]);
    let mut quant = [1u8; 64];
    quant[0] = 255;
    out.extend_from_slice(&quant);

    // SOF2: progressive, 8-bit, 8×8, one component, no subsampling.
    out.extend_from_slice(&[0xFF, 0xC2, 0x00, 0x0B, 0x08]);
    out.extend_from_slice(&[0x00, 0x08, 0x00, 0x08, 0x01, 0x01, 0x11, 0x00]);

    // DHT, DC table 0: a single two-bit code "00" meaning magnitude size 2.
    // No AC table: a DC-only progressive file never needs one, and a decoder
    // that demanded one anyway would refuse this.
    let mut counts = [0u8; 16];
    counts[1] = 1;
    let mut dht = vec![0x00];
    dht.extend_from_slice(&counts);
    dht.push(0x02);
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&((dht.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(&dht);

    // First DC scan, band 0..0, Ah 0 Al 1: code "00" then "01", which extends
    // to −2 and the point transform stores as −4.
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x00, 0x01]);
    out.push(0b0001_1111);

    // Refining DC scan, Ah 1 Al 0: one set bit, making −3. Its padding runs
    // the byte to 0xFF, so the fixture carries the stuffed zero a real encoder
    // would write — and a reader that skipped it would desynchronise.
    out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x00, 0x10]);
    out.extend_from_slice(&[0xFF, 0x00]);

    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

#[test]
fn a_progressive_jpeg_is_decoded_rather_than_placeheld() {
    let jpeg = progressive_jpeg();
    let mut builder = DocumentBuilder::new();
    assert!(
        builder.add_image(b"Im0", &ImageData::Jpeg(&jpeg)),
        "the builder reads a progressive SOF2 header"
    );
    builder.add_page(20.0, 20.0, |page| {
        page.image(b"Im0", 0.0, 0.0, 20.0, 20.0);
    });

    let bitmap = render(builder.finish());
    let (r, g, b) = pixel(&bitmap, 10, 10);

    // −3 × 255 through the transform is a dark grey near 32. The placeholder
    // this used to render was a light neutral, and stopping after the first
    // scan would leave −4 × 255, which clamps to black.
    assert_eq!((r, g, b), (r, r, r), "grey, got ({r}, {g}, {b})");
    assert!(
        (20..=45).contains(&r),
        "the decoded value, not a placeholder or an unrefined first scan: {r}"
    );
    assert!(
        bitmap.warnings.is_empty(),
        "and nothing was degraded: {:?}",
        bitmap.warnings
    );
}

/// An image resource that names a codec this build has no decoder for is
/// reported *and* leaves a placeholder, rather than silently occupying no
/// space (ruling 2).
#[test]
fn an_undecodable_image_leaves_a_placeholder() {
    let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 44 >>\nstream\n\
q 20 0 0 20 0 0 cm /Im0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /JPXDecode /Length 4 >>\n\
stream\n\x00\x00\x00\x00\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n";

    let bitmap = render(bytes.to_vec());
    let (r, g, b) = pixel(&bitmap, 10, 10);
    assert!(
        r < 240 && r == g && g == b,
        "a neutral placeholder, got ({r}, {g}, {b})"
    );
    assert!(
        bitmap
            .warnings
            .iter()
            .any(|w| matches!(w, tinker_pdf::RenderWarning::UnsupportedImage { .. })),
        "and it says which codec was missing: {:?}",
        bitmap.warnings
    );
}

/// 8.9.5.2: `/Decode [1 0]` inverts the samples. Ignoring it renders the
/// image as its own negative, which looks like a decoder bug rather than a
/// missing feature.
#[test]
fn a_decode_array_inverts_the_samples() {
    let image = |decode: &str| -> tinker_pdf::Bitmap {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 28 >>\nstream\n\
q 20 0 0 20 0 0 cm /Im0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 {decode} /Length 4 >>\n\
stream\n\x00\x00\x00\x00\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n"
        );
        render(bytes.into_bytes())
    };

    let plain = pixel(&image(""), 10, 10);
    assert!(plain.0 < 40, "all-zero grey samples are black: {plain:?}");

    let inverted = pixel(&image("/Decode [1 0]"), 10, 10);
    assert!(
        inverted.0 > 200,
        "and inverted they are white: {inverted:?}"
    );
}

/// 11.6.5.3: an `/SMask` supplies per-sample opacity. Without it every
/// soft-masked image paints as an opaque rectangle.
#[test]
fn a_soft_mask_makes_an_image_transparent() {
    let with_mask = |smask: &str| -> tinker_pdf::Bitmap {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 28 >>\nstream\n\
q 20 0 0 20 0 0 cm /Im0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 {smask} /Length 4 >>\n\
stream\n\x00\x00\x00\x00\nendstream\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 4 >>\n\
stream\n\x00\x00\x00\x00\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n"
        );
        render(bytes.into_bytes())
    };

    // Object 6 is an all-zero mask: fully transparent everywhere.
    let opaque = pixel(&with_mask(""), 10, 10);
    assert!(
        opaque.0 < 40,
        "unmasked, the black image paints: {opaque:?}"
    );

    let masked = pixel(&with_mask("/SMask 6 0 R"), 10, 10);
    assert!(
        masked.0 > 200,
        "a zero mask hides it entirely, leaving the page: {masked:?}"
    );
}

/// A stencil mask paints in the current fill colour, not in black
/// (8.9.6.2). Baking black in at decode time paints every stencil black
/// whatever the page asked for.
#[test]
fn a_stencil_mask_paints_in_the_fill_colour() {
    let bytes = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 42 >>\nstream\n\
q 1 0 0 rg 20 0 0 20 0 0 cm /Im0 Do Q\n\
endstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ImageMask true /BitsPerComponent 1 /Length 2 >>\n\
stream\n\x00\x00\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n";

    let bitmap = render(bytes.to_vec());
    let (r, g, b) = pixel(&bitmap, 10, 10);
    assert!(
        r > 200 && g < 60 && b < 60,
        "the stencil paints red, got ({r}, {g}, {b}) — black means the \
         colour was baked in at decode time"
    );
}

/// Builds a one-page document whose content stream is raw bytes.
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

/// 8.9.7: an inline image carries its dictionary and samples in the content
/// stream itself. They were scanned past and discarded, so the image never
/// drew — and once undecodable images began leaving a placeholder, every
/// inline image became a grey rectangle instead of nothing.
#[test]
fn an_inline_image_is_decoded() {
    let mut content = Vec::new();
    content.extend_from_slice(b"q 20 0 0 20 0 0 cm BI /W 2 /H 2 /CS /RGB /BPC 8 ID ");
    content.extend_from_slice(&[0xFF, 0x00, 0x00].repeat(4));
    content.extend_from_slice(b" EI Q");

    let bitmap = render(page_with_content(&content));
    let (r, g, b) = pixel(&bitmap, 10, 10);
    assert!(
        r > 200 && g < 60 && b < 60,
        "the inline image painted red, got ({r}, {g}, {b})"
    );
    assert!(
        bitmap.warnings.is_empty(),
        "and needed no excuses: {:?}",
        bitmap.warnings
    );
}

/// Table 93's short keys and the long ones mean the same thing.
#[test]
fn an_inline_image_accepts_long_key_names() {
    let mut content = Vec::new();
    content.extend_from_slice(
        b"q 20 0 0 20 0 0 cm BI /Width 2 /Height 2 /ColorSpace /DeviceGray \
          /BitsPerComponent 8 ID ",
    );
    content.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    content.extend_from_slice(b" EI Q");

    let bitmap = render(page_with_content(&content));
    let (r, _, _) = pixel(&bitmap, 10, 10);
    assert!(r < 60, "all-zero grey samples are black, got {r}");
}

/// An inline image whose filter this build cannot run is reported, and the
/// placeholder marks where it was.
#[test]
fn an_undecodable_inline_image_is_reported() {
    let mut content = Vec::new();
    content.extend_from_slice(b"q 20 0 0 20 0 0 cm BI /W 2 /H 2 /CS /G /BPC 8 /F /DCT ID ");
    content.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    content.extend_from_slice(b" EI Q");

    let bitmap = render(page_with_content(&content));
    assert!(
        bitmap
            .warnings
            .iter()
            .any(|w| matches!(w, tinker_pdf::RenderWarning::UnsupportedImage { .. })),
        "got {:?}",
        bitmap.warnings
    );
}

/// A JPEG behind another filter.
///
/// `[/FlateDecode /DCTDecode]` is an ordinary shape — a producer compressing
/// the JPEG bytes again — and the image path used to hand the JPEG decoder
/// the *undecoded* stream, so it saw deflate output and refused a perfectly
/// good image. The CCITT half of the same bug had no refusal path at all and
/// rendered noise.
#[test]
fn an_image_codec_behind_another_filter_still_decodes() {
    let jpeg = progressive_jpeg();

    // Deflate the JPEG bytes, so the stream is [/FlateDecode /DCTDecode].
    let deflated = tinker_pdf_filters::zlib_compress(&jpeg);

    let bytes = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 27 >>\nstream\nq 20 0 0 20 0 0 cm /Im0 Do Q\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 8 /Height 8\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8\n\
   /Filter [/FlateDecode /DCTDecode] /Length {} >>\nstream\n",
        deflated.len()
    )
    .into_bytes();

    let mut file = bytes;
    file.extend_from_slice(&deflated);
    file.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");

    let bitmap = render(file);
    let (r, g, b) = pixel(&bitmap, 10, 10);
    assert_eq!((r, g, b), (r, r, r), "grey, got ({r}, {g}, {b})");
    assert!(
        (20..=45).contains(&r),
        "the JPEG decoded through the outer filter rather than being refused: {r}"
    );
}

/// 8.9.5.2: `/Decode [1 0]` inverts. Both codec paths returned before the
/// decode array was even parsed, so a JPEG marked inverted rendered
/// identically to one that was not.
#[test]
fn a_decode_array_inverts_a_jpeg_too() {
    let jpeg = progressive_jpeg();
    let with_decode = |decode: &str| -> tinker_pdf::Bitmap {
        let mut file = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 27 >>\nstream\nq 20 0 0 20 0 0 cm /Im0 Do Q\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 8 /Height 8\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 {decode}\n\
   /Filter /DCTDecode /Length {} >>\nstream\n",
            jpeg.len()
        )
        .into_bytes();
        file.extend_from_slice(&jpeg);
        file.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");
        render(file)
    };

    let plain = pixel(&with_decode(""), 10, 10);
    let inverted = pixel(&with_decode("/Decode [1 0]"), 10, 10);

    assert!(plain.0 < 60, "the plain image is dark: {plain:?}");
    assert!(
        inverted.0 > 195,
        "and the inverted one is light: {inverted:?}"
    );
}
