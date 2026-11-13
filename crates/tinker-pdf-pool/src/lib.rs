//! The engine's own thread pool: the independent parts of one operation run
//! on threads the call starts inside one `std::thread::scope` and joins before
//! it returns, each result in a slot of its own, joined in index order.
//!
//! Feature documentation: `docs/design/parallelism.md`.
//!
//! **A leaf** (ruling 8): it takes closures and plain values and returns
//! values, and knows nothing of documents, so the writer, a decoder and the
//! facade each hand it their work as a plain parameter. No `unsafe`, no
//! third-party crate, no global pool, no executor and no thread that outlives
//! the call that started it (ruling 23's answer to H-16).
//!
//! # What it promises
//!
//! - **Index order.** [`map`] writes each result to its own pre-sized slot and
//!   returns them in index order, and [`waves`] hands each wave to its
//!   consumer in index order, so no output depends on the thread count or on
//!   which thread finished first.
//! - **Serial unless asked.** [`Parallelism::Serial`] is the default and runs
//!   every part on the caller's thread, in order.
//! - **Serial on the caller where no thread starts.** A worker is started
//!   through `std::thread::Builder::spawn_scoped`, which answers an error on
//!   wasm32 and on a host at its thread limit; that is not a panic, and the
//!   share the worker would have taken is taken on the caller, so no part is
//!   lost and the output is the serial output.
//! - **One level of threads.** A worker marks itself in a thread-local
//!   ([`on_worker`]), and a pooled call made on a worker runs every part on
//!   that worker, so pools never nest into more threads.
//! - **A panic reaches the caller.** A part that panics on a worker is carried
//!   to the caller once every worker has ended, and no thread is left waiting
//!   on a part that will not finish.

#![forbid(unsafe_code)]

use std::any::Any;
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, Scope, ScopedJoinHandle};

/// The most worker threads one call starts, whatever it asks for: a caller's
/// `Threads(usize::MAX)` asks the host for a bounded number.
pub const MAX_THREADS: usize = 256;

/// How an operation spends threads on its independent parts.
///
/// A plain `Copy` value, so a leaf takes it in a parameter struct with nothing
/// of the facade in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Parallelism {
    /// Everything on the caller's thread, in order.
    #[default]
    Serial,
    /// The operation's independent parts on this many worker threads the call
    /// starts and joins, the caller waiting; `Threads(0)` is `Serial`.
    Threads(usize),
}

impl Parallelism {
    /// How many workers a call over `parts` independent parts starts.
    ///
    /// None for `Serial`, for one part or none, and on a worker — a pooled
    /// call there runs on that worker ([`on_worker`]). Otherwise the threads
    /// asked for, never more than the parts and never more than
    /// [`MAX_THREADS`].
    #[must_use]
    pub fn workers(self, parts: usize) -> usize {
        match self {
            Parallelism::Serial => 0,
            Parallelism::Threads(_) if parts <= 1 || on_worker() => 0,
            Parallelism::Threads(threads) => threads.min(parts).min(MAX_THREADS),
        }
    }
}

/// What a crate hands its independent work to: the engine's pool, or a test's
/// own threads.
///
/// `run` calls `job(i)` exactly once for each `i` in `0..len`, on whatever
/// threads it has, and returns when every call has.
pub trait Executor: Sync {
    /// Runs `job` for every index in `0..len` and returns once all have.
    fn run(&self, len: usize, job: &(dyn Fn(usize) + Sync));
}

