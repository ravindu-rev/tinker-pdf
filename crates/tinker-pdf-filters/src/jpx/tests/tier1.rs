//! Milestone 3: T.800 Annex D's context tables, asserted entry by entry.
//!
//! **Not by round trip, and that is the whole point of this file.**
//!
//! Relabelling a context array is a bijection. Every adaptive state starts
//! identical, so an encoder's slot histories and a decoder's stay in step
//! under *any* permutation of the labels — the arithmetic coder never notices
//! that context 5 and context 7 have swapped names, because it only ever asks
//! "what happened last time in this slot".
//!
//! Gap 17 proved this on the codec next door rather than reasoning about it:
//! it transposed two of JBIG2's template 0 context bits and T.88's **Annex
//! H.1 still decoded to its published picture byte for byte**. A round trip
//! against an encoder that shares the mistake agrees with itself perfectly.
//!
//! JPEG 2000's exposure is wider than JBIG2's. There are three zero-coding
//! mappings rather than one — the LL/LH column, the HL column that is the
//! same table with the neighbour counts interchanged, and HH's entirely
//! different one — plus a sign-coding table that carries an **XOR bit**
//! alongside its context, which is a second thing to get wrong that produces
//! a picture rather than an error.
//!
//! So every row below is the standard's own number.

use crate::jpx::tier1::{
    initial_contexts, magnitude_refinement_context, pass_at, sign_coding_context,
    zero_coding_context, Pass, MAGNITUDE_REFINEMENT, MAX_PASSES, RUN_LENGTH, SIGN_CODING, UNIFORM,
    ZERO_CODING,
};
use crate::jpx::tier2::Orientation;
use crate::mq::MqDecoder;

/// Table D.1, the LL and LH column, all nine rows.
///
/// `(sigma_h, sigma_v, sigma_d, context)`. The rows are ordered as the
/// standard prints them, most significant first, because the table is a
/// priority list: the first row whose condition holds is the answer, and a
/// transcription that reorders them still satisfies most inputs.
#[test]
fn table_d1_ll_and_lh_columns_entry_by_entry() {
    // (h, v, d, want). `x` in the standard means "any", so the cases below
    // pick a representative and the saturating rows are checked separately.
    let rows: &[(u32, u32, u32, usize)] = &[
        (2, 0, 0, 8),
        (2, 1, 1, 8),
        (1, 1, 0, 7),
        (1, 2, 3, 7),
        (1, 0, 1, 6),
        (1, 0, 4, 6),
        (1, 0, 0, 5),
        (0, 2, 0, 4),
        (0, 2, 3, 4),
        (0, 1, 0, 3),
        (0, 1, 2, 3),
        (0, 0, 2, 2),
        (0, 0, 3, 2),
        (0, 0, 1, 1),
        (0, 0, 0, 0),
    ];

    for &(h, v, d, want) in rows {
        for orientation in [Orientation::Ll, Orientation::Lh] {
            assert_eq!(
                zero_coding_context(orientation, h, v, d),
                ZERO_CODING + want,
                "Table D.1 {orientation:?}: sigma_h {h}, sigma_v {v}, sigma_d {d}"
            );
        }
    }
}

/// Table D.1's HL column is the same table with the neighbour counts
/// interchanged, which the standard says in words rather than printing again.
///
/// This is the one place the transcription is not literal, so it gets its own
/// assertion: HL at `(h, v, d)` must equal LL at `(v, h, d)` for every row,
/// and — the part that matters — must **differ** from LL at `(h, v, d)`
/// wherever the table is not symmetric. Without that second half, a decoder
/// that ignored the swap entirely would pass.
#[test]
fn table_d1_hl_column_is_the_same_table_with_h_and_v_interchanged() {
    let mut asymmetric = 0;
    for h in 0..3 {
        for v in 0..3 {
            for d in 0..5 {
                assert_eq!(
                    zero_coding_context(Orientation::Hl, h, v, d),
                    zero_coding_context(Orientation::Ll, v, h, d),
                    "HL at ({h}, {v}, {d}) is LL at ({v}, {h}, {d})"
                );
                if zero_coding_context(Orientation::Hl, h, v, d)
                    != zero_coding_context(Orientation::Ll, h, v, d)
                {
                    asymmetric += 1;
                }
            }
        }
    }
    assert!(
        asymmetric > 0,
        "HL never differs from LL, so the interchange is not happening at all"
    );
}

