//! JPEG XR (ITU-T T.832 | ISO/IEC 29199-2), the decoder.
//!
//! Scope, the evidence design, and the milestone order: `docs/design/jpeg-xr.md`.
//!
//! # Why this is a free function and not a `Filter`
//!
//! No PDF stream is a JPEG XR image. `/Filter` names JPXDecode, DCTDecode,
//! JBIG2Decode and CCITTFaxDecode, and none of them reaches this format;
//! JPEG XR arrives only as an XPS image part, which OPC 9.1.5.1 recommends
//! and which the container half of this crate already sniffs. So this is a
//! sibling of [`crate::png_decode`] — bytes and a [`Limits`] in, a raster and
//! values out — and it is deliberately absent from [`crate::Filter`] and
//! [`crate::ImageCodec`], which are PDF filter dispatch and would be made
//! wrong by an entry nothing can name.
//!
//! For the same reason its warnings are [`JxrWarning`] rather than
//! [`crate::Warning`]. That enum is the closed set of leniencies a *PDF
//! stream filter* performs, and its variants are matched by name where a
//! reader attaches the object they happened in. A container-level codec that
//! no `/Filter` reaches has nothing to attach, so it carries its own set and
//! leaves that one closed.
//!
//! # A module directory, deliberately
//!
//! `jpx/` is the other one, and this is the second. Both formats are large
//! enough that a single file is a scrolling exercise rather than a structure,
//! and both split on the same seam: the bit reader, the container, the
//! headers, the entropy-coded coefficients, the transform, and the colour
//! pipeline are six separable concerns with narrow interfaces between them.
//! `jpeg.rs`, `jbig2.rs` and `ccitt.rs` stay single files because they do not
//! have that seam.
//!
//! # Determinism (ruling 4)
//!
//! T.832's transform is specified in **integers** — 9.9.7's inverse transform
//! is a lifting structure over `i32`, and 9.8's dequantization is a multiply
//! — so nothing on the pixel path here has any reason to reach for a float.
//! Every module in this directory carries `#![deny(clippy::float_arithmetic)]`
//! the way `tinker-pdf-shape` does, which makes that a build failure rather
//! than a convention. `cargo xtask libm` covers the other half.
//!
//! # Bounds (ruling 1)
//!
//! The structure branches on tiles x macroblocks x components x blocks x
//! coefficients, and T.832 bounds each factor without bounding the product:
//! 8.3.23 permits 4096 tile columns and 4096 tile rows, 8.4.12 permits 4111
//! components, and 8.3.21's dimensions are 32-bit. The budgets below are
//! **totals**, checked before any buffer exists, and the per-item caps beside
//! them each say in as many words that they are not the work cap — the lesson
//! `jpx/mod.rs` records at length and which is the same lesson here.

#![deny(clippy::float_arithmetic)]

pub(crate) mod bitstream;
pub(crate) mod coefficients;
pub(crate) mod colour;
pub(crate) mod container;
pub(crate) mod headers;
pub(crate) mod overlap;
pub(crate) mod tables;
pub(crate) mod transform;

use crate::Limits;

pub use container::{JxrChannels, JxrPixelFormat};

// --- the budget ---------------------------------------------------------

/// Samples in the decoded raster, summed over every component.
///
/// **A total, not a per-item cap.** The magnitude is the one
/// `jpx::MAX_JPX_SAMPLES` is measured to: `1 << 26` is 4096 x 4096 x 4
/// components, whose interleaved 16-bit output is exactly
/// `tinker_pdf_cos::limits::MAX_DECODED_STREAM`. A caller's own
/// [`Limits::max_output`] is checked as well and the smaller wins.
pub(crate) const MAX_JXR_SAMPLES: u64 = 1 << 26;

