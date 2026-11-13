//! The metric clients of the item variation store: `HVAR`, `VVAR` and `MVAR`
//! (OpenType 1.9.1, their chapters).
//!
//! Each holds an [`ItemVariationStore`] and says which delta set an item
//! takes: `HVAR` and `VVAR` through delta-set index maps — format 0 only, as
//! both chapters state — or, for an advance with no map, the glyph id as the
//! inner index with outer index zero; `MVAR` through value records keyed by
//! tag. Every answer is 16.16, the design's numeric model: a default from the
//! table the item lives in plus the item's net adjustment at the location.
//! A table whose region list does not have `fvar`'s axis count is refused by
//! name.

use super::item_store::{DeltaSetIndex, DeltaSetIndexMap, ItemVariationStore};
use super::{be16, be32, tag_value, DesignSpace, Fixed, Location, Tag, VariationError};
use crate::sfnt::Sfnt;

const TAG_HVAR: Tag = *b"HVAR";
const TAG_VVAR: Tag = *b"VVAR";
const TAG_MVAR: Tag = *b"MVAR";

/// The store a metrics table points at, with its axis count checked against
/// `fvar`'s.
fn store_at<'a>(
    table: &'a [u8],
    tag: Tag,
    offset: u32,
    axis_count: u16,
) -> Result<ItemVariationStore<'a>, VariationError> {
    if offset == 0 {
        return Err(VariationError::NoStore { table: tag });
    }
    let bytes = table
        .get(offset as usize..)
        .ok_or(VariationError::StorePastEnd)?;
    let store = ItemVariationStore::parse(bytes)?;
    if store.axis_count() != axis_count {
        return Err(VariationError::AxisCount {
            table: tag,
            found: store.axis_count(),
            expected: axis_count,
        });
    }
    Ok(store)
}

/// A delta-set index map a metrics table points at, where it points at one;
/// format 0 only, as `HVAR` and `VVAR` require.
fn map_at<'a>(
    table: &'a [u8],
    tag: Tag,
    offset: u32,
) -> Result<Option<DeltaSetIndexMap<'a>>, VariationError> {
    if offset == 0 {
        return Ok(None);
    }
    let bytes = table
        .get(offset as usize..)
        .ok_or(VariationError::MapPastEnd)?;
    let map = DeltaSetIndexMap::parse(bytes)?;
    if map.format() != 0 {
        return Err(VariationError::MapFormatIn {
            table: tag,
            format: map.format(),
        });
    }
    Ok(Some(map))
}

/// A metrics table's major version, which must be 1.
fn version(table: &[u8], tag: Tag) -> Result<(), VariationError> {
    let major = be16(table, 0).ok_or(VariationError::HeaderPastEnd { table: tag })?;
    let minor = be16(table, 2).ok_or(VariationError::HeaderPastEnd { table: tag })?;
    if major != 1 {
        return Err(VariationError::UnknownVersion {
            table: tag,
            major,
            minor,
        });
    }
    Ok(())
}

/// The axis count a design space's tables are read with.
fn axes_of(space: &DesignSpace) -> u16 {
    // `MAX_AXES` keeps it inside sixteen bits.
    u16::try_from(space.axis_count()).unwrap_or(u16::MAX)
}

/// `HVAR`: variations of `hmtx`'s advances and the side bearings.
#[derive(Clone, Debug)]
pub struct Hvar<'a> {
    store: ItemVariationStore<'a>,
    advance: Option<DeltaSetIndexMap<'a>>,
    left: Option<DeltaSetIndexMap<'a>>,
    right: Option<DeltaSetIndexMap<'a>>,
}

