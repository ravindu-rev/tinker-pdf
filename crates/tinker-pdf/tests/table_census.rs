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
//! was found over, grid agreement, cell assignment — cell by cell against the
//! stated `TH`s and `TD`s, a character placed only in a cell at its stated
//! row and column **with its stated spans** — the stated spans reproduced,
//! and extra tables on pages that state none: printed for SafeDocs and pdfjs,
//! where producers under-tag, and **asserted zero for veraPDF**, where
//! everything is tagged. Header evidence is read off the table found over
//! each stated one. **No floor is held**: no figure has been measured, and
//! the floors are owed to the first nightly run.
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
//! corpus holds: every stated table reads without a panic, every cell the
//! join gives characters to spells its text with them, and no table is
//! inferred on a veraPDF page that states none ([`held`]) — the design's
//! zero, owed its first run like everything else here. The eight span
//! fixtures' verdicts (spans reproduced, `SpanInconsistent` named) are
//! printed by name.
//!
//! # Ruling 13
//!
//! The corpora supply bytes; every number here is this engine reading them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tinker_pdf::{
    Document, HeaderEvidence, InferredTable, InferredTables, StatedTable, TableAttributes,
    TableEvidence, TableOptions, TableWarning, Tag,
};
use tinker_pdf_cos::build::DocumentBuilder;

/// Printed once when the census read the corpora. CI greps it.
const RAN: &str = "table-census: RAN";

/// Printed once when it could not.
const SKIPPED: &str = "table-census: SKIPPED";

/// Pages read per file, for the per-page readers; the element counts are
/// over the whole tree.
const PAGES_PER_FILE: u32 = 16;

/// The corpus where everything is tagged, so that a table inferred on a page
/// stating none is a mistake: asserted zero.
const NO_EXTRAS: &str = "verapdf";

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
    /// stated cells' characters placed in the same row and column with the
    /// same spans, of all.
    found: usize,
    grid: usize,
    placed: usize,
    placeable: usize,
    /// Stated cells with a span, and of those the ones an inferred cell
    /// reproduces at the same place with the same spans.
    spans: usize,
    spans_reproduced: usize,
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

/// A cell's place in its table: row, column, row span, column span.
type Place = (usize, usize, usize, usize);

/// The design's scores for one stated table against the inferred ones.
struct Score<'a> {
    /// The inferred table found over it: one covering at least half the
    /// stated table's characters' quad.
    found: Option<&'a InferredTable>,
    /// Whether its rows and columns are the stated ones, exactly.
    grid: bool,
    /// The stated cells' characters the inference put in a cell at the same
    /// row and column **with the same spans**, of how many — so a span split
    /// into two cells, or two cells merged into one, misplaces what it holds.
    placed: usize,
    total: usize,
    /// The stated cells with a span, and of those the ones the found table
    /// has a cell for at the same place with the same spans.
    spans: usize,
    spans_reproduced: usize,
}

/// `tables.rs`'s scores, for the corpus: cell by cell against the stated
/// `TH`s and `TD`s, spans and all.
fn score_by<'a>(stated: &StatedTable, inferred: &'a [InferredTable]) -> Score<'a> {
    let place_of = |row, column, row_span, col_span| -> Place { (row, column, row_span, col_span) };
    let total: usize = stated.cells.iter().map(|c| c.chars.len()).sum();
    let spanned: Vec<Place> = stated
        .cells
        .iter()
        .filter(|c| c.row_span > 1 || c.col_span > 1)
        .map(|c| place_of(c.row, c.column, c.row_span, c.col_span))
        .collect();
    let mut score = Score {
        found: None,
        grid: false,
        placed: 0,
        total,
        spans: spanned.len(),
        spans_reproduced: 0,
    };
    let Some(quad) = stated.quad else {
        return score;
    };
    let (sx0, sy0, sx1, sy1) = quad.bounds();
    let found = inferred.iter().find(|t| {
        let (x0, y0, x1, y1) = t.bounds.bounds();
        let overlap = (sx1.min(x1) - sx0.max(x0)).max(0.0) * (sy1.min(y1) - sy0.max(y0)).max(0.0);
        overlap * 2.0 >= (sx1 - sx0) * (sy1 - sy0)
    });
    let Some(table) = found else {
        return score;
    };
    score.found = Some(table);
    score.grid = table.rows == stated.rows && table.columns == stated.columns;
    let mut place = BTreeMap::new();
    let mut cells = std::collections::BTreeSet::new();
    for cell in &table.cells {
        let at = place_of(cell.row, cell.column, cell.row_span, cell.col_span);
        cells.insert(at);
        for c in &cell.chars {
            place.insert(key(c), at);
        }
    }
    score.placed = stated
        .cells
        .iter()
        .flat_map(|cell| cell.chars.iter().map(move |c| (cell, c)))
        .filter(|(cell, c)| {
            place.get(&key(c))
                == Some(&place_of(
                    cell.row,
                    cell.column,
                    cell.row_span,
                    cell.col_span,
                ))
        })
        .count();
    score.spans_reproduced = spanned.iter().filter(|at| cells.contains(*at)).count();
    score
}

