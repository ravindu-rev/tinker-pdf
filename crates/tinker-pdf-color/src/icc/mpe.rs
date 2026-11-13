//! ICC.1's `multiProcessElementsType` (ICC.1:2010 10.14): the floating-point
//! transforms of the `D2B*` tags, read and compiled into integer stages.
//!
//! A tag is a chain of processing elements, each handing its channels to the
//! next unclipped (10.14.2.1): curve sets (`cvst`), matrices with offsets
//! (`matf`), multi-dimensional lookup tables (`clut`), and the `bACS` and
//! `eACS` placeholders, which pass their channels through (10.14.2.5). Their
//! numbers are float32 in the file. They are read once, when the pipeline is
//! built, and compiled into fixed-point stages — a curve sampled over the
//! interval its input can take, a matrix and a grid in s15.16 — so that a
//! colour's evaluation is integer arithmetic and nothing on the pixel path
//! rounds differently on another target (ruling 4). The formulas a curve
//! segment is made of are evaluated with `tinker_pdf_math` while it is
//! sampled, never per colour.
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
//! and sampled curve entries together — to `MAX_CLUT_ENTRIES` in all; a
//! segment or sample count is checked against the bytes that would hold it
//! before anything is read.

use std::mem::size_of;

use super::{IccError, Reader, CURVE_ENTRIES, MAX_CLUT_ENTRIES, MAX_LUT_INPUTS};

/// The most processing elements one `multiProcessElementsType` tag may chain.
///
/// The count is a 32-bit field and elements may share their data (10.14.1),
/// so a few bytes can name a long chain that costs work on every colour; the
/// cap bounds that work. A `D2B*` tag's chain is a handful of curve sets, a
/// matrix and a grid, and the cap clears it many times over.
pub const MAX_MPE_ELEMENTS: usize = 64;

/// How many points a curve set's curve is sampled at, over the interval its
/// input can take: [`CURVE_ENTRIES`], the resolution of a matrix/TRC
/// profile's compiled curves.
const CURVE_SAMPLES: usize = CURVE_ENTRIES;

/// One in s15.16, the fixed point every stage computes in.
const ONE: i64 = 1 << 16;

/// A `multiProcessElementsType` tag, compiled.
#[derive(Clone, Debug, PartialEq)]
pub struct Pipeline {
    /// How many channels go in.
    inputs: usize,
    /// The stages, in the order the tag lists its elements; the placeholders
    /// are left out, since they change nothing.
    stages: Vec<Stage>,
    /// Whether the connection space the last stage gives is `Lab` rather
    /// than `XYZ`, as the profile's header says.
    lab: bool,
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
}

/// A curve sampled at evenly spaced points over `low ..= low + span`, its
/// input interval, in s15.16; an input outside it reads the nearer end.
#[derive(Clone, Debug, PartialEq)]
struct Sampled {
    low: i64,
    span: i64,
    table: Vec<i32>,
}

/// A segmented curve (`curf`, 10.14.2.2), as the file states it.
struct Segmented {
    /// The break-points, `segments.len() - 1` of them, never decreasing.
    breaks: Vec<f64>,
    segments: Vec<Segment>,
    /// For each segment, the value a sampled one starts from: the segment
    /// before it at the break-point between them. Unused for the first.
    starts: Vec<f64>,
}

