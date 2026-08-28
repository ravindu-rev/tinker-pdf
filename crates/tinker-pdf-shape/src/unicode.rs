//! The vendored UCD, compiled into static ranges by `build.rs`.
//!
//! Nothing here decides anything: it answers *what property does this
//! character have*, and [`crate::bidi`] and [`crate::shape`] are where the
//! answers turn into embedding levels and glyph runs. The split is the same
//! one `tinker-pdf-layout`'s `unicode` module draws, and for the same reason —
//! the tables are somebody else's published facts and the algorithm is ours,
//! and the two are wrong in completely different ways.
//!
//! # Why the generated table names variants rather than indices
//!
//! `build.rs` writes `BidiClass::AL` and `Script::Ethiopic` into every row
//! rather than a number, so a value this crate has never heard of **fails to
//! build**. Unicode 6.3 added `LRI`, `RLI`, `FSI` and `PDI`, and an
//! implementation that mapped an unknown class onto a default would have
//! resolved every isolate as `ON` — a paragraph that reorders plausibly and
//! wrongly, which is the shape of failure this whole crate is organised
//! around.

/// UAX #9's `Bidi_Class`, at Unicode 17.0's twenty-three values.
///
/// The names are the specification's abbreviations rather than spelled-out
/// ones, because every rule in [`crate::bidi`] is written in them and a
/// translation layer between the rule text and the code is a place for a
/// transcription error to hide.
#[allow(clippy::upper_case_acronyms)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BidiClass {
    /// Arabic letter: strong right-to-left, and the one that makes a following
    /// European number behave as an Arabic one (W2).
    AL,
    /// Arabic number.
    AN,
    /// Paragraph separator.
    B,
    /// Boundary neutral: removed by X9 and, in this implementation, kept in
    /// place and ignored. See [`crate::bidi`] for why.
    BN,
    /// Common number separator.
    CS,
    /// European number.
    EN,
    /// European number separator.
    ES,
    /// European number terminator.
    ET,
    /// First strong isolate.
    FSI,
    /// Left-to-right: the strong class of Latin, Greek, Cyrillic and most of
    /// the world's scripts.
    L,
    /// Left-to-right embedding.
    LRE,
    /// Left-to-right isolate.
    LRI,
    /// Left-to-right override.
    LRO,
    /// Non-spacing mark, which W1 resolves to whatever it follows.
    NSM,
    /// Other neutral.
    ON,
    /// Pop directional format.
    PDF,
    /// Pop directional isolate.
    PDI,
    /// Right-to-left: the strong class of Hebrew.
    R,
    /// Right-to-left embedding.
    RLE,
    /// Right-to-left isolate.
    RLI,
    /// Right-to-left override.
    RLO,
    /// Segment separator.
    S,
    /// Whitespace.
    WS,
}

impl BidiClass {
    /// Whether this class is one of the five X9 removes: the three embeddings
    /// and overrides that are not isolates, `PDF`, and `BN`.
    #[must_use]
    pub const fn is_removed_by_x9(self) -> bool {
        matches!(
            self,
            BidiClass::RLE | BidiClass::LRE | BidiClass::RLO | BidiClass::LRO | BidiClass::PDF
        ) || matches!(self, BidiClass::BN)
    }

    /// Whether this class is one of the three isolate initiators.
    #[must_use]
    pub const fn is_isolate_initiator(self) -> bool {
        matches!(self, BidiClass::LRI | BidiClass::RLI | BidiClass::FSI)
    }
}

/// Which end of a bracket pair a character is (`Bidi_Paired_Bracket_Type`).
///
/// The property's third value, `None`, is the absence of a row rather than a
/// variant: a character with no `Bidi_Paired_Bracket` is not in the table at
/// all, so [`bracket`] returns `None` for it and nothing has to distinguish
/// "not a bracket" from "a bracket of type none".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BracketKind {
    /// Opens a pair.
    Open,
    /// Closes one.
    Close,
}

