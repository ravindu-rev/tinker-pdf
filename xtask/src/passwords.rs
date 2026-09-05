//! The corpus password sidecar: thirty-four abstentions turned into
//! measurements.
//!
//! # Why this file exists
//!
//! `tpdf` tries the empty password and nothing else, which is right for a tool
//! and wrong for a census. Thirty-six corpus files are *encryption tests* —
//! that is what they were written to be — so a run that supplies no password
//! records them as failures and the pass rate carries thirty-six files nobody
//! measured. [`docs/verification.md`] called them "the runner's limitation,
//! not the engine's" and left the decision open; this is the decision.
//!
//! # Why a sidecar rather than a guess
//!
//! Every password here is **stated upstream**, and the `source` column says
//! where. Ruling 13 forbids an outside program adjudicating a document; it
//! does not forbid reading a test script for an input, which is what a
//! password is. Eight of the qpdf fixtures carry theirs in their own file
//! names — `enc-R2,V1,U=view,O=master.pdf` says the user password is `view` —
//! and the rest come from `qtest/*.test`, from pdf.js's `test_manifest.json`,
//! or from the spec its own unit test names.
//!
//! Two rows are **derived rather than quoted**, and both say so in their
//! source: `c-r4.pdf`, which no upstream script mentions, and
//! `job-json-encrypt-40.pdf`, likewise. Their passwords were found by trying
//! the ones their siblings use, and a derivation that had to be *tried* is
//! marked so nobody later reads it as a quotation.
//!
//! # What is not here
//!
//! Two of the thirty-six stay refused, and neither is a password:
//!
//! - `bad-encryption-length.pdf` spells the key `/Wength 128`, so `/Length` is
//!   absent and 7.6.3.2's default of **40 bits** applies. The file's own `/U`
//!   was computed at 128, which this engine can be shown by arithmetic rather
//!   than by trying: Algorithm 5 over the empty password reproduces `/U`
//!   exactly at sixteen key bytes and not at five. Following the default and
//!   refusing is the specification's answer; opening it would need a fallback
//!   that retries at a length the file never states.
//! - `issue-147.pdf` is ninety-one bytes of fuzzer output from qpdf's own
//!   `fuzz/` corpus — no header, `/O` and `/U` both empty strings, `/Length
//!   160`. There is no password: with an empty `/U` there is nothing for a
//!   password to authenticate against.
//!
//! # The one row that names a gap
//!
//! `saslprep-r6.pdf` is pdf.js's test that a revision 6 password is
//! **SASLprep-normalised before it is hashed** (ISO 32000-2 7.6.4.3.3, and
//! RFC 4013 under it). Upstream states the password as `S<U+00AA>SL<U+00AD>prep`
//! — a feminine ordinal and a soft hyphen — and the file authenticates the
//! *normalised* form, `SaSLprep`, because normalisation maps `<U+00AA>` to `a`
//! and deletes the soft hyphen. This engine does not implement SASLprep, so
//! the sidecar carries the normalised form and the row's source says why. The
//! gap is named in `docs/features/encryption.md` rather than hidden by this
//! file: what the sidecar buys is the *rest* of the file measured, and what it
//! must not buy is a missing feature looking like a present one.

use std::collections::BTreeMap;

/// Where the sidecar lives, relative to the repository root.
pub const PASSWORDS_PATH: &str = "corpus/passwords.tsv";

/// One file's password, and where it was read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The corpus, as `corpora.lock` names it.
    pub corpus: String,
    /// The file, relative to that corpus's files directory, with `/`
    /// separators — the same string the report records as `path`.
    pub path: String,
    /// The password to open it with.
    pub password: String,
    /// The upstream line, file or naming rule that states it.
    pub source: String,
}

/// Every row, indexed for the lookup a run does four thousand times.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Passwords {
    rows: Vec<Row>,
}

/// Why a sidecar was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum PasswordError {
    /// A line has the wrong number of columns.
    Columns {
        /// Which line, counting from one, blank and comment lines included.
        line: usize,
        /// How many tab-separated fields it had.
        found: usize,
    },
    /// A field the format requires is empty.
    Empty {
        /// Which line.
        line: usize,
        /// Which column.
        key: &'static str,
    },
    /// Two rows name the same file.
    Duplicate {
        /// Which line the second one is on.
        line: usize,
        /// The corpus and path both rows name.
        what: String,
    },
}

impl core::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PasswordError::Columns { line, found } => write!(
                f,
                "line {line} has {found} tab-separated fields and the format is \
                 four: corpus, path, password, source"
            ),
            PasswordError::Empty { line, key } => write!(
                f,
                "line {line} has an empty `{key}`; a password with no stated \
                 source is a guess, and this file does not carry guesses"
            ),
            PasswordError::Duplicate { line, what } => write!(
                f,
                "line {line} names `{what}` again; two passwords for one file \
                 means one of them is never tried and nobody knows which"
            ),
        }
    }
}

