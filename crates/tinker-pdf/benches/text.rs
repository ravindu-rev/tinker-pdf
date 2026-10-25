//! The text group: text read out of pages.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `text` list names these operations
//! in this order.

mod support;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::{inputs, Per};
use tinker_pdf::Document;

fn text(c: &mut Criterion) {
    let doc = Document::open(inputs::three_pages()).expect("it opens");
    let page = doc.page(0).expect("a page");

    let mut group = c.benchmark_group("text");
    group.sampling_mode(SamplingMode::Flat);

    group.throughput(Per::Pages(1).throughput());
    group.bench_function("extract a page of text", |b| {
        b.iter(|| black_box(page.text().plain_text()));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = text
}
criterion_main!(benches);
