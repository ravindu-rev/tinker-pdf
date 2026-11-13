//! Variable fonts: the design space a face's `fvar` and `avar` describe, and
//! the integer arithmetic every variation table is evaluated in.
//!
//! Design documentation: `docs/design/variable-fonts.md`; feature
//! documentation: `docs/features/fonts.md`.
//!
//! # One numeric model, in integers
//!
//! OpenType 1.9.1's *OpenType Font Variations Overview*, 'Coordinate Scales
//! and Normalization', fixes how a user's position becomes the normalised
//! coordinate every variation table is read at: 16.16 arithmetic, clamped to
//! [−1, +1], converted to 2.14 by adding 2 and shifting right by 2. Its
//! 'Algorithm for Interpolation of Instance Values' leaves the
//! representation of scalars and sums open, asking for 16 fractional bits at
//! least and rounding only where a value is used. The design document fills
//! that freedom one way, and this module is it:
//!
//! - a user coordinate is a 16.16 [`Fixed`], an `f64` reaching it by the
//!   chapter's own conversion ([`Fixed::from_f64`]);
//! - every 16.16 quotient and product is the exact value rounded to the
//!   nearest 1/65 536, a half rounding toward +∞ — the one rounding the
//!   chapter states, its conversion's;
//! - a normalised coordinate is the chapter's 2.14 value, one `i16` an axis,
//!   in a [`Location`];
//! - a region's scalar is a 16.16 value in [0, 1], the per-axis scalars
//!   multiplied in `fvar`'s axis order ([`region_scalar`]);
//! - a scaled integer delta is exact, and sums are exact in 64 bits,
//!   saturating.
//!
//! **Float arithmetic is denied here** — the lint below — so that the
//! float-free shaper can call this evaluator as it stands (ruling 4). The two
//! conversions at the edge, [`Fixed::from_f64`] and [`Fixed::to_f64`], are
//! the only floats, each allowed by name.
//!
//! # A face that is not variable
//!
//! A face with no `fvar`, or one whose `axisCount` is zero, has a
//! [`DesignSpace`] with no axes: every coordinate asked of it is the default,
//! and the `fvar` chapter has every variation table of such a face ignored.

#![deny(clippy::float_arithmetic)]

use crate::sfnt::Sfnt;

/// A four-byte table or axis tag, as the bytes it is: `*b"wght"`.
pub type Tag = [u8; 4];

/// The most axes a design space is read with. More is refused by name
/// ([`VariationError::TooManyAxes`]): every tuple record and every region
/// costs one entry an axis, and a face with sixty-four is beyond any made.
pub const MAX_AXES: usize = 64;

const TAG_FVAR: Tag = *b"fvar";
const TAG_AVAR: Tag = *b"avar";

