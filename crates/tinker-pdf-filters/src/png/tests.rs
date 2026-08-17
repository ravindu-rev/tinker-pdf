//! What the PNG decoder is held to.
//!
//! # Where the fixtures come from, and why it matters here more than usual
//!
//! Gap 18 milestone 6 found a JPEG 2000 plane count that was correct only for
//! the code-blocks its own in-tree encoder happened to produce, so a round trip
//! through a writer that shares the reader's assumptions proves the pair
//! consistent and nothing else. Three defences are used here:
//!
//! 1. **Expected pixels are written out literally**, computed from ISO/IEC
//!    15948's own text rather than read back from whatever the builder emitted.
//!    Every assertion below names the samples it wants.
//! 2. **The two interlace fixtures were produced outside this repository**, by
//!    a Python script using `zlib` and `binascii` — see [`ADAM7_8X8`] — and
//!    were then decoded by Pillow, an entirely separate PNG implementation,
//!    which agreed they are the sixty-four values 0..63 in row-major order.
//!    Nothing in this tree touched them before they were committed.
//! 3. **PngSuite** runs against the same decoder in `tests/png_suite.rs`, under
//!    ruling 9: fetched, never committed.
//!
//! The builder below uses `deflate::zlib_compress` for the *transport* only. It
//! is not under test here — `inflate.rs` and `deflate.rs` have their own — and
//! nothing about the chunk walk, Table 11.1, the palette, `tRNS`, sample-depth
//! scaling or Adam7 is decided by it.

use super::*;
use crate::deflate::zlib_compress;

const CAP: Limits = Limits::new(1 << 22);

// --- the builder --------------------------------------------------------

/// 5.3's chunk layout: length, type, data, CRC over type and data.
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

fn ihdr_data(width: u32, height: u32, depth: u8, colour: u8, interlace: u8) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&width.to_be_bytes());
    d.extend_from_slice(&height.to_be_bytes());
    d.extend_from_slice(&[depth, colour, 0, 0, interlace]);
    d
}

/// A whole file: signature, IHDR, the given middle chunks, IEND.
fn png(ihdr: &[u8], middle: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::from(PNG_SIGNATURE);
    out.extend_from_slice(&chunk(b"IHDR", ihdr));
    for c in middle {
        out.extend_from_slice(c);
    }
    out.extend_from_slice(&chunk(b"IEND", b""));
    out
}

/// Rows of already-packed scanline bytes, each prefixed with filter tag 0 and
/// the lot zlib-compressed — 9.2's "None" filter, which is the one that adds
/// nothing to what is being tested.
fn idat(rows: &[Vec<u8>]) -> Vec<u8> {
    let mut raw = Vec::new();
    for r in rows {
        raw.push(0u8);
        raw.extend_from_slice(r);
    }
    chunk(b"IDAT", &zlib_compress(&raw))
}

/// A complete non-interlaced image from unfiltered scanlines.
fn simple(width: u32, height: u32, depth: u8, colour: u8, rows: &[Vec<u8>]) -> Vec<u8> {
    png(&ihdr_data(width, height, depth, colour, 0), &[idat(rows)])
}

fn decode(bytes: &[u8]) -> PngImage {
    png_decode(bytes, &CAP).expect("fixture should decode")
}

// --- 5.2, the signature -------------------------------------------------

