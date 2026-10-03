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
    Document, InferredTable, PageTables, StatedTable, TableAttributes, TableEvidence, TableOptions,
    TableRule, TableRules, TableScope, TableSource, TableWarning, Tag,
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
