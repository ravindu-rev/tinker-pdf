//! Milestone 2's exit criterion: a synthetic corpus containing a file that
//! aborts and one that hangs produces a **complete report with both marked**.
//!
//! The two files are built deliberately, here, because there is no honest way
//! to obtain them. A real PDF that reliably aborts this engine would be a bug
//! to fix rather than a fixture to keep, and one that reliably hangs it would
//! be the same bug with a longer fuse — so waiting for the corpus to supply
//! them means the isolation is untested until the day it is needed. Instead
//! the files are valid PDFs carrying an instruction in a PDF comment (7.2.4,
//! ignored by every reader including this one), and `corpus-stub` is the child
//! that obeys them. What is under test is the runner: that one process dying
//! and one process never returning both come out as *entries in the report*
//! rather than as a run that stopped.
//!
//! Everything here goes through `corpus::run_files`, which is the same
//! function `cargo xtask corpus-run` calls over the real corpora.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// The timeouts below are five seconds rather than the fraction of a second a
// deliberately-hanging stub needs, and the difference is not slack -- it is
// the test being about the right thing.
//
// What these assert is that a hang is *recorded as a hang*, not that it is
// noticed within any particular interval. A tight bound makes the assertion
// depend on how quickly a healthy child can be spawned and run, which under
// `cargo test --workspace` is a machine-load question: at 700 ms these failed
// two of seven under full parallelism and passed alone, which is a test
// reporting on the runner rather than on the code. The stub's hang is a loop
// of sixty-second sleeps, so five seconds still trips it with room to spare
// while a healthy file has room to finish.
//
// Gap 15 reached the same conclusion from the other direction and built its
// cancellation tests on a counting token precisely so no clock was involved.

use xtask::corpus;
use xtask::report::{CorpusReport, Run, Settings};
use xtask::runner::{Child, Outcome};

/// A valid one-page PDF carrying `instruction` as a comment.
///
/// Really a PDF: `tpdf` opens every one of these and renders its page, which
/// is what keeps the fixtures honest. The only thing that makes one
/// pathological is a line of text saying so, and only to a harness that has
/// agreed to read it.
fn synthetic_pdf(instruction: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"%PDF-1.7\n");
    out.extend_from_slice(format!("%%TINKER-CORPUS-STUB {instruction}\n").as_bytes());

    let mut offsets = Vec::new();
    let push = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &str| {
        offsets.push(out.len());
        out.extend_from_slice(body.as_bytes());
    };
    push(
        &mut out,
        &mut offsets,
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
    );
    push(
        &mut out,
        &mut offsets,
        "2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n",
    );
    push(
        &mut out,
        &mut offsets,
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] \
         /Contents 4 0 R >>\nendobj\n",
    );
    push(
        &mut out,
        &mut offsets,
        "4 0 obj\n<< /Length 44 >>\nstream\n\
         0 0 1 rg 10 10 100 50 re f\n0 g 5 5 m 50 50 l S\nendstream\nendobj\n",
    );

    let xref_at = out.len();
    out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(b"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n");
    out.extend_from_slice(format!("{xref_at}\n%%EOF\n").as_bytes());
    out
}

/// Writes the synthetic corpus and returns its directory and its files.
fn synthetic_corpus(tag: &str, instructions: &[(&str, &str)]) -> (PathBuf, Vec<PathBuf>) {
    let dir = std::env::temp_dir().join(format!(
        "tinker-synthetic-{}-{tag}-{}",
        std::process::id(),
        instructions.len()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory for the synthetic corpus");

    let mut files = Vec::new();
    for (name, instruction) in instructions {
        let path = dir.join(format!("{name}.pdf"));
        std::fs::write(&path, synthetic_pdf(instruction)).expect("a synthetic file");
        files.push(path);
    }
    files.sort();
    (dir, files)
}

fn stub() -> Child {
    Child {
        program: PathBuf::from(env!("CARGO_BIN_EXE_corpus-stub")),
        args: Vec::new(),
    }
}

fn outcome_of<'a>(results: &'a [xtask::runner::FileResult], name: &str) -> &'a Outcome {
    &results
        .iter()
        .find(|r| r.path.contains(name))
        .unwrap_or_else(|| panic!("`{name}` is missing from the report entirely"))
        .outcome
}

