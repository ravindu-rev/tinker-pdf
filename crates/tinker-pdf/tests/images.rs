//! Images reach the page (8.9).
//!
//! There was no test anywhere that an image XObject actually renders. That
//! became apparent when a refactor moved `GlyphSource::image` out of its trait
//! impl and into an inherent one: the trait's default — which returns nothing
//! — took over, every image in every document silently stopped drawing, and
//! the whole suite stayed green. Only a dead-code warning gave it away.
//!
//! These are the tests that would have failed.

mod written_support;

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

    let bitmap = render(built("an rgb image is drawn", builder));
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

    let bitmap = render(built("a grey image is drawn", builder));
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

    let bitmap = render(built("an image lands where its transform puts it", builder));
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

    let bitmap = render(built(
        "a progressive jpeg is decoded rather than placeheld",
        builder,
    ));
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

/// Table 89: `/Interpolate` asks for a magnified image to be smoothed.
///
/// It was in the inline-image abbreviation table and read by nothing else in
/// the engine — no field on the decoded image, no branch at the sample site —
/// so a file that asked for smoothing got the same hard sample edges as one
/// that asked for none. The two documents below differ in that one key.
#[test]
fn interpolate_reaches_the_sampler() {
    // Two black samples and two mid-grey ones, magnified ten times per axis.
    // Mid grey rather than white because the whole file is a `str`, and the
    // samples have to be characters a `str` can hold.
    let with_flag = |interpolate: &str| -> tinker_pdf::Bitmap {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 27 >>\nstream\nq 20 0 0 20 0 0 cm /Im0 Do Q\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 2\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 {interpolate} /Length 4 >>\n\
stream\n\x00\x7f\x7f\x00\nendstream\nendobj\n\
trailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n"
        );
        render(bytes.into_bytes())
    };

    let levels = |bitmap: &tinker_pdf::Bitmap| {
        let mut values: Vec<u8> = bitmap
            .data
            .chunks_exact(bitmap.components())
            .map(|p| p[0])
            .collect();
        values.sort_unstable();
        values.dedup();
        values
    };

    let hard = levels(&with_flag(""));
    assert_eq!(
        hard,
        vec![0, 127],
        "without the key the samples are copied: a barcode stays sharp"
    );

    let smooth = levels(&with_flag("/Interpolate true"));
    assert!(
        smooth.len() > 20,
        "with it the magnified samples are blended, got {} levels",
        smooth.len()
    );
}

