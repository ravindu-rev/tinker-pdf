//! The extended kerning table, `kerx`: a face's kerning as AAT carries it.
//!
//! Apple's *TrueType Reference Manual*, *The 'kerx' table*
//! (`docs/design/shaping.md`, *AAT shaping*, *`kerx`*).
//!
//! # The table
//!
//! "The 'kerx' Table": a `uint16` version, "currently 2, 3, or 4", an unused
//! word and `nTables`, then the subtables, each a `length` ("including this
//! header"), a `coverage` and a `tupleCount` before its body. Version 3's
//! glyph coverage arrays after the last subtable are an optimisation by which
//! a subtable "may be skipped", and are not read. The coverage:
//!
//! | Mask | Name | Read as |
//! | --- | --- | --- |
//! | `0x80000000` | `vertical` | kerning for vertical text: run in no line, refused by name in an upright run (FT-07b's) |
//! | `0x40000000` | `crossStream` | the values move glyphs across the line: "If adjustment values are positive, the text will be moved up" |
//! | `0x20000000` | `variation` | "Set if table has variation kerning values", which `tupleCount` says how to read |
//! | `0x10000000` | `processDirection` | formats 1 and 4: "If clear, process the glyphs forwards ... If [set], process them from last to first" |
//! | `0x000000FF` | `formatMask` | the format: 0, 1, 2, 4 and 6 are defined |
//!
//! A subtable whose `tupleCount` is above 0 in a version-4 table holds
//! "tupleCount-dimensional vectors of kerning values" for a variable face, and
//! is refused by name ([`Warning::VariationKerningNotRun`]); "The subtable's
//! tupleCount will be ignored if the 'kerx' table version is less than 4".
//! "Since the adjustments made by these tables are additive", the subtables
//! run in order, over the run in layout order as `morx`'s do: a right-to-left
//! run is reversed for them and back after.
//!
//! # Values, and the three pair formats
//!
//! A value is in font units: "If this value is greater than zero, the glyphs
//! are moved apart", so it is added to the advance of its pair's left glyph;
//! in a cross-stream subtable it is added to the right glyph's `y_offset`,
//! that glyph alone, this engine's reading where the chapter does not say
//! whether a shift carries to the glyphs after it.
//!
//! - **Format 0**, "an ordered list of kerning pairs": `nPairs` and three
//!   search fields, then the pairs, each a left glyph, a right glyph and an
//!   `sint16` value, ordered by "an unsigned 32-bit number" the two glyphs
//!   make, and bisected by it; the sentinel the table may end with names
//!   glyph `0xFFFF`, which no run holds.
//! - **Format 2**, "a simple two-dimensional, byte offset-based array":
//!   `rowWidth` and the offsets "from beginning of this subtable" of the
//!   left-hand and right-hand class tables, lookup tables whose "Values are
//!   16-bit unsigned offsets and default to zero for glyphs not covered", and
//!   of the kerning array. The class values are the cell's byte offsets in
//!   the array, a row's pre-multiplied by its width, and "The array can be
//!   indexed by completing the left-hand and right-hand class mappings,
//!   adding the class values to the address of the kerning array, and
//!   fetching the kerning value to which the new address points" — row and
//!   column 0, where an uncovered glyph's zero lands, holding no kerning.
//! - **Format 6**, "a simple, index-based two-dimensional array": `flags`,
//!   whose bit 0 `valuesAreLong` makes the class tables' values and the
//!   array's four bytes rather than two, `rowCount`, `columnCount` and the
//!   offsets of the row and column index tables and the kerning array. The
//!   row index is "pre-multiplied by columnCount", and the two indices
//!   summed are "the index of the kerning value for the pair", within the
//!   `rowCount × columnCount` cells.
//!
//! A cell a subtable's values point past is named as malformed and kerns
//! nothing. Formats 1 and 4 are state machines this crate does not run yet,
//! each refused by name (ruling 3), as a format the chapter does not define
//! is ([`Warning::SubtableTypeNotRun`]).
//!
//! # Bounds
//!
//! The subtables of one table read and run are held to
//! [`Limits::max_subtables`] ([`Warning::SubtableBudgetExceeded`]); a
//! subtable shorter than its header or running past the table ends the walk
//! ([`Warning::MalformedAatSubtable`]), its length being the only way to the
//! next. A pair format costs one lookup a pair of the run, and every read is
//! `read::Bytes`', so a truncated table or an offset past its end answers
//! nothing and never panics (ruling 1).

