//! 9.10's output formatting.
//!
//! # What this file is for, given the lossless identity exists
//!
//! `tests/jxr_fixtures.rs` compares whole pictures against rasters this
//! repository authored, and it covers this stage completely: a wrong colour
//! transform, a wrong bias or a wrong shift fails it on every fixture. These
//! tests are here for the two things that check cannot do.
//!
//! **They name the stage.** An identity failure says "the picture is wrong";
//! it does not say whether the entropy decoder, the transform or the colour
//! conversion was at fault. A failure here says which.
//!
//! **They pin the exactness claim directly.** 9.10.4.3's inverse is three
//! integer lifting steps, so it is a *bijection* — the colour conversion
//! loses nothing, and the identity's bit-exactness depends on that being
//! true rather than nearly true.
//!
//! # Counted injections
//!
//! | Injection | Round-trip vectors that fail (of 4096) |
//! | --- | ---: |
//! | `Ceiling(V / 2)` written as `Floor(V / 2)` | **2048** |
//! | `Floor(t / 2)` written as `Ceiling(t / 2)` | **2048** |
//! | `t = -U` written as `t = U` | **3840** |
//!
//! Two of the three fire on exactly half the vectors, which is what a
//! rounding difference does: it changes the result only when the operand is
//! odd. Half is decisive for a test and invisible in a picture — a decoder
//! with either defect returns an image that is off by one in half its
//! samples, which no reviewer would see and which the lossless identity
//! catches immediately. That contrast is the argument for having the
//! identity at all.

use super::*;

/// The forward of [`inv_colour_convert2`], derived by reversing its three
/// lifting steps. Nothing in the decoder needs it; it exists so the claim
/// that the conversion is a bijection can be checked rather than asserted.
fn fwd_colour_convert2(r: i32, g: i32, b: i32) -> (i32, i32, i32) {
    let v = b.wrapping_sub(r);
    let t = r.wrapping_sub(g).wrapping_add((v.wrapping_add(1)) >> 1);
    let y = g.wrapping_add(t >> 1);
    let u = t.wrapping_neg();
    (y, u, v)
}

/// Every combination of a small signed range, which is where the rounding of
/// the two halving steps actually differs.
fn probes() -> impl Iterator<Item = (i32, i32, i32)> {
    (-8..8).flat_map(|r| (-8..8).flat_map(move |g| (-8..8).map(move |b| (r, g, b))))
}

#[test]
fn the_colour_transform_is_a_bijection() {
    // 9.10.4.3 is three integer lifting steps, so an RGB image converted to
    // the internal format comes back **bit-exact**. That is what lets the
    // lossless identity be a total check rather than a tolerance.
    let mut checked = 0;
    for (r, g, b) in probes() {
        let (y, u, v) = fwd_colour_convert2(r, g, b);
        assert_eq!(
            inv_colour_convert2(y, u, v),
            (r, g, b),
            "({r}, {g}, {b}) did not survive the round trip"
        );
        checked += 1;
    }
    assert_eq!(checked, 4096);
    // And a grey input stays grey: U and V are zero for R == G == B, which is
    // the property that makes YONLY and YUV444 agree on a greyscale image.
    for v in [-100i32, -1, 0, 1, 100] {
        assert_eq!(fwd_colour_convert2(v, v, v), (v, 0, 0));
    }
}

/// How many of [`probes`] fail to round-trip through `inverse`.
fn round_trip_failures(inverse: impl Fn(i32, i32, i32) -> (i32, i32, i32)) -> usize {
    probes()
        .filter(|&(r, g, b)| {
            let (y, u, v) = fwd_colour_convert2(r, g, b);
            inverse(y, u, v) != (r, g, b)
        })
        .count()
}

#[test]
fn rounding_the_chroma_halving_the_other_way_fails_half_the_vectors() {
    // 9.10.4.3 uses `Floor(t / 2)` for one step and `Ceiling(V / 2)` for the
    // other. They are not interchangeable, and each is wrong only on odd
    // operands — so a decoder with either defect is off by one in half its
    // samples and looks perfectly fine.
    let ceiling_as_floor = |y: i32, u: i32, v: i32| {
        let t = u.wrapping_neg();
        let g = y.wrapping_sub(t >> 1);
        let r = t.wrapping_add(g).wrapping_sub(v >> 1);
        (r, g, v.wrapping_add(r))
    };
    assert_eq!(round_trip_failures(ceiling_as_floor), 2048);

    let floor_as_ceiling = |y: i32, u: i32, v: i32| {
        let t = u.wrapping_neg();
        let g = y.wrapping_sub((t.wrapping_add(1)) >> 1);
        let r = t.wrapping_add(g).wrapping_sub((v.wrapping_add(1)) >> 1);
        (r, g, v.wrapping_add(r))
    };
    assert_eq!(round_trip_failures(floor_as_ceiling), 2048);
}

#[test]
fn dropping_the_negation_of_u_fails_almost_every_vector() {
    // `tempT = −ImagePlane[1][x][y]` is the first line of 9.10.4.3, and the
    // minus sign is easy to lose because every later use of `t` is additive.
    let no_negation = |y: i32, u: i32, v: i32| {
        let t = u;
        let g = y.wrapping_sub(t >> 1);
        let r = t.wrapping_add(g).wrapping_sub((v.wrapping_add(1)) >> 1);
        (r, g, v.wrapping_add(r))
    };
    // Not all of them: `u == 0` is its own negation, and the probe range
    // includes it.
    assert_eq!(round_trip_failures(no_negation), 3840);
}

