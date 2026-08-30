//! ITU-T T.832 clause 8.2 to 8.6: the codestream header layers.
//!
//! `CODED_IMAGE( )` (8.2.1) is an image header, one or two image plane
//! headers, an optional index table, an optional profile/level block, and
//! then the tiles. This module reads everything above the tiles and hands the
//! rest a fully derived geometry — macroblock counts, tile boundaries, band
//! count, quantization parameter sets — so that nothing below it has to reach
//! back into a header field.
//!
//! **Two facts about JPEG XR's geometry drive the whole decoder and are worth
//! stating here.** First, the coded area is the *extended* image: 6.2 makes
//! `ExtendedWidth[0]` equal to the output width plus the left and right
//! margins, and 8.3.30 requires the total to be a multiple of 16. So the
//! decoder always works in whole macroblocks and the margins are cropped at
//! the very end, in `colour.rs`. Second, tile boundaries are stated in
//! macroblocks and the *last* tile's size is inferred by subtraction (8.3.25)
//! — which means a file can declare tile widths that overrun the image, and
//! the subtraction underflows. That is checked here, not later.

#![deny(clippy::float_arithmetic)]

use super::bitstream::BitReader;
use super::{JxrError, JxrWarning, MAX_JXR_COMPONENTS, MAX_JXR_TILES};

/// Table 22's `OUTPUT_CLR_FMT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputClrFmt {
    YOnly,
    Yuv420,
    Yuv422,
    Yuv444,
    Cmyk,
    CmykDirect,
    NComponent,
    Rgb,
    Rgbe,
}

impl OutputClrFmt {
    fn from_bits(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::YOnly,
            1 => Self::Yuv420,
            2 => Self::Yuv422,
            3 => Self::Yuv444,
            4 => Self::Cmyk,
            5 => Self::CmykDirect,
            6 => Self::NComponent,
            7 => Self::Rgb,
            8 => Self::Rgbe,
            // Table 22 rows 9-15 are RESERVED.
            _ => return None,
        })
    }
}

/// Table 28's `INTERNAL_CLR_FMT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InternalClrFmt {
    YOnly,
    Yuv420,
    Yuv422,
    Yuv444,
    Yuvk,
    NComponent,
}

impl InternalClrFmt {
    fn from_bits(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::YOnly,
            1 => Self::Yuv420,
            2 => Self::Yuv422,
            3 => Self::Yuv444,
            4 => Self::Yuvk,
            6 => Self::NComponent,
            // Table 28 rows 5 and 7 are RESERVED.
            _ => return None,
        })
    }

    /// Table 31's `DetermineNumComponents( )` for the non-NCOMPONENT rows.
    fn fixed_component_count(self) -> Option<u32> {
        Some(match self {
            Self::YOnly => 1,
            Self::Yuv420 | Self::Yuv422 | Self::Yuv444 => 3,
            Self::Yuvk => 4,
            Self::NComponent => return None,
        })
    }

    /// Whether the chroma planes are subsampled, and by how much. Table 17.
    ///
    /// Every subsampled format is refused by name today, so nothing calls
    /// this until the milestone that stops refusing them.
    #[allow(dead_code)]
    pub(crate) fn chroma_shift(self) -> (u32, u32) {
        match self {
            // Table 17: YUV420 halves both axes, YUV422 halves the width
            // only, and everything else leaves the chroma arrays full size.
            Self::Yuv420 => (1, 1),
            Self::Yuv422 => (1, 0),
            _ => (0, 0),
        }
    }
}

/// Table 23's `OUTPUT_BITDEPTH`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OutputBitdepth {
    Bd1White1,
    Bd8,
    Bd16,
    Bd16S,
    Bd16F,
    Bd32S,
    Bd32F,
    Bd5,
    Bd10,
    Bd565,
    Bd1Black1,
}

