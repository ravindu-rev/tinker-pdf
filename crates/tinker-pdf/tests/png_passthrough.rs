//! The two routes a PNG takes into a page are one picture (gap 29, milestone 4).
//!
//! # What this file is really asserting, and what it cannot
//!
//! Gap 18 milestone 7's scar is a precision shift that passed four boundary
//! tests because the comparative ones rendered two dictionaries **through the
//! same defect**. So it is worth writing down exactly which parts of these two
//! paths are shared and which are not, rather than claiming a comparison proves
//! more than it does.
//!
//! Shared, and therefore invisible here:
//!
//! - `predictors.rs`. Both routes unfilter through `predictor_decode` with
//!   `/Predictor 15` — the reader on one side, `png.rs` on the other — because
//!   PDF's predictors *are* PNG 9.2's row filters. A Paeth bug would move both
//!   pictures together. `predictors.rs` has its own tests, and PngSuite's
//!   `f00`..`f99` files exercise the five filters against another encoder.
//! - `inflate.rs`, for the same reason.
//!
//! Not shared, and therefore what these tests are for:
//!
//! - **`/DecodeParms`.** The pass-through hands the reader `/Colors`,
//!   `/BitsPerComponent` and `/Columns` and the decoder derives its own from
//!   IHDR. `the_comparison_notices_when_a_predictor_parameter_is_wrong` builds
//!   the three wrong versions by hand and shows each renders differently.
//! - **The colour space.** An indexed file passes through as
//!   `[/Indexed /DeviceRGB ...]` and is resolved by `tinker-pdf-color`; the
//!   decoded twin has had its palette applied by `png.rs`. Two different pieces
//!   of code, one picture.
//! - **Sub-byte depths.** The reader divides a raw sample by `2^d - 1`; the
//!   decoder multiplies by 255, 85 or 17 (13.12). Those are the same number by
//!   two different routes, and nothing but a comparison says so.
//! - **`/Mask` against `/SMask`.** A `tRNS` becomes a colour key on one route
//!   and an alpha channel on the other, and 8.9.6.4 and 11.6.5.3 are entirely
//!   separate code.
//! - **Adam7.** An interlaced file cannot pass through, so its non-interlaced
//!   twin is its comparison — which puts the whole decoded route, split and
//!   re-deflate included, against the whole pass-through.
//!
//! Every fixture is built here from ISO/IEC 15948's own field layouts, and the
//! expected pixel values are written out rather than read back.

use tinker_pdf::{Bitmap, Document, RenderOptions};
use tinker_pdf_cos::{
    png_image, CompressedImage, DocumentBuilder, ImageColorSpace, ImageData, PngRoute,
};
use tinker_pdf_filters::{png_decode, png_scan, Crc32, Limits, PngColour, PNG_SIGNATURE};

const CAP: Limits = Limits::new(1 << 22);

// --- building PNGs ------------------------------------------------------

/// 5.3: length, type, data, CRC over the type and the data.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut c = Crc32::new();
    c.update(kind);
    c.update(data);
    out.extend_from_slice(&c.finish().to_be_bytes());
    out
}

fn ihdr(width: u32, height: u32, depth: u8, colour: u8, interlace: u8) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&width.to_be_bytes());
    d.extend_from_slice(&height.to_be_bytes());
    d.extend_from_slice(&[depth, colour, 0, 0, interlace]);
    chunk(b"IHDR", &d)
}

fn png_file(header: Vec<u8>, middle: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::from(PNG_SIGNATURE);
    out.extend_from_slice(&header);
    for c in middle {
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&chunk(b"IEND", b""));
    out
}

