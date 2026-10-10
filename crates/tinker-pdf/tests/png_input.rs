//! `Bitmap::from_png`: a PNG read back into a bitmap, through the facade.
//!
//! # What this file owns, and what it does not
//!
//! The decoder underneath is `tinker_pdf_filters::png_decode`, and it is not
//! adjudicated here: PngSuite does that in `tinker-pdf-filters`' own
//! `tests/png_suite.rs`, over files an encoder nobody here wrote produced, and
//! the `png` fuzz target holds its structural promises. What this file owns is
//! the **mapping** from what that decoder hands back onto a `Bitmap` — which
//! `PixelFormat` each decoded layout becomes, what sixteen bits become, what is
//! refused and what is only named — and the round trip through `to_png` that
//! the mapping has to make exact.
//!
//! So every expectation is named rather than decoded a second way. A 16-bit
//! sample's eight-bit value is computed here from the sample written, in
//! floating point, from `round(v × 255 / 65 535)`; a palette entry's colour is
//! the three bytes this file put in `PLTE`. The PNG files are assembled here
//! byte by byte with stored deflate blocks, so the bytes the decoder reads are
//! the bytes these helpers wrote, with no encoder of this engine's in between.

mod render_support;

use render_support::{axial_page, blend_grid_page, curvy_font, image_page};
use tinker_pdf::{
    Bitmap, Document, DocumentBuilder, PixelFormat, PixelRegion, PngError, PngReadError,
    RenderOptions, RenderWarning,
};

// ---- PNG files, assembled here ---------------------------------------------

/// 15948 5.2's eight-byte signature.
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// One chunk: length, type, data, and the CRC-32 over type and data (5.3).
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut covered = kind.to_vec();
    covered.extend_from_slice(data);
    out.extend_from_slice(&tinker_pdf_filters::crc32(&covered).to_be_bytes());
    out
}

/// A zlib stream whose deflate blocks are all stored, so no compressor of this
/// engine's stands between the samples written and the samples read.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    if data.is_empty() {
        out.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    let mut blocks = data.chunks(0xFFFF).peekable();
    while let Some(block) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        out.extend_from_slice(&(block.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        out.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    out.extend_from_slice(&((b << 16) | a).to_be_bytes());
    out
}

/// A whole file: IHDR for these parameters, the extra chunks in order, one
/// IDAT of `rows` each carrying filter type 0, and IEND.
fn png(
    width: u32,
    height: u32,
    depth: u8,
    colour_type: u8,
    extra: &[(&[u8; 4], Vec<u8>)],
    rows: &[Vec<u8>],
) -> Vec<u8> {
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[depth, colour_type, 0, 0, 0]);

    let mut filtered = Vec::new();
    for row in rows {
        filtered.push(0);
        filtered.extend_from_slice(row);
    }

    let mut out = SIGNATURE.to_vec();
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));
    for (kind, data) in extra {
        out.extend_from_slice(&chunk(kind, data));
    }
    out.extend_from_slice(&chunk(b"IDAT", &zlib_stored(&filtered)));
    out.extend_from_slice(&chunk(b"IEND", b""));
    out
}

/// A 16-bit sample in eight bits, computed the way the module documentation
/// states it rather than the way the implementation spells it.
fn nearest_eight(v: u16) -> u8 {
    (f64::from(v) * 255.0 / 65535.0).round() as u8
}

// ---- pages to round-trip ----------------------------------------------------

/// Text in an embedded face, so glyph coverage is in the pixels.
fn text_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.set_subset_fonts(false);
    assert!(builder.add_embedded_font(b"F0", b"Curvy", &curvy_font()));
    builder.add_page(90.0, 40.0, |page| {
        page.text(b"F0", 13.0, 4.0, 22.0, "Round trip");
        page.text(b"F0", 8.0, 4.5, 8.0, "0123456789 fox");
    });
    builder.finish()
}

/// The four pages whose renders go out and come back: text, an axial
/// shading, an image, and the blend grid. Four different paths into the
/// rasterizer, so a format mapping that is right only for one kind of pixel
/// does not pass on the others' account.
fn pages() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("text", text_page()),
        ("axial", axial_page()),
        ("image", image_page()),
        ("blend", blend_grid_page()),
    ]
}

