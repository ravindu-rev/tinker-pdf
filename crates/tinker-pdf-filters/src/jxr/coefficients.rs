//! ITU-T T.832 8.7 to 8.12 and 9.4 to 9.8: the entropy-coded coefficient
//! layers and everything that turns them back into transform coefficients.
//!
//! # Why the parse and the decode are interleaved here
//!
//! Clause 8 is a parser and clause 9 is a decoder, and the Recommendation
//! keeps them in separate clauses — but they **cannot** run in separate
//! passes. 8.11.7's `AdaptiveHPScan( )` chooses between the horizontal and
//! vertical scan orders by `MBHPMode`, and 9.6.3.2 computes `MBHPMode` from
//! the macroblock's *decoded and dequantized* LP coefficients. The clause
//! says so in as many words: "AdaptiveHPScan( ) shall only be invoked on a
//! macroblock after the HP prediction direction computation process ... has
//! been invoked and completed for this macroblock."
//!
//! So one macroblock's work is: parse DC, decode DC (9.4.2), parse LP, decode
//! LP (9.4.3), compute `MBHPMode`, parse CBPHP and HP, decode HP (9.4.4).
//! That ordering is why this file holds both halves rather than splitting on
//! the clause boundary the Recommendation uses.
//!
//! # Three orderings that are easy to get backwards, and are not
//!
//! **`PredDCLP` is captured before dequantization.** 9.4.2 runs remap, then
//! prediction, then `DequantizeDCCoefficients( )`, and 9.6.1.5's update of the
//! prediction variables happens *inside* the prediction step — so the value a
//! neighbouring macroblock predicts from is the pre-dequantization one.
//! Storing the dequantized value instead decodes a plausible picture that is
//! wrong wherever the quantizer changes.
//!
//! **HP prediction runs after dequantization, DC and LP prediction before
//! it.** 9.4.4 orders `CalcHPPredMode`, remap, dequantize, *then*
//! `HPCoefficientPrediction( )`. That asymmetry is the clause's and not a
//! transcription slip.
//!
//! **HP block indices are hierarchical on the wire and raster in the
//! buffer.** 8.7.17.1 stores CBPHP in "hierarchical raster scan order", where
//! consecutive nibbles are 2x2 block groups; 9.9.4's combination indexes
//! blocks in plain raster order. [`tables::HIER_SCAN_ORDER`] is the one place
//! the two meet, and it is applied at parse time so that everything below
//! this file sees raster order only.
//!
//! # What this build implements
//!
//! `INTERNAL_CLR_FMT` is YONLY or YUV444 — every subsampled and
//! four-component internal layout is refused by name in [`super`], so the
//! YUV420, YUV422 and YUVK branches of 8.7.16.1, 8.7.17.2, 9.5, 9.6 and 9.8
//! are **absent rather than written and unreachable**. A branch nothing can
//! enter is a branch nothing tests.
//!
//! # Bounds and damage (rulings 1 and 2)
//!
//! Every arithmetic path a codestream can drive is checked, saturating or
//! wrapping — never a debug panic. Levels accumulate in `i64` and are refused
//! when they leave `i32`, because 8.7.13's escape mode can describe a
//! magnitude near 2^29 and 8.7.12 then shifts it by up to fifteen more
//! places: a product no conformant codestream contains and which a fuzzer
//! reaches in seconds. A tile whose parse fails that way is **dropped to
//! zero** and recorded as [`JxrWarning::TileDroppedAsZero`] — which is
//! 8.7.10.1's own NOTE 1 and ruling 2's degrade path, so the rest of the
//! image still decodes.

#![deny(clippy::float_arithmetic)]

use super::bitstream::BitReader;
use super::headers::{BandsPresent, CodedImageHeaders, InternalClrFmt, PlaneHeader, QpSet, QpSets};
use super::tables;
use super::{JxrError, JxrRefusal, JxrWarning};

/// The reconstructed image planes, one sample array per component, each in
/// its own `ExtendedWidth[i]` x `ExtendedHeight[i]` geometry (6.2).
///
/// Every internal colour format this build accepts is 4:4:4, so all
/// components share one geometry and the dimensions are scalars rather than
/// per-component arrays.
#[allow(dead_code)] // Milestone 4's colour.rs is the first reader.
pub(crate) struct Planes {
    pub(crate) samples: Vec<Vec<i32>>,
    /// `ExtendedWidth[0]`, not the output width — cropping is `colour.rs`.
    pub(crate) width: u32,
    /// `ExtendedHeight[0]`.
    pub(crate) height: u32,
}

/// Decodes the primary image plane's coefficient layers and reconstructs its
/// samples.
///
/// **This stops short of pixels.** All of clause 8 and all of clause 9.4 to
/// 9.9 run: the samples are reconstructed, in the internal colour format and
/// the extended geometry. What is outstanding is 9.10's output formatting —
/// the colour transform back to RGB or grey, the bias, the bit depths and the
/// crop — so the result is refused by name rather than returned as a raster
/// whose numbers mean something other than what the file said.
pub(crate) fn decode_image(
    r: &mut BitReader<'_>,
    h: &CodedImageHeaders,
    warnings: &mut Vec<JxrWarning>,
) -> Result<Planes, JxrError> {
    let g = PlaneGeometry::from_headers(h, &h.primary)?;
    let mut d = PlaneDecoder::new(&g)?;
    d.parse_tiles(r, h, &h.primary, warnings)?;
    d.reconstruct();
    Err(JxrError::Unsupported(JxrRefusal::OutputFormatting))
}

// --- 8.8: adaptive VLC code table selection -----------------------------

/// 8.8's `AdaptiveVLC` data structure.
#[derive(Clone, Copy, Debug)]
struct AdaptiveVlc {
    table_index: usize,
    delta_table_index: usize,
    delta2_table_index: usize,
    discrim1: i32,
    discrim2: i32,
}

impl AdaptiveVlc {
    /// 8.8.3.5's `InitializeVLCTable1( )` — a symbol with exactly two code
    /// tables, where only `DiscrimVal1` exists.
    const fn init1() -> Self {
        Self {
            table_index: 0,
            delta_table_index: 0,
            delta2_table_index: 0,
            discrim1: 0,
            discrim2: 0,
        }
    }

    /// 8.8.3.6's `InitializeVLCTable2( )` — three or more code tables.
    /// `TableIndex` starts at **1**, not 0; that is the clause's own choice.
    const fn init2() -> Self {
        Self {
            table_index: 1,
            delta_table_index: 0,
            delta2_table_index: 1,
            discrim1: 0,
            discrim2: 0,
        }
    }

    /// 8.8.4.4's `AdaptVLCTable1( )`.
    fn adapt1(&mut self) {
        if self.discrim1 < -8 && self.table_index != 0 {
            self.table_index -= 1;
            self.discrim1 = 0;
        } else if self.discrim1 > 8 && self.table_index != 1 {
            self.table_index += 1;
            self.discrim1 = 0;
        } else {
            self.discrim1 = self.discrim1.clamp(-64, 64);
        }
    }

    /// 8.8.4.5's `AdaptVLCTable2( )`. `DiscrimVal1` only ever moves the table
    /// index down and `DiscrimVal2` only ever moves it up.
    fn adapt2(&mut self, max_table_index: usize) {
        let mut changed = false;
        if self.discrim1 < -8 && self.table_index != 0 {
            self.table_index -= 1;
            changed = true;
        } else if self.discrim2 > 8 && self.table_index != max_table_index {
            self.table_index += 1;
            changed = true;
        }
        if changed {
            self.discrim1 = 0;
            self.discrim2 = 0;
            if self.table_index == max_table_index {
                self.delta_table_index = self.table_index.saturating_sub(1);
                self.delta2_table_index = self.table_index.saturating_sub(1);
            } else if self.table_index == 0 {
                self.delta_table_index = 0;
                self.delta2_table_index = 0;
            } else {
                self.delta_table_index = self.table_index - 1;
                self.delta2_table_index = self.table_index;
            }
        } else {
            self.discrim1 = self.discrim1.clamp(-64, 64);
            self.discrim2 = self.discrim2.clamp(-64, 64);
        }
    }
}

/// Slots for every `AdaptiveVLC` instance 8.8.3 names, in the order
/// 8.8.3.1 to 8.8.3.4 name them.
///
/// Plain constants rather than an enum: the only operation is indexing an
/// array, and an enum whose variants are never constructed — only cast for
/// their discriminant — reads to the compiler as dead code.
mod slot {
    pub(super) const ABS_LEVEL_DC_LUM: usize = 0;
    pub(super) const ABS_LEVEL_DC_CHR: usize = 1;
    pub(super) const FIRST_IND_LP_LUM: usize = 2;
    pub(super) const IND_LP_LUM0: usize = 3;
    pub(super) const IND_LP_LUM1: usize = 4;
    pub(super) const FIRST_IND_LP_CHR: usize = 5;
    pub(super) const IND_LP_CHR0: usize = 6;
    pub(super) const IND_LP_CHR1: usize = 7;
    pub(super) const ABS_LEVEL_LP0: usize = 8;
    pub(super) const ABS_LEVEL_LP1: usize = 9;
    pub(super) const FIRST_IND_HP_LUM: usize = 10;
    pub(super) const IND_HP_LUM0: usize = 11;
    pub(super) const IND_HP_LUM1: usize = 12;
    pub(super) const FIRST_IND_HP_CHR: usize = 13;
    pub(super) const IND_HP_CHR0: usize = 14;
    pub(super) const IND_HP_CHR1: usize = 15;
    pub(super) const ABS_LEVEL_HP0: usize = 16;
    pub(super) const ABS_LEVEL_HP1: usize = 17;
    pub(super) const NUM_CBPHP: usize = 18;
    pub(super) const NUM_BLK_CBPHP: usize = 19;
    pub(super) const COUNT: usize = 20;
}

/// The twelve slots 8.8.3.6 initializes — every `FIRST_INDEX` and `INDEX_A`
/// structure, which are the ones with more than two code tables.
const MULTI_TABLE_SLOTS: [usize; 12] = [
    slot::FIRST_IND_LP_LUM,
    slot::IND_LP_LUM0,
    slot::IND_LP_LUM1,
    slot::FIRST_IND_LP_CHR,
    slot::IND_LP_CHR0,
    slot::IND_LP_CHR1,
    slot::FIRST_IND_HP_LUM,
    slot::IND_HP_LUM0,
    slot::IND_HP_LUM1,
    slot::FIRST_IND_HP_CHR,
    slot::IND_HP_CHR0,
    slot::IND_HP_CHR1,
];

// --- 8.12: adaptive coefficient normalization ---------------------------

/// 8.12's `Model` data structure: `MState` and `MBits`, luma and chroma.
#[derive(Clone, Copy, Debug)]
struct Model {
    state: [i32; 2],
    bits: [i32; 2],
}

impl Model {
    /// 8.12.1's `InitializeModelMB( )`.
    const fn init(band: i32) -> Self {
        let b = (2 - band) * 4;
        Self {
            state: [0, 0],
            bits: [b, b],
        }
    }

    /// 8.12.2's `UpdateModelMB( )` for the 4:4:4 and single-component
    /// layouts. `iWeight2[ ]`'s YUV420 and YUV422 rows are absent because no
    /// codestream reaching here has those internal formats.
    fn update(&mut self, lap_mean: [i64; 2], band: usize, fmt: InternalClrFmt, components: usize) {
        const MODEL_WEIGHT: i64 = 70;
        let mut mean = lap_mean;
        mean[0] = mean[0].saturating_mul(i64::from(tables::MODEL_WEIGHT_0[band]));
        let w1 = tables::MODEL_WEIGHT_1[band]
            .get(components.saturating_sub(1))
            .copied()
            .unwrap_or(0);
        mean[1] = mean[1].saturating_mul(i64::from(w1));
        if band == 2 {
            mean[1] >>= 4;
        }
        let models = usize::from(!matches!(fmt, InternalClrFmt::YOnly)) + 1;
        for (j, &lap) in mean.iter().enumerate().take(models) {
            let mut ms = i64::from(self.state[j]);
            let mut delta = (lap - MODEL_WEIGHT) >> 2;
            if delta <= -8 {
                delta += 4;
                if delta < -16 {
                    delta = -16;
                }
                ms += delta;
                if ms < -8 {
                    if self.bits[j] == 0 {
                        ms = -8;
                    } else {
                        ms = 0;
                        self.bits[j] -= 1;
                    }
                }
            } else if delta >= 8 {
                delta -= 4;
                if delta > 15 {
                    delta = 15;
                }
                ms += delta;
                if ms > 8 {
                    if self.bits[j] >= 15 {
                        self.bits[j] = 15;
                        ms = 8;
                    } else {
                        ms = 0;
                        self.bits[j] += 1;
                    }
                }
            }
            // Bounded by the branches above plus a delta in [-16, 15]; the
            // clamp guards a future edit rather than a reachable path.
            self.state[j] = ms.clamp(-64, 64) as i32;
        }
    }
}

