//! Bitmaps written byte by byte from the Win32 structure layouts, and the
//! decoder held to them.
//!
//! The Pillow, imagecodecs and bmpsuite files in `tests/images/` carry the
//! exit criterion — authored pixels a third-party encoder wrote. These are the
//! other half: the headers and escapes a real encoder was not asked for, built
//! here so that every [`BmpError`] and every BMP [`Warning`] is reached by a
//! test rather than listed.

use super::*;

const CAP: Limits = Limits::new(1 << 24);

/// A whole file: `BITMAPFILEHEADER`, a `BITMAPINFOHEADER` of `size` bytes
/// (the fields past forty zero unless `extra` fills them), `table`, then
/// `pixels`.
#[allow(clippy::too_many_arguments)] // one per header field a test varies
fn bmp(
    size: u32,
    width: i32,
    height: i32,
    bits: u16,
    compression: u32,
    colours_used: u32,
    extra: &[u8],
    table: &[u8],
    pixels: &[u8],
) -> Vec<u8> {
    let mut info = Vec::new();
    info.extend_from_slice(&size.to_le_bytes());
    info.extend_from_slice(&width.to_le_bytes());
    info.extend_from_slice(&height.to_le_bytes());
    info.extend_from_slice(&1u16.to_le_bytes());
    info.extend_from_slice(&bits.to_le_bytes());
    info.extend_from_slice(&compression.to_le_bytes());
    info.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
    info.extend_from_slice(&2835u32.to_le_bytes());
    info.extend_from_slice(&2835u32.to_le_bytes());
    info.extend_from_slice(&colours_used.to_le_bytes());
    info.extend_from_slice(&0u32.to_le_bytes());
    info.extend_from_slice(extra);
    let target = (size as usize).max(info.len());
    info.resize(target, 0);
    let offset = 14 + info.len() + table.len();
    let mut out = Vec::new();
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((offset + pixels.len()) as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(&info);
    out.extend_from_slice(table);
    out.extend_from_slice(pixels);
    out
}

/// A colour table of `RGBQUAD`s, blue first.
fn quads(colours: &[[u8; 3]]) -> Vec<u8> {
    colours.iter().flat_map(|&[r, g, b]| [b, g, r, 0]).collect()
}

fn indexed(img: &BmpImage) -> (&[u8], &[u8]) {
    match &img.pixels {
        ImagePixels::Indexed {
            palette, indices, ..
        } => (palette, indices),
        other => panic!("expected an indexed image, got {other:?}"),
    }
}

const FOUR: [[u8; 3]; 4] = [[0, 0, 0], [255, 0, 0], [0, 255, 0], [0, 0, 255]];

// ---- geometry ----------------------------------------------------------

#[test]
fn rows_are_stored_bottom_up_and_padded_to_four_bytes() {
    // 3 x 2 at eight bits: three index bytes and one of padding per row, and
    // the first stored row is the *bottom* one.
    let pixels = [1, 2, 3, 0xEE, 0, 1, 2, 0xEE];
    let file = bmp(40, 3, 2, 8, 0, 4, &[], &quads(&FOUR), &pixels);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert_eq!((img.width, img.height), (3, 2));
    let (palette, indices) = indexed(&img);
    assert_eq!(indices, [0, 1, 2, 1, 2, 3]);
    // Padded with black out to 2^8 entries.
    assert_eq!(palette.len(), 256 * 3);
    assert_eq!(&palette[..12], [0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
    assert!(img.complete);
    assert!(img.warnings.is_empty());
}

#[test]
fn a_negative_height_is_top_down() {
    let pixels = [1, 2, 3, 0, 0, 1, 2, 0];
    let file = bmp(40, 3, -2, 8, 0, 4, &[], &quads(&FOUR), &pixels);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert_eq!(indexed(&img).1, [1, 2, 3, 0, 1, 2]);
}

#[test]
fn one_two_and_four_bit_indices_are_read_most_significant_first() {
    // 1 bit: 10110 then padding.
    let file = bmp(
        40,
        5,
        1,
        1,
        0,
        2,
        &[],
        &quads(&FOUR[..2]),
        &[0b1011_0000, 0, 0, 0],
    );
    assert_eq!(
        indexed(&bmp_decode(&file, &CAP).unwrap()).1,
        [1, 0, 1, 1, 0]
    );
    // 2 bits: 3, 2, 1, 0, 1.
    let file = bmp(
        40,
        5,
        1,
        2,
        0,
        4,
        &[],
        &quads(&FOUR),
        &[0b1110_0100, 0b0100_0000, 0, 0],
    );
    assert_eq!(
        indexed(&bmp_decode(&file, &CAP).unwrap()).1,
        [3, 2, 1, 0, 1]
    );
    // 4 bits: 1, 2, 3.
    let file = bmp(40, 3, 1, 4, 0, 4, &[], &quads(&FOUR), &[0x12, 0x30, 0, 0]);
    assert_eq!(indexed(&bmp_decode(&file, &CAP).unwrap()).1, [1, 2, 3]);
}

#[test]
fn the_core_header_has_sixteen_bit_dimensions_and_three_byte_entries() {
    let mut info = Vec::new();
    info.extend_from_slice(&12u32.to_le_bytes());
    info.extend_from_slice(&2u16.to_le_bytes());
    info.extend_from_slice(&1u16.to_le_bytes());
    info.extend_from_slice(&1u16.to_le_bytes());
    info.extend_from_slice(&8u16.to_le_bytes());
    let mut table = Vec::new();
    for i in 0..256u32 {
        // RGBTRIPLE: blue, green, red.
        table.extend_from_slice(&[i as u8, 0, 255 - i as u8]);
    }
    let offset = 14 + info.len() + table.len();
    let mut file = b"BM".to_vec();
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&0u32.to_le_bytes());
    file.extend_from_slice(&(offset as u32).to_le_bytes());
    file.extend_from_slice(&info);
    file.extend_from_slice(&table);
    file.extend_from_slice(&[5, 250, 0, 0]);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    let (palette, indices) = indexed(&img);
    assert_eq!(indices, [5, 250]);
    assert_eq!(&palette[15..18], [250, 0, 5]);
}

// ---- direct colour -----------------------------------------------------

#[test]
fn twenty_four_bits_are_blue_green_red() {
    let pixels = [10, 20, 30, 40, 50, 60, 0, 0];
    let file = bmp(40, 2, 1, 24, 0, 0, &[], &[], &pixels);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert_eq!(img.pixels, ImagePixels::Rgb(vec![30, 20, 10, 60, 50, 40]));
}

#[test]
fn sixteen_bit_bi_rgb_is_five_five_five_scaled_to_nearest() {
    // Red 31, green 16, blue 1 in X1R5G5B5.
    let v: u16 = (31 << 10) | (16 << 5) | 1;
    let mut pixels = v.to_le_bytes().to_vec();
    pixels.extend_from_slice(&[0, 0]);
    let file = bmp(40, 1, 1, 16, 0, 0, &[], &[], &pixels);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    // 16 x 255 / 31 = 131.6 and 1 x 255 / 31 = 8.2.
    assert_eq!(img.pixels, ImagePixels::Rgb(vec![255, 132, 8]));
}

#[test]
fn the_unused_byte_of_a_32_bit_bi_rgb_pixel_is_not_alpha() {
    let file = bmp(40, 1, 1, 32, 0, 0, &[], &[], &[1, 2, 3, 0x40]);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert_eq!(img.pixels, ImagePixels::Rgb(vec![3, 2, 1]));
}

#[test]
fn bitfields_after_an_info_header_are_read_and_an_alpha_mask_makes_rgba() {
    // `BI_ALPHABITFIELDS`: four masks after a forty-byte header. Red in the
    // low byte, alpha in the high one — an order nothing could guess.
    let masks: Vec<u8> = [0x0000_00FFu32, 0x0000_FF00, 0x00FF_0000, 0xFF00_0000]
        .iter()
        .flat_map(|m| m.to_le_bytes())
        .collect();
    let mut file = bmp(40, 1, 1, 32, 6, 0, &[], &masks, &[9, 8, 7, 6]);
    // `bmp` counted the masks as a colour table, which puts them exactly
    // where `BI_ALPHABITFIELDS` says they are.
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert_eq!(img.pixels, ImagePixels::Rgba(vec![9, 8, 7, 6]));

    // And `BI_BITFIELDS` with three masks is opaque whatever the fourth byte.
    let three = &masks[..12];
    file = bmp(40, 1, 1, 32, 3, 0, &[], three, &[9, 8, 7, 6]);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert_eq!(img.pixels, ImagePixels::Rgb(vec![9, 8, 7]));
}

#[test]
fn a_v5_header_carries_its_masks_inside_itself() {
    let mut extra = Vec::new();
    for m in [0x0000_F800u32, 0x0000_07E0, 0x0000_001F, 0] {
        extra.extend_from_slice(&m.to_le_bytes());
    }
    // 5-6-5: red 31, green 63, blue 0.
    let v: u16 = (31 << 11) | (63 << 5);
    let mut pixels = v.to_le_bytes().to_vec();
    pixels.extend_from_slice(&[0, 0]);
    let file = bmp(124, 1, 1, 16, 3, 0, &extra, &[], &pixels);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert_eq!(img.pixels, ImagePixels::Rgb(vec![255, 255, 0]));
}

// ---- RLE -----------------------------------------------------------------

#[test]
fn rle8_runs_literals_and_the_escapes_decode() {
    // 4 x 2, bottom row first: a run of three 1s and one literal 2, EOL; then
    // a literal run of three (3, 2, 1) padded to a word, one more 0, EOB.
    let stream = [
        3, 1, 1, 2, 0, 0, // run of three, run of one, EOL
        0, 3, 3, 2, 1, 0, // absolute: three bytes and a pad byte
        1, 0, // run of one 0
        0, 1, // EOB
    ];
    let file = bmp(40, 4, 2, 8, 1, 4, &[], &quads(&FOUR), &stream);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert_eq!(indexed(&img).1, [3, 2, 1, 0, 1, 1, 1, 2]);
    assert!(img.complete);
    assert!(img.warnings.is_empty(), "{:?}", img.warnings);
}

#[test]
fn rle4_alternates_nibbles_high_first() {
    let stream = [
        5, 0x12, // 1 2 1 2 1
        0, 0, // EOL
        0, 3, 0x32, 0x10, // absolute: 3 2 1, two bytes, already a word
        2, 0x33, // 3 3
        0, 1,
    ];
    let file = bmp(40, 5, 2, 4, 2, 4, &[], &quads(&FOUR), &stream);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert_eq!(indexed(&img).1, [3, 2, 1, 3, 3, 1, 2, 1, 2, 1]);
}

#[test]
fn a_delta_leaves_pixels_undefined_and_says_so() {
    // Skip two right and one up, then one pixel, EOB.
    let stream = [0, 2, 2, 1, 1, 3, 0, 1];
    let file = bmp(40, 4, 2, 8, 1, 4, &[], &quads(&FOUR), &stream);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert_eq!(indexed(&img).1, [0, 0, 3, 0, 0, 0, 0, 0]);
    assert!(img.warnings.contains(&Warning::BmpRleUndefinedPixels));
    assert!(img.complete, "the file said where it ended");
}

#[test]
fn a_run_past_its_row_is_clipped_not_wrapped() {
    let stream = [6, 2, 0, 1];
    let file = bmp(40, 4, 2, 8, 1, 4, &[], &quads(&FOUR), &stream);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    // The bottom row is four 2s and the top row was never written.
    assert_eq!(indexed(&img).1, [0, 0, 0, 0, 2, 2, 2, 2]);
    assert!(img.warnings.contains(&Warning::BmpRleOverrun));
}

#[test]
fn an_rle_stream_cut_off_is_incomplete() {
    let stream = [2, 1, 0, 3, 1];
    let file = bmp(40, 4, 1, 8, 1, 4, &[], &quads(&FOUR), &stream);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert!(!img.complete);
    assert!(img.warnings.contains(&Warning::TruncatedInput));

    // And one that simply stops, with no marker and rows still to come.
    let stream = [4, 1];
    let file = bmp(40, 4, 2, 8, 1, 4, &[], &quads(&FOUR), &stream);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert!(!img.complete);
    assert!(img.warnings.contains(&Warning::EarlyEod));
}

// ---- leniencies ----------------------------------------------------------

#[test]
fn an_index_past_the_table_is_black_and_warned() {
    let file = bmp(40, 2, 1, 8, 0, 2, &[], &quads(&FOUR[1..3]), &[1, 7, 0, 0]);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    let (palette, indices) = indexed(&img);
    assert_eq!(indices, [1, 7]);
    assert_eq!(&palette[21..24], [0, 0, 0]);
    assert!(img.warnings.contains(&Warning::BmpPaletteIndexOutOfRange));
}

#[test]
fn a_short_pixel_array_is_incomplete() {
    let file = bmp(40, 2, 2, 24, 0, 0, &[], &[], &[1, 2, 3, 4, 5, 6, 0, 0, 7]);
    let img = bmp_decode(&file, &CAP).expect("decodes");
    assert!(!img.complete);
    assert!(img.warnings.contains(&Warning::TruncatedInput));
}

// ---- refusals, every one reached -------------------------------------------

#[test]
fn every_refusal_is_reached_by_name() {
    assert_eq!(bmp_decode(b"GIF89a", &CAP), Err(BmpError::NotBmp));
    assert_eq!(bmp_decode(b"BM\0\0", &CAP), Err(BmpError::NotBmp));

    let mut cut = bmp(40, 1, 1, 24, 0, 0, &[], &[], &[0; 4]);
    cut.truncate(30);
    assert_eq!(bmp_decode(&cut, &CAP), Err(BmpError::TruncatedHeader));

    let odd = bmp(40, 1, 1, 24, 0, 0, &[], &[], &[0; 4]);
    let mut odd = odd;
    odd[14..18].copy_from_slice(&200u32.to_le_bytes());
    assert_eq!(
        bmp_decode(&odd, &CAP),
        Err(BmpError::UnsupportedHeader(200))
    );

    let file = bmp(40, 0, 1, 24, 0, 0, &[], &[], &[]);
    assert!(matches!(
        bmp_decode(&file, &CAP),
        Err(BmpError::BadDimensions { width: 0, .. })
    ));
    let file = bmp(40, 1, 0, 24, 0, 0, &[], &[], &[]);
    assert!(matches!(
        bmp_decode(&file, &CAP),
        Err(BmpError::BadDimensions { height: 0, .. })
    ));

    // scRGB, and RLE8 at a depth it cannot carry.
    let file = bmp(40, 1, 1, 64, 0, 0, &[], &[], &[0; 8]);
    assert_eq!(
        bmp_decode(&file, &CAP),
        Err(BmpError::UnsupportedBitDepth(64))
    );
    let file = bmp(40, 1, 1, 4, 1, 0, &[], &quads(&FOUR), &[0; 4]);
    assert_eq!(
        bmp_decode(&file, &CAP),
        Err(BmpError::UnsupportedBitDepth(4))
    );

    // `BI_PNG`, and OS/2 2.x's RLE24 under its own number.
    let file = bmp(40, 1, 1, 24, 5, 0, &[], &[], &[0; 4]);
    assert_eq!(
        bmp_decode(&file, &CAP),
        Err(BmpError::UnsupportedCompression(5))
    );
    let file = bmp(64, 1, 1, 24, 4, 0, &[], &[], &[0; 4]);
    assert_eq!(
        bmp_decode(&file, &CAP),
        Err(BmpError::UnsupportedCompression(4))
    );

    // A split mask.
    let masks: Vec<u8> = [0x0000_0F0Fu32, 0x0000_F000, 0x00FF_0000]
        .iter()
        .flat_map(|m| m.to_le_bytes())
        .collect();
    let file = bmp(40, 1, 1, 32, 3, 0, &[], &masks, &[0; 4]);
    assert_eq!(bmp_decode(&file, &CAP), Err(BmpError::BadBitfields));

    // An indexed image whose table is squeezed out by `bfOffBits`.
    let mut file = bmp(40, 1, 1, 8, 0, 0, &[], &[], &[0; 4]);
    assert_eq!(bmp_decode(&file, &CAP), Err(BmpError::NoPalette));
    file[10..14].copy_from_slice(&9999u32.to_le_bytes());
    assert_eq!(bmp_decode(&file, &CAP), Err(BmpError::BadDataOffset(9999)));

    let file = bmp(40, 4, 4, 24, 0, 0, &[], &[], &[0; 64]);
    assert_eq!(
        bmp_decode(&file, &Limits::new(10)),
        Err(BmpError::ExceedsOutputLimit {
            bytes: 48,
            limit: 10
        })
    );
}

#[test]
fn an_image_past_the_sample_cap_is_refused_before_it_allocates() {
    // 2^31 - 1 square at three components: about 1.4 x 10^19 samples, from a
    // fifty-four byte file.
    let file = bmp(40, i32::MAX, i32::MAX, 24, 0, 0, &[], &[], &[]);
    let Err(BmpError::TooManySamples { samples, max }) = bmp_decode(&file, &CAP) else {
        panic!("the sample cap did not fire");
    };
    assert_eq!(max, MAX_BMP_SAMPLES);
    assert!(samples > MAX_BMP_SAMPLES);

    // One index past it, so the cap is a bound rather than an order of
    // magnitude. 8192 x 8192 x 1 is exactly 2^26 indices, and at that size it
    // is the *caller's* ceiling that answers — the cap let it through — while
    // one more row is the cap's own refusal. Nothing is allocated either way.
    let at = bmp(40, 8192, 8192, 8, 0, 1, &[], &quads(&FOUR[..1]), &[]);
    assert!(matches!(
        bmp_decode(&at, &CAP),
        Err(BmpError::ExceedsOutputLimit {
            bytes: 67_108_864,
            ..
        })
    ));
    let over = bmp(40, 8192, 8193, 8, 0, 1, &[], &quads(&FOUR[..1]), &[]);
    assert!(matches!(
        bmp_decode(&over, &Limits::new(usize::MAX)),
        Err(BmpError::TooManySamples { .. })
    ));
    // And a top-down height of `i32::MIN` does not overflow on the way.
    let file = bmp(40, 1, i32::MIN, 24, 0, 0, &[], &[], &[]);
    assert!(matches!(
        bmp_decode(&file, &CAP),
        Err(BmpError::TooManySamples { .. })
    ));
}
