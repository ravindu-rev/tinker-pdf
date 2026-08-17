//! The colour pipeline: T.800 Annex G's two component transforms, and Annex
//! I's palette and channel definitions.
//!
//! This is the last stage between a coefficient and a picture a PDF can
//! carry, and it is four separate things that share only their position in
//! the pipeline:
//!
//! - **G.2.1's inverse RCT**, the reversible 5/3's partner. Exact integer
//!   arithmetic, so it extends the plan's gate 1 rather than needing a gate
//!   of its own: a losslessly coded RGB stream must come back byte-identical
//!   to `opj_decompress`'s decode of it, and a decoder that has the two
//!   chrominance terms the wrong way round cannot pass that.
//! - **G.2.2's inverse ICT**, the irreversible 9/7's partner, and a *float*
//!   transform in the standard exactly as the 9/7 was. It gets the same
//!   fixed-point treatment and the **same format** — the plane's Q12, the
//!   constants' Q24, and [`super::wavelet::mul_q24`] itself rather than a
//!   second rounding written here. Inventing a second fixed-point width is
//!   the failure the plan was written to prevent and it would put half the
//!   pixel path outside the error analysis that justifies the other half.
//! - **G.1's DC level shift**, which is already written — it lives beside the
//!   wavelet as [`super::wavelet::level_shift`] because that is the last
//!   thing that touches a plane. What matters here is *when* it runs: after
//!   the component transform, never before. G.2 operates on the signed
//!   samples the wavelet produced, and shifting first offsets Y, Cb and Cr
//!   alike, which the RCT's `Y0 - floor((Y1 + Y2) / 4)` then turns into a
//!   half-shifted green and two fully-shifted chrominances. The picture that
//!   comes out has a colour cast and nothing else wrong with it.
//! - **Subsampling, by replication.** A component with `XRsiz` or `YRsiz`
//!   above one covers the reference grid in blocks, and each block takes the
//!   sample's value unfiltered. That is the opposite of what
//!   `docs/plans/02-filters.md` decided for JPEG, deliberately and for the
//!   same underlying reason: there the oracle is libjpeg-turbo, which does
//!   fancy upsampling, and here it is `opj_decompress -upsample`, which
//!   replicates. Gate 1's byte-identity is only reachable if the two agree.
//!
//! # Why the palette is not optional
//!
//! A `pclr` box turns one component of *indices* into three or four channels
//! of colour, and a decoder that ignores it renders the indices as grey
//! levels. That is a picture — it has structure, it has contrast, and it is
//! completely wrong — which is the failure mode this whole decoder is shaped
//! around. So `pclr`, `cmap` and `cdef` are read in [`super::boxes`] and
//! applied here, and every combination this build cannot interpret is refused
//! by name rather than approximated.

use super::boxes::{Jp2Header, Palette};
use super::codestream::Component;
use super::wavelet::{mul_q24, Arith, Plane};
use super::Refusal;

// --- G.2.2's constants ----------------------------------------------------

/// The inverse ICT's three multipliers, at the **same Q24** the 9/7's are.
///
/// T.800 G.2.2 gives them as decimals, and they are transcribed here as
/// integers and *recomputed from those decimals* by `tests::colour`, the way
/// Table F.4's six are. That test is not ceremony: milestone 5 injected one
/// wavelet constant carried at Q16 and shifted up, a relative error of one
/// part in a million, and **both comparison gates passed it** — only the
/// recomputation failed. The same hole exists here and it is the same size.
///
/// `round(1.402 * 2^24)`, the red channel's chrominance term.
pub(crate) const ICT_R_CR: i64 = 23_521_657;
/// `round(0.344136 * 2^24)`, subtracted from green.
pub(crate) const ICT_G_CB: i64 = 5_773_644;
/// `round(0.714136 * 2^24)`, also subtracted from green.
pub(crate) const ICT_G_CR: i64 = 11_981_214;
/// `round(1.772 * 2^24)`, the blue channel's chrominance term.
pub(crate) const ICT_B_CB: i64 = 29_729_227;

