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
//! Every pseudo-class `selectors-4` defers to the document language is
//! answered in [`Node`]'s trait implementation below and **nowhere else**:
//! that `xml:lang` beats `lang`, that an `<a>` is only a link when it has an
//! `href`, that a checkbox is `:checked` when it carries the attribute, and
//! that white-space-only character data does not stop an element being
//! `:empty`. Each of those is a sentence about HTML and XML, and the CSS crate
//! contains none of them.
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
//! rather than an omission. Two of the committed books wrap their cover in an
//! SVG `<image>`; whether that draws is `epub::svg`'s question and not this
//! reader's, and the elements carry no text, so keeping them costs a handful
//! of nodes and keeps the tree a faithful record of the document. What it must
//! not do is let an
//! SVG `<title>` be matched by this build's UA rule for HTML's `<title>` — see
//! [`Node::local_name`], which reports the local name and
//! [`Node::is_html`], which is what the tree walk keys the UA vocabulary on.

use tinker_pdf_css::selector::UiState;
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

    /// HTML §15.3.8's list numbering and §15.3.5's bidirectional text, as
    /// the presentational hints they state.
    ///
    /// `<ol start="n">` is `counter-reset: list-item n−1` and `<li value="n">`
    /// is `counter-set: list-item n`, parsed by HTML's *rules for parsing
    /// integers* — leading white space, an optional sign, digits, and anything
    /// after them ignored. `<ol reversed>` is `counter-reset:
    /// reversed(list-item)`, which this build refuses by value: the parser
    /// counts it against `counter-reset` on the element, rather than the list
    /// being numbered upwards with nothing to say so.
    ///
    /// The bidirectional half is [`bidi_hints`]'s.
    fn presentational_hints(&self) -> Option<String> {
        let list = match self.name.as_str() {
            "ol" if self.attr("reversed").is_some() => {
                Some("counter-reset: reversed(list-item)".to_owned())
            }
            "ol" => self
                .attr("start")
                .and_then(html_integer)
                .map(|start| format!("counter-reset: list-item {}", start.saturating_sub(1))),
            "li" => self
                .attr("value")
                .and_then(html_integer)
                .map(|value| format!("counter-set: list-item {value}")),
            _ => None,
        };
        match (list, bidi_hints(self)) {
            (Some(list), Some(bidi)) => Some(format!("{list}; {bidi}")),
            (list, bidi) => list.or_else(|| bidi.map(str::to_owned)),
        }
    }

    /// `selectors-4` §6.6.3's `:empty`.
    ///
    /// **Character data that is only white space is not content**, so
    /// `<td></td>` and `<td>\n  </td>` are the same empty cell. That is a
    /// decision and it is made here rather than in the CSS crate because it is
    /// a claim about *this* document language: a producer's indentation is
    /// markup formatting, and a book whose every empty table cell stopped
    /// matching `:empty` because pandoc indents its output would be styled by
    /// the pretty-printer.
    ///
    /// Comments and processing instructions cannot appear in `children` at
    /// all — [`read`] drops them — so §6.6.3's rule that they do not affect
    /// emptiness holds by construction rather than by a test here.
    fn is_empty(&self) -> bool {
        self.children.iter().all(|child| match child {
            Child::Element(_) => false,
            Child::Text(text) => text.chars().all(is_document_white_space),
        })
    }

    /// `selectors-4` §6.5.1's language, as XHTML declares it.
    ///
    /// `xml:lang` beats `lang`: an EPUB content document is XML (EPUB 3.3
    /// §3.2), and where a producer writes both — several do, to be readable by
    /// an HTML parser as well — the XML attribute is the normative one.
    ///
    /// An empty declaration is returned as itself rather than as `None`,
    /// because `lang=""` means *the language is not known* and that is a
    /// different statement from not having said: the CSS crate stops
    /// inheriting at it, and it matches no range.
    fn language(&self) -> Option<&str> {
        self.attr("xml:lang").or_else(|| self.attr("lang"))
    }

    /// `selectors-4` §6.6's directionality, as XHTML declares it.
    ///
    /// The value is passed through rather than resolved, `auto` included:
    /// HTML's `dir="auto"` means *work it out from the first strong character
    /// of the content*, which this build does not do — so it reaches `:dir()`
    /// as `auto`, matches neither keyword, and stops the inheritance, rather
    /// than being guessed at as one of the two.
    ///
    /// The document element with nothing declared is `ltr`, which is HTML's
    /// own default and is the one place a default belongs: an element deeper
    /// in the tree that says nothing must inherit rather than assume.
    fn direction(&self) -> Option<&str> {
        match self.attr("dir") {
            Some(value) if !value.is_empty() => Some(value),
            _ if self.parent.is_none() => Some("ltr"),
            _ => None,
        }
    }

    /// `selectors-4` §6.6.1's hyperlink source, as HTML defines one: `<a>`,
    /// `<area>` or `<link>` **with an `href`**. Without the attribute none of
    /// the three is a link, which is the negative half `:any-link` is for.
    fn is_link(&self) -> bool {
        self.is_html()
            && matches!(self.name.as_str(), "a" | "area" | "link")
            && self.attr("href").is_some()
    }

    /// `selectors-4` §12's states, as HTML defines them.
    ///
    /// Three of the four are `Option` because HTML scopes them to the elements
    /// that can hold the state at all — a `<p>` is neither `:enabled` nor
    /// `:disabled` — and the fourth, `read_only`, is `Some` for **every**
    /// element, which is HTML's rule rather than a slip: everything that is
    /// not editable is `:read-only`, so `p:read-only` does match a paragraph.
    ///
    /// **Two simplifications, named.** A control inside a disabled
    /// `<fieldset>` is `:disabled` in HTML and is not here, because this
    /// method sees one element and the ancestor walk would be the CSS crate
    /// asking a question only HTML can pose. And `readonly` is treated as
    /// applying to every `<input>`, where HTML applies it only to the text-like
    /// types. Both are invisible in an EPUB, whose scripting is refused by
    /// name and whose forms are inert either way.
    fn ui_state(&self) -> UiState {
        if !self.is_html() {
            return UiState::NONE;
        }
        let present = |name: &str| self.attr(name).is_some();
        let editable = self
            .attr("contenteditable")
            .is_some_and(|value| !value.eq_ignore_ascii_case("false"));
        UiState {
            checked: match self.name.as_str() {
                "input" => {
                    present("checked")
                        && self.attr("type").is_some_and(|kind| {
                            kind.eq_ignore_ascii_case("checkbox")
                                || kind.eq_ignore_ascii_case("radio")
                        })
                }
                "option" => present("selected"),
                _ => false,
            },
            disabled: match self.name.as_str() {
                "button" | "input" | "select" | "textarea" | "optgroup" | "option" | "fieldset" => {
                    Some(present("disabled"))
                }
                _ => None,
            },
            required: match self.name.as_str() {
                "input" | "select" | "textarea" => Some(present("required")),
                _ => None,
            },
            read_only: Some(match self.name.as_str() {
                "input" | "textarea" => present("readonly") || present("disabled"),
                _ => !editable,
            }),
        }
    }
}

