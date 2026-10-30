//! The calibration group: the one operation every calibrated target is a
//! fraction of, text of 100 dense pages.
//!
//! **This file is frozen.** It is the same input and call as the text
//! group's `text of 100 dense pages`, written out here rather than shared,
//! so that nothing a later row adds to the text group or to the support
//! module can move it; a change to it re-states every calibrated target,
//! which is the owner's (H-77). The harness and the rules every name here
//! keeps are `support/mod.rs`'s; `baseline.json`'s `calibration` list names
//! this operation.

mod support;

use std::hint::black_box;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::{inputs, Per};
use tinker_pdf::Document;

fn calibration(c: &mut Criterion) {
    let dense: Arc<[u8]> = inputs::dense(100, 0).into();
    let held = Document::open(Arc::clone(&dense)).expect("it opens");
    assert_eq!(held.page_count(), 100, "the dense pages");
    assert!(
        held.page(0)
            .expect("a page")
            .text()
            .plain_text()
            .contains(inputs::PHRASE),
        "the dense pages do not read their text"
    );

    let mut group = c.benchmark_group("calibration");
    group.sampling_mode(SamplingMode::Flat);

    group.throughput(Per::Pages(100).throughput());
    group.bench_function("text of 100 dense pages", |b| {
        b.iter(|| {
            let doc = Document::open(Arc::clone(&dense)).expect("it opens");
            let mut read = 0usize;
            for index in 0..doc.page_count() {
                if let Some(page) = doc.page(index) {
                    read += page.text().plain_text().len();
                }
            }
            black_box(read)
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = calibration
}
criterion_main!(benches);
