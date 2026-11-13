//! Size-scaling complexity tests for the XML reader and the HTML tree builder
//! (`docs/design/resource-exhaustion.md`, "The size-scaling law").
//!
//! Each shape is markup this file writes at a size n and at four times n,
//! read through a door that counts: [`Reader::steps`], the XML reader's
//! events, the bytes it passed and read again, the attribute names it checked
//! and the namespace bindings it declared, looked up and unbound; and
//! [`html::parse_counted`], the tree builder's own step count — the one its
//! unit tests hold to the input's length. The law is the design's: the work
//! at 4 n is at most 1.1 × 4 times the work at n. A quadratic shape costs
//! sixteen times as much and an n¹·⁵ one eight.
//!
//! Every shape is held to what it is as well as to what it costs: each read
//! gives back what was written, and the counted door gives the answer the
//! uncounted one gives. Every size keeps four times n inside the caps on the
//! shape's path — the depth of 256, the 256 attributes of an element, the
//! token total — so no shape measures a cap. No clock is read here.

use tinker_pdf_xml::html::{self, Namespace, NodeData};
use tinker_pdf_xml::{Event, Limits, Reader, Source};

/// Four times the size: the factor the law is stated for.
const K: u64 = 4;

/// The law for a shape whose parser reads each element a bounded number of
/// times, with both figures in the failure.
fn holds_the_law(shape: &str, n: u64, kn: u64) {
    println!("  {shape:40} work at n {n:>8}  at 4n {kn:>8}");
    assert!(n > 0, "{shape}: nothing was counted at n");
    assert!(
        kn * 10 <= n * K * 11,
        "{shape}: {kn} steps at 4n against {n} at n, past 1.1 x {K}"
    );
}

// ---------------------------------------------------------------------------
// The XML reader.
// ---------------------------------------------------------------------------

/// Every event of `markup`, owned, and the steps the reader took.
fn read(markup: &str) -> (Vec<Owned>, u64) {
    let source = Source::new(markup.as_bytes()).expect("the markup decodes");
    let mut reader: Reader<'_> = source.reader(&Limits::DEFAULT);
    let mut events = Vec::new();
    for event in reader.by_ref() {
        events.push(Owned::from(event.expect("the markup is well formed")));
    }
    let steps = reader.steps();
    assert!(
        steps >= u64::try_from(reader.events()).unwrap_or(u64::MAX),
        "the steps include every event"
    );
    (events, steps)
}

/// One event, with what a shape checks of it kept past the source.
#[derive(Debug, PartialEq, Eq)]
enum Owned {
    Start {
        local: String,
        namespace: Option<String>,
        attributes: Vec<(String, Option<String>, String)>,
    },
    End {
        local: String,
        namespace: Option<String>,
    },
    Text(String),
    Other,
}

impl From<Event<'_>> for Owned {
    fn from(event: Event<'_>) -> Self {
        match event {
            Event::Start(element) => Owned::Start {
                local: element.local().to_owned(),
                namespace: element.namespace().map(str::to_owned),
                attributes: element
                    .attributes()
                    .iter()
                    .map(|a| {
                        (
                            a.local().to_owned(),
                            a.namespace().map(str::to_owned),
                            a.value().to_owned(),
                        )
                    })
                    .collect(),
            },
            Event::End(name) => Owned::End {
                local: name.local().to_owned(),
                namespace: name.namespace().map(str::to_owned),
            },
            Event::Text(text) => Owned::Text(text.into_owned()),
            _ => Owned::Other,
        }
    }
}

/// Elements nested `n` deep inside a root, each closed in turn.
fn nested(n: usize) -> u64 {
    let markup = format!("<r>{}{}</r>", "<a>".repeat(n), "</a>".repeat(n));
    let (events, steps) = read(&markup);
    assert_eq!(
        events.len(),
        2 * n + 2,
        "a start and an end for every element"
    );
    let starts = events
        .iter()
        .filter(|e| matches!(e, Owned::Start { local, .. } if local == "a"))
        .count();
    assert_eq!(starts, n, "every nested element is read");
    steps
}

#[test]
fn elements_nested_deeper_cost_work_linear_in_the_depth() {
    // Inside the depth of 256 at both sizes, with the root.
    holds_the_law("XML nested n deep", nested(50), nested(200));
}