impl OutputBitdepth {
    fn from_bits(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::Bd1White1,
            1 => Self::Bd8,
            2 => Self::Bd16,
            3 => Self::Bd16S,
            4 => Self::Bd16F,
            6 => Self::Bd32S,
            7 => Self::Bd32F,
            8 => Self::Bd5,
            9 => Self::Bd10,
            10 => Self::Bd565,
            15 => Self::Bd1Black1,
            // Table 23 rows 5 and 11-14 are RESERVED.
            _ => return None,
        })
    }

    /// 8.4.13: `SHIFT_BITS` is present for exactly these three.
    fn has_shift_bits(self) -> bool {
        matches!(self, Self::Bd16 | Self::Bd16S | Self::Bd32S)
    }
}

/// Table 29's `BANDS_PRESENT`, as the band count Table 30 derives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum BandsPresent {
    /// Table 29 row 3: only DC.
    DcOnly,
    /// Table 29 row 2: DC and LP.
    NoHighpass,
    /// Table 29 row 1: DC, LP and HP, but no FLEXBITS.
    NoFlexbits,
    /// Table 29 row 0.
    All,
}

impl BandsPresent {
    fn from_bits(v: u32) -> Option<Self> {
        Some(match v {
            0 => Self::All,
            1 => Self::NoFlexbits,
            2 => Self::NoHighpass,
            3 => Self::DcOnly,
            // Table 29 rows 4-15 are RESERVED.
            _ => return None,
        })
    }

    /// Table 30's `NumBands`.
    pub(crate) fn num_bands(self) -> u32 {
        match self {
            Self::All => 4,
            Self::NoFlexbits => 3,
            Self::NoHighpass => 2,
            Self::DcOnly => 1,
        }
    }
}

/// Table 33's `COMPONENT_MODE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComponentMode {
    Uniform,
    Separate,
    Independent,
}

/// One quantization parameter set: a QP per component (9.7).
///
/// The three `COMPONENT_MODE` rows all resolve to the same thing — a value
/// per component — so they are flattened here rather than carried as a mode
/// plus two or three numbers. 9.7's derivation reads only the per-component
/// value, so nothing downstream needs to know which spelling the file used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QpSet {
    pub(crate) per_component: Vec<u8>,
}

impl QpSet {
    /// 8.4.22, 8.4.23 and 8.4.24 are the same structure over a different
    /// element name, so they are one function.
    fn read(r: &mut BitReader<'_>, num_components: u32) -> Result<Self, JxrError> {
        let mode = if num_components == 1 {
            // 8.4.22.2: inferred to be UNIFORM when there is one component.
            ComponentMode::Uniform
        } else {
            match r.read_u32(2)? {
                0 => ComponentMode::Uniform,
                1 => ComponentMode::Separate,
                2 => ComponentMode::Independent,
                // Table 33 row 3 is RESERVED. A file using it has not said
                // what its quantizers are, and guessing would decode a
                // plausible-looking wrong picture.
                _ => return Err(JxrError::ReservedValue("COMPONENT_MODE")),
            }
        };
        let n = usize::try_from(num_components).map_err(|_| JxrError::Truncated)?;
        let per_component = match mode {
            ComponentMode::Uniform => {
                // The cast is exact: read_u32(8) returns 0..=255.
                let q = r.read_u32(8)? as u8;
                vec![q; n]
            }
            ComponentMode::Separate => {
                let luma = r.read_u32(8)? as u8;
                let chroma = r.read_u32(8)? as u8;
                let mut v = vec![chroma; n];
                if let Some(first) = v.first_mut() {
                    *first = luma;
                }
                v
            }
            ComponentMode::Independent => {
                let mut v = Vec::with_capacity(n);
                for _ in 0..n {
                    v.push(r.read_u32(8)? as u8);
                }
                v
            }
        };
        Ok(Self { per_component })
    }

    /// The QP for one component, saturating at the last entry. The vector is
    /// always `num_components` long by construction, so the fallback is a
    /// defence against a future caller rather than a reachable path.
    #[allow(dead_code)] // Milestone 2: 9.7 reads it per component.
    pub(crate) fn get(&self, component: usize) -> u8 {
        self.per_component
            .get(component)
            .copied()
            .or_else(|| self.per_component.last().copied())
            .unwrap_or(0)
    }
}

