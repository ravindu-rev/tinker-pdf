//! The route from a code of a symbolic standard font to a substitute face's
//! glyph (`docs/design/symbol-faces.md`, "The route").
//!
//! Symbol and ZapfDingbats name their glyphs, and a face handed over in their
//! place — a TrueType or OpenType face, or a bare CFF program — is asked for
//! the code's name first, then for the name's Unicode value, then for the code
//! itself through a (3,0) subtable. The census H-94 is ruled with counts
//! through this function, and the facade draws through it, so the census's
//! figures are the drawing's.

use crate::cff::Cff;
use crate::encoding::{glyph_name_to_char, BuiltInEncoding};
use crate::sfnt::Sfnt;

/// `glyf`, whose absence makes an OpenType face's outlines CFF.
const TAG_GLYF: u32 = 0x676C_7966;
/// `CFF `, where such a face keeps them, names included.
const TAG_CFF: u32 = 0x4346_4620;

/// Which step of the route reached a glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolRoute {
    /// The face's own glyph names: a `glyf` face's `post` table, or the
    /// charset of an OpenType CFF face or a bare CFF program.
    Name,
    /// The name's Unicode value through the face's Unicode subtable.
    Unicode,
    /// The code through a (3,0) symbol subtable, at `0xF000` plus the code.
    SymbolCode,
}

