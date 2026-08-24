//! Open a document and say what the reader had to do to it.
//!
//! The interesting half is not `page_count` — it is `ladder_level` and
//! `warnings`. This engine never fails silently (ruling 2), so a file that
//! needed repairing opens *and says so*, and a program that ignores both
//! cannot tell a pristine document from one it rebuilt by scanning.
//!
//! Run: `cargo run -p tinker-pdf --example open [-- file.pdf]`

use tinker_pdf::{Document, LadderLevel};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        format!(
            "{}/../../testdata/simple-text.pdf",
            env!("CARGO_MANIFEST_DIR")
        )
    });

    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{path}: {error}");
            std::process::exit(1);
        }
    };

    let doc = match Document::open(bytes) {
        Ok(doc) => doc,
        Err(error) => {
            eprintln!("{path}: {error:?}");
            std::process::exit(1);
        }
    };

    println!("pages     {}", doc.page_count());
    println!("encrypted {}", doc.is_encrypted());

    // How much the reader had to work. `Trust` means the cross-reference table
    // was believed as written; anything else means it was not, and the
    // warnings below say which part.
    match doc.ladder_level() {
        LadderLevel::Trust => println!("opened    straight from the cross-reference table"),
        other => println!("opened    at {other:?} — the table was not enough"),
    }

    let warnings = doc.warnings();
    if warnings.is_empty() {
        println!("warnings  none");
    } else {
        println!("warnings  {}", warnings.len());
        for warning in warnings.iter().take(10) {
            println!("  {:?}", warning.kind);
        }
    }

    if let Some(page) = doc.page(0) {
        let (x0, y0, x1, y1) = page.crop_box();
        println!(
            "page 1    {:.0} x {:.0} pt, rotated {}",
            x1 - x0,
            y1 - y0,
            page.rotation()
        );
    }
}
