//! Read something that is not a PDF as a document.
//!
//! `Document::open` takes an EPUB, an XPS or OpenXPS package, or a comic
//! archive, and hands back the same `Document` a PDF would give — so every
//! example beside this one works on them unchanged. The signatures are tested
//! at fixed positions, so a PDF that happens to contain `PK\x03\x04` in a
//! stream is unaffected.
//!
//! One of these is not like the others. An EPUB is **reflowable**: its page
//! count is a function of the box you ask for and is not a property of the
//! file, which is why `OpenOptions` exists and why this example asks twice.
//!
//! Run: `cargo run -p tinker-pdf --example convert [-- book.epub]`

use tinker_pdf::{Document, OpenOptions};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        format!(
            "{}/tests/epub/pandoc-book-cover.epub",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(1);
    });

    let doc = Document::open(bytes.clone()).unwrap_or_else(|e| {
        eprintln!("{path}: {e:?}");
        std::process::exit(1);
    });
    let name = path.rsplit(['/', '\\']).next().unwrap_or(&path);
    println!("opened    {name} as a document");
    println!("pages     {}", doc.page_count());

    if let Some(page) = doc.page(0) {
        let (x0, y0, x1, y1) = page.crop_box();
        println!("page 1    {:.0} x {:.0} pt", x1 - x0, y1 - y0);
    }

    // The first page carrying any text at all, which for a book with a cover
    // plate is not page one — worth saying out loud, because "page 1 is empty"
    // reads as a defect and is a cover.
    for index in 0..doc.page_count() {
        let Some(page) = doc.page(index) else {
            continue;
        };
        let text = page.text().plain_text();
        if let Some(line) = text.lines().find(|l| !l.trim().is_empty()) {
            println!(
                "text      first on page {}: {:?}",
                index + 1,
                line.chars().take(60).collect::<String>()
            );
            break;
        }
    }

    // The same book at a different page box. For a PDF, an XPS or a comic
    // these two numbers are equal and the option changes nothing; for a
    // reflowable book they differ, and a build that ignored the argument
    // would be stable twice over and exactly as wrong as one that paginated
    // at random.
    let narrow = Document::open_with(bytes, &OpenOptions::at_page(300.0, 400.0))
        .expect("it opens at another box");
    println!(
        "at 300x400 pt: {} pages (was {} at the default box)",
        narrow.page_count(),
        doc.page_count()
    );

    for warning in doc.warnings().iter().take(5) {
        println!("warning   {:?}", warning.kind);
    }
}
