//! PNG encoding (ISO/IEC 15948 clauses 5, 9 and 11) — the writer half of
//! [`super`], and the only image encoder in this crate.
//!
//! # Why it is here and not in the tool that wanted it
//!
//! `tools/tpdf`'s `write_pnm` carried the argument against this for as long as
//! it existed: "a PNG writer would mean a deflate encoder in a test tool, and
//! the engine already has one it should not depend on from here." The premise
//! was right and the conclusion was backwards. Everything a PNG writer needs is
//! *already in this crate and nowhere else*: [`crate::zlib_compress`] is RFC
//! 1950, which is byte for byte what 10.3 puts inside IDAT; [`Crc32`] is 5.3's
//! per-chunk checksum with the resumable shape 5.3 needs, because the checksum
//! covers a chunk's type and its data and those are never adjacent in one
//! buffer; and `predictors.rs` already owns 9.2's predictor because PDF's
//! `/Predictor 15` *is* 9.2, adopted wholesale. A writer anywhere else would
//! have had to reach for all three or copy all three.
//!
//! So it sits beside the decoder, which is also what lets
//! `tests/png_suite.rs` hold the encoder to PngSuite directly rather than
//! through a facade.
//!
//! # Eight bits, and the round trip says so
//!
//! [`PngSource`] carries 8-bit components only. Table 11.1 permits 16 for four
//! of the five colour types and this writer never emits one, because the thing
//! it was built to write is a `tinker_pdf::Bitmap` and **`PixelFormat` has no
//! 16-bit variant**: every buffer in `tinker-pdf-raster` is a byte a channel.
//! Encoding at 16 would mean widening samples that carry eight bits of
//! information into two bytes that carry eight bits of information, doubling
//! every file to say nothing new.
//!
//! The consequence is stated here rather than left for a reader to discover in
//! a failing assertion: **a 16-bit PNG decoded and re-encoded through this
//! module comes back at 8-bit precision.** `tests/png_suite.rs` narrows the
//! first decode to eight bits before it compares, and says so at the assertion.
//! The round trip is at `Bitmap` precision, which is the precision the caller
//! that asked for it has.
//!
//! # No palette, no interlace, and why neither is a gap
//!
//! Colour types 0, 2, 4 and 6 are written; type 3 is not. A palette is a
//! *compression* decision — it makes a file smaller when the image has at most
//! 256 distinct colours — and choosing one means counting colours over the
//! whole raster and giving up when the count passes 256, which is a second
//! pass over the image to sometimes save bytes. The row filters below already
//! take the flat regions an indexed image would, and a decoder sees the same
//! pixels either way. Interlace is the same shape of decision one layer up: it
//! exists so a partial download shows a blurry whole image, and nothing here
//! writes to a socket.
//!
//! Both are read by [`super::png_decode`], which is the direction that matters:
//! a file somebody else wrote may be anything Table 11.1 permits, and a file
//! this engine writes is one of four things it chose.

use super::{ChunkType, PngColour, MAX_CHUNK_LEN, MAX_DIMENSION};
use crate::crc32::Crc32;
use crate::deflate::zlib_compress;
use crate::predictors::paeth;

/// Why a raster could not be written as a PNG.
///
/// All four are a *caller's* description of its own buffer failing to describe
/// a picture — not damage in data read from somewhere, which is what ruling 2
/// degrades for. There is nothing to degrade to here: a PNG with no pixels is
/// not a smaller PNG, and [`super::PngError::BadDimensions`] is what this
/// crate's own decoder answers a file claiming one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngEncodeError {
    /// A zero width or height, or one past 11.2.2's `2^31 - 1`.
    BadDimensions { width: u32, height: u32 },
    /// The row stride does not reach the end of a row. A stride *larger* than
    /// the row is normal — it is a padded buffer, and the padding is skipped —
    /// but a smaller one means the rows overlap, and an encoder that wrote
    /// them anyway would produce a sheared picture rather than a refusal.
    ShortStride { stride: usize, row_bytes: u64 },
    /// Fewer bytes than `stride x (height - 1) + row_bytes`. The last row is
    /// charged at its real width rather than at the stride, because a caller
    /// is entitled to stop its buffer at the final pixel.
    ShortData { have: usize, need: u64 },
}

