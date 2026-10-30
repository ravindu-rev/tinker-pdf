//! The analysis group: what a document's pages are inferred and judged to
//! be — its text, a redaction, reading order, tables and the three
//! validators — each over 100 pages.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `analysis` list names these
//! operations in this order.

mod support;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::{inputs, Per};
use tinker_pdf::redact::{self, Redaction};
use tinker_pdf::{Document, InferenceOptions, PageTables, Rect, TableSource};

/// Every page's text, the denominator the group's relations divide by.
fn text_of(doc: &Document) -> usize {
    doc.pages()
        .into_iter()
        .map(|page| page.text().plain_text().len())
        .sum()
}

fn analysis(c: &mut Criterion) {
    let report = Document::open(inputs::report(100)).expect("the report opens");
    assert_eq!(report.page_count(), 100);
    let claiming = Document::open(inputs::claiming(100)).expect("the claiming file opens");
    assert_eq!(claiming.page_count(), 100);

    let mut group = c.benchmark_group("analysis");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Per::Pages(100).throughput());

    group.bench_function("text of the 100 report pages", |b| {
        b.iter(|| black_box(text_of(&report)));
    });

    // The rectangle over each page's running head, so every page has text
    // to cut; the editor is not saved, which is the write group's.
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
        (0..100)
            .map(|page| {
                redact::apply(&mut editor, page, std::slice::from_ref(&head))
                    .expect("the page exists")
                    .glyphs
            })
            .sum()
    };
    assert!(cut >= 100, "the redaction removed the text under it: {cut}");
    group.bench_function("redact a rectangle on each of 100 pages", |b| {
        b.iter(|| {
            let mut editor = report.editor();
            for page in 0..100 {
                black_box(redact::apply(
                    &mut editor,
                    page,
                    std::slice::from_ref(&head),
                ));
            }
        });
    });

    let options = InferenceOptions::default();
    assert_eq!(report.inferred_orders(0..100, &options).len(), 100);
    group.bench_function("reading order inferred over 100 pages", |b| {
        b.iter(|| black_box(report.inferred_orders(0..100, &options)));
    });

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
    group.bench_function("tables inferred over 100 pages", |b| {
        b.iter(|| {
            for page in report.pages() {
                black_box(page.tables(TableSource::Inferred));
            }
        });
    });

    // Each validator reads the claim it is given, so each runs its rules.
    assert!(claiming.validate_pdfa().flavour.is_some(), "a PDF/A claim");
    assert!(claiming.validate_pdfua().part.is_some(), "a PDF/UA claim");
    assert!(claiming.validate_pdfx().claim.is_some(), "a PDF/X claim");
    group.bench_function("text of the 100 claiming pages", |b| {
        b.iter(|| black_box(text_of(&claiming)));
    });
    group.bench_function("PDF-A validation of 100 pages", |b| {
        b.iter(|| black_box(claiming.validate_pdfa()));
    });
    group.bench_function("PDF-UA validation of 100 pages", |b| {
        b.iter(|| black_box(claiming.validate_pdfua()));
    });
    group.bench_function("PDF-X validation of 100 pages", |b| {
        b.iter(|| black_box(claiming.validate_pdfx()));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = analysis
}
criterion_main!(benches);
