//! Fixtures written byte by byte from TIFF 6.0, and the decoder held to them.
//!
//! There is no `.tif` in this repository and none is fetched, so every file
//! below is assembled from the specification's own field layouts — the eight
//! byte header of p.13, the twelve byte directory entry of p.14, the strip and
//! tile geometry of p.39 and p.67 — and every coded strip from the coding
//! specification that owns it. A reader tested only against what its own
//! writer emits proves less than it looks, and the encoders here are written
//! from T.4, T.6, TIFF 6.0 §9 and §13 and T.81 rather than from `tiff.rs`.
//!
//! Three of them are worth naming, because they are the parts a fixture author
//! would otherwise be tempted to skip:
//!
//! - [`encode_ccitt`] is a real T.4/T.6 coder — one-dimensional runs from
//!   Table 1's terminating codes, and the changing-element algorithm of T.6
//!   §2.2 with pass, vertical and horizontal modes. It is what makes
//!   "compression 2, 3, 3-with-2D and 4 all decode" a claim about four
//!   different code paths rather than four names for one.
//! - [`encode_lzw`] emits both bit orders and both width rules from one code
//!   stream, which is the only way to test [`super::transcode_old_style_lzw`]
//!   against something that is not itself.
//! - [`flat_jpeg`] is a baseline T.81 datastream in the abbreviated form
//!   TIFF Technical Note 2 describes, so the `JPEGTables` splice has two halves
//!   to put together.

use super::*;
use crate::{tiff_decode, tiff_scan, Limits};

const CAP: Limits = Limits::new(1 << 24);

// ---- writing a TIFF ----------------------------------------------------

/// One field's values, before a byte order has been chosen for them.
#[derive(Clone)]
enum Values {
    /// TIFF type 1 (BYTE) or, with `kind` overridden, 7 (UNDEFINED).
    Bytes(Vec<u8>),
    /// Type 3, SHORT.
    Shorts(Vec<u16>),
    /// Type 4, LONG.
    Longs(Vec<u32>),
    /// Type 5, RATIONAL: two LONGs, numerator first.
    Rationals(Vec<(u32, u32)>),
}

impl Values {
    fn count(&self) -> u32 {
        match self {
            Values::Bytes(v) => v.len() as u32,
            Values::Shorts(v) => v.len() as u32,
            Values::Longs(v) => v.len() as u32,
            Values::Rationals(v) => v.len() as u32,
        }
    }

    fn encode(&self, little: bool) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            Values::Bytes(v) => out.extend_from_slice(v),
            Values::Shorts(v) => {
                for &x in v {
                    out.extend_from_slice(&if little {
                        x.to_le_bytes()
                    } else {
                        x.to_be_bytes()
                    });
                }
            }
            Values::Longs(v) => {
                for &x in v {
                    out.extend_from_slice(&if little {
                        x.to_le_bytes()
                    } else {
                        x.to_be_bytes()
                    });
                }
            }
            Values::Rationals(v) => {
                for &(n, d) in v {
                    for x in [n, d] {
                        out.extend_from_slice(&if little {
                            x.to_le_bytes()
                        } else {
                            x.to_be_bytes()
                        });
                    }
                }
            }
        }
        out
    }
}

#[derive(Clone)]
struct Tag {
    tag: u16,
    kind: u16,
    values: Values,
}

fn short(tag: u16, v: u16) -> Tag {
    Tag {
        tag,
        kind: 3,
        values: Values::Shorts(vec![v]),
    }
}

fn shorts(tag: u16, v: &[u16]) -> Tag {
    Tag {
        tag,
        kind: 3,
        values: Values::Shorts(v.to_vec()),
    }
}

fn long(tag: u16, v: u32) -> Tag {
    Tag {
        tag,
        kind: 4,
        values: Values::Longs(vec![v]),
    }
}

fn undefined(tag: u16, v: &[u8]) -> Tag {
    Tag {
        tag,
        kind: 7,
        values: Values::Bytes(v.to_vec()),
    }
}

fn rational(tag: u16, n: u32, d: u32) -> Tag {
    Tag {
        tag,
        kind: 5,
        values: Values::Rationals(vec![(n, d)]),
    }
}

/// A TIFF under construction: p.13's header, p.14's directory and a data area.
struct TiffFile {
    little: bool,
    tags: Vec<Tag>,
    segments: Vec<Vec<u8>>,
    /// Whether the segments are tiles (324/325) rather than strips (273/279).
    tiled: bool,
}

impl TiffFile {
    fn new(little: bool) -> TiffFile {
        TiffFile {
            little,
            tags: Vec::new(),
            segments: Vec::new(),
            tiled: false,
        }
    }

    fn tag(mut self, t: Tag) -> TiffFile {
        self.tags.push(t);
        self
    }

    fn segments(mut self, segments: Vec<Vec<u8>>, tiled: bool) -> TiffFile {
        self.segments = segments;
        self.tiled = tiled;
        self
    }

    /// Lays the file out: header, one directory, then every external payload
    /// and every segment.
    fn build(mut self) -> Vec<u8> {
        let (offsets_tag, counts_tag) = if self.tiled {
            (324u16, 325u16)
        } else {
            (273, 279)
        };
        let counts: Vec<u32> = self.segments.iter().map(|s| s.len() as u32).collect();
        self.tags.push(Tag {
            tag: counts_tag,
            kind: 4,
            values: Values::Longs(counts),
        });
        // The offsets are not known until the layout is; the payload's *length*
        // is, which is all the layout needs.
        self.tags.push(Tag {
            tag: offsets_tag,
            kind: 4,
            values: Values::Longs(vec![0; self.segments.len()]),
        });
        self.tags.sort_by_key(|t| t.tag);

        let n = self.tags.len();
        let data_start = 8 + 2 + 12 * n + 4;

        // p.15: a value of more than four bytes lives outside the entry, "on a
        // word boundary".
        let mut external: Vec<(usize, Vec<u8>)> = Vec::new();
        let mut at = data_start;
        for tag in &self.tags {
            let payload = tag.values.encode(self.little);
            if payload.len() > 4 {
                external.push((at, payload.clone()));
                at += payload.len() + payload.len() % 2;
            } else {
                external.push((0, payload));
            }
        }

        let mut segment_at = Vec::new();
        for segment in &self.segments {
            segment_at.push(at as u32);
            at += segment.len() + segment.len() % 2;
        }

        // Now the offsets are known, so the placeholder payload is rewritten in
        // place — its length is unchanged, so nothing above moves.
        let offsets_index = self
            .tags
            .iter()
            .position(|t| t.tag == offsets_tag)
            .expect("just pushed");
        self.tags[offsets_index].values = Values::Longs(segment_at.clone());
        let payload = self.tags[offsets_index].values.encode(self.little);
        external[offsets_index].1 = payload;

        let mut out = Vec::new();
        out.extend_from_slice(if self.little { b"II" } else { b"MM" });
        out.extend_from_slice(&self.u16(42));
        out.extend_from_slice(&self.u32(8));
        out.extend_from_slice(&self.u16(n as u16));
        for (index, tag) in self.tags.iter().enumerate() {
            out.extend_from_slice(&self.u16(tag.tag));
            out.extend_from_slice(&self.u16(tag.kind));
            out.extend_from_slice(&self.u32(tag.values.count()));
            let (offset, payload) = &external[index];
            if payload.len() > 4 {
                out.extend_from_slice(&self.u32(*offset as u32));
            } else {
                // p.15: "the value is left-justified within the 4-byte field".
                let mut four = payload.clone();
                four.resize(4, 0);
                out.extend_from_slice(&four);
            }
        }
        out.extend_from_slice(&self.u32(0)); // no next IFD

        for (offset, payload) in &external {
            if payload.len() > 4 {
                assert_eq!(out.len(), *offset, "external payload misplaced");
                out.extend_from_slice(payload);
                if payload.len() % 2 == 1 {
                    out.push(0);
                }
            }
        }
        for (index, segment) in self.segments.iter().enumerate() {
            assert_eq!(out.len() as u32, segment_at[index], "segment misplaced");
            out.extend_from_slice(segment);
            if segment.len() % 2 == 1 {
                out.push(0);
            }
        }
        out
    }

    fn u16(&self, v: u16) -> [u8; 2] {
        if self.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }

    fn u32(&self, v: u32) -> [u8; 4] {
        if self.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }
}

