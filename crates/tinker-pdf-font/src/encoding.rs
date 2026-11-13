//! Simple-font encodings (Annex D) and glyph names.
//!
//! A simple font maps a byte to a glyph name, and the glyph name to a
//! character. Both halves are needed: the name reaches the font program's
//! outline, and the character is what text extraction reports.
//!
//! Two encodings belong to one font each: the built-in encodings of the
//! symbolic standard fonts, Symbol and ZapfDingbats (D.5, D.6), which
//! [`BuiltInEncoding`] holds with the Unicode value the published glyph lists
//! give each name (`docs/design/symbol-faces.md`).

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

/// A built-in encoding Annex D gives one font alone: the two symbolic
/// standard fonts' (9.6.2.2, D.5 and D.6), whose codes name glyphs no text
/// encoding knows.
///
/// A font that names its base encoding, or renames a code with
/// `/Differences`, has those names first (9.6.6.1); this is the name a code
/// has when the document gives none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltInEncoding {
    /// Table D.5, the Symbol font's: 189 codes.
    Symbol,
    /// Table D.6, the ZapfDingbats font's: 188 codes.
    ZapfDingbats,
}

/// One code of a [`BuiltInEncoding`]: the code, the glyph name its table gives
/// it, and the Unicode value the published glyph list gives the name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuiltInCode {
    /// The code, as Annex D's table gives it in octal.
    pub code: u8,
    /// The glyph name the table gives the code.
    pub name: &'static str,
    /// The name's value in the Adobe Glyph List, or for ZapfDingbats' `a`
    /// names in the ITC Zapf Dingbats Glyph List.
    pub unicode: char,
}

impl BuiltInEncoding {
    /// Every code the table lists, ascending.
    #[must_use]
    pub fn codes(self) -> &'static [BuiltInCode] {
        match self {
            BuiltInEncoding::Symbol => &SYMBOL,
            BuiltInEncoding::ZapfDingbats => &DINGBATS,
        }
    }

    /// The glyph name the table gives `code`, or `None` where it lists no such
    /// code.
    #[must_use]
    pub fn glyph_name(self, code: u8) -> Option<&'static str> {
        let codes = self.codes();
        let at = codes.binary_search_by_key(&code, |entry| entry.code).ok()?;
        codes.get(at).map(|entry| entry.name)
    }

    /// The Unicode value the font's glyph list gives `name`.
    ///
    /// Every name the table encodes has one, and so do the fourteen names the
    /// ITC Zapf Dingbats Glyph List holds and Table D.6 gives no code, which a
    /// `/Differences` array can still name. A name the font's list does not
    /// hold has `None` here; `glyph_name_to_char` reads the general forms.
    #[must_use]
    pub fn unicode_of(self, name: &str) -> Option<char> {
        if let Some(entry) = self.codes().iter().find(|entry| entry.name == name) {
            return Some(entry.unicode);
        }
        match self {
            BuiltInEncoding::Symbol => None,
            BuiltInEncoding::ZapfDingbats => DINGBATS_UNENCODED
                .iter()
                .find(|(listed, _)| *listed == name)
                .map(|(_, value)| *value),
        }
    }
}

/// A row of a built-in encoding's table, for the tables below.
const fn c(code: u8, name: &'static str, unicode: char) -> BuiltInCode {
    BuiltInCode {
        code,
        name,
        unicode,
    }
}

