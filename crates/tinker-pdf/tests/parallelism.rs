//! The facade's page walks on the engine's own pool (row P-15,
//! `docs/design/parallelism.md`): where each page ran, read from MS-01's
//! stats; the order pages come back in; and what a walk holds at once.
//!
//! Where a page ran is read from the stats its own operation hands back,
//! never from a schedule: on four threads with every worker started, every
//! page is a worker's, whichever worker took it; serially, and on
//! `wasm32-wasip1`, where no thread starts, every page is the caller's. Where
//! a test must show distinct threads, its pages meet at a barrier only that
//! many threads can pass.

mod guard_support;
mod render_support;

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Barrier, Mutex};
use std::thread::{self, ThreadId};

use tinker_pdf::{
    Bitmap, CancelToken, Document, Parallelism, RenderOptions, RenderWarning, Stats, TextOptions,
};

/// Whether a worker thread can start on this target: on wasm32 none does,
/// and a walk's every page runs on the caller.
const THREADS_START: bool = !cfg!(target_family = "wasm");

/// A document of `pages` pages, each a line of text in the embedded face.
fn walked(pages: usize) -> Document {
    Document::open(guard_support::document(0, pages)).expect("the walk's document opens")
}

/// Where a page ran, by its stats: on a worker, and on the caller.
fn placed(stats: &Stats) -> (u64, u64) {
    (stats.pool_items_on_workers, stats.pool_items_on_caller)
}

/// Where a page of a walk of `parallelism` runs on this target.
fn expected_place(parallelism: Parallelism, pages: usize) -> (u64, u64) {
    if THREADS_START && parallelism.workers(pages) > 0 {
        (1, 0)
    } else {
        (0, 1)
    }
}

/// What a bitmap is compared by: its size, its pixels and its warnings.
fn drawn(bitmap: &Bitmap) -> (u32, u32, Vec<u8>, Vec<RenderWarning>) {
    (
        bitmap.width,
        bitmap.height,
        bitmap.data.clone(),
        bitmap.warnings.clone(),
    )
}

fn counting(parallelism: Parallelism) -> RenderOptions {
    RenderOptions {
        stats: true,
        parallelism,
        ..RenderOptions::default()
    }
}

/// **A page walk on four threads runs no page on the caller.**
///
/// Eight pages rendered through [`Document::render_pages`] with four threads
/// and stats asked: each page's own stats say a worker ran it and the caller
/// did not — on `wasm32-wasip1`, where no thread starts, that the caller ran
/// it — and every page comes back in page order. A part is no step, so each
/// page totals the work the same page rendered alone totals, and the page
/// rendered alone ran in no walk. Each page's counted text, walked through
/// [`Document::walk_pages`], says the same.
#[test]
fn a_page_walk_on_four_threads_runs_no_page_on_the_caller() {
    let doc = walked(8);
    let four = Parallelism::Threads(4);
    let expected = expected_place(four, 8);
    assert_eq!(
        expected,
        if THREADS_START { (1, 0) } else { (0, 1) },
        "four threads over eight pages start four workers wherever a thread starts"
    );

    let mut rendered = Vec::new();
    doc.render_pages(0..8, &counting(four), |index, bitmap| {
        let stats = bitmap.expect("every page renders").stats.expect("asked");
        rendered.push((index, placed(&stats), stats.work()));
    });
    assert_eq!(
        rendered
            .iter()
            .map(|(index, _, _)| *index)
            .collect::<Vec<_>>(),
        (0..8).collect::<Vec<_>>(),
        "in page order"
    );
    for (index, place, work) in &rendered {
        assert_eq!(
            *place, expected,
            "page {index}: (on workers, on the caller)"
        );
        let alone = doc
            .page(*index)
            .expect("a page")
            .render(&counting(Parallelism::Serial))
            .stats
            .expect("asked");
        assert_eq!(placed(&alone), (0, 0), "page {index} alone is no walk's");
        assert_eq!(*work, alone.work(), "page {index}: the work a page does");
    }

    let mut read = Vec::new();
    doc.walk_pages(
        0..8,
        four,
        |_, page| page.map(|page| placed(&page.text_counted(&TextOptions::default()).1)),
        |index, place| read.push((index, place)),
    );
    assert_eq!(
        read,
        (0..8)
            .map(|index| (index, Some(expected)))
            .collect::<Vec<_>>(),
        "every page's counted text says where it was read"
    );
}

