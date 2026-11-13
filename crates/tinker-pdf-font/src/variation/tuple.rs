//! The tuple variation store `gvar` and `cvar` share (OpenType 1.9.1,
//! *OpenType Font Variations Common Table Formats*, 'Tuple Variation
//! Store').
//!
//! A store is a header — a count of tuple variation tables with the
//! `SHARED_POINT_NUMBERS` flag, an offset to the serialized data, and one
//! `TupleVariationHeader` a table — and the serialized data: shared point
//! numbers, when the flag says so, then each table's private point numbers,
//! where it has them, and its packed deltas. A table's region is its peak,
//! embedded or taken from `gvar`'s shared tuples, and, for an intermediate
//! region, the start and end it embeds.
//!
//! **Every count is bounded by the format or refused.** The count is twelve
//! bits, so 4 095 tables; a point count is fifteen bits; a run that reads
//! more numbers than its count, or bytes past its table's
//! `variationDataSize`, is refused by name, so the work a store asks for is
//! linear in its bytes (ruling 1).

use super::{be16, region_scalar, Fixed, Location, RegionAxis, VariationError};

/// `SHARED_POINT_NUMBERS`, in `tupleVariationCount`.
const SHARED_POINT_NUMBERS: u16 = 0x8000;
/// The low twelve bits of `tupleVariationCount`.
const COUNT_MASK: u16 = 0x0FFF;
/// `EMBEDDED_PEAK_TUPLE`, in `tupleIndex`.
const EMBEDDED_PEAK_TUPLE: u16 = 0x8000;
/// `INTERMEDIATE_REGION`, in `tupleIndex`.
const INTERMEDIATE_REGION: u16 = 0x4000;
/// `PRIVATE_POINT_NUMBERS`, in `tupleIndex`.
const PRIVATE_POINT_NUMBERS: u16 = 0x2000;
/// The low twelve bits of `tupleIndex`: the shared tuple's index.
const TUPLE_INDEX_MASK: u16 = 0x0FFF;

/// `gvar`'s shared tuple records, or none — `cvar` has none, and requires
/// every peak to be embedded.
#[derive(Clone, Copy, Debug, Default)]
pub struct SharedTuples<'a> {
    data: &'a [u8],
    count: u16,
    axis_count: u16,
}

impl<'a> SharedTuples<'a> {
    /// No shared tuples.
    #[must_use]
    pub fn none() -> SharedTuples<'a> {
        SharedTuples::default()
    }

    /// `count` tuple records of `axis_count` 2.14 coordinates each, at the
    /// start of `data`.
    ///
    /// # Errors
    ///
    /// [`VariationError::SharedTuplesPastEnd`] when the records run past the
    /// end of `data`.
    pub fn new(
        data: &'a [u8],
        count: u16,
        axis_count: u16,
    ) -> Result<SharedTuples<'a>, VariationError> {
        let bytes = usize::from(count) * usize::from(axis_count) * 2;
        if bytes > data.len() {
            return Err(VariationError::SharedTuplesPastEnd { count });
        }
        Ok(SharedTuples {
            data,
            count,
            axis_count,
        })
    }

    /// How many records there are.
    #[must_use]
    pub fn len(&self) -> u16 {
        self.count
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// One record's coordinates.
    #[must_use]
    pub fn get(&self, index: u16) -> Option<Vec<i16>> {
        if index >= self.count {
            return None;
        }
        let axes = usize::from(self.axis_count);
        let at = usize::from(index) * axes * 2;
        (0..axes)
            .map(|axis| be16(self.data, at + axis * 2).map(|v| v as i16))
            .collect()
    }
}

/// Which items a tuple variation table moves: every one — every point and
/// its four phantom points in `gvar`, every `cvt ` entry in `cvar` — or the
/// ones listed, in increasing order, a number repeated once for each delta
/// it takes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PointNumbers {
    /// Every item.
    All,
    /// The items listed.
    Listed(Vec<u16>),
}

impl PointNumbers {
    /// How many logical deltas a run of these numbers carries for one
    /// coordinate, given how many items there are.
    #[must_use]
    pub fn count(&self, items: usize) -> usize {
        match self {
            PointNumbers::All => items,
            PointNumbers::Listed(numbers) => numbers.len(),
        }
    }

    /// The item each delta belongs to, in order.
    #[must_use]
    pub fn numbers(&self, items: usize) -> Vec<usize> {
        match self {
            PointNumbers::All => (0..items).collect(),
            PointNumbers::Listed(numbers) => numbers.iter().map(|n| usize::from(*n)).collect(),
        }
    }
}

