//! Single-flight fills: a map of slots under one mutex and a condition
//! variable, whose first reader of a key computes its value while readers of
//! that key on other threads wait for it (`docs/design/parallelism.md`,
//! "Single-flight fills").
//!
//! What it is for is a cache whose value is dear to compute — a decoded
//! stream, a decoded image — read by the threads of one pooled operation at
//! once. The values are deterministic, so two fills of one key race to the
//! same answer; what they cost is the second fill, and a count of fills would
//! read one or four by the schedule.
//!
//! **Why not `OnceLock`.** Same-thread re-entry deadlocks it, and re-entry is
//! what a `/Length` pointing into its own stream produces (`store.rs` gives
//! the reason at its head). The object store answers with a compare-and-swap
//! and no waiting at all, which costs a wasted parse; a decode is too dear to
//! waste, so a flight waits, under rules that cannot deadlock:
//!
//! 1. A read of a published key returns its value.
//! 2. A read of an empty slot claims it for this thread and fills it outside
//!    the lock; the fill publishes its value and wakes the slot's waiters.
//! 3. A read of a slot another thread is filling **waits** for that fill —
//!    unless this thread is itself in the middle of a fill, of any key in any
//!    map, in which case it computes the value for itself and publishes
//!    nothing. A thread-local count of the fills a thread has open says
//!    which: every `Flights` fill's, and every fill a table of another
//!    crate's that keeps these rules marks with [`OpenFill`] — the facade's
//!    document cache — so the rule holds across all of them.
//! 4. A same-thread re-entry for a key the thread is filling is rule 3's
//!    exception: it computes without waiting, as an uncached read does.
//! 5. A fill that unwinds — or answers a value that is not to be kept —
//!    releases its slot and wakes its waiters, so no waiter waits on a fill
//!    that will not publish; the first of them claims the slot in turn.
//!
//! Only a thread with no fill open ever waits, so no thread waits on a thread
//! that is waiting, and no cycle of waits can form, across any number of
//! maps. The price of rule 3 is a second computation where fills nest across
//! threads; the values are the same, so only a count can see it.

use std::cell::Cell;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, ThreadId};

/// What a fill answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filled<V> {
    /// A value to publish: every later read of the key is handed it.
    Keep(V),
    /// A value for this read alone: the slot is left empty, and the next
    /// read of the key fills it again.
    Once(V),
}

/// A map whose every key is filled once, however many threads read it at
/// once. See the module documentation for the rules.
#[derive(Debug)]
pub struct Flights<K, V> {
    slots: Mutex<HashMap<K, Slot<V>>>,
    /// Signalled whenever a slot is published or released.
    settled: Condvar,
}

impl<K, V> Default for Flights<K, V> {
    fn default() -> Self {
        Flights {
            slots: Mutex::new(HashMap::new()),
            settled: Condvar::new(),
        }
    }
}

/// One key's slot.
#[derive(Debug)]
enum Slot<V> {
    /// The thread that claimed the slot is filling it.
    Filling(ThreadId),
    /// The published value.
    Ready(V),
}

thread_local! {
    /// How many fills this thread has open, of any key in any map: a thread
    /// with one open never waits (rule 3).
    static OPEN: Cell<usize> = const { Cell::new(0) };
}