/// A list of QP sets, which is what `LP_QP( )` and `HP_QP( )` produce when a
/// tile declares more than one (8.7.10.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QpSets {
    pub(crate) sets: Vec<QpSet>,
}

impl QpSets {
    fn read(r: &mut BitReader<'_>, count: u32, num_components: u32) -> Result<Self, JxrError> {
        let n = usize::try_from(count).map_err(|_| JxrError::Truncated)?;
        let mut sets = Vec::with_capacity(n);
        for _ in 0..count {
            sets.push(QpSet::read(r, num_components)?);
        }
        Ok(Self { sets })
    }

    #[allow(dead_code)] // Milestone 2: the tile headers build these.
    pub(crate) fn one(set: QpSet) -> Self {
        Self { sets: vec![set] }
    }

    #[allow(dead_code)] // Milestone 2: 8.7.10.8's LP_QP_INDEX selects one.
    pub(crate) fn select(&self, index: usize) -> Option<&QpSet> {
        self.sets.get(index).or_else(|| self.sets.first())
    }
}

/// `IMAGE_HEADER( )`, 8.3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageHeader {
    pub(crate) hard_tiling: bool,
    pub(crate) tiling: bool,
    pub(crate) frequency_mode: bool,
    pub(crate) spatial_xfrm_subordinate: u8,
    pub(crate) index_table_present: bool,
    /// 8.3.10: 0 none, 1 second level only, 2 both levels. 3 is reserved.
    pub(crate) overlap_mode: u8,
    pub(crate) short_header: bool,
    pub(crate) long_word: bool,
    pub(crate) windowing: bool,
    pub(crate) trim_flexbits: bool,
    pub(crate) red_blue_not_swapped: bool,
    pub(crate) premultiplied_alpha: bool,
    pub(crate) alpha_image_plane: bool,
    pub(crate) output_clr_fmt: OutputClrFmt,
    pub(crate) output_bitdepth: OutputBitdepth,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) num_ver_tiles: u32,
    pub(crate) num_hor_tiles: u32,
    /// 8.3.25's `LeftMBIndexOfTile[ ]`, already closed with `MBWidth`, so it
    /// has `num_ver_tiles + 1` entries.
    pub(crate) left_mb_of_tile: Vec<u32>,
    /// 8.3.26's `TopMBIndexOfTile[ ]`, closed with `MBHeight`.
    pub(crate) top_mb_of_tile: Vec<u32>,
    pub(crate) top_margin: u32,
    pub(crate) left_margin: u32,
    pub(crate) bottom_margin: u32,
    pub(crate) right_margin: u32,
    /// 6.2's `ExtendedWidth[0]` and `ExtendedHeight[0]`.
    pub(crate) extended_width: u32,
    pub(crate) extended_height: u32,
    /// 6.4's `MBWidth` and `MBHeight`.
    pub(crate) mb_width: u32,
    pub(crate) mb_height: u32,
}