/// Packed point numbers at the start of `data`, and how many bytes they
/// took (common formats, 'Packed "Point" Numbers').
///
/// A first byte of zero is every point. Otherwise the count is that byte,
/// or, with its high bit set, fifteen bits of it and the next; runs follow,
/// each a control byte — `POINTS_ARE_WORDS` and the run's length less one —
/// and its numbers, each the difference from the number before it.
///
/// # Errors
///
/// [`VariationError::PointsPastEnd`] when the data ends first,
/// [`VariationError::PointRunsOverrun`] when a run holds more numbers than
/// the count has left, and [`VariationError::PointNumberOverflow`] when the
/// differences pass 65 535.
pub fn unpack_points(data: &[u8]) -> Result<(PointNumbers, usize), VariationError> {
    let byte = |at: usize| data.get(at).copied().ok_or(VariationError::PointsPastEnd);
    let first = byte(0)?;
    if first == 0 {
        return Ok((PointNumbers::All, 1));
    }
    let (count, mut at) = if first & 0x80 != 0 {
        (usize::from(u16::from_be_bytes([first & 0x7F, byte(1)?])), 2)
    } else {
        (usize::from(first), 1)
    };
    let mut numbers = Vec::with_capacity(count);
    let mut last = 0u32;
    while numbers.len() < count {
        let control = byte(at)?;
        at += 1;
        let run = usize::from(control & 0x7F) + 1;
        if numbers.len() + run > count {
            return Err(VariationError::PointRunsOverrun);
        }
        let words = control & 0x80 != 0;
        for _ in 0..run {
            let difference = if words {
                let value = be16(data, at).ok_or(VariationError::PointsPastEnd)?;
                at += 2;
                u32::from(value)
            } else {
                let value = byte(at)?;
                at += 1;
                u32::from(value)
            };
            last += difference;
            let number = u16::try_from(last).map_err(|_| VariationError::PointNumberOverflow)?;
            numbers.push(number);
        }
    }
    Ok((PointNumbers::Listed(numbers), at))
}

/// `count` packed deltas at the start of `data`, and how many bytes they
/// took (common formats, 'Packed Deltas').
///
/// Runs, each a control byte — `DELTAS_ARE_ZERO`, `DELTAS_ARE_WORDS` and the
/// run's length less one — and its values, none for a run of zeros, signed
/// words or signed bytes otherwise. `gvar`'s X and Y deltas are one stream of
/// twice the point count, which a run may cross.
///
/// # Errors
///
/// [`VariationError::DeltasPastEnd`] when the data ends first, and
/// [`VariationError::DeltaRunsOverrun`] when a run holds more deltas than the
/// count has left.
pub fn unpack_deltas(data: &[u8], count: usize) -> Result<(Vec<i32>, usize), VariationError> {
    let mut deltas = Vec::with_capacity(count);
    let mut at = 0usize;
    while deltas.len() < count {
        let control = *data.get(at).ok_or(VariationError::DeltasPastEnd)?;
        at += 1;
        let run = usize::from(control & 0x3F) + 1;
        if deltas.len() + run > count {
            return Err(VariationError::DeltaRunsOverrun);
        }
        if control & 0x80 != 0 {
            deltas.resize(deltas.len() + run, 0);
        } else if control & 0x40 != 0 {
            for _ in 0..run {
                let value = be16(data, at).ok_or(VariationError::DeltasPastEnd)?;
                at += 2;
                deltas.push(i32::from(value as i16));
            }
        } else {
            for _ in 0..run {
                let value = *data.get(at).ok_or(VariationError::DeltasPastEnd)?;
                at += 1;
                deltas.push(i32::from(value as i8));
            }
        }
    }
    Ok((deltas, at))
}

/// One tuple variation table: its region, the items it moves, and its
/// packed deltas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TupleVariation<'a> {
    peak: Vec<i16>,
    intermediate: Option<(Vec<i16>, Vec<i16>)>,
    points: PointNumbers,
    /// The table's serialized data after its private point numbers.
    deltas: &'a [u8],
}

