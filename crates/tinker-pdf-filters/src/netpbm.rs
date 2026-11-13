//! Netpbm — PBM, PGM, PPM and PAM, PFM beside them, and the portable inkmap
//! — as a *container* decoder.
//!
//! No PDF stream is ever a Netpbm file, so this is not a `/Filter` and does
//! not appear in [`crate::Filter`]. It is here for `bmp.rs`'s reason: an
//! archive or a page on its own hands out image files rather than PDF
//! streams, and the formats-in tier names these as ones a page arrives in.
//!
//! # What was written from
//!
//! The Netpbm format descriptions, one a format: `pbm`, `pgm`, `ppm`, `pnm`
//! (the name the first three share), `pam`, and `pfm`, which its description
//! calls inspired by the family and "not, however, an official Netpbm
//! format". The portable inkmap has no description at all, only two
//! producers' published documentation of their output, which ruling 21 lets
//! an implementer read and never the producers' code. `THIRDPARTY.md` names
//! every source and the revision read.
//!
//! | Magic | Format | Raster |
//! | --- | --- | --- |
//! | `P1` | plain PBM | the characters `0` and `1`, `1` black |
//! | `P4` | PBM | bits eight to a byte, most significant first, each row padded to a whole byte, `1` black |
//! | `P2` | plain PGM | grey values in ASCII decimal |
//! | `P5` | PGM | one byte a grey value under a maxval below 256, two most significant first above it |
//! | `P3` | plain PPM | red, green and blue in ASCII decimal |
//! | `P6` | PPM | red, green and blue, a byte or two a sample as PGM's |
//! | `P7` | PAM | a header of lines ending `ENDHDR`; tuples of `DEPTH` samples |
//! | `PF`, `Pf` | PFM | IEEE 754 single floats, three or one a pixel, rows bottom to top |
//!
//! # The decisions the family forces, each taken once
//!
//! - **White space is C's `isspace`**: space, carriage return, line feed,
//!   tab, vertical tab and form feed, which every description names. Before
//!   the white space that delimits a raster, a `#` starts a comment that runs
//!   to the next carriage return or line feed and is ignored, and it ends any
//!   number it touches.
//! - **A raw raster starts after exactly one white-space byte.** That byte
//!   is the delimiter the descriptions name, so `255\n\n` starts its raster
//!   with a line feed, a sample of ten, as the description says it does.
//! - **Samples are scaled, not handed back raw.** A maxval below 256 makes
//!   eight bits by rounding `v x 255 / maxval`, a larger one sixteen by
//!   rounding `v x 65535 / maxval` — [`crate::raster`]'s definition, exact in
//!   integers, which is [`crate::PngImage`]'s and [`crate::TiffImage`]'s
//!   output shape so that a consumer learns one layout. The file's own maxval
//!   is kept beside the samples. A sample above it is the maxval, with
//!   [`Warning::NetpbmSampleAboveMaxval`]: the descriptions make every value
//!   one "from 0 through Maxval".
//! - **PBM's `1` is black and PAM's `BLACKANDWHITE` `1` is white**, as the
//!   PAM description points out. Both come back as eight-bit grey, zero black.
//! - **A PAM is read by its tuple type.** The six the description defines —
//!   `BLACKANDWHITE`, `GRAYSCALE` and `RGB`, and each with `_ALPHA` — and
//!   `CMYK` and `CMYK_ALPHA`, which it does not define and its rule for a
//!   user's own tuple type admits: four planes of ink, cyan, magenta, yellow
//!   and black, zero no ink, which is what ISO 32000-2 8.6.4.4 means by a
//!   `/DeviceCMYK` component and `tiff.rs` by `InkSet` 1, and the alpha plane
//!   after them by the description's `_ALPHA` rule. `BLACKANDWHITE` is
//!   `GRAYSCALE` at a maxval of 1, and is read as grey whatever maxval it
//!   states, because a tuple type "does not affect how the samples or tuples
//!   are encoded". A depth greater than the tuple type uses is read as the
//!   description advises, its higher planes ignored, with
//!   [`Warning::NetpbmPlanesIgnored`]; a smaller one is refused. A tuple type
//!   this module does not know, the null one a header with no `TUPLTYPE` line
//!   has among them, is refused by name: the description says that a process
//!   using the format "must further define the format to specify the meanings
//!   of the data", and a depth is not a meaning.
//! - **PFM's floats are mapped as `tiff.rs` maps a floating-point sample**:
//!   `[0, 1]` onto sixteen bits, rounded to nearest, clamped outside it, a NaN
//!   black. Its scale factor's sign is the byte order and its magnitude the
//!   samples' unit, which the description leaves to "separately understood
//!   unit information", so it is handed back in [`NetpbmImage::scale`] and
//!   not applied. Its rows run bottom to top, and come back top first.
//! - **One image a file.** A raw file may hold several back to back; the
//!   first is the picture, and [`Warning::NetpbmImagesIgnored`] says more
//!   follow. Any other byte after a raw raster is
//!   [`Warning::TrailingGarbage`]. After a plain raster the descriptions allow
//!   "any junk you want ... if it starts with a white space character", and
//!   only junk that does not is garbage.
//! - **Damage costs pixels.** A raster that ends early leaves what it held,
//!   the rest read as the sample value zero — white in a PBM, black, no ink
//!   or transparent elsewhere — with [`Warning::TruncatedInput`] and
//!   `complete` false; a plain raster that meets a byte that is neither white
//!   space nor a sample stops there, with [`Warning::NetpbmRasterJunk`]. A
//!   `#` in a plain raster is the one such byte that does not stop it: the
//!   descriptions put comments before the raster only, and a comment cannot
//!   be a sample, so it is read as one before the raster is and skipped, with
//!   the same warning. A PAM header line this module does not read is passed
//!   over with [`Warning::NetpbmHeaderLineIgnored`]. Everything that would
//!   change what the samples *mean* is a [`NetpbmError`] instead (ruling 2's
//!   line, drawn where `png.rs` draws it).
//!
//! # The portable inkmap
//!
//! PKM is the one-bit CMYK member of the family by name and by nothing else:
//! no description defines it, and its two producers document two different
//! files. A PostScript interpreter's user manual lists it as "CMYK
//! internally, RGB output", in an ASCII form and a raw one — a plain or a raw
//! PPM, `P3` or `P6`, whose samples are the RGB the producer's inks became. A
//! PDF library's programming guide documents a bitmap of one bit a component
//! of CMYK saved "as a PAM image file". Neither file says it is an inkmap and
//! neither needs to: the first decodes as the PPM it is, to the RGB it holds,
//! and the second as a PAM, read here where its tuple type is `CMYK`, its
//! samples at a maxval of 1 scaled to 0 and 255 like any other.

