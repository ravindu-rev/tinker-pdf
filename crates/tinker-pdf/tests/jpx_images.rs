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

// ---- The levels a page asks for (P-10.8) -------------------------------------
//
// A JPEG 2000 codestream is decoded with the most resolution levels dropped
// whose image still has as many samples as the draw has pixels each way
// (`docs/design/jpx-speed.md`, "The page's request"), and what each decode did
// is counted: the levels it synthesised, the code-blocks it decoded and the
// levels it dropped (`Stats::jpx_levels_synthesised`,
// `Stats::jpx_code_blocks_decoded`, `Stats::jpx_levels_discarded`).

/// One of the benchmark set's committed codestreams (`benches/data/README.md`):
/// `irreversible-620x877.jp2` is the irreversible 9/7 with the ICT, one tile of
/// three components, five decomposition levels and 64 by 64 code-blocks.
fn committed(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/benches/data")).join(name),
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// A page of `page` points drawing `content`, whose `/Im0` is the codestream
/// `jpx` with `keys` in its dictionary beside `/Subtype /Image`, and whose
/// `/Mk` is a 4 by 3 grey image a soft mask can name as `6 0 R`.
fn jpx_page(page: (f64, f64), content: &str, keys: &str, jpx: &[u8]) -> Vec<u8> {
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
            jpx.len()
        )
        .as_bytes(),
    );
    file.extend_from_slice(jpx);
    file.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n");
    file
}

