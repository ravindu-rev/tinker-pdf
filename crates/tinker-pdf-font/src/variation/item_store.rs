//! The item variation store and its delta-set index maps (OpenType 1.9.1,
//! *OpenType Font Variations Common Table Formats*, 'Item Variation Store'
//! and 'Associating Target Items to Variation Data').
//!
//! A store is a region list — each region an axis's start, peak and end —
//! and item variation data subtables, each naming some of the regions and
//! holding a row of deltas an item, one a region it names. An item's delta
//! set is named by an outer index, the subtable, and an inner one, the row;
//! the index 0xFFFF/0xFFFF is no variation, as is an outer index whose offset
//! is NULL. `HVAR`, `VVAR` and `MVAR` reach their rows this way, `CFF2` reads
//! only each subtable's region indices, and `GDEF`'s variation data is the
//! same structure — so the shaper reads it through this module, under the
//! design's numeric model, with no float in it.
//!
//! **Bounds** (ruling 1): fewer than 32 768 regions, as the region list's
//! own rule says; an axis count of at most [`super::MAX_AXES`]; every
//! subtable, row and map inside the bytes it is read from; and every index
//! inside what it indexes — each refused by name past its bound.

use super::{be16, be32, region_scalar, Fixed, Location, RegionAxis, VariationError, MAX_AXES};

/// A delta-set index: the item variation data subtable, and the row in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeltaSetIndex {
    /// The subtable.
    pub outer: u16,
    /// The row.
    pub inner: u16,
}

impl DeltaSetIndex {
    /// The index that names no variation: 0xFFFF/0xFFFF.
    pub const NO_VARIATION: DeltaSetIndex = DeltaSetIndex {
        outer: 0xFFFF,
        inner: 0xFFFF,
    };

    /// An index.
    #[must_use]
    pub const fn new(outer: u16, inner: u16) -> DeltaSetIndex {
        DeltaSetIndex { outer, inner }
    }
}

/// A region count must be less than this (common formats, 'Variation
/// Regions': "Must be less than 32,768").
pub const REGION_LIMIT: u16 = 0x8000;

/// An item variation store.
#[derive(Clone, Debug)]
pub struct ItemVariationStore<'a> {
    data: &'a [u8],
    axis_count: u16,
    region_count: u16,
    /// Where the first `VariationRegion` record starts.
    regions_at: usize,
    data_offsets: Vec<u32>,
}