/// PngSuite ships four files that corrupt one signature byte each — `xs1n0g01`
/// through `xs7n0g01` — because every one of those bytes is load-bearing. The
/// same four corruptions, plus every other single-byte one, are refused here.
#[test]
fn every_single_byte_change_to_the_signature_is_refused() {
    assert_eq!(
        PNG_SIGNATURE,
        [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
    );
    let good = simple(1, 1, 8, 0, &[vec![7]]);
    assert!(png_decode(&good, &CAP).is_ok());
    for i in 0..8 {
        let mut bad = good.clone();
        bad[i] ^= 0x80;
        assert_eq!(png_decode(&bad, &CAP), Err(PngError::NotPng), "byte {i}");
        // And the two mangling cases the signature exists to catch.
        let mut lower = good.clone();
        lower[i] |= 0x20;
        if lower != good {
            assert_eq!(png_decode(&lower, &CAP), Err(PngError::NotPng), "case {i}");
        }
    }
    assert_eq!(png_decode(b"", &CAP), Err(PngError::NotPng));
    assert_eq!(png_decode(b"\x89PNG", &CAP), Err(PngError::NotPng));
}

// --- Table 11.1 ---------------------------------------------------------

/// The table, written a second time and the other way round: as the set of
/// *illegal* pairs over the whole cross product.
///
/// A predicate agreeing with itself proves nothing, which is
/// `crc32.rs`'s reason for keeping a bitwise reference beside a table-driven
/// one. Here the reference is the specification's fifteen pairs enumerated by
/// hand, and every one of the 8 x 14 = 112 other combinations is asserted
/// illegal — including the ones that are *nearly* right, like colour type 3
/// with depth 16 and colour type 4 with depth 4.
#[test]
fn table_11_1_admits_fifteen_pairs_and_refuses_every_other() {
    const LEGAL: [(u8, u8); 15] = [
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
        (4, 8),
        (4, 16),
        (6, 8),
        (6, 16),
    ];
    for colour in 0u8..8 {
        for depth in [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 15, 16, 17, 32] {
            let want = LEGAL.contains(&(colour, depth));
            assert_eq!(
                colour_type_depth_is_legal(colour, depth),
                want,
                "colour type {colour}, bit depth {depth}"
            );
        }
    }
    // The near-misses, named individually so a regression says which.
    assert!(!colour_type_depth_is_legal(3, 16));
    assert!(!colour_type_depth_is_legal(4, 4));
    assert!(!colour_type_depth_is_legal(6, 4));
    assert!(!colour_type_depth_is_legal(2, 4));
    assert!(!colour_type_depth_is_legal(1, 8));
    assert!(!colour_type_depth_is_legal(5, 8));
    assert!(!colour_type_depth_is_legal(7, 8));
}

/// And the refusal reaches the decoder, by name, carrying both numbers.
///
/// PngSuite's `xc1n0g08`, `xc9n2c08`, `xd0n2c08`, `xd3n2c08` and `xd9n2c08` are
/// five published files that exist to be refused here, and the five cases
/// below are those five.
#[test]
fn an_illegal_pair_is_refused_by_name_and_not_decoded_to_something() {
    for (colour, depth) in [(1u8, 8u8), (9, 8), (2, 0), (2, 3), (2, 99), (3, 16), (4, 4)] {
        let bytes = png(&ihdr_data(2, 2, depth, colour, 0), &[]);
        assert_eq!(
            png_decode(&bytes, &CAP),
            Err(PngError::BadColourTypeDepth {
                colour_type: colour,
                bit_depth: depth
            }),
            "colour {colour} depth {depth}"
        );
        // The scan refuses it too: nothing may take the header on trust
        // because it did not ask for pixels.
        assert!(png_scan(&bytes).is_err());
    }
}

/// Every one of Table 11.1's fifteen pairs, decoded to samples written out
/// from the specification rather than read back from the builder.
///
/// Two pixels each, chosen so the two are never equal: a decoder that returned
/// the first sample twice, or that dropped the low bits of a sub-byte pair,
/// would pass an all-zero fixture.
#[test]
fn every_legal_pair_decodes_to_the_samples_the_table_describes() {
    // Colour type 0. One channel; 13.12 scales 1, 2 and 4 to 8.
    let g1 = decode(&simple(2, 1, 1, 0, &[vec![0b1000_0000]]));
    assert_eq!((g1.colour, g1.bits_per_component), (PngColour::Grey, 8));
    assert_eq!(g1.data, vec![255, 0]);

    // Two 2-bit samples in the high nibble: 01 then 11, so 1 and 3.
    let g2 = decode(&simple(2, 1, 2, 0, &[vec![0b0111_0000]]));
    assert_eq!(g2.data, vec![85, 255]);

    let g4 = decode(&simple(2, 1, 4, 0, &[vec![0x1F]]));
    assert_eq!(g4.data, vec![17, 255]);

    let g8 = decode(&simple(2, 1, 8, 0, &[vec![9, 200]]));
    assert_eq!(g8.data, vec![9, 200]);

    let g16 = decode(&simple(2, 1, 16, 0, &[vec![0x12, 0x34, 0xFF, 0x01]]));
    assert_eq!((g16.colour, g16.bits_per_component), (PngColour::Grey, 16));
    assert_eq!(g16.data, vec![0x12, 0x34, 0xFF, 0x01]);

    // Colour type 2. Three channels.
    let c8 = decode(&simple(2, 1, 8, 2, &[vec![1, 2, 3, 250, 251, 252]]));
    assert_eq!((c8.colour, c8.bits_per_component), (PngColour::Rgb, 8));
    assert_eq!(c8.data, vec![1, 2, 3, 250, 251, 252]);

    let c16 = decode(&simple(
        1,
        1,
        16,
        2,
        &[vec![0x00, 0x01, 0x80, 0x00, 0xFF, 0xFE]],
    ));
    assert_eq!((c16.colour, c16.bits_per_component), (PngColour::Rgb, 16));
    assert_eq!(c16.data, vec![0x00, 0x01, 0x80, 0x00, 0xFF, 0xFE]);

    // Colour type 3. Indices are *not* scaled — they are looked up.
    let palette = chunk(
        b"PLTE",
        &[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
    );
    for (depth, row, want) in [
        (1u8, vec![0b0100_0000], vec![10, 20, 30, 40, 50, 60]),
        (2, vec![0b0011_0000], vec![10, 20, 30, 100, 110, 120]),
        (4, vec![0x02], vec![10, 20, 30, 70, 80, 90]),
        (8, vec![0x00, 0x03], vec![10, 20, 30, 100, 110, 120]),
    ] {
        let entries = 1usize << depth;
        let plte = if entries < 4 {
            chunk(b"PLTE", &[10, 20, 30, 40, 50, 60][..entries * 3])
        } else {
            palette.clone()
        };
        let bytes = png(&ihdr_data(2, 1, depth, 3, 0), &[plte, idat(&[row])]);
        let p = decode(&bytes);
        assert_eq!((p.colour, p.bits_per_component), (PngColour::Rgb, 8));
        assert_eq!(p.data, want, "indexed depth {depth}");
    }

    // Colour type 4. Two channels, and the alpha is a sample like any other.
    let ga8 = decode(&simple(2, 1, 8, 4, &[vec![7, 8, 9, 10]]));
    assert_eq!(
        (ga8.colour, ga8.bits_per_component),
        (PngColour::GreyAlpha, 8)
    );
    assert_eq!(ga8.data, vec![7, 8, 9, 10]);

    let ga16 = decode(&simple(1, 1, 16, 4, &[vec![0xAB, 0xCD, 0x00, 0x7F]]));
    assert_eq!(
        (ga16.colour, ga16.bits_per_component),
        (PngColour::GreyAlpha, 16)
    );
    assert_eq!(ga16.data, vec![0xAB, 0xCD, 0x00, 0x7F]);

    // Colour type 6.
    let rgba8 = decode(&simple(1, 2, 8, 6, &[vec![1, 2, 3, 4], vec![5, 6, 7, 8]]));
    assert_eq!(
        (rgba8.colour, rgba8.bits_per_component),
        (PngColour::Rgba, 8)
    );
    assert_eq!(rgba8.data, vec![1, 2, 3, 4, 5, 6, 7, 8]);

    let rgba16 = decode(&simple(
        1,
        1,
        16,
        6,
        &[vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]],
    ));
    assert_eq!(
        (rgba16.colour, rgba16.bits_per_component),
        (PngColour::Rgba, 16)
    );
    assert_eq!(
        rgba16.data,
        vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
    );
}

/// 13.12's scaling is a multiplication and not a shift, and the difference is
/// visible at the top of the range: 1-bit white is 255, not 128; 2-bit
/// three-quarters is 255, not 192; 4-bit fifteen is 255, not 240.
///
/// A decoder that shifted instead would produce an image that never quite
/// reaches white, which is exactly the kind of wrongness a comparative test
/// against another of this repository's own outputs cannot see.
#[test]
fn sub_byte_depths_are_scaled_to_the_full_range_not_shifted() {
    let one = decode(&simple(2, 1, 1, 0, &[vec![0b1000_0000]]));
    assert_eq!(one.data, vec![255, 0]);
    let two = decode(&simple(4, 1, 2, 0, &[vec![0b00_01_10_11]]));
    assert_eq!(two.data, vec![0, 85, 170, 255]);
    let four = decode(&simple(4, 1, 4, 0, &[vec![0x0F, 0x18]]));
    assert_eq!(four.data, vec![0, 255, 17, 136]);
    // And the multipliers themselves, which are 255 / (2^d - 1).
    assert_eq!(scale8(1, 1), 255);
    assert_eq!(scale8(3, 2), 255);
    assert_eq!(scale8(15, 4), 255);
    assert_eq!(scale8(1, 2), 85);
    assert_eq!(scale8(1, 4), 17);
    assert_eq!(scale8(200, 8), 200);
}

/// 16-bit samples are big-endian, and the fixture is asymmetric so a swap is
/// visible: `0x0102` reversed is `0x0201`, which is a different number.
#[test]
fn sixteen_bit_samples_keep_the_networks_byte_order() {
    let img = decode(&simple(2, 1, 16, 0, &[vec![0x01, 0x02, 0xFE, 0xFF]]));
    assert_eq!(img.data, vec![0x01, 0x02, 0xFE, 0xFF]);
    assert_eq!(sample(&[0x01, 0x02], 0, 16), 0x0102);
    assert_eq!(sample(&[0xFF, 0x00], 0, 16), 0xFF00);
}

/// Sub-byte samples are packed most significant first (7.2), so the first
/// pixel of a row lives in the *high* bits.
#[test]
fn sub_byte_samples_are_packed_most_significant_first() {
    assert_eq!(sample(&[0b1000_0000], 0, 1), 1);
    assert_eq!(sample(&[0b1000_0000], 7, 1), 0);
    assert_eq!(sample(&[0b0000_0001], 7, 1), 1);
    assert_eq!(sample(&[0b1100_0000], 0, 2), 3);
    assert_eq!(sample(&[0b0000_0011], 3, 2), 3);
    assert_eq!(sample(&[0xA5], 0, 4), 0xA);
    assert_eq!(sample(&[0xA5], 1, 4), 0x5);
    // Past the end reads zero rather than panicking (ruling 1).
    assert_eq!(sample(&[], 0, 8), 0);
    assert_eq!(sample(&[0x01], 4, 16), 0);
}

// --- 5.3, the chunk walk and its CRCs -----------------------------------

/// 5.4's rule, both ways: the case of a type's first letter decides whether a
/// failed CRC refuses the image or drops the chunk.
///
/// PngSuite publishes both halves — `xhdn0g08` corrupts IHDR's CRC and
/// `xcsn0g01` corrupts IDAT's, and both are in its "corrupted files" set.
#[test]
fn a_crc_failure_refuses_a_critical_chunk_and_drops_an_ancillary_one() {
    // Critical: IHDR, PLTE, IDAT and IEND each refuse, and each names itself.
    for kind in [b"IHDR", b"PLTE", b"IDAT", b"IEND"] {
        let good = png(
            &ihdr_data(2, 1, 8, 3, 0),
            &[chunk(b"PLTE", &[1, 2, 3, 4, 5, 6]), idat(&[vec![0, 1]])],
        );
        let bad = corrupt_crc(&good, kind);
        assert_eq!(
            png_decode(&bad, &CAP),
            Err(PngError::ChunkCrc(ChunkType(*kind))),
            "{}",
            ChunkType(*kind)
        );
    }

    // Ancillary: the chunk is dropped, the image still decodes, and ruling 10
    // leaves a record saying so.
    let with_gama = png(
        &ihdr_data(2, 1, 8, 0, 0),
        &[chunk(b"gAMA", &45455u32.to_be_bytes()), idat(&[vec![4, 9]])],
    );
    let bad = corrupt_crc(&with_gama, b"gAMA");
    let img = png_decode(&bad, &CAP).expect("an ancillary chunk cannot refuse an image");
    assert_eq!(img.data, vec![4, 9]);
    assert!(img.warnings.contains(&Warning::PngChunkCrcMismatch));
    // And an intact one leaves no warning at all: a decoder that warned on
    // every ancillary chunk would make ruling 10's distinction worthless.
    let clean = png_decode(&with_gama, &CAP).expect("intact");
    assert!(clean.warnings.is_empty(), "{:?}", clean.warnings);
}

/// Flips one bit of the named chunk's CRC field, wherever it sits.
fn corrupt_crc(file: &[u8], kind: &[u8; 4]) -> Vec<u8> {
    let mut out = file.to_vec();
    let mut at = 8usize;
    while at + 8 <= out.len() {
        let len = u32::from_be_bytes([out[at], out[at + 1], out[at + 2], out[at + 3]]) as usize;
        let here = &out[at + 4..at + 8];
        let crc_at = at + 8 + len;
        if here == kind {
            out[crc_at] ^= 0x01;
            return out;
        }
        at = crc_at + 4;
    }
    panic!("no {} chunk to corrupt", ChunkType(*kind));
}

/// 5.3 puts the CRC over the type **and** the data, and not over the length.
///
/// Written as a direct check of the definition rather than only through a
/// decode, because a CRC computed over the wrong span still rejects random
/// corruption and would look like a working check from outside.
#[test]
fn the_chunk_crc_covers_the_type_and_the_data_and_not_the_length() {
    let built = chunk(b"IEND", b"");
    assert_eq!(
        built,
        vec![0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]
    );
    let mut c = Crc32::new();
    c.update(b"tEXt");
    c.update(b"hello");
    let built = chunk(b"tEXt", b"hello");
    assert_eq!(&built[built.len() - 4..], &c.finish().to_be_bytes());
    assert_ne!(c.finish(), crc32_over_everything(b"tEXt", b"hello"));
}

/// The wrong span, for the assertion above to have something to differ from.
fn crc32_over_everything(kind: &[u8; 4], data: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(&(data.len() as u32).to_be_bytes());
    c.update(kind);
    c.update(data);
    c.finish()
}

/// 10.3: the compressed datastream may be cut at *any* byte, so the IDAT
/// chunks are one zlib stream and not several.
///
/// PngSuite publishes the same claim as four files — `oi1n0g16` through
/// `oi9n0g16`, the same image in one chunk, two, four unequal ones and a run of
/// length-one ones. The second half of this test is what makes the first half
/// mean something: the first chunk **on its own** does not decode, so a
/// decoder that inflated chunk by chunk would fail rather than quietly agree.
#[test]
fn idat_is_concatenated_across_chunks_before_anything_is_inflated() {
    let rows: Vec<Vec<u8>> = (0..8u8)
        .map(|y| (0..8u8).map(|x| y * 8 + x).collect())
        .collect();
    let mut raw = Vec::new();
    for r in &rows {
        raw.push(0u8);
        raw.extend_from_slice(r);
    }
    let stream = zlib_compress(&raw);
    // One byte, then one byte, then the rest — the `oi9`/`oi4` shapes at once.
    let split: Vec<Vec<u8>> = vec![
        chunk(b"IDAT", &stream[..1]),
        chunk(b"IDAT", &stream[1..2]),
        chunk(b"IDAT", &stream[2..]),
    ];
    let many = png(&ihdr_data(8, 8, 8, 0, 0), &split);
    let one = simple(8, 8, 8, 0, &rows);

    let want: Vec<u8> = (0..64u8).collect();
    assert_eq!(decode(&one).data, want);
    assert_eq!(
        decode(&many).data,
        want,
        "chunk boundaries changed the image"
    );

    // The scan is where the concatenation happens, and it is byte-exact.
    assert_eq!(png_scan(&many).expect("scan").idat, stream);
    // And the defect this test exists to catch is visible: the leading chunk
    // is not a stream.
    let head_only = png(&ihdr_data(8, 8, 8, 0, 0), &[chunk(b"IDAT", &stream[..1])]);
    assert_ne!(decode(&head_only).data, want);
}

/// An unknown chunk with an uppercase first letter is a critical chunk this
/// build cannot interpret, and 5.4's rule is that the image must not be shown
/// as though it had been. An unknown *ancillary* one is ignored in silence,
/// because ignoring it is correct rather than lenient.
#[test]
fn an_unknown_critical_chunk_refuses_and_an_unknown_ancillary_one_does_not() {
    let critical = png(
        &ihdr_data(1, 1, 8, 0, 0),
        &[chunk(b"zZZZ", b"!"), idat(&[vec![3]])],
    );
    // `zZZZ` has a lowercase first letter, so it is ancillary and ignored.
    let img = png_decode(&critical, &CAP).expect("ancillary");
    assert_eq!(img.data, vec![3]);
    assert!(img.warnings.is_empty());

    let real = png(
        &ihdr_data(1, 1, 8, 0, 0),
        &[chunk(b"ZzZZ", b"!"), idat(&[vec![3]])],
    );
    assert_eq!(
        png_decode(&real, &CAP),
        Err(PngError::UnknownCriticalChunk(ChunkType(*b"ZzZZ")))
    );

    assert!(ChunkType(*b"IHDR").is_critical());
    assert!(ChunkType(*b"PLTE").is_critical());
    assert!(!ChunkType(*b"tRNS").is_critical());
    assert!(!ChunkType(*b"gAMA").is_critical());
    assert_eq!(ChunkType(*b"IHDR").to_string(), "IHDR");
    assert_eq!(ChunkType([0, b'a', 0xFF, b'z']).to_string(), "\\x00a\\xffz");
}

/// 5.6 puts IHDR first, and nothing may be interpreted before it.
#[test]
fn a_file_whose_first_chunk_is_not_ihdr_is_refused() {
    let mut bytes = Vec::from(PNG_SIGNATURE);
    bytes.extend_from_slice(&chunk(b"PLTE", &[1, 2, 3]));
    bytes.extend_from_slice(&chunk(b"IHDR", &ihdr_data(1, 1, 8, 3, 0)));
    bytes.extend_from_slice(&idat(&[vec![0]]));
    bytes.extend_from_slice(&chunk(b"IEND", b""));
    assert_eq!(png_decode(&bytes, &CAP), Err(PngError::MissingHeader));

    // A 12-byte IHDR is not a header either.
    let short = png(&[0u8; 12], &[]);
    assert_eq!(png_decode(&short, &CAP), Err(PngError::MissingHeader));
    // Nor is a file that holds nothing but a signature.
    assert_eq!(
        png_decode(&PNG_SIGNATURE, &CAP),
        Err(PngError::MissingHeader)
    );
}

/// A file with a header and no IDAT has no image, which is a different answer
/// from a damaged one. PngSuite's `xdtn0g01` is exactly this file.
#[test]
fn a_file_with_no_idat_is_refused_rather_than_decoded_blank() {
    let bytes = png(&ihdr_data(4, 4, 8, 0, 0), &[]);
    assert_eq!(png_decode(&bytes, &CAP), Err(PngError::MissingImageData));
    // Distinct from an IDAT that is present and empty, which is damage: the
    // image exists and its rows are missing.
    let empty = png(&ihdr_data(4, 4, 8, 0, 0), &[chunk(b"IDAT", b"")]);
    let img = png_decode(&empty, &CAP).expect("an empty IDAT is damage, not absence");
    assert!(!img.complete);
    assert_eq!(img.data, vec![0u8; 16]);
}

/// IEND is PNG's end-of-data marker, so its absence is `EarlyEod` and bytes
/// after it are `TrailingGarbage` — the two names this crate already uses for
/// those two conditions everywhere else.
#[test]
fn a_missing_iend_warns_and_bytes_after_one_warn() {
    let rows = vec![vec![1u8, 2]];
    let mut truncated = Vec::from(PNG_SIGNATURE);
    truncated.extend_from_slice(&chunk(b"IHDR", &ihdr_data(2, 1, 8, 0, 0)));
    truncated.extend_from_slice(&idat(&rows));
    let img = png_decode(&truncated, &CAP).expect("degrade, not fail");
    assert_eq!(img.data, vec![1, 2]);
    assert!(img.warnings.contains(&Warning::EarlyEod));

    let mut trailing = simple(2, 1, 8, 0, &rows);
    trailing.extend_from_slice(b"and then some");
    let img = png_decode(&trailing, &CAP).expect("degrade, not fail");
    assert_eq!(img.data, vec![1, 2]);
    assert!(img.warnings.contains(&Warning::TrailingGarbage));

    // A clean file carries neither.
    let clean = png_decode(&simple(2, 1, 8, 0, &rows), &CAP).expect("clean");
    assert!(clean.warnings.is_empty(), "{:?}", clean.warnings);
}

/// A chunk length past 5.3's ceiling, or past the end of the file, stops the
/// walk rather than being trusted — and the image decodes from what was read.
#[test]
fn a_chunk_length_the_file_cannot_hold_stops_the_walk() {
    let mut bytes = Vec::from(PNG_SIGNATURE);
    bytes.extend_from_slice(&chunk(b"IHDR", &ihdr_data(2, 1, 8, 0, 0)));
    bytes.extend_from_slice(&idat(&[vec![5, 6]]));
    let mut over = bytes.clone();
    over.extend_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
    over.extend_from_slice(b"tEXt");
    let img = png_decode(&over, &CAP).expect("degrade");
    assert_eq!(img.data, vec![5, 6]);
    assert!(img.warnings.contains(&Warning::TruncatedInput));

    let mut lying = bytes.clone();
    lying.extend_from_slice(&1_000_000u32.to_be_bytes());
    lying.extend_from_slice(b"tEXt");
    lying.extend_from_slice(b"short");
    let img = png_decode(&lying, &CAP).expect("degrade");
    assert_eq!(img.data, vec![5, 6]);
    assert!(img.warnings.contains(&Warning::TruncatedInput));
}

// --- 11.2.3, PLTE -------------------------------------------------------

/// 11.2.3 bounds the palette against the bit depth, and the bound is per depth
/// rather than a flat 256: a 1-bit indexed image may name two colours, a 2-bit
/// four, a 4-bit sixteen.
///
/// The `max` in the refusal is asserted as well as the refusal, because a
/// decoder that checked against 256 for every depth would refuse the only case
/// a flat check catches and pass the three that matter.
#[test]
fn a_palette_larger_than_the_bit_depth_can_index_is_refused() {
    for (depth, entries) in [(1u8, 3usize), (2, 5), (4, 17), (8, 257)] {
        let plte = chunk(b"PLTE", &vec![0u8; entries * 3]);
        let bytes = png(&ihdr_data(1, 1, depth, 3, 0), &[plte, idat(&[vec![0]])]);
        assert_eq!(
            png_decode(&bytes, &CAP),
            Err(PngError::PaletteTooLarge {
                entries,
                max: 1usize << depth
            }),
            "depth {depth}"
        );
        // Exactly the limit is fine, which is what makes the check a bound
        // rather than an off-by-one.
        let ok = chunk(b"PLTE", &vec![0u8; (1usize << depth) * 3]);
        let bytes = png(&ihdr_data(1, 1, depth, 3, 0), &[ok, idat(&[vec![0]])]);
        assert!(
            png_decode(&bytes, &CAP).is_ok(),
            "depth {depth} at the limit"
        );
    }
}

/// Colour type 3 with no palette has nothing to index; a palette whose length
/// is not a multiple of three is not a palette.
#[test]
fn an_indexed_image_without_a_usable_palette_is_refused() {
    let bytes = png(&ihdr_data(1, 1, 8, 3, 0), &[idat(&[vec![0]])]);
    assert_eq!(png_decode(&bytes, &CAP), Err(PngError::MissingPalette));

    let bytes = png(
        &ihdr_data(1, 1, 8, 3, 0),
        &[chunk(b"PLTE", &[1, 2, 3, 4]), idat(&[vec![0]])],
    );
    assert_eq!(png_decode(&bytes, &CAP), Err(PngError::PaletteLength(4)));

    // On colour type 2 the same chunk is only a *suggested* palette, so a
    // malformed one is dropped rather than fatal.
    let bytes = png(
        &ihdr_data(1, 1, 8, 2, 0),
        &[chunk(b"PLTE", &[1, 2, 3, 4]), idat(&[vec![9, 8, 7]])],
    );
    let img = png_decode(&bytes, &CAP).expect("suggested palette is advisory");
    assert_eq!(img.data, vec![9, 8, 7]);
    assert!(img.warnings.contains(&Warning::PngChunkDropped));

    // And on colour types 0 and 4 it must not appear at all.
    let bytes = png(
        &ihdr_data(1, 1, 8, 0, 0),
        &[chunk(b"PLTE", &[1, 2, 3]), idat(&[vec![9]])],
    );
    let img = png_decode(&bytes, &CAP).expect("dropped, not fatal");
    assert_eq!(img.data, vec![9]);
    assert!(img.warnings.contains(&Warning::PngChunkDropped));
}

/// A sample past the end of PLTE is a black pixel and a warning, not a refusal
/// and not a panic: refusing would throw away every pixel that was fine
/// (ruling 2), and the surrounding pixels are asserted to prove it did not.
#[test]
fn a_palette_index_past_the_end_is_black_and_says_so() {
    let plte = chunk(b"PLTE", &[10, 20, 30, 40, 50, 60]);
    let bytes = png(&ihdr_data(3, 1, 8, 3, 0), &[plte, idat(&[vec![1, 250, 0]])]);
    let img = png_decode(&bytes, &CAP).expect("degrade");
    assert_eq!(img.data, vec![40, 50, 60, 0, 0, 0, 10, 20, 30]);
    assert!(img.warnings.contains(&Warning::PngPaletteIndexOutOfRange));
}

// --- 11.3.2.1, tRNS in all three forms ----------------------------------

/// Colour type 0: one grey key, compared against the **raw** sample.
///
/// The 4-bit case is the one that decides whether the comparison happens before
/// or after 13.12's scaling. `tRNS` stores 2 as `0x0002`; the scaled sample is
/// 34. A decoder comparing the scaled value against the stored one would find
/// no match at all and produce a fully opaque image — which looks entirely
/// reasonable until somebody notices the transparency is missing.
#[test]
fn trns_on_greyscale_keys_the_raw_sample_and_not_the_scaled_one() {
    let trns = chunk(b"tRNS", &2u16.to_be_bytes());
    let bytes = png(
        &ihdr_data(4, 1, 4, 0, 0),
        &[trns, idat(&[vec![0x02, 0x35]])],
    );
    let img = png_decode(&bytes, &CAP).expect("decode");
    assert_eq!(img.colour, PngColour::GreyAlpha);
    assert_eq!(img.bits_per_component, 8);
    // samples 0, 2, 3, 5 -> greys 0, 34, 51, 85; only the 2 is keyed out.
    assert_eq!(img.data, vec![0, 255, 34, 0, 51, 255, 85, 255]);

    // 8-bit, where raw and scaled coincide, so the two readings agree and the
    // test above is the only one that can tell them apart.
    let trns = chunk(b"tRNS", &200u16.to_be_bytes());
    let bytes = png(&ihdr_data(2, 1, 8, 0, 0), &[trns, idat(&[vec![200, 100]])]);
    let img = png_decode(&bytes, &CAP).expect("decode");
    assert_eq!(img.data, vec![200, 0, 100, 255]);

    // 16-bit, where the alpha is 0xFFFF rather than 0xFF.
    let trns = chunk(b"tRNS", &0x1234u16.to_be_bytes());
    let bytes = png(
        &ihdr_data(2, 1, 16, 0, 0),
        &[trns, idat(&[vec![0x12, 0x34, 0x00, 0x01]])],
    );
    let img = png_decode(&bytes, &CAP).expect("decode");
    assert_eq!(img.bits_per_component, 16);
    assert_eq!(
        img.data,
        vec![0x12, 0x34, 0x00, 0x00, 0x00, 0x01, 0xFF, 0xFF]
    );
}

/// Colour type 2: an RGB key, and all three components have to match. A
/// decoder testing only the red would key out the second pixel below too.
#[test]
fn trns_on_truecolour_keys_the_whole_triple() {
    let trns = chunk(b"tRNS", &[0, 1, 0, 2, 0, 3]);
    let bytes = png(
        &ihdr_data(3, 1, 8, 2, 0),
        &[trns, idat(&[vec![1, 2, 3, 1, 9, 9, 4, 5, 6]])],
    );
    let img = png_decode(&bytes, &CAP).expect("decode");
    assert_eq!(img.colour, PngColour::Rgba);
    assert_eq!(img.data, vec![1, 2, 3, 0, 1, 9, 9, 255, 4, 5, 6, 255]);
}

/// Colour type 3: one alpha per palette entry, entries past the list opaque —
/// and the *partial* values are the reason gap 29's table sends this case to
/// the decoder rather than through the pass-through, since PDF's colour-key
/// `/Mask` is a range test and cannot express 0x80.
#[test]
fn trns_on_a_palette_gives_each_entry_its_own_alpha() {
    let plte = chunk(b"PLTE", &[10, 11, 12, 20, 21, 22, 30, 31, 32, 40, 41, 42]);
    let trns = chunk(b"tRNS", &[0x00, 0x80]);
    let bytes = png(
        &ihdr_data(4, 1, 8, 3, 0),
        &[plte, trns, idat(&[vec![0, 1, 2, 3]])],
    );
    let img = png_decode(&bytes, &CAP).expect("decode");
    assert_eq!(img.colour, PngColour::Rgba);
    assert_eq!(
        img.data,
        vec![10, 11, 12, 0, 20, 21, 22, 0x80, 30, 31, 32, 255, 40, 41, 42, 255]
    );
    assert_eq!(
        png_scan(&bytes).expect("scan").transparency,
        Some(PngTransparency::Palette(vec![0x00, 0x80]))
    );
}

/// 11.3.2.1 forbids `tRNS` on colour types 4 and 6, which already carry alpha,
/// and it must follow PLTE on colour type 3. Both are dropped with a record
/// rather than silently absorbed.
#[test]
fn a_trns_that_does_not_belong_is_dropped_and_says_so() {
    let bytes = png(
        &ihdr_data(1, 1, 8, 6, 0),
        &[chunk(b"tRNS", &[0, 0]), idat(&[vec![1, 2, 3, 4]])],
    );
    let img = png_decode(&bytes, &CAP).expect("decode");
    assert_eq!(img.data, vec![1, 2, 3, 4]);
    assert!(img.warnings.contains(&Warning::PngChunkDropped));

    // Before PLTE, so there is nothing to index.
    let bytes = png(
        &ihdr_data(1, 1, 8, 3, 0),
        &[
            chunk(b"tRNS", &[0x40]),
            chunk(b"PLTE", &[7, 8, 9]),
            idat(&[vec![0]]),
        ],
    );
    let img = png_decode(&bytes, &CAP).expect("decode");
    assert_eq!(img.colour, PngColour::Rgb);
    assert_eq!(img.data, vec![7, 8, 9]);
    assert!(img.warnings.contains(&Warning::PngChunkDropped));

    // A grey key of the wrong length is not a key.
    let bytes = png(
        &ihdr_data(1, 1, 8, 0, 0),
        &[chunk(b"tRNS", &[0]), idat(&[vec![5]])],
    );
    let img = png_decode(&bytes, &CAP).expect("decode");
    assert_eq!(img.colour, PngColour::Grey);
    assert!(img.warnings.contains(&Warning::PngChunkDropped));
}

// --- 7.2, Adam7 ---------------------------------------------------------

/// Table 7.1 itself, as the 8 x 8 grid every copy of the specification prints.
///
/// Asserted directly against the constant, without decoding anything, because
/// this is the one place the numbers can be compared with the published table
/// rather than with their own consequences.
#[test]
fn the_adam7_pass_table_reproduces_the_published_grid() {
    #[rustfmt::skip]
    const GRID: [[u8; 8]; 8] = [
        [1, 6, 4, 6, 2, 6, 4, 6],
        [7, 7, 7, 7, 7, 7, 7, 7],
        [5, 6, 5, 6, 5, 6, 5, 6],
        [7, 7, 7, 7, 7, 7, 7, 7],
        [3, 6, 4, 6, 3, 6, 4, 6],
        [7, 7, 7, 7, 7, 7, 7, 7],
        [5, 6, 5, 6, 5, 6, 5, 6],
        [7, 7, 7, 7, 7, 7, 7, 7],
    ];
    let mut seen = [[0u8; 8]; 8];
    for (p, &(ystart, xstart, ystep, xstep)) in ADAM7.iter().enumerate() {
        let mut y = ystart;
        while y < 8 {
            let mut x = xstart;
            while x < 8 {
                assert_eq!(seen[y as usize][x as usize], 0, "two passes claim {x},{y}");
                seen[y as usize][x as usize] = p as u8 + 1;
                x += xstep;
            }
            y += ystep;
        }
    }
    assert_eq!(seen, GRID);

    // And the extents, for the sizes where a pass falls away entirely.
    assert_eq!(pass_extent(0, 8, 8), Some((1, 1)));
    assert_eq!(pass_extent(6, 8, 8), Some((8, 4)));
    assert_eq!(pass_extent(1, 1, 1), None, "pass 2 starts at column 4");
    assert_eq!(pass_extent(2, 1, 1), None, "pass 3 starts at row 4");
    assert_eq!(pass_extent(0, 1, 1), Some((1, 1)));
    assert_eq!(pass_extent(0, 5, 3), Some((1, 1)));
    assert_eq!(pass_extent(6, 5, 3), Some((5, 1)));
    assert_eq!(pass_extent(7, 8, 8), None, "there is no eighth pass");
}

/// **The interlace test, and the reason it is built the way it is.**
///
/// Gap 17 transposed two JBIG2 context bits and T.88's own Annex H.1 stream
/// still decoded byte for byte, because a relabelling is a bijection and a
/// comparative test cannot see one. Adam7 invites exactly that mistake — the
/// pass table's four columns are two pairs that read alike — and a transposed
/// pass is a *permutation* of the pixels. So the image here is eight by eight
/// with all sixty-four samples distinct: pixel (x, y) is y * 8 + x, and any
/// misplacement moves a value somewhere it can be named.
///
/// The fixture was produced outside this repository. `zlib` and `binascii` from
/// Python's standard library built the stream and the CRCs, the interlacing was
/// written from Table 7.1 in a separate script, and **Pillow** — a third-party
/// PNG implementation sharing no code with this one — decoded both files to
/// 0..63 in row-major order before either was committed.
#[test]
fn adam7_places_all_sixty_four_distinct_pixels() {
    let img = decode(&ADAM7_8X8);
    assert_eq!((img.width, img.height), (8, 8));
    assert_eq!(img.colour, PngColour::Grey);
    assert!(img.complete);
    let want: Vec<u8> = (0..64u8).collect();
    assert_eq!(img.data, want);

    // Every sample distinct, restated as a property rather than trusted from
    // the literal above: a decoder that duplicated a pass would repeat one.
    let mut sorted = img.data.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 64);

    // The non-interlaced twin, which additionally cycles filter types 0..4 so
    // that the two fixtures differ in every way a PNG can differ and still
    // agree on the picture.
    let plain = decode(&PLAIN_8X8);
    assert_eq!(plain.data, want);
    assert_eq!(plain.data, img.data, "interlace changed the image");
    assert!(png_scan(&ADAM7_8X8).expect("scan").header.interlaced);
    assert!(!png_scan(&PLAIN_8X8).expect("scan").header.interlaced);
}

