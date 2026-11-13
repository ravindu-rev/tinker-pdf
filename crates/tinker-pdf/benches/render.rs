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

mod support;

use std::cell::LazyCell;
use std::hint::black_box;
use std::path::Path;
#[cfg(tinker_planning_baseline)]
use std::sync::atomic::{AtomicUsize, Ordering};

use criterion::{criterion_group, criterion_main, BatchSize, Criterion};
use support::cli::Cli;
use support::{inputs, Group, Per};
#[cfg(not(tinker_planning_baseline))]
use tinker_pdf::ForcedColours;
#[cfg(tinker_planning_baseline)]
use tinker_pdf::PixelRegion;
use tinker_pdf::{
    Bitmap, Document, Page, RenderOptions, RenderWarning, SampleCodec, WriteMode, WriteOptions,
};
#[cfg(not(tinker_planning_baseline))]
use tinker_pdf::{DisplayList, Parallelism};

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

/// `page` at `dpi`, `times` times: the unit is every bitmap's pixels.
fn renders(page: &Page, dpi: f64, times: u64) -> Per {
    let bitmap = page.render(&RenderOptions::at_dpi(dpi));
    match support::pixels(&bitmap) {
        Per::Pixels(pixels) => Per::Pixels(pixels * times),
        other => other,
    }
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

    // ---- vectors ----------------------------------------------------------

    group.operation("250 paths at 150 dpi", |op| {
        let many_page = paths_page(250);
        op.per(renders(&many_page, 150.0, 1));
        op.iter(move || black_box(many_page.render(&RenderOptions::at_dpi(150.0))));
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

    let form = LazyCell::new(|| inputs::first_appearance(&annotated));
    let changed = |grey: f64| {
        let mut editor = annotated.editor();
        editor.put_stream(*form, inputs::square_appearance(&annotated, grey));
        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Incremental,
            ..WriteOptions::default()
        });
        Document::open(bytes).expect("the update reopens")
    };
    let annotated_page = LazyCell::new(|| {
        let annotated_page = annotated.page(0).expect("a page");
        let before = annotated_page.render(&RenderOptions::at_dpi(150.0));
        let after = changed(0.2)
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
    // Each changed file is saved and reopened untimed, and the page drawn
    // after the change is the one timed: one change an iteration.
    group.operation("re-render a changed 100x100 pt annotation", |op| {
        op.per(renders(&annotated_page, 150.0, 1));
        op.iter_batched(
            || changed(0.5),
            |doc| {
                black_box(
                    doc.page(0)
                        .expect("a page")
                        .render(&RenderOptions::at_dpi(150.0)),
                )
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
    // The page opened afresh and drawn once at 72 dpi, untimed, so the timed
    // render at 300 dpi is the second the page sees.
    group.operation(
        "a second render of the quarter-size JPEG page at 300 dpi",
        |op| {
            op.per(renders(&jpeg_quarter, 300.0, 1));
            op.iter_batched(
                || {
                    let page = Document::open(jpeg_quarter_bytes.clone())
                        .expect("it opens")
                        .page(0)
                        .expect("a page");
                    black_box(page.render(&RenderOptions::at_dpi(72.0)));
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
