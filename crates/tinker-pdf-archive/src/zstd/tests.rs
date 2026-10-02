//! The decoder held to the published tables, the zstd project's own golden
//! files, libzstd's frames over this repository's files, and frames built
//! here byte by byte where no encoder would write the case.

use super::fse::{self, Fault, Row};
use super::*;

fn golden(path: &str) -> Vec<u8> {
    let at = format!("{}/data/zstd-golden/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&at).unwrap_or_else(|e| panic!("{at}: {e}"))
}

fn coders(name: &str) -> Vec<u8> {
    let at = format!("{}/tests/coders/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&at).unwrap_or_else(|e| panic!("{at}: {e}"))
}

const ROOMY: Limits = Limits {
    max_unpacked: 1 << 24,
};

// ---------------------------------------------------------------------------
// The published tables
// ---------------------------------------------------------------------------

/// RFC 8878 Appendix A (and the format document's, the same three tables):
/// every state of the predefined decoding tables, as (symbol, number of
/// bits, base).
#[rustfmt::skip]
const APPENDIX_A_LITERALS_LENGTH: [(u8, u8, u16); 64] = [
    (0, 4, 0), (0, 4, 16), (1, 5, 32), (3, 5, 0), (4, 5, 0), (6, 5, 0), (7, 5, 0), (9, 5, 0),
    (10, 5, 0), (12, 5, 0), (14, 6, 0), (16, 5, 0), (18, 5, 0), (19, 5, 0), (21, 5, 0), (22, 5, 0),
    (24, 5, 0), (25, 5, 32), (26, 5, 0), (27, 6, 0), (29, 6, 0), (31, 6, 0), (0, 4, 32), (1, 4, 0),
    (2, 5, 0), (4, 5, 32), (5, 5, 0), (7, 5, 32), (8, 5, 0), (10, 5, 32), (11, 5, 0), (13, 6, 0),
    (16, 5, 32), (17, 5, 0), (19, 5, 32), (20, 5, 0), (22, 5, 32), (23, 5, 0), (25, 4, 0), (25, 4, 16),
    (26, 5, 32), (28, 6, 0), (30, 6, 0), (0, 4, 48), (1, 4, 16), (2, 5, 32), (3, 5, 32), (5, 5, 32),
    (6, 5, 32), (8, 5, 32), (9, 5, 32), (11, 5, 32), (12, 5, 32), (15, 6, 0), (17, 5, 32), (18, 5, 32),
    (20, 5, 32), (21, 5, 32), (23, 5, 32), (24, 5, 32), (35, 6, 0), (34, 6, 0), (33, 6, 0), (32, 6, 0),
];

#[rustfmt::skip]
const APPENDIX_A_MATCH_LENGTH: [(u8, u8, u16); 64] = [
    (0, 6, 0), (1, 4, 0), (2, 5, 32), (3, 5, 0), (5, 5, 0), (6, 5, 0), (8, 5, 0), (10, 6, 0),
    (13, 6, 0), (16, 6, 0), (19, 6, 0), (22, 6, 0), (25, 6, 0), (28, 6, 0), (31, 6, 0), (33, 6, 0),
    (35, 6, 0), (37, 6, 0), (39, 6, 0), (41, 6, 0), (43, 6, 0), (45, 6, 0), (1, 4, 16), (2, 4, 0),
    (3, 5, 32), (4, 5, 0), (6, 5, 32), (7, 5, 0), (9, 6, 0), (12, 6, 0), (15, 6, 0), (18, 6, 0),
    (21, 6, 0), (24, 6, 0), (27, 6, 0), (30, 6, 0), (32, 6, 0), (34, 6, 0), (36, 6, 0), (38, 6, 0),
    (40, 6, 0), (42, 6, 0), (44, 6, 0), (1, 4, 32), (1, 4, 48), (2, 4, 16), (4, 5, 32), (5, 5, 32),
    (7, 5, 32), (8, 5, 32), (11, 6, 0), (14, 6, 0), (17, 6, 0), (20, 6, 0), (23, 6, 0), (26, 6, 0),
    (29, 6, 0), (52, 6, 0), (51, 6, 0), (50, 6, 0), (49, 6, 0), (48, 6, 0), (47, 6, 0), (46, 6, 0),
];

#[rustfmt::skip]
const APPENDIX_A_OFFSET: [(u8, u8, u16); 32] = [
    (0, 5, 0), (6, 4, 0), (9, 5, 0), (15, 5, 0), (21, 5, 0), (3, 5, 0), (7, 4, 0), (12, 5, 0),
    (18, 5, 0), (23, 5, 0), (5, 5, 0), (8, 4, 0), (14, 5, 0), (20, 5, 0), (2, 5, 0), (7, 4, 16),
    (11, 5, 0), (17, 5, 0), (22, 5, 0), (4, 5, 0), (8, 4, 16), (13, 5, 0), (19, 5, 0), (1, 5, 0),
    (6, 4, 16), (10, 5, 0), (16, 5, 0), (28, 5, 0), (27, 5, 0), (26, 5, 0), (25, 5, 0), (24, 5, 0),
];

fn rows(table: &[(u8, u8, u16)]) -> Vec<Row> {
    table
        .iter()
        .map(|&(symbol, bits, base)| Row { symbol, bits, base })
        .collect()
}

#[test]
fn the_predefined_tables_are_appendix_a_state_by_state() {
    for (kind, log, expected) in [
        (Kind::LiteralsLength, 6, &APPENDIX_A_LITERALS_LENGTH[..]),
        (Kind::MatchLength, 6, &APPENDIX_A_MATCH_LENGTH[..]),
        (Kind::Offset, 5, &APPENDIX_A_OFFSET[..]),
    ] {
        let table = kind.predefined().expect("a predefined distribution builds");
        assert_eq!(table.log, log);
        assert_eq!(table.rows, rows(expected));
    }
}

#[test]
fn a_table_description_is_held_to_its_context() {
    // Accuracy log 5 and then zeros: every value reads as "less than one",
    // each taking one point, and 32 of them spend the table exactly.
    let (probs, log, _) = fse::read_distribution(&[0; 32], 35, 9).expect("32 symbols fit 35");
    assert_eq!((probs.len(), log), (32, 5));
    assert!(probs.iter().all(|&p| p == -1));
    // "Less than one" symbols take one row each from the top, in natural
    // order, and a full state reset.
    let table = fse::build(&probs, log).expect("it builds");
    assert_eq!(
        table.row(31),
        Row {
            symbol: 0,
            bits: 5,
            base: 0
        }
    );
    assert_eq!(
        table.row(0),
        Row {
            symbol: 31,
            bits: 5,
            base: 0
        }
    );

    // The same bits where the alphabet ends at 8: the points run out past
    // the last symbol the context has, which is corruption.
    assert_eq!(fse::read_distribution(&[0; 32], 8, 9), Err(Fault::Corrupt));
    // An accuracy log past the context's maximum: 5 + 10 against 9.
    assert_eq!(
        fse::read_distribution(&[0x0A; 8], 35, 9),
        Err(Fault::Corrupt)
    );
    // A description longer than the bytes it was given.
    assert_eq!(
        fse::read_distribution(&[0; 4], 35, 9),
        Err(Fault::Truncated)
    );
}

#[test]
fn repeat_offsets_follow_the_format_document_s_worked_table() {
    // `doc/zstd_compression_format.md`, "Offset updates rules": each row is
    // (Offset_Value, literals_length) and the three repeat offsets after it.
    let mut repeats = [1, 4, 8];
    for (value, literals, after) in [
        (1114, 11, [1111, 1, 4]),
        (1, 22, [1111, 1, 4]),
        (2225, 22, [2222, 1111, 1]),
        (1114, 111, [1111, 2222, 1111]),
        (3336, 33, [3333, 1111, 2222]),
        (2, 22, [1111, 3333, 2222]),
        (3, 33, [2222, 1111, 3333]),
        (3, 0, [2221, 2222, 1111]),
        (1, 0, [2222, 2221, 1111]),
    ] {
        let (offset, _) = resolve_offset(value, literals, &mut repeats).expect("an offset");
        assert_eq!(
            repeats, after,
            "after Offset_Value {value}, literals {literals}"
        );
        assert_eq!(offset, after[0]);
    }
    // "If Repeated_Offset1 - 1 evaluates to 0, then the data is considered
    // corrupted."
    let mut repeats = [1, 4, 8];
    assert_eq!(resolve_offset(3, 0, &mut repeats), Err(Error::BadOffset));
}

// ---------------------------------------------------------------------------
// The zstd project's golden files
// ---------------------------------------------------------------------------

#[test]
fn the_golden_files_decode_to_what_zstd_s_own_tests_say() {
    // `tests/playTests.sh`: "the following test verifies that the decoder is
    // compatible with RLE as first block" -- compared there against 1 MiB of
    // `/dev/zero`.
    let rle = decode(&golden("golden-decompression/rle-first-block.zst"), &ROOMY);
    assert_eq!(rle, Ok(vec![0u8; 1 << 20]));
    // Compared there against an empty file.
    let empty = decode(&golden("golden-decompression/empty-block.zst"), &ROOMY);
    assert_eq!(empty, Ok(Vec::new()));
    // The other two are `zstd -t`: they decode. What they decode to is in
    // the files themselves, as raw literals: "Hello World!\n" behind a
    // sequence count of zero in its two-byte form, and 131 068 zeros in a
    // compressed block exactly 128 KiB long -- `Block_Maximum_Size` itself,
    // which RFC 8878 allows ("no larger than") and no encoder needs.
    let zero_seq = golden("golden-decompression/zeroSeq_2B.zst");
    assert_eq!(&zero_seq[10..23], b"Hello World!\n");
    assert_eq!(decode(&zero_seq, &ROOMY), Ok(zero_seq[10..23].to_vec()));
    let block = golden("golden-decompression/block-128k.zst");
    assert_eq!(little_endian(&block, 6, 3), Some((128 * 1024) << 3 | 0b101));
    assert_eq!(decode(&block, &ROOMY), Ok(block[12..12 + 131_068].to_vec()));
}

#[test]
fn the_golden_error_files_are_refused_for_their_reasons() {
    // `tests/cli-tests/decompression/detectErrors.sh`: every file here must
    // fail `zstd -t`. Each is refused, and for the fault it was built for.
    for (name, why) in [
        // Two bytes after a sequence count of zero.
        ("zeroSeq_extraneous.zst", Error::BadSequences),
        // The first repeat offset minus one, when the first is one.
        ("off0.bin.zst", Error::BadOffset),
        // A Huffman weight stream too short for its two initial states.
        ("truncated_huff_state.zst", Error::BadLiterals),
    ] {
        let input = golden(&format!("golden-decompression-errors/{name}"));
        assert_eq!(decode(&input, &ROOMY), Err(why), "{name}");
    }
}

// ---------------------------------------------------------------------------
// libzstd's frames (`tests/coders/make-zstd.py`)
// ---------------------------------------------------------------------------

fn prose() -> Vec<u8> {
    coders("input/prose.txt")
}

#[test]
fn every_xxh64_path_matches_the_checksum_libzstd_wrote() {
    // Eighteen frames of prose's first n bytes, n either side of every
    // length the hash treats differently, each with its checksum on: a
    // decode that returns is eighteen checksums matched.
    let lengths = [
        0, 1, 3, 4, 5, 7, 8, 9, 15, 16, 31, 32, 33, 63, 64, 65, 100, 1000,
    ];
    let mut census = Census::default();
    let got = decode_counting(&coders("zstd-checksums.zst"), &ROOMY, &mut census);
    let prose = prose();
    let want: Vec<u8> = lengths.iter().flat_map(|&n| prose[..n].to_vec()).collect();
    assert_eq!(got, Ok(want));
    assert_eq!((census.frames, census.checksums), (18, 18));
}

#[test]
fn a_frame_that_names_a_dictionary_is_refused_by_name() {
    assert_eq!(
        decode(&coders("zstd-dictionary.zst"), &ROOMY),
        Err(Error::NeedsDictionary)
    );
    // One that needs a dictionary and cannot say so reaches back before its
    // first byte.
    assert_eq!(
        decode(&coders("zstd-raw-dictionary.zst"), &ROOMY),
        Err(Error::BadOffset)
    );
}

/// The fixtures together reach every coding RFC 8878 gives a block, which
/// is what makes "they decode to their inputs" a claim about the whole
/// decoder rather than its common path. A census of what the decoder met,
/// so a regenerated fixture that stopped reaching a path fails here.
#[test]
fn the_fixtures_reach_every_part_of_the_format() {
    let mut census = Census::default();
    for name in [
        "zstd-prose-l3.zst",
        "zstd-prose-l19.zst",
        "zstd-x86-l22.zst",
        "zstd-runs-l1.zst",
        "zstd-prose-w10.zst",
        "zstd-frames.zst",
        "zstd-modes.zst",
        "zstd-treeless.zst",
        "zstd-empty.zst",
        "zstd-checksums.zst",
    ] {
        decode_counting(&coders(name), &ROOMY, &mut census)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    let Census {
        frames,
        skippable_frames,
        checksums,
        single_segment,
        blocks,
        literals,
        four_streams,
        weights,
        modes,
        sequences,
        repeats,
        overlaps,
        carried_repeats,
        treeless_after_plain,
    } = census;
    assert!(
        frames > single_segment && single_segment > 0,
        "both window forms"
    );
    assert!(
        frames > checksums && checksums > 0,
        "frames with and without"
    );
    assert!(skippable_frames > 0);
    assert!(
        blocks.iter().all(|&n| n > 0),
        "raw, RLE, compressed: {blocks:?}"
    );
    assert!(
        literals.iter().all(|&n| n > 0),
        "raw, RLE, compressed, treeless literals: {literals:?}"
    );
    assert!(
        literals[2] + literals[3] > four_streams && four_streams > 0,
        "1 and 4 streams"
    );
    assert!(
        weights.iter().all(|&n| n > 0),
        "FSE and direct weights: {weights:?}"
    );
    for (kind, modes) in ["literals length", "offset", "match length"]
        .iter()
        .zip(modes)
    {
        assert!(
            modes.iter().all(|&n| n > 0),
            "{kind}: predefined, RLE, FSE, repeat: {modes:?}"
        );
    }
    assert!(sequences > 60_000);
    assert!(
        repeats.iter().all(|&n| n > 0),
        "all four repeats: {repeats:?}"
    );
    assert!(overlaps > 0, "matches over their own output");
    // What one block leaves the next: a repeat offset used before the block
    // has decoded one of its own, and a tree reused across a block whose
    // literals were raw.
    assert!(carried_repeats > 0, "a repeat offset from an earlier block");
    assert!(treeless_after_plain > 0, "a tree kept across a raw section");
}

#[test]
fn two_initial_weight_states_the_stream_does_not_hold_are_refused() {
    // An FSE description of Huffman weights, accuracy 5, built by hand:
    // weight 0 has no probability and weights 1 and 2 sixteen points each,
    // so state 0 is a weight-1 state.
    let description = [0x10, 0x88, 0x1F];
    // Ten bits of stream, both states 0: weights 1 and 1, the last symbol's
    // completing them with 2. A code.
    let whole = [&[0x05][..], &description, &[0x00, 0x04]].concat();
    assert!(super::huffman::read_tree(&whole).is_ok());
    // Only the end mark. Zeros read in place of the two states would make
    // the same weights and the same valid code; they are refused instead.
    let truncated = [&[0x04][..], &description, &[0x01]].concat();
    assert_eq!(
        super::huffman::read_tree(&truncated).err(),
        Some(Fault::Corrupt)
    );
}

#[test]
fn the_ceiling_is_exact() {
    let prose = prose();
    // A frame that declares its size, exactly at the ceiling and one under.
    let l3 = coders("zstd-prose-l3.zst");
    let exact = Limits {
        max_unpacked: prose.len(),
    };
    assert_eq!(decode(&l3, &exact).as_deref(), Ok(&prose[..]));
    let under = Limits {
        max_unpacked: prose.len() - 1,
    };
    assert_eq!(decode(&l3, &under), Err(Error::TooLarge));
    // A streamed frame, which declares nothing: the ceiling is met midway.
    let frames = coders("zstd-frames.zst");
    let total = decode(&frames, &ROOMY).expect("it decodes").len();
    let under = Limits {
        max_unpacked: total - 1,
    };
    assert_eq!(decode(&frames, &under), Err(Error::TooLarge));
    let early = Limits { max_unpacked: 1000 };
    assert_eq!(decode(&frames, &early), Err(Error::TooLarge));
}

// ---------------------------------------------------------------------------
// Frames built here, for what no encoder writes
// ---------------------------------------------------------------------------

const MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

fn block(last: bool, kind: u32, size: u32, content: &[u8]) -> Vec<u8> {
    let header = (size << 3) | (kind << 1) | u32::from(last);
    let mut out = header.to_le_bytes()[..3].to_vec();
    out.extend_from_slice(content);
    out
}

/// Two raw kilobytes, then one sequence: no literals, three bytes from
/// `offset` back, all three tables in RLE mode. The bitstream is the offset
/// code's extra bits and the end mark above them — which, for an offset
/// code, is `Offset_Value` itself.
fn one_match(window_descriptor: u8, offset: u32) -> (Vec<u8>, Vec<u8>) {
    let first: Vec<u8> = (0..1024u32).map(|i| i as u8).collect();
    let second: Vec<u8> = (0..1024u32).map(|i| (i * 7 + 3) as u8).collect();
    let value = offset + 3;
    let code = 31 - value.leading_zeros();
    let stream = value.to_le_bytes();
    let used = (code as usize + 1).div_ceil(8);
    let mut sequences = vec![0x00, 0x01, 0b0101_0100, 0x00, code as u8, 0x00];
    sequences.extend_from_slice(&stream[..used]);

    let mut frame = MAGIC.to_vec();
    frame.extend_from_slice(&[0x00, window_descriptor]);
    frame.extend(block(false, 0, 1024, &first));
    frame.extend(block(false, 0, 1024, &second));
    frame.extend(block(true, 2, sequences.len() as u32, &sequences));

    let mut want = [first, second].concat();
    // What the match copies, where the offset is inside the output at all.
    if let Some(from) = want.len().checked_sub(offset as usize) {
        let copy = want[from..from + 3].to_vec();
        want.extend(copy);
    }
    (frame, want)
}

#[test]
fn a_match_is_held_to_its_window_and_to_the_frame_s_output() {
    // Window 1 KiB (descriptor 0): 1 000 back is inside it...
    let (frame, want) = one_match(0x00, 1000);
    assert_eq!(decode(&frame, &ROOMY), Ok(want));
    // ...and 1 024 back is its edge, which a match may reach...
    let (frame, want) = one_match(0x00, 1024);
    assert_eq!(decode(&frame, &ROOMY), Ok(want));
    // ...and 1 025 back is past it, though the output reaches that far.
    let (frame, _) = one_match(0x00, 1025);
    assert_eq!(decode(&frame, &ROOMY), Err(Error::BadOffset));
    // Window 4 KiB (descriptor 0x10): 2 048 back is the frame's first byte,
    // and 2 049 is before it.
    let (frame, want) = one_match(0x10, 2048);
    assert_eq!(decode(&frame, &ROOMY), Ok(want));
    let (frame, _) = one_match(0x10, 2049);
    assert_eq!(decode(&frame, &ROOMY), Err(Error::BadOffset));
}

#[test]
fn a_frame_header_is_held_to_the_format() {
    let raw = |descriptor: &[u8], size: u32| {
        let mut f = MAGIC.to_vec();
        f.extend_from_slice(descriptor);
        f.extend(block(true, 0, size, &vec![b'x'; size as usize]));
        f
    };
    // Window 1 KiB: a 1 KiB raw block is the most a block may be.
    assert_eq!(
        decode(&raw(&[0x00, 0x00], 1024), &ROOMY),
        Ok(vec![b'x'; 1024])
    );
    assert_eq!(
        decode(&raw(&[0x00, 0x00], 1025), &ROOMY),
        Err(Error::BadBlock)
    );
    // The reserved bit.
    assert_eq!(
        decode(&raw(&[0x08, 0x00], 4), &ROOMY),
        Err(Error::BadFrameHeader)
    );
    // A one-byte dictionary ID of zero means none; of seven, one is needed.
    assert_eq!(decode(&raw(&[0x01, 0x00, 0], 4), &ROOMY), Ok(vec![b'x'; 4]));
    assert_eq!(
        decode(&raw(&[0x01, 0x00, 7], 4), &ROOMY),
        Err(Error::NeedsDictionary)
    );
    // Single segment, content size 5 in one byte: the window is 5 too.
    assert_eq!(decode(&raw(&[0x20, 5], 5), &ROOMY), Ok(vec![b'x'; 5]));
    assert_eq!(
        decode(&raw(&[0x20, 5], 4), &ROOMY),
        Err(Error::ContentSizeMismatch)
    );
    assert_eq!(decode(&raw(&[0x20, 5], 6), &ROOMY), Err(Error::BadBlock));
    // The two-byte content size starts at 256: 0x0000 is 256.
    assert_eq!(
        decode(&raw(&[0x40, 0x00, 0x00, 0x00], 256), &ROOMY),
        Ok(vec![b'x'; 256])
    );
    // A block of the reserved type.
    let mut reserved = MAGIC.to_vec();
    reserved.extend_from_slice(&[0x00, 0x00]);
    reserved.extend(block(true, 3, 0, &[]));
    assert_eq!(decode(&reserved, &ROOMY), Err(Error::BadBlock));
}

#[test]
fn what_is_not_a_frame_is_refused_and_a_skippable_one_skipped() {
    let empty = coders("zstd-empty.zst");
    assert_eq!(decode(&[], &ROOMY), Err(Error::NotZstd));
    assert_eq!(decode(b"PK\x03\x04", &ROOMY), Err(Error::NotZstd));
    // Legacy v0.7's magic, `0xFD2FB527`.
    assert_eq!(
        decode(&[0x27, 0xB5, 0x2F, 0xFD, 0, 0], &ROOMY),
        Err(Error::NotZstd)
    );
    // Bytes after a complete frame that start no frame.
    assert_eq!(
        decode(&[&empty[..], b"junk"].concat(), &ROOMY),
        Err(Error::NotZstd)
    );
    assert_eq!(
        decode(&[&empty[..], &MAGIC[..3]].concat(), &ROOMY),
        Err(Error::NotZstd)
    );
    // A skippable frame of any of its sixteen magics is skipped, alone or
    // not, and one that claims more than there is ends early.
    for nibble in [0x0u8, 0x7, 0xF] {
        let skip = [&[0x50 | nibble, 0x2A, 0x4D, 0x18, 3, 0, 0, 0][..], b"abc"].concat();
        assert_eq!(decode(&skip, &ROOMY), Ok(Vec::new()));
        assert_eq!(
            decode(&[&skip[..], &empty[..]].concat(), &ROOMY),
            Ok(Vec::new())
        );
        assert_eq!(decode(&skip[..10], &ROOMY), Err(Error::Truncated));
    }
    // A frame cut anywhere is refused, never a panic; cut in its header or
    // blocks it is truncated.
    let l3 = coders("zstd-prose-l3.zst");
    for cut in [5, 6, 9, 100, l3.len() - 5] {
        assert_eq!(
            decode(&l3[..cut], &ROOMY),
            Err(Error::Truncated),
            "cut at {cut}"
        );
    }
    // The checksum is checked: one bit of it changed.
    let mut damaged = empty.clone();
    *damaged.last_mut().expect("a checksum") ^= 1;
    assert_eq!(decode(&damaged, &ROOMY), Err(Error::ChecksumMismatch));
}
