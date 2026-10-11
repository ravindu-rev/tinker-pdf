//! GIF — CompuServe's Graphics Interchange Format, 87a and 89a, as a
//! *container* decoder.
//!
//! No PDF stream is ever a GIF file, so this is not a `/Filter`. It is here
//! for `bmp.rs`'s reason: the tier-4 archive row names GIF as a format a comic
//! page arrived in and became a placeholder for, and EPUB 3.3 §3.2 makes it a
//! core media type an `<img>` may name.
//!
//! Written from the GIF89a specification (CompuServe, 31 July 1990), which is
//! a superset of 87a; section numbers below are its own.
//!
//! # Why `lzw.rs` is not reused
//!
//! PDF's `/LZWDecode` and TIFF's LZW are one coding: 256 roots, codes packed
//! most significant bit first, width switched one code early. GIF's differs in
//! all three places that matter (Appendix F): the number of roots is
//! `2^code size` from the image data's own first byte, anything from 2 to 256;
//! codes are packed **least** significant bit first into 255-byte sub-blocks;
//! and the width grows when the table reaches `2^width` rather than one before
//! it. `tiff.rs`'s old-style transcoder repacks LSB-first codes into
//! `lzw.rs`'s form, but it assumes 256 roots and TIFF's own width rule, so it
//! would mis-size every code of a GIF whose table is not 256 entries. The
//! dictionary below is thirty lines and exists once, for GIF.
//!
//! # The decisions a GIF forces, each taken once
//!
//! - **The first frame is the picture.** A GIF may hold any number of images
//!   and a Netscape looping extension that animates them; a page is one
//!   picture and the first image is the one every viewer shows before any
//!   timer runs. Later images are not decoded, and
//!   [`Warning::GifFramesIgnored`] says there were some — `tiff.rs`'s
//!   `TiffExtraPagesIgnored` for the same reason.
//! - **Interlaced rows are put back in order** (§20.c's four passes, every
//!   8th row from 0, every 8th from 4, every 4th from 2, every 2nd from 1).
//! - **The canvas is the logical screen** (§18), and a first image smaller
//!   than it leaves pixels uncovered. §18 says what they are: "the Background
//!   Color is the color used for those pixels on the screen that are not
//!   covered by an image", an index into the global table. When the image
//!   carries a local table the two palettes cannot share one index space, so
//!   that picture is expanded to RGBA; and with no global table the
//!   background index "is meaningless" (§18 again), so uncovered pixels are
//!   transparent. Part of an image outside the screen is clipped, with
//!   [`Warning::GifFrameOutsideScreen`].
//! - **A transparent index stays an index.** The graphic control extension's
//!   transparency flag (§23) names one index every pixel carrying it is fully
//!   transparent at, which [`ImagePixels::Indexed`]'s `transparent` carries
//!   and PDF's colour-key `/Mask` expresses exactly — no second image needed.
//! - **An index past the active table is black**, with
//!   [`Warning::GifPaletteIndexOutOfRange`]; the table is padded to 256
//!   entries so the index addresses something (`png.rs`'s bargain).

use crate::raster::ImagePixels;
use crate::{Limits, Warning, Warnings};

// --- the budget ---------------------------------------------------------

/// Samples in **either** of the two buffers a decode makes, each charged on
/// its own before it exists: the canvas, `width x height x` one for an
/// indexed picture or four for one that had to be expanded; and the first
/// image's own indices, `width x height` bytes from its descriptor, which the
/// LZW stage writes before any of them is placed on the canvas.
///
/// | | Samples |
/// | --- | --- |
/// | The most any fixture in this crate spends | 960 — Pillow's interlaced 40 x 24, indexed |
/// | A comic page: 2000 x 3000, expanded | 24 000 000 |
/// | **This cap** | **67 108 864** |
///
/// `MAX_PNG_SAMPLES`'s `1 << 26`, for its arithmetic. GIF's fields are 16-bit,
/// and three of them reach past it from a few bytes. A logical screen asks for
/// up to 65 535 x 65 535 x 4 from thirteen. A screen left at zero makes the
/// canvas the image's own extent, `left + width` by `top + height`, which is
/// up to 131 070 a side and about 6.9 x 10^10 samples expanded. And the image
/// descriptor's size is not bounded by the screen at all: a one-pixel screen
/// over a 65 535 x 4 096 image is 268 million indices from thirty-five bytes,
/// which is what this cap missed until the descriptor was charged too.
///
/// **Reachable**: `an_image_past_the_sample_cap_is_refused_before_it_allocates`
/// builds the thirteen bytes, `a_zero_screen_is_charged_at_the_image_extent`
/// the zero screen, and
/// `an_image_descriptor_past_the_cap_is_refused_before_it_allocates` the
/// thirty-five.
pub const MAX_GIF_SAMPLES: u64 = 1 << 26;