/// The eight numbers a one-strip fixture needs, as one value rather than as
/// eight positional arguments — which is also the shape a reader can check a
/// call against without counting commas.
#[derive(Clone, Copy)]
struct Simple {
    little: bool,
    width: u32,
    height: u32,
    depth: u16,
    samples: u16,
    photometric: u16,
    compression: u16,
}

/// The common case: one strip, RGB or grey, uncompressed unless told otherwise.
fn image(shape: Simple, strip: Vec<u8>) -> Vec<u8> {
    TiffFile::new(shape.little)
        .tag(long(TAG_IMAGE_WIDTH, shape.width))
        .tag(long(TAG_IMAGE_LENGTH, shape.height))
        .tag(shorts(
            TAG_BITS_PER_SAMPLE,
            &vec![shape.depth; shape.samples as usize],
        ))
        .tag(short(TAG_COMPRESSION, shape.compression))
        .tag(short(TAG_PHOTOMETRIC, shape.photometric))
        .tag(short(TAG_SAMPLES_PER_PIXEL, shape.samples))
        .tag(long(TAG_ROWS_PER_STRIP, shape.height))
        .segments(vec![strip], false)
        .build()
}

/// A distinct colour per pixel, so a transposed or reversed raster is visible.
///
/// The same argument `cbz_support::distinct_pixels` makes one crate up: a
/// relabelling that is a bijection is invisible to a symmetric fixture, and
/// TIFF's strip and tile geometry is exactly where one happens.
fn distinct_rgb(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..height {
        for x in 0..width {
            out.push((x * 7 + 1) as u8);
            out.push((y * 11 + 2) as u8);
            out.push(((x * 37 + y * 13) & 0xFF) as u8 ^ 0x5A);
        }
    }
    out
}

// ---- PackBits (TIFF 6.0 §9) --------------------------------------------

fn encode_packbits(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < input.len() {
        let b = input[i];
        let mut run = 1usize;
        while i + run < input.len() && input[i + run] == b && run < 128 {
            run += 1;
        }
        if run >= 3 {
            out.push((257 - run) as u8);
            out.push(b);
            i += run;
        } else {
            let start = i;
            let mut lit = 0usize;
            while i < input.len() && lit < 128 {
                if i + 2 < input.len() && input[i] == input[i + 1] && input[i] == input[i + 2] {
                    break;
                }
                i += 1;
                lit += 1;
            }
            out.push((lit - 1) as u8);
            out.extend_from_slice(&input[start..start + lit]);
        }
    }
    out
}

// ---- LZW (TIFF 6.0 §13, and the bit order §13 does not describe) --------

/// Emits a legal LZW code stream for `input` — Clear, every byte as its own
/// literal code, then EndOfInformation — in whichever bit order and under
/// whichever width rule is asked for.
///
/// It compresses nothing, and that is the point: the *dictionary* is not what
/// is under test here, the packing is. Every literal still enlarges the
/// decoder's table by one, so the code width still walks 9 → 10 → 11 → 12 and
/// the two width rules still diverge, which is the whole of the difference
/// between the two forms.
fn encode_lzw(input: &[u8], msb_first: bool, early_change: bool) -> Vec<u8> {
    let early = u32::from(early_change);
    let mut out: Vec<u8> = Vec::new();
    let mut acc = 0u32;
    let mut bits = 0u32;
    let mut next = 258u32;
    let mut seen = false;

    let push = |code: u16, width: u32, out: &mut Vec<u8>, acc: &mut u32, bits: &mut u32| {
        if msb_first {
            *acc = (*acc << width) | u32::from(code);
            *bits += width;
            while *bits >= 8 {
                *bits -= 8;
                out.push((*acc >> *bits) as u8);
            }
        } else {
            *acc |= u32::from(code) << *bits;
            *bits += width;
            while *bits >= 8 {
                out.push((*acc & 0xFF) as u8);
                *acc >>= 8;
                *bits -= 8;
            }
        }
    };

    push(256, width_for(next, early), &mut out, &mut acc, &mut bits);
    for &byte in input {
        push(
            u16::from(byte),
            width_for(next, early),
            &mut out,
            &mut acc,
            &mut bits,
        );
        if seen {
            next += 1;
        } else {
            seen = true;
        }
        assert!(next < 4000, "the fixture would fill the table");
    }
    push(257, width_for(next, early), &mut out, &mut acc, &mut bits);
    if bits > 0 {
        if msb_first {
            out.push((acc << (8 - bits)) as u8);
        } else {
            out.push((acc & 0xFF) as u8);
        }
    }
    out
}

// ---- CCITT (T.4 and T.6) -----------------------------------------------

/// T.4 Table 1, terminating codes for white runs of 0 to 16.
///
/// Only sixteen, because every fixture here is sixteen pixels wide and no run
/// in one can be longer. A makeup code would be a seventeenth path this file
/// does not exercise, and writing the table out to 63 to leave most of it
/// unreached is the failure `png.rs`'s module note is about.
const WHITE: [&str; 17] = [
    "00110101", "000111", "0111", "1000", "1011", "1100", "1110", "1111", "10011", "10100",
    "00111", "01000", "001000", "000011", "110100", "110101", "101010",
];

/// T.4 Table 1, terminating codes for black runs of 0 to 16.
const BLACK: [&str; 17] = [
    "0000110111",
    "010",
    "11",
    "10",
    "011",
    "0011",
    "0010",
    "00011",
    "000101",
    "000100",
    "0000100",
    "0000101",
    "0000111",
    "00000100",
    "00000111",
    "000011000",
    "0000010111",
];

/// T.4 §4.2.1.3.2 / T.6 Table 4: the two-dimensional mode codes.
const PASS: &str = "0001";
const HORIZONTAL: &str = "001";
const VERTICAL: [(&str, isize); 7] = [
    ("0000010", -3),
    ("000010", -2),
    ("010", -1),
    ("1", 0),
    ("011", 1),
    ("000011", 2),
    ("0000011", 3),
];

/// T.4 §4.1.2: eleven zeros and a one.
const EOL: &str = "000000000001";

#[derive(Default)]
struct BitString(String);

impl BitString {
    fn push(&mut self, code: &str) {
        self.0.push_str(code);
    }

    fn align(&mut self) {
        while self.0.len() % 8 != 0 {
            self.0.push('0');
        }
    }

    fn finish(self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut byte = 0u8;
        let mut count = 0u32;
        for c in self.0.chars() {
            byte = (byte << 1) | u8::from(c == '1');
            count += 1;
            if count % 8 == 0 {
                out.push(byte);
                byte = 0;
            }
        }
        if count % 8 != 0 {
            out.push(byte << (8 - count % 8));
        }
        out
    }
}

fn run_code(black: bool, run: usize) -> &'static str {
    assert!(run <= 16, "the fixture tables stop at sixteen");
    if black {
        BLACK[run]
    } else {
        WHITE[run]
    }
}

/// Positions where a line's colour changes, with an imaginary white pixel
/// before it (T.4 §4.2.1.1's "changing element").
fn changing(line: &[bool]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut previous = false;
    for (index, &pixel) in line.iter().enumerate() {
        if pixel != previous {
            out.push(index);
            previous = pixel;
        }
    }
    out
}

/// One line coded one-dimensionally: alternating runs from white.
fn encode_1d(line: &[bool], bits: &mut BitString) {
    let width = line.len();
    let mut at = 0usize;
    let mut black = false;
    while at < width {
        let mut run = 0usize;
        while at + run < width && line[at + run] == black {
            run += 1;
        }
        bits.push(run_code(black, run));
        at += run;
        black = !black;
    }
    // A line ending on a colour change owes a final zero-length run of the
    // other colour only if the runs did not already reach the width, which the
    // loop above guarantees they did.
}

