//! `Page::images()`: the samples a page's images hold, before any colour
//! conversion, and the space they are in.
//!
//! Every fixture is built with `DocumentBuilder` from a known sample array,
//! so the right answer is the array the test handed over — not another
//! decoder's opinion (ruling 13). A lossless path returns exactly those bytes;
//! the JPEG case, which is lossy by construction, is held to its geometry and
//! to the decoder's own output instead.

use tinker_pdf::{
    CalculatorOp, CompressedImage, DeviceSpace, Document, DocumentBuilder, FormXObject, Function,
    ImageColorSpace, ImageData, ImageMask, ImageSpace, PageImage, SampleCodec, SoftMask,
};

mod pdfa_support;

/// Builds a one-page document drawing each named image once, in order.
fn page_drawing(build: impl FnOnce(&mut DocumentBuilder) -> Vec<&'static [u8]>) -> Document {
    let mut builder = DocumentBuilder::new();
    let names = build(&mut builder);
    builder.add_page(100.0, 100.0, |page| {
        for (at, name) in names.iter().enumerate() {
            page.image(name, 10.0 * at as f64, 20.0, 8.0, 6.0);
        }
    });
    Document::open(builder.finish()).expect("the built document opens")
}

/// The one image on page 0.
fn only(document: &Document) -> PageImage {
    let mut images = document.page(0).expect("a page").images();
    assert_eq!(images.len(), 1, "{images:#?}");
    images.remove(0)
}

fn raw(
    width: u32,
    height: u32,
    bits: u8,
    space: ImageColorSpace<'static>,
    data: &'static [u8],
) -> ImageData<'static> {
    ImageData::Compressed(CompressedImage {
        width,
        height,
        bits_per_component: bits,
        color_space: space,
        filter: None,
        data,
        color_key_mask: None,
        soft_mask: None,
    })
}

/// What every lossless fixture asserts: the samples are the array handed
/// over, at the depth and component count it was written at.
#[track_caller]
fn exactly(image: &PageImage, bits: u8, components: u8, samples: &[u8]) {
    assert_eq!(image.refused, None);
    assert_eq!(image.codec, SampleCodec::Stream);
    assert_eq!(image.bits_per_component, bits);
    assert_eq!(image.components, components);
    assert_eq!(image.samples, samples);
    assert!(!image.stencil);
    assert!(image.reference.is_some(), "an XObject has its reference");
    assert!(
        image.warnings.is_empty(),
        "a clean decode tolerated nothing: {:?}",
        image.warnings
    );
}

#[test]
fn grey_samples_come_back_exactly() {
    let samples: &'static [u8] = &[0, 64, 128, 255, 17, 34];
    let document = page_drawing(|b| {
        assert!(b.add_image(
            b"Im0",
            &ImageData::Gray8 {
                width: 3,
                height: 2,
                data: samples,
            }
        ));
        vec![b"Im0"]
    });
    let image = only(&document);
    exactly(&image, 8, 1, samples);
    assert_eq!((image.width, image.height), (3, 2));
    assert_eq!(image.color_space, Some(ImageSpace::DeviceGray));
    assert_eq!(image.name, b"Im0");
    assert!(image.decode.is_empty());
    assert_eq!(image.placements, vec![[8.0, 0.0, 0.0, 6.0, 0.0, 20.0]]);
}

#[test]
fn rgb_samples_come_back_exactly() {
    let samples: &'static [u8] = &[255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 99, 199];
    let document = page_drawing(|b| {
        assert!(b.add_image(
            b"Im0",
            &ImageData::Rgb8 {
                width: 2,
                height: 2,
                data: samples,
            }
        ));
        vec![b"Im0"]
    });
    let image = only(&document);
    exactly(&image, 8, 3, samples);
    assert_eq!(image.color_space, Some(ImageSpace::DeviceRgb));
}

