//! ITU-T T.832 Annex A: the tag-based file format that wraps a codestream.
//!
//! Annex A is TIFF's container shape — a four-byte file header, then image
//! file directories of twelve-byte entries — with its own magic and its own
//! tag set. It is *not* TIFF: A.5.2 fixes the byte order marker at `II` and
//! A.5.3 puts `0xBC` where TIFF puts 42, so a reader that accepted `MM` here
//! would be accepting a file the standard does not define.
//!
//! **What this module does not do is interpret pixels.** It answers three
//! questions: where the codestream is, how many bytes of it there are, and
//! which of Table A.6's pixel formats the file claims. Everything else in the
//! directory — EXIF, XMP, ICC, resolution, orientation — is walked and
//! ignored, which is what A.7.1 requires of a decoder for combinations it
//! does not recognise.

#![deny(clippy::float_arithmetic)]

use super::{JxrError, JxrWarning};

/// A.5.2 and A.5.3: `II`, then `0xBC`, then A.5.4's version.
pub(crate) const FILE_MAGIC: [u8; 3] = [0x49, 0x49, 0xBC];

/// A.5.4: the only defined `FILE_VERSION_ID`.
const FILE_VERSION: u8 = 1;

/// 8.3.2's `GDI_SIGNATURE`, which is what a bare codestream starts with.
pub(crate) const GDI_SIGNATURE: [u8; 8] = *b"WMPHOTO\0";

/// A.7.1 Table A.4's tags, in the subset this decoder reads.
mod tag {
    pub(super) const PIXEL_FORMAT: u16 = 0xBC01;
    pub(super) const SPATIAL_XFRM_PRIMARY: u16 = 0xBC02;
    pub(super) const IMAGE_WIDTH: u16 = 0xBC80;
    pub(super) const IMAGE_HEIGHT: u16 = 0xBC81;
    pub(super) const WIDTH_RESOLUTION: u16 = 0xBC82;
    pub(super) const HEIGHT_RESOLUTION: u16 = 0xBC83;
    pub(super) const IMAGE_OFFSET: u16 = 0xBCC0;
    pub(super) const IMAGE_BYTE_COUNT: u16 = 0xBCC1;
    pub(super) const ALPHA_OFFSET: u16 = 0xBCC2;
    pub(super) const ALPHA_BYTE_COUNT: u16 = 0xBCC3;
}

/// Table A.5's `SizeOfElement`, by `ELEMENT_TYPE`. `None` is the RESERVED
/// row, whose size the table leaves unspecified — an entry carrying one is
/// parsed and discarded rather than measured.
fn size_of_element(element_type: u16) -> Option<u32> {
    match element_type {
        1 | 2 | 6 | 7 => Some(1),
        3 | 8 => Some(2),
        4 | 9 | 11 => Some(4),
        5 | 10 | 12 => Some(8),
        _ => None,
    }
}

/// The colour interpretation a `PIXEL_FORMAT` GUID names, which is Table
/// A.6's "Colour" column. It is a *requirement on the codestream* rather than
/// a decode instruction: A.7.18 says `OUTPUT_CLR_FMT` shall equal this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JxrChannels {
    /// Table A.6 "YONLY": one channel.
    Gray,
    /// Three channels stored red-first.
    Rgb,
    /// Three channels stored blue-first, which is what most of the Windows
    /// stack's own formats are.
    Bgr,
    /// Four channels, blue-first, with alpha last.
    Bgra,
    /// Four channels, red-first, with alpha last.
    Rgba,
}

impl JxrChannels {
    /// Whether Table A.6's row carries an alpha channel, which is what
    /// decides whether A.3.2's separate alpha image plane has anywhere to go.
    #[must_use]
    pub const fn has_alpha(self) -> bool {
        matches!(self, Self::Bgra | Self::Rgba)
    }

    /// Samples per pixel in the decoded raster.
    #[must_use]
    pub const fn count(self) -> u8 {
        match self {
            Self::Gray => 1,
            Self::Rgb | Self::Bgr => 3,
            Self::Bgra | Self::Rgba => 4,
        }
    }
}