/// A small interlaced image, where most passes hold no pixels at all and
/// therefore contribute **no bytes** — not a zero-height pass with a filter tag.
///
/// A decoder that emitted a tag byte for an empty pass would consume the wrong
/// number of bytes and mis-place everything after it, which on an 8 x 8 image
/// is invisible because every pass is populated.
#[test]
fn interlaced_images_too_small_for_a_pass_skip_it_entirely() {
    // 1 x 1: only pass 1 exists.
    let one = png(&ihdr_data(1, 1, 8, 0, 1), &[idat(&[vec![0x2A]])]);
    let img = decode(&one);
    assert_eq!(img.data, vec![0x2A]);
    assert!(img.complete);

    // 5 x 3, where passes 1, 2, 4, 6 and 7 are populated and 3 and 5 are not.
    // Written as one row per pass in Table 7.1's order.
    let pixels: Vec<u8> = (0..15u8).collect();
    let mut raw: Vec<u8> = Vec::new();
    for (p, &(ystart, xstart, ystep, xstep)) in ADAM7.iter().enumerate() {
        let Some((pw, ph)) = pass_extent(p, 5, 3) else {
            continue;
        };
        for j in 0..ph {
            raw.push(0);
            for i in 0..pw {
                let (x, y) = (xstart + i * xstep, ystart + j * ystep);
                raw.push(pixels[(y * 5 + x) as usize]);
            }
        }
    }
    let bytes = png(
        &ihdr_data(5, 3, 8, 0, 1),
        &[chunk(b"IDAT", &zlib_compress(&raw))],
    );
    let img = decode(&bytes);
    assert_eq!(img.data, pixels);
    assert!(img.complete);
}

