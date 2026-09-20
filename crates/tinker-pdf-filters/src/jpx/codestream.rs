//! The JPEG 2000 codestream syntax (ITU-T T.800 Annex A).
//!
//! A codestream is SOC, a main header of marker segments, one or more
//! tile-parts each opening with SOT and ending its header with SOD, and EOC.
//! Every marker is `0xFF` followed by a byte above `0x8F`, and all but five
//! of them carry a two-byte length that includes itself.
//!
//! # Every marker in Table A.2, or a refusal that names it
//!
//! There are twenty markers in Table A.2 and this module accounts for all
//! twenty: sixteen are parsed, three are refused **by name** (POC, PPM, PPT)
//! and one — EPH — belongs to tier-2 and is refused here only when
//! it appears in a header. Anything else is refused as an unknown marker,
//! which is where every ISO/IEC 15444-2 marker lands, Part 2 being a
//! non-goal.
//!
//! Skipping a marker whose length happens to be readable is the failure this
//! guards against, and it is not hypothetical: POC changes the progression
//! order *mid-stream*, so a decoder that skips it reads every packet after it
//! in the wrong order and produces a soft, plausible image.
//!
//! RGN was the fourth refusal until Annex H was read. It is parsed here and
//! applied in [`super::wavelet`], because A.6.3 carries nothing but a
//! component index, a style and a shift, and H.1 says exactly what the shift
//! does to a coefficient. The refusal it leaves behind is narrower and named:
//! Table A.25 defines one `Srgn` style and reserves every other value, so a
//! reserved style is refused as a reserved style rather than stepped over.
//!
//! # Where the arithmetic on attacker numbers is
//!
//! A.5.1's tile grid. `XTsiz` is a divisor and `XOsiz` is a subtrahend, so a
//! zero `XTsiz` is a division by zero and `XOsiz > Xsiz` is an underflow.
//! Every one of A.5.1's constraints is checked here rather than assumed, and
//! each violation refuses the file.

use super::{
    Cursor, Refusal, MAX_JPX_COMPONENTS, MAX_JPX_LEVELS, MAX_JPX_PRECISION, MAX_JPX_SAMPLES,
    MAX_JPX_TILES,
};
use crate::Limits;

/// ITU-T T.800 Table A.2, in full. Every one of these is parsed below or
/// named in a refusal below; none is skipped.
pub(crate) mod marker {
    /// Start of codestream (A.4.1). No segment.
    pub const SOC: u16 = 0xFF4F;
    /// Start of tile-part (A.4.2).
    pub const SOT: u16 = 0xFF90;
    /// Start of data (A.4.3). No segment; the tile-part's packets follow.
    pub const SOD: u16 = 0xFF93;
    /// End of codestream (A.4.4). No segment.
    pub const EOC: u16 = 0xFFD9;
    /// Image and tile size (A.5.1).
    pub const SIZ: u16 = 0xFF51;
    /// Coding style default (A.6.1).
    pub const COD: u16 = 0xFF52;
    /// Coding style component (A.6.2).
    pub const COC: u16 = 0xFF53;
    /// Region of interest (A.6.3). **Parsed and applied**: it carries the
    /// Maxshift scaling value `s`, and H.1 realigns every coefficient of the
    /// component against it. Ignoring it leaves the background 2^s too
    /// small — a picture, not an error.
    pub const RGN: u16 = 0xFF5E;
    /// Quantization default (A.6.4).
    pub const QCD: u16 = 0xFF5C;
    /// Quantization component (A.6.5).
    pub const QCC: u16 = 0xFF5D;
    /// Progression order change (A.6.6). **Refused**: it changes the packet
    /// order mid-stream, so ignoring it mis-parses every packet after it.
    pub const POC: u16 = 0xFF5F;
    /// Tile-part lengths (A.7.1). An index; skipping it changes nothing.
    pub const TLM: u16 = 0xFF55;
    /// Packet length, main header (A.7.2). An index.
    pub const PLM: u16 = 0xFF57;
    /// Packet length, tile-part header (A.7.3). An index.
    pub const PLT: u16 = 0xFF58;
    /// Packed packet headers, main header (A.7.4). **Refused**: it moves
    /// every packet header out of the packets, so ignoring it means reading
    /// packet bodies as headers.
    pub const PPM: u16 = 0xFF60;
    /// Packed packet headers, tile-part header (A.7.5). **Refused**, as PPM.
    pub const PPT: u16 = 0xFF61;
    /// Start of packet (A.8.1). Inside the tile data, not the header.
    pub const SOP: u16 = 0xFF91;
    /// End of packet header (A.8.2). Inside the tile data. No segment.
    pub const EPH: u16 = 0xFF92;
    /// Component registration (A.9.1). **Parsed and carried, never applied**:
    /// A.9.1 says in as many words that it "has no effect on decoding the
    /// codestream", so honouring it means reading it and leaving the samples
    /// alone.
    pub const CRG: u16 = 0xFF63;
    /// Comment (A.9.2). Carries no coding information.
    pub const COM: u16 = 0xFF64;
}

/// The name a refusal gives a Table A.2 marker this build decodes no further.
///
/// Every entry is a marker the standard defines and this decoder deliberately
/// will not act on, and the name is what makes "refused by name" checkable.
const fn refused_by_design(code: u16) -> Option<&'static str> {
    Some(match code {
        // RGN left this list when Annex H was implemented. What is refused
        // now is a *value* inside the segment rather than the segment —
        // `Refusal::Feature("an Srgn ROI style Table A.25 reserves")`, from
        // `parse_rgn` — because Table A.25 defines only style 0.
        marker::POC => "POC, progression order change",
        marker::PPM => "PPM, packed packet headers in the main header",
        marker::PPT => "PPT, packed packet headers in a tile-part header",
        marker::SOP => "SOP outside tile data",
        marker::EPH => "EPH outside tile data",
        _ => return None,
    })
}

/// Whether T.800 Table A.2 defines this code at all.
///
/// The distinction matters to a reader of the warning: "a marker this build
/// does not decode" and "a marker nothing defines" are different problems
/// with different fixes, and collapsing them would have called a misplaced
/// SOC an unknown marker.
pub(crate) const fn in_table_a2(code: u16) -> bool {
    matches!(
        code,
        marker::SOC
            | marker::SOT
            | marker::SOD
            | marker::EOC
            | marker::SIZ
            | marker::COD
            | marker::COC
            | marker::RGN
            | marker::QCD
            | marker::QCC
            | marker::POC
            | marker::TLM
            | marker::PLM
            | marker::PLT
            | marker::PPM
            | marker::PPT
            | marker::SOP
            | marker::EPH
            | marker::CRG
            | marker::COM
    )
}

/// T.800 Table A.16, the five progression orders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Progression {
    /// Layer, resolution, component, position.
    Lrcp,
    /// Resolution, layer, component, position.
    Rlcp,
    /// Resolution, position, component, layer.
    Rpcl,
    /// Position, component, resolution, layer.
    Pcrl,
    /// Component, position, resolution, layer.
    Cprl,
}