/// One row of Table A.6, for the formats this build decodes.
///
/// The table has 82 rows. This is the subset whose "Num" column is UINT and
/// whose "BPC" is BD8 or BD16 — the integer formats. Every other row is a
/// fixed-point, half-float or float representation whose output formatting
/// (9.10.7) this build refuses by name rather than approximates, and the
/// refusal carries the mnemonic so a reader can say which one it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JxrPixelFormat {
    /// Table A.6's "Mnemonic" column, for warnings and for the doc's
    /// fixture table.
    pub mnemonic: &'static str,
    /// Table A.6's "NC" column.
    pub channels: JxrChannels,
    /// 8 or 16, from Table A.6's "BPC" column read through Table 23.
    pub bits_per_component: u8,
    /// Table A.6's last GUID byte, which is what distinguishes the rows.
    pub guid_tail: u8,
}

/// A.7.18: the first fifteen bytes are shared by every row of Table A.6.
const GUID_PREFIX: [u8; 15] = [
    0x24, 0xC3, 0xDD, 0x6F, 0x03, 0x4E, 0xFE, 0x4B, 0xB1, 0x85, 0x3D, 0x77, 0x76, 0x8D, 0xC9,
];

/// The rows of Table A.6 this build decodes, transcribed from the table.
///
/// Written as the table rather than as a predicate over the GUID's low bits,
/// for the reason `png.rs`'s `COLOUR_TYPE_DEPTHS` is: the rows are not
/// regular, and a predicate that got one of them wrong would read exactly
/// like a correct one. `0x0F` is 32bppBGRA and `0x0E` is 32bppBGR — four
/// bytes per pixel either way, three channels in one and four in the other —
/// which is precisely the pair a "high nibble means alpha" rule gets wrong.
const PIXEL_FORMATS: &[JxrPixelFormat] = &[
    JxrPixelFormat {
        mnemonic: "8bppGray",
        channels: JxrChannels::Gray,
        bits_per_component: 8,
        guid_tail: 0x08,
    },
    JxrPixelFormat {
        mnemonic: "16bppGray",
        channels: JxrChannels::Gray,
        bits_per_component: 16,
        guid_tail: 0x0B,
    },
    JxrPixelFormat {
        mnemonic: "24bppBGR",
        channels: JxrChannels::Bgr,
        bits_per_component: 8,
        guid_tail: 0x0C,
    },
    JxrPixelFormat {
        mnemonic: "24bppRGB",
        channels: JxrChannels::Rgb,
        bits_per_component: 8,
        guid_tail: 0x0D,
    },
    // 32bppBGR is three channels in four bytes: A.7.18's note is that the
    // fourth byte is padding the decoder writes but does not decode. This
    // build returns the three channels and lets the caller pad, which is why
    // its `channels` is Bgr rather than a fourth variant.
    JxrPixelFormat {
        mnemonic: "32bppBGR",
        channels: JxrChannels::Bgr,
        bits_per_component: 8,
        guid_tail: 0x0E,
    },
    JxrPixelFormat {
        mnemonic: "32bppBGRA",
        channels: JxrChannels::Bgra,
        bits_per_component: 8,
        guid_tail: 0x0F,
    },
    JxrPixelFormat {
        mnemonic: "48bppRGB",
        channels: JxrChannels::Rgb,
        bits_per_component: 16,
        guid_tail: 0x15,
    },
    JxrPixelFormat {
        mnemonic: "64bppRGBA",
        channels: JxrChannels::Rgba,
        bits_per_component: 16,
        guid_tail: 0x16,
    },
];

/// Table A.6 by GUID. `None` for every row this build does not decode and for
/// every sixteen bytes that are not one of Table A.6's GUIDs at all — the
/// caller distinguishes the two, because "a CMYK JPEG XR" and "a file whose
/// pixel format field is corrupt" are different sentences.
#[must_use]
pub(crate) fn pixel_format_from_guid(guid: &[u8; 16]) -> Option<JxrPixelFormat> {
    if guid[..15] != GUID_PREFIX {
        return None;
    }
    let tail = guid[15];
    PIXEL_FORMATS.iter().copied().find(|f| f.guid_tail == tail)
}

