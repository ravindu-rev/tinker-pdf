//! `cvar`: the control values at an instance (OpenType 1.9.1, the `cvar`
//! chapter).
//!
//! The whole table is one tuple variation store after a version, every peak
//! embedded — the store has no shared tuples to take one from, so a table
//! that leaves `EMBEDDED_PEAK_TUPLE` clear is refused by
//! [`VariationError::SharedTupleIndex`]. A point number is a `cvt ` index, one
//! delta each, and an index past the `cvt ` table's `length / 2` entries
//! moves nothing. The values come back in 16.16, the numeric model's
//! (`docs/design/variable-fonts.md`): each FWORD plus every table's scalar
//! times its integer delta, exactly. They reach a page only through a static
//! instance, whose hinting is the bytecode interpreter's.

use super::tuple::{SharedTuples, TupleStore};
use super::{be16, tag_value, DesignSpace, Fixed, Location, Tag, VariationError};
use crate::sfnt::Sfnt;

const TAG_CVAR: Tag = *b"cvar";
const TAG_CVT: Tag = *b"cvt ";

/// A parsed `cvar` table.
#[derive(Clone, Debug)]
pub struct Cvar<'a> {
    store: TupleStore<'a>,
}

impl<'a> Cvar<'a> {
    /// Parses a `cvar` table for a design space of `axis_count` axes.
    ///
    /// # Errors
    ///
    /// A version the chapter does not define, or a header past the end.
    pub fn parse(table: &'a [u8], axis_count: u16) -> Result<Cvar<'a>, VariationError> {
        let major = be16(table, 0).ok_or(VariationError::HeaderPastEnd { table: TAG_CVAR })?;
        let minor = be16(table, 2).ok_or(VariationError::HeaderPastEnd { table: TAG_CVAR })?;
        if major != 1 {
            return Err(VariationError::UnknownVersion {
                table: TAG_CVAR,
                major,
                minor,
            });
        }
        let store = TupleStore::parse(table, 4, axis_count, SharedTuples::none())?;
        Ok(Cvar { store })
    }

    /// The tuple variation store the table is.
    #[must_use]
    pub fn store(&self) -> &TupleStore<'a> {
        &self.store
    }

    /// The `cvt ` values at a location, in 16.16: each FWORD of `cvt` plus,
    /// for every tuple variation table, its scalar times its delta for that
    /// index. Every table's deltas are decoded wherever its scalar is zero
    /// too, so whether a table is refused does not depend on the location.
    ///
    /// # Errors
    ///
    /// The store's refusals ([`TupleStore::tuples`],
    /// [`super::tuple::TupleVariation::deltas`]).
    pub fn apply(&self, cvt: &[u8], location: &Location) -> Result<Vec<Fixed>, VariationError> {
        let entries = cvt.len() / 2;
        let mut values: Vec<Fixed> = (0..entries)
            .map(|i| Fixed::from_int(i32::from(be16(cvt, i * 2).unwrap_or(0) as i16)))
            .collect();
        for tuple in self.store.tuples()? {
            let scalar = tuple.scalar(location);
            let points = tuple.points();
            let deltas = tuple.deltas(points.count(entries))?;
            if scalar == Fixed::ZERO {
                continue;
            }
            for (index, delta) in points.numbers(entries).into_iter().zip(deltas) {
                if let Some(value) = values.get_mut(index) {
                    *value = value.saturating_add(scalar.scaled(delta));
                }
            }
        }
        Ok(values)
    }
}

/// A face's `cvt ` values at a location, in 16.16: its defaults where it
/// carries no `cvar` or does not vary, and none where it has no `cvt `.
///
/// # Errors
///
/// The `cvar` table's refusals. `cvar` carries no axis count of its own, so
/// its tuples are read with the design space's.
pub fn cvt_at(
    sfnt: &Sfnt,
    space: &DesignSpace,
    location: &Location,
) -> Result<Vec<Fixed>, VariationError> {
    let cvt = sfnt.table(tag_value(TAG_CVT)).unwrap_or(&[]);
    let cvar = sfnt
        .table(tag_value(TAG_CVAR))
        .filter(|_| space.is_variable());
    match cvar {
        Some(table) => {
            // `MAX_AXES` keeps the count inside sixteen bits.
            let axes = u16::try_from(space.axis_count()).unwrap_or(u16::MAX);
            Cvar::parse(table, axes)?.apply(cvt, location)
        }
        None => Ok((0..cvt.len() / 2)
            .map(|i| Fixed::from_int(i32::from(be16(cvt, i * 2).unwrap_or(0) as i16)))
            .collect()),
    }
}
