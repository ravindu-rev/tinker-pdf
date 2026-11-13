//! The layout group: reflowable documents laid out into pages, and the
//! containers whose pages are synthesised.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `layout` list names these operations
//! in this order. Each input is asserted before its operation is timed:
//! every committed book paginates and renders every page, each generated book has
//! a page a chapter, the hyphenated one ends a line at an automatic point, the
//! HTML and Markdown documents have their 20 pages, and every container
//! fixture's first page renders, from the container and from its synthesised
//! PDF.
//!
//! Each operation is sized into the set's 20-300 ms window on the hosted
//! runner (`docs/design/performance.md`): the generated books at ten and
//! thirty chapters, and the shorter operations repeated inside an iteration,
//! their unit counting every call.
//!
//! The hyphenated book's operation needs the `hyphenation` feature's
//! dictionaries, so the set is built with `--features hyphenation`
//! (`docs/verification.md`, "The benches") and a build without it leaves that
//! operation out; the planning baseline, which has no `hyphens: auto`, lays the
//! same book out without breaking a word in its place.

mod support;

use std::borrow::Borrow;
use std::cell::LazyCell;
use std::hint::black_box;
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use support::cli::Cli;
use support::{inputs, Group, Op, Per};
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
    let bytes: Arc<[u8]> = inputs::store()
        .bytes(name, || inputs::committed_book(name))
        .into();
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

/// `calls` opens of `book` at [`at_box`], each paginating it: every page
/// each call laid out.
fn paginate<'d>(op: &mut Op<'d>, book: &'d Book, calls: u64) {
    op.per(Per::Pages(calls * book.pages));
    op.iter(move || {
        for _ in 0..calls {
            black_box(
                Document::open_with(Arc::clone(&book.bytes), &at_box())
                    .expect("it opens")
                    .page_count(),
            );
        }
    });
}

/// Every one of the `pages` of `doc`, opened once untimed, rendered at
/// 72 dpi, `calls` times.
fn render<'d>(op: &mut Op<'d>, doc: impl Borrow<Document> + 'd, pages: u64, calls: u64) {
    op.per(Per::Pages(calls * pages));
    op.iter(move || {
        for _ in 0..calls {
            black_box(render_every_page(doc.borrow()));
        }
    });
}

/// `tpdf render` over the committed book `name` at 72 dpi, `calls` times,
/// then what it wrote held to the book's pages: a PNG a page, in order. The
/// child reads the book from the store, as `book`'s bytes were read.
fn render_book_cli<'d>(op: &mut Op<'d>, name: &str, book: &'d Book, calls: u64) {
    let cli = Cli::new(&format!("layout-render-{name}"));
    let out = cli.out("png");
    let path = inputs::store().file(&format!("{name}.epub"), || book.bytes.to_vec());
    let (ran, written, name) = (cli.clone(), out.clone(), name.to_string());
    op.per(Per::Pages(calls * book.pages));
    op.iter(move || {
        for _ in 0..calls {
            cli.run(&[&"render", &path, &"--out", &out, &"--dpi", &"72"]);
        }
    });
    op.after(move || {
        let pngs = ran.pngs(&written);
        assert_eq!(pngs.len() as u64, book.pages, "{name}: a PNG a page");
        for (at, (png, width, height)) in pngs.iter().enumerate() {
            assert_eq!(png, &format!("{name}-{:04}.png", at + 1));
            assert!(*width > 0 && *height > 0, "{png} has pixels");
        }
    });
}

/// A generated book of `chapters` chapters, asserted to have a page a
/// chapter, and the document it opens to.
fn generated(chapters: usize) -> (Arc<[u8]>, Document) {
    let bytes: Arc<[u8]> = inputs::store()
        .bytes(&format!("book-{chapters}.epub"), || inputs::book(chapters))
        .into();
    let doc = Document::open_with(Arc::clone(&bytes), &at_box()).expect("the book opens");
    assert_eq!(
        doc.page_count() as usize,
        chapters,
        "the generated book has a page for each of its chapters"
    );
    (bytes, doc)
}

/// A generated book of `chapters` chapters set in `hyphens: auto`
/// ([`inputs::hyphenated_book`]), asserted to have a page a chapter and,
/// where the build has the dictionaries, to end at least one line at an
/// automatic point — its text holds no hyphen of its own, so a line ending in
/// one broke at a dictionary's point, and the operation times hyphenation —
/// and the document it opens to.
#[cfg(any(feature = "hyphenation", tinker_planning_baseline))]
fn hyphenated(chapters: usize) -> (Arc<[u8]>, Document) {
    let bytes: Arc<[u8]> = inputs::store()
        .bytes(&format!("hyphenated-book-{chapters}.epub"), || {
            inputs::hyphenated_book(chapters)
        })
        .into();
    let doc = Document::open_with(Arc::clone(&bytes), &at_box()).expect("the book opens");
    assert_eq!(
        doc.page_count() as usize,
        chapters,
        "the hyphenated book has a page for each of its chapters"
    );
    #[cfg(not(tinker_planning_baseline))]
    {
        let automatic: usize = doc
            .pages()
            .iter()
            .map(|page| {
                page.text()
                    .lines()
                    .iter()
                    .filter(|line| line.text.trim_end().ends_with('-'))
                    .count()
            })
            .sum();
        assert!(
            automatic > 0,
            "the hyphenated book ends none of its lines at an automatic point, so the \
             operation would time no hyphenation"
        );
    }
    (bytes, doc)
}