/// The dictionary keys a codestream of `width` by `height` samples is drawn
/// with: no `/ColorSpace`, so its own colour applies (8.9.5.4).
fn jpx_keys(width: u32, height: u32) -> String {
    format!("/Width {width} /Height {height} /Filter /JPXDecode")
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

/// The levels a render's JPEG 2000 decodes dropped and synthesised.
fn levels(stats: &tinker_pdf::Stats) -> (u64, u64) {
    (stats.jpx_levels_discarded, stats.jpx_levels_synthesised)
}

/// **A7, the JPX half: a JPEG 2000 image drawn at a quarter of its size
/// decodes two levels fewer** (`docs/design/jpx-speed.md`, "Guard A7's JPX
/// half"), read from the render's counted work. The 620 by 877 codestream has
/// five decomposition levels in one tile of three components; drawn over 155
/// by 219 pixels, a quarter of its width and the floor of a quarter of its
/// height, it is decoded with two levels dropped, three synthesised a
/// tile-component where its full decode — which the same image takes under a
/// soft mask — synthesises five, and its tier-1 decodes the code-blocks of the
/// resolutions it keeps and no more: exactly what the decoder counts for the
/// same request, and fewer than the full decode's.
#[test]
fn a_jpx_image_drawn_at_a_quarter_decodes_two_levels_fewer() {
    let jpx = committed("irreversible-620x877.jp2");
    let keys = jpx_keys(620, 877);
    let content = "q 155 0 0 219 0 0 cm /Im0 Do Q";
    let (bitmap, stats) = counted_at(jpx_page((155.0, 219.0), content, &keys, &jpx), 1.0);
    assert_eq!((bitmap.width, bitmap.height), (155, 219));
    assert!(bitmap.warnings.is_empty(), "{:?}", bitmap.warnings);
    assert_eq!(stats.images_decoded, 1);
    assert_eq!(
        levels(&stats),
        (2, 3 * (5 - 2)),
        "A7: two levels dropped, three synthesised in each of three tile-components"
    );

    let (_, counts) = tinker_pdf_filters::jpx_decode_request(
        &jpx,
        &tinker_pdf_filters::Limits::new(1 << 28),
        &tinker_pdf_filters::JpxRequest {
            discard: 2,
            window: None,
        },
        &mut Vec::new(),
    )
    .expect("the codestream decodes");
    assert_eq!(
        (stats.jpx_levels_synthesised, stats.jpx_code_blocks_decoded),
        (counts.levels_synthesised, counts.code_blocks_decoded),
        "the render counts what the decode did"
    );

    let masked = format!("{keys} /SMask 6 0 R");
    let (_, full) = counted_at(jpx_page((155.0, 219.0), content, &masked, &jpx), 1.0);
    assert_eq!(
        levels(&full),
        (0, 3 * 5),
        "the full decode: every level of every tile-component"
    );
    assert!(
        stats.jpx_code_blocks_decoded < full.jpx_code_blocks_decoded,
        "{} code-blocks decoded reduced, {} whole",
        stats.jpx_code_blocks_decoded,
        full.jpx_code_blocks_decoded
    );
}

/// **The discard is the most levels whose image covers the draw on both
/// axes**: `W >> d` by `H >> d` at least the drawn pixels, so the reduced
/// grid is never drawn magnified. The decoder's own 32 by 24 colour fixture
/// of two levels is decoded whole at its own size and a hair past half its
/// width, with one level dropped at a half, two at a quarter, and at a tenth
/// still two, all it has; at half its width and its whole height whole, since
/// the height needs every row. A quarter-turned and a flipped draw ask the
/// length of each of the image's own axes, and a render at twice or four
/// times the scale draws that many more pixels and drops that many fewer
/// levels. The five-level codestream drops three, four and five at an
/// eighth, a sixteenth and a thirty-second of each side, floored, and five
/// at a pixel.
#[test]
fn a_jpx_image_is_decoded_with_the_most_levels_dropped_that_cover_its_drawn_size() {
    let small = codestream("c1-3.jp2");
    let large = committed("irreversible-620x877.jp2");
    let decoded = |jpx: &[u8], keys: &str, matrix: &str, scale: f64| {
        let content = format!("q {matrix} cm /Im0 Do Q");
        let bytes = jpx_page((64.0, 64.0), &content, keys, jpx);
        let (bitmap, stats) = counted_at(bytes, scale);
        assert!(
            bitmap.warnings.is_empty(),
            "{matrix}: {:?}",
            bitmap.warnings
        );
        assert_eq!(stats.images_decoded, 1, "{matrix}");
        levels(&stats)
    };
    let keys = jpx_keys(32, 24);
    for (matrix, scale, d, what) in [
        ("32 0 0 24 0 0", 1.0, 0, "its own size"),
        ("17 0 0 12 0 0", 1.0, 0, "a hair past half its width"),
        ("16 0 0 12 0 0", 1.0, 1, "a half"),
        ("8 0 0 6 0 0", 1.0, 2, "a quarter"),
        ("3.2 0 0 2.4 0 0", 1.0, 2, "a tenth, two being all it has"),
        ("16 0 0 24 0 0", 1.0, 0, "half its width and all its height"),
        ("0 8 -6 0 6 0", 1.0, 2, "a quarter, quarter-turned"),
        ("-8 0 0 -6 8 6", 1.0, 2, "a quarter, flipped both ways"),
        ("8 0 0 6 0 0", 2.0, 1, "a quarter drawn at twice the scale"),
        (
            "8 0 0 6 0 0",
            4.0,
            0,
            "a quarter drawn at four times the scale",
        ),
    ] {
        assert_eq!(
            decoded(&small, &keys, matrix, scale),
            (d, 3 * (2 - d)),
            "{what}: {d} of two levels dropped, in three tile-components"
        );
    }
    let keys = jpx_keys(620, 877);
    for (matrix, d, what) in [
        ("77 0 0 109 0 0", 3, "an eighth"),
        ("38 0 0 54 0 0", 4, "a sixteenth"),
        ("19 0 0 27 0 0", 5, "a thirty-second"),
        ("1 0 0 1 0 0", 5, "a pixel, five being all it has"),
    ] {
        assert_eq!(
            decoded(&large, &keys, matrix, 1.0),
            (d, 3 * (5 - d)),
            "{what}: {d} of five levels dropped, in three tile-components"
        );
    }
}

/// **A mask is sampled against the grid it was drawn for**: a codestream
/// with a soft mask, or a `/Mask` of either form, is decoded whole however
/// small it is drawn, and the same codestream with neither is not.
#[test]
fn a_masked_jpx_image_is_decoded_whole() {
    let jpx = codestream("c1-3.jp2");
    let keys = jpx_keys(32, 24);
    let content = "q 8 0 0 6 0 0 cm /Im0 Do Q";
    for (extra, d) in [
        ("", 2),
        ("/SMask 6 0 R", 0),
        ("/Mask [0 10 0 10 0 10]", 0),
        ("/Mask 6 0 R", 0),
    ] {
        let bytes = jpx_page((8.0, 6.0), content, &format!("{keys} {extra}"), &jpx);
        assert_eq!(
            levels(&counted_at(bytes, 1.0).1),
            (d, 3 * (2 - d)),
            "{extra:?}"
        );
    }
}

/// `file`'s `jp2c` box's contents: the codestream a JP2 file carries
/// (T.800 I.5.4), its box the file's last.
fn jp2c(file: &[u8]) -> &[u8] {
    let at = file
        .windows(4)
        .position(|w| w == b"jp2c")
        .expect("a jp2c box");
    &file[at + 4..]
}

/// One JP2 box: `LBox`, `TBox`, contents (T.800 I.4).
fn jp2_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = u32::try_from(body.len() + 8)
        .expect("a small box")
        .to_be_bytes()
        .to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out
}