/// PNG 9.4's Paeth predictor, written out so the encoder below is 9.2's own.
fn paeth(a: i32, b: i32, c: i32) -> i32 {
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// 9.2's five filters, cycling one per row.
///
/// A fixture written entirely with filter 0 would be a weaker stream than any
/// real encoder produces, and `/Predictor 15` exists precisely because the tag
/// changes from row to row.
fn filtered(rows: &[Vec<u8>], bpp: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev = vec![0u8; rows.first().map_or(0, Vec::len)];
    for (n, row) in rows.iter().enumerate() {
        let tag = (n % 5) as u8;
        out.push(tag);
        let mut encoded = vec![0u8; row.len()];
        for i in 0..row.len() {
            let a = if i >= bpp { i32::from(row[i - bpp]) } else { 0 };
            let b = i32::from(prev.get(i).copied().unwrap_or(0));
            let c = if i >= bpp {
                i32::from(prev.get(i - bpp).copied().unwrap_or(0))
            } else {
                0
            };
            let predictor = match tag {
                1 => a,
                2 => b,
                3 => (a + b) / 2,
                4 => paeth(a, b, c),
                _ => 0,
            };
            encoded[i] = (i32::from(row[i]) - predictor) as u8;
        }
        out.extend_from_slice(&encoded);
        prev.clone_from(row);
    }
    out
}

fn idat_of(raw: &[u8]) -> Vec<u8> {
    chunk(b"IDAT", &tinker_pdf_filters::zlib_compress(raw))
}

/// Bytes per pixel for 9.2's left-neighbour offset: `ceil(channels x d / 8)`,
/// at least one.
fn bpp_of(channels: usize, depth: u8) -> usize {
    ((channels * usize::from(depth)).div_ceil(8)).max(1)
}

/// A non-interlaced file from unpacked scanlines.
fn simple(
    width: u32,
    height: u32,
    depth: u8,
    colour: u8,
    rows: &[Vec<u8>],
    extra: &[Vec<u8>],
) -> Vec<u8> {
    let channels = match colour {
        2 => 3usize,
        4 => 2,
        6 => 4,
        _ => 1,
    };
    let mut middle = extra.to_vec();
    middle.push(idat_of(&filtered(rows, bpp_of(channels, depth))));
    png_file(ihdr(width, height, depth, colour, 0), &middle)
}

// --- the fixtures -------------------------------------------------------

/// An 8 x 8 truecolour image in which no two pixels are alike.
///
/// Distinct everywhere on purpose: a picture whose rows or columns repeat lets a
/// transposition, an off-by-one stride or a row read from the wrong place come
/// out looking correct, which is `png.rs`'s reason for the same choice about
/// Adam7 and gap 17's about two swapped context bits.
fn rgb_rows() -> Vec<Vec<u8>> {
    (0..8u32)
        .map(|y| {
            (0..8u32)
                .flat_map(|x| {
                    let n = y * 8 + x;
                    [(n * 3) as u8, (255 - n * 2) as u8, (n * n % 251) as u8]
                })
                .collect()
        })
        .collect()
}

// --- rendering both routes ----------------------------------------------

fn render(bytes: Vec<u8>) -> Bitmap {
    Document::open(bytes)
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::default())
}

/// One page whose `/MediaBox` is the image's own pixel dimensions, with the
/// image filling it — gap 29's page geometry, and the only scale at which one
/// output pixel is one source pixel.
fn page_of(image: &ImageData<'_>, width: u32, height: u32) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_image(b"Im0", image), "the builder took it");
    let (w, h) = (f64::from(width), f64::from(height));
    builder.add_page(w, h, |page| page.image(b"Im0", 0.0, 0.0, w, h));
    builder.finish()
}

/// The pass-through route: `png_image`, whatever it chose.
fn embedded(png: &[u8]) -> (Bitmap, PngRoute) {
    let prepared = png_image(png, &CAP).expect("the fixture is a PNG");
    let bitmap = render(page_of(
        &prepared.image(),
        prepared.width(),
        prepared.height(),
    ));
    (bitmap, prepared.route())
}

