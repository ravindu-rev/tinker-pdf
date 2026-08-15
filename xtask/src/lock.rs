//! The corpus lock: what a corpus is, where it comes from, and on what terms.
//!
//! Plan 14 is explicit that corpora are **fetched, pinned and never
//! committed** — size aside, redistribution rights are per-file murky in every
//! real-world PDF collection, and a pin plus a checksum reproduces the set
//! without this project becoming a distributor. This file is that pin.
//!
//! Two pins per corpus, and they answer different questions. `commit` is the
//! *content* pin: an upstream that rewrites history or moves a branch cannot
//! change what a commit hash names. `sha256` is the *byte* pin: it is what
//! makes a fetch verifiable at all, and it is the one a tampered mirror
//! fails. They are both here because a forge's archive of a fixed commit is
//! not guaranteed byte-stable forever — its compressor may change — and when
//! that happens the two pins disagree in a way that says exactly which kind of
//! change it was. Re-recording a `sha256` is then a deliberate act with its own
//! commit, which is what plan 23's risk table asks for.
//!
//! The format is `key = value` under `[name]` headers, read line by line for
//! the same reason `xtask` reads manifests that way: no dependency may be
//! added to check the rules about dependencies.

use std::path::{Path, PathBuf};

/// One corpus, as the lock file names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Corpus {
    /// The short name, used as the directory under `corpus/`, on the command
    /// line, and as the key in `ratchet.json`.
    pub name: String,
    /// Where the archive is fetched from.
    pub url: String,
    /// The upstream commit the archive is of: the content pin.
    pub commit: String,
    /// The archive's SHA-256, lowercase hex: the byte pin.
    pub sha256: String,
    /// The path *inside* the extracted archive holding the PDFs. Empty means
    /// the whole tree.
    pub subdir: String,
    /// An SPDX identifier where upstream has one, or a phrase where it does
    /// not. Several of these corpora have no single answer, which is itself
    /// the reason nothing here is redistributed.
    pub licence: String,
    /// Whether this project may redistribute the files. Every current entry
    /// is `false`; the field exists so that a future one which is not has to
    /// say so out loud rather than by omission.
    pub redistribute: bool,
    /// One sentence on what the corpus exercises, for the licence table.
    pub exercises: String,
}

const REQUIRED: &[&str] = &[
    "url",
    "commit",
    "sha256",
    "licence",
    "redistribute",
    "exercises",
];

/// Parses a lock file, refusing anything it cannot fully account for.
///
/// Refusing rather than skipping is the whole point. A lock that silently
/// dropped a corpus whose key was misspelled would produce a run over three
/// corpora that reads exactly like a run over four, and the ratchet would
/// happily accept the smaller number.
pub fn parse(text: &str) -> Result<Vec<Corpus>, String> {
    let mut out: Vec<Corpus> = Vec::new();
    let mut current: Option<(String, Vec<(String, String)>)> = None;

    for (index, raw) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw.split('#').next().unwrap_or(raw).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix('[') {
            let Some(name) = rest.strip_suffix(']') else {
                return Err(format!(
                    "line {line_number}: `{line}` opens a corpus and never closes it"
                ));
            };
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(format!("line {line_number}: a corpus needs a name"));
            }
            if !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return Err(format!(
                    "line {line_number}: `{name}` becomes a directory name and a \
                     ratchet key, so it is limited to letters, digits, `-` and `_`"
                ));
            }
            if let Some(finished) = current.take() {
                out.push(build(finished)?);
            }
            if out.iter().any(|c| c.name == name) {
                return Err(format!(
                    "line {line_number}: `{name}` is declared twice; one of them \
                     would silently win"
                ));
            }
            current = Some((name, Vec::new()));
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {line_number}: `{line}` is neither a `[corpus]` header nor a \
                 `key = value`"
            ));
        };
        let (key, value) = (key.trim().to_string(), value.trim().to_string());

        let Some((_, fields)) = current.as_mut() else {
            return Err(format!(
                "line {line_number}: `{key}` appears before any `[corpus]` header, \
                 so nothing owns it"
            ));
        };
        if fields.iter().any(|(k, _)| *k == key) {
            return Err(format!(
                "line {line_number}: `{key}` is given twice in the same corpus"
            ));
        }
        fields.push((key, value));
    }

    if let Some(finished) = current.take() {
        out.push(build(finished)?);
    }
    if out.is_empty() {
        return Err("the lock declares no corpora at all".to_string());
    }
    Ok(out)
}