/// `codestream`, one component of `width` by `height` eight-bit samples, in
/// JP2 boxes whose `pclr` maps it through a palette of 256 entries in three
/// columns (T.800 I.5.3.4, I.5.3.5), entry `i` being `(i, 255 - i, i / 2)`.
fn through_a_palette(codestream: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut pclr = vec![0x01, 0x00, 3, 7, 7, 7]; // NE = 256, NPC = 3, 8-bit unsigned
    for i in 0..=255u8 {
        pclr.extend_from_slice(&[i, 255 - i, i / 2]);
    }
    let cmap: Vec<u8> = (0..3u8).flat_map(|column| [0, 0, 1, column]).collect();
    let mut ihdr = height.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&[0, 1, 7, 7, 0, 0]);
    let mut header = jp2_box(b"ihdr", &ihdr);
    header.extend(jp2_box(b"colr", &[1, 0, 0, 0, 0, 0, 16])); // EnumCS 16, sRGB
    header.extend(jp2_box(b"pclr", &pclr));
    header.extend(jp2_box(b"cmap", &cmap));
    let mut file = vec![0, 0, 0, 12, b'j', b'P', b' ', b' ', 0x0D, 0x0A, 0x87, 0x0A];
    file.extend(jp2_box(b"ftyp", b"jp2 \0\0\0\0jp2 "));
    file.extend(jp2_box(b"jp2h", &header));
    file.extend(jp2_box(b"jp2c", codestream));
    file
}

/// **An index is no colour.** A lower resolution of a grey codestream's
/// samples is their average, which is a grey; of a palette's indices, an
/// index neither sample chose. So the decoder's 32 by 24 grey fixture of two
/// levels drawn at a quarter drops both — and drawn under an `/Indexed`
/// space (8.6.6.3), or with its codestream read through a `pclr` palette, it
/// is decoded whole, and every later draw of it is too: one page drawing it
/// at a quarter and whole decodes it once.
#[test]
fn a_jpx_image_whose_samples_are_indices_is_decoded_whole() {
    let grey = codestream("r1-3.jp2");
    let content = "q 8 0 0 6 0 0 cm /Im0 Do Q";
    let keys = jpx_keys(32, 24);
    let (bitmap, stats) = counted_at(jpx_page((8.0, 6.0), content, &keys, &grey), 1.0);
    assert!(bitmap.warnings.is_empty(), "{:?}", bitmap.warnings);
    assert_eq!(
        levels(&stats),
        (2, 0),
        "grey levels average into grey levels"
    );

    let indexed = format!("{keys} /ColorSpace {}", palette());
    let palette_read = through_a_palette(jp2c(&grey), 32, 24);
    for (what, keys, jpx) in [
        ("an /Indexed space", indexed.as_str(), grey.as_slice()),
        ("a pclr palette", keys.as_str(), palette_read.as_slice()),
    ] {
        let (bitmap, stats) = counted_at(jpx_page((8.0, 6.0), content, keys, jpx), 1.0);
        assert!(bitmap.warnings.is_empty(), "{what}: {:?}", bitmap.warnings);
        assert_eq!(levels(&stats), (0, 2), "{what}: decoded whole");
        let both = "q 8 0 0 6 0 0 cm /Im0 Do Q q 32 0 0 24 8 6 cm /Im0 Do Q";
        let (_, stats) = counted_at(jpx_page((40.0, 30.0), both, keys, jpx), 1.0);
        assert_eq!(
            (stats.image_misses, stats.image_hits, stats.images_decoded),
            (1, 1, 1),
            "{what}: the quarter's decode is the whole one's"
        );
    }
}

