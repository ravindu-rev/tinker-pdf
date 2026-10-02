//! Zstandard, hand-rolled: RFC 8878's frames and blocks, its Huffman-coded
//! literals, its FSE-coded sequences and their repeat offsets, and XXH64 for
//! the optional content checksum.
//!
//! One container carries it here: ZIP method 93 (APPNOTE 4.4.5, "Zstandard
//! (zstd) Compression"), whose entry data is a whole Zstandard stream — one
//! or more frames, skippable frames allowed — and whose directory declares
//! the unpacked size, so the output is bounded by the caller before a bit is
//! read.
//!
//! # Where it was written from
//!
//! RFC 8878 (February 2021), "Zstandard Compression and the
//! 'application/zstd' Media Type", read from rfc-editor.org on 26 September
//! 2026, and the zstd project's own `doc/zstd_compression_format.md`
//! (version 0.4.5, read the same day from `facebook/zstd`'s `dev` branch),
//! which is the RFC's text kept current. Where the two are silent the
//! decoder takes the reference implementation's reading, and each such
//! place is marked where it is made; there are three, all refusals the
//! golden files below hold a decoder to or that no encoder could trip:
//!
//! - a sequences section with zero sequences must end there
//!   (`zeroSeq_extraneous.zst`);
//! - two initial states an FSE-coded Huffman weight stream does not hold are
//!   corruption (`truncated_huff_state.zst`);
//! - "the expected number of symbols" in an FSE table description (§4.1.1)
//!   is read as *at most* the context's alphabet, which is what every
//!   encoder writes.
//!
//! No line of libzstd is copied or was read for this; the format documents
//! were enough, and their Appendix A — the three predefined decoding
//! tables, state by state — is what the table builder is held to.
//!
//! **What adjudicates it is three things, and none is this crate's.** A
//! frame may carry the low 32 bits of XXH64 over its content, and every one
//! that does is checked before a byte is handed back; a frame may declare
//! its content size, and every one that does is held to it; and ZIP checks
//! its own CRC-32 over the entry. The fixtures are python-zstandard 0.25.0
//! over libzstd 1.5.7 — the reference encoder — coding files made in this
//! repository (`tests/coders/`), so the expected answer is the file that
//! went in; and the zstd project's own `tests/golden-decompression/` and
//! `tests/golden-decompression-errors/` files, vendored in
//! `data/zstd-golden/`, which pin the corners an encoder is careful never to
//! write: an RLE first block, an empty block, a compressed block exactly
//! 128 KiB long, a zero-sequence count in its two-byte form, and three
//! frames a decoder must refuse.
//!
//! # Refused by name
//!
//! **Dictionaries** ([`Error::NeedsDictionary`]). A frame whose header names
//! a dictionary cannot be decoded without it, and neither ZIP nor anything
//! else here says where one would come from. A frame that needs one and does
//! not name it (a dictionary ID of zero is allowed to mean "unspecified")
//! reads back-references to bytes before the frame's first, which is
//! [`Error::BadOffset`].
//!
//! Zstandard's pre-1.0 "legacy" frames (magic numbers `0xFD2FB522` to
//! `0xFD2FB527`) are not Zstandard as RFC 8878 defines it and are
//! [`Error::NotZstd`].
//!
//! # Untrusted bytes
//!
//! Ruling 1. The output is bounded by [`Limits::max_unpacked`], checked
//! before every byte is written, and a frame that declares more content than
//! that is refused before its first block. Nothing else a stream says sizes
//! an allocation: the decoding tables are at most 512 rows and 2 048
//! entries, one block's literals at most 128 KiB, and the window — which a
//! frame may declare up to 3.75 TiB — is never allocated at all, because the
//! output *is* the window: a back-reference reads the bytes already decoded,
//! and is refused if it reaches before the frame's first byte or further
//! back than the frame's window. Every bit read past a stream's start is
//! detected and refused, and every stream must be used exactly.

