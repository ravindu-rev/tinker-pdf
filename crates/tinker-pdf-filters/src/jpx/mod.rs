//! JPEG 2000 (ITU-T T.800 / ISO-IEC 15444-1), the decoder.
//!
//! Scope, the three questions it was not allowed to start without, and the
//! milestone order: `docs/plans/gaps/18a-jpx-decoder.md`.
//!
//! # Why the refusals are the feature
//!
//! **A wrong JPEG 2000 decode looks like a photograph.** A wrong Huffman
//! table produces obvious garbage and a wrong CCITT mode code produces
//! streaks, but tier-2 hands tier-1 a byte range, tier-1 hands the wavelet a
//! set of coefficients, and the inverse wavelet is a *smoothing operator*: it
//! turns wrong coefficients into a soft, low-frequency, plausible image. A
//! decoder that mis-parses a packet header for a progression order it did not
//! implement does not fail. It produces a blurry picture nobody downstream
//! can distinguish from a bad scan, in a document where the real image was a
//! chest radiograph or a map.
//!
//! So every capability this build does not have is enumerated and named,
//! never defaulted past. Two of the checks are *integrity* checks rather
//! than capability checks and land with the milestones that can make them: a
//! packet whose declared length does not reach the next packet's start
//! (tier-2), and the segmentation symbol that says the arithmetic decoder is
//! still in step (tier-1).
//!
//! # Bounds (ruling 1)
//!
//! JPEG 2000 is the worst branching structure in the engine: a codestream
//! multiplies **tiles x components x resolutions x precincts x code-blocks x
//! layers x bit-planes x three coding passes**, and every one of those
//! factors is individually bounded by the standard while their product is
//! not. 65 535 tiles is legal, 16 384 components is legal, 33 resolutions is
//! legal, and a code-block may carry 31 bit-planes.
//!
//! `5adf502` landed the general lesson one layer up — an 1 851-byte page that
//! took 19.3 seconds to render 9 600 pixels, inside a depth cap that never
//! fired — and it is the same lesson here: **a per-item cap is not a work cap
//! once the structure branches.** So the budgets below are *totals*, spent
//! and never refunded, and the per-item caps beside them each say in as many
//! words that they are not the work cap.
//!
//! # What is built
//!
//! Milestones 1 to 3 of the plan: the container, the codestream headers,
//! tier-2 and tier-1. The dequantisation, the inverse wavelet and the colour
//! pipeline are milestones 4 to 6 and are **not** here, so [`jpx_decode`]
//! parses a codestream as far as its coefficients and then refuses it with
//! [`Refusal::NotBuilt`]. That is the honest answer and it draws the
//! placeholder (ruling 2); it is not a partial picture.

pub(crate) mod boxes;
pub(crate) mod codestream;
pub(crate) mod tier2;

use crate::{Capability, FilterError, Limits, Warning};

// --- the budget ---------------------------------------------------------

/// Tile-component samples, summed over **every** tile and **every**
/// component, checked with a checked multiply before any plane exists.
///
/// This is a total, not a per-item cap. One tile inside any sane ceiling
/// times T.800's legal 16 384 components is not inside anything, which is
/// why [`MAX_JPX_COMPONENTS`] below cannot stand in for it.
///
/// **The magnitude is measured, not chosen.** `1 << 26` samples is exactly
/// 4096 x 4096 x 4 components, whose interleaved 16-bit output is
/// 134 217 728 bytes — `tinker_pdf_cos::limits::MAX_DECODED_STREAM` to the
/// byte, the ceiling every other stream in this engine already decodes
/// under. The coefficient planes for it are 256 MB of `i32`, which is the
/// same order as the 268 MB the plan costs for a 4096 x 4096 four-component
/// tile. A caller's own [`Limits::max_output`] is checked as well and the
/// smaller of the two wins, so a tighter ceiling is respected rather than
/// overridden by this one.
pub(crate) const MAX_JPX_SAMPLES: u64 = 1 << 26;

/// Components in one codestream.
///
/// **Not the work cap** — see [`MAX_JPX_SAMPLES`], which is. T.800 A.5.1
/// permits 16 384, and this build refuses past 64 because the colour
/// pipeline has nothing to say about more: ISO 32000-1 8.6.6.5 caps a
/// DeviceN space at 32 colorants, and 64 leaves room for a `cdef` opacity
/// channel beside each of them. A file above this is refused by name rather
/// than truncated to something interpretable.
pub(crate) const MAX_JPX_COMPONENTS: u32 = 64;

/// Tiles in one codestream, which is T.800 A.5.1's own bound rather than an
/// invented one: the standard requires `numXtiles * numYtiles <= 65535`.
///
/// **Not the work cap.** 65 535 tiles of one sample each is nothing;
/// 65 535 tiles of a megapixel each is not, and only [`MAX_JPX_SAMPLES`]
/// can tell the two apart.
pub(crate) const MAX_JPX_TILES: u64 = 65_535;