use crate::raster::{scale_to_16, scale_to_8};
use crate::{Limits, Warning, Warnings};

// --- the budget ---------------------------------------------------------

/// Samples in the **output** raster — `width x height x` its components —
/// checked with a saturating multiply before any buffer exists.
///
/// | | Samples |
/// | --- | --- |
/// | The most any fixture in this crate spends | 1 560 — the 13 x 24 CMYK-and-alpha PAM, rounded up |
/// | A comic page: 2000 x 3000 in CMYK with alpha | 30 000 000 |
/// | **This cap** | **67 108 864** |
///
/// The same `1 << 26` as [`crate::MAX_BMP_SAMPLES`] and
/// [`crate::MAX_PNG_SAMPLES`], for the same arithmetic: it is the ceiling a
/// decoded page already lives under whichever container it came in.
///
/// **Reachable**: a width and a height are decimal numbers of any length, read
/// up to `u32::MAX` each, so a dozen bytes of header can ask for 2^64 pixels
/// at five components, and
/// `an_image_past_the_sample_cap_is_refused_before_it_allocates` builds one.
pub const MAX_NETPBM_SAMPLES: u64 = 1 << 26;

/// The longest tuple type kept: longer than every one this module reads, so a
/// header of a million `TUPLTYPE` lines costs its reading and no more memory.
const MAX_TUPLE_TYPE: usize = 64;

// --- what comes out -----------------------------------------------------

/// Which member of the family a file is, by its magic number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetpbmFormat {
    /// `P1`.
    PlainPbm,
    /// `P4`.
    Pbm,
    /// `P2`.
    PlainPgm,
    /// `P5`.
    Pgm,
    /// `P3`.
    PlainPpm,
    /// `P6`.
    Ppm,
    /// `P7`.
    Pam,
    /// `PF` (colour) or `Pf` (grey).
    Pfm,
}

impl NetpbmFormat {
    /// Whether the raster is written in ASCII rather than in binary.
    #[must_use]
    pub const fn is_plain(self) -> bool {
        matches!(self, Self::PlainPbm | Self::PlainPgm | Self::PlainPpm)
    }
}

/// What the samples of a pixel are, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetpbmColour {
    /// One sample, zero black. PBM and `BLACKANDWHITE` come back as this.
    Grey,
    /// Grey, then an opacity, zero transparent.
    GreyAlpha,
    Rgb,
    Rgba,
    /// Cyan, magenta, yellow and black ink, zero no ink.
    Cmyk,
    /// The same, then an opacity.
    CmykAlpha,
}

impl NetpbmColour {
    #[must_use]
    pub const fn components(self) -> u32 {
        match self {
            Self::Grey => 1,
            Self::GreyAlpha => 2,
            Self::Rgb => 3,
            Self::Rgba | Self::Cmyk => 4,
            Self::CmykAlpha => 5,
        }
    }

    /// Whether the last component is an opacity rather than a colour.
    #[must_use]
    pub const fn has_alpha(self) -> bool {
        matches!(self, Self::GreyAlpha | Self::Rgba | Self::CmykAlpha)
    }
}

/// One decoded image.
#[derive(Clone, Debug, PartialEq)]
pub struct NetpbmImage {
    pub width: u32,
    pub height: u32,
    pub format: NetpbmFormat,
    pub colour: NetpbmColour,
    /// 8 or 16: eight under a maxval below 256, sixteen above it and for PFM.
    pub bits_per_component: u8,
    /// The maxval the samples were scaled from: 1 for PBM, the header's for
    /// PGM, PPM and PAM, and `None` for PFM, whose samples are floats.
    pub maxval: Option<u16>,
    /// PFM's scale factor, the magnitude of its third header line, which
    /// states the samples' unit and is not applied to them; `None` elsewhere.
    pub scale: Option<f32>,
    /// Interleaved, row-major from the top, no row padding. Sixteen-bit
    /// components are big-endian, the family's own byte order.
    pub data: Vec<u8>,
    /// False when the raster ended early or a plain raster held junk —
    /// [`crate::Decoded::complete`]'s contract.
    pub complete: bool,
    /// Typed leniency records (ruling 10), deduplicated.
    pub warnings: Vec<Warning>,
}

