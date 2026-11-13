//! TrueType and OpenType font programs: the tables metrics need.
//!
//! What text extraction and layout require — the table directory, `head`,
//! `hhea`, `hmtx`, `cmap` and `post` — and the vertical metrics a column is
//! shaped with: `vhea`, `vmtx` and `VORG`. Outlines are [`crate::glyf`]'s and
//! [`crate::cff`]'s; the one place this reaches either is a glyph's vertical
//! origin, which the `vmtx` clause defines against the top of the glyph's
//! bounding box ([`Sfnt::vertical_origin`]).
//!
//! Beside the one `cmap` subtable text is looked up in, two more are read on
//! their own for the route `docs/design/symbol-faces.md` settles: a Unicode
//! subtable, asked for a glyph name's value, and a (3,0) symbol subtable, asked
//! for a code.
//!
//! What the font measurement API answers from (`docs/design/font-metrics.md`)
//! is read here too: every scalar value the text subtable maps
//! ([`Sfnt::char_map`]), a glyph's `post` name ([`Sfnt::glyph_name`]), a
//! glyph's and the face's boxes ([`Sfnt::glyph_box`], [`Sfnt::font_box`]),
//! the style facts of `OS/2`, `post` and `head` ([`Sfnt::style`]), and a
//! collection's other faces ([`Sfnt::parse_face`]).
//!
//! Every read is bounds-checked against the buffer the caller supplied,
//! because a font program is document-controlled data like any other.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use crate::cff::Cff;
use crate::glyf::Bounds;

/// A parsed table directory over borrowed bytes.
///
/// What it keeps beside the bytes holds no borrow of them — the `cmap`
/// subtables chosen and `VORG`'s order are numbers — so a face stays
/// covariant in its lifetime and a `&'a Sfnt<'b>` can be handed wherever a
/// shorter face is asked for. A parsed `CFF ` table borrows, so it is kept by
/// [`VerticalOrigins`], the view a caller reading many origins makes once,
/// and never here.
#[derive(Clone, Debug)]
pub struct Sfnt<'a> {
    /// The `cmap` subtable characters are looked up in, chosen at the first
    /// lookup and kept, so a run of lookups chooses once.
    cmap: OnceLock<Option<CmapChoice>>,
    /// The Unicode subtable [`Sfnt::glyph_for_unicode`] reads, chosen at its
    /// first lookup and kept.
    unicode_cmap: OnceLock<Option<CmapChoice>>,
    /// The (3,0) symbol subtable [`Sfnt::glyph_for_symbol_code`] reads, chosen
    /// at its first lookup and kept.
    symbol_cmap: OnceLock<Option<CmapChoice>>,
    /// Whether `VORG`'s records are in the ascending glyph order the clause
    /// requires, checked at the first lookup and kept: in order they are
    /// bisected, out of order scanned as written.
    vorg_sorted: OnceLock<bool>,
    /// Where each of a version 2.0 `post` table's own strings starts in the
    /// table and how long it is, read once at the first name asked for, as
    /// far as the table holds whole strings.
    post_strings: OnceLock<Vec<(usize, u8)>>,
    data: &'a [u8],
    tables: Vec<(u32, u32, u32)>,
    /// The `sfntVersion` of the directory these tables came from.
    ///
    /// **Of the directory, not of the file**, and for a collection those are
    /// different: the file begins `ttcf` and the face begins `0x00010000`.
    /// Anything rebuilding a single font out of these tables has to declare
    /// the second, and the only way to get the first was to read byte zero —
    /// which is what [`crate::subset`] used to do.
    version: u32,
    /// Units per em from `head`; 1000 when the table is missing or absurd.
    pub units_per_em: u16,
    /// Number of `hmtx` entries with their own advance, from `hhea`.
    num_h_metrics: u16,
    /// Number of `vmtx` entries with their own advance height, from `vhea`'s
    /// `numOfLongVerMetrics`; `None` for a face without a `vhea` that holds
    /// the count.
    num_v_metrics: Option<u16>,
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let b = data.get(at..at + 2)?;
    Some(u16::from_be_bytes([b[0], b[1]]))
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let b = data.get(at..at + 4)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

const TAG_CMAP: u32 = 0x636D_6170;
const TAG_HEAD: u32 = 0x6865_6164;
const TAG_HHEA: u32 = 0x6868_6561;
const TAG_MAXP: u32 = 0x6D61_7870;
const TAG_POST: u32 = 0x706F_7374;
const TAG_VHEA: u32 = 0x7668_6561;
const TAG_VMTX: u32 = 0x766D_7478;
const TAG_VORG: u32 = 0x564F_5247;
const TAG_GLYF: u32 = 0x676C_7966;
const TAG_CFF: u32 = 0x4346_4620;
const TAG_OS2: u32 = 0x4F53_2F32;
const TAG_LOCA: u32 = 0x6C6F_6361;

/// The highest Unicode scalar value.
const MAX_SCALAR: u32 = 0x10_FFFF;

/// The `cmap` subtable a face's characters are looked up in (9.6.6.4).
#[derive(Clone, Copy, Debug)]
struct CmapChoice {
    score: u32,
    platform: u16,
    /// The encoding record's encoding ID, beside its platform.
    encoding: u16,
    offset: usize,
    /// Whether the subtable's entries are in the order its format requires —
    /// format 4's end codes ascending, format 12's and 13's groups ascending,
    /// disjoint and each well formed — and every one of them readable. Then
    /// the entry a scan from the start stops at is the one a bisection finds,
    /// so a lookup bisects; a subtable out of order is scanned as written.
    ordered: bool,
}