/// UAX #24's `Script`, at Unicode 17.0's 175 values.
///
/// `Katakana_Or_Hiragana` is deliberately absent: it is a union value that
/// exists for `Script_Extensions` and that no character carries, so a variant
/// for it could never be produced. `build.rs` drops it by name rather than by
/// accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Script {
    /// `Adlam`, ISO 15924 `Adlm`.
    Adlam,
    /// `Ahom`, ISO 15924 `Ahom`.
    Ahom,
    /// `Anatolian_Hieroglyphs`, ISO 15924 `Hluw`.
    AnatolianHieroglyphs,
    /// `Arabic`, ISO 15924 `Arab`.
    Arabic,
    /// `Armenian`, ISO 15924 `Armn`.
    Armenian,
    /// `Avestan`, ISO 15924 `Avst`.
    Avestan,
    /// `Balinese`, ISO 15924 `Bali`.
    Balinese,
    /// `Bamum`, ISO 15924 `Bamu`.
    Bamum,
    /// `Bassa_Vah`, ISO 15924 `Bass`.
    BassaVah,
    /// `Batak`, ISO 15924 `Batk`.
    Batak,
    /// `Bengali`, ISO 15924 `Beng`.
    Bengali,
    /// `Beria_Erfe`, ISO 15924 `Berf`.
    BeriaErfe,
    /// `Bhaiksuki`, ISO 15924 `Bhks`.
    Bhaiksuki,
    /// `Bopomofo`, ISO 15924 `Bopo`.
    Bopomofo,
    /// `Brahmi`, ISO 15924 `Brah`.
    Brahmi,
    /// `Braille`, ISO 15924 `Brai`.
    Braille,
    /// `Buginese`, ISO 15924 `Bugi`.
    Buginese,
    /// `Buhid`, ISO 15924 `Buhd`.
    Buhid,
    /// `Canadian_Aboriginal`, ISO 15924 `Cans`.
    CanadianAboriginal,
    /// `Carian`, ISO 15924 `Cari`.
    Carian,
    /// `Caucasian_Albanian`, ISO 15924 `Aghb`.
    CaucasianAlbanian,
    /// `Chakma`, ISO 15924 `Cakm`.
    Chakma,
    /// `Cham`, ISO 15924 `Cham`.
    Cham,
    /// `Cherokee`, ISO 15924 `Cher`.
    Cherokee,
    /// `Chorasmian`, ISO 15924 `Chrs`.
    Chorasmian,
    /// `Common`, ISO 15924 `Zyyy`.
    Common,
    /// `Coptic`, ISO 15924 `Copt`.
    Coptic,
    /// `Cuneiform`, ISO 15924 `Xsux`.
    Cuneiform,
    /// `Cypriot`, ISO 15924 `Cprt`.
    Cypriot,
    /// `Cypro_Minoan`, ISO 15924 `Cpmn`.
    CyproMinoan,
    /// `Cyrillic`, ISO 15924 `Cyrl`.
    Cyrillic,
    /// `Deseret`, ISO 15924 `Dsrt`.
    Deseret,
    /// `Devanagari`, ISO 15924 `Deva`.
    Devanagari,
    /// `Dives_Akuru`, ISO 15924 `Diak`.
    DivesAkuru,
    /// `Dogra`, ISO 15924 `Dogr`.
    Dogra,
    /// `Duployan`, ISO 15924 `Dupl`.
    Duployan,
    /// `Egyptian_Hieroglyphs`, ISO 15924 `Egyp`.
    EgyptianHieroglyphs,
    /// `Elbasan`, ISO 15924 `Elba`.
    Elbasan,
    /// `Elymaic`, ISO 15924 `Elym`.
    Elymaic,
    /// `Ethiopic`, ISO 15924 `Ethi`.
    Ethiopic,
    /// `Garay`, ISO 15924 `Gara`.
    Garay,
    /// `Georgian`, ISO 15924 `Geor`.
    Georgian,
    /// `Glagolitic`, ISO 15924 `Glag`.
    Glagolitic,
    /// `Gothic`, ISO 15924 `Goth`.
    Gothic,
    /// `Grantha`, ISO 15924 `Gran`.
    Grantha,
    /// `Greek`, ISO 15924 `Grek`.
    Greek,
    /// `Gujarati`, ISO 15924 `Gujr`.
    Gujarati,
    /// `Gunjala_Gondi`, ISO 15924 `Gong`.
    GunjalaGondi,
    /// `Gurmukhi`, ISO 15924 `Guru`.
    Gurmukhi,
    /// `Gurung_Khema`, ISO 15924 `Gukh`.
    GurungKhema,
    /// `Han`, ISO 15924 `Hani`.
    Han,
    /// `Hangul`, ISO 15924 `Hang`.
    Hangul,
    /// `Hanifi_Rohingya`, ISO 15924 `Rohg`.
    HanifiRohingya,
    /// `Hanunoo`, ISO 15924 `Hano`.
    Hanunoo,
    /// `Hatran`, ISO 15924 `Hatr`.
    Hatran,
    /// `Hebrew`, ISO 15924 `Hebr`.
    Hebrew,
    /// `Hiragana`, ISO 15924 `Hira`.
    Hiragana,
    /// `Imperial_Aramaic`, ISO 15924 `Armi`.
    ImperialAramaic,
    /// `Inherited`, ISO 15924 `Zinh`.
    Inherited,
    /// `Inscriptional_Pahlavi`, ISO 15924 `Phli`.
    InscriptionalPahlavi,
    /// `Inscriptional_Parthian`, ISO 15924 `Prti`.
    InscriptionalParthian,
    /// `Javanese`, ISO 15924 `Java`.
    Javanese,
    /// `Kaithi`, ISO 15924 `Kthi`.
    Kaithi,
    /// `Kannada`, ISO 15924 `Knda`.
    Kannada,
    /// `Katakana`, ISO 15924 `Kana`.
    Katakana,
    /// `Kawi`, ISO 15924 `Kawi`.
    Kawi,
    /// `Kayah_Li`, ISO 15924 `Kali`.
    KayahLi,
    /// `Kharoshthi`, ISO 15924 `Khar`.
    Kharoshthi,
    /// `Khitan_Small_Script`, ISO 15924 `Kits`.
    KhitanSmallScript,
    /// `Khmer`, ISO 15924 `Khmr`.
    Khmer,
    /// `Khojki`, ISO 15924 `Khoj`.
    Khojki,
    /// `Khudawadi`, ISO 15924 `Sind`.
    Khudawadi,
    /// `Kirat_Rai`, ISO 15924 `Krai`.
    KiratRai,
    /// `Lao`, ISO 15924 `Laoo`.
    Lao,
    /// `Latin`, ISO 15924 `Latn`.
    Latin,
    /// `Lepcha`, ISO 15924 `Lepc`.
    Lepcha,
    /// `Limbu`, ISO 15924 `Limb`.
    Limbu,
    /// `Linear_A`, ISO 15924 `Lina`.
    LinearA,
    /// `Linear_B`, ISO 15924 `Linb`.
    LinearB,
    /// `Lisu`, ISO 15924 `Lisu`.
    Lisu,
    /// `Lycian`, ISO 15924 `Lyci`.
    Lycian,
    /// `Lydian`, ISO 15924 `Lydi`.
    Lydian,
    /// `Mahajani`, ISO 15924 `Mahj`.
    Mahajani,
    /// `Makasar`, ISO 15924 `Maka`.
    Makasar,
    /// `Malayalam`, ISO 15924 `Mlym`.
    Malayalam,
    /// `Mandaic`, ISO 15924 `Mand`.
    Mandaic,
    /// `Manichaean`, ISO 15924 `Mani`.
    Manichaean,
    /// `Marchen`, ISO 15924 `Marc`.
    Marchen,
    /// `Masaram_Gondi`, ISO 15924 `Gonm`.
    MasaramGondi,
    /// `Medefaidrin`, ISO 15924 `Medf`.
    Medefaidrin,
    /// `Meetei_Mayek`, ISO 15924 `Mtei`.
    MeeteiMayek,
    /// `Mende_Kikakui`, ISO 15924 `Mend`.
    MendeKikakui,
    /// `Meroitic_Cursive`, ISO 15924 `Merc`.
    MeroiticCursive,
    /// `Meroitic_Hieroglyphs`, ISO 15924 `Mero`.
    MeroiticHieroglyphs,
    /// `Miao`, ISO 15924 `Plrd`.
    Miao,
    /// `Modi`, ISO 15924 `Modi`.
    Modi,
    /// `Mongolian`, ISO 15924 `Mong`.
    Mongolian,
    /// `Mro`, ISO 15924 `Mroo`.
    Mro,
    /// `Multani`, ISO 15924 `Mult`.
    Multani,
    /// `Myanmar`, ISO 15924 `Mymr`.
    Myanmar,
    /// `Nabataean`, ISO 15924 `Nbat`.
    Nabataean,
    /// `Nag_Mundari`, ISO 15924 `Nagm`.
    NagMundari,
    /// `Nandinagari`, ISO 15924 `Nand`.
    Nandinagari,
    /// `New_Tai_Lue`, ISO 15924 `Talu`.
    NewTaiLue,
    /// `Newa`, ISO 15924 `Newa`.
    Newa,
    /// `Nko`, ISO 15924 `Nkoo`.
    Nko,
    /// `Nushu`, ISO 15924 `Nshu`.
    Nushu,
    /// `Nyiakeng_Puachue_Hmong`, ISO 15924 `Hmnp`.
    NyiakengPuachueHmong,
    /// `Ogham`, ISO 15924 `Ogam`.
    Ogham,
    /// `Ol_Chiki`, ISO 15924 `Olck`.
    OlChiki,
    /// `Ol_Onal`, ISO 15924 `Onao`.
    OlOnal,
    /// `Old_Hungarian`, ISO 15924 `Hung`.
    OldHungarian,
    /// `Old_Italic`, ISO 15924 `Ital`.
    OldItalic,
    /// `Old_North_Arabian`, ISO 15924 `Narb`.
    OldNorthArabian,
    /// `Old_Permic`, ISO 15924 `Perm`.
    OldPermic,
    /// `Old_Persian`, ISO 15924 `Xpeo`.
    OldPersian,
    /// `Old_Sogdian`, ISO 15924 `Sogo`.
    OldSogdian,
    /// `Old_South_Arabian`, ISO 15924 `Sarb`.
    OldSouthArabian,
    /// `Old_Turkic`, ISO 15924 `Orkh`.
    OldTurkic,
    /// `Old_Uyghur`, ISO 15924 `Ougr`.
    OldUyghur,
    /// `Oriya`, ISO 15924 `Orya`.
    Oriya,
    /// `Osage`, ISO 15924 `Osge`.
    Osage,
    /// `Osmanya`, ISO 15924 `Osma`.
    Osmanya,
    /// `Pahawh_Hmong`, ISO 15924 `Hmng`.
    PahawhHmong,
    /// `Palmyrene`, ISO 15924 `Palm`.
    Palmyrene,
    /// `Pau_Cin_Hau`, ISO 15924 `Pauc`.
    PauCinHau,
    /// `Phags_Pa`, ISO 15924 `Phag`.
    PhagsPa,
    /// `Phoenician`, ISO 15924 `Phnx`.
    Phoenician,
    /// `Psalter_Pahlavi`, ISO 15924 `Phlp`.
    PsalterPahlavi,
    /// `Rejang`, ISO 15924 `Rjng`.
    Rejang,
    /// `Runic`, ISO 15924 `Runr`.
    Runic,
    /// `Samaritan`, ISO 15924 `Samr`.
    Samaritan,
    /// `Saurashtra`, ISO 15924 `Saur`.
    Saurashtra,
    /// `Sharada`, ISO 15924 `Shrd`.
    Sharada,
    /// `Shavian`, ISO 15924 `Shaw`.
    Shavian,
    /// `Siddham`, ISO 15924 `Sidd`.
    Siddham,
    /// `Sidetic`, ISO 15924 `Sidt`.
    Sidetic,
    /// `SignWriting`, ISO 15924 `Sgnw`.
    SignWriting,
    /// `Sinhala`, ISO 15924 `Sinh`.
    Sinhala,
    /// `Sogdian`, ISO 15924 `Sogd`.
    Sogdian,
    /// `Sora_Sompeng`, ISO 15924 `Sora`.
    SoraSompeng,
    /// `Soyombo`, ISO 15924 `Soyo`.
    Soyombo,
    /// `Sundanese`, ISO 15924 `Sund`.
    Sundanese,
    /// `Sunuwar`, ISO 15924 `Sunu`.
    Sunuwar,
    /// `Syloti_Nagri`, ISO 15924 `Sylo`.
    SylotiNagri,
    /// `Syriac`, ISO 15924 `Syrc`.
    Syriac,
    /// `Tagalog`, ISO 15924 `Tglg`.
    Tagalog,
    /// `Tagbanwa`, ISO 15924 `Tagb`.
    Tagbanwa,
    /// `Tai_Le`, ISO 15924 `Tale`.
    TaiLe,
    /// `Tai_Tham`, ISO 15924 `Lana`.
    TaiTham,
    /// `Tai_Viet`, ISO 15924 `Tavt`.
    TaiViet,
    /// `Tai_Yo`, ISO 15924 `Tayo`.
    TaiYo,
    /// `Takri`, ISO 15924 `Takr`.
    Takri,
    /// `Tamil`, ISO 15924 `Taml`.
    Tamil,
    /// `Tangsa`, ISO 15924 `Tnsa`.
    Tangsa,
    /// `Tangut`, ISO 15924 `Tang`.
    Tangut,
    /// `Telugu`, ISO 15924 `Telu`.
    Telugu,
    /// `Thaana`, ISO 15924 `Thaa`.
    Thaana,
    /// `Thai`, ISO 15924 `Thai`.
    Thai,
    /// `Tibetan`, ISO 15924 `Tibt`.
    Tibetan,
    /// `Tifinagh`, ISO 15924 `Tfng`.
    Tifinagh,
    /// `Tirhuta`, ISO 15924 `Tirh`.
    Tirhuta,
    /// `Todhri`, ISO 15924 `Todr`.
    Todhri,
    /// `Tolong_Siki`, ISO 15924 `Tols`.
    TolongSiki,
    /// `Toto`, ISO 15924 `Toto`.
    Toto,
    /// `Tulu_Tigalari`, ISO 15924 `Tutg`.
    TuluTigalari,
    /// `Ugaritic`, ISO 15924 `Ugar`.
    Ugaritic,
    /// `Unknown`, ISO 15924 `Zzzz`.
    Unknown,
    /// `Vai`, ISO 15924 `Vaii`.
    Vai,
    /// `Vithkuqi`, ISO 15924 `Vith`.
    Vithkuqi,
    /// `Wancho`, ISO 15924 `Wcho`.
    Wancho,
    /// `Warang_Citi`, ISO 15924 `Wara`.
    WarangCiti,
    /// `Yezidi`, ISO 15924 `Yezi`.
    Yezidi,
    /// `Yi`, ISO 15924 `Yiii`.
    Yi,
    /// `Zanabazar_Square`, ISO 15924 `Zanb`.
    ZanabazarSquare,
}