/// The criterion itself.
#[test]
fn a_file_that_aborts_and_a_file_that_hangs_both_reach_the_report() {
    let (dir, files) = synthetic_corpus(
        "both",
        &[
            ("aa-good-first", "pass"),
            ("bb-aborts", "abort"),
            ("cc-hangs", "hang"),
            ("dd-good-last", "pass"),
            ("ee-degraded", "degraded"),
            ("ff-unopenable", "unopenable"),
        ],
    );

    let results = corpus::run_files(&stub(), &dir, &files, Duration::from_secs(5), 3);

    // Complete: every file has an entry. The failure this guards against is
    // not a wrong verdict, it is a *short report* — the run stopping at the
    // abort, or hanging at the hang, and producing something that reads
    // exactly like a corpus with four files in it.
    assert_eq!(
        results.len(),
        files.len(),
        "every file must have an entry: {results:#?}"
    );

    assert_eq!(*outcome_of(&results, "aa-good-first"), Outcome::Passed);
    assert_eq!(*outcome_of(&results, "dd-good-last"), Outcome::Passed);
    assert_eq!(
        *outcome_of(&results, "ee-degraded"),
        Outcome::Passed,
        "ruling 2: a placeholder is correct behaviour, so the file passed"
    );

    // Marked, and marked as two different things. A runner that called both
    // of them "failed" would satisfy a weaker reading of the criterion and
    // would be useless: a crash is a bug to fix and a hang is a bound to
    // find, and they are never the same afternoon's work.
    assert!(
        matches!(outcome_of(&results, "bb-aborts"), Outcome::Crashed(_)),
        "the aborting file: {:?}",
        outcome_of(&results, "bb-aborts")
    );
    assert_eq!(
        *outcome_of(&results, "cc-hangs"),
        Outcome::TimedOut,
        "the hanging file"
    );
    assert!(
        matches!(outcome_of(&results, "ff-unopenable"), Outcome::Failed(reason)
                 if reason.contains("not a PDF")),
        "a file that would not open is a failure, not a crash: {:?}",
        outcome_of(&results, "ff-unopenable")
    );

    let degraded = results
        .iter()
        .find(|r| r.path.contains("ee-degraded"))
        .expect("the degraded file");
    assert!(degraded.degraded(), "and it is on the second axis");
    assert!(degraded.capabilities.contains("jbig2"));

    let _ = std::fs::remove_dir_all(&dir);
}