// --- 8.10: adaptive CBPHP prediction ------------------------------------

/// 8.10's `CBPHPModelHP` data structure.
#[derive(Clone, Copy, Debug)]
struct CbpHpModel {
    state: [i32; 2],
    ones: [i32; 2],
    zeroes: [i32; 2],
}

impl CbpHpModel {
    /// 8.10.1's `InitializeCBPHPModel( )`.
    const fn init() -> Self {
        Self {
            state: [0, 0],
            ones: [-4, -4],
            zeroes: [4, 4],
        }
    }

    /// 8.10.2's `UpdateCBPHPModel( )`.
    fn update(&mut self, i: usize, n_orig: i32) {
        const N_DIFF: i32 = 3;
        let i = i.min(1);
        self.ones[i] = (self.ones[i] + n_orig - N_DIFF).clamp(-16, 15);
        self.zeroes[i] = (self.zeroes[i] + 16 - n_orig - N_DIFF).clamp(-16, 15);
        self.state[i] = if self.ones[i] < 0 {
            if self.ones[i] < self.zeroes[i] {
                1
            } else {
                2
            }
        } else if self.zeroes[i] < 0 {
            2
        } else {
            0
        };
    }
}

// --- 8.11: adaptive inverse scanning ------------------------------------

/// One adaptive scan order and the totals that reorder it (8.11.1).
#[derive(Clone, Copy, Debug)]
struct Scan {
    order: [u8; 16],
    totals: [i32; 16],
}

impl Scan {
    const fn new(order: [u8; 16]) -> Self {
        Self {
            order,
            totals: tables::SCAN_TOTALS,
        }
    }

    /// The body 8.11.6's `AdaptiveLPScan( )` and 8.11.7's `AdaptiveHPScan( )`
    /// share: return the raster position, bump the total, and bubble the
    /// entry one place forward if it has overtaken its neighbour.
    ///
    /// `None` when `i` is outside 1..=15, which a conformant codestream never
    /// produces — a block holds fifteen coefficients at positions 1 to 15.
    fn place(&mut self, i: usize) -> Option<usize> {
        if !(1..=15).contains(&i) {
            return None;
        }
        let k = usize::from(self.order[i]);
        self.totals[i] += 1;
        if i > 1 && self.totals[i] > self.totals[i - 1] {
            self.totals.swap(i, i - 1);
            self.order.swap(i, i - 1);
        }
        Some(k)
    }

    /// 8.11.4 and 8.11.5's `ResetTotals...( )`.
    fn reset_totals(&mut self) {
        self.totals = tables::SCAN_TOTALS;
    }
}

// --- the per-tile adaptive state ----------------------------------------

/// Everything `bInitializeContext` resets at the top-left macroblock of a
/// tile, held together so that "initialize the context" is one expression.
struct Context {
    vlc: [AdaptiveVlc; slot::COUNT],
    model_dc: Model,
    model_lp: Model,
    model_hp: Model,
    cbphp: CbpHpModel,
    scan_lp: Scan,
    scan_hp_hor: Scan,
    scan_hp_ver: Scan,
    /// 8.9's `CountZeroCBPLP` and `CountMaxCBPLP`.
    count_zero_cbplp: i32,
    count_max_cbplp: i32,
}

impl Context {
    fn new() -> Self {
        let mut vlc = [AdaptiveVlc::init1(); slot::COUNT];
        for i in MULTI_TABLE_SLOTS {
            vlc[i] = AdaptiveVlc::init2();
        }
        Self {
            vlc,
            model_dc: Model::init(0),
            model_lp: Model::init(1),
            model_hp: Model::init(2),
            cbphp: CbpHpModel::init(),
            scan_lp: Scan::new(tables::SCAN_ORDER_0),
            scan_hp_hor: Scan::new(tables::SCAN_ORDER_0),
            scan_hp_ver: Scan::new(tables::SCAN_ORDER_1),
            count_zero_cbplp: 1,
            count_max_cbplp: 1,
        }
    }

    /// 8.8.4.1's `AdaptDC( )`.
    fn adapt_dc(&mut self) {
        self.vlc[slot::ABS_LEVEL_DC_LUM].adapt1();
        self.vlc[slot::ABS_LEVEL_DC_CHR].adapt1();
    }

    /// 8.8.4.2's `AdaptLP( )`. The `iMaxTableIndex` arguments are the
    /// clause's: 4 for the five-table `FIRST_INDEX` structures and 3 for the
    /// four-table `INDEX_A` ones.
    fn adapt_lp(&mut self) {
        self.vlc[slot::FIRST_IND_LP_LUM].adapt2(4);
        self.vlc[slot::IND_LP_LUM0].adapt2(3);
        self.vlc[slot::IND_LP_LUM1].adapt2(3);
        self.vlc[slot::FIRST_IND_LP_CHR].adapt2(4);
        self.vlc[slot::IND_LP_CHR0].adapt2(3);
        self.vlc[slot::IND_LP_CHR1].adapt2(3);
        self.vlc[slot::ABS_LEVEL_LP0].adapt1();
        self.vlc[slot::ABS_LEVEL_LP1].adapt1();
    }

    /// 8.8.4.3's `AdaptHP( )`, which also adapts the two CBPHP structures.
    fn adapt_hp(&mut self) {
        self.vlc[slot::FIRST_IND_HP_LUM].adapt2(4);
        self.vlc[slot::IND_HP_LUM0].adapt2(3);
        self.vlc[slot::IND_HP_LUM1].adapt2(3);
        self.vlc[slot::FIRST_IND_HP_CHR].adapt2(4);
        self.vlc[slot::IND_HP_CHR0].adapt2(3);
        self.vlc[slot::IND_HP_CHR1].adapt2(3);
        self.vlc[slot::ABS_LEVEL_HP0].adapt1();
        self.vlc[slot::ABS_LEVEL_HP1].adapt1();
        self.vlc[slot::NUM_CBPHP].adapt1();
        self.vlc[slot::NUM_BLK_CBPHP].adapt1();
    }
}

// --- VLC reading ---------------------------------------------------------

/// Reads one variable-length code.
///
/// The accumulator is compared against the whole table at each length, which
/// is why the tables need no builder — see `tables.rs`. A code that reaches
/// `MAX_CODE_BITS` without matching is not in the table, which means the
/// reader is not where it thinks it is: that is [`JxrError::Truncated`], and
/// the tile is dropped rather than the file refused (ruling 2).
fn read_vlc(r: &mut BitReader<'_>, table: &[tables::Code]) -> Result<u8, JxrError> {
    /// The longest code in `tables.rs` is eight bits (`FIRST_INDEX` code
    /// table 4). The guard is generous rather than tight so that a table edit
    /// cannot silently start refusing valid codes.
    const MAX_CODE_BITS: u8 = 16;
    let mut acc: u16 = 0;
    let mut len: u8 = 0;
    loop {
        // The cast is exact: `read(1)` returns 0 or 1.
        acc = (acc << 1) | (r.read(1)? as u16);
        len += 1;
        for &(l, c, v) in table {
            if l == len && c == acc {
                return Ok(v);
            }
        }
        if len >= MAX_CODE_BITS {
            return Err(boom("read_vlc: no code matched"));
        }
    }
}

// --- geometry and quantization ------------------------------------------

/// The geometry one image plane's coefficient decode needs, lifted out of
/// [`CodedImageHeaders`] so nothing below reaches back into a header field.
pub(crate) struct PlaneGeometry {
    pub(crate) components: usize,
    pub(crate) scaled: bool,
    pub(crate) mb_width: usize,
    pub(crate) mb_height: usize,
    pub(crate) ext_width: usize,
    pub(crate) ext_height: usize,
    /// 8.3.10's `OVERLAP_MODE` and 8.3.4's `HARD_TILING_FLAG`, both read by
    /// 9.9.3 and 9.9.6.
    pub(crate) overlap_mode: u8,
    pub(crate) hard_tiling: bool,
    pub(crate) trim_flexbits_flag: bool,
    pub(crate) num_tile_cols: usize,
    pub(crate) num_tile_rows: usize,
    pub(crate) left_mb_of_tile: Vec<u32>,
    pub(crate) top_mb_of_tile: Vec<u32>,
}

impl PlaneGeometry {
    pub(crate) fn from_headers(
        h: &CodedImageHeaders,
        plane: &PlaneHeader,
    ) -> Result<Self, JxrError> {
        let usz = |v: u32| usize::try_from(v).map_err(|_| JxrError::BadDimensions);
        Ok(Self {
            components: usz(plane.num_components)?,
            scaled: plane.scaled,
            mb_width: usz(h.image.mb_width)?,
            mb_height: usz(h.image.mb_height)?,
            ext_width: usz(h.image.extended_width)?,
            ext_height: usz(h.image.extended_height)?,
            overlap_mode: h.image.overlap_mode,
            hard_tiling: h.image.hard_tiling,
            trim_flexbits_flag: h.image.trim_flexbits,
            // 8.3.23's NUM_VER_TILES is the number of tile *columns* and
            // 8.3.24's NUM_HOR_TILES the number of tile *rows*; the names read
            // backwards and are renamed here once rather than at every use.
            num_tile_cols: usz(h.image.num_ver_tiles)?,
            num_tile_rows: usz(h.image.num_hor_tiles)?,
            left_mb_of_tile: h.image.left_mb_of_tile.clone(),
            top_mb_of_tile: h.image.top_mb_of_tile.clone(),
        })
    }
}

/// The quantization parameters in force for one tile, after 9.7's derivation.
struct TileQp {
    dc: QpSet,
    lp: QpSets,
    hp: QpSets,
    num_lp_qps: u32,
    num_hp_qps: u32,
    use_dc_qp: bool,
    use_lp_qp: bool,
}

/// 9.8.4's `QuantMap( )`.
///
/// `iExp` is never negative for any `iQP` and `iScaledShift` the clause
/// admits — the unscaled branch subtracts 2 only where `iQP >> 4` is at least
/// 2 — so the shift is well defined and the result fits `i32`: the largest
/// value is `31 << 15`.
fn quant_map(qp: u8, scaled_shift: u32, scaled_flag: bool) -> i32 {
    let qp = i32::from(qp);
    if qp == 0 {
        return 1;
    }
    let (man, exp) = if scaled_flag {
        if qp < 16 {
            (qp, scaled_shift)
        } else {
            // `qp >= 16` makes `(qp >> 4) - 1` non-negative.
            (16 + (qp % 16), ((qp >> 4) - 1) as u32 + scaled_shift)
        }
    } else {
        const NOT_SCALED_SHIFT: i32 = -2;
        if qp < 32 {
            ((qp + 3) >> 2, 0)
        } else if qp < 48 {
            // `qp >> 4` is exactly 2 here, so the exponent is 0.
            (
                (16 + (qp % 16) + 1) >> 1,
                ((qp >> 4) + NOT_SCALED_SHIFT) as u32,
            )
        } else {
            // `qp >= 48` makes `qp >> 4` at least 3.
            (16 + (qp % 16), ((qp >> 4) - 1 + NOT_SCALED_SHIFT) as u32)
        }
    };
    // `exp` is at most 15 and `man` at most 31, so this cannot overflow.
    man << exp.min(30)
}