/// Table D.1's HH column, which is a different table rather than a permuted
/// one: it keys on the diagonal count first and the sum of the other two
/// second.
#[test]
fn table_d1_hh_column_entry_by_entry() {
    // (h + v, d, want) -- the standard's HH column keys on sigma_d and
    // sigma_h + sigma_v.
    let rows: &[(u32, u32, u32, usize)] = &[
        (0, 0, 3, 8),
        (1, 0, 4, 8),
        (1, 0, 2, 7),
        (0, 1, 2, 7),
        (0, 0, 2, 6),
        (2, 0, 1, 5),
        (1, 1, 1, 5),
        (1, 0, 1, 4),
        (0, 1, 1, 4),
        (0, 0, 1, 3),
        (2, 0, 0, 2),
        (1, 1, 0, 2),
        (1, 0, 0, 1),
        (0, 1, 0, 1),
        (0, 0, 0, 0),
    ];

    for &(h, v, d, want) in rows {
        assert_eq!(
            zero_coding_context(Orientation::Hh, h, v, d),
            ZERO_CODING + want,
            "Table D.1 HH: sigma_h {h}, sigma_v {v}, sigma_d {d}"
        );
    }
}

/// Every zero-coding answer is inside the nine contexts the table defines.
///
/// A saturating count — a code-block interior has up to two horizontal, two
/// vertical and four diagonal significant neighbours — must not walk off the
/// end into the sign-coding contexts, which is a defect that decodes rather
/// than crashes.
#[test]
fn zero_coding_never_leaves_its_nine_contexts() {
    for orientation in [
        Orientation::Ll,
        Orientation::Hl,
        Orientation::Lh,
        Orientation::Hh,
    ] {
        for h in 0..=2 {
            for v in 0..=2 {
                for d in 0..=4 {
                    let ctx = zero_coding_context(orientation, h, v, d);
                    assert!(
                        (ZERO_CODING..ZERO_CODING + 9).contains(&ctx),
                        "{orientation:?} ({h}, {v}, {d}) gave context {ctx}, outside D.1"
                    );
                }
            }
        }
    }
}

/// Table D.3, sign coding: nine rows, each carrying a context **and an XOR
/// bit**.
///
/// The XOR bit is the half that produces a picture rather than an error when
/// it is wrong: the sign is decoded and then flipped, so dropping the bit
/// gives a coefficient of the right magnitude and the wrong sign, and an
/// inverse wavelet turns a field of those into a plausible image.
#[test]
fn table_d3_sign_coding_entry_by_entry() {
    // (h_contribution, v_contribution, context, xor)
    let rows: &[(i32, i32, usize, u8)] = &[
        (1, 1, 13, 0),
        (1, 0, 12, 0),
        (1, -1, 11, 0),
        (0, 1, 10, 0),
        (0, 0, 9, 0),
        (0, -1, 10, 1),
        (-1, 1, 11, 1),
        (-1, 0, 12, 1),
        (-1, -1, 13, 1),
    ];

    for &(h, v, ctx, xor) in rows {
        assert_eq!(
            sign_coding_context(h, v),
            (ctx, xor),
            "Table D.3: h {h}, v {v}"
        );
    }
}

