//! JBIG2 generic-region encoding (ITU-T T.88 6.2.5 and 7.4.6) — the writer
//! half of [`super`], over the MQ encoder of [`crate::MqEncoder`].
//!
//! # Where it came from
//!
//! Two test-only pieces, promoted together because neither is any use alone.
//! [`crate::MqEncoder`] was `#[cfg(test)]` in `mq.rs`, written so a
//! generic-region test could put a picture in and demand the same picture
//! back; `jbig2.rs`'s test module had an `encode_arithmetic` around it that
//! walked 6.2.5.7's row loop the way [`super::decode_generic_into`] walks it.
//! Promoting them cost a boundary and a refusal set and nothing else: the
//! decision sequence below is the same one, the context function is literally
//! [`super::context`], and the bytes this produces for T.88 Annex H.1's image
//! are the bytes it produced before.
//!
//! What is new is that a caller outside this crate can now reach it, so it
//! takes a **packed raster and a stride** rather than a [`super::Bitmap`], and
//! refuses a raster that does not describe an image rather than asserting.
//!
//! # "Generic region" is narrower than "JBIG2", and here is the line
//!
//! T.88 has three coding lineages and this writes one of them: clause 6.2's
//! generic region, arithmetically coded, which is what a scanner's output and
//! a bilevel mask are. Written here:
//!
//! - All four templates of Figures 8 to 11, at any adaptive-pixel positions
//!   7.4.6.3 can express.
//! - 6.2.5.7's TPGDON, with the canonical SLTP choice — toggle whenever a row
//!   starts or stops repeating the row above.
//! - 7.4.6's region segment, via [`generic_region_segment`].
//!
//! **Not written, and each for a reason rather than for lack of time:**
//!
//! - **MMR generic regions (6.2.6).** Already written, and not here:
//!   [`crate::ccitt_g4_encode`] is the T.6 coder 6.2.6 defers to, and a
//!   generic region flags byte with bit 0 set followed by its output is one.
//!   A second copy inside this module would be a second thing to get wrong.
//! - **Symbol dictionaries and text regions (6.5, 6.4).** This module's
//!   *decoder* refuses that lineage by name, and the refusal is the feature —
//!   see [`super`]'s header. An encoder for a lineage this crate will not read
//!   back could not be held to anything here.
//! - **Refinement (6.3), halftone (6.6) and pattern dictionaries (6.7).** Each
//!   is a separate coding procedure that happens to share the MQ coder;
//!   promoting the row loop of 6.2.5.7 does not promote any of them.
//! - **6.2.5.7's USESKIP.** Only 6.6.5.1's halftone regions supply a skip
//!   bitmap, and halftone regions are not written.
//! - **The rest of a JBIG2 file.** No page information segment, no file
//!   header, no embedded-stream assembly (D.3). [`generic_region_segment`]
//!   emits one segment's *data*, which is what 7.2's segment header wraps.
//!
//! # What adjudicates it
//!
//! **ITU-T T.88 Annex H.1 segment 11**, which publishes nine bytes of coded
//! data *and*, as a picture, the 54 by 44 bitmap they code, at template 0 with
//! TPGDON on and the nominal AT pixels. `jbig2.rs`'s
//! `annex_h_generic_region_re_encodes_to_the_published_bytes` encodes that
//! picture and compares against those bytes; the decoder's half of the same
//! annex has been pinned since the module landed. Annex H.2's thirty bytes pin
//! the MQ coder underneath independently.
//!
//! That pins the MQ registers and `BYTEOUT`, `FLUSH`'s trailing `FF AC`,
//! 6.2.5.7's SLTP rule, the AT layout, and *which* pixels template 0 reads.
//! **It does not pin the context numbering, and that was measured rather than
//! assumed.** Swapping two of template 0's context bits was injected on
//! 15 September 2026: these nine bytes still matched, every round trip still
//! passed, and the only test in the workspace that moved was
//! `jbig2.rs`'s `template_context_bits_match_the_figures`. `mq.rs` says why —
//! a context index is only a label into an array whose slots all start
//! identical, so any bijection of the numbering is invisible to a coder, and a
//! published bitstream is something a coder produced. **The numbering is
//! adjudicated by T.88's Figures 8 to 11, transcribed in that test, for all
//! four templates, and by nothing else in this tree.**
//!
//! Templates 1, 2 and 3 and custom AT positions have no published bitstream at
//! all. Their *coding* is held by a round trip through
//! [`super::decode_arithmetic`] and their *positions* by the same figure
//! transcription, and that is said plainly because it is the weaker of the two
//! claims.
//!
//! # The shape, and who calls it
//!
//! [`Jbig2GenericSource`] is [`crate::PngSource`]'s shape for the reasons
//! `ccitt/encode.rs`'s header sets out against rulings 8 and 11: plain numbers
//! and borrowed bytes in, bytes out, no facade entry point, because a region
//! is not a document.
//!
//! **Nothing in this repository calls [`generic_encode`] outside
//! `jbig2.rs`'s tests.** The writer's contract is unchanged — it never
//! re-encodes image bytes — and promoting a coder does not change it. What
//! would call this is a writer that builds a `/JBIG2Decode` image XObject from
//! a *raster* rather than from bytes it was handed, and that needs D.3's
//! embedded-stream assembly, which the list above says is not written.

