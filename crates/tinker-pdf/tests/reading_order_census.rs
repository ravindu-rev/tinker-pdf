//! The reading-order census over the fetched corpora
//! (`docs/design/reading-order.md`, milestone 2 and on).
//!
//! The design's adjudicator: every tagged file the corpora carry, its
//! structure tree hidden, the inference run over the same `TextPage`, and the
//! two orders compared. The tree is a statement somebody other than this
//! project wrote about how the page reads (14.8), which is the only
//! third-party statement about reading order that exists at this scale.
//!
//! ```text
//! cargo xtask corpus-fetch
//! TINKER_CORPUS=$PWD/corpus/files TINKER_CORPUS_REQUIRED=1 \
//!   cargo test --release -p tinker-pdf --test reading_order_census -- --ignored --nocapture
//! ```
//!
//! `TINKER_CORPUS` names the directory holding one subdirectory per corpus;
//! the default is `corpus/files` beside the workspace. With
//! `TINKER_CORPUS_REQUIRED` set a missing corpus fails rather than skips.
//! **`RAN`/`SKIPPED` is printed on the first line**, because a census whose
//! corpus is missing is otherwise a passing test that measured nothing.
//!
//! # What is printed, and what is held
//!
//! Per corpus: files, tagged files, pages scored, and the **pair agreement**
//! of the content stream's order and of the inference's with the tree, both
//! as integers (`agreeing` of `pairs`) so a floor can be compared by
//! cross-multiplication the way `corpus/ratchet.json`'s bars are; the number
//! of tagged files whose stream already agrees exactly (which carry no
//! information about columns); the **column crossings** walking the tree over
//! the files that remain; and the characters the inference moved, which over
//! veraPDF's fixtures the design expects to be zero. And **running-head
//! precision**: of the blocks called a running head, foot or page number, how
//! many the producer drew as an artifact in a margin band — recall printed
//! beside it, and neither yet held, for the reason below. **Footnote
//! precision** likewise, against the producer's `/Note` elements.
//!
//! **No corpus figure has been measured yet.** The fetched corpora were not
//! reachable where this was written, so the floors table below is empty and
//! the roadmap row says the corpus score is owed. What is asserted whatever
//! the corpus holds: the population is split by corpus name as the design
//! requires — pdfjs and SafeDocs are the ratchet population, veraPDF's
//! one-paragraph fixtures the set where nothing may move — and every
//! inferred order is a permutation of the page it was inferred over.
//!
//! # Ruling 13
//!
//! The corpora supply bytes; every number here is this engine reading them.
//! The answer key is what each file's producer wrote into it, not another
//! program's verdict about it.

mod reading_order_support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use reading_order_support::{furniture_truth, note_keys, role_score, Agreement, RoleScore, Scored};
use tinker_pdf::{Document, Role};

/// Printed once when the census read the corpora. CI greps it.
const RAN: &str = "reading-order-census: RAN";

/// Printed once when it could not.
const SKIPPED: &str = "reading-order-census: SKIPPED";

/// Pages read per file. The design's K: a bound, so a document claiming a
/// hundred thousand pages is a measurement and not a timeout.
const PAGES_PER_FILE: u32 = 16;

/// The corpora whose tagged files are the population the scores stand for.
const RATCHET_POPULATION: &[&str] = &["pdfjs", "safedocs"];

/// The corpus whose tagged files are one-paragraph conformance fixtures:
/// where the inference must not move anything, because there is nothing to
/// move, and never part of an average.
const NOTHING_MAY_MOVE: &str = "verapdf";

/// Floors on the inference's pair agreement, as `(corpus, agreeing, pairs)`
/// — a fraction compared by cross-multiplication.
///
/// **Empty, because nothing has been measured**: the first nightly run of
/// `corpus.yml` prints the figures, and they are recorded here, with the
/// date, by the commit that reads them.
const INFERRED_FLOORS: &[(&str, u64, u64)] = &[];

fn required() -> bool {
    std::env::var_os("TINKER_CORPUS_REQUIRED").is_some_and(|value| value != "0")
}

fn corpus_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TINKER_CORPUS") {
        let path = PathBuf::from(path);
        return path.is_dir().then_some(path);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/files");
    root.canonicalize().ok().filter(|path| path.is_dir())
}

/// Every `.pdf` under `root`, in path order, with the corpus it belongs to
/// (the first directory under the root).
fn corpus(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            {
                let name = path
                    .strip_prefix(root)
                    .ok()
                    .and_then(|rest| rest.components().next())
                    .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
                    .unwrap_or_else(|| "?".to_string());
                out.push((name, path));
            }
        }
    }
    out.sort();
    out
}

fn open(bytes: Vec<u8>) -> Option<Document> {
    let document = Document::open(bytes).ok()?;
    if document.is_encrypted() {
        let _ = document.authenticate("");
    }
    Some(document)
}

/// What one corpus contributed.
#[derive(Default)]
struct Totals {
    files: usize,
    tagged: usize,
    multi_page: usize,
    pages: usize,
    stream: Agreement,
    inferred: Agreement,
    /// Tagged files whose stream agrees with the tree on every scored page.
    stream_exact: usize,
    /// Characters the inference put somewhere other than their stream
    /// position.
    moved: usize,
    /// Column crossings walking the tree, over the files whose stream does
    /// *not* already agree exactly — the design's column score, computed
    /// where it carries information.
    crossings: usize,
    /// The files those crossings were counted over.
    informative: usize,
    /// Running heads, feet and page numbers against the artifacts the
    /// producer drew in the margin bands, over the pages that have any.
    furniture: RoleScore,
    /// The pages that had any.
    furnished: usize,
    /// Footnotes against the producer's `/Note` elements, over the pages
    /// whose tree has any.
    notes: RoleScore,
    /// The pages that had any.
    noted: usize,
}