/// D.3's symmetry, stated as its own property: negating both contributions
/// keeps the context and flips the XOR bit.
///
/// A table transcribed with one row's XOR bit wrong satisfies every
/// individual row above only if that row is also wrong — this catches the
/// case where two rows were swapped and each looks locally plausible.
#[test]
fn sign_coding_is_antisymmetric() {
    for h in -1..=1 {
        for v in -1..=1 {
            let (ctx, xor) = sign_coding_context(h, v);
            let (neg_ctx, neg_xor) = sign_coding_context(-h, -v);
            assert_eq!(ctx, neg_ctx, "negating ({h}, {v}) changed the context");
            if h == 0 && v == 0 {
                assert_eq!(xor, neg_xor, "the origin is its own negation");
            } else {
                assert_ne!(
                    xor, neg_xor,
                    "negating ({h}, {v}) must flip the XOR bit, or a sign is decoded twice"
                );
            }
        }
    }
}

/// Every sign-coding answer is inside D.3's five contexts.
#[test]
fn sign_coding_never_leaves_its_five_contexts() {
    for h in -1..=1 {
        for v in -1..=1 {
            let (ctx, xor) = sign_coding_context(h, v);
            assert!(
                (SIGN_CODING..SIGN_CODING + 5).contains(&ctx),
                "({h}, {v}) gave context {ctx}, outside D.3"
            );
            assert!(xor <= 1, "the XOR bit is a bit");
        }
    }
}

/// Table D.4, magnitude refinement: three rows.
///
/// The `first` flag is whether this is the coefficient's first refinement,
/// and it is the whole table — a decoder that ignores it uses context 16 for
/// everything, which is a *single* wrong context rather than a crash and
/// therefore invisible without this assertion.
#[test]
fn table_d4_magnitude_refinement_entry_by_entry() {
    assert_eq!(
        magnitude_refinement_context(0, true),
        MAGNITUDE_REFINEMENT,
        "first refinement, no significant neighbour"
    );
    assert_eq!(
        magnitude_refinement_context(1, true),
        MAGNITUDE_REFINEMENT + 1,
        "first refinement, at least one significant neighbour"
    );
    assert_eq!(
        magnitude_refinement_context(8, true),
        MAGNITUDE_REFINEMENT + 1,
        "first refinement saturates at one neighbour"
    );
    for neighbours in 0..=8 {
        assert_eq!(
            magnitude_refinement_context(neighbours, false),
            MAGNITUDE_REFINEMENT + 2,
            "after the first refinement the neighbour count stops mattering"
        );
    }
}

/// Table D.7, the initial states — the reason `mq.rs` needed `set_state` at
/// all.
///
/// T.88 starts every context at state 0 with MPS 0, which is what made
/// JBIG2's context numbering a free choice. T.800 does not: the
/// all-insignificant zero-coding context starts at **4**, run-length at
/// **3**, and UNIFORM at **46**. Start them at zero and the first code-block
/// of every stream decodes against the wrong probabilities — and produces
/// coefficients, not an error.
#[test]
fn table_d7_initial_states_entry_by_entry() {
    let contexts = initial_contexts();

    assert_eq!(
        contexts.state(ZERO_CODING),
        Some((4, 0)),
        "D.7: the all-insignificant zero-coding context starts at index 4"
    );
    assert_eq!(
        contexts.state(RUN_LENGTH),
        Some((3, 0)),
        "D.7: run-length starts at index 3"
    );
    assert_eq!(
        contexts.state(UNIFORM),
        Some((46, 0)),
        "D.7: UNIFORM starts at index 46"
    );

    // And everything else starts where T.88 would have started it, which is
    // the standard's own wording: all other contexts are initialised to state
    // 0 with an MPS of 0.
    for ctx in 0..contexts.len() {
        if ctx == ZERO_CODING || ctx == RUN_LENGTH || ctx == UNIFORM {
            continue;
        }
        assert_eq!(
            contexts.state(ctx),
            Some((0, 0)),
            "context {ctx} is not one of D.7's three and must start at zero"
        );
    }
}

