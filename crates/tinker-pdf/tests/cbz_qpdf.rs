//! qpdf reads a synthesised comic archive clean (gap 29 milestone 5, ruling 9).
//!
//! Gap 29's design section says why this test is the reason synthesis was
//! chosen over an enum: *"A synthesised document can be saved and handed to
//! qpdf, which ruling 9 already puts in CI for gap 20, so 'the CBZ produced a
//! valid document' becomes a claim a third-party tool checks rather than a
//! claim this repository makes about itself."*
//!
//! Everything else in `tests/cbz.rs` is this engine reading its own output. A
//! `/DecodeParms` that only this reader honours, a page tree only this parser
//! walks, an `/Indexed` palette written in a form nothing else accepts — all
//! of those render perfectly here and are wrong. qpdf is what says so.
//!
//! # Ruling 9
//!
//! qpdf is a subprocess, never a dependency. Nothing links it, no part of it is
//! vendored, and its output is a transient comparison reference. The fixtures
//! go in `CARGO_TARGET_TMPDIR`, which is inside `target/` and never committed.
//!
//! # Skipped, not silently passed
//!
//! Gap 20 found that a skipped oracle exits 0 and reads exactly like a pass.
//! These tests print [`RAN`] or [`SKIPPED`] and the CI job greps its own output
//! for the first and fails on the second. Set `TINKER_QPDF` to an absolute path
//! when qpdf is installed somewhere `PATH` does not reach; quote it if the path
//! has spaces.

mod cbz_support;

use std::path::{Path, PathBuf};
use std::process::Command;

use cbz_support::{distinct_pixels, grey_jpeg, rgb_png, zip, Damage, ZipFile};
use tinker_pdf::cbz;
use tinker_pdf::{Container, Document, WriteMode, WriteOptions};

/// Printed once per test that actually invoked qpdf. CI greps for it.
const RAN: &str = "qpdf-oracle: RAN";

/// Printed once per test that could not. CI greps for it too, and fails.
const SKIPPED: &str = "qpdf-oracle: SKIPPED";

/// Where qpdf is, or `None`.
///
/// A version query rather than a `which`: an executable that cannot run is not
/// an oracle, and this is the one call whose failure means *skip* rather than
/// *fail*.
fn qpdf() -> Option<PathBuf> {
    let named =
        std::env::var_os("TINKER_QPDF").map_or_else(|| PathBuf::from("qpdf"), PathBuf::from);
    let output = Command::new(&named).arg("--version").output().ok()?;
    output.status.success().then_some(named)
}

/// Runs qpdf and returns its exit code with stdout and stderr joined.
fn run(qpdf: &Path, args: &[&str]) -> (i32, String) {
    let output = Command::new(qpdf)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("qpdf {args:?} did not run: {e}"));
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    (output.status.code().unwrap_or(-1), text)
}

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

/// Writes a fixture where qpdf can read it and hands back the path.
///
/// A directory per test, because cargo runs them on threads and two of them
/// would otherwise rewrite a file the other has open — which on Windows fails
/// outright and elsewhere fails worse, with qpdf reading half a file and
/// reporting a defect that is not there.
fn fixture(test: &str, name: &str, bytes: &[u8]) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("cbz-qpdf")
        .join(test);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("the fixture is written");
    path
}

/// An archive that exercises every shape this milestone can put on a page.
///
/// Both image routes (a JPEG placed verbatim, a PNG passed through as
/// `/FlateDecode` with `/Predictor 15`), a placeholder page, an entry that is
/// not a page at all, and names that only sort correctly under natural order.
fn comic() -> Vec<u8> {
    zip(
        &[
            ZipFile::stored("ComicInfo.xml", b"<?xml version=\"1.0\"?><ComicInfo/>"),
            ZipFile::stored("p10.png", &rgb_png(6, 4, &distinct_pixels(6, 4))),
            ZipFile::stored("p1.jpg", &grey_jpeg(16, 8)),
            ZipFile::deflated("p2.png", &rgb_png(5, 9, &distinct_pixels(5, 9))),
            ZipFile::stored("p3.gif", b"GIF89a\x04\x00\x04\x00\x00\x00\x00;"),
        ],
        Damage::None,
    )
}

/// The document the synthesiser hands the parser, checked by somebody else.
#[test]
fn qpdf_check_finds_nothing_wrong_with_a_synthesised_comic() {
    let qpdf = oracle!("--check over a synthesised CBZ");

    let (pdf, report) =
        cbz::synthesise(Container::Zip, &comic(), &cbz::Limits::DEFAULT).expect("it synthesises");
    assert_eq!(report.pages().len(), 4);

    let path = fixture("synthesised", "comic.pdf", &pdf);
    let (code, text) = run(&qpdf, &["--check", &path.display().to_string()]);
    assert_eq!(code, 0, "qpdf --check on the synthesised document:\n{text}");
    assert!(
        text.contains("No syntax or stream encoding errors found"),
        "qpdf found something:\n{text}"
    );
}