impl<'a> ItemVariationStore<'a> {
    /// Parses a store from its first byte.
    ///
    /// # Errors
    ///
    /// A format other than 1, a header, offset array or region list past the
    /// end, 32 768 regions or more, or more than [`MAX_AXES`] axes — by name.
    pub fn parse(data: &'a [u8]) -> Result<ItemVariationStore<'a>, VariationError> {
        let past = VariationError::StorePastEnd;
        let format = be16(data, 0).ok_or(past)?;
        if format != 1 {
            return Err(VariationError::StoreFormat { format });
        }
        let region_list_at = be32(data, 2).ok_or(past)? as usize;
        let data_count = be16(data, 6).ok_or(past)?;
        let mut data_offsets = Vec::with_capacity(usize::from(data_count));
        for index in 0..usize::from(data_count) {
            data_offsets.push(be32(data, 8 + index * 4).ok_or(past)?);
        }
        let axis_count = be16(data, region_list_at).ok_or(VariationError::RegionsPastEnd)?;
        let region_count = be16(data, region_list_at + 2).ok_or(VariationError::RegionsPastEnd)?;
        if region_count >= REGION_LIMIT {
            return Err(VariationError::RegionCount {
                count: region_count,
            });
        }
        if usize::from(axis_count) > MAX_AXES {
            return Err(VariationError::TooManyAxes { count: axis_count });
        }
        let regions_at = region_list_at + 4;
        let bytes = usize::from(region_count) * usize::from(axis_count) * 6;
        if regions_at.saturating_add(bytes) > data.len() {
            return Err(VariationError::RegionsPastEnd);
        }
        Ok(ItemVariationStore {
            data,
            axis_count,
            region_count,
            regions_at,
            data_offsets,
        })
    }

    /// The region list's axis count, which must be `fvar`'s.
    #[must_use]
    pub fn axis_count(&self) -> u16 {
        self.axis_count
    }

    /// How many regions the list holds.
    #[must_use]
    pub fn region_count(&self) -> u16 {
        self.region_count
    }

    /// One region, an axis at a time.
    ///
    /// # Errors
    ///
    /// [`VariationError::RegionIndex`] past the list.
    pub fn region(&self, index: u16) -> Result<Vec<RegionAxis>, VariationError> {
        if index >= self.region_count {
            return Err(VariationError::RegionIndex {
                index,
                count: self.region_count,
            });
        }
        let axes = usize::from(self.axis_count);
        let at = self.regions_at + usize::from(index) * axes * 6;
        // The list was measured against the bytes when the store was parsed.
        Ok((0..axes)
            .map(|axis| {
                let record = at + axis * 6;
                let read = |offset: usize| be16(self.data, record + offset).unwrap_or(0) as i16;
                RegionAxis {
                    start: read(0),
                    peak: read(2),
                    end: read(4),
                }
            })
            .collect())
    }

    /// Every region's scalar at a location, in the list's order — the cache
    /// the chapter suggests, computed once for an instance and read by every
    /// item.
    #[must_use]
    pub fn region_scalars(&self, location: &Location) -> Vec<Fixed> {
        (0..self.region_count)
            .map(|index| {
                self.region(index)
                    .map_or(Fixed::ZERO, |axes| region_scalar(&axes, location))
            })
            .collect()
    }

    /// How many item variation data subtables the store holds.
    #[must_use]
    pub fn data_count(&self) -> u16 {
        // `parse` read the count from sixteen bits.
        u16::try_from(self.data_offsets.len()).unwrap_or(u16::MAX)
    }

    /// One item variation data subtable; `None` where its offset is NULL,
    /// which is no variation for anything it would have held.
    ///
    /// # Errors
    ///
    /// An outer index past the subtables, a subtable past the store, a
    /// `wordDeltaCount` above its `regionIndexCount`, or a region index past
    /// the region list — by name.
    pub fn data(&self, outer: u16) -> Result<Option<ItemVariationData<'a>>, VariationError> {
        let offset = *self
            .data_offsets
            .get(usize::from(outer))
            .ok_or(VariationError::DeltaSetIndex { outer, inner: 0 })?;
        if offset == 0 {
            return Ok(None);
        }
        let at = offset as usize;
        let past = VariationError::DataPastEnd { outer };
        let item_count = be16(self.data, at).ok_or(past)?;
        let words = be16(self.data, at + 2).ok_or(past)?;
        let region_index_count = be16(self.data, at + 4).ok_or(past)?;
        let long_words = words & 0x8000 != 0;
        let word_count = words & 0x7FFF;
        if word_count > region_index_count {
            return Err(VariationError::WordDeltaCount {
                outer,
                words: word_count,
                regions: region_index_count,
            });
        }
        let mut region_indexes = Vec::with_capacity(usize::from(region_index_count));
        for column in 0..usize::from(region_index_count) {
            let index = be16(self.data, at + 6 + column * 2).ok_or(past)?;
            if index >= self.region_count {
                return Err(VariationError::RegionIndex {
                    index,
                    count: self.region_count,
                });
            }
            region_indexes.push(index);
        }
        let rows_at = at + 6 + usize::from(region_index_count) * 2;
        let unit = if long_words { 2 } else { 1 };
        let row_size = (usize::from(region_index_count) + usize::from(word_count)) * unit;
        let rows_end = rows_at.saturating_add(usize::from(item_count) * row_size);
        let rows = self.data.get(rows_at..rows_end).ok_or(past)?;
        Ok(Some(ItemVariationData {
            outer,
            item_count,
            word_count,
            long_words,
            region_indexes,
            row_size,
            rows,
        }))
    }

    /// An item's net adjustment at a location, in 16.16: each delta of its
    /// row times the scalar of the region its column names, summed exactly
    /// (the overview's 'Algorithm for Interpolation of Instance Values', the
    /// design's numeric model). `scalars` are [`Self::region_scalars`] at the
    /// location; a region past them counts as zero.
    ///
    /// # Errors
    ///
    /// [`Self::data`]'s, and an inner index past the subtable's items.
    pub fn delta(&self, index: DeltaSetIndex, scalars: &[Fixed]) -> Result<Fixed, VariationError> {
        if index == DeltaSetIndex::NO_VARIATION {
            return Ok(Fixed::ZERO);
        }
        let Some(data) = self.data(index.outer)? else {
            return Ok(Fixed::ZERO);
        };
        let deltas = data.deltas(index.inner)?;
        let mut sum = Fixed::ZERO;
        for (region, delta) in data.region_indexes.iter().zip(deltas) {
            let scalar = scalars
                .get(usize::from(*region))
                .copied()
                .unwrap_or(Fixed::ZERO);
            sum = sum.saturating_add(scalar.scaled(delta));
        }
        Ok(sum)
    }
}

/// One item variation data subtable: the regions it names and a row of
/// deltas an item.
#[derive(Clone, Debug)]
pub struct ItemVariationData<'a> {
    outer: u16,
    item_count: u16,
    word_count: u16,
    long_words: bool,
    region_indexes: Vec<u16>,
    row_size: usize,
    rows: &'a [u8],
}

