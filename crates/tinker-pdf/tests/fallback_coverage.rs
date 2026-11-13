//! **The census FT-02's faces are priced with** (`docs/design/fallback-faces.md`,
//! "Every script, emoji, mathematics, music and symbols", FT-02.2): for each
//! candidate face the design names — one a script, the class groups' faces,
//! and FT-01's four region faces, whose Han counts order step 2 of the lookup
//! — how many characters of each `Script` value and of each emoji,
//! mathematics, music and symbol class its `cmap` covers; its bytes as
//! published and as WOFF2, and whether the WOFF2 form unpacks to the face;
//! and each feature group's bytes together, which a native build carries.
//!
//! The census rules nothing but the mathematics face, by the rule the design
//! states: the faces are H-116's and the budget H-04's. It counts by the
//! functions the lookup will read through — `tinker_pdf_shape::unicode::script`
//! for a character's script, compiled from the vendored `Scripts.txt`, and
//! `Sfnt::glyph_for_char` for a face's cover — and reads the classes from the
//! layout crate's vendored UCD files, as the design names them.
//!
//! # Fetched, never committed
//!
//! The candidates are fetched once into a directory of the operator's, from
//! the upstreams the design names, and `TINKER_FALLBACK_FACES` names it;
//! nothing of them reaches the tree until FT-02.4 vendors a face (ruling 18).
//! A candidate's WOFF2 form, made by the reference encoder outside the
//! engine, is read from the same directory under its stem with `.woff2`.
//! Without the variable, or with one naming no directory, the census prints
//! [`SKIPPED`]; with it, [`RAN`] and its four tables, a candidate the
//! directory lacks marked absent.
//!
//! ```text
//! TINKER_FALLBACK_FACES=<directory> cargo test -p tinker-pdf --release --test fallback_coverage -- --ignored --nocapture
//! ```
//!
//! Its rows are committed as `fallback_coverage.tsv` beside this file, which
//! [`the_committed_census_is_whole`] holds whole and adding up (ruling 16);
//! each group's gzipped wasm, which a wasm build measures and the census
//! cannot, is `fallback_sizes.tsv`'s, which
//! [`the_committed_wasm_sizes_are_whole`] holds; and
//! [`the_designs_outcomes_are_the_records`] holds the outcomes the design
//! states to the counts. Each run's revision, machine and day go in the
//! message of the commit that records it.
//!
//! The tests below the census hold its instruments — the classes read from
//! the vendored files, a face counted by its `cmap`, the mathematics rule,
//! the candidate list against the design's tables — and the committed
//! records, and run in every `cargo test`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf_font::{woff, Sfnt};
use tinker_pdf_shape::unicode::{script, Script};

/// Printed when the census ran: a skipped census exits 0 and reads like a
/// pass, so a run without this line is no measurement.
const RAN: &str = "fallback-coverage: RAN";

/// Printed when there was nothing to measure.
const SKIPPED: &str = "fallback-coverage: SKIPPED";

/// The head of the census's `value` table: each `Script` value and class,
/// and how many characters it holds.
const VALUE_HEAD: &str = "value\tname\tcharacters";

/// The head of the census's `coverage` table: a row for each face and each
/// value or class its `cmap` covers any character of.
const COVERAGE_HEAD: &str = "coverage\tfile\tface\tgroup\tvalue\tcovered";

/// The head of the census's `storage` table: a row for each face.
const STORAGE_HEAD: &str =
    "storage\tfile\tface\tgroup\tbytes\twoff2_bytes\twoff2_unpacks\tupstream\tspdx";

/// The head of the census's `group` table: a row for each feature group.
const GROUP_HEAD: &str = "group\tname\tfaces\tbytes\twoff2_bytes";

/// The head of `fallback_sizes.tsv`'s one table: a row for the default
/// profile and for each of FT-02's groups.
const WASM_HEAD: &str = "wasm\tprofile\tgzip_bytes";

/// The most bytes a WOFF2 form is unpacked to.
const MAX_FACE: usize = 64 << 20;

/// A feature group of the design's, or FT-01's faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    Scripts,
    Limited,
    Historic,
    Emoji,
    Math,
    Music,
    Symbols,
    /// FT-01's region faces, counted for step 2's order and carried by
    /// `cjk-fonts`.
    Cjk,
}

impl Group {
    /// Every group, in the order the census prints them.
    const ALL: [Group; 8] = [
        Group::Scripts,
        Group::Limited,
        Group::Historic,
        Group::Emoji,
        Group::Math,
        Group::Music,
        Group::Symbols,
        Group::Cjk,
    ];

    /// The feature that carries it.
    fn name(self) -> &'static str {
        match self {
            Group::Scripts => "fallback-scripts",
            Group::Limited => "fallback-scripts-limited",
            Group::Historic => "fallback-scripts-historic",
            Group::Emoji => "fallback-emoji",
            Group::Math => "fallback-math",
            Group::Music => "fallback-music",
            Group::Symbols => "fallback-symbols",
            Group::Cjk => "cjk-fonts",
        }
    }
}

/// notofonts.github.io at the commit the design names.
const NOTO: &str = "https://github.com/notofonts/notofonts.github.io at 3460ae9";

/// google/fonts at the commit the design names.
const GOOGLE: &str = "https://github.com/google/fonts at 29b4ec6";

/// Source Han Sans's release, as FT-01 vendors it.
const SOURCE_HAN: &str = "https://github.com/adobe-fonts/source-han-sans release 2.005R";

