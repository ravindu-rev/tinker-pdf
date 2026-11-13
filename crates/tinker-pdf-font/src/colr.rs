//! `COLR`, the colour table: a colour glyph as layers of outline glyphs, each
//! filled with a palette entry (OpenType 1.9.1, the COLR chapter).
//!
//! Version 0 maps a base glyph to a slice of layer records, the first the
//! bottom layer (*BaseGlyph and Layer records*). Each layer is an outline
//! glyph and a `CPAL` palette entry, or 0xFFFF for the text's own foreground
//! colour (*Colors and solid color fills*). A version 1 table carries the same
//! two arrays first, and its paint graph after them.
//!
//! Every offset and count is held to the table's own bytes (ruling 1), and
//! what comes back is glyph ids, palette indices and colours — no PDF
//! vocabulary (ruling 8). Drawing them is the writer's.
//!
//! Design: `docs/design/colour-fonts.md`.

use crate::cpal::{Cpal, Rgba};

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// The palette index that is no index: the text's foreground colour (*Colors
/// and solid color fills*).
pub const FOREGROUND: u16 = 0xFFFF;

/// A colour reference as `COLR` writes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColourIndex {
    /// An entry of the palette the glyph is drawn in.
    Palette(u16),
    /// 0xFFFF: the colour the text is drawn in, which "must not be treated as
    /// an actual index into the CPAL ColorRecord array".
    Foreground,
}

impl ColourIndex {
    /// The reference a raw `paletteIndex` makes.
    #[must_use]
    pub fn from_raw(raw: u16) -> ColourIndex {
        if raw == FOREGROUND {
            ColourIndex::Foreground
        } else {
            ColourIndex::Palette(raw)
        }
    }
}

/// What a version 0 layer is filled with, its palette entry looked up.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LayerColour {
    /// The palette entry's colour, its own alpha included.
    Colour(Rgba),
    /// The text's colour.
    Foreground,
}

/// One version 0 layer: an outline glyph and what fills it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Layer {
    /// The glyph whose outline the layer fills.
    pub glyph: u16,
    /// Its colour.
    pub colour: LayerColour,
}

/// Something the reader tolerated in a table it still reads (ruling 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColrWarning {
    /// The `BaseGlyph` records are not in increasing glyph order, which
    /// *BaseGlyph and Layer records* requires: they are scanned, and the
    /// first record for a glyph answers for it.
    BaseGlyphsOutOfOrder,
}

/// A parsed `COLR` table over borrowed bytes.
#[derive(Clone, Debug)]
pub struct Colr<'a> {
    data: &'a [u8],
    version: u16,
    /// The `BaseGlyph` records: where they start and how many, where the whole
    /// array lies inside the table.
    base: Option<(usize, u16)>,
    /// Whether `base` is in increasing glyph order, so a lookup bisects.
    base_ordered: bool,
    /// The layer records: where they start and how many.
    layers_at: usize,
    layer_count: u16,
    warnings: Vec<ColrWarning>,
}

impl<'a> Colr<'a> {
    /// Parses the header of a version 0 or version 1 table.
    ///
    /// `None` for another version and for a header that runs past the data.
    /// A `BaseGlyph` array that runs past the table is read as none, so every
    /// glyph's layers answer `None`; the layer records are checked a lookup at
    /// a time.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<Colr<'a>> {
        let version = be16(data, 0)?;
        let header = match version {
            0 => 14,
            1 => 34,
            _ => return None,
        };
        if data.len() < header {
            return None;
        }
        let base_count = be16(data, 2)?;
        let base_at = usize::try_from(be32(data, 4)?).ok()?;
        let layers_at = usize::try_from(be32(data, 8)?).ok()?;
        let layer_count = be16(data, 12)?;