use super::lookup::{Lookup, LookupError};
use super::Site;
use crate::buffer::{Buffer, Direction};
use crate::common::Tag;
use crate::limits::Limits;
use crate::read::Bytes;
use crate::Warning;

/// `kerx`, as the face's table directory spells it.
pub const KERX: Tag = Tag::new(b"kerx");

/// The coverage bit of a subtable of vertical kerning values.
pub const KERX_VERTICAL: u32 = 0x8000_0000;
/// The coverage bit of a subtable of cross-stream values.
pub const CROSS_STREAM: u32 = 0x4000_0000;
/// The coverage bit of a subtable of variation kerning values.
pub const KERX_VARIATION: u32 = 0x2000_0000;
/// The coverage bit of a state-machine subtable processed last to first.
pub const PROCESS_DIRECTION: u32 = 0x1000_0000;
/// The coverage byte holding the subtable's format.
const FORMAT: u32 = 0x0000_00FF;

/// The bytes of the table's header.
const TABLE_HEADER: usize = 8;
/// The bytes of a subtable's header.
const SUBTABLE_HEADER: usize = 12;
/// The bytes of format 0's header after the subtable's: `nPairs` and the
/// three search fields.
const PAIRS_HEADER: usize = 16;
/// The bytes of one format 0 pair.
const PAIR: usize = 6;
/// Format 6's `valuesAreLong`.
const VALUES_ARE_LONG: u32 = 0x0000_0001;

/// A `kerx` table.
#[derive(Clone, Copy, Debug)]
pub struct Kerx<'a> {
    data: Bytes<'a>,
    version: u16,
    count: u32,
}

impl<'a> Kerx<'a> {
    /// Reads a `kerx` header: `None` for a version other than 2, 3 or 4, or a
    /// header that cannot be read.
    #[must_use]
    pub fn parse(bytes: &'a [u8]) -> Option<Kerx<'a>> {
        let data = Bytes::new(bytes);
        let version = data.u16(0)?;
        if !(2..=4).contains(&version) {
            return None;
        }
        let count = data.u32(4)?;
        Some(Kerx {
            data,
            version,
            count,
        })
    }

    /// The table's version, 2, 3 or 4.
    #[must_use]
    pub const fn version(&self) -> u16 {
        self.version
    }

    /// `nTables`, as the table states it.
    #[must_use]
    pub const fn subtable_count(&self) -> u32 {
        self.count
    }

    /// The subtables, in table order, read as far as the table holds them and
    /// `limits` allows, and what was refused on the way: past
    /// [`Limits::max_subtables`] nothing more is read
    /// ([`Warning::SubtableBudgetExceeded`]), and a subtable shorter than its
    /// header or running past the table ends the walk
    /// ([`Warning::MalformedAatSubtable`]).
    #[must_use]
    pub fn subtables(&self, limits: &Limits) -> (Vec<KerxSubtable<'a>>, Vec<Warning>) {
        let mut subtables: Vec<KerxSubtable<'a>> = Vec::new();
        let mut warnings: Vec<Warning> = Vec::new();
        let mut at = TABLE_HEADER;
        for index in 0..self.count {
            if index >= limits.max_subtables {
                warnings.push(Warning::SubtableBudgetExceeded { table: KERX });
                break;
            }
            let Some(subtable) = self.subtable_at(index, at) else {
                warnings.push(Warning::MalformedAatSubtable { site: site(index) });
                break;
            };
            at = match at.checked_add(subtable.bytes.len()) {
                Some(next) => next,
                None => break,
            };
            subtables.push(subtable);
        }
        (subtables, warnings)
    }

    /// The subtable whose header is at `at`: `None` where its header cannot
    /// be read, it is shorter than its header, or it runs past the table.
    fn subtable_at(&self, index: u32, at: usize) -> Option<KerxSubtable<'a>> {
        let header = self.data.at(at)?;
        let length = usize::try_from(header.u32(0)?).ok()?;
        let coverage = header.u32(4)?;
        let tuple_count = header.u32(8)?;
        if length < SUBTABLE_HEADER {
            return None;
        }
        Some(KerxSubtable {
            index,
            coverage,
            tuple_count,
            bytes: header.as_slice().get(..length)?,
        })
    }