/// The same key, in an inline image's short spelling (Table 93).
#[test]
fn an_inline_images_interpolate_abbreviation_reaches_the_sampler() {
    let levels = |dict: &[u8]| {
        let mut content = Vec::new();
        content.extend_from_slice(b"q 20 0 0 20 0 0 cm BI ");
        content.extend_from_slice(dict);
        content.extend_from_slice(b" ID ");
        content.extend_from_slice(&[0x00, 0x7F, 0x7F, 0x00]);
        content.extend_from_slice(b" EI Q");

        let bitmap = render(page_with_content(&content));
        let mut values: Vec<u8> = bitmap
            .data
            .chunks_exact(bitmap.components())
            .map(|p| p[0])
            .collect();
        values.sort_unstable();
        values.dedup();
        values.len()
    };

    assert_eq!(
        levels(b"/W 2 /H 2 /CS /G /BPC 8"),
        2,
        "no key, no smoothing"
    );
    assert!(
        levels(b"/W 2 /H 2 /CS /G /BPC 8 /I true") > 20,
        "`/I` is `/Interpolate`, and it has to survive the rewrite"
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
///
/// The codec here is JPX rather than DCT, which it used to be: 8.9.7 permits
/// DCT inline and this build now decodes it, so a DCT fixture would go on
/// warning only because four zero bytes are not a JPEG — a green test for a
/// capability it no longer describes. JPX is a gated codec wherever it appears
/// (ruling 2), which is what this test is about.
#[test]
fn an_undecodable_inline_image_is_reported() {
    let mut content = Vec::new();
    content.extend_from_slice(b"q 20 0 0 20 0 0 cm BI /W 2 /H 2 /CS /G /BPC 8 /F /JPXDecode ID ");
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

/// A one-page document drawing the image XObject `/Im0` over the whole 20 by
/// 20 page: its dictionary's `keys` beside `/Subtype /Image`, and `data` its
/// stream.
fn page_with_image(keys: &str, data: &[u8]) -> Vec<u8> {
    let mut file = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 27 >>\nstream\nq 20 0 0 20 0 0 cm /Im0 Do Q\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image {keys} /Length {} >>\nstream\n",
        data.len()
    )
    .into_bytes();
    file.extend_from_slice(data);
    file.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");
    file
}

/// A spot colour whose tints run from white to red, so a sample's tint is
/// told from its grey by the green and blue it takes away (8.6.6.4).
const SPOT: &str = "[/Separation /Spot /DeviceRGB << /FunctionType 2 /Domain [0 1] \
                    /C0 [1 1 1] /C1 [1 0 0] /N 1 >>]";

/// A palette of 256 entries, red below index 128 and blue from it (8.6.6.3).
fn palette() -> String {
    let entries: String = (0..256)
        .map(|index| if index < 128 { "FF0000" } else { "0000FF" })
        .collect();
    format!("[/Indexed /DeviceRGB 255 <{entries}>]")
}

/// The samples of the first image a document's first page draws, as
/// `Page::images` decodes them.
fn first_image_samples(file: Vec<u8>) -> Vec<u8> {
    Document::open(file)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .images()
        .first()
        .expect("an image")
        .samples
        .clone()
}

/// 8.9.5: a JPEG is drawn in the `/ColorSpace` its dictionary names, not in
/// the colour its frame alone gives — here a spot colour's tints, with and
/// without `/Decode`, and a palette.
///
/// Each is held to its twin: the JPEG's own samples, as `Page::images` lists
/// them, written as a plain sampled image in the same space, which must draw
/// the same bytes. And each must differ from the JPEG drawn in grey, because
/// two twins that agree since neither read the space prove nothing.
#[test]
fn a_jpeg_draws_in_the_space_its_dictionary_names() {
    let jpeg = progressive_jpeg();
    let dct = |space: &str, decode: &str| {
        page_with_image(
            &format!(
                "/Width 8 /Height 8 /ColorSpace {space} /BitsPerComponent 8 {decode} \
                 /Filter /DCTDecode"
            ),
            &jpeg,
        )
    };
    let samples = first_image_samples(dct("/DeviceGray", ""));
    assert_eq!(samples.len(), 64, "a byte a sample of the 8 by 8 frame");
    let grey = render(dct("/DeviceGray", ""));

    let palette = palette();
    for (space, decode) in [(SPOT, ""), (SPOT, "/Decode [1 0]"), (&*palette, "")] {
        let drawn = render(dct(space, decode));
        let twin = render(page_with_image(
            &format!("/Width 8 /Height 8 /ColorSpace {space} /BitsPerComponent 8 {decode}"),
            &samples,
        ));
        assert_eq!(
            drawn.data, twin.data,
            "{space} {decode}: the JPEG drew otherwise than its own samples draw in that space"
        );
        assert_ne!(drawn.data, grey.data, "{space} {decode}: drawn as grey");
        assert!(
            drawn.warnings.is_empty(),
            "{space} {decode}: {:?}",
            drawn.warnings
        );
    }

    // What the spaces make of a dark grey near 32: a light tint of the spot
    // colour, a deep one once `/Decode` reverses the sample, and the palette's
    // red.
    let (r, g, b) = pixel(&render(dct(SPOT, "")), 10, 10);
    assert!(
        r == 255 && g > 200 && g == b,
        "a light tint: ({r}, {g}, {b})"
    );
    let (r, g, b) = pixel(&render(dct(SPOT, "/Decode [1 0]")), 10, 10);
    assert!(r == 255 && g < 60 && g == b, "a deep tint: ({r}, {g}, {b})");
    assert_eq!(
        pixel(&render(dct(&palette, "")), 10, 10),
        (255, 0, 0),
        "the palette's red"
    );
}

/// An inline JPEG goes through the same entry point, its space read as an
/// inline image's is (8.9.7): a palette over it draws as the same samples
/// inline in the same space.
#[test]
fn an_inline_jpeg_draws_in_the_space_its_dictionary_names() {
    let inline = |filter: &str, data: &[u8]| {
        let hex: String = data.iter().map(|byte| format!("{byte:02X}")).collect();
        page_with_content(
            format!(
                "q 20 0 0 20 0 0 cm BI /W 8 /H 8 /CS {} /BPC 8 /F {filter} ID {hex}> EI Q",
                palette()
            )
            .as_bytes(),
        )
    };
    let jpeg = inline("[/AHx /DCT]", &progressive_jpeg());
    let samples = first_image_samples(jpeg.clone());
    assert_eq!(samples.len(), 64, "the inline JPEG's own samples");
    let drawn = render(jpeg);
    let twin = render(inline("/AHx", &samples));

    assert_eq!(
        drawn.data, twin.data,
        "the inline JPEG drew otherwise than its samples"
    );
    assert_eq!(pixel(&drawn, 10, 10), (255, 0, 0), "the palette's red");
    assert!(drawn.warnings.is_empty(), "{:?}", drawn.warnings);
}

/// A space of another number of components than the JPEG's frame cannot hold
/// its samples: it is refused and named under the image's name, and the JPEG
/// is drawn in its own colour (ruling 10).
#[test]
fn a_jpeg_whose_space_cannot_hold_its_samples_keeps_its_own_colour() {
    let jpeg = progressive_jpeg();
    let dct = |space: &str| {
        page_with_image(
            &format!(
                "/Width 8 /Height 8 /ColorSpace {space} /BitsPerComponent 8 /Filter /DCTDecode"
            ),
            &jpeg,
        )
    };
    let refused = render(dct("/DeviceRGB"));

    assert_eq!(
        refused.data,
        render(dct("/DeviceGray")).data,
        "drawn in the frame's own grey"
    );
    assert_eq!(
        refused.warnings,
        vec![tinker_pdf::RenderWarning::RepairedColorSpace {
            name: "Im0".to_string(),
            reason: "ImageSpaceComponents".to_string(),
        }],
        "and the refusal named"
    );
}

/// **A JPEG decodes into its image without a second copy**
/// (`docs/design/jpeg-speed.md`, "Decoding into the image"): beside the
/// image's RGB, a single-scan sequential 4:2:0 image's decode holds at most
/// 0.3 times that RGB — one MCU row of coefficients, of planes and of the
/// strip it hands on — read from the render's counted work. The page it
/// draws one sample a pixel is the decoder's samples, byte for byte, and the
/// uncounted render's.
#[test]
fn a_jpeg_decodes_into_its_image_without_a_second_copy() {
    use tinker_pdf_filters::{JpegOptions, JpegSampling, JpegSource, JpegSourceColour};

    let (width, height) = (256u32, 256u32);
    let samples: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            let (x, y) = (i % width, i / width);
            [x as u8, (y * 255 / height) as u8, ((x ^ y) * 3) as u8]
        })
        .collect();
    let jpeg = tinker_pdf_filters::jpeg_encode(
        &JpegSource {
            width,
            height,
            colour: JpegSourceColour::Rgb,
            stride: width as usize * 3,
            data: &samples,
        },
        &JpegOptions {
            sampling: JpegSampling::FourTwoZero,
            ..JpegOptions::default()
        },
    )
    .expect("the raster encodes");
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(b"Im0", &ImageData::Jpeg(&jpeg)));
    builder.add_page(f64::from(width), f64::from(height), |page| {
        page.image(b"Im0", 0.0, 0.0, f64::from(width), f64::from(height));
    });
    let doc = Document::open(built(
        "a jpeg decodes into its image without a second copy",
        builder,
    ))
    .expect("it opens");
    let page = doc.page(0).expect("a page");

    let counted = page.render(&RenderOptions {
        stats: true,
        ..RenderOptions::default()
    });
    let stats = counted.stats.expect("the render counted");
    let rgb = u64::from(width * height * 3);
    assert!(stats.jpeg_held > 0, "the decode's held bytes were counted");
    assert!(
        10 * stats.jpeg_held <= 3 * rgb,
        "the decode held {} bytes beside an image of {rgb}",
        stats.jpeg_held
    );

    let decoded = tinker_pdf_filters::jpeg_decode(&jpeg, 1 << 28).expect("it decodes");
    assert_eq!((decoded.width, decoded.height), (width, height));
    for y in 0..height {
        for x in 0..width {
            let at = (y * width + x) as usize * 3;
            let want = (decoded.data[at], decoded.data[at + 1], decoded.data[at + 2]);
            assert_eq!(pixel(&counted, x, y), want, "({x}, {y})");
        }
    }
    let plain = page.render(&RenderOptions::default());
    assert!(plain.data == counted.data, "counting changed the page");
}

