//! The write group: documents saved.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `write` list names these operations
//! in this order.
//!
//! An operation shorter than the set's 20-300 ms window repeats its call
//! inside one iteration, as many times as land it near the window's middle
//! on the hosted runner (`docs/design/performance.md`), and its unit counts
//! every page each call wrote; one longer takes a smaller input, named in
//! the operation.

mod support;

use std::cell::LazyCell;
use std::hint::black_box;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use support::cli::Cli;
use support::{colour, inputs, Group, Per};
#[cfg(not(tinker_planning_baseline))]
use tinker_pdf::write::AppendOptions;
use tinker_pdf::write::{self, FontPolicy, ImagePolicy, SaveOptions};
use tinker_pdf::{Document, WriteMode, WriteOptions};
use tinker_pdf_cos::ObjRef;

/// The tests' embedding faces, for the provider face the embedding pass
/// embeds (`docs/design/font-embedding.md`).
#[cfg(not(tinker_planning_baseline))]
#[path = "../tests/embed_support/mod.rs"]
mod embed_support;

/// A rewrite, as `tpdf` writes one.
fn rewrite() -> WriteOptions {
    WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    }
}

/// `small(pages)`'s bytes, the file a CLI operation reads beside them.
fn small_bytes(pages: usize) -> Vec<u8> {
    inputs::store().bytes(&format!("small-{pages}.pdf"), || inputs::small(pages))
}

/// `pages` small pages, opened, their count asserted.
fn small(pages: usize) -> Document {
    let doc = Document::open(small_bytes(pages)).expect("the small pages open");
    assert_eq!(doc.page_count() as usize, pages);
    doc
}

/// `dense(pages, seed)`'s bytes, the file a CLI operation reads beside them.
fn dense_bytes(pages: usize, seed: u64) -> Vec<u8> {
    let name = match seed {
        0 => format!("dense-{pages}.pdf"),
        seed => format!("dense-{pages}-{seed}.pdf"),
    };
    inputs::store().bytes(&name, || inputs::dense(pages, seed))
}

/// What an edit hands back: the update appended and the document reopened
/// over it (`write::Appended`).
#[cfg(not(tinker_planning_baseline))]
type Edited = write::Appended;

/// The planning baseline has no edit-and-reopen door: its arm's edit is the
/// document opened again from the incremental save.
#[cfg(tinker_planning_baseline)]
type Edited = Document;

/// `doc` with its first annotation's appearance, `form`, replaced, appended
/// through the facade's edit-and-reopen door (`write::append`): the update
/// section written from the document's length and last byte, and the
/// document reopened over it reading the section alone.
#[cfg(not(tinker_planning_baseline))]
fn edit(doc: &Document, form: ObjRef) -> Edited {
    let mut editor = doc.editor();
    editor.put_stream(form, inputs::square_appearance(doc, 0.2));
    write::append(doc, &editor, &AppendOptions::default()).expect("the update reopens")
}

/// The planning baseline's edit: saved as an incremental update and the
/// bytes opened, which reads the file whole.
#[cfg(tinker_planning_baseline)]
fn edit(doc: &Document, form: ObjRef) -> Edited {
    let mut editor = doc.editor();
    editor.put_stream(form, inputs::square_appearance(doc, 0.2));
    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });
    Document::open(bytes).expect("the update reopens")
}

/// What one edit reads, checked untimed: the update alone, and a document
/// with the pages of the one it was made over.
#[cfg(not(tinker_planning_baseline))]
fn reads_the_update(edited: &Edited, pages: u32) {
    assert_eq!(edited.full_open, None, "only the update is read");
    assert_eq!(edited.document.page_count(), pages);
}

/// The planning baseline reads the file whole: what its edit is checked for
/// is the pages.
#[cfg(tinker_planning_baseline)]
fn reads_the_update(edited: &Edited, pages: u32) {
    assert_eq!(edited.page_count(), pages);
}