impl Executor for Parallelism {
    /// Starts [`Parallelism::workers`] threads, each taking the next index
    /// from one shared counter and running it until the counter passes
    /// `len`; the caller waits in the scope's join. A worker that will not
    /// start leaves the caller to join the counter loop itself, so the share
    /// it would have taken is taken there.
    fn run(&self, len: usize, job: &(dyn Fn(usize) + Sync)) {
        let workers = self.workers(len);
        // The counter passes `len` by at most one a thread, so it cannot wrap
        // below this; a count of parts past it is no real operation's, and it
        // runs on the caller rather than near the edge of `usize`.
        if workers == 0 || len > usize::MAX - MAX_THREADS - 1 {
            (0..len).for_each(job);
            return;
        }
        let next = AtomicUsize::new(0);
        let drain = || loop {
            let index = next.fetch_add(1, Ordering::Relaxed);
            if index >= len {
                break;
            }
            job(index);
        };
        thread::scope(|scope| {
            let mut handles = Vec::with_capacity(workers);
            let mut refused = false;
            for worker in 0..workers {
                match start(scope, worker, || {
                    let _worker = Worker::enter();
                    drain();
                }) {
                    Ok(handle) => handles.push(handle),
                    Err(_) => refused = true,
                }
            }
            if refused {
                drain();
            }
            join(handles);
        });
    }
}

/// `f` over `0..len` on `executor`, each result in a slot of its own,
/// returned in index order.
///
/// A slot the executor left empty — which only an [`Executor`] that skips an
/// index can do — is filled on the caller, so no executor can make this
/// panic or lose a part.
pub fn map<T: Send>(executor: &dyn Executor, len: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let slots: Vec<Mutex<Option<T>>> = (0..len).map(|_| Mutex::new(None)).collect();
    executor.run(len, &|index| {
        let value = f(index);
        if let Some(slot) = slots.get(index) {
            *lock(slot) = Some(value);
        }
    });
    slots
        .into_iter()
        .enumerate()
        .map(|(index, slot)| {
            slot.into_inner()
                .unwrap_or_else(PoisonError::into_inner)
                .unwrap_or_else(|| f(index))
        })
        .collect()
}

/// `work` over `0..len` in waves of the worker count, each wave handed to
/// `deliver` in index order on the caller before the next starts.
///
/// With `w` workers started, worker `k` runs parts `k`, `k + w`, … and waits
/// at each wave's start until the wave before it has been delivered, so the
/// results alive at once are one wave's — at most `w` — the same on every run
/// whichever thread finishes first. A worker that will not start leaves its
/// column of the waves to the caller. `Serial`, one part, or a call on a
/// worker runs each part and delivers it in turn, on the caller.
pub fn waves<T: Send>(
    parallelism: Parallelism,
    len: usize,
    work: impl Fn(usize) -> T + Sync,
    mut deliver: impl FnMut(usize, T),
) {
    let workers = parallelism.workers(len);
    if workers == 0 {
        for index in 0..len {
            let value = work(index);
            deliver(index, value);
        }
        return;
    }
    let schedule = Schedule::new(workers);
    let (schedule, work) = (&schedule, &work);
    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        let mut columns = Vec::new();
        for column in 0..workers {
            match start(scope, column, move || {
                let _worker = Worker::enter();
                schedule.column(column, workers, len, work);
            }) {
                Ok(handle) => handles.push(handle),
                Err(_) => columns.push(column),
            }
        }
        // Whatever ends the caller's loop — the last wave, a worker that
        // unwound, or the caller unwinding out of `work` or `deliver` — no
        // worker is left waiting for a wave that will not open.
        let release = Release(schedule);
        for wave in 0..len.div_ceil(workers) {
            let first = wave * workers;
            for &column in &columns {
                let index = first + column;
                if index < len {
                    schedule.put(column, work(index));
                }
            }
            let Some(results) = schedule.take(workers.min(len - first)) else {
                break;
            };
            for (column, result) in results.into_iter().enumerate() {
                let index = first + column;
                deliver(index, result.unwrap_or_else(|| work(index)));
            }
            schedule.open(wave + 1);
        }
        drop(release);
        join(handles);
    });
}

thread_local! {
    /// Whether this thread is one of a pool's workers.
    static WORKER: Cell<bool> = const { Cell::new(false) };
}