/// The page's pixels, rendered over white and over black: a pixel the image
/// covers wholly is the same over both, one it does not reach at all is the
/// page's white over the first and black over the second.
fn over_white_and_black(
    page: (f64, f64),
    content: &str,
    keys: &str,
    jpx: &[u8],
) -> (tinker_pdf::Bitmap, tinker_pdf::Bitmap) {
    let white = render(jpx_page(page, content, keys, jpx));
    let black = render(jpx_page(
        page,
        &format!("0 g 0 0 {} {} re f {content}", page.0, page.1),
        keys,
        jpx,
    ));
    (white, black)
}

/// **A reduced image covers what the image covers.** The 620 by 877
/// codestream drawn at a quarter at a fractional offset is decoded with two
/// levels dropped, 155 by 220 samples whose last row stands for less than
/// four of the image's: it covers exactly the pixels the same image decoded
/// whole covers, wholly where that covers wholly — the grid is cut where the
/// image ends, not stretched to the square's edge — and close to the same
/// colours, the wavelet's low band standing in for the box filter. A quarter
/// of its odd height exactly drops a level fewer.
#[test]
fn a_reduced_jpx_image_covers_what_the_image_covers() {
    let jpx = committed("irreversible-620x877.jp2");
    let keys = jpx_keys(620, 877);
    let page = (165.0, 230.0);
    let exact = "q 155 0 0 219.25 3.4 5.7 cm /Im0 Do Q";
    let (_, stats) = counted_at(jpx_page(page, exact, &keys, &jpx), 1.0);
    assert_eq!(
        levels(&stats).0,
        1,
        "a quarter of an odd side drops one level"
    );
    let content = "q 155 0 0 219 3.4 5.7 cm /Im0 Do Q";
    let (_, stats) = counted_at(jpx_page(page, content, &keys, &jpx), 1.0);
    assert_eq!(levels(&stats).0, 2, "and its floor two");

    let masked = format!("{keys} /Mask [0 0 0 0 0 0]");
    let (_, whole) = counted_at(jpx_page(page, content, &masked, &jpx), 1.0);
    assert_eq!(levels(&whole).0, 0, "the mask keeps it whole");
    let (reduced_white, reduced_black) = over_white_and_black(page, content, &keys, &jpx);
    let (whole_white, whole_black) = over_white_and_black(page, content, &masked, &jpx);
    let pixel = |bitmap: &tinker_pdf::Bitmap, at: usize| {
        (bitmap.data[at], bitmap.data[at + 1], bitmap.data[at + 2])
    };
    let untouched = |w: (u8, u8, u8), b: (u8, u8, u8)| w == (255, 255, 255) && b == (0, 0, 0);
    let (mut covered, mut total) = (0usize, 0u64);
    for at in (0..reduced_white.data.len()).step_by(3) {
        let (rw, rb) = (pixel(&reduced_white, at), pixel(&reduced_black, at));
        let (ww, wb) = (pixel(&whole_white, at), pixel(&whole_black, at));
        assert_eq!(
            untouched(rw, rb),
            untouched(ww, wb),
            "pixel {}: the reduced grid reaches what the image reaches",
            at / 3
        );
        assert_eq!(
            rw == rb,
            ww == wb,
            "pixel {}: and covers wholly what the image covers wholly",
            at / 3
        );
        if rw == rb {
            covered += 1;
            total += u64::from(rw.0.abs_diff(ww.0))
                + u64::from(rw.1.abs_diff(ww.1))
                + u64::from(rw.2.abs_diff(ww.2));
        }
    }
    assert!(covered > 30_000, "the image covers {covered} pixels wholly");
    assert!(
        total <= 3 * 12 * covered as u64,
        "the reduced decode is near the whole one: {total} levels over {covered} pixels"
    );
}