impl<'a> Sfnt<'a> {
    /// Parses the table directory.
    ///
    /// Accepts the plain `sfnt` forms and a TrueType Collection, from which
    /// the first font is taken — a PDF embedding a collection means the first
    /// face, since it has no way to say otherwise.
    #[must_use]
    pub fn parse(data: &'a [u8]) -> Option<Sfnt<'a>> {
        Sfnt::parse_with(data, 0, &mut 0)
    }

    /// One face of a TrueType Collection by its index in the `ttcf` header,
    /// or a plain sfnt as face 0: the face [`Sfnt::parse`] reads is face 0.
    ///
    /// `None` for an index at or past [`Sfnt::faces`], whose header offsets
    /// are bounded by what the bytes hold, and wherever [`Sfnt::parse`]
    /// would answer `None` for the face's directory.
    #[must_use]
    pub fn parse_face(data: &'a [u8], index: u32) -> Option<Sfnt<'a>> {
        Sfnt::parse_with(data, index, &mut 0)
    }

    /// How many faces the bytes hold: a collection's `numFonts`, no more than
    /// the offsets its header has room for; one for a plain sfnt; none for
    /// anything else.
    #[must_use]
    pub fn faces(data: &[u8]) -> u32 {
        if data.get(..4) == Some(b"ttcf") {
            let Some(count) = be32(data, 8) else {
                return 0;
            };
            let room = data.len().saturating_sub(12) / 4;
            return count.min(u32::try_from(room).unwrap_or(u32::MAX));
        }
        match be32(data, 0) {
            Some(0x0001_0000 | 0x7472_7565 | 0x4F54_544F) => 1,
            _ => 0,
        }
    }

    /// [`Sfnt::parse`], and the steps it took
    /// (`docs/design/resource-exhaustion.md`, "The counters"): a table record
    /// read for each table the directory lists, and a record compared for
    /// each comparison the lookups of `head` and `hhea` made.
    #[must_use]
    pub fn parse_counted(data: &'a [u8]) -> (Option<Sfnt<'a>>, u64) {
        let mut steps = 0u64;
        let font = Sfnt::parse_with(data, 0, &mut steps);
        (font, steps)
    }

    fn parse_with(data: &'a [u8], face: u32, steps: &mut u64) -> Option<Sfnt<'a>> {
        let mut base = 0usize;
        if data.get(..4) == Some(b"ttcf") {
            // Face 0 is read wherever its offset is, whatever `numFonts`
            // says, as `parse` has always read a collection; a later face
            // only where the header counts it.
            if face != 0 && face >= Sfnt::faces(data) {
                return None;
            }
            let at = 12usize.checked_add(usize::try_from(face).ok()?.checked_mul(4)?)?;
            base = be32(data, at)? as usize;
        } else if face != 0 {
            return None;
        }

        let tag = be32(data, base)?;
        // 0x00010000 TrueType, 'true', 'OTTO' for CFF outlines.
        if !matches!(tag, 0x0001_0000 | 0x7472_7565 | 0x4F54_544F) {
            return None;
        }

        let count = be16(data, base + 4)? as usize;
        let mut tables = Vec::with_capacity(count.min(64));
        for i in 0..count.min(512) {
            *steps = steps.saturating_add(1);
            let at = base + 12 + i * 16;
            let (Some(tag), Some(off), Some(len)) =
                (be32(data, at), be32(data, at + 8), be32(data, at + 12))
            else {
                break;
            };
            tables.push((tag, off, len));
        }

        let mut sfnt = Sfnt {
            cmap: OnceLock::new(),
            unicode_cmap: OnceLock::new(),
            symbol_cmap: OnceLock::new(),
            vorg_sorted: OnceLock::new(),
            post_strings: OnceLock::new(),
            data,
            tables,
            version: tag,
            units_per_em: 1000,
            num_h_metrics: 0,
            num_v_metrics: None,
        };

        if let Some(head) = sfnt.table_with(TAG_HEAD, steps) {
            // A units-per-em of zero would divide by zero downstream.
            if let Some(upem) = be16(head, 18).filter(|v| *v >= 16) {
                sfnt.units_per_em = upem;
            }
        }
        if let Some(hhea) = sfnt.table_with(TAG_HHEA, steps) {
            sfnt.num_h_metrics = be16(hhea, 34).unwrap_or(0);
        }
        // `vhea` keeps its count where `hhea` keeps its own, at offset 34, in
        // both of its versions.
        if let Some(vhea) = sfnt.table_with(TAG_VHEA, steps) {
            sfnt.num_v_metrics = be16(vhea, 34);
        }

        Some(sfnt)
    }

    /// The `sfntVersion` of the directory the tables came from: `0x00010000`,
    /// `true` or `OTTO`, and never `ttcf`.
    ///
    /// [`Sfnt::parse`] refuses anything else, so this is one of exactly three
    /// values and is the right thing for a rebuilt single font to declare.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// One table's bytes, by tag.
    #[must_use]
    pub fn table(&self, tag: u32) -> Option<&'a [u8]> {
        self.table_with(tag, &mut 0)
    }

    /// The tags the directory lists, in its order, each once: a tag listed
    /// twice names the first record, as [`Sfnt::table`] reads it.
    #[must_use]
    pub fn tags(&self) -> Vec<[u8; 4]> {
        let mut out: Vec<[u8; 4]> = Vec::with_capacity(self.tables.len());
        for (tag, _, _) in &self.tables {
            let tag = tag.to_be_bytes();
            if !out.contains(&tag) {
                out.push(tag);
            }
        }
        out
    }

    /// [`Sfnt::table`], counting each record compared.
    fn table_with(&self, tag: u32, steps: &mut u64) -> Option<&'a [u8]> {
        let mut compared = 0u64;
        let found = self.tables.iter().find(|(t, _, _)| {
            compared += 1;
            *t == tag
        });
        *steps = steps.saturating_add(compared);
        let (_, off, len) = found?;
        self.data
            .get(*off as usize..(*off as usize).checked_add(*len as usize)?)
    }

    /// `maxp`'s `numGlyphs`, which sizes every per-glyph array a table keeps
    /// beside the outlines — `sbix`'s offsets among them — or `None` where the
    /// face has no `maxp` long enough to say.
    #[must_use]
    pub fn glyph_count(&self) -> Option<u16> {
        be16(self.table(TAG_MAXP)?, 4)
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

    /// A glyph's advance height, in font units, from `vmtx`.
    ///
    /// `vmtx` is `hmtx` stood on end: `vhea`'s `numOfLongVerMetrics` entries
    /// carry an advance height and a top side bearing each, and every glyph
    /// past them repeats the last advance, as a monospaced face's glyphs do.
    /// `None` for a face without a `vhea` or a `vmtx`, a count of zero, or an
    /// entry past the end of the table: a `vmtx` shorter than `vhea` claims is
    /// read as far as it goes and no further.
    #[must_use]
    pub fn vertical_advance(&self, glyph: u16) -> Option<u16> {
        let count = self.num_v_metrics.filter(|count| *count > 0)?;
        let vmtx = self.table(TAG_VMTX)?;
        let index = glyph.min(count - 1);
        be16(vmtx, usize::from(index) * 4)
    }

    /// A glyph's top side bearing, in font units, from `vmtx`: the distance
    /// from its vertical origin down to the top of its bounding box.
    ///
    /// A glyph inside the long metrics reads its own; one past them reads the
    /// array of side bearings after them, which the `vmtx` clause sizes to the
    /// face's glyph count, so a glyph at or past `maxp`'s count has none.
    /// `None` wherever [`Sfnt::vertical_advance`] is, and for a read past the
    /// end of the table.
    #[must_use]
    pub fn top_side_bearing(&self, glyph: u16) -> Option<i16> {
        let count = self.num_v_metrics.filter(|count| *count > 0)?;
        let vmtx = self.table(TAG_VMTX)?;
        let at = if glyph < count {
            usize::from(glyph) * 4 + 2
        } else {
            if self.glyph_count().is_some_and(|glyphs| glyph >= glyphs) {
                return None;
            }
            usize::from(count) * 4 + usize::from(glyph - count) * 2
        };
        be16(vmtx, at).map(|bearing| bearing as i16)
    }

    /// `VORG`'s vertical origin for a glyph: its record, or the table's
    /// default for a glyph it has none for.
    ///
    /// As the table states it, whatever outlines the face has; which of the
    /// two a column takes is [`Sfnt::vertical_origin`]'s question. `None` for
    /// a face without a `VORG`, or one whose major version is not 1, the only
    /// one written down. The record count is bounded by what the table holds,
    /// and the records are bisected where they are in the ascending order the
    /// clause requires and scanned as written where they are not, the first
    /// match winning, as an out-of-order `cmap` subtable is.
    #[must_use]
    pub fn vorg_origin(&self, glyph: u16) -> Option<i16> {
        let vorg = self.table(TAG_VORG)?;
        if be16(vorg, 0)? != 1 {
            return None;
        }
        let default = be16(vorg, 4)? as i16;
        let count = usize::from(be16(vorg, 6)?).min(vorg.len().saturating_sub(8) / 4);
        let record = |n: usize| -> Option<(u16, i16)> {
            let at = 8 + n * 4;
            Some((be16(vorg, at)?, be16(vorg, at + 2)? as i16))
        };
        let sorted = *self.vorg_sorted.get_or_init(|| {
            (1..count).all(|n| match (record(n - 1), record(n)) {
                (Some((before, _)), Some((after, _))) => before < after,
                _ => false,
            })
        });
        let found = if sorted {
            let (mut low, mut high) = (0usize, count);
            let mut hit = None;
            while low < high {
                let middle = low + (high - low) / 2;
                let Some((at, origin)) = record(middle) else {
                    break;
                };
                match at.cmp(&glyph) {
                    core::cmp::Ordering::Less => low = middle + 1,
                    core::cmp::Ordering::Greater => high = middle,
                    core::cmp::Ordering::Equal => {
                        hit = Some(origin);
                        break;
                    }
                }
            }
            hit
        } else {
            (0..count).find_map(|n| {
                record(n)
                    .filter(|(at, _)| *at == glyph)
                    .map(|(_, origin)| origin)
            })
        };
        Some(found.unwrap_or(default))
    }

    /// The y of a glyph's vertical origin, in font units: where a column's
    /// pen stands above the glyph.
    ///
    /// [`VerticalOrigins::origin`] through a view made for this one glyph. A
    /// caller asking for many glyphs makes the view once
    /// ([`Sfnt::vertical_origins`]), because for a CFF face without `VORG` the
    /// view is where the `CFF ` table is parsed.
    #[must_use]
    pub fn vertical_origin(&self, glyph: u16) -> Option<i32> {
        self.vertical_origins().origin(glyph)
    }

    /// The face's vertical origins, for as many glyphs as a caller asks.
    ///
    /// Which source the face's origins come from is decided here, once: its
    /// `glyf` headers for TrueType outlines, its `VORG` for CFF outlines that
    /// carry one, and otherwise its `CFF ` table, parsed here and kept by the
    /// view so a column of glyphs parses it once.
    #[must_use]
    pub fn vertical_origins(&self) -> VerticalOrigins<'_, 'a> {
        let source = if self.table(TAG_GLYF).is_some() {
            OriginSource::Glyf
        } else if self.vorg_origin(0).is_some() {
            OriginSource::Vorg
        } else {
            OriginSource::Cff(self.table(TAG_CFF).and_then(Cff::parse))
        };
        VerticalOrigins { face: self, source }
    }

    /// A glyph's left side bearing from `hmtx`, in font units: the second
    /// half of its long metric, or, for a glyph past `numberOfHMetrics`, its
    /// entry of the `leftSideBearings` array after them, which the `hmtx`
    /// clause sizes to the face's glyph count, so a glyph at or past `maxp`'s
    /// count has none — as [`Sfnt::top_side_bearing`] reads `vmtx`. The
    /// default an `HVAR` side-bearing delta applies to.
    #[must_use]
    pub fn left_side_bearing(&self, glyph: u16) -> Option<i16> {
        let hmtx = self.table(0x686D_7478)?;
        let long = self.num_h_metrics;
        if long == 0 {
            return None;
        }
        let at = if glyph < long {
            usize::from(glyph) * 4 + 2
        } else {
            if self.glyph_count().is_some_and(|glyphs| glyph >= glyphs) {
                return None;
            }
            usize::from(long) * 4 + usize::from(glyph - long) * 2
        };
        be16(hmtx, at).map(|value| value as i16)
    }

    /// A character's glyph index, through the best `cmap` subtable available.
    #[must_use]
    pub fn glyph_for_char(&self, c: char) -> Option<u16> {
        self.glyph_for_char_with(c, &mut 0)
    }

    /// [`Sfnt::glyph_for_char`], and the steps it took: an encoding record
    /// read for each one the first lookup weighs, an entry for each one it
    /// checks the subtable's order with, and an entry for each one this
    /// lookup examined — one for each step of a bisection, or of a scan
    /// through a subtable out of order.
    #[must_use]
    pub fn glyph_for_char_counted(&self, c: char) -> (Option<u16>, u64) {
        let mut steps = 0u64;
        let glyph = self.glyph_for_char_with(c, &mut steps);
        (glyph, steps)
    }

    fn glyph_for_char_with(&self, c: char, steps: &mut u64) -> Option<u16> {
        let mut chosen = 0u64;
        let choice = *self.cmap.get_or_init(|| self.choose_cmap(&mut chosen));
        *steps = steps.saturating_add(chosen);
        let CmapChoice {
            score,
            platform,
            offset,
            ordered,
            ..
        } = choice?;
        let sub = self.table_with(TAG_CMAP, steps)?.get(offset..)?;
        let mut code = u32::from(c);
        // A (3,0) symbol subtable maps the F0xx private-use block, and
        // documents address it with the low byte (9.6.6.4).
        if score == 2 && code < 0x100 {
            code |= 0xF000;
        }
        // **A Macintosh subtable is not indexed by Unicode.** Platform 1 is the
        // classic Mac OS, and its subtables are byte maps in one of the legacy
        // Mac OS encodings — which one is named by the subtable's `language`
        // field, one higher than the QuickDraw language code. Below U+0080
        // every one of those encodings agrees with ASCII and with Unicode, so
        // the byte *is* the scalar value; above it they do not, and handing the
        // scalar to the byte map returns a glyph that is wrong rather than
        // absent.
        //
        // `None` here rather than a wrong glyph, because a caller that gets
        // nothing can fall back and a caller that gets the wrong glyph cannot
        // tell. What would lift it is the vendored conversion table for the
        // encoding the language field names, and `docs/features/fonts.md`
        // records why this repository does not carry one.
        if platform == 1 && code >= 0x80 {
            return None;
        }
        lookup_cmap(sub, code, ordered, steps)
    }

    /// Which subtable a character is looked up in, and whether it is in its
    /// format's order: what [`Sfnt::glyph_for_char`] asks once and keeps.
    fn choose_cmap(&self, steps: &mut u64) -> Option<CmapChoice> {
        let cmap = self.table_with(TAG_CMAP, steps)?;
        let count = be16(cmap, 2)? as usize;

        // 9.6.6.4 prefers (3,1) Windows Unicode BMP, then (3,0) symbol, then
        // (1,0) Macintosh Roman. (3,10) covers characters beyond the BMP.
        let mut best: Option<(u32, u16, u16, usize)> = None;
        for i in 0..count.min(64) {
            *steps = steps.saturating_add(1);
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
            if score > 0 && best.is_none_or(|(b, _, _, _)| score > b) {
                best = Some((score, platform, encoding, offset as usize));
            }
        }

        let (score, platform, encoding, offset) = best?;
        let ordered = cmap.get(offset..).is_some_and(|sub| in_order(sub, steps));
        Some(CmapChoice {
            score,
            platform,
            encoding,
            offset,
            ordered,
        })
    }

    /// The platform and encoding of the subtable [`Sfnt::glyph_for_char`]
    /// looks characters up in, the one [`Sfnt::char_map`] reads: (3,10),
    /// else (3,1), else a platform 0 one, else (3,0), else (1,0), the first of
    /// equals. `None` for a face with no `cmap` or none of those.
    #[must_use]
    pub fn text_subtable(&self) -> Option<(u16, u16)> {
        let choice = (*self.cmap.get_or_init(|| self.choose_cmap(&mut 0)))?;
        Some((choice.platform, choice.encoding))
    }

    /// Every scalar value [`Sfnt::glyph_for_char`] answers with a glyph
    /// other than glyph 0, and that glyph, in ascending order of the scalar.
    ///
    /// The text subtable is read exactly as that lookup reads it: a (3,0)
    /// subtable answers a scalar below U+0100 at `0xF000` plus it, and its
    /// own codes from U+0100 up as themselves, so its `F0xx` codes answer two
    /// scalars each and its codes below U+0100 none; a Macintosh subtable
    /// answers no scalar past U+007F; a surrogate and a code past U+10FFFF are
    /// no scalar. In each format the lookup reads — 0, 2, 4, 6, 12 and 13 —
    /// the code a subtable maps is the one the lookup finds for it, out of
    /// order as written: a format 4 subtable's segments are read once in
    /// order, the first whose end is at or past a code deciding it, and a
    /// format 12 or 13 subtable's groups once in order, the first that holds a
    /// code deciding it, against the codes earlier groups decided — so the
    /// walk is linear in the subtable and the codes it maps, and the answer at
    /// most one entry a scalar value. Read afresh at each call: a caller
    /// asking often keeps it.
    #[must_use]
    pub fn char_map(&self) -> Vec<(char, u16)> {
        let Some(choice) = *self.cmap.get_or_init(|| self.choose_cmap(&mut 0)) else {
            return Vec::new();
        };
        let Some(sub) = self
            .table(TAG_CMAP)
            .and_then(|cmap| cmap.get(choice.offset..))
        else {
            return Vec::new();
        };
        let mut codes = subtable_codes(sub, choice.ordered);
        codes.sort_unstable_by_key(|(code, _)| *code);
        let mut out = Vec::with_capacity(codes.len());
        for (code, glyph) in codes {
            if choice.score == 2 {
                // 9.6.6.4: a symbol subtable's `F0xx` codes are addressed by
                // their low byte as well as by themselves.
                if (0xF000..=0xF0FF).contains(&code) {
                    if let Some(c) = char::from_u32(code - 0xF000) {
                        out.push((c, glyph));
                    }
                }
                if code < 0x100 {
                    continue;
                }
            }
            if choice.platform == 1 && code >= 0x80 {
                continue;
            }
            if let Some(c) = char::from_u32(code) {
                out.push((c, glyph));
            }
        }
        out.sort_unstable_by_key(|(c, _)| *c);
        out
    }

    /// The scalar values of [`Sfnt::char_map`], ascending.
    #[must_use]
    pub fn code_points(&self) -> Vec<char> {
        self.char_map().into_iter().map(|(c, _)| c).collect()
    }

    /// The glyph named `name`, through the `post` table's names.
    ///
    /// Version 1.0 is the 258 standard Macintosh names in their order, one a
    /// glyph; version 2.0 gives each glyph an index, one of those 258 below 258
    /// and the table's own strings from 258 on; version 2.5 gives each glyph an
    /// offset into the standard order; version 3.0 and any other name nothing.
    /// The first glyph with the name is the answer, `.notdef` included, which
    /// is how 2.0 marks a glyph it does not name.
    #[must_use]
    pub fn glyph_for_name(&self, name: &str) -> Option<u16> {
        let post = self.table(TAG_POST)?;
        let standard = MAC_STANDARD_NAMES.iter().position(|listed| *listed == name);
        match be32(post, 0)? {
            0x0001_0000 => {
                let index = standard?;
                // Version 1.0 says the face holds exactly the standard order,
                // so a name past the face's own glyphs is no glyph of it.
                let glyphs = self
                    .table(TAG_MAXP)
                    .and_then(|maxp| be16(maxp, 4))
                    .map_or(MAC_STANDARD_NAMES.len(), usize::from);
                if index >= glyphs {
                    return None;
                }
                u16::try_from(index).ok()
            }
            0x0002_0000 => {
                let count = usize::from(be16(post, 32)?);
                let strings = 34usize.checked_add(count.checked_mul(2)?)?;
                // Whether each of the table's own strings is `name`, read once,
                // so each glyph's index is answered in one step however often
                // a hostile table repeats the string.
                let mut spells = Vec::new();
                let mut at = strings;
                while let Some(&len) = post.get(at) {
                    let start = at + 1;
                    let Some(text) = post.get(start..start + usize::from(len)) else {
                        break;
                    };
                    spells.push(text == name.as_bytes());
                    at = start + usize::from(len);
                }
                (0..count).find_map(|glyph| {
                    let index = usize::from(be16(post, 34 + glyph * 2)?);
                    let named = match index.checked_sub(MAC_STANDARD_NAMES.len()) {
                        None => standard == Some(index),
                        Some(ordinal) => spells.get(ordinal).copied().unwrap_or(false),
                    };
                    if !named {
                        return None;
                    }
                    u16::try_from(glyph).ok()
                })
            }
            0x0002_5000 => {
                let index = i64::try_from(standard?).ok()?;
                let count = usize::from(be16(post, 32)?);
                (0..count).find_map(|glyph| {
                    // An offset is a signed byte: the glyph's place in the
                    // standard order less its own index.
                    let offset = i64::from(i8::from_be_bytes([*post.get(34 + glyph)?]));
                    let glyph = i64::try_from(glyph).ok()?;
                    if glyph + offset != index {
                        return None;
                    }
                    u16::try_from(glyph).ok()
                })
            }
            _ => None,
        }
    }

    /// Every glyph's name as `post` gives it, one a glyph of `maxp`'s count:
    /// version 1.0's standard Macintosh order, or version 2.0's indices into
    /// that order and into the table's own strings. `None` for any other
    /// version, a table cut short, an index naming a string the table does
    /// not hold, or a name that is not ASCII — a face whose glyphs are not
    /// all named, which is how a reader choosing between names and CIDs
    /// needs to know it.
    #[must_use]
    pub fn post_names(&self) -> Option<Vec<&'a str>> {
        let post = self.table(TAG_POST)?;
        let glyphs = usize::from(self.glyph_count()?);
        match be32(post, 0)? {
            0x0001_0000 => {
                (glyphs <= MAC_STANDARD_NAMES.len()).then(|| MAC_STANDARD_NAMES[..glyphs].to_vec())
            }
            0x0002_0000 => {
                let count = usize::from(be16(post, 32)?);
                if count < glyphs {
                    return None;
                }
                let strings = 34usize.checked_add(count.checked_mul(2)?)?;
                let mut own: Vec<&'a str> = Vec::new();
                let mut at = strings;
                while let Some(&len) = post.get(at) {
                    let text = post.get(at + 1..at + 1 + usize::from(len))?;
                    if !text.is_ascii() {
                        return None;
                    }
                    own.push(core::str::from_utf8(text).ok()?);
                    at += 1 + usize::from(len);
                }
                (0..glyphs)
                    .map(|glyph| {
                        let index = usize::from(be16(post, 34 + glyph * 2)?);
                        match index.checked_sub(MAC_STANDARD_NAMES.len()) {
                            None => MAC_STANDARD_NAMES.get(index).copied(),
                            Some(ordinal) => own.get(ordinal).copied(),
                        }
                    })
                    .collect()
            }
            _ => None,
        }
    }

    /// A character's glyph through the face's Unicode subtable alone: (3,10),
    /// else (3,1), else a platform 0 subtable of encoding 0 to 4 or 6, chosen
    /// once and kept. Encoding 5 maps variation sequences rather than
    /// characters, and the (3,0) and Macintosh subtables are not keyed by
    /// Unicode, so none of them is asked here.
    ///
    /// `None` where the face has no such subtable or it maps the character to
    /// glyph 0, which is `.notdef` and no answer
    /// (`docs/design/symbol-faces.md`, "The route").
    #[must_use]
    pub fn glyph_for_unicode(&self, c: char) -> Option<u16> {
        let choice = (*self
            .unicode_cmap
            .get_or_init(|| self.choose_subtable(unicode_score)))?;
        let sub = self.table(TAG_CMAP)?.get(choice.offset..)?;
        lookup_cmap(sub, u32::from(c), choice.ordered, &mut 0).filter(|glyph| *glyph != 0)
    }

    /// A code's glyph through the face's (3,0) symbol subtable alone, at
    /// `0xF000` plus the code (9.6.6.4), chosen once and kept: the reading a
    /// substituted symbolic face gets today, which the route keeps last.
    ///
    /// `None` where the face has no (3,0) subtable or it maps the code to
    /// glyph 0.
    #[must_use]
    pub fn glyph_for_symbol_code(&self, code: u8) -> Option<u16> {
        let choice = (*self
            .symbol_cmap
            .get_or_init(|| self.choose_subtable(symbol_score)))?;
        let sub = self.table(TAG_CMAP)?.get(choice.offset..)?;
        lookup_cmap(sub, 0xF000 | u32::from(code), choice.ordered, &mut 0)
            .filter(|glyph| *glyph != 0)
    }

    /// The best-scored subtable among the face's first 64 encoding records,
    /// where `score` gives each pair of platform and encoding a score and zero
    /// to one it does not take; the first of equal scores wins.
    fn choose_subtable(&self, score: fn(u16, u16) -> u32) -> Option<CmapChoice> {
        let cmap = self.table(TAG_CMAP)?;
        let count = usize::from(be16(cmap, 2)?);
        let mut best: Option<(u32, u16, u16, usize)> = None;
        for i in 0..count.min(64) {
            let at = 4 + i * 8;
            let (Some(platform), Some(encoding), Some(offset)) =
                (be16(cmap, at), be16(cmap, at + 2), be32(cmap, at + 4))
            else {
                break;
            };
            let scored = score(platform, encoding);
            if scored > 0 && best.is_none_or(|(b, _, _, _)| scored > b) {
                best = Some((scored, platform, encoding, offset as usize));
            }
        }
        let (score, platform, encoding, offset) = best?;
        let ordered = cmap.get(offset..).is_some_and(|sub| in_order(sub, &mut 0));
        Some(CmapChoice {
            score,
            platform,
            encoding,
            offset,
            ordered,
        })
    }

    /// A glyph's name from `post`, the reverse of [`Sfnt::glyph_for_name`].
    ///
    /// Version 1.0 is the 258 standard Macintosh names in their order, one a
    /// glyph, as far as `maxp`'s count reaches; version 2.0 gives each of its
    /// glyphs an index, one of those 258 below 258 and the table's own Pascal
    /// strings from 258 on, an index whose string the table does not hold
    /// whole naming nothing; version 2.5 gives each glyph a signed offset
    /// from its own index into the standard order; version 3.0 and any other
    /// name nothing. A string that is not UTF-8 is no name. Glyph names are
    /// ASCII in every face that follows the `post` chapter, which asks for
    /// PostScript names.
    #[must_use]
    pub fn glyph_name(&self, glyph: u16) -> Option<&'a str> {
        let post = self.table(TAG_POST)?;
        let index = usize::from(glyph);
        match be32(post, 0)? {
            0x0001_0000 => {
                let glyphs = self
                    .glyph_count()
                    .map_or(MAC_STANDARD_NAMES.len(), usize::from);
                if index >= glyphs {
                    return None;
                }
                MAC_STANDARD_NAMES.get(index).copied()
            }
            0x0002_0000 => {
                let count = usize::from(be16(post, 32)?);
                if index >= count {
                    return None;
                }
                let name = usize::from(be16(post, 34 + index * 2)?);
                match name.checked_sub(MAC_STANDARD_NAMES.len()) {
                    None => MAC_STANDARD_NAMES.get(name).copied(),
                    Some(ordinal) => {
                        let strings = self.post_strings.get_or_init(|| post_strings(post, count));
                        let &(start, len) = strings.get(ordinal)?;
                        let bytes = post.get(start..start + usize::from(len))?;
                        core::str::from_utf8(bytes).ok()
                    }
                }
            }
            0x0002_5000 => {
                let count = usize::from(be16(post, 32)?);
                if index >= count {
                    return None;
                }
                let offset = i64::from(i8::from_be_bytes([*post.get(34 + index)?]));
                let standard = usize::try_from(i64::from(glyph) + offset).ok()?;
                MAC_STANDARD_NAMES.get(standard).copied()
            }
            _ => None,
        }
    }