/// The reference route: milestone 3's decoder, embedded as plain samples.
///
/// Deliberately *not* `png_image`: this is the other side of the comparison and
/// must not share the code under test. Eight-bit output goes in as
/// [`ImageData::Rgb8`] or [`ImageData::Gray8`], which is what milestone 4's exit
/// criterion names; sixteen-bit output has no such variant and goes in as raw
/// samples with no filter at all, so the reader reads them directly.
fn decoded_reference(png: &[u8]) -> Bitmap {
    let image = png_decode(png, &CAP).expect("the fixture decodes");
    let (width, height) = (image.width, image.height);
    match (image.colour, image.bits_per_component) {
        (PngColour::Rgb, 8) => render(page_of(
            &ImageData::Rgb8 {
                width,
                height,
                data: &image.data,
            },
            width,
            height,
        )),
        (PngColour::Grey, 8) => render(page_of(
            &ImageData::Gray8 {
                width,
                height,
                data: &image.data,
            },
            width,
            height,
        )),
        (colour, bits) => {
            let space = match colour {
                PngColour::Grey => ImageColorSpace::DeviceGray,
                PngColour::Rgb => ImageColorSpace::DeviceRgb,
                other => panic!("{other:?} carries alpha and needs an /SMask, not this path"),
            };
            render(page_of(
                &ImageData::Compressed(CompressedImage {
                    width,
                    height,
                    bits_per_component: bits,
                    color_space: space,
                    filter: None,
                    data: &image.data,
                    color_key_mask: None,
                    soft_mask: None,
                }),
                width,
                height,
            ))
        }
    }
}

/// Two bitmaps, compared byte for byte with a message that says where.
fn assert_same_picture(left: &Bitmap, right: &Bitmap, what: &str) {
    assert_eq!(
        (left.width, left.height),
        (right.width, right.height),
        "{what}: size"
    );
    let differing = left
        .data
        .iter()
        .zip(right.data.iter())
        .position(|(a, b)| a != b);
    assert!(
        differing.is_none(),
        "{what}: first difference at byte {:?} — {:?} against {:?}",
        differing,
        differing.map(|i| left.data.get(i)),
        differing.map(|i| right.data.get(i)),
    );
}

/// A page that painted nothing would make any comparison pass, which is the
/// determinism file's own scar about a fixture measuring nothing.
///
/// `least` is how many distinct pixel values the fixture may show and still be
/// a picture; a two-colour 1-bit image legitimately has two, and a four-entry
/// palette with half of it masked has three.
fn assert_painted(bitmap: &Bitmap, least: usize, what: &str) {
    let mut seen: Vec<&[u8]> = bitmap.data.chunks_exact(bitmap.components()).collect();
    seen.sort_unstable();
    seen.dedup();
    assert!(
        seen.len() >= least,
        "{what}: only {} distinct pixels, wanted {least} — this page is not a picture",
        seen.len()
    );
    assert!(
        seen.iter().any(|p| p.iter().any(|v| *v != 255)),
        "{what}: every pixel is the white the page started as"
    );
}

// --- the headline -------------------------------------------------------

/// Milestone 4's exit criterion: the same PNG, the two routes, one picture.
#[test]
fn a_passed_through_png_and_its_decode_are_one_picture() {
    let png = simple(8, 8, 8, 2, &rgb_rows(), &[]);

    let (through, route) = embedded(&png);
    assert_eq!(
        route,
        PngRoute::PassedThrough,
        "the default route, or this test is comparing a decode with a decode"
    );
    assert_painted(&through, 9, "the pass-through");

    let reference = decoded_reference(&png);
    assert_same_picture(&through, &reference, "truecolour 8-bit");
}

