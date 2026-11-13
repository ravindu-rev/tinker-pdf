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

// ---- The subset walk of a save (P-15.4) ---------------------------------------

/// The `/Widths` the embedded face's fonts carry.
fn widths() -> String {
    format!("[320{}]", " 640".repeat(94))
}

/// A font dictionary over the embedded face, `/BaseFont` `base_font`, its
/// descriptor `descriptor`.
fn face_font(base_font: &str, descriptor: u32) -> String {
    format!(
        "<< /Type /Font /Subtype /TrueType /BaseFont /{base_font} /FirstChar 32 \
         /LastChar 126 /Widths {} /Encoding /WinAnsiEncoding /FontDescriptor {descriptor} 0 R >>",
        widths()
    )
}

/// A descriptor of the embedded face whose program is object `program`.
fn face_descriptor(program: u32) -> String {
    format!(
        "<< /Type /FontDescriptor /FontName /Curvy /Flags 32 /FontBBox [0 0 1000 1000] \
         /ItalicAngle 0 /Ascent 800 /Descent -200 /CapHeight 700 /StemV 80 \
         /FontFile2 {program} 0 R >>"
    )
}

/// The subset walk's document: six pages of 200 by 100 points whose pages
/// share fonts. Page 0 shows `F1`; page 1 `F1` and `F2`, a second font over a
/// program of its own; page 2 `F1` and `F3`, a font written directly into its
/// resource dictionary as `/DirectA`; page 3 a line in `F1` that the test
/// redacts; page 4 `F3` again, written directly as `/DirectB` over the same
/// program as `/DirectA`; page 5 fills with a tiling pattern whose cell shows
/// `F1`, and carries a square whose appearance shows `F2`.
///
/// 1 the catalog, 2 the page tree, 3 to 8 the pages, 9 to 14 their contents,
/// 15 to 17 `F1`, its descriptor and its program, 18 to 20 `F2`'s, 21 the
/// pattern, 22 the square and 23 its appearance, 24 and 25 the direct fonts'
/// descriptor and program.
fn subset_document() -> Vec<u8> {
    use guard_support::{deflated, face, object, pdf, stream};

    let direct = |base_font: &str| face_font(base_font, 24);
    let pages: [(String, String); 6] = [
        (
            "/Font << /F1 15 0 R >>".to_string(),
            "BT /F1 12 Tf 10 50 Td (Shared face, page one) Tj ET".to_string(),
        ),
        (
            "/Font << /F1 15 0 R /F2 18 0 R >>".to_string(),
            "BT /F1 12 Tf 10 60 Td (One face) Tj /F2 10 Tf 0 -20 Td (and another) Tj ET"
                .to_string(),
        ),
        (
            format!("/Font << /F1 15 0 R /F3 {} >>", direct("DirectA")),
            "BT /F3 12 Tf 10 60 Td (Written directly) Tj /F1 9 Tf 0 -20 Td (beside it) Tj ET"
                .to_string(),
        ),
        (
            "/Font << /F1 15 0 R >>".to_string(),
            "BT /F1 12 Tf 10 50 Td (Redacted words here) Tj ET".to_string(),
        ),
        (
            format!("/Font << /F3 {} >>", direct("DirectB")),
            "BT /F3 14 Tf 10 50 Td (Direct once more) Tj ET".to_string(),
        ),
        (
            "/Font << /F1 15 0 R >> /Pattern << /P0 21 0 R >>".to_string(),
            "/Pattern cs /P0 scn 10 10 180 80 re f".to_string(),
        ),
    ];
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 6 /Kids [3 0 R 4 0 R 5 0 R 6 0 R 7 0 R 8 0 R] >>"),
    ];
    for (at, (resources, _)) in pages.iter().enumerate() {
        let annots = if at == 5 { " /Annots [22 0 R]" } else { "" };
        objects.push(object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] \
             /Resources << {resources} >> /Contents {} 0 R{annots} >>",
            9 + at
        )));
    }
    for (_, content) in &pages {
        objects.push(deflated("", content.as_bytes()));
    }
    // 15 to 17.
    objects.extend(face(15));
    // 18 to 20.
    objects.push(object(&face_font("Curvy", 19)));
    objects.push(object(&face_descriptor(20)));
    objects.push(deflated("", &render_support::curvy_font()));
    // 21: the tiling pattern.
    objects.push(stream(
        "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 40 20] \
         /XStep 40 /YStep 20 /Resources << /Font << /F1 15 0 R >> >>",
        b"BT /F1 8 Tf 2 6 Td (cell) Tj ET",
    ));
    // 22 and 23: the square and its appearance.
    objects.push(object(
        "<< /Type /Annot /Subtype /Square /Rect [10 10 90 40] /F 4 /AP << /N 23 0 R >> >>",
    ));
    objects.push(stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 80 30] /Resources << /Font << /F2 18 0 R >> >>",
        b"BT /F2 9 Tf 2 10 Td (Appearance text) Tj ET",
    ));
    // 24 and 25: what the direct fonts embed.
    objects.push(object(&face_descriptor(25)));
    objects.push(deflated("", &render_support::curvy_font()));
    pdf(&objects)
}

