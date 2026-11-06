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

/// Which way a file disagrees with its annotation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    /// The corpus says `pass` and this build reported something.
    FalsePositive,
    /// The corpus says `fail` and this build found nothing.
    FalseNegative,
}

/// Whether `row` can be why a disagreement of `direction` happened.
///
/// A `staged` row names a rule this build does not run, and a rule that does
/// not run reports nothing: it can be why a failing file went uncaught and
/// never why a conforming file was reported. A `bug` or a `reading` row can be
/// either.
fn accounts_for(row: &Row, direction: Direction) -> bool {
    row.class != Class::Staged || direction == Direction::FalseNegative
}

/// The ledger held to a census's disagreements, each a subject and its
/// direction: the disagreements no row accounts for, and the rows no
/// disagreement they could account for sits under, which are stale.
///
/// A row sits over a subject that is its own or starts with it, so a
/// directory row covers every file in the directory.
fn audit(rows: &[Row], disagreements: &[(String, Direction)]) -> (Vec<String>, Vec<String>) {
    let explains = |row: &Row, (subject, direction): &(String, Direction)| {
        subject.starts_with(&row.subject) && accounts_for(row, *direction)
    };
    let uncovered = disagreements
        .iter()
        .filter(|disagreement| !rows.iter().any(|row| explains(row, disagreement)))
        .map(|(subject, _)| subject.clone())
        .collect();
    let stale = rows
        .iter()
        .filter(|row| {
            !disagreements
                .iter()
                .any(|disagreement| explains(row, disagreement))
        })
        .map(|row| row.subject.clone())
        .collect();
    (uncovered, stale)
}