// --- degradation (ruling 2) ---------------------------------------------

/// A truncated IDAT keeps the rows it had, warns, and says it is incomplete —
/// rather than refusing an image most of which arrived.
#[test]
fn a_truncated_idat_keeps_the_rows_it_managed() {
    let rows: Vec<Vec<u8>> = (0..4u8).map(|y| vec![y * 10, y * 10 + 1]).collect();
    let full = simple(2, 4, 8, 0, &rows);
    assert_eq!(decode(&full).data, vec![0, 1, 10, 11, 20, 21, 30, 31]);

    // Cut the compressed stream in half and rebuild the file around it.
    let mut raw = Vec::new();
    for r in &rows {
        raw.push(0u8);
        raw.extend_from_slice(r);
    }
    let stream = zlib_compress(&raw);
    let cut = png(
        &ihdr_data(2, 4, 8, 0, 1 - 1),
        &[chunk(b"IDAT", &stream[..stream.len() / 2])],
    );
    let img = png_decode(&cut, &CAP).expect("degrade rather than fail");
    assert!(!img.complete);
    assert_eq!(img.data.len(), 8, "the raster is still the declared size");
    assert!(img
        .warnings
        .iter()
        .any(|w| matches!(w, Warning::TruncatedInput | Warning::CorruptDeflateStream)));
}