impl Script {
    /// The script's ISO 15924 code, from `PropertyValueAliases.txt`.
    #[must_use]
    pub const fn iso15924(self) -> [u8; 4] {
        iso15924(self)
    }

    /// The OpenType script tag a face is asked for when a run is in this
    /// script — **by the registry's default rule, which has exceptions this
    /// crate does not yet carry.**
    ///
    /// # What this derivation is
    ///
    /// The OpenType script tags are a Microsoft registry, not a Unicode
    /// property, and the registry's own default is the ISO 15924 code in lower
    /// case: `Ethi` becomes `ethi`, `Arab` becomes `arab`, `Latn` becomes
    /// `latn`. That is what this returns, straight from the vendored alias
    /// file, so no list of tags is written out here to fall out of date.
    ///
    /// # What it is not, stated rather than discovered later
    ///
    /// The registry departs from that rule in a bounded set of cases, and this
    /// crate does not have them:
    ///
    /// - the Indic **version 2** tags — `dev2`, `bng2`, `gur2`, `gjr2`,
    ///   `ory2`, `tml2`, `tel2`, `knd2`, `mlm2`, `mym2` — which a modern face
    ///   uses in preference to the v1 tag of the same script;
    /// - the scripts whose tag is shorter than four letters and space-padded,
    ///   such as N'Ko and Lao;
    /// - Hiragana and Katakana, which share one tag rather than having one
    ///   each.
    ///
    /// Every one of those belongs to a script `docs/design/shaping.md`
    /// schedules for **milestone 4 or 5**, where the text-rendering-tests
    /// sections for that script are the grade. Until then the consequence is
    /// bounded and in the safe direction: a tag the face does not declare
    /// falls back to `DFLT` in [`crate::LayoutTable::lookups_for`], so the run
    /// gets fewer features rather than the wrong ones — the same output a face
    /// with no layout tables produces.
    #[must_use]
    pub const fn opentype_tag(self) -> crate::Tag {
        let mut code = self.iso15924();
        let mut at = 0;
        while at < 4 {
            code[at] = code[at].to_ascii_lowercase();
            at += 1;
        }
        crate::Tag::new(&code)
    }
}

