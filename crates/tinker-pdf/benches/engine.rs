//! Wall-clock benchmarks, deliberately outside the test suite.
//!
//! Clocks are banned from assertions in this repository — `bounds_ledger.rs`
//! bans `Instant::now` from itself, so that every bound is a property of an
//! input rather than of a machine. The cost of that rule is that every
//! performance number in `docs/` has been a one-time measurement with a date
//! beside it and nothing to stop it regressing.
//!
//! This is where the clocks live instead. Criterion is exempt tooling by name
//! (`Cargo.toml`'s header rule, `CONTRIBUTING.md`): it measures the engine and
//! is not part of it, nothing here ships, and `default-features = false` keeps
//! its plotting stack out of the tree. `cargo bench` runs on a schedule rather
//! than on the path between a push and a review, because a benchmark that
//! gates a pull request measures the runner it happened to land on.
//!
//! Regression comparison is the point, and it is criterion's to do:
//!
//! ```sh
//! cargo bench -p tinker-pdf -- --save-baseline before
//! # ... change something ...
//! cargo bench -p tinker-pdf -- --baseline before
//! ```
//!
//! Six operations, chosen because they are the ones the docs quote numbers
//! for: opening, rendering text, rendering a shading, extracting text,
//! rewriting a document, and paginating a book. Each is built or read from a
//! committed fixture, so a run needs no corpus.

use std::hint::black_box;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};
use tinker_pdf::{
    DeviceSpace, Document, DocumentBuilder, Function, OpenOptions, RenderOptions, Shading,
    WriteMode, WriteOptions,
};

fn text_document() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    for page in 0..3 {
        builder.add_page(595.0, 842.0, |p| {
            for line in 0..40 {
                p.text(
                    b"F0",
                    11.0,
                    72.0,
                    780.0 - f64::from(line) * 16.0,
                    &format!("Page {page}, line {line}: the quick brown fox jumps over it."),
                );
            }
        });
    }
    builder.finish()
}

/// One page carrying an axial shading over its whole area.
fn shading_document() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    let shading = Shading::Axial {
        color_space: DeviceSpace::Rgb,
        coords: [0.0, 0.0, 595.0, 842.0],
        function: Function::Exponential {
            domain: [0.0, 1.0],
            c0: vec![0.0, 0.1, 0.6],
            c1: vec![1.0, 0.9, 0.2],
            n: 1.0,
        },
        extend: (true, true),
    };
    builder.add_shading(b"S0", &shading);
    builder.add_page(595.0, 842.0, |page| {
        page.shading(b"S0");
    });
    builder.finish()
}

fn book() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/epub/pandoc-book-cover.epub"
    );
    std::fs::read(path).expect("the committed book")
}

fn benchmarks(c: &mut Criterion) {
    let text = text_document();
    let shading = shading_document();
    let epub = book();

    c.bench_function("open a 3-page document", |b| {
        b.iter(|| black_box(Document::open(text.clone()).expect("it opens")));
    });

    let doc = Document::open(text.clone()).expect("it opens");
    let page = doc.page(0).expect("a page");
    c.bench_function("render text at 150 dpi", |b| {
        b.iter(|| black_box(page.render(&RenderOptions::at_dpi(150.0))));
    });
    c.bench_function("extract a page of text", |b| {
        b.iter(|| black_box(page.text().plain_text()));
    });

    let shaded = Document::open(shading).expect("it opens");
    let shaded_page = shaded.page(0).expect("a page");
    // Its own group, with fewer samples and a longer window: a full-page axial
    // shading is two orders of magnitude slower than the rest of this file, and
    // criterion's default hundred samples would spend a quarter of a minute on
    // it alone.
    let mut slow = c.benchmark_group("shading");
    slow.sample_size(20);
    slow.measurement_time(Duration::from_secs(10));
    slow.bench_function("render a full-page axial shading at 150 dpi", |b| {
        b.iter(|| black_box(shaded_page.render(&RenderOptions::at_dpi(150.0))));
    });
    drop(slow);

    c.bench_function("rewrite a document", |b| {
        b.iter(|| {
            black_box(doc.editor().save(&WriteOptions {
                mode: WriteMode::Rewrite,
                ..WriteOptions::default()
            }))
        });
    });

    // The one whose answer is not a property of the file: an EPUB's page count
    // is a function of the box it is asked for (ruling 4's second half), so
    // this measures pagination rather than parsing.
    c.bench_function("paginate a book at 432x648 pt", |b| {
        b.iter(|| {
            black_box(
                Document::open_with(epub.clone(), &OpenOptions::at_page(432.0, 648.0))
                    .expect("it opens")
                    .page_count(),
            )
        });
    });
}

criterion_group!(engine, benchmarks);
criterion_main!(engine);