impl Progression {
    fn from_byte(b: u8) -> Result<Progression, Refusal> {
        Ok(match b {
            0 => Progression::Lrcp,
            1 => Progression::Rlcp,
            2 => Progression::Rpcl,
            3 => Progression::Pcrl,
            4 => Progression::Cprl,
            _ => {
                return Err(Refusal::Feature(
                    "a progression order Table A.16 does not define",
                ))
            }
        })
    }
}

/// T.800 Table A.19, the code-block style bits.
///
/// Five of the six change how tier-1 reads a code-block and are refused; the
/// sixth, `SEGMENTATION_SYMBOLS`, is implemented, because decoding the four
/// UNIFORM decisions at the end of each cleanup pass and **checking** them is
/// the one integrity check the format offers for free. A build that decoded
/// them and discarded them would have thrown away the only thing in JPEG 2000
/// that says the arithmetic decoder has gone out of step.
pub(crate) mod cb_style {
    /// Selective arithmetic coding bypass: raw bits instead of MQ decisions
    /// for some passes.
    pub const BYPASS: u8 = 0x01;
    /// Reset the context probabilities on each coding pass.
    pub const RESET: u8 = 0x02;
    /// Terminate the arithmetic coder on each coding pass.
    pub const TERMALL: u8 = 0x04;
    /// Vertically causal context: the stripe below is treated as
    /// insignificant when forming a context.
    pub const VERTICALLY_CAUSAL: u8 = 0x08;
    /// Predictable termination.
    pub const PREDICTABLE: u8 = 0x10;
    /// Segmentation symbols at the end of each cleanup pass (D.5).
    pub const SEGMENTATION_SYMBOLS: u8 = 0x20;
    /// The two this build refuses, and they are refused together for one
    /// reason: both change where a coding pass's *bytes* are, not how its
    /// decisions are read. `TERMALL` restarts the arithmetic coder at every
    /// pass and `BYPASS` codes some passes as raw bits, so a decoder needs a
    /// length per pass rather than one per code-block — which is a packet
    /// header change (B.10.7's multiple codeword segments), not a tier-1 one.
    /// The other three are decisions about context state and are implemented.
    pub const UNSUPPORTED: u8 = BYPASS | TERMALL;
    /// Every bit Table A.19 defines. A `Scod` byte with anything outside
    /// this is a codestream using a table this build has not read.
    ///
    /// Enumerated rather than derived from [`UNSUPPORTED`]. It was
    /// `UNSUPPORTED | SEGMENTATION_SYMBOLS`, which was the same set only while
    /// this build refused five of the six — so implementing three of them
    /// dropped them out of *defined* as well, and a codestream setting nothing
    /// but `RESET` was refused for using a bit the table does not define. The
    /// two sets answer different questions and now say so separately.
    pub const DEFINED: u8 =
        BYPASS | RESET | TERMALL | VERTICALLY_CAUSAL | PREDICTABLE | SEGMENTATION_SYMBOLS;
}

/// One component's registration offset, from CRG (A.9.1).
///
/// `x` is in units of 1/65536 of that component's own horizontal separation
/// `XRsiz`, so the offset in reference grid points is `dx * x / 65536` — the
/// units are the component's separation and not a reference grid point, which
/// is the field in this segment most easily got wrong.
///
/// **Carried and never applied.** A.9.1: "This marker segment has no effect on
/// decoding the codestream." It describes the centre of mass of a component's
/// samples for a renderer that wants to place them more precisely than the
/// reference grid does; the samples themselves are unchanged, so a decoder
/// that reads this and does nothing with it is a conforming one. Parsing it
/// rather than refusing it is what stops a conforming file being refused for
/// carrying information.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Registration {
    /// Horizontal offset, in units of 1/65536 of `XRsiz`.
    pub(crate) x: u16,
    /// Vertical offset, in units of 1/65536 of `YRsiz`.
    pub(crate) y: u16,
}

/// One component's geometry from SIZ (A.5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Component {
    /// Bit depth, 1 to 16. Above 16 is refused rather than truncated.
    pub(crate) precision: u8,
    pub(crate) signed: bool,
    /// `XRsiz`, the horizontal separation on the reference grid. At least 1.
    pub(crate) dx: u8,
    /// `YRsiz`.
    pub(crate) dy: u8,
}

/// SIZ (A.5.1): the reference grid, the tile grid and the components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Siz {
    pub(crate) xsiz: u32,
    pub(crate) ysiz: u32,
    pub(crate) xosiz: u32,
    pub(crate) yosiz: u32,
    pub(crate) xtsiz: u32,
    pub(crate) ytsiz: u32,
    pub(crate) xtosiz: u32,
    pub(crate) ytosiz: u32,
    pub(crate) components: Vec<Component>,
    /// `numXtiles` (A.5.1's equation A-4), computed once here so nothing
    /// downstream divides by `xtsiz` again.
    pub(crate) tiles_x: u32,
    pub(crate) tiles_y: u32,
}

impl Siz {
    /// The image's width on the reference grid.
    pub(crate) const fn width(&self) -> u32 {
        self.xsiz - self.xosiz
    }

    /// The image's height on the reference grid.
    pub(crate) const fn height(&self) -> u32 {
        self.ysiz - self.yosiz
    }

    /// Tile `t`'s bounds on the reference grid (A.5.1, equations A-5..A-8),
    /// clipped to the image.
    pub(crate) fn tile_bounds(&self, t: u32) -> (u32, u32, u32, u32) {
        let (px, py) = (t % self.tiles_x, t / self.tiles_x);
        // Every one of these cannot overflow: `tiles_x` was derived from
        // `xtsiz` so `xtosiz + px * xtsiz <= xsiz + xtsiz`, and `xtsiz` fits
        // `u32`, so the product is computed in `u64` and clipped back.
        let x0 = (u64::from(self.xtosiz) + u64::from(px) * u64::from(self.xtsiz))
            .max(u64::from(self.xosiz))
            .min(u64::from(self.xsiz)) as u32;
        let x1 = (u64::from(self.xtosiz) + u64::from(px + 1) * u64::from(self.xtsiz))
            .min(u64::from(self.xsiz)) as u32;
        let y0 = (u64::from(self.ytosiz) + u64::from(py) * u64::from(self.ytsiz))
            .max(u64::from(self.yosiz))
            .min(u64::from(self.ysiz)) as u32;
        let y1 = (u64::from(self.ytosiz) + u64::from(py + 1) * u64::from(self.ytsiz))
            .min(u64::from(self.ysiz)) as u32;
        (x0, y0, x1, y1)
    }

    /// Tile-component `(t, c)`'s bounds, which are the tile's bounds divided
    /// by the component's subsampling (B.2, equation B-12).
    pub(crate) fn tile_component_bounds(&self, t: u32, c: usize) -> (u32, u32, u32, u32) {
        let (x0, y0, x1, y1) = self.tile_bounds(t);
        let Some(comp) = self.components.get(c) else {
            return (0, 0, 0, 0);
        };
        let (dx, dy) = (u32::from(comp.dx).max(1), u32::from(comp.dy).max(1));
        (
            x0.div_ceil(dx),
            y0.div_ceil(dy),
            x1.div_ceil(dx),
            y1.div_ceil(dy),
        )
    }
}

