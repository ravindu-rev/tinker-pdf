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
//! twenty: fourteen are parsed, five are refused **by name** (RGN, POC, PPM,
//! PPT, CRG) and one — EPH — belongs to tier-2 and is refused here only when
//! it appears in a header. Anything else is refused as an unknown marker,
//! which is where every ISO/IEC 15444-2 marker lands, Part 2 being a
//! non-goal.
//!
//! Skipping a marker whose length happens to be readable is the failure this
//! guards against, and it is not hypothetical: RGN scales a region's
//! coefficients, and a decoder that skips it produces a picture with a bright
//! rectangle in it rather than an error. POC changes the progression order
//! *mid-stream*, so a decoder that skips it reads every packet after it in
//! the wrong order and produces a soft, plausible image.
//!
//! # Where the arithmetic on attacker numbers is
//!
//! A.5.1's tile grid. `XTsiz` is a divisor and `XOsiz` is a subtrahend, so a
//! zero `XTsiz` is a division by zero and `XOsiz > Xsiz` is an underflow.
//! Every one of A.5.1's constraints is checked here rather than assumed, and
//! each violation refuses the file.

use super::{Cursor, Refusal, MAX_JPX_COMPONENTS, MAX_JPX_LEVELS, MAX_JPX_SAMPLES, MAX_JPX_TILES};
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
    /// Region of interest (A.6.3). **Refused**: it scales a rectangle's
    /// coefficients, and ignoring it draws that rectangle too bright.
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
    /// Component registration (A.9.1). **Refused**: it is a sub-pixel
    /// registration offset between components, and this build has nowhere to
    /// put one.
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
        marker::RGN => "RGN, region of interest",
        marker::POC => "POC, progression order change",
        marker::PPM => "PPM, packed packet headers in the main header",
        marker::PPT => "PPT, packed packet headers in a tile-part header",
        marker::CRG => "CRG, component registration",
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
    /// The five this build refuses.
    pub const UNSUPPORTED: u8 = BYPASS | RESET | TERMALL | VERTICALLY_CAUSAL | PREDICTABLE;
    /// Every bit Table A.19 defines. A `Scod` byte with anything outside
    /// this is a codestream using a table this build has not read.
    pub const DEFINED: u8 = UNSUPPORTED | SEGMENTATION_SYMBOLS;
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
    /// Everything between SOD and the end of the tile-part: the packets.
    pub(crate) data: &'a [u8],
}

/// A parsed codestream: the main header and every tile-part in stream order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Codestream<'a> {
    pub(crate) siz: Siz,
    pub(crate) cod: Cod,
    /// Per-component COC overrides from the main header.
    pub(crate) coc: Vec<Option<CodingStyle>>,
    pub(crate) qcd: Quant,
    /// Per-component QCC overrides from the main header.
    pub(crate) qcc: Vec<Option<Quant>>,
    pub(crate) tile_parts: Vec<TilePart<'a>>,
}

impl Codestream<'_> {
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
            // A.7.1 to A.7.3: pure indices into the codestream. A decoder
            // that ignores them decodes the same picture, which is exactly
            // what makes them safe to skip — and their lengths are still
            // checked above, so a lying one is a truncation rather than a
            // read past the end.
            marker::TLM | marker::PLM | marker::PLT => {}
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
    check_tile_parts(&siz, &tile_parts)?;

    Ok(Codestream {
        siz,
        cod,
        coc,
        qcd,
        qcc,
        tile_parts,
    })
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
        if precision > 16 {
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
            marker::COD | marker::QCD | marker::COC | marker::QCC => {
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

/// A.4.2's rules about how tile-parts fit together.
///
/// Out-of-order parts and a tile whose parts do not cover it are both on the
/// refusal list, and both for the reason the whole list exists: a decoder
/// that reassembles them in stream order regardless produces a picture, and
/// the picture is wrong in a way that looks like compression.
fn check_tile_parts(siz: &Siz, parts: &[TilePart<'_>]) -> Result<(), Refusal> {
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
    for (t, total) in declared.iter().enumerate() {
        if let Some(total) = total {
            if next[t] != u32::from(*total) {
                return Err(Refusal::Structure("a tile whose parts do not cover it"));
            }
        }
        // A.4.2: every tile in the grid is coded. A tile no part covers has
        // no coefficients at all, and a decoder that leaves its samples at
        // whatever the plane was initialised to draws a grey rectangle inside
        // the picture and calls the decode a success. It is also what an
        // early EOC produces, which is how a truncated file would otherwise
        // arrive here looking complete.
        if next[t] == 0 {
            return Err(Refusal::Structure("a tile with no tile-parts"));
        }
    }
    Ok(())
}
