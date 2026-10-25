//! The open group: a document opened from its bytes.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `open` list names these operations
//! in this order.

mod support;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::{inputs, Per};
use tinker_pdf::Document;

fn open(c: &mut Criterion) {
    let text = inputs::three_pages();

    let mut group = c.benchmark_group("open");
    group.sampling_mode(SamplingMode::Flat);

    group.throughput(Per::Bytes(text.len() as u64).throughput());
    group.bench_function("open a 3-page document", |b| {
        b.iter(|| black_box(Document::open(text.clone()).expect("it opens")));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = open
}
criterion_main!(benches);
