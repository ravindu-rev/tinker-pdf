//! An EPUB content document, read into an element tree (gap 31, milestone 8).
//!
//! This is the join between `tinker-pdf-xml`'s event stream and
//! `tinker-pdf-css`'s [`tinker_pdf_css::Element`] trait, and it is the only
//! file in the workspace that knows both that `class` is a space-separated
//! token list and that `<html>` is a document element. Ruling 8 is why it is
//! here rather than in either leaf: the CSS crate matches selectors against a
//! trait so that *no XHTML vocabulary is in its public API*, and the whole
//! value of that boundary is lost if the vocabulary leaks back across it.
//!
//! # The shape, and why it is indices
//!
//! [`tinker_pdf_css::cascade::cascade`] takes a slice in **document order**
//! with every link an index into it, and refuses a slice whose parents do not
//! precede their children. So the tree is built as a flat `Vec<Node>` in the
//! order the reader met the start tags, which is document order by
//! construction — the refusal is unreachable from this producer and the test
//! that says so is `elements_are_in_document_order`.
//!
//! # What is dropped, and what is emphatically not
//!
//! **Comments, processing instructions and the doctype are dropped**, because
//! none of them is content. **Character data is kept exactly as written**,
//! including the indentation a producer put in — `css-text-3` §4.1.1's
//! collapsing is `tinker-pdf-layout`'s and doing it here would throw away the
//! distinction between a collapsible newline and a preserved one before
//! `white-space` had been consulted. A `<![CDATA[…]]>` section is character
//! data too: it is where calibre puts a `<style>` element's body, and a build
//! that dropped it would lose a stylesheet.
//!
//! **An element outside the XHTML namespace is kept**, and that is a decision
//! rather than an omission. Two of the six committed books wrap their cover in
//! an SVG `<image>`; this build draws no SVG (a named non-goal), and the
//! elements carry no text, so keeping them costs a handful of nodes and keeps
//! the tree a faithful record of the document. What it must not do is let an
//! SVG `<title>` be matched by this build's UA rule for HTML's `<title>` — see
//! [`Node::local_name`], which reports the local name and
//! [`Node::is_html`], which is what the tree walk keys the UA vocabulary on.

use tinker_pdf_css::Element as CssElement;
use tinker_pdf_xml::{Doctype, Error as XmlError, Event, Limits as XmlLimits, Source};

/// The XHTML namespace, which is what tells an `<image>` from an `<img>`.
pub const XHTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

/// One element of a content document.
///
/// The four fields the cascade needs are precomputed rather than derived on
/// each call: `id` and `classes` are read once out of the attribute list, and
/// the sibling links are filled in as the tree is built. Selector matching asks
/// for them once per candidate rule per element, which for a real book is the
/// hot loop of the whole reader.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// The local name, without a prefix.
    pub name: String,
    /// The namespace the name resolved in, or `None` for a name in no
    /// namespace at all.
    pub namespace: Option<String>,
    /// `id`, which XHTML spells with no namespace.
    pub id: Option<String>,
    /// `class`, split on white space per HTML's token-list rules.
    pub classes: Vec<String>,
    /// Every attribute, under the name the source spelled — `epub:type` stays
    /// `epub:type`, because that is what an author writing `[epub|type]` would
    /// have to have written and this build has no namespace syntax in
    /// selectors.
    pub attributes: Vec<(String, String)>,
    /// The parent's index, always less than this node's own.
    pub parent: Option<usize>,
    /// The previous element sibling.
    pub previous: Option<usize>,
    /// The next element sibling.
    pub next: Option<usize>,
    /// Children, in document order, elements and text interleaved.
    pub children: Vec<Child>,
    /// `style=""`, unparsed. The cascade parses it, because the declarations it
    /// yields do not outlive the call that matched them.
    pub style: Option<String>,
}

/// What sits inside an element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Child {
    /// A child element, by index into [`Dom::nodes`].
    Element(usize),
    /// Character data, exactly as the source wrote it.
    Text(String),
}

impl Node {
    /// Whether this element is in the XHTML namespace.
    ///
    /// A document with no `xmlns` at all — which EPUB 2's XHTML 1.1 profile
    /// permits and one committed producer writes — has `None` here, and its
    /// elements are treated as XHTML. That is the only reading that makes
    /// sense of a document whose media type already said what it is, and the
    /// alternative would set every EPUB 2 book with no UA rules at all.
    #[must_use]
    pub fn is_html(&self) -> bool {
        match &self.namespace {
            None => true,
            Some(ns) => ns == XHTML_NAMESPACE,
        }
    }

