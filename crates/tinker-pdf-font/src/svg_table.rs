//! `SVG `, the scalable vector graphics table: colour glyphs as SVG documents
//! (OpenType 1.9.1, the SVG chapter).
//!
//! The table is a document index (*SVG table formats*): records of a glyph
//! range, inclusive, and where in the table the document describing those
//! glyphs lies. A document may describe several ranges, and a record holds
//! one; the glyph's description is the document's element whose id is
//! `glyph` and the glyph id in decimal (*Glyph Identifiers*). A document is
//! plain UTF-8 or gzip-encoded (*Document encoding and format*), and an
//! encoded one is inflated here through `tinker_pdf_filters`' RFC 1952
//! reader, capped at [`MAX_SVG_DOCUMENT`].
//!
//! What comes back is bytes and an element id — no SVG is read here, and no
//! PDF vocabulary (ruling 8); `tinker-pdf-svg` reads the element. Every
//! offset and length is held to the table (ruling 1).
//!
//! Design: `docs/design/colour-fonts.md`.

use std::borrow::Cow;

use tinker_pdf_filters::{gzip_decode, GzipError, Limits};

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// The most bytes a gzip-encoded document may inflate to: 16 MiB
/// (`docs/design/colour-fonts.md`, *The SVG table*). One past it is refused
/// by name, not cut — a cut document is a different picture.
pub const MAX_SVG_DOCUMENT: usize = 16 << 20;

/// The first three bytes of a gzip-encoded document: *Document encoding and
/// format* requires deflate, so ID1, ID2 and CM 8.
const GZIP_SIGNATURE: [u8; 3] = [0x1f, 0x8b, 0x08];

/// One `SVGDocumentRecord`, as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SvgRecord {
    /// `startGlyphID`.
    pub start_glyph: u16,
    /// `endGlyphID`, inclusive.
    pub end_glyph: u16,
    /// `svgDocOffset`, from the start of the `SVGDocumentList`.
    pub offset: u32,
    /// `svgDocLength`: the encoded document's length.
    pub length: u32,
}

impl SvgRecord {
    /// Whether the record's range holds `glyph`; a range whose end is below
    /// its start holds none.
    #[must_use]
    pub fn holds(&self, glyph: u16) -> bool {
        (self.start_glyph..=self.end_glyph).contains(&glyph)
    }
}

/// A glyph's document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SvgDocument<'a> {
    /// The record whose range holds the glyph. Two glyphs whose records
    /// share an offset and a length share a document, which a caller reads
    /// once.
    pub record: SvgRecord,
    /// The document's bytes: the table's own for a plain document, inflated
    /// for a gzip-encoded one.
    pub data: Cow<'a, [u8]>,
    /// Whether the document was gzip-encoded.
    pub gzipped: bool,
    /// The id of the element that describes the glyph: `glyph` and the glyph
    /// id in decimal, not zero-padded (*Glyph Identifiers*).
    pub element: String,
}

/// Why a glyph's record names no document this reader hands back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SvgRefusal {
    /// The record's offset or length is zero, which *SVG table formats*
    /// forbids, or the document runs past the table.
    Malformed,
    /// A gzip-encoded document RFC 1952's reader refuses.
    Gzip(GzipError),
    /// A gzip-encoded document that inflates past [`MAX_SVG_DOCUMENT`].
    TooLarge,
}

/// Something the reader tolerated in a table it still reads (ruling 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SvgWarning {
    /// The document records are not sorted by `startGlyphID` with ranges
    /// that each start past the one before ends, which *SVG table formats*
    /// requires: they are scanned, and the first record whose range holds a
    /// glyph answers for it.
    DocumentsOutOfOrder,
}

/// A parsed `SVG ` table over borrowed bytes.
#[derive(Clone, Debug)]
pub struct SvgTable<'a> {
    data: &'a [u8],
    /// Where the `SVGDocumentList` starts.
    list_at: usize,
    /// `numEntries`.
    count: u16,
    /// Whether the records are in the order the chapter requires, so a
    /// lookup bisects.
    ordered: bool,
    warnings: Vec<SvgWarning>,
}