include!(concat!(env!("OUT_DIR"), "/ucd.rs"));

/// A code point's value in a sorted `(first, last, value)` table.
fn lookup<T: Copy>(table: &[(u32, u32, T)], code: u32, default: T) -> T {
    let mut low = 0usize;
    let mut high = table.len();
    while low < high {
        let middle = (low + high) / 2;
        let (first, last, value) = table[middle];
        if code < first {
            high = middle;
        } else if code > last {
            low = middle + 1;
        } else {
            return value;
        }
    }
    default
}

/// The second column of a sorted `(key, value, extra)` table.
fn paired<T: Copy>(table: &[(u32, u32, T)], code: u32) -> Option<(u32, T)> {
    let mut low = 0usize;
    let mut high = table.len();
    while low < high {
        let middle = (low + high) / 2;
        let (key, value, extra) = table[middle];
        if code < key {
            high = middle;
        } else if code > key {
            low = middle + 1;
        } else {
            return Some((value, extra));
        }
    }
    None
}

/// The character's `Bidi_Class`.
///
/// Total: every code point has one, because `build.rs` applied the property's
/// own `@missing` defaults before merging, so there is no "not in the table"
/// case for a caller to guess about.
#[must_use]
pub fn bidi_class(c: char) -> BidiClass {
    lookup(BIDI_CLASS, c as u32, BidiClass::L)
}