/// The four bytes of a tag as the `u32` [`Sfnt::table`] looks it up by.
#[must_use]
pub fn tag_value(tag: Tag) -> u32 {
    u32::from_be_bytes(tag)
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// `numerator / denominator`, rounded to the nearest integer with a half
/// rounding toward +∞: `floor(n / d + ½)`, which is `floor((2n + d) / 2d)`.
///
/// `None` for a zero denominator. Saturating, so no input overflows.
fn div_round(numerator: i128, denominator: i128) -> Option<i128> {
    if denominator == 0 {
        return None;
    }
    let (n, d) = if denominator < 0 {
        (numerator.saturating_neg(), denominator.saturating_neg())
    } else {
        (numerator, denominator)
    };
    let twice = d.saturating_mul(2);
    Some(n.saturating_mul(2).saturating_add(d).div_euclid(twice))
}

/// An `i128` held to the `i64` range.
fn saturate(value: i128) -> i64 {
    i64::try_from(value).unwrap_or(if value < 0 { i64::MIN } else { i64::MAX })
}

/// A 16.16 fixed-point number: the specification's `Fixed`, held in 64 bits
/// so that a default plus any sum of scaled deltas fits — the overview asks
/// for at least 32 significant bits in calculation.
///
/// Every operation that can lose a bit rounds to the nearest 1/65 536, a
/// half rounding toward +∞, which is the rounding the overview's
/// float-to-16.16 conversion states and the one this model uses throughout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fixed(i64);

impl Fixed {
    /// Zero.
    pub const ZERO: Fixed = Fixed(0);
    /// One.
    pub const ONE: Fixed = Fixed(1 << 16);
    /// Minus one.
    pub const MINUS_ONE: Fixed = Fixed(-(1 << 16));

    /// The value whose 16.16 representation is `bits`.
    #[must_use]
    pub const fn from_bits(bits: i64) -> Fixed {
        Fixed(bits)
    }

    /// The 16.16 representation: the value times 65 536.
    #[must_use]
    pub const fn bits(self) -> i64 {
        self.0
    }

    /// A whole number of units, exactly.
    #[must_use]
    pub const fn from_int(units: i32) -> Fixed {
        Fixed((units as i64) << 16)
    }

    /// A 2.14 value, exactly: four times its integer.
    #[must_use]
    pub const fn from_f2dot14(value: i16) -> Fixed {
        Fixed((value as i64) << 2)
    }

    /// A 32-bit `Fixed` as a table stores it.
    #[must_use]
    pub const fn from_fixed32(bits: i32) -> Fixed {
        Fixed(bits as i64)
    }

    /// An `f64`, by the overview's conversion to 16.16 ('Coordinate Scales
    /// and Normalization'): the fractional part times 65 536 rounded to the
    /// nearest integer, a half taking the next higher one, in the low word,
    /// and the integer part's two's complement in the high word. That is
    /// `floor(x · 65536 + ½)`, held to the 32-bit range the specification's
    /// `Fixed` has. A NaN is zero.
    #[must_use]
    #[allow(
        clippy::float_arithmetic,
        reason = "the conversion at the module's edge the overview defines"
    )]
    pub fn from_f64(value: f64) -> Fixed {
        if value.is_nan() {
            return Fixed::ZERO;
        }
        let scaled = (value * 65_536.0 + 0.5).floor();
        if scaled >= f64::from(i32::MAX) {
            Fixed(i64::from(i32::MAX))
        } else if scaled <= f64::from(i32::MIN) {
            Fixed(i64::from(i32::MIN))
        } else {
            // Inside the 32-bit range and integral, so the cast is exact.
            Fixed(scaled as i64)
        }
    }

    /// The value as an `f64`, exactly: 16 fractional bits and fewer than 53
    /// in all for any value a face can produce.
    #[must_use]
    #[allow(
        clippy::float_arithmetic,
        reason = "the conversion at the module's edge an outline reads"
    )]
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / 65_536.0
    }

    /// The nearest whole number, a half rounding toward +∞: `floor(v + ½)`,
    /// the rounding a static instance writes font units with.
    #[must_use]
    pub const fn round(self) -> i64 {
        self.0.saturating_add(1 << 15) >> 16
    }

    /// The product, rounded to 16.16.
    #[must_use]
    pub fn mul_rounded(self, other: Fixed) -> Fixed {
        let product = i128::from(self.0) * i128::from(other.0);
        Fixed(saturate((product + (1 << 15)) >> 16))
    }

    /// The quotient, rounded to 16.16; `None` for a zero divisor.
    #[must_use]
    pub fn div_rounded(self, divisor: Fixed) -> Option<Fixed> {
        let quotient = div_round(i128::from(self.0) << 16, i128::from(divisor.0))?;
        Some(Fixed(saturate(quotient)))
    }

    /// This value times a whole number, exactly — a scalar times an integer
    /// delta — saturating.
    #[must_use]
    pub fn scaled(self, delta: i32) -> Fixed {
        Fixed(self.0.saturating_mul(i64::from(delta)))
    }

    /// The sum, saturating.
    #[must_use]
    pub const fn saturating_add(self, other: Fixed) -> Fixed {
        Fixed(self.0.saturating_add(other.0))
    }

    /// The difference, saturating.
    #[must_use]
    pub const fn saturating_sub(self, other: Fixed) -> Fixed {
        Fixed(self.0.saturating_sub(other.0))
    }

    /// Held to [−1, +1], as the overview clamps every normalised value.
    #[must_use]
    pub fn clamp_unit(self) -> Fixed {
        self.max(Fixed::MINUS_ONE).min(Fixed::ONE)
    }

    /// The overview's conversion of a normalised 16.16 value to 2.14: add 2
    /// and shift right by 2, sign extending. The value is clamped to
    /// [−1, +1] first, so the result is in [−16 384, 16 384].
    #[must_use]
    pub fn to_f2dot14(self) -> i16 {
        let v = (self.clamp_unit().0 + 2) >> 2;
        // Within ±16 384 after the clamp, so the conversion cannot fail.
        i16::try_from(v).unwrap_or(0)
    }
}

/// A position in a design space: one normalised 2.14 coordinate an axis, in
/// `fvar`'s axis order — the exact value the overview says must be stored
/// and returned.
///
/// A coordinate past the end reads as zero, the default, so a location made
/// for fewer axes than a table names leaves the rest at their defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Location {
    coordinates: Vec<i16>,
}

impl Location {
    /// A location from normalised 2.14 coordinates, one an axis.
    #[must_use]
    pub fn new(coordinates: Vec<i16>) -> Location {
        Location { coordinates }
    }

    /// The coordinates, one an axis.
    #[must_use]
    pub fn coordinates(&self) -> &[i16] {
        &self.coordinates
    }

    /// One axis's coordinate; zero past the end.
    #[must_use]
    pub fn coordinate(&self, axis: usize) -> i16 {
        self.coordinates.get(axis).copied().unwrap_or(0)
    }

    /// Whether every coordinate is the default's, zero.
    #[must_use]
    pub fn is_default(&self) -> bool {
        self.coordinates.iter().all(|c| *c == 0)
    }
}

/// One axis of a region of the design space: where its effect starts, peaks
/// and ends, in 2.14.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RegionAxis {
    /// Where the effect starts.
    pub start: i16,
    /// Where it is whole.
    pub peak: i16,
    /// Where it ends.
    pub end: i16,
}

impl RegionAxis {
    /// The region a peak alone implies, as a tuple that is not intermediate
    /// carries it: from zero to the peak, on the peak's side (overview,
    /// 'Algorithm for Interpolation of Instance Values').
    #[must_use]
    pub fn from_peak(peak: i16) -> RegionAxis {
        match peak {
            p if p > 0 => RegionAxis {
                start: 0,
                peak: p,
                end: p,
            },
            p if p < 0 => RegionAxis {
                start: p,
                peak: p,
                end: 0,
            },
            _ => RegionAxis::default(),
        }
    }

    /// This axis's scalar at a coordinate, by the overview's pseudo-code: 1
    /// where the region is not valid for the axis or its peak is zero, 0
    /// outside `[start, end]`, 1 at the peak, and between them the
    /// coordinate's proximity to the peak, rounded to 16.16.
    #[must_use]
    pub fn scalar(self, coordinate: i16) -> Fixed {
        let (start, peak, end) = (
            i128::from(self.start),
            i128::from(self.peak),
            i128::from(self.end),
        );
        let c = i128::from(coordinate);
        // Not valid for the axis: ignore it, which is a factor of one.
        if start > peak || peak > end {
            return Fixed::ONE;
        }
        if start < 0 && end > 0 && peak != 0 {
            return Fixed::ONE;
        }
        if peak == 0 {
            return Fixed::ONE;
        }
        if c < start || c > end {
            return Fixed::ZERO;
        }
        if c == peak {
            return Fixed::ONE;
        }
        // Both denominators are positive: `c < peak` with `c >= start` puts
        // the start below the peak, and `c > peak` with `c <= end` the end
        // above it.
        let ratio = if c < peak {
            div_round((c - start) << 16, peak - start)
        } else {
            div_round((end - c) << 16, end - peak)
        };
        Fixed(ratio.map_or(0, saturate))
    }
}