/// One line coded two-dimensionally against `reference` — T.6 §2.2's
/// changing-element algorithm, pass, vertical and horizontal modes.
fn encode_2d(line: &[bool], reference: &[bool], bits: &mut BitString) {
    let width = line.len();
    let cur = changing(line);
    let refs = changing(reference);
    let mut a0: isize = -1;
    let mut black = false;

    while a0 < width as isize {
        let a1 = cur
            .iter()
            .copied()
            .find(|&p| p as isize > a0)
            .unwrap_or(width);
        let a2 = cur.iter().copied().find(|&p| p > a1).unwrap_or(width);
        // b1: the first changing element on the reference line right of a0
        // whose own colour is the opposite of a0's.
        let b1 = refs
            .iter()
            .copied()
            .find(|&p| p as isize > a0 && reference[p] != black)
            .unwrap_or(width);
        let b2 = refs.iter().copied().find(|&p| p > b1).unwrap_or(width);

        if b2 < a1 {
            bits.push(PASS);
            a0 = b2 as isize;
        } else if (a1 as isize - b1 as isize).abs() <= 3 {
            let delta = a1 as isize - b1 as isize;
            let code = VERTICAL
                .iter()
                .find(|(_, d)| *d == delta)
                .expect("|delta| <= 3");
            bits.push(code.0);
            a0 = a1 as isize;
            black = !black;
        } else {
            bits.push(HORIZONTAL);
            let start = a0.max(0) as usize;
            bits.push(run_code(black, a1 - start));
            bits.push(run_code(!black, a2 - a1));
            a0 = a2 as isize;
        }
        if a0 >= width as isize {
            break;
        }
    }
}

/// Which coding a fixture asks for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fax {
    /// Compression 2: one-dimensional, no EOLs, every row byte-aligned (§10).
    ModifiedHuffman,
    /// Compression 3, `T4Options` 0: one-dimensional with an EOL a row.
    G3OneDimensional,
    /// Compression 3, `T4Options` 1: an EOL and a tag bit a row, the first row
    /// one-dimensional and the rest two-dimensional (T.4 §4.2.1.3.1).
    G3Mixed,
    /// Compression 4: two-dimensional throughout, no EOLs.
    G4,
}

fn encode_ccitt(rows: &[Vec<bool>], kind: Fax) -> Vec<u8> {
    let width = rows.first().map_or(0, Vec::len);
    let mut bits = BitString::default();
    let mut reference = vec![false; width];
    for (index, row) in rows.iter().enumerate() {
        match kind {
            Fax::ModifiedHuffman => {
                encode_1d(row, &mut bits);
                bits.align();
            }
            Fax::G3OneDimensional => {
                bits.push(EOL);
                encode_1d(row, &mut bits);
            }
            Fax::G3Mixed => {
                bits.push(EOL);
                if index == 0 {
                    bits.push("1");
                    encode_1d(row, &mut bits);
                } else {
                    bits.push("0");
                    encode_2d(row, &reference, &mut bits);
                }
            }
            Fax::G4 => encode_2d(row, &reference, &mut bits),
        }
        reference = row.clone();
    }
    bits.finish()
}

/// A bilevel pattern with runs of several lengths and rows that differ from
/// their neighbours, so vertical mode, horizontal mode and pass mode are all
/// reached.
fn fax_rows() -> Vec<Vec<bool>> {
    let patterns = [
        "0000000011111111",
        "0011110000111100",
        "0011000000001100",
        "1111111100000000",
        "0101010101010101",
        "0000000000000000",
    ];
    patterns
        .iter()
        .map(|p| p.chars().map(|c| c == '1').collect())
        .collect()
}

// ---- JPEG (T.81), in TIFF Technical Note 2's split form -----------------

struct JpegBits {
    out: Vec<u8>,
    byte: u8,
    used: u32,
}

impl JpegBits {
    fn new() -> JpegBits {
        JpegBits {
            out: Vec::new(),
            byte: 0,
            used: 0,
        }
    }

    fn push(&mut self, bits: &str) {
        for c in bits.chars() {
            self.byte = (self.byte << 1) | u8::from(c == '1');
            self.used += 1;
            if self.used == 8 {
                self.out.push(self.byte);
                // B.1.1.5: a 0xFF in entropy-coded data is followed by a zero.
                if self.byte == 0xFF {
                    self.out.push(0x00);
                }
                self.byte = 0;
                self.used = 0;
            }
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.used > 0 {
            // F.1.2.3: the last byte is padded with ones.
            let pad = 8 - self.used;
            self.byte = (self.byte << pad) | ((1u8 << pad) - 1);
            self.out.push(self.byte);
            if self.byte == 0xFF {
                self.out.push(0x00);
            }
        }
        self.out
    }
}

fn marker(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0xFF, kind];
    out.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

/// The abbreviated table-specification datastream `JPEGTables` holds: SOI, a
/// quantisation table, two Huffman tables, EOI.
///
/// The Huffman tables are deliberately **not** Annex K's: a DC table of twelve
/// four-bit codes and an AC table with nothing in it but EOB is legal under
/// B.2.4.2, is a tenth the size, and is what a fixture needs. A decoder that
/// only works with Annex K's tables is broken in a way Annex K would hide.
fn jpeg_tables() -> Vec<u8> {
    let mut out = vec![0xFF, 0xD8];
    // DQT: eight-bit precision, table 0, every step size one.
    let mut dqt = vec![0x00];
    dqt.extend_from_slice(&[1u8; 64]);
    out.extend_from_slice(&marker(0xDB, &dqt));

    // DHT, class 0 (DC) table 0: twelve codes, all of length four.
    let mut dc = vec![0x00];
    let mut counts = [0u8; 16];
    counts[3] = 12;
    dc.extend_from_slice(&counts);
    dc.extend_from_slice(&(0u8..12).collect::<Vec<u8>>());
    out.extend_from_slice(&marker(0xC4, &dc));

    // DHT, class 1 (AC) table 0: one code of length two, the EOB symbol.
    let mut ac = vec![0x10];
    let mut counts = [0u8; 16];
    counts[1] = 1;
    ac.extend_from_slice(&counts);
    ac.push(0x00);
    out.extend_from_slice(&marker(0xC4, &ac));

    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

/// The abbreviated *image* datastream a strip holds: SOI, SOF0, SOS, the
/// entropy-coded blocks, EOI — and no tables at all.
///
/// Every 8x8 block is flat, so its only non-zero coefficient is the DC one and
/// F(0,0) is exactly eight times the level-shifted sample (T.81 A.3.3). With a
/// step size of one there is nothing to round, which is what makes the
/// round-trip below an equality rather than a tolerance.
fn flat_jpeg(
    blocks_across: usize,
    blocks_down: usize,
    value: impl Fn(usize, usize) -> u8,
) -> Vec<u8> {
    let width = (blocks_across * 8) as u16;
    let height = (blocks_down * 8) as u16;
    let mut out = vec![0xFF, 0xD8];

    let mut sof = vec![8u8];
    sof.extend_from_slice(&height.to_be_bytes());
    sof.extend_from_slice(&width.to_be_bytes());
    sof.push(1); // one component
    sof.extend_from_slice(&[1, 0x11, 0]);
    out.extend_from_slice(&marker(0xC0, &sof));

    out.extend_from_slice(&marker(0xDA, &[1, 1, 0x00, 0, 63, 0]));

    let mut bits = JpegBits::new();
    let mut previous = 0i32;
    for by in 0..blocks_down {
        for bx in 0..blocks_across {
            let dc = 8 * (i32::from(value(bx, by)) - 128);
            let diff = dc - previous;
            previous = dc;
            let category = if diff == 0 {
                0u32
            } else {
                32 - diff.unsigned_abs().leading_zeros()
            };
            // The DC table's code for category c is c in four bits.
            bits.push(&format!("{category:04b}"));
            if category > 0 {
                // F.1.2.1: a negative difference is coded as diff - 1.
                let v = if diff > 0 {
                    diff as u32
                } else {
                    (diff - 1) as u32 & ((1u32 << category) - 1)
                };
                for bit in (0..category).rev() {
                    bits.push(if v >> bit & 1 == 1 { "1" } else { "0" });
                }
            }
            bits.push("00"); // EOB
        }
    }
    out.extend_from_slice(&bits.finish());
    out.extend_from_slice(&[0xFF, 0xD9]);
    out
}

// ---- the header and the directory --------------------------------------

#[test]
fn both_byte_orders_read_the_same_image() {
    let pixels = distinct_rgb(4, 3);
    for little in [true, false] {
        let file = image(
            Simple {
                little,
                width: 4,
                height: 3,
                depth: 8,
                samples: 3,
                photometric: 2,
                compression: 1,
            },
            pixels.clone(),
        );
        let img = tiff_decode(&file, &CAP).expect("decodes");
        assert_eq!((img.width, img.height), (4, 3));
        assert_eq!(img.colour, TiffColour::Rgb);
        assert_eq!(img.bits_per_component, 8);
        assert_eq!(img.data, pixels, "little_endian = {little}");
        assert!(img.complete);
    }
}

#[test]
fn a_file_that_is_not_a_tiff_is_refused_by_name() {
    assert_eq!(tiff_scan(b"").unwrap_err(), TiffError::NotTiff);
    assert_eq!(
        tiff_scan(b"\x89PNG\r\n\x1a\n").unwrap_err(),
        TiffError::NotTiff
    );
    // The right order bytes and the wrong magic.
    assert_eq!(
        tiff_scan(b"II\x00\x00\x08\x00\x00\x00").unwrap_err(),
        TiffError::NotTiff
    );
}

#[test]
fn bigtiff_is_refused_as_the_different_format_it_is() {
    let mut file = image(
        Simple {
            little: true,
            width: 1,
            height: 1,
            depth: 8,
            samples: 1,
            photometric: 1,
            compression: 1,
        },
        vec![9],
    );
    file[2] = 43;
    file[3] = 0;
    assert_eq!(tiff_scan(&file).unwrap_err(), TiffError::BigTiff);
}

/// A directory whose `NextIFD` points at itself. Without the guard this walks
/// until something else stops it, and nothing else would.
#[test]
fn a_directory_chain_that_cycles_is_cut_and_reported() {
    let mut file = image(
        Simple {
            little: true,
            width: 2,
            height: 1,
            depth: 8,
            samples: 1,
            photometric: 1,
            compression: 1,
        },
        vec![1, 2],
    );
    // The next-IFD pointer sits after the entry array; point it at the
    // directory itself, which is at offset 8.
    let entries = u16::from_le_bytes([file[8], file[9]]) as usize;
    let next_at = 8 + 2 + 12 * entries;
    file[next_at..next_at + 4].copy_from_slice(&8u32.to_le_bytes());

    let scan = tiff_scan(&file).expect("the first directory is still readable");
    assert_eq!(scan.pages, 1);
    assert!(scan.warnings.contains(&Warning::TiffDirectoryCycle));
    assert_eq!(scan.decode(&CAP).expect("decodes").data, vec![1, 2]);
}

#[test]
fn a_field_whose_type_is_unknown_is_skipped_rather_than_fatal() {
    let mut file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 2))
        .tag(long(TAG_IMAGE_LENGTH, 1))
        .tag(short(TAG_BITS_PER_SAMPLE, 8))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 1))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
        .tag(long(TAG_ROWS_PER_STRIP, 1))
        // p.16: an unknown *type* is skipped, unlike an unknown tag.
        .tag(Tag {
            tag: 700,
            kind: 999,
            values: Values::Longs(vec![7]),
        })
        .segments(vec![vec![3, 4]], false)
        .build();
    assert_eq!(tiff_decode(&file, &CAP).expect("decodes").data, vec![3, 4]);
    // And the same file with the field's *tag* unknown but its type legal.
    file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 2))
        .tag(long(TAG_IMAGE_LENGTH, 1))
        .tag(short(TAG_BITS_PER_SAMPLE, 8))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 1))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
        .tag(long(TAG_ROWS_PER_STRIP, 1))
        .tag(long(60_000, 7))
        .segments(vec![vec![3, 4]], false)
        .build();
    assert_eq!(tiff_decode(&file, &CAP).expect("decodes").data, vec![3, 4]);
}

