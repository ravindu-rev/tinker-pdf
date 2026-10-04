//! HTML as the WHATWG living standard parses it: §13.2.5's tokenizer and
//! §13.2.6's tree builder, bytes or text in, a document tree out.
//!
//! Feature documentation: `docs/features/opening.md`.
//!
//! **Why a second parser in an XML crate.** Most HTML is not XML: a `<p>` or a
//! `<br>` left open, an attribute without quotes, a `&nbsp` without its
//! semicolon. The XML reader stops at the first of them, and before this
//! module a loose `.html` file opened as far as it parsed. HTML's own parser
//! never stops — every input is a document, and the standard says which one
//! — so a file that is not XML is read again by this. It is here rather than
//! in a crate of its own because it is the same kind of thing the XML reader
//! is, a markup reader with no PDF vocabulary (ruling 8), and it shares that
//! reader's [`Limits`].
//!
//! # What it is held to
//!
//! html5lib's tree-construction tests, vendored under `data/html5lib-tests`:
//! `tests/html5lib.rs` parses every test that runs with scripting disabled and
//! compares the tree, serialised the way the suite writes it, exactly, and
//! holds a counted floor. **Scripting is disabled, always**: nothing here runs
//! a script, so `<noscript>` is markup a reader sees, which is what a
//! renderer with no script engine must show.
//!
//! # What it does not do
//!
//! - **No scripts, no `document.write`, no speculative parsing**: the parser
//!   pause flag is never set.
//! - **Declarative shadow roots are not attached**: a `<template
//!   shadowrootmode>` is an ordinary template, which is what the standard
//!   gives a parser whose *allow declarative shadow roots* is false.
//! - **Encodings past UTF-8, UTF-16 and the single-byte family** — the
//!   multi-byte legacy encodings a `<meta charset>` may name are read as
//!   UTF-8 if the bytes are UTF-8 and windows-1252 if not, and
//!   [`Document::encoding`] says which, with `confident: false`.
//!
//! # Bounds
//!
//! The XML reader's [`Limits`], read as the same four ceilings: the stack of
//! open elements is held to [`Limits::max_depth`], a tag's attributes to
//! [`Limits::max_attributes`], a tag, attribute or DOCTYPE name to
//! [`Limits::max_name_len`], and [`Limits::max_tokens`] is **the total**,
//! spent by every token and by every node the tree builder creates — so the
//! elements the standard creates on its own account, reopening formatting
//! elements and cloning them in the adoption agency, are inside the same
//! budget as the tokens that asked for them. Past any of them the parse stops
//! and [`Document::stopped`] says which; the tree built so far is kept.

mod entities;
#[cfg(test)]
mod suite;
mod tokenizer;
mod tree;

use crate::encoding::{self, Label, SingleByte};
use crate::{Error, Limits};

/// The namespace an element is in. HTML's parser makes elements in three.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Namespace {
    /// `http://www.w3.org/1999/xhtml`.
    Html,
    /// `http://www.w3.org/2000/svg`.
    Svg,
    /// `http://www.w3.org/1998/Math/MathML`.
    MathMl,
}

impl Namespace {
    /// The namespace's URI.
    #[must_use]
    pub fn uri(self) -> &'static str {
        match self {
            Namespace::Html => "http://www.w3.org/1999/xhtml",
            Namespace::Svg => "http://www.w3.org/2000/svg",
            Namespace::MathMl => "http://www.w3.org/1998/Math/MathML",
        }
    }
}

/// The namespace an attribute is in, when it is in one: §13.2.6.1's *adjust
/// foreign attributes* puts `xlink:href`, `xml:lang` and `xmlns` there on an
/// SVG or MathML element. Every other attribute is in none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttributeNamespace {
    /// `http://www.w3.org/1999/xlink`.
    XLink,
    /// `http://www.w3.org/XML/1998/namespace`.
    Xml,
    /// `http://www.w3.org/2000/xmlns/`.
    Xmlns,
}

/// One attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    /// The local name: lowercased by the tokenizer, and given SVG's or
    /// MathML's camel case back by the tree builder on those elements.
    pub name: String,
    /// The namespace, for the eleven names adjusted into one.
    pub namespace: Option<AttributeNamespace>,
    /// The value, character references resolved.
    pub value: String,
}

impl Attribute {
    /// The name as a namespaced document would spell it — `xlink:href`,
    /// `xml:lang`, `xmlns:xlink` — and the local name otherwise.
    #[must_use]
    pub fn qualified(&self) -> String {
        match self.namespace {
            Some(AttributeNamespace::XLink) => format!("xlink:{}", self.name),
            Some(AttributeNamespace::Xml) => format!("xml:{}", self.name),
            Some(AttributeNamespace::Xmlns) if self.name != "xmlns" => {
                format!("xmlns:{}", self.name)
            }
            _ => self.name.clone(),
        }
    }
}

