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
//!
//! **Symbol and ZapfDingbats are measured by glyph name**, every name their
//! AFM files list ([`Standard14::advance_of_name`]): their codes name glyphs
//! no text encoding knows (Annex D.5 and D.6), so a code is measured by the
//! name the document or the font's built-in encoding gives it
//! (`docs/design/symbol-faces.md`).

use crate::encoding::BuiltInEncoding;

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

/// Symbol's advances by glyph name, in 1/1000 em, sorted by name: Adobe's
/// published `Symbol.afm`, every name it lists — Table D.5's 189 and `apple`,
/// which no code encodes. `THIRDPARTY.md` records the file.
const SYMBOL_ADVANCES: [(&str, u16); 190] = [
    ("Alpha", 722),
    ("Beta", 667),
    ("Chi", 722),
    ("Delta", 612),
    ("Epsilon", 611),
    ("Eta", 722),
    ("Euro", 750),
    ("Gamma", 603),
    ("Ifraktur", 686),
    ("Iota", 333),
    ("Kappa", 722),
    ("Lambda", 686),
    ("Mu", 889),
    ("Nu", 722),
    ("Omega", 768),
    ("Omicron", 722),
    ("Phi", 763),
    ("Pi", 768),
    ("Psi", 795),
    ("Rfraktur", 795),
    ("Rho", 556),
    ("Sigma", 592),
    ("Tau", 611),
    ("Theta", 741),
    ("Upsilon", 690),
    ("Upsilon1", 620),
    ("Xi", 645),
    ("Zeta", 611),
    ("aleph", 823),
    ("alpha", 631),
    ("ampersand", 778),
    ("angle", 768),
    ("angleleft", 329),
    ("angleright", 329),
    ("apple", 790),
    ("approxequal", 549),
    ("arrowboth", 1042),
    ("arrowdblboth", 1042),
    ("arrowdbldown", 603),
    ("arrowdblleft", 987),
    ("arrowdblright", 987),
    ("arrowdblup", 603),
    ("arrowdown", 603),
    ("arrowhorizex", 1000),
    ("arrowleft", 987),
    ("arrowright", 987),
    ("arrowup", 603),
    ("arrowvertex", 603),
    ("asteriskmath", 500),
    ("bar", 200),
    ("beta", 549),
    ("braceex", 494),
    ("braceleft", 480),
    ("braceleftbt", 494),
    ("braceleftmid", 494),
    ("bracelefttp", 494),
    ("braceright", 480),
    ("bracerightbt", 494),
    ("bracerightmid", 494),
    ("bracerighttp", 494),
    ("bracketleft", 333),
    ("bracketleftbt", 384),
    ("bracketleftex", 384),
    ("bracketlefttp", 384),
    ("bracketright", 333),
    ("bracketrightbt", 384),
    ("bracketrightex", 384),
    ("bracketrighttp", 384),
    ("bullet", 460),
    ("carriagereturn", 658),
    ("chi", 549),
    ("circlemultiply", 768),
    ("circleplus", 768),
    ("club", 753),
    ("colon", 278),
    ("comma", 250),
    ("congruent", 549),
    ("copyrightsans", 790),
    ("copyrightserif", 790),
    ("degree", 400),
    ("delta", 494),
    ("diamond", 753),
    ("divide", 549),
    ("dotmath", 250),
    ("eight", 500),
    ("element", 713),
    ("ellipsis", 1000),
    ("emptyset", 823),
    ("epsilon", 439),
    ("equal", 549),
    ("equivalence", 549),
    ("eta", 603),
    ("exclam", 333),
    ("existential", 549),
    ("five", 500),
    ("florin", 500),
    ("four", 500),
    ("fraction", 167),
    ("gamma", 411),
    ("gradient", 713),
    ("greater", 549),
    ("greaterequal", 549),
    ("heart", 753),
    ("infinity", 713),
    ("integral", 274),
    ("integralbt", 686),
    ("integralex", 686),
    ("integraltp", 686),
    ("intersection", 768),
    ("iota", 329),
    ("kappa", 549),
    ("lambda", 549),
    ("less", 549),
    ("lessequal", 549),
    ("logicaland", 603),
    ("logicalnot", 713),
    ("logicalor", 603),
    ("lozenge", 494),
    ("minus", 549),
    ("minute", 247),
    ("mu", 576),
    ("multiply", 549),
    ("nine", 500),
    ("notelement", 713),
    ("notequal", 549),
    ("notsubset", 713),
    ("nu", 521),
    ("numbersign", 500),
    ("omega", 686),
    ("omega1", 713),
    ("omicron", 549),
    ("one", 500),
    ("parenleft", 333),
    ("parenleftbt", 384),
    ("parenleftex", 384),
    ("parenlefttp", 384),
    ("parenright", 333),
    ("parenrightbt", 384),
    ("parenrightex", 384),
    ("parenrighttp", 384),
    ("partialdiff", 494),
    ("percent", 833),
    ("period", 250),
    ("perpendicular", 658),
    ("phi", 521),
    ("phi1", 603),
    ("pi", 549),
    ("plus", 549),
    ("plusminus", 549),
    ("product", 823),
    ("propersubset", 713),
    ("propersuperset", 713),
    ("proportional", 713),
    ("psi", 686),
    ("question", 444),
    ("radical", 549),
    ("radicalex", 500),
    ("reflexsubset", 713),
    ("reflexsuperset", 713),
    ("registersans", 790),
    ("registerserif", 790),
    ("rho", 549),
    ("second", 411),
    ("semicolon", 278),
    ("seven", 500),
    ("sigma", 603),
    ("sigma1", 439),
    ("similar", 549),
    ("six", 500),
    ("slash", 278),
    ("space", 250),
    ("spade", 753),
    ("suchthat", 439),
    ("summation", 713),
    ("tau", 439),
    ("therefore", 863),
    ("theta", 521),
    ("theta1", 631),
    ("three", 500),
    ("trademarksans", 786),
    ("trademarkserif", 890),
    ("two", 500),
    ("underscore", 500),
    ("union", 768),
    ("universal", 713),
    ("upsilon", 576),
    ("weierstrass", 987),
    ("xi", 493),
    ("zero", 500),
    ("zeta", 494),
];

