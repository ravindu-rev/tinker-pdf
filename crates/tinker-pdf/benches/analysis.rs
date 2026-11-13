//! The analysis group: what a document's pages are inferred and judged to
//! be — its text, a redaction, reading order, tables and the three
//! validators — each over the same five pages, so that each relation tier P
//! states divides by the text of the pages it read; and what a page draws,
//! read without its pixels or from them (RC-17): the text page's box,
//! transparency and overprint from its display list, and its grey test.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `analysis` list names these
//! operations in this order. Five pages bring the text and the inferences
//! inside the set's 20-300 ms window on the hosted runner
//! (`docs/design/performance.md`); the redaction and the validators, shorter,
//! repeat inside an iteration, their unit counting every call. The page
//! analysis rows have no target in tier P's table and no run has timed them,
//! so the design's analysis table gives their window as untimed; the
//! planning baseline, which has neither answer, renders the same page in each
//! one's place.

mod support;

use std::cell::LazyCell;
use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use support::{inputs, Group, Per};
use tinker_pdf::redact::{self, Redaction};
use tinker_pdf::{
    Bitmap, DisplayList, Document, InferenceOptions, Page, PageTables, Rect, RenderOptions,
    TableSource,
};
#[cfg(not(tinker_planning_baseline))]
use tinker_pdf::{GreyThresholds, Greyness, PageAnalysis};

/// The pages each operation reads.
const PAGES: usize = 5;

/// Every page's text, the denominator the group's relations divide by.
fn text_of(doc: &Document) -> usize {
    doc.pages()
        .into_iter()
        .map(|page| page.text().plain_text().len())
        .sum()
}

/// The analyses of the text page's list one iteration makes.
const ANALYSES: u64 = 5;

/// The grey tests of the text page one iteration makes.
const GREY_TESTS: u64 = 2;

/// The text page's box, transparency and overprint at 150 dpi, read from its
/// display list in one walk of its events (`DisplayList::analysis`).
#[cfg(not(tinker_planning_baseline))]
fn analysed(list: &DisplayList) -> PageAnalysis {
    list.analysis(&RenderOptions::at_dpi(150.0))
}

/// The planning baseline has no page analysis: its arm renders the page from
/// the same list at 150 dpi, the render the analysis is paired with.
#[cfg(tinker_planning_baseline)]
fn analysed(list: &DisplayList) -> Bitmap {
    list.render(&RenderOptions::at_dpi(150.0))
}

/// Whether the text page needs colour: drawn at 72 dpi and every pixel read
/// (`Page::greyness`), at the default thresholds.
#[cfg(not(tinker_planning_baseline))]
fn grey_test(page: &Page) -> Option<Greyness> {
    page.greyness(&RenderOptions::at_dpi(72.0), &GreyThresholds::default())
}

/// The planning baseline has no grey test: its arm draws the page at 72 dpi,
/// the render the test reads and is paired with.
#[cfg(tinker_planning_baseline)]
fn grey_test(page: &Page) -> Bitmap {
    page.render(&RenderOptions::at_dpi(72.0))
}

/// Before its runs, the text page's analysis holds what the page draws: its
/// marks are its 70 lines of glyphs, set from 36 points in and from a first
/// baseline at 806 down to a last at 47, inside the page; nothing on it is
/// transparent and nothing overprints.
#[cfg(not(tinker_planning_baseline))]
fn holds_the_analysis(list: &DisplayList) {
    let analysis = analysed(list);
    let marks = analysis.marks.expect("the text page marks itself");
    let (x0, y0, x1, y1) = marks.points;
    assert!(
        (0.0..45.0).contains(&x0) && (200.0..=595.0).contains(&x1),
        "the lines' box across the page: {:?}",
        marks.points
    );
    assert!(
        (0.0..60.0).contains(&y0) && (800.0..=842.0).contains(&y1),
        "the lines' box down the page: {:?}",
        marks.points
    );
    assert!(!analysis.transparency, "the text page uses no transparency");
    assert!(
        !analysis.overprint.used(),
        "nothing on the text page overprints: {:?}",
        analysis.overprint
    );
}

