//! The render group: pages drawn into bitmaps.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `render` list names these operations
//! in this order.
//!
//! Each operation is sized into the set's 20-300 ms window on the hosted
//! runner (`docs/design/performance.md`). One a full page draws too long on
//! draws a part of it: a *half-size* page is half of A4 each way and a
//! *quarter-size* one a quarter, its text, its image or its shading a
//! quarter or a sixteenth of the page's (`inputs::page_part`); fewer paths
//! or pages where the input is a count; and the shorter operations repeated
//! inside an iteration, their unit counting every call. The bitmap toolbox's
//! two scales count the pixels of the render they scale, which the planning
//! baseline's arm draws in their place, and the forced-colour render the
//! default render's, which it draws in that one's; no run has timed the
//! three yet, so the design's render table gives their window as untimed.
//! The coverage rows — the 250 paths under the centre and any-part rules and
//! at a 4-bit graphics level, and the text page at a 4-bit text level — stand
//! beside the default renders of the same pages, which the planning
//! baseline's arm draws in their place, and are untimed too. So are the two
//! separations rows, a page's plates, whose pages the planning baseline's arm
//! draws as their process inks in their place, and the two command stream
//! rows, the text page's stream written and the page drawn from it, in whose
//! place that arm draws the text page at 150 dpi; and so is the page of
//! colour glyphs, the write group's page (`support::colour`), which the
//! planning baseline's arm sets in the face's outlines.

mod support;

use std::cell::LazyCell;
use std::hint::black_box;
use std::path::Path;
#[cfg(tinker_planning_baseline)]
use std::sync::atomic::{AtomicUsize, Ordering};

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use support::cli::Cli;
use support::{colour, inputs, Group, Per};
#[cfg(not(tinker_planning_baseline))]
use tinker_pdf::{
    write::{self, AppendOptions},
    CacheLimits, DisplayList, OpenOptions, Parallelism,
};
use tinker_pdf::{Bitmap, Document, Page, RenderOptions, RenderWarning, SampleCodec};
#[cfg(not(tinker_planning_baseline))]
use tinker_pdf::{CommandStream, ForcedColours, HardEdge, SeparationOptions, Separations};
#[cfg(tinker_planning_baseline)]
use tinker_pdf::{PixelFormat, PixelRegion, WriteMode, WriteOptions};
use tinker_pdf_cos::ObjRef;

/// `bitmap`, the text page's 150-dpi render, resized to `width` by `height`
/// by the bitmap toolbox's scale.
#[cfg(not(tinker_planning_baseline))]
fn scaled(_page: &Page, bitmap: &Bitmap, width: u32, height: u32) -> Bitmap {
    bitmap.scaled(width, height).expect("a render is a picture")
}

/// The planning baseline has no scale: its arm renders the text page at
/// 150 dpi, which made the bitmap, in the scale's place — the operation the
/// gate pairs this one with (`baseline.json`'s `paired_with`).
#[cfg(tinker_planning_baseline)]
fn scaled(page: &Page, _bitmap: &Bitmap, _width: u32, _height: u32) -> Bitmap {
    page.render(&RenderOptions::at_dpi(150.0))
}

/// 150 dpi in forced colours, black on white, with fill-to-stroke.
#[cfg(not(tinker_planning_baseline))]
fn forced_at_150() -> RenderOptions {
    RenderOptions {
        forced_colours: Some(ForcedColours {
            fill_to_stroke: true,
            ..ForcedColours::default()
        }),
        ..RenderOptions::at_dpi(150.0)
    }
}

/// The planning baseline has no forced colours: its arm draws the default
/// render in their place, the operation the gate pairs this one with.
#[cfg(tinker_planning_baseline)]
fn forced_at_150() -> RenderOptions {
    RenderOptions::at_dpi(150.0)
}

/// `page`'s command stream, written (`Page::command_stream`): what the
/// stream write row times.
#[cfg(not(tinker_planning_baseline))]
fn written_stream(page: &Page) -> Vec<u8> {
    page.command_stream()
}

/// The planning baseline has no command stream: its arm renders the page at
/// 150 dpi in the write's place, the operation the gate pairs this one with.
#[cfg(tinker_planning_baseline)]
fn written_stream(page: &Page) -> Bitmap {
    page.render(&RenderOptions::at_dpi(150.0))
}

/// `page`'s command stream, written and parsed, held before a stream row's
/// runs to read back to the bytes it was written as, to be the page's, and to
/// draw at 150 dpi the page's own render, pixel for pixel and warning for
/// warning — so what is timed is a stream that is the page.
#[cfg(not(tinker_planning_baseline))]
fn parsed_stream(page: &Page, what: &str) -> CommandStream {
    let bytes = written_stream(page);
    let stream = CommandStream::parse(&bytes)
        .unwrap_or_else(|error| panic!("{what}: the stream does not read: {error}"));
    assert!(
        stream.to_bytes() == bytes,
        "{what}: the stream reads back to the bytes it was written as"
    );
    assert_eq!(stream.page_index(), 0, "{what}: the stream is the page's");
    let options = RenderOptions::at_dpi(150.0);
    let direct = page.render(&options);
    let replayed = stream.render(&options);
    assert_eq!(
        (replayed.width, replayed.height, replayed.format),
        (direct.width, direct.height, direct.format),
        "{what}: the stream draws the page's bitmap"
    );
    assert!(
        replayed.data == direct.data,
        "{what}: the stream draws the page's pixels"
    );
    assert_eq!(
        replayed.warnings, direct.warnings,
        "{what}: the stream reports what the page does"
    );
    stream
}

/// What a coverage row renders (RC-11.7a): graphics at 0 bits under a rule,
/// or one class at a level, the other anti-aliased.
#[derive(Clone, Copy)]
#[cfg_attr(tinker_planning_baseline, allow(dead_code))]
enum Coverage {
    /// Graphics at 0 bits under the centre rule.
    Centre,
    /// Graphics at 0 bits under the any-part rule.
    AnyPart,
    /// Graphics at this many bits.
    Graphics(u8),
    /// Text at this many bits.
    Text(u8),
}

/// 150 dpi under `coverage`.
#[cfg(not(tinker_planning_baseline))]
fn coverage_at_150(coverage: Coverage) -> RenderOptions {
    let at = RenderOptions::at_dpi(150.0);
    match coverage {
        Coverage::Centre => RenderOptions {
            graphics_bits: 0,
            hard_edge: HardEdge::Centre,
            ..at
        },
        Coverage::AnyPart => RenderOptions {
            graphics_bits: 0,
            hard_edge: HardEdge::AnyPart,
            ..at
        },
        Coverage::Graphics(bits) => RenderOptions {
            graphics_bits: bits,
            ..at
        },
        Coverage::Text(bits) => RenderOptions {
            text_bits: bits,
            ..at
        },
    }
}

/// The planning baseline has no levels or rules: its arm draws the default
/// render in their place, the operation the gate pairs each row with.
#[cfg(tinker_planning_baseline)]
fn coverage_at_150(_coverage: Coverage) -> RenderOptions {
    RenderOptions::at_dpi(150.0)
}

