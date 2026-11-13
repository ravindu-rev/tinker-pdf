//! The document cache (`docs/design/performance.md`, P-16): what a document
//! keeps across its renders when its caller states a budget for it.
//!
//! Feature documentation: `docs/features/rendering.md` ("The document's
//! cache") and `docs/features/opening.md` (`CacheLimits`).
//!
//! # What it holds
//!
//! One cache a document, behind the `Arc` every clone of the `Document` and
//! every page it hands out share, of typed entries each charged its bytes. A
//! kind is a map from its key to its value, and every value is a function of
//! its key and the document's immutable bytes: a decoded image of one image
//! XObject at one reduction, in the default colour spaces its scope paints
//! device spaces in (8.6.5.6), which are the one thing outside the object a
//! decode reads; a JPEG's or a JPEG 2000 codestream's frame, which a draw's
//! size is read against for the reduction it is decoded at; the profile one
//! ICC stream holds, and the compiled transforms of one profile's bytes,
//! wherever they are embedded; the recorded drawing of one form under
//! everything its drawing
//! reads; a glyph's mask under P-06's key and the anti-aliasing a render
//! rasterises it with; a tiling pattern's cell under everything its drawing
//! reads. So **a hit is the bytes a miss computes**, and a page draws the same
//! pixels whatever was drawn before it.
//!
//! A refusal is kept as firmly as a success, in the words the decode used, and
//! every entry carries what its computation reported, which every request
//! that meets it reports again.
//!
//! # The budget, and the one order everything leaves in
//!
//! The entries are kept in a [`tinker_pdf_cos::Accountant`]'s ledger, and a
//! budget stated at open charges the document's parsed objects to the same
//! one ([`DocumentCache::accountant`], `docs/design/performance.md`, "Parsed
//! objects under pressure"). Every entry is charged its payload and
//! [`ENTRY_BYTES`] beside it, and the bytes held never pass the budget: a
//! charge is taken under the ledger's one lock, before the entry is
//! published. Every request — a hit or a miss, of any kind, and every read of
//! a parsed object the ledger keeps — takes the next number of the ledger's
//! one counter, and the holding it reaches records it; a fill that would pass
//! the budget evicts what is held in the order of the numbers recorded,
//! oldest first, through one ordered index across every kind and the parsed
//! objects, until the new entry fits — passing over an object a caller holds.
//! An entry larger than the whole budget is never kept: it is handed to the
//! request that made it alone, as is one that what no caller holds cannot
//! make room for. The maps are only ever looked up, never iterated, since
//! their order is seeded per process; which holdings leave is decided by the
//! index alone, so on one thread the evictions are a function of the requests
//! and the same on every run and every target.
//!
//! # Filled single-flight
//!
//! A miss fills by `docs/design/parallelism.md`'s rules, which
//! `tinker_pdf_cos::Flights` keeps for the maps it holds and this table keeps
//! under the ledger's lock: the first request for a key fills it while
//! requests for it on other threads wait for the value; a thread already
//! inside a fill — of this table or of any `Flights`, which
//! [`tinker_pdf_cos::fill_open`] says — computes for itself and publishes
//! nothing, so no thread waits on a thread that waits; and a fill that unwinds
//! publishes nothing and wakes its waiters, the first of whom fills in turn.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Condvar, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};

use tinker_pdf_cos::{
    fill_open, limits, Accountant, Counter, Dict, Entries, Ledger, ObjRef, Object, OpenFill, Work,
};

use tinker_pdf_color::icc::{Conversion, Transforms};
use tinker_pdf_content::{FormInvocation, FormRecording};
use tinker_pdf_raster::fill::Mask;

use crate::resources::{Asked, CachedImage, Frame};
use crate::stats;

/// A document's cache budget: the most bytes what it keeps across its renders
/// holds at once (`docs/design/performance.md`, P-16).
///
/// Stated on [`crate::OpenOptions::cache_limits`] or with
/// [`crate::Document::with_cache_limits`]. **With none stated, a document
/// keeps nothing past the render that made it**, as it always has: a render's
/// own resources hold what they decode and go with it. With one stated, the
/// document keeps what its renders compute — decoded images, compiled ICC
/// transforms, forms' recorded drawings, glyph masks and tiling patterns'
/// cells — across renders, pages and clones of it, while the bytes fit,
/// evicting the least recently requested first. **A budget stated at open
/// charges the document's parsed objects to it too**, in the same order:
/// past the budget an object no caller holds is dropped and parsed again, to
/// an equal value, when it is next read. A budget stated on an open document
/// holds its cache alone,
/// since the parsed objects are the open's, shared by every document handed
/// back from it. A budget of zero is stated and keeps nothing at all: every
/// request computes again.
///
/// The budget bounds what the document keeps, not what a render needs to run —
/// its canvas, the page's content, a decoder's working set — and an entry or
/// an object a render is drawing from is its own copy until it lets it go.
///
/// **An image the budget cannot hold at the size it is drawn is drawn
/// smaller**: a JPEG or a JPEG 2000 image with no mask, whose decode at that
/// size would not fit the budget beside what its entry is charged, is decoded
/// at the least reduction past that size whose decode fits — a JPEG at a
/// half, a quarter or an eighth, a codestream with resolution levels dropped
/// — and the render names it, [`crate::RenderWarning::ReducedImage`]. Which
/// reduction is taken is a function of the image, its drawn size and the
/// budget, never of what the document holds. An image no reduction fits is
/// decoded as with no budget, and not kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CacheLimits {
    /// The most bytes the document keeps at once, each entry and each parsed
    /// object charged its payload and 64 bytes beside it.
    pub bytes: u64,
}

impl CacheLimits {
    /// A budget of `bytes`.
    #[must_use]
    pub const fn new(bytes: u64) -> CacheLimits {
        CacheLimits { bytes }
    }
}

/// What every entry is charged beside its payload: its key, its bookkeeping
/// and its entry in the eviction index, so that a cache of empty entries is
/// bounded too. P-06's glyph-mask cache charges a mask the same, and the
/// object store an object.
pub(crate) const ENTRY_BYTES: u64 = tinker_pdf_cos::accountant::ENTRY_BYTES;

/// What a fill answered.
pub(crate) enum Fill<V> {
    /// A value to keep, its payload `bytes` charged beside [`ENTRY_BYTES`]:
    /// every later request for the key is handed it, while it fits.
    Keep(V, u64),
    /// A value for this request alone: nothing is kept, and the next request
    /// for the key computes it again — a cell a cancelled render drew, which
    /// is not the cell its key names.
    Once(V),
}

/// A document's cache. See the module documentation.
pub(crate) struct DocumentCache {
    limits: CacheLimits,
    /// The ledger the entries are kept in and charged to, beside the parsed
    /// objects of a document whose budget was stated at open.
    accountant: Arc<Accountant<Table>>,
    /// Signalled whenever a slot is published or released.
    settled: Condvar,
}