#[test]
fn the_resolution_tags_are_read_as_the_rationals_they_are() {
    let file = TiffFile::new(false)
        .tag(long(TAG_IMAGE_WIDTH, 1))
        .tag(long(TAG_IMAGE_LENGTH, 1))
        .tag(short(TAG_BITS_PER_SAMPLE, 8))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 1))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
        .tag(long(TAG_ROWS_PER_STRIP, 1))
        .tag(rational(TAG_X_RESOLUTION, 300, 1))
        .tag(rational(TAG_Y_RESOLUTION, 600, 2))
        .tag(short(TAG_RESOLUTION_UNIT, 3))
        .segments(vec![vec![0x40]], false)
        .build();
    let scan = tiff_scan(&file).expect("scans");
    let resolution = scan.resolution.expect("both rationals are present");
    assert_eq!(resolution.x, (300, 1));
    assert_eq!(resolution.y, (600, 2));
    assert_eq!(resolution.unit, 3);
}

// ---- photometric interpretations ---------------------------------------

#[test]
fn white_is_zero_is_inverted_and_black_is_zero_is_not() {
    let samples = vec![0u8, 64, 192, 255];
    let dark = image(
        Simple {
            little: true,
            width: 4,
            height: 1,
            depth: 8,
            samples: 1,
            photometric: 0,
            compression: 1,
        },
        samples.clone(),
    );
    let light = image(
        Simple {
            little: true,
            width: 4,
            height: 1,
            depth: 8,
            samples: 1,
            photometric: 1,
            compression: 1,
        },
        samples.clone(),
    );
    assert_eq!(
        tiff_decode(&dark, &CAP).expect("decodes").data,
        vec![255, 191, 63, 0],
        "PhotometricInterpretation 0: sample 0 is white"
    );
    assert_eq!(
        tiff_decode(&light, &CAP).expect("decodes").data,
        samples,
        "PhotometricInterpretation 1: sample 0 is black"
    );
}

#[test]
fn a_palette_is_applied_from_three_arrays_rather_than_from_triples() {
    // p.23: all the reds, then all the greens, then all the blues — which is
    // not PLTE's layout and not `/Indexed`'s, and the one thing about a TIFF
    // palette a reader written from either of those gets wrong.
    let map: Vec<u16> = vec![
        0, 65535, 0, 65535, // red
        0, 0, 65535, 65535, // green
        0, 0, 0, 65535, // blue
    ];
    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 4))
        .tag(long(TAG_IMAGE_LENGTH, 1))
        .tag(short(TAG_BITS_PER_SAMPLE, 2))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 3))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
        .tag(long(TAG_ROWS_PER_STRIP, 1))
        .tag(shorts(TAG_COLOR_MAP, &map))
        // Indices 0, 1, 2, 3 at two bits each, high-order bit first.
        .segments(vec![vec![0b00_01_10_11]], false)
        .build();
    let img = tiff_decode(&file, &CAP).expect("decodes");
    assert_eq!(img.colour, TiffColour::Rgb);
    assert_eq!(img.data, vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 255, 255, 255]);
}

#[test]
fn a_color_map_written_at_eight_bits_is_read_as_one() {
    // The defect the module note names: every value at or below 255, which no
    // 16-bit map of a real image ever is.
    let map: Vec<u16> = vec![0, 255, 0, 128, 0, 64];
    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 2))
        .tag(long(TAG_IMAGE_LENGTH, 1))
        .tag(short(TAG_BITS_PER_SAMPLE, 1))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 3))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
        .tag(long(TAG_ROWS_PER_STRIP, 1))
        .tag(shorts(TAG_COLOR_MAP, &map))
        .segments(vec![vec![0b01_000000]], false)
        .build();
    let img = tiff_decode(&file, &CAP).expect("decodes");
    assert!(img.warnings.contains(&Warning::TiffColorMapIsEightBit));
    assert_eq!(img.data, vec![0, 0, 0, 255, 128, 64]);
}

#[test]
fn a_palette_image_with_no_color_map_is_refused() {
    let file = image(
        Simple {
            little: true,
            width: 2,
            height: 1,
            depth: 8,
            samples: 1,
            photometric: 3,
            compression: 1,
        },
        vec![0, 1],
    );
    assert_eq!(
        tiff_decode(&file, &CAP).unwrap_err(),
        TiffError::MissingColorMap
    );
}

#[test]
fn the_photometrics_this_build_does_not_read_are_named() {
    for code in [4u16, 5, 8, 32803] {
        let file = image(
            Simple {
                little: true,
                width: 1,
                height: 1,
                depth: 8,
                samples: 1,
                photometric: code,
                compression: 1,
            },
            vec![0],
        );
        assert_eq!(
            tiff_decode(&file, &CAP).unwrap_err(),
            TiffError::UnsupportedPhotometric(code),
            "photometric {code}"
        );
    }
    // 6 is YCbCr, which is read only where a JPEG has already undone it.
    let file = image(
        Simple {
            little: true,
            width: 1,
            height: 1,
            depth: 8,
            samples: 3,
            photometric: 6,
            compression: 1,
        },
        vec![0, 0, 0],
    );
    assert_eq!(
        tiff_decode(&file, &CAP).unwrap_err(),
        TiffError::UnsupportedPhotometric(6)
    );
}

// ---- bit depths --------------------------------------------------------

