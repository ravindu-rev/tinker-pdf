//! JPEG 2000 (ITU-T T.800 / ISO-IEC 15444-1), the decoder.
//!
//! Scope, the three questions it was not allowed to start without, and the
//! milestone order: gap 18a.
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
//! All of it, as this crate's half goes: the container, the codestream
//! headers, tier-2, tier-1, dequantisation, both inverse wavelets — the
//! reversible 5/3 and the irreversible 9/7 in fixed point — and Annex G and
//! I's colour pipeline. ISO 32000-1 8.9.5.4's rules about `/ColorSpace`,
//! `/BitsPerComponent` and `/SMaskInData` are `resources.rs`'s and no COS
//! type crosses into here (ruling 8).
//!
//! **What is refused is enumerated rather than defaulted**, and the list is
//! the plan's with three of its entries narrowed from *a capability this
//! build lacks* to *a value the standard reserves* — POC, RGN and, on
//! 23 September 2026, Table A.19's code-block styles. Each narrowing is
//! visible in the refusal string itself and in `tests::refusals`, which is
//! why they are named here rather than counted against a plan no longer in
//! the tree. What is refused: a progression order
//! Table A.16 does not define or a code-block style bit Table A.19 does not
//! define, an `Srgn` Table A.25 reserves, any Part 2 marker, SOP or EPH
//! outside the bit stream, tile-parts out of order, a `colr` this build
//! cannot map, precision above 16 bits, channels of differing bit depth, and
//! any of the three budgets above. Every entry is reached by a test in
//! `tests::refusals`, because "the refusals are the feature" is only a claim
//! if something checks that they fire.
//!
//! POC and RGN stood on that list as whole markers and no longer do — A.6.6's
//! progressions are B.12.2's progression order volumes in tier-2, and A.6.3's
//! shift is H.1's Maxshift in the dequantiser — so what is left of each is a
//! *value inside* a segment rather than the segment.
//!
//! **Table A.19 has left it entirely**, and it is the one entry that went to
//! zero rather than narrowing. All six code-block styles are decoded: the
//! three that are decisions about context state, the segmentation symbol that
//! is an integrity check, and — last, on 23 September 2026 — the selective
//! arithmetic coding bypass of D.6 and the termination on each coding pass of
//! D.4, which together are B.10.7.2's multiple codeword segments plus a raw
//! bit reader. What is left of the entry is a style **bit** Table A.19 does
//! not define, which is bits 6 and 7 and a value rather than a capability.
//! [`passes`] holds the split and says which half belonged to which bit.
//!
//! Measured against gap 23's nineteen real JPX files: fourteen decode, four
//! refuse by name, and one is never asked for — its image sits two form
//! XObjects deep and a form's own `/Resources` are consulted nowhere in this
//! engine, which is gap 11's recorded non-goal rather than this decoder's.
//! That is a claim about nineteen files and not about JPEG 2000.

pub(crate) mod boxes;
pub(crate) mod codestream;
pub(crate) mod colour;
pub(crate) mod passes;
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

