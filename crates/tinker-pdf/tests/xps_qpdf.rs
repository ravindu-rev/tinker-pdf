//! qpdf reads a synthesised fixed document clean (gap 30 milestone 4, ruling 9).
//!
//! Gap 30's design section makes this the fourth of its five arguments for
//! synthesising a PDF rather than emitting to the `Device` seam: *"Nothing
//! checks a `Device` call sequence. … A synthesised XPS is a PDF a third party
//! reads. A sequence of `fill_path` calls is a claim this repository makes
//! about itself."* This file is where that argument is cashed, and it is the
//! only test in gap 30 whose assertions come from outside this repository.
//!
//! It has earned its place once already, one gap over. Gap 29's milestone 5
//! found that **every page shared one image resource table** — 20 100 resource
//! entries for the 200 a 200-page archive uses — and qpdf alone caught it,
//! because the renderer drew the right picture either way. Everything in
//! `tests/xps_spine.rs` and `tests/xps_opc.rs` is this engine reading its own
//! output through its own parser; a page tree only this parser walks, or a
//! `/CropBox` written in a form nothing else accepts, passes every one of them.
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

mod xps_support;

use std::path::{Path, PathBuf};
use std::process::Command;

use tinker_pdf::{cbz, xps};
use tinker_pdf::{Document, WriteMode, WriteOptions};
use xps_support::{archive, document, fixed_page, fixed_page_with, one_page_package, part, with};

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
        .join("xps-qpdf")
        .join(test);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("the fixture is written");
    path
}

