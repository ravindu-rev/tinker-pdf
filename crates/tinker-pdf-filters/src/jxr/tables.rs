//! ITU-T T.832's constant tables: the VLC code tables of clause 8.7, the
//! deltaDisc tables of 8.8.2, the scan orders of 8.11.1 and the model weights
//! of 8.12.2.
//!
//! **Every table here is transcribed from the Recommendation's own tables**,
//! by number, and each is named with the number it came from so a reviewer
//! can check one against the other without reading the decoder. Nothing is
//! derived from another implementation's behaviour.
//!
//! # Why the codes are `(length, bits)` pairs and not a tree
//!
//! The largest table here has twelve entries and the longest code is eight
//! bits, so a decoder that accumulates bits and compares against the whole
//! table costs at most eight comparisons of a `u8` and a `u16` per symbol.
//! A tree would be faster and would need a builder, a validator and a test
//! that the builder agrees with the table — three things that can be wrong.
//! The linear form *is* the table, and
//! [`super::coefficients::tests::every_code_table_is_prefix_free`] checks
//! the property that makes it decodable rather than trusting the transcription.

#![deny(clippy::float_arithmetic)]

/// One row of a VLC code table: the code's length in bits, the code itself
/// (MSB-aligned within those bits), and the value it decodes to.
pub(crate) type Code = (u8, u16, u8);

// --- clause 8.7 code tables ---------------------------------------------

/// Table 51: `VAL_DC_YUV`.
pub(crate) const VAL_DC_YUV: &[Code] = &[
    (2, 0b10, 0),
    (3, 0b001, 1),
    (5, 0b00001, 2),
    (4, 0b0001, 3),
    (2, 0b11, 4),
    (3, 0b010, 5),
    (5, 0b00000, 6),
    (3, 0b011, 7),
];

/// Table 52: `ABS_LEVEL_INDEX`, code table 0.
///
/// **The last two rows are `00000` for 5 and `00001` for 6, in that order.**
/// Reading them the other way round is the one transcription slip this file
/// actually made, and it is called out here because the resulting table is
/// still prefix-free — it is only *incomplete*, by exactly one leaf, which
/// is why
/// [`super::coefficients::tests::every_code_table_is_a_complete_prefix_code`]
/// exists and why its exception list is empty.
pub(crate) const ABS_LEVEL_INDEX_0: &[Code] = &[
    (2, 0b01, 0),
    (2, 0b10, 1),
    (2, 0b11, 2),
    (3, 0b001, 3),
    (4, 0b0001, 4),
    (5, 0b00000, 5),
    (5, 0b00001, 6),
];

/// Table 52: `ABS_LEVEL_INDEX`, code table 1.
pub(crate) const ABS_LEVEL_INDEX_1: &[Code] = &[
    (1, 0b1, 0),
    (2, 0b01, 1),
    (3, 0b001, 2),
    (4, 0b0001, 3),
    (5, 0b00001, 4),
    (6, 0b000000, 5),
    (6, 0b000001, 6),
];

/// Table 52, both rows, indexed by `TableIndex`.
pub(crate) const ABS_LEVEL_INDEX: [&[Code]; 2] = [ABS_LEVEL_INDEX_0, ABS_LEVEL_INDEX_1];

/// Table 55: `CBPLP_YUV1` when `INTERNAL_CLR_FMT` is YUV444.
pub(crate) const CBPLP_YUV1_444: &[Code] = &[
    (1, 0b0, 0),
    (3, 0b100, 1),
    (4, 0b1010, 2),
    (4, 0b1011, 3),
    (4, 0b1100, 4),
    (4, 0b1101, 5),
    (4, 0b1110, 6),
    (4, 0b1111, 7),
];

/// Table 59: `NUM_CBPHP`, both code tables.
pub(crate) const NUM_CBPHP: [&[Code]; 2] = [
    &[
        (1, 0b1, 0),
        (2, 0b01, 1),
        (3, 0b001, 2),
        (4, 0b0000, 3),
        (4, 0b0001, 4),
    ],
    &[
        (1, 0b1, 0),
        (3, 0b000, 1),
        (3, 0b001, 2),
        (3, 0b010, 3),
        (3, 0b011, 4),
    ],
];