/// **Four workers are four threads, none of them the caller's.** Eight pages
/// on four threads meet, four at a time, at a barrier only four distinct
/// threads holding a page each can pass, and record their threads: four of
/// them, none the caller's. On `wasm32-wasip1` the barrier is one, and the
/// one thread is the caller's.
#[test]
fn four_walk_workers_are_four_threads_none_the_callers() {
    let doc = walked(8);
    let caller = thread::current().id();
    let barrier = Barrier::new(if THREADS_START { 4 } else { 1 });
    let mut threads: Vec<ThreadId> = Vec::new();
    doc.walk_pages(
        0..8,
        Parallelism::Threads(4),
        |_, page| {
            assert!(page.is_some(), "every page is found");
            barrier.wait();
            thread::current().id()
        },
        |_, thread| threads.push(thread),
    );
    let distinct: HashSet<ThreadId> = threads.iter().copied().collect();
    if THREADS_START {
        assert_eq!(distinct.len(), 4, "four threads ran the pages");
        assert!(!distinct.contains(&caller), "none of them the caller");
        // `waves`' columns: worker `k` ran pages `k` and `k + 4`.
        for k in 0..4 {
            assert_eq!(threads[k], threads[k + 4], "pages {k} and {}", k + 4);
        }
    } else {
        assert_eq!(distinct, HashSet::from([caller]), "the caller ran them all");
    }
}

/// A serial walk runs each page on the caller and hands it over before the
/// next starts, and so does `Threads(0)`; `Threads(1)` starts one worker,
/// which runs every page, wherever a thread starts.
#[test]
fn a_serial_walk_runs_each_page_on_the_caller_before_the_next() {
    let doc = walked(5);
    for parallelism in [Parallelism::Serial, Parallelism::Threads(0)] {
        let events = Mutex::new(Vec::new());
        doc.walk_pages(
            0..5,
            parallelism,
            |index, page| {
                let stats = page
                    .expect("a page")
                    .render(&counting(Parallelism::Serial))
                    .stats
                    .expect("asked");
                events
                    .lock()
                    .expect("the events")
                    .push(format!("work {index}"));
                placed(&stats)
            },
            |index, place| {
                assert_eq!(place, (0, 1), "{parallelism:?}: page {index} on the caller");
                events
                    .lock()
                    .expect("the events")
                    .push(format!("each {index}"));
            },
        );
        let events = events.into_inner().expect("the events");
        let interleaved: Vec<String> = (0..5)
            .flat_map(|index| [format!("work {index}"), format!("each {index}")])
            .collect();
        assert_eq!(
            events, interleaved,
            "{parallelism:?}: page, then its delivery"
        );
    }

    let one = Parallelism::Threads(1);
    let mut places = Vec::new();
    doc.render_pages(0..5, &counting(one), |_, bitmap| {
        places.push(placed(&bitmap.expect("a page").stats.expect("asked")));
    });
    assert_eq!(places, vec![expected_place(one, 5); 5]);
}

/// **A walk hands pages back in the order they were asked for.** Pages named
/// out of order, one twice and one the document does not have, on three
/// threads: each comes back where it was named, the missing one as `None`,
/// and every bitmap and every page's text is the page's own, read alone —
/// pixels, warnings and text.
#[test]
fn a_walk_hands_pages_back_in_the_order_asked() {
    let doc = walked(8);
    let asked = [5u32, 0, 7, 99, 2, 2, 6];
    let options = RenderOptions {
        parallelism: Parallelism::Threads(3),
        ..RenderOptions::at_dpi(108.0)
    };
    let mut rendered = Vec::new();
    doc.render_pages(asked, &options, |index, bitmap| {
        rendered.push((index, bitmap.as_ref().map(drawn)));
    });
    assert_eq!(
        rendered.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
        asked.to_vec()
    );
    for (index, bitmap) in &rendered {
        let alone = doc
            .page(*index)
            .map(|page| drawn(&page.render(&RenderOptions::at_dpi(108.0))));
        assert_eq!(*bitmap, alone, "page {index}");
        assert_eq!(bitmap.is_none(), *index == 99, "page {index} found or not");
    }

    let text_options = TextOptions {
        parallelism: Parallelism::Threads(3),
        ..TextOptions::default()
    };
    let mut read = Vec::new();
    doc.text_pages(asked, &text_options, |index, text| {
        read.push((index, text.map(|text| format!("{text:?}"))));
    });
    let alone: Vec<(u32, Option<String>)> = asked
        .iter()
        .map(|&index| {
            (
                index,
                doc.page(index).map(|page| format!("{:?}", page.text())),
            )
        })
        .collect();
    assert_eq!(read, alone, "each page's text is its own");
}

/// What a walk's part holds while it is alive: one more of `alive`, counted
/// down when it is dropped.
struct Held<'a> {
    alive: &'a AtomicUsize,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.alive.fetch_sub(1, Ordering::SeqCst);
    }
}

