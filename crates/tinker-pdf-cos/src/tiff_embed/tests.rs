//! The routing decision, and the claim that a placed strip is placed.
//!
//! The fixtures are TIFFs assembled from p.13's header and p.14's directory
//! entry, the same way `png_embed`'s tests assemble PNGs — a builder that is
//! not the one under test, in a crate that cannot see the filters crate's own.
//! The duplication is deliberate and bounded: what this file needs is the
//! *container*, and every coded strip in it is either a byte string whose
//! contents the router never looks at or the one committed G4 strip below.
//!
//! `choose` reads the directory and, apart from two two-byte checks — an LZW
//! strip's bit order and a JPEG strip's SOI — never reads a coded byte. That
//! is what makes a dummy strip a legitimate fixture for a routing test: the
//! route is a fact about the directory, and a test that had to encode a real
//! G4 page to find out which route it took would be testing the encoder.

use tinker_pdf_filters::{zlib_compress, Limits};

use super::*;
use crate::object::Object;
use crate::{CosDocument, DocumentBuilder, ObjRef};

const CAP: Limits = Limits::new(1 << 22);

// ---- a TIFF, from p.13 and p.14 ----------------------------------------

const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_FILL_ORDER: u16 = 266;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_ROWS_PER_STRIP: u16 = 278;
const TAG_PLANAR_CONFIGURATION: u16 = 284;
const TAG_PREDICTOR: u16 = 317;
const TAG_COLOR_MAP: u16 = 320;
const TAG_EXTRA_SAMPLES: u16 = 338;

/// One field: tag, type, and the values already encoded big-endian.
struct Tag {
    tag: u16,
    kind: u16,
    count: u32,
    payload: Vec<u8>,
}

fn long(tag: u16, v: u32) -> Tag {
    Tag {
        tag,
        kind: 4,
        count: 1,
        payload: v.to_be_bytes().to_vec(),
    }
}

fn short(tag: u16, v: u16) -> Tag {
    Tag {
        tag,
        kind: 3,
        count: 1,
        payload: v.to_be_bytes().to_vec(),
    }
}

fn shorts(tag: u16, v: &[u16]) -> Tag {
    Tag {
        tag,
        kind: 3,
        count: v.len() as u32,
        payload: v.iter().flat_map(|x| x.to_be_bytes()).collect(),
    }
}

/// Header, one directory, then every external payload and every strip.
///
/// Big-endian throughout — `MM\0*` — because the byte order is `tiff.rs`'s
/// business and this file's fixtures should differ from each other in the one
/// field each test is about.
fn build(mut tags: Vec<Tag>, strips: Vec<Vec<u8>>) -> Vec<u8> {
    tags.push(Tag {
        tag: 279, // StripByteCounts
        kind: 4,
        count: strips.len() as u32,
        payload: strips
            .iter()
            .flat_map(|s| (s.len() as u32).to_be_bytes())
            .collect(),
    });
    tags.push(Tag {
        tag: 273, // StripOffsets, filled in below
        kind: 4,
        count: strips.len() as u32,
        payload: vec![0; strips.len() * 4],
    });
    tags.sort_by_key(|t| t.tag);

    let n = tags.len();
    let data_start = 8 + 2 + 12 * n + 4;
    let mut at = data_start;
    let mut offsets = Vec::new();
    for tag in &tags {
        if tag.payload.len() > 4 {
            offsets.push(at);
            at += tag.payload.len() + tag.payload.len() % 2;
        } else {
            offsets.push(0);
        }
    }
    let mut strip_at = Vec::new();
    for strip in &strips {
        strip_at.push(at as u32);
        at += strip.len() + strip.len() % 2;
    }
    let index = tags.iter().position(|t| t.tag == 273).expect("just pushed");
    tags[index].payload = strip_at.iter().flat_map(|o| o.to_be_bytes()).collect();

    let mut out = Vec::new();
    out.extend_from_slice(b"MM\x00\x2a");
    out.extend_from_slice(&8u32.to_be_bytes());
    out.extend_from_slice(&(n as u16).to_be_bytes());
    for (i, tag) in tags.iter().enumerate() {
        out.extend_from_slice(&tag.tag.to_be_bytes());
        out.extend_from_slice(&tag.kind.to_be_bytes());
        out.extend_from_slice(&tag.count.to_be_bytes());
        if tag.payload.len() > 4 {
            out.extend_from_slice(&(offsets[i] as u32).to_be_bytes());
        } else {
            // p.15: "left-justified within the 4-byte field".
            let mut four = tag.payload.clone();
            four.resize(4, 0);
            out.extend_from_slice(&four);
        }
    }
    out.extend_from_slice(&0u32.to_be_bytes());
    for (i, tag) in tags.iter().enumerate() {
        if tag.payload.len() > 4 {
            assert_eq!(out.len(), offsets[i]);
            out.extend_from_slice(&tag.payload);
            if tag.payload.len() % 2 == 1 {
                out.push(0);
            }
        }
    }
    for (i, strip) in strips.iter().enumerate() {
        assert_eq!(out.len() as u32, strip_at[i]);
        out.extend_from_slice(strip);
        if strip.len() % 2 == 1 {
            out.push(0);
        }
    }
    out
}

