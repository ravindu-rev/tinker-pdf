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
//! Milestones 1 to 6 of the plan: the container, the codestream headers,
//! tier-2, tier-1, dequantisation, both inverse wavelets — the reversible 5/3
//! and the irreversible 9/7 in fixed point — and Annex G and I's colour
//! pipeline. What is left is the *PDF boundary* (milestone 7), which is not a
//! stage of this crate: ISO 32000-1 8.9.5.4's rules about `/ColorSpace`,
//! `/BitsPerComponent` and `/SMaskInData` are `resources.rs`'s and no COS
//! type crosses into here (ruling 8). So a JPX image still draws the
//! placeholder in a rendered page, and this decoder is what stops doing so.

pub(crate) mod boxes;
pub(crate) mod codestream;
pub(crate) mod colour;
pub(crate) mod tier1;
pub(crate) mod tier2;
pub(crate) mod wavelet;

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

/// **The real budget**: coefficients times coding passes, charged from a
/// code-block's declared pass count *before* the first of them runs.
///
/// It is a **total**, spent and never refunded across every tile, component,
/// resolution, precinct, code-block and layer of the codestream — because the
/// per-item caps beside it are each individually satisfiable by a stream that
/// is unbounded in the product. `5adf502` landed that lesson one layer up: an
/// 1 851-byte page that took 19.3 seconds to render 9 600 pixels, inside a
/// nesting cap that was in place the whole time, because **a depth or
/// per-item cap is not a work cap once the structure branches.**
///
/// # The magnitude is measured, and the first value was wrong
///
/// It stood at `3 << 31` from milestone 1 until milestone 8 measured it, and
/// **at that value it could not fire at all.** Tier-1's work is bounded above
/// by the total code-block area times [`tier1::MAX_PASSES`], and the total
/// code-block area is not a free parameter: T.800's subbands are critically
/// sampled, so the areas over every resolution of a tile-component sum to
/// exactly that tile-component's sample count, which [`MAX_JPX_SAMPLES`]
/// caps. So no codestream can ask for more than `MAX_JPX_SAMPLES *
/// MAX_PASSES` = 6 106 906 624 coefficient-passes, and `3 << 31` is
/// 6 442 450 944 — above the ceiling of its own input. It was a budget with
/// nothing on the other side of it, which is precisely the shape
/// `MAX_GROUP_DEPTH` had when `5adf502` found it, arrived at here by a
/// one-bit slip between a derivation and the constant it produced: the
/// paragraph that set it said "rounding that worst case up to 48 and
/// multiplying by `MAX_JPX_SAMPLES`", and 48 times `1 << 26` is `3 << 30`.
/// `the_work_cap_can_fire_at_all` now asserts the relation rather than the
/// number, so the next reader who adjusts it cannot walk back past the input.
///
/// **What the measurement says.** Every codestream this repository has was
/// run and its spend divided by its tile-component sample count: the 44
/// `opj_compress` fixtures under `tests/jpx`, which span five progression
/// orders, one to five decomposition levels, 4 x 4 to 64 x 64 code-blocks,
/// three quality layers, multiple tiles, subsampling, RGB through the RCT and
/// the ICT, and lossy 9/7 both at a rate and truncated; and the fifteen of
/// gap 23's nineteen real files that reach tier-1. The densest is **22.75
/// coefficient-passes per tile-component sample** — a lossless 5/3 greyscale,
/// which is the heaviest coding there is, since every plane of every
/// coefficient is present — and the densest *real* file is 22.0. Lossy
/// streams spend between 0.17 and 15.
///
/// So this is 48 per sample: `48 * MAX_JPX_SAMPLES`, which is **2.1 times the
/// densest thing ever measured here** and 53 per cent of what the format can
/// ask for. A budget that refuses a real file is worse than one that is
/// generous, and the gap between 22.75 and 91 is where the only honest room
/// to choose is.
///
/// **What it adds over [`MAX_JPX_SAMPLES`] is smaller than the plan assumed**,
/// and saying so is the point of having measured. The plan costed a hostile
/// stream at `1 << 22` code-blocks of 4 096 coefficients and 91 passes —
/// 1.5 x 10^12 — but that is 1.7 x 10^10 coefficients, 256 times what
/// [`MAX_JPX_SAMPLES`] permits, so the sample ceiling already forbids it. The
/// factor genuinely bought here is 91/48, just under two. What is *not*
/// redundant is when the charge falls: it comes from the pass count in the
/// packet header, so a code-block is refused before it decodes a single
/// decision rather than after ninety of them.
pub(crate) const MAX_JPX_WORK: u64 = 3 << 30;

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
    /// A cleanup pass did not end with T.800 D.5's `1010`.
    ///
    /// The **second** integrity refusal, and it has its own variant for the
    /// same reason [`Refusal::PacketLength`] does. When a codestream signals
    /// segmentation symbols (Table A.19 bit 0x20) every cleanup pass ends
    /// with four decisions in the UNIFORM context whose value the standard
    /// fixes, so decoding them and *checking* them is a free per-code-block
    /// verification that the arithmetic decoder is still in step. A build
    /// that decoded them and discarded them would have thrown away the one
    /// thing in the format that says the picture is wrong — and tier-1 is
    /// exactly where being subtly out of step produces plausible coefficients
    /// rather than an error.
    SegmentationSymbol,
    /// The codestream parsed and this build has not got the stage that comes
    /// next.
    ///
    /// It named dequantisation, then the inverse wavelet, then the colour
    /// pipeline, as milestones 4 to 6 each removed the one below it. What is
    /// left is the stage nobody has specified: reconciling components of
    /// *different* bit depths into the single output precision `JpxImage`
    /// carries and `/BitsPerComponent` is. Scaling an 8-bit channel to sit
    /// beside a 12-bit one is a choice about the picture's contrast, and a
    /// decoder that made it silently would be choosing.
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
            // Structure too, and for the same reason: a code-block whose
            // cleanup pass does not end where D.5 says it does is a
            // codestream disagreeing with itself.
            Self::SegmentationSymbol => Warning::JpxStructureInvalid,
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
    /// The channel a `cdef` box typed as opacity (T.800 I.5.3.6), if the
    /// file had one.
    ///
    /// It is carried out of the colour channels rather than interleaved with
    /// them because what it is *for* is not this crate's decision:
    /// `/SMaskInData` says whether it is ignored, used as a soft mask, or
    /// used and un-multiplied, and that rule is ISO 32000-1 8.9.5.4's and
    /// lives beside `/ColorSpace` (ruling 8).
    pub opacity: Option<JpxOpacity>,
}

