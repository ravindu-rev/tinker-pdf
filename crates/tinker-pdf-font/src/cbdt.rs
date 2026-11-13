//! `CBLC` and `CBDT`, the colour bitmap location and data tables: a glyph as a
//! PNG per strike (OpenType 1.9.1, the CBLC and CBDT chapters, over the EBLC
//! chapter's layout).
//!
//! `CBLC` is `EBLC`'s layout at major version 3 (*CblcHeader*): one
//! `BitmapSize` record a strike, each naming an *IndexSubtableList* of glyph
//! ranges, each range an index subtable in one of five formats that locate a
//! glyph's data in `CBDT` and may carry the glyphs' metrics (*IndexSubtable
//! formats*). `CBDT`'s formats 17, 18 and 19 are PNG data with small, big or
//! the index subtable's metrics (*Glyph bitmap data formats*).
//!
//! The PNG is handed back as bytes, undecoded, with its strike's pixels per em
//! and its metrics in pixels. Every offset, count and length is held to the
//! table it is in (ruling 1); the uncompressed and component formats CBDT
//! inherits from `EBDT` are refused by name (ruling 2). A strike is chosen by
//! [`StrikeChoice`], the rule `sbix` shares.
//!
//! Design: `docs/design/colour-fonts.md`.

use crate::sbix::{choose, StrikeChoice};

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// The `flags` bit saying small metrics are for vertical text (*Bitmap flags*).
const VERTICAL_METRICS: u8 = 0x02;
/// The `flags` bit saying small metrics are for horizontal text.
const HORIZONTAL_METRICS: u8 = 0x01;

/// One strike's `BitmapSize` record, the fields a glyph's placement reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BitmapStrike {
    /// `ppemX`.
    pub ppem_x: u8,
    /// `ppemY`, the em's height, which a [`StrikeChoice`] compares.
    pub ppem_y: u8,
    /// `bitDepth`: 32 for colour.
    pub bit_depth: u8,
    /// `flags`: which direction small metrics are for (*Bitmap flags*).
    pub flags: u8,
    /// `startGlyphIndex`.
    pub start_glyph: u16,
    /// `endGlyphIndex`.
    pub end_glyph: u16,
}

/// A direction's bearings and advance, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bearings {
    /// From the origin to the bitmap's left edge (horizontal), or to its left
    /// edge from the vertical origin.
    pub bearing_x: i8,
    /// From the origin to the bitmap's top edge.
    pub bearing_y: i8,
    /// The advance.
    pub advance: u8,
}

/// A glyph's bitmap metrics, in pixels: big metrics carry both directions,
/// small ones the direction the strike's flags name (*Glyph metrics*).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BitmapMetrics {
    /// Rows.
    pub height: u8,
    /// Columns.
    pub width: u8,
    /// For horizontal text.
    pub horizontal: Option<Bearings>,
    /// For vertical text.
    pub vertical: Option<Bearings>,
}

/// A glyph's bitmap in one strike.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CbdtGlyph<'a> {
    /// The strike's `ppemX`.
    pub ppem_x: u8,
    /// The strike's `ppemY`.
    pub ppem_y: u8,
    /// The glyph data format: 17, 18 or 19.
    pub format: u16,
    /// Its metrics.
    pub metrics: BitmapMetrics,
    /// The PNG, undecoded.
    pub png: &'a [u8],
}

/// Why a glyph's data is no bitmap this reader hands back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CbdtRefusal {
    /// A glyph data format other than 17, 18 and 19: the formats CBDT inherits
    /// from `EBDT` (1 to 9), which a later step reads, and any the chapters
    /// do not define.
    DataFormat(u16),
    /// An index subtable format the chapter does not define.
    IndexFormat(u16),
    /// An index entry, an offset or a length that runs past its subtable or its
    /// table, offsets that run backwards, or format 19's data under an index
    /// subtable that carries no metrics.
    Malformed,
}

/// A parsed `CBLC`/`CBDT` pair over borrowed bytes.
#[derive(Clone, Debug)]
pub struct ColourBitmaps<'a> {
    cblc: &'a [u8],
    cbdt: &'a [u8],
    /// Each strike's record, and its index subtable list's offset, size and
    /// count.
    strikes: Vec<(BitmapStrike, usize, usize, u32)>,
}

/// Where a glyph's data lies in `CBDT` and where its metrics come from.
struct Located {
    at: usize,
    len: usize,
    image_format: u16,
    /// The index subtable's `bigMetrics`, for formats 2 and 5.
    metrics: Option<[u8; 8]>,
}

fn big_metrics(bytes: [u8; 8]) -> BitmapMetrics {
    BitmapMetrics {
        height: bytes[0],
        width: bytes[1],
        horizontal: Some(Bearings {
            bearing_x: bytes[2] as i8,
            bearing_y: bytes[3] as i8,
            advance: bytes[4],
        }),
        vertical: Some(Bearings {
            bearing_x: bytes[5] as i8,
            bearing_y: bytes[6] as i8,
            advance: bytes[7],
        }),
    }
}

