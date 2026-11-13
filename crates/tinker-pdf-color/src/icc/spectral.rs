//! A version 5 profile's spectral connection space (ICC.2:2023 6.3.3,
//! 7.2.21) reduced to a colorimetric one when its tables are built, and the
//! viewing conditions it is reduced under — the
//! `spectralViewingConditionsTag` (`svcn`, 9.2.111, of 10.2.22's Table 69)
//! (`docs/design/icc.md`, "Version 5").
//!
//! A `DToBx` tag of such a profile gives a reflectance or a transmission
//! spectrum (9.2.77): one channel a wavelength of the header's spectral range
//! (7.2.22). The spectrum is integrated against the colour matching
//! functions of the observer the profile's `svcn` carries and the power of
//! its illuminant, each resampled linearly to the range's wavelengths and
//! zero outside its own:
//!
//! `X = Σ R(λ) S(λ) x̄(λ) / Σ S(λ) ȳ(λ)`, alike for `Y` and `Z`,
//!
//! so a perfect reflector has a `Y` of one. Linear in the spectrum, the
//! reduction is three rows of a weight a wavelength ([`ViewingConditions::reduction`]),
//! built once in `f64`, which the tag's pipeline ends in (`mpe`), so a
//! colour's evaluation never sees the spectrum's channels past it.
//!
//! What this engine does not reduce is refused by name ([`SpectralRefusal`]):
//! emission, bi-spectral and sparse-matrix spectra, a profile with no
//! `svcn`, and a standard observer or illuminant named with no steps of its
//! own, whose tables (10.2.22, Tables X and Y) are data no licence this
//! repository admits lets it carry (ruling 18). Every step count is a 16-bit
//! field, held to [`MAX_SPECTRAL_STEPS`] before the values it counts are
//! read.

use super::{IccError, Reader, SpectralRange, Version5};

/// The most steps a spectral range may sample: the spectral connection
/// space's, which is its channel count (7.2.21), an observer's and an
/// illuminant's.
///
/// | | Steps |
/// | --- | --- |
/// | The most any fixture in this repository spends | 1 024 |
/// | A 200-page comic archive | 0 |
/// | A dense fixed document | 81 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **1 024** |
///
/// The document's 81 is 380 nm to 780 nm at 5 nm, the range ICC.2 itself
/// samples its assumed observers and illuminants at (10.2.22, Tables X and
/// Y); a comic page and a book picture carry no profile this parser opens.
/// The fixture is the test that reads ranges of exactly this many.
///
/// Reachable: a `spectralRange`'s step count is a 16-bit field (4.2.8), so
/// six bytes may declare 65 535;
/// `a_spectral_range_past_the_step_cap_is_refused_before_it_is_read` builds
/// one past the cap.
pub const MAX_SPECTRAL_STEPS: usize = 1024;

/// Why a profile's spectral connection space is not reduced, or its viewing
/// conditions not connected to the standard ones, by name
/// (`IccError::Spectral`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpectralRefusal {
    /// Emission (`es`), bi-spectral (`bs`) or sparse-matrix (`sm`) spectra,
    /// by the header's two letters (7.2.21, Table 21): outside what this
    /// engine reduces (`docs/design/icc.md`, "Non-goals").
    Spectra([u8; 2]),
    /// A spectral connection space with no `svcn` to integrate it against.
    NoViewingConditions,
    /// A standard observer named with no steps of its own, whose table
    /// (10.2.22, Table X) this engine does not carry.
    ObserverWithoutTable,
    /// A standard illuminant named with no steps of its own (Table Y), or
    /// one its colour temperature alone defines.
    IlluminantWithoutTable,
    /// Viewing conditions other than the CIE 1931 observer under D50 with no
    /// `customToStandardPccTag` (`c2sp`, 9.2.57) to connect a colour into the
    /// standard ones through, as 10.2.22 has a CMM refuse it.
    NoCustomToStandard,
    /// The same out of the connection space, with no
    /// `standardToCustomPccTag` (`s2cp`, 9.2.113).
    NoStandardToCustom,
    /// A matrix/TRC or grey model in viewing conditions other than the
    /// standard ones: its transform takes no `c2sp` after it, so it is
    /// passed over, and a profile it alone serves is refused.
    CustomModel,
}

