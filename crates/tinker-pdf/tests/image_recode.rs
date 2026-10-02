//! **Image recompression and downsampling on save** — `SaveOptions::images`.
//!
//! Every expected answer here is either the generator's input (a lossless
//! coding must give back exactly the samples the test wrote) or arithmetic
//! done in this file from the standards' own formulas: a box filter's block
//! means by hand, and a JPEG's error bound from T.81 A.3.3's DCT and the
//! caller's quantisation table. No decoder's output is the answer to anything
//! (ruling 13); the repository's decoders are only how the bytes the save
//! wrote are read back, as any reader would.

// The bound below is T.81's sums written as T.81 writes them, over `x`, `y`,
// `u` and `v`; an iterator chain would hide which index is which.
#![allow(clippy::needless_range_loop)]

use tinker_pdf::{
    write::save, BilevelCodec, ContinuousCodec, Document, ImageCoding, ImageOutcome, ImagePolicy,
    ImageRecoding, ImageReport, JpegTables, RenderOptions, SaveOptions, UntouchedImageReason,
};
use tinker_pdf_cos::{CosDocument, ObjRef, WriteMode, WriteOptions};

// ---------------------------------------------------------------------------
// Fixtures.
// ---------------------------------------------------------------------------

/// A one-page document of `width` x `height` points around `content`, with
/// `resources` and `objects` numbered from 5.
fn pdf(content: &str, width: u32, height: u32, resources: &str, objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = format!(
        "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}]\n\
   /Resources {resources} /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
        content.len()
    )
    .into_bytes();
    for (index, object) in objects.iter().enumerate() {
        out.extend_from_slice(format!("{} 0 obj\n", index + 5).as_bytes());
        out.extend_from_slice(object);
        out.extend_from_slice(b"\nendobj\n");
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\n%%EOF\n",
            objects.len() + 5
        )
        .as_bytes(),
    );
    out
}

/// A stream object: `dict` and raw `data`.
fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// An image XObject whose samples are stored as hex, so the stored stream is
/// twice their size and any real coding is smaller.
fn hex_image(dict: &str, samples: &[u8]) -> Vec<u8> {
    let mut hex: Vec<u8> = samples
        .iter()
        .flat_map(|b| format!("{b:02x}").into_bytes())
        .collect();
    hex.push(b'>');
    stream(
        &format!("/Type /XObject /Subtype /Image {dict} /Filter /ASCIIHexDecode"),
        &hex,
    )
}

/// Grey 8-bit samples: a ramp with a few spikes, compressible and not flat.
fn grey(width: usize, height: usize) -> Vec<u8> {
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            if (x * 7 + y * 3) % 23 == 0 {
                250
            } else {
                ((x * 10 + y * 3) % 200) as u8
            }
        })
        .collect()
}

/// RGB 8-bit samples.
fn rgb(width: usize, height: usize) -> Vec<u8> {
    (0..width * height)
        .flat_map(|i| {
            let (x, y) = (i % width, i / width);
            [
                (x * 20 % 256) as u8,
                (y * 25 % 256) as u8,
                ((x + y) * 7 % 256) as u8,
            ]
        })
        .collect()
}