/// The cache's entries, kept in the ledger behind its one lock.
#[derive(Default)]
pub(crate) struct Table {
    /// Decoded images.
    images: HashMap<ImageKey, Slot<Arc<CachedImage>>>,
    /// The frames of the images a draw's size reduces.
    frames: HashMap<FrameKey, Slot<Option<Frame>>>,
    /// The profile each ICC stream holds, and the streams that would not
    /// decode.
    profiles: HashMap<ProfileKey, Slot<Option<IccKey>>>,
    /// Compiled ICC transforms, by the profile's bytes, and the profiles that
    /// would not compile.
    icc: HashMap<IccKey, Slot<Option<Arc<Transforms>>>>,
    /// Forms' drawings, recorded.
    forms: HashMap<FormKey, Slot<Arc<KeptForm>>>,
    /// Glyph masks.
    masks: HashMap<MaskKey, Slot<Arc<Mask>>>,
    /// Tiling patterns' cells, and the patterns whose cell would not draw.
    cells: HashMap<CellKey, Slot<Option<Arc<KeptCell>>>>,
    /// A kind of plain values, for the tests of the machinery itself.
    #[cfg(test)]
    tests: HashMap<u32, Slot<Arc<Vec<u8>>>>,
}

impl Entries for Table {
    type Key = Key;

    /// Evicts the published entry under `key`, handing back what it was
    /// charged.
    fn evict(&mut self, key: &Key) -> u64 {
        match key {
            Key::Image(key) => remove_ready(&mut self.images, key),
            Key::Frame(key) => remove_ready(&mut self.frames, key),
            Key::Profile(key) => remove_ready(&mut self.profiles, key),
            Key::Icc(key) => remove_ready(&mut self.icc, key),
            Key::Form(key) => remove_ready(&mut self.forms, &**key),
            Key::Mask(key) => remove_ready(&mut self.masks, key),
            Key::Cell(key) => remove_ready(&mut self.cells, &**key),
            #[cfg(test)]
            Key::Test(key) => remove_ready(&mut self.tests, key),
        }
    }

    fn clear(&mut self) {
        *self = Table::default();
    }
}

/// Removes `key`'s published entry from `slots`, handing back its charge. A
/// slot being filled is never in the eviction index and is left alone.
fn remove_ready<K: Eq + Hash, V>(slots: &mut HashMap<K, Slot<V>>, key: &K) -> u64 {
    match slots.get(key) {
        Some(Slot::Ready { charge, .. }) => {
            let charge = *charge;
            slots.remove(key);
            charge
        }
        _ => 0,
    }
}

/// One key's slot.
enum Slot<V> {
    /// The thread that claimed the slot is filling it.
    Filling(ThreadId),
    /// The published value, the number of the request that last asked for
    /// it, and what it is charged.
    Ready { value: V, asked: u64, charge: u64 },
}

/// An entry's key, of whichever kind: what the eviction index names.
#[derive(Clone, Debug)]
pub(crate) enum Key {
    Image(ImageKey),
    Frame(FrameKey),
    Profile(ProfileKey),
    Icc(IccKey),
    Form(Box<FormKey>),
    Mask(MaskKey),
    Cell(Box<CellKey>),
    #[cfg(test)]
    Test(u32),
}

/// One kind of entry: its key type, its value and its map in the table.
trait Kind: Clone + Eq + Hash {
    type Value: Clone;
    /// This kind's map.
    fn slots(table: &mut Table) -> &mut HashMap<Self, Slot<Self::Value>>;
    /// This key as the eviction index names it.
    fn key(&self) -> Key;
}

impl Kind for ImageKey {
    type Value = Arc<CachedImage>;

    fn slots(table: &mut Table) -> &mut HashMap<Self, Slot<Self::Value>> {
        &mut table.images
    }

    fn key(&self) -> Key {
        Key::Image(self.clone())
    }
}

impl Kind for FrameKey {
    type Value = Option<Frame>;

    fn slots(table: &mut Table) -> &mut HashMap<Self, Slot<Self::Value>> {
        &mut table.frames
    }

    fn key(&self) -> Key {
        Key::Frame(self.clone())
    }
}

impl Kind for ProfileKey {
    type Value = Option<IccKey>;

    fn slots(table: &mut Table) -> &mut HashMap<Self, Slot<Self::Value>> {
        &mut table.profiles
    }

    fn key(&self) -> Key {
        Key::Profile(*self)
    }
}

impl Kind for IccKey {
    type Value = Option<Arc<Transforms>>;

    fn slots(table: &mut Table) -> &mut HashMap<Self, Slot<Self::Value>> {
        &mut table.icc
    }

    fn key(&self) -> Key {
        Key::Icc(self.clone())
    }
}

impl Kind for FormKey {
    type Value = Arc<KeptForm>;

    fn slots(table: &mut Table) -> &mut HashMap<Self, Slot<Self::Value>> {
        &mut table.forms
    }

    fn key(&self) -> Key {
        Key::Form(Box::new(self.clone()))
    }
}

impl Kind for MaskKey {
    type Value = Arc<Mask>;

    fn slots(table: &mut Table) -> &mut HashMap<Self, Slot<Self::Value>> {
        &mut table.masks
    }

    fn key(&self) -> Key {
        Key::Mask(self.clone())
    }
}

impl Kind for CellKey {
    type Value = Option<Arc<KeptCell>>;

    fn slots(table: &mut Table) -> &mut HashMap<Self, Slot<Self::Value>> {
        &mut table.cells
    }

    fn key(&self) -> Key {
        Key::Cell(Box::new(self.clone()))
    }
}

#[cfg(test)]
impl Kind for u32 {
    type Value = Arc<Vec<u8>>;

    fn slots(table: &mut Table) -> &mut HashMap<Self, Slot<Self::Value>> {
        &mut table.tests
    }

    fn key(&self) -> Key {
        Key::Test(*self)
    }
}

/// What a request found in its slot, read out before the table is changed.
enum Found<V> {
    /// Published: the value, and the number of the request that asked last.
    Ready(V, u64),
    /// Being filled by this thread.
    Filling(ThreadId),
    /// Nothing.
    Empty,
}

impl DocumentCache {
    /// An empty cache under `limits`, keeping its entries in a ledger of its
    /// own.
    pub(crate) fn new(limits: CacheLimits) -> DocumentCache {
        DocumentCache {
            limits,
            accountant: Arc::new(Accountant::new(limits.bytes, Table::default())),
            settled: Condvar::new(),
        }
    }

    /// The budget this cache holds its entries to.
    pub(crate) fn limits(&self) -> CacheLimits {
        self.limits
    }

    /// The ledger this cache keeps its entries in, for the document's object
    /// store to charge beside it where its budget is stated at open
    /// (`tinker_pdf_cos::CosDocument::charge_objects_to`).
    pub(crate) fn accountant(&self) -> Arc<Accountant<Table>> {
        Arc::clone(&self.accountant)
    }

