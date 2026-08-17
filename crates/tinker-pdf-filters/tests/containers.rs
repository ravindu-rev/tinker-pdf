//! The two things a container reader needs from this crate that a PDF stream
//! filter does not: raw DEFLATE at its own door, and CRC-32.
//!
//! **Nothing consumes either of them yet.** `tinker-pdf-zip` is gap 29's
//! milestone 2 and the PNG decoder is its milestone 3; this file is milestone
//! 1, and the ordering is deliberate — building the ZIP reader on
//! `flate_decode` and repairing it afterwards would be one commit shipping a
//! known-wrong path. Gap 18a's M0 did the same thing for `mq.rs`.
//!
//! That makes these tests the only thing standing between a wrong answer and a
//! consumer inheriting it, so they are written as though the consumer existed:
//! `end` is asserted against the byte that follows a stream rather than
//! against a number, because what milestone 2 does with it is find a data
//! descriptor, and gap 18's milestone 6 found a plane count that was correct
//! only for inputs its own encoder could produce.
//!
//! Provenance: the two DEFLATE streams are the same genuine zlib output that
//! `vectors.rs` carries (level 0 for the stored block, `Z_FIXED` for the
//! other), restated here rather than shared so that this file can be read on
//! its own. `ZLIB_SNIFF_WITNESS` is constructed, and every byte of it is
//! explained where it is written down.

use tinker_pdf_filters::{crc32, flate_decode, inflate_raw, Crc32, Limits, Warning};

const CAP: Limits = Limits::new(1 << 20);

/// zlib level 0: one final stored block holding "Hello, hello, hello!".
/// Byte-aligned throughout, so its stream ends exactly on a byte boundary.
const DEFLATE_STORED: &[u8] = &[
    0x01, 0x14, 0x00, 0xeb, 0xff, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x2c, 0x20, 0x68, 0x65, 0x6c, 0x6c,
    0x6f, 0x2c, 0x20, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x21,
];

/// zlib `Z_FIXED`: the same text through fixed Huffman codes, so this one ends
/// part-way through its last byte. The two together are the only two shapes
/// `end` can have.
const DEFLATE_FIXED: &[u8] = &[
    0xf3, 0x48, 0xcd, 0xc9, 0xc9, 0xd7, 0x51, 0xc8, 0x40, 0xa2, 0x14, 0x01,
];

const TEXT: &[u8] = b"Hello, hello, hello!";

/// What APPNOTE 4.3.9 puts after an entry whose sizes were not known when it
/// was written: the optional signature, then the CRC and the two sizes.
///
/// It is here as a *suffix* rather than as a structure to parse. The claim
/// this file can make about it is only that `end` lands on its first byte;
/// reading it is milestone 2's job.
const DATA_DESCRIPTOR: &[u8] = &[
    b'P', b'K', 0x07, 0x08, // signature
    0x11, 0x22, 0x33, 0x44, // CRC-32
    0x0c, 0x00, 0x00, 0x00, // compressed size
    0x14, 0x00, 0x00, 0x00, // uncompressed size
];

/// A raw DEFLATE stream whose first two bytes are a structurally valid zlib
/// header, so that the two entry points can be shown to disagree about one
/// committed input.
///
/// It is constructible rather than accidental, and ZIP entry data is chosen by
/// whoever wrote the archive, so this is reachable. Byte by byte:
///
/// - `0x08` — CM 8 with CINFO 0 to a zlib reader. To a DEFLATE reader it is
///   BFINAL 0, BTYPE 00 (stored), and bit 3 set, which a stored block discards
///   as padding to the byte boundary. That set pad bit is the whole trick: it
///   is what lifts the low nibble to 8.
/// - `0x1D` — the low byte of the stored block's LEN, chosen from the eight
///   values that make `0x08nn` a multiple of 31. `0x081D` is 2 077 = 31 x 67,
///   so the header's checksum passes.
/// - `0x00 0xE2 0xFF` — LEN's high byte and NLEN, so LEN is 29 and NLEN is its
///   complement.
/// - `0x1D 0x00` — the first two payload bytes, which are *also* what the
///   zlib reader will take as its own NLEN two bytes later. They are chosen so
///   that its mis-read block is structurally valid, because a corrupt one
///   would produce nothing and `flate_decode` would then retry as raw and
///   recover. This is the "engineered to yield one byte" case exactly.
/// - 27 bytes of text, which are the rest of the true payload.
/// - `0x03 0x00` — a final fixed-Huffman block containing nothing but
///   end-of-block, which is the cheapest legal way to set BFINAL after a
///   header that could not.
const ZLIB_SNIFF_WITNESS: &[u8] = &[
    0x08, 0x1D, 0x00, 0xE2, 0xFF, 0x1D, 0x00, b'r', b'a', b'w', b' ', b'd', b'e', b'f', b'l', b'a',
    b't', b'e', b' ', b'n', b'o', b't', b' ', b'a', b' ', b'z', b'l', b'i', b'b', b' ', b'b', b'o',
    b'd', b'y', 0x03, 0x00,
];