/// The case the renderer's RGB-only type could never answer: the four ink
/// values, not what they looked like.
#[test]
fn cmyk_samples_come_back_as_ink_not_as_rgb() {
    let samples: &'static [u8] = &[255, 0, 0, 0, 0, 128, 0, 64];
    let document = page_drawing(|b| {
        assert!(b.add_image(b"Im0", &raw(2, 1, 8, ImageColorSpace::DeviceCmyk, samples)));
        vec![b"Im0"]
    });
    let image = only(&document);
    exactly(&image, 8, 4, samples);
    assert_eq!(image.color_space, Some(ImageSpace::DeviceCmyk));
}

/// An indexed image's samples are indices, and its palette comes with it.
#[test]
fn indexed_samples_are_indices_with_their_palette() {
    let palette: &'static [u8] = &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255];
    let samples: &'static [u8] = &[0, 3, 2, 1];
    let document = page_drawing(|b| {
        assert!(b.add_image(
            b"Im0",
            &raw(
                4,
                1,
                8,
                ImageColorSpace::Indexed {
                    base: DeviceSpace::Rgb,
                    lookup: palette,
                },
                samples
            )
        ));
        vec![b"Im0"]
    });
    let image = only(&document);
    exactly(&image, 8, 1, samples);
    assert_eq!(
        image.color_space,
        Some(ImageSpace::Indexed {
            base: Box::new(ImageSpace::DeviceRgb),
            high: 3,
            lookup: palette.to_vec(),
        })
    );
}

/// An ICC image names its profile, and the profile's bytes come back.
#[test]
fn an_icc_image_carries_its_profile() {
    let profile = pdfa_support::srgb_like();
    let samples: &'static [u8] = &[10, 20, 30, 40, 50, 60];
    let document = page_drawing(|b| {
        assert!(b.add_icc_color_space(b"CS0", &profile, 3));
        assert!(b.add_image(
            b"Im0",
            &raw(
                2,
                1,
                8,
                ImageColorSpace::Icc {
                    resource: b"CS0",
                    components: 3,
                },
                samples
            )
        ));
        vec![b"Im0"]
    });
    let image = only(&document);
    exactly(&image, 8, 3, samples);
    assert_eq!(
        image.color_space,
        Some(ImageSpace::Icc {
            components: 3,
            profile,
            alternate: None,
        })
    );
}

/// One bit a sample, ten to a row: each row padded to two bytes, exactly as
/// the stream holds it.
#[test]
fn one_bit_samples_keep_their_row_padding() {
    let samples: &'static [u8] = &[0b1010_1010, 0b1100_0000, 0b0101_0101, 0b0000_0000];
    let document = page_drawing(|b| {
        assert!(b.add_image(b"Im0", &raw(10, 2, 1, ImageColorSpace::DeviceGray, samples)));
        vec![b"Im0"]
    });
    let image = only(&document);
    exactly(&image, 1, 1, samples);
}

/// Sixteen bits a sample, big-endian, untruncated — the renderer's own path
/// keeps only the high byte, and extraction must not.
#[test]
fn sixteen_bit_samples_keep_both_bytes() {
    let samples: &'static [u8] = &[0x12, 0x34, 0xFF, 0x01, 0x00, 0x80];
    let document = page_drawing(|b| {
        assert!(b.add_image(b"Im0", &raw(3, 1, 16, ImageColorSpace::DeviceGray, samples)));
        vec![b"Im0"]
    });
    let image = only(&document);
    exactly(&image, 16, 1, samples);
}