/// Tiles in one image.
///
/// **Not the work cap** — [`MAX_JXR_SAMPLES`] is. 8.3.23 and 8.3.24 are
/// 12-bit fields, so the standard's own ceiling is 4096 x 4096 = 16 777 216
/// tiles, and a tile is at least one macroblock of 256 samples: a codestream
/// at the standard's ceiling is describing 4 294 967 296 samples, which
/// [`MAX_JXR_SAMPLES`] already refuses. This cap bounds the *bookkeeping*
/// instead — the index table is one `VLW_ESC( )` per tile per band and is
/// read before a single macroblock is — and it is the same order as
/// `jpx`'s 65 535, which is that standard's own tile bound.
pub(crate) const MAX_JXR_TILES: u64 = 65_535;

/// Components in one image plane.
///
/// **Not the work cap.** 8.4.12 permits 4111 and this build refuses past 16,
/// which is the ceiling Annex A's own note gives the NCOMPONENT formats
/// ("n in the range of 2 to 16, inclusive"). A file above it is refused by
/// name rather than truncated to something interpretable.
pub(crate) const MAX_JXR_COMPONENTS: u32 = 16;

/// Macroblocks in one image, across every tile.
///
/// **Not the work cap** on its own, but the one that catches the shape
/// [`MAX_JXR_SAMPLES`] cannot: the coefficient decoder allocates per
/// macroblock — a DC value, sixteen LP coefficients and 240 HP coefficients
/// per component — before it knows how many samples survive cropping, and
/// 8.3.21's 32-bit dimensions can describe a macroblock grid whose product
/// overflows what the sample budget was computed from.
pub(crate) const MAX_JXR_MACROBLOCKS: u64 = 1 << 22;

// --- refusals -----------------------------------------------------------

/// Why an image was refused outright.
///
/// Refusals rather than warnings, for the reason the JPEG 2000 decoder in
/// this crate states and which is sharper still here: **a wrong JPEG XR
/// decode looks like a photograph.** The inverse transform of 9.9 is a
/// smoothing operator over a lapped basis, so wrong coefficients do not
/// produce noise — they produce a soft, plausible picture with faint seams at
/// the block edges, which is exactly what a human skimming a page does not
/// see. Every capability this build does not have is therefore named, and
/// nothing is defaulted past.
///
/// Damage that costs *pixels* rather than meaning is a [`JxrWarning`] and
/// leaves a partial image (ruling 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JxrError {
    /// Neither Annex A's file header nor 8.3.2's `GDI_SIGNATURE`.
    NotJxr,
    /// A.5.4: `FILE_VERSION_ID` other than 1.
    UnsupportedFileVersion(u8),
    /// 8.3.3: `RESERVED_B` other than 1, which the clause reserves as the
    /// signal of a codestream not compatible with prior decoder versions.
    UnsupportedCodestreamVersion(u8),
    /// The bytes ended inside a syntax structure.
    Truncated,
    /// One of Table A.4's four Required tags was absent or unreadable. The
    /// tag number is carried because "no PIXEL_FORMAT" and "no IMAGE_OFFSET"
    /// are different sentences to show a human.
    MissingRequiredTag(u16),
    /// A syntax element took one of the values its clause marks RESERVED.
    /// The name is the clause's own spelling of the element.
    ReservedValue(&'static str),
    /// A width or height of zero, or an extended size that is not the
    /// multiple of 16 that 8.3.21 and 8.3.22 require.
    BadDimensions,
    /// The declared tile widths or heights do not partition the macroblock
    /// grid — 8.3.25's last-tile subtraction would underflow.
    BadTiling,
    /// 8.5.2's start code was wrong, or the table's size cannot be reconciled
    /// with the codestream's length, or more than one tile packet exists with
    /// no table to locate them.
    BadIndexTable,
    /// 8.6's profile/level block did not terminate inside `SubsequentBytes`.
    BadProfileLevel,
    /// The alpha image plane's header contradicts 8.4.2 or Table 30.
    BadAlphaPlane,
    /// 8.7.10.1: a tile packet did not begin with `0x000001`. The tile index
    /// is carried, because a file whose *first* tile is misplaced is a
    /// different failure from one whose last is.
    BadTileStartCode(u32),
    /// [`MAX_JXR_SAMPLES`] would be spent. Refused before any buffer exists.
    TooManySamples { samples: u64, max: u64 },
    /// [`MAX_JXR_TILES`] would be spent.
    TooManyTiles { tiles: u64, max: u64 },
    /// [`MAX_JXR_COMPONENTS`] would be spent.
    TooManyComponents { components: u32, max: u32 },
    /// [`MAX_JXR_MACROBLOCKS`] would be spent.
    TooManyMacroblocks { macroblocks: u64, max: u64 },
    /// The raster would be larger than the caller's own
    /// [`Limits::max_output`]. Separate from [`JxrError::TooManySamples`]
    /// because one number is this crate's and the other is the caller's.
    ExceedsOutputLimit { bytes: u64, limit: usize },
    /// A capability this build refuses by name. The list is
    /// `docs/features/filters.md`'s, and every variant is reached by a test.
    Unsupported(JxrRefusal),
}