/// Table 60: `NUM_BLKCBPHP` when `INTERNAL_CLR_FMT` is YONLY, YUVK or
/// NCOMPONENT. Identical in content to Table 59 and written out separately
/// because the Recommendation writes it out separately — a later edition that
/// changed one and not the other would break a shared constant silently.
pub(crate) const NUM_BLKCBPHP_YONLY: [&[Code]; 2] = [
    &[
        (1, 0b1, 0),
        (2, 0b01, 1),
        (3, 0b001, 2),
        (4, 0b0000, 3),
        (4, 0b0001, 4),
    ],
    &[
        (1, 0b1, 0),
        (3, 0b000, 1),
        (3, 0b001, 2),
        (3, 0b010, 3),
        (3, 0b011, 4),
    ],
];

/// Table 61: `NUM_BLKCBPHP` for every other `INTERNAL_CLR_FMT`.
pub(crate) const NUM_BLKCBPHP_CHROMA: [&[Code]; 2] = [
    &[
        (3, 0b010, 0),
        (5, 0b00000, 1),
        (4, 0b0010, 2),
        (5, 0b00001, 3),
        (5, 0b00010, 4),
        (1, 0b1, 5),
        (3, 0b011, 6),
        (5, 0b00011, 7),
        (4, 0b0011, 8),
    ],
    &[
        (1, 0b1, 0),
        (3, 0b001, 1),
        (3, 0b010, 2),
        (4, 0b0001, 3),
        (6, 0b000001, 4),
        (3, 0b011, 5),
        (5, 0b00001, 6),
        (7, 0b0000000, 7),
        (7, 0b0000001, 8),
    ],
];

/// Table 62: `CHR_CBPHP`, `VAL_INC` and `CBPHP_CH_BLK` share one table.
pub(crate) const CHR_CBPHP: &[Code] = &[(1, 0b1, 0), (2, 0b01, 1), (2, 0b00, 2)];

/// Table 63: `NUM_CH_BLK`.
pub(crate) const NUM_CH_BLK: &[Code] = &[(1, 0b1, 0), (2, 0b01, 1), (3, 0b000, 2), (3, 0b001, 3)];

/// Table 64: `REF_CBPHP1`.
pub(crate) const REF_CBPHP1: &[Code] = &[
    (2, 0b00, 3),
    (2, 0b01, 5),
    (3, 0b100, 6),
    (3, 0b101, 9),
    (3, 0b110, 10),
    (3, 0b111, 12),
];

/// Table 76: `RUN_VALUE` when `iMaxRun` is 2.
pub(crate) const RUN_VALUE_2: &[Code] = &[(1, 0b1, 1), (1, 0b0, 2)];

/// Table 77: `RUN_VALUE` when `iMaxRun` is 3.
pub(crate) const RUN_VALUE_3: &[Code] = &[(1, 0b1, 1), (2, 0b01, 2), (2, 0b00, 3)];

/// Table 78: `RUN_VALUE` when `iMaxRun` is 4.
pub(crate) const RUN_VALUE_4: &[Code] = &[(1, 0b1, 1), (2, 0b01, 2), (3, 0b001, 3), (3, 0b000, 4)];

/// Table 79: `RUN_INDEX`.
pub(crate) const RUN_INDEX: &[Code] = &[
    (1, 0b1, 0),
    (2, 0b01, 1),
    (3, 0b001, 2),
    (4, 0b0000, 3),
    (4, 0b0001, 4),
];

/// Table 80: `INDEX_A`, four code tables.
pub(crate) const INDEX_A: [&[Code]; 4] = [
    &[
        (1, 0b1, 0),
        (5, 0b00000, 1),
        (3, 0b001, 2),
        (5, 0b00001, 3),
        (2, 0b01, 4),
        (4, 0b0001, 5),
    ],
    &[
        (2, 0b01, 0),
        (4, 0b0000, 1),
        (2, 0b10, 2),
        (4, 0b0001, 3),
        (2, 0b11, 4),
        (3, 0b001, 5),
    ],
    &[
        (4, 0b0000, 0),
        (4, 0b0001, 1),
        (2, 0b01, 2),
        (2, 0b10, 3),
        (2, 0b11, 4),
        (3, 0b001, 5),
    ],
    &[
        (5, 0b00000, 0),
        (5, 0b00001, 1),
        (2, 0b01, 2),
        (1, 0b1, 3),
        (4, 0b0001, 4),
        (3, 0b001, 5),
    ],
];