/// Scores one tagged file into `totals`.
fn score(doc: &Document, totals: &mut Totals) {
    let pages = doc.page_count().min(PAGES_PER_FILE);
    let mut file_stream = Agreement::default();
    let mut file_crossings = 0usize;
    for index in 0..pages {
        let Some(scored) = Scored::read(doc, index) else {
            continue;
        };
        let n = scored.inferred.permutation.len();
        let mut seen = vec![false; n];
        for at in &scored.inferred.permutation {
            assert!(
                *at < n && !seen[*at],
                "page {index}: the inferred order is not a permutation"
            );
            seen[*at] = true;
        }
        let stream = scored.stream_agreement();
        totals.pages += 1;
        totals.stream = totals.stream.plus(stream);
        totals.inferred = totals.inferred.plus(scored.inferred_agreement());
        totals.moved += scored.inferred.moved();
        file_crossings += scored.crossings();
        let notes = note_keys(doc, index);
        if !notes.is_empty() {
            totals.noted += 1;
            totals.notes =
                totals
                    .notes
                    .plus(role_score(&scored.inferred, &[Role::Footnote], &notes));
        }
        let truth = furniture_truth(doc, index);
        if !truth.is_empty() {
            totals.furnished += 1;
            totals.furniture = totals.furniture.plus(role_score(
                &scored.inferred,
                &[Role::RunningHead, Role::RunningFoot, Role::PageNumber],
                &truth,
            ));
        }
        file_stream = file_stream.plus(stream);
    }
    if file_stream.at_least(1, 1) {
        totals.stream_exact += 1;
    } else {
        totals.informative += 1;
        totals.crossings += file_crossings;
    }
}

#[test]
#[ignore = "reads the fetched corpora; set TINKER_CORPUS=corpus/files"]
fn the_inference_is_scored_against_every_tagged_files_own_order() {
    let Some(root) = corpus_root() else {
        println!("{SKIPPED} (no corpus; set TINKER_CORPUS)");
        assert!(
            !required(),
            "TINKER_CORPUS_REQUIRED is set and there is no corpus at TINKER_CORPUS: \
             this census would have passed over nothing"
        );
        return;
    };
    let files = corpus(&root);
    if files.is_empty() {
        println!("{SKIPPED} ({} holds no PDFs)", root.display());
        assert!(
            !required(),
            "TINKER_CORPUS_REQUIRED is set and the corpus is empty"
        );
        return;
    }
    println!("{RAN} ({} files under {})", files.len(), root.display());

    let mut per_corpus: BTreeMap<String, Totals> = BTreeMap::new();
    for (name, path) in &files {
        let totals = per_corpus.entry(name.clone()).or_default();
        totals.files += 1;
        let Some(doc) = std::fs::read(path).ok().and_then(open) else {
            continue;
        };
        if doc.structure().is_none() {
            continue;
        }
        totals.tagged += 1;
        if doc.page_count() > 1 {
            totals.multi_page += 1;
        }
        score(&doc, totals);
    }

    println!();
    println!(
        "{:<14} {:>6} {:>6} {:>6} {:>6} {:>22} {:>22} {:>6} {:>8}",
        "corpus",
        "files",
        "tagged",
        "multi",
        "pages",
        "stream agree/pairs",
        "inferred agree/pairs",
        "exact",
        "moved"
    );
    for (name, t) in &per_corpus {
        let role = if RATCHET_POPULATION.contains(&name.as_str()) {
            "  (ratchet population)"
        } else if name == NOTHING_MAY_MOVE {
            "  (where nothing may move)"
        } else {
            ""
        };
        println!(
            "{name:<14} {:>6} {:>6} {:>6} {:>6} {:>22} {:>22} {:>6} {:>8}{role}",
            t.files,
            t.tagged,
            t.multi_page,
            t.pages,
            format!("{}/{}", t.stream.agreeing, t.stream.pairs),
            format!("{}/{}", t.inferred.agreeing, t.inferred.pairs),
            t.stream_exact,
            t.moved,
        );
        println!(
            "{:<14} stream {:.6}  inferred {:.6}  crossings {} over {} files the stream does not already read",
            "",
            t.stream.score(),
            t.inferred.score(),
            t.crossings,
            t.informative
        );
        println!(
            "{:<14} furniture: {} of {} blocks called were marked; {} of {} marked characters found, over {} pages",
            "",
            t.furniture.correct,
            t.furniture.called,
            t.furniture.found,
            t.furniture.truth,
            t.furnished
        );
        println!(
            "{:<14} footnotes: {} of {} blocks called were /Note; {} of {} /Note characters found, over {} pages",
            "",
            t.notes.correct,
            t.notes.called,
            t.notes.found,
            t.notes.truth,
            t.noted
        );
    }

    let tagged: usize = per_corpus.values().map(|t| t.tagged).sum();
    assert!(
        tagged > 0,
        "the corpora hold no tagged file this engine could read"
    );
    for (corpus, agreeing, pairs) in INFERRED_FLOORS {
        let t = per_corpus
            .get(*corpus)
            .unwrap_or_else(|| panic!("no corpus named {corpus}"));
        assert!(
            u128::from(t.inferred.agreeing) * u128::from(*pairs)
                >= u128::from(*agreeing) * u128::from(t.inferred.pairs),
            "{corpus}: the inference's agreement fell below its floor"
        );
    }
}