impl<'a> SvgTable<'a> {
    /// Parses the header and checks the document records' order once.
    ///
    /// `None` for a version other than 0, for a list offset of zero, and for
    /// a header or a record array that runs past the table: an index that
    /// cannot be read places no document.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<SvgTable<'a>> {
        // The header: `version`, `svgDocumentListOffset` and `reserved`, which
        // the chapter sets to 0 and asks nothing of.
        if data.len() < 10 || be16(data, 0)? != 0 {
            return None;
        }
        let list_at = usize::try_from(be32(data, 2)?).ok().filter(|&at| at != 0)?;
        let count = be16(data, list_at)?;
        let records_end = list_at
            .checked_add(2)?
            .checked_add(usize::from(count) * 12)?;
        if records_end > data.len() {
            return None;
        }
        let mut table = SvgTable {
            data,
            list_at,
            count,
            ordered: true,
            warnings: Vec::new(),
        };
        let mut previous_end: Option<u16> = None;
        for record in table.records() {
            let in_order = record.start_glyph <= record.end_glyph
                && previous_end.is_none_or(|end| record.start_glyph > end);
            if !in_order {
                table.ordered = false;
                table.warnings.push(SvgWarning::DocumentsOutOfOrder);
                break;
            }
            previous_end = Some(record.end_glyph);
        }
        Some(table)
    }

    /// What the reader tolerated, in the order it met it.
    #[must_use]
    pub fn warnings(&self) -> &[SvgWarning] {
        &self.warnings
    }

    /// Record `i`, which the parse held inside the table.
    fn record_at(&self, i: usize) -> Option<SvgRecord> {
        let at = self.list_at + 2 + i * 12;
        Some(SvgRecord {
            start_glyph: be16(self.data, at)?,
            end_glyph: be16(self.data, at + 2)?,
            offset: be32(self.data, at + 4)?,
            length: be32(self.data, at + 8)?,
        })
    }

    /// Every document record, in table order.
    #[must_use]
    pub fn records(&self) -> Vec<SvgRecord> {
        (0..usize::from(self.count))
            .filter_map(|i| self.record_at(i))
            .collect()
    }

    /// The record whose range holds `glyph`: bisected where the records are
    /// in order, and otherwise the first in table order that holds it.
    #[must_use]
    pub fn record(&self, glyph: u16) -> Option<SvgRecord> {
        let count = usize::from(self.count);
        if !self.ordered {
            return (0..count)
                .filter_map(|i| self.record_at(i))
                .find(|record| record.holds(glyph));
        }
        // The last record starting at or below the glyph is the only one that
        // can hold it, the ranges being sorted and disjoint.
        let (mut low, mut high) = (0usize, count);
        while low < high {
            let middle = low + (high - low) / 2;
            if self.record_at(middle)?.start_glyph <= glyph {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        low.checked_sub(1)
            .and_then(|i| self.record_at(i))
            .filter(|record| record.holds(glyph))
    }

    /// Whether some record's range holds `glyph`.
    #[must_use]
    pub fn has_document(&self, glyph: u16) -> bool {
        self.record(glyph).is_some()
    }

    /// The document `record` names: the table's bytes, or what they inflate
    /// to where they begin with gzip's signature.
    ///
    /// # Errors
    /// [`SvgRefusal::Malformed`] for an offset or length of zero or a
    /// document past the table; [`SvgRefusal::TooLarge`] for one that
    /// inflates past [`MAX_SVG_DOCUMENT`]; [`SvgRefusal::Gzip`] for any other
    /// gzip failure.
    pub fn document_of(&self, record: &SvgRecord) -> Result<(Cow<'a, [u8]>, bool), SvgRefusal> {
        if record.offset == 0 || record.length == 0 {
            return Err(SvgRefusal::Malformed);
        }
        let from = usize::try_from(record.offset)
            .ok()
            .and_then(|offset| self.list_at.checked_add(offset))
            .ok_or(SvgRefusal::Malformed)?;
        let to = usize::try_from(record.length)
            .ok()
            .and_then(|length| from.checked_add(length))
            .ok_or(SvgRefusal::Malformed)?;
        let bytes = self.data.get(from..to).ok_or(SvgRefusal::Malformed)?;
        if !bytes.starts_with(&GZIP_SIGNATURE) {
            return Ok((Cow::Borrowed(bytes), false));
        }
        match gzip_decode(bytes, &Limits::new(MAX_SVG_DOCUMENT)) {
            Ok(inflated) => Ok((Cow::Owned(inflated), true)),
            Err(GzipError::ExceedsOutputLimit { .. }) => Err(SvgRefusal::TooLarge),
            Err(error) => Err(SvgRefusal::Gzip(error)),
        }
    }

    /// `glyph`'s document, and the id of the element that describes it.
    ///
    /// `Ok(None)` where no record's range holds the glyph.
    ///
    /// # Errors
    /// [`SvgTable::document_of`]'s, for the record that holds the glyph.
    pub fn document(&self, glyph: u16) -> Result<Option<SvgDocument<'a>>, SvgRefusal> {
        let Some(record) = self.record(glyph) else {
            return Ok(None);
        };
        let (data, gzipped) = self.document_of(&record)?;
        Ok(Some(SvgDocument {
            record,
            data,
            gzipped,
            element: format!("glyph{glyph}"),
        }))
    }
}