/// A coefficient magnitude that has left `i32`. Not a file-level refusal: it
/// means *this tile* is not what the reader thinks it is, so the tile is
/// dropped and the image keeps its others (ruling 2).
/// A desynchronised bit reader, named at the point it was noticed.
///
/// Every caller means the same thing — "the bits here are not the syntax
/// element the parse expected" — so they share one error. The `&'static str`
/// is not carried into [`JxrError`] on purpose: a caller cannot act on
/// *where* a tile desynchronised, only on the fact that it did, and 8.7.10.1's
/// remedy is the same either way. It is a name for a reader of this file.
const fn boom(_where: &'static str) -> JxrError {
    JxrError::Truncated
}

fn narrow(v: i64) -> Result<i32, JxrError> {
    i32::try_from(v).map_err(|_| JxrError::Truncated)
}

// --- the plane decoder ---------------------------------------------------

/// One image plane's coefficient state for the whole image.
pub(crate) struct PlaneDecoder<'a> {
    g: &'a PlaneGeometry,
    /// `MbDCLP[MBx][MBy][i][j]`, flattened to `(mb * components + i) * 16 + j`.
    pub(crate) dclp: Vec<i32>,
    /// `PredDCLP[MBx][MBy][i][j]` for j in 0..=6. Held separately from
    /// `dclp` because 9.6.1.5 and 9.6.2.5 capture it **before**
    /// dequantization, so it is not a view of the same numbers.
    pred: Vec<i32>,
    /// `MBCBPHP[MBx][MBy][i]`.
    cbphp: Vec<u32>,
    /// `MBQPIndexLP` and `MBQPIndexHP`, one each per macroblock.
    qp_index_lp: Vec<u8>,
    qp_index_hp: Vec<u8>,
    /// `ImagePlane[i][x][y]`. HP coefficients are scattered here at parse
    /// time, at 9.9.4's own positions, so that no whole-image `MBBuffer` has
    /// to exist — see [`Self::decode_hp`].
    pub(crate) plane: Vec<Vec<i32>>,
    /// `ModelBitsMBHP[MBx][MBy][ ]`, which 8.7.19.2 needs when the flexbits
    /// arrive in a packet of their own.
    model_bits_hp: Vec<[i32; 2]>,
    /// `MBDCMode` and `MBHPMode` for the macroblock being decoded.
    mb_dc_mode: u8,
    mb_hp_mode: u8,
    /// Where each packet's parse actually stopped, in bytes from the start of
    /// the coded image, indexed **exactly as 8.5.3's `IndexOffsetTile[ ]` is**
    /// — one entry per tile in spatial mode, one per tile per band in
    /// frequency mode.
    ///
    /// The index table says where each packet *starts*, so the next larger
    /// entry says where this one must end. Comparing the two is a total check
    /// that the entropy decoder stayed in step with the encoder, and it is
    /// recorded by the decoder rather than reconstructed by a test because
    /// only the decoder knows where it actually stopped.
    pub(crate) packet_ends: Vec<Option<u64>>,
}

impl<'a> PlaneDecoder<'a> {
    pub(crate) fn new(g: &'a PlaneGeometry) -> Result<Self, JxrError> {
        let mbs = g
            .mb_width
            .checked_mul(g.mb_height)
            .ok_or(JxrError::BadDimensions)?;
        let per_component = mbs
            .checked_mul(g.components)
            .ok_or(JxrError::BadDimensions)?;
        let plane_len = g
            .ext_width
            .checked_mul(g.ext_height)
            .ok_or(JxrError::BadDimensions)?;
        Ok(Self {
            g,
            dclp: vec![
                0;
                per_component
                    .checked_mul(16)
                    .ok_or(JxrError::BadDimensions)?
            ],
            pred: vec![
                0;
                per_component
                    .checked_mul(7)
                    .ok_or(JxrError::BadDimensions)?
            ],
            cbphp: vec![0; per_component],
            qp_index_lp: vec![0; mbs],
            qp_index_hp: vec![0; mbs],
            plane: vec![vec![0; plane_len]; g.components],
            model_bits_hp: vec![[0; 2]; mbs],
            mb_dc_mode: 3,
            mb_hp_mode: 2,
            packet_ends: Vec::new(),
        })
    }

    fn dclp_at(&self, mb: usize, i: usize, j: usize) -> i32 {
        self.dclp[(mb * self.g.components + i) * 16 + j]
    }

    fn dclp_set(&mut self, mb: usize, i: usize, j: usize, v: i32) {
        self.dclp[(mb * self.g.components + i) * 16 + j] = v;
    }

    fn pred_at(&self, mb: usize, i: usize, j: usize) -> i32 {
        self.pred[(mb * self.g.components + i) * 7 + j]
    }

    fn pred_set(&mut self, mb: usize, i: usize, j: usize, v: i32) {
        self.pred[(mb * self.g.components + i) * 7 + j] = v;
    }
}

// --- clause 8.7: the tile walk ------------------------------------------

/// Scratch for one macroblock's parse outputs, allocated once per plane so
/// that a four-thousand-macroblock image does not allocate four thousand
/// times.
struct Scratch {
    /// `DCInput[i]`.
    dc: Vec<i32>,
    /// `LPInput[i][j]`, sixteen per component with index 0 unused.
    lp: Vec<[i32; 16]>,
    /// `HPInputVLC[i][blk][j]`, flattened to `16 * blk + j` with **raster**
    /// block indices — see the module docs.
    hp_vlc: Vec<[i32; 256]>,
    /// `HPInputFlex[i][blk][j]`, same layout.
    hp_flex: Vec<[i32; 256]>,
    /// `iRLCoeffs[ ]` / `iLocalCoeff[ ]`: run and level, interleaved.
    rl: [i32; 32],
}

impl Scratch {
    fn new(components: usize) -> Self {
        Self {
            dc: vec![0; components],
            lp: vec![[0; 16]; components],
            hp_vlc: vec![[0; 256]; components],
            hp_flex: vec![[0; 256]; components],
            rl: [0; 32],
        }
    }
}

/// Which macroblock, and where it sits in its tile: the flags 8.7.11 and 9.6
/// branch on, computed once.
#[derive(Clone, Copy)]
struct MbPos {
    x: usize,
    y: usize,
    index: usize,
    left_edge: bool,
    top_edge: bool,
    /// 8.7.11's `bResetContext`.
    reset_context: bool,
    /// 8.7.16.1's `bResetTotals`.
    reset_totals: bool,
}

