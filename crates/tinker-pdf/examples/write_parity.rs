//! The write-parity scripts, run against the facade itself (gap 32).
//!
//! ```text
//! cargo run -p tinker-pdf --example write_parity -- testdata/form-fields.pdf
//! ```
//!
//! This is the **reference surface**. Three bindings run the same two scripts
//! -- `bindings/python/tests/write_parity.py`,
//! `bindings/js/tests/write_parity.mjs`, and the write leg of
//! `bindings/dotnet/tests/Smoke` -- and `cargo xtask bindings-parity` requires
//! all four to print the same hashes. Ruling 11 is what makes that the right
//! test: a binding projects the facade 1:1 and adds no logic, so four
//! surfaces disagreeing means one of them added something.
//!
//! Two scripts, both with every input pinned:
//!
//! - **fill-and-save** opens the committed form fixture, fills its damaged
//!   field (one `SkippedWidget`, which must be *reported* rather than
//!   flattened into failure), fills its undamaged control field (no skipped
//!   widgets, which is what keeps "the report was non-empty" distinguishable
//!   from "the report is always non-empty"), ticks a checkbox whose on state
//!   is `/On` rather than `/Yes`, selects a radio option, and saves
//!   incrementally (7.5.6, so the original bytes survive as a prefix);
//! - **build-a-document** registers a base font and an image, draws two pages
//!   through `begin_page`/`push_page`, sets `/Info` and an outline, and
//!   finishes.
//!
//! The image is computed from a formula rather than read from a file, so four
//! languages produce the same 64 bytes with no fixture between them -- a
//! parity suite whose surfaces read the same *file* proves they can read a
//! file.
//!
//! Every artefact is put through this engine's strict structural validator
//! before its hash is printed. Under ruling 13 that check is first-party,
//! which is exactly why it can be a gate here rather than an external step
//! that might be skipped: four byte-identical outputs agreeing tells you
//! nothing if all four are wrong, and this is what rules that out.

use std::path::PathBuf;

use tinker_pdf::{
    DestKind, Document, DocumentBuilder, ImageData, OutlineEntry, Target, WriteMode, WriteOptions,
};

/// The eight-by-eight grey image both the Rust and the three binding scripts
/// build, from the same formula.
fn parity_image() -> Vec<u8> {
    (0..64u32).map(|i| ((i * 7) % 256) as u8).collect()
}

/// Hex of a SHA-256, the one hash this repository already owns.
fn sha256_hex(bytes: &[u8]) -> String {
    tinker_pdf_crypto::sha2::sha256(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Script one: open a form, fill it, save incrementally.
fn fill_and_save(fixture: &[u8]) -> Vec<u8> {
    let document = Document::open(fixture.to_vec()).expect("the form fixture opens");
    let mut editor = document.editor();

    let skipped = editor
        .fill_field("name", "Ada Lovelace")
        .expect("the value is taken -- ruling 2 degrades rather than failing");
    assert_eq!(
        skipped.len(),
        1,
        "the fixture's /Rect-less widget must be reported, not swallowed"
    );
    assert_eq!(skipped[0].to_string(), "7 0 R: no usable /Rect (12.5.2)");

    let clean = editor
        .fill_field("notes", "every surface writes this")
        .expect("the value is taken");
    assert!(
        clean.is_empty(),
        "the control field's widget is well formed, so nothing is skipped"
    );

    assert!(editor.set_checkbox("agree", true));
    assert!(editor.select_radio("colour", "red"));

    editor.save(&WriteOptions {
        mode: WriteMode::Incremental,
        ..WriteOptions::default()
    })
}

/// Script two: build a document from pages, a font and an image.
fn build_a_document() -> Vec<u8> {
    let samples = parity_image();
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F1", b"Helvetica");
    assert!(builder.add_image(
        b"Im1",
        &ImageData::Gray8 {
            width: 8,
            height: 8,
            data: &samples,
        }
    ));

    let mut one = builder.begin_page(200.0, 200.0);
    one.text(b"F1", 14.0, 20.0, 170.0, "Page one");
    one.fill_rect(20.0, 40.0, 60.0, 60.0, 0.25);
    one.image(b"Im1", 100.0, 40.0, 60.0, 60.0);
    builder.push_page(one);

    let mut two = builder.begin_page(200.0, 200.0);
    two.text(b"F1", 14.0, 20.0, 170.0, "Page two");
    builder.push_page(two);

    builder.set_info(b"Title", "tinker-pdf write parity");
    assert!(builder.set_outline(
        [(0u32, "Page one"), (1, "Page two")]
            .into_iter()
            .map(|(index, title)| OutlineEntry {
                title: title.to_string(),
                target: Some(Target::Page {
                    index,
                    view: DestKind::Fit,
                }),
                open: false,
                children: Vec::new(),
            })
            .collect()
    ));
    builder.finish()
}

/// Validates, then prints the line `cargo xtask bindings-parity` reads.
///
/// The validation is not decoration and not optional. A surface that printed a
/// hash without it would be claiming agreement about bytes nobody checked were
/// a document.
fn report(script: &str, bytes: &[u8]) {
    let document = Document::open(bytes.to_vec())
        .unwrap_or_else(|e| panic!("{script}: the artefact does not reopen: {e}"));
    let defects = document.validate();
    assert!(
        defects.is_empty(),
        "{script}: the artefact does not pass the strict validator: {:?}",
        defects.iter().map(|d| d.kind.as_str()).collect::<Vec<_>>()
    );
    println!(
        "WROTE sha256={} surface=facade script={script} bytes={}",
        sha256_hex(bytes),
        bytes.len()
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let fixture = args.next().map(PathBuf::from).unwrap_or_else(|| {
        eprintln!("usage: write_parity <form-fields.pdf>");
        std::process::exit(2);
    });
    let bytes =
        std::fs::read(&fixture).unwrap_or_else(|e| panic!("reading {}: {e}", fixture.display()));

    report("fill-and-save", &fill_and_save(&bytes));
    report("build-a-document", &build_a_document());
    println!("FACADE-PARITY: RAN");
}
