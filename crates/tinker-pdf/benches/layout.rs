//! The layout group: reflowable documents laid out into pages, and the
//! containers whose pages are synthesised.
//!
//! The harness, the sampling and the rules every name here keeps are
//! `support/mod.rs`'s; `baseline.json`'s `layout` list names these operations
//! in this order. Each input is asserted before its operation is timed:
//! every committed book paginates and renders every page, each generated book has
//! a page a chapter, the HTML and Markdown documents have their 20 pages, and
//! every container fixture's first page renders, from the container and from
//! its synthesised PDF.

mod support;

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

/// `calls` opens of `book` at [`at_box`], each paginating it: every page
/// each call laid out.
fn paginate(op: &mut Op<'_, '_>, book: &Book, calls: u64) {
    op.per(Per::Pages(calls * book.pages));
    op.iter(|| {
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
/// 72 dpi.
fn render(op: &mut Op<'_, '_>, doc: &Document, pages: u64) {
    op.per(Per::Pages(pages));
    op.iter(|| black_box(render_every_page(doc)));
}

/// `tpdf render` over the committed book `name` at 72 dpi, then what it
/// wrote held to the book's pages: a PNG a page, in order.
fn render_book_cli(op: &mut Op<'_, '_>, name: &str, book: &Book) {
    let cli = Cli::new(&format!("layout-render-{name}"));
    let out = cli.out("png");
    let path = inputs::committed_book_path(&format!("{name}.epub"));
    op.per(Per::Pages(book.pages));
    op.iter(|| cli.run(&[&"render", &path, &"--out", &out, &"--dpi", &"72"]));
    let pngs = cli.pngs(&out);
    assert_eq!(pngs.len() as u64, book.pages, "{name}: a PNG a page");
    for (at, (png, width, height)) in pngs.iter().enumerate() {
        assert_eq!(png, &format!("{name}-{:04}.png", at + 1));
        assert!(*width > 0 && *height > 0, "{png} has pixels");
    }
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

    let thousand = LazyCell::new(|| generated(1000));
    let hundred = LazyCell::new(|| generated(100));

    let svg = LazyCell::new(|| containers(inputs::svg_fixtures(), 17));
    let xps = LazyCell::new(|| containers(inputs::xps_fixtures(), 19));
    let cbz = LazyCell::new(|| containers(inputs::cbz_fixtures(), 9));

    let mut group = Group::new(c, "layout");

    // The one whose answer is not a property of the file: an EPUB's page count
    // is a function of the box it is asked for (ruling 4's second half), so
    // this measures pagination rather than parsing.
    group.operation("paginate a book at 432x648 pt", |op| {
        let epub = inputs::committed_book("pandoc-book-cover.epub");
        let pages = Document::open_with(epub.clone(), &OpenOptions::at_page(432.0, 648.0))
            .expect("it opens")
            .page_count();
        op.per(Per::Pages(u64::from(pages)));
        op.iter(|| {
            black_box(
                Document::open_with(epub.clone(), &OpenOptions::at_page(432.0, 648.0))
                    .expect("it opens")
                    .page_count(),
            )
        });
    });

    // Each committed book opened and paginated four times an iteration. One
    // call a book, each with its name as a literal, because `cargo xtask
    // baseline` reads a group's names from its `operation` calls.
    group.operation(
        "open and paginate calibre-book-cover.epub at 432x648 pt",
        |op| paginate(op, &calibre_book_cover, 4),
    );
    group.operation(
        "open and paginate calibre-book-nocover.epub at 432x648 pt",
        |op| paginate(op, &calibre_book_nocover, 4),
    );
    group.operation(
        "open and paginate calibre-embedded-font.epub at 432x648 pt",
        |op| paginate(op, &calibre_embedded_font, 4),
    );
    group.operation(
        "open and paginate kcc-fixed-layout.epub at 432x648 pt",
        |op| paginate(op, &kcc_fixed_layout, 4),
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
        paginate(op, &pandoc_plates, 4)
    });

    // Each committed book's every page, the book opened once untimed.
    group.operation(
        "render every page of calibre-book-cover.epub at 72 dpi",
        |op| render(op, &opened(&calibre_book_cover), calibre_book_cover.pages),
    );
    group.operation(
        "render every page of calibre-book-nocover.epub at 72 dpi",
        |op| {
            render(
                op,
                &opened(&calibre_book_nocover),
                calibre_book_nocover.pages,
            )
        },
    );
    group.operation(
        "render every page of calibre-embedded-font.epub at 72 dpi",
        |op| {
            render(
                op,
                &opened(&calibre_embedded_font),
                calibre_embedded_font.pages,
            )
        },
    );
    group.operation(
        "render every page of kcc-fixed-layout.epub at 72 dpi",
        |op| render(op, &opened(&kcc_fixed_layout), kcc_fixed_layout.pages),
    );
    group.operation(
        "render every page of pandoc-book-cover.epub at 72 dpi",
        |op| render(op, &opened(&pandoc_book_cover), pandoc_book_cover.pages),
    );
    group.operation(
        "render every page of pandoc-book-epub2.epub at 72 dpi",
        |op| render(op, &opened(&pandoc_book_epub2), pandoc_book_epub2.pages),
    );
    group.operation(
        "render every page of pandoc-book-nocover.epub at 72 dpi",
        |op| render(op, &opened(&pandoc_book_nocover), pandoc_book_nocover.pages),
    );
    group.operation(
        "render every page of pandoc-embedded-font.epub at 72 dpi",
        |op| {
            render(
                op,
                &opened(&pandoc_embedded_font),
                pandoc_embedded_font.pages,
            )
        },
    );
    group.operation("render every page of pandoc-plates.epub at 72 dpi", |op| {
        render(op, &opened(&pandoc_plates), pandoc_plates.pages)
    });

    // The generated books: a page a chapter, so a thousand chapters against a
    // hundred is the linearity relation.
    group.operation("open and paginate the 1000-chapter book", |op| {
        let (bytes, _) = &*thousand;
        op.per(Per::Pages(1000));
        op.iter(|| {
            black_box(
                Document::open_with(Arc::clone(bytes), &at_box())
                    .expect("it opens")
                    .page_count(),
            )
        });
    });
    group.operation("render the 1000-chapter book at 72 dpi", |op| {
        render(op, &thousand.1, 1000)
    });
    group.operation("open and paginate the 100-chapter book", |op| {
        let (bytes, _) = &*hundred;
        op.per(Per::Pages(100));
        op.iter(|| {
            black_box(
                Document::open_with(Arc::clone(bytes), &at_box())
                    .expect("it opens")
                    .page_count(),
            )
        });
    });
    group.operation("render the 100-chapter book at 72 dpi", |op| {
        render(op, &hundred.1, 100)
    });

    // Markup laid out and written as a PDF.
    group.operation("the 20-page HTML report to PDF", |op| {
        let report = inputs::html_report();
        let page_box = inputs::default_box();
        let (_, laid) =
            DocumentBuilder::from_html(&report, "", page_box).expect("the report lays out");
        assert_eq!(laid.pages(), 20, "the HTML report's pages");
        op.per(Per::Pages(20));
        op.iter(|| {
            let (builder, _) =
                DocumentBuilder::from_html(&report, "", page_box).expect("it lays out");
            black_box(builder.finish())
        });
    });
    group.operation("the 20-page Markdown document to PDF", |op| {
        let markdown = inputs::markdown();
        let markdown_doc =
            Document::open_markdown(markdown.as_bytes().to_vec(), &OpenOptions::default())
                .expect("the Markdown document lays out");
        assert_eq!(
            markdown_doc.page_count(),
            20,
            "the Markdown document's pages"
        );
        op.per(Per::Pages(20));
        op.iter(|| {
            black_box(
                Document::open_markdown(markdown.as_bytes().to_vec(), &OpenOptions::default())
                    .expect("it lays out"),
            )
        });
    });

    // Each format a set at a time, every fixture of it in one iteration,
    // beside the same pages from the PDF the facade synthesised for each.
    group.operation("open each SVG fixture, render page 1", |op| {
        op.per(Per::Pages(svg[0].len() as u64));
        op.iter(|| black_box(open_and_render(&svg[0])));
    });
    group.operation("render each SVG fixture's synthesised PDF", |op| {
        op.per(Per::Pages(svg[1].len() as u64));
        op.iter(|| black_box(open_and_render(&svg[1])));
    });
    group.operation("open each XPS fixture, render page 1", |op| {
        op.per(Per::Pages(xps[0].len() as u64));
        op.iter(|| black_box(open_and_render(&xps[0])));
    });
    group.operation("render each XPS fixture's synthesised PDF", |op| {
        op.per(Per::Pages(xps[1].len() as u64));
        op.iter(|| black_box(open_and_render(&xps[1])));
    });
    group.operation("open each CBZ fixture, render page 1", |op| {
        op.per(Per::Pages(cbz[0].len() as u64));
        op.iter(|| black_box(open_and_render(&cbz[0])));
    });
    group.operation("render each CBZ fixture's synthesised PDF", |op| {
        op.per(Per::Pages(cbz[1].len() as u64));
        op.iter(|| black_box(open_and_render(&cbz[1])));
    });

    // Each committed book's every page through the CLI, read from where it
    // is committed: a PNG a page, as `tpdf render` writes them.
    group.operation(
        "render every page of calibre-book-cover.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "calibre-book-cover", &calibre_book_cover),
    );
    group.operation(
        "render every page of calibre-book-nocover.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "calibre-book-nocover", &calibre_book_nocover),
    );
    group.operation(
        "render every page of calibre-embedded-font.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "calibre-embedded-font", &calibre_embedded_font),
    );
    group.operation(
        "render every page of kcc-fixed-layout.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "kcc-fixed-layout", &kcc_fixed_layout),
    );
    group.operation(
        "render every page of pandoc-book-cover.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "pandoc-book-cover", &pandoc_book_cover),
    );
    group.operation(
        "render every page of pandoc-book-epub2.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "pandoc-book-epub2", &pandoc_book_epub2),
    );
    group.operation(
        "render every page of pandoc-book-nocover.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "pandoc-book-nocover", &pandoc_book_nocover),
    );
    group.operation(
        "render every page of pandoc-embedded-font.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "pandoc-embedded-font", &pandoc_embedded_font),
    );
    group.operation(
        "render every page of pandoc-plates.epub at 72 dpi, CLI",
        |op| render_book_cli(op, "pandoc-plates", &pandoc_plates),
    );

    group.finish();
}

criterion_group! {
    name = benches;
    config = support::config();
    targets = layout
}
criterion_main!(benches);