    /// An attribute's value, by the name the source spelled.
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

impl CssElement for Node {
    fn local_name(&self) -> &str {
        &self.name
    }

    fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    fn classes(&self) -> &[String] {
        &self.classes
    }

    fn attribute(&self, name: &str) -> Option<&str> {
        self.attr(name)
    }

    fn parent(&self) -> Option<usize> {
        self.parent
    }

    fn previous_sibling(&self) -> Option<usize> {
        self.previous
    }

    fn next_sibling(&self) -> Option<usize> {
        self.next
    }

    fn inline_style(&self) -> Option<&str> {
        self.style.as_deref()
    }
}

/// What could not be read about a content document.
///
/// Separate from [`crate::epub::SpineDefect`] because these are recoverable:
/// a document that hit one of them still produces the pages its text needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MarkupDefect {
    /// The reader stopped before the end of the document: a well-formedness
    /// error, a cap, or an encoding this build does not decode.
    ///
    /// **The tree built so far is kept**, which is ruling 2 rather than
    /// laziness: a book whose last chapter has one unescaped `&` in its last
    /// paragraph should lose that paragraph and not the chapter.
    Truncated,
    /// The document has no element at all.
    Empty,
}

/// EPUB 3.3 §8.2.2.6's viewport dimensions, in CSS pixels.
///
/// **This is where a fixed-layout content document's page size comes from**,
/// and it is in the *content document* rather than in the package: §8.2.2.6
/// makes the `<meta name="viewport">` element the one place a pre-paginated
/// XHTML document states how big it is, so two spine items of one book may be
/// two different sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// The `width` of the initial containing block, in CSS pixels.
    pub width: f64,
    /// The `height`.
    pub height: f64,
}

/// A content document as an element tree.
#[derive(Clone, Debug, Default)]
pub struct Dom {
    /// Every element, in document order, parents before children.
    pub nodes: Vec<Node>,
    /// The document element's index, if the document had one.
    pub root: Option<usize>,
    /// What had to be tolerated.
    pub defects: Vec<MarkupDefect>,
    /// What `tinker-pdf-xml` warned about, carried so a caller can report the
    /// doctype question milestone 2 built the warning for.
    pub warnings: Vec<tinker_pdf_xml::Warning>,
}

impl Dom {
    /// Every descendant of `at`, including `at` itself, as a predicate.
    ///
    /// Used to decide whether a positioned text run came from inside a given
    /// element — which is how an `<a href>`'s rectangle and an `id`'s page are
    /// found once the tree has been flattened, fragmented and paginated.
    /// Walking up from the descendant is what makes it O(depth) rather than
    /// O(subtree).
    #[must_use]
    pub fn contains(&self, at: usize, descendant: usize) -> bool {
        let mut cursor = Some(descendant);
        while let Some(index) = cursor {
            if index == at {
                return true;
            }
            cursor = self.nodes.get(index).and_then(|node| node.parent);
        }
        false
    }

    /// The first element with the given `id`, in document order.
    #[must_use]
    pub fn by_id(&self, id: &str) -> Option<usize> {
        self.nodes
            .iter()
            .position(|node| node.id.as_deref() == Some(id))
    }

    /// §8.2.2.6's viewport dimensions, or `None`.
    ///
    /// `None` covers three different documents and the caller cannot act on
    /// the difference, so they are one answer: no `<meta name="viewport">` at
    /// all, one whose `content` names neither dimension, and one that says
    /// `width=device-width` — which is valid HTML and is **not** valid here,
    /// because §8.2.2.6's grammar is two numbers and a reading system with no
    /// device cannot resolve the keyword into one.
    ///
    /// The first `<meta name="viewport">` in document order wins, which is what
    /// a browser does with two of them.
    #[must_use]
    pub fn viewport(&self) -> Option<Viewport> {
        let meta = self.nodes.iter().find(|node| {
            node.is_html()
                && node.name == "meta"
                && node
                    .attr("name")
                    .is_some_and(|name| name.eq_ignore_ascii_case("viewport"))
        })?;
        let content = meta.attr("content")?;
        let mut width = None;
        let mut height = None;
        for pair in content.split(',') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            let value: f64 = value.trim().parse().ok()?;
            if !value.is_finite() || value <= 0.0 {
                return None;
            }
            match key.trim().to_ascii_lowercase().as_str() {
                "width" => width = Some(value),
                "height" => height = Some(value),
                _ => {}
            }
        }
        Some(Viewport {
            width: width?,
            height: height?,
        })
    }

    /// The `<body>`, or the document element when there is none.
    ///
    /// A content document without a `<body>` is not well-formed XHTML and is
    /// also not worth losing a chapter over: laying the document element out
    /// sets the same text, and the `<head>` inside it is `display: none` by the
    /// UA sheet either way.
    #[must_use]
    pub fn body(&self) -> Option<usize> {
        self.nodes
            .iter()
            .position(|node| node.is_html() && node.name == "body")
            .or(self.root)
    }
}