/// Component sample precision, in bits.
///
/// **T.800 Table A.11 allows 1 to 38** — `Ssiz` is `x000 0000` to
/// `x010 0101`, "component sample bit depth = value + 1", with the table's
/// own footnote a) adding that "not all combinations of coding styles will
/// allow the coding of 38-bit samples". This build stops at 16, and the
/// reason is arithmetic rather than taste, which is why the figure has a
/// name here instead of being a literal at each of the three places that
/// reads a depth.
///
/// **The coefficient plane is what caps it, not the output.** E.1's
/// dequantisation clamps a coefficient to `2^(R_b + 2)` sample units, where
/// `R_b` is the component precision; [`wavelet::Fixed`] stores a plane entry
/// at Q12 in an `i32`. So a clamped coefficient occupies `2^(R + 2 + 12)` of
/// the plane's own format, which is `2^30` at 16 bits — exactly
/// [`wavelet::PLANE_BOUND`] — and `2^31` at 17, which an `i32` does not
/// hold. Seventeen bits is where the plane format runs out, not where a
/// policy begins, and `the_coefficient_plane_is_what_caps_precision` in
/// `tests::bounds` asserts that relation rather than restating the 16.
///
/// The output cannot carry it either: [`JpxImage::precision`] is 8 or 16 and
/// ISO 32000-1 Table 89 gives `/BitsPerComponent` as 1, 2, 4, 8 or 16, so
/// there is no member of the PDF image model above this to widen into. That
/// is why this is a **limit** in ROADMAP's Named non-goals rather than a row
/// — unlike JPEG's twelve-bit frames, which decode and are narrowed to eight
/// on the way out, because there the coefficient path is fixed at the
/// frame's own precision and only the *sample* needed narrowing.
pub(crate) const MAX_JPX_PRECISION: u8 = 16;

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
/// run and its spend divided by its tile-component sample count: the 39
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
pub enum Refusal {
    /// A marker T.800 Table A.2 defines, refused **where it stands**: SOP or
    /// EPH in a header. Carries the marker's name.
    ///
    /// **No Table A.2 marker is refused as a capability any more**, and the
    /// last one left on 21 September 2026. It was five, and they went one at
    /// a time, each for its own reason. CRG went when A.9.1 was read: the
    /// clause says it "has no effect on decoding the codestream", so it is
    /// parsed and carried. RGN went when Annex H was implemented; what is
    /// left of it is a [`Refusal::Feature`] naming the one `Srgn` style Table
    /// A.25 reserves, which is a value inside the segment rather than the
    /// segment. PPM and PPT went when packed packet headers were implemented
    /// (A.7.4, A.7.5); they are parsed, and tier-2 reads a packet's header
    /// bits from the packed stream. POC went last, when A.6.6's progressions
    /// became B.12.2's progression order volumes in tier-2's packet sequence.
    ///
    /// What is left is the two markers A.8 puts inside the bit stream. Both
    /// are implemented there; a header is the one place they have no meaning,
    /// and that is what this variant now names.
    Marker(&'static str),
    /// A marker code Table A.2 does not define — which includes every marker
    /// ISO/IEC 15444-2 adds, since Part 2 is a non-goal.
    UnknownMarker(u16),
    /// A structural constraint: A.5.1's tile grid, a marker segment length
    /// that does not match its contents, marker ordering, a JP2 box that
    /// runs past its parent.
    Structure(&'static str),
    /// A coding feature this build does not implement: a progression order,
    /// a quantisation style, an `Rsiz` capability — or a **value** a table
    /// reserves, which is what most of this variant now carries. Table A.19's
    /// code-block styles were the last capability here and all six are
    /// decoded; what fires for A.19 now is a bit the table does not define.
    Feature(&'static str),
    /// Component precision above [`MAX_JPX_PRECISION`]. T.800 Table A.11
    /// allows 38.
    ///
    /// **A limit rather than a gap**, and ROADMAP's Named non-goals is where
    /// it is argued. The short form: E.1 clamps a coefficient to
    /// `2^(R_b + 2)` sample units and the plane holds a Q12 `i32`, so 17 bits
    /// is where the plane format runs out; and there is nowhere to hand the
    /// result even if it were widened, because ISO 32000-1 Table 89 gives
    /// `/BitsPerComponent` as 1, 2, 4, 8 or 16.
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
}

impl Refusal {
    /// The one typed leniency record this refusal leaves behind (ruling 10).
    ///
    /// Coarser than the refusal itself on purpose: [`Warning`] is a closed
    /// set recorded at most once per decode, and a variant per marker would
    /// make it neither.
    #[must_use]
    pub const fn warning(self) -> Warning {
        match self {
            Self::Marker(_) => Warning::JpxMarkerUnsupported,
            Self::UnknownMarker(_) => Warning::JpxMarkerUnknown,
            Self::Structure(_) => Warning::JpxStructureInvalid,
            Self::Feature(_) => Warning::JpxFeatureUnsupported,
            Self::Precision(_) => Warning::JpxPrecisionUnsupported,
            Self::Truncated(_) => Warning::JpxTruncated,
            Self::Budget(_) => Warning::JpxBudgetSpent,
            // The two integrity refusals keep their own warnings, where they
            // both reported `JpxStructureInvalid` until milestone 8. That
            // mapping was defensible in the small -- a codestream disagreeing
            // with itself *is* structurally invalid -- and wrong in the large
            // for two reasons.
            //
            // It made two public `Warning` variants that nothing could emit,
            // which is a surface claiming a distinction it does not draw. And
            // it threw away the distinction at exactly the boundary where a
            // caller can act on it: a capability refusal says this build is
            // short of a feature and a better build would draw the picture,
            // while an integrity refusal says the *file* is inconsistent and
            // no decoder should draw it. The `Refusal` enum has kept them
            // apart from the milestone that wrote them, on the grounds that a
            // reader should be able to tell them apart; a reader outside this
            // crate is still a reader.
            Self::PacketLength => Warning::JpxPacketLength,
            Self::SegmentationSymbol => Warning::JpxSegmentationSymbol,
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

    /// The bytes not yet read.
    pub(crate) fn rest(&self) -> &'a [u8] {
        self.data.get(self.at..).unwrap_or(&[])
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
    /// Whether some tile declared more parts than the file carried, so its
    /// samples are whatever the plane was initialised to rather than a decode.
    ///
    /// The picture keeps its shape and loses a rectangle of it — damage that
    /// costs pixels, which this crate draws around and reports rather than
    /// refusing (ruling 2). `check_tile_parts` is where the two are told
    /// apart, and a codestream with *no* whole tile is still a refusal.
    pub truncated: bool,
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
    /// The resolution levels the decode dropped from the top: the request's
    /// [`JpxRequest::discard`], clamped to the fewest levels any
    /// tile-component of the codestream has. The image lies on the reference
    /// grid reduced by `2^discard` (T.800 B.5), so `0` is the full size.
    pub discard: u8,
}

/// What a decode is asked for (`docs/design/jpx-speed.md`, "The decode
/// request").
///
/// A plain, hashable value, because it is what a caller's image cache keys
/// on: a hit must return exactly what a miss computes for the same request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct JpxRequest {
    /// Resolution levels to drop from the top. `0` decodes the full size;
    /// `d` decodes the image on the reference grid reduced by `2^d`, every
    /// dimension `ceil(x / 2^d)` (T.800 B.5), from the codestream's lower
    /// resolutions alone. A figure past the fewest levels a tile-component
    /// has is clamped to it, and [`JpxImage::discard`] says what was applied.
    pub discard: u8,
    /// The part of the image wanted, or `None` for all of it: a rectangle of
    /// the image a decode at [`JpxRequest::discard`] returns, in its own
    /// pixels from its top-left corner. The decode returns that rectangle's
    /// samples alone, clamped to the image — exactly the same rectangle of
    /// the whole decode — and a rectangle off the image is an empty image.
    pub window: Option<JpxWindow>,
}

/// A rectangle of a decoded image, in its own pixels from its top-left
/// corner: [`JpxRequest::window`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct JpxWindow {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// What one decode did, counted: the leaf's half of guard A7, handed back
/// as a value because this crate keeps no tally of its own (ruling 8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JpxCounts {
    /// Inverse-wavelet levels synthesised, every tile-component of every
    /// tile summed: a tile-component of `N` levels decoded with `d` dropped
    /// synthesises `N - d`.
    pub levels_synthesised: u64,
    /// Code-blocks tier-1 decoded. A dropped resolution's are not among
    /// them.
    pub code_blocks_decoded: u64,
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
    jpx_decode_counted(input, limits, warnings).0
}

/// [`jpx_decode`], and the steps it took (`docs/design/resource-exhaustion.md`,
/// "The counters"): what tier-2's budget was charged — each precinct and
/// code-block laid out and each packet ordered — with each packet read and
/// each code-block its header examined; what tier-1's work budget was
/// charged, each coefficient once a coding pass; each sample of every
/// reconstructed tile-component; and each byte of the image returned.
pub fn jpx_decode_counted(
    input: &[u8],
    limits: &Limits,
    warnings: &mut Vec<Warning>,
) -> (Result<JpxImage, FilterError>, u64) {
    let mut steps = 0u64;
    let decoded = request_counted(input, limits, &JpxRequest::default(), warnings, &mut steps)
        .map(|(image, _)| image);
    (decoded, steps)
}

/// [`jpx_decode`], asked for what `request` names — the full image at
/// [`JpxRequest::default`], or one with its top levels dropped — and with
/// what the decode did counted beside the image.
///
/// # Errors
///
/// [`FilterError::Unsupported`], as [`jpx_decode`] gives it, with the
/// reason in `warnings`.
pub fn jpx_decode_request(
    input: &[u8],
    limits: &Limits,
    request: &JpxRequest,
    warnings: &mut Vec<Warning>,
) -> Result<(JpxImage, JpxCounts), FilterError> {
    request_counted(input, limits, request, warnings, &mut 0)
}

/// [`jpx_decode_request`], adding the steps [`jpx_decode_counted`] names to
/// `steps`.
fn request_counted(
    input: &[u8],
    limits: &Limits,
    request: &JpxRequest,
    warnings: &mut Vec<Warning>,
    steps: &mut u64,
) -> Result<(JpxImage, JpxCounts), FilterError> {
    let mut clamped = false;
    let mut counts = JpxCounts::default();
    let decoded = decode_inner(input, limits, request, &mut clamped, &mut counts, steps);
    let truncated = matches!(&decoded, Ok(image) if image.truncated);
    // Recorded whether the decode went on to succeed or not: E.1's clamp is a
    // leniency, and ruling 10 wants a leniency to survive the result it led
    // to rather than only the ones that failed.
    if clamped && !warnings.contains(&Warning::JpxCoefficientClamped) {
        warnings.push(Warning::JpxCoefficientClamped);
    }
    match decoded {
        Ok(image) => {
            // A tile that stopped early is damage this decode drew around
            // rather than refused, and ruling 10 wants it named on the way
            // out — `Decoded::complete` is not reachable from here, so the
            // warning is the whole record.
            if truncated && !warnings.contains(&Warning::JpxTruncated) {
                warnings.push(Warning::JpxTruncated);
            }
            Ok((image, counts))
        }
        Err(refusal) => {
            let w = refusal.warning();
            if !warnings.contains(&w) {
                warnings.push(w);
            }
            Err(FilterError::Unsupported(Capability::Jpx))
        }
    }
}

/// [`jpx_decode`], with the precise reason for a refusal rather than the one
/// [`Warning`] its class reports.
///
/// The same decode; the difference is what comes back. `Warning` is a closed
/// set of seven, and `JpxStructureInvalid` alone covers every one of
/// [`Refusal::Structure`]'s conditions — a tile-part naming a tile outside the
/// grid, tile-parts out of order, two parts disagreeing about `TNsot`, a box
/// shorter than its header, a marker in the wrong place. Those are different
/// sentences to show a human and different answers to whether a file is worth
/// a roadmap row, and a corpus census cannot tell them apart from the warning.
///
/// # Errors
/// The [`Refusal`] the decode stopped on.
pub fn jpx_decode_attributed(input: &[u8], limits: &Limits) -> Result<JpxImage, Refusal> {
    let mut clamped = false;
    decode_inner(
        input,
        limits,
        &JpxRequest::default(),
        &mut clamped,
        &mut JpxCounts::default(),
        &mut 0,
    )
}

/// What a JPEG 2000 file says about itself before one sample is decoded.
///
/// The shape [`jpx_decode`] would hand back, without the samples: a caller
/// that places the file's own bytes rather than decoding them — the comic
/// path, which puts a `.jp2` page into a document as a `/JPXDecode` stream —
/// needs the page's size and whether the file is one this build could draw,
/// and it needs them at the cost of a header rather than of a raster.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JpxHeader {
    pub width: u32,
    pub height: u32,
    /// Colour channels a decode would produce, excluding any opacity channel
    /// — which a `pclr` palette makes different from the codestream's `Csiz`.
    pub components: u8,
    /// 8 or 16: the precision a decode would widen every channel to.
    pub precision: u8,
    /// What the container's `colr` box declares, or [`JpxColour::Unstated`].
    pub colour: JpxColour,
    /// Whether a `cdef` box typed a channel as opacity.
    pub opacity: bool,
}

/// Reads a [`JpxHeader`]: every stage of [`jpx_decode`] that runs before
/// tier-2, and none after.
///
/// That is Annex I's box walk, Annex A's main header and every tile-part
/// header, the three budgets, Annex I's channel plan and the precision and
/// channel-count refusals — so a file this returns `Ok` for is refused later
/// only for damage inside its packets, which is a statement about the bit
/// stream rather than about anything the file declared. What it does not do is
/// the work: no packet is parsed, no code-block decoded and no plane
/// allocated.
///
/// # Errors
/// The [`Refusal`] a decode would have stopped on at the same stage.
pub fn jpx_header(input: &[u8], limits: &Limits) -> Result<JpxHeader, Refusal> {
    let container = boxes::parse(input)?;
    let stream = codestream::parse(container.codestream)?;
    stream.check_budget(limits)?;
    let header = container.header.as_ref();
    let plan = colour::plan(header, &stream.siz.components)?;
    if plan.precision > MAX_JPX_PRECISION {
        return Err(Refusal::Precision(plan.precision));
    }
    if plan.colour.len() > MAX_JPX_COMPONENTS as usize {
        return Err(Refusal::Budget("output colour channels"));
    }
    let precision = if plan.precision > 8 { 16u8 } else { 8 };
    let width = stream.siz.width();
    let height = stream.siz.height();
    // The same output charge `decode_inner` makes, so a file whose raster the
    // caller's ceiling would refuse is refused here rather than placed and
    // then refused by the renderer.
    let channels = plan.colour.len() + usize::from(plan.opacity.is_some());
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|n| n.checked_mul(channels as u64))
        .and_then(|n| n.checked_mul(u64::from(precision / 8)))
        .ok_or(Refusal::Budget("output samples"))?;
    if bytes > limits.max_output as u64 {
        return Err(Refusal::Budget("the caller's output ceiling"));
    }
    Ok(JpxHeader {
        width,
        height,
        components: u8::try_from(plan.colour.len())
            .map_err(|_| Refusal::Budget("output colour channels"))?,
        precision,
        colour: header.map_or(JpxColour::Unstated, |h| h.colour),
        opacity: plan.opacity.is_some(),
    })
}

fn decode_inner(
    input: &[u8],
    limits: &Limits,
    request: &JpxRequest,
    clamped: &mut bool,
    counts: &mut JpxCounts,
    steps: &mut u64,
) -> Result<JpxImage, Refusal> {
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
    let mut tiles = tier2::decode_tiles_counted(&stream, steps)?;
    // The levels dropped: the request's, clamped to the fewest any
    // tile-component has, so every plane comes out at one scale and no
    // tile-component is asked for a resolution it does not have.
    let fewest = tiles
        .iter()
        .flat_map(|tile| tile.components.iter())
        .map(|component| component.resolutions.len().saturating_sub(1))
        .min()
        .unwrap_or(0);
    let discard = request
        .discard
        .min(u8::try_from(fewest).unwrap_or(MAX_JPX_LEVELS));
    // Tier-1 runs here for the same reason tier-2 does: it is where the
    // second integrity check lives. A stream that signalled segmentation
    // symbols has told the decoder what every cleanup pass must end with, so
    // a code-block that has drifted is named here rather than handed on as
    // plausible coefficients. A dropped resolution's code-blocks are not
    // decoded at all.
    // T.800 B.5: the image a decode dropping `discard` levels returns lies
    // on the reference grid reduced by `2^discard`, every coordinate
    // `ceil(x / 2^discard)` — the image area's and each tile's alike. The
    // window is a rectangle of it, and each tile the window reaches needs of
    // each of its components what the window reads (`wavelet::Needs`):
    // a tile it misses is neither decoded nor synthesised. Tier-2 has read
    // every packet header, as it must to find the packets after them.
    let grid = Grid::reduced(&stream.siz, discard);
    let window = grid.window(request.window);
    let needs: Option<Vec<Option<Vec<wavelet::Needs>>>> = match request.window {
        None => None,
        Some(_) => Some(
            tiles
                .iter()
                .map(|tile| tile_needs(&stream, tile, &grid, window, discard))
                .collect::<Result<_, _>>()?,
        ),
    };
    counts.code_blocks_decoded +=
        tier1::decode_tiles_counted(&stream, &mut tiles, discard, needs.as_deref(), steps)?;

    // Annex I's boxes and Annex A's components together decide what a channel
    // *is*, and that has to be settled before a single sample is written: a
    // `pclr` palette turns one component into three or four channels, so the
    // output is not the shape the codestream's component count suggests.
    let header = container.header.as_ref();
    let plan = colour::plan(header, &stream.siz.components)?;
    let palette = header.and_then(|h| h.palette.as_ref());
    if plan.precision > MAX_JPX_PRECISION {
        return Err(Refusal::Precision(plan.precision));
    }
    // **The output is 8 or 16 bits and nothing else**, which is what
    // `JpxImage::precision` has always documented and what ISO 32000-1's
    // sample path can carry. T.800 allows any precision from 1 to 38 and this
    // build accepts 1 to 16, so a 6-bit or 12-bit component is an ordinary
    // file rather than a curiosity -- 12-bit in particular is what medical and
    // geospatial imagery is, which is the whole domain this decoder was built
    // for. Widening at the boundary is therefore the answer and refusing is
    // not.
    //
    // Until milestone 8 the codestream's own precision was handed out
    // unchanged, and a fuzz session found it: a 6-bit image came back with
    // `precision: 6` and samples in 0..=63, which `resources.rs` then read as
    // 8-bit and drew at a quarter of its brightness. A picture, in other
    // words, and a plausible one -- this codec's whole hazard, reached not
    // through any of the wavelet arithmetic but through a field in SIZ.
    let precision = if plan.precision > 8 { 16 } else { 8 };
    if plan.colour.len() > MAX_JPX_COMPONENTS as usize {
        return Err(Refusal::Budget("output colour channels"));
    }

    let (width, height) = (window.x1 - window.x0, window.y1 - window.y0);
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

    for (t, tile) in tiles.iter().enumerate() {
        // A tile the window misses is neither synthesised nor written.
        let need: Option<&Vec<wavelet::Needs>> = match &needs {
            None => None,
            Some(needs) => match needs.get(t) {
                Some(Some(need)) => Some(need),
                _ => continue,
            },
        };
        // Every component of the tile at once, because G.2's transform reads
        // three planes and applying it one plane at a time is not a thing
        // that can be done.
        let mut planes =
            wavelet::reconstruct_tile(&stream, tile, discard, need.map(Vec::as_slice), clamped)?;
        counts.levels_synthesised += wavelet::levels_synthesised(tile, discard);
        let reconstructed = planes
            .iter()
            .map(|plane| plane.samples.len())
            .fold(0usize, usize::saturating_add);
        *steps = steps.saturating_add(u64::try_from(reconstructed).unwrap_or(u64::MAX));
        for (c, plane) in planes.iter_mut().enumerate() {
            let component = &stream.siz.components[c];
            // G.1, and *after* G.2 rather than before it — which is what
            // `reconstruct_tile` having already run makes true here. Over
            // the samples the window reads, or every sample.
            let region = need.and_then(|need| need.get(c)).map(wavelet::Needs::top);
            wavelet::level_shift_in(plane, component.precision, component.signed, region);
        }
        let bounds = window.and(&grid.tile(stream.siz.tile_bounds(u32::from(tile.index))));
        if bounds.is_empty() {
            continue;
        }
        for (k, channel) in plan.colour.iter().enumerate() {
            blit(
                &planes,
                &stream.siz,
                &window,
                bounds,
                *channel,
                palette,
                Blit {
                    out: &mut samples,
                    stride,
                    at: k,
                    precision,
                },
            );
        }
        if let (Some(channel), Some(out)) = (plan.opacity, opacity.as_mut()) {
            blit(
                &planes,
                &stream.siz,
                &window,
                bounds,
                channel,
                palette,
                Blit {
                    out,
                    stride: 1,
                    at: 0,
                    precision,
                },
            );
        }
    }

    let returned = samples
        .len()
        .saturating_add(opacity.as_ref().map_or(0, Vec::len));
    *steps = steps.saturating_add(u64::try_from(returned).unwrap_or(u64::MAX));
    Ok(JpxImage {
        width,
        height,
        components: u8::try_from(stride).map_err(|_| Refusal::Budget("output colour channels"))?,
        precision,
        samples,
        truncated: stream.short_tiles.iter().any(|short| *short),
        colour: header.map_or(JpxColour::Unstated, |h| h.colour),
        opacity: opacity.map(|samples| JpxOpacity {
            premultiplied: plan.premultiplied,
            samples,
        }),
        discard,
    })
}

/// The image area on the reference grid a decode writes into: the full grid
/// at no discard, and T.800 B.5's grid reduced by `2^d` at a discard of `d`.
struct Grid {
    /// The reduced `XOsiz` and `YOsiz`: where the image area starts.
    x0: u32,
    y0: u32,
    /// The reduced image's size.
    width: u32,
    height: u32,
    discard: u8,
}

impl Grid {
    /// `siz`'s image area reduced by `2^discard`.
    fn reduced(siz: &codestream::Siz, discard: u8) -> Grid {
        let (x0, y0) = (reduce(siz.xosiz, discard), reduce(siz.yosiz, discard));
        Grid {
            x0,
            y0,
            width: reduce(siz.xsiz, discard).saturating_sub(x0),
            height: reduce(siz.ysiz, discard).saturating_sub(y0),
            discard,
        }
    }