/// §20's "the largest code a table may hold is 4095": twelve bits.
const MAX_CODES: usize = 4096;

// --- the refusals -------------------------------------------------------

/// Why a GIF was refused outright.
///
/// Damage that costs *pixels* — image data that stops early, a code the table
/// does not hold yet — is a [`Warning`] and leaves a partial raster (ruling 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GifError {
    /// The first six bytes are neither `GIF87a` nor `GIF89a`.
    NotGif,
    /// The logical screen descriptor or a colour table is cut off.
    TruncatedHeader,
    /// The file ends, or reaches its trailer, before any image descriptor:
    /// there is no picture, as distinct from a damaged one.
    NoImage,
    /// A zero image width or height, on the first image or on a logical screen
    /// it cannot stand in for.
    BadDimensions { width: u16, height: u16 },
    /// Neither a local nor a global colour table. §18 lets a decoder supply a
    /// system default; a guessed palette is a guessed picture, so this one
    /// does not.
    NoColourTable,
    /// An LZW minimum code size outside 1 to 8. The roots are the indices, and
    /// an index is a byte.
    BadCodeSize(u8),
    /// [`MAX_GIF_SAMPLES`] would be spent by the canvas or by the image's own
    /// indices. Refused before any buffer exists.
    TooManySamples { samples: u64, max: u64 },
    /// The canvas, or the image's own indices, would be larger than the
    /// caller's own [`Limits::max_output`].
    ExceedsOutputLimit { bytes: u64, limit: usize },
}

impl core::fmt::Display for GifError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotGif => f.write_str("not a GIF: no GIF87a or GIF89a signature"),
            Self::TruncatedHeader => f.write_str("the GIF screen descriptor or a table is cut off"),
            Self::NoImage => f.write_str("a GIF with no image in it"),
            Self::BadDimensions { width, height } => {
                write!(f, "GIF dimensions {width} x {height}")
            }
            Self::NoColourTable => f.write_str("a GIF image with no colour table"),
            Self::BadCodeSize(size) => write!(f, "GIF LZW minimum code size {size}"),
            Self::TooManySamples { samples, max } => {
                write!(f, "{samples} samples, ceiling is {max}")
            }
            Self::ExceedsOutputLimit { bytes, limit } => {
                write!(f, "{bytes} bytes of raster, caller's ceiling is {limit}")
            }
        }
    }
}

impl std::error::Error for GifError {}

// --- what comes out -----------------------------------------------------

/// The first image of a GIF, on its logical screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GifImage {
    pub width: u32,
    pub height: u32,
    /// Indexed with the transparent index the graphic control extension named,
    /// or RGBA when a local table and an uncovered background could not share
    /// one index space.
    pub pixels: ImagePixels,
    /// False when the image data ended before the image did.
    pub complete: bool,
    /// Typed leniency records (ruling 10), deduplicated.
    pub warnings: Vec<Warning>,
}

/// The first image descriptor and everything it needs.
struct Frame<'a> {
    left: usize,
    top: usize,
    width: usize,
    height: usize,
    interlaced: bool,
    /// RGB triples; `None` when the image uses the global table.
    local: Option<&'a [u8]>,
    code_size: u8,
    /// The concatenated sub-blocks.
    data: Vec<u8>,
    /// Whether the sub-block chain ended with its terminator.
    whole: bool,
}

/// A cursor over the file.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        let lo = self.u8()?;
        let hi = self.u8()?;
        Some(u16::from_le_bytes([lo, hi]))
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }

    /// Skips a chain of data sub-blocks (§15). Returns false when it ran off
    /// the end of the file.
    fn skip_sub_blocks(&mut self) -> bool {
        loop {
            let Some(len) = self.u8() else {
                return false;
            };
            if len == 0 {
                return true;
            }
            if self.take(usize::from(len)).is_none() {
                self.at = self.bytes.len();
                return false;
            }
        }
    }

    /// Concatenates a chain of sub-blocks, and says whether it was whole.
    fn sub_blocks(&mut self) -> (Vec<u8>, bool) {
        let mut out = Vec::new();
        loop {
            let Some(len) = self.u8() else {
                return (out, false);
            };
            if len == 0 {
                return (out, true);
            }
            match self.take(usize::from(len)) {
                Some(block) => out.extend_from_slice(block),
                None => {
                    out.extend_from_slice(self.bytes.get(self.at..).unwrap_or(&[]));
                    self.at = self.bytes.len();
                    return (out, false);
                }
            }
        }
    }
}

