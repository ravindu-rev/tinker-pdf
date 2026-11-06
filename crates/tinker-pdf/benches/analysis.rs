//! The analysis group: what a document's pages are inferred and judged to
//! be — its text, a redaction, reading order, tables and the three
//! validators — each over the same five pages, so that each relation tier P
//! states divides by the text of the pages it read.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `analysis` list names these
//! operations in this order. Five pages bring the text and the inferences
//! inside the set's 20-300 ms window on the hosted runner
//! (`docs/design/performance.md`); the redaction and the validators, shorter,
//! repeat inside an iteration, their unit counting every call.

mod support;

use std::cell::LazyCell;
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use support::{inputs, Group, Per};
use tinker_pdf::redact::{self, Redaction};
use tinker_pdf::{Document, InferenceOptions, PageTables, Rect, TableSource};

/// The pages each operation reads.
const PAGES: usize = 5;

/// Every page's text, the denominator the group's relations divide by.
fn text_of(doc: &Document) -> usize {
    doc.pages()
        .into_iter()
        .map(|page| page.text().plain_text().len())
        .sum()
}

fn analysis(c: &mut Criterion) {
    let report = LazyCell::new(|| {
        let report = Document::open(inputs::report(PAGES)).expect("the report opens");
        assert_eq!(report.page_count() as usize, PAGES);
        report
    });
    // Each validator reads the claim it is given, so each runs its rules.
    let claiming = LazyCell::new(|| {
        let claiming = Document::open(inputs::claiming(PAGES)).expect("the claiming file opens");
        assert_eq!(claiming.page_count() as usize, PAGES);
        assert!(claiming.validate_pdfa().flavour.is_some(), "a PDF/A claim");
        assert!(claiming.validate_pdfua().part.is_some(), "a PDF/UA claim");
        assert!(claiming.validate_pdfx().claim.is_some(), "a PDF/X claim");
        claiming
    });

    let mut group = Group::new(c, "analysis");

    group.operation("text of the 5 report pages", |op| {
        op.per(Per::Pages(PAGES as u64));
        op.iter(|| black_box(text_of(&report)));
    });

    // The rectangle over each page's running head, so every page has text
    // to cut; the editor is not saved, which is the write group's.
    group.operation("redact a rectangle on each of 5 pages", |op| {
        let head = Redaction {
            area: Rect {
                x0: 40.0,
                y0: 796.0,
                x1: 555.0,
                y1: 822.0,
            },
            mark: true,
        };
        let cut: usize = {
            let mut editor = report.editor();
            (0..PAGES as u32)
                .map(|page| {
                    redact::apply(&mut editor, page, std::slice::from_ref(&head))
                        .expect("the page exists")
                        .glyphs
                })
                .sum()
        };
        assert!(
            cut >= PAGES,
            "the redaction removed the text under it: {cut}"
        );
        op.per(Per::Pages(14 * PAGES as u64));
        op.iter(|| {
            for _ in 0..14 {
                let mut editor = report.editor();
                for page in 0..PAGES as u32 {
                    black_box(redact::apply(
                        &mut editor,
                        page,
                        std::slice::from_ref(&head),
                    ));
                }
            }
        });
    });

    group.operation("reading order inferred over 5 pages", |op| {
        let options = InferenceOptions::default();
        let pages = 0..PAGES as u32;
        assert_eq!(report.inferred_orders(pages.clone(), &options).len(), PAGES);
        op.per(Per::Pages(PAGES as u64));
        op.iter(|| black_box(report.inferred_orders(pages.clone(), &options)));
    });

    group.operation("tables inferred over 5 pages", |op| {
        assert!(
            report
                .pages()
                .into_iter()
                .all(|page| match page.tables(TableSource::Inferred) {
                    PageTables::Inferred(found) => !found.tables.is_empty(),
                    PageTables::Stated(stated) => !stated.is_empty(),
                }),
            "every report page's table is found"
        );
        op.per(Per::Pages(PAGES as u64));
        op.iter(|| {
            for page in report.pages() {
                black_box(page.tables(TableSource::Inferred));
            }
        });
    });

    group.operation("text of the 5 claiming pages", |op| {
        op.per(Per::Pages(PAGES as u64));
        op.iter(|| black_box(text_of(&claiming)));
    });
    group.operation("PDF-A validation of 5 pages", |op| {
        op.per(Per::Pages(10 * PAGES as u64));
        op.iter(|| {
            for _ in 0..10 {
                black_box(claiming.validate_pdfa());
            }
        });
    });
    group.operation("PDF-UA validation of 5 pages", |op| {
        op.per(Per::Pages(11 * PAGES as u64));
        op.iter(|| {
            for _ in 0..11 {
                black_box(claiming.validate_pdfua());
            }
        });
    });
    group.operation("PDF-X validation of 5 pages", |op| {
        op.per(Per::Pages(22 * PAGES as u64));
        op.iter(|| {
            for _ in 0..22 {
                black_box(claiming.validate_pdfx());
            }
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = analysis
}
criterion_main!(benches);