impl<'a> ColourBitmaps<'a> {
    /// Parses the two headers and every `BitmapSize` record.
    ///
    /// `None` for a `CBLC` or a `CBDT` whose major version is not 3, and for a
    /// `BitmapSize` array that runs past `CBLC`. The index subtables are read a
    /// lookup at a time.
    #[must_use]
    pub fn parse(cblc: &'a [u8], cbdt: &'a [u8]) -> Option<ColourBitmaps<'a>> {
        if be16(cblc, 0)? != 3 || be16(cbdt, 0)? != 3 {
            return None;
        }
        let count = be32(cblc, 4)?;
        let end = 8u64 + u64::from(count) * 48;
        if end > u64::try_from(cblc.len()).ok()? {
            return None;
        }
        let mut strikes = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let record = 8 + i * 48;
            let byte = |o: usize| cblc.get(record + o).copied();
            strikes.push((
                BitmapStrike {
                    ppem_x: byte(44)?,
                    ppem_y: byte(45)?,
                    bit_depth: byte(46)?,
                    flags: byte(47)?,
                    start_glyph: be16(cblc, record + 40)?,
                    end_glyph: be16(cblc, record + 42)?,
                },
                usize::try_from(be32(cblc, record)?).ok()?,
                usize::try_from(be32(cblc, record + 4)?).ok()?,
                be32(cblc, record + 8)?,
            ));
        }
        Some(ColourBitmaps {
            cblc,
            cbdt,
            strikes,
        })
    }

    /// Every strike's record, in table order; a strike's index here is the one
    /// [`ColourBitmaps::glyph`] takes.
    #[must_use]
    pub fn strikes(&self) -> Vec<BitmapStrike> {
        self.strikes.iter().map(|(strike, ..)| *strike).collect()
    }

    /// Where `glyph`'s data lies in strike `strike`, `Ok(None)` where the
    /// strike places none.
    fn locate(&self, strike: usize, glyph: u16) -> Result<Option<Located>, CbdtRefusal> {
        let Some(&(_, list_at, list_size, count)) = self.strikes.get(strike) else {
            return Ok(None);
        };
        let bad = CbdtRefusal::Malformed;
        // The list and its subtables lie inside `indexSubtableListSize` bytes
        // of it, and inside the table.
        let list_end = list_at
            .checked_add(list_size)
            .ok_or(bad)?
            .min(self.cblc.len());
        let records_end = u64::try_from(list_at).map_err(|_| bad)? + u64::from(count) * 8;
        if records_end > u64::try_from(list_end).map_err(|_| bad)? {
            return Err(bad);
        }
        let list = self.cblc.get(..list_end).ok_or(bad)?;
        let mut found = None;
        for i in 0..count as usize {
            let record = list_at + i * 8;
            let first = be16(list, record).ok_or(bad)?;
            let last = be16(list, record + 2).ok_or(bad)?;
            if (first..=last).contains(&glyph) {
                let offset = be32(list, record + 4).ok_or(bad)?;
                found = Some((first, last, offset));
                break;
            }
        }
        let Some((first, last, offset)) = found else {
            return Ok(None);
        };
        let sub = list_at
            .checked_add(usize::try_from(offset).map_err(|_| bad)?)
            .ok_or(bad)?;
        let index_format = be16(list, sub).ok_or(bad)?;
        let image_format = be16(list, sub + 2).ok_or(bad)?;
        let image_at = usize::try_from(be32(list, sub + 4).ok_or(bad)?).map_err(|_| bad)?;
        let nth = usize::from(glyph - first);
        let span = |start: u32, end: u32| -> Result<Option<(usize, usize)>, CbdtRefusal> {
            if end < start {
                return Err(bad);
            }
            if end == start {
                return Ok(None);
            }
            let at = image_at
                .checked_add(usize::try_from(start).map_err(|_| bad)?)
                .ok_or(bad)?;
            Ok(Some((at, (end - start) as usize)))
        };
        let metrics_at = |at: usize| -> Result<[u8; 8], CbdtRefusal> {
            let bytes = list.get(at..at.checked_add(8).ok_or(bad)?).ok_or(bad)?;
            let mut out = [0u8; 8];
            out.copy_from_slice(bytes);
            Ok(out)
        };
        let constant = |index: usize, metrics: [u8; 8]| -> Result<Option<Located>, CbdtRefusal> {
            let size = usize::try_from(be32(list, sub + 8).ok_or(bad)?).map_err(|_| bad)?;
            let at = image_at
                .checked_add(index.checked_mul(size).ok_or(bad)?)
                .ok_or(bad)?;
            Ok((size > 0).then_some(Located {
                at,
                len: size,
                image_format,
                metrics: Some(metrics),
            }))
        };
        let (at, len) = match index_format {
            // Formats 1 and 3: an offset per glyph of the range and one more,
            // four bytes and two.
            1 | 3 => {
                let wide = index_format == 1;
                let entry = |i: usize| -> Result<u32, CbdtRefusal> {
                    if wide {
                        be32(list, sub + 8 + i * 4).ok_or(bad)
                    } else {
                        be16(list, sub + 8 + i * 2).map(u32::from).ok_or(bad)
                    }
                };
                // The whole array is the subtable's, whichever glyph is asked.
                let _ = entry(usize::from(last - first) + 1)?;
                match span(entry(nth)?, entry(nth + 1)?)? {
                    Some(span) => span,
                    None => return Ok(None),
                }
            }
            2 => return constant(nth, metrics_at(sub + 12)?),
            4 => {
                let glyphs = be32(list, sub + 8).ok_or(bad)? as usize;
                let pairs_end = (sub + 12)
                    .checked_add(
                        glyphs
                            .checked_add(1)
                            .ok_or(bad)?
                            .checked_mul(4)
                            .ok_or(bad)?,
                    )
                    .ok_or(bad)?;
                if pairs_end > list.len() {
                    return Err(bad);
                }
                let pair = |i: usize| (be16(list, sub + 12 + i * 4), be16(list, sub + 14 + i * 4));
                let Some(i) = (0..glyphs).find(|&i| pair(i).0 == Some(glyph)) else {
                    return Ok(None);
                };
                let (Some(start), Some(end)) = (pair(i).1, pair(i + 1).1) else {
                    return Err(bad);
                };
                match span(u32::from(start), u32::from(end))? {
                    Some(span) => span,
                    None => return Ok(None),
                }
            }
            5 => {
                let metrics = metrics_at(sub + 12)?;
                let glyphs = be32(list, sub + 20).ok_or(bad)? as usize;
                let ids_end = (sub + 24)
                    .checked_add(glyphs.checked_mul(2).ok_or(bad)?)
                    .ok_or(bad)?;
                if ids_end > list.len() {
                    return Err(bad);
                }
                let Some(i) = (0..glyphs).find(|&i| be16(list, sub + 24 + i * 2) == Some(glyph))
                else {
                    return Ok(None);
                };
                return constant(i, metrics);
            }
            other => return Err(CbdtRefusal::IndexFormat(other)),
        };
        Ok(Some(Located {
            at,
            len,
            image_format,
            metrics: None,
        }))
    }

    /// `glyph`'s bitmap in strike `strike`.
    ///
    /// `Ok(None)` where no index subtable of the strike holds the glyph, where
    /// the one that does places no data for it, and for a strike past the
    /// count.
    pub fn glyph(&self, strike: usize, glyph: u16) -> Result<Option<CbdtGlyph<'a>>, CbdtRefusal> {
        let Some(located) = self.locate(strike, glyph)? else {
            return Ok(None);
        };
        let Some(&(record, ..)) = self.strikes.get(strike) else {
            return Ok(None);
        };
        let bad = CbdtRefusal::Malformed;
        let end = located.at.checked_add(located.len).ok_or(bad)?;
        let data = self.cbdt.get(located.at..end).ok_or(bad)?;
        let (metrics, rest) = match located.image_format {
            17 => {
                let small = data.get(..5).ok_or(bad)?;
                let bearings = Bearings {
                    bearing_x: small[2] as i8,
                    bearing_y: small[3] as i8,
                    advance: small[4],
                };
                // *Bitmap flags*: small metrics are for the direction the
                // strike names, horizontal unless it names vertical alone.
                let vertical =
                    record.flags & VERTICAL_METRICS != 0 && record.flags & HORIZONTAL_METRICS == 0;
                let metrics = BitmapMetrics {
                    height: small[0],
                    width: small[1],
                    horizontal: (!vertical).then_some(bearings),
                    vertical: vertical.then_some(bearings),
                };
                (metrics, &data[5..])
            }
            18 => {
                let big = data.get(..8).ok_or(bad)?;
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(big);
                (big_metrics(bytes), &data[8..])
            }
            19 => (big_metrics(located.metrics.ok_or(bad)?), data),
            other => return Err(CbdtRefusal::DataFormat(other)),
        };
        let length = usize::try_from(be32(rest, 0).ok_or(bad)?).map_err(|_| bad)?;
        let png = rest
            .get(4..4usize.checked_add(length).ok_or(bad)?)
            .ok_or(bad)?;
        Ok(Some(CbdtGlyph {
            ppem_x: record.ppem_x,
            ppem_y: record.ppem_y,
            format: located.image_format,
            metrics,
            png,
        }))
    }

    /// Whether strike `strike` holds a bitmap for `glyph` this reader hands
    /// back.
    #[must_use]
    pub fn holds(&self, strike: usize, glyph: u16) -> bool {
        matches!(self.glyph(strike, glyph), Ok(Some(_)))
    }

    /// The strike `choice` picks for `glyph`, among those that hold it,
    /// comparing `ppemY`; `None` where none does.
    #[must_use]
    pub fn choose_strike(&self, glyph: u16, choice: StrikeChoice) -> Option<usize> {
        let candidates: Vec<(usize, u16)> = self
            .strikes
            .iter()
            .enumerate()
            .filter(|(index, _)| self.holds(*index, glyph))
            .map(|(index, (strike, ..))| (index, u16::from(strike.ppem_y)))
            .collect();
        choose(&candidates, choice)
    }
}
