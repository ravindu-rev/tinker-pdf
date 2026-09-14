//! What the PNG encoder is held to, before PngSuite ever sees it.
//!
//! # The two things a writer's own tests cannot do
//!
//! A test in this file can read the bytes back with [`super::super::png_decode`]
//! and find them unchanged, and that proves the pair self-consistent and
//! nothing more — gap 18 milestone 6's JPEG 2000 plane count is the standing
//! record of what that is worth. So the assertions here are of two kinds and
//! neither is a round trip through our own decoder:
//!
//! 1. **The container is asserted against ISO/IEC 15948's own text**, byte
//!    offset by byte offset: the signature, IHDR first with the fields 11.2.2
//!    names in the order it names them, IEND last and empty, and every chunk's
//!    CRC recomputed here over the type and the data.
//! 2. **The filtered stream is asserted against 9.2 by hand**, on images small
//!    enough that the right answer can be written out literally — which is
//!    what catches a filter emitted under the wrong type byte, the defect a
//!    round trip through a decoder sharing the same table cannot see.
//!
//! The third leg, and the one that reaches every colour type and bit depth, is
//! `tests/png_suite.rs`: 176 files produced by an encoder nobody here wrote,
//! decoded, re-encoded and unfiltered again by a transcription of 9.2 living
//! in that test rather than in this crate.

use super::*;
use crate::png::{png_decode, png_scan, PngColour, PNG_SIGNATURE};
use crate::{crc32, Limits};

const CAP: Limits = Limits::new(1 << 22);

/// A source over a tightly packed buffer — stride equal to the row.
fn packed<'a>(width: u32, height: u32, colour: PngColour, data: &'a [u8]) -> PngSource<'a> {
    PngSource {
        width,
        height,
        colour,
        stride: width as usize * colour.components() as usize,
        data,
    }
}

/// Walks a file the way 5.3 says to, returning `(type, data)` in file order.
///
/// Written here rather than reached for from `png.rs` on purpose: this is a
/// test of what the writer emitted, and asking the reader what it made of it
/// would check only that the two agree.
fn walk(file: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
    assert_eq!(&file[..8], &PNG_SIGNATURE, "5.2's eight-byte signature");
    let mut out = Vec::new();
    let mut at = 8usize;
    while at + 12 <= file.len() {
        let len = u32::from_be_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]) as usize;
        let kind = [file[at + 4], file[at + 5], file[at + 6], file[at + 7]];
        let data = file[at + 8..at + 8 + len].to_vec();
        let declared = u32::from_be_bytes([
            file[at + 8 + len],
            file[at + 9 + len],
            file[at + 10 + len],
            file[at + 11 + len],
        ]);
        // 5.3: over the type and the data, and not over the length.
        let mut body = Vec::from(kind);
        body.extend_from_slice(&data);
        assert_eq!(
            declared,
            crc32(&body),
            "chunk {} carries the wrong CRC",
            String::from_utf8_lossy(&kind)
        );
        out.push((kind, data));
        at += 12 + len;
    }
    assert_eq!(at, file.len(), "a chunk ran off the end of the file");
    out
}

/// The inflated contents of every IDAT, concatenated the way 10.3 says one
/// zlib stream may be.
fn filtered_stream(file: &[u8]) -> Vec<u8> {
    let mut idat = Vec::new();
    for (kind, data) in walk(file) {
        if &kind == b"IDAT" {
            idat.extend_from_slice(&data);
        }
    }
    let (data, complete) =
        crate::inflate::flate_bytes(&idat, &CAP, &mut crate::Warnings::default());
    assert!(complete, "the IDAT zlib stream did not inflate whole");
    data
}

// --- the container ------------------------------------------------------

