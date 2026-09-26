//! The entropy layer's own properties: the code tables, the adaptation, and
//! 9.8.4's quantizer map.
//!
//! # What these tests are for, given there is no oracle
//!
//! `docs/design/jpeg-xr.md` names three first-party evidence legs, and none
//! of them reaches *inside* the entropy decoder: the lossless identity is a
//! whole-image comparison, so a transcription slip in a VLC table shows up as
//! "the picture is wrong" with no indication of where. These tests close that
//! gap for the part of the format that is pure data — **twenty-seven** code
//! tables transcribed by hand from ITU-T T.832's Tables 51 to 91 — by checking the
//! *structural* properties a Huffman table has to have. A single wrong bit in
//! any code almost always breaks one of them.
//!
//! # Counted injections
//!
//! Every check below is run against a deliberately broken copy of the thing
//! it checks, and the number of assertions that fire is recorded here. A
//! plausible break that fires **nothing** would mean the suite does not test
//! what it claims, so a zero is reported as loudly as a large number.
//!
//! | Injection | Assertions that fire |
//! | --- | ---: |
//! | One code shortened by a bit (`FIRST_INDEX` table 0, value 7: `1` -> the empty prefix of `010`) — breaks prefix-freeness | **11** offending pairs |
//! | The same shortening, measured by the Kraft sum instead | **1** table over budget |
//! | One code's value column duplicated (`INDEX_A` table 1, value 5 -> 4) | **1** table with a repeated value |
//! | `read_vlc` made length-blind (matching on bits alone) | **24 of the 27** code tables mis-decode a symbol |
//! | 9.8.4's `iNotScaledShift` flipped from -2 to +2 | **5** of the 9 spot values change |
//! | 8.8.4.5's `DiscrimVal1` allowed to *raise* the table index too | **1** of the 5 adaptation steps changes |
//!
//! Six injections, and none of them is silent. Two of the counts are worth
//! reading twice, and both were measured rather than predicted — the numbers
//! written here first were 8 and 2, and both were wrong.
//!
//! The length-blind reader fails **24 of 27** tables, not a handful: a
//! one-bit `0` and a two-bit `00` are the same number, and almost every
//! table in T.832 contains such a pair. That is why the length is stored
//! beside the code and compared with it.
//!
//! The discriminant injection fires on only **1 of 5** steps, and the reason
//! is worth stating rather than tuning away: the two discriminants usually
//! agree, so conflating them is invisible except in the one case where
//! `DiscrimVal1` alone is above the threshold — a table index that should
//! have stayed put and instead climbs. A rare divergence in a *stateful*
//! decoder is not a small bug: from that macroblock on, every symbol is read
//! with the wrong table.

use super::*;

// --- the structural properties of a code table --------------------------

/// The pairs in `table` where one code is a prefix of another. A prefix code
/// has none; if it had one, a reader accumulating bits could never tell which
/// of the two it had just seen.
fn prefix_violations(table: &[tables::Code]) -> usize {
    let mut count = 0;
    for (i, &(la, ca, _)) in table.iter().enumerate() {
        for (j, &(lb, cb, _)) in table.iter().enumerate() {
            if i == j || la > lb {
                continue;
            }
            // Is `ca` (length `la`) the leading `la` bits of `cb`?
            if cb >> (lb - la) == ca {
                count += 1;
            }
        }
    }
    count
}

/// Kraft's sum for `table`, scaled by `2^max_len` so it is exact in integers
/// (ruling 4 — and this module denies float arithmetic anyway). A complete
/// prefix code sums to exactly `2^max_len`.
fn kraft(table: &[tables::Code]) -> (u32, u32) {
    let max = table.iter().map(|&(l, _, _)| l).max().unwrap_or(0);
    let total: u32 = table.iter().map(|&(l, _, _)| 1u32 << (max - l)).sum();
    (total, 1u32 << max)
}