/// Reads a content document into a tree.
///
/// [`Doctype::SkipExternalId`] is milestone 2's mode and this is its first
/// caller: every EPUB 2 content document in the committed corpus carries
/// XHTML 1.1's doctype, and `Doctype::Refuse` — which is what XPS passes and
/// what every other reader in this workspace uses — would refuse each of them
/// before the first tag.
///
/// # Errors
/// Only what [`Source::new`] refuses: an encoding this build does not decode,
/// or a character §2.2 forbids. Everything the *reader* refuses is a
/// [`MarkupDefect`] on a partial tree instead, because a document that stops
/// half way has still said most of a chapter.
pub fn read(bytes: &[u8], limits: &XmlLimits) -> Result<Dom, XmlError> {
    let source = Source::new(bytes)?;
    let mut dom = Dom {
        warnings: source.warnings().to_vec(),
        ..Dom::default()
    };
    let mut reader = source.reader_with(limits, Doctype::SkipExternalId);
    // The indices of the elements that are open, innermost last.
    let mut open: Vec<usize> = Vec::new();

    for event in &mut reader {
        let event = match event {
            Ok(event) => event,
            Err(_) => {
                dom.defects.push(MarkupDefect::Truncated);
                break;
            }
        };
        match event {
            Event::Start(element) => {
                let index = dom.nodes.len();
                let mut node = Node {
                    name: element.local().to_owned(),
                    namespace: element.namespace().map(str::to_owned),
                    id: None,
                    classes: Vec::new(),
                    attributes: Vec::with_capacity(element.attributes().len()),
                    parent: open.last().copied(),
                    previous: None,
                    next: None,
                    children: Vec::new(),
                    style: None,
                };
                for attribute in element.attributes() {
                    let name = attribute.name().qualified();
                    let value = attribute.value();
                    // The three the cascade asks for by name, read once here
                    // rather than scanned for on every selector match.
                    match name {
                        "id" => node.id = Some(value.to_owned()),
                        "class" => {
                            node.classes = value.split_whitespace().map(str::to_owned).collect();
                        }
                        "style" => node.style = Some(value.to_owned()),
                        _ => {}
                    }
                    node.attributes.push((name.to_owned(), value.to_owned()));
                }
                if let Some(&parent) = open.last() {
                    // The previous *element* sibling, which is the last element
                    // child the parent already has — text between the two is
                    // not a sibling in `selectors-4`'s sense.
                    let previous =
                        dom.nodes[parent]
                            .children
                            .iter()
                            .rev()
                            .find_map(|child| match child {
                                Child::Element(at) => Some(*at),
                                Child::Text(_) => None,
                            });
                    node.previous = previous;
                    if let Some(previous) = previous {
                        dom.nodes[previous].next = Some(index);
                    }
                    dom.nodes[parent].children.push(Child::Element(index));
                } else if dom.root.is_none() {
                    dom.root = Some(index);
                }
                dom.nodes.push(node);
                open.push(index);
            }
            Event::End(_) => {
                // `open` cannot be empty here, and that is the reader's
                // guarantee rather than an assumption: `tinker-pdf-xml` emits
                // one `End` per `Start` — an empty-element tag produces both —
                // and refuses a stray end tag by name before it reaches this
                // loop. The injection matrix is what says so: a defect that
                // recorded a mismatch here survived every test in the suite
                // because nothing can produce one.
                open.pop();
            }
            Event::Text(text) | Event::Cdata(text) => {
                if let Some(&parent) = open.last() {
                    dom.nodes[parent]
                        .children
                        .push(Child::Text(text.into_owned()));
                }
            }
            Event::Comment(_) | Event::Instruction { .. } => {}
        }
    }

    dom.warnings.extend_from_slice(reader.warnings());
    // **There is no second check for elements left open**, and its absence is
    // the injection matrix's finding rather than an oversight. A document that
    // ends inside an element is `Error::Unterminated(Construct::Element)` from
    // the reader, which the arm above has already recorded as
    // [`MarkupDefect::Truncated`]; a fallback here would be the same rule
    // enforced twice, with only one half reachable — and a defect injected into
    // the unreachable half survived the whole suite, which is exactly what a
    // rule enforced twice hides.
    if dom.nodes.is_empty() {
        dom.defects.push(MarkupDefect::Empty);
    }
    Ok(dom)
}