/// Whether these sixteen bytes are one of Table A.6's GUIDs at all, decoded
/// by this build or not.
#[must_use]
pub(crate) fn is_table_a6_guid(guid: &[u8; 16]) -> bool {
    guid[..15] == GUID_PREFIX
}

/// What the container said, before a single codestream bit is read.
// Not `Eq`: `resolution` is a pair of floats, and a resolution read from two
// IEEE-754 fields has no total equality to offer. `PartialEq` is what the
// tests compare with and is all this ever needed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Container {
    /// Byte range of the primary `CODED_IMAGE( )`, already checked to lie
    /// inside the file.
    pub(crate) image: core::ops::Range<usize>,
    /// A.7.18's pixel format, when it is a row this build decodes.
    pub(crate) format: Option<JxrPixelFormat>,
    /// True when the GUID was one of Table A.6's but not a decoded row — a
    /// named refusal rather than a corrupt field.
    pub(crate) format_known_unsupported: bool,
    /// 0xBC80 and 0xBC81. Checked against the codestream's own
    /// `WIDTH_MINUS1`/`HEIGHT_MINUS1` by the caller, since A.7 makes them a
    /// second statement of the same fact and the two can disagree.
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// 0xBC02, Table 21's requested orientation. Reported, never applied:
    /// 8.3.8 calls it a *preferred* transformation subordinate to the
    /// application, and this crate has no application.
    pub(crate) spatial_transform: u8,
    /// Present when a *separate* alpha image plane is carried (A.3.2): a
    /// second, complete `CODED_IMAGE( )` that decodes to the alpha channel.
    pub(crate) alpha: Option<core::ops::Range<usize>>,
    /// 0xBC82 and 0xBC83, in pixels per inch.
    ///
    /// `None` when the file states neither, or states one without the other,
    /// or states something that is not a positive finite number. XPS 13.4.1
    /// makes 96 the default and that default is the *caller's* to apply —
    /// what an absent resolution means is a question about the document, not
    /// about the codec.
    pub(crate) resolution: Option<(f32, f32)>,
    pub(crate) warnings: Vec<JxrWarning>,
}

/// Reads a little-endian `u16` at `offset`, or `Truncated`.
fn le16(data: &[u8], offset: usize) -> Result<u16, JxrError> {
    let end = offset.checked_add(2).ok_or(JxrError::Truncated)?;
    let slice = data.get(offset..end).ok_or(JxrError::Truncated)?;
    // The slice is exactly two bytes long, so both index reads are in range.
    match slice {
        [a, b] => Ok(u16::from_le_bytes([*a, *b])),
        _ => Err(JxrError::Truncated),
    }
}

/// Reads a little-endian `u32` at `offset`, or `Truncated`.
fn le32(data: &[u8], offset: usize) -> Result<u32, JxrError> {
    let end = offset.checked_add(4).ok_or(JxrError::Truncated)?;
    let slice = data.get(offset..end).ok_or(JxrError::Truncated)?;
    match slice {
        [a, b, c, d] => Ok(u32::from_le_bytes([*a, *b, *c, *d])),
        _ => Err(JxrError::Truncated),
    }
}

/// One `IFD_ENTRY( )` (A.7), already resolved to its bytes.
struct Entry {
    tag: u16,
    element_type: u16,
    count: u32,
    /// The four bytes of `VALUES_OR_OFFSET` as written. Whether they are the
    /// value or an offset to it is A.7.5's rule, applied by `payload`.
    raw: [u8; 4],
}

impl Entry {
    /// A.7.5: when the payload fits in four bytes it is stored inline;
    /// otherwise `VALUES_OR_OFFSET` is a file offset to it.
    fn payload<'a>(&'a self, file: &'a [u8]) -> Option<&'a [u8]> {
        let size = size_of_element(self.element_type)?;
        let bytes = size.checked_mul(self.count)?;
        let bytes = usize::try_from(bytes).ok()?;
        if bytes <= 4 {
            self.raw.get(..bytes)
        } else {
            let start = usize::try_from(u32::from_le_bytes(self.raw)).ok()?;
            let end = start.checked_add(bytes)?;
            file.get(start..end)
        }
    }