/// A `staged` row covers a `fail` file under it and not a `pass` file, and
/// with only a `pass` file under it, it is stale; a `reading` or a `bug` row
/// covers the `pass` file.
#[test]
fn a_staged_row_does_not_cover_a_false_positive() {
    let row = |class: Class, subject: &str| Row {
        class,
        clause: "6.2.10".to_string(),
        subject: subject.to_string(),
        reason: "the reason this row is written for".to_string(),
    };
    let directory = "PDF_A-4/6.2 Graphics/6.2.10 Fonts";
    let pass = (
        format!("{directory}/6-2-10-3-2-t01-pass-a.pdf"),
        Direction::FalsePositive,
    );
    let fail = (directory.to_string(), Direction::FalseNegative);

    let staged = [row(Class::Staged, directory)];
    let (uncovered, stale) = audit(&staged, std::slice::from_ref(&pass));
    assert_eq!(
        uncovered,
        std::slice::from_ref(&pass.0),
        "a staged rule reports nothing"
    );
    assert_eq!(stale, [directory], "nothing it can explain sits under it");

    for class in [Class::Reading, Class::Bug] {
        let (uncovered, stale) = audit(&[row(class, directory)], std::slice::from_ref(&pass));
        assert!(uncovered.is_empty(), "{class:?}: {uncovered:?}");
        assert!(stale.is_empty(), "{class:?}: {stale:?}");
    }

    let (uncovered, stale) = audit(&staged, &[fail, pass.clone()]);
    assert_eq!(
        uncovered,
        [pass.0],
        "the fail file is covered, the pass file not"
    );
    assert!(
        stale.is_empty(),
        "a false negative sits under it: {stale:?}"
    );
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

/// Whether the absence of a corpus is a failure rather than a skip.
///
/// **A banner is not a gate.** A skipped census prints one line, passes and
/// exits zero, which in a wall of build output reads exactly like a census that
/// ran — and `epub_fetched.rs` records what that cost when it happened there: a
/// suite skipped for most of a session, in every lane, and a stale pin and two
/// unrecorded re-baselines accumulated behind it.
///
/// It matters more here than anywhere, because this file is the **instrument**
/// the PDF/A work is measured with. `CONTRIBUTING.md` names a campaign whose
/// subject can skip as one of the two ways a counted injection lies: every
/// injection against a census that found no corpus reports zero, and a row of
/// zeros reads as a suite that does not care rather than as a harness that did
/// not run.
///
/// Opt-in, because a contributor with no network still has to be able to run
/// `cargo test`; set in CI and set by anyone counting an injection here.
fn required() -> bool {
    std::env::var_os("TINKER_CORPUS_REQUIRED").is_some_and(|value| value != "0")
}

/// The veraPDF corpus's own root, which is **not** `corpus/files`.
///
/// This distinction is the whole measurement, and getting it wrong made the
/// bar read `0/0` for as long as it did. [`suite_of`] takes the first
/// component of a file's path relative to this root and asks whether it names
/// a PDF/A suite; rooted at `corpus/files`, that first component is `verapdf`,
/// which names no suite, so **every one of the 2 896 annotated files was
/// excluded as a test of some other standard** and the census passed over
/// nothing while printing a plausible page of output.
///
/// So the root is the corpus directory itself, and `corpus/files/verapdf` is
/// tried before `corpus/files` for a `TINKER_CORPUS` that names the fetch
/// directory rather than the corpus. `a_census_that_measured_nothing_is_a
/// _failure` is what stops the same thing happening the next way round.
fn corpus_root() -> Option<PathBuf> {
    let candidate = |path: PathBuf| -> Option<PathBuf> {
        if path.join("verapdf").is_dir() {
            return Some(path.join("verapdf"));
        }
        path.is_dir().then_some(path)
    };
    if let Ok(path) = std::env::var("TINKER_CORPUS") {
        return candidate(PathBuf::from(path));
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/files")
        .canonicalize()
        .ok()
        .and_then(candidate)
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

/// Which of the validator's **rule groups** a corpus file is a test of.
///
/// The directory tree is organised by clause, and the clause numbering differs
/// per part — fonts are 6.3 in part 1, 6.2.11 in parts 2 and 3 and 6.2.10 in
/// part 4 — so the mapping is made on the directory *name*, which the suite
/// keeps stable across parts: every font directory is called `Fonts`, every
/// output-intent directory `Output intent`, and so on.
///
/// This exists because milestone 5's exit criterion is a **font- and
/// colour-clause agreement rate**, and the per-clause-group table above cannot
/// give one: `PDF_A-2b/6.2 Graphics` holds the colour clauses, the font
/// clauses, transparency and images in one row.
fn rule_group_of(relative: &str) -> Option<&'static str> {
    let lower = relative.to_ascii_lowercase();
    // Longest-lived signal first: `6.2.11 Fonts` and `6.3 Fonts` both contain
    // `fonts`, and nothing else in the tree does.
    if lower.contains("/fonts") || lower.contains(" fonts/") {
        return Some("fonts");
    }
    for marker in [
        "output intent",
        "colour space",
        "color space",
        "colour spaces",
        "iccbased",
        "uncalibrated",
        "separation and devicen",
        "indexed and pattern",
        "rendering intent",
        "transparency",
    ] {
        if lower.contains(marker) {
            return Some("colour");
        }
    }
    None
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
        println!("pdfa-ledger: SKIPPED (no corpus; set TINKER_CORPUS to the fetched corpus/files)");
        assert!(
            !required(),
            "TINKER_CORPUS_REQUIRED is set and there is no corpus at TINKER_CORPUS:              this census would have passed over nothing"
        );
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();

    let mut groups: BTreeMap<String, Tally> = BTreeMap::new();
    let mut rule_groups: BTreeMap<&'static str, Tally> = BTreeMap::new();
    let mut other_suites: BTreeMap<String, Tally> = BTreeMap::new();
    let mut bar = Tally::default();
    let mut false_positives: Vec<(String, Vec<String>)> = Vec::new();
    let mut false_negatives: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // What this build reported on each failing file it caught, so that a row
    // the run finds stale is judged on why its files are caught now: by the
    // rule it was written for, or by another finding.
    let mut caught: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
        if let Some(rule_group) = rule_group_of(&relative) {
            let tally = rule_groups.entry(rule_group).or_default();
            match (expected_pass, clean) {
                (true, true) => tally.pass_agreed += 1,
                (true, false) => tally.pass_disagreed += 1,
                (false, false) => tally.fail_agreed += 1,
                (false, true) => tally.fail_disagreed += 1,
            }
        }
        match (expected_pass, clean) {
            (true, true) => bar.pass_agreed += 1,
            (true, false) => {
                bar.pass_disagreed += 1;
                // Each finding as it displays itself: its clause, its
                // object and its kind with the kind's fields.
                false_positives.push((
                    relative.clone(),
                    verdict.findings.iter().map(ToString::to_string).collect(),
                ));
            }
            (false, false) => {
                bar.fail_agreed += 1;
                caught.insert(
                    relative.clone(),
                    verdict.findings.iter().map(ToString::to_string).collect(),
                );
            }
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
        "pdfa-ledger: RAN over {annotated} annotated files of {} found",
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
        "\nper rule group (milestone 5's exit criterion is the font and colour \
         rows): agreed/total  (pass +/-, fail +/-)"
    );
    for (rule_group, tally) in &rule_groups {
        println!(
            "  {:<48} {:>4}/{:<4}  pass {}/{}  fail {}/{}",
            rule_group,
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

    // A census that excluded everything is not a census. The bar read `0/0`
    // for as long as the root was `corpus/files` rather than the corpus, and
    // it printed this whole page while doing it -- which is exactly the shape
    // `docs/verification.md` calls out: a check that can be absent is red, not
    // green, and a plausible page of output is the worst kind of absent.
    assert!(
        bar.total() > 0,
        "the bar is empty: every annotated file was excluded, so the root is not the corpus own root -- see corpus_root"
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
    for (path, findings) in &false_positives {
        println!("  {path}\n    {}", findings.join("; "));
    }

    println!(
        "\n--- FALSE NEGATIVES ({}) : the corpus says fail and this build \
         found nothing, by group ---",
        bar.fail_disagreed
    );
    for (group, names) in &false_negatives {
        println!("  {group}: {}", names.len());
        // A small group is named file by file. A ledger row for four files
        // that cannot say which four is a row nobody can act on, and the
        // three rows that currently say "nobody has established why" are
        // exactly the small groups — so this is the affordance that lets the
        // next person turn them into something better.
        if names.len() <= 4 {
            for name in names {
                println!("      {name}");
            }
        }
    }

    // ---- and every one of them is accounted for in the ledger ------------
    //
    // This is what stops the ledger being a snapshot somebody wrote once. A
    // disagreement with no row fails here by name; a row whose subject no
    // longer disagrees is stale and fails here too. The reason field is
    // mandatory at the reader (`a_row_without_a_reason_is_refused`), so
    // "covered" cannot mean "listed without an explanation". Which row can
    // cover which direction is `audit`'s, which a test holds without the
    // corpus.
    let rows = read_ledger(&ledger_text()).expect("the committed ledger is complete");
    let disagreements: Vec<(String, Direction)> = false_positives
        .iter()
        .map(|(path, _)| (path.clone(), Direction::FalsePositive))
        .chain(
            false_negatives
                .keys()
                .map(|key| (key.clone(), Direction::FalseNegative)),
        )
        .collect();
    let (uncovered, stale) = audit(&rows, &disagreements);

    println!(
        "\nledger: {} rows, {} disagreements uncovered, {} rows stale",
        rows.len(),
        uncovered.len(),
        stale.len()
    );
    // A stale row is judged on why its files no longer disagree, so each
    // failing file under it is printed with what this build reported on it.
    for subject in &stale {
        println!("  stale: {subject}");
        for (path, findings) in caught
            .iter()
            .filter(|(path, _)| path.starts_with(subject.as_str()))
        {
            println!("    {path}: {}", findings.join("; "));
        }
    }
    assert!(
        uncovered.is_empty(),
        "disagreements with no ledger row — every one of these needs a class \
         and a reason, and \"I do not know why\" is an acceptable reason and a \
         missing row is not: {uncovered:#?}"
    );
    assert!(
        stale.is_empty(),
        "ledger rows whose subject no longer disagrees; delete them: {stale:#?}"
    );
}

/// The syntax group alone, over the same files, so the sweep the design doc
/// calls cheap can be seen to be cheap and to still say something.
#[test]
#[ignore = "walks the fetched corpora; run with --ignored --nocapture"]
fn a_syntax_only_sweep_over_the_whole_corpus() {
    let Some(root) = corpus_root() else {
        println!("pdfa-ledger: SKIPPED (no corpus; set TINKER_CORPUS to the fetched corpus/files)");
        assert!(
            !required(),
            "TINKER_CORPUS_REQUIRED is set and there is no corpus at TINKER_CORPUS:              this census would have passed over nothing"
        );
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
    println!("pdfa-ledger: RAN a syntax-only sweep over {swept} files");
    for (kind, count) in &kinds {
        println!("  {kind:<32} {count}");
    }
}