/// **Each discard's decode is kept apart.** A retained page drawn at 72 dpi
/// decodes its codestream with two levels dropped; at four times the scale it
/// draws it at its own size and decodes it whole, a second miss; at 72 dpi
/// again it meets the reduced decode in the cache, decodes nothing and draws
/// the same bytes. One page drawing the image at both sizes decodes it at
/// both.
#[test]
fn each_discard_of_a_jpx_image_is_cached_apart() {
    let jpx = codestream("c1-3.jp2");
    let keys = jpx_keys(32, 24);
    let bytes = jpx_page((8.0, 6.0), "q 8 0 0 6 0 0 cm /Im0 Do Q", &keys, &jpx);
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
        (stats.image_misses, stats.image_hits, levels(&stats)),
        (1, 0, (2, 0)),
        "a quarter, decoded"
    );
    let (_, stats) = at(4.0);
    assert_eq!(
        (stats.image_misses, stats.image_hits, levels(&stats)),
        (1, 0, (0, 6)),
        "its own size, decoded whole: another discard's miss"
    );
    let (again, stats) = at(1.0);
    assert_eq!(
        (stats.image_misses, stats.image_hits, levels(&stats)),
        (0, 1, (0, 0)),
        "the quarter, found"
    );
    assert!(again.data == first.data, "a hit is what the miss drew");
    assert_eq!(again.warnings, first.warnings);

    let both = jpx_page(
        (72.0, 54.0),
        "q 8 0 0 6 0 0 cm /Im0 Do Q q 32 0 0 24 8 6 cm /Im0 Do Q q 8 0 0 6 40 30 cm /Im0 Do Q",
        &keys,
        &jpx,
    );
    let (_, stats) = counted_at(both, 1.0);
    assert_eq!(
        (stats.image_misses, stats.image_hits),
        (2, 1),
        "a quarter, the whole image, then the quarter found"
    );
    assert_eq!(levels(&stats), (2, 6), "two dropped once, none once");
}

/// An inline codestream is decoded at the size it is drawn too: 8.9.7 gives
/// an inline image no mask, so drawn at a quarter it drops both its levels
/// and drawn whole none.
#[test]
fn an_inline_jpx_image_is_decoded_at_its_drawn_size() {
    let hex: String = codestream("c1-3.jp2")
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    let drawn = |width: f64, height: f64| {
        let content = format!(
            "q {width} 0 0 {height} 0 0 cm BI /W 32 /H 24 /BPC 8 /F [/AHx /JPXDecode] \
             ID {hex}> EI Q"
        );
        let (bitmap, stats) = counted_at(page_with_content(content.as_bytes()), 1.0);
        assert!(bitmap.warnings.is_empty(), "{:?}", bitmap.warnings);
        levels(&stats)
    };
    assert_eq!(drawn(32.0, 24.0), (0, 6), "whole");
    assert_eq!(drawn(8.0, 6.0), (2, 0), "a quarter");
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

/// **What is not a draw asks for the image whole**: a codestream drawn at a
/// quarter is written into the page's SVG at its own 32 by 24, listed by
/// `Page::images` at that size with every sample, and a render of the page
/// that met it first changes neither.
#[test]
fn the_svg_writer_and_the_image_list_take_a_jpx_image_whole() {
    let jpx = codestream("c1-3.jp2");
    let keys = jpx_keys(32, 24);
    let bytes = jpx_page((8.0, 6.0), "q 8 0 0 6 0 0 cm /Im0 Do Q", &keys, &jpx);
    let doc = Document::open(bytes).expect("it opens");
    let page = doc.page(0).expect("a page");
    let list = page.display_list();
    let rendered = list.render(&RenderOptions {
        stats: true,
        ..RenderOptions::default()
    });
    let stats = rendered.stats.expect("counted");
    assert_eq!(levels(&stats).0, 2, "the render met it reduced");

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
        assert_eq!((png.width, png.height), (32, 24), "written whole");
    }

    let images = page.images();
    let image = images.first().expect("the page lists its image");
    assert_eq!((image.width, image.height), (32, 24));
    assert_eq!(image.samples.len(), 32 * 24 * 3, "every sample");
}

