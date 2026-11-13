//! The output direction (`docs/design/icc.md`, "The output transform"): a
//! profile's transform from the connection space to its device components,
//! compiled for a rendering intent.
//!
//! The transform comes from the first source ICC.1 8.10.2's rows name for the
//! intent with `B` for `A` ([`Intent::order`]): the intent's own `B2A*` table,
//! then `B2A0`, then the matrix/TRC or grey model inverted, then the other
//! `B2A*` tables, each passed over where it does not read.
//!
//! A `B2A*` table — `mft1` or `mft2` — takes the connection space as its
//! input: the value is encoded on the way in as the table's type defines it
//! (`u1Fixed15` XYZ through the table's matrix, or Lab in the table's own
//! scale), then runs through the input curves, the grid and the output
//! curves, out to the data space's channels.
//!
//! A matrix/TRC model is inverted when the transform is built: the inverse of
//! its colorant columns, computed in `f64` and held in s15.16, and each tone
//! curve inverted by bisection over its compiled table into
//! [`CURVE_ENTRIES`] entries; a grey model's `grayTRC` likewise. Per colour
//! the evaluation is integer: the connection-space value is quantized once on
//! the way in, and what follows is a fixed-point matrix multiply and table
//! reads, so nothing on the path rounds differently on another target
//! (ruling 4). ICC-absolute colorimetry divides the value by the profile's
//! media white over the PCS white before it enters (ICC.1 6.2.3), the
//! division folded into the inverse matrix, or into the grey curve's scale,
//! and made in XYZ before a table.
//!
//! A matrix whose determinant is below one s15.16 step, or a curve that
//! decreases anywhere or never rises, has no inverse, and the transform is
//! refused as [`IccError::NoInverse`] (ruling 1).

use std::sync::Arc;

use super::{
    compile_curve, fixed, round_shift, Curve, IccError, Intent, Lut, Model, Profile, Step,
    CURVE_ENTRIES, ONE,
};

/// The most device channels a transform out of the connection space gives:
/// ICC.1 7.2.6's `FCLR`, fifteen, the most channels a data colour space has.
/// A `B2A*` table's output count, a byte, is held to it before any of its
/// curves is read.
pub const MAX_DEVICE_CHANNELS: usize = 15;

/// The largest magnitude a connection-space component is taken at on the way
/// in: twice what `u1Fixed15`, the most an XYZ table encodes, can hold, so a
/// value the format can state passes untouched and a product with an s15.16
/// matrix entry stays far inside `i64`.
const PCS_LIMIT: f64 = 4.0;

/// A device colour: a profile's data-space channels, each from 0 to 65 535,
/// at most [`MAX_DEVICE_CHANNELS`] of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceColour {
    values: [u16; MAX_DEVICE_CHANNELS],
    count: usize,
}

impl DeviceColour {
    /// The colour of `values`, as many channels as it holds up to
    /// [`MAX_DEVICE_CHANNELS`].
    fn of(values: &[u16]) -> DeviceColour {
        let mut colour = DeviceColour {
            values: [0; MAX_DEVICE_CHANNELS],
            count: values.len().min(MAX_DEVICE_CHANNELS),
        };
        for (slot, value) in colour.values.iter_mut().zip(values) {
            *slot = *value;
        }
        colour
    }

    /// The channels, in the order the data colour space lists them.
    #[must_use]
    pub fn channels(&self) -> &[u16] {
        self.values.get(..self.count).unwrap_or(&[])
    }
}

/// A profile's transform from the connection space to its device
/// components, for one rendering intent.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    inverse: Inverse,
}

