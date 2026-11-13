//! What a face states about itself, for a writer describing it beside the
//! program it embeds: its PostScript name, its embedding licence, its box and
//! vertical metrics, its italic angle, pitch and stem, and its family class.
//!
//! Read from the tables an sfnt keeps each in — `name`, `OS/2`, `head`,
//! `hhea`, `post` — and from a CFF's Name INDEX, Top DICT and Private DICT,
//! each length scaled to **thousandths of an em** by the face's own units per
//! em or `FontMatrix`. Every field is `None` where the face does not state
//! it: what a caller writes in its place is the caller's, and nothing here
//! guesses one.

use crate::cff::{dict_get, parse_dict, Cff, Index};
use crate::sfnt::Sfnt;

/// `name`.
const TAG_NAME: u32 = u32::from_be_bytes(*b"name");
/// `OS/2`.
const TAG_OS2: u32 = u32::from_be_bytes(*b"OS/2");
/// `head`.
const TAG_HEAD: u32 = u32::from_be_bytes(*b"head");
/// `hhea`.
const TAG_HHEA: u32 = u32::from_be_bytes(*b"hhea");
/// `post`.
const TAG_POST: u32 = u32::from_be_bytes(*b"post");
/// `glyf`.
const TAG_GLYF: u32 = u32::from_be_bytes(*b"glyf");
/// `CFF `.
const TAG_CFF: u32 = u32::from_be_bytes(*b"CFF ");

/// The longest PostScript name the OpenType `name` table's ID 6 admits.
const MAX_POSTSCRIPT_NAME: usize = 63;

/// What a face states about itself ([`face_facts`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FaceFacts {
    /// The face's PostScript name: `name` ID 6, else a CFF's Name INDEX
    /// entry, where it is one to 63 printable ASCII characters outside
    /// `[](){}<>/%` — the characters OpenType's `name` chapter admits in ID 6.
    pub postscript_name: Option<String>,
    /// `OS/2`'s `fsType`, the face's embedding licence, as stated; `None` for
    /// a face with no `OS/2` table, a bare CFF among them.
    pub fs_type: Option<u16>,
    /// `OS/2`'s `sFamilyClass`: the class in the high byte, the subclass in
    /// the low.
    pub family_class: Option<u16>,
    /// Whether the face says it is italic: `OS/2`'s `fsSelection` bit 0, or
    /// `head`'s `macStyle` bit 1; `None` where it has neither table.
    pub italic: Option<bool>,
    /// Whether every glyph is one width: `post`'s `isFixedPitch`, or a CFF Top
    /// DICT's `isFixedPitch`.
    pub fixed_pitch: Option<bool>,
    /// `[x_min, y_min, x_max, y_max]`: `head`'s box, or a CFF Top DICT's
    /// `FontBBox`.
    pub bbox: Option<[f64; 4]>,
    /// How far above the baseline the face reaches: `hhea`'s ascender, else
    /// `OS/2`'s typographic ascender, else — for a bare CFF, which has
    /// neither — the top of its box.
    pub ascent: Option<f64>,
    /// How far below it, negative: `hhea`'s descender, else `OS/2`'s, else the
    /// bottom of a bare CFF's box.
    pub descent: Option<f64>,
    /// `OS/2`'s `sCapHeight`, where its version (2 and up) states one, else
    /// the top of the glyph `H` reaches — through the `cmap`, or a CFF's
    /// charset by name.
    pub cap_height: Option<f64>,
    /// `OS/2`'s `sxHeight`, where its version states one, else the top of the
    /// glyph `x` reaches.
    pub x_height: Option<f64>,
    /// The angle of the face's dominant vertical strokes, in degrees
    /// counterclockwise from the vertical: `post`'s `italicAngle`, or a CFF
    /// Top DICT's `ItalicAngle`.
    pub italic_angle: Option<f64>,
    /// The dominant vertical stem: a CFF Private DICT's `StdVW`. An sfnt
    /// with `glyf` outlines states none.
    pub stem_v: Option<f64>,
}

/// What `program` — an sfnt, `glyf` or `CFF ` flavoured, or a bare CFF —
/// states about itself; `None` for bytes neither reads.
#[must_use]
pub fn face_facts(program: &[u8]) -> Option<FaceFacts> {
    if let Some(sfnt) = Sfnt::parse(program) {
        return Some(sfnt_facts(&sfnt));
    }
    let cff = Cff::parse(program)?;
    Some(cff_facts(program, &cff, true))
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]))
}

fn be_i16(data: &[u8], at: usize) -> Option<i16> {
    be16(data, at).map(|value| value as i16)
}

