//! The `multiProcessElementsType` (ICC.1:2010 10.14, ICC.2:2023 10.2.17
//! and clause 11): the floating-point transforms of the `D2B*` and `B2D*`
//! tags, and of a version 5 profile's `A2B*` and `B2A*` tags, read and
//! compiled into integer stages.
//!
//! A tag is a chain of processing elements, each handing its channels to the
//! next unclipped (10.14.2.1): curve sets (`cvst`), matrices with offsets
//! (`matf`), multi-dimensional lookup tables (`clut`, and ICC.2's extended
//! `xclt`, whose values may be 8- or 16-bit integers or half floats), and
//! the `bACS` and `eACS` placeholders, which pass their channels through
//! (10.14.2.5), and ICC.2's calculator (`calc`, 11.2.1, the `calc` module),
//! whose program is run over a grid of its inputs' intervals when the stage
//! is built. A curve set's curve is a segmented curve (`curf`) of formula
//! segments — ICC.1's three and ICC.2's five more (Table 111) — and sampled
//! ones, ICC.2's single sampled curve (`sngf`, 11.2.2.2), or its sampled
//! calculator curve (`clcf`, 11.2.2.3), a calculator sampled into a single
//! sampled curve when it is read. Their numbers are floats in the file. They
//! are read once, when the pipeline is built, and compiled into fixed-point
//! stages — a curve sampled over the interval its input can take, a matrix
//! and a grid in s15.16 — so that a colour's evaluation is integer
//! arithmetic and nothing on the pixel path rounds differently on another
//! target (ruling 4). The formulas a curve segment is made of are evaluated
//! with `tinker_pdf_math` while it is sampled, never per colour.
//!
//! A tag runs from the device to the connection space (a `D2B*` tag, a
//! version 5 `A2B*`), from the connection space to the device (a `B2D*`
//! tag, a version 5 `B2A*`), or from one device to another (a device link's
//! `D2B0`). Its first element's inputs take the interval its start can hold:
//! a device's `0..=1`, or the connection space's own range — XYZ from 0 to
//! the most `u1Fixed15` encodes, `L*` from 0 to 100 and `a*` and `b*` from
//! −128 to 127 — and a colour's components are held there on the way in.
//!
//! A tag holding an element of a type this build does not read is refused
//! by the element's name, [`IccError::UnsupportedElement`]: 10.14.1 makes such
//! a tag unusable, and ICC.1 8.10.2 a) passes it over, so the intent converts
//! through its `A2B*` table instead (`docs/design/icc.md`, "The table each
//! intent compiles").
//!
//! # Every count is bounded before it is allocated
//!
//! The element count is a 32-bit field, a channel count sixteen bits, a
//! segment count and a sample count are file fields, and a grid's volume is a
//! product of them (ruling 1). The element count is held to
//! [`MAX_MPE_ELEMENTS`], every channel count to `MAX_LUT_INPUTS`, a grid to
//! `MAX_CLUT_ENTRIES`, and every table the pipeline compiles — grid entries
//! and sampled curve entries together, a calculator's sub-element tables and
//! its sampled calculator curves among them — to `MAX_CLUT_ENTRIES` in all;
//! a segment or sample count is checked against the bytes that would hold it
//! before anything is read. The steps the tag's calculators take while it is
//! read are held to `MAX_CALC_WORK` in all, each run counted before it is
//! made ([`Budget`]).

use std::mem::size_of;

use super::calc::{Calculator, CalculatorLimit, MAX_CALC_WORK};
use super::{
    half_float, IccError, Reader, CURVE_ENTRIES, MAX_CLUT_ENTRIES, MAX_DEVICE_CHANNELS,
    MAX_LUT_INPUTS,
};

/// The most processing elements one `multiProcessElementsType` tag may chain.
///
/// The count is a 32-bit field read before the position table it describes,
/// and elements may share their data (10.14.1), so a few bytes can name a
/// long chain that costs work on every colour; the cap bounds that work.
///
/// | | Elements |
/// | --- | --- |
/// | The most any fixture in this repository spends | 64 |
/// | A 200-page comic archive | 0 |
/// | A dense fixed document | 5 |
/// | A 300-page reflowable book | 0 |
/// | **This cap** | **64** |
///
/// The document's five is a v4 table's whole shape written as elements — a
/// curve set, a matrix, a curve set, a grid and a curve set — which is what a
/// `D2B*` tag or a version 5 profile's transform chains; a comic page and a
/// book picture carry no profile this parser opens. The fixture is the test
/// that declares exactly this many and is walked.
///
/// Reachable: the count is a 32-bit field at offset 12, so a 16-byte tag may
/// declare 4 294 967 295 elements;
/// `an_element_count_past_the_cap_is_refused_before_the_table_is_walked`
/// builds one past the cap.
pub const MAX_MPE_ELEMENTS: usize = 64;

/// How many points a curve set's curve is sampled at, over the interval its
/// input can take: [`CURVE_ENTRIES`], the resolution of a matrix/TRC
/// profile's compiled curves.
const CURVE_SAMPLES: usize = CURVE_ENTRIES;

/// One in s15.16, the fixed point every stage computes in.
const ONE: i64 = 1 << 16;

/// What reading one `multiProcessElementsType` tag may still spend: table
/// entries — grids, sampled curves, a calculator's sub-element tables and
/// its sampled calculator curves together — and the steps its calculators
/// take while it is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Budget {
    /// Entries still to spend, of [`MAX_CLUT_ENTRIES`].
    pub(super) entries: usize,
    /// Calculator steps still to spend, of [`MAX_CALC_WORK`].
    pub(super) steps: usize,
}

impl Budget {
    /// A whole tag's.
    pub(super) const TAG: Budget = Budget {
        entries: MAX_CLUT_ENTRIES,
        steps: MAX_CALC_WORK,
    };

    /// `entries` more entries spent: [`IccError::TooLarge`] past what is
    /// left, which is then unchanged.
    pub(super) fn spend_entries(&mut self, entries: usize) -> Result<(), IccError> {
        self.entries = self
            .entries
            .checked_sub(entries)
            .ok_or(IccError::TooLarge)?;
        Ok(())
    }

    /// `runs` runs of a calculator spent, each the `steps` its program takes
    /// at the most and one more for the run:
    /// [`IccError::CalculatorLimit`]`(`[`CalculatorLimit::Work`]`)` past what
    /// is left, which is then unchanged — found before any of them is made.
    pub(super) fn spend_runs(&mut self, runs: usize, steps: usize) -> Result<(), IccError> {
        let past = IccError::CalculatorLimit(CalculatorLimit::Work);
        let spent = runs.checked_mul(steps.saturating_add(1)).ok_or(past)?;
        self.steps = self.steps.checked_sub(spent).ok_or(past)?;
        Ok(())
    }
}

/// The most XYZ the connection space encodes: `u1Fixed15`'s `1 + 32 767 /
/// 32 768` (ICC.1 6.3.4.2), the top of the interval a tag out of an XYZ
/// connection space takes.
const XYZ_REACH: f64 = 65535.0 / 32768.0;

/// A `multiProcessElementsType` tag, compiled.
#[derive(Clone, Debug, PartialEq)]
pub struct Pipeline {
    /// How many channels go in.
    inputs: usize,
    /// How many channels come out: the connection space's three, or a
    /// device's.
    outputs: usize,
    /// Each input's interval in s15.16, which a component is held to on the
    /// way in: a device's `0..=1`, or the connection space's range.
    domain: Vec<(i64, i64)>,
    /// The stages, in the order the tag lists its elements; the placeholders
    /// are left out, since they change nothing.
    stages: Vec<Stage>,
    /// Whether the connection space at the tag's connection-space end is
    /// `Lab` rather than `XYZ`, as the profile's header says.
    lab: bool,
}

/// What a tag's two ends are, which fixes the channel counts it is held to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ends {
    /// A `D2B*` tag, or a version 5 `A2B*`: the device's channels in, each
    /// held to `0..=1`, the connection space's three out.
    Forward,
    /// A `B2D*` tag, or a version 5 `B2A*`: the connection space's three in,
    /// each held to its range, the data space's `channels` out.
    Output { channels: usize },
    /// A device link's `D2B0` (ICC.1 8.6, 8.10.3): `inputs` device channels
    /// in, each held to `0..=1`, and the `outputs` of the device the link's
    /// connection-space field names out, with no connection space at
    /// either end.
    Link { inputs: usize, outputs: usize },
    /// A version 5 `D2B*` tag over a spectral connection space (ICC.2
    /// 9.2.77): the device's channels in, each held to `0..=1`, and the
    /// spectral connection space's `channels` out, one a wavelength.
    Spectral { channels: usize },
    /// A `customToStandardPccTag` or a `standardToCustomPccTag` (ICC.2
    /// 9.2.57, 9.2.113): XYZ in, each held to the connection space's range,
    /// and XYZ out.
    Connection,
}

impl Ends {
    /// The interval each of `inputs` channels takes at the tag's start: the
    /// connection space's range for a tag out of it — XYZ to what
    /// `u1Fixed15` encodes, or `L*` 0 to 100 and `a*` and `b*` −128 to 127 —
    /// and a device's `0..=1` otherwise.
    fn domain(self, inputs: usize, lab: bool) -> Vec<(f64, f64)> {
        match self {
            Ends::Output { .. } if lab => vec![(0.0, 100.0), (-128.0, 127.0), (-128.0, 127.0)],
            Ends::Output { .. } | Ends::Connection => vec![(0.0, XYZ_REACH); inputs],
            Ends::Forward | Ends::Link { .. } | Ends::Spectral { .. } => {
                vec![(0.0, 1.0); inputs]
            }
        }
    }

    /// The most channels an element between the ends may give:
    /// `MAX_LUT_INPUTS`, or a spectral connection space's count where that
    /// is more.
    fn most(self) -> usize {
        match self {
            Ends::Spectral { channels } => channels.max(MAX_LUT_INPUTS),
            _ => MAX_LUT_INPUTS,
        }
    }
}

/// One compiled processing element.
#[derive(Clone, Debug, PartialEq)]
enum Stage {
    /// A curve set: one curve a channel.
    Curves(Vec<Sampled>),
    /// A matrix with offsets: `outputs` rows of `inputs` coefficients, then
    /// one offset a row, each in s15.16.
    Matrix {
        inputs: usize,
        outputs: usize,
        coefficients: Vec<i32>,
        offsets: Vec<i32>,
    },
    /// A lookup table: grid points along each input axis, the last axis
    /// varying fastest, `outputs` values a point in s15.16.
    Grid {
        grids: Vec<usize>,
        outputs: usize,
        values: Vec<i32>,
    },
    /// A calculator, sampled when the stage was built: a grid over each
    /// input's interval — its low end and its span, in s15.16 — the last
    /// axis varying fastest, `outputs` values a point in s15.16.
    Calculated {
        domain: Vec<(i64, i64)>,
        grids: Vec<usize>,
        outputs: usize,
        values: Vec<i32>,
    },
}