#[test]
fn the_small_depths_are_expanded_by_the_multiplication_the_division_reduces_to() {
    // 255/1, 255/3 and 255/15 are exactly 255, 85 and 17.
    let one = image(
        Simple {
            little: true,
            width: 8,
            height: 1,
            depth: 1,
            samples: 1,
            photometric: 1,
            compression: 1,
        },
        vec![0b1010_1010],
    );
    assert_eq!(
        tiff_decode(&one, &CAP).expect("decodes").data,
        vec![255, 0, 255, 0, 255, 0, 255, 0]
    );
    let two = image(
        Simple {
            little: true,
            width: 4,
            height: 1,
            depth: 2,
            samples: 1,
            photometric: 1,
            compression: 1,
        },
        vec![0b00_01_10_11],
    );
    assert_eq!(
        tiff_decode(&two, &CAP).expect("decodes").data,
        vec![0, 85, 170, 255]
    );
    let four = image(
        Simple {
            little: true,
            width: 2,
            height: 1,
            depth: 4,
            samples: 1,
            photometric: 1,
            compression: 1,
        },
        vec![0x0F],
    );
    assert_eq!(
        tiff_decode(&four, &CAP).expect("decodes").data,
        vec![0, 255]
    );
}

#[test]
fn sixteen_bit_samples_leave_big_endian_whatever_order_they_arrived_in() {
    let values: [u16; 3] = [0x0102, 0xFFFE, 0x8000];
    for little in [true, false] {
        let mut strip = Vec::new();
        for v in values {
            strip.extend_from_slice(&if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            });
        }
        let file = image(
            Simple {
                little,
                width: 3,
                height: 1,
                depth: 16,
                samples: 1,
                photometric: 1,
                compression: 1,
            },
            strip,
        );
        let img = tiff_decode(&file, &CAP).expect("decodes");
        assert_eq!(img.bits_per_component, 16);
        assert_eq!(
            img.data,
            values
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<u8>>(),
            "little_endian = {little}"
        );
    }
}

#[test]
fn two_depths_in_one_image_are_refused_rather_than_half_read() {
    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 1))
        .tag(long(TAG_IMAGE_LENGTH, 1))
        .tag(shorts(TAG_BITS_PER_SAMPLE, &[8, 4, 8]))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 2))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 3))
        .tag(long(TAG_ROWS_PER_STRIP, 1))
        .segments(vec![vec![0, 0, 0]], false)
        .build();
    assert_eq!(
        tiff_decode(&file, &CAP).unwrap_err(),
        TiffError::UnequalBitDepths
    );
}

#[test]
fn float_and_signed_samples_are_refused_rather_than_read_as_unsigned() {
    for format in [2u16, 3] {
        let file = TiffFile::new(true)
            .tag(long(TAG_IMAGE_WIDTH, 1))
            .tag(long(TAG_IMAGE_LENGTH, 1))
            .tag(short(TAG_BITS_PER_SAMPLE, 16))
            .tag(short(TAG_COMPRESSION, 1))
            .tag(short(TAG_PHOTOMETRIC, 1))
            .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
            .tag(long(TAG_ROWS_PER_STRIP, 1))
            .tag(short(TAG_SAMPLE_FORMAT, format))
            .segments(vec![vec![0, 0]], false)
            .build();
        assert_eq!(
            tiff_decode(&file, &CAP).unwrap_err(),
            TiffError::UnsupportedSampleFormat(format)
        );
    }
}

// ---- compressions ------------------------------------------------------

/// Every compression this build decodes, over one image, against one expected
/// raster. Six codings, one picture — which is the only way "compression 5 and
/// compression 8 produce the same image" is a claim rather than two.
#[test]
fn every_compression_produces_the_same_picture() {
    let pixels = distinct_rgb(8, 4);
    let strips: [(u16, Vec<u8>); 6] = [
        (1, pixels.clone()),
        (5, encode_lzw(&pixels, true, true)),
        (8, crate::zlib_compress(&pixels)),
        (32946, crate::zlib_compress(&pixels)),
        (32773, encode_packbits(&pixels)),
        // The pre-1993 form, which is the same code stream in the other bit
        // order under the other width rule.
        (5, encode_lzw(&pixels, false, false)),
    ];
    for (compression, strip) in strips {
        let file = image(
            Simple {
                little: true,
                width: 8,
                height: 4,
                depth: 8,
                samples: 3,
                photometric: 2,
                compression,
            },
            strip,
        );
        let img = tiff_decode(&file, &CAP).expect("decodes");
        assert_eq!(img.data, pixels, "compression {compression}");
        assert!(img.complete, "compression {compression}");
    }
}

#[test]
fn an_old_style_lzw_strip_is_detected_and_reported() {
    let pixels = distinct_rgb(8, 4);
    let old = image(
        Simple {
            little: true,
            width: 8,
            height: 4,
            depth: 8,
            samples: 3,
            photometric: 2,
            compression: 5,
        },
        encode_lzw(&pixels, false, false),
    );
    let new = image(
        Simple {
            little: true,
            width: 8,
            height: 4,
            depth: 8,
            samples: 3,
            photometric: 2,
            compression: 5,
        },
        encode_lzw(&pixels, true, true),
    );
    let old = tiff_decode(&old, &CAP).expect("decodes");
    let new = tiff_decode(&new, &CAP).expect("decodes");
    assert_eq!(old.data, new.data);
    assert!(old.warnings.contains(&Warning::TiffOldStyleLzw));
    assert!(!new.warnings.contains(&Warning::TiffOldStyleLzw));
}

#[test]
fn the_compressions_this_build_does_not_decode_are_named() {
    // 6 is the old-style JPEG TIFF Technical Note 2 withdrew, 34712 is JPEG
    // 2000, 32771 is a CCITT variant nothing writes.
    for code in [6u16, 34712, 32771, 9, 10] {
        let file = image(
            Simple {
                little: true,
                width: 1,
                height: 1,
                depth: 8,
                samples: 1,
                photometric: 1,
                compression: code,
            },
            vec![0],
        );
        assert_eq!(
            tiff_decode(&file, &CAP).unwrap_err(),
            TiffError::UnsupportedCompression(code),
            "compression {code}"
        );
    }
}

/// The four CCITT codings, over one bilevel picture.
#[test]
fn every_ccitt_coding_produces_the_same_bilevel_picture() {
    let rows = fax_rows();
    let width = rows[0].len() as u32;
    let height = rows.len() as u32;
    // PhotometricInterpretation 0 pairs with the fax codings (§10): the
    // decompressed 1 is black, and photometric 0 says sample 0 is white — so
    // an inverted decoder produces a photographic negative and nothing else.
    let expected: Vec<u8> = rows
        .iter()
        .flat_map(|row| row.iter().map(|&black| if black { 0u8 } else { 255 }))
        .collect();

    let cases: [(u16, Fax, u32); 4] = [
        (2, Fax::ModifiedHuffman, 0),
        (3, Fax::G3OneDimensional, 0),
        (3, Fax::G3Mixed, 1),
        (4, Fax::G4, 0),
    ];
    for (compression, kind, t4options) in cases {
        let mut file = TiffFile::new(true)
            .tag(long(TAG_IMAGE_WIDTH, width))
            .tag(long(TAG_IMAGE_LENGTH, height))
            .tag(short(TAG_BITS_PER_SAMPLE, 1))
            .tag(short(TAG_COMPRESSION, compression))
            .tag(short(TAG_PHOTOMETRIC, 0))
            .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
            .tag(long(TAG_ROWS_PER_STRIP, height));
        if compression == 3 {
            file = file.tag(long(TAG_T4_OPTIONS, t4options));
        }
        let bytes = file
            .segments(vec![encode_ccitt(&rows, kind)], false)
            .build();
        let img = tiff_decode(&bytes, &CAP).expect("decodes");
        assert_eq!(
            img.data, expected,
            "compression {compression}, T4Options {t4options}"
        );
    }
}