    /// Kerns `buffer` by the table's subtables, as the module documentation
    /// says, and names what it refused, each warning once.
    ///
    /// The run's direction is the buffer's: a right-to-left run is reversed
    /// into layout order for the subtables and back after them, and a
    /// top-to-bottom one is vertical text, which no subtable runs over —
    /// a vertical one is refused by name there, and a horizontal one is not
    /// for it.
    pub fn run(&self, buffer: &mut Buffer, limits: &Limits) -> Vec<Warning> {
        let direction = buffer.direction();
        let rtl = direction == Direction::RightToLeft;
        let vertical = direction.is_vertical();
        let (subtables, mut warnings) = self.subtables(limits);
        if rtl {
            buffer.reverse();
        }
        for subtable in &subtables {
            let site = site(subtable.index);
            if subtable.coverage & KERX_VERTICAL != 0 {
                if vertical {
                    warnings.push(Warning::VerticalKerningNotRun { site });
                }
                continue;
            }
            if vertical {
                continue;
            }
            if self.version >= 4 && subtable.tuple_count > 0 {
                warnings.push(Warning::VariationKerningNotRun { site });
                continue;
            }
            let cross = subtable.coverage & CROSS_STREAM != 0;
            let outcome = match subtable.format() {
                0 => Pairs::parse(subtable.bytes)
                    .map(|table| kern_pairs(&table, buffer, cross, site, &mut warnings)),
                2 => ClassArray::parse(subtable.bytes)
                    .map(|table| kern_pairs(&table, buffer, cross, site, &mut warnings)),
                6 => IndexArray::parse(subtable.bytes)
                    .map(|table| kern_pairs(&table, buffer, cross, site, &mut warnings)),
                kind => Err(Refused::Format(kind)),
            };
            match outcome {
                Ok(()) => {}
                Err(refused) => warnings.push(refused.warning(site)),
            }
        }
        if rtl {
            buffer.reverse();
        }
        dedup(warnings)
    }
}

/// Where in `kerx` subtable `index` stands: chain 0, a table without chains.
const fn site(index: u32) -> Site {
    Site {
        table: KERX,
        chain: 0,
        subtable: index,
    }
}

/// Each warning once, in the order first recorded.
fn dedup(warnings: Vec<Warning>) -> Vec<Warning> {
    let mut out: Vec<Warning> = Vec::with_capacity(warnings.len());
    for warning in warnings {
        if !out.contains(&warning) {
            out.push(warning);
        }
    }
    out
}

/// One subtable of a `kerx` table.
#[derive(Clone, Copy, Debug)]
pub struct KerxSubtable<'a> {
    index: u32,
    /// `coverage`: its orientation, cross-stream, variation and direction
    /// bits, and its format.
    pub coverage: u32,
    /// `tupleCount`.
    pub tuple_count: u32,
    /// The whole subtable, its header included: the offsets of formats 2 and
    /// 6 are "from beginning of this subtable".
    bytes: &'a [u8],
}

impl<'a> KerxSubtable<'a> {
    /// Which subtable of its table this is, counting from zero.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// Its format, the low byte of its coverage.
    #[must_use]
    pub const fn format(&self) -> u8 {
        // The mask keeps the low byte, which fits.
        #[allow(clippy::cast_possible_truncation)]
        let format = (self.coverage & FORMAT) as u8;
        format
    }

    /// The whole subtable, its header included.
    #[must_use]
    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

/// Why a subtable did not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refused {
    /// Its header or tables cannot be read.
    Malformed,
    /// A lookup table of a format the manual does not define.
    Lookup(u16),
    /// A format this crate does not run.
    Format(u8),
}

impl Refused {
    /// The warning a subtable at `site` records.
    const fn warning(self, site: Site) -> Warning {
        match self {
            Refused::Malformed => Warning::MalformedAatSubtable { site },
            Refused::Lookup(format) => Warning::UnknownLookupFormat { site, format },
            Refused::Format(kind) => Warning::SubtableTypeNotRun { site, kind },
        }
    }
}

impl From<LookupError> for Refused {
    fn from(error: LookupError) -> Refused {
        match error {
            LookupError::Format(format) => Refused::Lookup(format),
            LookupError::Malformed => Refused::Malformed,
        }
    }
}

/// What a pair format's table says of one pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    /// No kerning: the table does not hold the pair.
    Absent,
    /// The value, in font units.
    Value(i32),
    /// The table's values point past it.
    Malformed,
}

