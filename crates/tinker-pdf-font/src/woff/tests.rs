//! The parts of both containers that no committed file reaches.
//!
//! `tests/woff_fixtures.rs` holds the decoders to seven files two real
//! encoders wrote, which is what says they read a WOFF. This file is the other
//! half: the variable-width codings, the refusals, and the shapes a
//! general-purpose encoder never emits — because a corpus of well-formed files
//! exercises none of them, and every one is a branch a hostile input reaches
//! first.
//!
//! # Counted injections
//!
//! **Twenty-two assertions fire here, and none of them is zero.** Each names a
//! rule one of the two specifications states, reintroduces the input that
//! breaks it, and asserts the refusal by variant rather than by "is an error":
//! [`the_255uint16_coding_is_not_unique`] (3),
//! [`uintbase128_refuses_both_spellings_the_spec_forbids`] (4),
//! [`a_woff1_header_is_checked_before_it_is_believed`] (5),
//! [`a_woff2_header_is_checked_before_it_is_believed`] (4),
//! [`a_transform_this_build_cannot_reverse_is_named`] (3),
//! [`the_output_ceiling_is_not_advisory`] (3).
//!
//! The one that is *not* an injection is
//! [`the_two_fields_a_woff2_reads_and_ignores`], which asserts a
//! **non**-refusal: §3.2 says in as many words that a decoder "MUST NOT reject
//! a downloaded font file if the reserved header value is not zero", where
//! WOFF 1.0 §3 says the opposite about its own. Two containers, two rules,
//! and a build that applied either to both would be wrong about half the web.

use super::*;
use crate::Sfnt;

/// A ceiling far above anything built here.
const ROOMY: usize = 1 << 20;

// ---- §3.1's two variable-width codings --------------------------------------

/// A reader over bytes, for the coding tests.
fn reader(bytes: &[u8]) -> Reader<'_> {
    Reader::new(bytes)
}

/// §3.1 gives 506 three spellings and says "a decoder MUST accept them all".
///
/// The non-uniqueness is the point. An encoder picks one; a decoder that
/// implemented only the short form reads a file from one producer and not
/// another, and the failure looks like a corrupt font rather than a missing
/// branch.
#[test]
fn the_255uint16_coding_is_not_unique() {
    let mut caught = 0;
    // §3.1's own example, verbatim: "the value 506 can be encoded as
    // [255, 253], [254, 0], and [253, 1, 250]".
    for spelling in [vec![255u8, 253], vec![254, 0], vec![253, 1, 250]] {
        assert_eq!(
            reader(&spelling).u255().expect("a 255UInt16 decodes"),
            506,
            "{spelling:?}"
        );
        caught += 1;
    }
    assert_eq!(caught, 3, "three spellings of one number");

    // The boundaries either side of the one-byte form.
    assert_eq!(reader(&[0]).u255().unwrap(), 0);
    assert_eq!(reader(&[252]).u255().unwrap(), 252);
    // 253 is the word escape, so a bare 253 is two more bytes.
    assert_eq!(reader(&[253, 0xFF, 0xFF]).u255().unwrap(), 0xFFFF);
    // And a truncated one is short, not a panic.
    assert_eq!(reader(&[253, 0x01]).u255(), Err(WoffError::Truncated));
    assert_eq!(reader(&[]).u255(), Err(WoffError::Truncated));
}

/// §3.1 forbids two spellings of a `UIntBase128` outright, and both are the
/// kind a decoder written from the diagram would accept.
#[test]
fn uintbase128_refuses_both_spellings_the_spec_forbids() {
    let mut caught = 0;

    // "Any value that would be encoded with a leading zero is invalid" — a
    // first byte of 0x80 is a continuation carrying no bits.
    assert!(
        matches!(
            reader(&[0x80, 0x01]).base128(),
            Err(WoffError::Malformed(_))
        ),
        "a leading zero is refused"
    );
    caught += 1;

    // "The maximum size ... is 5 bytes."
    assert!(
        matches!(
            reader(&[0x81, 0x81, 0x81, 0x81, 0x81, 0x01]).base128(),
            Err(WoffError::Malformed(_))
        ),
        "a sixth continuation byte is refused"
    );
    caught += 1;

    // A value past 2^32 - 1 in five legal bytes.
    assert!(
        matches!(
            reader(&[0x90, 0x80, 0x80, 0x80, 0x00]).base128(),
            Err(WoffError::Malformed(_))
        ),
        "an overflowing value is refused rather than wrapped"
    );
    caught += 1;

    // Truncation mid-sequence.
    assert_eq!(reader(&[0x81, 0x81]).base128(), Err(WoffError::Truncated));
    caught += 1;

    assert_eq!(caught, 4, "four refusals");

    // And the values that are legal, at both ends.
    assert_eq!(reader(&[0x00]).base128().unwrap(), 0);
    assert_eq!(reader(&[0x7F]).base128().unwrap(), 127);
    assert_eq!(reader(&[0x81, 0x00]).base128().unwrap(), 128);
    assert_eq!(
        reader(&[0x8F, 0xFF, 0xFF, 0xFF, 0x7F]).base128().unwrap(),
        u32::MAX,
        "2^32 - 1 fits in five bytes and is not the overflow case"
    );
}