/// PNG's IDAT is zlib-wrapped (10.3), so an unwrapped one is a real leniency
/// and gets the crate's existing name for it. This is the one place
/// `RawDeflateFallback` means something here rather than being noise, which is
/// why the PNG decoder goes through the sniffing door and the ZIP reader does
/// not.
#[test]
fn an_idat_with_no_zlib_wrapper_decodes_and_records_the_leniency() {
    let raw = vec![0u8, 1, 2];
    let bytes = png(
        &ihdr_data(2, 1, 8, 0, 0),
        &[chunk(b"IDAT", &crate::deflate::deflate(&raw))],
    );
    let img = png_decode(&bytes, &CAP).expect("decode");
    assert_eq!(img.data, vec![1, 2]);
    assert!(img.warnings.contains(&Warning::RawDeflateFallback));
}

/// **A row that stops between two samples of one pixel places no pixel.**
///
/// Written because milestone 3's injection matrix caught nothing when the
/// short-row pixel count was divided by the bits in a *sample* rather than the
/// bits in a *pixel*. The two agree on every complete row and on every
/// one-channel image, which is what the rest of this file is made of, so the
/// defect survived a suite of thirty tests.
///
/// It matters because the wrong count writes a pixel from samples that were
/// never in the file: the red of a truncated RGB triple, beside two
/// zeroes — a saturated red where the truth is that nothing arrived. Damage
/// should look like damage.
#[test]
fn a_row_that_stops_mid_pixel_places_no_partial_pixel() {
    // Three RGB pixels a row, two rows, and the second row cut after one byte
    // — so its red is present and its green and blue are not. The stream is
    // *complete*; it is the raster that is short, which is why this is a
    // different fixture from the truncated-IDAT one above.
    let mut raw = vec![0u8];
    raw.extend_from_slice(&[10, 20, 30, 40, 50, 60, 70, 80, 90]);
    raw.extend_from_slice(&[0u8, 0xAA]);
    let bytes = png(
        &ihdr_data(3, 2, 8, 2, 0),
        &[chunk(b"IDAT", &zlib_compress(&raw))],
    );
    let img = png_decode(&bytes, &CAP).expect("degrade");
    assert_eq!(
        img.data,
        vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        "a pixel with only its red sample must not be drawn"
    );
    assert!(!img.complete);
    assert!(img.warnings.contains(&Warning::PredictorRowShort));

    // And the same at 16 bits, where a pixel is six bytes and a row can stop
    // in the middle of a single *component*.
    let mut raw = vec![0u8];
    raw.extend_from_slice(&[0, 1, 0, 2, 0, 3]);
    raw.extend_from_slice(&[0u8, 0xFF, 0xFF, 0xFF]);
    let bytes = png(
        &ihdr_data(1, 2, 16, 2, 0),
        &[chunk(b"IDAT", &zlib_compress(&raw))],
    );
    let img = png_decode(&bytes, &CAP).expect("degrade");
    assert_eq!(img.data, vec![0, 1, 0, 2, 0, 3, 0, 0, 0, 0, 0, 0]);
}

