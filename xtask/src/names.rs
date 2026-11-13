//! The naming lint (K-09): the roadmap names no engine, viewer, validator or
//! document tool.
//!
//! `docs/ROADMAP.md` says so of itself, and ruling 23 records the owner's
//! answer to how far the rule reaches (H-07): the roadmap names none of them,
//! each corpus is named by its role, and the provenance files —
//! `corpus/corpora.lock`, `corpus/README.md`, `THIRDPARTY.md` — keep the
//! names, because recording provenance is their job. So this reads the
//! roadmap, `docs/ROADMAP.md` and every file under `docs/roadmap/`, and
//! nothing else.
//!
//! # The names are not written down
//!
//! A list of names to keep out of a file is itself a file of those names, and
//! committing it would put every one of them in the tree. So the list,
//! `xtask/never-named.sha256`, holds the SHA-256 of each name instead, one a
//! line, in the form [`words`] gives a text: lowercased, a two-word name as
//! its two words joined by one space. The scan splits a file on every
//! character that is not a letter or a digit — line breaks included, so two
//! words a wrap separates are still a pair — and hashes every word and every
//! pair of adjacent words. A match fails with the file, the line and the
//! hash, and never the word, so that the failure does not write the name
//! either. There is no exemption mechanism: a name the roadmap needs is said
//! by its role.
//!
//! That stops a grep and not a determined reader. A hash of a short public
//! name is reversed by hashing candidate names, so the list keeps the names
//! out of the roadmap and claims nothing about keeping itself secret.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tinker_pdf_crypto::sha2::sha256;

/// The committed list of hashes, from the repository root.
pub const LIST: &str = "xtask/never-named.sha256";

/// The roadmap's index, from the repository root.
pub const ROADMAP: &str = "docs/ROADMAP.md";

/// The directory of the roadmap's step files, from the repository root.
pub const STEPS: &str = "docs/roadmap";

/// One word of a text, lowercased, with the line it is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    /// The word, lowercased.
    pub text: String,
    /// Its line, counted from 1.
    pub line: usize,
}

/// The words of `text`: each maximal run of letters and digits, lowercased,
/// in order, with its line.
///
/// A letter or a digit is what [`char::is_alphanumeric`] says one is, so a
/// name in another script splits the same way a Latin one does. Every other
/// character ends a word — punctuation, white space, a line break — so
/// `Name-Two`, `name_two` and `name\ntwo` all give the two words `name` and
/// `two`.
#[must_use]
pub fn words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut start = 1;
    let mut line = 1;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if current.is_empty() {
                start = line;
            }
            current.extend(c.to_lowercase());
            continue;
        }
        if !current.is_empty() {
            out.push(Word {
                text: std::mem::take(&mut current),
                line: start,
            });
        }
        if c == '\n' {
            line += 1;
        }
    }
    if !current.is_empty() {
        out.push(Word {
            text: current,
            line: start,
        });
    }
    out
}

