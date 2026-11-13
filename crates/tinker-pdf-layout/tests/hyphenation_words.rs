//! Each vendored hyphenation dictionary, held to the words it publishes
//! (`docs/design/hyphenation.md`, FT-15's exit).
//!
//! The words are the dictionary authors' own: the `\hyphenation{…}` list each
//! vendored file carries, the only hyphenated words any of them publishes. They
//! are read **here**, from the vendored file, by a reader of this test's own —
//! a comment cut at `%`, the whitespace-separated words of every
//! `\hyphenation` block — and not by the crate's, so that a misreading in
//! `hyphenate/compile.rs` cannot agree with itself. Each word must come back
//! from the compiled table with exactly its published points, less those its
//! dictionary's minimums forbid: the minimums are the dictionary's rule for
//! every word, and the design keeps them for an exception as for a pattern.
//!
//! A shrinking set cannot read as a passing one: the eight languages are named
//! below, and each prints a `RAN` line with its word count.

use std::path::PathBuf;

use tinker_pdf_layout::hyphenate;

/// The vendored dictionaries, as the design lists them: the tag each tree is
/// named by and the language tag its table is found by.
const VENDORED: [(&str, &str); 8] = [
    ("af", "af"),
    ("cu", "cu"),
    ("en-gb", "en-GB"),
    ("ga", "ga"),
    ("nl", "nl"),
    ("pl", "pl"),
    ("pt", "pt"),
    ("sk", "sk"),
];

/// A vendored file, read as the build reads it.
fn vendored(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join(format!("hyphenation-{name}"))
        .join(format!("hyph-{name}.tex"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The entries of every block a command opens, comments cut.
fn entries(text: &str, command: &str) -> Vec<String> {
    let mut code = String::new();
    for line in text.lines() {
        code.push_str(line.split('%').next().unwrap_or(""));
        code.push('\n');
    }
    let mut out = Vec::new();
    let opening = format!("\\{command}{{");
    let mut rest = code.as_str();
    while let Some(at) = rest.find(&opening) {
        let inside = &rest[at + opening.len()..];
        let end = inside.find('}').expect("every block is closed");
        out.extend(inside[..end].split_whitespace().map(str::to_owned));
        rest = &inside[end..];
    }
    out
}

/// A published word's letters, and the byte offset in them of each point it
/// marks that its minimums allow: after `i` of `n` letters, `i ≥ left` and
/// `n − i ≥ right`, a minimum of zero counting as one.
fn published(word: &str, (left, right): (u8, u8)) -> (String, Vec<usize>) {
    let letters: String = word.chars().filter(|&c| c != '-').collect();
    let n = letters.chars().count();
    let left = usize::from(left.max(1));
    let right = usize::from(right.max(1));
    let mut points = Vec::new();
    let (mut before, mut bytes) = (0usize, 0usize);
    for c in word.chars() {
        if c == '-' {
            if before >= left && n - before >= right {
                points.push(bytes);
            }
        } else {
            before += 1;
            bytes += c.len_utf8();
        }
    }
    (letters, points)
}

/// **Each vendored language hyphenates its dictionary's published words with
/// exactly their points**, and each is the table its own language finds.
#[cfg(feature = "hyphenation")]
#[test]
fn each_language_hyphenates_its_dictionarys_published_test_words() {
    let tables = hyphenate::tables();
    let named: Vec<(&str, &str)> = tables
        .iter()
        .map(|table| (table.name, table.language))
        .collect();
    assert_eq!(named, VENDORED, "the vendored set is the design's eight");
    for table in tables {
        let found = hyphenate::dictionary(table.language).expect("its own language");
        assert!(std::ptr::eq(found, table.dictionary), "{}", table.name);
        let words = entries(&vendored(table.name), "hyphenation");
        assert!(!words.is_empty(), "{} publishes no words", table.name);
        for word in &words {
            let (letters, expected) = published(word, table.dictionary.minimums());
            let mut points = Vec::new();
            table.dictionary.points(&letters, &mut points);
            assert_eq!(points, expected, "{}: {word}", table.name);
        }
        println!(
            "RAN hyphenation_words: {} ({}), {} published words",
            table.name,
            table.language,
            words.len()
        );
    }
}

/// **Each compiled table is the size `hyphenation_tables.tsv` records, made
/// of the counts it records**: the patterns and words read here from the
/// vendored file, and the letters, nodes, vectors, longest pattern and bytes
/// the compiled table reports. A row that moves is a table that changed, and
/// the file is where the figure lives (ruling 16).
#[cfg(feature = "hyphenation")]
#[test]
fn the_compiled_tables_are_the_sizes_the_design_records() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("hyphenation_tables.tsv");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut rows: Vec<Vec<&str>> = text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| line.split('\t').collect())
        .collect();
    let header = rows.remove(0);
    assert_eq!(
        header,
        ["tag", "serves", "patterns", "words", "letters", "nodes", "vectors", "longest", "bytes"]
    );
    assert_eq!(
        rows.len(),
        hyphenate::tables().len(),
        "a row for each table"
    );
    for (row, table) in rows.iter().zip(hyphenate::tables()) {
        let source = vendored(table.name);
        let compiled = table.dictionary;
        let actual = [
            table.name.to_owned(),
            table.language.to_owned(),
            entries(&source, "patterns").len().to_string(),
            entries(&source, "hyphenation").len().to_string(),
            compiled.letters().to_string(),
            compiled.nodes().to_string(),
            compiled.vectors().to_string(),
            compiled.longest().to_string(),
            compiled.bytes().to_string(),
        ];
        assert_eq!(
            row.as_slice(),
            actual.as_slice(),
            "{}'s row; the table compiled to {actual:?}",
            table.name
        );
    }
}

/// **A build without the feature carries no table**, so every language finds
/// none and `hyphens: auto` is set as `manual` everywhere.
#[cfg(not(feature = "hyphenation"))]
#[test]
fn a_build_without_the_feature_has_no_table() {
    assert!(hyphenate::tables().is_empty());
    for (_, language) in VENDORED {
        assert!(hyphenate::dictionary(language).is_none(), "{language}");
    }
    // The vendored files are still in the tree the package ships, which is
    // what a consumer who turns the feature on compiles.
    for (name, _) in VENDORED {
        assert!(!entries(&vendored(name), "patterns").is_empty(), "{name}");
    }
    println!("RAN hyphenation_words: no table without the `hyphenation` feature");
}

/// The test's own reader and the published points it derives, on words
/// written here: a hyphen at an edge of the minimums is kept, one inside is
/// not, and the offsets are bytes.
#[test]
fn the_published_points_are_read_as_the_minimums_allow() {
    assert_eq!(
        published("baad-jie-mou-e", (1, 2)),
        ("baadjiemoue".to_owned(), vec![4, 7])
    );
    assert_eq!(
        published("me-tó-da", (2, 3)),
        ("metóda".to_owned(), vec![2])
    );
    assert_eq!(
        entries(
            "% \\hyphenation{not-this}\n\\patterns{1a}\n\\hyphenation{a-b % c-d\n e-f}",
            "hyphenation"
        ),
        ["a-b", "e-f"]
    );
}