/// Reads the sidecar.
///
/// Tab-separated because a password may contain a space and eight of these do
/// — `asdf asdf asdf asdf asdf asdf qwer` is one of qpdf's. Comments start
/// with `#`; blank lines are skipped. The file is UTF-8, and three passwords
/// are not ASCII: `p&#228;ssw&#246;rt`, `&#230;&#248;&#229;` and the `SaSLprep`
/// row's own note.
pub fn parse(text: &str) -> Result<Passwords, PasswordError> {
    let mut rows: Vec<Row> = Vec::new();
    let mut seen: BTreeMap<(String, String), ()> = BTreeMap::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Only the *line* ends are trimmed. A password may legitimately begin
        // or end with a space, and trimming the fields would silently change
        // one; the format's own separator is the tab.
        let fields: Vec<&str> = raw.trim_end_matches(['\r', '\n']).split('\t').collect();
        let [corpus, path, password, source] = fields.as_slice() else {
            return Err(PasswordError::Columns {
                line,
                found: fields.len(),
            });
        };
        for (key, value) in [
            ("corpus", *corpus),
            ("path", *path),
            ("password", *password),
            ("source", *source),
        ] {
            if value.trim().is_empty() {
                return Err(PasswordError::Empty { line, key });
            }
        }
        let key = ((*corpus).to_string(), (*path).to_string());
        if seen.insert(key, ()).is_some() {
            return Err(PasswordError::Duplicate {
                line,
                what: format!("{corpus}/{path}"),
            });
        }
        rows.push(Row {
            corpus: (*corpus).to_string(),
            path: (*path).to_string(),
            password: (*password).to_string(),
            source: (*source).to_string(),
        });
    }
    Ok(Passwords { rows })
}

impl Passwords {
    /// The rows for one corpus, by the path the report records.
    ///
    /// Built once per corpus rather than searched per file: four thousand
    /// linear scans of thirty-four rows is nothing, and a map says what the
    /// lookup key is.
    #[must_use]
    pub fn for_corpus(&self, name: &str) -> BTreeMap<&str, &str> {
        self.rows
            .iter()
            .filter(|row| row.corpus == name)
            .map(|row| (row.path.as_str(), row.password.as_str()))
            .collect()
    }

    /// Every row, in file order.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The rows naming a corpus that is not in the lockfile.
    ///
    /// A stale row is silent by construction — it matches no file and nothing
    /// happens — so it is looked for rather than waited for. A row whose
    /// *corpus* is unknown is always a defect; a row whose *path* is unknown
    /// may just be a sampled run, which is why only this half is checked here
    /// and the other half is reported by the run that saw the files.
    #[must_use]
    pub fn corpora_not_in(&self, known: &[String]) -> Vec<&Row> {
        self.rows
            .iter()
            .filter(|row| !known.contains(&row.corpus))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "# a comment\n\
                        pdfjs\tissue3371.pdf\tELXRTQWS\ttest_manifest.json\n\
                        \n\
                        qpdf\t20-pages.pdf\tuser\tcheck-encryption.test\n";

    #[test]
    fn it_reads_rows_and_skips_comments_and_blanks() {
        let read = parse(GOOD).expect("it parses");
        assert_eq!(read.rows().len(), 2);
        assert_eq!(read.for_corpus("pdfjs")["issue3371.pdf"], "ELXRTQWS");
        assert_eq!(read.for_corpus("qpdf")["20-pages.pdf"], "user");
        assert!(read.for_corpus("verapdf").is_empty());
    }

    /// A password may contain a space, which is why the separator is a tab and
    /// why the fields are not trimmed.
    #[test]
    fn a_password_keeps_its_spaces() {
        let text = "qpdf\tenc-long-password.pdf\tasdf asdf qwer\tencryption.test:90\n";
        let read = parse(text).expect("it parses");
        assert_eq!(
            read.for_corpus("qpdf")["enc-long-password.pdf"],
            "asdf asdf qwer"
        );
    }

    #[test]
    fn a_row_with_no_source_is_refused() {
        let text = "qpdf\tc-r2.pdf\tuser1\t\n";
        assert_eq!(
            parse(text),
            Err(PasswordError::Empty {
                line: 1,
                key: "source"
            })
        );
    }

    #[test]
    fn a_row_with_the_wrong_shape_is_refused() {
        let text = "qpdf\tc-r2.pdf\tuser1\n";
        assert_eq!(
            parse(text),
            Err(PasswordError::Columns { line: 1, found: 3 })
        );
    }

    /// Two rows for one file is the failure that would otherwise be invisible:
    /// one of them is used, the other is not, and the report shows only that
    /// the file opened.
    #[test]
    fn two_passwords_for_one_file_are_refused() {
        let text = "qpdf\tc-r2.pdf\tuser1\tencryption.test:381\n\
                    qpdf\tc-r2.pdf\towner1\ta guess\n";
        assert_eq!(
            parse(text),
            Err(PasswordError::Duplicate {
                line: 2,
                what: "qpdf/c-r2.pdf".to_string()
            })
        );
    }

    #[test]
    fn a_row_naming_a_corpus_the_lockfile_does_not_have_is_found() {
        let text = "pdfjs\tissue3371.pdf\tELXRTQWS\tmanifest\n\
                    oldname\tgone.pdf\tx\tsomewhere\n";
        let read = parse(text).expect("it parses");
        let known = vec!["pdfjs".to_string(), "qpdf".to_string()];
        let stale = read.corpora_not_in(&known);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].path, "gone.pdf");
    }

    /// The committed sidecar is the one the runs use, so it is parsed here
    /// rather than only by a run somebody remembers to make.
    #[test]
    fn the_committed_sidecar_parses() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the repository root")
            .join(PASSWORDS_PATH);
        let text = std::fs::read_to_string(&root).expect("the sidecar is committed");
        let read = parse(&text).expect("the committed sidecar parses");
        assert!(
            read.rows().len() >= 34,
            "the sidecar carries {} rows and the corpus has 34 files it opens",
            read.rows().len()
        );
    }
}
