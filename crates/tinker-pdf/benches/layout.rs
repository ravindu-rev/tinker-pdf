//! The layout group: reflowable documents laid out into pages, and the
//! containers whose pages are synthesised.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `layout` list names these operations
//! in this order. Each input is asserted before anything is timed: every
//! committed book paginates and renders every page, each generated book has
//! a page a chapter, the HTML and Markdown documents have their 20 pages, and
//! every container fixture's first page renders, from the container and from
//! its synthesised PDF.

mod support;

use std::hint::black_box;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Bencher, Criterion, SamplingMode};
use support::cli::Cli;
use support::{inputs, Per};
use tinker_pdf::{Document, DocumentBuilder, FromHtml, OpenOptions, RenderOptions};

/// The box a book is opened at here: the default, 432 × 648 pt.
fn at_box() -> OpenOptions {
    OpenOptions::at_page(432.0, 648.0)
}

/// A book's bytes and its page count at [`at_box`].
struct Book {
    bytes: Arc<[u8]>,
    pages: u64,
}

/// A committed book, by file name, asserted to paginate and to render every
/// page.
fn committed(name: &str) -> Book {
    let bytes: Arc<[u8]> = inputs::committed_book(name).into();
    let doc = Document::open_with(Arc::clone(&bytes), &at_box()).expect("the book opens");
    let pages = doc.page_count();
    assert!(pages > 0, "{name} paginates");
    let rendered = render_every_page(&doc);
    assert_eq!(rendered, pages, "{name} renders every page");
    Book {
        bytes,
        pages: u64::from(pages),
    }
}

/// Every page of `doc` at 72 dpi. How many rendered to a bitmap with pixels.
fn render_every_page(doc: &Document) -> u32 {
    let options = RenderOptions::at_dpi(72.0);
    let mut rendered = 0;
    for page in doc.pages() {
        let bitmap = black_box(page.render(&options));
        if bitmap.width > 0 && bitmap.height > 0 {
            rendered += 1;
        }
    }
    rendered
}

/// `calls` opens of `book` at [`at_box`], each paginating it.
fn paginate(b: &mut Bencher<'_>, book: &Book, calls: usize) {
    b.iter(|| {
        for _ in 0..calls {
            black_box(
                Document::open_with(Arc::clone(&book.bytes), &at_box())
                    .expect("it opens")
                    .page_count(),
            );
        }
    });
}

/// Every page of `doc`, opened once untimed, rendered at 72 dpi.
fn render(b: &mut Bencher<'_>, doc: &Document) {
    b.iter(|| black_box(render_every_page(doc)));
}

/// A generated book of `chapters` chapters, asserted to have a page a
/// chapter, and the document it opens to.
fn generated(chapters: usize) -> (Arc<[u8]>, Document) {
    let bytes: Arc<[u8]> = inputs::book(chapters).into();
    let doc = Document::open_with(Arc::clone(&bytes), &at_box()).expect("the book opens");
    assert_eq!(
        doc.page_count() as usize,
        chapters,
        "the generated book has a page for each of its chapters"
    );
    (bytes, doc)
}

/// Each fixture of one format opened and its first page rendered at 72 dpi:
/// one call of the container operations. How many rendered.
fn open_and_render(fixtures: &[inputs::Fixture]) -> usize {
    let options = RenderOptions::at_dpi(72.0);
    fixtures
        .iter()
        .filter(|fixture| {
            let doc = Document::open(Arc::clone(&fixture.bytes)).expect("the fixture opens");
            let bitmap = black_box(doc.page(0).expect("a first page").render(&options));
            bitmap.width > 0 && bitmap.height > 0
        })
        .count()
}

/// Each fixture's synthesised PDF, the bytes the facade wrote for it.
fn synthesised(fixtures: &[inputs::Fixture]) -> Vec<inputs::Fixture> {
    fixtures
        .iter()
        .map(|fixture| {
            let doc = Document::open(Arc::clone(&fixture.bytes))
                .unwrap_or_else(|error| panic!("{} opens: {error}", fixture.name));
            inputs::Fixture {
                name: fixture.name.clone(),
                bytes: doc.cos().bytes().to_vec().into(),
            }
        })
        .collect()
}