/// What the witness really says.
const WITNESS_TRUTH: &[u8] = b"\x1d\x00raw deflate not a zlib body";

// ---------------------------------------------------------------------------
// `end`: the field with no consumer in this milestone
// ---------------------------------------------------------------------------

/// The assertion is `input[end..]`, not `end`, because that is the question
/// milestone 2 asks: a ZIP entry with general-purpose bit 3 set does not know
/// its own compressed size, so the data descriptor is found by inflating and
/// looking at what is left. An `end` one short hands back a stream's last byte
/// as the descriptor's first; one long eats the descriptor's.
#[test]
fn end_lands_on_the_first_byte_after_the_stream() {
    for (name, stream) in [("stored", DEFLATE_STORED), ("fixed", DEFLATE_FIXED)] {
        let mut input = stream.to_vec();
        input.extend_from_slice(DATA_DESCRIPTOR);

        let r = inflate_raw(&input, &CAP);
        assert_eq!(r.data, TEXT, "{name}");
        assert!(r.complete, "{name}: {:?}", r.warnings);
        assert!(r.warnings.is_empty(), "{name}: {:?}", r.warnings);

        assert_eq!(r.end, stream.len(), "{name}");
        assert_eq!(&input[r.end..], DATA_DESCRIPTOR, "{name}");
        assert_eq!(&input[..r.end], stream, "{name}");
    }
}

/// The same claim with no suffix at all, which is the other boundary: a stream
/// that is the whole input must report the whole input, and a stream ending
/// mid-byte must count the byte it ends in — DEFLATE finishes on a bit and
/// every container that carries it resumes on a byte.
#[test]
fn end_counts_the_byte_a_stream_ends_inside() {
    let r = inflate_raw(DEFLATE_FIXED, &CAP);
    assert!(r.complete);
    assert_eq!(r.end, DEFLATE_FIXED.len());

    // Not byte-aligned: the block ends part-way through the last byte, so a
    // count of *bits* would be short of 8 x 12.
    let r = inflate_raw(DEFLATE_STORED, &CAP);
    assert!(r.complete);
    assert_eq!(r.end, DEFLATE_STORED.len());
}

/// A suffix that begins with a plausible DEFLATE byte, so that an `end` which
/// overshot would not merely land on structure — it would land on structure
/// that reads as more stream. The descriptor's signature starts `PK`, and
/// `0x50` is BFINAL 0 with BTYPE 00.
#[test]
fn end_is_not_confused_by_a_suffix_that_looks_like_a_block() {
    let mut input = DEFLATE_FIXED.to_vec();
    input.push(0x50);
    input.extend_from_slice(&[0x4b, 0x07, 0x08]);
    let r = inflate_raw(&input, &CAP);
    assert!(r.complete);
    assert_eq!(r.data, TEXT);
    assert_eq!(&input[r.end..], b"PK\x07\x08");
}

/// Two blocks, so that `end` is proved to count to the end of the *final* one
/// rather than to the end of the first. The witness is the multi-block stream
/// this file already has to carry, so it does the work twice.
#[test]
fn end_counts_to_the_final_block_and_not_the_first() {
    let r = inflate_raw(ZLIB_SNIFF_WITNESS, &CAP);
    assert!(r.complete);
    assert_eq!(r.end, ZLIB_SNIFF_WITNESS.len());
    // The first block ends at 34; the final one is the two bytes after it.
    assert!(r.end > 34);
}

// ---------------------------------------------------------------------------
// The ceiling, proved by its own refusal rather than by a clock
// ---------------------------------------------------------------------------