/// Before its runs, a coverage row's render holds the page the default
/// render holds: its size, no warning and about its ink.
#[cfg(not(tinker_planning_baseline))]
fn holds_the_page(page: &Page, options: &RenderOptions, what: &str) {
    let default = page.render(&RenderOptions::at_dpi(150.0));
    let drawn = page.render(options);
    assert_eq!(
        (drawn.width, drawn.height),
        (default.width, default.height),
        "{what}"
    );
    assert!(drawn.warnings.is_empty(), "{what}: {:?}", drawn.warnings);
    assert!(
        inked(&drawn) * 2 > inked(&default),
        "{what} inks {} pixels where the default render inks {}",
        inked(&drawn),
        inked(&default)
    );
}

/// `page` at `dpi`, `times` times: the unit is every bitmap's pixels.
fn renders(page: &Page, dpi: f64, times: u64) -> Per {
    let bitmap = page.render(&RenderOptions::at_dpi(dpi));
    match support::pixels(&bitmap) {
        Per::Pixels(pixels) => Per::Pixels(pixels * times),
        other => other,
    }
}

/// A coverage row of the 250-path page: the page, held to its default
/// render before the runs, and its render at 150 dpi under `coverage`, an
/// iteration a render.
fn paths_under(op: &mut support::Op<'_>, coverage: Coverage, what: &str) {
    let many_page = Document::open(stored("paths-250.pdf", || inputs::paths(250)))
        .expect("the paths open")
        .page(0)
        .expect("a page");
    let options = coverage_at_150(coverage);
    #[cfg(not(tinker_planning_baseline))]
    holds_the_page(&many_page, &options, what);
    #[cfg(tinker_planning_baseline)]
    let _ = what;
    op.per(renders(&many_page, 150.0, 1));
    op.iter(move || black_box(many_page.render(&options)));
}

/// The budget MS-04's second render states, at open
/// (`docs/design/performance.md`, P-16, "MS-05's reading, and a stated
/// budget"): room for everything the quarter-size JPEG page's renders compute
/// — its JPEG at full size, 620 by 877 samples, and the frame beside it — and
/// for its parsed objects, so nothing the second render asks has left.
#[cfg(not(tinker_planning_baseline))]
const SECOND_RENDER_BUDGET: u64 = 64 << 20;

/// `bytes` opened under the second render's budget, stated at open, so that
/// the document keeps what its renders compute across them.
#[cfg(not(tinker_planning_baseline))]
fn under_the_budget(bytes: Vec<u8>) -> Document {
    Document::open_with(
        bytes,
        &OpenOptions::default().with_cache_limits(CacheLimits::new(SECOND_RENDER_BUDGET)),
    )
    .expect("the page opens")
}

/// The planning baseline keeps nothing past a render: its arm opens the page
/// as it is, and its second render decodes everything again.
#[cfg(tinker_planning_baseline)]
fn under_the_budget(bytes: Vec<u8>) -> Document {
    Document::open(bytes).expect("the page opens")
}

