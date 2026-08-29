//! The disagreement ledger: what this validator says about the veraPDF corpus,
//! where it differs from the corpus's own annotations, and why (milestone 4 of
//! `docs/design/pdfa.md`).
//!
//! # What the corpus is, and is not, here
//!
//! Every fixture's name carries `-pass-` or `-fail-` and its directory carries
//! the clause it is a test for. Those annotations are a **published statement**
//! by the people who wrote the conformance suite — data, admissible under
//! ruling 13 — and they are the bar this validator's agreement is measured
//! against. They are not where the rules came from: every rule in
//! `src/pdfa/syntax.rs` cites the ISO 19005 clause text it implements, and
//! where this build reads a clause differently from the suite, the ledger
//! records the reading rather than moving the rule.
//!
//! # The ledger's one hard requirement
//!
//! **A row without a reason string fails the test that reads it.** Not a
//! warning, not a default — [`read_ledger`] refuses the row and
//! [`a_row_without_a_reason_is_refused`] proves the refusal. The reason is the
//! whole value of the file: a list of disagreeing filenames is a list of
//! things nobody understands, and the only thing that turns it into knowledge
//! is somebody having written down what they found out. Where nobody has, the
//! honest reason says so in those words and the row is still a row.
//!
//! ```sh
//! cargo test -p tinker-pdf --test pdfa_ledger -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::{Document, PdfACoverage};

// ---- the ledger format ----------------------------------------------------

/// How a disagreement is classified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// This engine is wrong and the row is a bug to fix.
    Bug,
    /// A rule this build knows it does not run, named in `PDFA_STAGED`.
    Staged,
    /// A clause this build reads differently from the suite, recorded with the
    /// clause and the reading.
    Reading,
}

impl Class {
    fn parse(text: &str) -> Option<Class> {
        match text {
            "bug" => Some(Class::Bug),
            "staged" => Some(Class::Staged),
            "reading" => Some(Class::Reading),
            _ => None,
        }
    }
}

/// One recorded disagreement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// How it is classified.
    pub class: Class,
    /// The clause, as the part in question numbers it.
    pub clause: String,
    /// The file, as a path relative to the corpus root, or a directory when
    /// the same reason covers every file in it.
    pub subject: String,
    /// Why. Mandatory, and the point of the file.
    pub reason: String,
}

/// Why a ledger line was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum LedgerError {
    /// Fewer than the four tab-separated fields.
    Fields {
        /// Which line, counting from one.
        line: usize,
        /// How many fields it had.
        found: usize,
    },
    /// The class was not one of the three.
    Class {
        /// Which line.
        line: usize,
        /// What it said.
        found: String,
    },
    /// A field that must not be empty was.
    Empty {
        /// Which line.
        line: usize,
        /// Which field.
        field: &'static str,
    },
}

/// Reads the ledger, refusing any row that is not fully accounted for.
///
/// The refusal on an empty reason is the one this file exists to have. It is
/// checked *before* anything else about the row is used, so a row cannot slip
/// through by being well formed in every other respect.
pub fn read_ledger(text: &str) -> Result<Vec<Row>, LedgerError> {
    let mut rows = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim_end_matches(['\r', '\n']);
        if trimmed.trim().is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = trimmed.split('\t').collect();
        if fields.len() != 4 {
            return Err(LedgerError::Fields {
                line,
                found: fields.len(),
            });
        }
        let Some(class) = Class::parse(fields[0].trim()) else {
            return Err(LedgerError::Class {
                line,
                found: fields[0].trim().to_string(),
            });
        };
        for (field, value) in [
            ("clause", fields[1]),
            ("subject", fields[2]),
            ("reason", fields[3]),
        ] {
            if value.trim().is_empty() {
                return Err(LedgerError::Empty { line, field });
            }
        }
        rows.push(Row {
            class,
            clause: fields[1].trim().to_string(),
            subject: fields[2].trim().to_string(),
            reason: fields[3].trim().to_string(),
        });
    }
    Ok(rows)
}