impl ImageHeader {
    pub(crate) fn read(
        r: &mut BitReader<'_>,
        warnings: &mut Vec<JxrWarning>,
    ) -> Result<Self, JxrError> {
        let signature = r.read(64)?;
        if signature != u64::from_be_bytes(super::container::GDI_SIGNATURE) {
            return Err(JxrError::NotJxr);
        }
        // 8.3.3: RESERVED_B shall be 1. 8.3.5 says decoders shall *ignore*
        // RESERVED_C, and the asymmetry is the clause's own: RESERVED_B is
        // the version escape, so a value this build does not know means the
        // rest of the header may not be laid out as read.
        let reserved_b = r.read_u32(4)?;
        if reserved_b != 1 {
            return Err(JxrError::UnsupportedCodestreamVersion(
                // The cast is exact: a four-bit read is 0..=15.
                reserved_b as u8,
            ));
        }
        let hard_tiling = r.flag()?;
        let _reserved_c = r.read_u32(3)?;
        let tiling = r.flag()?;
        let frequency_mode = r.flag()?;
        let spatial_xfrm_subordinate = r.read_u32(3)? as u8;
        let index_table_present = r.flag()?;
        let overlap_mode = r.read_u32(2)? as u8;
        if overlap_mode == 3 {
            // 8.3.10: "The value 3 is reserved." Overlap filtering changes
            // every sample near a block edge, so a decoder that guessed at
            // an unknown mode would return an image that is wrong exactly
            // where a reader would not look.
            return Err(JxrError::ReservedValue("OVERLAP_MODE"));
        }
        let short_header = r.flag()?;
        let long_word = r.flag()?;
        let windowing = r.flag()?;
        let trim_flexbits = r.flag()?;
        let _reserved_d = r.flag()?;
        let red_blue_not_swapped = r.flag()?;
        let premultiplied_alpha = r.flag()?;
        let alpha_image_plane = r.flag()?;
        let output_clr_fmt = OutputClrFmt::from_bits(r.read_u32(4)?)
            .ok_or(JxrError::ReservedValue("OUTPUT_CLR_FMT"))?;
        let output_bitdepth = OutputBitdepth::from_bits(r.read_u32(4)?)
            .ok_or(JxrError::ReservedValue("OUTPUT_BITDEPTH"))?;

        let dim_bits = if short_header { 16 } else { 32 };
        let width = r
            .read_u32(dim_bits)?
            .checked_add(1)
            .ok_or(JxrError::BadDimensions)?;
        let height = r
            .read_u32(dim_bits)?
            .checked_add(1)
            .ok_or(JxrError::BadDimensions)?;

        // 8.3.23 and 8.3.24: absent means one tile.
        let (num_ver_tiles_minus1, num_hor_tiles_minus1) = if tiling {
            (r.read_u32(12)?, r.read_u32(12)?)
        } else {
            (0, 0)
        };
        let num_ver_tiles = num_ver_tiles_minus1 + 1;
        let num_hor_tiles = num_hor_tiles_minus1 + 1;
        // The 12-bit fields cap each axis at 4096, so the product cannot
        // overflow a u64; it is still checked against the budget before any
        // per-tile allocation exists.
        let total_tiles = u64::from(num_ver_tiles) * u64::from(num_hor_tiles);
        if total_tiles > MAX_JXR_TILES {
            return Err(JxrError::TooManyTiles {
                tiles: total_tiles,
                max: MAX_JXR_TILES,
            });
        }

        let tile_dim_bits = if short_header { 8 } else { 16 };
        let mut tile_widths = Vec::new();
        for _ in 0..num_ver_tiles_minus1 {
            tile_widths.push(r.read_u32(tile_dim_bits)?);
        }
        let mut tile_heights = Vec::new();
        for _ in 0..num_hor_tiles_minus1 {
            tile_heights.push(r.read_u32(tile_dim_bits)?);
        }

        let (top_margin, left_margin, bottom_margin, right_margin) = if windowing {
            (
                r.read_u32(6)?,
                r.read_u32(6)?,
                r.read_u32(6)?,
                r.read_u32(6)?,
            )
        } else {
            // 8.3.29 and 8.3.30: when absent, the bottom and right margins
            // are inferred so that the extended dimensions are multiples of
            // 16. Top and left are inferred as 0.
            let bottom = if height % 16 == 0 {
                0
            } else {
                16 - (height % 16)
            };
            let right = if width % 16 == 0 {
                0
            } else {
                16 - (width % 16)
            };
            (0, 0, bottom, right)
        };

        // 6.2: ExtendedWidth[0] = WIDTH_MINUS1 + 1 + LEFT + RIGHT.
        let extended_width = width
            .checked_add(left_margin)
            .and_then(|n| n.checked_add(right_margin))
            .ok_or(JxrError::BadDimensions)?;
        let extended_height = height
            .checked_add(top_margin)
            .and_then(|n| n.checked_add(bottom_margin))
            .ok_or(JxrError::BadDimensions)?;
        // 8.3.21 and 8.3.22 make both a multiple of 16. A file that breaks
        // this has not described a macroblock grid, and 6.4's MBWidth is a
        // truncating division that would silently drop the remainder.
        if extended_width % 16 != 0
            || extended_height % 16 != 0
            || extended_width == 0
            || extended_height == 0
        {
            return Err(JxrError::BadDimensions);
        }
        let mb_width = extended_width / 16;
        let mb_height = extended_height / 16;

        // 8.3.25 Table 24, and its overrun check. The clause derives the last
        // tile's width by subtraction, so a file whose declared widths sum
        // past MBWidth makes that subtraction underflow — refused by name
        // rather than wrapped.
        let left_mb_of_tile = boundaries(&tile_widths, mb_width)?;
        let top_mb_of_tile = boundaries(&tile_heights, mb_height)?;

        // 8.3.16: outside the three bit depths the flag applies to, its value
        // is reserved and decoders ignore it. Recorded when it is set where
        // it has no meaning, because that is a file saying something it
        // cannot mean.
        let flag_applies = matches!(output_clr_fmt, OutputClrFmt::Rgb)
            && matches!(
                output_bitdepth,
                OutputBitdepth::Bd5 | OutputBitdepth::Bd565 | OutputBitdepth::Bd10
            );
        if red_blue_not_swapped && !flag_applies {
            warnings.push(JxrWarning::ReservedFlagSet("RED_BLUE_NOT_SWAPPED_FLAG"));
        }

        Ok(Self {
            hard_tiling,
            tiling,
            frequency_mode,
            spatial_xfrm_subordinate,
            index_table_present,
            overlap_mode,
            short_header,
            long_word,
            windowing,
            trim_flexbits,
            red_blue_not_swapped,
            premultiplied_alpha,
            alpha_image_plane,
            output_clr_fmt,
            output_bitdepth,
            width,
            height,
            num_ver_tiles,
            num_hor_tiles,
            left_mb_of_tile,
            top_mb_of_tile,
            top_margin,
            left_margin,
            bottom_margin,
            right_margin,
            extended_width,
            extended_height,
            mb_width,
            mb_height,
        })
    }