/// Table D.5, Symbol set and encoding, ascending by code: 32 to 126 and 160 to
/// 254, but 240. The table orders its rows by name; each name's value is the
/// Adobe Glyph List's (`glyphlist.txt`, table version 2.0).
const SYMBOL: [BuiltInCode; 189] = [
    c(0o040, "space", '\u{0020}'),
    c(0o041, "exclam", '\u{0021}'),
    c(0o042, "universal", '\u{2200}'),
    c(0o043, "numbersign", '\u{0023}'),
    c(0o044, "existential", '\u{2203}'),
    c(0o045, "percent", '\u{0025}'),
    c(0o046, "ampersand", '\u{0026}'),
    c(0o047, "suchthat", '\u{220B}'),
    c(0o050, "parenleft", '\u{0028}'),
    c(0o051, "parenright", '\u{0029}'),
    c(0o052, "asteriskmath", '\u{2217}'),
    c(0o053, "plus", '\u{002B}'),
    c(0o054, "comma", '\u{002C}'),
    c(0o055, "minus", '\u{2212}'),
    c(0o056, "period", '\u{002E}'),
    c(0o057, "slash", '\u{002F}'),
    c(0o060, "zero", '\u{0030}'),
    c(0o061, "one", '\u{0031}'),
    c(0o062, "two", '\u{0032}'),
    c(0o063, "three", '\u{0033}'),
    c(0o064, "four", '\u{0034}'),
    c(0o065, "five", '\u{0035}'),
    c(0o066, "six", '\u{0036}'),
    c(0o067, "seven", '\u{0037}'),
    c(0o070, "eight", '\u{0038}'),
    c(0o071, "nine", '\u{0039}'),
    c(0o072, "colon", '\u{003A}'),
    c(0o073, "semicolon", '\u{003B}'),
    c(0o074, "less", '\u{003C}'),
    c(0o075, "equal", '\u{003D}'),
    c(0o076, "greater", '\u{003E}'),
    c(0o077, "question", '\u{003F}'),
    c(0o100, "congruent", '\u{2245}'),
    c(0o101, "Alpha", '\u{0391}'),
    c(0o102, "Beta", '\u{0392}'),
    c(0o103, "Chi", '\u{03A7}'),
    c(0o104, "Delta", '\u{2206}'),
    c(0o105, "Epsilon", '\u{0395}'),
    c(0o106, "Phi", '\u{03A6}'),
    c(0o107, "Gamma", '\u{0393}'),
    c(0o110, "Eta", '\u{0397}'),
    c(0o111, "Iota", '\u{0399}'),
    c(0o112, "theta1", '\u{03D1}'),
    c(0o113, "Kappa", '\u{039A}'),
    c(0o114, "Lambda", '\u{039B}'),
    c(0o115, "Mu", '\u{039C}'),
    c(0o116, "Nu", '\u{039D}'),
    c(0o117, "Omicron", '\u{039F}'),
    c(0o120, "Pi", '\u{03A0}'),
    c(0o121, "Theta", '\u{0398}'),
    c(0o122, "Rho", '\u{03A1}'),
    c(0o123, "Sigma", '\u{03A3}'),
    c(0o124, "Tau", '\u{03A4}'),
    c(0o125, "Upsilon", '\u{03A5}'),
    c(0o126, "sigma1", '\u{03C2}'),
    c(0o127, "Omega", '\u{2126}'),
    c(0o130, "Xi", '\u{039E}'),
    c(0o131, "Psi", '\u{03A8}'),
    c(0o132, "Zeta", '\u{0396}'),
    c(0o133, "bracketleft", '\u{005B}'),
    c(0o134, "therefore", '\u{2234}'),
    c(0o135, "bracketright", '\u{005D}'),
    c(0o136, "perpendicular", '\u{22A5}'),
    c(0o137, "underscore", '\u{005F}'),
    c(0o140, "radicalex", '\u{F8E5}'),
    c(0o141, "alpha", '\u{03B1}'),
    c(0o142, "beta", '\u{03B2}'),
    c(0o143, "chi", '\u{03C7}'),
    c(0o144, "delta", '\u{03B4}'),
    c(0o145, "epsilon", '\u{03B5}'),
    c(0o146, "phi", '\u{03C6}'),
    c(0o147, "gamma", '\u{03B3}'),
    c(0o150, "eta", '\u{03B7}'),
    c(0o151, "iota", '\u{03B9}'),
    c(0o152, "phi1", '\u{03D5}'),
    c(0o153, "kappa", '\u{03BA}'),
    c(0o154, "lambda", '\u{03BB}'),
    c(0o155, "mu", '\u{00B5}'),
    c(0o156, "nu", '\u{03BD}'),
    c(0o157, "omicron", '\u{03BF}'),
    c(0o160, "pi", '\u{03C0}'),
    c(0o161, "theta", '\u{03B8}'),
    c(0o162, "rho", '\u{03C1}'),
    c(0o163, "sigma", '\u{03C3}'),
    c(0o164, "tau", '\u{03C4}'),
    c(0o165, "upsilon", '\u{03C5}'),
    c(0o166, "omega1", '\u{03D6}'),
    c(0o167, "omega", '\u{03C9}'),
    c(0o170, "xi", '\u{03BE}'),
    c(0o171, "psi", '\u{03C8}'),
    c(0o172, "zeta", '\u{03B6}'),
    c(0o173, "braceleft", '\u{007B}'),
    c(0o174, "bar", '\u{007C}'),
    c(0o175, "braceright", '\u{007D}'),
    c(0o176, "similar", '\u{223C}'),
    c(0o240, "Euro", '\u{20AC}'),
    c(0o241, "Upsilon1", '\u{03D2}'),
    c(0o242, "minute", '\u{2032}'),
    c(0o243, "lessequal", '\u{2264}'),
    c(0o244, "fraction", '\u{2044}'),
    c(0o245, "infinity", '\u{221E}'),
    c(0o246, "florin", '\u{0192}'),
    c(0o247, "club", '\u{2663}'),
    c(0o250, "diamond", '\u{2666}'),
    c(0o251, "heart", '\u{2665}'),
    c(0o252, "spade", '\u{2660}'),
    c(0o253, "arrowboth", '\u{2194}'),
    c(0o254, "arrowleft", '\u{2190}'),
    c(0o255, "arrowup", '\u{2191}'),
    c(0o256, "arrowright", '\u{2192}'),
    c(0o257, "arrowdown", '\u{2193}'),
    c(0o260, "degree", '\u{00B0}'),
    c(0o261, "plusminus", '\u{00B1}'),
    c(0o262, "second", '\u{2033}'),
    c(0o263, "greaterequal", '\u{2265}'),
    c(0o264, "multiply", '\u{00D7}'),
    c(0o265, "proportional", '\u{221D}'),
    c(0o266, "partialdiff", '\u{2202}'),
    c(0o267, "bullet", '\u{2022}'),
    c(0o270, "divide", '\u{00F7}'),
    c(0o271, "notequal", '\u{2260}'),
    c(0o272, "equivalence", '\u{2261}'),
    c(0o273, "approxequal", '\u{2248}'),
    c(0o274, "ellipsis", '\u{2026}'),
    c(0o275, "arrowvertex", '\u{F8E6}'),
    c(0o276, "arrowhorizex", '\u{F8E7}'),
    c(0o277, "carriagereturn", '\u{21B5}'),
    c(0o300, "aleph", '\u{2135}'),
    c(0o301, "Ifraktur", '\u{2111}'),
    c(0o302, "Rfraktur", '\u{211C}'),
    c(0o303, "weierstrass", '\u{2118}'),
    c(0o304, "circlemultiply", '\u{2297}'),
    c(0o305, "circleplus", '\u{2295}'),
    c(0o306, "emptyset", '\u{2205}'),
    c(0o307, "intersection", '\u{2229}'),
    c(0o310, "union", '\u{222A}'),
    c(0o311, "propersuperset", '\u{2283}'),
    c(0o312, "reflexsuperset", '\u{2287}'),
    c(0o313, "notsubset", '\u{2284}'),
    c(0o314, "propersubset", '\u{2282}'),
    c(0o315, "reflexsubset", '\u{2286}'),
    c(0o316, "element", '\u{2208}'),
    c(0o317, "notelement", '\u{2209}'),
    c(0o320, "angle", '\u{2220}'),
    c(0o321, "gradient", '\u{2207}'),
    c(0o322, "registerserif", '\u{F6DA}'),
    c(0o323, "copyrightserif", '\u{F6D9}'),
    c(0o324, "trademarkserif", '\u{F6DB}'),
    c(0o325, "product", '\u{220F}'),
    c(0o326, "radical", '\u{221A}'),
    c(0o327, "dotmath", '\u{22C5}'),
    c(0o330, "logicalnot", '\u{00AC}'),
    c(0o331, "logicaland", '\u{2227}'),
    c(0o332, "logicalor", '\u{2228}'),
    c(0o333, "arrowdblboth", '\u{21D4}'),
    c(0o334, "arrowdblleft", '\u{21D0}'),
    c(0o335, "arrowdblup", '\u{21D1}'),
    c(0o336, "arrowdblright", '\u{21D2}'),
    c(0o337, "arrowdbldown", '\u{21D3}'),
    c(0o340, "lozenge", '\u{25CA}'),
    c(0o341, "angleleft", '\u{2329}'),
    c(0o342, "registersans", '\u{F8E8}'),
    c(0o343, "copyrightsans", '\u{F8E9}'),
    c(0o344, "trademarksans", '\u{F8EA}'),
    c(0o345, "summation", '\u{2211}'),
    c(0o346, "parenlefttp", '\u{F8EB}'),
    c(0o347, "parenleftex", '\u{F8EC}'),
    c(0o350, "parenleftbt", '\u{F8ED}'),
    c(0o351, "bracketlefttp", '\u{F8EE}'),
    c(0o352, "bracketleftex", '\u{F8EF}'),
    c(0o353, "bracketleftbt", '\u{F8F0}'),
    c(0o354, "bracelefttp", '\u{F8F1}'),
    c(0o355, "braceleftmid", '\u{F8F2}'),
    c(0o356, "braceleftbt", '\u{F8F3}'),
    c(0o357, "braceex", '\u{F8F4}'),
    c(0o361, "angleright", '\u{232A}'),
    c(0o362, "integral", '\u{222B}'),
    c(0o363, "integraltp", '\u{2320}'),
    c(0o364, "integralex", '\u{F8F5}'),
    c(0o365, "integralbt", '\u{2321}'),
    c(0o366, "parenrighttp", '\u{F8F6}'),
    c(0o367, "parenrightex", '\u{F8F7}'),
    c(0o370, "parenrightbt", '\u{F8F8}'),
    c(0o371, "bracketrighttp", '\u{F8F9}'),
    c(0o372, "bracketrightex", '\u{F8FA}'),
    c(0o373, "bracketrightbt", '\u{F8FB}'),
    c(0o374, "bracerighttp", '\u{F8FC}'),
    c(0o375, "bracerightmid", '\u{F8FD}'),
    c(0o376, "bracerightbt", '\u{F8FE}'),
];