// --- the refusals -------------------------------------------------------

/// Why a file was refused outright.
///
/// Damage that costs *pixels* is a [`Warning`] and leaves a partial raster
/// (ruling 2); these are the conditions under which there is no picture to
/// degrade, or the samples would mean something else.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetpbmError {
    /// The file begins with none of `P1` to `P7`, `PF` and `Pf`.
    NotNetpbm,
    /// The file ends inside its header.
    TruncatedHeader,
    /// The header holds something its format does not allow where it is,
    /// named: a field that is not a decimal number, a PAM line given twice
    /// or missing, a PFM scale factor that is not a number.
    BadHeader(&'static str),
    /// A width or a height of zero, or one past what 32 bits hold.
    BadDimensions { width: u64, height: u64 },
    /// A maxval of zero or above 65535: "less than 65536, and more than
    /// zero".
    BadMaxval(u64),
    /// A PAM depth of zero, or one with fewer planes than its tuple type
    /// needs.
    BadDepth { depth: u64, needs: u32 },
    /// A PAM tuple type this module does not read, as the header spelled it
    /// (cut to 64 bytes); empty when the header had no `TUPLTYPE` line.
    UnknownTupleType(String),
    /// A PFM scale factor of zero, which the description forbids and which
    /// has no byte order.
    BadScale,
    /// [`MAX_NETPBM_SAMPLES`] would be spent. Refused before any buffer
    /// exists.
    TooManySamples { samples: u64, max: u64 },
    /// The raster would be larger than the caller's own
    /// [`Limits::max_output`].
    ExceedsOutputLimit { bytes: u64, limit: usize },
}

impl core::fmt::Display for NetpbmError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotNetpbm => f.write_str("not a Netpbm file: no P1 to P7, PF or Pf magic number"),
            Self::TruncatedHeader => f.write_str("the Netpbm header is cut off"),
            Self::BadHeader(what) => write!(f, "Netpbm header: {what}"),
            Self::BadDimensions { width, height } => {
                write!(f, "Netpbm dimensions {width} x {height}")
            }
            Self::BadMaxval(maxval) => write!(f, "Netpbm maxval {maxval}"),
            Self::BadDepth { depth, needs } => {
                write!(f, "PAM depth {depth} where its tuple type needs {needs}")
            }
            Self::UnknownTupleType(name) if name.is_empty() => {
                f.write_str("a PAM with no tuple type, which says nothing its samples mean")
            }
            Self::UnknownTupleType(name) => write!(f, "PAM tuple type {name:?}"),
            Self::BadScale => f.write_str("a PFM scale factor of zero"),
            Self::TooManySamples { samples, max } => {
                write!(f, "{samples} samples, ceiling is {max}")
            }
            Self::ExceedsOutputLimit { bytes, limit } => {
                write!(f, "{bytes} bytes of raster, caller's ceiling is {limit}")
            }
        }
    }
}

impl std::error::Error for NetpbmError {}

// --- the entry points ---------------------------------------------------

/// The header, and the first image's raster.
///
/// # Errors
/// Any [`NetpbmError`].
pub fn netpbm_decode(bytes: &[u8], limits: &Limits) -> Result<NetpbmImage, NetpbmError> {
    netpbm_decode_counted(bytes, limits).0
}

/// [`netpbm_decode`], and the steps it took
/// (`docs/design/resource-exhaustion.md`, "The counters"): each header field
/// or PAM header line read, each row of a binary raster, each sample of a
/// plain one, and each byte of the picture returned.
pub fn netpbm_decode_counted(
    bytes: &[u8],
    limits: &Limits,
) -> (Result<NetpbmImage, NetpbmError>, u64) {
    let mut w = Warnings::default();
    let image = decode_with(bytes, limits, &mut w);
    if let Ok(image) = &image {
        w.step_len(image.data.len());
    }
    let steps = w.steps();
    (
        image.map(|mut image| {
            image.warnings = w.into_vec();
            image
        }),
        steps,
    )
}

/// Everything a header says, whichever header it was.
struct Header {
    format: NetpbmFormat,
    width: usize,
    height: usize,
    colour: NetpbmColour,
    /// PBM's 1, the maxval, or 65535 for PFM's sixteen-bit mapping.
    maxval: u32,
    /// Samples a pixel occupies in the file: PAM's depth, or the colour's.
    depth: u64,
    /// Where the raster begins.
    raster: usize,
    /// PFM only.
    scale: Option<f32>,
    little_endian: bool,
}