/// And the comparison would notice. Each of the three `/DecodeParms` numbers is
/// wrong in turn, in a hand-built file the writer would have refused.
///
/// Without this, "the two agree" is a claim about a comparison nobody has shown
/// to be sensitive — which is gap 18 milestone 7's failure exactly.
#[test]
fn the_comparison_notices_when_a_predictor_parameter_is_wrong() {
    let png = simple(8, 8, 8, 2, &rgb_rows(), &[]);
    let idat = png_scan(&png).expect("it scans").idat;
    let reference = decoded_reference(&png);

    let hand_built = |parms: &str| -> Bitmap {
        let mut file = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 8 8]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 25 >>\nstream\nq 8 0 0 8 0 0 cm /Im0 Do Q\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 8 /Height 8\n\
   /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode\n\
   {parms} /Length {} >>\nstream\n",
            idat.len()
        )
        .into_bytes();
        file.extend_from_slice(&idat);
        file.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Size 6 /Root 1 0 R >>\n%%EOF\n");
        render(file)
    };

    let right = "/DecodeParms << /Predictor 15 /Colors 3 /BitsPerComponent 8 /Columns 8 >>";
    assert_same_picture(
        &hand_built(right),
        &reference,
        "the hand-built control with the right parameters",
    );

    for wrong in [
        "/DecodeParms << /Predictor 15 /Colors 3 /BitsPerComponent 8 /Columns 7 >>",
        "/DecodeParms << /Predictor 15 /Colors 1 /BitsPerComponent 8 /Columns 8 >>",
        "/DecodeParms << /Predictor 15 /Colors 3 /BitsPerComponent 4 /Columns 8 >>",
        "/DecodeParms << /Predictor 2 /Colors 3 /BitsPerComponent 8 /Columns 8 >>",
        "",
    ] {
        let broken = hand_built(wrong);
        assert_ne!(
            broken.data, reference.data,
            "{wrong:?} produced the right picture, so this comparison proves nothing"
        );
    }

    // One value that is *not* a control, and finding out why is worth a line.
    // 7.4.4.4: any `/Predictor` of 10 or more means "PNG prediction", and the
    // filter actually used is the tag byte at the head of each row — so 12
    // ("PNG Up") and 15 ("PNG optimum") decode identically, and a reader that
    // honoured 12 as a fixed Up filter would be wrong. This started out in the
    // list above and belongs here instead.
    assert_eq!(
        hand_built("/DecodeParms << /Predictor 12 /Colors 3 /BitsPerComponent 8 /Columns 8 >>")
            .data,
        reference.data,
        "every PNG predictor from 10 up reads its filter from the row"
    );
}

// --- the shapes that pass through ---------------------------------------

/// Every non-interlaced colour type without interleaved alpha, at every depth
/// Table 11.1 pairs it with, against its own decode.
#[test]
fn every_pass_through_shape_agrees_with_its_decode() {
    // (colour type, bit depth). Colour types 4 and 6 are absent because they
    // cannot pass through at all; `every_colour_type_takes_the_route_the_design_table_says`
    // in `tinker-pdf-cos` holds that half.
    const SHAPES: [(u8, u8); 11] = [
        (0, 1),
        (0, 2),
        (0, 4),
        (0, 8),
        (0, 16),
        (2, 8),
        (2, 16),
        (3, 1),
        (3, 2),
        (3, 4),
        (3, 8),
    ];

    for (colour, depth) in SHAPES {
        let png = shape_fixture(colour, depth);
        let (through, route) = embedded(&png);
        assert_eq!(
            route,
            PngRoute::PassedThrough,
            "colour type {colour} depth {depth}"
        );
        assert_painted(&through, 2, &format!("colour type {colour} depth {depth}"));
        assert_same_picture(
            &through,
            &decoded_reference(&png),
            &format!("colour type {colour} depth {depth}"),
        );
    }
}

/// An 8 x 8 image of the given shape whose samples run over the whole range the
/// depth can hold, and — for an indexed image — over the whole palette.
fn shape_fixture(colour: u8, depth: u8) -> Vec<u8> {
    let width = 8u32;
    let height = 8u32;
    let channels = if colour == 2 { 3usize } else { 1 };
    let levels = 1u32 << depth.min(8);

    let mut extra = Vec::new();
    if colour == 3 {
        // A palette of every value the depth can index, each a different
        // colour, so a wrong index is a wrong colour rather than a near one.
        let entries = levels.min(256);
        let mut lookup = Vec::new();
        for i in 0..entries {
            lookup.extend_from_slice(&[
                (i * 37 % 256) as u8,
                (255 - i * 11 % 256) as u8,
                (i * 91 % 256) as u8,
            ]);
        }
        extra.push(chunk(b"PLTE", &lookup));
    }

    let mut rows = Vec::new();
    for y in 0..height {
        let mut samples = Vec::new();
        for x in 0..width {
            for c in 0..channels {
                let n = (y * width + x) * channels as u32 + c as u32;
                samples.push(match depth {
                    16 => u32::from((n * 2551) as u16),
                    _ => n % levels,
                });
            }
        }
        rows.push(pack(&samples, depth));
    }
    simple(width, height, depth, colour, &rows, &extra)
}