/// Scores one page's stated tables against the tables inferred over it with
/// the tree hidden, into `totals`.
fn score_page(stated: &[StatedTable], inferred: &InferredTables, totals: &mut Totals) {
    if stated.is_empty() {
        totals.extra += inferred.tables.len();
    }
    for table in stated {
        let score = score_by(table, &inferred.tables);
        let class = usize::from(score.found.map(|t| t.evidence) == Some(TableEvidence::Aligned));
        if score.found.is_some() {
            if let Some(slot) = totals.found_by.get_mut(class) {
                *slot += 1;
            }
        }
        if score.grid {
            if let Some(slot) = totals.grid_by.get_mut(class) {
                *slot += 1;
            }
        }
        let th_row = table.cells.iter().filter(|c| c.row == 0).all(|c| c.header)
            && table.cells.iter().any(|c| c.row == 0);
        if let (Some(found), true) = (score.found, th_row) {
            totals.headed += 1;
            // The table found over this one, not any table on the page.
            if matches!(
                found.header,
                HeaderEvidence::FillBeneath | HeaderEvidence::RuleBeneath
            ) {
                totals.header_evidence += 1;
            }
        }
        totals.found += usize::from(score.found.is_some());
        totals.grid += usize::from(score.grid);
        totals.placed += score.placed;
        totals.placeable += score.total;
        totals.spans += score.spans;
        totals.spans_reproduced += score.spans_reproduced;
    }
}

/// What the census holds whatever the corpus measures, as the invariants it
/// broke: **no table inferred on a veraPDF page that states none** — the
/// design's "asserted zero over the veraPDF table fixtures", where
/// everything is tagged and an extra table is a mistake.
fn held(per_corpus: &BTreeMap<String, Totals>) -> Vec<String> {
    let mut broken = Vec::new();
    if let Some(t) = per_corpus.get(NO_EXTRAS) {
        if t.extra != 0 {
            broken.push(format!(
                "{NO_EXTRAS}: {} tables inferred on pages that state none",
                t.extra
            ));
        }
    }
    broken
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
            if inferred
                .warnings
                .iter()
                .any(|w| matches!(w, TableWarning::LatticesCross { .. }))
            {
                totals.crossed += 1;
            }
            score_page(&stated, &inferred, totals);
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
                    // The design's milestone-4 verdicts, printed by name
                    // until a run has measured them: a `pass` file's spans
                    // reproduced, a `fail` file's refused by name.
                    let score = score_by(&table, &inferred.tables);
                    let named = table
                        .warnings
                        .iter()
                        .any(|w| matches!(w, TableWarning::SpanInconsistent { .. }));
                    println!(
                        "span fixture {stem}: stated spans reproduced {} of {}, cells {}/{}, SpanInconsistent named {named}",
                        score.spans_reproduced, score.spans, score.placed, score.total
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
            "{:<14} inferred, tree hidden: found {} of {} stated, grid {} of those, cells {}/{} (spans and all), stated spans reproduced {} of {}; {} extra tables",
            "", t.found, t.stated, t.grid, t.placed, t.placeable, t.spans_reproduced, t.spans, t.extra
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
    let broken = held(&per_corpus);
    assert!(broken.is_empty(), "{broken:#?}");
}

/// **The veraPDF zero is held, not printed.** A census whose veraPDF row
/// infers a table on a page that states none fails; every other corpus's
/// extras are printed and read, since producers there under-tag.
#[test]
fn the_census_holds_the_verapdf_zero() {
    let mut per_corpus: BTreeMap<String, Totals> = BTreeMap::new();
    per_corpus.insert(NO_EXTRAS.to_string(), Totals::default());
    per_corpus.insert("safedocs".to_string(), Totals::default());
    if let Some(t) = per_corpus.get_mut("safedocs") {
        t.extra = 12;
    }
    assert!(held(&per_corpus).is_empty());
    if let Some(t) = per_corpus.get_mut(NO_EXTRAS) {
        t.extra = 1;
    }
    assert_eq!(held(&per_corpus).len(), 1);
}

/// A one-page document drawing a 2 x 2 ruled grid at (72, 600), 100 points
/// by 20 a cell, every interior rule drawn, each of `cells` drawn at its
/// cell's top left and tagged as its `TH` or `TD` with the column span it
/// states — a stated span the ink rules as two cells. With `fill`, the
/// grid's first row is shaded and a second, untagged grid is drawn below it.
fn ruled_and_tagged(cells: &[&[(&str, u32, bool)]], second_with_fill: bool) -> Document {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| {
        let mut rules = String::new();
        let mut grid = |top: f64| {
            rules.push_str("0 g 0.5 w\n");
            for row in 0..=2 {
                let y = top - f64::from(row) * 20.0;
                rules.push_str(&format!("72 {y} m 272 {y} l S\n"));
            }
            for column in 0..=2 {
                let x = 72.0 + f64::from(column) * 100.0;
                rules.push_str(&format!("{x} {} m {x} {top} l S\n", top - 40.0));
            }
        };
        grid(600.0);
        if second_with_fill {
            grid(400.0);
            rules.push_str("0.85 g 72 380 200 20 re f 0 g\n");
        }
        page.raw(rules.as_bytes());
        page.tagged_with(&Tag::new(b"Table"), |page| {
            for (row, cells) in cells.iter().enumerate() {
                page.tagged_with(&Tag::new(b"TR"), |page| {
                    let mut column = 0u32;
                    for (text, col_span, header) in *cells {
                        let mut attributes = TableAttributes::default();
                        attributes.col_span = (*col_span > 1).then_some(*col_span);
                        let name: &[u8] = if *header { b"TH" } else { b"TD" };
                        let tag = if attributes.is_empty() {
                            Tag::new(name)
                        } else {
                            Tag::new(name).table(attributes)
                        };
                        let (x, y) = (76.0 + f64::from(column) * 100.0, 586.0 - row as f64 * 20.0);
                        page.tagged_with(&tag, |page| page.text(b"F1", 10.0, x, y, text));
                        column += col_span;
                    }
                });
            }
        });
        if second_with_fill {
            for (row, column) in [(0u32, 0u32), (0, 1), (1, 0), (1, 1)] {
                let (x, y) = (
                    76.0 + f64::from(column) * 100.0,
                    386.0 - f64::from(row) * 20.0,
                );
                page.text(b"F1", 10.0, x, y, "n");
            }
        }
    });
    open(builder.finish()).expect("the fixture opens")
}

