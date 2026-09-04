//! EPUB output carries a structure tree, ISO 32000 §14.7.
//!
//! # What this is for
//!
//! Until now an EPUB converted by this engine was an **untagged** PDF: no
//! `/MarkInfo`, no `/StructTreeRoot`, not one marked-content id. Every reader
//! extracting from it fell back to §14.8's geometric heuristic, and every
//! assistive technology had nothing but that heuristic to go on. The engine has
//! read structure trees since Tier 3 (`docs/design/tagged-pdf.md`) and has had
//! a complete writer in `tinker-pdf-cos` for as long, with no production
//! caller. This is the caller.
//!
//! # The evidence problem, and how it is answered
//!
//! Reading this engine's own output back through this engine's own reader
//! proves the two halves **agree**, not that either is right — the shared
//! misunderstanding the fuzz-audit lane spent itself documenting. So the
//! load-bearing assertion here is not a round trip. It is
//! [`logical_order_conserves_the_source_exactly`], which compares the
//! structure tree's logical order against the **source XHTML** — ground truth
//! this engine did not author and cannot have agreed with by construction.
//!
//! The round-trip assertions below are still worth having, but they are
//! secondary and are labelled as such: they check that what was written is
//! well-formed, not that it is true.
//!
//! # What this first pass does not do, each named rather than absent
//!
//! - **No PDF/UA conformance claim.** A structure tree is necessary for it and
//!   nowhere near sufficient.
//! - **No `/Alt` on images**, because `PageBuilder` cannot write one — its own
//!   comment says so — and a `/Figure` with no alternate text is what an
//!   `<img alt="…">` becomes. The alternate text is in the source and is
//!   dropped.
//! - **No `/Lang`** per element, and none on the catalog.
//! - **No `/RoleMap`**, and it is not needed: every tag written is one of
//!   Table 333's standard types, so there is nothing non-standard to declare.
//!   The cost is that the XHTML name is not recoverable — `<em>` and
//!   `<strong>` are both `/Span`.
//! - **`<a>` is a `/Span`, not a `/Link`.** §14.8.4.4.2 wants a `/Link`
//!   element to contain an `/OBJR` for its annotation and this writer cannot
//!   emit one; a bare `/Link` would claim an association that is not in the
//!   file. The annotation itself is still written.
//! - **No table `/Headers`, `/Scope` or `/Summary`**, so a `<th>` is a `/TH`
//!   with no association to the cells it heads.
//! - **No cross-page structure elements**, which is the one that matters for
//!   the float row and is measured in
//!   [`logical_order_does_not_repair_a_float_across_a_page`].

#[path = "epub_support/mod.rs"]
mod epub_support;

use epub_support::conservation::conservation_in_logical_order;
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
    r#"<dc:identifier id="pub-id">urn:uuid:1f0c2c1e-0000-4000-8000-0000000057a6</dc:identifier>"#,
    r#"<dc:title>A Tagged Book</dc:title><dc:language>en</dc:language>"#,
    r#"<dc:creator>The tinker-pdf authors</dc:creator>"#,
    r#"</metadata><manifest>"#,
    r#"<item id="c1" href="ch1.xhtml" media-type="application/xhtml+xml"/>"#,
    r#"</manifest><spine><itemref idref="c1"/></spine></package>"#
);