/// [`MAX_GIF_SAMPLES`] and the caller's ceiling, charged on one buffer about
/// to be made.
fn charge(samples: u64, limits: &Limits) -> Result<(), GifError> {
    if samples > MAX_GIF_SAMPLES {
        return Err(GifError::TooManySamples {
            samples,
            max: MAX_GIF_SAMPLES,
        });
    }
    if samples > limits.max_output as u64 {
        return Err(GifError::ExceedsOutputLimit {
            bytes: samples,
            limit: limits.max_output,
        });
    }
    Ok(())
}

/// A colour table of `2^(size + 1)` RGB triples (§18, §20).
fn table<'a>(r: &mut Reader<'a>, packed: u8) -> Option<&'a [u8]> {
    let entries = 2usize << (packed & 0x07);
    r.take(entries * 3)
}

// --- the entry point ----------------------------------------------------

/// Header, logical screen, the first image, and the pixels.
///
/// # Errors
/// Any [`GifError`].
pub fn gif_decode(bytes: &[u8], limits: &Limits) -> Result<GifImage, GifError> {
    if !(bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Err(GifError::NotGif);
    }
    let mut r = Reader { bytes, at: 6 };
    let mut w = Warnings::default();

    // §18, the logical screen descriptor.
    let screen_w = r.u16().ok_or(GifError::TruncatedHeader)?;
    let screen_h = r.u16().ok_or(GifError::TruncatedHeader)?;
    let packed = r.u8().ok_or(GifError::TruncatedHeader)?;
    let background = r.u8().ok_or(GifError::TruncatedHeader)?;
    let _aspect = r.u8().ok_or(GifError::TruncatedHeader)?;
    let global = if packed & 0x80 != 0 {
        Some(table(&mut r, packed).ok_or(GifError::TruncatedHeader)?)
    } else {
        None
    };

    // Blocks until the first image descriptor. A graphic control extension
    // applies to the next image only (§23), so only the last one before it
    // counts.
    let mut transparent: Option<u8> = None;
    let frame = loop {
        match r.u8() {
            // §24, extension introducer.
            Some(0x21) => {
                let label = r.u8().ok_or(GifError::NoImage)?;
                if label == 0xF9 {
                    // §23: a four-byte block — packed fields, delay, index.
                    let (data, _) = r.sub_blocks();
                    if let (Some(&flags), Some(&index)) = (data.first(), data.get(3)) {
                        transparent = (flags & 1 != 0).then_some(index);
                    }
                } else if !r.skip_sub_blocks() {
                    return Err(GifError::NoImage);
                }
            }
            // §20, image separator.
            Some(0x2C) => break read_frame(&mut r)?,
            // §27, the trailer, anything else, or the end: no image came.
            _ => return Err(GifError::NoImage),
        }
    };

    // Is there another image after this one? Walked rather than decoded, and
    // only to say that the file is an animation this build draws the first
    // frame of.
    if another_image_follows(&mut r) {
        w.push(Warning::GifFramesIgnored);
    }

    // The canvas: §18's logical screen, or — for a writer that left it at
    // zero — the first image's own extent.
    let (canvas_w, canvas_h) = if screen_w == 0 || screen_h == 0 {
        (frame.left + frame.width, frame.top + frame.height)
    } else {
        (usize::from(screen_w), usize::from(screen_h))
    };
    if frame.width == 0 || frame.height == 0 || canvas_w == 0 || canvas_h == 0 {
        return Err(GifError::BadDimensions {
            width: frame.width as u16,
            height: frame.height as u16,
        });
    }
    if frame.left + frame.width > canvas_w || frame.top + frame.height > canvas_h {
        w.push(Warning::GifFrameOutsideScreen);
    }
    let covers =
        frame.left == 0 && frame.top == 0 && frame.width >= canvas_w && frame.height >= canvas_h;

    let active = frame.local.or(global).ok_or(GifError::NoColourTable)?;
    // A local table with pixels it does not cover: two index spaces, one
    // canvas, so the picture has to be expanded.
    let expand = frame.local.is_some() && !covers;
    let components: u64 = if expand { 4 } else { 1 };
    // Two buffers, and each is charged before either exists: the canvas at
    // the components it comes back in, and the image's own indices — one
    // byte each, which is what the LZW stage writes before any is placed,
    // sized by the descriptor whatever the screen is.
    charge(
        (canvas_w as u64)
            .saturating_mul(canvas_h as u64)
            .saturating_mul(components),
        limits,
    )?;
    charge(
        (frame.width as u64).saturating_mul(frame.height as u64),
        limits,
    )?;
    if !(1..=8).contains(&frame.code_size) {
        return Err(GifError::BadCodeSize(frame.code_size));
    }

    // The image's own indices, in row order.
    let total = frame.width * frame.height;
    let mut decoded = vec![0u8; total];
    let produced = lzw(&frame.data, frame.code_size, &mut decoded, &mut w);
    let complete = produced >= total;
    if !complete {
        w.push(Warning::TruncatedInput);
    }
    if !frame.whole && complete {
        // Every pixel arrived and the sub-block chain did not close: the file
        // ends where its image did, which a truncated trailer looks like.
        w.push(Warning::EarlyEod);
    }
    let rows = row_order(frame.height, frame.interlaced);

    let entries = active.len() / 3;
    if decoded.iter().any(|&i| usize::from(i) >= entries) {
        w.push(Warning::GifPaletteIndexOutOfRange);
    }

    let pixels = if expand {
        // §18: uncovered pixels are the global background colour, or
        // transparent when there is no global table to take it from.
        let back = global.and_then(|g| {
            let at = usize::from(background) * 3;
            g.get(at..at + 3)
        });
        let mut out = vec![0u8; canvas_w * canvas_h * 4];
        if let Some(&[r_, g_, b_]) = back {
            for px in out.chunks_exact_mut(4) {
                px.copy_from_slice(&[r_, g_, b_, 255]);
            }
        }
        for (i, &index) in decoded.iter().enumerate() {
            let x = frame.left + i % frame.width;
            let y = frame.top.saturating_add(rows_at(&rows, i / frame.width));
            if x >= canvas_w || y >= canvas_h {
                continue;
            }
            let at = (y * canvas_w + x) * 4;
            let colour = if transparent == Some(index) {
                [0, 0, 0, 0]
            } else {
                let c = usize::from(index) * 3;
                match active.get(c..c + 3) {
                    Some(&[r_, g_, b_]) => [r_, g_, b_, 255],
                    _ => [0, 0, 0, 255],
                }
            };
            if let Some(slot) = out.get_mut(at..at + 4) {
                slot.copy_from_slice(&colour);
            }
        }
        ImagePixels::Rgba(out)
    } else {
        let mut indices = vec![background; canvas_w * canvas_h];
        for (i, &index) in decoded.iter().enumerate() {
            let x = frame.left + i % frame.width;
            let y = frame.top.saturating_add(rows_at(&rows, i / frame.width));
            if x >= canvas_w || y >= canvas_h {
                continue;
            }
            if let Some(slot) = indices.get_mut(y * canvas_w + x) {
                *slot = index;
            }
        }
        if !covers && usize::from(background) >= entries {
            w.push(Warning::GifPaletteIndexOutOfRange);
        }
        let mut palette = active.to_vec();
        // Padded with black to 256 entries, so every index addresses one.
        palette.resize(256 * 3, 0);
        ImagePixels::Indexed {
            palette,
            indices,
            transparent,
        }
    };

    Ok(GifImage {
        width: canvas_w as u32,
        height: canvas_h as u32,
        pixels,
        complete,
        warnings: w.into_vec(),
    })
}