/// One of milestone 1's real packages, unmodified.
fn corpus(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/xps")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The bytes the synthesiser hands the parser, before the parser sees them.
fn synthesise(package: &[u8]) -> Vec<u8> {
    let opened =
        cbz::open_archive(package, &tinker_pdf_zip::Limits::DEFAULT).expect("a readable ZIP");
    match xps::route(opened, &xps::Limits::DEFAULT) {
        xps::Routing::Document(pdf, _) => pdf,
        xps::Routing::Refused(why) => panic!("the package was refused: {why}"),
        xps::Routing::NotXps(_) => panic!("the package is an XPS"),
    }
}

/// A three-page document whose markup order agrees with no order over its
/// names, and whose three pages are three different sizes.
///
/// The same fixture `tests/xps_spine.rs` builds, for the same reason and read
/// by a different program: every real package names its pages `1`, `2`, `3` and
/// references them in that order, so no file a producer wrote can tell markup
/// order from filename order.
fn out_of_order() -> Vec<u8> {
    archive(vec![
        part(
            "_rels/.rels",
            &xps_support::package_rels("/FixedDocumentSequence.fdseq"),
        ),
        part(
            "FixedDocumentSequence.fdseq",
            &xps_support::sequence(&["Documents/1/FixedDocument.fdoc"]),
        ),
        part(
            "Documents/1/FixedDocument.fdoc",
            &document(&["Pages/p10.fpage", "Pages/p1.fpage", "Pages/p2.fpage"]),
        ),
        part("Documents/1/Pages/p2.fpage", &fixed_page("200", "300")),
        part("Documents/1/Pages/p10.fpage", &fixed_page("816", "1056")),
        part("Documents/1/Pages/p1.fpage", &fixed_page("400", "600")),
        part("[Content_Types].xml", xps_support::CONTENT_TYPES),
    ])
}

/// The page objects qpdf found, in **qpdf's** page order.
///
/// Read out of `--show-pages` rather than assumed, because the object numbering
/// is the writer's business and a test that hardcoded it would break on a
/// change that is not a defect. The format is qpdf 12's own — `page 1: 3 0 R` —
/// and it was read off the tool before it was asserted against, which is the
/// lesson gap 29's milestone 5 recorded after writing this test's twin against
/// a format it had assumed.
fn page_objects(dump: &str) -> Vec<String> {
    dump.lines()
        .filter_map(|line| line.trim().strip_prefix("page "))
        .filter_map(|rest| rest.split_once(':'))
        .filter_map(|(_, reference)| reference.split_whitespace().next())
        .map(str::to_string)
        .collect()
}

/// One object, printed by qpdf and flattened to single spaces.
fn object(qpdf: &Path, path: &Path, number: &str) -> String {
    let (code, text) = run(
        qpdf,
        &[
            &format!("--show-object={number}"),
            &path.display().to_string(),
        ],
    );
    assert_eq!(code, 0, "qpdf --show-object={number}:\n{text}");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---- the structure ------------------------------------------------------

/// The document the synthesiser hands the parser, checked by somebody else.
///
/// A real package, because the whole of milestone 1 was about not proving this
/// gap's claims against files this repository wrote.
#[test]
fn qpdf_check_finds_nothing_wrong_with_a_synthesised_fixed_document() {
    let qpdf = oracle!("--check over a synthesised XPS");

    for name in [
        "wpf-three-pages.xps",
        "wpf-image-and-text.xps",
        "xpsom-gradients.oxps",
    ] {
        let pdf = synthesise(&corpus(name));
        let path = fixture("synthesised", &format!("{name}.pdf"), &pdf);
        let (code, text) = run(&qpdf, &["--check", &path.display().to_string()]);
        assert_eq!(code, 0, "qpdf --check on {name}:\n{text}");
        assert!(
            text.contains("No syntax or stream encoding errors found"),
            "{name}: qpdf found something:\n{text}"
        );
    }
}

/// And the same document after `Document::cos` has been handed to the writer.
///
/// This is row 4's own sentence: `cos()` returns the synthesised document, and
/// **saving it produces a file qpdf reads clean**. Both write modes, because
/// linearisation reorders the whole file and is the one gap 20 built this job
/// for.
#[test]
fn qpdf_check_finds_nothing_wrong_with_a_saved_fixed_document() {
    let qpdf = oracle!("--check over a saved synthesised XPS");

    let document = Document::open(corpus("wpf-three-pages.xps")).expect("a fixed document");
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

// ---- what the structure says --------------------------------------------

/// qpdf agrees the pages are in **markup order**, at the sizes the markup
/// states, and that **not one of them carries an image**.
///
/// Three independent readings and a third party doing all of them:
///
/// - `--show-pages` gives the page tree's own order, which is what a consumer
///   sees; the fixture's markup order agrees with no sort over its part names,
///   so a build that sorted would be caught here as well as in
///   `tests/xps_spine.rs`, by a program that shares none of its code.
/// - `--show-object` per page gives each `/MediaBox`, read from the page object
///   rather than out of the page dump, so the two readings cannot share a
///   mistake — gap 29's milestone-4 finding, where a pass-through and its
///   decoded twin disagreed and only reading both showed it.
/// - `--with-images` lists every image XObject a page carries, and a fixed
///   document carries **none**. That is the defect this whole gap exists for,
///   asserted from outside: before milestone 3 this same file opened as a
///   one-page comic whose page *was* a resource, and a build that regressed to
///   paging resources would put an image on a page here.
#[test]
fn qpdf_sees_the_pages_in_markup_order_at_the_sizes_the_markup_states() {
    let qpdf = oracle!("--show-pages over a synthesised XPS");

    let pdf = synthesise(&out_of_order());
    let path = fixture("pages", "out-of-order.pdf", &pdf);

    let (code, dump) = run(
        &qpdf,
        &["--show-pages", "--with-images", &path.display().to_string()],
    );
    assert_eq!(code, 0, "qpdf --show-pages:\n{dump}");

    let objects = page_objects(&dump);
    assert_eq!(objects.len(), 3, "qpdf counts three pages:\n{dump}");

    // `--with-images` prints a `W x H` line per image XObject. A fixed document
    // has no images at all, which is what a comic-paged package would not be
    // able to say.
    assert!(
        !dump.lines().any(|line| line.contains(" x ")),
        "no page of a fixed document carries an image XObject:\n{dump}"
    );

    // Markup order is p10, p1, p2 -- 816 x 1056, 400 x 600, 200 x 300 in units
    // of 1/96 inch. Natural order would be 612, 450, 225 tall and lexicographic
    // order 450, 792, 225.
    let wanted = [
        "/MediaBox [ 0 0 612 792 ]",
        "/MediaBox [ 0 0 300 450 ]",
        "/MediaBox [ 0 0 150 225 ]",
    ];
    for (at, (number, want)) in objects.iter().zip(wanted.iter()).enumerate() {
        let flat = object(&qpdf, &path, number);
        assert!(
            flat.contains(want),
            "page {} wanted {want}:\n{flat}",
            at + 1
        );
    }
}

/// qpdf sees the two rectangles a fixed page stated, and sees none where a page
/// stated none.
///
/// 10.3.1's `ContentBox` becomes `/CropBox` and 10.3.2's `BleedBox` becomes
/// `/BleedBox`, once scaled by 1/96-to-1/72 and once flipped from XPS's
/// top-left origin to PDF's bottom-left. `tests/xps_spine.rs` asserts the
/// numbers through this engine's own reader, which normalises a rectangle and
/// clips a crop box to the media box on the way — so it cannot see a box
/// written back to front or one written outside the page. qpdf prints the array
/// as the file holds it.
///
/// The second page states nothing, and qpdf finds no key at all rather than a
/// box equal to the media box: 7.7.3.3 makes those two different documents.
#[test]
fn qpdf_sees_the_boxes_a_fixed_page_stated_and_none_where_it_stated_none() {
    let qpdf = oracle!("--show-object over a fixed page's boxes");

    let parts = with(
        with(
            one_page_package(),
            "Documents/1/FixedDocument.fdoc",
            &document(&["Pages/1.fpage", "Pages/2.fpage"]),
        ),
        "Documents/1/Pages/1.fpage",
        &fixed_page_with(
            "816",
            "1056",
            r#"ContentBox="96,48,192,96" BleedBox="48,24,720,1008" "#,
        ),
    );
    let parts = with(
        parts,
        "Documents/1/Pages/2.fpage",
        &fixed_page("816", "1056"),
    );
    let pdf = synthesise(&archive(parts));
    let path = fixture("boxes", "boxes.pdf", &pdf);

    let (code, dump) = run(&qpdf, &["--show-pages", &path.display().to_string()]);
    assert_eq!(code, 0, "qpdf --show-pages:\n{dump}");
    let objects = page_objects(&dump);
    assert_eq!(objects.len(), 2, "two pages:\n{dump}");

    let stated = object(&qpdf, &path, objects.first().expect("the first page"));
    assert!(
        stated.contains("/MediaBox [ 0 0 612 792 ]"),
        "the page keeps its own size:\n{stated}"
    );
    assert!(
        stated.contains("/CropBox [ 72 684 216 756 ]"),
        "ContentBox=\"96,48,192,96\" scaled once and flipped once:\n{stated}"
    );
    assert!(
        stated.contains("/BleedBox [ 36 18 576 774 ]"),
        "BleedBox=\"48,24,720,1008\" likewise:\n{stated}"
    );

    let bare = object(&qpdf, &path, objects.get(1).expect("the second page"));
    assert!(
        bare.contains("/MediaBox [ 0 0 612 792 ]"),
        "the same page size:\n{bare}"
    );
    assert!(
        !bare.contains("/CropBox") && !bare.contains("/BleedBox"),
        "a page that stated no box carries no key:\n{bare}"
    );
}