/// Every `PixelFormat` a page comes back in by default.
///
/// `CmykA8` and `LabA8` are not among them: `page_format` turns both into
/// `Rgba8`, so asking for either renders a bitmap this list already covers.
const PAGE_FORMATS: [PixelFormat; 4] = [
    PixelFormat::Gray8,
    PixelFormat::GrayA8,
    PixelFormat::Rgb8,
    PixelFormat::Rgba8,
];

fn assert_same_bitmap(read: &Bitmap, rendered: &Bitmap, what: &str) {
    assert_eq!(
        (read.width, read.height, read.format, read.stride),
        (
            rendered.width,
            rendered.height,
            rendered.format,
            rendered.stride
        ),
        "{what}: the shape changed on the way through"
    );
    if read.data != rendered.data {
        let first = read
            .data
            .iter()
            .zip(&rendered.data)
            .position(|(a, b)| a != b);
        panic!(
            "{what}: {} of {} bytes differ, the first at {first:?}",
            read.data
                .iter()
                .zip(&rendered.data)
                .filter(|(a, b)| a != b)
                .count(),
            read.data.len()
        );
    }
    assert!(
        read.warnings.is_empty(),
        "{what}: a file this engine wrote was read with something tolerated: {:?}",
        read.warnings
    );
}

// ---- the round trip ---------------------------------------------------------

/// **The row's exit criterion**: render, `to_png`, `from_png`, and the bitmap
/// that comes back is the bitmap that went out — width, height, format, stride
/// and every byte — for every format a page can come back in.
///
/// Four pages at four formats, and each at two scales, because a stride is a
/// function of the width and a page at 1x and one at 1.5x disagree about
/// whether the row length is a multiple of anything convenient.
#[test]
fn render_to_png_and_back_is_byte_identical_for_every_page_format() {
    for (name, bytes) in pages() {
        let doc = Document::open(bytes).expect("the page opens");
        let page = doc.page(0).expect("a page");
        for format in PAGE_FORMATS {
            for scale in [1.0, 1.5] {
                let rendered = page.render(&RenderOptions {
                    format,
                    scale,
                    ..RenderOptions::default()
                });
                assert_eq!(rendered.format, format, "{name}: asked for {format:?}");
                let png = rendered.to_png().expect("a rendered page is a picture");
                let read = Bitmap::from_png(&png).expect("this engine's own PNG reads back");
                assert_same_bitmap(&read, &rendered, &format!("{name} {format:?} at {scale}"));
            }
        }
    }
}

/// A tile comes back as exactly the tile: a region's bitmap is narrower than
/// the page and its stride is its own, which is the shape most likely to be
/// read back at the page's stride by mistake.
#[test]
fn a_region_round_trips_as_the_region() {
    let doc = Document::open(text_page()).expect("it opens");
    let page = doc.page(0).expect("a page");
    for format in PAGE_FORMATS {
        let tile = page.render(&RenderOptions {
            format,
            region: Some(PixelRegion::new(7, 3, 37, 23)),
            ..RenderOptions::default()
        });
        assert_eq!((tile.width, tile.height), (37, 23));
        let read = Bitmap::from_png(&tile.to_png().expect("a picture")).expect("it reads");
        assert_same_bitmap(&read, &tile, &format!("a 37 x 23 tile in {format:?}"));
    }
}

/// The two formats PNG has no colour type for come back as the light `to_png`
/// wrote, and the colours are named rather than recomputed: pure cyan ink is
/// `(0, 255, 255)` because 8.6.4.4 says `R = 1 − min(1, C + K)`, and a Lab
/// white is white.
#[test]
fn ink_and_lab_come_back_as_the_light_that_was_written() {
    let cmyk = Bitmap {
        width: 3,
        height: 1,
        format: PixelFormat::CmykA8,
        stride: 15,
        data: vec![
            255, 0, 0, 0, 255, // cyan
            0, 0, 0, 255, 128, // black at half alpha
            0, 0, 0, 0, 255, // no ink
        ],
        warnings: Vec::new(),
        premultiplied: false,
    };
    let read = Bitmap::from_png(&cmyk.to_png().expect("a picture")).expect("it reads");
    assert_eq!(read.format, PixelFormat::Rgba8, "ink comes back as light");
    assert_eq!(
        read.data,
        vec![0, 255, 255, 255, 0, 0, 0, 128, 255, 255, 255, 255]
    );

    // L = 100 is 255 in `LabA8`'s encoding, and a = b = 0 is 128.
    let lab = Bitmap {
        width: 1,
        height: 1,
        format: PixelFormat::LabA8,
        stride: 4,
        data: vec![255, 128, 128, 200],
        warnings: Vec::new(),
        premultiplied: false,
    };
    let read = Bitmap::from_png(&lab.to_png().expect("a picture")).expect("it reads");
    assert_eq!(read.format, PixelFormat::Rgba8);
    assert_eq!(read.data[3], 200, "the alpha is kept");
    for channel in &read.data[..3] {
        assert!(*channel >= 254, "Lab white is white: {:?}", &read.data[..3]);
    }
}