/// A profile's viewing conditions (`svcn`, ICC.2 10.2.22, Table 69): its
/// observer's colour matching functions and its illuminant's power, each
/// over a spectral range of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewingConditions {
    /// The colorimetric observer (Table 70): zero for a custom one, one the
    /// CIE 1931 standard observer, two the CIE 1964.
    pub observer: u32,
    /// The illuminant (Table 71): zero for a custom one, one D50, and so on.
    pub illuminant: u32,
    /// The wavelengths the colour matching functions are given at.
    observer_range: SpectralRange,
    /// `x̄`, `ȳ` and `z̄`, a value a step of `observer_range` each.
    matching: [Vec<f64>; 3],
    /// The wavelengths the illuminant's power is given at.
    illuminant_range: SpectralRange,
    /// The illuminant's power, a value a step of `illuminant_range`.
    power: Vec<f64>,
}

impl ViewingConditions {
    /// The `spectralViewingConditionsType` (Table 69) in `data`.
    ///
    /// # Errors
    ///
    /// [`IccError::Spectral`] for a standard observer or illuminant named with
    /// no steps (Tables X and Y, which this engine does not carry);
    /// [`IccError::TooLarge`] for a range past [`MAX_SPECTRAL_STEPS`], before
    /// the values it counts are read; [`IccError::MalformedTag`] for a tag
    /// that is not one, a range that is not a range — fewer than two steps,
    /// or a start that is not finite and below a finite end — a custom
    /// observer or illuminant with no steps, or a value that is not finite.
    pub fn read(data: &[u8]) -> Result<ViewingConditions, IccError> {
        let reader = Reader { bytes: data };
        if reader.sig(0) != Some(*b"svcn") {
            return Err(IccError::MalformedTag);
        }
        let observer = reader.u32(8).ok_or(IccError::MalformedTag)?;
        let observer_range = SpectralRange::read(&reader, 12).ok_or(IccError::MalformedTag)?;
        if observer_range.steps == 0 {
            return Err(match observer {
                0 => IccError::MalformedTag,
                _ => IccError::Spectral(SpectralRefusal::ObserverWithoutTable),
            });
        }
        let n = steps(observer_range)?;
        // The matching functions are there before anything is allocated
        // for them: twelve bytes a step, then the illuminant's 40 bytes of
        // fields before its own values.
        if 20 + 12 * n + 40 > data.len() {
            return Err(IccError::MalformedTag);
        }
        let floats = |from: usize, count: usize| -> Result<Vec<f64>, IccError> {
            (0..count)
                .map(|index| float32(&reader, from + 4 * index))
                .collect()
        };
        let matching = [
            floats(20, n)?,
            floats(20 + 4 * n, n)?,
            floats(20 + 8 * n, n)?,
        ];
        let at = 20 + 12 * n;
        let illuminant = reader.u32(at).ok_or(IccError::MalformedTag)?;
        let illuminant_range =
            SpectralRange::read(&reader, at + 8).ok_or(IccError::MalformedTag)?;
        if illuminant_range.steps == 0 {
            return Err(match illuminant {
                0 => IccError::MalformedTag,
                _ => IccError::Spectral(SpectralRefusal::IlluminantWithoutTable),
            });
        }
        let m = steps(illuminant_range)?;
        // The illuminant's values, then its and the surround's XYZ.
        if at + 16 + 4 * m + 24 > data.len() {
            return Err(IccError::MalformedTag);
        }
        let power = floats(at + 16, m)?;
        Ok(ViewingConditions {
            observer,
            illuminant,
            observer_range,
            matching,
            illuminant_range,
            power,
        })
    }

    /// Whether these are the standard connection's own: the CIE 1931
    /// standard observer under D50 (10.2.22), which needs no `c2sp` or
    /// `s2cp` to connect.
    #[must_use]
    pub fn standard(&self) -> bool {
        self.observer == 1 && self.illuminant == 1
    }