/// A pair format: the cell for a left glyph and a right one.
trait PairTable {
    fn cell(&self, left: u16, right: u16) -> Cell;
}

/// Kerns each pair of neighbours of `buffer`, in layout order, by `table`: the
/// left glyph's advance, or the right glyph's `y_offset` across the line.
fn kern_pairs<T: PairTable>(
    table: &T,
    buffer: &mut Buffer,
    cross: bool,
    site: Site,
    warnings: &mut Vec<Warning>,
) {
    for at in 1..buffer.len() {
        let (Some(left), Some(right)) = (buffer.glyph_id(at - 1), buffer.glyph_id(at)) else {
            continue;
        };
        match table.cell(left, right) {
            Cell::Absent | Cell::Value(0) => {}
            Cell::Value(value) => {
                let (moved, field) = if cross { (at, true) } else { (at - 1, false) };
                if let Some(glyph) = buffer.glyph_mut(moved) {
                    if field {
                        glyph.y_offset = glyph.y_offset.saturating_add(value);
                    } else {
                        glyph.x_advance = glyph.x_advance.saturating_add(value);
                    }
                }
            }
            Cell::Malformed => warnings.push(Warning::MalformedAatSubtable { site }),
        }
    }
}

/// A format 0 subtable's ordered pairs.
struct Pairs<'a> {
    pairs: Bytes<'a>,
    count: usize,
}

impl<'a> Pairs<'a> {
    /// Reads the pairs after the subtable's header, as many as `nPairs`
    /// says and the subtable holds.
    fn parse(subtable: &'a [u8]) -> Result<Pairs<'a>, Refused> {
        let body = Bytes::new(subtable)
            .at(SUBTABLE_HEADER)
            .ok_or(Refused::Malformed)?;
        let claimed = usize::try_from(body.u32(0).ok_or(Refused::Malformed)?)
            .map_err(|_| Refused::Malformed)?;
        let pairs = body.at(PAIRS_HEADER).ok_or(Refused::Malformed)?;
        let room = pairs.len() / PAIR;
        Ok(Pairs {
            pairs,
            count: claimed.min(room),
        })
    }

    /// The 32-bit key of pair `n`: its left glyph above its right.
    fn key(&self, n: usize) -> Option<u32> {
        let at = n.checked_mul(PAIR)?;
        let left = u32::from(self.pairs.u16(at)?);
        let right = u32::from(self.pairs.u16(at.checked_add(2)?)?);
        Some((left << 16) | right)
    }
}

impl PairTable for Pairs<'_> {
    fn cell(&self, left: u16, right: u16) -> Cell {
        let wanted = (u32::from(left) << 16) | u32::from(right);
        let (mut low, mut high) = (0usize, self.count);
        while low < high {
            let middle = low + (high - low) / 2;
            match self.key(middle) {
                Some(key) if key < wanted => low = middle + 1,
                Some(_) => high = middle,
                None => return Cell::Malformed,
            }
        }
        if low >= self.count || self.key(low) != Some(wanted) {
            return Cell::Absent;
        }
        let value = low
            .checked_mul(PAIR)
            .and_then(|at| at.checked_add(4))
            .and_then(|at| self.pairs.i16(at));
        value.map_or(Cell::Malformed, |value| Cell::Value(i32::from(value)))
    }
}

/// A format 2 subtable's class tables and byte-offset kerning array.
struct ClassArray<'a> {
    subtable: Bytes<'a>,
    left: Lookup<'a>,
    right: Lookup<'a>,
    array: usize,
}

impl<'a> ClassArray<'a> {
    /// Reads the header after the subtable's: `rowWidth`, and the offsets of
    /// the two class tables and the array from the subtable's start.
    fn parse(subtable: &'a [u8]) -> Result<ClassArray<'a>, Refused> {
        let data = Bytes::new(subtable);
        let offset = |at: usize| -> Result<usize, Refused> {
            data.u32(at)
                .and_then(|offset| usize::try_from(offset).ok())
                .ok_or(Refused::Malformed)
        };
        let lookup = |at: usize| -> Result<Lookup<'a>, Refused> {
            let table = data.at(offset(at)?).ok_or(Refused::Malformed)?;
            Ok(Lookup::parse(table.as_slice())?)
        };
        // `rowWidth`, at 12, is read for the header's sake: the class values
        // are byte offsets, so the cell needs no multiplying.
        offset(SUBTABLE_HEADER)?;
        Ok(ClassArray {
            subtable: data,
            left: lookup(SUBTABLE_HEADER + 4)?,
            right: lookup(SUBTABLE_HEADER + 8)?,
            array: offset(SUBTABLE_HEADER + 12)?,
        })
    }
}

