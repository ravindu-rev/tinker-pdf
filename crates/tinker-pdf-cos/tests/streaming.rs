//! Opening a document over a [`ByteSource`] (`docs/design/streaming-open.md`).
//!
//! The property under test is the one ruling 4 makes non-negotiable: **arrival
//! is not an input.** The same bytes produce the same objects, the same
//! warnings and the same ladder level whether they arrived as one buffer, as
//! aligned chunks, or one byte at a time from the most hostile conforming
//! source that can be written.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use tinker_pdf_cos::{
    ByteSource, CosDocument, CountingSource, LadderLevel, Name, ShreddedSource, SliceSource,
    SourceMiss,
};

/// A minimal, honest document: a catalog, an empty page tree, a classic table.
fn a_document() -> Vec<u8> {
    let body = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n\
xref\n0 3\n\
0000000000 65535 f \n\
0000000009 00000 n \n\
0000000058 00000 n \n\
trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n110\n%%EOF\n";
    body.to_vec()
}

/// A source that answers only the ranges a host has fed it.
///
/// The wasm host loop in miniature: a read outside what has been fed is a miss
/// naming what it wanted, the test feeds that, and asks again.
struct Fed {
    bytes: Vec<u8>,
    available: Mutex<Vec<Range<u64>>>,
}

impl Fed {
    fn new(bytes: Vec<u8>) -> Fed {
        Fed {
            bytes,
            available: Mutex::new(Vec::new()),
        }
    }

    fn feed(&self, range: Range<u64>) {
        self.available.lock().expect("no panic here").push(range);
    }

    fn has(&self, at: u64) -> bool {
        self.available
            .lock()
            .expect("no panic here")
            .iter()
            .any(|r| r.contains(&at))
    }
}

impl ByteSource for Fed {
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
        let end = range.end.min(self.len());
        if range.start >= end {
            return Ok(Arc::from(&[][..]));
        }
        if !self.has(range.start) {
            return Err(SourceMiss::at(range));
        }
        let mut at = range.start;
        while at < end && self.has(at) {
            at += 1;
        }
        let from = range.start as usize;
        let to = at as usize;
        Ok(Arc::from(&self.bytes[from..to]))
    }
}

/// What a document says about itself, as plain values two opens can be
/// compared on.
fn observed(doc: &CosDocument) -> (LadderLevel, usize, Vec<u8>, Option<i64>, usize) {
    let root = doc.resolve_key(doc.trailer(), Name::ROOT);
    let pages = root
        .as_dict()
        .map(|d| doc.resolve_key(d, Name::PAGES))
        .unwrap_or_else(|| Arc::new(tinker_pdf_cos::Object::Null));
    let count = pages.as_dict().and_then(|d| d.get_int(Name::COUNT));
    (
        doc.ladder_level(),
        doc.warnings().len(),
        doc.bytes().to_vec(),
        count,
        doc.xref().len(),
    )
}

#[test]
fn a_buffer_a_slice_source_and_a_shredded_source_open_the_same_document() {
    let bytes = a_document();
    let buffered = CosDocument::open(bytes.clone()).expect("it opens");
    let sliced = CosDocument::open_source(Arc::new(SliceSource::new(bytes.clone())))
        .expect("it opens over a slice source");
    let shredded = CosDocument::open_source(Arc::new(ShreddedSource::new(SliceSource::new(
        bytes.clone(),
    ))))
    .expect("it opens one byte at a time");

    assert_eq!(observed(&buffered), observed(&sliced));
    assert_eq!(
        observed(&buffered),
        observed(&shredded),
        "a source that answers one byte at a time is the same document"
    );
    assert_eq!(buffered.ladder_level(), LadderLevel::Trust);
    assert!(buffered.warnings().is_empty(), "the fixture is honest");
}

#[test]
fn a_document_from_a_buffer_is_never_streamed_and_always_whole() {
    let doc = CosDocument::open(a_document()).expect("it opens");
    assert!(!doc.is_streamed());
    assert!(doc.whole_file_fetched());

    let streamed =
        CosDocument::open_source(Arc::new(SliceSource::new(a_document()))).expect("it opens");
    assert!(streamed.is_streamed());
}

/// The instruments do not change what is read, only what is counted.
#[test]
fn counting_a_source_changes_no_value_it_passes_through() {
    let bytes = a_document();
    let counter = Arc::new(CountingSource::new(SliceSource::new(bytes.clone())));
    let counted = CosDocument::open_source(counter.clone()).expect("it opens");
    let plain = CosDocument::open(bytes).expect("it opens");
    assert_eq!(observed(&counted), observed(&plain));
    assert!(counter.bytes_read() > 0, "something was read");
}

/// A source that has nothing yet is a document that does not open, rather
/// than one that opens empty.
#[test]
fn a_source_that_answers_nothing_refuses_rather_than_opening_empty() {
    let source = Arc::new(Fed::new(a_document()));
    assert!(CosDocument::open_source(source).is_err());
}

/// The retry converges: a host that feeds what a miss asked for and opens
/// again gets the answer a buffer would have given.
#[test]
fn feeding_what_a_miss_named_converges_on_the_buffer_answer() {
    let bytes = a_document();
    let source = Arc::new(Fed::new(bytes.clone()));
    assert!(
        CosDocument::open_source(source.clone()).is_err(),
        "nothing has been fed yet"
    );
    source.feed(0..bytes.len() as u64);
    let fed = CosDocument::open_source(source).expect("it opens once the bytes are there");
    let plain = CosDocument::open(bytes).expect("it opens");
    assert_eq!(observed(&fed), observed(&plain));
}