impl TupleVariation<'_> {
    /// The peak, one 2.14 coordinate an axis.
    #[must_use]
    pub fn peak(&self) -> &[i16] {
        &self.peak
    }

    /// An intermediate region's start and end.
    #[must_use]
    pub fn intermediate(&self) -> Option<(&[i16], &[i16])> {
        self.intermediate
            .as_ref()
            .map(|(start, end)| (start.as_slice(), end.as_slice()))
    }

    /// The region, an axis at a time: the intermediate start and end where
    /// the table embeds them, else the ones the peak implies.
    #[must_use]
    pub fn region(&self) -> Vec<RegionAxis> {
        match &self.intermediate {
            Some((start, end)) => self
                .peak
                .iter()
                .zip(start.iter().zip(end))
                .map(|(peak, (start, end))| RegionAxis {
                    start: *start,
                    peak: *peak,
                    end: *end,
                })
                .collect(),
            None => self
                .peak
                .iter()
                .map(|p| RegionAxis::from_peak(*p))
                .collect(),
        }
    }

    /// The table's scalar at a location.
    #[must_use]
    pub fn scalar(&self, location: &Location) -> Fixed {
        region_scalar(&self.region(), location)
    }

    /// The items the table moves.
    #[must_use]
    pub fn points(&self) -> &PointNumbers {
        &self.points
    }

    /// The table's deltas, `count` of them: one an item it moves in `cvar`,
    /// two in `gvar`, the X deltas then the Y deltas.
    ///
    /// # Errors
    ///
    /// [`unpack_deltas`]'s.
    pub fn deltas(&self, count: usize) -> Result<Vec<i32>, VariationError> {
        unpack_deltas(self.deltas, count).map(|(deltas, _)| deltas)
    }
}

/// A tuple variation store: a `GlyphVariationData` table, or `cvar`.
#[derive(Clone, Debug)]
pub struct TupleStore<'a> {
    table: &'a [u8],
    axis_count: u16,
    shared: SharedTuples<'a>,
    count: u16,
    shared_points: bool,
    headers_at: usize,
    data_at: usize,
}