/// The part of a coding style that COD and COC both carry (A.6.1's SPcod and
/// A.6.2's SPcoc, which are the same fields).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CodingStyle {
    /// `NL`, decomposition levels. Resolutions are `levels + 1`.
    pub(crate) levels: u8,
    /// `xcb`, the code-block width exponent. 2 to 10.
    pub(crate) cb_width: u8,
    /// `ycb`.
    pub(crate) cb_height: u8,
    /// Table A.19's bits, after the unsupported ones have been refused.
    pub(crate) cb_style: u8,
    /// The 5/3 reversible transform, rather than the 9/7 irreversible one.
    pub(crate) reversible: bool,
    /// `(PPx, PPy)` per resolution, lowest first, `levels + 1` of them.
    /// A.6.1's default when `Scod` bit 0 is clear is 15 for every resolution,
    /// which is a precinct larger than any code-block partition and is what
    /// makes "no precincts" and "one precinct" the same thing.
    pub(crate) precincts: Vec<(u8, u8)>,
}

impl CodingStyle {
    /// Whether the segmentation symbols of D.5 end every cleanup pass.
    ///
    /// Read by tier-1: decoding the `1010` in UNIFORM and *checking* it is a
    /// free per-code-block verification that the MQ decoder is still in step,
    /// and the plan calls discarding it throwing away the one integrity check
    /// the format offers. `Refusal::SegmentationSymbol` is what it fires.
    pub(crate) const fn segmentation_symbols(&self) -> bool {
        self.cb_style & cb_style::SEGMENTATION_SYMBOLS != 0
    }

    /// A.6.1 Table A.19 bit 1: the context states return to Table D.7's at
    /// every coding pass boundary rather than only at the code-block's start.
    pub(crate) const fn reset_contexts(&self) -> bool {
        self.cb_style & cb_style::RESET != 0
    }

    /// Table A.19 bit 3: context formation treats the stripe below the one
    /// being coded as insignificant, so a stripe depends on nothing beneath
    /// it.
    pub(crate) const fn vertically_causal(&self) -> bool {
        self.cb_style & cb_style::VERTICALLY_CAUSAL != 0
    }

    /// `(PPx, PPy)` at resolution `r`.
    pub(crate) fn precinct_exponents(&self, r: usize) -> (u8, u8) {
        self.precincts.get(r).copied().unwrap_or((15, 15))
    }

    /// The code-block exponents actually used at resolution `r`, after B.7's
    /// clamp against the precinct.
    ///
    /// A precinct is never smaller than the code-blocks inside it. At
    /// resolution 0 the precinct partition maps one-to-one onto the single LL
    /// subband; above it, a precinct in *resolution* coordinates covers half
    /// as much in each of the three subbands, so the code-block exponent is
    /// clamped against `PPx - 1` rather than `PPx`. Dropping that minus one
    /// produces the right number of code-blocks in the wrong places, which is
    /// a picture rather than an error.
    pub(crate) fn code_block_exponents(&self, r: usize) -> (u8, u8) {
        let (ppx, ppy) = self.precinct_exponents(r);
        let (ppx, ppy) = if r == 0 {
            (ppx, ppy)
        } else {
            (ppx.saturating_sub(1), ppy.saturating_sub(1))
        };
        (self.cb_width.min(ppx), self.cb_height.min(ppy))
    }
}

/// COD (A.6.1): the coding style, defaulting every component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cod {
    pub(crate) progression: Progression,
    pub(crate) layers: u16,
    /// `SGcod`'s multiple component transform: the RCT for a reversible
    /// stream and the ICT for an irreversible one.
    pub(crate) mct: bool,
    /// SOP marker segments delimit the packets.
    pub(crate) sop: bool,
    /// EPH markers end the packet headers.
    pub(crate) eph: bool,
    pub(crate) style: CodingStyle,
}

/// T.800 Table A.28's quantisation styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QuantStyle {
    /// No quantisation: reversible, and `SPqcd` carries exponents only.
    None,
    /// Scalar derived — one step size, from which the rest are derived
    /// (E.1.1's equation E-5).
    Derived,
    /// Scalar expounded — one step size per subband.
    Expounded,
}

/// QCD (A.6.4) or QCC (A.6.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Quant {
    pub(crate) style: QuantStyle,
    /// `Sqcd`'s top three bits. E.1 lets a coder carry this many bits above
    /// the nominal dynamic range, and it is attacker-controlled.
    pub(crate) guard_bits: u8,
    /// `(exponent, mantissa)` per subband. The mantissa is zero for
    /// [`QuantStyle::None`].
    pub(crate) steps: Vec<(u8, u16)>,
}

/// RGN (A.6.3 and Table A.26): one component's region-of-interest shift.
///
/// `Srgn` is not carried, and that is deliberate rather than an omission.
/// Table A.25 gives exactly one ROI style — 0, "Implicit ROI (maximum
/// shift)" — and reserves every other value, so [`parse_rgn`] refuses
/// anything else and what survives is the one style's own parameter. A field
/// that can only hold one value is a field a reader has to check.
///
/// `shift` is Table A.26's "implicit ROI shift", 0 to 255: the number of
/// binary places the encoder put the ROI's coefficients above the
/// background's, which H.1 calls `s`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Roi {
    pub(crate) shift: u8,
}

/// One tile-part: its SOT fields, whatever its header overrode, and its data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TilePart<'a> {
    /// `Isot`, the tile this part belongs to.
    pub(crate) tile: u16,
    /// `TPsot`, this part's index within the tile.
    pub(crate) index: u8,
    /// `TNsot`, the number of parts. Zero means "not yet known", which A.4.2
    /// permits only in the first part.
    pub(crate) parts: u8,
    /// The tile-part header's COD, if it carried one. A.6.1 permits it only
    /// in the first tile-part of a tile.
    pub(crate) cod: Option<Cod>,
    pub(crate) coc: Vec<(u16, CodingStyle)>,
    pub(crate) qcd: Option<Quant>,
    pub(crate) qcc: Vec<(u16, Quant)>,
    /// The tile-part header's RGN segments, by component. A.6.3 permits one
    /// per component and only in the first tile-part of a tile.
    pub(crate) rgn: Vec<(u16, Roi)>,
    /// Everything between SOD and the end of the tile-part: the packets.
    pub(crate) data: &'a [u8],
}