/// An element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    /// The local name.
    pub name: String,
    /// The namespace.
    pub namespace: Namespace,
    /// Attributes, in source order.
    pub attributes: Vec<Attribute>,
    /// A `<template>`'s content: the [`NodeData::Fragment`] its children are
    /// in, which is not among its children.
    pub template_contents: Option<usize>,
    /// Whether a MathML `annotation-xml` is an HTML integration point — its
    /// start tag's `encoding` was `text/html` or `application/xhtml+xml`.
    pub(crate) integration_point: bool,
}

impl Element {
    /// The value of the attribute in no namespace with this name.
    #[must_use]
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| a.namespace.is_none() && a.name == name)
            .map(|a| a.value.as_str())
    }
}

/// What a node is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeData {
    /// The document: node 0, always.
    Document,
    /// A document fragment: a `<template>`'s content.
    Fragment,
    /// `<!DOCTYPE>`, with each identifier the empty string where it was
    /// missing.
    Doctype {
        name: String,
        public_id: String,
        system_id: String,
    },
    Element(Element),
    /// Character data. Two text nodes are never siblings: the tree builder
    /// appends to the one before.
    Text(String),
    Comment(String),
}

/// One node of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// The parent's index; `None` for the document, a template's content and
    /// a node the adoption agency took out of the tree.
    pub parent: Option<usize>,
    /// Children, in order, as indices.
    pub children: Vec<usize>,
    /// What the node is.
    pub data: NodeData,
}

impl Node {
    /// The element, if this node is one.
    #[must_use]
    pub fn element(&self) -> Option<&Element> {
        match &self.data {
            NodeData::Element(element) => Some(element),
            _ => None,
        }
    }
}

/// §13.2.6.4.1's quirks modes, which decide one thing the tree builder does:
/// whether a `<table>` closes an open `<p>`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Quirks {
    #[default]
    NoQuirks,
    Limited,
    Quirks,
}

/// How the bytes were decoded, by [`parse_bytes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecodedAs {
    /// UTF-8.
    Utf8,
    /// UTF-16, little-endian.
    Utf16LittleEndian,
    /// UTF-16, big-endian.
    Utf16BigEndian,
    /// A single-byte encoding.
    SingleByte(SingleByte),
}

/// What [`parse_bytes`] decided about the encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Decoding {
    /// The encoding the text was decoded from.
    pub encoding: DecodedAs,
    /// Whether something in the bytes said so: a byte order mark or a
    /// `<meta charset>` this build decodes. `false` is a default or a guess —
    /// UTF-8 because the bytes were UTF-8, windows-1252 because they were
    /// not.
    pub confident: bool,
    /// Bytes the decoder could not map, each read as U+FFFD.
    pub replaced: usize,
    /// The Encoding Standard's name for an encoding a `<meta>` named and
    /// this crate does not decode — one of the multi-byte legacy encodings,
    /// `replacement` or `x-user-defined` — set aside for the guess.
    pub not_decoded: Option<&'static str>,
}

/// A parsed document.
#[derive(Clone, Debug, Default)]
pub struct Document {
    nodes: Vec<Node>,
    root: usize,
    quirks: Quirks,
    errors: usize,
    stopped: Option<Error>,
    decoding: Option<Decoding>,
}

impl Document {
    /// Every node; index 0 is the [`NodeData::Document`].
    #[must_use]
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// One node.
    #[must_use]
    pub fn node(&self, at: usize) -> Option<&Node> {
        self.nodes.get(at)
    }

    /// The node whose children are the result: the document, or for
    /// [`parse_fragment`] the `<html>` element the fragment was parsed into.
    #[must_use]
    pub fn root(&self) -> usize {
        self.root
    }

    /// The document element: the root's first element child.
    #[must_use]
    pub fn document_element(&self) -> Option<usize> {
        self.node(self.root)?
            .children
            .iter()
            .copied()
            .find(|&child| self.node(child).is_some_and(|n| n.element().is_some()))
    }

    /// The quirks mode the DOCTYPE decided.
    #[must_use]
    pub fn quirks(&self) -> Quirks {
        self.quirks
    }

    /// How many parse errors §13.2 names were met. Every one of them was
    /// recovered from — that is what an HTML parse error is.
    #[must_use]
    pub fn errors(&self) -> usize {
        self.errors
    }

    /// The cap that stopped the parse, if one did.
    #[must_use]
    pub fn stopped(&self) -> Option<Error> {
        self.stopped
    }

    /// How [`parse_bytes`] decoded the input; `None` from [`parse`].
    #[must_use]
    pub fn encoding(&self) -> Option<Decoding> {
        self.decoding
    }
}

