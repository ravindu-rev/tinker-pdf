//! Milestone 8: the three totals, and the property each of them has to have.
//!
//! Ruling 1. The plan specified `MAX_JPX_SAMPLES`, `MAX_JPX_CODE_BLOCKS` and
//! `MAX_JPX_WORK` **by shape and deliberately not by number**, because —
//! its words — "picking them from imagination is how `MAX_GROUP_DEPTH` got
//! set to a number that did not stop anything". `5adf502` is the scar: an
//! 1 851-byte page that took 19.3 seconds to render 9 600 pixels, inside a
//! nesting cap that was in place the entire time.
//!
//! So the tests here assert *relations* rather than magnitudes wherever a
//! relation exists, since a magnitude restated in a test is a transcription
//! of the constant and agrees with it however wrong it is. The one that
//! matters most is [`the_work_cap_can_fire_at_all`], which is the test that
//! `MAX_GROUP_DEPTH` never had.

use crate::jpx::codestream::parse;
use crate::jpx::tier1::{decode_code_block, initial_contexts, MAX_PASSES};
use crate::jpx::tier2::Orientation;
use crate::jpx::{Refusal, MAX_JPX_CODE_BLOCKS, MAX_JPX_SAMPLES, MAX_JPX_WORK};

use super::writer::Spec;

/// A budget must be below the most its own inputs can ask for, or it is not
/// a budget.
///
/// **This is the one that was false.** `MAX_JPX_WORK` stood at `3 << 31` from
/// milestone 1 until this milestone measured it, and no codestream could
/// reach it: tier-1's work is `sum(code-block area * passes)`, T.800's
/// subbands are critically sampled so the areas over every resolution of a
/// tile-component sum to exactly that tile-component's sample count, and
/// `passes` is capped at [`MAX_PASSES`]. So the ceiling of the input is
/// `MAX_JPX_SAMPLES * MAX_PASSES`, and `3 << 31` sat above it.
///
/// The relation is asserted rather than the number for the reason the whole
/// module opens with: a test that restated `3 << 31` would have agreed with
/// the constant perfectly and said nothing.
#[test]
fn the_work_cap_can_fire_at_all() {
    let ceiling = MAX_JPX_SAMPLES * u64::from(MAX_PASSES);
    assert!(
        MAX_JPX_WORK < ceiling,
        "MAX_JPX_WORK is {MAX_JPX_WORK}, at or above the {ceiling} \
         coefficient-passes a codestream at the sample ceiling can ask for. \
         A budget no input can reach is not a budget -- it is MAX_GROUP_DEPTH \
         again, and 5adf502 is what that costs."
    );
    // And below is not enough on its own: a cap one pass under the ceiling
    // would satisfy the line above and still refuse nothing real. The
    // measured worst case over every codestream this repository has is 22.75
    // coefficient-passes per tile-component sample -- a lossless 5/3
    // greyscale, which is the densest coding there is -- so a cap that leaves
    // real headroom sits between that and 91.
    let per_sample = MAX_JPX_WORK / MAX_JPX_SAMPLES;
    assert!(
        (23..u64::from(MAX_PASSES)).contains(&per_sample),
        "MAX_JPX_WORK is {per_sample} coefficient-passes per sample, which is \
         either under the 22.75 the densest measured codestream spends -- so \
         it refuses a real file -- or at the format's own maximum, so it \
         refuses nothing."
    );
}

/// The code-block cap is T.800's arithmetic on the sample cap, not a number
/// of its own.
///
/// B.7 puts a code-block at 4 x 4 samples minimum, so `MAX_JPX_SAMPLES`
/// cannot contain more than a sixteenth of itself in code-blocks however the
/// partition is cut. Asserted as the relation so that moving one constant and
/// forgetting the other fails here.
#[test]
fn the_code_block_cap_is_the_sample_cap_at_b7s_smallest_block() {
    assert_eq!(MAX_JPX_CODE_BLOCKS * 16, MAX_JPX_SAMPLES);
}