/// The Noto families the design's three script tables and its class table
/// name, each published as `fonts/<Family>/unhinted/ttf/<Family>-Regular.ttf`,
/// and the group each is in, in the design's order.
const NOTO_FAMILIES: &[(&str, Group)] = &[
    ("NotoSansArabic", Group::Scripts),
    ("NotoSansArmenian", Group::Scripts),
    ("NotoSansBengali", Group::Scripts),
    ("NotoSans", Group::Scripts),
    ("NotoSansDevanagari", Group::Scripts),
    ("NotoSansEthiopic", Group::Scripts),
    ("NotoSansGeorgian", Group::Scripts),
    ("NotoSansGujarati", Group::Scripts),
    ("NotoSansGurmukhi", Group::Scripts),
    ("NotoSansHebrew", Group::Scripts),
    ("NotoSansKannada", Group::Scripts),
    ("NotoSansKhmer", Group::Scripts),
    ("NotoSansLao", Group::Scripts),
    ("NotoSansMalayalam", Group::Scripts),
    ("NotoSansMyanmar", Group::Scripts),
    ("NotoSansOriya", Group::Scripts),
    ("NotoSansSinhala", Group::Scripts),
    ("NotoSansTamil", Group::Scripts),
    ("NotoSansTelugu", Group::Scripts),
    ("NotoSansThaana", Group::Scripts),
    ("NotoSansThai", Group::Scripts),
    ("NotoSerifTibetan", Group::Scripts),
    ("NotoSansAdlam", Group::Limited),
    ("NotoSansBalinese", Group::Limited),
    ("NotoSansBamum", Group::Limited),
    ("NotoSansBatak", Group::Limited),
    ("NotoSansChakma", Group::Limited),
    ("NotoSansCanadianAboriginal", Group::Limited),
    ("NotoSansCham", Group::Limited),
    ("NotoSansCherokee", Group::Limited),
    ("NotoSerifNPHmong", Group::Limited),
    ("NotoSansJavanese", Group::Limited),
    ("NotoSansKayahLi", Group::Limited),
    ("NotoSansTaiTham", Group::Limited),
    ("NotoSansLepcha", Group::Limited),
    ("NotoSansLimbu", Group::Limited),
    ("NotoSansLisu", Group::Limited),
    ("NotoSansMandaic", Group::Limited),
    ("NotoSansMeeteiMayek", Group::Limited),
    ("NotoSansNewa", Group::Limited),
    ("NotoSansNKo", Group::Limited),
    ("NotoSansOlChiki", Group::Limited),
    ("NotoSansOsage", Group::Limited),
    ("NotoSansMiao", Group::Limited),
    ("NotoSansHanifiRohingya", Group::Limited),
    ("NotoSansSaurashtra", Group::Limited),
    ("NotoSansSundanese", Group::Limited),
    ("NotoSansSylotiNagri", Group::Limited),
    ("NotoSansSyriac", Group::Limited),
    ("NotoSansTaiLe", Group::Limited),
    ("NotoSansNewTaiLue", Group::Limited),
    ("NotoSansTaiViet", Group::Limited),
    ("NotoSansTifinagh", Group::Limited),
    ("NotoSansVai", Group::Limited),
    ("NotoSansWancho", Group::Limited),
    ("NotoSansYi", Group::Limited),
    ("NotoSansCaucasianAlbanian", Group::Historic),
    ("NotoSerifAhom", Group::Historic),
    ("NotoSansImperialAramaic", Group::Historic),
    ("NotoSansAvestan", Group::Historic),
    ("NotoSansBassaVah", Group::Historic),
    ("NotoSansBhaiksuki", Group::Historic),
    ("NotoSansBrahmi", Group::Historic),
    ("NotoSansBuginese", Group::Historic),
    ("NotoSansBuhid", Group::Historic),
    ("NotoSansCarian", Group::Historic),
    ("NotoSansChorasmian", Group::Historic),
    ("NotoSansCoptic", Group::Historic),
    ("NotoSansCyproMinoan", Group::Historic),
    ("NotoSansCypriot", Group::Historic),
    ("NotoSerifDivesAkuru", Group::Historic),
    ("NotoSerifDogra", Group::Historic),
    ("NotoSansDeseret", Group::Historic),
    ("NotoSansDuployan", Group::Historic),
    ("NotoSansEgyptianHieroglyphs", Group::Historic),
    ("NotoSansElbasan", Group::Historic),
    ("NotoSansElymaic", Group::Historic),
    ("NotoSansGlagolitic", Group::Historic),
    ("NotoSansGunjalaGondi", Group::Historic),
    ("NotoSansMasaramGondi", Group::Historic),
    ("NotoSansGothic", Group::Historic),
    ("NotoSansGrantha", Group::Historic),
    ("NotoSansHanunoo", Group::Historic),
    ("NotoSansHatran", Group::Historic),
    ("NotoSansAnatolianHieroglyphs", Group::Historic),
    ("NotoSansPahawhHmong", Group::Historic),
    ("NotoSansOldHungarian", Group::Historic),
    ("NotoSansOldItalic", Group::Historic),
    ("NotoSansKawi", Group::Historic),
    ("NotoSansKharoshthi", Group::Historic),
    ("NotoSansKhojki", Group::Historic),
    ("NotoSerifKhitanSmallScript", Group::Historic),
    ("NotoSansKaithi", Group::Historic),
    ("NotoSansLinearA", Group::Historic),
    ("NotoSansLinearB", Group::Historic),
    ("NotoSansLycian", Group::Historic),
    ("NotoSansLydian", Group::Historic),
    ("NotoSerifMakasar", Group::Historic),
    ("NotoSansMahajani", Group::Historic),
    ("NotoSansManichaean", Group::Historic),
    ("NotoSansMarchen", Group::Historic),
    ("NotoSansMedefaidrin", Group::Historic),
    ("NotoSansMendeKikakui", Group::Historic),
    ("NotoSansMeroitic", Group::Historic),
    ("NotoSansModi", Group::Historic),
    ("NotoSansMongolian", Group::Historic),
    ("NotoSansMro", Group::Historic),
    ("NotoSansMultani", Group::Historic),
    ("NotoSansNagMundari", Group::Historic),
    ("NotoSansOldNorthArabian", Group::Historic),
    ("NotoSansNandinagari", Group::Historic),
    ("NotoSansNabataean", Group::Historic),
    ("NotoSansNushu", Group::Historic),
    ("NotoSansOgham", Group::Historic),
    ("NotoSansOldTurkic", Group::Historic),
    ("NotoSansOsmanya", Group::Historic),
    ("NotoSerifOldUyghur", Group::Historic),
    ("NotoSansPalmyrene", Group::Historic),
    ("NotoSansPauCinHau", Group::Historic),
    ("NotoSansOldPermic", Group::Historic),
    ("NotoSansPhagsPa", Group::Historic),
    ("NotoSansInscriptionalPahlavi", Group::Historic),
    ("NotoSansPsalterPahlavi", Group::Historic),
    ("NotoSansPhoenician", Group::Historic),
    ("NotoSansInscriptionalParthian", Group::Historic),
    ("NotoSansRejang", Group::Historic),
    ("NotoSansRunic", Group::Historic),
    ("NotoSansSamaritan", Group::Historic),
    ("NotoSansOldSouthArabian", Group::Historic),
    ("NotoSansSignWriting", Group::Historic),
    ("NotoSansShavian", Group::Historic),
    ("NotoSansSharada", Group::Historic),
    ("NotoSansSiddham", Group::Historic),
    ("NotoSansKhudawadi", Group::Historic),
    ("NotoSansSoraSompeng", Group::Historic),
    ("NotoSansSogdian", Group::Historic),
    ("NotoSansOldSogdian", Group::Historic),
    ("NotoSansSoyombo", Group::Historic),
    ("NotoSansSunuwar", Group::Historic),
    ("NotoSansTagbanwa", Group::Historic),
    ("NotoSansTakri", Group::Historic),
    ("NotoSerifTangut", Group::Historic),
    ("NotoSansTagalog", Group::Historic),
    ("NotoSansTirhuta", Group::Historic),
    ("NotoSansTangsa", Group::Historic),
    ("NotoSerifTodhri", Group::Historic),
    ("NotoSerifToto", Group::Historic),
    ("NotoSansUgaritic", Group::Historic),
    ("NotoSansVithkuqi", Group::Historic),
    ("NotoSansWarangCiti", Group::Historic),
    ("NotoSansOldPersian", Group::Historic),
    ("NotoSansCuneiform", Group::Historic),
    ("NotoSerifYezidi", Group::Historic),
    ("NotoSansZanabazarSquare", Group::Historic),
    ("NotoSansMath", Group::Math),
    ("NotoMusic", Group::Music),
    ("NotoZnamennyMusicalNotation", Group::Music),
    ("NotoSansSymbols", Group::Symbols),
    ("NotoSansSymbols2", Group::Symbols),
    ("NotoSansMayanNumerals", Group::Symbols),
    ("NotoSansIndicSiyaqNumbers", Group::Symbols),
    ("NotoSerifOttomanSiyaq", Group::Symbols),
];

