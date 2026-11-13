//! One byte budget and the one order everything it holds leaves in
//! (`docs/design/performance.md`, P-16, "Parsed objects under pressure").
//!
//! Feature documentation: `docs/features/opening.md` (a budget stated at
//! open) and `docs/features/rendering.md` ("The document's cache").
//!
//! An [`Accountant`] is what a document's object store and the cache a layer
//! above keeps both charge when their caller states a budget: one counter
//! every request takes its number from, one total of what is held, and one
//! ordered index from each holding's last request to the holding, across both
//! holders. A charge that would take the total past the budget makes room by
//! evicting the holdings least recently requested first, whichever holder
//! keeps them, until it fits: an entry of the cache above always, and a parsed
//! object only where no caller holds it — its `Arc` owned by nothing but the
//! store. A charge larger than the whole budget is never kept, and neither is
//! one the holdings no caller holds cannot make room for; so what is held,
//! read at any moment, never passes the budget.
//!
//! **One lock.** Everything a budget decides it decides under its ledger's
//! mutex: the store's kept objects are inside it, and so is the table of the
//! cache above ([`Entries`]), so evicting either is a step taken under the
//! lock the charge is taken under, and no other lock is ever taken inside it.
//! Nothing is parsed, decoded or computed under it: a holder computes outside
//! and takes the lock to look up, charge and publish.
//!
//! **Deterministic on one thread.** The maps are only ever looked up, since
//! their order is seeded per process; which holdings leave is decided by the
//! ordered index alone, so a serial caller asking the same sequence of things
//! evicts the same holdings on every run and every target.

use core::fmt;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::object::Object;

/// What every holding is charged beside its payload — its key, its
/// bookkeeping and its place in the order — so that a budget of empty
/// holdings is bounded too.
pub const ENTRY_BYTES: u64 = 64;

/// The table of a cache that charges an [`Accountant`] beside a document's
/// object store: what the ledger evicts from when one of its entries is the
/// holding requested longest ago.
///
/// The table lives inside the ledger ([`Ledger::entries`]), so the cache
/// reaches it under the one lock the ledger is kept under.
pub trait Entries: Send + 'static {
    /// What an entry is kept under, as the ledger's order names it.
    type Key: Clone + Send + 'static;

    /// Removes the published entry under `key`, handing back what it was
    /// charged; 0 where nothing is published under it.
    fn evict(&mut self, key: &Self::Key) -> u64;

    /// Removes every entry.
    fn clear(&mut self);
}

/// The table of an accountant that a document's object store alone charges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoEntries;

impl Entries for NoEntries {
    type Key = core::convert::Infallible;

    fn evict(&mut self, key: &Self::Key) -> u64 {
        match *key {}
    }

    fn clear(&mut self) {}
}

/// One byte budget that a document's object store and the cache above it
/// charge. See the module documentation.
pub struct Accountant<E: Entries = NoEntries> {
    budget: u64,
    ledger: Mutex<Ledger<E>>,
}

impl<E: Entries> Accountant<E> {
    /// An accountant holding nothing yet under a budget of `budget` bytes,
    /// with `entries` the table of the cache that charges it beside the
    /// object store.
    #[must_use]
    pub fn new(budget: u64, entries: E) -> Accountant<E> {
        Accountant {
            budget,
            ledger: Mutex::new(Ledger {
                budget,
                requests: 0,
                held: 0,
                entries_held: 0,
                objects_held: 0,
                order: BTreeMap::new(),
                objects: HashMap::new(),
                entries,
            }),
        }
    }

    /// The most bytes this accountant's holdings are charged at once.
    #[must_use]
    pub fn budget(&self) -> u64 {
        self.budget
    }

    /// The ledger, under its lock. A lock that a panic elsewhere cannot
    /// poison: nothing panics while holding it, and nothing is computed
    /// under it.
    pub fn lock(&self) -> MutexGuard<'_, Ledger<E>> {
        self.ledger.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What every holding is charged together, now.
    #[must_use]
    pub fn held(&self) -> u64 {
        self.lock().held
    }