/// Before its runs, the second render is held to what it times: drawn after
/// the page's render at 150 dpi under the budget, it decodes no image and
/// misses nothing the document keeps, and it draws what a first render of
/// the page at 300 dpi draws, byte for byte, saying nothing.
#[cfg(not(tinker_planning_baseline))]
fn served_by_the_cache(bytes: &[u8]) {
    let page = under_the_budget(bytes.to_vec()).page(0).expect("a page");
    black_box(page.render(&RenderOptions::at_dpi(150.0)));
    let second = page.render(&RenderOptions {
        stats: true,
        ..RenderOptions::at_dpi(300.0)
    });
    let stats = second.stats.expect("a counted render hands its stats back");
    assert_eq!(
        (stats.images_decoded, stats.cache_misses),
        (0, 0),
        "the second render is served by the document's cache: {stats:?}"
    );
    assert!(stats.cache_hits > 0, "it found what it draws: {stats:?}");
    assert!(
        second.warnings.is_empty(),
        "the page draws cleanly: {:?}",
        second.warnings
    );
    let first = Document::open(bytes.to_vec())
        .expect("the page opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::at_dpi(300.0));
    assert!(
        second.data == first.data,
        "the second render draws the page a first render draws"
    );
}

/// One of the fixtures committed under `fuzz/corpus/render_page/`, opened.
fn fixture(name: &str) -> Document {
    let bytes = inputs::store().bytes(name, || inputs::render_fixture(name));
    Document::open(bytes).expect("the fixture opens")
}

/// The input `name`, stored as `generate` writes it.
fn stored(name: &str, generate: impl FnOnce() -> Vec<u8>) -> Vec<u8> {
    inputs::store().bytes(name, generate)
}

/// How many of a bitmap's pixels are not white paper.
fn inked(bitmap: &Bitmap) -> usize {
    let n = bitmap.components();
    bitmap
        .data
        .chunks(bitmap.stride)
        .flat_map(|row| row.chunks(n).take(bitmap.width as usize))
        .filter(|pixel| pixel.iter().take(3).any(|&v| v < 128))
        .count()
}

/// How many of a bitmap's pixels are a colour rather than a grey: their
/// largest and smallest channels a quarter of the range apart or more.
#[cfg(not(tinker_planning_baseline))]
fn coloured(bitmap: &Bitmap) -> usize {
    let n = bitmap.components();
    bitmap
        .data
        .chunks(bitmap.stride)
        .flat_map(|row| row.chunks(n).take(bitmap.width as usize))
        .filter(|pixel| {
            let channels = &pixel[..3];
            let most = channels.iter().max().copied().unwrap_or(0);
            let least = channels.iter().min().copied().unwrap_or(0);
            most - least >= 64
        })
        .count()
}

/// The name the page of colour glyphs is stored under. The planning
/// baseline's arm of `support::colour` writes the same lines in the face's
/// outlines, so its page is stored under a name of its own, and no build of
/// a paired run reads another build's page.
#[cfg(not(tinker_planning_baseline))]
const COLOUR_PAGE: &str = "colour-glyphs-page.pdf";
#[cfg(tinker_planning_baseline)]
const COLOUR_PAGE: &str = "colour-glyphs-page-in-outlines.pdf";

/// The page of colour glyphs, written from the face's own generator and held
/// before it is timed: its text its lines, drawn at 150 dpi with no warning
/// and its glyphs inked — in the head, in colour.
fn colour_glyphs_page() -> Page {
    let lines = inputs::colour_lines();
    let bytes = stored(COLOUR_PAGE, || {
        colour::colour_page(&inputs::colour_face(), &lines)
    });
    colour::holds_its_lines(&bytes, &lines);
    let page = Document::open(bytes)
        .expect("the page of colour glyphs opens")
        .page(0)
        .expect("a page");
    let drawn = page.render(&RenderOptions::at_dpi(150.0));
    assert!(
        drawn.warnings.is_empty(),
        "the page of colour glyphs draws cleanly: {:?}",
        drawn.warnings
    );
    assert!(
        inked(&drawn) > 100_000,
        "the page inks its glyphs: {} pixels",
        inked(&drawn)
    );
    #[cfg(not(tinker_planning_baseline))]
    assert!(
        coloured(&drawn) * 4 > inked(&drawn),
        "its glyphs are drawn in colour: {} of {} inked pixels",
        coloured(&drawn),
        inked(&drawn)
    );
    page
}

/// `bytes`' first page, its one image decoded whole by `codec` and drawn
/// with no image reported damaged or unsupported.
fn image_page(bytes: Vec<u8>, codec: SampleCodec) -> Page {
    let page = Document::open(bytes)
        .expect("the image page opens")
        .page(0)
        .expect("a page");
    let images = page.images();
    assert_eq!(images.len(), 1, "one image");
    assert_eq!(images[0].codec, codec, "the image is the codec's");
    assert_eq!(images[0].refused, None, "the image decodes");
    let drawn = page.render(&RenderOptions::at_dpi(72.0));
    assert!(
        !drawn.warnings.iter().any(|w| matches!(
            w,
            RenderWarning::UnsupportedImage { .. } | RenderWarning::DamagedImage { .. }
        )),
        "the image drew: {:?}",
        drawn.warnings
    );
    page
}

/// The intents page draws each column in its own intent: its four columns'
/// fills, images and shadings are four colours each, and nothing is reported
/// unsupported, damaged or repaired, so the operation times four intents'
/// work and not one intent's four times.
#[cfg(not(tinker_planning_baseline))]
fn four_intents(page: &Page) {
    let drawn = page.render(&RenderOptions::at_dpi(72.0));
    assert!(
        drawn.warnings.is_empty(),
        "the intents page draws cleanly: {:?}",
        drawn.warnings
    );
    let n = drawn.components();
    let colour = |x: u32, y: u32| {
        let at = y as usize * drawn.stride + x as usize * n;
        drawn.data[at..at + 3].to_vec()
    };
    // The fill's, the image's and the shading's rows at 72 dpi, the page 842
    // points tall and each column 148.75 wide.
    for (row, what) in [(141u32, "fill"), (421, "image"), (701, "shading")] {
        let colours: Vec<Vec<u8>> = (0..4u32)
            .map(|column| colour((148.75 * f64::from(column) + 74.0) as u32, row))
            .collect();
        for first in 0..4 {
            for second in first + 1..4 {
                assert_ne!(
                    colours[first], colours[second],
                    "the {what}s of columns {first} and {second} are one colour"
                );
            }
        }
    }
}

/// The planning baseline reads no intent and draws every column alike.
#[cfg(tinker_planning_baseline)]
fn four_intents(_page: &Page) {}

/// The version 5 image page (`inputs::v5_page`), its one image decoded whole,
/// held before it is timed to draw its picture through its profile: unlike
/// the same samples as `/DeviceRGB`, which is how a picture whose profile the
/// parser refused draws, so the operation times a version 5 conversion and
/// not a copy.
#[cfg(not(tinker_planning_baseline))]
fn v5_image_page() -> Page {
    let page = image_page(
        stored("v5-quarter.pdf", || inputs::v5_page(4, true)),
        SampleCodec::Stream,
    );
    let drawn = page.render(&RenderOptions::at_dpi(72.0));
    let device = Document::open(inputs::v5_page(4, false))
        .expect("the device twin opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::at_dpi(72.0));
    assert_eq!(
        (drawn.width, drawn.height),
        (device.width, device.height),
        "the page and its device twin are one size"
    );
    assert!(
        drawn.data != device.data,
        "the version 5 page draws its picture through its profile, not as /DeviceRGB \
         draws the samples"
    );
    page
}

/// The planning baseline reads no version 5 profile: its arm draws the CMYK
/// page, its own ICC image of the same samples' size, in the v5 page's
/// place, the operation the pair is held against.
#[cfg(tinker_planning_baseline)]
fn v5_image_page() -> Page {
    image_page(
        stored("cmyk-quarter.pdf", || inputs::cmyk_page(4)),
        SampleCodec::Stream,
    )
}

/// `page`'s plates at `dpi`, every colorant a plate of its own
/// (`Page::separations` under the default plan): what the separations rows
/// time.
#[cfg(not(tinker_planning_baseline))]
fn plates_at(page: &Page, dpi: f64) -> Separations {
    page.separations(&SeparationOptions {
        render: RenderOptions::at_dpi(dpi),
        ..SeparationOptions::default()
    })
}

/// The planning baseline has no separations: its arm draws the page as its
/// process inks, `CmykA8`, in their place — the plates of the four process
/// colorants that revision can draw.
#[cfg(tinker_planning_baseline)]
fn plates_at(page: &Page, dpi: f64) -> Bitmap {
    page.render(&RenderOptions {
        format: PixelFormat::CmykA8,
        allow_cmyk: true,
        ..RenderOptions::at_dpi(dpi)
    })
}

/// Before its runs, a separations row's page is held to the plates it
/// times: `plates` of them, every one holding ink, and no warning — so a
/// page whose spots fell back to their alternates, or whose plates were
/// capped, is not what gets timed.
#[cfg(not(tinker_planning_baseline))]
fn holds_its_plates(page: &Page, dpi: f64, plates: usize, what: &str) {
    let drawn = plates_at(page, dpi);
    assert!(
        drawn.warnings.is_empty(),
        "{what} draws its plates cleanly: {:?}",
        drawn.warnings
    );
    assert_eq!(drawn.plates.len(), plates, "{what}: its plates");
    assert!(
        drawn
            .plates
            .iter()
            .all(|plate| plate.tints.iter().any(|tint| *tint != 0)),
        "{what}: every plate holds ink"
    );
}

/// What the four-band operation draws from: the page's display list, kept,
/// which the facade draws in bands on the engine's pool — the retained list
/// tier P times bands over (`docs/design/parallelism.md`).
#[cfg(not(tinker_planning_baseline))]
fn band_source(page: &Page) -> DisplayList {
    page.display_list()
}

/// `list` at `dpi` in four horizontal bands on four threads of the engine's
/// own pool (`DisplayList::render` with `Threads(4)`), written into one
/// bitmap: the page's bytes.
#[cfg(not(tinker_planning_baseline))]
fn in_bands(list: &DisplayList, dpi: f64, _width: u32, _height: u32) -> Vec<u8> {
    list.render(&RenderOptions {
        parallelism: Parallelism::Threads(4),
        ..RenderOptions::at_dpi(dpi)
    })
    .data
}

/// The planning baseline has no bands of its own: its arm draws the page.
#[cfg(tinker_planning_baseline)]
fn band_source(page: &Page) -> Page {
    page.clone()
}

/// The planning baseline has no bands of its own: its arm draws `page` at
/// `dpi` as four horizontal regions, each on a scoped thread of the
/// caller's, and stacks their rows — the caller-thread operation the tier's
/// relation was measured on.
#[cfg(tinker_planning_baseline)]
fn in_bands(page: &Page, dpi: f64, width: u32, height: u32) -> Vec<u8> {
    let band = height.div_ceil(4);
    let bands: Vec<Bitmap> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..4)
            .map(|at| {
                scope.spawn(move || {
                    let top = band * at;
                    let mut options = RenderOptions::at_dpi(dpi);
                    options.region = Some(PixelRegion::new(
                        0,
                        top,
                        width,
                        band.min(height.saturating_sub(top)),
                    ));
                    page.render(&options)
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().expect("a band renders"))
            .collect()
    });
    bands.into_iter().flat_map(|band| band.data).collect()
}

/// The size `bytes`' first page renders to at `dpi`, which `tpdf render`
/// writes its PNG at.
fn rendered_size(bytes: &[u8], dpi: f64) -> (u32, u32) {
    let bitmap = Document::open(bytes.to_vec())
        .expect("it opens")
        .page(0)
        .expect("a page")
        .render(&RenderOptions::at_dpi(dpi));
    (bitmap.width, bitmap.height)
}

/// Every page of `doc` at `dpi` on `threads` threads of the engine's own
/// pool: the facade's page walk (`Document::render_pages`), each page drawn
/// on the worker that takes it and handed back in page order.
#[cfg(not(tinker_planning_baseline))]
fn every_page_on(doc: &Document, dpi: f64, threads: usize) -> usize {
    let options = RenderOptions {
        parallelism: Parallelism::Threads(threads),
        ..RenderOptions::at_dpi(dpi)
    };
    let mut drawn = 0usize;
    doc.render_pages(0..doc.page_count(), &options, |_, bitmap| {
        if let Some(bitmap) = bitmap {
            black_box(bitmap);
            drawn += 1;
        }
    });
    drawn
}

/// The planning baseline has no page walk: its arm draws every page of `doc`
/// at `dpi` on `jobs` scoped threads of the caller's, each taking the next
/// page as it frees — the serial and caller-thread operations the tier's
/// factors were measured on.
#[cfg(tinker_planning_baseline)]
fn every_page_on(doc: &Document, dpi: f64, jobs: usize) -> usize {
    let pages = doc.page_count() as usize;
    let next = AtomicUsize::new(0);
    let drawn = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let (next, drawn) = (&next, &drawn);
            scope.spawn(move || loop {
                let slot = next.fetch_add(1, Ordering::Relaxed);
                if slot >= pages {
                    return;
                }
                if let Some(page) = doc.page(slot as u32) {
                    black_box(page.render(&RenderOptions::at_dpi(dpi)));
                    drawn.fetch_add(1, Ordering::Relaxed);
                }
            });
        }
    });
    drawn.into_inner()
}

