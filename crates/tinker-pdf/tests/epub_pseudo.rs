//! `::before` and `::after`, from a stylesheet to a box on a page.
//!
//! `crates/tinker-pdf-css/src/tests/pseudo.rs` is the cascade half — what the
//! box inherits, when it exists, that the originating element is still not
//! styled. This is the other half, and the half that could not be written on
//! the CSS side at all: **layout has no selector engine**, so the box has to be
//! generated before layout sees anything, and the seam is
//! `StyleTree::pseudo(element, which)` called from `epub::read::build`.
//!
//! # What is generated, and what is refused by name
//!
//! `content` reads **strings**, **`attr()`**, and any concatenation of the two.
//! Three families are refused, each as `Unsupported { property: "content" }`
//! so a book that asks is counted rather than quietly given an empty box:
//!
//! * `url()` / `<image>` — a replaced element, which needs a size before the
//!   image is fetched. A different layout question from this one.
//! * `counter()` / `counters()` — these need `counter-reset`,
//!   `counter-increment` and a scoped counter tree, none of which exist here. A
//!   `counter()` resolved to nothing would number every list item zero.
//! * `open-quote` and its three siblings — these read the `quotes` property,
//!   which is still unimplemented and which pandoc writes four times in the
//!   committed corpus. Guessing `"` is wrong in every language that does not
//!   use it.
//!
//! `::first-line` and `::first-letter` still generate nothing and are still
//! counted by `Warning::PseudoElementUnsupported`. Neither is generated
//! content: both select part of an *already laid out* box, so honouring either
//! means a second layout pass. `::marker`, `::placeholder` and `::selection`
//! are not parsed at all — `::selection` for the reason row 142 already gives
//! for the seven pseudo-classes, that it names a state of a reading session
//! and a paginated document has none.
//!
//! # Generated content is not conserved text, and the harness is right
//!
//! `epub_conservation.rs` compares the page against the source markup and
//! counts anything on the page that is not in the source as **extra**.
//! Generated content is exactly that, by definition. It is not a defect in
//! either the harness or this feature, and
//! [`generated_content_is_extra_and_the_harness_says_so`] pins it — because
//! the alternative is the first book that writes a `::before` moving a ratchet
//! nobody expected to move.
//!
//! **No committed book uses a pseudo-element**, so no recorded figure moves
//! today. That is the same shape as the WOFF caveat and is stated in
//! `docs/features/epub.md` rather than left to be inferred.
//!
//! # Counted injections
//!
//! **Seven defects, reintroduced one at a time and measured with
//! `--no-fail-fast` across this file and `tinker-pdf-css`'s `pseudo.rs` and
//! `selector.rs` together** — because a defect on this seam breaks tests on
//! both sides of it, and counting one side would understate it. Every number
//! was measured; not one is the number that was expected before it was run.
//!
//! | injected | tests fired |
//! | --- | --- |
//! | `::before` inserted after the children instead of before | 3 |
//! | `::after` inserted before the children instead of after | 2 |
//! | the box inheriting from the parent rather than the originating element | 2 |
//! | a box generated even when `content` is absent | 1 |
//! | a box generated when `content: none` | 1 |
//! | `matches` letting a pseudo-element selector style its own element | 4 |
//! | `attr()` on a missing attribute generating no box rather than an empty one | 1 |
//!
//! **None fires zero.** Three fire once, and the reason differs between them
//! in a way worth writing down rather than averaging away:
//!
//! * The two `content` injections both land on
//!   `tinker-pdf-css`'s `a_box_exists_exactly_when_content_does`, which
//!   is one table-driven test asserting four cases. One test, four assertions,
//!   and the count is tests — so *one* here is not thinner evidence than
//!   *four* elsewhere, it is the same evidence reported by a different unit.
//! * The `attr()` injection fires once because exactly one fixture asks for an
//!   attribute the element does not carry. That is the whole of §2.4's
//!   empty-string rule, and it has one fixture because it is one sentence.
//!
//! Two tests here fire under **no** injection at all, and that is deliberate:
//! [`every_shape_here_renders`] is ruling 1's sweep, and
//! [`generated_content_is_extra_and_the_harness_says_so`] pins an *interaction*
//! rather than discriminating an answer. Neither is evidence about this
//! feature's correctness and neither is counted as if it were.

mod epub_support;

use epub_support::{ocf_zip, OcfEntry};
use tinker_pdf::{Document, RenderOptions};

// ---- fixtures ---------------------------------------------------------------