/// The character this one is paired with, and which end of the pair it is.
///
/// `None` for everything that is not a bracket, which is all but sixty-odd
/// characters.
#[must_use]
pub fn bracket(c: char) -> Option<(char, BracketKind)> {
    let (code, kind) = paired(BRACKETS, c as u32)?;
    Some((char::from_u32(code)?, kind))
}

/// The character this one becomes when the run reads right to left (rule L4).
///
/// A *character*, not a glyph: UAX #9 says the mirroring is a rendering
/// operation and that a font may hold the mirrored form itself. This is the
/// character-level answer, which is the one a leaf crate can give.
#[must_use]
pub fn mirrored(c: char) -> Option<char> {
    let mut low = 0usize;
    let mut high = MIRRORING.len();
    let code = c as u32;
    while low < high {
        let middle = (low + high) / 2;
        let (key, value) = MIRRORING[middle];
        if code < key {
            high = middle;
        } else if code > key {
            low = middle + 1;
        } else {
            return char::from_u32(value);
        }
    }
    None
}

/// The character's UAX #24 `Script`.
///
/// [`Script::Unknown`] where `Scripts.txt` lists nothing, which is the value
/// the property itself gives an unassigned code point.
#[must_use]
pub fn script(c: char) -> Script {
    lookup(SCRIPT, c as u32, Script::Unknown)
}