impl PlaneDecoder<'_> {
    /// 8.7.1's `CODED_TILES( )`.
    pub(crate) fn parse_tiles(
        &mut self,
        r: &mut BitReader<'_>,
        h: &CodedImageHeaders,
        plane: &PlaneHeader,
        warnings: &mut Vec<JxrWarning>,
    ) -> Result<(), JxrError> {
        let tiles = self.tile_count();
        self.packet_ends = vec![None; h.index_offsets.len()];
        let mut scratch = Scratch::new(self.g.components);
        if h.image.frequency_mode {
            self.parse_frequency(r, h, plane, tiles, &mut scratch, warnings);
        } else {
            for n in 0..tiles {
                let mb_count = h.tile_mb_counts.get(n).copied().unwrap_or(0);
                if self
                    .try_tile_spatial(r, h, plane, n, mb_count, &mut scratch)
                    .is_err()
                {
                    self.drop_tile(n);
                    super::push_once(warnings, JxrWarning::TileDroppedAsZero);
                } else if let Some(slot) = self.packet_ends.get_mut(n) {
                    *slot = Some(r.byte_pos());
                }
            }
            self.refuse_overrunning_tiles(h, tiles, warnings);
        }
        Ok(())
    }

    /// Drops any spatial tile whose parse ran **past** the byte at which the
    /// next tile packet begins.
    ///
    /// 8.5.3 gives each packet's start, so the next packet's start is this
    /// one's ceiling. A decoder that reads beyond it has lost synchronisation
    /// with the encoder — one wrong VLC table or one missed adaptation is
    /// enough — and every macroblock it "decoded" past that point is noise
    /// shaped like a picture. Under-reading is *not* refused here: the clause
    /// does not forbid an encoder from leaving slack between packets, so a
    /// short parse is only evidence when the fixture is known to be packed,
    /// which is a test's job rather than the decoder's.
    fn refuse_overrunning_tiles(
        &mut self,
        h: &CodedImageHeaders,
        tiles: usize,
        warnings: &mut Vec<JxrWarning>,
    ) {
        for n in 0..tiles {
            let Some(Some(end)) = self.packet_ends.get(n).copied() else {
                continue;
            };
            let Some(&next) = h.index_offsets.get(n + 1) else {
                continue;
            };
            let Some(ceiling) = h.tile_base.checked_add(next) else {
                continue;
            };
            if end > ceiling {
                self.drop_tile(n);
                self.packet_ends[n] = None;
                super::push_once(warnings, JxrWarning::TileDroppedAsZero);
            }
        }
    }

    fn tile_count(&self) -> usize {
        self.g.num_tile_cols * self.g.num_tile_rows
    }

    /// Seeks to one spatial tile packet and parses it.
    fn try_tile_spatial(
        &mut self,
        r: &mut BitReader<'_>,
        h: &CodedImageHeaders,
        plane: &PlaneHeader,
        tile: usize,
        mb_count: u64,
        scratch: &mut Scratch,
    ) -> Result<(), JxrError> {
        let offset = h.index_offsets.get(tile).copied().unwrap_or(0);
        let start = h.tile_base.checked_add(offset).ok_or(JxrError::Truncated)?;
        r.seek_byte(start)?;
        self.parse_tile_spatial(r, plane, tile, mb_count, scratch)
    }

    /// 8.7.1's frequency-mode half: one pass over the tiles per band.
    ///
    /// The FLEXBITS packet is read through a **second reader**, positioned by
    /// the index table and stepped alongside the HIGHPASS pass, rather than in
    /// a fifth pass over stored coefficients. That is what lets this decoder
    /// hold no whole-image `MBBuffer`: 8.7.19.1 walks a tile's macroblocks in
    /// exactly the order 8.7.18.2 does, so the two readers stay in step and
    /// each macroblock's flexbits arrive while its VLC coefficients are still
    /// in [`Scratch`].
    fn parse_frequency(
        &mut self,
        r: &mut BitReader<'_>,
        h: &CodedImageHeaders,
        plane: &PlaneHeader,
        tiles: usize,
        scratch: &mut Scratch,
        warnings: &mut Vec<JxrWarning>,
    ) {
        let bands = usize::try_from(plane.bands_present.num_bands()).unwrap_or(1);
        // One slot per tile, holding its derived quantization parameters.
        // `None` means the tile has already failed and its later bands are
        // not attempted — the DC packet is what the LP packet predicts from,
        // so decoding LP over a dropped DC would build on zeroes and call the
        // result a picture.
        let mut qps: Vec<Option<TileQp>> = Vec::with_capacity(tiles);
        // Pass 1: every tile's DC packet, decoded as it is parsed so that the
        // LP pass has something to predict from.
        for n in 0..tiles {
            let mb_count = h.tile_mb_counts.get(n).copied().unwrap_or(0);
            match self.try_tile_dc(r, h, plane, n, bands, mb_count, scratch) {
                Ok(qp) => {
                    self.record_packet_end(n * bands, r.byte_pos());
                    qps.push(Some(qp));
                }
                Err(_) => {
                    self.drop_tile(n);
                    super::push_once(warnings, JxrWarning::TileDroppedAsZero);
                    qps.push(None);
                }
            }
        }
        if bands < 2 {
            return;
        }
        // Pass 2: the LP packets.
        for (n, slot) in qps.iter_mut().enumerate() {
            let mb_count = h.tile_mb_counts.get(n).copied().unwrap_or(0);
            let ok = match slot.as_mut() {
                Some(qp) => self
                    .try_tile_lowpass(r, h, plane, n, bands, mb_count, qp, scratch)
                    .is_ok(),
                None => continue,
            };
            if ok {
                self.record_packet_end(n * bands + 1, r.byte_pos());
            } else {
                *slot = None;
                self.drop_tile(n);
                super::push_once(warnings, JxrWarning::TileDroppedAsZero);
            }
        }
        if bands < 3 {
            return;
        }
        // Pass 3: the HP packets, with the FLEXBITS packet read alongside.
        let data = r.data();
        for (n, slot) in qps.iter_mut().enumerate() {
            let mb_count = h.tile_mb_counts.get(n).copied().unwrap_or(0);
            let ok = match slot.as_mut() {
                Some(qp) => self
                    .try_tile_highpass(r, data, h, plane, n, bands, mb_count, qp, scratch)
                    .is_ok(),
                None => continue,
            };
            if ok {
                self.record_packet_end(n * bands + 2, r.byte_pos());
            } else {
                *slot = None;
                self.drop_tile(n);
                super::push_once(warnings, JxrWarning::TileDroppedAsZero);
            }
        }
    }

    fn record_packet_end(&mut self, at: usize, end: u64) {
        if let Some(slot) = self.packet_ends.get_mut(at) {
            *slot = Some(end);
        }
    }

    /// 8.5.3's `IndexOffsetTile[ ]` for one tile and band, as an absolute
    /// byte offset in the coded image.
    fn band_start(
        h: &CodedImageHeaders,
        tile: usize,
        bands: usize,
        band: usize,
    ) -> Result<u64, JxrError> {
        let offset = h
            .index_offsets
            .get(tile * bands + band)
            .copied()
            .ok_or(JxrError::BadIndexTable)?;
        h.tile_base.checked_add(offset).ok_or(JxrError::Truncated)
    }

    #[allow(clippy::too_many_arguments)]
    fn try_tile_dc(
        &mut self,
        r: &mut BitReader<'_>,
        h: &CodedImageHeaders,
        plane: &PlaneHeader,
        tile: usize,
        bands: usize,
        mb_count: u64,
        scratch: &mut Scratch,
    ) -> Result<TileQp, JxrError> {
        r.seek_byte(Self::band_start(h, tile, bands, 0)?)?;
        self.parse_tile_dc(r, plane, tile, mb_count, scratch)
    }

    #[allow(clippy::too_many_arguments)]
    fn try_tile_lowpass(
        &mut self,
        r: &mut BitReader<'_>,
        h: &CodedImageHeaders,
        plane: &PlaneHeader,
        tile: usize,
        bands: usize,
        mb_count: u64,
        qp: &mut TileQp,
        scratch: &mut Scratch,
    ) -> Result<(), JxrError> {
        r.seek_byte(Self::band_start(h, tile, bands, 1)?)?;
        Self::tile_prologue(r, tile)?;
        Self::parse_tile_lowpass_headers(r, plane, qp)?;
        self.walk_tile_lowpass(r, plane, tile, mb_count, qp, scratch)
    }

    #[allow(clippy::too_many_arguments)]
    fn try_tile_highpass(
        &mut self,
        r: &mut BitReader<'_>,
        data: &[u8],
        h: &CodedImageHeaders,
        plane: &PlaneHeader,
        tile: usize,
        bands: usize,
        mb_count: u64,
        qp: &mut TileQp,
        scratch: &mut Scratch,
    ) -> Result<(), JxrError> {
        r.seek_byte(Self::band_start(h, tile, bands, 2)?)?;
        Self::tile_prologue(r, tile)?;
        Self::parse_tile_highpass_headers(r, plane, qp)?;
        let mut flex = if bands > 3 {
            let mut f = BitReader::new(data);
            f.seek_byte(Self::band_start(h, tile, bands, 3)?)?;
            let trim = Self::parse_flexbits_prologue(&mut f, self.g.trim_flexbits_flag)?;
            Some((f, trim))
        } else {
            None
        };
        self.walk_tile_highpass(r, flex.as_mut(), plane, tile, mb_count, qp, scratch)?;
        if let Some((f, _)) = flex.as_mut() {
            // 8.7.9 closes the FLEXBITS packet with the same padding every
            // tile packet ends with, so its end is comparable to the index
            // table the same way a spatial tile's is. Checking it separately
            // matters: the two readers are stepped in parallel, and if they
            // fell out of step the HIGHPASS packet would still end correctly
            // and only the picture would be wrong.
            f.align_to_byte()?;
            let end = f.byte_pos();
            self.record_packet_end(tile * bands + 3, end);
        }
        Ok(())
    }

    /// 8.7.10.1 and 8.7.10.2: the three-byte start code and the byte after
    /// it, which every tile packet begins with.
    fn tile_prologue(r: &mut BitReader<'_>, tile: usize) -> Result<(), JxrError> {
        if r.read_u32(24)? != 0x00_0001 {
            // 8.3.23 and 8.3.24 cap each axis at 4096, so the index fits the
            // `u32` the refusal carries.
            return Err(JxrError::BadTileStartCode(tile as u32));
        }
        let _arbitrary = r.read(8)?;
        Ok(())
    }

    /// 8.7.9's prologue: start code, arbitrary byte and `TRIM_FLEXBITS`.
    fn parse_flexbits_prologue(
        r: &mut BitReader<'_>,
        trim_flexbits_flag: bool,
    ) -> Result<u32, JxrError> {
        if r.read_u32(24)? != 0x00_0001 {
            return Err(JxrError::Truncated);
        }
        let _arbitrary = r.read(8)?;
        if trim_flexbits_flag {
            r.read_u32(4)
        } else {
            Ok(0)
        }
    }

    /// 8.7.2's `TILE_SPATIAL( )`.
    fn parse_tile_spatial(
        &mut self,
        r: &mut BitReader<'_>,
        plane: &PlaneHeader,
        tile: usize,
        mb_count: u64,
        scratch: &mut Scratch,
    ) -> Result<(), JxrError> {
        Self::tile_prologue(r, tile)?;
        let trim = if self.g.trim_flexbits_flag {
            r.read_u32(4)?
        } else {
            0
        };
        let mut qp = Self::parse_tile_header_dc(r, plane)?;
        if plane.bands_present != BandsPresent::DcOnly {
            Self::parse_tile_lowpass_headers(r, plane, &mut qp)?;
            if plane.bands_present != BandsPresent::NoHighpass {
                Self::parse_tile_highpass_headers(r, plane, &mut qp)?;
            }
        }
        let mut ctx = Context::new();
        for n in 0..mb_count {
            let pos = self.mb_pos(tile, n)?;
            self.read_qp_indices(r, plane, &qp, &pos)?;
            self.mb_dc(r, &mut ctx, plane, &pos, scratch)?;
            self.decode_dc(&qp, plane, &pos, scratch);
            if plane.bands_present != BandsPresent::DcOnly {
                self.mb_lp(r, &mut ctx, plane, &pos, scratch)?;
                self.decode_lp(&qp, plane, &pos, scratch);
                if plane.bands_present != BandsPresent::NoHighpass {
                    self.calc_hp_pred_mode(&pos);
                    self.mb_cbphp(r, &mut ctx, plane, &pos)?;
                    self.mb_hp(r, None, &mut ctx, plane, &pos, scratch, trim)?;
                    self.decode_hp(&qp, plane, &pos, scratch);
                }
            }
        }
        // 8.4.21's padding closes every tile packet.
        r.align_to_byte()?;
        Ok(())
    }

    /// 8.7.3's `TILE_DC( )`, plus 9.4.2 for each of its macroblocks.
    fn parse_tile_dc(
        &mut self,
        r: &mut BitReader<'_>,
        plane: &PlaneHeader,
        tile: usize,
        mb_count: u64,
        scratch: &mut Scratch,
    ) -> Result<TileQp, JxrError> {
        Self::tile_prologue(r, tile)?;
        let qp = Self::parse_tile_header_dc(r, plane)?;
        let mut ctx = Context::new();
        for n in 0..mb_count {
            let pos = self.mb_pos(tile, n)?;
            self.mb_dc(r, &mut ctx, plane, &pos, scratch)?;
            self.decode_dc(&qp, plane, &pos, scratch);
        }
        r.align_to_byte()?;
        Ok(qp)
    }

    /// 8.7.5's `TILE_LOWPASS( )` body, after its header.
    fn walk_tile_lowpass(
        &mut self,
        r: &mut BitReader<'_>,
        plane: &PlaneHeader,
        tile: usize,
        mb_count: u64,
        qp: &TileQp,
        scratch: &mut Scratch,
    ) -> Result<(), JxrError> {
        let mut ctx = Context::new();
        for n in 0..mb_count {
            let pos = self.mb_pos(tile, n)?;
            if qp.num_lp_qps > 1 && !qp.use_dc_qp {
                self.qp_index_lp[pos.index] = Self::decode_qp_index(r, qp.num_lp_qps)?;
            }
            self.mb_lp(r, &mut ctx, plane, &pos, scratch)?;
            self.decode_lp(qp, plane, &pos, scratch);
        }
        r.align_to_byte()?;
        Ok(())
    }

    /// 8.7.7's `TILE_HIGHPASS( )` body, with 8.7.9's flexbits alongside.
    #[allow(clippy::too_many_arguments)]
    fn walk_tile_highpass(
        &mut self,
        r: &mut BitReader<'_>,
        mut flex: Option<&mut (BitReader<'_>, u32)>,
        plane: &PlaneHeader,
        tile: usize,
        mb_count: u64,
        qp: &TileQp,
        scratch: &mut Scratch,
    ) -> Result<(), JxrError> {
        let mut ctx = Context::new();
        for n in 0..mb_count {
            let pos = self.mb_pos(tile, n)?;
            if qp.num_hp_qps > 1 && !qp.use_lp_qp {
                self.qp_index_hp[pos.index] = Self::decode_qp_index(r, qp.num_hp_qps)?;
            } else if qp.use_lp_qp {
                self.qp_index_hp[pos.index] = self.qp_index_lp[pos.index];
            }
            self.calc_hp_pred_mode(&pos);
            self.mb_cbphp(r, &mut ctx, plane, &pos)?;
            match flex.as_deref_mut() {
                Some((f, trim)) => {
                    let trim = *trim;
                    self.mb_hp(r, Some(f), &mut ctx, plane, &pos, scratch, trim)?;
                }
                None => self.mb_hp(r, None, &mut ctx, plane, &pos, scratch, 0)?,
            }
            self.decode_hp(qp, plane, &pos, scratch);
        }
        r.align_to_byte()?;
        Ok(())
    }

    /// 8.7.4's `TILE_HEADER_DC( )`, and 9.7.1's assignment.
    fn parse_tile_header_dc(
        r: &mut BitReader<'_>,
        plane: &PlaneHeader,
    ) -> Result<TileQp, JxrError> {
        let dc = if plane.dc_uniform {
            plane
                .dc_qp
                .clone()
                .ok_or(JxrError::ReservedValue("DC_QP"))?
        } else {
            QpSet::read(r, plane.num_components)?
        };
        // 9.7.2.1 and 9.7.3.1: when the plane header carried them, the tile
        // level does nothing and `NumLPQPs` and `NumHPQPs` are both 1.
        let lp = plane
            .lp_qp
            .clone()
            .unwrap_or_else(|| QpSets::one(dc.clone()));
        let hp = plane
            .hp_qp
            .clone()
            .unwrap_or_else(|| QpSets::one(dc.clone()));
        Ok(TileQp {
            dc,
            lp,
            hp,
            num_lp_qps: 1,
            num_hp_qps: 1,
            use_dc_qp: false,
            use_lp_qp: false,
        })
    }

    /// 8.7.6's `TILE_HEADER_LOWPASS( )`, and 9.7.2.2's derivation.
    fn parse_tile_lowpass_headers(
        r: &mut BitReader<'_>,
        plane: &PlaneHeader,
        qp: &mut TileQp,
    ) -> Result<(), JxrError> {
        if plane.lp_uniform {
            return Ok(());
        }
        qp.use_dc_qp = r.flag()?;
        if qp.use_dc_qp {
            qp.num_lp_qps = 1;
            qp.lp = QpSets::one(qp.dc.clone());
        } else {
            qp.num_lp_qps = r.read_u32(4)? + 1;
            qp.lp = QpSets::read(r, qp.num_lp_qps, plane.num_components)?;
        }
        Ok(())
    }

    /// 8.7.8's `TILE_HEADER_HIGHPASS( )`, and 9.7.3.2's derivation.
    fn parse_tile_highpass_headers(
        r: &mut BitReader<'_>,
        plane: &PlaneHeader,
        qp: &mut TileQp,
    ) -> Result<(), JxrError> {
        if plane.hp_uniform {
            return Ok(());
        }
        qp.use_lp_qp = r.flag()?;
        if qp.use_lp_qp {
            qp.num_hp_qps = qp.num_lp_qps;
            qp.hp = qp.lp.clone();
        } else {
            qp.num_hp_qps = r.read_u32(4)? + 1;
            qp.hp = QpSets::read(r, qp.num_hp_qps, plane.num_components)?;
        }
        Ok(())
    }

    /// The `LP_QP_INDEX` and `HP_QP_INDEX` reads at the head of a spatial
    /// macroblock (Table 39), including 8.7.10.9's inference.
    fn read_qp_indices(
        &mut self,
        r: &mut BitReader<'_>,
        plane: &PlaneHeader,
        qp: &TileQp,
        pos: &MbPos,
    ) -> Result<(), JxrError> {
        if plane.bands_present == BandsPresent::DcOnly {
            return Ok(());
        }
        if qp.num_lp_qps > 1 && !qp.use_dc_qp {
            self.qp_index_lp[pos.index] = Self::decode_qp_index(r, qp.num_lp_qps)?;
        }
        if plane.bands_present == BandsPresent::NoHighpass {
            return Ok(());
        }
        if qp.num_hp_qps > 1 && !qp.use_lp_qp {
            self.qp_index_hp[pos.index] = Self::decode_qp_index(r, qp.num_hp_qps)?;
        } else if qp.use_lp_qp {
            // 8.7.10.9: absent because the HP band shares the LP sets, so the
            // index is the LP one rather than zero.
            self.qp_index_hp[pos.index] = self.qp_index_lp[pos.index];
        }
        Ok(())
    }

    /// 8.7.10.10's `DECODE_QP_INDEX( )`.
    fn decode_qp_index(r: &mut BitReader<'_>, num_qp: u32) -> Result<u8, JxrError> {
        let bits = tables::BITS_QP_INDEX
            .get(num_qp as usize)
            .copied()
            .ok_or(JxrError::Truncated)?;
        if !r.flag()? {
            return Ok(0);
        }
        // `bits` is at most 4, so the value plus one is at most 16.
        Ok((r.read_u32(bits)? + 1) as u8)
    }

    /// Where macroblock `n` of tile `tile` sits, and the flags that follow.
    fn mb_pos(&self, tile: usize, n: u64) -> Result<MbPos, JxrError> {
        let tx = tile % self.g.num_tile_cols;
        let ty = tile / self.g.num_tile_cols;
        let get = |v: &[u32], i: usize| -> Result<usize, JxrError> {
            v.get(i).map(|&x| x as usize).ok_or(JxrError::BadTiling)
        };
        let left = get(&self.g.left_mb_of_tile, tx)?;
        let right = get(&self.g.left_mb_of_tile, tx + 1)?;
        let top = get(&self.g.top_mb_of_tile, ty)?;
        let width = right.checked_sub(left).ok_or(JxrError::BadTiling)?;
        if width == 0 {
            return Err(JxrError::BadTiling);
        }
        let n = usize::try_from(n).map_err(|_| JxrError::BadTiling)?;
        let x = left + n % width;
        let y = top + n / width;
        if x >= self.g.mb_width || y >= self.g.mb_height {
            return Err(JxrError::BadTiling);
        }
        let within = x - left;
        Ok(MbPos {
            x,
            y,
            index: y * self.g.mb_width + x,
            left_edge: x == left,
            top_edge: y == top,
            reset_context: x + 1 == right || within % 16 == 0,
            reset_totals: within % 16 == 0,
        })
    }

    /// Zeroes everything one tile contributed, for 8.7.10.1's NOTE 1.
    fn drop_tile(&mut self, tile: usize) {
        let tx = tile % self.g.num_tile_cols;
        let ty = tile / self.g.num_tile_cols;
        let bound = |v: &[u32], i: usize| v.get(i).map(|&x| x as usize);
        let (Some(left), Some(right)) = (
            bound(&self.g.left_mb_of_tile, tx),
            bound(&self.g.left_mb_of_tile, tx + 1),
        ) else {
            return;
        };
        let (Some(top), Some(bottom)) = (
            bound(&self.g.top_mb_of_tile, ty),
            bound(&self.g.top_mb_of_tile, ty + 1),
        ) else {
            return;
        };
        let nc = self.g.components;
        for y in top..bottom.min(self.g.mb_height) {
            for x in left..right.min(self.g.mb_width) {
                let mb = y * self.g.mb_width + x;
                for i in 0..nc {
                    let base = (mb * nc + i) * 16;
                    self.dclp[base..base + 16].fill(0);
                    let pbase = (mb * nc + i) * 7;
                    self.pred[pbase..pbase + 7].fill(0);
                    self.cbphp[mb * nc + i] = 0;
                    for row in 0..16 {
                        let py = 16 * y + row;
                        if py >= self.g.ext_height {
                            break;
                        }
                        let row_start = py * self.g.ext_width;
                        let start = row_start + 16 * x;
                        let end = (start + 16).min(row_start + self.g.ext_width);
                        if start < end {
                            self.plane[i][start..end].fill(0);
                        }
                    }
                }
            }
        }
    }
}