/// A rewrite that converts each Type 1 program it cuts to CFF
/// (`docs/design/subsetting.md`, *When a save converts*).
#[cfg(not(tinker_planning_baseline))]
fn converting() -> SaveOptions {
    SaveOptions {
        write: rewrite(),
        type1: write::Type1Programs::Convert,
        ..SaveOptions::default()
    }
}

/// The planning baseline converts nothing: its arm saves through the default
/// door in the conversion's place, the operation the gate pairs this one
/// with (`baseline.json`'s `paired_with`).
#[cfg(tinker_planning_baseline)]
fn converting() -> SaveOptions {
    SaveOptions {
        write: rewrite(),
        ..SaveOptions::default()
    }
}

/// The pages the embedding pass embeds fonts for, opened with the host
/// provider whose face draws them, and the rewrite that asks the pass to
/// embed that face: the face the tests' embedding pass is held with,
/// synthesised byte by byte (`tests/embed_support`).
#[cfg(not(tinker_planning_baseline))]
fn embedding() -> (Document, SaveOptions) {
    let provider = embed_support::provider(embed_support::sans().truetype());
    let bytes = inputs::store().bytes("embed-4.pdf", || inputs::embed_pages(4, 0));
    let doc = Document::open(bytes)
        .expect("the pages open")
        .with_fonts(Arc::clone(&provider));
    assert_eq!(doc.page_count(), 4);
    let options = SaveOptions {
        write: rewrite(),
        embed: write::FontEmbedding::Embed(provider),
        ..SaveOptions::default()
    };
    (doc, options)
}

/// The planning baseline embeds nothing: its arm cuts the dense pages
/// through the default door in the pass's place, subset-on-save of 4 pages,
/// the operation the gate pairs this one with.
#[cfg(tinker_planning_baseline)]
fn embedding() -> (Document, SaveOptions) {
    let doc = Document::open(dense_bytes(4, 0)).expect("the dense pages open");
    let options = SaveOptions {
        write: rewrite(),
        ..SaveOptions::default()
    };
    (doc, options)
}

/// The four Type 1 pages subset-on-save cuts and converts.
fn type1_document() -> (Vec<u8>, Document) {
    let bytes = inputs::store().bytes("type1-4.pdf", || inputs::type1_pages(4, 0));
    let doc = Document::open(bytes.clone()).expect("the Type 1 pages open");
    assert_eq!(doc.page_count(), 4);
    (bytes, doc)
}

/// `doc` saved under `options`, reopened, and its page count held to the
/// original's.
fn saved_reopens(doc: &Document, options: &WriteOptions) {
    let bytes = doc.editor().save(options);
    let again = Document::open(bytes).expect("the saved file reopens");
    assert_eq!(again.page_count(), doc.page_count());
}