    /// What the parsed objects are charged together, now.
    #[must_use]
    pub fn objects_held(&self) -> u64 {
        self.lock().objects_held
    }
}

impl Accountant<NoEntries> {
    /// An accountant that a document's object store alone charges, under a
    /// budget of `budget` bytes.
    #[must_use]
    pub fn for_objects(budget: u64) -> Accountant {
        Accountant::new(budget, NoEntries)
    }
}

impl<E: Entries> fmt::Debug for Accountant<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ledger = self.lock();
        f.debug_struct("Accountant")
            .field("budget", &self.budget)
            .field("held", &ledger.held)
            .field("objects_held", &ledger.objects_held)
            .finish()
    }
}

/// Everything an [`Accountant`] keeps, behind its one lock.
pub struct Ledger<E: Entries> {
    /// The budget, as the accountant states it.
    budget: u64,
    /// The requests made so far, of every holder.
    requests: u64,
    /// What every holding is charged together.
    held: u64,
    /// What the cache's entries are charged together.
    entries_held: u64,
    /// What the parsed objects are charged together.
    objects_held: u64,
    /// Each holding's last request, to the holding: the order evictions are
    /// made in, least recently requested first.
    order: BTreeMap<u64, Holding<E::Key>>,
    /// The parsed objects kept, under their store and their number.
    objects: HashMap<(u64, u32), Kept>,
    /// The table of the cache that charges this ledger beside the store.
    entries: E,
}

/// One holding, as the order names it.
enum Holding<K> {
    /// A parsed object: its store and its number.
    Object(u64, u32),
    /// An entry of the cache above.
    Entry(K),
}

/// A parsed object kept: the value, the number of its last request and its
/// charge.
struct Kept {
    object: Arc<Object>,
    asked: u64,
    charge: u64,
}

/// What [`Ledger::admit`] did with a charge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Admitted {
    /// Whether the entry is kept: false for a charge larger than the whole
    /// budget, and for one that the holdings no caller holds could not make
    /// room for.
    pub kept: bool,
    /// The cache's entries evicted to make room for it.
    pub entries_evicted: u64,
    /// The parsed objects dropped to make room for it.
    pub objects_dropped: u64,
}

impl<E: Entries> Ledger<E> {
    /// The table of the cache above.
    pub fn entries(&mut self) -> &mut E {
        &mut self.entries
    }

    /// The next request's number: every request of every holder, a hit or a
    /// miss, takes one.
    pub fn request(&mut self) -> u64 {
        self.requests = self.requests.saturating_add(1);
        self.requests
    }

    /// Moves the holding whose last request was numbered `last` to the
    /// request numbered `asked`, which found it: a hit.
    pub fn reorder(&mut self, last: u64, asked: u64) {
        if let Some(holding) = self.order.remove(&last) {
            self.order.insert(asked, holding);
        }
    }

    /// Records an entry of the cache above under `key`, charged `charge`, for
    /// the request numbered `asked`, evicting the holdings least recently
    /// requested first until it fits. The caller publishes the entry in its
    /// table where [`Admitted::kept`] says it is kept, and otherwise keeps
    /// nothing: what is held never passes the budget.
    pub fn admit(&mut self, key: E::Key, charge: u64, asked: u64) -> Admitted {
        let admitted = self.make_room(charge);
        if admitted.kept {
            self.held = self.held.saturating_add(charge);
            self.entries_held = self.entries_held.saturating_add(charge);
            self.order.insert(asked, Holding::Entry(key));
        }
        admitted
    }

    /// Forgets every entry of the cache above, keeping the parsed objects:
    /// what a cache that is let go of leaves behind it.
    pub fn forget_entries(&mut self) {
        self.order
            .retain(|_, holding| matches!(holding, Holding::Object(..)));
        self.held = self.held.saturating_sub(self.entries_held);
        self.entries_held = 0;
        self.entries.clear();
    }