/// The SHA-256 of `text`'s UTF-8 bytes, as 64 lowercase hex digits.
#[must_use]
pub fn digest(text: &str) -> String {
    let mut out = String::with_capacity(64);
    for byte in sha256(text.as_bytes()) {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Whether `line` is one hash as the list states one: 64 lowercase hex
/// digits and nothing else.
#[must_use]
pub fn is_hash(line: &str) -> bool {
    line.len() == 64
        && line
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The list's hashes, or a problem for each line that is not one.
///
/// A line that is not a hash can never match, so the name it was meant to
/// hold would go unchecked while the list looked whole: that is a failure,
/// not a line to pass over. An empty line is one too.
pub fn read_list(text: &str) -> Result<BTreeSet<String>, Vec<String>> {
    let mut hashes = BTreeSet::new();
    let mut problems = Vec::new();
    for (number, line) in text.lines().enumerate() {
        if is_hash(line) {
            hashes.insert(line.to_string());
        } else {
            problems.push(format!(
                "{LIST}:{}: not a hash: each line is 64 lowercase hex digits and nothing else",
                number + 1
            ));
        }
    }
    if problems.is_empty() {
        Ok(hashes)
    } else {
        Err(problems)
    }
}

/// Every place in `text` where a word or a pair of adjacent words hashes to
/// one of `list`'s, as `(line, hash)` in line order: a pair is placed on the
/// line of its first word.
///
/// Each distinct word and pair is hashed once, however often it occurs, and
/// every occurrence of a match is reported.
#[must_use]
pub fn scan(text: &str, list: &BTreeSet<String>) -> Vec<(usize, String)> {
    let words = words(text);
    let mut places: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, word) in words.iter().enumerate() {
        places.entry(word.text.clone()).or_default().push(word.line);
        if let Some(next) = words.get(index + 1) {
            places
                .entry(format!("{} {}", word.text, next.text))
                .or_default()
                .push(word.line);
        }
    }
    let mut hits = Vec::new();
    for (token, lines) in &places {
        let hash = digest(token);
        if list.contains(&hash) {
            hits.extend(lines.iter().map(|line| (*line, hash.clone())));
        }
    }
    hits.sort();
    hits
}

/// [`scan`]'s hits in `text` as the problems `cargo xtask names` prints,
/// `file` being the path the text was read from.
#[must_use]
pub fn findings(file: &str, text: &str, list: &BTreeSet<String>) -> Vec<String> {
    scan(text, list)
        .into_iter()
        .map(|(line, hash)| {
            format!(
                "{file}:{line}: a word or word pair hashes to {hash}, which {LIST} lists: \
                 the roadmap names no engine, viewer, validator or document tool \
                 (ruling 23, H-07), so say it by its role"
            )
        })
        .collect()
}

/// The files the lint reads: `docs/ROADMAP.md` and every file under
/// `docs/roadmap/`, at any depth, in path order.
fn roadmap_files(root: &Path) -> Result<Vec<PathBuf>, Vec<String>> {
    let mut files = vec![root.join(ROADMAP)];
    let mut pending = vec![root.join(STEPS)];
    let mut found = Vec::new();
    while let Some(dir) = pending.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| vec![format!("{}: cannot be listed ({e})", shown(root, &dir))])?;
        for entry in entries {
            let path = entry
                .map_err(|e| vec![format!("{}: cannot be listed ({e})", shown(root, &dir))])?
                .path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(path);
            }
        }
    }
    if found.is_empty() {
        // A scan of nothing passes, and would go on passing after the step
        // files moved: that is a check that does not run, not a clean one.
        return Err(vec![format!(
            "{STEPS}: holds no file, so the step files were not scanned"
        )]);
    }
    found.sort();
    files.extend(found);
    Ok(files)
}

