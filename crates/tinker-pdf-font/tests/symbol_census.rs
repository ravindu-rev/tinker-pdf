//! **The census H-94 is ruled with** (`docs/design/symbol-faces.md`,
//! FT-03.3): for each candidate face the design names, how many of Symbol's
//! 189 and ZapfDingbats' 188 codes reach a glyph by the route, which step
//! reached each, the licence the face's `name` table states and the one its
//! upstream declares, whether `deny.toml` admits that, and the face's bytes.
//!
//! The census rules nothing: admissibility is H-94's, and the census only
//! prices each answer. It counts through `tinker_pdf_font::substitute_glyph`,
//! the function the facade draws a substituted Symbol or ZapfDingbats font
//! through, so its figures are the drawing's.
//!
//! # Fetched, never committed
//!
//! The candidates are fetched once into a directory of the operator's, from
//! the upstreams `symbol-faces.md` names, and `TINKER_SYMBOL_FACES` names
//! it; nothing of them reaches the tree (ruling 18). Without the variable, or
//! with one that names no directory, the census prints [`SKIPPED`] and does
//! nothing; with it, [`RAN`] and a row per candidate, a candidate the
//! directory lacks marked absent.
//!
//! ```text
//! TINKER_SYMBOL_FACES=<directory> cargo test -p tinker-pdf-font --test symbol_census -- --ignored --nocapture
//! ```
//!
//! The rows it prints are committed, when it runs, as `symbol_census.tsv`
//! beside this file, and the run's revision and day go in that commit's
//! message (ruling 16).
//!
//! The tests below the census hold its instruments — the allowlist reading,
//! the SPDX reading, the `name` table reading, the candidate list and the
//! counting — to what they state, over inputs built here, and run in every
//! `cargo test`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tinker_pdf_font::encoding::{base_char, BaseEncoding};
use tinker_pdf_font::{substitute_glyph, BuiltInEncoding, Sfnt, SymbolRoute};

/// Printed when the census ran: a skipped census exits 0 and reads like a
/// pass, so a run without this line is no measurement.
const RAN: &str = "symbol-census: RAN";

/// Printed when there was nothing to measure.
const SKIPPED: &str = "symbol-census: SKIPPED";

/// One candidate face, as `symbol-faces.md`'s census table names it.
struct Candidate {
    /// The file the census reads from `TINKER_SYMBOL_FACES`.
    file: &'static str,
    /// The face's name.
    face: &'static str,
    /// Where it is fetched from.
    upstream: &'static str,
    /// The licence its upstream declares, in the upstream's words.
    declared: &'static str,
    /// That licence's SPDX expression, as far as SPDX names it.
    spdx: &'static str,
}

const URW: &str = "https://github.com/ArtifexSoftware/urw-base35-fonts tag 20200910 fonts/";
const URW_LICENCE: &str = "GNU Affero GPL version 3, with an exception for including the fonts in a PostScript or PDF document";