/// What this build refuses, by name.
///
/// Each variant is a *decision*, and each is reachable by a test in
/// `tests::refusals` — "the refusals are the feature" is only a claim if
/// something checks that they fire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JxrRefusal {
    /// Table A.6 row whose "Num" column is SINT or Float. 9.10.7's
    /// postscaling for fixed-point and floating-point output is a different
    /// output stage, and approximating it would return an image whose
    /// numbers mean something other than what the file said.
    FloatOrFixedPointFormat,
    /// Table A.6's CMYK, CMYKDIRECT, NCOMPONENT and RGBE rows.
    UnsupportedColourFormat,
    /// A Table A.6 GUID this build has no row for at all.
    UnknownPixelFormat,
    /// 8.3.20's BD1WHITE1, BD1BLACK1, BD5, BD565 and BD10 — the packed
    /// sub-byte and cross-byte output depths of 9.10.8.3 to 9.10.8.6.
    PackedOutputBitdepth,
    /// 8.3.18's interleaved alpha image plane.
    InterleavedAlphaPlane,
    /// Table 28's YUV420, YUV422 and YUVK internal colour formats — the
    /// subsampled and four-component internal layouts, whose chroma
    /// upsampling is 9.10.3 and whose macroblock geometry is not the
    /// 4:4:4 one this build implements.
    SubsampledInternalFormat,
    /// 8.3.13's windowing with a non-zero top or left margin. The bottom and
    /// right margins are the ordinary padding to a multiple of 16 and are
    /// always handled; a top or left margin shifts the whole sample grid.
    WindowedOrigin,
}

impl core::fmt::Display for JxrError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotJxr => f.write_str("not a JPEG XR file or codestream"),
            Self::UnsupportedFileVersion(v) => write!(f, "JPEG XR file version {v}"),
            Self::UnsupportedCodestreamVersion(v) => {
                write!(f, "JPEG XR codestream version {v}")
            }
            Self::Truncated => f.write_str("the codestream ended inside a syntax structure"),
            Self::MissingRequiredTag(t) => write!(f, "required IFD tag 0x{t:04X} is absent"),
            Self::ReservedValue(n) => write!(f, "{n} took a reserved value"),
            Self::BadDimensions => {
                f.write_str("image dimensions do not describe a macroblock grid")
            }
            Self::BadTiling => f.write_str("tile sizes do not partition the macroblock grid"),
            Self::BadIndexTable => f.write_str("the tile index table is unusable"),
            Self::BadProfileLevel => f.write_str("the profile and level block is unterminated"),
            Self::BadAlphaPlane => f.write_str("the alpha image plane header is inconsistent"),
            Self::BadTileStartCode(n) => write!(f, "tile {n} has no start code"),
            Self::TooManySamples { samples, max } => {
                write!(f, "{samples} samples exceeds the budget of {max}")
            }
            Self::TooManyTiles { tiles, max } => {
                write!(f, "{tiles} tiles exceeds the budget of {max}")
            }
            Self::TooManyComponents { components, max } => {
                write!(f, "{components} components exceeds the budget of {max}")
            }
            Self::TooManyMacroblocks { macroblocks, max } => {
                write!(f, "{macroblocks} macroblocks exceeds the budget of {max}")
            }
            Self::ExceedsOutputLimit { bytes, limit } => {
                write!(f, "{bytes} bytes exceeds the caller's limit of {limit}")
            }
            Self::Unsupported(r) => write!(f, "unsupported JPEG XR feature: {r:?}"),
        }
    }
}