// --- 8.7.11 to 8.7.14: the DC band --------------------------------------

impl PlaneDecoder<'_> {
    /// 8.7.11's `MB_DC( )`.
    fn mb_dc(
        &mut self,
        r: &mut BitReader<'_>,
        ctx: &mut Context,
        plane: &PlaneHeader,
        pos: &MbPos,
        scratch: &mut Scratch,
    ) -> Result<(), JxrError> {
        const BAND: usize = 0;
        if pos.left_edge && pos.top_edge {
            ctx.vlc[slot::ABS_LEVEL_DC_LUM] = AdaptiveVlc::init1();
            ctx.vlc[slot::ABS_LEVEL_DC_CHR] = AdaptiveVlc::init1();
            ctx.model_dc = Model::init(0);
        }
        let mut lap_mean = [0i64; 2];
        if matches!(plane.internal_clr_fmt, InternalClrFmt::YOnly) {
            for n in 0..self.g.components {
                let abs_level = r.flag()?;
                let m = usize::from(n != 0);
                if abs_level {
                    lap_mean[m] += 1;
                }
                let bits = ctx.model_dc.bits[m];
                scratch.dc[n] = Self::decode_dc_value(r, ctx, bits, BAND, false, abs_level)?;
            }
        } else {
            // 8.7.14.2: one code jointly states which of Y, U and V carry a
            // variable-length coded part.
            let val = read_vlc(r, tables::VAL_DC_YUV)?;
            for (n, (mask, m, chroma)) in [(4u8, 0usize, false), (2, 1, true), (1, 1, true)]
                .into_iter()
                .enumerate()
            {
                let abs_level = (val & mask) != 0;
                if abs_level {
                    lap_mean[m] += 1;
                }
                let bits = ctx.model_dc.bits[m];
                let v = Self::decode_dc_value(r, ctx, bits, BAND, chroma, abs_level)?;
                if let Some(cell) = scratch.dc.get_mut(n) {
                    *cell = v;
                }
            }
        }
        ctx.model_dc
            .update(lap_mean, BAND, plane.internal_clr_fmt, self.g.components);
        if pos.reset_context {
            ctx.adapt_dc();
        }
        Ok(())
    }

    /// 8.7.12's `DECODE_DC( )`.
    fn decode_dc_value(
        r: &mut BitReader<'_>,
        ctx: &mut Context,
        model_bits: i32,
        band: usize,
        chroma: bool,
        abs_level: bool,
    ) -> Result<i32, JxrError> {
        let mut dc: i64 = 0;
        if abs_level {
            dc = Self::decode_abs_level(r, ctx, band, chroma, 0)? - 1;
        }
        if model_bits > 0 {
            // 8.12's `MBits` is capped at 15 by `UpdateModelMB( )`.
            let bits = model_bits.clamp(0, 15) as u32;
            let refine = r.read(bits)? as i64;
            dc = (dc << bits) | refine;
        }
        if dc != 0 && r.flag()? {
            dc = -dc;
        }
        narrow(dc)
    }

    /// 8.7.13's `DECODE_ABS_LEVEL( )`.
    fn decode_abs_level(
        r: &mut BitReader<'_>,
        ctx: &mut Context,
        band: usize,
        chroma: bool,
        context: u8,
    ) -> Result<i64, JxrError> {
        const REMAP: [i64; 6] = [2, 3, 4, 6, 10, 14];
        const FIXED_LEN: [u32; 6] = [0, 0, 1, 2, 2, 2];
        let at = match (band, chroma, context != 0) {
            (0, false, _) => slot::ABS_LEVEL_DC_LUM,
            (0, true, _) => slot::ABS_LEVEL_DC_CHR,
            (1, _, false) => slot::ABS_LEVEL_LP0,
            (1, _, true) => slot::ABS_LEVEL_LP1,
            (_, _, false) => slot::ABS_LEVEL_HP0,
            (_, _, true) => slot::ABS_LEVEL_HP1,
        };
        let ti = ctx.vlc[at].table_index.min(1);
        let index = read_vlc(r, tables::ABS_LEVEL_INDEX[ti])?;
        ctx.vlc[at].discrim1 = ctx.vlc[at].discrim1.saturating_add(
            tables::ABS_LEVEL_INDEX_DELTA[0]
                .get(usize::from(index))
                .copied()
                .unwrap_or(0),
        );
        if index < 6 {
            let i = usize::from(index);
            let fixed = FIXED_LEN[i];
            let mut level = REMAP[i];
            if fixed > 0 {
                level += r.read(fixed)? as i64;
            }
            Ok(level)
        } else {
            // 8.7.13's escape mode. `fixed` is at most 4 + 15 + 3 + 7 = 29,
            // so both the shift and the read stay inside a `u64`.
            let mut fixed = r.read_u32(4)? + 4;
            if fixed == 19 {
                fixed += r.read_u32(2)?;
                if fixed == 22 {
                    fixed += r.read_u32(3)?;
                }
            }
            let refine = r.read(fixed)? as i64;
            Ok(2 + (1i64 << fixed) + refine)
        }
    }
}

// --- 8.7.16: the LP band -------------------------------------------------