/// The candidates, in the design's order.
const CANDIDATES: [Candidate; 10] = [
    Candidate {
        file: "StandardSymbolsPS.ttf",
        face: "URW++ Standard Symbols PS",
        upstream: URW,
        declared: URW_LICENCE,
        spdx: "AGPL-3.0-only",
    },
    Candidate {
        file: "StandardSymbolsPS.otf",
        face: "URW++ Standard Symbols PS",
        upstream: URW,
        declared: URW_LICENCE,
        spdx: "AGPL-3.0-only",
    },
    Candidate {
        file: "D050000L.ttf",
        face: "URW++ D050000L",
        upstream: URW,
        declared: URW_LICENCE,
        spdx: "AGPL-3.0-only",
    },
    Candidate {
        file: "D050000L.otf",
        face: "URW++ D050000L",
        upstream: URW,
        declared: URW_LICENCE,
        spdx: "AGPL-3.0-only",
    },
    Candidate {
        file: "NotoSansSymbols-Regular.ttf",
        face: "Noto Sans Symbols",
        upstream: "https://github.com/notofonts/symbols release NotoSansSymbols-v2.003, unhinted TrueType",
        declared: "SIL Open Font License 1.1",
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "NotoSansSymbols2-Regular.ttf",
        face: "Noto Sans Symbols 2",
        upstream: "https://github.com/notofonts/symbols release NotoSansSymbols2-v2.008, unhinted TrueType",
        declared: "SIL Open Font License 1.1",
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "NotoSansMath-Regular.ttf",
        face: "Noto Sans Math",
        upstream: "https://github.com/notofonts/math release NotoSansMath-v3.000, unhinted TrueType",
        declared: "SIL Open Font License 1.1",
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "STIXTwoMath-Regular.otf",
        face: "STIX Two Math",
        upstream: "https://github.com/stipub/stixfonts release v2.13 fonts/static_otf/",
        declared: "SIL Open Font License 1.1",
        spdx: "OFL-1.1",
    },
    Candidate {
        file: "DejaVuSans.ttf",
        face: "DejaVu Sans",
        upstream: "https://github.com/dejavu-fonts/dejavu-fonts release version_2_37, dejavu-fonts-ttf-2.37",
        declared: "the Bitstream Vera Fonts licence, with DejaVu's changes in the public domain and the Arev Fonts licence for the glyphs taken from them",
        spdx: "Bitstream-Vera",
    },
    Candidate {
        file: "FreeSerif.otf",
        face: "GNU FreeFont FreeSerif",
        upstream: "https://ftp.gnu.org/gnu/freefont/ freefont-otf-20120503",
        declared: "GNU GPL version 3 or later, with the font exception",
        spdx: "GPL-3.0-or-later WITH Font-exception-2.0",
    },
];

/// The directory `TINKER_SYMBOL_FACES` names, when it names one.
fn faces_dir() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("TINKER_SYMBOL_FACES")?);
    dir.is_dir().then_some(dir)
}

fn repo_file(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// What the route reached for one encoding over one face.
#[derive(Debug, Default, PartialEq, Eq)]
struct Reach {
    by_name: usize,
    by_unicode: usize,
    by_symbol_code: usize,
    /// The codes nothing reached, by name, in code order.
    missed: Vec<&'static str>,
    /// The codes something reached.
    reached: BTreeSet<u8>,
}

impl Reach {
    fn total(&self) -> usize {
        self.by_name + self.by_unicode + self.by_symbol_code
    }
}

/// Every code of `encoding` through the route over `program`, each code with
/// the name its built-in encoding gives it, as a document that names nothing
/// else shows it.
fn reach(program: &[u8], encoding: BuiltInEncoding) -> Reach {
    let mut out = Reach::default();
    for entry in encoding.codes() {
        match substitute_glyph(program, encoding, Some(entry.name), entry.code) {
            Some((_, SymbolRoute::Name)) => out.by_name += 1,
            Some((_, SymbolRoute::Unicode)) => out.by_unicode += 1,
            Some((_, SymbolRoute::SymbolCode)) => out.by_symbol_code += 1,
            None => {
                out.missed.push(entry.name);
                continue;
            }
        }
        out.reached.insert(entry.code);
    }
    out
}

fn be16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

/// The `name` table record `id` as text: a Windows Unicode record first
/// (platform 3, encoding 1 or 10), American English before any other
/// language, then a Unicode platform record, then a Macintosh Roman one.
/// The text has its white space made single spaces, so it fits a TSV cell.
fn name_record(program: &[u8], id: u16) -> Option<String> {
    let table = Sfnt::parse(program)?.table(0x6E61_6D65)?;
    let count = usize::from(be16(table, 2)?);
    let strings = usize::from(be16(table, 4)?);
    let mut best: Option<(u8, String)> = None;
    for i in 0..count.min(1 << 12) {
        let at = 6 + i * 12;
        let (Some(platform), Some(encoding), Some(language), Some(name), Some(len), Some(off)) = (
            be16(table, at),
            be16(table, at + 2),
            be16(table, at + 4),
            be16(table, at + 6),
            be16(table, at + 8),
            be16(table, at + 10),
        ) else {
            break;
        };
        if name != id {
            continue;
        }
        let start = strings + usize::from(off);
        let Some(bytes) = table.get(start..start + usize::from(len)) else {
            continue;
        };
        let rank = match (platform, encoding, language) {
            (3, 1 | 10, 0x0409) => 0,
            (3, 1 | 10, _) => 1,
            (0, _, _) => 2,
            (1, 0, _) => 3,
            _ => continue,
        };
        if best.as_ref().is_some_and(|(held, _)| *held <= rank) {
            continue;
        }
        let text: String = if platform == 1 {
            bytes
                .iter()
                .filter_map(|byte| base_char(BaseEncoding::MacRoman, *byte))
                .collect()
        } else {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            char::decode_utf16(units)
                .map(|unit| unit.unwrap_or('\u{FFFD}'))
                .collect()
        };
        best = Some((rank, text.split_whitespace().collect::<Vec<_>>().join(" ")));
    }
    best.map(|(_, text)| text)
}

/// The identifiers `deny.toml`'s `[licenses]` table allows, in its order.
fn allowlist(deny: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_licenses = false;
    let mut in_allow = false;
    for line in deny.lines() {
        let code = line.split('#').next().unwrap_or("").trim();
        if code.starts_with('[') && !in_allow {
            in_licenses = code == "[licenses]";
            continue;
        }
        if in_licenses && !in_allow && code.starts_with("allow") && code.contains('[') {
            in_allow = true;
        }
        if !in_allow {
            continue;
        }
        let mut rest = code;
        while let Some(open) = rest.find('"') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('"') else {
                break;
            };
            out.push(after[..close].to_string());
            rest = &after[close + 1..];
        }
        if code.contains(']') {
            in_allow = false;
            in_licenses = false;
        }
    }
    out
}

