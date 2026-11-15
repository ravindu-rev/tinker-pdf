//! Every property this crate answers from a table, at every code point, held
//! to the vendored file it was compiled from — read here, by this file's own
//! parser, not by `build.rs`'s.
//!
//! `build.rs` writes each table as runs and `unicode.rs` decodes them at first
//! use, the smallest form after gzip, which is what a wasm build ships. A form
//! is a place for an off-by-one to hide — a run one code point short, a value
//! index one along, the last run of a set dropped — and the conformance files
//! reach only the code points their cases name. So each property is asked of
//! all 1 114 112 code points here, the surrogates included, and compared with
//! what its file says.

use tinker_pdf_unicode::unicode::{
    east_asian_width, grapheme_break, indic_conjunct_break, is_case_ignorable, is_cased,
    is_combining, is_extended_pictographic, is_final_punctuation, is_initial_punctuation,
    is_letter_or_number, is_lowercase, is_mark, is_punctuation_or_symbol, is_unassigned,
    line_break_class,
};

const CODE_POINTS: usize = 0x11_0000;

/// One vendored file's rows, `(first, last, fields after the code points)`.
fn rows(name: &str) -> Vec<(usize, usize, Vec<String>)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("data/ucd")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut out = Vec::new();
    for line in text.lines() {
        let data = line.split('#').next().unwrap_or("");
        let fields: Vec<&str> = data.split(';').map(str::trim).collect();
        if fields.len() < 2 || fields[0].is_empty() {
            continue;
        }
        let (first, last) = match fields[0].split_once("..") {
            Some((a, b)) => (a, b),
            None => (fields[0], fields[0]),
        };
        let code = |hex: &str| usize::from_str_radix(hex, 16).expect("a code point");
        out.push((
            code(first),
            code(last),
            fields[1..].iter().map(|f| (*f).to_owned()).collect(),
        ));
    }
    assert!(!out.is_empty(), "{name} has no rows");
    out
}

/// Each code point's value of an enumerated property, `default` where no row
/// names one; `pick` reads a row's value, `None` for a row of another
/// property.
fn values(name: &str, default: &str, pick: impl Fn(&[String]) -> Option<String>) -> Vec<String> {
    let mut out = vec![default.to_owned(); CODE_POINTS];
    for (first, last, fields) in rows(name) {
        if let Some(value) = pick(&fields) {
            for slot in &mut out[first..=last] {
                slot.clone_from(&value);
            }
        }
    }
    out
}

/// Each code point's membership of a binary property.
fn members(name: &str, wanted: impl Fn(&[String]) -> bool) -> Vec<bool> {
    let mut out = vec![false; CODE_POINTS];
    for (first, last, fields) in rows(name) {
        if wanted(&fields) {
            for slot in &mut out[first..=last] {
                *slot = true;
            }
        }
    }
    out
}

/// Every code point that is a `char`, with its index.
fn characters() -> impl Iterator<Item = (usize, char)> {
    (0..CODE_POINTS).filter_map(|code| char::from_u32(code as u32).map(|c| (code, c)))
}

/// The first code points where `ours` and `file` disagree, at most ten.
fn disagreements<T: PartialEq + std::fmt::Debug>(
    what: &str,
    ours: impl Fn(char) -> T,
    file: impl Fn(usize) -> T,
) -> Vec<String> {
    characters()
        .filter(|&(code, c)| ours(c) != file(code))
        .take(10)
        .map(|(code, c)| {
            format!(
                "{what} U+{code:04X}: ours {:?}, the file's {:?}",
                ours(c),
                file(code)
            )
        })
        .collect()
}