/// An editor over [`subset_document`] with page 3's line redacted.
fn redacted_editor() -> tinker_pdf::DocumentEditor {
    let document = Document::open(subset_document()).expect("the subset document opens");
    let mut editor = document.editor();
    let report = tinker_pdf::redact::apply(
        &mut editor,
        3,
        &[tinker_pdf::redact::Redaction {
            area: tinker_pdf::Rect {
                x0: 0.0,
                y0: 40.0,
                x1: 200.0,
                y1: 70.0,
            },
            mark: true,
        }],
    )
    .expect("page 3 is there");
    assert!(report.glyphs > 0, "the redaction cut page 3's line");
    editor
}

/// The editor's document saved as `options` asks.
fn saved_with(options: &tinker_pdf::write::SaveOptions) -> tinker_pdf::write::Saved {
    tinker_pdf::write::save(&mut redacted_editor(), options)
}

/// **A subset walk on 2, 3, 4 and 7 threads cuts what the serial walk cuts.**
///
/// The document's pages share two font objects, one redacted, one filled
/// with a tiling pattern whose cell shows text, one carrying an appearance
/// that shows text, and two write fonts directly into their resource
/// dictionaries over one program. Saved with the subset walk on 2, 3, 4 and
/// 7 threads, every save is the serial save byte for byte, and its subset
/// report — what was cut, what was left whole and why — is the serial
/// report: the direct program is reported under `/DirectA`, the first direct
/// font in page order to name it, never under `/DirectB`.
#[test]
fn a_subset_walk_on_2_3_4_and_7_threads_cuts_what_the_serial_walk_cuts() {
    use tinker_pdf::write::SaveOptions;
    use tinker_pdf::UntouchedReason;

    let serial = saved_with(&SaveOptions::default());
    let report = serial.fonts.report().expect("the font pass ran");
    let direct: Vec<&str> = report
        .untouched
        .iter()
        .filter(|untouched| untouched.reason == UntouchedReason::NotAnObject)
        .map(|untouched| untouched.base_font.as_str())
        .collect();
    assert_eq!(
        direct,
        ["DirectA"],
        "the direct program, under the first font that named it"
    );
    let programs: HashSet<u32> = report
        .subsetted
        .iter()
        .map(|cut| cut.program.num)
        .chain(
            report
                .untouched
                .iter()
                .map(|untouched| untouched.program.num),
        )
        .collect();
    assert_eq!(
        programs,
        HashSet::from([17, 20, 25]),
        "every program is accounted for"
    );

    for threads in [2usize, 3, 4, 7] {
        let walked = saved_with(&SaveOptions {
            parallelism: Parallelism::Threads(threads),
            ..SaveOptions::default()
        });
        assert!(
            walked.bytes == serial.bytes,
            "{threads} threads: the saved file differs"
        );
        assert_eq!(walked.fonts, serial.fonts, "{threads} threads: the report");
        assert_eq!(walked.images, serial.images, "{threads} threads: images");
    }
}