fn be32(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn sfnt_facts(sfnt: &Sfnt<'_>) -> FaceFacts {
    let scale = 1000.0 / f64::from(sfnt.units_per_em.max(1));
    let units = |value: i16| f64::from(value) * scale;

    let head = sfnt.table(TAG_HEAD);
    let hhea = sfnt.table(TAG_HHEA);
    let os2 = sfnt.table(TAG_OS2);
    let post = sfnt.table(TAG_POST);
    // An `OTTO` face keeps its outlines, and its stems, in a CFF.
    let cff = sfnt
        .table(TAG_GLYF)
        .is_none()
        .then(|| sfnt.table(TAG_CFF))
        .flatten()
        .and_then(|table| Cff::parse(table).map(|cff| (table, cff)));

    let bbox = head.and_then(|head| {
        Some([
            units(be_i16(head, 36)?),
            units(be_i16(head, 38)?),
            units(be_i16(head, 40)?),
            units(be_i16(head, 42)?),
        ])
    });
    let version = os2.and_then(|os2| be16(os2, 0));
    // OS/2 version 2 adds `sxHeight` at 86 and `sCapHeight` at 88; zero is
    // the table's own "not stated".
    let stated_height = |at: usize| {
        os2.filter(|_| version.is_some_and(|v| v >= 2))
            .and_then(|os2| be_i16(os2, at))
            .filter(|value| *value > 0)
            .map(units)
    };
    let typo = |at: usize| os2.and_then(|os2| be_i16(os2, at)).map(units);
    let hhea_metric = |at: usize| hhea.and_then(|hhea| be_i16(hhea, at)).map(units);

    let fs_selection = os2.and_then(|os2| be16(os2, 62));
    let mac_style = head.and_then(|head| be16(head, 44));
    let italic = match (fs_selection, mac_style) {
        (None, None) => None,
        (selection, style) => Some(
            selection.is_some_and(|bits| bits & 1 != 0)
                || style.is_some_and(|bits| bits & 0b10 != 0),
        ),
    };

    let top = |c: char| {
        let glyph = sfnt.glyph_for_char(c).filter(|glyph| *glyph != 0)?;
        match &cff {
            Some((_, cff)) => outline_top(cff, glyph).map(|top| top * cff_scale(cff, glyph)),
            None => crate::glyf::bounds(sfnt, glyph)
                .filter(|bounds| *bounds != crate::glyf::Bounds::default())
                .map(|bounds| units(bounds.y_max)),
        }
    };

    let inner = cff
        .as_ref()
        .map(|(table, cff)| cff_facts(table, cff, false))
        .unwrap_or_default();

    FaceFacts {
        postscript_name: postscript_name(sfnt).or(inner.postscript_name),
        fs_type: os2.and_then(|os2| be16(os2, 8)),
        family_class: os2.and_then(|os2| be16(os2, 30)),
        italic,
        fixed_pitch: post.and_then(|post| be32(post, 12)).map(|pitch| pitch != 0),
        bbox,
        ascent: hhea_metric(4).or_else(|| typo(68)),
        descent: hhea_metric(6).or_else(|| typo(70)),
        cap_height: stated_height(88).or_else(|| top('H')),
        x_height: stated_height(86).or_else(|| top('x')),
        italic_angle: post
            .and_then(|post| be32(post, 4))
            .map(|fixed| f64::from(fixed as i32) / 65536.0),
        stem_v: inner.stem_v,
    }
}

/// A CFF's facts. `bare` says the program is the whole answer, with no sfnt
/// around it to state its vertical metrics, which then come from its box.
fn cff_facts(program: &[u8], cff: &Cff<'_>, bare: bool) -> FaceFacts {
    let header_size = program.get(2).map_or(4, |size| usize::from(*size));
    let parsed = Index::parse(program, header_size).and_then(|(names, at)| {
        let (tops, _) = Index::parse(program, at)?;
        Some((names, parse_dict(tops.get(0)?)))
    });
    let Some((names, top)) = parsed else {
        return FaceFacts::default();
    };
    let operands = |op: u16| dict_get(&top, op);
    let first = |op: u16| operands(op).and_then(<[f64]>::first).copied();

    // The Top DICT's `FontMatrix` scales glyph space to an em; a CID-keyed
    // font's per-Font-DICT matrices are the glyphs' own, read where a glyph
    // is.
    let scale = top_scale(cff);
    let bbox = operands(5)
        .filter(|values| values.len() >= 4)
        .map(|values| [values[0], values[1], values[2], values[3]].map(|value| value * scale));

    // `StdVW` (11) in the Private DICT `Private` (18) points at — the Top
    // DICT's, else the first Font DICT's in a CID-keyed font.
    let pair = |operands: &[f64]| Some((*operands.first()?, *operands.get(1)?));
    let private_at = operands(18).and_then(pair).or_else(|| {
        let (fds, _) = Index::parse(program, offset(first(0x0C24)?)?)?;
        let dict = parse_dict(fds.get(0)?);
        dict_get(&dict, 18).and_then(pair)
    });
    let stem_v = private_at.and_then(|(size, at)| {
        let (size, at) = (offset(size)?, offset(at)?);
        let private = parse_dict(program.get(at..at.checked_add(size)?)?);
        dict_get(&private, 11)
            .and_then(<[f64]>::first)
            .map(|stem| stem * scale)
    });

    let top_of = |name: &str| {
        let glyph = cff.gid_for_name(name).filter(|glyph| *glyph != 0)?;
        outline_top(cff, glyph).map(|top| top * cff_scale(cff, glyph))
    };

    FaceFacts {
        postscript_name: names
            .get(0)
            .and_then(|name| core::str::from_utf8(name).ok())
            .filter(|name| is_postscript_name(name))
            .map(str::to_string),
        fs_type: None,
        family_class: None,
        italic: None,
        fixed_pitch: first(0x0C01).map(|pitch| pitch != 0.0),
        bbox,
        ascent: bbox.filter(|_| bare).map(|bbox| bbox[3]),
        descent: bbox.filter(|_| bare).map(|bbox| bbox[1]),
        cap_height: top_of("H"),
        x_height: top_of("x"),
        italic_angle: Some(first(0x0C02).unwrap_or(0.0)),
        stem_v,
    }
}

/// A DICT operand as a position or a size, refusing anything that is not one.
fn offset(value: f64) -> Option<usize> {
    (value.is_finite() && (0.0..=2_147_483_647.0).contains(&value)).then_some(value as usize)
}

/// Thousandths of an em per unit of the Top DICT's `FontMatrix`.
fn top_scale(cff: &Cff<'_>) -> f64 {
    // Glyph 0's matrix is the Top DICT's wherever no Font DICT overrides it,
    // which is every font that is not CID-keyed.
    cff_scale(cff, 0)
}

/// Thousandths of an em per charstring unit of `glyph`.
fn cff_scale(cff: &Cff<'_>, glyph: u16) -> f64 {
    let a = cff.font_matrix_for(glyph)[0];
    if a.is_finite() && a != 0.0 {
        a * 1000.0
    } else {
        1.0
    }
}

/// The highest point of a CFF glyph's outline, control points included, in
/// charstring units.
fn outline_top(cff: &Cff<'_>, glyph: u16) -> Option<f64> {
    use crate::outline::Segment;
    let outline = cff.outline(glyph)?;
    outline
        .segments
        .iter()
        .flat_map(|segment| match *segment {
            Segment::MoveTo { y, .. } | Segment::LineTo { y, .. } => [Some(y), None, None],
            Segment::QuadTo { cy, y, .. } => [Some(cy), Some(y), None],
            Segment::CurveTo { c1y, c2y, y, .. } => [Some(c1y), Some(c2y), Some(y)],
            Segment::Close => [None, None, None],
        })
        .flatten()
        .filter(|y| y.is_finite())
        .reduce(f64::max)
}

/// Whether `name` is a PostScript name OpenType's `name` ID 6 admits: one to
/// 63 characters, each printable ASCII and none of `[](){}<>/%`.
fn is_postscript_name(name: &str) -> bool {
    (1..=MAX_POSTSCRIPT_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|b| (33..=126).contains(&b) && !b"[](){}<>/%".contains(&b))
}

/// `name` ID 6: a Windows Unicode record first, then a Unicode-platform one,
/// then a Macintosh Roman one, the first readable record of each kind.
fn postscript_name(sfnt: &Sfnt<'_>) -> Option<String> {
    let table = sfnt.table(TAG_NAME)?;
    let count = usize::from(be16(table, 2)?);
    let storage = usize::from(be16(table, 4)?);
    let mut best: Option<(u8, String)> = None;
    for record in 0..count.min(1024) {
        let at = 6 + record * 12;
        let (Some(platform), Some(encoding), Some(id), Some(length), Some(start)) = (
            be16(table, at),
            be16(table, at + 2),
            be16(table, at + 6),
            be16(table, at + 8),
            be16(table, at + 10),
        ) else {
            break;
        };
        if id != 6 {
            continue;
        }
        let from = storage.checked_add(usize::from(start))?;
        let Some(bytes) = table.get(from..from.saturating_add(usize::from(length))) else {
            continue;
        };
        let (rank, text) = match (platform, encoding) {
            (3, 0 | 1) => (0, utf16be(bytes)),
            (0, _) => (1, utf16be(bytes)),
            (1, 0) => (
                2,
                bytes
                    .is_ascii()
                    .then(|| bytes.iter().map(|b| char::from(*b)).collect()),
            ),
            _ => continue,
        };
        let Some(text) = text.filter(|text| is_postscript_name(text)) else {
            continue;
        };
        if best.as_ref().is_none_or(|(held, _)| rank < *held) {
            best = Some((rank, text));
        }
    }
    best.map(|(_, text)| text)
}

/// UTF-16BE, refused where a unit pair does not decode.
fn utf16be(bytes: &[u8]) -> Option<String> {
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

#[cfg(test)]
mod tests;
