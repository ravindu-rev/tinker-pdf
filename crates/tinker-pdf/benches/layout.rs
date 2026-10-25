//! The layout group: reflowable documents laid out into pages.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `layout` list names these operations
//! in this order.

mod support;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::{inputs, Per};
use tinker_pdf::{Document, OpenOptions};

fn layout(c: &mut Criterion) {
    let epub = inputs::committed_book("pandoc-book-cover.epub");
    let pages = Document::open_with(epub.clone(), &OpenOptions::at_page(432.0, 648.0))
        .expect("it opens")
        .page_count();

    let mut group = c.benchmark_group("layout");
    group.sampling_mode(SamplingMode::Flat);

    // The one whose answer is not a property of the file: an EPUB's page count
    // is a function of the box it is asked for (ruling 4's second half), so
    // this measures pagination rather than parsing.
    group.throughput(Per::Pages(u64::from(pages)).throughput());
    group.bench_function("paginate a book at 432x648 pt", |b| {
        b.iter(|| {
            black_box(
                Document::open_with(epub.clone(), &OpenOptions::at_page(432.0, 648.0))
                    .expect("it opens")
                    .page_count(),
            )
        });
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = layout
}
criterion_main!(benches);