impl PlaneDecoder<'_> {
    /// 8.7.16.1's `MB_LP( )`.
    fn mb_lp(
        &mut self,
        r: &mut BitReader<'_>,
        ctx: &mut Context,
        plane: &PlaneHeader,
        pos: &MbPos,
        scratch: &mut Scratch,
    ) -> Result<(), JxrError> {
        const BAND: usize = 1;
        if pos.left_edge && pos.top_edge {
            ctx.count_zero_cbplp = 1;
            ctx.count_max_cbplp = 1;
            for i in [
                slot::FIRST_IND_LP_LUM,
                slot::IND_LP_LUM0,
                slot::IND_LP_LUM1,
                slot::FIRST_IND_LP_CHR,
                slot::IND_LP_CHR0,
                slot::IND_LP_CHR1,
            ] {
                ctx.vlc[i] = AdaptiveVlc::init2();
            }
            ctx.vlc[slot::ABS_LEVEL_LP0] = AdaptiveVlc::init1();
            ctx.vlc[slot::ABS_LEVEL_LP1] = AdaptiveVlc::init1();
            ctx.scan_lp = Scan::new(tables::SCAN_ORDER_0);
            ctx.model_lp = Model::init(1);
        }
        if pos.reset_totals {
            ctx.scan_lp.reset_totals();
        }
        let full_planes = self.g.components;
        let cbplp = if matches!(plane.internal_clr_fmt, InternalClrFmt::Yuv444) {
            // The cast is exact: `full_planes` is 3 in this branch.
            let max = (full_planes as i32) * 4 - 5;
            let v = if ctx.count_zero_cbplp <= 0 || ctx.count_max_cbplp < 0 {
                let raw = i32::from(read_vlc(r, tables::CBPLP_YUV1_444)?);
                if ctx.count_max_cbplp < ctx.count_zero_cbplp {
                    max - raw
                } else {
                    raw
                }
            } else {
                r.read_u32(full_planes as u32)? as i32
            };
            // 8.9.3's `UpdateCountCBPLP( )`.
            ctx.count_zero_cbplp = (ctx.count_zero_cbplp + 1 - 4 * i32::from(v == 0)).clamp(-8, 7);
            ctx.count_max_cbplp = (ctx.count_max_cbplp + 1 - 4 * i32::from(v == max)).clamp(-8, 7);
            v
        } else {
            let mut v = 0i32;
            for n in 0..full_planes {
                v |= i32::from(r.flag()?) << n;
            }
            v
        };
        scratch.lp.fill([0; 16]);
        let mut lap_mean = [0i64; 2];
        for n in 0..full_planes {
            let index = usize::from(n != 0);
            let mut non_zero = 0usize;
            if (cbplp >> n) & 1 != 0 {
                scratch.rl = [0; 32];
                non_zero = Self::decode_block(r, ctx, &mut scratch.rl, BAND, n != 0, 1)?;
                let mut i = 1usize;
                for k in 0..non_zero {
                    let run =
                        usize::try_from(scratch.rl[k * 2]).map_err(|_| JxrError::Truncated)?;
                    i = i.checked_add(run).ok_or(JxrError::Truncated)?;
                    let at = ctx
                        .scan_lp
                        .place(i)
                        .ok_or_else(|| boom("lp scan location"))?;
                    scratch.lp[n][at] = scratch.rl[k * 2 + 1];
                    i += 1;
                }
            }
            // The cast is exact: `non_zero` is at most 16.
            lap_mean[index] += non_zero as i64;
            let model_bits = ctx.model_lp.bits[index];
            if model_bits > 0 {
                for k in 1..16 {
                    let j = tables::TRANSPOSE_444[k];
                    scratch.lp[n][j] = Self::refine_lp(r, scratch.lp[n][j], model_bits)?;
                }
            }
        }
        ctx.model_lp
            .update(lap_mean, BAND, plane.internal_clr_fmt, self.g.components);
        if pos.reset_context {
            ctx.adapt_lp();
        }
        Ok(())
    }

    /// 8.7.16.2's `REFINE_LP( )`.
    fn refine_lp(r: &mut BitReader<'_>, coeff: i32, model_bits: i32) -> Result<i32, JxrError> {
        let bits = model_bits.clamp(0, 15) as u32;
        let refine = r.read(bits)? as i64;
        let c = i64::from(coeff);
        let out = if c > 0 {
            (c << bits) + refine
        } else if c < 0 {
            (c << bits) - refine
        } else if refine != 0 && r.flag()? {
            -refine
        } else {
            refine
        };
        narrow(out)
    }
}

// --- 8.7.17: the coded block pattern, highpass --------------------------

impl PlaneDecoder<'_> {
    /// 8.7.17.2's `MB_CBPHP( )`, followed by 8.7.17.5's prediction.
    fn mb_cbphp(
        &mut self,
        r: &mut BitReader<'_>,
        ctx: &mut Context,
        plane: &PlaneHeader,
        pos: &MbPos,
    ) -> Result<(), JxrError> {
        const FLC: [u32; 6] = [0, 2, 1, 2, 2, 0];
        const OFF: [u32; 6] = [0, 4, 2, 8, 12, 1];
        const OUT: [u32; 16] = [0, 15, 3, 12, 1, 2, 4, 8, 5, 6, 9, 10, 7, 11, 13, 14];
        if pos.left_edge && pos.top_edge {
            ctx.vlc[slot::NUM_CBPHP] = AdaptiveVlc::init1();
            ctx.vlc[slot::NUM_BLK_CBPHP] = AdaptiveVlc::init1();
        }
        let yuv444 = matches!(plane.internal_clr_fmt, InternalClrFmt::Yuv444);
        let mut diff = vec![0u32; self.g.components];
        // 8.7.17.2: one `NUM_CBPHP` for the whole macroblock in the chroma
        // formats, one per component otherwise.
        let outer = if yuv444 { 1 } else { self.g.components };
        for i in 0..outer {
            let ti = ctx.vlc[slot::NUM_CBPHP].table_index.min(1);
            let num_cbphp = read_vlc(r, tables::NUM_CBPHP[ti])?;
            ctx.vlc[slot::NUM_CBPHP].discrim1 = ctx.vlc[slot::NUM_CBPHP].discrim1.saturating_add(
                tables::NUM_CBPHP_DELTA[0]
                    .get(usize::from(num_cbphp))
                    .copied()
                    .unwrap_or(0),
            );
            let group = Self::refine_cbphp(r, u32::from(num_cbphp))?;
            for block in 0..4u32 {
                if group & (1 << block) == 0 {
                    continue;
                }
                let ti = ctx.vlc[slot::NUM_BLK_CBPHP].table_index.min(1);
                let (codes, delta): (&[&[tables::Code]; 2], &[i32]) = if yuv444 {
                    (
                        &tables::NUM_BLKCBPHP_CHROMA,
                        &tables::NUM_BLKCBPHP_DELTA_CHROMA[0],
                    )
                } else {
                    (
                        &tables::NUM_BLKCBPHP_YONLY,
                        &tables::NUM_BLKCBPHP_DELTA_YONLY[0],
                    )
                };
                let num_blk = read_vlc(r, codes[ti])?;
                ctx.vlc[slot::NUM_BLK_CBPHP].discrim1 = ctx.vlc[slot::NUM_BLK_CBPHP]
                    .discrim1
                    .saturating_add(delta.get(usize::from(num_blk)).copied().unwrap_or(0));
                let mut val = u32::from(num_blk) + 1;
                let mut blk = 0u32;
                if val >= 6 {
                    let chr = u32::from(read_vlc(r, tables::CHR_CBPHP)?);
                    blk = 0x10 * (chr + 1);
                    if val >= 9 {
                        val += u32::from(read_vlc(r, tables::CHR_CBPHP)?);
                    }
                    val -= 6;
                }
                let val = val as usize;
                if val >= FLC.len() {
                    // Only reachable from a codestream whose `NUM_BLKCBPHP`
                    // and `VAL_INC` disagree with 8.7.17.1's own arithmetic.
                    return Err(boom("cbphp val out of range"));
                }
                let mut code = OFF[val];
                if FLC[val] > 0 {
                    code += r.read_u32(FLC[val])?;
                }
                blk += OUT.get(code as usize).copied().ok_or(JxrError::Truncated)?;
                if yuv444 {
                    diff[0] |= (blk & 0x0F) << (block * 4);
                    for k in 0..2usize {
                        if (blk >> (k + 4)) & 1 == 0 {
                            continue;
                        }
                        let n = u32::from(read_vlc(r, tables::NUM_CH_BLK)?);
                        let chr = Self::refine_cbphp(r, n + 1)?;
                        if let Some(d) = diff.get_mut(k + 1) {
                            *d |= chr << (block * 4);
                        }
                    }
                } else if let Some(d) = diff.get_mut(i) {
                    *d |= blk << (block * 4);
                }
            }
        }
        self.pred_cbphp(ctx, pos, &diff);
        Ok(())
    }

    /// 8.7.17.3's `REFINE_CBPHP( )`.
    fn refine_cbphp(r: &mut BitReader<'_>, num: u32) -> Result<u32, JxrError> {
        Ok(match num {
            1 => 1 << r.read_u32(2)?,
            2 => u32::from(read_vlc(r, tables::REF_CBPHP1)?),
            3 => 0x0F ^ (1 << r.read_u32(2)?),
            4 => 0x0F,
            _ => 0,
        })
    }

    /// 8.7.17.5.1's `PredCBPHP( )`. Both internal formats here take
    /// 8.7.17.5.2's path for every component.
    fn pred_cbphp(&mut self, ctx: &mut Context, pos: &MbPos, diff: &[u32]) {
        if pos.left_edge && pos.top_edge {
            ctx.cbphp = CbpHpModel::init();
        }
        let nc = self.g.components;
        for i in 0..nc {
            let c1 = usize::from(i > 0);
            let mut v = diff.get(i).copied().unwrap_or(0);
            if ctx.cbphp.state[c1] == 0 {
                if pos.left_edge {
                    if pos.top_edge {
                        v ^= 1;
                    } else {
                        let up = self.cbphp[(pos.index - self.g.mb_width) * nc + i];
                        v ^= (up >> 10) & 1;
                    }
                } else {
                    let left = self.cbphp[(pos.index - 1) * nc + i];
                    v ^= (left >> 5) & 1;
                }
                v ^= 0x02 & (v << 1);
                v ^= 0x10 & (v << 3);
                v ^= 0x20 & (v << 1);
                v ^= (v & 0x33) << 2;
                v ^= (v & 0x00CC) << 6;
                v ^= (v & 0x3300) << 2;
            } else if ctx.cbphp.state[c1] == 2 {
                v ^= 0x0000_FFFF;
            }
            v &= 0xFFFF;
            // The cast is exact: a 16-bit population count is at most 16.
            ctx.cbphp.update(c1, v.count_ones() as i32);
            self.cbphp[pos.index * nc + i] = v;
        }
    }
}

// --- 8.7.18 and 8.7.19: the HP band and its flexbits --------------------

