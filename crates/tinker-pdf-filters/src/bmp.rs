//! BMP — the Windows device-independent bitmap, as a *container* decoder.
//!
//! No PDF stream is ever a BMP file, so this is not a `/Filter` and does not
//! appear in [`crate::Filter`]. It is here for `png.rs`'s and `tiff.rs`'s
//! reason: a comic archive hands out image files rather than PDF streams, and
//! the tier-4 archive row names BMP as one of the formats a page arrived in
//! and became a placeholder for.
//!
//! # What was written from
//!
//! There is no single BMP standard. The layouts are Microsoft's Win32
//! documentation of `BITMAPFILEHEADER`, `BITMAPCOREHEADER`,
//! `BITMAPINFOHEADER`, `BITMAPV4HEADER` and `BITMAPV5HEADER`, and of the
//! `biCompression` values `BI_RGB`, `BI_RLE8`, `BI_RLE4`, `BI_BITFIELDS` and
//! `BI_ALPHABITFIELDS`; IBM's OS/2 2.x header is the same first forty bytes
//! under different compression numbers. Everything below cites the structure
//! and field it reads rather than a page number, because there are no pages.
//!
//! # The decisions a BMP forces, each taken once and written down
//!
//! - **Rows are stored bottom-up** unless `biHeight` is negative, and every
//!   row is padded to a four-byte boundary (`biSizeImage`'s own formula).
//!   The raster handed back is top row first, like every other decoder here.
//! - **An indexed image stays indexed.** 1-, 2-, 4- and 8-bit pictures come
//!   back as a palette and one index a pixel, never expanded, which is
//!   `cbz.rs`'s whole cost argument applied to the format that is most often
//!   indexed. The palette is padded with black out to `2^biBitCount` entries,
//!   so an index past the table the file supplied addresses black rather than
//!   nothing, and [`Warning::BmpPaletteIndexOutOfRange`] says one was used —
//!   `png.rs`'s ruling-2 bargain exactly.
//! - **The fourth byte of a 32-bit `BI_RGB` pixel is not alpha.** Microsoft's
//!   `BITMAPINFOHEADER` text says of 32 bits a pixel that "the high byte in
//!   each DWORD is not used", and a reader that treats it as opacity makes
//!   every picture from a writer that left garbage there transparent toward
//!   whatever the garbage says. Alpha is read **only** from an alpha mask:
//!   `BI_BITFIELDS` under a V3, V4 or V5 header that carries one, or
//!   `BI_ALPHABITFIELDS`. Some browsers guess otherwise from the pixel values;
//!   a guess is not a reading of the file.
//! - **`BI_BITFIELDS` masks are scaled, not truncated.** A 5-bit field is
//!   mapped to eight bits by `v x 255 / 31` rounded to nearest — the
//!   definition, rather than bit replication, which agrees with it for some
//!   widths and not others ([`crate::raster`]). `BI_RGB` at 16 bits is the
//!   5-5-5 masks the documentation names as its default.
//! - **An RLE "delta" or early end-of-line leaves pixels undefined**, and the
//!   documentation says nothing about what they are. They are index 0 here,
//!   with [`Warning::BmpRleUndefinedPixels`], because a colour chosen quietly
//!   is a colour a reader cannot tell from one the file stated. A run that
//!   reaches past the end of its row is clipped there rather than wrapped,
//!   with [`Warning::BmpRleOverrun`].
//!
//! # What is refused, by name
//!
//! [`BmpError`] carries each. `BI_JPEG` and `BI_PNG` are a whole JPEG or PNG
//! file inside the bitmap, which the documentation restricts to printer device
//! contexts; OS/2's Huffman 1D and RLE24 are the two codings OS/2 2.x numbered
//! 3 and 4; `BI_CMYK` and its two RLE forms are Windows Metafile print-spooler
//! formats; and a 64-bit pixel is a scRGB fixed-point number nothing here maps
//! to a display value. Every one of those is refused rather than guessed at,
//! for `png.rs`'s reason: a decoder that answers a header it does not
//! understand with a plausible raster produces a page indistinguishable from a
//! correct one.

