//! Simple-font encodings (Annex D) and glyph names.
//!
//! A simple font maps a byte to a glyph name, and the glyph name to a
//! character. Both halves are needed: the name reaches the font program's
//! outline, and the character is what text extraction reports.

/// The four base encodings a simple font may name (9.6.5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BaseEncoding {
    /// `/StandardEncoding`: Adobe's original, and the implicit default for a
    /// non-symbolic Type 1 font with no `/Encoding`.
    #[default]
    Standard,
    /// `/WinAnsiEncoding`: Windows code page 1252.
    WinAnsi,
    /// `/MacRomanEncoding`.
    MacRoman,
    /// `/PDFDocEncoding`, which appears in text strings rather than content,
    /// but a font may still name it.
    PdfDoc,
}

/// Names differing from Latin-1 in StandardEncoding, by code (Annex D.2).
///
/// The eight names from `Oslash` on were missing until August 2026, which no
/// test noticed because the codes they sit at have no Latin-1 meaning either
/// — the table returned `None` and text extraction reported nothing for them.
/// Glyph *selection* is what made it visible: a CFF font's standard-encoding
/// fallback resolves a code to a name and the name to a glyph, so a missing
/// name is a glyph that draws as `.notdef`.
const STANDARD_HIGH: [(u8, &str); 54] = [
    (0xA1, "exclamdown"),
    (0xA2, "cent"),
    (0xA3, "sterling"),
    (0xA4, "fraction"),
    (0xA5, "yen"),
    (0xA6, "florin"),
    (0xA7, "section"),
    (0xA8, "currency"),
    (0xA9, "quotesingle"),
    (0xAA, "quotedblleft"),
    (0xAB, "guillemotleft"),
    (0xAC, "guilsinglleft"),
    (0xAD, "guilsinglright"),
    (0xAE, "fi"),
    (0xAF, "fl"),
    (0xB1, "endash"),
    (0xB2, "dagger"),
    (0xB3, "daggerdbl"),
    (0xB4, "periodcentered"),
    (0xB6, "paragraph"),
    (0xB7, "bullet"),
    (0xB8, "quotesinglbase"),
    (0xB9, "quotedblbase"),
    (0xBA, "quotedblright"),
    (0xBB, "guillemotright"),
    (0xBC, "ellipsis"),
    (0xBD, "perthousand"),
    (0xBF, "questiondown"),
    (0xC1, "grave"),
    (0xC2, "acute"),
    (0xC3, "circumflex"),
    (0xC4, "tilde"),
    (0xC5, "macron"),
    (0xC6, "breve"),
    (0xC7, "dotaccent"),
    (0xC8, "dieresis"),
    (0xCA, "ring"),
    (0xCB, "cedilla"),
    (0xCD, "hungarumlaut"),
    (0xCE, "ogonek"),
    (0xCF, "caron"),
    (0xD0, "emdash"),
    (0xE1, "AE"),
    (0xE3, "ordfeminine"),
    (0xE8, "Lslash"),
    (0xE9, "Oslash"),
    (0xEA, "OE"),
    (0xEB, "ordmasculine"),
    (0xF1, "ae"),
    (0xF5, "dotlessi"),
    (0xF8, "lslash"),
    (0xF9, "oslash"),
    (0xFA, "oe"),
    (0xFB, "germandbls"),
];

/// WinAnsiEncoding's 0x80..0x9F block; the rest coincides with Latin-1
/// (Annex D.2, and code page 1252).
const WIN_ANSI_80_9F: [char; 32] = [
    '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{FFFD}', '\u{017D}', '\u{FFFD}',
    '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
];

/// MacRomanEncoding above 0x7F (Annex D.2).
const MAC_ROMAN_HIGH: [char; 128] = [
    'Ä', 'Å', 'Ç', 'É', 'Ñ', 'Ö', 'Ü', 'á', 'à', 'â', 'ä', 'ã', 'å', 'ç', 'é', 'è', //
    'ê', 'ë', 'í', 'ì', 'î', 'ï', 'ñ', 'ó', 'ò', 'ô', 'ö', 'õ', 'ú', 'ù', 'û', 'ü', //
    '†', '°', '¢', '£', '§', '•', '¶', 'ß', '®', '©', '™', '´', '¨', '≠', 'Æ', 'Ø', //
    '∞', '±', '≤', '≥', '¥', 'µ', '∂', '∑', '∏', 'π', '∫', 'ª', 'º', 'Ω', 'æ', 'ø', //
    '¿', '¡', '¬', '√', 'ƒ', '≈', '∆', '«', '»', '…', '\u{00A0}', 'À', 'Ã', 'Õ', 'Œ', 'œ', //
    '–', '—', '“', '”', '‘', '’', '÷', '◊', 'ÿ', 'Ÿ', '⁄', '€', '‹', '›', 'ﬁ', 'ﬂ', //
    '‡', '·', '‚', '„', '‰', 'Â', 'Ê', 'Á', 'Ë', 'È', 'Í', 'Î', 'Ï', 'Ì', 'Ó', 'Ô', //
    '\u{F8FF}', 'Ò', 'Ú', 'Û', 'Ù', 'ı', 'ˆ', '˜', '¯', '˘', '˙', '˚', '¸', '˝', '˛', 'ˇ',
];

