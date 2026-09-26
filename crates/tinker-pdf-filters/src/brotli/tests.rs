//! The parts of RFC 7932 that are *transcribed* rather than implemented.
//!
//! A decoder can be tested by decoding things, and `tests/brotli_vectors.rs`
//! does that. What decoding cannot test is a table that was copied out of a
//! specification and copied slightly wrong — a single mistyped byte in the
//! 122 KiB dictionary breaks exactly the streams that reference that word and
//! nothing else, which is a bug that ships.
//!
//! RFC 7932 anticipated that and publishes a CRC-32 for four of its tables.
//! Every one of them is checked here against the crate's own `crc32`, so the
//! specification is the thing that says whether the transcription is right.

use super::{
    alphabet_bits, apply_transform, inverse_move_to_front, PrefixCode, DICTIONARY, DOFFSET, NDBITS,
    TRANSFORMS,
};
use crate::{crc32, Limits};

// ---- the transcribed tables, against the RFC's own check values -------------

/// **Appendix A's dictionary is the dictionary, to the byte.**
///
/// The RFC states the length and the CRC-32 in the sentence that introduces
/// the hexadecimal dump, which makes this the strongest check available on a
/// vendored blob: not "it is 122 784 bytes long" but "it is *these* 122 784
/// bytes".
#[test]
fn the_dictionary_matches_the_published_crc32() {
    assert_eq!(DICTIONARY.len(), 122_784, "Appendix A's stated length");
    assert_eq!(
        crc32(DICTIONARY),
        0x5136_cb04,
        "Appendix A's stated CRC-32 of the DICT array"
    );
}

/// **§8's `DOFFSET` recursion lands exactly on the end of the dictionary.**
///
/// `DICTSIZE = DOFFSET[24] + 24 * NWORDS[24]`, and the recursion is driven by
/// `NDBITS`, so a mistyped bit depth would put every word of every longer
/// length at the wrong offset. That the arithmetic ends on the last byte of
/// the blob is the check that the two agree.
#[test]
fn the_offset_recursion_ends_at_the_end_of_the_dictionary() {
    let words = 1usize << NDBITS[24];
    assert_eq!(DOFFSET[24] as usize + 24 * words, DICTIONARY.len());
    // §8: `NWORDS[length] = 0 (if length < 4)`, which the bit-depth array
    // spells as a zero for each of the first four lengths.
    assert!(
        NDBITS.iter().take(4).all(|&bits| bits == 0),
        "no dictionary words shorter than four"
    );
}

/// **Appendix B's 121 transformations, re-encoded the way Appendix B says.**
///
/// "each transform is the prefix sequence of bytes plus a terminating zero
/// byte, a single-byte value identifying the transform, and the suffix
/// sequence of bytes plus a terminating zero" — 648 bytes, CRC-32 0x3d965f81.
/// This rebuilds that sequence from the table the decoder actually reads, so
/// a wrong prefix, a wrong suffix and a wrong elementary transform are all
/// caught, and by the specification rather than by a font.
#[test]
fn the_transform_table_matches_the_published_crc32() {
    let mut encoded = Vec::new();
    for (prefix, elementary, suffix) in TRANSFORMS {
        encoded.extend_from_slice(prefix);
        encoded.push(0);
        encoded.push(elementary);
        encoded.extend_from_slice(suffix);
        encoded.push(0);
    }
    assert_eq!(encoded.len(), 648, "Appendix B's stated length");
    assert_eq!(crc32(&encoded), 0x3d96_5f81, "Appendix B's stated CRC-32");
}

/// **§7.1's three context lookups, against the CRC-32 table §7.1 prints.**
///
/// The context tables decide *which prefix code* reads the next literal. A
/// wrong entry does not corrupt one byte; it picks the wrong tree and the rest
/// of the meta-block decodes to noise, so this is worth pinning exactly.
#[test]
fn the_context_tables_match_their_published_crc32s() {
    for (name, table, want) in [
        ("Lut0", &super::LUT0, 0x8e91_efb7u32),
        ("Lut1", &super::LUT1, 0xd01a_32f4),
        ("Lut2", &super::LUT2, 0x0dd7_a0d6),
    ] {
        assert_eq!(table.len(), 256, "{name} is 256 bytes");
        assert_eq!(crc32(table.as_slice()), want, "{name}'s published CRC-32");
    }
}

// ---- the pieces the tables feed ---------------------------------------------

/// **§3.4's `ALPHABET_BITS` is the width that holds the largest symbol.**
///
/// Named here because the off-by-one is invisible: 256 needs eight bits and
/// 257 needs nine, and a decoder that computed `ceil(log2(size))` naively
/// reads one bit too many for every power of two — which is every literal
/// alphabet there is.
#[test]
fn alphabet_bits_is_the_width_of_the_largest_symbol() {
    assert_eq!(alphabet_bits(256), 8, "literals: symbols 0..=255");
    assert_eq!(alphabet_bits(704), 10, "insert-and-copy: symbols 0..=703");
    assert_eq!(alphabet_bits(26), 5, "block counts: symbols 0..=25");
    assert_eq!(alphabet_bits(2), 1);
    assert_eq!(alphabet_bits(1), 0, "one symbol needs no bits to name");
}

/// **§7.3's inverse move-to-front, on the example its own C code implies.**
#[test]
fn inverse_move_to_front_moves_the_named_value_to_the_front() {
    let mut values = vec![0u8, 1, 2, 0];
    inverse_move_to_front(&mut values);
    // 0 -> 0 (list unchanged), 1 -> 1 (list becomes 1,0,2,...),
    // 2 -> 2 (list becomes 2,1,0,...), 0 -> 2 (the front is now 2).
    assert_eq!(values, vec![0, 1, 2, 2]);
}