/// Whether an SPDX expression is admitted: one of its `OR` alternatives has
/// every `AND` term on the allowlist, a term with an exception (`WITH`)
/// listed whole, as `deny.toml` lists `Apache-2.0 WITH LLVM-exception`.
/// The candidates' expressions carry no parentheses, and one that did would
/// be read as a term and admitted by nothing.
fn admitted(expression: &str, allow: &[String]) -> bool {
    expression.split(" OR ").any(|alternative| {
        let terms: Vec<&str> = alternative.split(" AND ").map(str::trim).collect();
        !terms.iter().any(|term| term.is_empty())
            && terms
                .iter()
                .all(|term| allow.iter().any(|allowed| allowed == term))
    })
}

/// A TSV cell: no tab or line break inside it.
fn cell(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

/// **The census.** Prints a header and a row per candidate, then a row
/// counting the codes some admitted candidate reaches, since a bundled answer
/// could pair two faces. Holds nothing.
#[test]
#[ignore = "a census for the owner: H-94 is ruled with its rows, and it holds nothing"]
fn symbol_face_census() {
    let Some(dir) = faces_dir() else {
        println!(
            "{SKIPPED} symbol_face_census (set TINKER_SYMBOL_FACES to a directory holding \
             the candidates docs/design/symbol-faces.md names)"
        );
        return;
    };
    let deny = std::fs::read_to_string(repo_file("deny.toml")).expect("deny.toml");
    let allow = allowlist(&deny);
    println!("{RAN} symbol_face_census ({})", dir.display());
    println!(
        "file\tface\tbytes\tsymbol_reached_of_189\tsymbol_by_name\tsymbol_by_unicode\t\
         symbol_by_symbol_code\tdingbats_reached_of_188\tdingbats_by_name\tdingbats_by_unicode\t\
         dingbats_by_symbol_code\tname_table_licence\tname_table_licence_url\tupstream\t\
         upstream_licence\tspdx\tdeny_toml_admits\tsymbol_missed\tdingbats_missed"
    );
    let mut together = [BTreeSet::new(), BTreeSet::new()];
    for candidate in &CANDIDATES {
        let admits = admitted(candidate.spdx, &allow);
        let Ok(program) = std::fs::read(dir.join(candidate.file)) else {
            println!(
                "{}\t{}\tabsent\t\t\t\t\t\t\t\t\t\t\t{}\t{}\t{}\t{}\t\t",
                candidate.file,
                candidate.face,
                cell(candidate.upstream),
                cell(candidate.declared),
                candidate.spdx,
                if admits { "yes" } else { "no" },
            );
            continue;
        };
        let symbol = reach(&program, BuiltInEncoding::Symbol);
        let dingbats = reach(&program, BuiltInEncoding::ZapfDingbats);
        if admits {
            together[0].extend(symbol.reached.iter().copied());
            together[1].extend(dingbats.reached.iter().copied());
        }
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            candidate.file,
            candidate.face,
            program.len(),
            symbol.total(),
            symbol.by_name,
            symbol.by_unicode,
            symbol.by_symbol_code,
            dingbats.total(),
            dingbats.by_name,
            dingbats.by_unicode,
            dingbats.by_symbol_code,
            cell(&name_record(&program, 13).unwrap_or_default()),
            cell(&name_record(&program, 14).unwrap_or_default()),
            cell(candidate.upstream),
            cell(candidate.declared),
            candidate.spdx,
            if admits { "yes" } else { "no" },
            symbol.missed.join(","),
            dingbats.missed.join(","),
        );
    }
    println!(
        "admitted together\t\t\t{}\t\t\t\t{}\t\t\t\t\t\t\t\t\t\t\t\t",
        together[0].len(),
        together[1].len(),
    );
}