    /// Table 26's `DetermineNumMBInTile( )`, flattened to the raster order
    /// 8.7.1 iterates in.
    pub(crate) fn tile_mb_counts(&self) -> Vec<u64> {
        let mut out = Vec::new();
        for row in 0..self.num_hor_tiles as usize {
            for col in 0..self.num_ver_tiles as usize {
                let w = match (
                    self.left_mb_of_tile.get(col + 1),
                    self.left_mb_of_tile.get(col),
                ) {
                    (Some(a), Some(b)) => u64::from(a.saturating_sub(*b)),
                    _ => 0,
                };
                let h = match (
                    self.top_mb_of_tile.get(row + 1),
                    self.top_mb_of_tile.get(row),
                ) {
                    (Some(a), Some(b)) => u64::from(a.saturating_sub(*b)),
                    _ => 0,
                };
                out.push(w * h);
            }
        }
        out
    }
}

/// 8.3.25 Table 24 / 8.3.26 Table 25: prefix sums closed with the total, with
/// the overrun the clause's subtraction would otherwise hide made explicit.
fn boundaries(sizes: &[u32], total: u32) -> Result<Vec<u32>, JxrError> {
    let mut out = Vec::with_capacity(sizes.len() + 1);
    out.push(0u32);
    let mut acc: u32 = 0;
    for &size in sizes {
        if size == 0 {
            // A zero-width tile column is not a partition of anything, and
            // Table 24's sequence is required to be strictly increasing.
            return Err(JxrError::BadTiling);
        }
        acc = acc.checked_add(size).ok_or(JxrError::BadTiling)?;
        if acc >= total {
            // The last tile is what remains, so the declared ones must leave
            // at least one macroblock over.
            return Err(JxrError::BadTiling);
        }
        out.push(acc);
    }
    out.push(total);
    Ok(out)
}

