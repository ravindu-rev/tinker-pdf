//! Tables, stated and inferred (`docs/design/table-reconstruction.md`), held
//! over a first-party holdout set.
//!
//! # What adjudicates
//!
//! The answer key is a **structure tree**: the `Table`, `TR`, `TH` and `TD`
//! elements a producer wrote. For the corpus that producer is a third party,
//! and `table_census.rs` is where it is read; here it is this engine's EPUB
//! writer, whose tables `epub_structure.rs` holds to the XHTML markup — the
//! author's grid, which this engine did not choose — and the tagging API,
//! whose fixtures are arithmetic with a known answer. What a fixture here
//! cannot show is said where it applies: the layout that drew a book's rules
//! and the inference that reads them share an author.

mod epub_support;

use epub_support::book::styled_book;
use tinker_pdf::tables::MAX_TABLE_RULES;
use tinker_pdf::{
    Document, HeaderEvidence, InferredTable, PageTables, StatedTable, TableAttributes,
    TableEvidence, TableOptions, TableRule, TableRules, TableScope, TableSource, TableWarning, Tag,
};
use tinker_pdf_cos::build::DocumentBuilder;

fn open(bytes: Vec<u8>) -> Document {
    Document::open(bytes).expect("the fixture opens")
}

/// The stated tables of every page of `doc`, each with the index of its page.
fn stated(doc: &Document) -> Vec<(u32, StatedTable)> {
    let mut out = Vec::new();
    for index in 0..doc.page_count() {
        let page = doc.page(index).expect("a page");
        out.extend(page.stated_tables().into_iter().map(|t| (index, t)));
    }
    out
}

/// A table with a header row, a column span and a row span, as an EPUB
/// author writes one.
const FRUIT: &str = concat!(
    "<table>",
    r#"<thead><tr><th scope="col">Name</th><th scope="col">Weight</th><th scope="col">Price</th></tr></thead>"#,
    "<tbody>",
    "<tr><td>Apple</td><td>150</td><td>1.20</td></tr>",
    r#"<tr><td colspan="2">Banana split</td><td>3.40</td></tr>"#,
    r#"<tr><td rowspan="2">Cherry</td><td>5</td><td>0.10</td></tr>"#,
    "<tr><td>6</td><td>0.12</td></tr>",
    "</tbody></table>"
);

// ---- stated tables ---------------------------------------------------------------

/// **A book's table reads back as its author wrote it** — rows, columns,
/// spans, header cells, `/Scope` and every cell's text — through the tree
/// this engine's EPUB writer made from the markup and the characters the
/// join claims for each cell on the page it was drawn on.
#[test]
fn a_books_table_reads_back_as_its_markup_states_it() {
    let doc = open(styled_book("en", "td, th { padding: 4px }", FRUIT));
    let tables = stated(&doc);
    assert_eq!(tables.len(), 1, "one table, on one page");
    let (_, table) = &tables[0];
    assert_eq!((table.rows, table.columns), (5, 3));
    assert!(table.warnings.is_empty(), "{:?}", table.warnings);
    let grid: Vec<(usize, usize, usize, usize, bool, &str)> = table
        .cells
        .iter()
        .map(|c| {
            (
                c.row,
                c.column,
                c.row_span,
                c.col_span,
                c.header,
                c.text.as_str(),
            )
        })
        .collect();
    assert_eq!(
        grid,
        [
            (0, 0, 1, 1, true, "Name"),
            (0, 1, 1, 1, true, "Weight"),
            (0, 2, 1, 1, true, "Price"),
            (1, 0, 1, 1, false, "Apple"),
            (1, 1, 1, 1, false, "150"),
            (1, 2, 1, 1, false, "1.20"),
            (2, 0, 1, 2, false, "Banana split"),
            (2, 2, 1, 1, false, "3.40"),
            (3, 0, 2, 1, false, "Cherry"),
            (3, 1, 1, 1, false, "5"),
            (3, 2, 1, 1, false, "0.10"),
            (4, 1, 1, 1, false, "6"),
            (4, 2, 1, 1, false, "0.12"),
        ]
    );
    assert!(table
        .cells
        .iter()
        .filter(|c| c.header)
        .all(|c| c.scope == Some(TableScope::Column)));
    // The characters are the page's own: each cell's quad lies inside the
    // table's, and the text the join claims is the text the cell spells.
    let outer = table.quad.expect("the table draws text").bounds();
    for cell in &table.cells {
        let spelled: String = cell.chars.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(spelled, cell.text);
        let (x0, y0, x1, y1) = cell.quad.expect("a drawn cell").bounds();
        assert!(x0 >= outer.0 && y0 >= outer.1 && x1 <= outer.2 && y1 <= outer.3);
    }
}

/// Builds a one-page document whose one table is `rows`, each a list of
/// `(text, row span, column span)`, tagged with the tagging API.
fn tagged_table(rows: &[&[(&str, u32, u32)]]) -> Document {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| {
        page.tagged_with(&Tag::new(b"Table"), |page| {
            for (r, cells) in rows.iter().enumerate() {
                page.tagged_with(&Tag::new(b"TR"), |page| {
                    for (c, (text, row_span, col_span)) in cells.iter().enumerate() {
                        let mut attributes = TableAttributes::default();
                        attributes.row_span = (*row_span > 1).then_some(*row_span);
                        attributes.col_span = (*col_span > 1).then_some(*col_span);
                        let tag = if attributes.is_empty() {
                            Tag::new(b"TD")
                        } else {
                            Tag::new(b"TD").table(attributes)
                        };
                        page.tagged_with(&tag, |page| {
                            page.text(
                                b"F1",
                                10.0,
                                72.0 + c as f64 * 80.0,
                                700.0 - r as f64 * 20.0,
                                text,
                            );
                        });
                    }
                });
            }
        });
    });
    open(builder.finish())
}

/// **Spans that do not add up are named, not repaired.** A column span
/// reaching into a slot a row span above already holds, and a row span past
/// the last row: each cell is reported where its row's free column put it,
/// at the span it states, with `SpanInconsistent` naming it.
#[test]
fn spans_that_do_not_add_up_are_named_and_not_repaired() {
    // Row 0: A, then B spanning two rows. Row 1: C spanning two columns —
    // into column 1, which B still holds.
    let doc = tagged_table(&[&[("A", 1, 1), ("B", 2, 1)], &[("C", 1, 2)]]);
    let table = &stated(&doc)[0].1;
    assert!(
        table
            .warnings
            .contains(&TableWarning::SpanInconsistent { row: 1, column: 0 }),
        "{:?}",
        table.warnings
    );
    let c = table.cells.iter().find(|c| c.text == "C").expect("C");
    assert_eq!((c.row, c.column, c.col_span), (1, 0, 2));

    // A row span of five in a table of two rows.
    let doc = tagged_table(&[&[("A", 5, 1), ("B", 1, 1)], &[("C", 1, 1)]]);
    let table = &stated(&doc)[0].1;
    assert!(table
        .warnings
        .contains(&TableWarning::SpanInconsistent { row: 0, column: 0 }));
    assert_eq!(table.cells[0].row_span, 5, "the span the file states");
}

/// **A row that fills fewer columns than the table is ragged**, and said to
/// be: the first such row, how many it fills, how many the table has.
#[test]
fn a_ragged_row_is_named() {
    let doc = tagged_table(&[&[("A", 1, 1), ("B", 1, 1), ("C", 1, 1)], &[("D", 1, 1)]]);
    let table = &stated(&doc)[0].1;
    assert_eq!((table.rows, table.columns), (2, 3));
    assert_eq!(
        table.warnings,
        [TableWarning::RaggedRows {
            row: 1,
            width: 1,
            columns: 3
        }]
    );
    // And a regular one is not.
    let doc = tagged_table(&[&[("A", 1, 1), ("B", 1, 1)], &[("C", 1, 2)]]);
    assert!(stated(&doc)[0].1.warnings.is_empty());
}