/// ZapfDingbats' advances by glyph name, in 1/1000 em, sorted by name:
/// Adobe's published `ZapfDingbats.afm`, every name it lists — Table D.6's
/// 188 and the fourteen it encodes at 128 to 141, which the table does not.
const DINGBATS_ADVANCES: [(&str, u16); 202] = [
    ("a1", 974),
    ("a10", 692),
    ("a100", 668),
    ("a101", 732),
    ("a102", 544),
    ("a103", 544),
    ("a104", 910),
    ("a105", 911),
    ("a106", 667),
    ("a107", 760),
    ("a108", 760),
    ("a109", 626),
    ("a11", 960),
    ("a110", 694),
    ("a111", 595),
    ("a112", 776),
    ("a117", 690),
    ("a118", 791),
    ("a119", 790),
    ("a12", 939),
    ("a120", 788),
    ("a121", 788),
    ("a122", 788),
    ("a123", 788),
    ("a124", 788),
    ("a125", 788),
    ("a126", 788),
    ("a127", 788),
    ("a128", 788),
    ("a129", 788),
    ("a13", 549),
    ("a130", 788),
    ("a131", 788),
    ("a132", 788),
    ("a133", 788),
    ("a134", 788),
    ("a135", 788),
    ("a136", 788),
    ("a137", 788),
    ("a138", 788),
    ("a139", 788),
    ("a14", 855),
    ("a140", 788),
    ("a141", 788),
    ("a142", 788),
    ("a143", 788),
    ("a144", 788),
    ("a145", 788),
    ("a146", 788),
    ("a147", 788),
    ("a148", 788),
    ("a149", 788),
    ("a15", 911),
    ("a150", 788),
    ("a151", 788),
    ("a152", 788),
    ("a153", 788),
    ("a154", 788),
    ("a155", 788),
    ("a156", 788),
    ("a157", 788),
    ("a158", 788),
    ("a159", 788),
    ("a16", 933),
    ("a160", 894),
    ("a161", 838),
    ("a162", 924),
    ("a163", 1016),
    ("a164", 458),
    ("a165", 924),
    ("a166", 918),
    ("a167", 927),
    ("a168", 928),
    ("a169", 928),
    ("a17", 945),
    ("a170", 834),
    ("a171", 873),
    ("a172", 828),
    ("a173", 924),
    ("a174", 917),
    ("a175", 930),
    ("a176", 931),
    ("a177", 463),
    ("a178", 883),
    ("a179", 836),
    ("a18", 974),
    ("a180", 867),
    ("a181", 696),
    ("a182", 874),
    ("a183", 760),
    ("a184", 946),
    ("a185", 865),
    ("a186", 967),
    ("a187", 831),
    ("a188", 873),
    ("a189", 927),
    ("a19", 755),
    ("a190", 970),
    ("a191", 918),
    ("a192", 748),
    ("a193", 836),
    ("a194", 771),
    ("a195", 888),
    ("a196", 748),
    ("a197", 771),
    ("a198", 888),
    ("a199", 867),
    ("a2", 961),
    ("a20", 846),
    ("a200", 696),
    ("a201", 874),
    ("a202", 974),
    ("a203", 762),
    ("a204", 759),
    ("a205", 509),
    ("a206", 410),
    ("a21", 762),
    ("a22", 761),
    ("a23", 571),
    ("a24", 677),
    ("a25", 763),
    ("a26", 760),
    ("a27", 759),
    ("a28", 754),
    ("a29", 786),
    ("a3", 980),
    ("a30", 788),
    ("a31", 788),
    ("a32", 790),
    ("a33", 793),
    ("a34", 794),
    ("a35", 816),
    ("a36", 823),
    ("a37", 789),
    ("a38", 841),
    ("a39", 823),
    ("a4", 719),
    ("a40", 833),
    ("a41", 816),
    ("a42", 831),
    ("a43", 923),
    ("a44", 744),
    ("a45", 723),
    ("a46", 749),
    ("a47", 790),
    ("a48", 792),
    ("a49", 695),
    ("a5", 789),
    ("a50", 776),
    ("a51", 768),
    ("a52", 792),
    ("a53", 759),
    ("a54", 707),
    ("a55", 708),
    ("a56", 682),
    ("a57", 701),
    ("a58", 826),
    ("a59", 815),
    ("a6", 494),
    ("a60", 789),
    ("a61", 789),
    ("a62", 707),
    ("a63", 687),
    ("a64", 696),
    ("a65", 689),
    ("a66", 786),
    ("a67", 787),
    ("a68", 713),
    ("a69", 791),
    ("a7", 552),
    ("a70", 785),
    ("a71", 791),
    ("a72", 873),
    ("a73", 761),
    ("a74", 762),
    ("a75", 759),
    ("a76", 892),
    ("a77", 892),
    ("a78", 788),
    ("a79", 784),
    ("a8", 537),
    ("a81", 438),
    ("a82", 138),
    ("a83", 277),
    ("a84", 415),
    ("a85", 509),
    ("a86", 410),
    ("a87", 234),
    ("a88", 234),
    ("a89", 390),
    ("a9", 577),
    ("a90", 390),
    ("a91", 276),
    ("a92", 276),
    ("a93", 317),
    ("a94", 317),
    ("a95", 334),
    ("a96", 334),
    ("a97", 392),
    ("a98", 392),
    ("a99", 668),
    ("space", 278),
];

