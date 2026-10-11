//! Every PDF/X claim in the fetched corpora, read through
//! `Document::validate_pdfx` (`docs/design/pdfx.md`, milestone 1's census).
//!
//! ```text
//! cargo xtask corpus-fetch
//! TINKER_CORPUS=<abs path to corpus/files> TINKER_CORPUS_REQUIRED=1 \
//!   cargo test --release -p tinker-pdf --test pdfx_census -- --ignored --nocapture
//! ```
//!
//! With `TINKER_CORPUS` unset the census looks for `corpus/files` beside the
//! workspace, which is where `cargo xtask corpus-fetch` writes and where the
//! nightly `corpus.yml` leaves it.
//!
//! # What this can measure, which is one direction only
//!
//! No annotated PDF/X conformance corpus exists, and none of the five
//! corpora `corpus/corpora.lock` pins is a PDF/X suite. What they carry is
//! real producers' claims — the design counted 23 files with a
//! `GTS_PDFXVersion` key by a raw byte search, a floor — and, in every other
//! file, no claim at all. So the census holds two things and prints the rest:
//!
//! - **a file that claims nothing gets an empty verdict** — no finding, no
//!   abstention, no group run — over every file in every corpus, because a
//!   PDF/X rule that fired on a file with no claim would be enforcing a
//!   standard nobody asked for;
//! - **a file claiming a level this build validates has no finding**, unless
//!   [`EXPLAINED`] names it with a reason. A real producer's claim is not an
//!   annotation and a finding on one may be right, but nobody can tell until
//!   somebody reads the file, and a finding nobody has read is a false
//!   positive until shown otherwise — the design's "zero findings over the
//!   real claims or a ledger row per finding with a reason".
//!
//! Every claim is printed with the flavour this build reads it as, which is
//! the milestone's exit criterion. **No count here is a floor**: the census
//! was written where the corpora are not reachable, and the first nightly run
//! owes the numbers. The design's own reading of the version strings found
//! `PDF/X-4`, `PDF/X-3:2002`, `PDF/X-1:2001` and an empty one — none a level
//! these rules run under — so the second assertion may hold vacuously, and the
//! census says how many files it held over rather than letting that pass for
//! a measurement.
//!
//! # Skipped, not silently passed
//!
//! A test over bytes that are not in the repository can fail to run for a
//! reason that looks exactly like a pass, so it prints [`RAN`] or [`SKIPPED`]
//! and a job depending on it greps its own output for the second; with
//! `TINKER_CORPUS_REQUIRED` set, a skip is a failure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::{Document, PdfXCoverage};

/// Printed once when the census ran. CI greps for it.
const RAN: &str = "pdfx-census: RAN";

/// Printed once when it could not. CI greps for this one and fails.
const SKIPPED: &str = "pdfx-census: SKIPPED";

/// Real files claiming a validated level whose findings somebody has read,
/// as `(file name, reason)`. Empty: no such file has been seen yet.
const EXPLAINED: &[(&str, &str)] = &[];

/// Whether a missing corpus is a failure rather than a skip.
fn required() -> bool {
    std::env::var_os("TINKER_CORPUS_REQUIRED").is_some_and(|value| value != "0")
}

/// The fetched corpora: `TINKER_CORPUS`, or the fetch directory beside the
/// workspace.
fn corpus_root() -> Option<PathBuf> {
    let base = match std::env::var("TINKER_CORPUS") {
        Ok(path) => PathBuf::from(path),
        Err(_) => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/files")
            .canonicalize()
            .ok()?,
    };
    base.is_dir().then_some(base)
}

/// Every `.pdf` under `root`, recursively.
fn pdfs_under(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            pdfs_under(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
        {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "reads the fetched corpora; set TINKER_CORPUS=corpus/files"]
fn every_pdfx_claim_in_the_fetched_corpora_is_read_and_named() {
    let Some(root) = corpus_root() else {
        println!("{SKIPPED} the PDF/X census -- no corpus at TINKER_CORPUS or corpus/files");
        assert!(
            !required(),
            "TINKER_CORPUS_REQUIRED is set and the corpus is not there"
        );
        return;
    };
    let mut files = Vec::new();
    pdfs_under(&root, &mut files);
    files.sort();
    println!("{RAN} the PDF/X census over {} files", files.len());

    let mut unopened = 0usize;
    let mut unclaimed = 0usize;
    let mut by_version: BTreeMap<String, usize> = BTreeMap::new();
    let mut validated = 0usize;
    let mut unexplained: Vec<String> = Vec::new();
    let mut judged_unclaimed: Vec<String> = Vec::new();

    for path in &files {
        let Ok(bytes) = std::fs::read(path) else {
            unopened += 1;
            continue;
        };
        let Ok(doc) = Document::open(bytes) else {
            unopened += 1;
            continue;
        };
        let name = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        let verdict = doc.validate_pdfx();
        let Some(claim) = &verdict.claim else {
            unclaimed += 1;
            if !verdict.findings.is_empty()
                || !verdict.abstained.is_empty()
                || verdict.coverage != PdfXCoverage::default()
            {
                judged_unclaimed.push(name);
            }
            continue;
        };
        *by_version.entry(claim.version.clone()).or_default() += 1;
        let read_as = verdict
            .flavour
            .map_or("a level this build does not identify".to_string(), |f| {
                f.to_string()
            });
        println!(
            "  {name}: GTS_PDFXVersion {:?}, GTS_PDFXConformance {:?} -> {read_as}; \
             {} findings, {} abstentions, ran {}",
            claim.version,
            claim.conformance,
            verdict.findings.len(),
            verdict.abstained.len(),
            verdict.coverage
        );
        for finding in &verdict.findings {
            println!("      {finding}");
        }
        if verdict.flavour.is_some_and(|f| f.is_validated()) {
            validated += 1;
            let file = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            if !verdict.findings.is_empty() && !EXPLAINED.iter().any(|(f, _)| *f == file) {
                unexplained.push(name);
            }
        }
    }

    println!("pdfx-census: {unopened} files did not open, {unclaimed} claim nothing");
    for (version, count) in &by_version {
        println!("pdfx-census: {count:>5} claim {version:?}");
    }
    println!(
        "pdfx-census: {validated} claim a level this build validates; the \
         no-finding bar below holds over that many files and no more"
    );

    assert!(
        judged_unclaimed.is_empty(),
        "a file claiming no PDF/X level was judged: {judged_unclaimed:#?}"
    );
    assert!(
        unexplained.is_empty(),
        "a real claim to a validated level has findings nobody has read; read \
         the file, then fix the rule or add it to EXPLAINED with the reason: \
         {unexplained:#?}"
    );
}