impl std::error::Error for JxrError {}

/// Every leniency this decoder performs, as data rather than a log line
/// (ruling 10).
///
/// Separate from [`crate::Warning`] on purpose — see the module docs. Each is
/// recorded at most once per decode, so a file with a million damaged tiles
/// cannot turn leniency into an allocation attack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum JxrWarning {
    /// A.7.2 requires ascending tags; these were not.
    IfdTagsOutOfOrder,
    /// 8.4.21's alignment padding carried a bit that was not zero.
    NonZeroPadding,
    /// A reserved flag was set where its clause gives it no meaning.
    ReservedFlagSet(&'static str),
    /// Annex A's `IMAGE_WIDTH`/`IMAGE_HEIGHT` disagreed with the
    /// codestream's own `WIDTH_MINUS1`/`HEIGHT_MINUS1`. The codestream wins,
    /// because it is what the samples were coded against.
    ContainerDimensionsDisagree,
    /// A tile packet was damaged or ran short; 8.7.10.1's own note says to
    /// infer zero coefficients for it, which is what this decoder does.
    TileDroppedAsZero,
    /// The image was decoded but the container asked for an orientation
    /// (Table 21) that this crate reports rather than applies.
    SpatialTransformNotApplied,
    /// A.3.2's separate alpha image plane was present and could not be
    /// decoded, so the image is opaque where the file said transparent.
    ///
    /// A warning rather than a refusal, and the choice is ruling 2's: an
    /// unreadable alpha plane costs *transparency*, not the picture, and a
    /// page that loses its illustration because a channel it may not even use
    /// was damaged is the worse outcome. It sets `complete` to false, so a
    /// caller that does care can tell.
    AlphaPlaneDropped,
}

impl JxrWarning {
    /// A stable name, for the dedup key a caller sorts on.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IfdTagsOutOfOrder => "jxr-ifd-tags-out-of-order",
            Self::NonZeroPadding => "jxr-non-zero-padding",
            Self::ReservedFlagSet(_) => "jxr-reserved-flag-set",
            Self::ContainerDimensionsDisagree => "jxr-container-dimensions-disagree",
            Self::TileDroppedAsZero => "jxr-tile-dropped-as-zero",
            Self::SpatialTransformNotApplied => "jxr-spatial-transform-not-applied",
            Self::AlphaPlaneDropped => "jxr-alpha-plane-dropped",
        }
    }
}

/// The output raster, plus everything needed to address a pixel in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JxrImage {
    pub width: u32,
    pub height: u32,
    /// Table A.6's row, which names the channel order the data is in.
    pub format: JxrPixelFormat,
    /// Interleaved, row-major, no row padding. 16-bit components are
    /// **little-endian**, which is this container's byte order (A.7.3) and
    /// not a choice made here — the one place this differs from
    /// [`crate::PngImage`], whose 16-bit samples are big-endian because PNG's
    /// are.
    pub data: Vec<u8>,
    /// False when a tile had to be dropped or the codestream ran short — the
    /// same contract as [`crate::Decoded::complete`].
    pub complete: bool,
    pub warnings: Vec<JxrWarning>,
}