/// What an [`Output`] runs a connection-space value through.
#[derive(Clone, PartialEq)]
enum Inverse {
    /// A matrix/TRC model inverted: the columns' inverse in s15.16, ICC
    /// -absolute colorimetry's division by the media scale folded into it,
    /// then each channel's inverted curve.
    Matrix {
        inverse: [[i32; 3]; 3],
        curves: [Box<[u16]>; 3],
    },
    /// A grey model inverted: the luminance — `Y` under an XYZ connection
    /// space, `L*` under Lab — through the inverted `grayTRC`.
    Grey {
        /// Whether the connection space is `Lab` rather than `XYZ`.
        lab: bool,
        /// ICC-absolute colorimetry's media scale for `Y`, where it is not
        /// one: the value is `Y` over it.
        media: Option<f64>,
        /// Under XYZ, the s15.16 factor `Y` is multiplied by on the way in:
        /// one, or one over the media scale.
        factor: i64,
        curve: Box<[u16]>,
    },
    /// A `B2A*` table, the connection-space value divided by `media` in XYZ
    /// first where that is given: ICC-absolute colorimetry's scale.
    Table {
        lut: Arc<Lut>,
        media: Option<[f64; 3]>,
    },
}

/// The model and its matrix, or the table's shape, and not the 4 096 entries
/// of each inverted curve or a table's grid, which say nothing a reader
/// needs.
impl std::fmt::Debug for Inverse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Inverse::Matrix { inverse, .. } => f
                .debug_struct("Matrix")
                .field("inverse", inverse)
                .finish_non_exhaustive(),
            Inverse::Grey { lab, media, .. } => f
                .debug_struct("Grey")
                .field("lab", lab)
                .field("media", media)
                .finish_non_exhaustive(),
            Inverse::Table { lut, media } => f
                .debug_struct("Table")
                .field("outputs", &lut.outputs())
                .field("lab", &lut.lab)
                .field("media", media)
                .finish_non_exhaustive(),
        }
    }
}

impl Output {
    /// The profile's transform out of the connection space for `intent`,
    /// taken by ICC.1 8.10.2's rows with `B` for `A`
    /// (`docs/design/icc.md`, "The output transform"): the first of the
    /// intent's own `B2A*` table, `B2A0`, the matrix/TRC or grey model
    /// inverted, and the other `B2A*` tables that the profile carries and
    /// that reads — `B2A1`, `B2A0`, the model and `B2A2` for relative
    /// colorimetric, which absolute colorimetric takes too with the media
    /// white divided out (6.2.3). A table that does not read, or a model with
    /// no inverse, is passed over to the next.
    ///
    /// # Errors
    ///
    /// Where nothing reads, the first reason one did not: among them
    /// [`IccError::NoInverse`] for a model whose matrix's determinant is below
    /// one s15.16 step or one of whose curves decreases anywhere or never
    /// rises, [`IccError::TooLarge`] for a table giving more than
    /// [`MAX_DEVICE_CHANNELS`] channels and [`IccError::UnsupportedSpace`] for
    /// one giving another count than the data space has; and
    /// [`IccError::MissingTags`] for a profile carrying neither a `B2A*`
    /// table nor a model.
    pub fn compile(profile: &Profile, intent: Intent) -> Result<Output, IccError> {
        let media = (intent == Intent::AbsoluteColorimetric)
            .then(|| profile.media_scale())
            .filter(|scale| *scale != [1.0; 3]);
        let mut passed_over = None;
        for step in intent.order() {
            let built = match step {
                // ICC.1 8.10.2 a)'s `BToD*` tags are not read, so a profile
                // carrying one converts through its `B2A*` tables and model.
                Step::Pipeline(_) => continue,
                Step::Table(tag) => {
                    let Some((_, element)) = profile.tables.output_at(tag) else {
                        continue;
                    };
                    match profile.channels() {
                        Some(channels) => profile
                            .tables
                            .read_output(element, usize::from(channels))
                            .map(|lut| Inverse::Table { lut, media }),
                        None => Err(IccError::UnsupportedSpace),
                    }
                }
                Step::Model => match &profile.model {
                    Some(model) => invert_model(model, media),
                    None => continue,
                },
            };
            match built {
                Ok(inverse) => return Ok(Output { inverse }),
                Err(error) => {
                    passed_over.get_or_insert(error);
                }
            }
        }
        Err(passed_over.unwrap_or(IccError::MissingTags))
    }

    /// How many device channels the transform gives: three for a matrix/TRC
    /// profile, one for a grey one, and a `B2A*` table's own count, the data
    /// space's.
    #[must_use]
    pub fn channels(&self) -> usize {
        match &self.inverse {
            Inverse::Matrix { .. } => 3,
            Inverse::Grey { .. } => 1,
            Inverse::Table { lut, .. } => lut.outputs(),
        }
    }