impl core::fmt::Display for PngEncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadDimensions { width, height } => {
                write!(f, "image dimensions {width} x {height}")
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

impl std::error::Error for PngEncodeError {}

/// An interleaved 8-bit raster, and where its rows begin.
///
/// Borrowed rather than owned: the caller already holds the pixels, and the
/// one caller that matters — `tinker_pdf::Bitmap::to_png` — holds a whole
/// rendered page.
#[derive(Clone, Copy, Debug)]
pub struct PngSource<'a> {
    pub width: u32,
    pub height: u32,
    /// Which of the four colour types [`png_encode`] writes. There is no
    /// indexed variant here for the reason the module header gives.
    pub colour: PngColour,
    /// Bytes from the start of one row to the start of the next. At least
    /// `width x colour.components()`; more means the buffer is padded and the
    /// tail of each row is not written.
    ///
    /// **This is the field an encoder is most likely to ignore**, because for
    /// every unpadded buffer it equals the row length and a version that reads
    /// the raster as one contiguous run produces byte-identical output. A
    /// `Canvas` does not pad today, so nothing in this engine would notice —
    /// which is exactly why `a_padded_stride_is_not_read_as_pixels` builds one
    /// that does.
    pub stride: usize,
    pub data: &'a [u8],
}

/// 9.2's five filter types, in the specification's own numbering — which is
/// also the byte that precedes every scanline.
const FILTERS: [u8; 5] = [0, 1, 2, 3, 4];

/// Writes a complete PNG file: signature, IHDR, IDAT, IEND.
///
/// # Errors
/// Any [`PngEncodeError`] — all three are a raster that does not describe a
/// picture.
pub fn png_encode(source: &PngSource<'_>) -> Result<Vec<u8>, PngEncodeError> {
    let (width, height) = (source.width, source.height);
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(PngEncodeError::BadDimensions { width, height });
    }

    // In `u64` throughout, and compared before anything is converted: on a
    // 32-bit target `width x components` overflows a `usize` while still being
    // a width 11.2.2 permits, and an overflowed row length is a slice inside
    // the buffer rather than a refusal (ruling 1). A stride is a `usize`, so a
    // row length past `usize` is caught by the stride comparison itself.
    let components = u64::from(source.colour.components());
    let row_bytes = u64::from(width) * components;
    let stride = source.stride as u64;
    if stride < row_bytes {
        return Err(PngEncodeError::ShortStride {
            stride: source.stride,
            row_bytes,
        });
    }
    // The last row is charged at `row_bytes` and not at the stride: a caller
    // whose buffer ends at the final pixel has given us every pixel.
    let need = stride
        .saturating_mul(u64::from(height) - 1)
        .saturating_add(row_bytes);
    if (source.data.len() as u64) < need {
        return Err(PngEncodeError::ShortData {
            have: source.data.len(),
            need,
        });
    }
    // Both comparisons above passed, so `row_bytes <= stride <= data.len()`.
    let row_bytes = row_bytes as usize;
    let bpp = source.colour.components() as usize;

    let filtered = filter_rasters(source, row_bytes, bpp);

    let mut out = Vec::from(super::PNG_SIGNATURE);
    chunk(&mut out, super::IHDR, &ihdr(source));
    // 5.3 caps a chunk at `2^31 - 1` bytes and 10.3 lets one zlib stream span
    // any number of IDATs, so the compressed bytes are cut at that ceiling
    // rather than refused. For every raster this engine can render the loop
    // runs exactly once — a page is capped at 67.1 Mpx, which is 268 MB of
    // RGBA before compression — and it is written as a loop anyway because
    // `Vec::chunks` costs nothing when there is one chunk, where a refusal
    // would be a branch that can only ever fire on a caller's own buffer.
    for part in zlib_compress(&filtered).chunks(MAX_CHUNK_LEN as usize) {
        chunk(&mut out, super::IDAT, part);
    }
    chunk(&mut out, super::IEND, b"");
    Ok(out)
}