/// A region's scalar at a location: one, multiplied by each axis's scalar in
/// order, each product rounded to 16.16 — a value in [0, 1].
#[must_use]
pub fn region_scalar(axes: &[RegionAxis], location: &Location) -> Fixed {
    let mut scalar = Fixed::ONE;
    for (index, axis) in axes.iter().enumerate() {
        let factor = axis.scalar(location.coordinate(index));
        if factor == Fixed::ZERO {
            return Fixed::ZERO;
        }
        scalar = scalar.mul_rounded(factor);
    }
    scalar
}

/// One axis of a design space, as `fvar` records it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Axis {
    /// What varies: `wght`, `wdth`, `opsz`, or a foundry's own tag.
    pub tag: Tag,
    /// The least user value.
    pub min: Fixed,
    /// The default user value, at which the face's own outlines apply.
    pub default: Fixed,
    /// The greatest user value.
    pub max: Fixed,
    /// The axis qualifiers.
    pub flags: u16,
    /// The `name` table entry naming the axis.
    pub name_id: u16,
}

impl Axis {
    /// `HIDDEN_AXIS`: the axis is not one to offer in a menu.
    pub const HIDDEN: u16 = 0x0001;

    /// Whether the font asks that the axis not be shown to a person.
    #[must_use]
    pub fn is_hidden(&self) -> bool {
        self.flags & Axis::HIDDEN != 0
    }

    /// A user value clamped to this axis and normalised by the default
    /// mapping, in 16.16 (overview, the normalisation's steps 2 and 3 and the
    /// precision list's steps 1 to 3): minus one at the minimum, zero at the
    /// default, one at the maximum, linear between, clamped to [−1, +1].
    #[must_use]
    pub fn normalise(&self, user: Fixed) -> Fixed {
        let user = user.max(self.min).min(self.max);
        let normalised = if user < self.default {
            user.saturating_sub(self.default)
                .div_rounded(self.default.saturating_sub(self.min))
        } else if user > self.default {
            user.saturating_sub(self.default)
                .div_rounded(self.max.saturating_sub(self.default))
        } else {
            Some(Fixed::ZERO)
        };
        normalised.unwrap_or(Fixed::ZERO).clamp_unit()
    }
}

/// A named instance: a position the designer named, in user coordinates.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NamedInstance {
    /// The `name` table entry naming the instance's subfamily.
    pub subfamily_name_id: u16,
    /// Reserved; zero in this version.
    pub flags: u16,
    /// One user coordinate an axis, in `fvar`'s axis order.
    pub coordinates: Vec<Fixed>,
    /// The `name` table entry of its PostScript name, where the records hold
    /// one; 0xFFFF says it has none.
    pub postscript_name_id: Option<u16>,
}

/// One correspondence of an `avar` segment map, in 2.14: a default
/// normalised value and the value it becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AxisValueMap {
    /// The default normalised value.
    pub from: i16,
    /// The final normalised value.
    pub to: i16,
}

/// One axis's `avar` segment map: the maps kept, and whether they modify the
/// axis at all.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SegmentMap {
    maps: Vec<AxisValueMap>,
    applies: bool,
}

impl SegmentMap {
    /// A map from its records in table order, read as the `avar` chapter
    /// says: a record whose `fromCoordinate` does not exceed the last kept
    /// one's, or whose `toCoordinate` falls below it, is ignored; and an axis
    /// whose kept records lack any of −1→−1, 0→0 and 1→1 is not modified.
    #[must_use]
    pub fn new(records: &[AxisValueMap]) -> SegmentMap {
        let mut maps: Vec<AxisValueMap> = Vec::with_capacity(records.len());
        for record in records {
            if let Some(last) = maps.last() {
                if record.from <= last.from || record.to < last.to {
                    continue;
                }
            }
            maps.push(*record);
        }
        let has = |value: i16| maps.iter().any(|m| m.from == value && m.to == value);
        let applies = has(-0x4000) && has(0) && has(0x4000);
        SegmentMap { maps, applies }
    }

    /// The records kept.
    #[must_use]
    pub fn maps(&self) -> &[AxisValueMap] {
        &self.maps
    }

    /// Whether the map modifies its axis: it holds the three required
    /// correspondences.
    #[must_use]
    pub fn applies(&self) -> bool {
        self.applies
    }

    /// A default normalised 16.16 value through the map (overview, the
    /// `avar` step): the first kept record at or past it is `endSeg`; equal,
    /// its `toCoordinate`; past, interpolated from the record before it, the
    /// ratio and the product each rounded to 16.16. Clamped to [−1, +1]. An
    /// axis the map does not modify keeps its value.
    #[must_use]
    pub fn map(&self, value: Fixed) -> Fixed {
        if !self.applies {
            return value.clamp_unit();
        }
        for (index, end) in self.maps.iter().enumerate() {
            let end_from = Fixed::from_f2dot14(end.from);
            if end_from < value {
                continue;
            }
            if end_from == value {
                return Fixed::from_f2dot14(end.to).clamp_unit();
            }
            // The kept maps hold −1 and every value is at least −1, so a map
            // strictly past the value is never the first.
            let Some(start) = index.checked_sub(1).and_then(|i| self.maps.get(i)) else {
                return value.clamp_unit();
            };
            let start_from = Fixed::from_f2dot14(start.from);
            let start_to = Fixed::from_f2dot14(start.to);
            let end_to = Fixed::from_f2dot14(end.to);
            let Some(ratio) = value
                .saturating_sub(start_from)
                .div_rounded(end_from.saturating_sub(start_from))
            else {
                return value.clamp_unit();
            };
            return start_to
                .saturating_add(ratio.mul_rounded(end_to.saturating_sub(start_to)))
                .clamp_unit();
        }
        // Every value is at most one and the kept maps hold one.
        value.clamp_unit()
    }
}