    /// What a spectrum over `range` reduces to XYZ by: three rows — `X`, `Y`
    /// and `Z` — of a weight a step of `range`, `S(λ) x̄(λ) / Σ S(λ) ȳ(λ)`
    /// and alike, each function read at the step's wavelength by
    /// [`resample`].
    ///
    /// # Errors
    ///
    /// [`IccError::TooLarge`] for a range past [`MAX_SPECTRAL_STEPS`];
    /// [`IccError::MalformedTag`] for one that is not a range, or for an
    /// illuminant and observer whose `Σ S(λ) ȳ(λ)` over it is not a positive
    /// finite number, which nothing is reduced against.
    pub fn reduction(&self, range: SpectralRange) -> Result<Vec<f64>, IccError> {
        let steps = steps(range)?;
        let mut weights = vec![0.0; 3 * steps];
        let mut white = 0.0;
        for step in 0..steps {
            let wavelength = wavelength(range, step);
            let power = resample(self.illuminant_range, &self.power, wavelength);
            for (row, matching) in self.matching.iter().enumerate() {
                weights[row * steps + step] =
                    power * resample(self.observer_range, matching, wavelength);
            }
            white += weights[steps + step];
        }
        if !(white.is_finite() && white > 0.0) {
            return Err(IccError::MalformedTag);
        }
        for weight in &mut weights {
            *weight /= white;
        }
        Ok(weights)
    }
}

/// The steps `range` samples, held to [`MAX_SPECTRAL_STEPS`]: two at the
/// least, from a finite start to a finite end above it.
///
/// # Errors
///
/// [`IccError::TooLarge`] past the cap; [`IccError::MalformedTag`] for a
/// range that is not one.
fn steps(range: SpectralRange) -> Result<usize, IccError> {
    let steps = usize::from(range.steps);
    if steps > MAX_SPECTRAL_STEPS {
        return Err(IccError::TooLarge);
    }
    if steps < 2 || !range.start.is_finite() || !range.end.is_finite() || range.start >= range.end {
        return Err(IccError::MalformedTag);
    }
    Ok(steps)
}

/// Step `step`'s wavelength over `range`: `start + step (end − start) /
/// (steps − 1)` (7.2.22).
fn wavelength(range: SpectralRange, step: usize) -> f64 {
    let last = f64::from(range.steps.max(2) - 1);
    range.start + (range.end - range.start) * step as f64 / last
}

/// `values`, one a step of `range`, read at `wavelength`: linearly between
/// the two steps either side, and zero outside the range, where the function
/// is not given.
fn resample(range: SpectralRange, values: &[f64], wavelength: f64) -> f64 {
    if !(wavelength >= range.start && wavelength <= range.end) || values.is_empty() {
        return 0.0;
    }
    let last = values.len() - 1;
    if last == 0 {
        return values[0];
    }
    let position = (wavelength - range.start) / (range.end - range.start) * last as f64;
    let below = (position.floor() as usize).min(last - 1);
    let fraction = position - below as f64;
    values[below] + (values[below + 1] - values[below]) * fraction
}

/// A float32 at `at`, which has to be finite.
fn float32(reader: &Reader<'_>, at: usize) -> Result<f64, IccError> {
    let value = f64::from(f32::from_bits(
        reader.u32(at).ok_or(IccError::MalformedTag)?,
    ));
    if value.is_finite() {
        Ok(value)
    } else {
        Err(IccError::MalformedTag)
    }
}