/// A curve sampled at evenly spaced points over `low ..= low + span`, its
/// input interval, in s15.16; an input outside it reads the nearer end.
#[derive(Clone, Debug, PartialEq)]
struct Sampled {
    low: i64,
    span: i64,
    table: Vec<i32>,
}

/// One curve of a curve set, as the file states it: what a pipeline samples
/// and a calculator evaluates.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Curve {
    /// A segmented curve (`curf`).
    Segmented(Segmented),
    /// ICC.2's single sampled curve (`sngf`, 11.2.2.2), or its sampled
    /// calculator curve (`clcf`, 11.2.2.3) once its calculator is sampled.
    Single(SingleSampled),
}

impl Curve {
    /// The curve at `x`.
    pub(super) fn eval(&self, x: f64) -> f64 {
        match self {
            Curve::Segmented(curve) => curve.eval(x),
            Curve::Single(curve) => curve.eval(x),
        }
    }

    /// The numbers it keeps, which a calculator's sub-element spends as
    /// entries.
    pub(super) fn held(&self) -> usize {
        match self {
            Curve::Segmented(curve) => curve.held(),
            Curve::Single(curve) => curve.values.len(),
        }
    }
}

/// A segmented curve (`curf`, 10.14.2.2), as the file states it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Segmented {
    /// The break-points, `segments.len() - 1` of them, never decreasing.
    breaks: Vec<f64>,
    segments: Vec<Segment>,
    /// For each segment, the value a sampled one starts from: the segment
    /// before it at the break-point between them. Unused for the first.
    starts: Vec<f64>,
}

/// One segment of a segmented curve.
#[derive(Clone, Debug, PartialEq)]
enum Segment {
    /// `parf`: a formula, by function type and its parameters in order.
    Formula { function: u16, params: [f64; 7] },
    /// `samf`: samples evenly spaced over the segment, its start excluded.
    Samples(Vec<f64>),
}

/// ICC.2's single sampled curve (11.2.2.2): `values` evenly spaced from
/// `first` to `last`, read linearly between them, and beyond them clipped to
/// the end value or extended along the line through the two end entries.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SingleSampled {
    first: f64,
    last: f64,
    extrapolate: bool,
    values: Vec<f64>,
}

/// How an ICC.2 table stores its values (4.2.10, Table 8): the number
/// itself, or a fraction of `0..=1` in an unsigned integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Values {
    /// `float32Number`.
    Float32,
    /// `float16Number` (4.2.2).
    Float16,
    /// `uInt16Number`, 65 535 being one.
    Unsigned16,
    /// `uInt8Number`, 255 being one.
    Unsigned8,
}

impl Values {
    /// The encoding a `valueEncodingType` field names, or `None` for one
    /// Table 8 does not list.
    fn of(field: u32) -> Option<Values> {
        Some(match field {
            0 => Values::Float32,
            1 => Values::Float16,
            2 => Values::Unsigned16,
            3 => Values::Unsigned8,
            _ => return None,
        })
    }

    /// The bytes one value takes.
    fn width(self) -> usize {
        match self {
            Values::Float32 => 4,
            Values::Float16 | Values::Unsigned16 => 2,
            Values::Unsigned8 => 1,
        }
    }

    /// The value at `at`, which has to be finite.
    fn read(self, reader: &Reader<'_>, at: usize) -> Result<f64, IccError> {
        match self {
            Values::Float32 => float32(reader, at),
            Values::Float16 => {
                let value = half_float(reader.u16(at).ok_or(IccError::MalformedTag)?);
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(IccError::MalformedTag)
                }
            }
            Values::Unsigned16 => {
                Ok(f64::from(reader.u16(at).ok_or(IccError::MalformedTag)?) / 65535.0)
            }
            Values::Unsigned8 => {
                Ok(f64::from(*reader.bytes.get(at).ok_or(IccError::MalformedTag)?) / 255.0)
            }
        }
    }
}

/// A `multiProcessElementsType` tag's pipeline, read from `data`: device
/// channels in, the three channels of the connection space out, `Lab` where
/// `lab` and otherwise `XYZ`.
///
/// # Errors
///
/// [`IccError::UnsupportedElement`] where an element is of a type this build
/// does not read, [`IccError::TooLarge`] where a count passes its cap, and
/// [`IccError::MalformedTag`] where the tag's structure or a value in it is
/// not one ICC.1 or ICC.2 allows: a channel count that does not chain, a
/// break-point that decreases, a number that is not finite, a curve that is
/// not finite over the inputs it is given.
pub fn read(data: &[u8], lab: bool) -> Result<Pipeline, IccError> {
    read_tag(data, lab, Ends::Forward)
}

/// A tag out of the connection space — a `B2D*` tag, or a version 5
/// profile's `B2A*` — read from `data`: the connection space's three
/// channels in, `Lab` where `lab` and otherwise `XYZ`, each held to the
/// space's range, and the data space's `channels` out, each a device value.
///
/// # Errors
///
/// [`read`]'s, and [`IccError::TooLarge`] for a tag giving more than
/// [`MAX_DEVICE_CHANNELS`] channels, before any element is read;
/// [`IccError::UnsupportedSpace`] for one giving another count than the data
/// space has; [`IccError::MalformedTag`] for one not taking three.
pub fn read_output(data: &[u8], lab: bool, channels: usize) -> Result<Pipeline, IccError> {
    read_tag(data, lab, Ends::Output { channels })
}

/// A device link's `D2B0` (ICC.1 8.6, 8.10.3 a)), read from `data`: the
/// `inputs` channels of the link's data space in, the `outputs` of the
/// device its connection-space field names out, each a device value with no
/// connection space at either end.
///
/// # Errors
///
/// [`read`]'s, and [`IccError::TooLarge`] for a tag giving more than
/// [`MAX_DEVICE_CHANNELS`] channels, before any element is read, and
/// [`IccError::UnsupportedSpace`] for one whose counts are not the two
/// spaces'.
pub fn read_link(data: &[u8], inputs: usize, outputs: usize) -> Result<Pipeline, IccError> {
    read_tag(data, false, Ends::Link { inputs, outputs })
}

/// A version 5 `D2B*` tag over a spectral connection space (ICC.2 9.2.77),
/// read from `data` and reduced to XYZ: its elements give a spectrum, one
/// channel a wavelength, which a last stage of `weights` — three rows of a
/// weight a wavelength, `spectral`'s reduction — takes to `X`, `Y` and `Z`
/// with the PCS white's `Y` at one. Its elements may give as many channels
/// as the spectrum has, a matrix's coefficients spent from the tag's
/// entries as a grid's are, and the reduction's with them.
///
/// # Errors
///
/// [`read`]'s, and [`IccError::UnsupportedSpace`] for a tag that does not
/// give the spectrum's channels.
pub(super) fn read_spectral(data: &[u8], weights: &[f64]) -> Result<Pipeline, IccError> {
    let channels = weights.len() / 3;
    let mut pipeline = read_tag(data, false, Ends::Spectral { channels })?;
    pipeline.stages.push(Stage::Matrix {
        inputs: channels,
        outputs: 3,
        coefficients: weights.iter().map(|&weight| fixed32(weight)).collect(),
        offsets: vec![0; 3],
    });
    pipeline.outputs = 3;
    Ok(pipeline)
}

/// A `customToStandardPccTag` or `standardToCustomPccTag` (ICC.2 9.2.57,
/// 9.2.113) read from `data`: XYZ under one set of viewing conditions to XYZ
/// under the other, three channels each way.
///
/// # Errors
///
/// [`read`]'s, and [`IccError::MalformedTag`] for a tag that does not take
/// and give three.
pub(super) fn read_connection(data: &[u8]) -> Result<Pipeline, IccError> {
    read_tag(data, false, Ends::Connection)
}

/// A `multiProcessElementsType` tag's pipeline, read from `data` between the
/// two ends `ends` names.
fn read_tag(data: &[u8], lab: bool, ends: Ends) -> Result<Pipeline, IccError> {
    let reader = Reader { bytes: data };
    if reader.sig(0) != Some(*b"mpet") {
        return Err(IccError::MalformedTag);
    }
    let inputs = usize::from(reader.u16(8).ok_or(IccError::MalformedTag)?);
    let outputs = usize::from(reader.u16(10).ok_or(IccError::MalformedTag)?);
    let count = reader.u32(12).ok_or(IccError::MalformedTag)? as usize;
    if inputs == 0 || outputs == 0 || count == 0 {
        return Err(IccError::MalformedTag);
    }
    if inputs > MAX_LUT_INPUTS || count > MAX_MPE_ELEMENTS {
        return Err(IccError::TooLarge);
    }
    match ends {
        // Three is what the connection space is, and what a `D2B*` tag gives.
        Ends::Forward if outputs != 3 => return Err(IccError::UnsupportedSpace),
        Ends::Forward => {}
        Ends::Output { .. } | Ends::Link { .. } if outputs > MAX_DEVICE_CHANNELS => {
            return Err(IccError::TooLarge)
        }
        Ends::Output { channels } if outputs != channels => return Err(IccError::UnsupportedSpace),
        Ends::Output { .. } if inputs != 3 => return Err(IccError::MalformedTag),
        Ends::Output { .. } => {}
        Ends::Link {
            inputs: data_space,
            outputs: device,
        } if inputs != data_space || outputs != device => return Err(IccError::UnsupportedSpace),
        Ends::Link { .. } => {}
        Ends::Spectral { channels } if outputs != channels => {
            return Err(IccError::UnsupportedSpace)
        }
        Ends::Spectral { .. } => {}
        // 9.2.57, 9.2.113: three channels in and out.
        Ends::Connection if inputs != 3 || outputs != 3 => return Err(IccError::MalformedTag),
        Ends::Connection => {}
    }

    // The elements, located, and every one's type known before any is
    // compiled: a tag with one element this build does not read is not used
    // at all (10.14.1), whatever the rest of it says.
    let mut elements = Vec::with_capacity(count);
    for index in 0..count {
        let at = 16 + 8 * index;
        let offset = reader.u32(at).ok_or(IccError::MalformedTag)? as usize;
        let size = reader.u32(at + 4).ok_or(IccError::MalformedTag)? as usize;
        let element = offset
            .checked_add(size)
            .and_then(|end| data.get(offset..end))
            .ok_or(IccError::MalformedTag)?;
        let signature = Reader { bytes: element }
            .sig(0)
            .ok_or(IccError::MalformedTag)?;
        if !matches!(
            &signature,
            b"cvst" | b"matf" | b"clut" | b"xclt" | b"calc" | b"bACS" | b"eACS"
        ) {
            return Err(IccError::UnsupportedElement(signature));
        }
        elements.push((signature, element));
    }

    // Each element's input interval, starting from the tag's start: a
    // colour's components arrive held to it.
    let start = ends.domain(inputs, lab);
    let domain = start
        .iter()
        .map(|&(low, high)| (fixed(low), fixed(high)))
        .collect();
    let mut intervals = start;
    let mut budget = Budget::TAG;
    // A spectrum's channels may pass `MAX_LUT_INPUTS`, so a matrix between
    // them is as large as a grid: its coefficients are spent as entries, and
    // so are the reduction's that `read_spectral` ends the tag in.
    let spectral = matches!(ends, Ends::Spectral { .. });
    if let Ends::Spectral { channels } = ends {
        budget.spend_entries(3 * channels + 3)?;
    }
    let mut stages = Vec::new();
    for (signature, element) in elements {
        let reader = Reader { bytes: element };
        let p = usize::from(reader.u16(8).ok_or(IccError::MalformedTag)?);
        let q = usize::from(reader.u16(10).ok_or(IccError::MalformedTag)?);
        // 10.14.2.1: an element takes the channels the one before it gives.
        if p != intervals.len() || q == 0 {
            return Err(IccError::MalformedTag);
        }
        if q > ends.most() {
            return Err(IccError::TooLarge);
        }
        match &signature {
            b"cvst" => {
                if q != p {
                    return Err(IccError::MalformedTag);
                }
                stages.push(curve_set(element, &mut intervals, &mut budget)?);
            }
            b"matf" => {
                if spectral {
                    budget.spend_entries(p * q + q)?;
                }
                stages.push(matrix(element, q, &mut intervals)?);
            }
            // ICC.1's grid: float32 values from byte 28, its counts at 12.
            b"clut" => stages.push(grid(
                element,
                q,
                &mut intervals,
                &mut budget,
                (12, 28, Values::Float32),
            )?),
            // ICC.2's extended grid (11.2.7): its encoding at 12, its counts
            // at 16 and its values from 32.
            b"xclt" => {
                let encoding = reader
                    .u32(12)
                    .and_then(Values::of)
                    .ok_or(IccError::MalformedTag)?;
                stages.push(grid(
                    element,
                    q,
                    &mut intervals,
                    &mut budget,
                    (16, 32, encoding),
                )?);
            }
            b"calc" => stages.push(calculated(element, q, &mut intervals, &mut budget)?),
            // `bACS` and `eACS`: pass-through, with as many channels out as
            // in (10.14.2.5).
            _ => {
                if q != p {
                    return Err(IccError::MalformedTag);
                }
            }
        }
    }
    if intervals.len() != outputs {
        return Err(IccError::MalformedTag);
    }
    Ok(Pipeline {
        inputs,
        outputs,
        domain,
        stages,
        lab,
    })
}