/// The tags every fixture here shares.
fn base(
    width: u32,
    height: u32,
    depth: u16,
    samples: u16,
    photometric: u16,
    compression: u16,
) -> Vec<Tag> {
    vec![
        long(TAG_IMAGE_WIDTH, width),
        long(TAG_IMAGE_LENGTH, height),
        shorts(TAG_BITS_PER_SAMPLE, &vec![depth; samples as usize]),
        short(TAG_COMPRESSION, compression),
        short(TAG_PHOTOMETRIC, photometric),
        short(TAG_SAMPLES_PER_PIXEL, samples),
        long(TAG_ROWS_PER_STRIP, height),
    ]
}

/// A G4 strip: sixteen pixels wide, six rows, coded by
/// `tinker_pdf_filters::tiff::tests::encode_ccitt` from T.4 Table 1's
/// terminating codes and T.6 Table 4's mode codes.
///
/// Committed as bytes rather than re-encoded here because a T.6 coder in this
/// crate would be a second implementation of something the filters crate
/// already writes and tests against its own decoder. What this file asserts
/// about it is that these exact bytes are the ones written into the PDF
/// stream, which does not need them to mean anything — and
/// `tinker_pdf_filters`' own tests are where they are held to meaning one
/// particular picture.
const G4_STRIP: [u8; 24] = [
    0x33, 0x14, 0xBB, 0x0C, 0x2C, 0x20, 0xF0, 0x88, 0xB3, 0x65, 0x0E, 0x50, 0xE1, 0x11, 0xD1, 0x1D,
    0x11, 0xD0, 0x20, 0x94, 0x44, 0x44, 0x44, 0x58,
];

fn g4_tiff() -> Vec<u8> {
    build(base(16, 6, 1, 1, 0, 4), vec![G4_STRIP.to_vec()])
}

/// Finds the one image XObject in a written document and returns its raw
/// stream bytes and the dictionary text in front of them.
///
/// Written PDF source rather than a re-parse, deliberately: what is under test
/// is that the bytes on disk *are* the strip's, and reading them back through
/// this repository's own parser would let a filter applied on the way in and
/// undone on the way out cancel out invisibly.
fn image_stream(pdf: &[u8]) -> (String, Vec<u8>) {
    let at = find(pdf, b"/Subtype /Image").expect("an image XObject was written");
    let start = pdf[..at]
        .windows(2)
        .rposition(|w| w == b"<<")
        .expect("the dictionary opens");
    let stream = find(&pdf[at..], b"stream\n")
        .map(|i| at + i + 7)
        .expect("a stream");
    let end = find(&pdf[stream..], b"\nendstream")
        .map(|i| stream + i)
        .expect("the stream ends");
    (
        String::from_utf8_lossy(&pdf[start..stream]).into_owned(),
        pdf[stream..end].to_vec(),
    )
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn write_one(image: &TiffImageData) -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    assert!(
        builder.add_image(b"Im", &image.image()),
        "add_image refused the dictionary this module built"
    );
    let (w, h) = (f64::from(image.width()), f64::from(image.height()));
    builder.add_page(w, h, |page| page.image(b"Im", 0.0, 0.0, w, h));
    builder.finish()
}