    /// A single unsigned integer from a BYTE, USHORT or ULONG entry — the
    /// three types Table A.4 allows for every offset and dimension tag.
    /// Table A.4's `WIDTH_RESOLUTION` and `HEIGHT_RESOLUTION` are the only
    /// FLOAT entries this decoder reads, and they are metadata rather than
    /// samples: a resolution says how large to *draw* the picture, never what
    /// is in it. So this is the one float in the whole `jxr` directory, it is
    /// a reinterpretation of four bytes rather than any arithmetic, and
    /// ruling 4's ban on floats on the pixel path is untouched by it.
    ///
    /// `None` for anything that is not a single finite positive float —
    /// a zero, a negative, a NaN or an infinity is not a resolution, and the
    /// caller's default is better than a division by one of them.
    fn float_scalar(&self, file: &[u8]) -> Option<f32> {
        if self.count != 1 || self.element_type != 11 {
            return None;
        }
        let payload = self.payload(file)?;
        let [a, b, c, d] = payload else { return None };
        let value = f32::from_bits(u32::from_le_bytes([*a, *b, *c, *d]));
        (value.is_finite() && value > 0.0).then_some(value)
    }

    fn scalar(&self, file: &[u8]) -> Option<u32> {
        if self.count != 1 {
            return None;
        }
        let payload = self.payload(file)?;
        match (self.element_type, payload) {
            (1, [a]) => Some(u32::from(*a)),
            (3, [a, b]) => Some(u32::from(u16::from_le_bytes([*a, *b]))),
            (4, [a, b, c, d]) => Some(u32::from_le_bytes([*a, *b, *c, *d])),
            _ => None,
        }
    }
}