impl<'a> TupleStore<'a> {
    /// The store whose `tupleVariationCount` is at `header_at` in `table`,
    /// its `dataOffset` measured from the start of `table` — `cvar`'s header
    /// follows its version, a `GlyphVariationData` table starts with it.
    ///
    /// # Errors
    ///
    /// [`VariationError::TupleHeadersPastEnd`] when the header is cut short
    /// or its data offset points past the table.
    pub fn parse(
        table: &'a [u8],
        header_at: usize,
        axis_count: u16,
        shared: SharedTuples<'a>,
    ) -> Result<TupleStore<'a>, VariationError> {
        let past = VariationError::TupleHeadersPastEnd;
        let packed = be16(table, header_at).ok_or(past)?;
        let data_at = usize::from(be16(table, header_at + 2).ok_or(past)?);
        if data_at > table.len() {
            return Err(past);
        }
        Ok(TupleStore {
            table,
            axis_count,
            shared,
            count: packed & COUNT_MASK,
            shared_points: packed & SHARED_POINT_NUMBERS != 0,
            headers_at: header_at + 4,
            data_at,
        })
    }

    /// How many tuple variation tables the store holds.
    #[must_use]
    pub fn len(&self) -> usize {
        usize::from(self.count)
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Whether the store's serialized data starts with shared point numbers.
    #[must_use]
    pub fn has_shared_points(&self) -> bool {
        self.shared_points
    }

    /// Every tuple variation table, its region read and its point numbers
    /// resolved, private or shared.
    ///
    /// # Errors
    ///
    /// A header past the end, a shared tuple index past the shared tuples, a
    /// table's data past the store, point numbers that do not decode, or a
    /// table with neither private nor shared point numbers — each by name.
    pub fn tuples(&self) -> Result<Vec<TupleVariation<'a>>, VariationError> {
        let axes = usize::from(self.axis_count);
        let tuple = |at: usize| -> Option<Vec<i16>> {
            (0..axes)
                .map(|axis| be16(self.table, at + axis * 2).map(|v| v as i16))
                .collect()
        };
        let mut data = self.data_at;
        let shared_points = if self.shared_points {
            let rest = self
                .table
                .get(data..)
                .ok_or(VariationError::PointsPastEnd)?;
            let (points, used) = unpack_points(rest)?;
            data += used;
            Some(points)
        } else {
            None
        };

        let mut tuples = Vec::with_capacity(self.len());
        let mut at = self.headers_at;
        for index in 0..self.count {
            let past = VariationError::TupleHeadersPastEnd;
            let size = usize::from(be16(self.table, at).ok_or(past)?);
            let flags = be16(self.table, at + 2).ok_or(past)?;
            at += 4;
            let peak = if flags & EMBEDDED_PEAK_TUPLE != 0 {
                let peak = tuple(at).ok_or(past)?;
                at += axes * 2;
                peak
            } else {
                let shared = flags & TUPLE_INDEX_MASK;
                self.shared
                    .get(shared)
                    .ok_or_else(|| VariationError::SharedTupleIndex {
                        index: shared,
                        count: self.shared.len(),
                    })?
            };
            let intermediate = if flags & INTERMEDIATE_REGION != 0 {
                let start = tuple(at).ok_or(past)?;
                let end = tuple(at + axes * 2).ok_or(past)?;
                at += axes * 4;
                Some((start, end))
            } else {
                None
            };
            let serialized = data
                .checked_add(size)
                .and_then(|end| self.table.get(data..end))
                .ok_or(VariationError::TupleDataPastEnd { tuple: index })?;
            data += size;
            let (points, deltas) = if flags & PRIVATE_POINT_NUMBERS != 0 {
                let (points, used) = unpack_points(serialized)?;
                (points, serialized.get(used..).unwrap_or(&[]))
            } else {
                let points = shared_points
                    .clone()
                    .ok_or(VariationError::NoPointNumbers { tuple: index })?;
                (points, serialized)
            };
            tuples.push(TupleVariation {
                peak,
                intermediate,
                points,
                deltas,
            });
        }
        Ok(tuples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chapters_point_counts_and_runs_decode() {
        // A count of 0x32 is fifty; 0x81 0x22 is 290.
        let mut fifty = vec![0x32, 0x31];
        fifty.extend(std::iter::repeat_n(1u8, 50));
        let (points, used) = unpack_points(&fifty).expect("fifty");
        assert_eq!(points, PointNumbers::Listed((1..=50).collect()));
        assert_eq!(used, fifty.len());

        let mut wide = vec![0x81, 0x22];
        for _ in 0..2 {
            // Two runs of 128 words, then one of 34 bytes: 290 in all.
            wide.push(0xFF);
            for _ in 0..128 {
                wide.extend_from_slice(&1u16.to_be_bytes());
            }
        }
        wide.push(0x21);
        wide.extend(std::iter::repeat_n(1u8, 34));
        let (points, used) = unpack_points(&wide).expect("290");
        assert_eq!(points, PointNumbers::Listed((1..=290).collect()));
        assert_eq!(used, wide.len());

        // A control byte of 0x02 is three bytes; 0xD4 is 85 words.
        let (points, _) = unpack_points(&[3, 0x02, 4, 1, 2]).expect("three");
        assert_eq!(points, PointNumbers::Listed(vec![4, 5, 7]));
        assert_eq!(unpack_points(&[0]).expect("all"), (PointNumbers::All, 1));
    }

    #[test]
    fn the_chapters_packed_delta_example_decodes() {
        let packed = [
            0x03, 0x0A, 0x97, 0x00, 0xC6, 0x87, 0x41, 0x10, 0x22, 0xFB, 0x34,
        ];
        let (deltas, used) = unpack_deltas(&packed, 14).expect("fourteen");
        assert_eq!(
            deltas,
            vec![10, -105, 0, -58, 0, 0, 0, 0, 0, 0, 0, 0, 4_130, -1_228]
        );
        assert_eq!(used, packed.len());
        // Asked for fewer than its first run holds, the stream is refused.
        assert_eq!(
            unpack_deltas(&packed, 2),
            Err(VariationError::DeltaRunsOverrun)
        );
        assert_eq!(
            unpack_deltas(&packed, 15),
            Err(VariationError::DeltasPastEnd)
        );
        assert_eq!(unpack_deltas(&[], 0), Ok((Vec::new(), 0)));
    }

    #[test]
    fn point_numbers_past_their_count_or_their_bytes_or_the_word_range_are_refused() {
        assert_eq!(unpack_points(&[]), Err(VariationError::PointsPastEnd));
        assert_eq!(unpack_points(&[0x81]), Err(VariationError::PointsPastEnd));
        assert_eq!(
            unpack_points(&[2, 0x02, 1, 1, 1]),
            Err(VariationError::PointRunsOverrun)
        );
        assert_eq!(
            unpack_points(&[2, 0x01, 1]),
            Err(VariationError::PointsPastEnd)
        );
        assert_eq!(
            unpack_points(&[2, 0x81, 0xFF, 0xFF, 0x00, 0x01]),
            Err(VariationError::PointNumberOverflow)
        );
        // A two-byte count of zero lists no points, which is not "every point".
        assert_eq!(
            unpack_points(&[0x80, 0x00]),
            Ok((PointNumbers::Listed(Vec::new()), 2))
        );
    }
}
