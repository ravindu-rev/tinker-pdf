//! `gvar`: TrueType outlines at an instance (OpenType 1.9.1, the `gvar`
//! chapter).
//!
//! The table is a header, the shared tuples, and one `GlyphVariationData`
//! table a glyph — a tuple variation store ([`TupleStore`]) whose point
//! numbers are a simple glyph's points, or a composite's components in the
//! order its records give them, and then the four phantom points either way
//! ('Point numbers and processing for composite glyphs').
//! [`Gvar::simple_deltas`] and [`Gvar::composite_deltas`] sum every table's
//! scaled deltas in 16.16, the numeric model's (`docs/design/
//! variable-fonts.md`): an integer delta times its scalar exactly, and a
//! delta inferred for a point the table does not list — rounded to 16.16 once
//! from its exact value — times its scalar rounded as every 16.16 product is.
//! `crate::glyf::outline_at` adds them to the points before the implied
//! midpoints are resolved.
//!
//! **Inferred deltas** ('Inferred deltas for un-referenced point numbers')
//! are a simple glyph's only, contour by contour, from the points' default
//! positions and the table's unscaled deltas: a contour the table lists no
//! point of moves nothing, and an unlisted point takes, on each axis, the
//! delta its two nearest listed neighbours give it — the same delta where the
//! two stand at one coordinate and agree, none where they disagree, the
//! nearer one's where the point is outside them, and the linear
//! interpolation between them otherwise. Phantom points belong to no contour
//! and are moved only where a table lists them. A composite's components are
//! moved only where listed too, and its phantom points take the X deltas of
//! the two horizontal ones and the Y deltas of the two vertical ones.
//!
//! **Bounded work.** Every table is decoded at every location, as `cvar`'s
//! are, so whether a glyph is refused does not depend on where it is asked
//! for; and a glyph's tables times its points may not pass
//! [`MAX_GLYPH_DELTA_WORK`], since four thousand tables over sixty-five
//! thousand points is a few bytes each and a quarter of a billion steps.

use super::tuple::{PointNumbers, SharedTuples, TupleStore};
use super::{
    be16, be32, div_round, saturate, tag_value, DesignSpace, Fixed, Location, Tag, VariationError,
};
use crate::glyf::SimpleGlyph;
use crate::sfnt::Sfnt;

const TAG_GVAR: Tag = *b"gvar";

/// The most point deltas one outline may decode and apply: a glyph's tuple
/// variation tables times its points and phantom points, summed over every
/// glyph a composite draws. Past it the outline is refused by name
/// ([`VariationError::GlyphDeltaWork`]). A face's glyph moves a few hundred
/// points by a few dozen tables; four million is the work of a thousand
/// tables over four thousand points.
pub const MAX_GLYPH_DELTA_WORK: u64 = 1 << 22;

/// The size of `gvar`'s header before its offset array.
const HEADER: usize = 20;

/// A parsed `gvar` table.
#[derive(Clone, Debug)]
pub struct Gvar<'a> {
    table: &'a [u8],
    axis_count: u16,
    shared: SharedTuples<'a>,
    glyph_count: u16,
    long_offsets: bool,
    array_at: usize,
}