/// Every code table in `tables.rs`, with the name the Recommendation gives it.
fn all_tables() -> Vec<(&'static str, &'static [tables::Code])> {
    let mut v: Vec<(&'static str, &'static [tables::Code])> = vec![
        ("VAL_DC_YUV", tables::VAL_DC_YUV),
        ("CBPLP_YUV1_444", tables::CBPLP_YUV1_444),
        ("CHR_CBPHP", tables::CHR_CBPHP),
        ("NUM_CH_BLK", tables::NUM_CH_BLK),
        ("REF_CBPHP1", tables::REF_CBPHP1),
        ("RUN_VALUE_2", tables::RUN_VALUE_2),
        ("RUN_VALUE_3", tables::RUN_VALUE_3),
        ("RUN_VALUE_4", tables::RUN_VALUE_4),
        ("RUN_INDEX", tables::RUN_INDEX),
        ("INDEX_B", tables::INDEX_B),
    ];
    for (i, t) in tables::ABS_LEVEL_INDEX.iter().enumerate() {
        v.push((
            if i == 0 {
                "ABS_LEVEL_INDEX[0]"
            } else {
                "ABS_LEVEL_INDEX[1]"
            },
            t,
        ));
    }
    for t in tables::NUM_CBPHP {
        v.push(("NUM_CBPHP", t));
    }
    for t in tables::NUM_BLKCBPHP_YONLY {
        v.push(("NUM_BLKCBPHP_YONLY", t));
    }
    for t in tables::NUM_BLKCBPHP_CHROMA {
        v.push(("NUM_BLKCBPHP_CHROMA", t));
    }
    for t in tables::INDEX_A {
        v.push(("INDEX_A", t));
    }
    for t in tables::FIRST_INDEX {
        v.push(("FIRST_INDEX", t));
    }
    v
}

#[test]
fn the_table_count_is_what_the_prose_says() {
    // Every count in this file's header is a claim about `all_tables()`, and
    // a claim in a comment drifts silently. Ten tables are named singly, and
    // six constants carry the rest: ABS_LEVEL_INDEX and the three CBPHP pairs
    // contribute two each, INDEX_A four, FIRST_INDEX five.
    assert_eq!(all_tables().len(), 27, "the header's count is stale");
}

#[test]
fn every_code_table_is_prefix_free() {
    for (name, table) in all_tables() {
        assert_eq!(prefix_violations(table), 0, "{name} is not a prefix code");
    }
}

#[test]
fn a_shortened_code_is_caught_as_a_prefix_violation() {
    // The injection: `FIRST_INDEX` code table 0's value 7 is the one-bit code
    // `1`. Shorten another entry so that it collides — take value 5's `010`
    // and drop its last bit, making `01` a prefix of nothing but making
    // value 11's `011` share it. This is the shape a transcription slip takes
    // when a space in the Recommendation's "0000 1" is read as a separator
    // rather than as part of one code.
    let mut broken: Vec<tables::Code> = tables::FIRST_INDEX[0].to_vec();
    broken[7] = (0, 0, 7); // the empty code, a prefix of everything
    assert_eq!(
        prefix_violations(&broken),
        11,
        "the injection must fire on every other code in the table"
    );
}

#[test]
fn every_code_table_is_a_complete_prefix_code() {
    // A complete code assigns every leaf of its binary tree, and **every one
    // of the twenty-seven is complete**. There is no exception list, and
    // that is the point: an earlier draft of this file transcribed Table 52's
    // code table 0 with its last two rows swapped, which left one leaf
    // unassigned. The table was still prefix-free, still decoded most
    // symbols, and still passed every other check here — this assertion is
    // the one that caught it, and it was caught only after the exception it
    // had been given was removed. A test with an exception list is a test
    // that has been talked out of firing.
    for (name, table) in all_tables() {
        let (sum, budget) = kraft(table);
        assert_eq!(sum, budget, "{name} is not a complete prefix code");
    }
}

#[test]
fn a_shortened_code_is_caught_by_the_kraft_sum_too() {
    // The same injection as above, measured by the other property. Two
    // independent checks over the same data is the point: a slip that keeps
    // the code lengths but permutes the bits breaks prefix-freeness and not
    // the Kraft sum, and a slip that changes a length breaks both.
    let mut over = 0;
    for (name, table) in all_tables() {
        let mut broken: Vec<tables::Code> = table.to_vec();
        if name != "FIRST_INDEX" {
            continue;
        }
        broken[7].0 -= 1;
        let (sum, budget) = kraft(&broken);
        if sum > budget {
            over += 1;
        }
        break;
    }
    assert_eq!(over, 1, "shortening a code must put its table over budget");
}