/// One segment of a segmented curve.
enum Segment {
    /// `parf`: a formula, by function type and its parameters in order.
    Formula { function: u16, params: [f64; 5] },
    /// `samf`: samples evenly spaced over the segment, its start excluded.
    Samples(Vec<f64>),
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
/// not one ICC.1 allows: a channel count that does not chain, a break-point
/// that decreases, a number that is not finite, a curve that is not finite
/// over the inputs it is given.
pub fn read(data: &[u8], lab: bool) -> Result<Pipeline, IccError> {
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
    // Three is what the connection space is, and what a `D2B*` tag gives.
    if outputs != 3 {
        return Err(IccError::UnsupportedSpace);
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
        if !matches!(&signature, b"cvst" | b"matf" | b"clut" | b"bACS" | b"eACS") {
            return Err(IccError::UnsupportedElement(signature));
        }
        elements.push((signature, element));
    }

    // Each element's input interval, starting from the device's: a colour's
    // components arrive clamped to `0..=1`.
    let mut intervals = vec![(0.0f64, 1.0f64); inputs];
    let mut budget = MAX_CLUT_ENTRIES;
    let mut stages = Vec::new();
    for (signature, element) in elements {
        let reader = Reader { bytes: element };
        let p = usize::from(reader.u16(8).ok_or(IccError::MalformedTag)?);
        let q = usize::from(reader.u16(10).ok_or(IccError::MalformedTag)?);
        // 10.14.2.1: an element takes the channels the one before it gives.
        if p != intervals.len() || q == 0 {
            return Err(IccError::MalformedTag);
        }
        if q > MAX_LUT_INPUTS {
            return Err(IccError::TooLarge);
        }
        match &signature {
            b"cvst" => {
                if q != p {
                    return Err(IccError::MalformedTag);
                }
                stages.push(curve_set(element, &mut intervals, &mut budget)?);
            }
            b"matf" => stages.push(matrix(element, q, &mut intervals)?),
            b"clut" => stages.push(grid(element, q, &mut intervals, &mut budget)?),
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
/// [`MAX_MPE_ELEMENTS`] elements. A channel count is read as at most
/// `MAX_LUT_INPUTS`, past which the read refuses the tag, and an element the
/// tag does not hold allocates nothing, since the read refuses it too.
pub(super) fn bound(data: &[u8]) -> usize {
    let reader = Reader { bytes: data };
    let count = reader
        .u32(12)
        .map_or(0, |count| count as usize)
        .min(MAX_MPE_ELEMENTS);
    let mut entries = 0usize;
    let mut beside = size_of::<Pipeline>() + MAX_MPE_ELEMENTS * size_of::<Stage>();
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
        let p = header.u16(8).map_or(0, usize::from).min(MAX_LUT_INPUTS);
        let q = header.u16(10).map_or(0, usize::from).min(MAX_LUT_INPUTS);
        match header.sig(0).as_ref() {
            Some(b"cvst") => {
                entries = entries.saturating_add(q * CURVE_SAMPLES);
                beside = beside.saturating_add(q * size_of::<Sampled>());
            }
            Some(b"matf") => beside = beside.saturating_add((p * q + q) * size_of::<i32>()),
            Some(b"clut") => {
                let points = (0..p)
                    .map(|axis| {
                        element
                            .get(12 + axis)
                            .map_or(0, |&count| usize::from(count))
                    })
                    .fold(1usize, usize::saturating_mul);
                entries = entries.saturating_add(points.saturating_mul(q));
                beside = beside.saturating_add(p * size_of::<usize>());
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

    /// The bytes the pipeline holds: its own fields, its stages, and each
    /// stage's sampled curves, coefficients and offsets, or grid counts and
    /// values.
    pub(super) fn held_bytes(&self) -> usize {
        let stages = size_of::<Pipeline>() + self.stages.capacity() * size_of::<Stage>();
        self.stages
            .iter()
            .map(Stage::held_bytes)
            .fold(stages, usize::saturating_add)
    }

    /// Whether the connection space it gives is `Lab` rather than `XYZ`.
    pub fn lab(&self) -> bool {
        self.lab
    }

    /// The connection-space value for `components`, each clamped to `0..=1`,
    /// in its own units: XYZ with the PCS white's `Y` at one, or `L*` from
    /// nought to a hundred and `a*` and `b*` as they are (6.3.4.2).
    ///
    /// Integer throughout: the floats are the caller's components on the way
    /// in and the answer on the way out.
    pub fn evaluate(&self, components: &[f64]) -> [f64; 3] {
        let mut values: Vec<i64> = (0..self.inputs)
            .map(|channel| {
                let value = components.get(channel).copied().unwrap_or(0.0);
                fixed(value.clamp(0.0, 1.0))
            })
            .collect();
        for stage in &self.stages {
            values = stage.apply(&values);
        }
        [0, 1, 2].map(|channel| values.get(channel).copied().unwrap_or(0) as f64 / ONE as f64)
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
                let mut base = Vec::with_capacity(grids.len());
                let mut fractions = Vec::with_capacity(grids.len());
                for (axis, &points) in grids.iter().enumerate() {
                    // 10.14.2.4: an input outside `0..=1` reads the nearer
                    // end of the range.
                    let x = values.get(axis).copied().unwrap_or(0).clamp(0, ONE);
                    let scaled = x * (points as i64 - 1);
                    let cell = ((scaled >> 16) as usize).min(points - 2);
                    base.push(cell);
                    fractions.push(scaled - ((cell as i64) << 16));
                }
                let lookup = Lookup {
                    grids,
                    outputs: *outputs,
                    table,
                    base: &base,
                    fractions: &fractions,
                };
                (0..*outputs)
                    .map(|channel| lookup.corner(0, 0, channel))
                    .collect()
            }
        }
    }
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
}

/// A curve set (10.14.2.2): one segmented curve a channel, each sampled over
/// its channel's input interval, which becomes the interval of what it gives.
fn curve_set(
    element: &[u8],
    intervals: &mut [(f64, f64)],
    budget: &mut usize,
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
        let curve = segmented(data)?;
        *budget = budget
            .checked_sub(CURVE_SAMPLES)
            .ok_or(IccError::TooLarge)?;
        let sampled = curve.sample(*interval)?;
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

/// A lookup-table element (10.14.2.4): sixteen grid counts of which the
/// first `p` are used, then the grid, `q` floats a point.
fn grid(
    element: &[u8],
    q: usize,
    intervals: &mut Vec<(f64, f64)>,
    budget: &mut usize,
) -> Result<Stage, IccError> {
    let p = intervals.len();
    let mut grids = Vec::with_capacity(p);
    let mut points = 1usize;
    for axis in 0..p {
        let count = usize::from(*element.get(12 + axis).ok_or(IccError::MalformedTag)?);
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
    if entries > MAX_CLUT_ENTRIES || entries > *budget {
        return Err(IccError::TooLarge);
    }
    // The bytes are there before anything is allocated for them.
    let end = entries
        .checked_mul(4)
        .and_then(|bytes| bytes.checked_add(28))
        .ok_or(IccError::TooLarge)?;
    if end > element.len() {
        return Err(IccError::MalformedTag);
    }
    *budget -= entries;
    let reader = Reader { bytes: element };
    let mut values = Vec::with_capacity(entries);
    for index in 0..entries {
        values.push(fixed32(float32(&reader, 28 + 4 * index)?));
    }
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

/// A segmented curve (`curf`): a segment count, `count - 1` break-points,
/// then the segments, the first and last a formula (10.14.2.2).
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
                // ICC.1:2010 Table 57: four parameters for the first form,
                // five for the other two.
                let needed = match function {
                    0 => 4,
                    1 | 2 => 5,
                    _ => return Err(IccError::MalformedTag),
                };
                let mut params = [0.0; 5];
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

    /// The curve at `x`: the first segment whose range holds it — the first
    /// up to and including the first break-point, each other from the
    /// break-point before it, excluded, to the one after it, included.
    fn eval(&self, x: f64) -> f64 {
        let index = self
            .breaks
            .iter()
            .position(|&point| x <= point)
            .unwrap_or(self.breaks.len());
        self.value(index, x)
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

    /// The curve sampled at [`CURVE_SAMPLES`] evenly spaced points over
    /// `interval`, in s15.16; one point where the interval is one.
    fn sample(&self, interval: (f64, f64)) -> Result<Sampled, IccError> {
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
            let value = self.eval(at as f64 / ONE as f64);
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

/// ICC.1:2010 Table 57's three formula segments, by function type.
fn formula(function: u16, params: &[f64; 5], x: f64) -> f64 {
    use tinker_pdf_math::{log10, pow};
    match function {
        // Y = (a X + b)^γ + c
        0 => {
            let [gamma, a, b, c, _] = *params;
            pow(a * x + b, gamma) + c
        }
        // Y = a log10(b X^γ + c) + d
        1 => {
            let [gamma, a, b, c, d] = *params;
            a * log10(b * pow(x, gamma) + c) + d
        }
        // Y = a b^(c X + d) + e
        2 => {
            let [a, b, c, d, e] = *params;
            a * pow(b, c * x + d) + e
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