impl PlaneDecoder<'_> {
    /// 8.7.18.2's `MB_HP( )` and 8.7.18.3's `MB_HP_FLEX( )`, which differ
    /// only in whether the flexbits arrive inline.
    ///
    /// `flex` is `Some` in frequency mode, where 8.7.9's packet is a byte
    /// range of its own; `None` in spatial mode, where 8.7.18.3 reads them
    /// from the same reader immediately after each block.
    #[allow(clippy::too_many_arguments)]
    fn mb_hp(
        &mut self,
        r: &mut BitReader<'_>,
        flex: Option<&mut BitReader<'_>>,
        ctx: &mut Context,
        plane: &PlaneHeader,
        pos: &MbPos,
        scratch: &mut Scratch,
        trim: u32,
    ) -> Result<(), JxrError> {
        const BAND: usize = 2;
        if pos.left_edge && pos.top_edge {
            for i in [
                slot::FIRST_IND_HP_LUM,
                slot::IND_HP_LUM0,
                slot::IND_HP_LUM1,
                slot::FIRST_IND_HP_CHR,
                slot::IND_HP_CHR0,
                slot::IND_HP_CHR1,
            ] {
                ctx.vlc[i] = AdaptiveVlc::init2();
            }
            ctx.vlc[slot::ABS_LEVEL_HP0] = AdaptiveVlc::init1();
            ctx.vlc[slot::ABS_LEVEL_HP1] = AdaptiveVlc::init1();
            ctx.scan_hp_hor = Scan::new(tables::SCAN_ORDER_0);
            ctx.scan_hp_ver = Scan::new(tables::SCAN_ORDER_1);
            ctx.model_hp = Model::init(2);
        }
        if pos.reset_totals {
            ctx.scan_hp_hor.reset_totals();
            ctx.scan_hp_ver.reset_totals();
        }
        let all_bands = plane.bands_present == BandsPresent::All;
        let inline_flex = flex.is_none() && all_bands;
        let vertical = self.mb_hp_mode == 1;
        let nc = self.g.components;
        let mut lap_mean = [0i64; 2];
        for i in 0..nc {
            let chroma = i > 0;
            let index = usize::from(chroma);
            let model_bits = ctx.model_hp.bits[index];
            let mut cbphp = self.cbphp[pos.index * nc + i];
            scratch.hp_vlc[i] = [0; 256];
            scratch.hp_flex[i] = [0; 256];
            for block in 0..16usize {
                let mapped = tables::HIER_SCAN_ORDER[block];
                let mut non_zero = 0usize;
                if cbphp & 1 != 0 {
                    scratch.rl = [0; 32];
                    non_zero = Self::decode_block(r, ctx, &mut scratch.rl, BAND, chroma, 1)?;
                    let mut k = 1usize;
                    for kk in 0..non_zero {
                        let run =
                            usize::try_from(scratch.rl[kk * 2]).map_err(|_| JxrError::Truncated)?;
                        k = k.checked_add(run).ok_or(JxrError::Truncated)?;
                        let scan = if vertical {
                            &mut ctx.scan_hp_ver
                        } else {
                            &mut ctx.scan_hp_hor
                        };
                        let at = scan.place(k).ok_or_else(|| boom("hp scan location"))?;
                        scratch.hp_vlc[i][16 * mapped + at] = scratch.rl[kk * 2 + 1];
                        k += 1;
                    }
                }
                if inline_flex {
                    Self::block_flexbits(r, scratch, i, mapped, model_bits, trim)?;
                }
                lap_mean[index] += non_zero as i64;
                cbphp >>= 1;
            }
        }
        // 8.7.19.1's separate FLEXBITS packet walks components and blocks in
        // exactly the order the loop above just did.
        if let Some(f) = flex {
            if all_bands {
                for i in 0..nc {
                    let model_bits = ctx.model_hp.bits[usize::from(i > 0)];
                    for block in 0..16usize {
                        let mapped = tables::HIER_SCAN_ORDER[block];
                        Self::block_flexbits(f, scratch, i, mapped, model_bits, trim)?;
                    }
                }
            }
        }
        self.model_bits_hp[pos.index] = [ctx.model_hp.bits[0], ctx.model_hp.bits[1]];
        ctx.model_hp
            .update(lap_mean, BAND, plane.internal_clr_fmt, nc);
        if pos.reset_context {
            ctx.adapt_hp();
        }
        Ok(())
    }

    /// 8.7.19.2's `BLOCK_FLEXBITS( )`.
    fn block_flexbits(
        r: &mut BitReader<'_>,
        scratch: &mut Scratch,
        component: usize,
        block: usize,
        model_bits: i32,
        trim: u32,
    ) -> Result<(), JxrError> {
        let left = model_bits - i32::try_from(trim).unwrap_or(i32::MAX);
        if left <= 0 {
            return Ok(());
        }
        let left = left.min(15) as u32;
        let trim = trim.min(15);
        for n in 1..16 {
            let j = tables::TRANSPOSE_444[n];
            let vlc = scratch.hp_vlc[component][16 * block + j];
            let flex = Self::decode_flex(r, vlc, left)?;
            // `flex` is at most 2^15 in magnitude and `trim` at most 15, so
            // the shift stays inside `i32`.
            scratch.hp_flex[component][16 * block + j] = flex.wrapping_shl(trim);
        }
        Ok(())
    }

    /// 8.7.19.3's `DECODE_FLEX( )`.
    fn decode_flex(r: &mut BitReader<'_>, vlc: i32, bits: u32) -> Result<i32, JxrError> {
        // `bits` is at most 15, so the read fits an `i32` unchecked.
        let raw = r.read(bits)? as i32;
        if vlc != 0 {
            // The VLC part already carried the sign; the flexbits only
            // refine the magnitude.
            return Ok(if vlc < 0 { -raw } else { raw });
        }
        // A coefficient the VLC layer left at zero has no sign yet, so one is
        // coded — but only when the refinement made it non-zero.
        let negative = raw != 0 && r.flag()?;
        Ok(if negative { -raw } else { raw })
    }

    /// 8.7.18.5's `DECODE_BLOCK( )`, shared by the LP and HP bands.
    fn decode_block(
        r: &mut BitReader<'_>,
        ctx: &mut Context,
        coeff: &mut [i32; 32],
        band: usize,
        chroma: bool,
        start_location: i32,
    ) -> Result<usize, JxrError> {
        let mut num_nz = 1usize;
        let first = Self::decode_first_index(r, ctx, band, chroma)?;
        let sign = r.flag()?;
        let sr = first & 1;
        let mut srn = first >> 2;
        let mut context = sr & srn;
        let mut location = start_location;
        let level = if first & 2 != 0 {
            Self::decode_abs_level(r, ctx, band, chroma, context)?
        } else {
            1
        };
        coeff[1] = narrow(if sign { -level } else { level })?;
        coeff[0] = 0;
        if sr == 0 {
            coeff[0] = Self::decode_run(r, 15 - location)?;
        }
        location += coeff[0] + 1;
        while srn != 0 {
            if num_nz >= 16 {
                // Sixteen run/level pairs is every position a block has; a
                // seventeenth means the reader has lost the bitstream.
                return Err(boom("decode_block: seventeen pairs"));
            }
            let sr = srn & 1;
            coeff[num_nz * 2] = 0;
            if sr == 0 {
                coeff[num_nz * 2] = Self::decode_run(r, 15 - location)?;
            }
            location += coeff[num_nz * 2] + 1;
            let index = Self::decode_index(r, ctx, location, band, chroma, context)?;
            srn = index >> 1;
            context &= srn;
            let sign = r.flag()?;
            let level = if index & 1 != 0 {
                Self::decode_abs_level(r, ctx, band, chroma, context)?
            } else {
                1
            };
            coeff[num_nz * 2 + 1] = narrow(if sign { -level } else { level })?;
            num_nz += 1;
        }
        Ok(num_nz)
    }

    /// 8.7.18.6's `DECODE_RUN( )`.
    fn decode_run(r: &mut BitReader<'_>, max_run: i32) -> Result<i32, JxrError> {
        const REMAP: [i32; 15] = [1, 2, 3, 5, 7, 1, 2, 3, 5, 7, 1, 2, 3, 4, 5];
        const RUN_BIN: [i32; 15] = [-1, -1, -1, -1, 2, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0];
        const FIXED: [u32; 15] = [0, 0, 1, 1, 3, 0, 0, 1, 1, 2, 0, 0, 0, 0, 1];
        if max_run < 5 {
            return Ok(match max_run {
                2 => i32::from(read_vlc(r, tables::RUN_VALUE_2)?),
                3 => i32::from(read_vlc(r, tables::RUN_VALUE_3)?),
                4 => i32::from(read_vlc(r, tables::RUN_VALUE_4)?),
                // 8.7.18.6: `iRun` is 1 when `iMaxRun` is 1. The same
                // fallback covers the zero and negative cases a damaged
                // stream produces, which the caller then rejects by location.
                _ => 1,
            });
        }
        let bin = RUN_BIN
            .get(max_run as usize)
            .copied()
            .ok_or(JxrError::Truncated)?;
        let index = i32::from(read_vlc(r, tables::RUN_INDEX)?) + 5 * bin;
        let index = usize::try_from(index).map_err(|_| JxrError::Truncated)?;
        let fixed = FIXED.get(index).copied().ok_or(JxrError::Truncated)?;
        let mut run = REMAP[index];
        if fixed > 0 {
            // `fixed` is at most 3, so the value fits unchecked.
            run += r.read_u32(fixed)? as i32;
        }
        Ok(run)
    }

    /// 8.7.18.7's `DECODE_INDEX( )`.
    fn decode_index(
        r: &mut BitReader<'_>,
        ctx: &mut Context,
        location: i32,
        band: usize,
        chroma: bool,
        context: u8,
    ) -> Result<u8, JxrError> {
        if location > 15 {
            // 8.7.18.9.6's `INDEX_C_FLAG`.
            return Ok(u8::from(r.flag()?));
        }
        if location == 15 {
            return read_vlc(r, tables::INDEX_B);
        }
        let at = match (band, chroma, context != 0) {
            (1, false, false) => slot::IND_LP_LUM0,
            (1, false, true) => slot::IND_LP_LUM1,
            (1, true, false) => slot::IND_LP_CHR0,
            (1, true, true) => slot::IND_LP_CHR1,
            (_, false, false) => slot::IND_HP_LUM0,
            (_, false, true) => slot::IND_HP_LUM1,
            (_, true, false) => slot::IND_HP_CHR0,
            (_, true, true) => slot::IND_HP_CHR1,
        };
        let ti = ctx.vlc[at].table_index.min(3);
        let value = read_vlc(r, tables::INDEX_A[ti])?;
        let d1 = ctx.vlc[at].delta_table_index.min(2);
        let d2 = ctx.vlc[at].delta2_table_index.min(2);
        let n = usize::from(value);
        ctx.vlc[at].discrim1 = ctx.vlc[at]
            .discrim1
            .saturating_add(tables::INDEX1_DELTA[d1].get(n).copied().unwrap_or(0));
        ctx.vlc[at].discrim2 = ctx.vlc[at]
            .discrim2
            .saturating_add(tables::INDEX1_DELTA[d2].get(n).copied().unwrap_or(0));
        Ok(value)
    }

    /// 8.7.18.8's `DECODE_FIRST_INDEX( )`.
    fn decode_first_index(
        r: &mut BitReader<'_>,
        ctx: &mut Context,
        band: usize,
        chroma: bool,
    ) -> Result<u8, JxrError> {
        let at = match (band, chroma) {
            (1, false) => slot::FIRST_IND_LP_LUM,
            (1, true) => slot::FIRST_IND_LP_CHR,
            (_, false) => slot::FIRST_IND_HP_LUM,
            (_, true) => slot::FIRST_IND_HP_CHR,
        };
        let ti = ctx.vlc[at].table_index.min(4);
        let value = read_vlc(r, tables::FIRST_INDEX[ti])?;
        let d1 = ctx.vlc[at].delta_table_index.min(3);
        let d2 = ctx.vlc[at].delta2_table_index.min(3);
        let n = usize::from(value);
        ctx.vlc[at].discrim1 = ctx.vlc[at]
            .discrim1
            .saturating_add(tables::FIRST_INDEX_DELTA[d1].get(n).copied().unwrap_or(0));
        ctx.vlc[at].discrim2 = ctx.vlc[at]
            .discrim2
            .saturating_add(tables::FIRST_INDEX_DELTA[d2].get(n).copied().unwrap_or(0));
        Ok(value)
    }
}

// --- clause 9.4 to 9.8: remapping, prediction and dequantization --------