/// §20: the image descriptor, its local table, and its data.
fn read_frame<'a>(r: &mut Reader<'a>) -> Result<Frame<'a>, GifError> {
    let left = r.u16().ok_or(GifError::NoImage)?;
    let top = r.u16().ok_or(GifError::NoImage)?;
    let width = r.u16().ok_or(GifError::NoImage)?;
    let height = r.u16().ok_or(GifError::NoImage)?;
    let packed = r.u8().ok_or(GifError::NoImage)?;
    let local = if packed & 0x80 != 0 {
        Some(table(r, packed).ok_or(GifError::TruncatedHeader)?)
    } else {
        None
    };
    // §22: the image data opens with the LZW minimum code size.
    let code_size = r.u8().ok_or(GifError::NoImage)?;
    let (data, whole) = r.sub_blocks();
    Ok(Frame {
        left: usize::from(left),
        top: usize::from(top),
        width: usize::from(width),
        height: usize::from(height),
        interlaced: packed & 0x40 != 0,
        local,
        code_size,
        data,
        whole,
    })
}

/// Walks the blocks after the first image, looking for a second one.
fn another_image_follows(r: &mut Reader<'_>) -> bool {
    loop {
        match r.u8() {
            Some(0x21) => {
                if r.u8().is_none() || !r.skip_sub_blocks() {
                    return false;
                }
            }
            Some(0x2C) => return true,
            _ => return false,
        }
    }
}