/// `IMAGE_PLANE_HEADER( )`, 8.4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaneHeader {
    pub(crate) internal_clr_fmt: InternalClrFmt,
    pub(crate) scaled: bool,
    pub(crate) bands_present: BandsPresent,
    pub(crate) chroma_centering_x: u32,
    pub(crate) chroma_centering_y: u32,
    pub(crate) num_components: u32,
    pub(crate) shift_bits: u32,
    pub(crate) len_mantissa: u32,
    pub(crate) exp_bias: i64,
    pub(crate) dc_uniform: bool,
    pub(crate) lp_uniform: bool,
    pub(crate) hp_uniform: bool,
    /// Present exactly when the matching `_UNIFORM_FLAG` is set; otherwise
    /// the tile headers carry them (8.4.16, 8.4.18, 8.4.20).
    pub(crate) dc_qp: Option<QpSet>,
    pub(crate) lp_qp: Option<QpSets>,
    pub(crate) hp_qp: Option<QpSets>,
}

impl PlaneHeader {
    pub(crate) fn read(
        r: &mut BitReader<'_>,
        image: &ImageHeader,
        is_alpha: bool,
        warnings: &mut Vec<JxrWarning>,
    ) -> Result<Self, JxrError> {
        let internal_clr_fmt = InternalClrFmt::from_bits(r.read_u32(3)?)
            .ok_or(JxrError::ReservedValue("INTERNAL_CLR_FMT"))?;
        if is_alpha && internal_clr_fmt != InternalClrFmt::YOnly {
            // 8.4.2: when IsCurrPlaneAlphaFlag is TRUE the value shall be 0.
            return Err(JxrError::BadAlphaPlane);
        }
        let scaled = r.flag()?;
        let bands_present = BandsPresent::from_bits(r.read_u32(4)?)
            .ok_or(JxrError::ReservedValue("BANDS_PRESENT"))?;

        let mut chroma_centering_x = 0;
        let mut chroma_centering_y = 0;
        let mut num_components = 0;
        match internal_clr_fmt {
            InternalClrFmt::Yuv444 | InternalClrFmt::Yuv420 | InternalClrFmt::Yuv422 => {
                if matches!(
                    internal_clr_fmt,
                    InternalClrFmt::Yuv420 | InternalClrFmt::Yuv422
                ) {
                    let _reserved_e = r.flag()?;
                    chroma_centering_x = r.read_u32(3)?;
                } else {
                    let _reserved_f = r.read_u32(4)?;
                }
                if internal_clr_fmt == InternalClrFmt::Yuv420 {
                    let _reserved_g = r.flag()?;
                    chroma_centering_y = r.read_u32(3)?;
                } else {
                    let _reserved_h = r.read_u32(4)?;
                }
            }
            InternalClrFmt::NComponent => {
                let minus1 = r.read_u32(4)?;
                if minus1 == 0xF {
                    num_components = r.read_u32(12)? + 16;
                } else {
                    num_components = minus1 + 1;
                    let _reserved_h = r.read_u32(4)?;
                }
            }
            InternalClrFmt::YOnly | InternalClrFmt::Yuvk => {}
        }
        if let Some(fixed) = internal_clr_fmt.fixed_component_count() {
            num_components = fixed;
        }
        if num_components == 0 || num_components > MAX_JXR_COMPONENTS {
            return Err(JxrError::TooManyComponents {
                components: num_components,
                max: MAX_JXR_COMPONENTS,
            });
        }

        let shift_bits = if image.output_bitdepth.has_shift_bits() {
            r.read_u32(8)?
        } else {
            0
        };
        let (len_mantissa, exp_bias) = if image.output_bitdepth == OutputBitdepth::Bd32F {
            (r.read_u32(8)?, r.read_signed(8)?)
        } else {
            (0, 0)
        };

        let dc_uniform = r.flag()?;
        let dc_qp = if dc_uniform {
            Some(QpSet::read(r, num_components)?)
        } else {
            None
        };

        let mut lp_uniform = false;
        let mut hp_uniform = false;
        let mut lp_qp = None;
        let mut hp_qp = None;
        if bands_present != BandsPresent::DcOnly {
            let _reserved_i = r.flag()?;
            lp_uniform = r.flag()?;
            if lp_uniform {
                // 8.4.1 sets NumLPQPs = 1 in this branch.
                lp_qp = Some(QpSets::read(r, 1, num_components)?);
            }
            if bands_present != BandsPresent::NoHighpass {
                let _reserved_j = r.flag()?;
                hp_uniform = r.flag()?;
                if hp_uniform {
                    hp_qp = Some(QpSets::read(r, 1, num_components)?);
                }
            }
        }

        if !r.align_to_byte()? {
            warnings.push(JxrWarning::NonZeroPadding);
        }

        Ok(Self {
            internal_clr_fmt,
            scaled,
            bands_present,
            chroma_centering_x,
            chroma_centering_y,
            num_components,
            shift_bits,
            len_mantissa,
            exp_bias,
            dc_uniform,
            lp_uniform,
            hp_uniform,
            dc_qp,
            lp_qp,
            hp_qp,
        })
    }
}