/// **A cell spanning four billion columns is a clamp, not an allocation.** A
/// table has no more columns than cells, so the grid stops there and the
/// span is named.
#[test]
fn a_vast_span_is_clamped_and_named() {
    let doc = tagged_table(&[&[("A", 1, u32::MAX), ("B", u32::MAX, 1)], &[("C", 1, 1)]]);
    let table = &stated(&doc)[0].1;
    assert!(table.columns <= 3, "{} columns", table.columns);
    assert!(table
        .warnings
        .iter()
        .any(|w| matches!(w, TableWarning::SpanInconsistent { .. })));
}

/// **And past the first row and column.** A span of four billion added to a
/// row or a column past the first overflows a 32-bit `usize` — wasm32 is a
/// first-class target — so the spans are compared with what is left of the
/// table rather than added to where they start. The cell is placed where its
/// row's free column puts it, at the spans the file states, and named.
#[test]
fn a_vast_span_past_the_first_row_and_column_is_clamped_and_named() {
    let doc = tagged_table(&[
        &[("A", 1, 1), ("B", 1, 1)],
        &[("C", 1, 1), ("D", u32::MAX, u32::MAX)],
    ]);
    let table = &stated(&doc)[0].1;
    assert_eq!(table.rows, 2);
    assert!(table.columns <= 4, "{} columns", table.columns);
    assert!(
        table
            .warnings
            .contains(&TableWarning::SpanInconsistent { row: 1, column: 1 }),
        "{:?}",
        table.warnings
    );
    let d = table.cells.iter().find(|c| c.text == "D").expect("D");
    assert_eq!((d.row, d.column), (1, 1));
    assert_eq!(
        (d.row_span, d.col_span),
        (u32::MAX as usize, u32::MAX as usize)
    );
}

/// **A page of many small stated tables is read in bounded work.** Each
/// table's cells were joined by a call of their own, and each call grouped
/// the whole page's characters by sequence again — the page's characters
/// times its tables, from a tree whose element cap allows tens of thousands
/// of one-cell tables. Four thousand of them here, each of four characters,
/// each read back with its text; and asked through `Page::tables`, which
/// reads them once more.
#[test]
fn a_page_of_many_small_stated_tables_is_read_in_bounded_work() {
    const TABLES: usize = 4_000;
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| {
        for at in 0..TABLES {
            let (x, y) = (
                20.0 + (at % 40) as f64 * 14.0,
                760.0 - (at / 40) as f64 * 7.0,
            );
            page.tagged_with(&Tag::new(b"Table"), |page| {
                page.tagged_with(&Tag::new(b"TR"), |page| {
                    page.tagged_with(&Tag::new(b"TD"), |page| {
                        page.text(b"F1", 4.0, x, y, "abcd");
                    });
                });
            });
        }
    });
    let doc = open(builder.finish());
    let page = doc.page(0).expect("a page");
    let tables = page.stated_tables();
    assert_eq!(tables.len(), TABLES);
    for table in &tables {
        assert_eq!((table.rows, table.columns), (1, 1));
        assert_eq!(table.cells.len(), 1);
        assert_eq!(table.cells[0].text, "abcd");
    }
    let PageTables::Stated(again) = page.tables(TableSource::Inferred) else {
        panic!("a page that states tables answers with them");
    };
    assert_eq!(again.len(), TABLES);
}

/// An untagged page states no table, and a tagged page with none states none.
#[test]
fn no_tree_and_no_table_state_nothing() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| {
        page.text(b"F1", 10.0, 72.0, 700.0, "plain")
    });
    assert!(stated(&open(builder.finish())).is_empty());
    let doc = open(styled_book("en", "", "<p>No table here.</p>"));
    assert!(stated(&doc).is_empty());
}

/// Every committed book's stated tables, counted and read: measured when this
/// landed, five of the nine books carry one table of twelve cells each, every
/// one regular and every cell's characters spelling its text.
#[test]
fn the_committed_books_tables_are_read() {
    let books = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/epub");
    let mut entries: Vec<_> = std::fs::read_dir(books)
        .expect("the committed books")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "epub"))
        .collect();
    entries.sort();
    let mut total = 0usize;
    for path in entries {
        let doc = open(std::fs::read(&path).expect("readable"));
        let tables = stated(&doc);
        let cells: usize = tables.iter().map(|(_, t)| t.cells.len()).sum();
        for (_, table) in &tables {
            assert!(table.warnings.is_empty(), "{path:?}: {:?}", table.warnings);
            for cell in &table.cells {
                let spelled: String = cell.chars.iter().map(|c| c.text.as_str()).collect();
                assert_eq!(spelled, cell.text, "{path:?}");
            }
        }
        println!(
            "{:<28} {} tables, {cells} cells",
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            tables.len()
        );
        total += tables.len();
    }
    println!("{total} stated tables across the committed books");
    assert_eq!(total, 5);
}

// ---- rules ---------------------------------------------------------------------------

/// A one-page document whose content is `content`, with Helvetica as `/F1`.
fn drawn(content: &str) -> Document {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| page.raw(content.as_bytes()));
    open(builder.finish())
}

fn rules(doc: &Document) -> TableRules {
    doc.page(0).expect("a page").table_rules()
}

/// The lines of a grid of `rows` by `columns` cells, 100 points wide and 20
/// tall, its top left corner at (72, 600): `(y of each horizontal line, x of
/// each vertical one)`.
fn grid_lines(rows: usize, columns: usize) -> (Vec<f64>, Vec<f64>) {
    let ys = (0..=rows).map(|r| 600.0 - r as f64 * 20.0).collect();
    let xs = (0..=columns).map(|c| 72.0 + c as f64 * 100.0).collect();
    (ys, xs)
}

/// **A grid stroked as lines yields its lines**: three rows of four cells,
/// drawn as four horizontal and five vertical strokes half a point wide,
/// read back as nine rules at the coordinates the grid was drawn at, each
/// the full length and the stroke's weight.
#[test]
fn a_stroked_grid_yields_its_lines_where_they_were_drawn() {
    let (ys, xs) = grid_lines(3, 4);
    let mut content = String::from("0.5 w\n");
    for y in &ys {
        content.push_str(&format!("72 {y} m 472 {y} l S\n"));
    }
    for x in &xs {
        content.push_str(&format!("{x} 540 m {x} 600 l S\n"));
    }
    let read = rules(&drawn(&content));
    assert!(read.warnings.is_empty(), "{:?}", read.warnings);
    let mut expected: Vec<TableRule> = ys
        .iter()
        .map(|y| TableRule {
            horizontal: true,
            at: *y,
            from: 72.0,
            to: 472.0,
            width: 0.5,
        })
        .chain(xs.iter().map(|x| TableRule {
            horizontal: false,
            at: *x,
            from: 540.0,
            to: 600.0,
            width: 0.5,
        }))
        .collect();
    expected.sort_by(|a, b| a.at.total_cmp(&b.at));
    let mut got = read.rules.clone();
    got.sort_by(|a, b| a.at.total_cmp(&b.at));
    assert_eq!(got, expected);
}

