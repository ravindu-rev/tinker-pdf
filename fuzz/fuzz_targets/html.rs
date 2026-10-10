//! HTML's tokenizer and tree builder (`tinker_pdf_xml::html`, tier 5's
//! formats row): bytes in, through §13.2.3's decoding, a document tree out.
//!
//! The input is markup and nothing frames it, so what a mutator finds is the
//! standard's seams: a tag cut in its attribute list, a character reference
//! with no semicolon in an attribute, a `</script>` inside an escaped comment,
//! a table cell outside a row with formatting elements open across it — the
//! foster-parenting and adoption-agency paths, which move nodes between
//! parents and are where an arena of indices can go wrong.
//!
//! # What this target checks
//!
//! **That the parser never panics, hangs or exhausts memory** (ruling 1), and
//! beyond that:
//!
//! - **The tree is a tree.** Every node is reached from the document at most
//!   once, every child names its parent back, and nothing reachable is its
//!   own ancestor — the adoption agency detaches and reattaches, and a node
//!   left in two parents' lists would be drawn twice.
//! - **The token cap is the total.** No more nodes than `max_tokens` allows
//!   (plus the document and the handful one token may make after the cap
//!   is crossed), whatever the tree
//!   builder created on its own account.
//! - **The attribute cap is per element.** No element carries more than
//!   `max_attributes`, including the `<html>` and `<body>` that later tags'
//!   attributes are merged into — which is how the review of the formats
//!   lane found one element holding a hundred thousand.
//! - **The tree is no bigger than its input and the clone cap.** The bytes
//!   of every attribute in the tree are at most the input's length plus
//!   `MAX_HTML_CLONE_BYTES`, since a clone of a formatting element is the
//!   only node that copies what the input said once.
//! - **Parsing is deterministic.**
//! - **The facade's conversion keeps the XML reader's depth bound**: the
//!   EPUB tree built from any HTML document is no deeper than `max_depth`,
//!   and its parents precede their children, which the cascade requires.
//!
//! The control byte picks the four [`Limits`], two bits each, so every cap is
//! crossable inside one iteration; the body's length picks a fragment context
//! for a second parse.
//!
//! # What this target cannot find, and what covers it instead
//!
//! Whether the tree is the one the standard builds. That is
//! `crates/tinker-pdf-xml/tests/html5lib.rs`, which holds the parser to
//! html5lib's tree-construction suite on every `cargo test`.
#![no_main]
use libfuzzer_sys::fuzz_target;

use tinker_pdf_xml::html::{self, Document, Namespace};
use tinker_pdf_xml::limits::MAX_HTML_CLONE_BYTES;
use tinker_pdf_xml::Limits;

const CONTEXTS: [(Namespace, &str); 8] = [
    (Namespace::Html, "body"),
    (Namespace::Html, "td"),
    (Namespace::Html, "table"),
    (Namespace::Html, "template"),
    (Namespace::Html, "select"),
    (Namespace::Html, "title"),
    (Namespace::Svg, "svg"),
    (Namespace::MathMl, "math"),
];

fn check_tree(document: &Document, limits: &Limits, extra: usize, input: usize) {
    let nodes = document.nodes();
    assert!(
        nodes.len() <= limits.max_tokens + extra,
        "{} nodes past a cap of {}",
        nodes.len(),
        limits.max_tokens
    );
    let mut attribute_bytes = 0usize;
    for element in nodes.iter().filter_map(|n| n.element()) {
        assert!(
            element.attributes.len() <= limits.max_attributes,
            "<{}> carries {} attributes past a cap of {}",
            element.name,
            element.attributes.len(),
            limits.max_attributes
        );
        for attribute in &element.attributes {
            attribute_bytes += attribute.name.len() + attribute.value.len();
        }
    }
    // A decoded byte may be three UTF-8 bytes (windows-1252's upper half),
    // and a fragment's context element is one the input did not write.
    assert!(
        attribute_bytes <= input * 3 + MAX_HTML_CLONE_BYTES,
        "{attribute_bytes} bytes of attributes from {input} bytes of input"
    );
    let mut seen = vec![false; nodes.len()];
    let mut stack = vec![0usize];
    while let Some(at) = stack.pop() {
        assert!(!seen[at], "node {at} is reached twice");
        seen[at] = true;
        let node = &nodes[at];
        for &child in &node.children {
            assert!(child < nodes.len(), "a child index past the arena");
            assert_eq!(nodes[child].parent, Some(at), "a child names another parent");
            stack.push(child);
        }
        if let Some(contents) = node.element().and_then(|e| e.template_contents) {
            assert!(contents < nodes.len());
            assert_eq!(nodes[contents].parent, None, "a template's content has a parent");
            stack.push(contents);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let (control, body) = data.split_at(data.len().min(1));
    let knobs = control.first().copied().unwrap_or(0);
    let limits = Limits {
        max_depth: [4, 16, 64, 256][usize::from(knobs & 3)],
        max_attributes: [0, 2, 16, 256][usize::from((knobs >> 2) & 3)],
        max_name_len: [1, 8, 64, 1024][usize::from((knobs >> 4) & 3)],
        max_tokens: [16, 256, 4096, 65_536][usize::from((knobs >> 6) & 3)],
    };

    let document = html::parse_bytes(body, &limits);
    check_tree(&document, &limits, 16, body.len());
    let again = html::parse_bytes(body, &limits);
    assert!(
        document.nodes() == again.nodes(),
        "parsing is not deterministic"
    );

    let dom = tinker_pdf::epub::xhtml::from_html(&document, &limits);
    for (index, node) in dom.nodes.iter().enumerate() {
        if let Some(parent) = node.parent {
            assert!(parent < index, "a parent after its child");
        }
        let mut depth = 1;
        let mut cursor = node.parent;
        while let Some(up) = cursor {
            depth += 1;
            cursor = dom.nodes[up].parent;
        }
        assert!(
            depth <= limits.max_depth,
            "an element {depth} deep past a cap of {}",
            limits.max_depth
        );
    }

    let (text, _) = html::decode(body);
    let context = CONTEXTS[body.len() % CONTEXTS.len()];
    let fragment = html::parse_fragment(&text, context, &limits);
    check_tree(&fragment, &limits, 16, body.len());
});