/// A one-bit raster `width` wide: a frame and a diagonal band, rows padded
/// to whole bytes with zero bits. 1 is white in a one-bit DeviceGray image.
fn bilevel(width: usize, height: usize) -> Vec<u8> {
    let stride = width.div_ceil(8);
    let mut out = vec![0u8; stride * height];
    for y in 0..height {
        for x in 0..width {
            let ink = x == 0 || y == 0 || x == width - 1 || y == height - 1 || (x + 2 * y) % 11 < 3;
            if !ink {
                out[y * stride + x / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    out
}

/// The significant bits of a one-bit raster, padding dropped.
fn bits(raster: &[u8], width: usize, height: usize) -> Vec<bool> {
    let stride = width.div_ceil(8);
    (0..height)
        .flat_map(|y| (0..width).map(move |x| (y, x)))
        .map(|(y, x)| {
            raster
                .get(y * stride + x / 8)
                .is_some_and(|b| b & (0x80 >> (x % 8)) != 0)
        })
        .collect()
}

fn recode(continuous: ContinuousCodec, bilevel: BilevelCodec) -> SaveOptions {
    SaveOptions {
        images: ImagePolicy::Recode(ImageRecoding::new(continuous, bilevel)),
        ..SaveOptions::default()
    }
}

/// Saves `bytes` with `options` and hands back the saved document and the
/// pass's report.
fn saved(bytes: Vec<u8>, options: &SaveOptions) -> (Document, ImageReport) {
    let document = Document::open(bytes).expect("it opens");
    let mut editor = document.editor();
    let out = save(&mut editor, options);
    let report = match out.images {
        ImageOutcome::Recoded(report) => report,
        other => panic!("a rewrite reports `Recoded`, not {other:?}"),
    };
    (Document::open(out.bytes).expect("the save reopens"), report)
}

fn dict_of(doc: &CosDocument, number: u32) -> tinker_pdf_cos::Dict {
    doc.get(ObjRef::new(number, 0))
        .expect("the object")
        .as_dict()
        .cloned()
        .expect("a stream")
}

fn name_of(doc: &CosDocument, dict: &tinker_pdf_cos::Dict, key: &[u8]) -> Option<String> {
    dict.get_name(doc.intern(key))
        .and_then(|n| doc.name_bytes(n))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn int_of(doc: &CosDocument, dict: &tinker_pdf_cos::Dict, key: &[u8]) -> Option<i64> {
    dict.get_int(doc.intern(key))
}

fn render(document: &Document) -> Vec<u8> {
    document
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
        .data
}

// ---------------------------------------------------------------------------
// Off by default.
// ---------------------------------------------------------------------------

/// **A default save is byte-identical to what a save was before this pass
/// existed**: the same bytes as the font pass and the serializer alone, every
/// image stream exactly as stored, and `Kept`.
#[test]
fn the_default_save_leaves_every_image_as_stored() {
    let jpeg = tinker_pdf_filters::jpeg_encode(
        &tinker_pdf_filters::JpegSource {
            width: 16,
            height: 8,
            colour: tinker_pdf_filters::JpegSourceColour::Gray,
            stride: 16,
            data: &grey(16, 8),
        },
        &tinker_pdf_filters::JpegOptions::default(),
    )
    .expect("a JPEG");
    let bytes = pdf(
        "q 72 0 0 36 0 0 cm /A Do Q q 72 0 0 36 0 50 cm /B Do Q q 72 0 0 36 100 0 cm /C Do Q",
        200,
        100,
        "<< /XObject << /A 5 0 R /B 6 0 R /C 7 0 R >> >>",
        &[
            hex_image(
                "/Width 24 /Height 16 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &grey(24, 16),
            ),
            stream(
                "/Type /XObject /Subtype /Image /Width 16 /Height 8 /ColorSpace /DeviceGray \
                 /BitsPerComponent 8 /Filter /DCTDecode",
                &jpeg,
            ),
            hex_image("/Width 37 /Height 20 /ImageMask true", &bilevel(37, 20)),
        ],
    );
    let document = Document::open(bytes.clone()).expect("opens");

    let mut editor = document.editor();
    let out = save(&mut editor, &SaveOptions::default());
    assert_eq!(out.images, ImageOutcome::Kept, "the pass did not run");

    // What a save was before: the font pass, then the serializer.
    let mut before = document.editor();
    let _ = tinker_pdf::subset::apply(&mut before);
    assert_eq!(
        out.bytes,
        before.save(&WriteOptions::default()),
        "byte for byte the save this door made before images were a pass"
    );
    assert_eq!(
        out.bytes,
        save(
            &mut document.editor(),
            &SaveOptions {
                images: ImagePolicy::Keep,
                ..SaveOptions::default()
            }
        )
        .bytes,
        "and `Keep` named is the default"
    );

    let reopened = Document::open(out.bytes).expect("reopens");
    for number in 5..=7 {
        assert_eq!(
            reopened
                .cos()
                .stream_raw(ObjRef::new(number, 0))
                .expect("a stream"),
            document
                .cos()
                .stream_raw(ObjRef::new(number, 0))
                .expect("a stream"),
            "image {number}'s stored bytes are untouched"
        );
    }
}

// ---------------------------------------------------------------------------
// Lossless.
// ---------------------------------------------------------------------------

/// **Every lossless coding gives back exactly the samples the test wrote**,
/// and the page draws the same pixels: deflate over eight-bit grey and RGB,
/// four-bit grey and an indexed image; and deflate, G4 and a JBIG2 generic
/// region over a one-bit grey image and a stencil mask, whose bits come back
/// in their own polarity (the render is the check that a stencil still
/// paints where it painted).
#[test]
fn lossless_codings_give_back_exactly_the_samples_written() {
    let four_bit: Vec<u8> = (0..6)
        .flat_map(|y| (0..5).map(move |x| (((x * 2 + y) % 16) << 4 | ((x * 2 + 1 + y) % 16)) as u8))
        .collect();
    let indices: Vec<u8> = (0..48).map(|i| (i % 4) as u8).collect();
    let samples: Vec<Vec<u8>> = vec![
        grey(24, 16),
        rgb(12, 10),
        four_bit,
        indices,
        bilevel(37, 20),
        bilevel(37, 20),
    ];
    let objects = vec![
        hex_image(
            "/Width 24 /Height 16 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &samples[0],
        ),
        hex_image(
            "/Width 12 /Height 10 /ColorSpace /DeviceRGB /BitsPerComponent 8",
            &samples[1],
        ),
        hex_image(
            "/Width 10 /Height 6 /ColorSpace /DeviceGray /BitsPerComponent 4",
            &samples[2],
        ),
        hex_image(
            "/Width 12 /Height 4 /ColorSpace [/Indexed /DeviceRGB 3 <ff000000ff000000ffffff00>] \
             /BitsPerComponent 8",
            &samples[3],
        ),
        hex_image(
            "/Width 37 /Height 20 /ColorSpace /DeviceGray /BitsPerComponent 1",
            &samples[4],
        ),
        hex_image("/Width 37 /Height 20 /ImageMask true", &samples[5]),
    ];
    let content = "q 48 0 0 32 4 4 cm /A Do Q q 36 0 0 30 60 4 cm /B Do Q \
                   q 30 0 0 18 100 4 cm /C Do Q q 36 0 0 12 140 4 cm /D Do Q \
                   q 74 0 0 40 4 50 cm /E Do Q 1 0 0 rg q 74 0 0 40 100 50 cm /F Do Q";
    let resources = "<< /XObject << /A 5 0 R /B 6 0 R /C 7 0 R /D 8 0 R /E 9 0 R /F 10 0 R >> >>";
    let bytes = pdf(content, 200, 100, resources, &objects);
    let original = Document::open(bytes.clone()).expect("opens");
    let pixels = render(&original);

    for codec in [
        BilevelCodec::Flate,
        BilevelCodec::CcittG4,
        BilevelCodec::Jbig2Generic,
    ] {
        let (document, report) = saved(bytes.clone(), &recode(ContinuousCodec::Flate, codec));
        assert!(
            report.untouched.is_empty(),
            "{codec:?}: {:?}",
            report.untouched
        );
        assert_eq!(report.recoded.len(), 6, "{codec:?}");
        let cos = document.cos();
        for (index, want) in samples.iter().enumerate() {
            let number = 5 + index as u32;
            let dict = dict_of(cos, number);
            let filter = name_of(cos, &dict, b"Filter");
            let bilevel_image = index >= 4;
            if !bilevel_image {
                assert_eq!(filter.as_deref(), Some("FlateDecode"), "image {number}");
                let got = cos.stream_decoded(ObjRef::new(number, 0)).expect("decodes");
                assert_eq!(&got, want, "{codec:?}: image {number}'s samples, exactly");
                continue;
            }
            let coded = cos
                .stream_image_input(ObjRef::new(number, 0))
                .expect("bytes");
            let got = match codec {
                BilevelCodec::Flate => {
                    assert_eq!(filter.as_deref(), Some("FlateDecode"));
                    cos.stream_decoded(ObjRef::new(number, 0)).expect("decodes")
                }
                BilevelCodec::CcittG4 => {
                    assert_eq!(filter.as_deref(), Some("CCITTFaxDecode"));
                    let parms = dict
                        .get_dict(tinker_pdf_cos::Name::DECODE_PARMS)
                        .expect("parameters");
                    assert_eq!(int_of(cos, parms, b"K"), Some(-1), "G4");
                    assert_eq!(int_of(cos, parms, b"Columns"), Some(37));
                    tinker_pdf_filters::ccitt_decode(
                        &coded,
                        &tinker_pdf_filters::CcittParams {
                            k: -1,
                            columns: 37,
                            rows: 20,
                            ..tinker_pdf_filters::CcittParams::default()
                        },
                        1 << 20,
                    )
                    .0
                }
                _ => {
                    assert_eq!(filter.as_deref(), Some("JBIG2Decode"));
                    let mut warnings = Vec::new();
                    let decoded = tinker_pdf_filters::jbig2_decode(
                        &coded,
                        &tinker_pdf_filters::Jbig2Params {
                            globals: &[],
                            width: 37,
                            height: 20,
                        },
                        1 << 20,
                        &mut warnings,
                    )
                    .expect("the region decodes");
                    assert!(warnings.is_empty(), "{warnings:?}");
                    // T.88 codes 1 for black; a one-bit sample is 0 for black.
                    decoded.iter().map(|b| !b).collect()
                }
            };
            assert_eq!(
                bits(&got, 37, 20),
                bits(want, 37, 20),
                "{codec:?}: image {number}'s bits, exactly"
            );
        }
        assert_eq!(
            render(&document),
            pixels,
            "{codec:?}: the page draws the same pixels"
        );
    }
}

// ---------------------------------------------------------------------------
// Downsampling.
// ---------------------------------------------------------------------------

/// The means of `factor`-sized blocks over interleaved eight-bit samples,
/// rounded half up — `floor((2 * sum + count) / (2 * count))` — and the size.
fn means(
    samples: &[u8],
    width: usize,
    height: usize,
    n: usize,
    (fx, fy): (usize, usize),
) -> (Vec<u8>, usize, usize) {
    let (ow, oh) = (width.div_ceil(fx), height.div_ceil(fy));
    let mut out = Vec::new();
    for oy in 0..oh {
        for ox in 0..ow {
            for c in 0..n {
                let (mut sum, mut count) = (0u64, 0u64);
                for y in oy * fy..((oy + 1) * fy).min(height) {
                    for x in ox * fx..((ox + 1) * fx).min(width) {
                        sum += u64::from(samples[(y * width + x) * n + c]);
                        count += 1;
                    }
                }
                out.push(((2 * sum + count) / (2 * count)) as u8);
            }
        }
    }
    (out, ow, oh)
}

/// **Downsampling is an exact integer box filter at the finest placement.**
/// A grey image 10 x 6 drawn twice — once 72 x 36 points, which is 10 ppi
/// across and 12 ppi down, once twice that — and an RGB image 9 x 7 drawn only
/// inside a form whose `/Matrix` halves it, so the form is walked; at a
/// maximum of 4 ppi the factors are ceil(10 / 4) = 3 and ceil(12 / 4) = 3 for
/// the grey image, whose output is 4 x 2 with a one-column partial block on
/// the right, and the RGB image's are worked out the same way.
#[test]
fn downsampling_is_an_exact_box_filter_at_the_finest_placement() {
    let g = grey(10, 6);
    let c = rgb(9, 7);
    let form = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 400 400] /Matrix [0.5 0 0 0.5 0 0] \
         /Resources << /XObject << /C 6 0 R >> >>",
        // 9 samples over 108 x 0.5 = 54 points is 12 ppi; 7 over 28 points, 18.
        b"q 108 0 0 56 0 0 cm /C Do Q",
    );
    let bytes = pdf(
        "q 72 0 0 36 0 0 cm /G Do Q q 144 0 0 72 50 20 cm /G Do Q /F Do",
        300,
        200,
        "<< /XObject << /G 5 0 R /F 7 0 R >> >>",
        &[
            hex_image(
                "/Width 10 /Height 6 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &g,
            ),
            hex_image(
                "/Width 9 /Height 7 /ColorSpace /DeviceRGB /BitsPerComponent 8",
                &c,
            ),
            form,
        ],
    );
    let options = SaveOptions {
        images: ImagePolicy::Recode(
            ImageRecoding::new(ContinuousCodec::Keep, BilevelCodec::Keep).with_max_ppi(4.0),
        ),
        ..SaveOptions::default()
    };
    let (document, report) = saved(bytes, &options);
    let cos = document.cos();

    let (want, w, h) = means(&g, 10, 6, 1, (3, 3));
    assert_eq!((w, h), (4, 2));
    let grey_row = report
        .recoded
        .iter()
        .find(|r| r.image.num == 5)
        .expect("the grey image was resampled");
    assert_eq!(grey_row.size, (10, 6));
    assert_eq!(grey_row.resized, (4, 2));
    assert_eq!(
        grey_row.coding,
        ImageCoding::Flate,
        "`Keep` deflates what it resamples"
    );
    assert_eq!(grey_row.resolution_kept, None);
    let dict = dict_of(cos, 5);
    assert_eq!(
        (int_of(cos, &dict, b"Width"), int_of(cos, &dict, b"Height")),
        (Some(4), Some(2))
    );
    assert_eq!(
        cos.stream_decoded(ObjRef::new(5, 0)).expect("decodes"),
        want
    );

    // 12 ppi across and 18 down at 4: factors 3 and 5, so 3 x 2.
    let (want, w, h) = means(&c, 9, 7, 3, (3, 5));
    assert_eq!((w, h), (3, 2));
    let colour_row = report
        .recoded
        .iter()
        .find(|r| r.image.num == 6)
        .expect("the form's image was resampled");
    assert_eq!(
        colour_row.resized,
        (3, 2),
        "the placement inside the form was the one measured"
    );
    assert_eq!(
        cos.stream_decoded(ObjRef::new(6, 0)).expect("decodes"),
        want
    );
}

// ---------------------------------------------------------------------------
// JPEG.
// ---------------------------------------------------------------------------

/// T.81 A.3.3's basis, `C(u)/2 cos((2x+1) u pi / 16)`.
fn basis(x: usize, u: usize) -> f64 {
    let c = if u == 0 {
        std::f64::consts::FRAC_1_SQRT_2
    } else {
        1.0
    };
    c / 2.0 * ((2.0 * x as f64 + 1.0) * u as f64 * std::f64::consts::PI / 16.0).cos()
}

/// The same, as a coder holding it at 1/16384 holds it.
fn held(x: usize, u: usize) -> f64 {
    (basis(x, u) * 16384.0).round() / 16384.0
}

/// The largest error a baseline round trip can put on each sample of one 8x8
/// plane, from the caller's table `q` (natural order) and the plane's values
/// `p` (`margin` either side of them).
///
/// - The quantiser rounds each coefficient to the nearest multiple of its
///   `Q`, so the coefficient comes back within `Q / 2`, plus `eF`, what the
///   encoder's 1/16384 basis can move the coefficient before it rounds.
/// - That error reaches a sample through the inverse DCT,
///   `sum (Q/2 + eF) |T(x,u) T(y,v)|`, which is A.3.3's equation applied to
///   the error rather than to the coefficient.
/// - A decoder holding the basis at 1/16384 moves the result by at most
///   `sum |F'| |T~ T~ - T T|`, where `|F'| <= |F| + Q/2 + eF`.
/// - A separable integer IDCT that floors after each pass loses less than one
///   unit per row pass, carried through the column pass (`sum_v |T~(y,v)|`),
///   and less than one more at the end.
fn plane_bound(q: &[u8; 64], p: &[[f64; 8]; 8], margin: f64) -> [[f64; 8]; 8] {
    let mut bound = [[0.0; 8]; 8];
    let shifted = |x: usize, y: usize| p[y][x] - 128.0;
    // The coefficients of the plane as T.81 defines them, and how far a
    // 1/16384 basis can move each one.
    let mut f = [[0.0f64; 8]; 8];
    let mut e = [[0.0f64; 8]; 8];
    for v in 0..8 {
        for u in 0..8 {
            for y in 0..8 {
                for x in 0..8 {
                    let s = shifted(x, y);
                    f[v][u] += s * basis(x, u) * basis(y, v);
                    e[v][u] += (s.abs() + margin)
                        * (held(x, u) * held(y, v) - basis(x, u) * basis(y, v)).abs();
                }
                // The margin's own reach into the coefficient.
            }
            let reach: f64 = (0..8)
                .flat_map(|y| (0..8).map(move |x| (basis(x, u) * basis(y, v)).abs()))
                .sum();
            f[v][u] = f[v][u].abs() + margin * reach;
        }
    }
    for y in 0..8 {
        for x in 0..8 {
            let mut total = 1.0;
            for v in 0..8 {
                total += held(y, v).abs();
                for u in 0..8 {
                    let half = f64::from(q[v * 8 + u]) / 2.0 + e[v][u];
                    total += half * (basis(x, u) * basis(y, v)).abs();
                    total += (f[v][u] + half)
                        * (held(x, u) * held(y, v) - basis(x, u) * basis(y, v)).abs();
                }
            }
            bound[y][x] = total;
        }
    }
    bound
}

/// The 8x8 block of a `width`-wide plane at block `(bx, by)`.
fn block(plane: &[f64], width: usize, bx: usize, by: usize) -> [[f64; 8]; 8] {
    let mut out = [[0.0; 8]; 8];
    for (y, row) in out.iter_mut().enumerate() {
        for (x, value) in row.iter_mut().enumerate() {
            *value = plane[(by * 8 + y) * width + bx * 8 + x];
        }
    }
    out
}

/// Figure A.6's zig-zag: the natural index of the `k`th coefficient.
const ZIGZAG: [usize; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];

/// The tables a JPEG's DQT segments carry, by destination, in natural order,
/// and each component's destination from SOF0.
fn tables_in(jpeg: &[u8]) -> (Vec<(u8, [u8; 64])>, Vec<u8>) {
    let (mut tables, mut components) = (Vec::new(), Vec::new());
    let mut at = 2;
    while at + 4 <= jpeg.len() && jpeg[at] == 0xFF {
        let marker = jpeg[at + 1];
        let length = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        let body = &jpeg[at + 4..at + 2 + length];
        match marker {
            0xDB => {
                for table in body.chunks(65) {
                    let mut natural = [0u8; 64];
                    for (k, value) in table[1..].iter().enumerate() {
                        natural[ZIGZAG[k]] = *value;
                    }
                    tables.push((table[0] & 0x0F, natural));
                }
            }
            0xC0 => {
                let count = usize::from(body[5]);
                components = (0..count).map(|i| body[6 + i * 3 + 2]).collect();
            }
            0xDA => break,
            _ => {}
        }
        at += 2 + length;
    }
    (tables, components)
}

/// A textured plane — noise over a ramp, which a coarse table would visibly
/// get wrong — so a bound computed from a fine table is a real constraint.
fn textured(width: usize, height: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..width * height)
        .map(|i| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let (x, y) = (i % width, i / width);
            ((x * 6 + y * 4) as u64 % 160 + state % 64) as u8
        })
        .collect()
}

/// **JPEG decodes within the error the caller's tables bound.** A grey and an
/// RGB image, 64 x 64 and textured, coded with the caller's own tables at
/// 4:4:4: the DQT segments carry exactly those tables, and every sample read
/// back is within the bound worked out above from them — for RGB, through
/// T.871's inverse transform: the encoder's rounding of each of `Y`, `Cb`,
/// `Cr` (half a level and its fixed point), each plane's bound, and the
/// decoder's truncation to a byte.
#[test]
fn jpeg_decodes_within_the_bound_the_callers_tables_give() {
    let mut luminance = [0u8; 64];
    let mut chrominance = [0u8; 64];
    for (k, (l, c)) in luminance.iter_mut().zip(chrominance.iter_mut()).enumerate() {
        let (u, v) = (k % 8, k / 8);
        *l = (2 + u + v) as u8;
        *c = (3 + 2 * (u + v)) as u8;
    }
    let tables = JpegTables {
        luminance,
        chrominance,
        subsampled: false,
    };
    let g = textured(64, 64, 0x1234_5678);
    let c: Vec<u8> = textured(64, 64, 0x9E37_79B9)
        .into_iter()
        .zip(textured(64, 64, 0x7F4A_7C15))
        .zip(textured(64, 64, 0x0BAD_F00D))
        .flat_map(|((r, g), b)| [r, g, b])
        .collect();
    let bytes = pdf(
        "q 72 0 0 72 0 0 cm /G Do Q q 72 0 0 72 100 0 cm /C Do Q",
        200,
        100,
        "<< /XObject << /G 5 0 R /C 6 0 R >> >>",
        &[
            hex_image(
                "/Width 64 /Height 64 /ColorSpace /DeviceGray /BitsPerComponent 8",
                &g,
            ),
            hex_image(
                "/Width 64 /Height 64 /ColorSpace /DeviceRGB /BitsPerComponent 8",
                &c,
            ),
        ],
    );
    let (document, report) = saved(
        bytes,
        &recode(ContinuousCodec::Jpeg(tables), BilevelCodec::Keep),
    );
    assert_eq!(report.recoded.len(), 2, "{report:?}");
    assert!(report.recoded.iter().all(|r| r.coding == ImageCoding::Jpeg));
    let cos = document.cos();

    // The grey image.
    let jpeg = cos.stream_raw(ObjRef::new(5, 0)).expect("bytes");
    assert_eq!(
        name_of(cos, &dict_of(cos, 5), b"Filter").as_deref(),
        Some("DCTDecode")
    );
    let (written, components) = tables_in(&jpeg);
    assert_eq!(components, vec![0]);
    assert_eq!(written, vec![(0, luminance)], "the caller's table, exactly");
    let decoded = tinker_pdf_filters::jpeg_decode(&jpeg, 1 << 20).expect("decodes");
    let plane: Vec<f64> = g.iter().map(|&v| f64::from(v)).collect();
    let mut worst = 0.0f64;
    for by in 0..8 {
        for bx in 0..8 {
            let bound = plane_bound(&luminance, &block(&plane, 64, bx, by), 0.0);
            for y in 0..8 {
                for x in 0..8 {
                    let i = (by * 8 + y) * 64 + bx * 8 + x;
                    let error = (f64::from(decoded.data[i]) - plane[i]).abs();
                    assert!(
                        error <= bound[y][x],
                        "grey ({}, {}): {error} past the bound {}",
                        bx * 8 + x,
                        by * 8 + y,
                        bound[y][x]
                    );
                    worst = worst.max(error);
                }
            }
        }
    }
    assert!(worst > 0.0, "it is lossy: this is a JPEG and not a copy");

    // The RGB image, through T.871 clause 7's exact transform.
    let jpeg = cos.stream_raw(ObjRef::new(6, 0)).expect("bytes");
    let (written, components) = tables_in(&jpeg);
    let table = |id: u8| written.iter().find(|(d, _)| *d == id).map(|(_, t)| *t);
    assert_eq!(components.len(), 3);
    assert_eq!(
        table(components[0]),
        Some(luminance),
        "Y through the luminance table"
    );
    assert_eq!(
        table(components[1]),
        Some(chrominance),
        "Cb through the chrominance table"
    );
    assert_eq!(table(components[2]), Some(chrominance), "Cr likewise");
    let decoded = tinker_pdf_filters::jpeg_decode(&jpeg, 1 << 20).expect("decodes");
    let pixel = |i: usize| {
        (
            f64::from(c[i * 3]),
            f64::from(c[i * 3 + 1]),
            f64::from(c[i * 3 + 2]),
        )
    };
    let luma = |(r, g, b): (f64, f64, f64)| 0.299 * r + 0.587 * g + 0.114 * b;
    let planes: [Vec<f64>; 3] = [
        (0..4096).map(|i| luma(pixel(i))).collect(),
        (0..4096)
            .map(|i| (pixel(i).2 - luma(pixel(i))) / 1.772 + 128.0)
            .collect(),
        (0..4096)
            .map(|i| (pixel(i).0 - luma(pixel(i))) / 1.402 + 128.0)
            .collect(),
    ];
    // The encoder rounds each plane to a byte (half a level) in fixed point
    // (a hundredth more is generous), and clamps, which only moves a value
    // towards the range the original is in.
    let margin = 0.51;
    for by in 0..8 {
        for bx in 0..8 {
            let b: Vec<[[f64; 8]; 8]> = (0..3)
                .map(|k| {
                    let q = if k == 0 { &luminance } else { &chrominance };
                    plane_bound(q, &block(&planes[k], 64, bx, by), margin)
                })
                .collect();
            for y in 0..8 {
                for x in 0..8 {
                    let i = (by * 8 + y) * 64 + bx * 8 + x;
                    let (dy, dcb, dcr) = (
                        b[0][y][x] + margin,
                        b[1][y][x] + margin,
                        b[2][y][x] + margin,
                    );
                    // T.871: R = Y + 1.402 (Cr - 128), G = Y - 0.344136 (Cb -
                    // 128) - 0.714136 (Cr - 128), B = Y + 1.772 (Cb - 128);
                    // and a decoder that truncates to a byte loses up to one
                    // level more (a thousandth for its arithmetic).
                    let limits = [
                        dy + 1.402 * dcr + 1.001,
                        dy + 0.344_137 * dcb + 0.714_137 * dcr + 1.001,
                        dy + 1.772 * dcb + 1.001,
                    ];
                    let (r, g, bl) = pixel(i);
                    for (k, original) in [r, g, bl].into_iter().enumerate() {
                        let error = (f64::from(decoded.data[i * 3 + k]) - original).abs();
                        assert!(
                            error <= limits[k],
                            "RGB ({}, {}) component {k}: {error} past {}",
                            bx * 8 + x,
                            by * 8 + y,
                            limits[k]
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// What is left whole.
// ---------------------------------------------------------------------------

/// **What a coding or a resample cannot keep is left whole and named by
/// object reference**, and what can be kept is: a JPEG-coded original (its
/// codec is not decoded here), a colour-key mask, an indexed image, a CMYK
/// image and a sixteen-bit one, each asked for JPEG; a soft mask asked for
/// JPEG, which is coded losslessly instead; a `/Matte` pair and an image only
/// a tiling pattern draws, asked to downsample, recoded at their own
/// resolution and saying why; and an image already deflated, which deflates
/// to no smaller.
#[test]
fn what_cannot_be_kept_is_left_whole_by_name() {
    let jpeg = tinker_pdf_filters::jpeg_encode(
        &tinker_pdf_filters::JpegSource {
            width: 16,
            height: 8,
            colour: tinker_pdf_filters::JpegSourceColour::Gray,
            stride: 16,
            data: &grey(16, 8),
        },
        &tinker_pdf_filters::JpegOptions::default(),
    )
    .expect("a JPEG");
    let deflated = tinker_pdf_filters::zlib_compress(&grey(64, 48));
    let cell = stream(
        "/PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 \
         /Resources << /XObject << /T 14 0 R >> >>",
        b"q 10 0 0 10 0 0 cm /T Do Q",
    );
    let objects = vec![
        // 5: a JPEG original.
        stream(
            "/Type /XObject /Subtype /Image /Width 16 /Height 8 /ColorSpace /DeviceGray \
             /BitsPerComponent 8 /Filter /DCTDecode",
            &jpeg,
        ),
        // 6: a colour-key mask.
        hex_image(
            "/Width 64 /Height 48 /ColorSpace /DeviceGray /BitsPerComponent 8 /Mask [0 10]",
            &grey(64, 48),
        ),
        // 7: indexed.
        hex_image(
            "/Width 12 /Height 4 /ColorSpace [/Indexed /DeviceRGB 3 <ff000000ff000000ffffff00>] \
             /BitsPerComponent 8",
            &(0..48).map(|i| (i % 4) as u8).collect::<Vec<u8>>(),
        ),
        // 8: CMYK.
        hex_image(
            "/Width 6 /Height 4 /ColorSpace /DeviceCMYK /BitsPerComponent 8",
            &(0..96).map(|i| (i * 3 % 256) as u8).collect::<Vec<u8>>(),
        ),
        // 9: sixteen bits.
        hex_image(
            "/Width 6 /Height 4 /ColorSpace /DeviceGray /BitsPerComponent 16",
            &(0..48).map(|i| (i * 5 % 256) as u8).collect::<Vec<u8>>(),
        ),
        // 10: an image with a soft mask (11), which is asked for JPEG.
        hex_image(
            "/Width 64 /Height 48 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask 11 0 R",
            &grey(64, 48),
        ),
        // 11: the soft mask.
        hex_image(
            "/Width 64 /Height 48 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &grey(64, 48),
        ),
        // 12: already deflated, as this build deflates, and drawn by nothing.
        stream(
            "/Type /XObject /Subtype /Image /Width 64 /Height 48 /ColorSpace /DeviceGray \
             /BitsPerComponent 8 /Filter /FlateDecode",
            &deflated,
        ),
        // 13: the pattern.
        cell,
        // 14: an image only the pattern draws.
        hex_image(
            "/Width 64 /Height 48 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &grey(64, 48),
        ),
        // 15: an image whose soft mask (16) carries /Matte.
        hex_image(
            "/Width 64 /Height 48 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask 16 0 R",
            &grey(64, 48),
        ),
        // 16: that soft mask.
        hex_image(
            "/Width 64 /Height 48 /ColorSpace /DeviceGray /BitsPerComponent 8 /Matte [0.5]",
            &grey(64, 48),
        ),
    ];
    let content = "q 10 0 0 10 0 0 cm /A Do /B Do /C Do /D Do /E Do /F Do /M Do Q \
                   /Pattern cs /P scn 50 50 40 40 re f";
    let resources = "<< /XObject << /A 5 0 R /B 6 0 R /C 7 0 R /D 8 0 R /E 9 0 R /F 10 0 R \
                     /H 12 0 R /M 15 0 R >> /Pattern << /P 13 0 R >> >>";
    let bytes = pdf(content, 200, 100, resources, &objects);

    let reason = |report: &ImageReport, number: u32| {
        report
            .untouched
            .iter()
            .find(|u| u.image == ObjRef::new(number, 0))
            .map(|u| u.reason.clone())
    };
    let recoded = |report: &ImageReport, number: u32| {
        report
            .recoded
            .iter()
            .find(|r| r.image == ObjRef::new(number, 0))
            .cloned()
    };

    // JPEG asked of everything continuous.
    let mut luminance = [4u8; 64];
    luminance[0] = 2;
    let tables = JpegTables {
        luminance,
        chrominance: [6; 64],
        subsampled: false,
    };
    let (_, report) = saved(
        bytes.clone(),
        &recode(ContinuousCodec::Jpeg(tables), BilevelCodec::Keep),
    );
    assert_eq!(
        reason(&report, 5),
        Some(UntouchedImageReason::Filter {
            name: "DCTDecode".into()
        })
    );
    assert_eq!(
        reason(&report, 6),
        Some(UntouchedImageReason::ColourKeyMask)
    );
    assert_eq!(reason(&report, 7), Some(UntouchedImageReason::Indexed));
    assert_eq!(
        reason(&report, 8),
        Some(UntouchedImageReason::Components { count: 4 })
    );
    assert_eq!(
        reason(&report, 9),
        Some(UntouchedImageReason::Depth { bits: 16 })
    );
    assert_eq!(
        recoded(&report, 10).map(|r| r.coding),
        Some(ImageCoding::Jpeg)
    );
    assert_eq!(
        recoded(&report, 11).map(|r| r.coding),
        Some(ImageCoding::Flate),
        "a soft mask is coverage, and is coded losslessly"
    );
    // A ramp deflates to less than a JPEG's own Huffman tables.
    assert_eq!(reason(&report, 12), Some(UntouchedImageReason::NotSmaller));

    // Deflate and a resolution asked of everything.
    let options = SaveOptions {
        images: ImagePolicy::Recode(
            ImageRecoding::new(ContinuousCodec::Flate, BilevelCodec::Keep).with_max_ppi(1.0),
        ),
        ..SaveOptions::default()
    };
    let (_, report) = saved(bytes.clone(), &options);
    assert_eq!(
        reason(&report, 12),
        Some(UntouchedImageReason::NotSmaller),
        "deflate of what this build deflated is the same bytes, and nothing \
         draws it, so it is not resampled into something smaller"
    );
    let kept = |number: u32| recoded(&report, number).and_then(|r| r.resolution_kept);
    assert_eq!(kept(6), Some(UntouchedImageReason::ColourKeyMask));
    assert_eq!(kept(7), Some(UntouchedImageReason::Indexed));
    assert_eq!(kept(9), Some(UntouchedImageReason::Depth { bits: 16 }));
    assert_eq!(
        kept(14),
        Some(UntouchedImageReason::Unplaced),
        "a pattern cell is not walked"
    );
    assert_eq!(kept(15), Some(UntouchedImageReason::Matte));
    assert_eq!(kept(16), Some(UntouchedImageReason::Matte));
    // A placed eight-bit image is resampled: 64 samples over 10 points is
    // 460.8 ppi, so 1 ppi takes every sample of a row into one.
    assert_eq!(recoded(&report, 10).map(|r| r.resized), Some((1, 1)));
    // And its soft mask follows it, placed where its image is.
    assert_eq!(recoded(&report, 11).map(|r| r.resized), Some((1, 1)));

    // A resolution and no coding: what keeps the resolution is the answer for
    // an image the pass then leaves alone, not "nothing was asked".
    let options = SaveOptions {
        images: ImagePolicy::Recode(
            ImageRecoding::new(ContinuousCodec::Keep, BilevelCodec::Keep).with_max_ppi(1.0),
        ),
        ..SaveOptions::default()
    };
    let (_, report) = saved(bytes, &options);
    assert_eq!(
        reason(&report, 6),
        Some(UntouchedImageReason::ColourKeyMask)
    );
    assert_eq!(reason(&report, 14), Some(UntouchedImageReason::Unplaced));
    assert_eq!(
        recoded(&report, 10).map(|r| r.coding),
        Some(ImageCoding::Flate)
    );
}

/// **An appended save says the originals are still in the file**, as the
/// font pass does: the recoded streams are what a reader draws, and the
/// prefix still holds every stored one.
#[test]
fn an_incremental_save_says_the_originals_remain() {
    let bytes = pdf(
        "q 72 0 0 36 0 0 cm /A Do Q",
        200,
        100,
        "<< /XObject << /A 5 0 R >> >>",
        &[hex_image(
            "/Width 24 /Height 16 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &grey(24, 16),
        )],
    );
    let document = Document::open(bytes.clone()).expect("opens");
    let mut editor = document.editor();
    let out = save(
        &mut editor,
        &SaveOptions {
            write: WriteOptions {
                mode: WriteMode::Incremental,
                ..WriteOptions::default()
            },
            ..recode(ContinuousCodec::Flate, BilevelCodec::Keep)
        },
    );
    let ImageOutcome::RecodedButTheOriginalsRemain(report) = &out.images else {
        panic!("{:?}", out.images)
    };
    assert_eq!(report.recoded.len(), 1);
    assert!(
        out.bytes.starts_with(&bytes),
        "7.5.6: the original is the prefix"
    );
    let reopened = Document::open(out.bytes.clone()).expect("reopens");
    assert_eq!(
        reopened
            .cos()
            .stream_decoded(ObjRef::new(5, 0))
            .expect("decodes"),
        grey(24, 16)
    );
}

/// **The pass never panics on a hostile document** (ruling 1): mutated
/// versions of the fixtures above — bytes flipped, runs deleted, numbers
/// replaced — saved with every coding and a resolution, and each save must
/// reopen or be a file this reader refuses by name.
#[test]
fn a_hostile_document_never_panics_the_pass() {
    let pages = vec![
        pdf(
            "q 72 0 0 36 0 0 cm /A Do Q q 74 0 0 40 4 50 cm /E Do Q /F Do",
            200,
            100,
            "<< /XObject << /A 5 0 R /E 6 0 R /F 7 0 R >> >>",
            &[
                hex_image(
                    "/Width 24 /Height 16 /ColorSpace [/ICCBased 8 0 R] /BitsPerComponent 8 \
                     /Decode [1 0]",
                    &grey(24, 16),
                ),
                hex_image("/Width 37 /Height 20 /ImageMask true", &bilevel(37, 20)),
                stream(
                    "/Type /XObject /Subtype /Form /BBox [0 0 9 9] /Matrix [2 0 0 2 0 0] \
                     /Resources << /XObject << /F 7 0 R /A 5 0 R >> >>",
                    b"/F Do q 9 0 0 9 0 0 cm /A Do Q",
                ),
                stream("/N 1", b""),
            ],
        ),
        pdf(
            "q 1e300 0 0 0 0 0 cm /A Do Q q 0 0 0 0 0 0 cm /A Do Q",
            50,
            50,
            "<< /XObject << /A 5 0 R >> >>",
            &[hex_image(
                "/Width 3 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /SMask 5 0 R",
                &rgb(3, 2),
            )],
        ),
    ];
    let mut jpeg = [1u8; 64];
    jpeg[63] = 0;
    let policies = [
        ImageRecoding::new(ContinuousCodec::Flate, BilevelCodec::CcittG4).with_max_ppi(3.0),
        ImageRecoding::new(
            ContinuousCodec::Jpeg(JpegTables {
                luminance: [3; 64],
                chrominance: jpeg,
                subsampled: true,
            }),
            BilevelCodec::Jbig2Generic,
        )
        .with_max_ppi(f64::NAN),
        ImageRecoding::new(ContinuousCodec::Keep, BilevelCodec::Flate).with_max_ppi(1e-300),
    ];
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut ran = 0usize;
    for page in &pages {
        for round in 0..45 {
            let mut bytes = page.clone();
            for _ in 0..1 + next() % 5 {
                let at = (next() as usize) % bytes.len().max(1);
                match next() % 4 {
                    0 => bytes[at] ^= 1 << (next() % 8),
                    1 => {
                        let end = (at + 1 + next() as usize % 12).min(bytes.len());
                        bytes.drain(at..end);
                    }
                    2 => bytes
                        .splice(at..at, b" 99999 ".iter().copied())
                        .for_each(drop),
                    _ => bytes
                        .splice(at..at, b" -1 0.5 ".iter().copied())
                        .for_each(drop),
                }
                if bytes.is_empty() {
                    bytes.push(b' ');
                }
            }
            let Ok(document) = Document::open(bytes) else {
                continue;
            };
            let mut editor = document.editor();
            let options = SaveOptions {
                images: ImagePolicy::Recode(policies[round % policies.len()].clone()),
                ..SaveOptions::default()
            };
            let out = save(&mut editor, &options);
            ran += 1;
            let _ = Document::open(out.bytes);
        }
    }
    assert!(ran > 30, "the campaign reached the pass {ran} times");
}
