//! TrueType and OpenType font programs: the tables metrics need.
//!
//! Wave 1 reads only what text extraction and layout require — the table
//! directory, `head`, `hhea`, `hmtx`, `cmap` and `post`. Glyph outlines are
//! wave 2; nothing here touches `glyf` or `CFF `.
//!
//! Beside the one `cmap` subtable text is looked up in, two more are read on
//! their own for the route `docs/design/symbol-faces.md` settles: a Unicode
//! subtable, asked for a glyph name's value, and a (3,0) symbol subtable, asked
//! for a code.
//!
//! Every read is bounds-checked against the buffer the caller supplied,
//! because a font program is document-controlled data like any other.

use std::sync::OnceLock;

/// A parsed table directory over borrowed bytes.
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

/// The `cmap` subtable a face's characters are looked up in (9.6.6.4).
#[derive(Clone, Copy, Debug)]
struct CmapChoice {
    score: u32,
    platform: u16,
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
        Sfnt::parse_with(data, &mut 0)
    }

    /// [`Sfnt::parse`], and the steps it took
    /// (`docs/design/resource-exhaustion.md`, "The counters"): a table record
    /// read for each table the directory lists, and a record compared for
    /// each comparison the lookups of `head` and `hhea` made.
    #[must_use]
    pub fn parse_counted(data: &'a [u8]) -> (Option<Sfnt<'a>>, u64) {
        let mut steps = 0u64;
        let font = Sfnt::parse_with(data, &mut steps);
        (font, steps)
    }

    fn parse_with(data: &'a [u8], steps: &mut u64) -> Option<Sfnt<'a>> {
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
            data,
            tables,
            version: tag,
            units_per_em: 1000,
            num_h_metrics: 0,
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
        let mut best: Option<(u32, u16, usize)> = None;
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
            if score > 0 && best.is_none_or(|(b, _, _)| score > b) {
                best = Some((score, platform, offset as usize));
            }
        }

        let (score, platform, offset) = best?;
        let ordered = cmap.get(offset..).is_some_and(|sub| in_order(sub, steps));
        Some(CmapChoice {
            score,
            platform,
            offset,
            ordered,
        })
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
        let mut best: Option<(u32, u16, usize)> = None;
        for i in 0..count.min(64) {
            let at = 4 + i * 8;
            let (Some(platform), Some(encoding), Some(offset)) =
                (be16(cmap, at), be16(cmap, at + 2), be32(cmap, at + 4))
            else {
                break;
            };
            let scored = score(platform, encoding);
            if scored > 0 && best.is_none_or(|(b, _, _)| scored > b) {
                best = Some((scored, platform, offset as usize));
            }
        }
        let (score, platform, offset) = best?;
        let ordered = cmap.get(offset..).is_some_and(|sub| in_order(sub, &mut 0));
        Some(CmapChoice {
            score,
            platform,
            offset,
            ordered,
        })
    }
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
}