/// 7.2: samples packed most significant first, never straddling a byte.
fn pack(samples: &[u32], depth: u8) -> Vec<u8> {
    match depth {
        16 => samples
            .iter()
            .flat_map(|s| (*s as u16).to_be_bytes())
            .collect(),
        8 => samples.iter().map(|s| *s as u8).collect(),
        d => {
            let per = 8 / usize::from(d);
            let mut out = vec![0u8; samples.len().div_ceil(per)];
            for (i, s) in samples.iter().enumerate() {
                let shift = 8 - usize::from(d) * (i % per + 1);
                out[i / per] |= ((*s as u8) & ((1u8 << d) - 1)) << shift;
            }
            out
        }
    }
}

// --- 8.9.6.4 against 11.6.5.3 -------------------------------------------

/// A `tRNS` colour key and an alpha channel are one picture.
///
/// The left side is a truecolour file with a `tRNS`, which passes through
/// carrying `/Mask`; the right is the same image written as colour type 6 with
/// the keyed pixels at alpha zero, which is decoded, split and given an
/// `/SMask`. Nothing about 8.9.6.4 and 11.6.5.3 is shared code, so this is the
/// sharpest comparison in the file.
///
/// # The decoys are the point
///
/// 8.9.6.4 masks a sample only when **every** component falls inside its own
/// range. A fixture whose key colour is the only place any of those three values
/// ever appears cannot tell that rule from "any component inside its range",
/// because no pixel exists where the two answers differ. So the colours below
/// are built so that some pixels carry the key's red and not its green, some its
/// green alone and some two of the three — and the test asserts those pixels
/// exist before it compares anything.
#[test]
fn a_colour_key_and_a_soft_mask_are_one_picture() {
    let key = [0x20u8, 0x40, 0x60];
    let mut rgb_rows = Vec::new();
    let mut rgba_rows = Vec::new();
    let mut whole = 0usize;
    let mut decoys = 0usize;
    for y in 0..8u32 {
        let mut rgb = Vec::new();
        let mut rgba = Vec::new();
        for x in 0..8u32 {
            let pixel = [
                if x % 3 == 0 {
                    key[0]
                } else {
                    0x11 + x as u8 * 7
                },
                if y % 3 == 0 {
                    key[1]
                } else {
                    0x51 + y as u8 * 3
                },
                if (x + y) % 3 == 0 {
                    key[2]
                } else {
                    0x71 + x as u8
                },
            ];
            // Transparency is derived from the colour rather than declared
            // beside it, so the twin's alpha is 8.9.6.4's own rule applied by
            // hand and a decoy cannot accidentally disagree with it.
            let transparent = pixel == key;
            let matched = (0..3).filter(|&i| pixel[i] == key[i]).count();
            if transparent {
                whole += 1;
            } else if matched > 0 {
                decoys += 1;
            }
            rgb.extend_from_slice(&pixel);
            rgba.extend_from_slice(&pixel);
            rgba.push(if transparent { 0 } else { 255 });
        }
        rgb_rows.push(rgb);
        rgba_rows.push(rgba);
    }
    assert!(
        whole >= 4 && decoys >= 20,
        "the fixture needs whole matches and partial ones: {whole} and {decoys}"
    );

    let keyed = simple(
        8,
        8,
        8,
        2,
        &rgb_rows,
        &[chunk(b"tRNS", &[0, key[0], 0, key[1], 0, key[2]])],
    );
    let alpha = simple(8, 8, 8, 6, &rgba_rows, &[]);

    let (left, left_route) = embedded(&keyed);
    let (right, right_route) = embedded(&alpha);
    assert_eq!(
        left_route,
        PngRoute::PassedThrough,
        "the key passes through"
    );
    assert_eq!(right_route, PngRoute::Decoded, "the alpha channel does not");
    assert_painted(&left, 9, "the colour-keyed image");
    assert_same_picture(&left, &right, "a colour key against a soft mask");
}