// --- 9.10.5 and 9.10.6 ---------------------------------------------------

#[test]
fn the_bias_and_the_scaling_are_9_10_5s_and_9_10_6s() {
    use crate::jxr::headers::OutputBitdepth;

    // 9.10.5: the bias is half the output range, and it is pre-shifted by
    // `iScale` so that 9.10.6's right shift undoes both together.
    assert_eq!(add_bias_amount(OutputBitdepth::Bd8, 0, false), 128);
    assert_eq!(add_bias_amount(OutputBitdepth::Bd8, 0, true), 128 << 3);
    assert_eq!(add_bias_amount(OutputBitdepth::Bd16, 0, false), 1 << 15);
    assert_eq!(
        add_bias_amount(OutputBitdepth::Bd16, 0, true),
        (1 << 15) << 3
    );
    // `SHIFT_BITS` narrows the bias, and only for the depths 8.4.13 reads it
    // for. 9.10.7.2 widens the sample again afterwards, so the two are not a
    // matched pair that cancels.
    assert_eq!(add_bias_amount(OutputBitdepth::Bd16, 4, false), 1 << 11);
    assert_eq!(add_bias_amount(OutputBitdepth::Bd8, 4, false), 128);

    // 9.10.6: no scaling at all unless SCALED_FLAG, and the rounding term is
    // 4 for BD16 against 3 for BD8 — the clause's own asymmetry.
    assert_eq!(compute_scaling(OutputBitdepth::Bd8, false), (0, 0));
    assert_eq!(compute_scaling(OutputBitdepth::Bd16, false), (0, 0));
    assert_eq!(compute_scaling(OutputBitdepth::Bd8, true), (3, 3));
    assert_eq!(compute_scaling(OutputBitdepth::Bd16, true), (3, 4));
}

#[test]
fn the_bias_and_scaling_invert_an_exact_encode() {
    use crate::jxr::headers::OutputBitdepth;

    // The two stages together are the inverse of the encoder's "subtract the
    // bias and scale up", so a sample that was scaled exactly comes back
    // exactly. This is the arithmetic the lossless identity depends on, and
    // it is checked here across the whole output range rather than only at
    // the values a fixture happens to contain.
    for depth in [OutputBitdepth::Bd8, OutputBitdepth::Bd16] {
        let high = if matches!(depth, OutputBitdepth::Bd16) {
            65_535
        } else {
            255
        };
        for scaled in [false, true] {
            let bias = add_bias_amount(depth, 0, scaled);
            let (scale, rounding) = compute_scaling(depth, scaled);
            for v in [0i32, 1, 2, 127, 128, high / 2, high - 1, high] {
                // What the encoder held: the sample less its bias, scaled up.
                let internal = (v - (bias >> scale)) << scale;
                let out = (internal + bias + rounding) >> scale;
                assert_eq!(out, v, "{depth:?} scaled={scaled} at {v}");
            }
        }
    }
}

#[test]
fn clipping_is_the_output_range_and_nothing_wider() {
    // 9.10.8.2. The clip is the last thing standing between a wrong
    // coefficient and a wrapped byte, so it is checked at both ends and one
    // past them.
    assert_eq!(clipping_basic(-1, 8), 0);
    assert_eq!(clipping_basic(0, 8), 0);
    assert_eq!(clipping_basic(255, 8), 255);
    assert_eq!(clipping_basic(256, 8), 255);
    assert_eq!(clipping_basic(i32::MIN, 8), 0);
    assert_eq!(clipping_basic(i32::MAX, 8), 255);
    assert_eq!(clipping_basic(-1, 16), 0);
    assert_eq!(clipping_basic(65_535, 16), 65_535);
    assert_eq!(clipping_basic(65_536, 16), 65_535);
    assert_eq!(clipping_basic(i32::MAX, 16), 65_535);
}

#[test]
fn every_channel_order_is_a_permutation_of_the_planes_it_names() {
    // Table A.6's rows differ only in channel order, and getting one wrong
    // returns a picture with red and blue swapped — which is obvious in a
    // photograph and completely invisible in a greyscale one.
    for channels in [
        JxrChannels::Gray,
        JxrChannels::Rgb,
        JxrChannels::Bgr,
        JxrChannels::Bgra,
        JxrChannels::Rgba,
    ] {
        let order = channel_order(channels);
        assert_eq!(
            order.len(),
            usize::from(channels.count()),
            "{channels:?} names the wrong number of channels"
        );
        let colour: Vec<usize> = order
            .iter()
            .copied()
            .filter(|&p| p != ALPHA_CHANNEL)
            .collect();
        let mut sorted = colour.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), colour.len(), "{channels:?} repeats a plane");
        assert!(
            sorted.iter().all(|&p| p < 3),
            "{channels:?} names a plane that does not exist"
        );
        assert_eq!(
            order.contains(&ALPHA_CHANNEL),
            channels.has_alpha(),
            "{channels:?} disagrees with has_alpha"
        );
    }
}