/// **A grid drawn as twelve cells' borders yields forty-eight**: each cell's
/// four sides a filled rectangle three quarters of a point thick — which is
/// how the EPUB path draws a CSS border — each read as a rule along its long
/// side, centred on the rectangle, as thick as it is thin.
#[test]
fn a_grid_of_cell_borders_yields_four_rules_a_cell() {
    let t = 0.75;
    let mut content = String::new();
    let mut expected = Vec::new();
    for row in 0..3 {
        for column in 0..4 {
            let (x0, y1) = (72.0 + column as f64 * 100.0, 600.0 - row as f64 * 20.0);
            let (x1, y0) = (x0 + 100.0, y1 - 20.0);
            for (x, y, w, h) in [
                (x0, y1 - t, 100.0, t),
                (x0, y0, 100.0, t),
                (x0, y0, t, 20.0),
                (x1 - t, y0, t, 20.0),
            ] {
                content.push_str(&format!("{x} {y} {w} {h} re f\n"));
                expected.push(if w > h {
                    TableRule {
                        horizontal: true,
                        at: y + t / 2.0,
                        from: x,
                        to: x + w,
                        width: t,
                    }
                } else {
                    TableRule {
                        horizontal: false,
                        at: x + t / 2.0,
                        from: y,
                        to: y + h,
                        width: t,
                    }
                });
            }
        }
    }
    let read = rules(&drawn(&content));
    assert_eq!(read.rules.len(), 48);
    for (got, want) in read.rules.iter().zip(&expected) {
        assert_eq!(got.horizontal, want.horizontal);
        for (a, b) in [
            (got.at, want.at),
            (got.from, want.from),
            (got.to, want.to),
            (got.width, want.width),
        ] {
            assert!((a - b).abs() < 1e-9, "{got:?} against {want:?}");
        }
    }
}

/// **A rectangular clip cuts a rule, and `Q` lifts it**; a clip that is not a
/// rectangle refuses the rule outright and says how many it refused.
#[test]
fn a_rule_is_cut_to_a_rectangular_clip_and_refused_under_any_other() {
    let read = rules(&drawn(concat!(
        "q 100 100 50 50 re W n 0 125 m 300 125 l S Q\n",
        "0 175 m 300 175 l S\n",
    )));
    assert!(read.warnings.is_empty());
    assert_eq!(
        read.rules
            .iter()
            .map(|r| (r.at, r.from, r.to))
            .collect::<Vec<_>>(),
        [(125.0, 100.0, 150.0), (175.0, 0.0, 300.0)]
    );

    let read = rules(&drawn(concat!(
        "q 100 100 m 200 100 l 150 200 l h W n 0 125 m 300 125 l S Q\n",
        "0 175 m 300 175 l S\n",
    )));
    assert_eq!(
        read.warnings,
        [TableWarning::ClipNotRectangular { rules: 1 }]
    );
    assert_eq!(read.rules.len(), 1, "the rule after Q is read");
}

/// **Past the cap no rule is read**, and the warning says how many there
/// were — the firing test the bounds ledger names, spending the cap and one
/// more.
#[test]
fn a_page_past_the_rule_cap_has_no_rules_read() {
    let mut content = String::new();
    for at in 0..=MAX_TABLE_RULES {
        let (x, y) = (
            (at % 100) as f64 * 5.0 + 20.0,
            (at / 100) as f64 * 4.0 + 20.0,
        );
        content.push_str(&format!("{x} {y} m {} {y} l\n", x + 3.0));
    }
    content.push_str("S\n");
    let read = rules(&drawn(&content));
    assert_eq!(
        read.warnings,
        [TableWarning::TooManyRules {
            drawn: MAX_TABLE_RULES + 1
        }]
    );
    assert!(read.rules.is_empty());
}

/// **What is not a rule is not read as one**: a curve, a diagonal, a stroke
/// five points wide, a filled square, and a line shorter than two points.
#[test]
fn curves_diagonals_bars_and_dots_are_not_rules() {
    let read = rules(&drawn(concat!(
        "0.5 w 72 100 m 100 150 200 150 300 100 c S\n",
        "72 200 m 300 260 l S\n",
        "5 w 72 300 m 300 300 l S 0.5 w\n",
        "72 400 50 50 re f\n",
        "72 500 m 73 500 l S\n",
    )));
    assert!(read.rules.is_empty(), "{:?}", read.rules);
}

// ---- inferred tables: the lattice ----------------------------------------------------

fn hidden() -> TableOptions {
    TableOptions {
        hide_structure: true,
    }
}

/// A character's identity across two readings of one page: origin and text.
type Key = (u64, u64, String);

fn key(c: &tinker_pdf::TextChar) -> Key {
    (c.origin.0.to_bits(), c.origin.1.to_bits(), c.text.clone())
}

/// The design's scores for one stated table against the inferred ones:
/// whether one was found over it, whether its grid matches exactly, and how
/// many of the stated cells' characters the inference put in the same row and
/// column, of how many.
fn score(stated: &StatedTable, inferred: &[InferredTable]) -> (bool, bool, usize, usize) {
    let total: usize = stated.cells.iter().map(|c| c.chars.len()).sum();
    let Some(quad) = stated.quad else {
        return (false, false, 0, total);
    };
    let (sx0, sy0, sx1, sy1) = quad.bounds();
    let found = inferred.iter().find(|t| {
        let (x0, y0, x1, y1) = t.bounds.bounds();
        let overlap = (sx1.min(x1) - sx0.max(x0)).max(0.0) * (sy1.min(y1) - sy0.max(y0)).max(0.0);
        overlap * 2.0 >= (sx1 - sx0) * (sy1 - sy0)
    });
    let Some(table) = found else {
        return (false, false, 0, total);
    };
    let grid = table.rows == stated.rows && table.columns == stated.columns;
    let mut place = std::collections::BTreeMap::new();
    for cell in &table.cells {
        for c in &cell.chars {
            place.insert(key(c), (cell.row, cell.column));
        }
    }
    let agreeing = stated
        .cells
        .iter()
        .flat_map(|cell| cell.chars.iter().map(move |c| (cell, c)))
        .filter(|(cell, c)| place.get(&key(c)) == Some(&(cell.row, cell.column)))
        .count();
    (true, grid, agreeing, total)
}

/// Three rows of four cells, ruled with half-point strokes, each cell's text
/// `r{row}c{column}` drawn at its top left; `tag` tags the table row by row
/// in the tree, and the text is drawn **column by column**, as a producer
/// that fills a table a column at a time would.
fn ruled_grid(tag: bool) -> Document {
    let (ys, xs) = grid_lines(3, 4);
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| {
        let mut rules = String::from("0.5 w\n");
        for y in &ys {
            rules.push_str(&format!("72 {y} m 472 {y} l S\n"));
        }
        for x in &xs {
            rules.push_str(&format!("{x} 540 m {x} 600 l S\n"));
        }
        page.raw(rules.as_bytes());
        let draw = |page: &mut tinker_pdf_cos::build::PageBuilder, row: usize, column: usize| {
            let (x, y) = (76.0 + column as f64 * 100.0, 586.0 - row as f64 * 20.0);
            page.text(b"F1", 10.0, x, y, &format!("r{row}c{column}"));
        };
        if tag {
            page.tagged_with(&Tag::new(b"Table"), |page| {
                for row in 0..3 {
                    page.tagged_with(&Tag::new(b"TR"), |page| {
                        for column in 0..4 {
                            let order = (row * 4 + column) as u64;
                            page.tagged_with(&Tag::new(b"TD").keyed(order + 1, order), |page| {
                                draw(page, row, column)
                            });
                        }
                    });
                }
            });
        } else {
            for column in 0..4 {
                for row in 0..3 {
                    draw(page, row, column);
                }
            }
        }
    });
    open(builder.finish())
}

