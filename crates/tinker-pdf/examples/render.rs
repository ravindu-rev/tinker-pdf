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

    // `Bitmap::to_png` is the whole of it. This used to hand-roll a PNM here,
    // under a comment saying a PNM needs no encoder — true, and the reason it
    // was written that way was that there was no PNG encoder to call. There is
    // one now, it lives in the engine beside the zlib compressor and the
    // CRC-32 a PNG is made of, and `None` comes back only for a bitmap that is
    // not a picture: a zero dimension, or a buffer shorter than its own rows.
    let out = std::env::temp_dir().join("tinker-pdf-example.png");
    let png = bitmap
        .to_png()
        .expect("a rendered page is always a picture");
    std::fs::write(&out, png).expect("a file in the temporary directory");
    println!("wrote     {}", out.display());
}
