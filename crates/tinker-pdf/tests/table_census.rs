//! The table census over the fetched corpora
//! (`docs/design/table-reconstruction.md`, milestone 1 and on).
//!
//! The design's adjudicator: every tagged file that carries a `Table`
//! element, read as the producer stated it. Milestone 1 counts the family —
//! files with a `Table`, `Table`, `TR`, `TH` and `TD` elements and files with
//! a `TH`, per corpus, by `standard_type` after the role map — and reads every
//! stated table on the pages it scores, tallying the spans that do not add up.
//! Milestone 2 adds the most rules any scored page draws, which is the figure
//! `MAX_TABLE_RULES` was to be sized from, and the pages past it. Milestone 3
//! adds the design's scores, the tree hidden: stated tables an inferred one
//! was found over, grid agreement, cell assignment, and extra tables on pages
//! that state none — printed for SafeDocs and pdfjs, where producers
//! under-tag, and for veraPDF, where the design expects zero. **None of these
//! is held**: no figure has been measured, and the floors are owed to the
//! first nightly run.
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

use tinker_pdf::{
    Document, HeaderEvidence, InferredTable, StatedTable, TableEvidence, TableOptions, TableWarning,
};

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
    /// Pages whose lattices' frames cross, so that none of those was read.
    crossed: usize,
    /// The design's scores, the tree hidden: stated tables an inferred one
    /// was found over, of those the ones whose grid matched exactly, and the
    /// stated cells' characters placed in the same row and column, of all.
    found: usize,
    grid: usize,
    placed: usize,
    placeable: usize,
    /// The same, by the evidence the found table was built on — never
    /// averaged together, as the design asks: `[ruled, aligned]`.
    found_by: [usize; 2],
    grid_by: [usize; 2],
    /// Inferred tables on scored pages whose tree states none.
    extra: usize,
    /// Inferred tables found over a stated one whose first row is all `TH`,
    /// and of those the ones whose first row the ink sets apart.
    headed: usize,
    header_evidence: usize,
}

/// A character's identity across two readings of one page.
type Key = (u64, u64, String);

fn key(c: &tinker_pdf::TextChar) -> Key {
    (c.origin.0.to_bits(), c.origin.1.to_bits(), c.text.clone())
}

/// The design's scores for one stated table against the inferred ones —
/// `tables.rs`'s, for the corpus: found (an inferred table covering at least
/// half the stated one's characters' quad), the grid exact, and the stated
/// cells' characters placed in the same row and column, of how many.
///
/// With the evidence of the table found, so `Ruled` and `Aligned` are scored
/// apart.
fn score_by(
    stated: &StatedTable,
    inferred: &[InferredTable],
) -> (bool, bool, usize, usize, Option<TableEvidence>) {
    let total: usize = stated.cells.iter().map(|c| c.chars.len()).sum();
    let Some(quad) = stated.quad else {
        return (false, false, 0, total, None);
    };
    let (sx0, sy0, sx1, sy1) = quad.bounds();
    let found = inferred.iter().find(|t| {
        let (x0, y0, x1, y1) = t.bounds.bounds();
        let overlap = (sx1.min(x1) - sx0.max(x0)).max(0.0) * (sy1.min(y1) - sy0.max(y0)).max(0.0);
        overlap * 2.0 >= (sx1 - sx0) * (sy1 - sy0)
    });
    let Some(table) = found else {
        return (false, false, 0, total, None);
    };
    let grid = table.rows == stated.rows && table.columns == stated.columns;
    let mut place = BTreeMap::new();
    for cell in &table.cells {
        for c in &cell.chars {
            place.insert(key(c), (cell.row, cell.column));
        }
    }
    let placed = stated
        .cells
        .iter()
        .flat_map(|cell| cell.chars.iter().map(move |c| (cell, c)))
        .filter(|(cell, c)| place.get(&key(c)) == Some(&(cell.row, cell.column)))
        .count();
    (true, grid, placed, total, Some(table.evidence))
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
            let stated = page.stated_tables();
            let inferred = page.inferred_tables(&TableOptions {
                hide_structure: true,
            });
            if stated.is_empty() {
                totals.extra += inferred.tables.len();
            }
            if inferred
                .warnings
                .iter()
                .any(|w| matches!(w, TableWarning::LatticesCross { .. }))
            {
                totals.crossed += 1;
            }
            for table in &stated {
                let (found, grid, placed, total, evidence) = score_by(table, &inferred.tables);
                let class = usize::from(evidence == Some(TableEvidence::Aligned));
                if found {
                    if let Some(slot) = totals.found_by.get_mut(class) {
                        *slot += 1;
                    }
                }
                if grid {
                    if let Some(slot) = totals.grid_by.get_mut(class) {
                        *slot += 1;
                    }
                }
                let th_row = table.cells.iter().filter(|c| c.row == 0).all(|c| c.header)
                    && table.cells.iter().any(|c| c.row == 0);
                if found && th_row {
                    totals.headed += 1;
                    if inferred.tables.iter().any(|t| {
                        matches!(
                            t.header,
                            HeaderEvidence::FillBeneath | HeaderEvidence::RuleBeneath
                        )
                    }) {
                        totals.header_evidence += 1;
                    }
                }
                totals.found += usize::from(found);
                totals.grid += usize::from(grid);
                totals.placed += placed;
                totals.placeable += total;
            }
            for table in stated {
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
                    for t in &inferred.tables {
                        let spans: Vec<String> = t
                            .cells
                            .iter()
                            .filter(|c| c.row_span > 1 || c.col_span > 1)
                            .map(|c| {
                                format!("({},{}) {}x{}", c.row, c.column, c.row_span, c.col_span)
                            })
                            .collect();
                        println!(
                            "span fixture {stem}, inferred: {}x{} table, spans {spans:?}, warnings {:?}",
                            t.rows, t.columns, t.warnings
                        );
                    }
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
            "{:<14} most rules on one page {}, pages past MAX_TABLE_RULES {}, pages whose lattices cross {}",
            "", t.most_rules, t.over_cap, t.crossed
        );
        println!(
            "{:<14} inferred, tree hidden: found {} of {} stated, grid {} of those, cells {}/{}; {} extra tables",
            "", t.found, t.stated, t.grid, t.placed, t.placeable, t.extra
        );
        println!(
            "{:<14} header rows (all TH) found {}, set apart by fill or rule {}",
            "", t.headed, t.header_evidence
        );
        println!(
            "{:<14} by evidence: ruled found {} grid {}; aligned found {} grid {}",
            "", t.found_by[0], t.grid_by[0], t.found_by[1], t.grid_by[1]
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