/// A lock that a panic elsewhere cannot poison: nothing here panics while
/// holding one, and a fill runs outside it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl<K: Eq + Hash + Clone, V: Clone> Flights<K, V> {
    /// A map that has filled nothing.
    #[must_use]
    pub fn new() -> Flights<K, V> {
        Flights::default()
    }

    /// The value of `key`: the published one, or what `fill` answers, which
    /// this read computes when no other thread is filling the key — or when
    /// one is and this thread has a fill of its own open — and otherwise
    /// waits for.
    pub fn read(&self, key: K, fill: impl FnOnce() -> Filled<V>) -> V {
        let me = thread::current().id();
        let mut slots = lock(&self.slots);
        loop {
            match slots.get(&key) {
                Some(Slot::Ready(value)) => return value.clone(),
                Some(Slot::Filling(owner)) => {
                    // Rules 3 and 4: a thread with a fill open, or the one
                    // filling this key, computes for itself and publishes
                    // nothing, so that it never waits on anyone.
                    if *owner == me || OPEN.with(Cell::get) > 0 {
                        drop(slots);
                        let _open = Open::enter();
                        return match fill() {
                            Filled::Keep(value) | Filled::Once(value) => value,
                        };
                    }
                    slots = self
                        .settled
                        .wait(slots)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                None => {
                    slots.insert(key.clone(), Slot::Filling(me));
                    drop(slots);
                    let claim = Claim {
                        flights: self,
                        key: Some(key),
                        _open: Open::enter(),
                    };
                    return claim.fill(fill);
                }
            }
        }
    }

    /// Whether `key` has a published value.
    #[must_use]
    pub fn is_ready(&self, key: &K) -> bool {
        matches!(lock(&self.slots).get(key), Some(Slot::Ready(_)))
    }

    /// How many keys hold a slot, published or being filled: what a cache
    /// bounded by its entries compares with its bound before it keeps one
    /// more.
    #[must_use]
    pub fn len(&self) -> usize {
        lock(&self.slots).len()
    }

    /// Whether no key holds a slot.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        lock(&self.slots).is_empty()
    }
}

/// Whether this thread has a fill open in any single-flight map — a
/// [`Flights`], or a table that keeps these rules under a lock of its own
/// and marks its fills with [`OpenFill`] — and so must compute rather than
/// wait (rule 3).
#[must_use]
pub fn fill_open() -> bool {
    OPEN.with(Cell::get) > 0
}

/// One fill this thread has open in a single-flight table that keeps this
/// module's rules under a lock of its own — the facade's document cache — for
/// as long as it lives. It is counted where every [`Flights`] fill is, so rule
/// 3 holds across that table and every `Flights` at once: a thread with a
/// fill open in either never waits in either, and no cycle of waits can form
/// between them.
#[must_use = "the fill is open only while the guard lives"]
pub struct OpenFill {
    _open: Open,
}

impl OpenFill {
    /// Marks a fill open on this thread until the guard is dropped.
    pub fn enter() -> OpenFill {
        OpenFill {
            _open: Open::enter(),
        }
    }
}

/// One open fill on this thread, for as long as it lives.
struct Open;

impl Open {
    fn enter() -> Open {
        OPEN.with(|open| open.set(open.get().saturating_add(1)));
        Open
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        OPEN.with(|open| open.set(open.get().saturating_sub(1)));
    }
}

/// A slot this thread has claimed: published when its fill keeps a value,
/// and released, its waiters woken, when the fill keeps none or unwinds.
struct Claim<'a, K: Eq + Hash + Clone, V: Clone> {
    flights: &'a Flights<K, V>,
    /// The key claimed, until it is published or released.
    key: Option<K>,
    _open: Open,
}

impl<K: Eq + Hash + Clone, V: Clone> Claim<'_, K, V> {
    /// Runs the fill and settles the slot by what it answered.
    fn fill(mut self, fill: impl FnOnce() -> Filled<V>) -> V {
        let filled = fill();
        let mut slots = lock(&self.flights.slots);
        let value = match filled {
            Filled::Keep(value) => {
                if let Some(key) = self.key.take() {
                    slots.insert(key, Slot::Ready(value.clone()));
                }
                value
            }
            Filled::Once(value) => {
                if let Some(key) = self.key.take() {
                    slots.remove(&key);
                }
                value
            }
        };
        drop(slots);
        self.flights.settled.notify_all();
        value
    }
}