/// **A walk holds one wave of pages at once.** Twelve pages on three
/// threads, each part holding a value until `each` drops it: never more than
/// three alive at once, and one on `wasm32-wasip1`, where the caller runs
/// each page and hands it over before the next.
#[test]
fn a_walk_holds_one_wave_of_pages_at_once() {
    let doc = walked(12);
    let alive = AtomicUsize::new(0);
    let most = AtomicUsize::new(0);
    let mut delivered = 0;
    doc.walk_pages(
        0..12,
        Parallelism::Threads(3),
        |_, page| {
            let bitmap = page.map(|page| page.render(&RenderOptions::at_dpi(36.0)));
            let now = alive.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(now, Ordering::SeqCst);
            (Held { alive: &alive }, bitmap)
        },
        |_, (held, bitmap)| {
            assert!(bitmap.is_some(), "the page rendered");
            drop(held);
            delivered += 1;
        },
    );
    assert_eq!(delivered, 12);
    let most = most.load(Ordering::SeqCst);
    assert!(
        most <= if THREADS_START { 3 } else { 1 },
        "{most} pages were alive at once"
    );
    assert_eq!(alive.load(Ordering::SeqCst), 0, "and none is left");
}

/// **A walk inside a walk runs its pages on that worker.** Each of four
/// outer pages walks three pages of its own asking for four threads: they run
/// on the outer page's thread, and their stats say a worker ran them. The
/// outer page's own render, made after its inner walk, still counts the
/// outer part.
#[test]
fn a_walk_inside_a_walk_runs_its_pages_on_that_worker() {
    let doc = walked(4);
    let mut outcomes = Vec::new();
    doc.walk_pages(
        0..4,
        Parallelism::Threads(4),
        |_, page| {
            let me = thread::current().id();
            let mut inner = Vec::new();
            doc.walk_pages(
                0..3,
                Parallelism::Threads(4),
                |_, inner_page| {
                    let stats = inner_page
                        .expect("an inner page")
                        .render(&counting(Parallelism::Serial))
                        .stats
                        .expect("asked");
                    (thread::current().id(), placed(&stats))
                },
                |_, ran| inner.push(ran),
            );
            let own = page
                .expect("an outer page")
                .render(&counting(Parallelism::Serial))
                .stats
                .expect("asked");
            let on_this_thread = inner.iter().all(|(thread, _)| *thread == me);
            let inner_places: Vec<(u64, u64)> = inner.into_iter().map(|(_, place)| place).collect();
            (on_this_thread, inner_places, placed(&own))
        },
        |_, outcome| outcomes.push(outcome),
    );
    let worker = expected_place(Parallelism::Threads(4), 4);
    for (on_this_thread, inner, own) in outcomes {
        assert!(
            on_this_thread,
            "the inner pages ran on the outer page's thread"
        );
        assert_eq!(inner, vec![worker; 3], "where the inner pages ran");
        assert_eq!(own, worker, "the outer page still counts its own part");
    }
}

/// A walk whose render is cancelled before it starts hands back every page,
/// each reporting the cancellation as the same page rendered alone with the
/// same token does.
#[test]
fn a_cancelled_walk_reports_every_page_as_its_render_does() {
    let doc = walked(6);
    let cancel = CancelToken::new();
    cancel.cancel();
    let options = RenderOptions {
        cancel: Some(cancel),
        parallelism: Parallelism::Threads(3),
        ..RenderOptions::default()
    };
    let mut rendered = Vec::new();
    doc.render_pages(0..6, &options, |index, bitmap| {
        rendered.push((index, bitmap.expect("a page")));
    });
    assert_eq!(rendered.len(), 6);
    for (index, bitmap) in &rendered {
        assert!(
            bitmap.warnings.contains(&RenderWarning::Cancelled),
            "page {index} says it was cancelled"
        );
        let alone = doc.page(*index).expect("a page").render(&RenderOptions {
            parallelism: Parallelism::Serial,
            ..options.clone()
        });
        assert_eq!(drawn(bitmap), drawn(&alone), "page {index}");
    }
}

/// Every option that carries threads asks for none unless told to, and a
/// render asked for threads of its own — not through a walk — draws exactly
/// what it draws serially.
#[test]
fn every_walk_is_serial_unless_asked() {
    assert_eq!(RenderOptions::default().parallelism, Parallelism::Serial);
    assert_eq!(
        RenderOptions::at_dpi(300.0).parallelism,
        Parallelism::Serial
    );
    assert_eq!(TextOptions::default().parallelism, Parallelism::Serial);
    assert_eq!(Parallelism::default(), Parallelism::Serial);

    let doc = walked(2);
    let page = doc.page(1).expect("a page");
    let threaded = page.render(&RenderOptions {
        parallelism: Parallelism::Threads(4),
        ..RenderOptions::default()
    });
    assert_eq!(
        drawn(&threaded),
        drawn(&page.render(&RenderOptions::default()))
    );
    let text = page.text_with(&TextOptions {
        parallelism: Parallelism::Threads(4),
        ..TextOptions::default()
    });
    assert_eq!(format!("{text:?}"), format!("{:?}", page.text()));
}