/// What `builder` writes, held to the validate-on-save helper as a document
/// this engine wrote from nothing it did not write.
#[track_caller]
fn built(label: &str, builder: tinker_pdf::DocumentBuilder) -> Vec<u8> {
    let bytes = builder.finish();
    written_support::authored(label, &bytes);
    bytes
}

// ---- The drawn-size request (P-09.8) ----------------------------------------
//
// A JPEG is decoded at the smallest of an eighth, a quarter and a half whose
// image still has as many samples as the draw has pixels each way
// (`docs/design/jpeg-speed.md`, "The drawn-size request"), and the samples the
// inverse transforms output are counted (`Stats::jpeg_samples_transformed`).

/// A 4:2:0 baseline JPEG of `width` by `height` samples from the engine's own
/// encoder: a smooth, dark gradient, every channel of every sample under half
/// of full scale, so a pixel the image covers at all — at the least coverage
/// there is — is never the page's white.
fn smooth_jpeg(width: u32, height: u32) -> Vec<u8> {
    use tinker_pdf_filters::{JpegOptions, JpegSampling, JpegSource, JpegSourceColour};

    let samples: Vec<u8> = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| {
                [
                    (20 + x * 90 / width) as u8,
                    (30 + y * 80 / height) as u8,
                    (40 + (x + y) * 60 / (width + height)) as u8,
                ]
            })
        })
        .collect();
    tinker_pdf_filters::jpeg_encode(
        &JpegSource {
            width,
            height,
            colour: JpegSourceColour::Rgb,
            stride: width as usize * 3,
            data: &samples,
        },
        &JpegOptions {
            sampling: JpegSampling::FourTwoZero,
            ..JpegOptions::default()
        },
    )
    .expect("the raster encodes")
}