/// `Compression` 7 in the form TIFF Technical Note 2 defines: the tables in
/// tag 347 and an abbreviated image datastream in the strip.
///
/// Asserted two ways, and the split between them is deliberate. What this
/// module adds to `jpeg.rs` is the **splice** and the **placement**, so the
/// exact claim is against `jpeg_decode`'s own output over the same spliced
/// stream: byte for byte, no tolerance. The literal values `flat_jpeg` encoded
/// are asserted separately and to within one, because they belong to the
/// integer IDCT rather than to TIFF — a DC-only block of 8x(v-128) comes back
/// as v or v-1 depending on where `jpeg.rs` rounds, and pinning that number
/// here would make a TIFF test fail for a JPEG reason. That every pixel of a
/// block is the *same* value is asserted with no tolerance at all, since a
/// mis-spliced table set is what makes a flat block stop being flat.
#[test]
fn a_jpeg_strip_is_spliced_with_the_tables_tag_and_decoded() {
    let value = |bx: usize, by: usize| (40 + bx * 30 + by * 15) as u8;
    let strip = flat_jpeg(2, 2, value);
    let tables = jpeg_tables();
    let file = TiffFile::new(false)
        .tag(long(TAG_IMAGE_WIDTH, 16))
        .tag(long(TAG_IMAGE_LENGTH, 16))
        .tag(short(TAG_BITS_PER_SAMPLE, 8))
        .tag(short(TAG_COMPRESSION, 7))
        .tag(short(TAG_PHOTOMETRIC, 1))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
        .tag(long(TAG_ROWS_PER_STRIP, 16))
        .tag(undefined(TAG_JPEG_TABLES, &tables))
        .segments(vec![strip.clone()], false)
        .build();

    let img = tiff_decode(&file, &CAP).expect("decodes");
    assert_eq!(img.colour, TiffColour::Grey);
    for by in 0..2usize {
        for bx in 0..2usize {
            let at = (by * 8) * 16 + bx * 8;
            let got = img.data[at];
            assert!(
                got.abs_diff(value(bx, by)) <= 1,
                "block ({bx}, {by}) came back {got}, not {}",
                value(bx, by)
            );
            for row in 0..8usize {
                for col in 0..8usize {
                    assert_eq!(
                        img.data[(by * 8 + row) * 16 + bx * 8 + col],
                        got,
                        "a flat block came back unflat, so the splice is wrong"
                    );
                }
            }
        }
    }

    let spliced = [&tables[..tables.len() - 2], &strip[2..]].concat();
    let direct = crate::jpeg_decode(&spliced, 1 << 20).expect("the spliced stream is a JPEG");
    assert_eq!(img.data, direct.data, "the splice is the whole difference");
}

/// A `Compression` 7 strip that is already a complete JPEG, with no tables tag
/// at all — the other half of Technical Note 2, and the shape a writer that
/// emits one strip per file uses.
#[test]
fn a_jpeg_strip_that_carries_its_own_tables_needs_no_splice() {
    let value = |_: usize, _: usize| 200u8;
    let mut whole = jpeg_tables();
    whole.truncate(whole.len() - 2);
    let strip = flat_jpeg(1, 1, value);
    whole.extend_from_slice(&strip[2..]);

    let file = image(
        Simple {
            little: true,
            width: 8,
            height: 8,
            depth: 8,
            samples: 1,
            photometric: 1,
            compression: 7,
        },
        whole,
    );
    let img = tiff_decode(&file, &CAP).expect("decodes");
    assert!(
        img.data[0].abs_diff(200) <= 1,
        "the IDCT's own rounding, no more"
    );
    assert!(img.data.iter().all(|&b| b == img.data[0]), "one flat block");
}

// ---- strips, tiles and planes ------------------------------------------

#[test]
fn a_multi_strip_image_reassembles_in_order() {
    let pixels = distinct_rgb(4, 7);
    let row = 4 * 3;
    // Three rows a strip over seven rows: two full strips and a short one,
    // which is p.39's "the last strip may have fewer rows".
    let strips: Vec<Vec<u8>> = pixels.chunks(row * 3).map(<[u8]>::to_vec).collect();
    assert_eq!(strips.len(), 3);
    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 4))
        .tag(long(TAG_IMAGE_LENGTH, 7))
        .tag(shorts(TAG_BITS_PER_SAMPLE, &[8, 8, 8]))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 2))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 3))
        .tag(long(TAG_ROWS_PER_STRIP, 3))
        .segments(strips, false)
        .build();
    assert_eq!(tiff_decode(&file, &CAP).expect("decodes").data, pixels);
}

#[test]
fn the_default_rows_per_strip_is_the_whole_image() {
    // p.39: the default is 2^32-1, "effectively infinity". A file with no
    // RowsPerStrip at all is one strip, and a reader that defaulted it to one
    // would read the first row and nothing else.
    let pixels = distinct_rgb(3, 5);
    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 3))
        .tag(long(TAG_IMAGE_LENGTH, 5))
        .tag(shorts(TAG_BITS_PER_SAMPLE, &[8, 8, 8]))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 2))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 3))
        .segments(vec![pixels.clone()], false)
        .build();
    assert_eq!(tiff_decode(&file, &CAP).expect("decodes").data, pixels);
}

/// Tiles, including edge tiles that are stored full size and padded (p.67).
///
/// The image is 20 x 20 with 16 x 16 tiles, so of the four tiles exactly one
/// is whole and three are mostly padding — which is where a decoder that reads
/// an edge tile at the image's stride instead of the tile's produces a
/// diagonal smear rather than a picture.
#[test]
fn a_tiled_image_reassembles_and_the_edge_padding_is_dropped() {
    let (w, h) = (20u32, 20u32);
    let pixels = distinct_rgb(w, h);
    let (tw, th) = (16usize, 16usize);
    let across = (w as usize).div_ceil(tw);
    let down = (h as usize).div_ceil(th);

    let mut tiles = Vec::new();
    for ty in 0..down {
        for tx in 0..across {
            let mut tile = vec![0u8; tw * th * 3];
            for row in 0..th {
                for col in 0..tw {
                    let (x, y) = (tx * tw + col, ty * th + row);
                    if x >= w as usize || y >= h as usize {
                        continue;
                    }
                    let src = (y * w as usize + x) * 3;
                    let dst = (row * tw + col) * 3;
                    tile[dst..dst + 3].copy_from_slice(&pixels[src..src + 3]);
                }
            }
            tiles.push(tile);
        }
    }

    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, w))
        .tag(long(TAG_IMAGE_LENGTH, h))
        .tag(shorts(TAG_BITS_PER_SAMPLE, &[8, 8, 8]))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 2))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 3))
        .tag(long(TAG_TILE_WIDTH, tw as u32))
        .tag(long(TAG_TILE_LENGTH, th as u32))
        .segments(tiles, true)
        .build();
    assert_eq!(tiff_decode(&file, &CAP).expect("decodes").data, pixels);
}

#[test]
fn a_tile_geometry_the_specification_forbids_is_refused() {
    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 20))
        .tag(long(TAG_IMAGE_LENGTH, 20))
        .tag(short(TAG_BITS_PER_SAMPLE, 8))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 1))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
        // p.67: both must be a multiple of 16.
        .tag(long(TAG_TILE_WIDTH, 20))
        .tag(long(TAG_TILE_LENGTH, 16))
        .segments(vec![vec![0; 320]], true)
        .build();
    assert_eq!(
        tiff_decode(&file, &CAP).unwrap_err(),
        TiffError::BadTileGeometry {
            width: 20,
            height: 16
        }
    );
}

/// `PlanarConfiguration` 2: every red, then every green, then every blue, each
/// in its own strips (p.38).
#[test]
fn a_planar_image_interleaves_its_three_sets_of_strips() {
    let (w, h) = (4u32, 4u32);
    let pixels = distinct_rgb(w, h);
    let mut planes: Vec<Vec<u8>> = vec![Vec::new(); 3];
    for pixel in pixels.chunks_exact(3) {
        for (index, plane) in planes.iter_mut().enumerate() {
            plane.push(pixel[index]);
        }
    }
    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, w))
        .tag(long(TAG_IMAGE_LENGTH, h))
        .tag(shorts(TAG_BITS_PER_SAMPLE, &[8, 8, 8]))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 2))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 3))
        .tag(short(TAG_PLANAR_CONFIGURATION, 2))
        .tag(long(TAG_ROWS_PER_STRIP, h))
        .segments(planes, false)
        .build();
    assert_eq!(tiff_decode(&file, &CAP).expect("decodes").data, pixels);
}

#[test]
fn fewer_segments_than_the_geometry_needs_is_refused() {
    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 4))
        .tag(long(TAG_IMAGE_LENGTH, 8))
        .tag(short(TAG_BITS_PER_SAMPLE, 8))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 1))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
        .tag(long(TAG_ROWS_PER_STRIP, 2))
        // Four strips are needed; two are given.
        .segments(vec![vec![0; 8], vec![0; 8]], false)
        .build();
    assert_eq!(
        tiff_decode(&file, &CAP).unwrap_err(),
        TiffError::InconsistentSegments
    );
}