/// Which raster row the `n`th stored row is, for §20.c's four passes.
fn row_order(height: usize, interlaced: bool) -> Vec<usize> {
    if !interlaced {
        return (0..height).collect();
    }
    let mut rows = Vec::with_capacity(height);
    for (start, step) in [(0usize, 8usize), (4, 8), (2, 4), (1, 2)] {
        rows.extend((start..height).step_by(step));
    }
    rows
}

fn rows_at(rows: &[usize], stored: usize) -> usize {
    rows.get(stored).copied().unwrap_or(usize::MAX)
}

/// Appendix F's variable-length-code LZW, least significant bit first.
///
/// Writes into `out` and returns how many indices it produced. Stops at the
/// end-of-information code, at the end of the data, at a code the table does
/// not hold, or when `out` is full.
///
/// The four tables are indexed directly, and every index is provably inside
/// them: a code is read at most twelve bits wide, so it is below
/// [`MAX_CODES`]; `next` is only ever stored to while it is below
/// [`MAX_CODES`]; and a string is walked for exactly the `length` its own
/// entry was built with, through prefixes that were themselves codes.
fn lzw(data: &[u8], min: u8, out: &mut [u8], w: &mut Warnings) -> usize {
    let clear = 1usize << min;
    let end = clear + 1;
    let mut prefix = vec![0u16; MAX_CODES];
    let mut suffix = vec![0u8; MAX_CODES];
    let mut first = vec![0u8; MAX_CODES];
    let mut length = vec![0u16; MAX_CODES];
    for code in 0..clear.min(MAX_CODES) {
        // A root is one index; `min` is at most 8, so `code` fits a byte.
        suffix[code] = code as u8;
        first[code] = code as u8;
        length[code] = 1;
    }

    let mut width = u32::from(min) + 1;
    let mut next = clear + 2;
    let mut previous: Option<usize> = None;
    let mut written = 0usize;
    let mut bitpos = 0u64;
    let total_bits = (data.len() as u64) * 8;

    while written < out.len() {
        if bitpos + u64::from(width) > total_bits {
            break;
        }
        let code = read_lsb(data, bitpos, width);
        bitpos += u64::from(width);

        if code == clear {
            width = u32::from(min) + 1;
            next = clear + 2;
            previous = None;
            continue;
        }
        if code == end {
            break;
        }
        match previous {
            None => {
                // The first code after a clear must be a root.
                if code >= clear {
                    w.push(Warning::BadLzwCode);
                    break;
                }
            }
            Some(p) => {
                let head = if code < next {
                    first[code]
                } else if code == next {
                    // Appendix F's KwKwK case: the code being defined now.
                    first[p]
                } else {
                    w.push(Warning::BadLzwCode);
                    break;
                };
                if next < MAX_CODES {
                    prefix[next] = p as u16;
                    suffix[next] = head;
                    first[next] = first[p];
                    length[next] = length[p].saturating_add(1);
                    next += 1;
                }
            }
        }

        // Written back to front, since the table stores a string as its last
        // index and the code of everything before it.
        let len = usize::from(length[code]);
        let mut c = code;
        for k in (0..len).rev() {
            if let Some(slot) = out.get_mut(written + k) {
                *slot = suffix[c];
            }
            c = usize::from(prefix[c]);
        }
        written = written.saturating_add(len);
        previous = Some(code);

        // Appendix F: the width grows when the next code would not fit.
        if next == 1usize << width && width < 12 {
            width += 1;
        }
    }
    written.min(out.len())
}

/// `width` bits at `bitpos`, least significant first — GIF's packing.
fn read_lsb(data: &[u8], bitpos: u64, width: u32) -> usize {
    let byte = (bitpos / 8) as usize;
    let off = (bitpos % 8) as u32;
    let b0 = u32::from(data.get(byte).copied().unwrap_or(0));
    let b1 = u32::from(data.get(byte + 1).copied().unwrap_or(0));
    let b2 = u32::from(data.get(byte + 2).copied().unwrap_or(0));
    // width <= 12 and off <= 7, so a code never spans more than three bytes.
    let chunk = b0 | (b1 << 8) | (b2 << 16);
    ((chunk >> off) & ((1u32 << width) - 1)) as usize
}

#[cfg(test)]
mod tests;