mod bits;
mod fse;
mod huffman;
mod xxhash;

#[cfg(test)]
mod tests;

use bits::Backward;
use fse::{Fault, Table};
use huffman::{Huffman, Weights};

/// What one decode may produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The most bytes the whole input may decode to. Callers here pass the
    /// entry's declared size, already bounded by their own container's cap.
    pub max_unpacked: usize,
}

/// Why a stream did not decode. Every variant is a fact about the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Error {
    /// The input, or what follows a complete frame, does not begin with a
    /// Zstandard frame's magic number (`0xFD2FB528`) or a skippable frame's
    /// (`0x184D2A50` to `0x184D2A5F`).
    NotZstd,
    /// The input ended inside a frame.
    Truncated,
    /// A frame header with its reserved bit set (§3.1.1.1.1.4).
    BadFrameHeader,
    /// A frame whose header names a dictionary. See this module's header.
    NeedsDictionary,
    /// A block of the reserved type, or one larger — compressed or
    /// decompressed — than its frame's `Block_Maximum_Size`.
    BadBlock,
    /// A literals section that is not the format: sizes that do not fit the
    /// block, a Huffman description that does not describe a code, a
    /// treeless block with no tree before it, or a stream not used exactly.
    BadLiterals,
    /// A sequences section that is not the format: bytes after a zero
    /// sequence count, a reserved mode bit, a table description that does
    /// not describe a table, a repeat of a table no block has set, a
    /// literal length past the literals, or a stream not used exactly.
    BadSequences,
    /// A back-reference to offset zero, before the frame's first byte, or
    /// further back than the frame's window.
    BadOffset,
    /// A frame whose content is not the size its header declares.
    ContentSizeMismatch,
    /// A frame whose content does not hash to its `Content_Checksum`.
    ChecksumMismatch,
    /// The output would pass [`Limits::max_unpacked`].
    TooLarge,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Error::NotZstd => "not a Zstandard frame",
            Error::Truncated => "a Zstandard frame that ends early",
            Error::BadFrameHeader => "a Zstandard frame header with its reserved bit set",
            Error::NeedsDictionary => "a Zstandard frame that needs a dictionary",
            Error::BadBlock => "a Zstandard block of the reserved type or past its maximum size",
            Error::BadLiterals => "a Zstandard literals section that is not the format",
            Error::BadSequences => "a Zstandard sequences section that is not the format",
            Error::BadOffset => "a back-reference outside the frame's output or window",
            Error::ContentSizeMismatch => "a frame whose content is not its declared size",
            Error::ChecksumMismatch => "a frame whose content checksum does not match",
            Error::TooLarge => "an output past this build's cap",
        })
    }
}

impl std::error::Error for Error {}

/// A Zstandard frame's magic number, little-endian.
const FRAME_MAGIC: u32 = 0xFD2F_B528;
/// A skippable frame's, with its low four bits free.
const SKIPPABLE_MAGIC: u32 = 0x184D_2A50;
/// `Block_Maximum_Size`'s ceiling, whatever the window: 128 KiB.
const MAX_BLOCK: u64 = 128 * 1024;

/// What the decoder met on its way through an input, for the tests that
/// hold the fixtures to reaching every part of the format. Counting costs a
/// few increments per block; nothing outside the tests reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Census {
    pub(crate) frames: u32,
    pub(crate) skippable_frames: u32,
    pub(crate) checksums: u32,
    pub(crate) single_segment: u32,
    /// Raw, RLE and compressed blocks.
    pub(crate) blocks: [u32; 3],
    /// Raw, RLE, compressed and treeless literals sections.
    pub(crate) literals: [u32; 4],
    pub(crate) four_streams: u32,
    /// Huffman descriptions with FSE-coded and with direct weights.
    pub(crate) weights: [u32; 2],
    /// Per symbol type — literals length, offset, match length — the blocks
    /// that used each mode: predefined, RLE, FSE-compressed, repeat.
    pub(crate) modes: [[u32; 4]; 3],
    pub(crate) sequences: u64,
    /// Sequences that used a repeat offset: the first, second and third
    /// repeat, and the first minus one.
    pub(crate) repeats: [u64; 4],
    /// Matches that overlap their own output.
    pub(crate) overlaps: u64,
    /// Sequences that used a repeat offset an earlier block left, before
    /// their own block had decoded an offset of its own.
    pub(crate) carried_repeats: u64,
    /// Treeless literals sections after a raw or RLE one in the same frame:
    /// the tree outlives sections that did not replace it.
    pub(crate) treeless_after_plain: u32,
}