/// How a caller names an instance: by its index among `fvar`'s named
/// instances, or by user coordinates, each with its axis's tag.
///
/// The one argument every caller of the variation machinery passes — the
/// document builder and the book renderer alike (`docs/design/
/// variable-fonts.md`, 'Who asks for an instance'). The default is the
/// default instance: no coordinate given.
#[derive(Clone, Debug, PartialEq)]
pub enum Instance {
    /// A named instance, by its index in `fvar`.
    Named(u16),
    /// User coordinates. A tag the face does not have is ignored, a tag given
    /// twice takes its last value, and an axis given none, or a NaN, stays at
    /// its default (`fvar`, 'Variation Instance Selection').
    User(Vec<UserCoordinate>),
}

impl Default for Instance {
    fn default() -> Instance {
        Instance::User(Vec::new())
    }
}

/// One axis's user coordinate in an [`Instance`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UserCoordinate {
    /// The axis.
    pub tag: Tag,
    /// The value, in the axis's user scale: 700.0 for a bold `wght`.
    pub value: f64,
}

impl UserCoordinate {
    /// A coordinate.
    #[must_use]
    pub fn new(tag: Tag, value: f64) -> UserCoordinate {
        UserCoordinate { tag, value }
    }
}

/// A face's design space: its axes, its named instances, and its `avar`
/// segment maps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DesignSpace {
    axes: Vec<Axis>,
    instances: Vec<NamedInstance>,
    /// One map an axis when the face has an `avar`; empty when it has none.
    segments: Vec<SegmentMap>,
}

impl DesignSpace {
    /// The design space of a face: its `fvar` and `avar` tables.
    ///
    /// A face with no `fvar` has no axes, which is not an error.
    ///
    /// # Errors
    ///
    /// A table this reader refuses, by name ([`VariationError`]). A caller
    /// draws such a face at its default.
    pub fn read(sfnt: &Sfnt) -> Result<DesignSpace, VariationError> {
        let Some(fvar) = sfnt.table(tag_value(TAG_FVAR)) else {
            return Ok(DesignSpace::default());
        };
        DesignSpace::parse(fvar, sfnt.table(tag_value(TAG_AVAR)))
    }

    /// The design space from an `fvar` table and, where the face has one, its
    /// `avar` table.
    ///
    /// # Errors
    ///
    /// A table this reader refuses, by name.
    pub fn parse(fvar: &[u8], avar: Option<&[u8]>) -> Result<DesignSpace, VariationError> {
        let (axes, instances) = parse_fvar(fvar)?;
        if axes.is_empty() {
            // The `fvar` chapter: a zero axis count makes the face a
            // non-variable one, and its variation data is ignored.
            return Ok(DesignSpace::default());
        }
        let segments = match avar {
            Some(avar) => parse_avar(avar, axes.len())?,
            None => Vec::new(),
        };
        Ok(DesignSpace {
            axes,
            instances,
            segments,
        })
    }

    /// The axes, in `fvar`'s order — the order every table's tuples follow.
    #[must_use]
    pub fn axes(&self) -> &[Axis] {
        &self.axes
    }

    /// The named instances, in `fvar`'s order.
    #[must_use]
    pub fn instances(&self) -> &[NamedInstance] {
        &self.instances
    }

    /// Whether the face varies at all.
    #[must_use]
    pub fn is_variable(&self) -> bool {
        !self.axes.is_empty()
    }

    /// How many axes the space has.
    #[must_use]
    pub fn axis_count(&self) -> usize {
        self.axes.len()
    }

    /// The first axis carrying a tag.
    #[must_use]
    pub fn axis_index(&self, tag: Tag) -> Option<usize> {
        self.axes.iter().position(|axis| axis.tag == tag)
    }

    /// Whether the face carries an `avar`.
    #[must_use]
    pub fn has_avar(&self) -> bool {
        !self.segments.is_empty()
    }

    /// One axis's `avar` segment map, where the face has an `avar`.
    #[must_use]
    pub fn segment_map(&self, axis: usize) -> Option<&SegmentMap> {
        self.segments.get(axis)
    }

    /// One axis's user value normalised: the default mapping, then `avar`,
    /// then 2.14 — the coordinate the overview says is stored.
    #[must_use]
    pub fn normalise_axis(&self, axis: usize, user: Fixed) -> i16 {
        let Some(record) = self.axes.get(axis) else {
            return 0;
        };
        let mut value = record.normalise(user);
        if let Some(map) = self.segments.get(axis) {
            value = map.map(value);
        }
        value.to_f2dot14()
    }

    /// User coordinates, one an axis in order, normalised into a location;
    /// an axis past the end of `user` takes its default.
    #[must_use]
    pub fn normalise(&self, user: &[Fixed]) -> Location {
        let coordinates = self
            .axes
            .iter()
            .enumerate()
            .map(|(index, axis)| {
                let value = user.get(index).copied().unwrap_or(axis.default);
                self.normalise_axis(index, value)
            })
            .collect();
        Location { coordinates }
    }

