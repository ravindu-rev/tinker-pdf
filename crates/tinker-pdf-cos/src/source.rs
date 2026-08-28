//! Where a document's bytes come from, when they are not all in hand.
//!
//! Feature documentation: `docs/design/streaming-open.md`.
//!
//! A caller holding a byte *source* rather than a byte *buffer* — a memory
//! map, a file handle, an HTTP server answering range requests — opens a
//! document through [`ByteSource`]. The engine defines the trait; hosts
//! implement transport, which is the `FontProvider` pattern one layer down.
//! Nothing here performs any transport itself: there is no HTTP client, no
//! file I/O and no mmap call anywhere in this workspace, because
//! `wasm32-unknown-unknown` has none of them and is a first-class target.
//!
//! # Synchronous, forever
//!
//! An async trait was considered and rejected. It would need a runtime the
//! workspace does not have, it would colour every function from `load` to
//! `Device`, and it would make output depend on when bytes arrived — which is
//! exactly what ruling 4 exists to prevent. The shape here is sync plus a
//! typed miss: a read the host has not fed yet returns [`SourceMiss`] naming
//! the range it wanted, and that propagates upward without ever becoming a
//! value. A native host backs the source with a mapping and never misses; a
//! wasm host feeds ranges and calls again, and because parsing is pure and the
//! store caches, the retry repeats no completed work.
//!
//! # Arrival is not an input
//!
//! Ruling 4's contract extends rather than bends. The same bytes and the same
//! query sequence produce the same values, the same warnings and the same
//! pixels regardless of chunking, of how a source split its reads, or of how
//! many misses happened on the way. [`ShreddedSource`] exists to prove it:
//! it is the most hostile conforming source that can be written, and the
//! determinism suite runs over it.

use core::fmt;
use core::ops::Range;
use std::sync::{Arc, Mutex};

use crate::store::MutexExt;

/// A range a source could not supply.
///
/// A typed refusal in the sense of rulings 2 and 10, not an error string: it
/// names the exact bytes the engine wanted, so a host can fetch them and ask
/// again. It is never converted into a value — a slot that could not be read
/// must not become a published null, because a later read would then get the
/// miss's answer forever.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceMiss {
    /// The bytes the engine asked for and did not get.
    pub needed: Range<u64>,
}

impl SourceMiss {
    /// A miss naming `range`.
    #[must_use]
    pub fn at(range: Range<u64>) -> SourceMiss {
        SourceMiss { needed: range }
    }
}

impl fmt::Display for SourceMiss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "bytes {}..{} are not available",
            self.needed.start, self.needed.end
        )
    }
}

impl std::error::Error for SourceMiss {}

/// A document's bytes, as ranges a host can answer.
///
/// Not an I/O trait: it performs no transport, blocks on nothing, and on wasm
/// is backed by memory the host has already fetched.
pub trait ByteSource: Send + Sync {
    /// How many bytes the whole document has.
    ///
    /// Fixed for the life of the source. Every offset in a PDF is measured
    /// against the end of the file, so a length that moved would move the
    /// document under the reader.
    fn len(&self) -> u64;

    /// Whether the document has no bytes at all.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The bytes at the start of `range`, or a miss naming what was wanted.
    ///
    /// A source may return **fewer** bytes than asked for, exactly as a
    /// POSIX read may, and callers loop until they have the range or miss. It
    /// must never return zero bytes for a non-empty range that lies inside
    /// [`ByteSource::len`], because a caller looping on that would not
    /// terminate. A range past the end is clamped, and one entirely past the
    /// end is empty rather than a miss: end of file is a fact about the
    /// document, not a failure to fetch.
    fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss>;
}

impl<T: ByteSource + ?Sized> ByteSource for Arc<T> {
    fn len(&self) -> u64 {
        (**self).len()
    }

    fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
        (**self).read(range)
    }
}

/// The degenerate source: bytes already in memory.
///
/// This is today's contract, which is why `Document::open(bytes)` keeps its
/// exact signature. It never misses and never returns a short read.
#[derive(Clone, Debug)]
pub struct SliceSource {
    bytes: Arc<[u8]>,
}

impl SliceSource {
    /// A source over `bytes`.
    #[must_use]
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> SliceSource {
        SliceSource {
            bytes: bytes.into(),
        }
    }

    /// The bytes behind it, shared rather than copied.
    #[must_use]
    pub fn bytes(&self) -> Arc<[u8]> {
        Arc::clone(&self.bytes)
    }
}

impl ByteSource for SliceSource {
    fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
        Ok(Arc::from(clamped(&self.bytes, &range)))
    }
}