fn ledger_text() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/pdfa_ledger.tsv"
    ))
    .expect("the ledger is committed beside this test")
}

// ---- the refusal, proved --------------------------------------------------

/// The failure mode the milestone asks to be built and shown.
///
/// A row identical to a good one but for an empty reason is refused, and the
/// error names the line and the field so the person who wrote it knows what to
/// do. Three shapes of missing reason are tried, because "the field is absent"
/// and "the field is there and blank" are different mistakes and a reader that
/// caught only one would let the other through.
#[test]
fn a_row_without_a_reason_is_refused() {
    let good = "reading\t6.1.2\tsome/file.pdf\tthe clause says the header is 1.n";
    assert_eq!(read_ledger(good).expect("a complete row is read").len(), 1);

    // The field is there and empty.
    assert_eq!(
        read_ledger("reading\t6.1.2\tsome/file.pdf\t"),
        Err(LedgerError::Empty {
            line: 1,
            field: "reason"
        })
    );
    // The field is there and only whitespace.
    assert_eq!(
        read_ledger("reading\t6.1.2\tsome/file.pdf\t   "),
        Err(LedgerError::Empty {
            line: 1,
            field: "reason"
        })
    );
    // The field is absent altogether.
    assert_eq!(
        read_ledger("reading\t6.1.2\tsome/file.pdf"),
        Err(LedgerError::Fields { line: 1, found: 3 })
    );

    // And the refusal survives being the second row rather than the first,
    // which is where a reader that validated only what it had already
    // accepted would stop looking.
    let mixed = format!("{good}\nreading\t6.1.2\tother.pdf\t\n");
    assert_eq!(
        read_ledger(&mixed),
        Err(LedgerError::Empty {
            line: 2,
            field: "reason"
        })
    );

    // The other two fields are mandatory on the same terms.
    assert_eq!(
        read_ledger("reading\t\tsome/file.pdf\ta reason"),
        Err(LedgerError::Empty {
            line: 1,
            field: "clause"
        })
    );
    assert_eq!(
        read_ledger("nonsense\t6.1.2\tf.pdf\ta reason"),
        Err(LedgerError::Class {
            line: 1,
            found: "nonsense".to_string()
        })
    );
}

/// The committed ledger is read by the same reader, so a row added without a
/// reason fails the suite rather than sitting there.
#[test]
fn the_committed_ledger_is_complete() {
    let rows = read_ledger(&ledger_text()).expect("every committed row is complete");
    assert!(!rows.is_empty(), "a ledger with no rows records nothing");
    for row in &rows {
        assert!(
            row.reason.len() >= 24,
            "a reason short enough to be a shrug is not a reason: {row:?}"
        );
    }

    // A `staged` row has to point at a rule that is actually staged, or the
    // classification is a story rather than a fact.
    let staged: Vec<&str> = tinker_pdf::PDFA_STAGED
        .iter()
        .map(|rule| rule.clause)
        .collect();
    for row in rows.iter().filter(|row| row.class == Class::Staged) {
        assert!(
            staged
                .iter()
                .any(|clause| row.clause.starts_with(clause)
                    || clause.starts_with(row.clause.as_str())),
            "no staged rule covers clause {}: {row:?}",
            row.clause
        );
    }
}

// ---- the census -----------------------------------------------------------

fn corpus_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TINKER_CORPUS") {
        let path = PathBuf::from(path);
        return path.is_dir().then_some(path);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/files")
        .canonicalize()
        .ok()
        .filter(|path| path.is_dir())
}

fn pdfs_under(root: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            pdfs_under(&path, into);
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        {
            into.push(path);
        }
    }
}

/// What the corpus says a file is: `true` for `-pass-`, `false` for `-fail-`.
fn annotation(name: &str) -> Option<bool> {
    if name.contains("-pass-") {
        Some(true)
    } else if name.contains("-fail-") {
        Some(false)
    } else {
        None
    }
}