/// A page of `page` points drawing `content`, whose `/Im0` is the JPEG `jpeg`
/// with `keys` in its dictionary beside `/Subtype /Image`, and whose `/Mk` is
/// a 4 by 3 grey image a soft mask can name as `6 0 R`.
fn jpeg_page(page: (f64, f64), content: &str, keys: &str, jpeg: &[u8]) -> Vec<u8> {
    let (width, height) = page;
    let mut file = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n\
   /Resources << /XObject << /Im0 5 0 R /Mk 6 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Image /Width 4 /Height 3 /ColorSpace /DeviceGray \
   /BitsPerComponent 8 /Length 12 >>\nstream\n",
        content.len()
    )
    .into_bytes();
    file.extend_from_slice(&[200, 180, 160, 140, 120, 100, 90, 110, 130, 150, 170, 190]);
    file.extend_from_slice(b"\nendstream\nendobj\n");
    file.extend_from_slice(
        format!(
            "5 0 obj\n<< /Type /XObject /Subtype /Image {keys} /Length {} >>\nstream\n",
            jpeg.len()
        )
        .as_bytes(),
    );
    file.extend_from_slice(jpeg);
    file.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n");
    file
}

/// The dictionary keys a JPEG of `width` by `height` samples is drawn with.
fn jpeg_keys(width: u32, height: u32) -> String {
    format!(
        "/Width {width} /Height {height} /ColorSpace /DeviceRGB /BitsPerComponent 8 \
         /Filter /DCTDecode"
    )
}

/// Page 0 of `bytes` rendered at `scale` with its work counted.
fn counted_at(bytes: Vec<u8>, scale: f64) -> (tinker_pdf::Bitmap, tinker_pdf::Stats) {
    let bitmap = Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions {
            scale,
            stats: true,
            ..RenderOptions::default()
        });
    let stats = bitmap.stats.expect("the render counted");
    (bitmap, stats)
}

