//! Milestone 7: a JPEG 2000 image on a page (8.9.5.4).
//!
//! The clause is mostly about which of the image dictionary's claims survive
//! a codestream that describes itself, so most of these assert *which of two
//! disagreeing sources won* rather than that something drew. A test that
//! renders a JPX image and checks it is not blank would have passed against
//! every defect the last four milestones found -- including one that halved
//! every coefficient and looked exactly like a decode.

use tinker_pdf::{Document, RenderOptions};

/// A minimal one-page document wrapping a JPX codestream as an image XObject.
fn page_with_jpx(codestream: &[u8], extra: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    let mut offsets = Vec::new();

    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &[u8]| {
        offsets.push(out.len());
        let n = offsets.len();
        out.extend_from_slice(format!("{n} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };

    obj(&mut out, &mut offsets, b"<< /Type /Catalog /Pages 2 0 R >>");
    obj(
        &mut out,
        &mut offsets,
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
    );
    obj(
        &mut out,
        &mut offsets,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n   /Resources << /XObject << /Im0 5 0 R >> >>\n   /Contents 4 0 R >>",
    );
    let content = b"q 20 0 0 20 0 0 cm /Im0 Do Q";
    obj(
        &mut out,
        &mut offsets,
        format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            String::from_utf8_lossy(content)
        )
        .as_bytes(),
    );

    offsets.push(out.len());
    out.extend_from_slice(b"5 0 obj\n");
    out.extend_from_slice(
        format!(
            "<< /Type /XObject /Subtype /Image /Width 16 /Height 16 \
             /Filter /JPXDecode{extra} /Length {} >>\nstream\n",
            codestream.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(codestream);
    out.extend_from_slice(b"\nendstream\nendobj\n");

    let start = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", offsets.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for at in &offsets {
        out.extend_from_slice(format!("{at:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
            offsets.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn codestream(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tinker-pdf-filters/tests/jpx"
        ))
        .join(name),
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// A JPX image reaches the page, and the pixels are the codestream's.
///
/// Asserted by position against the source image rather than by "something
/// drew": the greyscale fixture is a known ramp, so a decode at the wrong
/// scale or offset -- the defect milestone 4 found, which looked entirely
/// like a picture -- moves specific pixels by a specific amount.
#[test]
fn a_jpx_image_draws_the_samples_its_codestream_holds() {
    let pdf = page_with_jpx(&codestream("r1-2.jp2"), "");
    let doc = Document::open(pdf).expect("it opens");
    let page = doc.page(0).expect("a page");
    let bitmap = page.render(&RenderOptions::default());

    assert!(
        !bitmap
            .warnings
            .iter()
            .any(|w| matches!(w, tinker_pdf::RenderWarning::UnsupportedImage { .. })),
        "a JPX image is no longer an unsupported codec: {:?}",
        bitmap.warnings
    );

    // **Absolute levels, against the codestream's own source.** An earlier
    // draft asserted only that ink appeared, and an injection that shifted
    // every sample down four bits passed it -- along with every other test
    // here, because the comparative ones move both sides equally. That is the
    // same shape milestone 4 hit (a decode at half amplitude looks like a
    // decode) and milestone 6 hit again (an f64 reference sharing a stage
    // with the code it checks inherits its error).
    //
    // The instrument is the distribution rather than pixel correspondence.
    // The image is 32 by 24 drawn into a 20 by 20 square, so which source
    // sample lands under which device pixel is a question about the
    // resampler, and answering it here would test that instead. A uniform
    // scaling -- which is what every defect of this class produces -- moves
    // the mean and the range together, and neither depends on the mapping.
    let source = std::fs::read(
        std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tinker-pdf-filters/tests/jpx"
        ))
        .join("r1.pgm"),
    )
    .expect("the source the fixture was made from");
    let pixels = &source[source.len() - 32 * 24..];

    let want_mean = pixels.iter().map(|&p| u32::from(p)).sum::<u32>() / (pixels.len() as u32);
    let drawn: Vec<u8> = bitmap.data.chunks(3).map(|p| p[0]).collect();
    let got_mean = drawn.iter().map(|&p| u32::from(p)).sum::<u32>() / (drawn.len() as u32);

    assert!(
        got_mean.abs_diff(want_mean) <= 12,
        "the page averages {got_mean} where the codestream's source averages          {want_mean}; a gap this size is a scale or an offset applied to every          sample, not resampling"
    );

    let want_range = u32::from(*pixels.iter().max().expect("non-empty"))
        - u32::from(*pixels.iter().min().expect("non-empty"));
    let got_range = u32::from(*drawn.iter().max().expect("non-empty"))
        - u32::from(*drawn.iter().min().expect("non-empty"));
    assert!(
        got_range * 2 >= want_range,
        "the page spans {got_range} levels where the source spans          {want_range}; the contrast has been compressed, which is what a          precision shift does and what a halved coefficient does"
    );
}

/// 8.9.5.4: `/BitsPerComponent` is ignored for JPXDecode.
///
/// The codestream is eight bits and the dictionary claims four. A build that
/// honoured the dictionary would shift every sample and produce a picture at
/// a sixteenth of its range -- which is to say, a picture.
#[test]
fn bits_per_component_is_ignored_and_the_codestream_wins() {
    let honest = page_with_jpx(&codestream("r1-2.jp2"), "");
    let lying = page_with_jpx(&codestream("r1-2.jp2"), " /BitsPerComponent 4");

    let render = |pdf: Vec<u8>| {
        Document::open(pdf)
            .expect("opens")
            .page(0)
            .expect("a page")
            .render(&RenderOptions::default())
            .data
    };

    assert_eq!(
        render(honest),
        render(lying),
        "the dictionary's /BitsPerComponent changed the pixels, and 8.9.5.4 \
         says the codestream's precision is the only one that counts"
    );
}

/// 8.9.5.4: `/Decode` is ignored for JPXDecode.
///
/// Every other image filter here honours it, which is exactly why this needs
/// its own assertion -- the generic sample path applies `/Decode` and a JPX
/// image must not reach it.
#[test]
fn decode_is_ignored_for_a_jpx_image() {
    let plain = page_with_jpx(&codestream("r1-2.jp2"), "");
    let inverted = page_with_jpx(&codestream("r1-2.jp2"), " /Decode [1 0]");

    let render = |pdf: Vec<u8>| {
        Document::open(pdf)
            .expect("opens")
            .page(0)
            .expect("a page")
            .render(&RenderOptions::default())
            .data
    };

    assert_eq!(
        render(plain),
        render(inverted),
        "/Decode [1 0] inverted a JPX image, and 8.9.5.4 says it is ignored"
    );
}

/// 8.9.5.4: `/ImageMask` shall not be combined with JPXDecode.
///
/// Refused by name rather than painted as a stencil of whatever the
/// codestream held -- a stencil is one bit per sample with no colour space,
/// and a JPX codestream is neither.
#[test]
fn image_mask_with_jpx_is_refused_rather_than_stencilled() {
    let pdf = page_with_jpx(&codestream("r1-2.jp2"), " /ImageMask true");
    let doc = Document::open(pdf).expect("it opens");
    let page = doc.page(0).expect("a page");
    let bitmap = page.render(&RenderOptions::default());

    assert!(
        bitmap
            .warnings
            .iter()
            .any(|w| matches!(w, tinker_pdf::RenderWarning::UnsupportedImage { .. })),
        "the combination 8.9.5.4 forbids drew something instead of reporting: {:?}",
        bitmap.warnings
    );
}
