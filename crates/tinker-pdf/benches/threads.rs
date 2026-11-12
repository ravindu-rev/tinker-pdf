//! The threads group: the CLI's pool, `tpdf render --jobs`, rendering the
//! same pages on 1, 2, 4 and 8 jobs (MS-09).
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `threads` list names these
//! operations in this order. What the group is for is the ratios between
//! them, which come from one run of one binary on one host; the 300-dpi
//! page in four bands is the render group's, beside the one-thread render it
//! is compared to there, so it is timed once.
//!
//! The input is sized into the set's 20-300 ms window by area rather than by
//! count: a page drawn to PNG through `tpdf` costs about sixteen times what a
//! quarter-size one does, so the pages are a quarter of A4 each way, and 24
//! of them, so that each of eight jobs still takes three and a ratio measures
//! how the pool scales rather than how a few pages fall between the jobs.
//! They embed their face, as the text page does, so every glyph is drawn.

mod support;

use std::cell::LazyCell;
use std::path::Path;

use criterion::{criterion_group, criterion_main, Criterion};
use support::cli::{self, Cli};
use support::{inputs, Group, Op, Per};
use tinker_pdf::{Document, RenderOptions, RenderWarning};

/// How many pages every operation renders.
const PAGES: u32 = 24;

fn threads(c: &mut Criterion) {
    let pages = LazyCell::new(|| {
        let bytes = inputs::dense_part(PAGES as usize, 0, 4);
        let doc = Document::open(bytes.clone()).expect("the pages open");
        assert_eq!(doc.page_count(), PAGES, "the input's pages");
        let drawn = doc
            .page(0)
            .expect("a page")
            .render(&RenderOptions::at_dpi(150.0));
        assert!(
            !drawn.warnings.contains(&RenderWarning::UnreadableFont),
            "the pages' face is read: {:?}",
            drawn.warnings
        );
        assert!(
            drawn.data.iter().any(|&v| v < 128),
            "the first page inks its glyphs"
        );
        bytes
    });
    let pdf = LazyCell::new(|| cli::fixture("dense-quarter-24.pdf", || pages.clone()));
    let size = LazyCell::new(|| {
        let bitmap = Document::open(pages.clone())
            .expect("the pages open")
            .page(0)
            .expect("a page")
            .render(&RenderOptions::at_dpi(150.0));
        (bitmap.width, bitmap.height)
    });

    let mut group = Group::new(c, "threads");

    group.operation(
        "24 quarter-size text pages to PNG at 150 dpi, 1 job, CLI",
        |op| render_on(op, &pdf, "threads-jobs-1", "1", *size),
    );
    group.operation(
        "24 quarter-size text pages to PNG at 150 dpi, 2 jobs, CLI",
        |op| render_on(op, &pdf, "threads-jobs-2", "2", *size),
    );
    group.operation(
        "24 quarter-size text pages to PNG at 150 dpi, 4 jobs, CLI",
        |op| render_on(op, &pdf, "threads-jobs-4", "4", *size),
    );
    group.operation(
        "24 quarter-size text pages to PNG at 150 dpi, 8 jobs, CLI",
        |op| render_on(op, &pdf, "threads-jobs-8", "8", *size),
    );

    group.finish();
}

/// `tpdf render` of every page of `pdf` at 150 dpi on `jobs` jobs, one
/// child a call, its run kept under `slug`; then the PNGs the last call
/// wrote, one a page, in order, each the page's `size`.
fn render_on(op: &mut Op<'_, '_>, pdf: &Path, slug: &str, jobs: &str, size: (u32, u32)) {
    let cli = Cli::new(slug);
    let out = cli.out("png");
    op.per(Per::Pages(u64::from(PAGES)));
    op.iter(|| {
        cli.run(&[
            &"render", &pdf, &"--out", &out, &"--dpi", &"150", &"--jobs", &jobs,
        ])
    });
    let pngs = cli.pngs(&out);
    assert_eq!(pngs.len(), PAGES as usize, "a PNG a page on {jobs} jobs");
    for (at, (name, width, height)) in pngs.iter().enumerate() {
        assert_eq!(name, &format!("dense-quarter-24-{:04}.png", at + 1));
        assert_eq!((*width, *height), size, "{name} is the page at 150 dpi");
    }
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = threads
}
criterion_main!(benches);