/// The whole code-block is charged before any of it runs, and the boundary is
/// `area * passes` exactly.
///
/// This is the assertion the two possible designs disagree on. Charging one
/// pass at a time also totals `area * passes`, so a test that only checked
/// the sum would pass against either — but a per-pass charge refuses the last
/// pass having already run the ones before it, which is to say having already
/// spent the time the budget exists to prevent. So the evidence is that the
/// budget is **untouched** after the refusal: nothing was decoded.
///
/// `5adf502`'s method, and the milestone's exit criterion, is that the
/// refusal be assertable by the warning rather than by a clock. It only is if
/// the charge comes from what the packet header *declared*.
#[test]
fn the_work_charge_is_the_whole_block_before_any_of_it() {
    // Sixteen coefficients over ten passes: inside every per-item cap there
    // is, and 160 coefficient-passes.
    let (width, height, passes) = (4u32, 4u32, 10u32);
    let cost = u64::from(width * height * passes);
    let data = [0x80u8; 64];

    let mut contexts = initial_contexts();
    let mut exact = cost;
    assert!(
        decode_code_block(
            &data,
            width,
            height,
            passes,
            Orientation::Ll,
            false,
            &mut contexts,
            &mut exact,
        )
        .is_ok(),
        "a block costing exactly the remaining budget is inside it"
    );
    assert_eq!(exact, 0, "and it spends all of it");

    let mut short = cost - 1;
    assert_eq!(
        decode_code_block(
            &data,
            width,
            height,
            passes,
            Orientation::Ll,
            false,
            &mut contexts,
            &mut short,
        ),
        Err(Refusal::Budget("tier-1 coefficient passes")),
        "a block costing one more than the budget is refused"
    );
    assert_eq!(
        short,
        cost - 1,
        "and refused before it decoded anything: a per-pass charge would have \
         run nine of the ten passes and left {} here",
        u64::from(width * height) - 1
    );
}

/// A branching stream that stays inside every per-item cap is refused by the
/// total.
///
/// Every factor here is individually legal and modest — one tile, one
/// component, one decomposition level, a 64 x 64 code-block partition, an
/// image well inside `MAX_JPX_SAMPLES` — and the codestream is under two
/// hundred bytes. What runs away is the *product* with the pass count, which
/// comes out of a packet header and costs a handful of bits to write.
///
/// Asserted through `decode_code_block` at the budget the geometry implies
/// rather than by building a 35-megapixel fixture, because the point is that
/// the refusal arrives from the declaration: driving the real ceiling would
/// need an image of that size and would then be a test paced by a clock,
/// which is the thing `5adf502` says not to build.
#[test]
fn a_stream_inside_every_per_item_cap_is_refused_by_the_total() {
    // T.800's largest code-block, at T.800's most passes. One of these is
    // 372 736 coefficient-passes, bought with a segment length and a pass
    // count in a packet header.
    let (width, height) = (64u32, 64u32);
    let per_block = u64::from(width * height) * u64::from(MAX_PASSES);
    assert_eq!(per_block, 372_736);

    // The real budget lets 8 641 of them through and refuses the next, and
    // every one of those blocks is inside MAX_JPX_CODE_BLOCKS (4 194 304)
    // and inside Table A.18's code-block bound and inside MAX_PASSES.
    let blocks = MAX_JPX_WORK / per_block;
    assert!(
        blocks < MAX_JPX_CODE_BLOCKS,
        "the per-item cap is not the one"
    );

    let mut contexts = initial_contexts();
    let mut work = MAX_JPX_WORK - blocks * per_block;
    let data = [0x80u8; 256];
    assert_eq!(
        decode_code_block(
            &data,
            width,
            height,
            MAX_PASSES,
            Orientation::Hh,
            false,
            &mut contexts,
            &mut work,
        ),
        Err(Refusal::Budget("tier-1 coefficient passes")),
        "the {blocks}th 64x64 block at 91 passes is past the total, and every \
         per-item cap is still satisfied"
    );
}

/// The sample cap is spent across components, not per tile-component.
///
/// The plan's own sentence, made into a file: "one tile inside the ceiling
/// times 16 384 components is not". This is one tile of 4096 x 4096 — a
/// quarter of `MAX_JPX_SAMPLES` on its own, and an entirely ordinary image —
/// carried on the largest component count `MAX_JPX_COMPONENTS` permits. Every
/// per-item cap is satisfied and the product is sixteen times the total.
#[test]
fn the_sample_cap_is_spent_across_components_rather_than_per_tile() {
    let spec = Spec {
        xsiz: 4096,
        ysiz: 4096,
        xtsiz: 4096,
        ytsiz: 4096,
        components: vec![(8, false, 1, 1); 64],
        ..Spec::default()
    };
    let bytes = spec.codestream(&[]);
    let stream = parse(&bytes).expect("the headers parse");
    assert!(
        u64::from(spec.xsiz) * u64::from(spec.ysiz) < MAX_JPX_SAMPLES,
        "the one tile-component is inside the total, which is the point"
    );
    assert_eq!(
        stream.check_budget(&crate::Limits::new(usize::MAX)),
        Err(Refusal::Budget("tile-component samples")),
        "and 64 of it is not"
    );
}