/// The IDAT went through the **zlib** door, so its Adler-32 was checked.
///
/// 10.3 wraps the compressed datastream, unlike a ZIP entry, which is raw
/// DEFLATE by definition — and the whole reason `inflate_raw` exists as a
/// separate entry point is that the two containers want different doors. This
/// is the assertion that says this one took the door with the checksum on it:
/// corrupt the Adler and nothing else, and the decode says so.
#[test]
fn the_idat_adler_is_verified_because_png_is_zlib_wrapped() {
    let raw = vec![0u8, 7, 8];
    let mut stream = zlib_compress(&raw);
    let n = stream.len();
    let img = png_decode(
        &png(&ihdr_data(2, 1, 8, 0, 0), &[chunk(b"IDAT", &stream)]),
        &CAP,
    )
    .expect("decode");
    assert!(img.warnings.is_empty(), "{:?}", img.warnings);
    assert_eq!(img.data, vec![7, 8]);

    stream[n - 1] ^= 0x01;
    let img = png_decode(
        &png(&ihdr_data(2, 1, 8, 0, 0), &[chunk(b"IDAT", &stream)]),
        &CAP,
    )
    .expect("degrade rather than fail: the pixels arrived");
    assert_eq!(img.data, vec![7, 8]);
    assert!(img.warnings.contains(&Warning::AdlerMismatch));
    assert!(!img.complete);
}

// --- the bounds ---------------------------------------------------------

/// `MAX_PNG_SAMPLES` fires, and it fires **by its own refusal** rather than by
/// how long anything took — `5adf502`'s method, for its stated reason: a timing
/// assertion fails on a slow machine and passes on a fast one with the budget
/// removed.
///
/// Thirteen bytes of IHDR is the whole attack, which is what makes the cap
/// reachable in the sense gap 18a milestone 8 found `MAX_JPX_WORK` was not.
#[test]
fn an_image_past_the_sample_cap_is_refused_before_it_allocates() {
    // 2^31 - 1 square, at four charged components: about 1.8 x 10^19 samples.
    let bytes = png(&ihdr_data(0x7FFF_FFFF, 0x7FFF_FFFF, 8, 2, 0), &[]);
    let Err(PngError::TooManySamples { samples, max }) = png_decode(&bytes, &CAP) else {
        panic!("the sample cap did not fire");
    };
    assert_eq!(max, MAX_PNG_SAMPLES);
    assert!(samples > MAX_PNG_SAMPLES);

    // One sample past it, so the cap is a bound rather than an order of
    // magnitude. 4096 x 4096 x 4 is exactly the cap; one more row is not.
    let at = png(&ihdr_data(4096, 4096, 8, 6, 0), &[]);
    assert_eq!(png_decode(&at, &CAP), Err(PngError::MissingImageData));
    let over = png(&ihdr_data(4096, 4097, 8, 6, 0), &[]);
    assert!(matches!(
        png_decode(&over, &CAP),
        Err(PngError::TooManySamples { .. })
    ));

    // A single-pixel-wide image is bounded by the same product, which is why
    // there is no separate dimension cap.
    let tall = png(&ihdr_data(1, 0x7FFF_FFFF, 8, 0, 0), &[]);
    assert!(matches!(
        png_decode(&tall, &CAP),
        Err(PngError::TooManySamples { .. })
    ));

    // And zero is refused with its own name rather than as a sample count.
    let empty = png(&ihdr_data(0, 4, 8, 0, 0), &[]);
    assert_eq!(
        png_decode(&empty, &CAP),
        Err(PngError::BadDimensions {
            width: 0,
            height: 4
        })
    );
}

