//! Extract text, with where it is rather than only what it says.
//!
//! `Page::text` needs no font faces and no rasterizer: the advances come from
//! the document's own `/Widths`, so a page that renders as blank for want of a
//! face still extracts perfectly. That asymmetry is the point of having this
//! example beside `render`.
//!
//! Run: `cargo run -p tinker-pdf --example extract [-- file.pdf]`

use tinker_pdf::Document;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        format!(
            "{}/../../testdata/simple-text.pdf",
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

    let mut characters = 0usize;
    for index in 0..doc.page_count() {
        let Some(page) = doc.page(index) else {
            continue;
        };
        let text = page.text();

        // The whole page as a string, which is what most callers want.
        let plain = text.plain_text();
        characters += plain.chars().count();
        if index == 0 {
            let first: String = plain.lines().take(3).collect::<Vec<_>>().join(" / ");
            println!("page 1    {first}");
        }

        // And the same characters with their positions, which is what a search
        // highlighter or a selection needs. Reported per line so the output
        // stays readable.
        if index == 0 {
            for line in text.lines().iter().take(2) {
                let (x0, y0, x1, y1) = line.quad.bounds();
                println!(
                    "  line    {x0:.1}, {y0:.1} to {x1:.1}, {y1:.1}  {:?}",
                    line.text.chars().take(40).collect::<String>()
                );
            }
        }

        for warning in &text.warnings {
            println!("  warning {warning:?}");
        }
    }
    println!(
        "total     {characters} characters over {} pages",
        doc.page_count()
    );
}
