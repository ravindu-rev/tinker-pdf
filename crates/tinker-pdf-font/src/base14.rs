//! Metrics for the 14 standard fonts (9.6.2.2, Annex D.5).
//!
//! A simple font may omit `/Widths` entirely when it names one of these, and
//! plenty do, so the numbers have to live somewhere. They are Adobe's
//! published AFM advance widths in 1/1000 em.
//!
//! **Coverage is the printable ASCII range**, which is what these tables hold
//! exactly. Above 0x7F a width is approximated from the glyph's base letter —
//! `eacute` takes `e`'s advance, which is true in these faces to within a unit
//! or two and far better than a flat default. The approximation is reported,
//! never silent: see [`Metrics::width_of`]'s `exact` flag.

/// Which of the standard faces a font resolves to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standard14 {
    /// Helvetica, and its oblique, which share advances.
    Helvetica,
    /// Helvetica-Bold and Helvetica-BoldOblique.
    HelveticaBold,
    /// Times-Roman.
    TimesRoman,
    /// Times-Bold.
    TimesBold,
    /// Times-Italic.
    TimesItalic,
    /// Times-BoldItalic.
    TimesBoldItalic,
    /// Any Courier, all of which are monospaced at 600.
    Courier,
    /// Symbol.
    Symbol,
    /// ZapfDingbats.
    ZapfDingbats,
}

/// Advances for codes 32..=126, in 1/1000 em.
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, //
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, //
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, //
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556, //
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, //
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, //
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611, //
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778, //
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556, //
    333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611, //
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

const TIMES_ROMAN: [u16; 95] = [
    250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250, 278, //
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 278, 278, 564, 564, 564, 444, //
    921, 722, 667, 667, 722, 611, 556, 722, 722, 333, 389, 722, 611, 889, 722, 722, //
    556, 722, 667, 556, 611, 722, 722, 944, 722, 722, 611, 333, 278, 333, 469, 500, //
    333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500, 278, 778, 500, 500, //
    500, 500, 333, 389, 278, 500, 500, 722, 500, 500, 444, 480, 200, 480, 541,
];

const TIMES_BOLD: [u16; 95] = [
    250, 333, 555, 500, 500, 1000, 833, 278, 333, 333, 500, 570, 250, 333, 250, 278, //
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333, 333, 570, 570, 570, 500, //
    930, 722, 667, 722, 722, 667, 611, 778, 778, 389, 500, 778, 667, 944, 722, 778, //
    611, 778, 722, 556, 667, 722, 722, 1000, 722, 722, 667, 333, 278, 333, 581, 500, //
    333, 500, 556, 444, 556, 444, 333, 500, 556, 278, 333, 556, 278, 833, 556, 500, //
    556, 556, 444, 389, 333, 556, 500, 722, 500, 500, 444, 394, 220, 394, 520,
];

const TIMES_ITALIC: [u16; 95] = [
    250, 333, 420, 500, 500, 833, 778, 214, 333, 333, 500, 675, 250, 333, 250, 278, //
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333, 333, 675, 675, 675, 500, //
    920, 611, 611, 667, 722, 611, 611, 722, 722, 333, 444, 667, 556, 833, 667, 722, //
    611, 722, 611, 500, 556, 722, 611, 833, 611, 556, 556, 389, 278, 389, 422, 500, //
    333, 500, 500, 444, 500, 444, 278, 500, 500, 278, 278, 444, 278, 722, 500, 500, //
    500, 500, 389, 389, 278, 500, 444, 667, 444, 444, 389, 400, 275, 400, 541,
];

const TIMES_BOLD_ITALIC: [u16; 95] = [
    250, 389, 555, 500, 500, 833, 778, 278, 333, 333, 500, 570, 250, 333, 250, 278, //
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333, 333, 570, 570, 570, 500, //
    832, 667, 667, 667, 722, 667, 667, 722, 778, 389, 500, 667, 611, 889, 722, 722, //
    611, 722, 667, 556, 611, 722, 667, 889, 667, 611, 611, 333, 278, 333, 570, 500, //
    333, 500, 500, 444, 500, 444, 333, 500, 556, 278, 278, 500, 278, 778, 556, 500, //
    500, 500, 389, 389, 278, 556, 444, 667, 500, 444, 389, 348, 220, 348, 570,
];

/// The advance every Courier glyph has: it is a monospaced face.
const COURIER_ADVANCE: u16 = 600;