fn build((name, fields): (String, Vec<(String, String)>)) -> Result<Corpus, String> {
    let find = |key: &str| -> Option<String> {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };

    for key in REQUIRED {
        if find(key).is_none() {
            return Err(format!("[{name}]: `{key}` is missing"));
        }
    }
    for (key, _) in &fields {
        if !REQUIRED.contains(&key.as_str()) && key != "subdir" {
            return Err(format!(
                "[{name}]: `{key}` is not a lock field — the known ones are {}, \
                 and `subdir`",
                REQUIRED.join(", ")
            ));
        }
    }

    let sha256 = find("sha256").unwrap_or_default();
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "[{name}]: `sha256 = {sha256}` is not a 64-character hex digest, so \
             nothing could ever verify against it"
        ));
    }
    if sha256.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(format!(
            "[{name}]: the digest is compared as text, so it is written in \
             lowercase hex"
        ));
    }

    let commit = find("commit").unwrap_or_default();
    if commit.len() != 40 || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "[{name}]: `commit = {commit}` is not a 40-character git object id; \
             a branch or a tag is not a pin, because upstream can move it"
        ));
    }

    let url = find("url").unwrap_or_default();
    if !url.starts_with("https://") {
        return Err(format!(
            "[{name}]: `{url}` is not https, and an unauthenticated fetch of a \
             corpus is a supply chain nobody is watching"
        ));
    }

    let redistribute = match find("redistribute").unwrap_or_default().as_str() {
        "yes" => true,
        "no" => false,
        other => {
            return Err(format!(
                "[{name}]: `redistribute = {other}` must be `yes` or `no`; this \
                 decides whether the files may leave the fetch cache"
            ))
        }
    };

    let licence = find("licence").unwrap_or_default();
    if licence.is_empty() {
        return Err(format!(
            "[{name}]: `licence` is empty — plan 14 wants a table, and a blank \
             row is worse than no row"
        ));
    }

    Ok(Corpus {
        name,
        url,
        commit,
        sha256,
        subdir: find("subdir").unwrap_or_default(),
        licence,
        redistribute,
        exercises: find("exercises").unwrap_or_default(),
    })
}

/// The default location of the lock, relative to the repository root.
pub const LOCK_PATH: &str = "corpus/corpora.lock";

/// Where a corpus's archive is cached. Ignored by git, per plan 14.
pub fn archive_path(root: &Path, corpus: &Corpus) -> PathBuf {
    root.join("corpus/cache")
        .join(format!("{}.tar.gz", corpus.name))
}

/// Where a corpus's files begin once extracted, including its `subdir`.
pub fn files_dir(root: &Path, corpus: &Corpus) -> PathBuf {
    let base = root.join("corpus/files").join(&corpus.name);
    if corpus.subdir.is_empty() {
        base
    } else {
        base.join(&corpus.subdir)
    }
}