const CONTAINER_XML: &str = concat!(
    r#"<?xml version="1.0" encoding="UTF-8"?>"#,
    r#"<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">"#,
    r#"<rootfiles><rootfile full-path="EPUB/content.opf" media-type="application/oebps-package+xml"/>"#,
    r#"</rootfiles></container>"#
);

const PACKAGE: &str = concat!(
    r#"<?xml version="1.0" encoding="utf-8"?>"#,
    r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">"#,
    r#"<metadata xmlns:dc="http://purl.org/dc/elements/1.1/">"#,
    r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-00000000000b</dc:identifier>"#,
    r#"<dc:title>A Book With Generated Content</dc:title>"#,
    r#"<dc:language>en</dc:language>"#,
    r#"<dc:creator>The tinker-pdf authors</dc:creator>"#,
    r#"</metadata><manifest>"#,
    r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
);

/// A one-paragraph book, set by `style`, whose paragraph markup is `body`.
fn book(style: &str, body: &str) -> Vec<u8> {
    let chapter = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>A Chapter</title>"#,
            r#"<style>{}</style></head><body>{}</body></html>"#
        ),
        style, body
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

/// The first page's text, with the line breaks removed.
///
/// Where the lines fell is a different claim, and one
/// [`a_block_generated_box_takes_its_own_line`] makes deliberately.
fn text_of(style: &str, body: &str) -> String {
    let doc = Document::open(book(style, body)).expect("a book");
    doc.page(0)
        .expect("a page")
        .text()
        .plain_text()
        .replace('\n', "")
}

/// The same, keeping the lines.
fn lines_of(style: &str, body: &str) -> Vec<String> {
    let doc = Document::open(book(style, body)).expect("a book");
    doc.page(0)
        .expect("a page")
        .text()
        .plain_text()
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

// ---- the box reaches the page ------------------------------------------------

/// **The claim the row closed on: a `::before` puts text on the page.**
///
/// Asserted on the extracted text and not on the absence of a warning. A book
/// that generated nothing reports nothing either, and that is exactly the state
/// this replaces.
#[test]
fn a_before_and_an_after_reach_the_page_in_order() {
    let text = text_of(
        "p::before { content: \"[\" } p::after { content: \"]\" }",
        "<p>middle</p>",
    );
    assert_eq!(
        text, "[middle]",
        "the generated boxes are not in document order around the element's own text"
    );
}

/// §12.1 puts the generated box **inside** the originating element, as its
/// first and last child.
///
/// The distinguishing fixture is a sibling: if `::before` were inserted beside
/// the `<p>` rather than inside it, its text would land outside the paragraph
/// and this ordering would still read the same — so the assertion is made
/// against a second paragraph, where "inside" and "beside" give different
/// orders.
#[test]
fn a_generated_box_is_inside_its_originating_element() {
    let text = text_of("p::after { content: \"!\" }", "<p>one</p><p>two</p>");
    assert_eq!(
        text, "one!two!",
        "each `::after` belongs to its own paragraph"
    );
}

/// `display` on the generated box is the box's own, so a `block` one takes a
/// line and the default `inline` one does not.
#[test]
fn a_block_generated_box_takes_its_own_line() {
    let inline = lines_of("p::before { content: \"tag\" }", "<p>body</p>");
    assert_eq!(inline, vec!["tagbody"], "an inline box shares the line");

    let block = lines_of(
        "p::before { content: \"tag\"; display: block }",
        "<p>body</p>",
    );
    assert_eq!(block, vec!["tag", "body"], "a block box takes its own");
}

/// `attr()` reads the originating element, on the page and not only in the
/// cascade.
#[test]
fn attr_reaches_the_page() {
    let text = text_of(
        "p::before { content: attr(data-label) \": \" }",
        r#"<p data-label="Note">body</p>"#,
    );
    assert_eq!(text, "Note: body");
}

/// No `content` is no box, all the way to the page.
#[test]
fn a_styled_but_unfilled_pseudo_element_puts_nothing_on_the_page() {
    let text = text_of("p::before { color: #ff0000 }", "<p>body</p>");
    assert_eq!(text, "body");

    let none = text_of("p::before { content: none }", "<p>body</p>");
    assert_eq!(none, "body");
}

/// The originating element is not styled by the rule — the failure the old
/// behaviour existed to prevent, asserted where it would show: on the page.
#[test]
fn the_paragraph_is_not_styled_by_its_own_pseudo_element_rule() {
    // `display: none` on the generated box must not take the paragraph away.
    let text = text_of("p::before { content: \"x\"; display: none }", "<p>body</p>");
    assert_eq!(
        text, "body",
        "the paragraph inherited its generated box's `display: none`"
    );
}

// ---- what is refused, by name ------------------------------------------------

/// The three families of `content` value this build does not read, each
/// counted as a gap in `content` rather than silently producing an empty box.
#[test]
fn the_three_refused_content_families_are_named() {
    use tinker_pdf::ArchiveWarning;
    let mut refused = 0usize;
    for value in [
        "url(a.png)",
        "counter(chapter)",
        "counters(section, \".\")",
        "open-quote",
        "close-quote",
        "no-open-quote",
        "no-close-quote",
    ] {
        let source = format!("p::before {{ content: {value} }}");
        let doc = Document::open(book(&source, "<p>body</p>")).expect("a book");
        let warnings = doc.archive().expect("a report").warnings().to_vec();
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                ArchiveWarning::UnimplementedProperty { property, .. } if *property == "content"
            )),
            "{value} was not counted as a gap in `content`: {warnings:?}"
        );
        // And nothing reached the page.
        let text = doc
            .page(0)
            .expect("a page")
            .text()
            .plain_text()
            .replace('\n', "");
        assert_eq!(text, "body", "{value} put something on the page");
        refused += 1;
    }
    assert_eq!(refused, 7, "three families, seven spellings");
}