impl JxrImage {
    /// Samples per pixel.
    #[must_use]
    pub const fn channels(&self) -> u8 {
        self.format.channels.count()
    }

    /// 8 or 16.
    #[must_use]
    pub const fn bits_per_component(&self) -> u8 {
        self.format.bits_per_component
    }
}

/// Decodes an Annex A file, or a bare `CODED_IMAGE( )` codestream.
///
/// Both are accepted because 9.1.5.1's XPS image part is the first and
/// systems that carry a codestream without the container are the second, and
/// a caller holding bytes should not have to decide which it has.
///
/// # Errors
/// [`JxrError`], every variant of which is a decision rather than data — see
/// its documentation and `docs/features/filters.md`'s refusal table. Damage
/// that costs pixels leaves a [`JxrWarning`] and a partial image instead
/// (ruling 2).
pub fn jxr_decode(bytes: &[u8], limits: &Limits) -> Result<JxrImage, JxrError> {
    let mut warnings: Vec<JxrWarning> = Vec::new();

    // A bare codestream is recognised by 8.3.2's signature; anything else is
    // put to Annex A, which refuses what is neither.
    let (codestream, container) = if bytes.starts_with(&container::GDI_SIGNATURE) {
        (bytes, None)
    } else {
        let c = container::read(bytes)?;
        for w in &c.warnings {
            push_once(&mut warnings, *w);
        }
        let slice = bytes.get(c.image.clone()).ok_or(JxrError::Truncated)?;
        (slice, Some(c))
    };

    // Annex A's pixel format is what names the output channel order. A bare
    // codestream has none, so the header's own OUTPUT_CLR_FMT and
    // OUTPUT_BITDEPTH stand in — resolved after the header is read.
    if let Some(c) = &container {
        if c.format_known_unsupported {
            return Err(JxrError::Unsupported(JxrRefusal::UnknownPixelFormat));
        }
        if c.spatial_transform != 0 {
            push_once(&mut warnings, JxrWarning::SpatialTransformNotApplied);
        }
    }

    let mut reader = bitstream::BitReader::new(codestream);
    let headers = headers::CodedImageHeaders::read(&mut reader, &mut warnings)?;

    // The refusals, in the order a file meets them.
    refuse_unsupported(&headers)?;

    let format = resolve_format(container.as_ref(), &headers)?;

    if let Some(c) = &container {
        if c.width != headers.image.width || c.height != headers.image.height {
            push_once(&mut warnings, JxrWarning::ContainerDimensionsDisagree);
        }
    }

    // Budgets, all before a buffer exists (ruling 1).
    let macroblocks = u64::from(headers.image.mb_width)
        .checked_mul(u64::from(headers.image.mb_height))
        .ok_or(JxrError::BadDimensions)?;
    if macroblocks > MAX_JXR_MACROBLOCKS {
        return Err(JxrError::TooManyMacroblocks {
            macroblocks,
            max: MAX_JXR_MACROBLOCKS,
        });
    }
    let samples = u64::from(headers.image.extended_width)
        .checked_mul(u64::from(headers.image.extended_height))
        .and_then(|n| n.checked_mul(u64::from(headers.primary.num_components)))
        .ok_or(JxrError::BadDimensions)?;
    if samples > MAX_JXR_SAMPLES {
        return Err(JxrError::TooManySamples {
            samples,
            max: MAX_JXR_SAMPLES,
        });
    }
    let out_bytes = u64::from(headers.image.width)
        .checked_mul(u64::from(headers.image.height))
        .and_then(|n| n.checked_mul(u64::from(format.channels.count())))
        .and_then(|n| n.checked_mul(u64::from(format.bits_per_component / 8)))
        .ok_or(JxrError::BadDimensions)?;
    if out_bytes > limits.max_output as u64 {
        return Err(JxrError::ExceedsOutputLimit {
            bytes: out_bytes,
            limit: limits.max_output,
        });
    }

    let planes = coefficients::decode_image(&mut reader, &headers, &mut warnings)?;

    // A.3.2's separate alpha image plane: a second, complete `CODED_IMAGE( )`
    // at `ALPHA_OFFSET`, with its own header, its own `SCALED_FLAG` and its
    // own `SHIFT_BITS`. It is decoded here rather than inside `colour.rs`
    // because it is a whole image, not a channel — and only when the pixel
    // format actually has an alpha channel to put it in, so that a file
    // carrying one for a format Table A.6 gives no alpha does not spend the
    // budget decoding something with nowhere to go.
    let alpha_range = container
        .as_ref()
        .filter(|_| format.channels.has_alpha())
        .and_then(|c| c.alpha.clone());
    let alpha = match alpha_range {
        Some(range) => {
            let decoded = decode_alpha_plane(bytes, range, &headers, &mut warnings);
            if decoded.is_none() {
                push_once(&mut warnings, JxrWarning::AlphaPlaneDropped);
            }
            decoded
        }
        None => None,
    };

    let complete = !warnings.contains(&JxrWarning::TileDroppedAsZero)
        && !warnings.contains(&JxrWarning::AlphaPlaneDropped);
    let alpha_ref = alpha.as_ref().map(|(p, h)| (p, &h.primary));
    let data = colour::format_output(&planes, &headers, format, alpha_ref)?;

    warnings.sort();
    warnings.dedup();
    Ok(JxrImage {
        width: headers.image.width,
        height: headers.image.height,
        format,
        data,
        complete,
        warnings,
    })
}

