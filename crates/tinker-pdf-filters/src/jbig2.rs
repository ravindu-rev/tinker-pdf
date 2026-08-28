//! JBIG2 (ITU-T T.88): the generic-region lineage.
//!
//! Feature documentation: `docs/features/filters.md`. The reasoning behind
//! the scope is gap 17's.
//!
//! JBIG2 is two largely separate codecs sharing one container. The **generic
//! region** lineage codes a bilevel bitmap pixel by pixel, either with the MQ
//! arithmetic coder against a template of already-decoded neighbours (6.2.5)
//! or with the same MMR coding a G4 fax uses (6.2.6). That is what a scanner
//! emits. The **symbol dictionary plus text region** lineage builds a
//! dictionary of glyph bitmaps and then places them, and that is what
//! `jbig2enc` and OCRmyPDF emit — most PDFs that have been through an OCR
//! pipeline. This module decodes the first and **refuses the second by name**.
//!
//! # The refusal is the feature
//!
//! The tempting shape is to skip segments this build does not understand and
//! return whatever page came out. A file that is symbol dictionary plus text
//! region would then decode its page information segment, find no generic
//! region, and hand back a **blank white page reported as success** —
//! indistinguishable from a correct decode of a blank scan, and strictly
//! worse than the grey placeholder it replaced, which at least says something
//! is missing.
//!
//! So [`decode`] returns [`FilterError::Unsupported`] when no region was
//! composited onto the page, and the caller draws the placeholder. Ruling 2
//! degrades; it does not invent content. The corpus makes this the *common*
//! path rather than a corner: of the 103 JBIG2 files gap 23 measured, the OCR
//! lineage is the bulk.
//!
//! # Polarity
//!
//! JBIG2 is 1 = black (6.2.2). A 1-bit DeviceGray image is 0 = black. This
//! module returns **JBIG2's own sense**, unconverted, exactly as
//! [`crate::T6Rows`] does — the inversion belongs at the PDF boundary where
//! `/ImageMask` and `/Decode` are also read, not buried in a decoder that
//! would then be guessing which convention its caller wanted.
//!
//! # Allocation
//!
//! Region width and height are attacker-controlled 32-bit values, and a
//! 300 dpi A4 page is 8.7 megabytes at one byte per pixel. Every allocation
//! here goes through [`packed_size`], a checked multiply against the output
//! ceiling, **before** the allocation happens — the pattern `ccitt.rs`
//! already uses. A region declaring 2^32 pixels is refused rather than
//! attempted (ruling 1).

use std::collections::{BTreeMap, BTreeSet};

use crate::mq::{MqContexts, MqDecoder};
use crate::{Capability, FilterError, Warning};

/// Segment types (T.88 7.3, Table 34) this decoder distinguishes by name.
mod kind {
    pub const SYMBOL_DICTIONARY: u8 = 0;
    pub const INTERMEDIATE_TEXT_REGION: u8 = 4;
    pub const IMMEDIATE_TEXT_REGION: u8 = 6;
    pub const IMMEDIATE_LOSSLESS_TEXT_REGION: u8 = 7;
    pub const PATTERN_DICTIONARY: u8 = 16;
    pub const INTERMEDIATE_HALFTONE_REGION: u8 = 20;
    pub const IMMEDIATE_HALFTONE_REGION: u8 = 22;
    pub const IMMEDIATE_LOSSLESS_HALFTONE_REGION: u8 = 23;
    pub const INTERMEDIATE_GENERIC_REGION: u8 = 36;
    pub const IMMEDIATE_GENERIC_REGION: u8 = 38;
    pub const IMMEDIATE_LOSSLESS_GENERIC_REGION: u8 = 39;
    pub const INTERMEDIATE_REFINEMENT_REGION: u8 = 40;
    pub const IMMEDIATE_REFINEMENT_REGION: u8 = 42;
    pub const IMMEDIATE_LOSSLESS_REFINEMENT_REGION: u8 = 43;
    pub const PAGE_INFORMATION: u8 = 48;
    pub const END_OF_PAGE: u8 = 49;
    pub const END_OF_STRIPE: u8 = 50;
    pub const END_OF_FILE: u8 = 51;
    pub const PROFILES: u8 = 52;
    pub const TABLES: u8 = 53;
    pub const COLOUR_PALETTE: u8 = 54;
    pub const EXTENSION: u8 = 62;
}

/// The eight bytes a standalone JBIG2 file opens with (T.88 D.4.1).
///
/// A PDF stream is the *embedded* organisation (D.3) and carries none of
/// this — but producers that pasted a whole file into a stream exist, and
/// skipping a header that is there costs four lines.
const FILE_HEADER: [u8; 8] = [0x97, 0x4A, 0x42, 0x32, 0x0D, 0x0A, 0x1A, 0x0A];

/// What the stream tier knows about a JBIG2 image that the coded bytes do
/// not carry (ISO 32000-1 7.4.7).
#[derive(Clone, Copy, Debug)]
pub struct Jbig2Params<'a> {
    /// The `/JBIG2Globals` stream's bytes, already run through its own filter
    /// chain, or empty. Its segments — a shared symbol dictionary, usually —
    /// are visible to every image that names it.
    pub globals: &'a [u8],
    /// The image's declared width in pixels.
    pub width: u32,
    /// The image's declared height in pixels.
    pub height: u32,
}

/// A parsed segment header (T.88 7.2) and the data block that follows it.
///
/// The number and the referred-to list are both kept now. They were both
/// dropped while the generic-region lineage was all this decoded, because
/// nothing followed a reference — but 7.4.3 makes a text region's symbol list
/// *the concatenation of its referred-to dictionaries' exports, in reference
/// order*, and custom tables are reached the same way. Neither is unbounded:
/// the referred-to numbers already had to fit inside this header for it to be
/// a header at all.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Segment<'a> {
    /// 7.2.2, and 7.2.5's input: it decides how wide the referred-to numbers
    /// in this same header are.
    number: u32,
    /// 7.2.3, the low six bits of the header flags.
    kind: u8,
    /// 7.2.4 and 7.2.5, in the order the header gives them, which is the
    /// order 7.4.3 concatenates their exports in.
    referred: Vec<u32>,
    /// 7.2.6.
    page: u32,
    /// 7.2.7 through 7.2.8: the segment's own data.
    data: &'a [u8],
}

/// A big-endian cursor that runs out rather than panicking.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Reader<'a> {
        Reader { data, at: 0 }
    }

    fn done(&self) -> bool {
        self.at >= self.data.len()
    }

    fn u8(&mut self) -> Option<u8> {
        let b = self.data.get(self.at).copied()?;
        self.at += 1;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from(self.u8()?) << 8 | u16::from(self.u8()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from(self.u16()?) << 16 | u32::from(self.u16()?))
    }

    fn skip(&mut self, n: usize) -> Option<()> {
        self.at = self.at.checked_add(n).filter(|a| *a <= self.data.len())?;
        Some(())
    }

    /// A signed byte. The AT pixel coordinates of 7.4.6.2 are the only place
    /// this codec has one, and they are routinely negative.
    fn i8(&mut self) -> Option<i8> {
        self.u8().map(|b| b as i8)
    }

    /// Everything from the cursor on: a region segment's coded data, which
    /// runs to the end of the segment rather than carrying its own length.
    fn rest(&self) -> &'a [u8] {
        self.data.get(self.at..).unwrap_or(&[])
    }

    /// The next `n` bytes, or everything left when the segment claims more
    /// than the stream holds — which is the truncation case, reported by the
    /// caller rather than swallowed here.
    fn take(&mut self, n: usize) -> (&'a [u8], bool) {
        let end = self.at.saturating_add(n).min(self.data.len());
        let whole = end == self.at.saturating_add(n);
        let slice = self.data.get(self.at..end).unwrap_or(&[]);
        self.at = end;
        (slice, whole)
    }
}

/// Records a leniency once. Same contract as the rest of the crate: one entry
/// per condition per decode, not one per occurrence.
fn note(warnings: &mut Vec<Warning>, warning: Warning) {
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

/// Splits a stream into segments (T.88 7.2), in the order they appear.
///
/// This is the *embedded* organisation of Annex D.3, which is the only one a
/// PDF may use: every segment header is immediately followed by its own data.
/// The sequential file organisation of D.2 is the same layout with a file
/// header in front, so a whole file pasted into a stream parses too.
///
/// The random-access organisation — all headers, then all data — cannot
/// appear in a PDF and is not guessed at: it is recorded and the stream ends
/// there, because parsing it as sequential would read data blocks as headers
/// and invent segments that are not in the file.
fn segments<'a>(data: &'a [u8], warnings: &mut Vec<Warning>) -> Vec<Segment<'a>> {
    let mut reader = Reader::new(data);
    if data.starts_with(&FILE_HEADER) {
        let _ = reader.skip(FILE_HEADER.len());
        let flags = reader.u8().unwrap_or(0);
        // D.4.2 bit 0: 1 is sequential, 0 is random access. Bit 1: 0 means
        // the number of pages is known and follows as four bytes.
        if flags & 1 == 0 {
            note(warnings, Warning::Jbig2SegmentSkipped);
            return Vec::new();
        }
        if flags & 2 == 0 {
            let _ = reader.skip(4);
        }
    }

    let mut out = Vec::new();
    // A header is eleven bytes at the very least, so this cannot spin: every
    // turn either consumes bytes or breaks.
    while !reader.done() {
        let before = reader.at;
        let Some(segment) = read_segment(&mut reader, warnings) else {
            break;
        };
        if reader.at <= before {
            break;
        }
        out.push(segment);
    }
    out
}

/// One segment header and its data (T.88 7.2).
fn read_segment<'a>(reader: &mut Reader<'a>, warnings: &mut Vec<Warning>) -> Option<Segment<'a>> {
    let number = reader.u32()?;
    let flags = reader.u8()?;
    let kind = flags & 0x3F;
    // 7.2.3 bit 6: the page association field is four bytes rather than one.
    let long_page = flags & 0x40 != 0;

    // 7.2.4: the top three bits of the next byte are the count of referred-to
    // segments — unless they are all set, in which case the whole four bytes
    // are the count and a run of retain flags follows.
    let first = reader.u8()?;
    let count = if first >> 5 == 7 {
        reader.at -= 1;
        let long = reader.u32()? & 0x1FFF_FFFF;
        // ceil((count + 1) / 8) bytes of retain flags. `count` is
        // attacker-controlled, so the skip is checked against the data.
        let retain = (long as usize).checked_add(1)?.div_ceil(8);
        reader.skip(retain)?;
        long
    } else {
        u32::from(first >> 5)
    };

    // 7.2.5: how wide each referred-to segment number is, decided by *this*
    // segment's number rather than by the values being referred to.
    let width = if number <= 256 {
        1
    } else if number <= 65536 {
        2
    } else {
        4
    };
    // A count is up to 2^29, and the referred-to numbers are the only thing
    // between here and the data, so the whole run has to fit in what is left
    // or the header is not a header — which is also what bounds the vector
    // below without a cap of its own.
    let referred_bytes = (count as usize).checked_mul(width)?;
    let start = reader.at;
    reader.skip(referred_bytes)?;
    let mut referred = Vec::with_capacity(count as usize);
    for index in 0..count as usize {
        let at = start + index * width;
        let bytes = reader.data.get(at..at + width)?;
        referred.push(match width {
            1 => u32::from(bytes[0]),
            2 => u32::from(u16::from_be_bytes([bytes[0], bytes[1]])),
            _ => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        });
    }

    let page = if long_page {
        reader.u32()?
    } else {
        u32::from(reader.u8()?)
    };

    let length = reader.u32()?;
    if length == u32::MAX {
        // 7.2.7: an unknown data length is legal only for an immediate
        // generic region, and finding its end means scanning for a row
        // terminator that depends on the region's own coding. Nothing after
        // this segment can be located, so the stream ends here rather than
        // being guessed at.
        note(warnings, Warning::Jbig2SegmentSkipped);
        return None;
    }
    let (data, whole) = reader.take(length as usize);
    if !whole {
        note(warnings, Warning::TruncatedInput);
    }

    Some(Segment {
        number,
        kind,
        referred,
        page,
        data,
    })
}

/// # Annex A, ahead of its caller
///
/// The three items below are read only by this module's tests until the
/// symbol dictionary that drives them lands (milestone 3 of
/// `docs/design/jbig2-symbol-text.md`). They arrive first deliberately: their
/// round trips are what says they are right, and a decoder whose arithmetic is
/// only exercised through the thing that consumes it cannot be told apart from
/// a consumer that compensates for it.
///
/// How many contexts A.2's integer decoder keeps.
///
/// Nine bits of `PREV`, and `PREV` is held to that width by the folding in
/// [`decode_int`] rather than by the array's length — the array is sized to
/// match it so the fold is the only thing deciding, and an index can never be
/// the thing that is wrong.
#[allow(dead_code)]
const INT_CONTEXTS: usize = 512;

/// **A.2: the integer arithmetic decoding procedure.**
///
/// Reads a sign, then a prefix that says how many magnitude bits follow and
/// what to add to them. `None` is OOB — the out-of-band value A.2 spells as a
/// negative zero, which is how a symbol dictionary's height class says it has
/// ended and how a text region says a strip has.
///
/// # Why `PREV` folds rather than grows
///
/// The context for each bit is the value decoded so far, so `PREV` doubles per
/// bit and would run past the array after nine of them. A.2 folds it back
/// instead: once it reaches 256 the top bit is pinned and the rest rotate
/// under it, so the last eight bits decoded pick the context and the value
/// keeps its place in the tree. A build that let it grow would index out of
/// the array on the tenth bit of a 32-bit magnitude — which is every large
/// coordinate in a real text region, not an edge case.
#[allow(dead_code)]
fn decode_int(coder: &mut MqDecoder<'_>, cx: &mut MqContexts) -> Option<i32> {
    let mut prev = 1usize;
    let bit = |coder: &mut MqDecoder<'_>, cx: &mut MqContexts, prev: &mut usize| -> u32 {
        let d = u32::from(coder.decode_at(cx, *prev));
        // A.2 step 2: nine bits wide, top bit pinned once it is reached.
        *prev = if *prev < 256 {
            (*prev << 1) | d as usize
        } else {
            (((*prev << 1) | d as usize) & 511) | 256
        };
        d
    };

    let sign = bit(coder, cx, &mut prev);
    // The prefix is unary-ish: each 1 buys a wider field and a larger offset.
    let (width, offset) = if bit(coder, cx, &mut prev) == 0 {
        (2, 0i64)
    } else if bit(coder, cx, &mut prev) == 0 {
        (4, 4)
    } else if bit(coder, cx, &mut prev) == 0 {
        (6, 20)
    } else if bit(coder, cx, &mut prev) == 0 {
        (8, 84)
    } else if bit(coder, cx, &mut prev) == 0 {
        (12, 340)
    } else {
        (32, 4436)
    };

    let mut value = 0i64;
    for _ in 0..width {
        value = (value << 1) | i64::from(bit(coder, cx, &mut prev));
    }
    value += offset;

    // A.2 step 4: a negative zero is not a value, it is the end of something.
    if sign == 1 && value == 0 {
        return None;
    }
    let value = if sign == 1 { -value } else { value };
    // 32 magnitude bits plus the offset exceed `i32` by design; the callers
    // are coordinates and counts that a region's own bounds reject anyway, so
    // saturating here keeps the arithmetic downstream in one type.
    Some(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
}

/// **A.3: the IAID decoding procedure**, which reads a symbol's index.
///
/// Unlike A.2 this is a plain fixed-width read down a context *tree*: the
/// contexts are the prefix decoded so far, so the array is twice as wide as
/// the code length and `PREV` never needs folding. `code_len` is
/// `SBSYMCODELEN`, and it is the caller's to derive from the symbol count.
#[allow(dead_code)]
fn decode_iaid(coder: &mut MqDecoder<'_>, cx: &mut MqContexts, code_len: u32) -> u32 {
    let mut prev = 1usize;
    for _ in 0..code_len.min(31) {
        let d = usize::from(coder.decode_at(cx, prev));
        prev = (prev << 1) | d;
    }
    (prev as u32).wrapping_sub(1 << code_len.min(31))
}

/// How many contexts [`decode_iaid`] needs for a given code length.
#[allow(dead_code)]
fn iaid_contexts(code_len: u32) -> usize {
    1usize << (code_len.min(31) + 1)
}

/// 7.4.1.5's external combination operators, over one pixel.
///
/// Shared by [`Bitmap::composite`] and the clipped placement a text region
/// needs, so the two cannot come to disagree about what XNOR means.
fn combine(destination: u32, source: u32, op: u8) -> u32 {
    match op {
        1 => source & destination,
        2 => source ^ destination,
        3 => !(source ^ destination) & 1,
        4 => source,
        // 0 is OR, and so is anything 7.4.1.5 leaves undefined: a region drawn
        // with an operator nobody defined should still appear rather than
        // erase what is under it.
        _ => source | destination,
    }
}

// ---- Annex B: Huffman coding -----------------------------------------------
//
// The other half of T.88's symbol lineage. Where the arithmetic variant reads
// decisions from the MQ coder, this one reads *prefix codes* out of the
// bitstream, and the standard publishes fifteen tables of them.
//
// **Where these numbers come from, stated plainly.** The tables below are
// reconstructed rather than transcribed from a copy of T.88, and the check on
// them is the standard's own datastream: Annex H.1 codes one picture twice, as
// a Huffman page and an arithmetic page, and
// `annex_h_codes_one_picture_twice_and_both_ways_agree` requires the two
// decodes to be byte-identical over the whole page. A single wrong prefix
// length desynchronises the reader and the pages differ, so there is no
// outcome where a wrong table produces a plausible picture — which is the
// failure mode this module's refusals exist to prevent. Only the six tables
// that page reaches are here; the rest refuse by name until something needs
// them, and the reachability census in `crates/tinker-pdf/tests/jbig2_census.rs`
// is what would say when.

/// A bit reader, most significant bit first, over a segment's data.
///
/// Separate from [`Reader`], which is byte-oriented: a Huffman-coded segment
/// interleaves bit-aligned prefix codes with byte-aligned bitmaps, and the two
/// readers meet at [`BitReader::align`].
struct BitReader<'a> {
    bytes: &'a [u8],
    /// The next bit to read, counted from the start of `bytes`.
    at: usize,
}

impl<'a> BitReader<'a> {
    fn new(bytes: &'a [u8]) -> BitReader<'a> {
        BitReader { bytes, at: 0 }
    }

    /// One bit, or `None` past the end.
    fn bit(&mut self) -> Option<u32> {
        let byte = *self.bytes.get(self.at / 8)?;
        let shift = 7 - (self.at % 8);
        self.at += 1;
        Some(u32::from((byte >> shift) & 1))
    }

    /// `n` bits, most significant first. `n` above 32 is a caller error and
    /// answers `None` rather than wrapping.
    fn bits(&mut self, n: u32) -> Option<u32> {
        if n > 32 {
            return None;
        }
        let mut value = 0u32;
        for _ in 0..n {
            value = (value << 1) | self.bit()?;
        }
        Some(value)
    }

    /// Moves to the next byte boundary, which is where a collective bitmap
    /// starts (6.5.9) and where 7.4.3.1.7's symbol codes stop.
    fn align(&mut self) {
        self.at = self.at.div_ceil(8) * 8;
    }

    /// How many whole bytes have been consumed, for handing the rest to a
    /// byte-oriented decoder.
    const fn byte_position(&self) -> usize {
        self.at.div_ceil(8)
    }
}

/// What one line of an Annex B table says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HuffLine {
    /// The number of bits in this line's prefix code. Zero means the line is
    /// not present in the table at all, which is how B.3's and B.5's optional
    /// lines are spelled.
    prefix_len: u8,
    /// How many bits of offset follow the prefix. 32 marks the two open-ended
    /// lines, which is why this is not a range in the ordinary sense.
    range_len: u8,
    /// The value the offset is added to — or, for [`LineKind::Lower`],
    /// subtracted from.
    range_low: i32,
    kind: LineKind,
}

/// Which of B.2's three shapes a line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineKind {
    /// `range_low + offset`.
    Normal,
    /// `range_low - offset`: the open-ended line running downwards.
    Lower,
    /// Out of band, which carries no offset at all and ends a run.
    Oob,
}

impl HuffLine {
    const fn normal(prefix_len: u8, range_len: u8, range_low: i32) -> HuffLine {
        HuffLine {
            prefix_len,
            range_len,
            range_low,
            kind: LineKind::Normal,
        }
    }

    const fn lower(prefix_len: u8, range_low: i32) -> HuffLine {
        HuffLine {
            prefix_len,
            range_len: 32,
            range_low,
            kind: LineKind::Lower,
        }
    }

    const fn oob(prefix_len: u8) -> HuffLine {
        HuffLine {
            prefix_len,
            range_len: 0,
            range_low: 0,
            kind: LineKind::Oob,
        }
    }
}

/// One decoded value, or the out-of-band marker that ends a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HuffValue {
    Value(i32),
    Oob,
}

/// An Annex B table with its prefix codes assigned.
struct HuffTable {
    lines: Vec<HuffLine>,
    /// The prefix code of each line, in the same order.
    codes: Vec<u32>,
}

impl HuffTable {
    /// B.3's assignment procedure: canonical codes, shortest first, in table
    /// order within a length.
    ///
    /// The same construction every canonical prefix code uses, and it is
    /// written out rather than borrowed from `inflate.rs` because that one
    /// speaks RFC 1951's conventions and this one speaks B.3's — the two agree
    /// today and a shared helper would be a place for them to stop agreeing.
    fn new(lines: Vec<HuffLine>) -> HuffTable {
        let max = lines.iter().map(|l| l.prefix_len).max().unwrap_or(0);
        let mut counts = vec![0u32; usize::from(max) + 1];
        for line in &lines {
            if line.prefix_len > 0 {
                counts[usize::from(line.prefix_len)] += 1;
            }
        }
        let mut first = vec![0u32; usize::from(max) + 2];
        for len in 1..=usize::from(max) {
            first[len + 1] = (first[len] + counts[len]) << 1;
        }
        let mut next = first.clone();
        let mut codes = Vec::with_capacity(lines.len());
        for line in &lines {
            if line.prefix_len == 0 {
                codes.push(0);
                continue;
            }
            let len = usize::from(line.prefix_len);
            codes.push(next[len]);
            next[len] += 1;
        }
        HuffTable { lines, codes }
    }

    /// Reads one value, growing a candidate prefix a bit at a time.
    ///
    /// Linear in the table's length per bit, which for tables of at most
    /// twenty lines is cheaper than the structure a faster search would need.
    fn decode(&self, reader: &mut BitReader<'_>) -> Option<HuffValue> {
        let mut code = 0u32;
        let mut len = 0u8;
        while len < 32 {
            code = (code << 1) | reader.bit()?;
            len += 1;
            for (line, assigned) in self.lines.iter().zip(&self.codes) {
                if line.prefix_len != len || *assigned != code {
                    continue;
                }
                return Some(match line.kind {
                    LineKind::Oob => HuffValue::Oob,
                    LineKind::Lower => {
                        let offset = reader.bits(u32::from(line.range_len))?;
                        HuffValue::Value(line.range_low.checked_sub(offset as i32)?)
                    }
                    LineKind::Normal => {
                        let offset = reader.bits(u32::from(line.range_len))?;
                        HuffValue::Value(line.range_low.checked_add(offset as i32)?)
                    }
                });
            }
        }
        None
    }

