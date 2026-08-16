//! The JP2 file format (ITU-T T.800 Annex I) — the box structure around a
//! codestream.
//!
//! PDF permits both shapes for a `/JPXDecode` stream (ISO 32000-1 7.4.9): a
//! whole JP2 file, and a bare J2K codestream with no boxes at all. The two
//! are told apart by their first bytes and nothing else — a JP2 opens with
//! the twelve-byte `jP  ` signature box, a bare codestream opens with SOC
//! immediately followed by SIZ.
//!
//! # What a box is
//!
//! `LBox` (4 bytes, the whole box's length including these eight), then
//! `TBox` (4 bytes, the type). `LBox == 1` means a 64-bit `XLBox` follows and
//! the length is that instead; `LBox == 0` means the box runs to the end of
//! the file. Every other value below 8 is a length that cannot include its
//! own header, and it is refused rather than clamped: a zero-length box in a
//! walk that clamps is an infinite loop, which is exactly the shape ruling 1
//! is about.
//!
//! # What is read here and what is deferred
//!
//! `ihdr` fixes the image geometry and `bpcc` the per-component precision
//! when `ihdr` says they differ, and both are read because the codestream's
//! own SIZ must agree with them or the file is inconsistent. `colr` is read
//! for the colour space. `pclr`, `cmap` and `cdef` are read in full and
//! applied by [`super::colour`]: a palette turns one component of indices
//! into three or four channels, `cmap` says which component each channel
//! comes from and through which palette column, and `cdef` names the one
//! that is opacity rather than colour.
//!
//! Reading them is not optional in the way skipping an unknown box is.
//! Ignoring a `pclr` renders a palette image's *indices* as grey levels,
//! which is a picture — a wrong one, plausible enough that nothing
//! downstream can tell — and that is the failure this whole decoder is
//! shaped around.

use super::{Cursor, JpxColour, Refusal};

/// Box types, as the four-character codes T.800 Annex I gives them.
mod ty {
    pub const HEADER: u32 = 0x6A70_3268; // "jp2h"
    pub const IMAGE_HEADER: u32 = 0x6968_6472; // "ihdr"
    pub const BITS_PER_COMPONENT: u32 = 0x6270_6363; // "bpcc"
    pub const COLOUR: u32 = 0x636F_6C72; // "colr"
    pub const PALETTE: u32 = 0x7063_6C72; // "pclr"
    pub const COMPONENT_MAP: u32 = 0x636D_6170; // "cmap"
    pub const CHANNEL_DEFINITION: u32 = 0x6364_6566; // "cdef"
    pub const RESOLUTION: u32 = 0x7265_7320; // "res "
    pub const CODESTREAM: u32 = 0x6A70_3263; // "jp2c"
}

/// The twelve bytes a JP2 file opens with: length 12, type `jP  `, contents
/// `0D0A870A` (T.800 I.5.1). The contents are a line-ending check, the same
/// trick PNG's signature uses.
const SIGNATURE: [u8; 12] = [
    0x00, 0x00, 0x00, 0x0C, 0x6A, 0x50, 0x20, 0x20, 0x0D, 0x0A, 0x87, 0x0A,
];

/// SOC followed by SIZ — the first four bytes of every codestream (A.4.1).
const SOC_SIZ: [u8; 4] = [0xFF, 0x4F, 0xFF, 0x51];

/// Boxes one file may hold, over both levels. A *total*, spent and never
/// refunded across the file box list and `jp2h`'s children together.
///
/// There is no depth cap here because there is no recursion to cap: this
/// walker descends into exactly one superbox, `jp2h`, and reads none of the
/// superboxes inside it. JPX's `asoc` association trees, which do nest to
/// any depth, are a non-goal and their contents are never entered. A cap on
/// a depth that is fixed at two would be theatre — `5adf502`'s lesson said
/// backwards.
const MAX_BOXES: u32 = 4096;