    /// The ledger, under its one lock.
    fn ledger(&self) -> MutexGuard<'_, Ledger<Table>> {
        self.accountant.lock()
    }

    /// The decoded image under `key`, and whether the request found it kept:
    /// the published entry, or what `fill` answers, filled single-flight. See
    /// the module documentation.
    pub(crate) fn image(
        &self,
        key: ImageKey,
        work: Option<&Work>,
        fill: impl FnOnce() -> Fill<Arc<CachedImage>>,
    ) -> (Arc<CachedImage>, bool) {
        self.read(key, work, fill)
    }

    /// The frame under `key` — what the image's first decode found it to be
    /// (`resources::Frame`): a JPEG's own size, or a JPEG 2000 codestream's
    /// and the levels it can drop; `None` where the decode was refused or the
    /// image is one no draw reduces — and whether the request found it kept,
    /// filled as [`DocumentCache::image`] fills.
    pub(crate) fn frame(
        &self,
        key: FrameKey,
        work: Option<&Work>,
        fill: impl FnOnce() -> Fill<Option<Frame>>,
    ) -> (Option<Frame>, bool) {
        self.read(key, work, fill)
    }

    /// The profile the ICC stream under `key` holds, as the transforms of it
    /// are kept under ([`IccKey`]), or `None` where the stream would not
    /// decode, and whether the request found it kept, filled as
    /// [`DocumentCache::image`] fills an image: so a stream is decoded and
    /// its bytes digested once a document while it is kept, and a stream that
    /// will not decode is refused once.
    pub(crate) fn profile(
        &self,
        key: ProfileKey,
        work: Option<&Work>,
        fill: impl FnOnce() -> Fill<Option<IccKey>>,
    ) -> (Option<IccKey>, bool) {
        self.read(key, work, fill)
    }

    /// The compiled transforms of the ICC profile under `key`, or the
    /// profile's refusal, and whether the request found it kept, filled as
    /// [`DocumentCache::image`] fills an image.
    pub(crate) fn icc(
        &self,
        key: IccKey,
        work: Option<&Work>,
        fill: impl FnOnce() -> Fill<Option<Arc<Transforms>>>,
    ) -> (Option<Arc<Transforms>>, bool) {
        self.read(key, work, fill)
    }

    /// The cell of a tiling pattern kept under `key` — or `None` where the
    /// pattern's cell would not draw — and whether the request found it kept,
    /// filled as [`DocumentCache::image`] fills an image: single-flight, since
    /// a cell is a function of its key alone, so renders on several threads
    /// that reach one cell at once draw it once.
    pub(crate) fn cell(
        &self,
        key: CellKey,
        work: Option<&Work>,
        fill: impl FnOnce() -> Fill<Option<Arc<KeptCell>>>,
    ) -> (Option<Arc<KeptCell>>, bool) {
        self.read(key, work, fill)
    }

    /// The recording of a form's drawing kept under `key`, or, where none is,
    /// the number of this request, for [`DocumentCache::offer_form`] to keep
    /// the recording the interpretation makes under.
    ///
    /// **Not single-flight**: a form's recording is made by the render that
    /// draws the form, as it draws it, so a render never waits for another
    /// thread's; two that miss one key at once each interpret it, and the
    /// first to offer its recording keeps it.
    pub(crate) fn form(&self, key: &FormKey, work: Option<&Work>) -> Result<Arc<KeptForm>, u64> {
        self.get(key, work)
    }

    /// Keeps `value` under `key` for the request numbered `asked` that missed
    /// it, charged `bytes` beside [`ENTRY_BYTES`], where nothing is kept
    /// under the key already.
    pub(crate) fn offer_form(
        &self,
        key: FormKey,
        value: Arc<KeptForm>,
        bytes: u64,
        asked: u64,
        work: Option<&Work>,
    ) {
        self.offer(key, value, bytes, asked, work);
    }

    /// The glyph mask kept under `key`, or the number of the request that
    /// missed it, for [`DocumentCache::offer_mask`]; filled as a form's
    /// drawing is, not single-flight, since a mask is rasterised by the render
    /// that draws the glyph, as it draws it.
    pub(crate) fn mask(&self, key: &MaskKey, work: Option<&Work>) -> Result<Arc<Mask>, u64> {
        self.get(key, work)
    }

    /// Keeps `mask` under `key` for the request numbered `asked` that missed
    /// it, charged `bytes` beside [`ENTRY_BYTES`], where nothing is kept
    /// under the key already.
    pub(crate) fn offer_mask(
        &self,
        key: MaskKey,
        mask: Arc<Mask>,
        bytes: u64,
        asked: u64,
        work: Option<&Work>,
    ) {
        self.offer(key, mask, bytes, asked, work);
    }

    /// The value published under `key`, the request counted and the entry
    /// marked as asked for by it; or, where there is none, the request's
    /// number, the request counted as a miss.
    fn get<K: Kind>(&self, key: &K, work: Option<&Work>) -> Result<K::Value, u64> {
        let mut ledger = self.ledger();
        let asked = ledger.request();
        let found = match K::slots(ledger.entries()).get_mut(key) {
            Some(Slot::Ready {
                value, asked: last, ..
            }) => {
                let previous = *last;
                *last = asked;
                Some((value.clone(), previous))
            }
            _ => None,
        };
        let held = ledger.entries_held();
        let found = match found {
            Some((value, previous)) => {
                ledger.reorder(previous, asked);
                Ok(value)
            }
            None => Err(asked),
        };
        drop(ledger);
        stats::count(
            work,
            if found.is_ok() {
                Counter::CacheHits
            } else {
                Counter::CacheMisses
            },
        );
        stats::cache_held(work, held);
        found
    }

    /// Publishes `value` under `key` for the request numbered `asked`, as a
    /// fill publishes it, where the key holds nothing yet. The key is looked
    /// at under the lock the value is published under, so two renders that
    /// offer one key at once keep the first value, charged once.
    fn offer<K: Kind>(&self, key: K, value: K::Value, bytes: u64, asked: u64, work: Option<&Work>) {
        let mut ledger = self.ledger();
        if K::slots(ledger.entries()).contains_key(&key) {
            return;
        }
        let (evicted, held) = publish_in(&mut ledger, key, value, bytes, asked);
        drop(ledger);
        if let Some(work) = work {
            work.add(Counter::CacheEvictions, evicted);
        }
        stats::cache_held(work, held);
    }

    /// The value under `key` and whether it was found kept: the published
    /// value, or what `fill` answers — computed by this request when no
    /// other thread is filling the key, or when one is and this thread has a
    /// fill of its own open, and otherwise waited for.
    fn read<K: Kind>(
        &self,
        key: K,
        work: Option<&Work>,
        fill: impl FnOnce() -> Fill<K::Value>,
    ) -> (K::Value, bool) {
        let me = thread::current().id();
        let mut ledger = self.ledger();
        loop {
            let found = match K::slots(ledger.entries()).get(&key) {
                Some(Slot::Ready { value, asked, .. }) => Found::Ready(value.clone(), *asked),
                Some(Slot::Filling(owner)) => Found::Filling(*owner),
                None => Found::Empty,
            };
            match found {
                Found::Ready(value, last) => {
                    let asked = ledger.request();
                    if let Some(Slot::Ready { asked: slot, .. }) =
                        K::slots(ledger.entries()).get_mut(&key)
                    {
                        *slot = asked;
                    }
                    ledger.reorder(last, asked);
                    let held = ledger.entries_held();
                    drop(ledger);
                    stats::count(work, Counter::CacheHits);
                    stats::cache_held(work, held);
                    return (value, true);
                }
                Found::Filling(owner) => {
                    // Rules 3 and 4: a thread with a fill open, or the one
                    // filling this key, computes for itself and publishes
                    // nothing, so that it never waits on anyone.
                    if owner == me || fill_open() {
                        ledger.request();
                        let held = ledger.entries_held();
                        drop(ledger);
                        stats::count(work, Counter::CacheMisses);
                        stats::cache_held(work, held);
                        let _open = OpenFill::enter();
                        let value = match fill() {
                            Fill::Keep(value, _) | Fill::Once(value) => value,
                        };
                        return (value, false);
                    }
                    ledger = self
                        .settled
                        .wait(ledger)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                Found::Empty => {
                    let asked = ledger.request();
                    K::slots(ledger.entries()).insert(key.clone(), Slot::Filling(me));
                    drop(ledger);
                    stats::count(work, Counter::CacheMisses);
                    let claim = Claim {
                        cache: self,
                        key: Some(key),
                        _open: OpenFill::enter(),
                    };
                    return (claim.fill(asked, work, fill), false);
                }
            }
        }
    }

    /// The bytes the published entries are charged, together.
    #[cfg(test)]
    fn held(&self) -> u64 {
        self.ledger().entries_held()
    }

    /// Whether `key` has a published entry, without requesting it.
    #[cfg(test)]
    fn holds<K: Kind>(&self, key: &K) -> bool {
        matches!(
            K::slots(self.ledger().entries()).get(key),
            Some(Slot::Ready { .. })
        )
    }
}