/// What a change of the annotated text page hands back: the update appended
/// and the document reopened over it (`write::Appended`), which patches a
/// page of it.
#[cfg(not(tinker_planning_baseline))]
type Changed = write::Appended;

/// The planning baseline has no edit-and-reopen door: its arm's change is
/// the document opened again from the incremental save.
#[cfg(tinker_planning_baseline)]
type Changed = Document;

/// `annotated` with its square's appearance, `form`, replaced by `grey`'s,
/// appended through the facade's edit-and-reopen door (`write::append`).
#[cfg(not(tinker_planning_baseline))]
fn change(annotated: &Document, form: ObjRef, grey: f64) -> Changed {
    let mut editor = annotated.editor();
    editor.put_stream(form, inputs::square_appearance(annotated, grey));
    write::append(annotated, &editor, &AppendOptions::default()).expect("the update reopens")
}

/// The planning baseline's change: saved as an incremental update and the
/// bytes opened, which reads the file whole.
#[cfg(tinker_planning_baseline)]
fn change(annotated: &Document, form: ObjRef, grey: f64) -> Changed {
    let mut editor = annotated.editor();
    editor.put_stream(form, inputs::square_appearance(annotated, grey));
    let bytes = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });
    Document::open(bytes).expect("the update reopens")
}

/// The document a change made.
#[cfg(not(tinker_planning_baseline))]
fn changed_document(changed: &Changed) -> &Document {
    &changed.document
}

/// The document a change made: the planning baseline's change is one.
#[cfg(tinker_planning_baseline)]
fn changed_document(changed: &Changed) -> &Document {
    changed
}

/// The changed page drawn again over `bitmap`, its render before the
/// change: patched in place by the rectangle the change made
/// (`Appended::patch`), so no other bitmap is drawn.
#[cfg(not(tinker_planning_baseline))]
fn redraw(changed: &Changed, bitmap: &mut Bitmap, options: &RenderOptions) -> Option<Bitmap> {
    black_box(changed.patch(0, bitmap, options));
    None
}

/// The planning baseline cannot patch a page: its arm draws the changed page
/// whole, leaving the bitmap before the change as it was.
#[cfg(tinker_planning_baseline)]
fn redraw(changed: &Changed, _before: &mut Bitmap, options: &RenderOptions) -> Option<Bitmap> {
    Some(changed.page(0).expect("a page").render(options))
}