impl PairTable for ClassArray<'_> {
    fn cell(&self, left: u16, right: u16) -> Cell {
        let row = usize::from(self.left.value16(left).unwrap_or(0));
        let column = usize::from(self.right.value16(right).unwrap_or(0));
        let at = self
            .array
            .checked_add(row)
            .and_then(|at| at.checked_add(column));
        at.and_then(|at| self.subtable.i16(at))
            .map_or(Cell::Malformed, |value| Cell::Value(i32::from(value)))
    }
}

/// A format 6 subtable's index tables and kerning array.
struct IndexArray<'a> {
    rows: Lookup<'a>,
    columns: Lookup<'a>,
    array: Bytes<'a>,
    cells: u64,
    long: bool,
}

impl<'a> IndexArray<'a> {
    /// Reads the header after the subtable's: `flags`, `rowCount`,
    /// `columnCount`, and the offsets of the two index tables and the array
    /// from the subtable's start.
    fn parse(subtable: &'a [u8]) -> Result<IndexArray<'a>, Refused> {
        let data = Bytes::new(subtable);
        let flags = data.u32(SUBTABLE_HEADER).ok_or(Refused::Malformed)?;
        let rows = data.u16(SUBTABLE_HEADER + 4).ok_or(Refused::Malformed)?;
        let columns = data.u16(SUBTABLE_HEADER + 6).ok_or(Refused::Malformed)?;
        let long = flags & VALUES_ARE_LONG != 0;
        let size = if long { 4 } else { 2 };
        let section = |at: usize| -> Result<Bytes<'a>, Refused> {
            data.u32(at)
                .and_then(|offset| usize::try_from(offset).ok())
                .and_then(|offset| data.at(offset))
                .ok_or(Refused::Malformed)
        };
        let lookup = |at: usize| -> Result<Lookup<'a>, Refused> {
            Ok(Lookup::parse_sized(section(at)?.as_slice(), size)?)
        };
        Ok(IndexArray {
            rows: lookup(SUBTABLE_HEADER + 8)?,
            columns: lookup(SUBTABLE_HEADER + 12)?,
            array: section(SUBTABLE_HEADER + 16)?,
            cells: u64::from(rows) * u64::from(columns),
            long,
        })
    }
}

impl PairTable for IndexArray<'_> {
    fn cell(&self, left: u16, right: u16) -> Cell {
        let row = self.rows.value(left).unwrap_or(0);
        let column = self.columns.value(right).unwrap_or(0);
        let Some(index) = row.checked_add(column).filter(|index| *index < self.cells) else {
            return Cell::Malformed;
        };
        let Ok(index) = usize::try_from(index) else {
            return Cell::Malformed;
        };
        let value = if self.long {
            index
                .checked_mul(4)
                .and_then(|at| self.array.u32(at))
                .map(|value| i32::from_be_bytes(value.to_be_bytes()))
        } else {
            index
                .checked_mul(2)
                .and_then(|at| self.array.i16(at))
                .map(i32::from)
        };
        value.map_or(Cell::Malformed, Cell::Value)
    }
}

#[cfg(test)]
mod tests {
    use super::{Cell, ClassArray, IndexArray, PairTable, Pairs, Refused};