/// A `/Mask` the reader cannot make sense of leaves the image **opaque**.
///
/// The direction matters and it is not symmetric. Ignoring a mask shows pixels
/// the file wanted hidden, which whoever is looking can see; honouring a
/// malformed one hides pixels that were in the file, and the result is
/// indistinguishable from an image that was always that shape. So an array of
/// the wrong length, an integer past `2^BitsPerComponent - 1`, an empty range,
/// and `/Mask`'s other form — a reference to a stencil-mask image, which this
/// build does not read — all leave the picture alone.
///
/// The out-of-range case is the one nothing else could reach: `add_image`
/// refuses to *write* such an array, so it has to be hand-built. Without the
/// guard, `/Mask [0 300]` covers every value an 8-bit sample can hold and the
/// page comes out blank. It is the one defect of twenty-eight that survived the
/// first injection matrix.
#[test]
fn a_mask_the_reader_cannot_read_leaves_the_image_opaque() {
    let with_mask = |mask: &str| -> Bitmap {
        let bytes = format!(
            "%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 4 4]\n\
   /Resources << /XObject << /Im0 5 0 R >> >> /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 25 >>\nstream\nq 4 0 0 4 0 0 cm /Im0 Do Q\nendstream\nendobj\n\
5 0 obj\n<< /Type /XObject /Subtype /Image /Width 4 /Height 4\n\
   /ColorSpace /DeviceGray /BitsPerComponent 8 {mask} /Length 16 >>\n\
stream\n\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\x11\nendstream\nendobj\n\
6 0 obj\n<< /Type /XObject /Subtype /Image /Width 4 /Height 4 /ImageMask true\n\
   /BitsPerComponent 1 /Length 4 >>\nstream\n\x00\x00\x00\x00\nendstream\nendobj\n\
trailer\n<< /Size 7 /Root 1 0 R >>\n%%EOF\n"
        );
        render(bytes.into_bytes())
    };

    let painted = |bitmap: &Bitmap| {
        bitmap
            .data
            .chunks_exact(bitmap.components())
            .all(|p| p[0] == 0x11)
    };

    assert!(painted(&with_mask("")), "the control paints its samples");
    assert!(
        painted(&with_mask("/Mask [0 300]")),
        "an integer past 2^8 - 1 makes the array unreadable, not all-masking"
    );
    // The wrong-length case needs its **first pair to match**, or it proves
    // nothing: with the length check gone, only as many components as the array
    // holds are read, and a leading pair that matches nothing leaves the image
    // opaque for the same reason a rejected array does. That is how the first
    // version of this line survived its own injection.
    assert!(
        painted(&with_mask("/Mask [17 17 0 0]")),
        "four integers for a one-component image is not 2 x n"
    );
    assert!(
        painted(&with_mask("/Mask [5 1]")),
        "a minimum above its maximum names no range at all"
    );
    assert!(
        painted(&with_mask("/Mask 6 0 R")),
        "and /Mask's stencil form is a feature this build does not have"
    );

    // The readable one still masks, or the four above prove nothing: a build
    // that ignored every /Mask would pass all of them.
    let hidden = with_mask("/Mask [17 17]");
    assert!(
        hidden
            .data
            .chunks_exact(hidden.components())
            .all(|p| p[0] == 255),
        "a readable key still hides the samples it names"
    );
}

/// And the key actually hides something: an image entirely of the key colour
/// leaves the page as it found it, where the same file without the `tRNS`
/// covers it.
///
/// The comparison above would pass on a build that ignored `/Mask` *and* threw
/// the alpha away, since both pictures would then be opaque. This is the test
/// that says the transparency exists at all.
#[test]
fn a_colour_key_leaves_the_page_showing_through() {
    let key = [0x11u8, 0x22, 0x33];
    let rows: Vec<Vec<u8>> = (0..4).map(|_| key.repeat(4)).collect();
    let opaque = simple(4, 4, 8, 2, &rows, &[]);
    let masked = simple(
        4,
        4,
        8,
        2,
        &rows,
        &[chunk(b"tRNS", &[0, key[0], 0, key[1], 0, key[2]])],
    );

    let dark = embedded(&opaque).0;
    assert_eq!(
        dark.data
            .chunks_exact(dark.components())
            .next()
            .map(|p| p[0]),
        Some(key[0]),
        "without the key the image paints"
    );

    let clear = embedded(&masked).0;
    assert!(
        clear
            .data
            .chunks_exact(clear.components())
            .all(|p| p[0] == 255 && p[1] == 255 && p[2] == 255),
        "with it every pixel is masked and the white page shows through"
    );
}

