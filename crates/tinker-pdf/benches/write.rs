//! The write group: documents saved.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `write` list names these operations
//! in this order.

mod support;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::{inputs, Per};
use tinker_pdf::{Document, WriteMode, WriteOptions};

fn write(c: &mut Criterion) {
    let doc = Document::open(inputs::three_pages()).expect("it opens");

    let mut group = c.benchmark_group("write");
    group.sampling_mode(SamplingMode::Flat);

    // A rewrite reads every object the file has (`edit/save.rs`'s walk of
    // the cross-reference table), so its first iteration leaves the document
    // as warm as any earlier read of it would have.
    group.throughput(Per::Pages(u64::from(doc.page_count())).throughput());
    group.bench_function("rewrite a document", |b| {
        b.iter(|| {
            black_box(doc.editor().save(&WriteOptions {
                mode: WriteMode::Rewrite,
                ..WriteOptions::default()
            }))
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = write
}
criterion_main!(benches);