/// §13.2.3.5: every CR LF pair and every lone CR is one LF.
fn normalise_newlines(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains('\r') {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            out.push('\n');
        } else {
            out.push(c);
        }
    }
    std::borrow::Cow::Owned(out)
}

/// Parses a document from text, with scripting disabled.
#[must_use]
pub fn parse(text: &str, limits: &Limits) -> Document {
    let text = normalise_newlines(text);
    tree::parse_document(&text, limits)
}

/// Parses `text` as the content of a `context` element — §13.2.8's fragment
/// algorithm — with scripting disabled. The result's [`Document::root`] is
/// the `<html>` element the fragment's nodes are children of.
#[must_use]
pub fn parse_fragment(text: &str, context: (Namespace, &str), limits: &Limits) -> Document {
    let text = normalise_newlines(text);
    tree::parse_fragment(&text, context, limits)
}

/// Decodes bytes as §13.2.3 does and parses them.
///
/// The encoding is the first of: a byte order mark (UTF-8 or UTF-16 in either
/// order); a `<meta charset>` or `<meta http-equiv="content-type">` in the
/// first kilobyte, by §13.2.3.2's prescan, naming an encoding this crate
/// decodes; UTF-8 when the bytes are valid UTF-8; and windows-1252 —
/// §13.2.3.3's default for most locales, and what a document that says
/// nothing about its encoding and is not UTF-8 is overwhelmingly in.
#[must_use]
pub fn parse_bytes(bytes: &[u8], limits: &Limits) -> Document {
    let (text, decoding) = decode(bytes);
    let mut document = parse(&text, limits);
    document.decoding = Some(decoding);
    document
}

/// The bytes as text, and how they were read.
#[must_use]
pub fn decode(bytes: &[u8]) -> (String, Decoding) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        let (text, replaced) = lossy_utf8(rest);
        return (text, decoded(DecodedAs::Utf8, true, replaced));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let (text, replaced) = lossy_utf16(rest, false);
        return (text, decoded(DecodedAs::Utf16LittleEndian, true, replaced));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let (text, replaced) = lossy_utf16(rest, true);
        return (text, decoded(DecodedAs::Utf16BigEndian, true, replaced));
    }
    let mut not_decoded = None;
    match prescan(bytes) {
        Some(Label::SingleByte(single)) => {
            let (text, replaced) = single.decode(bytes);
            return (text, decoded(DecodedAs::SingleByte(single), true, replaced));
        }
        // §13.2.3.2: a UTF-16 label found by the prescan means UTF-8, since
        // an ASCII-compatible `<meta>` could not have been read in UTF-16.
        Some(Label::Utf8 | Label::Utf16LittleEndian | Label::Utf16BigEndian) => {
            let (text, replaced) = lossy_utf8(bytes);
            return (text, decoded(DecodedAs::Utf8, true, replaced));
        }
        Some(Label::Unsupported(name)) => not_decoded = Some(name),
        _ => {}
    }
    let (text, mut decoding) = match std::str::from_utf8(bytes) {
        Ok(text) => (text.to_owned(), decoded(DecodedAs::Utf8, false, 0)),
        Err(_) => {
            let single = SingleByte::Windows1252;
            let (text, replaced) = single.decode(bytes);
            (
                text,
                decoded(DecodedAs::SingleByte(single), false, replaced),
            )
        }
    };
    decoding.not_decoded = not_decoded;
    (text, decoding)
}

fn decoded(encoding: DecodedAs, confident: bool, replaced: usize) -> Decoding {
    Decoding {
        encoding,
        confident,
        replaced,
        not_decoded: None,
    }
}

fn lossy_utf8(bytes: &[u8]) -> (String, usize) {
    let mut text = String::with_capacity(bytes.len());
    let mut replaced = 0;
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            text.push('\u{FFFD}');
            replaced += 1;
        }
    }
    (text, replaced)
}

fn lossy_utf16(bytes: &[u8], big_endian: bool) -> (String, usize) {
    let units = bytes.chunks(2).map(|pair| match pair {
        [a, b] if big_endian => u16::from_be_bytes([*a, *b]),
        [a, b] => u16::from_le_bytes([*a, *b]),
        // An odd byte at the end is half a code unit, which is not one.
        _ => 0xDC00,
    });
    let mut text = String::with_capacity(bytes.len() / 2);
    let mut replaced = 0;
    for unit in char::decode_utf16(units) {
        match unit {
            Ok(c) => text.push(c),
            Err(_) => {
                text.push('\u{FFFD}');
                replaced += 1;
            }
        }
    }
    (text, replaced)
}