/// A parsed codestream: the main header and every tile-part in stream order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Codestream<'a> {
    /// Which tiles declared more parts than arrived, so are left blank.
    ///
    /// Empty in a complete codestream. See [`check_tile_parts`]: a tile that
    /// stops early costs pixels rather than meaning, and the caller reports it
    /// as damage rather than refusing the image.
    pub(crate) short_tiles: Vec<bool>,
    pub(crate) siz: Siz,
    pub(crate) cod: Cod,
    /// Per-component COC overrides from the main header.
    pub(crate) coc: Vec<Option<CodingStyle>>,
    pub(crate) qcd: Quant,
    /// Per-component QCC overrides from the main header.
    pub(crate) qcc: Vec<Option<Quant>>,
    /// Per-component RGN from the main header (A.6.3). `None` for a
    /// component with no region of interest, which is every component of
    /// almost every file.
    pub(crate) rgn: Vec<Option<Roi>>,
    pub(crate) tile_parts: Vec<TilePart<'a>>,
    /// Per-component registration offsets from CRG (A.9.1), when the main
    /// header carried one. Read and never applied — see [`Registration`].
    pub(crate) registration: Option<Vec<Registration>>,
}

impl Codestream<'_> {
    /// The coding style in force for component `c` of tile `t`.
    ///
    /// A.6.2's precedence, which is four deep and is the thing a decoder gets
    /// wrong quietly: a tile-part COC beats a tile-part COD beats a main
    /// header COC beats the main header COD. Applying the main-header default
    /// to every component decodes a chroma plane at a luma plane's
    /// decomposition depth — a picture, not an error.
    pub(crate) fn style_for(&self, tile: u16, c: usize) -> &CodingStyle {
        if let Some(part) = self.first_part(tile) {
            if let Some((_, style)) = part.coc.iter().find(|(i, _)| usize::from(*i) == c) {
                return style;
            }
            if let Some(cod) = &part.cod {
                return &cod.style;
            }
        }
        if let Some(Some(style)) = self.coc.get(c) {
            return style;
        }
        &self.cod.style
    }

    /// The COD in force for tile `t` — the progression, layer count and
    /// transform, which are per-tile but not per-component.
    pub(crate) fn cod_for(&self, tile: u16) -> &Cod {
        self.first_part(tile)
            .and_then(|p| p.cod.as_ref())
            .unwrap_or(&self.cod)
    }

    /// The quantisation in force for component `c` of tile `t`, by A.6.5's
    /// precedence, which mirrors A.6.2's.
    ///
    /// Read by milestone 4, which is where a coefficient stops being an
    /// integer from tier-1 and becomes a number with a magnitude.
    #[allow(dead_code, reason = "dequantisation, milestone 4")]
    pub(crate) fn quant_for(&self, tile: u16, c: usize) -> &Quant {
        if let Some(part) = self.first_part(tile) {
            if let Some((_, q)) = part.qcc.iter().find(|(i, _)| usize::from(*i) == c) {
                return q;
            }
            if let Some(q) = &part.qcd {
                return q;
            }
        }
        if let Some(Some(q)) = self.qcc.get(c) {
            return q;
        }
        &self.qcd
    }

    /// The region of interest in force for component `c` of tile `t`, by
    /// A.6.3's precedence.
    ///
    /// Two deep rather than [`Self::quant_for`]'s four, and the clause says
    /// so in as many words: "The RGN marker segment for a particular
    /// component which appears in a tile-part header overrides any marker for
    /// that component in the main header, for the tile in which it appears."
    /// There is no RGN default to fall back to — a component with no RGN
    /// anywhere has no region of interest, and H.1 then applies to nothing.
    ///
    /// **The main header's RGN is not inherited by a tile that has its own.**
    /// A.6.3's Usage paragraph says a main-header RGN is "valid for all tiles
    /// except those with an RGN marker segment", which is the override read
    /// from the other end, and a decoder that took the larger of the two, or
    /// added them, would be inventing an arithmetic the clause does not have.
    pub(crate) fn roi_for(&self, tile: u16, c: usize) -> Option<Roi> {
        if let Some(part) = self.first_part(tile) {
            if let Some((_, roi)) = part.rgn.iter().find(|(i, _)| usize::from(*i) == c) {
                return Some(*roi);
            }
        }
        self.rgn.get(c).copied().flatten()
    }

    fn first_part(&self, tile: u16) -> Option<&TilePart<'_>> {
        self.tile_parts
            .iter()
            .find(|p| p.tile == tile && p.index == 0)
    }

    /// The budget of [`super::MAX_JPX_SAMPLES`], and the caller's own
    /// ceiling, both spent before any plane exists.
    ///
    /// Tile-component samples summed over every tile and every component,
    /// with a checked multiply at each step. One tile inside any sane
    /// ceiling times T.800's legal 16 384 components is not inside anything,
    /// which is why the per-item caps cannot stand in for this one.
    pub(crate) fn check_budget(&self, limits: &Limits) -> Result<(), Refusal> {
        let mut total: u64 = 0;
        let tiles = u64::from(self.siz.tiles_x) * u64::from(self.siz.tiles_y);
        for t in 0..tiles {
            let t = u32::try_from(t).map_err(|_| Refusal::Budget("tiles"))?;
            for c in 0..self.siz.components.len() {
                let (x0, y0, x1, y1) = self.siz.tile_component_bounds(t, c);
                let w = u64::from(x1.saturating_sub(x0));
                let h = u64::from(y1.saturating_sub(y0));
                total = w
                    .checked_mul(h)
                    .and_then(|n| total.checked_add(n))
                    .ok_or(Refusal::Budget("tile-component samples"))?;
                if total > MAX_JPX_SAMPLES {
                    return Err(Refusal::Budget("tile-component samples"));
                }
            }
        }
        // The output the caller would have to hold: one or two bytes per
        // sample per colour component, over the whole reference grid.
        let bytes = u64::from(self.siz.width())
            .checked_mul(u64::from(self.siz.height()))
            .and_then(|n| n.checked_mul(self.siz.components.len() as u64))
            .and_then(|n| n.checked_mul(if self.max_precision() > 8 { 2 } else { 1 }))
            .ok_or(Refusal::Budget("output samples"))?;
        if bytes > limits.max_output as u64 {
            return Err(Refusal::Budget("the caller's output ceiling"));
        }
        Ok(())
    }

    pub(crate) fn max_precision(&self) -> u8 {
        self.siz
            .components
            .iter()
            .map(|c| c.precision)
            .max()
            .unwrap_or(8)
    }
}