/// The opacity channel `cdef` named, and which of its two types it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JpxOpacity {
    /// `cdef` type 2 rather than 1: the colour channels have already been
    /// multiplied by this one. **They are not un-multiplied here** — that is
    /// `/SMaskInData` 2's rule and it needs the image dictionary.
    pub premultiplied: bool,
    /// One sample per pixel, row-major, at [`JpxImage::precision`].
    pub samples: Vec<u8>,
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
    let mut clamped = false;
    let decoded = decode_inner(input, limits, &mut clamped);
    // Recorded whether the decode went on to succeed or not: E.1's clamp is a
    // leniency, and ruling 10 wants a leniency to survive the result it led
    // to rather than only the ones that failed.
    if clamped && !warnings.contains(&Warning::JpxCoefficientClamped) {
        warnings.push(Warning::JpxCoefficientClamped);
    }
    match decoded {
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

fn decode_inner(input: &[u8], limits: &Limits, clamped: &mut bool) -> Result<JpxImage, Refusal> {
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
    let mut tiles = tier2::decode_tiles(&stream)?;
    // Tier-1 runs here for the same reason tier-2 does: it is where the
    // second integrity check lives. A stream that signalled segmentation
    // symbols has told the decoder what every cleanup pass must end with, so
    // a code-block that has drifted is named here rather than handed on as
    // plausible coefficients.
    tier1::decode_tiles(&stream, &mut tiles)?;

    // Annex I's boxes and Annex A's components together decide what a channel
    // *is*, and that has to be settled before a single sample is written: a
    // `pclr` palette turns one component into three or four channels, so the
    // output is not the shape the codestream's component count suggests.
    let header = container.header.as_ref();
    let plan = colour::plan(header, &stream.siz.components)?;
    let palette = header.and_then(|h| h.palette.as_ref());
    let precision = plan.precision;
    if precision > 16 {
        return Err(Refusal::Precision(precision));
    }
    if plan.colour.len() > MAX_JPX_COMPONENTS as usize {
        return Err(Refusal::Budget("output colour channels"));
    }

    let width = stream.siz.width();
    let height = stream.siz.height();
    let stride = plan.colour.len();
    let wide = usize::from(precision > 8) + 1;
    // Re-spent here rather than trusted from `check_budget`, because a
    // palette is exactly the thing that makes that earlier figure wrong: it
    // was computed from the codestream's component count, and one component
    // of indices can generate four channels. A `pclr` box is a few hundred
    // bytes and it quadruples the output (ruling 1).
    let channels = stride + usize::from(plan.opacity.is_some());
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(channels as u64))
        .and_then(|n| n.checked_mul(wide as u64))
        .ok_or(Refusal::Budget("output samples"))?;
    if bytes > limits.max_output as u64 {
        return Err(Refusal::Budget("the caller's output ceiling"));
    }

    let pixels = (width as usize) * (height as usize);
    let mut samples = vec![0u8; pixels * stride * wide];
    let mut opacity = plan.opacity.map(|_| vec![0u8; pixels * wide]);

    for tile in &tiles {
        // Every component of the tile at once, because G.2's transform reads
        // three planes and applying it one plane at a time is not a thing
        // that can be done.
        let mut planes = wavelet::reconstruct_tile(&stream, tile, clamped)?;
        for (c, plane) in planes.iter_mut().enumerate() {
            let component = &stream.siz.components[c];
            // G.1, and *after* G.2 rather than before it — which is what
            // `reconstruct_tile` having already run makes true here.
            wavelet::level_shift(plane, component.precision, component.signed);
        }
        let bounds = stream.siz.tile_bounds(u32::from(tile.index));
        for (k, channel) in plan.colour.iter().enumerate() {
            blit(
                &planes,
                &stream.siz,
                bounds,
                *channel,
                palette,
                Blit {
                    out: &mut samples,
                    stride,
                    at: k,
                    wide,
                },
            );
        }
        if let (Some(channel), Some(out)) = (plan.opacity, opacity.as_mut()) {
            blit(
                &planes,
                &stream.siz,
                bounds,
                channel,
                palette,
                Blit {
                    out,
                    stride: 1,
                    at: 0,
                    wide,
                },
            );
        }
    }

    Ok(JpxImage {
        width,
        height,
        components: u8::try_from(stride).map_err(|_| Refusal::Budget("output colour channels"))?,
        precision,
        samples,
        colour: header.map_or(JpxColour::Unstated, |h| h.colour),
        opacity: opacity.map(|samples| JpxOpacity {
            premultiplied: plan.premultiplied,
            samples,
        }),
    })
}