/// The glyph a substitute `program` draws for `code` of a font whose built-in
/// encoding is `encoding`, the code's glyph name being `name` — the name the
/// document gives it, else the built-in encoding's — and the step that
/// reached it.
///
/// The first of these that reaches a glyph other than glyph 0, which is
/// `.notdef` in every face and no answer:
///
/// 1. the face's own glyph names, asked for `name`;
/// 2. `name`'s Unicode value — `encoding`'s list's
///    ([`BuiltInEncoding::unicode_of`]), else [`glyph_name_to_char`]'s —
///    through the face's Unicode subtable ([`Sfnt::glyph_for_unicode`]);
/// 3. `code` through the face's (3,0) symbol subtable
///    ([`Sfnt::glyph_for_symbol_code`]), today's reading of a substituted
///    symbolic face, kept last.
///
/// A code with no name takes the third step alone. A bare CFF program has no
/// `cmap`, so only the first step is open to it. `None` where nothing reaches
/// a glyph, or `program` is neither an sfnt nor a CFF program.
#[must_use]
pub fn substitute_glyph(
    program: &[u8],
    encoding: BuiltInEncoding,
    name: Option<&str>,
    code: u8,
) -> Option<(u16, SymbolRoute)> {
    let reached = |glyph: Option<u16>, route| glyph.filter(|g| *g != 0).map(|g| (g, route));

    if let Some(face) = Sfnt::parse(program) {
        if let Some(name) = name {
            // An OpenType face with no `glyf` keeps its outlines, and its
            // names, in its `CFF ` table.
            let named = if face.table(TAG_GLYF).is_none() {
                face.table(TAG_CFF)
                    .and_then(Cff::parse)
                    .and_then(|cff| cff.gid_for_name(name))
            } else {
                face.glyph_for_name(name)
            };
            if let Some(found) = reached(named, SymbolRoute::Name) {
                return Some(found);
            }
            let value = encoding
                .unicode_of(name)
                .or_else(|| glyph_name_to_char(name));
            if let Some(found) = reached(
                value.and_then(|c| face.glyph_for_unicode(c)),
                SymbolRoute::Unicode,
            ) {
                return Some(found);
            }
        }
        return reached(face.glyph_for_symbol_code(code), SymbolRoute::SymbolCode);
    }

    let cff = Cff::parse(program)?;
    reached(cff.gid_for_name(name?), SymbolRoute::Name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use BuiltInEncoding::{Symbol, ZapfDingbats};
    use SymbolRoute::{Name, SymbolCode, Unicode};

    const TRUE_TYPE: u32 = 0x0001_0000;
    const OTTO: u32 = 0x4F54_544F;

    /// An sfnt of the version given holding every table given, in that order.
    fn sfnt(version: u32, tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = version.to_be_bytes().to_vec();
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

    /// A version 2.0 `post` table: each glyph's index, then its strings.
    fn post_2(indices: &[u16], strings: &[&str]) -> Vec<u8> {
        let mut out = 0x0002_0000u32.to_be_bytes().to_vec();
        out.extend_from_slice(&[0; 28]);
        out.extend_from_slice(&(indices.len() as u16).to_be_bytes());
        for index in indices {
            out.extend_from_slice(&index.to_be_bytes());
        }
        for string in strings {
            out.push(string.len() as u8);
            out.extend_from_slice(string.as_bytes());
        }
        out
    }

    /// A version 3.0 `post` table, which names nothing.
    fn post_3() -> Vec<u8> {
        let mut out = 0x0003_0000u32.to_be_bytes().to_vec();
        out.extend_from_slice(&[0; 28]);
        out
    }

    /// A format 4 subtable mapping each code to its glyph, a segment each,
    /// and the closing segment.
    fn format_4(pairs: &[(u16, u16)]) -> Vec<u8> {
        let mut pairs = pairs.to_vec();
        pairs.sort_unstable();
        let segments = pairs.len() + 1;
        let mut out = Vec::new();
        out.extend_from_slice(&4u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        out.extend_from_slice(&((segments * 2) as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        for (code, _) in &pairs {
            out.extend_from_slice(&code.to_be_bytes());
        }
        out.extend_from_slice(&0xFFFFu16.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes());
        for (code, _) in &pairs {
            out.extend_from_slice(&code.to_be_bytes());
        }
        out.extend_from_slice(&0xFFFFu16.to_be_bytes());
        for (code, glyph) in &pairs {
            out.extend_from_slice(&glyph.wrapping_sub(*code).to_be_bytes());
        }
        out.extend_from_slice(&1u16.to_be_bytes());
        for _ in 0..segments {
            out.extend_from_slice(&0u16.to_be_bytes());
        }
        out
    }

    /// A `cmap` table of one subtable per record.
    fn cmap_of(records: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_be_bytes());
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

    /// A CFF INDEX of `entries`, with one-byte offsets.
    fn index(entries: &[&[u8]]) -> Vec<u8> {
        let mut out = (entries.len() as u16).to_be_bytes().to_vec();
        if entries.is_empty() {
            return out;
        }
        out.push(1);
        let mut offset = 1usize;
        out.push(offset as u8);
        for entry in entries {
            offset += entry.len();
            out.push(offset as u8);
        }
        for entry in entries {
            out.extend_from_slice(entry);
        }
        out
    }

    /// A bare CFF program whose glyph 0 is `.notdef` and glyph `n` is named
    /// `names[n - 1]`, each a string of its own from SID 391 on, every
    /// charstring a bare `endchar`.
    fn cff_named(names: &[&str]) -> Vec<u8> {
        let header = vec![1u8, 0, 4, 1];
        let name_index = index(&[b"T"]);
        let strings: Vec<&[u8]> = names.iter().map(|name| name.as_bytes()).collect();
        let string_index = index(&strings);
        let global_subrs = index(&[]);
        let endchar: &[u8] = &[14];
        let charstrings = index(&vec![endchar; names.len() + 1]);
        let mut charset = vec![0u8]; // format 0
        for sid in 391..391 + names.len() {
            charset.extend_from_slice(&(sid as u16).to_be_bytes());
        }
        // The Top DICT is two five-byte integers and their operators, so its
        // INDEX is 17 bytes and every offset after it is known before it is.
        let top_index_len = 2 + 1 + 2 + 12;
        let charset_at = header.len()
            + name_index.len()
            + top_index_len
            + string_index.len()
            + global_subrs.len();
        let charstrings_at = charset_at + charset.len();
        let mut top = vec![29];
        top.extend_from_slice(&(charset_at as i32).to_be_bytes());
        top.push(15); // charset
        top.push(29);
        top.extend_from_slice(&(charstrings_at as i32).to_be_bytes());
        top.push(17); // CharStrings
        let top_index = index(&[&top]);
        assert_eq!(top_index.len(), top_index_len);
        [
            header,
            name_index,
            top_index,
            string_index,
            global_subrs,
            charset,
            charstrings,
        ]
        .concat()
    }

    /// FT-03.3: the face's own name first, the name's Unicode value next, the
    /// code through the (3,0) subtable last, and a code with no name takes
    /// the last step alone.
    #[test]
    fn the_route_takes_its_steps_in_order() {
        let cmap = cmap_of(&[
            (3, 0, format_4(&[(0xF061, 5), (0xF021, 7)])),
            (3, 1, format_4(&[(0x03B1, 4), (0x2701, 6), (0x2665, 8)])),
        ]);
        let named = post_2(&[0, 0, 0, 258, 0, 0], &["alpha"]);
        let full = sfnt(
            TRUE_TYPE,
            &[
                (b"cmap", cmap.clone()),
                (b"glyf", Vec::new()),
                (b"post", named),
            ],
        );
        assert_eq!(
            substitute_glyph(&full, Symbol, Some("alpha"), 0x61),
            Some((3, Name))
        );

        let unnamed = sfnt(
            TRUE_TYPE,
            &[
                (b"cmap", cmap.clone()),
                (b"glyf", Vec::new()),
                (b"post", post_3()),
            ],
        );
        assert_eq!(
            substitute_glyph(&unnamed, Symbol, Some("alpha"), 0x61),
            Some((4, Unicode)),
            "no name in post: the Glyph List's U+03B1"
        );

        let symbol_only = sfnt(
            TRUE_TYPE,
            &[
                (b"cmap", cmap_of(&[(3, 0, format_4(&[(0xF061, 5)]))])),
                (b"glyf", Vec::new()),
                (b"post", post_3()),
            ],
        );
        assert_eq!(
            substitute_glyph(&symbol_only, Symbol, Some("alpha"), 0x61),
            Some((5, SymbolCode)),
            "no name and no Unicode subtable: today's reading"
        );

        assert_eq!(
            substitute_glyph(&full, Symbol, None, 0x61),
            Some((5, SymbolCode)),
            "a code with no name skips the first two steps"
        );
        assert_eq!(substitute_glyph(&full, Symbol, Some("beta"), 0x62), None);

        // Each font's list gives the value: `a1` is U+2701 for ZapfDingbats
        // and has no value for Symbol, whose code 0x21 then reaches (3,0).
        assert_eq!(
            substitute_glyph(&unnamed, ZapfDingbats, Some("a1"), 0x21),
            Some((6, Unicode))
        );
        assert_eq!(
            substitute_glyph(&unnamed, Symbol, Some("a1"), 0x21),
            Some((7, SymbolCode))
        );
        // A name neither list holds takes the general forms.
        assert_eq!(
            substitute_glyph(&unnamed, Symbol, Some("uni2665"), 0x40),
            Some((8, Unicode))
        );

        assert_eq!(
            substitute_glyph(b"not a font", Symbol, Some("alpha"), 0x61),
            None
        );
        assert_eq!(substitute_glyph(&[], ZapfDingbats, None, 0x21), None);
    }

    /// FT-03.3: glyph 0 is `.notdef` and no answer, so a name or a value that
    /// reaches it falls to the next step, and the last step's glyph 0 is no
    /// glyph.
    #[test]
    fn a_name_reaching_notdef_falls_to_the_next_step() {
        let named_notdef = post_2(&[258, 0, 0], &["alpha"]);
        let face = sfnt(
            TRUE_TYPE,
            &[
                (
                    b"cmap",
                    cmap_of(&[
                        (3, 0, format_4(&[(0xF061, 2)])),
                        (3, 1, format_4(&[(0x03B1, 0)])),
                    ]),
                ),
                (b"glyf", Vec::new()),
                (b"post", named_notdef.clone()),
            ],
        );
        assert_eq!(
            substitute_glyph(&face, Symbol, Some("alpha"), 0x61),
            Some((2, SymbolCode)),
            "the name and the value both reach glyph 0"
        );

        let face = sfnt(
            TRUE_TYPE,
            &[
                (b"cmap", cmap_of(&[(3, 1, format_4(&[(0x03B1, 1)]))])),
                (b"glyf", Vec::new()),
                (b"post", named_notdef),
            ],
        );
        assert_eq!(
            substitute_glyph(&face, Symbol, Some("alpha"), 0x61),
            Some((1, Unicode))
        );

        let face = sfnt(
            TRUE_TYPE,
            &[
                (b"cmap", cmap_of(&[(3, 0, format_4(&[(0xF061, 0)]))])),
                (b"glyf", Vec::new()),
            ],
        );
        assert_eq!(substitute_glyph(&face, Symbol, Some("alpha"), 0x61), None);

        // `.notdef` asked by name is glyph 0 in a charset as in `post`.
        let cff = cff_named(&["alpha"]);
        assert_eq!(substitute_glyph(&cff, Symbol, Some(".notdef"), 0x61), None);
    }

    /// FT-03.3: a bare CFF program is asked by its charset alone, having no
    /// `cmap`; inside an OpenType face the charset still names the glyphs, and
    /// the face's subtables answer what it does not.
    #[test]
    fn a_bare_cff_is_asked_by_its_charset() {
        let cff = cff_named(&["alpha", "beta", "a1"]);
        assert!(
            Cff::parse(&cff).is_some(),
            "the program the test builds parses"
        );
        assert_eq!(
            substitute_glyph(&cff, Symbol, Some("alpha"), 0x61),
            Some((1, Name))
        );
        assert_eq!(
            substitute_glyph(&cff, Symbol, Some("beta"), 0x62),
            Some((2, Name))
        );
        assert_eq!(
            substitute_glyph(&cff, ZapfDingbats, Some("a1"), 0x21),
            Some((3, Name))
        );
        assert_eq!(
            substitute_glyph(&cff, Symbol, Some("gamma"), 0x67),
            None,
            "a name the charset lacks, and no cmap to fall to"
        );
        assert_eq!(
            substitute_glyph(&cff, Symbol, None, 0x61),
            None,
            "no name, and no (3,0) subtable to ask"
        );

        let cmap = cmap_of(&[
            (3, 0, format_4(&[(0xF064, 2)])),
            (3, 1, format_4(&[(0x03B3, 3)])),
        ]);
        let otto = sfnt(OTTO, &[(b"CFF ", cff.clone()), (b"cmap", cmap.clone())]);
        assert_eq!(
            substitute_glyph(&otto, Symbol, Some("alpha"), 0x61),
            Some((1, Name))
        );
        assert_eq!(
            substitute_glyph(&otto, Symbol, Some("gamma"), 0x67),
            Some((3, Unicode))
        );
        assert_eq!(
            substitute_glyph(&otto, Symbol, Some("delta"), 0x64),
            Some((2, SymbolCode))
        );

        // A `CFF ` table that does not parse names nothing, and the face's
        // subtables still answer.
        let broken = sfnt(OTTO, &[(b"CFF ", vec![1, 0]), (b"cmap", cmap)]);
        assert_eq!(
            substitute_glyph(&broken, Symbol, Some("gamma"), 0x67),
            Some((3, Unicode))
        );
        assert_eq!(
            substitute_glyph(&broken, Symbol, Some("alpha"), 0x64),
            Some((2, SymbolCode))
        );
    }

    /// Ruling 1: no byte string makes the route panic. Every prefix of each
    /// program the tests build, and each with a byte flipped, reaches an
    /// answer or `None`.
    #[test]
    fn a_truncated_or_damaged_program_answers_or_declines() {
        let cmap = cmap_of(&[
            (3, 0, format_4(&[(0xF061, 5)])),
            (3, 1, format_4(&[(0x03B1, 4)])),
        ]);
        let programs = [
            sfnt(
                TRUE_TYPE,
                &[
                    (b"cmap", cmap.clone()),
                    (b"glyf", Vec::new()),
                    (b"post", post_2(&[0, 258], &["alpha"])),
                ],
            ),
            sfnt(OTTO, &[(b"CFF ", cff_named(&["alpha"])), (b"cmap", cmap)]),
            cff_named(&["alpha", "beta"]),
        ];
        for program in &programs {
            for end in 0..=program.len() {
                let prefix = &program[..end];
                for name in [Some("alpha"), Some(""), None] {
                    let _ = substitute_glyph(prefix, Symbol, name, 0x61);
                    let _ = substitute_glyph(prefix, ZapfDingbats, name, 0x21);
                }
            }
            for at in 0..program.len() {
                let mut damaged = program.clone();
                damaged[at] ^= 0xFF;
                let _ = substitute_glyph(&damaged, Symbol, Some("alpha"), 0x61);
            }
        }
    }
}
