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

/// A one-page document whose content stream is `content`.
fn page_with_content(content: &[u8]) -> Vec<u8> {
    let mut file = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 20 20]\n\
   /Resources << >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n",
        content.len()
    )
    .into_bytes();
    file.extend_from_slice(content);
    file.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Size 5 /Root 1 0 R >>\n%%EOF\n");
    file
}

fn render(pdf: Vec<u8>) -> tinker_pdf::Bitmap {
    Document::open(pdf)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// The first image the first page draws, as `Page::images` decodes it.
fn first_image(pdf: Vec<u8>) -> tinker_pdf::PageImage {
    Document::open(pdf)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .images()
        .first()
        .expect("an image")
        .clone()
}

/// A spot colour whose tints run from white to red (8.6.6.4).
const SPOT: &str = "[/Separation /Spot /DeviceRGB << /FunctionType 2 /Domain [0 1] \
                    /C0 [1 1 1] /C1 [1 0 0] /N 1 >>]";

/// A CIE L*a*b* space, which reads no component as a device space does
/// (8.6.5.4).
const LAB: &str = "[/Lab << /WhitePoint [0.9505 1 1.089] /Range [-100 100 -100 100] >>]";

/// A palette of 256 entries, red below index 128 and blue from it (8.6.6.3).
fn palette() -> String {
    let entries: String = (0..256)
        .map(|index| if index < 128 { "FF0000" } else { "0000FF" })
        .collect();
    format!("[/Indexed /DeviceRGB 255 <{entries}>]")
}

/// 7.4.9: a `/ColorSpace` in the image dictionary overrides the codestream's
/// own colour specification, so the grey ramp drawn under a spot colour or a
/// palette, and the colour image under `/Lab`, are drawn in those spaces.
///
/// Each is held to its twin: the codestream's own samples, as `Page::images`
/// lists them, written as a plain sampled image at the codestream's size and
/// in the same space, which must draw the same bytes. And each must differ
/// from the codestream drawn in its own colour, or the twins agreeing would
/// prove nothing.
#[test]
fn a_jpx_image_draws_in_the_space_its_dictionary_names() {
    let palette = palette();
    for (fixture, space) in [
        ("r1-2.jp2", SPOT),
        ("r1-2.jp2", &*palette),
        ("c1-2.jp2", LAB),
    ] {
        let own = page_with_jpx(&codestream(fixture), "");
        let image = first_image(own.clone());
        assert_eq!(image.refused, None, "{fixture} decodes");
        let drawn = render(page_with_jpx(
            &codestream(fixture),
            &format!(" /ColorSpace {space}"),
        ));
        let twin = render(page_with_image(
            &format!(
                "/Width {} /Height {} /ColorSpace {space} /BitsPerComponent {}",
                image.width, image.height, image.bits_per_component
            ),
            &image.samples,
        ));

        assert_eq!(
            drawn.data, twin.data,
            "{fixture} under {space}: the codestream drew otherwise than its own samples \
             draw in that space"
        );
        assert_ne!(
            drawn.data,
            render(own).data,
            "{fixture} under {space}: drawn in the codestream's own colour"
        );
        assert!(
            drawn.warnings.is_empty(),
            "{fixture} under {space}: {:?}",
            drawn.warnings
        );
    }
}

/// An inline JPEG 2000 image goes through the same entry point, its space
/// read as an inline image's is (8.9.7).
#[test]
fn an_inline_jpx_image_draws_in_the_space_its_dictionary_names() {
    let inline = |filter: &str, data: &[u8]| {
        let hex: String = data.iter().map(|byte| format!("{byte:02X}")).collect();
        page_with_content(
            format!(
                "q 20 0 0 20 0 0 cm BI /W 32 /H 24 /CS {} /BPC 8 /F {filter} ID {hex}> EI Q",
                palette()
            )
            .as_bytes(),
        )
    };
    // 8.9.7 abbreviates no JPXDecode, so the filter is written in full.
    let jpx = inline("[/AHx /JPXDecode]", &codestream("r1-2.jp2"));
    let image = first_image(jpx.clone());
    assert_eq!(image.refused, None, "the inline codestream decodes");
    assert_eq!(image.samples.len(), 32 * 24, "the ramp's own samples");
    let drawn = render(jpx);
    let twin = render(inline("/AHx", &image.samples));

    assert_eq!(
        drawn.data, twin.data,
        "the inline codestream drew otherwise than its samples"
    );
    assert!(drawn.warnings.is_empty(), "{:?}", drawn.warnings);
}

/// A space of another number of components than the codestream gives cannot
/// hold its samples: it is refused and named under the image's name, and the
/// codestream drawn in its own colour (ruling 10).
#[test]
fn a_jpx_image_whose_space_cannot_hold_its_samples_keeps_its_own_colour() {
    let refused = render(page_with_jpx(
        &codestream("c1-2.jp2"),
        " /ColorSpace /DeviceGray",
    ));

    assert_eq!(
        refused.data,
        render(page_with_jpx(&codestream("c1-2.jp2"), "")).data,
        "drawn in the codestream's own colour"
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