/// 11.2.2's thirteen bytes.
fn ihdr(source: &PngSource<'_>) -> Vec<u8> {
    let mut d = Vec::with_capacity(13);
    d.extend_from_slice(&source.width.to_be_bytes());
    d.extend_from_slice(&source.height.to_be_bytes());
    d.push(8);
    d.push(colour_type(source.colour));
    // Compression method 0 (the zlib deflate of 10.3), filter method 0 (the
    // five adaptive filters of 9.2), interlace method 0 (one raster). No
    // other value of any of the three has ever been defined, and `png.rs`
    // refuses every other value on the way in.
    d.extend_from_slice(&[0, 0, 0]);
    d
}

/// Table 11.1's colour-type column, for the four layouts this writes.
///
/// A `match` rather than arithmetic on the component count, which is the
/// plausible version that gets it wrong: grey+alpha has two components and is
/// type **4**, truecolour has three and is type **2**, so the two orderings
/// disagree and a reader handed the wrong one reads a two-channel image as a
/// three-channel one and runs off the end of every row.
const fn colour_type(colour: PngColour) -> u8 {
    match colour {
        PngColour::Grey => 0,
        PngColour::Rgb => 2,
        PngColour::GreyAlpha => 4,
        PngColour::Rgba => 6,
    }
}

/// Every scanline filtered by whichever of the five costs least, each one
/// prefixed with its type byte (9.1), concatenated into the stream that goes
/// inside IDAT.
fn filter_rasters(source: &PngSource<'_>, row_bytes: usize, bpp: usize) -> Vec<u8> {
    // One tag plus one row per scanline, exactly. Saturating rather than plain
    // arithmetic: `png_encode` has already shown that `row_bytes x height` fits
    // the caller's buffer, but the extra tag byte a row is not in that buffer,
    // and a reservation that wraps is an allocation smaller than what is about
    // to be pushed into it (ruling 1).
    let mut filtered = Vec::with_capacity(
        row_bytes
            .saturating_add(1)
            .saturating_mul(source.height as usize),
    );
    let mut prior = vec![0u8; row_bytes];
    let mut current = vec![0u8; row_bytes];
    let mut candidate = vec![0u8; row_bytes];
    let mut best = vec![0u8; row_bytes];

    for y in 0..source.height as usize {
        let at = y * source.stride;
        // Bounded by the `ShortData` check in `png_encode`, and read through
        // `get` regardless (ruling 1): a `Vec` index is a panic and this one
        // is arithmetic on a caller's number.
        match source.data.get(at..at + row_bytes) {
            Some(row) => current.copy_from_slice(row),
            None => current.fill(0),
        }

        let (mut best_kind, mut best_cost) = (0u8, u64::MAX);
        for kind in FILTERS {
            filter_row(kind, &current, &prior, bpp, &mut candidate);
            let cost = cost_of(&candidate);
            // Strictly less, so the first of a tie wins and the choice does
            // not depend on iteration order being stable (ruling 4). 9.2's own
            // ordering puts None first, which is the one a decoder unfilters
            // fastest, so a tie resolving to it is the right tie to take.
            if cost < best_cost {
                best_cost = cost;
                best_kind = kind;
                best.copy_from_slice(&candidate);
            }
        }

        filtered.push(best_kind);
        filtered.extend_from_slice(&best);
        core::mem::swap(&mut prior, &mut current);
    }
    filtered
}