/// Symbol and ZapfDingbats have no relation to text encodings, and a document
/// using them without `/Widths` is rare enough that a face-wide default beats
/// a table of numbers nobody can check.
const SYMBOLIC_DEFAULT: u16 = 600;

impl Standard14 {
    /// Matches a `/BaseFont` name, including the common aliases documents use.
    ///
    /// Subset prefixes (`ABCDEF+Helvetica`) are stripped, style words are read
    /// from whatever separator the producer chose, and the substitutions every
    /// viewer makes — Arial for Helvetica, TimesNewRoman for Times — are
    /// applied, because a document naming Arial with no `/Widths` still has to
    /// lay out.
    #[must_use]
    pub fn from_base_font(name: &str) -> Option<Standard14> {
        // 9.6.4: a subset name is six uppercase letters and a plus sign.
        let name = name.split_once('+').map_or(name, |(_, rest)| rest);
        let lower = name.to_ascii_lowercase();
        let lower = lower.replace([' ', '-', '_', ','], "");

        let bold = lower.contains("bold") || lower.contains("black") || lower.contains("heavy");
        let italic = lower.contains("italic") || lower.contains("oblique");

        if lower.contains("courier") || lower.contains("mono") {
            return Some(Standard14::Courier);
        }
        if lower.contains("zapf") || lower.contains("dingbat") {
            return Some(Standard14::ZapfDingbats);
        }
        if lower.starts_with("symbol") {
            return Some(Standard14::Symbol);
        }
        if lower.contains("times") || lower.contains("serif") || lower.contains("roman") {
            return Some(match (bold, italic) {
                (true, true) => Standard14::TimesBoldItalic,
                (true, false) => Standard14::TimesBold,
                (false, true) => Standard14::TimesItalic,
                (false, false) => Standard14::TimesRoman,
            });
        }
        if lower.contains("helvetica") || lower.contains("arial") || lower.contains("sans") {
            return Some(if bold {
                Standard14::HelveticaBold
            } else {
                Standard14::Helvetica
            });
        }
        None
    }

    fn table(self) -> Option<&'static [u16; 95]> {
        match self {
            Standard14::Helvetica => Some(&HELVETICA),
            Standard14::HelveticaBold => Some(&HELVETICA_BOLD),
            Standard14::TimesRoman => Some(&TIMES_ROMAN),
            Standard14::TimesBold => Some(&TIMES_BOLD),
            Standard14::TimesItalic => Some(&TIMES_ITALIC),
            Standard14::TimesBoldItalic => Some(&TIMES_BOLD_ITALIC),
            Standard14::Courier | Standard14::Symbol | Standard14::ZapfDingbats => None,
        }
    }

    /// The advance of the character `c`, in 1/1000 em, and whether the number
    /// is the published one rather than an approximation.
    #[must_use]
    pub fn advance(self, c: char) -> (u16, bool) {
        match self {
            Standard14::Courier => return (COURIER_ADVANCE, true),
            Standard14::Symbol | Standard14::ZapfDingbats => {
                return (SYMBOLIC_DEFAULT, false);
            }
            _ => {}
        }
        let Some(table) = self.table() else {
            return (SYMBOLIC_DEFAULT, false);
        };

        let index = |ch: char| -> Option<usize> {
            let code = u32::from(ch);
            (32..=126).contains(&code).then(|| (code - 32) as usize)
        };

        if let Some(w) = index(c).and_then(|i| table.get(i).copied()) {
            return (w, true);
        }

        // Above ASCII, approximate from the base letter: an accented glyph
        // carries its base's advance in these faces.
        if let Some(base) = base_letter(c) {
            if let Some(w) = index(base).and_then(|i| table.get(i).copied()) {
                return (w, false);
            }
        }

        // A space-like or unknown character: the face's own space advance is
        // the least wrong constant available.
        let space = table.first().copied().unwrap_or(SYMBOLIC_DEFAULT);
        (space, false)
    }
}