    /// A glyph's box as its `glyf` header states it, read through `loca`:
    /// [`crate::glyf::bounds`], the origin point for a glyph with no outline,
    /// and `None` for a face with no `glyf`, `loca` or `head`, a glyph past
    /// the end of `loca`, and data too short to hold the header.
    #[must_use]
    pub fn glyph_box(&self, glyph: u16) -> Option<Bounds> {
        crate::glyf::bounds(self, glyph)
    }

    /// How many glyphs `loca` indexes: its entries less one, two bytes each
    /// in the short form `head`'s `indexToLocFormat` of 0 names and four in
    /// the long, held to 65 535, the most a glyph index addresses. `None` for
    /// a face with no `loca`, or no `head` to say which form it is in. What
    /// a face with no `maxp` that states its count holds.
    #[must_use]
    pub fn loca_glyph_count(&self) -> Option<u16> {
        let head = self.table(TAG_HEAD)?;
        let long = be16(head, 50)? != 0;
        let loca = self.table(TAG_LOCA)?;
        let entries = loca.len() / if long { 4 } else { 2 };
        Some(u16::try_from(entries.saturating_sub(1)).unwrap_or(u16::MAX))
    }

    /// The face's box, `head`'s `xMin`, `yMin`, `xMax` and `yMax`, as
    /// written; `None` for a face with no `head` long enough to hold them.
    #[must_use]
    pub fn font_box(&self) -> Option<Bounds> {
        let head = self.table(TAG_HEAD)?;
        Some(Bounds {
            x_min: be16(head, 36)? as i16,
            y_min: be16(head, 38)? as i16,
            x_max: be16(head, 40)? as i16,
            y_max: be16(head, 42)? as i16,
        })
    }

    /// The style facts of `post`, `OS/2` and `head` the font flags read
    /// (`docs/design/font-metrics.md`, *Flags*), each as its table writes it
    /// and `None` where the table is absent or too short to hold it.
    #[must_use]
    pub fn style(&self) -> Style {
        let post = self.table(TAG_POST);
        let os2 = self.table(TAG_OS2);
        let head = self.table(TAG_HEAD);
        let os2_u16 = |at: usize| os2.and_then(|t| be16(t, at));
        // sxHeight and sCapHeight are version 2's.
        let os2_v2 = os2_u16(0).filter(|version| *version >= 2).is_some();
        Style {
            italic_angle: post
                .and_then(|t| be32(t, 4))
                .map(|fixed| f64::from(fixed as i32) / 65_536.0),
            underline_position: post.and_then(|t| be16(t, 8)).map(|v| v as i16),
            underline_thickness: post.and_then(|t| be16(t, 10)).map(|v| v as i16),
            fixed_pitch: post.and_then(|t| be32(t, 12)).map(|v| v != 0),
            weight_class: os2_u16(4),
            width_class: os2_u16(6),
            family_class: os2_u16(30).map(|v| v as i16),
            panose: os2
                .and_then(|t| t.get(32..42))
                .and_then(|b| <[u8; 10]>::try_from(b).ok()),
            selection: os2_u16(62),
            x_height: if os2_v2 {
                os2_u16(86).map(|v| v as i16)
            } else {
                None
            },
            cap_height: if os2_v2 {
                os2_u16(88).map(|v| v as i16)
            } else {
                None
            },
            mac_style: head.and_then(|t| be16(t, 44)),
        }
    }
}

/// The style facts of an sfnt's `post`, `OS/2` and `head` tables
/// ([`Sfnt::style`]): what the font flags and the font measurement API read
/// a face's slant, pitch, weight, class and heights from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Style {
    /// `post`'s `italicAngle`, its 16.16 `Fixed` in degrees counterclockwise
    /// from the vertical.
    pub italic_angle: Option<f64>,
    /// `post`'s `underlinePosition`, in font units.
    pub underline_position: Option<i16>,
    /// `post`'s `underlineThickness`, in font units.
    pub underline_thickness: Option<i16>,
    /// Whether `post`'s `isFixedPitch` is not zero.
    pub fixed_pitch: Option<bool>,
    /// `OS/2`'s `usWeightClass`.
    pub weight_class: Option<u16>,
    /// `OS/2`'s `usWidthClass`.
    pub width_class: Option<u16>,
    /// `OS/2`'s `sFamilyClass`: the IBM font class in its high byte, the
    /// subclass in its low.
    pub family_class: Option<i16>,
    /// `OS/2`'s ten PANOSE digits, family kind first.
    pub panose: Option<[u8; 10]>,
    /// `OS/2`'s `fsSelection`: bit 0 `ITALIC`, bit 5 `BOLD`, bit 6
    /// `REGULAR`, bit 9 `OBLIQUE`.
    pub selection: Option<u16>,
    /// `OS/2`'s `sxHeight`, which version 2 added: `None` in an earlier one.
    pub x_height: Option<i16>,
    /// `OS/2`'s `sCapHeight`, which version 2 added.
    pub cap_height: Option<i16>,
    /// `head`'s `macStyle`: bit 0 bold, bit 1 italic.
    pub mac_style: Option<u16>,
}

/// Where each of a version 2.0 `post` table's own Pascal strings starts and
/// how long it is, after the `count` glyph indices: as many as the table
/// holds whole, a string cut short ending the list.
fn post_strings(post: &[u8], count: usize) -> Vec<(usize, u8)> {
    let mut out = Vec::new();
    let Some(mut at) = count.checked_mul(2).and_then(|n| n.checked_add(34)) else {
        return out;
    };
    while let Some(&len) = post.get(at) {
        let start = at + 1;
        if post.get(start..start + usize::from(len)).is_none() {
            break;
        }
        out.push((start, len));
        at = start + usize::from(len);
    }
    out
}

/// Every code a `cmap` subtable maps to a glyph other than glyph 0, and the
/// glyph, as [`lookup_cmap`] reads the subtable: the same answer it gives a
/// lookup of each code, in an order of its own.
fn subtable_codes(sub: &[u8], ordered: bool) -> Vec<(u32, u16)> {
    let mut out = Vec::new();
    let mut each = |code: u32| {
        if let Some(glyph) = lookup_cmap(sub, code, ordered, &mut 0).filter(|g| *g != 0) {
            out.push((code, glyph));
        }
    };
    match be16(sub, 0) {
        // One byte, or one byte or two: every code is asked, each in one step.
        Some(0) => (0..=0xFF).for_each(&mut each),
        Some(2) => (0..=0xFFFF).for_each(&mut each),
        // A trimmed range: the codes it counts, which may pass 0xFFFF, as the
        // lookup reads them.
        Some(6) => {
            let (Some(first), Some(count)) = (be16(sub, 6), be16(sub, 8)) else {
                return out;
            };
            let first = u32::from(first);
            (first..first + u32::from(count)).for_each(&mut each);
        }
        Some(4) => return format_4_codes(sub),
        Some(format @ (12 | 13)) => return group_codes(sub, format),
        _ => {}
    }
    out
}

/// A format 4 subtable's codes and glyphs, its segments read once in order.
///
/// A lookup of a code stops at the first segment whose end is at or past the
/// code — by a scan from the start, or by a bisection where the ends ascend,
/// which finds the same segment — and answers glyph 0 where the segment
/// starts past it. So each segment decides the codes past every end before
/// it and up to its own, and none twice. A segment whose fields the
/// subtable does not hold decides its codes as no glyph, as the lookup does,
/// and an end it does not hold ends the subtable for every code after.
fn format_4_codes(sub: &[u8]) -> Vec<(u32, u16)> {
    let mut out = Vec::new();
    let Some(seg2) = be16(sub, 6) else {
        return out;
    };
    let seg2 = usize::from(seg2);
    let segs = seg2 / 2;
    let ends = 14;
    let starts = ends + seg2 + 2;
    let deltas = starts + seg2;
    let ranges = deltas + seg2;
    // Every code below `next` belongs to an earlier segment.
    let mut next = 0u32;
    for i in 0..segs {
        let Some(end) = be16(sub, ends + i * 2) else {
            break;
        };
        let end = u32::from(end);
        if end < next {
            continue;
        }
        let first = next;
        next = end + 1;
        let (Some(start), Some(delta), Some(range_offset)) = (
            be16(sub, starts + i * 2),
            be16(sub, deltas + i * 2),
            be16(sub, ranges + i * 2),
        ) else {
            continue;
        };
        let start = u32::from(start);
        for code in first.max(start)..=end {
            // `code` is at most 0xFFFF: an end is a 16-bit value.
            let code16 = code as u16;
            let glyph = if range_offset == 0 {
                code16.wrapping_add(delta)
            } else {
                let at = ranges
                    + i * 2
                    + usize::from(range_offset)
                    + usize::from(code16 - start as u16) * 2;
                match be16(sub, at) {
                    Some(0) | None => 0,
                    Some(glyph) => glyph.wrapping_add(delta),
                }
            };
            if glyph != 0 {
                out.push((code, glyph));
            }
        }
    }
    out
}

/// A format 12 or 13 subtable's codes and glyphs, its groups read once in
/// order.
///
/// A lookup of a code answers from the first group that holds it — by a
/// scan as written, or by a bisection where the groups ascend and are
/// disjoint, which finds the same group — whatever that group answers, and
/// stops at the first group the subtable does not hold whole. So each group
/// decides the codes in its range no earlier group holds: the ranges decided
/// so far are kept merged, and only a group's codes outside them are read.
/// Codes past U+10FFFF are no scalar value and are not read.
fn group_codes(sub: &[u8], format: u16) -> Vec<(u32, u16)> {
    let mut out = Vec::new();
    let Some(groups) = be32(sub, 12) else {
        return out;
    };
    let groups = (groups as usize).min(1 << 20);
    let mut decided: BTreeMap<u32, u32> = BTreeMap::new();
    for i in 0..groups {
        let at = 16 + i * 12;
        let (Some(start), Some(end), Some(glyph)) =
            (be32(sub, at), be32(sub, at + 4), be32(sub, at + 8))
        else {
            break;
        };
        if start > end || start > MAX_SCALAR {
            continue;
        }
        let end = end.min(MAX_SCALAR);
        claim(&mut decided, start, end, |from, to| {
            for code in from..=to {
                let id = if format == 12 {
                    glyph
                        .checked_add(code - start)
                        .and_then(|id| u16::try_from(id).ok())
                } else {
                    u16::try_from(glyph).ok()
                };
                if let Some(id) = id.filter(|id| *id != 0) {
                    out.push((code, id));
                }
            }
        });
    }
    out
}

/// Calls `each` with every run of `start..=end` no range in `decided` holds,
/// ascending, then adds the range to `decided`, merged with every range it
/// meets or touches. Each range of `decided` is removed at most once, so a
/// walk of `n` ranges is `n log n` however they overlap.
fn claim(decided: &mut BTreeMap<u32, u32>, start: u32, end: u32, mut each: impl FnMut(u32, u32)) {
    let mut cursor = start;
    let mut first = start;
    let mut last = end;
    let mut absorbed = Vec::new();
    if let Some((&at, &to)) = decided.range(..start).next_back() {
        if to >= start {
            cursor = to.saturating_add(1);
            first = at;
            last = last.max(to);
            absorbed.push(at);
        } else if to.checked_add(1) == Some(start) {
            first = at;
            absorbed.push(at);
        }
    }
    for (&at, &to) in decided.range(start..=end.saturating_add(1)) {
        if at > end {
            // Touching the range's end: merged, holding nothing of it.
            last = last.max(to);
            absorbed.push(at);
            continue;
        }
        if cursor < at {
            each(cursor, at - 1);
        }
        cursor = cursor.max(to.saturating_add(1));
        last = last.max(to);
        absorbed.push(at);
    }
    if cursor <= end {
        each(cursor, end);
    }
    for at in absorbed {
        decided.remove(&at);
    }
    decided.insert(first, last);
}

/// How [`Sfnt::glyph_for_unicode`] scores a subtable: (3,10) over (3,1) over a
/// platform 0 subtable that maps characters, and nothing else.
fn unicode_score(platform: u16, encoding: u16) -> u32 {
    match (platform, encoding) {
        (3, 10) => 3,
        (3, 1) => 2,
        (0, 0..=4 | 6) => 1,
        _ => 0,
    }
}

/// How [`Sfnt::glyph_for_symbol_code`] scores a subtable: (3,0) alone.
fn symbol_score(platform: u16, encoding: u16) -> u32 {
    u32::from(platform == 3 && encoding == 0)
}