/// Parses a whole codestream: SOC, the main header, every tile-part, EOC.
pub(crate) fn parse(data: &[u8]) -> Result<Codestream<'_>, Refusal> {
    let mut c = Cursor::new(data);
    if c.u16() != Some(marker::SOC) {
        return Err(Refusal::Structure(
            "a codestream that does not open with SOC",
        ));
    }

    let mut siz: Option<Siz> = None;
    let mut cod: Option<Cod> = None;
    let mut qcd: Option<Quant> = None;
    let mut coc: Vec<Option<CodingStyle>> = Vec::new();
    let mut qcc: Vec<Option<Quant>> = Vec::new();
    let mut rgn: Vec<Option<Roi>> = Vec::new();
    let mut registration: Option<Vec<Registration>> = None;
    let mut tile_parts: Vec<TilePart<'_>> = Vec::new();

    loop {
        let Some(code) = c.u16() else {
            return Err(Refusal::Truncated("a codestream marker"));
        };
        if code == marker::EOC {
            break;
        }
        if code == marker::SOT {
            let part = tile_part(&mut c, siz.as_ref(), cod.as_ref())?;
            if tile_parts.len() as u64 > MAX_JPX_TILES * 4 {
                return Err(Refusal::Budget("tile-parts"));
            }
            tile_parts.push(part);
            continue;
        }
        if !tile_parts.is_empty() {
            // A.4: once the tile-parts have started the only markers left at
            // this level are SOT and EOC. A main-header marker here is a file
            // whose structure this build has not understood, and carrying on
            // would mean applying it to some tiles and not others.
            return Err(Refusal::Structure("a main header marker after a tile-part"));
        }

        // Every remaining marker carries a length that includes itself.
        let len = c
            .u16()
            .ok_or(Refusal::Truncated("a marker segment length"))?;
        let body_len = usize::from(len).checked_sub(2).ok_or(Refusal::Structure(
            "a marker segment shorter than its length field",
        ))?;
        let body = c
            .take(body_len)
            .ok_or(Refusal::Truncated("a marker segment body"))?;

        match code {
            marker::SIZ => {
                if siz.is_some() {
                    return Err(Refusal::Structure("a second SIZ marker"));
                }
                let parsed = parse_siz(body)?;
                coc = vec![None; parsed.components.len()];
                qcc = vec![None; parsed.components.len()];
                rgn = vec![None; parsed.components.len()];
                siz = Some(parsed);
            }
            marker::COD => {
                let s = siz.as_ref().ok_or(Refusal::Structure("COD before SIZ"))?;
                if cod.is_some() {
                    // A.6.1 permits one COD in the main header. Two is a
                    // codestream saying two different things about how every
                    // component is coded, and last-one-wins is a guess that
                    // decodes to a picture.
                    return Err(Refusal::Structure("a second COD marker"));
                }
                cod = Some(parse_cod(body, s)?);
            }
            marker::COC => {
                let s = siz.as_ref().ok_or(Refusal::Structure("COC before SIZ"))?;
                let (index, style) = parse_coc(body, s, cod.as_ref())?;
                let slot = coc
                    .get_mut(usize::from(index))
                    .ok_or(Refusal::Structure("COC names a component SIZ did not"))?;
                if slot.is_some() {
                    return Err(Refusal::Structure("two COC markers for one component"));
                }
                *slot = Some(style);
            }
            marker::QCD => {
                if qcd.is_some() {
                    return Err(Refusal::Structure("a second QCD marker"));
                }
                qcd = Some(parse_quant(body, "QCD")?);
            }
            marker::QCC => {
                let s = siz.as_ref().ok_or(Refusal::Structure("QCC before SIZ"))?;
                let (index, q) = parse_qcc(body, s)?;
                let slot = qcc
                    .get_mut(usize::from(index))
                    .ok_or(Refusal::Structure("QCC names a component SIZ did not"))?;
                if slot.is_some() {
                    return Err(Refusal::Structure("two QCC markers for one component"));
                }
                *slot = Some(q);
            }
            // A.6.3: region of interest. Table A.4 makes it optional in the
            // main header with "no more than one RGN per component", and
            // A.6.3 repeats it — "There may be at most one RGN marker segment
            // for each component in either the main or tile-part headers".
            marker::RGN => {
                let s = siz.as_ref().ok_or(Refusal::Structure("RGN before SIZ"))?;
                let (index, roi) = parse_rgn(body, s)?;
                let slot = rgn
                    .get_mut(usize::from(index))
                    .ok_or(Refusal::Structure("RGN names a component SIZ did not"))?;
                if slot.is_some() {
                    return Err(Refusal::Structure("two RGN markers for one component"));
                }
                *slot = Some(roi);
            }
            // A.7.1 to A.7.3: pure indices into the codestream. A decoder
            // that ignores them decodes the same picture, which is exactly
            // what makes them safe to skip — and their lengths are still
            // checked above, so a lying one is a truncation rather than a
            // read past the end.
            marker::TLM | marker::PLM | marker::PLT => {}
            // A.9.1: component registration. Parsed so that a file carrying
            // it is not refused for saying something true about itself, and
            // then not applied, because the clause says it changes nothing.
            marker::CRG => {
                if registration.is_some() {
                    // A.9.1: "Only one CRG may be used in the main header".
                    return Err(Refusal::Structure("a second CRG marker"));
                }
                let Some(siz) = siz.as_ref() else {
                    // A.6.1's ordering puts SIZ first, and without it there is
                    // no component count to read this against.
                    return Err(Refusal::Structure("a CRG marker before SIZ"));
                };
                registration = Some(parse_crg(body, siz.components.len())?);
            }
            // A.9.2: a comment.
            marker::COM => {}
            _ => return Err(refuse_marker(code)),
        }
    }

    let Some(siz) = siz else {
        return Err(Refusal::Structure("a codestream with no SIZ marker"));
    };
    let Some(cod) = cod else {
        return Err(Refusal::Structure("a codestream with no COD marker"));
    };
    let Some(qcd) = qcd else {
        return Err(Refusal::Structure("a codestream with no QCD marker"));
    };
    let short_tiles = check_tile_parts(&siz, &tile_parts)?;

    Ok(Codestream {
        short_tiles,
        siz,
        cod,
        coc,
        qcd,
        qcc,
        rgn,
        tile_parts,
        registration,
    })
}

/// T.800 A.6.3 and Table A.24: the RGN marker segment.
///
/// Four fields after the length, in Figure A.12's order: `Crgn`, `Srgn`,
/// `SPrgn`. `Crgn` is one byte when `Csiz < 257` and two above, which is the
/// rule COC's `Ccoc` and QCC's `Cqcc` carry too, and Table A.24 spells it the
/// same way in both rows: "0 to 255; if Csiz < 257 / 0 to 16 383;
/// Csiz >= 257".
///
/// **The length is checked for equality rather than sufficiency.** Table A.24
/// gives `Lrgn` as "5 to 6" — two for itself, one or two for `Crgn`, one for
/// `Srgn`, one for `SPrgn` — so there is exactly one legal body length for a
/// given `Csiz` and a segment that disagrees is malformed. A decoder that
/// read the first fields out of a longer one would be inventing a tolerance
/// the table does not give, which is the mistake `parse_crg` records for CRG.
///
/// **A reserved `Srgn` is refused rather than stepped over**, and that is the
/// whole scope decision in one line. Table A.25 defines value 0, "Implicit
/// ROI (maximum shift)", and says "All other values reserved". A style this
/// build has never seen describes a realignment of the coefficients that
/// H.1's Maxshift arithmetic is not, so applying H.1 to it would put the
/// background at the wrong magnitude and draw a plausible picture — the exact
/// failure the JPEG decoder in this tree had when SOF3, SOF5, SOF6 and SOF7
/// were skipped rather than refused.
fn parse_rgn(body: &[u8], siz: &Siz) -> Result<(u16, Roi), Refusal> {
    let wide = siz.components.len() >= 257;
    let want = if wide { 4 } else { 3 };
    if body.len() != want {
        return Err(Refusal::Structure(
            "an RGN marker segment whose length is not Table A.24's",
        ));
    }
    let mut c = Cursor::new(body);
    let index = if wide { c.u16() } else { c.u8().map(u16::from) }
        .ok_or(Refusal::Truncated("an RGN component index"))?;
    let (Some(srgn), Some(sprgn)) = (c.u8(), c.u8()) else {
        return Err(Refusal::Truncated("an RGN marker segment"));
    };
    if srgn != 0 {
        return Err(Refusal::Feature("an Srgn ROI style Table A.25 reserves"));
    }
    Ok((index, Roi { shift: sprgn }))
}