/// One candidate face.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    /// The file the census reads from `TINKER_FALLBACK_FACES`.
    file: String,
    /// The face, as the design's tables name it.
    face: String,
    /// Its group.
    group: Group,
    /// Where it is fetched from.
    upstream: String,
    /// Its licence's SPDX expression.
    spdx: &'static str,
}

/// A family's name as the design spells it: its words apart, a number its
/// own word, and the two names whose capitals run together as they are
/// written.
fn spelled(family: &str) -> String {
    match family {
        "NotoSerifNPHmong" => return "Noto Serif NP Hmong".to_string(),
        "NotoSansNKo" => return "Noto Sans NKo".to_string(),
        "STIXTwoMath" => return "STIX Two Math".to_string(),
        _ => {}
    }
    let mut out = String::new();
    let mut previous: Option<char> = None;
    for c in family.chars() {
        if let Some(before) = previous {
            if (c.is_ascii_uppercase() && before.is_ascii_lowercase())
                || (c.is_ascii_digit() && before.is_ascii_alphabetic())
            {
                out.push(' ');
            }
        }
        out.push(c);
        previous = Some(c);
    }
    out
}

/// Every candidate, in the census's order: the Noto families, then the
/// emoji face and the second mathematics face from google/fonts, then FT-01's
/// four region faces.
fn candidates() -> Vec<Candidate> {
    let mut out: Vec<Candidate> = NOTO_FAMILIES
        .iter()
        .map(|(family, group)| Candidate {
            file: format!("{family}-Regular.ttf"),
            face: spelled(family),
            group: *group,
            upstream: format!("{NOTO} fonts/{family}/unhinted/ttf/"),
            spdx: "OFL-1.1",
        })
        .collect();
    out.push(Candidate {
        file: "NotoEmoji[wght].ttf".to_string(),
        face: "Noto Emoji".to_string(),
        group: Group::Emoji,
        upstream: format!("{GOOGLE} ofl/notoemoji/"),
        spdx: "OFL-1.1",
    });
    out.push(Candidate {
        file: "STIXTwoMath-Regular.ttf".to_string(),
        face: "STIX Two Math".to_string(),
        group: Group::Math,
        upstream: format!("{GOOGLE} ofl/stixtwomath/"),
        spdx: "OFL-1.1",
    });
    for region in ["JP", "CN", "TW", "KR"] {
        out.push(Candidate {
            file: format!("SourceHanSans{region}-Regular.otf"),
            face: format!("Source Han Sans {region}"),
            group: Group::Cjk,
            upstream: format!("{SOURCE_HAN} SubsetOTF/{region}/"),
            spdx: "OFL-1.1",
        });
    }
    out
}

// --- the classes ------------------------------------------------------------

/// The classes the census counts beside the scripts, in its order.
const CLASSES: [Class; 5] = [
    Class::Emoji,
    Class::Pictographic,
    Class::Math,
    Class::Music,
    Class::Symbols,
];

/// A class of the design's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// `Emoji_Presentation`.
    Emoji,
    /// `Extended_Pictographic`, assigned.
    Pictographic,
    /// `Math`.
    Math,
    /// The four musical blocks, by their characters' names.
    Music,
    /// `So`, `Sm`, and `No` of the `Common` script.
    Symbols,
}

impl Class {
    fn name(self) -> &'static str {
        match self {
            Class::Emoji => "class:emoji",
            Class::Pictographic => "class:pictographic",
            Class::Math => "class:math",
            Class::Music => "class:music",
            Class::Symbols => "class:symbols",
        }
    }

    fn bit(self) -> u8 {
        match self {
            Class::Emoji => 1,
            Class::Pictographic => 2,
            Class::Math => 4,
            Class::Music => 8,
            Class::Symbols => 16,
        }
    }
}

/// The names `UnicodeData.txt` begins the four musical blocks' characters
/// with: Musical Symbols, Byzantine Musical Symbols, Ancient Greek Musical
/// Notation and Znamenny Musical Notation.
const MUSIC_NAMES: [&str; 5] = [
    "MUSICAL SYMBOL ",
    "BYZANTINE MUSICAL SYMBOL ",
    "GREEK VOCAL NOTATION SYMBOL",
    "GREEK INSTRUMENTAL NOTATION SYMBOL",
    "ZNAMENNY ",
];