/// Which suite a file belongs to: the corpus's top-level directory.
fn suite_of(relative: &str) -> &str {
    relative.split('/').next().unwrap_or("")
}

/// Whether a suite's files are tests **of PDF/A**.
///
/// This distinction is the measurement's, not the validator's, and getting it
/// wrong the first time cost 195 spurious disagreements. A `PDF_UA-1` fixture
/// annotated `pass` is a statement that the file conforms to **PDF/UA**; it
/// makes no PDF/A claim at all, so this validator correctly reports that it
/// claims none, and scoring that as a disagreement measures the wrong thing.
/// `Isartor test files` **are** PDF/A: they are the original PDF/A-1b
/// conformance suite, filed under `PDFA-1b` directories.
fn is_a_pdfa_test(suite: &str) -> bool {
    suite.starts_with("PDF_A-") || suite == "Isartor test files"
}

/// The narrowest clause directory a file sits in: three path components, so
/// `PDF_A-1b/6.1 File structure/6.1.10 Filters` rather than its clause group.
fn subclause_of(relative: &str) -> String {
    relative.split('/').take(3).collect::<Vec<_>>().join("/")
}

/// The clause group a file sits in: `PDF_A-2b/6.1 File structure`.
fn group_of(relative: &str) -> String {
    let parts: Vec<&str> = relative.split('/').collect();
    match parts.len() {
        0 => String::new(),
        1 => parts[0].to_string(),
        _ => format!("{}/{}", parts[0], parts[1]),
    }
}

/// One clause group's agreement.
#[derive(Default, Clone, Copy)]
struct Tally {
    /// Files the corpus annotates `pass` that this build found nothing wrong
    /// with.
    pass_agreed: usize,
    /// Files the corpus annotates `pass` that this build reported on.
    pass_disagreed: usize,
    /// Files the corpus annotates `fail` that this build reported on.
    fail_agreed: usize,
    /// Files the corpus annotates `fail` that this build found nothing wrong
    /// with.
    fail_disagreed: usize,
}

impl Tally {
    fn total(self) -> usize {
        self.pass_agreed + self.pass_disagreed + self.fail_agreed + self.fail_disagreed
    }
    fn agreed(self) -> usize {
        self.pass_agreed + self.fail_agreed
    }
}