/// G.2.1's inverse reversible component transform, in exact integers.
///
/// The forward transform is `Y1 = I2 - I1` and `Y2 = I0 - I1` — **blue minus
/// green in the first chrominance and red minus green in the second** — so
/// the inverse adds `Y2` back to make red and `Y1` back to make blue. Getting
/// those two the wrong way round swaps red and blue in every pixel whose
/// chrominances differ, which on a photograph is a picture that looks like a
/// channel-order bug in something else entirely.
///
/// `i64` throughout: `Y1 + Y2` is two plane entries added, and a plane entry
/// is only bounded by `i32` on the reversible path.
pub(crate) fn inverse_rct(y0: i64, y1: i64, y2: i64) -> (i64, i64, i64) {
    // `div_euclid` rather than `>> 2` for the same reason F.3.8.1 uses it:
    // the standard writes a floor and only one of Rust's two spellings is
    // one for negative values. (They agree here; the spelling is the
    // documentation.)
    let green = y0 - (y1 + y2).div_euclid(4);
    (y2 + green, green, y1 + green)
}

/// G.2.2's inverse irreversible component transform, over a Q12 line.
///
/// Every product goes through [`mul_q24`], which is the 9/7's own rounding —
/// `(p + (1 << 23)) >> 24`, round-half-up on an arithmetic shift, identical
/// on every target where `lrintf` is not (ruling 4). The bound holds with
/// room: a plane entry is at most 2^30 and the largest constant here is
/// 1.772 at Q24, so the product tops out at 2^54.8 against the 2^60 the
/// wavelet's `debug_assert` allows.
pub(crate) fn inverse_ict(y0: i64, y1: i64, y2: i64) -> (i64, i64, i64) {
    let red = y0 + mul_q24(y2, ICT_R_CR);
    let green = y0 - mul_q24(y1, ICT_G_CB) - mul_q24(y2, ICT_G_CR);
    let blue = y0 + mul_q24(y1, ICT_B_CB);
    (red, green, blue)
}

/// Apply the inverse multiple component transform across three planes.
///
/// Which of the two runs is [`Arith`]'s business, not this function's, for
/// the reason the wavelet's two arithmetics share one ladder: the geometry is
/// the same and a second copy of it is a second thing to get wrong. The
/// pairing is not a choice either — A.6.1 ties the RCT to the reversible 5/3
/// and the ICT to the irreversible 9/7, so the arithmetic already decided.
pub(crate) fn inverse_mct<A: Arith>(planes: &mut [Plane<A>]) -> Result<(), Refusal> {
    // A.6.1's SGcod names one transform for the tile, and G.2 defines it over
    // three components. Fewer than three is a codestream asking for a
    // transform of something it has not got.
    let [p0, p1, p2] = &planes[..3] else {
        return Err(Refusal::Structure(
            "a multiple component transform on fewer than three components",
        ));
    };
    // G.2 transforms three samples of the *same* pixel, so three components
    // at different subsampling factors have no such pixel. A stream that
    // signals both is refused rather than resolved by resampling one of them
    // first, which would be this decoder inventing a picture.
    let shape = (p0.width, p0.height);
    if shape != (p1.width, p1.height) || shape != (p2.width, p2.height) {
        return Err(Refusal::Structure(
            "a multiple component transform over components of different sizes",
        ));
    }
    for i in 0..planes[0].samples.len() {
        let (a, b, c) = A::inverse_mct(
            planes[0].samples[i],
            planes[1].samples[i],
            planes[2].samples[i],
        );
        planes[0].samples[i] = a;
        planes[1].samples[i] = b;
        planes[2].samples[i] = c;
    }
    Ok(())
}

// --- Annex I's channels ---------------------------------------------------