/// **A subset walk on four threads walks no page on the caller.** Saved with
/// stats asked, the walk on four threads counts its six pages on the pool's
/// workers and none on the caller — on `wasm32-wasip1` all six on the caller
/// — and takes the serial save's steps: the same work, every stream decoded
/// at most once though pages read them side by side. The serial save counts
/// no part, and a save asked for threads over one page walks it on the
/// caller.
#[test]
fn a_subset_walk_on_four_threads_walks_no_page_on_the_caller() {
    use tinker_pdf::write::SaveOptions;

    let counted = |parallelism| {
        saved_with(&SaveOptions {
            stats: true,
            parallelism,
            ..SaveOptions::default()
        })
        .stats
        .expect("asked")
    };
    let serial = counted(Parallelism::Serial);
    assert_eq!(placed(&serial), (0, 0), "a serial save counts no part");

    let four = counted(Parallelism::Threads(4));
    assert_eq!(
        placed(&four),
        if THREADS_START { (6, 0) } else { (0, 6) },
        "where the six pages were walked"
    );
    assert_eq!(four.work(), serial.work(), "the serial save's steps");
    assert_eq!(four.streams_decoded, serial.streams_decoded);
    assert_eq!(
        (four.most_decodes_of_one_stream, four.most_decoded_stream),
        (
            serial.most_decodes_of_one_stream,
            serial.most_decoded_stream
        ),
        "no stream decoded more often than serially, though pages read them side by side"
    );

    let one_page = Document::open(guard_support::document(0, 1)).expect("one page opens");
    let saved = tinker_pdf::write::save(
        &mut one_page.editor(),
        &SaveOptions {
            stats: true,
            parallelism: Parallelism::Threads(4),
            ..SaveOptions::default()
        },
    );
    assert_eq!(
        placed(&saved.stats.expect("asked")),
        (0, 1),
        "one page is the caller's, the pool starting no thread for it"
    );
}

// ---- Single-flight fills (P-15.6) -------------------------------------------
//
// The caches a display list's renders share fill once however many renders
// miss a key at once (`docs/design/parallelism.md`, "Single-flight fills").
// Counted through each render's own stats and summed: the decodes, extractions
// and builds a set of concurrent renders made are what one render made.

/// A page whose caches its renders share: a 320 by 240 4:2:0 JPEG drawn at
/// half its size, a Flate RGB image whose data is cut short, which decodes
/// what it has and warns, and a form with resources of its own showing a line
/// of text in the embedded face. 1 the catalog, 2 the page tree, 3 the page,
/// 4 its content, 5 the JPEG, 6 the cut image, 7 the form, 8 to 10 the face.
fn shared_caches_document() -> Vec<u8> {
    use guard_support::{deflated, face, object, pdf, stream};

    let jpeg = render_support::minified_baseline_jpeg(320, 240);
    let noise: Vec<u8> = (0..24u32 * 18 * 3)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();
    let mut cut = tinker_pdf_filters::zlib_compress(&noise);
    cut.truncate(cut.len() * 2 / 3);
    let content = "q 160 0 0 120 4 4 cm /Jp Do Q q 60 0 0 45 170 100 cm /Fl Do Q \
                   q 1 0 0 1 170 20 cm /Fm Do Q";
    let mut objects = vec![
        object("<< /Type /Catalog /Pages 2 0 R >>"),
        object("<< /Type /Pages /Count 1 /Kids [3 0 R] >>"),
        object(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 240 160] \
             /Resources << /XObject << /Jp 5 0 R /Fl 6 0 R /Fm 7 0 R >> >> /Contents 4 0 R >>",
        ),
        deflated("", content.as_bytes()),
        stream(
            "/Type /XObject /Subtype /Image /Width 320 /Height 240 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /DCTDecode",
            &jpeg,
        ),
        stream(
            "/Type /XObject /Subtype /Image /Width 24 /Height 18 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /FlateDecode",
            &cut,
        ),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 70 40] \
             /Resources << /Font << /F1 8 0 R >> >>",
            b"BT /F1 12 Tf 2 12 Td (Shared) Tj ET",
        ),
    ];
    objects.extend(face(8));
    pdf(&objects)
}