/// The bytes of `range` inside `buf`, clamped to it.
fn clamped<'a>(buf: &'a [u8], range: &Range<u64>) -> &'a [u8] {
    let start = usize::try_from(range.start)
        .unwrap_or(usize::MAX)
        .min(buf.len());
    let end = usize::try_from(range.end)
        .unwrap_or(usize::MAX)
        .min(buf.len())
        .max(start);
    buf.get(start..end).unwrap_or(&[])
}

/// A source that records what was asked of the one underneath it.
///
/// The instrument the byte budgets are measured with. It changes no value it
/// passes through — ruling 4 would not survive a counter that did — so a
/// document opened over one is the same document, byte for byte, as the same
/// document opened over the source it wraps.
pub struct CountingSource<S> {
    inner: S,
    reads: Mutex<Vec<Range<u64>>>,
}

impl<S: ByteSource> CountingSource<S> {
    /// Wraps `inner`, counting from zero.
    pub fn new(inner: S) -> CountingSource<S> {
        CountingSource {
            inner,
            reads: Mutex::new(Vec::new()),
        }
    }

    /// Every range asked for, in the order it was asked for.
    ///
    /// The order is a diagnostic and never an input: two runs that fetch the
    /// same bytes in a different order produce the same document.
    #[must_use]
    pub fn ranges(&self) -> Vec<Range<u64>> {
        self.reads.lock_safe().clone()
    }

    /// How many reads reached the source underneath.
    #[must_use]
    pub fn reads(&self) -> usize {
        self.reads.lock_safe().len()
    }

    /// How many bytes were asked for in total, counting a byte twice when it
    /// was fetched twice.
    ///
    /// The gross figure on purpose: a chunk cache that re-fetched what it
    /// already held would be invisible in a de-duplicated one, and the cache
    /// is the thing being measured.
    #[must_use]
    pub fn bytes_read(&self) -> u64 {
        self.reads
            .lock_safe()
            .iter()
            .map(|r| r.end.saturating_sub(r.start))
            .sum()
    }

    /// Whether any read overlapped `range`.
    ///
    /// What "not one read touches the tail" is asserted with.
    #[must_use]
    pub fn touched(&self, range: &Range<u64>) -> bool {
        self.reads
            .lock_safe()
            .iter()
            .any(|read| read.start < range.end && range.start < read.end)
    }

    /// The reads that overlapped `range`, for a failure message that says
    /// which ones did.
    #[must_use]
    pub fn touching(&self, range: &Range<u64>) -> Vec<Range<u64>> {
        self.reads
            .lock_safe()
            .iter()
            .filter(|read| read.start < range.end && range.start < read.end)
            .cloned()
            .collect()
    }

    /// Forgets what has been counted so far, so a measurement can start at a
    /// point other than `open`.
    pub fn reset(&self) {
        self.reads.lock_safe().clear();
    }
}

impl<S: ByteSource> ByteSource for CountingSource<S> {
    fn len(&self) -> u64 {
        self.inner.len()
    }

    fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
        let got = self.inner.read(range.clone())?;
        // What arrived rather than what was asked for: a short read costs the
        // bytes it delivered, and a budget measured on the request would flatter
        // a source that answers in slices.
        let start = range.start.min(self.inner.len());
        let end = start.saturating_add(got.len() as u64);
        self.reads.lock_safe().push(start..end);
        Ok(got)
    }
}

/// The most hostile source that still conforms.
///
/// It never serves a range whole when it can serve it in pieces: every read
/// of more than one byte comes back as exactly one byte, so a caller that
/// forgot to loop stalls immediately and a caller that assumed it got what it
/// asked for reads the wrong bytes. Nothing is reordered and nothing is
/// withheld, because a source that lied about the bytes would be testing the
/// wrong thing — what is being tested is that arrival is not an input.
pub struct ShreddedSource<S> {
    inner: S,
}

impl<S: ByteSource> ShreddedSource<S> {
    /// Wraps `inner`, splitting every read it passes on.
    pub fn new(inner: S) -> ShreddedSource<S> {
        ShreddedSource { inner }
    }
}

impl<S: ByteSource> ByteSource for ShreddedSource<S> {
    fn len(&self) -> u64 {
        self.inner.len()
    }

    fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
        let end = range.end.min(range.start.saturating_add(1));
        self.inner.read(range.start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A source that answers nothing, for the miss path.
    struct Absent {
        len: u64,
    }

    impl ByteSource for Absent {
        fn len(&self) -> u64 {
            self.len
        }

        fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
            Err(SourceMiss::at(range))
        }
    }