/// 5.2, 5.6 and 11.2: the signature, IHDR first with 11.2.2's thirteen bytes,
/// IEND last and empty, and nothing else in a file this writer produces.
#[test]
fn the_container_is_the_one_clause_5_and_clause_11_describe() {
    let file = png_encode(&packed(2, 3, PngColour::Rgb, &[7u8; 18])).expect("a 2 x 3 RGB");
    assert_eq!(
        &file[..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
    );

    let chunks = walk(&file);
    let kinds: Vec<String> = chunks
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
        .collect();
    assert_eq!(kinds, vec!["IHDR", "IDAT", "IEND"]);

    let (_, ihdr) = &chunks[0];
    assert_eq!(ihdr.len(), 13, "11.2.2's IHDR is thirteen bytes");
    assert_eq!(&ihdr[0..4], &2u32.to_be_bytes(), "width");
    assert_eq!(&ihdr[4..8], &3u32.to_be_bytes(), "height");
    assert_eq!(ihdr[8], 8, "bit depth");
    assert_eq!(ihdr[9], 2, "colour type 2, truecolour");
    assert_eq!(
        ihdr[10], 0,
        "compression method 0, the zlib deflate of 10.3"
    );
    assert_eq!(ihdr[11], 0, "filter method 0, the five filters of 9.2");
    assert_eq!(ihdr[12], 0, "interlace method 0, one raster");

    assert!(chunks[2].1.is_empty(), "IEND carries no data");
}

/// The CRC covers the **type and the data**, and not the length.
///
/// A checksum over `length || type || data` is self-consistent: a writer and a
/// reader that shared the mistake would agree on every file and no other
/// program in the world would read one. So the number is recomputed here from
/// the two covered slices and compared against the wrong reading as well, to
/// show the two are not the same number.
#[test]
fn the_crc_covers_the_type_and_the_data_and_not_the_length() {
    let file = png_encode(&packed(1, 1, PngColour::Grey, &[0x5A])).expect("a 1 x 1 grey");
    for (kind, data) in walk(&file) {
        let mut right = Vec::from(kind);
        right.extend_from_slice(&data);
        let mut wrong = (data.len() as u32).to_be_bytes().to_vec();
        wrong.extend_from_slice(&right);
        if !data.is_empty() {
            assert_ne!(
                crc32(&right),
                crc32(&wrong),
                "the two readings coincide, so this test proves nothing"
            );
        }
    }
    // And the file the walk accepted is the one `png_scan` accepts, which is
    // the same check from the other side: a critical chunk whose CRC is wrong
    // is `PngError::ChunkCrc` there.
    assert!(png_scan(&file).is_ok());
}

/// The IDAT payload is exactly what [`zlib_compress`] makes of the filtered
/// stream — no second wrapper, no re-framing, nothing between.
#[test]
fn the_idat_is_zlib_compress_of_the_filtered_stream() {
    let data: Vec<u8> = (0..64u8).collect();
    let file = png_encode(&packed(8, 8, PngColour::Grey, &data)).expect("an 8 x 8 grey");
    let mut idat = Vec::new();
    for (kind, body) in walk(&file) {
        if &kind == b"IDAT" {
            idat.extend_from_slice(&body);
        }
    }
    assert_eq!(idat, zlib_compress(&filtered_stream(&file)));
}

// --- Table 11.1 ---------------------------------------------------------

/// Each of the four layouts this writer emits carries the colour type Table
/// 11.1 gives it, and the pairing with depth 8 is one the table permits.
///
/// The four numbers are written out rather than derived, because deriving them
/// from the component count is the plausible version that is wrong: grey+alpha
/// has two components and is type **4** while truecolour has three and is type
/// **2**, so any ordering by width disagrees with the table.
#[test]
fn every_layout_carries_table_11_1s_own_colour_type() {
    const EXPECT: [(PngColour, u8, usize); 4] = [
        (PngColour::Grey, 0, 1),
        (PngColour::Rgb, 2, 3),
        (PngColour::GreyAlpha, 4, 2),
        (PngColour::Rgba, 6, 4),
    ];
    for (colour, want, components) in EXPECT {
        assert_eq!(colour.components() as usize, components, "{colour:?}");
        assert!(
            crate::colour_type_depth_is_legal(want, 8),
            "colour type {want} at depth 8 is not in Table 11.1"
        );
        let data = vec![0x33u8; components * 2];
        let file = png_encode(&packed(2, 1, colour, &data)).expect("two pixels");
        assert_eq!(walk(&file)[0].1[9], want, "{colour:?}: IHDR colour type");
        // And the header the decoder reads back agrees, which is the claim a
        // consumer actually depends on.
        let header = png_scan(&file).expect("valid").header;
        assert_eq!(header.colour_type, want);
        assert_eq!(header.bit_depth, 8);
        assert!(!header.interlaced);
    }
}

// --- clause 9.2 ---------------------------------------------------------

/// A one-row image whose bytes rise by one: **Sub** predicts every byte after
/// the first exactly, so its filtered row is all ones and no other filter can
/// beat a sum of `width - 1`.
///
/// The type byte is asserted to be **1** and the row to the literal bytes 9.2
/// gives, so a Sub row emitted under any other tag fails here even though a
/// decoder unfiltering with the wrong formula would still be self-consistent
/// with an encoder that filtered with it.
#[test]
fn a_ramp_row_is_filtered_as_sub_and_tagged_as_one() {
    let row: Vec<u8> = (0..16u8).collect();
    let file = png_encode(&packed(16, 1, PngColour::Grey, &row)).expect("a ramp");
    let stream = filtered_stream(&file);
    assert_eq!(stream.len(), 17, "one tag plus sixteen bytes");
    assert_eq!(stream[0], 1, "9.2 type 1, Sub");
    assert_eq!(stream[1], 0, "the first byte has no left neighbour");
    assert!(stream[2..].iter().all(|&b| b == 1), "every step is +1");
}

/// Rows identical to the one above them: **Up** zeroes all of them, so every
/// row after the first is tagged **2** and is all zeroes.
#[test]
fn identical_rows_are_filtered_as_up_and_tagged_as_two() {
    let row = [10u8, 90, 200, 7];
    let mut data = Vec::new();
    for _ in 0..4 {
        data.extend_from_slice(&row);
    }
    let file = png_encode(&packed(4, 4, PngColour::Grey, &data)).expect("four equal rows");
    let stream = filtered_stream(&file);
    assert_eq!(stream.len(), 20);
    for y in 1..4 {
        let at = y * 5;
        assert_eq!(stream[at], 2, "row {y}: 9.2 type 2, Up");
        assert_eq!(&stream[at + 1..at + 5], &[0, 0, 0, 0], "row {y}");
    }
}

/// 9.2's five formulas, applied by hand to a three-pixel grey image, checked
/// against what the encoder actually emitted for each row.
///
/// The point is the **tag**, not the bytes: a filtered row and its type byte
/// have to agree, and an encoder that computed Average and wrote 4 produces a
/// file every decoder in the world reads as noise while a round trip through a
/// decoder making the same substitution reads it back perfectly.
#[test]
fn the_emitted_tag_names_the_formula_that_was_applied() {
    // A raster with no structure any one filter dominates, so several rows
    // choose differently and several tags get exercised at once.
    let data: [u8; 12] = [0, 40, 200, 3, 41, 190, 90, 90, 90, 255, 1, 128];
    let file = png_encode(&packed(4, 3, PngColour::Grey, &data)).expect("a 4 x 3 grey");
    let stream = filtered_stream(&file);
    assert_eq!(stream.len(), 15);

    let mut prior = [0u8; 4];
    for y in 0..3usize {
        let tag = stream[y * 5];
        let got = &stream[y * 5 + 1..y * 5 + 5];
        let row: [u8; 4] = data[y * 4..y * 4 + 4].try_into().expect("four bytes");
        // 9.2, Table 9.1 — the filtering direction, transcribed here and not
        // called from the encoder.
        let mut want = [0u8; 4];
        for i in 0..4usize {
            let x = row[i];
            let a = if i >= 1 { row[i - 1] } else { 0 };
            let b = prior[i];
            let c = if i >= 1 { prior[i - 1] } else { 0 };
            want[i] = match tag {
                0 => x,
                1 => x.wrapping_sub(a),
                2 => x.wrapping_sub(b),
                3 => x.wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => {
                    let (ai, bi, ci) = (i32::from(a), i32::from(b), i32::from(c));
                    let p = ai + bi - ci;
                    let (pa, pb, pc) = ((p - ai).abs(), (p - bi).abs(), (p - ci).abs());
                    let pred = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    };
                    x.wrapping_sub(pred)
                }
                other => panic!("row {y} was tagged {other}, which 9.2 does not define"),
            };
        }
        assert_eq!(got, want, "row {y} was tagged {tag} and is not that filter");
        prior = row;
    }
}