    /// The default instance's location: zero on every axis.
    #[must_use]
    pub fn default_location(&self) -> Location {
        Location {
            coordinates: vec![0; self.axes.len()],
        }
    }

    /// The user coordinates an instance names, one an axis in order.
    ///
    /// # Errors
    ///
    /// [`VariationError::NoSuchInstance`] for a named index past the face's
    /// named instances.
    pub fn user_coordinates(&self, instance: &Instance) -> Result<Vec<Fixed>, VariationError> {
        match instance {
            Instance::Named(index) => {
                let named = self.instances.get(usize::from(*index)).ok_or_else(|| {
                    VariationError::NoSuchInstance {
                        index: *index,
                        count: u16::try_from(self.instances.len()).unwrap_or(u16::MAX),
                    }
                })?;
                Ok(self
                    .axes
                    .iter()
                    .enumerate()
                    .map(|(axis, record)| {
                        named
                            .coordinates
                            .get(axis)
                            .copied()
                            .unwrap_or(record.default)
                    })
                    .collect())
            }
            Instance::User(coordinates) => {
                let mut user: Vec<Fixed> = self.axes.iter().map(|axis| axis.default).collect();
                for coordinate in coordinates {
                    if coordinate.value.is_nan() {
                        continue;
                    }
                    let value = Fixed::from_f64(coordinate.value);
                    for (slot, axis) in user.iter_mut().zip(&self.axes) {
                        if axis.tag == coordinate.tag {
                            *slot = value;
                        }
                    }
                }
                Ok(user)
            }
        }
    }

    /// The location an instance names.
    ///
    /// # Errors
    ///
    /// [`VariationError::NoSuchInstance`] for a named index past the face's
    /// named instances.
    pub fn location(&self, instance: &Instance) -> Result<Location, VariationError> {
        Ok(self.normalise(&self.user_coordinates(instance)?))
    }
}

/// Reads `fvar`'s axes and named instances (`fvar`, 'Table Formats').
fn parse_fvar(fvar: &[u8]) -> Result<(Vec<Axis>, Vec<NamedInstance>), VariationError> {
    let header = |at| be16(fvar, at).ok_or(VariationError::HeaderPastEnd { table: TAG_FVAR });
    let major = header(0)?;
    let minor = header(2)?;
    let axes_at = usize::from(header(4)?);
    let axis_count = header(8)?;
    let axis_size = header(10)?;
    let instance_count = header(12)?;
    let instance_size = header(14)?;
    if major != 1 {
        return Err(VariationError::UnknownVersion {
            table: TAG_FVAR,
            major,
            minor,
        });
    }
    if axis_count == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    if usize::from(axis_count) > MAX_AXES {
        return Err(VariationError::TooManyAxes { count: axis_count });
    }
    // A VariationAxisRecord is twenty bytes; a larger size is a compatible
    // extension, read past.
    if axis_size < 20 {
        return Err(VariationError::AxisSize { size: axis_size });
    }
    let axes_end = usize::from(axis_count)
        .checked_mul(usize::from(axis_size))
        .and_then(|bytes| bytes.checked_add(axes_at))
        .filter(|end| *end <= fvar.len())
        .ok_or(VariationError::AxesPastEnd { count: axis_count })?;

    let mut axes = Vec::with_capacity(usize::from(axis_count));
    for index in 0..axis_count {
        let at = axes_at + usize::from(index) * usize::from(axis_size);
        let past = VariationError::AxesPastEnd { count: axis_count };
        let tag = fvar
            .get(at..at + 4)
            .and_then(|b| <Tag>::try_from(b).ok())
            .ok_or(past)?;
        let fixed = |offset: usize| -> Result<Fixed, VariationError> {
            be32(fvar, at + offset)
                .map(|bits| Fixed::from_fixed32(bits as i32))
                .ok_or(past)
        };
        let (min, default, max) = (fixed(4)?, fixed(8)?, fixed(12)?);
        let flags = be16(fvar, at + 16).ok_or(past)?;
        let name_id = be16(fvar, at + 18).ok_or(past)?;
        if min > default || default > max {
            return Err(VariationError::AxisRange { axis: index });
        }
        axes.push(Axis {
            tag,
            min,
            default,
            max,
            flags,
            name_id,
        });
    }

    // The instances array directly follows the axes array.
    let coordinates_size = usize::from(axis_count) * 4;
    let mut instances = Vec::new();
    if instance_count > 0 {
        if usize::from(instance_size) < coordinates_size + 4 {
            return Err(VariationError::InstanceSize {
                size: instance_size,
            });
        }
        let has_postscript = usize::from(instance_size) >= coordinates_size + 6;
        let past = VariationError::InstancesPastEnd {
            count: instance_count,
        };
        usize::from(instance_count)
            .checked_mul(usize::from(instance_size))
            .and_then(|bytes| bytes.checked_add(axes_end))
            .filter(|end| *end <= fvar.len())
            .ok_or(past)?;
        instances.reserve(usize::from(instance_count));
        for index in 0..usize::from(instance_count) {
            let at = axes_end + index * usize::from(instance_size);
            let subfamily_name_id = be16(fvar, at).ok_or(past)?;
            let flags = be16(fvar, at + 2).ok_or(past)?;
            let mut coordinates = Vec::with_capacity(usize::from(axis_count));
            for axis in 0..usize::from(axis_count) {
                let bits = be32(fvar, at + 4 + axis * 4).ok_or(past)?;
                coordinates.push(Fixed::from_fixed32(bits as i32));
            }
            let postscript_name_id = if has_postscript {
                Some(be16(fvar, at + 4 + coordinates_size).ok_or(past)?)
            } else {
                None
            };
            instances.push(NamedInstance {
                subfamily_name_id,
                flags,
                coordinates,
                postscript_name_id,
            });
        }
    }
    Ok((axes, instances))
}