    #[test]
    fn a_slice_source_answers_every_range_in_full() {
        let source = SliceSource::new(&b"0123456789"[..]);
        assert_eq!(source.len(), 10);
        assert!(!source.is_empty());
        assert_eq!(&*source.read(0..10).expect("in bounds"), b"0123456789");
        assert_eq!(&*source.read(3..6).expect("in bounds"), b"345");
        assert_eq!(&*source.read(0..0).expect("empty"), b"");
    }

    /// End of file is a fact about the document, not a failure to fetch, so a
    /// range past the end clamps rather than missing. A reader looking for
    /// `startxref` in the last 64 KB of a 300-byte file asks for exactly that.
    #[test]
    fn a_range_past_the_end_clamps_rather_than_missing() {
        let source = SliceSource::new(&b"abc"[..]);
        assert_eq!(&*source.read(1..99).expect("clamped"), b"bc");
        assert_eq!(&*source.read(99..200).expect("clamped"), b"");
        // An inverted range is not a range a reader should produce, and it
        // must still not panic (ruling 1). Built rather than written, because
        // a literal inverted range is a lint.
        let inverted = Range {
            start: 2u64,
            end: 1u64,
        };
        assert_eq!(&*source.read(inverted).expect("inverted"), b"");
    }

    #[test]
    fn a_miss_names_the_range_it_wanted() {
        let source = Absent { len: 100 };
        let miss = source.read(10..20).expect_err("it answers nothing");
        assert_eq!(miss.needed, 10..20);
        assert_eq!(miss.to_string(), "bytes 10..20 are not available");
    }

    /// The counter measures what arrived, not what was asked for.
    #[test]
    fn the_counter_charges_a_short_read_for_what_it_delivered() {
        let counted = CountingSource::new(ShreddedSource::new(SliceSource::new(&b"abcdef"[..])));
        assert_eq!(&*counted.read(0..6).expect("one byte"), b"a");
        assert_eq!(counted.reads(), 1);
        assert_eq!(counted.bytes_read(), 1, "one byte arrived, not six");
        assert_eq!(counted.ranges(), vec![0..1]);
    }

    #[test]
    fn the_counter_says_which_reads_touched_a_range() {
        let counted = CountingSource::new(SliceSource::new(&b"0123456789"[..]));
        let _ = counted.read(0..4);
        let _ = counted.read(8..10);
        assert!(counted.touched(&(9..10)));
        assert_eq!(counted.touching(&(9..10)), vec![8..10]);
        assert!(!counted.touched(&(4..8)), "nothing read the middle");
        assert!(!counted.touched(&(4..4)), "an empty range touches nothing");
        assert_eq!(counted.bytes_read(), 6);
        counted.reset();
        assert_eq!(counted.reads(), 0);
        assert_eq!(counted.bytes_read(), 0);
    }

    /// The shredder never serves a range whole when it can serve it in
    /// pieces, and never serves nothing for a range that has bytes in it —
    /// which is the property that keeps a looping caller terminating.
    #[test]
    fn the_shredder_answers_one_byte_at_a_time_and_never_none() {
        let source = ShreddedSource::new(SliceSource::new(&b"abcdef"[..]));
        let mut at = 0u64;
        let mut seen = Vec::new();
        while at < source.len() {
            let got = source.read(at..source.len()).expect("in bounds");
            assert_eq!(got.len(), 1, "every read of a longer range is one byte");
            seen.extend_from_slice(&got);
            at += got.len() as u64;
        }
        assert_eq!(seen, b"abcdef");
    }

    /// A miss passes through the instruments unchanged: neither of them may
    /// turn an absent range into bytes.
    #[test]
    fn a_miss_survives_both_wrappers() {
        let counted = CountingSource::new(ShreddedSource::new(Absent { len: 50 }));
        let miss = counted.read(4..40).expect_err("nothing is available");
        assert_eq!(miss.needed, 4..5, "shredded to one byte, and still absent");
        assert_eq!(counted.bytes_read(), 0, "a miss delivers nothing");
    }

    /// The trait is object-safe and shareable, which is what a host holding
    /// one behind an `Arc` needs.
    #[test]
    fn a_source_is_shareable_behind_a_trait_object() {
        let source: Arc<dyn ByteSource> = Arc::new(SliceSource::new(&b"shared"[..]));
        let clone = Arc::clone(&source);
        assert_eq!(&*clone.read(0..6).expect("in bounds"), b"shared");
        assert_eq!(source.len(), 6);
    }
}