fn decode_with(
    bytes: &[u8],
    limits: &Limits,
    w: &mut Warnings,
) -> Result<NetpbmImage, NetpbmError> {
    let (Some(&b'P'), Some(&kind)) = (bytes.first(), bytes.get(1)) else {
        return Err(NetpbmError::NotNetpbm);
    };
    let header = match kind {
        b'1' => pnm_header(bytes, NetpbmFormat::PlainPbm, w)?,
        b'2' => pnm_header(bytes, NetpbmFormat::PlainPgm, w)?,
        b'3' => pnm_header(bytes, NetpbmFormat::PlainPpm, w)?,
        b'4' => pnm_header(bytes, NetpbmFormat::Pbm, w)?,
        b'5' => pnm_header(bytes, NetpbmFormat::Pgm, w)?,
        b'6' => pnm_header(bytes, NetpbmFormat::Ppm, w)?,
        b'7' => pam_header(bytes, w)?,
        b'F' => pfm_header(bytes, NetpbmColour::Rgb, w)?,
        b'f' => pfm_header(bytes, NetpbmColour::Grey, w)?,
        _ => return Err(NetpbmError::NotNetpbm),
    };

    let components = header.colour.components();
    let wide = header.format == NetpbmFormat::Pfm || header.maxval > 255;
    let samples = (header.width as u64)
        .saturating_mul(header.height as u64)
        .saturating_mul(u64::from(components));
    if samples > MAX_NETPBM_SAMPLES {
        return Err(NetpbmError::TooManySamples {
            samples,
            max: MAX_NETPBM_SAMPLES,
        });
    }
    let out_bytes = samples.saturating_mul(if wide { 2 } else { 1 });
    if out_bytes > limits.max_output as u64 {
        return Err(NetpbmError::ExceedsOutputLimit {
            bytes: out_bytes,
            limit: limits.max_output,
        });
    }

    // Under `MAX_NETPBM_SAMPLES` twice over, so this fits a `usize` on every
    // target. The fill is the sample value zero: white in a PBM, which is the
    // one format whose zero is not dark.
    let pbm = matches!(header.format, NetpbmFormat::PlainPbm | NetpbmFormat::Pbm);
    let mut data = vec![if pbm { 255u8 } else { 0 }; out_bytes as usize];
    let raster = bytes.get(header.raster..).unwrap_or(&[]);

    let (complete, consumed) = match header.format {
        NetpbmFormat::Pbm => raw_bits(raster, &header, &mut data, w),
        NetpbmFormat::PlainPbm => plain_bits(raster, &header, &mut data, w),
        NetpbmFormat::PlainPgm | NetpbmFormat::PlainPpm => {
            plain_samples(raster, &header, &mut data, w)
        }
        NetpbmFormat::Pgm | NetpbmFormat::Ppm | NetpbmFormat::Pam => {
            raw_samples(raster, &header, &mut data, w)
        }
        NetpbmFormat::Pfm => floats(raster, &header, &mut data, w),
    };
    if !complete {
        w.push(Warning::TruncatedInput);
    }
    let rest = raster.get(consumed..).unwrap_or(&[]);
    after_the_raster(rest, header.format, bytes, w);

    Ok(NetpbmImage {
        width: header.width as u32,
        height: header.height as u32,
        format: header.format,
        colour: header.colour,
        bits_per_component: if wide { 16 } else { 8 },
        maxval: if header.format == NetpbmFormat::Pfm {
            None
        } else {
            // 1 to 65535 by the header checks.
            Some(header.maxval as u16)
        },
        scale: header.scale,
        data,
        complete,
        warnings: Vec::new(),
    })
}

/// What the bytes after the first image's raster are.
fn after_the_raster(rest: &[u8], format: NetpbmFormat, file: &[u8], w: &mut Warnings) {
    if rest.is_empty() {
        return;
    }
    if format.is_plain() {
        // "You can put any junk you want after the raster, if it starts with
        // a white space character."
        if !rest.first().is_some_and(|&b| is_space(b)) {
            w.push(Warning::TrailingGarbage);
        }
        return;
    }
    // "There are no data, delimiters, or padding before, after, or between
    // images": another image of the kind starts exactly where this one ends.
    let another = match format {
        NetpbmFormat::Pfm => false,
        NetpbmFormat::Pam => rest.starts_with(b"P7\n"),
        _ => rest.len() >= 2 && rest.get(..2) == file.get(..2),
    };
    w.push(if another {
        Warning::NetpbmImagesIgnored
    } else {
        Warning::TrailingGarbage
    });
}

// --- the headers --------------------------------------------------------

/// "A 'white space' character is space, CR, LF, TAB, VT, or FF (I.e. what the
/// ANSI standard C isspace() function calls white space)."
const fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\r' | b'\n' | b'\t' | 0x0b | 0x0c)
}

/// The end of a comment that starts at `at`: the next carriage return or line
/// feed, which the comment does not include, or the end of the input.
fn comment_end(bytes: &[u8], at: usize) -> usize {
    bytes
        .get(at..)
        .and_then(|rest| rest.iter().position(|&b| b == b'\r' || b == b'\n'))
        .map_or(bytes.len(), |n| at + n)
}

/// A PNM header's next decimal field: white space and comments before it,
/// then its digits, which must end at white space, a comment or the input's
/// end. Saturating, so a field of a thousand digits is `u64::MAX` rather than
/// an overflow.
fn pnm_number(bytes: &[u8], at: &mut usize, w: &mut Warnings) -> Result<u64, NetpbmError> {
    loop {
        match bytes.get(*at) {
            Some(&b) if is_space(b) => *at += 1,
            Some(b'#') => *at = comment_end(bytes, *at),
            _ => break,
        }
    }
    w.step_by(1);
    let start = *at;
    let mut value = 0u64;
    while let Some(&b) = bytes.get(*at) {
        if !b.is_ascii_digit() {
            break;
        }
        value = value.saturating_mul(10).saturating_add(u64::from(b - b'0'));
        *at += 1;
    }
    match bytes.get(*at) {
        _ if *at == start && *at >= bytes.len() => Err(NetpbmError::TruncatedHeader),
        _ if *at == start => Err(NetpbmError::BadHeader(
            "a header field is not a decimal number",
        )),
        Some(&b) if !is_space(b) && b != b'#' => Err(NetpbmError::BadHeader(
            "a header number runs into a byte that is not white space",
        )),
        _ => Ok(value),
    }
}