impl<'a> Hvar<'a> {
    /// Parses an `HVAR` table for a design space of `axis_count` axes.
    ///
    /// # Errors
    ///
    /// A version other than 1, a header past the end, no store, the store's
    /// and the maps' refusals, a map of format 1, and a region list whose
    /// axis count is not `axis_count` — by name.
    pub fn parse(table: &'a [u8], axis_count: u16) -> Result<Hvar<'a>, VariationError> {
        version(table, TAG_HVAR)?;
        let field = |at| be32(table, at).ok_or(VariationError::HeaderPastEnd { table: TAG_HVAR });
        let offsets = (field(4)?, field(8)?, field(12)?, field(16)?);
        let store = store_at(table, TAG_HVAR, offsets.0, axis_count)?;
        let advance = map_at(table, TAG_HVAR, offsets.1)?;
        let left = map_at(table, TAG_HVAR, offsets.2)?;
        let right = map_at(table, TAG_HVAR, offsets.3)?;
        Ok(Hvar {
            store,
            advance,
            left,
            right,
        })
    }

    /// A face's `HVAR`, where it varies and carries one.
    ///
    /// # Errors
    ///
    /// [`Hvar::parse`]'s.
    pub fn read(sfnt: &Sfnt<'a>, space: &DesignSpace) -> Result<Option<Hvar<'a>>, VariationError> {
        match sfnt.table(tag_value(TAG_HVAR)) {
            Some(table) if space.is_variable() => Hvar::parse(table, axes_of(space)).map(Some),
            _ => Ok(None),
        }
    }

    /// The store.
    #[must_use]
    pub fn store(&self) -> &ItemVariationStore<'a> {
        &self.store
    }

    /// The store's region scalars at a location, which every delta below
    /// reads.
    #[must_use]
    pub fn scalars(&self, location: &Location) -> Vec<Fixed> {
        self.store.region_scalars(location)
    }

    /// The delta set of a glyph's advance: through the map, or the glyph id
    /// as the inner index of the first subtable where there is none (`HVAR`,
    /// 'Processing').
    ///
    /// # Errors
    ///
    /// A map entry whose outer index needs more than sixteen bits.
    pub fn advance_index(&self, glyph: u16) -> Result<DeltaSetIndex, VariationError> {
        match &self.advance {
            Some(map) => map.get(u32::from(glyph)),
            None => Ok(DeltaSetIndex::new(0, glyph)),
        }
    }

    /// A glyph's advance adjustment.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the glyph takes.
    pub fn advance_delta(&self, glyph: u16, scalars: &[Fixed]) -> Result<Fixed, VariationError> {
        self.store.delta(self.advance_index(glyph)?, scalars)
    }

    /// A glyph's left side-bearing adjustment, where the table varies side
    /// bearings.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the glyph takes.
    pub fn left_side_bearing_delta(
        &self,
        glyph: u16,
        scalars: &[Fixed],
    ) -> Result<Option<Fixed>, VariationError> {
        mapped_delta(&self.store, self.left.as_ref(), glyph, scalars)
    }

    /// A glyph's right side-bearing adjustment, where the table varies side
    /// bearings.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the glyph takes.
    pub fn right_side_bearing_delta(
        &self,
        glyph: u16,
        scalars: &[Fixed],
    ) -> Result<Option<Fixed>, VariationError> {
        mapped_delta(&self.store, self.right.as_ref(), glyph, scalars)
    }

    /// A glyph's advance at a location: `hmtx`'s plus the adjustment; `None`
    /// where `hmtx` gives the glyph none.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the glyph takes.
    pub fn advance_at(
        &self,
        sfnt: &Sfnt,
        glyph: u16,
        location: &Location,
    ) -> Result<Option<Fixed>, VariationError> {
        let Some(default) = sfnt.advance(glyph) else {
            return Ok(None);
        };
        let delta = self.advance_delta(glyph, &self.scalars(location))?;
        Ok(Some(
            Fixed::from_int(i32::from(default)).saturating_add(delta),
        ))
    }
}

/// An item's delta through a map, or none where there is no map.
fn mapped_delta(
    store: &ItemVariationStore,
    map: Option<&DeltaSetIndexMap>,
    glyph: u16,
    scalars: &[Fixed],
) -> Result<Option<Fixed>, VariationError> {
    let Some(map) = map else {
        return Ok(None);
    };
    store.delta(map.get(u32::from(glyph))?, scalars).map(Some)
}

/// `VVAR`: variations of `vmtx`'s advances, the side bearings and `VORG`'s
/// origins.
#[derive(Clone, Debug)]
pub struct Vvar<'a> {
    store: ItemVariationStore<'a>,
    advance: Option<DeltaSetIndexMap<'a>>,
    top: Option<DeltaSetIndexMap<'a>>,
    bottom: Option<DeltaSetIndexMap<'a>>,
    origin: Option<DeltaSetIndexMap<'a>>,
}