    /// The device colour `pcs` is, a connection-space value in the profile's
    /// own connection space as real numbers: D50 XYZ with the PCS white's `Y`
    /// at one, or `L*`, `a*` and `b*` as they stand. A component that is not
    /// a number is taken as zero, and one past four as four; light the
    /// inverse matrix takes below zero or past one is held there, and a
    /// table's encoded input within its currency.
    #[must_use]
    pub fn apply(&self, pcs: [f64; 3]) -> DeviceColour {
        match &self.inverse {
            Inverse::Matrix { inverse, curves } => {
                let xyz = pcs.map(quantize);
                let mut out = [0u16; 3];
                for ((slot, row), curve) in out.iter_mut().zip(inverse).zip(curves) {
                    let sum: i64 = row
                        .iter()
                        .zip(xyz)
                        .map(|(entry, value)| i64::from(*entry) * value)
                        .sum();
                    *slot = through(curve, round_shift(sum, 16));
                }
                DeviceColour::of(&out)
            }
            Inverse::Grey {
                lab,
                media,
                factor,
                curve,
            } => {
                let light = if *lab {
                    // 6.3.4.2: `L*` over 0..100 is the curve's output. Under
                    // ICC-absolute colorimetry the media white divides `Y`,
                    // so the lightness is found again from it (6.2.3).
                    let lightness = match media {
                        None => pcs[0],
                        Some(scale) => {
                            let [_, y, _] = lab_to_xyz(pcs);
                            xyz_to_lab([0.0, y / scale, 0.0])[0]
                        }
                    };
                    quantize(lightness / 100.0)
                } else {
                    round_shift(quantize(pcs[1]) * factor, 16)
                };
                DeviceColour::of(&[through(curve, light)])
            }
            Inverse::Table { lut, media } => {
                let pcs = match media {
                    Some(scale) => scale_connection(pcs, lut.lab, scale.map(|s| 1.0 / s)),
                    None => pcs,
                };
                let values = lut.evaluate(&lut.encode_connection(pcs));
                let mut out = [0u16; MAX_DEVICE_CHANNELS];
                for (slot, value) in out.iter_mut().zip(values) {
                    *slot = (value.clamp(0.0, 1.0) * 65535.0).round() as u16;
                }
                DeviceColour::of(out.get(..lut.outputs()).unwrap_or(&[]))
            }
        }
    }
}

/// A matrix/TRC or grey model inverted, its connection-space value divided
/// by `media` first where that is given.
fn invert_model(model: &Model, media: Option<[f64; 3]>) -> Result<Inverse, IccError> {
    Ok(match model {
        Model::MatrixTrc { columns, curves } => Inverse::Matrix {
            inverse: invert_columns(columns, media.unwrap_or([1.0; 3]))?,
            curves: [
                invert_curve(&curves[0])?,
                invert_curve(&curves[1])?,
                invert_curve(&curves[2])?,
            ],
        },
        Model::Grey { lab, curve } => {
            let luminance = media.map(|scale| scale[1]);
            Inverse::Grey {
                lab: *lab,
                media: luminance,
                factor: i64::from(fixed(1.0 / luminance.unwrap_or(1.0))),
                curve: invert_curve(curve)?,
            }
        }
    })
}

/// `pcs`, a connection-space value in its own units, scaled channel by
/// channel in XYZ by `scale` — ICC-absolute colorimetry's media white, or its
/// inverse (6.2.3) — under Lab through XYZ and back (6.4).
pub(super) fn scale_connection(pcs: [f64; 3], lab: bool, scale: [f64; 3]) -> [f64; 3] {
    if lab {
        let xyz = lab_to_xyz(pcs);
        xyz_to_lab([xyz[0] * scale[0], xyz[1] * scale[1], xyz[2] * scale[2]])
    } else {
        [pcs[0] * scale[0], pcs[1] * scale[1], pcs[2] * scale[2]]
    }
}