/// One of the layout crate's vendored UCD files.
fn ucd(file: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tinker-pdf-layout/data/ucd")
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The code points a UCD property file gives `value`, as `(first, last)`
/// ranges: each line `XXXX` or `XXXX..YYYY`, `;`, the value, the rest a
/// comment.
fn ranges_of(text: &str, value: &str) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let data = line.split('#').next().unwrap_or("");
        let mut fields = data.split(';');
        let (Some(points), Some(found)) = (fields.next(), fields.next()) else {
            continue;
        };
        if found.trim() != value {
            continue;
        }
        let points = points.trim();
        let (first, last) = points.split_once("..").unwrap_or((points, points));
        if let (Ok(first), Ok(last)) = (
            u32::from_str_radix(first.trim(), 16),
            u32::from_str_radix(last.trim(), 16),
        ) {
            out.push((first, last));
        }
    }
    out
}

/// Each code point's classes, a bit a class, indexed by code point: read from
/// `emoji-data.txt`, `DerivedCoreProperties.txt`, `DerivedGeneralCategory.txt`
/// and `UnicodeData.txt`, and held to assigned characters — those
/// `Scripts.txt` gives a script — since `Extended_Pictographic` reserves
/// unassigned ones.
fn classes() -> Vec<u8> {
    let mut bits = vec![0u8; 0x11_0000];
    let mut set = |ranges: &[(u32, u32)], class: Class| {
        for (first, last) in ranges {
            for point in *first..=*last {
                if let Some(slot) = bits.get_mut(point as usize) {
                    *slot |= class.bit();
                }
            }
        }
    };
    let emoji = ucd("emoji-data.txt");
    set(&ranges_of(&emoji, "Emoji_Presentation"), Class::Emoji);
    set(
        &ranges_of(&emoji, "Extended_Pictographic"),
        Class::Pictographic,
    );
    set(
        &ranges_of(&ucd("DerivedCoreProperties.txt"), "Math"),
        Class::Math,
    );
    let categories = ucd("DerivedGeneralCategory.txt");
    set(&ranges_of(&categories, "So"), Class::Symbols);
    set(&ranges_of(&categories, "Sm"), Class::Symbols);
    let common_numbers: Vec<(u32, u32)> = ranges_of(&categories, "No")
        .into_iter()
        .flat_map(|(first, last)| first..=last)
        .filter(|point| char::from_u32(*point).is_some_and(|c| script(c) == Script::Common))
        .map(|point| (point, point))
        .collect();
    set(&common_numbers, Class::Symbols);
    let music: Vec<(u32, u32)> = ucd("UnicodeData.txt")
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(';');
            let point = u32::from_str_radix(fields.next()?, 16).ok()?;
            let name = fields.next()?;
            MUSIC_NAMES
                .iter()
                .any(|prefix| name.starts_with(prefix))
                .then_some((point, point))
        })
        .collect();
    set(&music, Class::Music);
    for (point, slot) in bits.iter_mut().enumerate() {
        let assigned = u32::try_from(point)
            .ok()
            .and_then(char::from_u32)
            .is_some_and(|c| script(c) != Script::Unknown);
        if !assigned {
            *slot = 0;
        }
    }
    bits
}

/// Every assigned character and its script, by `Scripts.txt` through the
/// shape crate, in code point order.
fn assigned() -> Vec<(char, Script)> {
    (0..=0x10_FFFF_u32)
        .filter_map(char::from_u32)
        .map(|c| (c, script(c)))
        .filter(|(_, value)| *value != Script::Unknown)
        .collect()
}

/// A `Script` value as the census names it: `script:` and its ISO 15924
/// code.
fn script_name(value: Script) -> String {
    let code = value.iso15924();
    format!("script:{}", String::from_utf8_lossy(&code))
}

/// What a face covers: a count for each value and each class its `cmap`
/// maps any character of to a glyph other than `.notdef`, by name.
fn coverage(
    face: &Sfnt<'_>,
    assigned: &[(char, Script)],
    classes: &[u8],
) -> BTreeMap<String, usize> {
    let mut by_script: BTreeMap<Script, usize> = BTreeMap::new();
    let mut by_class = [0usize; CLASSES.len()];
    for (c, value) in assigned {
        let c = *c;
        if face.glyph_for_char(c).is_none_or(|glyph| glyph == 0) {
            continue;
        }
        *by_script.entry(*value).or_insert(0) += 1;
        let bits = classes.get(c as usize).copied().unwrap_or(0);
        for (count, class) in by_class.iter_mut().zip(CLASSES) {
            if bits & class.bit() != 0 {
                *count += 1;
            }
        }
    }
    let mut out: BTreeMap<String, usize> = by_script
        .into_iter()
        .map(|(value, count)| (script_name(value), count))
        .collect();
    for (count, class) in by_class.into_iter().zip(CLASSES) {
        if count > 0 {
            out.insert(class.name().to_string(), count);
        }
    }
    out
}

/// How many characters each value and class holds, by name.
fn totals(assigned: &[(char, Script)], classes: &[u8]) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for (_, value) in assigned {
        *out.entry(script_name(*value)).or_insert(0) += 1;
    }
    for class in CLASSES {
        let held = classes
            .iter()
            .filter(|bits| **bits & class.bit() != 0)
            .count();
        out.insert(class.name().to_string(), held);
    }
    out
}

// --- the storage forms --------------------------------------------------------

/// `maxp`'s glyph count.
fn glyphs(font: &[u8]) -> Option<u16> {
    let maxp = Sfnt::parse(font)?.table(u32::from_be_bytes(*b"maxp"))?;
    Some(u16::from_be_bytes([*maxp.get(4)?, *maxp.get(5)?]))
}

/// Whether a WOFF2 form unpacks, through `woff.rs`, to a face of the
/// published face's glyph count; `no: <why>` where it does not.
fn unpacks(published: &[u8], woff2: &[u8]) -> String {
    match woff::decode(woff2, MAX_FACE) {
        Ok(font) if glyphs(&font).is_some() && glyphs(&font) == glyphs(published) => {
            "yes".to_string()
        }
        Ok(font) => format!(
            "no: {:?} glyphs, not {:?}",
            glyphs(&font),
            glyphs(published)
        ),
        Err(error) => format!("no: {error}"),
    }
}