/// The character a code stands for in a base encoding.
///
/// `None` where the encoding leaves the code undefined, which callers report
/// rather than guess at.
#[must_use]
pub fn base_char(encoding: BaseEncoding, code: u8) -> Option<char> {
    match encoding {
        BaseEncoding::WinAnsi => match code {
            0x00..=0x7F => Some(char::from(code)),
            0x80..=0x9F => WIN_ANSI_80_9F
                .get(usize::from(code - 0x80))
                .copied()
                .filter(|c| *c != '\u{FFFD}'),
            // 0xA0 and 0xAD are non-breaking space and soft hyphen in 1252,
            // which PDF maps to space and hyphen.
            0xA0 => Some(' '),
            0xAD => Some('-'),
            _ => Some(char::from(code)),
        },
        BaseEncoding::MacRoman => match code {
            0x00..=0x7F => Some(char::from(code)),
            _ => MAC_ROMAN_HIGH.get(usize::from(code - 0x80)).copied(),
        },
        BaseEncoding::PdfDoc => Some(char::from(code)),
        BaseEncoding::Standard => match code {
            // Standard differs from ASCII at the quote and grave.
            0x27 => Some('\u{2019}'),
            0x60 => Some('\u{2018}'),
            0x00..=0x7F => Some(char::from(code)),
            _ => STANDARD_HIGH
                .iter()
                .find(|(c, _)| *c == code)
                .and_then(|(_, name)| glyph_name_to_char(name)),
        },
    }
}

/// The glyph name a code stands for in a base encoding, where one is defined.
#[must_use]
pub fn base_glyph_name(encoding: BaseEncoding, code: u8) -> Option<&'static str> {
    if let BaseEncoding::Standard = encoding {
        if let Some((_, name)) = STANDARD_HIGH.iter().find(|(c, _)| *c == code) {
            return Some(name);
        }
    }
    ASCII_NAMES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| *name)
}

/// Glyph names for the printable ASCII range, shared by every encoding.
const ASCII_NAMES: [(u8, &str); 95] = [
    (32, "space"),
    (33, "exclam"),
    (34, "quotedbl"),
    (35, "numbersign"),
    (36, "dollar"),
    (37, "percent"),
    (38, "ampersand"),
    (39, "quoteright"),
    (40, "parenleft"),
    (41, "parenright"),
    (42, "asterisk"),
    (43, "plus"),
    (44, "comma"),
    (45, "hyphen"),
    (46, "period"),
    (47, "slash"),
    (48, "zero"),
    (49, "one"),
    (50, "two"),
    (51, "three"),
    (52, "four"),
    (53, "five"),
    (54, "six"),
    (55, "seven"),
    (56, "eight"),
    (57, "nine"),
    (58, "colon"),
    (59, "semicolon"),
    (60, "less"),
    (61, "equal"),
    (62, "greater"),
    (63, "question"),
    (64, "at"),
    (65, "A"),
    (66, "B"),
    (67, "C"),
    (68, "D"),
    (69, "E"),
    (70, "F"),
    (71, "G"),
    (72, "H"),
    (73, "I"),
    (74, "J"),
    (75, "K"),
    (76, "L"),
    (77, "M"),
    (78, "N"),
    (79, "O"),
    (80, "P"),
    (81, "Q"),
    (82, "R"),
    (83, "S"),
    (84, "T"),
    (85, "U"),
    (86, "V"),
    (87, "W"),
    (88, "X"),
    (89, "Y"),
    (90, "Z"),
    (91, "bracketleft"),
    (92, "backslash"),
    (93, "bracketright"),
    (94, "asciicircum"),
    (95, "underscore"),
    (96, "quoteleft"),
    (97, "a"),
    (98, "b"),
    (99, "c"),
    (100, "d"),
    (101, "e"),
    (102, "f"),
    (103, "g"),
    (104, "h"),
    (105, "i"),
    (106, "j"),
    (107, "k"),
    (108, "l"),
    (109, "m"),
    (110, "n"),
    (111, "o"),
    (112, "p"),
    (113, "q"),
    (114, "r"),
    (115, "s"),
    (116, "t"),
    (117, "u"),
    (118, "v"),
    (119, "w"),
    (120, "x"),
    (121, "y"),
    (122, "z"),
    (123, "braceleft"),
    (124, "bar"),
    (125, "braceright"),
    (126, "asciitilde"),
];

