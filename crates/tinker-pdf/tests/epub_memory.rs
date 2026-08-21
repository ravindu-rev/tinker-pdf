//! What a book with a large picture costs to read (gap 31, milestone 13).
//!
//! # Why this file exists rather than an arithmetic claim
//!
//! Gap 29 closed with its peak-memory figure **unmeasured** — its milestone 6
//! recorded that the 3.6 GB the design turns on is arithmetic, that no 200-page
//! archive was ever opened, and that the claim is therefore that the design
//! *cannot* hold a raster per page rather than that anybody watched it not. Gap
//! 30's milestone 9 answered that for a fixed document and recorded the
//! fixture's own limitation in the same breath: it measured 0.2 MB against a
//! 17.2 MB decoded raster, and it had to **build** the package to do it, because
//! every image Windows put in that corpus is 32 x 32.
//!
//! Gap 31 is the first of the three whose corpus can answer on its own. The
//! fetched twenty include books of a megabyte, and one of them —
//! `sample-linear-algebra.epub` — is 94 content documents that generate 993 349
//! boxes, which is a different cost from a raster and the one this format
//! actually has. So there are two measurements here rather than one:
//!
//! - **the raster claim**, which is gap 29's pass-through inherited whole and is
//!   asserted on every build against a hand-built book, because no committed
//!   book carries a picture big enough to tell;
//! - **the box-tree claim**, which is this format's own and is what the
//!   `--ignored` test below writes out for the plan's figure.
//!
//! # What is asserted
//!
//! The cheap half runs on every build: the synthesised document is **smaller
//! than the decoded raster would be**, which is a statement about the bytes
//! rather than about the allocator, and it is what the pass-through actually
//! promises.

mod epub_support;

use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::{Document, OpenOptions, WriteMode, WriteOptions};

/// A page whose whole content is one large picture.
const WIDE: u32 = 2000;
const TALL: u32 = 3000;

/// A gradient with structure in it, which is what a photograph looks like to a
/// compressor.
///
/// **Not noise**, for `xps_memory.rs`'s reason and it is worth repeating here:
/// incompressible data is the one case where the pass-through and the decode
/// cost the same, because the IDAT *is* the raster. That is a true property of
/// the claim rather than a defect in it.
fn plate() -> Vec<u8> {
    let mut pixels = Vec::with_capacity((WIDE * TALL * 3) as usize);
    for y in 0..TALL {
        for x in 0..WIDE {
            pixels.push((x * 255 / WIDE) as u8);
            pixels.push((y * 255 / TALL) as u8);
            pixels.push(((x + y) * 255 / (WIDE + TALL)) as u8);
        }
    }
    png(WIDE, TALL, &pixels)
}

/// An RGB PNG, written from the specification.
///
/// `cbz_support`'s own `rgb_png` is the same function and is not reused,
/// because that module is the comic path's fixture kit and importing it here
/// would make a book's fixtures depend on a comic's.
fn png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(pixels.len() + height as usize);
    for row in pixels.chunks_exact(width as usize * 3) {
        raw.push(0); // filter type 0, `None`
        raw.extend_from_slice(row);
    }
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |tag: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(tag);
        out.extend_from_slice(data);
        let mut crc = Vec::with_capacity(4 + data.len());
        crc.extend_from_slice(tag);
        crc.extend_from_slice(data);
        out.extend_from_slice(&tinker_pdf_filters::crc32(&crc).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &tinker_pdf_filters::zlib_compress(&raw));
    chunk(b"IEND", &[]);
    out
}