use crate::raster::{scale_to_8, ImagePixels};
use crate::{Limits, Warning, Warnings};

// --- the budget ---------------------------------------------------------

/// Samples in the **output** raster — `width x height x` one for an indexed
/// image, three or four for a direct one — checked with a saturating multiply
/// before any buffer exists.
///
/// | | Samples |
/// | --- | --- |
/// | The most any fixture in this crate spends | 32 768 — bmpsuite's 127 x 64 at four components, rounded up |
/// | A comic page: 2000 x 3000 with an alpha mask | 24 000 000 |
/// | **This cap** | **67 108 864** |
///
/// The same `1 << 26` as [`crate::MAX_PNG_SAMPLES`] and
/// [`crate::MAX_TIFF_SAMPLES`], for the same arithmetic: it is the ceiling a
/// decoded comic page already lives under whichever container it came in, and
/// a BMP has no pass-through route that would let it spend less.
///
/// **Reachable**: `biWidth` and `biHeight` are signed 32-bit fields, so a
/// fifty-four byte header can ask for 2^62 samples, and
/// `an_image_past_the_sample_cap_is_refused_before_it_allocates` builds one.
pub const MAX_BMP_SAMPLES: u64 = 1 << 26;

// --- the header ---------------------------------------------------------

/// `BITMAPFILEHEADER` is fourteen bytes and the info header starts there.
const FILE_HEADER: usize = 14;

/// `biSize` values Windows defines: `BITMAPINFOHEADER`, the two undocumented
/// V2 and V3 extensions that add the colour masks inside the header, and
/// `BITMAPV4HEADER` and `BITMAPV5HEADER`.
const WINDOWS_HEADERS: [u32; 5] = [40, 52, 56, 108, 124];

/// `BITMAPCOREHEADER`, OS/2 1.x's twelve bytes with 16-bit dimensions.
const CORE_HEADER: u32 = 12;

/// OS/2 2.x's header is 64 bytes, and writers truncated it to any length from
/// 16 that keeps whole fields.
const OS2_V2_MIN: u32 = 16;
const OS2_V2_MAX: u32 = 64;

const BI_RGB: u32 = 0;
const BI_RLE8: u32 = 1;
const BI_RLE4: u32 = 2;
const BI_BITFIELDS: u32 = 3;
const BI_ALPHABITFIELDS: u32 = 6;

// --- the refusals -------------------------------------------------------

/// Why a bitmap was refused outright.
///
/// Damage that costs *pixels* — a truncated pixel array, an RLE stream that
/// stops early — is a [`Warning`] and leaves a partial raster (ruling 2);
/// these are the conditions that would make a raster mean something else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmpError {
    /// The file does not begin `BM`, or is too short to hold the fourteen-byte
    /// file header and an info header's size field.
    NotBmp,
    /// The info header is cut off before its own declared end.
    TruncatedHeader,
    /// A `biSize` that is none of the header layouts above.
    UnsupportedHeader(u32),
    /// A width that is not positive, or a height of zero, or either past what
    /// 32 bits can say.
    BadDimensions { width: i64, height: i64 },
    /// A `biBitCount` outside {1, 2, 4, 8, 16, 24, 32}, or one its compression
    /// cannot carry (`BI_RLE8` at anything but 8, `BI_RLE4` at anything but 4,
    /// bit fields at an indexed depth). 64 is scRGB and is here too.
    UnsupportedBitDepth(u16),
    /// A `biCompression` this build does not decode, by its own number:
    /// `BI_JPEG` 4, `BI_PNG` 5, the `BI_CMYK` family 11 to 13, and OS/2 2.x's
    /// Huffman 1D and RLE24, which it numbers 3 and 4.
    UnsupportedCompression(u32),
    /// A colour mask that is not one contiguous run of bits, or three colour
    /// masks that are all zero. Shifting a split mask produces numbers, and
    /// the numbers are not colours.
    BadBitfields,
    /// An indexed image with no colour table at all: every index addresses
    /// nothing, so there is no picture — as distinct from a damaged one.
    NoPalette,
    /// `bfOffBits` points inside the headers or past the end of the file.
    BadDataOffset(u32),
    /// [`MAX_BMP_SAMPLES`] would be spent. Refused before any buffer exists.
    TooManySamples { samples: u64, max: u64 },
    /// The raster would be larger than the caller's own
    /// [`Limits::max_output`].
    ExceedsOutputLimit { bytes: u64, limit: usize },
}