/// A width and a height, each from 1 to `u32::MAX`.
fn dimensions(width: u64, height: u64) -> Result<(usize, usize), NetpbmError> {
    let bad = NetpbmError::BadDimensions { width, height };
    if width == 0 || height == 0 || width > u64::from(u32::MAX) || height > u64::from(u32::MAX) {
        return Err(bad);
    }
    match (usize::try_from(width), usize::try_from(height)) {
        (Ok(w), Ok(h)) => Ok((w, h)),
        _ => Err(bad),
    }
}

/// "Must be less than 65536, and more than zero."
fn checked_maxval(maxval: u64) -> Result<u32, NetpbmError> {
    if maxval == 0 || maxval > 65_535 {
        return Err(NetpbmError::BadMaxval(maxval));
    }
    Ok(maxval as u32)
}

/// `P1` to `P6`: the magic number, white space, the width, white space, the
/// height, and for PGM and PPM white space and the maxval; then, for a raw
/// raster, a comment if one is there and the one white-space byte that
/// delimits the raster.
fn pnm_header(bytes: &[u8], format: NetpbmFormat, w: &mut Warnings) -> Result<Header, NetpbmError> {
    // The magic number is followed by white space, which a comment may stand
    // in front of.
    match bytes.get(2) {
        None => return Err(NetpbmError::TruncatedHeader),
        Some(&b) if !is_space(b) && b != b'#' => {
            return Err(NetpbmError::BadHeader(
                "the magic number runs into the width",
            ))
        }
        Some(_) => {}
    }
    let mut at = 2usize;
    let width = pnm_number(bytes, &mut at, w)?;
    let height = pnm_number(bytes, &mut at, w)?;
    let (width, height) = dimensions(width, height)?;
    let (colour, maxval) = match format {
        NetpbmFormat::PlainPbm | NetpbmFormat::Pbm => (NetpbmColour::Grey, 1),
        NetpbmFormat::PlainPgm | NetpbmFormat::Pgm => (
            NetpbmColour::Grey,
            checked_maxval(pnm_number(bytes, &mut at, w)?)?,
        ),
        _ => (
            NetpbmColour::Rgb,
            checked_maxval(pnm_number(bytes, &mut at, w)?)?,
        ),
    };
    // "Before the whitespace character that delimits the raster, any
    // characters from a '#' to but not including the next carriage return or
    // newline character, or end of file, is a comment."
    if bytes.get(at) == Some(&b'#') {
        at = comment_end(bytes, at);
    }
    // "A single whitespace character (usually a newline)." The number reader
    // stopped at white space, a comment or the end, and the comment ends at
    // white space or the end, so this is the delimiter or nothing.
    if at >= bytes.len() {
        return Err(NetpbmError::TruncatedHeader);
    }
    Ok(Header {
        format,
        width,
        height,
        colour,
        maxval,
        depth: u64::from(colour.components()),
        raster: at + 1,
        scale: None,
        little_endian: false,
    })
}

/// The tuple types this module reads: the colour each one is, and the planes
/// it uses.
fn tuple_colour(tuple_type: &[u8]) -> Option<NetpbmColour> {
    Some(match tuple_type {
        b"BLACKANDWHITE" | b"GRAYSCALE" => NetpbmColour::Grey,
        b"BLACKANDWHITE_ALPHA" | b"GRAYSCALE_ALPHA" => NetpbmColour::GreyAlpha,
        b"RGB" => NetpbmColour::Rgb,
        b"RGB_ALPHA" => NetpbmColour::Rgba,
        b"CMYK" => NetpbmColour::Cmyk,
        b"CMYK_ALPHA" => NetpbmColour::CmykAlpha,
        _ => return None,
    })
}

/// A PAM header line's tokens: the first, the second, and whether a third
/// follows.
fn tokens(line: &[u8]) -> (Option<&[u8]>, Option<&[u8]>, bool) {
    let mut parts = line.split(|&b| is_space(b)).filter(|t| !t.is_empty());
    let first = parts.next();
    let second = parts.next();
    (first, second, parts.next().is_some())
}

/// A PAM field's value: its second token, all digits, saturating.
fn pam_number(token: Option<&[u8]>) -> Result<u64, NetpbmError> {
    let Some(token) = token else {
        return Err(NetpbmError::BadHeader("a PAM header line has no value"));
    };
    if !token.iter().all(u8::is_ascii_digit) {
        return Err(NetpbmError::BadHeader(
            "a PAM header value is not a decimal number",
        ));
    }
    Ok(token.iter().fold(0u64, |acc, &b| {
        acc.saturating_mul(10).saturating_add(u64::from(b - b'0'))
    }))
}