/// A child that dies **after** printing part of a record, and exits zero.
///
/// This is the case an exit code cannot catch and the sentinel can. The stub's
/// `truncate` role prints everything up to `rendered` and stops; the process
/// exits successfully, having said the file opened and has nine pages. A
/// runner that trusted what it was given would record a pass.
#[test]
fn a_child_that_stops_mid_record_is_a_crash_rather_than_a_pass() {
    let (dir, files) = synthetic_corpus("truncated", &[("aa-truncates", "truncate")]);
    let results = corpus::run_files(&stub(), &dir, &files, Duration::from_secs(5), 1);
    assert_eq!(results.len(), 1);
    assert!(
        matches!(&results[0].outcome, Outcome::Crashed(_)),
        "an incomplete record is never a result: {:?}",
        results[0].outcome
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The timeout is a bound on the run, not merely a label on one file.
///
/// Three hanging files at a 500 ms timeout, run two at a time, must finish in
/// appreciably less than the four seconds that no timeout at all would take —
/// and the assertion is deliberately loose, because a slow machine under load
/// is not a bug.
#[test]
fn the_timeout_bounds_the_whole_run() {
    let (dir, files) = synthetic_corpus(
        "timeout",
        &[
            ("aa-hangs", "hang"),
            ("bb-hangs", "hang"),
            ("cc-hangs", "hang"),
        ],
    );
    let started = Instant::now();
    let results = corpus::run_files(&stub(), &dir, &files, Duration::from_millis(500), 2);
    let elapsed = started.elapsed();

    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|r| r.outcome == Outcome::TimedOut));
    assert!(
        elapsed < Duration::from_secs(8),
        "three 500 ms timeouts two at a time took {elapsed:?}; the children \
         sleep for a minute each, so this only passes if they were killed"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The report that comes out of such a run says what happened, in the file
/// somebody reads afterwards.
#[test]
fn the_report_carries_both_states_by_name() {
    let (dir, files) = synthetic_corpus(
        "report",
        &[
            ("aa-good", "pass"),
            ("bb-aborts", "abort"),
            ("cc-hangs", "hang"),
        ],
    );
    let results = corpus::run_files(&stub(), &dir, &files, Duration::from_secs(5), 3);

    let run = Run {
        corpora: vec![CorpusReport {
            name: "synthetic".to_string(),
            files: results,
        }],
        limits: Vec::new(),
        settings: Settings {
            timeout_seconds: 1,
            dpi: 72.0,
            fonts: "none".to_string(),
        },
    };

    assert!(run.complete(), "nothing was skipped");
    let text = run.to_report_json().to_pretty();
    assert!(text.contains("\"path\": \"aa-good.pdf\""), "{text}");
    assert!(text.contains("\"outcome\": \"crashed\""), "{text}");
    assert!(text.contains("\"outcome\": \"timed_out\""), "{text}");
    assert!(text.contains("\"complete\": true"), "{text}");

    let outcomes = run.corpora[0].outcomes();
    assert_eq!(outcomes["passed"], 1);
    assert_eq!(outcomes["crashed"], 1);
    assert_eq!(outcomes["timed_out"], 1);

    let _ = std::fs::remove_dir_all(&dir);
}

/// A corpus directory that is not there produces a loud absence, never a
/// quiet zero. This is the shape of the CI failure gap 23's risk table is
/// most worried about: a job that finds no corpus and reports success.
#[test]
fn a_corpus_directory_that_is_not_there_yields_no_files() {
    let missing = std::env::temp_dir().join("tinker-corpus-that-is-not-there");
    let _ = std::fs::remove_dir_all(&missing);
    assert!(corpus::pdfs_under(&missing).is_empty());

    // And the ratchet refuses to be cleared by it — see
    // `ratchet::tests::a_corpus_that_ran_nothing_is_refused_rather_than_passed`
    // for the comparison itself. Here the point is only that the runner
    // reports zero rather than erroring out, so that the refusal happens
    // where it can carry a reason.
    let results = corpus::run_files(&stub(), &missing, &[], Duration::from_secs(1), 1);
    assert!(results.is_empty());
}

/// The child is spawned once per file, so a file cannot leave state behind
/// for the next one. Proved by the one thing shared state would break: an
/// aborting file sitting between two good ones changes neither.
#[test]
fn one_bad_file_does_not_affect_its_neighbours() {
    let (dir, files) = synthetic_corpus(
        "neighbours",
        &[
            ("aa-good", "pass"),
            ("bb-abort", "abort"),
            ("cc-good", "pass"),
            ("dd-hang", "hang"),
            ("ee-good", "pass"),
        ],
    );
    let results = corpus::run_files(&stub(), &dir, &files, Duration::from_secs(5), 1);
    let passed = results
        .iter()
        .filter(|r| r.outcome == Outcome::Passed)
        .count();
    assert_eq!(passed, 3, "{results:#?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The synthetic files are PDFs, and the real child reads them as such.
///
/// Without this the fixtures could drift into being marker files that only
/// the stub understands, and the isolation tests would stop saying anything
/// about a corpus run. Skipped rather than failed when `tpdf` has not been
/// built — and it says so, because a test that skips silently is the thing
/// this repository has twice had to fix in CI.
#[test]
fn the_synthetic_files_are_real_pdfs() {
    let exe = std::env::current_exe().expect("this test binary's path");
    // `target/<profile>/deps/<test>-<hash>.exe`, so `tpdf` is two levels up.
    let Some(profile) = exe.parent().and_then(Path::parent) else {
        panic!("this test binary is not where cargo puts test binaries");
    };
    let tpdf = profile.join(if cfg!(windows) { "tpdf.exe" } else { "tpdf" });
    if !tpdf.exists() {
        println!(
            "corpus-isolation: SKIPPED the real-child check — {} does not \
             exist; build it with `cargo build -p tpdf`",
            tpdf.display()
        );
        return;
    }

    let (dir, files) = synthetic_corpus("real", &[("aa-good", "pass"), ("bb-abort", "abort")]);
    let child = Child {
        program: tpdf,
        args: vec!["probe".to_string(), "--dpi".to_string(), "72".to_string()],
    };
    let results = corpus::run_files(&child, &dir, &files, Duration::from_secs(20), 2);

    assert_eq!(results.len(), 2);
    for result in &results {
        assert_eq!(
            result.outcome,
            Outcome::Passed,
            "the engine opens and renders both of these; the marker is a PDF \
             comment and means nothing to it: {result:#?}"
        );
        assert_eq!(result.pages, 1);
        assert_eq!(result.rendered, 1);
    }
    println!("corpus-isolation: RAN the real-child check");
    let _ = std::fs::remove_dir_all(&dir);
}
