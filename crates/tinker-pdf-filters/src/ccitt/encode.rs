//! CCITT Group 4 encoding (ITU-T T.6 §2.2) — the writer half of [`super`].
//!
//! # Where it came from
//!
//! `tiff/tests.rs` has carried a T.4/T.6 coder since the TIFF decoder landed,
//! for the reason that file's header states: "a reader tested only against
//! what its own writer emits proves less than it looks", so its fixtures are
//! coded from the specification rather than from `ccitt.rs`. That coder is
//! what this module is promoted from, and promoting it cost three things the
//! fixture version could do without:
//!
//! - **The run tables stopped at sixteen.** Every TIFF fixture is sixteen
//!   pixels wide, so the fixture tables were seventeen entries of T.4 Table 2
//!   and no make-up codes at all — with an `assert!(run <= 16)` where the rest
//!   of the table would have been. A real encoder needs all 64 terminating
//!   codes, both 27-entry make-up tables, T.4 Table 3b's shared extension, and
//!   the rule for a run past 2 623 pels.
//! - **It coded `Vec<Vec<bool>>`.** A caller here holds packed rows —
//!   `/BitsPerComponent 1`, most significant bit first — because that is what
//!   [`super::decode`] produces, so this takes the same layout with the same
//!   `/BlackIs1` polarity parameter rather than a pixel a byte.
//! - **It had no end-of-block and no ceiling.** `/EndOfBlock` is a PDF
//!   parameter with a default of true (7.4.6, Table 11), and T.6 §2.4.1.1's
//!   EOFB is what it names.
//!
//! **`tiff/tests.rs`'s coder stays where it is, and deleting it would be a
//! regression rather than a tidy-up.** It is the fixture builder for the TIFF
//! decoder's compression 2, 3 and 4 tests, and it has its own seventeen-entry
//! tables transcribed separately. Re-pointing those fixtures at this module
//! would make them a round trip against a coder that shares `ccitt.rs`'s
//! tables — exactly the self round trip that file's header was written to
//! avoid. Two independent transcriptions of T.4 Table 2 is the feature, not
//! the duplication.
//!
//! # G4 only, and what that leaves out
//!
//! This writes `/K` negative and nothing else: pure two-dimensional coding,
//! no EOL, no tag bit, no byte alignment between rows. The three one-dimensional
//! shapes the *decoder* reads — `/K` zero, `/K` positive with T.4 §4.2.1.3.1's
//! per-line tag, and TIFF's compression 2 with its byte-aligned rows — are not
//! written here. That is a deliberate stop rather than an oversight: G4 is
//! strictly smaller than any of them on every image, a PDF writer choosing a
//! fax coding has no reason to pick a worse one, and each of the others is a
//! different framing around the *same* row coder that a caller which wanted it
//! would have to ask for by name. The uncompressed mode of T.6 §2.3.1 is not
//! written either, and neither is the extension code that would enter it.
//!
//! # What adjudicates it
//!
//! Two independent pieces of third-party data, and neither of them is this
//! crate's decoder:
//!
//! - **T.4 Tables 2, 3a and 3b**, and Table 4's two-dimensional code words.
//!   `T-REC-T.4-200307-I` was fetched from the ITU on 15 September 2026 and
//!   **read twice**: once out of the text layer `tpdf text` extracts, once off
//!   `tpdf render`'s pages at 170 dpi. The two readings agree on all 195 table
//!   entries, and `super::tests::the_run_tables_are_itu_t_t_4_s_own` asserts
//!   every entry the tables below emit from against that transcription rather
//!   than against either implementation. That test is the *only* thing in the
//!   tree that reaches a make-up code no fixture happens to use: changing
//!   Table 3a's white 960 fires it and one other test, and nothing else.
//! - **ITU-T T.88 Annex H.1 segment 4**, which is 26 bytes of MMR — T.6 with
//!   nothing around it (T.88 6.2.6) — coding a bitmap the same annex publishes
//!   as a picture. Both halves are the standard's: `jbig2.rs` already decodes
//!   those bytes and compares against that picture, and
//!   `annex_h_mmr_region_re_encodes_to_the_published_bytes` now encodes that
//!   picture and compares against those bytes. An encoder written to agree
//!   with a wrong decoder passes a round trip and fails this.
//!
//!   What those 26 bytes cannot reach is named rather than left implied: the
//!   frame is 54 pixels wide, so no run in it is longer than 54 and **not one
//!   make-up code is exercised**. Pass mode, horizontal mode with two run
//!   lengths, V(0), §2.2.1's imaginary white reference line and §2.2.5.1's
//!   "first run minus one" are all in them; VR(1..3), VL(1..3) and every
//!   make-up code are held by the table transcription above and by round trips
//!   that say so in their own doc comments.
//!
//! # The shape, and the two rulings that fix it
//!
//! [`CcittSource`] follows [`crate::PngSource`], which landed in this crate in
//! September 2026 and is this crate's precedent for an encoder's surface: a
//! borrowed raster with an explicit stride, plain numbers beside it, a free
//! function returning `Result<Vec<u8>, _>`, and a refusal enum whose variants
//! are all a caller mis-describing its own buffer. Both rulings were read
//! before this was settled, and neither says what a first glance suggests:
//!
//! - **Ruling 8 — leaf crates stay PDF-free — is satisfied, and the field
//!   names are not a violation of it.** The ruling bars COS types and PDF
//!   vocabulary from a leaf's public API; what crosses this boundary is four
//!   integers, two booleans and a byte slice. `columns`, `rows`, `black_is_1`
//!   and `end_of_block` are named after the `/CCITTFaxDecode` parameters
//!   because [`super::CcittParams`] — public since the decoder landed — named
//!   them that first, and one name per concept in a crate beats two. A reader
//!   coding a fax with no PDF anywhere near it still gets a coder.
//! - **Ruling 11 — the facade is the only public surface — does not reach
//!   here, and that is the point of its wording.** It makes `tinker_pdf` the
//!   surface for a *document*; a raster is not one. `png_encode` is the
//!   precedent again: the facade *projects* it as `Bitmap::to_png` because a
//!   caller of the facade holds a rendered page, and does not re-export it.
//!   Nothing on the facade holds a bilevel raster, so there is nothing to
//!   project, and inventing a facade entry point to satisfy a ruling that does
//!   not ask for one would be the actual mistake.
//!
//! # Who calls this
//!
//! **Nothing in this repository, outside `ccitt.rs`'s and `jbig2.rs`'s own
//! tests.** The roadmap row's evidence — "the writer never re-encodes image
//! bytes by contract" — is still true after this commit, because promoting a
//! coder does not change a writer's contract; it only stops the contract being
//! the reason a coder does not exist.
//!
//! What would call it: a writer that builds a `/CCITTFaxDecode` image XObject
//! from a raster — the `creation.md` refusal row's other half — and partial
//! image redaction, which the roadmap says waits on an image encoder. Both need
//! a decision about *when* to choose a fax coding over deflate that belongs to
//! whoever writes them, not here.