/// `P7` and a newline, then lines of white-space-delimited tokens until the
/// `ENDHDR` line; the raster begins after that line's newline.
fn pam_header(bytes: &[u8], w: &mut Warnings) -> Result<Header, NetpbmError> {
    // "The header begins with the ASCII characters 'P7' followed by newline."
    // A carriage return or blank before the newline is white space on the
    // line; anything else is not a PAM, and an image viewer's thumbnail
    // format, which opens `P7 332`, is the case the description names.
    let first_end = bytes
        .iter()
        .position(|&b| b == b'\n')
        .ok_or(NetpbmError::TruncatedHeader)?;
    if !bytes
        .get(2..first_end)
        .is_some_and(|rest| rest.iter().all(|&b| is_space(b)))
    {
        return Err(NetpbmError::BadHeader(
            "P7 is not followed by a newline, so it is not a PAM header",
        ));
    }

    let mut at = first_end + 1;
    let (mut width, mut height, mut depth, mut maxval) = (None, None, None, None);
    let mut tuple_type: Vec<u8> = Vec::new();
    let mut ended = false;
    while let Some(rest) = bytes.get(at..) {
        let Some(n) = rest.iter().position(|&b| b == b'\n') else {
            break;
        };
        let line = rest.get(..n).unwrap_or(&[]);
        at += n + 1;
        w.step_by(1);
        let (first, second, more) = tokens(line);
        let Some(first) = first else {
            // "A header line which has zero tokens is valid but has no
            // meaning."
            continue;
        };
        if first.first() == Some(&b'#') {
            continue;
        }
        let field = match first {
            b"ENDHDR" => {
                if second.is_some() {
                    w.push(Warning::NetpbmHeaderLineIgnored);
                }
                ended = true;
                break;
            }
            b"TUPLTYPE" => {
                // "The rest of the line is not tokenized, but the tuple type
                // does not include any white space immediately following
                // TUPLTYPE or at the very end of the line."
                let after = line
                    .windows(8)
                    .position(|win| win == b"TUPLTYPE")
                    .and_then(|p| line.get(p + 8..))
                    .unwrap_or(&[]);
                let start = after.iter().position(|&b| !is_space(b));
                let end = after.iter().rposition(|&b| !is_space(b));
                match (start, end) {
                    (Some(s), Some(e)) => {
                        if tuple_type.len() < MAX_TUPLE_TYPE {
                            if !tuple_type.is_empty() {
                                tuple_type.push(b' ');
                            }
                            let value = after.get(s..=e).unwrap_or(&[]);
                            let room = MAX_TUPLE_TYPE.saturating_sub(tuple_type.len());
                            tuple_type.extend_from_slice(value.get(..room).unwrap_or(value));
                        }
                    }
                    // "There must be something other than white space after
                    // the TUPLTYPE token."
                    _ => w.push(Warning::NetpbmHeaderLineIgnored),
                }
                continue;
            }
            b"WIDTH" => &mut width,
            b"HEIGHT" => &mut height,
            b"DEPTH" => &mut depth,
            b"MAXVAL" => &mut maxval,
            _ => {
                w.push(Warning::NetpbmHeaderLineIgnored);
                continue;
            }
        };
        if field.is_some() {
            return Err(NetpbmError::BadHeader(
                "a PAM header line that may appear once appears twice",
            ));
        }
        *field = Some(pam_number(second)?);
        if more {
            w.push(Warning::NetpbmHeaderLineIgnored);
        }
    }
    if !ended {
        return Err(NetpbmError::TruncatedHeader);
    }

    let (Some(width), Some(height), Some(depth), Some(maxval)) = (width, height, depth, maxval)
    else {
        return Err(NetpbmError::BadHeader(
            "a PAM header without one of its WIDTH, HEIGHT, DEPTH and MAXVAL lines",
        ));
    };
    let (width, height) = dimensions(width, height)?;
    let maxval = checked_maxval(maxval)?;
    let Some(colour) = tuple_colour(&tuple_type) else {
        return Err(NetpbmError::UnknownTupleType(
            String::from_utf8_lossy(&tuple_type).into_owned(),
        ));
    };
    let needs = colour.components();
    if depth < u64::from(needs) {
        return Err(NetpbmError::BadDepth { depth, needs });
    }
    if depth > u64::from(needs) {
        // "It is good practice to accept a depth that is too great and just
        // ignore the higher numbered planes."
        w.push(Warning::NetpbmPlanesIgnored);
    }
    Ok(Header {
        format: NetpbmFormat::Pam,
        width,
        height,
        colour,
        maxval,
        depth,
        raster: at,
        scale: None,
        little_endian: false,
    })
}

/// Whether `token` is a decimal number as the PFM description means one: a
/// sign, digits with at most one point among them, and an exponent — and not
/// the infinities and NaNs a float parser would also accept.
fn is_decimal(token: &[u8]) -> bool {
    let body = match token.first() {
        Some(b'+' | b'-') => token.get(1..).unwrap_or(&[]),
        _ => token,
    };
    let (mantissa, exponent) = match body.iter().position(|&b| b == b'e' || b == b'E') {
        Some(e) => (body.get(..e).unwrap_or(&[]), body.get(e + 1..)),
        None => (body, None),
    };
    let digits = mantissa.iter().filter(|b| b.is_ascii_digit()).count();
    let points = mantissa.iter().filter(|&&b| b == b'.').count();
    let mantissa_ok = digits > 0 && points <= 1 && digits + points == mantissa.len();
    let exponent_ok = exponent.is_none_or(|e| {
        let e = match e.first() {
            Some(b'+' | b'-') => e.get(1..).unwrap_or(&[]),
            _ => e,
        };
        !e.is_empty() && e.iter().all(u8::is_ascii_digit)
    });
    mantissa_ok && exponent_ok
}