/// Ruling 1. `inflate_raw` is reached with attacker-chosen bytes, and a small
/// input that expands without bound is what a ZIP entry is for.
///
/// The assertion is the warning and the output length, never elapsed time —
/// `5adf502`'s method, taken for its stated reason: a timing assertion fails
/// on a slow machine and passes on a fast one with the budget removed.
#[test]
fn the_output_ceiling_refuses_a_bomb_by_its_own_warning() {
    // A megabyte of zeroes. The expansion ratio is the point; this crate's own
    // encoder is only the cheapest way to get one, and nothing here depends on
    // what it chose to emit.
    let bomb = tinker_pdf_filters::deflate(&vec![0u8; 1 << 20]);
    assert!(
        bomb.len() * 16 < (1 << 20),
        "a stream that expands at least sixteenfold, from {} bytes",
        bomb.len()
    );

    let r = inflate_raw(&bomb, &Limits::new(4096));
    assert_eq!(r.data.len(), 4096);
    assert!(r.capped);
    assert!(!r.complete, "capped implies not complete");
    assert_eq!(r.warnings, vec![Warning::OutputCapHit]);
}

/// `end` after a capped decode records where this decoder gave up, not where
/// the stream ends, and the documentation says so. The contract a consumer can
/// rely on is the implication asserted here: read `end` only when `complete`.
#[test]
fn a_capped_decode_says_so_rather_than_reporting_a_stream_end() {
    let mut input = DEFLATE_STORED.to_vec();
    input.extend_from_slice(DATA_DESCRIPTOR);
    let r = inflate_raw(&input, &Limits::new(4));
    assert_eq!(r.data, b"Hell");
    assert!(r.capped);
    assert!(!r.complete);
    assert_eq!(r.warnings, vec![Warning::OutputCapHit]);
    assert_ne!(
        r.end,
        DEFLATE_STORED.len(),
        "a capped end is not a stream end, and a consumer that used it here \
         would find the descriptor in the wrong place"
    );
}

/// A ZIP entry can be empty, damaged or cut off by the archive ending, and all
/// three arrive here as bytes. None of them may panic and none may claim
/// completeness (ruling 1).
#[test]
fn nothing_incomplete_claims_otherwise() {
    let empty = inflate_raw(b"", &CAP);
    assert!(empty.data.is_empty());
    assert!(!empty.complete);
    assert!(!empty.capped);
    assert_eq!(empty.warnings, vec![Warning::TruncatedInput]);

    let truncated = inflate_raw(&DEFLATE_STORED[..8], &CAP);
    assert!(!truncated.complete);
    assert_eq!(truncated.warnings, vec![Warning::TruncatedInput]);

    // BTYPE 11 is reserved.
    let corrupt = inflate_raw(&[0b111], &CAP);
    assert!(!corrupt.complete);
    assert_eq!(corrupt.warnings, vec![Warning::CorruptDeflateStream]);
}

// ---------------------------------------------------------------------------
// The three costs `flate_decode` would impose on container data
// ---------------------------------------------------------------------------

/// Cost one, and the certain one. A ZIP entry stored with method 8 is raw
/// DEFLATE by definition, so the sniffing path calls the normal case a
/// fallback and says so once per entry. Ruling 10 exists so that "it opened"
/// and "it opened cleanly" stay distinguishable; a warning on every entry of
/// every archive makes that distinction worthless for the whole format.
#[test]
fn the_sniffing_door_manufactures_a_fallback_warning_on_every_entry() {
    let through_the_filter = flate_decode(DEFLATE_FIXED, &CAP, None).expect("no predictor");
    assert_eq!(through_the_filter.data, TEXT);
    assert_eq!(
        through_the_filter.warnings,
        vec![Warning::RawDeflateFallback],
    );

    let through_the_seam = inflate_raw(DEFLATE_FIXED, &CAP);
    assert_eq!(through_the_seam.data, TEXT);
    assert!(
        through_the_seam.warnings.is_empty(),
        "{:?}",
        through_the_seam.warnings
    );
}

/// Cost two. What follows an entry whose size was streamed is its data
/// descriptor and then the next local header: structure, not garbage. The
/// sniffing door compares the stream's end against the whole input it was
/// handed and reports the difference, which for a container is every time.
#[test]
fn the_sniffing_door_manufactures_trailing_garbage_on_a_streamed_entry() {
    let mut input = DEFLATE_FIXED.to_vec();
    input.extend_from_slice(DATA_DESCRIPTOR);

    let through_the_filter = flate_decode(&input, &CAP, None).expect("no predictor");
    assert_eq!(through_the_filter.data, TEXT);
    assert_eq!(
        through_the_filter.warnings,
        vec![Warning::RawDeflateFallback, Warning::TrailingGarbage],
    );

    let through_the_seam = inflate_raw(&input, &CAP);
    assert_eq!(through_the_seam.data, TEXT);
    assert!(through_the_seam.warnings.is_empty());
    // And it says where the descriptor starts instead of complaining about it.
    assert_eq!(&input[through_the_seam.end..], DATA_DESCRIPTOR);
}