/// The heuristic is 12.8's: the **signed** sum of absolute differences.
///
/// Read unsigned, a filtered byte of `0xFF` scores 255 — the worst there is —
/// when it is a difference of minus one and the best prediction a filter can
/// make. So an image whose rows each step *down* by one is the case that parts
/// the two readings: Sub produces `0xFF` everywhere, which the signed rule
/// scores at `width - 1` and the unsigned rule at `255 x (width - 1)`, and only
/// the signed rule picks it.
#[test]
fn the_filter_choice_reads_a_filtered_byte_as_signed() {
    let row: Vec<u8> = (0..16u8).map(|i| 200 - i).collect();
    let file = png_encode(&packed(16, 1, PngColour::Grey, &row)).expect("a falling ramp");
    let stream = filtered_stream(&file);
    assert_eq!(stream[0], 1, "Sub, chosen only if 0xFF reads as -1");
    assert!(stream[2..].iter().all(|&b| b == 0xFF));

    // And the scoring function itself, on the two bytes that decide it.
    assert_eq!(cost_of(&[0xFF]), 1, "-1");
    assert_eq!(cost_of(&[0x80]), 128, "-128, the largest magnitude");
    assert_eq!(cost_of(&[0x7F]), 127);
    assert_eq!(cost_of(&[0x00]), 0);
}