impl core::fmt::Display for BmpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotBmp => f.write_str("not a BMP: no BM file header"),
            Self::TruncatedHeader => f.write_str("the BMP info header is cut off"),
            Self::UnsupportedHeader(size) => write!(f, "BMP info header of {size} bytes"),
            Self::BadDimensions { width, height } => {
                write!(f, "BMP dimensions {width} x {height}")
            }
            Self::UnsupportedBitDepth(bits) => write!(f, "BMP biBitCount {bits}"),
            Self::UnsupportedCompression(c) => write!(f, "BMP biCompression {c}"),
            Self::BadBitfields => f.write_str("BMP colour masks that are not colour masks"),
            Self::NoPalette => f.write_str("an indexed BMP with no colour table"),
            Self::BadDataOffset(at) => write!(f, "BMP pixel data offset {at}"),
            Self::TooManySamples { samples, max } => {
                write!(f, "{samples} samples, ceiling is {max}")
            }
            Self::ExceedsOutputLimit { bytes, limit } => {
                write!(f, "{bytes} bytes of raster, caller's ceiling is {limit}")
            }
        }
    }
}

impl std::error::Error for BmpError {}

// --- what comes out -----------------------------------------------------

/// One decoded bitmap.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BmpImage {
    pub width: u32,
    pub height: u32,
    /// Indexed for 1, 2, 4 and 8 bits a pixel; RGB or RGBA for the rest. A
    /// BMP has no transparent index, so an indexed one's is always `None`.
    pub pixels: ImagePixels,
    /// False when the pixel array or the RLE stream ended before the image
    /// did — [`crate::Decoded::complete`]'s contract.
    pub complete: bool,
    /// Typed leniency records (ruling 10), deduplicated.
    pub warnings: Vec<Warning>,
}

/// How the pixel array is coded, once the header has been read.
#[derive(Clone, Copy)]
enum Coding {
    Rgb,
    Rle8,
    Rle4,
    /// Red, green, blue and alpha masks, alpha zero for none.
    Bitfields([u32; 4]),
}

/// Everything the header said.
struct Header {
    width: usize,
    height: usize,
    bottom_up: bool,
    bits: u16,
    coding: Coding,
    /// Where the colour table starts and how wide one entry is.
    table_at: usize,
    entry: usize,
    /// `biClrUsed`, zero when the header has none.
    colours_used: u32,
    data_at: usize,
}

// --- the entry point ----------------------------------------------------