/// The directory `TINKER_FALLBACK_FACES` names, when it names one.
fn faces_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("TINKER_FALLBACK_FACES")?);
    dir.is_dir().then_some(dir)
}

/// The WOFF2 form of `file` in `dir`: the file of its stem with `.woff2`.
fn woff2_beside(dir: &Path, file: &str) -> Option<Vec<u8>> {
    let stem = Path::new(file).file_stem()?.to_str()?;
    std::fs::read(dir.join(format!("{stem}.woff2"))).ok()
}

// --- the census -----------------------------------------------------------------

/// **The census.** Prints, after [`RAN`], four tables, each row led by its
/// table's name: `value`, each `Script` value and class and the characters
/// it holds; `coverage`, a row for each candidate and each value or class
/// its `cmap` covers any character of; `storage`, a row for each candidate,
/// its bytes as published and as WOFF2 and whether that form unpacks to it;
/// and `group`, the faces each group carries — every candidate of it, but of
/// the mathematics candidates the one [`chosen_math`] chooses — and their
/// bytes together. Holds nothing.
#[test]
#[ignore = "a census for the owner: H-116 and H-04 are ruled with its rows, and it holds nothing"]
fn fallback_face_coverage_census() {
    let Some(dir) = faces_dir() else {
        println!(
            "{SKIPPED} fallback_face_coverage_census (set TINKER_FALLBACK_FACES to a directory \
             holding the candidates docs/design/fallback-faces.md names)"
        );
        return;
    };
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!(
        "{RAN} fallback_face_coverage_census ({}, {profile})",
        dir.display()
    );
    let classes = classes();
    let assigned = assigned();
    println!("{VALUE_HEAD}");
    for (name, characters) in totals(&assigned, &classes) {
        println!("value\t{name}\t{characters}");
    }

    let candidates = candidates();
    let mut stored: Vec<Stored> = Vec::new();
    let mut storage_rows = Vec::new();
    println!("{COVERAGE_HEAD}");
    for candidate in &candidates {
        let group = candidate.group.name();
        let Ok(font) = std::fs::read(dir.join(&candidate.file)) else {
            println!(
                "coverage\t{}\t{}\t{group}\tabsent\t",
                candidate.file, candidate.face
            );
            storage_rows.push(format!(
                "storage\t{}\t{}\t{group}\tabsent\t\t\t{}\t{}",
                candidate.file, candidate.face, candidate.upstream, candidate.spdx
            ));
            continue;
        };
        let mut math = 0;
        match Sfnt::parse(&font) {
            Some(face) => {
                for (value, covered) in coverage(&face, &assigned, &classes) {
                    if value == Class::Math.name() {
                        math = covered;
                    }
                    println!(
                        "coverage\t{}\t{}\t{group}\t{value}\t{covered}",
                        candidate.file, candidate.face
                    );
                }
            }
            None => println!(
                "coverage\t{}\t{}\t{group}\tunreadable\t",
                candidate.file, candidate.face
            ),
        }
        let woff2 = woff2_beside(&dir, &candidate.file);
        let (woff2_bytes, woff2_unpacks) = match &woff2 {
            Some(packed) => (packed.len().to_string(), unpacks(&font, packed)),
            None => ("absent".to_string(), "absent".to_string()),
        };
        storage_rows.push(format!(
            "storage\t{}\t{}\t{group}\t{}\t{woff2_bytes}\t{woff2_unpacks}\t{}\t{}",
            candidate.file,
            candidate.face,
            font.len(),
            candidate.upstream,
            candidate.spdx
        ));
        stored.push(Stored {
            group: candidate.group,
            file: candidate.file.clone(),
            bytes: font.len(),
            woff2: woff2.as_ref().map(Vec::len),
            math,
        });
    }

    println!("{STORAGE_HEAD}");
    for row in storage_rows {
        println!("{row}");
    }
    let math: Vec<(&str, usize, Option<usize>)> = stored
        .iter()
        .filter(|face| face.group == Group::Math)
        .map(|face| (face.file.as_str(), face.math, face.woff2))
        .collect();
    let chosen = chosen_math(&math);
    println!("{GROUP_HEAD}");
    for group in Group::ALL {
        let carried: Vec<&Stored> = stored
            .iter()
            .filter(|face| face.group == group)
            .filter(|face| group != Group::Math || Some(face.file.as_str()) == chosen)
            .collect();
        let bytes: usize = carried.iter().map(|face| face.bytes).sum();
        let woff2: Option<usize> = carried.iter().map(|face| face.woff2).sum();
        println!(
            "group\t{}\t{}\t{bytes}\t{}",
            group.name(),
            carried.len(),
            woff2.map_or_else(|| "absent".to_string(), |bytes| bytes.to_string())
        );
    }
}

/// A candidate the census read: its group and file, its bytes as published
/// and as WOFF2, and how many `Math` characters its `cmap` covers.
struct Stored {
    group: Group,
    file: String,
    bytes: usize,
    woff2: Option<usize>,
    math: usize,
}

/// **The mathematics face, by the design's rule**: of the candidates, each
/// `(file, Math characters covered, WOFF2 bytes)`, the one that covers the
/// most, and of two that cover as many the smaller stored, a form not
/// measured never the smaller; of two alike in both, the first. `None` where
/// there is no candidate.
fn chosen_math<'a>(candidates: &[(&'a str, usize, Option<usize>)]) -> Option<&'a str> {
    candidates
        .iter()
        .min_by_key(|(_, covered, woff2)| {
            (
                std::cmp::Reverse(*covered),
                woff2.is_none(),
                woff2.unwrap_or(0),
            )
        })
        .map(|(file, _, _)| *file)
}

// --- the instruments ------------------------------------------------------------