impl<K: Eq + Hash + Clone, V: Clone> Drop for Claim<'_, K, V> {
    /// A fill that unwound: its slot is released, and its waiters woken to
    /// claim it in turn (rule 5).
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            lock(&self.flights.slots).remove(&key);
            self.flights.settled.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;
    use std::time::Duration;

    /// A published key is handed out on every read with no fill run, and a
    /// value kept for one read alone is filled again by the next.
    #[test]
    fn a_kept_value_is_read_back_and_a_passing_one_is_not() {
        let flights: Flights<u32, String> = Flights::new();
        let mut fills = 0;
        for _ in 0..3 {
            let value = flights.read(7, || {
                fills += 1;
                Filled::Keep("seven".to_string())
            });
            assert_eq!(value, "seven");
        }
        assert_eq!(fills, 1, "filled once, read three times");
        assert!(flights.is_ready(&7));

        let mut passing = 0;
        for _ in 0..3 {
            let value = flights.read(8, || {
                passing += 1;
                Filled::Once("eight".to_string())
            });
            assert_eq!(value, "eight");
        }
        assert_eq!(passing, 3, "a value not kept is filled at every read");
        assert!(!flights.is_ready(&8));
    }

    /// **Four threads that miss one key at once fill it once.** They meet at
    /// a barrier before the read, and the fill is slow enough that the other
    /// three arrive while it runs: one fill, four readers of its value.
    #[test]
    fn concurrent_readers_of_one_key_fill_it_once() {
        let flights: Flights<u32, u64> = Flights::new();
        let fills = AtomicUsize::new(0);
        let barrier = Barrier::new(4);
        let values: Vec<u64> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        flights.read(1, || {
                            fills.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(40));
                            Filled::Keep(42)
                        })
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("no reader panicked"))
                .collect()
        });
        assert_eq!(values, vec![42; 4]);
        assert_eq!(fills.load(Ordering::SeqCst), 1, "one fill for four readers");
    }

    /// **A same-thread re-entry never waits on itself.** A fill that reads
    /// its own key again computes the inner value without waiting and
    /// publishes nothing; the outer fill's value is the one kept.
    #[test]
    fn a_same_thread_reentry_never_waits_on_itself() {
        let flights: Flights<u32, u32> = Flights::new();
        let value = flights.read(3, || {
            let inner = flights.read(3, || Filled::Keep(30));
            assert_eq!(inner, 30, "the re-entry computed its own");
            assert!(!flights.is_ready(&3), "and published nothing");
            Filled::Keep(inner + 1)
        });
        assert_eq!(value, 31);
        assert_eq!(
            flights.read(3, || Filled::Keep(0)),
            31,
            "the outer value kept"
        );
    }

    /// **A thread with a fill open never waits.** Two threads each claim a
    /// key and, inside the fill, read the key the other is filling: each
    /// computes it for itself, and both finish — where waiting would be a
    /// cycle of two.
    #[test]
    fn crossed_fills_compute_rather_than_deadlock() {
        let flights: Flights<u32, u32> = Flights::new();
        let claimed = Barrier::new(2);
        let (one, two) = std::thread::scope(|scope| {
            let one = scope.spawn(|| {
                flights.read(1, || {
                    claimed.wait();
                    let other = flights.read(2, || Filled::Keep(20));
                    Filled::Keep(10 + other)
                })
            });
            let two = scope.spawn(|| {
                flights.read(2, || {
                    claimed.wait();
                    let other = flights.read(1, || Filled::Keep(10));
                    Filled::Keep(20 + other)
                })
            });
            (one.join().expect("no panic"), two.join().expect("no panic"))
        });
        assert_eq!((one, two), (30, 30));
    }

    /// **A fill that unwinds releases its slot.** The panicking fill's slot
    /// is empty after it, and the next read fills it; a reader waiting on the
    /// fill while it unwound claims the slot in turn and fills it.
    #[test]
    fn a_fill_that_unwinds_releases_its_slot() {
        let flights: Flights<u32, u32> = Flights::new();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            flights.read(5, || panic!("the fill failed"))
        }));
        assert!(unwound.is_err());
        assert!(!flights.is_ready(&5), "nothing was published");
        assert_eq!(flights.read(5, || Filled::Keep(50)), 50, "filled afresh");

        let started = Barrier::new(2);
        let waited = std::thread::scope(|scope| {
            let failing = scope.spawn(|| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    flights.read(6, || {
                        started.wait();
                        std::thread::sleep(Duration::from_millis(40));
                        panic!("the first fill failed")
                    })
                }))
                .is_err()
            });
            let waiting = scope.spawn(|| {
                started.wait();
                flights.read(6, || Filled::Keep(60))
            });
            (
                failing.join().expect("caught"),
                waiting.join().expect("no panic"),
            )
        });
        assert_eq!(waited, (true, 60), "the waiter filled the released slot");
        assert!(flights.is_ready(&6));
    }

    /// A fill that keeps nothing wakes a reader waiting on it, which fills the
    /// slot itself.
    #[test]
    fn a_waiter_on_a_passing_fill_fills_the_slot_itself() {
        let flights: Flights<u32, u32> = Flights::new();
        let started = Barrier::new(2);
        let fills = AtomicUsize::new(0);
        let (first, second) = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                flights.read(9, || {
                    fills.fetch_add(1, Ordering::SeqCst);
                    started.wait();
                    std::thread::sleep(Duration::from_millis(40));
                    Filled::Once(1)
                })
            });
            let second = scope.spawn(|| {
                started.wait();
                flights.read(9, || {
                    fills.fetch_add(1, Ordering::SeqCst);
                    Filled::Keep(2)
                })
            });
            (
                first.join().expect("no panic"),
                second.join().expect("no panic"),
            )
        });
        assert_eq!((first, second), (1, 2));
        assert_eq!(fills.load(Ordering::SeqCst), 2);
        assert_eq!(flights.read(9, || Filled::Keep(3)), 2, "the second's kept");
    }

    /// A map's length counts every key that holds a slot: a published one,
    /// and one being filled, which a fill reading the length sees as its own;
    /// a value kept for one read alone leaves no slot behind.
    #[test]
    fn the_length_counts_published_and_filling_slots() {
        let flights: Flights<u32, usize> = Flights::new();
        assert!(flights.is_empty());
        assert_eq!(flights.read(1, || Filled::Keep(0)), 0);
        assert_eq!(flights.len(), 1);
        let seen = flights.read(2, || Filled::Keep(flights.len()));
        assert_eq!(seen, 2, "the slot being filled is counted");
        assert_eq!(flights.read(3, || Filled::Once(flights.len())), 3);
        assert_eq!(flights.len(), 2, "a passing value leaves no slot");
        assert!(!flights.is_empty());
    }

    #[test]
    fn a_map_of_flights_is_shared_between_threads() {
        fn shared<T: Send + Sync>() {}
        shared::<Flights<u32, std::sync::Arc<Vec<u8>>>>();
    }

    /// **A fill open in a table of another crate's counts as one here.** A
    /// thread holding an [`OpenFill`] reads a key another thread is filling
    /// and computes it for itself rather than waiting, as a thread inside a
    /// `Flights` fill does; and [`fill_open`] says so while the guard lives
    /// and not after.
    #[test]
    fn an_open_fill_elsewhere_computes_rather_than_waits() {
        assert!(!fill_open(), "nothing is open on a fresh test thread");
        {
            let _outer = OpenFill::enter();
            assert!(fill_open());
            let _inner = OpenFill::enter();
            assert!(fill_open(), "nested guards count");
        }
        assert!(!fill_open(), "both guards dropped, none open");

        let flights: Flights<u32, u32> = Flights::new();
        let claimed = Barrier::new(2);
        let release = Barrier::new(2);
        let (filled, computed) = std::thread::scope(|scope| {
            let filling = scope.spawn(|| {
                flights.read(4, || {
                    claimed.wait();
                    // Held until the other thread has computed its own.
                    release.wait();
                    Filled::Keep(40)
                })
            });
            let other = scope.spawn(|| {
                claimed.wait();
                let _open = OpenFill::enter();
                // Waiting here would never end: the fill waits on `release`.
                let value = flights.read(4, || Filled::Keep(41));
                release.wait();
                value
            });
            (
                filling.join().expect("no panic"),
                other.join().expect("no panic"),
            )
        });
        assert_eq!((filled, computed), (40, 41), "the other thread computed");
        assert_eq!(
            flights.read(4, || Filled::Keep(0)),
            40,
            "only the claim published"
        );
    }
}