/// `css-text-3`'s document white space: the five characters a producer's
/// indentation is made of.
fn is_document_white_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{000C}')
}

/// HTML §2.3.4.1's *rules for parsing integers*: leading ASCII white space, an
/// optional `-` or `+`, at least one digit, and nothing after the digits read.
///
/// `None` where there is no digit, which HTML calls an error and which leaves
/// the attribute without effect — `start="x"` numbers from one, as it does in
/// a browser. A value past `i32`'s range is clamped, `css-values-4` §5.1's rule
/// for the integer the hint becomes.
fn html_integer(raw: &str) -> Option<i32> {
    let text = raw.trim_start_matches(is_document_white_space);
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let digits: &str = &digits[..digits
        .bytes()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(digits.len())];
    if digits.is_empty() {
        return None;
    }
    let magnitude = digits.bytes().fold(0i64, |acc, b| {
        (acc * 10 + i64::from(b - b'0')).min(i64::from(i32::MAX) + 1)
    });
    let value = if negative { -magnitude } else { magnitude };
    Some(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
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
    /// The document is not well-formed XML and was read by HTML's own parser
    /// (WHATWG §13.2) instead, which reads every input to its end: a loose
    /// HTML file, or HTML handed to `DocumentBuilder::from_html`. Never said
    /// of an EPUB chapter, which is XHTML by its media type.
    NotXml,
    /// Bytes the decoder in front of HTML's parser could not map — malformed
    /// UTF-8 or UTF-16 after its byte order mark, or a byte the single-byte
    /// table a `<meta>` or an XML declaration named leaves unmapped, as
    /// windows-1253 leaves 0xAA — each read as U+FFFD. Never windows-1252,
    /// HTML's default, whose table maps all 256 bytes.
    Undecodable,
    /// A `<meta charset>` named an encoding this build does not decode — one
    /// of the multi-byte legacy encodings — and HTML's decoder read the bytes
    /// as UTF-8 where they are UTF-8 and as windows-1252 where they are not.
    EncodingNotDecoded,
    /// Elements HTML's tree builder nested past the XML reader's depth cap,
    /// which it can do by nesting the adoption agency's clones: their text is
    /// kept, in the deepest element the cap allows, and their structure is
    /// not.
    TooDeep,
}

/// HTML §15.3.5's bidirectional rendering, as the declarations an element's
/// `dir` attribute and its name make.
///
/// HTML writes these as user-agent rules keyed on `[dir]` and `:dir()`; they
/// are hints here, at the start of the author sheet rather than in
/// `ua.css`, because a rule keyed on an attribute alone is tried against
/// every element of every book and a hint is asked of each element once —
/// and an author rule beats both alike. The value is HTML's enumerated
/// attribute, ASCII case-insensitive, and a value that is none of the three
/// is no `dir` at all.
///
/// - `dir="ltr"` and `dir="rtl"` set `direction` and open an isolate, as
///   §15.3.5's `[dir] { unicode-bidi: isolate }` does;
/// - `dir="auto"`, and a `<bdi>` with no `dir`, are `unicode-bidi:
///   plaintext`: the isolate whose direction is its content's first strong
///   character (`css-writing-modes-3` §2.2) and, on a block, each
///   paragraph's own P2 and P3. HTML computes a `direction` from the content
///   instead and lets it inherit, which a descendant's own `direction`
///   would read; here a descendant inherits the parent's;
/// - `<bdo>` is `unicode-bidi: isolate-override`, which this build refuses
///   by value, so each `<bdo>` is counted rather than read as honoured.
fn bidi_hints(node: &Node) -> Option<&'static str> {
    if !node.is_html() {
        return None;
    }
    let dir = node.attr("dir").and_then(|value| {
        ["ltr", "rtl", "auto"]
            .into_iter()
            .find(|keyword| value.eq_ignore_ascii_case(keyword))
    });
    let bdo = node.name == "bdo";
    Some(match (dir, bdo) {
        (Some("ltr"), true) => "direction: ltr; unicode-bidi: isolate-override",
        (Some("rtl"), true) => "direction: rtl; unicode-bidi: isolate-override",
        (_, true) => "unicode-bidi: isolate-override",
        (Some("ltr"), false) => "direction: ltr; unicode-bidi: isolate",
        (Some("rtl"), false) => "direction: rtl; unicode-bidi: isolate",
        (Some(_), false) => "unicode-bidi: plaintext",
        (None, false) if node.name == "bdi" => "unicode-bidi: plaintext",
        (None, false) => return None,
    })
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

    /// The document's `<title>`, white space collapsed, or `None` when it has
    /// none or the element is empty.
    ///
    /// HTML's own definition — the first `title` element in the HTML
    /// namespace, in tree order — which is why an SVG `<title>` inside the body
    /// is never it. A book's title comes from its package document instead;
    /// this is for a content document that is the whole document.
    #[must_use]
    pub fn title(&self) -> Option<String> {
        let node = self
            .nodes
            .iter()
            .find(|node| node.is_html() && node.name == "title")?;
        let mut text = String::new();
        for child in &node.children {
            if let Child::Text(chunk) = child {
                text.push_str(chunk);
            }
        }
        let title = text
            .split(is_document_white_space)
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        (!title.is_empty()).then_some(title)
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
    Ok(read_reporting(&source, limits).0)
}

/// [`read`] of a decoded source, and the refusal that truncated the tree, if
/// one did — which is what [`read_markup_or_html`] decides on.
fn read_reporting(source: &Source<'_>, limits: &XmlLimits) -> (Dom, Option<XmlError>) {
    let mut dom = Dom {
        warnings: source.warnings().to_vec(),
        ..Dom::default()
    };
    let mut reader = source.reader_with(limits, Doctype::SkipExternalId);
    // The indices of the elements that are open, innermost last.
    let mut open: Vec<usize> = Vec::new();
    let mut refusal = None;

    for event in &mut reader {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                dom.defects.push(MarkupDefect::Truncated);
                refusal = Some(error);
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
    (dom, refusal)
}

/// Reads a document that is HTML **or** XHTML: as XML first, and — when the
/// XML reader refuses it for anything but one of its caps — by HTML's own
/// parser (`tinker_pdf_xml::html`, WHATWG §13.2), with
/// [`MarkupDefect::NotXml`] saying so.
///
/// **XML first**, because a loose file that is well-formed XHTML is read
/// exactly as an EPUB's chapter is, and `tests/standalone.rs` holds the two
/// pixel-equal; HTML's parser reads `<div/>` as an open `<div>` and would
/// break that for every such file. **HTML when XML refuses**, because a file
/// that is not well-formed XML is HTML — a `<p>` left open, an attribute
/// unquoted, a `&nbsp` without its semicolon — and HTML's parser reads every
/// input to the end where the XML reader stopped at the first of them. A
/// refusal at a cap is not a question of syntax, and reading the document
/// again would meet the same cap, so the XML reader's tree stands.
///
/// **In the encoding its declaration names**, single-byte ones included
/// ([`Source::with_declared_encoding`]): a loose file is not an EPUB chapter,
/// which EPUB 3.3 holds to UTF-8 or UTF-16, and an XHTML file whose
/// declaration says `windows-1251` is well-formed XML in windows-1251. Read by
/// [`Source::new`] it was refused for its encoding and handed to HTML's
/// parser, which does not read an XML declaration and set the page in
/// windows-1252's letters. A file that declares one and is *not* well-formed
/// goes to HTML's parser as the characters that encoding decodes, not as
/// bytes for it to guess at, with [`MarkupDefect::Undecodable`] if the table
/// left a byte unmapped.
#[must_use]
pub fn read_markup_or_html(bytes: &[u8], limits: &XmlLimits) -> Dom {
    let source = Source::with_declared_encoding(bytes);
    if let Ok(source) = &source {
        match read_reporting(source, limits) {
            (dom, None) => return dom,
            (
                dom,
                Some(
                    XmlError::DepthCap
                    | XmlError::AttributeCap
                    | XmlError::NameCap
                    | XmlError::TokenCap,
                ),
            ) => return dom,
            _ => {}
        }
        if let tinker_pdf_xml::Encoding::SingleByte(_) = source.encoding() {
            let mut dom = from_html(&tinker_pdf_xml::html::parse(source.text(), limits), limits);
            if source
                .warnings()
                .contains(&tinker_pdf_xml::Warning::UnmappedByte)
            {
                dom.defects.push(MarkupDefect::Undecodable);
            }
            return dom;
        }
    }
    from_html(&tinker_pdf_xml::html::parse_bytes(bytes, limits), limits)
}

/// The tree HTML's parser built, as this reader's tree.
///
/// Elements in document order, parents first, every element in the namespace
/// the parser put it in — so an `<svg>` inside a `<p>` is an SVG element here
/// as it is in an XHTML file that declares it. Comments, the DOCTYPE and a
/// `<template>`'s content (which is not among its children, and is inert) are
/// dropped, as [`read`] drops what is not content.
///
/// **One bound is kept here and not in the parser.** HTML's tree builder can
/// make the tree deeper than its own stack of open elements, because the
/// adoption agency nests clones inside the blocks it moves; every reader past
/// this one was written against `tinker_pdf_xml::limits::MAX_XML_DEPTH`
/// standing in front of it. An element past `limits.max_depth` is not made: its
/// text is kept, in the deepest element the cap allows, and
/// [`MarkupDefect::TooDeep`] says so.
#[must_use]
pub fn from_html(document: &tinker_pdf_xml::html::Document, limits: &XmlLimits) -> Dom {
    use tinker_pdf_xml::html::NodeData;

    let mut dom = Dom {
        defects: vec![MarkupDefect::NotXml],
        ..Dom::default()
    };
    if document.stopped().is_some() {
        dom.defects.push(MarkupDefect::Truncated);
    }
    if document.encoding().is_some_and(|d| d.not_decoded.is_some()) {
        dom.defects.push(MarkupDefect::EncodingNotDecoded);
    }
    if document.encoding().is_some_and(|d| d.replaced > 0) {
        dom.defects.push(MarkupDefect::Undecodable);
    }
    let Some(root) = document.document_element() else {
        dom.defects.push(MarkupDefect::Empty);
        return dom;
    };
    let mut too_deep = false;
    // (node in the HTML tree, the element it goes inside, its depth)
    let mut stack: Vec<(usize, Option<usize>, usize)> = vec![(root, None, 1)];
    while let Some((at, parent, depth)) = stack.pop() {
        let Some(node) = document.node(at) else {
            continue;
        };
        match &node.data {
            NodeData::Text(text) => {
                if let Some(parent) = parent.and_then(|p| dom.nodes.get_mut(p)) {
                    parent.children.push(Child::Text(text.clone()));
                }
            }
            NodeData::Element(element) => {
                if depth > limits.max_depth {
                    // The element is not made; what it holds goes on into the
                    // deepest one that was.
                    too_deep = true;
                    for &child in node.children.iter().rev() {
                        stack.push((child, parent, depth));
                    }
                    continue;
                }
                let index = dom.nodes.len();
                let mut made = Node {
                    name: element.name.clone(),
                    namespace: Some(element.namespace.uri().to_owned()),
                    id: None,
                    classes: Vec::new(),
                    attributes: Vec::with_capacity(element.attributes.len()),
                    parent,
                    previous: None,
                    next: None,
                    children: Vec::new(),
                    style: None,
                };
                for attribute in &element.attributes {
                    let name = attribute.qualified();
                    match name.as_str() {
                        "id" => made.id = Some(attribute.value.clone()),
                        "class" => {
                            made.classes = attribute
                                .value
                                .split_whitespace()
                                .map(str::to_owned)
                                .collect();
                        }
                        "style" => made.style = Some(attribute.value.clone()),
                        _ => {}
                    }
                    made.attributes.push((name, attribute.value.clone()));
                }
                if let Some(parent_index) = parent {
                    let previous = dom.nodes.get(parent_index).and_then(|p| {
                        p.children.iter().rev().find_map(|child| match child {
                            Child::Element(at) => Some(*at),
                            Child::Text(_) => None,
                        })
                    });
                    made.previous = previous;
                    if let Some(previous) = previous.and_then(|p| dom.nodes.get_mut(p)) {
                        previous.next = Some(index);
                    }
                    if let Some(p) = dom.nodes.get_mut(parent_index) {
                        p.children.push(Child::Element(index));
                    }
                } else if dom.root.is_none() {
                    dom.root = Some(index);
                }
                dom.nodes.push(made);
                for &child in node.children.iter().rev() {
                    stack.push((child, Some(index), depth + 1));
                }
            }
            NodeData::Document
            | NodeData::Fragment
            | NodeData::Doctype { .. }
            | NodeData::Comment(_) => {}
        }
    }
    if too_deep {
        dom.defects.push(MarkupDefect::TooDeep);
    }
    if dom.nodes.is_empty() {
        dom.defects.push(MarkupDefect::Empty);
    }
    dom
}