/// Decodes every frame of `input`, one after another, skipping skippable
/// frames. The input must be frames and nothing else.
///
/// # Errors
/// [`Error`], one variant per way the input is not a Zstandard stream this
/// build reads.
pub fn decode(input: &[u8], limits: &Limits) -> Result<Vec<u8>, Error> {
    decode_counting(input, limits, &mut Census::default())
}

/// [`decode`], counting what it meets into `census`.
pub(crate) fn decode_counting(
    input: &[u8],
    limits: &Limits,
    census: &mut Census,
) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let mut rest = input;
    loop {
        let Some((magic, _)) = rest.split_first_chunk::<4>() else {
            // Nothing left after at least one frame is the end; anything
            // else — an empty input, or one to three stray bytes — is not a
            // frame.
            if rest.is_empty() && census.frames + census.skippable_frames > 0 {
                return Ok(out);
            }
            return Err(Error::NotZstd);
        };
        let magic = u32::from_le_bytes(*magic);
        let used = if magic == FRAME_MAGIC {
            census.frames += 1;
            decode_frame(rest, limits, &mut out, census)?
        } else if magic & 0xFFFF_FFF0 == SKIPPABLE_MAGIC {
            census.skippable_frames += 1;
            let size = rest
                .get(4..8)
                .and_then(|b| b.try_into().ok())
                .map(u32::from_le_bytes)
                .ok_or(Error::Truncated)?;
            usize::try_from(size)
                .ok()
                .and_then(|size| size.checked_add(8))
                .filter(|&end| end <= rest.len())
                .ok_or(Error::Truncated)?
        } else {
            return Err(Error::NotZstd);
        };
        rest = rest.get(used..).ok_or(Error::Truncated)?;
    }
}

/// What a frame carries from block to block (§3.1.1.3, "Prerequisites").
struct Frame {
    /// The last Huffman table a compressed literals section described.
    huffman: Option<Huffman>,
    /// The last literals length, offset and match length tables a sequences
    /// section with at least one sequence used.
    tables: [Option<Table>; 3],
    /// The three repeat offsets, most recent first.
    repeats: [u64; 3],
    /// A block with sequences has been decoded, so `repeats` is what it
    /// left rather than the frame's starting three. For the census.
    repeats_carried: bool,
    /// The last literals section was raw or RLE. For the census.
    plain_literals: bool,
    /// Where this frame's output starts in the output buffer.
    start: usize,
    window: u64,
    block_max: usize,
    /// One block's literals, reused.
    literals: Vec<u8>,
}