/// The `colr` enumerated colour spaces T.800 Table I.10 assigns, of the ones
/// this build can name.
mod enumcs {
    pub const CMYK: u32 = 12;
    pub const SRGB: u32 = 16;
    pub const GREYSCALE: u32 = 17;
    pub const SYCC: u32 = 18;
    pub const E_YCC: u32 = 24;
}

/// What the container said, and where the codestream is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Container<'a> {
    /// The `jp2c` contents, or the whole input for a bare codestream.
    pub(crate) codestream: &'a [u8],
    /// `None` for a bare codestream, which carries no header boxes at all.
    pub(crate) header: Option<Jp2Header>,
}

/// The `jp2h` superbox's contents, as far as this build reads them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Jp2Header {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) components: u16,
    /// `ihdr`'s BPC, decoded: `(precision, signed)`. `None` when BPC was 255,
    /// which means the components differ and `bpcc` carries them.
    pub(crate) bpc: Option<(u8, bool)>,
    /// `bpcc`, decoded the same way, when it was present.
    pub(crate) bpcc: Vec<(u8, bool)>,
    pub(crate) colour: JpxColour,
    /// `pclr` (I.5.3.4), when the file carried one.
    pub(crate) palette: Option<Palette>,
    /// `cmap` (I.5.3.5), one entry per output channel, in channel order.
    pub(crate) component_map: Vec<ChannelMap>,
    /// `cdef` (I.5.3.6), whatever order the file wrote them in.
    pub(crate) channel_definition: Vec<ChannelDef>,
}

/// `pclr` (I.5.3.4): a lookup table from one component's sample value to
/// several channels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Palette {
    /// `Bi` per generated channel, decoded as `(precision, signed)`.
    pub(crate) channels: Vec<(u8, bool)>,
    /// `NE` rows of `NPC` entries, held column-major so a channel's whole
    /// column is contiguous: `columns[j][i]` is entry `i` of channel `j`.
    pub(crate) columns: Vec<Vec<i32>>,
}

/// One row of `cmap` (I.5.3.5): where one output channel's samples come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChannelMap {
    /// `CMP`, the codestream component this channel reads.
    pub(crate) component: u16,
    /// `PCOL`, the palette column, when `MTYP` was 1. `None` is `MTYP` 0 —
    /// the component's samples used directly.
    pub(crate) column: Option<u8>,
}

/// One row of `cdef` (I.5.3.6): what a channel *is*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ChannelDef {
    /// `Cn`, the channel this row describes.
    pub(crate) channel: u16,
    /// `Typ`: 0 colour, 1 opacity, 2 premultiplied opacity, 65535
    /// unspecified.
    pub(crate) kind: u16,
    /// `Asoc`: 0 the whole image, 1..n colour channel n, 65535 none.
    pub(crate) association: u16,
}

/// Palette entries, T.800 I.5.3.4's own bound (`NE` is 1 to 1024).
const MAX_PALETTE_ENTRIES: u32 = 1024;

/// Channels one `cmap` or `cdef` may describe.
///
/// **Not a work cap** — there is no work here to cap, only allocation, and
/// [`super::MAX_JPX_COMPONENTS`] already bounds what a channel can point at.
/// This bounds the list itself, which `cmap` sizes from its own box length
/// and `cdef` from a 16-bit count it writes for itself.
const MAX_CHANNELS: usize = 256;

/// Splits a `/JPXDecode` stream into its codestream and, if it had one, its
/// JP2 header.
pub(crate) fn parse(input: &[u8]) -> Result<Container<'_>, Refusal> {
    if input.starts_with(&SOC_SIZ) {
        // A bare codestream. PDF permits it and OpenJPEG writes it for a
        // `.j2k` output, so it is not a corner case.
        return Ok(Container {
            codestream: input,
            header: None,
        });
    }
    if !input.starts_with(&SIGNATURE) {
        return Err(Refusal::Structure(
            "neither a JP2 signature box nor an SOC/SIZ codestream",
        ));
    }

    let mut header = None;
    let mut codestream = None;
    let mut budget = MAX_BOXES;
    for (kind, body) in level(input, &mut budget)? {
        match kind {
            // The first `jp2h` wins. A second is a file saying two different
            // things about its own geometry, and taking the later one would
            // be a guess.
            ty::HEADER if header.is_none() => header = Some(jp2_header(body, &mut budget)?),
            ty::CODESTREAM if codestream.is_none() => codestream = Some(body),
            _ => {}
        }
    }

    let Some(codestream) = codestream else {
        return Err(Refusal::Structure("no jp2c contiguous codestream box"));
    };
    if !codestream.starts_with(&SOC_SIZ) {
        return Err(Refusal::Structure("jp2c does not begin with SOC then SIZ"));
    }
    Ok(Container { codestream, header })
}