use super::{
    next_change, row_bytes, RunCode, BLACK_MAKEUP, BLACK_TERM, EXT_MAKEUP, WHITE_MAKEUP, WHITE_TERM,
};

/// A packed bilevel raster to be coded, in the layout [`super::decode`] emits.
///
/// Borrowed rather than owned, for [`crate::PngSource`]'s reason: the caller
/// already holds the pixels.
#[derive(Clone, Copy, Debug)]
pub struct CcittSource<'a> {
    /// `/Columns`, the pixels per row.
    pub columns: u32,
    /// `/Rows`. Unlike the decoder's, this may not be zero: a writer knows how
    /// tall its image is, and "until the data runs out" is a reader's rule.
    pub rows: u32,
    /// `/BlackIs1`: whether a 1 bit in [`Self::data`] means black. The default
    /// of false means **0 is black**, which is 7.4.6 Table 11's default and
    /// also what a one-bit DeviceGray sample means.
    pub black_is_1: bool,
    /// Bytes from the start of one row to the start of the next. At least
    /// `columns.div_ceil(8)`; more means the buffer is padded and the tail of
    /// each row is not read.
    pub stride: usize,
    /// `/EndOfBlock`: whether to close the data with T.6 §2.4.1.1's EOFB.
    /// True is 7.4.6 Table 11's default and is what a reader that does not
    /// know the row count needs.
    pub end_of_block: bool,
    pub data: &'a [u8],
}