/// File header, info header, colour table, and the pixels.
///
/// # Errors
/// Any [`BmpError`].
pub fn bmp_decode(bytes: &[u8], limits: &Limits) -> Result<BmpImage, BmpError> {
    let header = read_header(bytes)?;
    let mut w = Warnings::default();

    let indexed = header.bits <= 8;
    let alpha = matches!(header.coding, Coding::Bitfields(masks) if masks[3] != 0);
    let components: u64 = if indexed {
        1
    } else if alpha {
        4
    } else {
        3
    };
    let samples = (header.width as u64)
        .saturating_mul(header.height as u64)
        .saturating_mul(components);
    if samples > MAX_BMP_SAMPLES {
        return Err(BmpError::TooManySamples {
            samples,
            max: MAX_BMP_SAMPLES,
        });
    }
    if samples > limits.max_output as u64 {
        return Err(BmpError::ExceedsOutputLimit {
            bytes: samples,
            limit: limits.max_output,
        });
    }
    // Under `MAX_BMP_SAMPLES`, so this fits a `usize` on every target.
    let pixels_n = header.width * header.height;
    let data = bytes.get(header.data_at..).unwrap_or(&[]);

    let (pixels, complete) = if indexed {
        let palette = read_palette(bytes, &header)?;
        let mut indices = vec![0u8; pixels_n];
        let complete = match header.coding {
            Coding::Rle8 | Coding::Rle4 => rle(data, &header, &mut indices, &mut w),
            _ => packed_indices(data, &header, &mut indices, &mut w),
        };
        let entries = palette.len() / 3;
        if indices.iter().any(|&i| usize::from(i) >= entries) {
            w.push(Warning::BmpPaletteIndexOutOfRange);
        }
        let mut palette = palette;
        // Padded with black to the depth's own table size, so that every index
        // the depth can express addresses an entry.
        let full = 1usize << header.bits;
        palette.resize(full * 3, 0);
        (
            ImagePixels::Indexed {
                palette,
                indices,
                transparent: None,
            },
            complete,
        )
    } else {
        let masks = match header.coding {
            Coding::Bitfields(masks) => masks,
            // `BI_RGB`'s defaults, from the `BITMAPINFOHEADER` text: 5-5-5 at
            // 16 bits and 8-8-8 with an unused byte at 32. Twenty-four bits
            // is three bytes in B, G, R order, which these masks also say.
            _ if header.bits == 16 => [0x7C00, 0x03E0, 0x001F, 0],
            _ => [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0],
        };
        direct(data, &header, masks, alpha, &mut w)
    };

    Ok(BmpImage {
        width: header.width as u32,
        height: header.height as u32,
        pixels,
        complete,
        warnings: w.into_vec(),
    })
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    let s = bytes.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    let s = bytes.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([
        *s.first()?,
        *s.get(1)?,
        *s.get(2)?,
        *s.get(3)?,
    ]))
}