/// A.3.2's separate alpha image plane: a second, complete `CODED_IMAGE( )`.
///
/// Returns `None` for anything wrong with it, which the caller turns into
/// [`JxrWarning::AlphaPlaneDropped`] and an opaque image (ruling 2). The
/// checks are 8.4.2's and Table 30's: one YONLY component at the primary's
/// dimensions. A plane that disagrees has not said what its transparency is,
/// and inventing it is worse than admitting the loss.
fn decode_alpha_plane(
    bytes: &[u8],
    range: core::ops::Range<usize>,
    primary: &headers::CodedImageHeaders,
    warnings: &mut Vec<JxrWarning>,
) -> Option<(coefficients::Planes, headers::CodedImageHeaders)> {
    let slice = bytes.get(range)?;
    let mut r = bitstream::BitReader::new(slice);
    // The alpha plane's own leniencies are its own; they are collected into a
    // scratch list and discarded, because a warning that says "the padding
    // bit was set" without saying which plane it was in is worse than none.
    let mut scratch = Vec::new();
    let h = headers::CodedImageHeaders::read(&mut r, &mut scratch).ok()?;
    if h.image.width != primary.image.width
        || h.image.height != primary.image.height
        || h.primary.num_components != 1
        || h.primary.internal_clr_fmt != headers::InternalClrFmt::YOnly
    {
        return None;
    }
    refuse_unsupported(&h).ok()?;
    let planes = coefficients::decode_image(&mut r, &h, &mut scratch).ok()?;
    // A dropped tile inside the alpha plane still costs transparency, so it
    // is promoted to the one warning the caller can attribute.
    if scratch.contains(&JxrWarning::TileDroppedAsZero) {
        push_once(warnings, JxrWarning::TileDroppedAsZero);
    }
    Some((planes, h))
}

/// Records a warning at most once (ruling 10's dedup contract).
fn push_once(warnings: &mut Vec<JxrWarning>, w: JxrWarning) {
    if !warnings.contains(&w) {
        warnings.push(w);
    }
}