/// Where one channel's samples are going.
struct Blit<'a> {
    out: &'a mut [u8],
    /// Channels per pixel in `out`.
    stride: usize,
    /// This channel's index within a pixel.
    at: usize,
    /// Bytes per sample: 2 above eight bits, 1 otherwise.
    wide: usize,
}

/// Write one channel of one tile onto the reference grid, replicating a
/// subsampled component.
///
/// **Replication, not filtering**, and the reason is the oracle rather than
/// taste: `opj_decompress -upsample` takes the sample at `floor(X / XRsiz)`
/// unfiltered, and gate 1 asks for byte-identity against it. A triangle
/// filter — which is what `docs/plans/02-filters.md` chose for JPEG, where
/// the oracle is libjpeg-turbo and libjpeg-turbo interpolates — would put
/// every edge pixel a level or two out and the gate would have to become a
/// tolerance.
///
/// The clamp on the way in is the same rule one step further: `XRsiz` need
/// not divide the tile's origin, so the first reference-grid column of a tile
/// can map below the component's own first column. Reading the nearest real
/// sample is what replication means at an edge; reading zero would put a
/// black line down the left of every such tile.
fn blit(
    planes: &[wavelet::Plane],
    siz: &codestream::Siz,
    (x0, y0, x1, y1): (u32, u32, u32, u32),
    channel: colour::Channel,
    palette: Option<&boxes::Palette>,
    to: Blit<'_>,
) {
    let (Some(plane), Some(component)) = (
        planes.get(channel.component),
        siz.components.get(channel.component),
    ) else {
        return;
    };
    if plane.width == 0 || plane.height == 0 {
        return;
    }
    let (dx, dy) = (
        u32::from(component.dx).max(1),
        u32::from(component.dy).max(1),
    );
    let width = siz.width();
    let height = siz.height();
    for y in y0..y1 {
        let Some(py) = y.checked_sub(siz.yosiz) else {
            continue;
        };
        if py >= height {
            continue;
        }
        let cy = (y / dy).saturating_sub(plane.y0).min(plane.height - 1);
        for x in x0..x1 {
            let Some(px) = x.checked_sub(siz.xosiz) else {
                continue;
            };
            if px >= width {
                continue;
            }
            let cx = (x / dx).saturating_sub(plane.x0).min(plane.width - 1);
            let value = channel.lookup(palette, plane.at(cx, cy));
            let index = ((py as usize) * (width as usize) + (px as usize)) * to.stride + to.at;
            #[allow(
                clippy::cast_sign_loss,
                reason = "clamped by level_shift, or a palette entry of the declared precision"
            )]
            let value = value as u32;
            if to.wide == 2 {
                to.out[index * 2] = (value >> 8) as u8;
                to.out[index * 2 + 1] = (value & 0xFF) as u8;
            } else {
                to.out[index] = value as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests;