/// The most bytes reading the `multiProcessElementsType` tag `data` can
/// leave its pipeline holding ([`Pipeline::held_bytes`]), from the tag's
/// element headers alone: a curve set's [`CURVE_SAMPLES`] entries a channel,
/// a matrix's coefficients and offsets, and a grid's counts and values — the
/// curves' and grids' entries together no more than [`MAX_CLUT_ENTRIES`], as
/// the read holds them — beside a stage for each of at most
/// [`MAX_MPE_ELEMENTS`] elements and an interval for each input. A channel
/// count is read as at most `MAX_LUT_INPUTS`, past which the read refuses
/// the tag, and an element the tag does not hold allocates nothing, since
/// the read refuses it too.
pub(super) fn bound(data: &[u8]) -> usize {
    bound_of(data, None)
}

/// [`bound`] for a spectral `D2B*` tag of `channels` wavelengths
/// ([`read_spectral`]): a channel count read as at most `channels` where
/// that passes `MAX_LUT_INPUTS`, a matrix's coefficients among the entries,
/// as the read spends them, and the reduction's stage and coefficients
/// beside the elements'.
pub(super) fn spectral_bound(data: &[u8], channels: usize) -> usize {
    bound_of(data, Some(channels))
}

/// [`bound`], or [`spectral_bound`] where `spectral` gives the spectrum's
/// channels.
fn bound_of(data: &[u8], spectral: Option<usize>) -> usize {
    let reader = Reader { bytes: data };
    let count = reader
        .u32(12)
        .map_or(0, |count| count as usize)
        .min(MAX_MPE_ELEMENTS);
    let most = spectral.map_or(MAX_LUT_INPUTS, |channels| channels.max(MAX_LUT_INPUTS));
    let mut entries = spectral.map_or(0, |channels| 3 * channels + 3);
    let mut beside = size_of::<Pipeline>()
        + (MAX_MPE_ELEMENTS + usize::from(spectral.is_some())) * size_of::<Stage>()
        + MAX_LUT_INPUTS * size_of::<(i64, i64)>();
    for index in 0..count {
        let at = 16 + 8 * index;
        let element = reader
            .u32(at)
            .zip(reader.u32(at + 4))
            .and_then(|(offset, size)| {
                let offset = offset as usize;
                data.get(offset..offset.checked_add(size as usize)?)
            });
        let Some(element) = element else {
            continue;
        };
        let header = Reader { bytes: element };
        let p = header.u16(8).map_or(0, usize::from).min(most);
        let q = header.u16(10).map_or(0, usize::from).min(most);
        let grid = |counts: usize| {
            (0..p)
                .map(|axis| {
                    element
                        .get(counts + axis)
                        .map_or(0, |&count| usize::from(count))
                })
                .fold(1usize, usize::saturating_mul)
                .saturating_mul(q)
        };
        match header.sig(0).as_ref() {
            Some(b"cvst") => {
                entries = entries.saturating_add(q * CURVE_SAMPLES);
                beside = beside.saturating_add(q * size_of::<Sampled>());
            }
            Some(b"matf") if spectral.is_some() => entries = entries.saturating_add(p * q + q),
            Some(b"matf") => beside = beside.saturating_add((p * q + q) * size_of::<i32>()),
            Some(b"clut") => {
                entries = entries.saturating_add(grid(12));
                beside = beside.saturating_add(p * size_of::<usize>());
            }
            Some(b"xclt") => {
                entries = entries.saturating_add(grid(16));
                beside = beside.saturating_add(p * size_of::<usize>());
            }
            Some(b"calc") => {
                let points = calculator_points(p);
                entries = entries.saturating_add(
                    (0..p)
                        .fold(1usize, |nodes, _| nodes.saturating_mul(points))
                        .saturating_mul(q),
                );
                beside = beside.saturating_add(p * (size_of::<usize>() + size_of::<(i64, i64)>()));
            }
            _ => {}
        }
    }
    entries
        .min(MAX_CLUT_ENTRIES)
        .saturating_mul(size_of::<i32>())
        .saturating_add(beside)
}

impl Pipeline {
    /// How many channels the pipeline takes.
    pub fn inputs(&self) -> usize {
        self.inputs
    }

    /// The bytes the pipeline holds: its own fields, its intervals, its
    /// stages, and each stage's sampled curves, coefficients and offsets, or
    /// grid counts and values.
    pub(super) fn held_bytes(&self) -> usize {
        let stages = size_of::<Pipeline>()
            + self.domain.capacity() * size_of::<(i64, i64)>()
            + self.stages.capacity() * size_of::<Stage>();
        self.stages
            .iter()
            .map(Stage::held_bytes)
            .fold(stages, usize::saturating_add)
    }

    /// Whether the connection space it gives or takes is `Lab` rather than
    /// `XYZ`.
    pub fn lab(&self) -> bool {
        self.lab
    }

    /// How many channels the pipeline gives.
    pub fn outputs(&self) -> usize {
        self.outputs
    }

    /// The connection-space value for `components`, each clamped to `0..=1`,
    /// in its own units: XYZ with the PCS white's `Y` at one, or `L*` from
    /// nought to a hundred and `a*` and `b*` as they are (6.3.4.2).
    ///
    /// Integer throughout: the floats are the caller's components on the way
    /// in and the answer on the way out.
    pub fn evaluate(&self, components: &[f64]) -> [f64; 3] {
        let values = self.run(components);
        [0, 1, 2].map(|channel| values.get(channel).copied().unwrap_or(0) as f64 / ONE as f64)
    }

    /// Every channel the pipeline gives `components`, each held to its
    /// input's interval on the way in — a device's values, or a
    /// connection-space value in its own units — in the order the tag's last
    /// element gives them.
    pub fn evaluate_all(&self, components: &[f64]) -> Vec<f64> {
        self.run(components)
            .into_iter()
            .map(|value| value as f64 / ONE as f64)
            .collect()
    }

    /// The stages over `components`, in s15.16: a component that is not a
    /// number is zero, and each is held to its input's interval.
    fn run(&self, components: &[f64]) -> Vec<i64> {
        let mut values: Vec<i64> = self
            .domain
            .iter()
            .enumerate()
            .map(|(channel, &(low, high))| {
                let value = components.get(channel).copied().unwrap_or(0.0);
                let value = if value.is_nan() { 0.0 } else { value };
                fixed(value).clamp(low, high)
            })
            .collect();
        for stage in &self.stages {
            values = stage.apply(&values);
        }
        values
    }
}

impl Stage {
    /// The bytes a stage holds beside its own size.
    fn held_bytes(&self) -> usize {
        match self {
            Stage::Curves(curves) => curves
                .iter()
                .map(|curve| curve.table.capacity() * size_of::<i32>())
                .fold(
                    curves.capacity() * size_of::<Sampled>(),
                    usize::saturating_add,
                ),
            Stage::Matrix {
                coefficients,
                offsets,
                ..
            } => (coefficients.capacity() + offsets.capacity()) * size_of::<i32>(),
            Stage::Grid { grids, values, .. } => {
                grids.capacity() * size_of::<usize>() + values.capacity() * size_of::<i32>()
            }
            Stage::Calculated {
                domain,
                grids,
                values,
                ..
            } => {
                domain.capacity() * size_of::<(i64, i64)>()
                    + grids.capacity() * size_of::<usize>()
                    + values.capacity() * size_of::<i32>()
            }
        }
    }