/// Decodes one frame from the start of `src` (its magic number included)
/// onto `out`, returning the bytes it took.
fn decode_frame(
    src: &[u8],
    limits: &Limits,
    out: &mut Vec<u8>,
    census: &mut Census,
) -> Result<usize, Error> {
    let mut at = 4usize;
    let descriptor = *src.get(at).ok_or(Error::Truncated)?;
    at += 1;
    let single_segment = descriptor & 0x20 != 0;
    if descriptor & 0x08 != 0 {
        return Err(Error::BadFrameHeader);
    }
    let has_checksum = descriptor & 0x04 != 0;

    // §3.1.1.1.2: absent in a single-segment frame, whose window is its
    // content size.
    let mut window = None;
    if !single_segment {
        let byte = *src.get(at).ok_or(Error::Truncated)?;
        at += 1;
        let base = 1u64 << (10 + u32::from(byte >> 3));
        window = Some(base + (base / 8) * u64::from(byte & 7));
    }

    let id_size = [0usize, 1, 2, 4][usize::from(descriptor & 3)];
    let id = little_endian(src, at, id_size).ok_or(Error::Truncated)?;
    at += id_size;
    if id != 0 {
        return Err(Error::NeedsDictionary);
    }

    let content_size = match descriptor >> 6 {
        0 if single_segment => Some(1),
        0 => None,
        1 => Some(2),
        2 => Some(4),
        _ => Some(8),
    }
    .map(|size| {
        let value = little_endian(src, at, size).ok_or(Error::Truncated)?;
        at += size;
        // The two-byte form starts at 256, since one byte covers below it.
        Ok(if size == 2 { value + 256 } else { value })
    })
    .transpose()?;

    let window = window.or(content_size).unwrap_or(0);
    if single_segment {
        census.single_segment += 1;
    }
    // A frame declaring more than the caller allows is refused before its
    // first block rather than after the budget runs out in the middle.
    let room = limits.max_unpacked.saturating_sub(out.len()) as u64;
    if content_size.is_some_and(|size| size > room) {
        return Err(Error::TooLarge);
    }
    let mut frame = Frame {
        huffman: None,
        tables: [None, None, None],
        repeats: [1, 4, 8],
        repeats_carried: false,
        plain_literals: false,
        start: out.len(),
        window,
        block_max: window.min(MAX_BLOCK) as usize,
        literals: Vec::new(),
    };

    loop {
        let header = little_endian(src, at, 3).ok_or(Error::Truncated)?;
        at += 3;
        let last = header & 1 != 0;
        let size = (header >> 3) as usize;
        let kind = (header >> 1) & 3;
        if kind == 3 || size > frame.block_max {
            return Err(Error::BadBlock);
        }
        match kind {
            0 => {
                census.blocks[0] += 1;
                let data = src.get(at..at + size).ok_or(Error::Truncated)?;
                room_for(out, size, limits)?;
                out.extend_from_slice(data);
                at += size;
            }
            1 => {
                census.blocks[1] += 1;
                let byte = *src.get(at).ok_or(Error::Truncated)?;
                room_for(out, size, limits)?;
                out.resize(out.len() + size, byte);
                at += 1;
            }
            _ => {
                census.blocks[2] += 1;
                let data = src.get(at..at + size).ok_or(Error::Truncated)?;
                decode_block(data, &mut frame, out, limits, census)?;
                at += size;
            }
        }
        let produced = (out.len() - frame.start) as u64;
        if content_size.is_some_and(|size| produced > size) {
            return Err(Error::ContentSizeMismatch);
        }
        if last {
            if content_size.is_some_and(|size| produced != size) {
                return Err(Error::ContentSizeMismatch);
            }
            break;
        }
    }

    if has_checksum {
        census.checksums += 1;
        let stored = little_endian(src, at, 4).ok_or(Error::Truncated)?;
        at += 4;
        let content = out.get(frame.start..).unwrap_or(&[]);
        if xxhash::xxh64(content) & 0xFFFF_FFFF != stored {
            return Err(Error::ChecksumMismatch);
        }
    }
    Ok(at)
}

/// `size` bytes little-endian at `at`, `size` at most 8.
fn little_endian(src: &[u8], at: usize, size: usize) -> Option<u64> {
    let bytes = src.get(at..at.checked_add(size)?)?;
    Some(
        bytes
            .iter()
            .rev()
            .fold(0u64, |acc, &b| (acc << 8) | u64::from(b)),
    )
}

fn room_for(out: &[u8], more: usize, limits: &Limits) -> Result<(), Error> {
    match out.len().checked_add(more) {
        Some(total) if total <= limits.max_unpacked => Ok(()),
        _ => Err(Error::TooLarge),
    }
}