// ---- the claim this module exists for ----------------------------------

/// The exit criterion: a G4 strip reaches the page as **its own bytes**.
///
/// Not "the same picture", not "the same size" — the byte string in the file
/// is the byte string the TIFF held. A build that decoded and re-encoded would
/// pass every pixel comparison in this repository and would still have thrown
/// away the reason the module exists, which is why this test compares bytes
/// and not pixels.
#[test]
fn a_g4_tiff_strip_is_placed_not_decoded() {
    let prepared = tiff_image(&g4_tiff(), &CAP).expect("decodes");
    assert_eq!(prepared.route(), TiffRoute::Placed);
    assert!(prepared.complete());

    let pdf = write_one(&prepared);
    let (dict, data) = image_stream(&pdf);

    assert_eq!(data, G4_STRIP, "the written stream is not the strip");
    assert!(dict.contains("/Filter /CCITTFaxDecode"), "{dict}");
    assert!(dict.contains("/K -1"), "G4 is /K negative: {dict}");
    assert!(dict.contains("/Columns 16"), "{dict}");
    assert!(dict.contains("/Rows 6"), "{dict}");
    // PhotometricInterpretation 0 says sample 0 is white, and a fax codes
    // black runs as 1s — so `/BlackIs1 false` makes the filter emit 0 where
    // the file meant black, which `/DeviceGray` renders as black with no
    // `/Decode` array at all.
    assert!(dict.contains("/BlackIs1 false"), "{dict}");
    assert!(
        dict.contains("/EndOfBlock false"),
        "a strip carries no EOFB: {dict}"
    );
    assert!(dict.contains("/BitsPerComponent 1"), "{dict}");
    assert!(dict.contains("/ColorSpace /DeviceGray"), "{dict}");
}

/// The other half of the photometric: `BlackIsZero` with a fax coding means
/// the coded black runs are *white*, and `/BlackIs1` is the only thing that
/// says so.
#[test]
fn the_photometric_reaches_the_page_as_black_is_one() {
    let file = build(base(16, 6, 1, 1, 1, 4), vec![G4_STRIP.to_vec()]);
    let prepared = tiff_image(&file, &CAP).expect("decodes");
    assert_eq!(prepared.route(), TiffRoute::Placed);
    let (dict, data) = image_stream(&write_one(&prepared));
    assert_eq!(data, G4_STRIP);
    assert!(dict.contains("/BlackIs1 true"), "{dict}");
}

/// `Compression` 3's `T4Options` decide two of Table 11's entries, and this is
/// the only place the directory's own bits reach a `/DecodeParms`.
#[test]
fn the_t4_options_bits_become_k_and_encoded_byte_align() {
    for (options, k, align) in [
        (0u32, "/K 0", "false"),
        (1, "/K 4", "false"),
        (5, "/K 4", "true"),
    ] {
        let mut tags = base(16, 6, 1, 1, 0, 3);
        tags.push(long(292, options));
        let file = build(tags, vec![G4_STRIP.to_vec()]);
        let prepared = tiff_image(&file, &CAP).expect("decodes");
        assert_eq!(prepared.route(), TiffRoute::Placed);
        let (dict, _) = image_stream(&write_one(&prepared));
        assert!(dict.contains(k), "T4Options {options}: {dict}");
        assert!(
            dict.contains(&format!("/EncodedByteAlign {align}")),
            "T4Options {options}: {dict}"
        );
    }
}

/// Compression 2 is §10's one-dimensional coding with every row byte-aligned,
/// which is `/K 0` and `/EncodedByteAlign true` and no `T4Options` to read.
#[test]
fn modified_huffman_is_k_zero_and_byte_aligned() {
    let file = build(base(16, 6, 1, 1, 0, 2), vec![G4_STRIP.to_vec()]);
    let prepared = tiff_image(&file, &CAP).expect("decodes");
    assert_eq!(prepared.route(), TiffRoute::Placed);
    let (dict, _) = image_stream(&write_one(&prepared));
    assert!(dict.contains("/K 0"), "{dict}");
    assert!(dict.contains("/EncodedByteAlign true"), "{dict}");
}

