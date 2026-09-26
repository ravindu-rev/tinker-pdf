//! GIFs written block by block from the GIF89a specification, and the decoder
//! held to them.
//!
//! The Pillow files in `tests/images/gif/` carry the exit criterion — authored
//! pixels a real encoder compressed. These build what an encoder was not asked
//! for: a local table, a first image smaller than its screen, an animation,
//! Appendix F's KwKwK code, every root size, and one file per [`GifError`].
//!
//! [`literal`] is a deliberately naive LZW coder: it emits every index as a
//! root code, clearing nothing, and so exercises the width rule and the table
//! the decoder builds without ever using an entry. Pillow's files are the ones
//! that use entries.

use super::*;

const CAP: Limits = Limits::new(1 << 24);

/// Every index as a root code, least significant bit first, with Appendix
/// F's width rule tracked the way a decoder tracks it.
fn literal(indices: &[u8], min: u8) -> Vec<u8> {
    let clear = 1u32 << min;
    let mut width = u32::from(min) + 1;
    let mut next = clear + 2;
    let mut bits = BitWriter::default();
    bits.push(clear, width);
    for (n, &index) in indices.iter().enumerate() {
        bits.push(u32::from(index), width);
        if n > 0 && next < 4096 {
            next += 1;
        }
        if next == 1 << width && width < 12 {
            width += 1;
        }
    }
    bits.push(clear + 1, width);
    bits.finish()
}

#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    bits: u32,
}