/// **The classes are read from the vendored files as the design names
/// them**: `+` is `Math` and a symbol (`Sm`); U+1F600 is an emoji, a
/// pictograph and a symbol (`So`); U+1D11E, the G clef, is music and a
/// symbol; U+1D2E0, a Mayan numeral, is `Common`'s `No` and so a symbol;
/// `A`, `1` and `日` are none of them; and an unassigned code point is in no
/// class, `Extended_Pictographic`'s reserved ones included.
#[test]
fn the_classes_are_read_from_the_vendored_data() {
    let classes = classes();
    let of = |point: u32| {
        let bits = classes[point as usize];
        CLASSES
            .iter()
            .filter(|class| bits & class.bit() != 0)
            .copied()
            .collect::<Vec<Class>>()
    };
    assert_eq!(of(0x2B), [Class::Math, Class::Symbols], "+");
    assert_eq!(
        of(0x1F600),
        [Class::Emoji, Class::Pictographic, Class::Symbols],
        "U+1F600"
    );
    assert_eq!(of(0x1D11E), [Class::Music, Class::Symbols], "the G clef");
    assert_eq!(of(0x1D2E0), [Class::Symbols], "a Mayan numeral");
    for point in [0x41, 0x31, 0x65E5] {
        assert_eq!(of(point), [], "U+{point:04X}");
    }
    // Extended_Pictographic reserves U+1FC00..U+1FFFD, unassigned.
    assert_eq!(of(0x1FC00), [], "a reserved pictograph is no character");
    let held = totals(&assigned(), &classes);
    for class in CLASSES {
        assert!(held[class.name()] > 0, "{} holds characters", class.name());
    }
    assert!(held["script:Latn"] > 1000, "Latin holds its characters");
    assert!(held["script:Hani"] > 90_000, "and Han its");
    assert!(!held.contains_key("script:Zzzz"), "unassigned is no value");
}

/// A face whose one table is a `cmap` of format 12 under (3, 10), mapping
/// each of `points` to its own glyph and `notdef` to glyph 0.
fn face_mapping(points: &[u32], notdef: &[u32]) -> Vec<u8> {
    let mut groups: Vec<(u32, u32)> = points
        .iter()
        .enumerate()
        .map(|(at, point)| (*point, u32::try_from(at).expect("few") + 1))
        .chain(notdef.iter().map(|point| (*point, 0)))
        .collect();
    groups.sort_unstable();
    let mut subtable = Vec::new();
    subtable.extend_from_slice(&12u16.to_be_bytes());
    subtable.extend_from_slice(&0u16.to_be_bytes());
    let length = 16 + 12 * groups.len();
    subtable.extend_from_slice(&u32::try_from(length).expect("small").to_be_bytes());
    subtable.extend_from_slice(&0u32.to_be_bytes());
    subtable.extend_from_slice(&u32::try_from(groups.len()).expect("few").to_be_bytes());
    for (point, glyph) in groups {
        subtable.extend_from_slice(&point.to_be_bytes());
        subtable.extend_from_slice(&point.to_be_bytes());
        subtable.extend_from_slice(&glyph.to_be_bytes());
    }
    let mut cmap = Vec::new();
    cmap.extend_from_slice(&0u16.to_be_bytes());
    cmap.extend_from_slice(&1u16.to_be_bytes());
    cmap.extend_from_slice(&3u16.to_be_bytes());
    cmap.extend_from_slice(&10u16.to_be_bytes());
    cmap.extend_from_slice(&12u32.to_be_bytes());
    cmap.extend_from_slice(&subtable);

    let mut font = Vec::new();
    font.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    font.extend_from_slice(&1u16.to_be_bytes());
    font.extend_from_slice(&[0, 16, 0, 0, 0, 0]);
    font.extend_from_slice(b"cmap");
    font.extend_from_slice(&0u32.to_be_bytes());
    font.extend_from_slice(&28u32.to_be_bytes());
    font.extend_from_slice(&u32::try_from(cmap.len()).expect("small").to_be_bytes());
    font.extend_from_slice(&cmap);
    font
}

/// **A face is counted by its `cmap`**: a face mapping `A`, `+`, U+1F600,
/// the G clef and あ to glyphs, and `日` to `.notdef`, covers one Latin
/// character, three `Common` ones and one Hiragana, no Han, and one `Math`,
/// one emoji, one pictograph, one musical and three symbol characters, each
/// counted once.
#[test]
fn a_face_is_counted_by_its_cmap() {
    let font = face_mapping(&[0x41, 0x2B, 0x1F600, 0x1D11E, 0x3042], &[0x65E5]);
    let face = Sfnt::parse(&font).expect("an sfnt");
    let counted = coverage(&face, &assigned(), &classes());
    let expected: BTreeMap<String, usize> = [
        ("script:Latn", 1),
        ("script:Zyyy", 3),
        ("script:Hira", 1),
        ("class:emoji", 1),
        ("class:pictographic", 1),
        ("class:math", 1),
        ("class:music", 1),
        ("class:symbols", 3),
    ]
    .into_iter()
    .map(|(name, count)| (name.to_string(), count))
    .collect();
    assert_eq!(counted, expected);
}

/// **The mathematics face is the rule's**: the face covering the most `Math`
/// characters wins whatever its size; of two covering as many, the smaller
/// WOFF2 form, whichever comes first; a form not measured loses such a tie
/// to a measured one, even a larger; of two alike in both, the first; and
/// no candidate chooses none.
#[test]
fn the_mathematics_face_is_the_rules() {
    assert_eq!(
        chosen_math(&[("a", 100, Some(10)), ("b", 101, Some(900))]),
        Some("b"),
        "the most covered, though larger"
    );
    assert_eq!(
        chosen_math(&[("a", 100, Some(900)), ("b", 100, Some(10))]),
        Some("b"),
        "of as many, the smaller"
    );
    assert_eq!(
        chosen_math(&[("a", 100, Some(10)), ("b", 100, Some(900))]),
        Some("a"),
        "the smaller, first"
    );
    assert_eq!(
        chosen_math(&[("a", 100, None), ("b", 100, Some(900))]),
        Some("b"),
        "an unmeasured form is never the smaller"
    );
    assert_eq!(
        chosen_math(&[("a", 100, None), ("b", 99, Some(10))]),
        Some("a"),
        "but coverage comes first"
    );
    assert_eq!(
        chosen_math(&[("a", 100, Some(10)), ("b", 100, Some(10))]),
        Some("a"),
        "alike: the first"
    );
    assert_eq!(chosen_math(&[]), None, "no candidate");
}