/// LZW and DEFLATE, with and without `Predictor` 2, over four colour spaces.
///
/// The strips are arbitrary bytes: `choose` reads the directory and looks at
/// two bytes of an LZW strip and nothing else, so what is under test here is a
/// dictionary rather than a decode.
#[test]
fn lzw_and_deflate_strips_are_placed_under_their_own_filters() {
    /// One row of the table: what the directory says, and the two names the
    /// dictionary should end up carrying.
    struct Case {
        compression: u16,
        strip: Vec<u8>,
        samples: u16,
        photometric: u16,
        predictor: u16,
        filter: &'static str,
        space: &'static str,
    }

    // An LZW strip that is *not* in the pre-1993 bit order: section 13 opens
    // with the Clear code, which packed MSB-first begins 0x80.
    let lzw: Vec<u8> = vec![0x80, 0x00, 0x11, 0x22, 0x33];
    let deflate = zlib_compress(&[7u8; 48]);
    let map: Vec<u16> = (0..3 * 256u32).map(|v| (v % 65535) as u16).collect();
    let case = |compression, strip: &[u8], samples, photometric, predictor, filter, space| Case {
        compression,
        strip: strip.to_vec(),
        samples,
        photometric,
        predictor,
        filter,
        space,
    };

    let cases = [
        case(5, &lzw, 3, 2, 1, "/LZWDecode", "/DeviceRGB"),
        case(5, &lzw, 1, 1, 1, "/LZWDecode", "/DeviceGray"),
        case(5, &lzw, 3, 2, 2, "/LZWDecode", "/DeviceRGB"),
        case(8, &deflate, 3, 2, 1, "/FlateDecode", "/DeviceRGB"),
        case(32946, &deflate, 3, 2, 2, "/FlateDecode", "/DeviceRGB"),
        case(8, &deflate, 1, 3, 1, "/FlateDecode", "/Indexed"),
    ];
    for c in cases {
        let mut tags = base(4, 4, 8, c.samples, c.photometric, c.compression);
        tags.push(short(TAG_PREDICTOR, c.predictor));
        if c.photometric == 3 {
            tags.push(shorts(TAG_COLOR_MAP, &map));
        }
        let file = build(tags, vec![c.strip.clone()]);
        let prepared = tiff_image(&file, &CAP).expect("decodes");
        assert_eq!(
            prepared.route(),
            TiffRoute::Placed,
            "compression {}, predictor {}",
            c.compression,
            c.predictor
        );
        let (dict, data) = image_stream(&write_one(&prepared));
        assert_eq!(data, c.strip, "compression {}", c.compression);
        assert!(dict.contains(c.filter), "{dict}");
        assert!(dict.contains(c.space), "{dict}");
        if c.predictor == 2 {
            assert!(dict.contains("/Predictor 2"), "{dict}");
            let colors = if c.photometric == 3 { 1 } else { c.samples };
            assert!(dict.contains(&format!("/Colors {colors}")), "{dict}");
            assert!(dict.contains("/Columns 4"), "{dict}");
        } else {
            assert!(!dict.contains("/Predictor"), "{dict}");
        }
    }
}

/// `Compression` 7's two halves become one `/DCTDecode` stream, and the route
/// says which of the two shapes the file used.
#[test]
fn a_jpeg_strip_is_spliced_or_placed_and_never_decoded() {
    // Only the SOI and EOI matter to the splice; nothing here decodes them.
    let tables: Vec<u8> = vec![
        0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x05, 0x00, 0x01, 0x02, 0xFF, 0xD9,
    ];
    let strip: Vec<u8> = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x11, 0x22, 0xFF, 0xD9];

    let mut tags = base(8, 8, 8, 1, 1, 7);
    tags.push(Tag {
        tag: 347,
        kind: 7,
        count: tables.len() as u32,
        payload: tables.clone(),
    });
    let spliced = tiff_image(&build(tags, vec![strip.clone()]), &CAP).expect("decodes");
    assert_eq!(spliced.route(), TiffRoute::Spliced);
    let (dict, data) = image_stream(&write_one(&spliced));
    assert!(dict.contains("/Filter /DCTDecode"), "{dict}");
    assert_eq!(
        data,
        [&tables[..tables.len() - 2], &strip[2..]].concat(),
        "the tables without their EOI, then the strip without its SOI"
    );

    // With no tables tag the strip is already whole, and its bytes are placed.
    let bare =
        tiff_image(&build(base(8, 8, 8, 1, 1, 7), vec![strip.clone()]), &CAP).expect("decodes");
    assert_eq!(bare.route(), TiffRoute::Placed);
    assert_eq!(image_stream(&write_one(&bare)).1, strip);
}

