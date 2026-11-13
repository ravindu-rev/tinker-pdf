//! AAT lookup tables: a glyph index in, a value out.
//!
//! Apple's *TrueType Reference Manual*, *AAT special tables* chapter, "Lookup
//! Tables" and "Binary Searching Tables". A lookup table is a `uint16` format
//! and a header the format defines, and it answers a value for each glyph it
//! covers and nothing for any other. Six formats are defined:
//!
//! - **0**, simple array: one value a glyph, indexed by glyph index. The
//!   chapter gives the array no length, so it is as long as the bytes that
//!   hold it.
//! - **2**, segment single: segments of contiguous glyphs, `lastGlyph`,
//!   `firstGlyph` and one value the whole segment shares.
//! - **4**, segment array: the same segments, each holding instead "a 16-bit
//!   offset from the start of the table" to an array of one value a glyph.
//! - **6**, single table: `(glyph, value)` pairs.
//! - **8**, trimmed array: a first glyph, a count, and that many two-byte
//!   values.
//! - **10**, extended trimmed array: the same with values of `unitSize`
//!   bytes, "1, 2, 4, and 8".
//!
//! Formats 2, 4 and 6 open with a `BinSrchHeader`, whose `unitSize` and
//! `nUnits` are read and whose `searchRange`, `entrySelector` and `rangeShift`
//! "are no longer used", so they are not; their units are "sorted, according
//! to the first word in each one", and a unit whose glyph is `0xFFFF` is the
//! search's guard — "The value that indicates binary search termination is
//! 0xFFFF" — and answers nothing. A value's size is the unit's share past its
//! glyphs in formats 2 and 6, `unitSize` in format 10, and two bytes in
//! formats 0, 4 and 8, which state no size: the tables this crate reads them
//! for (`morx`'s classes and substitutions, `ankr`'s offsets) keep two-byte
//! values. Every read is a bounds check (`read::Bytes`), so a unit count past
//! the table, an offset past its end or a truncated header answers nothing
//! and never panics (ruling 1).

use crate::read::Bytes;

/// A lookup table's format, by the number the manual gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LookupFormat {
    /// Format 0, simple array.
    SimpleArray,
    /// Format 2, segment single.
    SegmentSingle,
    /// Format 4, segment array.
    SegmentArray,
    /// Format 6, single table.
    SingleTable,
    /// Format 8, trimmed array.
    TrimmedArray,
    /// Format 10, extended trimmed array.
    ExtendedTrimmedArray,
}

impl LookupFormat {
    /// The number the manual gives the format.
    #[must_use]
    pub const fn number(self) -> u16 {
        match self {
            LookupFormat::SimpleArray => 0,
            LookupFormat::SegmentSingle => 2,
            LookupFormat::SegmentArray => 4,
            LookupFormat::SingleTable => 6,
            LookupFormat::TrimmedArray => 8,
            LookupFormat::ExtendedTrimmedArray => 10,
        }
    }
}

/// Why a lookup table could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LookupError {
    /// A format the manual does not define, by its number.
    Format(u16),
    /// A header too short for its format, or a unit or value size the format
    /// does not allow.
    Malformed,
}

/// The value sizes a unit may give, in bytes: format 10's list, "1, 2, 4,
/// and 8", which formats 2 and 6 are held to as well.
const VALUE_SIZES: [usize; 4] = [1, 2, 4, 8];

/// The `BinSrchHeader` fields after the format: five `uint16`s.
const SEARCH_HEADER: usize = 2 + 10;

/// A lookup table, read.
#[derive(Clone, Copy, Debug)]
pub struct Lookup<'a> {
    /// The table, from its format field to the end of whatever holds it.
    data: Bytes<'a>,
    format: LookupFormat,
    /// The bytes of one value.
    value_size: usize,
    /// The bytes of one unit, in formats 2, 4, 6 and 10.
    unit_size: usize,
    /// How many units, in formats 2, 4 and 6, or values, in formats 8 and
    /// 10, the table holds.
    units: usize,
    /// The first glyph of a trimmed array.
    first: u16,
}