/// A tie goes to the lower-numbered filter, so the choice does not depend on
/// the order five candidates happen to be visited in (ruling 4).
///
/// A single black pixel is the smallest image where every filter scores zero.
#[test]
fn a_tie_between_filters_resolves_to_the_lowest_numbered_one() {
    let file = png_encode(&packed(1, 1, PngColour::Grey, &[0])).expect("one black pixel");
    assert_eq!(filtered_stream(&file), vec![0, 0], "type 0, None");
}

/// Two encodes of the same raster are the same bytes, and a padded buffer
/// encodes to the same file as the packed one it pads (ruling 4).
#[test]
fn the_same_raster_encodes_to_the_same_bytes_every_time() {
    let data: Vec<u8> = (0..120u8).map(|i| i.wrapping_mul(37)).collect();
    let once = png_encode(&packed(10, 4, PngColour::Rgb, &data)).expect("a 10 x 4 RGB");
    let twice = png_encode(&packed(10, 4, PngColour::Rgb, &data)).expect("again");
    assert_eq!(once, twice);
}

// --- the stride ---------------------------------------------------------

/// **Padding is not pixels.** A buffer whose stride exceeds its row length
/// holds bytes that are not part of the image, and an encoder reading the
/// raster as one contiguous run writes them — shifting every row after the
/// first and producing a sheared picture that is still a valid PNG.
///
/// Nothing in this engine pads today, which is exactly why the fixture is
/// here: a `Canvas` that starts padding would make the defect appear with no
/// test failing.
#[test]
fn a_padded_stride_is_not_read_as_pixels() {
    let rows: [[u8; 3]; 3] = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
    let mut padded = Vec::new();
    for r in rows {
        padded.extend_from_slice(&r);
        // Values a contiguous read would mistake for pixels, and which no row
        // of the image contains.
        padded.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);
    }
    let file = png_encode(&PngSource {
        width: 3,
        height: 3,
        colour: PngColour::Grey,
        stride: 8,
        data: &padded,
    })
    .expect("a padded 3 x 3 grey");

    let packed_file =
        png_encode(&packed(3, 3, PngColour::Grey, &[1, 2, 3, 4, 5, 6, 7, 8, 9])).expect("packed");
    assert_eq!(
        file, packed_file,
        "the padding changed the file, so it was written as pixels"
    );
    let image = png_decode(&file, &CAP).expect("valid");
    assert_eq!(image.data, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
}

/// A buffer that stops at the last pixel is complete: the final row is charged
/// at its own width and not at the stride.
#[test]
fn a_buffer_ending_at_the_final_pixel_is_enough() {
    let mut data = vec![1u8, 2, 3, 0, 0];
    data.extend_from_slice(&[4, 5, 6]);
    let file = png_encode(&PngSource {
        width: 3,
        height: 2,
        colour: PngColour::Grey,
        stride: 5,
        data: &data,
    })
    .expect("the last row may stop at its last pixel");
    assert_eq!(
        png_decode(&file, &CAP).expect("valid").data,
        vec![1, 2, 3, 4, 5, 6]
    );
}

// --- the refusals -------------------------------------------------------