impl Drop for DocumentCache {
    /// A cache let go of leaves nothing in its ledger: a document whose
    /// budget was stated at open keeps its parsed objects there after its
    /// cache is replaced (`crate::Document::with_fonts`), and the entries of a
    /// cache no document holds would be charged to it for nothing.
    fn drop(&mut self) {
        self.ledger().forget_entries();
    }
}

/// Publishes `value` under `key` for the request numbered `asked`, charged
/// `bytes` beside [`ENTRY_BYTES`], evicting what is held least recently
/// requested first until it fits; or, for a value larger than the whole
/// budget — or one that what no caller holds cannot make room for — releases
/// the slot and keeps nothing. Hands back how many of the cache's entries
/// left for it and the bytes its entries are charged after.
fn publish_in<K: Kind>(
    ledger: &mut Ledger<Table>,
    key: K,
    value: K::Value,
    bytes: u64,
    asked: u64,
) -> (u64, u64) {
    let charge = bytes.saturating_add(ENTRY_BYTES);
    let admitted = ledger.admit(key.key(), charge, asked);
    if admitted.kept {
        K::slots(ledger.entries()).insert(
            key,
            Slot::Ready {
                value,
                asked,
                charge,
            },
        );
    } else {
        K::slots(ledger.entries()).remove(&key);
    }
    (admitted.entries_evicted, ledger.entries_held())
}

/// A slot this thread has claimed: published or released when its fill
/// answers, and released, its waiters woken, when the fill unwinds.
struct Claim<'a, K: Kind> {
    cache: &'a DocumentCache,
    /// The key claimed, until it is published or released.
    key: Option<K>,
    _open: OpenFill,
}

impl<K: Kind> Claim<'_, K> {
    /// Runs the fill and settles the slot by what it answered.
    fn fill(
        mut self,
        asked: u64,
        work: Option<&Work>,
        fill: impl FnOnce() -> Fill<K::Value>,
    ) -> K::Value {
        let filled = fill();
        let Some(key) = self.key.take() else {
            // Unreachable: the key is taken here and nowhere else.
            return match filled {
                Fill::Keep(value, _) | Fill::Once(value) => value,
            };
        };
        let (value, evicted, held) = match filled {
            Fill::Keep(value, bytes) => {
                let (evicted, held) =
                    publish_in(&mut self.cache.ledger(), key, value.clone(), bytes, asked);
                (value, evicted, held)
            }
            Fill::Once(value) => {
                let mut ledger = self.cache.ledger();
                K::slots(ledger.entries()).remove(&key);
                (value, 0, ledger.entries_held())
            }
        };
        self.cache.settled.notify_all();
        if let Some(work) = work {
            work.add(Counter::CacheEvictions, evicted);
        }
        stats::cache_held(work, held);
        value
    }
}

impl<K: Kind> Drop for Claim<'_, K> {
    /// A fill that unwound: its slot is released, and its waiters woken to
    /// fill it in turn.
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            K::slots(self.cache.ledger().entries()).remove(&key);
            self.cache.settled.notify_all();
        }
    }
}

// What a decoded image is kept under (`docs/design/performance.md`, P-16):
/// the image XObject, what its decode is asked for, the conversion its
/// colours take, and the default colour spaces its scope paints device
/// spaces in (8.6.5.6) — the two things outside the object an image's
/// decode reads, so two scopes that name one image and different defaults
/// keep two entries, and two that name it and the same defaults share one,
/// and one image drawn under two rendering intents its colours take is two
/// decodes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ImageKey {
    /// The image XObject.
    pub(crate) object: ObjRef,
    /// What the draw asked the decode for (`resources::Asked`): how many of
    /// the image's own samples each way one decoded sample stands for — 1 at
    /// full size, 2, 4 or 8 for a JPEG decoded at a half, a quarter or an
    /// eighth, and `2^d` for a JPEG 2000 codestream decoded with `d` levels
    /// dropped (`tinker_pdf_render::Reduced::factor`) — and the rectangle of
    /// the decoded image it holds, where a codestream's decode holds part of
    /// it (`tinker_pdf_render::DecodedImage::window`). A reduction the
    /// budget took a draw to past what its size asks is keyed as the same
    /// reduction asked by a draw's size, since it is the same decode: a hit
    /// is the bytes that request's miss computes, and a request for one
    /// reduction is never served another.
    pub(crate) asked: Asked,
    /// The conversion the image's colours take, its own `/Intent`'s or the
    /// one in force at the draw (8.6.5.8, Table 89), where an intent changes
    /// them — an ICC space, or a space holding one, as painted; `None` for
    /// an image no intent changes, which is one decode under every intent.
    pub(crate) conversion: Option<Conversion>,
    /// The scope's `/DefaultGray`, `/DefaultRGB` and `/DefaultCMYK` that it
    /// puts in, each as written ([`Written`]); empty for a scope that paints
    /// every device space as itself.
    pub(crate) painted: Written,
}

/// What the frame of an image a draw's size reduces — a JPEG or a JPEG 2000
/// codestream with no mask, under no `/Indexed` space —
/// is kept under: the image XObject and its scope's defaults, as its decodes
/// are. The frame is what every draw of the image reads its reduction from,
/// learnt by the image's first decode.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FrameKey {
    /// The image XObject.
    pub(crate) object: ObjRef,
    /// The scope's defaults, as [`ImageKey::painted`].
    pub(crate) painted: Written,
}

/// What the profile one ICC stream holds is kept under: the stream's object,
/// which a scope's question about an `ICCBased` space names. Its value is the
/// profile's key among the transforms ([`IccKey`]), which holds the bytes,
/// so a stream named on many pages, forms and renders is decoded and its bytes
/// digested once while it is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProfileKey(pub(crate) ObjRef);

/// What a compiled ICC transform is kept under (`docs/design/icc.md`, P-22,
/// "The document cache: its key and its charge"): the profile's bytes, their
/// SHA-256 standing for them in the map and the bytes themselves compared in
/// full on a hit — so one profile embedded as many objects is one entry, and
/// two profiles are two whatever their digests — and the destination the
/// transforms convert toward. The conversion a transform was compiled for —
/// its rendering intent, and black-point compensation where RC-02 adds it —
/// is the kept `Transforms`' own key within the entry, so a transform's whole
/// key is the profile's bytes, the destination and the conversion. A profile
/// that will not compile is refused under its bytes, once a document.
#[derive(Clone, Debug)]
pub(crate) struct IccKey {
    digest: [u8; 32],
    profile: Arc<Vec<u8>>,
    destination: Destination,
}