/// **A ruled grid is read row by row however it was drawn.** Untagged, its
/// text drawn a column at a time: one table, three rows, four columns, each
/// cell's text the one drawn inside it, ruled evidence, and a permutation
/// that is every character of the table once.
#[test]
fn a_ruled_grid_is_read_row_by_row_however_it_was_drawn() {
    let doc = ruled_grid(false);
    let page = doc.page(0).expect("a page");
    let found = page.inferred_tables(&TableOptions::default());
    assert!(found.warnings.is_empty(), "{:?}", found.warnings);
    assert_eq!(found.tables.len(), 1);
    let table = &found.tables[0];
    assert_eq!((table.rows, table.columns), (3, 4));
    assert_eq!(table.evidence, TableEvidence::Ruled);
    for cell in &table.cells {
        assert_eq!(cell.text, format!("r{}c{}", cell.row, cell.column));
    }
    let text: String = table.cells.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(text, "r0c0r0c1r0c2r0c3r1c0r1c1r1c2r1c3r2c0r2c1r2c2r2c3");
    let mut seen = table.permutation.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), table.permutation.len());
    assert_eq!(table.permutation.len(), 12 * 4);
    // The page's own text is not touched.
    assert_eq!(
        page.text().plain_text(),
        ruled_grid(false)
            .page(0)
            .expect("a page")
            .text()
            .plain_text()
    );
}

/// **Against the tree it hid.** The same grid tagged row by row in the tree
/// and drawn column by column: with the tree hidden, the table is found over
/// the stated one, its grid is the stated grid, and every stated cell's
/// characters are in the same row and column — where the stream's order
/// reads the table a column at a time.
#[test]
fn a_ruled_grid_matches_the_table_it_was_tagged_as() {
    let doc = ruled_grid(true);
    let page = doc.page(0).expect("a page");
    let stated = page.stated_tables();
    assert_eq!(stated.len(), 1);
    let inferred = page.inferred_tables(&hidden());
    assert_eq!(inferred.warnings, [TableWarning::TreePresent]);
    let (found, grid, agreeing, total) = score(&stated[0], &inferred.tables);
    println!("ruled grid: found {found} grid {grid} cells {agreeing}/{total}");
    assert!(found && grid);
    assert_eq!(agreeing, total);
    assert_eq!(total, 48);
    // Without hiding it, the tree's table is the answer and nothing is
    // inferred.
    let declined = page.inferred_tables(&TableOptions::default());
    assert!(declined.tables.is_empty());
    assert_eq!(declined.warnings, [TableWarning::TreePresent]);
    assert!(matches!(
        page.tables(TableSource::Inferred),
        PageTables::Stated(_)
    ));
}

/// A three-column table of five rows, every cell bordered, as an EPUB author
/// sets one: `collapse` merges adjacent borders, `separate` leaves the CSS
/// default spacing between them.
fn bordered_book(model: &str) -> Document {
    let mut body = String::from("<table><tr><th>Name</th><th>Weight</th><th>Price</th></tr>");
    for (name, weight, price) in [
        ("Apple", "150", "1.20"),
        ("Banana", "120", "0.80"),
        ("Cherry", "5", "0.10"),
        ("Damson", "30", "0.45"),
    ] {
        body.push_str(&format!(
            "<tr><td>{name}</td><td>{weight}</td><td>{price}</td></tr>"
        ));
    }
    body.push_str("</table>");
    open(styled_book(
        "en",
        &format!("table {{ border-collapse: {model} }} td, th {{ border: 1px solid black; padding: 4px }}"),
        &body,
    ))
}

/// **A book's bordered table is recovered exactly**, in both of CSS 2.2
/// §17.6's border models: the answer key is the XHTML grid, through the tree
/// this engine's EPUB writer made of it; the rules are the borders its layout
/// drew. Found, the same grid, every character in its stated cell. What this
/// cannot show is said in the module documentation: the layout that drew the
/// rules and the inference that reads them share an author.
#[test]
fn a_books_bordered_table_is_recovered_exactly() {
    for model in ["collapse", "separate"] {
        let doc = bordered_book(model);
        let mut scored = 0;
        for index in 0..doc.page_count() {
            let page = doc.page(index).expect("a page");
            let stated = page.stated_tables();
            if stated.is_empty() {
                continue;
            }
            let inferred = page.inferred_tables(&hidden());
            for table in &stated {
                let (found, grid, agreeing, total) = score(table, &inferred.tables);
                println!("{model}: found {found} grid {grid} cells {agreeing}/{total}");
                assert!(found && grid, "{model}: {} tables", inferred.tables.len());
                assert_eq!(agreeing, total, "{model}");
                scored += 1;
            }
        }
        assert_eq!(scored, 1, "{model}");
    }
}

/// **Nothing that is not a table is one**: a framed paragraph is a box of
/// one cell; a grid of empty boxes is a form; and no committed book's page
/// whose tree states no table, nor any committed `testdata` page, yields one.
#[test]
fn boxes_forms_and_untabled_pages_are_not_tables() {
    let boxed = drawn(concat!(
        "0.5 w 72 500 400 100 re S\n",
        "BT /F1 10 Tf 80 580 Td (A framed paragraph of prose, inside one box.) Tj ET\n",
    ));
    assert!(boxed
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default())
        .tables
        .is_empty());

    let (ys, xs) = grid_lines(4, 4);
    let mut form = String::from("0.5 w\n");
    for y in &ys {
        form.push_str(&format!("72 {y} m 472 {y} l S\n"));
    }
    for x in &xs {
        form.push_str(&format!("{x} 520 m {x} 600 l S\n"));
    }
    // Two labels in sixteen boxes: text in fewer than one cell in four.
    form.push_str("BT /F1 10 Tf 76 586 Td (Name:) Tj ET\n");
    form.push_str("BT /F1 10 Tf 76 566 Td (Date:) Tj ET\n");
    let form = drawn(&form);
    assert!(form
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default())
        .tables
        .is_empty());

    let mut extras = 0usize;
    let mut pages = 0usize;
    for dir in [
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../testdata"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/epub"),
    ] {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .expect("the committed fixtures")
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("pdf" | "epub")))
            .collect();
        entries.sort();
        for path in entries {
            let Ok(doc) = Document::open(std::fs::read(&path).expect("readable")) else {
                continue;
            };
            for index in 0..doc.page_count() {
                let page = doc.page(index).expect("a page");
                if !page.stated_tables().is_empty() {
                    continue;
                }
                pages += 1;
                let found = page.inferred_tables(&hidden()).tables.len();
                if found > 0 {
                    println!("{path:?} page {index}: {found} tables");
                }
                extras += found;
            }
        }
    }
    println!("{extras} tables inferred over {pages} committed pages that state none");
    assert_eq!(extras, 0);
}