/// Why a raster could not be coded as G4.
///
/// All three are a caller describing its own buffer wrongly, not damage in
/// data read from somewhere — [`crate::PngEncodeError`]'s distinction, for the
/// same reason. Ruling 2 degrades a *read*; there is nothing to degrade to
/// when the pixels asked for were never handed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CcittEncodeError {
    /// A zero width or height, or a width past the 65 536 columns
    /// [`super::decode`] will read back.
    BadDimensions { columns: u32, rows: u32 },
    /// The row stride does not reach the end of a row, so the rows overlap.
    ShortStride { stride: usize, row_bytes: usize },
    /// Fewer bytes than `stride x (rows - 1) + row_bytes`. The last row is
    /// charged at its real width rather than at the stride.
    ShortData { have: usize, need: u64 },
}

impl core::fmt::Display for CcittEncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadDimensions { columns, rows } => {
                write!(f, "image dimensions {columns} x {rows}")
            }
            Self::ShortStride { stride, row_bytes } => {
                write!(f, "stride of {stride} bytes for a row of {row_bytes}")
            }
            Self::ShortData { have, need } => {
                write!(f, "{have} bytes of raster, {need} needed")
            }
        }
    }
}

impl std::error::Error for CcittEncodeError {}

/// The ceiling [`super::decode`] clamps `/Columns` to, so a wider image would
/// not survive its own round trip.
const MAX_COLUMNS: u32 = 1 << 16;

/// T.6 Table 1's two-dimensional mode codes, as `(bit length, code)`.
///
/// Pass and horizontal are single values; the seven vertical codes are indexed
/// by `a1 - b1` from -3 to 3, which is the order T.4 Table 4 prints them in
/// upside down — VL(3) first, then VL(2), VL(1), V(0), VR(1), VR(2), VR(3).
const PASS: (u32, u32) = (4, 0b0001);
const HORIZONTAL: (u32, u32) = (3, 0b001);
const VERTICAL: [(u32, u32); 7] = [
    (7, 0b0000010),
    (6, 0b000010),
    (3, 0b010),
    (1, 0b1),
    (3, 0b011),
    (6, 0b000011),
    (7, 0b0000011),
];

/// T.4 §4.1.2's end-of-line, which T.6 §2.4.1.1 uses twice as its EOFB.
const EOL: (u32, u32) = (12, 0b0000_0000_0001);

/// Writes bits most significant first — [`super::Bits`] read backwards.
struct BitWriter {
    out: Vec<u8>,
    /// Bits written into the byte at the end of `out`, 0 to 7.
    used: u32,
}

impl BitWriter {
    fn new(capacity: usize) -> BitWriter {
        BitWriter {
            out: Vec::with_capacity(capacity),
            used: 0,
        }
    }

    /// The low `length` bits of `code`, most significant first.
    fn push(&mut self, (length, code): (u32, u32)) {
        for index in (0..length).rev() {
            if self.used == 0 {
                self.out.push(0);
            }
            if (code >> index) & 1 == 1 {
                if let Some(byte) = self.out.last_mut() {
                    *byte |= 0x80 >> self.used;
                }
            }
            self.used = (self.used + 1) % 8;
        }
    }

    /// The coded bytes. The final partial byte is already padded with zeros
    /// — T.6 §2.4.1.2's pad bits — because every byte is pushed zeroed and
    /// only the 1 bits are written into it.
    fn finish(self) -> Vec<u8> {
        self.out
    }
}