/// The colour space a profile's transforms convert toward: sRGB, the only
/// one this engine converts to; a destination profile (RC-06) is a value of
/// its own, keyed by its bytes as a source is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Destination {
    /// IEC 61966-2-1's sRGB.
    Srgb,
}

impl IccKey {
    /// The key of the transforms of `profile`'s bytes toward sRGB.
    pub(crate) fn srgb(profile: Arc<Vec<u8>>) -> IccKey {
        IccKey {
            digest: tinker_pdf_crypto::sha2::sha256(&profile),
            profile,
            destination: Destination::Srgb,
        }
    }

    /// The profile's bytes, shared: what the transforms compiled from them
    /// read and keep as their source (`Transforms::compile_shared`), with no
    /// copy.
    pub(crate) fn shared(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.profile)
    }

    /// The bytes the key holds beside its own fields: the profile's.
    pub(crate) fn held(&self) -> u64 {
        u64::try_from(self.profile.len()).unwrap_or(u64::MAX)
    }
}

impl PartialEq for IccKey {
    fn eq(&self, other: &IccKey) -> bool {
        self.destination == other.destination
            && self.digest == other.digest
            && (Arc::ptr_eq(&self.profile, &other.profile) || self.profile == other.profile)
    }
}

impl Eq for IccKey {}

impl Hash for IccKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.digest.hash(state);
        self.destination.hash(state);
    }
}

/// What a form's recorded drawing is kept under: the interpreter's
/// [`FormInvocation`] — the form and everything of the interpretation its
/// drawing can read — and, for a form that brings no resources of its own,
/// the scope its names resolve in, its resource dictionary written exactly
/// ([`Written`]), so two scopes that hold the same resources share the
/// drawing and two that do not keep two; and the usage, the layer options
/// and, where those states read a `/Zoom` entry, the magnification the render
/// names, whose optional-content states its drawing's marked content and the
/// forms inside it are shown or hidden by (8.11.4.4, 8.11.4.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FormKey {
    /// The invocation, as the interpreter wrote it.
    pub(crate) invocation: FormInvocation,
    /// The invoking scope's resources, where the form resolves its names in
    /// them; `None` for a form with resources of its own.
    pub(crate) scope: Option<Written>,
    /// The usage the render names (`crate::RenderOptions::usage`).
    pub(crate) usage: Option<crate::Usage>,
    /// The configuration the render names (`crate::RenderOptions::layers`).
    pub(crate) layers: crate::LayerOptions,
    /// The render's magnification, as its bits, where its states read a
    /// `/Zoom` entry (`crate::RenderOptions::magnification`); `None` where
    /// they read none.
    pub(crate) magnification: Option<u64>,
}

/// A form's recorded drawing as the document keeps it: the recording, and the
/// fonts the interpretation found no font for in the scope it was invoked in,
/// which every render that replays it reports again, as its own
/// interpretation would have.
pub(crate) struct KeptForm {
    pub(crate) recording: Arc<FormRecording>,
    pub(crate) missing_fonts: Vec<String>,
}

/// What a tiling pattern's cell is kept under (P-23.2): the pattern's
/// object; everything of the request the cell's pixels are a function of —
/// `TileRequest::to_pixels`'s six numbers by their bits, so the device matrix
/// and the sub-pixel phase are unquantised and a hit is the bytes a miss
/// draws, the `/BBox` the cell is clipped to by its bits, the buffer's size
/// and format, the anti-aliasing, the graphics and text levels, the rule at
/// level 0 and the thinnest stroke the cell is drawn with, the nesting
/// depth, which the recursion cap reads, and on plates the plan a coloured
/// cell paints its colorants by and the plates held live outside it; for a
/// pattern that brings no
/// resources of its own, the scope its names resolve in, its resource
/// dictionary written exactly ([`Written`]); and the usage, the layer options
/// and, where those states read a `/Zoom` entry, the magnification the render
/// names, whose optional-content states the cell's content is shown or hidden
/// by (8.11.4.4, 8.11.4.3), as a form's kept drawing is keyed
/// ([`FormKey`]). A `PaintType 2` cell is drawn as its content draws it, a
/// shape whose colour the fill takes at the blit, so one cell serves every
/// colour it is filled in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CellKey {
    /// The pattern's stream.
    pub(crate) pattern: ObjRef,
    /// Pattern space to the cell's buffer, `a` to `f`, by their bits.
    pub(crate) to_pixels: [u64; 6],
    /// The `/BBox` in pattern space, by its bits.
    pub(crate) bbox: [u64; 4],
    /// The buffer's width and height.
    pub(crate) size: (u32, u32),
    /// The buffer's format.
    pub(crate) format: tinker_pdf_raster::canvas::PixelFormat,
    /// Whether the cell's edges are anti-aliased.
    pub(crate) antialias: bool,
    /// The graphics and the text level.
    pub(crate) levels: (u8, u8),
    /// The rule a pixel is whole or empty by at level 0.
    pub(crate) hard_edge: tinker_pdf_raster::HardEdge,
    /// The thinnest stroke, in device pixels, by its bits.
    pub(crate) thinnest_line: Option<u64>,
    /// How many tiling patterns are open above the cell.
    pub(crate) depth: u32,
    /// The invoking scope's resources, where the pattern resolves its names
    /// in them; `None` for a pattern with resources of its own.
    pub(crate) scope: Option<Written>,
    /// The usage the render names (`crate::RenderOptions::usage`).
    pub(crate) usage: Option<crate::Usage>,
    /// The layer options the render names (`crate::RenderOptions::layers`):
    /// its configuration, overrides, language and user.
    pub(crate) layers: crate::LayerOptions,
    /// The render's magnification, as its bits, where its states read a
    /// `/Zoom` entry (`crate::RenderOptions::magnification`); `None` where
    /// they read none, so cells kept at one magnification serve every other.
    pub(crate) magnification: Option<u64>,
    /// The plate plan a coloured cell drawn on plates paints its colorants by
    /// (`TileRequest::plates`), whose names and uses say which plate each
    /// paint lands on; `None` for every other cell.
    pub(crate) plates: Option<Arc<tinker_pdf_render::PlatePlan>>,
    /// The bytes of plates held live outside a plates cell
    /// (`TileRequest::plate_bytes`), which decide whether a group inside it
    /// composites on plates or in ink.
    pub(crate) plate_bytes: u64,
    /// Whether the cell's paints overprint (`TileRequest::overprint`), which
    /// decides which channels each of them leaves.
    pub(crate) overprint: bool,
}

impl CellKey {
    /// The bytes the key holds beside its own fields: a scope's resources,
    /// written, what its layer options hold — the overrides, the language
    /// and the user's names — and a plate plan's names and tables.
    pub(crate) fn held(&self) -> u64 {
        let scope = self
            .scope
            .as_ref()
            .map_or(0, |scope| u64::try_from(scope.len()).unwrap_or(u64::MAX));
        let plates = self.plates.as_ref().map_or(0, |plan| plan.held_bytes());
        scope
            .saturating_add(layers_held(&self.layers))
            .saturating_add(plates)
    }
}