/// **A table in a cell is refused by name**, and the table around it
/// returned; **a table ending in the foot band may continue**; **a glyph
/// across a rule is counted**.
#[test]
fn nested_continued_and_crossed_tables_are_named() {
    let (ys, xs) = grid_lines(3, 4);
    let mut content = String::from("0.5 w\n");
    for y in &ys {
        content.push_str(&format!("72 {y} m 472 {y} l S\n"));
    }
    for x in &xs {
        content.push_str(&format!("{x} 540 m {x} 600 l S\n"));
    }
    for row in 0..3 {
        for column in 0..4 {
            let (x, y) = (76.0 + column as f64 * 100.0, 586.0 - row as f64 * 20.0);
            content.push_str(&format!(
                "BT /F1 10 Tf {x} {y} Td (r{row}c{column}) Tj ET\n"
            ));
        }
    }
    // Beside it, a grid of its own whose bottom-right cell — 372 to 472
    // across, 360 to 420 down — holds a two-by-two grid ruled small, every
    // inner line well over half an em from the cell's.
    let mut outer = String::from("0.5 w\n");
    for y in [480.0, 420.0, 360.0] {
        outer.push_str(&format!("272 {y} m 472 {y} l S\n"));
    }
    for x in [272.0, 372.0, 472.0] {
        outer.push_str(&format!("{x} 360 m {x} 480 l S\n"));
    }
    for (x, y, text) in [
        (276.0, 466.0, "a"),
        (376.0, 466.0, "b"),
        (276.0, 406.0, "c"),
    ] {
        outer.push_str(&format!("BT /F1 10 Tf {x} {y} Td ({text}) Tj ET\n"));
    }
    outer.push_str("0.2 w 385 370 m 460 370 l S 385 390 m 460 390 l S 385 410 m 460 410 l S\n");
    outer.push_str("385 370 m 385 410 l S 420 370 m 420 410 l S 460 370 m 460 410 l S\n");
    outer.push_str("BT /F1 10 Tf 390 395 Td (d) Tj ET BT /F1 10 Tf 425 375 Td (e) Tj ET\n");
    let nested = drawn(&outer);
    let found = nested
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    let shapes: Vec<(usize, usize)> = found.tables.iter().map(|t| (t.rows, t.columns)).collect();
    assert_eq!(shapes, [(2, 2)]);
    assert!(
        found.tables[0]
            .warnings
            .contains(&TableWarning::NestedLattice),
        "{:?}",
        found.tables[0].warnings
    );

    let doc = drawn(&content);
    let found = doc
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert_eq!(found.tables.len(), 1);
    assert!(
        found.tables[0].warnings.is_empty(),
        "{:?}",
        found.tables[0].warnings
    );

    // The same grid at the foot of the page, its last rule at y = 40.
    let shifted: String = content
        .lines()
        .take(1 + ys.len() + xs.len())
        .map(|l| {
            l.replace(" 600", " 100")
                .replace(" 580", " 80")
                .replace(" 560", " 60")
                .replace(" 540", " 40")
                + "\n"
        })
        .collect();
    let mut low = shifted;
    for row in 0..3 {
        for column in 0..4 {
            let (x, y) = (76.0 + column as f64 * 100.0, 86.0 - row as f64 * 20.0);
            low.push_str(&format!(
                "BT /F1 10 Tf {x} {y} Td (r{row}c{column}) Tj ET\n"
            ));
        }
    }
    let found = drawn(&low)
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert_eq!(found.tables.len(), 1);
    assert!(found.tables[0]
        .warnings
        .contains(&TableWarning::MayContinue));

    // A word drawn across the rule at x = 172: set from 148, its second `d`
    // runs from 170.2 to 175.8, so the rule passes through its middle.
    let mut crossed = content
        .lines()
        .take(1 + ys.len() + xs.len())
        .map(|l| format!("{l}\n"))
        .collect::<String>();
    crossed.push_str("BT /F1 10 Tf 148 586 Td (straddling) Tj ET\n");
    crossed.push_str("BT /F1 10 Tf 280 566 Td (inside) Tj ET\n");
    let found = drawn(&crossed)
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert_eq!(found.tables.len(), 1);
    assert!(found.tables[0]
        .warnings
        .iter()
        .any(|w| matches!(w, TableWarning::TextCrossesRule { chars } if *chars > 0)));
}

/// Every stream position the page's tables hold, each once, or a panic
/// naming the one held twice.
fn held_once(tables: &[InferredTable]) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    for table in tables {
        for at in &table.permutation {
            assert!(seen.insert(*at), "character {at} is in two tables");
        }
    }
    seen.len()
}

/// **A table in a merged cell is nested too, and no character is in two
/// tables.** A grid of two rows whose second row is one cell across both
/// columns — no rule at x = 222 under y = 560 — holds a small grid of its own
/// that straddles where that rule would be. It is in no one grid cell of the
/// outer table, which is how nesting was first asked, and read that way both
/// tables claimed its four letters; it lies inside the outer frame, which is
/// how nesting is asked now.
#[test]
fn a_table_in_a_merged_cell_is_nested_and_held_once() {
    let mut content = String::from("0.5 w\n");
    for y in [600.0, 560.0, 480.0] {
        content.push_str(&format!("72 {y} m 372 {y} l S\n"));
    }
    content.push_str("72 480 m 72 600 l S 372 480 m 372 600 l S 222 560 m 222 600 l S\n");
    for y in [545.0, 525.0, 505.0] {
        content.push_str(&format!("150 {y} m 300 {y} l S\n"));
    }
    for x in [150.0, 225.0, 300.0] {
        content.push_str(&format!("{x} 505 m {x} 545 l S\n"));
    }
    for (x, y, text) in [
        (80.0, 575.0, "a"),
        (230.0, 575.0, "b"),
        (155.0, 530.0, "c"),
        (230.0, 530.0, "d"),
        (155.0, 510.0, "e"),
        (230.0, 510.0, "f"),
    ] {
        content.push_str(&format!("BT /F1 10 Tf {x} {y} Td ({text}) Tj ET\n"));
    }
    let found = drawn(&content)
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    let shapes: Vec<(usize, usize)> = found.tables.iter().map(|t| (t.rows, t.columns)).collect();
    assert_eq!(shapes, [(2, 2)]);
    let table = &found.tables[0];
    assert!(
        table.warnings.contains(&TableWarning::NestedLattice),
        "{:?}",
        table.warnings
    );
    let merged = table
        .cells
        .iter()
        .find(|c| c.row == 1)
        .expect("the merged row");
    assert_eq!(merged.col_span, 2);
    let mut letters: Vec<char> = merged.text.chars().filter(|c| !c.is_whitespace()).collect();
    letters.sort_unstable();
    assert_eq!(letters, ['c', 'd', 'e', 'f']);
    assert_eq!(held_once(&found.tables), 6);
}

/// **Lattices that pass each other are no table.** Two small grids, each
/// connected, whose frames overlap with neither inside the other — one's
/// verticals run past the other's corner without meeting a rule of it —
/// would each claim what lies in the overlap; neither is read, and the page
/// says so.
#[test]
fn lattices_whose_frames_cross_are_no_table() {
    let content = concat!(
        "0.5 w\n",
        // One grid: a row of two cells, 72 to 272 across, 560 to 600 down.
        "72 600 m 272 600 l S 72 560 m 272 560 l S\n",
        "72 560 m 72 600 l S 172 560 m 172 600 l S 272 560 m 272 600 l S\n",
        // The other: a row of two cells, 250 to 400 across, 540 to 580
        // down — its leftmost vertical drawn under its frame, its top rule
        // starting right of the first grid's.
        "250 540 m 400 540 l S 300 580 m 400 580 l S\n",
        "250 500 m 250 540 l S 300 520 m 300 590 l S 400 520 m 400 590 l S\n",
        "BT /F1 10 Tf 80 575 Td (a) Tj ET BT /F1 10 Tf 180 575 Td (b) Tj ET\n",
        "BT /F1 10 Tf 260 552 Td (c) Tj ET BT /F1 10 Tf 320 552 Td (d) Tj ET\n",
    );
    let found = drawn(content)
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert!(found.tables.is_empty(), "{:?}", found.tables);
    assert_eq!(
        found.warnings,
        [TableWarning::LatticesCross { lattices: 2 }]
    );

    // Either grid alone is a table.
    let alone: String = content
        .lines()
        .filter(|l| !l.starts_with("250") && !l.contains("(c)"))
        .map(|l| format!("{l}\n"))
        .collect();
    let found = drawn(&alone)
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert_eq!(found.tables.len(), 1);
    assert!(found.warnings.is_empty(), "{:?}", found.warnings);
}