/// Cost three, the rare one: the two doors decode one committed input to two
/// different answers.
///
/// This is narrower than "the sniff is unsafe" and it is worth stating
/// precisely. The stream has to open with a non-final stored block whose
/// discarded pad bit is set, which is what puts an 8 in the low nibble; the
/// second byte has to complete the modulo-31 check; and the mis-read must
/// yield at least one byte, because `flate_decode` retries as raw only when
/// the wrapped read produced nothing at all. All three are properties of
/// attacker-chosen entry data.
///
/// The sharpest part is the warnings. The sniffing door does not report that
/// it took the zlib branch — `RawDeflateFallback` is pushed on the *other*
/// path — so its only warning is `TruncatedInput`, about a stream that is not
/// truncated. Nothing in its answer says the bytes were read as something they
/// are not.
#[test]
fn one_input_decodes_two_ways_and_only_the_raw_door_is_right() {
    let through_the_seam = inflate_raw(ZLIB_SNIFF_WITNESS, &CAP);
    assert_eq!(through_the_seam.data, WITNESS_TRUTH);
    assert!(through_the_seam.complete);
    assert!(through_the_seam.warnings.is_empty());
    assert_eq!(through_the_seam.end, ZLIB_SNIFF_WITNESS.len());

    let through_the_filter = flate_decode(ZLIB_SNIFF_WITNESS, &CAP, None).expect("no predictor");
    assert_ne!(through_the_filter.data, WITNESS_TRUTH);
    assert_eq!(
        through_the_filter.data, b"raw deflate not a zlib body\x03\x00",
        "the payload read from two bytes further in, which is the same length \
         and looks just as much like text"
    );
    assert!(!through_the_filter.complete);
    assert_eq!(
        through_the_filter.warnings,
        vec![Warning::TruncatedInput],
        "and not one warning says the wrong door was taken"
    );
}

/// The seam adds a door; it does not move the one that was there. Every stream
/// `flate_decode` already handled reaches it unchanged, including the zlib
/// wrapper the raw door deliberately cannot read.
#[test]
fn the_filter_door_still_sniffs_and_the_raw_door_never_does() {
    // Genuine zlib output: header, the fixed-Huffman body, Adler-32.
    let mut wrapped = vec![0x78, 0x01];
    wrapped.extend_from_slice(DEFLATE_FIXED);
    wrapped.extend_from_slice(&[0x48, 0x9e, 0x06, 0xd6]);

    let filtered = flate_decode(&wrapped, &CAP, None).expect("no predictor");
    assert_eq!(filtered.data, TEXT);
    assert!(filtered.complete);
    assert!(filtered.warnings.is_empty(), "{:?}", filtered.warnings);

    // The raw door reads 0x78 as BFINAL 0, BTYPE 00 and then a stored length
    // that is not its own complement. It does not recover, and it must not
    // try: a caller here said it holds raw DEFLATE.
    let raw = inflate_raw(&wrapped, &CAP);
    assert!(!raw.complete);
    assert_ne!(raw.data, TEXT);
}

// ---------------------------------------------------------------------------
// CRC-32 across the crate boundary
// ---------------------------------------------------------------------------

/// The published values again from outside the crate, which is the only thing
/// that proves they are exported at all. `crc32.rs` carries the argument and
/// the cross-check against a second formulation.
#[test]
fn the_published_crc_values_survive_the_crate_boundary() {
    assert_eq!(crc32(b""), 0);
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b"IEND"), 0xAE42_6082);
}

/// The shape milestone 2 will use: a decode and its checksum in one pass, with
/// the CRC continued across the chunks the inflater hands back rather than
/// recomputed over a reassembled buffer.
#[test]
fn a_crc_can_be_carried_across_the_pieces_a_reader_gets_them_in() {
    let r = inflate_raw(DEFLATE_FIXED, &CAP);
    assert!(r.complete);

    let mut running = Crc32::new();
    for piece in r.data.chunks(7) {
        running.update(piece);
    }
    assert_eq!(running.finish(), crc32(TEXT));
    assert_eq!(running.finish(), 0xE42E_EBAC, "CRC-32 of the decoded text");
}