/// The bytes `layers` holds beside its own fields: its overrides, its
/// language and its user's names.
fn layers_held(layers: &crate::LayerOptions) -> u64 {
    let overrides = layers
        .overrides
        .len()
        .saturating_mul(std::mem::size_of::<(ObjRef, bool)>());
    let text = |text: &Option<String>| text.as_ref().map_or(0, String::len);
    let bytes = overrides
        .saturating_add(text(&layers.language))
        .saturating_add(text(&layers.user.name))
        .saturating_add(text(&layers.user.title))
        .saturating_add(text(&layers.user.organisation));
    u64::try_from(bytes).unwrap_or(u64::MAX)
}

/// A tiling pattern's cell as the document keeps it: the cell, and what its
/// drawing told the scope its content ran in — fonts it found none for,
/// images it decoded with damage, colour spaces it read with a repair, images
/// it drew reduced — which every fill that meets it kept reports again, as
/// its own drawing would have, beside the cell's own warnings.
pub(crate) struct KeptCell {
    /// The cell, its pixels shared.
    pub(crate) tile: tinker_pdf_render::Tile,
    /// The fonts the cell's content found none for.
    pub(crate) missing_fonts: Vec<String>,
    /// The images it decoded with damage, and what was tolerated.
    pub(crate) damaged: Vec<(String, String)>,
    /// The colour spaces it read with a repair, and the repair.
    pub(crate) repaired: Vec<(String, String)>,
    /// The images it drew reduced, and the reduction.
    pub(crate) reduced: Vec<(String, u32)>,
}

impl KeptCell {
    /// What the entry is charged beside the cache's own bookkeeping and its
    /// key: the cell's pixels, the words it says again, and its warnings at
    /// 64 bytes each.
    pub(crate) fn bytes(&self) -> u64 {
        let words = self
            .missing_fonts
            .iter()
            .map(String::len)
            .chain(
                self.damaged
                    .iter()
                    .map(|(name, what)| name.len() + what.len()),
            )
            .chain(
                self.repaired
                    .iter()
                    .map(|(name, what)| name.len() + what.len()),
            )
            .chain(self.reduced.iter().map(|(name, _)| name.len() + 4))
            .sum::<usize>();
        let warnings = self.tile.warnings.len().saturating_mul(64);
        u64::try_from(
            self.tile
                .canvas
                .data
                .len()
                .saturating_add(words)
                .saturating_add(warnings),
        )
        .unwrap_or(u64::MAX)
    }
}

/// What a glyph mask is kept under: P-06's key — the font, the code, the
/// quantised 2x2, the phase and the flattening tolerance
/// (`tinker_pdf_render::MaskKey`) — with the font as the document names it
/// across renders ([`MaskFont`]), and the anti-aliasing a render rasterises a
/// mask with, so that a hit is the mask a miss rasterises whichever render
/// asks.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MaskKey {
    /// The font.
    pub(crate) font: MaskFont,
    /// The code the glyph was shown by.
    pub(crate) code: u32,
    /// The quantised em-to-device 2x2.
    pub(crate) matrix: [i32; 4],
    /// The origin's phase on each axis.
    pub(crate) phase: [u8; 2],
    /// The bits of coverage the mask keeps: the render's text level.
    pub(crate) bits: u8,
    /// The rule it is whole or empty by at 0 bits.
    pub(crate) edge: tinker_pdf_raster::HardEdge,
    /// The bits of the tolerance the glyph's outline is flattened to.
    pub(crate) tolerance: u64,
}

impl MaskKey {
    /// The key a render asks for by `asked`, its font named `font`.
    pub(crate) fn of(asked: &tinker_pdf_render::MaskKey, font: MaskFont) -> MaskKey {
        MaskKey {
            font,
            code: asked.code,
            matrix: asked.matrix,
            phase: asked.phase,
            bits: asked.bits,
            edge: asked.edge,
            tolerance: asked.tolerance,
        }
    }

    /// The bytes the key holds beside its own fields: a scoped font's
    /// resources, written.
    pub(crate) fn held(&self) -> u64 {
        match &self.font {
            MaskFont::Object(_) => 0,
            MaskFont::Scoped { scope, .. } => u64::try_from(scope.len()).unwrap_or(u64::MAX),
        }
    }
}

/// A glyph mask's font, as the document's cache keys it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MaskFont {
    /// A font with an object of its own, `num << 16 | gen`: one font wherever
    /// a scope names it.
    Object(u64),
    /// A font written into a scope's own resource dictionary, which has no
    /// object: the name its glyphs carry there, as the document interns it,
    /// and the scope's resources written exactly ([`Written`]), so two scopes
    /// holding the same resources share its masks and two that do not keep
    /// their own.
    Scoped {
        /// The id the glyph's font carries in the scope.
        font_id: u64,
        /// The scope's resources.
        scope: Written,
    },
}

/// Objects written out exactly, as a key holds them ([`write_exact`]), with
/// a digest of the bytes standing in for them in the hash.
///
/// Two keys holding equal bytes are one key; the digest only spares a
/// lookup from hashing all of them.
#[derive(Clone, Debug)]
pub(crate) struct Written {
    digest: u64,
    bytes: Arc<[u8]>,
}

impl Written {
    /// Nothing written: what a key holds for a scope that puts nothing in.
    pub(crate) fn nothing() -> Written {
        Written::of(Vec::new())
    }

    /// `bytes`, as written by [`write_exact`].
    pub(crate) fn of(bytes: Vec<u8>) -> Written {
        // FNV-1a over the bytes: a digest for the hash and nothing more, so it
        // need not resist anything.
        let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in &bytes {
            digest ^= u64::from(*byte);
            digest = digest.wrapping_mul(0x0100_0000_01b3);
        }
        Written {
            digest,
            bytes: Arc::from(bytes),
        }
    }

    /// How many bytes are written.
    pub(crate) fn len(&self) -> usize {
        self.bytes.len()
    }
}

impl Default for Written {
    fn default() -> Written {
        Written::nothing()
    }
}

impl PartialEq for Written {
    fn eq(&self, other: &Written) -> bool {
        Arc::ptr_eq(&self.bytes, &other.bytes)
            || (self.digest == other.digest && self.bytes == other.bytes)
    }
}

impl Eq for Written {}

impl Hash for Written {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.digest.hash(state);
    }
}

/// Appends `object` to `out` so that two objects append the same bytes
/// exactly when they are the same value: a number by its bits, a name by its
/// interned symbol, a string by its bytes, a reference by its number and
/// generation, an array and a dictionary with their lengths first and a
/// dictionary's entries in their order.
///
/// What a reference names is the document's immutable object, so a value
/// written with its references is the whole of what reading it can read. A
/// real is its bits, not its spelling: two reals the writer would round alike
/// read as two numbers, and are two keys.
pub(crate) fn write_exact(object: &Object, out: &mut Vec<u8>) {
    write_exact_at(object, out, 0);
}

/// Appends `dict` to `out` as [`write_exact`] appends a dictionary.
pub(crate) fn write_exact_dict(dict: &Dict, out: &mut Vec<u8>) {
    write_dict_at(dict, out, 0);
}

fn write_dict_at(dict: &Dict, out: &mut Vec<u8>, depth: u32) {
    out.push(7);
    out.extend_from_slice(
        &u64::try_from(dict.entries().len())
            .unwrap_or(u64::MAX)
            .to_le_bytes(),
    );
    for (key, value) in dict.entries() {
        out.extend_from_slice(&key.id().to_le_bytes());
        write_exact_at(value, out, depth + 1);
    }
}