/// Named glyphs outside ASCII that appear in `/Differences` constantly.
const NAMED: [(&str, char); 72] = [
    ("exclamdown", '¡'),
    ("cent", '¢'),
    ("sterling", '£'),
    ("fraction", '⁄'),
    ("yen", '¥'),
    ("florin", 'ƒ'),
    ("section", '§'),
    ("currency", '¤'),
    ("quotesingle", '\''),
    ("quotedblleft", '“'),
    ("guillemotleft", '«'),
    ("guilsinglleft", '‹'),
    ("guilsinglright", '›'),
    ("fi", 'ﬁ'),
    ("fl", 'ﬂ'),
    ("endash", '–'),
    ("dagger", '†'),
    ("daggerdbl", '‡'),
    ("periodcentered", '·'),
    ("paragraph", '¶'),
    ("bullet", '•'),
    ("quotesinglbase", '‚'),
    ("quotedblbase", '„'),
    ("quotedblright", '”'),
    ("guillemotright", '»'),
    ("ellipsis", '…'),
    ("perthousand", '‰'),
    ("questiondown", '¿'),
    ("grave", '`'),
    ("acute", '´'),
    ("circumflex", 'ˆ'),
    ("tilde", '˜'),
    ("macron", '¯'),
    ("breve", '˘'),
    ("dotaccent", '˙'),
    ("dieresis", '¨'),
    ("ring", '˚'),
    ("cedilla", '¸'),
    ("hungarumlaut", '˝'),
    ("ogonek", '˛'),
    ("caron", 'ˇ'),
    ("emdash", '—'),
    ("AE", 'Æ'),
    ("ordfeminine", 'ª'),
    ("Lslash", 'Ł'),
    ("Oslash", 'Ø'),
    ("OE", 'Œ'),
    ("ordmasculine", 'º'),
    ("ae", 'æ'),
    ("dotlessi", 'ı'),
    ("lslash", 'ł'),
    ("oslash", 'ø'),
    ("oe", 'œ'),
    ("germandbls", 'ß'),
    ("quoteleft", '‘'),
    ("quoteright", '’'),
    ("quotedbl", '"'),
    ("degree", '°'),
    ("plusminus", '±'),
    ("mu", 'µ'),
    ("multiply", '×'),
    ("divide", '÷'),
    ("copyright", '©'),
    ("registered", '®'),
    ("trademark", '™'),
    ("logicalnot", '¬'),
    ("minus", '−'),
    ("Euro", '€'),
    ("euro", '€'),
    ("nbspace", ' '),
    ("space", ' '),
    ("hyphen", '-'),
];

/// The glyph name a character is known by, the inverse of
/// [`glyph_name_to_char`].
///
/// Type 1 fonts address their glyphs by name and by nothing else — there is no
/// glyph id — so a code that has been turned into a character has to be turned
/// back into a name before the font can be asked for it.
///
/// Falls back to the AGL's algorithmic `uniXXXX` form, which is what a font
/// tool writes for anything outside the named set, so a glyph the tables do
/// not list is still reachable when the font happens to spell it that way.
#[must_use]
pub fn glyph_name_for_char(c: char) -> Option<String> {
    let code = u32::from(c);
    if let Ok(byte) = u8::try_from(code) {
        if let Some((_, name)) = ASCII_NAMES.iter().find(|(b, _)| *b == byte) {
            return Some((*name).to_string());
        }
    }
    if let Some((name, _)) = NAMED.iter().find(|(_, ch)| *ch == c) {
        return Some((*name).to_string());
    }
    (code <= 0xFFFF).then(|| format!("uni{code:04X}"))
}

