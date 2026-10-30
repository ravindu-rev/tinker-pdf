//! The render group: pages drawn into bitmaps.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `render` list names these operations
//! in this order.

mod support;

use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};

use criterion::{criterion_group, criterion_main, BatchSize, Criterion, SamplingMode};
use support::cli::{self, Cli};
use support::{inputs, Per};
use tinker_pdf::{
    Bitmap, Document, Page, PixelRegion, RenderOptions, RenderWarning, SampleCodec, WriteMode,
    WriteOptions,
};

/// `page` at `dpi`, `times` times: the unit is every bitmap's pixels.
fn renders(page: &Page, dpi: f64, times: u64) -> Per {
    let bitmap = page.render(&RenderOptions::at_dpi(dpi));
    match support::pixels(&bitmap) {
        Per::Pixels(pixels) => Per::Pixels(pixels * times),
        other => other,
    }
}

/// The first page of a fixture under `fuzz/corpus/render_page/`.
fn fixture(name: &str) -> Document {
    let path = format!(
        "{}/../../fuzz/corpus/render_page/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("the fixture {path}: {error}"));
    Document::open(bytes).expect("the fixture opens")
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

/// `page` at `dpi` in four horizontal bands, each rendered on its own scoped
/// thread through `RenderOptions::region`, and put back together.
fn in_bands(page: &Page, dpi: f64, width: u32, height: u32) -> Vec<Bitmap> {
    let band = height.div_ceil(4);
    std::thread::scope(|scope| {
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
    })
}

/// Every page of `doc` at `dpi` on `jobs` scoped threads, each taking the
/// next page as it frees, as `examples/parallel.rs` does.
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
    let doc = Document::open(inputs::three_pages()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let shaded = Document::open(inputs::axial()).expect("it opens");
    let shaded_page = shaded.page(0).expect("a page");
    let paths = Document::open(inputs::triangles()).expect("it opens");
    let paths_page = paths.page(0).expect("a page");

    let mut group = c.benchmark_group("render");
    group.sampling_mode(SamplingMode::Flat);

    // No glyph of this page is drawn in the default build: see
    // `inputs::triangles`.
    group.throughput(support::pixels(&page.render(&RenderOptions::at_dpi(150.0))).throughput());
    group.bench_function("render text at 150 dpi", |b| {
        b.iter(|| black_box(page.render(&RenderOptions::at_dpi(150.0))));
    });

    group.throughput(
        support::pixels(&shaded_page.render(&RenderOptions::at_dpi(150.0))).throughput(),
    );
    group.bench_function("render a full-page axial shading at 150 dpi", |b| {
        b.iter(|| black_box(shaded_page.render(&RenderOptions::at_dpi(150.0))));
    });

    group.throughput(
        support::pixels(&paths_page.render(&RenderOptions::at_dpi(150.0))).throughput(),
    );
    group.bench_function("fill 300 anti-aliased paths at 150 dpi", |b| {
        b.iter(|| black_box(paths_page.render(&RenderOptions::at_dpi(150.0))));
    });

    // ---- the text page, its glyphs drawn ---------------------------------

    // The dense page embeds its face, so unlike the seven's text page every
    // glyph is an outline the rasteriser fills.
    let text = Document::open(inputs::dense(1, 0)).expect("the text page opens");
    let text_page = text.page(0).expect("a page");
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

    group.throughput(renders(&text_page, 72.0, 5).throughput());
    group.bench_function("the text page at 72 dpi", |b| {
        b.iter(|| {
            for _ in 0..5 {
                black_box(text_page.render(&RenderOptions::at_dpi(72.0)));
            }
        });
    });
    group.throughput(renders(&text_page, 150.0, 2).throughput());
    group.bench_function("the text page at 150 dpi", |b| {
        b.iter(|| {
            for _ in 0..2 {
                black_box(text_page.render(&RenderOptions::at_dpi(150.0)));
            }
        });
    });
    group.throughput(renders(&text_page, 300.0, 1).throughput());
    group.bench_function("the text page at 300 dpi", |b| {
        b.iter(|| black_box(text_page.render(&RenderOptions::at_dpi(300.0))));
    });
    group.throughput(Per::Pages(21).throughput());
    group.bench_function("record the text page's display list", |b| {
        b.iter(|| {
            for _ in 0..21 {
                black_box(text_page.display_list());
            }
        });
    });

    // ---- vectors ----------------------------------------------------------

    let many = Document::open(inputs::paths(5000)).expect("the paths open");
    let many_page = many.page(0).expect("a page");
    let curves = Document::open(inputs::curves(2000)).expect("the curves open");
    let curves_page = curves.page(0).expect("a page");
    group.throughput(renders(&many_page, 150.0, 1).throughput());
    group.bench_function("5000 paths at 150 dpi", |b| {
        b.iter(|| black_box(many_page.render(&RenderOptions::at_dpi(150.0))));
    });
    group.throughput(renders(&many_page, 300.0, 1).throughput());
    group.bench_function("5000 paths at 300 dpi", |b| {
        b.iter(|| black_box(many_page.render(&RenderOptions::at_dpi(300.0))));
    });
    group.throughput(renders(&curves_page, 150.0, 1).throughput());
    group.bench_function("the 2000 stroked curves alone at 150 dpi", |b| {
        b.iter(|| black_box(curves_page.render(&RenderOptions::at_dpi(150.0))));
    });

    // ---- shadings, the committed fixtures and tiling ----------------------

    group.throughput(renders(&shaded_page, 300.0, 1).throughput());
    group.bench_function("a full-page axial shading at 300 dpi", |b| {
        b.iter(|| black_box(shaded_page.render(&RenderOptions::at_dpi(300.0))));
    });

    let mesh = fixture("mesh-shadings.pdf");
    let mesh_page = mesh.page(0).expect("a page");
    group.throughput(renders(&mesh_page, 150.0, 7).throughput());
    group.bench_function("the mesh shadings fixture at 150 dpi", |b| {
        b.iter(|| {
            for _ in 0..7 {
                black_box(mesh_page.render(&RenderOptions::at_dpi(150.0)));
            }
        });
    });
    group.throughput(renders(&mesh_page, 300.0, 3).throughput());
    group.bench_function("the mesh shadings fixture at 300 dpi", |b| {
        b.iter(|| {
            for _ in 0..3 {
                black_box(mesh_page.render(&RenderOptions::at_dpi(300.0)));
            }
        });
    });

    let glass = fixture("transparency.pdf");
    let glass_page = glass.page(0).expect("a page");
    group.throughput(renders(&glass_page, 150.0, 9).throughput());
    group.bench_function("the transparency fixture at 150 dpi", |b| {
        b.iter(|| {
            for _ in 0..9 {
                black_box(glass_page.render(&RenderOptions::at_dpi(150.0)));
            }
        });
    });

    // The fifty strips cover exactly what the one rectangle covers, so the
    // two pages paint the same area through the same pattern.
    let once = Document::open(inputs::tiling(1)).expect("the tiled page opens");
    let once_page = once.page(0).expect("a page");
    let fifty = Document::open(inputs::tiling(50)).expect("the tiled page opens");
    let fifty_page = fifty.page(0).expect("a page");
    group.throughput(renders(&once_page, 150.0, 1).throughput());
    group.bench_function("a page tiled by one pattern fill at 150 dpi", |b| {
        b.iter(|| black_box(once_page.render(&RenderOptions::at_dpi(150.0))));
    });
    group.throughput(renders(&fifty_page, 150.0, 1).throughput());
    group.bench_function("a page tiled by 50 pattern fills at 150 dpi", |b| {
        b.iter(|| black_box(fifty_page.render(&RenderOptions::at_dpi(150.0))));
    });

    // ---- threads ----------------------------------------------------------

    // Ruling 5: the four bands, put back together, are the page.
    let whole = text_page.render(&RenderOptions::at_dpi(300.0));
    let bands = in_bands(&text_page, 300.0, whole.width, whole.height);
    let mut rows: Vec<u8> = Vec::with_capacity(whole.data.len());
    for band in &bands {
        assert_eq!(band.stride, whole.stride, "a band is the page's width");
        rows.extend_from_slice(&band.data);
    }
    assert!(
        rows == whole.data,
        "the four bands reassemble to the whole page byte for byte"
    );
    group.throughput(renders(&text_page, 300.0, 3).throughput());
    group.bench_function("the 300-dpi text page in 4 bands on 4 threads", |b| {
        b.iter(|| {
            for _ in 0..3 {
                black_box(in_bands(&text_page, 300.0, whole.width, whole.height));
            }
        });
    });

    let hundred = Document::open(inputs::dense(100, 0)).expect("the dense pages open");
    assert_eq!(every_page_on(&hundred, 150.0, 4), 100, "every page draws");
    group.throughput(Per::Pages(100).throughput());
    group.bench_function("100 text pages at 150 dpi on 1 thread", |b| {
        b.iter(|| black_box(every_page_on(&hundred, 150.0, 1)));
    });
    group.bench_function("100 text pages at 150 dpi on 4 threads", |b| {
        b.iter(|| black_box(every_page_on(&hundred, 150.0, 4)));
    });

    // ---- an annotation changed --------------------------------------------

    let annotated =
        Document::open(inputs::annotated(&inputs::dense(1, 0))).expect("the annotated page");
    let annotated_page = annotated.page(0).expect("a page");
    let form = inputs::first_appearance(&annotated);
    let changed = |grey: f64| {
        let mut editor = annotated.editor();
        editor.put_stream(form, inputs::square_appearance(&annotated, grey));
        let bytes = editor.save(&WriteOptions {
            mode: WriteMode::Incremental,
            ..WriteOptions::default()
        });
        Document::open(bytes).expect("the update reopens")
    };
    let before = annotated_page.render(&RenderOptions::at_dpi(150.0));
    let after = changed(0.2)
        .page(0)
        .expect("a page")
        .render(&RenderOptions::at_dpi(150.0));
    assert!(before.data != after.data, "the changed appearance draws");
    group.throughput(renders(&annotated_page, 150.0, 2).throughput());
    group.bench_function("the annotated text page at 150 dpi", |b| {
        b.iter(|| {
            for _ in 0..2 {
                black_box(annotated_page.render(&RenderOptions::at_dpi(150.0)));
            }
        });
    });
    // Each changed file is saved and reopened untimed, and the page drawn
    // after the change is the one timed: three changes an iteration.
    group.throughput(renders(&annotated_page, 150.0, 3).throughput());
    group.bench_function("re-render a changed 100x100 pt annotation", |b| {
        b.iter_batched(
            || [changed(0.2), changed(0.5), changed(0.8)],
            |documents| {
                for doc in &documents {
                    black_box(
                        doc.page(0)
                            .expect("a page")
                            .render(&RenderOptions::at_dpi(150.0)),
                    );
                }
            },
            BatchSize::PerIteration,
        );
    });

    // ---- the image pages ----------------------------------------------------

    let flate_page = image_page(inputs::flate_page(), SampleCodec::Stream);
    let cmyk_page = image_page(inputs::cmyk_page(), SampleCodec::Stream);
    let jpeg_bytes = inputs::jpeg_page();
    let jpeg_page = image_page(jpeg_bytes.clone(), SampleCodec::Dct);
    let jpx_page = image_page(inputs::jpx_page(), SampleCodec::Jpx);
    let g4_page = image_page(inputs::g4_page(), SampleCodec::CcittFax);
    let jbig2_page = image_page(inputs::jbig2_page(), SampleCodec::Jbig2);

    // The display list is recorded once and its renders share the decoded
    // image, so what is timed is the replay.
    let replayed = flate_page.display_list();
    group.throughput(renders(&flate_page, 150.0, 1).throughput());
    group.bench_function("replay the Flate RGB image page at 150 dpi", |b| {
        b.iter(|| black_box(replayed.render(&RenderOptions::at_dpi(150.0))));
    });
    // Drawn at the image's own resolution, so each sample is converted once:
    // the unit is the image's samples.
    let (width, height) = inputs::SCAN;
    group.throughput(Per::Pixels(u64::from(width) * u64::from(height)).throughput());
    group.bench_function("an ICC-based CMYK image to sRGB at 300 dpi", |b| {
        b.iter(|| black_box(cmyk_page.render(&RenderOptions::at_dpi(300.0))));
    });
    group.throughput(renders(&jpeg_page, 72.0, 1).throughput());
    group.bench_function("the JPEG page at 72 dpi", |b| {
        b.iter(|| black_box(jpeg_page.render(&RenderOptions::at_dpi(72.0))));
    });
    group.throughput(renders(&jpeg_page, 300.0, 1).throughput());
    group.bench_function("the JPEG page at 300 dpi", |b| {
        b.iter(|| black_box(jpeg_page.render(&RenderOptions::at_dpi(300.0))));
    });
    // The page opened afresh and drawn once at 72 dpi, untimed, so the timed
    // render at 300 dpi is the second the page sees.
    group.bench_function("a second render of the JPEG page at 300 dpi", |b| {
        b.iter_batched(
            || {
                let page = Document::open(jpeg_bytes.clone())
                    .expect("it opens")
                    .page(0)
                    .expect("a page");
                black_box(page.render(&RenderOptions::at_dpi(72.0)));
                page
            },
            |page| black_box(page.render(&RenderOptions::at_dpi(300.0))),
            BatchSize::PerIteration,
        );
    });
    group.throughput(renders(&jpx_page, 72.0, 1).throughput());
    group.bench_function("the JPEG 2000 page at 72 dpi", |b| {
        b.iter(|| black_box(jpx_page.render(&RenderOptions::at_dpi(72.0))));
    });
    group.throughput(renders(&g4_page, 150.0, 1).throughput());
    group.bench_function("the G4 scan page at 150 dpi", |b| {
        b.iter(|| black_box(g4_page.render(&RenderOptions::at_dpi(150.0))));
    });
    group.bench_function("the JBIG2 scan page at 150 dpi", |b| {
        b.iter(|| black_box(jbig2_page.render(&RenderOptions::at_dpi(150.0))));
    });
    group.throughput(renders(&flate_page, 72.0, 1).throughput());
    group.bench_function("the Flate RGB image page at 72 dpi", |b| {
        b.iter(|| black_box(flate_page.render(&RenderOptions::at_dpi(72.0))));
    });

    // ---- through the CLI: the PNGs `tpdf render` writes ------------------

    let text_pdf = cli::fixture("dense-1.pdf", || inputs::dense(1, 0));
    let twenty_pdf = cli::fixture("dense-20.pdf", || inputs::dense(20, 0));
    let at_150 = Cli::new("render-text-150");
    let out_150 = at_150.out("png");
    group
        .throughput(Per::Pixels(1240 * 1755).throughput())
        .bench_function("the text page to PNG at 150 dpi, CLI", |b| {
            b.iter(|| at_150.run(&[&"render", &text_pdf, &"--out", &out_150, &"--dpi", &"150"]));
        });
    assert_eq!(
        at_150.pngs(&out_150),
        [("dense-1-0001.png".to_string(), 1240, 1755)],
        "one PNG, the page at 150 dpi"
    );
    let at_300 = Cli::new("render-text-300");
    let out_300 = at_300.out("png");
    group
        .throughput(Per::Pixels(2480 * 3509).throughput())
        .bench_function("the text page to PNG at 300 dpi, CLI", |b| {
            b.iter(|| at_300.run(&[&"render", &text_pdf, &"--out", &out_300, &"--dpi", &"300"]));
        });
    assert_eq!(
        at_300.pngs(&out_300),
        [("dense-1-0001.png".to_string(), 2480, 3509)],
        "one PNG, the page at 300 dpi"
    );
    let serial = Cli::new("render-20-jobs-1");
    let out_serial = serial.out("png");
    group
        .throughput(Per::Pages(20).throughput())
        .bench_function("20 pages to PNG at 150 dpi, 1 job, CLI", |b| {
            b.iter(|| {
                serial.run(&[
                    &"render",
                    &twenty_pdf,
                    &"--out",
                    &out_serial,
                    &"--dpi",
                    &"150",
                    &"--jobs",
                    &"1",
                ])
            });
        });
    let parallel = Cli::new("render-20-jobs-4");
    let out_parallel = parallel.out("png");
    group
        .throughput(Per::Pages(20).throughput())
        .bench_function("20 pages to PNG at 150 dpi, 4 jobs, CLI", |b| {
            b.iter(|| {
                parallel.run(&[
                    &"render",
                    &twenty_pdf,
                    &"--out",
                    &out_parallel,
                    &"--dpi",
                    &"150",
                    &"--jobs",
                    &"4",
                ])
            });
        });
    for (cli, out) in [(&serial, &out_serial), (&parallel, &out_parallel)] {
        let pngs = cli.pngs(out);
        assert_eq!(pngs.len(), 20, "a PNG a page");
        for (at, (name, width, height)) in pngs.iter().enumerate() {
            assert_eq!(name, &format!("dense-20-{:04}.png", at + 1));
            assert_eq!(
                (*width, *height),
                (1240, 1755),
                "{name} is the page at 150 dpi"
            );
        }
    }

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = render
}
criterion_main!(benches);