/// An indexed `tRNS` naming a run of indices, against the same picture with a
/// real alpha channel.
#[test]
fn an_indexed_colour_key_and_a_soft_mask_are_one_picture() {
    // Four palette entries; indices 1 and 2 are transparent, which is the
    // contiguous run 8.9.6.4 can express and gap 29's table calls the general
    // case of "one fully transparent index".
    let palette = [10u8, 200, 30, 40, 50, 60, 70, 80, 90, 250, 20, 120];
    let index_at = |x: u32, y: u32| (x + y * 3) % 4;

    let mut indexed_rows = Vec::new();
    let mut rgba_rows = Vec::new();
    for y in 0..8u32 {
        let mut samples = Vec::new();
        let mut rgba = Vec::new();
        for x in 0..8u32 {
            let i = index_at(x, y);
            samples.push(i);
            let e = (i * 3) as usize;
            rgba.extend_from_slice(&palette[e..e + 3]);
            rgba.push(if (1..=2).contains(&i) { 0 } else { 255 });
        }
        indexed_rows.push(pack(&samples, 2));
        rgba_rows.push(rgba);
    }

    let keyed = simple(
        8,
        8,
        2,
        3,
        &indexed_rows,
        &[chunk(b"PLTE", &palette), chunk(b"tRNS", &[255, 0, 0, 255])],
    );
    let alpha = simple(8, 8, 8, 6, &rgba_rows, &[]);

    let (left, left_route) = embedded(&keyed);
    let (right, _) = embedded(&alpha);
    assert_eq!(left_route, PngRoute::PassedThrough);
    assert_painted(&left, 3, "the indexed colour-keyed image");
    assert_same_picture(&left, &right, "an indexed colour key against a soft mask");
}

// --- Adam7, which has no pass-through -----------------------------------

/// An interlaced file and its non-interlaced twin are one picture.
///
/// The twin passes through and the interlaced one is decoded, split and
/// re-deflated, so this puts the *whole* fallback route against the whole
/// default one — which is what nothing else in this file does, since every
/// other comparison has the decoder on only one side of it.
#[test]
fn an_interlaced_png_and_its_twin_are_one_picture() {
    let rows = rgb_rows();
    let plain = simple(8, 8, 8, 2, &rows, &[]);

    // Table 7.1: starting row, starting column, row increment, column
    // increment. Each pass is filtered on its own, at its own width.
    const PASSES: [(u32, u32, u32, u32); 7] = [
        (0, 0, 8, 8),
        (0, 4, 8, 8),
        (4, 0, 8, 4),
        (0, 2, 4, 4),
        (2, 0, 4, 2),
        (0, 1, 2, 2),
        (1, 0, 2, 1),
    ];
    let mut raw = Vec::new();
    for (ystart, xstart, ystep, xstep) in PASSES {
        let columns: Vec<u32> = (xstart..8).step_by(xstep as usize).collect();
        let lines: Vec<u32> = (ystart..8).step_by(ystep as usize).collect();
        if columns.is_empty() || lines.is_empty() {
            continue;
        }
        let pass_rows: Vec<Vec<u8>> = lines
            .iter()
            .map(|&y| {
                columns
                    .iter()
                    .flat_map(|&x| {
                        let at = (x as usize) * 3;
                        rows[y as usize][at..at + 3].to_vec()
                    })
                    .collect()
            })
            .collect();
        raw.extend_from_slice(&filtered(&pass_rows, 3));
    }
    let interlaced = png_file(ihdr(8, 8, 8, 2, 1), &[idat_of(&raw)]);

    let (through, through_route) = embedded(&plain);
    let (woven, woven_route) = embedded(&interlaced);
    assert_eq!(through_route, PngRoute::PassedThrough);
    assert_eq!(woven_route, PngRoute::Decoded, "Adam7 has no pass-through");
    assert_painted(&woven, 9, "the interlaced image");
    assert_same_picture(&woven, &through, "Adam7 against its non-interlaced twin");
}