/// Reads `avar`'s segment maps, one an axis (`avar`, 'Table formats').
fn parse_avar(avar: &[u8], axis_count: usize) -> Result<Vec<SegmentMap>, VariationError> {
    let header = |at| be16(avar, at).ok_or(VariationError::HeaderPastEnd { table: TAG_AVAR });
    let major = header(0)?;
    let minor = header(2)?;
    let count = header(6)?;
    if major != 1 {
        return Err(VariationError::UnknownVersion {
            table: TAG_AVAR,
            major,
            minor,
        });
    }
    if usize::from(count) != axis_count {
        // `fvar` holds at most `MAX_AXES`, so the count fits.
        let expected = u16::try_from(axis_count).unwrap_or(u16::MAX);
        return Err(VariationError::AxisCount {
            table: TAG_AVAR,
            found: count,
            expected,
        });
    }
    let mut at = 8usize;
    let mut segments = Vec::with_capacity(axis_count);
    for axis in 0..count {
        let past = VariationError::SegmentsPastEnd { axis };
        let maps = usize::from(be16(avar, at).ok_or(past)?);
        at += 2;
        let end = maps
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(at))
            .filter(|end| *end <= avar.len())
            .ok_or(past)?;
        let mut records = Vec::with_capacity(maps);
        while at < end {
            let from = be16(avar, at).ok_or(past)? as i16;
            let to = be16(avar, at + 2).ok_or(past)? as i16;
            records.push(AxisValueMap { from, to });
            at += 4;
        }
        segments.push(SegmentMap::new(&records));
    }
    Ok(segments)
}

/// Why a variation table was refused. Each names the rule the table broke;
/// a face refused here is drawn at its default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum VariationError {
    /// A table is shorter than its fixed header.
    HeaderPastEnd {
        /// The table.
        table: Tag,
    },
    /// A table's major version is one its chapter does not define.
    UnknownVersion {
        /// The table.
        table: Tag,
        /// Its major version.
        major: u16,
        /// Its minor version.
        minor: u16,
    },
    /// `fvar` declares more axes than [`MAX_AXES`].
    TooManyAxes {
        /// The count it declares.
        count: u16,
    },
    /// `fvar`'s `axisSize` is smaller than a `VariationAxisRecord`.
    AxisSize {
        /// The size it declares.
        size: u16,
    },
    /// `fvar`'s axis records run past the table's end.
    AxesPastEnd {
        /// The count it declares.
        count: u16,
    },
    /// An axis whose minimum exceeds its default, or whose default exceeds
    /// its maximum.
    AxisRange {
        /// The axis, by index.
        axis: u16,
    },
    /// `fvar`'s `instanceSize` is too small for an instance's coordinates.
    InstanceSize {
        /// The size it declares.
        size: u16,
    },
    /// `fvar`'s instance records run past the table's end.
    InstancesPastEnd {
        /// The count it declares.
        count: u16,
    },
    /// A table's axis count is not `fvar`'s.
    AxisCount {
        /// The table.
        table: Tag,
        /// The count it declares.
        found: u16,
        /// `fvar`'s.
        expected: u16,
    },
    /// An `avar` segment map runs past the table's end.
    SegmentsPastEnd {
        /// The axis whose map does.
        axis: u16,
    },
    /// A named instance the face does not have.
    NoSuchInstance {
        /// The index asked for.
        index: u16,
        /// How many the face has.
        count: u16,
    },
}

/// A tag as the four characters it is, for a message.
fn tag_text(tag: Tag) -> String {
    tag.iter()
        .map(|&b| {
            if (0x20..0x7F).contains(&b) {
                char::from(b)
            } else {
                '?'
            }
        })
        .collect()
}

impl core::fmt::Display for VariationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            VariationError::HeaderPastEnd { table } => {
                write!(f, "the {} table ends inside its header", tag_text(*table))
            }
            VariationError::UnknownVersion {
                table,
                major,
                minor,
            } => write!(
                f,
                "the {} table is version {major}.{minor}, which is not read here",
                tag_text(*table)
            ),
            VariationError::TooManyAxes { count } => write!(
                f,
                "fvar declares {count} axes, more than the {MAX_AXES} read here"
            ),
            VariationError::AxisSize { size } => {
                write!(f, "fvar's axis records are {size} bytes, under twenty")
            }
            VariationError::AxesPastEnd { count } => {
                write!(f, "fvar's {count} axis records run past the table's end")
            }
            VariationError::AxisRange { axis } => write!(
                f,
                "fvar's axis {axis} has its default outside its minimum and maximum"
            ),
            VariationError::InstanceSize { size } => write!(
                f,
                "fvar's instance records are {size} bytes, too few for their coordinates"
            ),
            VariationError::InstancesPastEnd { count } => {
                write!(f, "fvar's {count} named instances run past the table's end")
            }
            VariationError::AxisCount {
                table,
                found,
                expected,
            } => write!(
                f,
                "the {} table names {found} axes where fvar names {expected}",
                tag_text(*table)
            ),
            VariationError::SegmentsPastEnd { axis } => {
                write!(
                    f,
                    "avar's segment map for axis {axis} runs past the table's end"
                )
            }
            VariationError::NoSuchInstance { index, count } => write!(
                f,
                "named instance {index} was asked for and the face has {count}"
            ),
        }
    }
}

impl std::error::Error for VariationError {}

#[cfg(test)]
#[allow(
    clippy::float_arithmetic,
    reason = "the tests state the overview's values as decimals a person can read"
)]
mod tests {
    use super::*;

