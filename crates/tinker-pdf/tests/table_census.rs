//! The table census over the fetched corpora
//! (`docs/design/table-reconstruction.md`, milestone 1 and on).
//!
//! The design's adjudicator: every tagged file that carries a `Table`
//! element, read as the producer stated it. Milestone 1 counts the family —
//! files with a `Table`, `Table`, `TR`, `TH` and `TD` elements and files with
//! a `TH`, per corpus, by `standard_type` after the role map — and reads every
//! stated table on the pages it scores, tallying the spans that do not add up.
//! Milestone 2 adds the most rules any scored page draws, which is the figure
//! `MAX_TABLE_RULES` was to be sized from, and the pages past it.
//!
//! ```text
//! cargo xtask corpus-fetch
//! TINKER_CORPUS=$PWD/corpus/files TINKER_CORPUS_REQUIRED=1 \
//!   cargo test --release -p tinker-pdf --test table_census -- --ignored --nocapture
//! ```
//!
//! **`RAN`/`SKIPPED` is printed on the first line.** With
//! `TINKER_CORPUS_REQUIRED` set a missing corpus fails rather than skips.
//!
//! # What is held, and what is owed
//!
//! The design records a scratch walk of 16 September 2026 — 207 files with a
//! `Table`, 1 908 tables, 74 474 `TD`s — and its milestone 1 asks this test
//! to re-derive them. **It has not run anywhere yet**: the fetched corpora
//! were not reachable where it was written. So the design's figures are
//! printed beside what the run measures, as `DESIGN_RECORDED`, and are not
//! asserted; the first nightly run either confirms them or says by how much
//! they were wrong, and the floors follow from it. Asserted whatever the
//! corpus holds: every stated table reads without a panic, and every cell the
//! join gives characters to spells its text with them.
//!
//! # Ruling 13
//!
//! The corpora supply bytes; every number here is this engine reading them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::{Document, TableWarning};

/// Printed once when the census read the corpora. CI greps it.
const RAN: &str = "table-census: RAN";

/// Printed once when it could not.
const SKIPPED: &str = "table-census: SKIPPED";

/// Pages read per file, for the per-page readers; the element counts are
/// over the whole tree.
const PAGES_PER_FILE: u32 = 16;

/// The design's scratch walk, per corpus: files with a `Table`, `Table`,
/// `TR`, `TH`, `TD`, files with a `TH`. Printed for comparison, not held.
const DESIGN_RECORDED: &[(&str, [usize; 6])] = &[
    ("safedocs", [145, 1_836, 14_880, 3_052, 73_698, 44]),
    ("verapdf", [49, 49, 175, 204, 274, 49]),
    ("pdfjs", [13, 23, 180, 29, 502, 5]),
];

/// The PDF/UA fixtures whose spans their own outlines state in words, by the
/// file-name stem the design lists.
const SPAN_FIXTURES: &[&str] = &[
    "7.2-t15-pass-a",
    "7.2-t41-fail-a",
    "7.2-t42-fail-a",
    "7.2-t43-fail-a",
    "8.2.5.26-t01-pass-a",
    "8.2.5.26-t03-fail-a",
    "8.2.5.26-t03-fail-b",
    "8.2.5.26-t04-fail-a",
];

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

/// Every `.pdf` under `root`, in path order, with its corpus.
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
    /// Files with a `Table`, `Table`, `TR`, `TH`, `TD`, files with a `TH`.
    family: [usize; 6],
    /// Stated tables read on the scored pages.
    stated: usize,
    inconsistent: usize,
    ragged: usize,
    /// The most rules one scored page draws, which `MAX_TABLE_RULES` was to
    /// be sized from, and the pages past that cap.
    most_rules: usize,
    over_cap: usize,
}

#[test]
#[ignore = "reads the fetched corpora; set TINKER_CORPUS=corpus/files"]
fn every_stated_table_in_the_corpora_is_counted_and_read() {
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
        let Some(tree) = doc.structure() else {
            continue;
        };
        let mut counts = [0usize; 4];
        for element in tree.elements() {
            match element.standard_type.as_str() {
                "Table" => counts[0] += 1,
                "TR" => counts[1] += 1,
                "TH" => counts[2] += 1,
                "TD" => counts[3] += 1,
                _ => {}
            }
        }
        if counts[0] == 0 {
            continue;
        }
        totals.family[0] += 1;
        for (at, count) in counts.iter().enumerate() {
            totals.family[at + 1] += count;
        }
        if counts[2] > 0 {
            totals.family[5] += 1;
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let span_fixture = SPAN_FIXTURES.iter().any(|f| stem.contains(f));
        for index in 0..doc.page_count().min(PAGES_PER_FILE) {
            let Some(page) = doc.page(index) else {
                continue;
            };
            let rules = page.table_rules();
            let drawn = rules
                .warnings
                .iter()
                .find_map(|w| match w {
                    TableWarning::TooManyRules { drawn } => Some(*drawn),
                    _ => None,
                })
                .unwrap_or(rules.rules.len());
            totals.most_rules = totals.most_rules.max(drawn);
            if drawn > tinker_pdf::tables::MAX_TABLE_RULES {
                totals.over_cap += 1;
            }
            for table in page.stated_tables() {
                totals.stated += 1;
                for warning in &table.warnings {
                    match warning {
                        TableWarning::SpanInconsistent { .. } => totals.inconsistent += 1,
                        TableWarning::RaggedRows { .. } => totals.ragged += 1,
                        _ => {}
                    }
                }
                for cell in &table.cells {
                    if !cell.chars.is_empty() {
                        let spelled: String = cell.chars.iter().map(|c| c.text.as_str()).collect();
                        assert_eq!(
                            spelled,
                            cell.text,
                            "{}: a cell's text is not its characters",
                            path.display()
                        );
                    }
                }
                if span_fixture {
                    let spans: Vec<String> = table
                        .cells
                        .iter()
                        .filter(|c| c.row_span > 1 || c.col_span > 1)
                        .map(|c| format!("({},{}) {}x{}", c.row, c.column, c.row_span, c.col_span))
                        .collect();
                    println!(
                        "span fixture {stem}: {}x{} table, spans {spans:?}, warnings {:?}",
                        table.rows, table.columns, table.warnings
                    );
                }
            }
        }
    }

    println!();
    println!(
        "{:<14} {:>6} {:>8} {:>7} {:>7} {:>7} {:>8} {:>8} {:>7} {:>6} {:>6}",
        "corpus",
        "files",
        "w/Table",
        "Table",
        "TR",
        "TH",
        "TD",
        "w/TH",
        "stated",
        "spans",
        "ragged"
    );
    let mut total = [0usize; 6];
    for (name, t) in &per_corpus {
        println!(
            "{name:<14} {:>6} {:>8} {:>7} {:>7} {:>7} {:>8} {:>8} {:>7} {:>6} {:>6}",
            t.files,
            t.family[0],
            t.family[1],
            t.family[2],
            t.family[3],
            t.family[4],
            t.family[5],
            t.stated,
            t.inconsistent,
            t.ragged
        );
        println!(
            "{:<14} most rules on one page {}, pages past MAX_TABLE_RULES {}",
            "", t.most_rules, t.over_cap
        );
        if let Some((_, recorded)) = DESIGN_RECORDED.iter().find(|(c, _)| c == name) {
            println!("{:<14} design's 16 September walk: {recorded:?}", "");
        }
        for (at, count) in t.family.iter().enumerate() {
            total[at] += count;
        }
    }
    println!("total family {total:?} (the design recorded [207, 1908, 15235, 3285, 74474, 98])");
}