fn render(c: &mut Criterion) {
    let page = LazyCell::new(|| {
        Document::open(stored("three-pages.pdf", inputs::three_pages))
            .expect("it opens")
            .page(0)
            .expect("a page")
    });
    let shaded_page = LazyCell::new(|| {
        Document::open(stored("axial.pdf", inputs::axial))
            .expect("it opens")
            .page(0)
            .expect("a page")
    });
    let shaded_half = LazyCell::new(|| {
        Document::open(stored("axial-half.pdf", || inputs::axial_part(2)))
            .expect("it opens")
            .page(0)
            .expect("a page")
    });

    // The dense page embeds its face, so unlike the seven's text page every
    // glyph is an outline the rasteriser fills.
    let text_page = LazyCell::new(|| {
        let text_page = Document::open(stored("dense-1.pdf", || inputs::dense(1, 0)))
            .expect("the text page opens")
            .page(0)
            .expect("a page");
        let drawn = text_page.render(&RenderOptions::at_dpi(150.0));
        assert!(
            !drawn.warnings.contains(&RenderWarning::UnreadableFont),
            "the text page's face is read: {:?}",
            drawn.warnings
        );
        assert!(
            inked(&drawn) > 50_000,
            "the text page inks its glyphs: {} pixels",
            inked(&drawn)
        );
        text_page
    });
    let paths_page = |count: usize| {
        Document::open(stored(&format!("paths-{count}.pdf"), || {
            inputs::paths(count)
        }))
        .expect("the paths open")
        .page(0)
        .expect("a page")
    };
    let mesh_page = LazyCell::new(|| fixture("mesh-shadings.pdf").page(0).expect("a page"));
    let eight = LazyCell::new(|| {
        let eight = Document::open(stored("dense-half-8.pdf", || inputs::dense_part(8, 0, 2)))
            .expect("the dense pages open");
        assert_eq!(every_page_on(&eight, 150.0, 4), 8, "every page draws");
        eight
    });
    // Written from the dense page's own generator, not from the stored one,
    // so a process timing one of its operations writes its own input alone.
    let annotated = LazyCell::new(|| {
        Document::open(stored("dense-1-annotated.pdf", || {
            inputs::annotated(&inputs::dense(1, 0))
        }))
        .expect("the annotated page")
    });
    let jpeg_quarter_bytes =
        LazyCell::new(|| stored("jpeg-quarter.pdf", || inputs::jpeg_page_part(4)));
    let jpeg_quarter = LazyCell::new(|| image_page(jpeg_quarter_bytes.clone(), SampleCodec::Dct));
    let half_text = LazyCell::new(|| stored("dense-half-1.pdf", || inputs::dense_part(1, 0, 2)));
    let quarter_text =
        LazyCell::new(|| stored("dense-quarter-1.pdf", || inputs::dense_part(1, 0, 4)));
    let quarter_eight =
        LazyCell::new(|| stored("dense-quarter-8.pdf", || inputs::dense_part(8, 0, 4)));
    let half_pdf = LazyCell::new(|| inputs::store().file("dense-half-1.pdf", || half_text.clone()));
    let quarter_pdf =
        LazyCell::new(|| inputs::store().file("dense-quarter-1.pdf", || quarter_text.clone()));
    let eight_pdf =
        LazyCell::new(|| inputs::store().file("dense-quarter-8.pdf", || quarter_eight.clone()));

    let mut group = Group::new(c, "render");

    // No glyph of this page is drawn in the default build: see
    // `inputs::triangles`.
    group.operation("render text at 150 dpi", |op| {
        op.per(support::pixels(&page.render(&RenderOptions::at_dpi(150.0))));
        op.iter(|| black_box(page.render(&RenderOptions::at_dpi(150.0))));
    });

    group.operation("render a full-page axial shading at 150 dpi", |op| {
        op.per(support::pixels(
            &shaded_page.render(&RenderOptions::at_dpi(150.0)),
        ));
        op.iter(|| black_box(shaded_page.render(&RenderOptions::at_dpi(150.0))));
    });

    group.operation("fill 300 anti-aliased paths at 150 dpi", |op| {
        let paths_page = Document::open(stored("triangles.pdf", inputs::triangles))
            .expect("it opens")
            .page(0)
            .expect("a page");
        op.per(support::pixels(
            &paths_page.render(&RenderOptions::at_dpi(150.0)),
        ));
        op.iter(move || black_box(paths_page.render(&RenderOptions::at_dpi(150.0))));
    });

    // ---- the text page, its glyphs drawn ---------------------------------

    group.operation("the text page at 72 dpi", |op| {
        op.per(renders(&text_page, 72.0, 2));
        op.iter(|| {
            for _ in 0..2 {
                black_box(text_page.render(&RenderOptions::at_dpi(72.0)));
            }
        });
    });
    group.operation("the text page at 150 dpi", |op| {
        op.per(renders(&text_page, 150.0, 2));
        op.iter(|| {
            for _ in 0..2 {
                black_box(text_page.render(&RenderOptions::at_dpi(150.0)));
            }
        });
    });
    group.operation("the text page at 300 dpi", |op| {
        op.per(renders(&text_page, 300.0, 1));
        op.iter(|| black_box(text_page.render(&RenderOptions::at_dpi(300.0))));
    });
    group.operation("record the text page's display list", |op| {
        op.per(Per::Pages(5));
        op.iter(|| {
            for _ in 0..5 {
                black_box(text_page.display_list());
            }
        });
    });

    // ---- the bitmap toolbox and forced colours ---------------------------

    // The text page's 150-dpi render, drawn once untimed and scaled a call an
    // iteration. The unit is the render's pixels, which the planning
    // baseline's arm renders in the scale's place.
    let at_150 = LazyCell::new(|| text_page.render(&RenderOptions::at_dpi(150.0)));
    group.operation("the 150-dpi text page scaled to a half", |op| {
        let (text_page, at_150): (&Page, &Bitmap) = (&text_page, &at_150);
        let (width, height) = (at_150.width / 2, at_150.height / 2);
        #[cfg(not(tinker_planning_baseline))]
        {
            let half = scaled(text_page, at_150, width, height);
            assert_eq!(
                (half.width, half.height),
                (width, height),
                "a half each way"
            );
            assert!(inked(&half) > 10_000, "the glyphs survive the scale");
        }
        op.per(support::pixels(at_150));
        op.iter(move || black_box(scaled(text_page, at_150, width, height)));
    });
    group.operation("the 150-dpi text page scaled to two thirds", |op| {
        let (text_page, at_150): (&Page, &Bitmap) = (&text_page, &at_150);
        let (width, height) = (at_150.width * 2 / 3, at_150.height * 2 / 3);
        #[cfg(not(tinker_planning_baseline))]
        {
            let smaller = scaled(text_page, at_150, width, height);
            assert_eq!(
                (smaller.width, smaller.height),
                (width, height),
                "two thirds each way"
            );
            assert!(inked(&smaller) > 10_000, "the glyphs survive the scale");
        }
        op.per(support::pixels(at_150));
        op.iter(move || black_box(scaled(text_page, at_150, width, height)));
    });
    // Beside the default render of the same page, which the planning
    // baseline's arm draws in its place: two renders an iteration, as that
    // one makes.
    group.operation(
        "the text page at 150 dpi in forced colours, fill to stroke",
        |op| {
            #[cfg(not(tinker_planning_baseline))]
            {
                let forced = text_page.render(&forced_at_150());
                assert_eq!((forced.width, forced.height), (at_150.width, at_150.height));
                assert!(
                    forced
                        .data
                        .chunks_exact(3)
                        .all(|pixel| pixel[0] == pixel[1] && pixel[1] == pixel[2]),
                    "black on white is grey"
                );
                assert!(
                    inked(&forced) > 10_000,
                    "the glyphs, still filled, ink the page"
                );
            }
            op.per(renders(&text_page, 150.0, 2));
            op.iter(|| {
                for _ in 0..2 {
                    black_box(text_page.render(&forced_at_150()));
                }
            });
        },
    );

    // The text page beside its default render, which the planning baseline's
    // arm draws in its place: its glyphs' masks at a 4-bit text level, two
    // renders an iteration as that one makes.
    group.operation("the text page at 150 dpi at a 4-bit text level", |op| {
        let text_page: &Page = &text_page;
        let options = coverage_at_150(Coverage::Text(4));
        #[cfg(not(tinker_planning_baseline))]
        holds_the_page(text_page, &options, "the text page at a 4-bit text level");
        op.per(renders(text_page, 150.0, 2));
        op.iter(move || {
            for _ in 0..2 {
                black_box(text_page.render(&options));
            }
        });
    });

    // ---- the command stream -----------------------------------------------

    // RC-18's stream of the text page: the page interpreted and written as
    // versioned bytes, with every glyph outline its runs name, two writes an
    // iteration. The unit is the 150-dpi render's pixels, two of them, because
    // the planning baseline, which has no command stream, renders the page at
    // 150 dpi twice in its place: the operation the gate pairs this one with.
    group.operation("write the 150-dpi text page's command stream", |op| {
        let text_page: &Page = &text_page;
        #[cfg(not(tinker_planning_baseline))]
        parsed_stream(text_page, "the text page's command stream");
        op.per(renders(text_page, 150.0, 2));
        op.iter(move || {
            for _ in 0..2 {
                black_box(written_stream(text_page));
            }
        });
    });
    // The same stream, written and parsed once untimed, drawn at 150 dpi
    // (`CommandStream::render`) beside the page's direct render, which the
    // planning baseline's arm draws in its place: two renders an iteration, as
    // that one makes.
    group.operation(
        "the text page at 150 dpi from its parsed command stream",
        |op| {
            let text_page: &Page = &text_page;
            #[cfg(not(tinker_planning_baseline))]
            let source = parsed_stream(text_page, "the text page's parsed command stream");
            #[cfg(tinker_planning_baseline)]
            let source = text_page;
            op.per(renders(text_page, 150.0, 2));
            op.iter(move || {
                for _ in 0..2 {
                    black_box(source.render(&RenderOptions::at_dpi(150.0)));
                }
            });
        },
    );

    // ---- vectors ----------------------------------------------------------

    group.operation("250 paths at 150 dpi", |op| {
        let many_page = paths_page(250);
        op.per(renders(&many_page, 150.0, 1));
        op.iter(move || black_box(many_page.render(&RenderOptions::at_dpi(150.0))));
    });

    // ---- coverage levels and rules ---------------------------------------

    // The 250 paths beside their default render, which the planning
    // baseline's arm draws in each one's place: at 0 bits under the centre
    // rule, which reads one sub-scanline a row; under the any-part rule,
    // which also walks every edge over every row it reaches; and at a 4-bit
    // level, the coverage quantised.
    group.operation("250 paths at 150 dpi under the centre rule", |op| {
        paths_under(op, Coverage::Centre, "the 250 paths under the centre rule");
    });
    group.operation("250 paths at 150 dpi under the any-part rule", |op| {
        paths_under(
            op,
            Coverage::AnyPart,
            "the 250 paths under the any-part rule",
        );
    });
    group.operation("250 paths at 150 dpi at a 4-bit graphics level", |op| {
        paths_under(
            op,
            Coverage::Graphics(4),
            "the 250 paths at a 4-bit graphics level",
        );
    });
    group.operation("100 paths at 300 dpi", |op| {
        let many_page = paths_page(100);
        op.per(renders(&many_page, 300.0, 1));
        op.iter(move || black_box(many_page.render(&RenderOptions::at_dpi(300.0))));
    });
    // The 250 paths' curves: two in five of them.
    group.operation("the 100 stroked curves alone at 150 dpi", |op| {
        let curves_page = Document::open(stored("curves-100.pdf", || inputs::curves(100)))
            .expect("the curves open")
            .page(0)
            .expect("a page");
        op.per(renders(&curves_page, 150.0, 1));
        op.iter(move || black_box(curves_page.render(&RenderOptions::at_dpi(150.0))));
    });

    // ---- shadings, the committed fixtures and tiling ----------------------

    group.operation("an axial shading over a half-size page at 300 dpi", |op| {
        op.per(renders(&shaded_half, 300.0, 1));
        op.iter(|| black_box(shaded_half.render(&RenderOptions::at_dpi(300.0))));
    });

    group.operation("the mesh shadings fixture at 150 dpi", |op| {
        op.per(renders(&mesh_page, 150.0, 7));
        op.iter(|| {
            for _ in 0..7 {
                black_box(mesh_page.render(&RenderOptions::at_dpi(150.0)));
            }
        });
    });
    group.operation("the mesh shadings fixture at 300 dpi", |op| {
        op.per(renders(&mesh_page, 300.0, 3));
        op.iter(|| {
            for _ in 0..3 {
                black_box(mesh_page.render(&RenderOptions::at_dpi(300.0)));
            }
        });
    });

    group.operation("the transparency fixture at 150 dpi", |op| {
        let glass_page = fixture("transparency.pdf").page(0).expect("a page");
        op.per(renders(&glass_page, 150.0, 9));
        op.iter(move || {
            for _ in 0..9 {
                black_box(glass_page.render(&RenderOptions::at_dpi(150.0)));
            }
        });
    });

    // The fifty strips cover exactly what the one rectangle covers, so the
    // two pages paint the same area through the same pattern.
    group.operation("a page tiled by one pattern fill at 150 dpi", |op| {
        let once_page = Document::open(stored("tiling-1.pdf", || inputs::tiling(1)))
            .expect("the tiled page opens")
            .page(0)
            .expect("a page");
        op.per(renders(&once_page, 150.0, 1));
        op.iter(move || black_box(once_page.render(&RenderOptions::at_dpi(150.0))));
    });
    group.operation("a page tiled by 50 pattern fills at 150 dpi", |op| {
        let fifty_page = Document::open(stored("tiling-50.pdf", || inputs::tiling(50)))
            .expect("the tiled page opens")
            .page(0)
            .expect("a page");
        op.per(renders(&fifty_page, 150.0, 1));
        op.iter(move || black_box(fifty_page.render(&RenderOptions::at_dpi(150.0))));
    });

    // ---- threads ----------------------------------------------------------

    // Ruling 5: the four bands, put back together, are the page.
    group.operation("the 300-dpi text page in 4 bands on 4 threads", |op| {
        let text_page: &Page = &text_page;
        let whole = text_page.render(&RenderOptions::at_dpi(300.0));
        let (width, height) = (whole.width, whole.height);
        let source = band_source(text_page);
        assert!(
            in_bands(&source, 300.0, width, height) == whole.data,
            "the four bands are the whole page byte for byte"
        );
        op.per(renders(text_page, 300.0, 1));
        op.iter(move || black_box(in_bands(&source, 300.0, width, height)));
    });

    group.operation("8 half-size text pages at 150 dpi on 1 thread", |op| {
        op.per(Per::Pages(8));
        op.iter(|| black_box(every_page_on(&eight, 150.0, 1)));
    });
    group.operation("8 half-size text pages at 150 dpi on 4 threads", |op| {
        op.per(Per::Pages(8));
        op.iter(|| black_box(every_page_on(&eight, 150.0, 4)));
    });

    // ---- an annotation changed --------------------------------------------

    // The square's appearance replaced and appended through the facade's
    // edit-and-reopen door (`change`), which hands back the document reopened
    // over the update and patches a page of it.
    let form = LazyCell::new(|| inputs::first_appearance(&annotated));
    let changed = |grey: f64| change(&annotated, *form, grey);
    let annotated_page = LazyCell::new(|| {
        let annotated_page = annotated.page(0).expect("a page");
        let before = annotated_page.render(&RenderOptions::at_dpi(150.0));
        let after = changed_document(&changed(0.2))
            .page(0)
            .expect("a page")
            .render(&RenderOptions::at_dpi(150.0));
        assert!(before.data != after.data, "the changed appearance draws");
        annotated_page
    });
    group.operation("the annotated text page at 150 dpi", |op| {
        op.per(renders(&annotated_page, 150.0, 2));
        op.iter(|| {
            for _ in 0..2 {
                black_box(annotated_page.render(&RenderOptions::at_dpi(150.0)));
            }
        });
    });
    // Each change is appended and reopened untimed, beside a copy of the
    // bitmap `the annotated text page at 150 dpi` renders, and what is timed
    // is the patch of that bitmap (`redraw`): the 100 x 100 pt square's
    // rectangle rendered and written over it, one change an iteration.
    // Untimed first: the patch takes the page, and is the page drawn whole
    // after the change.
    group.operation("re-render a changed 100x100 pt annotation", |op| {
        let options = RenderOptions::at_dpi(150.0);
        let before = annotated_page.render(&options);
        let appended = changed(0.5);
        #[cfg(not(tinker_planning_baseline))]
        assert_eq!(
            appended
                .patch(0, &mut before.clone(), &options)
                .expect("the changed page")
                .fallback,
            None,
            "the page is patched, not drawn whole"
        );
        let mut patched = before.clone();
        let redrawn = redraw(&appended, &mut patched, &options).unwrap_or(patched);
        let full = changed_document(&appended)
            .page(0)
            .expect("a page")
            .render(&options);
        assert!(
            redrawn.data == full.data && redrawn.warnings == full.warnings,
            "the patch is the full render after the change"
        );
        op.per(renders(&annotated_page, 150.0, 1));
        let changed = &changed;
        op.iter_batched(
            move || (changed(0.5), before.clone()),
            move |(changed, mut bitmap)| {
                let drawn = redraw(&changed, &mut bitmap, &options);
                black_box((bitmap, drawn))
            },
            BatchSize::PerIteration,
        );
    });

    // ---- the image pages ----------------------------------------------------

    // The display list is recorded once and its renders share the decoded
    // image, so what is timed is the replay.
    group.operation(
        "replay the half-size Flate RGB image page at 150 dpi",
        |op| {
            let flate_half = image_page(
                stored("flate-half.pdf", || inputs::flate_page(2)),
                SampleCodec::Stream,
            );
            let replayed = flate_half.display_list();
            op.per(renders(&flate_half, 150.0, 1));
            op.iter(move || black_box(replayed.render(&RenderOptions::at_dpi(150.0))));
        },
    );
    // Drawn at the image's own resolution, so each sample is converted once:
    // the unit is the image's samples.
    group.calibrated(
        "a quarter-size ICC-based CMYK image to sRGB at 300 dpi",
        |op| {
            let cmyk_page = image_page(
                stored("cmyk-quarter.pdf", || inputs::cmyk_page(4)),
                SampleCodec::Stream,
            );
            let (width, height) = inputs::scan_part(4);
            op.per(Per::Pixels(u64::from(width) * u64::from(height)));
            op.iter(move || black_box(cmyk_page.render(&RenderOptions::at_dpi(300.0))));
        },
    );
    // The CMYK page's samples as RGB under `/ICCBased` over a version 5
    // profile of multi-process elements (`inputs::v5_page`), drawn at the
    // image's own resolution so each sample is converted once through the
    // pipeline's grid: the unit is the image's samples. The planning
    // baseline reads no version 5 profile, so its arm draws the CMYK page,
    // its own ICC image of as many samples, in this one's place.
    group.operation(
        "an ICC-based image to sRGB through a v5 multi-process profile",
        |op| {
            let v5_page = v5_image_page();
            let (width, height) = inputs::scan_part(4);
            op.per(Per::Pixels(u64::from(width) * u64::from(height)));
            op.iter(move || black_box(v5_page.render(&RenderOptions::at_dpi(300.0))));
        },
    );
    // RC-01's page at A4 (`inputs::intents_page`): under each of Table 69's
    // intents a fill, an image and an axial shading in one ICC space, so each
    // intent's transform is compiled, the image decoded once in each and each
    // shading's pixels converted through its own. The planning baseline,
    // which reads no intent, draws the same page.
    group.operation(
        "render an ICC page under each rendering intent at 150 dpi",
        |op| {
            let intents = Document::open(stored("icc-intents.pdf", inputs::intents_page))
                .expect("the intents page opens")
                .page(0)
                .expect("a page");
            four_intents(&intents);
            op.per(renders(&intents, 150.0, 1));
            op.iter(move || black_box(intents.render(&RenderOptions::at_dpi(150.0))));
        },
    );

    // ---- separations ------------------------------------------------------

    // RC-04.16's fixture (`inputs::separations_page`): four spots on their
    // plates through fills, text, two groups, an image and a stencil, a
    // shading and a mesh, the page `separations.rs` fingerprints; its eight
    // plates a call. The unit is the page's pixels at 150 dpi. The planning
    // baseline has no separations, so its arm draws the page as its process
    // inks in their place.
    group.operation("the separations fixture's plates at 150 dpi", |op| {
        let fixture = Document::open(stored("separations.pdf", inputs::separations_page))
            .expect("the separations fixture opens")
            .page(0)
            .expect("a page");
        #[cfg(not(tinker_planning_baseline))]
        holds_its_plates(&fixture, 150.0, 8, "the separations fixture");
        op.per(renders(&fixture, 150.0, 1));
        op.iter(move || black_box(plates_at(&fixture, 150.0)));
    });
    // RC-04.15's page (`inputs::spots_page(64)`): A4 whose sixteen `/DeviceN`
    // spaces name sixty-four spots, each filling a square of its own, so its
    // sixty-eight plates, the cap's, a call at 72 dpi; the planning
    // baseline's arm draws its process inks in their place.
    group.operation("the 64-spot page's 68 plates at 72 dpi", |op| {
        let spots = Document::open(stored("spots-64.pdf", || inputs::spots_page(64)))
            .expect("the 64-spot page opens")
            .page(0)
            .expect("a page");
        #[cfg(not(tinker_planning_baseline))]
        holds_its_plates(&spots, 72.0, 68, "the 64-spot page");
        op.per(renders(&spots, 72.0, 1));
        op.iter(move || black_box(plates_at(&spots, 72.0)));
    });
    group.operation("the half-size JPEG page at 72 dpi", |op| {
        let jpeg_half = image_page(
            stored("jpeg-half.pdf", || inputs::jpeg_page_part(2)),
            SampleCodec::Dct,
        );
        op.per(renders(&jpeg_half, 72.0, 1));
        op.iter(move || black_box(jpeg_half.render(&RenderOptions::at_dpi(72.0))));
    });
    group.operation("the quarter-size JPEG page at 300 dpi", |op| {
        op.per(renders(&jpeg_quarter, 300.0, 1));
        op.iter(|| black_box(jpeg_quarter.render(&RenderOptions::at_dpi(300.0))));
    });
    // The page opened afresh under the budget the operation states and drawn
    // once at 150 dpi, untimed, so the timed render at 300 dpi is the second
    // the page sees and is served by the document's cache: at both
    // resolutions the JPEG is drawn larger than half its samples, so both
    // draws ask its full-size decode, which the first keeps
    // (`docs/design/performance.md`, P-16, "What a second render at another
    // dpi reuses"). The planning baseline has no cache, and its arm draws the
    // page a second time decoding everything again.
    group.operation(
        "a second render of the quarter-size JPEG page at 300 dpi under a 64 MiB budget",
        |op| {
            op.per(renders(&jpeg_quarter, 300.0, 1));
            #[cfg(not(tinker_planning_baseline))]
            served_by_the_cache(&jpeg_quarter_bytes);
            op.iter_batched(
                || {
                    let page = under_the_budget(jpeg_quarter_bytes.clone())
                        .page(0)
                        .expect("a page");
                    black_box(page.render(&RenderOptions::at_dpi(150.0)));
                    page
                },
                |page| black_box(page.render(&RenderOptions::at_dpi(300.0))),
                BatchSize::PerIteration,
            );
        },
    );
    group.operation("the quarter-size JPEG 2000 page at 72 dpi", |op| {
        let jpx_page = image_page(
            stored("jpx-quarter.pdf", inputs::jpx_page_part),
            SampleCodec::Jpx,
        );
        op.per(renders(&jpx_page, 72.0, 1));
        op.iter(move || black_box(jpx_page.render(&RenderOptions::at_dpi(72.0))));
    });
    group.operation("the half-size G4 scan page at 150 dpi", |op| {
        let g4_page = image_page(
            stored("g4-half.pdf", || inputs::g4_page(2)),
            SampleCodec::CcittFax,
        );
        op.per(renders(&g4_page, 150.0, 1));
        op.iter(move || black_box(g4_page.render(&RenderOptions::at_dpi(150.0))));
    });
    group.operation("the half-size JBIG2 scan page at 150 dpi", |op| {
        let jbig2_page = image_page(
            stored("jbig2-half.pdf", || inputs::jbig2_page(2)),
            SampleCodec::Jbig2,
        );
        op.per(renders(&jbig2_page, 150.0, 1));
        op.iter(move || black_box(jbig2_page.render(&RenderOptions::at_dpi(150.0))));
    });
    group.operation("the quarter-size Flate RGB image page at 72 dpi", |op| {
        let flate_quarter = image_page(
            stored("flate-quarter.pdf", || inputs::flate_page(4)),
            SampleCodec::Stream,
        );
        op.per(renders(&flate_quarter, 72.0, 1));
        op.iter(move || black_box(flate_quarter.render(&RenderOptions::at_dpi(72.0))));
    });

    // The write group's page of colour glyphs (FT-06.17a): 25 lines of 21
    // capitals of `inputs::colour_face()`, each a Type 3 glyph whose
    // procedure fills its disc with a radial gradient, its eyes with a solid
    // and its mouth with a linear gradient, every glyph drawn where it stands.
    // The planning baseline, which has no colour glyphs, draws the same lines
    // in the face's outlines in its place.
    group.operation("render a page of colour glyphs at 150 dpi", |op| {
        let colour_page = colour_glyphs_page();
        op.per(renders(&colour_page, 150.0, 1));
        op.iter(move || black_box(colour_page.render(&RenderOptions::at_dpi(150.0))));
    });

    // ---- through the CLI: the PNGs `tpdf render` writes ------------------

    group.operation("the half-size text page to PNG at 150 dpi, CLI", |op| {
        let (width, height) = rendered_size(&half_text, 150.0);
        let half_pdf: &Path = &half_pdf;
        let at_150 = Cli::new("render-text-150");
        let out_150 = at_150.out("png");
        let (ran, written) = (at_150.clone(), out_150.clone());
        op.per(Per::Pixels(u64::from(width) * u64::from(height)));
        op.iter(move || at_150.run(&[&"render", &half_pdf, &"--out", &out_150, &"--dpi", &"150"]));
        op.after(move || {
            assert_eq!(
                ran.pngs(&written),
                [("dense-half-1-0001.png".to_string(), width, height)],
                "one PNG, the page at 150 dpi"
            );
        });
    });
    group.operation("the quarter-size text page to PNG at 300 dpi, CLI", |op| {
        let (width, height) = rendered_size(&quarter_text, 300.0);
        let quarter_pdf: &Path = &quarter_pdf;
        let at_300 = Cli::new("render-text-300");
        let out_300 = at_300.out("png");
        let (ran, written) = (at_300.clone(), out_300.clone());
        op.per(Per::Pixels(u64::from(width) * u64::from(height)));
        op.iter(move || {
            at_300.run(&[
                &"render",
                &quarter_pdf,
                &"--out",
                &out_300,
                &"--dpi",
                &"300",
            ])
        });
        op.after(move || {
            assert_eq!(
                ran.pngs(&written),
                [("dense-quarter-1-0001.png".to_string(), width, height)],
                "one PNG, the page at 300 dpi"
            );
        });
    });
    group.operation("8 quarter-size pages to PNG at 150 dpi, 1 job, CLI", |op| {
        let quarter_eight: &[u8] = &quarter_eight;
        let eight_pdf: &Path = &eight_pdf;
        let serial = Cli::new("render-8-jobs-1");
        let out_serial = serial.out("png");
        let (ran, written) = (serial.clone(), out_serial.clone());
        op.per(Per::Pages(8));
        op.iter(move || {
            serial.run(&[
                &"render",
                &eight_pdf,
                &"--out",
                &out_serial,
                &"--dpi",
                &"150",
                &"--jobs",
                &"1",
            ])
        });
        op.after(move || assert_eight_pngs(&ran, &written, quarter_eight));
    });
    group.operation(
        "8 quarter-size pages to PNG at 150 dpi, 4 jobs, CLI",
        |op| {
            let quarter_eight: &[u8] = &quarter_eight;
            let eight_pdf: &Path = &eight_pdf;
            let parallel = Cli::new("render-8-jobs-4");
            let out_parallel = parallel.out("png");
            let (ran, written) = (parallel.clone(), out_parallel.clone());
            op.per(Per::Pages(8));
            op.iter(move || {
                parallel.run(&[
                    &"render",
                    &eight_pdf,
                    &"--out",
                    &out_parallel,
                    &"--dpi",
                    &"150",
                    &"--jobs",
                    &"4",
                ])
            });
            op.after(move || assert_eight_pngs(&ran, &written, quarter_eight));
        },
    );

    group.finish();
}

/// The eight pages' render wrote a PNG a page, in order, each the page at
/// 150 dpi.
fn assert_eight_pngs(cli: &Cli, out: &std::path::Path, bytes: &[u8]) {
    let size = rendered_size(bytes, 150.0);
    let pngs = cli.pngs(out);
    assert_eq!(pngs.len(), 8, "a PNG a page");
    for (at, (name, width, height)) in pngs.iter().enumerate() {
        assert_eq!(name, &format!("dense-quarter-8-{:04}.png", at + 1));
        assert_eq!((*width, *height), size, "{name} is the page at 150 dpi");
    }
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = render
}
criterion_main!(benches);