impl<'a> Gvar<'a> {
    /// Parses a `gvar` table for a design space of `axis_count` axes, from
    /// the header the chapter's field list gives.
    ///
    /// # Errors
    ///
    /// A header or the offset array past the table's end, a version other
    /// than 1, an `axisCount` other than `axis_count`, and shared tuples past
    /// the table's end — by name.
    pub fn parse(table: &'a [u8], axis_count: u16) -> Result<Gvar<'a>, VariationError> {
        let header = |at| be16(table, at).ok_or(VariationError::HeaderPastEnd { table: TAG_GVAR });
        let major = header(0)?;
        let minor = header(2)?;
        if major != 1 {
            return Err(VariationError::UnknownVersion {
                table: TAG_GVAR,
                major,
                minor,
            });
        }
        let axes = header(4)?;
        if axes != axis_count {
            return Err(VariationError::AxisCount {
                table: TAG_GVAR,
                found: axes,
                expected: axis_count,
            });
        }
        let shared_count = header(6)?;
        let shared_at = be32(table, 8).ok_or(VariationError::HeaderPastEnd { table: TAG_GVAR })?;
        let glyph_count = header(12)?;
        let flags = header(14)?;
        let array_at = be32(table, 16).ok_or(VariationError::HeaderPastEnd { table: TAG_GVAR })?;
        let long_offsets = flags & 1 != 0;
        let width = if long_offsets { 4 } else { 2 };
        (usize::from(glyph_count) + 1)
            .checked_mul(width)
            .and_then(|bytes| bytes.checked_add(HEADER))
            .filter(|end| *end <= table.len())
            .ok_or(VariationError::GlyphOffsetsPastEnd { count: glyph_count })?;
        let shared_bytes = usize::try_from(shared_at)
            .ok()
            .and_then(|at| table.get(at..))
            .ok_or(VariationError::SharedTuplesPastEnd {
                count: shared_count,
            })?;
        let shared = SharedTuples::new(shared_bytes, shared_count, axis_count)?;
        Ok(Gvar {
            table,
            axis_count,
            shared,
            glyph_count,
            long_offsets,
            array_at: usize::try_from(array_at).unwrap_or(usize::MAX),
        })
    }

    /// A face's `gvar`, where it varies and carries one.
    ///
    /// # Errors
    ///
    /// [`Gvar::parse`]'s.
    pub fn read(sfnt: &Sfnt<'a>, space: &DesignSpace) -> Result<Option<Gvar<'a>>, VariationError> {
        match sfnt.table(tag_value(TAG_GVAR)) {
            Some(table) if space.is_variable() => {
                // `MAX_AXES` keeps the count inside sixteen bits.
                let axes = u16::try_from(space.axis_count()).unwrap_or(u16::MAX);
                Gvar::parse(table, axes).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// The glyph count the table states, which must be the face's; a glyph
    /// past it has no variation data.
    #[must_use]
    pub fn glyph_count(&self) -> u16 {
        self.glyph_count
    }

    /// The shared tuples.
    #[must_use]
    pub fn shared_tuples(&self) -> SharedTuples<'a> {
        self.shared
    }

    /// One `glyphVariationDataOffsets` entry, in bytes from the array.
    fn offset(&self, index: usize) -> Option<usize> {
        if self.long_offsets {
            be32(self.table, HEADER + index * 4).and_then(|v| usize::try_from(v).ok())
        } else {
            be16(self.table, HEADER + index * 2).map(|v| usize::from(v) * 2)
        }
    }

    /// A glyph's `GlyphVariationData` as a tuple variation store; `None`
    /// where its size is zero or the glyph is past the table's count, which
    /// is the same outline across the whole design space.
    ///
    /// # Errors
    ///
    /// [`VariationError::GlyphDataOffsets`] where the glyph's two offsets
    /// decrease, [`VariationError::GlyphDataPastEnd`] where its data runs
    /// past the table, and the store's header refusals.
    pub fn glyph(&self, glyph: u16) -> Result<Option<TupleStore<'a>>, VariationError> {
        if glyph >= self.glyph_count {
            return Ok(None);
        }
        let index = usize::from(glyph);
        // The offset array was bounded by `parse` for every glyph and one.
        let (Some(start), Some(end)) = (self.offset(index), self.offset(index + 1)) else {
            return Err(VariationError::GlyphOffsetsPastEnd {
                count: self.glyph_count,
            });
        };
        if end < start {
            return Err(VariationError::GlyphDataOffsets { glyph });
        }
        if end == start {
            return Ok(None);
        }
        let data = self
            .array_at
            .checked_add(start)
            .zip(self.array_at.checked_add(end))
            .and_then(|(from, to)| self.table.get(from..to))
            .ok_or(VariationError::GlyphDataPastEnd { glyph })?;
        TupleStore::parse(data, 0, self.axis_count, self.shared).map(Some)
    }

    /// The net deltas of a simple glyph's points at a location, in 16.16
    /// font units: one `[x, y]` for each of its contour points and its four
    /// phantom points, in that order — [`SimpleGlyph::points`]' order. An
    /// untouched point's delta is inferred contour by contour, as the module
    /// says; a glyph with no variation data moves nothing.
    ///
    /// # Errors
    ///
    /// The glyph's store's refusals, and [`VariationError::GlyphDeltaWork`]
    /// past [`MAX_GLYPH_DELTA_WORK`].
    pub fn simple_deltas(
        &self,
        glyph: u16,
        points: &SimpleGlyph,
        location: &Location,
    ) -> Result<Vec<[Fixed; 2]>, VariationError> {
        self.simple_deltas_charged(glyph, points, location, &mut 0)
    }

    /// [`Gvar::simple_deltas`], charging its work to `work`, which may not
    /// pass [`MAX_GLYPH_DELTA_WORK`].
    pub(crate) fn simple_deltas_charged(
        &self,
        glyph: u16,
        points: &SimpleGlyph,
        location: &Location,
        work: &mut u64,
    ) -> Result<Vec<[Fixed; 2]>, VariationError> {
        let items = points.points.len();
        let mut net = vec![[Fixed::ZERO; 2]; items];
        let Some(store) = self.glyph(glyph)? else {
            return Ok(net);
        };
        let tuples = store.tuples()?;
        charge(work, tuples.len(), items, glyph)?;
        let contours = points.contours();
        for tuple in &tuples {
            let numbers = tuple.points();
            let deltas = tuple.deltas(numbers.count(items).saturating_mul(2))?;
            let scalar = tuple.scalar(location);
            if scalar == Fixed::ZERO {
                continue;
            }
            match numbers {
                PointNumbers::All => apply_all(&mut net, &deltas, scalar),
                PointNumbers::Listed(listed) => {
                    let explicit = listed_deltas(listed, &deltas, items);
                    for (slot, delta) in net.iter_mut().zip(&explicit) {
                        if let Some(delta) = delta {
                            for axis in 0..2 {
                                slot[axis] = slot[axis].saturating_add(scalar.scaled(delta[axis]));
                            }
                        }
                    }
                    for &(first, last) in &contours {
                        infer_contour(&points.points, &explicit, first, last, scalar, &mut net);
                    }
                }
            }
        }
        Ok(net)
    }

    /// The net deltas of a composite's components and its phantom points at
    /// a location, in 16.16 font units: one `[x, y]` for each of its
    /// `components` records in order and then each phantom point, the two
    /// horizontal phantom points keeping their X deltas and the two vertical
    /// ones their Y deltas, as the chapter's composite process applies them.
    /// Nothing is inferred for a composite.
    ///
    /// # Errors
    ///
    /// The glyph's store's refusals, and [`VariationError::GlyphDeltaWork`]
    /// past [`MAX_GLYPH_DELTA_WORK`].
    pub fn composite_deltas(
        &self,
        glyph: u16,
        components: usize,
        location: &Location,
    ) -> Result<Vec<[Fixed; 2]>, VariationError> {
        self.composite_deltas_charged(glyph, components, location, &mut 0)
    }

    /// [`Gvar::composite_deltas`], charging its work to `work`.
    pub(crate) fn composite_deltas_charged(
        &self,
        glyph: u16,
        components: usize,
        location: &Location,
        work: &mut u64,
    ) -> Result<Vec<[Fixed; 2]>, VariationError> {
        let mut net = self.listed_only(glyph, components.saturating_add(4), location, work)?;
        // `components + 4` items, so the four phantom points are the last:
        // the left side bearing and advance width points keep their X
        // deltas, the top side bearing and advance height points their Y.
        let at = net.len().saturating_sub(4);
        if let Some(phantom) = net.get_mut(at..) {
            for (index, point) in phantom.iter_mut().enumerate() {
                let ignored = if index < 2 { 1 } else { 0 };
                point[ignored] = Fixed::ZERO;
            }
        }
        Ok(net)
    }

    /// The net deltas of a glyph's four phantom points at a location, its
    /// first `points` items being its contour points or its components:
    /// what [`Gvar::simple_deltas`] or [`Gvar::composite_deltas`] gives the
    /// last four items, with no contour walked, since a phantom point is
    /// never inferred.
    ///
    /// # Errors
    ///
    /// As [`Gvar::simple_deltas`].
    pub fn phantom_deltas(
        &self,
        glyph: u16,
        points: usize,
        composite: bool,
        location: &Location,
    ) -> Result<[[Fixed; 2]; 4], VariationError> {
        let mut work = 0;
        let net = if composite {
            self.composite_deltas_charged(glyph, points, location, &mut work)?
        } else {
            self.listed_only(glyph, points.saturating_add(4), location, &mut work)?
        };
        let mut phantom = [[Fixed::ZERO; 2]; 4];
        if let Some(last) = net.get(net.len().saturating_sub(4)..) {
            for (slot, delta) in phantom.iter_mut().zip(last) {
                *slot = *delta;
            }
        }
        Ok(phantom)
    }

    /// Every table's deltas for `items` items, the listed ones only.
    fn listed_only(
        &self,
        glyph: u16,
        items: usize,
        location: &Location,
        work: &mut u64,
    ) -> Result<Vec<[Fixed; 2]>, VariationError> {
        let mut net = vec![[Fixed::ZERO; 2]; items];
        let Some(store) = self.glyph(glyph)? else {
            return Ok(net);
        };
        let tuples = store.tuples()?;
        charge(work, tuples.len(), items, glyph)?;
        for tuple in &tuples {
            let numbers = tuple.points();
            let deltas = tuple.deltas(numbers.count(items).saturating_mul(2))?;
            let scalar = tuple.scalar(location);
            if scalar == Fixed::ZERO {
                continue;
            }
            match numbers {
                PointNumbers::All => apply_all(&mut net, &deltas, scalar),
                PointNumbers::Listed(listed) => {
                    let explicit = listed_deltas(listed, &deltas, items);
                    for (slot, delta) in net.iter_mut().zip(&explicit) {
                        if let Some(delta) = delta {
                            for axis in 0..2 {
                                slot[axis] = slot[axis].saturating_add(scalar.scaled(delta[axis]));
                            }
                        }
                    }
                }
            }
        }
        Ok(net)
    }
}

/// Charges `tables` tables over `items` items to `work`, refusing the glyph
/// past [`MAX_GLYPH_DELTA_WORK`].
fn charge(work: &mut u64, tables: usize, items: usize, glyph: u16) -> Result<(), VariationError> {
    let cost = u64::try_from(tables)
        .unwrap_or(u64::MAX)
        .saturating_mul(u64::try_from(items).unwrap_or(u64::MAX));
    *work = work.saturating_add(cost);
    if *work > MAX_GLYPH_DELTA_WORK {
        return Err(VariationError::GlyphDeltaWork { glyph });
    }
    Ok(())
}

/// A table that lists every item: its X deltas then its Y deltas, one each an
/// item, scaled exactly.
fn apply_all(net: &mut [[Fixed; 2]], deltas: &[i32], scalar: Fixed) {
    let items = net.len();
    for (index, slot) in net.iter_mut().enumerate() {
        let x = deltas.get(index).copied().unwrap_or(0);
        let y = deltas.get(items + index).copied().unwrap_or(0);
        slot[0] = slot[0].saturating_add(scalar.scaled(x));
        slot[1] = slot[1].saturating_add(scalar.scaled(y));
    }
}

/// The deltas a table lists, by item: the sum of its deltas for each number
/// it lists, once for each time it lists it, and `None` for an item it does
/// not list. A number past the items moves nothing.
fn listed_deltas(listed: &[u16], deltas: &[i32], items: usize) -> Vec<Option<[i32; 2]>> {
    let count = listed.len();
    let mut explicit: Vec<Option<[i32; 2]>> = vec![None; items];
    for (at, number) in listed.iter().enumerate() {
        let Some(slot) = explicit.get_mut(usize::from(*number)) else {
            continue;
        };
        let x = deltas.get(at).copied().unwrap_or(0);
        let y = deltas.get(count + at).copied().unwrap_or(0);
        let sum = slot.get_or_insert([0, 0]);
        sum[0] = sum[0].saturating_add(x);
        sum[1] = sum[1].saturating_add(y);
    }
    explicit
}

/// The deltas one table infers for a contour's unlisted points, scaled and
/// added to `net`: each run of unlisted points between two listed ones,
/// taken around the contour, reads those two — the nearest listed point
/// before it in point order, or the contour's last listed one where none
/// precedes it, and the nearest after it, or the first.
fn infer_contour(
    points: &[[i32; 2]],
    explicit: &[Option<[i32; 2]>],
    first: usize,
    last: usize,
    scalar: Fixed,
    net: &mut [[Fixed; 2]],
) {
    let listed: Vec<usize> = (first..=last)
        .filter(|i| explicit.get(*i).is_some_and(Option::is_some))
        .collect();
    let len = last + 1 - first;
    if listed.is_empty() || listed.len() == len {
        return;
    }
    let after = |i: usize| if i == last { first } else { i + 1 };
    for (k, &before) in listed.iter().enumerate() {
        let next = listed[(k + 1) % listed.len()];
        let mut point = after(before);
        while point != next {
            for axis in 0..2 {
                let delta = inferred(points, explicit, point, before, next, axis);
                if let Some(slot) = net.get_mut(point) {
                    slot[axis] = slot[axis].saturating_add(delta.mul_rounded(scalar));
                }
            }
            point = after(point);
        }
    }
}

/// One axis's inferred delta for `point`, unscaled, from its listed
/// neighbours `before` and `after`, by the chapter's pseudo-code; the
/// interpolation's exact value rounded to 16.16 once.
fn inferred(
    points: &[[i32; 2]],
    explicit: &[Option<[i32; 2]>],
    point: usize,
    before: usize,
    after: usize,
    axis: usize,
) -> Fixed {
    let coordinate = |i: usize| points.get(i).map_or(0, |p| i128::from(p[axis]));
    let delta = |i: usize| {
        explicit
            .get(i)
            .copied()
            .flatten()
            .map_or(0, |d| i128::from(d[axis]))
    };
    let (target, cp, cn) = (coordinate(point), coordinate(before), coordinate(after));
    let (dp, dn) = (delta(before), delta(after));
    let whole = |value: i128| Fixed::from_bits(saturate(value << 16));
    if cp == cn {
        return if dp == dn { whole(dp) } else { Fixed::ZERO };
    }
    if target <= cp.min(cn) {
        return whole(if cp < cn { dp } else { dn });
    }
    if target >= cp.max(cn) {
        return whole(if cp > cn { dp } else { dn });
    }
    // Between the two: `dp + (target − cp) · (dn − dp) / (cn − cp)`, exactly
    // `(dp · (cn − target) + dn · (target − cp)) / (cn − cp)`, which is the
    // same whichever neighbour is the reference.
    let numerator = (dp * (cn - target) + dn * (target - cp)) << 16;
    Fixed::from_bits(div_round(numerator, cn - cp).map_or(0, saturate))
}