/// Walks the file header and the first image file directory.
///
/// # Errors
/// [`JxrError::NotJxr`] when the four-byte file header is not A.5's, and
/// [`JxrError::MissingRequiredTag`] when one of Table A.4's four Required
/// tags is absent — there is no image without them, as distinct from an
/// image that is damaged.
pub(crate) fn read(file: &[u8]) -> Result<Container, JxrError> {
    let mut warnings = Vec::new();

    if file.len() < 8 || file.get(..3) != Some(&FILE_MAGIC[..]) {
        return Err(JxrError::NotJxr);
    }
    // A.5.4: the version byte. A file claiming a version this build does not
    // know is refused rather than read as though it were version 1 — the
    // clause reserves other values precisely so that a later version may
    // change what follows.
    let version = file.get(3).copied().ok_or(JxrError::Truncated)?;
    if version != FILE_VERSION {
        return Err(JxrError::UnsupportedFileVersion(version));
    }

    let first_ifd = usize::try_from(le32(file, 4)?).map_err(|_| JxrError::Truncated)?;
    let num_entries = le16(file, first_ifd)?;
    if num_entries == 0 {
        // A.6.2: NUM_ENTRIES shall not be 0. A directory with no entries
        // cannot carry the four required tags, so this is the same refusal
        // by a shorter route.
        return Err(JxrError::MissingRequiredTag(tag::IMAGE_OFFSET));
    }

    let mut entries = Vec::new();
    let mut previous_tag: Option<u16> = None;
    for i in 0..u32::from(num_entries) {
        // 2 for NUM_ENTRIES, then twelve bytes an entry (Table A.3). The
        // arithmetic is checked because `first_ifd` is file-derived.
        let offset = (i as usize)
            .checked_mul(12)
            .and_then(|n| n.checked_add(2))
            .and_then(|n| n.checked_add(first_ifd))
            .ok_or(JxrError::Truncated)?;
        let tag = le16(file, offset)?;
        let element_type = le16(file, offset + 2)?;
        let count = le32(file, offset + 4)?;
        let raw_at = offset.checked_add(8).ok_or(JxrError::Truncated)?;
        let raw_end = raw_at.checked_add(4).ok_or(JxrError::Truncated)?;
        let raw_slice = file.get(raw_at..raw_end).ok_or(JxrError::Truncated)?;
        let raw = match raw_slice {
            [a, b, c, d] => [*a, *b, *c, *d],
            _ => return Err(JxrError::Truncated),
        };
        // A.7.2: tags are strictly ascending. A file that breaks this is
        // still readable — the entries are self-describing — so it is a
        // warning rather than a refusal (ruling 2).
        if previous_tag.is_some_and(|previous| tag <= previous)
            && !warnings.contains(&JxrWarning::IfdTagsOutOfOrder)
        {
            warnings.push(JxrWarning::IfdTagsOutOfOrder);
        }
        previous_tag = Some(tag);
        entries.push(Entry {
            tag,
            element_type,
            count,
            raw,
        });
    }

    let find = |wanted: u16| entries.iter().find(|e| e.tag == wanted);
    let required = |wanted: u16| -> Result<&Entry, JxrError> {
        find(wanted).ok_or(JxrError::MissingRequiredTag(wanted))
    };

    let width = required(tag::IMAGE_WIDTH)?
        .scalar(file)
        .ok_or(JxrError::MissingRequiredTag(tag::IMAGE_WIDTH))?;
    let height = required(tag::IMAGE_HEIGHT)?
        .scalar(file)
        .ok_or(JxrError::MissingRequiredTag(tag::IMAGE_HEIGHT))?;
    let image_offset = required(tag::IMAGE_OFFSET)?
        .scalar(file)
        .ok_or(JxrError::MissingRequiredTag(tag::IMAGE_OFFSET))?;
    let image_bytes = required(tag::IMAGE_BYTE_COUNT)?
        .scalar(file)
        .ok_or(JxrError::MissingRequiredTag(tag::IMAGE_BYTE_COUNT))?;

    let start = usize::try_from(image_offset).map_err(|_| JxrError::Truncated)?;
    let len = usize::try_from(image_bytes).map_err(|_| JxrError::Truncated)?;
    let end = start.checked_add(len).ok_or(JxrError::Truncated)?;
    if end > file.len() {
        return Err(JxrError::Truncated);
    }

    // A.7.18. A missing PIXEL_FORMAT is a missing Required tag; a present one
    // this build cannot decode is a different sentence, and the caller says
    // so by name.
    let pf = required(tag::PIXEL_FORMAT)?;
    let mut format = None;
    let mut format_known_unsupported = false;
    if pf.element_type == 1 && pf.count == 16 {
        if let Some(bytes) = pf.payload(file) {
            let mut guid = [0u8; 16];
            if bytes.len() == 16 {
                guid.copy_from_slice(bytes);
                format = pixel_format_from_guid(&guid);
                format_known_unsupported = format.is_none() && is_table_a6_guid(&guid);
            }
        }
    }
    if format.is_none() && !format_known_unsupported {
        return Err(JxrError::MissingRequiredTag(tag::PIXEL_FORMAT));
    }

    let spatial_transform = find(tag::SPATIAL_XFRM_PRIMARY)
        .and_then(|e| e.scalar(file))
        // Table 21 has eight rows; anything else is out of the field's range
        // and is ignored rather than applied.
        .filter(|&v| v < 8)
        .unwrap_or(0);
    // The cast is exact: the filter above bounds the value below 8.
    let spatial_transform = spatial_transform as u8;

    let alpha = match (
        find(tag::ALPHA_OFFSET).and_then(|e| e.scalar(file)),
        find(tag::ALPHA_BYTE_COUNT).and_then(|e| e.scalar(file)),
    ) {
        (Some(o), Some(n)) => {
            let s = usize::try_from(o).map_err(|_| JxrError::Truncated)?;
            let l = usize::try_from(n).map_err(|_| JxrError::Truncated)?;
            let e = s.checked_add(l).ok_or(JxrError::Truncated)?;
            if e <= file.len() {
                Some(s..e)
            } else {
                None
            }
        }
        _ => None,
    };

    // Both or neither: a file that states one axis has not stated a
    // resolution, and pairing a stated axis with an assumed one draws the
    // picture at the wrong aspect ratio rather than the wrong size.
    let resolution = match (
        find(tag::WIDTH_RESOLUTION).and_then(|e| e.float_scalar(file)),
        find(tag::HEIGHT_RESOLUTION).and_then(|e| e.float_scalar(file)),
    ) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    };

    Ok(Container {
        image: start..end,
        format,
        format_known_unsupported,
        width,
        height,
        spatial_transform,
        alpha,
        resolution,
        warnings,
    })
}