/// One run length, as T.6 §2.2.4 step 2 iii) spells the rule out.
///
/// Runs of 0 to 63 take a terminating code. Runs of 64 to 2 623 take the
/// make-up code "nearest, not longer" followed by a terminating code for the
/// remainder — *nearest, not longer*, which is not the same as the largest in
/// the table and not the same as dividing by 64, because T.4 Table 3a's rows
/// are 64 apart only up to 1 728 and Table 3b's carry on to 2 560. Runs of
/// 2 624 and above open with as many 2 560 make-ups as it takes to bring the
/// remainder under 2 560, and then follow the rule above.
fn write_run(bits: &mut BitWriter, white: bool, run: u32) {
    let (term, makeup): (&[RunCode], &[RunCode]) = if white {
        (&WHITE_TERM, &WHITE_MAKEUP)
    } else {
        (&BLACK_TERM, &BLACK_MAKEUP)
    };
    // Table 3b's last row, which is the only one the long-run rule names.
    let (long_length, long_code, long_run) = EXT_MAKEUP[EXT_MAKEUP.len() - 1];
    let mut left = run;
    if left >= 2624 {
        loop {
            bits.push((long_length, long_code));
            left -= u32::from(long_run);
            if left < u32::from(long_run) {
                break;
            }
        }
    }
    if left >= 64 {
        // Nearest, not longer. `max_by_key` over both tables rather than
        // arithmetic on `left / 64`: the two tables are not one arithmetic
        // sequence, and a version that assumed they were would emit a make-up
        // for 1 792 out of the *colour's* table, where no such row exists.
        if let Some(&(length, code, size)) = makeup
            .iter()
            .chain(EXT_MAKEUP.iter())
            .filter(|(_, _, size)| u32::from(*size) <= left)
            .max_by_key(|(_, _, size)| *size)
        {
            bits.push((length, code));
            left -= u32::from(size);
        }
    }
    // `left` is now under 64 and the table has 64 rows, so the index is in
    // range; `get` regardless, because a miss here would be a silent wrong
    // run rather than a panic (ruling 1).
    if let Some(&(length, code, _)) = term.get(left as usize) {
        bits.push((length, code));
    }
}

/// Where a packed row changes colour, in the representation
/// [`super::decode_row`] returns and [`super::next_change`] reads: positions
/// in increasing order, the first of which begins a black run.
fn changes_of(row: &[u8], columns: usize, black_is_1: bool) -> Vec<usize> {
    let mut out = Vec::new();
    let mut previous = false;
    for index in 0..columns {
        let byte = row.get(index >> 3).copied().unwrap_or(0);
        let set = (byte >> (7 - (index & 7))) & 1 == 1;
        let black = set == black_is_1;
        if black != previous {
            out.push(index);
            previous = black;
        }
    }
    out
}

/// The first changing element of `changes` strictly right of `at`, or
/// `columns` — T.6 §2.2.5.2's "imaginary changing element situated just after
/// the last actual element".
fn after(changes: &[usize], at: isize, columns: usize) -> usize {
    changes
        .iter()
        .copied()
        .find(|&position| (position as isize) > at)
        .unwrap_or(columns)
        .min(columns)
}