fn write(c: &mut Criterion) {
    let small_100 = LazyCell::new(|| {
        let doc = small(100);
        saved_reopens(&doc, &rewrite());
        doc
    });
    let small_1000 = LazyCell::new(|| {
        let doc = small(1000);
        saved_reopens(&doc, &rewrite());
        doc
    });
    let image: LazyCell<Arc<[u8]>> = LazyCell::new(|| {
        inputs::store()
            .bytes("small-image.pdf", || inputs::small_image_file().to_vec())
            .into()
    });

    let mut group = Group::new(c, "write");

    // A rewrite reads every object the file has (`edit/save.rs`'s walk of
    // the cross-reference table), so its first iteration leaves the document
    // as warm as any earlier read of it would have.
    group.operation("rewrite a document", |op| {
        let doc = Document::open(inputs::store().bytes("three-pages.pdf", inputs::three_pages))
            .expect("it opens");
        op.per(Per::Pages(u64::from(doc.page_count())));
        op.iter(move || {
            black_box(doc.editor().save(&WriteOptions {
                mode: WriteMode::Rewrite,
                ..WriteOptions::default()
            }))
        });
    });

    group.operation("rewrite 100 pages", |op| {
        op.per(Per::Pages(200 * 100));
        op.iter(|| {
            for _ in 0..200 {
                black_box(small_100.editor().save(&rewrite()));
            }
        });
    });
    group.operation("rewrite 1000 pages", |op| {
        op.per(Per::Pages(20 * 1000));
        op.iter(|| {
            for _ in 0..20 {
                black_box(small_1000.editor().save(&rewrite()));
            }
        });
    });
    group.operation("rewrite 3000 pages", |op| {
        let small_3000 = small(3000);
        saved_reopens(&small_3000, &rewrite());
        op.per(Per::Pages(7 * 3000));
        op.iter(move || {
            for _ in 0..7 {
                black_box(small_3000.editor().save(&rewrite()));
            }
        });
    });

    group.operation("rewrite 100 pages into object streams", |op| {
        let small_100: &Document = &small_100;
        let streams = WriteOptions {
            object_streams: true,
            ..rewrite()
        };
        saved_reopens(small_100, &streams);
        op.per(Per::Pages(200 * 100));
        op.iter(move || {
            for _ in 0..200 {
                black_box(small_100.editor().save(&streams));
            }
        });
    });
    group.operation("rewrite 100 pages linearized", |op| {
        let small_100: &Document = &small_100;
        let linear = WriteOptions {
            linearize: true,
            ..rewrite()
        };
        saved_reopens(small_100, &linear);
        op.per(Per::Pages(75 * 100));
        op.iter(move || {
            for _ in 0..75 {
                black_box(small_100.editor().save(&linear));
            }
        });
    });

    // The facade's door with its default policies: a rewrite, every
    // embedded program cut to what the pages draw.
    group.operation("subset-on-save of 4 pages", |op| {
        let bytes = dense_bytes(4, 0);
        let dense = Document::open(bytes.clone()).expect("the dense pages open");
        let subset = SaveOptions {
            write: rewrite(),
            ..SaveOptions::default()
        };
        let saved = write::save(&mut dense.editor(), &subset).bytes;
        assert!(
            saved.len() < bytes.len(),
            "the subset file is smaller than the whole face's"
        );
        assert_eq!(Document::open(saved).expect("it reopens").page_count(), 4);
        op.per(Per::Pages(4));
        op.iter(move || black_box(write::save(&mut dense.editor(), &subset)));
    });

    // The same door over a Type 1 program (FT-11.11): the walk, then
    // `subset_type1` cutting it to the glyphs drawn.
    group.operation("subset-on-save of 4 pages over a Type 1 font", |op| {
        let (bytes, doc) = type1_document();
        let subset = SaveOptions {
            write: rewrite(),
            ..SaveOptions::default()
        };
        #[cfg(not(tinker_planning_baseline))]
        {
            let saved = write::save(&mut doc.editor(), &subset);
            let report = saved.fonts.report().expect("the font pass ran");
            assert_eq!(report.subsetted.len(), 1, "the program is cut: {report:?}");
            assert!(!report.subsetted[0].converted);
            assert!(saved.bytes.len() < bytes.len(), "the cut file is smaller");
        }
        op.per(Per::Pages(4));
        op.iter(move || black_box(write::save(&mut doc.editor(), &subset)));
    });
    // And the same save asked to convert it to CFF first (FT-11.8):
    // `type1_to_cff`, its every glyph outlined in both programs, every shown
    // code read again, and the CFF subsetter's cut.
    group.operation(
        "subset-on-save of 4 pages over a Type 1 font converted to CFF",
        |op| {
            let (bytes, doc) = type1_document();
            let options = converting();
            #[cfg(not(tinker_planning_baseline))]
            {
                let saved = write::save(&mut doc.editor(), &options);
                let report = saved.fonts.report().expect("the font pass ran");
                assert!(report.unconverted.is_empty(), "{report:?}");
                assert_eq!(report.subsetted.len(), 1, "{report:?}");
                assert!(report.subsetted[0].converted, "the program is a CFF now");
                assert!(saved.bytes.len() < bytes.len(), "the cut file is smaller");
            }
            op.per(Per::Pages(4));
            op.iter(move || black_box(write::save(&mut doc.editor(), &options)));
        },
    );

    // The embedding pass on the same door (FT-12.7): the fonts the pages
    // draw and no program embeds, the provider's face embedded for each and
    // cut to what it drew, timed with the font pass after it.
    group.operation(
        "embed-on-save of 4 pages over a standard-14 and a TrueType font",
        |op| {
            let (doc, options) = embedding();
            #[cfg(not(tinker_planning_baseline))]
            {
                let saved = write::save(&mut doc.editor(), &options);
                let report = saved.embedded.expect("the pass was asked for");
                assert_eq!(report.embedded.len(), 2, "both fonts embedded: {report:?}");
                assert!(report.left.is_empty(), "nothing left: {report:?}");
                assert_eq!(
                    Document::open(saved.bytes)
                        .expect("it reopens")
                        .page_count(),
                    4
                );
            }
            op.per(Per::Pages(4));
            op.iter(move || black_box(write::save(&mut doc.editor(), &options)));
        },
    );

    // `tpdf merge`'s shape: the first file's editor, every page of the
    // others imported after its own, saved with stream deduplication and
    // the programs kept as they are.
    group.operation("merge 4 x 50 pages", |op| {
        let parts: Vec<Document> = (0..4)
            .map(|seed| Document::open(dense_bytes(50, seed)).expect("a part opens"))
            .collect();
        let merged = SaveOptions {
            write: WriteOptions {
                deduplicate_streams: true,
                ..rewrite()
            },
            fonts: FontPolicy::Keep,
            images: ImagePolicy::Keep,
            ..SaveOptions::default()
        };
        let merge = move || {
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
            200
        );
        op.per(Per::Pages(200));
        op.iter(move || black_box(merge()));
    });

    // Tier P's pair, through the crypto crate's own CBC: the small image
    // file's bytes encrypted once, then each direction timed over them.
    let key = [0x42u8; 32];
    let iv = [0x24u8; 16];
    let sealed = LazyCell::new(|| {
        let sealed = inputs::store().bytes("small-image.aes-cbc", || {
            tinker_pdf_crypto::aes::cbc_encrypt_with_iv_prefix(&key, &iv, &image)
                .expect("a 256-bit key encrypts")
        });
        let (opened, notes) = tinker_pdf_crypto::aes::cbc_decrypt_with_iv_prefix(&key, &sealed);
        assert!(notes.is_empty(), "a clean decrypt: {notes:?}");
        assert_eq!(
            &opened[..],
            &image[..],
            "the decrypted bytes are the plain ones"
        );
        sealed
    });
    group.operation("AES-256-CBC decrypt 2.5 MB", |op| {
        op.per(Per::Bytes(image.len() as u64));
        op.iter(|| {
            black_box(tinker_pdf_crypto::aes::cbc_decrypt_with_iv_prefix(
                &key, &sealed,
            ))
        });
    });
    group.operation("AES-256-CBC encrypt 2.5 MB", |op| {
        // The decrypt's check, that the key and the IV give the bytes back.
        LazyCell::force(&sealed);
        op.per(Per::Bytes(image.len() as u64));
        op.iter(|| {
            black_box(tinker_pdf_crypto::aes::cbc_encrypt_with_iv_prefix(
                &key, &iv, &image,
            ))
        });
    });

    // One annotation's appearance replaced and appended through the facade's
    // edit-and-reopen door (`edit`) — 170 times an iteration over a thousand
    // pages, 2 700 times over ten. Nothing it does reads the rest of the file,
    // which is what holds the thousand pages to twice the ten.
    group.operation("edit an annotation, save, reopen, 1000 pages", |op| {
        let large = Document::open(inputs::store().bytes("small-1000-annotated.pdf", || {
            inputs::annotated(&inputs::small(1000))
        }))
        .expect("it opens");
        let large_form = inputs::first_appearance(&large);
        reads_the_update(&edit(&large, large_form), 1000);
        op.per(Per::Pages(170 * 1000));
        op.iter(move || {
            for _ in 0..170 {
                drop(black_box(edit(&large, large_form)));
            }
        });
    });
    group.operation("edit an annotation, save, reopen, 10 pages", |op| {
        let short = Document::open(inputs::store().bytes("small-10-annotated.pdf", || {
            inputs::annotated(&inputs::small(10))
        }))
        .expect("it opens");
        let short_form = inputs::first_appearance(&short);
        reads_the_update(&edit(&short, short_form), 10);
        op.per(Per::Pages(2700 * 10));
        op.iter(move || {
            for _ in 0..2700 {
                drop(black_box(edit(&short, short_form)));
            }
        });
    });

    // An editor that changes nothing, saved as an incremental update: the
    // original bytes and what an update with nothing in it appends, 2 000
    // times an iteration.
    group.operation("no-op incremental save of 100 pages", |op| {
        let original = small_bytes(100);
        let doc = Document::open(original.clone()).expect("the small pages open");
        let incremental = WriteOptions {
            mode: WriteMode::Incremental,
            ..WriteOptions::default()
        };
        let saved = doc.editor().save(&incremental);
        assert!(
            saved.starts_with(&original),
            "an incremental save keeps the original bytes as its prefix"
        );
        assert_eq!(
            Document::open(saved)
                .expect("the update reopens")
                .page_count(),
            100,
            "the update reopens with the original's pages"
        );
        op.per(Per::Pages(2000 * 100));
        op.iter(move || {
            for _ in 0..2000 {
                black_box(doc.editor().save(&incremental));
            }
        });
    });

    // A page of colour glyphs (FT-06.17a), ten an iteration: 25 lines of 21
    // capitals of `inputs::colour_face()`, each a `COLR` version 1 graph of
    // three layers — a disc under a radial gradient, its eyes in a solid, its
    // mouth under a linear gradient — written as Type 3 glyphs through the
    // colour run writer (`support::colour`), each glyph's procedure once a
    // document. The planning baseline, which has no colour glyphs, writes the
    // same lines in the face's outlines in its place.
    group.operation("write a page of colour glyphs", |op| {
        let program = inputs::store().bytes("colour-face.ttf", inputs::colour_face);
        let lines = inputs::colour_lines();
        colour::holds_its_lines(&colour::colour_page(&program, &lines), &lines);
        op.per(Per::Pages(10));
        op.iter(move || {
            for _ in 0..10 {
                black_box(colour::colour_page(&program, &lines));
            }
        });
    });

    // ---- through the CLI -----------------------------------------------
    //
    // `tpdf` has no rewrite command: `merge` given one input writes it again
    // under a rewrite with stream deduplication and nothing else. Every
    // writing command subsets by default, so each names its policy: `keep`
    // but for subset-on-save, so that each figure times one pass.

    group.operation("rewrite 100 pages, CLI", |op| {
        let small_100_pdf = inputs::store().file("small-100.pdf", || inputs::small(100));
        let rewrite_100 = Cli::new("write-rewrite-100");
        let out_100 = rewrite_100.out("out.pdf");
        let written = out_100.clone();
        op.per(Per::Pages(26 * 100));
        op.iter(move || {
            for _ in 0..26 {
                rewrite_100.run(&[
                    &"merge",
                    &small_100_pdf,
                    &"--out",
                    &out_100,
                    &"--font-policy",
                    &"keep",
                ]);
            }
        });
        op.after(move || assert_eq!(reopened(&written), 100, "the rewrite's pages"));
    });
    group.operation("rewrite 1000 pages, CLI", |op| {
        let small_1000_pdf = inputs::store().file("small-1000.pdf", || inputs::small(1000));
        let rewrite_1000 = Cli::new("write-rewrite-1000");
        let out_1000 = rewrite_1000.out("out.pdf");
        let written = out_1000.clone();
        op.per(Per::Pages(6 * 1000));
        op.iter(move || {
            for _ in 0..6 {
                rewrite_1000.run(&[
                    &"merge",
                    &small_1000_pdf,
                    &"--out",
                    &out_1000,
                    &"--font-policy",
                    &"keep",
                ]);
            }
        });
        op.after(move || assert_eq!(reopened(&written), 1000, "the rewrite's pages"));
    });
    group.operation("subset-on-save of 4 pages, CLI", |op| {
        let dense_pdf = inputs::store().file("dense-4.pdf", || inputs::dense(4, 0));
        let subset_cli = Cli::new("write-subset-4");
        let out_subset = subset_cli.out("out.pdf");
        let (read, written) = (dense_pdf.clone(), out_subset.clone());
        op.per(Per::Pages(4));
        op.iter(move || {
            subset_cli.run(&[
                &"merge",
                &dense_pdf,
                &"--out",
                &out_subset,
                &"--font-policy",
                &"subset",
            ])
        });
        op.after(move || {
            assert_eq!(reopened(&written), 4, "the subset file's pages");
            assert!(
                std::fs::metadata(&written).expect("the subset file").len()
                    < std::fs::metadata(&read).expect("the dense file").len(),
                "the subset file is smaller than the whole face's"
            );
        });
    });
    group.operation("merge 4 x 50 pages, CLI", |op| {
        let parts_pdf: Vec<_> = (0..4)
            .map(|seed| {
                let name = match seed {
                    0 => "dense-50.pdf".to_string(),
                    seed => format!("dense-50-{seed}.pdf"),
                };
                inputs::store().file(&name, || inputs::dense(50, seed))
            })
            .collect();
        let merge_cli = Cli::new("write-merge-4x50");
        let out_merge = merge_cli.out("out.pdf");
        let written = out_merge.clone();
        op.per(Per::Pages(200));
        op.iter(move || {
            merge_cli.run(&[
                &"merge",
                &parts_pdf[0],
                &parts_pdf[1],
                &parts_pdf[2],
                &parts_pdf[3],
                &"--out",
                &out_merge,
                &"--font-policy",
                &"keep",
            ])
        });
        op.after(move || assert_eq!(reopened(&written), 200, "the merge's pages"));
    });
    group.operation("encrypt the 2.5 MB image file, CLI", |op| {
        let image: &Arc<[u8]> = &image;
        let image_pdf = inputs::store().file("small-image.pdf", || image.to_vec());
        // A figure must not depend on `/dev/urandom`.
        let entropy = inputs::store().file("entropy", || vec![0x5A; 48]);
        let encrypt = Cli::new("write-encrypt-image");
        let out_encrypted = encrypt.out("out.pdf");
        let written = out_encrypted.clone();
        op.per(Per::Bytes(image.len() as u64));
        op.iter(move || {
            encrypt.run(&[
                &"encrypt",
                &image_pdf,
                &"--out",
                &out_encrypted,
                &"--entropy",
                &entropy,
                &"--font-policy",
                &"keep",
            ])
        });
        op.after(move || {
            let sealed = Document::open(std::fs::read(&written).expect("the encrypted file"))
                .expect("the encrypted file opens");
            sealed
                .authenticate("")
                .expect("the encrypted file authenticates with the empty password");
            assert_eq!(
                sealed.page_count(),
                Document::open(image.clone())
                    .expect("it opens")
                    .page_count(),
                "the encrypted file's pages"
            );
        });
    });

    group.finish();
}

/// The page count of the file a command wrote at `path`, reopened.
fn reopened(path: &std::path::Path) -> u32 {
    Document::open(std::fs::read(path).expect("the written file"))
        .expect("the written file reopens")
        .page_count()
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = write
}
criterion_main!(benches);