    /// The stage's outputs for `values`.
    fn apply(&self, values: &[i64]) -> Vec<i64> {
        match self {
            Stage::Curves(curves) => values
                .iter()
                .zip(curves)
                .map(|(&value, curve)| curve.at(value))
                .collect(),
            Stage::Matrix {
                inputs,
                outputs,
                coefficients,
                offsets,
            } => (0..*outputs)
                .map(|row| {
                    let mut sum = 0i128;
                    for (column, &value) in values.iter().enumerate().take(*inputs) {
                        let coefficient = coefficients.get(row * inputs + column).copied();
                        sum += i128::from(coefficient.unwrap_or(0)) * i128::from(value);
                    }
                    let offset = i128::from(offsets.get(row).copied().unwrap_or(0));
                    saturate(((sum + (1 << 15)) >> 16) + offset)
                })
                .collect(),
            Stage::Grid {
                grids,
                outputs,
                values: table,
            } => {
                // 10.14.2.4: an input outside `0..=1` reads the nearer end of
                // the range.
                let held: Vec<i64> = (0..grids.len())
                    .map(|axis| values.get(axis).copied().unwrap_or(0).clamp(0, ONE))
                    .collect();
                look_up(grids, *outputs, table, &held)
            }
            // A calculator's grid spans each input's interval: the value's
            // place along it, held to it, is where the grid is read.
            Stage::Calculated {
                domain,
                grids,
                outputs,
                values: table,
            } => {
                let held: Vec<i64> = domain
                    .iter()
                    .enumerate()
                    .map(|(axis, &(low, span))| {
                        if span <= 0 {
                            return 0;
                        }
                        let along = values.get(axis).copied().unwrap_or(0) - low;
                        ((i128::from(along) << 16) / i128::from(span)).clamp(0, i128::from(ONE))
                            as i64
                    })
                    .collect();
                look_up(grids, *outputs, table, &held)
            }
        }
    }
}

/// A grid of `table`, `grids` points along each axis and `outputs` values a
/// point, read at `held`, each coordinate in s15.16 over `0..=1`:
/// multilinearly, one axis at a time.
fn look_up(grids: &[usize], outputs: usize, table: &[i32], held: &[i64]) -> Vec<i64> {
    let mut base = Vec::with_capacity(grids.len());
    let mut fractions = Vec::with_capacity(grids.len());
    for (axis, &points) in grids.iter().enumerate() {
        let x = held.get(axis).copied().unwrap_or(0);
        let scaled = x * (points as i64 - 1);
        let cell = ((scaled >> 16) as usize).min(points - 2);
        base.push(cell);
        fractions.push(scaled - ((cell as i64) << 16));
    }
    let lookup = Lookup {
        grids,
        outputs,
        table,
        base: &base,
        fractions: &fractions,
    };
    (0..outputs)
        .map(|channel| lookup.corner(0, 0, channel))
        .collect()
}

/// One evaluation of a grid: the cell a colour falls in and how far along
/// each axis.
struct Lookup<'a> {
    grids: &'a [usize],
    outputs: usize,
    table: &'a [i32],
    base: &'a [usize],
    fractions: &'a [i64],
}

impl Lookup<'_> {
    /// Channel `channel` interpolated over the axes from `axis` on, the axes
    /// before it fixed at the grid index `offset` they make: linearly along
    /// `axis` between the cell's two nodes, each the same over the axes after
    /// it. Multilinear interpolation, one axis at a time, so no weight is a
    /// product of sixteen fractions; the depth is the input count, which
    /// `MAX_LUT_INPUTS` bounds.
    fn corner(&self, axis: usize, offset: usize, channel: usize) -> i64 {
        let Some(&points) = self.grids.get(axis) else {
            let at = offset * self.outputs + channel;
            return i64::from(self.table.get(at).copied().unwrap_or(0));
        };
        let cell = self.base.get(axis).copied().unwrap_or(0);
        let fraction = self.fractions.get(axis).copied().unwrap_or(0);
        let low = self.corner(axis + 1, offset * points + cell, channel);
        if fraction == 0 {
            return low;
        }
        let high = self.corner(
            axis + 1,
            offset * points + (cell + 1).min(points - 1),
            channel,
        );
        let step = (i128::from(high - low) * i128::from(fraction) + (1 << 15)) >> 16;
        low + step as i64
    }
}

impl Sampled {
    /// The curve at `value`, interpolated linearly between its samples.
    fn at(&self, value: i64) -> i64 {
        let last = self.table.len().saturating_sub(1);
        if self.span <= 0 || last == 0 {
            return i64::from(self.table.first().copied().unwrap_or(0));
        }
        let along = i128::from((value - self.low).clamp(0, self.span)) * last as i128;
        let span = i128::from(self.span);
        let index = (along / span) as usize;
        if index >= last {
            return i64::from(self.table[last]);
        }
        let remainder = along % span;
        let a = i128::from(self.table[index]);
        let b = i128::from(self.table[index + 1]);
        (a + ((b - a) * remainder + span / 2).div_euclid(span)) as i64
    }

    /// The interval of what the curve gives: its samples', since it
    /// interpolates between them.
    fn range(&self) -> (f64, f64) {
        let low = self.table.iter().copied().min().unwrap_or(0);
        let high = self.table.iter().copied().max().unwrap_or(0);
        (low as f64 / ONE as f64, high as f64 / ONE as f64)
    }

    /// `curve` sampled at [`CURVE_SAMPLES`] evenly spaced points over
    /// `interval`, in s15.16; one point where the interval is one.
    fn of(curve: &Curve, interval: (f64, f64)) -> Result<Sampled, IccError> {
        let low = fixed(interval.0);
        let span = (fixed(interval.1) - low).max(0);
        let points = if span == 0 { 1 } else { CURVE_SAMPLES };
        let mut table = Vec::with_capacity(points);
        for index in 0..points {
            let at = if points == 1 {
                low
            } else {
                low + (i128::from(span) * index as i128 / (points as i128 - 1)) as i64
            };
            let value = curve.eval(at as f64 / ONE as f64);
            // A curve that is not a number over the inputs it is given
            // converts nothing those inputs could mean.
            if !value.is_finite() {
                return Err(IccError::MalformedTag);
            }
            table.push(fixed32(value));
        }
        Ok(Sampled { low, span, table })
    }
}

/// A curve set (10.14.2.2, ICC.2 11.2.2): one curve a channel, each sampled
/// over its channel's input interval, which becomes the interval of what it
/// gives.
fn curve_set(
    element: &[u8],
    intervals: &mut [(f64, f64)],
    budget: &mut Budget,
) -> Result<Stage, IccError> {
    let reader = Reader { bytes: element };
    let mut curves = Vec::with_capacity(intervals.len());
    for (channel, interval) in intervals.iter_mut().enumerate() {
        let at = 12 + 8 * channel;
        let offset = reader.u32(at).ok_or(IccError::MalformedTag)? as usize;
        let size = reader.u32(at + 4).ok_or(IccError::MalformedTag)? as usize;
        let data = offset
            .checked_add(size)
            .and_then(|end| element.get(offset..end))
            .ok_or(IccError::MalformedTag)?;
        let curve = read_curve(data, budget, &mut Calculator::read)?;
        budget.spend_entries(CURVE_SAMPLES)?;
        let sampled = Sampled::of(&curve, *interval)?;
        *interval = sampled.range();
        curves.push(sampled);
    }
    Ok(Stage::Curves(curves))
}

/// A matrix element (10.14.2.3): `q` rows of the `p` coefficients that make
/// one output, then `q` offsets, `Y = E X + e` (equation 21).
fn matrix(element: &[u8], q: usize, intervals: &mut Vec<(f64, f64)>) -> Result<Stage, IccError> {
    let p = intervals.len();
    let reader = Reader { bytes: element };
    let float = |index: usize| float32(&reader, 12 + 4 * index);
    let mut coefficients = Vec::with_capacity(p * q);
    for index in 0..p * q {
        coefficients.push(fixed32(float(index)?));
    }
    let mut offsets = Vec::with_capacity(q);
    for row in 0..q {
        offsets.push(fixed32(float(p * q + row)?));
    }
    // What each row can give, from what each column can take.
    let next: Vec<(f64, f64)> = (0..q)
        .map(|row| {
            let offset = f64::from(offsets[row]) / ONE as f64;
            let (mut low, mut high) = (offset, offset);
            for (column, &(from, to)) in intervals.iter().enumerate() {
                let coefficient = f64::from(coefficients[row * p + column]) / ONE as f64;
                let (a, b) = (coefficient * from, coefficient * to);
                low += a.min(b);
                high += a.max(b);
            }
            (low, high)
        })
        .collect();
    *intervals = next;
    Ok(Stage::Matrix {
        inputs: p,
        outputs: q,
        coefficients,
        offsets,
    })
}

/// A lookup-table element — ICC.1's `clut` (10.14.2.4) or ICC.2's `xclt`
/// (11.2.7) — whose sixteen grid counts start at `layout`'s first offset, of
/// which the first `p` are used, and whose values start at its second, `q`
/// a point in its encoding.
fn grid(
    element: &[u8],
    q: usize,
    intervals: &mut Vec<(f64, f64)>,
    budget: &mut Budget,
    layout: (usize, usize, Values),
) -> Result<Stage, IccError> {
    let p = intervals.len();
    // Sixteen grid counts, so no more inputs than `MAX_LUT_INPUTS`, which a
    // spectrum's channels may pass.
    if p > MAX_LUT_INPUTS {
        return Err(IccError::TooLarge);
    }
    let (grids, read) = grid_layout(element, p, q, layout, budget.entries)?;
    budget.spend_entries(read.len())?;
    let values: Vec<i32> = read.into_iter().map(fixed32).collect();
    // Interpolation mixes nodes, so each output stays within its nodes.
    let next: Vec<(f64, f64)> = (0..q)
        .map(|channel| {
            let mut column = values.iter().skip(channel).step_by(q).copied();
            let first = column.next().unwrap_or(0);
            let (low, high) = column.fold((first, first), |(low, high), value| {
                (low.min(value), high.max(value))
            });
            (f64::from(low) / ONE as f64, f64::from(high) / ONE as f64)
        })
        .collect();
    *intervals = next;
    Ok(Stage::Grid {
        grids,
        outputs: q,
        values,
    })
}

/// A grid element's counts and values, in `f64` as its encoding states them
/// (`layout`: where its sixteen counts start, where its values start, and
/// their encoding), over `p` inputs to `q` outputs, its entries held to
/// `budget` and [`MAX_CLUT_ENTRIES`] before any is read.
fn grid_layout(
    element: &[u8],
    p: usize,
    q: usize,
    layout: (usize, usize, Values),
    budget: usize,
) -> Result<(Vec<usize>, Vec<f64>), IccError> {
    let (counts_at, values_at, encoding) = layout;
    let mut grids = Vec::with_capacity(p);
    let mut points = 1usize;
    for axis in 0..p {
        let count = usize::from(
            *element
                .get(counts_at + axis)
                .ok_or(IccError::MalformedTag)?,
        );
        // 10.14.2.4: two points at least in every dimension.
        if count < 2 {
            return Err(IccError::MalformedTag);
        }
        points = points.checked_mul(count).ok_or(IccError::TooLarge)?;
        if points > MAX_CLUT_ENTRIES {
            return Err(IccError::TooLarge);
        }
        grids.push(count);
    }
    let entries = points.checked_mul(q).ok_or(IccError::TooLarge)?;
    if entries > MAX_CLUT_ENTRIES || entries > budget {
        return Err(IccError::TooLarge);
    }
    // The bytes are there before anything is allocated for them.
    let end = entries
        .checked_mul(encoding.width())
        .and_then(|bytes| bytes.checked_add(values_at))
        .ok_or(IccError::TooLarge)?;
    if end > element.len() {
        return Err(IccError::MalformedTag);
    }
    let reader = Reader { bytes: element };
    let mut values = Vec::with_capacity(entries);
    for index in 0..entries {
        values.push(encoding.read(&reader, values_at + encoding.width() * index)?);
    }
    Ok((grids, values))
}