/// `docs/design/fallback-faces.md`, as committed.
fn design() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/design/fallback-faces.md");
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// **The candidates are the design's**: every face the design's three
/// script tables and its class table name is a candidate of the group its
/// table names, and every candidate but FT-01's region faces is one of
/// them; each is a Regular of the upstream the design names, under
/// `OFL-1.1`.
#[test]
fn every_candidate_is_the_designs() {
    let doc = design();
    let section = doc
        .split("## Every script, emoji, mathematics, music and symbols (FT-02)")
        .nth(1)
        .expect("the FT-02 section");
    let mut designed: Vec<(String, Group)> = Vec::new();
    let mut group = None;
    for line in section.lines() {
        if line.starts_with("`fallback-scripts`,") {
            group = Some(Group::Scripts);
        } else if line.starts_with("`fallback-scripts-limited`,") {
            group = Some(Group::Limited);
        } else if line.starts_with("`fallback-scripts-historic`,") {
            group = Some(Group::Historic);
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if !line.starts_with("| ") || cells.len() < 3 || cells[0] == "---" {
            continue;
        }
        if cells[0] == "Face" || cells[0] == "Feature" {
            continue;
        }
        if let Some(feature) = cells[0].strip_prefix('`').and_then(|c| c.strip_suffix('`')) {
            let of = Group::ALL
                .into_iter()
                .find(|group| group.name() == feature)
                .unwrap_or_else(|| panic!("the design's feature `{feature}`"));
            for candidate in cells[1].split(';') {
                let face = candidate.split(',').next().unwrap_or("").trim();
                designed.push((face.to_string(), of));
            }
        } else if let Some(of) = group {
            designed.push((cells[0].to_string(), of));
        }
    }
    let mut ours: Vec<(String, Group)> = candidates()
        .into_iter()
        .filter(|candidate| candidate.group != Group::Cjk)
        .map(|candidate| (candidate.face, candidate.group))
        .collect();
    designed.sort();
    ours.sort();
    assert_eq!(ours, designed, "the census's candidates and the design's");
    for candidate in candidates() {
        assert_eq!(candidate.spdx, "OFL-1.1", "{}", candidate.face);
        assert!(
            candidate.file.ends_with("-Regular.ttf")
                || candidate.file.ends_with("-Regular.otf")
                || candidate.file == "NotoEmoji[wght].ttf",
            "{}",
            candidate.file
        );
    }
    let files: Vec<String> = candidates().into_iter().map(|c| c.file).collect();
    let mut unique = files.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), files.len(), "no candidate twice");
}

/// The committed record's rows, `#` comments and blank lines aside.
fn record() -> Vec<Vec<String>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fallback_coverage.tsv");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| line.split('\t').map(str::to_string).collect())
        .collect()
}

/// The mathematics face the record's own counts choose, by [`chosen_math`]:
/// each mathematics candidate's `class:math` coverage row, none counting as
/// none covered, and its `storage` row's WOFF2 bytes.
fn recorded_math_choice(rows: &[Vec<String>]) -> Option<String> {
    let group = Group::Math.name();
    let faces: Vec<(&str, usize, Option<usize>)> = rows
        .iter()
        .filter(|row| row[0] == "storage" && row.get(3).is_some_and(|cell| cell == group))
        .map(|row| {
            let covered: usize = rows
                .iter()
                .find(|coverage| {
                    coverage[0] == "coverage"
                        && coverage.get(1) == Some(&row[1])
                        && coverage
                            .get(4)
                            .is_some_and(|value| value == Class::Math.name())
                })
                .and_then(|coverage| coverage.get(5)?.parse().ok())
                .unwrap_or(0);
            let woff2: Option<usize> = row.get(5).and_then(|bytes| bytes.parse().ok());
            (row[1].as_str(), covered, woff2)
        })
        .collect();
    chosen_math(&faces).map(str::to_string)
}

/// **The committed census is whole and adds up**: its four heads, in order;
/// a `value` row for every `Script` value with characters and every class,
/// each the count the vendored data gives; a `coverage` row for every
/// candidate, each value or class one the `value` table holds and covered no
/// more than it holds; a `storage` row for every candidate, its WOFF2 form
/// unpacking to it; and a `group` row for every group, its faces those it
/// carries — its candidates, but of the mathematics ones the one
/// [`chosen_math`] chooses by the record's own counts — and its bytes their
/// sum.
#[test]
fn the_committed_census_is_whole() {
    let rows = record();
    let heads: Vec<String> = rows
        .iter()
        .filter(|row| {
            row.get(1)
                .is_some_and(|cell| cell == "name" || cell == "file")
        })
        .map(|row| row.join("\t"))
        .collect();
    assert_eq!(heads, [VALUE_HEAD, COVERAGE_HEAD, STORAGE_HEAD, GROUP_HEAD]);
    let of = |table: &str| -> Vec<&Vec<String>> {
        rows.iter()
            .filter(|row| row[0] == table && row.get(1).is_some_and(|c| c != "name" && c != "file"))
            .collect()
    };

    let committed: BTreeMap<&str, usize> = of("value")
        .into_iter()
        .map(|row| (row[1].as_str(), row[2].parse().expect("a count")))
        .collect();
    let derived = totals(&assigned(), &classes());
    assert_eq!(
        committed,
        derived
            .iter()
            .map(|(name, count)| (name.as_str(), *count))
            .collect::<BTreeMap<_, _>>(),
        "each value's characters, as the vendored data gives them"
    );

    let candidates = candidates();
    for candidate in &candidates {
        let covered: Vec<&Vec<String>> = of("coverage")
            .into_iter()
            .filter(|row| row[1] == candidate.file)
            .collect();
        assert!(!covered.is_empty(), "{} is counted", candidate.file);
        for row in covered {
            assert_eq!(row[2], candidate.face, "{}", candidate.file);
            assert_eq!(row[3], candidate.group.name(), "{}", candidate.file);
            let held = committed
                .get(row[4].as_str())
                .unwrap_or_else(|| panic!("{}: {} is a value", candidate.file, row[4]));
            let count: usize = row[5].parse().expect("a count");
            assert!(
                count > 0 && count <= *held,
                "{} covers {count} of {}'s {held}",
                candidate.file,
                row[4]
            );
        }
        let storage: Vec<&Vec<String>> = of("storage")
            .into_iter()
            .filter(|row| row[1] == candidate.file)
            .collect();
        assert_eq!(storage.len(), 1, "{} is stored once", candidate.file);
        let row = storage[0];
        assert!(row[4].parse::<usize>().is_ok(), "{}: its bytes", row[1]);
        assert!(
            row[5].parse::<usize>().is_ok(),
            "{}: its WOFF2 bytes",
            row[1]
        );
        assert_eq!(row[6], "yes", "{}: its WOFF2 form unpacks to it", row[1]);
        assert_eq!(row[7], candidate.upstream, "{}", row[1]);
        assert_eq!(row[8], candidate.spdx, "{}", row[1]);
    }

    let math = recorded_math_choice(&rows);
    assert!(math.is_some(), "a mathematics face is chosen");
    for group in Group::ALL {
        let row = of("group")
            .into_iter()
            .find(|row| row[1] == group.name())
            .unwrap_or_else(|| panic!("a row for {}", group.name()));
        let members: Vec<&Vec<String>> = of("storage")
            .into_iter()
            .filter(|storage| storage[3] == group.name())
            .filter(|storage| group != Group::Math || math.as_deref() == Some(storage[1].as_str()))
            .collect();
        let sum = |at: usize| -> usize {
            members
                .iter()
                .map(|storage| storage[at].parse::<usize>().expect("a count"))
                .sum()
        };
        assert_eq!(
            row[2].parse::<usize>().ok(),
            Some(members.len()),
            "{}'s faces",
            group.name()
        );
        assert_eq!(
            row[3].parse::<usize>().ok(),
            Some(sum(4)),
            "{}",
            group.name()
        );
        assert_eq!(
            row[4].parse::<usize>().ok(),
            Some(sum(5)),
            "{}",
            group.name()
        );
    }
}