use super::{context, packed_size, template_bits, tpgdon_context, Bitmap, RegionInfo};
use crate::mq::encoder::MqEncoder;

/// A packed bilevel region to be coded, and the 6.2.5 parameters it is to be
/// coded under.
///
/// Borrowed rather than owned, for [`crate::PngSource`]'s reason: the caller
/// already holds the pixels.
#[derive(Clone, Copy, Debug)]
pub struct Jbig2GenericSource<'a> {
    pub width: u32,
    pub height: u32,
    /// 7.4.6.2's GBTEMPLATE, 0 to 3 — Figures 8 to 11.
    pub template: u8,
    /// 7.4.6.2's TPGDON, 6.2.5.7's typical prediction.
    pub tpgdon: bool,
    /// 7.4.6.3's AT pixels. Template 0 uses all four; the others use only the
    /// first, and the remaining three are ignored rather than refused because
    /// 7.4.6.3 does not write them at all.
    pub at: [(i8, i8); 4],
    /// Bytes from the start of one row to the start of the next. At least
    /// `width.div_ceil(8)`; more means the buffer is padded.
    pub stride: usize,
    /// The raster, most significant bit first, **1 for black** — JBIG2's own
    /// sense (6.2.2), which is what [`crate::jbig2_decode`] returns. A caller
    /// holding a one-bit DeviceGray image inverts here rather than having this
    /// module guess which convention it meant.
    pub data: &'a [u8],
}

/// Why a region could not be coded.
///
/// A caller describing its own buffer or its own parameters wrongly, never
/// damage in data read from somewhere — [`crate::PngEncodeError`]'s
/// distinction, for the same reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jbig2EncodeError {
    /// A zero width or height, or a size whose packed length overflows a
    /// `usize` — [`super::packed_size`]'s checked multiply, on the writing
    /// side.
    BadDimensions { width: u32, height: u32 },
    /// A GBTEMPLATE above 3. There are four figures and no fifth.
    BadTemplate(u8),
    /// The row stride does not reach the end of a row, so the rows overlap.
    ShortStride { stride: usize, row_bytes: usize },
    /// Fewer bytes than `stride x (height - 1) + row_bytes`. The last row is
    /// charged at its real width rather than at the stride.
    ShortData { have: usize, need: u64 },
}

impl core::fmt::Display for Jbig2EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::BadDimensions { width, height } => {
                write!(f, "region dimensions {width} x {height}")
            }
            Self::BadTemplate(t) => write!(f, "generic region template {t}"),
            Self::ShortStride { stride, row_bytes } => {
                write!(f, "stride of {stride} bytes for a row of {row_bytes}")
            }
            Self::ShortData { have, need } => {
                write!(f, "{have} bytes of raster, {need} needed")
            }
        }
    }
}

impl std::error::Error for Jbig2EncodeError {}