// ---- everything that has to decode, and the reason it does -------------

/// Every row of the module note's second table, checked to take the route the
/// table says it takes.
///
/// One test rather than eleven because what is being asserted is the *table*:
/// a row that quietly started passing through would be a correctness bug in
/// one of eleven different ways, and they are all the same claim.
#[test]
fn the_files_that_cannot_pass_through_take_the_decoder() {
    let pixels: Vec<u8> = (0..4 * 4 * 3u32).map(|v| (v * 7 % 251) as u8).collect();
    let grey: Vec<u8> = (0..16u32).map(|v| (v * 17) as u8).collect();
    let lzw_old: Vec<u8> = vec![0x00, 0x01, 0x11, 0x22, 0x33];

    // (why, the file)
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "Compression 1 has nothing to pass through",
            build(base(4, 4, 8, 3, 2, 1), vec![pixels.clone()]),
        ),
        (
            "PackBits is not RunLengthDecode",
            build(base(4, 4, 8, 1, 1, 32773), vec![vec![0x0F; 17]]),
        ),
        ("two strips are not one stream", {
            let mut tags = base(4, 4, 8, 3, 2, 1);
            tags.retain(|t| t.tag != TAG_ROWS_PER_STRIP);
            tags.push(long(TAG_ROWS_PER_STRIP, 2));
            build(tags, vec![pixels[..24].to_vec(), pixels[24..].to_vec()])
        }),
        (
            "the pre-1993 LZW bit order",
            build(base(4, 4, 8, 3, 2, 5), vec![lzw_old]),
        ),
        (
            "WhiteIsZero outside the fax codings needs /Decode [1 0]",
            build(base(4, 4, 8, 1, 0, 1), vec![grey.clone()]),
        ),
        ("PlanarConfiguration 2", {
            let mut tags = base(4, 4, 8, 3, 2, 1);
            tags.push(short(TAG_PLANAR_CONFIGURATION, 2));
            build(tags, vec![grey.clone(), grey.clone(), grey.clone()])
        }),
        ("FillOrder 2", {
            let mut tags = base(4, 4, 8, 1, 1, 1);
            tags.push(short(TAG_FILL_ORDER, 2));
            build(tags, vec![grey.clone()])
        }),
        ("ExtraSamples needs an /SMask", {
            let mut tags = base(2, 2, 8, 4, 2, 1);
            tags.push(short(TAG_EXTRA_SAMPLES, 2));
            build(tags, vec![vec![9u8; 16]])
        }),
        ("a tile is stored padded", {
            let mut tags = base(16, 16, 8, 1, 1, 1);
            tags.retain(|t| t.tag != TAG_ROWS_PER_STRIP);
            tags.push(long(322, 16));
            tags.push(long(323, 16));
            let mut file = build(tags, vec![vec![3u8; 256]]);
            // `build` writes strips; retag them as tiles, which is the
            // same two arrays under two other numbers.
            retag(&mut file, 273, 324);
            retag(&mut file, 279, 325);
            file
        }),
    ];

    for (why, file) in cases {
        let prepared = tiff_image(&file, &CAP).unwrap_or_else(|e| panic!("{why}: {e}"));
        assert_eq!(prepared.route(), TiffRoute::Decoded, "{why}");
    }
}

/// A 16-bit `II` file decodes and a 16-bit `MM` file is placed, which is the
/// one routing decision that turns on the byte order alone.
#[test]
fn sixteen_bit_samples_pass_through_only_from_a_big_endian_file() {
    let big = build(base(2, 2, 16, 1, 1, 8), vec![zlib_compress(&[0u8; 8])]);
    assert_eq!(
        tiff_image(&big, &CAP).expect("decodes").route(),
        TiffRoute::Placed
    );

    // The same file with its two order bytes and every multi-byte field
    // rewritten little-endian is a different fixture, so this reaches for the
    // simplest thing that is true: an `II` file built the same way.
    let mut little = big.clone();
    swap_to_little_endian(&mut little);
    assert_eq!(
        tiff_image(&little, &CAP).expect("decodes").route(),
        TiffRoute::Decoded,
        "8.9.5.2 wants big-endian samples"
    );
}