/// `::first-line` and `::first-letter` still generate nothing.
///
/// That they are still **named** is asserted in
/// `tinker-pdf-css`'s `tests/selector.rs`, where the warning lives: a
/// `Warning::PseudoElementUnsupported` is a stylesheet's report and does not
/// travel out through `ArchiveWarning`, so this side can only assert the
/// behaviour. Both halves exist and neither pretends to be the other.
#[test]
fn the_two_remaining_pseudo_elements_generate_nothing() {
    for selector in ["p::first-line", "p::first-letter"] {
        let source = format!("{selector} {{ content: \"x\"; color: #ff0000 }}");
        assert_eq!(
            text_of(&source, "<p>body</p>"),
            "body",
            "{selector} put something on the page"
        );
    }
}

// ---- the conservation interaction --------------------------------------------

/// **Generated content is text the source document does not contain, and the
/// conservation harness reports it as `extra`.**
///
/// Pinned rather than worked around. The harness compares the page against the
/// source markup; generated content is on one side and not the other, and
/// there is no reading of "conserved" under which it should be. What would be
/// wrong is for a future book with a `::before` to move `CONSERVATION.tsv`
/// with nobody expecting it, which is what this test exists to stop.
///
/// No committed book generates any, so no recorded figure moves today.
#[test]
fn generated_content_is_extra_and_the_harness_says_so() {
    use epub_support::conservation::{compare, paginated_text, spine_text};

    let control = book("p { color: #000000 }", "<p>body</p>");
    let generated = book("p::before { content: \"XX\" }", "<p>body</p>");

    let control_doc = Document::open(control.clone()).expect("a book");
    let verdict = compare(&spine_text(&control), &paginated_text(&control_doc));
    assert!(
        verdict.holds(),
        "the control book does not conserve, so the comparison below says nothing: {verdict:?}"
    );

    let generated_doc = Document::open(generated.clone()).expect("a book");
    let verdict = compare(&spine_text(&generated), &paginated_text(&generated_doc));
    assert_eq!(
        verdict.extra, 2,
        "the two generated characters were not reported as extra: {verdict:?}"
    );
    assert_eq!(verdict.missing, 0, "nothing from the source was lost");
    assert!(
        !verdict.holds(),
        "conservation cannot hold over text the source does not have"
    );
}

/// And the render path does not panic on any of it (ruling 1).
#[test]
fn every_shape_here_renders() {
    for (style, body) in [
        ("p::before { content: \"x\" }", "<p>body</p>"),
        ("p::after { content: attr(id) }", "<p id=\"a\">body</p>"),
        ("p::before { content: \"\" }", "<p>body</p>"),
        ("p::before { content: attr(nope) }", "<p>body</p>"),
        (
            "p::before { content: \"x\"; display: block; margin: 4px }",
            "<p>body</p>",
        ),
        ("* ::before { content: \"z\" }", "<p>body</p>"),
    ] {
        let doc = Document::open(book(style, body)).expect("a book");
        for index in 0..doc.page_count() {
            let page = doc.page(index).expect("a page");
            let _ = page.text().plain_text();
            let _ = page.render(&RenderOptions::default());
        }
    }
}