/// Samples a 512 by 384 4:2:0 JPEG's inverse transforms output at `N/8`:
/// 64 by 48 luma blocks and 32 by 24 of each chroma, 4 608 blocks, `N × N`
/// samples each.
fn transformed_512_by_384(n: u64) -> u64 {
    4_608 * n * n
}

/// **A7, the JPEG half: a JPEG drawn at a quarter of its size each way
/// transforms at most four samples a drawn pixel** (`docs/design/jpeg-speed.md`,
/// "Guard A7's JPEG half"), read from the render's counted work. A 512 by 384
/// 4:2:0 image drawn over 128 by 96 pixels is decoded at a quarter, which
/// transforms a sample and a half a drawn pixel; its full decode, which the
/// same image takes under a soft mask, transforms twenty-four.
#[test]
fn a_jpeg_drawn_at_a_quarter_transforms_four_samples_a_pixel() {
    let jpeg = smooth_jpeg(512, 384);
    let keys = jpeg_keys(512, 384);
    let drawn_pixels = 128u64 * 96;
    let content = "q 128 0 0 96 0 0 cm /Im0 Do Q";

    let (bitmap, stats) = counted_at(jpeg_page((128.0, 96.0), content, &keys, &jpeg), 1.0);
    assert_eq!((bitmap.width, bitmap.height), (128, 96));
    assert!(stats.jpeg_samples_transformed > 0, "the decode was counted");
    assert!(
        stats.jpeg_samples_transformed <= 4 * drawn_pixels,
        "A7: {} samples transformed for {drawn_pixels} drawn pixels",
        stats.jpeg_samples_transformed
    );
    assert_eq!(
        stats.jpeg_samples_transformed,
        transformed_512_by_384(2),
        "two by two samples a block: a sample and a half a drawn pixel"
    );
    assert_eq!(stats.images_decoded, 1);

    let masked = format!("{keys} /SMask 6 0 R");
    let (_, full) = counted_at(jpeg_page((128.0, 96.0), content, &masked, &jpeg), 1.0);
    assert_eq!(
        full.jpeg_samples_transformed,
        transformed_512_by_384(8),
        "the full decode: twenty-four samples a drawn pixel"
    );
    assert_eq!(full.jpeg_samples_transformed, 24 * drawn_pixels);
}

/// **The scale is the smallest whose image covers the draw on both axes**:
/// drawn at its own size or a hair past half it is decoded whole; at a half,
/// a quarter and an eighth at those; smaller than an eighth at an eighth, the
/// smallest there is; at half its width and its whole height whole, since the
/// height needs every row. A quarter-turned and a flipped draw ask the length
/// of each of the image's own axes, and a render at twice the scale draws
/// twice the pixels and asks twice the size.
#[test]
fn a_jpeg_is_decoded_at_the_smallest_scale_that_covers_its_drawn_size() {
    let jpeg = smooth_jpeg(512, 384);
    let keys = jpeg_keys(512, 384);
    let transformed = |matrix: &str, scale: f64| {
        let content = format!("q {matrix} cm /Im0 Do Q");
        let bytes = jpeg_page((600.0, 600.0), &content, &keys, &jpeg);
        counted_at(bytes, scale).1.jpeg_samples_transformed
    };
    for (matrix, scale, n, what) in [
        ("512 0 0 384 0 0", 1.0, 8, "its own size"),
        ("257 0 0 192 0 0", 1.0, 8, "a hair past half its width"),
        ("256 0 0 192 0 0", 1.0, 4, "a half"),
        ("128 0 0 96 0 0", 1.0, 2, "a quarter"),
        ("64 0 0 48 0 0", 1.0, 1, "an eighth"),
        ("10 0 0 7 0 0", 1.0, 1, "smaller than an eighth"),
        (
            "256 0 0 384 0 0",
            1.0,
            8,
            "half its width and all its height",
        ),
        ("0 128 -96 0 96 0", 1.0, 2, "a quarter, quarter-turned"),
        (
            "-128 0 0 -96 128 96",
            1.0,
            2,
            "a quarter, flipped both ways",
        ),
        (
            "128 0 0 96 0 0",
            2.0,
            4,
            "a quarter drawn at twice the scale",
        ),
        (
            "64 0 0 48 0 0",
            4.0,
            4,
            "an eighth drawn at four times the scale",
        ),
    ] {
        assert_eq!(
            transformed(matrix, scale),
            transformed_512_by_384(n),
            "{what}: decoded at {n}/8"
        );
    }
}