        let mut warnings = Vec::new();
        let base_end = base_at.checked_add(usize::from(base_count) * 6);
        let base = (base_count > 0 && base_end.is_some_and(|end| end <= data.len()))
            .then_some((base_at, base_count));
        let mut base_ordered = true;
        if let Some((at, count)) = base {
            let mut previous: Option<u16> = None;
            for i in 0..usize::from(count) {
                let glyph = be16(data, at + i * 6)?;
                if previous.is_some_and(|previous| glyph <= previous) {
                    base_ordered = false;
                    warnings.push(ColrWarning::BaseGlyphsOutOfOrder);
                    break;
                }
                previous = Some(glyph);
            }
        }
        Some(Colr {
            data,
            version,
            base,
            base_ordered,
            layers_at,
            layer_count,
            warnings,
        })
    }

    /// The table's version, 0 or 1.
    #[must_use]
    pub fn version(&self) -> u16 {
        self.version
    }

    /// What the reader tolerated, in the order it met it.
    #[must_use]
    pub fn warnings(&self) -> &[ColrWarning] {
        &self.warnings
    }

    /// The `BaseGlyph` record for `glyph`: its first layer and layer count.
    fn base_record(&self, glyph: u16) -> Option<(u16, u16)> {
        let (at, count) = self.base?;
        let record = |i: usize| -> Option<(u16, u16, u16)> {
            let r = at + i * 6;
            Some((
                be16(self.data, r)?,
                be16(self.data, r + 2)?,
                be16(self.data, r + 4)?,
            ))
        };
        if !self.base_ordered {
            return (0..usize::from(count))
                .filter_map(record)
                .find(|(id, _, _)| *id == glyph)
                .map(|(_, first, layers)| (first, layers));
        }
        let (mut low, mut high) = (0usize, usize::from(count));
        while low < high {
            let middle = low + (high - low) / 2;
            let (id, first, layers) = record(middle)?;
            match id.cmp(&glyph) {
                core::cmp::Ordering::Less => low = middle + 1,
                core::cmp::Ordering::Greater => high = middle,
                core::cmp::Ordering::Equal => return Some((first, layers)),
            }
        }
        None
    }

    /// Whether `glyph` has a version 0 record.
    #[must_use]
    pub fn has_layers(&self, glyph: u16) -> bool {
        self.base_record(glyph).is_some()
    }

    /// `glyph`'s version 0 layers as the table writes them, bottom first: each
    /// outline glyph and its colour reference.
    ///
    /// `None` where the glyph has no record, and where its slice runs past
    /// `numLayerRecords` or past the table. A record of no layers answers an
    /// empty list.
    #[must_use]
    pub fn layer_records(&self, glyph: u16) -> Option<Vec<(u16, ColourIndex)>> {
        let (first, count) = self.base_record(glyph)?;
        let end = u32::from(first) + u32::from(count);
        if end > u32::from(self.layer_count) {
            return None;
        }
        (usize::from(first)..usize::from(first) + usize::from(count))
            .map(|i| {
                let at = self.layers_at.checked_add(i.checked_mul(4)?)?;
                let glyph = be16(self.data, at)?;
                let index = be16(self.data, at.checked_add(2)?)?;
                Some((glyph, ColourIndex::from_raw(index)))
            })
            .collect()
    }

    /// `glyph`'s version 0 layers, bottom first, each palette entry looked up in
    /// palette `palette` of `cpal`.
    ///
    /// `None` where [`Colr::layer_records`] answers `None`, and where any
    /// layer names an entry the palette does not have: a glyph drawn with a
    /// layer missing is a different picture, and its outline is the honest
    /// one.
    #[must_use]
    pub fn layers(&self, glyph: u16, cpal: &Cpal<'_>, palette: u16) -> Option<Vec<Layer>> {
        self.layer_records(glyph)?
            .into_iter()
            .map(|(glyph, index)| {
                let colour = match index {
                    ColourIndex::Foreground => LayerColour::Foreground,
                    ColourIndex::Palette(entry) => {
                        LayerColour::Colour(cpal.colour(palette, entry)?)
                    }
                };
                Some(Layer { glyph, colour })
            })
            .collect()
    }
}
