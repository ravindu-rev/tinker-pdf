//! A float reads where it was written, and a forced break does not move it.
//!
//! # Why this file exists
//!
//! `epub_fetched.rs` holds this shape against `pg16328-beowulf.epub`, and that
//! book has to be fetched. A defect held only by a fetched corpus is a defect
//! nobody else can verify: the sweep is skipped on a machine with no network,
//! and a skip exits zero. So the shape is here as well, as a few hundred bytes
//! of markup — and this file runs everywhere, always, with no corpus at all.
//!
//! # The shape
//!
//! Project Gutenberg's *Beowulf* sets a marginal gloss beside almost every
//! chapter heading — `<span class="sidenote">`, `display: block; float: right;
//! clear: right` — and its headings force a page break. Both halves are needed:
//!
//! - **the forced break**, which ends a page early, so that page is much
//!   shorter than the page box; and
//! - **the float**, whose static position is past that break.
//!
//! `fragment::beside` used to decide which floats a page draws by asking
//! whether the float's position was within `top + page height` of the page's
//! top. That is the right question only when a page is as tall as the page box.
//! After a forced break it is not, and the reach ran past the break into the
//! next page's column: the gloss was drawn on the page *before* the one it
//! belonged to, and came out before the heading it was written after.
//!
//! The question §9.5 actually asks is whether the float sits beside *this*
//! page's content, so the reach is now where the next page's column begins.

#[path = "epub_support/mod.rs"]
mod epub_support;

use epub_support::conservation::conservation;
use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::{Document, OpenOptions};

const CONTAINER_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
    r#"</rootfiles></container>"#
);

const PACKAGE: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
    r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
    r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-00000000f10a</dc:identifier>"#,
    r#"<dc:title>A Book With Marginal Glosses</dc:title>"#,
    r#"<dc:language>en</dc:language>"#,
    r#"<dc:creator>The tinker-pdf authors</dc:creator>"#,
    r#"</metadata><manifest>"#,
    r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
);

/// Project Gutenberg's stylesheet, less what does not bear on the shape.
const STYLE: &str = "h2 { page-break-before: always; margin: 0 } \
     p { margin: 0 } \
     span.sidenote { display: block; float: right; clear: right; width: 40%; \
     margin: 0; text-align: left }";

/// Two chapters of the shape, which is one more than the defect needs.
///
/// One reproduces it; two are here because it **accumulates** — each gloss
/// clears the one before it — and a fixture of one could not tell a build that
/// placed the first float correctly and drifted after that.
fn body() -> String {
    // **A paragraph before the first heading, and it is the fixture.** The
    // defect needs a page that ends *early*, and only a page with something on
    // it before the forced break is one: a book that opens with its first
    // heading has no such page and reproduces nothing.
    let mut out = String::from("<p>opening line</p>");
    for chapter in ["ONE", "TWO"] {
        out.push_str(&format!("<h2>{chapter}</h2>"));
        out.push_str(&format!(
            "<span class=\"sidenote\">gloss of {chapter} here</span>"
        ));
        for line in 1..=4 {
            out.push_str(&format!("<p>{chapter} body line {line}</p>"));
        }
    }
    out
}

fn glossed_book() -> Vec<u8> {
    let chapter = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Glosses</title>"#,
            r#"<style>{}</style></head><body>{}</body></html>"#
        ),
        STYLE,
        body()
    );
    let entries = vec![
        OcfEntry::stored("mimetype", b"application/epub+zip"),
        OcfEntry::deflated("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        OcfEntry::deflated("EPUB/content.opf", PACKAGE.as_bytes()),
        OcfEntry::deflated("EPUB/ch1.xhtml", chapter.as_bytes()),
    ];
    let directory: Vec<usize> = (0..entries.len()).collect();
    ocf_zip(&entries, &directory)
}

/// Every character back, in order, once — at fifteen page boxes rather than
/// one.
///
/// **The sweep is the fixture**, and its floor is chosen rather than round.
/// The defect is a relationship between where a forced break falls and how
/// tall a page box is, so one height is a fixture for one coincidence.
/// Measured against the old reach, this book diverges by 3 characters at every
/// page from 170 to 250 points, by 6 at 260, and by 130 at 280 — and by none
/// at 160, which is why the sweep starts above it.
///
/// **Below 160 points it does not belong here.** There the gloss is taller
/// than the space beside it and is *broken across pages* instead, which is a
/// second defect with a warning of its own (`FloatBrokenAcrossPages`) and is
/// still open. A fixture that reproduced both would be evidence for neither.
#[test]
fn a_gloss_after_a_forced_break_reads_where_it_was_written() {
    let bytes = glossed_book();
    for tenths in 16..=30 {
        let height = f64::from(tenths) * 10.0;
        let doc = Document::open_with(bytes.clone(), &OpenOptions::at_page(200.0, height))
            .expect("the book opens");
        let verdict = conservation(&bytes, &doc);
        assert!(
            verdict.holds(),
            "at a {height}-point page the gloss moved: {} extra, {} missing, {:?}",
            verdict.extra,
            verdict.missing,
            verdict.divergences
        );
    }
}

/// The mechanism, asserted where conservation cannot see it: the gloss is on
/// the page its own chapter is on, and **not** the page before.
///
/// Conservation says the order came out right; this says the float is where
/// §9.5 put it rather than one page early with its reading order accidentally
/// undisturbed.
#[test]
fn the_gloss_is_drawn_on_its_own_chapters_page() {
    let bytes = glossed_book();
    let doc = Document::open_with(bytes, &OpenOptions::at_page(200.0, 180.0)).expect("a book");
    let pages: Vec<String> = (0..doc.page_count())
        .map(|at| doc.page(at).expect("a page").text().plain_text())
        .collect();
    let heading = pages
        .iter()
        .position(|page| page.split_whitespace().any(|word| word == "ONE"))
        .expect("the first chapter's heading is on a page");
    assert!(heading > 0, "the chapter starts the book: {pages:?}");
    let flat = |at: usize| -> String { pages[at].chars().filter(|c| !c.is_whitespace()).collect() };
    assert!(
        flat(heading).contains("glossofONE"),
        "the gloss is not on its chapter's page: {pages:?}"
    );
    assert!(
        !flat(heading - 1).contains("glossofONE"),
        "the gloss is on the page before its chapter, which is the defect: {pages:?}"
    );
}