/// **Nine hundred small tables are read, each character once.** Thirty rows
/// of thirty grids of two cells, a letter in each: every grid a table, and
/// each reading only the characters within its own frame.
#[test]
fn a_page_of_small_tables_reads_each_one() {
    let mut content = String::from("0.2 w\n");
    for row in 0..30 {
        for column in 0..30 {
            let (x, y) = (8.0 + column as f64 * 20.0, 20.0 + row as f64 * 25.0);
            let (right, top) = (x + 16.0, y + 8.0);
            content.push_str(&format!(
                "{x} {y} m {right} {y} l S {x} {top} m {right} {top} l S\n"
            ));
            for v in [x, x + 8.0, right] {
                content.push_str(&format!("{v} {y} m {v} {top} l S\n"));
            }
            for cell in 0..2 {
                content.push_str(&format!(
                    "BT /F1 4 Tf {} {} Td (x) Tj ET\n",
                    x + 2.0 + cell as f64 * 8.0,
                    y + 2.5
                ));
            }
        }
    }
    let doc = drawn(&content);
    let page = doc.page(0).expect("a page");
    let found = page.inferred_tables(&TableOptions::default());
    assert_eq!(found.tables.len(), 900);
    assert!(found.tables.iter().all(|t| t.cells.len() == 2));
    assert_eq!(held_once(&found.tables), 1_800);
}

/// **A page of hatching is not a table, and is not a square's worth of
/// cells.** Two hundred lines each way, one word in one cell: a lattice of
/// nearly forty thousand cells that holds text in one, refused before a cell
/// is made.
#[test]
fn a_page_of_hatching_makes_no_cells() {
    let mut content = String::from("0.2 w\n");
    for at in 0..200 {
        let v = 20.0 + at as f64 * 2.8;
        content.push_str(&format!("20 {v} m 580 {v} l S {v} 20 m {v} 580 l S\n"));
    }
    content.push_str("BT /F1 1 Tf 21 21 Td (x) Tj ET\n");
    let found = drawn(&content)
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert!(found.tables.is_empty());
    assert_eq!(found.rules.len(), 400);
}

/// **A cell of two lines reads top line first**, whichever the producer drew
/// first.
#[test]
fn a_cell_of_two_lines_reads_from_its_top_line() {
    let read = drawn(concat!(
        "0.5 w 72 600 m 272 600 l S 72 560 m 272 560 l S 72 520 m 272 520 l S\n",
        "72 520 m 72 600 l S 172 520 m 172 600 l S 272 520 m 272 600 l S\n",
        "BT /F1 10 Tf 76 572 Td (second) Tj ET BT /F1 10 Tf 76 586 Td (first) Tj ET\n",
        "BT /F1 10 Tf 176 586 Td (b) Tj ET BT /F1 10 Tf 76 546 Td (c) Tj ET\n",
    ));
    let found = read
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert_eq!(found.tables.len(), 1);
    assert_eq!(found.tables[0].cells[0].text, "firstsecond");
}

/// **Rules that stop short of each other still meet**: the verticals of a
/// grid drawn two points short of the horizontals at both ends, as a
/// producer that leaves the corners open draws them, rule one table.
#[test]
fn rules_that_stop_short_still_meet() {
    let (ys, xs) = grid_lines(3, 4);
    let mut content = String::from("0.5 w\n");
    for y in &ys {
        content.push_str(&format!("72 {y} m 472 {y} l S\n"));
    }
    for x in &xs {
        content.push_str(&format!("{x} 542 m {x} 598 l S\n"));
    }
    for row in 0..3 {
        for column in 0..4 {
            let (x, y) = (76.0 + column as f64 * 100.0, 586.0 - row as f64 * 20.0);
            content.push_str(&format!(
                "BT /F1 10 Tf {x} {y} Td (r{row}c{column}) Tj ET\n"
            ));
        }
    }
    let found = drawn(&content)
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    let shapes: Vec<(usize, usize)> = found.tables.iter().map(|t| (t.rows, t.columns)).collect();
    assert_eq!(shapes, [(3, 4)]);
}

// ---- spans, header evidence, direction --------------------------------------------

/// **A book's spanned table is recovered with its spans**: [`FRUIT`] —
/// a column span and a row span — bordered cell by cell, in both border
/// models. Where the markup merges cells the layout draws no border between
/// them, so the lattice has no rule there, and the shape the missing rules
/// leave is the span. Every cell at its stated place with its stated span,
/// and every character in its stated cell.
#[test]
fn a_books_spanned_table_is_recovered_with_its_spans() {
    for model in ["collapse", "separate"] {
        let doc = open(styled_book(
            "en",
            &format!(
                "table {{ border-collapse: {model} }} td, th {{ border: 1px solid black; padding: 4px }}"
            ),
            FRUIT,
        ));
        let page = doc.page(0).expect("a page");
        let stated = page.stated_tables();
        assert_eq!(stated.len(), 1);
        let inferred = page.inferred_tables(&hidden());
        let (found, grid, agreeing, total) = score(&stated[0], &inferred.tables);
        println!("{model}: found {found} grid {grid} cells {agreeing}/{total}");
        assert!(found && grid, "{model}");
        assert_eq!(agreeing, total, "{model}");
        let table = &inferred.tables[0];
        let shape = |cells: Vec<(usize, usize, usize, usize)>| {
            let mut cells = cells;
            cells.sort_unstable();
            cells
        };
        assert_eq!(
            shape(
                table
                    .cells
                    .iter()
                    .map(|c| (c.row, c.column, c.row_span, c.col_span))
                    .collect()
            ),
            shape(
                stated[0]
                    .cells
                    .iter()
                    .map(|c| (c.row, c.column, c.row_span, c.col_span))
                    .collect()
            ),
            "{model}"
        );
        assert!(table.warnings.is_empty(), "{model}: {:?}", table.warnings);
    }
}

/// A three-by-three ruled grid whose interior rules are those `keep` lets
/// through — `(horizontal, index along the other axis, row or column)` —
/// with a letter in every cell.
fn partly_ruled(keep: impl Fn(bool, usize, usize) -> bool) -> Document {
    let mut content = String::from("0.5 w 72 600 m 372 600 l S 72 540 m 372 540 l S\n");
    content.push_str("72 540 m 72 600 l S 372 540 m 372 600 l S\n");
    for row in 0..3 {
        for column in 0..3 {
            let (x0, y1) = (72.0 + column as f64 * 100.0, 600.0 - row as f64 * 20.0);
            // The rule under this cell and the one to its right.
            if row < 2 && keep(true, row, column) {
                content.push_str(&format!(
                    "{x0} {} m {} {} l S\n",
                    y1 - 20.0,
                    x0 + 100.0,
                    y1 - 20.0
                ));
            }
            if column < 2 && keep(false, row, column) {
                content.push_str(&format!(
                    "{} {} m {} {y1} l S\n",
                    x0 + 100.0,
                    y1 - 20.0,
                    x0 + 100.0
                ));
            }
            let letter = (b'a' + (row * 3 + column) as u8) as char;
            content.push_str(&format!(
                "BT /F1 10 Tf {} {} Td ({letter}) Tj ET\n",
                x0 + 4.0,
                y1 - 14.0
            ));
        }
    }
    drawn(&content)
}

/// **A missing rule is a span; a shape that is not a rectangle is named.**
/// The rule between the first two cells of the top row left out is a column
/// span of two; leave out the rule under the first cell as well and the
/// three cells make an L, which no span describes, so they stay grid cells
/// and `SpanNotRectangular` says where.
#[test]
fn a_missing_rule_is_a_span_and_an_l_is_named() {
    let doc = partly_ruled(|horizontal, row, column| !(!horizontal && row == 0 && column == 0));
    let table = &doc
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default())
        .tables[0];
    let first = &table.cells[0];
    assert_eq!(
        (first.row, first.column, first.row_span, first.col_span),
        (0, 0, 1, 2)
    );
    assert_eq!(first.text, "ab");
    assert_eq!(table.cells.len(), 8);
    assert!(table.warnings.is_empty(), "{:?}", table.warnings);

    let doc = partly_ruled(|horizontal, row, column| {
        // Neither rule of the first cell — not the one to its right, not
        // the one under it.
        let _ = horizontal;
        row != 0 || column != 0
    });
    let table = &doc
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default())
        .tables[0];
    assert_eq!(
        table.warnings,
        [TableWarning::SpanNotRectangular { row: 0, column: 0 }]
    );
    assert_eq!(table.cells.len(), 9, "the L stays three grid cells");
}