#[cfg(test)]
mod tests {
    use super::{bidi_class, bracket, mirrored, script, BidiClass, BracketKind, Script};

    #[test]
    fn the_classes_of_the_characters_uax9s_own_examples_use() {
        assert_eq!(bidi_class('a'), BidiClass::L);
        // HEBREW LETTER ALEF.
        assert_eq!(bidi_class('\u{05D0}'), BidiClass::R);
        // ARABIC LETTER ALEF.
        assert_eq!(bidi_class('\u{0627}'), BidiClass::AL);
        assert_eq!(bidi_class('1'), BidiClass::EN);
        // ARABIC-INDIC DIGIT ONE.
        assert_eq!(bidi_class('\u{0661}'), BidiClass::AN);
        assert_eq!(bidi_class(' '), BidiClass::WS);
        assert_eq!(bidi_class('('), BidiClass::ON);
        assert_eq!(bidi_class('\u{2066}'), BidiClass::LRI);
        assert_eq!(bidi_class('\u{2069}'), BidiClass::PDI);
    }

    /// The `@missing` block defaults are applied, and this is the case that
    /// says so: U+05EB is unassigned, sits in the Hebrew block, and is `R`
    /// rather than the file-wide `L`.
    #[test]
    fn an_unassigned_code_point_in_a_right_to_left_block_is_not_left_to_right() {
        assert_eq!(bidi_class('\u{05EB}'), BidiClass::R);
        // And one in an Arabic block is AL rather than R.
        assert_eq!(bidi_class('\u{08B5}'), BidiClass::AL);
        // While an unassigned code point outside any such block stays L.
        assert_eq!(bidi_class('\u{0378}'), BidiClass::L);
    }