/// Copies the caller's raster into the [`Bitmap`] the context function reads,
/// after checking every number the copy depends on.
///
/// The bitmap is a second allocation rather than a view over the caller's
/// bytes, and that is the stride's fault: [`Bitmap`] is packed at exactly
/// `width.div_ceil(8)` a row and [`super::context`] indexes it that way, while
/// a caller's buffer may be padded. Reading a padded buffer as though it were
/// packed shears the picture by a few pixels a row, which is the failure that
/// looks like a template bug.
fn raster(source: &Jbig2GenericSource<'_>) -> Result<Bitmap, Jbig2EncodeError> {
    if source.template > 3 {
        return Err(Jbig2EncodeError::BadTemplate(source.template));
    }
    // The checked multiply happens *before* the allocation, exactly as the
    // decoder's does (ruling 1) — here the ceiling is `usize::MAX` because the
    // size is the caller's own buffer rather than an attacker's claim, and what
    // is being caught is the overflow rather than the size.
    let Some(bytes) = packed_size(source.width, source.height, usize::MAX) else {
        return Err(Jbig2EncodeError::BadDimensions {
            width: source.width,
            height: source.height,
        });
    };
    let stride = (source.width as usize).div_ceil(8);
    if source.stride < stride {
        return Err(Jbig2EncodeError::ShortStride {
            stride: source.stride,
            row_bytes: stride,
        });
    }
    // In `u64` and before anything is converted, for `png_encode`'s reason: on
    // a 32-bit target the product overflows a `usize` while still naming a
    // legal region, and an overflowed length is a slice inside the buffer
    // rather than a refusal. The last row is charged at `stride` and not at
    // the caller's stride, because a buffer ending at the final pixel has
    // given us every pixel.
    let need = (source.stride as u64)
        .saturating_mul(u64::from(source.height) - 1)
        .saturating_add(stride as u64);
    if (source.data.len() as u64) < need {
        return Err(Jbig2EncodeError::ShortData {
            have: source.data.len(),
            need,
        });
    }

    let mut bitmap = Bitmap {
        bits: vec![0u8; bytes],
        width: source.width,
        height: source.height,
        stride,
    };
    for y in 0..source.height as usize {
        let from = y * source.stride;
        let to = y * stride;
        // Both slices are bounded by the checks above; `get`/`get_mut`
        // regardless (ruling 1).
        if let (Some(src), Some(dst)) = (
            source.data.get(from..from + stride),
            bitmap.bits.get_mut(to..to + stride),
        ) {
            dst.copy_from_slice(src);
        }
    }
    // The padding bits past `width` in each row's last byte are the caller's,
    // and 6.2.5.2 says every position outside the region reads 0. Clearing
    // them here means `Bitmap::get` never has to, and means two rasters that
    // differ only in their padding code to the same bytes.
    if source.width % 8 != 0 {
        let mask = 0xFFu8 << (8 - source.width % 8);
        for y in 0..source.height as usize {
            if let Some(byte) = bitmap.bits.get_mut(y * stride + stride - 1) {
                *byte &= mask;
            }
        }
    }
    Ok(bitmap)
}

/// Codes a bilevel region as T.88 6.2.5.7's arithmetic generic region: the MQ
/// bytes alone, with none of 7.4.6's header in front of them.
///
/// This is the half a caller wants when it is assembling a segment itself, and
/// the half T.88 Annex H.1 segment 11 publishes nine bytes of.
///
/// # Errors
/// Any [`Jbig2EncodeError`] — a raster or a template that does not describe a
/// region.
pub fn generic_encode(source: &Jbig2GenericSource<'_>) -> Result<Vec<u8>, Jbig2EncodeError> {
    let bitmap = raster(source)?;
    let at = source.at.map(|(x, y)| (i32::from(x), i32::from(y)));
    let mut encoder = MqEncoder::new(1 << template_bits(source.template));
    // 6.2.5.7's LTP, which starts at 0 and is *not* reset per row: SLTP codes
    // the change rather than the state, so a decoder that missed one row would
    // have every later row inverted. That is what makes the toggle below the
    // decision the standard fixes and the row's identity merely the input to
    // it.
    let mut ltp = 0u8;

    for y in 0..bitmap.height {
        if source.tpgdon {
            let typical = y > 0 && rows_identical(&bitmap, y - 1, y);
            let sltp = u8::from(typical != (ltp == 1));
            encoder.encode_at(tpgdon_context(source.template), sltp);
            ltp ^= sltp;
            if ltp == 1 {
                // The decoder copies the row above and asks the coder for
                // nothing, so the encoder must offer it nothing.
                continue;
            }
        }
        for x in 0..bitmap.width {
            // **Deliberately the decoder's own context function.** The two
            // directions must form the identical index from the identical
            // neighbours or nothing decodes, and a second transcription of
            // four figures is a second thing to get wrong — `png/encode.rs`
            // records the same trade for the Paeth predictor. What sharing
            // costs is that a defect in `context` cancels between the two,
            // which is precisely why the guard that matters is T.88 Annex
            // H.1's published bytes rather than a round trip.
            let cx = context(&bitmap, source.template, &at, x as i32, y as i32);
            encoder.encode_at(cx, bitmap.get(x as i32, y as i32) as u8);
        }
    }
    Ok(encoder.flush())
}