pub fn read(root: &Path) -> Result<Vec<Corpus>, String> {
    let path = root.join(LOCK_PATH);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// The licence table plan 14 asks for, rendered from the lock so it cannot
/// drift away from what is actually fetched.
pub fn licence_table(corpora: &[Corpus]) -> String {
    let mut out = String::from(
        "| Corpus | What it exercises | Upstream licence | Redistributed here? |\n\
         | --- | --- | --- | --- |\n",
    );
    for corpus in corpora {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            corpus.name,
            corpus.exercises,
            corpus.licence,
            if corpus.redistribute {
                "yes"
            } else {
                "**no** — fetched, never committed"
            }
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "\
# a comment
[pdfjs]
url = https://codeload.github.com/mozilla/pdf.js/tar.gz/b96d745c277a5666013359a7272d74d1896fbb9f
commit = b96d745c277a5666013359a7272d74d1896fbb9f
sha256 = 0000000000000000000000000000000000000000000000000000000000000001
subdir = pdf.js-b96d745c/test/pdfs
licence = Mixed
redistribute = no
exercises = decades of real-world breakage
";

    #[test]
    fn a_well_formed_lock_reads() {
        let corpora = parse(GOOD).expect("it parses");
        assert_eq!(corpora.len(), 1);
        assert_eq!(corpora[0].name, "pdfjs");
        assert!(!corpora[0].redistribute);
        assert_eq!(corpora[0].subdir, "pdf.js-b96d745c/test/pdfs");
    }

    /// Every rejection names the line and says what to do about it. A lock
    /// that failed with `parse error` would send whoever hit it to read the
    /// parser rather than their own file.
    #[test]
    fn every_malformation_is_rejected_with_a_useful_message() {
        let cases: &[(&str, &str)] = &[
            (
                "url = https://example.invalid/x\n",
                "before any `[corpus]` header",
            ),
            ("[pdfjs\nurl = https://x/\n", "never closes it"),
            ("[]\n", "needs a name"),
            ("[a b]\n", "letters, digits"),
            ("[pdfjs]\nnonsense\n", "neither a `[corpus]` header"),
            (
                &GOOD.replace(
                    "sha256 = 0000000000000000000000000000000000000000000000000000000000000001",
                    "sha256 = deadbeef",
                ),
                "not a 64-character hex digest",
            ),
            (
                &GOOD.replace(
                    "commit = b96d745c277a5666013359a7272d74d1896fbb9f",
                    "commit = master",
                ),
                "not a 40-character git object id",
            ),
            (
                &GOOD.replace("url = https://", "url = http://"),
                "is not https",
            ),
            (
                &GOOD.replace("redistribute = no", "redistribute = maybe"),
                "must be `yes` or `no`",
            ),
            (&GOOD.replace("licence = Mixed", "licence ="), "is empty"),
            (
                &GOOD.replace("commit = ", "commitish = "),
                "`commit` is missing",
            ),
            (
                &GOOD.replace("subdir = ", "sudbir = "),
                "is not a lock field",
            ),
            (&format!("{GOOD}{GOOD}"), "declared twice"),
            (
                &GOOD.replace("subdir = pdf", "url = https://x/\nsubdir = pdf"),
                "given twice in the same corpus",
            ),
            ("", "declares no corpora at all"),
        ];

        for (text, expected) in cases {
            let error = parse(text).expect_err(&format!("`{text}` must be rejected"));
            assert!(
                error.contains(expected),
                "the message for `{text}` should mention `{expected}`, and says: {error}"
            );
        }
    }

    /// An uppercase digest verifies against nothing, because the comparison is
    /// textual. Rejecting it is kinder than a fetch that always fails.
    #[test]
    fn an_uppercase_digest_is_refused_rather_than_silently_never_matching() {
        let text = GOOD.replace(
            "sha256 = 0000000000000000000000000000000000000000000000000000000000000001",
            "sha256 = 000000000000000000000000000000000000000000000000000000000000000A",
        );
        let error = parse(&text).expect_err("it is refused");
        assert!(error.contains("lowercase"), "{error}");
    }

    /// The table is generated, so a corpus cannot be fetched without its
    /// licence being stated somewhere a person reads.
    #[test]
    fn the_licence_table_names_every_corpus() {
        let corpora = parse(GOOD).expect("it parses");
        let table = licence_table(&corpora);
        assert!(table.contains("| `pdfjs` |"), "{table}");
        assert!(table.contains("Mixed"), "{table}");
        assert!(table.contains("never committed"), "{table}");
    }

    /// The lock in this repository is the one that has to parse.
    #[test]
    fn this_repositorys_lock_is_well_formed() {
        let root = crate::repo_root();
        let corpora = read(&root).expect("corpus/corpora.lock parses");
        assert!(
            corpora.len() >= 4,
            "the four corpora plan 23 names: {corpora:#?}"
        );
        for corpus in &corpora {
            assert!(
                !corpus.redistribute,
                "{} claims redistribution rights; plan 14 says nothing from any \
                 corpus enters git, so that claim needs its own review",
                corpus.name
            );
        }
    }

    /// Plan 14 wants a licence table and `corpus/README.md` is where a person
    /// looks for it. Generated above and committed there, so this checks the
    /// committed copy still matches the lock.
    #[test]
    fn the_committed_licence_table_matches_the_lock() {
        let root = crate::repo_root();
        let corpora = read(&root).expect("the lock parses");
        let readme = std::fs::read_to_string(root.join("corpus/README.md"))
            .expect("corpus/README.md exists");
        for line in licence_table(&corpora).lines() {
            assert!(
                readme.contains(line),
                "corpus/README.md has drifted from the lock; it is missing:\n{line}\n\
                 Regenerate with `cargo xtask corpus-licences`."
            );
        }
    }
}