/// Table D.6, ZapfDingbats set and encoding, ascending by code as the table is:
/// 32 to 126, 161 to 239 and 241 to 254. Each `a` name's value is the ITC Zapf
/// Dingbats Glyph List's (`zapfdingbats.txt`, table version 2.0), `space`'s
/// the Adobe Glyph List's.
const DINGBATS: [BuiltInCode; 188] = [
    c(0o040, "space", '\u{0020}'),
    c(0o041, "a1", '\u{2701}'),
    c(0o042, "a2", '\u{2702}'),
    c(0o043, "a202", '\u{2703}'),
    c(0o044, "a3", '\u{2704}'),
    c(0o045, "a4", '\u{260E}'),
    c(0o046, "a5", '\u{2706}'),
    c(0o047, "a119", '\u{2707}'),
    c(0o050, "a118", '\u{2708}'),
    c(0o051, "a117", '\u{2709}'),
    c(0o052, "a11", '\u{261B}'),
    c(0o053, "a12", '\u{261E}'),
    c(0o054, "a13", '\u{270C}'),
    c(0o055, "a14", '\u{270D}'),
    c(0o056, "a15", '\u{270E}'),
    c(0o057, "a16", '\u{270F}'),
    c(0o060, "a105", '\u{2710}'),
    c(0o061, "a17", '\u{2711}'),
    c(0o062, "a18", '\u{2712}'),
    c(0o063, "a19", '\u{2713}'),
    c(0o064, "a20", '\u{2714}'),
    c(0o065, "a21", '\u{2715}'),
    c(0o066, "a22", '\u{2716}'),
    c(0o067, "a23", '\u{2717}'),
    c(0o070, "a24", '\u{2718}'),
    c(0o071, "a25", '\u{2719}'),
    c(0o072, "a26", '\u{271A}'),
    c(0o073, "a27", '\u{271B}'),
    c(0o074, "a28", '\u{271C}'),
    c(0o075, "a6", '\u{271D}'),
    c(0o076, "a7", '\u{271E}'),
    c(0o077, "a8", '\u{271F}'),
    c(0o100, "a9", '\u{2720}'),
    c(0o101, "a10", '\u{2721}'),
    c(0o102, "a29", '\u{2722}'),
    c(0o103, "a30", '\u{2723}'),
    c(0o104, "a31", '\u{2724}'),
    c(0o105, "a32", '\u{2725}'),
    c(0o106, "a33", '\u{2726}'),
    c(0o107, "a34", '\u{2727}'),
    c(0o110, "a35", '\u{2605}'),
    c(0o111, "a36", '\u{2729}'),
    c(0o112, "a37", '\u{272A}'),
    c(0o113, "a38", '\u{272B}'),
    c(0o114, "a39", '\u{272C}'),
    c(0o115, "a40", '\u{272D}'),
    c(0o116, "a41", '\u{272E}'),
    c(0o117, "a42", '\u{272F}'),
    c(0o120, "a43", '\u{2730}'),
    c(0o121, "a44", '\u{2731}'),
    c(0o122, "a45", '\u{2732}'),
    c(0o123, "a46", '\u{2733}'),
    c(0o124, "a47", '\u{2734}'),
    c(0o125, "a48", '\u{2735}'),
    c(0o126, "a49", '\u{2736}'),
    c(0o127, "a50", '\u{2737}'),
    c(0o130, "a51", '\u{2738}'),
    c(0o131, "a52", '\u{2739}'),
    c(0o132, "a53", '\u{273A}'),
    c(0o133, "a54", '\u{273B}'),
    c(0o134, "a55", '\u{273C}'),
    c(0o135, "a56", '\u{273D}'),
    c(0o136, "a57", '\u{273E}'),
    c(0o137, "a58", '\u{273F}'),
    c(0o140, "a59", '\u{2740}'),
    c(0o141, "a60", '\u{2741}'),
    c(0o142, "a61", '\u{2742}'),
    c(0o143, "a62", '\u{2743}'),
    c(0o144, "a63", '\u{2744}'),
    c(0o145, "a64", '\u{2745}'),
    c(0o146, "a65", '\u{2746}'),
    c(0o147, "a66", '\u{2747}'),
    c(0o150, "a67", '\u{2748}'),
    c(0o151, "a68", '\u{2749}'),
    c(0o152, "a69", '\u{274A}'),
    c(0o153, "a70", '\u{274B}'),
    c(0o154, "a71", '\u{25CF}'),
    c(0o155, "a72", '\u{274D}'),
    c(0o156, "a73", '\u{25A0}'),
    c(0o157, "a74", '\u{274F}'),
    c(0o160, "a203", '\u{2750}'),
    c(0o161, "a75", '\u{2751}'),
    c(0o162, "a204", '\u{2752}'),
    c(0o163, "a76", '\u{25B2}'),
    c(0o164, "a77", '\u{25BC}'),
    c(0o165, "a78", '\u{25C6}'),
    c(0o166, "a79", '\u{2756}'),
    c(0o167, "a81", '\u{25D7}'),
    c(0o170, "a82", '\u{2758}'),
    c(0o171, "a83", '\u{2759}'),
    c(0o172, "a84", '\u{275A}'),
    c(0o173, "a97", '\u{275B}'),
    c(0o174, "a98", '\u{275C}'),
    c(0o175, "a99", '\u{275D}'),
    c(0o176, "a100", '\u{275E}'),
    c(0o241, "a101", '\u{2761}'),
    c(0o242, "a102", '\u{2762}'),
    c(0o243, "a103", '\u{2763}'),
    c(0o244, "a104", '\u{2764}'),
    c(0o245, "a106", '\u{2765}'),
    c(0o246, "a107", '\u{2766}'),
    c(0o247, "a108", '\u{2767}'),
    c(0o250, "a112", '\u{2663}'),
    c(0o251, "a111", '\u{2666}'),
    c(0o252, "a110", '\u{2665}'),
    c(0o253, "a109", '\u{2660}'),
    c(0o254, "a120", '\u{2460}'),
    c(0o255, "a121", '\u{2461}'),
    c(0o256, "a122", '\u{2462}'),
    c(0o257, "a123", '\u{2463}'),
    c(0o260, "a124", '\u{2464}'),
    c(0o261, "a125", '\u{2465}'),
    c(0o262, "a126", '\u{2466}'),
    c(0o263, "a127", '\u{2467}'),
    c(0o264, "a128", '\u{2468}'),
    c(0o265, "a129", '\u{2469}'),
    c(0o266, "a130", '\u{2776}'),
    c(0o267, "a131", '\u{2777}'),
    c(0o270, "a132", '\u{2778}'),
    c(0o271, "a133", '\u{2779}'),
    c(0o272, "a134", '\u{277A}'),
    c(0o273, "a135", '\u{277B}'),
    c(0o274, "a136", '\u{277C}'),
    c(0o275, "a137", '\u{277D}'),
    c(0o276, "a138", '\u{277E}'),
    c(0o277, "a139", '\u{277F}'),
    c(0o300, "a140", '\u{2780}'),
    c(0o301, "a141", '\u{2781}'),
    c(0o302, "a142", '\u{2782}'),
    c(0o303, "a143", '\u{2783}'),
    c(0o304, "a144", '\u{2784}'),
    c(0o305, "a145", '\u{2785}'),
    c(0o306, "a146", '\u{2786}'),
    c(0o307, "a147", '\u{2787}'),
    c(0o310, "a148", '\u{2788}'),
    c(0o311, "a149", '\u{2789}'),
    c(0o312, "a150", '\u{278A}'),
    c(0o313, "a151", '\u{278B}'),
    c(0o314, "a152", '\u{278C}'),
    c(0o315, "a153", '\u{278D}'),
    c(0o316, "a154", '\u{278E}'),
    c(0o317, "a155", '\u{278F}'),
    c(0o320, "a156", '\u{2790}'),
    c(0o321, "a157", '\u{2791}'),
    c(0o322, "a158", '\u{2792}'),
    c(0o323, "a159", '\u{2793}'),
    c(0o324, "a160", '\u{2794}'),
    c(0o325, "a161", '\u{2192}'),
    c(0o326, "a163", '\u{2194}'),
    c(0o327, "a164", '\u{2195}'),
    c(0o330, "a196", '\u{2798}'),
    c(0o331, "a165", '\u{2799}'),
    c(0o332, "a192", '\u{279A}'),
    c(0o333, "a166", '\u{279B}'),
    c(0o334, "a167", '\u{279C}'),
    c(0o335, "a168", '\u{279D}'),
    c(0o336, "a169", '\u{279E}'),
    c(0o337, "a170", '\u{279F}'),
    c(0o340, "a171", '\u{27A0}'),
    c(0o341, "a172", '\u{27A1}'),
    c(0o342, "a173", '\u{27A2}'),
    c(0o343, "a162", '\u{27A3}'),
    c(0o344, "a174", '\u{27A4}'),
    c(0o345, "a175", '\u{27A5}'),
    c(0o346, "a176", '\u{27A6}'),
    c(0o347, "a177", '\u{27A7}'),
    c(0o350, "a178", '\u{27A8}'),
    c(0o351, "a179", '\u{27A9}'),
    c(0o352, "a193", '\u{27AA}'),
    c(0o353, "a180", '\u{27AB}'),
    c(0o354, "a199", '\u{27AC}'),
    c(0o355, "a181", '\u{27AD}'),
    c(0o356, "a200", '\u{27AE}'),
    c(0o357, "a182", '\u{27AF}'),
    c(0o361, "a201", '\u{27B1}'),
    c(0o362, "a183", '\u{27B2}'),
    c(0o363, "a184", '\u{27B3}'),
    c(0o364, "a197", '\u{27B4}'),
    c(0o365, "a185", '\u{27B5}'),
    c(0o366, "a194", '\u{27B6}'),
    c(0o367, "a198", '\u{27B7}'),
    c(0o370, "a186", '\u{27B8}'),
    c(0o371, "a195", '\u{27B9}'),
    c(0o372, "a187", '\u{27BA}'),
    c(0o373, "a188", '\u{27BB}'),
    c(0o374, "a189", '\u{27BC}'),
    c(0o375, "a190", '\u{27BD}'),
    c(0o376, "a191", '\u{27BE}'),
];

