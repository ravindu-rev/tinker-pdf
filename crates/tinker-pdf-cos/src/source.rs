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
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use crate::store::{LockExt, MutexExt};

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

/// How many bytes one cached chunk holds.
///
/// Fixed and aligned, so the byte counter measures policy rather than luck:
/// the same document over the same source fetches the same chunks in the same
/// order whatever order its objects are read in. One page, which is also
/// [`crate::limits::MAX_HEADER_SCAN`] -- the head window is exactly one chunk,
/// and a budget that had to explain a granularity nothing else uses would be a
/// budget nobody could check.
pub const CHUNK_SIZE: u64 = 4096;

/// Where a document's bytes live: all of them, or a cache over a source.
///
/// The whole-buffer arm is today's contract and costs nothing new. The chunked
/// arm fetches fixed aligned chunks, each fetched once and kept, so repeated
/// small reads coalesce. Which arm a document has changes which ranges are
/// fetched and never a value, which is the invariant the determinism suite
/// runs over a shredded source to prove.
pub(crate) enum Backing {
    /// Every byte, in hand.
    Whole(Arc<[u8]>),
    /// A chunk cache over a source that may not have them all yet.
    Chunked(ChunkCache),
}

/// Fixed-granularity chunks over a [`ByteSource`].
pub(crate) struct ChunkCache {
    source: Arc<dyn ByteSource>,
    len: u64,
    /// Chunk index to its bytes. Only complete chunks are published: a
    /// half-filled chunk cached after a miss would answer later reads with the
    /// bytes that happened to arrive first, which is the miss's answer baked
    /// in forever.
    chunks: RwLock<BTreeMap<u64, Arc<[u8]>>>,
    /// The whole document, once something has needed all of it.
    whole: OnceLock<Arc<[u8]>>,
    /// Whether that has happened, readable without materialising it.
    fetched_whole: AtomicBool,
}

impl Backing {
    /// Bytes already in hand.
    pub(crate) fn whole_buffer(bytes: Arc<[u8]>) -> Backing {
        Backing::Whole(bytes)
    }

    /// A chunk cache over `source`.
    pub(crate) fn chunked(source: Arc<dyn ByteSource>) -> Backing {
        let len = source.len();
        Backing::Chunked(ChunkCache {
            source,
            len,
            chunks: RwLock::new(BTreeMap::new()),
            whole: OnceLock::new(),
            fetched_whole: AtomicBool::new(false),
        })
    }

    /// How many bytes the document has.
    pub(crate) fn len(&self) -> u64 {
        match self {
            Backing::Whole(bytes) => bytes.len() as u64,
            Backing::Chunked(cache) => cache.len,
        }
    }

    /// Whether reads have to be fetched.
    pub(crate) fn is_streamed(&self) -> bool {
        matches!(self, Backing::Chunked(_))
    }

    /// Whether everything has been fetched, which is what a whole-file
    /// operation on a streamed document has to declare (ruling 10).
    pub(crate) fn whole_fetched(&self) -> bool {
        match self {
            Backing::Whole(_) => true,
            Backing::Chunked(cache) => cache.fetched_whole.load(Ordering::Acquire),
        }
    }

    /// The bytes of `range`, clamped to the document.
    pub(crate) fn window(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
        match self {
            Backing::Whole(bytes) => Ok(Arc::from(clamped(bytes, &range))),
            Backing::Chunked(cache) => cache.window(range),
        }
    }

    /// Every byte, fetching whatever is missing.
    pub(crate) fn materialise(&self) -> Result<Arc<[u8]>, SourceMiss> {
        match self {
            Backing::Whole(bytes) => Ok(Arc::clone(bytes)),
            Backing::Chunked(cache) => cache.materialise(),
        }
    }

    /// This backing as bytes a windowed reader can walk.
    ///
    /// The whole-buffer arm borrows rather than copies, which is what keeps a
    /// document opened from bytes at exactly the cost it always had.
    pub(crate) fn view(&self) -> Bytes<'_> {
        match self {
            Backing::Whole(bytes) => Bytes::Whole(bytes),
            Backing::Chunked(_) => Bytes::Streamed(self),
        }
    }

    /// Every byte as a slice, or nothing when a range is still absent.
    ///
    /// The shape `CosDocument::bytes` needs, which cannot return a result
    /// without changing a signature ruling 12's parity tests compare. A caller
    /// that needs to tell an empty document from bytes that were never fetched
    /// asks [`Backing::whole_fetched`].
    pub(crate) fn whole(&self) -> &[u8] {
        match self {
            Backing::Whole(bytes) => bytes,
            Backing::Chunked(cache) => cache.whole_slice(),
        }
    }
}

