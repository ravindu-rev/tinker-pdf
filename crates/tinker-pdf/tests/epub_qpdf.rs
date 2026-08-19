//! qpdf reads a synthesised book clean (gap 31 milestone 4, ruling 9).
//!
//! Everything in `tests/epub_package.rs` is this engine reading its own output.
//! A page tree only this parser walks, a content stream only this interpreter
//! decodes, an `/Info` written in a form nothing else accepts — all of those
//! render perfectly there and are wrong. qpdf is what says so, and gap 29's
//! milestone 5 found a defect **qpdf alone** caught because the renderer drew
//! the right picture either way.
//!
//! # What only qpdf can see here
//!
//! Milestone 4's pages are grey rectangles, so `--check` is the smaller half.
//! The larger half is `--filtered-stream-data`, which **decodes** a content
//! stream and hands over its operators: that turns *"every page is the neutral
//! placeholder"* from a claim this repository makes about its own renderer into
//! one a third-party tool makes about the bytes. A build whose placeholder was
//! white, or whose fill covered part of the page, draws differently and is
//! caught here rather than by a pixel comparison against itself.
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
//! when qpdf is installed somewhere `PATH` does not reach.

mod epub_support;

use std::path::{Path, PathBuf};
use std::process::Command;

use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::{Document, OpenOptions, WriteMode, WriteOptions};

/// Printed once per test that actually invoked qpdf. CI greps for it.
const RAN: &str = "qpdf-oracle: RAN";

/// Printed once per test that could not. CI greps for it too, and fails.
const SKIPPED: &str = "qpdf-oracle: SKIPPED";

fn qpdf() -> Option<PathBuf> {
    let named =
        std::env::var_os("TINKER_QPDF").map_or_else(|| PathBuf::from("qpdf"), PathBuf::from);
    let output = Command::new(&named).arg("--version").output().ok()?;
    output.status.success().then_some(named)
}

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
        .join("epub-qpdf")
        .join(test);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("the fixture is written");
    path
}

const CONTAINER_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
    r#"</rootfiles></container>"#
);

const PACKAGE_OPF: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
    r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
    r#"<dc:identifier id="pub-id">urn:uuid:2b6c0a10-0000-4000-8000-00000000000b</dc:identifier>"#,
    r#"<dc:title>A Book Read By Somebody Else</dc:title>"#,
    r#"<dc:language>en</dc:language>"#,
    r#"<dc:creator>The tinker-pdf authors</dc:creator></metadata>"#,
    r#"<manifest>"#,
    r#"<item id="c1" href="text/ch1.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"<item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"<item id="gone" href="text/missing.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"</manifest>"#,
    r#"<spine><itemref idref="c1"/><itemref idref="gone"/><itemref idref="c2"/></spine>"#,
    r#"</package>"#
);

fn chapter(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body><p>{body}</p></body></html>"#
    )
}

