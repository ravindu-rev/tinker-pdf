//! TrueType and OpenType font programs: the tables metrics need.
//!
//! Wave 1 reads only what text extraction and layout require — the table
//! directory, `head`, `hhea`, `hmtx`, `cmap` and `post`. Glyph outlines are
//! wave 2; nothing here touches `glyf` or `CFF `.
//!
//! Every read is bounds-checked against the buffer the caller supplied,
//! because a font program is document-controlled data like any other.

/// A parsed table directory over borrowed bytes.
#[derive(Clone, Debug)]
pub struct Sfnt<'a> {
    data: &'a [u8],
    tables: Vec<(u32, u32, u32)>,
    /// Units per em from `head`; 1000 when the table is missing or absurd.
    pub units_per_em: u16,
    /// Number of `hmtx` entries with their own advance, from `hhea`.
    num_h_metrics: u16,
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at + 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl<'a> Sfnt<'a> {
    /// Parses the table directory.
    ///
    /// Accepts the plain `sfnt` forms and a TrueType Collection, from which
    /// the first font is taken — a PDF embedding a collection means the first
    /// face, since it has no way to say otherwise.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<Sfnt<'a>> {
        let mut base = 0usize;
        if data.get(..4) == Some(b"ttcf") {
            base = be32(data, 12)? as usize;
        }

        let tag = be32(data, base)?;
        // 0x00010000 TrueType, 'true', 'OTTO' for CFF outlines.
        if !matches!(tag, 0x0001_0000 | 0x7472_7565 | 0x4F54_544F) {
            return None;
        }

        let count = be16(data, base + 4)? as usize;
        let mut tables = Vec::with_capacity(count.min(64));
        for i in 0..count.min(512) {
            let at = base + 12 + i * 16;
            let (Some(tag), Some(off), Some(len)) =
                (be32(data, at), be32(data, at + 8), be32(data, at + 12))
            else {
                break;
            };
            tables.push((tag, off, len));
        }

        let mut sfnt = Sfnt {
            data,
            tables,
            units_per_em: 1000,
            num_h_metrics: 0,
        };

        if let Some(head) = sfnt.table(0x6865_6164) {
            // A units-per-em of zero would divide by zero downstream.
            if let Some(upem) = be16(head, 18).filter(|v| *v >= 16) {
                sfnt.units_per_em = upem;
            }
        }
        if let Some(hhea) = sfnt.table(0x6868_6561) {
            sfnt.num_h_metrics = be16(hhea, 34).unwrap_or(0);
        }

        Some(sfnt)
    }

    /// One table's bytes, by tag.
    #[must_use]
    pub fn table(&self, tag: u32) -> Option<&'a [u8]> {
        let (_, off, len) = self.tables.iter().find(|(t, _, _)| *t == tag)?;
        self.data.get(*off as usize..(*off as usize).checked_add(*len as usize)?)
    }

    /// The advance of a glyph, in font units.
    ///
    /// `hmtx` stores full entries for the first `numberOfHMetrics` glyphs and
    /// only side bearings after that, every one of which repeats the last
    /// advance — that is how a monospaced font stays small.
    #[must_use]
    pub fn advance(&self, glyph: u16) -> Option<u16> {
        let hmtx = self.table(0x686D_7478)?;
        if self.num_h_metrics == 0 {
            return None;
        }
        let index = glyph.min(self.num_h_metrics - 1);
        be16(hmtx, usize::from(index) * 4)
    }

    /// A character's glyph index, through the best `cmap` subtable available.
    #[must_use]
    pub fn glyph_for_char(&self, c: char) -> Option<u16> {
        let cmap = self.table(0x636D_6170)?;
        let count = be16(cmap, 2)? as usize;

        // 9.6.6.4 prefers (3,1) Windows Unicode BMP, then (3,0) symbol, then
        // (1,0) Macintosh Roman. (3,10) covers characters beyond the BMP.
        let mut best: Option<(u32, usize)> = None;
        for i in 0..count.min(64) {
            let at = 4 + i * 8;
            let (Some(platform), Some(encoding), Some(offset)) =
                (be16(cmap, at), be16(cmap, at + 2), be32(cmap, at + 4))
            else {
                break;
            };
            let score = match (platform, encoding) {
                (3, 10) => 5,
                (3, 1) => 4,
                (0, _) => 3,
                (3, 0) => 2,
                (1, 0) => 1,
                _ => 0,
            };
            if score > 0 && best.is_none_or(|(b, _)| score > b) {
                best = Some((score, offset as usize));
            }
        }

        let (score, offset) = best?;
        let sub = cmap.get(offset..)?;
        let mut code = u32::from(c);
        // A (3,0) symbol subtable maps the F0xx private-use block, and
        // documents address it with the low byte (9.6.6.4).
        if score == 2 && code < 0x100 {
            code |= 0xF000;
        }
        lookup_cmap(sub, code)
    }

    /// The glyph named `name`, through the `post` table's version 2 names.
    #[must_use]
    pub fn glyph_for_name(&self, name: &str) -> Option<u16> {
        let post = self.table(0x706F_7374)?;
        if be32(post, 0)? != 0x0002_0000 {
            return None;
        }
        let count = be16(post, 32)? as usize;

        let indices: Vec<u16> = (0..count)
            .filter_map(|i| be16(post, 34 + i * 2))
            .collect();

        // Names past the 258 standard Macintosh ones are Pascal strings
        // packed after the index array.
        let mut names: Vec<&[u8]> = Vec::new();
        let mut at = 34 + count * 2;
        while at < post.len() && names.len() < count {
            let len = *post.get(at)? as usize;
            let start = at + 1;
            let Some(text) = post.get(start..start + len) else {
                break;
            };
            names.push(text);
            at = start + len;
        }

        indices.iter().enumerate().find_map(|(glyph, &index)| {
            if index < 258 {
                return None;
            }
            let custom = names.get(usize::from(index) - 258)?;
            (*custom == name.as_bytes()).then_some(glyph as u16)
        })
    }
}

