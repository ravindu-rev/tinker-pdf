//! HTML tag soup, generated: one element tree written as markup the tree
//! builder has to repair, built into a tree that must be one.
//!
//! `html` hands mutated bytes to the parser, which reaches the tokenizer's
//! seams; the tree builder's insertion modes are reached as far as a mutation
//! happens to spell a table or a template. This target reads its input as
//! choices (`tinker_pdf_fuzz::Choices`) and writes the tree
//! `tinker_pdf_fuzz::markup` generates as soup: tables and their parts with
//! text and elements written straight inside them (foster parenting),
//! formatting elements closed out of order (the adoption agency), `template`,
//! `select` and `option`, `svg` and `math` with CDATA, raw-text elements with
//! markup inside, end tags left out, stray end tags, attributes quoted three
//! ways and duplicated, references with and without their semicolons and
//! character references to nothing, and no declaration, a standards-mode one
//! or a quirks one — parsed under the four bounds the input chooses.
//!
//! What is asserted beyond "it did not panic":
//!
//! - **The tree is a tree.** Every node is reached from the document once,
//!   every child names its parent back, and a template's content is
//!   parentless and reached through its template alone.
//! - **It is built inside `Limits::max_tokens`**, which counts every node the
//!   builder makes, the elements it creates on its own account included
//!   (plus the document and the few one token may make past the cap), and no
//!   element carries more than `max_attributes`.
//! - **Parsing is deterministic.**
//!
//! `MAX_DOM_NODES` is the cascade's bound and binds a tree a stylesheet is
//! cascaded over, which this one is not.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Whether the tree is the one the standard builds from the soup. That is
//! `crates/tinker-pdf-xml/tests/html5lib.rs`, which holds the tree builder to
//! html5lib's tree-construction suite on every `cargo test`.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_fuzz::markup;
use tinker_pdf_fuzz::Choices;
use tinker_pdf_xml::html::{self, Document};
use tinker_pdf_xml::Limits;

/// The nodes past the cap one token may make: the document, and what the
/// token that crossed it creates.
const PAST_THE_CAP: usize = 16;

fn check_tree(document: &Document, limits: &Limits) {
    let nodes = document.nodes();
    assert!(
        nodes.len() <= limits.max_tokens + PAST_THE_CAP,
        "{} nodes past a cap of {}",
        nodes.len(),
        limits.max_tokens
    );
    for element in nodes.iter().filter_map(|n| n.element()) {
        assert!(
            element.attributes.len() <= limits.max_attributes,
            "<{}> carries {} attributes past a cap of {}",
            element.name,
            element.attributes.len(),
            limits.max_attributes
        );
    }
    let mut seen = vec![false; nodes.len()];
    let mut stack = vec![document.root()];
    while let Some(at) = stack.pop() {
        assert!(!seen[at], "node {at} is reached twice");
        seen[at] = true;
        let node = &nodes[at];
        for &child in &node.children {
            assert!(child < nodes.len(), "a child index past the arena");
            assert_eq!(
                nodes[child].parent,
                Some(at),
                "a child names another parent"
            );
            stack.push(child);
        }
        if let Some(contents) = node.element().and_then(|e| e.template_contents) {
            assert!(
                contents < nodes.len(),
                "a template's content past the arena"
            );
            assert_eq!(
                nodes[contents].parent, None,
                "a template's content has a parent"
            );
            stack.push(contents);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let doc = markup::html(&mut Choices::new(data));
    let document = html::parse_bytes(&doc.bytes, &doc.limits);
    check_tree(&document, &doc.limits);
    let again = html::parse_bytes(&doc.bytes, &doc.limits);
    assert!(
        document.nodes() == again.nodes(),
        "parsing is not deterministic"
    );
});