/// One compressed block (§3.1.1.3): a literals section, then a sequences
/// section that interleaves copies of those literals with back-references.
fn decode_block(
    block: &[u8],
    frame: &mut Frame,
    out: &mut Vec<u8>,
    limits: &Limits,
    census: &mut Census,
) -> Result<(), Error> {
    let used = read_literals(block, frame, census)?;
    let sequences = block.get(used..).ok_or(Error::BadLiterals)?;
    let block_start = out.len();
    execute_sequences(sequences, frame, out, limits, census)?;
    if out.len() - block_start > frame.block_max {
        return Err(Error::BadBlock);
    }
    Ok(())
}

/// Reads the literals section at the start of `block` into
/// `frame.literals`, returning the bytes it took (§3.1.1.3.1).
fn read_literals(block: &[u8], frame: &mut Frame, census: &mut Census) -> Result<usize, Error> {
    let first = *block.first().ok_or(Error::BadLiterals)?;
    let kind = first & 3;
    let size_format = (first >> 2) & 3;
    frame.literals.clear();

    if kind < 2 {
        // Raw and RLE: a regenerated size and nothing else, in 5, 12 or 20
        // bits.
        let (header, regenerated) = match size_format {
            0 | 2 => (1, usize::from(first >> 3)),
            1 => {
                let v = little_endian(block, 0, 2).ok_or(Error::BadLiterals)?;
                (2, (v >> 4) as usize)
            }
            _ => {
                let v = little_endian(block, 0, 3).ok_or(Error::BadLiterals)?;
                (3, (v >> 4) as usize)
            }
        };
        if regenerated > frame.block_max {
            return Err(Error::BadLiterals);
        }
        census.literals[usize::from(kind)] += 1;
        frame.plain_literals = true;
        if kind == 0 {
            let data = block
                .get(header..header + regenerated)
                .ok_or(Error::BadLiterals)?;
            frame.literals.extend_from_slice(data);
            return Ok(header + regenerated);
        }
        let byte = *block.get(header).ok_or(Error::BadLiterals)?;
        frame.literals.resize(regenerated, byte);
        return Ok(header + 1);
    }

    // Compressed and treeless: both sizes, in 10, 14 or 18 bits each, and
    // one stream or four.
    let (header, bits) = match size_format {
        0 | 1 => (3, 10),
        2 => (4, 14),
        _ => (5, 18),
    };
    let four = size_format != 0;
    let v = little_endian(block, 0, header).ok_or(Error::BadLiterals)?;
    let mask = (1u64 << bits) - 1;
    let regenerated = ((v >> 4) & mask) as usize;
    let compressed = ((v >> (4 + bits)) & mask) as usize;
    if regenerated > frame.block_max {
        return Err(Error::BadLiterals);
    }
    // Four streams split the literals four ways, which fewer than six
    // cannot be (the reference decoder's `MIN_LITERALS_FOR_4_STREAMS`).
    if four && regenerated < 6 {
        return Err(Error::BadLiterals);
    }
    let section = block
        .get(header..header + compressed)
        .ok_or(Error::BadLiterals)?;
    census.literals[usize::from(kind)] += 1;
    if kind == 3 && frame.plain_literals {
        census.treeless_after_plain += 1;
    }
    frame.plain_literals = false;

    let mut streams = section;
    if kind == 2 {
        let (tree, weights, used) = huffman::read_tree(section).map_err(|_| Error::BadLiterals)?;
        census.weights[match weights {
            Weights::Fse => 0,
            Weights::Direct => 1,
        }] += 1;
        frame.huffman = Some(tree);
        streams = section.get(used..).ok_or(Error::BadLiterals)?;
    }
    // A treeless section with no tree before it in the frame is corruption.
    let tree = frame.huffman.as_ref().ok_or(Error::BadLiterals)?;
    frame.literals.reserve(regenerated);
    let decoded = if four {
        census.four_streams += 1;
        tree.decode_four(streams, regenerated, &mut frame.literals)
    } else {
        tree.decode_stream(streams, regenerated, &mut frame.literals)
    };
    decoded.map_err(|_| Error::BadLiterals)?;
    Ok(header + compressed)
}