/// A grid element — `clut` or `xclt` — of `p` inputs and `q` outputs as a
/// calculator's sub-element reads it: its counts and its values in `f64`,
/// its entries spent from `budget` before any is read.
pub(super) fn grid_values(
    element: &[u8],
    p: usize,
    q: usize,
    budget: &mut Budget,
) -> Result<(Vec<usize>, Vec<f64>), IccError> {
    let reader = Reader { bytes: element };
    let layout = match reader.sig(0).as_ref() {
        Some(b"clut") => (12, 28, Values::Float32),
        Some(b"xclt") => (
            16,
            32,
            reader
                .u32(12)
                .and_then(Values::of)
                .ok_or(IccError::MalformedTag)?,
        ),
        _ => return Err(IccError::MalformedTag),
    };
    let (grids, values) = grid_layout(element, p, q, layout, budget.entries)?;
    budget.spend_entries(values.len())?;
    Ok((grids, values))
}

/// A matrix element of `p` inputs and `q` outputs as a calculator's
/// sub-element reads it: its `p × q` coefficients row by row, then its `q`
/// offsets, in `f64`, each finite.
pub(super) fn matrix_values(element: &[u8], p: usize, q: usize) -> Result<Vec<f64>, IccError> {
    let reader = Reader { bytes: element };
    let count = p * q + q;
    if 12 + 4 * count > element.len() {
        return Err(IccError::MalformedTag);
    }
    (0..count)
        .map(|index| float32(&reader, 12 + 4 * index))
        .collect()
}

/// The grid points along each axis a calculator's stage samples it at, by
/// how many inputs it takes (`docs/design/icc.md`, "Version 5"): 33 up to
/// three, 17 for four, 9 for five and six, 5 beyond.
fn calculator_points(inputs: usize) -> usize {
    match inputs {
        0..=3 => 33,
        4 => 17,
        5 | 6 => 9,
        _ => 5,
    }
}

/// A calculator element (ICC.2 11.2.1) as a stage: its program read and
/// validated, then run at every node of a grid over its inputs' intervals —
/// [`calculator_points`] an axis — the grid's entries held to `budget` and
/// [`MAX_CLUT_ENTRIES`], and the runs to the steps `budget` has left, before
/// any node is run. Each output is held to s15.16 as every stage's is, a
/// value that is not a number taken as zero; the interval of each is its
/// nodes'.
fn calculated(
    element: &[u8],
    q: usize,
    intervals: &mut Vec<(f64, f64)>,
    budget: &mut Budget,
) -> Result<Stage, IccError> {
    let p = intervals.len();
    let calculator = Calculator::read(element, budget)?;
    if calculator.inputs() != p || calculator.outputs() != q {
        return Err(IccError::MalformedTag);
    }
    let points = calculator_points(p);
    let nodes = (0..p).try_fold(1usize, |nodes, _| nodes.checked_mul(points));
    let entries = nodes.and_then(|nodes| nodes.checked_mul(q));
    if entries.is_none_or(|entries| entries > MAX_CLUT_ENTRIES || entries > budget.entries) {
        return Err(IccError::TooLarge);
    }
    let nodes = nodes.unwrap_or(0);
    let entries = entries.unwrap_or(0);
    budget.spend_runs(nodes, calculator.steps())?;
    budget.spend_entries(entries)?;
    let domain: Vec<(i64, i64)> = intervals
        .iter()
        .map(|&(low, high)| {
            let low_fixed = fixed(low);
            (low_fixed, (fixed(high) - low_fixed).max(0))
        })
        .collect();
    let mut values = Vec::with_capacity(entries);
    let mut input = vec![0.0f64; p];
    for node in 0..nodes {
        // The node's place along each axis, the first axis slowest.
        let mut rest = node;
        for axis in (0..p).rev() {
            let index = rest % points;
            rest /= points;
            let (low, span) = domain[axis];
            input[axis] =
                (low as f64 + span as f64 * index as f64 / (points - 1) as f64) / ONE as f64;
        }
        for value in calculator.evaluate(&input) {
            values.push(fixed32(value));
        }
    }
    let next: Vec<(f64, f64)> = (0..q)
        .map(|channel| {
            let mut column = values.iter().skip(channel).step_by(q).copied();
            let first = column.next().unwrap_or(0);
            let (low, high) = column.fold((first, first), |(low, high), value| {
                (low.min(value), high.max(value))
            });
            (f64::from(low) / ONE as f64, f64::from(high) / ONE as f64)
        })
        .collect();
    *intervals = next;
    Ok(Stage::Calculated {
        domain,
        grids: vec![points; p],
        outputs: q,
        values,
    })
}

/// One curve of a curve set read from `data`: a segmented curve, a single
/// sampled curve, or a sampled calculator curve (`clcf`, ICC.2 11.2.2.3),
/// whose calculator `calculator` reads and which is sampled at once into a
/// single sampled curve of its recommended entries — held to
/// [`CURVE_SAMPLES`], the resolution every curve is sampled at after — over
/// its range, clipped or extended past it as its extension type says. Its
/// calculator's runs are spent from `budget` before any is made.
pub(super) fn read_curve(
    data: &[u8],
    budget: &mut Budget,
    calculator: &mut dyn FnMut(&[u8], &mut Budget) -> Result<Calculator, IccError>,
) -> Result<Curve, IccError> {
    let reader = Reader { bytes: data };
    match reader.sig(0).as_ref() {
        Some(b"curf") => Ok(Curve::Segmented(segmented(data)?)),
        Some(b"sngf") => Ok(Curve::Single(single_sampled(data)?)),
        Some(b"clcf") => {
            let recommended = reader.u32(8).ok_or(IccError::MalformedTag)? as usize;
            let first = float32(&reader, 12)?;
            let last = float32(&reader, 16)?;
            let extension = reader.u16(20).ok_or(IccError::MalformedTag)?;
            if recommended < 2 || first >= last || extension > 1 {
                return Err(IccError::MalformedTag);
            }
            let program = calculator(data.get(24..).ok_or(IccError::MalformedTag)?, budget)?;
            // 11.2.2.3: the embedded calculator takes one channel and gives
            // one.
            if program.inputs() != 1 || program.outputs() != 1 {
                return Err(IccError::MalformedTag);
            }
            let entries = recommended.min(CURVE_SAMPLES);
            budget.spend_runs(entries, program.steps())?;
            let values = (0..entries)
                .map(|index| {
                    let x = first + (last - first) * index as f64 / (entries - 1) as f64;
                    program.evaluate(&[x]).first().copied().unwrap_or(0.0)
                })
                .collect();
            Ok(Curve::Single(SingleSampled {
                first,
                last,
                extrapolate: extension == 1,
                values,
            }))
        }
        _ => Err(IccError::MalformedTag),
    }
}

/// A single sampled curve (`sngf`, ICC.2 11.2.2.2): its entry count, the
/// inputs of its first and last entries, how it extends past them and how
/// its entries are encoded, then the entries. Two entries at least, the
/// first input below the last, and each entry finite.
fn single_sampled(data: &[u8]) -> Result<SingleSampled, IccError> {
    let reader = Reader { bytes: data };
    let count = reader.u32(8).ok_or(IccError::MalformedTag)? as usize;
    let first = float32(&reader, 12)?;
    let last = float32(&reader, 16)?;
    let extension = reader.u16(20).ok_or(IccError::MalformedTag)?;
    let encoding = reader
        .u16(22)
        .and_then(|field| Values::of(u32::from(field)))
        .ok_or(IccError::MalformedTag)?;
    if count < 2 || first >= last || extension > 1 {
        return Err(IccError::MalformedTag);
    }
    if count > MAX_CLUT_ENTRIES {
        return Err(IccError::TooLarge);
    }
    // The entries are there before anything is allocated for them.
    let end = count
        .checked_mul(encoding.width())
        .and_then(|bytes| bytes.checked_add(24))
        .ok_or(IccError::TooLarge)?;
    if end > data.len() {
        return Err(IccError::MalformedTag);
    }
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        values.push(encoding.read(&reader, 24 + encoding.width() * index)?);
    }
    Ok(SingleSampled {
        first,
        last,
        extrapolate: extension == 1,
        values,
    })
}

impl SingleSampled {
    /// The curve at `x`: between its first and last inputs, linearly between
    /// the two entries either side; beyond them, the end entry where it
    /// clips, and the line through the two end entries where it extends.
    fn eval(&self, x: f64) -> f64 {
        let last = self.values.len() - 1;
        let step = (self.last - self.first) / last as f64;
        let line = |from: usize, x: f64| {
            let (a, b) = (self.values[from], self.values[from + 1]);
            let at = self.first + step * from as f64;
            a + (b - a) * (x - at) / step
        };
        if x < self.first {
            return if self.extrapolate {
                line(0, x)
            } else {
                self.values[0]
            };
        }
        if x > self.last {
            return if self.extrapolate {
                line(last - 1, x)
            } else {
                self.values[last]
            };
        }
        let position = (x - self.first) / step;
        let below = (position.floor() as usize).min(last - 1);
        let fraction = position - below as f64;
        let (a, b) = (self.values[below], self.values[below + 1]);
        a + (b - a) * fraction
    }
}

