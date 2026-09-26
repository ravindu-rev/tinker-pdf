//! What the bzip2 decoder is held to beside the committed archives.
//!
//! The streams below are libbzip2 1.0.8's, through CPython 3.11.15's `bz2`
//! module (`bz2.compress(data, 9)`), each small enough to read as hex; their
//! expected answer is the `data` they were made from, never another decoder's
//! output (ruling 13). The larger and more demanding fixtures — two blocks,
//! six Huffman groups, every run length either side of the run-length pass's
//! thresholds — are in `tests/coders/` and read by `tests/coders.rs`.

use super::*;

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("hex"))
        .collect()
}

const ROOMY: Limits = Limits {
    max_unpacked: 1 << 20,
};

/// `bz2.compress(b"", 9)`: a stream with no block, only the end marker and a
/// combined CRC of zero. A ZIP writer emits exactly this for an empty entry.
const EMPTY: &str = "425a683917724538509000000000";
/// `bz2.compress(b"a", 9)`.
const ONE: &str = "425a683931415926535919939b6b00000001002000200021184682ee48a70a120332736d60";
/// `bz2.compress(b"aaaa", 9)`: four equal bytes, which the run-length pass
/// writes as four bytes and a count of zero.
const FOUR: &str = "425a6839314159265359881233a600000241004000200020002100820b177245385090881233a6";
/// `bz2.compress(b"aaaaa" * 3, 9)`: fifteen, four and a count of eleven.
const FIFTEEN: &str =
    "425a6839314159265359005cfc4700000241000008200020002100820b177245385090005cfc47";
/// `bz2.compress(b"Hello, hello, hello. " * 3, 9)`.
const HELLO: &str = "425a6839314159265359654ed43800000a1500400500400244a000310c08114d262460f216f9d183a87087c5dc914e14241953b50e00";

#[test]
fn small_streams_from_libbzip2_decode_to_what_went_in() {
    for (stream, want) in [
        (EMPTY, Vec::new()),
        (ONE, b"a".to_vec()),
        (FOUR, b"aaaa".to_vec()),
        (FIFTEEN, b"aaaaa".repeat(3)),
        (HELLO, b"Hello, hello, hello. ".repeat(3)),
    ] {
        assert_eq!(decode(&hex(stream), &ROOMY), Ok(want), "{stream}");
    }
}

/// CRC-32/BZIP2's published check value: the CRC of the nine ASCII digits.
/// The reflected CRC-32 of the same digits is `CBF43926`; this one is not.
#[test]
fn the_block_crc_is_the_msb_first_crc_32_and_not_zip_s() {
    let mut crc = u32::MAX;
    for &byte in b"123456789" {
        crc = crc_update(crc, byte);
    }
    assert_eq!(!crc, 0xFC89_1918);
}

#[test]
fn two_streams_end_to_end_are_one_output() {
    let mut both = hex(HELLO);
    both.extend(hex(FIFTEEN));
    let mut want = b"Hello, hello, hello. ".repeat(3);
    want.extend(b"aaaaa".repeat(3));
    assert_eq!(decode(&both, &ROOMY), Ok(want));
    // Bytes after a complete stream that are not another stream end the
    // decode; the container's length and CRC are what judge them.
    let mut trailing = hex(ONE);
    trailing.extend_from_slice(b"\0\0\0\0");
    assert_eq!(decode(&trailing, &ROOMY), Ok(b"a".to_vec()));
}

#[test]
fn a_stream_that_is_not_bzip2_is_refused_by_name() {
    assert_eq!(decode(b"", &ROOMY), Err(Error::NotBzip2));
    assert_eq!(decode(b"BZh0", &ROOMY), Err(Error::NotBzip2));
    assert_eq!(decode(b"BZx9", &ROOMY), Err(Error::NotBzip2));
    let mut bad = hex(ONE);
    bad[4] ^= 0x01; // inside the block magic
    assert_eq!(decode(&bad, &ROOMY), Err(Error::BadMagic));
}

#[test]
fn every_checksum_is_checked() {
    // The block's own CRC, the eleventh to fourteenth bytes.
    let mut bad = hex(HELLO);
    bad[10] ^= 0x01;
    assert_eq!(decode(&bad, &ROOMY), Err(Error::BlockCrcMismatch));
    // The combined CRC, the last four bytes but for the padding.
    let good = hex(HELLO);
    let mut bad = good.clone();
    let end = bad.len();
    bad[end - 2] ^= 0x10;
    assert_eq!(decode(&bad, &ROOMY), Err(Error::StreamCrcMismatch));
}

/// The randomised bit is the first bit after the block CRC: setting it in a
/// real stream is refused before anything else in the block is read.
#[test]
fn a_randomised_block_is_refused_by_name() {
    let mut bytes = hex(HELLO);
    bytes[14] |= 0x80;
    assert_eq!(decode(&bytes, &ROOMY), Err(Error::Randomised));
}

#[test]
fn the_output_cap_is_a_refusal_and_not_a_truncation() {
    let fifteen = hex(FIFTEEN);
    assert_eq!(
        decode(&fifteen, &Limits { max_unpacked: 15 }),
        Ok(b"aaaaa".repeat(3))
    );
    // The run of eleven is refused as a whole, before any of it is written.
    assert_eq!(
        decode(&fifteen, &Limits { max_unpacked: 14 }),
        Err(Error::TooLarge)
    );
    assert_eq!(
        decode(&hex(ONE), &Limits { max_unpacked: 0 }),
        Err(Error::TooLarge)
    );
}

#[test]
fn a_stream_cut_anywhere_is_refused_and_never_panics() {
    for stream in [EMPTY, ONE, FOUR, FIFTEEN, HELLO] {
        let bytes = hex(stream);
        for cut in 0..bytes.len() {
            assert!(
                decode(&bytes[..cut], &ROOMY).is_err(),
                "{stream} cut at {cut}"
            );
        }
    }
}

/// Every single-bit flip of every small stream answers something, and when it
/// answers `Ok` it is within the cap. Most flips are caught by a checksum;
/// the point is that none of them panics or loops.
#[test]
fn every_bit_flip_of_a_real_stream_answers_rather_than_panics() {
    for stream in [ONE, FOUR, FIFTEEN, HELLO] {
        let bytes = hex(stream);
        for at in 0..bytes.len() {
            for bit in 0..8 {
                let mut damaged = bytes.clone();
                damaged[at] ^= 1 << bit;
                if let Ok(out) = decode(&damaged, &Limits { max_unpacked: 4096 }) {
                    assert!(out.len() <= 4096);
                }
            }
        }
    }
}