    /// A tile's bounds on the full reference grid, reduced as the image's
    /// are.
    fn tile(&self, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> wavelet::Rect {
        let d = self.discard;
        wavelet::Rect {
            x0: reduce(x0, d),
            y0: reduce(y0, d),
            x1: reduce(x1, d),
            y1: reduce(y1, d),
        }
    }

    /// `asked` on this grid, clamped to the image: the whole image for no
    /// window, and an empty rectangle for one that holds no pixel of it.
    fn window(&self, asked: Option<JpxWindow>) -> wavelet::Rect {
        let Some(asked) = asked else {
            return wavelet::Rect {
                x0: self.x0,
                y0: self.y0,
                x1: self.x0 + self.width,
                y1: self.y0 + self.height,
            };
        };
        // In `u64`, since a caller's corner plus its size may pass `u32`.
        let side = |at: u32, size: u32, extent: u32| {
            let start = u64::from(at).min(u64::from(extent));
            let end = (u64::from(at) + u64::from(size)).min(u64::from(extent));
            (start as u32, end as u32)
        };
        let (x0, x1) = side(asked.x, asked.width, self.width);
        let (y0, y1) = side(asked.y, asked.height, self.height);
        if x0 >= x1 || y0 >= y1 {
            return wavelet::Rect::default();
        }
        wavelet::Rect {
            x0: self.x0 + x0,
            y0: self.y0 + y0,
            x1: self.x0 + x1,
            y1: self.y0 + y1,
        }
    }
}

/// What a window needs of each component of `tile`, or `None` for a tile it
/// misses.
///
/// A component's need at its top kept resolution is what the blit reads:
/// reduced reference sample `x` of the tile's part of the window reads
/// component sample `floor(x / XRsiz)`, clamped into the plane, so the
/// samples are the clamped images of the part's first and last columns and
/// everything between. Each component takes its own filter's reach.
fn tile_needs(
    stream: &codestream::Codestream<'_>,
    tile: &tier2::Tile,
    grid: &Grid,
    window: wavelet::Rect,
    discard: u8,
) -> Result<Option<Vec<wavelet::Needs>>, Refusal> {
    let part = window.and(&grid.tile(stream.siz.tile_bounds(u32::from(tile.index))));
    if part.is_empty() {
        return Ok(None);
    }
    let mut out = Vec::with_capacity(tile.components.len());
    for (c, tc) in tile.components.iter().enumerate() {
        let component = stream.siz.components.get(c).ok_or(Refusal::Structure(
            "a component the SIZ marker does not declare",
        ))?;
        let (dx, dy) = (
            u32::from(component.dx).max(1),
            u32::from(component.dy).max(1),
        );
        let kept = tc
            .resolutions
            .len()
            .saturating_sub(usize::from(discard))
            .max(1);
        let reach = wavelet::reach(wavelet::reversible(stream, tile.index, c)?);
        let top = tc
            .resolutions
            .get(kept - 1)
            .map_or(wavelet::Rect::default(), |res| {
                if res.x0 >= res.x1 || res.y0 >= res.y1 {
                    return wavelet::Rect::default();
                }
                let across = |v: u32| v.clamp(res.x0, res.x1 - 1);
                let down = |v: u32| v.clamp(res.y0, res.y1 - 1);
                wavelet::Rect {
                    x0: across(part.x0 / dx),
                    y0: down(part.y0 / dy),
                    x1: across((part.x1 - 1) / dx) + 1,
                    y1: down((part.y1 - 1) / dy) + 1,
                }
            });
        out.push(wavelet::Needs::of(tc, discard, top, reach));
    }
    Ok(Some(out))
}

/// `ceil(v / 2^d)`, B.5's reduction of one reference-grid coordinate. `d` is
/// at most [`MAX_JPX_LEVELS`], so the divisor fits `u64`.
fn reduce(v: u32, d: u8) -> u32 {
    let divisor = 1u64 << d.min(MAX_JPX_LEVELS);
    // The quotient of a `u32` by at least one fits `u32`.
    u64::from(v).div_ceil(divisor) as u32
}

/// Where one channel's samples are going.
struct Blit<'a> {
    out: &'a mut [u8],
    /// Channels per pixel in `out`.
    stride: usize,
    /// This channel's index within a pixel.
    at: usize,
    /// The output precision, 8 or 16, that [`normalise`] widens each sample
    /// to from its own channel's.
    ///
    /// The bytes per sample come from it rather than riding beside it: two
    /// numbers that must agree are a chance for them to disagree, and the
    /// buffer this writes into was sized from the same rule.
    precision: u8,
}

/// One decoded sample, moved out of its channel's domain and into the
/// output's.
///
/// Two things happen and both are corrections rather than choices.
///
/// **The sign.** T.800 G.1's level shift moves an *unsigned* component into
/// range and deliberately leaves a signed one centred on zero, because that is
/// where the wavelet wants it. PDF's sample path has no signed samples at all,
/// so the shift G.1 did not do happens here. Without it a signed component's
/// negative half wraps through `as u32` and comes back as the top half of the
/// range: mid-grey renders black and black renders mid-grey, in a picture that
/// is otherwise correct.
///
/// **The width.** `from` is what SIZ or the `pclr` box declared, anywhere in
/// 1 to 16; `to` is 8 or 16, because that is what `JpxImage` carries and what
/// `/BitsPerComponent` can be. The scaling is the full-range one --
/// `v * (2^to - 1) / (2^from - 1)`, rounded to nearest -- so 0 maps to 0 and
/// the largest input maps to the largest output, which a shift would not do:
/// `v << 2` on a 6-bit sample tops out at 252 and leaves every image it
/// touches unable to reach white.
///
/// It is an identity when `from == to`, which is every fixture in this
/// repository and every one of gap 23's nineteen files, so nothing that
/// already decoded moves.
fn normalise(value: i32, from: u8, to: u8, signed: bool) -> u32 {
    // Both are `(Ssiz & 0x7F) + 1` or a `pclr` `Bi` read the same way, and
    // both are refused above 16 where they are read. Stated because the two
    // things below would be a shift by minus one and a division by zero if a
    // later reader let a zero through, and neither would look like this
    // function's fault (ruling 1).
    debug_assert!((1..=16).contains(&from) && (to == 8 || to == 16));
    let shifted = if signed {
        value.saturating_add(1i32 << (i32::from(from) - 1))
    } else {
        value
    };
    let in_max = (1u32 << from) - 1;
    let out_max = (1u32 << to) - 1;
    #[allow(
        clippy::cast_sign_loss,
        reason = "clamped into 0..=in_max on the line above"
    )]
    let v = shifted.clamp(0, in_max as i32) as u32;
    // `in_max` is at most 65 535 and `out_max` at most 65 535, so the product
    // is under 2^32 and the arithmetic needs no widening.
    (v * out_max + in_max / 2) / in_max
}