/// **A mask is sampled against the grid it was drawn for**: a JPEG with a
/// soft mask, or with a `/Mask` of either form, is decoded at full size
/// however small it is drawn, and the same JPEG with neither is not.
#[test]
fn a_masked_jpeg_is_decoded_at_full_size() {
    let jpeg = smooth_jpeg(512, 384);
    let keys = jpeg_keys(512, 384);
    let content = "q 64 0 0 48 0 0 cm /Im0 Do Q";
    for (extra, n) in [
        ("", 1),
        ("/SMask 6 0 R", 8),
        ("/Mask [0 10 0 10 0 10]", 8),
        ("/Mask 6 0 R", 8),
    ] {
        let bytes = jpeg_page((64.0, 48.0), content, &format!("{keys} {extra}"), &jpeg);
        assert_eq!(
            counted_at(bytes, 1.0).1.jpeg_samples_transformed,
            transformed_512_by_384(n),
            "{extra:?}"
        );
    }
}

/// **An index is no colour**: a grey JPEG drawn at an eighth of its size is
/// decoded at an eighth, and the same JPEG under an `/Indexed` space
/// (8.6.6.3), whose samples select a palette's entries, is decoded whole — a
/// smaller scale averages two indices into a third neither sample chose — and
/// a later draw of it at its own size meets that decode.
#[test]
fn an_indexed_jpeg_is_decoded_at_full_size() {
    use tinker_pdf_filters::{JpegOptions, JpegSource, JpegSourceColour};

    // Checks of 64 by 48 samples, 0 and 200: the palette's red and its blue.
    let samples: Vec<u8> = (0..384u32)
        .flat_map(|y| (0..512u32).map(move |x| ((x / 64 + y / 48) % 2 * 200) as u8))
        .collect();
    let jpeg = tinker_pdf_filters::jpeg_encode(
        &JpegSource {
            width: 512,
            height: 384,
            colour: JpegSourceColour::Gray,
            stride: 512,
            data: &samples,
        },
        &JpegOptions::default(),
    )
    .expect("the raster encodes");
    // 64 by 48 blocks of one component.
    let blocks = 64 * 48;
    let content = "q 64 0 0 48 0 0 cm /Im0 Do Q";
    let grey = "/Width 512 /Height 384 /ColorSpace /DeviceGray /BitsPerComponent 8 \
                /Filter /DCTDecode";
    let (_, stats) = counted_at(jpeg_page((64.0, 48.0), content, grey, &jpeg), 1.0);
    assert_eq!(
        stats.jpeg_samples_transformed, blocks,
        "grey, at an eighth: one sample a block"
    );
    let indexed = format!(
        "/Width 512 /Height 384 /ColorSpace {} /BitsPerComponent 8 /Filter /DCTDecode",
        palette()
    );
    let (bitmap, stats) = counted_at(jpeg_page((64.0, 48.0), content, &indexed, &jpeg), 1.0);
    assert!(bitmap.warnings.is_empty(), "{:?}", bitmap.warnings);
    assert_eq!(
        stats.jpeg_samples_transformed,
        blocks * 64,
        "indices, whole: sixty-four samples a block"
    );
    let both = "q 64 0 0 48 0 0 cm /Im0 Do Q q 512 0 0 384 64 48 cm /Im0 Do Q";
    let (_, stats) = counted_at(jpeg_page((576.0, 432.0), both, &indexed, &jpeg), 1.0);
    assert_eq!(
        (
            stats.image_misses,
            stats.image_hits,
            stats.jpeg_samples_transformed
        ),
        (1, 1, blocks * 64),
        "the eighth's decode is the whole one's"
    );
}