/// A book with a resolving spine item, one that does not, and a third that
/// does — so the document qpdf reads is the one a partial build produces rather
/// than the one it would like to.
fn book() -> Vec<u8> {
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", PACKAGE_OPF.as_bytes()),
        OcfEntry::deflated("EPUB/text/ch1.xhtml", chapter("One.").as_bytes()),
        OcfEntry::deflated("EPUB/text/ch2.xhtml", chapter("Two.").as_bytes()),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// The document the synthesiser hands the parser, checked by somebody else.
#[test]
fn qpdf_check_finds_nothing_wrong_with_a_synthesised_book() {
    let qpdf = oracle!("--check over a synthesised EPUB");

    let doc = Document::open(book()).expect("a book");
    assert_eq!(doc.page_count(), 3);
    let pdf = doc.editor().save(&WriteOptions::default());

    let path = fixture("synthesised", "book.pdf", &pdf);
    let (code, text) = run(&qpdf, &["--check", &path.display().to_string()]);
    assert_eq!(code, 0, "qpdf --check on the synthesised book:\n{text}");
    assert!(
        text.contains("No syntax or stream encoding errors found"),
        "qpdf found something:\n{text}"
    );
}

/// And the same document after [`Document::cos`] has been handed to the writer.
///
/// This is the exit criterion's own sentence: `cos()` returns the synthesised
/// document, and **saving it produces a file qpdf reads clean**.
#[test]
fn qpdf_check_finds_nothing_wrong_with_a_saved_synthesised_book() {
    let qpdf = oracle!("--check over a saved synthesised EPUB");

    let document = Document::open(book()).expect("a book");
    assert!(document.archive().is_some(), "it was synthesised");
    assert!(
        document.cos().catalog().is_some(),
        "cos() lends the synthesised document, catalog and all"
    );

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

/// qpdf agrees there are three pages, that none carries an image, and that
/// every one of them is the box `OpenOptions` stated.
///
/// **A page count and a page box are two claims** and both are read from qpdf
/// rather than from this engine: `--show-pages` walks the page tree and
/// `--show-object` prints each page's own `/MediaBox`. A build that wrote the
/// right number of pages at the wrong size satisfies the first and not the
/// second.
#[test]
fn qpdf_sees_the_spines_pages_at_the_box_the_caller_stated() {
    let qpdf = oracle!("--show-pages over a synthesised EPUB");

    let doc = Document::open_with(book(), &OpenOptions::at_page(300.0, 500.0)).expect("a book");
    let pdf = doc.editor().save(&WriteOptions::default());
    let path = fixture("pages", "book.pdf", &pdf);

    let (code, dump) = run(
        &qpdf,
        &["--show-pages", "--with-images", &path.display().to_string()],
    );
    assert_eq!(code, 0, "qpdf --show-pages:\n{dump}");

    // qpdf 12.3.2 prints `page 1: 3 0 R`, then an indented `content:` and the
    // stream's own reference on the line after it. Read from its real output
    // rather than from an assumption about the format, which six milestones of
    // this programme have had to correct after guessing.
    let pages = page_objects(&dump);
    assert_eq!(pages.len(), 3, "qpdf counts three pages:\n{dump}");
    assert!(
        !dump.contains("image"),
        "a book of placeholders carries no image XObject:\n{dump}"
    );

    // Each page's own object, read separately from the page dump so the two
    // readings cannot share a mistake.
    for object in &pages {
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
            flat.contains("/MediaBox [ 0 0 300 500 ]"),
            "object {object} is not the box the caller stated:\n{text}"
        );
    }
}

/// The object number of every page, out of `--show-pages`' own output.
fn page_objects(dump: &str) -> Vec<String> {
    dump.lines()
        .filter_map(|line| line.strip_prefix("page "))
        .filter_map(|rest| rest.split_once(": "))
        .map(|(_, reference)| {
            reference
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// The object number of every page's content stream, likewise: qpdf prints an
/// indented `content:` and then the reference on the next line.
fn content_objects(dump: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = dump.lines();
    while let Some(line) = lines.next() {
        if line.trim() != "content:" {
            continue;
        }
        let Some(reference) = lines.next() else {
            break;
        };
        out.push(
            reference
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned(),
        );
    }
    out
}

/// **Somebody else decodes the content stream and agrees the page is grey.**
///
/// `--filtered-stream-data` hands over a stream's bytes with its filters
/// applied, so what comes back is the operators this build wrote. The
/// assertion is on all three: the grey `0.749…` that is 0xBF, the `re` that
/// covers the whole page box, and the `f` that fills it. A build whose
/// placeholder was white would write `1 g`, and one that filled part of the
/// page would write a smaller rectangle — both render as *something* and only
/// this reads what it is.
#[test]
fn qpdf_decodes_a_placeholder_page_and_it_is_the_neutral_grey() {
    let qpdf = oracle!("--filtered-stream-data over a synthesised EPUB");

    let doc = Document::open(book()).expect("a book");
    let pdf = doc.editor().save(&WriteOptions::default());
    let path = fixture("streams", "book.pdf", &pdf);

    // Every content stream in the document, found by asking qpdf which objects
    // are streams rather than by counting on the writer's numbering.
    let (code, dump) = run(&qpdf, &["--show-pages", &path.display().to_string()]);
    assert_eq!(code, 0, "qpdf --show-pages:\n{dump}");
    let contents = content_objects(&dump);
    assert_eq!(
        contents.len(),
        3,
        "three pages, three content streams:\n{dump}"
    );

    for object in &contents {
        let (code, stream) = run(
            &qpdf,
            &[
                &format!("--show-object={object}"),
                "--filtered-stream-data",
                &path.display().to_string(),
            ],
        );
        assert_eq!(code, 0, "qpdf --show-object={object}:\n{stream}");
        // `0.7490196078431373 g 0 0 432 648 re f`, decoded by qpdf. The grey is
        // compared as a **number** rather than as the string it happens to be
        // printed as, because how many digits a writer emits is a formatting
        // choice and 191/255 is the claim.
        let words: Vec<&str> = stream.split_whitespace().collect();
        let grey: f64 = words
            .iter()
            .position(|word| *word == "g")
            .and_then(|at| words.get(at.wrapping_sub(1)))
            .and_then(|word| word.parse().ok())
            .unwrap_or_else(|| panic!("no grey level in: {stream}"));
        assert!(
            (grey - 191.0 / 255.0).abs() < 1e-9,
            "page content is not the neutral placeholder grey: {grey}"
        );
        let flat = words.join(" ");
        assert!(
            flat.contains("0 0 432 648 re"),
            "the fill does not cover the whole page: {flat}"
        );
        assert!(flat.ends_with(" f"), "nothing is filled: {flat}");
    }
}

/// The book's own metadata reaches the document's `/Info`, and qpdf reads it
/// back.
///
/// §5.5.3.1's three required Dublin Core elements are the criterion; a
/// `dc:title` that is parsed and thrown away is the failure this plan is
/// organised around. Read through qpdf rather than through this engine's own
/// metadata surface, because a `/Info` written in a form only this parser
/// accepts satisfies that surface and nothing else.
#[test]
fn qpdf_reads_the_books_title_out_of_the_synthesised_documents_info() {
    let qpdf = oracle!("--show-object=trailer over a synthesised EPUB");

    let doc = Document::open(book()).expect("a book");
    let pdf = doc.editor().save(&WriteOptions::default());
    let path = fixture("info", "book.pdf", &pdf);

    let (code, trailer) = run(
        &qpdf,
        &["--show-object=trailer", &path.display().to_string()],
    );
    assert_eq!(code, 0, "qpdf --show-object=trailer:\n{trailer}");
    let info = trailer
        .split_whitespace()
        .skip_while(|word| *word != "/Info")
        .nth(1)
        .expect("an /Info reference")
        .to_owned();

    let (code, text) = run(
        &qpdf,
        &[
            &format!("--show-object={info}"),
            &path.display().to_string(),
        ],
    );
    assert_eq!(code, 0, "qpdf --show-object={info}:\n{text}");
    assert!(
        text.contains("A Book Read By Somebody Else"),
        "the book's title did not reach /Title:\n{text}"
    );
    assert!(
        text.contains("The tinker-pdf authors"),
        "the book's creator did not reach /Author:\n{text}"
    );
}