// ---- the containers announce themselves --------------------------------------

/// Neither signature is guessed at, and nothing else is either container.
#[test]
fn the_signature_decides_and_nothing_else_does() {
    assert_eq!(packaging(b"wOFF...."), Some(Packaging::Woff));
    assert_eq!(packaging(b"wOF2...."), Some(Packaging::Woff2));
    assert_eq!(packaging(b"\x00\x01\x00\x00"), None, "a bare TrueType");
    assert_eq!(packaging(b"OTTO"), None, "a bare CFF-flavoured sfnt");
    assert_eq!(packaging(b"ttcf"), None, "a bare collection");
    assert_eq!(packaging(b"wOF"), None, "three bytes decide nothing");
    assert_eq!(packaging(b""), None);
    assert_eq!(decode(b"", ROOMY), Err(WoffError::NotAWoff));

    assert_eq!(Packaging::Woff.name(), "woff");
    assert_eq!(Packaging::Woff2.name(), "woff2");
}

// ---- hand-built containers ---------------------------------------------------

/// A minimal WOFF 1.0 with one stored table, which is the smallest thing the
/// header checks can be aimed at.
///
/// Built here rather than committed because every test below wants a
/// *different* one: a fixture can be corrupted but it cannot be made to have
/// a table count of zero and still be a file anyone would recognise.
fn woff1(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let count = tables.len();
    let mut header = vec![0u8; WOFF1_HEADER + count * WOFF1_ENTRY];
    header[0..4].copy_from_slice(b"wOFF");
    header[4..8].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    header[12..14].copy_from_slice(&(count as u16).to_be_bytes());

    let mut body = Vec::new();
    let mut sfnt_size = 12 + count * 16;
    for (i, (tag, data)) in tables.iter().enumerate() {
        let at = WOFF1_HEADER + i * WOFF1_ENTRY;
        let offset = header.len() + body.len();
        header[at..at + 4].copy_from_slice(*tag);
        header[at + 4..at + 8].copy_from_slice(&(offset as u32).to_be_bytes());
        header[at + 8..at + 12].copy_from_slice(&(data.len() as u32).to_be_bytes());
        header[at + 12..at + 16].copy_from_slice(&(data.len() as u32).to_be_bytes());
        let tag = u32::from_be_bytes(**tag);
        header[at + 16..at + 20].copy_from_slice(&directory_checksum(tag, data).to_be_bytes());
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        sfnt_size += (data.len() + 3) & !3;
    }
    header[16..20].copy_from_slice(&(sfnt_size as u32).to_be_bytes());
    let mut out = header;
    out.extend_from_slice(&body);
    let total = out.len() as u32;
    out[8..12].copy_from_slice(&total.to_be_bytes());
    out
}

/// A `head` table long enough to be one, with the two fields anything reads.
fn head() -> Vec<u8> {
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    head[50..52].copy_from_slice(&1i16.to_be_bytes());
    head
}

/// The builder builds something this decoder accepts, or every refusal test
/// below is asserting against a file that was broken to begin with.
#[test]
fn the_hand_built_woff1_is_accepted_before_it_is_broken() {
    let woff = woff1(&[(b"head", head()), (b"cvt ", vec![1, 2, 3, 4, 5, 6, 7, 8])]);
    let font = decode(&woff, ROOMY).expect("a hand-built WOFF 1.0 decodes");
    let sfnt = Sfnt::parse(&font).expect("it is an sfnt");
    assert_eq!(sfnt.units_per_em, 1000);
    assert_eq!(
        sfnt.table(0x6376_7420),
        Some(&[1u8, 2, 3, 4, 5, 6, 7, 8][..]),
        "the second table came through"
    );
}