/// Everything above the tiles, read once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CodedImageHeaders {
    pub(crate) image: ImageHeader,
    pub(crate) primary: PlaneHeader,
    pub(crate) alpha: Option<PlaneHeader>,
    /// 8.5.3's `IndexOffsetTile[ ]`, or the single implicit zero offset when
    /// there is no index table and one tile packet.
    pub(crate) index_offsets: Vec<u64>,
    /// Byte offset, from the start of the coded image, of the first tile
    /// packet. 8.5.3 measures `IndexOffsetTile[ ]` from here.
    pub(crate) tile_base: u64,
    /// Table 26's per-tile macroblock counts, in 8.7.1's raster order.
    pub(crate) tile_mb_counts: Vec<u64>,
}

impl CodedImageHeaders {
    pub(crate) fn read(
        r: &mut BitReader<'_>,
        warnings: &mut Vec<JxrWarning>,
    ) -> Result<Self, JxrError> {
        let image = ImageHeader::read(r, warnings)?;
        let primary = PlaneHeader::read(r, &image, false, warnings)?;
        let alpha = if image.alpha_image_plane {
            Some(PlaneHeader::read(r, &image, true, warnings)?)
        } else {
            None
        };

        // 8.4.4: NumBandsOfPrimary is the primary plane's band count, and it
        // is what the index table is sized by even when an alpha plane is
        // present with fewer bands.
        let num_bands_of_primary = primary.bands_present.num_bands();
        if let Some(a) = &alpha {
            if a.bands_present.num_bands() > num_bands_of_primary {
                // Table 30: the alpha plane's NumBands shall not exceed the
                // primary's. A file that breaks this has an index table whose
                // size the decoder and the encoder disagree about.
                return Err(JxrError::BadAlphaPlane);
            }
        }

        let tiles = u64::from(image.num_ver_tiles) * u64::from(image.num_hor_tiles);
        let entries = if image.frequency_mode {
            tiles
                .checked_mul(u64::from(num_bands_of_primary))
                .ok_or(JxrError::BadTiling)?
        } else {
            tiles
        };

        let mut index_offsets = Vec::new();
        if image.index_table_present {
            let start_code = r.read_u32(16)?;
            if start_code != 0x0001 {
                // 8.5.2: the value shall be 0x0001. Without it the offsets
                // that follow are not offsets, and every tile seek would be
                // to an arbitrary place in the file.
                return Err(JxrError::BadIndexTable);
            }
            let n = usize::try_from(entries).map_err(|_| JxrError::BadTiling)?;
            // One entry is at least one byte on the wire, so an index table
            // claiming more entries than the codestream has bytes is refused
            // before the vector is reserved.
            if entries > r.bits_left() / 8 {
                return Err(JxrError::BadIndexTable);
            }
            index_offsets.reserve(n);
            for _ in 0..entries {
                index_offsets.push(r.vlw_esc()?);
            }
        } else {
            // 8.5.3: "When the number of tile packets is 1, the index offset
            // of the only packet is 0." With no index table there is no way
            // to seek, so more than one packet cannot be located.
            if entries != 1 {
                return Err(JxrError::BadIndexTable);
            }
            index_offsets.push(0);
        }

        // 8.2.1: SubsequentBytes, then the profile/level block and any
        // reserved bytes, then the tiles begin.
        let subsequent = r.vlw_esc()?;
        if subsequent > 0 {
            if subsequent < 4 {
                // 8.2.2: "shall not be less than 4" when non-zero.
                return Err(JxrError::BadProfileLevel);
            }
            let used = read_profile_level_info(r)?;
            let additional = subsequent
                .checked_sub(used)
                .ok_or(JxrError::BadProfileLevel)?;
            if additional > r.bits_left() / 8 {
                return Err(JxrError::Truncated);
            }
            for _ in 0..additional {
                // 8.2.3: RESERVED_A_BYTE, ignored.
                let _ = r.read(8)?;
            }
        }

        if !r.is_byte_aligned() {
            // Everything above is byte-sized or ends with 8.4.21's padding,
            // so reaching here unaligned would mean this module read a field
            // of the wrong width — a bug rather than a bad file.
            return Err(JxrError::Truncated);
        }
        let tile_base = r.byte_pos();
        let tile_mb_counts = image.tile_mb_counts();

        Ok(Self {
            image,
            primary,
            alpha,
            index_offsets,
            tile_base,
            tile_mb_counts,
        })
    }
}