/// The caller's own ceiling refuses under its own name and with its own number,
/// so a host that lowered it can tell its decision from this crate's.
#[test]
fn the_callers_output_ceiling_refuses_with_the_callers_number() {
    let rows: Vec<Vec<u8>> = (0..4u8).map(|_| vec![0; 4]).collect();
    let bytes = simple(4, 4, 8, 0, &rows);
    assert!(png_decode(&bytes, &Limits::new(16)).is_ok());
    assert_eq!(
        png_decode(&bytes, &Limits::new(15)),
        Err(PngError::ExceedsOutputLimit {
            bytes: 16,
            limit: 15
        })
    );
}

/// An IDAT that inflates past the raster is stopped at the raster, and the
/// excess is reported rather than absorbed.
///
/// This is the zlib bomb, and the answer to it is that the ceiling comes from
/// IHDR — which is already under `MAX_PNG_SAMPLES` — rather than from a second
/// constant somebody would have to keep in step.
#[test]
fn an_idat_that_inflates_past_the_raster_stops_at_the_raster() {
    // A 4 x 1 greyscale wants five bytes of filtered data. Give it a megabyte.
    let mut raw = vec![0u8; 1 << 20];
    raw[0] = 0;
    let bytes = png(
        &ihdr_data(4, 1, 8, 0, 0),
        &[chunk(b"IDAT", &zlib_compress(&raw))],
    );
    let img = png_decode(&bytes, &CAP).expect("decode what the raster holds");
    assert_eq!(img.data.len(), 4);
    assert!(img.warnings.contains(&Warning::OutputCapHit));
    assert!(!img.complete);
}

/// The ledger in [`MAX_PNG_SAMPLES`]'s doc comment, checked as arithmetic
/// rather than left as prose — gap 18a milestone 8's failure was a constant
/// whose relationship to its own inputs had never been written down.
#[test]
fn the_bounds_ledger_says_what_the_arithmetic_says() {
    // The largest fixture in this module: 32 x 32 RGBA.
    let rows: Vec<Vec<u8>> = (0..32u8)
        .map(|y| (0..128u8).map(|x| x ^ y).collect())
        .collect();
    let big = simple(32, 32, 8, 6, &rows);
    let img = decode(&big);
    let spent = u64::from(img.width) * u64::from(img.height) * u64::from(img.colour.components());
    assert_eq!(spent, 4_096, "the ledger's first number");
    assert!(spent * 16_384 == MAX_PNG_SAMPLES);

    // The second: gap 29's yardstick page, 2000 x 3000 with alpha.
    let page = 2000u64 * 3000 * 4;
    assert_eq!(page, 24_000_000);
    assert!(
        page < MAX_PNG_SAMPLES,
        "a real comic page must not be refused"
    );

    // The third, and the margin between them.
    assert_eq!(MAX_PNG_SAMPLES, 67_108_864);
    assert_eq!(MAX_PNG_SAMPLES, 4096 * 4096 * 4);
    assert!(MAX_PNG_SAMPLES * 10 / page == 27, "2.7x of margin");
}

// --- the pass-through seam gap 29 milestone 4 will use ------------------