/// `PF` or `Pf`, the width and the height, and the scale factor whose sign
/// is the byte order, each followed by white space; the raster begins after
/// the one white-space byte that follows the scale.
fn pfm_header(bytes: &[u8], colour: NetpbmColour, w: &mut Warnings) -> Result<Header, NetpbmError> {
    let mut at = 2usize;
    let mut token = |at: &mut usize| -> Result<(usize, usize), NetpbmError> {
        let before = *at;
        while bytes.get(*at).is_some_and(|&b| is_space(b)) {
            *at += 1;
        }
        if *at == before {
            return Err(if *at >= bytes.len() {
                NetpbmError::TruncatedHeader
            } else {
                NetpbmError::BadHeader("a PFM header field is not preceded by white space")
            });
        }
        let start = *at;
        while bytes.get(*at).is_some_and(|&b| !is_space(b)) {
            *at += 1;
        }
        w.step_by(1);
        if *at >= bytes.len() {
            // Every PFM field is followed by a white-space character.
            return Err(NetpbmError::TruncatedHeader);
        }
        Ok((start, *at))
    };
    let number = |range: (usize, usize)| -> Result<u64, NetpbmError> {
        let digits = bytes.get(range.0..range.1).unwrap_or(&[]);
        if !digits.iter().all(u8::is_ascii_digit) {
            return Err(NetpbmError::BadHeader(
                "a PFM dimension is not a decimal integer",
            ));
        }
        Ok(digits.iter().fold(0u64, |acc, &b| {
            acc.saturating_mul(10).saturating_add(u64::from(b - b'0'))
        }))
    };
    let width = number(token(&mut at)?)?;
    let height = number(token(&mut at)?)?;
    let (width, height) = dimensions(width, height)?;
    let (start, end) = token(&mut at)?;
    let text = bytes.get(start..end).unwrap_or(&[]);
    if !is_decimal(text) {
        return Err(NetpbmError::BadHeader(
            "the PFM scale factor is not a decimal number",
        ));
    }
    // ASCII by the check above, and `str::parse` rounds correctly on every
    // target (ruling 4). Read as a double so that a small scale is not taken
    // for zero, and handed back as the single the samples are.
    let scale = core::str::from_utf8(text)
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .ok_or(NetpbmError::BadHeader(
            "the PFM scale factor is not a decimal number",
        ))?;
    if scale == 0.0 {
        return Err(NetpbmError::BadScale);
    }
    Ok(Header {
        format: NetpbmFormat::Pfm,
        width,
        height,
        colour,
        maxval: 65_535,
        depth: u64::from(colour.components()),
        // The one white-space byte after the scale factor.
        raster: end + 1,
        scale: Some(scale.abs() as f32),
        little_endian: scale < 0.0,
    })
}

// --- the rasters --------------------------------------------------------
//
// Each returns whether the raster was whole and how many of its bytes it
// read, so the bytes after it can be judged.

/// `P4`: rows of `ceil(width / 8)` bytes, most significant bit first, `1`
/// black.
fn raw_bits(raster: &[u8], header: &Header, out: &mut [u8], w: &mut Warnings) -> (bool, usize) {
    let row_bytes = header.width.div_ceil(8);
    for y in 0..header.height {
        w.step_by(1);
        let start = y.saturating_mul(row_bytes);
        let row = raster.get(start..).unwrap_or(&[]);
        for x in 0..header.width {
            let Some(&byte) = row.get(x / 8) else {
                return (false, raster.len());
            };
            if byte & (0x80 >> (x % 8)) != 0 {
                if let Some(slot) = out.get_mut(y * header.width + x) {
                    *slot = 0;
                }
            }
        }
    }
    (true, row_bytes.saturating_mul(header.height))
}

/// The white space before a plain raster's next sample. A comment there is
/// outside the descriptions, which put comments before the raster only, and
/// cannot be a sample, so it is read as one would be before the raster and
/// skipped, with [`Warning::NetpbmRasterJunk`].
fn plain_space(raster: &[u8], at: &mut usize, w: &mut Warnings) {
    loop {
        match raster.get(*at) {
            Some(&b) if is_space(b) => *at += 1,
            Some(b'#') => {
                w.push(Warning::NetpbmRasterJunk);
                *at = comment_end(raster, *at);
            }
            _ => break,
        }
    }
}

/// `P1`: one `0` or `1` a pixel, white space between them ignored.
fn plain_bits(raster: &[u8], header: &Header, out: &mut [u8], w: &mut Warnings) -> (bool, usize) {
    let pixels = header.width * header.height;
    let mut at = 0usize;
    for index in 0..pixels {
        w.step_by(1);
        plain_space(raster, &mut at, w);
        match raster.get(at) {
            Some(b'0') => {}
            Some(b'1') => {
                if let Some(slot) = out.get_mut(index) {
                    *slot = 0;
                }
            }
            None => return (false, at),
            Some(_) => {
                w.push(Warning::NetpbmRasterJunk);
                return (false, raster.len());
            }
        }
        at += 1;
    }
    (true, at)
}

