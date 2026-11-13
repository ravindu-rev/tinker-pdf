//! F.3.3's interleave at an odd origin, held to the standard's own
//! equations worked by hand.
//!
//! A resolution's grid starts wherever B-14 puts it, and F.3.3 places each
//! subband by **absolute** position on it: a low-pass sample `u` at `2u`, a
//! high-pass one at `2u + 1`. So at an odd origin the first sample of the
//! plane is a high-pass one, and F.3.8.1's two lifting steps run with the
//! parities the other way round. Every committed codestream starts its grid
//! at zero, where relative and absolute parity agree, so none of them could
//! tell a decoder that placed the bands by their index within the plane from
//! one that placed them by absolute position — and at an odd origin the first
//! puts a low-pass sample where the lifting reads a high-pass one, and a
//! resolution of five samples, two low and three high, loses its last
//! high-pass sample off the end of the line.
//!
//! Each expected value below is computed here from F.3.8.1 — the even step
//! `X(2n) = Y(2n) - floor((Y(2n-1) + Y(2n+1) + 2) / 4)`, then the odd step
//! `X(2n+1) = Y(2n+1) + floor((X(2n) + X(2n+2)) / 2)` — with F.3.7's
//! symmetric extension about the first and last samples, and G.1's level
//! shift of 128, never by the decoder. The codestreams are the test writer's
//! (`writer::coded`), reversible, one level, with the coefficients named.

use super::writer::{coded, At, Spec};
use crate::jpx::jpx_decode;
use crate::jpx::tier2::Orientation;
use crate::Limits;

/// `a` divided by `b` rounded down: F.3.8.1's `floor`.
fn floor_div(a: i32, b: i32) -> i32 {
    a.div_euclid(b)
}

/// F.3.8.1 over the four samples at absolute positions 1 to 4, the first an
/// odd (high-pass) one: `h0` at 1, `a` at 2, `h1` at 3, `b` at 4.
///
/// The even step at 4 reads position 5, which F.3.7 reflects onto 3; the odd
/// step at 1 reads position 0, which it reflects onto 2.
fn four_from_one(h0: i32, a: i32, h1: i32, b: i32) -> [i32; 4] {
    let x2 = a - floor_div(h0 + h1 + 2, 4);
    let x4 = b - floor_div(h1 + h1 + 2, 4);
    let x1 = h0 + floor_div(x2 + x2, 2);
    let x3 = h1 + floor_div(x2 + x4, 2);
    [x1, x2, x3, x4]
}

/// F.3.8.1 over the two samples at absolute positions 1 and 2: `h` at 1, `l`
/// at 2. The even step at 2 reads 1 and 3, and F.3.7 reflects 3 onto 1; the
/// odd step at 1 reads 0 and 2, and 0 reflects onto 2.
fn two_from_one(h: i32, l: i32) -> [i32; 2] {
    let x2 = l - floor_div(h + h + 2, 4);
    let x1 = h + floor_div(x2 + x2, 2);
    [x1, x2]
}

/// One component of eight unsigned bits, reversible, one level, no
/// quantisation, one tile over the image whose area is `[x0, x1) x [y0, y1)`.
fn one_level(x0: u32, y0: u32, x1: u32, y1: u32) -> Spec {
    Spec {
        xsiz: x1,
        ysiz: y1,
        xosiz: x0,
        yosiz: y0,
        xtsiz: x1,
        ytsiz: y1,
        xtosiz: 0,
        ytosiz: 0,
        levels: 1,
        transform: 1,
        quant_style: 0,
        ..Spec::default()
    }
}

/// The decoded samples of `spec` written with `value`'s coefficients.
fn decoded(spec: &Spec, value: &dyn Fn(At) -> i32) -> Vec<u8> {
    let mut warnings = Vec::new();
    let image = jpx_decode(&coded(spec, value), &Limits::new(1 << 20), &mut warnings)
        .unwrap_or_else(|e| panic!("it decodes: {e:?} {warnings:?}"));
    assert!(warnings.is_empty(), "{warnings:?}");
    image.samples
}