// ---- the mapping, one decoded shape at a time --------------------------------

/// Sixteen bits round to the nearest eight — not the high byte. The samples
/// are chosen where the two answers differ (`0x0081` and `0xFF00`) as well as
/// at both ends and in the middle, so a truncating conversion fails by name.
#[test]
fn sixteen_bit_samples_round_to_the_nearest_eight() {
    let samples: [u16; 8] = [
        0x0000, 0x0080, 0x0081, 0x7FFF, 0x8000, 0x80FF, 0xFF00, 0xFFFF,
    ];
    let row: Vec<u8> = samples.iter().flat_map(|v| v.to_be_bytes()).collect();
    let file = png(8, 1, 16, 0, &[], &[row]);
    let read = Bitmap::from_png(&file).expect("a 16-bit grey file reads");
    assert_eq!(read.format, PixelFormat::Gray8);
    let expected: Vec<u8> = samples.iter().map(|v| nearest_eight(*v)).collect();
    assert_eq!(read.data, expected);
    // And the two places truncation would have answered differently.
    assert_eq!(read.data[2], 1, "0x0081 is nearer 1 than 0");
    assert_eq!(read.data[6], 254, "0xFF00 is nearer 254 than 255");

    // Every channel of an RGBA pixel, alpha included, goes the same way.
    let pixel: [u16; 4] = [0x0081, 0x1234, 0xFEFF, 0x8080];
    let row: Vec<u8> = pixel.iter().flat_map(|v| v.to_be_bytes()).collect();
    let read = Bitmap::from_png(&png(1, 1, 16, 6, &[], &[row])).expect("it reads");
    assert_eq!(read.format, PixelFormat::Rgba8);
    let expected: Vec<u8> = pixel.iter().map(|v| nearest_eight(*v)).collect();
    assert_eq!(read.data, expected);
}

/// A 16-bit file written from 8-bit samples by replication comes back to the
/// samples it was written from — all 256 of them.
#[test]
fn replicated_sixteen_bit_samples_come_back_exactly() {
    let row: Vec<u8> = (0..=255u8).flat_map(|v| [v, v]).collect();
    let read = Bitmap::from_png(&png(256, 1, 16, 0, &[], &[row])).expect("it reads");
    assert_eq!(read.data, (0..=255u8).collect::<Vec<u8>>());
}