/// The design's name for the region of FT-01's face `SourceHanSans<region>`.
fn region_name(region: &str) -> &'static str {
    match region {
        "CN" => "Simplified Chinese",
        "TW" => "Traditional Chinese",
        "JP" => "Japanese",
        "KR" => "Korean",
        other => panic!("no region {other}"),
    }
}

/// **The design states the outcomes the record gives**: the mathematics
/// face [`chosen_math`] chooses by the committed counts, and the order
/// FT-01's four region faces are asked in where neither a language nor a
/// collection chooses — by how many Han characters each covers, most first,
/// no two covering as many — each named in the design after a dash and
/// followed by the record's name.
#[test]
fn the_designs_outcomes_are_the_records() {
    let rows = record();
    let design = design().split_whitespace().collect::<Vec<_>>().join(" ");

    let math = recorded_math_choice(&rows).expect("a mathematics face");
    let face = candidates()
        .into_iter()
        .find(|candidate| candidate.file == math)
        .map(|candidate| candidate.face)
        .expect("a candidate");
    assert!(
        design.contains(&format!("— {face}, by `fallback_coverage.tsv`")),
        "the design names {face} the mathematics face"
    );

    let mut han: Vec<(usize, &str)> = ["JP", "CN", "TW", "KR"]
        .into_iter()
        .map(|region| {
            let file = format!("SourceHanSans{region}-Regular.otf");
            let covered: usize = rows
                .iter()
                .find(|row| {
                    row[0] == "coverage"
                        && row.get(1) == Some(&file)
                        && row.get(4).is_some_and(|value| value == "script:Hani")
                })
                .and_then(|row| row.get(5)?.parse().ok())
                .unwrap_or_else(|| panic!("{file} counts Han"));
            (covered, region)
        })
        .collect();
    han.sort_by(|a, b| b.0.cmp(&a.0));
    assert!(
        han.windows(2).all(|pair| pair[0].0 > pair[1].0),
        "no two region faces cover as many Han characters: {han:?}"
    );
    let order: Vec<&str> = han.iter().map(|(_, region)| region_name(region)).collect();
    assert!(
        design.contains(&format!(
            "— {}, by `fallback_coverage.tsv`",
            order.join(", ")
        )),
        "the design asks the region faces {order:?}"
    );
}

/// **The committed wasm sizes are whole and carry their faces**:
/// `fallback_sizes.tsv` beside this file, its head and then a row for the
/// default profile and for each of FT-02's groups, in the census's order,
/// each the gzipped bytes of `bindings/js`'s release wasm as `release.yml`
/// builds the default profile, a group's with a feature carrying the WOFF2
/// forms the census's `group` row counts, all of one revision; FT-01's
/// faces are `footprint.json`'s `cjk-fonts` profile instead. Gzip cannot
/// shrink a WOFF2 form, already Brotli's, by a hundredth, so each group's
/// figure passes the default's by at least 99 in 100 of those WOFF2 bytes:
/// a build whose linker dropped the faces fails.
#[test]
fn the_committed_wasm_sizes_are_whole() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fallback_sizes.tsv");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}: each profile's gzipped wasm is measured by a wasm build, as \
             docs/design/fallback-faces.md says, and recorded in this file",
            path.display()
        )
    });
    let rows: Vec<Vec<&str>> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| line.split('\t').collect())
        .collect();
    assert_eq!(
        rows.first().map(|row| row.join("\t")).as_deref(),
        Some(WASM_HEAD),
        "its head"
    );
    let profiles: Vec<&str> = rows[1..]
        .iter()
        .map(|row| row.get(1).copied().unwrap_or(""))
        .collect();
    let mut expected = vec!["default"];
    expected.extend(
        Group::ALL
            .into_iter()
            .filter(|group| *group != Group::Cjk)
            .map(Group::name),
    );
    assert_eq!(
        profiles, expected,
        "a row for the default profile and for each group, in order"
    );
    let figure = |row: &[&str]| -> usize {
        assert_eq!(row.len(), 3, "{row:?}");
        assert_eq!(row[0], "wasm", "{row:?}");
        row[2]
            .parse()
            .ok()
            .filter(|bytes| *bytes > 0)
            .unwrap_or_else(|| panic!("{}: a figure", row[1]))
    };
    let default = figure(&rows[1]);
    let census = record();
    for row in &rows[2..] {
        let bytes = figure(row);
        let woff2: usize = census
            .iter()
            .find(|group| group[0] == "group" && group.get(1).is_some_and(|name| name == row[1]))
            .and_then(|group| group.get(4)?.parse().ok())
            .unwrap_or_else(|| panic!("{}: the census's group row", row[1]));
        assert!(
            bytes > default && bytes - default >= woff2 - woff2 / 100,
            "{}: {bytes} gzipped, the default {default}, for {woff2} WOFF2 bytes",
            row[1]
        );
    }
}