/// A segmented curve (`curf`): a segment count, `count - 1` break-points,
/// then the segments, the first and last a formula (10.14.2.2, ICC.2
/// 11.2.2.4).
fn segmented(data: &[u8]) -> Result<Segmented, IccError> {
    let reader = Reader { bytes: data };
    if reader.sig(0) != Some(*b"curf") {
        return Err(IccError::MalformedTag);
    }
    let count = usize::from(reader.u16(8).ok_or(IccError::MalformedTag)?);
    if count == 0 {
        return Err(IccError::MalformedTag);
    }
    // The break-points are there, and room for the segments after them —
    // sixteen bytes each at the least, a `samf` of one sample — before
    // either is allocated.
    let mut at = 12 + 4 * (count - 1);
    if at + 16 * count > data.len() {
        return Err(IccError::MalformedTag);
    }
    let mut breaks = Vec::with_capacity(count - 1);
    for index in 0..count - 1 {
        let point = float32(&reader, 12 + 4 * index)?;
        // "Successive break-points shall not be decreasing."
        if breaks.last().is_some_and(|&before| point < before) {
            return Err(IccError::MalformedTag);
        }
        breaks.push(point);
    }
    let mut segments = Vec::with_capacity(count);
    for index in 0..count {
        let signature = reader.sig(at).ok_or(IccError::MalformedTag)?;
        let segment = match &signature {
            b"parf" => {
                let function = reader.u16(at + 8).ok_or(IccError::MalformedTag)?;
                let needed = formula_parameters(function).ok_or(IccError::MalformedTag)?;
                let mut params = [0.0; 7];
                for (slot, param) in params.iter_mut().enumerate().take(needed) {
                    *param = float32(&reader, at + 12 + 4 * slot)?;
                }
                at += 12 + 4 * needed;
                Segment::Formula { function, params }
            }
            // "The first and last curve segments shall be specified in terms
            // of a formula."
            b"samf" if index > 0 && index < count - 1 => {
                let samples = reader.u32(at + 8).ok_or(IccError::MalformedTag)? as usize;
                if samples == 0 {
                    return Err(IccError::MalformedTag);
                }
                let end = samples
                    .checked_mul(4)
                    .and_then(|bytes| bytes.checked_add(at + 12))
                    .ok_or(IccError::TooLarge)?;
                if end > data.len() {
                    return Err(IccError::MalformedTag);
                }
                let mut values = Vec::with_capacity(samples);
                for sample in 0..samples {
                    values.push(float32(&reader, at + 12 + 4 * sample)?);
                }
                at = end;
                Segment::Samples(values)
            }
            _ => return Err(IccError::MalformedTag),
        };
        segments.push(segment);
    }
    // Each segment's start, in order, so a sampled segment that begins where
    // another sampled one ends reads that one's last value without recursion.
    let mut curve = Segmented {
        breaks,
        segments,
        starts: Vec::with_capacity(count),
    };
    curve.starts.push(0.0);
    for index in 1..count {
        let start = curve.ends_at(index - 1);
        curve.starts.push(start);
    }
    Ok(curve)
}

/// How many parameters a formula segment of `function` carries: ICC.1:2010
/// Table 57's three and ICC.2:2023 Table 111's five more. Table 111 gives
/// function type 6 a field of 24 bytes and lists seven parameters, `ω`,
/// `γ`, `a` to `e`, which its formula reads all of: seven are read.
fn formula_parameters(function: u16) -> Option<usize> {
    Some(match function {
        0 => 4,
        1..=4 => 5,
        5 | 7 => 6,
        6 => 7,
        _ => return None,
    })
}

impl Segmented {
    /// Segment `index` at its own last point, the break-point after it.
    fn ends_at(&self, index: usize) -> f64 {
        match self.segments.get(index) {
            Some(Segment::Samples(values)) => values.last().copied().unwrap_or(0.0),
            Some(Segment::Formula { .. }) => {
                let point = self.breaks.get(index).copied().unwrap_or(0.0);
                self.value(index, point)
            }
            None => 0.0,
        }
    }

    /// The curve at `x`, by its [`Segmented::segment`].
    fn eval(&self, x: f64) -> f64 {
        self.value(self.segment(x), x)
    }

    /// Which segment `x` falls in: the first whose range holds it — the
    /// first up to and including the first break-point, each other from the
    /// break-point before it, excluded, to the one after it, included — and
    /// the last for a value that is not a number. The break-points never
    /// decrease, so it is found by halving them, whatever their count.
    fn segment(&self, x: f64) -> usize {
        if x.is_nan() {
            self.breaks.len()
        } else {
            self.breaks.partition_point(|&point| point < x)
        }
    }

    /// The numbers it keeps: its break-points, each segment's start, a
    /// formula's parameters and a sampled segment's samples.
    fn held(&self) -> usize {
        let segments: usize = self
            .segments
            .iter()
            .map(|segment| match segment {
                Segment::Formula { params, .. } => params.len(),
                Segment::Samples(values) => values.len(),
            })
            .sum();
        self.breaks.len() + self.starts.len() + segments
    }

    /// Segment `index` at `x`.
    fn value(&self, index: usize, x: f64) -> f64 {
        match self.segments.get(index) {
            Some(Segment::Formula { function, params }) => formula(*function, params, x),
            Some(Segment::Samples(values)) => {
                // An interior segment, so both its break-points exist. Its
                // `j`th sample stands at `from + j (to - from) / n`, and the
                // value it starts from at `from` is the segment before it's.
                let from = self
                    .breaks
                    .get(index.wrapping_sub(1))
                    .copied()
                    .unwrap_or(0.0);
                let to = self.breaks.get(index).copied().unwrap_or(from);
                let count = values.len();
                if to <= from || count == 0 {
                    return values.last().copied().unwrap_or(0.0);
                }
                let position = ((x - from) / (to - from) * count as f64).clamp(0.0, count as f64);
                let below = (position.floor() as usize).min(count - 1);
                let fraction = position - below as f64;
                let start = if below == 0 {
                    self.starts.get(index).copied().unwrap_or(0.0)
                } else {
                    values[below - 1]
                };
                start + (values[below] - start) * fraction
            }
            None => f64::NAN,
        }
    }
}

/// A formula segment of `function` at `x`, its parameters in the order the
/// tables list them: ICC.1:2010 Table 57's three and ICC.2:2023 Table 111's
/// five more.
fn formula(function: u16, params: &[f64; 7], x: f64) -> f64 {
    use tinker_pdf_math::{exp, ln, log10, pow};
    match function {
        // Y = (a X + b)^γ + c
        0 => {
            let [gamma, a, b, c, ..] = *params;
            pow(a * x + b, gamma) + c
        }
        // Y = a log10(b X^γ + c) + d
        1 => {
            let [gamma, a, b, c, d, ..] = *params;
            a * log10(b * pow(x, gamma) + c) + d
        }
        // Y = a b^(c X + d) + e
        2 => {
            let [a, b, c, d, e, ..] = *params;
            a * pow(b, c * x + d) + e
        }
        // Y = a (b X + c)^γ + d
        3 => {
            let [gamma, a, b, c, d, ..] = *params;
            a * pow(b * x + c, gamma) + d
        }
        // Y = a ln(d X^γ − b) + c
        4 => {
            let [gamma, a, b, c, d, ..] = *params;
            a * ln(d * pow(x, gamma) - b) + c
        }
        // Y = e exp((d X^γ − c) / a) + b
        5 => {
            let [gamma, a, b, c, d, e, _] = *params;
            e * exp((d * pow(x, gamma) - c) / a) + b
        }
        // Y = d (max(e X^γ − a, 0) / (b − c X^γ))^ω
        6 => {
            let [omega, gamma, a, b, c, d, e] = *params;
            let raised = pow(x, gamma);
            d * pow((e * raised - a).max(0.0) / (b - c * raised), omega)
        }
        // Y = d ((a + b X^γ) / (1 + c X^γ))^ω
        7 => {
            let [omega, gamma, a, b, c, d, _] = *params;
            let raised = pow(x, gamma);
            d * pow((a + b * raised) / (1.0 + c * raised), omega)
        }
        _ => f64::NAN,
    }
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

/// A real number in s15.16, as the stages carry it.
fn fixed(value: f64) -> i64 {
    i64::from(fixed32(value))
}

/// A real number in s15.16, saturating at the format's range rather than
/// wrapping: a value past thirty-two thousand is no colour.
fn fixed32(value: f64) -> i32 {
    let scaled = (value * ONE as f64).round();
    if scaled >= f64::from(i32::MAX) {
        i32::MAX
    } else if scaled <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        // NaN compares false above and casts to zero.
        scaled as i32
    }
}