impl ChunkCache {
    /// One aligned chunk, fetched at most once.
    fn chunk(&self, index: u64) -> Result<Arc<[u8]>, SourceMiss> {
        if let Some(chunk) = self.chunks.read_lock().get(&index) {
            return Ok(Arc::clone(chunk));
        }
        let start = index.saturating_mul(CHUNK_SIZE).min(self.len);
        let end = start.saturating_add(CHUNK_SIZE).min(self.len);

        // The loop is the whole point of a source being allowed to answer
        // short: bytes are gathered until the chunk is whole, so a source that
        // answers one byte at a time yields the same chunk as one that answers
        // all of it. Nothing is cached until it is complete.
        let mut bytes = Vec::with_capacity((end - start) as usize);
        let mut at = start;
        while at < end {
            let got = self.source.read(at..end)?;
            if got.is_empty() {
                // A conforming source never does this for a non-empty range
                // inside its length. Treating it as a miss rather than looping
                // forever is ruling 1: a hostile host is untrusted input too.
                return Err(SourceMiss::at(at..end));
            }
            at = at.saturating_add(got.len() as u64).min(end);
            bytes.extend_from_slice(&got);
        }
        bytes.truncate((end - start) as usize);

        let chunk: Arc<[u8]> = Arc::from(bytes);
        let mut chunks = self.chunks.write_lock();
        // Another thread may have fetched the same chunk meanwhile. Both read
        // the same bytes, so whichever is already there wins and this one is
        // dropped, exactly as the slot store resolves the same race.
        Ok(Arc::clone(
            chunks.entry(index).or_insert_with(|| Arc::clone(&chunk)),
        ))
    }

    fn window(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
        let start = range.start.min(self.len);
        let end = range.end.min(self.len).max(start);
        if start == end {
            return Ok(Arc::from(&[][..]));
        }
        let first = start / CHUNK_SIZE;
        let last = (end - 1) / CHUNK_SIZE;
        let mut out = Vec::with_capacity((end - start) as usize);
        for index in first..=last {
            let chunk = self.chunk(index)?;
            let base = index.saturating_mul(CHUNK_SIZE);
            let from = start.saturating_sub(base).min(chunk.len() as u64);
            let to = end.saturating_sub(base).min(chunk.len() as u64);
            out.extend_from_slice(chunk.get(from as usize..to as usize).unwrap_or(&[]));
        }
        Ok(Arc::from(out))
    }

    fn materialise(&self) -> Result<Arc<[u8]>, SourceMiss> {
        if let Some(whole) = self.whole.get() {
            return Ok(Arc::clone(whole));
        }
        let all = self.window(0..self.len)?;
        // The bytes are set before the flag, so a reader that sees the flag
        // finds them.
        let _ = self.whole.set(Arc::clone(&all));
        self.fetched_whole.store(true, Ordering::Release);
        Ok(self.whole.get().map_or(all, Arc::clone))
    }

    fn whole_slice(&self) -> &[u8] {
        if self.materialise().is_err() {
            // Ruling 2: an absent range degrades to nothing rather than to
            // wrong bytes. The caller sees an empty document, and
            // `whole_fetched` says which of the two it is.
            return &[];
        }
        self.whole.get().map_or(&[], |bytes| bytes)
    }
}

/// Where the walker's bytes come from.
///
/// One walker, two suppliers. The whole-buffer arm hands back the whole
/// buffer for every window, so a document opened from bytes takes exactly the
/// path it always took at exactly the cost it always had; the streamed arm
/// fetches a window per section and grows it until the section parses. A
/// second walk written for streaming would be a second reading of 7.5.4 and
/// 7.5.8, and the two would disagree in private.
pub(crate) enum Bytes<'a> {
    /// Every byte, in hand.
    Whole(&'a [u8]),
    /// A chunk cache, which pays for what the walk asks for.
    Streamed(&'a Backing),
}

/// Bytes covering part of a document, and the offset they start at.
pub(crate) struct Window<'a> {
    base: u64,
    held: Held<'a>,
}

enum Held<'a> {
    Borrowed(&'a [u8]),
    Owned(Arc<[u8]>),
}

impl Window<'_> {
    pub(crate) fn bytes(&self) -> &[u8] {
        match &self.held {
            Held::Borrowed(bytes) => bytes,
            Held::Owned(bytes) => bytes,
        }
    }

    /// A document offset as an offset into this window, when it lies inside.
    pub(crate) fn local(&self, at: u64) -> Option<u64> {
        let local = at.checked_sub(self.base)?;
        (local <= self.bytes().len() as u64).then_some(local)
    }

    /// A window offset back as a document offset.
    pub(crate) fn abs(&self, local: u64) -> u64 {
        self.base.saturating_add(local)
    }

    /// The document offset just past the last byte this window holds.
    pub(crate) fn end(&self) -> u64 {
        self.base.saturating_add(self.bytes().len() as u64)
    }
}

impl Bytes<'_> {
    pub(crate) fn len(&self) -> u64 {
        match self {
            Bytes::Whole(buf) => buf.len() as u64,
            Bytes::Streamed(backing) => backing.len(),
        }
    }

    /// A window of at least `want` bytes from `at`, or `None` when the bytes
    /// are not available.
    ///
    /// The whole-buffer arm ignores `want` and hands back everything, which is
    /// what keeps that path free of any new cost or any new behaviour.
    pub(crate) fn window(&self, at: u64, want: u64) -> Option<Window<'_>> {
        match self {
            Bytes::Whole(buf) => Some(Window {
                base: 0,
                held: Held::Borrowed(buf),
            }),
            Bytes::Streamed(backing) => {
                let end = at.saturating_add(want).min(backing.len());
                let bytes = backing.window(at..end).ok()?;
                Some(Window {
                    base: at,
                    held: Held::Owned(bytes),
                })
            }
        }
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