/// A `/Separation` and a `/DeviceN` image report their colorants' names and
/// their alternate, and their samples are tints.
#[test]
fn tint_images_name_their_colorants() {
    let ramp = Function::Exponential {
        domain: [0.0, 1.0],
        c0: vec![1.0, 1.0, 1.0],
        c1: vec![0.0, 0.0, 1.0],
        n: 1.0,
    };
    use CalculatorOp::{Number as N, Operator as Op};
    let two = Function::Calculator {
        domain: vec![[0.0, 1.0]; 2],
        range: vec![[0.0, 1.0]; 3],
        program: vec![
            Op("pop"),
            N(1.0),
            Op("exch"),
            Op("sub"),
            Op("dup"),
            Op("dup"),
        ],
    };
    let one: &'static [u8] = &[0, 128, 255];
    let pairs: &'static [u8] = &[0, 255, 255, 0];
    let document = page_drawing(|b| {
        assert!(b.add_separation_color_space(b"CS0", b"Spot Blue", DeviceSpace::Rgb, &ramp));
        assert!(b.add_device_n_color_space(
            b"CS1",
            &[b"Cyan", b"Spot Orange"],
            DeviceSpace::Rgb,
            &two,
            None
        ));
        assert!(b.add_image(
            b"Im0",
            &raw(
                3,
                1,
                8,
                ImageColorSpace::Tint {
                    resource: b"CS0",
                    components: 1
                },
                one
            )
        ));
        assert!(b.add_image(
            b"Im1",
            &raw(
                2,
                1,
                8,
                ImageColorSpace::Tint {
                    resource: b"CS1",
                    components: 2
                },
                pairs
            )
        ));
        vec![b"Im0", b"Im1"]
    });
    let images = document.page(0).expect("a page").images();
    assert_eq!(images.len(), 2);
    exactly(&images[0], 8, 1, one);
    assert_eq!(
        images[0].color_space,
        Some(ImageSpace::Separation {
            colorant: b"Spot Blue".to_vec(),
            alternate: Box::new(ImageSpace::DeviceRgb),
        })
    );
    exactly(&images[1], 8, 2, pairs);
    assert_eq!(
        images[1].color_space,
        Some(ImageSpace::DeviceN {
            colorants: vec![b"Cyan".to_vec(), b"Spot Orange".to_vec()],
            alternate: Box::new(ImageSpace::DeviceRgb),
        })
    );
}

/// A soft mask and a colour-key mask come with the image, the first as an
/// image of its own and the second as its ranges.
#[test]
fn masks_come_with_the_image() {
    let samples: &'static [u8] = &[1, 2, 3, 4];
    let alpha: &'static [u8] = &[255, 0, 128, 64];
    let ranges: &'static [(u32, u32)] = &[(2, 3)];
    let document = page_drawing(|b| {
        assert!(b.add_image(
            b"Im0",
            &ImageData::Compressed(CompressedImage {
                width: 2,
                height: 2,
                bits_per_component: 8,
                color_space: ImageColorSpace::DeviceGray,
                filter: None,
                data: samples,
                color_key_mask: Some(ranges),
                soft_mask: Some(SoftMask {
                    width: 2,
                    height: 2,
                    bits_per_component: 8,
                    filter: None,
                    data: alpha,
                }),
            })
        ));
        vec![b"Im0"]
    });
    let image = only(&document);
    exactly(&image, 8, 1, samples);
    assert_eq!(image.mask, Some(ImageMask::ColorKey(vec![(2, 3)])));
    let soft = image.soft_mask.as_deref().expect("a soft mask");
    assert_eq!(soft.samples, alpha);
    assert_eq!(soft.color_space, Some(ImageSpace::DeviceGray));
    assert!(soft.reference.is_some() && soft.reference != image.reference);
    assert!(soft.placements.is_empty(), "a mask is not drawn on its own");
}