    /// The value, refusing the out-of-band marker a caller did not expect.
    fn value(&self, reader: &mut BitReader<'_>) -> Option<i32> {
        match self.decode(reader)? {
            HuffValue::Value(v) => Some(v),
            HuffValue::Oob => None,
        }
    }
}

/// Table B.1, which counts sizes: bitmap sizes, aggregate instance counts and
/// the export runs of 6.5.10.
fn table_b1() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(1, 4, 0),
        HuffLine::normal(2, 8, 16),
        HuffLine::normal(3, 16, 272),
        HuffLine::normal(3, 32, 65_808),
    ])
}

/// Table B.2, the symbol-width deltas, which needs an out-of-band value to end
/// a height class.
fn table_b2() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(1, 0, 0),
        HuffLine::normal(2, 0, 1),
        HuffLine::normal(3, 0, 2),
        HuffLine::normal(4, 3, 3),
        HuffLine::normal(5, 6, 11),
        HuffLine::normal(6, 32, 75),
        HuffLine::oob(6),
    ])
}

/// Table B.3, the symbol-width deltas over a range that runs both ways.
fn table_b3() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(8, 8, -256),
        HuffLine::normal(1, 0, 0),
        HuffLine::normal(2, 0, 1),
        HuffLine::normal(3, 0, 2),
        HuffLine::normal(4, 3, 3),
        HuffLine::normal(5, 6, 11),
        HuffLine::lower(8, -257),
        HuffLine::normal(7, 32, 75),
        HuffLine::oob(6),
    ])
}

/// Table B.4, the height-class deltas.
fn table_b4() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(1, 0, 1),
        HuffLine::normal(2, 0, 2),
        HuffLine::normal(3, 0, 3),
        HuffLine::normal(4, 3, 4),
        HuffLine::normal(5, 6, 12),
        HuffLine::normal(5, 32, 76),
    ])
}

/// Table B.5, the height-class deltas over a range that runs both ways.
fn table_b5() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(7, 8, -255),
        HuffLine::normal(1, 0, 1),
        HuffLine::normal(2, 0, 2),
        HuffLine::normal(3, 0, 3),
        HuffLine::normal(4, 3, 4),
        HuffLine::normal(5, 6, 12),
        HuffLine::lower(7, -256),
        HuffLine::normal(6, 32, 76),
    ])
}

/// Table B.6, a text region's first-symbol coordinate, which runs both ways.
fn table_b6() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(5, 10, -2048),
        HuffLine::normal(4, 9, -1024),
        HuffLine::normal(4, 8, -512),
        HuffLine::normal(4, 7, -256),
        HuffLine::normal(5, 6, -128),
        HuffLine::normal(5, 5, -64),
        HuffLine::normal(4, 5, -32),
        HuffLine::normal(2, 7, 0),
        HuffLine::normal(3, 7, 128),
        HuffLine::normal(3, 8, 256),
        HuffLine::normal(4, 9, 512),
        HuffLine::normal(4, 10, 1024),
        HuffLine::lower(6, -2049),
        HuffLine::normal(6, 32, 2048),
    ])
}

/// Table B.7, a text region's first-symbol coordinate over a wider range.
fn table_b7() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(4, 9, -1024),
        HuffLine::normal(3, 8, -512),
        HuffLine::normal(4, 7, -256),
        HuffLine::normal(5, 6, -128),
        HuffLine::normal(5, 5, -64),
        HuffLine::normal(4, 5, -32),
        HuffLine::normal(4, 9, 0),
        HuffLine::normal(5, 10, 512),
        HuffLine::normal(3, 10, 1536),
        HuffLine::normal(6, 32, -1025),
        HuffLine::normal(5, 32, 2560),
    ])
}

/// Table B.8, the gap between symbols along a strip.
fn table_b8() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(8, 3, -15),
        HuffLine::normal(9, 1, -7),
        HuffLine::normal(8, 1, -5),
        HuffLine::normal(9, 0, -3),
        HuffLine::normal(7, 0, -2),
        HuffLine::normal(4, 0, -1),
        HuffLine::normal(2, 1, 0),
        HuffLine::normal(5, 0, 2),
        HuffLine::normal(6, 0, 3),
        HuffLine::normal(3, 4, 4),
        HuffLine::normal(6, 1, 20),
        HuffLine::normal(4, 4, 22),
        HuffLine::normal(4, 5, 38),
        HuffLine::normal(5, 6, 70),
        HuffLine::normal(5, 7, 134),
        HuffLine::normal(6, 7, 262),
        HuffLine::normal(7, 8, 390),
        HuffLine::normal(6, 10, 646),
        HuffLine::lower(9, -16),
        HuffLine::normal(9, 32, 1670),
        HuffLine::oob(2),
    ])
}

/// Table B.9, the gap between symbols at twice B.8's resolution.
fn table_b9() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(8, 4, -31),
        HuffLine::normal(9, 2, -15),
        HuffLine::normal(8, 2, -11),
        HuffLine::normal(9, 1, -7),
        HuffLine::normal(7, 1, -5),
        HuffLine::normal(4, 1, -3),
        HuffLine::normal(3, 1, -1),
        HuffLine::normal(3, 1, 1),
        HuffLine::normal(5, 1, 3),
        HuffLine::normal(6, 1, 5),
        HuffLine::normal(3, 5, 7),
        HuffLine::normal(6, 2, 39),
        HuffLine::normal(4, 5, 43),
        HuffLine::normal(4, 6, 75),
        HuffLine::normal(5, 7, 139),
        HuffLine::normal(5, 8, 267),
        HuffLine::normal(6, 8, 523),
        HuffLine::normal(7, 9, 779),
        HuffLine::normal(6, 11, 1291),
        HuffLine::lower(9, -32),
        HuffLine::normal(9, 32, 3339),
        HuffLine::oob(2),
    ])
}

/// Table B.10, the gap between symbols over the widest range.
fn table_b10() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(7, 4, -21),
        HuffLine::normal(8, 0, -5),
        HuffLine::normal(7, 0, -4),
        HuffLine::normal(5, 0, -3),
        HuffLine::normal(2, 2, -2),
        HuffLine::normal(5, 0, 2),
        HuffLine::normal(6, 0, 3),
        HuffLine::normal(7, 0, 4),
        HuffLine::normal(8, 0, 5),
        HuffLine::normal(2, 6, 6),
        HuffLine::normal(5, 5, 70),
        HuffLine::normal(6, 5, 102),
        HuffLine::normal(7, 6, 134),
        HuffLine::normal(8, 7, 198),
        HuffLine::normal(8, 8, 326),
        HuffLine::normal(8, 9, 582),
        HuffLine::normal(8, 10, 1094),
        HuffLine::normal(7, 11, 2118),
        HuffLine::lower(8, -22),
        HuffLine::normal(8, 32, 4166),
        HuffLine::oob(2),
    ])
}

/// Table B.11, a strip's vertical coordinate at the finest resolution.
fn table_b11() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(1, 0, 1),
        HuffLine::normal(2, 1, 2),
        HuffLine::normal(4, 0, 4),
        HuffLine::normal(4, 1, 5),
        HuffLine::normal(5, 1, 7),
        HuffLine::normal(5, 2, 9),
        HuffLine::normal(6, 2, 13),
        HuffLine::normal(7, 2, 17),
        HuffLine::normal(7, 3, 21),
        HuffLine::normal(7, 4, 29),
        HuffLine::normal(7, 5, 45),
        HuffLine::normal(7, 6, 77),
        HuffLine::normal(7, 32, 141),
    ])
}

/// Table B.12, a strip's vertical coordinate.
fn table_b12() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(1, 0, 0),
        HuffLine::normal(2, 1, 1),
        HuffLine::normal(4, 0, 3),
        HuffLine::normal(5, 1, 4),
        HuffLine::normal(5, 2, 6),
        HuffLine::normal(6, 3, 10),
        HuffLine::normal(7, 4, 18),
        HuffLine::normal(7, 5, 34),
        HuffLine::normal(7, 6, 66),
        HuffLine::normal(7, 32, 130),
    ])
}

/// A.1's integer decoders and 6.3's refinement states, as one bundle.
///
/// 6.5.8.2 is why this is a struct rather than a pile of locals: a symbol
/// dictionary that aggregates runs 6.4's text region procedure **over its own
/// decoder**, so every one of these has to survive from the dictionary into
/// the text region and back out to the next symbol. Handing them over as a
/// bundle is what makes that sharing hard to get wrong.
struct ArithContexts {
    iadh: MqContexts,
    iadw: MqContexts,
    iaex: MqContexts,
    iaai: MqContexts,
    iadt: MqContexts,
    iafs: MqContexts,
    iads: MqContexts,
    iait: MqContexts,
    iari: MqContexts,
    iardw: MqContexts,
    iardh: MqContexts,
    iardx: MqContexts,
    iardy: MqContexts,
    iaid: MqContexts,
    refine: MqContexts,
}

impl ArithContexts {
    /// `refine_bits` sizes 6.3's context set, which is thousands of states
    /// and dead weight for the many dictionaries and regions that never
    /// refine; zero asks for none at all.
    fn new(code_len: u32, refine_bits: usize) -> ArithContexts {
        ArithContexts {
            iadh: MqContexts::new(INT_CONTEXTS),
            iadw: MqContexts::new(INT_CONTEXTS),
            iaex: MqContexts::new(INT_CONTEXTS),
            iaai: MqContexts::new(INT_CONTEXTS),
            iadt: MqContexts::new(INT_CONTEXTS),
            iafs: MqContexts::new(INT_CONTEXTS),
            iads: MqContexts::new(INT_CONTEXTS),
            iait: MqContexts::new(INT_CONTEXTS),
            iari: MqContexts::new(INT_CONTEXTS),
            iardw: MqContexts::new(INT_CONTEXTS),
            iardh: MqContexts::new(INT_CONTEXTS),
            iardx: MqContexts::new(INT_CONTEXTS),
            iardy: MqContexts::new(INT_CONTEXTS),
            iaid: MqContexts::new(iaid_contexts(code_len)),
            refine: MqContexts::new(if refine_bits == 0 {
                0
            } else {
                1 << refine_bits
            }),
        }
    }
}

/// The most symbols one dictionary may export or decode.
///
/// `SDNUMNEWSYMS` and `SDNUMEXSYMS` are 32-bit and attacker-controlled, and
/// each new symbol is an allocation. Measured against real OCR output at
/// milestone 7 of `docs/design/jbig2-symbol-text.md`; until then it is a
/// ceiling rather than a ledger row, and it is generous enough that no page of
/// text approaches it (ruling 1).
const MAX_JBIG2_SYMBOLS: u32 = 100_000;

/// The most pixels one dictionary's symbols may occupy in total.
///
/// A per-symbol bound is not a work bound once the count branches: ten thousand
/// symbols of a thousand pixels each is a bitmap nobody asked for, and every
/// one of them is individually reasonable.
const MAX_JBIG2_SYMBOL_PIXELS: u64 = 1 << 26;

/// **Clause 6.5.9: a symbol dictionary, Huffman-coded.**
///
/// The shape of the loop is 6.5.5's — height classes, each a run of widths —
/// but the symbols are not coded individually. Each class arrives as one
/// *collective bitmap* as wide as its symbols laid side by side, and the
/// symbols are cut out of it afterwards. That is why this is a separate
/// function rather than a branch inside the arithmetic one: only the outer
/// loop is shared, and sharing it would mean threading two decoders through
/// every line of it.
fn symbol_dictionary_huffman(
    flags: u16,
    reader: &mut Reader<'_>,
    imported: &[Bitmap],
    ceiling: usize,
    warnings: &mut Vec<Warning>,
) -> Option<Vec<Bitmap>> {
    // 7.4.3.1.1 bits 2 to 7 pick the tables. A selector of 3 means the segment
    // brought its own (clause 7.4.13), which nothing here reads yet.
    let dh = match (flags >> 2) & 0x0003 {
        0 => table_b4(),
        1 => table_b5(),
        _ => {
            note(warnings, Warning::Jbig2VariantSkipped);
            return None;
        }
    };
    let dw = match (flags >> 4) & 0x0003 {
        0 => table_b2(),
        1 => table_b3(),
        _ => {
            note(warnings, Warning::Jbig2VariantSkipped);
            return None;
        }
    };
    if (flags >> 6) & 0x0001 != 0 || (flags >> 7) & 0x0001 != 0 {
        note(warnings, Warning::Jbig2VariantSkipped);
        return None;
    }
    let sizes = table_b1();

    let num_ex = reader.u32()?;
    let num_new = reader.u32()?;
    if num_new > MAX_JBIG2_SYMBOLS || num_ex > MAX_JBIG2_SYMBOLS {
        note(warnings, Warning::Jbig2SymbolLimitHit);
        return None;
    }

    let rest = reader.rest();
    let mut bits = BitReader::new(rest);
    let mut new_symbols: Vec<Bitmap> = Vec::new();
    let mut spent: u64 = 0;
    let mut height: i64 = 0;

    while (new_symbols.len() as u32) < num_new {
        height = height.checked_add(i64::from(dh.value(&mut bits)?))?;
        if height <= 0 || height > i64::from(u32::MAX) {
            note(warnings, Warning::Jbig2SymbolLimitHit);
            return None;
        }

        // The widths of this class, and nothing else: the pixels come later.
        let mut widths: Vec<u32> = Vec::new();
        let mut width: i64 = 0;
        let mut total: i64 = 0;
        // Out of band ends the height class, which is why this is a
        // `while let` over the value rather than a loop with a break.
        while let HuffValue::Value(delta) = dw.decode(&mut bits)? {
            width = width.checked_add(i64::from(delta))?;
            if width <= 0 || width > i64::from(u32::MAX) {
                note(warnings, Warning::Jbig2SymbolLimitHit);
                return None;
            }
            if (new_symbols.len() + widths.len()) as u64 >= u64::from(num_new) {
                note(warnings, Warning::Jbig2SymbolLimitHit);
                return None;
            }
            total = total.checked_add(width)?;
            spent = spent.checked_add((width as u64).checked_mul(height as u64)?)?;
            if spent > MAX_JBIG2_SYMBOL_PIXELS {
                note(warnings, Warning::Jbig2SymbolLimitHit);
                return None;
            }
            widths.push(width as u32);
        }
        if widths.is_empty() {
            continue;
        }

        // 6.5.9: the collective bitmap. `BMSIZE` of zero means it is stored
        // uncompressed, one row of `total` bits padded to a byte; anything
        // else is that many bytes of MMR, which is the same T.6 decoder a fax
        // and a generic region already use.
        let bmsize = sizes.value(&mut bits)?;
        if bmsize < 0 {
            return None;
        }
        bits.align();
        let start = bits.byte_position();
        let Some(mut collective) = Bitmap::new(total as u32, height as u32, ceiling) else {
            note(warnings, Warning::Jbig2RegionTooLarge);
            return None;
        };
        if bmsize == 0 {
            let stride = (total as usize).div_ceil(8);
            let needed = stride.checked_mul(height as usize)?;
            let raw = rest.get(start..start.checked_add(needed)?)?;
            for y in 0..height as usize {
                for x in 0..total as usize {
                    let byte = *raw.get(y * stride + x / 8)?;
                    let bit = (byte >> (7 - (x % 8))) & 1;
                    collective.set(x as u32, y as u32, u32::from(bit));
                }
            }
            bits.at = (start + needed) * 8;
        } else {
            let end = start.checked_add(bmsize as usize)?;
            let raw = rest.get(start..end)?;
            if !decode_mmr(raw, &mut collective, warnings) {
                note(warnings, Warning::Jbig2SegmentSkipped);
                return None;
            }
            bits.at = end * 8;
        }

        // And cut the class out of it, left to right.
        let mut x = 0u32;
        for w in widths {
            let Some(mut symbol) = Bitmap::new(w, height as u32, ceiling) else {
                note(warnings, Warning::Jbig2RegionTooLarge);
                return None;
            };
            for row in 0..height as u32 {
                for col in 0..w {
                    symbol.set(col, row, collective.get((x + col) as i32, row as i32));
                }
            }
            x = x.checked_add(w)?;
            new_symbols.push(symbol);
        }
    }

    // 6.5.10's export runs, over Table B.1, exactly as the arithmetic variant
    // reads them over IAEX.
    let total = imported.len().checked_add(new_symbols.len())?;
    let mut exported = Vec::new();
    let mut index = 0usize;
    let mut exporting = false;
    let mut guard = 0u32;
    while index < total {
        guard += 1;
        if guard > MAX_JBIG2_SYMBOLS {
            note(warnings, Warning::Jbig2SymbolLimitHit);
            return None;
        }
        let run = sizes.value(&mut bits)?;
        if run < 0 {
            return None;
        }
        let run = run as usize;
        if exporting {
            for offset in 0..run {
                let at = index.checked_add(offset)?;
                if at >= total {
                    break;
                }
                let symbol = if at < imported.len() {
                    imported.get(at)?.clone()
                } else {
                    new_symbols.get(at - imported.len())?.clone()
                };
                exported.push(symbol);
            }
        }
        index = index.checked_add(run)?;
        exporting = !exporting;
    }

    if exported.len() as u32 != num_ex {
        note(warnings, Warning::Jbig2SymbolLimitHit);
        return None;
    }
    Some(exported)
}

/// **Clause 6.5: a symbol dictionary**, arithmetic, without refinement.
///
/// Returns the symbols the dictionary *exports* (6.5.10), which is a selection
/// over its imported symbols followed by its new ones — not the new ones alone.
/// A dictionary that re-exports what it imported is ordinary, and a text region
/// numbers its symbols across the whole exported run.
///
/// `None` is the refusal, and the caller turns it into the named warning. What
/// is refused here rather than decoded: the Huffman variant (SDHUFF), the
/// refinement and aggregate variant (SDREFAGG), and a dictionary that consumes
/// a retained context from another segment — all three are later milestones,
/// and all three are counted in the corpus census that scheduled them.
fn symbol_dictionary(
    segment: &Segment<'_>,
    imported: &[Bitmap],
    ceiling: usize,
    warnings: &mut Vec<Warning>,
) -> Option<Vec<Bitmap>> {
    let mut reader = Reader::new(segment.data);
    // 7.4.3.1.1.
    let flags = reader.u16()?;
    let huff = flags & 0x0001 != 0;
    let refagg = flags & 0x0002 != 0;
    let context_used = flags & 0x0100 != 0;
    let template = ((flags >> 10) & 0x0003) as u8;
    let rtemplate = ((flags >> 12) & 0x0001) as u8;

    if context_used || (refagg && huff) {
        // Named rather than lumped in with "a segment type this build does not
        // decode": these are variants of a segment it *does* decode, and the
        // difference is what tells a file that needs one lineage from a file
        // that needs another. Refinement itself is decoded now; what is
        // refused here is its Huffman road, which codes each refinement's
        // length in a field this decoder does not read.
        note(warnings, Warning::Jbig2VariantSkipped);
        return None;
    }
    if huff {
        // 6.5.9: the Huffman variant does not code symbols one at a time. A
        // whole height class arrives as one *collective* bitmap and the
        // symbols are sliced out of it by the widths just read, so it is a
        // different loop rather than a different decoder inside the same one.
        return symbol_dictionary_huffman(flags, &mut reader, imported, ceiling, warnings);
    }

    // 7.4.3.1.2: four AT pairs for template 0, one for the others. Reading the
    // wrong number puts the coded data at the wrong offset, so this decodes as
    // noise rather than as a slightly wrong picture.
    let mut at = NOMINAL_AT[template as usize];
    let pairs = if template == 0 { 4 } else { 1 };
    for slot in at.iter_mut().take(pairs) {
        let (Some(dx), Some(dy)) = (reader.i8(), reader.i8()) else {
            note(warnings, Warning::TruncatedInput);
            return None;
        };
        *slot = (dx, dy);
    }

    // 7.4.3.1.3: and the refinement pair after them, present only for an
    // aggregating dictionary at template 0. One more offset that has to be
    // right before the coded data begins.
    let mut refine_at = NOMINAL_REFINE_AT;
    if refagg && rtemplate == 0 {
        for slot in &mut refine_at {
            let (Some(dx), Some(dy)) = (reader.i8(), reader.i8()) else {
                note(warnings, Warning::TruncatedInput);
                return None;
            };
            *slot = (dx, dy);
        }
    }

    // 7.4.3.1.4 and 7.4.3.1.5.
    let num_ex = reader.u32()?;
    let num_new = reader.u32()?;
    if num_new > MAX_JBIG2_SYMBOLS || num_ex > MAX_JBIG2_SYMBOLS {
        note(warnings, Warning::Jbig2SymbolLimitHit);
        return None;
    }

    let mut coder = MqDecoder::new(reader.rest());
    let mut generic = MqContexts::new(1 << template_bits(template));
    // 6.5.8.2.3: the symbol code is as wide as the *whole* dictionary needs —
    // imported and new together — and not as wide as the symbols decoded so
    // far. One bit too few decodes the first aggregate symbol correctly and
    // every value after it as noise, which is a failure that looks like a
    // wrong refinement template rather than like a wrong count.
    let code_len = symbol_code_length(imported.len().checked_add(num_new as usize)?);
    let refine_layout = refagg.then(|| refine_template(rtemplate, refine_at));
    let refine_bits = refine_layout.as_ref().map_or(0, RefineTemplate::bits);
    let mut cx = ArithContexts::new(code_len, refine_bits);

    // 6.5: imported and new symbols share one index space, so they share one
    // vector. `pool[..base]` is what came in and the rest is what this
    // dictionary decoded, which is also the order 6.5.10 exports in — and it
    // is the array 6.5.8.2 refines against, which is why it has to be one.
    let base = imported.len();
    let mut pool: Vec<Bitmap> = imported.to_vec();
    let mut spent: u64 = 0;
    // 6.5.5: symbols arrive in height classes, each taller than the last.
    let mut height: i64 = 0;
    while ((pool.len() - base) as u32) < num_new {
        let delta = decode_int(&mut coder, &mut cx.iadh)?;
        height = height.checked_add(i64::from(delta))?;
        if height <= 0 || height > i64::from(u32::MAX) {
            note(warnings, Warning::Jbig2SymbolLimitHit);
            return None;
        }

        // Within a class the widths accumulate too, and OOB ends the class.
        let mut width: i64 = 0;
        // The `None` here is OOB — a value the format defines to end the
        // height class — rather than the reader running out of anything.
        while let Some(delta) = decode_int(&mut coder, &mut cx.iadw) {
            width = width.checked_add(i64::from(delta))?;
            if width <= 0 || width > i64::from(u32::MAX) {
                note(warnings, Warning::Jbig2SymbolLimitHit);
                return None;
            }
            if ((pool.len() - base) as u32) >= num_new {
                // More symbols than the header promised. The header is what
                // sized everything downstream, so this is a broken stream
                // rather than a longer dictionary.
                note(warnings, Warning::Jbig2SymbolLimitHit);
                return None;
            }

            spent = spent.checked_add((width as u64).checked_mul(height as u64)?)?;
            if spent > MAX_JBIG2_SYMBOL_PIXELS {
                note(warnings, Warning::Jbig2SymbolLimitHit);
                return None;
            }
            let Some(mut symbol) = Bitmap::new(width as u32, height as u32, ceiling) else {
                note(warnings, Warning::Jbig2RegionTooLarge);
                return None;
            };

            if refagg {
                // 6.5.8.2: the symbol is built out of symbols already known
                // rather than coded from nothing.
                let instances = decode_int(&mut coder, &mut cx.iaai)?;
                if instances <= 0 || instances as u32 > MAX_JBIG2_TEXT_INSTANCES {
                    note(warnings, Warning::Jbig2SymbolLimitHit);
                    return None;
                }
                if instances == 1 {
                    // 6.5.8.2.2: a single instance is a plain refinement, and
                    // its three values come straight off the dictionary's own
                    // decoders rather than through 6.4's strip loop.
                    let id = decode_iaid(&mut coder, &mut cx.iaid, code_len) as usize;
                    let rdx = decode_int(&mut coder, &mut cx.iardx)?;
                    let rdy = decode_int(&mut coder, &mut cx.iardy)?;
                    let Some(reference) = pool.get(id) else {
                        note(warnings, Warning::Jbig2SymbolLimitHit);
                        return None;
                    };
                    let dx = refinement_offset(width, reference.width, rdx);
                    let dy = refinement_offset(height, reference.height, rdy);
                    decode_refinement_into(
                        &mut coder,
                        &mut cx.refine,
                        refine_layout.as_ref()?,
                        false,
                        reference,
                        (i32::try_from(dx).ok()?, i32::try_from(dy).ok()?),
                        &mut symbol,
                    );
                } else {
                    // 6.5.8.2.1: more than one and the symbol is a text region
                    // in its own right — one strip tall, top-left cornered,
                    // OR-composited, over this same decoder.
                    let params = TextParams {
                        symbols: &pool,
                        instances: instances as u32,
                        strips: 1,
                        log_strips: 0,
                        corner: corner::TOPLEFT,
                        comb_op: 0,
                        ds_offset: 0,
                        refine: Some(refine_template(rtemplate, refine_at)),
                        code_len,
                    };
                    text_region_procedure(
                        &params,
                        &mut coder,
                        &mut cx,
                        &mut None,
                        ceiling,
                        &mut symbol,
                        warnings,
                    )?;
                }
            } else {
                // 6.5.8.1: the generic procedure, over the dictionary's own
                // coder and context set. TPGDON is off for a symbol — 6.5.8.1
                // says so, and a symbol is too short for it to pay anyway.
                decode_generic_into(&mut coder, &mut generic, template, false, &at, &mut symbol);
            }
            pool.push(symbol);
        }
    }

    // 6.5.10: the export flags are run lengths over the pool — the imported
    // symbols followed by the new ones — alternating between runs that are not
    // exported and runs that are, starting with the former.
    let total = pool.len();
    let mut exported = Vec::new();
    let mut index = 0usize;
    let mut exporting = false;
    while index < total {
        let run = decode_int(&mut coder, &mut cx.iaex)?;
        if run < 0 {
            return None;
        }
        let run = run as usize;
        if exporting {
            for offset in 0..run {
                let at = index.checked_add(offset)?;
                if at >= total {
                    break;
                }
                exported.push(pool.get(at)?.clone());
            }
        }
        index = index.checked_add(run)?;
        exporting = !exporting;
        if run == 0 && index == 0 && exported.is_empty() && !exporting {
            // A pair of zero-length runs makes no progress and would spin.
            break;
        }
    }

    if exported.len() as u32 != num_ex {
        // The count the header promised is what a text region will index
        // against, so a disagreement is not a smaller dictionary.
        note(warnings, Warning::Jbig2SymbolLimitHit);
        return None;
    }
    Some(exported)
}