/// T.800 A.9.1 and Table A.42: the CRG marker segment.
///
/// The segment is `Csiz` pairs of 16-bit values, **interleaved** as
/// `Xcrg_0, Ycrg_0, Xcrg_1, Ycrg_1, …` rather than all the horizontals
/// followed by all the verticals. Figure A.23 is what settles that: the prose
/// says "This value is repeated for every component" separately of `Xcrg_i`
/// and of `Ycrg_i`, which is ambiguous between the two orders on its own. SIZ
/// itself interleaves the same way.
///
/// **The loop is bounded by `Csiz`, never by `Lcrg`.** `parse_siz` has already
/// refused a component count above `MAX_JPX_COMPONENTS`, so counting pairs off
/// the component count inherits that bound, while counting them off the
/// segment length would take the count from the file. The length is then
/// checked for equality rather than sufficiency, because Table A.42 fixes
/// `Lcrg` at `2 + 4 × Csiz` — a segment that disagrees is malformed rather
/// than merely long, and a decoder that read the first `Csiz` pairs out of a
/// longer one would be inventing a tolerance the table does not give.
fn parse_crg(body: &[u8], components: usize) -> Result<Vec<Registration>, Refusal> {
    if body.len() != 4 * components {
        return Err(Refusal::Structure(
            "a CRG marker segment whose length is not 4 bytes per component",
        ));
    }
    let mut out = Vec::with_capacity(components);
    for pair in body.chunks_exact(4) {
        out.push(Registration {
            x: u16::from_be_bytes([pair[0], pair[1]]),
            y: u16::from_be_bytes([pair[2], pair[3]]),
        });
    }
    Ok(out)
}

/// The refusal a marker this build does not decode produces.
///
/// Split out so the test that walks Table A.2 can call it directly, and so
/// that "named" is a property of one function rather than of a match arm
/// that could quietly acquire a wildcard.
pub(crate) fn refuse_marker(code: u16) -> Refusal {
    match refused_by_design(code) {
        Some(name) => Refusal::Marker(name),
        // Defined, and reached somewhere A.4's ordering does not put it: a
        // SIZ inside a tile-part header, an SOC after the first two bytes.
        // Structural rather than unknown, because calling a marker the
        // standard defines "unknown" sends a reader looking for the wrong
        // thing.
        None if in_table_a2(code) => Refusal::Structure("a Table A.2 marker out of place"),
        None => Refusal::UnknownMarker(code),
    }
}

/// SIZ (A.5.1), including every one of its constraints.
fn parse_siz(body: &[u8]) -> Result<Siz, Refusal> {
    let mut c = Cursor::new(body);
    let (
        Some(rsiz),
        Some(xsiz),
        Some(ysiz),
        Some(xosiz),
        Some(yosiz),
        Some(xtsiz),
        Some(ytsiz),
        Some(xtosiz),
        Some(ytosiz),
        Some(csiz),
    ) = (
        c.u16(),
        c.u32(),
        c.u32(),
        c.u32(),
        c.u32(),
        c.u32(),
        c.u32(),
        c.u32(),
        c.u32(),
        c.u16(),
    )
    else {
        return Err(Refusal::Truncated("a SIZ marker segment"));
    };

    // A.5.1 Table A.10. 0 is Part 1 with no restriction; 1 and 2 are Profile
    // 0 and Profile 1, which restrict rather than extend and decode the same.
    // Everything else — every Part 2 capability, and every later amendment's
    // — is a codestream this build cannot claim to decode.
    if rsiz > 2 {
        return Err(Refusal::Feature("an Rsiz capability beyond Part 1"));
    }

    // Every one of these is A.5.1's own constraint, and every one of them is
    // arithmetic on an attacker-controlled 32-bit number rather than policy.
    if xtsiz == 0 || ytsiz == 0 {
        // A divisor.
        return Err(Refusal::Structure("a zero tile size"));
    }
    if xosiz >= xsiz || yosiz >= ysiz {
        // A subtrahend, and an empty image.
        return Err(Refusal::Structure("an image offset at or past its size"));
    }
    if xtosiz > xosiz || ytosiz > yosiz {
        return Err(Refusal::Structure("a tile offset past the image offset"));
    }
    // A.5.1: the first tile must intersect the image, or the tile grid does
    // not cover it at all.
    if u64::from(xtosiz) + u64::from(xtsiz) <= u64::from(xosiz)
        || u64::from(ytosiz) + u64::from(ytsiz) <= u64::from(yosiz)
    {
        return Err(Refusal::Structure("a tile grid that misses the image"));
    }
    if csiz == 0 {
        return Err(Refusal::Structure("a codestream with no components"));
    }
    if u32::from(csiz) > MAX_JPX_COMPONENTS {
        return Err(Refusal::Feature(
            "more components than the colour pipeline can interpret",
        ));
    }
    // A.5.1: Lsiz is 38 + 3 * Csiz, and the body is Lsiz - 2.
    if body.len() != 36 + 3 * usize::from(csiz) {
        return Err(Refusal::Structure("a SIZ length that does not match Csiz"));
    }

    let mut components = Vec::with_capacity(usize::from(csiz));
    for _ in 0..csiz {
        let (Some(ssiz), Some(dx), Some(dy)) = (c.u8(), c.u8(), c.u8()) else {
            return Err(Refusal::Truncated("a SIZ component"));
        };
        let precision = (ssiz & 0x7F) + 1;
        if precision > MAX_JPX_PRECISION {
            return Err(Refusal::Precision(precision));
        }
        if dx == 0 || dy == 0 {
            // Another divisor: B.2's tile-component bounds divide by these.
            return Err(Refusal::Structure("a zero component separation"));
        }
        components.push(Component {
            precision,
            signed: ssiz & 0x80 != 0,
            dx,
            dy,
        });
    }

    // A.5.1 equation A-4. `xtsiz` is non-zero and `xtosiz <= xosiz < xsiz`,
    // both checked above, so neither the subtraction nor the division can go
    // wrong here — which is the whole point of checking them first.
    let tiles_x = (xsiz - xtosiz).div_ceil(xtsiz);
    let tiles_y = (ysiz - ytosiz).div_ceil(ytsiz);
    if u64::from(tiles_x) * u64::from(tiles_y) > MAX_JPX_TILES {
        // A.5.1's own bound, not an invented one.
        return Err(Refusal::Budget("tiles"));
    }

    Ok(Siz {
        xsiz,
        ysiz,
        xosiz,
        yosiz,
        xtsiz,
        ytsiz,
        xtosiz,
        ytosiz,
        components,
        tiles_x,
        tiles_y,
    })
}