/// Each decoded layout becomes the one format with its shape, and a palette
/// arrives applied — with `tRNS` making it RGBA and its absence leaving RGB.
#[test]
fn every_decoded_layout_lands_on_its_own_format() {
    // Grey + alpha, and truecolour, at eight bits: taken byte for byte.
    let read = Bitmap::from_png(&png(2, 1, 8, 4, &[], &[vec![10, 20, 30, 40]])).expect("reads");
    assert_eq!(
        (read.format, read.stride, read.data.clone()),
        (PixelFormat::GrayA8, 4, vec![10, 20, 30, 40])
    );
    let read =
        Bitmap::from_png(&png(1, 2, 8, 2, &[], &[vec![1, 2, 3], vec![4, 5, 6]])).expect("reads");
    assert_eq!(
        (read.format, read.stride, read.data.clone()),
        (PixelFormat::Rgb8, 3, vec![1, 2, 3, 4, 5, 6])
    );

    // An indexed image: the palette's own bytes, in the order the indices name.
    let palette = vec![200, 10, 20, 30, 40, 250];
    let indexed = |extra: &[(&[u8; 4], Vec<u8>)]| png(3, 1, 8, 3, extra, &[vec![1, 0, 1]]);
    let read = Bitmap::from_png(&indexed(&[(b"PLTE", palette.clone())])).expect("reads");
    assert_eq!(read.format, PixelFormat::Rgb8, "no tRNS, no alpha");
    assert_eq!(read.data, vec![30, 40, 250, 200, 10, 20, 30, 40, 250]);

    // With `tRNS`, entry 0 at alpha 7 and entry 1 past the list — opaque.
    let read =
        Bitmap::from_png(&indexed(&[(b"PLTE", palette), (b"tRNS", vec![7])])).expect("reads");
    assert_eq!(read.format, PixelFormat::Rgba8, "tRNS adds alpha");
    assert_eq!(
        read.data,
        vec![30, 40, 250, 255, 200, 10, 20, 7, 30, 40, 250, 255]
    );

    // A one-bit grey image widens to the two ends of the byte.
    let read = Bitmap::from_png(&png(8, 1, 1, 0, &[], &[vec![0b1010_0110]])).expect("reads");
    assert_eq!(read.format, PixelFormat::Gray8);
    assert_eq!(read.data, vec![255, 0, 255, 0, 0, 255, 255, 0]);
}

// ---- refused, and named -----------------------------------------------------

/// The decoder's reasons come through as they are, so a caller can tell a file
/// that is not a PNG from one whose header Table 11.1 does not permit.
#[test]
fn a_refusal_carries_the_decoders_own_reason() {
    assert_eq!(
        Bitmap::from_png(b"not a png at all").err(),
        Some(PngReadError::Refused(PngError::NotPng))
    );
    assert_eq!(
        Bitmap::from_png(&[]).err(),
        Some(PngReadError::Refused(PngError::NotPng))
    );
    // Colour type 2 at four bits is not in Table 11.1.
    assert_eq!(
        Bitmap::from_png(&png(1, 1, 4, 2, &[], &[vec![0]])).err(),
        Some(PngReadError::Refused(PngError::BadColourTypeDepth {
            colour_type: 2,
            bit_depth: 4
        }))
    );
    // Colour type 3 needs its palette.
    assert_eq!(
        Bitmap::from_png(&png(1, 1, 8, 3, &[], &[vec![0]])).err(),
        Some(PngReadError::Refused(PngError::MissingPalette))
    );
}

/// A raster that stops short of its declared height is refused rather than
/// handed back with a band of zeroes — the one place this door is stricter
/// than the decoder under it, and the reason is in the module documentation.
#[test]
fn a_raster_short_of_its_height_is_refused_by_name() {
    // Four rows declared, two written.
    let short = png(2, 4, 8, 0, &[], &[vec![1, 2], vec![3, 4]]);
    match Bitmap::from_png(&short) {
        Err(PngReadError::Incomplete { reasons }) => assert!(
            reasons.contains(&"truncated-input"),
            "the decoder's own identifier is carried: {reasons:?}"
        ),
        other => panic!("a short raster must be refused as incomplete, got {other:?}"),
    }
}

/// Damage that costs no pixels is named, not refused: an ancillary chunk with
/// a bad CRC is dropped and the picture is whole.
#[test]
fn damage_that_costs_no_pixels_is_named_on_the_bitmap() {
    let mut text = chunk(b"tEXt", b"Comment\0read back");
    let last = text.len() - 1;
    text[last] ^= 0xFF; // the CRC, broken
    let mut file = png(2, 1, 8, 0, &[], &[vec![9, 99]]);
    // After IHDR (8 + 25 bytes), before IDAT.
    let at = SIGNATURE.len() + 25;
    file.splice(at..at, text);

    let read = Bitmap::from_png(&file).expect("the picture is whole");
    assert_eq!(read.data, vec![9, 99]);
    assert!(
        read.warnings.contains(&RenderWarning::DamagedImage {
            name: "PNG".to_string(),
            reason: "png-chunk-crc-mismatch".to_string(),
        }),
        "the tolerated damage is on the bitmap (ruling 10): {:?}",
        read.warnings
    );
}

// ---- hostile input ------------------------------------------------------------

/// A small, fixed-seed generator, so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() % bound as u64) as usize
        }
    }
}