    #[test]
    fn brackets_pair_both_ways() {
        assert_eq!(bracket('('), Some((')', BracketKind::Open)));
        assert_eq!(bracket(')'), Some(('(', BracketKind::Close)));
        assert_eq!(bracket('a'), None);
    }

    #[test]
    fn mirroring_is_a_character_map() {
        assert_eq!(mirrored('('), Some(')'));
        assert_eq!(mirrored('<'), Some('>'));
        assert_eq!(mirrored('a'), None);
    }

    #[test]
    fn scripts_and_their_tags() {
        assert_eq!(script('a'), Script::Latin);
        assert_eq!(script('\u{1208}'), Script::Ethiopic);
        assert_eq!(script('\u{0627}'), Script::Arabic);
        assert_eq!(Script::Ethiopic.iso15924(), *b"Ethi");
        assert_eq!(Script::Ethiopic.opentype_tag(), crate::Tag::new(b"ethi"));
        assert_eq!(Script::Latin.opentype_tag(), crate::Tag::new(b"latn"));
        // The derivation this crate does not yet have, asserted as the thing
        // it currently does so that milestone 4 has to change a test rather
        // than discover a surprise: N'Ko's registry tag is `nko `.
        assert_eq!(Script::Nko.opentype_tag(), crate::Tag::new(b"nkoo"));
    }

    /// A code point Unicode has not assigned to a script is `Unknown`, and a
    /// caller can tell that from a script it does know.
    #[test]
    fn an_unlisted_code_point_is_unknown() {
        assert_eq!(script('\u{E0080}'), Script::Unknown);
    }
}