/// One output channel: where its samples come from and what they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Channel {
    /// The codestream component this channel reads.
    pub(crate) component: usize,
    /// The palette column its samples are looked up through, when `cmap`
    /// said `MTYP` 1.
    pub(crate) column: Option<usize>,
    /// Bits per sample, from the palette column when there is one and from
    /// SIZ otherwise.
    pub(crate) precision: u8,
    /// Whether those bits are two's complement, from the same place.
    ///
    /// T.800 G.1 shifts an *unsigned* component into range and leaves a
    /// signed one centred on zero, so a signed channel arrives here still
    /// negative for half its values. PDF's sample path has no signed samples
    /// at all, so the shift the level shift did not do is done at the output
    /// boundary instead -- see `jpx::normalise`.
    pub(crate) signed: bool,
}

/// What the container says the decoded components add up to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    /// The colour channels, in the order `cdef`'s associations put them.
    pub(crate) colour: Vec<Channel>,
    /// The one channel `cdef` typed as opacity, if there was one.
    pub(crate) opacity: Option<Channel>,
    /// `cdef` type 2 rather than 1: the colour channels have already been
    /// multiplied by this one. **Un-multiplying them is not done here** —
    /// ISO 32000-1 8.9.5.4 makes it `/SMaskInData` 2's rule and it belongs
    /// beside `/ColorSpace`, which is the split ruling 8 asks for.
    pub(crate) premultiplied: bool,
    /// Every channel's precision, which they must share.
    pub(crate) precision: u8,
}

/// I.5.3.4's `NE` bound, as an index ceiling.
const MAX_PALETTE_INDEX: i32 = 1023;