/// One element of `n` attributes, each value holding a reference.
fn many_attributes(n: usize) -> u64 {
    let attributes: String = (0..n).map(|i| format!(" a{i}=\"v{i}&amp;\"")).collect();
    let markup = format!("<r{attributes}/>");
    let (events, steps) = read(&markup);
    let Some(Owned::Start { attributes, .. }) = events.first() else {
        panic!("the element starts");
    };
    assert_eq!(attributes.len(), n, "every attribute is read");
    for i in [0, n - 1] {
        assert_eq!(
            attributes[i],
            (format!("a{i}"), None, format!("v{i}&")),
            "attribute {i} reads back, its reference resolved"
        );
    }
    steps
}

#[test]
fn an_element_of_more_attributes_costs_work_linear_in_its_attributes() {
    // Inside the 256 attributes of one element at both sizes. Each name is
    // checked against the others once by its spelling and once by its
    // expanded name: a comparison of every pair was the square of the count.
    holds_the_law(
        "XML element of n attributes",
        many_attributes(50),
        many_attributes(200),
    );
}

/// One element declaring `n` prefixes, with an attribute in each, named in
/// the last.
fn declarations_on_one_element(n: usize) -> u64 {
    let declarations: String = (0..n)
        .map(|i| format!(" xmlns:p{i}=\"urn:x:{i}\""))
        .collect();
    let attributes: String = (0..n).map(|i| format!(" p{i}:a=\"{i}\"")).collect();
    let last = n - 1;
    let markup = format!("<p{last}:r{declarations}{attributes}/>");
    let (events, steps) = read(&markup);
    let Some(Owned::Start {
        local,
        namespace,
        attributes,
    }) = events.first()
    else {
        panic!("the element starts");
    };
    assert_eq!(local, "r");
    assert_eq!(namespace.as_deref(), Some(format!("urn:x:{last}").as_str()));
    assert_eq!(
        attributes.len(),
        n,
        "the declarations are consumed, every attribute is kept"
    );
    for i in [0, n - 1] {
        assert_eq!(
            attributes[i],
            ("a".to_owned(), Some(format!("urn:x:{i}")), i.to_string()),
            "attribute {i} is in the namespace its prefix was bound to"
        );
    }
    steps
}

#[test]
fn an_element_declaring_more_prefixes_costs_work_linear_in_its_declarations() {
    // `2 n` items on one element, inside its 256 at both sizes. Every prefix
    // is looked up in the scope once for its attribute: a search of every
    // binding in scope from the innermost was the square of the count.
    holds_the_law(
        "XML n declarations on one element",
        declarations_on_one_element(25),
        declarations_on_one_element(100),
    );
}

/// A root binding the default namespace and one prefix, and `n` elements
/// nested inside it, each declaring a prefix of its own and carrying an
/// attribute in the root's.
fn declarations_nested(n: usize) -> u64 {
    let mut markup = String::from("<r xmlns=\"urn:root\" xmlns:p=\"urn:p\">");
    for i in 0..n {
        markup.push_str(&format!("<e xmlns:q{i}=\"urn:q:{i}\" p:a=\"{i}\">"));
    }
    markup.push_str(&"</e>".repeat(n));
    markup.push_str("</r>");
    let (events, steps) = read(&markup);
    assert_eq!(events.len(), 2 * n + 2);
    let deepest = events.get(n).expect("the deepest element starts");
    assert_eq!(
        deepest,
        &Owned::Start {
            local: "e".to_owned(),
            namespace: Some("urn:root".to_owned()),
            attributes: vec![(
                "a".to_owned(),
                Some("urn:p".to_owned()),
                (n - 1).to_string()
            )],
        },
        "under every inner declaration, the root's two bindings still resolve"
    );
    assert_eq!(
        events.last(),
        Some(&Owned::End {
            local: "r".to_owned(),
            namespace: Some("urn:root".to_owned()),
        })
    );
    steps
}

#[test]
fn more_declarations_in_scope_cost_each_name_no_more() {
    // Inside the depth of 256 at both sizes. Every unprefixed name looks up
    // the default namespace and every attribute the root's prefix, under one
    // more binding at each level: a search from the innermost binding was the
    // square of the depth.
    holds_the_law(
        "XML n declarations nested",
        declarations_nested(50),
        declarations_nested(200),
    );
}

/// Character data of `n` references, numeric in both radixes and predefined.
fn references(n: usize) -> u64 {
    let markup = format!("<r>{}</r>", "&#x41;&#66;&amp;".repeat(n));
    let (events, steps) = read(&markup);
    let Some(Owned::Text(text)) = events.get(1) else {
        panic!("one run of text");
    };
    assert_eq!(text.len(), 3 * n, "every reference is one character");
    assert!(
        text.starts_with("AB&AB&"),
        "the references resolve: {text:.12}"
    );
    steps
}