/// Every named refusal that can be decided from the headers alone.
fn refuse_unsupported(h: &headers::CodedImageHeaders) -> Result<(), JxrError> {
    use headers::{InternalClrFmt, OutputBitdepth, OutputClrFmt};

    if h.image.alpha_image_plane {
        return Err(JxrError::Unsupported(JxrRefusal::InterleavedAlphaPlane));
    }
    match h.primary.internal_clr_fmt {
        InternalClrFmt::YOnly | InternalClrFmt::Yuv444 => {}
        InternalClrFmt::Yuv420 | InternalClrFmt::Yuv422 | InternalClrFmt::Yuvk => {
            return Err(JxrError::Unsupported(JxrRefusal::SubsampledInternalFormat));
        }
        InternalClrFmt::NComponent => {
            return Err(JxrError::Unsupported(JxrRefusal::UnsupportedColourFormat));
        }
    }
    match h.image.output_clr_fmt {
        OutputClrFmt::YOnly | OutputClrFmt::Rgb => {}
        OutputClrFmt::Yuv420 | OutputClrFmt::Yuv422 | OutputClrFmt::Yuv444 => {
            return Err(JxrError::Unsupported(JxrRefusal::SubsampledInternalFormat));
        }
        OutputClrFmt::Cmyk
        | OutputClrFmt::CmykDirect
        | OutputClrFmt::NComponent
        | OutputClrFmt::Rgbe => {
            return Err(JxrError::Unsupported(JxrRefusal::UnsupportedColourFormat));
        }
    }
    match h.image.output_bitdepth {
        OutputBitdepth::Bd8 | OutputBitdepth::Bd16 => {}
        OutputBitdepth::Bd16S
        | OutputBitdepth::Bd32S
        | OutputBitdepth::Bd16F
        | OutputBitdepth::Bd32F => {
            return Err(JxrError::Unsupported(JxrRefusal::FloatOrFixedPointFormat));
        }
        OutputBitdepth::Bd1White1
        | OutputBitdepth::Bd1Black1
        | OutputBitdepth::Bd5
        | OutputBitdepth::Bd565
        | OutputBitdepth::Bd10 => {
            return Err(JxrError::Unsupported(JxrRefusal::PackedOutputBitdepth));
        }
    }
    // 6.3: the top and left margins shift the whole output window off the
    // macroblock grid's origin. The bottom and right margins are the ordinary
    // padding to a multiple of 16 and cost nothing.
    if h.image.top_margin != 0 || h.image.left_margin != 0 {
        return Err(JxrError::Unsupported(JxrRefusal::WindowedOrigin));
    }
    Ok(())
}

/// Table A.6's row, or the equivalent derived from the codestream when there
/// is no container to state one.
fn resolve_format(
    container: Option<&container::Container>,
    h: &headers::CodedImageHeaders,
) -> Result<JxrPixelFormat, JxrError> {
    use headers::{OutputBitdepth, OutputClrFmt};

    if let Some(c) = container {
        if let Some(f) = c.format {
            return Ok(f);
        }
    }
    // A bare codestream: 8.3.19 and 8.3.20 say everything Table A.6 would
    // have. The mnemonic is the table's, so a warning reads the same either
    // way.
    let bits = match h.image.output_bitdepth {
        OutputBitdepth::Bd8 => 8,
        OutputBitdepth::Bd16 => 16,
        _ => return Err(JxrError::Unsupported(JxrRefusal::FloatOrFixedPointFormat)),
    };
    let (mnemonic, channels, tail) = match (h.image.output_clr_fmt, bits) {
        (OutputClrFmt::YOnly, 8) => ("8bppGray", JxrChannels::Gray, 0x08),
        (OutputClrFmt::YOnly, 16) => ("16bppGray", JxrChannels::Gray, 0x0B),
        (OutputClrFmt::Rgb, 8) => ("24bppRGB", JxrChannels::Rgb, 0x0D),
        (OutputClrFmt::Rgb, 16) => ("48bppRGB", JxrChannels::Rgb, 0x15),
        _ => return Err(JxrError::Unsupported(JxrRefusal::UnsupportedColourFormat)),
    };
    Ok(JxrPixelFormat {
        mnemonic,
        channels,
        bits_per_component: bits,
        guid_tail: tail,
    })
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