// ---- The window a page asks for (P-10.9) -------------------------------------
//
// A JPEG 2000 codestream is decoded over the part of it a draw can show: the
// canvas a tile or a band stands for, and the clip, widened by the sampler's
// reach (`docs/design/jpx-speed.md`, "The visible window"), so a region's
// render decodes the code-blocks its window reaches and no others.

/// The code-blocks tier-1 decodes for the 620 by 877 codestream with two
/// levels dropped, over `window` of the 155 by 220 image that leaves, or all
/// of it.
fn blocks_for(jpx: &[u8], window: Option<(u32, u32, u32, u32)>) -> u64 {
    let (_, counts) = tinker_pdf_filters::jpx_decode_request(
        jpx,
        &tinker_pdf_filters::Limits::new(1 << 28),
        &tinker_pdf_filters::JpxRequest {
            discard: 2,
            window: window.map(|(x, y, width, height)| tinker_pdf_filters::JpxWindow {
                x,
                y,
                width,
                height,
            }),
        },
        &mut Vec::new(),
    )
    .expect("the codestream decodes");
    counts.code_blocks_decoded
}

/// **A region's render decodes only the code-blocks its window reaches**:
/// the 620 by 877 codestream drawn at a quarter over its 155 by 219 page, two
/// levels dropped, decodes every block of the four resolutions it keeps for
/// the whole page — exactly the decoder's count with no window — and for the
/// page's top-left corner of 40 by 40 pixels at least the blocks the bare
/// corner reaches, 40 by 41 samples, no more than a corner of 64 by 64
/// reaches — the sampler's reach is a few samples — and fewer than the whole.
#[test]
fn a_region_render_decodes_only_the_code_blocks_its_window_reaches() {
    let jpx = committed("irreversible-620x877.jp2");
    let keys = jpx_keys(620, 877);
    let bytes = jpx_page(
        (155.0, 219.0),
        "q 155 0 0 219 0 0 cm /Im0 Do Q",
        &keys,
        &jpx,
    );
    let doc = Document::open(bytes).expect("it opens");
    let page = doc.page(0).expect("a page");
    let counted = |region: Option<tinker_pdf::PixelRegion>| {
        let bitmap = page.render(&RenderOptions {
            region,
            stats: true,
            ..RenderOptions::default()
        });
        assert!(bitmap.warnings.is_empty(), "{:?}", bitmap.warnings);
        bitmap.stats.expect("counted").jpx_code_blocks_decoded
    };
    let whole = counted(None);
    assert_eq!(whole, blocks_for(&jpx, None), "the page holds all of it");
    let corner = counted(Some(tinker_pdf::PixelRegion::new(0, 0, 40, 40)));
    let (least, most) = (
        blocks_for(&jpx, Some((0, 0, 40, 41))),
        blocks_for(&jpx, Some((0, 0, 64, 64))),
    );
    assert!(
        (least..=most).contains(&corner),
        "the corner decodes {corner} code-blocks, between {least} and {most}"
    );
    assert!(corner < whole, "{corner} of the page's {whole}");
}