    /// The budget every holding is held to together.
    #[must_use]
    pub fn budget(&self) -> u64 {
        self.budget
    }

    /// What every holding is charged together.
    #[must_use]
    pub fn held(&self) -> u64 {
        self.held
    }

    /// What the cache's entries are charged together.
    #[must_use]
    pub fn entries_held(&self) -> u64 {
        self.entries_held
    }

    /// What the parsed objects are charged together.
    #[must_use]
    pub fn objects_held(&self) -> u64 {
        self.objects_held
    }

    /// Makes room for a charge of `charge`, evicting the holdings least
    /// recently requested first — passing over an object a caller holds —
    /// until it fits, and says whether it does. A charge larger than the
    /// whole budget evicts nothing and does not fit.
    fn make_room(&mut self, charge: u64) -> Admitted {
        let mut admitted = Admitted::default();
        if charge > self.budget {
            return admitted;
        }
        // The objects a caller holds, passed over, back in the order once the
        // room is made: at the number they were requested by, so a pass
        // moves nothing.
        let mut held_by_callers = Vec::new();
        while self.held.saturating_add(charge) > self.budget {
            let Some((number, holding)) = self.order.pop_first() else {
                break;
            };
            match holding {
                Holding::Entry(key) => {
                    let freed = self.entries.evict(&key);
                    self.held = self.held.saturating_sub(freed);
                    self.entries_held = self.entries_held.saturating_sub(freed);
                    admitted.entries_evicted = admitted.entries_evicted.saturating_add(1);
                }
                Holding::Object(store, num) => {
                    let unheld = self
                        .objects
                        .get(&(store, num))
                        .is_some_and(|kept| Arc::strong_count(&kept.object) == 1);
                    if !unheld {
                        if self.objects.contains_key(&(store, num)) {
                            held_by_callers.push((number, Holding::Object(store, num)));
                        }
                        continue;
                    }
                    if let Some(kept) = self.objects.remove(&(store, num)) {
                        self.held = self.held.saturating_sub(kept.charge);
                        self.objects_held = self.objects_held.saturating_sub(kept.charge);
                        admitted.objects_dropped = admitted.objects_dropped.saturating_add(1);
                    }
                }
            }
        }
        for (number, holding) in held_by_callers {
            self.order.insert(number, holding);
        }
        admitted.kept = self.held.saturating_add(charge) <= self.budget;
        admitted
    }
}

/// The object store's side of an accountant: what `crate::store::SlotStore`
/// asks of the ledger it charges, whatever cache shares it.
pub(crate) trait ObjectLedger: Send + Sync {
    /// The object `num` of the store `store`, where it is kept: the request
    /// numbered, and the object moved to the end of the order.
    fn get(&self, store: u64, num: u32) -> Option<Arc<Object>>;

    /// Keeps `object` as `num` of the store `store`, charged `charge`, unless
    /// one is kept under it already, and hands back whichever is kept — or
    /// `object` itself, kept nowhere, where it does not fit.
    fn keep(&self, store: u64, num: u32, object: Arc<Object>, charge: u64) -> Arc<Object>;

    /// Forgets every object of the store `store`: outstanding `Arc`s keep
    /// their values.
    fn forget_objects(&self, store: u64);
}

impl<E: Entries> ObjectLedger for Accountant<E> {
    fn get(&self, store: u64, num: u32) -> Option<Arc<Object>> {
        let mut ledger = self.lock();
        let last = ledger.objects.get(&(store, num))?.asked;
        let asked = ledger.request();
        ledger.reorder(last, asked);
        let kept = ledger.objects.get_mut(&(store, num))?;
        kept.asked = asked;
        Some(Arc::clone(&kept.object))
    }