impl<'a> Vvar<'a> {
    /// Parses a `VVAR` table for a design space of `axis_count` axes.
    ///
    /// # Errors
    ///
    /// As [`Hvar::parse`].
    pub fn parse(table: &'a [u8], axis_count: u16) -> Result<Vvar<'a>, VariationError> {
        version(table, TAG_VVAR)?;
        let field = |at| be32(table, at).ok_or(VariationError::HeaderPastEnd { table: TAG_VVAR });
        let offsets = (field(4)?, field(8)?, field(12)?, field(16)?, field(20)?);
        let store = store_at(table, TAG_VVAR, offsets.0, axis_count)?;
        let advance = map_at(table, TAG_VVAR, offsets.1)?;
        let top = map_at(table, TAG_VVAR, offsets.2)?;
        let bottom = map_at(table, TAG_VVAR, offsets.3)?;
        let origin = map_at(table, TAG_VVAR, offsets.4)?;
        Ok(Vvar {
            store,
            advance,
            top,
            bottom,
            origin,
        })
    }

    /// A face's `VVAR`, where it varies and carries one.
    ///
    /// # Errors
    ///
    /// [`Vvar::parse`]'s.
    pub fn read(sfnt: &Sfnt<'a>, space: &DesignSpace) -> Result<Option<Vvar<'a>>, VariationError> {
        match sfnt.table(tag_value(TAG_VVAR)) {
            Some(table) if space.is_variable() => Vvar::parse(table, axes_of(space)).map(Some),
            _ => Ok(None),
        }
    }

    /// The store.
    #[must_use]
    pub fn store(&self) -> &ItemVariationStore<'a> {
        &self.store
    }

    /// The store's region scalars at a location.
    #[must_use]
    pub fn scalars(&self, location: &Location) -> Vec<Fixed> {
        self.store.region_scalars(location)
    }

    /// The delta set of a glyph's advance height: through the map, or the
    /// glyph id as `HVAR` takes it where there is none.
    ///
    /// # Errors
    ///
    /// A map entry whose outer index needs more than sixteen bits.
    pub fn advance_index(&self, glyph: u16) -> Result<DeltaSetIndex, VariationError> {
        match &self.advance {
            Some(map) => map.get(u32::from(glyph)),
            None => Ok(DeltaSetIndex::new(0, glyph)),
        }
    }

    /// A glyph's advance-height adjustment.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the glyph takes.
    pub fn advance_delta(&self, glyph: u16, scalars: &[Fixed]) -> Result<Fixed, VariationError> {
        self.store.delta(self.advance_index(glyph)?, scalars)
    }

    /// A glyph's top side-bearing adjustment, where the table varies it.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the glyph takes.
    pub fn top_side_bearing_delta(
        &self,
        glyph: u16,
        scalars: &[Fixed],
    ) -> Result<Option<Fixed>, VariationError> {
        mapped_delta(&self.store, self.top.as_ref(), glyph, scalars)
    }

    /// A glyph's bottom side-bearing adjustment, where the table varies it.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the glyph takes.
    pub fn bottom_side_bearing_delta(
        &self,
        glyph: u16,
        scalars: &[Fixed],
    ) -> Result<Option<Fixed>, VariationError> {
        mapped_delta(&self.store, self.bottom.as_ref(), glyph, scalars)
    }

    /// A glyph's vertical-origin adjustment, where the table varies it — a
    /// CFF2 face's, whose origins `VORG` records.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the glyph takes.
    pub fn origin_delta(
        &self,
        glyph: u16,
        scalars: &[Fixed],
    ) -> Result<Option<Fixed>, VariationError> {
        mapped_delta(&self.store, self.origin.as_ref(), glyph, scalars)
    }

    /// A glyph's advance height at a location: `vmtx`'s plus the adjustment;
    /// `None` where `vmtx` gives the glyph none.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the glyph takes.
    pub fn advance_at(
        &self,
        sfnt: &Sfnt,
        glyph: u16,
        location: &Location,
    ) -> Result<Option<Fixed>, VariationError> {
        let Some(default) = sfnt.vertical_advance(glyph) else {
            return Ok(None);
        };
        let delta = self.advance_delta(glyph, &self.scalars(location))?;
        Ok(Some(
            Fixed::from_int(i32::from(default)).saturating_add(delta),
        ))
    }

    /// A glyph's vertical origin at a location: `VORG`'s `vertOriginY`
    /// ([`Sfnt::vorg_origin`]) plus the adjustment, where the face has a
    /// `VORG`; `None` where it has none.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the glyph takes.
    pub fn origin_at(
        &self,
        sfnt: &Sfnt,
        glyph: u16,
        location: &Location,
    ) -> Result<Option<Fixed>, VariationError> {
        let Some(default) = sfnt.vorg_origin(glyph) else {
            return Ok(None);
        };
        let delta = self
            .origin_delta(glyph, &self.scalars(location))?
            .unwrap_or(Fixed::ZERO);
        Ok(Some(
            Fixed::from_int(i32::from(default)).saturating_add(delta),
        ))
    }
}

/// Where the default of a metric `MVAR` names lives: the table, the field's
/// offset in it, and whether the field is signed (`MVAR`, 'Value Tags').
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricField {
    /// The value tag.
    pub tag: Tag,
    /// The table holding the default.
    pub table: Tag,
    /// The field's byte offset in it.
    pub offset: usize,
    /// Whether it is an FWORD rather than a UFWORD or a `uint16`.
    pub signed: bool,
}

const fn field(tag: &[u8; 4], table: &[u8; 4], offset: usize, signed: bool) -> MetricField {
    MetricField {
        tag: *tag,
        table: *table,
        offset,
        signed,
    }
}

/// Every value tag the `MVAR` chapter defines, with its default's place: the
/// `OS/2`, `hhea`, `vhea` and `post` metrics, and `gasp`'s ranges.
pub const METRIC_FIELDS: &[MetricField] = &[
    field(b"hasc", b"OS/2", 68, true),
    field(b"hdsc", b"OS/2", 70, true),
    field(b"hlgp", b"OS/2", 72, true),
    field(b"hcla", b"OS/2", 74, false),
    field(b"hcld", b"OS/2", 76, false),
    field(b"vasc", b"vhea", 4, true),
    field(b"vdsc", b"vhea", 6, true),
    field(b"vlgp", b"vhea", 8, true),
    field(b"hcrs", b"hhea", 18, true),
    field(b"hcrn", b"hhea", 20, true),
    field(b"hcof", b"hhea", 22, true),
    field(b"vcrs", b"vhea", 18, true),
    field(b"vcrn", b"vhea", 20, true),
    field(b"vcof", b"vhea", 22, true),
    field(b"xhgt", b"OS/2", 86, true),
    field(b"cpht", b"OS/2", 88, true),
    field(b"sbxs", b"OS/2", 10, true),
    field(b"sbys", b"OS/2", 12, true),
    field(b"sbxo", b"OS/2", 14, true),
    field(b"sbyo", b"OS/2", 16, true),
    field(b"spxs", b"OS/2", 18, true),
    field(b"spys", b"OS/2", 20, true),
    field(b"spxo", b"OS/2", 22, true),
    field(b"spyo", b"OS/2", 24, true),
    field(b"strs", b"OS/2", 26, true),
    field(b"stro", b"OS/2", 28, true),
    field(b"unds", b"post", 10, true),
    field(b"undo", b"post", 8, true),
    field(b"gsp0", b"gasp", 4, false),
    field(b"gsp1", b"gasp", 8, false),
    field(b"gsp2", b"gasp", 12, false),
    field(b"gsp3", b"gasp", 16, false),
    field(b"gsp4", b"gasp", 20, false),
    field(b"gsp5", b"gasp", 24, false),
    field(b"gsp6", b"gasp", 28, false),
    field(b"gsp7", b"gasp", 32, false),
    field(b"gsp8", b"gasp", 36, false),
    field(b"gsp9", b"gasp", 40, false),
];

/// The default of the metric a value tag names, from the face's own table;
/// `None` for a tag the chapter does not define, or a field the face's table
/// does not reach — an `OS/2` older than version 2 has no x-height, and a
/// `gasp` range past its count is not there.
#[must_use]
pub fn metric_default(sfnt: &Sfnt, tag: Tag) -> Option<i32> {
    let field = METRIC_FIELDS.iter().find(|field| field.tag == tag)?;
    let table = sfnt.table(tag_value(field.table))?;
    if field.table == *b"gasp" {
        let ranges = usize::from(be16(table, 2)?);
        let range = (field.offset - 4) / 4;
        if range >= ranges {
            return None;
        }
    }
    let value = be16(table, field.offset)?;
    Some(if field.signed {
        i32::from(value as i16)
    } else {
        i32::from(value)
    })
}

/// `MVAR`: variations of font-wide metrics, by value tag.
#[derive(Clone, Debug)]
pub struct Mvar<'a> {
    store: Option<ItemVariationStore<'a>>,
    records: &'a [u8],
    record_size: usize,
    count: usize,
}

