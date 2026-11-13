//! The text group: text read out of pages.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `text` list names these operations
//! in this order. Each input is asserted to be what its operation reads
//! before that operation is timed: its pages, its face embedded, its text
//! holding what each search finds.
//!
//! Each operation is sized into the set's 20-300 ms window on the hosted
//! runner (`docs/design/performance.md`): five dense pages where a whole
//! read of the dense text is longer, twenty standard-14 pages, 4 000 small
//! pages beside 1 000, and the shorter ones repeated inside an iteration,
//! their unit counting every call.

mod support;

use std::cell::LazyCell;
use std::hint::black_box;
use std::path::Path;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use support::cli::Cli;
use support::{inputs, Group, Per};
use tinker_pdf::{Document, SearchOptions, TextFormat, TextPage, TextWriter};

fn text(c: &mut Criterion) {
    let dense = LazyCell::new(|| dense_pages(100));
    let dense_5 = LazyCell::new(|| dense_pages(5));
    let unaccented = SearchOptions {
        whole_word: true,
        diacritic_insensitive: true,
        ..SearchOptions::default()
    };
    let prebuilt = LazyCell::new(|| {
        let held = Document::open(Arc::clone(&dense)).expect("it opens");
        let prebuilt: Vec<TextPage> = held.pages().iter().map(|page| page.text()).collect();
        for (index, text) in prebuilt.iter().enumerate() {
            assert!(
                text.plain_text().contains(inputs::PHRASE),
                "dense page {index} does not read its phrase"
            );
            assert!(
                !text.search(inputs::PHRASE).is_empty(),
                "the phrase is not found on dense page {index}"
            );
            assert!(
                !text.search_with(inputs::UNACCENTED, &unaccented).is_empty(),
                "`{}` is not found, whole and unaccented, on dense page {index}",
                inputs::UNACCENTED
            );
        }
        prebuilt
    });
    let small_1000 = LazyCell::new(|| small_pages(1000));
    let small_4000 = LazyCell::new(|| small_pages(4000));
    let dense_pdf = LazyCell::new(|| inputs::store().file("dense-5.pdf", || dense_5.to_vec()));

    let mut group = Group::new(c, "text");

    group.operation("extract a page of text", |op| {
        let doc = Document::open(inputs::store().bytes("three-pages.pdf", inputs::three_pages))
            .expect("it opens");
        let page = doc.page(0).expect("a page");
        op.per(Per::Pages(1));
        op.iter(move || black_box(page.text().plain_text()));
    });

    group.operation("text of 5 dense pages", |op| {
        op.per(Per::Pages(5));
        op.iter(|| black_box(every_page_text(&dense_5)));
    });

    group.operation("text of 20 standard-14 pages", |op| {
        let producer = pages(
            inputs::store().bytes("producer-20.pdf", || inputs::producer(20)),
            20,
        );
        let standard = Document::open(Arc::clone(&producer)).expect("it opens");
        assert!(
            standard
                .page(0)
                .expect("a page")
                .text()
                .plain_text()
                .contains(inputs::PHRASE),
            "the standard-14 pages do not read their phrase"
        );
        op.per(Per::Pages(20));
        op.iter(move || black_box(every_page_text(&producer)));
    });

    group.operation("text of 1000 small pages", |op| {
        op.per(Per::Pages(2 * 1000));
        op.iter(|| {
            for _ in 0..2 {
                black_box(every_page_text(&small_1000));
            }
        });
    });

    group.operation("text of 4000 small pages", |op| {
        op.per(Per::Pages(4000));
        op.iter(|| black_box(every_page_text(&small_4000)));
    });

    group.operation("structured JSON of 5 pages", |op| {
        op.per(Per::Pages(5));
        op.iter(|| {
            let doc = Document::open(Arc::clone(&dense_5)).expect("it opens");
            let mut writer = TextWriter::new(TextFormat::Json);
            let mut written = 0usize;
            for index in 0..doc.page_count() {
                if let Some(page) = doc.page(index) {
                    writer.page(&page.text_frame(), &page.text());
                    written += writer.take().len();
                }
            }
            written += writer.finish().len();
            black_box(written)
        });
    });

    group.operation("search a phrase over 5 pages from scratch", |op| {
        op.per(Per::Pages(5));
        op.iter(|| {
            let doc = Document::open(Arc::clone(&dense_5)).expect("it opens");
            let mut hits = 0usize;
            for index in 0..doc.page_count() {
                if let Some(page) = doc.page(index) {
                    hits += page.text().search(inputs::PHRASE).len();
                }
            }
            black_box(hits)
        });
    });

    group.operation("search 100 pages' prebuilt text", |op| {
        op.per(Per::Pages(500));
        op.iter(|| {
            let mut hits = 0usize;
            for _ in 0..5 {
                for text in prebuilt.iter() {
                    hits += text.search(inputs::PHRASE).len();
                }
            }
            black_box(hits)
        });
    });

    group.operation(
        "search 100 pages' prebuilt text, whole words, unaccented",
        |op| {
            op.per(Per::Pages(200));
            op.iter(|| {
                let mut hits = 0usize;
                for _ in 0..2 {
                    for text in prebuilt.iter() {
                        hits += text.search_with(inputs::UNACCENTED, &unaccented).len();
                    }
                }
                black_box(hits)
            });
        },
    );

    // The same text through the CLI, one child a call: plain text with a
    // form feed after every page, and the structured text as JSON.
    group.operation("text of 5 dense pages, CLI", |op| {
        let dense_pdf: &Path = &dense_pdf;
        let dense_text = Cli::new("text-dense-5");
        let ran = dense_text.clone();
        op.per(Per::Pages(5));
        op.iter(move || dense_text.run(&[&"text", &dense_pdf]));
        op.after(move || {
            assert_eq!(ran.stdout().matches('\u{c}').count(), 5);
            assert_eq!(
                ran.stdout().matches(inputs::PHRASE).count(),
                5,
                "every dense page's phrase"
            );
        });
    });
    group.operation("text of 1000 small pages, CLI", |op| {
        let small_1000_pdf = inputs::store().file("small-1000.pdf", || small_1000.to_vec());
        let small_text = Cli::new("text-small-1000");
        let ran = small_text.clone();
        op.per(Per::Pages(2 * 1000));
        op.iter(move || {
            for _ in 0..2 {
                small_text.run(&[&"text", &small_1000_pdf]);
            }
        });
        op.after(move || assert_eq!(ran.stdout().matches('\u{c}').count(), 1000));
    });
    group.operation("text of 4000 small pages, CLI", |op| {
        let small_4000_pdf = inputs::store().file("small-4000.pdf", || small_4000.to_vec());
        let more_text = Cli::new("text-small-4000");
        let ran = more_text.clone();
        op.per(Per::Pages(4000));
        op.iter(move || more_text.run(&[&"text", &small_4000_pdf]));
        op.after(move || assert_eq!(ran.stdout().matches('\u{c}').count(), 4000));
    });
    group.operation("structured JSON of 5 pages, CLI", |op| {
        let dense_pdf: &Path = &dense_pdf;
        let json = Cli::new("text-json-5");
        let ran = json.clone();
        op.per(Per::Pages(5));
        op.iter(move || json.run(&[&"text", &dense_pdf, &"--json"]));
        op.after(move || {
            let written = ran.stdout();
            assert!(written.starts_with("{\"format\":\"tinker-pdf/text\""));
            assert_eq!(
                written.matches("{\"index\":").count(),
                5,
                "the JSON's pages"
            );
        });
    });

    group.finish();
}