/// An image drawn twice is one entry with two placements; an image drawn
/// inside a form is found through the form's own resources, at the page's
/// transform composed with the form's.
#[test]
fn placements_are_every_drawing_including_inside_a_form() {
    let samples: &'static [u8] = &[7, 8];
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Gray8 {
            width: 2,
            height: 1,
            data: samples,
        }
    ));
    assert!(builder.add_form(
        b"Fm0",
        &FormXObject {
            bbox: [0.0, 0.0, 100.0, 100.0],
            matrix: Some([1.0, 0.0, 0.0, 1.0, 50.0, 0.0]),
            group: None,
            content: b"q 4 0 0 2 1 1 cm /Im0 Do Q",
        },
    ));
    builder.add_page(100.0, 100.0, |page| {
        page.image(b"Im0", 0.0, 0.0, 10.0, 10.0);
        page.image(b"Im0", 20.0, 30.0, 5.0, 5.0);
        assert!(page.form(b"Fm0"));
    });
    let document = Document::open(builder.finish()).expect("it opens");
    let image = only(&document);
    exactly(&image, 8, 1, samples);
    assert_eq!(
        image.placements,
        vec![
            [10.0, 0.0, 0.0, 10.0, 0.0, 0.0],
            [5.0, 0.0, 0.0, 5.0, 20.0, 30.0],
            [4.0, 0.0, 0.0, 2.0, 51.0, 1.0],
        ]
    );
}

/// An image only a form's own `/Resources` name is found there: the page's
/// resources do not carry it, so a walk that looked names up in the page's
/// scope inside the form would not find it at all.
#[test]
fn an_image_only_a_form_names_is_found_through_the_forms_scope() {
    let inner: &'static [u8] = &[1, 2, 3, 4];
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(
        b"Only",
        &ImageData::Gray8 {
            width: 2,
            height: 2,
            data: inner,
        }
    ));
    assert!(builder.add_form(
        b"Fm0",
        &FormXObject {
            bbox: [0.0, 0.0, 100.0, 100.0],
            matrix: None,
            group: None,
            content: b"q 3 0 0 3 7 9 cm /Only Do Q",
        },
    ));
    // The page is begun after this, so its `/XObject` holds the form and not
    // the image.
    builder.clear_image_resources();
    builder.add_page(100.0, 100.0, |page| {
        page.raw(b"q 2 0 0 2 0 0 cm");
        assert!(page.form(b"Fm0"));
        page.raw(b"Q");
    });
    let document = Document::open(builder.finish()).expect("it opens");
    let image = only(&document);
    exactly(&image, 8, 1, inner);
    assert_eq!(image.name, b"Only");
    assert_eq!(image.placements, vec![[6.0, 0.0, 0.0, 6.0, 14.0, 18.0]]);
}

/// An inline image is reported as itself — no reference, its own samples —
/// and one naming a page colour space resource finds it (8.9.7).
#[test]
fn inline_images_are_reported_too() {
    let profile = pdfa_support::srgb_like();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_icc_color_space(b"CS0", &profile, 3));
    builder.add_page(100.0, 100.0, |page| {
        let mut grey = b"q 10 0 0 10 5 5 cm BI /W 3 /H 1 /CS /G /BPC 8 ID ".to_vec();
        grey.extend_from_slice(&[0, 0x80, 0xFF]);
        grey.extend_from_slice(b" EI Q");
        page.raw(&grey);
        let mut named = b"BI /W 1 /H 1 /CS /CS0 /BPC 8 /D [1 0 1 0 1 0] ID ".to_vec();
        named.extend_from_slice(&[1, 2, 3]);
        named.extend_from_slice(b" EI");
        page.raw(&named);
    });
    let document = Document::open(builder.finish()).expect("it opens");
    let images = document.page(0).expect("a page").images();
    assert_eq!(images.len(), 2, "{images:#?}");

    assert_eq!(images[0].reference, None);
    assert!(images[0].name.is_empty());
    assert_eq!(images[0].samples, [0, 0x80, 0xFF]);
    assert_eq!(images[0].color_space, Some(ImageSpace::DeviceGray));
    assert_eq!(images[0].placements, vec![[10.0, 0.0, 0.0, 10.0, 5.0, 5.0]]);

    assert_eq!(images[1].samples, [1, 2, 3]);
    assert_eq!(images[1].components, 3);
    assert_eq!(images[1].decode, vec![(1.0, 0.0); 3]);
    assert!(matches!(
        &images[1].color_space,
        Some(ImageSpace::Icc { components: 3, profile: p, .. }) if *p == profile
    ));
}