/// A path as this repository names it: from the root, with forward slashes.
fn shown(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// `cargo xtask names` — no word or word pair of the roadmap hashes to one
/// of the list's.
pub fn check(root: &Path) -> Result<(), Vec<String>> {
    let text = std::fs::read_to_string(root.join(LIST)).map_err(|e| {
        vec![format!(
            "{LIST}: cannot be read ({e}), so nothing was checked"
        )]
    })?;
    let list = read_list(&text)?;
    let mut problems = Vec::new();
    for file in roadmap_files(root)? {
        let name = shown(root, &file);
        match std::fs::read_to_string(&file) {
            Ok(text) => problems.extend(findings(&name, &text, &list)),
            // Never a silent skip: a file this cannot read is one it cannot
            // vouch for.
            Err(e) => problems.push(format!(
                "{name}: cannot be read ({e}), so it was not scanned"
            )),
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list holding the hashes of `names`, as the committed one holds its
    /// own. The names are invented, so no real one is written here.
    fn list_of(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| digest(name)).collect()
    }

    #[test]
    fn the_digest_is_sha256_in_lowercase_hex() {
        // FIPS 180-4's "abc" example.
        assert_eq!(
            digest("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(is_hash(&digest("")));
    }

    #[test]
    fn an_injected_hashed_name_is_caught() {
        let text = "A first line.\nThe second names Quazzleware here.\nA third.\n";
        let list = list_of(&["quazzleware"]);
        assert_eq!(scan(text, &list), vec![(2, digest("quazzleware"))]);

        let found = findings("docs/roadmap/X.md", text, &list);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("docs/roadmap/X.md:2: "),
            "{}",
            found[0]
        );
        assert!(found[0].contains(&digest("quazzleware")), "{}", found[0]);
        assert!(
            !found[0].to_lowercase().contains("quazzleware"),
            "the failure writes the name it exists to keep out: {}",
            found[0]
        );
    }

    #[test]
    fn every_occurrence_is_reported_on_its_own_line() {
        let text = "quazzleware\nnothing\nmore quazzleware, and quazzleware\n";
        let list = list_of(&["quazzleware"]);
        let lines: Vec<usize> = scan(text, &list)
            .into_iter()
            .map(|(line, _)| line)
            .collect();
        assert_eq!(lines, vec![1, 3, 3]);
    }

    #[test]
    fn a_hashed_word_pair_is_caught() {
        let text = "Nothing here.\nIt reads like the Frumble Plex viewer does.\n";
        let list = list_of(&["frumble plex"]);
        assert_eq!(scan(text, &list), vec![(2, digest("frumble plex"))]);
        // Neither word alone is on the list, so neither alone is caught.
        assert!(scan("frumble, and later plex", &list).is_empty());
    }

    #[test]
    fn a_pair_split_across_a_line_break_is_caught() {
        let text = "a table row that wraps after frumble\nplex and goes on\n";
        let list = list_of(&["frumble plex"]);
        assert_eq!(scan(text, &list), vec![(1, digest("frumble plex"))]);
        // A line break between a table's cells, or a list item's, is the
        // same: the two are adjacent words of the file.
        let text = "| frumble |\n| plex |\n";
        assert_eq!(scan(text, &list), vec![(1, digest("frumble plex"))]);
    }

    #[test]
    fn case_and_punctuation_do_not_hide_a_name() {
        let word = list_of(&["quazzleware"]);
        for text in [
            "QUAZZLEWARE",
            "Quazzleware's reader",
            "(quazzleware)",
            "`quazzleware`",
            "**Quazzleware**:",
            "quazzleware.",
            "[quazzleware](x)",
        ] {
            assert_eq!(scan(text, &word).len(), 1, "{text:?} hid the word");
        }
        let pair = list_of(&["frumble plex"]);
        for text in [
            "Frumble-Plex",
            "frumble_plex",
            "frumble/plex",
            "FRUMBLE PLEX",
            "frumble — plex",
            "`frumble` **plex**",
            "frumble,\n  plex",
        ] {
            assert_eq!(scan(text, &pair).len(), 1, "{text:?} hid the pair");
        }
    }

    #[test]
    fn a_name_inside_a_longer_word_is_not_that_name() {
        // Words are whole runs of letters and digits: a name that is the
        // middle of another word is not written there.
        let list = list_of(&["plex"]);
        assert!(scan("a perplexing complexity", &list).is_empty());
    }

    #[test]
    fn a_clean_file_passes() {
        let text = "Every page renders the way the world renders it.\n";
        assert!(scan(text, &list_of(&["quazzleware", "frumble plex"])).is_empty());
        assert!(scan(text, &BTreeSet::new()).is_empty());
        assert!(scan("", &list_of(&["quazzleware"])).is_empty());
    }

    #[test]
    fn words_are_lowercased_runs_of_letters_and_digits_with_their_lines() {
        let found = words("Ab-c d2\n\n  ÉTÉ x_y");
        let as_pairs: Vec<(&str, usize)> =
            found.iter().map(|w| (w.text.as_str(), w.line)).collect();
        assert_eq!(
            as_pairs,
            vec![
                ("ab", 1),
                ("c", 1),
                ("d2", 1),
                ("été", 3),
                ("x", 3),
                ("y", 3)
            ]
        );
    }

    #[test]
    fn a_line_that_is_not_a_hash_fails_the_list() {
        let good = digest("quazzleware");
        assert_eq!(read_list(&format!("{good}\n")).map(|l| l.len()), Ok(1));
        assert_eq!(read_list("").map(|l| l.len()), Ok(0));
        for bad in [
            good.to_uppercase(),
            format!("{good} "),
            good[..63].to_string(),
            format!("# {good}"),
            String::new(),
        ] {
            let text = format!("{good}\n{bad}\n");
            let problems = read_list(&text).expect_err(&bad);
            assert_eq!(problems.len(), 1, "{problems:?}");
            assert!(
                problems[0].starts_with(&format!("{LIST}:2: ")),
                "{problems:?}"
            );
        }
    }

    /// The lint runs against this repository, which is the point of it.
    #[test]
    fn the_roadmap_names_nothing_the_list_holds() {
        if let Err(problems) = check(&crate::repo_root()) {
            panic!("the roadmap names what it says it does not:\n{problems:#?}");
        }
    }
}