/// One coding line against its reference line — T.6 §2.2.4's flow diagram,
/// step for step.
///
/// `b1` and `b2` come from [`super::next_change`] and [`super::following`],
/// which are the decoder's own. That sharing is deliberate and is the same
/// trade `png/encode.rs` records for the Paeth predictor: the two directions
/// must agree on where b1 sits or nothing round-trips, and a second copy is a
/// second thing to get wrong. What it costs is that a defect in `next_change`
/// cancels between the two — which is exactly why the guard that matters here
/// is T.88 Annex H.1's published bitstream and not a round trip.
fn encode_row(bits: &mut BitWriter, current: &[usize], reference: &[usize], columns: usize) {
    let mut a0: isize = -1;
    let mut white = true;

    loop {
        let a1 = after(current, a0, columns);
        let b1 = next_change(reference, a0, white, columns);
        let b2 = super::following(reference, b1, columns);

        if b2 < a1 {
            // Pass. a0 moves to the element under b2 and the colour does not
            // change, because no changing element of the coding line has been
            // coded (T.6 §2.2.3.1).
            bits.push(PASS);
            a0 = b2 as isize;
        } else {
            let delta = a1 as isize - b1 as isize;
            if delta.abs() <= 3 {
                // Vertical. `delta + 3` indexes VL(3)..VR(3); the cast is safe
                // because the branch has just bounded it to -3..=3.
                bits.push(VERTICAL[(delta + 3) as usize]);
                a0 = a1 as isize;
                white = !white;
            } else {
                // Horizontal: two explicit runs, and a0 lands on a2 with the
                // colour unchanged because two runs were coded.
                let a2 = after(current, a1 as isize, columns);
                bits.push(HORIZONTAL);
                // T.6 §2.2.5.1: the first run on a line is a0a1 - 1, which is
                // a1 counted from zero — a0 sits *before* the first element.
                let start = if a0 < 0 { 0 } else { a0 as usize };
                write_run(bits, white, (a1 - start) as u32);
                write_run(bits, !white, (a2 - a1) as u32);
                a0 = a2 as isize;
            }
        }

        if a0 >= columns as isize {
            return;
        }
    }
}

/// Codes a packed bilevel raster as ITU-T T.6 two-dimensional data — PDF's
/// `/CCITTFaxDecode` with `/K` negative, and JBIG2's MMR (T.88 6.2.6) when
/// [`CcittSource::end_of_block`] is false.
///
/// # Errors
/// Any [`CcittEncodeError`] — all three are a raster that does not describe an
/// image.
pub fn g4_encode(source: &CcittSource<'_>) -> Result<Vec<u8>, CcittEncodeError> {
    let (columns, rows) = (source.columns, source.rows);
    if columns == 0 || rows == 0 || columns > MAX_COLUMNS {
        return Err(CcittEncodeError::BadDimensions { columns, rows });
    }
    let stride_needed = row_bytes(columns as usize);
    if source.stride < stride_needed {
        return Err(CcittEncodeError::ShortStride {
            stride: source.stride,
            row_bytes: stride_needed,
        });
    }
    // In `u64`, and before anything is converted: `stride x rows` is arithmetic
    // on two caller-supplied numbers and an overflowed product is a slice
    // *inside* the buffer rather than a refusal (ruling 1). The last row is
    // charged at `stride_needed` and not at the stride, because a caller whose
    // buffer ends at the final pixel has given us every pixel.
    let need = (source.stride as u64)
        .saturating_mul(u64::from(rows) - 1)
        .saturating_add(stride_needed as u64);
    if (source.data.len() as u64) < need {
        return Err(CcittEncodeError::ShortData {
            have: source.data.len(),
            need,
        });
    }

    let columns = columns as usize;
    // A 54 x 44 frame codes to 26 bytes and a page of text to well under one
    // bit a pixel, so a byte a row plus a byte per eight columns is a
    // reservation that is usually too large and never wildly so. Saturating,
    // because both terms are caller numbers.
    let guess = (rows as usize)
        .saturating_mul(stride_needed.saturating_add(1) / 4)
        .saturating_add(8);
    let mut bits = BitWriter::new(guess);
    // T.6 §2.2.1: the reference line for the first coding line is an imaginary
    // white line, which has no changing elements at all.
    let mut reference: Vec<usize> = Vec::new();

    for y in 0..rows as usize {
        let at = y * source.stride;
        // Bounded by the `ShortData` check above, and read through `get`
        // regardless (ruling 1): an empty row codes as all white rather than
        // indexing past the buffer.
        let row = source.data.get(at..at + stride_needed).unwrap_or(&[]);
        let current = changes_of(row, columns, source.black_is_1);
        encode_row(&mut bits, &current, &reference, columns);
        reference = current;
    }

    if source.end_of_block {
        bits.push(EOL);
        bits.push(EOL);
    }
    Ok(bits.finish())
}