/// `count` dense pages, their face asserted embedded.
fn dense_pages(count: u32) -> Arc<[u8]> {
    let dense = pages(
        inputs::store().bytes(&format!("dense-{count}.pdf"), || {
            inputs::dense(count as usize, 0)
        }),
        count,
    );
    let held = Document::open(Arc::clone(&dense)).expect("it opens");
    assert!(
        held.fonts()
            .iter()
            .any(|font| font.name == "LiberationSerif" && font.program.is_some()),
        "the dense pages embed their face"
    );
    dense
}

/// `small(count)`, which opens to its pages.
fn small_pages(count: u32) -> Arc<[u8]> {
    pages(
        inputs::store().bytes(&format!("small-{count}.pdf"), || {
            inputs::small(count as usize)
        }),
        count,
    )
}

/// `bytes`, which open to `count` pages.
fn pages(bytes: Vec<u8>, count: u32) -> Arc<[u8]> {
    let bytes: Arc<[u8]> = bytes.into();
    let doc = Document::open(Arc::clone(&bytes)).expect("it opens");
    assert_eq!(doc.page_count(), count, "the input's pages");
    bytes
}

/// The document opened from `bytes`, and every page's text read, by index,
/// as `tpdf text` reads it.
fn every_page_text(bytes: &Arc<[u8]>) -> usize {
    let doc = Document::open(Arc::clone(bytes)).expect("it opens");
    let mut read = 0usize;
    for index in 0..doc.page_count() {
        if let Some(page) = doc.page(index) {
            read += page.text().plain_text().len();
        }
    }
    read
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = text
}
criterion_main!(benches);