/// And the same document after `Document::cos` has been handed to the writer.
///
/// This is the exit criterion's own sentence: `cos()` returns the synthesised
/// document, and **saving it produces a file qpdf reads clean**.
#[test]
fn qpdf_check_finds_nothing_wrong_with_a_saved_synthesised_document() {
    let qpdf = oracle!("--check over a saved synthesised CBZ");

    let document = Document::open(comic()).expect("the archive opens");
    assert!(document.archive().is_some(), "it was synthesised");

    for (name, linearize) in [("rewritten.pdf", false), ("linearized.pdf", true)] {
        let saved = document.editor().save(&WriteOptions {
            mode: WriteMode::Rewrite,
            linearize,
            ..WriteOptions::default()
        });
        let path = fixture("saved", name, &saved);
        let (code, text) = run(&qpdf, &["--check", &path.display().to_string()]);
        assert_eq!(code, 0, "qpdf --check on {name}:\n{text}");
        assert!(
            text.contains("No syntax or stream encoding errors found"),
            "{name}: qpdf found something:\n{text}"
        );
        if linearize {
            assert!(
                text.contains("File is linearized"),
                "{name}: qpdf does not think it is linearized:\n{text}"
            );
        }
    }
}

/// qpdf agrees the pages are there, in the order this build put them, at the
/// sizes the images are.
///
/// `--check` above reads the structure; this reads what the structure *says*. A
/// page tree qpdf can walk whose media boxes are not the images' own pixel
/// sizes, or whose pages are in the order the archive stored them rather than
/// the order a reader wants, passes that check and is still the wrong document.
///
/// Both halves are read from qpdf rather than from this engine. `--show-pages
/// --with-images` prints each page's image XObjects with the dimensions qpdf
/// resolved from the *image dictionaries*, and `--show-object` prints the
/// `/MediaBox` from the page. The two are independent: a build that wrote a
/// correct `/MediaBox` over an image whose `/Width` disagreed would satisfy one
/// and not the other, which is the failure milestone 4 found in the mask and
/// the reason it is worth reading both.
#[test]
fn qpdf_sees_the_pages_at_the_sizes_the_images_are() {
    let qpdf = oracle!("--show-pages over a synthesised CBZ");

    let (pdf, _) =
        cbz::synthesise(Container::Zip, &comic(), &cbz::Limits::DEFAULT).expect("it synthesises");
    let path = fixture("pages", "comic.pdf", &pdf);

    let (code, dump) = run(
        &qpdf,
        &["--show-pages", "--with-images", &path.display().to_string()],
    );
    assert_eq!(code, 0, "qpdf --show-pages:\n{dump}");

    // Natural order, which is the whole point of the fixture's names: stored
    // as p10, p1, p2, p3 and paged as p1, p2, p3, p10. Lexicographic order
    // would put the 6 x 4 page first.
    let pages: Vec<&str> = dump
        .match_indices("page ")
        .map(|(i, _)| &dump[i..])
        .collect();
    assert_eq!(pages.len(), 4, "qpdf counts four pages:\n{dump}");

    // Page 3 is the GIF, which this build cannot decode, so it is the
    // placeholder -- it has no image at all, and that is what distinguishes a
    // placeholder from a page that merely rendered badly.
    let images: Vec<&str> = dump.lines().filter(|l| l.contains(" x ")).collect();
    assert_eq!(
        images.len(),
        3,
        "three of the four pages carry an image; the placeholder carries none:\n{dump}"
    );
    assert!(images[0].contains("16 x 8"), "page 1 is the JPEG:\n{dump}");
    assert!(
        images[1].contains("5 x 9"),
        "page 2 is the deflated PNG:\n{dump}"
    );
    assert!(
        images[2].contains("6 x 4"),
        "page 4 is the stored PNG:\n{dump}"
    );

    // The media boxes, read per page object rather than out of the page dump,
    // so the two readings cannot share a mistake. The placeholder takes the
    // first real page's size, which `PLACEHOLDER_SIZE`'s doc comment argues
    // for: a comic's pages are one size and a placeholder that matches its
    // neighbours does not make the book jump.
    let boxes = ["0 0 16 8", "0 0 5 9", "0 0 16 8", "0 0 6 4"];
    for (page, want) in boxes.iter().enumerate() {
        let object = format!("{}", 6 + page);
        let (code, text) = run(
            &qpdf,
            &[
                &format!("--show-object={object}"),
                &path.display().to_string(),
            ],
        );
        assert_eq!(code, 0, "qpdf --show-object={object}:\n{text}");
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains(&format!("/MediaBox [ {want} ]")),
            "page {} wanted /MediaBox [ {want} ]:\n{text}",
            page + 1
        );
    }
}
