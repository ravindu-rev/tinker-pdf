//! The pool through its public surface: results in index order at every
//! thread count, and parts that provably leave the caller.
//!
//! Neither test counts a schedule. The first makes the parts finish in the
//! reverse of their order and holds the results to index order; the second
//! makes the parts prove they ran on distinct threads by meeting at a barrier
//! only that many threads can pass, rather than asserting which worker took
//! how many.

use std::collections::HashSet;
use std::sync::{Barrier, Mutex};
use std::thread::{self, ThreadId};
use std::time::Duration;

use tinker_pdf_pool::{map, on_worker, waves, Executor, Parallelism};

/// The parts each run takes.
const PARTS: usize = 12;

/// Part `index`, made to finish after every later part that started with it:
/// the earlier the part, the longer it sleeps, so completion runs in the
/// reverse of index order wherever parts run side by side.
fn slow_part(index: usize, finished: &Mutex<Vec<usize>>) -> usize {
    let wait = (PARTS - index) as u64 * 5;
    thread::sleep(Duration::from_millis(wait));
    if let Ok(mut finished) = finished.lock() {
        finished.push(index);
    }
    index * 10 + 1
}

/// **Results come back in index order at every thread count.**
///
/// Serially and on 1, 2, 3, 4 and 7 threads, twelve parts made to finish in
/// the reverse of their order: [`map`] returns each result in its own index's
/// place, [`waves`] delivers them in index order, and [`Executor::run`] calls
/// every index exactly once. On two threads or more the parts did finish out
/// of order, so the order of the results is the pool's doing.
#[test]
fn results_come_back_in_index_order_at_every_thread_count() {
    let expected: Vec<usize> = (0..PARTS).map(|index| index * 10 + 1).collect();
    for parallelism in [
        Parallelism::Serial,
        Parallelism::Threads(1),
        Parallelism::Threads(2),
        Parallelism::Threads(3),
        Parallelism::Threads(4),
        Parallelism::Threads(7),
    ] {
        let finished = Mutex::new(Vec::new());
        let mapped = map(&parallelism, PARTS, |index| slow_part(index, &finished));
        assert_eq!(mapped, expected, "{parallelism:?}: map's results");
        let finished = finished.into_inner().unwrap_or_default();
        let in_order: Vec<usize> = (0..PARTS).collect();
        if parallelism.workers(PARTS) >= 2 {
            assert_ne!(
                finished, in_order,
                "{parallelism:?}: the parts finished in index order, so this proved nothing"
            );
        } else {
            assert_eq!(finished, in_order, "{parallelism:?}: one thread, in order");
        }

        let finished = Mutex::new(Vec::new());
        let mut delivered = Vec::new();
        waves(
            parallelism,
            PARTS,
            |index| slow_part(index, &finished),
            |index, value| delivered.push((index, value)),
        );
        assert_eq!(
            delivered,
            expected.iter().copied().enumerate().collect::<Vec<_>>(),
            "{parallelism:?}: waves deliver in index order"
        );

        let called = Mutex::new(Vec::new());
        parallelism.run(PARTS, &|index| {
            if let Ok(mut called) = called.lock() {
                called.push(index);
            }
        });
        let mut called = called.into_inner().unwrap_or_default();
        called.sort_unstable();
        assert_eq!(called, in_order, "{parallelism:?}: every index once");
    }
}

/// **Four threads run every part off the caller.**
///
/// Eight parts on four threads, through [`map`], [`waves`] and
/// [`Executor::run`]: each part waits at a barrier of four, which only four
/// distinct threads each holding a part can pass, and records its thread.
/// Every operation finished, so four distinct threads ran its parts, none of
/// them the caller's, and every part saw itself on a worker. Under [`waves`]
/// worker `k` runs parts `k` and `k + 4`, so those pairs share a thread.
#[test]
fn four_threads_run_every_item_off_the_caller() {
    let caller = thread::current().id();
    let check = |what: &str, ran: Vec<(usize, ThreadId, bool)>| {
        assert_eq!(ran.len(), 8, "{what}: every part ran");
        let threads: HashSet<ThreadId> = ran.iter().map(|(_, thread, _)| *thread).collect();
        assert_eq!(threads.len(), 4, "{what}: four distinct threads");
        assert!(!threads.contains(&caller), "{what}: the caller ran a part");
        assert!(
            ran.iter().all(|(_, _, worker)| *worker),
            "{what}: every part ran on a worker"
        );
    };
    let four = Parallelism::Threads(4);

    let barrier = Barrier::new(4);
    let part = |index: usize| {
        barrier.wait();
        (index, thread::current().id(), on_worker())
    };
    check("map", map(&four, 8, part));

    let barrier = Barrier::new(4);
    let mut delivered = Vec::new();
    waves(
        four,
        8,
        |index| {
            barrier.wait();
            (index, thread::current().id(), on_worker())
        },
        |_, ran| delivered.push(ran),
    );
    for k in 0..4 {
        assert_eq!(
            delivered[k].1,
            delivered[k + 4].1,
            "waves: parts {k} and {} are worker {k}'s column",
            k + 4
        );
    }
    check("waves", delivered);

    let barrier = Barrier::new(4);
    let ran = Mutex::new(Vec::new());
    four.run(8, &|index| {
        barrier.wait();
        if let Ok(mut ran) = ran.lock() {
            ran.push((index, thread::current().id(), on_worker()));
        }
    });
    check("run", ran.into_inner().unwrap_or_default());
}