    /// 2.14 from a value a test can read: `f(0.5)` is 8 192.
    fn f2(value: i32, per: i32) -> i16 {
        i16::try_from(value * 16_384 / per).expect("in range")
    }

    #[test]
    fn the_conversion_from_f64_rounds_a_half_up_in_both_signs() {
        assert_eq!(Fixed::from_f64(1.0).bits(), 65_536);
        assert_eq!(Fixed::from_f64(-1.0).bits(), -65_536);
        assert_eq!(Fixed::from_f64(0.5).bits(), 32_768);
        // −0.3: the integer part −1 and the fraction 0.7, whose 45 875.2 is
        // truncated, so the value is −65 536 + 45 875.
        assert_eq!(Fixed::from_f64(-0.3).bits(), -65_536 + 45_875);
        // Exactly half a step: up, toward +∞, on both sides of zero.
        assert_eq!(Fixed::from_f64(0.5 / 65_536.0).bits(), 1);
        assert_eq!(Fixed::from_f64(-0.5 / 65_536.0).bits(), 0);
        assert_eq!(Fixed::from_f64(-1.5 / 65_536.0).bits(), -1);
        assert_eq!(Fixed::from_f64(f64::NAN), Fixed::ZERO);
        assert_eq!(Fixed::from_f64(1e12).bits(), i64::from(i32::MAX));
        assert_eq!(
            Fixed::from_f64(f64::NEG_INFINITY).bits(),
            i64::from(i32::MIN)
        );
        assert!((Fixed::from_f64(650.25).to_f64() - 650.25).abs() < f64::EPSILON);
    }

    #[test]
    fn rounding_takes_a_half_toward_positive_infinity() {
        assert_eq!(Fixed::from_bits(0x8000).round(), 1);
        assert_eq!(Fixed::from_bits(-0x8000).round(), 0);
        assert_eq!(Fixed::from_bits(-0x8001).round(), -1);
        assert_eq!(Fixed::from_bits(0x7FFF).round(), 0);
        // A third, rounded: 21 845.33 down; and a negative tie up.
        assert_eq!(
            Fixed::ONE.div_rounded(Fixed::from_int(3)).map(Fixed::bits),
            Some(21_845)
        );
        assert_eq!(
            Fixed::from_bits(-1)
                .div_rounded(Fixed::from_int(2))
                .map(Fixed::bits),
            Some(0),
            "−1/2 of an ulp is a tie, which rounds up"
        );
        assert_eq!(
            Fixed::from_bits(1)
                .div_rounded(Fixed::from_int(-2))
                .map(Fixed::bits),
            Some(0),
            "a negative divisor rounds the same way"
        );
        assert_eq!(Fixed::ONE.div_rounded(Fixed::ZERO), None);
        assert_eq!(
            Fixed::from_bits(3)
                .mul_rounded(Fixed::from_bits(0x8000))
                .bits(),
            2,
            "1.5 ulps rounds up to two"
        );
        assert_eq!(
            Fixed::from_bits(-3)
                .mul_rounded(Fixed::from_bits(0x8000))
                .bits(),
            -1,
            "−1.5 ulps rounds up to minus one"
        );
    }

    #[test]
    fn two_point_fourteen_adds_two_and_shifts() {
        assert_eq!(Fixed::ONE.to_f2dot14(), 16_384);
        assert_eq!(Fixed::MINUS_ONE.to_f2dot14(), -16_384);
        assert_eq!(
            Fixed::from_bits(2).to_f2dot14(),
            1,
            "half a 2.14 step rounds up"
        );
        assert_eq!(Fixed::from_bits(1).to_f2dot14(), 0);
        assert_eq!(
            Fixed::from_bits(-2).to_f2dot14(),
            0,
            "and the negative tie up too"
        );
        assert_eq!(Fixed::from_bits(-3).to_f2dot14(), -1);
        assert_eq!(Fixed::from_int(7).to_f2dot14(), 16_384, "clamped first");
    }

    #[test]
    fn the_overviews_normalisation_example_is_minus_a_half() {
        let axis = Axis {
            tag: *b"wght",
            min: Fixed::from_int(100),
            default: Fixed::from_int(400),
            max: Fixed::from_int(900),
            flags: 0,
            name_id: 256,
        };
        assert_eq!(
            axis.normalise(Fixed::from_int(250)),
            Fixed::from_bits(-32_768)
        );
        assert_eq!(axis.normalise(Fixed::from_int(100)), Fixed::MINUS_ONE);
        assert_eq!(axis.normalise(Fixed::from_int(900)), Fixed::ONE);
        assert_eq!(axis.normalise(Fixed::from_int(400)), Fixed::ZERO);
        assert_eq!(
            axis.normalise(Fixed::from_int(-5)),
            Fixed::MINUS_ONE,
            "clamped"
        );
        assert_eq!(
            axis.normalise(Fixed::from_int(5_000)),
            Fixed::ONE,
            "clamped"
        );
        // An axis whose default is its minimum has nothing below it.
        let from_default = Axis {
            min: Fixed::from_int(400),
            ..axis
        };
        assert_eq!(from_default.normalise(Fixed::from_int(100)), Fixed::ZERO);
    }