/// **A reduced JPEG covers what the image covers.** An odd-sized JPEG, 333 by
/// 217, drawn at a quarter at a fractional offset is decoded at a quarter, 84
/// by 55 samples whose last column and row stand for less than four of the
/// image's: it paints exactly the pixels the same image decoded whole paints
/// — the grid is cut where the image ends, not stretched to the square's
/// edge — and close to the same colours, the reduced transform standing in
/// for the box filter.
#[test]
fn a_reduced_jpeg_covers_what_the_image_covers() {
    let jpeg = smooth_jpeg(333, 217);
    let keys = jpeg_keys(333, 217);
    let content = "q 83.25 0 0 54.25 3.4 5.7 cm /Im0 Do Q";
    let (reduced, stats) = counted_at(jpeg_page((95.0, 70.0), content, &keys, &jpeg), 1.0);
    // 21 by 14 MCUs of four luma blocks and one of each chroma, two by two
    // samples each.
    assert_eq!(stats.jpeg_samples_transformed, 21 * 14 * 6 * 4, "a quarter");
    let masked = format!("{keys} /Mask [0 0 0 0 0 0]");
    let (whole, stats) = counted_at(jpeg_page((95.0, 70.0), content, &masked, &jpeg), 1.0);
    assert_eq!(stats.jpeg_samples_transformed, 21 * 14 * 6 * 64, "whole");
    assert_eq!((reduced.width, reduced.height), (whole.width, whole.height));

    let (mut painted, mut differing, mut total) = (0usize, 0usize, 0u64);
    for y in 0..reduced.height {
        for x in 0..reduced.width {
            let (a, b) = (pixel(&reduced, x, y), pixel(&whole, x, y));
            let white = |p: (u8, u8, u8)| p == (255, 255, 255);
            assert_eq!(
                white(a),
                white(b),
                "({x}, {y}): the reduced grid covers {} what the image covers",
                if white(a) { "less than" } else { "more than" }
            );
            if !white(a) {
                painted += 1;
                let diff = u64::from(a.0.abs_diff(b.0))
                    + u64::from(a.1.abs_diff(b.1))
                    + u64::from(a.2.abs_diff(b.2));
                total += diff;
                differing += usize::from(diff > 0);
            }
        }
    }
    assert!(painted > 4_000, "the image covers {painted} pixels");
    assert!(
        total <= 3 * 6 * painted as u64,
        "the reduced decode is near the whole one: {total} levels over {painted} pixels, \
         {differing} differing"
    );
}

/// **Each scale's decode is kept apart.** A retained page drawn at 72 dpi
/// decodes its JPEG at a quarter; at four times the scale it draws it at its
/// own size and decodes it whole, a second miss; at 72 dpi again it meets the
/// quarter in the cache and transforms nothing, and draws the same bytes. One
/// page drawing the image at both sizes decodes it at both.
#[test]
fn each_scale_of_a_jpeg_is_cached_apart() {
    let jpeg = smooth_jpeg(512, 384);
    let keys = jpeg_keys(512, 384);
    let bytes = jpeg_page((128.0, 96.0), "q 128 0 0 96 0 0 cm /Im0 Do Q", &keys, &jpeg);
    let doc = Document::open(bytes).expect("it opens");
    let list = doc.page(0).expect("a page").display_list();
    let at = |scale: f64| {
        let bitmap = list.render(&RenderOptions {
            scale,
            stats: true,
            ..RenderOptions::default()
        });
        let stats = bitmap.stats.expect("counted");
        (bitmap, stats)
    };
    let (first, stats) = at(1.0);
    assert_eq!(
        (
            stats.image_misses,
            stats.image_hits,
            stats.jpeg_samples_transformed
        ),
        (1, 0, transformed_512_by_384(2)),
        "a quarter, decoded"
    );
    let (_, stats) = at(4.0);
    assert_eq!(
        (
            stats.image_misses,
            stats.image_hits,
            stats.jpeg_samples_transformed
        ),
        (1, 0, transformed_512_by_384(8)),
        "its own size, decoded whole: another scale's miss"
    );
    let (again, stats) = at(1.0);
    assert_eq!(
        (
            stats.image_misses,
            stats.image_hits,
            stats.jpeg_samples_transformed
        ),
        (0, 1, 0),
        "the quarter, found"
    );
    assert!(again.data == first.data, "a hit is what the miss drew");
    assert_eq!(again.warnings, first.warnings);

    let both = jpeg_page(
        (640.0, 480.0),
        "q 128 0 0 96 0 0 cm /Im0 Do Q q 512 0 0 384 128 96 cm /Im0 Do Q \
         q 128 0 0 96 0 384 cm /Im0 Do Q",
        &keys,
        &jpeg,
    );
    let (_, stats) = counted_at(both, 1.0);
    assert_eq!(
        (stats.image_misses, stats.image_hits),
        (2, 1),
        "a quarter, the whole image, then the quarter found"
    );
    assert_eq!(
        stats.jpeg_samples_transformed,
        transformed_512_by_384(2) + transformed_512_by_384(8)
    );
}