/// Decomposition levels, T.800 A.6.1's own bound. Resolutions are one more.
pub(crate) const MAX_JPX_LEVELS: u8 = 32;

/// Code-blocks in one tile-component, across every resolution and precinct.
///
/// **Not the work cap** — [`MAX_JPX_SAMPLES`] is. This bounds the *bookkeeping*
/// instead, which the sample count does not: a tag tree's node count comes
/// from the precinct grid, and the precinct grid comes from file-supplied
/// exponents, so a codestream can declare a modest image partitioned into an
/// enormous number of very small precincts and ask for the trees before a
/// single coefficient is read.
///
/// The magnitude is T.800's own arithmetic rather than an invented one. B.7
/// puts a code-block at 4x4 samples minimum, so [`MAX_JPX_SAMPLES`] at
/// `1 << 26` cannot contain more than `1 << 22` of them however it is cut up.
/// Anything past that is describing a partition that cannot correspond to the
/// samples it claims, and is refused by name rather than allocated for.
pub(crate) const MAX_JPX_CODE_BLOCKS: u64 = 1 << 22;

// --- the refusal list ---------------------------------------------------

/// Why a codestream was refused.
///
/// The public surface is [`FilterError::Unsupported`] plus one [`Warning`],
/// because that is the whole degradation contract (ruling 2) and a caller
/// draws the same placeholder for all of it. This type is what the *tests*
/// assert against, and it exists because "every marker in Table A.2 is
/// either parsed or named in a refusal" is only checkable if the refusal
/// carries the name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// A marker T.800 Table A.2 defines and this build does not decode:
    /// RGN, POC, PPM, PPT, CRG. Carries the marker's name.
    Marker(&'static str),
    /// A marker code Table A.2 does not define — which includes every marker
    /// ISO/IEC 15444-2 adds, since Part 2 is a non-goal.
    UnknownMarker(u16),
    /// A structural constraint: A.5.1's tile grid, a marker segment length
    /// that does not match its contents, marker ordering, a JP2 box that
    /// runs past its parent.
    Structure(&'static str),
    /// A coding feature this build does not implement: a progression order,
    /// a Table A.19 code-block style bit, a quantisation style, an `Rsiz`
    /// capability.
    Feature(&'static str),
    /// Component precision above 16 bits. T.800 allows 38; PDF's sample path
    /// reads at most 16 and the fixed-point format is proved for 16, so this
    /// is refused rather than truncated.
    Precision(u8),
    /// The data ended inside something that was still being read.
    Truncated(&'static str),
    /// A total from the block above was spent. Carries which.
    Budget(&'static str),
    /// A packet did not end where the next one begins.
    ///
    /// An **integrity** refusal rather than a capability one, and the
    /// distinction is why it has its own variant instead of folding into
    /// [`Refusal::Structure`]. Tier-2 carries no image data: it hands tier-1
    /// a byte range, and a byte range wrong by a few bytes still decodes into
    /// coefficients, which still go through the inverse wavelet, which
    /// smooths them into a photograph. Nothing downstream can tell.
    ///
    /// So the arithmetic is checked against itself: with SOP signalled every
    /// packet must begin with one, with EPH signalled every header must end
    /// with one, and in every case a tile's packets must consume its data
    /// exactly. A tier-2 parser that has gone wrong almost never lands on a
    /// packet boundary by accident, which makes this the cheapest real
    /// defence in the decoder -- and it fires before a single pixel exists.
    PacketLength,
    /// The codestream parsed and this build has not got the stage that comes
    /// next. Milestones 4 to 6: dequantisation, the inverse wavelet, colour.
    NotBuilt(&'static str),
}

impl Refusal {
    /// The one typed leniency record this refusal leaves behind (ruling 10).
    ///
    /// Coarser than the refusal itself on purpose: [`Warning`] is a closed
    /// set recorded at most once per decode, and a variant per marker would
    /// make it neither.
    pub(crate) const fn warning(self) -> Warning {
        match self {
            Self::Marker(_) => Warning::JpxMarkerUnsupported,
            Self::UnknownMarker(_) => Warning::JpxMarkerUnknown,
            Self::Structure(_) => Warning::JpxStructureInvalid,
            Self::Feature(_) => Warning::JpxFeatureUnsupported,
            Self::Precision(_) => Warning::JpxPrecisionUnsupported,
            Self::Truncated(_) => Warning::JpxTruncated,
            Self::Budget(_) => Warning::JpxBudgetSpent,
            // Structure, not a category of its own: a packet that does not
            // end where the next begins is a codestream whose arithmetic
            // disagrees with itself, which is what that warning already says.
            Self::PacketLength => Warning::JpxStructureInvalid,
            Self::NotBuilt(_) => Warning::JpxStageNotBuilt,
        }
    }
}

/// A big-endian cursor that runs out rather than panicking.
///
/// Every field width JPEG 2000 uses is here, and every one of them returns
/// `None` at the end of the data instead of indexing. Ruling 1: the lengths
/// this reads are attacker-controlled and the reader is the only thing
/// between them and a slice.
pub(crate) struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Cursor<'a> {
        Cursor { data, at: 0 }
    }

    pub(crate) fn position(&self) -> usize {
        self.at
    }

    pub(crate) fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.at)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        let b = self.data.get(self.at).copied()?;
        self.at += 1;
        Some(b)
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        Some(u16::from(self.u8()?) << 8 | u16::from(self.u8()?))
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        Some(u32::from(self.u16()?) << 16 | u32::from(self.u16()?))
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        Some(u64::from(self.u32()?) << 32 | u64::from(self.u32()?))
    }

    /// `n` bytes, or `None` if fewer remain. Never a short slice: a caller
    /// that got `Some` may assume the length.
    pub(crate) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.data.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
}

/// A JPEG 2000 image, as this crate hands it back.
///
/// Ruling 8 binds the shape: no COS type crosses this boundary and no PDF
/// vocabulary appears in it. In particular **`/SMaskInData` is not here** —
/// deciding what an opacity channel is *for* is ISO 32000-1 8.9.5.4's rule
/// and it belongs in `resources.rs` beside `/ColorSpace`, which is the same
/// split gap 17 made for JBIG2's polarity and for the same reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JpxImage {
    pub width: u32,
    pub height: u32,
    /// Colour components, excluding any opacity channel.
    pub components: u8,
    /// 8 or 16. The codestream's own precision decides it, not
    /// `/BitsPerComponent` (8.9.5.4).
    pub precision: u8,
    /// Interleaved samples, `components` per pixel, row-major, big-endian
    /// when `precision` is 16.
    pub samples: Vec<u8>,
    /// What the codestream says its colour space is. `/ColorSpace` on the
    /// image dictionary overrides this when it is present.
    pub colour: JpxColour,
}