/// Each [`PngEncodeError`] is reached, because a variant nothing can produce is
/// a claim rather than a check.
#[test]
fn every_refusal_is_reachable() {
    for (width, height) in [(0u32, 4u32), (4, 0), (0, 0)] {
        assert_eq!(
            png_encode(&PngSource {
                width,
                height,
                colour: PngColour::Grey,
                stride: 4,
                data: &[0u8; 16],
            }),
            Err(PngEncodeError::BadDimensions { width, height })
        );
    }

    assert_eq!(
        png_encode(&PngSource {
            width: 4,
            height: 2,
            colour: PngColour::Rgb,
            // A row is twelve bytes; eleven would overlap the next.
            stride: 11,
            data: &[0u8; 64],
        }),
        Err(PngEncodeError::ShortStride {
            stride: 11,
            row_bytes: 12
        })
    );

    assert_eq!(
        png_encode(&packed(4, 4, PngColour::Rgba, &[0u8; 63])),
        Err(PngEncodeError::ShortData { have: 63, need: 64 })
    );

    // And every one says which number was wrong, so a caller can act on it.
    for e in [
        PngEncodeError::BadDimensions {
            width: 0,
            height: 1,
        },
        PngEncodeError::ShortStride {
            stride: 1,
            row_bytes: 2,
        },
        PngEncodeError::ShortData { have: 1, need: 2 },
    ] {
        assert!(!e.to_string().is_empty());
    }
}

/// 11.2.2's ceiling is honoured on the way out as well as on the way in: a
/// dimension past `2^31 - 1` is refused without a buffer being asked for.
#[test]
fn a_dimension_past_clause_11_2_2s_ceiling_is_refused() {
    assert_eq!(
        png_encode(&PngSource {
            width: MAX_DIMENSION + 1,
            height: 1,
            colour: PngColour::Grey,
            stride: 1 << 31,
            data: &[0u8; 4],
        }),
        Err(PngEncodeError::BadDimensions {
            width: MAX_DIMENSION + 1,
            height: 1
        })
    );
}

// --- what a reader makes of it ------------------------------------------

/// The one round trip in this file, kept small and labelled for what it is
/// worth: it shows the two halves of this crate agree, which is a necessary
/// condition and not evidence about the standard. The evidence is
/// `tests/png_suite.rs`.
#[test]
fn a_file_this_writer_produced_reads_back_through_this_crates_decoder() {
    for colour in [
        PngColour::Grey,
        PngColour::GreyAlpha,
        PngColour::Rgb,
        PngColour::Rgba,
    ] {
        let n = colour.components() as usize;
        let data: Vec<u8> = (0..(7 * 5 * n) as u32)
            .map(|i| (i.wrapping_mul(97) >> 1) as u8)
            .collect();
        let file = png_encode(&packed(7, 5, colour, &data)).expect("a 7 x 5");
        let image = png_decode(&file, &CAP).expect("valid");
        assert_eq!(image.width, 7);
        assert_eq!(image.height, 5);
        assert_eq!(image.colour, colour);
        assert_eq!(image.bits_per_component, 8);
        assert_eq!(image.data, data, "{colour:?}");
        assert!(image.complete);
        // Ruling 10: a file this engine wrote must take no leniency at all
        // from this engine's reader.
        assert!(
            image.warnings.is_empty(),
            "{colour:?}: {:?}",
            image.warnings
        );
    }
}

/// Alpha survives. A `GreyAlpha` or `Rgba` raster whose alpha varies comes
/// back with the same alpha, which an encoder that wrote the colour channels
/// and dropped the last one would fail while still producing a valid picture.
#[test]
fn a_varying_alpha_channel_survives_the_write() {
    let data: Vec<u8> = vec![
        10, 0, // grey 10, transparent
        20, 128, // grey 20, half
        30, 255, // grey 30, opaque
    ];
    let file = png_encode(&packed(3, 1, PngColour::GreyAlpha, &data)).expect("grey+alpha");
    assert_eq!(png_decode(&file, &CAP).expect("valid").data, data);

    let rgba: Vec<u8> = vec![1, 2, 3, 0, 4, 5, 6, 77, 7, 8, 9, 255];
    let file = png_encode(&packed(3, 1, PngColour::Rgba, &rgba)).expect("RGBA");
    assert_eq!(png_decode(&file, &CAP).expect("valid").data, rgba);
}