/// The allowlist is read from `deny.toml` itself: the identifiers the
/// `[licenses]` table's `allow` array holds, its comments ignored.
#[test]
fn the_census_reads_deny_tomls_allowlist() {
    let deny = std::fs::read_to_string(repo_file("deny.toml")).expect("deny.toml");
    let allow = allowlist(&deny);
    for listed in [
        "OFL-1.1",
        "BSD-3-Clause",
        "MIT",
        "Apache-2.0 WITH LLVM-exception",
    ] {
        assert!(allow.iter().any(|id| id == listed), "{listed} in {allow:?}");
    }
    assert!(!allow.iter().any(|id| id.contains("GPL")), "{allow:?}");

    let written = "[advisories]\nallow = [\"not-this\"]\n\n[licenses]\n# a comment \"Zlib\"\nallow = [\n  \"MIT\", # \"ISC\"\n  \"OFL-1.1\",\n]\n\n[bans]\nallow = [\"neither\"]\n";
    assert_eq!(allowlist(written), ["MIT", "OFL-1.1"]);
    assert!(allowlist("").is_empty());
}

/// An expression is admitted when one alternative has every term listed, an
/// exception listed whole with its licence.
#[test]
fn an_spdx_expression_is_admitted_only_when_every_identifier_it_needs_is() {
    let allow: Vec<String> = ["MIT", "OFL-1.1", "Apache-2.0 WITH LLVM-exception"]
        .iter()
        .map(|id| (*id).to_string())
        .collect();
    assert!(admitted("OFL-1.1", &allow));
    assert!(!admitted("AGPL-3.0-only", &allow));
    assert!(admitted("AGPL-3.0-only OR MIT", &allow));
    assert!(!admitted("MIT AND AGPL-3.0-only", &allow));
    assert!(admitted("MIT AND OFL-1.1", &allow));
    assert!(admitted("Apache-2.0 WITH LLVM-exception", &allow));
    assert!(
        !admitted("Apache-2.0", &allow),
        "the exception is not the licence"
    );
    assert!(!admitted(
        "GPL-3.0-or-later WITH Font-exception-2.0",
        &allow
    ));
    assert!(!admitted("", &allow));
    assert!(
        !admitted("(MIT OR OFL-1.1)", &allow),
        "parentheses are read as a term"
    );
}

/// A `name` table with `records`, each `(platform, encoding, language, id,
/// bytes)`, in a face of its own.
fn face_with_names(records: &[(u16, u16, u16, u16, Vec<u8>)]) -> Vec<u8> {
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_be_bytes()); // format
    table.extend_from_slice(&(records.len() as u16).to_be_bytes());
    table.extend_from_slice(&((6 + 12 * records.len()) as u16).to_be_bytes());
    let mut offset = 0usize;
    for (platform, encoding, language, id, bytes) in records {
        for value in [
            *platform,
            *encoding,
            *language,
            *id,
            bytes.len() as u16,
            offset as u16,
        ] {
            table.extend_from_slice(&value.to_be_bytes());
        }
        offset += bytes.len();
    }
    for (.., bytes) in records {
        table.extend_from_slice(bytes);
    }
    let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(b"name");
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&28u32.to_be_bytes());
    out.extend_from_slice(&(table.len() as u32).to_be_bytes());
    out.extend_from_slice(&table);
    out
}

fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

/// The licence text is the Windows English record where there is one, then
/// any Windows record, then a Unicode one, then a Macintosh Roman one; its
/// white space is single spaces, and a record past the table's end is none.
#[test]
fn a_name_table_is_read_windows_english_first() {
    let face = face_with_names(&[
        (1, 0, 0, 13, b"Mac \xA9 text".to_vec()),
        (3, 1, 0x040C, 13, utf16("en fran\u{E7}ais")),
        (
            3,
            1,
            0x0409,
            13,
            utf16("This Font Software is licensed\nunder the OFL"),
        ),
        (3, 1, 0x0409, 14, utf16("https://openfontlicense.org")),
    ]);
    assert_eq!(
        name_record(&face, 13).as_deref(),
        Some("This Font Software is licensed under the OFL")
    );
    assert_eq!(
        name_record(&face, 14).as_deref(),
        Some("https://openfontlicense.org")
    );
    assert_eq!(name_record(&face, 0), None, "no record for the ID");

    let face = face_with_names(&[
        (1, 0, 0, 13, b"Mac \xA9 text".to_vec()),
        (3, 1, 0x040C, 13, utf16("en fran\u{E7}ais")),
    ]);
    assert_eq!(name_record(&face, 13).as_deref(), Some("en fran\u{E7}ais"));
    let face = face_with_names(&[(1, 0, 0, 13, b"Mac \xA9 text".to_vec())]);
    assert_eq!(
        name_record(&face, 13).as_deref(),
        Some("Mac \u{A9} text"),
        "0xA9 is the copyright sign in Mac Roman"
    );
    let face = face_with_names(&[(0, 3, 0, 13, utf16("Unicode"))]);
    assert_eq!(name_record(&face, 13).as_deref(), Some("Unicode"));
    let face = face_with_names(&[(2, 0, 0, 13, b"ISO".to_vec())]);
    assert_eq!(name_record(&face, 13), None, "an ISO record is not read");

    let mut cut = face_with_names(&[(3, 1, 0x0409, 13, utf16("a long licence text"))]);
    cut.truncate(cut.len() - 4);
    assert_eq!(name_record(&cut, 13), None);
    assert_eq!(name_record(b"not a font", 13), None);
}

/// The candidate list is the design's: every file and SPDX expression the
/// census reads is in `symbol-faces.md`'s census table, and every file that
/// table names is a candidate.
#[test]
fn every_candidate_is_the_designs() {
    let doc =
        std::fs::read_to_string(repo_file("docs/design/symbol-faces.md")).expect("the design");
    let section = doc
        .split("### The census")
        .nth(1)
        .and_then(|rest| rest.split("\n### ").next())
        .expect("the census section");
    let mut named = BTreeSet::new();
    for row in section.lines().filter(|line| line.starts_with("| `")) {
        let files = row.split('|').nth(1).unwrap_or("");
        for file in files.split('`').skip(1).step_by(2) {
            named.insert(file.to_string());
        }
        let spdx = row.split('|').nth(5).unwrap_or("");
        let candidates: Vec<&Candidate> = CANDIDATES
            .iter()
            .filter(|candidate| files.contains(&format!("`{}`", candidate.file)))
            .collect();
        assert!(!candidates.is_empty(), "{row}");
        for candidate in candidates {
            assert!(
                spdx.contains(&format!("`{}`", candidate.spdx)),
                "{} is {} in the design: {row}",
                candidate.file,
                candidate.spdx
            );
        }
    }
    let read: BTreeSet<String> = CANDIDATES
        .iter()
        .map(|candidate| candidate.file.to_string())
        .collect();
    assert_eq!(named, read, "the design's files and the census's");
}

