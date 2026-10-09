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
//! renderer with no script engine must show. Its encoding tests, vendored
//! beside them, are run through [`parse_bytes`] the same way: all 82 decode in
//! the encoding the suite names.
//!
//! # What it does not do
//!
//! - **No scripts, no `document.write`, no speculative parsing**: the parser
//!   pause flag is never set.
//! - **Declarative shadow roots are not attached**: a `<template
//!   shadowrootmode>` is an ordinary template, which is what the standard
//!   gives a parser whose *allow declarative shadow roots* is false.
//! - **Encodings past UTF-8, UTF-16 and the single-byte family** — the
//!   multi-byte legacy encodings a `<meta charset>` or an XML declaration may
//!   name are read as UTF-8 if the bytes are UTF-8 and windows-1252 if not,
//!   and [`Document::encoding`] says which, with `confident: false`.
//! - **No guessing by letter frequency.** §13.2.3.2 lets a decoder with
//!   nothing to go on autodetect; this one tells UTF-8 from not, and that is
//!   all.
//!
//! # Bounds
//!
//! The XML reader's [`Limits`], read as the same four ceilings: the stack of
//! open elements is held to [`Limits::max_depth`]; an element's attributes to
//! [`Limits::max_attributes`] — a tag's, and the `<html>` or `<body>` that a
//! later tag's attributes are merged into; a tag, attribute or DOCTYPE name to
//! [`Limits::max_name_len`]; and [`Limits::max_tokens`] is **the total**,
//! spent by every token, by every node the tree builder creates and by every
//! attribute it copies onto a clone — so the elements the standard creates on
//! its own account, reopening formatting elements and cloning them in the
//! adoption agency, are inside the same budget as the tokens that asked for
//! them.
//!
//! Two more are HTML's own, in [`crate::limits`]: the list of active
//! formatting elements is held to [`MAX_HTML_ACTIVE_FORMATTING`] entries,
//! because its cells' markers let it outgrow the stack; and the bytes of the
//! attributes those clones copy, the one place a tree is bigger than its
//! input, to [`MAX_HTML_CLONE_BYTES`]. Past any of the six the parse stops and
//! [`Document::stopped`] says which; the tree built so far is kept.
//!
//! [`MAX_HTML_ACTIVE_FORMATTING`]: crate::limits::MAX_HTML_ACTIVE_FORMATTING
//! [`MAX_HTML_CLONE_BYTES`]: crate::limits::MAX_HTML_CLONE_BYTES

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
            .find(|a| {
                step();
                a.namespace.is_none() && a.name == name
            })
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
    /// Whether something in the bytes said so: a byte order mark, UTF-16's
    /// `<?x`, or a `<meta charset>` or XML declaration naming an encoding this
    /// build decodes. `false` is a default or a guess — UTF-8 because the
    /// bytes were UTF-8, windows-1252 because they were not.
    pub confident: bool,
    /// Bytes the decoder could not map, each read as U+FFFD.
    pub replaced: usize,
    /// The Encoding Standard's name for an encoding a `<meta>` or an XML
    /// declaration named and this crate does not decode — one of the
    /// multi-byte legacy encodings, `replacement`, or an XML declaration's
    /// `x-user-defined` — set aside for the guess. (A `<meta>`'s
    /// `x-user-defined` is not one: §13.2.3.2 reads it as windows-1252.)
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
    /// The encoding the first `<meta>` the tree builder met named, for
    /// [`parse_bytes`]'s §13.2.3.4.
    meta_encoding: Option<Label>,
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

