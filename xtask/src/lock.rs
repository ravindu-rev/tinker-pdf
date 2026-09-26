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
//! **One pin, when there is only one thing to pin.** A corpus that is not a
//! git repository has no commit, and `archive = zip` says so. The temptation
//! is to require the field anyway and let somebody write a zero — a pin that
//! pins nothing, in a file whose whole purpose is that a pin means something.
//! So `commit` is required for a forge archive and *refused* for a published
//! object, and that object's `sha256` carries the entire guarantee: if the
//! bytes ever change, every fetch fails loudly and a person decides what
//! happened. That is a weaker position than two pins and it is stated rather
//! than glossed — a `zip` entry's corpus is only as reproducible as its host.
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
    /// The upstream commit the archive is of: the content pin. Empty for an
    /// entry whose upstream is not a git repository — see [`Archive`].
    pub commit: String,
    /// What kind of archive the URL names, which decides how it is unpacked
    /// and whether a `commit` is required.
    pub archive: Archive,
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
    /// A per-file timeout for *this* corpus, in seconds, where the run's
    /// default is wrong for it.
    ///
    /// **A timeout is a property of the documents, not of the run.** The four
    /// fixture corpora are files somebody wrote to exercise a reader: small,
    /// fast, and a minute is three times the slowest of them. The production
    /// corpus is a thousand documents off the open web, whose median is 288 KB
    /// and whose slowest *passing* file takes 55 seconds on the machine that
    /// recorded the bar -- so at one minute, twenty-eight of them sit within a
    /// factor of three of the limit and a slower runner turns rendering into a
    /// coin toss. That is the failure this repository spent a week on already,
    /// arriving by a different door.
    ///
    /// `None` means the run's own `--timeout`.
    pub timeout_seconds: Option<u64>,
}

/// How a corpus's archive is packed, and therefore how it is pinned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Archive {
    /// A forge's `tar.gz` of one commit — every entry before September 2026.
    /// Carries a `commit`, and extraction strips the single leading directory
    /// codeload wraps everything in.
    CodeloadTarGz,
    /// A `.zip` published as an object at a stable URL, with no repository
    /// behind it and so no commit to name. Extracted whole.
    Zip,
}

impl Archive {
    /// The word the lock file spells.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Archive::CodeloadTarGz => "tar.gz",
            Archive::Zip => "zip",
        }
    }

    /// The extension the cached archive is given.
    #[must_use]
    pub fn extension(self) -> &'static str {
        self.word()
    }
}

/// Fields every entry must carry, whatever its archive.
const REQUIRED: &[&str] = &["url", "sha256", "licence", "redistribute", "exercises"];