/// The one page of `doc`, scored as the census scores it.
fn census_of(doc: &Document) -> Totals {
    let page = doc.page(0).expect("a page");
    let stated = page.stated_tables();
    let inferred = page.inferred_tables(&TableOptions {
        hide_structure: true,
    });
    let mut totals = Totals::default();
    score_page(&stated, &inferred, &mut totals);
    totals
}

/// **Cell assignment is scored with spans.** A stated header cell spanning
/// both columns, which the page rules as two: the grid agrees and every
/// character sits in row 0, column 0 — but not in a cell spanning two
/// columns, so the header's characters are misplaced and its span is not
/// reproduced. The same table stated without the span scores every
/// character placed.
#[test]
fn the_census_scores_cells_with_their_spans() {
    let spanned = census_of(&ruled_and_tagged(
        &[&[("Wide", 2, true)], &[("a", 1, false), ("b", 1, false)]],
        false,
    ));
    assert_eq!((spanned.found, spanned.grid), (1, 1));
    assert_eq!((spanned.placed, spanned.placeable), (2, 6));
    assert_eq!((spanned.spans_reproduced, spanned.spans), (0, 1));

    let plain = census_of(&ruled_and_tagged(
        &[
            &[("Wide", 1, true), ("x", 1, true)],
            &[("a", 1, false), ("b", 1, false)],
        ],
        false,
    ));
    assert_eq!((plain.found, plain.grid), (1, 1));
    assert_eq!((plain.placed, plain.placeable), (7, 7));
    assert_eq!((plain.spans_reproduced, plain.spans), (0, 0));
}

/// **Header evidence is the found table's.** A stated table whose first row
/// is all `TH`, unshaded, on a page with a second, untagged grid whose first
/// row is shaded: the header is counted, and its evidence is not — the fill
/// is under another table.
#[test]
fn the_census_reads_header_evidence_off_the_table_found() {
    let totals = census_of(&ruled_and_tagged(
        &[
            &[("Name", 1, true), ("Price", 1, true)],
            &[("a", 1, false), ("b", 1, false)],
        ],
        true,
    ));
    assert_eq!(totals.found, 1);
    assert_eq!((totals.headed, totals.header_evidence), (1, 0));
}