/// **The enumerated properties are their files'**, every code point:
/// `Line_Break` (unlisted `XX`), `East_Asian_Width` (unlisted `N`),
/// `Grapheme_Cluster_Break` (unlisted `Other`, `Regional_Indicator` as `RI`)
/// and `Indic_Conjunct_Break` (unlisted none), each variant named as its file
/// names the value.
#[test]
fn every_enumerated_property_is_its_files() {
    let first = |fields: &[String]| fields.first().cloned();
    let line_break = values("LineBreak.txt", "XX", first);
    let east_asian = values("EastAsianWidth.txt", "N", first);
    let grapheme = values("GraphemeBreakProperty.txt", "Other", |fields| {
        fields.first().map(|value| {
            if value == "Regional_Indicator" {
                "RI".to_owned()
            } else {
                value.clone()
            }
        })
    });
    let conjunct = values("DerivedCoreProperties.txt", "None", |fields| {
        (fields.first().map(String::as_str) == Some("InCB"))
            .then(|| fields.get(1).cloned())
            .flatten()
    });
    let mut wrong = Vec::new();
    wrong.extend(disagreements(
        "Line_Break",
        |c| format!("{:?}", line_break_class(c)),
        |code| line_break[code].clone(),
    ));
    wrong.extend(disagreements(
        "East_Asian_Width",
        |c| format!("{:?}", east_asian_width(c)),
        |code| east_asian[code].clone(),
    ));
    wrong.extend(disagreements(
        "Grapheme_Cluster_Break",
        |c| format!("{:?}", grapheme_break(c)),
        |code| grapheme[code].clone(),
    ));
    wrong.extend(disagreements(
        "Indic_Conjunct_Break",
        |c| indic_conjunct_break(c).map_or_else(|| "None".to_owned(), |v| format!("{v:?}")),
        |code| conjunct[code].clone(),
    ));
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// **The binary properties are their files'**, every code point: the general
/// categories the tables are cut from (`Cn`, `Mn`/`Mc`, `Mn`/`Mc`/`Me`, `Pi`,
/// `Pf`, `P*`/`S*`, `L*`/`N*`), `Extended_Pictographic`, and `Cased`,
/// `Case_Ignorable` and `Lowercase`.
#[test]
fn every_binary_property_is_its_files() {
    let category = values("DerivedGeneralCategory.txt", "Cn", |fields| {
        fields.first().cloned()
    });
    let core = |property: &'static str| {
        members("DerivedCoreProperties.txt", move |fields| {
            fields.first().map(String::as_str) == Some(property) && fields.len() == 1
        })
    };
    let pictographic = members("emoji-data.txt", |fields| {
        fields.first().map(String::as_str) == Some("Extended_Pictographic")
    });
    let (cased, ignorable, lowercase) = (core("Cased"), core("Case_Ignorable"), core("Lowercase"));
    let is = |code: usize, wanted: &[&str]| wanted.contains(&category[code].as_str());
    let starts = |code: usize, letters: &str| category[code].starts_with(|c| letters.contains(c));
    let mut wrong = Vec::new();
    wrong.extend(disagreements("Cn", is_unassigned, |code| is(code, &["Cn"])));
    wrong.extend(disagreements("Mn or Mc", is_combining, |code| {
        is(code, &["Mn", "Mc"])
    }));
    wrong.extend(disagreements("M", is_mark, |code| {
        is(code, &["Mn", "Mc", "Me"])
    }));
    wrong.extend(disagreements("Pi", is_initial_punctuation, |code| {
        is(code, &["Pi"])
    }));
    wrong.extend(disagreements("Pf", is_final_punctuation, |code| {
        is(code, &["Pf"])
    }));
    wrong.extend(disagreements("P or S", is_punctuation_or_symbol, |code| {
        starts(code, "PS")
    }));
    wrong.extend(disagreements("L or N", is_letter_or_number, |code| {
        starts(code, "LN")
    }));
    wrong.extend(disagreements(
        "Extended_Pictographic",
        is_extended_pictographic,
        |code| pictographic[code],
    ));
    wrong.extend(disagreements("Cased", is_cased, |code| cased[code]));
    wrong.extend(disagreements("Case_Ignorable", is_case_ignorable, |code| {
        ignorable[code]
    }));
    wrong.extend(disagreements("Lowercase", is_lowercase, |code| {
        lowercase[code]
    }));
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