/// The three symbol types a sequence is coded in.
#[derive(Clone, Copy)]
enum Kind {
    LiteralsLength,
    Offset,
    MatchLength,
}

impl Kind {
    /// The largest code, and the largest accuracy log an FSE description of
    /// this type may declare (§3.1.1.3.2.1).
    fn limits(self) -> (usize, u32) {
        match self {
            Kind::LiteralsLength => (35, 9),
            // RFC 8878 recommends supporting offset codes to at least 22
            // and notes the reference decoder takes 31, which is the most a
            // 32-bit offset can use; this takes 31.
            Kind::Offset => (31, 8),
            Kind::MatchLength => (52, 9),
        }
    }

    fn predefined(self) -> Result<Table, Fault> {
        match self {
            Kind::LiteralsLength => fse::build(&fse::LITERALS_LENGTH_DEFAULT, 6),
            Kind::Offset => fse::build(&fse::OFFSET_DEFAULT, 5),
            Kind::MatchLength => fse::build(&fse::MATCH_LENGTH_DEFAULT, 6),
        }
    }
}

/// §3.1.1.3.2.1.1: literals length codes 0–35 as (baseline, extra bits).
const LITERALS_LENGTH: [(u32, u32); 36] = [
    (0, 0),
    (1, 0),
    (2, 0),
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 0),
    (12, 0),
    (13, 0),
    (14, 0),
    (15, 0),
    (16, 1),
    (18, 1),
    (20, 1),
    (22, 1),
    (24, 2),
    (28, 2),
    (32, 3),
    (40, 3),
    (48, 4),
    (64, 6),
    (128, 7),
    (256, 8),
    (512, 9),
    (1024, 10),
    (2048, 11),
    (4096, 12),
    (8192, 13),
    (16384, 14),
    (32768, 15),
    (65536, 16),
];

/// §3.1.1.3.2.1.1: match length codes 0–52 as (baseline, extra bits).
const MATCH_LENGTH: [(u32, u32); 53] = [
    (3, 0),
    (4, 0),
    (5, 0),
    (6, 0),
    (7, 0),
    (8, 0),
    (9, 0),
    (10, 0),
    (11, 0),
    (12, 0),
    (13, 0),
    (14, 0),
    (15, 0),
    (16, 0),
    (17, 0),
    (18, 0),
    (19, 0),
    (20, 0),
    (21, 0),
    (22, 0),
    (23, 0),
    (24, 0),
    (25, 0),
    (26, 0),
    (27, 0),
    (28, 0),
    (29, 0),
    (30, 0),
    (31, 0),
    (32, 0),
    (33, 0),
    (34, 0),
    (35, 1),
    (37, 1),
    (39, 1),
    (41, 1),
    (43, 2),
    (47, 2),
    (51, 3),
    (59, 3),
    (67, 4),
    (83, 4),
    (99, 5),
    (131, 7),
    (259, 8),
    (515, 9),
    (1027, 10),
    (2051, 11),
    (4099, 12),
    (8195, 13),
    (16387, 14),
    (32771, 15),
    (65539, 16),
];

/// Reads one symbol type's table from `src` at `*at` in `mode`, into
/// `slot` — which holds the previous block's for `Repeat_Mode`.
fn read_table(
    kind: Kind,
    mode: u8,
    src: &[u8],
    at: &mut usize,
    slot: &mut Option<Table>,
) -> Result<(), Error> {
    let (max_symbol, max_log) = kind.limits();
    let table = match mode {
        0 => kind.predefined().map_err(|_| Error::BadSequences)?,
        1 => {
            let symbol = *src.get(*at).ok_or(Error::BadSequences)?;
            *at += 1;
            if usize::from(symbol) > max_symbol {
                return Err(Error::BadSequences);
            }
            Table::rle(symbol)
        }
        2 => {
            let rest = src.get(*at..).ok_or(Error::BadSequences)?;
            let (probs, log, used) = fse::read_distribution(rest, max_symbol, max_log)
                .map_err(|_| Error::BadSequences)?;
            *at += used;
            fse::build(&probs, log).map_err(|_| Error::BadSequences)?
        }
        // Repeat: what the last block with sequences used, which must exist.
        _ => return slot.as_ref().map(|_| ()).ok_or(Error::BadSequences),
    };
    *slot = Some(table);
    Ok(())
}