/// Whether this thread is one of a pool's workers.
///
/// A pooled call made on a worker runs every part on that worker
/// ([`Parallelism::workers`] answers none), so an operation nests no deeper
/// than one level of threads. A thread-local flag, not global state.
#[must_use]
pub fn on_worker() -> bool {
    WORKER.with(Cell::get)
}

/// Marks the thread it is made on as a worker for as long as it lives.
struct Worker;

impl Worker {
    fn enter() -> Worker {
        WORKER.with(|worker| worker.set(true));
        Worker
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        WORKER.with(|worker| worker.set(false));
    }
}

/// Starts worker `index` in `scope`, running `body`: the one place this crate
/// starts a thread, and so the one place a start can fail.
fn start<'scope, 'env>(
    scope: &'scope Scope<'scope, 'env>,
    index: usize,
    body: impl FnOnce() + Send + 'scope,
) -> std::io::Result<ScopedJoinHandle<'scope, ()>> {
    if refused(index) {
        return Err(std::io::Error::other("the test refused this worker"));
    }
    thread::Builder::new().spawn_scoped(scope, body)
}

/// Joins every worker, and carries the first one's panic on to the caller
/// once all of them have ended.
fn join(handles: Vec<ScopedJoinHandle<'_, ()>>) {
    let mut panic: Option<Box<dyn Any + Send>> = None;
    for handle in handles {
        if let Err(payload) = handle.join() {
            panic.get_or_insert(payload);
        }
    }
    if let Some(payload) = panic {
        std::panic::resume_unwind(payload);
    }
}

/// A lock that a panic elsewhere cannot poison: nothing here panics while
/// holding one, and a part runs outside every lock.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The state of [`waves`] between its caller and its workers.
struct Schedule<T> {
    state: Mutex<Waves<T>>,
    changed: Condvar,
}

/// What [`Schedule`] guards.
struct Waves<T> {
    /// The wave the workers may run: each waits until this reaches its next.
    open: usize,
    /// One slot a column, for the wave in flight.
    slots: Vec<Option<T>>,
    /// How many of the wave in flight have finished.
    filled: usize,
    /// Set when the walk ends or anything in it unwound: whoever is waiting
    /// stops.
    stopped: bool,
}

impl<T> Schedule<T> {
    fn new(columns: usize) -> Schedule<T> {
        Schedule {
            state: Mutex::new(Waves {
                open: 0,
                slots: (0..columns).map(|_| None).collect(),
                filled: 0,
                stopped: false,
            }),
            changed: Condvar::new(),
        }
    }

    /// Worker `column`'s share: parts `column`, `column + columns`, …, each
    /// once its wave opens.
    fn column(&self, column: usize, columns: usize, len: usize, work: &impl Fn(usize) -> T) {
        let _abandon = Abandon(self);
        let mut index = column;
        let mut wave = 0;
        while index < len {
            {
                let mut state = lock(&self.state);
                while state.open < wave && !state.stopped {
                    state = self
                        .changed
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                if state.stopped {
                    return;
                }
            }
            self.put(column, work(index));
            index = index.saturating_add(columns);
            wave += 1;
        }
    }

    /// Column `column`'s result for the wave in flight.
    fn put(&self, column: usize, value: T) {
        let mut state = lock(&self.state);
        if let Some(slot) = state.slots.get_mut(column) {
            *slot = Some(value);
        }
        state.filled += 1;
        self.changed.notify_all();
    }

    /// The first `parts` columns' results once all have finished, in column
    /// order, or `None` where the walk stopped first because a worker
    /// unwound.
    fn take(&self, parts: usize) -> Option<Vec<Option<T>>> {
        let mut state = lock(&self.state);
        while state.filled < parts && !state.stopped {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if state.filled < parts {
            return None;
        }
        state.filled = 0;
        Some(
            state
                .slots
                .iter_mut()
                .take(parts)
                .map(Option::take)
                .collect(),
        )
    }

    /// Lets the workers run wave `wave`.
    fn open(&self, wave: usize) {
        lock(&self.state).open = wave;
        self.changed.notify_all();
    }

    /// Ends the walk for everyone still waiting.
    fn stop(&self) {
        lock(&self.state).stopped = true;
        self.changed.notify_all();
    }
}

/// Stops the walk when a worker's share unwinds, so the caller waiting for
/// its part stops waiting and carries the panic on.
struct Abandon<'a, T>(&'a Schedule<T>);

impl<T> Drop for Abandon<'_, T> {
    fn drop(&mut self) {
        if thread::panicking() {
            self.0.stop();
        }
    }
}