/// `render` four times at once — on four threads that start it together at a
/// barrier, where threads start — the results in thread order. On
/// `wasm32-wasip1`, where none starts, four times in turn on the caller.
fn four_at_once<T: Send>(render: impl Fn() -> T + Sync) -> Vec<T> {
    if !THREADS_START {
        return (0..4).map(|_| render()).collect();
    }
    let start = Barrier::new(4);
    thread::scope(|scope| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    start.wait();
                    render()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("no render panicked"))
            .collect()
    })
}

/// The sum of four renders' stats.
fn summed(bitmaps: &[Bitmap]) -> Stats {
    bitmaps
        .iter()
        .map(|bitmap| bitmap.stats.expect("each render counted"))
        .fold(Stats::default(), |sum, stats| sum + stats)
}

/// **Four renders of one display list decode a shared image once.** Four
/// threads replay one list at once, meeting at a barrier before each asks for
/// the page's JPEG and its cut Flate image: between them they decode each
/// once — two misses and two decodes, the other six lookups hits — and the
/// JPEG's samples are transformed once. Each render is the page drawn
/// directly, pixels and warnings: the cut image's damage, said by the render
/// that decoded it, is said again by each that met its decode, waiting on it
/// or finding it published. The form's scope, built when the list was
/// recorded, is found by all four.
#[test]
fn four_renders_of_one_display_list_decode_a_shared_image_once() {
    let doc = Document::open(shared_caches_document()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let direct = page.render(&RenderOptions::default());
    assert!(
        direct
            .warnings
            .iter()
            .any(|warning| matches!(warning, RenderWarning::DamagedImage { .. })),
        "the cut image is named as damaged: {:?}",
        direct.warnings
    );
    let counted = RenderOptions {
        stats: true,
        ..RenderOptions::default()
    };

    // One replay alone, of a list of its own: what one render decodes.
    let alone = page.display_list().render(&counted);
    let alone = alone.stats.expect("counted");
    assert_eq!(
        (alone.image_misses, alone.images_decoded, alone.image_hits),
        (2, 2, 0)
    );
    assert!(alone.jpeg_samples_transformed > 0);

    let list = page.display_list();
    let renders = four_at_once(|| list.render(&counted));
    for (index, bitmap) in renders.iter().enumerate() {
        assert!(
            drawn(bitmap) == drawn(&direct),
            "render {index} is not the page drawn directly: {:?} against {:?}",
            bitmap.warnings,
            direct.warnings
        );
    }
    let sum = summed(&renders);
    assert_eq!(
        (sum.image_misses, sum.images_decoded, sum.image_hits),
        (2, 2, 6),
        "each image decoded once between the four"
    );
    assert_eq!(
        sum.jpeg_samples_transformed, alone.jpeg_samples_transformed,
        "the JPEG transformed once"
    );
    assert_eq!(
        (sum.form_scope_misses, sum.form_scope_hits),
        (0, 4),
        "the form's scope, built at the recording, found by each"
    );
}

/// **Four renders extract a shared glyph once**: the form's line of text
/// replayed by four renders at once is extracted, code by code, once between
/// them — the misses one replay alone makes, the rest hits — and its font's
/// program, which only an extraction looks up, is looked up as often as one
/// replay looks it up and read at least once. The program's misses are the
/// one figure a schedule can move: an extraction, its fill open, that meets a
/// program another thread is reading reads it for itself rather than wait
/// (the design's rule 3), so they are bounded by the extractions, not one.
#[test]
fn four_renders_of_one_display_list_extract_a_shared_glyph_once() {
    let doc = Document::open(shared_caches_document()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let counted = RenderOptions {
        stats: true,
        ..RenderOptions::default()
    };
    let alone = page.display_list().render(&counted);
    let alone = alone.stats.expect("counted");
    let lookups = alone.glyph_outline_misses + alone.glyph_outline_hits;
    assert_eq!(
        alone.glyph_outline_misses, 6,
        "six letters, each a code of its own: {alone:?}"
    );

    let list = page.display_list();
    let sum = summed(&four_at_once(|| list.render(&counted)));
    assert_eq!(
        sum.glyph_outline_misses, alone.glyph_outline_misses,
        "each glyph extracted once between the four"
    );
    assert_eq!(
        sum.glyph_outline_hits,
        4 * lookups - alone.glyph_outline_misses
    );
    assert_eq!(
        sum.font_program_misses + sum.font_program_hits,
        alone.font_program_misses + alone.font_program_hits,
        "the program is looked up by the extractions alone, one replay's worth"
    );
    assert!(
        (1..=alone.glyph_outline_misses).contains(&sum.font_program_misses),
        "the program read at least once and at most once an extraction: {}",
        sum.font_program_misses
    );
}

/// **A same-thread re-entry never waits on itself**, driven through the fill
/// itself (`tinker_pdf_cos::Flights`, which the facade's caches are): a fill
/// that reads its own key again computes the inner value and publishes
/// nothing, the outer value the one kept; and across two maps, as the
/// facade's nest — a form scope's fill reading an image whose fill reads the
/// form scope again — the innermost read computes rather than waits, and
/// each outer value is kept. Run on a thread of its own with a deadline,
/// where threads start, so a wait on itself fails rather than hangs.
#[test]
fn a_same_thread_reentry_never_waits_on_itself() {
    use std::time::Duration;
    use tinker_pdf_cos::{Filled, Flights};

    fn reenter() -> (u32, u32, u32, u32, u32) {
        let images: Flights<Vec<u8>, u32> = Flights::new();
        let outer = images.read(b"Im0".to_vec(), || {
            let inner = images.read(b"Im0".to_vec(), || Filled::Keep(7));
            assert!(
                !images.is_ready(&b"Im0".to_vec()),
                "the re-entry published nothing"
            );
            Filled::Keep(inner + 1)
        });
        let scopes: Flights<u64, u32> = Flights::new();
        let nested = scopes.read(1, || {
            let image = images.read(b"Im1".to_vec(), || {
                let again = scopes.read(1, || Filled::Keep(100));
                Filled::Keep(again + 10)
            });
            Filled::Keep(image + 1)
        });
        (
            outer,
            images.read(b"Im0".to_vec(), || Filled::Keep(0)),
            nested,
            scopes.read(1, || Filled::Keep(0)),
            images.read(b"Im1".to_vec(), || Filled::Keep(0)),
        )
    }

    let got = if THREADS_START {
        let (send, receive) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = send.send(reenter());
        });
        receive
            .recv_timeout(Duration::from_secs(60))
            .expect("a re-entry waited on itself and never finished")
    } else {
        reenter()
    };
    assert_eq!(
        got,
        (8, 8, 111, 111, 110),
        "each re-entry computed its own, and each outer value was kept"
    );
}