// ---- the predictor -----------------------------------------------------

#[test]
fn horizontal_differencing_is_undone_at_every_depth_and_both_orders() {
    for little in [true, false] {
        // Eight-bit RGB, three components, so the left neighbour is three
        // samples back rather than one.
        let pixels = distinct_rgb(6, 3);
        let mut coded = pixels.clone();
        for row in coded.chunks_exact_mut(6 * 3) {
            for i in (3..row.len()).rev() {
                row[i] = row[i].wrapping_sub(row[i - 3]);
            }
        }
        let file = TiffFile::new(little)
            .tag(long(TAG_IMAGE_WIDTH, 6))
            .tag(long(TAG_IMAGE_LENGTH, 3))
            .tag(shorts(TAG_BITS_PER_SAMPLE, &[8, 8, 8]))
            .tag(short(TAG_COMPRESSION, 5))
            .tag(short(TAG_PHOTOMETRIC, 2))
            .tag(short(TAG_SAMPLES_PER_PIXEL, 3))
            .tag(short(TAG_PREDICTOR, 2))
            .tag(long(TAG_ROWS_PER_STRIP, 3))
            .segments(vec![encode_lzw(&coded, true, true)], false)
            .build();
        assert_eq!(
            tiff_decode(&file, &CAP).expect("decodes").data,
            pixels,
            "little_endian = {little}"
        );
    }
}

/// The 16-bit predictor differences whole samples in the **file's** byte
/// order, which is the one place a little-endian TIFF and PDF's own
/// `/Predictor 2` disagree.
#[test]
fn the_sixteen_bit_predictor_follows_the_files_byte_order() {
    let values: [u16; 6] = [0x1000, 0x1234, 0x0001, 0xFFFF, 0x8000, 0x0100];
    for little in [true, false] {
        let mut coded: Vec<u16> = values.to_vec();
        for i in (1..coded.len()).rev() {
            coded[i] = coded[i].wrapping_sub(coded[i - 1]);
        }
        let strip: Vec<u8> = coded
            .iter()
            .flat_map(|v| {
                if little {
                    v.to_le_bytes()
                } else {
                    v.to_be_bytes()
                }
            })
            .collect();
        let file = TiffFile::new(little)
            .tag(long(TAG_IMAGE_WIDTH, 6))
            .tag(long(TAG_IMAGE_LENGTH, 1))
            .tag(short(TAG_BITS_PER_SAMPLE, 16))
            .tag(short(TAG_COMPRESSION, 1))
            .tag(short(TAG_PHOTOMETRIC, 1))
            .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
            .tag(short(TAG_PREDICTOR, 2))
            .tag(long(TAG_ROWS_PER_STRIP, 1))
            .segments(vec![strip], false)
            .build();
        assert_eq!(
            tiff_decode(&file, &CAP).expect("decodes").data,
            values
                .iter()
                .flat_map(|v| v.to_be_bytes())
                .collect::<Vec<u8>>(),
            "little_endian = {little}"
        );
    }
}

#[test]
fn a_predictor_this_build_does_not_implement_is_named() {
    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 1))
        .tag(long(TAG_IMAGE_LENGTH, 1))
        .tag(short(TAG_BITS_PER_SAMPLE, 8))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 1))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
        // 3 is the floating-point predictor of Technical Note 3.
        .tag(short(TAG_PREDICTOR, 3))
        .tag(long(TAG_ROWS_PER_STRIP, 1))
        .segments(vec![vec![0]], false)
        .build();
    assert_eq!(
        tiff_decode(&file, &CAP).unwrap_err(),
        TiffError::UnsupportedPredictor(3)
    );
}

// ---- extra samples -----------------------------------------------------

#[test]
fn an_unassociated_alpha_channel_arrives_as_the_fourth_component() {
    let samples = vec![10u8, 20, 30, 128, 200, 210, 220, 255];
    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 2))
        .tag(long(TAG_IMAGE_LENGTH, 1))
        .tag(shorts(TAG_BITS_PER_SAMPLE, &[8, 8, 8, 8]))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 2))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 4))
        // p.31: 2 is unassociated alpha, which is not premultiplied.
        .tag(short(TAG_EXTRA_SAMPLES, 2))
        .tag(long(TAG_ROWS_PER_STRIP, 1))
        .segments(vec![samples.clone()], false)
        .build();
    let img = tiff_decode(&file, &CAP).expect("decodes");
    assert_eq!(img.colour, TiffColour::Rgba);
    assert!(img.colour.has_alpha());
    assert_eq!(img.data, samples);
}

#[test]
fn associated_alpha_is_divided_back_out_of_the_colour() {
    // p.31: associated alpha is premultiplied. Half-opaque mid grey is stored
    // as (64, 64, 64, 128) and means (128, 128, 128) at half opacity.
    let file = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 1))
        .tag(long(TAG_IMAGE_LENGTH, 1))
        .tag(shorts(TAG_BITS_PER_SAMPLE, &[8, 8, 8, 8]))
        .tag(short(TAG_COMPRESSION, 1))
        .tag(short(TAG_PHOTOMETRIC, 2))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 4))
        .tag(short(TAG_EXTRA_SAMPLES, 1))
        .tag(long(TAG_ROWS_PER_STRIP, 1))
        .segments(vec![vec![64, 64, 64, 128]], false)
        .build();
    let img = tiff_decode(&file, &CAP).expect("decodes");
    assert_eq!(img.data, vec![128, 128, 128, 128]);
}

// ---- bounds ------------------------------------------------------------

/// Twenty-six bytes of tag data can ask for 2^62 samples, which is what makes
/// the cap something other than decoration.
#[test]
fn an_image_past_the_sample_cap_is_refused_before_it_allocates() {
    let file = image(
        Simple {
            little: true,
            width: u32::MAX,
            height: u32::MAX,
            depth: 8,
            samples: 3,
            photometric: 2,
            compression: 1,
        },
        vec![0; 4],
    );
    match tiff_decode(&file, &Limits::new(usize::MAX)) {
        Err(TiffError::TooManySamples { samples, max }) => {
            assert_eq!(max, MAX_TIFF_SAMPLES);
            assert!(samples > MAX_TIFF_SAMPLES);
        }
        other => panic!("expected a sample-cap refusal, got {other:?}"),
    }
}

#[test]
fn a_raster_past_the_callers_own_ceiling_is_refused_under_the_callers_number() {
    let file = image(
        Simple {
            little: true,
            width: 64,
            height: 64,
            depth: 8,
            samples: 3,
            photometric: 2,
            compression: 1,
        },
        vec![0; 64 * 64 * 3],
    );
    match tiff_decode(&file, &Limits::new(1)) {
        Err(TiffError::ExceedsOutputLimit { bytes, limit }) => {
            assert_eq!(limit, 1);
            assert_eq!(bytes, 64 * 64 * 3);
        }
        other => panic!("expected the caller's ceiling, got {other:?}"),
    }
}

#[test]
fn a_strip_that_is_not_inside_the_file_leaves_its_rows_blank() {
    let mut file = image(
        Simple {
            little: true,
            width: 4,
            height: 2,
            depth: 8,
            samples: 1,
            photometric: 1,
            compression: 1,
        },
        vec![1, 2, 3, 4, 5, 6, 7, 8],
    );
    // Point the (single, inline) strip offset past the end of the file.
    let entries = u16::from_le_bytes([file[8], file[9]]) as usize;
    for index in 0..entries {
        let at = 10 + index * 12;
        if u16::from_le_bytes([file[at], file[at + 1]]) == TAG_STRIP_OFFSETS {
            file[at + 8..at + 12].copy_from_slice(&0xFFFF_0000u32.to_le_bytes());
        }
    }
    let img = tiff_decode(&file, &CAP).expect("the directory is still readable");
    assert_eq!(img.data, vec![0; 8]);
    assert!(!img.complete, "a strip that is not there is not complete");
}

#[test]
fn a_truncated_strip_keeps_the_rows_that_arrived() {
    let pixels = distinct_rgb(4, 4);
    let mut short = pixels.clone();
    short.truncate(4 * 3 * 2);
    let file = image(
        Simple {
            little: true,
            width: 4,
            height: 4,
            depth: 8,
            samples: 3,
            photometric: 2,
            compression: 1,
        },
        short,
    );
    let img = tiff_decode(&file, &CAP).expect("decodes");
    assert!(!img.complete);
    assert_eq!(&img.data[..24], &pixels[..24]);
    assert_eq!(&img.data[24..], &vec![0u8; 24][..]);
}