/// Stops the walk when the caller's loop ends, however it ends.
struct Release<'a, T>(&'a Schedule<T>);

impl<T> Drop for Release<'_, T> {
    fn drop(&mut self) {
        self.0.stop();
    }
}

#[cfg(test)]
thread_local! {
    /// The workers a test has said will not start, one bit each: the start
    /// failure wasm32 and a host at its thread limit give, on a native target.
    static REFUSED: Cell<u64> = const { Cell::new(0) };
}

/// Whether worker `index` is to fail to start, as the test asked.
#[cfg(test)]
fn refused(index: usize) -> bool {
    let bit = u32::try_from(index)
        .ok()
        .and_then(|shift| 1u64.checked_shl(shift))
        .unwrap_or(0);
    REFUSED.with(Cell::get) & bit != 0
}

/// No worker is refused outside a test: a start fails only where the host
/// says so.
#[cfg(not(test))]
const fn refused(_index: usize) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::thread::ThreadId;

    /// Runs `body` with the workers in `mask` refused, and clears the mask
    /// after.
    fn refusing<R>(mask: u64, body: impl FnOnce() -> R) -> R {
        REFUSED.with(|refused| refused.set(mask));
        let result = body();
        REFUSED.with(|refused| refused.set(0));
        result
    }

    /// Where each part ran: its index, its thread and whether that thread was
    /// a worker, in the order they ran.
    type Ran = Mutex<Vec<(usize, ThreadId, bool)>>;

    fn record(ran: &Ran, index: usize) {
        lock(ran).push((index, thread::current().id(), on_worker()));
    }

    /// **A worker that will not start leaves its share to the caller.**
    ///
    /// Four workers asked for over twenty-three parts, with no worker
    /// refused, the first, the second, two of them, and all four, through
    /// [`Executor::run`], [`map`] and [`waves`]: every part runs exactly once
    /// and the results come back in index order. Where every worker is
    /// refused, every part runs on the caller; where some are, the caller
    /// joins in. Under [`waves`] the refused worker's column — parts `k`,
    /// `k + 4`, … — is exactly what the caller runs.
    #[test]
    fn a_worker_that_will_not_start_leaves_its_share_to_the_caller() {
        const PARTS: usize = 23;
        let caller = thread::current().id();
        for mask in [0b0000u64, 0b0001, 0b0010, 0b0101, 0b1111] {
            let refused: Vec<usize> = (0..4).filter(|k| (mask >> k) & 1 == 1).collect();

            // `run`: every index once.
            let ran: Ran = Mutex::new(Vec::new());
            refusing(mask, || {
                Parallelism::Threads(4).run(PARTS, &|index| record(&ran, index));
            });
            let ran = ran.into_inner().unwrap_or_else(PoisonError::into_inner);
            let indices: BTreeSet<usize> = ran.iter().map(|(index, _, _)| *index).collect();
            assert_eq!(ran.len(), PARTS, "mask {mask:04b}: each part once");
            assert_eq!(indices.len(), PARTS, "mask {mask:04b}: every part");
            if mask == 0b1111 {
                assert!(
                    ran.iter()
                        .all(|(_, thread, worker)| *thread == caller && !worker),
                    "mask {mask:04b}: no worker started, so the caller ran everything"
                );
            }
            if mask == 0 {
                assert!(
                    ran.iter()
                        .all(|(_, thread, worker)| *thread != caller && *worker),
                    "every worker started, so the caller ran nothing"
                );
            }

            // `map`: in index order whatever ran where.
            let mapped = refusing(mask, || {
                map(&Parallelism::Threads(4), PARTS, |index| {
                    (index * 7, on_worker())
                })
            });
            let values: Vec<usize> = mapped.iter().map(|(value, _)| *value).collect();
            assert_eq!(
                values,
                (0..PARTS).map(|index| index * 7).collect::<Vec<_>>(),
                "mask {mask:04b}: map's results in index order"
            );
            if mask == 0b1111 {
                assert!(mapped.iter().all(|(_, worker)| !worker));
            }

            // `waves`: the refused columns are the caller's.
            let mut delivered = Vec::new();
            refusing(mask, || {
                waves(
                    Parallelism::Threads(4),
                    PARTS,
                    |index| (index, thread::current().id(), on_worker()),
                    |index, part| delivered.push((index, part)),
                );
            });
            assert_eq!(
                delivered
                    .iter()
                    .map(|(index, _)| *index)
                    .collect::<Vec<_>>(),
                (0..PARTS).collect::<Vec<_>>(),
                "mask {mask:04b}: waves deliver in index order"
            );
            for (index, (ran_as, thread, worker)) in &delivered {
                assert_eq!(index, ran_as);
                let on_caller = refused.contains(&(index % 4));
                assert_eq!(
                    (*thread == caller, !worker),
                    (on_caller, on_caller),
                    "mask {mask:04b}: part {index} ran on the caller exactly when its \
                     column's worker was refused"
                );
            }
        }
    }

    /// The thread count a call starts: none serially, for one part or none,
    /// or on a worker; otherwise the threads asked for, held to the parts and
    /// to [`MAX_THREADS`].
    #[test]
    fn a_call_starts_at_most_its_parts_and_the_cap() {
        assert_eq!(Parallelism::default(), Parallelism::Serial);
        assert_eq!(Parallelism::Serial.workers(100), 0);
        assert_eq!(Parallelism::Threads(0).workers(100), 0);
        assert_eq!(Parallelism::Threads(4).workers(0), 0);
        assert_eq!(Parallelism::Threads(4).workers(1), 0);
        assert_eq!(Parallelism::Threads(4).workers(2), 2);
        assert_eq!(Parallelism::Threads(4).workers(100), 4);
        assert_eq!(Parallelism::Threads(usize::MAX).workers(100), 100);
        assert_eq!(
            Parallelism::Threads(usize::MAX).workers(10_000),
            MAX_THREADS
        );
        assert!(!on_worker(), "a test's own thread is no worker");
        let inside = map(&Parallelism::Threads(2), 2, |_| {
            (on_worker(), Parallelism::Threads(4).workers(100))
        });
        assert_eq!(inside, vec![(true, 0), (true, 0)], "on a worker, none");
        assert!(!on_worker(), "and the caller is still none");
    }

    /// **A pooled call made on a worker runs on that worker.** Each of four
    /// parts runs a nested [`map`] and [`waves`] of eight asking for four
    /// threads; every nested part ran on its outer part's thread.
    #[test]
    fn a_pool_inside_a_worker_runs_on_that_worker() {
        let outer = map(&Parallelism::Threads(4), 4, |_| {
            let me = thread::current().id();
            let inner = map(&Parallelism::Threads(4), 8, |_| thread::current().id());
            let mut waved = Vec::new();
            waves(
                Parallelism::Threads(4),
                8,
                |_| thread::current().id(),
                |_, thread| waved.push(thread),
            );
            inner.iter().chain(&waved).all(|thread| *thread == me)
        });
        assert_eq!(outer, vec![true; 4]);
    }

    /// No parts, no calls; one part, on the caller.
    #[test]
    fn an_empty_or_single_operation_runs_on_the_caller() {
        let caller = thread::current().id();
        assert!(map(&Parallelism::Threads(4), 0, |index| index).is_empty());
        assert_eq!(
            map(&Parallelism::Threads(4), 1, |_| thread::current().id()),
            vec![caller]
        );
        let mut delivered = Vec::new();
        waves(
            Parallelism::Threads(4),
            0,
            |index| index,
            |_, _| {
                delivered.push(());
            },
        );
        assert!(delivered.is_empty());
        let mut threads = Vec::new();
        waves(
            Parallelism::Threads(4),
            1,
            |_| thread::current().id(),
            |_, thread| threads.push(thread),
        );
        assert_eq!(threads, vec![caller]);
    }

    /// An [`Executor`] that skips every odd index: [`map`] fills each slot it
    /// left empty on the caller, so nothing is lost and nothing panics.
    #[test]
    fn an_executor_that_skips_an_index_loses_no_part() {
        struct EvenOnly;
        impl Executor for EvenOnly {
            fn run(&self, len: usize, job: &(dyn Fn(usize) + Sync)) {
                (0..len).step_by(2).for_each(job);
            }
        }
        assert_eq!(
            map(&EvenOnly, 9, |index| index * index),
            (0..9).map(|index| index * index).collect::<Vec<_>>()
        );
    }

    /// **A part that panics reaches the caller, and nothing waits forever.**
    /// A panic in one part of [`map`], of [`waves`] on a worker, of a part the
    /// caller runs for a refused worker, and in `deliver`, each comes back as
    /// the caller's panic with its own message once every worker has ended.
    #[test]
    fn a_panic_in_a_part_reaches_the_caller() {
        let message = |payload: Box<dyn Any + Send>| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_default()
        };
        let caught = std::panic::catch_unwind(|| {
            map(&Parallelism::Threads(4), 16, |index| {
                assert!(index != 5, "part five");
                index
            })
        });
        assert_eq!(message(caught.expect_err("it panicked")), "part five");

        let caught = std::panic::catch_unwind(|| {
            waves(
                Parallelism::Threads(3),
                10,
                |index| {
                    assert!(index != 4, "wave part four");
                    index
                },
                |_, _| {},
            );
        });
        assert_eq!(message(caught.expect_err("it panicked")), "wave part four");

        // Worker 1 refused, so its column — 1, 4, 7 — runs on the caller.
        let caught = std::panic::catch_unwind(|| {
            refusing(0b10, || {
                waves(
                    Parallelism::Threads(3),
                    10,
                    |index| {
                        assert!(index != 4, "the caller's part four");
                        index
                    },
                    |_, _| {},
                );
            });
        });
        REFUSED.with(|refused| refused.set(0));
        assert_eq!(
            message(caught.expect_err("it panicked")),
            "the caller's part four"
        );

        let caught = std::panic::catch_unwind(|| {
            waves(
                Parallelism::Threads(3),
                10,
                |index| index,
                |index, _| assert!(index != 6, "delivery six"),
            );
        });
        assert_eq!(message(caught.expect_err("it panicked")), "delivery six");
    }

    /// **`waves` keeps one wave alive.** Over forty parts on three, four and
    /// seven workers, the results finished and not yet delivered never number
    /// more than the workers, and every part is delivered in index order.
    #[test]
    fn waves_keep_one_wave_alive() {
        for threads in [3usize, 4, 7] {
            let alive = AtomicUsize::new(0);
            let most = AtomicUsize::new(0);
            let mut delivered = Vec::new();
            waves(
                Parallelism::Threads(threads),
                40,
                |index| {
                    let now = alive.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    index
                },
                |index, value| {
                    alive.fetch_sub(1, Ordering::SeqCst);
                    delivered.push((index, value));
                },
            );
            assert!(
                most.load(Ordering::SeqCst) <= threads,
                "{threads} workers held {} results at once",
                most.load(Ordering::SeqCst)
            );
            assert_eq!(
                delivered,
                (0..40).map(|index| (index, index)).collect::<Vec<_>>()
            );
        }
    }
}