/// A ruled three-by-three grid of letters, with `extra` drawn under it.
fn grid_with(extra: &str, rule_under_first: &str) -> Document {
    let mut content = String::from(extra);
    content.push_str("0 g 0.5 w 72 600 m 372 600 l S 72 540 m 372 540 l S 72 560 m 372 560 l S\n");
    content.push_str(rule_under_first);
    for x in [72.0, 172.0, 272.0, 372.0] {
        content.push_str(&format!("{x} 540 m {x} 600 l S\n"));
    }
    for row in 0..3 {
        for column in 0..3 {
            let letter = (b'a' + (row * 3 + column) as u8) as char;
            content.push_str(&format!(
                "BT /F1 10 Tf {} {} Td ({letter}) Tj ET\n",
                76.0 + column as f64 * 100.0,
                586.0 - row as f64 * 20.0
            ));
        }
    }
    drawn(&content)
}

fn header_of(doc: &Document) -> HeaderEvidence {
    doc.page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default())
        .tables[0]
        .header
}

/// **Header evidence is what the ink shows.** A grey fill under the first
/// row and none under the second: `FillBeneath`. The unfilled twin:
/// `FirstRow`, which is no evidence and says so. A rule under the first row
/// three times the others' weight, or doubled where they are single:
/// `RuleBeneath`. A book whose header cells have a background: `FillBeneath`.
#[test]
fn header_evidence_is_what_the_ink_shows() {
    let thin = "72 580 m 372 580 l S\n";
    assert_eq!(
        header_of(&grid_with("0.85 g 72 580 300 20 re f\n", thin)),
        HeaderEvidence::FillBeneath
    );
    assert_eq!(header_of(&grid_with("", thin)), HeaderEvidence::FirstRow);
    // Shading under both rows is a striped table, not a header.
    assert_eq!(
        header_of(&grid_with("0.85 g 72 560 300 40 re f\n", thin)),
        HeaderEvidence::FirstRow
    );
    assert_eq!(
        header_of(&grid_with("", "1.5 w 72 580 m 372 580 l S 0.5 w\n")),
        HeaderEvidence::RuleBeneath
    );
    assert_eq!(
        header_of(&grid_with(
            "",
            "72 580 m 372 580 l S 72 581.5 m 372 581.5 l S\n"
        )),
        HeaderEvidence::RuleBeneath
    );

    let book = open(styled_book(
        "en",
        "table { border-collapse: collapse } td, th { border: 1px solid black; padding: 4px } th { background: #cccccc }",
        "<table><tr><th>Name</th><th>Price</th></tr><tr><td>Apple</td><td>1.20</td></tr><tr><td>Banana</td><td>0.80</td></tr></table>",
    ));
    let page = book.page(0).expect("a page");
    let found = page.inferred_tables(&hidden());
    assert_eq!(found.tables.len(), 1);
    assert_eq!(found.tables[0].header, HeaderEvidence::FillBeneath);
}

/// A one-row table sets no row apart.
#[test]
fn a_one_row_table_has_no_header_evidence() {
    let doc = drawn(concat!(
        "0.5 w 72 600 m 372 600 l S 72 580 m 372 580 l S\n",
        "72 580 m 72 600 l S 172 580 m 172 600 l S 272 580 m 272 600 l S 372 580 m 372 600 l S\n",
        "BT /F1 10 Tf 76 586 Td (a) Tj ET BT /F1 10 Tf 176 586 Td (b) Tj ET BT /F1 10 Tf 276 586 Td (c) Tj ET\n",
    ));
    assert_eq!(header_of(&doc), HeaderEvidence::None);
}

/// **A right-to-left table is read from the right.** A ruled two-by-three
/// grid of Hebrew words, each set in visual order the way a producer draws
/// one: most of the table's characters are on right-to-left lines, so column
/// 0 is the rightmost and each row reads right to left.
#[test]
fn a_right_to_left_table_reads_from_the_right() {
    use epub_support::typeface::Face;
    use tinker_pdf_cos::build::{Glyph, PlacedGlyph};
    const LETTERS: &str = "\u{5D0}\u{5D1}\u{5D2}\u{5D3}\u{5D4}\u{5D5}";
    let face = Face::new("Fixture Hebrew", LETTERS);
    let program = face.build();
    let mut builder = DocumentBuilder::new();
    assert!(builder.add_cid_font(b"F0", b"FixtureHebrew", &program));
    let letters: Vec<char> = LETTERS.chars().collect();
    let mut content = Vec::new();
    content.extend_from_slice(
        b"0.5 w 72 600 m 372 600 l S 72 580 m 372 580 l S 72 560 m 372 560 l S\n",
    );
    content.extend_from_slice(
        b"72 560 m 72 600 l S 172 560 m 172 600 l S 272 560 m 272 600 l S 372 560 m 372 600 l S\n",
    );
    // Cell (row, grid column) holds two copies of one letter, so which cell
    // a reading puts first is visible in its text.
    for row in 0..2 {
        for column in 0..3 {
            let letter = letters[row * 3 + column].to_string();
            let glyphs: Vec<PlacedGlyph<'_>> = (0..2)
                .map(|i| PlacedGlyph {
                    glyph: Glyph {
                        id: face.glyph_of(letters[row * 3 + column]).expect("covered"),
                        text: &letter,
                    },
                    x: i as f64 * 5.0,
                    rise: 0.0,
                })
                .collect();
            let (x, y) = (90.0 + column as f64 * 100.0, 586.0 - row as f64 * 20.0);
            assert!(builder.glyph_run(
                &mut content,
                b"F0",
                10.0,
                [1.0, 0.0, 0.0, 1.0, x, y],
                &glyphs
            ));
        }
    }
    builder.add_page(612.0, 792.0, |page| page.raw(&content));
    let doc = open(builder.finish());
    let found = doc
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert_eq!(found.tables.len(), 1);
    let table = &found.tables[0];
    let first = &table.cells[0];
    assert_eq!((first.row, first.column), (0, 0));
    // The rightmost cell of the top row — grid column 2 — is read first.
    assert_eq!(first.text, letters[2].to_string().repeat(2));
    let row: String = table
        .cells
        .iter()
        .filter(|c| c.row == 0)
        .map(|c| c.text.chars().next().unwrap_or(' '))
        .collect();
    assert_eq!(
        row,
        [letters[2], letters[1], letters[0]]
            .iter()
            .collect::<String>()
    );
    let (x0, _, _, _) = first.quad.bounds();
    assert!(x0 > 270.0, "column 0 is the rightmost");
}

// ---- aligned tables -------------------------------------------------------------

