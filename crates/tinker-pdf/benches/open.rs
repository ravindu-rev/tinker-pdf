//! The open group: a document opened from its bytes.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `open` list names these operations
//! in this order. The AES-256 file declares no unit, and criterion keeps a
//! group's throughput from one operation to the next, so it comes first.
//!
//! An operation shorter than the set's 20-300 ms window repeats its call
//! inside one iteration, as many times as land it near the window's middle
//! on the hosted runner (`docs/design/performance.md`); its unit counts every
//! call.

mod support;

use std::cell::LazyCell;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};
use support::cli::{self, Cli};
use support::{inputs, Group, Per};
use tinker_pdf::{CountingSource, Document, RenderOptions, SliceSource};

fn open(c: &mut Criterion) {
    let small_3000 = LazyCell::new(|| small(3000));
    let small_10000 = LazyCell::new(|| small(10000));

    let mut group = Group::new(c, "open");

    group.operation("open the AES-256 file", |op| {
        let aes = inputs::aes_file();
        op.iter(|| {
            for _ in 0..6 {
                let doc = Document::open(Arc::clone(&aes)).expect("it opens");
                doc.authenticate("").expect("the empty password opens it");
                black_box(doc.page_count());
            }
        });
    });

    group.operation("open a 3-page document", |op| {
        let text = inputs::three_pages();
        op.per(Per::Bytes(text.len() as u64));
        op.iter(|| black_box(Document::open(text.clone()).expect("it opens")));
    });

    group.operation("open and count the 23 MB image file", |op| {
        let image = inputs::image_file();
        op.per(Per::Bytes(100 * image.len() as u64));
        op.iter(|| {
            for _ in 0..100 {
                black_box(
                    Document::open(Arc::clone(&image))
                        .expect("it opens")
                        .page_count(),
                );
            }
        });
    });

    group.operation("every page's size, 3000 pages", |op| {
        op.per(Per::Pages(12 * 3000));
        op.iter(|| {
            for _ in 0..12 {
                black_box(every_page_size(&small_3000));
            }
        });
    });

    group.operation("every page's size, 10000 pages", |op| {
        op.per(Per::Pages(4 * 10000));
        op.iter(|| {
            for _ in 0..4 {
                black_box(every_page_size(&small_10000));
            }
        });
    });

    // The image file through the CLI, twenty-four children an iteration:
    // tier P's figure includes the process.
    group.operation("open and count the 23 MB image file, CLI", |op| {
        let image = inputs::image_file();
        let image_pdf = cli::fixture("image.pdf", || image.to_vec());
        let info = Cli::new("open-image-info");
        op.per(Per::Bytes(24 * image.len() as u64));
        op.iter(|| {
            for _ in 0..24 {
                info.run(&[&"info", &image_pdf]);
            }
        });
        let pages = Document::open(Arc::clone(&image))
            .expect("it opens")
            .page_count();
        assert!(
            info.stdout().contains(&format!("  pages       {pages}\n")),
            "tpdf info counts the image file's {pages} pages"
        );
    });

    // Tier P's time to first page over a slow link (MS-08): each iteration
    // opens the fixture over a fresh counting source and renders page one,
    // and its time is that work plus the link time of what it read, at 50 ms
    // a request and 10 Mbit/s. Nothing sleeps; the counts do not vary, so the
    // link part is a constant (`streaming_open.rs` holds them).
    group.operation(
        "first page of the 60-page linearized fixture over a slow link",
        |op| {
            let fixture: Arc<[u8]> = inputs::linearized(60).into();
            op.per(Per::Pages(1));
            op.iter_plus(|| {
                let source = Arc::new(CountingSource::new(SliceSource::new(fixture.to_vec())));
                let doc = Document::open_streaming(source.clone()).expect("it opens");
                black_box(
                    doc.page(0)
                        .expect("page one")
                        .render(&RenderOptions::default()),
                );
                source.link_time(Duration::from_millis(50), 10_000_000)
            });
        },
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