/// The most symbol instances one text region may place.
///
/// `SBNUMINSTANCES` is 32-bit and each instance is a composite over the region,
/// so the count is work rather than memory and a per-instance bound would not
/// bound it.
const MAX_JBIG2_TEXT_INSTANCES: u32 = 1 << 22;

/// The tables a Huffman text region reads its coordinates through (7.4.4.1.2).
struct TextTables {
    fs: HuffTable,
    ds: HuffTable,
    dt: HuffTable,
}

impl TextTables {
    /// Picks them from the selector field, refusing the custom-table settings
    /// clause 7.4.13 defines and nothing here reads yet.
    fn select(selectors: u16, warnings: &mut Vec<Warning>) -> Option<TextTables> {
        let refuse = |warnings: &mut Vec<Warning>| {
            note(warnings, Warning::Jbig2VariantSkipped);
            None
        };
        let fs = match selectors & 0x0003 {
            0 => table_b6(),
            1 => table_b7(),
            _ => return refuse(warnings),
        };
        let ds = match (selectors >> 2) & 0x0003 {
            0 => table_b8(),
            1 => table_b9(),
            2 => table_b10(),
            _ => return refuse(warnings),
        };
        let dt = match (selectors >> 4) & 0x0003 {
            0 => table_b11(),
            1 => table_b12(),
            2 => table_b13(),
            _ => return refuse(warnings),
        };
        Some(TextTables { fs, ds, dt })
    }
}

/// 7.4.3.1.7: the symbol-ID code lengths, themselves run-length coded.
///
/// Thirty-five four-bit lengths build a *runcode* table; that table then reads
/// one length per symbol, with three of its values meaning "repeat" rather
/// than naming a length. A table of codes for reading a table of codes, which
/// is what makes this the fiddliest field in the format.
fn symbol_id_codes(bits: &mut BitReader<'_>, symbols: usize) -> Option<HuffTable> {
    let mut runcodes = Vec::with_capacity(35);
    for index in 0..35u8 {
        let length = bits.bits(4)? as u8;
        runcodes.push(HuffLine::normal(length, 0, i32::from(index)));
    }
    let runcode = HuffTable::new(runcodes);

    let mut lengths: Vec<u8> = Vec::with_capacity(symbols);
    let mut previous = 0u8;
    while lengths.len() < symbols {
        let code = runcode.value(bits)?;
        match code {
            0..=31 => {
                previous = code as u8;
                lengths.push(previous);
            }
            32 => {
                // Repeat the last length, three to six times.
                let repeat = 3 + bits.bits(2)?;
                for _ in 0..repeat {
                    if lengths.len() >= symbols {
                        break;
                    }
                    lengths.push(previous);
                }
            }
            33 => {
                let repeat = 3 + bits.bits(3)?;
                for _ in 0..repeat {
                    if lengths.len() >= symbols {
                        break;
                    }
                    lengths.push(0);
                }
            }
            34 => {
                let repeat = 11 + bits.bits(7)?;
                for _ in 0..repeat {
                    if lengths.len() >= symbols {
                        break;
                    }
                    lengths.push(0);
                }
            }
            _ => return None,
        }
    }

    // 7.4.3.1.7: the region's own data starts on the next byte boundary.
    bits.align();
    Some(HuffTable::new(
        lengths
            .into_iter()
            .enumerate()
            .map(|(index, length)| HuffLine::normal(length, 0, index as i32))
            .collect(),
    ))
}

/// 7.4.4.1.1's REFCORNER values.
mod corner {
    pub const TOPLEFT: u8 = 1;
    pub const TOPRIGHT: u8 = 3;
}

/// Table B.13, a strip's vertical coordinate at the coarsest resolution.
fn table_b13() -> HuffTable {
    HuffTable::new(vec![
        HuffLine::normal(1, 0, 1),
        HuffLine::normal(3, 0, 2),
        HuffLine::normal(4, 0, 3),
        HuffLine::normal(5, 0, 4),
        HuffLine::normal(4, 1, 5),
        HuffLine::normal(3, 3, 7),
        HuffLine::normal(6, 1, 15),
        HuffLine::normal(6, 2, 17),
        HuffLine::normal(6, 3, 21),
        HuffLine::normal(6, 4, 29),
        HuffLine::normal(6, 5, 45),
        HuffLine::normal(7, 6, 77),
        HuffLine::normal(7, 32, 141),
    ])
}

/// **Clause 6.4: a text region**, arithmetic, without refinement.
///
/// Symbols arrive in *strips*: a vertical coordinate shared by a run of them,
/// then along each strip a horizontal coordinate that accumulates, ended by the
/// out-of-band value. Both coordinates are deltas all the way down, so a single
/// misread leaves everything after it displaced rather than absent — which is
/// why the corpus census measured which of these knobs real files use before
/// any of this was written. Fifty-five of the corpus's fifty-eight text regions
/// use more than one strip.
///
/// # Where a symbol goes
///
/// `REFCORNER` names which corner of the symbol its coordinate refers to, and
/// the useful consequence is that **the horizontal placement does not depend on
/// it**. 6.4.5 advances the running coordinate past the symbol's width *before*
/// drawing for the two right-hand corners and *after* drawing for the two
/// left-hand ones, so the symbol's left edge is the value the coordinate held on
/// entry either way, and it ends at the symbol's far edge either way. The corner
/// decides only whether the other coordinate names the top of the symbol or its
/// bottom.
/// The parameters 6.4's procedure runs on, once its caller has worked out
/// where they come from.
///
/// A text region segment reads them from its header; an aggregate symbol
/// (6.5.8.2.1) has them fixed by the clause instead. Naming them in one place
/// is what lets the strip loop below be 6.4.5 exactly once.
struct TextParams<'a> {
    symbols: &'a [Bitmap],
    instances: u32,
    strips: i64,
    log_strips: u32,
    corner: u8,
    comb_op: u8,
    ds_offset: i32,
    refine: Option<RefineTemplate<'a>>,
    code_len: u32,
}

/// The Huffman road's state: the coordinate tables, the symbol-ID code, and
/// the bit reader all three share.
type TextHuffman<'a> = Option<(TextTables, HuffTable, BitReader<'a>)>;

/// **T.88 6.4: the text region decoding procedure**, over a coder, contexts
/// and output bitmap the caller owns.
///
/// Split out of [`text_region`] because 6.5.8.2.1 runs exactly this inside a
/// symbol dictionary: an aggregate symbol *is* a text region, decoded over the
/// dictionary's own arithmetic decoder onto a bitmap the size of the symbol.
/// Sharing the decoder is not an optimisation — the adaptive state a symbol
/// leaves behind is the state the next one is coded against, so a second
/// decoder here would decode the first aggregate correctly and then noise.
fn text_region_procedure(
    params: &TextParams<'_>,
    coder: &mut MqDecoder<'_>,
    cx: &mut ArithContexts,
    huffman: &mut TextHuffman<'_>,
    ceiling: usize,
    region: &mut Bitmap,
    warnings: &mut Vec<Warning>,
) -> Option<()> {
    // The two roads, each closing over its own reader. Everything below asks
    // these rather than either decoder, so the strip loop is 6.4.5 once.
    macro_rules! read_dt {
        () => {
            match huffman.as_mut() {
                Some((tables, _, bits)) => tables.dt.value(bits)?,
                None => decode_int(coder, &mut cx.iadt)?,
            }
        };
    }
    macro_rules! read_fs {
        () => {
            match huffman.as_mut() {
                Some((tables, _, bits)) => tables.fs.value(bits)?,
                None => decode_int(coder, &mut cx.iafs)?,
            }
        };
    }

    // 6.4.5 step 1: the first strip coordinate is the negative of what is
    // coded, which is what lets a region's first strip begin above its origin.
    let mut strip_t = -i64::from(read_dt!()) * params.strips;
    let mut first_s: i64 = 0;
    let mut placed = 0u32;

    while placed < params.instances {
        let delta = read_dt!();
        strip_t = strip_t.checked_add(i64::from(delta).checked_mul(params.strips)?)?;

        // A strip's first symbol is placed relative to the previous strip's
        // first, not to the previous symbol.
        first_s = first_s.checked_add(i64::from(read_fs!()))?;
        let mut cur_s = first_s;
        let mut first = true;

        loop {
            if !first {
                // OOB ends the strip. Anything else is the gap to the next
                // symbol, measured from the far edge of the last one.
                let gap = match huffman.as_mut() {
                    Some((tables, _, bits)) => match tables.ds.decode(bits)? {
                        HuffValue::Value(gap) => gap,
                        HuffValue::Oob => break,
                    },
                    None => match decode_int(coder, &mut cx.iads) {
                        Some(gap) => gap,
                        None => break,
                    },
                };
                cur_s = cur_s
                    .checked_add(i64::from(gap))?
                    .checked_add(i64::from(params.ds_offset))?;
            }
            first = false;
            if placed >= params.instances {
                // More instances than the header promised, which is what sized
                // the work; a longer region is a broken stream.
                note(warnings, Warning::Jbig2SymbolLimitHit);
                return None;
            }

            let cur_t = if params.strips == 1 {
                0
            } else {
                match huffman.as_mut() {
                    // 6.4.5: with Huffman the strip offset is a plain field of
                    // `log2(SBSTRIPS)` bits, not a table lookup — the only
                    // coordinate in the region that is read the same way twice.
                    Some((_, _, bits)) => i64::from(bits.bits(params.log_strips)?),
                    None => i64::from(decode_int(coder, &mut cx.iait)?),
                }
            };
            let t = strip_t.checked_add(cur_t)?;
            let id = match huffman.as_mut() {
                Some((_, codes, bits)) => codes.value(bits)?.max(0) as usize,
                None => decode_iaid(coder, &mut cx.iaid, params.code_len) as usize,
            };
            // A code the dictionary does not define is a damaged stream rather
            // than a reason to stop: the last symbol stands in, which keeps the
            // strip's coordinates advancing by a plausible width.
            let symbol = params.symbols.get(id).or_else(|| params.symbols.last())?;

            // 6.4.11: with SBREFINE an instance may be a refinement of the
            // symbol rather than the symbol itself, sized by its own deltas.
            let refined;
            let symbol = if let Some(template) = params.refine.as_ref() {
                let ri = decode_int(coder, &mut cx.iari)?;
                if ri == 0 {
                    symbol
                } else {
                    let rdw = i64::from(decode_int(coder, &mut cx.iardw)?);
                    let rdh = i64::from(decode_int(coder, &mut cx.iardh)?);
                    let rdx = decode_int(coder, &mut cx.iardx)?;
                    let rdy = decode_int(coder, &mut cx.iardy)?;
                    let width = i64::from(symbol.width).checked_add(rdw)?;
                    let height = i64::from(symbol.height).checked_add(rdh)?;
                    if width <= 0 || height <= 0 || width > i64::from(u32::MAX) {
                        note(warnings, Warning::Jbig2SymbolLimitHit);
                        return None;
                    }
                    if height > i64::from(u32::MAX) {
                        note(warnings, Warning::Jbig2SymbolLimitHit);
                        return None;
                    }
                    let Some(mut target) = Bitmap::new(width as u32, height as u32, ceiling) else {
                        note(warnings, Warning::Jbig2RegionTooLarge);
                        return None;
                    };
                    let dx = refinement_offset(width, symbol.width, rdx);
                    let dy = refinement_offset(height, symbol.height, rdy);
                    decode_refinement_into(
                        coder,
                        &mut cx.refine,
                        template,
                        false,
                        symbol,
                        (i32::try_from(dx).ok()?, i32::try_from(dy).ok()?),
                        &mut target,
                    );
                    refined = target;
                    &refined
                }
            } else {
                symbol
            };

            let width = i64::from(symbol.width);
            let height = i64::from(symbol.height);
            let x = cur_s;
            let y = if params.corner == corner::TOPLEFT || params.corner == corner::TOPRIGHT {
                t
            } else {
                t.checked_sub(height - 1)?
            };
            composite_signed(region, symbol, x, y, params.comb_op);

            cur_s = cur_s.checked_add(width - 1)?;
            placed += 1;
        }
    }

    Some(())
}

fn text_region(
    segment: &Segment<'_>,
    symbols: &[Bitmap],
    ceiling: usize,
    warnings: &mut Vec<Warning>,
) -> Option<(RegionInfo, Bitmap)> {
    let mut reader = Reader::new(segment.data);
    let info = RegionInfo::read(&mut reader)?;
    // 7.4.4.1.1.
    let flags = reader.u16()?;
    let huff = flags & 0x0001 != 0;
    let refine = flags & 0x0002 != 0;
    let log_strips = u32::from((flags >> 2) & 0x0003);
    let corner = ((flags >> 4) & 0x0003) as u8;
    let transposed = flags & 0x0040 != 0;
    let comb_op = ((flags >> 7) & 0x0003) as u8;
    let default_pixel = flags & 0x0200 != 0;
    // Bits 10 to 14 are a signed five-bit field.
    let ds_offset = {
        let raw = i32::from((flags >> 10) & 0x001F);
        if raw > 15 {
            raw - 32
        } else {
            raw
        }
    };
    let rtemplate = ((flags >> 15) & 0x0001) as u8;

    if transposed || (refine && huff) {
        // Transposed is scheduled and was counted at four files. The Huffman
        // road of refinement is narrower: it codes each refinement's length in
        // a field this decoder does not read.
        note(warnings, Warning::Jbig2VariantSkipped);
        return None;
    }

    // 7.4.4.1.2 sits *before* 7.4.4.5, and reading them the other way round
    // makes the instance count the two flag bytes followed by half of itself.
    let selectors = if huff { Some(reader.u16()?) } else { None };

    // 7.4.4.1.3 sits between them: the refinement AT pair, present only for a
    // refining region at template 0.
    let mut rat = NOMINAL_REFINE_AT;
    if refine && rtemplate == 0 {
        for slot in &mut rat {
            let (Some(dx), Some(dy)) = (reader.i8(), reader.i8()) else {
                note(warnings, Warning::TruncatedInput);
                return None;
            };
            *slot = (dx, dy);
        }
    }

    // 7.4.4.5.
    let instances = reader.u32()?;
    if instances > MAX_JBIG2_TEXT_INSTANCES {
        note(warnings, Warning::Jbig2SymbolLimitHit);
        return None;
    }
    if symbols.is_empty() {
        // Every instance names a symbol; with no dictionary behind it there is
        // nothing to place, and an empty region is not a region.
        note(warnings, Warning::Jbig2SegmentSkipped);
        return None;
    }

    let code_len = symbol_code_length(symbols.len());
    let strips = 1i64 << log_strips;

    let Some(mut region) = Bitmap::new(info.width, info.height, ceiling) else {
        note(warnings, Warning::Jbig2RegionTooLarge);
        return None;
    };
    if default_pixel {
        region.fill_black();
    }

    // 7.4.4.1.2: a Huffman region names its tables in a second flags field,
    // then carries the symbol-ID code lengths of 7.4.3.1.7 before its data.
    let mut huffman = None;
    if let Some(selectors) = selectors {
        let tables = TextTables::select(selectors, warnings)?;
        let mut bits = BitReader::new(reader.rest());
        let Some(symbol_codes) = symbol_id_codes(&mut bits, symbols.len()) else {
            note(warnings, Warning::TruncatedInput);
            return None;
        };
        huffman = Some((tables, symbol_codes, bits));
    }

    let template = refine.then(|| refine_template(rtemplate, rat));
    let refine_bits = template.as_ref().map_or(0, RefineTemplate::bits);

    let mut coder = MqDecoder::new(reader.rest());
    let mut cx = ArithContexts::new(code_len, refine_bits);
    let params = TextParams {
        symbols,
        instances,
        strips,
        log_strips,
        corner,
        comb_op,
        ds_offset,
        refine: template,
        code_len,
    };
    text_region_procedure(
        &params,
        &mut coder,
        &mut cx,
        &mut huffman,
        ceiling,
        &mut region,
        warnings,
    )?;

    Some((info, region))
}

/// 6.4.5's symbol code width: as many bits as the symbol count needs.
fn symbol_code_length(count: usize) -> u32 {
    let mut bits = 0u32;
    while bits < 31 && (1usize << bits) < count {
        bits += 1;
    }
    bits
}

/// [`Bitmap::composite`] at a coordinate that may be negative.
///
/// A symbol placed above or left of the region's origin is ordinary — 6.4.5's
/// first strip coordinate is explicitly the negative of what is coded — and the
/// part that falls outside is clipped rather than refused.
fn composite_signed(into: &mut Bitmap, source: &Bitmap, x: i64, y: i64, op: u8) {
    if x >= i64::from(into.width) || y >= i64::from(into.height) {
        return;
    }
    if x >= 0 && y >= 0 {
        into.composite(source, x as u32, y as u32, op);
        return;
    }
    // The clipped case, pixel by pixel: `Bitmap::composite` takes an unsigned
    // origin, and shifting the source instead would need a second bitmap.
    for row in 0..source.height {
        let Some(ty) = y.checked_add(i64::from(row)) else {
            continue;
        };
        if ty < 0 || ty >= i64::from(into.height) {
            continue;
        }
        for col in 0..source.width {
            let Some(tx) = x.checked_add(i64::from(col)) else {
                continue;
            };
            if tx < 0 || tx >= i64::from(into.width) {
                continue;
            }
            let value = source.get(col as i32, row as i32);
            let existing = into.get(tx as i32, ty as i32);
            into.set(tx as u32, ty as u32, combine(existing, value, op));
        }
    }
}

/// Whether a segment type is one this build decodes.
///
/// Everything else is skipped **and recorded**, which is what keeps the
/// refusal honest: the skip is not silent, and it is not sufficient on its
/// own — a page that ends with no region on it refuses regardless.
fn understood(kind: u8) -> bool {
    matches!(
        kind,
        kind::SYMBOL_DICTIONARY
            | kind::INTERMEDIATE_TEXT_REGION
            | kind::IMMEDIATE_TEXT_REGION
            | kind::IMMEDIATE_LOSSLESS_TEXT_REGION
            | kind::IMMEDIATE_GENERIC_REGION
            | kind::IMMEDIATE_LOSSLESS_GENERIC_REGION
            | kind::IMMEDIATE_REFINEMENT_REGION
            | kind::IMMEDIATE_LOSSLESS_REFINEMENT_REGION
            | kind::INTERMEDIATE_GENERIC_REGION
            | kind::INTERMEDIATE_REFINEMENT_REGION
            | kind::PAGE_INFORMATION
            | kind::END_OF_PAGE
            | kind::END_OF_STRIPE
            | kind::END_OF_FILE
            | kind::PROFILES
            | kind::EXTENSION
    )
}

/// Whether a segment carries content this build cannot reproduce.
///
/// Distinguished from [`understood`] because the two answer different
/// questions. An extension segment is skippable by design (7.4.14 makes them
/// optional unless a necessity bit says otherwise); a text region is a
/// picture that will be missing from the page. Only the second kind is worth
/// a warning naming a lineage.
///
/// An **intermediate** generic region (type 36) is here rather than in
/// [`understood`] even though its bits decode perfectly well. 7.4.6.1 says an
/// intermediate result goes to an auxiliary buffer for a later segment to
/// refer to, and the only thing that refers to one is a refinement region,
/// which this build refuses. Compositing it onto the page would draw a
/// working buffer as if it were finished content.
fn carries_content(kind: u8) -> bool {
    matches!(
        kind,
        kind::PATTERN_DICTIONARY
            | kind::INTERMEDIATE_HALFTONE_REGION
            | kind::IMMEDIATE_HALFTONE_REGION
            | kind::IMMEDIATE_LOSSLESS_HALFTONE_REGION
            | kind::TABLES
            | kind::COLOUR_PALETTE
    )
}