/// One level of boxes: each one's type and its *contents*.
///
/// Flat rather than recursive on purpose. Exactly two levels are ever read —
/// the file, and `jp2h` — so a recursive walker would be a general mechanism
/// built for a fixed depth, and a general mechanism over attacker-controlled
/// lengths is a thing that needs its own bound. `budget` is shared across
/// both calls so the count is a total.
fn level<'a>(data: &'a [u8], budget: &mut u32) -> Result<Vec<(u32, &'a [u8])>, Refusal> {
    let mut cursor = Cursor::new(data);
    let mut out = Vec::new();
    while !cursor.is_empty() {
        *budget = budget
            .checked_sub(1)
            .ok_or(Refusal::Budget("JP2 boxes in one file"))?;
        let (Some(lbox), Some(tbox)) = (cursor.u32(), cursor.u32()) else {
            return Err(Refusal::Truncated("a JP2 box header"));
        };
        // I.4: 1 means a 64-bit length follows the type; 0 means the box runs
        // to the end of the file. Every other value is the whole length
        // *including* the eight bytes just read, so anything below 8 (or 16
        // with an XLBox) describes a box shorter than its own header — and a
        // walk that clamped that to zero would never advance, which is a hang
        // rather than a wrong answer.
        let body_len = match lbox {
            1 => {
                let xl = cursor.u64().ok_or(Refusal::Truncated("a JP2 XLBox"))?;
                let n = xl
                    .checked_sub(16)
                    .ok_or(Refusal::Structure("a JP2 XLBox shorter than its header"))?;
                usize::try_from(n).map_err(|_| Refusal::Structure("a JP2 box past addressable"))?
            }
            0 => cursor.remaining(),
            n => usize::try_from(n)
                .ok()
                .and_then(|n: usize| n.checked_sub(8))
                .ok_or(Refusal::Structure("a JP2 box shorter than its header"))?,
        };
        let bytes = cursor
            .take(body_len)
            .ok_or(Refusal::Truncated("a JP2 box body"))?;
        out.push((tbox, bytes));
    }
    Ok(out)
}

/// `jp2h`'s children (I.5.3).
fn jp2_header(body: &[u8], budget: &mut u32) -> Result<Jp2Header, Refusal> {
    let mut header = Jp2Header::default();
    let mut seen_ihdr = false;
    for (kind, bytes) in level(body, budget)? {
        match kind {
            ty::IMAGE_HEADER if !seen_ihdr => {
                seen_ihdr = true;
                image_header(bytes, &mut header)?;
            }
            ty::BITS_PER_COMPONENT => {
                header.bpcc = bytes.iter().map(|&b| decode_bpc(b)).collect();
            }
            // The first `colr` wins. I.5.3.3 permits several so a reader can
            // pick the best it understands; taking the first this build can
            // name is the ruling 2 shape.
            ty::COLOUR if header.colour == JpxColour::Unstated => {
                header.colour = colour(bytes)?;
            }
            // The first of each wins, for the reason `colr` and `ihdr` do: a
            // file carrying two palettes is saying two different things about
            // its own pixels, and taking the later one would be a guess.
            ty::PALETTE if header.palette.is_none() => header.palette = Some(palette(bytes)?),
            ty::COMPONENT_MAP if header.component_map.is_empty() => {
                header.component_map = component_map(bytes)?;
            }
            ty::CHANNEL_DEFINITION if header.channel_definition.is_empty() => {
                header.channel_definition = channel_definition(bytes)?;
            }
            // I.5.3.7. A display resolution changes nothing about the
            // samples, and PDF sizes an image from `/Width`, `/Height` and
            // the CTM rather than from this.
            ty::RESOLUTION => {}
            _ => {}
        }
    }
    if !seen_ihdr {
        return Err(Refusal::Structure("jp2h with no ihdr image header box"));
    }
    Ok(header)
}

