//! Rotate and crop a page, and save without touching the original bytes.
//!
//! `WriteMode::Incremental` appends: the output *starts with the input, byte
//! for byte*, which is the only way to modify a signed document without
//! breaking the signature over it. This example asserts that property rather
//! than describing it, because it is the reason to prefer this mode.
//!
//! Run: `cargo run -p tinker-pdf --example edit [-- file.pdf]`

use tinker_pdf::{Document, WriteMode, WriteOptions};

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        format!(
            "{}/../../testdata/simple-text.pdf",
            env!("CARGO_MANIFEST_DIR")
        )
    });
    let original = std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("{path}: {e}");
        std::process::exit(1);
    });
    let doc = Document::open(original.clone()).unwrap_or_else(|e| {
        eprintln!("{path}: {e:?}");
        std::process::exit(1);
    });

    let mut editor = doc.editor();
    if !editor.rotate_page(0, 90) {
        eprintln!("{path}: page 1 would not rotate");
        std::process::exit(1);
    }
    // A quarter in from each edge of whatever the page already is. 14.11.2's
    // crop box is what a viewer shows; the media box underneath is unchanged.
    if let Some(page) = doc.page(0) {
        let (x0, y0, x1, y1) = page.crop_box();
        let (dx, dy) = ((x1 - x0) / 4.0, (y1 - y0) / 4.0);
        editor.set_crop_box(0, x0 + dx, y0 + dy, x1 - dx, y1 - dy);
    }
    println!(
        "edited    rotated and cropped; dirty: {}",
        editor.is_dirty()
    );

    let saved = editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    });

    // The property that makes incremental saving worth having.
    assert!(
        saved.starts_with(&original),
        "an incremental save must leave the original bytes alone"
    );
    println!(
        "saved     {} bytes, of which the first {} are the original, untouched",
        saved.len(),
        original.len()
    );

    let again = Document::open(saved).expect("the saved document reopens");
    let page = again.page(0).expect("a first page");
    let (x0, y0, x1, y1) = page.crop_box();
    println!(
        "page 1    now {:.0} x {:.0} pt, rotated {}",
        x1 - x0,
        y1 - y0,
        page.rotation()
    );
}