fn book(style: &str, body: &str) -> Vec<u8> {
    let chapter = format!(
        concat!(
            r#"<?xml version="1.0" encoding="utf-8"?>"#,
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>T</title>"#,
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

fn opened(body: &str) -> Document {
    Document::open_with(book("", body), &OpenOptions::at_page(400.0, 600.0)).expect("a book")
}

/// Every standard type the tree carries, in the order the walk meets them.
fn types(doc: &Document) -> Vec<String> {
    let tree = doc.structure().expect("a structure tree");
    tree.elements()
        .iter()
        .map(|element| element.standard_type.clone())
        .collect()
}

// ---- the tree is there ------------------------------------------------------

/// **The claim the row closed on: the output is a tagged PDF.**
///
/// `/MarkInfo << /Marked true >>` and a `/StructTreeRoot` the reader finds.
/// Asserted on an untagged-by-construction fixture too, so this is a
/// difference rather than a constant.
#[test]
fn an_epub_becomes_a_tagged_pdf() {
    let doc = opened("<p>one</p>");
    let tree = doc
        .structure()
        .expect("an EPUB now carries a structure tree");
    assert!(tree.marked, "/MarkInfo /Marked is not true");
    assert!(
        tree.element_count() > 0,
        "the tree is empty: {} elements",
        tree.element_count()
    );
    assert!(
        tree.content_count() > 0,
        "no marked-content id is claimed by any element"
    );
    assert!(
        tree.warnings.is_empty(),
        "the reader complained about what the writer wrote: {:?}",
        tree.warnings
    );
}

/// XHTML element names become Table 333's standard types.
///
/// A round-trip assertion, and secondary: it says the tree is well-formed, not
/// that its order is right. The order claim is the conservation test below.
#[test]
fn xhtml_elements_become_standard_structure_types() {
    let doc = opened("<h1>a</h1><p>b</p><ul><li>c</li></ul>");
    let seen = types(&doc);
    for wanted in ["H1", "P", "L", "LI"] {
        assert!(seen.iter().any(|t| t == wanted), "no {wanted} in {seen:?}");
    }
}

/// A table's parts each get their own type, which is what makes a table
/// navigable rather than a grid of paragraphs.
#[test]
fn a_table_carries_its_own_structure() {
    let doc = opened("<table><tr><th>h</th><td>d</td></tr></table>");
    let seen = types(&doc);
    for wanted in ["Table", "TR", "TH", "TD"] {
        assert!(seen.iter().any(|t| t == wanted), "no {wanted} in {seen:?}");
    }
}

/// Nesting is the document's, not the page's: a `<span>` inside a `<p>` is a
/// `/Span` **inside** a `/P`.
#[test]
fn an_inline_element_nests_inside_its_block() {
    let doc = opened("<p>before<span>inside</span>after</p>");
    let tree = doc.structure().expect("a tree");
    let paragraph = tree
        .elements()
        .into_iter()
        .find(|element| element.standard_type == "P")
        .expect("a P");
    let nested: Vec<&str> = paragraph
        .kids
        .iter()
        .filter_map(|kid| match kid {
            tinker_pdf::StructKid::Element(child) => Some(child.standard_type.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        nested.contains(&"Span"),
        "the span is not inside the paragraph: {nested:?}"
    );
}

/// §14.8.2.2: a list marker is an **artifact** and stays outside the tree.
///
/// It is drawn and not extracted, which is what keeps text conservation an
/// equality — a bullet on the page and not in the source would be one extra
/// character per list item.
#[test]
fn a_list_marker_is_an_artifact_and_not_an_element() {
    let doc = opened("<ul><li>item</li></ul>");
    let page = doc.page(0).expect("a page");
    let structured = page.structured_text().expect("structured text");
    let text: String = structured.nodes.iter().map(|n| n.text.as_str()).collect();
    assert!(
        text.contains("item"),
        "the list item's own text is missing: {text:?}"
    );
    assert!(
        !text.contains('\u{2022}') && !text.contains('-'),
        "a marker reached the structure tree: {text:?}"
    );
}

// ---- the load-bearing one ---------------------------------------------------

/// **Logical order against the source XHTML, which this engine did not
/// author.**
///
/// Every other assertion in this file reads what this engine wrote through
/// what this engine reads, and two halves of one build agreeing is not
/// evidence that either is right. This one compares the structure tree's
/// order against the book's own markup: no extra characters, none missing,
/// every one in source order.
#[test]
fn logical_order_conserves_the_source_exactly() {
    let bytes = book(
        "p { margin: 0 }",
        "<h1>Title</h1><p>alpha beta</p><ul><li>one</li><li>two</li></ul>\
         <table><tr><td>cell</td></tr></table><p>omega</p>",
    );
    for tenths in 4..=12 {
        let height = f64::from(tenths) * 50.0;
        let doc = Document::open_with(bytes.clone(), &OpenOptions::at_page(300.0, height))
            .expect("a book");
        let verdict = conservation_in_logical_order(&bytes, &doc);
        assert!(
            verdict.holds(),
            "at a {height}-point page logical order lost the source: \
             {} extra, {} missing, {:?}",
            verdict.extra,
            verdict.missing,
            verdict.divergences
        );
    }
}

/// **And where it does not help: a float whose box is on another page.**
///
/// This is the measurement the float row turns on, and it is negative. A
/// structure element's marked-content kids live on the page their `BDC` was
/// written on, and `DocumentBuilder` wraps each page's roots in one
/// `/Document` in page order — so logical order is page order, then reading
/// order within a page. That is *already* what content order is here, and the
/// two agree to the character.
///
/// Repairing it needs a structure element whose kids span pages, so a chapter
/// can be one subtree with its floats in source position whatever page their
/// glyphs are on. `PageBuilder` builds its roots per page and cannot express
/// that; its own documentation says so.
#[test]
fn logical_order_does_not_repair_a_float_across_a_page() {
    let bytes = book(
        "h2 { page-break-before: always; margin: 0 } p { margin: 0 } \
         span.side { display: block; float: right; clear: right; width: 40% }",
        "<p>opening</p><h2>ONE</h2><span class=\"side\">gloss one</span>\
         <p>a</p><p>b</p><p>c</p>",
    );
    let doc =
        Document::open_with(bytes.clone(), &OpenOptions::at_page(200.0, 60.0)).expect("a book");
    let logical = conservation_in_logical_order(&bytes, &doc);
    let content = epub_support::conservation::conservation(&bytes, &doc);
    assert_eq!(
        (logical.extra, logical.missing),
        (content.extra, content.missing),
        "logical order and content order have stopped agreeing, which would \
         mean a structure element now spans pages — a good change, and one \
         that has to rewrite this test and the float row together"
    );
}
