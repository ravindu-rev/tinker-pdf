//! Render every page on a pool of threads this program owns, and prove the
//! pool changed nothing.
//!
//! The engine spawns no thread, on any target — that is a policy, not an
//! omission, and it is why the same types compile single-threaded for wasm32.
//! What it offers a threaded caller instead is three properties: `Document` is
//! `Send + Sync`; `Document::page` hands back an *owned* `Page`, cloning an
//! `Arc` rather than borrowing, so a worker holding a page borrows nothing;
//! and `FontProvider` is `Send + Sync` too. Given those, a pool over page
//! indices is a dozen lines of `std::thread::scope` in the embedder, with no
//! lifetime work and no library support at all. Those dozen lines are this
//! file.
//!
//! Two things worth copying. The document is shared by reference and never
//! cloned per thread: a clone each would prove only that a `Document` can be
//! *sent*, and the property being relied on is that one can be *shared*. And
//! the pages come back sorted by index rather than in completion order,
//! because a result that depends on which worker finished first is not a
//! result — `tpdf render --jobs N` protects its stdout the same way.
//!
//! Then it checks the claim rather than asserting it: every page drawn on the
//! pool is compared byte for byte with the same page drawn serially. A
//! concurrency guarantee that nothing compares against is a guarantee nothing
//! has tested.
//!
//! Run: `cargo run -p tinker-pdf --example parallel [-- file.pdf]`

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tinker_pdf::{Document, RenderOptions};

/// What a rendered page is compared by: its size and its pixels.
type Drawn = Option<(u32, u32, Vec<u8>)>;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        format!(
            "{}/../../testdata/outline-3level.pdf",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(1);
    });
    let doc = Document::open(bytes).unwrap_or_else(|e| {
        eprintln!("{path}: {e:?}");
        std::process::exit(1);
    });

    let pages = doc.page_count();
    if pages == 0 {
        eprintln!("{path}: no pages");
        std::process::exit(1);
    }

    // Low, because this is a demonstration of where the threads live and not
    // a benchmark, and a fixture that takes a second per page teaches nothing
    // extra.
    let options = RenderOptions::at_dpi(72.0);

    // A page the tree will not hand back is `None` rather than a panic, in
    // both passes: an example runs against whatever file it is given, and
    // ruling 1 says a damaged one is a result, not a crash.
    let draw = |index: u32| -> Drawn {
        let bitmap = doc.page(index)?.render(&options);
        Some((bitmap.width, bitmap.height, bitmap.data))
    };

    // The serial answer first, so that there is something for the pool to be
    // identical to.
    let clock = Instant::now();
    let serial: Vec<Drawn> = (0..pages).map(draw).collect();
    let serial_ms = clock.elapsed().as_millis();

    // Never more workers than pages: eight threads over six pages is two
    // threads whose whole life is starting and stopping.
    let jobs = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(pages as usize);

    let next = AtomicUsize::new(0);
    let drawn: Mutex<Vec<(usize, Drawn)>> = Mutex::new(Vec::with_capacity(pages as usize));
    let clock = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            // `&doc` — one document, shared. Cloning it per thread would
            // compile just as well and quietly stop exercising `Sync`.
            let (draw, next, drawn) = (&draw, &next, &drawn);
            scope.spawn(move || loop {
                let slot = next.fetch_add(1, Ordering::Relaxed);
                if slot >= pages as usize {
                    return;
                }
                let page = draw(slot as u32);
                drawn.lock().expect("the results lock").push((slot, page));
            });
        }
    });
    let parallel_ms = clock.elapsed().as_millis();

    let mut parallel = drawn.into_inner().expect("the results lock");
    parallel.sort_by_key(|(slot, _)| *slot);
    let parallel: Vec<Drawn> = parallel.into_iter().map(|(_, page)| page).collect();

    println!("document  {path}");
    println!("pages     {pages}");
    println!("threads   {jobs}");
    println!("serial    {serial_ms} ms");
    println!("parallel  {parallel_ms} ms");

    if serial != parallel {
        let differing: Vec<u32> = serial
            .iter()
            .zip(&parallel)
            .enumerate()
            .filter(|(_, (one, other))| one != other)
            .map(|(index, _)| index as u32 + 1)
            .collect();
        eprintln!(
            "pages {differing:?} came back different from the pool, which would be \
             a bug in the engine rather than in this example"
        );
        std::process::exit(1);
    }
    println!(
        "checked   every page byte-identical to the serial render, so the pool \
         changed the clock and nothing else"
    );
}