/// COD (A.6.1).
fn parse_cod(body: &[u8], siz: &Siz) -> Result<Cod, Refusal> {
    let mut c = Cursor::new(body);
    let (Some(scod), Some(order), Some(layers), Some(mct)) = (c.u8(), c.u8(), c.u16(), c.u8())
    else {
        return Err(Refusal::Truncated("a COD marker segment"));
    };
    if layers == 0 {
        return Err(Refusal::Structure("a COD declaring no quality layers"));
    }
    if mct > 1 {
        return Err(Refusal::Feature(
            "a multiple component transform beyond the RCT and ICT",
        ));
    }
    if scod & !0x07 != 0 {
        return Err(Refusal::Feature("a Scod bit Table A.13 does not define"));
    }
    let style = parse_style(&mut c, scod & 0x01 != 0, siz)?;
    Ok(Cod {
        progression: Progression::from_byte(order)?,
        layers,
        mct: mct == 1,
        sop: scod & 0x02 != 0,
        eph: scod & 0x04 != 0,
        style,
    })
}

/// COC (A.6.2). Returns the component it overrides.
fn parse_coc(body: &[u8], siz: &Siz, _cod: Option<&Cod>) -> Result<(u16, CodingStyle), Refusal> {
    let mut c = Cursor::new(body);
    // A.6.2: one byte when there are fewer than 257 components, two above.
    let index = if siz.components.len() < 257 {
        c.u8().map(u16::from)
    } else {
        c.u16()
    }
    .ok_or(Refusal::Truncated("a COC component index"))?;
    let scoc = c.u8().ok_or(Refusal::Truncated("a COC Scoc"))?;
    if scoc & !0x01 != 0 {
        return Err(Refusal::Feature("an Scoc bit Table A.23 does not define"));
    }
    let style = parse_style(&mut c, scoc & 0x01 != 0, siz)?;
    Ok((index, style))
}

/// SPcod / SPcoc (A.6.1 Table A.15), which are the same five fields plus the
/// optional precinct sizes.
fn parse_style(c: &mut Cursor<'_>, precincts: bool, siz: &Siz) -> Result<CodingStyle, Refusal> {
    let (Some(levels), Some(xcb), Some(ycb), Some(style), Some(transform)) =
        (c.u8(), c.u8(), c.u8(), c.u8(), c.u8())
    else {
        return Err(Refusal::Truncated("a coding style"));
    };
    if levels > MAX_JPX_LEVELS {
        return Err(Refusal::Structure(
            "more decomposition levels than A.6.1 allows",
        ));
    }
    // A.6.1: the stored value is the exponent minus 2, and the exponents are
    // 2..=10 with their sum at most 12 — a 4096-coefficient code-block. The
    // addition is in `u16` because `xcb` is one attacker-controlled byte and
    // `255 + 2` is not a code-block size, it is an overflow.
    let (cb_width, cb_height) = (u16::from(xcb) + 2, u16::from(ycb) + 2);
    if !(2..=10).contains(&cb_width) || !(2..=10).contains(&cb_height) || cb_width + cb_height > 12
    {
        return Err(Refusal::Structure(
            "a code-block size Table A.18 does not allow",
        ));
    }
    let (cb_width, cb_height) = (cb_width as u8, cb_height as u8);
    if style & cb_style::UNSUPPORTED != 0 {
        return Err(Refusal::Feature(
            "a Table A.19 code-block style this build does not implement",
        ));
    }
    if style & !cb_style::DEFINED != 0 {
        return Err(Refusal::Feature(
            "a code-block style bit Table A.19 does not define",
        ));
    }
    if transform > 1 {
        return Err(Refusal::Feature(
            "a wavelet transform Table A.20 does not define",
        ));
    }

    let count = usize::from(levels) + 1;
    let precincts = if precincts {
        let mut out = Vec::with_capacity(count);
        for r in 0..count {
            let b = c.u8().ok_or(Refusal::Truncated("a precinct size"))?;
            let (ppx, ppy) = (b & 0x0F, b >> 4);
            // A.6.1: PPx and PPy may be zero only for resolution 0, because
            // a precinct of one coefficient at any higher resolution would be
            // smaller than the 2x2 the partition maps onto it.
            if r > 0 && (ppx == 0 || ppy == 0) {
                return Err(Refusal::Structure(
                    "a zero precinct exponent above resolution 0",
                ));
            }
            out.push((ppx, ppy));
        }
        out
    } else {
        // A.6.1's default: 2^15, which is larger than any legal code-block
        // partition, so the whole subband is one precinct.
        vec![(15, 15); count]
    };
    let _ = siz;
    Ok(CodingStyle {
        levels,
        cb_width,
        cb_height,
        cb_style: style,
        reversible: transform == 1,
        precincts,
    })
}

/// QCD (A.6.4) and the tail of QCC (A.6.5).
fn parse_quant(body: &[u8], what: &'static str) -> Result<Quant, Refusal> {
    let mut c = Cursor::new(body);
    let sqcd = c.u8().ok_or(Refusal::Truncated(what))?;
    let guard_bits = sqcd >> 5;
    let style = match sqcd & 0x1F {
        0 => QuantStyle::None,
        1 => QuantStyle::Derived,
        2 => QuantStyle::Expounded,
        _ => {
            return Err(Refusal::Feature(
                "a quantisation style Table A.28 does not define",
            ))
        }
    };
    let mut steps = Vec::new();
    match style {
        QuantStyle::None => {
            while let Some(b) = c.u8() {
                steps.push((b >> 3, 0));
            }
        }
        QuantStyle::Derived | QuantStyle::Expounded => {
            while c.remaining() >= 2 {
                let v = c.u16().ok_or(Refusal::Truncated(what))?;
                steps.push(((v >> 11) as u8, v & 0x07FF));
            }
        }
    }
    if steps.is_empty() {
        return Err(Refusal::Structure(
            "a quantisation marker with no step sizes",
        ));
    }
    Ok(Quant {
        style,
        guard_bits,
        steps,
    })
}

/// QCC (A.6.5).
fn parse_qcc(body: &[u8], siz: &Siz) -> Result<(u16, Quant), Refusal> {
    let width = usize::from(u8::from(siz.components.len() >= 257)) + 1;
    let (index, tail) = if width == 1 {
        let Some((&b, tail)) = body.split_first() else {
            return Err(Refusal::Truncated("a QCC component index"));
        };
        (u16::from(b), tail)
    } else {
        let Some((head, tail)) = body.split_at_checked(2) else {
            return Err(Refusal::Truncated("a QCC component index"));
        };
        (u16::from(head[0]) << 8 | u16::from(head[1]), tail)
    };
    Ok((index, parse_quant(tail, "QCC")?))
}

