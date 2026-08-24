//! Build a document from nothing, and read back what was written.
//!
//! `DocumentBuilder` is the other half of the library: without it a caller
//! depending on this crate could read a document and never produce one. Every
//! byte here is written by this workspace — the deflate encoder, the
//! cross-reference table, the font dictionaries.
//!
//! The last third of this example matters more than the first two: it opens
//! what it just wrote and asserts the text and the outline come back. A
//! writer proved only by "it produced bytes" is not proved at all.
//!
//! Run: `cargo run -p tinker-pdf --example create [-- out.pdf]`

use tinker_pdf::{DestKind, Document, DocumentBuilder, OutlineEntry, Target};

fn main() {
    let mut builder = DocumentBuilder::new();
    builder.set_info(b"Title", "A document built from nothing");
    builder.set_info(b"Creator", "tinker-pdf, examples/create.rs");
    builder.add_base_font(b"F0", b"Helvetica");

    let chapters = ["Openings", "Middles", "Endings"];
    for (index, chapter) in chapters.iter().enumerate() {
        builder.add_page(400.0, 300.0, |page| {
            // A tinted band across the top, to show that this is drawing and
            // not only typesetting.
            page.set_fill_rgb(0.90, 0.93, 0.98);
            page.fill_rect(0.0, 250.0, 400.0, 50.0, 0.0);
            page.set_fill_rgb(0.0, 0.0, 0.0);
            page.text(b"F0", 22.0, 40.0, 265.0, chapter);
            page.text(
                b"F0",
                11.0,
                40.0,
                200.0,
                &format!("Page {} of {}.", index + 1, chapters.len()),
            );
            if index + 1 < chapters.len() {
                page.text(
                    b"F0",
                    11.0,
                    40.0,
                    180.0,
                    "The next chapter is one link away.",
                );
                page.link(
                    40.0,
                    176.0,
                    240.0,
                    192.0,
                    &Target::Page {
                        index: (index + 1) as u32,
                        view: DestKind::Fit,
                    },
                );
            }
        });
    }

    builder.set_outline(
        chapters
            .iter()
            .enumerate()
            .map(|(index, chapter)| OutlineEntry {
                title: (*chapter).to_string(),
                target: Some(Target::Page {
                    index: index as u32,
                    view: DestKind::Fit,
                }),
                open: false,
                children: Vec::new(),
            })
            .collect(),
    );

    let bytes = builder.finish();
    println!("built     {} bytes, {} pages", bytes.len(), chapters.len());

    // Read it back. This is the half that makes the example evidence rather
    // than a demonstration.
    let doc = Document::open(bytes.clone()).expect("what was written opens");
    assert_eq!(doc.page_count(), chapters.len() as u32);
    let first = doc.page(0).expect("a first page");
    let text = first.text().plain_text();
    assert!(text.contains("Openings"), "the text came back: {text:?}");
    println!(
        "read back page 1 says {:?}",
        text.lines().next().unwrap_or("")
    );
    println!("outline   {} entries", doc.outline().len());

    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(&path, &bytes).expect("writing the document");
        println!("wrote     {path}");
    }
}