    /// The `avar` chapter's example map and the overview's table of what it
    /// does to nine values, each within half a 2.14 step of the table's four
    /// decimals.
    #[test]
    fn the_overviews_avar_example_maps_as_its_table_says() {
        let map = SegmentMap::new(&[
            AxisValueMap {
                from: -16_384,
                to: -16_384,
            },
            AxisValueMap {
                from: f2(-3, 4),
                to: f2(-1, 2),
            },
            AxisValueMap { from: 0, to: 0 },
            AxisValueMap {
                from: f2(2, 5),
                to: f2(2, 5),
            },
            AxisValueMap {
                from: f2(3, 5),
                to: f2(9, 10),
            },
            AxisValueMap {
                from: 16_384,
                to: 16_384,
            },
        ]);
        assert!(map.applies());
        let through = |value: Fixed| map.map(value).to_f2dot14();
        // −0.5: a third of the way from −0.75 to 0, so −0.5 + (1/3)·0.5.
        assert_eq!(through(Fixed::from_bits(-32_768)), -5_461);
        assert_eq!(through(Fixed::MINUS_ONE), -16_384);
        assert_eq!(through(Fixed::from_bits(-49_152)), f2(-1, 2));
        assert_eq!(through(Fixed::ZERO), 0);
        assert_eq!(
            through(Fixed::from_bits(16_384)),
            4_096,
            "0.25 maps to 0.25"
        );
        // 0.5 is halfway from 0.4 to 0.6, so 0.65.
        assert_eq!(through(Fixed::from_bits(32_768)), 10_650);
        assert_eq!(through(Fixed::ONE), 16_384);
    }

    #[test]
    fn avar_records_out_of_order_are_ignored_and_a_missing_anchor_disables_the_map() {
        // A `from` that does not advance and a `to` that falls back are both
        // dropped, and the rest still maps.
        let map = SegmentMap::new(&[
            AxisValueMap {
                from: -16_384,
                to: -16_384,
            },
            AxisValueMap { from: 0, to: 0 },
            AxisValueMap { from: 0, to: 4_000 },
            AxisValueMap {
                from: 8_192,
                to: -100,
            },
            AxisValueMap {
                from: 8_192,
                to: 12_288,
            },
            AxisValueMap {
                from: 16_384,
                to: 16_384,
            },
        ]);
        assert_eq!(map.maps().len(), 4);
        assert!(map.applies());
        assert_eq!(map.map(Fixed::from_bits(32_768)).to_f2dot14(), 12_288);

        // Without 0 → 0 the axis is not modified at all.
        let missing = SegmentMap::new(&[
            AxisValueMap {
                from: -16_384,
                to: -16_384,
            },
            AxisValueMap {
                from: 8_192,
                to: 12_288,
            },
            AxisValueMap {
                from: 16_384,
                to: 16_384,
            },
        ]);
        assert!(!missing.applies());
        assert_eq!(
            missing.map(Fixed::from_bits(32_768)),
            Fixed::from_bits(32_768)
        );
        assert!(!SegmentMap::new(&[]).applies());
    }

    #[test]
    fn an_axis_scalar_follows_every_branch_of_the_pseudo_code() {
        let ramp = RegionAxis::from_peak(16_384);
        assert_eq!(
            ramp,
            RegionAxis {
                start: 0,
                peak: 16_384,
                end: 16_384
            }
        );
        assert_eq!(ramp.scalar(8_192), Fixed::from_bits(32_768));
        assert_eq!(ramp.scalar(16_384), Fixed::ONE);
        assert_eq!(ramp.scalar(-1), Fixed::ZERO, "outside the region");
        assert_eq!(
            RegionAxis::from_peak(-8_192).scalar(-4_096),
            Fixed::from_bits(32_768)
        );
        assert_eq!(RegionAxis::from_peak(0), RegionAxis::default());
        assert_eq!(
            RegionAxis::from_peak(0).scalar(5_000),
            Fixed::ONE,
            "peak zero"
        );

        let tooth = RegionAxis {
            start: 4_096,
            peak: 8_192,
            end: 16_384,
        };
        assert_eq!(tooth.scalar(6_144), Fixed::from_bits(32_768), "rising half");
        assert_eq!(
            tooth.scalar(12_288),
            Fixed::from_bits(32_768),
            "falling half"
        );
        assert_eq!(tooth.scalar(4_096), Fixed::ZERO, "at the start");
        assert_eq!(tooth.scalar(16_384), Fixed::ZERO, "at the end");
        assert_eq!(tooth.scalar(3_000), Fixed::ZERO, "below the start");
        // Invalid for the axis: ignored, which is one, not zero.
        let disordered = RegionAxis {
            start: 8_192,
            peak: 4_096,
            end: 16_384,
        };
        assert_eq!(disordered.scalar(0), Fixed::ONE);
        let crossing = RegionAxis {
            start: -4_096,
            peak: 8_192,
            end: 16_384,
        };
        assert_eq!(crossing.scalar(-16_384), Fixed::ONE);
        // A ratio that does not land on 16.16: a third.
        let third = RegionAxis::from_peak(3 * 4_096).scalar(4_096);
        assert_eq!(third, Fixed::from_bits(21_845));
    }

    #[test]
    fn a_region_scalar_multiplies_its_axes_in_order_rounding_each_product() {
        let location = Location::new(vec![4_096, 8_192, -16_384]);
        let axes = [
            RegionAxis::from_peak(3 * 4_096),
            RegionAxis::from_peak(16_384),
            RegionAxis::from_peak(0),
        ];
        // A third, then times a half: 21 845 · 32 768 / 65 536, a tie that
        // rounds up to 10 923.
        assert_eq!(region_scalar(&axes, &location), Fixed::from_bits(10_923));
        let outside = [
            RegionAxis::from_peak(-16_384),
            RegionAxis::from_peak(16_384),
        ];
        assert_eq!(region_scalar(&outside, &location), Fixed::ZERO);
        assert_eq!(
            region_scalar(&[], &location),
            Fixed::ONE,
            "no axes, no limit"
        );
        // A location shorter than the region reads its missing axes at zero.
        let short = Location::new(vec![16_384]);
        assert_eq!(region_scalar(&axes[1..], &short), Fixed::ONE);
        assert_eq!(region_scalar(&axes[..2], &short), Fixed::ZERO);
    }
}