/// SOT (A.4.2) and the tile-part header that follows it, up to SOD.
fn tile_part<'a>(
    c: &mut Cursor<'a>,
    siz: Option<&Siz>,
    cod: Option<&Cod>,
) -> Result<TilePart<'a>, Refusal> {
    let start = c.position() - 2;
    let siz = siz.ok_or(Refusal::Structure("SOT before SIZ"))?;
    let (Some(lsot), Some(isot), Some(psot), Some(tpsot), Some(tnsot)) =
        (c.u16(), c.u16(), c.u32(), c.u8(), c.u8())
    else {
        return Err(Refusal::Truncated("an SOT marker segment"));
    };
    if lsot != 10 {
        return Err(Refusal::Structure("an SOT length that is not 10"));
    }
    let tiles = u64::from(siz.tiles_x) * u64::from(siz.tiles_y);
    if u64::from(isot) >= tiles {
        return Err(Refusal::Structure("an SOT naming a tile outside the grid"));
    }

    let mut part = TilePart {
        tile: isot,
        index: tpsot,
        parts: tnsot,
        cod: None,
        coc: Vec::new(),
        qcd: None,
        qcc: Vec::new(),
        rgn: Vec::new(),
        data: &[],
    };

    loop {
        let Some(code) = c.u16() else {
            return Err(Refusal::Truncated("a tile-part header marker"));
        };
        if code == marker::SOD {
            break;
        }
        let len = c
            .u16()
            .ok_or(Refusal::Truncated("a tile-part marker segment length"))?;
        let body_len = usize::from(len).checked_sub(2).ok_or(Refusal::Structure(
            "a marker segment shorter than its length field",
        ))?;
        let body = c
            .take(body_len)
            .ok_or(Refusal::Truncated("a tile-part marker segment body"))?;
        match code {
            // A.6.1 and A.6.4: a COD or QCD in a tile-part header applies to
            // the whole tile, and only the first tile-part may carry one.
            marker::COD if tpsot == 0 => part.cod = Some(parse_cod(body, siz)?),
            marker::QCD if tpsot == 0 => part.qcd = Some(parse_quant(body, "QCD")?),
            marker::COC if tpsot == 0 => part.coc.push(parse_coc(body, siz, cod)?),
            marker::QCC if tpsot == 0 => part.qcc.push(parse_qcc(body, siz)?),
            // A.6.3: "If there are multiple tile-parts in a tile, then this
            // marker segment shall be found only in the first tile-part
            // header", and at most one per component in that header.
            marker::RGN if tpsot == 0 => {
                let (index, roi) = parse_rgn(body, siz)?;
                if usize::from(index) >= siz.components.len() {
                    return Err(Refusal::Structure("RGN names a component SIZ did not"));
                }
                if part.rgn.iter().any(|(i, _)| *i == index) {
                    return Err(Refusal::Structure("two RGN markers for one component"));
                }
                part.rgn.push((index, roi));
            }
            marker::COD | marker::QCD | marker::COC | marker::QCC | marker::RGN => {
                return Err(Refusal::Structure(
                    "a coding style marker in a tile-part after the first",
                ))
            }
            marker::PLT | marker::COM => {}
            _ => return Err(refuse_marker(code)),
        }
    }

    // A.4.2: Psot is the whole tile-part from the first byte of SOT to the
    // last byte of its data, and 0 means "to the end of the codestream" —
    // which A.4.2 permits only in the last tile-part.
    let header_len = c.position() - start;
    let body_len = if psot == 0 {
        c.remaining()
    } else {
        usize::try_from(psot)
            .ok()
            .and_then(|n| n.checked_sub(header_len))
            .ok_or(Refusal::Structure("a Psot shorter than its own header"))?
    };
    part.data = c
        .take(body_len)
        .ok_or(Refusal::Truncated("a tile-part's data"))?;
    Ok(part)
}

/// A.4.2's rules about how tile-parts fit together, and which tiles arrived
/// whole.
///
/// **Two different failures live here and they were treated as one.**
///
/// Parts *out of order*, or two of them disagreeing about `TNsot`, or one
/// naming a tile outside the grid, are all a codestream contradicting itself.
/// A decoder that reassembles them in stream order regardless produces a
/// picture, and the picture is wrong in a way that looks like compression —
/// which is what the whole refusal list exists to prevent.
///
/// A tile whose declared parts did not all *arrive* is a different thing: it
/// is a file that stops early. That costs pixels rather than meaning, and this
/// crate already draws that line — `JxrWarning::TileDroppedAsZero` is the same
/// bargain in JPEG XR, and a fax row that will not decode is replicated rather
/// than refused. Two documents off the open web are the reason it is drawn
/// here too: both carry tiles that decode and one that stops, and refusing the
/// image threw away the tiles that were whole.
///
/// Returns the tiles that are short, for the caller to leave blank.
fn check_tile_parts(siz: &Siz, parts: &[TilePart<'_>]) -> Result<Vec<bool>, Refusal> {
    let tiles = usize::try_from(u64::from(siz.tiles_x) * u64::from(siz.tiles_y))
        .map_err(|_| Refusal::Budget("tiles"))?;
    let mut next = vec![0u32; tiles];
    let mut declared = vec![None::<u8>; tiles];
    for part in parts {
        let t = usize::from(part.tile);
        let (Some(seen), Some(total)) = (next.get_mut(t), declared.get_mut(t)) else {
            return Err(Refusal::Structure(
                "a tile-part naming a tile outside the grid",
            ));
        };
        if u32::from(part.index) != *seen {
            return Err(Refusal::Structure("tile-parts out of order"));
        }
        *seen += 1;
        if part.parts != 0 {
            if total.is_some_and(|n| n != part.parts) {
                return Err(Refusal::Structure(
                    "two tile-parts declaring different TNsot",
                ));
            }
            *total = Some(part.parts);
        }
    }
    let mut short = vec![false; tiles];
    let mut whole = 0usize;
    for (t, total) in declared.iter().enumerate() {
        // A.4.2: every tile in the grid is coded, and a tile that declared `n`
        // parts and got fewer is a file that stopped. Either way the tile has
        // no complete set of coefficients, so it is marked rather than decoded
        // — a decoder that read what arrived would draw a partial tile and
        // call the decode a success.
        let missing = next[t] == 0 || total.is_some_and(|n| next[t] != u32::from(n));
        short[t] = missing;
        if !missing {
            whole += 1;
        }
    }
    if whole == 0 {
        // Nothing arrived whole, so there is no picture to degrade *to*. This
        // is the refusal the note above is about: a page of grey rectangles
        // reported as a successful decode is worse than the placeholder.
        return Err(Refusal::Structure("a codestream with no complete tile"));
    }
    Ok(short)
}