/// `ihdr` (I.5.3.1): height, width, component count, precision, compression.
fn image_header(bytes: &[u8], header: &mut Jp2Header) -> Result<(), Refusal> {
    let mut c = Cursor::new(bytes);
    let (Some(height), Some(width), Some(nc), Some(bpc), Some(compression)) =
        (c.u32(), c.u32(), c.u16(), c.u8(), c.u8())
    else {
        return Err(Refusal::Truncated("an ihdr image header box"));
    };
    // I.5.3.1: C is 7 and nothing else. A JP2 file whose compression type is
    // not JPEG 2000 is a JP2 file this crate has no business decoding, and
    // guessing that it meant 7 is how a decoder ends up parsing a codestream
    // that is not one.
    if compression != 7 {
        return Err(Refusal::Structure("ihdr compression type is not 7"));
    }
    if width == 0 || height == 0 {
        return Err(Refusal::Structure("ihdr declares a zero dimension"));
    }
    if nc == 0 {
        return Err(Refusal::Structure("ihdr declares no components"));
    }
    header.width = width;
    header.height = height;
    header.components = nc;
    // 255 means "the components differ, read bpcc".
    header.bpc = (bpc != 255).then(|| decode_bpc(bpc));
    Ok(())
}

/// A BPC or BPCC byte: seven bits of `precision - 1`, and a sign bit.
const fn decode_bpc(b: u8) -> (u8, bool) {
    ((b & 0x7F) + 1, b & 0x80 != 0)
}

/// `pclr` (I.5.3.4): `NE` entries of `NPC` channels, each channel carrying
/// its own bit depth.
///
/// The entry width is `ceil(B/8)` bytes and it is **per channel**, so a
/// palette may hold an 8-bit red beside a 16-bit alpha and the rows are not
/// uniformly sized. Reading it with one width for the whole table is the
/// mistake that produces a palette shifted by a byte, which is a picture in
/// wrong colours rather than an error.
fn palette(bytes: &[u8]) -> Result<Palette, Refusal> {
    let mut c = Cursor::new(bytes);
    let (Some(ne), Some(npc)) = (c.u16(), c.u8()) else {
        return Err(Refusal::Truncated("a pclr palette box"));
    };
    if ne == 0 || u32::from(ne) > MAX_PALETTE_ENTRIES {
        return Err(Refusal::Structure(
            "a pclr entry count outside I.5.3.4's 1 to 1024",
        ));
    }
    if npc == 0 {
        return Err(Refusal::Structure("a pclr palette generating no channels"));
    }
    let mut channels = Vec::with_capacity(usize::from(npc));
    for _ in 0..npc {
        let b = c.u8().ok_or(Refusal::Truncated("a pclr channel depth"))?;
        let (precision, signed) = decode_bpc(b);
        // The same ceiling the codestream's own components get, and for the
        // same reason: PDF's sample path reads at most 16 bits and this build
        // refuses past it rather than truncating.
        if precision > 16 {
            return Err(Refusal::Precision(precision));
        }
        channels.push((precision, signed));
    }

    let mut columns = vec![Vec::with_capacity(usize::from(ne)); usize::from(npc)];
    for _ in 0..ne {
        for (j, &(precision, signed)) in channels.iter().enumerate() {
            let width = usize::from(precision).div_ceil(8);
            let raw = c
                .take(width)
                .ok_or(Refusal::Truncated("a pclr palette entry"))?;
            let mut v: i32 = 0;
            for &byte in raw {
                v = (v << 8) | i32::from(byte);
            }
            if signed {
                // Sign-extend from the declared precision, which is what
                // makes a signed palette channel mean what I.5.3.4 says
                // rather than a large positive number.
                let sign = 1i32 << (i32::from(precision) - 1);
                if v & sign != 0 {
                    v -= sign << 1;
                }
            }
            columns[j].push(v);
        }
    }
    Ok(Palette { channels, columns })
}