#[cfg(test)]
thread_local! {
    /// [`step`]'s count, on this thread.
    pub(crate) static STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One step of a loop a crafted input could make quadratic: a child looked at
/// in search of a reference node, two attributes compared by Noah's Ark, an
/// attribute looked up by name. Counted only under `cfg(test)`, where the
/// tree builder's unit tests hold the total to a multiple of the input's
/// length — so that a regression to a quadratic loop **fails** rather than
/// runs slowly, which `cargo test`, having no timeout, would not notice.
/// Everywhere else it is nothing.
#[inline]
pub(crate) fn step() {
    #[cfg(test)]
    STEPS.with(|steps| steps.set(steps.get().saturating_add(1)));
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
/// order); §13.2.3.2's prescan of the first kilobyte — a UTF-16 `<?x` with no
/// mark, then a `<meta charset>` or `<meta http-equiv="content-type">`, then,
/// when the prescan runs out of bytes without one, the `encoding` an
/// `<?xml … ?>` at the very start declares; UTF-8 when the bytes are valid
/// UTF-8; and windows-1252 — §13.2.3.3's default for most locales, and what a
/// document that says nothing about its encoding and is not UTF-8 is
/// overwhelmingly in. An encoding the prescan finds and this crate does not
/// decode is [`Decoding::not_decoded`], and the guess stands.
///
/// **Then §13.2.3.4.** Every encoding but a byte order mark's is tentative,
/// and the first `<meta>` the tree builder meets that names one
/// (§13.2.6.4.4's, in `<head>` or handed to `<head>`'s rules by another
/// mode) *changes the encoding*: when it names another than the one the bytes
/// were read in, they are decoded again in it and parsed again, once — the
/// second reading is certain. That is how a `<meta>` past the prescan's
/// kilobyte is read, and the only way this parses the input twice.
#[must_use]
pub fn parse_bytes(bytes: &[u8], limits: &Limits) -> Document {
    let (text, decoding) = decode(bytes);
    let document = parse(&text, limits);
    let (mut document, decoding) = match change_encoding(bytes, &decoding, document.meta_encoding) {
        Some((text, changed)) => (parse(&text, limits), changed),
        None => (document, decoding),
    };
    document.decoding = Some(decoding);
    document
}

/// The bytes as text, and how they were read: §13.2.3.2's *encoding sniffing
/// algorithm*, before a byte is parsed — [`parse_bytes`] without its
/// §13.2.3.4.
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
    match prescan(bytes) {
        Some(Label::Unsupported(name)) => guess(bytes, Some(name)),
        Some(label) => decode_as(bytes, label).unwrap_or_else(|| guess(bytes, None)),
        None => guess(bytes, None),
    }
}

/// The bytes in an encoding something in them named, confidently; `None` for
/// one this crate does not decode.
fn decode_as(bytes: &[u8], label: Label) -> Option<(String, Decoding)> {
    let (text, decoding) = match label {
        Label::Utf8 => {
            let (text, replaced) = lossy_utf8(bytes);
            (text, decoded(DecodedAs::Utf8, true, replaced))
        }
        // Only the prescan's step 2 says UTF-16 here: a `<meta>` or a
        // declaration naming it means UTF-8 by the time it arrives.
        Label::Utf16LittleEndian => {
            let (text, replaced) = lossy_utf16(bytes, false);
            (text, decoded(DecodedAs::Utf16LittleEndian, true, replaced))
        }
        Label::Utf16BigEndian => {
            let (text, replaced) = lossy_utf16(bytes, true);
            (text, decoded(DecodedAs::Utf16BigEndian, true, replaced))
        }
        Label::SingleByte(single) => {
            let (text, replaced) = single.decode(bytes);
            (text, decoded(DecodedAs::SingleByte(single), true, replaced))
        }
        Label::Unsupported(_) => return None,
    };
    Some((text, decoding))
}

/// §13.2.3.2's last two steps, when nothing in the bytes named an encoding
/// this crate decodes: UTF-8 if they are UTF-8 (step 8's autodetection, which
/// the standard's note calls especially effective over a whole file), and
/// windows-1252 if not. `not_decoded` is the encoding they did name.
fn guess(bytes: &[u8], not_decoded: Option<&'static str>) -> (String, Decoding) {
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

/// §13.2.3.4's *change the encoding*, for the encoding the first `<meta>` the
/// tree builder met named: the bytes decoded again in it, certain, or `None`
/// to leave the first reading standing — a byte order mark's encoding is
/// already certain, UTF-16 is never changed (step 1), and an encoding equal to
/// the one in use only becomes certain (step 4). The one in use, for a guess
/// made past an encoding this crate does not decode, is that encoding: it is
/// what the standard would be reading in.
fn change_encoding(
    bytes: &[u8],
    decoding: &Decoding,
    requested: Option<Label>,
) -> Option<(String, Decoding)> {
    let requested = requested?;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return None;
    }
    let current = match (decoding.not_decoded, decoding.encoding) {
        (_, DecodedAs::Utf16LittleEndian | DecodedAs::Utf16BigEndian) => return None,
        (Some(name), _) => Label::Unsupported(name),
        (None, DecodedAs::Utf8) => Label::Utf8,
        (None, DecodedAs::SingleByte(single)) => Label::SingleByte(single),
    };
    // Steps 2 and 3.
    let new = match requested {
        Label::Utf16LittleEndian | Label::Utf16BigEndian => Label::Utf8,
        Label::Unsupported("x-user-defined") => Label::SingleByte(SingleByte::Windows1252),
        other => other,
    };
    if new == current {
        return None;
    }
    // Step 6: read again, in the new encoding, which is now certain — or, for
    // one this crate does not decode, the guess, with that encoding named.
    Some(match new {
        Label::Unsupported(name) => guess(bytes, Some(name)),
        other => decode_as(bytes, other)?,
    })
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

/// How far the prescan reads: its end condition, the first kilobyte, which
/// §13.2.3.2 encourages and the authoring rules hold a `<meta charset>` to.
const PRESCAN_WINDOW: usize = 1024;

/// §13.2.3.2's *prescan a byte stream to determine its encoding*, over the
/// first [`PRESCAN_WINDOW`] bytes.
///
/// Step 2 first: a UTF-16 `<?x`, little-endian or big, is that UTF-16. Then
/// the loop: comments skipped, other tags' attributes skipped, and the first
/// `<meta>` that names an encoding — `charset`, or `http-equiv="content-type"`
/// with a `content` holding `charset=` — wins. A `charset` attribute overrides
/// a `content` on its `<meta>` whichever comes first, and one whose label
/// names no encoding is the standard's *failure*: that `<meta>` names none,
/// whatever its `content` says. A `<meta>` naming UTF-16 means UTF-8, since an
/// ASCII-compatible `<meta>` could not have been read in UTF-16, and
/// `x-user-defined` means windows-1252.
///
/// **Running out of bytes ends the loop wherever it happens** — at the
/// window's end, or inside a comment, a tag or an attribute, so that a
/// `<meta charset=euc-jp` with no `>` names nothing — and the answer is then
/// the standard's *get an XML encoding* over the same bytes.
fn prescan(bytes: &[u8]) -> Option<Label> {
    let head = bytes
        .get(..bytes.len().min(PRESCAN_WINDOW))
        .unwrap_or(bytes);
    if head.starts_with(b"<\0?\0x\0") {
        return Some(Label::Utf16LittleEndian);
    }
    if head.starts_with(b"\0<\0?\0x") {
        return Some(Label::Utf16BigEndian);
    }
    prescan_loop(head).or_else(|| xml_encoding(head))
}

/// The prescan's steps 3 and 4: the encoding the first `<meta>` that names
/// one names, or `None` when the bytes run out — the only way the loop ends
/// without one.
fn prescan_loop(head: &[u8]) -> Option<Label> {
    let mut at = 0;
    loop {
        let rest = head.get(at..).filter(|rest| !rest.is_empty())?;
        if rest.starts_with(b"<!--") {
            // The first `>` after two dashes, which may be the opening's own.
            at += 2 + find(rest.get(2..)?, b"-->")? + 3;
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
            // §13.2.3.2's three variables, as it names them: `charset` is
            // null, failure or an encoding, and `need pragma` is null, true or
            // false.
            let mut charset: Option<Option<Label>> = None;
            let mut need_pragma: Option<bool> = None;
            let mut got_pragma = false;
            let mut seen: Vec<Vec<u8>> = Vec::new();
            while let Sniffed::Attribute(name, value, next) = prescan_attribute(head, at)? {
                at = next;
                if seen.contains(&name) {
                    continue;
                }
                seen.push(name.clone());
                match name.as_slice() {
                    b"http-equiv" => {
                        if value.eq_ignore_ascii_case(b"content-type") {
                            got_pragma = true;
                        }
                    }
                    // Only an encoding the content names, and only while
                    // `charset` is still null.
                    b"content" => {
                        if let (None, Some(found)) = (charset, charset_from_content(&value)) {
                            charset = Some(Some(found));
                            need_pragma = Some(true);
                        }
                    }
                    // Whatever came before, and failure where the label is
                    // not one: a `charset` attribute overrides a `content`.
                    b"charset" => {
                        charset = Some(std::str::from_utf8(&value).ok().and_then(encoding::lookup));
                        need_pragma = Some(false);
                    }
                    _ => {}
                }
            }
            let found = match need_pragma {
                // Nothing named an encoding.
                None => None,
                // A `content` with no `http-equiv="content-type"` beside it.
                Some(true) if !got_pragma => None,
                // An encoding, or failure, which is the next byte too.
                Some(_) => charset.flatten(),
            };
            if let Some(label) = found {
                return Some(match label {
                    Label::Utf16LittleEndian | Label::Utf16BigEndian => Label::Utf8,
                    Label::Unsupported("x-user-defined") => {
                        Label::SingleByte(SingleByte::Windows1252)
                    }
                    other => other,
                });
            }
            // The next byte: past the tag's `>`.
            at += 1;
            continue;
        }
        let letter = |at: usize| rest.get(at).is_some_and(u8::is_ascii_alphabetic);
        let tag =
            rest.first() == Some(&b'<') && (letter(1) || (rest.get(1) == Some(&b'/') && letter(2)));
        if tag {
            at += 1;
            while !matches!(head.get(at)?, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ' | b'>') {
                at += 1;
            }
            while let Sniffed::Attribute(_, _, next) = prescan_attribute(head, at)? {
                at = next;
            }
            at += 1;
            continue;
        }
        if rest.starts_with(b"<!") || rest.starts_with(b"</") || rest.starts_with(b"<?") {
            at += find(rest, b">")? + 1;
            continue;
        }
        at += 1;
    }
}

/// What §13.2.3.2's *get an attribute* found.
enum Sniffed {
    /// The name lowercased, the value, and where the next attribute starts.
    Attribute(Vec<u8>, Vec<u8>, usize),
    /// The tag's `>`: there is no attribute.
    End,
}

/// §13.2.3.2's *get an attribute*, from `at` — `None` when the bytes run out,
/// which ends the prescan.
fn prescan_attribute(bytes: &[u8], mut at: usize) -> Option<Sniffed> {
    let space = |b: u8| matches!(b, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ');
    while space(*bytes.get(at)?) || *bytes.get(at)? == b'/' {
        at += 1;
    }
    if *bytes.get(at)? == b'>' {
        return Some(Sniffed::End);
    }
    let mut name = Vec::new();
    let mut equals = false;
    loop {
        let b = *bytes.get(at)?;
        if b == b'=' && !name.is_empty() {
            at += 1;
            equals = true;
            break;
        }
        if space(b) {
            break;
        }
        if b == b'/' || b == b'>' {
            return Some(Sniffed::Attribute(name, Vec::new(), at));
        }
        name.push(b.to_ascii_lowercase());
        at += 1;
    }
    if !equals {
        // *Spaces*: a name with no `=` after it has the empty value.
        while space(*bytes.get(at)?) {
            at += 1;
        }
        if *bytes.get(at)? != b'=' {
            return Some(Sniffed::Attribute(name, Vec::new(), at));
        }
        at += 1;
    }
    // *Value*.
    while space(*bytes.get(at)?) {
        at += 1;
    }
    let mut value = Vec::new();
    match *bytes.get(at)? {
        quote @ (b'"' | b'\'') => loop {
            at += 1;
            let b = *bytes.get(at)?;
            if b == quote {
                return Some(Sniffed::Attribute(name, value, at + 1));
            }
            value.push(b.to_ascii_lowercase());
        },
        b'>' => return Some(Sniffed::Attribute(name, value, at)),
        b => {
            value.push(b.to_ascii_lowercase());
            at += 1;
        }
    }
    loop {
        let b = *bytes.get(at)?;
        if space(b) || b == b'>' {
            return Some(Sniffed::Attribute(name, value, at));
        }
        value.push(b.to_ascii_lowercase());
        at += 1;
    }
}

/// §13.2.3.2's *get an XML encoding*: the label in the `encoding` of an
/// `<?xml` the bytes begin with, read as bytes — which works because every
/// encoding the standard could name agrees with ASCII there — and UTF-16 as
/// UTF-8, as the `<meta>`'s is. `None` is the standard's failure.
fn xml_encoding(bytes: &[u8]) -> Option<Label> {
    if !bytes.starts_with(b"<?xml") {
        return None;
    }
    let end = bytes.iter().position(|&b| b == b'>')?;
    let mut at = find(bytes.get(..end)?, b"encoding")? + 8;
    while *bytes.get(at)? <= 0x20 {
        at += 1;
    }
    if *bytes.get(at)? != b'=' {
        return None;
    }
    at += 1;
    while *bytes.get(at)? <= 0x20 {
        at += 1;
    }
    let quote = *bytes.get(at)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let rest = bytes.get(at + 1..)?;
    let label = rest.get(..rest.iter().position(|&b| b == quote)?)?;
    if label.iter().any(|&b| b <= 0x20) {
        return None;
    }
    // Isomorphic decoding: a byte past ASCII is a character no label holds.
    let label: String = label.iter().copied().map(char::from).collect();
    Some(match encoding::lookup(&label)? {
        Label::Utf16LittleEndian | Label::Utf16BigEndian => Label::Utf8,
        other => other,
    })
}

/// §2.5.6's *extracting a character encoding from a meta element*: the label
/// after the first `charset` (in any case) that an `=` follows, past white
/// space; a `charset` with no `=` after it is passed over for the next.
pub(crate) fn charset_from_content(value: &[u8]) -> Option<Label> {
    let mut from = 0;
    loop {
        let at = from + find_ignoring_case(value.get(from..)?, b"charset")?;
        from = at + 7;
        let Some(rest) = value.get(from..)?.trim_ascii_start().strip_prefix(b"=") else {
            continue;
        };
        let rest = rest.trim_ascii_start();
        let label: &[u8] = match rest.first()? {
            &quote @ (b'"' | b'\'') => {
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
        return encoding::lookup(std::str::from_utf8(label).ok()?);
    }
}

fn find_ignoring_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}