    fn u16s(values: &[u16]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    fn u32s(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// A subtable header of `format` before `body`, its length the whole.
    fn subtable(format: u32, body: &[u8]) -> Vec<u8> {
        let length = u32::try_from(12 + body.len()).expect("small");
        let mut out = u32s(&[length, format, 0]);
        out.extend_from_slice(body);
        out
    }

    /// Format 0 bisects its ordered pairs by the key the two glyphs make,
    /// a pair it does not hold answering nothing, and reads no more pairs
    /// than the subtable holds whatever `nPairs` claims.
    #[test]
    fn format_0_bisects_its_pairs() {
        let mut body = u32s(&[400, 12, 1, 6]);
        for (left, right, value) in [(1u16, 2u16, -10i16), (1, 5, 20), (3, 1, -30), (9, 9, 40)] {
            body.extend(u16s(&[left, right]));
            body.extend(value.to_be_bytes());
        }
        let bytes = subtable(0, &body);
        let table = Pairs::parse(&bytes).expect("readable");
        assert_eq!(table.count, 4, "the four pairs held, not the 400 claimed");
        assert_eq!(table.cell(1, 2), Cell::Value(-10));
        assert_eq!(table.cell(1, 5), Cell::Value(20));
        assert_eq!(table.cell(3, 1), Cell::Value(-30));
        assert_eq!(table.cell(9, 9), Cell::Value(40));
        for (left, right) in [(1u16, 3u16), (2, 1), (0, 0), (9, 10), (0xFFFF, 0xFFFF)] {
            assert_eq!(table.cell(left, right), Cell::Absent, "{left} {right}");
        }
        assert_eq!(
            Pairs::parse(&subtable(0, &[0, 0])).err(),
            Some(Refused::Malformed)
        );
    }

    /// Format 2 adds its two class values to the kerning array's address,
    /// a glyph neither class table covers taking 0, and a cell past the
    /// subtable is malformed.
    #[test]
    fn format_2_adds_its_class_offsets_to_the_array() {
        // Header after the subtable's: rowWidth 4, the left table at 28, the
        // right one at 44, the array at 60; offsets from the subtable start.
        let mut body = u32s(&[4, 28, 44, 60]);
        // The left table: format 8, glyph 5 in row 1 (offset 4).
        body.extend(u16s(&[8, 5, 1, 4]));
        body.extend([0u8; 8]);
        // The right table: format 8, glyph 6 in column 1 (offset 2), glyph 7
        // far past the array.
        body.extend(u16s(&[8, 6, 2, 2, 400]));
        body.extend([0u8; 6]);
        // The array: two rows of two, row 0 and column 0 zero.
        body.extend(u16s(&[0, 0, 0]));
        body.extend((-55i16).to_be_bytes());
        let bytes = subtable(2, &body);
        assert_eq!(bytes.len(), 68);
        let table = ClassArray::parse(&bytes).expect("readable");
        assert_eq!(table.cell(5, 6), Cell::Value(-55));
        assert_eq!(table.cell(5, 9), Cell::Value(0), "column 0");
        assert_eq!(table.cell(9, 6), Cell::Value(0), "row 0");
        assert_eq!(table.cell(5, 7), Cell::Malformed);
    }

    /// Format 6 sums a row index pre-multiplied by `columnCount` and a column
    /// index into the array, two-byte values or four where `valuesAreLong`
    /// is set, and an index past `rowCount × columnCount` is malformed.
    #[test]
    fn format_6_indexes_its_array_by_row_and_column() {
        for long in [false, true] {
            let size = if long { 4usize } else { 2 };
            let value = |v: i32| -> Vec<u8> {
                if long {
                    v.to_be_bytes().to_vec()
                } else {
                    i16::try_from(v).expect("small").to_be_bytes().to_vec()
                }
            };
            // The tables, each a format 8 lookup of `size`-byte values: rows
            // for glyph 3 (row 1, pre-multiplied: 2), columns for glyphs 4
            // and 5 (columns 1 and 2, the second past the two columns).
            let mut rows = u16s(&[8, 3, 1]);
            rows.extend(value(2));
            let mut columns = u16s(&[8, 4, 2]);
            columns.extend(value(1));
            columns.extend(value(5));
            let array: Vec<u8> = [0, 0, 0, -70].iter().flat_map(|v| value(*v)).collect();
            let header = 12 + 20;
            let rows_at = header;
            let columns_at = rows_at + rows.len();
            let array_at = columns_at + columns.len();
            let mut body = u32s(&[u32::from(long)]);
            body.extend(u16s(&[2, 2]));
            body.extend(u32s(&[
                u32::try_from(rows_at).expect("small"),
                u32::try_from(columns_at).expect("small"),
                u32::try_from(array_at).expect("small"),
            ]));
            body.extend(rows);
            body.extend(columns);
            body.extend(array);
            let bytes = subtable(6, &body);
            let table = IndexArray::parse(&bytes).expect("readable");
            assert_eq!(table.cell(3, 4), Cell::Value(-70), "size {size}");
            assert_eq!(table.cell(3, 9), Cell::Value(0), "size {size}");
            assert_eq!(table.cell(9, 4), Cell::Value(0), "size {size}");
            assert_eq!(table.cell(3, 5), Cell::Malformed, "size {size}");
        }
    }
}