#[test]
fn more_character_references_cost_work_linear_in_the_references() {
    holds_the_law(
        "XML n character references",
        references(1000),
        references(4000),
    );
}

// ---------------------------------------------------------------------------
// The HTML tree builder.
// ---------------------------------------------------------------------------

/// The document `markup` parses to and the tree builder's steps, after
/// checking that the parse ran to its end and that the uncounted door builds
/// the same tree.
fn build(markup: &str) -> (html::Document, u64) {
    let (document, steps) = html::parse_counted(markup, &Limits::DEFAULT);
    assert_eq!(document.stopped(), None, "no cap stops the shape");
    let plain = html::parse(markup, &Limits::DEFAULT);
    assert_eq!(
        plain.nodes(),
        document.nodes(),
        "the counted door builds what the uncounted one builds"
    );
    (document, steps)
}

/// The elements named `name`, in document order.
fn named<'d>(document: &'d html::Document, name: &str) -> Vec<&'d html::Node> {
    document
        .nodes()
        .iter()
        .filter(|node| {
            node.element()
                .is_some_and(|e| e.name == name && e.namespace == Namespace::Html)
        })
        .collect()
}

/// The text of a node's children, concatenated.
fn text_of(document: &html::Document, node: &html::Node) -> String {
    node.children
        .iter()
        .filter_map(|&child| match document.node(child).map(|n| &n.data) {
            Some(NodeData::Text(text)) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// A paragraph leaving three formatting elements open, and `n` paragraphs
/// after it, each of which reopens all three before its text.
fn reopened(n: usize) -> u64 {
    let markup = format!(
        "<!DOCTYPE html><p><b class=a><i class=b><u class=c>x</p>{}",
        "<p>y</p>".repeat(n)
    );
    let (document, steps) = build(&markup);
    let paragraphs = named(&document, "p");
    assert_eq!(paragraphs.len(), n + 1, "every paragraph is built");
    // Each later paragraph holds a clone of `b`, in it `i`, in it `u`, and in
    // that the paragraph's text.
    for &paragraph in paragraphs.iter().skip(1) {
        let mut node = paragraph;
        for (name, class) in [("b", "a"), ("i", "b"), ("u", "c")] {
            let child = node
                .children
                .first()
                .and_then(|&c| document.node(c))
                .expect("a reopened element");
            let element = child.element().expect("an element");
            assert_eq!(element.name, name, "reopened in the order they were open");
            assert_eq!(
                element.attribute("class"),
                Some(class),
                "with its attributes"
            );
            node = child;
        }
        assert_eq!(text_of(&document, node), "y");
    }
    assert_eq!(named(&document, "b").len(), n + 1, "one clone a paragraph");
    steps
}

#[test]
fn reopening_formatting_elements_costs_work_linear_in_the_paragraphs() {
    holds_the_law(
        "HTML n formatting elements reopened",
        reopened(1000),
        reopened(4000),
    );
}

/// A table with `n` elements and their text fostered out of it.
fn fostered_elements(n: usize) -> u64 {
    let markup = format!(
        "<!DOCTYPE html><table>{}</table>",
        "<span>x</span>".repeat(n)
    );
    let (document, steps) = build(&markup);
    let body = named(&document, "body");
    let body = body.first().expect("a body");
    assert_eq!(
        body.children.len(),
        n + 1,
        "every span stands in front of the table"
    );
    let last = body
        .children
        .last()
        .and_then(|&c| document.node(c))
        .and_then(html::Node::element)
        .expect("the table, last");
    assert_eq!(last.name, "table");
    for span in named(&document, "span") {
        assert_eq!(text_of(&document, span), "x", "each span keeps its text");
    }
    steps
}

/// A table with `n` runs of text fostered out of it, each before a
/// formatting element fostered out too.
fn fostered_text(n: usize) -> u64 {
    let markup = format!("<!DOCTYPE html><table>{}</table>", "x<i></i>".repeat(n));
    let (document, steps) = build(&markup);
    let body = named(&document, "body");
    let body = body.first().expect("a body");
    assert_eq!(
        body.children.len(),
        2 * n + 1,
        "every run and every element stands in front of the table"
    );
    assert_eq!(text_of(&document, body), "x".repeat(n));
    steps
}

#[test]
fn fostering_more_out_of_a_table_costs_work_linear_in_what_is_fostered() {
    holds_the_law(
        "HTML n elements fostered",
        fostered_elements(1000),
        fostered_elements(4000),
    );
    holds_the_law(
        "HTML n text runs fostered",
        fostered_text(1000),
        fostered_text(4000),
    );
}

/// `n` tables nested a cell inside another, each closed in turn.
fn nested_tables(n: usize) -> u64 {
    let markup = format!(
        "<!DOCTYPE html>{}{}",
        "<table><tr><td>".repeat(n),
        "</td></tr></table>".repeat(n)
    );
    let (document, steps) = build(&markup);
    assert_eq!(named(&document, "table").len(), n, "every table is built");
    assert_eq!(
        named(&document, "tbody").len(),
        n,
        "each with the body its row implies"
    );
    let cells = named(&document, "td");
    assert_eq!(cells.len(), n);
    for (depth, cell) in cells.iter().enumerate() {
        let tables_inside = cell
            .children
            .iter()
            .filter(|&&c| {
                document
                    .node(c)
                    .and_then(html::Node::element)
                    .is_some_and(|e| e.name == "table")
            })
            .count();
        let expected = usize::from(depth + 1 < n);
        assert_eq!(tables_inside, expected, "cell {depth} holds the next table");
    }
    steps
}

#[test]
fn tables_nested_deeper_cost_work_linear_in_the_depth() {
    // Four open elements a table — table, tbody, tr, td — so 48 deep is 192
    // open elements, inside the depth of 256 with the root and the body.
    holds_the_law("HTML n nested tables", nested_tables(12), nested_tables(48));
}

#[test]
fn the_counted_doors_count_and_agree_with_the_uncounted_ones() {
    let markup = "<!DOCTYPE html><p><b>x</p><p>y</p>";
    let (document, steps) = html::parse_counted(markup, &Limits::DEFAULT);
    assert!(steps > 0, "a reopened element is counted");
    assert_eq!(
        document.nodes(),
        html::parse(markup, &Limits::DEFAULT).nodes()
    );

    let (fragment, steps) =
        html::parse_fragment_counted("<b>x</b>", (Namespace::Html, "div"), &Limits::DEFAULT);
    assert!(steps > 0, "the adoption agency's search is counted");
    assert_eq!(
        fragment.nodes(),
        html::parse_fragment("<b>x</b>", (Namespace::Html, "div"), &Limits::DEFAULT).nodes()
    );

    // A `<meta>` past the prescan's kilobyte changes the encoding, and both
    // readings are counted.
    let padding = "<!-- ".to_owned() + &"-".repeat(1100) + " -->";
    let bytes =
        format!("{padding}<meta charset=windows-1252><p>\u{e9}<b>x</p><p>y</p>").into_bytes();
    let (document, twice) = html::parse_bytes_counted(&bytes, &Limits::DEFAULT);
    let (_, once) = html::parse_counted(&String::from_utf8_lossy(&bytes), &Limits::DEFAULT);
    assert!(
        twice > once,
        "the second reading's steps are added: {twice} against {once}"
    );
    assert_eq!(
        document.nodes(),
        html::parse_bytes(&bytes, &Limits::DEFAULT).nodes()
    );
    assert_eq!(
        document.encoding().map(|d| d.confident),
        Some(true),
        "the second reading is certain"
    );
}

#[test]
fn the_xml_steps_only_add_and_stop_with_a_refusal() {
    let source = Source::new(b"<r a=\"1\" a=\"2\"/>").expect("decodes");
    let mut reader = source.reader(&Limits::DEFAULT);
    let first = reader.next();
    assert!(
        matches!(first, Some(Err(_))),
        "a repeated attribute is refused"
    );
    let at_refusal = reader.steps();
    assert!(
        at_refusal > 0,
        "the names checked before the refusal are counted"
    );
    assert!(
        reader.next().is_none(),
        "the reader is fused by the refusal"
    );
    assert_eq!(reader.steps(), at_refusal, "and counts nothing after it");

    // The steps of a document read whole never fall as it is read.
    let source = Source::new(b"<r xmlns:p=\"u\"><p:a>t&amp;</p:a><!-- c --></r>").expect("decodes");
    let mut reader = source.reader(&Limits::DEFAULT);
    let mut last = reader.steps();
    while let Some(event) = reader.next() {
        let _ = event.expect("well formed");
        let now = reader.steps();
        assert!(now > last, "every event costs a step at least");
        last = now;
    }
}