/// §13.2.3.2's prescan, over the first 1 024 bytes: comments skipped, other
/// tags' attributes skipped, and the first `<meta>` that names an encoding
/// — `charset`, or `http-equiv="content-type"` with a `content` holding
/// `charset=` — wins.
fn prescan(bytes: &[u8]) -> Option<Label> {
    let head = bytes.get(..bytes.len().min(1024)).unwrap_or(bytes);
    let mut at = 0;
    while at < head.len() {
        let rest = head.get(at..).unwrap_or_default();
        if rest.starts_with(b"<!--") {
            let end = find(rest.get(2..).unwrap_or_default(), b"-->")?;
            at += 2 + end + 3;
            continue;
        }
        let meta = rest
            .get(..5)
            .is_some_and(|w| w.eq_ignore_ascii_case(b"<meta"))
            && rest
                .get(5)
                .is_some_and(|b| b.is_ascii_whitespace() || *b == b'/');
        if meta {
            at += 5;
            let mut charset: Option<Label> = None;
            let mut pragma = false;
            let mut content: Option<Label> = None;
            let mut seen: Vec<Vec<u8>> = Vec::new();
            loop {
                let Some((name, value, next)) = prescan_attribute(head, at) else {
                    break;
                };
                at = next;
                if seen.contains(&name) {
                    continue;
                }
                seen.push(name.clone());
                match name.as_slice() {
                    b"http-equiv" => pragma = value.eq_ignore_ascii_case(b"content-type"),
                    b"content" if content.is_none() => content = charset_from_content(&value),
                    b"charset" if charset.is_none() => {
                        charset = std::str::from_utf8(&value).ok().and_then(encoding::lookup);
                    }
                    _ => {}
                }
            }
            let found = charset.or(if pragma { content } else { None });
            if found.is_some() {
                return found;
            }
            continue;
        }
        let tag = rest.first() == Some(&b'<')
            && rest
                .get(1)
                .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'/');
        if tag {
            at += 1;
            while head
                .get(at)
                .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'>')
            {
                at += 1;
            }
            while let Some((_, _, next)) = prescan_attribute(head, at) {
                at = next;
            }
            continue;
        }
        if rest.starts_with(b"<!") || rest.starts_with(b"<?") {
            at += find(rest, b">")? + 1;
            continue;
        }
        at += 1;
    }
    None
}

/// §13.2.3.2's *get an attribute*, from `at`: the name lowercased, the value,
/// and where the next one starts — or `None` at the tag's `>`.
fn prescan_attribute(bytes: &[u8], mut at: usize) -> Option<(Vec<u8>, Vec<u8>, usize)> {
    while bytes
        .get(at)
        .is_some_and(|b| b.is_ascii_whitespace() || *b == b'/')
    {
        at += 1;
    }
    if bytes.get(at).is_none_or(|b| *b == b'>') {
        return None;
    }
    let mut name = Vec::new();
    while let Some(&b) = bytes.get(at) {
        if b == b'=' && !name.is_empty() {
            break;
        }
        if b.is_ascii_whitespace() || b == b'/' || b == b'>' {
            break;
        }
        name.push(b.to_ascii_lowercase());
        at += 1;
    }
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    if bytes.get(at) != Some(&b'=') {
        return Some((name, Vec::new(), at));
    }
    at += 1;
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    let mut value = Vec::new();
    match bytes.get(at) {
        Some(&quote @ (b'"' | b'\'')) => {
            at += 1;
            while let Some(&b) = bytes.get(at) {
                at += 1;
                if b == quote {
                    return Some((name, value, at));
                }
                value.push(b.to_ascii_lowercase());
            }
            // Ran off the end of the window inside a quoted value.
            None
        }
        _ => {
            while let Some(&b) = bytes.get(at) {
                if b.is_ascii_whitespace() || b == b'>' {
                    break;
                }
                value.push(b.to_ascii_lowercase());
                at += 1;
            }
            Some((name, value, at))
        }
    }
}

/// §2.5.6's *extracting a character encoding from a meta element*: the label
/// after the first `charset=` in a `content` value.
fn charset_from_content(value: &[u8]) -> Option<Label> {
    let at = find(value, b"charset")?;
    let mut rest = value.get(at + 7..)?.trim_ascii_start();
    rest = rest.strip_prefix(b"=")?.trim_ascii_start();
    let label: &[u8] = match rest.first() {
        Some(&quote @ (b'"' | b'\'')) => {
            let body = rest.get(1..)?;
            body.get(..body.iter().position(|&b| b == quote)?)?
        }
        _ => {
            let end = rest
                .iter()
                .position(|b| b.is_ascii_whitespace() || *b == b';')
                .unwrap_or(rest.len());
            rest.get(..end)?
        }
    };
    encoding::lookup(std::str::from_utf8(label).ok()?)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