/// Fields an entry may carry; which of them are legal depends on the archive.
const OPTIONAL: &[&str] = &["commit", "subdir", "archive", "timeout"];

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
        if !REQUIRED.contains(&key.as_str()) && !OPTIONAL.contains(&key.as_str()) {
            return Err(format!(
                "[{name}]: `{key}` is not a lock field — the known ones are {}, \
                 and optionally {}",
                REQUIRED.join(", "),
                OPTIONAL.join(", ")
            ));
        }
    }

    let archive = match find("archive").as_deref() {
        None | Some("tar.gz") => Archive::CodeloadTarGz,
        Some("zip") => Archive::Zip,
        Some(other) => {
            return Err(format!(
                "[{name}]: `archive = {other}` is not a kind this fetches; it is \
                 `tar.gz` (the default, a forge archive of a commit) or `zip` (a \
                 published object)"
            ))
        }
    };

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
    match archive {
        Archive::CodeloadTarGz => {
            if commit.len() != 40 || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!(
                    "[{name}]: `commit = {commit}` is not a 40-character git \
                     object id; a branch or a tag is not a pin, because upstream \
                     can move it"
                ));
            }
        }
        // Refused rather than ignored. An entry carrying a commit that nothing
        // verifies reads as two pins and is one, which is the exact confusion
        // this file exists to prevent.
        Archive::Zip => {
            if !commit.is_empty() {
                return Err(format!(
                    "[{name}]: `archive = zip` names a published object rather \
                     than a repository, so there is nothing for `commit = \
                     {commit}` to pin; the `sha256` carries the whole guarantee \
                     here and a second pin would overstate it"
                ));
            }
        }
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

    let timeout_seconds = match find("timeout") {
        None => None,
        Some(raw) => match raw.parse::<u64>() {
            Ok(0) | Err(_) => {
                return Err(format!(
                    "[{name}]: `timeout = {raw}` must be a whole number of \
                     seconds above zero"
                ))
            }
            Ok(seconds) => Some(seconds),
        },
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
        archive,
        timeout_seconds,
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
        .join(format!("{}.{}", corpus.name, corpus.archive.extension()))
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
        "| Corpus | What it exercises | Upstream licence | Pinned by | Redistributed here? |\n\
         | --- | --- | --- | --- | --- |\n",
    );
    for corpus in corpora {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            corpus.name,
            corpus.exercises,
            corpus.licence,
            // How an entry is pinned belongs in the table a person reads:
            // one of these has a single pin, and this is the only place
            // that difference would otherwise not appear.
            match corpus.archive {
                Archive::CodeloadTarGz => "commit and sha256",
                Archive::Zip => "sha256 alone (no repository)",
            },
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
                "is not a lock field",
            ),
            (
                // A forge archive with no commit is the pin that is not there.
                &GOOD.replace("commit = b96d745c277a5666013359a7272d74d1896fbb9f\n", ""),
                "not a 40-character git object id",
            ),
            (
                &GOOD.replace("licence = Mixed", "archive = tarball\nlicence = Mixed"),
                "is not a kind this fetches",
            ),
            (
                // A published object carrying a commit: two pins claimed, one
                // verified.
                &GOOD.replace("licence = Mixed", "archive = zip\nlicence = Mixed"),
                "there is nothing for",
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

    /// A published object pins by its bytes and nothing else, and that is a
    /// whole entry rather than a degraded one.
    ///
    /// The pair of assertions is the point: it parses *and* the archive kind
    /// survives into the `Corpus`, because the kind is what decides which
    /// program unpacks it. A parse that quietly produced `CodeloadTarGz` would
    /// send `tar -xzf` at a zip and fail with a message about gzip headers.
    #[test]
    fn a_published_object_pins_by_its_bytes_alone() {
        let text = GOOD
            .replace("commit = b96d745c277a5666013359a7272d74d1896fbb9f\n", "")
            .replace("licence = Mixed", "archive = zip\nlicence = Mixed");
        let corpora = parse(&text).expect("a zip entry needs no commit");
        assert_eq!(corpora[0].archive, Archive::Zip);
        assert_eq!(corpora[0].commit, "");
        let table = licence_table(&corpora);
        assert!(
            table.contains("sha256 alone"),
            "the table has to say which entries have one pin: {table}"
        );
    }

    /// A corpus may state the timeout its own documents need.
    ///
    /// The zero case is the one worth a test rather than a glance: `timeout =
    /// 0` would kill every child the instant it started and report a thousand
    /// stalls, which reads exactly like an engine that stopped working.
    #[test]
    fn a_corpus_may_state_its_own_timeout_and_nonsense_is_refused() {
        let stated = GOOD.replace("licence = Mixed", "timeout = 300\nlicence = Mixed");
        let corpora = parse(&stated).expect("a stated timeout parses");
        assert_eq!(corpora[0].timeout_seconds, Some(300));

        let unstated = parse(GOOD).expect("it parses");
        assert_eq!(
            unstated[0].timeout_seconds, None,
            "no field means the run's own default, not a zero"
        );

        for bad in ["timeout = 0", "timeout = soon", "timeout = -5"] {
            let text = GOOD.replace("licence = Mixed", &format!("{bad}\nlicence = Mixed"));
            let error = parse(&text).expect_err("nonsense is refused");
            assert!(error.contains("whole number of seconds"), "{error}");
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