/// The channels of the spectral connection space `header` names (7.2.21,
/// Table 21), where it is reflectance (`rs`) or transmission (`ts`) spectra:
/// the signature's count, which has to be the steps of the header's spectral
/// range (7.2.22).
///
/// # Errors
///
/// [`IccError::Spectral`] for emission, bi-spectral or sparse-matrix
/// spectra, by name; [`IccError::TooLarge`] for a count past
/// [`MAX_SPECTRAL_STEPS`]; [`IccError::MalformedTag`] for a signature Table
/// 21 does not list, a count other than the range's steps, or a range that
/// is not one.
pub(super) fn channels(header: &Version5) -> Result<usize, IccError> {
    let [first, second, high, low] = header.spectral_pcs;
    let count = usize::from(u16::from_be_bytes([high, low]));
    match &[first, second] {
        b"rs" | b"ts" => {}
        b"es" | b"bs" | b"sm" => {
            return Err(IccError::Spectral(SpectralRefusal::Spectra([
                first, second,
            ])))
        }
        _ => return Err(IccError::MalformedTag),
    }
    if count > MAX_SPECTRAL_STEPS {
        return Err(IccError::TooLarge);
    }
    if count != usize::from(header.spectral_range.steps) {
        return Err(IccError::MalformedTag);
    }
    steps(header.spectral_range)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A half float's bits for `value`, which the tests keep to whole
    /// nanometres a half float holds exactly.
    fn half(value: f64) -> [u8; 2] {
        // Normal half floats: 1.m × 2^e, ten bits of fraction.
        let exponent = value.log2().floor() as i32;
        let fraction = ((value / 2f64.powi(exponent) - 1.0) * 1024.0).round() as u16;
        (((exponent + 15) as u16) << 10 | fraction).to_be_bytes()
    }

    /// A `spectralRange` from `start` to `end` over `steps`.
    fn range(start: f64, end: f64, steps: u16) -> Vec<u8> {
        let mut out = half(start).to_vec();
        out.extend_from_slice(&half(end));
        out.extend_from_slice(&steps.to_be_bytes());
        out
    }

    /// Float32 bytes.
    fn floats(values: &[f64]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|&value| (value as f32).to_bits().to_be_bytes())
            .collect()
    }

    /// An `svcn` of `observer` type over `observer_range` with matching
    /// functions `matching`, and `illuminant` type over `illuminant_range`
    /// with power `power`.
    fn svcn(
        observer: u32,
        observer_range: &[u8],
        matching: [&[f64]; 3],
        illuminant: u32,
        illuminant_range: &[u8],
        power: &[f64],
    ) -> Vec<u8> {
        let mut out = b"svcn".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&observer.to_be_bytes());
        out.extend_from_slice(observer_range);
        out.extend_from_slice(&[0; 2]);
        for row in matching {
            out.extend(floats(row));
        }
        out.extend_from_slice(&illuminant.to_be_bytes());
        out.extend(floats(&[5000.0]));
        out.extend_from_slice(illuminant_range);
        out.extend_from_slice(&[0; 2]);
        out.extend(floats(power));
        out.extend_from_slice(&[0; 24]);
        out
    }

    fn stored(value: f64) -> f64 {
        f64::from(value as f32)
    }

    /// **The reduction is the weighted sum 10.2.22's data gives**: over a
    /// spectral range the observer and illuminant are given at, each weight
    /// is `S x̄ / Σ S ȳ` at its wavelength, so a perfect reflector's `Y` is
    /// one; over a range of half their step, a weight between two of their
    /// steps is the mean of the two; and a wavelength outside the observer's
    /// range weighs nothing.
    #[test]
    fn the_reduction_weighs_each_wavelength_by_its_power_and_matching() {
        let x = [0.25, 1.0, 0.5];
        let y = [0.5, 1.0, 0.25];
        let z = [1.0, 0.5, 0.0];
        let power = [2.0, 1.0, 0.5];
        let viewing = ViewingConditions::read(&svcn(
            0,
            &range(400.0, 600.0, 3),
            [&x, &y, &z],
            0,
            &range(400.0, 600.0, 3),
            &power,
        ))
        .expect("the conditions read");
        assert!(!viewing.standard());
        let over = |start: f64, end: f64, steps: u16| SpectralRange { start, end, steps };

        let weights = viewing.reduction(over(400.0, 600.0, 3)).expect("reduces");
        let white: f64 = (0..3).map(|i| stored(power[i]) * stored(y[i])).sum();
        for (row, matching) in [x, y, z].iter().enumerate() {
            for step in 0..3 {
                let want = stored(power[step]) * stored(matching[step]) / white;
                assert!(
                    (weights[row * 3 + step] - want).abs() < 1e-12,
                    "{row} {step}"
                );
            }
        }
        let y_of_white: f64 = weights[3..6].iter().sum();
        assert!((y_of_white - 1.0).abs() < 1e-12);

        // Half the step: the middle wavelengths read the mean of the two
        // either side, before the whole is divided by its own white.
        let halves = viewing.reduction(over(400.0, 600.0, 5)).expect("reduces");
        let raw = |row: usize, step: usize| stored(power[step]) * stored([x, y, z][row][step]);
        let mean = |row: usize, a: usize, b: usize| {
            let s = (stored(power[a]) + stored(power[b])) / 2.0;
            let c = (stored([x, y, z][row][a]) + stored([x, y, z][row][b])) / 2.0;
            s * c
        };
        let white = raw(1, 0) + mean(1, 0, 1) + raw(1, 1) + mean(1, 1, 2) + raw(1, 2);
        assert!((halves[5 + 1] - mean(1, 0, 1) / white).abs() < 1e-12);
        assert!((halves[5 + 2] - raw(1, 1) / white).abs() < 1e-12);

        // 300 nm to 600 nm in four steps: 300 nm is outside both ranges.
        let wider = viewing.reduction(over(300.0, 600.0, 4)).expect("reduces");
        assert_eq!([wider[0], wider[4], wider[8]], [0.0, 0.0, 0.0]);
    }

    /// **The standard viewing conditions are the CIE 1931 observer under
    /// D50**, by their types (Tables 70 and 71); any other observer or
    /// illuminant is a custom connection's.
    #[test]
    fn the_standard_conditions_are_the_1931_observer_under_d50() {
        let ones = [1.0, 1.0];
        let read = |observer: u32, illuminant: u32| {
            ViewingConditions::read(&svcn(
                observer,
                &range(400.0, 700.0, 2),
                [&ones, &ones, &ones],
                illuminant,
                &range(400.0, 700.0, 2),
                &ones,
            ))
            .expect("the conditions read")
            .standard()
        };
        assert!(read(1, 1));
        assert!(!read(2, 1), "the 1964 observer");
        assert!(!read(1, 2), "D65");
        assert!(!read(0, 1), "a custom observer");
    }

    /// **What is not carried is refused by name** (10.2.22): a standard
    /// observer or illuminant with no steps of its own — Tables X and Y,
    /// which this engine does not carry — is `IccError::Spectral`, and a
    /// custom one with no steps, a range of one step or one ending where it
    /// starts, a value that is not finite and a tag cut short are malformed.
    #[test]
    fn what_the_conditions_do_not_carry_is_refused_by_name() {
        let ones = [1.0, 1.0];
        let good = range(400.0, 700.0, 2);
        let tag =
            |observer: u32, observer_range: &[u8], illuminant: u32, illuminant_range: &[u8]| {
                let steps = |bytes: &[u8]| usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
                let matching = vec![1.0; steps(observer_range)];
                let power = vec![1.0; steps(illuminant_range)];
                ViewingConditions::read(&svcn(
                    observer,
                    observer_range,
                    [&matching, &matching, &matching],
                    illuminant,
                    illuminant_range,
                    &power,
                ))
            };
        let none = range(380.0, 780.0, 0);
        assert_eq!(
            tag(1, &none, 1, &good),
            Err(IccError::Spectral(SpectralRefusal::ObserverWithoutTable))
        );
        assert_eq!(
            tag(1, &good, 1, &none),
            Err(IccError::Spectral(SpectralRefusal::IlluminantWithoutTable))
        );
        assert_eq!(
            tag(1, &good, 9, &none),
            Err(IccError::Spectral(SpectralRefusal::IlluminantWithoutTable)),
            "a black body by its temperature"
        );
        assert_eq!(tag(0, &none, 1, &good), Err(IccError::MalformedTag));
        assert_eq!(tag(1, &good, 0, &none), Err(IccError::MalformedTag));
        assert_eq!(
            tag(1, &range(400.0, 700.0, 1), 1, &good),
            Err(IccError::MalformedTag)
        );
        assert_eq!(
            tag(1, &range(500.0, 500.0, 2), 1, &good),
            Err(IccError::MalformedTag)
        );

        let mut infinite = svcn(1, &good, [&ones, &ones, &ones], 1, &good, &ones);
        infinite[20..24].copy_from_slice(&f32::INFINITY.to_bits().to_be_bytes());
        assert_eq!(
            ViewingConditions::read(&infinite),
            Err(IccError::MalformedTag)
        );
        let whole = svcn(1, &good, [&ones, &ones, &ones], 1, &good, &ones);
        assert_eq!(
            ViewingConditions::read(&whole[..whole.len() - 1]),
            Err(IccError::MalformedTag)
        );
        assert!(ViewingConditions::read(&whole).is_ok());

        let dark = svcn(1, &good, [&ones, &[0.0, 0.0], &ones], 1, &good, &ones);
        let viewing = ViewingConditions::read(&dark).expect("reads");
        assert_eq!(
            viewing.reduction(SpectralRange {
                start: 400.0,
                end: 700.0,
                steps: 2
            }),
            Err(IccError::MalformedTag),
            "no Y to divide by"
        );
    }

    /// **A spectral range past the step cap is refused before it is read**
    /// (ruling 1). The row `crates/tinker-pdf/tests/bounds_ledger.rs` names
    /// as proving [`MAX_SPECTRAL_STEPS`]: an observer, an illuminant and a
    /// spectral connection space of one step past the cap are each
    /// `IccError::TooLarge`, the observer's before its 12 300 bytes of
    /// matching functions are looked for; each at exactly the cap reads and
    /// reduces.
    ///
    /// No clock. The assertion is the typed refusal.
    #[test]
    fn a_spectral_range_past_the_step_cap_is_refused_before_it_is_read() {
        let cap = MAX_SPECTRAL_STEPS as u16;
        let ones = vec![1.0; MAX_SPECTRAL_STEPS];
        let at_cap = range(380.0, 780.0, cap);
        let past = range(380.0, 780.0, cap + 1);
        let full = |observer_range: &[u8], illuminant_range: &[u8]| {
            svcn(
                1,
                observer_range,
                [&ones, &ones, &ones],
                1,
                illuminant_range,
                &ones,
            )
        };
        // The observer's count is checked before its values are: the tag
        // past the cap holds only the cap's values, and is refused for the
        // count all the same.
        assert_eq!(
            ViewingConditions::read(&full(&past, &at_cap)),
            Err(IccError::TooLarge)
        );
        let mut header = b"svcn".to_vec();
        header.extend_from_slice(&[0; 4]);
        header.extend_from_slice(&1u32.to_be_bytes());
        header.extend_from_slice(&range(380.0, 780.0, u16::MAX));
        assert_eq!(ViewingConditions::read(&header), Err(IccError::TooLarge));
        assert_eq!(
            ViewingConditions::read(&full(&at_cap, &past)),
            Err(IccError::TooLarge)
        );
        let viewing = ViewingConditions::read(&full(&at_cap, &at_cap)).expect("exactly the cap");
        let over = |steps: u16| SpectralRange {
            start: 380.0,
            end: 780.0,
            steps,
        };
        assert_eq!(viewing.reduction(over(cap + 1)), Err(IccError::TooLarge));
        let weights = viewing.reduction(over(cap)).expect("exactly the cap");
        assert_eq!(weights.len(), 3 * MAX_SPECTRAL_STEPS);

        let header = |pcs: [u8; 4], steps: u16| Version5 {
            spectral_pcs: pcs,
            spectral_range: over(steps),
            bispectral_range: over(0),
            mcs: [0; 4],
            subclass: [0; 4],
        };
        let signature = |kind: &[u8; 2], count: u16| {
            let [high, low] = count.to_be_bytes();
            [kind[0], kind[1], high, low]
        };
        assert_eq!(
            channels(&header(signature(b"rs", cap + 1), cap + 1)),
            Err(IccError::TooLarge)
        );
        assert_eq!(
            channels(&header(signature(b"rs", cap), cap)),
            Ok(MAX_SPECTRAL_STEPS)
        );
    }

    /// **The header's spectral connection space is reflectance or
    /// transmission** (7.2.21, Table 21): `rs` and `ts` give their count,
    /// which has to be the spectral range's steps; emission, bi-spectral and
    /// sparse-matrix spectra are refused by their letters; a signature Table
    /// 21 does not list is malformed.
    #[test]
    fn the_spectral_connection_space_is_reflectance_or_transmission() {
        let header = |pcs: &[u8; 4], steps: u16| Version5 {
            spectral_pcs: *pcs,
            spectral_range: SpectralRange {
                start: 400.0,
                end: 700.0,
                steps,
            },
            bispectral_range: SpectralRange {
                start: 0.0,
                end: 0.0,
                steps: 0,
            },
            mcs: [0; 4],
            subclass: [0; 4],
        };
        assert_eq!(channels(&header(b"rs\x00\x1f", 31)), Ok(31));
        assert_eq!(channels(&header(b"ts\x00\x1f", 31)), Ok(31));
        assert_eq!(
            channels(&header(b"rs\x00\x1f", 16)),
            Err(IccError::MalformedTag),
            "a count that is not the range's"
        );
        for kind in [b"es", b"bs", b"sm"] {
            let pcs = [kind[0], kind[1], 0, 31];
            assert_eq!(
                channels(&header(&pcs, 31)),
                Err(IccError::Spectral(SpectralRefusal::Spectra(*kind)))
            );
        }
        assert_eq!(
            channels(&header(b"xx\x00\x1f", 31)),
            Err(IccError::MalformedTag)
        );
    }
}