/// A reset returns to D.7's states rather than to T.88's zeros.
///
/// This is the defect that would show up on the *second* code-block of every
/// stream and nowhere else: the first one is fine because the contexts were
/// just built, and the picture degrades gradually as a tile progresses, which
/// reads as compression artefacts rather than as a bug.
///
/// The disturbance has to come from *decoding*, not from `set_state`. That
/// method deliberately redefines the baseline -- its own documentation says
/// "the state survives `MqContexts::reset`", which is the whole reason it
/// exists -- so using it to scramble the contexts would only move the target
/// and the test would pass against a `reset` that did nothing at all.
#[test]
fn a_reset_returns_to_the_states_t800_asked_for() {
    let mut contexts = initial_contexts();

    // Decode against the contexts, which is what adapts them. The bytes are
    // arbitrary: what matters is that the adaptive state moves off D.7's
    // numbers before the reset is asked to put it back.
    let data = [0x84u8, 0xC7, 0x3B, 0xFC, 0xE1, 0xA1, 0x43, 0x04, 0x02, 0x20];
    let mut decoder = MqDecoder::new(&data);
    for _ in 0..64 {
        let _ = decoder.decode_at(&mut contexts, ZERO_CODING);
        let _ = decoder.decode_at(&mut contexts, UNIFORM);
    }

    assert_ne!(
        (contexts.state(ZERO_CODING), contexts.state(UNIFORM)),
        (Some((4, 0)), Some((46, 0))),
        "decoding did not move the contexts, so the reset below proves nothing"
    );

    contexts.reset();

    assert_eq!(contexts.state(ZERO_CODING), Some((4, 0)));
    assert_eq!(contexts.state(RUN_LENGTH), Some((3, 0)));
    assert_eq!(contexts.state(UNIFORM), Some((46, 0)));
    assert_eq!(contexts.state(SIGN_CODING), Some((0, 0)));
}

/// A code-block survives the trip out and back.
///
/// This is the *weaker* half of milestone 3's evidence and is written down as
/// such. A round trip through the encoder beside the decoder cannot see a
/// bijective relabelling of the context array — that is what the Annex D
/// tables above are for, and injecting a transposition proves it: two of them
/// fail and this test does not.
///
/// What it does catch is everything the tables cannot: the scan pattern, the
/// stripe order, the pass sequence, the run-length shortcut, sign propagation
/// across a stripe boundary, and the segmentation symbol. A table says what
/// number a context has; only a round trip says the coefficients came back.
#[test]
fn a_code_block_round_trips_through_all_three_passes() {
    use crate::jpx::tier1::{decode_code_block, encoder::encode_code_block};

    // Values chosen so every pass has work: a mixture of signs, one zero
    // (which stays insignificant and exercises the run-length shortcut), and
    // magnitudes spread across the bit-planes so refinement runs more than
    // once.
    let values: Vec<i32> = vec![
        5, -3, 0, 7, //
        -1, 2, 6, 0, //
        0, -7, 1, 3, //
        4, 0, -2, 5,
    ];
    let (w, h) = (4usize, 4usize);
    let planes = 4;

    for orientation in [
        Orientation::Ll,
        Orientation::Hl,
        Orientation::Lh,
        Orientation::Hh,
    ] {
        for segmentation in [false, true] {
            let (data, passes) =
                encode_code_block(&values, w, h, planes, orientation, segmentation);

            let mut contexts = initial_contexts();
            let mut work = u64::MAX;
            let got = decode_code_block(
                &data,
                w as u32,
                h as u32,
                passes,
                orientation,
                segmentation,
                &mut contexts,
                &mut work,
            )
            .unwrap_or_else(|e| panic!("{orientation:?}, segmentation {segmentation}: {e:?}"));

            assert_eq!(
                got.coefficients, values,
                "{orientation:?}, segmentation symbols {segmentation}"
            );
            // A block coded to the bottom knows every coefficient to plane
            // zero, which is what makes E.1.1.2's half a per-block constant
            // for everything except a truncated stream.
            assert!(
                got.half_planes.iter().all(|&h| h == 0),
                "{orientation:?}: a fully coded block should know every                  coefficient to the lowest plane"
            );
        }
    }
}