#[test]
fn every_code_table_maps_each_value_once() {
    for (name, table) in all_tables() {
        let mut seen: Vec<u8> = table.iter().map(|&(_, _, v)| v).collect();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), before, "{name} decodes two codes to one value");
    }
}

#[test]
fn a_duplicated_value_column_is_caught() {
    // `INDEX_A` code table 1's value 5 misread as a repeat of 4 — the shape a
    // slip takes when the Recommendation's value column is read one row out.
    let mut broken: Vec<tables::Code> = tables::INDEX_A[1].to_vec();
    broken[5].2 = 4;
    let mut seen: Vec<u8> = broken.iter().map(|&(_, _, v)| v).collect();
    seen.sort_unstable();
    let before = seen.len();
    seen.dedup();
    assert_eq!(before - seen.len(), 1, "exactly one duplicate must show");
}

// --- the reader agrees with the tables ----------------------------------

/// Packs codes MSB-first, which is what 5.2's `u(n)` reads.
fn pack(codes: &[(u8, u16)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &(len, code) in codes {
        acc = (acc << len) | u32::from(code);
        bits += u32::from(len);
        while bits >= 8 {
            bits -= 8;
            // The mask keeps the narrowing exact.
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    if bits > 0 {
        out.push(((acc << (8 - bits)) & 0xFF) as u8);
    }
    out
}

#[test]
fn read_vlc_returns_every_symbol_of_every_table() {
    for (name, table) in all_tables() {
        // One pass writes the whole table's codes in order; one read must
        // return the whole value column in the same order. Writing them back
        // to back also proves the reader consumes exactly the code's length
        // and no more, which reading one code at a time would not.
        let packed = pack(&table.iter().map(|&(l, c, _)| (l, c)).collect::<Vec<_>>());
        let mut r = BitReader::new(&packed);
        for &(_, _, want) in table {
            assert_eq!(read_vlc(&mut r, table), Ok(want), "{name}");
        }
    }
}

/// `read_vlc` with the length comparison removed — the defect the `(len,
/// code)` pair exists to prevent.
fn read_vlc_length_blind(r: &mut BitReader<'_>, table: &[tables::Code]) -> Result<u8, JxrError> {
    let mut acc: u16 = 0;
    let mut len: u8 = 0;
    loop {
        acc = (acc << 1) | (r.read(1)? as u16);
        len += 1;
        for &(_, c, v) in table {
            if c == acc {
                return Ok(v);
            }
        }
        if len >= 16 {
            return Err(JxrError::Truncated);
        }
    }
}

#[test]
fn a_length_blind_reader_mis_decodes_most_tables() {
    // The injection that matters most, because it is the one a reviewer
    // would expect to be harmless: a code table whose codes are all distinct
    // as *numbers* looks decodable without the lengths. Most are not — a
    // one-bit `0` and a two-bit `00` are the same number.
    let mut wrong = 0;
    for (_, table) in all_tables() {
        let packed = pack(&table.iter().map(|&(l, c, _)| (l, c)).collect::<Vec<_>>());
        let mut r = BitReader::new(&packed);
        let mut ok = true;
        for &(_, _, want) in table {
            if read_vlc_length_blind(&mut r, table) != Ok(want) {
                ok = false;
                break;
            }
        }
        if !ok {
            wrong += 1;
        }
    }
    assert_eq!(
        wrong, 24,
        "the length comparison must be load-bearing for almost every table"
    );
}

// --- 9.8.4's quantizer map ----------------------------------------------

#[test]
fn quant_map_matches_9_8_4s_three_branches() {
    // Hand-computed from Table 149. The unscaled branch's three ranges meet
    // at 32 and 48, and both boundaries are checked from either side, because
    // an off-by-one there is a wrong picture at exactly one quantizer.
    let unscaled = [
        (0u8, 1i32),
        (1, 1),
        (5, 2),
        (31, 8),
        (32, 8),
        (47, 16),
        (48, 16),
        (63, 31),
        (64, 32),
    ];
    for (qp, want) in unscaled {
        assert_eq!(quant_map(qp, 0, false), want, "unscaled QP {qp}");
        // `iScaledShift` has no effect when `SCALED_FLAG` is false.
        assert_eq!(quant_map(qp, 1, false), want, "unscaled QP {qp}, shift 1");
    }
    let scaled_luma = [(0u8, 1i32), (1, 2), (15, 30), (16, 32), (31, 62), (32, 64)];
    for (qp, want) in scaled_luma {
        assert_eq!(quant_map(qp, 1, true), want, "scaled QP {qp}, shift 1");
    }
    let scaled_chroma = [(0u8, 1i32), (1, 1), (15, 15), (16, 16), (32, 32)];
    for (qp, want) in scaled_chroma {
        assert_eq!(quant_map(qp, 0, true), want, "scaled QP {qp}, shift 0");
    }
    // The largest value the clause can produce still fits an `i32`.
    assert_eq!(quant_map(255, 0, false), 31 << 12);
}

#[test]
fn flipping_the_not_scaled_shift_changes_five_of_nine_spot_values() {
    // `iNotScaledShift` is -2 in Table 149. Its sign is the kind of thing a
    // transcription gets wrong, and it only shows above QP 32 — below that
    // the exponent is a literal 0 and the shift is unused.
    fn broken(qp: u8) -> i32 {
        let qp = i32::from(qp);
        if qp == 0 {
            return 1;
        }
        const NOT_SCALED_SHIFT: i32 = 2; // the injection: was -2
        let (man, exp) = if qp < 32 {
            ((qp + 3) >> 2, 0)
        } else if qp < 48 {
            (
                (16 + (qp % 16) + 1) >> 1,
                ((qp >> 4) + NOT_SCALED_SHIFT) as u32,
            )
        } else {
            (16 + (qp % 16), ((qp >> 4) - 1 + NOT_SCALED_SHIFT) as u32)
        };
        man << exp.min(30)
    }
    let probes = [0u8, 1, 5, 31, 32, 47, 48, 63, 64];
    let changed = probes
        .iter()
        .filter(|&&qp| broken(qp) != quant_map(qp, 0, false))
        .count();
    // Four of the nine probes are below QP 32, where Table 149's exponent is
    // a literal 0 and the shift is unused; every probe above it changes.
    assert_eq!(
        changed, 5,
        "the sign of iNotScaledShift must be load-bearing"
    );
}

// --- 8.8.4's adaptation --------------------------------------------------

#[test]
fn adapt_vlc_table1_moves_only_past_its_thresholds() {
    // 8.8.4.4: the bounds are -8 and 8 *exclusive*, and reaching one without
    // passing it clips the discriminant instead of switching tables.
    let mut v = AdaptiveVlc::init1();
    v.discrim1 = 8;
    v.adapt1();
    assert_eq!((v.table_index, v.discrim1), (0, 8), "8 is not past 8");
    v.discrim1 = 9;
    v.adapt1();
    assert_eq!((v.table_index, v.discrim1), (1, 0), "9 is");
    // At the maximum index the discriminant is clipped rather than ignored.
    v.discrim1 = 1000;
    v.adapt1();
    assert_eq!((v.table_index, v.discrim1), (1, 64));
    v.discrim1 = -9;
    v.adapt1();
    assert_eq!((v.table_index, v.discrim1), (0, 0));
}

#[test]
fn adapt_vlc_table2_uses_one_discriminant_per_direction() {
    // 8.8.4.5: `DiscrimVal1` only ever lowers the index and `DiscrimVal2`
    // only ever raises it. A structure that used one for both would still
    // adapt — plausibly, and wrongly.
    let mut v = AdaptiveVlc::init2();
    assert_eq!(v.table_index, 1, "8.8.3.6 starts at 1");
    v.discrim2 = 9;
    v.adapt2(4);
    assert_eq!(v.table_index, 2);
    assert_eq!((v.delta_table_index, v.delta2_table_index), (1, 2));
    v.discrim1 = -9;
    v.adapt2(4);
    assert_eq!(v.table_index, 1);
    // At the top index both delta tables are the last one.
    v.table_index = 3;
    v.discrim2 = 9;
    v.adapt2(4);
    assert_eq!(v.table_index, 4);
    assert_eq!((v.delta_table_index, v.delta2_table_index), (3, 3));
    // At the bottom index both are the first.
    v.table_index = 1;
    v.discrim1 = -9;
    v.adapt2(4);
    assert_eq!(v.table_index, 0);
    assert_eq!((v.delta_table_index, v.delta2_table_index), (0, 0));
}

#[test]
fn conflating_the_two_discriminants_changes_one_of_five_steps() {
    // The injection: `DiscrimVal1` allowed to raise the index as well.
    fn broken(v: &mut AdaptiveVlc, max: usize) {
        if v.discrim1 < -8 && v.table_index != 0 {
            v.table_index -= 1;
        } else if (v.discrim1 > 8 || v.discrim2 > 8) && v.table_index != max {
            v.table_index += 1;
        }
        v.discrim1 = 0;
        v.discrim2 = 0;
    }
    let steps: [(i32, i32); 5] = [(9, 0), (0, 9), (-9, 0), (0, -9), (9, 9)];
    let mut changed = 0;
    for (d1, d2) in steps {
        let mut real = AdaptiveVlc::init2();
        real.discrim1 = d1;
        real.discrim2 = d2;
        real.adapt2(4);
        let mut inj = AdaptiveVlc::init2();
        inj.discrim1 = d1;
        inj.discrim2 = d2;
        broken(&mut inj, 4);
        if real.table_index != inj.table_index {
            changed += 1;
        }
    }
    // Only the (9, 0) step separates them: everywhere else the two
    // discriminants happen to agree. See the module header — a divergence
    // this rare is not a small bug in a decoder whose state carries forward.
    assert_eq!(
        changed, 1,
        "the direction each discriminant controls must be load-bearing"
    );
}

// --- 8.12.2's model bits -------------------------------------------------

#[test]
fn the_model_bits_move_one_step_at_a_time() {
    // 8.12.2 never moves `MBits` by more than one per macroblock, whatever
    // the input: a codec that jumped would shift every coefficient in the
    // macroblock by a power of two.
    let mut m = Model::init(2);
    assert_eq!(m.bits, [0, 0], "the HP band starts at (2 - 2) * 4");
    let mut previous = m.bits;
    for lap in [0i64, 1, 4, 16, 64, 256, 1024, 4096, 0, 0, 0, 0, 0, 0, 0, 0] {
        m.update([lap, lap], 2, InternalClrFmt::Yuv444, 3);
        assert!(
            (m.bits[0] - previous[0]).abs() <= 1 && (m.bits[1] - previous[1]).abs() <= 1,
            "MBits jumped from {previous:?} to {:?}",
            m.bits
        );
        assert!(m.bits[0] >= 0 && m.bits[0] <= 15, "MBits left 0..=15");
        previous = m.bits;
    }
    // And the DC and LP bands start where 8.12.1 says.
    assert_eq!(Model::init(0).bits, [8, 8]);
    assert_eq!(Model::init(1).bits, [4, 4]);
}

// --- 8.11's adaptive scan ------------------------------------------------

#[test]
fn the_adaptive_scan_is_a_permutation_however_it_adapts() {
    // 8.11.1: the scan order is "a permutation of the integers 1 to 15", and
    // 8.11.6 only ever *swaps* neighbours — so it stays one. A scan that
    // dropped or duplicated a position would put two coefficients in one
    // place, which is a soft, plausible, wrong picture.
    let mut scan = Scan::new(tables::SCAN_ORDER_0);
    for round in 0..64 {
        for i in 1..=15usize {
            // A deterministic, uneven access pattern: enough asymmetry to
            // drive the bubble in both directions.
            let at = ((i * 7 + round * 5) % 15) + 1;
            assert!(scan.place(at).is_some());
        }
        let mut seen: Vec<u8> = scan.order[1..].to_vec();
        seen.sort_unstable();
        assert_eq!(seen, (1..=15u8).collect::<Vec<_>>(), "round {round}");
    }
    // Out of range is refused rather than wrapped — a block has fifteen
    // coefficients at positions 1 to 15 and no others.
    assert_eq!(scan.place(0), None);
    assert_eq!(scan.place(16), None);
}