/// Writes one sample at `index` of the output, scaled from `maxval`, and says
/// whether it was above it.
fn put(out: &mut [u8], index: usize, value: u64, maxval: u32, wide: bool) -> bool {
    let above = value > u64::from(maxval);
    let value = if above { maxval } else { value as u32 };
    if wide {
        let bytes = scale_to_16(value, maxval).to_be_bytes();
        if let Some(slot) = out.get_mut(index * 2..index * 2 + 2) {
            slot.copy_from_slice(&bytes);
        }
    } else if let Some(slot) = out.get_mut(index) {
        *slot = scale_to_8(value, maxval);
    }
    above
}

/// `P2` and `P3`: ASCII decimal samples of any length, white space between.
fn plain_samples(
    raster: &[u8],
    header: &Header,
    out: &mut [u8],
    w: &mut Warnings,
) -> (bool, usize) {
    let samples = header.width * header.height * header.colour.components() as usize;
    let wide = header.maxval > 255;
    let mut above = false;
    let mut at = 0usize;
    let mut whole = true;
    for index in 0..samples {
        w.step_by(1);
        plain_space(raster, &mut at, w);
        let start = at;
        let mut value = 0u64;
        while let Some(&b) = raster.get(at) {
            if !b.is_ascii_digit() {
                break;
            }
            value = value.saturating_mul(10).saturating_add(u64::from(b - b'0'));
            at += 1;
        }
        if at == start {
            if at < raster.len() {
                w.push(Warning::NetpbmRasterJunk);
                at = raster.len();
            }
            whole = false;
            break;
        }
        above |= put(out, index, value, header.maxval, wide);
    }
    if above {
        w.push(Warning::NetpbmSampleAboveMaxval);
    }
    (whole, at)
}

/// `P5`, `P6` and `P7`: a byte a sample under a maxval below 256, two most
/// significant first above it, `depth` samples a pixel of which the colour's
/// first planes are read.
fn raw_samples(raster: &[u8], header: &Header, out: &mut [u8], w: &mut Warnings) -> (bool, usize) {
    let wide = header.maxval > 255;
    let sample_bytes: u64 = if wide { 2 } else { 1 };
    let used = header.colour.components() as usize;
    let tuple = header.depth.saturating_mul(sample_bytes);
    let row = tuple.saturating_mul(header.width as u64);
    // The eight-bit table, built once: each byte's scaled value.
    let mut table = [0u8; 256];
    if !wide {
        for (value, slot) in table.iter_mut().enumerate() {
            *slot = scale_to_8(value as u32, header.maxval);
        }
    }
    let narrow_max = header.maxval as usize;
    let mut above = false;
    for y in 0..header.height {
        w.step_by(1);
        let row_at = row.saturating_mul(y as u64);
        for x in 0..header.width {
            let at = row_at.saturating_add(tuple.saturating_mul(x as u64));
            let first = (y * header.width + x) * used;
            let Some(px) = usize::try_from(at)
                .ok()
                .and_then(|at| raster.get(at..at.checked_add(used * sample_bytes as usize)?))
            else {
                return (false, raster.len());
            };
            if wide {
                for (plane, pair) in px.chunks_exact(2).enumerate() {
                    let &[high, low] = pair else {
                        continue;
                    };
                    let value = u16::from_be_bytes([high, low]);
                    above |= put(out, first + plane, u64::from(value), header.maxval, true);
                }
            } else {
                for (plane, &byte) in px.iter().enumerate() {
                    above |= usize::from(byte) > narrow_max;
                    if let Some(slot) = out.get_mut(first + plane) {
                        *slot = table[usize::from(byte)];
                    }
                }
            }
        }
    }
    if above {
        w.push(Warning::NetpbmSampleAboveMaxval);
    }
    let read = row.saturating_mul(header.height as u64);
    (true, usize::try_from(read).unwrap_or(raster.len()))
}

/// PFM: four-byte IEEE floats in the header's byte order, rows bottom to top,
/// each mapped from `[0, 1]` onto sixteen bits.
fn floats(raster: &[u8], header: &Header, out: &mut [u8], w: &mut Warnings) -> (bool, usize) {
    let components = header.colour.components() as usize;
    let row_bytes = header.width * components * 4;
    for stored in 0..header.height {
        w.step_by(1);
        // "The rows ordered bottom to top."
        let y = header.height - 1 - stored;
        let start = stored.saturating_mul(row_bytes);
        let row = raster.get(start..).unwrap_or(&[]);
        for i in 0..header.width * components {
            let Some(&[a, b, c, d]) = row.get(i * 4..i * 4 + 4) else {
                return (false, raster.len());
            };
            let bits = [a, b, c, d];
            let value = f32::from_bits(if header.little_endian {
                u32::from_le_bytes(bits)
            } else {
                u32::from_be_bytes(bits)
            });
            let index = y * header.width * components + i;
            if let Some(slot) = out.get_mut(index * 2..index * 2 + 2) {
                slot.copy_from_slice(&float_to_16(value).to_be_bytes());
            }
        }
    }
    (true, row_bytes.saturating_mul(header.height))
}

/// `tiff.rs`'s floating-point mapping: `[0, 1]` onto `0..=65535`, rounded to
/// nearest, clamped outside it, a NaN black. IEEE double arithmetic, whose
/// operations and `floor` are correctly rounded on every target (ruling 4).
fn float_to_16(value: f32) -> u16 {
    let value = f64::from(value);
    if value.is_nan() {
        return 0;
    }
    let unit = value.clamp(0.0, 1.0);
    // In `0..=65535` by the clamp.
    (unit * 65_535.0 + 0.5).floor() as u16
}

#[cfg(test)]
mod tests;