/// The ITC Zapf Dingbats Glyph List's names Table D.6 gives no code, with their
/// values: reached only by a name a `/Differences` array gives a code.
const DINGBATS_UNENCODED: [(&str, char); 14] = [
    ("a85", '\u{276F}'),
    ("a86", '\u{2771}'),
    ("a87", '\u{2772}'),
    ("a88", '\u{2773}'),
    ("a89", '\u{2768}'),
    ("a90", '\u{2769}'),
    ("a91", '\u{276C}'),
    ("a92", '\u{276D}'),
    ("a93", '\u{276A}'),
    ("a94", '\u{276B}'),
    ("a95", '\u{2774}'),
    ("a96", '\u{2775}'),
    ("a205", '\u{276E}'),
    ("a206", '\u{2770}'),
];

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

    /// Table D.5 again, keyed by name, as the table itself orders its rows,
    /// and each value read again from the Adobe Glyph List: a second
    /// transcription, made apart from `SYMBOL`'s, which is keyed by code.
    const SYMBOL_BY_NAME: [(&str, u8, char); 189] = [
        ("Alpha", 0o101, '\u{0391}'),
        ("Beta", 0o102, '\u{0392}'),
        ("Chi", 0o103, '\u{03A7}'),
        ("Delta", 0o104, '\u{2206}'),
        ("Epsilon", 0o105, '\u{0395}'),
        ("Eta", 0o110, '\u{0397}'),
        ("Euro", 0o240, '\u{20AC}'),
        ("Gamma", 0o107, '\u{0393}'),
        ("Ifraktur", 0o301, '\u{2111}'),
        ("Iota", 0o111, '\u{0399}'),
        ("Kappa", 0o113, '\u{039A}'),
        ("Lambda", 0o114, '\u{039B}'),
        ("Mu", 0o115, '\u{039C}'),
        ("Nu", 0o116, '\u{039D}'),
        ("Omega", 0o127, '\u{2126}'),
        ("Omicron", 0o117, '\u{039F}'),
        ("Phi", 0o106, '\u{03A6}'),
        ("Pi", 0o120, '\u{03A0}'),
        ("Psi", 0o131, '\u{03A8}'),
        ("Rfraktur", 0o302, '\u{211C}'),
        ("Rho", 0o122, '\u{03A1}'),
        ("Sigma", 0o123, '\u{03A3}'),
        ("Tau", 0o124, '\u{03A4}'),
        ("Theta", 0o121, '\u{0398}'),
        ("Upsilon", 0o125, '\u{03A5}'),
        ("Upsilon1", 0o241, '\u{03D2}'),
        ("Xi", 0o130, '\u{039E}'),
        ("Zeta", 0o132, '\u{0396}'),
        ("aleph", 0o300, '\u{2135}'),
        ("alpha", 0o141, '\u{03B1}'),
        ("ampersand", 0o046, '\u{0026}'),
        ("angle", 0o320, '\u{2220}'),
        ("angleleft", 0o341, '\u{2329}'),
        ("angleright", 0o361, '\u{232A}'),
        ("approxequal", 0o273, '\u{2248}'),
        ("arrowboth", 0o253, '\u{2194}'),
        ("arrowdblboth", 0o333, '\u{21D4}'),
        ("arrowdbldown", 0o337, '\u{21D3}'),
        ("arrowdblleft", 0o334, '\u{21D0}'),
        ("arrowdblright", 0o336, '\u{21D2}'),
        ("arrowdblup", 0o335, '\u{21D1}'),
        ("arrowdown", 0o257, '\u{2193}'),
        ("arrowhorizex", 0o276, '\u{F8E7}'),
        ("arrowleft", 0o254, '\u{2190}'),
        ("arrowright", 0o256, '\u{2192}'),
        ("arrowup", 0o255, '\u{2191}'),
        ("arrowvertex", 0o275, '\u{F8E6}'),
        ("asteriskmath", 0o052, '\u{2217}'),
        ("bar", 0o174, '\u{007C}'),
        ("beta", 0o142, '\u{03B2}'),
        ("braceex", 0o357, '\u{F8F4}'),
        ("braceleft", 0o173, '\u{007B}'),
        ("braceleftbt", 0o356, '\u{F8F3}'),
        ("braceleftmid", 0o355, '\u{F8F2}'),
        ("bracelefttp", 0o354, '\u{F8F1}'),
        ("braceright", 0o175, '\u{007D}'),
        ("bracerightbt", 0o376, '\u{F8FE}'),
        ("bracerightmid", 0o375, '\u{F8FD}'),
        ("bracerighttp", 0o374, '\u{F8FC}'),
        ("bracketleft", 0o133, '\u{005B}'),
        ("bracketleftbt", 0o353, '\u{F8F0}'),
        ("bracketleftex", 0o352, '\u{F8EF}'),
        ("bracketlefttp", 0o351, '\u{F8EE}'),
        ("bracketright", 0o135, '\u{005D}'),
        ("bracketrightbt", 0o373, '\u{F8FB}'),
        ("bracketrightex", 0o372, '\u{F8FA}'),
        ("bracketrighttp", 0o371, '\u{F8F9}'),
        ("bullet", 0o267, '\u{2022}'),
        ("carriagereturn", 0o277, '\u{21B5}'),
        ("chi", 0o143, '\u{03C7}'),
        ("circlemultiply", 0o304, '\u{2297}'),
        ("circleplus", 0o305, '\u{2295}'),
        ("club", 0o247, '\u{2663}'),
        ("colon", 0o072, '\u{003A}'),
        ("comma", 0o054, '\u{002C}'),
        ("congruent", 0o100, '\u{2245}'),
        ("copyrightsans", 0o343, '\u{F8E9}'),
        ("copyrightserif", 0o323, '\u{F6D9}'),
        ("degree", 0o260, '\u{00B0}'),
        ("delta", 0o144, '\u{03B4}'),
        ("diamond", 0o250, '\u{2666}'),
        ("divide", 0o270, '\u{00F7}'),
        ("dotmath", 0o327, '\u{22C5}'),
        ("eight", 0o070, '\u{0038}'),
        ("element", 0o316, '\u{2208}'),
        ("ellipsis", 0o274, '\u{2026}'),
        ("emptyset", 0o306, '\u{2205}'),
        ("epsilon", 0o145, '\u{03B5}'),
        ("equal", 0o075, '\u{003D}'),
        ("equivalence", 0o272, '\u{2261}'),
        ("eta", 0o150, '\u{03B7}'),
        ("exclam", 0o041, '\u{0021}'),
        ("existential", 0o044, '\u{2203}'),
        ("five", 0o065, '\u{0035}'),
        ("florin", 0o246, '\u{0192}'),
        ("four", 0o064, '\u{0034}'),
        ("fraction", 0o244, '\u{2044}'),
        ("gamma", 0o147, '\u{03B3}'),
        ("gradient", 0o321, '\u{2207}'),
        ("greater", 0o076, '\u{003E}'),
        ("greaterequal", 0o263, '\u{2265}'),
        ("heart", 0o251, '\u{2665}'),
        ("infinity", 0o245, '\u{221E}'),
        ("integral", 0o362, '\u{222B}'),
        ("integralbt", 0o365, '\u{2321}'),
        ("integralex", 0o364, '\u{F8F5}'),
        ("integraltp", 0o363, '\u{2320}'),
        ("intersection", 0o307, '\u{2229}'),
        ("iota", 0o151, '\u{03B9}'),
        ("kappa", 0o153, '\u{03BA}'),
        ("lambda", 0o154, '\u{03BB}'),
        ("less", 0o074, '\u{003C}'),
        ("lessequal", 0o243, '\u{2264}'),
        ("logicaland", 0o331, '\u{2227}'),
        ("logicalnot", 0o330, '\u{00AC}'),
        ("logicalor", 0o332, '\u{2228}'),
        ("lozenge", 0o340, '\u{25CA}'),
        ("minus", 0o055, '\u{2212}'),
        ("minute", 0o242, '\u{2032}'),
        ("mu", 0o155, '\u{00B5}'),
        ("multiply", 0o264, '\u{00D7}'),
        ("nine", 0o071, '\u{0039}'),
        ("notelement", 0o317, '\u{2209}'),
        ("notequal", 0o271, '\u{2260}'),
        ("notsubset", 0o313, '\u{2284}'),
        ("nu", 0o156, '\u{03BD}'),
        ("numbersign", 0o043, '\u{0023}'),
        ("omega", 0o167, '\u{03C9}'),
        ("omega1", 0o166, '\u{03D6}'),
        ("omicron", 0o157, '\u{03BF}'),
        ("one", 0o061, '\u{0031}'),
        ("parenleft", 0o050, '\u{0028}'),
        ("parenleftbt", 0o350, '\u{F8ED}'),
        ("parenleftex", 0o347, '\u{F8EC}'),
        ("parenlefttp", 0o346, '\u{F8EB}'),
        ("parenright", 0o051, '\u{0029}'),
        ("parenrightbt", 0o370, '\u{F8F8}'),
        ("parenrightex", 0o367, '\u{F8F7}'),
        ("parenrighttp", 0o366, '\u{F8F6}'),
        ("partialdiff", 0o266, '\u{2202}'),
        ("percent", 0o045, '\u{0025}'),
        ("period", 0o056, '\u{002E}'),
        ("perpendicular", 0o136, '\u{22A5}'),
        ("phi", 0o146, '\u{03C6}'),
        ("phi1", 0o152, '\u{03D5}'),
        ("pi", 0o160, '\u{03C0}'),
        ("plus", 0o053, '\u{002B}'),
        ("plusminus", 0o261, '\u{00B1}'),
        ("product", 0o325, '\u{220F}'),
        ("propersubset", 0o314, '\u{2282}'),
        ("propersuperset", 0o311, '\u{2283}'),
        ("proportional", 0o265, '\u{221D}'),
        ("psi", 0o171, '\u{03C8}'),
        ("question", 0o077, '\u{003F}'),
        ("radical", 0o326, '\u{221A}'),
        ("radicalex", 0o140, '\u{F8E5}'),
        ("reflexsubset", 0o315, '\u{2286}'),
        ("reflexsuperset", 0o312, '\u{2287}'),
        ("registersans", 0o342, '\u{F8E8}'),
        ("registerserif", 0o322, '\u{F6DA}'),
        ("rho", 0o162, '\u{03C1}'),
        ("second", 0o262, '\u{2033}'),
        ("semicolon", 0o073, '\u{003B}'),
        ("seven", 0o067, '\u{0037}'),
        ("sigma", 0o163, '\u{03C3}'),
        ("sigma1", 0o126, '\u{03C2}'),
        ("similar", 0o176, '\u{223C}'),
        ("six", 0o066, '\u{0036}'),
        ("slash", 0o057, '\u{002F}'),
        ("space", 0o040, '\u{0020}'),
        ("spade", 0o252, '\u{2660}'),
        ("suchthat", 0o047, '\u{220B}'),
        ("summation", 0o345, '\u{2211}'),
        ("tau", 0o164, '\u{03C4}'),
        ("therefore", 0o134, '\u{2234}'),
        ("theta", 0o161, '\u{03B8}'),
        ("theta1", 0o112, '\u{03D1}'),
        ("three", 0o063, '\u{0033}'),
        ("trademarksans", 0o344, '\u{F8EA}'),
        ("trademarkserif", 0o324, '\u{F6DB}'),
        ("two", 0o062, '\u{0032}'),
        ("underscore", 0o137, '\u{005F}'),
        ("union", 0o310, '\u{222A}'),
        ("universal", 0o042, '\u{2200}'),
        ("upsilon", 0o165, '\u{03C5}'),
        ("weierstrass", 0o303, '\u{2118}'),
        ("xi", 0o170, '\u{03BE}'),
        ("zero", 0o060, '\u{0030}'),
        ("zeta", 0o172, '\u{03B6}'),
    ];

    /// Table D.6 again, keyed by name where the table is ordered by code, and
    /// each value read again from the ITC Zapf Dingbats and Adobe Glyph Lists.
    const DINGBATS_BY_NAME: [(&str, u8, char); 188] = [
        ("a1", 0o041, '\u{2701}'),
        ("a10", 0o101, '\u{2721}'),
        ("a100", 0o176, '\u{275E}'),
        ("a101", 0o241, '\u{2761}'),
        ("a102", 0o242, '\u{2762}'),
        ("a103", 0o243, '\u{2763}'),
        ("a104", 0o244, '\u{2764}'),
        ("a105", 0o060, '\u{2710}'),
        ("a106", 0o245, '\u{2765}'),
        ("a107", 0o246, '\u{2766}'),
        ("a108", 0o247, '\u{2767}'),
        ("a109", 0o253, '\u{2660}'),
        ("a11", 0o052, '\u{261B}'),
        ("a110", 0o252, '\u{2665}'),
        ("a111", 0o251, '\u{2666}'),
        ("a112", 0o250, '\u{2663}'),
        ("a117", 0o051, '\u{2709}'),
        ("a118", 0o050, '\u{2708}'),
        ("a119", 0o047, '\u{2707}'),
        ("a12", 0o053, '\u{261E}'),
        ("a120", 0o254, '\u{2460}'),
        ("a121", 0o255, '\u{2461}'),
        ("a122", 0o256, '\u{2462}'),
        ("a123", 0o257, '\u{2463}'),
        ("a124", 0o260, '\u{2464}'),
        ("a125", 0o261, '\u{2465}'),
        ("a126", 0o262, '\u{2466}'),
        ("a127", 0o263, '\u{2467}'),
        ("a128", 0o264, '\u{2468}'),
        ("a129", 0o265, '\u{2469}'),
        ("a13", 0o054, '\u{270C}'),
        ("a130", 0o266, '\u{2776}'),
        ("a131", 0o267, '\u{2777}'),
        ("a132", 0o270, '\u{2778}'),
        ("a133", 0o271, '\u{2779}'),
        ("a134", 0o272, '\u{277A}'),
        ("a135", 0o273, '\u{277B}'),
        ("a136", 0o274, '\u{277C}'),
        ("a137", 0o275, '\u{277D}'),
        ("a138", 0o276, '\u{277E}'),
        ("a139", 0o277, '\u{277F}'),
        ("a14", 0o055, '\u{270D}'),
        ("a140", 0o300, '\u{2780}'),
        ("a141", 0o301, '\u{2781}'),
        ("a142", 0o302, '\u{2782}'),
        ("a143", 0o303, '\u{2783}'),
        ("a144", 0o304, '\u{2784}'),
        ("a145", 0o305, '\u{2785}'),
        ("a146", 0o306, '\u{2786}'),
        ("a147", 0o307, '\u{2787}'),
        ("a148", 0o310, '\u{2788}'),
        ("a149", 0o311, '\u{2789}'),
        ("a15", 0o056, '\u{270E}'),
        ("a150", 0o312, '\u{278A}'),
        ("a151", 0o313, '\u{278B}'),
        ("a152", 0o314, '\u{278C}'),
        ("a153", 0o315, '\u{278D}'),
        ("a154", 0o316, '\u{278E}'),
        ("a155", 0o317, '\u{278F}'),
        ("a156", 0o320, '\u{2790}'),
        ("a157", 0o321, '\u{2791}'),
        ("a158", 0o322, '\u{2792}'),
        ("a159", 0o323, '\u{2793}'),
        ("a16", 0o057, '\u{270F}'),
        ("a160", 0o324, '\u{2794}'),
        ("a161", 0o325, '\u{2192}'),
        ("a162", 0o343, '\u{27A3}'),
        ("a163", 0o326, '\u{2194}'),
        ("a164", 0o327, '\u{2195}'),
        ("a165", 0o331, '\u{2799}'),
        ("a166", 0o333, '\u{279B}'),
        ("a167", 0o334, '\u{279C}'),
        ("a168", 0o335, '\u{279D}'),
        ("a169", 0o336, '\u{279E}'),
        ("a17", 0o061, '\u{2711}'),
        ("a170", 0o337, '\u{279F}'),
        ("a171", 0o340, '\u{27A0}'),
        ("a172", 0o341, '\u{27A1}'),
        ("a173", 0o342, '\u{27A2}'),
        ("a174", 0o344, '\u{27A4}'),
        ("a175", 0o345, '\u{27A5}'),
        ("a176", 0o346, '\u{27A6}'),
        ("a177", 0o347, '\u{27A7}'),
        ("a178", 0o350, '\u{27A8}'),
        ("a179", 0o351, '\u{27A9}'),
        ("a18", 0o062, '\u{2712}'),
        ("a180", 0o353, '\u{27AB}'),
        ("a181", 0o355, '\u{27AD}'),
        ("a182", 0o357, '\u{27AF}'),
        ("a183", 0o362, '\u{27B2}'),
        ("a184", 0o363, '\u{27B3}'),
        ("a185", 0o365, '\u{27B5}'),
        ("a186", 0o370, '\u{27B8}'),
        ("a187", 0o372, '\u{27BA}'),
        ("a188", 0o373, '\u{27BB}'),
        ("a189", 0o374, '\u{27BC}'),
        ("a19", 0o063, '\u{2713}'),
        ("a190", 0o375, '\u{27BD}'),
        ("a191", 0o376, '\u{27BE}'),
        ("a192", 0o332, '\u{279A}'),
        ("a193", 0o352, '\u{27AA}'),
        ("a194", 0o366, '\u{27B6}'),
        ("a195", 0o371, '\u{27B9}'),
        ("a196", 0o330, '\u{2798}'),
        ("a197", 0o364, '\u{27B4}'),
        ("a198", 0o367, '\u{27B7}'),
        ("a199", 0o354, '\u{27AC}'),
        ("a2", 0o042, '\u{2702}'),
        ("a20", 0o064, '\u{2714}'),
        ("a200", 0o356, '\u{27AE}'),
        ("a201", 0o361, '\u{27B1}'),
        ("a202", 0o043, '\u{2703}'),
        ("a203", 0o160, '\u{2750}'),
        ("a204", 0o162, '\u{2752}'),
        ("a21", 0o065, '\u{2715}'),
        ("a22", 0o066, '\u{2716}'),
        ("a23", 0o067, '\u{2717}'),
        ("a24", 0o070, '\u{2718}'),
        ("a25", 0o071, '\u{2719}'),
        ("a26", 0o072, '\u{271A}'),
        ("a27", 0o073, '\u{271B}'),
        ("a28", 0o074, '\u{271C}'),
        ("a29", 0o102, '\u{2722}'),
        ("a3", 0o044, '\u{2704}'),
        ("a30", 0o103, '\u{2723}'),
        ("a31", 0o104, '\u{2724}'),
        ("a32", 0o105, '\u{2725}'),
        ("a33", 0o106, '\u{2726}'),
        ("a34", 0o107, '\u{2727}'),
        ("a35", 0o110, '\u{2605}'),
        ("a36", 0o111, '\u{2729}'),
        ("a37", 0o112, '\u{272A}'),
        ("a38", 0o113, '\u{272B}'),
        ("a39", 0o114, '\u{272C}'),
        ("a4", 0o045, '\u{260E}'),
        ("a40", 0o115, '\u{272D}'),
        ("a41", 0o116, '\u{272E}'),
        ("a42", 0o117, '\u{272F}'),
        ("a43", 0o120, '\u{2730}'),
        ("a44", 0o121, '\u{2731}'),
        ("a45", 0o122, '\u{2732}'),
        ("a46", 0o123, '\u{2733}'),
        ("a47", 0o124, '\u{2734}'),
        ("a48", 0o125, '\u{2735}'),
        ("a49", 0o126, '\u{2736}'),
        ("a5", 0o046, '\u{2706}'),
        ("a50", 0o127, '\u{2737}'),
        ("a51", 0o130, '\u{2738}'),
        ("a52", 0o131, '\u{2739}'),
        ("a53", 0o132, '\u{273A}'),
        ("a54", 0o133, '\u{273B}'),
        ("a55", 0o134, '\u{273C}'),
        ("a56", 0o135, '\u{273D}'),
        ("a57", 0o136, '\u{273E}'),
        ("a58", 0o137, '\u{273F}'),
        ("a59", 0o140, '\u{2740}'),
        ("a6", 0o075, '\u{271D}'),
        ("a60", 0o141, '\u{2741}'),
        ("a61", 0o142, '\u{2742}'),
        ("a62", 0o143, '\u{2743}'),
        ("a63", 0o144, '\u{2744}'),
        ("a64", 0o145, '\u{2745}'),
        ("a65", 0o146, '\u{2746}'),
        ("a66", 0o147, '\u{2747}'),
        ("a67", 0o150, '\u{2748}'),
        ("a68", 0o151, '\u{2749}'),
        ("a69", 0o152, '\u{274A}'),
        ("a7", 0o076, '\u{271E}'),
        ("a70", 0o153, '\u{274B}'),
        ("a71", 0o154, '\u{25CF}'),
        ("a72", 0o155, '\u{274D}'),
        ("a73", 0o156, '\u{25A0}'),
        ("a74", 0o157, '\u{274F}'),
        ("a75", 0o161, '\u{2751}'),
        ("a76", 0o163, '\u{25B2}'),
        ("a77", 0o164, '\u{25BC}'),
        ("a78", 0o165, '\u{25C6}'),
        ("a79", 0o166, '\u{2756}'),
        ("a8", 0o077, '\u{271F}'),
        ("a81", 0o167, '\u{25D7}'),
        ("a82", 0o170, '\u{2758}'),
        ("a83", 0o171, '\u{2759}'),
        ("a84", 0o172, '\u{275A}'),
        ("a9", 0o100, '\u{2720}'),
        ("a97", 0o173, '\u{275B}'),
        ("a98", 0o174, '\u{275C}'),
        ("a99", 0o175, '\u{275D}'),
        ("space", 0o040, '\u{0020}'),
    ];

    /// The ITC list's names Table D.6 does not encode, read again, by name.
    const DINGBATS_UNENCODED_BY_NAME: [(&str, char); 14] = [
        ("a205", '\u{276E}'),
        ("a206", '\u{2770}'),
        ("a85", '\u{276F}'),
        ("a86", '\u{2771}'),
        ("a87", '\u{2772}'),
        ("a88", '\u{2773}'),
        ("a89", '\u{2768}'),
        ("a90", '\u{2769}'),
        ("a91", '\u{276C}'),
        ("a92", '\u{276D}'),
        ("a93", '\u{276A}'),
        ("a94", '\u{276B}'),
        ("a95", '\u{2774}'),
        ("a96", '\u{2775}'),
    ];

    /// Holds one built-in encoding to its table's count, its codes, and the
    /// second transcription keyed by name, pair by pair and value by value,
    /// both ways round.
    fn holds_to_the_table(
        encoding: BuiltInEncoding,
        count: usize,
        listed: &[u8],
        by_name: &[(&str, u8, char)],
    ) {
        let codes = encoding.codes();
        assert_eq!(codes.len(), count, "{encoding:?} lists {count} codes");
        assert!(
            codes.windows(2).all(|pair| pair[0].code < pair[1].code),
            "{encoding:?}: ascending, so no code twice"
        );
        let names: std::collections::BTreeSet<&str> =
            codes.iter().map(|entry| entry.name).collect();
        assert_eq!(names.len(), count, "{encoding:?}: no name twice");
        let expected: Vec<u8> = codes.iter().map(|entry| entry.code).collect();
        assert_eq!(expected, listed, "{encoding:?}: the codes the table lists");

        assert_eq!(by_name.len(), count);
        assert!(
            by_name.windows(2).all(|pair| pair[0].0 < pair[1].0),
            "the second transcription is keyed by name"
        );
        for (name, code, unicode) in by_name {
            assert_eq!(
                encoding.glyph_name(*code),
                Some(*name),
                "{encoding:?} {code:#o}"
            );
            assert_eq!(
                encoding.unicode_of(name),
                Some(*unicode),
                "{encoding:?} {name}"
            );
        }
        for entry in codes {
            assert!(
                by_name.contains(&(entry.name, entry.code, entry.unicode)),
                "{encoding:?}: {entry:?} is in the second transcription"
            );
        }
        // A code the table does not list has no name.
        for code in 0..=255u8 {
            assert_eq!(
                encoding.glyph_name(code).is_some(),
                listed.contains(&code),
                "{encoding:?} {code:#o}"
            );
        }
    }

    /// FT-03.2: Table D.5 lists 189 codes, 32 to 126 and 160 to 254 but 240.
    #[test]
    fn the_symbol_built_in_encoding_names_189_codes() {
        let listed: Vec<u8> = (32..=126u8)
            .chain(160..=254)
            .filter(|code| *code != 240)
            .collect();
        holds_to_the_table(BuiltInEncoding::Symbol, 189, &listed, &SYMBOL_BY_NAME);
        assert_eq!(BuiltInEncoding::Symbol.glyph_name(0o141), Some("alpha"));
        assert_eq!(
            BuiltInEncoding::Symbol.glyph_name(0o360),
            None,
            "240 is not listed"
        );
        assert_eq!(BuiltInEncoding::Symbol.unicode_of("Euro"), Some('\u{20AC}'));
    }

    /// FT-03.2: Table D.6 lists 188 codes, 32 to 126, 161 to 239 and 241 to
    /// 254; 128 to 141 are not among them.
    #[test]
    fn the_dingbats_built_in_encoding_names_188_codes() {
        let listed: Vec<u8> = (32..=126u8).chain(161..=239).chain(241..=254).collect();
        holds_to_the_table(
            BuiltInEncoding::ZapfDingbats,
            188,
            &listed,
            &DINGBATS_BY_NAME,
        );
        assert_eq!(BuiltInEncoding::ZapfDingbats.glyph_name(0o041), Some("a1"));
        for code in 128..=141u8 {
            assert_eq!(
                BuiltInEncoding::ZapfDingbats.glyph_name(code),
                None,
                "{code}"
            );
        }
    }

    /// The fourteen names the ITC list holds and Table D.6 does not encode
    /// keep their values, for a `/Differences` array that names them, and have
    /// no code.
    #[test]
    fn the_unencoded_dingbats_names_keep_their_values() {
        assert_eq!(DINGBATS_UNENCODED.len(), 14);
        for (name, unicode) in DINGBATS_UNENCODED_BY_NAME {
            assert_eq!(
                BuiltInEncoding::ZapfDingbats.unicode_of(name),
                Some(unicode),
                "{name}"
            );
            assert!(
                BuiltInEncoding::ZapfDingbats
                    .codes()
                    .iter()
                    .all(|entry| entry.name != name),
                "{name} has no code"
            );
            assert_eq!(
                BuiltInEncoding::Symbol.unicode_of(name),
                None,
                "Symbol's list is another"
            );
        }
    }

    /// Each font answers from its own list: a name the other font's table holds,
    /// or none holds, has no value here.
    #[test]
    fn a_name_outside_the_fonts_list_has_no_value_here() {
        assert_eq!(BuiltInEncoding::Symbol.unicode_of("a1"), None);
        assert_eq!(BuiltInEncoding::ZapfDingbats.unicode_of("alpha"), None);
        assert_eq!(
            BuiltInEncoding::Symbol.unicode_of("A"),
            None,
            "Symbol's A is Alpha"
        );
        assert_eq!(BuiltInEncoding::Symbol.unicode_of(""), None);
        assert_eq!(
            BuiltInEncoding::ZapfDingbats.unicode_of("nosuchglyph"),
            None
        );
        // Both fonts encode `space`, and both lists give it U+0020.
        for encoding in [BuiltInEncoding::Symbol, BuiltInEncoding::ZapfDingbats] {
            assert_eq!(encoding.glyph_name(0o040), Some("space"));
            assert_eq!(encoding.unicode_of("space"), Some(' '));
        }
    }
}