/// A format's fixtures and their synthesised PDFs, each asserted to render
/// its first page.
fn containers(fixtures: Vec<inputs::Fixture>, count: usize) -> [Vec<inputs::Fixture>; 2] {
    assert_eq!(fixtures.len(), count, "the committed fixtures");
    let pdfs = synthesised(&fixtures);
    assert_eq!(
        open_and_render(&fixtures),
        count,
        "each renders its first page"
    );
    assert_eq!(
        open_and_render(&pdfs),
        count,
        "and from its synthesised PDF"
    );
    [fixtures, pdfs]
}

fn layout(c: &mut Criterion) {
    let epub = inputs::committed_book("pandoc-book-cover.epub");
    let pages = Document::open_with(epub.clone(), &OpenOptions::at_page(432.0, 648.0))
        .expect("it opens")
        .page_count();

    let calibre_book_cover = committed("calibre-book-cover.epub");
    let calibre_book_nocover = committed("calibre-book-nocover.epub");
    let calibre_embedded_font = committed("calibre-embedded-font.epub");
    let kcc_fixed_layout = committed("kcc-fixed-layout.epub");
    let pandoc_book_cover = committed("pandoc-book-cover.epub");
    let pandoc_book_epub2 = committed("pandoc-book-epub2.epub");
    let pandoc_book_nocover = committed("pandoc-book-nocover.epub");
    let pandoc_embedded_font = committed("pandoc-embedded-font.epub");
    let pandoc_plates = committed("pandoc-plates.epub");
    let opened = |book: &Book| {
        Document::open_with(Arc::clone(&book.bytes), &at_box()).expect("the book opens")
    };

    let (thousand, thousand_doc) = generated(1000);
    let (hundred, hundred_doc) = generated(100);

    let report = inputs::html_report();
    let page_box = inputs::default_box();
    let (_, laid) = DocumentBuilder::from_html(&report, "", page_box).expect("the report lays out");
    assert_eq!(laid.pages(), 20, "the HTML report's pages");
    let markdown = inputs::markdown();
    let markdown_doc =
        Document::open_markdown(markdown.as_bytes().to_vec(), &OpenOptions::default())
            .expect("the Markdown document lays out");
    assert_eq!(
        markdown_doc.page_count(),
        20,
        "the Markdown document's pages"
    );

    let [svg, svg_pdfs] = containers(inputs::svg_fixtures(), 17);
    let [xps, xps_pdfs] = containers(inputs::xps_fixtures(), 19);
    let [cbz, cbz_pdfs] = containers(inputs::cbz_fixtures(), 9);

    let mut group = c.benchmark_group("layout");
    group.sampling_mode(SamplingMode::Flat);

    // The one whose answer is not a property of the file: an EPUB's page count
    // is a function of the box it is asked for (ruling 4's second half), so
    // this measures pagination rather than parsing.
    group.throughput(Per::Pages(u64::from(pages)).throughput());
    group.bench_function("paginate a book at 432x648 pt", |b| {
        b.iter(|| {
            black_box(
                Document::open_with(epub.clone(), &OpenOptions::at_page(432.0, 648.0))
                    .expect("it opens")
                    .page_count(),
            )
        });
    });

    // Each committed book opened and paginated four times an iteration. One
    // call a book, each with its name as a literal, because `cargo xtask
    // baseline` reads a group's names from its `bench_function` calls.
    group
        .throughput(Per::Pages(4 * calibre_book_cover.pages).throughput())
        .bench_function(
            "open and paginate calibre-book-cover.epub at 432x648 pt",
            |b| {
                paginate(b, &calibre_book_cover, 4);
            },
        );
    group
        .throughput(Per::Pages(4 * calibre_book_nocover.pages).throughput())
        .bench_function(
            "open and paginate calibre-book-nocover.epub at 432x648 pt",
            |b| {
                paginate(b, &calibre_book_nocover, 4);
            },
        );
    group
        .throughput(Per::Pages(4 * calibre_embedded_font.pages).throughput())
        .bench_function(
            "open and paginate calibre-embedded-font.epub at 432x648 pt",
            |b| {
                paginate(b, &calibre_embedded_font, 4);
            },
        );
    group
        .throughput(Per::Pages(4 * kcc_fixed_layout.pages).throughput())
        .bench_function(
            "open and paginate kcc-fixed-layout.epub at 432x648 pt",
            |b| {
                paginate(b, &kcc_fixed_layout, 4);
            },
        );
    group
        .throughput(Per::Pages(4 * pandoc_book_cover.pages).throughput())
        .bench_function(
            "open and paginate pandoc-book-cover.epub at 432x648 pt",
            |b| {
                paginate(b, &pandoc_book_cover, 4);
            },
        );
    group
        .throughput(Per::Pages(4 * pandoc_book_epub2.pages).throughput())
        .bench_function(
            "open and paginate pandoc-book-epub2.epub at 432x648 pt",
            |b| {
                paginate(b, &pandoc_book_epub2, 4);
            },
        );
    group
        .throughput(Per::Pages(4 * pandoc_book_nocover.pages).throughput())
        .bench_function(
            "open and paginate pandoc-book-nocover.epub at 432x648 pt",
            |b| {
                paginate(b, &pandoc_book_nocover, 4);
            },
        );
    group
        .throughput(Per::Pages(4 * pandoc_embedded_font.pages).throughput())
        .bench_function(
            "open and paginate pandoc-embedded-font.epub at 432x648 pt",
            |b| {
                paginate(b, &pandoc_embedded_font, 4);
            },
        );
    group
        .throughput(Per::Pages(4 * pandoc_plates.pages).throughput())
        .bench_function("open and paginate pandoc-plates.epub at 432x648 pt", |b| {
            paginate(b, &pandoc_plates, 4);
        });

    // Each committed book's every page, the book opened once untimed.
    let doc = opened(&calibre_book_cover);
    group
        .throughput(Per::Pages(calibre_book_cover.pages).throughput())
        .bench_function(
            "render every page of calibre-book-cover.epub at 72 dpi",
            |b| {
                render(b, &doc);
            },
        );
    let doc = opened(&calibre_book_nocover);
    group
        .throughput(Per::Pages(calibre_book_nocover.pages).throughput())
        .bench_function(
            "render every page of calibre-book-nocover.epub at 72 dpi",
            |b| {
                render(b, &doc);
            },
        );
    let doc = opened(&calibre_embedded_font);
    group
        .throughput(Per::Pages(calibre_embedded_font.pages).throughput())
        .bench_function(
            "render every page of calibre-embedded-font.epub at 72 dpi",
            |b| {
                render(b, &doc);
            },
        );
    let doc = opened(&kcc_fixed_layout);
    group
        .throughput(Per::Pages(kcc_fixed_layout.pages).throughput())
        .bench_function(
            "render every page of kcc-fixed-layout.epub at 72 dpi",
            |b| {
                render(b, &doc);
            },
        );
    let doc = opened(&pandoc_book_cover);
    group
        .throughput(Per::Pages(pandoc_book_cover.pages).throughput())
        .bench_function(
            "render every page of pandoc-book-cover.epub at 72 dpi",
            |b| {
                render(b, &doc);
            },
        );
    let doc = opened(&pandoc_book_epub2);
    group
        .throughput(Per::Pages(pandoc_book_epub2.pages).throughput())
        .bench_function(
            "render every page of pandoc-book-epub2.epub at 72 dpi",
            |b| {
                render(b, &doc);
            },
        );
    let doc = opened(&pandoc_book_nocover);
    group
        .throughput(Per::Pages(pandoc_book_nocover.pages).throughput())
        .bench_function(
            "render every page of pandoc-book-nocover.epub at 72 dpi",
            |b| {
                render(b, &doc);
            },
        );
    let doc = opened(&pandoc_embedded_font);
    group
        .throughput(Per::Pages(pandoc_embedded_font.pages).throughput())
        .bench_function(
            "render every page of pandoc-embedded-font.epub at 72 dpi",
            |b| {
                render(b, &doc);
            },
        );
    let doc = opened(&pandoc_plates);
    group
        .throughput(Per::Pages(pandoc_plates.pages).throughput())
        .bench_function("render every page of pandoc-plates.epub at 72 dpi", |b| {
            render(b, &doc);
        });

    // The generated books: a page a chapter, so a thousand chapters against a
    // hundred is the linearity relation.
    group
        .throughput(Per::Pages(1000).throughput())
        .bench_function("open and paginate the 1000-chapter book", |b| {
            b.iter(|| {
                black_box(
                    Document::open_with(Arc::clone(&thousand), &at_box())
                        .expect("it opens")
                        .page_count(),
                )
            });
        });
    group
        .throughput(Per::Pages(1000).throughput())
        .bench_function("render the 1000-chapter book at 72 dpi", |b| {
            render(b, &thousand_doc);
        });
    group
        .throughput(Per::Pages(100).throughput())
        .bench_function("open and paginate the 100-chapter book", |b| {
            b.iter(|| {
                black_box(
                    Document::open_with(Arc::clone(&hundred), &at_box())
                        .expect("it opens")
                        .page_count(),
                )
            });
        });
    group
        .throughput(Per::Pages(100).throughput())
        .bench_function("render the 100-chapter book at 72 dpi", |b| {
            render(b, &hundred_doc);
        });

    // Markup laid out and written as a PDF.
    group
        .throughput(Per::Pages(20).throughput())
        .bench_function("the 20-page HTML report to PDF", |b| {
            b.iter(|| {
                let (builder, _) =
                    DocumentBuilder::from_html(&report, "", page_box).expect("it lays out");
                black_box(builder.finish())
            });
        });
    group
        .throughput(Per::Pages(20).throughput())
        .bench_function("the 20-page Markdown document to PDF", |b| {
            b.iter(|| {
                black_box(
                    Document::open_markdown(markdown.as_bytes().to_vec(), &OpenOptions::default())
                        .expect("it lays out"),
                )
            });
        });

    // Each format a set at a time, every fixture of it in one iteration,
    // beside the same pages from the PDF the facade synthesised for each.
    group
        .throughput(Per::Pages(svg.len() as u64).throughput())
        .bench_function("open each SVG fixture, render page 1", |b| {
            b.iter(|| black_box(open_and_render(&svg)));
        });
    group
        .throughput(Per::Pages(svg_pdfs.len() as u64).throughput())
        .bench_function("render each SVG fixture's synthesised PDF", |b| {
            b.iter(|| black_box(open_and_render(&svg_pdfs)));
        });
    group
        .throughput(Per::Pages(xps.len() as u64).throughput())
        .bench_function("open each XPS fixture, render page 1", |b| {
            b.iter(|| black_box(open_and_render(&xps)));
        });
    group
        .throughput(Per::Pages(xps_pdfs.len() as u64).throughput())
        .bench_function("render each XPS fixture's synthesised PDF", |b| {
            b.iter(|| black_box(open_and_render(&xps_pdfs)));
        });
    group
        .throughput(Per::Pages(cbz.len() as u64).throughput())
        .bench_function("open each CBZ fixture, render page 1", |b| {
            b.iter(|| black_box(open_and_render(&cbz)));
        });
    group
        .throughput(Per::Pages(cbz_pdfs.len() as u64).throughput())
        .bench_function("render each CBZ fixture's synthesised PDF", |b| {
            b.iter(|| black_box(open_and_render(&cbz_pdfs)));
        });

    // Each committed book's every page through the CLI, read from where it
    // is committed: a PNG a page, as `tpdf render` writes them.
    let (cli, out) = book_cli("calibre-book-cover");
    group
        .throughput(Per::Pages(calibre_book_cover.pages).throughput())
        .bench_function(
            "render every page of calibre-book-cover.epub at 72 dpi, CLI",
            |b| {
                b.iter(|| render_book(&cli, "calibre-book-cover", &out));
            },
        );
    assert_book_pngs(&cli, &out, "calibre-book-cover", calibre_book_cover.pages);
    let (cli, out) = book_cli("calibre-book-nocover");
    group
        .throughput(Per::Pages(calibre_book_nocover.pages).throughput())
        .bench_function(
            "render every page of calibre-book-nocover.epub at 72 dpi, CLI",
            |b| {
                b.iter(|| render_book(&cli, "calibre-book-nocover", &out));
            },
        );
    assert_book_pngs(
        &cli,
        &out,
        "calibre-book-nocover",
        calibre_book_nocover.pages,
    );
    let (cli, out) = book_cli("calibre-embedded-font");
    group
        .throughput(Per::Pages(calibre_embedded_font.pages).throughput())
        .bench_function(
            "render every page of calibre-embedded-font.epub at 72 dpi, CLI",
            |b| {
                b.iter(|| render_book(&cli, "calibre-embedded-font", &out));
            },
        );
    assert_book_pngs(
        &cli,
        &out,
        "calibre-embedded-font",
        calibre_embedded_font.pages,
    );
    let (cli, out) = book_cli("kcc-fixed-layout");
    group
        .throughput(Per::Pages(kcc_fixed_layout.pages).throughput())
        .bench_function(
            "render every page of kcc-fixed-layout.epub at 72 dpi, CLI",
            |b| {
                b.iter(|| render_book(&cli, "kcc-fixed-layout", &out));
            },
        );
    assert_book_pngs(&cli, &out, "kcc-fixed-layout", kcc_fixed_layout.pages);
    let (cli, out) = book_cli("pandoc-book-cover");
    group
        .throughput(Per::Pages(pandoc_book_cover.pages).throughput())
        .bench_function(
            "render every page of pandoc-book-cover.epub at 72 dpi, CLI",
            |b| {
                b.iter(|| render_book(&cli, "pandoc-book-cover", &out));
            },
        );
    assert_book_pngs(&cli, &out, "pandoc-book-cover", pandoc_book_cover.pages);
    let (cli, out) = book_cli("pandoc-book-epub2");
    group
        .throughput(Per::Pages(pandoc_book_epub2.pages).throughput())
        .bench_function(
            "render every page of pandoc-book-epub2.epub at 72 dpi, CLI",
            |b| {
                b.iter(|| render_book(&cli, "pandoc-book-epub2", &out));
            },
        );
    assert_book_pngs(&cli, &out, "pandoc-book-epub2", pandoc_book_epub2.pages);
    let (cli, out) = book_cli("pandoc-book-nocover");
    group
        .throughput(Per::Pages(pandoc_book_nocover.pages).throughput())
        .bench_function(
            "render every page of pandoc-book-nocover.epub at 72 dpi, CLI",
            |b| {
                b.iter(|| render_book(&cli, "pandoc-book-nocover", &out));
            },
        );
    assert_book_pngs(&cli, &out, "pandoc-book-nocover", pandoc_book_nocover.pages);
    let (cli, out) = book_cli("pandoc-embedded-font");
    group
        .throughput(Per::Pages(pandoc_embedded_font.pages).throughput())
        .bench_function(
            "render every page of pandoc-embedded-font.epub at 72 dpi, CLI",
            |b| {
                b.iter(|| render_book(&cli, "pandoc-embedded-font", &out));
            },
        );
    assert_book_pngs(
        &cli,
        &out,
        "pandoc-embedded-font",
        pandoc_embedded_font.pages,
    );
    let (cli, out) = book_cli("pandoc-plates");
    group
        .throughput(Per::Pages(pandoc_plates.pages).throughput())
        .bench_function(
            "render every page of pandoc-plates.epub at 72 dpi, CLI",
            |b| {
                b.iter(|| render_book(&cli, "pandoc-plates", &out));
            },
        );
    assert_book_pngs(&cli, &out, "pandoc-plates", pandoc_plates.pages);

    group.finish();
}

/// The CLI runner for one book's render, and the directory its PNGs go to.
fn book_cli(book: &str) -> (Cli, std::path::PathBuf) {
    let cli = Cli::new(&format!("layout-render-{book}"));
    let out = cli.out("png");
    (cli, out)
}

/// `tpdf render` over the committed `book` at 72 dpi.
fn render_book(cli: &Cli, book: &str, out: &std::path::Path) {
    let path = inputs::committed_book_path(&format!("{book}.epub"));
    cli.run(&[&"render", &path, &"--out", &out, &"--dpi", &"72"]);
}

/// The render wrote one PNG for each of the book's `pages`, in order.
fn assert_book_pngs(cli: &Cli, out: &std::path::Path, book: &str, pages: u64) {
    let pngs = cli.pngs(out);
    assert_eq!(pngs.len() as u64, pages, "{book}: a PNG a page");
    for (at, (name, width, height)) in pngs.iter().enumerate() {
        assert_eq!(name, &format!("{book}-{:04}.png", at + 1));
        assert!(*width > 0 && *height > 0, "{name} has pixels");
    }
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = layout
}
criterion_main!(benches);