/// Five things WOFF 1.0's header states that this build checks rather than
/// believes.
#[test]
fn a_woff1_header_is_checked_before_it_is_believed() {
    let mut caught = 0;
    let good = woff1(&[(b"head", head())]);

    // §3: the reserved field "MUST be set to zero. If this field is non-zero,
    // a conforming user agent MUST reject the file."
    let mut broken = good.clone();
    broken[14] = 1;
    assert!(matches!(
        decode(&broken, ROOMY),
        Err(WoffError::Malformed(_))
    ));
    caught += 1;

    // A directory with nothing in it. Not a rule either specification states
    // in those words — it is what `numTables` of zero means, and an sfnt with
    // no tables is not a font.
    let mut broken = good.clone();
    broken[12..14].copy_from_slice(&0u16.to_be_bytes());
    assert!(matches!(
        decode(&broken, ROOMY),
        Err(WoffError::Malformed(_))
    ));
    caught += 1;

    // §5: "WOFF files containing table directory entries for which compLength
    // is greater than origLength are considered invalid."
    let mut broken = good.clone();
    let at = WOFF1_HEADER;
    broken[at + 8..at + 12].copy_from_slice(&1000u32.to_be_bytes());
    assert!(matches!(
        decode(&broken, ROOMY),
        Err(WoffError::Malformed(_))
    ));
    caught += 1;

    // §4: a totalSfntSize that disagrees with the directory.
    let mut broken = good.clone();
    broken[16..20].copy_from_slice(&99u32.to_be_bytes());
    assert!(matches!(
        decode(&broken, ROOMY),
        Err(WoffError::Malformed(_))
    ));
    caught += 1;

    // §5's origChecksum, which is the only end-to-end integrity check either
    // container carries.
    let mut broken = good.clone();
    broken[at + 16..at + 20].copy_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
    assert_eq!(
        decode(&broken, ROOMY),
        Err(WoffError::TableChecksum { tag: 0x6865_6164 })
    );
    caught += 1;

    assert_eq!(caught, 5, "five header rules");
}

/// A table offset that points outside the file is short, not a panic.
#[test]
fn a_woff1_table_outside_the_file_is_truncated() {
    let mut woff = woff1(&[(b"head", head())]);
    let at = WOFF1_HEADER;
    woff[at + 4..at + 8].copy_from_slice(&0xFFFF_0000u32.to_be_bytes());
    assert_eq!(decode(&woff, ROOMY), Err(WoffError::Truncated));
}

/// `head` is checksummed with `checkSumAdjustment` taken as zero, and every
/// other table is checksummed as it stands.
///
/// The rule sits in the sfnt specification rather than in either WOFF, which
/// is exactly why it is easy to miss: both producers copy the value out of a
/// real directory, so a decoder that summed `head`'s raw bytes refuses every
/// well-formed WOFF in the world and reports it as a damaged font.
#[test]
fn head_is_the_one_table_whose_checksum_is_not_its_bytes() {
    let mut head = head();
    head[8..12].copy_from_slice(&0x1234_5678u32.to_be_bytes());
    let mut zeroed = head.clone();
    zeroed[8..12].fill(0);

    assert_eq!(directory_checksum(TAG_HEAD, &head), checksum(&zeroed));
    assert_ne!(
        directory_checksum(TAG_HEAD, &head),
        checksum(&head),
        "the adjustment is excluded, so the two differ"
    );
    // Any other tag is its bytes and nothing else.
    assert_eq!(directory_checksum(TAG_MAXP, &head), checksum(&head));
    // A `head` too short to hold the field falls back rather than indexing.
    assert_eq!(
        directory_checksum(TAG_HEAD, &[1, 2, 3]),
        checksum(&[1, 2, 3])
    );
}

/// §5's padding rule: the sum is over the table as *stored*, so a table whose
/// length is not a multiple of four is summed as though the missing bytes were
/// zero.
#[test]
fn a_checksum_pads_the_final_word_with_zeros() {
    assert_eq!(checksum(&[]), 0);
    assert_eq!(checksum(&[0, 0, 0, 1]), 1);
    assert_eq!(checksum(&[1]), 0x0100_0000, "one byte, zero-padded");
    assert_eq!(checksum(&[1, 2, 3]), 0x0102_0300);
    // It wraps rather than overflowing.
    assert_eq!(checksum(&[0xFF; 8]), 0xFFFF_FFFEu32);
}