    fn keep(&self, store: u64, num: u32, object: Arc<Object>, charge: u64) -> Arc<Object> {
        let mut ledger = self.lock();
        if let Some(kept) = ledger.objects.get(&(store, num)) {
            // Two loads raced and the other published first: its copy is the
            // one every reader is handed.
            return Arc::clone(&kept.object);
        }
        let asked = ledger.request();
        if ledger.make_room(charge).kept {
            ledger.held = ledger.held.saturating_add(charge);
            ledger.objects_held = ledger.objects_held.saturating_add(charge);
            ledger.order.insert(asked, Holding::Object(store, num));
            ledger.objects.insert(
                (store, num),
                Kept {
                    object: Arc::clone(&object),
                    asked,
                    charge,
                },
            );
        }
        object
    }

    fn forget_objects(&self, store: u64) {
        let mut ledger = self.lock();
        let numbers: Vec<u64> = ledger
            .objects
            .iter()
            .filter(|((of, _), _)| *of == store)
            .map(|(_, kept)| kept.asked)
            .collect();
        for number in numbers {
            ledger.order.remove(&number);
        }
        let mut freed = 0u64;
        ledger.objects.retain(|(of, _), kept| {
            let forget = *of == store;
            if forget {
                freed = freed.saturating_add(kept.charge);
            }
            !forget
        });
        ledger.held = ledger.held.saturating_sub(freed);
        ledger.objects_held = ledger.objects_held.saturating_sub(freed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table of plain entries under a number: each its charge and the
    /// number of its last request, as a cache keeps them.
    #[derive(Default)]
    struct Plain {
        kept: HashMap<u32, (u64, u64)>,
    }

    impl Entries for Plain {
        type Key = u32;

        fn evict(&mut self, key: &u32) -> u64 {
            self.kept.remove(key).map_or(0, |(charge, _)| charge)
        }

        fn clear(&mut self) {
            self.kept.clear();
        }
    }

    /// Asks `accountant` for the entry `key`, publishing one charged `charge`
    /// where none is kept, as a cache does, and says whether it was found.
    fn ask(accountant: &Accountant<Plain>, key: u32, charge: u64) -> bool {
        let mut ledger = accountant.lock();
        let asked = ledger.request();
        if let Some((_, last)) = ledger.entries().kept.get(&key).copied() {
            ledger.reorder(last, asked);
            if let Some(entry) = ledger.entries().kept.get_mut(&key) {
                entry.1 = asked;
            }
            return true;
        }
        if ledger.admit(key, charge, asked).kept {
            ledger.entries().kept.insert(key, (charge, asked));
        }
        false
    }

    /// Whether `accountant` keeps the entry `key`, without requesting it.
    fn holds(accountant: &Accountant<Plain>, key: u32) -> bool {
        accountant.lock().entries().kept.contains_key(&key)
    }

    /// The object `num` of `store`, charged `charge`, kept where it fits and
    /// handed back.
    fn keep(ledger: &dyn ObjectLedger, store: u64, num: u32, charge: u64) -> Arc<Object> {
        ledger.keep(store, num, Arc::new(Object::Int(i64::from(num))), charge)
    }

    /// Whether `ledger` keeps object `num` of store 0. Asking numbers the
    /// request, as every read does.
    fn kept(ledger: &dyn ObjectLedger, num: u32) -> bool {
        ledger.get(0, num).is_some()
    }

    #[test]
    fn objects_and_entries_leave_in_one_order() {
        let accountant = Accountant::new(300, Plain::default());
        let objects: &dyn ObjectLedger = &accountant;
        // Object 1, entry 10 and object 2: three holdings of 100.
        drop(keep(objects, 0, 1, 100));
        assert!(!ask(&accountant, 10, 100));
        drop(keep(objects, 0, 2, 100));
        assert_eq!(accountant.held(), 300);
        // Object 1 is read again, so entry 10 is the oldest now.
        assert!(kept(objects, 1));
        assert!(!ask(&accountant, 11, 100), "a fourth holding misses");
        assert!(!holds(&accountant, 10), "and entry 10 left for it");
        assert!(holds(&accountant, 11));
        {
            let ledger = accountant.lock();
            assert_eq!(ledger.held(), 300);
            assert_eq!((ledger.entries_held(), ledger.objects_held()), (100, 200));
        }
        // An object kept now evicts object 2, the oldest left.
        drop(keep(objects, 0, 3, 100));
        assert!(!kept(objects, 2), "object 2 dropped");
        assert!(kept(objects, 1) && kept(objects, 3));
        assert!(holds(&accountant, 11));
        assert_eq!(accountant.held(), 300);
    }

    #[test]
    fn an_object_a_caller_holds_is_passed_over() {
        let accountant = Accountant::for_objects(200);
        let objects: &dyn ObjectLedger = &accountant;
        let held = keep(objects, 0, 1, 100);
        drop(keep(objects, 0, 2, 100));
        // Object 1 is the oldest, and held: object 2 makes room instead.
        drop(keep(objects, 0, 3, 100));
        let first = objects.get(0, 1).expect("held, so never dropped");
        assert!(
            Arc::ptr_eq(&first, &held),
            "the very value the caller holds"
        );
        drop(first);
        assert!(!kept(objects, 2));
        let three = objects.get(0, 3).expect("kept");
        // Every object held, nothing can make room: the new one is handed
        // back and kept nowhere, and what is held stays inside the budget.
        let four = keep(objects, 0, 4, 100);
        assert_eq!(*four, Object::Int(4), "handed back all the same");
        assert!(!kept(objects, 4), "and kept nowhere");
        assert_eq!(accountant.held(), 200);
        // Let go of, object 1 is the oldest and leaves first.
        drop((held, three));
        drop(keep(objects, 0, 5, 100));
        assert!(!kept(objects, 1));
        assert!(kept(objects, 3) && kept(objects, 5));
        assert_eq!(accountant.held(), 200);
    }

    #[test]
    fn a_charge_past_the_whole_budget_is_never_kept_and_evicts_nothing() {
        let accountant = Accountant::new(100, Plain::default());
        let objects: &dyn ObjectLedger = &accountant;
        drop(keep(objects, 0, 1, 60));
        assert!(!ask(&accountant, 7, 101));
        assert!(!holds(&accountant, 7));
        assert!(kept(objects, 1), "nothing evicted for it");
        drop(keep(objects, 0, 2, 101));
        assert!(!kept(objects, 2));
        assert!(kept(objects, 1));
        assert_eq!(accountant.held(), 60);
    }

    #[test]
    fn a_budget_of_zero_keeps_nothing() {
        let accountant = Accountant::new(0, Plain::default());
        let objects: &dyn ObjectLedger = &accountant;
        for _ in 0..3 {
            assert_eq!(*keep(objects, 0, 1, ENTRY_BYTES), Object::Int(1));
            assert!(!kept(objects, 1), "even an empty object's charge");
            assert!(!ask(&accountant, 1, ENTRY_BYTES));
            assert!(!holds(&accountant, 1));
        }
        assert_eq!(accountant.held(), 0);
    }

    #[test]
    fn the_first_publisher_wins() {
        let accountant = Accountant::for_objects(1_000);
        let objects: &dyn ObjectLedger = &accountant;
        let first = keep(objects, 0, 9, 100);
        let second = objects.keep(0, 9, Arc::new(Object::Bool(true)), 100);
        assert!(
            Arc::ptr_eq(&first, &second),
            "the second load is handed the first's"
        );
        assert_eq!(accountant.held(), 100, "charged once");
    }

    #[test]
    fn stores_sharing_an_accountant_keep_their_own_numbers() {
        let accountant = Accountant::for_objects(1_000);
        let objects: &dyn ObjectLedger = &accountant;
        drop(objects.keep(1, 5, Arc::new(Object::Int(1)), 100));
        drop(objects.keep(2, 5, Arc::new(Object::Int(2)), 100));
        assert_eq!(objects.get(1, 5).as_deref(), Some(&Object::Int(1)));
        assert_eq!(objects.get(2, 5).as_deref(), Some(&Object::Int(2)));
        objects.forget_objects(1);
        assert!(objects.get(1, 5).is_none(), "store 1's forgotten");
        assert_eq!(objects.get(2, 5).as_deref(), Some(&Object::Int(2)));
        assert_eq!(accountant.held(), 100);
        assert_eq!(accountant.objects_held(), 100);
        // A forgotten object's place in the order went with it: room is made
        // from what is left.
        drop(objects.keep(1, 6, Arc::new(Object::Int(6)), 900));
        assert!(objects.get(1, 6).is_some());
        assert_eq!(accountant.held(), 1_000);
    }

    #[test]
    fn forgetting_the_entries_keeps_the_objects() {
        let accountant = Accountant::new(1_000, Plain::default());
        let objects: &dyn ObjectLedger = &accountant;
        drop(keep(objects, 0, 1, 100));
        assert!(!ask(&accountant, 3, 200));
        assert_eq!(accountant.held(), 300);
        accountant.lock().forget_entries();
        assert_eq!(accountant.held(), 100);
        assert!(!holds(&accountant, 3));
        assert!(kept(objects, 1));
        // The order holds no entry after it: room is made from the objects
        // alone, and the forgotten entry is charged nothing.
        let tight = Accountant::new(150, Plain::default());
        assert!(!ask(&tight, 3, 100));
        tight.lock().forget_entries();
        let objects: &dyn ObjectLedger = &tight;
        drop(keep(objects, 0, 1, 150));
        assert!(kept(objects, 1));
        assert_eq!(tight.held(), 150);
    }

    /// One scripted run over three holdings' room — odd numbers objects, even
    /// ones entries, each charged 100 — and what it leaves: the requests that
    /// hit, the objects kept and the entries kept.
    fn scripted() -> (Vec<bool>, Vec<u32>, Vec<u32>) {
        let accountant = Accountant::new(300, Plain::default());
        let objects: &dyn ObjectLedger = &accountant;
        let mut hits = Vec::new();
        for step in [1u32, 2, 3, 1, 4, 5, 2, 6] {
            let hit = if step % 2 == 0 {
                ask(&accountant, step, 100)
            } else {
                let hit = kept(objects, step);
                if !hit {
                    drop(keep(objects, 0, step, 100));
                }
                hit
            };
            hits.push(hit);
            assert!(accountant.held() <= 300, "never past the budget");
        }
        let mut entries: Vec<u32> = accountant.lock().entries().kept.keys().copied().collect();
        entries.sort_unstable();
        let objects = [1u32, 3, 5]
            .into_iter()
            .filter(|num| accountant.lock().objects.contains_key(&(0, *num)))
            .collect();
        (hits, objects, entries)
    }

    /// **The evictions follow the requests across both holders**: object 1,
    /// entry 2 and object 3 fill the room; object 1 hits; entry 4 evicts entry
    /// 2, the oldest; object 5 evicts object 3; entry 2, asked again, misses
    /// and evicts object 1; entry 6 evicts entry 4 — the same on every run,
    /// whatever the hash order of the process.
    #[test]
    fn evictions_follow_the_requests_across_both_holders() {
        let (hits, objects, entries) = scripted();
        assert_eq!(
            hits,
            [false, false, false, true, false, false, false, false]
        );
        assert_eq!(objects, [5], "object 5 alone is kept");
        assert_eq!(entries, [2, 6]);
        for _ in 0..16 {
            assert_eq!(scripted(), (hits.clone(), objects.clone(), entries.clone()));
        }
    }

    #[test]
    fn an_accountant_is_shared_between_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<Accountant>();
        shared::<Accountant<Plain>>();
    }
}