/// Write one channel of one tile onto the reference grid, replicating a
/// subsampled component.
///
/// **Replication, not filtering**, and the reason is the oracle rather than
/// taste: `opj_decompress -upsample` takes the sample at `floor(X / XRsiz)`
/// unfiltered, and gate 1 asks for byte-identity against it. A triangle
/// filter — which is what plan 02 chose for JPEG, where
/// the oracle is libjpeg-turbo and libjpeg-turbo interpolates — would put
/// every edge pixel a level or two out and the gate would have to become a
/// tolerance.
///
/// The clamp on the way in is the same rule one step further: `XRsiz` need
/// not divide the tile's origin, so the first reference-grid column of a tile
/// can map below the component's own first column. Reading the nearest real
/// sample is what replication means at an edge; reading zero would put a
/// black line down the left of every such tile.
///
/// On a reduced grid (a discard of `d`) the same rule runs at the lower
/// scale: the plane is resolution `N_L - d`'s, and a reduced reference
/// sample `x` reads component sample `floor(x / XRsiz)` of it, as the full
/// grid's `x` reads `floor(x / XRsiz)` of the full plane. `bounds` is the part
/// of the tile, reduced as B.5 reduces it, that `window` holds, and the
/// samples are written from the window's own top-left corner.
fn blit(
    planes: &[wavelet::Plane],
    siz: &codestream::Siz,
    window: &wavelet::Rect,
    bounds: wavelet::Rect,
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
    let (width, height) = (window.x1 - window.x0, window.y1 - window.y0);
    for y in bounds.y0..bounds.y1 {
        let Some(py) = y.checked_sub(window.y0) else {
            continue;
        };
        if py >= height {
            continue;
        }
        let cy = (y / dy).saturating_sub(plane.y0).min(plane.height - 1);
        for x in bounds.x0..bounds.x1 {
            let Some(px) = x.checked_sub(window.x0) else {
                continue;
            };
            if px >= width {
                continue;
            }
            let cx = (x / dx).saturating_sub(plane.x0).min(plane.width - 1);
            let value = channel.lookup(palette, plane.at(cx, cy));
            let index = ((py as usize) * (width as usize) + (px as usize)) * to.stride + to.at;
            let value = normalise(value, channel.precision, to.precision, channel.signed);
            if to.precision > 8 {
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