/// 8.6.1's `PROFILE_LEVEL_INFO( )`, returning the byte count it consumed.
fn read_profile_level_info(r: &mut BitReader<'_>) -> Result<u64, JxrError> {
    let mut bytes = 0u64;
    loop {
        let _profile_idc = r.read_u32(8)?;
        let _level_idc = r.read_u32(8)?;
        let _reserved_l = r.read_u32(15)?;
        let last = r.flag()?;
        bytes = bytes.checked_add(4).ok_or(JxrError::BadProfileLevel)?;
        if last {
            return Ok(bytes);
        }
        // 8.6.4 says no combination repeats, which bounds the loop at 2^31
        // iterations — not a bound. The real one is the codestream: four
        // bytes an iteration, refused as soon as the remaining bytes cannot
        // hold another.
        if r.bits_left() < 32 {
            return Err(JxrError::BadProfileLevel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_boundaries_close_with_the_total() {
        assert_eq!(boundaries(&[2, 3], 10), Ok(vec![0, 2, 5, 10]));
        assert_eq!(boundaries(&[], 4), Ok(vec![0, 4]));
    }

    #[test]
    fn declared_tile_widths_that_overrun_the_image_refuse() {
        // 8.3.25 derives the last tile by subtraction; 10 - 12 underflows,
        // and an unchecked decoder would wrap to four billion macroblocks.
        assert_eq!(boundaries(&[12], 10), Err(JxrError::BadTiling));
        // Exactly consuming the image leaves the last tile empty, which
        // Table 24's strictly-increasing sequence forbids.
        assert_eq!(boundaries(&[10], 10), Err(JxrError::BadTiling));
        assert_eq!(boundaries(&[0], 10), Err(JxrError::BadTiling));
    }

    #[test]
    fn a_uniform_qp_set_repeats_across_components() {
        let mut r = BitReader::new(&[0b00_010101, 0b01_000000]);
        let set = QpSet::read(&mut r, 3).expect("a two-bit mode and one byte");
        assert_eq!(set.per_component, vec![0b01010101; 3]);
    }

    #[test]
    fn a_separate_qp_set_splits_luma_from_chroma() {
        // COMPONENT_MODE = 1 (SEPARATE), then two bytes.
        let mut r = BitReader::new(&[0b01_000000, 0b11_000000, 0b11_000000]);
        let set = QpSet::read(&mut r, 3).expect("mode plus two bytes");
        assert_eq!(set.per_component.len(), 3);
        assert_eq!(set.per_component[1], set.per_component[2]);
    }

    #[test]
    fn a_reserved_component_mode_refuses() {
        let mut r = BitReader::new(&[0b11_000000, 0, 0, 0, 0]);
        assert_eq!(
            QpSet::read(&mut r, 3),
            Err(JxrError::ReservedValue("COMPONENT_MODE"))
        );
    }
}