/// A corrupted code-block with segmentation symbols signalled is caught.
///
/// The symbol is four bits decoded in UNIFORM at the end of every cleanup
/// pass, and it must be `1010`. Checking it is free and it is the one
/// integrity check the format offers at this layer: an MQ decoder that has
/// drifted out of step produces plausible coefficients rather than an error,
/// and this is what notices.
#[test]
fn a_segmentation_symbol_catches_a_decoder_out_of_step() {
    use crate::jpx::tier1::{decode_code_block, encoder::encode_code_block};

    let values: Vec<i32> = vec![5, -3, 0, 7, -1, 2, 6, 0, 0, -7, 1, 3, 4, 0, -2, 5];
    let (data, passes) = encode_code_block(&values, 4, 4, 4, Orientation::Ll, true);

    // Flip a byte in the middle of the codeword segment. The MQ decoder does
    // not fail on this -- it cannot, every byte is a legal codeword -- it just
    // starts producing different decisions from that point.
    let mut corrupted = data.clone();
    let at = corrupted.len() / 2;
    corrupted[at] ^= 0x5A;

    let mut contexts = initial_contexts();
    let mut work = u64::MAX;
    let got = decode_code_block(
        &corrupted,
        4,
        4,
        passes,
        Orientation::Ll,
        true,
        &mut contexts,
        &mut work,
    );

    match got {
        Err(_) => {}
        Ok(decoded) => assert_ne!(
            decoded.coefficients, values,
            "a corrupted block decoded to the original coefficients, which \
             means the corruption was in a byte nothing read"
        ),
    }
}

/// A **truncated** code-block knows its coefficients to different depths, and
/// tier-1 records which.
///
/// This is the other half of the pass-count finding, and it is the one that
/// changes pixels rather than panicking. E.1.1.2 reconstructs a coefficient
/// at the midpoint of the interval a decoder knows it lies in, and on a
/// stream that stops part way down a plane that interval is **not** the same
/// for every coefficient of the block: one that became significant in the
/// last cleanup pass and never met a refinement pass is known only to that
/// plane, where its neighbours are known a plane further down.
///
/// Reconstructing them all at the block's lowest plane puts the first kind a
/// quarter of its own magnitude low — a soft dark patch on an otherwise
/// correct picture, which is this codec's signature failure. `opj_decompress`
/// disagreed by twenty levels of 255 on a three-component fixture truncated
/// to a twentieth, and nothing else in the repository did.
#[test]
fn a_truncated_code_block_records_the_depth_each_coefficient_reached() {
    use crate::jpx::tier1::{decode_code_block, encoder::encode_code_block};

    let values: Vec<i32> = vec![5, -3, 0, 7, -1, 2, 6, 0, 0, -7, 1, 3, 4, 0, -2, 5];
    let planes = 4;
    let (data, passes) = encode_code_block(&values, 4, 4, planes, Orientation::Ll, false);

    // Every pass count the encoder's stream can be cut to, including the ones
    // that do not land on a plane boundary -- which is the case a rate
    // allocator produces and the in-tree encoder never does.
    for cut in 1..=passes {
        let mut contexts = initial_contexts();
        let mut work = u64::MAX;
        let got = decode_code_block(
            &data,
            4,
            4,
            cut,
            Orientation::Ll,
            false,
            &mut contexts,
            &mut work,
        )
        .unwrap_or_else(|e| panic!("{cut} passes: {e:?}"));

        let reached = (cut - 1).div_ceil(3) + 1;
        assert_eq!(got.planes, reached, "{cut} passes");
        for (i, &h) in got.half_planes.iter().enumerate() {
            assert!(
                u32::from(h) < reached,
                "{cut} passes: coefficient {i} claims plane {h} of {reached}"
            );
        }
        // Whether the last pass narrowed every coefficient or only some is
        // the whole point: after a cleanup pass the block is uniform, and
        // after a significance pass it is not.
        if cut == passes {
            assert!(
                got.half_planes.iter().all(|&h| h == 0),
                "a complete block knows everything to plane zero"
            );
        }
    }

    // And the shape the oracle caught: somewhere in that range there is a cut
    // whose significant coefficients are known to **different** depths,
    // because a significance pass narrows the coefficients it takes and
    // leaves the ones already significant where they were. If no cut produced
    // one, the per-coefficient record would be a per-block constant and the
    // whole mechanism could be replaced by `planes`.
    let mut uneven = 0;
    for cut in 1..=passes {
        let mut contexts = initial_contexts();
        let mut work = u64::MAX;
        let got = decode_code_block(
            &data,
            4,
            4,
            cut,
            Orientation::Ll,
            false,
            &mut contexts,
            &mut work,
        )
        .expect("a truncated decode");
        let depths: Vec<u8> = got
            .coefficients
            .iter()
            .zip(&got.half_planes)
            .filter(|(v, _)| **v != 0)
            .map(|(_, &h)| h)
            .collect();
        if depths.len() > 1 && depths.iter().any(|&h| h != depths[0]) {
            uneven += 1;
        }
    }
    assert!(
        uneven > 0,
        "no truncation produced coefficients known to different depths, so \
         the per-coefficient record is not measuring anything"
    );
}