/// A JPEG is lossy, so it is held to its geometry and to the decoder's own
/// output rather than to the array it was made from: three components of
/// eight bits, in the frame's own RGB, never the dictionary's claim.
#[test]
fn a_jpeg_reports_the_decoders_samples() {
    let pixels: Vec<u8> = (0..16 * 8 * 3).map(|i| (i * 7 % 256) as u8).collect();
    let jpeg = tinker_pdf_filters::jpeg_encode(
        &tinker_pdf_filters::JpegSource {
            width: 16,
            height: 8,
            stride: 16 * 3,
            colour: tinker_pdf_filters::JpegSourceColour::Rgb,
            data: &pixels,
        },
        &tinker_pdf_filters::JpegOptions::default(),
    )
    .expect("the fixture encodes");
    let expected = tinker_pdf_filters::jpeg_decode(&jpeg, 1 << 28).expect("and decodes");

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(b"Im0", &ImageData::Jpeg(&jpeg)));
    builder.add_page(100.0, 100.0, |page| page.image(b"Im0", 0.0, 0.0, 16.0, 8.0));
    let document = Document::open(builder.finish()).expect("it opens");
    let image = only(&document);
    assert_eq!(image.codec, SampleCodec::Dct);
    assert_eq!((image.width, image.height), (16, 8));
    assert_eq!((image.bits_per_component, image.components), (8, 3));
    assert_eq!(image.samples, expected.data);
    assert_eq!(image.color_space, Some(ImageSpace::DeviceRgb));
}

/// A fax is one-bit samples in PDF's polarity, and lossless: the raster the
/// encoder was handed comes back. It stays one bit a sample when the
/// dictionary claims eight — 7.4.6 makes the codec, not the claim, decide —
/// which is the rule the renderer's own decode reads through the same
/// function.
#[test]
fn a_fax_is_one_bit_samples_whatever_the_dictionary_claims() {
    let raster: &[u8] = &[0b1111_0000, 0b0000_1111, 0b1010_1010];
    let params = tinker_pdf::CcittParams {
        k: -1,
        columns: 8,
        rows: 3,
        black_is_1: false,
        byte_align: false,
        end_of_line: false,
        end_of_block: true,
    };
    let coded = tinker_pdf_filters::ccitt_g4_encode(&tinker_pdf_filters::CcittSource {
        columns: 8,
        rows: 3,
        black_is_1: false,
        stride: 1,
        end_of_block: true,
        data: raster,
    })
    .expect("the fixture encodes");

    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(
        b"Im0",
        &ImageData::Compressed(CompressedImage {
            width: 8,
            height: 3,
            bits_per_component: 1,
            color_space: ImageColorSpace::DeviceGray,
            filter: Some(tinker_pdf::ImageFilter::CcittFax(params)),
            data: &coded,
            color_key_mask: None,
            soft_mask: None,
        })
    ));
    builder.add_page(100.0, 100.0, |page| page.image(b"Im0", 0.0, 0.0, 8.0, 3.0));
    let honest = builder.finish();

    // The same file with the dictionary lying about the depth, the same
    // length so every offset still holds.
    let at = honest
        .windows(19)
        .position(|w| w == b"/BitsPerComponent 1")
        .expect("the image states its depth");
    let mut lying = honest.clone();
    lying[at + 18] = b'8';

    for bytes in [honest, lying] {
        let document = Document::open(bytes).expect("it opens");
        let image = only(&document);
        assert_eq!(image.codec, SampleCodec::CcittFax);
        assert_eq!(image.bits_per_component, 1);
        assert_eq!(image.samples, raster);
        assert_eq!(image.refused, None);
    }
}

