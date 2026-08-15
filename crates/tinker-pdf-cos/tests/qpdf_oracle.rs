//! qpdf as an external arbiter for linearized output (gap 20, ruling 9).
//!
//! Plan 09 names `qpdf --check` and `qpdf --show-linearization` as the
//! arbiters for Annex F, and until this file existed neither had ever run.
//! `tests/linearized.rs` checks everything the engine can check about its own
//! output; what it cannot check is whether the format was *understood*, and
//! the hint tables are where that mattered — they were written by this
//! repository and read by nothing in it, and they were wrong in five separate
//! ways while every test passed.
//!
//! # Ruling 9
//!
//! qpdf is a subprocess, never a dependency. Nothing links it, no part of it
//! is vendored, and its output is a transient comparison reference that is
//! read and dropped. The fixtures are written into `CARGO_TARGET_TMPDIR`
//! because qpdf reads files rather than pipes; nothing there is committed.
//!
//! # Skipped, not silently passed
//!
//! A check that quietly succeeds when its oracle is missing is a check that
//! will one day be missing everywhere. When qpdf cannot be found these tests
//! print [`SKIPPED`] and do nothing, and the CI job greps its own output for
//! [`RAN`] and fails if it does not find it — the same shape as the
//! `wasm-determinism` job, which had the same hazard: a run and a non-run both
//! exit zero and look identical.
//!
//! Set `TINKER_QPDF` to an absolute path when qpdf is installed somewhere the
//! `PATH` does not reach.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use tinker_pdf_cos::{
    CosDocument, DocumentBuilder, DocumentEditor, Encryption, WriteMode, WriteOptions,
};

/// Printed once per test that actually invoked qpdf. CI greps for it.
const RAN: &str = "qpdf-oracle: RAN";

/// Printed once per test that could not. CI greps for it too, and fails.
const SKIPPED: &str = "qpdf-oracle: SKIPPED";

/// Where qpdf is, or `None`.
///
/// A version query rather than a `which`: an executable that cannot run is
/// not an oracle, and this is the one call whose failure means *skip* rather
/// than *fail*.
fn qpdf() -> Option<PathBuf> {
    let named =
        std::env::var_os("TINKER_QPDF").map_or_else(|| PathBuf::from("qpdf"), PathBuf::from);
    let output = Command::new(&named).arg("--version").output().ok()?;
    output.status.success().then_some(named)
}

/// Runs qpdf and returns its exit code with stdout and stderr joined.
///
/// Joined because qpdf writes its warnings to stderr and its findings to
/// stdout, and every assertion here is about the pair.
fn run(qpdf: &Path, args: &[&str]) -> (i32, String) {
    let output = Command::new(qpdf)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("qpdf {args:?} did not run: {e}"));
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.code().unwrap_or(-1), text)
}

/// The oracle, or a printed skip.
///
/// Every test begins with this. The banner is deliberately shouted: a skipped
/// oracle in a green run is the thing this whole file exists to make visible.
macro_rules! oracle {
    ($what:expr) => {
        match qpdf() {
            Some(path) => {
                println!("{} {} ({})", RAN, $what, path.display());
                path
            }
            None => {
                println!(
                    "{} {} -- qpdf is not on PATH and TINKER_QPDF is unset",
                    SKIPPED, $what
                );
                return;
            }
        }
    };
}

// ---- Fixtures ---------------------------------------------------------------

/// The same document `linearized.rs` uses: one font every page shares.
fn document(page_count: usize) -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.set_info(b"Title", "linearized");
    for index in 0..page_count {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F0", 12.0, 10.0, 50.0, &format!("page {index}"));
        });
    }
    Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
}