/// The unaccented letter an accented character is built on.
///
/// Deliberately small: it covers Latin-1 and Latin Extended-A, which is what
/// the base-14 faces can render at all.
fn base_letter(c: char) -> Option<char> {
    let table: &[(&str, char)] = &[
        ("ÀÁÂÃÄÅĀĂĄ", 'A'),
        ("àáâãäåāăą", 'a'),
        ("ÈÉÊËĒĔĖĘĚ", 'E'),
        ("èéêëēĕėęě", 'e'),
        ("ÌÍÎÏĨĪĬĮİ", 'I'),
        ("ìíîïĩīĭįı", 'i'),
        ("ÒÓÔÕÖØŌŎŐ", 'O'),
        ("òóôõöøōŏő", 'o'),
        ("ÙÚÛÜŨŪŬŮŰŲ", 'U'),
        ("ùúûüũūŭůűų", 'u'),
        ("ÝŶŸ", 'Y'),
        ("ýÿŷ", 'y'),
        ("ÑŃŅŇ", 'N'),
        ("ñńņň", 'n'),
        ("ÇĆĈĊČ", 'C'),
        ("çćĉċč", 'c'),
        ("ŚŜŞŠ", 'S'),
        ("śŝşš", 's'),
        ("ŹŻŽ", 'Z'),
        ("źżž", 'z'),
        ("ĜĞĠĢ", 'G'),
        ("ĝğġģ", 'g'),
        ("ŔŖŘ", 'R'),
        ("ŕŗř", 'r'),
        ("ĹĻĽŁ", 'L'),
        ("ĺļľł", 'l'),
        ("ŢŤ", 'T'),
        ("ţť", 't'),
        ("Ď", 'D'),
        ("ď", 'd'),
        ("Ĥ", 'H'),
        ("ĥ", 'h'),
        ("Ĵ", 'J'),
        ("ĵ", 'j'),
        ("Ķ", 'K'),
        ("ķ", 'k'),
        ("Ŵ", 'W'),
        ("ŵ", 'w'),
    ];
    table
        .iter()
        .find(|(set, _)| set.chars().any(|x| x == c))
        .map(|(_, base)| *base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helvetica_advances_match_the_published_metrics() {
        let f = Standard14::Helvetica;
        assert_eq!(f.advance(' '), (278, true));
        assert_eq!(f.advance('A'), (667, true));
        assert_eq!(f.advance('i'), (222, true));
        assert_eq!(f.advance('m'), (833, true));
        assert_eq!(f.advance('W'), (944, true));
        assert_eq!(f.advance('.'), (278, true));
    }

    #[test]
    fn times_and_courier_differ_as_they_should() {
        assert_eq!(Standard14::TimesRoman.advance('A'), (722, true));
        assert_eq!(Standard14::TimesRoman.advance('i'), (278, true));
        // Courier is monospaced, so every glyph is the same width.
        for c in ['A', 'i', 'W', ' ', '.'] {
            assert_eq!(Standard14::Courier.advance(c), (600, true));
        }
    }

    #[test]
    fn tables_are_the_right_length() {
        for table in [
            &HELVETICA,
            &HELVETICA_BOLD,
            &TIMES_ROMAN,
            &TIMES_BOLD,
            &TIMES_ITALIC,
            &TIMES_BOLD_ITALIC,
        ] {
            assert_eq!(table.len(), 95, "codes 32..=126 inclusive");
        }
    }

    #[test]
    fn accented_characters_borrow_their_base_advance_and_say_so() {
        let f = Standard14::Helvetica;
        let (w, exact) = f.advance('é');
        assert_eq!(w, f.advance('e').0);
        assert!(!exact, "an approximation must report itself");
        assert!(f.advance('e').1, "the base letter is exact");
    }

    #[test]
    fn base_font_names_resolve_through_their_aliases() {
        let cases = [
            ("Helvetica", Standard14::Helvetica),
            ("Helvetica-Oblique", Standard14::Helvetica),
            ("Helvetica-Bold", Standard14::HelveticaBold),
            ("ABCDEF+Helvetica", Standard14::Helvetica),
            ("Arial", Standard14::Helvetica),
            ("ArialMT", Standard14::Helvetica),
            ("Arial-BoldMT", Standard14::HelveticaBold),
            ("Times-Roman", Standard14::TimesRoman),
            ("TimesNewRoman", Standard14::TimesRoman),
            ("TimesNewRoman,BoldItalic", Standard14::TimesBoldItalic),
            ("Times-Italic", Standard14::TimesItalic),
            ("Courier", Standard14::Courier),
            ("CourierNew-Bold", Standard14::Courier),
            ("Symbol", Standard14::Symbol),
            ("ZapfDingbats", Standard14::ZapfDingbats),
        ];
        for (name, want) in cases {
            assert_eq!(
                Standard14::from_base_font(name),
                Some(want),
                "matching {name}"
            );
        }
        assert_eq!(Standard14::from_base_font("SomeEmbeddedFont"), None);
    }
}