/// A one-chapter book whose only content is a `WIDE` x `TALL` picture.
fn book_of_one_plate() -> Vec<u8> {
    let chapter = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<!DOCTYPE html>"#,
        r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Plate</title></head>"#,
        r#"<body><p><img src="plate.png" alt="a plate"/></p></body></html>"#
    );
    let package = concat!(
        r#"<?xml version="1.0" encoding="utf-8"?>"#,
        r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">"#,
        r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
        r#"<dc:identifier id="id">urn:uuid:0000</dc:identifier>"#,
        r#"<dc:title>Plate</dc:title><dc:language>en</dc:language></metadata>"#,
        r#"<manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>"#,
        r#"<item id="p" href="plate.png" media-type="image/png"/></manifest>"#,
        r#"<spine><itemref idref="c1"/></spine></package>"#
    );
    let container = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
        r#"<rootfiles><rootfile full-path="content.opf" "#,
        r#"media-type="application/oebps-package+xml"/></rootfiles></container>"#
    );
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", container.as_bytes()),
        OcfEntry::deflated("content.opf", package.as_bytes()),
        OcfEntry::deflated("c1.xhtml", chapter.as_bytes()),
        OcfEntry::deflated("plate.png", &plate()),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// The synthesised book is smaller than the decoded raster would be.
///
/// *w × h × 3* for this plate is 18 MB. A build that decoded the PNG and
/// embedded samples would produce a document at least that large whatever else
/// it did, so a document under it is one that did not decode — which is gap
/// 29's pass-through, stated in bytes rather than in allocator behaviour, and
/// inherited by this format without a line of new code.
#[test]
fn a_book_of_one_large_plate_does_not_cost_its_decoded_size() {
    let bytes = book_of_one_plate();
    let document = Document::open(bytes.clone()).expect("a book");
    let saved = document.editor().save(&WriteOptions {
        mode: WriteMode::Rewrite,
        ..WriteOptions::default()
    });

    let decoded = (WIDE as usize) * (TALL as usize) * 3;
    assert!(
        saved.len() < decoded,
        "the document is {} bytes and the decoded raster alone would be {decoded}",
        saved.len()
    );
    assert!(
        saved.len() < bytes.len() * 3,
        "the document is {} bytes over a {}-byte book",
        saved.len(),
        bytes.len()
    );
    // The margin is worth an eye rather than only a bound: a page of six
    // million samples that costs well under a tenth of them is one whose
    // picture was never expanded.
    assert!(
        saved.len() * 10 < decoded,
        "the document is {} bytes against a {decoded}-byte raster, which is not \
         the order of magnitude a pass-through gives",
        saved.len()
    );
}

/// Opens the largest book in each corpus, for the plan's peak-memory figure.
///
/// `--ignored` because it is a measurement rather than an assertion: what it
/// costs is read off the process by whatever is watching it, and a test that
/// asserted a number of bytes would be asserting something about this machine's
/// allocator. What it prints is the two figures a reader needs beside the
/// external one — the book's size on disk and what it synthesised to — so the
/// peak can be compared against something.
///
/// **Both corpora**, and the second is skipped by name rather than silently:
/// `TINKER_EPUB_CORPUS` names the directory `fetch-corpus.sh` fills, and
/// without it the only figure available is the committed one, which is a
/// hundred times smaller.
#[test]
#[ignore = "a measurement, not an assertion: run it under something that watches the process"]
fn open_the_largest_book_in_each_corpus_for_measurement() {
    let committed = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("epub");
    measure("committed", &committed);
    match std::env::var("TINKER_EPUB_CORPUS") {
        Ok(dir) => measure("fetched", std::path::Path::new(&dir)),
        Err(_) => println!("epub-memory: SKIPPED the fetched corpus (TINKER_EPUB_CORPUS unset)"),
    }
}

/// Opens every `.epub` in a directory and reports the largest.
fn measure(label: &str, dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        println!("epub-memory: {label} has no directory at {}", dir.display());
        return;
    };
    let mut books: Vec<(String, Vec<u8>)> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("epub"))
        .map(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {name}: {e}"));
            (name, bytes)
        })
        .collect();
    books.sort_by_key(|(_, bytes)| bytes.len());
    let Some((name, bytes)) = books.pop() else {
        println!("epub-memory: {label} holds no books");
        return;
    };
    let document = Document::open_with(bytes.clone(), &OpenOptions::default())
        .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    let report = document.archive().expect("a book");
    let cost = report.book_cost().expect("a book publishes its cost");
    println!(
        "epub-memory: {label} largest {name} {} bytes -> {} pages, {} boxes, {} synthesised bytes",
        bytes.len(),
        cost.pages,
        cost.boxes,
        report.synthesised_bytes(),
    );
}
