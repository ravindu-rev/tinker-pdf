//! JBIG2 (ITU-T T.88): the generic-region lineage.
//!
//! Scope and the reasoning behind it: `docs/plans/gaps/17-jbig2-generic-region.md`.
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
/// The segment *number* is not kept: 7.2.5 uses it only to decide how wide
/// the referred-to numbers in this same header are, and nothing this build
/// decodes follows a reference. Keeping a field nothing reads is how a
/// half-parsed header comes to look complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Segment<'a> {
    /// 7.2.3, the low six bits of the header flags.
    kind: u8,
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
    // or the header is not a header.
    let referred_bytes = (count as usize).checked_mul(width)?;
    reader.skip(referred_bytes)?;

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

    Some(Segment { kind, page, data })
}

/// Whether a segment type is one this build decodes.
///
/// Everything else is skipped **and recorded**, which is what keeps the
/// refusal honest: the skip is not silent, and it is not sufficient on its
/// own — a page that ends with no region on it refuses regardless.
fn understood(kind: u8) -> bool {
    matches!(
        kind,
        kind::IMMEDIATE_GENERIC_REGION
            | kind::IMMEDIATE_LOSSLESS_GENERIC_REGION
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
        kind::INTERMEDIATE_GENERIC_REGION
            | kind::SYMBOL_DICTIONARY
            | kind::INTERMEDIATE_TEXT_REGION
            | kind::IMMEDIATE_TEXT_REGION
            | kind::IMMEDIATE_LOSSLESS_TEXT_REGION
            | kind::PATTERN_DICTIONARY
            | kind::INTERMEDIATE_HALFTONE_REGION
            | kind::IMMEDIATE_HALFTONE_REGION
            | kind::IMMEDIATE_LOSSLESS_HALFTONE_REGION
            | kind::INTERMEDIATE_REFINEMENT_REGION
            | kind::IMMEDIATE_REFINEMENT_REGION
            | kind::IMMEDIATE_LOSSLESS_REFINEMENT_REGION
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
                let value = match op {
                    1 => s & d,
                    2 => s ^ d,
                    3 => !(s ^ d),
                    4 => s,
                    // 0 is OR, and so is anything 7.4.1.5 leaves undefined:
                    // a region drawn with an operator nobody defined should
                    // still appear rather than erase what is under it.
                    _ => s | d,
                };
                self.set(dx, dy, value);
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
    let mut ltp = 0u8;

    for y in 0..into.height {
        if tpgdon {
            // 6.2.5.7: one decision per row against a context the standard
            // fixes, toggling "this row is the same as the last one".
            ltp ^= coder.decode_at(&mut contexts, tpgdon_context(template));
            if ltp == 1 {
                if y > 0 {
                    into.copy_row(y - 1, y);
                }
                continue;
            }
        }
        for x in 0..into.width {
            let cx = context(into, template, at, x as i32, y as i32);
            let pixel = coder.decode_at(&mut contexts, cx);
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

    if mmr {
        // MMR generic regions arrive in the next milestone. Refusing here
        // rather than compositing an undecoded bitmap is deliberate: a blank
        // region counted as a region would turn the refusal into a blank page
        // reported as success.
        note(warnings, Warning::Jbig2SegmentSkipped);
        return None;
    }

    let Some(mut bitmap) = Bitmap::new(info.width, info.height, ceiling) else {
        note(warnings, Warning::Jbig2RegionTooLarge);
        return None;
    };
    decode_arithmetic(reader.rest(), template, tpgdon, &at, &mut bitmap);
    Some((info, bitmap))
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
        match segment.kind {
            kind::PAGE_INFORMATION => page.begin(segment, warnings),
            kind::IMMEDIATE_GENERIC_REGION | kind::IMMEDIATE_LOSSLESS_GENERIC_REGION => {
                page.draw_generic(segment, max_output, warnings);
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

/// The page bitmap regions are composited onto, and what the page
/// information segment said about it.
struct Page {
    /// Packed 1-bpp rows, most significant bit first, 1 = black.
    bitmap: Bitmap,
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
    use super::*;

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
    const PAGE_1: std::ops::Range<usize> = 13..400;
    const PAGE_2: std::ops::Range<usize> = 400..682;

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
            vec![Warning::Jbig2SegmentSkipped],
            "the globals' symbol dictionary has to be seen, or a file whose \
             whole payload is shared would refuse without saying why"
        );
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
            bitmap: Bitmap::new(8, 8, 64).expect("eight by eight"),
            number: None,
            regions: 0,
        };
        let data = page_info(8, 8, 0x04);
        let segment = Segment {
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
            bitmap: Bitmap::new(8, 8, 64).expect("eight by eight"),
            number: None,
            regions: 0,
        };
        let mut warnings = Vec::new();
        page.begin(
            &Segment {
                kind: kind::PAGE_INFORMATION,
                page: 1,
                data: &data,
            },
            &mut warnings,
        );
        let elsewhere = Segment {
            kind: kind::IMMEDIATE_GENERIC_REGION,
            page: 2,
            data: &[],
        };
        let globalish = Segment {
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

    /// Until MMR arrives, an MMR region must refuse rather than composite the
    /// blank bitmap it allocated.
    ///
    /// This is the refusal's sharpest edge: a region that *was* recognised,
    /// *was* sized, and produced nothing is the one case where counting a
    /// region before decoding it would hand back a plausible blank page.
    #[test]
    fn an_undecoded_mmr_region_does_not_count_as_a_region() {
        let mut warnings = Vec::new();
        let params = Jbig2Params {
            globals: &[],
            width: 64,
            height: 56,
        };
        assert_eq!(
            decode(&ANNEX_H[PAGE_1], &params, 1 << 20, &mut warnings),
            Err(FilterError::Unsupported(Capability::Jbig2)),
            "Annex H.1's first page codes its region with MMR"
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

    /// T.88 Figure 8's numbering, transcribed pixel by pixel.
    ///
    /// **No datastream can replace this test, and finding that out is the
    /// reason it is written this way.** Relabelling the context bits is a
    /// bijection on the context array; every state starts identical, so an
    /// encoder's slot histories and a decoder's stay in step under any
    /// permutation. Transposing bits 0 and 1 was injected here and Annex H.1
    /// still decoded to its published picture, byte for byte. Only this
    /// assertion moved.
    ///
    /// That does not make the order cosmetic, because 6.2.5.7's
    /// pseudo-context is a *literal* slot number. Once TPGDON is on, the SLTP
    /// decision shares the array with whichever neighbourhood the numbering
    /// puts at `0x9B25` — so the numbering stops being a free choice and
    /// becomes part of what the encoder agreed to.
    #[test]
    fn template_0_context_bits_match_the_figure() {
        let at = NOMINAL_AT[0];
        // (dx, dy) of the neighbour, and the bit T.88 Figure 8 puts it in.
        let places = [
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
        ];
        for ((dx, dy), bit) in places {
            let mut bitmap = Bitmap::new(16, 16, 1 << 10).expect("small");
            bitmap.set((8 + dx) as u32, (8 + dy) as u32, 1);
            assert_eq!(
                context(&bitmap, 0, &at, 8, 8),
                1 << bit,
                "the pixel at ({dx}, {dy}) belongs in bit {bit}"
            );
        }
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
}