/// A connection-space component in s15.16: zero for one that is not a
/// number, and held within [`PCS_LIMIT`].
fn quantize(value: f64) -> i64 {
    if value.is_nan() {
        return 0;
    }
    // Within ±4, so the product with 65 536 is exact and fits.
    (value.clamp(-PCS_LIMIT, PCS_LIMIT) * ONE as f64).round() as i64
}

/// The inverse of a matrix/TRC model's columns — `columns[c]` the `[X, Y, Z]`
/// of colorant `c`, so the matrix that takes linear device values to XYZ has
/// them as its columns — with each XYZ component divided by `media`'s on the
/// way in, held in s15.16: by its cofactors over its determinant, computed in
/// `f64`. [`IccError::NoInverse`] where the determinant is below one s15.16
/// step or is not a number.
fn invert_columns(columns: &[[f64; 3]; 3], media: [f64; 3]) -> Result<[[i32; 3]; 3], IccError> {
    let m = |row: usize, column: usize| columns[column][row];
    let cofactor = |row: usize, column: usize| {
        let (r0, r1) = ((row + 1) % 3, (row + 2) % 3);
        let (c0, c1) = ((column + 1) % 3, (column + 2) % 3);
        m(r0, c0) * m(r1, c1) - m(r0, c1) * m(r1, c0)
    };
    let determinant =
        m(0, 0) * cofactor(0, 0) + m(0, 1) * cofactor(0, 1) + m(0, 2) * cofactor(0, 2);
    if !determinant.is_finite() || determinant.abs() < 1.0 / ONE as f64 {
        return Err(IccError::NoInverse);
    }
    let mut inverse = [[0i32; 3]; 3];
    for (row, out) in inverse.iter_mut().enumerate() {
        for (column, slot) in out.iter_mut().enumerate() {
            // The adjugate is the cofactors transposed.
            let entry = cofactor(column, row) / determinant / media[column];
            *slot = fixed(entry);
        }
    }
    Ok(inverse)
}

/// A tone curve's inverse, sampled at [`CURVE_ENTRIES`] evenly spaced values
/// of light over `0..=1` as 16-bit device values: each found by bisection over
/// the curve's own compiled table, the first of its entries at or above the
/// light, and interpolated linearly between that entry and the one before
/// it. A light below the curve's first entry is device zero, and one above
/// its last is full scale.
///
/// A curve whose compiled table decreases anywhere, or whose last entry is
/// its first, has no inverse: [`IccError::NoInverse`].
fn invert_curve(curve: &Curve) -> Result<Box<[u16]>, IccError> {
    let forward = compile_curve(curve);
    let rises = forward.first() < forward.last();
    if !rises || forward.windows(2).any(|pair| pair[1] < pair[0]) {
        return Err(IccError::NoInverse);
    }
    let last = CURVE_ENTRIES - 1;
    Ok((0..CURVE_ENTRIES)
        .map(|entry| {
            let target = entry as f64 * 65535.0 / last as f64;
            let at = forward.partition_point(|&value| f64::from(value) < target);
            let x = match (
                at.checked_sub(1).and_then(|below| forward.get(below)),
                forward.get(at),
            ) {
                // At or below the first entry: the curve's foot.
                (None, _) => 0.0,
                // Above the last: full scale.
                (Some(_), None) => 1.0,
                // `below < target <= above`, so the gap is positive.
                (Some(&below), Some(&above)) => {
                    let (below, above) = (f64::from(below), f64::from(above));
                    ((at - 1) as f64 + (target - below) / (above - below)) / last as f64
                }
            };
            (x.clamp(0.0, 1.0) * 65535.0).round() as u16
        })
        .collect())
}

/// Light `linear`, in s15.16, through inverted curve `table`: held within
/// `0..=1`, located between two of its entries and interpolated between them
/// in 16-bit fixed point, rounding to the nearest.
fn through(table: &[u16], linear: i64) -> u16 {
    let Some(last) = table.len().checked_sub(1) else {
        return 0;
    };
    // Within `0..=ONE`, times at most 4 095: far inside `i64`.
    let position = linear.clamp(0, ONE) * last as i64;
    let index = (position >> 16) as usize;
    let fraction = position & 0xFFFF;
    match (table.get(index), table.get(index + 1)) {
        (Some(&low), Some(&high)) => {
            let (low, high) = (i64::from(low), i64::from(high));
            // The inverse of a curve that never decreases never decreases.
            (low + round_shift((high - low) * fraction, 16)).clamp(0, 65535) as u16
        }
        (Some(&low), None) => low,
        _ => table.get(last).copied().unwrap_or(0),
    }
}