fn retag(file: &mut [u8], from: u16, to: u16) {
    let entries = usize::from(u16::from_be_bytes([file[8], file[9]]));
    for index in 0..entries {
        let at = 10 + index * 12;
        if u16::from_be_bytes([file[at], file[at + 1]]) == from {
            file[at..at + 2].copy_from_slice(&to.to_be_bytes());
        }
    }
}

/// Rewrites a `MM` file's header, entry count, tags, types, counts and inline
/// values as `II`. The strip data is bytes and does not move.
fn swap_to_little_endian(file: &mut [u8]) {
    file[0] = b'I';
    file[1] = b'I';
    file[2..4].copy_from_slice(&42u16.to_le_bytes());
    let first = u32::from_be_bytes([file[4], file[5], file[6], file[7]]);
    file[4..8].copy_from_slice(&first.to_le_bytes());
    let at = first as usize;
    let entries = usize::from(u16::from_be_bytes([file[at], file[at + 1]]));
    file[at..at + 2].copy_from_slice(&(entries as u16).to_le_bytes());
    for index in 0..entries {
        let e = at + 2 + index * 12;
        let tag = u16::from_be_bytes([file[e], file[e + 1]]);
        let kind = u16::from_be_bytes([file[e + 2], file[e + 3]]);
        let count = u32::from_be_bytes([file[e + 4], file[e + 5], file[e + 6], file[e + 7]]);
        file[e..e + 2].copy_from_slice(&tag.to_le_bytes());
        file[e + 2..e + 4].copy_from_slice(&kind.to_le_bytes());
        file[e + 4..e + 8].copy_from_slice(&count.to_le_bytes());
        // Every field in these fixtures is one SHORT or one LONG, so the value
        // is inline and left-justified either way.
        match kind {
            3 => {
                let v = u16::from_be_bytes([file[e + 8], file[e + 9]]);
                file[e + 8..e + 10].copy_from_slice(&v.to_le_bytes());
            }
            _ => {
                let v = u32::from_be_bytes([file[e + 8], file[e + 9], file[e + 10], file[e + 11]]);
                file[e + 8..e + 12].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
}

// ---- the decoded route -------------------------------------------------

#[test]
fn an_extra_sample_becomes_a_soft_mask() {
    let mut tags = base(2, 1, 8, 4, 2, 1);
    tags.push(short(TAG_EXTRA_SAMPLES, 2));
    let file = build(tags, vec![vec![10, 20, 30, 128, 200, 210, 220, 255]]);
    let prepared = tiff_image(&file, &CAP).expect("decodes");
    assert_eq!(prepared.route(), TiffRoute::Decoded);

    let ImageData::Compressed(image) = prepared.image() else {
        panic!("this module builds nothing else");
    };
    let mask = image
        .soft_mask
        .expect("an ExtraSamples image gets an /SMask");
    assert_eq!((mask.width, mask.height), (2, 1));
    assert_eq!(mask.bits_per_component, 8);

    let pdf = write_one(&prepared);
    assert!(
        find(&pdf, b"/SMask").is_some(),
        "the mask reached the dictionary"
    );
}

/// The `ColorMap` reaches `/Indexed` as p.23's three arrays already turned
/// into triples, which is what makes `/hival` and the lookup agree.
#[test]
fn a_palette_becomes_an_indexed_colour_space() {
    // Two entries: p.23's reds, then greens, then blues.
    let map: Vec<u16> = vec![65535, 0, 0, 65535, 0, 0];
    let mut tags = base(8, 1, 1, 1, 3, 8);
    tags.push(shorts(TAG_COLOR_MAP, &map));
    let file = build(tags, vec![zlib_compress(&[0b1010_1010])]);
    let prepared = tiff_image(&file, &CAP).expect("decodes");
    assert_eq!(prepared.route(), TiffRoute::Placed);

    let ImageData::Compressed(image) = prepared.image() else {
        panic!("this module builds nothing else");
    };
    let ImageColorSpace::Indexed { base: b, lookup } = image.color_space else {
        panic!("a palette image is /Indexed");
    };
    assert_eq!(b, DeviceSpace::Rgb);
    // One bit indexes two entries, and the map is transposed into triples.
    assert_eq!(lookup, &[255, 0, 0, 0, 255, 0]);

    let (dict, _) = image_stream(&write_one(&prepared));
    assert!(dict.contains("/Indexed"), "{dict}");
    assert!(dict.contains("/DeviceRGB"), "{dict}");
}

#[test]
fn a_file_that_is_not_a_tiff_is_refused_rather_than_routed() {
    assert!(matches!(
        tiff_image(b"not a tiff", &CAP).err(),
        Some(TiffError::NotTiff)
    ));
    // A directory that is fine and a photometric that is not.
    let file = build(base(4, 4, 8, 4, 5, 1), vec![vec![0; 64]]);
    assert!(matches!(
        tiff_image(&file, &CAP).err(),
        Some(TiffError::UnsupportedPhotometric(5))
    ));
}

/// The pass-through builds no raster, so the caller's ceiling has nothing to
/// stop — which is a property of the design rather than an accident, and the
/// one that makes a 200-page archive affordable.
#[test]
fn the_output_ceiling_binds_the_decoder_and_not_the_pass_through() {
    let one_byte = Limits::new(1);
    let placed = tiff_image(&g4_tiff(), &one_byte).expect("a placed strip needs no room");
    assert_eq!(placed.route(), TiffRoute::Placed);

    let uncompressed = build(base(16, 16, 8, 3, 2, 1), vec![vec![0; 16 * 16 * 3]]);
    assert!(matches!(
        tiff_image(&uncompressed, &one_byte),
        Err(TiffError::ExceedsOutputLimit { limit: 1, .. })
    ));
}

/// A strip the directory points outside the file is not complete, and the
/// route still places it — because there is no checksum to have failed, and
/// the module note says so.
#[test]
fn a_strip_outside_the_file_is_reported_rather_than_placed() {
    let mut file = g4_tiff();
    let entries = usize::from(u16::from_be_bytes([file[8], file[9]]));
    for index in 0..entries {
        let at = 10 + index * 12;
        if u16::from_be_bytes([file[at], file[at + 1]]) == 273 {
            file[at + 8..at + 12].copy_from_slice(&0x00FF_0000u32.to_be_bytes());
        }
    }
    let prepared = tiff_image(&file, &CAP).expect("the directory is readable");
    assert!(!prepared.complete());
    // An empty strip cannot be placed: there would be nothing in the stream.
    assert_eq!(prepared.route(), TiffRoute::Decoded);
}

/// The dictionary this module writes is one this repository's own parser
/// reads back, which is the other half of "the bytes are placed": a stream
/// nobody can find is not an image.
#[test]
fn the_dictionary_a_placed_strip_writes_survives_a_reparse() {
    let prepared = tiff_image(&g4_tiff(), &CAP).expect("decodes");
    let doc = CosDocument::open(write_one(&prepared)).expect("it opens");

    let subtype = doc.intern(b"Subtype");
    let filter = doc.intern(b"Filter");
    let mut found = false;
    for num in 1..40u32 {
        let Ok(object) = doc.get(ObjRef::new(num, 0)) else {
            continue;
        };
        let Some(dict) = object.as_dict() else {
            continue;
        };
        let is_image = dict
            .get(subtype)
            .and_then(Object::as_name)
            .and_then(|n| doc.name_bytes(n))
            .is_some_and(|b| b.as_ref() == b"Image");
        if !is_image {
            continue;
        }
        found = true;
        assert_eq!(
            dict.get(filter)
                .and_then(Object::as_name)
                .and_then(|n| doc.name_bytes(n))
                .as_deref()
                .map(<[u8]>::to_vec),
            Some(b"CCITTFaxDecode".to_vec())
        );
    }
    assert!(found, "the image object is in the parsed document");
}