fn read_header(bytes: &[u8]) -> Result<Header, BmpError> {
    if !bytes.starts_with(b"BM") {
        return Err(BmpError::NotBmp);
    }
    let data_offset = u32_at(bytes, 10).ok_or(BmpError::NotBmp)?;
    let size = u32_at(bytes, FILE_HEADER).ok_or(BmpError::NotBmp)?;

    let core = size == CORE_HEADER;
    let windows = WINDOWS_HEADERS.contains(&size);
    let os2 = !windows && (OS2_V2_MIN..=OS2_V2_MAX).contains(&size);
    if !core && !windows && !os2 {
        return Err(BmpError::UnsupportedHeader(size));
    }
    let header_end = FILE_HEADER
        .checked_add(size as usize)
        .ok_or(BmpError::TruncatedHeader)?;
    if bytes.len() < header_end {
        return Err(BmpError::TruncatedHeader);
    }
    let field = |offset: usize| u32_at(bytes, FILE_HEADER + offset);

    let (width, height, bits, compression) = if core {
        // `BITMAPCOREHEADER`: unsigned 16-bit dimensions, so always bottom-up.
        let w = u16_at(bytes, FILE_HEADER + 4).ok_or(BmpError::TruncatedHeader)?;
        let h = u16_at(bytes, FILE_HEADER + 6).ok_or(BmpError::TruncatedHeader)?;
        let bits = u16_at(bytes, FILE_HEADER + 10).ok_or(BmpError::TruncatedHeader)?;
        (i64::from(w), i64::from(h), bits, BI_RGB)
    } else {
        let w = field(4).ok_or(BmpError::TruncatedHeader)? as i32;
        let h = field(8).ok_or(BmpError::TruncatedHeader)? as i32;
        let bits = u16_at(bytes, FILE_HEADER + 14).ok_or(BmpError::TruncatedHeader)?;
        // OS/2 2.x lets a writer stop the header before `ulCompression`, and
        // what it did not write is zero.
        let compression = if size >= 20 {
            field(16).ok_or(BmpError::TruncatedHeader)?
        } else {
            BI_RGB
        };
        (i64::from(w), i64::from(h), bits, compression)
    };

    // `biHeight`: "If biHeight is negative, the bitmap is a top-down DIB".
    if width <= 0 || height == 0 {
        return Err(BmpError::BadDimensions { width, height });
    }
    let bottom_up = height > 0;
    let rows = height.unsigned_abs();
    let (Ok(width_px), Ok(height_px)) = (usize::try_from(width), usize::try_from(rows)) else {
        return Err(BmpError::BadDimensions { width, height });
    };

    // OS/2 2.x numbers two codings of its own where Windows put bit fields
    // and JPEG. Neither is decoded here, and saying so by the number the
    // file used is the whole of what can honestly be said.
    if os2 && (compression == 3 || compression == 4) {
        return Err(BmpError::UnsupportedCompression(compression));
    }

    let mut extra = 0usize;
    let coding = match compression {
        BI_RGB => Coding::Rgb,
        BI_RLE8 => Coding::Rle8,
        BI_RLE4 => Coding::Rle4,
        BI_BITFIELDS | BI_ALPHABITFIELDS => {
            let alpha_too = compression == BI_ALPHABITFIELDS;
            let masks = if size >= 52 {
                // V2 and later carry the masks inside the header itself.
                [
                    field(40).unwrap_or(0),
                    field(44).unwrap_or(0),
                    field(48).unwrap_or(0),
                    if size >= 56 {
                        field(52).unwrap_or(0)
                    } else {
                        0
                    },
                ]
            } else {
                // `BITMAPINFOHEADER` followed by three DWORD masks, or four
                // for `BI_ALPHABITFIELDS`; the colour table starts after them.
                extra = if alpha_too { 16 } else { 12 };
                let at = header_end;
                [
                    u32_at(bytes, at).ok_or(BmpError::TruncatedHeader)?,
                    u32_at(bytes, at + 4).ok_or(BmpError::TruncatedHeader)?,
                    u32_at(bytes, at + 8).ok_or(BmpError::TruncatedHeader)?,
                    if alpha_too {
                        u32_at(bytes, at + 12).ok_or(BmpError::TruncatedHeader)?
                    } else {
                        0
                    },
                ]
            };
            if masks[..3].iter().all(|&m| m == 0) || masks.iter().any(|&m| !contiguous(m)) {
                return Err(BmpError::BadBitfields);
            }
            Coding::Bitfields(masks)
        }
        other => return Err(BmpError::UnsupportedCompression(other)),
    };

    let depth_ok = match coding {
        Coding::Rgb => matches!(bits, 1 | 2 | 4 | 8 | 16 | 24 | 32),
        Coding::Rle8 => bits == 8,
        Coding::Rle4 => bits == 4,
        Coding::Bitfields(_) => matches!(bits, 16 | 24 | 32),
    };
    if !depth_ok {
        return Err(BmpError::UnsupportedBitDepth(bits));
    }

    let table_at = header_end + extra;
    let data_at = data_offset as usize;
    if data_at < header_end || data_at > bytes.len() {
        return Err(BmpError::BadDataOffset(data_offset));
    }

    Ok(Header {
        width: width_px,
        height: height_px,
        bottom_up,
        bits,
        coding,
        table_at,
        entry: if core { 3 } else { 4 },
        colours_used: if core || size < 36 {
            0
        } else {
            field(32).unwrap_or(0)
        },
        data_at,
    })
}

/// One run of set bits, or none at all.
const fn contiguous(mask: u32) -> bool {
    if mask == 0 {
        return true;
    }
    let shifted = mask >> mask.trailing_zeros();
    shifted & shifted.wrapping_add(1) == 0
}