/// Whatever comes back is a bitmap whose fields agree with each other; the
/// answer is returned so the campaign can count how often each side was
/// reached.
fn check(bytes: &[u8]) -> bool {
    match Bitmap::from_png(bytes) {
        Ok(bitmap) => {
            assert!(
                bitmap.width > 0 && bitmap.height > 0,
                "a picture has pixels"
            );
            assert_eq!(
                bitmap.stride,
                bitmap.width as usize * bitmap.format.components()
            );
            assert_eq!(bitmap.data.len(), bitmap.stride * bitmap.height as usize);
            true
        }
        Err(_) => false,
    }
}

/// Recomputes every chunk's CRC, so a mutation inside a chunk reaches the code
/// behind the checksum instead of stopping at it.
fn recrc(file: &mut [u8]) {
    let mut at = SIGNATURE.len();
    while at + 12 <= file.len() {
        let length = u32::from_be_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]);
        let end = at + 8 + length as usize;
        if end + 4 > file.len() {
            return;
        }
        let crc = tinker_pdf_filters::crc32(&file[at + 4..end]);
        file[end..end + 4].copy_from_slice(&crc.to_be_bytes());
        at = end + 4;
    }
}

/// **Arbitrary bytes never panic**, and neither does anything near a real
/// file: random buffers, random buffers wearing the signature, and every
/// seed file below mutated by flips, truncations, insertions and splices —
/// half of those with their CRCs repaired so the mutation lands in the header,
/// the palette and the inflater rather than in the checksum.
#[test]
fn arbitrary_bytes_never_panic() {
    let mut rng = Rng(0x5EED_0FB1_7A9E);

    for _ in 0..4000 {
        let len = rng.below(200);
        let mut bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        assert!(!check(&bytes) || bytes.starts_with(&SIGNATURE));
        if rng.below(2) == 0 {
            let mut signed = SIGNATURE.to_vec();
            signed.append(&mut bytes);
            let _ = check(&signed);
        }
    }

    let rendered = Document::open(text_page())
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions {
            format: PixelFormat::Rgba8,
            scale: 0.5,
            ..RenderOptions::default()
        })
        .to_png()
        .expect("a picture");
    let seeds = vec![
        rendered,
        png(3, 2, 16, 6, &[], &[vec![7; 24], vec![200; 24]]),
        png(
            3,
            1,
            8,
            3,
            &[(b"PLTE", vec![1, 2, 3, 4, 5, 6]), (b"tRNS", vec![0])],
            &[vec![0, 1, 0]],
        ),
        png(
            9,
            3,
            2,
            0,
            &[(b"tRNS", vec![0, 1])],
            &vec![vec![0x1B, 0xE4, 0x80]; 3],
        ),
        png(2, 2, 8, 4, &[], &[vec![1, 2, 3, 4], vec![5, 6, 7, 8]]),
    ];

    // How many mutated files decoded to a picture and how many were refused.
    // Both floors matter: a campaign that is refused every time has only
    // tested the signature check, and one that is never refused has not
    // mutated anything that mattered.
    let (mut read, mut refused) = (0usize, 0usize);
    for seed in &seeds {
        assert!(check(seed), "every seed is a readable file");
        for _ in 0..1500 {
            let mut file = seed.clone();
            match rng.below(4) {
                0 => {
                    for _ in 0..=rng.below(4) {
                        let at = rng.below(file.len());
                        file[at] ^= 1 << rng.below(8);
                    }
                }
                1 => file.truncate(rng.below(file.len())),
                2 => {
                    let at = rng.below(file.len());
                    let junk: Vec<u8> = (0..=rng.below(8)).map(|_| rng.next() as u8).collect();
                    file.splice(at..at, junk);
                }
                _ => {
                    // A header field set to something extreme: the width and
                    // height live at bytes 16..24, depth and colour type after.
                    let at = 16 + rng.below(10);
                    if at < file.len() {
                        file[at] = [0, 1, 0x7F, 0x80, 0xFF][rng.below(5)];
                    }
                }
            }
            if rng.below(2) == 0 {
                recrc(&mut file);
            }
            if check(&file) {
                read += 1;
            } else {
                refused += 1;
            }
        }
    }
    assert!(
        read > 500 && refused > 500,
        "the campaign must reach both answers: {read} read, {refused} refused"
    );
}