/// Bytes a packed 1-bpp bitmap of these dimensions occupies, or `None` if it
/// would exceed `ceiling`.
///
/// **Checked before the allocation, not after it.** `width` and `height` come
/// straight off the wire, so `width.div_ceil(8) * height` overflows a 32-bit
/// `usize` for perfectly ordinary-looking garbage and allocates a hundred
/// gigabytes on a 64-bit one. Ruling 1 wants the arithmetic bounded, and the
/// only way to bound it is to do it in `usize` with `checked_mul` before
/// anything reserves memory.
fn packed_size(width: u32, height: u32, ceiling: usize) -> Option<usize> {
    if width == 0 || height == 0 {
        return None;
    }
    let stride = (width as usize).div_ceil(8);
    let bytes = stride.checked_mul(height as usize)?;
    (bytes <= ceiling).then_some(bytes)
}

/// A bilevel bitmap: packed one bit per pixel, most significant bit first,
/// **1 = black** (T.88 6.2.2).
///
/// The page is one of these and every region decodes into another, which is
/// what makes 6.2.2's composition a single operation rather than a special
/// case per caller.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Bitmap {
    bits: Vec<u8>,
    width: u32,
    height: u32,
    stride: usize,
}

impl Bitmap {
    /// An all-white bitmap, or `None` if it would exceed `ceiling` — the
    /// checked multiply of [`packed_size`], before any allocation.
    /// Paints every pixel black (1 in JBIG2's sense, 6.2.2).
    ///
    /// 7.4.4.1.1's default pixel value: a text region may start from a black
    /// page and knock symbols out of it.
    fn fill_black(&mut self) {
        self.bits.iter_mut().for_each(|byte| *byte = 0xFF);
    }

    fn new(width: u32, height: u32, ceiling: usize) -> Option<Bitmap> {
        let bytes = packed_size(width, height, ceiling)?;
        Some(Bitmap {
            bits: vec![0u8; bytes],
            width,
            height,
            stride: (width as usize).div_ceil(8),
        })
    }

    /// The pixel at `(x, y)`, or 0 outside.
    ///
    /// 6.2.5.2: a template reaches above the first row and to either side of
    /// every row, and every pixel it reaches outside the region is 0. That
    /// rule is why this takes signed coordinates and answers rather than
    /// refusing — the out-of-range case is the *common* one on row zero, not
    /// an error.
    fn get(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return 0;
        }
        let at = (y as usize) * self.stride + (x as usize >> 3);
        let byte = self.bits.get(at).copied().unwrap_or(0);
        u32::from((byte >> (7 - (x as usize & 7))) & 1)
    }

    fn set(&mut self, x: u32, y: u32, value: u32) {
        let at = (y as usize) * self.stride + (x as usize >> 3);
        let mask = 0x80u8 >> (x as usize & 7);
        if let Some(byte) = self.bits.get_mut(at) {
            if value & 1 != 0 {
                *byte |= mask;
            } else {
                *byte &= !mask;
            }
        }
    }

    /// Copies row `from` over row `to`. TPGDON's whole purpose (6.2.5.7).
    fn copy_row(&mut self, from: u32, to: u32) {
        let (Some(f), Some(t)) = (
            (from as usize).checked_mul(self.stride),
            (to as usize).checked_mul(self.stride),
        ) else {
            return;
        };
        if f + self.stride <= self.bits.len() && t + self.stride <= self.bits.len() {
            self.bits.copy_within(f..f + self.stride, t);
        }
    }

    /// Composites `source` at `(x, y)` under one of 7.4.1.5's external
    /// combination operators, clipped to this bitmap.
    ///
    /// A region whose placement puts it partly off the page is not an error —
    /// a striped page composites regions that overhang by design — so the
    /// clip is silent.
    /// The rectangle at `(x, y)`, lifted out as its own bitmap.
    ///
    /// 6.3.2's reference for a refinement region that refers to no
    /// intermediate one: whatever the page already holds under the region's
    /// own box. Anything outside the page reads 0, which is what [`Bitmap::get`]
    /// already answers, so a region hanging off an edge is ordinary.
    fn window(&self, x: u32, y: u32, width: u32, height: u32, ceiling: usize) -> Option<Bitmap> {
        let mut out = Bitmap::new(width, height, ceiling)?;
        for row in 0..height {
            for col in 0..width {
                let (Ok(sx), Ok(sy)) = (
                    i32::try_from(u64::from(x) + u64::from(col)),
                    i32::try_from(u64::from(y) + u64::from(row)),
                ) else {
                    continue;
                };
                out.set(col, row, self.get(sx, sy));
            }
        }
        Some(out)
    }

    fn composite(&mut self, source: &Bitmap, x: u32, y: u32, op: u8) {
        for sy in 0..source.height {
            let Some(dy) = y.checked_add(sy) else { return };
            if dy >= self.height {
                return;
            }
            for sx in 0..source.width {
                let Some(dx) = x.checked_add(sx) else { break };
                if dx >= self.width {
                    break;
                }
                let s = source.get(sx as i32, sy as i32);
                let d = self.get(dx as i32, dy as i32);
                self.set(dx, dy, combine(d, s, op));
            }
        }
    }
}

/// A region segment information field (T.88 7.4.1): seventeen bytes that
/// every region segment, of every kind, opens with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RegionInfo {
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    /// 7.4.1.5, the low three bits of the flags: OR, AND, XOR, XNOR, REPLACE.
    op: u8,
}

impl RegionInfo {
    fn read(reader: &mut Reader<'_>) -> Option<RegionInfo> {
        Some(RegionInfo {
            width: reader.u32()?,
            height: reader.u32()?,
            x: reader.u32()?,
            y: reader.u32()?,
            op: reader.u8()? & 0x07,
        })
    }
}

/// The nominal AT pixel positions of T.88 6.2.5.3, per template.
///
/// A file that uses these still writes them into the segment, so this is not
/// a default the wire format leans on — it is the answer to "what did the
/// figure in the standard show", and having it here is what lets a test say
/// that a custom AT pixel actually changed something.
const NOMINAL_AT: [[(i8, i8); 4]; 4] = [
    [(3, -1), (-3, -1), (2, -2), (-2, -2)],
    [(3, -1), (0, 0), (0, 0), (0, 0)],
    [(2, -1), (0, 0), (0, 0), (0, 0)],
    [(2, -1), (0, 0), (0, 0), (0, 0)],
];

/// How many bits of context a template forms, which is how many adaptive
/// states the region needs (T.88 6.2.5.7, Figures 8 to 11).
const fn template_bits(template: u8) -> usize {
    match template {
        0 => 16,
        1 => 13,
        _ => 10,
    }
}

/// T.88 6.2.5.7's pseudo-context for the typical-prediction decision.
///
/// These are **fixed by the standard**, not chosen: the SLTP bit shares the
/// context array with real neighbourhoods, so an encoder and a decoder have
/// to agree on which slot it uses.
///
/// Getting one wrong is quieter than it sounds, and the quiet is the danger.
/// A wrong slot that no neighbourhood in a particular picture happens to
/// reach behaves identically to the right one — every context starts in the
/// same state, so an unused slot is an unused slot. Changing template 0's to
/// `0x9B24` decodes Annex H.1 perfectly. It only breaks on a file whose
/// pixels reach the aliased neighbourhood, which is why the value is asserted
/// against the standard directly rather than inferred from a picture.
const fn tpgdon_context(template: u8) -> usize {
    match template {
        0 => 0x9B25,
        1 => 0x0795,
        2 => 0x00E5,
        _ => 0x0195,
    }
}

/// The context value for the pixel at `(x, y)` (T.88 6.2.5.7, Figures 8-11).
///
/// **Transcribed from the figures, bit by bit.** The templates are pictures
/// in the standard and the numbering is the reading order of those pictures,
/// so there is nothing to derive: the current row's nearest neighbour is bit
/// 0 and the count runs leftwards, then outwards through the rows above, with
/// each AT pixel in the slot its nominal position would have occupied.
///
/// Every template is written out rather than folded into a loop. A loop over
/// a table of offsets would be shorter and would make the four layouts look
/// interchangeable, which is exactly the thing that is not true about them.
/// The destination layer's fixed positions, 6.3.5.3 template 0.
///
/// All are causal — the pixel being decoded is not written yet, so a position
/// at or after it would read a zero that carries no information and would put
/// this decoder out of step with any encoder.
const REFINE_0_HERE: [(i8, i8); 3] = [(0, -1), (1, -1), (-1, 0)];

/// The reference layer's fixed positions for template 0: its whole
/// three-by-three neighbourhood bar the corner the adaptive pixel occupies.
const REFINE_0_THERE: [(i8, i8); 8] = [
    (0, -1),
    (1, -1),
    (-1, 0),
    (0, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// Template 1's destination positions. It has no adaptive pixels, which is
/// why 7.4.4.1.3's `SBRAT` is absent whenever `SBRTEMPLATE` is one.
const REFINE_1_HERE: [(i8, i8); 4] = [(-1, -1), (0, -1), (1, -1), (-1, 0)];

/// Template 1's reference positions.
const REFINE_1_THERE: [(i8, i8); 6] = [(0, -1), (-1, 0), (0, 0), (1, 0), (0, 1), (1, 1)];

/// 6.3.5.6's TPGRON pseudo-context for each template, **in this file's own
/// bit order** — see [`refinement_context`] for why that is not the
/// standard's, and `docs/design/jbig2-symbol-text.md` for how these two
/// numbers were determined rather than transcribed.
const TPGRON_0: usize = 0x0010;
const TPGRON_1: usize = 0x0008;

/// 6.3.5.3's nominal adaptive positions: `at[0]` in the destination layer,
/// `at[1]` in the reference.
const NOMINAL_REFINE_AT: [(i8, i8); 2] = [(-1, -1), (-1, -1)];

/// The layout a refinement template names, as data: the destination-layer
/// positions, the reference-layer positions, and whether two of them are
/// adaptive.
struct RefineTemplate<'a> {
    here: &'a [(i8, i8)],
    there: &'a [(i8, i8)],
    at: Option<[(i8, i8); 2]>,
    /// 6.3.5.6's TPGRON pseudo-context: the slot the typical-prediction
    /// decision shares the context array with.
    typical: usize,
}

impl RefineTemplate<'_> {
    /// How many context bits the layout forms, which is how many adaptive
    /// states 6.3 needs.
    fn bits(&self) -> usize {
        self.here.len() + self.there.len() + usize::from(self.at.is_some()) * 2
    }
}

/// **T.88 6.3.5.3's refinement context.**
///
/// Two layers at once: what has already been decoded of the target, and the
/// reference the target is a refinement *of*, shifted by the offset the caller
/// decoded. Template 0 forms thirteen bits, two of them adaptive (7.4.3.1.3's
/// `SDRAT`, 7.4.4.1.3's `SBRAT`); template 1 forms ten and has none.
///
/// # The bit order here is not the standard's, and that is sound
///
/// A context index is a label for an adaptive state slot and nothing more:
/// [`MqDecoder::decode_at`] reads and writes `state[cx]`, every slot begins in
/// the same state, and the A and C registers are global. Relabel every context
/// through any bijection and each slot is still reached by exactly the same
/// neighbourhoods in the same order, so the decision sequence is bit-for-bit
/// unchanged. **Only the set of positions has to be right**, which is what
/// lets this file hold a refinement decoder verified against Annex H rather
/// than transcribed from two figures.
///
/// The one place that freedom stops is 6.3.5.6's TPGRON pseudo-context, which
/// is a bare number in the standard's own ordering and does not survive a
/// relabelling. [`decode_refinement_into`] therefore has no TPGRON, and the
/// one caller that could set it refuses instead of guessing which slot it
/// names.
fn refinement_context(
    into: &Bitmap,
    reference: &Bitmap,
    dx: i32,
    dy: i32,
    template: &RefineTemplate<'_>,
    x: i32,
    y: i32,
) -> usize {
    // The reference is read at the target pixel shifted by the offset the
    // caller decoded, which is what makes a refinement a *difference* rather
    // than a second picture.
    let there = |ox: i8, oy: i8| reference.get(x - dx + i32::from(ox), y - dy + i32::from(oy));
    let mut value = 0u32;
    if let Some(at) = template.at {
        value = into.get(x + i32::from(at[0].0), y + i32::from(at[0].1));
        value = (value << 1) | there(at[1].0, at[1].1);
    }
    for (ox, oy) in template.here {
        value = (value << 1) | into.get(x + i32::from(*ox), y + i32::from(*oy));
    }
    for (ox, oy) in template.there {
        value = (value << 1) | there(*ox, *oy);
    }
    value as usize
}

/// **Clause 6.3: a generic refinement region**, over a coder the caller owns.
///
/// Decodes `into` as a refinement of `reference` shifted by `(dx, dy)`. The
/// coder and context set are the caller's because 6.5.8.2 runs this inside a
/// symbol dictionary, where the adaptive state has to survive from one symbol
/// to the next.
///
/// TPGRON (6.3.5.6) is deliberately absent — see [`refinement_context`].
fn decode_refinement_into(
    coder: &mut MqDecoder<'_>,
    contexts: &mut MqContexts,
    template: &RefineTemplate<'_>,
    tpgron: bool,
    reference: &Bitmap,
    // 6.3.5.3's GRREFERENCEDX/DY, as one value because they are never
    // meaningful apart.
    (dx, dy): (i32, i32),
    into: &mut Bitmap,
) {
    let mut ltp = 0u8;
    for y in 0..into.height {
        if tpgron {
            // 6.3.5.6: one decision per row toggling "this row is typical",
            // which is the refinement analogue of TPGDON and is why refining a
            // picture that barely changed costs almost nothing.
            ltp ^= coder.decode_at(contexts, template.typical);
        }
        for x in 0..into.width {
            let (sx, sy) = (x as i32, y as i32);
            if ltp == 1 {
                // In a typical row a pixel whose whole reference neighbourhood
                // agrees is not coded at all: it is that value. Only the
                // pixels on a boundary cost a decision.
                let centre = reference.get(sx - dx, sy - dy);
                let uniform = (-1..=1).all(|oy| {
                    (-1..=1).all(|ox| reference.get(sx - dx + ox, sy - dy + oy) == centre)
                });
                if uniform {
                    into.set(x, y, centre);
                    continue;
                }
            }
            let cx = refinement_context(into, reference, dx, dy, template, sx, sy);
            let pixel = coder.decode_at(contexts, cx);
            into.set(x, y, u32::from(pixel));
        }
    }
}

/// The layout `SDRTEMPLATE` or `SBRTEMPLATE` selects, with the adaptive pair
/// the header carried.
fn refine_template(rtemplate: u8, at: [(i8, i8); 2]) -> RefineTemplate<'static> {
    if rtemplate == 0 {
        RefineTemplate {
            here: &REFINE_0_HERE,
            there: &REFINE_0_THERE,
            at: Some(at),
            typical: TPGRON_0,
        }
    } else {
        RefineTemplate {
            here: &REFINE_1_HERE,
            there: &REFINE_1_THERE,
            at: None,
            typical: TPGRON_1,
        }
    }
}

/// 6.4.11 and 6.5.8.2.2's reference offset, which is the same arithmetic in
/// both: the size difference is split evenly and the coded offset added.
fn refinement_offset(target: i64, reference: u32, coded: i32) -> i64 {
    (target - i64::from(reference)).div_euclid(2) + i64::from(coded)
}

fn context(bitmap: &Bitmap, template: u8, at: &[(i8, i8); 4], x: i32, y: i32) -> usize {
    let p = |dx: i32, dy: i32| bitmap.get(x + dx, y + dy);
    let a = |i: usize| bitmap.get(x + i32::from(at[i].0), y + i32::from(at[i].1));
    let value = match template {
        0 => {
            (a(3) << 15)
                | (p(-1, -2) << 14)
                | (p(0, -2) << 13)
                | (p(1, -2) << 12)
                | (a(2) << 11)
                | (a(1) << 10)
                | (p(-2, -1) << 9)
                | (p(-1, -1) << 8)
                | (p(0, -1) << 7)
                | (p(1, -1) << 6)
                | (p(2, -1) << 5)
                | (a(0) << 4)
                | (p(-4, 0) << 3)
                | (p(-3, 0) << 2)
                | (p(-2, 0) << 1)
                | p(-1, 0)
        }
        1 => {
            (p(-1, -2) << 12)
                | (p(0, -2) << 11)
                | (p(1, -2) << 10)
                | (p(2, -2) << 9)
                | (p(-2, -1) << 8)
                | (p(-1, -1) << 7)
                | (p(0, -1) << 6)
                | (p(1, -1) << 5)
                | (p(2, -1) << 4)
                | (a(0) << 3)
                | (p(-3, 0) << 2)
                | (p(-2, 0) << 1)
                | p(-1, 0)
        }
        2 => {
            (p(-1, -2) << 9)
                | (p(0, -2) << 8)
                | (p(1, -2) << 7)
                | (p(-2, -1) << 6)
                | (p(-1, -1) << 5)
                | (p(0, -1) << 4)
                | (p(1, -1) << 3)
                | (a(0) << 2)
                | (p(-2, 0) << 1)
                | p(-1, 0)
        }
        // Template 3 is the only one that reads a single row above (Figure
        // 11), which is what makes it the cheap template for a striped page.
        _ => {
            (p(-3, -1) << 9)
                | (p(-2, -1) << 8)
                | (p(-1, -1) << 7)
                | (p(0, -1) << 6)
                | (p(1, -1) << 5)
                | (a(0) << 4)
                | (p(-4, 0) << 3)
                | (p(-3, 0) << 2)
                | (p(-2, 0) << 1)
                | p(-1, 0)
        }
    };
    value as usize
}

/// Decodes a generic region's pixels with the MQ coder (T.88 6.2.5.7).
///
/// The decision order is the standard's: every pixel of every row in raster
/// order, each against the context its already-decoded neighbours form. The
/// coder never runs out — past the end of the data it reads `0xFF`, which
/// E.3.4 treats as a marker — so a truncated region fills with whatever the
/// terminated coder yields rather than stopping short of its own height.
fn decode_arithmetic(
    data: &[u8],
    template: u8,
    tpgdon: bool,
    at: &[(i8, i8); 4],
    into: &mut Bitmap,
) {
    let mut coder = MqDecoder::new(data);
    let mut contexts = MqContexts::new(1 << template_bits(template));
    decode_generic_into(&mut coder, &mut contexts, template, tpgdon, at, into);
}

/// 6.2.5.7's row loop, over a coder and a context set the **caller** owns.
///
/// A region is one bitmap and can keep both to itself, which is what
/// [`decode_arithmetic`] does. A symbol dictionary cannot: 6.5.8.1 decodes
/// every symbol in the dictionary from one coder with one adaptive context set
/// carried across all of them, so the state that makes symbol two cheap is the
/// state symbol one left behind. Restarting either per symbol decodes the
/// first one correctly and then noise.
fn decode_generic_into(
    coder: &mut MqDecoder<'_>,
    contexts: &mut MqContexts,
    template: u8,
    tpgdon: bool,
    at: &[(i8, i8); 4],
    into: &mut Bitmap,
) {
    let mut ltp = 0u8;

    for y in 0..into.height {
        if tpgdon {
            // 6.2.5.7: one decision per row against a context the standard
            // fixes, toggling "this row is the same as the last one".
            ltp ^= coder.decode_at(contexts, tpgdon_context(template));
            if ltp == 1 {
                if y > 0 {
                    into.copy_row(y - 1, y);
                }
                continue;
            }
        }
        for x in 0..into.width {
            let cx = context(into, template, at, x as i32, y as i32);
            let pixel = coder.decode_at(contexts, cx);
            into.set(x, y, u32::from(pixel));
        }
    }
}

/// One generic region segment (T.88 7.4.6), decoded onto its own bitmap.
///
/// `None` means nothing was drawn and the caller must not count a region —
/// which is the whole refusal contract, so it is a return value rather than a
/// flag somebody could forget to read.
fn generic_region(
    segment: &Segment<'_>,
    ceiling: usize,
    warnings: &mut Vec<Warning>,
) -> Option<(RegionInfo, Bitmap)> {
    let mut reader = Reader::new(segment.data);
    let Some(info) = RegionInfo::read(&mut reader) else {
        note(warnings, Warning::TruncatedInput);
        return None;
    };
    // 7.4.6.2. Bit 0 selects MMR, bits 1-2 the template, bit 3 TPGDON.
    let Some(flags) = reader.u8() else {
        note(warnings, Warning::TruncatedInput);
        return None;
    };
    let mmr = flags & 0x01 != 0;
    let template = (flags >> 1) & 0x03;
    let tpgdon = flags & 0x08 != 0;

    // 7.4.6.3: the AT pixels are in the segment whenever it is not MMR —
    // four pairs for template 0, one for the others. Reading the wrong number
    // of them puts the coded data at the wrong offset, so a file with custom
    // AT pixels would decode as noise rather than as a slightly wrong
    // picture.
    let mut at = NOMINAL_AT[template as usize];
    if !mmr {
        let pairs = if template == 0 { 4 } else { 1 };
        for slot in at.iter_mut().take(pairs) {
            let (Some(dx), Some(dy)) = (reader.i8(), reader.i8()) else {
                note(warnings, Warning::TruncatedInput);
                return None;
            };
            *slot = (dx, dy);
        }
    }

    let Some(mut bitmap) = Bitmap::new(info.width, info.height, ceiling) else {
        note(warnings, Warning::Jbig2RegionTooLarge);
        return None;
    };
    if mmr {
        if !decode_mmr(reader.rest(), &mut bitmap, warnings) {
            // Not one row came out, so this is not T.6 data and there is no
            // region here. Compositing the blank bitmap that was just sized
            // would count as a region and turn the refusal into a blank page
            // reported as success.
            note(warnings, Warning::Jbig2SegmentSkipped);
            return None;
        }
    } else {
        decode_arithmetic(reader.rest(), template, tpgdon, &at, &mut bitmap);
    }
    Some((info, bitmap))
}

/// Decodes a generic region's pixels with MMR (T.88 6.2.6).
///
/// 6.2.6 is one sentence of substance: the region is coded exactly as a T.6
/// image of its own width, against an imaginary all-white line above its
/// first row. That is [`T6Rows`], which gap 16 left behind for this — so
/// there is one implementation of the T.6 mode codes in this crate and one
/// set of tests over them, and a change to either has to keep a fax and a
/// JBIG2 region both correct.
///
/// The polarity is already right and that is not a coincidence:
/// [`T6Rows`] packs **1 for black**, which is JBIG2's sense (6.2.2) rather
/// than PDF's, precisely so this caller has nothing to convert.
///
/// Returns whether any row decoded at all.
fn decode_mmr(data: &[u8], into: &mut Bitmap, warnings: &mut Vec<Warning>) -> bool {
    let mut rows = crate::T6Rows::new(data, 0, into.width);
    let stride = into.stride;
    let mut decoded = 0u32;
    for y in 0..into.height {
        let start = (y as usize) * stride;
        let Some(row) = into.bits.get_mut(start..start + stride) else {
            break;
        };
        if !rows.next_row(row) {
            break;
        }
        decoded += 1;
    }
    if decoded < into.height {
        // A region whose coding ran out part way is still a region: the rows
        // that decoded are on the page and the rest stay white, which is the
        // same bargain the fax path strikes. Only "not one row" is a refusal.
        note(warnings, Warning::TruncatedInput);
    }
    decoded > 0
}