/// The character a glyph name stands for.
///
/// Covers the ASCII names, the common named glyphs, and the algorithmic
/// `uniXXXX` and `uXXXX[XX]` forms of the Adobe Glyph List specification. A
/// name this does not know returns `None`, which the caller reports rather
/// than substituting something plausible.
#[must_use]
pub fn glyph_name_to_char(name: &str) -> Option<char> {
    if let Some((code, _)) = ASCII_NAMES.iter().find(|(_, n)| *n == name) {
        return Some(char::from(*code));
    }
    if let Some((_, c)) = NAMED.iter().find(|(n, _)| *n == name) {
        return Some(*c);
    }

    // AGL algorithmic names: uniXXXX, or uXXXX through uXXXXXX.
    if let Some(hex) = name.strip_prefix("uni") {
        if hex.len() >= 4 {
            if let Some(c) = hex.get(..4).and_then(from_hex) {
                return Some(c);
            }
        }
    }
    if let Some(hex) = name.strip_prefix('u') {
        if (4..=6).contains(&hex.len()) {
            if let Some(c) = from_hex(hex) {
                return Some(c);
            }
        }
    }

    // A name of the form `gXX` or `cidXX` identifies a glyph by index, which
    // carries no character meaning at all.
    if name.starts_with('g') || name.starts_with("cid") {
        return None;
    }

    // Suffixed names such as `a.sc` or `one.oldstyle` denote a variant of the
    // base glyph and extract as the base character.
    name.split_once('.')
        .and_then(|(base, _)| (!base.is_empty()).then(|| glyph_name_to_char(base))?)
}

fn from_hex(hex: &str) -> Option<char> {
    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win_ansi_is_code_page_1252() {
        assert_eq!(base_char(BaseEncoding::WinAnsi, b'A'), Some('A'));
        assert_eq!(base_char(BaseEncoding::WinAnsi, 0x80), Some('€'));
        assert_eq!(base_char(BaseEncoding::WinAnsi, 0x92), Some('’'));
        assert_eq!(base_char(BaseEncoding::WinAnsi, 0xE9), Some('é'));
        // Undefined 1252 positions have no character.
        assert_eq!(base_char(BaseEncoding::WinAnsi, 0x81), None);
    }

    #[test]
    fn standard_encoding_differs_at_the_quotes() {
        assert_eq!(base_char(BaseEncoding::Standard, 0x27), Some('\u{2019}'));
        assert_eq!(base_char(BaseEncoding::Standard, 0x60), Some('\u{2018}'));
        assert_eq!(base_char(BaseEncoding::WinAnsi, 0x27), Some('\''));
    }

    /// Annex D.2's StandardEncoding runs to 0xFB, and the last eight names
    /// were absent. They are the ones a Type 1 or CFF font reaches by name,
    /// so a code with no name here draws `.notdef` however good the font is.
    #[test]
    fn standard_encoding_names_its_whole_high_range() {
        assert_eq!(
            base_glyph_name(BaseEncoding::Standard, 0xE9),
            Some("Oslash")
        );
        assert_eq!(
            base_glyph_name(BaseEncoding::Standard, 0xFB),
            Some("germandbls")
        );
        assert_eq!(base_char(BaseEncoding::Standard, 0xF5), Some('ı'));
        assert_eq!(base_char(BaseEncoding::Standard, 0xFB), Some('ß'));
        // A code the encoding genuinely leaves undefined still has no name.
        assert_eq!(base_glyph_name(BaseEncoding::Standard, 0xFF), None);
        assert_eq!(base_glyph_name(BaseEncoding::Standard, b'A'), Some("A"));
    }

    #[test]
    fn mac_roman_high_range() {
        assert_eq!(base_char(BaseEncoding::MacRoman, 0x80), Some('Ä'));
        assert_eq!(base_char(BaseEncoding::MacRoman, 0xA5), Some('•'));
    }

    #[test]
    fn glyph_names_resolve_by_table_and_by_algorithm() {
        assert_eq!(glyph_name_to_char("A"), Some('A'));
        assert_eq!(glyph_name_to_char("space"), Some(' '));
        assert_eq!(glyph_name_to_char("germandbls"), Some('ß'));
        assert_eq!(glyph_name_to_char("uni20AC"), Some('€'));
        assert_eq!(glyph_name_to_char("u1F600"), Some('\u{1F600}'));
        assert_eq!(glyph_name_to_char("a.sc"), Some('a'), "variant suffix");
        assert_eq!(glyph_name_to_char("g42"), None, "a glyph index is not text");
        assert_eq!(glyph_name_to_char("cid7"), None);
        assert_eq!(glyph_name_to_char("nosuchglyph"), None);
    }

    #[test]
    fn ascii_names_are_complete_and_ordered() {
        assert_eq!(ASCII_NAMES.len(), 95, "every printable ASCII code");
        for (i, (code, _)) in ASCII_NAMES.iter().enumerate() {
            assert_eq!(usize::from(*code), i + 32);
        }
    }
}
