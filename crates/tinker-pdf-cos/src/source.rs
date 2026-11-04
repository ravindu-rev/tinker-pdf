//! Where a document's bytes come from, when they are not all in hand.
//!
//! Feature documentation: `docs/design/streaming-open.md`.
//!
//! A caller holding a byte *source* rather than a byte *buffer* — a memory
//! map, a file handle, an HTTP server answering range requests — opens a
//! document through [`ByteSource`]. The engine defines the trait; hosts
//! implement transport, which is the `FontProvider` pattern one layer down.
//! Nothing here performs any transport itself: there is no HTTP client, no
//! file I/O and no mmap call anywhere in the engine crates, because
//! `wasm32-unknown-unknown` has none of them and is a first-class target. A
//! host's own source, such as the CLI's over a file, is the host's code.
//!
//! # Synchronous, forever
//!
//! An async trait was considered and rejected. It would need a runtime the
//! workspace does not have, it would colour every function from `load` to
//! `Device`, and it would make output depend on when bytes arrived — which is
//! exactly what ruling 4 exists to prevent. The shape here is sync plus a
//! typed miss: a read the host has not fed yet returns [`SourceMiss`] naming
//! the range it wanted, and that propagates upward without ever becoming a
//! value. A native host backs the source with a mapping, which never misses,
//! or with reads at offsets, which miss only when a read fails; a wasm host
//! feeds ranges and calls again, and because parsing is pure and the store
//! caches, the retry repeats no completed work.
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
/// small reads coalesce, and the chunks a read lacks in a row are fetched with
/// one read; until something needs every byte: then it holds the whole
/// document once, in place of its chunks, and cuts every later read from it.
/// Which arm a document has changes which ranges are fetched and never a
/// value, which is the invariant the determinism suite runs over a shredded
/// source to prove.
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
    /// in forever. Empty once `whole` is set, which holds them all.
    chunks: RwLock<BTreeMap<u64, Arc<[u8]>>>,
    /// The whole document, once something has needed all of it; every read
    /// after that is cut from it.
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

    /// The bytes of `range`, clamped to the document, read for the caller
    /// alone and kept by nothing.
    ///
    /// Straight from the source on the chunked arm: no chunk is published and
    /// none is consulted, so a pass that touches every object's header, which
    /// is what the windowed verdict is, leaves the cache holding what it held
    /// before. What it read is the caller's, and gone when the caller drops
    /// it. Once the whole document is held, it is cut from that instead.
    pub(crate) fn read_transient(&self, range: Range<u64>) -> Result<Vec<u8>, SourceMiss> {
        match self {
            Backing::Whole(bytes) => Ok(clamped(bytes, &range).to_vec()),
            Backing::Chunked(cache) => {
                if let Some(whole) = cache.whole.get() {
                    return Ok(clamped(whole, &range).to_vec());
                }
                let start = range.start.min(cache.len);
                let end = range.end.min(cache.len).max(start);
                let mut bytes = Vec::new();
                cache.gather(start, end, &mut bytes)?;
                Ok(bytes)
            }
        }
    }

    /// How many bytes this backing holds: the buffer, or every cached chunk
    /// and the whole document, which are never both held once a fetch of the
    /// whole has finished.
    ///
    /// The instrument for "kept by nothing", which a byte counter over the
    /// source cannot see: a read that went to the source and was then cached
    /// costs the same transport as one that was dropped.
    #[cfg(test)]
    pub(crate) fn retained(&self) -> u64 {
        match self {
            Backing::Whole(bytes) => bytes.len() as u64,
            Backing::Chunked(cache) => {
                let chunks: u64 = cache
                    .chunks
                    .read_lock()
                    .values()
                    .map(|chunk| chunk.len() as u64)
                    .sum();
                let whole = cache.whole.get().map_or(0, |bytes| bytes.len() as u64);
                chunks.saturating_add(whole)
            }
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
    /// The chunks `first..past`, which the cache did not hold, fetched as one
    /// range: one read, when the source answers it whole.
    ///
    /// Only whole chunks are published. A miss part-way through keeps the
    /// chunks that arrived whole before it and drops the part-filled one, as
    /// fetching them one at a time would.
    fn run(&self, first: u64, past: u64) -> Result<Vec<Arc<[u8]>>, SourceMiss> {
        let start = first.saturating_mul(CHUNK_SIZE).min(self.len);
        let end = past.saturating_mul(CHUNK_SIZE).min(self.len);
        let mut bytes = Vec::new();
        let fetched = self.gather(start, end, &mut bytes);
        if fetched.is_err() {
            // Nothing is cached until it is complete: the chunk the miss
            // stopped in is dropped.
            let whole = bytes.len() - bytes.len() % CHUNK_SIZE as usize;
            bytes.truncate(whole);
        }
        let chunks: Vec<Arc<[u8]>> = bytes.chunks(CHUNK_SIZE as usize).map(Arc::from).collect();
        let chunks = self.publish(first, chunks);
        fetched.map(|()| chunks)
    }

    /// Publishes `chunks`, consecutive from `first`, and hands back the ones
    /// to read.
    fn publish(&self, first: u64, chunks: Vec<Arc<[u8]>>) -> Vec<Arc<[u8]>> {
        let mut cached = self.chunks.write_lock();
        // A fetch of the whole that finished while these chunks were read
        // holds their bytes already, and emptied the map under this lock after
        // setting the whole: the chunks are the caller's alone, never
        // published beside it.
        if self.whole.get().is_some() {
            return chunks;
        }
        // Another thread may have fetched the same chunk meanwhile. Both read
        // the same bytes, so whichever is already there wins and this one is
        // dropped, exactly as the slot store resolves the same race.
        (first..)
            .zip(chunks)
            .map(|(index, chunk)| Arc::clone(cached.entry(index).or_insert(chunk)))
            .collect()
    }

    /// Every byte of the document, read from the source as one range, so that
    /// the bytes are held once.
    ///
    /// An answer of the whole length is kept as it is: a source that reads a
    /// file into one allocation costs no second one. A short answer is the
    /// start of a buffer of the document's length, filled in place as the
    /// rest arrives, so a source that answers a piece at a time is never
    /// joined from a copy either. The buffer's length is the host's claim, as
    /// a window's is.
    fn read_whole(&self) -> Result<Arc<[u8]>, SourceMiss> {
        let whole = 0..self.len;
        let len = usize::try_from(self.len).map_err(|_| SourceMiss::at(whole.clone()))?;
        let first = self.source.read(whole.clone())?;
        if first.len() == len {
            return Ok(first);
        }
        if first.len() > len {
            return Ok(Arc::from(&first[..len]));
        }
        if first.is_empty() {
            // As `gather` reads it: a conforming source never answers nothing
            // for a range inside its length (ruling 1).
            return Err(SourceMiss::at(whole));
        }
        let mut buffer: Arc<[u8]> = std::iter::repeat_n(0, len).collect();
        let Some(bytes) = Arc::get_mut(&mut buffer) else {
            // A buffer made here is held by nothing else.
            return Err(SourceMiss::at(whole));
        };
        bytes[..first.len()].copy_from_slice(&first);
        let mut at = first.len();
        while at < len {
            let got = self.source.read(at as u64..self.len)?;
            if got.is_empty() {
                return Err(SourceMiss::at(at as u64..self.len));
            }
            let took = got.len().min(len - at);
            bytes[at..at + took].copy_from_slice(&got[..took]);
            at += took;
        }
        Ok(buffer)
    }

    /// The bytes of `start..end`, which lies inside the document, asked of the
    /// source until it has answered all of them, onto `bytes`.
    ///
    /// The loop is the whole point of a source being allowed to answer short:
    /// bytes are gathered until the range is whole, so a source that answers
    /// one byte at a time yields the same bytes as one that answers all of
    /// them. On a miss, `bytes` ends with what arrived before it.
    fn gather(&self, start: u64, end: u64, bytes: &mut Vec<u8>) -> Result<(), SourceMiss> {
        // The length is the host's claim, so what is reserved ahead of the
        // bytes is bounded and the rest grows as they arrive (ruling 1).
        const RESERVE: usize = 1 << 20;
        let want = usize::try_from(end.saturating_sub(start)).unwrap_or(usize::MAX);
        bytes.reserve(want.min(RESERVE));
        let mut at = start;
        while at < end {
            let got = self.source.read(at..end)?;
            if got.is_empty() {
                // A conforming source never does this for a non-empty range
                // inside its length. Treating it as a miss rather than looping
                // forever is ruling 1: a hostile host is untrusted input too.
                return Err(SourceMiss::at(at..end));
            }
            // An answer longer than the range is cut to it.
            let took = got
                .len()
                .min(usize::try_from(end - at).unwrap_or(usize::MAX));
            bytes.extend_from_slice(&got[..took]);
            at = at.saturating_add(took as u64);
        }
        Ok(())
    }

    fn window(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
        let start = range.start.min(self.len);
        let end = range.end.min(self.len).max(start);
        if start == end {
            return Ok(Arc::from(&[][..]));
        }
        if let Some(whole) = self.whole.get() {
            return Ok(Arc::from(clamped(whole, &(start..end))));
        }
        let first = start / CHUNK_SIZE;
        let last = (end - 1) / CHUNK_SIZE;
        let held: Vec<Option<Arc<[u8]>>> = {
            let cached = self.chunks.read_lock();
            (first..=last)
                .map(|index| cached.get(&index).cloned())
                .collect()
        };
        // The chunks the cache holds, and each run of those it does not,
        // fetched with one read.
        let mut chunks = Vec::with_capacity(held.len());
        let mut slots = held.into_iter().peekable();
        let mut index = first;
        while let Some(slot) = slots.next() {
            if let Some(chunk) = slot {
                chunks.push(chunk);
                index += 1;
                continue;
            }
            let mut past = index + 1;
            while slots.next_if(Option::is_none).is_some() {
                past += 1;
            }
            chunks.extend(self.run(index, past)?);
            index = past;
        }
        let mut out = Vec::with_capacity((end - start) as usize);
        for (index, chunk) in (first..).zip(&chunks) {
            let base = index.saturating_mul(CHUNK_SIZE);
            let from = start.saturating_sub(base).min(chunk.len() as u64);
            let to = end.saturating_sub(base).min(chunk.len() as u64);
            out.extend_from_slice(chunk.get(from as usize..to as usize).unwrap_or(&[]));
        }
        Ok(Arc::from(out))
    }

    /// Every byte, held once: read whole from the source, set in place of
    /// the chunks, which are dropped.
    fn materialise(&self) -> Result<Arc<[u8]>, SourceMiss> {
        if let Some(whole) = self.whole.get() {
            return Ok(Arc::clone(whole));
        }
        let all = self.read_whole()?;
        // The bytes are set before the flag, so a reader that sees the flag
        // finds them. Another thread's whole, set first, is kept.
        let _ = self.whole.set(Arc::clone(&all));
        self.fetched_whole.store(true, Ordering::Release);
        // Emptied after the whole is set and under the lock a chunk is
        // published under, so a chunk gathered meanwhile is not published
        // beside it (`publish`).
        self.chunks.write_lock().clear();
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

    /// `len` bytes no two neighbours of which are alike.
    fn file(len: usize) -> Arc<[u8]> {
        (0..len).map(|at| (at % 251) as u8).collect()
    }

    /// A source over `bytes` that answers at most `piece` bytes a read, and
    /// a read of every byte with `bytes` itself rather than a copy.
    struct Pieces {
        bytes: Arc<[u8]>,
        piece: usize,
    }

    impl ByteSource for Pieces {
        fn len(&self) -> u64 {
            self.bytes.len() as u64
        }

        fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
            if range == (0..self.len()) && self.piece >= self.bytes.len() {
                return Ok(Arc::clone(&self.bytes));
            }
            let start = range.start.min(self.len());
            let end = range.end.min(start.saturating_add(self.piece as u64));
            Ok(Arc::from(clamped(&self.bytes, &(start..end))))
        }
    }

    /// The exit: a fetch of the whole over a 1 MiB source, after the reads an
    /// open makes have cached chunks at both ends, holds 1 MiB, the chunks
    /// gone; and a read after it is cut from the whole, publishing nothing.
    #[test]
    fn a_whole_file_fetch_holds_the_file_once() {
        const MIB: u64 = 1 << 20;
        let bytes = file(MIB as usize);
        let backing = Backing::chunked(Arc::new(SliceSource::new(Arc::clone(&bytes))));
        backing.window(0..100).expect("the head");
        backing.window(MIB - 100..MIB).expect("the tail");
        assert_eq!(backing.retained(), 2 * CHUNK_SIZE, "two chunks before");

        let whole = backing.materialise().expect("every byte");
        assert_eq!(&*whole, &*bytes);
        assert!(backing.whole_fetched());
        assert_eq!(backing.retained(), MIB, "the file once, its chunks dropped");

        let window = backing.window(5_000..6_000).expect("a window");
        assert_eq!(&*window, &bytes[5_000..6_000]);
        assert_eq!(backing.retained(), MIB, "a later read publishes nothing");
        assert!(Arc::ptr_eq(&backing.materialise().expect("again"), &whole));
    }

    /// A source that answers a read of every byte in one allocation hands
    /// the backing its buffer: the bytes are held once in the process.
    #[test]
    fn an_answer_of_the_whole_length_is_the_buffer_itself() {
        let bytes = file(50_000);
        let backing = Backing::chunked(Arc::new(Pieces {
            bytes: Arc::clone(&bytes),
            piece: usize::MAX,
        }));
        let whole = backing.materialise().expect("every byte");
        assert!(Arc::ptr_eq(&whole, &bytes), "the source's own allocation");
    }

    /// A source that answers a piece at a time fills one buffer of the
    /// document's length, as one that answers a byte at a time does: the same
    /// bytes, held once.
    #[test]
    fn a_short_answer_fills_the_buffer_in_place() {
        let bytes = file(50_000);
        let pieces = Backing::chunked(Arc::new(Pieces {
            bytes: Arc::clone(&bytes),
            piece: 7_000,
        }));
        assert_eq!(&*pieces.materialise().expect("every byte"), &*bytes);
        assert_eq!(pieces.retained(), bytes.len() as u64);

        let small = file(3_000);
        let shredded = Backing::chunked(Arc::new(ShreddedSource::new(SliceSource::new(
            Arc::clone(&small),
        ))));
        assert_eq!(&*shredded.materialise().expect("every byte"), &*small);
        assert_eq!(shredded.retained(), small.len() as u64);

        let absent = Backing::chunked(Arc::new(Absent { len: 10 }));
        assert_eq!(
            absent.materialise().expect_err("nothing arrives").needed,
            0..10
        );
        assert!(!absent.whole_fetched());
    }

    /// A source that, asked for the first chunk past the head, has the
    /// backing it feeds fetch the whole before it answers: a chunk in flight
    /// when the chunk map is emptied.
    struct Racing {
        bytes: Arc<[u8]>,
        backing: OnceLock<std::sync::Weak<Backing>>,
        raced: AtomicBool,
    }

    impl ByteSource for Racing {
        fn len(&self) -> u64 {
            self.bytes.len() as u64
        }

        fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
            if range.start >= CHUNK_SIZE && !self.raced.swap(true, Ordering::SeqCst) {
                if let Some(backing) = self.backing.get().and_then(std::sync::Weak::upgrade) {
                    backing.materialise()?;
                }
            }
            Ok(Arc::from(clamped(&self.bytes, &range)))
        }
    }

    /// A chunk read while a fetch of the whole finishes answers its reader
    /// and is not published beside the whole: the file stays held once.
    #[test]
    fn a_chunk_in_flight_when_the_whole_arrives_is_not_published() {
        let bytes = file(20_000);
        let source = Arc::new(Racing {
            bytes: Arc::clone(&bytes),
            backing: OnceLock::new(),
            raced: AtomicBool::new(false),
        });
        let backing = Arc::new(Backing::chunked(source.clone()));
        source
            .backing
            .set(Arc::downgrade(&backing))
            .expect("set once");
        let window = backing
            .window(CHUNK_SIZE + 10..CHUNK_SIZE + 20)
            .expect("the window");
        assert_eq!(&*window, &bytes[4_106..4_116]);
        assert!(source.raced.load(Ordering::SeqCst), "the race was run");
        assert!(backing.whole_fetched());
        assert_eq!(backing.retained(), bytes.len() as u64, "no chunk beside it");
    }

    /// Once the whole is held, a window and a transient read are cut from it
    /// and the source is not asked again.
    #[test]
    fn reads_after_a_whole_fetch_do_not_reach_the_source() {
        let bytes = file(30_000);
        let source = Arc::new(CountingSource::new(SliceSource::new(Arc::clone(&bytes))));
        let backing = Backing::chunked(source.clone());
        backing.materialise().expect("every byte");
        let reads = source.reads();
        assert_eq!(
            &*backing.window(9_000..9_500).expect("a window"),
            &bytes[9_000..9_500]
        );
        assert_eq!(
            backing.read_transient(100..200).expect("a transient read"),
            &bytes[100..200]
        );
        assert_eq!(source.reads(), reads, "the source was not asked again");
    }

    /// A chunk cache over `bytes`, and the counter that sees its every read.
    fn counted(bytes: &Arc<[u8]>) -> (Arc<CountingSource<SliceSource>>, Backing) {
        let source = Arc::new(CountingSource::new(SliceSource::new(Arc::clone(bytes))));
        let backing = Backing::chunked(source.clone());
        (source, backing)
    }

    /// The exit: a window over chunks the cache does not hold is one read,
    /// from its first chunk's start to its last chunk's end and no further,
    /// and each chunk is published whole.
    #[test]
    fn a_window_over_missing_chunks_is_one_read() {
        const C: u64 = CHUNK_SIZE;
        let bytes = file(10 * C as usize + 500);
        let (source, backing) = counted(&bytes);
        let window = backing.window(100..3 * C + 100).expect("four chunks");
        assert_eq!(&*window, &bytes[100..3 * C as usize + 100]);
        assert_eq!(source.ranges(), vec![0..4 * C], "one read of the four");
        assert_eq!(backing.retained(), 4 * C, "each chunk published");

        backing.window(C..2 * C).expect("a cached chunk");
        assert_eq!(source.reads(), 1, "what the cache holds is not read again");

        // The document's last chunk is short, and the run ends with it.
        let len = bytes.len() as u64;
        let tail = backing.window(9 * C + 10..len).expect("the last two");
        assert_eq!(&*tail, &bytes[9 * C as usize + 10..]);
        assert_eq!(source.ranges(), vec![0..4 * C, 9 * C..len]);
        assert_eq!(backing.retained(), 5 * C + 500);
    }

    /// A chunk the cache holds splits the missing chunks around it into two
    /// runs, each one read.
    #[test]
    fn a_cached_chunk_splits_a_window_into_two_reads() {
        const C: u64 = CHUNK_SIZE;
        let bytes = file(6 * C as usize);
        let (source, backing) = counted(&bytes);
        backing.window(2 * C + 5..2 * C + 6).expect("chunk 2");
        source.reset();

        let window = backing.window(0..5 * C).expect("chunks 0 to 4");
        assert_eq!(&*window, &bytes[..5 * C as usize]);
        assert_eq!(
            source.ranges(),
            vec![0..2 * C, 3 * C..5 * C],
            "the runs either side of the cached chunk"
        );
        assert_eq!(backing.retained(), 5 * C);
    }

    /// A source that answers short is asked again from where it stopped,
    /// across the run's chunk boundaries as within one chunk.
    #[test]
    fn a_short_reading_source_still_fills_a_coalesced_run() {
        const C: u64 = CHUNK_SIZE;
        let bytes = file(4 * C as usize);
        let source = Arc::new(CountingSource::new(Pieces {
            bytes: Arc::clone(&bytes),
            piece: 1_000,
        }));
        let backing = Backing::chunked(source.clone());
        let window = backing.window(10..3 * C - 10).expect("three chunks");
        assert_eq!(&*window, &bytes[10..3 * C as usize - 10]);
        // 12,288 bytes in answers of at most 1,000 are thirteen reads, where
        // the three chunks read one at a time are fifteen.
        assert_eq!(source.reads(), 13);
        assert_eq!(source.bytes_read(), 3 * C);
        assert_eq!(backing.retained(), 3 * C, "three whole chunks");

        let small = file(2 * C as usize + 7);
        let shredded = Backing::chunked(Arc::new(ShreddedSource::new(SliceSource::new(
            Arc::clone(&small),
        ))));
        let all = 0..small.len() as u64;
        assert_eq!(&*shredded.window(all).expect("a byte at a time"), &*small);
        assert_eq!(shredded.retained(), small.len() as u64);
    }

    /// A source with the bytes before `upto` and none from there on, as a
    /// host has that has fetched a prefix; `upto` moves as more arrives.
    struct Prefix {
        bytes: Arc<[u8]>,
        upto: std::sync::atomic::AtomicU64,
    }

    impl ByteSource for Prefix {
        fn len(&self) -> u64 {
            self.bytes.len() as u64
        }

        fn read(&self, range: Range<u64>) -> Result<Arc<[u8]>, SourceMiss> {
            let upto = self.upto.load(Ordering::SeqCst);
            if range.start >= upto && range.start < self.len() {
                return Err(SourceMiss::at(range));
            }
            let end = range.end.min(upto);
            Ok(Arc::from(clamped(&self.bytes, &(range.start..end))))
        }
    }

    /// A miss part-way through a run keeps the chunks that arrived whole
    /// before it, drops the one it stopped in, and names the rest of the run;
    /// the retry reads from the dropped chunk's start.
    #[test]
    fn a_miss_inside_a_run_publishes_only_whole_chunks() {
        const C: u64 = CHUNK_SIZE;
        let bytes = file(5 * C as usize);
        let prefix = Arc::new(Prefix {
            bytes: Arc::clone(&bytes),
            upto: std::sync::atomic::AtomicU64::new(2 * C + 100),
        });
        let source = Arc::new(CountingSource::new(Arc::clone(&prefix)));
        let backing = Backing::chunked(source.clone());

        let miss = backing.window(0..4 * C).expect_err("chunk 2 stops short");
        assert_eq!(miss.needed, 2 * C + 100..4 * C, "the rest of the run");
        assert_eq!(backing.retained(), 2 * C, "chunks 0 and 1, and none of 2");

        prefix.upto.store(bytes.len() as u64, Ordering::SeqCst);
        source.reset();
        let window = backing.window(0..4 * C).expect("every byte has arrived");
        assert_eq!(&*window, &bytes[..4 * C as usize]);
        assert_eq!(
            source.ranges(),
            vec![2 * C..4 * C],
            "from the dropped chunk's start"
        );
        assert_eq!(backing.retained(), 4 * C);
    }
}