/// Looks a code up in one `cmap` subtable, in the formats that occur.
fn lookup_cmap(sub: &[u8], code: u32) -> Option<u16> {
    match be16(sub, 0)? {
        // Format 0: a 256-byte table.
        0 => {
            let byte = u8::try_from(code).ok()?;
            sub.get(6 + usize::from(byte)).map(|&g| u16::from(g))
        }
        // Format 4: segmented, the ubiquitous BMP format.
        4 => {
            let code = u16::try_from(code).ok()?;
            let seg2 = be16(sub, 6)? as usize;
            let segs = seg2 / 2;
            let ends = 14;
            let starts = ends + seg2 + 2;
            let deltas = starts + seg2;
            let ranges = deltas + seg2;

            for i in 0..segs {
                let end = be16(sub, ends + i * 2)?;
                if code > end {
                    continue;
                }
                let start = be16(sub, starts + i * 2)?;
                if code < start {
                    return Some(0);
                }
                let delta = be16(sub, deltas + i * 2)?;
                let range_offset = be16(sub, ranges + i * 2)?;
                if range_offset == 0 {
                    return Some(code.wrapping_add(delta));
                }
                // The offset is relative to its own slot, which is the one
                // genuinely awkward thing about format 4.
                let at = ranges + i * 2 + usize::from(range_offset)
                    + usize::from(code - start) * 2;
                let glyph = be16(sub, at)?;
                return Some(if glyph == 0 {
                    0
                } else {
                    glyph.wrapping_add(delta)
                });
            }
            Some(0)
        }
        // Format 6: a single trimmed range.
        6 => {
            let first = u32::from(be16(sub, 6)?);
            let count = u32::from(be16(sub, 8)?);
            if code < first || code >= first + count {
                return Some(0);
            }
            be16(sub, 10 + ((code - first) as usize) * 2)
        }
        // Format 12: grouped ranges, for characters beyond the BMP.
        12 => {
            let groups = be32(sub, 12)? as usize;
            for i in 0..groups.min(1 << 20) {
                let at = 16 + i * 12;
                let (Some(start), Some(end), Some(glyph)) =
                    (be32(sub, at), be32(sub, at + 4), be32(sub, at + 8))
                else {
                    break;
                };
                if (start..=end).contains(&code) {
                    return u16::try_from(glyph + (code - start)).ok();
                }
            }
            Some(0)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal sfnt with one table, to exercise the directory.
    fn sfnt_with(tag: &[u8; 4], table: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes()); // numTables
        out.extend_from_slice(&[0; 6]); // search hints, unread
        out.extend_from_slice(tag);
        out.extend_from_slice(&0u32.to_be_bytes()); // checksum
        out.extend_from_slice(&28u32.to_be_bytes()); // offset
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        out.extend_from_slice(table);
        out
    }

    #[test]
    fn the_table_directory_is_read() {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&2048u16.to_be_bytes());
        let data = sfnt_with(b"head", &head);
        let sfnt = Sfnt::parse(&data).expect("a valid directory");
        assert_eq!(sfnt.units_per_em, 2048);
        assert!(sfnt.table(0x6865_6164).is_some());
        assert!(sfnt.table(0x676C_7966).is_none(), "no glyf in this font");
    }

    #[test]
    fn an_absurd_units_per_em_falls_back() {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&0u16.to_be_bytes());
        let data = sfnt_with(b"head", &head);
        let sfnt = Sfnt::parse(&data).expect("a valid directory");
        assert_eq!(sfnt.units_per_em, 1000, "zero would divide by zero later");
    }

    #[test]
    fn format_4_cmap_maps_a_segment() {
        // One segment covering 'A'..'C' with a delta, plus the required
        // terminating segment at 0xFFFF.
        let mut sub: Vec<u8> = Vec::new();
        sub.extend_from_slice(&4u16.to_be_bytes()); // format
        sub.extend_from_slice(&0u16.to_be_bytes()); // length, unread
        sub.extend_from_slice(&0u16.to_be_bytes()); // language
        sub.extend_from_slice(&4u16.to_be_bytes()); // segCountX2 = 2 segments
        sub.extend_from_slice(&[0; 6]); // search hints
        sub.extend_from_slice(&0x0043u16.to_be_bytes()); // endCode[0] = 'C'
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes()); // endCode[1]
        sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        sub.extend_from_slice(&0x0041u16.to_be_bytes()); // startCode[0] = 'A'
        sub.extend_from_slice(&0xFFFFu16.to_be_bytes()); // startCode[1]
        sub.extend_from_slice(&10u16.to_be_bytes()); // idDelta[0]
        sub.extend_from_slice(&1u16.to_be_bytes()); // idDelta[1]
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset[0]
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset[1]

        let mut cmap: Vec<u8> = Vec::new();
        cmap.extend_from_slice(&0u16.to_be_bytes()); // version
        cmap.extend_from_slice(&1u16.to_be_bytes()); // numTables
        cmap.extend_from_slice(&3u16.to_be_bytes()); // platform 3
        cmap.extend_from_slice(&1u16.to_be_bytes()); // encoding 1
        cmap.extend_from_slice(&12u32.to_be_bytes()); // offset
        cmap.extend_from_slice(&sub);

        let data = sfnt_with(b"cmap", &cmap);
        let sfnt = Sfnt::parse(&data).expect("a valid directory");
        assert_eq!(sfnt.glyph_for_char('A'), Some(0x41 + 10));
        assert_eq!(sfnt.glyph_for_char('C'), Some(0x43 + 10));
        assert_eq!(sfnt.glyph_for_char('Z'), Some(0), "outside the segment");
    }

    #[test]
    fn garbage_is_rejected_without_panicking() {
        assert!(Sfnt::parse(&[]).is_none());
        assert!(Sfnt::parse(b"not a font").is_none());
        assert!(Sfnt::parse(&[0u8; 12]).is_none());
        // A directory claiming more tables than the buffer holds.
        let mut data = vec![0u8; 12];
        data[..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
        data[4..6].copy_from_slice(&500u16.to_be_bytes());
        let sfnt = Sfnt::parse(&data).expect("the header is valid");
        assert!(sfnt.table(0x6865_6164).is_none());
    }
}