/// CIE L*a*b* as the D50 XYZ it is relative to, by the relation the forward
/// pipeline's Lab connection space reaches XYZ through (8.6.5.4 run
/// backwards, ICC.1 6.4): what a caller holding XYZ for a profile whose
/// connection space is Lab, or Lab for one whose is XYZ, converts with.
#[must_use]
pub fn lab_to_xyz(lab: [f64; 3]) -> [f64; 3] {
    crate::lab_to_xyz(lab[0], lab[1], lab[2])
}

/// D50 XYZ as CIE L*a*b*, the inverse of [`lab_to_xyz`] (8.6.5.4, ICC.1
/// 6.4): each component over the D50 white [`lab_to_xyz`] reads Lab against,
/// through the cube root above `(6/29)³` and the line below it. The cube root
/// is `tinker_pdf_math`'s, so it is the same number on every target.
#[must_use]
pub fn xyz_to_lab(xyz: [f64; 3]) -> [f64; 3] {
    let white = lab_to_xyz([100.0, 0.0, 0.0]);
    const DELTA: f64 = 6.0 / 29.0;
    let f = |t: f64| {
        if t > DELTA * DELTA * DELTA {
            tinker_pdf_math::cbrt(t)
        } else {
            t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
        }
    };
    let ratio = |channel: usize| {
        if white[channel] > 0.0 {
            xyz[channel] / white[channel]
        } else {
            xyz[channel]
        }
    };
    let (fx, fy, fz) = (f(ratio(0)), f(ratio(1)), f(ratio(2)));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A curve's compiled table read at `x`, as the forward transform reads
    /// it: the nearest of its entries.
    fn forward(curve: &Curve, x: f64) -> u16 {
        let table = compile_curve(curve);
        table[(x * (CURVE_ENTRIES - 1) as f64).round() as usize]
    }

    /// **An inverted curve undoes its forward table.** For a gamma, a
    /// parametric curve with a linear foot and a sampled curve, each device
    /// value's light, read back through the inverse, is the device value
    /// within the inverse's own step there; the inverse never decreases, and
    /// spans the device axis from zero to full scale.
    #[test]
    fn an_inverted_curve_undoes_its_forward_table() {
        let curves = [
            Curve::Gamma(2.2),
            Curve::Parametric {
                function: 3,
                params: vec![2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045],
            },
            Curve::Sampled(vec![0, 3_000, 12_000, 30_000, 65_535]),
            Curve::Identity,
        ];
        for curve in &curves {
            let inverse = invert_curve(curve).expect("an increasing curve inverts");
            assert_eq!(inverse.len(), CURVE_ENTRIES);
            assert!(
                inverse.windows(2).all(|pair| pair[0] <= pair[1]),
                "{curve:?}"
            );
            assert_eq!(inverse[0], 0, "{curve:?}: light zero is device zero");
            assert_eq!(
                inverse[CURVE_ENTRIES - 1],
                65535,
                "{curve:?}: full light is full scale"
            );
            for step in 0..=64u32 {
                let x = f64::from(step) / 64.0;
                let light = i64::from(forward(curve, x)) * ONE / 65535;
                let back = i64::from(through(&inverse, light));
                let want = (x * 65535.0).round() as i64;
                // The inverse's slope there, measured over two of its entries
                // either side, bounds what one entry's rounding moves it.
                let at = ((light * (CURVE_ENTRIES as i64 - 1)) >> 16) as usize;
                let (low, high) = (at.saturating_sub(2), (at + 3).min(CURVE_ENTRIES - 1));
                let slack = i64::from(inverse[high]) - i64::from(inverse[low]) + 2;
                assert!(
                    (back - want).abs() <= slack,
                    "{curve:?} at {x}: {back} against {want}, slack {slack}"
                );
            }
        }
    }

    /// **A curve with no inverse is refused by name**: one that decreases
    /// anywhere, one that never rises, and an identity read backwards.
    #[test]
    fn a_curve_with_no_inverse_is_refused() {
        for curve in [
            Curve::Sampled(vec![65_535, 0]),
            Curve::Sampled(vec![0, 40_000, 30_000, 65_535]),
            Curve::Sampled(vec![20_000, 20_000]),
            Curve::Gamma(0.0),
        ] {
            assert_eq!(
                invert_curve(&curve),
                Err(IccError::NoInverse),
                "{curve:?} has no inverse"
            );
        }
    }

    /// **A singular matrix has no inverse**: columns all zero, two columns
    /// alike, and a determinant below one s15.16 step; a non-finite column
    /// likewise. The identity's inverse is the identity, and a media scale
    /// divides each column of it.
    #[test]
    fn a_singular_matrix_has_no_inverse() {
        let zero = [[0.0; 3]; 3];
        let twins = [[0.4, 0.2, 0.0], [0.4, 0.2, 0.0], [0.1, 0.1, 0.7]];
        let tiny = [[1e-2, 0.0, 0.0], [0.0, 1e-2, 0.0], [0.0, 0.0, 1e-2]];
        let nan = [[f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        for columns in [zero, twins, tiny, nan] {
            assert_eq!(
                invert_columns(&columns, [1.0; 3]),
                Err(IccError::NoInverse),
                "{columns:?}"
            );
        }
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let one = fixed(1.0);
        assert_eq!(
            invert_columns(&identity, [1.0; 3]),
            Ok([[one, 0, 0], [0, one, 0], [0, 0, one]])
        );
        assert_eq!(
            invert_columns(&identity, [2.0, 4.0, 0.5]),
            Ok([[fixed(0.5), 0, 0], [0, fixed(0.25), 0], [0, 0, fixed(2.0)]])
        );
    }

    /// **Any connection-space value answers a device colour**: zeros, the
    /// PCS white, values past either end, infinities and NaN, each through a
    /// matrix and a grey inverse, every channel within the device axis and
    /// none a panic.
    #[test]
    fn any_connection_space_value_answers_a_device_colour() {
        let matrix = Output {
            inverse: Inverse::Matrix {
                inverse: invert_columns(
                    &[
                        [0.4361, 0.2225, 0.0139],
                        [0.3851, 0.7169, 0.0971],
                        [0.1431, 0.0606, 0.7141],
                    ],
                    [1.0; 3],
                )
                .expect("sRGB's columns invert"),
                curves: [
                    invert_curve(&Curve::Gamma(2.2)).expect("inverts"),
                    invert_curve(&Curve::Gamma(2.2)).expect("inverts"),
                    invert_curve(&Curve::Gamma(2.2)).expect("inverts"),
                ],
            },
        };
        let grey = |lab: bool| Output {
            inverse: Inverse::Grey {
                lab,
                media: Some(0.9),
                factor: i64::from(fixed(1.0 / 0.9)),
                curve: invert_curve(&Curve::Identity).expect("inverts"),
            },
        };
        for output in [matrix, grey(false), grey(true)] {
            for pcs in [
                [0.0; 3],
                [0.9642, 1.0, 0.8249],
                [100.0, 0.0, 0.0],
                [-5.0, 9.0, 1e300],
                [f64::NAN, f64::INFINITY, f64::NEG_INFINITY],
            ] {
                let colour = output.apply(pcs);
                assert_eq!(colour.channels().len(), output.channels(), "{pcs:?}");
            }
        }
    }

    /// `xyz_to_lab` is `lab_to_xyz`'s inverse, across both sides of the
    /// relation's break.
    #[test]
    fn xyz_to_lab_inverts_lab_to_xyz() {
        for lab in [
            [0.0, 0.0, 0.0],
            [5.0, -3.0, 2.0],
            [50.0, 20.0, -30.0],
            [100.0, 0.0, 0.0],
            [73.0, -60.0, 80.0],
        ] {
            let back = xyz_to_lab(lab_to_xyz(lab));
            for (got, want) in back.iter().zip(lab) {
                assert!((got - want).abs() < 1e-9, "{lab:?} came back as {back:?}");
            }
        }
    }
}