// ---- WOFF 2.0 ----------------------------------------------------------------

/// A WOFF2 header over a directory and a Brotli stream the caller supplies.
fn woff2(flavor: u32, directory: &[u8], compressed: &[u8], block: usize) -> Vec<u8> {
    let mut out = vec![0u8; WOFF2_HEADER];
    out[0..4].copy_from_slice(b"wOF2");
    out[4..8].copy_from_slice(&flavor.to_be_bytes());
    out[12..14].copy_from_slice(&1u16.to_be_bytes());
    out[16..20].copy_from_slice(&(block as u32).to_be_bytes());
    out[20..24].copy_from_slice(&(compressed.len() as u32).to_be_bytes());
    out.extend_from_slice(directory);
    out.extend_from_slice(compressed);
    let len = out.len() as u32;
    out[8..12].copy_from_slice(&len.to_be_bytes());
    out
}

/// §3.2 says the reserved field is **not** grounds for refusal, where WOFF 1.0
/// §3 says its own is. This asserts the non-refusal, which is the assertion
/// nothing else would notice the loss of.
#[test]
fn the_two_fields_a_woff2_reads_and_ignores() {
    // A directory naming one table: `cvt ` (known index 8), version 0, length
    // 4, untransformed. The block is those four bytes, stored uncompressed by
    // a one-meta-block Brotli stream.
    let directory = [8u8, 4];
    let block = [0xAAu8, 0xBB, 0xCC, 0xDD];
    let compressed = brotli_uncompressed(&block);

    let good = woff2(0x0001_0000, &directory, &compressed, block.len());
    let font = decode(&good, ROOMY).expect("a hand-built WOFF 2.0 decodes");
    assert_eq!(
        Sfnt::parse(&font).and_then(|s| s.table(0x6376_7420)),
        Some(&block[..]),
        "the table came through"
    );

    let mut reserved = good.clone();
    reserved[14..16].copy_from_slice(&0xFFFFu16.to_be_bytes());
    assert_eq!(
        decode(&reserved, ROOMY),
        Ok(font.clone()),
        "§3.2: a decoder MUST NOT reject a WOFF2 for a non-zero reserved field"
    );

    // And the second field the two containers disagree about. WOFF 1.0 §4:
    // "If this value is incorrect, a conforming user agent MUST reject the
    // file as invalid." WOFF 2.0 §3.2, of the field with the same name:
    // "User agents MUST NOT reject correctly decoded font file if the
    // resulting font file size doesn't match the totalSfntSize value", and it
    // gives the reason a paragraph later — a transformed `glyf` reconstructs
    // to a size the encoder had no way to state in advance. So a build that
    // enforced WOFF 1.0's rule here would refuse files for being *correct*.
    let mut wrong_size = good;
    wrong_size[16..20].copy_from_slice(&0xDEAD_BEEFu32.to_be_bytes());
    assert_eq!(
        decode(&wrong_size, ROOMY),
        Ok(font),
        "§3.2: totalSfntSize is for reference and is not a gate"
    );
}

/// Four things WOFF 2.0's header and directory state that this build checks.
#[test]
fn a_woff2_header_is_checked_before_it_is_believed() {
    let mut caught = 0;
    let directory = [8u8, 4];
    let block = [0xAAu8, 0xBB, 0xCC, 0xDD];
    let compressed = brotli_uncompressed(&block);
    let good = woff2(0x0001_0000, &directory, &compressed, block.len());

    // A directory with no entries.
    let mut broken = good.clone();
    broken[12..14].copy_from_slice(&0u16.to_be_bytes());
    assert!(matches!(
        decode(&broken, ROOMY),
        Err(WoffError::Malformed(_))
    ));
    caught += 1;

    // More tables than a font plausibly has, stated before a byte is read.
    let mut broken = good.clone();
    broken[12..14].copy_from_slice(&0xFFFFu16.to_be_bytes());
    assert!(matches!(
        decode(&broken, ROOMY),
        Err(WoffError::Malformed(_) | WoffError::Truncated)
    ));
    caught += 1;

    // §5: "The sum of the origLength ... and transformLength ... MUST equal
    // the size of the font data block after it has been decompressed." Widen
    // the directory's declared length and the block no longer matches it.
    let mut broken = good.clone();
    broken[WOFF2_HEADER + 1] = 8;
    assert!(
        matches!(decode(&broken, ROOMY), Err(WoffError::Malformed(_))),
        "a decompressed block that is not the size the directory declared"
    );
    caught += 1;

    // A compressed stream that is not one.
    let mut broken = good.clone();
    let at = WOFF2_HEADER + directory.len();
    broken[at] ^= 0xFF;
    assert!(
        matches!(decode(&broken, ROOMY), Err(WoffError::Malformed(_))),
        "§5: a data block that will not decompress invalidates the file"
    );
    caught += 1;

    assert_eq!(caught, 4, "four header rules");
}