/// A one-page document over hand-written bytes: `objects` are appended after
/// the catalog, the page tree, the page (whose `/Resources` is `resources`)
/// and its content stream `content`, which are objects 1 to 4.
fn written(width: u32, height: u32, resources: &str, content: &[u8], objects: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    out.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n");
    out.extend_from_slice(
        format!(
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n\
             /Resources {resources} /Contents 4 0 R >>\nendobj\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(format!("4 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes());
    out.extend_from_slice(content);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    out.extend_from_slice(objects);
    out.extend_from_slice(b"trailer\n<< /Size 20 /Root 1 0 R >>\n%%EOF\n");
    out
}

/// A `/Mask` that is a reference is a stencil mask or a colour key according
/// to what it reaches (8.9.6.3, 8.9.6.4): an indirect `[0 0]` is a colour key,
/// as the renderer reads it — the sample it names is not painted — and not a
/// stencil mask that would not open.
#[test]
fn an_indirect_colour_key_mask_is_read_as_the_renderer_reads_it() {
    let mut objects = Vec::new();
    objects.extend_from_slice(
        b"5 0 obj\n<< /Type /XObject /Subtype /Image /Width 2 /Height 1 \
          /BitsPerComponent 8 /ColorSpace /DeviceGray /Mask 6 0 R /Length 2 >>\nstream\n",
    );
    objects.extend_from_slice(&[0, 128]);
    objects.extend_from_slice(b"\nendstream\nendobj\n6 0 obj\n[0 0]\nendobj\n");
    let bytes = written(
        20,
        10,
        "<< /XObject << /Im0 5 0 R >> >>",
        b"q 20 0 0 10 0 0 cm /Im0 Do Q",
        &objects,
    );
    let document = Document::open(bytes).expect("it opens");
    let image = only(&document);
    exactly(&image, 8, 1, &[0, 128]);
    assert_eq!(image.mask, Some(ImageMask::ColorKey(vec![(0, 0)])));

    let bitmap = document
        .page(0)
        .expect("a page")
        .render(&tinker_pdf::RenderOptions::default());
    let at = |x: usize| {
        let i = 5 * bitmap.stride + x * bitmap.components();
        (bitmap.data[i], bitmap.data[i + 1], bitmap.data[i + 2])
    };
    assert_eq!(at(5), (255, 255, 255), "the renderer keys the 0 out");
    assert_eq!(at(15), (128, 128, 128), "and paints the 128");
}

/// Packs a pattern of `0` and `1` into bytes, most significant bit first,
/// ignoring anything else — `ccitt.rs`'s helper, so T.4's codes can be spaced
/// as its tables print them.
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

/// Ruling 10 on the extraction path: a fax the decoder had to forgive — a row
/// that would not decode, replicated from the one above — is listed with its
/// samples **and** with what was forgiven, as an XObject and inline, and the
/// reasons are the ones the render of the same page names. A damaged image
/// that looked like a clean one was the defect.
#[test]
fn a_damaged_fax_says_what_the_decoder_forgave_on_the_image() {
    // `ccitt.rs`'s damaged row: one good row, then a code T.4 does not have.
    let coded = bits(concat!("001 00110101 011 1 ", "111 ", "00000001"));
    let parms = "/K -1 /Columns 8 /Rows 4";

    let mut content = Vec::new();
    content.extend_from_slice(b"q 20 0 0 20 0 0 cm /Im0 Do Q q 20 0 0 20 20 0 cm ");
    content.extend_from_slice(
        format!("BI /W 8 /H 4 /CS /G /BPC 1 /F /CCF /DP << {parms} >> ID ").as_bytes(),
    );
    content.extend_from_slice(&coded);
    content.extend_from_slice(b" EI Q");
    let mut objects = Vec::new();
    objects.extend_from_slice(
        format!(
            "5 0 obj\n<< /Type /XObject /Subtype /Image /Width 8 /Height 4 \
             /ColorSpace /DeviceGray /BitsPerComponent 1 \
             /Filter /CCITTFaxDecode /DecodeParms << {parms} >> /Length {} >>\nstream\n",
            coded.len()
        )
        .as_bytes(),
    );
    objects.extend_from_slice(&coded);
    objects.extend_from_slice(b"\nendstream\nendobj\n");
    let document = Document::open(written(
        40,
        20,
        "<< /XObject << /Im0 5 0 R >> >>",
        &content,
        &objects,
    ))
    .expect("it opens");

    let page = document.page(0).expect("a page");
    let images = page.images();
    assert_eq!(images.len(), 2, "{images:#?}");
    let rendered = page.render(&tinker_pdf::RenderOptions::default());
    for (image, name) in images.iter().zip(["Im0", "inline"]) {
        assert_eq!(image.codec, SampleCodec::CcittFax, "{name}");
        assert_eq!(image.refused, None, "{name}: decoded, with a leniency");
        assert_eq!(image.samples.len(), 4, "{name}: all four rows are there");
        assert!(
            !image.warnings.is_empty(),
            "{name}: the image says what the decoder forgave"
        );
        let mut named: Vec<String> = rendered
            .warnings
            .iter()
            .filter_map(|warning| match warning {
                tinker_pdf::RenderWarning::DamagedImage { name: n, reason } if n == name => {
                    Some(reason.clone())
                }
                _ => None,
            })
            .collect();
        let mut listed = image.warnings.clone();
        named.sort();
        listed.sort();
        assert_eq!(listed, named, "{name}: the render names the same");
    }
}

// ---- hostile images ---------------------------------------------------------

/// A page over hand-written image dictionaries, each wrong in its own way.
fn hostile(image_dicts: &[&str]) -> Vec<u8> {
    let mut objects = String::new();
    let mut names = String::new();
    let mut content = String::new();
    for (at, dict) in image_dicts.iter().enumerate() {
        let num = 10 + at;
        let body = "\u{1}\u{2}\u{3}";
        objects.push_str(&format!(
            "{num} 0 obj\n<< /Type /XObject /Subtype /Image {dict} /Length {} >>\nstream\n{body}\nendstream\nendobj\n",
            body.len()
        ));
        names.push_str(&format!("/I{at} {num} 0 R "));
        content.push_str(&format!("/I{at} Do "));
    }
    content.push_str("BI /W 99999 /H 99999 /CS /Nope /BPC 13 /F /Bogus ID xyz EI ");
    content.push_str("BI ID EI ");
    content.push_str("BI /W 1 /H 1 /CS [/I /CS0 300 <00>] /BPC 8 /D [1] ID a EI ");
    format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 10 10]\n\
   /Resources << /XObject << {names}>> /ColorSpace << /CS0 [/Indexed /CS0 1 <00>] >> >>\n\
   /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n\
{objects}\
trailer\n<< /Size 60 /Root 1 0 R >>\n%%EOF\n",
        content.len()
    )
    .into_bytes()
}

/// Nothing an image dictionary can say makes extraction panic, and a
/// dictionary that cannot be decoded is still listed, with the reason.
#[test]
fn hostile_images_never_panic_and_are_still_listed() {
    let bytes = hostile(&[
        "/Width 4000000000 /Height 3 /BitsPerComponent 8 /ColorSpace /DeviceRGB",
        "/Width 2 /Height 2 /BitsPerComponent 7 /ColorSpace [/Indexed /DeviceRGB 900 (ab)]",
        "/Width 2 /Height 2 /BitsPerComponent 8 /ColorSpace [/ICCBased 999 0 R]",
        "/Width 2 /Height 2 /BitsPerComponent 8 /ColorSpace [/DeviceN [/A 12 (x)] /DeviceGray 5]",
        "/Width 2 /Height 2 /BitsPerComponent 8 /ColorSpace [/Separation] /Decode [1 0 1]",
        "/Width 2 /Height 2 /BitsPerComponent 8 /ColorSpace /DeviceGray /Mask 11 0 R /SMask 10 0 R",
        "/Width 2 /Height 2 /BitsPerComponent 8 /ColorSpace /DeviceGray /Mask [5 1 99999999999]",
        "/Width 8 /Height 8 /Filter /DCTDecode /ColorSpace /DeviceRGB",
        "/Width 8 /Height 8 /Filter /JPXDecode",
        "/Width 8 /Height 8 /Filter /CCITTFaxDecode /DecodeParms << /K -1 /Columns 0 >>",
        "/Width 8 /Height 8 /Filter /JBIG2Decode",
        "/Width 8 /Height 8 /Filter [/FlateDecode /LZWDecode] /BitsPerComponent 8 /ColorSpace /DeviceGray",
        "/Width 2 /Height 2 /ImageMask true /BitsPerComponent 8 /Decode [0 1]",
        "/Width -5 /Height 2 /ColorSpace [/Lab << /Range [1] >>]",
        "/Width 2 /Height 2 /BitsPerComponent 8 /ColorSpace [/CalRGB << /Matrix [1 2] /Gamma 3 >>]",
    ]);
    let document = Document::open(bytes).expect("it opens");
    let images = document.page(0).expect("a page").images();
    assert_eq!(images.len(), 18, "fifteen XObjects and three inline images");
    for image in &images {
        let wanted = u64::from(image.width)
            * u64::from(image.components)
            * u64::from(image.bits_per_component);
        let wanted = wanted.div_ceil(8) * u64::from(image.height);
        assert!(
            image.samples.len() as u64 <= wanted,
            "never more samples than the geometry lays out: {image:#?}"
        );
        if image.refused.is_some() {
            assert!(image.samples.is_empty());
        }
    }
    // The inline image with an empty dictionary has no geometry at all.
    assert!(images
        .iter()
        .any(|i| i.reference.is_none() && i.refused.as_deref() == Some("empty")));
}

/// A deterministic mutation sweep over a document holding every kind of
/// image above, calling `images()` on whatever still opens.
#[test]
fn mutated_image_documents_never_panic() {
    let mut builder = DocumentBuilder::new();
    let palette: &'static [u8] = &[0, 0, 0, 255, 255, 255];
    assert!(builder.add_image(
        b"A",
        &raw(
            3,
            2,
            1,
            ImageColorSpace::Indexed {
                base: DeviceSpace::Rgb,
                lookup: palette
            },
            &[0b1010_0000, 0b0100_0000]
        )
    ));
    assert!(builder.add_image(b"B", &raw(1, 1, 16, ImageColorSpace::DeviceCmyk, &[1; 8])));
    assert!(builder.add_image(
        b"C",
        &ImageData::Rgb8 {
            width: 2,
            height: 1,
            data: &[1, 2, 3, 4, 5, 6]
        }
    ));
    builder.add_page(20.0, 20.0, |page| {
        page.image(b"A", 0.0, 0.0, 5.0, 5.0);
        page.image(b"B", 5.0, 0.0, 5.0, 5.0);
        page.image(b"C", 10.0, 0.0, 5.0, 5.0);
        page.raw(b"BI /W 2 /H 1 /CS /RGB /BPC 8 ID abcdef EI");
    });
    let original = builder.finish();

    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let rounds = if cfg!(debug_assertions) { 150 } else { 3000 };
    for _ in 0..rounds {
        let mut bytes = original.clone();
        for _ in 0..(1 + next() % 8) {
            let at = (next() % bytes.len() as u64) as usize;
            bytes[at] = (next() & 0xFF) as u8;
        }
        if let Ok(document) = Document::open(bytes) {
            for page in document.pages() {
                let _ = page.images();
            }
        }
    }
}
