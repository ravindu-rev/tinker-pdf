//! Render every page on the engine's own pool, and prove the pool changed
//! nothing.
//!
//! `Document::render_pages` renders each page as `RenderOptions` asks on the
//! threads its `parallelism` names — `Parallelism::Serial` unless a caller asks
//! — and hands the bitmaps back in page order on the calling thread. The pool
//! is the engine's: threads started in a scope for the call and joined before
//! it returns, no global runtime, and on wasm32, where no thread starts, the
//! same pages drawn on the caller. The walk shares the one document by
//! reference across its workers, and each worker takes its pages through
//! `Document::page`, which hands back an owned `Page`.
//!
//! Then it checks the claim rather than asserting it: every page the walk drew
//! is compared byte for byte with the same page drawn serially. A concurrency
//! guarantee that nothing compares against is a guarantee nothing has tested.
//!
//! Run: `cargo run -p tinker-pdf --example parallel [-- file.pdf]`

use std::time::Instant;

use tinker_pdf::{Document, Parallelism, RenderOptions};

/// What a rendered page is compared by: its size, its pixels and its
/// warnings.
type Drawn = Option<(u32, u32, Vec<u8>, Vec<tinker_pdf::RenderWarning>)>;

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
    let serial_options = RenderOptions::at_dpi(72.0);

    // A page the tree will not hand back is `None` rather than a panic, in
    // both passes: an example runs against whatever file it is given, and
    // ruling 1 says a damaged one is a result, not a crash.
    let drawn = |bitmap: Option<tinker_pdf::Bitmap>| -> Drawn {
        bitmap.map(|bitmap| (bitmap.width, bitmap.height, bitmap.data, bitmap.warnings))
    };

    // The serial answer first, so that there is something for the pool to be
    // identical to.
    let clock = Instant::now();
    let serial: Vec<Drawn> = (0..pages)
        .map(|index| drawn(doc.page(index).map(|page| page.render(&serial_options))))
        .collect();
    let serial_ms = clock.elapsed().as_millis();

    // The walk never starts more workers than pages, so asking for every core
    // costs nothing on a short document.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let walked_options = RenderOptions {
        parallelism: Parallelism::Threads(threads),
        ..serial_options.clone()
    };
    let started = Parallelism::Threads(threads).workers(pages as usize);

    let clock = Instant::now();
    let mut parallel: Vec<Drawn> = Vec::with_capacity(pages as usize);
    doc.render_pages(0..pages, &walked_options, |_, bitmap| {
        parallel.push(drawn(bitmap));
    });
    let parallel_ms = clock.elapsed().as_millis();

    println!("document  {path}");
    println!("pages     {pages}");
    println!("threads   {started}");
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
            "pages {differing:?} came back different from the engine's page walk, which \
             would be a bug in the engine rather than in this example"
        );
        std::process::exit(1);
    }
    println!(
        "checked   every page byte-identical to the serial render, warnings included, so \
         the pool changed the clock and nothing else"
    );
}