/// The colour table, as RGB triples.
///
/// `biClrUsed` entries, or `2^biBitCount` when it is zero — and never more
/// than the depth can index nor more than lie between the header and
/// `bfOffBits`, which is where the table must end.
fn read_palette(bytes: &[u8], header: &Header) -> Result<Vec<u8>, BmpError> {
    let depth_max = 1usize << header.bits;
    let declared = match header.colours_used {
        0 => depth_max,
        n => (n as usize).min(depth_max),
    };
    let room = header.data_at.saturating_sub(header.table_at) / header.entry;
    let entries = declared.min(room);
    if entries == 0 {
        return Err(BmpError::NoPalette);
    }
    let mut palette = Vec::with_capacity(entries * 3);
    for index in 0..entries {
        let at = header.table_at + index * header.entry;
        // `RGBQUAD` and `RGBTRIPLE` both store blue first.
        let Some(&[blue, green, red]) = bytes.get(at..at + 3) else {
            break;
        };
        palette.extend_from_slice(&[red, green, blue]);
    }
    if palette.is_empty() {
        return Err(BmpError::NoPalette);
    }
    Ok(palette)
}

/// Bytes one stored row occupies: `biSizeImage`'s own formula, rounded up to
/// a whole DWORD.
fn stride(width: usize, bits: u16) -> usize {
    width.saturating_mul(usize::from(bits)).saturating_add(31) / 32 * 4
}

/// The raster row a stored row lands on.
const fn target_row(header: &Header, stored: usize) -> usize {
    if header.bottom_up {
        header.height - 1 - stored
    } else {
        stored
    }
}

/// 1, 2, 4 and 8 bits a pixel, uncompressed. Returns whether every row was
/// present.
fn packed_indices(data: &[u8], header: &Header, out: &mut [u8], w: &mut Warnings) -> bool {
    let stride = stride(header.width, header.bits);
    let bits = usize::from(header.bits);
    let mut complete = true;
    for stored in 0..header.height {
        let start = stored.saturating_mul(stride);
        let row = data.get(start..).unwrap_or(&[]);
        let row = row.get(..stride).unwrap_or(row);
        let y = target_row(header, stored);
        for x in 0..header.width {
            let bit = x * bits;
            let Some(&byte) = row.get(bit / 8) else {
                complete = false;
                break;
            };
            // Most significant bits first, as every packed format here is.
            let shift = 8 - (bit % 8) - bits;
            let index = (byte >> shift) & ((1u16 << bits) - 1) as u8;
            if let Some(slot) = out.get_mut(y * header.width + x) {
                *slot = index;
            }
        }
    }
    if !complete {
        w.push(Warning::TruncatedInput);
    }
    complete
}

/// 16, 24 and 32 bits a pixel, under the masks the header gave or the
/// defaults `BI_RGB` implies.
fn direct(
    data: &[u8],
    header: &Header,
    masks: [u32; 4],
    alpha: bool,
    w: &mut Warnings,
) -> (ImagePixels, bool) {
    let stride = stride(header.width, header.bits);
    let bytes_per_pixel = usize::from(header.bits / 8);
    let components = if alpha { 4 } else { 3 };
    let mut out = vec![0u8; header.width * header.height * components];
    let shifts = masks.map(|m| if m == 0 { 0 } else { m.trailing_zeros() });
    let maxima = [0, 1, 2, 3].map(|i| masks[i].checked_shr(shifts[i]).unwrap_or(0));
    let mut complete = true;

    for stored in 0..header.height {
        let start = stored.saturating_mul(stride);
        let row = data.get(start..).unwrap_or(&[]);
        let y = target_row(header, stored);
        for x in 0..header.width {
            let at = x * bytes_per_pixel;
            let Some(px) = row.get(at..at + bytes_per_pixel) else {
                complete = false;
                break;
            };
            let value = px
                .iter()
                .rev()
                .fold(0u32, |acc, &b| (acc << 8) | u32::from(b));
            let base = (y * header.width + x) * components;
            let Some(slot) = out.get_mut(base..base + components) else {
                continue;
            };
            for (c, sample) in slot.iter_mut().enumerate() {
                *sample = scale_to_8((value & masks[c]) >> shifts[c], maxima[c]);
            }
        }
    }
    if !complete {
        w.push(Warning::TruncatedInput);
    }
    (
        if alpha {
            ImagePixels::Rgba(out)
        } else {
            ImagePixels::Rgb(out)
        },
        complete,
    )
}