impl BitWriter {
    fn push(&mut self, code: u32, width: u32) {
        self.acc |= u64::from(code) << self.bits;
        self.bits += width;
        while self.bits >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.bits -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.bits > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// A colour table, padded to a power of two, and its size field.
fn table(colours: &[[u8; 3]]) -> (Vec<u8>, u8) {
    let mut n = 0u8;
    while (2usize << n) < colours.len() {
        n += 1;
    }
    let mut out: Vec<u8> = colours.iter().flatten().copied().collect();
    out.resize((2usize << n) * 3, 0);
    (out, n)
}

/// Data sub-blocks of at most 255 bytes, and the terminator.
fn sub_blocks(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(255) {
        out.push(chunk.len() as u8);
        out.extend_from_slice(chunk);
    }
    out.push(0);
    out
}

fn gif(
    screen: (u16, u16),
    global: Option<&[[u8; 3]]>,
    background: u8,
    blocks: &[Vec<u8>],
) -> Vec<u8> {
    let mut out = b"GIF89a".to_vec();
    out.extend_from_slice(&screen.0.to_le_bytes());
    out.extend_from_slice(&screen.1.to_le_bytes());
    match global {
        Some(colours) => {
            let (bytes, n) = table(colours);
            out.extend_from_slice(&[0x80 | 0x70 | n, background, 0]);
            out.extend_from_slice(&bytes);
        }
        None => out.extend_from_slice(&[0x70, background, 0]),
    }
    for block in blocks {
        out.extend_from_slice(block);
    }
    out.push(0x3B);
    out
}

#[allow(clippy::too_many_arguments)] // one per descriptor field a test varies
fn image(
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    local: Option<&[[u8; 3]]>,
    interlaced: bool,
    min: u8,
    indices: &[u8],
) -> Vec<u8> {
    let mut out = vec![0x2C];
    for v in [left, top, width, height] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let mut packed = if interlaced { 0x40 } else { 0 };
    let local_bytes = local.map(|colours| {
        let (bytes, n) = table(colours);
        packed |= 0x80 | n;
        bytes
    });
    out.push(packed);
    if let Some(bytes) = local_bytes {
        out.extend_from_slice(&bytes);
    }
    out.push(min);
    out.extend_from_slice(&sub_blocks(&literal(indices, min)));
    out
}

/// §23's graphic control extension, with or without a transparent index.
fn control(transparent: Option<u8>) -> Vec<u8> {
    vec![
        0x21,
        0xF9,
        4,
        u8::from(transparent.is_some()),
        0,
        0,
        transparent.unwrap_or(0),
        0,
    ]
}

const FOUR: [[u8; 3]; 4] = [[0, 0, 0], [255, 0, 0], [0, 255, 0], [0, 0, 255]];

fn indexed(img: &GifImage) -> (&[u8], &[u8], Option<u8>) {
    match &img.pixels {
        ImagePixels::Indexed {
            palette,
            indices,
            transparent,
        } => (palette, indices, *transparent),
        other => panic!("expected indexed, got {other:?}"),
    }
}

// ---- the picture -------------------------------------------------------

#[test]
fn a_whole_screen_image_decodes_to_its_indices_over_the_global_table() {
    let indices = [0, 1, 2, 3, 3, 2, 1, 0];
    let file = gif(
        (4, 2),
        Some(&FOUR),
        0,
        &[image(0, 0, 4, 2, None, false, 2, &indices)],
    );
    let img = gif_decode(&file, &CAP).expect("decodes");
    assert_eq!((img.width, img.height), (4, 2));
    let (palette, got, transparent) = indexed(&img);
    assert_eq!(got, indices);
    assert_eq!(transparent, None);
    assert_eq!(&palette[..12], FOUR.as_flattened());
    assert_eq!(palette.len(), 256 * 3);
    assert!(img.complete);
    assert!(img.warnings.is_empty(), "{:?}", img.warnings);
}

#[test]
fn every_root_size_decodes() {
    for min in 1..=8u8 {
        let n = 1usize << min;
        // Enough indices to walk the width up through several steps.
        let indices: Vec<u8> = (0..300).map(|i| (i % n) as u8).collect();
        let colours: Vec<[u8; 3]> = (0..n).map(|i| [i as u8, 0, 0]).collect();
        let file = gif(
            (30, 10),
            Some(&colours),
            0,
            &[image(0, 0, 30, 10, None, false, min, &indices)],
        );
        let img = gif_decode(&file, &CAP).unwrap_or_else(|e| panic!("min {min}: {e}"));
        assert_eq!(indexed(&img).1, indices, "min {min}");
    }
}

#[test]
fn interlaced_rows_are_put_back_in_order() {
    // Ten rows, row y filled with y. Stored in pass order: 0 and 8, then 4,
    // then 2 and 6, then 1, 3, 5, 7 and 9.
    let stored_rows = [0u8, 8, 4, 2, 6, 1, 3, 5, 7, 9];
    let indices: Vec<u8> = stored_rows.iter().flat_map(|&r| [r, r]).collect();
    let colours: Vec<[u8; 3]> = (0..16).map(|i| [i as u8; 3]).collect();
    let file = gif(
        (2, 10),
        Some(&colours),
        0,
        &[image(0, 0, 2, 10, None, true, 4, &indices)],
    );
    let img = gif_decode(&file, &CAP).expect("decodes");
    let want: Vec<u8> = (0..10u8).flat_map(|r| [r, r]).collect();
    assert_eq!(indexed(&img).1, want);
}

#[test]
fn the_kwkwk_code_is_the_string_being_defined() {
    // Min 2: clear 4, end 5, first free 6. Codes: clear, 1, 6, 5. The 6 is
    // being defined by the very code that names it — Appendix F's case — and
    // is "1 1", so the output is three 1s.
    let mut bits = BitWriter::default();
    for code in [4u32, 1, 6, 5] {
        bits.push(code, 3);
    }
    let mut block = vec![0x2C, 0, 0, 0, 0, 3, 0, 1, 0, 0, 2];
    block.extend_from_slice(&sub_blocks(&bits.finish()));
    let file = gif((3, 1), Some(&FOUR), 0, &[block]);
    let img = gif_decode(&file, &CAP).expect("decodes");
    assert_eq!(indexed(&img).1, [1, 1, 1]);
}

#[test]
fn a_transparent_index_stays_an_index() {
    let file = gif(
        (2, 1),
        Some(&FOUR),
        0,
        &[control(Some(2)), image(0, 0, 2, 1, None, false, 2, &[2, 1])],
    );
    let img = gif_decode(&file, &CAP).expect("decodes");
    assert_eq!(indexed(&img).2, Some(2));
    assert_eq!(img.pixels.rgba_at(0), Some([0, 255, 0, 0]));
    assert_eq!(img.pixels.rgba_at(1), Some([255, 0, 0, 255]));
}

#[test]
fn a_local_table_is_the_one_the_image_uses() {
    let local = [[9, 9, 9], [8, 8, 8]];
    let file = gif(
        (2, 1),
        Some(&FOUR),
        0,
        &[image(0, 0, 2, 1, Some(&local), false, 2, &[1, 0])],
    );
    let img = gif_decode(&file, &CAP).expect("decodes");
    assert_eq!(img.pixels.rgba_at(0), Some([8, 8, 8, 255]));
    assert_eq!(img.pixels.rgba_at(1), Some([9, 9, 9, 255]));
}

#[test]
fn an_uncovered_screen_is_the_global_background() {
    // A 1 x 1 image at (1, 1) on a 3 x 2 screen, background index 3.
    let file = gif(
        (3, 2),
        Some(&FOUR),
        3,
        &[image(1, 1, 1, 1, None, false, 2, &[1])],
    );
    let img = gif_decode(&file, &CAP).expect("decodes");
    assert_eq!(indexed(&img).1, [3, 3, 3, 3, 1, 3]);
}

#[test]
fn a_local_table_on_a_partial_image_is_expanded_to_rgba() {
    let local = [[9, 9, 9], [8, 8, 8]];
    let with_global = gif(
        (2, 1),
        Some(&FOUR),
        2,
        &[image(1, 0, 1, 1, Some(&local), false, 2, &[1])],
    );
    let img = gif_decode(&with_global, &CAP).expect("decodes");
    assert_eq!(
        img.pixels,
        ImagePixels::Rgba(vec![0, 255, 0, 255, 8, 8, 8, 255])
    );

    // And with no global table the background is meaningless (§18), so the
    // uncovered pixel is transparent.
    let without = gif(
        (2, 1),
        None,
        2,
        &[image(1, 0, 1, 1, Some(&local), false, 2, &[1])],
    );
    let img = gif_decode(&without, &CAP).expect("decodes");
    assert_eq!(
        img.pixels,
        ImagePixels::Rgba(vec![0, 0, 0, 0, 8, 8, 8, 255])
    );
}

// ---- leniencies ----------------------------------------------------------

#[test]
fn a_second_image_is_not_read_and_says_so() {
    let file = gif(
        (2, 1),
        Some(&FOUR),
        0,
        &[
            image(0, 0, 2, 1, None, false, 2, &[1, 2]),
            control(None),
            image(0, 0, 2, 1, None, false, 2, &[3, 3]),
        ],
    );
    let img = gif_decode(&file, &CAP).expect("decodes");
    assert_eq!(indexed(&img).1, [1, 2]);
    assert!(img.warnings.contains(&Warning::GifFramesIgnored));
}

#[test]
fn an_image_past_its_screen_is_clipped_and_says_so() {
    let file = gif(
        (2, 1),
        Some(&FOUR),
        0,
        &[image(1, 0, 2, 1, None, false, 2, &[1, 2])],
    );
    let img = gif_decode(&file, &CAP).expect("decodes");
    assert_eq!(indexed(&img).1, [0, 1]);
    assert!(img.warnings.contains(&Warning::GifFrameOutsideScreen));
}

#[test]
fn an_index_past_the_table_is_black_and_warned() {
    // A two-entry table and a root size of 2, so index 3 exists as a code and
    // not as a colour.
    let file = gif(
        (2, 1),
        Some(&FOUR[..2]),
        0,
        &[image(0, 0, 2, 1, None, false, 2, &[3, 1])],
    );
    let img = gif_decode(&file, &CAP).expect("decodes");
    assert_eq!(img.pixels.rgba_at(0), Some([0, 0, 0, 255]));
    assert!(img.warnings.contains(&Warning::GifPaletteIndexOutOfRange));
}

#[test]
fn damaged_image_data_leaves_a_partial_picture() {
    // A code the table does not hold yet: 7 straight after the first root.
    let mut bits = BitWriter::default();
    for code in [4u32, 1, 7] {
        bits.push(code, 3);
    }
    let mut block = vec![0x2C, 0, 0, 0, 0, 3, 0, 1, 0, 0, 2];
    block.extend_from_slice(&sub_blocks(&bits.finish()));
    let img = gif_decode(&gif((3, 1), Some(&FOUR), 0, &[block]), &CAP).expect("decodes");
    assert!(!img.complete);
    assert!(img.warnings.contains(&Warning::BadLzwCode));
    assert!(img.warnings.contains(&Warning::TruncatedInput));

    // And a stream that simply stops.
    let mut file = gif(
        (4, 1),
        Some(&FOUR),
        0,
        &[image(0, 0, 4, 1, None, false, 2, &[1, 2, 3, 1])],
    );
    file.truncate(file.len() - 5);
    let img = gif_decode(&file, &CAP).expect("decodes");
    assert!(!img.complete);
}

// ---- refusals, every one reached -------------------------------------------

#[test]
fn every_refusal_is_reached_by_name() {
    assert_eq!(gif_decode(b"GIF90a", &CAP), Err(GifError::NotGif));
    assert_eq!(
        gif_decode(b"GIF89a\x01\x00", &CAP),
        Err(GifError::TruncatedHeader)
    );
    // A global table promised and not there.
    assert_eq!(
        gif_decode(b"GIF89a\x01\x00\x01\x00\x87\x00\x00\x00", &CAP),
        Err(GifError::TruncatedHeader)
    );
    // A trailer and nothing before it.
    assert_eq!(
        gif_decode(&gif((1, 1), Some(&FOUR), 0, &[]), &CAP),
        Err(GifError::NoImage)
    );
    assert_eq!(
        gif_decode(
            &gif(
                (1, 1),
                Some(&FOUR),
                0,
                &[image(0, 0, 0, 1, None, false, 2, &[])]
            ),
            &CAP
        ),
        Err(GifError::BadDimensions {
            width: 0,
            height: 1
        })
    );
    assert_eq!(
        gif_decode(
            &gif((1, 1), None, 0, &[image(0, 0, 1, 1, None, false, 2, &[0])]),
            &CAP
        ),
        Err(GifError::NoColourTable)
    );
    let mut bad = image(0, 0, 1, 1, None, false, 2, &[0]);
    bad[10] = 12;
    assert_eq!(
        gif_decode(&gif((1, 1), Some(&FOUR), 0, &[bad]), &CAP),
        Err(GifError::BadCodeSize(12))
    );
    assert_eq!(
        gif_decode(
            &gif(
                (4, 4),
                Some(&FOUR),
                0,
                &[image(0, 0, 4, 4, None, false, 2, &[0; 16])]
            ),
            &Limits::new(15)
        ),
        Err(GifError::ExceedsOutputLimit {
            bytes: 16,
            limit: 15
        })
    );
}

#[test]
fn an_image_past_the_sample_cap_is_refused_before_it_allocates() {
    // A 65 535 square logical screen with a 1 x 1 image carrying a local
    // table: the picture would have to be expanded, so the charge is four
    // components, about 1.7 x 10^10 samples.
    let local = [[1, 2, 3], [4, 5, 6]];
    let file = gif(
        (u16::MAX, u16::MAX),
        None,
        0,
        &[image(0, 0, 1, 1, Some(&local), false, 2, &[1])],
    );
    let Err(GifError::TooManySamples { samples, max }) =
        gif_decode(&file, &Limits::new(usize::MAX))
    else {
        panic!("the sample cap did not fire");
    };
    assert_eq!(max, MAX_GIF_SAMPLES);
    assert_eq!(samples, 65_535 * 65_535 * 4);
    // One component under the same screen is 4 294 836 225 — still past it.
    let whole = gif(
        (u16::MAX, u16::MAX),
        Some(&FOUR),
        0,
        &[image(0, 0, 1, 1, None, false, 2, &[1])],
    );
    assert!(matches!(
        gif_decode(&whole, &Limits::new(usize::MAX)),
        Err(GifError::TooManySamples { .. })
    ));
}