/// The colour space a codestream declares (T.800 Annex I `colr`), in this
/// crate's own vocabulary.
///
/// A method or enumerated value this build cannot map is **reported**, not
/// guessed at (ruling 2), so there is no `Unknown` that renders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JpxColour {
    /// No `colr` box at all — a bare J2K codestream. The component count is
    /// all there is to go on.
    #[default]
    Unstated,
    Greyscale,
    Srgb,
    Sycc,
    EYcc,
    Cmyk,
    /// `colr` method 2: an embedded ICC profile. Read for its component
    /// count and reported; conversion is `tinker-pdf-color`'s business.
    IccProfile,
}

/// Decodes a JPEG 2000 codestream, boxed (JP2/JPX) or bare (J2K).
///
/// # Errors
///
/// [`FilterError::Unsupported`] for every codestream this build cannot
/// decode *correctly*, which is the whole degradation contract: the caller
/// draws the neutral placeholder rather than being handed a plausible
/// picture of nothing. `warnings` is a sink rather than a return value
/// because the refusal is an `Err` and ruling 10 wants the reason to survive
/// it — "refused because it uses a progression order this build has not got"
/// and "refused because a packet header did not land on a packet boundary"
/// are different failures and the error alone cannot tell them apart.
pub fn jpx_decode(
    input: &[u8],
    limits: &Limits,
    warnings: &mut Vec<Warning>,
) -> Result<JpxImage, FilterError> {
    match decode_inner(input, limits) {
        Ok(image) => Ok(image),
        Err(refusal) => {
            let w = refusal.warning();
            if !warnings.contains(&w) {
                warnings.push(w);
            }
            Err(FilterError::Unsupported(Capability::Jpx))
        }
    }
}

fn decode_inner(input: &[u8], limits: &Limits) -> Result<JpxImage, Refusal> {
    let container = boxes::parse(input)?;
    let stream = codestream::parse(container.codestream)?;
    stream.check_budget(limits)?;
    // Tier-2 runs here rather than beside tier-1, and it runs even though
    // nothing consumes its answer yet. Two reasons, both about what a refusal
    // is worth. It is where the integrity checks live -- a packet that does
    // not end where the next begins is a codestream disagreeing with itself,
    // and catching that here means a malformed file is refused *by name*
    // instead of reaching a stage that would smooth it into a photograph. And
    // it is what makes the refusal below honest: `tier-1` names the one stage
    // that is missing, where "tier-2 and everything after it" named five and
    // would have gone on naming five after tier-2 was written.
    let _tiles = tier2::decode_tiles(&stream)?;
    // Milestones 3 to 6. The packets are located and there is nothing yet to
    // turn their bytes into coefficients, so this refuses rather than
    // inventing any.
    Err(Refusal::NotBuilt("tier-1"))
}

#[cfg(test)]
mod tests;