/// §4.1: "If a decoder encounters a table entry that specifies an unknown
/// transformation version number the entire font MUST be rejected."
///
/// Three tags, because the legal version numbers are different for each and a
/// build that applied one table's rule to another would accept a file it
/// cannot reverse. `glyf` and `loca` take 0 (transformed) or 3 (null); `hmtx`
/// takes 0 (null) or 1 (transformed) — the sense is inverted between them.
#[test]
fn a_transform_this_build_cannot_reverse_is_named() {
    let mut caught = 0;

    // `cvt ` is known index 8 and its only legal version is 0.
    for version in 1u8..=3 {
        let directory = [8 | (version << 6), 4];
        let block = [0u8; 4];
        let compressed = brotli_uncompressed(&block);
        let file = woff2(0x0001_0000, &directory, &compressed, block.len());
        assert_eq!(
            decode(&file, ROOMY),
            Err(WoffError::UnknownTransform {
                tag: 0x6376_7420,
                version
            }),
            "cvt version {version}"
        );
    }
    caught += 1;

    // `glyf` (index 10): versions 1 and 2 are neither the transform nor the
    // null transform.
    for version in [1u8, 2] {
        let directory = [10 | (version << 6), 4];
        let compressed = brotli_uncompressed(&[0u8; 4]);
        let file = woff2(0x0001_0000, &directory, &compressed, 4);
        assert!(
            matches!(
                decode(&file, ROOMY),
                Err(WoffError::UnknownTransform { .. })
            ),
            "glyf version {version}"
        );
    }
    caught += 1;

    // `hmtx` (index 3): versions 2 and 3.
    for version in [2u8, 3] {
        let directory = [3 | (version << 6), 4];
        let compressed = brotli_uncompressed(&[0u8; 4]);
        let file = woff2(0x0001_0000, &directory, &compressed, 4);
        assert!(
            matches!(
                decode(&file, ROOMY),
                Err(WoffError::UnknownTransform { .. })
            ),
            "hmtx version {version}"
        );
    }
    caught += 1;

    assert_eq!(caught, 3, "three tables, three different rules");
}

/// A transformed `loca` that claims a length of its own (§5.3 makes it zero).
#[test]
fn a_transformed_loca_with_a_length_is_refused() {
    // `glyf` transformed (index 10, version 0), then `loca` transformed
    // (index 11, version 0) with a non-zero transformLength.
    let directory = [10u8, 4, 4, 11, 4, 4];
    let compressed = brotli_uncompressed(&[0u8; 8]);
    let file = woff2(0x0001_0000, &directory, &compressed, 8);
    assert!(matches!(decode(&file, ROOMY), Err(WoffError::Malformed(_))));
}

/// A four-byte tag that is not in §4.1's table of 63, which is what index 63
/// announces.
#[test]
fn an_unknown_tag_is_carried_through_rather_than_dropped() {
    let mut directory = vec![63u8];
    directory.extend_from_slice(b"ZZZZ");
    directory.push(4);
    let block = [1u8, 2, 3, 4];
    let compressed = brotli_uncompressed(&block);
    let file = woff2(0x0001_0000, &directory, &compressed, block.len());
    let font = decode(&file, ROOMY).expect("an unknown tag is still a table");
    let sfnt = Sfnt::parse(&font).expect("it is an sfnt");
    assert_eq!(
        sfnt.table(u32::from_be_bytes(*b"ZZZZ")),
        Some(&block[..]),
        "a tag this build has no opinion about is still carried"
    );
}