/// [`png_scan`] hands back the header, the palette, the transparency and the
/// compressed bytes **without inflating**, which is what makes gap 29's
/// pass-through possible at all: decoding every PNG in a 200-page archive holds
/// about 3.6 GB, and copying the IDAT holds the archive.
#[test]
fn the_scan_hands_back_everything_but_the_pixels() {
    let plte = chunk(b"PLTE", &[1, 2, 3, 4, 5, 6]);
    let trns = chunk(b"tRNS", &[0x10]);
    let row = [0u8, 1];
    let mut raw = vec![0u8];
    raw.extend_from_slice(&row);
    let stream = zlib_compress(&raw);
    let bytes = png(
        &ihdr_data(2, 1, 8, 3, 0),
        &[plte, trns, chunk(b"IDAT", &stream)],
    );

    let scan = png_scan(&bytes).expect("scan");
    assert_eq!(
        scan.header,
        PngHeader {
            width: 2,
            height: 1,
            bit_depth: 8,
            colour_type: 3,
            interlaced: false
        }
    );
    assert_eq!(scan.palette, vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(
        scan.transparency,
        Some(PngTransparency::Palette(vec![0x10]))
    );
    assert_eq!(scan.idat, stream, "the IDAT must arrive untouched");
    assert!(scan.warnings.is_empty());

    // And decoding from the scan is the same picture as decoding the file, so
    // milestone 4 can hold one scan and choose either route from it.
    assert_eq!(scan.decode(&CAP).expect("decode"), decode(&bytes));
    assert_eq!(scan.header.channels(), 1);
}

/// The channel counts of Table 11.1, which is what `/Colors` becomes on the
/// pass-through path.
#[test]
fn the_channel_count_is_table_11_1s_own_column() {
    let h = |colour_type| PngHeader {
        width: 1,
        height: 1,
        bit_depth: 8,
        colour_type,
        interlaced: false,
    };
    assert_eq!(h(0).channels(), 1);
    assert_eq!(h(2).channels(), 3);
    assert_eq!(h(3).channels(), 1, "indices, not colours");
    assert_eq!(h(4).channels(), 2);
    assert_eq!(h(6).channels(), 4);
    assert_eq!(PngColour::Grey.components(), 1);
    assert_eq!(PngColour::GreyAlpha.components(), 2);
    assert_eq!(PngColour::Rgb.components(), 3);
    assert_eq!(PngColour::Rgba.components(), 4);
    // 7.2's row formula, including the sub-byte rounding.
    assert_eq!(h(0).row_bytes(1), 1);
    assert_eq!(h(2).row_bytes(3), 9);
    assert_eq!(
        PngHeader {
            bit_depth: 1,
            ..h(0)
        }
        .row_bytes(9),
        2
    );
}

// --- ruling 1, on arbitrary bytes ---------------------------------------

/// Two cheap versions of the fuzz target, run on every `cargo test`, because a
/// panic introduced today should not wait on a fuzz session — the shape
/// `tinker-pdf-zip` landed in milestone 2.
#[test]
fn truncating_a_good_file_anywhere_never_panics() {
    for file in [&ADAM7_8X8[..], &PLAIN_8X8[..]] {
        for n in 0..file.len() {
            let _ = png_decode(&file[..n], &CAP);
            let _ = png_scan(&file[..n]);
        }
    }
}

#[test]
fn flipping_any_single_byte_never_panics() {
    let file = PLAIN_8X8.to_vec();
    for i in 0..file.len() {
        for bit in [0x01u8, 0x80] {
            let mut bad = file.clone();
            bad[i] ^= bit;
            let _ = png_decode(&bad, &CAP);
            let _ = png_decode(&bad, &Limits::new(1));
        }
    }
}

#[test]
fn arbitrary_bytes_are_refused_rather_than_decoded() {
    for input in [
        &b""[..],
        &[0u8; 64][..],
        &[0xFFu8; 8][..],
        b"%PDF-1.7\n",
        b"PK\x03\x04",
        &PNG_SIGNATURE[..],
    ] {
        let _ = png_decode(input, &CAP);
    }
    // A signature followed by nothing but zeros parses as a zero-length chunk
    // whose type is `\0\0\0\0` — critical by 5.4, since bit 5 of the first byte
    // is clear — declaring a CRC of zero. The **checksum** is what refuses it,
    // and it refuses first: `crc32` of those four bytes is not zero, and no
    // interpretation of a chunk happens before its integrity is established.
    let mut zeros = Vec::from(PNG_SIGNATURE);
    zeros.extend_from_slice(&[0u8; 32]);
    assert_eq!(
        png_decode(&zeros, &CAP),
        Err(PngError::ChunkCrc(ChunkType([0, 0, 0, 0])))
    );
    // With the CRC made right, the same chunk is refused for what it is.
    let mut named = Vec::from(PNG_SIGNATURE);
    named.extend_from_slice(&chunk(&[0, 0, 0, 0], b""));
    assert_eq!(
        png_decode(&named, &CAP),
        Err(PngError::MissingHeader),
        "5.6 puts IHDR first, and this is not it"
    );
}

/// Every [`PngError`] renders, because a refusal a host cannot show is a
/// refusal that becomes "something went wrong".
#[test]
fn every_refusal_has_a_sentence() {
    let all = [
        PngError::NotPng,
        PngError::MissingHeader,
        PngError::ChunkCrc(IHDR),
        PngError::UnknownCriticalChunk(ChunkType(*b"ZzZZ")),
        PngError::BadColourTypeDepth {
            colour_type: 1,
            bit_depth: 8,
        },
        PngError::BadDimensions {
            width: 0,
            height: 1,
        },
        PngError::UnsupportedCompression(1),
        PngError::UnsupportedFilterMethod(1),
        PngError::UnsupportedInterlace(2),
        PngError::MissingPalette,
        PngError::PaletteLength(4),
        PngError::PaletteTooLarge { entries: 3, max: 2 },
        PngError::MissingImageData,
        PngError::TooManySamples {
            samples: 1,
            max: MAX_PNG_SAMPLES,
        },
        PngError::ExceedsOutputLimit { bytes: 2, limit: 1 },
    ];
    for e in all {
        assert!(!e.to_string().is_empty(), "{e:?}");
    }
    assert_eq!(
        PngError::BadColourTypeDepth {
            colour_type: 1,
            bit_depth: 8
        }
        .to_string(),
        "colour type 1 with bit depth 8 is not in Table 11.1"
    );
    assert_eq!(
        PngError::ChunkCrc(IDAT).to_string(),
        "CRC mismatch on critical chunk IDAT"
    );
    assert_eq!(Warning::PngChunkDropped.as_str(), "png-chunk-dropped");
}

/// IHDR's three method bytes each have exactly one defined value, and a file
/// naming another is refused by name rather than decoded on the assumption
/// that it meant zero.
#[test]
fn the_three_method_bytes_admit_only_their_defined_value() {
    let with = |compression: u8, filter: u8, interlace: u8| {
        let mut d = ihdr_data(1, 1, 8, 0, interlace);
        d[10] = compression;
        d[11] = filter;
        png(&d, &[idat(&[vec![1]])])
    };
    assert!(png_decode(&with(0, 0, 0), &CAP).is_ok());
    assert!(png_decode(&with(0, 0, 1), &CAP).is_ok());
    assert_eq!(
        png_decode(&with(1, 0, 0), &CAP),
        Err(PngError::UnsupportedCompression(1))
    );
    assert_eq!(
        png_decode(&with(0, 1, 0), &CAP),
        Err(PngError::UnsupportedFilterMethod(1))
    );
    assert_eq!(
        png_decode(&with(0, 0, 2), &CAP),
        Err(PngError::UnsupportedInterlace(2))
    );
}

/// A second IHDR or PLTE is dropped rather than allowed to redefine the image
/// half way through it.
#[test]
fn a_repeated_header_or_palette_is_dropped() {
    let bytes = png(
        &ihdr_data(2, 1, 8, 0, 0),
        &[
            chunk(b"IHDR", &ihdr_data(9, 9, 8, 6, 1)),
            idat(&[vec![1, 2]]),
        ],
    );
    let img = png_decode(&bytes, &CAP).expect("decode");
    assert_eq!((img.width, img.height), (2, 1));
    assert_eq!(img.data, vec![1, 2]);
    assert!(img.warnings.contains(&Warning::PngChunkDropped));

    let bytes = png(
        &ihdr_data(1, 1, 8, 3, 0),
        &[
            chunk(b"PLTE", &[1, 2, 3]),
            chunk(b"PLTE", &[9, 9, 9]),
            idat(&[vec![0]]),
        ],
    );
    let img = png_decode(&bytes, &CAP).expect("decode");
    assert_eq!(img.data, vec![1, 2, 3]);
    assert!(img.warnings.contains(&Warning::PngChunkDropped));
}

// --- the two fixtures produced outside this repository -------------------

/// An 8 x 8 greyscale, **not interlaced**, whose pixel (x, y) is `y * 8 + x`,
/// with the five filter types of 9.2 cycling one per row.
///
/// Produced by a Python script — `zlib` for the stream, `binascii.crc32` for
/// the chunk checksums, the filters written from 9.2 — so nothing in this tree
/// touched it, and then decoded by **Pillow**, which agreed it is 0..63 in
/// row-major order before it was committed. It is the twin of [`ADAM7_8X8`] and
/// the two must agree.
///
/// The generating script is recorded in gap 29's milestone 3 note rather than
/// committed, for the reason ruling 9 gives about oracles: what is worth
/// keeping is the artefact it produced and the record of where it came from.
#[rustfmt::skip]
const PLAIN_8X8: [u8; 102] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
    0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x08,
    0x08, 0x00, 0x00, 0x00, 0x00, 0xE1, 0x64, 0xE1, 0x57, 0x00, 0x00, 0x00,
    0x2D, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x60, 0x60, 0x64, 0x62,
    0x66, 0x61, 0x65, 0x63, 0x67, 0xE4, 0x60, 0x84, 0x00, 0x26, 0x0E, 0x28,
    0x60, 0x16, 0x60, 0x85, 0x00, 0x16, 0x98, 0x14, 0x83, 0x86, 0xA6, 0x96,
    0xB6, 0x8E, 0xAE, 0x9E, 0x3E, 0xA3, 0x01, 0xBA, 0x62, 0x00, 0x46, 0x8C,
    0x02, 0x8E, 0xE1, 0x91, 0x45, 0xA3, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// The same image, **Adam7 interlaced**, every row filtered with type 0 so that
/// a misplacement cannot hide inside a prediction.
///
/// Same provenance as [`PLAIN_8X8`], and the same Pillow confirmation. This is
/// the fixture `adam7_places_all_sixty_four_distinct_pixels` exists for.
#[rustfmt::skip]
const ADAM7_8X8: [u8; 143] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D,
    0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x08,
    0x08, 0x00, 0x00, 0x00, 0x01, 0x96, 0x63, 0xD1, 0xC1, 0x00, 0x00, 0x00,
    0x56, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x05, 0xC1, 0x87, 0x02, 0x42,
    0x00, 0x00, 0x05, 0xC0, 0x67, 0xCF, 0x64, 0x85, 0xEC, 0xEC, 0x96, 0x55,
    0x29, 0x32, 0xFE, 0xFF, 0xAF, 0xDC, 0x01, 0xA0, 0x11, 0x67, 0x20, 0x59,
    0x24, 0x05, 0x54, 0xDD, 0xB4, 0xD0, 0xF6, 0xEF, 0x01, 0x04, 0xC5, 0x70,
    0xD0, 0x8C, 0x93, 0x8D, 0x4B, 0x9A, 0x97, 0xE8, 0x5E, 0x9F, 0x2F, 0x78,
    0x41, 0x94, 0xE4, 0x83, 0x72, 0x84, 0x73, 0x76, 0x3D, 0x3F, 0x08, 0x23,
    0x54, 0xD7, 0xDB, 0xFD, 0xF1, 0xAC, 0x1B, 0xFC, 0xC6, 0xE9, 0x3F, 0x2F,
    0xEB, 0xB6, 0x03, 0xEE, 0x63, 0x07, 0xE1, 0x24, 0x5F, 0x6E, 0x8F, 0x00,
    0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];