/// **A committed book's unruled table is found by its alignment.** The two
/// calibre books lay their twelve-cell table out as a grid with no border:
/// the header row centred over its columns, the body rows starting at three
/// recurring left edges. With the tree hidden it is found as an `Aligned`
/// table — named `NoRules` — over the stated one, with the stated grid and
/// every character in its stated cell. The three pandoc books set the same
/// table as running text, which no reader of the page could call a grid, and
/// nothing is found there: recall over the committed books' five stated
/// tables is two, by alignment, and none by rules.
#[test]
fn a_committed_books_unruled_table_is_found_by_its_alignment() {
    let books = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/epub");
    let mut entries: Vec<_> = std::fs::read_dir(books)
        .expect("the committed books")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "epub"))
        .collect();
    entries.sort();
    let (mut stated_count, mut found_aligned, mut found_ruled) = (0usize, 0usize, 0usize);
    for path in entries {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let doc = open(std::fs::read(&path).expect("readable"));
        for index in 0..doc.page_count() {
            let page = doc.page(index).expect("a page");
            let stated = page.stated_tables();
            if stated.is_empty() {
                continue;
            }
            let inferred = page.inferred_tables(&hidden());
            for table in &stated {
                stated_count += 1;
                let (found, grid, agreeing, total) = score(table, &inferred.tables);
                let evidence = inferred.tables.first().map(|t| t.evidence);
                println!(
                    "{name}: found {found} ({evidence:?}) grid {grid} cells {agreeing}/{total}"
                );
                if name.starts_with("calibre-book") {
                    assert!(found && grid, "{name}");
                    assert_eq!(agreeing, total, "{name}");
                    let t = &inferred.tables[0];
                    assert_eq!(t.evidence, TableEvidence::Aligned);
                    assert_eq!(t.warnings, [TableWarning::NoRules]);
                }
                if found {
                    match evidence {
                        Some(TableEvidence::Aligned) => found_aligned += 1,
                        _ => found_ruled += 1,
                    }
                }
            }
        }
    }
    println!("{stated_count} stated tables: {found_aligned} found aligned, {found_ruled} ruled");
    assert_eq!((stated_count, found_aligned, found_ruled), (5, 2, 0));
}

/// A line of words, deterministic in `seed`, at least `length` characters.
fn prose(seed: usize, length: usize) -> String {
    const WORDS: &[&str] = &[
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
        "juliett", "kilo", "lima", "mike", "november", "oscar", "papa",
    ];
    let mut out = String::new();
    let mut at = seed.wrapping_mul(7);
    while out.len() < length {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(WORDS[at % WORDS.len()]);
        at = at.wrapping_mul(31).wrapping_add(17);
    }
    out
}

/// **Prose is not a table**: a paragraph, whose lines are one fragment each;
/// two columns of it side by side, whose rows are two fragments starting at
/// two recurring edges — columns of a page, both as wide as a column of prose
/// is; and two rows of aligned words, one fewer than a run needs.
#[test]
fn prose_in_one_column_or_two_is_not_a_table() {
    let mut paragraph = String::new();
    for row in 0..12 {
        paragraph.push_str(&format!(
            "BT /F1 10 Tf 72 {} Td ({}) Tj ET\n",
            700 - row * 12,
            prose(row, 90)
        ));
    }
    let found = drawn(&paragraph)
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert!(found.tables.is_empty());

    let mut columns = String::new();
    for row in 0..20 {
        for (x, seed) in [(72, row), (324, 100 + row)] {
            columns.push_str(&format!(
                "BT /F1 10 Tf {x} {} Td ({}) Tj ET\n",
                700 - row * 12,
                prose(seed, 40)
            ));
        }
    }
    let found = drawn(&columns)
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert!(found.tables.is_empty(), "{} tables", found.tables.len());

    let two = drawn(concat!(
        "BT /F1 10 Tf 72 700 Td (Name) Tj ET BT /F1 10 Tf 200 700 Td (Price) Tj ET\n",
        "BT /F1 10 Tf 72 688 Td (Apple) Tj ET BT /F1 10 Tf 200 688 Td (1.20) Tj ET\n",
    ));
    assert!(two
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default())
        .tables
        .is_empty());
}

/// **An unruled table tagged as one is found as one**, against the tree it
/// hid: four rows of three short cells at recurring left edges — each row
/// set up to a point off the others, as a producer rounding positions sets
/// them, which the quarter-em tolerance absorbs — drawn a column at a time.
#[test]
fn an_unruled_tagged_table_is_found_by_its_alignment() {
    let cells = [
        ["Name", "Weight", "Price"],
        ["Apple", "150", "1.20"],
        ["Banana", "120", "0.80"],
        ["Cherry", "5", "0.10"],
    ];
    // The tagging API orders by key within a parent, so the table is built
    // with its cells keyed in reading order and drawn column by column.
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    builder.add_page(612.0, 792.0, |page| {
        page.tagged_with(&Tag::new(b"Table"), |page| {
            for c in 0..3 {
                for (r, row) in cells.iter().enumerate() {
                    let order = (r * 3 + c) as u64;
                    page.tagged_with(&Tag::new(b"TR").keyed(1000 + r as u64, r as u64), |page| {
                        page.tagged_with(&Tag::new(b"TD").keyed(order + 1, order), |page| {
                            let jitter = [0.0, 0.8, -0.6, 1.0][r];
                            page.text(
                                b"F1",
                                10.0,
                                72.0 + c as f64 * 90.0 + jitter,
                                700.0 - r as f64 * 14.0,
                                row[c],
                            );
                        });
                    });
                }
            }
        });
    });
    let doc = open(builder.finish());
    let page = doc.page(0).expect("a page");
    let stated = page.stated_tables();
    assert_eq!(stated.len(), 1);
    assert_eq!((stated[0].rows, stated[0].columns), (4, 3));
    let inferred = page.inferred_tables(&hidden());
    let (found, grid, agreeing, total) = score(&stated[0], &inferred.tables);
    println!("unruled: found {found} grid {grid} cells {agreeing}/{total}");
    assert!(found && grid);
    assert_eq!(agreeing, total);
    assert_eq!(inferred.tables[0].evidence, TableEvidence::Aligned);
}

/// **Monospaced text is not a table.** A listing in Courier whose every word
/// is four letters long: each word starts where the one above it does, so a
/// line cut at its spaces would be a grid of aligned columns. A Courier space
/// is three fifths of an em, short of [`tinker_pdf::tables::ALIGNED_GAP_EMS`],
/// so each line is one fragment and nothing aligns.
#[test]
fn a_monospaced_listing_is_not_a_table() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F2", b"Courier");
    builder.add_page(612.0, 792.0, |page| {
        for row in 0..10 {
            let words: Vec<&str> = (0..8)
                .map(|w| ["load", "push", "jump", "test", "move", "call"][(row * 5 + w * 3) % 6])
                .collect();
            page.text(
                b"F2",
                10.0,
                72.0,
                700.0 - row as f64 * 12.0,
                &words.join(" "),
            );
        }
    });
    let doc = open(builder.finish());
    let found = doc
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert!(found.tables.is_empty(), "{} tables", found.tables.len());
}

/// **Columns aligned with spaces in one string are columns.** Each row of a
/// table drawn as a single string, its cells padded apart with runs of
/// spaces, as a plain-text report is: the spaces are glyphs, and they are the
/// gap, not ink across it, so the rows still cut into their cells.
#[test]
fn columns_aligned_with_spaces_are_found() {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F2", b"Courier");
    builder.add_page(612.0, 792.0, |page| {
        for (row, text) in [
            "Name      Weight    Price",
            "Apple     150       1.20",
            "Banana    120       0.80",
            "Cherry    5         0.10",
        ]
        .iter()
        .enumerate()
        {
            page.text(b"F2", 10.0, 72.0, 700.0 - row as f64 * 12.0, text);
        }
    });
    let doc = open(builder.finish());
    let found = doc
        .page(0)
        .expect("a page")
        .inferred_tables(&TableOptions::default());
    assert_eq!(found.tables.len(), 1);
    let table = &found.tables[0];
    assert_eq!((table.rows, table.columns), (4, 3));
    let texts: Vec<String> = table
        .cells
        .iter()
        .map(|c| c.text.trim().to_string())
        .collect();
    assert_eq!(
        texts,
        [
            "Name", "Weight", "Price", "Apple", "150", "1.20", "Banana", "120", "0.80", "Cherry",
            "5", "0.10"
        ]
    );
}