/// The sequences section (§3.1.1.3.2) and their execution (§3.1.1.4):
/// each sequence copies literals, then copies a match from the output.
fn execute_sequences(
    src: &[u8],
    frame: &mut Frame,
    out: &mut Vec<u8>,
    limits: &Limits,
    census: &mut Census,
) -> Result<(), Error> {
    let first = *src.first().ok_or(Error::BadSequences)?;
    let byte = |i: usize| {
        src.get(i)
            .copied()
            .map(usize::from)
            .ok_or(Error::BadSequences)
    };
    let (count, mut at) = match first {
        0..=127 => (usize::from(first), 1),
        128..=254 => (((usize::from(first) - 128) << 8) + byte(1)?, 2),
        255 => (byte(1)? + (byte(2)? << 8) + 0x7F00, 3),
    };

    if count == 0 {
        // "No sequences: the section ends immediately" — and anything after
        // the count is corruption. The RFC does not say so; the reference
        // decoder refuses it and `zeroSeq_extraneous.zst` holds a decoder to
        // refusing it too.
        if at != src.len() {
            return Err(Error::BadSequences);
        }
        room_for(out, frame.literals.len(), limits)?;
        out.extend_from_slice(&frame.literals);
        return Ok(());
    }

    let modes = *src.get(at).ok_or(Error::BadSequences)?;
    at += 1;
    if modes & 3 != 0 {
        return Err(Error::BadSequences);
    }
    let kinds = [
        (Kind::LiteralsLength, modes >> 6),
        (Kind::Offset, (modes >> 4) & 3),
        (Kind::MatchLength, (modes >> 2) & 3),
    ];
    for (index, (kind, mode)) in kinds.into_iter().enumerate() {
        let slot = frame.tables.get_mut(index).ok_or(Error::BadSequences)?;
        read_table(kind, mode, src, &mut at, slot)?;
        if let Some(c) = census
            .modes
            .get_mut(index)
            .and_then(|m| m.get_mut(usize::from(mode)))
        {
            *c += 1;
        }
    }
    let [Some(ll), Some(of), Some(ml)] = &frame.tables else {
        return Err(Error::BadSequences);
    };

    let stream = src.get(at..).unwrap_or(&[]);
    let mut bits = Backward::new(stream).ok_or(Error::BadSequences)?;
    // Initial states: literals length, then offset, then match length.
    let mut ll_state = bits.read(ll.log) as usize;
    let mut of_state = bits.read(of.log) as usize;
    let mut ml_state = bits.read(ml.log) as usize;

    let literals = &frame.literals;
    let mut taken = 0usize;
    let mut own_offset = false;
    for index in 0..count {
        let (ll_row, of_row, ml_row) = (ll.row(ll_state), of.row(of_state), ml.row(ml_state));
        // Extra bits in the order §3.1.1.3.2.1.2 gives: offset, match
        // length, literals length.
        let code = u32::from(of_row.symbol);
        if code > 31 {
            return Err(Error::BadSequences);
        }
        let offset_value = (1u64 << code) + bits.read(code);
        let (base, extra) = *MATCH_LENGTH
            .get(usize::from(ml_row.symbol))
            .ok_or(Error::BadSequences)?;
        let match_length = u64::from(base) + bits.read(extra);
        let (base, extra) = *LITERALS_LENGTH
            .get(usize::from(ll_row.symbol))
            .ok_or(Error::BadSequences)?;
        let literals_length = u64::from(base) + bits.read(extra);

        let (offset, repeat) = resolve_offset(offset_value, literals_length, &mut frame.repeats)?;
        if let Some(r) = repeat.and_then(|r| census.repeats.get_mut(r)) {
            *r += 1;
        }
        match repeat {
            Some(_) if frame.repeats_carried && !own_offset => census.carried_repeats += 1,
            None => own_offset = true,
            Some(_) => {}
        }

        // Every sequence but the last updates the states: literals length,
        // match length, offset.
        if index + 1 < count {
            ll_state = usize::from(ll_row.base) + bits.read(u32::from(ll_row.bits)) as usize;
            ml_state = usize::from(ml_row.base) + bits.read(u32::from(ml_row.bits)) as usize;
            of_state = usize::from(of_row.base) + bits.read(u32::from(of_row.bits)) as usize;
        }
        if bits.overflowed() {
            return Err(Error::BadSequences);
        }

        // Literals first.
        let literals_length = usize::try_from(literals_length).map_err(|_| Error::BadSequences)?;
        let copy = taken
            .checked_add(literals_length)
            .and_then(|end| literals.get(taken..end))
            .ok_or(Error::BadSequences)?;
        room_for(out, copy.len(), limits)?;
        out.extend_from_slice(copy);
        taken += literals_length;

        // Then the match, from the output this frame has produced, no
        // further back than its window.
        let produced = (out.len() - frame.start) as u64;
        if offset > produced || offset > frame.window {
            return Err(Error::BadOffset);
        }
        let match_length = match_length as usize;
        room_for(out, match_length, limits)?;
        // `offset` is at least 1 and at most `out.len() - frame.start`, so
        // `from` is inside `out`.
        let offset = offset as usize;
        let mut from = out.len() - offset;
        if match_length > offset {
            census.overlaps += 1;
        }
        // A match may overlap its own output (offset 1, length 100 is a run
        // of one byte), so it is copied in pieces no longer than the offset:
        // each piece reads only bytes that exist before it is appended, and
        // `from + piece <= out.len()` holds for every one.
        let mut left = match_length;
        while left > 0 {
            let piece = left.min(offset);
            out.extend_from_within(from..from + piece);
            from += piece;
            left -= piece;
        }
    }
    census.sequences += count as u64;
    frame.repeats_carried = true;
    // §3.1.1.3.2.1.2: "At the end, the bitstream shall be entirely
    // consumed; otherwise, the bitstream is considered corrupted."
    if !bits.finished() {
        return Err(Error::BadSequences);
    }
    // Literals no sequence took end the block.
    let tail = literals.get(taken..).unwrap_or(&[]);
    room_for(out, tail.len(), limits)?;
    out.extend_from_slice(tail);
    Ok(())
}

/// §3.1.1.5: an `Offset_Value` above 3 is an offset of three less; 1 to 3
/// name a repeat offset, shifted by one when the sequence has no literals.
/// Returns the offset, which repeat it used (0 to 3, the last being the
/// first repeat minus one) if any, and updates the three.
fn resolve_offset(
    value: u64,
    literals_length: u64,
    repeats: &mut [u64; 3],
) -> Result<(u64, Option<usize>), Error> {
    let [first, second, third] = *repeats;
    if value > 3 {
        let offset = value - 3;
        *repeats = [offset, first, second];
        return Ok((offset, None));
    }
    let index = value + u64::from(literals_length == 0);
    let offset = match index {
        1 => first,
        2 => second,
        3 => third,
        _ => first.saturating_sub(1),
    };
    // "If Repeated_Offset1 - 1 evaluates to 0, then the data is considered
    // corrupted."
    if offset == 0 {
        return Err(Error::BadOffset);
    }
    match index {
        1 => {}
        2 => *repeats = [second, first, third],
        _ => *repeats = [offset, first, second],
    }
    Ok((offset, Some(index as usize - 1)))
}