/// The advance every Courier glyph has: it is a monospaced face.
const COURIER_ADVANCE: u16 = 600;

/// What a Symbol or ZapfDingbats glyph the metrics do not name is laid out
/// with, reported as inexact, and what a character of either is measured with:
/// their glyphs are measured by name, and a character names none of them.
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

    /// The built-in encoding (Annex D.5, D.6) of the symbolic standard font
    /// `base_font` names — Symbol's or ZapfDingbats' — which a face
    /// substituted for it is read by and its codes are measured by, or `None`
    /// for every other name.
    ///
    /// Narrower than [`Standard14::from_base_font`], which reads any name
    /// holding `zapf` as ZapfDingbats and so takes Zapf Chancery, a text face,
    /// for it: here a name is ZapfDingbats only where it names dingbats, so a
    /// text face keeps the readings a text face gets.
    #[must_use]
    pub fn built_in_encoding_of(base_font: &str) -> Option<BuiltInEncoding> {
        match Standard14::from_base_font(base_font)? {
            Standard14::Symbol => Some(BuiltInEncoding::Symbol),
            Standard14::ZapfDingbats if base_font.to_ascii_lowercase().contains("dingbat") => {
                Some(BuiltInEncoding::ZapfDingbats)
            }
            _ => None,
        }
    }

    /// The advance of the glyph `name` names, in 1/1000 em, and whether it is
    /// the published one, for Symbol and ZapfDingbats: the advance Adobe's
    /// Core 14 metrics give the name, and for a name they do not list the
    /// face-wide 600, reported as inexact. `None` for every other face, whose
    /// glyphs are measured by character ([`Standard14::advance`]).
    #[must_use]
    pub fn advance_of_name(self, name: &str) -> Option<(u16, bool)> {
        let table: &[(&str, u16)] = match self {
            Standard14::Symbol => &SYMBOL_ADVANCES,
            Standard14::ZapfDingbats => &DINGBATS_ADVANCES,
            _ => return None,
        };
        let found = table
            .binary_search_by(|(listed, _)| (*listed).cmp(name))
            .ok()
            .and_then(|at| table.get(at));
        Some(found.map_or((SYMBOLIC_DEFAULT, false), |(_, advance)| (*advance, true)))
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

    /// **The symbolic faces' advances are read by name** (FT-03.4): Adobe's
    /// published metrics, each name once and in order, so the tables are
    /// searched; every name of each built-in encoding has its advance, and the
    /// names no code of the tables encodes keep theirs; a name the metrics do
    /// not list is the face-wide default, reported as inexact; and a text face
    /// measures nothing by name.
    #[test]
    fn the_symbolic_faces_advances_are_read_by_name() {
        for (table, count) in [
            (&SYMBOL_ADVANCES[..], 190usize),
            (&DINGBATS_ADVANCES[..], 202),
        ] {
            assert_eq!(table.len(), count);
            assert!(
                table.windows(2).all(|pair| pair[0].0 < pair[1].0),
                "sorted by name, each name once"
            );
        }
        let symbol = Standard14::Symbol;
        for (name, want) in [
            ("space", 250),
            ("exclam", 333),
            ("universal", 713),
            ("Alpha", 722),
            ("Beta", 667),
            ("alpha", 631),
            ("Euro", 750),
            ("fraction", 167),
            ("integral", 274),
            ("arrowboth", 1042),
            ("trademarkserif", 890),
            ("apple", 790),
        ] {
            assert_eq!(
                symbol.advance_of_name(name),
                Some((want, true)),
                "Symbol's {name}"
            );
        }
        let dingbats = Standard14::ZapfDingbats;
        for (name, want) in [
            ("space", 278),
            ("a1", 974),
            ("a2", 961),
            ("a20", 846),
            ("a82", 138),
            ("a163", 1016),
            ("a191", 918),
            ("a85", 509),
            ("a96", 334),
        ] {
            assert_eq!(
                dingbats.advance_of_name(name),
                Some((want, true)),
                "ZapfDingbats' {name}"
            );
        }
        for (face, encoding) in [
            (symbol, BuiltInEncoding::Symbol),
            (dingbats, BuiltInEncoding::ZapfDingbats),
        ] {
            for entry in encoding.codes() {
                assert!(
                    face.advance_of_name(entry.name)
                        .is_some_and(|(_, exact)| exact),
                    "{face:?}'s {}",
                    entry.name
                );
            }
        }
        assert_eq!(
            symbol.advance_of_name("a1"),
            Some((600, false)),
            "a ZapfDingbats name in Symbol"
        );
        assert_eq!(dingbats.advance_of_name("alpha"), Some((600, false)));
        assert_eq!(symbol.advance_of_name(""), Some((600, false)));
        assert_eq!(Standard14::Helvetica.advance_of_name("space"), None);
        assert_eq!(Standard14::Courier.advance_of_name("a1"), None);
    }

    /// A name takes a built-in encoding only where it names Symbol or
    /// ZapfDingbats: aliases, subset tags and style suffixes included, and
    /// Zapf Chancery, which the looser match reads as ZapfDingbats, not.
    #[test]
    fn only_symbol_and_dingbats_names_take_a_built_in_encoding() {
        for name in ["Symbol", "SymbolMT", "ABCDEF+Symbol", "Symbol,Bold"] {
            assert_eq!(
                Standard14::built_in_encoding_of(name),
                Some(BuiltInEncoding::Symbol),
                "{name}"
            );
        }
        for name in [
            "ZapfDingbats",
            "ITCZapfDingbats",
            "ZapfDingbatsITC",
            "Dingbats",
            "ABCDEF+ZapfDingbats",
        ] {
            assert_eq!(
                Standard14::built_in_encoding_of(name),
                Some(BuiltInEncoding::ZapfDingbats),
                "{name}"
            );
        }
        for name in [
            "ZapfChancery-MediumItalic",
            "Helvetica",
            "Times-Roman",
            "Courier",
            "Wingdings",
            "SomeEmbeddedFont",
            "",
        ] {
            assert_eq!(Standard14::built_in_encoding_of(name), None, "{name}");
        }
        assert_eq!(
            Standard14::from_base_font("ZapfChancery-MediumItalic"),
            Some(Standard14::ZapfDingbats),
            "the looser match, whose character widths Zapf Chancery keeps"
        );
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