impl<'a> Mvar<'a> {
    /// Parses an `MVAR` table for a design space of `axis_count` axes.
    ///
    /// # Errors
    ///
    /// A version other than 1, a header or value records past the end, a
    /// record size under eight bytes, records with no store, the store's
    /// refusals, and a region list whose axis count is not `axis_count` — by
    /// name.
    pub fn parse(table: &'a [u8], axis_count: u16) -> Result<Mvar<'a>, VariationError> {
        version(table, TAG_MVAR)?;
        let header = |at| be16(table, at).ok_or(VariationError::HeaderPastEnd { table: TAG_MVAR });
        let record_size = header(6)?;
        let count = header(8)?;
        let store_offset = header(10)?;
        // A tag and two indices: eight bytes, and more is an extension.
        if record_size < 8 {
            return Err(VariationError::ValueRecordSize { size: record_size });
        }
        let end = 12 + usize::from(record_size) * usize::from(count);
        let records = table
            .get(12..end)
            .ok_or(VariationError::ValueRecordsPastEnd { count })?;
        let store = if count == 0 {
            None
        } else {
            Some(store_at(
                table,
                TAG_MVAR,
                u32::from(store_offset),
                axis_count,
            )?)
        };
        Ok(Mvar {
            store,
            records,
            record_size: usize::from(record_size),
            count: usize::from(count),
        })
    }

    /// A face's `MVAR`, where it varies and carries one.
    ///
    /// # Errors
    ///
    /// [`Mvar::parse`]'s.
    pub fn read(sfnt: &Sfnt<'a>, space: &DesignSpace) -> Result<Option<Mvar<'a>>, VariationError> {
        match sfnt.table(tag_value(TAG_MVAR)) {
            Some(table) if space.is_variable() => Mvar::parse(table, axes_of(space)).map(Some),
            _ => Ok(None),
        }
    }

    /// The store, where the table has records.
    #[must_use]
    pub fn store(&self) -> Option<&ItemVariationStore<'a>> {
        self.store.as_ref()
    }

    /// The store's region scalars at a location; none where there is no
    /// store.
    #[must_use]
    pub fn scalars(&self, location: &Location) -> Vec<Fixed> {
        self.store
            .as_ref()
            .map_or_else(Vec::new, |store| store.region_scalars(location))
    }

    /// Every value tag the records name, in record order.
    #[must_use]
    pub fn tags(&self) -> Vec<Tag> {
        (0..self.count)
            .filter_map(|record| {
                let at = record * self.record_size;
                self.records
                    .get(at..at + 4)
                    .and_then(|bytes| <Tag>::try_from(bytes).ok())
            })
            .collect()
    }

    /// The delta set a tag's record names, or `None` where no record names
    /// it. The records are scanned in order, so a table out of the binary
    /// order the chapter asks for still answers with its first record.
    #[must_use]
    pub fn index(&self, tag: Tag) -> Option<DeltaSetIndex> {
        (0..self.count).find_map(|record| {
            let at = record * self.record_size;
            if self.records.get(at..at + 4)? != tag {
                return None;
            }
            Some(DeltaSetIndex::new(
                be16(self.records, at + 4)?,
                be16(self.records, at + 6)?,
            ))
        })
    }

    /// A tag's adjustment: `None` where no record names it, which the
    /// chapter makes constant across the design space.
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the record names.
    pub fn delta(&self, tag: Tag, scalars: &[Fixed]) -> Result<Option<Fixed>, VariationError> {
        let (Some(index), Some(store)) = (self.index(tag), self.store.as_ref()) else {
            return Ok(None);
        };
        store.delta(index, scalars).map(Some)
    }

    /// The metric a tag names at a location: its default from the face's
    /// table plus its adjustment, the default alone where no record names
    /// it; `None` where the face has no default for it
    /// ([`metric_default`]).
    ///
    /// # Errors
    ///
    /// The store's refusals for the index the record names.
    pub fn value_at(
        &self,
        sfnt: &Sfnt,
        tag: Tag,
        location: &Location,
    ) -> Result<Option<Fixed>, VariationError> {
        let Some(default) = metric_default(sfnt, tag) else {
            return Ok(None);
        };
        let delta = self
            .delta(tag, &self.scalars(location))?
            .unwrap_or(Fixed::ZERO);
        Ok(Some(Fixed::from_int(default).saturating_add(delta)))
    }
}