/// Measures this validator against the corpus's own annotations, per clause
/// group, and prints every disagreement by name.
///
/// `RAN` / `SKIPPED` on the first line, because a census that silently finds
/// no corpus and passes is worse than one that fails.
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn agreement_with_the_corpus_annotations_per_clause_group() {
    let Some(root) = corpus_root() else {
        println!("SKIPPED (no corpus; set TINKER_CORPUS to the fetched corpus/files)");
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();

    let mut groups: BTreeMap<String, Tally> = BTreeMap::new();
    let mut other_suites: BTreeMap<String, Tally> = BTreeMap::new();
    let mut bar = Tally::default();
    let mut false_positives: Vec<String> = Vec::new();
    let mut false_negatives: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut unopenable = 0usize;
    let mut annotated = 0usize;

    for path in &files {
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let Some(expected_pass) = annotation(&name) else {
            continue;
        };
        annotated += 1;
        let Ok(bytes) = std::fs::read(path) else {
            unopenable += 1;
            continue;
        };
        let Ok(document) = Document::open(bytes) else {
            unopenable += 1;
            continue;
        };
        let verdict = document.validate_pdfa();
        let clean = verdict.findings.is_empty();

        let suite = suite_of(&relative).to_string();
        let key = group_of(&relative);
        let counts_towards_the_bar = is_a_pdfa_test(&suite);
        let tally = if counts_towards_the_bar {
            groups.entry(key.clone()).or_default()
        } else {
            other_suites.entry(suite).or_default()
        };

        match (expected_pass, clean) {
            (true, true) => tally.pass_agreed += 1,
            (true, false) => tally.pass_disagreed += 1,
            (false, false) => tally.fail_agreed += 1,
            (false, true) => tally.fail_disagreed += 1,
        }
        if !counts_towards_the_bar {
            continue;
        }
        match (expected_pass, clean) {
            (true, true) => bar.pass_agreed += 1,
            (true, false) => {
                bar.pass_disagreed += 1;
                false_positives.push(format!(
                    "{relative}\n    {:?}",
                    verdict
                        .findings
                        .iter()
                        .map(|f| format!("{} {:?}", f.clause, f.kind))
                        .collect::<Vec<_>>()
                ));
            }
            (false, false) => bar.fail_agreed += 1,
            (false, true) => {
                bar.fail_disagreed += 1;
                // Keyed by *subclause*, because one reason covering a whole
                // clause directory would be a generalisation exactly where the
                // specific cases are the interesting ones.
                false_negatives
                    .entry(subclause_of(&relative))
                    .or_default()
                    .push(relative.clone());
            }
        }
    }

    println!(
        "RAN over {annotated} annotated files of {} found",
        files.len()
    );
    if unopenable > 0 {
        println!("{unopenable} could not be opened at all");
    }

    println!("\nper clause group: agreed/total  (pass +/-, fail +/-)");
    for (group, tally) in &groups {
        println!(
            "  {:<48} {:>4}/{:<4}  pass {}/{}  fail {}/{}",
            group,
            tally.agreed(),
            tally.total(),
            tally.pass_agreed,
            tally.pass_disagreed,
            tally.fail_agreed,
            tally.fail_disagreed
        );
    }
    println!(
        "\nTHE BAR (files that are tests of PDF/A) {}/{}  \
         pass agreed {}, pass disagreed {}, fail agreed {}, fail disagreed {}",
        bar.agreed(),
        bar.total(),
        bar.pass_agreed,
        bar.pass_disagreed,
        bar.fail_agreed,
        bar.fail_disagreed
    );

    println!(
        "\nnot tests of PDF/A, reported and excluded from the bar — a fixture \
         for another standard makes no PDF/A claim, so judging it against \
         PDF/A measures the measurement rather than the engine:"
    );
    let mut excluded = 0usize;
    for (suite, tally) in &other_suites {
        excluded += tally.total();
        println!(
            "  {:<48} {:>4} files  (this build reported on {} of them)",
            suite,
            tally.total(),
            tally.pass_disagreed + tally.fail_agreed
        );
    }
    println!("  excluded in total: {excluded}");

    println!(
        "\n--- FALSE POSITIVES ({}) : the corpus says pass and this build \
         reported something ---",
        false_positives.len()
    );
    for line in &false_positives {
        println!("  {line}");
    }

    println!(
        "\n--- FALSE NEGATIVES ({}) : the corpus says fail and this build \
         found nothing, by group ---",
        bar.fail_disagreed
    );
    for (group, names) in &false_negatives {
        println!("  {group}: {}", names.len());
    }
}

/// The syntax group alone, over the same files, so the sweep the design doc
/// calls cheap can be seen to be cheap and to still say something.
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn a_syntax_only_sweep_over_the_whole_corpus() {
    let Some(root) = corpus_root() else {
        println!("SKIPPED (no corpus; set TINKER_CORPUS to the fetched corpus/files)");
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();

    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut swept = 0usize;
    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(document) = Document::open(bytes) else {
            continue;
        };
        swept += 1;
        for finding in document.validate_pdfa_with(PdfACoverage::SYNTAX).findings {
            let label = format!("{:?}", finding.kind)
                .split_whitespace()
                .next()
                .unwrap_or("?")
                .trim_end_matches('{')
                .to_string();
            *kinds.entry(label).or_default() += 1;
        }
    }
    println!("RAN a syntax-only sweep over {swept} files");
    for (kind, count) in &kinds {
        println!("  {kind:<32} {count}");
    }
}