// ---- hostile input, on stable ------------------------------------------

/// A hand-rolled xorshift, so the sweep is identical everywhere (ruling 4).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

/// The fixtures above, damaged deterministically, decoded, and required only
/// not to panic.
///
/// `crates/tinker-pdf/tests/hostile_input.rs` is this idea for whole
/// documents and it is where the reasoning is written down: `fuzz/` covers the
/// same entry points far more deeply and needs a nightly toolchain, so nobody
/// runs it by accident, and ruling 1 would be enforced by review alone between
/// releases. This is the TIFF-shaped version, close to the builders so a
/// mutation lands on a field rather than on a wrapper.
///
/// The damage is aimed where a TIFF is trusting: a directory entry's count and
/// offset, a strip's offset, and the length prefixes each of those indexes
/// with. A flipped byte in the entry array is worth a hundred flipped bytes in
/// a strip, because a strip that decodes to nonsense is *allowed* to.
#[test]
fn mutated_fixtures_never_panic() {
    let pixels = distinct_rgb(8, 4);
    let rows = fax_rows();
    let originals: Vec<Vec<u8>> = vec![
        image(
            Simple {
                little: true,
                width: 8,
                height: 4,
                depth: 8,
                samples: 3,
                photometric: 2,
                compression: 1,
            },
            pixels.clone(),
        ),
        image(
            Simple {
                little: false,
                width: 8,
                height: 4,
                depth: 8,
                samples: 3,
                photometric: 2,
                compression: 5,
            },
            encode_lzw(&pixels, true, true),
        ),
        image(
            Simple {
                little: true,
                width: 8,
                height: 4,
                depth: 8,
                samples: 3,
                photometric: 2,
                compression: 5,
            },
            encode_lzw(&pixels, false, false),
        ),
        image(
            Simple {
                little: true,
                width: 8,
                height: 4,
                depth: 8,
                samples: 3,
                photometric: 2,
                compression: 32773,
            },
            encode_packbits(&pixels),
        ),
        image(
            Simple {
                little: false,
                width: 8,
                height: 4,
                depth: 8,
                samples: 3,
                photometric: 2,
                compression: 8,
            },
            crate::zlib_compress(&pixels),
        ),
        TiffFile::new(true)
            .tag(long(TAG_IMAGE_WIDTH, 16))
            .tag(long(TAG_IMAGE_LENGTH, rows.len() as u32))
            .tag(short(TAG_BITS_PER_SAMPLE, 1))
            .tag(short(TAG_COMPRESSION, 4))
            .tag(short(TAG_PHOTOMETRIC, 0))
            .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
            .tag(long(TAG_ROWS_PER_STRIP, rows.len() as u32))
            .segments(vec![encode_ccitt(&rows, Fax::G4)], false)
            .build(),
    ];

    let mut rng = Rng(0x5DEE_CE66_D1CE_B00D);
    for original in &originals {
        for round in 0..400u32 {
            let mut bytes = original.clone();
            match round % 4 {
                // A byte anywhere.
                0 => {
                    let at = (rng.next() as usize) % bytes.len();
                    bytes[at] ^= (rng.next() & 0xFF) as u8;
                }
                // A byte inside the entry array, where every length and every
                // offset lives.
                1 => {
                    let entries = usize::from(u16::from_le_bytes([bytes[8], bytes[9]]))
                        .min(usize::from(u16::from_be_bytes([bytes[8], bytes[9]])));
                    let span = (2 + entries * 12).min(bytes.len() - 8);
                    let at = 8 + (rng.next() as usize) % span.max(1);
                    bytes[at] ^= (rng.next() & 0xFF) as u8;
                }
                // Truncation, which is what a half-copied file looks like.
                2 => {
                    let keep = (rng.next() as usize) % bytes.len();
                    bytes.truncate(keep);
                }
                // A four-byte word set to something enormous, which is how an
                // offset or a dimension attacks an allocation.
                _ => {
                    let at = ((rng.next() as usize) % bytes.len().saturating_sub(4).max(1)) & !3;
                    if at + 4 <= bytes.len() {
                        bytes[at..at + 4].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
                    }
                }
            }
            // Only that it did not panic and did not allocate the world. A
            // mutated file that decodes is not required to be *right*.
            if let Ok(image) = tiff_decode(&bytes, &Limits::new(1 << 20)) {
                assert!(image.data.len() <= 1 << 20);
            }
            let _ = tiff_decode(&bytes, &Limits::new(1));
        }
    }
}

// ---- the fuzz corpus ---------------------------------------------------

/// Writes the six seeds `fuzz/corpus/tiff/` carries, so the seeds and the
/// fixtures here cannot drift apart.
///
/// Run with `--ignored` when a fixture changes; the corpus is committed, and a
/// run that rewrites it is a diff to look at rather than to apply blindly.
///
/// Each seed is the target's **control byte** and then a file, because the
/// first byte is what picks the output ceiling. One of the six carries a
/// control byte of zero — a ceiling of one byte — since that is the only value
/// from which `ExceedsOutputLimit` fires on an otherwise perfectly good file.
#[test]
#[ignore = "writes into fuzz/corpus/tiff, which is committed"]
fn write_the_fuzz_seeds() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/tiff");

    let pixels = distinct_rgb(8, 4);
    let rows = fax_rows();
    let map: Vec<u16> = (0..12u16).map(|v| v * 5000).collect();

    let strip_lzw = image(
        Simple {
            little: true,
            width: 8,
            height: 4,
            depth: 8,
            samples: 3,
            photometric: 2,
            compression: 5,
        },
        encode_lzw(&pixels, true, true),
    );
    let old_lzw = image(
        Simple {
            little: false,
            width: 8,
            height: 4,
            depth: 8,
            samples: 3,
            photometric: 2,
            compression: 5,
        },
        encode_lzw(&pixels, false, false),
    );
    let g4 = TiffFile::new(false)
        .tag(long(TAG_IMAGE_WIDTH, 16))
        .tag(long(TAG_IMAGE_LENGTH, rows.len() as u32))
        .tag(short(TAG_BITS_PER_SAMPLE, 1))
        .tag(short(TAG_COMPRESSION, 4))
        .tag(short(TAG_PHOTOMETRIC, 0))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
        .tag(long(TAG_ROWS_PER_STRIP, rows.len() as u32))
        .segments(vec![encode_ccitt(&rows, Fax::G4)], false)
        .build();
    let palette = TiffFile::new(true)
        .tag(long(TAG_IMAGE_WIDTH, 4))
        .tag(long(TAG_IMAGE_LENGTH, 2))
        .tag(short(TAG_BITS_PER_SAMPLE, 2))
        .tag(short(TAG_COMPRESSION, 32773))
        .tag(short(TAG_PHOTOMETRIC, 3))
        .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
        .tag(long(TAG_ROWS_PER_STRIP, 2))
        .tag(shorts(TAG_COLOR_MAP, &map))
        .segments(
            vec![encode_packbits(&[0b00_01_10_11, 0b11_10_01_00])],
            false,
        )
        .build();
    let tiled = {
        let tile = vec![0x7Fu8; 16 * 16];
        TiffFile::new(true)
            .tag(long(TAG_IMAGE_WIDTH, 16))
            .tag(long(TAG_IMAGE_LENGTH, 16))
            .tag(short(TAG_BITS_PER_SAMPLE, 8))
            .tag(short(TAG_COMPRESSION, 1))
            .tag(short(TAG_PHOTOMETRIC, 1))
            .tag(short(TAG_SAMPLES_PER_PIXEL, 1))
            .tag(long(TAG_TILE_WIDTH, 16))
            .tag(long(TAG_TILE_LENGTH, 16))
            .segments(vec![tile], true)
            .build()
    };

    for (name, bytes) in [
        ("lzw-rgb-strip", [&[0x03u8][..], &strip_lzw].concat()),
        ("lzw-old-style-mm", [&[0x03][..], &old_lzw].concat()),
        ("g4-bilevel", [&[0x03][..], &g4].concat()),
        ("palette-packbits", [&[0x03][..], &palette].concat()),
        ("tiled-grey", [&[0x03][..], &tiled].concat()),
        // The same file against a one-byte ceiling.
        ("lzw-rgb-strip-tight", [&[0x00][..], &strip_lzw].concat()),
    ] {
        std::fs::write(base.join(name), bytes).expect("the corpus directory exists");
    }
}