/// Every known-tag index resolves to the tag §4.1 lists for it, and 63 is not
/// one of them.
#[test]
fn the_known_tag_table_is_the_one_the_spec_prints() {
    assert_eq!(KNOWN_TAGS.len(), 63, "index 63 means a tag follows");
    // The first ten, which are the sfnt tables every face has.
    assert_eq!(KNOWN_TAGS[0], b"cmap");
    assert_eq!(KNOWN_TAGS[1], b"head");
    assert_eq!(KNOWN_TAGS[10], b"glyf");
    assert_eq!(KNOWN_TAGS[11], b"loca");
    assert_eq!(KNOWN_TAGS[13], b"CFF ");
    assert_eq!(KNOWN_TAGS[62], b"Sill");
    // §4.1 ends by reminding that a short tag is padded with spaces, and
    // three of these are. A tag that dropped the space is one no reader looks
    // for, so the padding is asserted rather than assumed.
    for tag in [b"cvt ", b"OS/2", b"SVG "] {
        assert!(
            KNOWN_TAGS.contains(&tag),
            "{}",
            String::from_utf8_lossy(tag)
        );
    }
}

// ---- the ceiling -------------------------------------------------------------

/// `max_output` is not advisory, and it is checked before the memory is asked
/// for rather than after.
///
/// A WOFF2 directory states its lengths in `UIntBase128`, which reaches
/// 2^32 - 1 in five bytes: a forty-byte file can ask for four gigabytes, and a
/// decoder that allocated first would be a denial of service with a font
/// extension.
#[test]
fn the_output_ceiling_is_not_advisory() {
    let mut caught = 0;

    // WOFF 1.0, where the length is a plain u32.
    let woff = woff1(&[(b"head", head())]);
    assert_eq!(
        decode(&woff, 16),
        Err(WoffError::ExceedsOutputLimit { limit: 16 })
    );
    caught += 1;

    // WOFF 2.0, where it is not.
    let directory = [8u8, 4];
    let compressed = brotli_uncompressed(&[0u8; 4]);
    let file = woff2(0x0001_0000, &directory, &compressed, 4);
    assert_eq!(
        decode(&file, 8),
        Err(WoffError::ExceedsOutputLimit { limit: 8 })
    );
    caught += 1;

    // And the one that matters: a directory claiming four gigabytes, refused
    // without decompressing anything. `0x8F FF FF FF 7F` is 2^32 - 1.
    let directory = [8u8, 0x8F, 0xFF, 0xFF, 0xFF, 0x7F];
    let file = woff2(0x0001_0000, &directory, &[0u8; 4], 4);
    assert_eq!(
        decode(&file, ROOMY),
        Err(WoffError::ExceedsOutputLimit { limit: ROOMY }),
        "the claim is refused before a byte is decompressed"
    );
    caught += 1;

    assert_eq!(caught, 3, "three ceilings");
}

// ---- messages ----------------------------------------------------------------

/// Every error prints, and a tag prints as the four characters it is.
///
/// Ruling 10: a warning carries provenance, and "the glyf table's checksum
/// disagrees" is provenance where "error 5" is not.
#[test]
fn every_refusal_says_which_table_and_why() {
    assert_eq!(tag_name(0x676C_7966), "glyf");
    assert_eq!(tag_name(0x4F53_2F32), "OS/2");
    assert_eq!(tag_name(0x6376_7420), "cvt ");
    // A tag with bytes outside printable ASCII prints them as `?` rather than
    // as whatever the terminal makes of them.
    assert_eq!(tag_name(0x0001_0203), "????");

    let messages = [
        WoffError::NotAWoff.to_string(),
        WoffError::Truncated.to_string(),
        WoffError::Malformed("a reason").to_string(),
        WoffError::TableChecksum { tag: TAG_GLYF }.to_string(),
        WoffError::Unpackable { tag: TAG_LOCA }.to_string(),
        WoffError::UnknownTransform {
            tag: TAG_HMTX,
            version: 2,
        }
        .to_string(),
        WoffError::ExceedsOutputLimit { limit: 99 }.to_string(),
    ];
    for message in &messages {
        assert!(!message.is_empty());
        assert!(
            !message.contains("WoffError"),
            "a message, not a variant name: {message}"
        );
    }
    assert!(messages[3].contains("glyf"));
    assert!(messages[4].contains("loca"));
    assert!(messages[5].contains("hmtx") && messages[5].contains('2'));
    assert!(messages[6].contains("99"));
}