/// **A clip is shown from the samples under it**: the same codestream drawn
/// under a clip that shows its top-left 40 by 40 pixels decodes fewer
/// code-blocks than drawn whole, and every pixel the clip shows is the pixel
/// the whole decode draws there — the window is widened by the sampler's
/// reach, so no pixel it shows reads a sample it left out — while the rest of
/// the page is left alone.
#[test]
fn a_clipped_jpx_image_is_shown_from_the_samples_under_its_clip() {
    let jpx = committed("irreversible-620x877.jp2");
    let keys = jpx_keys(620, 877);
    let draw = "q 155 0 0 219 0 0 cm /Im0 Do Q";
    let (whole, all) = counted_at(jpx_page((155.0, 219.0), draw, &keys, &jpx), 1.0);
    let clipped = format!("q 0 179 40 40 re W n {draw} Q");
    let (shown, part) = counted_at(jpx_page((155.0, 219.0), &clipped, &keys, &jpx), 1.0);
    assert_eq!(levels(&part).0, 2, "the clip changes no level");
    assert!(
        part.jpx_code_blocks_decoded < all.jpx_code_blocks_decoded,
        "{} code-blocks under the clip, {} whole",
        part.jpx_code_blocks_decoded,
        all.jpx_code_blocks_decoded
    );
    for y in 0..219usize {
        for x in 0..155usize {
            let at = y * whole.stride + x * 3;
            let pixel = |bitmap: &tinker_pdf::Bitmap| {
                (bitmap.data[at], bitmap.data[at + 1], bitmap.data[at + 2])
            };
            if x < 40 && y < 40 {
                assert_eq!(
                    pixel(&shown),
                    pixel(&whole),
                    "({x}, {y}): shown as drawn whole"
                );
            } else {
                assert_eq!(pixel(&shown), (255, 255, 255), "({x}, {y}): the page");
            }
        }
    }
}

/// **Each window's decode is kept apart.** A retained page's renders of two
/// regions decode two windows; the first region again meets its own in the
/// cache, decodes nothing and draws the same bytes; and the whole page is a
/// decode of its own, kept as well.
#[test]
fn each_window_of_a_jpx_image_is_cached_apart() {
    let jpx = committed("irreversible-620x877.jp2");
    let keys = jpx_keys(620, 877);
    let bytes = jpx_page(
        (155.0, 219.0),
        "q 155 0 0 219 0 0 cm /Im0 Do Q",
        &keys,
        &jpx,
    );
    let doc = Document::open(bytes).expect("it opens");
    let list = doc.page(0).expect("a page").display_list();
    let at = |region: Option<tinker_pdf::PixelRegion>| {
        let bitmap = list.render(&RenderOptions {
            region,
            stats: true,
            ..RenderOptions::default()
        });
        let stats = bitmap.stats.clone().expect("counted");
        (bitmap, stats)
    };
    let left = Some(tinker_pdf::PixelRegion::new(0, 0, 40, 40));
    let right = Some(tinker_pdf::PixelRegion::new(110, 170, 45, 49));
    let (first, stats) = at(left);
    assert_eq!((stats.image_misses, stats.image_hits), (1, 0), "a window");
    assert!(stats.jpx_code_blocks_decoded > 0);
    let (_, stats) = at(right);
    assert_eq!((stats.image_misses, stats.image_hits), (1, 0), "another");
    let (again, stats) = at(left);
    assert_eq!(
        (
            stats.image_misses,
            stats.image_hits,
            stats.jpx_code_blocks_decoded
        ),
        (0, 1, 0),
        "the first, found"
    );
    assert!(again.data == first.data, "a hit is what the miss drew");
    let (_, stats) = at(None);
    assert_eq!((stats.image_misses, stats.image_hits), (1, 0), "the whole");
    let (_, stats) = at(None);
    assert_eq!((stats.image_misses, stats.image_hits), (0, 1), "found");
}