impl PlaneDecoder<'_> {
    /// 9.4.2's `DCTransformCoefficientDecoding( )`.
    fn decode_dc(&mut self, qp: &TileQp, plane: &PlaneHeader, pos: &MbPos, scratch: &Scratch) {
        let nc = self.g.components;
        // 9.5.1's remap.
        for i in 0..nc {
            self.dclp_set(pos.index, i, 0, scratch.dc[i]);
        }
        self.mb_dc_mode = self.calc_dc_pred_mode(plane, pos);
        // 9.6.1.4's `DCCoefficientPrediction( )`.
        if self.mb_dc_mode != 3 {
            for i in 0..nc {
                let left = if pos.x > 0 {
                    self.pred_at(pos.index - 1, i, 0)
                } else {
                    0
                };
                let top = if pos.y > 0 {
                    self.pred_at(pos.index - self.g.mb_width, i, 0)
                } else {
                    0
                };
                let base = self.dclp_at(pos.index, i, 0);
                let v = match self.mb_dc_mode {
                    0 => base.wrapping_add(left),
                    1 => base.wrapping_add(top),
                    _ => base.wrapping_add(top.wrapping_add(left) >> 1),
                };
                self.dclp_set(pos.index, i, 0, v);
            }
        }
        // 9.6.1.5's update, before dequantization — see the module docs.
        for i in 0..nc {
            let v = self.dclp_at(pos.index, i, 0);
            self.pred_set(pos.index, i, 0, v);
        }
        // 9.8.1's dequantization.
        for i in 0..nc {
            let factor = quant_map(qp.dc.get(i), u32::from(i == 0), self.g.scaled);
            let v = self.dclp_at(pos.index, i, 0).wrapping_mul(factor);
            self.dclp_set(pos.index, i, 0, v);
        }
    }

    /// 9.6.1.3's `CalcDCPredMode( )`.
    fn calc_dc_pred_mode(&self, plane: &PlaneHeader, pos: &MbPos) -> u8 {
        if pos.left_edge && pos.top_edge {
            return 3;
        }
        if pos.left_edge {
            return 1;
        }
        if pos.top_edge {
            return 0;
        }
        let left_mb = pos.index - 1;
        let top_mb = pos.index - self.g.mb_width;
        let top_left_mb = top_mb - 1;
        let abs = |a: i32, b: i32| i64::from(a.wrapping_sub(b).unsigned_abs());
        let mut hor = abs(self.pred_at(top_left_mb, 0, 0), self.pred_at(left_mb, 0, 0));
        let mut ver = abs(self.pred_at(top_left_mb, 0, 0), self.pred_at(top_mb, 0, 0));
        if !matches!(plane.internal_clr_fmt, InternalClrFmt::YOnly) {
            // 4:4:4 is the only multi-component internal format reaching here,
            // so 9.6.1.3's `iScale` is its default of 2.
            hor *= 2;
            ver *= 2;
            for i in 1..nc_min3(self.g.components) {
                let tl = self.pred_at(top_left_mb, i, 0);
                hor += abs(tl, self.pred_at(left_mb, i, 0));
                ver += abs(tl, self.pred_at(top_mb, i, 0));
            }
        }
        const OR_WT: i64 = 4;
        if hor.saturating_mul(OR_WT) < ver {
            1
        } else if ver.saturating_mul(OR_WT) < hor {
            0
        } else {
            2
        }
    }

    /// 9.4.3's `LPTransformCoefficientDecoding( )`.
    fn decode_lp(&mut self, qp: &TileQp, plane: &PlaneHeader, pos: &MbPos, scratch: &Scratch) {
        let nc = self.g.components;
        let dc_only = plane.bands_present == BandsPresent::DcOnly;
        // 9.5.2's remap.
        for i in 0..nc {
            for j in 1..16 {
                let v = if dc_only { 0 } else { scratch.lp[i][j] };
                self.dclp_set(pos.index, i, j, v);
            }
        }
        // 9.6.2.3's `CalcLPPredMode( )`: prediction is refused across a
        // quantizer change, which is what keeps it from crossing a boundary
        // the numbers no longer mean the same thing across.
        let lp_mode = if self.mb_dc_mode == 0
            && self.qp_index_lp[pos.index] == self.qp_index_lp[pos.index - 1]
        {
            0
        } else if self.mb_dc_mode == 1
            && self.qp_index_lp[pos.index] == self.qp_index_lp[pos.index - self.g.mb_width]
        {
            1
        } else {
            2
        };
        // 9.6.2.4's `LPCoefficientPrediction( )`.
        if lp_mode == 0 {
            let src = pos.index - 1;
            for i in 0..nc {
                for (dst, from) in [(4usize, 4usize), (8, 5), (12, 6)] {
                    let v = self
                        .dclp_at(pos.index, i, dst)
                        .wrapping_add(self.pred_at(src, i, from));
                    self.dclp_set(pos.index, i, dst, v);
                }
            }
        } else if lp_mode == 1 {
            let src = pos.index - self.g.mb_width;
            for i in 0..nc {
                for j in 1..4usize {
                    let v = self
                        .dclp_at(pos.index, i, j)
                        .wrapping_add(self.pred_at(src, i, j));
                    self.dclp_set(pos.index, i, j, v);
                }
            }
        }
        // 9.6.2.5's update, again before dequantization. The mapping is not
        // the identity: slots 5 and 6 hold coefficients 8 and 12.
        for i in 0..nc {
            for (at, j) in [(1usize, 1usize), (2, 2), (3, 3), (4, 4), (5, 8), (6, 12)] {
                let v = self.dclp_at(pos.index, i, j);
                self.pred_set(pos.index, i, at, v);
            }
        }
        // 9.8.2's dequantization.
        let k = usize::from(self.qp_index_lp[pos.index]);
        for i in 0..nc {
            let set = qp.lp.select(k).map_or(0, |s| s.get(i));
            let factor = quant_map(set, u32::from(i == 0), self.g.scaled);
            for j in 1..16 {
                let v = self.dclp_at(pos.index, i, j).wrapping_mul(factor);
                self.dclp_set(pos.index, i, j, v);
            }
        }
    }

    /// 9.6.3.2's `CalcHPPredMode( )`. Runs after 9.4.3 and before the HP
    /// coefficients are scanned, which is the ordering 8.11.7 demands.
    fn calc_hp_pred_mode(&mut self, pos: &MbPos) {
        let mb = pos.index;
        let mut hor = 0i64;
        let mut ver = 0i64;
        for j in [1usize, 2, 3] {
            hor += i64::from(self.dclp_at(mb, 0, j).unsigned_abs());
        }
        for j in [4usize, 8, 12] {
            ver += i64::from(self.dclp_at(mb, 0, j).unsigned_abs());
        }
        for i in 1..nc_min3(self.g.components) {
            hor += i64::from(self.dclp_at(mb, i, 1).unsigned_abs());
            ver += i64::from(self.dclp_at(mb, i, 4).unsigned_abs());
        }
        const OR_WT: i64 = 4;
        self.mb_hp_mode = if hor.saturating_mul(OR_WT) < ver {
            0
        } else if ver.saturating_mul(OR_WT) < hor {
            1
        } else {
            2
        };
    }

    /// 9.4.4's remaining three steps — 9.5.3's remap, 9.8.3's dequantization
    /// and 9.6.3.3's prediction — then the HP half of 9.9.4's combination.
    fn decode_hp(&mut self, qp: &TileQp, plane: &PlaneHeader, pos: &MbPos, scratch: &Scratch) {
        let nc = self.g.components;
        let bands = plane.bands_present;
        let k = usize::from(self.qp_index_hp[pos.index]);
        let model_bits = self.model_bits_hp[pos.index];
        let mut buffer = [0i32; 256];
        for i in 0..nc {
            let shift = model_bits[usize::from(i > 0)].clamp(0, 15) as u32;
            // 9.5.4's `HPBlockCoefficientRemap( )`.
            for block in 0..16usize {
                for j in 1..16usize {
                    let at = 16 * block + j;
                    let mut v = if matches!(bands, BandsPresent::All | BandsPresent::NoFlexbits) {
                        scratch.hp_vlc[i][at].wrapping_shl(shift)
                    } else {
                        0
                    };
                    if bands == BandsPresent::All {
                        v = v.wrapping_add(scratch.hp_flex[i][at]);
                    }
                    buffer[at] = v;
                }
            }
            // 9.8.3's dequantization. `iScaledShift` is 1 for **every**
            // component in the HP band, unlike the DC and LP bands where it
            // is 1 for luma and 0 for chroma.
            let set = qp.hp.select(k).map_or(0, |s| s.get(i));
            let factor = quant_map(set, 1, self.g.scaled);
            for block in 0..16usize {
                for j in 1..16usize {
                    let at = 16 * block + j;
                    buffer[at] = buffer[at].wrapping_mul(factor);
                }
            }
            // 9.6.3.3's `HPCoefficientPrediction( )`, after dequantization.
            if self.mb_hp_mode == 0 {
                for block in [1usize, 2, 3, 5, 6, 7, 9, 10, 11, 13, 14, 15] {
                    for j in [4usize, 8, 12] {
                        buffer[16 * block + j] =
                            buffer[16 * block + j].wrapping_add(buffer[16 * (block - 1) + j]);
                    }
                }
            } else if self.mb_hp_mode == 1 {
                for block in 4..16usize {
                    for j in [1usize, 2, 3] {
                        buffer[16 * block + j] =
                            buffer[16 * block + j].wrapping_add(buffer[16 * (block - 4) + j]);
                    }
                }
            }
            // The HP half of 9.9.4's combination, done here so that no
            // whole-image `MBBuffer` has to exist — see the struct docs.
            for (at, &value) in buffer.iter().enumerate() {
                let j = at % 16;
                if j == 0 {
                    continue;
                }
                let block = at / 16;
                let x = 16 * pos.x + 4 * (block % 4) + (j % 4);
                let y = 16 * pos.y + 4 * (block / 4) + (j / 4);
                if x < self.g.ext_width && y < self.g.ext_height {
                    self.plane[i][y * self.g.ext_width + x] = value;
                }
            }
        }
    }

    /// 9.9.1's `SampleReconstruction( )`.
    ///
    /// The HP half of 9.9.4's combination already happened at parse time —
    /// see [`Self::decode_hp`] — so what remains is the first level's
    /// transform over `MbDCLP`, the DC and LP half of the combination, and
    /// the second level's transform over the sample plane.
    ///
    /// 8.3.10's `OVERLAP_MODE` selects how much of 9.9.3 and 9.9.6 runs: 0
    /// neither, 1 the second level only, 2 both. The clause puts the
    /// first-level filter *between* the two transforms and the second-level
    /// filter after both, and that ordering is load-bearing — the overlap
    /// filter is what makes the transform lapped, and applying it on the
    /// wrong side of a transform is a picture with seams rather than one
    /// without.
    /// [`Self::reconstruct`] with 9.9.6's second-level filter optionally
    /// suppressed, so that `overlap::tests` can run its counted injection
    /// against a real codestream rather than a synthetic one.
    #[cfg(test)]
    pub(crate) fn reconstruct_for_test(&mut self, disable_second_level: bool) {
        self.reconstruct_inner(disable_second_level);
    }

    fn reconstruct(&mut self) {
        self.reconstruct_inner(false);
    }

    fn reconstruct_inner(&mut self, disable_second_level: bool) {
        let components = self.g.components;
        let scaled = self.g.scaled;
        let mb_count = self.g.mb_width * self.g.mb_height;
        let (width, height) = (self.g.ext_width, self.g.ext_height);
        let geometry = self.geometry();
        super::transform::first_level(&mut self.dclp, components, scaled, mb_count);
        if self.g.overlap_mode == 2 {
            super::overlap::first_level(&mut self.dclp, &geometry);
        }
        self.combine_dclp();
        for plane in &mut self.plane {
            super::transform::second_level(plane, width, height);
        }
        if self.g.overlap_mode != 0 && !disable_second_level {
            for plane in &mut self.plane {
                super::overlap::second_level(plane, &geometry);
            }
        }
    }

    /// The tile and macroblock layout the overlap filters index by.
    fn geometry(&self) -> super::overlap::Geometry {
        super::overlap::Geometry {
            components: self.g.components,
            mb_width: self.g.mb_width,
            mb_height: self.g.mb_height,
            ext_width: self.g.ext_width,
            ext_height: self.g.ext_height,
            hard_tiling: self.g.hard_tiling,
            left_mb_of_tile: self.g.left_mb_of_tile.clone(),
            top_mb_of_tile: self.g.top_mb_of_tile.clone(),
            num_tile_cols: self.g.num_tile_cols,
            num_tile_rows: self.g.num_tile_rows,
        }
    }

    /// The DC and LP half of 9.9.4's `SecondLevelCoefficientCombination( )`.
    fn combine_dclp(&mut self) {
        for mby in 0..self.g.mb_height {
            for mbx in 0..self.g.mb_width {
                let mb = mby * self.g.mb_width + mbx;
                for i in 0..self.g.components {
                    for j in 0..16 {
                        let x = 16 * mbx + 4 * (j % 4);
                        let y = 16 * mby + 4 * (j / 4);
                        let v = self.dclp_at(mb, i, j);
                        self.plane[i][y * self.g.ext_width + x] = v;
                    }
                }
            }
        }
    }
}

/// The chroma loops of 9.6.1.3 and 9.6.3.2 run over components 1 and 2 only,
/// whatever `NumComponents` is.
const fn nc_min3(components: usize) -> usize {
    if components > 3 {
        3
    } else {
        components
    }
}

#[cfg(test)]
#[path = "tests/coefficients.rs"]
mod tests;