/// Table 81: `INDEX_B`. The value column is 0, 2, 1, 3 — not ascending, and
/// written here in the table's own order so the transcription is checkable
/// against the Recommendation line by line.
pub(crate) const INDEX_B: &[Code] = &[(1, 0b0, 0), (2, 0b10, 2), (3, 0b110, 1), (3, 0b111, 3)];

/// Table 82: `FIRST_INDEX`, five code tables.
pub(crate) const FIRST_INDEX: [&[Code]; 5] = [
    &[
        (5, 0b00001, 0),
        (6, 0b000001, 1),
        (7, 0b0000000, 2),
        (7, 0b0000001, 3),
        (5, 0b00100, 4),
        (3, 0b010, 5),
        (5, 0b00101, 6),
        (1, 0b1, 7),
        (5, 0b00110, 8),
        (4, 0b0001, 9),
        (5, 0b00111, 10),
        (3, 0b011, 11),
    ],
    &[
        (4, 0b0010, 0),
        (5, 0b00010, 1),
        (6, 0b000000, 2),
        (6, 0b000001, 3),
        (4, 0b0011, 4),
        (3, 0b010, 5),
        (5, 0b00011, 6),
        (2, 0b11, 7),
        (3, 0b011, 8),
        (3, 0b100, 9),
        (5, 0b00001, 10),
        (3, 0b101, 11),
    ],
    &[
        (2, 0b11, 0),
        (3, 0b001, 1),
        (7, 0b0000000, 2),
        (7, 0b0000001, 3),
        (5, 0b00001, 4),
        (3, 0b010, 5),
        (7, 0b0000010, 6),
        (3, 0b011, 7),
        (3, 0b100, 8),
        (3, 0b101, 9),
        (7, 0b0000011, 10),
        (4, 0b0001, 11),
    ],
    &[
        (3, 0b001, 0),
        (2, 0b11, 1),
        (7, 0b0000000, 2),
        (5, 0b00001, 3),
        (5, 0b00010, 4),
        (3, 0b010, 5),
        (7, 0b0000001, 6),
        (3, 0b011, 7),
        (5, 0b00011, 8),
        (3, 0b100, 9),
        (6, 0b000001, 10),
        (3, 0b101, 11),
    ],
    &[
        (3, 0b010, 0),
        (1, 0b1, 1),
        (7, 0b0000001, 2),
        (4, 0b0001, 3),
        (7, 0b0000010, 4),
        (3, 0b011, 5),
        (8, 0b00000000, 6),
        (4, 0b0010, 7),
        (7, 0b0000011, 8),
        (4, 0b0011, 9),
        (8, 0b00000001, 10),
        (5, 0b00001, 11),
    ],
];

// --- 8.8.2 deltaDisc tables ---------------------------------------------

/// Table 86: `AbslevelIndexDelta[m][n]`. One row, because `ABS_LEVEL_INDEX`
/// has exactly two code tables and therefore one transition.
pub(crate) const ABS_LEVEL_INDEX_DELTA: [[i32; 7]; 1] = [[1, 0, -1, -1, -1, -1, -1]];

/// Table 87: `FirstIndexDelta[m][n]`, four rows for five code tables.
pub(crate) const FIRST_INDEX_DELTA: [[i32; 12]; 4] = [
    [1, 1, 1, 1, 1, 0, 0, -1, 2, 1, 0, 0],
    [2, 2, -1, -1, -1, 0, -2, -1, 0, 0, -2, -1],
    [-1, 1, 0, 2, 0, 0, 0, 0, -2, 0, 1, 1],
    [0, 1, 0, 1, -2, 0, -1, -1, -2, -1, -2, -2],
];