/// A `post` version 2.0 table naming each glyph by a string of its own.
fn post_naming(names: &[&str]) -> Vec<u8> {
    let mut out = 0x0002_0000u32.to_be_bytes().to_vec();
    out.extend_from_slice(&[0; 28]);
    out.extend_from_slice(&((names.len() + 1) as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // glyph 0, .notdef
    for i in 0..names.len() {
        out.extend_from_slice(&((258 + i) as u16).to_be_bytes());
    }
    for name in names {
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
    }
    out
}

/// A format 12 subtable mapping each character to its glyph, a group each.
fn format_12(pairs: &[(u32, u16)]) -> Vec<u8> {
    let mut pairs = pairs.to_vec();
    pairs.sort_unstable();
    let mut out = Vec::new();
    out.extend_from_slice(&12u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&((16 + 12 * pairs.len()) as u32).to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&(pairs.len() as u32).to_be_bytes());
    for (code, glyph) in pairs {
        out.extend_from_slice(&code.to_be_bytes());
        out.extend_from_slice(&code.to_be_bytes());
        out.extend_from_slice(&u32::from(glyph).to_be_bytes());
    }
    out
}

/// A face of `glyf`, `post` and a `cmap` of a (3,10) and a (3,0) subtable.
fn face(post: Vec<u8>, unicode: &[(u32, u16)], symbol: &[(u32, u16)]) -> Vec<u8> {
    let subtables = [(3u16, 10u16, format_12(unicode)), (3, 0, format_12(symbol))];
    let mut cmap = Vec::new();
    cmap.extend_from_slice(&0u16.to_be_bytes());
    cmap.extend_from_slice(&2u16.to_be_bytes());
    let mut offset = 4 + 8 * subtables.len();
    for (platform, encoding, sub) in &subtables {
        cmap.extend_from_slice(&platform.to_be_bytes());
        cmap.extend_from_slice(&encoding.to_be_bytes());
        cmap.extend_from_slice(&(offset as u32).to_be_bytes());
        offset += sub.len();
    }
    for (.., sub) in &subtables {
        cmap.extend_from_slice(sub);
    }
    let tables: [(&[u8; 4], Vec<u8>); 3] =
        [(b"cmap", cmap), (b"glyf", Vec::new()), (b"post", post)];
    let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * tables.len();
    for (tag, table) in &tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(table.len() as u32).to_be_bytes());
        offset += table.len();
    }
    for (_, table) in &tables {
        out.extend_from_slice(table);
    }
    out
}

/// The census counts each code by the step the route took: a face built here
/// names all but ten of Symbol's codes, maps five of those ten by their Glyph
/// List value and three more at 0xF000, and leaves two, which it lists as
/// missed, in code order.
#[test]
fn the_census_counts_each_code_by_its_step() {
    let codes = BuiltInEncoding::Symbol.codes();
    let (left, named) = codes.split_at(10);
    let names: Vec<&str> = named.iter().map(|entry| entry.name).collect();
    let first = u16::try_from(names.len() + 1).expect("few glyphs");
    let unicode: Vec<(u32, u16)> = left[..5]
        .iter()
        .zip(first..)
        .map(|(entry, glyph)| (u32::from(entry.unicode), glyph))
        .collect();
    let symbol: Vec<(u32, u16)> = left[5..8]
        .iter()
        .zip(first + 5..)
        .map(|(entry, glyph)| (0xF000 | u32::from(entry.code), glyph))
        .collect();
    let program = face(post_naming(&names), &unicode, &symbol);
    let counted = reach(&program, BuiltInEncoding::Symbol);
    assert_eq!(counted.by_name, 179);
    assert_eq!(counted.by_unicode, 5);
    assert_eq!(counted.by_symbol_code, 3);
    assert_eq!(counted.total(), 187);
    assert_eq!(counted.missed, [left[8].name, left[9].name]);
    assert_eq!(counted.reached.len(), 187);

    // The same face read as ZapfDingbats reaches what its (3,0) subtable and
    // the names it shares with Symbol (`space` alone) reach.
    let dingbats = reach(&program, BuiltInEncoding::ZapfDingbats);
    assert_eq!(dingbats.total() + dingbats.missed.len(), 188);
    assert!(dingbats.missed.contains(&"a1"));

    // A program that is no font reaches nothing, and misses every code.
    let nothing = reach(b"not a font", BuiltInEncoding::ZapfDingbats);
    assert_eq!(nothing.total(), 0);
    assert_eq!(nothing.missed.len(), 188);
}