/// ISO/IEC 15948 9.2, Table 9.1 — the **filtering** direction, which is the
/// reconstruction formulas of `predictors.rs` read right to left.
///
/// `a` is the byte `bpp` to the left, `b` the byte above, `c` the byte above
/// and to the left; each is zero where it falls outside the image, which 9.2
/// states explicitly rather than leaving to a convention.
///
/// The encoder's `a`, `b` and `c` come from the **original** scanlines, not
/// from anything reconstructed. That is not an optimisation: 9.2's decoder
/// reconstructs to exactly the original bytes, so the two agree, and reading
/// the originals is what makes every row's five candidates computable without
/// having decoded the four that were not chosen.
fn filter_row(kind: u8, row: &[u8], prior: &[u8], bpp: usize, out: &mut [u8]) {
    for i in 0..row.len() {
        let x = row[i];
        let a = if i >= bpp { row[i - bpp] } else { 0 };
        let b = prior.get(i).copied().unwrap_or(0);
        let c = if i >= bpp {
            prior.get(i - bpp).copied().unwrap_or(0)
        } else {
            0
        };
        let Some(slot) = out.get_mut(i) else { return };
        *slot = match kind {
            // 0, None.
            0 => x,
            // 1, Sub.
            1 => x.wrapping_sub(a),
            // 2, Up.
            2 => x.wrapping_sub(b),
            // 3, Average. 9.2 is explicit that the sum is formed without
            // overflow and the floor is taken before the subtraction, which is
            // why the addition happens in `u16` and not in `u8`.
            3 => x.wrapping_sub(((u16::from(a) + u16::from(b)) / 2) as u8),
            // 4, Paeth. The predictor itself is `predictors.rs`'s, which is
            // the one this crate's decoder unfilters with and the one PDF's
            // `/Predictor 15` reaches. **Deliberately shared rather than
            // transcribed twice**: the two directions must agree byte for byte
            // or nothing round-trips, and a second copy is a second thing to
            // get wrong. What sharing costs is that a defect in it cancels
            // between this encoder and that decoder — which is precisely the
            // symmetry `png_suite.rs`'s independently transcribed unfilter
            // exists to break, and the injection matrix measured it: reversing
            // the tie-break leaves the whole round trip green — 0 of the 162
            // files it compares notice — and is caught by that transcription
            // and by two decoder-side PngSuite comparisons that pit third-party
            // files against each other rather than against our own output.
            // Three of 2 014, and not one of them a round trip.
            _ => x.wrapping_sub(paeth(a, b, c)),
        };
    }
}

/// The heuristic ISO/IEC 15948 12.8 recommends for choosing a filter: **the
/// minimum sum of absolute differences**, over the filtered bytes read as
/// signed values.
///
/// Signed is the whole content of the rule and the easy half to lose. A
/// filtered byte of `0xFF` is a difference of **-1**, which is a very good
/// prediction; read unsigned it is 255, the worst score there is, and a
/// heuristic reading it that way rejects exactly the rows the filter helped
/// most. So `128..=255` counts as `256 - b`, which is `|b - 256|`.
///
/// Integer arithmetic only, and a `u64` accumulator that cannot overflow: the
/// largest term is 128 and the longest row this crate will encode is bounded
/// by a `usize`, so the sum is under `2^63` on any target (ruling 4). A float
/// score would make the chosen filter — and therefore every compressed byte —
/// depend on the target's rounding.
fn cost_of(filtered: &[u8]) -> u64 {
    filtered
        .iter()
        .map(|&b| u64::from(if b < 128 { b } else { 0u8.wrapping_sub(b) }))
        .sum()
}

/// 5.3's chunk layout: a four-byte length, a four-byte type, the data, and a
/// CRC-32 **over the type and the data and not over the length**.
///
/// That exclusion is the one thing about a PNG chunk that is easy to state
/// wrongly and impossible to see: a checksum over `length || type || data` is
/// self-consistent, so a writer and a reader that shared the mistake would
/// agree perfectly and every other program in the world would reject the file.
/// It is why [`Crc32`] is resumable rather than a one-slice function — the two
/// covered runs are never adjacent in one buffer — and why
/// `the_crc_covers_the_type_and_the_data_and_not_the_length` checks the number
/// against a value computed from the two slices separately. Dropping the type
/// from the sum is caught by 22 assertions, the widest of any defect in this
/// module's injection matrix: it makes every file this engine writes
/// unreadable, including by this engine.
fn chunk(out: &mut Vec<u8>, kind: ChunkType, data: &[u8]) {
    // Every caller is either a fixed 13 bytes, an empty IEND, or a slice cut
    // at `MAX_CHUNK_LEN`, so the conversion cannot fail; `unwrap_or` rather
    // than an `unwrap` all the same (ruling 1), and the fallback is the
    // ceiling 5.3 names so a hypothetical over-long chunk is refused by a
    // reader rather than mis-parsed by one.
    let len = u32::try_from(data.len()).unwrap_or(MAX_CHUNK_LEN);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&kind.0);
    out.extend_from_slice(data);
    let mut c = Crc32::new();
    c.update(&kind.0);
    c.update(data);
    out.extend_from_slice(&c.finish().to_be_bytes());
}

#[cfg(test)]
mod tests;