impl<'a> Lookup<'a> {
    /// Reads the lookup table at the start of `bytes`.
    ///
    /// # Errors
    ///
    /// [`LookupError::Format`] for a format the manual does not define, and
    /// [`LookupError::Malformed`] for a header that cannot be read or a unit
    /// or value size its format does not allow: a segment of fewer than six
    /// bytes or a pair of fewer than four, or a value of any size but 1, 2, 4
    /// or 8.
    pub fn parse(bytes: &'a [u8]) -> Result<Lookup<'a>, LookupError> {
        let data = Bytes::new(bytes);
        let number = data.u16(0).ok_or(LookupError::Malformed)?;
        let format = match number {
            0 => LookupFormat::SimpleArray,
            2 => LookupFormat::SegmentSingle,
            4 => LookupFormat::SegmentArray,
            6 => LookupFormat::SingleTable,
            8 => LookupFormat::TrimmedArray,
            10 => LookupFormat::ExtendedTrimmedArray,
            other => return Err(LookupError::Format(other)),
        };
        let mut lookup = Lookup {
            data,
            format,
            value_size: 2,
            unit_size: 0,
            units: 0,
            first: 0,
        };
        match format {
            LookupFormat::SimpleArray => {}
            LookupFormat::SegmentSingle
            | LookupFormat::SegmentArray
            | LookupFormat::SingleTable => {
                let unit_size = usize::from(data.u16(2).ok_or(LookupError::Malformed)?);
                let claimed = usize::from(data.u16(4).ok_or(LookupError::Malformed)?);
                if data.len() < SEARCH_HEADER {
                    return Err(LookupError::Malformed);
                }
                // The glyphs a unit names before its value: a segment's last
                // and first glyph, or a pair's one.
                let glyphs = if format == LookupFormat::SingleTable {
                    2
                } else {
                    4
                };
                lookup.value_size = match format {
                    LookupFormat::SegmentArray => 2,
                    _ => unit_size
                        .checked_sub(glyphs)
                        .ok_or(LookupError::Malformed)?,
                };
                if (format == LookupFormat::SegmentArray && unit_size < glyphs + 2)
                    || !VALUE_SIZES.contains(&lookup.value_size)
                {
                    return Err(LookupError::Malformed);
                }
                lookup.unit_size = unit_size;
                // A count past what the table holds is read as far as the
                // table goes.
                let room = (data.len() - SEARCH_HEADER) / unit_size;
                lookup.units = claimed.min(room);
            }
            LookupFormat::TrimmedArray => {
                lookup.first = data.u16(2).ok_or(LookupError::Malformed)?;
                lookup.units = usize::from(data.u16(4).ok_or(LookupError::Malformed)?);
            }
            LookupFormat::ExtendedTrimmedArray => {
                let unit_size = usize::from(data.u16(2).ok_or(LookupError::Malformed)?);
                if !VALUE_SIZES.contains(&unit_size) {
                    return Err(LookupError::Malformed);
                }
                lookup.value_size = unit_size;
                lookup.unit_size = unit_size;
                lookup.first = data.u16(4).ok_or(LookupError::Malformed)?;
                lookup.units = usize::from(data.u16(6).ok_or(LookupError::Malformed)?);
            }
        }
        Ok(lookup)
    }

    /// The table's format.
    #[must_use]
    pub const fn format(&self) -> LookupFormat {
        self.format
    }

    /// The bytes of one value this table answers.
    #[must_use]
    pub const fn value_size(&self) -> usize {
        self.value_size
    }

    /// The value the table gives `glyph`, or `None` for a glyph it does not
    /// cover — or covers with a value its bytes do not hold.
    #[must_use]
    pub fn value(&self, glyph: u16) -> Option<u64> {
        match self.format {
            LookupFormat::SimpleArray => {
                let at = 2usize.checked_add(usize::from(glyph).checked_mul(2)?)?;
                read_value(self.data, at, 2)
            }
            LookupFormat::SegmentSingle => {
                let unit = self.segment(glyph)?;
                read_value(self.data, unit.checked_add(4)?, self.value_size)
            }
            LookupFormat::SegmentArray => {
                let unit = self.segment(glyph)?;
                let first = self.data.u16(unit.checked_add(2)?)?;
                let offset = usize::from(self.data.u16(unit.checked_add(4)?)?);
                let index = usize::from(glyph.checked_sub(first)?);
                read_value(self.data, offset.checked_add(index.checked_mul(2)?)?, 2)
            }
            LookupFormat::SingleTable => {
                let unit = self.pair(glyph)?;
                read_value(self.data, unit.checked_add(2)?, self.value_size)
            }
            LookupFormat::TrimmedArray => {
                let index = usize::from(glyph.checked_sub(self.first)?);
                if index >= self.units {
                    return None;
                }
                read_value(self.data, 6usize.checked_add(index.checked_mul(2)?)?, 2)
            }
            LookupFormat::ExtendedTrimmedArray => {
                let index = usize::from(glyph.checked_sub(self.first)?);
                if index >= self.units {
                    return None;
                }
                let at = 8usize.checked_add(index.checked_mul(self.unit_size)?)?;
                read_value(self.data, at, self.value_size)
            }
        }
    }

    /// The value the table gives `glyph`, where it fits sixteen bits: a
    /// class, a glyph, or an offset.
    #[must_use]
    pub fn value16(&self, glyph: u16) -> Option<u16> {
        self.value(glyph)
            .and_then(|value| u16::try_from(value).ok())
    }

    /// Where unit `index` starts.
    fn unit(&self, index: usize) -> Option<usize> {
        SEARCH_HEADER.checked_add(index.checked_mul(self.unit_size)?)
    }

    /// The segment of format 2 or 4 holding `glyph`: the first, in the
    /// table's sort order, whose last glyph is not below it, provided its
    /// first glyph is not above it and it is not the guard.
    fn segment(&self, glyph: u16) -> Option<usize> {
        let (mut low, mut high) = (0usize, self.units);
        while low < high {
            let middle = low + (high - low) / 2;
            if self.data.u16(self.unit(middle)?)? < glyph {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if low >= self.units {
            return None;
        }
        let unit = self.unit(low)?;
        let last = self.data.u16(unit)?;
        let first = self.data.u16(unit.checked_add(2)?)?;
        let guard = first == 0xFFFF && last == 0xFFFF;
        ((first..=last).contains(&glyph) && !guard).then_some(unit)
    }

    /// The pair of format 6 naming `glyph`, found by the same search over the
    /// pairs' glyphs; the guard's `0xFFFF` names nothing.
    fn pair(&self, glyph: u16) -> Option<usize> {
        if glyph == 0xFFFF {
            return None;
        }
        let (mut low, mut high) = (0usize, self.units);
        while low < high {
            let middle = low + (high - low) / 2;
            if self.data.u16(self.unit(middle)?)? < glyph {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if low >= self.units {
            return None;
        }
        let unit = self.unit(low)?;
        (self.data.u16(unit)? == glyph).then_some(unit)
    }
}

/// The big-endian value of `size` bytes at `at`.
fn read_value(data: Bytes<'_>, at: usize, size: usize) -> Option<u64> {
    match size {
        1 => data.u8(at).map(u64::from),
        2 => data.u16(at).map(u64::from),
        4 => data.u32(at).map(u64::from),
        8 => {
            let high = u64::from(data.u32(at)?);
            let low = u64::from(data.u32(at.checked_add(4)?)?);
            Some((high << 32) | low)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Lookup, LookupError, LookupFormat};

    fn u16s(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    #[test]
    fn a_format_the_manual_does_not_define_is_named() {
        for format in [1u16, 3, 5, 7, 9, 11, 0xFFFF] {
            let table = u16s(&[format, 0, 0, 0, 0, 0]);
            assert_eq!(
                Lookup::parse(&table).err(),
                Some(LookupError::Format(format))
            );
        }
        assert_eq!(Lookup::parse(&[]).err(), Some(LookupError::Malformed));
        assert_eq!(Lookup::parse(&[0]).err(), Some(LookupError::Malformed));
    }

    #[test]
    fn every_format_reports_its_number() {
        for (number, format) in [
            (0, LookupFormat::SimpleArray),
            (2, LookupFormat::SegmentSingle),
            (4, LookupFormat::SegmentArray),
            (6, LookupFormat::SingleTable),
            (8, LookupFormat::TrimmedArray),
            (10, LookupFormat::ExtendedTrimmedArray),
        ] {
            assert_eq!(format.number(), number);
        }
    }

    /// A segment or pair shorter than its glyphs and a two-byte value, or a
    /// value of a size format 10 does not list, is no table to read.
    #[test]
    fn a_unit_too_small_or_a_value_of_an_unlisted_size_is_malformed() {
        // Format 2 with five-byte segments: a one-byte value is listed.
        let five = u16s(&[2, 5, 0, 0, 0, 0]);
        assert!(Lookup::parse(&five).is_ok());
        // Four-byte segments leave no value; three-byte values are unlisted.
        for size in [0u16, 2, 4, 7, 9] {
            let table = u16s(&[2, size, 0, 0, 0, 0]);
            assert_eq!(
                Lookup::parse(&table).err(),
                Some(LookupError::Malformed),
                "{size}"
            );
        }
        // Format 4's segments must hold their offset.
        let four = u16s(&[4, 5, 0, 0, 0, 0]);
        assert_eq!(Lookup::parse(&four).err(), Some(LookupError::Malformed));
        // Format 6 pairs of two bytes hold no value.
        let six = u16s(&[6, 2, 0, 0, 0, 0]);
        assert_eq!(Lookup::parse(&six).err(), Some(LookupError::Malformed));
        // Format 10's unit sizes.
        for size in [0u16, 3, 5, 6, 16] {
            let table = u16s(&[10, size, 0, 0]);
            assert_eq!(Lookup::parse(&table).err(), Some(LookupError::Malformed));
        }
        // A binary-search header cut short.
        assert_eq!(
            Lookup::parse(&u16s(&[6, 4, 1])).err(),
            Some(LookupError::Malformed)
        );
    }

    /// A unit count past the table is read as far as the table goes, and a
    /// glyph past it answers nothing.
    #[test]
    fn a_count_past_the_table_reads_what_the_table_holds() {
        // Format 6 claiming 400 pairs and holding two.
        let mut table = u16s(&[6, 4, 400, 0, 0, 0]);
        table.extend(u16s(&[3, 30, 5, 50]));
        let lookup = Lookup::parse(&table).expect("readable");
        assert_eq!(lookup.value(3), Some(30));
        assert_eq!(lookup.value(5), Some(50));
        assert_eq!(lookup.value(4), None);
        // Format 8 claiming ten values and holding three.
        let mut trimmed = u16s(&[8, 7, 10]);
        trimmed.extend(u16s(&[70, 80, 90]));
        let lookup = Lookup::parse(&trimmed).expect("readable");
        assert_eq!(lookup.value(9), Some(90));
        assert_eq!(lookup.value(10), None, "its value is past the table");
        assert_eq!(lookup.value(6), None);
    }
}