/// `BI_RLE8` and `BI_RLE4`: pairs of bytes, a count and a value, with a count
/// of zero introducing one of four escapes.
///
/// Returns whether the stream was read to its end-of-bitmap marker without
/// being cut off.
fn rle(data: &[u8], header: &Header, out: &mut [u8], w: &mut Warnings) -> bool {
    let four = matches!(header.coding, Coding::Rle4);
    // `x` across the row and `y` in *stored* rows, bottom first.
    let (mut x, mut y) = (0usize, 0usize);
    let mut at = 0usize;
    let mut written = 0usize;
    let mut overrun = false;
    let mut ended = false;
    let mut cut = false;

    let mut put = |x: usize, y: usize, value: u8, written: &mut usize, overrun: &mut bool| {
        if x < header.width && y < header.height {
            let row = target_row(header, y);
            if let Some(slot) = out.get_mut(row * header.width + x) {
                *slot = value;
                *written += 1;
            }
        } else {
            *overrun = true;
        }
    };

    while y < header.height {
        let (Some(&count), Some(&value)) = (data.get(at), data.get(at + 1)) else {
            cut = at < data.len();
            break;
        };
        at += 2;
        if count > 0 {
            // An encoded run. RLE4 alternates the high and low nibbles of the
            // one byte, starting with the high.
            for k in 0..usize::from(count) {
                let v = if four {
                    if k % 2 == 0 {
                        value >> 4
                    } else {
                        value & 0x0F
                    }
                } else {
                    value
                };
                put(x, y, v, &mut written, &mut overrun);
                x = x.saturating_add(1);
            }
            continue;
        }
        match value {
            // End of line.
            0 => {
                x = 0;
                y += 1;
            }
            // End of bitmap.
            1 => {
                ended = true;
                break;
            }
            // Delta: the next two bytes move the cursor right and up.
            2 => {
                let (Some(&dx), Some(&dy)) = (data.get(at), data.get(at + 1)) else {
                    cut = true;
                    break;
                };
                at += 2;
                x = x.saturating_add(usize::from(dx));
                y = y.saturating_add(usize::from(dy));
            }
            // Absolute mode: `value` literal pixels, padded to a whole word.
            n => {
                let n = usize::from(n);
                let len = if four { n.div_ceil(2) } else { n };
                for k in 0..n {
                    let Some(&byte) = data.get(at + if four { k / 2 } else { k }) else {
                        cut = true;
                        break;
                    };
                    let v = if four {
                        if k % 2 == 0 {
                            byte >> 4
                        } else {
                            byte & 0x0F
                        }
                    } else {
                        byte
                    };
                    put(x, y, v, &mut written, &mut overrun);
                    x = x.saturating_add(1);
                }
                if cut {
                    break;
                }
                at += len + (len & 1);
            }
        }
    }

    if overrun {
        w.push(Warning::BmpRleOverrun);
    }
    if cut {
        w.push(Warning::TruncatedInput);
    } else if !ended && y < header.height {
        // The stream stopped with rows still to come and no marker saying so.
        w.push(Warning::EarlyEod);
    }
    if written < header.width * header.height {
        w.push(Warning::BmpRleUndefinedPixels);
    }
    !cut && (ended || y >= header.height)
}

#[cfg(test)]
mod tests;