/// G.1's shift onto eight unsigned bits; every value here stays inside them.
fn shifted(values: &[i32]) -> Vec<u8> {
    values
        .iter()
        .map(|&v| u8::try_from(v + 128).expect("a value inside eight bits"))
        .collect()
}

/// A row of four starting at column 1: B-15 puts LL's samples 1 and 2 at
/// columns 2 and 4 and HL's samples 0 and 1 at columns 1 and 3.
#[test]
fn a_row_starting_at_an_odd_column_is_interleaved_by_absolute_position() {
    let (a, b, h0, h1) = (20, -7, 5, -9);
    let value = move |at: At| match (at.resolution, at.orientation, at.x) {
        (0, Orientation::Ll, 1) => a,
        (0, Orientation::Ll, 2) => b,
        (1, Orientation::Hl, 0) => h0,
        (1, Orientation::Hl, 1) => h1,
        _ => 0,
    };
    assert_eq!(
        decoded(&one_level(1, 0, 5, 1), &value),
        shifted(&four_from_one(h0, a, h1, b))
    );
}

/// The same down a column starting at row 1, through LH.
#[test]
fn a_column_starting_at_an_odd_row_is_interleaved_by_absolute_position() {
    let (a, b, h0, h1) = (-31, 12, -6, 11);
    let value = move |at: At| match (at.resolution, at.orientation, at.y) {
        (0, Orientation::Ll, 1) => a,
        (0, Orientation::Ll, 2) => b,
        (1, Orientation::Lh, 0) => h0,
        (1, Orientation::Lh, 1) => h1,
        _ => 0,
    };
    assert_eq!(
        decoded(&one_level(0, 1, 1, 5), &value),
        shifted(&four_from_one(h0, a, h1, b))
    );
}

/// A 2 x 2 image at `(1, 1)`: one sample of each subband, at the corner
/// F.3.3 gives it — HH at `(1, 1)`, LH at `(2, 1)`, HL at `(1, 2)` and LL at
/// `(2, 2)` — synthesised along the rows and then down the columns
/// (F.3.4).
#[test]
fn a_plane_starting_at_an_odd_corner_puts_each_band_at_its_own_corner() {
    let (ll, hl, lh, hh) = (40, -13, 9, -22);
    let value = move |at: At| match (at.resolution, at.orientation) {
        (0, Orientation::Ll) => ll,
        (1, Orientation::Hl) => hl,
        (1, Orientation::Lh) => lh,
        (1, Orientation::Hh) => hh,
        _ => 0,
    };
    // Row 1 holds HH then LH, row 2 HL then LL.
    let row1 = two_from_one(hh, lh);
    let row2 = two_from_one(hl, ll);
    // Column 1 holds row 1's first sample above row 2's, column 2 the same.
    let column1 = two_from_one(row1[0], row2[0]);
    let column2 = two_from_one(row1[1], row2[1]);
    assert_eq!(
        decoded(&one_level(1, 1, 3, 3), &value),
        shifted(&[column1[0], column2[0], column1[1], column2[1]])
    );
}

/// A resolution of five samples starting at column 3 holds two low-pass and
/// three high-pass ones (B-15), and every one of them reaches the picture:
/// changing any single coefficient changes the decode.
#[test]
fn every_band_sample_of_an_odd_origin_reaches_the_picture() {
    let spec = one_level(3, 0, 8, 1);
    let base = |at: At| match (at.resolution, at.orientation) {
        (0, Orientation::Ll) => 10 + at.x as i32,
        (1, Orientation::Hl) => 3 - at.x as i32,
        _ => 0,
    };
    let before = decoded(&spec, &base);
    // LL's samples are 2 and 3, HL's 1, 2 and 3.
    for (orientation, x) in [
        (Orientation::Ll, 2),
        (Orientation::Ll, 3),
        (Orientation::Hl, 1),
        (Orientation::Hl, 2),
        (Orientation::Hl, 3),
    ] {
        let changed = move |at: At| {
            let v = base(at);
            if at.resolution == usize::from(orientation != Orientation::Ll)
                && at.orientation == orientation
                && at.x == x
            {
                v + 40
            } else {
                v
            }
        };
        assert_ne!(
            decoded(&spec, &changed),
            before,
            "{orientation:?} sample {x} reaches no sample of the picture"
        );
    }
}