/// An inline JPEG is decoded at the size it is drawn too (8.9.7 gives it no
/// mask): a 128 by 96 4:2:0 image inline, drawn at a quarter, transforms a
/// sixteenth of what it transforms drawn whole.
#[test]
fn an_inline_jpeg_is_decoded_at_its_drawn_size() {
    let jpeg = smooth_jpeg(128, 96);
    let hex: String = jpeg.iter().map(|byte| format!("{byte:02X}")).collect();
    let drawn = |width: f64, height: f64| {
        let content = format!(
            "q {width} 0 0 {height} 0 0 cm BI /W 128 /H 96 /CS /RGB /BPC 8 \
             /F [/AHx /DCT] ID {hex}> EI Q"
        );
        let (bitmap, stats) = counted_at(page_with_content(content.as_bytes()), 1.0);
        assert!(bitmap.warnings.is_empty(), "{:?}", bitmap.warnings);
        stats.jpeg_samples_transformed
    };
    // 16 by 12 luma blocks and 8 by 6 of each chroma: 288 blocks.
    assert_eq!(drawn(128.0, 96.0), 288 * 64, "whole");
    assert_eq!(drawn(32.0, 24.0), 288 * 4, "a quarter");
}

/// Minimal base64 (RFC 4648 §4), to read an SVG's `data:` URI back.
fn unbase64(text: &str) -> Vec<u8> {
    let value = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("not base64: {c}"),
        }
    };
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let n = chunk
            .iter()
            .take(4 - pad)
            .fold(0u32, |n, &c| (n << 6) | value(c))
            << (6 * pad);
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&bytes[..3 - pad]);
    }
    out
}

/// **What is not a draw asks for the image whole**: a JPEG drawn at a quarter
/// is written into the page's SVG at its own 512 by 384, listed by
/// `Page::images` at that size with every sample, and a render of the page
/// that met it first changes neither.
#[test]
fn the_svg_writer_and_the_image_list_take_a_jpeg_whole() {
    let jpeg = smooth_jpeg(512, 384);
    let keys = jpeg_keys(512, 384);
    let bytes = jpeg_page((128.0, 96.0), "q 128 0 0 96 0 0 cm /Im0 Do Q", &keys, &jpeg);
    let doc = Document::open(bytes).expect("it opens");
    let page = doc.page(0).expect("a page");
    let list = page.display_list();
    let _ = list.render(&RenderOptions::default());

    for svg in [
        page.to_svg(&tinker_pdf::SvgOptions::default()),
        list.to_svg(&tinker_pdf::SvgOptions::default()),
    ] {
        let start = svg
            .markup
            .find("data:image/png;base64,")
            .expect("the image is written as a PNG");
        let data = &svg.markup[start + "data:image/png;base64,".len()..];
        let end = data.find('"').expect("the URI ends");
        let png = tinker_pdf::Bitmap::from_png(&unbase64(&data[..end])).expect("it decodes");
        assert_eq!((png.width, png.height), (512, 384), "written whole");
    }

    let images = page.images();
    let image = images.first().expect("the page lists its image");
    assert_eq!((image.width, image.height), (512, 384));
    assert_eq!(image.samples.len(), 512 * 384 * 3, "every sample");
}