/// D.2's pass sequence, walked at **every** legal pass count rather than at
/// the ones the in-tree encoder happens to write.
///
/// This is the test milestone 6 had to add, and what it is worth saying is
/// why nothing before it existed. The plane count was derived as
/// `passes.div_ceil(3)`, which is right exactly when a code-block ends on a
/// cleanup pass — `passes = 1, 4, 7, ...` — and the encoder in `writer` emits
/// `3 * planes - 2` passes by construction, so **every round trip in this
/// crate took the one case that worked.** A tier-2 stream from
/// `opj_compress` does not: a lossy code-block stops wherever the rate
/// allocator put it, and one that stops on a significance or refinement pass
/// asked `pass_at` for a plane below zero.
///
/// So the assertion is not about a pass sequence being plausible. It is that
/// the first pass is a cleanup on the top plane, that the planes descend by
/// one every three passes and never below zero, and that the count of planes
/// touched is the count the dequantiser is told — because that number is what
/// E.1 multiplies a truncated code-block back up by, and a plane out there is
/// a coefficient out by a factor of two.
#[test]
fn the_pass_sequence_covers_every_plane_at_every_pass_count() {
    for passes in 1..=MAX_PASSES {
        let planes = (passes - 1).div_ceil(3) + 1;
        assert!(
            planes <= 31,
            "{passes} passes claims {planes} planes, past what an i32 holds"
        );
        let mut seen = std::collections::BTreeSet::new();
        let mut last = u32::MAX;
        for i in 0..passes {
            let (pass, plane) = pass_at(i, planes);
            assert!(
                plane < planes,
                "pass {i} of {passes} asked for plane {plane} of {planes}"
            );
            if i == 0 {
                assert_eq!(pass, Pass::Cleanup, "the first pass is a cleanup");
                assert_eq!(plane, planes - 1, "on the most significant plane");
            }
            assert!(
                plane <= last,
                "pass {i} of {passes} went back up to plane {plane}"
            );
            last = plane;
            seen.insert(plane);
        }
        assert_eq!(
            seen.len() as u32,
            planes,
            "{passes} passes touched {} planes but the dequantiser is told \
             {planes}, and E.1 scales a truncated code-block by the \
             difference",
            seen.len()
        );
        // The lowest plane reached is zero exactly when the block was coded
        // to the bottom, which is what `planes` claims by definition.
        assert_eq!(last, 0, "{passes} passes did not reach the lowest plane");
    }
}