/// **§8's elementary transforms, one assertion each for the tricky three.**
///
/// `FermentFirst` and `FermentAll` are the only two that *modify* bytes, and
/// the C in §8 is written for UTF-8: an ASCII letter flips bit 5, a two-byte
/// lead flips bit 5 of the **following** byte, and a three-byte lead flips
/// bit 2 of the byte after that. A decoder that upper-cased ASCII and
/// stopped would pass every English test and mangle every other language.
#[test]
fn the_elementary_transforms_follow_section_8s_c_code() {
    // Transform 0 is Identity with no prefix or suffix.
    assert_eq!(apply_transform(b"word", 0), b"word");
    // Transform 9 is FermentFirst with no affixes: the first letter cases up.
    assert_eq!(apply_transform(b"word", 9), b"Word");
    // Transform 44 is FermentAll: every ASCII letter.
    assert_eq!(apply_transform(b"word", 44), b"WORD");
    // Transform 5 is Identity with the suffix " the ".
    assert_eq!(apply_transform(b"of", 5), b"of the ");
    // Transform 3 is OmitFirst1, 12 is OmitLast1.
    assert_eq!(apply_transform(b"word", 3), b"ord");
    assert_eq!(apply_transform(b"word", 12), b"wor");
    // Omitting more than there is leaves nothing, rather than underflowing.
    // 54 is OmitFirst9 and 64 is OmitLast9, which are the two the decoder can
    // be handed for a four-byte word — and an empty result is precisely the
    // case the command loop has to notice, since it produces no output.
    assert_eq!(apply_transform(b"ab", 54), b"", "OmitFirst9 of two bytes");
    assert_eq!(apply_transform(b"ab", 64), b"", "OmitLast9 of two bytes");
    // Two-byte UTF-8: FermentFirst flips bit 5 of the continuation byte, so
    // U+00E9 (0xC3 0xA9) becomes U+00C9 (0xC3 0x89).
    assert_eq!(apply_transform(b"\xC3\xA9x", 9), b"\xC3\x89x");
    // Three-byte UTF-8: bit 2 of the third byte.
    assert_eq!(apply_transform(b"\xE0\xA4\xAA", 9), b"\xE0\xA4\xAF");
}

/// **A prefix code that is not exactly full is refused, both ways.**
///
/// §3.5 states the rule as `sum(32768 >> length) == 32768`. Under-full leaves
/// bit patterns that decode to nothing, over-full gives two symbols the same
/// code; a decoder that accepted either would produce output from a stream no
/// encoder could have written.
#[test]
fn a_prefix_code_must_be_exactly_full() {
    // 1, 2, 2 is full.
    assert!(PrefixCode::from_lengths(&[1, 2, 2]).is_ok());
    // 1, 2 leaves a quarter of the space unreachable.
    assert!(PrefixCode::from_lengths(&[1, 2]).is_err(), "under-full");
    // 1, 1, 1 asks for three of two places.
    assert!(PrefixCode::from_lengths(&[1, 1, 1]).is_err(), "over-full");
    // No symbols at all is not a code.
    assert!(PrefixCode::from_lengths(&[0, 0]).is_err());
}

/// **The dictionary is reachable, and the words at its edges are words.**
///
/// A smoke test on the offset arithmetic rather than on the decoder: length 4
/// index 0 is the first word in the blob, and the RFC's own hexadecimal dump
/// opens with it.
#[test]
fn the_first_dictionary_words_are_where_the_offsets_say() {
    let four = DOFFSET[4] as usize;
    assert_eq!(
        &DICTIONARY[four..four + 4],
        b"time",
        "Appendix A's first word"
    );
    assert_eq!(&DICTIONARY[four + 4..four + 8], b"down");
    // The last length with words is 24, and its last word ends the blob.
    let last = DOFFSET[24] as usize + ((1usize << NDBITS[24]) - 1) * 24;
    assert_eq!(last + 24, DICTIONARY.len());
}

/// **The output ceiling is a refusal, not a truncation.**
///
/// A stream that decodes to more than the caller allowed is a stream the
/// caller cannot use: half a font program is not a font. This is the one
/// place this decoder deliberately differs from `inflate`, whose contract is
/// "corrupt input truncates and warns" because a truncated *image* is still
/// an image.
#[test]
fn the_output_ceiling_refuses_rather_than_truncating() {
    // 1024 identical bytes, from the committed reference vectors.
    let stream = include_bytes!("../../tests/brotli/repeated-byte-q11.br");
    assert_eq!(
        super::brotli_decode(stream, &Limits::new(1 << 20))
            .expect("it decodes with room")
            .len(),
        1024
    );
    assert_eq!(
        super::brotli_decode(stream, &Limits::new(16)),
        Err(super::BrotliError::ExceedsOutputLimit { limit: 16 })
    );
}

/// **Every prefix of every reference vector is a refusal or a short read,
/// never a panic.**
///
/// Ruling 1's standing property, run over the one corpus that is guaranteed
/// to be *nearly* valid at every length — which is where a decoder that
/// trusts a header it has already read falls over.
#[test]
fn every_truncation_of_a_real_stream_is_refused_rather_than_panicking() {
    for stream in [
        &include_bytes!("../../tests/brotli/english-prose-q11.br")[..],
        &include_bytes!("../../tests/brotli/sfnt-shaped-q11.br")[..],
        &include_bytes!("../../tests/brotli/utf8-text-q11.br")[..],
    ] {
        for cut in 0..stream.len() {
            let _ = super::brotli_decode(&stream[..cut], &Limits::new(1 << 16));
        }
    }
}