impl ItemVariationData<'_> {
    /// How many rows the subtable holds.
    #[must_use]
    pub fn item_count(&self) -> u16 {
        self.item_count
    }

    /// The regions its columns name, in column order — `CFF2`'s active
    /// regions, whose count is a `blend`'s `k`.
    #[must_use]
    pub fn region_indexes(&self) -> &[u16] {
        &self.region_indexes
    }

    /// How many deltas of each row use the long type.
    #[must_use]
    pub fn word_count(&self) -> u16 {
        self.word_count
    }

    /// Whether the long type is 32-bit and the short one 16 (`LONG_WORDS`),
    /// rather than 16 and 8.
    #[must_use]
    pub fn long_words(&self) -> bool {
        self.long_words
    }

    /// One row's deltas, a column at a time: the long ones first, then the
    /// short ones.
    ///
    /// # Errors
    ///
    /// [`VariationError::DeltaSetIndex`] for a row past the subtable.
    pub fn deltas(&self, inner: u16) -> Result<Vec<i32>, VariationError> {
        if inner >= self.item_count {
            return Err(VariationError::DeltaSetIndex {
                outer: self.outer,
                inner,
            });
        }
        let at = usize::from(inner) * self.row_size;
        let mut deltas = Vec::with_capacity(self.region_indexes.len());
        let mut cursor = at;
        for column in 0..self.region_indexes.len() {
            let long = column < usize::from(self.word_count);
            // The rows were measured against the bytes when the subtable was
            // read.
            let value = match (self.long_words, long) {
                (true, true) => {
                    let value = be32(self.rows, cursor).unwrap_or(0) as i32;
                    cursor += 4;
                    value
                }
                (true, false) | (false, true) => {
                    let value = i32::from(be16(self.rows, cursor).unwrap_or(0) as i16);
                    cursor += 2;
                    value
                }
                (false, false) => {
                    let value = i32::from(self.rows.get(cursor).copied().unwrap_or(0) as i8);
                    cursor += 1;
                    value
                }
            };
            deltas.push(value);
        }
        Ok(deltas)
    }
}

/// A delta-set index map, formats 0 and 1: an item's index into a store.
#[derive(Clone, Debug)]
pub struct DeltaSetIndexMap<'a> {
    format: u8,
    entry_format: u8,
    count: u32,
    entries: &'a [u8],
}

impl<'a> DeltaSetIndexMap<'a> {
    /// Parses a map from its first byte.
    ///
    /// # Errors
    ///
    /// A format other than 0 and 1, a map with no entries, or entries past
    /// the end — by name.
    pub fn parse(data: &'a [u8]) -> Result<DeltaSetIndexMap<'a>, VariationError> {
        let past = VariationError::MapPastEnd;
        let format = *data.first().ok_or(past)?;
        let entry_format = *data.get(1).ok_or(past)?;
        let (count, at) = match format {
            0 => (u32::from(be16(data, 2).ok_or(past)?), 4),
            1 => (be32(data, 2).ok_or(past)?, 6),
            _ => return Err(VariationError::MapFormat { format }),
        };
        if count == 0 {
            return Err(VariationError::EmptyMap);
        }
        let size = usize::from((entry_format & 0x30) >> 4) + 1;
        let end = (count as usize)
            .checked_mul(size)
            .and_then(|bytes| bytes.checked_add(at))
            .ok_or(past)?;
        let entries = data.get(at..end).ok_or(past)?;
        Ok(DeltaSetIndexMap {
            format,
            entry_format,
            count,
            entries,
        })
    }

    /// The map's format.
    #[must_use]
    pub fn format(&self) -> u8 {
        self.format
    }

    /// How many entries it holds.
    #[must_use]
    pub fn len(&self) -> u32 {
        self.count
    }

    /// Whether it holds none, which [`Self::parse`] refuses, so never.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The delta-set index of an item; an item at or past the count takes the
    /// last entry.
    ///
    /// # Errors
    ///
    /// [`VariationError::MapEntryOuter`] for an entry whose outer index needs
    /// more than sixteen bits.
    pub fn get(&self, item: u32) -> Result<DeltaSetIndex, VariationError> {
        let index = item.min(self.count.saturating_sub(1)) as usize;
        let size = usize::from((self.entry_format & 0x30) >> 4) + 1;
        let at = index * size;
        let entry = self
            .entries
            .get(at..at + size)
            .ok_or(VariationError::MapPastEnd)?
            .iter()
            .fold(0u32, |entry, byte| (entry << 8) | u32::from(*byte));
        let inner_bits = u32::from(self.entry_format & 0x0F) + 1;
        let outer = entry >> inner_bits;
        let inner = entry & ((1u32 << inner_bits) - 1);
        Ok(DeltaSetIndex {
            outer: u16::try_from(outer).map_err(|_| VariationError::MapEntryOuter { outer })?,
            // At most sixteen bits by the mask.
            inner: u16::try_from(inner).unwrap_or(u16::MAX),
        })
    }
}