/// Whether two rows hold the same pixels — 6.2.5.7's LTP condition.
///
/// Compared over the packed bytes rather than pixel by pixel, which is exact
/// because [`raster`] has already cleared the padding bits past `width`.
fn rows_identical(bitmap: &Bitmap, a: u32, b: u32) -> bool {
    let stride = bitmap.stride;
    let (Some(x), Some(y)) = (
        bitmap
            .bits
            .get(a as usize * stride..(a as usize + 1) * stride),
        bitmap
            .bits
            .get(b as usize * stride..(b as usize + 1) * stride),
    ) else {
        return false;
    };
    x == y
}

/// The **data** of one immediate generic region segment (T.88 7.4.6): 7.4.1's
/// seventeen-byte region information field, 7.4.6.2's flags, 7.4.6.3's AT
/// pixels, and the coded data.
///
/// What 7.2's segment header wraps, and nothing more — no segment number, no
/// referred-to segments, no page association, no page information segment and
/// no file header. Those belong to whoever is assembling a stream, because the
/// numbering of segments is a property of the stream and not of a region.
///
/// `x`, `y` and `op` are 7.4.1's placement: where the region lands on the page
/// and which of 7.4.1.5's five combination operators composites it. `op` is
/// masked to three bits, which is the field's width.
///
/// # Errors
/// Any [`Jbig2EncodeError`], from [`generic_encode`].
pub fn generic_region_segment(
    source: &Jbig2GenericSource<'_>,
    x: u32,
    y: u32,
    op: u8,
) -> Result<Vec<u8>, Jbig2EncodeError> {
    let coded = generic_encode(source)?;
    let info = RegionInfo {
        width: source.width,
        height: source.height,
        x,
        y,
        op: op & 0x07,
    };
    let mut out = Vec::with_capacity(coded.len() + 26);
    // 7.4.1, big-endian throughout — which is the one thing about this header
    // that is invisible when it is wrong on a square region, because a decoder
    // reading a byte-swapped 54 gets 905 969 664 and refuses, but a decoder
    // reading a byte-swapped width *equal to* a byte-swapped height decodes a
    // transposed picture and says nothing.
    out.extend_from_slice(&info.width.to_be_bytes());
    out.extend_from_slice(&info.height.to_be_bytes());
    out.extend_from_slice(&info.x.to_be_bytes());
    out.extend_from_slice(&info.y.to_be_bytes());
    out.push(info.op);
    // 7.4.6.2: bit 0 MMR (never set here — see the module header), bits 1-2
    // the template, bit 3 TPGDON.
    out.push((u8::from(source.tpgdon) << 3) | (source.template << 1));
    // 7.4.6.3: four pairs at template 0, one at the others. Writing four for
    // every template would put the coded data eight bytes late and decode as
    // noise rather than as a slightly wrong picture.
    let pairs = if source.template == 0 { 4 } else { 1 };
    for (dx, dy) in source.at.iter().take(pairs) {
        out.push(*dx as u8);
        out.push(*dy as u8);
    }
    out.extend_from_slice(&coded);
    Ok(out)
}