// ---- ruling 1: it never panics -----------------------------------------------

/// A sweep of shapes with no committed file behind them.
///
/// Not a fuzz target — `fuzz/fuzz_targets/woff.rs` is — and not a substitute
/// for one. This runs on every `cargo test`, which is where a regression is
/// cheapest to find.
#[test]
fn nothing_here_panics_on_nonsense() {
    let mut inputs: Vec<Vec<u8>> = Vec::new();
    for signature in [&b"wOFF"[..], b"wOF2", b"\x00\x01\x00\x00"] {
        for length in 0..80usize {
            let mut bytes = signature.to_vec();
            bytes.resize(length.max(signature.len()), 0);
            inputs.push(bytes.clone());
            // The same length with every byte set, which drives every count,
            // length and offset to its maximum.
            let mut full = signature.to_vec();
            full.resize(length.max(signature.len()), 0xFF);
            inputs.push(full);
        }
    }
    // A header claiming the maximum table count, with nothing behind it.
    let mut greedy = vec![0u8; WOFF1_HEADER];
    greedy[0..4].copy_from_slice(b"wOFF");
    greedy[12..14].copy_from_slice(&0xFFFFu16.to_be_bytes());
    inputs.push(greedy);

    for input in &inputs {
        // Both a roomy ceiling and a tiny one: the tiny one takes the early
        // return and the roomy one walks the whole directory.
        let _ = decode(input, ROOMY);
        let _ = decode(input, 1);
        let _ = decode(input, 0);
    }
    assert!(
        inputs.len() > 400,
        "the sweep is not empty: {}",
        inputs.len()
    );
}

// ---- a Brotli stream this file can build -------------------------------------

/// One uncompressed meta-block holding `data`, as §9.2 describes it.
///
/// Hand-built because there is no encoder in this tree — `tinker-pdf-filters`
/// decodes Brotli and does not produce it — and because an uncompressed
/// meta-block is the one shape whose bits can be written out by hand in a
/// dozen lines. Every WOFF2 above needs *a* valid stream and none of them
/// cares what compression was applied to it.
fn brotli_uncompressed(data: &[u8]) -> Vec<u8> {
    assert!(!data.is_empty() && data.len() <= 0x0100_0000);
    let mut bits: Vec<bool> = Vec::new();
    let mut push = |value: u32, count: u32| {
        for i in 0..count {
            bits.push((value >> i) & 1 == 1);
        }
    };
    // WBITS: a single 0 bit means a window of 16 (§9.1).
    push(0, 1);
    // ISLAST = 0: this is not the final meta-block, so a second empty one
    // follows and closes the stream.
    push(0, 1);
    // MNIBBLES = 4 nibbles (the 00 code), then MLEN - 1 in 16 bits.
    push(0, 2);
    push((data.len() - 1) as u32, 16);
    // ISUNCOMPRESSED = 1.
    push(1, 1);
    // Pad to a byte boundary; the literal bytes follow whole.
    while bits.len() % 8 != 0 {
        bits.push(false);
    }

    let mut out = Vec::new();
    for chunk in bits.chunks(8) {
        let mut byte = 0u8;
        for (i, bit) in chunk.iter().enumerate() {
            if *bit {
                byte |= 1 << i;
            }
        }
        out.push(byte);
    }
    out.extend_from_slice(data);

    // The closing meta-block: ISLAST = 1, ISLASTEMPTY = 1.
    let mut tail: Vec<bool> = vec![true, true];
    while tail.len() % 8 != 0 {
        tail.push(false);
    }
    let mut byte = 0u8;
    for (i, bit) in tail.iter().enumerate() {
        if *bit {
            byte |= 1 << i;
        }
    }
    out.push(byte);
    out
}

/// The hand-built Brotli stream is one, or every WOFF2 above is testing a
/// decoder against bytes that were never a stream.
#[test]
fn the_hand_built_brotli_stream_decodes() {
    for payload in [&[0xAAu8][..], &[1, 2, 3, 4], &[0x5A; 300]] {
        let stream = brotli_uncompressed(payload);
        let out = brotli_decode(&stream, &Limits::new(1 << 16))
            .expect("an uncompressed meta-block decodes");
        assert_eq!(out, payload);
    }
}
