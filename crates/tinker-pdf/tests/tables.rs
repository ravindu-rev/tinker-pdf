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
use tinker_pdf::{Document, StatedTable, TableAttributes, TableScope, TableWarning, Tag};
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
