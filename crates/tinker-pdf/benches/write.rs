//! The write group: documents saved.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `write` list names these operations
//! in this order.

mod support;

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use support::{inputs, Per};
use tinker_pdf::write::{self, FontPolicy, ImagePolicy, SaveOptions};
use tinker_pdf::{Document, WriteMode, WriteOptions};

/// A rewrite, as `tpdf` writes one.
fn rewrite() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    }
}

/// `pages` small pages, opened, their count asserted.
fn small(pages: usize) -> Document {
    let doc = Document::open(inputs::small(pages)).expect("the small pages open");
    assert_eq!(doc.page_count() as usize, pages);
    doc
}

/// `doc` saved under `options`, reopened, and its page count held to the
/// original's.
fn saved_reopens(doc: &Document, options: &WriteOptions) {
    let bytes = doc.editor().save(options);
    let again = Document::open(bytes).expect("the saved file reopens");
    assert_eq!(again.page_count(), doc.page_count());
}

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

    let small_100 = small(100);
    let small_1000 = small(1000);
    let small_3000 = small(3000);
    for doc in [&small_100, &small_1000, &small_3000] {
        saved_reopens(doc, &rewrite());
    }
    group.throughput(Per::Pages(100).throughput());
    group.bench_function("rewrite 100 pages", |b| {
        b.iter(|| black_box(small_100.editor().save(&rewrite())));
    });
    group.throughput(Per::Pages(1000).throughput());
    group.bench_function("rewrite 1000 pages", |b| {
        b.iter(|| black_box(small_1000.editor().save(&rewrite())));
    });
    group.throughput(Per::Pages(3000).throughput());
    group.bench_function("rewrite 3000 pages", |b| {
        b.iter(|| black_box(small_3000.editor().save(&rewrite())));
    });

    // Ten and seven saves an iteration, which brings each inside the set's
    // window; the unit counts every page each one wrote.
    let streams = WriteOptions {
        object_streams: true,
        ..rewrite()
    };
    let linear = WriteOptions {
        linearize: true,
        ..rewrite()
    };
    saved_reopens(&small_100, &streams);
    saved_reopens(&small_100, &linear);
    group.throughput(Per::Pages(10 * 100).throughput());
    group.bench_function("rewrite 100 pages into object streams", |b| {
        b.iter(|| {
            for _ in 0..10 {
                black_box(small_100.editor().save(&streams));
            }
        });
    });
    group.throughput(Per::Pages(7 * 100).throughput());
    group.bench_function("rewrite 100 pages linearized", |b| {
        b.iter(|| {
            for _ in 0..7 {
                black_box(small_100.editor().save(&linear));
            }
        });
    });

    // The facade's door with its default policies: a rewrite, every
    // embedded program cut to what the pages draw.
    let dense = Document::open(inputs::dense(100, 0)).expect("the dense pages open");
    let subset = SaveOptions {
        write: rewrite(),
        ..SaveOptions::default()
    };
    let saved = write::save(&mut dense.editor(), &subset).bytes;
    assert!(
        saved.len() < inputs::dense(100, 0).len(),
        "the subset file is smaller than the whole face's"
    );
    assert_eq!(Document::open(saved).expect("it reopens").page_count(), 100);
    group.throughput(Per::Pages(100).throughput());
    group.bench_function("subset-on-save of 100 pages", |b| {
        b.iter(|| black_box(write::save(&mut dense.editor(), &subset)));
    });

    // `tpdf merge`'s shape: the first file's editor, every page of the
    // others imported after its own, saved with stream deduplication and
    // the programs kept as they are.
    let parts: Vec<Document> = (0..4)
        .map(|seed| Document::open(inputs::dense(100, seed)).expect("a part opens"))
        .collect();
    let merged = SaveOptions {
        write: WriteOptions {
            deduplicate_streams: true,
            ..rewrite()
        },
        fonts: FontPolicy::Keep,
        images: ImagePolicy::Keep,
    };
    let merge = || {
        let mut editor = parts[0].editor();
        for part in &parts[1..] {
            for page in 0..part.page_count() {
                let at = editor.page_refs().len() as u32;
                editor
                    .import_page(part.cos(), page, at)
                    .expect("the page imports");
            }
        }
        write::save(&mut editor, &merged).bytes
    };
    assert_eq!(
        Document::open(merge())
            .expect("the merge reopens")
            .page_count(),
        400
    );
    group.throughput(Per::Pages(400).throughput());
    group.bench_function("merge 4 x 100 pages", |b| {
        b.iter(|| black_box(merge()));
    });

    // Tier P's pair, through the crypto crate's own CBC: the image file's
    // bytes encrypted once, then each direction timed over them.
    let plain = inputs::image_file();
    let key = [0x42u8; 32];
    let iv = [0x24u8; 16];
    let sealed = tinker_pdf_crypto::aes::cbc_encrypt_with_iv_prefix(&key, &iv, &plain)
        .expect("a 256-bit key encrypts");
    let (opened, notes) = tinker_pdf_crypto::aes::cbc_decrypt_with_iv_prefix(&key, &sealed);
    assert!(notes.is_empty(), "a clean decrypt: {notes:?}");
    assert_eq!(
        &opened[..],
        &plain[..],
        "the decrypted bytes are the plain ones"
    );
    group.throughput(Per::Bytes(plain.len() as u64).throughput());
    group.bench_function("AES-256-CBC decrypt 23 MB", |b| {
        b.iter(|| {
            black_box(tinker_pdf_crypto::aes::cbc_decrypt_with_iv_prefix(
                &key, &sealed,
            ))
        });
    });
    group.bench_function("AES-256-CBC encrypt 23 MB", |b| {
        b.iter(|| {
            black_box(tinker_pdf_crypto::aes::cbc_encrypt_with_iv_prefix(
                &key, &iv, &plain,
            ))
        });
    });

    // One annotation's appearance replaced, the change appended as an
    // incremental update, and the bytes reopened: thirteen times an
    // iteration over a thousand pages, once over ten.
    let large = Document::open(inputs::annotated(&inputs::small(1000))).expect("it opens");
    let short = Document::open(inputs::annotated(&inputs::small(10))).expect("it opens");
    let (large_form, short_form) = (
        inputs::first_appearance(&large),
        inputs::first_appearance(&short),
    );
    let edit = |doc: &Document, form| {
        let mut editor = doc.editor();
        editor.put_stream(form, inputs::square_appearance(doc, 0.2));
        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Incremental,
            ..WriteOptions::default()
        });
        Document::open(bytes).expect("the update reopens")
    };
    assert_eq!(edit(&large, large_form).page_count(), 1000);
    assert_eq!(edit(&short, short_form).page_count(), 10);
    group.throughput(Per::Pages(13 * 1000).throughput());
    group.bench_function("edit an annotation, save, reopen, 1000 pages", |b| {
        b.iter(|| {
            for _ in 0..13 {
                black_box(edit(&large, large_form));
            }
        });
    });
    group.throughput(Per::Pages(10).throughput());
    group.bench_function("edit an annotation, save, reopen, 10 pages", |b| {
        b.iter(|| black_box(edit(&short, short_form)));
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = write
}
criterion_main!(benches);