/// [`open_and_render`] `calls` times an iteration, a page a fixture a call.
fn containers_rendered<'d>(op: &mut Op<'d>, fixtures: &'d [inputs::Fixture], calls: u64) {
    op.per(Per::Pages(calls * fixtures.len() as u64));
    op.iter(move || {
        for _ in 0..calls {
            black_box(open_and_render(fixtures));
        }
    });
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

/// A format's `count` fixtures, each asserted to render its first page.
fn containers(fixtures: Vec<inputs::Fixture>, count: usize) -> Vec<inputs::Fixture> {
    assert_eq!(fixtures.len(), count, "the committed fixtures");
    assert_eq!(
        open_and_render(&fixtures),
        count,
        "each renders its first page"
    );
    fixtures
}

/// The PDFs the facade synthesised for a format's `count` fixtures, each
/// asserted to render its first page.
fn synthesised_pdfs(pdfs: Vec<inputs::Fixture>, count: usize) -> Vec<inputs::Fixture> {
    assert_eq!(pdfs.len(), count, "the synthesised PDFs");
    assert_eq!(
        open_and_render(&pdfs),
        count,
        "and from its synthesised PDF"
    );
    pdfs
}

fn layout(c: &mut Criterion) {
    let calibre_book_cover = LazyCell::new(|| committed("calibre-book-cover.epub"));
    let calibre_book_nocover = LazyCell::new(|| committed("calibre-book-nocover.epub"));
    let calibre_embedded_font = LazyCell::new(|| committed("calibre-embedded-font.epub"));
    let kcc_fixed_layout = LazyCell::new(|| committed("kcc-fixed-layout.epub"));
    let pandoc_book_cover = LazyCell::new(|| committed("pandoc-book-cover.epub"));
    let pandoc_book_epub2 = LazyCell::new(|| committed("pandoc-book-epub2.epub"));
    let pandoc_book_nocover = LazyCell::new(|| committed("pandoc-book-nocover.epub"));
    let pandoc_embedded_font = LazyCell::new(|| committed("pandoc-embedded-font.epub"));
    let pandoc_plates = LazyCell::new(|| committed("pandoc-plates.epub"));
    let opened = |book: &Book| {
        Document::open_with(Arc::clone(&book.bytes), &at_box()).expect("the book opens")
    };

    let thirty = LazyCell::new(|| generated(30));
    let ten = LazyCell::new(|| generated(10));
    #[cfg(any(feature = "hyphenation", tinker_planning_baseline))]
    let hyphenated_ten = LazyCell::new(|| hyphenated(10));

    // Each set of fixtures, and the PDFs the facade synthesised for it, which
    // are written from the committed fixtures themselves, so a process timing
    // the synthesised set writes that set alone.
    let store = inputs::store();
    let svg = LazyCell::new(|| containers(store.set("svg.bundle", inputs::svg_fixtures), 17));
    let svg_pdfs = LazyCell::new(|| {
        synthesised_pdfs(
            store.set("svg-synthesised.bundle", || {
                synthesised(&inputs::svg_fixtures())
            }),
            17,
        )
    });
    let xps = LazyCell::new(|| containers(store.set("xps.bundle", inputs::xps_fixtures), 19));
    let xps_pdfs = LazyCell::new(|| {
        synthesised_pdfs(
            store.set("xps-synthesised.bundle", || {
                synthesised(&inputs::xps_fixtures())
            }),
            19,
        )
    });
    let cbz = LazyCell::new(|| containers(store.set("cbz.bundle", inputs::cbz_fixtures), 9));
    let cbz_pdfs = LazyCell::new(|| {
        synthesised_pdfs(
            store.set("cbz-synthesised.bundle", || {
                synthesised(&inputs::cbz_fixtures())
            }),
            9,
        )
    });

    let mut group = Group::new(c, "layout");

    // The one whose answer is not a property of the file: an EPUB's page count
    // is a function of the box it is asked for (ruling 4's second half), so
    // this measures pagination rather than parsing.
    group.operation("paginate a book at 432x648 pt", |op| {
        let name = "pandoc-book-cover.epub";
        let epub = store.bytes(name, || inputs::committed_book(name));
        let pages = Document::open_with(epub.clone(), &OpenOptions::at_page(432.0, 648.0))
            .expect("it opens")
            .page_count();
        op.per(Per::Pages(u64::from(pages)));
        op.iter(move || {
            black_box(
                Document::open_with(epub.clone(), &OpenOptions::at_page(432.0, 648.0))
                    .expect("it opens")
                    .page_count(),
            )
        });
    });

    // Each committed book opened and paginated as many times an iteration as
    // bring it inside the window. One call a book, each with its name as a
    // literal, because `cargo xtask baseline` reads a group's names from its
    // `operation` calls.
    group.operation(
        "open and paginate calibre-book-cover.epub at 432x648 pt",
        |op| paginate(op, &calibre_book_cover, 18),
    );
    group.operation(
        "open and paginate calibre-book-nocover.epub at 432x648 pt",
        |op| paginate(op, &calibre_book_nocover, 19),
    );
    group.operation(
        "open and paginate calibre-embedded-font.epub at 432x648 pt",
        |op| paginate(op, &calibre_embedded_font, 4),
    );
    group.operation(
        "open and paginate kcc-fixed-layout.epub at 432x648 pt",
        |op| paginate(op, &kcc_fixed_layout, 86),
    );
    group.operation(
        "open and paginate pandoc-book-cover.epub at 432x648 pt",
        |op| paginate(op, &pandoc_book_cover, 4),
    );
    group.operation(
        "open and paginate pandoc-book-epub2.epub at 432x648 pt",
        |op| paginate(op, &pandoc_book_epub2, 4),
    );
    group.operation(
        "open and paginate pandoc-book-nocover.epub at 432x648 pt",
        |op| paginate(op, &pandoc_book_nocover, 4),
    );
    group.operation(
        "open and paginate pandoc-embedded-font.epub at 432x648 pt",
        |op| paginate(op, &pandoc_embedded_font, 4),
    );
    group.operation("open and paginate pandoc-plates.epub at 432x648 pt", |op| {
        paginate(op, &pandoc_plates, 17)
    });

    // Each committed book's every page, the book opened once untimed.
    group.operation(
        "render every page of calibre-book-cover.epub at 72 dpi",
        |op| {
            render(
                op,
                opened(&calibre_book_cover),
                calibre_book_cover.pages,
                12,
            )
        },
    );
    group.operation(
        "render every page of calibre-book-nocover.epub at 72 dpi",
        |op| {
            render(
                op,
                opened(&calibre_book_nocover),
                calibre_book_nocover.pages,
                15,
            )
        },
    );
    group.operation(
        "render every page of calibre-embedded-font.epub at 72 dpi",
        |op| {
            render(
                op,
                opened(&calibre_embedded_font),
                calibre_embedded_font.pages,
                7,
            )
        },
    );
    group.operation(
        "render every page of kcc-fixed-layout.epub at 72 dpi",
        |op| render(op, opened(&kcc_fixed_layout), kcc_fixed_layout.pages, 32),
    );
    group.operation(
        "render every page of pandoc-book-cover.epub at 72 dpi",
        |op| render(op, opened(&pandoc_book_cover), pandoc_book_cover.pages, 9),
    );
    group.operation(
        "render every page of pandoc-book-epub2.epub at 72 dpi",
        |op| render(op, opened(&pandoc_book_epub2), pandoc_book_epub2.pages, 9),
    );
    group.operation(
        "render every page of pandoc-book-nocover.epub at 72 dpi",
        |op| {
            render(
                op,
                opened(&pandoc_book_nocover),
                pandoc_book_nocover.pages,
                11,
            )
        },
    );
    group.operation(
        "render every page of pandoc-embedded-font.epub at 72 dpi",
        |op| {
            render(
                op,
                opened(&pandoc_embedded_font),
                pandoc_embedded_font.pages,
                5,
            )
        },
    );
    group.operation("render every page of pandoc-plates.epub at 72 dpi", |op| {
        render(op, opened(&pandoc_plates), pandoc_plates.pages, 13)
    });

    // The generated books: a page a chapter, so thirty chapters against ten
    // is the linearity relation, each call's figure against the other's.
    group.calibrated("open and paginate the 30-chapter book", |op| {
        let (bytes, _) = &*thirty;
        op.per(Per::Pages(30));
        op.iter(move || {
            black_box(
                Document::open_with(Arc::clone(bytes), &at_box())
                    .expect("it opens")
                    .page_count(),
            )
        });
    });
    group.calibrated("render the 30-chapter book at 72 dpi", |op| {
        render(op, &thirty.1, 30, 1)
    });
    group.operation("open and paginate the 10-chapter book", |op| {
        let (bytes, _) = &*ten;
        op.per(Per::Pages(2 * 10));
        op.iter(move || {
            for _ in 0..2 {
                black_box(
                    Document::open_with(Arc::clone(bytes), &at_box())
                        .expect("it opens")
                        .page_count(),
                );
            }
        });
    });
    group.operation("render the 10-chapter book at 72 dpi", |op| {
        render(op, &ten.1, 10, 1)
    });
    // The 10-chapter book's shape in British English set in `hyphens: auto`
    // (FT-15.8): each paragraph long words the vendored `en-GB` dictionary
    // breaks at a line's end, opened and paginated twice an iteration as the
    // 10-chapter book is. A build without the `hyphenation` feature leaves it
    // out; the planning baseline, which has no `hyphens: auto`, paginates the
    // same book without breaking a word in its place.
    #[cfg(any(feature = "hyphenation", tinker_planning_baseline))]
    group.operation("open and paginate the 10-chapter hyphenated book", |op| {
        let (bytes, _) = &*hyphenated_ten;
        op.per(Per::Pages(2 * 10));
        op.iter(move || {
            for _ in 0..2 {
                black_box(
                    Document::open_with(Arc::clone(bytes), &at_box())
                        .expect("it opens")
                        .page_count(),
                );
            }
        });
    });

    // Markup laid out and written as a PDF.
    group.calibrated("the 20-page HTML report to PDF", |op| {
        let report = store.text("report.html", inputs::html_report);
        let page_box = inputs::default_box();
        let (_, laid) =
            DocumentBuilder::from_html(&report, "", page_box).expect("the report lays out");
        assert_eq!(laid.pages(), 20, "the HTML report's pages");
        op.per(Per::Pages(3 * 20));
        op.iter(move || {
            for _ in 0..3 {
                let (builder, _) =
                    DocumentBuilder::from_html(&report, "", page_box).expect("it lays out");
                black_box(builder.finish());
            }
        });
    });
    group.calibrated("the 20-page Markdown document to PDF", |op| {
        let markdown = store.text("report.md", inputs::markdown);
        let markdown_doc =
            Document::open_markdown(markdown.as_bytes().to_vec(), &OpenOptions::default())
                .expect("the Markdown document lays out");
        assert_eq!(
            markdown_doc.page_count(),
            20,
            "the Markdown document's pages"
        );
        op.per(Per::Pages(3 * 20));
        op.iter(move || {
            for _ in 0..3 {
                black_box(
                    Document::open_markdown(markdown.as_bytes().to_vec(), &OpenOptions::default())
                        .expect("it lays out"),
                );
            }
        });
    });

    // Each format a set at a time, every fixture of it in one iteration,
    // beside the same pages from the PDF the facade synthesised for each.
    group.operation("open each SVG fixture, render page 1", |op| {
        containers_rendered(op, &svg, 4)
    });
    group.operation("render each SVG fixture's synthesised PDF", |op| {
        containers_rendered(op, &svg_pdfs, 5)
    });
    group.operation("open each XPS fixture, render page 1", |op| {
        containers_rendered(op, &xps, 1)
    });
    group.operation("render each XPS fixture's synthesised PDF", |op| {
        containers_rendered(op, &xps_pdfs, 1)
    });
    group.operation("open each CBZ fixture, render page 1", |op| {
        containers_rendered(op, &cbz, 8)
    });
    group.operation("render each CBZ fixture's synthesised PDF", |op| {
        containers_rendered(op, &cbz_pdfs, 14)
    });

    // Each committed book's every page through the CLI, read from the
    // store: a PNG a page, as `tpdf render` writes them.
    group.operation(
        "render every page of calibre-book-cover.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "calibre-book-cover", &calibre_book_cover, 1),
    );
    group.operation(
        "render every page of calibre-book-nocover.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "calibre-book-nocover", &calibre_book_nocover, 1),
    );
    group.operation(
        "render every page of calibre-embedded-font.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "calibre-embedded-font", &calibre_embedded_font, 1),
    );
    group.operation(
        "render every page of kcc-fixed-layout.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "kcc-fixed-layout", &kcc_fixed_layout, 7),
    );
    group.operation(
        "render every page of pandoc-book-cover.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "pandoc-book-cover", &pandoc_book_cover, 1),
    );
    group.operation(
        "render every page of pandoc-book-epub2.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "pandoc-book-epub2", &pandoc_book_epub2, 1),
    );
    group.operation(
        "render every page of pandoc-book-nocover.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "pandoc-book-nocover", &pandoc_book_nocover, 1),
    );
    group.operation(
        "render every page of pandoc-embedded-font.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "pandoc-embedded-font", &pandoc_embedded_font, 1),
    );
    group.operation(
        "render every page of pandoc-plates.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "pandoc-plates", &pandoc_plates, 1),
    );

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = layout
}
criterion_main!(benches);