/// The 258 glyph names of the standard Macintosh order, by index, as Apple's
/// TrueType Reference Manual gives them in its `post` chapter: what a version
/// 1.0 table means by each glyph and a version 2.0 or 2.5 table by an index
/// below 258.
const MAC_STANDARD_NAMES: [&str; 258] = [
    ".notdef",
    ".null",
    "nonmarkingreturn",
    "space",
    "exclam",
    "quotedbl",
    "numbersign",
    "dollar",
    "percent",
    "ampersand",
    "quotesingle",
    "parenleft",
    "parenright",
    "asterisk",
    "plus",
    "comma",
    "hyphen",
    "period",
    "slash",
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "colon",
    "semicolon",
    "less",
    "equal",
    "greater",
    "question",
    "at",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "bracketleft",
    "backslash",
    "bracketright",
    "asciicircum",
    "underscore",
    "grave",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "braceleft",
    "bar",
    "braceright",
    "asciitilde",
    "Adieresis",
    "Aring",
    "Ccedilla",
    "Eacute",
    "Ntilde",
    "Odieresis",
    "Udieresis",
    "aacute",
    "agrave",
    "acircumflex",
    "adieresis",
    "atilde",
    "aring",
    "ccedilla",
    "eacute",
    "egrave",
    "ecircumflex",
    "edieresis",
    "iacute",
    "igrave",
    "icircumflex",
    "idieresis",
    "ntilde",
    "oacute",
    "ograve",
    "ocircumflex",
    "odieresis",
    "otilde",
    "uacute",
    "ugrave",
    "ucircumflex",
    "udieresis",
    "dagger",
    "degree",
    "cent",
    "sterling",
    "section",
    "bullet",
    "paragraph",
    "germandbls",
    "registered",
    "copyright",
    "trademark",
    "acute",
    "dieresis",
    "notequal",
    "AE",
    "Oslash",
    "infinity",
    "plusminus",
    "lessequal",
    "greaterequal",
    "yen",
    "mu",
    "partialdiff",
    "summation",
    "product",
    "pi",
    "integral",
    "ordfeminine",
    "ordmasculine",
    "Omega",
    "ae",
    "oslash",
    "questiondown",
    "exclamdown",
    "logicalnot",
    "radical",
    "florin",
    "approxequal",
    "Delta",
    "guillemotleft",
    "guillemotright",
    "ellipsis",
    "nonbreakingspace",
    "Agrave",
    "Atilde",
    "Otilde",
    "OE",
    "oe",
    "endash",
    "emdash",
    "quotedblleft",
    "quotedblright",
    "quoteleft",
    "quoteright",
    "divide",
    "lozenge",
    "ydieresis",
    "Ydieresis",
    "fraction",
    "currency",
    "guilsinglleft",
    "guilsinglright",
    "fi",
    "fl",
    "daggerdbl",
    "periodcentered",
    "quotesinglbase",
    "quotedblbase",
    "perthousand",
    "Acircumflex",
    "Ecircumflex",
    "Aacute",
    "Edieresis",
    "Egrave",
    "Iacute",
    "Icircumflex",
    "Idieresis",
    "Igrave",
    "Oacute",
    "Ocircumflex",
    "apple",
    "Ograve",
    "Uacute",
    "Ucircumflex",
    "Ugrave",
    "dotlessi",
    "circumflex",
    "tilde",
    "macron",
    "breve",
    "dotaccent",
    "ring",
    "cedilla",
    "hungarumlaut",
    "ogonek",
    "caron",
    "Lslash",
    "lslash",
    "Scaron",
    "scaron",
    "Zcaron",
    "zcaron",
    "brokenbar",
    "Eth",
    "eth",
    "Yacute",
    "yacute",
    "Thorn",
    "thorn",
    "minus",
    "multiply",
    "onesuperior",
    "twosuperior",
    "threesuperior",
    "onehalf",
    "onequarter",
    "threequarters",
    "franc",
    "Gbreve",
    "gbreve",
    "Idotaccent",
    "Scedilla",
    "scedilla",
    "Cacute",
    "cacute",
    "Ccaron",
    "ccaron",
    "dcroat",
];

/// Where a face's vertical origins come from, decided once per view.
#[derive(Clone, Debug)]
enum OriginSource<'a> {
    /// TrueType outlines: each glyph's `glyf` header.
    Glyf,
    /// CFF outlines with a `VORG`, the `VORG` clause's shortcut past the box.
    Vorg,
    /// CFF outlines without one: each glyph's charstring, through the parsed
    /// `CFF ` table, or nothing where the face has no table this crate reads.
    Cff(Option<Cff<'a>>),
}

/// A face's vertical origins, read for as many glyphs as a caller asks
/// ([`Sfnt::vertical_origins`]).
///
/// The `vmtx` clause's definition: a glyph's vertical origin is its top side
/// bearing plus the top of its bounding box. Which box is a question for the
/// face's outlines:
///
/// - **TrueType outlines** (a `glyf` table): the `yMax` its header states
///   ([`crate::glyf::bounds`]). A `VORG` is ignored, as the `VORG` clause
///   requires of a face with TrueType outlines.
/// - **CFF outlines** (a `CFF ` table): `VORG`'s origin where the face has
///   one, which is the clause's own shortcut past the box; without one, the top
///   of the glyph's outline as its charstring draws it, control points
///   included ([`crate::Outline::bounds`]) and rounded to a unit — the bounding
///   box wherever the outline has a point at its topmost extreme. A `CFF2`
///   face, which [`Cff`] does not read, has `VORG`'s origins or none.
///
/// A glyph with no outline has the origin point for its box, so its vertical
/// origin is its top side bearing.
#[derive(Clone, Debug)]
pub struct VerticalOrigins<'s, 'a> {
    face: &'s Sfnt<'a>,
    source: OriginSource<'a>,
}

impl VerticalOrigins<'_, '_> {
    /// The y of a glyph's vertical origin, in font units.
    ///
    /// `None` for a glyph with no top side bearing and no `VORG` to stand in
    /// for one, or no box to add one to: a glyph past `loca`, a charstring the
    /// face does not have, a face with no outline table this crate reads.
    #[must_use]
    pub fn origin(&self, glyph: u16) -> Option<i32> {
        match &self.source {
            OriginSource::Glyf => {
                let top = crate::glyf::bounds(self.face, glyph)?.y_max;
                let bearing = self.face.top_side_bearing(glyph)?;
                Some(i32::from(top) + i32::from(bearing))
            }
            OriginSource::Vorg => self.face.vorg_origin(glyph).map(i32::from),
            OriginSource::Cff(cff) => {
                let bearing = self.face.top_side_bearing(glyph)?;
                let outline = cff.as_ref()?.outline(glyph)?;
                // An outline that draws nothing is bounded at its origin.
                let top = outline.bounds().map_or(0, |(_, _, _, top)| {
                    // `bounds` passes over every point that is not finite, so
                    // `top` is finite; the clamp holds a hostile charstring's
                    // inside `i32`.
                    top.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
                });
                Some(top.saturating_add(i32::from(bearing)))
            }
        }
    }
}

/// Whether a format 4, 12 or 13 subtable's entries are in the order its
/// format requires, every one readable, counting each entry read.
///
/// Read once a face and kept ([`CmapChoice::ordered`]): a subtable in order is
/// bisected and gives the answer a scan from the start gives, and one out of
/// order — which a broken producer writes and the format does not allow — is
/// scanned as written, so the first entry that matches still wins.
fn in_order(sub: &[u8], steps: &mut u64) -> bool {
    match be16(sub, 0) {
        Some(4) => {
            let Some(seg2) = be16(sub, 6) else {
                return false;
            };
            let mut previous = 0u16;
            for i in 0..usize::from(seg2) / 2 {
                *steps = steps.saturating_add(1);
                let Some(end) = be16(sub, 14 + i * 2) else {
                    return false;
                };
                if end < previous {
                    return false;
                }
                previous = end;
            }
            true
        }
        Some(12 | 13) => {
            let Some(groups) = be32(sub, 12) else {
                return false;
            };
            let mut previous_end: Option<u32> = None;
            for i in 0..(groups as usize).min(1 << 20) {
                *steps = steps.saturating_add(1);
                let at = 16 + i * 12;
                let (Some(start), Some(end), Some(_)) =
                    (be32(sub, at), be32(sub, at + 4), be32(sub, at + 8))
                else {
                    return false;
                };
                if start > end || previous_end.is_some_and(|previous| start <= previous) {
                    return false;
                }
                previous_end = Some(end);
            }
            true
        }
        _ => false,
    }
}