/// Work out the output channels from the container's boxes and the
/// codestream's components.
///
/// With no `cmap` the channels *are* the components, in order, which is the
/// overwhelmingly common case and the one every fixture in the oracle test
/// exercises. With one, each channel names a component and optionally a
/// palette column.
pub(crate) fn plan(header: Option<&Jp2Header>, components: &[Component]) -> Result<Plan, Refusal> {
    let (map, palette, defs) = match header {
        Some(h) => (
            h.component_map.as_slice(),
            h.palette.as_ref(),
            h.channel_definition.as_slice(),
        ),
        None => (&[][..], None, &[][..]),
    };

    // I.5.3.4: a palette exists to be pointed at. One with no `cmap` describes
    // a mapping the file never applies, and guessing that it meant component
    // zero is exactly the guess ruling 2 forbids.
    if palette.is_some() && map.is_empty() {
        return Err(Refusal::Structure(
            "a pclr palette with no cmap to apply it",
        ));
    }

    let mut channels: Vec<Channel> = Vec::new();
    if map.is_empty() {
        for (i, c) in components.iter().enumerate() {
            channels.push(Channel {
                component: i,
                column: None,
                precision: c.precision,
                signed: c.signed,
            });
        }
    } else {
        for entry in map {
            let component = usize::from(entry.component);
            let source = components.get(component).ok_or(Refusal::Structure(
                "a cmap channel naming no such component",
            ))?;
            let (column, precision, signed) = match entry.column {
                None => (None, source.precision, source.signed),
                Some(col) => {
                    let palette = palette.ok_or(Refusal::Structure(
                        "a cmap palette mapping with no pclr box",
                    ))?;
                    let col = usize::from(col);
                    let &(precision, signed) = palette.channels.get(col).ok_or(
                        Refusal::Structure("a cmap PCOL past the palette's channels"),
                    )?;
                    (Some(col), precision, signed)
                }
            };
            channels.push(Channel {
                component,
                column,
                precision,
                signed,
            });
        }
    }
    if channels.is_empty() {
        return Err(Refusal::Structure("an image with no output channels"));
    }

    // I.5.3.6: `cdef` types a channel and associates it with a colour. A type
    // outside 0, 1 and 2 has no meaning this build can act on, and 65535
    // ("unspecified") least of all -- a channel nobody can name is not a
    // channel that may be rendered as though it were colour.
    let mut opacity: Option<usize> = None;
    let mut premultiplied = false;
    let mut association = vec![0u16; channels.len()];
    for def in defs {
        let index = usize::from(def.channel);
        if index >= channels.len() {
            return Err(Refusal::Structure("a cdef naming no such channel"));
        }
        match def.kind {
            0 => association[index] = def.association,
            1 | 2 => {
                if opacity.is_some() {
                    return Err(Refusal::Feature("more than one cdef opacity channel"));
                }
                opacity = Some(index);
                premultiplied = def.kind == 2;
            }
            _ => {
                return Err(Refusal::Feature(
                    "a cdef channel type this build cannot map",
                ))
            }
        }
    }

    let opacity_channel = opacity.map(|i| channels[i]);
    let mut colour: Vec<(u16, Channel)> = channels
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != opacity)
        .map(|(i, c)| (association[i], *c))
        .collect();
    // I.5.3.6's `Asoc` is 1-based and says which colour this channel *is*, so
    // a file may declare them out of order. Reordering is only safe when the
    // associations are a permutation of 1..=n: anything else (all zero, which
    // is what a file without `cdef` leaves, or a partial set) means the
    // declared order is the only order there is.
    let mut seen: Vec<u16> = colour.iter().map(|(a, _)| *a).collect();
    seen.sort_unstable();
    if seen == (1..=colour.len() as u16).collect::<Vec<_>>() {
        colour.sort_by_key(|(a, _)| *a);
    }
    let colour: Vec<Channel> = colour.into_iter().map(|(_, c)| c).collect();
    if colour.is_empty() {
        return Err(Refusal::Structure(
            "an image whose every channel is opacity",
        ));
    }

    // One precision for the whole output, because `JpxImage` carries one and
    // ISO 32000-1's `/BitsPerComponent` is one number. Normalising channels of
    // different depths against each other means scaling an 8-bit channel to
    // sit beside a 12-bit one, which is a choice about the picture's contrast
    // and not an arithmetic consequence of anything -- so it is refused by
    // name, in the same class as a `colr` method this build cannot map and for
    // the same reason: a decoder that picked silently would be picking.
    //
    // It carried `Refusal::NotBuilt` until milestone 8. That variant meant
    // "the codestream parsed and a *stage* of this decoder comes next", and it
    // named dequantisation, then the wavelet, then the colour pipeline as
    // milestones 4 to 6 removed the one below it. Every stage now exists, so
    // the variant went with them: what is left here is a capability this build
    // does not have, which is what `Feature` is, and keeping a stage-shaped
    // refusal for it would have gone on telling a reader that some part of the
    // decoder was missing.
    let precision = colour[0].precision;
    if colour
        .iter()
        .chain(opacity_channel.as_ref())
        .any(|c| c.precision != precision)
    {
        return Err(Refusal::Feature(
            "channels of differing bit depth, which one output precision cannot carry",
        ));
    }

    Ok(Plan {
        colour,
        opacity: opacity_channel,
        premultiplied,
        precision,
    })
}

impl Channel {
    /// This channel's value at one component sample, through the palette if
    /// it has one.
    ///
    /// The index is clamped rather than refused. A palette index is a decoded
    /// *sample*, so a lossy codestream (or a truncated one) can land a pixel
    /// outside the table by a level or two through no fault of the file's,
    /// and refusing a whole image for that would be the refusal doing harm.
    /// Out of range by a mile is the same clamp and stays a picture, which is
    /// the one place in this decoder where that is the right answer: a
    /// palette is a lookup, not a smoothing operator, so a clamped index
    /// shows as a wrong pixel and not as a plausible photograph.
    pub(crate) fn lookup(self, palette: Option<&Palette>, value: i32) -> i32 {
        let Some(column) = self.column else {
            return value;
        };
        let Some(entries) = palette.and_then(|p| p.columns.get(column)) else {
            return value;
        };
        if entries.is_empty() {
            return value;
        }
        let index = value.clamp(0, MAX_PALETTE_INDEX.min(entries.len() as i32 - 1));
        entries[index as usize]
    }
}
