//! The open group: a document opened from its bytes.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `open` list names these operations
//! in this order. The AES-256 file declares no unit, and criterion keeps a
//! group's throughput from one operation to the next, so it comes first.

mod support;

use std::hint::black_box;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::cli::{self, Cli};
use support::{inputs, Per};
use tinker_pdf::Document;

fn open(c: &mut Criterion) {
    let text = inputs::three_pages();
    let image = inputs::image_file();
    let aes = inputs::aes_file();
    let small_3000 = small(3000);
    let small_10000 = small(10000);

    let mut group = c.benchmark_group("open");
    group.sampling_mode(SamplingMode::Flat);

    group.bench_function("open the AES-256 file", |b| {
        b.iter(|| {
            let doc = Document::open(Arc::clone(&aes)).expect("it opens");
            doc.authenticate("").expect("the empty password opens it");
            black_box(doc.page_count())
        });
    });

    group.throughput(Per::Bytes(text.len() as u64).throughput());
    group.bench_function("open a 3-page document", |b| {
        b.iter(|| black_box(Document::open(text.clone()).expect("it opens")));
    });

    group.throughput(Per::Bytes(image.len() as u64).throughput());
    group.bench_function("open and count the 23 MB image file", |b| {
        b.iter(|| {
            black_box(
                Document::open(Arc::clone(&image))
                    .expect("it opens")
                    .page_count(),
            )
        });
    });

    group.throughput(Per::Pages(3000).throughput());
    group.bench_function("every page's size, 3000 pages", |b| {
        b.iter(|| black_box(every_page_size(&small_3000)));
    });

    group.throughput(Per::Pages(10000).throughput());
    group.bench_function("every page's size, 10000 pages", |b| {
        b.iter(|| black_box(every_page_size(&small_10000)));
    });

    // The image file through the CLI, five children an iteration: tier P's
    // figure includes the process.
    let image_pdf = cli::fixture("image.pdf", || image.to_vec());
    let info = Cli::new("open-image-info");
    group
        .throughput(Per::Bytes(5 * image.len() as u64).throughput())
        .bench_function("open and count the 23 MB image file, CLI", |b| {
            b.iter(|| {
                for _ in 0..5 {
                    info.run(&[&"info", &image_pdf]);
                }
            });
        });
    let pages = Document::open(Arc::clone(&image))
        .expect("it opens")
        .page_count();
    assert!(
        info.stdout().contains(&format!("  pages       {pages}\n")),
        "tpdf info counts the image file's {pages} pages"
    );

    group.finish();
}

/// `small(pages)`, its page count asserted.
fn small(pages: u32) -> Arc<[u8]> {
    let bytes: Arc<[u8]> = inputs::small(pages as usize).into();
    let doc = Document::open(Arc::clone(&bytes)).expect("the small pages open");
    assert_eq!(doc.page_count(), pages, "the small document's pages");
    bytes
}

/// The document opened from `bytes`, and every page's size read, by index.
fn every_page_size(bytes: &Arc<[u8]>) -> f64 {
    let doc = Document::open(Arc::clone(bytes)).expect("it opens");
    let mut area = 0.0;
    for index in 0..doc.page_count() {
        let (w, h) = doc.page(index).expect("a page").size();
        area += w * h;
    }
    area
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = open
}
criterion_main!(benches);