/// The first of `count` entries whose key, read by `key`, is at least `code`,
/// by bisection over keys that do not descend: the entry a scan from the start
/// stops at. `count` when none is. Each key read is a step.
fn lower_bound(
    count: usize,
    code: u32,
    steps: &mut u64,
    key: impl Fn(usize) -> Option<u32>,
) -> Option<usize> {
    let (mut low, mut high) = (0usize, count);
    while low < high {
        let middle = low + (high - low) / 2;
        *steps = steps.saturating_add(1);
        if key(middle)? < code {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    Some(low)
}

/// Looks a code up in one `cmap` subtable, in the formats that occur.
fn lookup_cmap(sub: &[u8], code: u32, ordered: bool, steps: &mut u64) -> Option<u16> {
    *steps = steps.saturating_add(1);
    match be16(sub, 0)? {
        // Format 0: a 256-byte table.
        0 => {
            let byte = u8::try_from(code).ok()?;
            sub.get(6 + usize::from(byte)).map(|&g| u16::from(g))
        }
        // Format 2: the high-byte mapping the legacy CJK encodings use.
        //
        // A 256-entry key array says, for each *first* byte, which subheader
        // reads the byte after it — key 0 meaning "this byte is not a lead
        // byte", which is how a mixed one- and two-byte encoding is written as
        // one table. Subheader 0 is therefore the single-byte map and the rest
        // are one per lead byte.
        //
        // **`code` here is a byte or a byte pair, not a scalar value.** That is
        // the whole difficulty with this format and it is not this function's
        // to solve: the bytes are in Shift-JIS, Big5, GBK or one of the
        // Macintosh legacy encodings, named by the subtable's platform,
        // encoding and language, and turning a Unicode scalar into one needs a
        // conversion table this repository does not carry. See
        // `Sfnt::glyph_for_char`, which is where that decision is recorded,
        // and `features/fonts.md`, which records why. What is here is the
        // format read correctly, so a caller holding real codes gets real
        // glyphs.
        2 => {
            let keys = 6usize;
            let headers = keys + 512;
            if code > 0xFFFF {
                return Some(0);
            }
            // The *first* byte selects the subheader, whether or not a second
            // follows. Key 0 says the byte stands alone and subheader 0 reads
            // it; anything else says it leads a pair, and the key is already
            // the byte offset into the subheader array because each subheader
            // is eight bytes wide.
            let lead = if code < 0x100 { code } else { code >> 8 } as usize;
            let key = usize::from(be16(sub, keys + lead * 2)?);
            // A single byte that leads a pair is half a code, and a lead byte
            // whose key is zero is not a lead byte. Both are "no glyph".
            if (code < 0x100) != (key == 0) {
                return Some(0);
            }
            let lo = (code & 0xFF) as usize;
            let header = headers.checked_add(key)?;
            let first = u32::from(be16(sub, header)?);
            let count = u32::from(be16(sub, header + 2)?);
            let delta = be16(sub, header + 4)?;
            let range = usize::from(be16(sub, header + 6)?);
            let byte = lo as u32;
            if count == 0 || byte < first || byte >= first + count {
                return Some(0);
            }
            // The offset is relative to its own slot, as in format 4.
            let at = header
                .checked_add(6)?
                .checked_add(range)?
                .checked_add((byte - first) as usize * 2)?;
            let glyph = be16(sub, at)?;
            Some(if glyph == 0 {
                0
            } else {
                glyph.wrapping_add(delta)
            })
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

            // In order, the first segment whose end is at least the code is
            // the one bisection finds, and the segments before it are the
            // ones a scan passes over.
            let from = if ordered {
                lower_bound(segs, u32::from(code), steps, |i| {
                    be16(sub, ends + i * 2).map(u32::from)
                })?
            } else {
                0
            };
            for i in from..segs {
                *steps = steps.saturating_add(1);
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
                let at = ranges + i * 2 + usize::from(range_offset) + usize::from(code - start) * 2;
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
            for i in group_candidates(sub, groups, code, ordered, steps)? {
                *steps = steps.saturating_add(1);
                let at = 16 + i * 12;
                let (Some(start), Some(end), Some(glyph)) =
                    (be32(sub, at), be32(sub, at + 4), be32(sub, at + 8))
                else {
                    break;
                };
                if (start..=end).contains(&code) {
                    // `startGlyphID` is whatever the file says and the offset
                    // into the group runs to the whole of `u32`, so the sum
                    // overflows long before it fails to be a glyph id.
                    return glyph
                        .checked_add(code - start)
                        .and_then(|id| u16::try_from(id).ok());
                }
            }
            Some(0)
        }
        // Format 13: the same groups as format 12, read the same way, and the
        // glyph is the *same* one for every code in the range rather than an
        // offset into a run.
        //
        // It exists for the last-resort fonts, where thousands of code points
        // all map to one box — writing that as format 12 would need a group per
        // character. The two formats differing only in whether the offset is
        // added is why this arm is beside that one and not somewhere else: a
        // reader that treated 13 as 12 would return a different glyph for every
        // character in the range, all but the first of them wrong.
        13 => {
            let groups = be32(sub, 12)? as usize;
            for i in group_candidates(sub, groups, code, ordered, steps)? {
                *steps = steps.saturating_add(1);
                let at = 16 + i * 12;
                let (Some(start), Some(end), Some(glyph)) =
                    (be32(sub, at), be32(sub, at + 4), be32(sub, at + 8))
                else {
                    break;
                };
                if (start..=end).contains(&code) {
                    return u16::try_from(glyph).ok();
                }
            }
            Some(0)
        }
        _ => None,
    }
}

/// The format 12 or 13 groups a lookup of `code` reads, in the order it
/// reads them: every group, when the subtable is out of order, and when it is
/// in order only the one group that can hold the code — the last that starts
/// at or below it, found by bisection — or none.
fn group_candidates(
    sub: &[u8],
    groups: usize,
    code: u32,
    ordered: bool,
    steps: &mut u64,
) -> Option<core::ops::Range<usize>> {
    let groups = groups.min(1 << 20);
    if !ordered {
        return Some(0..groups);
    }
    // The groups starting at or below `code` are a prefix, since the starts
    // ascend; the last of them is the only one that can hold it, the groups
    // being disjoint.
    let past = lower_bound(groups, code.saturating_add(1), steps, |i| {
        be32(sub, 16 + i * 12)
    })?;
    if code == u32::MAX {
        // `code + 1` saturated, so the bound found the first start that is
        // `u32::MAX` rather than the first past it.
        return Some(groups.saturating_sub(1)..groups);
    }
    Some(past.saturating_sub(1)..past)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A lookup that scans, as one through a subtable out of order does.
    fn lookup_cmap_scanned(sub: &[u8], code: u32) -> Option<u16> {
        lookup_cmap(sub, code, false, &mut 0)
    }

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

    /// **Format 2 reads a byte and a byte pair, and refuses the halves.**
    ///
    /// A hand-built subtable in the shape the legacy CJK encodings use: `0x41`
    /// is a single-byte code, `0x81` leads a pair, and every other lead byte
    /// maps nothing. The three cases that matter are the two lookups and the
    /// two ways of getting half a code — a lead byte on its own, and a pair
    /// whose first byte does not lead.
    #[test]
    fn a_format_2_subtable_reads_one_byte_and_two() {
        let mut sub: Vec<u8> = Vec::new();
        sub.extend_from_slice(&2u16.to_be_bytes()); // format
        sub.extend_from_slice(&0u16.to_be_bytes()); // length, unread here
        sub.extend_from_slice(&0u16.to_be_bytes()); // language
                                                    // subHeaderKeys: 0 everywhere but 0x81, which names subheader 1 at
                                                    // byte offset 8.
        let mut keys = vec![0u8; 512];
        keys[0x81 * 2 + 1] = 8;
        sub.extend_from_slice(&keys);

        // Subheader 0: the single-byte map, codes 0x41..0x42.
        let headers = sub.len();
        sub.extend_from_slice(&0x41u16.to_be_bytes()); // firstCode
        sub.extend_from_slice(&2u16.to_be_bytes()); // entryCount
        sub.extend_from_slice(&0u16.to_be_bytes()); // idDelta
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset, patched
                                                    // Subheader 1: the pair map, low bytes 0x40..0x41.
        sub.extend_from_slice(&0x40u16.to_be_bytes());
        sub.extend_from_slice(&2u16.to_be_bytes());
        sub.extend_from_slice(&1000u16.to_be_bytes()); // idDelta
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset, patched
        let glyphs = sub.len();
        for id in [7u16, 8, 1, 0] {
            sub.extend_from_slice(&id.to_be_bytes());
        }

        // Each offset is measured from its own slot, as in format 4.
        let patch = |sub: &mut Vec<u8>, at: usize, to: usize| {
            let offset = (to - (at + 6)) as u16;
            sub[at + 6..at + 8].copy_from_slice(&offset.to_be_bytes());
        };
        patch(&mut sub, headers, glyphs);
        patch(&mut sub, headers + 8, glyphs + 4);

        assert_eq!(
            lookup_cmap_scanned(&sub, 0x41),
            Some(7),
            "a single-byte code"
        );
        assert_eq!(lookup_cmap_scanned(&sub, 0x42), Some(8), "the next one");
        assert_eq!(lookup_cmap_scanned(&sub, 0x43), Some(0), "past the run");
        assert_eq!(
            lookup_cmap_scanned(&sub, 0x8140),
            Some(1001),
            "a pair, and `idDelta` is added to the glyph the array holds"
        );
        assert_eq!(
            lookup_cmap_scanned(&sub, 0x8141),
            Some(0),
            "a zero in the array stays zero rather than taking the delta"
        );
        assert_eq!(
            lookup_cmap_scanned(&sub, 0x81),
            Some(0),
            "a lead byte on its own is half a code"
        );
        assert_eq!(
            lookup_cmap_scanned(&sub, 0x4141),
            Some(0),
            "a pair whose first byte does not lead is not a code either"
        );
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

    /// Wraps a `cmap` subtable in the table and the directory around it.
    fn font_with_cmap_subtable(platform: u16, encoding: u16, sub: &[u8]) -> Vec<u8> {
        let mut cmap: Vec<u8> = Vec::new();
        cmap.extend_from_slice(&0u16.to_be_bytes()); // version
        cmap.extend_from_slice(&1u16.to_be_bytes()); // numTables
        cmap.extend_from_slice(&platform.to_be_bytes());
        cmap.extend_from_slice(&encoding.to_be_bytes());
        cmap.extend_from_slice(&12u32.to_be_bytes()); // offset
        cmap.extend_from_slice(sub);
        sfnt_with(b"cmap", &cmap)
    }

    /// Found by the `sfnt` and `truetype` fuzz targets within thirty seconds
    /// of each other, both reducing to this: a format 12 group whose
    /// `startGlyphID` is near the top of `u32`, so adding the offset into the
    /// group overflows before the result can be rejected for not fitting a
    /// glyph id. `attempt to add with overflow` — a panic on untrusted input,
    /// which ruling 1 does not allow, and a silently wrong glyph in a build
    /// with overflow checks off, which is worse.
    #[test]
    fn a_format_12_group_starting_near_the_top_of_u32_does_not_overflow() {
        let mut sub: Vec<u8> = Vec::new();
        sub.extend_from_slice(&12u16.to_be_bytes()); // format
        sub.extend_from_slice(&0u16.to_be_bytes()); // reserved
        sub.extend_from_slice(&0u32.to_be_bytes()); // length, unread
        sub.extend_from_slice(&0u32.to_be_bytes()); // language
        sub.extend_from_slice(&1u32.to_be_bytes()); // numGroups
        sub.extend_from_slice(&0u32.to_be_bytes()); // startCharCode
        sub.extend_from_slice(&0x0010_FFFFu32.to_be_bytes()); // endCharCode
        sub.extend_from_slice(&u32::MAX.to_be_bytes()); // startGlyphID

        // Platform 3, encoding 10 is the (3,10) subtable format 12 lives in.
        let data = font_with_cmap_subtable(3, 10, &sub);
        let sfnt = Sfnt::parse(&data).expect("a valid directory");

        // The lowest code in the group is the one whose sum still fits.
        assert_eq!(sfnt.glyph_for_char('\0'), None, "0xFFFFFFFF is no glyph id");
        // Every later code used to overflow the addition itself.
        assert_eq!(sfnt.glyph_for_char('A'), None);
        assert_eq!(sfnt.glyph_for_char('\u{1F600}'), None);
    }

    /// Builds an sfnt around several tables, in the order given.
    fn face(tables: &[(&[u8; 4], &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let mut offset = 12 + tables.len() * 16;
        let mut body = Vec::new();
        for (tag, data) in tables {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            offset += data.len();
            body.extend_from_slice(data);
        }
        out.extend_from_slice(&body);
        out
    }

    /// A `head` at 1000 units per em, its `loca` long.
    fn head() -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        head[50..52].copy_from_slice(&1i16.to_be_bytes());
        head
    }

    /// A `maxp` stating `glyphs`.
    fn maxp(glyphs: u16) -> Vec<u8> {
        let mut maxp = vec![0u8; 6];
        maxp[0..4].copy_from_slice(&0x0000_5000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&glyphs.to_be_bytes());
        maxp
    }

    /// A version 1.1 `vhea` whose `numOfLongVerMetrics` is `count`.
    fn vhea(count: u16) -> Vec<u8> {
        let mut vhea = vec![0u8; 36];
        vhea[0..4].copy_from_slice(&0x0001_1000u32.to_be_bytes());
        vhea[4..6].copy_from_slice(&500i16.to_be_bytes()); // vertTypoAscender
        vhea[6..8].copy_from_slice(&(-500i16).to_be_bytes()); // vertTypoDescender
        vhea[34..36].copy_from_slice(&count.to_be_bytes());
        vhea
    }

    /// A `vmtx`: the long metrics, then the side bearings after them.
    fn vmtx(long: &[(u16, i16)], bearings: &[i16]) -> Vec<u8> {
        let mut vmtx = Vec::new();
        for (advance, bearing) in long {
            vmtx.extend_from_slice(&advance.to_be_bytes());
            vmtx.extend_from_slice(&bearing.to_be_bytes());
        }
        for bearing in bearings {
            vmtx.extend_from_slice(&bearing.to_be_bytes());
        }
        vmtx
    }

    /// A `VORG` of `default` and the records given, in the order given.
    fn vorg(default: i16, records: &[(u16, i16)]) -> Vec<u8> {
        let mut vorg = Vec::new();
        vorg.extend_from_slice(&1u16.to_be_bytes()); // majorVersion
        vorg.extend_from_slice(&0u16.to_be_bytes()); // minorVersion
        vorg.extend_from_slice(&default.to_be_bytes());
        vorg.extend_from_slice(&(records.len() as u16).to_be_bytes());
        for (glyph, origin) in records {
            vorg.extend_from_slice(&glyph.to_be_bytes());
            vorg.extend_from_slice(&origin.to_be_bytes());
        }
        vorg
    }

    /// Every vertical metric reads back as the tables were written, and a
    /// read past what they hold answers `None`.
    ///
    /// The face has no outline table, so its vertical origins are `VORG`'s:
    /// a CFF face's shortcut, and the one source a face without a box has.
    #[test]
    fn vertical_metrics_read_from_vhea_vmtx_and_vorg() {
        let data = face(&[
            (b"VORG", &vorg(880, &[(2, 861), (3, 849)])),
            (b"head", &head()),
            (b"maxp", &maxp(4)),
            (b"vhea", &vhea(2)),
            (b"vmtx", &vmtx(&[(1000, 80), (900, 120)], &[50, 60])),
        ]);
        let sfnt = Sfnt::parse(&data).expect("a valid directory");

        let advances: Vec<Option<u16>> = (0..4).map(|g| sfnt.vertical_advance(g)).collect();
        assert_eq!(advances, [Some(1000), Some(900), Some(900), Some(900)]);
        let bearings: Vec<Option<i16>> = (0..4).map(|g| sfnt.top_side_bearing(g)).collect();
        assert_eq!(bearings, [Some(80), Some(120), Some(50), Some(60)]);
        assert_eq!(sfnt.top_side_bearing(4), None, "past maxp's glyph count");

        let origins: Vec<Option<i16>> = (0..5).map(|g| sfnt.vorg_origin(g)).collect();
        assert_eq!(
            origins,
            [Some(880), Some(880), Some(861), Some(849), Some(880)],
            "a record where the glyph has one, the default where it has none"
        );
        let vertical: Vec<Option<i32>> = (0..4).map(|g| sfnt.vertical_origin(g)).collect();
        assert_eq!(vertical, [Some(880), Some(880), Some(861), Some(849)]);
    }

    /// `hmtx`'s rule on end: a glyph past the long metrics repeats the last
    /// advance height, however far past, and reads its own side bearing from
    /// the array after them.
    #[test]
    fn a_glyph_past_the_long_metrics_repeats_the_last_advance_height() {
        let data = face(&[
            (b"head", &head()),
            (b"maxp", &maxp(4)),
            (b"vhea", &vhea(1)),
            (b"vmtx", &vmtx(&[(1000, 10)], &[20, 30, 40])),
        ]);
        let sfnt = Sfnt::parse(&data).expect("a valid directory");
        for glyph in [0u16, 1, 2, 3, 500, u16::MAX] {
            assert_eq!(sfnt.vertical_advance(glyph), Some(1000), "glyph {glyph}");
        }
        let bearings: Vec<Option<i16>> = (0..5).map(|g| sfnt.top_side_bearing(g)).collect();
        assert_eq!(bearings, [Some(10), Some(20), Some(30), Some(40), None]);
    }

    /// No `vhea`, no `vmtx`, a count of zero, or a `vhea` too short to hold
    /// its count: every vertical metric answers `None`, and so does `VORG`'s
    /// with no `VORG` or one of a version nobody has written down.
    #[test]
    fn a_face_without_vertical_metrics_answers_none() {
        let mut hhea = vec![0u8; 36];
        hhea[34..36].copy_from_slice(&1u16.to_be_bytes());
        let hmtx = [1000u16.to_be_bytes(), 0u16.to_be_bytes()].concat();
        let long = vmtx(&[(1000, 10)], &[]);
        let short_vhea = vhea(1)[..34].to_vec();
        let mut future = vorg(880, &[]);
        future[0..2].copy_from_slice(&2u16.to_be_bytes());

        let faces: [(&str, Vec<u8>); 6] = [
            (
                "horizontal metrics only",
                face(&[(b"head", &head()), (b"hhea", &hhea), (b"hmtx", &hmtx)]),
            ),
            (
                "a vmtx and no vhea",
                face(&[(b"head", &head()), (b"vmtx", &long)]),
            ),
            (
                "a vhea and no vmtx",
                face(&[(b"head", &head()), (b"vhea", &vhea(1))]),
            ),
            (
                "a vhea counting no metric",
                face(&[(b"head", &head()), (b"vhea", &vhea(0)), (b"vmtx", &long)]),
            ),
            (
                "a vhea too short to hold its count",
                face(&[(b"head", &head()), (b"vhea", &short_vhea), (b"vmtx", &long)]),
            ),
            (
                "a VORG of major version 2",
                face(&[(b"VORG", &future), (b"head", &head())]),
            ),
        ];
        for (what, data) in &faces {
            let sfnt = Sfnt::parse(data).expect("a valid directory");
            for glyph in [0u16, 1, 7] {
                assert_eq!(sfnt.vertical_advance(glyph), None, "{what}");
                assert_eq!(sfnt.top_side_bearing(glyph), None, "{what}");
                assert_eq!(sfnt.vorg_origin(glyph), None, "{what}");
                assert_eq!(sfnt.vertical_origin(glyph), None, "{what}");
            }
        }
        let horizontal = Sfnt::parse(&faces[0].1).expect("a valid directory");
        assert_eq!(horizontal.advance(0), Some(1000), "and hmtx still reads");
    }

    /// A `vmtx` shorter than its `vhea` claims is read as far as it goes: the
    /// entries it holds answer, and every one past its end answers `None`
    /// rather than a number read from beyond the table.
    #[test]
    fn a_vmtx_shorter_than_vhea_claims_answers_none_past_its_end() {
        let data = face(&[
            (b"head", &head()),
            (b"vhea", &vhea(3)),
            (b"vmtx", &vmtx(&[(1000, 10)], &[])),
            (b"zzzz", &[0xAB; 16]),
        ]);
        let sfnt = Sfnt::parse(&data).expect("a valid directory");
        assert_eq!(sfnt.vertical_advance(0), Some(1000));
        assert_eq!(sfnt.top_side_bearing(0), Some(10));
        for glyph in [1u16, 2, 3, 9] {
            assert_eq!(sfnt.vertical_advance(glyph), None, "glyph {glyph}");
            assert_eq!(sfnt.top_side_bearing(glyph), None, "glyph {glyph}");
        }
    }

    /// A face with TrueType outlines takes its vertical origin from `glyf`'s
    /// `yMax` and `vmtx`, and its `VORG` is ignored, as the `VORG` clause
    /// requires — however much the two disagree.
    #[test]
    fn a_vorg_beside_glyf_is_ignored() {
        // Glyph 0 is empty; glyph 1 is a header whose box tops out at 700.
        let mut glyf = Vec::new();
        glyf.extend_from_slice(&1i16.to_be_bytes()); // numberOfContours
        for edge in [0i16, -100, 500, 700] {
            glyf.extend_from_slice(&edge.to_be_bytes());
        }
        let loca: Vec<u8> = [0u32, 0, 10].iter().flat_map(|o| o.to_be_bytes()).collect();
        let data = face(&[
            (b"VORG", &vorg(880, &[(1, 999)])),
            (b"glyf", &glyf),
            (b"head", &head()),
            (b"loca", &loca),
            (b"maxp", &maxp(2)),
            (b"vhea", &vhea(2)),
            (b"vmtx", &vmtx(&[(1000, 90), (1000, 100)], &[])),
        ]);
        let sfnt = Sfnt::parse(&data).expect("a valid directory");
        assert_eq!(
            sfnt.vorg_origin(1),
            Some(999),
            "the table itself still reads"
        );
        assert_eq!(
            sfnt.vertical_origin(1),
            Some(800),
            "yMax 700 and a bearing of 100"
        );
        assert_eq!(
            sfnt.vertical_origin(0),
            Some(90),
            "an empty glyph's box is its origin, so its bearing is its origin"
        );
        assert_eq!(sfnt.vertical_origin(2), None, "past the end of loca");
    }

    /// `VORG` records out of the order the clause requires are scanned as
    /// written and the first match wins; a count past what the table holds
    /// reads only the records that are there.
    #[test]
    fn a_vorg_out_of_order_or_overcounted_is_read_as_written() {
        let data = face(&[
            (b"VORG", &vorg(880, &[(5, 700), (2, 600), (5, 650)])),
            (b"head", &head()),
        ]);
        let sfnt = Sfnt::parse(&data).expect("a valid directory");
        assert_eq!(
            sfnt.vorg_origin(5),
            Some(700),
            "the first record for glyph 5"
        );
        assert_eq!(sfnt.vorg_origin(2), Some(600));
        assert_eq!(sfnt.vorg_origin(3), Some(880), "the default");

        let mut overcounted = vorg(880, &[(4, 777)]);
        overcounted[6..8].copy_from_slice(&1000u16.to_be_bytes());
        let data = face(&[(b"VORG", &overcounted), (b"head", &head())]);
        let sfnt = Sfnt::parse(&data).expect("a valid directory");
        assert_eq!(sfnt.vorg_origin(4), Some(777));
        assert_eq!(sfnt.vorg_origin(9), Some(880));
    }

    /// Two long metrics and two side bearings after them: glyphs 2 and 3
    /// repeat glyph 1's advance and keep their own bearings.
    #[test]
    fn left_side_bearings_come_from_the_long_metrics_and_the_array_after_them() {
        let mut hhea = vec![0u8; 36];
        hhea[34..36].copy_from_slice(&2u16.to_be_bytes());
        let mut hmtx = Vec::new();
        for (advance, bearing) in [(500u16, 10i16), (600, -20)] {
            hmtx.extend_from_slice(&advance.to_be_bytes());
            hmtx.extend_from_slice(&bearing.to_be_bytes());
        }
        hmtx.extend_from_slice(&30i16.to_be_bytes());
        hmtx.extend_from_slice(&(-40i16).to_be_bytes());
        let data = sfnt_with_tables(&[(b"hhea", hhea), (b"hmtx", hmtx)]);
        let sfnt = Sfnt::parse(&data).expect("a face");
        assert_eq!(sfnt.left_side_bearing(0), Some(10));
        assert_eq!(sfnt.left_side_bearing(1), Some(-20));
        assert_eq!(sfnt.left_side_bearing(2), Some(30));
        assert_eq!(sfnt.left_side_bearing(3), Some(-40));
        assert_eq!(sfnt.left_side_bearing(4), None, "past the array");
        assert_eq!(sfnt.advance(3), Some(600));
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

    /// Builds an sfnt holding every table given, in that order.
    fn sfnt_with_tables(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&(tables.len() as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let mut offset = 12 + 16 * tables.len();
        for (tag, table) in tables {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(table.len() as u32).to_be_bytes());
            offset += table.len();
        }
        for (_, table) in tables {
            out.extend_from_slice(table);
        }
        out
    }

    /// A `post` table: its version, the rest of the 32-byte header, and the
    /// body that version carries.
    fn post(version: u32, body: &[u8]) -> Vec<u8> {
        let mut out = version.to_be_bytes().to_vec();
        out.extend_from_slice(&[0; 28]);
        out.extend_from_slice(body);
        out
    }

    /// A version 2.0 body: each glyph's index, then the table's own strings.
    fn post_2_body(indices: &[u16], strings: &[&str]) -> Vec<u8> {
        let mut out = (indices.len() as u16).to_be_bytes().to_vec();
        for index in indices {
            out.extend_from_slice(&index.to_be_bytes());
        }
        for string in strings {
            out.push(string.len() as u8);
            out.extend_from_slice(string.as_bytes());
        }
        out
    }

    /// A format 4 subtable mapping each code to its glyph, a segment each,
    /// and the closing segment at 0xFFFF.
    fn format_4(pairs: &[(u16, u16)]) -> Vec<u8> {
        let mut pairs = pairs.to_vec();
        pairs.sort_unstable();
        let segments = pairs.len() + 1;
        let mut out = Vec::new();
        out.extend_from_slice(&4u16.to_be_bytes()); // format
        out.extend_from_slice(&0u16.to_be_bytes()); // length, unread
        out.extend_from_slice(&0u16.to_be_bytes()); // language
        out.extend_from_slice(&((segments * 2) as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]); // search hints
        for (code, _) in &pairs {
            out.extend_from_slice(&code.to_be_bytes()); // endCode
        }
        out.extend_from_slice(&0xFFFFu16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        for (code, _) in &pairs {
            out.extend_from_slice(&code.to_be_bytes()); // startCode
        }
        out.extend_from_slice(&0xFFFFu16.to_be_bytes());
        for (code, glyph) in &pairs {
            out.extend_from_slice(&glyph.wrapping_sub(*code).to_be_bytes()); // idDelta
        }
        out.extend_from_slice(&1u16.to_be_bytes());
        for _ in 0..segments {
            out.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset
        }
        out
    }

    /// A `cmap` table of one subtable per record, in the order given.
    fn cmap_of(records: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_be_bytes()); // version
        out.extend_from_slice(&(records.len() as u16).to_be_bytes());
        let mut offset = 4 + 8 * records.len();
        for (platform, encoding, sub) in records {
            out.extend_from_slice(&platform.to_be_bytes());
            out.extend_from_slice(&encoding.to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            offset += sub.len();
        }
        for (_, _, sub) in records {
            out.extend_from_slice(sub);
        }
        out
    }

    /// FT-03.3: a version 2.0 `post` names a glyph by its place in the
    /// standard Macintosh order as well as by the table's own strings, so
    /// `space`, `Delta` and `summation`, which a face stores by index, reach
    /// their glyphs (`docs/design/symbol-faces.md`, "The route").
    #[test]
    fn a_post_name_in_the_standard_order_reaches_its_glyph() {
        // .notdef, space, Delta, summation, "alpha", unnamed, "uni2665", space.
        let body = post_2_body(&[0, 3, 168, 153, 258, 0, 259, 3], &["alpha", "uni2665"]);
        let data = sfnt_with_tables(&[(b"post", post(0x0002_0000, &body))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(
            face.glyph_for_name("space"),
            Some(1),
            "the first glyph so named"
        );
        assert_eq!(face.glyph_for_name("Delta"), Some(2));
        assert_eq!(face.glyph_for_name("summation"), Some(3));
        assert_eq!(
            face.glyph_for_name("alpha"),
            Some(4),
            "the table's own string"
        );
        assert_eq!(face.glyph_for_name("uni2665"), Some(6));
        assert_eq!(
            face.glyph_for_name(".notdef"),
            Some(0),
            "an unnamed glyph is .notdef, and glyph 0 comes first"
        );
        assert_eq!(
            face.glyph_for_name("Omega"),
            None,
            "in the standard order, but no glyph has its index"
        );
        assert_eq!(face.glyph_for_name("beta"), None);
        assert_eq!(face.glyph_for_name(""), None);

        // The order is Apple's: its first, its last, and the names between.
        assert_eq!(MAC_STANDARD_NAMES[0], ".notdef");
        assert_eq!(MAC_STANDARD_NAMES[3], "space");
        assert_eq!(MAC_STANDARD_NAMES[36], "A");
        assert_eq!(MAC_STANDARD_NAMES[153], "summation");
        assert_eq!(MAC_STANDARD_NAMES[168], "Delta");
        assert_eq!(MAC_STANDARD_NAMES[210], "apple");
        assert_eq!(MAC_STANDARD_NAMES[257], "dcroat");
        let distinct: std::collections::BTreeSet<&str> =
            MAC_STANDARD_NAMES.iter().copied().collect();
        assert_eq!(distinct.len(), 258, "no name twice in the order");
    }

    /// FT-03.3: each version of `post` names as it states. 1.0 is the
    /// standard order glyph for glyph, as far as `maxp`'s count reaches; 2.5
    /// is a signed offset into it; 3.0 and a version no specification gives
    /// name nothing; a 2.0 table cut short names what it holds and no more.
    #[test]
    fn each_post_version_names_as_it_states() {
        let mut maxp = vec![0u8; 6];
        maxp[..4].copy_from_slice(&0x0000_5000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&40u16.to_be_bytes());
        let data = sfnt_with_tables(&[(b"maxp", maxp), (b"post", post(0x0001_0000, &[]))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_for_name("A"), Some(36));
        assert_eq!(face.glyph_for_name("space"), Some(3));
        assert_eq!(
            face.glyph_for_name("Delta"),
            None,
            "index 168 is past the face's 40 glyphs"
        );
        assert_eq!(face.glyph_for_name("alpha"), None, "not in the order");
        let data = sfnt_with_tables(&[(b"post", post(0x0001_0000, &[]))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(
            face.glyph_for_name("dcroat"),
            Some(257),
            "with no maxp, the order whole"
        );

        // 2.5: A, space and Adieresis by positive offsets, nonmarkingreturn by
        // a negative one.
        let mut body = 5u16.to_be_bytes().to_vec();
        for offset in [0i8, 35, 1, 95, -2] {
            body.extend_from_slice(&offset.to_be_bytes());
        }
        let data = sfnt_with_tables(&[(b"post", post(0x0002_5000, &body))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_for_name("A"), Some(1));
        assert_eq!(face.glyph_for_name("space"), Some(2));
        assert_eq!(face.glyph_for_name("Adieresis"), Some(3));
        assert_eq!(face.glyph_for_name("nonmarkingreturn"), Some(4));
        assert_eq!(face.glyph_for_name("B"), None);
        assert_eq!(face.glyph_for_name("alpha"), None, "2.5 has no strings");

        for version in [0x0003_0000u32, 0x0004_0000, 0, 0xFFFF_FFFF] {
            let data =
                sfnt_with_tables(&[(b"post", post(version, &post_2_body(&[0, 3], &["alpha"])))]);
            let face = Sfnt::parse(&data).expect("a directory");
            assert_eq!(face.glyph_for_name("space"), None, "{version:#x}");
            assert_eq!(face.glyph_for_name("alpha"), None, "{version:#x}");
        }

        // A 2.0 table whose second string is cut short.
        let mut body = post_2_body(&[0, 3, 258, 259], &["alpha", "beta"]);
        body.truncate(body.len() - 2);
        let data = sfnt_with_tables(&[(b"post", post(0x0002_0000, &body))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_for_name("space"), Some(1));
        assert_eq!(face.glyph_for_name("alpha"), Some(2));
        assert_eq!(
            face.glyph_for_name("beta"),
            None,
            "a string cut short is no name"
        );
        // An index array that claims more glyphs than the table holds.
        let mut body = 9u16.to_be_bytes().to_vec();
        body.extend_from_slice(&3u16.to_be_bytes());
        let data = sfnt_with_tables(&[(b"post", post(0x0002_0000, &body))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_for_name("space"), Some(0));
        assert_eq!(face.glyph_for_name("A"), None);
        // A header with no body, an empty table, and no table at all.
        for table in [post(0x0002_0000, &[]), Vec::new(), vec![0, 2]] {
            let data = sfnt_with_tables(&[(b"post", table)]);
            let face = Sfnt::parse(&data).expect("a directory");
            assert_eq!(face.glyph_for_name("space"), None);
        }
        let data = sfnt_with(b"head", &[0u8; 54]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_for_name("space"), None, "no post table");
    }

    /// FT-03.3: the Unicode subtable and the (3,0) symbol subtable are asked
    /// apart, each only for what it is keyed by, whichever the text lookup
    /// chose; (3,10) is preferred to (3,1), a platform 0 subtable is taken
    /// where neither is but never encoding 5's, and glyph 0 is no answer.
    #[test]
    fn the_unicode_and_symbol_subtables_are_asked_apart() {
        let both = cmap_of(&[
            (3, 0, format_4(&[(0xF061, 7), (0xF020, 9)])),
            (3, 1, format_4(&[(0x03B1, 5), (0x0020, 3)])),
        ]);
        let data = sfnt_with_tables(&[(b"cmap", both)]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_for_unicode('\u{03B1}'), Some(5));
        assert_eq!(face.glyph_for_unicode(' '), Some(3));
        assert_eq!(
            face.glyph_for_unicode('\u{F061}'),
            None,
            "the Unicode subtable maps nothing there"
        );
        assert_eq!(face.glyph_for_unicode('\u{1F600}'), None, "past a format 4");
        assert_eq!(face.glyph_for_symbol_code(0x61), Some(7));
        assert_eq!(face.glyph_for_symbol_code(0x20), Some(9));
        assert_eq!(
            face.glyph_for_symbol_code(0x62),
            None,
            "glyph 0 is no answer"
        );
        assert_eq!(
            face.glyph_for_char('\u{03B1}'),
            Some(5),
            "the text lookup took (3,1)"
        );

        let data = sfnt_with_tables(&[(
            b"cmap",
            cmap_of(&[
                (3, 1, format_4(&[(0x03B1, 5)])),
                (3, 10, format_4(&[(0x03B1, 6)])),
            ]),
        )]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_for_unicode('\u{03B1}'), Some(6), "(3,10) first");

        let data = sfnt_with_tables(&[(b"cmap", cmap_of(&[(0, 3, format_4(&[(0x03B1, 8)]))]))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_for_unicode('\u{03B1}'), Some(8), "platform 0");
        assert_eq!(face.glyph_for_symbol_code(0x61), None, "no (3,0) subtable");

        let data = sfnt_with_tables(&[(
            b"cmap",
            cmap_of(&[
                (0, 5, format_4(&[(0x03B1, 8)])),
                (1, 0, format_4(&[(0x61, 4)])),
            ]),
        )]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(
            face.glyph_for_unicode('\u{03B1}'),
            None,
            "encoding 5 maps sequences"
        );
        assert_eq!(
            face.glyph_for_unicode('a'),
            None,
            "a Macintosh subtable is not keyed by Unicode"
        );

        // A face with only a (3,0) subtable: the text lookup reads it at
        // 0xF000 too, and reaches the same glyph.
        let data = sfnt_with_tables(&[(b"cmap", cmap_of(&[(3, 0, format_4(&[(0xF061, 7)]))]))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_for_symbol_code(0x61), Some(7));
        assert_eq!(face.glyph_for_unicode('a'), None);
        assert_eq!(face.glyph_for_char('a'), Some(7));

        // A subtable that maps a code to glyph 0 answers nothing, and neither
        // does a face with no cmap, or one whose cmap is cut short.
        let data = sfnt_with_tables(&[(b"cmap", cmap_of(&[(3, 1, format_4(&[(0x03B1, 0)]))]))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_for_unicode('\u{03B1}'), None);
        for table in [Vec::new(), vec![0, 0, 0, 9], cmap_of(&[(3, 1, vec![0, 4])])] {
            let data = sfnt_with_tables(&[(b"cmap", table)]);
            let face = Sfnt::parse(&data).expect("a directory");
            assert_eq!(face.glyph_for_unicode('a'), None);
            assert_eq!(face.glyph_for_symbol_code(0x61), None);
        }
    }

    // ---- FT-16.2: what the font measurement API reads -------------------

    /// A format 0 subtable mapping each byte given to its glyph.
    fn format_0(pairs: &[(u8, u8)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_be_bytes()); // format
        out.extend_from_slice(&262u16.to_be_bytes()); // length
        out.extend_from_slice(&0u16.to_be_bytes()); // language
        let mut glyphs = [0u8; 256];
        for (code, glyph) in pairs {
            glyphs[usize::from(*code)] = *glyph;
        }
        out.extend_from_slice(&glyphs);
        out
    }

    /// A format 6 subtable: `first`, then one glyph a code.
    fn format_6(first: u16, glyphs: &[u16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&6u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // length, unread
        out.extend_from_slice(&0u16.to_be_bytes()); // language
        out.extend_from_slice(&first.to_be_bytes());
        out.extend_from_slice(&(glyphs.len() as u16).to_be_bytes());
        for glyph in glyphs {
            out.extend_from_slice(&glyph.to_be_bytes());
        }
        out
    }

    /// A format 12 or 13 subtable of the groups given, in the order given,
    /// each `(startCharCode, endCharCode, glyph)`.
    fn format_groups(format: u16, groups: &[(u32, u32, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&format.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // reserved
        out.extend_from_slice(&0u32.to_be_bytes()); // length, unread
        out.extend_from_slice(&0u32.to_be_bytes()); // language
        out.extend_from_slice(&(groups.len() as u32).to_be_bytes());
        for (start, end, glyph) in groups {
            out.extend_from_slice(&start.to_be_bytes());
            out.extend_from_slice(&end.to_be_bytes());
            out.extend_from_slice(&glyph.to_be_bytes());
        }
        out
    }

    /// One format 4 segment: its start and end codes, its delta, and the
    /// glyphs its `idRangeOffset` points at, where it has any.
    struct Segment4 {
        start: u16,
        end: u16,
        delta: u16,
        array: Option<Vec<u16>>,
    }

    /// A format 4 subtable of the segments given, in the order given — out
    /// of order where they are — each glyph array placed after the
    /// `idRangeOffset`s and pointed at from its segment's slot.
    fn format_4_segments(segments: &[Segment4]) -> Vec<u8> {
        let count = segments.len();
        let ranges = 14 + count * 2 + 2 + count * 2 + count * 2;
        let arrays = ranges + count * 2;
        let mut placed = Vec::new();
        let mut offsets = Vec::new();
        for (i, segment) in segments.iter().enumerate() {
            match &segment.array {
                None => offsets.push(0u16),
                Some(glyphs) => {
                    let at = arrays + placed.len() * 2;
                    offsets.push((at - (ranges + i * 2)) as u16);
                    placed.extend_from_slice(glyphs);
                }
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // length, unread
        out.extend_from_slice(&0u16.to_be_bytes()); // language
        out.extend_from_slice(&((count * 2) as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]); // search hints
        for segment in segments {
            out.extend_from_slice(&segment.end.to_be_bytes());
        }
        out.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
        for segment in segments {
            out.extend_from_slice(&segment.start.to_be_bytes());
        }
        for segment in segments {
            out.extend_from_slice(&segment.delta.to_be_bytes());
        }
        for offset in &offsets {
            out.extend_from_slice(&offset.to_be_bytes());
        }
        for glyph in &placed {
            out.extend_from_slice(&glyph.to_be_bytes());
        }
        out
    }

    /// The terminating segment every format 4 subtable ends with; its delta
    /// takes 0xFFFF to glyph 0.
    fn terminator() -> Segment4 {
        Segment4 {
            start: 0xFFFF,
            end: 0xFFFF,
            delta: 1,
            array: None,
        }
    }

    /// The format 2 subtable `a_format_2_subtable_reads_one_byte_and_two`
    /// builds: 0x41 and 0x42 single bytes, 0x8140 and 0x8141 a pair.
    fn format_2() -> Vec<u8> {
        let mut sub: Vec<u8> = Vec::new();
        sub.extend_from_slice(&2u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        sub.extend_from_slice(&0u16.to_be_bytes());
        let mut keys = vec![0u8; 512];
        keys[0x81 * 2 + 1] = 8;
        sub.extend_from_slice(&keys);
        let headers = sub.len();
        for (first, count, delta) in [(0x41u16, 2u16, 0u16), (0x40, 2, 1000)] {
            sub.extend_from_slice(&first.to_be_bytes());
            sub.extend_from_slice(&count.to_be_bytes());
            sub.extend_from_slice(&delta.to_be_bytes());
            sub.extend_from_slice(&0u16.to_be_bytes());
        }
        let glyphs = sub.len();
        for id in [7u16, 8, 1, 0] {
            sub.extend_from_slice(&id.to_be_bytes());
        }
        for (header, to) in [(headers, glyphs), (headers + 8, glyphs + 4)] {
            let offset = (to - (header + 6)) as u16;
            sub[header + 6..header + 8].copy_from_slice(&offset.to_be_bytes());
        }
        sub
    }

    /// A face holding one `cmap` subtable under `(platform, encoding)`, with
    /// enough glyphs for every glyph a test maps.
    fn mapped_face(platform: u16, encoding: u16, sub: Vec<u8>) -> Vec<u8> {
        face(&[
            (b"cmap", &cmap_of(&[(platform, encoding, sub)])),
            (b"head", &head()),
            (b"maxp", &maxp(u16::MAX)),
        ])
    }

    /// Every scalar value `char_map` answers is the one `glyph_for_char`
    /// answers, and every scalar `glyph_for_char` answers with a glyph is in
    /// `char_map`: the walk reads the subtable as the lookup does.
    fn assert_agrees_with_the_lookup(what: &str, sfnt: &Sfnt<'_>, map: &[(char, u16)]) {
        let scalars = (0u32..0x3_0000).chain([0xF_0000, 0x10_FFFE, 0x10_FFFF]);
        let mut expected = Vec::new();
        for scalar in scalars {
            let Some(c) = char::from_u32(scalar) else {
                continue;
            };
            if let Some(glyph) = sfnt.glyph_for_char(c).filter(|glyph| *glyph != 0) {
                expected.push((c, glyph));
            }
        }
        let read: Vec<(char, u16)> = map
            .iter()
            .copied()
            .filter(|(c, _)| {
                let scalar = u32::from(*c);
                scalar < 0x3_0000 || [0xF_0000, 0x10_FFFE, 0x10_FFFF].contains(&scalar)
            })
            .collect();
        assert_eq!(read, expected, "{what}: char_map against glyph_for_char");
    }

    /// FT-16.2: every scalar the chosen subtable maps, in each format the
    /// lookup reads, with the glyph it maps to, written in the test — and
    /// nothing the lookup does not answer.
    #[test]
    fn code_points_are_every_scalar_the_subtable_maps() {
        let ordered = format_4_segments(&[
            Segment4 {
                start: 0x41,
                end: 0x43,
                delta: 10,
                array: None,
            },
            Segment4 {
                start: 0x61,
                end: 0x62,
                delta: 0,
                array: Some(vec![7, 0]),
            },
            terminator(),
        ]);
        // Out of order: 'P' to 'Z' first, then 'A' to 'E', whose codes the
        // first segment already decides — its end is past them and its start
        // is not — so a lookup of 'B' answers glyph 0.
        let unordered = format_4_segments(&[
            Segment4 {
                start: 0x50,
                end: 0x5A,
                delta: 100,
                array: None,
            },
            Segment4 {
                start: 0x41,
                end: 0x45,
                delta: 200,
                array: None,
            },
            terminator(),
        ]);
        let symbol = format_4_segments(&[
            Segment4 {
                start: 0x41,
                end: 0x41,
                delta: 9u16.wrapping_sub(0x41),
                array: None,
            },
            Segment4 {
                start: 0x2022,
                end: 0x2022,
                delta: 8u16.wrapping_sub(0x2022),
                array: None,
            },
            Segment4 {
                start: 0xF020,
                end: 0xF020,
                delta: 5u16.wrapping_sub(0xF020),
                array: None,
            },
            Segment4 {
                start: 0xF041,
                end: 0xF041,
                delta: 6u16.wrapping_sub(0xF041),
                array: None,
            },
            terminator(),
        ]);
        let cases: Vec<(&str, Vec<u8>, Vec<(char, u16)>)> = vec![
            (
                "format 4, in order, a delta and a glyph array",
                mapped_face(3, 1, ordered),
                vec![('A', 0x4B), ('B', 0x4C), ('C', 0x4D), ('a', 7)],
            ),
            (
                "format 4, out of order",
                mapped_face(3, 1, unordered),
                ('P'..='Z')
                    .map(|c| (c, u32::from(c) as u16 + 100))
                    .collect(),
            ),
            (
                "format 4 under (3,0): F0xx codes twice, low codes never",
                mapped_face(3, 0, symbol),
                vec![
                    (' ', 5),
                    ('A', 6),
                    ('\u{2022}', 8),
                    ('\u{F020}', 5),
                    ('\u{F041}', 6),
                ],
            ),
            (
                "format 12, in order, past the Basic Multilingual Plane",
                mapped_face(
                    3,
                    10,
                    format_groups(12, &[(0x41, 0x42, 5), (0x1_F600, 0x1_F601, 9)]),
                ),
                vec![('A', 5), ('B', 6), ('\u{1F600}', 9), ('\u{1F601}', 10)],
            ),
            (
                "format 12, overlapping groups, an overflow and a glyph 0",
                mapped_face(
                    3,
                    10,
                    format_groups(
                        12,
                        &[
                            (0x41, 0x45, 10),
                            (0x43, 0x48, 20),
                            (0x30, 0x31, 0xFFFF),
                            (0x32, 0x32, 0),
                            (0x5A, 0x59, 3),
                        ],
                    ),
                ),
                vec![
                    ('0', 0xFFFF),
                    ('A', 10),
                    ('B', 11),
                    ('C', 12),
                    ('D', 13),
                    ('E', 14),
                    ('F', 23),
                    ('G', 24),
                    ('H', 25),
                ],
            ),
            (
                "format 13, one glyph for a range",
                mapped_face(0, 6, format_groups(13, &[(0x41, 0x43, 7)])),
                vec![('A', 7), ('B', 7), ('C', 7)],
            ),
            (
                "format 0 under (1,0): nothing past U+007F",
                mapped_face(1, 0, format_0(&[(0x41, 3), (0x80, 4)])),
                vec![('A', 3)],
            ),
            (
                "format 6",
                mapped_face(3, 1, format_6(0x20, &[1, 0, 2])),
                vec![(' ', 1), ('"', 2)],
            ),
            (
                "format 2 under platform 0",
                mapped_face(0, 3, format_2()),
                vec![('A', 7), ('B', 8), ('\u{8140}', 1001)],
            ),
        ];
        for (what, data, expected) in &cases {
            let sfnt = Sfnt::parse(data).expect("a directory");
            let map = sfnt.char_map();
            assert_eq!(&map, expected, "{what}");
            assert_eq!(
                sfnt.code_points(),
                expected.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
                "{what}: the code points are the map's scalars"
            );
            assert_agrees_with_the_lookup(what, &sfnt, &map);
        }

        // The subtable chosen is the lookup's, and its record is named.
        let data = sfnt_with_tables(&[(
            b"cmap",
            cmap_of(&[(1, 0, format_0(&[(0x41, 3)])), (3, 1, format_6(0x41, &[4]))]),
        )]);
        let sfnt = Sfnt::parse(&data).expect("a directory");
        assert_eq!(sfnt.text_subtable(), Some((3, 1)));
        assert_eq!(sfnt.char_map(), vec![('A', 4)]);

        // A face with no cmap, or none of the records the lookup takes, or a
        // subtable of a format it does not read, maps nothing.
        for data in [
            sfnt_with(b"head", &head()),
            mapped_face(3, 2, format_6(0x41, &[4])),
            mapped_face(3, 1, vec![0, 99, 0, 0]),
        ] {
            let sfnt = Sfnt::parse(&data).expect("a directory");
            assert!(sfnt.char_map().is_empty());
            assert!(sfnt.code_points().is_empty());
        }
    }

    /// A subtable whose counts claim more than it holds is read as far as
    /// it goes: format 12's groups up to the first it does not hold whole,
    /// format 4's segments up to the first field it does not hold.
    #[test]
    fn an_overcounted_subtable_maps_what_it_holds() {
        let mut groups = format_groups(12, &[(0x41, 0x42, 5)]);
        groups[12..16].copy_from_slice(&u32::MAX.to_be_bytes());
        let data = mapped_face(3, 10, groups);
        let sfnt = Sfnt::parse(&data).expect("a directory");
        let map = sfnt.char_map();
        assert_eq!(map, vec![('A', 5), ('B', 6)]);
        assert_agrees_with_the_lookup("overcounted groups", &sfnt, &map);

        // Two segments, the subtable cut inside its start codes: the first
        // end is held and its start is not, so nothing maps.
        let mut segments = format_4_segments(&[
            Segment4 {
                start: 0x41,
                end: 0x42,
                delta: 1,
                array: None,
            },
            terminator(),
        ]);
        segments[6..8].copy_from_slice(&0xFFFEu16.to_be_bytes());
        segments.truncate(18);
        let data = mapped_face(3, 1, segments);
        let sfnt = Sfnt::parse(&data).expect("a directory");
        let map = sfnt.char_map();
        assert_agrees_with_the_lookup("overcounted segments", &sfnt, &map);
        assert!(map.is_empty(), "no start code is held: {map:?}");
    }

    /// FT-16.2: `post` names each glyph as its version states, the
    /// standard order's names transcribed from the `post` chapter, and the
    /// names read back to the glyphs `glyph_for_name` finds.
    #[test]
    fn post_names_glyphs_in_formats_1_2_and_3() {
        // 1.0: the standard order, as far as maxp's count reaches.
        let data = sfnt_with_tables(&[(b"maxp", maxp(40)), (b"post", post(0x0001_0000, &[]))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_name(0), Some(".notdef"));
        assert_eq!(face.glyph_name(1), Some(".null"));
        assert_eq!(face.glyph_name(3), Some("space"));
        assert_eq!(face.glyph_name(36), Some("A"));
        assert_eq!(face.glyph_name(39), Some("D"));
        assert_eq!(face.glyph_name(40), None, "past maxp's count");
        let data = sfnt_with_tables(&[(b"post", post(0x0001_0000, &[]))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(
            face.glyph_name(257),
            Some("dcroat"),
            "no maxp: the order whole"
        );
        assert_eq!(face.glyph_name(258), None);

        // 2.0: the standard order below 258, the table's strings from 258,
        // an index past its strings naming nothing.
        let body = post_2_body(&[0, 3, 168, 258, 259, 0, 300], &["alpha", "uni2665"]);
        let data = sfnt_with_tables(&[(b"post", post(0x0002_0000, &body))]);
        let face = Sfnt::parse(&data).expect("a directory");
        let names: Vec<Option<&str>> = (0..8).map(|glyph| face.glyph_name(glyph)).collect();
        assert_eq!(
            names,
            [
                Some(".notdef"),
                Some("space"),
                Some("Delta"),
                Some("alpha"),
                Some("uni2665"),
                Some(".notdef"),
                None,
                None,
            ]
        );
        for glyph in 0..5u16 {
            let name = face.glyph_name(glyph).expect("named");
            let first = (0..=glyph)
                .find(|g| face.glyph_name(*g) == Some(name))
                .expect("itself at least");
            assert_eq!(face.glyph_for_name(name), Some(first), "{name}");
        }

        // 2.5: a signed offset from the glyph's own index.
        let mut body = 5u16.to_be_bytes().to_vec();
        for offset in [0i8, 35, 1, 95, -2] {
            body.extend_from_slice(&offset.to_be_bytes());
        }
        let data = sfnt_with_tables(&[(b"post", post(0x0002_5000, &body))]);
        let face = Sfnt::parse(&data).expect("a directory");
        let names: Vec<Option<&str>> = (0..6).map(|glyph| face.glyph_name(glyph)).collect();
        assert_eq!(
            names,
            [
                Some(".notdef"),
                Some("A"),
                Some("space"),
                Some("Adieresis"),
                Some("nonmarkingreturn"),
                None,
            ]
        );
        // An offset that leaves the standard order names nothing.
        let mut body = 2u16.to_be_bytes().to_vec();
        body.extend_from_slice(&(-1i8).to_be_bytes());
        body.extend_from_slice(&127i8.to_be_bytes());
        let data = sfnt_with_tables(&[(b"post", post(0x0002_5000, &body))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_name(0), None, "index -1");
        assert_eq!(face.glyph_name(1), Some("ucircumflex"), "1 + 127 = 128");

        // 3.0, and a version no specification gives, name nothing.
        for version in [0x0003_0000u32, 0x0004_0000, 0] {
            let body = post_2_body(&[0, 3], &["alpha"]);
            let data = sfnt_with_tables(&[(b"post", post(version, &body))]);
            let face = Sfnt::parse(&data).expect("a directory");
            for glyph in 0..3 {
                assert_eq!(face.glyph_name(glyph), None, "{version:#x}");
            }
        }

        // A string cut short, a string that is not UTF-8, a table that ends
        // in its index array, and no table at all.
        let mut body = post_2_body(&[258, 259, 260], &["alpha", "beta", "gamma"]);
        body.truncate(body.len() - 2);
        let data = sfnt_with_tables(&[(b"post", post(0x0002_0000, &body))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_name(0), Some("alpha"));
        assert_eq!(face.glyph_name(1), Some("beta"));
        assert_eq!(face.glyph_name(2), None, "cut short");
        let mut body = post_2_body(&[258], &["x"]);
        let last = body.len() - 1;
        body[last] = 0xFF;
        let data = sfnt_with_tables(&[(b"post", post(0x0002_0000, &body))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_name(0), None, "not UTF-8");
        let mut body = 9u16.to_be_bytes().to_vec();
        body.extend_from_slice(&3u16.to_be_bytes());
        let data = sfnt_with_tables(&[(b"post", post(0x0002_0000, &body))]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_name(0), Some("space"));
        assert_eq!(face.glyph_name(1), None, "its index is not in the table");
        let data = sfnt_with(b"head", &[0u8; 54]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_name(0), None, "no post table");
    }

    /// A `glyf` and `loca` of six glyphs: an empty one, a simple one, a
    /// composite one, one whose header is written backwards, one too short
    /// to hold its header, and the end of `loca`.
    fn boxed_face(long: bool) -> Vec<u8> {
        let mut glyf = Vec::new();
        let header = |contours: i16, edges: [i16; 4], glyf: &mut Vec<u8>| {
            glyf.extend_from_slice(&contours.to_be_bytes());
            for edge in edges {
                glyf.extend_from_slice(&edge.to_be_bytes());
            }
        };
        header(1, [-10, -20, 500, 700], &mut glyf);
        header(-1, [5, 6, 7, 8], &mut glyf);
        header(1, [900, 800, -900, -800], &mut glyf);
        glyf.extend_from_slice(&[0, 1, 0, 0]);
        // Glyph 0 empty, 1 at 0, 2 at 10, 3 at 20, 4 at 30 (four bytes).
        let offsets = [0u32, 0, 10, 20, 30, 34];
        let loca: Vec<u8> = if long {
            offsets.iter().flat_map(|o| o.to_be_bytes()).collect()
        } else {
            offsets
                .iter()
                .flat_map(|o| ((o / 2) as u16).to_be_bytes())
                .collect()
        };
        let mut head = head();
        head[50..52].copy_from_slice(&i16::from(long).to_be_bytes());
        face(&[
            (b"glyf", &glyf),
            (b"head", &head),
            (b"loca", &loca),
            (b"maxp", &maxp(5)),
        ])
    }

    /// FT-16.2: a glyph's box is the four values its `glyf` header writes,
    /// found through `loca` in either of its forms.
    #[test]
    fn a_glyph_box_is_its_glyf_header() {
        let bounds = |x_min, y_min, x_max, y_max| Bounds {
            x_min,
            y_min,
            x_max,
            y_max,
        };
        for long in [false, true] {
            let data = boxed_face(long);
            let face = Sfnt::parse(&data).expect("a directory");
            assert_eq!(
                face.glyph_box(0),
                Some(bounds(0, 0, 0, 0)),
                "empty: the origin"
            );
            assert_eq!(face.glyph_box(1), Some(bounds(-10, -20, 500, 700)));
            assert_eq!(face.glyph_box(2), Some(bounds(5, 6, 7, 8)), "composite");
            assert_eq!(
                face.glyph_box(3),
                Some(bounds(900, 800, -900, -800)),
                "as written"
            );
            assert_eq!(face.glyph_box(4), None, "too short for a header");
            assert_eq!(face.glyph_box(5), None, "past loca");
            assert_eq!(face.glyph_box(u16::MAX), None);
            assert_eq!(face.loca_glyph_count(), Some(5), "six entries");
        }
        let data = sfnt_with(b"head", &head());
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(face.glyph_box(0), None, "no glyf or loca");
        assert_eq!(face.loca_glyph_count(), None);
        // An empty loca indexes nothing.
        let data = face_of_loca(&[]);
        assert_eq!(
            Sfnt::parse(&data).expect("a directory").loca_glyph_count(),
            Some(0)
        );
    }

    /// A face of `head` (long offsets) and the `loca` given.
    fn face_of_loca(loca: &[u8]) -> Vec<u8> {
        face(&[(b"head", &head()), (b"loca", loca)])
    }

    /// FT-16.2: the face's box is `head`'s four values as written.
    #[test]
    fn the_font_box_is_heads() {
        let mut head = head();
        for (at, value) in [(36, -100i16), (38, -200), (40, 1100), (42, 900)] {
            head[at..at + 2].copy_from_slice(&value.to_be_bytes());
        }
        let data = sfnt_with(b"head", &head);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(
            face.font_box(),
            Some(Bounds {
                x_min: -100,
                y_min: -200,
                x_max: 1100,
                y_max: 900,
            })
        );
        let data = sfnt_with(b"head", &head[..42]);
        assert_eq!(Sfnt::parse(&data).expect("a directory").font_box(), None);
        let data = sfnt_with(b"maxp", &maxp(1));
        assert_eq!(Sfnt::parse(&data).expect("a directory").font_box(), None);
    }

    /// An `OS/2` table of `version`, `len` bytes long, its fields written
    /// where it holds them.
    fn os2(version: u16, len: usize) -> Vec<u8> {
        let mut os2 = vec![0u8; len];
        let mut put = |at: usize, bytes: &[u8]| {
            if let Some(slot) = os2.get_mut(at..at + bytes.len()) {
                slot.copy_from_slice(bytes);
            }
        };
        put(0, &version.to_be_bytes());
        put(4, &700u16.to_be_bytes()); // usWeightClass
        put(6, &5u16.to_be_bytes()); // usWidthClass
        put(30, &0x0105u16.to_be_bytes()); // sFamilyClass
        put(32, &[2, 2, 6, 3, 5, 4, 5, 2, 3, 4]); // PANOSE
        put(62, &0x0201u16.to_be_bytes()); // fsSelection
        put(86, &(-1i16).to_be_bytes()); // sxHeight
        put(88, &1409i16.to_be_bytes()); // sCapHeight
        os2
    }

    /// FT-16.2: the style facts are `post`'s, `OS/2`'s and `head`'s fields
    /// as written, each `None` where its table does not hold it.
    #[test]
    fn the_style_facts_are_os2_posts_and_heads() {
        let mut post_table = post(0x0003_0000, &[]);
        post_table[4..8].copy_from_slice(&(-819_200i32).to_be_bytes()); // -12.5
        post_table[8..10].copy_from_slice(&(-100i16).to_be_bytes());
        post_table[10..12].copy_from_slice(&50i16.to_be_bytes());
        post_table[12..16].copy_from_slice(&1u32.to_be_bytes());
        let mut head = head();
        head[44..46].copy_from_slice(&3u16.to_be_bytes());
        let data = sfnt_with_tables(&[
            (b"OS/2", os2(2, 96)),
            (b"head", head),
            (b"post", post_table),
        ]);
        let face = Sfnt::parse(&data).expect("a directory");
        assert_eq!(
            face.style(),
            Style {
                italic_angle: Some(-12.5),
                underline_position: Some(-100),
                underline_thickness: Some(50),
                fixed_pitch: Some(true),
                weight_class: Some(700),
                width_class: Some(5),
                family_class: Some(0x0105),
                panose: Some([2, 2, 6, 3, 5, 4, 5, 2, 3, 4]),
                selection: Some(0x0201),
                x_height: Some(-1),
                cap_height: Some(1409),
                mac_style: Some(3),
            }
        );

        // Version 1 holds no heights, whatever its bytes; version 0 of
        // Apple's 78 bytes holds the rest.
        for (version, len) in [(1u16, 96usize), (0, 78)] {
            let data = sfnt_with_tables(&[(b"OS/2", os2(version, len))]);
            let style = Sfnt::parse(&data).expect("a directory").style();
            assert_eq!(style.x_height, None, "version {version}");
            assert_eq!(style.cap_height, None, "version {version}");
            assert_eq!(style.weight_class, Some(700), "version {version}");
            assert_eq!(style.selection, Some(0x0201), "version {version}");
        }
        // A version 2 table cut before its heights, and one cut before its
        // PANOSE.
        let data = sfnt_with_tables(&[(b"OS/2", os2(2, 87))]);
        let style = Sfnt::parse(&data).expect("a directory").style();
        assert_eq!((style.x_height, style.cap_height), (None, None));
        let data = sfnt_with_tables(&[(b"OS/2", os2(2, 40))]);
        let style = Sfnt::parse(&data).expect("a directory").style();
        assert_eq!(style.panose, None);
        assert_eq!(style.family_class, Some(0x0105));
        assert_eq!(style.selection, None);

        // An upright, proportional post; and no tables at all.
        let data = sfnt_with_tables(&[(b"post", post(0x0003_0000, &[]))]);
        let style = Sfnt::parse(&data).expect("a directory").style();
        assert_eq!(style.italic_angle, Some(0.0));
        assert_eq!(style.fixed_pitch, Some(false));
        let data = sfnt_with(b"maxp", &maxp(1));
        assert_eq!(
            Sfnt::parse(&data).expect("a directory").style(),
            Style::default()
        );
    }

    /// A collection's faces are read by their index in the `ttcf` header,
    /// and a face past its count, or past what the header holds, is none.
    #[test]
    fn a_collections_faces_are_read_by_index() {
        let head_of = |units: u16| {
            let mut head = head();
            head[18..20].copy_from_slice(&units.to_be_bytes());
            head
        };
        // Two directories, each one table, after a header of two offsets;
        // each directory's table offset moved by where it now sits.
        let first = face(&[(b"head", &head_of(1000))]);
        let second = face(&[(b"head", &head_of(2048))]);
        let header_len = 12 + 8;
        let moved = |directory: &[u8], by: usize| -> Vec<u8> {
            let mut moved = directory.to_vec();
            let offset = u32::from_be_bytes([moved[20], moved[21], moved[22], moved[23]]);
            moved[20..24].copy_from_slice(&(offset + by as u32).to_be_bytes());
            moved
        };
        let mut data = b"ttcf".to_vec();
        data.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        data.extend_from_slice(&2u32.to_be_bytes());
        data.extend_from_slice(&(header_len as u32).to_be_bytes());
        data.extend_from_slice(&((header_len + first.len()) as u32).to_be_bytes());
        data.extend_from_slice(&moved(&first, header_len));
        data.extend_from_slice(&moved(&second, header_len + first.len()));
        assert_eq!(Sfnt::faces(&data), 2);
        let units: Vec<Option<u16>> = (0..3)
            .map(|face| Sfnt::parse_face(&data, face).map(|sfnt| sfnt.units_per_em))
            .collect();
        assert_eq!(units, [Some(1000), Some(2048), None]);
        assert_eq!(
            Sfnt::parse(&data).map(|sfnt| sfnt.units_per_em),
            Some(1000),
            "parse is face 0"
        );

        // A count past the header's room is held to it.
        let mut overcounted = data.clone();
        overcounted[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        let room = ((overcounted.len() - 12) / 4) as u32;
        assert_eq!(Sfnt::faces(&overcounted), room);
        assert!(Sfnt::parse_face(&overcounted, room).is_none());
        assert!(Sfnt::parse_face(&overcounted, u32::MAX).is_none());

        // A plain face is face 0 alone; anything else has none.
        let plain = face(&[(b"head", &head())]);
        assert_eq!(Sfnt::faces(&plain), 1);
        assert!(Sfnt::parse_face(&plain, 0).is_some());
        assert!(Sfnt::parse_face(&plain, 1).is_none());
        assert_eq!(Sfnt::faces(b"not a font"), 0);
        assert_eq!(Sfnt::faces(b"ttcf"), 0);
        assert!(Sfnt::parse_face(b"ttcf", 0).is_none());
    }

    /// Every reader FT-16.2 adds, over one input.
    fn read_everything(data: &[u8]) {
        let faces = Sfnt::faces(data);
        for face in [0, 1, faces.saturating_sub(1), faces, u32::MAX] {
            let Some(sfnt) = Sfnt::parse_face(data, face) else {
                continue;
            };
            let map = sfnt.char_map();
            assert!(map.windows(2).all(|pair| pair[0].0 < pair[1].0));
            assert!(map.iter().all(|(_, glyph)| *glyph != 0));
            let _ = sfnt.code_points();
            let _ = sfnt.text_subtable();
            for glyph in (0..64u16).chain([u16::MAX - 1, u16::MAX]) {
                let _ = sfnt.glyph_name(glyph);
                let _ = sfnt.glyph_box(glyph);
            }
            let _ = sfnt.font_box();
            let _ = sfnt.style();
            let _ = sfnt.loca_glyph_count();
        }
    }

    /// A `cmap` table of one subtable of a format the lookup reads, under a
    /// record the lookup takes, its body arbitrary after the format.
    fn arbitrary_cmap() -> impl proptest::strategy::Strategy<Value = Vec<u8>> {
        use proptest::prelude::*;
        (
            prop_oneof![Just(0u16), Just(2), Just(4), Just(6), Just(12), Just(13)],
            prop_oneof![
                Just((3u16, 1u16)),
                Just((3, 10)),
                Just((3, 0)),
                Just((1, 0)),
                Just((0, 3))
            ],
            proptest::collection::vec(any::<u8>(), 0..512),
        )
            .prop_map(|(format, (platform, encoding), body)| {
                let mut sub = format.to_be_bytes().to_vec();
                sub.extend_from_slice(&body);
                cmap_of(&[(platform, encoding, sub)])
            })
    }

    /// A `post` version the readers tell apart.
    fn arbitrary_post_version() -> impl proptest::strategy::Strategy<Value = u32> {
        use proptest::prelude::*;
        prop_oneof![
            Just(0x0001_0000u32),
            Just(0x0002_0000),
            Just(0x0002_5000),
            Just(0x0003_0000),
            any::<u32>()
        ]
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

        /// Ruling 1: no reader FT-16.2 adds panics on arbitrary bytes, as a
        /// face or as a collection's header.
        #[test]
        fn the_measurement_readers_never_panic_on_arbitrary_bytes(
            data in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..1024)
        ) {
            read_everything(&data);
            let mut ttcf = b"ttcf".to_vec();
            ttcf.extend_from_slice(&data);
            read_everything(&ttcf);
        }

        /// Ruling 1, over a directory whose tables are arbitrary: the
        /// readers meet every count and offset a table can claim.
        #[test]
        fn the_measurement_readers_never_panic_on_arbitrary_tables(
            cmap in arbitrary_cmap(),
            post_body in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..512),
            version in arbitrary_post_version(),
            os2_body in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..128),
            head_body in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..64),
            loca in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..128),
            glyf in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..256),
            maxp_body in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..8),
        ) {
            let data = sfnt_with_tables(&[
                (b"OS/2", os2_body),
                (b"cmap", cmap),
                (b"glyf", glyf),
                (b"head", head_body),
                (b"loca", loca),
                (b"maxp", maxp_body),
                (b"post", post(version, &post_body)),
            ]);
            read_everything(&data);
        }
    }
}
