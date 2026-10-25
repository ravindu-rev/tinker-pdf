//! The render group: pages drawn into bitmaps.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `render` list names these operations
//! in this order.

mod support;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::inputs;
use tinker_pdf::{Document, RenderOptions};

fn render(c: &mut Criterion) {
    let doc = Document::open(inputs::three_pages()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let shaded = Document::open(inputs::axial()).expect("it opens");
    let shaded_page = shaded.page(0).expect("a page");
    let paths = Document::open(inputs::triangles()).expect("it opens");
    let paths_page = paths.page(0).expect("a page");

    let mut group = c.benchmark_group("render");
    group.sampling_mode(SamplingMode::Flat);

    // No glyph of this page is drawn in the default build: see
    // `inputs::triangles`.
    group.throughput(support::pixels(&page.render(&RenderOptions::at_dpi(150.0))).throughput());
    group.bench_function("render text at 150 dpi", |b| {
        b.iter(|| black_box(page.render(&RenderOptions::at_dpi(150.0))));
    });

    group.throughput(
        support::pixels(&shaded_page.render(&RenderOptions::at_dpi(150.0))).throughput(),
    );
    group.bench_function("render a full-page axial shading at 150 dpi", |b| {
        b.iter(|| black_box(shaded_page.render(&RenderOptions::at_dpi(150.0))));
    });

    group.throughput(
        support::pixels(&paths_page.render(&RenderOptions::at_dpi(150.0))).throughput(),
    );
    group.bench_function("fill 300 anti-aliased paths at 150 dpi", |b| {
        b.iter(|| black_box(paths_page.render(&RenderOptions::at_dpi(150.0))));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = render
}
criterion_main!(benches);