/// Table 88: `Index1Delta[m][n]`, three rows for four code tables.
pub(crate) const INDEX1_DELTA: [[i32; 6]; 3] = [
    [-1, 1, 1, 1, 0, 1],
    [-2, 0, 0, 2, 0, 0],
    [-1, -1, 0, 1, -2, 0],
];

/// Table 89: `NumCBPHPDelta[m][n]`.
pub(crate) const NUM_CBPHP_DELTA: [[i32; 5]; 1] = [[0, -1, 0, 1, 1]];

/// Table 90: `NumBlkCBPHPDelta[m][n]` for YONLY, NCOMPONENT and YUVK.
pub(crate) const NUM_BLKCBPHP_DELTA_YONLY: [[i32; 5]; 1] = [[0, -1, 0, 1, 1]];

/// Table 91: `NumBlkCBPHPDelta` for every other internal colour format.
pub(crate) const NUM_BLKCBPHP_DELTA_CHROMA: [[i32; 9]; 1] = [[2, 2, 1, 1, -1, -2, -2, -2, -3]];

// --- 8.11.1 scan orders --------------------------------------------------

/// Table 107's `ScanOrder0[i]`, with index 0 unused and set to 0 so that the
/// array is indexed by the clause's own `i` (which runs 1 to 15).
pub(crate) const SCAN_ORDER_0: [u8; 16] = [0, 4, 1, 5, 8, 2, 9, 6, 12, 3, 10, 13, 7, 14, 11, 15];

/// Table 107's `ScanOrder1[i]`, indexed the same way.
pub(crate) const SCAN_ORDER_1: [u8; 16] = [0, 1, 2, 5, 4, 3, 6, 9, 8, 7, 12, 15, 13, 10, 11, 14];

/// Table 108's `ScanTotals[i]`, indexed the same way.
pub(crate) const SCAN_TOTALS: [i32; 16] =
    [0, 32, 30, 28, 26, 24, 22, 20, 18, 16, 14, 12, 10, 8, 6, 4];

// --- 8.12.2 model weights ------------------------------------------------

/// `iWeight0[3]` — the luma weight per band (DC, LP, HP).
pub(crate) const MODEL_WEIGHT_0: [i32; 3] = [240, 12, 1];

/// `iWeight1[3][MAX_COMPONENTS]` — the chroma weight per band, indexed by
/// `NumComponents - 1`.
pub(crate) const MODEL_WEIGHT_1: [[i32; 16]; 3] = [
    [
        0, 240, 120, 80, 60, 48, 40, 34, 30, 27, 24, 22, 20, 18, 17, 16,
    ],
    [0, 12, 6, 4, 3, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 1],
    [0, 16, 8, 5, 4, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1, 1],
];

// --- 8.7 fixed permutations ---------------------------------------------

/// `iTranspose444[ ]`, from Table 53 and Table 84.
pub(crate) const TRANSPOSE_444: [usize; 16] =
    [0, 4, 8, 12, 1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15];

/// Table 164's `InvPermArr[i]`, 9.9.7.5: `arrayTemp[InvPermArr[i]]` takes
/// `arrayInput[i]`, so this reads as "where coefficient `i` goes", not "where
/// it comes from". Reading it the other way transposes the whole transform.
pub(crate) const INV_PERM: [usize; 16] = [0, 8, 4, 13, 2, 15, 3, 14, 1, 12, 5, 9, 7, 11, 6, 10];

/// `iHierScanOrder[ ]`, from Table 69: the hierarchical raster order in which
/// 8.7.17.1 stores a macroblock's sixteen blocks.
pub(crate) const HIER_SCAN_ORDER: [usize; 16] =
    [0, 1, 4, 5, 2, 3, 6, 7, 8, 9, 12, 13, 10, 11, 14, 15];

/// `iBitsQPIndex[ ]` from Table 47: the width of `QPINDEX_REF` for a table of
/// `iNumQP` quantization parameters.
pub(crate) const BITS_QP_INDEX: [u32; 17] = [0, 0, 1, 1, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4];
