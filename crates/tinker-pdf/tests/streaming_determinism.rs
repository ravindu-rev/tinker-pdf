//! Ruling 4 over a byte source: **arrival is not an input.**
//!
//! `determinism.rs` pins what this engine renders. This file pins that the
//! same bytes render to the same pixels however they arrived — as one buffer,
//! as aligned chunks fetched on demand, or one byte at a time out of the most
//! hostile source that still conforms to [`ByteSource`].
//!
//! It is a separate file rather than a case inside `determinism.rs` on
//! purpose. The committed fingerprints there are a claim about *this engine's
//! output*; this is a claim about *equality between two paths*, and an
//! equality that held because both sides changed together would be worth
//! nothing. So nothing here is a committed hash: every assertion compares the
//! streamed render against the buffered one, and each fixture is held to a
//! floor of ink first, because two blank pages are equal too.

use std::sync::Arc;

use tinker_pdf::{
    Bitmap, CountingSource, Document, DocumentBuilder, RenderOptions, ShreddedSource, SliceSource,
};

/// Pixels that are not the white the page started as.
fn ink(bitmap: &Bitmap) -> usize {
    bitmap
        .data
        .chunks_exact(bitmap.components())
        .filter(|pixel| pixel.iter().any(|value| *value != 255))
        .count()
}

/// Curves and diagonals, which is where coverage arithmetic lives. A page of
/// axis-aligned rectangles would be equal across any two paths whatever the
/// rasteriser did, because every span is full or empty.
fn curves_page() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(200.0, 100.0, |page| {
        page.raw(
            b"0.10 0.20 0.80 rg\n\
              20 20 m 180 40 l 100 90 l h f\n\
              0.90 0.30 0.10 rg\n\
              30 30 m 60 80 90 20 170 70 c 100 10 l h f\n\
              0.20 0.60 0.30 rg\n\
              10.5 12.25 m 190.75 18.5 l 150.5 88.125 l h f\n",
        );
    });
    builder.finish()
}

/// Two pages, so the second one's objects live past whatever the first one
/// needed and a path that only ever read the head would fail here.
fn two_page_document() -> Vec<u8> {
    let mut builder = DocumentBuilder::new();
    builder.add_page(200.0, 100.0, |page| {
        page.raw(b"0.15 0.35 0.75 rg\n20 20 m 180 40 l 100 90 l h f\n");
    });
    builder.add_page(200.0, 100.0, |page| {
        page.raw(b"0.75 0.15 0.35 rg\n25 25 m 60 85 120 15 175 75 c 110 12 l h f\n");
    });
    builder.finish()
}

/// One fixture: how to build it, which page is rendered, and the fewest
/// non-background pixels it may paint.
struct Fixture {
    name: &'static str,
    build: fn() -> Vec<u8>,
    page: u32,
    least_ink: usize,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "curves",
        build: curves_page,
        page: 0,
        least_ink: 3000,
    },
    Fixture {
        name: "two pages, first",
        build: two_page_document,
        page: 0,
        least_ink: 2000,
    },
    Fixture {
        name: "two pages, second",
        build: two_page_document,
        page: 1,
        least_ink: 1000,
    },
];

/// The rendered page, and everything the render had to say about it.
fn render(document: &Document, at: u32) -> Bitmap {
    document
        .page(at)
        .expect("the fixture has this page")
        .render(&RenderOptions::default())
}

/// A bitmap as the bytes two renders are compared on: dimensions as well as
/// pixels, because two renders differing only in size would otherwise have to
/// differ in content to be caught.
fn shape_and_pixels(bitmap: &Bitmap) -> (u32, u32, Vec<u8>) {
    (bitmap.width, bitmap.height, bitmap.data.clone())
}

#[test]
fn every_fixture_renders_identically_from_a_buffer_and_from_a_source() {
    for fixture in FIXTURES {
        let bytes = (fixture.build)();

        let buffered = Document::open(bytes.clone()).expect("it opens");
        let from_buffer = render(&buffered, fixture.page);
        let drawn = ink(&from_buffer);
        assert!(
            drawn >= fixture.least_ink,
            "the {} fixture painted {drawn} pixels, fewer than the {} it is supposed to: \
             two blank pages are equal too",
            fixture.name,
            fixture.least_ink
        );

        let sliced = Document::open_streaming(Arc::new(SliceSource::new(bytes.clone())))
            .expect("it opens over a slice source");
        assert_eq!(
            shape_and_pixels(&render(&sliced, fixture.page)),
            shape_and_pixels(&from_buffer),
            "the {} fixture over a slice source",
            fixture.name
        );

        let shredded = Document::open_streaming(Arc::new(ShreddedSource::new(SliceSource::new(
            bytes.clone(),
        ))))
        .expect("it opens one byte at a time");
        let from_shreds = render(&shredded, fixture.page);
        assert_eq!(
            shape_and_pixels(&from_shreds),
            shape_and_pixels(&from_buffer),
            "the {} fixture over a source that answers one byte at a time",
            fixture.name
        );
        assert_eq!(
            from_shreds.warnings, from_buffer.warnings,
            "and it had the same to say about it ({})",
            fixture.name
        );
    }
}

/// Warnings and the ladder level are values too, and a streamed document must
/// not acquire either of its own.
#[test]
fn a_streamed_document_reports_what_a_buffered_one_reports() {
    for fixture in FIXTURES {
        let bytes = (fixture.build)();
        let buffered = Document::open(bytes.clone()).expect("it opens");
        let streamed = Document::open_streaming(Arc::new(ShreddedSource::new(SliceSource::new(
            bytes.clone(),
        ))))
        .expect("it opens");

        // Read the same things in the same order, so the lazily-arriving
        // warnings both documents accumulate are comparable.
        let _ = render(&buffered, fixture.page);
        let _ = render(&streamed, fixture.page);

        assert_eq!(buffered.ladder_level(), streamed.ladder_level());
        assert_eq!(buffered.warnings(), streamed.warnings());
        assert_eq!(buffered.page_count(), streamed.page_count());
        assert!(streamed.is_streamed());
        assert!(!buffered.is_streamed());
    }
}

/// Chunking is policy, and policy must not reach a value.
///
/// The counter is between the engine and the bytes, so if anything it measures
/// could change what is read, this is where it would show.
#[test]
fn counting_the_reads_changes_none_of_them() {
    let bytes = curves_page();
    let counter = Arc::new(CountingSource::new(SliceSource::new(bytes.clone())));
    let counted = Document::open_streaming(counter.clone()).expect("it opens");
    let plain = Document::open(bytes).expect("it opens");
    assert_eq!(
        shape_and_pixels(&render(&counted, 0)),
        shape_and_pixels(&render(&plain, 0))
    );
    assert!(counter.bytes_read() > 0, "something was read");
    assert!(counter.reads() > 0);
}