/// `cmap` (I.5.3.5): four bytes per output channel, and the box length is the
/// only thing that says how many there are.
fn component_map(bytes: &[u8]) -> Result<Vec<ChannelMap>, Refusal> {
    if bytes.len() % 4 != 0 {
        return Err(Refusal::Structure(
            "a cmap box that is not a whole number of channels",
        ));
    }
    if bytes.len() / 4 > MAX_CHANNELS {
        return Err(Refusal::Budget("cmap channels"));
    }
    let mut c = Cursor::new(bytes);
    let mut out = Vec::with_capacity(bytes.len() / 4);
    while !c.is_empty() {
        let (Some(component), Some(mtyp), Some(pcol)) = (c.u16(), c.u8(), c.u8()) else {
            return Err(Refusal::Truncated("a cmap channel mapping"));
        };
        let column = match mtyp {
            0 => None,
            1 => Some(pcol),
            // I.5.3.5 defines 0 and 1 and nothing else. A third value is a
            // mapping this build would have to invent, and inventing one
            // renders the picture in a way nobody can check.
            _ => return Err(Refusal::Feature("a cmap MTYP I.5.3.5 does not define")),
        };
        out.push(ChannelMap { component, column });
    }
    Ok(out)
}

/// `cdef` (I.5.3.6): a count, then three 16-bit fields per channel.
fn channel_definition(bytes: &[u8]) -> Result<Vec<ChannelDef>, Refusal> {
    let mut c = Cursor::new(bytes);
    let n = c.u16().ok_or(Refusal::Truncated("a cdef channel count"))?;
    if usize::from(n) > MAX_CHANNELS {
        return Err(Refusal::Budget("cdef channels"));
    }
    let mut out = Vec::with_capacity(usize::from(n));
    for _ in 0..n {
        let (Some(channel), Some(kind), Some(association)) = (c.u16(), c.u16(), c.u16()) else {
            return Err(Refusal::Truncated("a cdef channel definition"));
        };
        out.push(ChannelDef {
            channel,
            kind,
            association,
        });
    }
    Ok(out)
}

/// `colr` (I.5.3.3): the colour specification.
fn colour(bytes: &[u8]) -> Result<JpxColour, Refusal> {
    let mut c = Cursor::new(bytes);
    let (Some(meth), Some(_prec), Some(_approx)) = (c.u8(), c.u8(), c.u8()) else {
        return Err(Refusal::Truncated("a colr colour specification box"));
    };
    match meth {
        1 => {
            let cs = c.u32().ok_or(Refusal::Truncated("a colr EnumCS"))?;
            match cs {
                enumcs::SRGB => Ok(JpxColour::Srgb),
                enumcs::GREYSCALE => Ok(JpxColour::Greyscale),
                enumcs::SYCC => Ok(JpxColour::Sycc),
                enumcs::E_YCC => Ok(JpxColour::EYcc),
                enumcs::CMYK => Ok(JpxColour::Cmyk),
                // Ruling 2: reported, not guessed at. An enumerated space
                // this build cannot map rendered as sRGB is a picture in the
                // wrong colours that reads as a colour-management problem.
                _ => Err(Refusal::Feature("a colr EnumCS this build cannot map")),
            }
        }
        // Method 2 is an embedded ICC profile. It is named rather than
        // mapped: conversion is `tinker-pdf-color`'s business and 15444-1
        // does not change that.
        2 => Ok(JpxColour::IccProfile),
        _ => Err(Refusal::Feature("a colr method this build cannot map")),
    }
}