/// A document with a real part 8: page one uses one font, the rest use
/// another that page one never touches, so it is shared between pages two
/// onward and belongs nowhere else.
fn shared_resource_document() -> Arc<CosDocument> {
    let mut builder = DocumentBuilder::new();
    builder.add_base_font(b"F0", b"Helvetica");
    builder.add_page(200.0, 100.0, |page| {
        page.text(b"F0", 12.0, 10.0, 50.0, "page 0");
    });
    builder.add_base_font(b"F1", b"Courier");
    for index in 1..6 {
        builder.add_page(200.0, 100.0, |page| {
            page.text(b"F1", 12.0, 10.0, 50.0, &format!("page {index}"));
        });
    }
    Arc::new(CosDocument::open(builder.finish()).expect("it opens"))
}

fn save(doc: Arc<CosDocument>, encryption: Option<Encryption>) -> Vec<u8> {
    DocumentEditor::new(doc).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        linearize: true,
        object_streams: false,
        encryption,
        ..WriteOptions::default()
    })
}

fn linearized(page_count: usize) -> Vec<u8> {
    save(document(page_count), None)
}

/// Writes a fixture where qpdf can read it and hands back the path.
///
/// `CARGO_TARGET_TMPDIR` is cargo's own scratch directory for an integration
/// test: inside `target/`, never committed, and the same on every platform.
fn fixture(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("qpdf-oracle");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("the fixture is written");
    path
}

/// Every fixture `--check` is pointed at, as (name, bytes).
fn unencrypted_fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        // One page: no shared objects at all, and every hint-table delta is
        // necessarily zero.
        ("one-page.pdf", linearized(1)),
        // Two pages: the smallest document with anything in the shared table
        // to get wrong, and the smallest one that failed before.
        ("two-page.pdf", linearized(2)),
        // Six: enough for a per-page delta to be non-zero more than once.
        ("six-page.pdf", linearized(6)),
        // And one with a part 8, which the other three do not have: their
        // only shared object is the font, and page one uses it, so F.3.8 puts
        // it in part 6 and leaves the shared section empty.
        (
            "shared-resource.pdf",
            save(shared_resource_document(), None),
        ),
    ]
}

// ---- Milestone 2: `qpdf --check` -------------------------------------------

/// The arbiter plan 09 names, run at last.
///
/// Before the hint-table fix this reported
/// `error encountered while checking linearization data: overflow reading bit
/// stream: wanted = 1; available = 0` on everything with more than one page,
/// and four further mismatches on the one-page fixture — `/T`, `/E`, the
/// first page object offset, and a page length — while the whole test suite
/// was green.
#[test]
fn qpdf_check_finds_nothing_wrong_with_the_linearized_output() {
    let qpdf = oracle!("--check over the linearized fixtures");

    for (name, bytes) in unencrypted_fixtures() {
        let path = fixture(name, &bytes);
        let (code, text) = run(&qpdf, &["--check", &path.display().to_string()]);

        assert!(
            text.contains("File is linearized"),
            "{name}: qpdf does not think it is linearized:\n{text}"
        );
        assert!(
            text.contains("No syntax or stream encoding errors found"),
            "{name}: qpdf found errors:\n{text}"
        );
        assert!(!text.contains("WARNING"), "{name}: qpdf warned:\n{text}");
        assert_eq!(code, 0, "{name}: qpdf exited {code}:\n{text}");
    }
}

/// `--check` alone would pass on a file with no `/Linearized` in it at all,
/// so this says the fixtures are the thing under test.
///
/// It is the same trap gap 19 found in `/H`: an assertion that holds whether
/// or not the feature works is not an assertion about the feature.
#[test]
fn qpdf_says_an_ordinary_file_is_not_linearized() {
    let qpdf = oracle!("--check over an unlinearized file");

    let bytes = DocumentEditor::new(document(6)).save(&WriteOptions {
        mode: WriteMode::Rewrite,
        object_streams: false,
        ..WriteOptions::default()
    });
    let path = fixture("not-linearized.pdf", &bytes);
    let (code, text) = run(&qpdf, &["--check", &path.display().to_string()]);

    assert_eq!(code, 0, "the ordinary layout is still valid:\n{text}");
    assert!(
        text.contains("File is not linearized"),
        "and qpdf can tell the two apart:\n{text}"
    );
}