/// Decodes an embedded JBIG2 stream into packed one-bit-per-pixel rows.
///
/// Rows are `params.width.div_ceil(8)` bytes each, most significant bit
/// first, **1 for black** — JBIG2's own sense (6.2.2), which the caller
/// inverts for PDF's.
///
/// `warnings` is a sink rather than a return value because the refusal below
/// is an `Err`, and ruling 10 wants what was skipped to survive it: "no
/// region, because the file is a symbol dictionary" and "no region, because
/// the stream was truncated" are different failures and the caller cannot
/// tell them apart from the error alone.
///
/// # Errors
/// [`FilterError::Unsupported`] when no region was composited onto the page.
/// That is the whole degradation contract for this codec: the caller draws
/// the neutral placeholder (ruling 2) rather than being handed a blank page
/// that reads as a successful decode of a blank scan.
pub fn decode(
    data: &[u8],
    params: &Jbig2Params<'_>,
    max_output: usize,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<u8>, FilterError> {
    let Some(bitmap) = Bitmap::new(params.width, params.height, max_output) else {
        note(warnings, Warning::Jbig2RegionTooLarge);
        return Err(FilterError::Unsupported(Capability::Jbig2));
    };
    let mut page = Page {
        intermediate: BTreeMap::new(),
        symbols: BTreeMap::new(),
        seen: BTreeSet::new(),
        refused: BTreeSet::new(),
        bitmap,
        number: None,
        regions: 0,
    };

    // D.3: the globals stream's segments are read first and are visible to
    // the page's own, which is how a shared symbol dictionary reaches every
    // image that names it. They are ordinary segments in every other way.
    let globals = segments(params.globals, warnings);
    let own = segments(data, warnings);
    for segment in globals.iter().chain(own.iter()) {
        if !understood(segment.kind) {
            if carries_content(segment.kind) {
                note(warnings, Warning::Jbig2SegmentSkipped);
            }
            continue;
        }
        if !page.owns(segment) {
            continue;
        }
        page.seen.insert(segment.number);
        match segment.kind {
            kind::PAGE_INFORMATION => page.begin(segment, warnings),
            kind::IMMEDIATE_GENERIC_REGION | kind::IMMEDIATE_LOSSLESS_GENERIC_REGION => {
                page.draw_generic(segment, max_output, warnings);
            }
            kind::INTERMEDIATE_GENERIC_REGION => page.keep_generic(segment, max_output, warnings),
            kind::INTERMEDIATE_REFINEMENT_REGION => {
                page.draw_refinement(segment, max_output, true, warnings);
            }
            kind::IMMEDIATE_REFINEMENT_REGION | kind::IMMEDIATE_LOSSLESS_REFINEMENT_REGION => {
                page.draw_refinement(segment, max_output, false, warnings);
            }
            kind::SYMBOL_DICTIONARY => page.read_symbols(segment, max_output, warnings),
            kind::INTERMEDIATE_TEXT_REGION => {
                page.draw_text(segment, max_output, true, warnings);
            }
            kind::IMMEDIATE_TEXT_REGION | kind::IMMEDIATE_LOSSLESS_TEXT_REGION => {
                page.draw_text(segment, max_output, false, warnings);
            }
            _ => {}
        }
    }

    if page.regions == 0 {
        // The refusal. Not polish, and not a fallback: see the module note.
        note(warnings, Warning::Jbig2SegmentSkipped);
        return Err(FilterError::Unsupported(Capability::Jbig2));
    }
    Ok(page.bitmap.bits)
}

/// **One generic refinement region segment (T.88 7.4.7)**, decoded against a
/// reference the caller supplies.
///
/// The reference is 6.3.2's: with no intermediate region referred to, it is
/// what the page already holds under this region's own box. A refinement
/// region improves a picture that is already there rather than drawing a new
/// one, which is why it is the one region type that reads the page back.
fn refinement_region(
    segment: &Segment<'_>,
    reference: &Bitmap,
    ceiling: usize,
    warnings: &mut Vec<Warning>,
) -> Option<(RegionInfo, Bitmap)> {
    let mut reader = Reader::new(segment.data);
    let Some(info) = RegionInfo::read(&mut reader) else {
        note(warnings, Warning::TruncatedInput);
        return None;
    };
    // 7.4.7.2. Bit 0 selects the template, bit 1 TPGRON.
    let Some(flags) = reader.u8() else {
        note(warnings, Warning::TruncatedInput);
        return None;
    };
    let rtemplate = flags & 0x01;
    let tpgron = flags & 0x02 != 0;
    // 7.4.7.3: the adaptive pair, at template 0 only.
    let mut at = NOMINAL_REFINE_AT;
    if rtemplate == 0 {
        for slot in &mut at {
            let (Some(dx), Some(dy)) = (reader.i8(), reader.i8()) else {
                note(warnings, Warning::TruncatedInput);
                return None;
            };
            *slot = (dx, dy);
        }
    }
    let template = refine_template(rtemplate, at);
    let Some(mut region) = Bitmap::new(info.width, info.height, ceiling) else {
        note(warnings, Warning::Jbig2RegionTooLarge);
        return None;
    };
    let mut coder = MqDecoder::new(reader.rest());
    let mut contexts = MqContexts::new(1 << template.bits());
    // 6.3.5.3: the region and its reference are the same size and in the same
    // place, so the offset between them is zero.
    decode_refinement_into(
        &mut coder,
        &mut contexts,
        &template,
        tpgron,
        reference,
        (0, 0),
        &mut region,
    );
    Some((info, region))
}

/// The page bitmap regions are composited onto, and what the page
/// information segment said about it.
struct Page {
    /// Packed 1-bpp rows, most significant bit first, 1 = black.
    bitmap: Bitmap,
    /// 7.4.6.1's auxiliary buffers: what each *intermediate* region decoded
    /// to, by segment number.
    ///
    /// An intermediate region is not drawn. It waits for the segment that
    /// refers to it — in practice a refinement region, which takes it as the
    /// reference 6.3.2 asks for — and that is the whole reason the two exist
    /// as separate segment types.
    intermediate: BTreeMap<u32, Bitmap>,
    /// What each symbol dictionary exported, by its segment number (7.4.3).
    ///
    /// A `BTreeMap` rather than a hash map because a text region's symbol list
    /// is the concatenation of its referred-to dictionaries' exports *in
    /// reference order*, and anything that iterates has to do so the same way
    /// on every target (ruling 4).
    symbols: BTreeMap<u32, Vec<Bitmap>>,
    /// Every segment number this stream has offered, whatever its type.
    ///
    /// A text region refers to its dictionaries *and* to its custom tables, and
    /// the two have to be told apart: a table contributes no symbols and is not
    /// a gap, while a dictionary that is absent or refused is. A number that
    /// was never seen at all is the second case — T.88 Annex H.1's page 2 is
    /// exactly that, referring to a dictionary that belongs to page 1.
    seen: BTreeSet<u32>,
    /// Symbol dictionaries that were offered and refused, by segment number.
    ///
    /// A text region numbers its symbols across the *concatenation* of every
    /// dictionary it refers to (7.4.3), so one missing dictionary does not cost
    /// its own symbols — it renumbers all of them, and every instance after the
    /// gap draws the wrong glyph at the right place. That is worse than drawing
    /// nothing and it looks like a working decoder, so a region that refers to
    /// one of these is refused whole.
    refused: BTreeSet<u32>,
    /// The page association of the page information segment, once one has
    /// been seen. A multi-page JBIG2 file pasted into a PDF stream carries
    /// segments for pages this image is not, and compositing those would
    /// draw another page's content onto this one.
    number: Option<u32>,
    /// How many regions were composited. Zero is the refusal.
    regions: usize,
}

impl Page {
    /// A page information segment (T.88 7.4.8).
    ///
    /// The page's own declared dimensions are read and *not* believed over
    /// the caller's: ISO 32000-1 7.4.7 makes the image dictionary's `/Width`
    /// and `/Height` the authority for an embedded stream, and a striped page
    /// writes `0xFFFFFFFF` for its height precisely because it does not yet
    /// know. What is taken from here is the default pixel value, which
    /// decides whether the page starts black.
    fn begin(&mut self, segment: &Segment<'_>, warnings: &mut Vec<Warning>) {
        let mut reader = Reader::new(segment.data);
        let (Some(width), Some(height)) = (reader.u32(), reader.u32()) else {
            note(warnings, Warning::TruncatedInput);
            return;
        };
        let _ = (reader.u32(), reader.u32()); // x and y resolution
        let Some(flags) = reader.u8() else {
            note(warnings, Warning::TruncatedInput);
            return;
        };
        self.number = Some(segment.page);
        if width != self.bitmap.width || (height != u32::MAX && height != self.bitmap.height) {
            // Not fatal and not repaired: the region segments carry their own
            // placement, so a page that disagrees with the dictionary still
            // composites at the coordinates it names. Worth recording,
            // because it is also what a stream pasted from another file looks
            // like.
            note(warnings, Warning::Jbig2SegmentSkipped);
        }
        // 7.4.8.5 bit 2: the value every pixel starts at. A scan of a mostly
        // black page is coded as black-by-default with white regions on it,
        // and ignoring this bit renders it as its own negative.
        if flags & 0x04 != 0 {
            self.bitmap.bits.fill(0xFF);
        }
    }

    /// Decodes a generic region segment and composites it (T.88 7.4.6).
    ///
    /// `regions` only moves when a bitmap actually arrived. A region that was
    /// refused — too large, truncated, or coded a way this build does not
    /// decode — leaves the count alone, so a file whose only region could not
    /// be decoded still reaches the refusal instead of returning the blank
    /// page it was composited onto.
    fn draw_generic(&mut self, segment: &Segment<'_>, ceiling: usize, warnings: &mut Vec<Warning>) {
        let Some((info, region)) = generic_region(segment, ceiling, warnings) else {
            return;
        };
        self.bitmap.composite(&region, info.x, info.y, info.op);
        self.regions += 1;
    }

    /// 7.4.7: decodes a generic refinement region and composites it.
    ///
    /// Unlike every other region, this one reads the page before it writes
    /// it: 6.3.2 makes the reference whatever is already under the region's
    /// box, so the window is lifted out first and the refinement decoded
    /// against it.
    fn draw_refinement(
        &mut self,
        segment: &Segment<'_>,
        ceiling: usize,
        intermediate: bool,
        warnings: &mut Vec<Warning>,
    ) {
        let Some(box_) = RegionInfo::read(&mut Reader::new(segment.data)) else {
            note(warnings, Warning::TruncatedInput);
            return;
        };
        // 6.3.2: the reference is a referred-to intermediate region if there
        // is one, and otherwise whatever the page already holds under this
        // region's own box.
        let referred = segment
            .referred
            .iter()
            .find_map(|number| self.intermediate.get(number))
            .cloned();
        let reference = match referred {
            Some(bitmap) => bitmap,
            None => {
                let Some(window) =
                    self.bitmap
                        .window(box_.x, box_.y, box_.width, box_.height, ceiling)
                else {
                    note(warnings, Warning::Jbig2RegionTooLarge);
                    return;
                };
                window
            }
        };
        let Some((info, region)) = refinement_region(segment, &reference, ceiling, warnings) else {
            note(warnings, Warning::Jbig2SegmentSkipped);
            return;
        };
        if intermediate {
            // 7.4.6.1: it waits to be referred to rather than being drawn, and
            // it is not a region for the purpose of the refusal.
            self.intermediate.insert(segment.number, region);
            return;
        }
        self.bitmap.composite(&region, info.x, info.y, info.op);
        self.regions += 1;
    }

    /// 7.4.6 for an *intermediate* generic region: decoded and kept, not drawn.
    fn keep_generic(&mut self, segment: &Segment<'_>, ceiling: usize, warnings: &mut Vec<Warning>) {
        let Some((_, region)) = generic_region(segment, ceiling, warnings) else {
            note(warnings, Warning::Jbig2SegmentSkipped);
            return;
        };
        self.intermediate.insert(segment.number, region);
    }

    /// 7.4.3: decodes a symbol dictionary and keeps what it exported.
    ///
    /// Nothing draws yet — a dictionary is not a region and does not count as
    /// one, so a file of dictionaries alone still refuses. The text region that
    /// reads these is the next milestone.
    ///
    /// Its imports are the concatenation of the dictionaries it refers to, in
    /// reference order (7.4.3). A reference this file has not seen is not an
    /// error here: it leaves the import list short, the export count then
    /// disagrees with the header, and the dictionary refuses by name rather
    /// than exporting symbols numbered against a list that was never built.
    fn read_symbols(&mut self, segment: &Segment<'_>, ceiling: usize, warnings: &mut Vec<Warning>) {
        let mut imported = Vec::new();
        for number in &segment.referred {
            if let Some(exports) = self.symbols.get(number) {
                imported.extend(exports.iter().cloned());
            }
        }
        match symbol_dictionary(segment, &imported, ceiling, warnings) {
            Some(exported) => {
                self.symbols.insert(segment.number, exported);
            }
            None => {
                self.refused.insert(segment.number);
                note(warnings, Warning::Jbig2SegmentSkipped);
            }
        }
    }

    /// 7.4.4: decodes a text region and composites it.
    ///
    /// Its symbols are the concatenation of the dictionaries it refers to, in
    /// reference order (7.4.3) — the same rule a dictionary uses for its own
    /// imports, and the reason both keep the referred-to list rather than the
    /// set of it. The instance codes in the region are indices into that
    /// concatenation, so a dictionary that was refused shortens the list and
    /// every symbol after it would be the wrong one: a region whose referred-to
    /// dictionaries did not all arrive is refused rather than drawn wrong.
    ///
    /// Like [`Page::draw_generic`], `regions` moves only when a bitmap actually
    /// arrived, so a file whose only text region refused still reaches the
    /// refusal instead of returning the blank page it was composited onto.
    fn draw_text(
        &mut self,
        segment: &Segment<'_>,
        ceiling: usize,
        intermediate: bool,
        warnings: &mut Vec<Warning>,
    ) {
        let dangling = |number: &u32| {
            !self.symbols.contains_key(number)
                && (self.refused.contains(number) || !self.seen.contains(number))
        };
        if segment.referred.iter().any(dangling) {
            // T.88 Annex H.1's own page 2 is this case: its arithmetic text
            // region refers to page 1's *Huffman* dictionary as well as its
            // own, so until the Huffman variant lands the numbering is short by
            // that dictionary's exports and every instance would draw the wrong
            // symbol. Named rather than attempted.
            note(warnings, Warning::Jbig2VariantSkipped);
            return;
        }
        let mut symbols = Vec::new();
        for number in &segment.referred {
            // A referred-to segment that is not a dictionary at all is
            // ordinary — a text region refers to its custom tables the same
            // way — and contributes nothing to the numbering.
            if let Some(exports) = self.symbols.get(number) {
                symbols.extend(exports.iter().cloned());
            }
        }
        let Some((info, region)) = text_region(segment, &symbols, ceiling, warnings) else {
            note(warnings, Warning::Jbig2SegmentSkipped);
            return;
        };
        if intermediate {
            // 7.4.6.1: an intermediate region is *not* drawn. It waits for the
            // segment that refers to it — here a refinement region, which takes
            // it as 6.3.2's reference — and compositing it as well would draw
            // the picture twice, once unrefined.
            self.intermediate.insert(segment.number, region);
            return;
        }
        self.bitmap.composite(&region, info.x, info.y, info.op);
        self.regions += 1;
    }

    /// Whether a segment's page association names this page.
    ///
    /// Page 0 is the association D.3 gives a segment that belongs to no
    /// particular page — what a `/JBIG2Globals` stream carries — so it always
    /// matches; and until a page information segment has been seen there is
    /// nothing for a segment to disagree with.
    fn owns(&self, segment: &Segment<'_>) -> bool {
        segment.page == 0 || self.number.is_none_or(|n| n == segment.page)
    }
}

#[cfg(test)]
mod tests {

    /// **B.3's code assignment is canonical**, checked against a table small
    /// enough to write the answer out by hand.
    ///
    /// B.1's four lines have prefix lengths 1, 2, 3, 3, so the codes are 0,
    /// 10, 110 and 111 — shortest first, and in table order within a length.
    /// This is the one piece of Annex B that is a construction rather than a
    /// datum, so it is worth pinning separately from the tables it is applied
    /// to: if the assignment is wrong every table is wrong the same way, and
    /// the page-level check could not say which.
    #[test]
    fn annex_b_assigns_canonical_prefix_codes() {
        let table = table_b1();
        assert_eq!(table.codes, vec![0b0, 0b10, 0b110, 0b111]);

        // And a table carrying an out-of-band line still assigns in order:
        // B.2's seven lines are 1, 2, 3, 4, 5, 6, 6.
        let b2 = table_b2();
        assert_eq!(
            b2.codes,
            vec![0b0, 0b10, 0b110, 0b1110, 0b11110, 0b111110, 0b111111]
        );
    }

    /// A value reads its prefix and then its offset.
    ///
    /// B.1 line 1 is a two-bit prefix `10` and eight bits of offset over a low
    /// of 16, so `10` followed by `00000101` is 21.
    #[test]
    fn a_huffman_line_adds_its_offset_to_its_low() {
        let table = table_b1();
        let bytes = [0b1000_0001, 0b0100_0000];
        let mut reader = BitReader::new(&bytes);
        assert_eq!(table.decode(&mut reader), Some(HuffValue::Value(16 + 5)));
    }

    /// And the out-of-band line carries no offset at all.
    #[test]
    fn the_out_of_band_line_ends_a_run() {
        let table = table_b2();
        let bytes = [0b1111_1100];
        let mut reader = BitReader::new(&bytes);
        assert_eq!(table.decode(&mut reader), Some(HuffValue::Oob));
    }
    use super::*;
    use crate::mq::encoder::MqEncoder;

    /// A.2's ranges, as `(first magnitude, field width, offset)`.
    ///
    /// Written from the clause rather than from [`decode_int`], because a
    /// round trip against a table copied out of the decoder proves the two
    /// copies agree and nothing else.
    const INT_RANGES: [(i64, u32, i64); 6] = [
        (0, 2, 0),
        (4, 4, 4),
        (20, 6, 20),
        (84, 8, 84),
        (340, 12, 340),
        (4436, 32, 4436),
    ];

    /// The encoder's side of A.2, for the round trip below.
    fn encode_int(encoder: &mut MqEncoder, prev: &mut usize, value: Option<i32>) {
        let bit = |encoder: &mut MqEncoder, prev: &mut usize, d: u8| {
            encoder.encode_at(*prev, d);
            *prev = if *prev < 256 {
                (*prev << 1) | usize::from(d)
            } else {
                (((*prev << 1) | usize::from(d)) & 511) | 256
            };
        };

        // OOB is the negative zero: sign set, magnitude nothing.
        let (sign, magnitude) = match value {
            None => (1u8, 0i64),
            Some(v) => (u8::from(v < 0), i64::from(v).abs()),
        };
        bit(encoder, prev, sign);

        let which = INT_RANGES
            .iter()
            .rposition(|(first, _, _)| magnitude >= *first)
            .unwrap_or(0);
        for step in 0..which {
            let _ = step;
            bit(encoder, prev, 1);
        }
        if which < INT_RANGES.len() - 1 {
            bit(encoder, prev, 0);
        }

        let (_, width, offset) = INT_RANGES[which];
        let field = magnitude - offset;
        for index in (0..width).rev() {
            bit(encoder, prev, ((field >> index) & 1) as u8);
        }
    }

    /// **Every range of A.2 round-trips, and so does the value that is not a
    /// value.**
    ///
    /// The magnitudes are the first and last of each of the six fields and one
    /// in the middle, so a build that got an offset or a width wrong fails at
    /// the boundary rather than somewhere in the interior where two mistakes
    /// can cancel. Both signs, because the sign is decoded first and shares the
    /// context tree with everything after it.
    ///
    /// `None` is OOB — the negative zero that ends a height class (6.5.7) and a
    /// text region's strip (6.4.5). It is in the same sequence as the ordinary
    /// values on purpose: OOB must not disturb the contexts for what follows
    /// it, and a test that decoded it alone could not tell.
    #[test]
    fn every_integer_range_and_oob_round_trips() {
        let mut values: Vec<Option<i32>> = vec![None];
        for (first, width, offset) in INT_RANGES {
            let last = offset + (1i64 << width) - 1;
            for magnitude in [first, first + 1, (first + last) / 2, last.min(1 << 30)] {
                values.push(Some(magnitude as i32));
                if magnitude != 0 {
                    values.push(Some(-(magnitude as i32)));
                }
                values.push(None);
            }
        }

        let mut encoder = MqEncoder::new(INT_CONTEXTS);
        for value in &values {
            let mut prev = 1usize;
            encode_int(&mut encoder, &mut prev, *value);
        }
        let bytes = encoder.flush();

        let mut coder = MqDecoder::new(&bytes);
        let mut cx = MqContexts::new(INT_CONTEXTS);
        for (index, want) in values.iter().enumerate() {
            let got = decode_int(&mut coder, &mut cx);
            assert_eq!(
                got, *want,
                "value {index} of the sequence: A.2 decoded {got:?} where {want:?} was encoded"
            );
        }
    }

    /// **A.3 reads back the symbol index it was given**, at every code length a
    /// dictionary can ask for.
    ///
    /// The lengths bracket the byte boundaries and the single-symbol case,
    /// where `SBSYMCODELEN` is zero and the procedure must read nothing at all
    /// and answer nothing — a loop written with the wrong bound reads one bit
    /// there and desynchronises everything after it.
    #[test]
    fn the_symbol_index_procedure_round_trips_at_every_code_length() {
        for code_len in [0u32, 1, 2, 7, 8, 9, 15, 16] {
            let count = 1u32 << code_len;
            let ids: Vec<u32> = (0..count.min(64)).chain([count - 1]).collect();

            let mut encoder = MqEncoder::new(iaid_contexts(code_len));
            for id in &ids {
                let mut prev = 1usize;
                for index in (0..code_len).rev() {
                    let d = ((id >> index) & 1) as u8;
                    encoder.encode_at(prev, d);
                    prev = (prev << 1) | usize::from(d);
                }
            }
            let bytes = encoder.flush();

            let mut coder = MqDecoder::new(&bytes);
            let mut cx = MqContexts::new(iaid_contexts(code_len));
            for want in &ids {
                let got = decode_iaid(&mut coder, &mut cx, code_len);
                assert_eq!(got, *want, "code length {code_len}");
            }
        }
    }

    /// **ITU-T T.88 Annex H.1, "Datastream example" — the whole file, byte
    /// for byte.** Three pages, twenty-one segments, and the only JBIG2 in
    /// this repository that somebody else wrote.
    ///
    /// This is the artefact the plan's risk table calls the highest-value
    /// single thing in the effort, and it is worth saying what it buys.
    /// Pages 1 and 2 are **the same picture coded two different ways** — page
    /// 1's generic region is MMR (segment 4), page 2's is arithmetic with
    /// template 0 and TPGDON (segment 11) — so the two decoders in this file
    /// share no code at all and must still agree pixel for pixel. A
    /// round-trip against an encoder written here could not say that; nor
    /// could it say that the context numbering matches the numbering the rest
    /// of the world encodes against.
    ///
    /// Transcribed from the copy in SerenityOS's test corpus, which is
    /// published under BSD-2-Clause and states that it reproduces the annex's
    /// bitstream exactly; the bytes themselves are the standard's. Every
    /// field was re-derived from clause 7.2 before it was trusted, which is
    /// where the segment offsets below come from.
    #[rustfmt::skip]
    const ANNEX_H: [u8; 860] = [
        0x97, 0x4A, 0x42, 0x32, 0x0D, 0x0A, 0x1A, 0x0A, 0x01, 0x00, 0x00, 0x00,
        0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x18,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0xE9, 0xCB,
        0xF4, 0x00, 0x26, 0xAF, 0x04, 0xBF, 0xF0, 0x78, 0x2F, 0xE0, 0x00, 0x40,
        0x00, 0x00, 0x00, 0x01, 0x30, 0x00, 0x01, 0x00, 0x00, 0x00, 0x13, 0x00,
        0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x38, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01,
        0x01, 0x00, 0x00, 0x00, 0x1C, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x00,
        0x00, 0x00, 0x02, 0xE5, 0xCD, 0xF8, 0x00, 0x79, 0xE0, 0x84, 0x10, 0x81,
        0xF0, 0x82, 0x10, 0x86, 0x10, 0x79, 0xF0, 0x00, 0x80, 0x00, 0x00, 0x00,
        0x03, 0x07, 0x42, 0x00, 0x02, 0x01, 0x00, 0x00, 0x00, 0x31, 0x00, 0x00,
        0x00, 0x25, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x0C, 0x09, 0x00, 0x10, 0x00, 0x00, 0x00, 0x05, 0x01,
        0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x0C, 0x40, 0x07, 0x08, 0x70, 0x41, 0xD0, 0x00,
        0x00, 0x00, 0x04, 0x27, 0x00, 0x01, 0x00, 0x00, 0x00, 0x2C, 0x00, 0x00,
        0x00, 0x36, 0x00, 0x00, 0x00, 0x2C, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
        0x00, 0x0B, 0x00, 0x01, 0x26, 0xA0, 0x71, 0xCE, 0xA7, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xF8, 0xF0, 0x00, 0x00, 0x00, 0x05, 0x10, 0x01,
        0x01, 0x00, 0x00, 0x00, 0x2D, 0x01, 0x04, 0x04, 0x00, 0x00, 0x00, 0x0F,
        0x20, 0xD1, 0x84, 0x61, 0x18, 0x45, 0xF2, 0xF9, 0x7C, 0x8F, 0x11, 0xC3,
        0x9E, 0x45, 0xF2, 0xF9, 0x7D, 0x42, 0x85, 0x0A, 0xAA, 0x84, 0x62, 0x2F,
        0xEE, 0xEC, 0x44, 0x62, 0x22, 0x35, 0x2A, 0x0A, 0x83, 0xB9, 0xDC, 0xEE,
        0x77, 0x80, 0x00, 0x00, 0x00, 0x06, 0x17, 0x20, 0x05, 0x01, 0x00, 0x00,
        0x00, 0x57, 0x00, 0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x24, 0x00, 0x00,
        0x00, 0x10, 0x00, 0x00, 0x00, 0x0F, 0x00, 0x01, 0x00, 0x00, 0x00, 0x08,
        0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x04, 0x00, 0x00, 0x00, 0xAA, 0xAA, 0xAA, 0xAA, 0x80, 0x08, 0x00, 0x80,
        0x36, 0xD5, 0x55, 0x6B, 0x5A, 0xD4, 0x00, 0x40, 0x04, 0x2E, 0xE9, 0x52,
        0xD2, 0xD2, 0xD2, 0x8A, 0xA5, 0x4A, 0x00, 0x20, 0x02, 0x23, 0xE0, 0x95,
        0x24, 0xB4, 0x92, 0x8A, 0x4A, 0x92, 0x54, 0x92, 0xD2, 0x4A, 0x29, 0x2A,
        0x49, 0x40, 0x04, 0x00, 0x40, 0x00, 0x00, 0x00, 0x07, 0x31, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x30, 0x00, 0x02, 0x00,
        0x00, 0x00, 0x13, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x38, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x09, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x1B, 0x08, 0x00, 0x02,
        0xFF, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x4F, 0xE7, 0x8C,
        0x20, 0x0E, 0x1D, 0xC7, 0xCF, 0x01, 0x11, 0xC4, 0xB2, 0x6F, 0xFF, 0xAC,
        0x00, 0x00, 0x00, 0x0A, 0x07, 0x40, 0x00, 0x09, 0x02, 0x00, 0x00, 0x00,
        0x1F, 0x00, 0x00, 0x00, 0x25, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00,
        0x04, 0x00, 0x00, 0x00, 0x01, 0x00, 0x0C, 0x08, 0x00, 0x00, 0x00, 0x05,
        0x8D, 0x6E, 0x5A, 0x12, 0x40, 0x85, 0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0B,
        0x27, 0x00, 0x02, 0x00, 0x00, 0x00, 0x23, 0x00, 0x00, 0x00, 0x36, 0x00,
        0x00, 0x00, 0x2C, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x0B, 0x00,
        0x08, 0x03, 0xFF, 0xFD, 0xFF, 0x02, 0xFE, 0xFE, 0xFE, 0x04, 0xEE, 0xED,
        0x87, 0xFB, 0xCB, 0x2B, 0xFF, 0xAC, 0x00, 0x00, 0x00, 0x0C, 0x10, 0x01,
        0x02, 0x00, 0x00, 0x00, 0x1C, 0x06, 0x04, 0x04, 0x00, 0x00, 0x00, 0x0F,
        0x90, 0x71, 0x6B, 0x6D, 0x99, 0xA7, 0xAA, 0x49, 0x7D, 0xF2, 0xE5, 0x48,
        0x1F, 0xDC, 0x68, 0xBC, 0x6E, 0x40, 0xBB, 0xFF, 0xAC, 0x00, 0x00, 0x00,
        0x0D, 0x17, 0x20, 0x0C, 0x02, 0x00, 0x00, 0x00, 0x3E, 0x00, 0x00, 0x00,
        0x20, 0x00, 0x00, 0x00, 0x24, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00,
        0x0F, 0x00, 0x02, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x09, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x87,
        0xCB, 0x82, 0x1E, 0x66, 0xA4, 0x14, 0xEB, 0x3C, 0x4A, 0x15, 0xFA, 0xCC,
        0xD6, 0xF3, 0xB1, 0x6F, 0x4C, 0xED, 0xBF, 0xA7, 0xBF, 0xFF, 0xAC, 0x00,
        0x00, 0x00, 0x0E, 0x31, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x0F, 0x30, 0x00, 0x03, 0x00, 0x00, 0x00, 0x13, 0x00, 0x00, 0x00,
        0x25, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x00, 0x16, 0x08, 0x00, 0x02, 0xFF, 0x00, 0x00, 0x00, 0x01, 0x00,
        0x00, 0x00, 0x01, 0x4F, 0xE7, 0x8D, 0x68, 0x1B, 0x14, 0x2F, 0x3F, 0xFF,
        0xAC, 0x00, 0x00, 0x00, 0x11, 0x00, 0x21, 0x10, 0x03, 0x00, 0x00, 0x00,
        0x20, 0x08, 0x02, 0x02, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00,
        0x03, 0x00, 0x00, 0x00, 0x02, 0x4F, 0xE9, 0xD7, 0xD5, 0x90, 0xC3, 0xB5,
        0x26, 0xA7, 0xFB, 0x6D, 0x14, 0x98, 0x3F, 0xFF, 0xAC, 0x00, 0x00, 0x00,
        0x12, 0x07, 0x20, 0x11, 0x03, 0x00, 0x00, 0x00, 0x25, 0x00, 0x00, 0x00,
        0x25, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x8C, 0x12, 0x00, 0x00, 0x00, 0x04, 0xA9, 0x5C, 0x8B, 0xF4,
        0xC3, 0x7D, 0x96, 0x6A, 0x28, 0xE5, 0x76, 0x8F, 0xFF, 0xAC, 0x00, 0x00,
        0x00, 0x13, 0x31, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x14, 0x33, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    /// Where each of Annex H.1's pages begins and ends inside [`ANNEX_H`],
    /// derived by walking clause 7.2's headers.
    ///
    /// Slicing the file this way rather than transcribing three shorter
    /// fixtures keeps one authoritative copy of the annex: a sub-stream that
    /// disagreed with the whole would be a transcription error nothing could
    /// catch.
    ///
    /// The first thirteen bytes are D.4's file header and page count, which
    /// the embedded organisation a PDF uses (D.3) does not carry.
    /// Segment 0: the symbol dictionary both pages' text regions refer to.
    ///
    /// It is declared on page 0, which is T.88's way of saying shared, and
    /// ISO 32000-1 7.4.7 carries exactly this in .
    const SHARED_DICTIONARY: std::ops::Range<usize> = 13..48;

    const PAGE_1: std::ops::Range<usize> = 13..400;
    const PAGE_2: std::ops::Range<usize> = 400..682;

    /// Page 3, the refinement page. Segment 16 — the dictionary its own
    /// dictionary refines against — sits inside this range rather than before
    /// it, because it is declared on page 0 and the annex puts it where it is
    /// first needed.
    const PAGE_3: std::ops::Range<usize> = 682..860;

    /// **The picture T.88 Annex H.1 publishes for its generic region.**
    ///
    /// Fifty-four by forty-four, a frame two pixels thick, drawn at (4, 11)
    /// on both of the annex's first two pages. `#` is a 1, which is black
    /// (6.2.2).
    #[rustfmt::skip]
    const ANNEX_H_REGION: [&str; 44] = [
        "######################################################",
        "######################################################",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "##..................................................##",
        "######################################################",
        "######################################################",
    ];

    /// Packed rows as one string per row, `#` for black.
    ///
    /// Every bitmap assertion in this file goes through this rather than
    /// comparing byte vectors, because the failure of a wrong template is a
    /// *picture* — a frame that lost its right edge, a page that went to
    /// noise on row three — and a hex diff of 448 bytes says none of that.
    fn picture(bits: &[u8], width: u32, height: u32) -> Vec<String> {
        let stride = (width as usize).div_ceil(8);
        (0..height as usize)
            .map(|y| {
                (0..width as usize)
                    .map(|x| {
                        let byte = bits.get(y * stride + (x >> 3)).copied().unwrap_or(0);
                        if (byte >> (7 - (x & 7))) & 1 == 1 {
                            '#'
                        } else {
                            '.'
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// The 54 by 44 window Annex H.1 places its generic region in, lifted
    /// back off the 64 by 56 page it was composited onto.
    fn region_window(page: &[String]) -> Vec<String> {
        page[11..55]
            .iter()
            .map(|row| row[4..58].to_string())
            .collect()
    }

    /// A picture with something in it for every template to get wrong.
    ///
    /// Forty by twenty-four, and deliberately awkward: a diagonal, solid
    /// blocks, two checkerboards at opposite phases, a run of identical rows
    /// for typical prediction to collapse, and ink hard against both edges so
    /// the out-of-region rule of 6.2.5.2 is doing work on every row rather
    /// than only on the first.
    #[rustfmt::skip]
    const SPECIMEN: [&str; 24] = [
        "........................................",
        ".#......................................",
        "..#.....................................",
        "...#....................................",
        "....#...................................",
        ".....#..................................",
        "......########..........................",
        "......########..........................",
        "......########..........................",
        "..............#.........................",
        "...............#........................",
        "....#.#.#.#.#.#.#.#.#.#.................",
        "...#.#.#.#.#.#.#.#.#.#.#................",
        "........................................",
        "........................................",
        "..............................##########",
        "..............................##########",
        "#.#.#.#.#.#.#.#.#.#.#.#.#.#.#.#.#.#.#.#.",
        "########################################",
        "........................................",
        "...###...###...###...###...###...###....",
        "..#...#.#...#.#...#.#...#.#...#.#...#...",
        "...###...###...###...###...###...###....",
        "........................................",
    ];

    /// AT positions nowhere near 6.2.5.3's nominal ones, and all causal —
    /// above the current row, or to its left on it — so a round-trip is
    /// legal.
    ///
    /// Distance from the nominal set is the point. A decoder that read the
    /// AT bytes but ignored them, or never read them at all, forms different
    /// contexts from the encoder here and the picture does not come back.
    const CUSTOM_AT: [(i8, i8); 4] = [(-5, 0), (4, -1), (-4, -2), (5, -2)];

    fn bitmap_from(rows: &[&str]) -> Bitmap {
        let height = rows.len() as u32;
        let width = rows.first().map_or(0, |row| row.len()) as u32;
        let mut bitmap = Bitmap::new(width, height, 1 << 20).expect("a test picture fits");
        for (y, row) in rows.iter().enumerate() {
            for (x, cell) in row.chars().enumerate() {
                bitmap.set(x as u32, y as u32, u32::from(cell == '#'));
            }
        }
        bitmap
    }

    fn rows_identical(bitmap: &Bitmap, a: u32, b: u32) -> bool {
        (0..bitmap.width).all(|x| bitmap.get(x as i32, a as i32) == bitmap.get(x as i32, b as i32))
    }

    /// The exact inverse of [`decode_arithmetic`], through the encoder Annex
    /// H.2 pins.
    ///
    /// Contexts come off the *source* rather than off a partially rebuilt
    /// bitmap, which is the same thing: every template reads only pixels
    /// above the current row or left of it on it, so by the time a decoder
    /// forms a context it holds exactly these values.
    /// Encodes a symbol dictionary segment's data part (7.4.3 and 6.5), the
    /// arithmetic variant, exporting every new symbol.
    ///
    /// The mirror of [`symbol_dictionary`], written from the clause: height
    /// classes in ascending order, widths accumulating inside each, OOB to end
    /// a class, and 6.5.10's alternating export runs. One coder and one
    /// generic context set across the whole dictionary, which is the thing the
    /// round trip is really checking.
    fn symbol_dictionary_data(classes: &[&[&[&str]]], template: u8) -> Vec<u8> {
        let count: usize = classes.iter().map(|class| class.len()).sum();
        symbol_dictionary_with_exports(
            classes,
            template,
            &[Some(0), Some(count as i32)],
            count as u32,
        )
    }

    /// The same, with 6.5.10's export runs and the promised count given
    /// explicitly, so a test can select across imported symbols or promise a
    /// count the data does not keep.
    fn symbol_dictionary_with_exports(
        classes: &[&[&[&str]]],
        template: u8,
        runs: &[Option<i32>],
        num_ex: u32,
    ) -> Vec<u8> {
        let at = NOMINAL_AT[template as usize];
        let count: usize = classes.iter().map(|class| class.len()).sum();

        let mut data = Vec::new();
        // 7.4.3.1.1: arithmetic, no refinement, this template.
        data.extend_from_slice(&(u16::from(template) << 10).to_be_bytes());
        for (dx, dy) in at.iter().take(if template == 0 { 4 } else { 1 }) {
            data.push(*dx as u8);
            data.push(*dy as u8);
        }
        data.extend_from_slice(&num_ex.to_be_bytes()); // SDNUMEXSYMS
        data.extend_from_slice(&(count as u32).to_be_bytes()); // SDNUMNEWSYMS

        // The decoder keeps one `MqContexts` array per procedure; the encoder
        // has a single array, so the procedures are laid out end to end in it
        // and each is given a base. Context states are per index either way,
        // and the coder's own registers are shared either way, which is what
        // makes the two arrangements the same stream.
        let generic_len = 1usize << template_bits(template);
        let mut encoder = MqEncoder::new(generic_len + INT_CONTEXTS * 3);
        let mut height = 0i64;
        let mut prevs = [1usize; 3]; // IADH, IADW, IAEX.
        let iadh = generic_len;
        let iadw = iadh + INT_CONTEXTS;
        let iaex = iadw + INT_CONTEXTS;

        for class in classes {
            let class_height = class
                .first()
                .map(|rows| rows.len() as i64)
                .unwrap_or_default();
            encode_int_at(
                &mut encoder,
                iadh,
                &mut prevs[0],
                Some((class_height - height) as i32),
            );
            height = class_height;

            let mut width = 0i64;
            for rows in *class {
                let symbol = bitmap_from(rows);
                encode_int_at(
                    &mut encoder,
                    iadw,
                    &mut prevs[1],
                    Some((i64::from(symbol.width) - width) as i32),
                );
                width = i64::from(symbol.width);
                for y in 0..symbol.height {
                    for x in 0..symbol.width {
                        let cx = context(&symbol, template, &at, x as i32, y as i32);
                        encoder.encode_at(cx, symbol.get(x as i32, y as i32) as u8);
                    }
                }
            }
            // OOB ends the height class.
            encode_int_at(&mut encoder, iadw, &mut prevs[1], None);
        }

        // 6.5.10: alternating runs, the first of them not exported.
        for run in runs {
            encode_int_at(&mut encoder, iaex, &mut prevs[2], *run);
        }

        data.extend(encoder.flush());
        data
    }

    /// One symbol instance for [`text_region_segment`].
    struct Instance {
        /// Which exported symbol, by index.
        id: u32,
        /// The gap from the previous instance's far edge, or `None` for the
        /// first in a strip — which takes 6.4.5's `FIRSTS` delta instead.
        gap: Option<i32>,
        /// The coordinate within the strip, ignored when there is one strip.
        t: i32,
    }

    /// A text region segment (7.4.4) over an arithmetic coder, laid out the way
    /// [`symbol_dictionary_with_exports`] lays a dictionary out: one context
    /// array with a base per procedure, which is the same stream the decoder's
    /// array-per-procedure reads.
    ///
    /// `strips` is `SBSTRIPS`, and each strip is `(first_s_delta, instances)`.
    fn text_region_segment(
        width: u32,
        height: u32,
        corner: u8,
        strips: u32,
        symbols: usize,
        strip_t: &[i32],
        strip_rows: &[(i32, Vec<Instance>)],
    ) -> Vec<u8> {
        let log_strips = strips.trailing_zeros();
        let instances: u32 = strip_rows.iter().map(|(_, run)| run.len() as u32).sum();

        let mut data = Vec::new();
        // 7.4.1: the region segment information field.
        data.extend_from_slice(&width.to_be_bytes());
        data.extend_from_slice(&height.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes()); // x
        data.extend_from_slice(&0u32.to_be_bytes()); // y
        data.push(0); // external combination operator: OR
                      // 7.4.4.1.1: arithmetic, no refinement, no transposition, OR, and no
                      // SBDSOFFSET — every knob this milestone does not implement is off, and
                      // the ones it does are exercised by the fixtures rather than defaulted.
        let flags = ((log_strips as u16) << 2) | (u16::from(corner) << 4);
        data.extend_from_slice(&flags.to_be_bytes());
        data.extend_from_slice(&instances.to_be_bytes());

        let code_len = symbol_code_length(symbols);
        let id_len = iaid_contexts(code_len);
        let mut encoder = MqEncoder::new(INT_CONTEXTS * 4 + id_len);
        let (iadt, iafs, iads, iait) = (0, INT_CONTEXTS, INT_CONTEXTS * 2, INT_CONTEXTS * 3);
        let iaid = INT_CONTEXTS * 4;
        let mut prevs = [1usize; 4];

        // 6.4.5 step 1: the initial strip coordinate, negated by the decoder.
        encode_int_at(&mut encoder, iadt, &mut prevs[0], Some(0));
        for (index, (first_s, run)) in strip_rows.iter().enumerate() {
            let delta = strip_t.get(index).copied().unwrap_or(0);
            encode_int_at(&mut encoder, iadt, &mut prevs[0], Some(delta));
            encode_int_at(&mut encoder, iafs, &mut prevs[1], Some(*first_s));
            for instance in run {
                if let Some(gap) = instance.gap {
                    encode_int_at(&mut encoder, iads, &mut prevs[2], Some(gap));
                }
                if strips > 1 {
                    encode_int_at(&mut encoder, iait, &mut prevs[3], Some(instance.t));
                }
                // A.3: the symbol code is a fixed-width tree walk rather than
                // one of Annex A's integer procedures.
                let mut prev = 1usize;
                for bit in (0..code_len).rev() {
                    let d = ((instance.id >> bit) & 1) as u8;
                    encoder.encode_at(iaid + prev, d);
                    prev = (prev << 1) | usize::from(d);
                }
            }
            // OOB ends the strip.
            encode_int_at(&mut encoder, iads, &mut prevs[2], None);
        }

        data.extend(encoder.flush());
        data
    }

    /// **Milestone 4.** A text region places the symbols a dictionary exported,
    /// at the coordinates 6.4.5 computes, across two strips.
    ///
    /// The fixture is a round trip against an encoder written from the same
    /// clause, so what it proves is that the *plumbing* holds: the strip
    /// coordinate accumulating, the out-of-band value ending a strip rather
    /// than the region, the gap being measured from the previous symbol's far
    /// edge rather than its origin, the symbol code being as wide as the count
    /// needs, and the region landing where its segment says. It cannot prove
    /// the placement convention itself — both sides share one reading of the
    /// clause — and T.88 Annex H.1's own page cannot either, for a reason the
    /// test below records.
    #[test]
    fn a_text_region_places_its_symbols_where_6_4_5_computes() {
        // Two symbols, two and three wide, both two high.
        let dictionary = symbol_dictionary_data(&[&[&["##", "##"], &["###", "###"]]], 0);
        let region = text_region_segment(
            12,
            6,
            corner::TOPLEFT,
            1,
            2,
            &[0, 3],
            &[
                (
                    0,
                    vec![
                        Instance {
                            id: 0,
                            gap: None,
                            t: 0,
                        },
                        // The first symbol is two wide, so the running
                        // coordinate stands at 1 and a gap of 3 puts this one
                        // at 4 — which is what "from the far edge" means.
                        Instance {
                            id: 1,
                            gap: Some(3),
                            t: 0,
                        },
                    ],
                ),
                (
                    1,
                    vec![Instance {
                        id: 0,
                        gap: None,
                        t: 0,
                    }],
                ),
            ],
        );

        let mut stream = header(0, kind::PAGE_INFORMATION, 1, &page_info(12, 6, 0));
        stream.extend(header(1, kind::SYMBOL_DICTIONARY, 1, &dictionary));
        stream.extend(header_referring(
            2,
            kind::IMMEDIATE_TEXT_REGION,
            1,
            &[1],
            &region,
        ));

        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: 12,
            height: 6,
        };
        let bits = decode(&stream, &params, 1 << 20, &mut warnings)
            .expect("an arithmetic text region is decoded");
        assert_eq!(
            picture(&bits, 12, 6),
            [
                "##..###.....",
                "##..###.....",
                "............",
                ".##.........",
                ".##.........",
                "............",
            ],
            "warnings: {warnings:?}"
        );
    }

    /// A text region whose dictionary refused is refused **whole**, by name.
    ///
    /// 7.4.3 numbers a region's symbols across the concatenation of every
    /// dictionary it refers to, so a missing one does not cost its own symbols
    /// — it renumbers all of them, and every instance after the gap draws the
    /// wrong symbol at the right place. A decoder that carried on would produce
    /// a page that looks like text and says something else.
    ///
    /// **T.88 Annex H.1's page 2 is exactly this case**, which is why the
    /// annex cannot adjudicate this milestone: its arithmetic text region
    /// (segment 10) refers to segments 0 and 9, and segment 0 is page 1's
    /// *Huffman* dictionary. The published page appears when the Huffman
    /// variant lands, and until then this is what correct looks like.
    #[test]
    fn a_text_region_whose_dictionary_refused_is_refused_by_name() {
        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: 64,
            height: 56,
        };
        let bits = decode(&ANNEX_H[PAGE_2], &params, 1 << 20, &mut warnings)
            .expect("the page's generic region still decodes");

        assert!(
            warnings.contains(&Warning::Jbig2VariantSkipped),
            "the text region refers to a Huffman dictionary and must say so: \
             {warnings:?}"
        );
        // And it drew nothing: the text region sits in the page's top rows,
        // above the generic region this build does decode.
        let page = picture(&bits, 64, 56);
        assert!(
            page[0..11].iter().all(|row| !row.contains('#')),
            "a refused text region put ink on the page"
        );
    }

    /// [`encode_int`] against a context array that begins at `base`.
    fn encode_int_at(encoder: &mut MqEncoder, base: usize, prev: &mut usize, value: Option<i32>) {
        *prev = 1;
        let bit = |encoder: &mut MqEncoder, prev: &mut usize, d: u8| {
            encoder.encode_at(base + *prev, d);
            *prev = if *prev < 256 {
                (*prev << 1) | usize::from(d)
            } else {
                (((*prev << 1) | usize::from(d)) & 511) | 256
            };
        };
        let (sign, magnitude) = match value {
            None => (1u8, 0i64),
            Some(v) => (u8::from(v < 0), i64::from(v).abs()),
        };
        bit(encoder, prev, sign);
        let which = INT_RANGES
            .iter()
            .rposition(|(first, _, _)| magnitude >= *first)
            .unwrap_or(0);
        for _ in 0..which {
            bit(encoder, prev, 1);
        }
        if which < INT_RANGES.len() - 1 {
            bit(encoder, prev, 0);
        }
        let (_, width, offset) = INT_RANGES[which];
        let field = magnitude - offset;
        for index in (0..width).rev() {
            bit(encoder, prev, ((field >> index) & 1) as u8);
        }
    }

    /// **A symbol dictionary decodes back the symbols it was built from**,
    /// pixel for pixel, across several height classes.
    ///
    /// Clause 6.5's shape is two nested accumulations — heights across classes,
    /// widths inside one — ended by an out-of-band value, and every symbol
    /// after the first is decoded from adaptive state the ones before it left
    /// behind (6.5.8.1). So the interesting failures are all *downstream*: a
    /// build that restarts the contexts per symbol, or loses the width
    /// accumulator, or reads OOB as a width, decodes symbol one correctly and
    /// then noise. Three classes with several symbols each is the smallest
    /// fixture where all three of those show.
    ///
    /// The symbols are asymmetric on both axes on purpose: a transposed
    /// width and height, or a row and column swapped in the context, survives
    /// any square fixture.
    #[test]
    fn a_symbol_dictionary_round_trips_its_symbols() {
        let short: [&[&str]; 2] = [&["#..#", ".##.", "#..#"], &["####", "#...", "#..#"]];
        let tall: [&[&str]; 3] = [
            &["#.", "##", "#.", "..", "#."],
            &["#####", ".#...", ".#...", ".#...", "..###"],
            &["#", "#", "#", "#", "."],
        ];
        let taller: [&[&str]; 1] = [&[
            "#..#..#", "......#", "#######", ".#...#.", "#.....#", "##...##", "....#..",
        ]];
        let classes: [&[&[&str]]; 3] = [&short, &tall, &taller];

        for template in 0..4u8 {
            let data = symbol_dictionary_data(&classes, template);
            let segment = Segment {
                number: 1,
                referred: Vec::new(),
                kind: kind::SYMBOL_DICTIONARY,
                page: 1,
                data: &data,
            };
            let mut warnings = Vec::new();
            let exported = symbol_dictionary(&segment, &[], 1 << 20, &mut warnings)
                .unwrap_or_else(|| panic!("template {template} did not decode: {warnings:?}"));

            let expected: Vec<&[&str]> = classes.iter().flat_map(|c| c.iter().copied()).collect();
            assert_eq!(exported.len(), expected.len(), "template {template}");
            for (index, rows) in expected.iter().enumerate() {
                let want = bitmap_from(rows);
                let got = &exported[index];
                assert_eq!(
                    (got.width, got.height),
                    (want.width, want.height),
                    "template {template}, symbol {index}: dimensions"
                );
                for y in 0..want.height {
                    for x in 0..want.width {
                        assert_eq!(
                            got.get(x as i32, y as i32),
                            want.get(x as i32, y as i32),
                            "template {template}, symbol {index}, pixel ({x}, {y})"
                        );
                    }
                }
            }
        }
    }

    /// **A dictionary exports what 6.5.10's runs select, out of its imported
    /// symbols as well as its new ones.**
    ///
    /// The export flags run over the imported symbols *followed by* the new
    /// ones, so a dictionary can re-export what it was given, drop what it
    /// decoded, or interleave the two — and a text region numbers its symbols
    /// across whatever comes out. A build that exported only the new symbols
    /// passes every single-dictionary fixture and then puts the wrong glyph on
    /// every page of a document whose dictionaries chain.
    #[test]
    fn export_runs_select_across_imported_and_new_symbols() {
        let new_symbols: [&[&str]; 2] = [&["##", ".#"], &["#.", "##"]];
        let classes: [&[&[&str]]; 1] = [&new_symbols];
        let imported = [bitmap_from(&["#"]), bitmap_from(&[".."])];

        // Skip one, take two, skip one: the second imported symbol and the
        // first new one.
        let runs = [Some(1), Some(2), Some(1)];
        let data = symbol_dictionary_with_exports(&classes, 0, &runs, 2);

        let segment = Segment {
            number: 1,
            referred: Vec::new(),
            kind: kind::SYMBOL_DICTIONARY,
            page: 1,
            data: &data,
        };
        let mut warnings = Vec::new();
        let exported = symbol_dictionary(&segment, &imported, 1 << 20, &mut warnings)
            .unwrap_or_else(|| panic!("it did not decode: {warnings:?}"));

        assert_eq!(exported.len(), 2, "two symbols were selected");
        assert_eq!(
            (exported[0].width, exported[0].height),
            (2, 1),
            "the first export is the second *imported* symbol, not a new one"
        );
        assert_eq!(
            (exported[1].width, exported[1].height),
            (2, 2),
            "the second export is the first new symbol"
        );
    }

    /// **Every variant this milestone does not decode refuses by its own
    /// name**, rather than as the segment type nobody has started.
    ///
    /// `Jbig2SegmentSkipped` means a lineage with no work behind it;
    /// `Jbig2VariantSkipped` means a file one scheduled milestone away. The
    /// corpus census counted how many files each of those is, and folding them
    /// together is how the residual after a capability lands comes to look
    /// like the refusal it replaced.
    #[test]
    fn the_variants_this_build_does_not_decode_refuse_by_their_own_name() {
        // SDREFAGG's one remaining road — over Huffman, whose refinement
        // lengths are a field this decoder does not read — a consumed retained
        // context, and a custom-table selector, clause 7.4.13's type 53
        // segments, which nothing reads yet. Neither SDHUFF nor SDREFAGG is
        // refused on its own any more, and neither is either refinement
        // template: all of that decodes.
        for flags in [0x0003u16, 0x0100, 0x000D] {
            let mut data = Vec::new();
            data.extend_from_slice(&flags.to_be_bytes());
            data.extend_from_slice(&[0; 8]); // AT, template 0.
            data.extend_from_slice(&1u32.to_be_bytes());
            data.extend_from_slice(&1u32.to_be_bytes());
            let segment = Segment {
                number: 1,
                referred: Vec::new(),
                kind: kind::SYMBOL_DICTIONARY,
                page: 1,
                data: &data,
            };
            let mut warnings = Vec::new();
            assert!(
                symbol_dictionary(&segment, &[], 1 << 20, &mut warnings).is_none(),
                "flags {flags:#06x} decoded"
            );
            assert_eq!(
                warnings,
                vec![Warning::Jbig2VariantSkipped],
                "flags {flags:#06x} refused under the wrong name"
            );
        }
    }

    /// **A dictionary promising more symbols than it holds is refused**, not
    /// truncated.
    ///
    /// `SDNUMEXSYMS` is what a text region indexes against, so a dictionary
    /// that comes up short would silently renumber every symbol after the gap.
    #[test]
    fn a_dictionary_that_does_not_keep_its_promised_count_is_refused() {
        let new_symbols: [&[&str]; 1] = [&["#"]];
        let classes: [&[&[&str]]; 1] = [&new_symbols];
        // One symbol encoded, two exports promised.
        let data = symbol_dictionary_with_exports(&classes, 0, &[Some(0), Some(1)], 2);
        let segment = Segment {
            number: 1,
            referred: Vec::new(),
            kind: kind::SYMBOL_DICTIONARY,
            page: 1,
            data: &data,
        };
        let mut warnings = Vec::new();
        assert!(symbol_dictionary(&segment, &[], 1 << 20, &mut warnings).is_none());
        assert!(
            warnings.contains(&Warning::Jbig2SymbolLimitHit),
            "{warnings:?}"
        );
    }

    fn encode_arithmetic(
        source: &Bitmap,
        template: u8,
        tpgdon: bool,
        at: &[(i8, i8); 4],
    ) -> Vec<u8> {
        let mut encoder = MqEncoder::new(1 << template_bits(template));
        let mut ltp = 0u8;
        for y in 0..source.height {
            if tpgdon {
                let typical = y > 0 && rows_identical(source, y - 1, y);
                let sltp = u8::from(typical != (ltp == 1));
                encoder.encode_at(tpgdon_context(template), sltp);
                ltp ^= sltp;
                if ltp == 1 {
                    continue;
                }
            }
            for x in 0..source.width {
                let cx = context(source, template, at, x as i32, y as i32);
                encoder.encode_at(cx, source.get(x as i32, y as i32) as u8);
            }
        }
        encoder.flush()
    }

    /// A one-page embedded stream carrying one generic region at the origin.
    fn generic_region_stream(
        rows: &[&str],
        template: u8,
        tpgdon: bool,
        at: [(i8, i8); 4],
    ) -> Vec<u8> {
        let source = bitmap_from(rows);
        let mut data = Vec::new();
        // 7.4.1, the region segment information field.
        data.extend_from_slice(&source.width.to_be_bytes());
        data.extend_from_slice(&source.height.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes());
        data.push(0); // OR
                      // 7.4.6.2, the generic region flags.
        data.push((u8::from(tpgdon) << 3) | (template << 1));
        for (dx, dy) in at.iter().take(if template == 0 { 4 } else { 1 }) {
            data.push(*dx as u8);
            data.push(*dy as u8);
        }
        data.extend(encode_arithmetic(&source, template, tpgdon, &at));

        let info = page_info(source.width, source.height, 0);
        let mut stream = header(0, kind::PAGE_INFORMATION, 1, &info);
        stream.extend(header(1, kind::IMMEDIATE_GENERIC_REGION, 1, &data));
        stream
    }

    fn round_trip(rows: &[&str], template: u8, tpgdon: bool, at: [(i8, i8); 4]) -> Vec<String> {
        let stream = generic_region_stream(rows, template, tpgdon, at);
        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: rows.first().map_or(0, |row| row.len()) as u32,
            height: rows.len() as u32,
        };
        let bits = decode(&stream, &params, 1 << 20, &mut warnings).expect("a region was decoded");
        assert!(warnings.is_empty(), "a clean stream warned: {warnings:?}");
        picture(&bits, params.width, params.height)
    }

    /// A segment header (T.88 7.2) in its short form: no referred-to
    /// segments, one-byte page association.
    fn header(number: u32, kind: u8, page: u8, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&number.to_be_bytes());
        out.push(kind & 0x3F);
        out.push(0); // no referred-to segments, no retain flags
        out.push(page);
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        out
    }

    /// [`header`], for a segment that refers to others (7.2.4, 7.2.5).
    ///
    /// The referred-to numbers are one byte each here, which 7.2.5 allows for
    /// any segment numbered 256 or below — every fixture in this file is.
    fn header_referring(number: u32, kind: u8, page: u8, refers: &[u32], data: &[u8]) -> Vec<u8> {
        assert!(number <= 256 && refers.len() <= 4, "the short forms only");
        let mut out = Vec::new();
        out.extend_from_slice(&number.to_be_bytes());
        out.push(kind & 0x3F);
        out.push((refers.len() as u8) << 5);
        for referred in refers {
            out.push(*referred as u8);
        }
        out.push(page);
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        out
    }

    /// A page information segment's nineteen bytes (T.88 7.4.8).
    fn page_info(width: u32, height: u32, flags: u8) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes()); // x resolution
        out.extend_from_slice(&0u32.to_be_bytes()); // y resolution
        out.push(flags);
        out.extend_from_slice(&0u16.to_be_bytes()); // striping
        out
    }

    #[test]
    fn segments_enumerate_in_order() {
        let mut stream = header(0, kind::PAGE_INFORMATION, 1, &[1, 2, 3]);
        stream.extend(header(1, kind::IMMEDIATE_GENERIC_REGION, 1, &[4, 5]));
        stream.extend(header(2, kind::END_OF_PAGE, 1, &[]));

        let mut warnings = Vec::new();
        let parsed = segments(&stream, &mut warnings);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].kind, kind::PAGE_INFORMATION);
        assert_eq!(parsed[0].page, 1);
        assert_eq!(parsed[0].data, &[1, 2, 3]);
        assert_eq!(parsed[1].kind, kind::IMMEDIATE_GENERIC_REGION);
        assert_eq!(parsed[1].data, &[4, 5]);
        assert_eq!(parsed[2].data, &[] as &[u8]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_whole_file_pasted_into_a_stream_still_parses() {
        // D.4: file header, sequential organisation, number of pages known.
        let mut stream = FILE_HEADER.to_vec();
        stream.push(0b01); // sequential, page count present
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.extend(header(0, kind::PAGE_INFORMATION, 1, &[9]));

        let mut warnings = Vec::new();
        let parsed = segments(&stream, &mut warnings);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].data, &[9]);
    }

    #[test]
    fn random_access_organisation_is_recorded_rather_than_guessed() {
        let mut stream = FILE_HEADER.to_vec();
        stream.push(0b10); // random access, page count absent
        stream.extend(header(0, kind::PAGE_INFORMATION, 1, &[9]));

        let mut warnings = Vec::new();
        assert!(segments(&stream, &mut warnings).is_empty());
        assert_eq!(warnings, vec![Warning::Jbig2SegmentSkipped]);
    }

    #[test]
    fn long_form_referred_to_count_skips_its_retain_flags() {
        // 7.2.4 long form: 0b111 in the top three bits, count in the low 29,
        // then ceil((count + 1) / 8) retain bytes, then `count` one-byte
        // referred-to numbers.
        let mut stream = Vec::new();
        stream.extend_from_slice(&7u32.to_be_bytes()); // segment number 7
        stream.push(kind::IMMEDIATE_GENERIC_REGION);
        stream.extend_from_slice(&(0xE000_0000u32 | 9).to_be_bytes());
        stream.extend_from_slice(&[0u8; 2]); // ceil(10 / 8) retain bytes
        stream.extend_from_slice(&[0u8; 9]); // nine one-byte referred numbers
        stream.push(1); // page
        stream.extend_from_slice(&2u32.to_be_bytes());
        stream.extend_from_slice(&[0xAB, 0xCD]);

        let mut warnings = Vec::new();
        let parsed = segments(&stream, &mut warnings);
        assert_eq!(parsed.len(), 1, "the header's own length arithmetic is off");
        assert_eq!(parsed[0].data, &[0xAB, 0xCD]);
    }

    #[test]
    fn referred_to_numbers_widen_with_the_segment_number() {
        // 7.2.5: a segment numbered above 65536 refers with four-byte
        // numbers. Reading them as one byte each would put the page
        // association nine bytes early and invent a segment.
        let mut stream = Vec::new();
        stream.extend_from_slice(&70_000u32.to_be_bytes());
        stream.push(0x40 | kind::IMMEDIATE_GENERIC_REGION); // four-byte page
        stream.push(2 << 5); // two referred-to segments
        stream.extend_from_slice(&[0u8; 8]);
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.extend_from_slice(&1u32.to_be_bytes());
        stream.push(0x5A);

        let mut warnings = Vec::new();
        let parsed = segments(&stream, &mut warnings);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].page, 1);
        assert_eq!(
            parsed[0].data,
            &[0x5A],
            "the referred-to run was read at the wrong width, so the page \
             association and the length came from the middle of it"
        );
    }

    #[test]
    fn a_segment_claiming_more_than_the_stream_holds_is_truncated_and_named() {
        let mut stream = header(0, kind::IMMEDIATE_GENERIC_REGION, 1, &[1, 2, 3]);
        // Rewrite the length to claim far more than follows.
        let at = stream.len() - 4 - 3;
        stream[at..at + 4].copy_from_slice(&999u32.to_be_bytes());

        let mut warnings = Vec::new();
        let parsed = segments(&stream, &mut warnings);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].data, &[1, 2, 3]);
        assert_eq!(warnings, vec![Warning::TruncatedInput]);
    }

    #[test]
    fn an_unknown_data_length_ends_the_stream_rather_than_guessing() {
        let mut stream = header(0, kind::IMMEDIATE_GENERIC_REGION, 1, &[1]);
        let at = stream.len() - 5;
        stream[at..at + 4].copy_from_slice(&u32::MAX.to_be_bytes());

        let mut warnings = Vec::new();
        assert!(segments(&stream, &mut warnings).is_empty());
        assert_eq!(warnings, vec![Warning::Jbig2SegmentSkipped]);
    }

    /// The lineage this plan does not build, named rather than absorbed.
    #[test]
    fn a_symbol_dictionary_file_refuses_and_says_so() {
        let mut stream = header(0, kind::PAGE_INFORMATION, 1, &page_info(64, 56, 0));
        stream.extend(header(1, kind::SYMBOL_DICTIONARY, 1, &[0u8; 4]));
        stream.extend(header(2, kind::IMMEDIATE_TEXT_REGION, 1, &[0u8; 4]));

        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: 64,
            height: 56,
        };
        let out = decode(&stream, &params, 1 << 20, &mut warnings);
        assert_eq!(out, Err(FilterError::Unsupported(Capability::Jbig2)));
        assert!(warnings.contains(&Warning::Jbig2SegmentSkipped));
    }

    /// A globals stream's segments are read, and read *first*.
    #[test]
    fn globals_segments_are_enumerated_before_the_stream_s_own() {
        let globals = header(0, kind::SYMBOL_DICTIONARY, 0, &[0u8; 4]);
        let stream = header(1, kind::PAGE_INFORMATION, 1, &page_info(8, 8, 0));

        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &globals,
            width: 8,
            height: 8,
        };
        assert_eq!(
            decode(&stream, &params, 1 << 20, &mut warnings),
            Err(FilterError::Unsupported(Capability::Jbig2))
        );
        assert_eq!(
            warnings,
            vec![Warning::TruncatedInput, Warning::Jbig2SegmentSkipped],
            "the globals' symbol dictionary has to be seen, or a file whose \
             whole payload is shared would refuse without saying why"
        );
        // `TruncatedInput` is the stronger form of what this always
        // asserted. The four bytes here stand in for a dictionary, and
        // while nothing decoded one they were skipped unread; now that
        // clause 6.5 runs, the same bytes are read far enough to be short
        // of an AT pixel. A segment cannot be found truncated without
        // having been reached, so the enumeration order this test is named
        // for is what put it there.
    }

    /// 7.4.8.5 bit 2: a page that starts black.
    ///
    /// The refusal hides the page from a caller, so the bitmap is checked
    /// through the one thing that survives it — a region composited onto it
    /// arrives in the next milestone, so for now the fill is checked where it
    /// happens.
    #[test]
    fn the_page_default_pixel_value_starts_the_page_black() {
        let mut page = Page {
            intermediate: BTreeMap::new(),
            symbols: BTreeMap::new(),
            seen: BTreeSet::new(),
            refused: BTreeSet::new(),
            bitmap: Bitmap::new(8, 8, 64).expect("eight by eight"),
            number: None,
            regions: 0,
        };
        let data = page_info(8, 8, 0x04);
        let segment = Segment {
            number: 1,
            referred: Vec::new(),
            kind: kind::PAGE_INFORMATION,
            page: 1,
            data: &data,
        };
        let mut warnings = Vec::new();
        page.begin(&segment, &mut warnings);
        assert_eq!(page.bitmap.bits, vec![0xFF; 8], "1 is black (6.2.2)");
        assert_eq!(page.number, Some(1));
        assert!(warnings.is_empty());
    }

    #[test]
    fn a_segment_for_another_page_is_not_composited_onto_this_one() {
        let data = page_info(8, 8, 0);
        let mut page = Page {
            intermediate: BTreeMap::new(),
            symbols: BTreeMap::new(),
            seen: BTreeSet::new(),
            refused: BTreeSet::new(),
            bitmap: Bitmap::new(8, 8, 64).expect("eight by eight"),
            number: None,
            regions: 0,
        };
        let mut warnings = Vec::new();
        page.begin(
            &Segment {
                number: 1,
                referred: Vec::new(),
                kind: kind::PAGE_INFORMATION,
                page: 1,
                data: &data,
            },
            &mut warnings,
        );
        let elsewhere = Segment {
            number: 1,
            referred: Vec::new(),
            kind: kind::IMMEDIATE_GENERIC_REGION,
            page: 2,
            data: &[],
        };
        let globalish = Segment {
            number: 1,
            referred: Vec::new(),
            kind: kind::IMMEDIATE_GENERIC_REGION,
            page: 0,
            data: &[],
        };
        assert!(!page.owns(&elsewhere));
        assert!(page.owns(&globalish));
    }

    #[test]
    fn a_region_declaring_four_billion_pixels_is_refused_before_allocating() {
        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: u32::MAX,
            height: u32::MAX,
        };
        let out = decode(&[], &params, 1 << 20, &mut warnings);
        assert_eq!(out, Err(FilterError::Unsupported(Capability::Jbig2)));
        assert_eq!(warnings, vec![Warning::Jbig2RegionTooLarge]);
    }

    /// Both halves of the bound, because they catch on different targets.
    ///
    /// `checked_mul` is what saves a 32-bit `usize` — wasm32 is a first-class
    /// target here — where `2^32 / 8 * 2^32` does not fit at all. On a 64-bit
    /// one it fits comfortably, as two exabytes, and only the ceiling refuses
    /// it. A test that asserted the multiply alone would pass on wasm and
    /// prove nothing on the machine most of this is built on.
    #[test]
    fn packed_size_refuses_what_it_cannot_multiply_or_cannot_afford() {
        assert_eq!(packed_size(8, 2, 1024), Some(2));
        assert_eq!(packed_size(9, 2, 1024), Some(4));
        assert_eq!(packed_size(0, 2, 1024), None);
        assert_eq!(packed_size(8, 0, 1024), None);
        assert_eq!(packed_size(64, 64, 100), None, "past the ceiling");
        assert_eq!(
            packed_size(u32::MAX, u32::MAX, 1 << 28),
            None,
            "2^32 by 2^32 must not reach an allocator on any target"
        );
        #[cfg(target_pointer_width = "32")]
        assert_eq!(packed_size(u32::MAX, u32::MAX, usize::MAX), None);
    }

    /// **The milestone.** T.88 Annex H.1's arithmetically coded generic
    /// region — template 0, TPGDON on, the nominal AT pixels written out in
    /// full — against the picture the annex publishes for it.
    ///
    /// Nine bytes of coded data for 2 376 pixels, which is what makes this
    /// worth having: almost every row runs through 6.2.5.7's
    /// typical-prediction path, so the picture only appears at all if the
    /// coder, the template's pixel *set*, the AT positions, TPGDON's row copy
    /// and the composition coordinates are all right at once.
    ///
    /// Measured, by injection, against what it does and does not catch:
    /// inverting the polarity, dropping a row of the Qe table and ignoring
    /// the TPGDON flag each fail it. Transposing two context bits and moving
    /// the pseudo-context to `0x9B24` do not — see
    /// [`Self::template_0_context_bits_match_the_figure`]. Knowing which is
    /// which is the difference between a fixture and a fixture one believes.
    #[test]
    fn annex_h_generic_region_decodes_to_its_published_bitmap() {
        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: 64,
            height: 56,
        };
        let bits = decode(&ANNEX_H[PAGE_2], &params, 1 << 20, &mut warnings)
            .expect("the page carries a generic region, so it is not refused");

        let page = picture(&bits, 64, 56);
        assert_eq!(region_window(&page), ANNEX_H_REGION);

        // And nothing outside the region was touched: the rest of Annex H.1's
        // page is a text region and a halftone region, neither of which this
        // build draws.
        assert!(
            page[0..11].iter().all(|row| !row.contains('#')),
            "a region composited above the coordinates its segment named"
        );
        assert!(
            page[11..55].iter().all(|row| !row[0..4].contains('#')),
            "a region composited left of the coordinates its segment named"
        );
        assert!(
            warnings.contains(&Warning::Jbig2SegmentSkipped),
            "the symbol dictionary, text region and halftone region on this \
             page are all missing from it, and ruling 10 wants that recorded"
        );
    }

    /// The region lands where 7.4.1 says, not at the origin.
    #[test]
    fn a_region_is_composited_at_the_coordinates_its_segment_names() {
        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: 64,
            height: 56,
        };
        let bits = decode(&ANNEX_H[PAGE_2], &params, 1 << 20, &mut warnings).expect("decodes");
        let page = picture(&bits, 64, 56);
        assert_eq!(
            page[11], "....######################################################......",
            "the frame's first row starts at x = 4 and is 54 wide"
        );
        assert_eq!(page[10], ".".repeat(64), "row 10 is above the region");
        assert_eq!(page[55], ".".repeat(64), "row 55 is below it");
    }

    /// **Annex H codes the same two symbols both ways, and they come out
    /// identical.** This is the standard adjudicating the Huffman variant.
    ///
    /// Segment 2 carries them with `SDHUFF = 1` — height classes, a collective
    /// bitmap, and Tables B.1, B.2 and B.4 — and segment 9 carries them with
    /// `SDHUFF = 0`, through the MQ coder and Annex A's integer decoders. The
    /// two share no code below `Segment`, so agreeing on the exact pixels of a
    /// 'c' and an 'a' is not something a wrong prefix length can do by
    /// accident: a table off by one bit desynchronises the reader and produces
    /// noise, not a glyph.
    ///
    /// It matters because the tables in this module are **reconstructed**
    /// rather than transcribed from a copy of T.88, and this is what stands in
    /// for the transcription. What it covers is exactly the dictionary: B.1's
    /// sizes, B.2's width deltas, B.4's height deltas, 6.5.9's collective
    /// bitmap and 6.5.10's export runs. The text region's own three tables are
    /// **not** covered by it — see the note below.
    #[test]
    fn annex_h_codes_the_same_symbols_two_ways() {
        let all = segments(&ANNEX_H[13..682], &mut Vec::new());
        let decode_of = |number: u32| {
            let segment = all
                .iter()
                .find(|s| s.number == number)
                .expect("the segment");
            let mut warnings = Vec::new();
            let symbols = symbol_dictionary(segment, &[], 1 << 20, &mut warnings)
                .expect("a symbol dictionary");
            assert!(warnings.is_empty(), "segment {number}: {warnings:?}");
            symbols
        };

        let huffman = decode_of(2);
        let arithmetic = decode_of(9);
        assert_eq!(huffman.len(), 2, "the annex puts two symbols in each");
        assert_eq!(
            huffman, arithmetic,
            "the Huffman and arithmetic symbol dictionaries disagree, which \
             means a reconstructed Annex B table is wrong"
        );

        // And they are glyphs rather than noise, said as a shape so a failure
        // shows what came out instead of a byte count.
        assert_eq!(
            (huffman[0].width, huffman[0].height),
            (6, 6),
            "the annex's symbols are six by six"
        );
    }

    /// **The generic region is coded both ways and both agree**, pixel for
    /// pixel, against the picture the annex publishes.
    ///
    /// One goes through [`T6Rows`] and the T.6 mode codes; the other through
    /// the MQ coder and template 0. For them to agree on the region by
    /// accident is not a thing that happens.
    ///
    /// # What this test used to claim, and why it stopped
    ///
    /// It compared the two pages *whole* — and passed, which looked like the
    /// strongest assertion in the file. It was not: both pages' text regions
    /// were being skipped, so the comparison was over the generic region and
    /// two identical expanses of white. The moment the Huffman variant landed
    /// and page 1's text began to draw, the pages stopped matching, because
    /// **Annex H's three pages do not draw the same text** — page 1 sets one
    /// arrangement of its symbols and page 2 another. Only the generic region
    /// is the same picture twice, and that is now all this claims.
    ///
    /// The cross-check the whole-page comparison was standing in for is
    /// `annex_h_codes_the_same_symbols_two_ways`, which is a real one.
    #[test]
    fn annex_h_codes_one_picture_twice_and_both_ways_agree() {
        let params = Jbig2Params {
            globals: &[],
            width: 64,
            height: 56,
        };
        let mut mmr_warnings = Vec::new();
        let mmr = decode(&ANNEX_H[PAGE_1], &params, 1 << 20, &mut mmr_warnings)
            .expect("page 1 carries an MMR generic region");
        // Page 2's text region refers to segment 0, which sits on page 0 and is
        // therefore shared: in a PDF it arrives through `/JBIG2Globals`, and
        // here it is the bytes before page 1 begins.
        let shared = Jbig2Params {
            globals: &ANNEX_H[SHARED_DICTIONARY],
            ..params
        };
        let mut arithmetic_warnings = Vec::new();
        let arithmetic = decode(&ANNEX_H[PAGE_2], &shared, 1 << 20, &mut arithmetic_warnings)
            .expect("page 2 carries an arithmetically coded one");

        assert_eq!(
            region_window(&picture(&mmr, 64, 56)),
            ANNEX_H_REGION,
            "the MMR region does not match the annex's published picture"
        );
        assert_eq!(
            region_window(&picture(&arithmetic, 64, 56)),
            ANNEX_H_REGION,
            "the arithmetic region does not match the annex's published picture"
        );
        assert!(!mmr_warnings.contains(&Warning::TruncatedInput));
        assert!(!arithmetic_warnings.contains(&Warning::TruncatedInput));
    }

    /// The whole file, file header and all, the way a producer that pasted a
    /// standalone JBIG2 into a PDF stream would deliver it.
    ///
    /// D.4's sequential organisation puts page 1 first, so this is the MMR
    /// page again — reached this time through the eight-byte identifier and
    /// the page count rather than by slicing.
    #[test]
    fn the_whole_annex_h_file_decodes_its_first_page() {
        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: 64,
            height: 56,
        };
        let bits = decode(&ANNEX_H, &params, 1 << 20, &mut warnings).expect("page 1 decodes");
        assert_eq!(region_window(&picture(&bits, 64, 56)), ANNEX_H_REGION);
        assert!(
            warnings.contains(&Warning::Jbig2SegmentSkipped),
            "pages 2 and 3, and this page's text and halftone regions, are \
             all missing from the result"
        );
    }

    /// An MMR region whose data is not T.6 at all decodes no row, so it is
    /// not a region.
    ///
    /// The refusal's sharpest edge: a region that *was* recognised and *was*
    /// sized and produced nothing is the one case where counting it before
    /// decoding it hands back a plausible blank page.
    #[test]
    fn an_mmr_region_that_decodes_no_row_is_not_a_region() {
        let mut data = Vec::new();
        data.extend_from_slice(&16u32.to_be_bytes()); // width
        data.extend_from_slice(&16u32.to_be_bytes()); // height
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&0u32.to_be_bytes());
        data.push(0); // OR
        data.push(0x01); // MMR
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

        let mut stream = header(0, kind::PAGE_INFORMATION, 1, &page_info(16, 16, 0));
        stream.extend(header(1, kind::IMMEDIATE_GENERIC_REGION, 1, &data));

        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: 16,
            height: 16,
        };
        assert_eq!(
            decode(&stream, &params, 1 << 20, &mut warnings),
            Err(FilterError::Unsupported(Capability::Jbig2))
        );
        assert!(warnings.contains(&Warning::Jbig2SegmentSkipped));
    }

    /// 6.2.5.3's nominal AT positions, which Annex H.1 writes out explicitly
    /// and every other test here relies on.
    #[test]
    fn nominal_at_pixels_are_the_ones_the_standard_draws() {
        assert_eq!(NOMINAL_AT[0], [(3, -1), (-3, -1), (2, -2), (-2, -2)]);
        assert_eq!(NOMINAL_AT[1][0], (3, -1));
        assert_eq!(NOMINAL_AT[2][0], (2, -1));
        assert_eq!(NOMINAL_AT[3][0], (2, -1));
    }

    /// 6.2.5.7's pseudo-contexts, and that each fits the template it belongs
    /// to.
    ///
    /// The second half is what makes this more than a transcription check: a
    /// pseudo-context outside its own template's context array would index
    /// past the end, and [`MqDecoder::decode_at`] answers 0 there rather than
    /// panicking — so the failure would be a decoder that quietly never sees
    /// a typical row.
    #[test]
    fn tpgdon_pseudo_contexts_are_the_values_the_standard_fixes() {
        assert_eq!(tpgdon_context(0), 0x9B25);
        assert_eq!(tpgdon_context(1), 0x0795);
        assert_eq!(tpgdon_context(2), 0x00E5);
        assert_eq!(tpgdon_context(3), 0x0195);
        for template in 0..4u8 {
            assert!(
                tpgdon_context(template) < (1 << template_bits(template)),
                "template {template}'s pseudo-context is outside its own array"
            );
        }
    }

    /// T.88 Figures 8 to 11, transcribed pixel by pixel, for all four
    /// templates.
    ///
    /// **No datastream can replace this test, and finding that out is the
    /// reason it is written this way.** Relabelling the context bits is a
    /// bijection on the context array; every state starts identical, so an
    /// encoder's slot histories and a decoder's stay in step under any
    /// permutation. Transposing bits 0 and 1 was injected here: Annex H.1
    /// still decoded to its published picture byte for byte, and every
    /// round-trip below still passed. Only this assertion moved.
    ///
    /// That does not make the order cosmetic, because 6.2.5.7's
    /// pseudo-context is a *literal* slot number. Once TPGDON is on, the SLTP
    /// decision shares the array with whichever neighbourhood the numbering
    /// puts at `0x9B25` -- so the numbering stops being a free choice and
    /// becomes part of what the encoder agreed to.
    ///
    /// It pins two things a permutation cannot hide as well: *which* pixels a
    /// template reads at all, and how many bits it forms. Either of those
    /// wrong destroys any picture at all.
    #[test]
    fn template_context_bits_match_the_figures() {
        // (dx, dy) of the neighbour, and the bit the figure puts it in. The
        // AT entries sit at their nominal positions, which is where the
        // figures draw them.
        let figures: [&[((i32, i32), u32)]; 4] = [
            &[
                ((-1, 0), 0),
                ((-2, 0), 1),
                ((-3, 0), 2),
                ((-4, 0), 3),
                ((3, -1), 4), // A1
                ((2, -1), 5),
                ((1, -1), 6),
                ((0, -1), 7),
                ((-1, -1), 8),
                ((-2, -1), 9),
                ((-3, -1), 10), // A2
                ((2, -2), 11),  // A3
                ((1, -2), 12),
                ((0, -2), 13),
                ((-1, -2), 14),
                ((-2, -2), 15), // A4
            ],
            &[
                ((-1, 0), 0),
                ((-2, 0), 1),
                ((-3, 0), 2),
                ((3, -1), 3), // A1
                ((2, -1), 4),
                ((1, -1), 5),
                ((0, -1), 6),
                ((-1, -1), 7),
                ((-2, -1), 8),
                ((2, -2), 9),
                ((1, -2), 10),
                ((0, -2), 11),
                ((-1, -2), 12),
            ],
            &[
                ((-1, 0), 0),
                ((-2, 0), 1),
                ((2, -1), 2), // A1
                ((1, -1), 3),
                ((0, -1), 4),
                ((-1, -1), 5),
                ((-2, -1), 6),
                ((1, -2), 7),
                ((0, -2), 8),
                ((-1, -2), 9),
            ],
            &[
                ((-1, 0), 0),
                ((-2, 0), 1),
                ((-3, 0), 2),
                ((-4, 0), 3),
                ((2, -1), 4), // A1
                ((1, -1), 5),
                ((0, -1), 6),
                ((-1, -1), 7),
                ((-2, -1), 8),
                ((-3, -1), 9),
            ],
        ];

        for (template, places) in figures.iter().enumerate() {
            let template = template as u8;
            let at = NOMINAL_AT[template as usize];
            assert_eq!(
                places.len(),
                template_bits(template),
                "template {template} forms {} context bits and its figure                  names {} pixels; one of the two is wrong",
                template_bits(template),
                places.len()
            );
            for ((dx, dy), bit) in places.iter().copied() {
                let mut bitmap = Bitmap::new(16, 16, 1 << 10).expect("small");
                bitmap.set((8 + dx) as u32, (8 + dy) as u32, 1);
                assert_eq!(
                    context(&bitmap, template, &at, 8, 8),
                    1 << bit,
                    "template {template}: the pixel at ({dx}, {dy}) belongs                      in bit {bit}"
                );
            }
        }
    }

    /// **The milestone.** Every template puts a hand-built picture back
    /// exactly as it went in, through the encoder Annex H.2 pins.
    #[test]
    fn every_template_round_trips_a_hand_built_region() {
        for template in 0..4u8 {
            assert_eq!(
                round_trip(&SPECIMEN, template, false, NOMINAL_AT[template as usize]),
                SPECIMEN,
                "template {template} did not survive a round-trip"
            );
        }
    }

    /// The same four with typical prediction on.
    ///
    /// [`SPECIMEN`] carries three runs of identical rows, so LTP toggles on
    /// and off several times rather than staying where it started. A decoder
    /// that read the SLTP bit and never acted on it, or acted on it once,
    /// gets a different picture rather than a slightly wrong one.
    #[test]
    fn every_template_round_trips_with_typical_prediction_on() {
        for template in 0..4u8 {
            assert_eq!(
                round_trip(&SPECIMEN, template, true, NOMINAL_AT[template as usize]),
                SPECIMEN,
                "template {template} did not survive TPGDON"
            );
        }
    }

    /// The AT pixels the segment names, rather than the ones 6.2.5.3
    /// nominates.
    ///
    /// This is the assertion that catches an AT pixel left at its default
    /// when the header said otherwise. The encoder used [`CUSTOM_AT`]; a
    /// decoder that fell back to [`NOMINAL_AT`], or read the pairs and threw
    /// them away, forms a different context for every pixel of every row and
    /// the picture does not come back at all.
    #[test]
    fn every_template_round_trips_with_the_at_pixels_the_segment_names() {
        for template in 0..4u8 {
            for tpgdon in [false, true] {
                assert_eq!(
                    round_trip(&SPECIMEN, template, tpgdon, CUSTOM_AT),
                    SPECIMEN,
                    "template {template} ignored its AT pixels (TPGDON {tpgdon})"
                );
            }
        }
    }

    /// A region one row tall, and one a single column wide.
    ///
    /// Every template reads two rows up and as many as four columns either
    /// side, so almost every context here is 6.2.5.2's outside-the-region
    /// rule rather than real pixels. A one-column region is also what the
    /// last stripe of a striped page can degenerate to.
    #[test]
    fn a_region_at_the_edge_of_its_own_dimensions_round_trips() {
        assert_eq!(
            round_trip(&["#.#.#.#."], 0, false, NOMINAL_AT[0]),
            ["#.#.#.#."]
        );
        assert_eq!(
            round_trip(&["#", ".", "#", "#", "."], 2, true, NOMINAL_AT[2]),
            ["#", ".", "#", "#", "."]
        );
    }

    #[test]
    fn arbitrary_bytes_never_panic() {
        let mut seed = 0x1234_5678u32;
        for _ in 0..2048 {
            let len = (seed % 96) as usize;
            let mut bytes = Vec::with_capacity(len);
            for _ in 0..len {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                bytes.push((seed >> 16) as u8);
            }
            let mut warnings = Vec::new();
            let params = Jbig2Params {
                globals: &bytes,
                width: 32,
                height: 32,
            };
            let _ = decode(&bytes, &params, 1 << 16, &mut warnings);
        }
    }

    /// **T.88 Annex H.1's page 3 decodes, and it decodes as text.**
    ///
    /// Page 3 is the annex's refinement page, and reaching this picture needs
    /// every part of clause 6.3 at once: 6.5.8.2.2's single refinement,
    /// 6.5.8.2.1's aggregate — a symbol that is itself a text region —
    /// 6.4.11's per-instance refinement inside the region that draws them, and
    /// both of 6.3.5.3's templates, since the dictionary codes at `SDRTEMPLATE`
    /// 0 and the region at `SBRTEMPLATE` 1.
    ///
    /// The assertion is the whole page rather than a count because the point
    /// is *legibility*: a refinement template wrong in one position leaves the
    /// arithmetic decoder in step for a while and then produces noise, and
    /// noise is what this test exists to tell apart from letters. The third
    /// glyph's descender is the useful detail — it is two rows below the
    /// baseline that everything else sits on, so a decoder that had merely
    /// stayed in step would not have put it there.
    #[test]
    fn annex_h_page_3_decodes_its_refined_text() {
        let params = Jbig2Params {
            globals: &[],
            width: 37,
            height: 8,
        };
        let mut warnings = Vec::new();
        let bits = decode(&ANNEX_H[PAGE_3], &params, 1 << 20, &mut warnings)
            .expect("page 3 carries a refined text region");
        assert_eq!(
            picture(&bits, 37, 8),
            [
                ".####....####...####....####....####.",
                "#....#.......#..#...#.......#..#....#",
                "#........#####..#...#...#####..#.....",
                "#.......#....#..#...#..#....#..#.....",
                "#....#..#....#..####...#....#..#....#",
                ".####....#####..#.......#####...####.",
                "................#....................",
                "................#....................",
            ]
        );
        assert!(warnings.is_empty(), "page 3 warned: {warnings:?}");
    }

    /// **6.5.8.2's two roads, told apart by what they produce.**
    ///
    /// Annex H's page 3 dictionary imports one symbol and decodes two, and the
    /// two take different roads: `REFAGGNINST = 1` refines the imported letter
    /// into another letter, and `REFAGGNINST = 2` builds a symbol that is a
    /// whole text region — two instances placed side by side.
    ///
    /// Asserting the bitmaps rather than the count is what makes this evidence.
    /// The aggregate is the pair of the other two, in order and correctly
    /// spaced, which is a coincidence no desynchronised decoder produces.
    #[test]
    fn annex_h_page_3_refines_one_symbol_and_aggregates_another() {
        let all = segments(&ANNEX_H[13..], &mut Vec::new());
        let dictionary = |number: u32| {
            all.iter()
                .find(|segment| segment.number == number)
                .expect("segment")
        };
        let imported = symbol_dictionary(dictionary(16), &[], 1 << 20, &mut Vec::new())
            .expect("the shared dictionary decodes");
        let mut warnings = Vec::new();
        let exported = symbol_dictionary(dictionary(17), &imported, 1 << 20, &mut warnings)
            .expect("the refining dictionary decodes");

        assert_eq!(exported.len(), 3, "one imported symbol and two new ones");
        // The import, untouched.
        assert_eq!(
            bitmap_rows(&exported[0]),
            [".####.", ".....#", ".#####", "#....#", "#....#", ".#####"]
        );
        // 6.5.8.2.2: a refinement of it, at `IARDX` = `IARDY` = 0.
        assert_eq!(
            bitmap_rows(&exported[1]),
            [".####.", "#....#", "#.....", "#.....", "#....#", ".####."]
        );
        // 6.5.8.2.1: an aggregate of the two above, which is why it is exactly
        // twice as wide plus the two columns between them.
        assert_eq!(
            bitmap_rows(&exported[2]),
            [
                ".####....####.",
                ".....#..#....#",
                ".#####..#.....",
                "#....#..#.....",
                "#....#..#....#",
                ".#####...####.",
            ]
        );
        assert!(warnings.is_empty(), "the dictionary warned: {warnings:?}");
    }

    /// **The refinement context's bit order is a free choice, and this proves
    /// it** — which is the argument [`refinement_context`] rests on.
    ///
    /// A context index only ever names an adaptive state slot: the decoder
    /// reads and writes `state[cx]`, every slot starts identical, and A and C
    /// are global. So relabelling every context through a bijection cannot
    /// change a single decision. Here the same thirteen positions are given to
    /// the decoder in two different orders over the same coded bytes, and the
    /// two bitmaps have to be identical.
    ///
    /// If this ever fails, the file's own bit order has stopped being a
    /// bijection — a position repeated or dropped — and the templates below
    /// are no longer the sets they claim to be.
    #[test]
    fn a_relabelled_refinement_template_decodes_identically() {
        let reference = bitmap_from(&[".####.", ".....#", ".#####", "#....#", "#....#", ".#####"]);
        // Any bytes will do: the claim is that two orders agree, not that
        // either decodes anything in particular.
        let coded: [u8; 24] = [
            0x4F, 0xE7, 0x8D, 0x68, 0x1B, 0xA5, 0x3C, 0x91, 0x07, 0xF2, 0x40, 0x8E, 0xD3, 0x66,
            0xAA, 0x19, 0x5C, 0xB0, 0x27, 0xE1, 0x74, 0x9F, 0x38, 0xC6,
        ];
        let straight = RefineTemplate {
            here: &REFINE_0_HERE,
            there: &REFINE_0_THERE,
            at: Some(NOMINAL_REFINE_AT),
            typical: TPGRON_0,
        };
        // The same set, read in the opposite order within each layer and with
        // the layers swapped over — a different index for every neighbourhood.
        let here: Vec<(i8, i8)> = REFINE_0_HERE.iter().rev().copied().collect();
        let there: Vec<(i8, i8)> = REFINE_0_THERE.iter().rev().copied().collect();
        let relabelled = RefineTemplate {
            here: &here,
            there: &there,
            at: Some(NOMINAL_REFINE_AT),
            typical: TPGRON_0,
        };
        assert_eq!(straight.bits(), relabelled.bits());

        let decode_with = |template: &RefineTemplate<'_>| {
            let mut coder = MqDecoder::new(&coded);
            let mut contexts = MqContexts::new(1 << template.bits());
            let mut into = Bitmap::new(6, 6, 1 << 20).expect("bitmap");
            decode_refinement_into(
                &mut coder,
                &mut contexts,
                template,
                false,
                &reference,
                (0, 0),
                &mut into,
            );
            bitmap_rows(&into)
        };
        assert_eq!(decode_with(&straight), decode_with(&relabelled));
    }

    /// The rows of a bitmap, as `#` and `.`.
    fn bitmap_rows(bitmap: &Bitmap) -> Vec<String> {
        (0..bitmap.height)
            .map(|y| {
                (0..bitmap.width)
                    .map(|x| {
                        if bitmap.get(x as i32, y as i32) == 1 {
                            '#'
                        } else {
                            '.'
                        }
                    })
                    .collect()
            })
            .collect()
    }
}