// ---- Bands of one page (P-15.7) ----------------------------------------------

/// **A four-band render runs no band on the caller.** A page of text drawn
/// from its display list, and through `Page::render`, on four threads is cut
/// into four bands — no run of images stands on it to move a cut — and its
/// stats say all four ran on the pool's workers and none on the caller, on
/// `wasm32-wasip1`, where no thread starts, all four on the caller; whatever
/// the schedule, since a band counts where it ran. The page is the page drawn
/// whole, and a render asked for no threads counts no band.
#[test]
fn a_four_band_render_runs_no_band_on_the_caller() {
    let doc = walked(1);
    let page = doc.page(0).expect("a page");
    let list = page.display_list();
    let whole = page.render(&RenderOptions::default());
    let expected = if THREADS_START { (4, 0) } else { (0, 4) };
    for (door, banded) in [
        (
            "DisplayList::render",
            list.render(&counting(Parallelism::Threads(4))),
        ),
        (
            "Page::render",
            page.render(&counting(Parallelism::Threads(4))),
        ),
    ] {
        assert!(
            drawn(&banded) == drawn(&whole),
            "{door}: the bands are the page"
        );
        assert_eq!(
            placed(&banded.stats.expect("asked")),
            expected,
            "{door}: four bands, (on workers, on the caller)"
        );
    }
    let serial = list.render(&counting(Parallelism::Serial));
    assert_eq!(placed(&serial.stats.expect("asked")), (0, 0), "no band");
    let one = list.render(&counting(Parallelism::Threads(1)));
    assert_eq!(
        placed(&one.stats.expect("asked")),
        (0, 0),
        "one thread, one band"
    );
}