/// A sum held to s15.16's range.
fn saturate(value: i128) -> i64 {
    value.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `mpet` header declaring `count` elements over three channels each
    /// way, and nothing after it.
    fn header(count: u32) -> Vec<u8> {
        let mut out = b"mpet".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&3u16.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        out
    }

    /// **An element count past the cap is refused before the table is
    /// walked** (ruling 1). The row `crates/tinker-pdf/tests/bounds_ledger.rs`
    /// names as proving [`MAX_MPE_ELEMENTS`]: a 16-byte tag — its header and
    /// nothing after it — declaring one element past the cap, or four
    /// billion, is `IccError::TooLarge` in each direction a tag runs, where a
    /// reader that walked first would find the position table missing and
    /// answer `MalformedTag`; and one declaring exactly the cap is admitted
    /// to the walk, and refused by it.
    ///
    /// No clock. The assertion is the typed refusal.
    #[test]
    fn an_element_count_past_the_cap_is_refused_before_the_table_is_walked() {
        for count in [MAX_MPE_ELEMENTS as u32 + 1, u32::MAX] {
            let past = header(count);
            assert_eq!(past.len(), 16);
            assert_eq!(read(&past, false), Err(IccError::TooLarge), "{count}");
            assert_eq!(
                read_output(&past, true, 3),
                Err(IccError::TooLarge),
                "{count}, out of the connection space"
            );
            assert_eq!(
                read_link(&past, 3, 3),
                Err(IccError::TooLarge),
                "{count}, in a link"
            );
        }
        assert_eq!(
            read(&header(MAX_MPE_ELEMENTS as u32), false),
            Err(IccError::MalformedTag),
            "the cap itself is walked, and its table is not there"
        );
    }

    /// **A half float reads as IEEE 754 has it** (ICC.2 4.2.2), and an
    /// encoding Table 8 does not list is none.
    #[test]
    fn the_value_encodings_read_as_table_8_has_them() {
        let bytes = [
            0x3C, 0x00, // 1.0 as a half float
            0xC1, 0x00, // -2.5
            0x00, 0x01, // the least subnormal, 2^-24
            0x7C, 0x00, // infinity, which no table may hold
            0xFF, 0xFF, // 65 535 as an unsigned 16-bit value
            0x80, // 128 as an unsigned byte
        ];
        let reader = Reader { bytes: &bytes };
        assert_eq!(Values::Float16.read(&reader, 0), Ok(1.0));
        assert_eq!(Values::Float16.read(&reader, 2), Ok(-2.5));
        assert_eq!(Values::Float16.read(&reader, 4), Ok(1.0 / 16_777_216.0));
        assert_eq!(
            Values::Float16.read(&reader, 6),
            Err(IccError::MalformedTag)
        );
        assert_eq!(Values::Unsigned16.read(&reader, 8), Ok(1.0));
        assert_eq!(Values::Unsigned8.read(&reader, 10), Ok(128.0 / 255.0));
        assert_eq!(Values::of(4), None);
        assert_eq!(
            [0, 1, 2, 3].map(|field| Values::of(field).map(Values::width)),
            [Some(4), Some(2), Some(2), Some(1)]
        );
    }

    /// `value` as the float32 an element stores it in.
    fn float(value: f64) -> [u8; 4] {
        (value as f32).to_bits().to_be_bytes()
    }

    /// What a float32 field holding `value` reads back as.
    fn stored(value: f64) -> f64 {
        f64::from(value as f32)
    }

    /// A one-segment `curf` of a formula of `function` over `params`.
    fn formula_curve(function: u16, params: &[f64]) -> Vec<u8> {
        let mut out = b"curf".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&[0; 2]);
        out.extend_from_slice(b"parf");
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&function.to_be_bytes());
        out.extend_from_slice(&[0; 2]);
        for &param in params {
            out.extend_from_slice(&float(param));
        }
        out
    }

    /// A `sngf` of `count` entries whose bytes are `body`, in `encoding`,
    /// from `first` to `last`, extended where `extend` is one.
    fn single(
        first: f64,
        last: f64,
        extend: u16,
        encoding: u16,
        count: u32,
        body: &[u8],
    ) -> Vec<u8> {
        let mut out = b"sngf".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&float(first));
        out.extend_from_slice(&float(last));
        out.extend_from_slice(&extend.to_be_bytes());
        out.extend_from_slice(&encoding.to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    /// **ICC.2's formula segments are their closed forms** (Table 111): each
    /// of function types 3 to 7, read from its `parf`, gives at points across
    /// its domain what its formula gives the stored parameters; and ICC.1's
    /// three keep theirs. A function type past 7 is malformed, and so is a
    /// segment whose parameters the curve's bytes cannot hold.
    #[test]
    fn icc_2s_formula_segments_are_their_closed_forms() {
        type Closed = fn(&[f64], f64) -> f64;
        let cases: [(u16, Vec<f64>, Closed); 8] = [
            (0, vec![2.2, 0.9, 0.1, 0.05], |p, x| {
                (p[1] * x + p[2]).powf(p[0]) + p[3]
            }),
            (1, vec![1.5, 0.4, 8.0, 1.0, 0.1], |p, x| {
                p[1] * (p[2] * x.powf(p[0]) + p[3]).log10() + p[4]
            }),
            (2, vec![0.3, 2.5, 1.2, -0.5, 0.05], |p, x| {
                p[0] * p[1].powf(p[2] * x + p[3]) + p[4]
            }),
            (3, vec![2.4, 0.8, 1.1, 0.05, 0.02], |p, x| {
                p[1] * (p[2] * x + p[3]).powf(p[0]) + p[4]
            }),
            (4, vec![1.2, 0.3, -1.5, 0.1, 2.0], |p, x| {
                p[1] * (p[4] * x.powf(p[0]) - p[2]).ln() + p[3]
            }),
            (5, vec![1.1, 2.0, -0.2, 0.3, 1.5, 0.25], |p, x| {
                p[5] * ((p[4] * x.powf(p[0]) - p[3]) / p[1]).exp() + p[2]
            }),
            (6, vec![0.7, 1.3, 0.1, 2.5, 1.0, 0.6, 1.2], |p, x| {
                let raised = x.powf(p[1]);
                p[5] * ((p[6] * raised - p[2]).max(0.0) / (p[3] - p[4] * raised)).powf(p[0])
            }),
            (7, vec![1.4, 0.9, 0.05, 0.8, 0.6, 0.9], |p, x| {
                let raised = x.powf(p[1]);
                p[5] * ((p[2] + p[3] * raised) / (1.0 + p[4] * raised)).powf(p[0])
            }),
        ];
        for (function, params, closed) in cases {
            let curve = segmented(&formula_curve(function, &params))
                .unwrap_or_else(|error| panic!("type {function}: {error:?}"));
            let stored_params: Vec<f64> = params.iter().map(|&v| stored(v)).collect();
            for step in 0..=20u32 {
                let x = f64::from(step) / 20.0;
                let (got, want) = (curve.eval(x), closed(&stored_params, x));
                assert!(
                    (got - want).abs() <= 1e-12 * want.abs().max(1.0),
                    "type {function} at {x}: {got} against {want}"
                );
            }
        }
        assert_eq!(
            segmented(&formula_curve(8, &[1.0; 7])).err(),
            Some(IccError::MalformedTag),
            "a function type past 7"
        );
        let mut short = formula_curve(6, &[1.0; 7]);
        short.truncate(short.len() - 8);
        assert_eq!(
            segmented(&short).err(),
            Some(IccError::MalformedTag),
            "seven parameters cut short"
        );
    }

    /// **A single sampled curve reads between its entries, and clips or
    /// extends past its ends** (ICC.2 11.2.2.2), in each encoding Table 8
    /// lists. Fewer than two entries, a first input not below the last, an
    /// extension or an encoding the clause does not list, entries the bytes
    /// cannot hold, and a half float that is not finite are each malformed.
    #[test]
    fn a_single_sampled_curve_clips_or_extends_past_its_ends() {
        let mut floats = Vec::new();
        for value in [0.0, 0.2, 0.6, 1.0] {
            floats.extend_from_slice(&float(value));
        }
        let clipped = single_sampled(&single(0.25, 1.75, 0, 0, 4, &floats)).expect("float32");
        let extended = single_sampled(&single(0.25, 1.75, 1, 0, 4, &floats)).expect("float32");
        // Entries at 0.25, 0.75, 1.25 and 1.75.
        for (x, inside) in [(0.25, 0.0), (0.5, 0.1), (1.0, 0.4), (1.5, 0.8), (1.75, 1.0)] {
            assert!((clipped.eval(x) - inside).abs() < 1e-6, "{x}");
            assert!((extended.eval(x) - inside).abs() < 1e-6, "{x}");
        }
        assert_eq!(
            clipped.eval(0.0),
            0.0,
            "below the first input, the first entry"
        );
        assert!(
            (clipped.eval(2.0) - 1.0).abs() < 1e-9,
            "past the last, the last"
        );
        assert!(
            (extended.eval(0.0) + 0.1).abs() < 1e-6,
            "the first two entries' line"
        );
        assert!(
            (extended.eval(2.0) - 1.2).abs() < 1e-6,
            "the last two entries' line"
        );

        let half = single_sampled(&single(0.0, 1.0, 0, 1, 2, &[0x34, 0x00, 0x3C, 0x00]))
            .expect("half floats");
        assert_eq!(half.eval(0.5), 0.625, "0.25 and 1.0, halfway");
        let sixteen = single_sampled(&single(0.0, 1.0, 0, 2, 2, &[0, 0, 0xFF, 0xFF])).expect("u16");
        assert_eq!(sixteen.eval(0.5), 0.5);
        let eight = single_sampled(&single(0.0, 1.0, 0, 3, 3, &[0, 0x80, 0xFF])).expect("u8");
        assert!((eight.eval(0.5) - 128.0 / 255.0).abs() < 1e-12);

        for (bytes, what) in [
            (single(0.0, 1.0, 0, 0, 1, &floats), "one entry"),
            (
                single(1.0, 1.0, 0, 0, 4, &floats),
                "a first input not below the last",
            ),
            (single(0.0, 1.0, 2, 0, 4, &floats), "an extension of two"),
            (single(0.0, 1.0, 0, 4, 4, &floats), "an encoding of four"),
            (
                single(0.0, 1.0, 0, 0, 5, &floats),
                "five entries in four's bytes",
            ),
            (
                single(0.0, 1.0, 0, 1, 2, &[0x7C, 0x00, 0x3C, 0x00]),
                "an infinite half float",
            ),
        ] {
            assert_eq!(
                single_sampled(&bytes).err(),
                Some(IccError::MalformedTag),
                "{what}"
            );
        }
    }

    /// An element's header, `p` inputs and `q` outputs, and `body`.
    fn element(signature: &[u8; 4], p: u16, q: u16, body: &[u8]) -> Vec<u8> {
        let mut out = signature.to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&p.to_be_bytes());
        out.extend_from_slice(&q.to_be_bytes());
        out.extend_from_slice(body);
        out
    }

    /// An `mpet` tag of `inputs` and `outputs` channels chaining `elements`.
    fn tag(inputs: u16, outputs: u16, elements: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"mpet".to_vec();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&inputs.to_be_bytes());
        out.extend_from_slice(&outputs.to_be_bytes());
        out.extend_from_slice(&(elements.len() as u32).to_be_bytes());
        let start = 16 + 8 * elements.len();
        let mut data = Vec::new();
        for element in elements {
            out.extend_from_slice(&((start + data.len()) as u32).to_be_bytes());
            out.extend_from_slice(&(element.len() as u32).to_be_bytes());
            data.extend_from_slice(element);
            while data.len() % 4 != 0 {
                data.push(0);
            }
        }
        out.extend(data);
        out
    }

    /// A curve set of `channels` copies of `curve`.
    fn curves_of(channels: usize, curve: &[u8]) -> Vec<u8> {
        let mut out = element(b"cvst", channels as u16, channels as u16, &[]);
        let start = 12 + 8 * channels;
        for channel in 0..channels {
            let at = start + channel * curve.len();
            out.extend_from_slice(&(at as u32).to_be_bytes());
            out.extend_from_slice(&(curve.len() as u32).to_be_bytes());
        }
        for _ in 0..channels {
            out.extend_from_slice(curve);
        }
        out
    }

    /// An `xclt` of a two-point grid over `p` inputs giving `q` outputs, its
    /// values in `encoding`'s bytes.
    fn extended(p: u16, q: u16, encoding: u32, values: &[u8]) -> Vec<u8> {
        let mut body = encoding.to_be_bytes().to_vec();
        let mut counts = [0u8; 16];
        counts[..usize::from(p)].fill(2);
        body.extend_from_slice(&counts);
        body.extend_from_slice(values);
        element(b"xclt", p, q, &body)
    }

    /// **An extended grid reads in each encoding Table 8 lists** (ICC.2
    /// 11.2.7): a two-point grid over one input of half floats, of 16-bit
    /// and of 8-bit fractions, each interpolated between its two nodes; an
    /// encoding the table does not list, values its bytes cannot hold and a
    /// half float that is not finite are malformed, and a grid past
    /// `MAX_CLUT_ENTRIES` too large.
    #[test]
    fn an_extended_grid_reads_in_each_encoding() {
        let link = Ends::Link {
            inputs: 1,
            outputs: 1,
        };
        for (encoding, values, low, high) in [
            (1u32, vec![0x34, 0x00, 0x3A, 0x00], 0.25, 0.75),
            (
                2,
                vec![0x40, 0x00, 0xC0, 0x00],
                16384.0 / 65535.0,
                49152.0 / 65535.0,
            ),
            (3, vec![0x40, 0xC0], 64.0 / 255.0, 192.0 / 255.0),
        ] {
            let pipeline = read_tag(
                &tag(1, 1, &[extended(1, 1, encoding, &values)]),
                false,
                link,
            )
            .unwrap_or_else(|error| panic!("encoding {encoding}: {error:?}"));
            let midway = pipeline.evaluate_all(&[0.5])[0];
            assert!(
                (midway - (low + high) / 2.0).abs() < 2.0 / 65536.0,
                "encoding {encoding}: {midway}"
            );
            assert!((pipeline.evaluate_all(&[0.0])[0] - low).abs() < 1.0 / 65536.0);
        }
        for (bytes, what) in [
            (extended(1, 1, 4, &[0; 8]), "an encoding of four"),
            (
                extended(1, 1, 0, &[0; 4]),
                "two float32 values in four bytes",
            ),
            (
                extended(1, 1, 1, &[0x7C, 0x00, 0, 0]),
                "an infinite half float",
            ),
        ] {
            assert_eq!(
                read_tag(&tag(1, 1, &[bytes]), false, link),
                Err(IccError::MalformedTag),
                "{what}"
            );
        }
        let mut wide = element(b"xclt", 16, 3, &0u32.to_be_bytes());
        wide.extend_from_slice(&[255; 16]);
        let identity = curves_of(16, &formula_curve(0, &[1.0, 1.0, 0.0, 0.0]));
        assert_eq!(
            read_tag(&tag(16, 3, &[identity, wide]), false, Ends::Forward),
            Err(IccError::TooLarge),
            "a grid of 255^16 points"
        );
    }

    /// **A tag out of the connection space takes the space's range**: a
    /// `B2D*` tag's first curve is sampled over XYZ from 0 to what
    /// `u1Fixed15` encodes, or over `L*` 0 to 100 and `a*`, `b*` −128 to 127,
    /// and a value past the range is held to it. Its counts are held as a
    /// `B2A*` table's are: more than fifteen outputs is too many before any
    /// element is read, another count than the data space's another space,
    /// and four inputs where the connection space has three malformed. A
    /// sampled calculator curve (`clcf`) too short to hold its header is
    /// malformed.
    #[test]
    fn a_tag_out_of_the_connection_space_takes_its_range() {
        let curves = curves_of(3, &formula_curve(0, &[1.0, 1.0, 0.0, 0.0]));
        let xyz = read_output(&tag(3, 3, &[curves.clone()]), false, 3).expect("XYZ in");
        assert_eq!(xyz.domain, vec![(0, 131_070); 3]);
        let got = xyz.evaluate_all(&[1.5, -1.0, 9.0]);
        assert!((got[0] - 1.5).abs() < 1e-3, "{got:?}");
        assert_eq!(got[1], 0.0);
        assert!(
            (got[2] - 65535.0 / 32768.0).abs() < 1e-3,
            "held to u1Fixed15's top: {got:?}"
        );
        let lab = read_output(&tag(3, 3, &[curves.clone()]), true, 3).expect("Lab in");
        assert_eq!(
            lab.domain,
            vec![
                (0, 100 << 16),
                (-128 << 16, 127 << 16),
                (-128 << 16, 127 << 16)
            ]
        );
        let got = lab.evaluate_all(&[50.0, -60.0, 90.0]);
        for (got, want) in got.iter().zip([50.0, -60.0, 90.0]) {
            assert!((got - want).abs() < 0.05, "{got} against {want}");
        }

        assert_eq!(
            read_output(&tag(3, 16, &[element(b"matf", 3, 16, &[])]), false, 15),
            Err(IccError::TooLarge)
        );
        assert_eq!(
            read_output(&tag(3, 3, &[curves.clone()]), false, 4),
            Err(IccError::UnsupportedSpace)
        );
        assert_eq!(
            read_output(&tag(4, 4, &[element(b"matf", 4, 4, &[0; 80])]), false, 4),
            Err(IccError::MalformedTag)
        );
        let calculated = curves_of(3, b"clcf\0\0\0\0");
        assert_eq!(
            read(&tag(3, 3, &[calculated]), false),
            Err(IccError::MalformedTag)
        );
    }

    /// **A value an element stores that is not finite is a typed refusal**:
    /// a matrix coefficient that is not a number, an infinite grid value and
    /// an infinite break-point are each malformed.
    #[test]
    fn a_non_finite_element_value_is_refused() {
        let mut nan = element(b"matf", 1, 1, &[]);
        nan.extend_from_slice(&f32::NAN.to_bits().to_be_bytes());
        nan.extend_from_slice(&float(0.0));
        let mut counts = [0u8; 16];
        counts[0] = 2;
        let mut infinite = element(b"clut", 1, 1, &counts);
        infinite.extend_from_slice(&float(0.0));
        infinite.extend_from_slice(&f32::INFINITY.to_bits().to_be_bytes());
        let link = Ends::Link {
            inputs: 1,
            outputs: 1,
        };
        for (bytes, what) in [
            (nan, "a matrix of NaN"),
            (infinite, "an infinite grid value"),
        ] {
            assert_eq!(
                read_tag(&tag(1, 1, &[bytes]), false, link),
                Err(IccError::MalformedTag),
                "{what}"
            );
        }
        let segment = formula_curve(0, &[1.0; 4])[12..].to_vec();
        let mut broken = b"curf".to_vec();
        broken.extend_from_slice(&[0; 4]);
        broken.extend_from_slice(&2u16.to_be_bytes());
        broken.extend_from_slice(&[0; 2]);
        broken.extend_from_slice(&f32::INFINITY.to_bits().to_be_bytes());
        broken.extend_from_slice(&segment);
        broken.extend_from_slice(&segment);
        assert_eq!(segmented(&broken).err(), Some(IccError::MalformedTag));
    }

    /// **A segmented curve finds its segment by halving its break-points**:
    /// over a curve of a thousand segments, some break-points repeated, the
    /// segment a value falls in is the one walking the break-points in
    /// order finds — the first whose break-point is at or above it, and the
    /// last past them all or for a value that is not a number — at every
    /// break-point, between them and beyond both ends, and each segment
    /// gives its own value there.
    #[test]
    fn a_segmented_curve_finds_its_segment_by_halving() {
        let count = 1000usize;
        // Break-point `i` at `i / 1000`, every tenth one repeated.
        let breaks: Vec<f64> = (1..count)
            .map(|index| stored((index - usize::from(index % 10 == 0)) as f64 / count as f64))
            .collect();
        let mut data = b"curf".to_vec();
        data.extend_from_slice(&[0; 4]);
        data.extend_from_slice(&(count as u16).to_be_bytes());
        data.extend_from_slice(&[0; 2]);
        for &point in &breaks {
            data.extend_from_slice(&float(point));
        }
        // Segment `i` is the constant `i`: (0 X + 1)^1 + (i − 1).
        for index in 0..count {
            data.extend_from_slice(&formula_curve(0, &[1.0, 0.0, 1.0, index as f64 - 1.0])[12..]);
        }
        let curve = segmented(&data).expect("the curve reads");
        let walked = |x: f64| {
            breaks
                .iter()
                .position(|&point| x <= point)
                .unwrap_or(breaks.len())
        };
        let mut probes: Vec<f64> = breaks.clone();
        probes.extend(breaks.iter().map(|&point| point + 1e-6));
        probes.extend(breaks.iter().map(|&point| point - 1e-6));
        probes.extend([-1.0, 0.0, 1.0, 2.0]);
        for x in probes {
            let index = curve.segment(x);
            assert_eq!(index, walked(x), "{x}");
            assert_eq!(curve.eval(x), index as f64, "{x}");
        }
        assert_eq!(curve.segment(f64::NEG_INFINITY), 0);
        assert_eq!(curve.segment(f64::INFINITY), count - 1);
        assert_eq!(curve.segment(f64::NAN), count - 1);
    }

    /// **A spectral tag gives as many channels as its spectrum has** (ICC.2
    /// 9.2.77): a matrix from three device channels to 36 wavelengths and a
    /// curve set over the 36, read as a spectral `D2B*` tag, end in the
    /// reduction's weights — each output the weighted sum of the spectrum
    /// the elements give — while the same tag read as a colorimetric one is
    /// refused for its 36 channels, as it always was; a grid after the 36
    /// is refused for its inputs, a tag giving another count than the
    /// spectrum's is refused for it, the matrix's coefficients and the
    /// reduction's are spent from the tag's entries, and what the read holds
    /// is within what its bound says before it.
    #[test]
    fn a_spectral_tag_gives_as_many_channels_as_its_spectrum() {
        let channels = 36usize;
        // Wavelength `w` of the spectrum: a quarter of the first device
        // channel, w / 72 of the second and a tenth of the third.
        let coefficients: Vec<[f64; 3]> = (0..channels)
            .map(|w| [0.25, w as f64 / 72.0, 0.1])
            .collect();
        let mut body = Vec::new();
        for row in &coefficients {
            for &value in row {
                body.extend_from_slice(&float(value));
            }
        }
        for _ in 0..channels {
            body.extend_from_slice(&float(0.0));
        }
        let matrix = element(b"matf", 3, channels as u16, &body);
        let curves = curves_of(channels, &formula_curve(0, &[1.0, 1.0, 0.0, 0.0]));
        let spectral = tag(3, channels as u16, &[matrix.clone(), curves.clone()]);
        // X weighs each wavelength by its number, Y by one, Z by nothing.
        let weights: Vec<f64> = (0..3 * channels)
            .map(|index| match index / channels {
                0 => (index % channels) as f64 / 1024.0,
                1 => 1.0 / 64.0,
                _ => 0.0,
            })
            .collect();
        let pipeline = read_spectral(&spectral, &weights).expect("the spectrum reduces");
        assert_eq!((pipeline.inputs(), pipeline.outputs()), (3, 3));
        assert!(!pipeline.lab());
        let x = [0.8, 0.5, 0.3];
        let spectrum: Vec<f64> = coefficients
            .iter()
            .map(|row| (0..3).map(|c| stored(row[c]) * x[c]).sum::<f64>())
            .collect();
        let want = [0, 1, 2].map(|row| {
            (0..channels)
                .map(|w| weights[row * channels + w] * spectrum[w])
                .sum::<f64>()
        });
        let got = pipeline.evaluate(&x);
        for channel in 0..3 {
            assert!(
                (got[channel] - want[channel]).abs() < 1e-3,
                "channel {channel}: {got:?} against {want:?}"
            );
        }
        assert!(pipeline.held_bytes() <= spectral_bound(&spectral, channels));

        // A colorimetric tag's elements still give no more than fifteen.
        let back = element(
            b"matf",
            channels as u16,
            3,
            &vec![0; (3 * channels + 3) * 4],
        );
        assert_eq!(
            read(&tag(3, 3, &[matrix.clone(), back]), false),
            Err(IccError::TooLarge)
        );
        assert_eq!(read(&spectral, false), Err(IccError::UnsupportedSpace));
        let counts = [2u8; 16];
        let grid_after = tag(
            3,
            channels as u16,
            &[
                matrix.clone(),
                element(b"clut", channels as u16, channels as u16, &counts),
            ],
        );
        assert_eq!(
            read_spectral(&grid_after, &weights),
            Err(IccError::TooLarge)
        );
        assert_eq!(
            read_spectral(&tag(3, channels as u16 - 1, &[matrix]), &weights),
            Err(IccError::UnsupportedSpace)
        );
    }
}
