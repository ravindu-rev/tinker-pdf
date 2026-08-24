//! Render a page to pixels, and keep the warnings that came with them.
//!
//! Two things worth copying. The first is that `Bitmap::warnings` is part of
//! the result rather than a log: ruling 2 says a page that could not be drawn
//! correctly still comes back, with a placeholder and a named reason, so a
//! caller that drops the warnings has thrown away the only signal that the
//! picture is wrong.
//!
//! The second is that this engine bundles no font faces. A document that names
//! Helvetica and embeds nothing extracts its text perfectly and draws none of
//! it, reported as `RenderWarning::UnreadableFont` — see `FontProvider` and
//! `docs/features/fonts.md`, where the corpus-wide cost of that is measured.
//!
//! Run: `cargo run -p tinker-pdf --example render [-- file.pdf]`

use std::io::Write;

use tinker_pdf::{Document, RenderOptions};

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
    let Some(page) = doc.page(0) else {
        eprintln!("{path}: no pages");
        std::process::exit(1);
    };

    let bitmap = page.render(&RenderOptions::at_dpi(150.0));
    println!(
        "rendered  {} x {} px, {} bytes, {} per pixel",
        bitmap.width,
        bitmap.height,
        bitmap.data.len(),
        bitmap.components()
    );

    // Never a log line: the warnings are how a caller learns that what came
    // back is a placeholder rather than the page.
    if bitmap.warnings.is_empty() {
        println!("warnings  none — this is the page, not an approximation of it");
    } else {
        for warning in &bitmap.warnings {
            println!("warning   {warning:?}");
        }
    }

    // Written as a PNM because it needs no encoder and every image viewer on
    // every platform reads it. `Bitmap::data` is plain interleaved bytes.
    let out = std::env::temp_dir().join("tinker-pdf-example.pnm");
    let mut file = std::fs::File::create(&out).expect("a file in the temporary directory");
    let magic = if bitmap.components() >= 3 { "P6" } else { "P5" };
    write!(file, "{magic}\n{} {}\n255\n", bitmap.width, bitmap.height).expect("the header");
    let wanted = if bitmap.components() >= 3 { 3 } else { 1 };
    for row in bitmap.data.chunks_exact(bitmap.stride) {
        for pixel in row
            .chunks_exact(bitmap.components())
            .take(bitmap.width as usize)
        {
            file.write_all(&pixel[..wanted]).expect("a pixel");
        }
    }
    println!("wrote     {}", out.display());
}