fn write_exact_at(object: &Object, out: &mut Vec<u8>, depth: u32) {
    // The parser nests no deeper than this, so a value written past it is
    // one no document holds; the marker keeps the walk bounded all the same.
    if depth > limits::MAX_NEST_DEPTH {
        out.push(0xFF);
        return;
    }
    let length = |out: &mut Vec<u8>, n: usize| {
        out.extend_from_slice(&u64::try_from(n).unwrap_or(u64::MAX).to_le_bytes());
    };
    match object {
        Object::Null => out.push(0),
        Object::Bool(value) => out.extend_from_slice(&[1, u8::from(*value)]),
        Object::Int(value) => {
            out.push(2);
            out.extend_from_slice(&value.to_le_bytes());
        }
        Object::Real(value) => {
            out.push(3);
            out.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        Object::String(string) => {
            out.push(4);
            length(out, string.bytes.len());
            out.extend_from_slice(&string.bytes);
        }
        Object::Name(name) => {
            out.push(5);
            out.extend_from_slice(&name.id().to_le_bytes());
        }
        Object::Array(items) => {
            out.push(6);
            length(out, items.len());
            for item in items {
                write_exact_at(item, out, depth + 1);
            }
        }
        Object::Dict(dict) => write_dict_at(dict, out, depth),
        Object::Stream(stream) => {
            out.push(8);
            write_dict_at(&stream.dict, out, depth + 1);
            out.extend_from_slice(&stream.data_start.to_le_bytes());
            out.extend_from_slice(&stream.len_hint.unwrap_or(u64::MAX).to_le_bytes());
        }
        Object::Ref(reference) => {
            out.push(9);
            out.extend_from_slice(&reference.num.to_le_bytes());
            out.extend_from_slice(&reference.gen.to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;
    use std::time::Duration;

    /// A value of `bytes` bytes, charged `bytes` beside [`ENTRY_BYTES`].
    fn value(bytes: usize) -> Fill<Arc<Vec<u8>>> {
        Fill::Keep(Arc::new(vec![7; bytes]), bytes as u64)
    }

    /// Requests `key` from `cache`, filling a value of `bytes` bytes on a
    /// miss, and says whether it hit.
    fn ask(cache: &DocumentCache, key: u32, bytes: usize, work: Option<&Work>) -> bool {
        cache.read(key, work, || value(bytes)).1
    }

    #[test]
    fn a_hit_is_handed_the_value_the_miss_kept() {
        let cache = DocumentCache::new(CacheLimits::new(1 << 20));
        let work = Work::new();
        let mut fills = 0;
        let (first, hit) = cache.read(5u32, Some(&work), || {
            fills += 1;
            value(100)
        });
        assert!(!hit);
        let (second, hit) = cache.read(5u32, Some(&work), || {
            fills += 1;
            value(100)
        });
        assert!(hit);
        assert_eq!(fills, 1, "filled once, read twice");
        assert!(Arc::ptr_eq(&first, &second), "the very value, not a copy");
        assert_eq!(cache.held(), 100 + ENTRY_BYTES);
        let stats = work.stats();
        assert_eq!((stats.cache_hits, stats.cache_misses), (1, 1));
        assert_eq!(stats.cache_bytes_held, 100 + ENTRY_BYTES);
        assert_eq!(stats.cache_evictions, 0);
    }

    /// The entries a run of requests leaves in a cache that holds three of
    /// them, with the hits it scored: whatever the hash order, the least
    /// recently requested go first.
    fn survivors() -> (Vec<u32>, Vec<bool>, u64) {
        let cache = DocumentCache::new(CacheLimits::new(3 * (100 + ENTRY_BYTES)));
        let work = Work::new();
        let mut hits = Vec::new();
        for key in [1, 2, 3, 1, 4, 1, 5, 4, 3] {
            hits.push(ask(&cache, key, 100, Some(&work)));
            assert!(
                cache.held() <= 3 * (100 + ENTRY_BYTES),
                "never past the budget"
            );
        }
        let kept = (1..=5).filter(|key| cache.holds(key)).collect();
        (kept, hits, work.stats().cache_evictions)
    }

    #[test]
    fn eviction_follows_the_request_sequence() {
        let (kept, hits, evictions) = survivors();
        assert_eq!(
            hits,
            [false, false, false, true, false, true, false, true, false],
            "1, 2 and 3 fill it; 1 hits; 4 evicts 2, the least recently \
             requested; 1 hits; 5 evicts 3, 1 having been requested since; 4 \
             hits; 3 evicts 1"
        );
        assert_eq!(kept, [3, 4, 5], "the three requested last");
        assert_eq!(evictions, 3);
        // A hash map is seeded per process and per map; the order the cache
        // evicts in is not its order.
        for _ in 0..16 {
            assert_eq!(survivors(), (kept.clone(), hits.clone(), evictions));
        }
    }

    #[test]
    fn an_entry_past_the_whole_budget_is_used_once_and_not_kept() {
        let cache = DocumentCache::new(CacheLimits::new(1000));
        assert!(!ask(&cache, 1, 100, None));
        assert!(
            !ask(&cache, 2, 1000, None),
            "1 000 bytes and 64 beside them"
        );
        assert!(!cache.holds(&2), "past the budget, not kept");
        assert!(cache.holds(&1), "and nothing evicted for it");
        assert!(!ask(&cache, 2, 1000, None), "a miss again, every time");
        assert_eq!(cache.held(), 100 + ENTRY_BYTES);
    }

    #[test]
    fn a_budget_of_zero_keeps_nothing() {
        let cache = DocumentCache::new(CacheLimits::new(0));
        let work = Work::new();
        for _ in 0..3 {
            assert!(!ask(&cache, 1, 0, Some(&work)), "even an empty value");
        }
        assert_eq!(cache.held(), 0);
        let stats = work.stats();
        assert_eq!((stats.cache_hits, stats.cache_misses), (0, 3));
        assert_eq!(stats.cache_bytes_held, 0);
    }

    #[test]
    fn eviction_frees_exactly_what_a_new_entry_needs() {
        let cache = DocumentCache::new(CacheLimits::new(3 * (100 + ENTRY_BYTES)));
        for key in 0..3 {
            ask(&cache, key, 100, None);
        }
        // An entry of two and a bit evicts the two requested first and keeps
        // the third.
        ask(&cache, 9, 200 + ENTRY_BYTES as usize, None);
        assert!(!cache.holds(&0) && !cache.holds(&1));
        assert!(cache.holds(&2) && cache.holds(&9));
        assert_eq!(cache.held(), (100 + ENTRY_BYTES) + (200 + 2 * ENTRY_BYTES));
    }

    #[test]
    fn a_fill_that_keeps_nothing_is_filled_again() {
        let cache = DocumentCache::new(CacheLimits::new(1 << 20));
        let mut fills = 0;
        for _ in 0..3 {
            let (got, hit) = cache.read(4u32, None, || {
                fills += 1;
                Fill::Once(Arc::new(vec![1]))
            });
            assert!(!hit);
            assert_eq!(*got, vec![1]);
        }
        assert_eq!(fills, 3, "a value not kept is filled at every request");
        assert!(!cache.holds(&4));
    }

    /// **Four threads that miss one key at once fill it once.** They meet at
    /// a barrier before the request, and the fill is slow enough that the
    /// other three arrive while it runs: one fill, four holders of its value.
    #[test]
    fn four_requests_at_once_fill_a_key_once() {
        let cache = DocumentCache::new(CacheLimits::new(1 << 20));
        let fills = AtomicUsize::new(0);
        let barrier = Barrier::new(4);
        let values: Vec<Arc<Vec<u8>>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        cache
                            .read(1u32, None, || {
                                fills.fetch_add(1, Ordering::SeqCst);
                                std::thread::sleep(Duration::from_millis(40));
                                value(10)
                            })
                            .0
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("no request panicked"))
                .collect()
        });
        assert_eq!(
            fills.load(Ordering::SeqCst),
            1,
            "one fill for four requests"
        );
        assert!(values.iter().all(|got| Arc::ptr_eq(got, &values[0])));
    }

    /// **A same-thread re-entry never waits on itself.** A fill that requests
    /// its own key again computes the inner value without waiting and
    /// publishes nothing; the outer fill's value is the one kept.
    #[test]
    fn a_same_thread_reentry_never_waits_on_itself() {
        let cache = DocumentCache::new(CacheLimits::new(1 << 20));
        let (outer, hit) = cache.read(3u32, None, || {
            let (inner, hit) = cache.read(3u32, None, || value(1));
            assert!(!hit, "the re-entry computed its own");
            assert!(!cache.holds(&3), "and published nothing");
            Fill::Keep(Arc::new(vec![inner.len() as u8 + 1]), 1)
        });
        assert!(!hit);
        assert_eq!(*outer, vec![2]);
        assert_eq!(*cache.read(3u32, None, || value(9)).0, vec![2], "kept");
    }

    /// **A thread with a fill open never waits**, in this table or in a
    /// `Flights`: two threads each claim a key and, inside the fill, request
    /// the key the other is filling, and each computes it for itself.
    #[test]
    fn crossed_fills_compute_rather_than_deadlock() {
        let cache = DocumentCache::new(CacheLimits::new(1 << 20));
        let claimed = Barrier::new(2);
        let (one, two) = std::thread::scope(|scope| {
            let one = scope.spawn(|| {
                cache.read(1u32, None, || {
                    claimed.wait();
                    let other = cache.read(2u32, None, || value(20)).0;
                    Fill::Keep(Arc::new(vec![other.len() as u8 + 10]), 1)
                })
            });
            let two = scope.spawn(|| {
                cache.read(2u32, None, || {
                    claimed.wait();
                    let other = cache.read(1u32, None, || value(10)).0;
                    Fill::Keep(Arc::new(vec![other.len() as u8 + 20]), 1)
                })
            });
            (one.join().expect("no panic"), two.join().expect("no panic"))
        });
        assert_eq!(*one.0, vec![30]);
        assert_eq!(*two.0, vec![30]);

        // Across this table and a `Flights`: a thread inside a `Flights` fill
        // requests a key here another thread is filling, while that thread
        // waits on the `Flights` key.
        let flights: tinker_pdf_cos::Flights<u32, u32> = tinker_pdf_cos::Flights::new();
        let claimed = Barrier::new(2);
        let (here, there) = std::thread::scope(|scope| {
            let here = scope.spawn(|| {
                cache.read(7u32, None, || {
                    claimed.wait();
                    let other = flights.read(8, || tinker_pdf_cos::Filled::Keep(80));
                    Fill::Keep(Arc::new(vec![(other % 256) as u8]), 1)
                })
            });
            let there = scope.spawn(|| {
                flights.read(8, || {
                    claimed.wait();
                    let other = cache.read(7u32, None, || value(3)).0;
                    tinker_pdf_cos::Filled::Keep(other.len() as u32 + 77)
                })
            });
            (
                here.join().expect("no panic"),
                there.join().expect("no panic"),
            )
        });
        assert_eq!(*here.0, vec![80]);
        assert_eq!(there, 80);
    }

    /// **A fill that unwinds releases its slot**, and a request waiting on
    /// it fills the slot in turn.
    #[test]
    fn a_fill_that_unwinds_releases_its_slot() {
        let cache = DocumentCache::new(CacheLimits::new(1 << 20));
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cache.read(5u32, None, || panic!("the fill failed"))
        }));
        assert!(unwound.is_err());
        assert!(!cache.holds(&5), "nothing was published");
        assert_eq!(*cache.read(5u32, None, || value(2)).0, vec![7, 7]);

        let started = Barrier::new(2);
        let waited = std::thread::scope(|scope| {
            let failing = scope.spawn(|| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    cache.read(6u32, None, || {
                        started.wait();
                        std::thread::sleep(Duration::from_millis(40));
                        panic!("the first fill failed")
                    })
                }))
                .is_err()
            });
            let waiting = scope.spawn(|| {
                started.wait();
                cache.read(6u32, None, || value(3)).0
            });
            (
                failing.join().expect("caught"),
                waiting.join().expect("no panic"),
            )
        });
        assert!(waited.0);
        assert_eq!(*waited.1, vec![7, 7, 7], "the waiter filled the slot");
        assert!(cache.holds(&6));
        assert_eq!(cache.held(), 2 + ENTRY_BYTES + 3 + ENTRY_BYTES);
    }

    #[test]
    fn a_cache_is_shared_between_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<DocumentCache>();
    }

    #[test]
    fn objects_written_exactly_are_equal_only_when_they_are_the_same_value() {
        let doc = tinker_pdf_cos::CosDocument::open(
            b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF"
                .to_vec(),
        )
        .expect("a document to intern names in");
        let name = |bytes: &[u8]| Object::Name(doc.intern(bytes));
        let written = |object: &Object| {
            let mut out = Vec::new();
            write_exact(object, &mut out);
            Written::of(out)
        };
        let array = |items: Vec<Object>| Object::Array(items);
        let calibrated = array(vec![name(b"ICCBased"), Object::Ref(ObjRef::new(9, 0))]);
        assert_eq!(written(&calibrated), written(&calibrated.clone()));
        let pairs = [
            (Object::Int(1), Object::Real(1.0)),
            (Object::Real(0.0), Object::Real(-0.0)),
            (
                Object::Real(0.1),
                Object::Real(f64::from_bits(0.1f64.to_bits() + 1)),
            ),
            (
                Object::Ref(ObjRef::new(9, 0)),
                Object::Ref(ObjRef::new(9, 1)),
            ),
            (name(b"DeviceRGB"), name(b"DeviceGray")),
            (
                array(vec![Object::Int(1), Object::Int(2)]),
                array(vec![Object::Int(2), Object::Int(1)]),
            ),
            (
                array(vec![array(vec![]), Object::Null]),
                array(vec![Object::Null, array(vec![])]),
            ),
            (
                Object::String(tinker_pdf_cos::PdfString::literal(b"ab".to_vec())),
                Object::String(tinker_pdf_cos::PdfString::literal(b"a".to_vec())),
            ),
            (Object::Bool(true), Object::Int(1)),
        ];
        for (one, other) in &pairs {
            assert_ne!(written(one), written(other), "{one:?} against {other:?}");
            assert_eq!(written(one), written(one), "{one:?} against itself");
        }
        assert_eq!(Written::nothing(), Written::default());
        assert_eq!(Written::nothing().len(), 0);
    }
}