/// The planning baseline answers nothing to hold.
#[cfg(tinker_planning_baseline)]
fn holds_the_analysis(_list: &DisplayList) {}

/// Before its runs, the grey test holds what the page draws: black glyphs on
/// white paper, grey at every coverage, so no pixel of the 595 by 842 render
/// is coloured.
#[cfg(not(tinker_planning_baseline))]
fn holds_the_grey_test(page: &Page) {
    let greyness = grey_test(page).expect("a render is a picture");
    assert!(
        greyness.grey && greyness.coloured == 0,
        "the text page is grey: {greyness:?}"
    );
    assert_eq!(
        greyness.pixels,
        595 * 842,
        "every pixel of the render is read"
    );
}

/// The planning baseline answers nothing to hold.
#[cfg(tinker_planning_baseline)]
fn holds_the_grey_test(_page: &Page) {}

/// A render's pixels, as many times as an iteration draws it.
fn pixels_of(bitmap: &Bitmap, times: u64) -> Per {
    Per::Pixels(u64::from(bitmap.width) * u64::from(bitmap.height) * times)
}

fn analysis(c: &mut Criterion) {
    let report = LazyCell::new(|| {
        let report = Document::open(
            inputs::store().bytes(&format!("report-{PAGES}.pdf"), || inputs::report(PAGES)),
        )
        .expect("the report opens");
        assert_eq!(report.page_count() as usize, PAGES);
        report
    });
    // Each validator reads the claim it is given, so each runs its rules.
    let claiming = LazyCell::new(|| {
        let claiming = Document::open(
            inputs::store().bytes(&format!("claiming-{PAGES}.pdf"), || inputs::claiming(PAGES)),
        )
        .expect("the claiming file opens");
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
        let report: &Document = &report;
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
        op.iter(move || {
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
        let report: &Document = &report;
        let options = InferenceOptions::default();
        let pages = 0..PAGES as u32;
        assert_eq!(report.inferred_orders(pages.clone(), &options).len(), PAGES);
        op.per(Per::Pages(PAGES as u64));
        op.iter(move || black_box(report.inferred_orders(pages.clone(), &options)));
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

    // ---- what a page draws (RC-17) --------------------------------------

    // The render group's text page, `dense(1, 0)`, under the same stored
    // name, so a paired run's three builds read one file: its embedded
    // face's glyphs are every mark, so its box is read from their outlines.
    let text_page = LazyCell::new(|| {
        Document::open(inputs::store().bytes("dense-1.pdf", || inputs::dense(1, 0)))
            .expect("the text page opens")
            .page(0)
            .expect("a page")
    });

    // The page's display list, recorded once untimed; what is timed is the
    // walk of its events that answers the box, transparency and overprint
    // together. The planning baseline renders the page from the same list at
    // 150 dpi in its place.
    group.operation(
        "the text page's box, transparency and overprint at 150 dpi",
        |op| {
            let list = text_page.display_list();
            holds_the_analysis(&list);
            op.per(Per::Pages(ANALYSES));
            op.iter(move || {
                for _ in 0..ANALYSES {
                    black_box(analysed(&list));
                }
            });
        },
    );

    // Pixels decide the grey test, so it draws the page and reads every pixel
    // of the render; the planning baseline draws the same render in its
    // place. The unit is the render's pixels.
    group.operation("the text page's grey test at 72 dpi", |op| {
        let page: &Page = &text_page;
        holds_the_grey_test(page);
        op.per(pixels_of(
            &page.render(&RenderOptions::at_dpi(72.0)),
            GREY_TESTS,
        ));
        op.iter(move || {
            for _ in 0..GREY_TESTS {
                black_box(grey_test(page));
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
