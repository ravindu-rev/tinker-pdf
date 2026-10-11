//! FictionBook 2 onto the HTML path (tier 5's FB2 row).
//!
//! An FB2 file is one XML document holding a whole book: a `<description>`
//! with its title, authors and cover, one or more `<body>` elements of nested
//! `<section>`s, and the pictures themselves as base64 in `<binary>` elements
//! at the end. Every element FictionBook 2.1's schema defines has an XHTML
//! counterpart a reading system would set it as, so the reader here is a
//! **translation**: it walks the FB2 with `tinker-pdf-xml` and writes an XHTML
//! document the EPUB reader then lays out as a book of one chapter, exactly as
//! a loose XHTML file is ([`crate::standalone`]). There is no second cascade
//! and no second painter.
//!
//! # The mapping
//!
//! Block structure becomes `<div>` and `<p>` carrying the FB2 element's name
//! as a class — `<section>` is `<div class="section">`, `<v>` is
//! `<p class="v">` — so [`STYLESHEET`], the reading system's own sheet for the
//! format, can set a title, an epigraph or a stanza the way the format means
//! it, and a book's own `<stylesheet type="text/css">` can override any of it.
//! The inline elements are HTML's: `<strong>`, `<em>` for `<emphasis>`, `<s>`
//! for `<strikethrough>`, `<sub>`, `<sup>`, `<code>`, `<span>` for `<style>`,
//! `<a href>` for `<a l:href>`. A note reference (`type="note"`) is a link to
//! the note's section, which the book's second `<body name="notes">` holds,
//! and lands on its page through the cross-reference pass every EPUB link
//! goes through.
//!
//! **Pictures are `<img src="#id">`** and the provider handed to the layout
//! answers `#id` with the decoded `<binary>` — the seam
//! [`crate::epub::read::Resources`] exists for. A `<binary>` whose base64 does
//! not decode is [`TranslationDefect::BinaryUnreadable`], and the picture that
//! names it is the `ImageNotDrawn` every unresolved picture is.
//!
//! # What is refused, by name
//!
//! - **An element the schema does not define**, or one in another namespace
//!   that borrows an FB2 name, is read as its content and counted as
//!   [`TranslationDefect::UnknownElement`]: its text reaches the page and its
//!   structure does not.
//! - **An encoding the XML declaration names is decoded when it is one of the
//!   Encoding Standard's single-byte encodings** — `windows-1251` and `koi8-r`,
//!   which a great many real FB2 files are, and their siblings
//!   (`tinker_pdf_xml::encoding::SingleByte`) — by
//!   `tinker_pdf_xml::Source::with_declared_encoding`, and a byte the
//!   declared table leaves unmapped is U+FFFD and counted as
//!   [`TranslationDefect::UnmappedByte`]. **A multi-byte encoding** — GBK,
//!   Big5, Shift_JIS — stops the translation before its first element: such
//!   a book opens as an empty page with [`crate::ArchiveWarning::Markup`]
//!   saying why.
//! - The `<description>` is metadata: `book-title` becomes `/Title` and the
//!   first `author` `/Author`, the cover is the first page's picture, and the
//!   rest — genres, dates, the annotation, `document-info` — is not set.

use std::collections::BTreeMap;

use tinker_pdf_xml::{Event, Limits as XmlLimits, Source};

use crate::epub::read::{Resources, Unavailable};
use crate::epub::Limits;
use crate::standalone::{base64_decode, TranslationDefect};

/// FictionBook 2.0's namespace, which 2.1 kept.
pub const FB2_NAMESPACE: &str = "http://www.gribuser.ru/xml/fictionbook/2.0";

/// The reading system's sheet for FB2: how a format with no presentation of
/// its own is set.
///
/// Applied as an author sheet ahead of the book's own `<stylesheet>`, so a
/// book that styles its titles wins every tie. The choices are the ones FB2
/// readers have converged on — a centred bold title, an epigraph set narrow
/// and to the right, verse indented, a note reference raised — and are a
/// reading system's opinion, which is what a sheet is for.
pub const STYLESHEET: &str = "\
.title { margin: 1em 0 0.5em 0; text-align: center; font-weight: bold } \
.body > .title { font-size: 1.5em } \
.body > .section > .title { font-size: 1.3em } \
.title p { margin: 0 } \
.subtitle { text-align: center; font-weight: bold; margin: 1em 0 } \
.epigraph { margin: 0.5em 0 0.5em 40%; font-style: italic } \
.text-author { text-align: right; font-style: italic } \
.poem { margin: 1em 0 1em 2em } \
.stanza { margin: 0.5em 0 } \
.v { margin: 0 } \
.cite { margin: 1em 2em } \
.empty-line { margin: 0; height: 1em } \
.image { text-align: center; margin: 1em 0 } \
.coverpage { text-align: center } \
p { margin: 0; text-indent: 1.5em } \
.title p, .subtitle, .v, .text-author, .image { text-indent: 0 } \
a.note { font-size: 0.75em } \
";

/// One FB2 document, translated.
pub(crate) struct Translated {
    /// The XHTML document the EPUB reader lays out. Its `<title>` is the
    /// `<book-title>`, which is how that reaches `/Title`.
    pub xhtml: String,
    /// The first `<author>`, for `/Author`.
    pub author: Option<String>,
    /// The decoded `<binary>` elements, by `id`.
    pub binaries: Binaries,
    /// What the translation had to do, with counts.
    pub defects: Vec<(TranslationDefect, usize)>,
}

/// The pictures an FB2 carries in itself, answered by `#id`.
///
/// Keyed, so a book of many pictures each named once is not a search of every
/// binary per picture; the first of two binaries sharing an `id` (which XML
/// forbids and a damaged file has) is the one answered.
#[derive(Default)]
pub(crate) struct Binaries {
    entries: BTreeMap<String, Vec<u8>>,
}

impl Resources for Binaries {
    fn fetch(
        &mut self,
        _referring: &str,
        reference: &str,
        _limits: &Limits,
    ) -> Result<(String, Vec<u8>), Unavailable> {
        let id = reference
            .trim()
            .strip_prefix('#')
            .ok_or(Unavailable::Missing)?;
        self.entries
            .get(id)
            .map(|bytes| (reference.trim().to_owned(), bytes.clone()))
            .ok_or(Unavailable::Missing)
    }
}

/// What an FB2 element becomes.
enum Becomes {
    /// An XHTML element with a class: the FB2 name, kept for the sheet.
    Block(&'static str, &'static str),
    /// An XHTML inline element.
    Inline(&'static str),
    /// Nothing of its own; its content is written.
    Transparent,
    /// Nothing at all, content included.
    Dropped,
}

fn becomes(local: &str) -> Option<Becomes> {
    Some(match local {
        "body" => Becomes::Block("div", "body"),
        "section" => Becomes::Block("div", "section"),
        "title" => Becomes::Block("div", "title"),
        "epigraph" => Becomes::Block("div", "epigraph"),
        "annotation" => Becomes::Block("div", "annotation"),
        "cite" => Becomes::Block("blockquote", "cite"),
        "poem" => Becomes::Block("div", "poem"),
        "stanza" => Becomes::Block("div", "stanza"),
        "p" => Becomes::Block("p", ""),
        "v" => Becomes::Block("p", "v"),
        "subtitle" => Becomes::Block("p", "subtitle"),
        "text-author" => Becomes::Block("p", "text-author"),
        "date" => Becomes::Block("p", "date"),
        "table" => Becomes::Block("table", ""),
        "tr" => Becomes::Block("tr", ""),
        "td" => Becomes::Block("td", ""),
        "th" => Becomes::Block("th", ""),
        "strong" => Becomes::Inline("strong"),
        "emphasis" => Becomes::Inline("em"),
        "strikethrough" => Becomes::Inline("s"),
        "sub" => Becomes::Inline("sub"),
        "sup" => Becomes::Inline("sup"),
        "code" => Becomes::Inline("code"),
        "style" => Becomes::Inline("span"),
        "FictionBook" => Becomes::Transparent,
        // Metadata, read separately; its text is not the book's.
        "description" | "stylesheet" | "binary" => Becomes::Dropped,
        _ => return None,
    })
}

fn escape(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

/// An attribute's value by local name, in no namespace or in any — FB2 writes
/// `l:href`, `xlink:href` and occasionally a bare `href` for one attribute.
fn attribute<'a>(element: &'a tinker_pdf_xml::Element<'_>, local: &str) -> Option<&'a str> {
    element
        .attributes()
        .iter()
        .find(|attribute| attribute.local() == local)
        .map(|attribute| attribute.value())
}

#[derive(Default)]
struct Counts {
    counts: Vec<(TranslationDefect, usize)>,
}

impl Counts {
    fn note(&mut self, defect: TranslationDefect) {
        match self.counts.iter_mut().find(|(seen, _)| *seen == defect) {
            Some(slot) => slot.1 += 1,
            None => self.counts.push((defect, 1)),
        }
    }
}

/// What is being read: the book's own content, the description's fields, a
/// binary's text, a stylesheet's text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Book,
    Description,
    Binary,
    Stylesheet,
}

/// Reads an FB2 document into an XHTML one.
///
/// # Errors
/// `tinker_pdf_xml`'s refusal to begin: an encoding it does not decode, or a
/// character XML 1.0 §2.2 forbids. A document that stops part way is
/// translated as far as it read; the XHTML is then well-formed with every open
/// element closed, and the stop is the caller's to report.
pub(crate) fn translate(
    bytes: &[u8],
    limits: &XmlLimits,
) -> Result<(Translated, bool), tinker_pdf_xml::Error> {
    let source = Source::with_declared_encoding(bytes)?;
    let mut reader = source.reader(limits);
    let mut counts = Counts::default();
    // A single-byte table holds no U+FFFD, so every one in the decoded text is
    // a byte the declared encoding left unmapped.
    if let tinker_pdf_xml::Encoding::SingleByte(_) = source.encoding() {
        let unmapped = source.text().matches('\u{FFFD}').count();
        if unmapped > 0 {
            counts
                .counts
                .push((TranslationDefect::UnmappedByte, unmapped));
        }
    }
    let mut body = String::new();
    // What each open FB2 element wrote, so its end tag closes the right thing.
    let mut open: Vec<Option<&'static str>> = Vec::new();
    let mut mode = Mode::Book;
    // How deep inside the dropped element that set `mode` the reader is.
    let mut dropped_depth = 0usize;
    let mut title_info = 0usize;
    let mut path: Vec<String> = Vec::new();
    let mut title = String::new();
    let mut author = String::new();
    let mut author_done = false;
    let mut covers: Vec<String> = Vec::new();
    let mut binaries = Binaries::default();
    let mut binary_id = String::new();
    let mut binary_text = String::new();
    let mut stylesheet = String::new();
    let mut truncated = false;

    for event in &mut reader {
        let event = match event {
            Ok(event) => event,
            Err(_) => {
                truncated = true;
                break;
            }
        };
        match event {
            Event::Start(element) => {
                let local = element.local();
                let in_fb2 = element.namespace().is_none_or(|ns| ns == FB2_NAMESPACE);
                if mode != Mode::Book {
                    dropped_depth += 1;
                    path.push(local.to_owned());
                    if mode == Mode::Description {
                        if local == "title-info" {
                            title_info += 1;
                        }
                        let inside_title_info = title_info > 0;
                        if inside_title_info && local == "image" {
                            if let Some(href) = attribute(&element, "href") {
                                covers.push(href.to_owned());
                            }
                        }
                    }
                    continue;
                }
                match (in_fb2, becomes(local)) {
                    (true, Some(Becomes::Dropped)) => {
                        mode = match local {
                            "description" => Mode::Description,
                            "binary" => {
                                binary_id = attribute(&element, "id").unwrap_or("").to_owned();
                                binary_text.clear();
                                Mode::Binary
                            }
                            "stylesheet" => Mode::Stylesheet,
                            _ => Mode::Description,
                        };
                        dropped_depth = 1;
                        path.clear();
                        path.push(local.to_owned());
                        open.push(None);
                    }
                    (true, Some(Becomes::Transparent)) => open.push(None),
                    (true, Some(Becomes::Block(tag, class))) => {
                        body.push('<');
                        body.push_str(tag);
                        let mut classes = class.to_owned();
                        if local == "body" && attribute(&element, "name") == Some("notes") {
                            classes.push_str(" notes");
                        }
                        if !classes.is_empty() {
                            body.push_str(" class=\"");
                            escape(&mut body, &classes);
                            body.push('"');
                        }
                        if let Some(id) = attribute(&element, "id") {
                            body.push_str(" id=\"");
                            escape(&mut body, id);
                            body.push('"');
                        }
                        for span in ["colspan", "rowspan"] {
                            if let Some(value) = attribute(&element, span) {
                                body.push(' ');
                                body.push_str(span);
                                body.push_str("=\"");
                                escape(&mut body, value);
                                body.push('"');
                            }
                        }
                        body.push('>');
                        open.push(Some(tag));
                    }
                    (true, Some(Becomes::Inline(tag))) => {
                        body.push('<');
                        body.push_str(tag);
                        body.push('>');
                        open.push(Some(tag));
                    }
                    (true, None) if local == "a" => {
                        body.push_str("<a");
                        if let Some(href) = attribute(&element, "href") {
                            body.push_str(" href=\"");
                            escape(&mut body, href);
                            body.push('"');
                        }
                        if attribute(&element, "type") == Some("note") {
                            body.push_str(" class=\"note\"");
                        }
                        body.push('>');
                        open.push(Some("a"));
                    }
                    (true, None) if local == "image" => {
                        // A block picture is a `<div class="image">` round an
                        // `<img>`; one inside a paragraph is the `<img>` alone.
                        let inline = matches!(
                            open.last(),
                            Some(Some(
                                "p" | "strong"
                                    | "em"
                                    | "s"
                                    | "sub"
                                    | "sup"
                                    | "code"
                                    | "span"
                                    | "a"
                                    | "td"
                                    | "th"
                            ))
                        );
                        if !inline {
                            body.push_str("<div class=\"image\">");
                        }
                        body.push_str("<img src=\"");
                        escape(&mut body, attribute(&element, "href").unwrap_or(""));
                        body.push_str("\" alt=\"");
                        escape(&mut body, attribute(&element, "alt").unwrap_or(""));
                        body.push_str("\"/>");
                        if !inline {
                            body.push_str("</div>");
                        }
                        open.push(None);
                    }
                    (true, None) if local == "empty-line" => {
                        body.push_str("<p class=\"empty-line\">\u{A0}</p>");
                        open.push(None);
                    }
                    _ => {
                        counts.note(TranslationDefect::UnknownElement);
                        open.push(None);
                    }
                }
            }
            Event::End(_) => {
                if mode != Mode::Book {
                    dropped_depth = dropped_depth.saturating_sub(1);
                    if mode == Mode::Description && path.last().is_some_and(|l| l == "title-info") {
                        title_info = title_info.saturating_sub(1);
                    }
                    if mode == Mode::Description
                        && path.last().is_some_and(|l| l == "author")
                        && title_info > 0
                        && !author.trim().is_empty()
                    {
                        author_done = true;
                    }
                    path.pop();
                    if dropped_depth == 0 {
                        if mode == Mode::Binary {
                            match base64_decode(binary_text.as_bytes()) {
                                Some(bytes) => {
                                    binaries
                                        .entries
                                        .entry(std::mem::take(&mut binary_id))
                                        .or_insert(bytes);
                                }
                                None => counts.note(TranslationDefect::BinaryUnreadable),
                            }
                            binary_text.clear();
                        }
                        mode = Mode::Book;
                        open.pop();
                    }
                    continue;
                }
                if let Some(Some(tag)) = open.pop() {
                    body.push_str("</");
                    body.push_str(tag);
                    body.push('>');
                }
            }
            Event::Text(text) | Event::Cdata(text) => match mode {
                Mode::Book => escape(&mut body, &text),
                Mode::Binary => binary_text.push_str(&text),
                Mode::Stylesheet => stylesheet.push_str(&text),
                Mode::Description => {
                    let inside = |name: &str| path.iter().any(|p| p == name);
                    if title_info > 0 && path.last().is_some_and(|l| l == "book-title") {
                        title.push_str(&text);
                    } else if title_info > 0
                        && !author_done
                        && inside("author")
                        && path.last().is_some_and(|l| {
                            matches!(
                                l.as_str(),
                                "first-name" | "middle-name" | "last-name" | "nickname"
                            )
                        })
                    {
                        if !author.is_empty() {
                            author.push(' ');
                        }
                        author.push_str(text.trim());
                    }
                }
            },
            Event::Comment(_) | Event::Instruction { .. } => {}
        }
    }
    // A document that stopped part way closes what it opened, so the XHTML is
    // well-formed and the reader's own defect is the only one.
    while let Some(entry) = open.pop() {
        if let Some(tag) = entry {
            body.push_str("</");
            body.push_str(tag);
            body.push('>');
        }
    }

    let title = collapse(&title);
    let author = collapse(&author);
    let mut xhtml = String::with_capacity(body.len() + STYLESHEET.len() + 512);
    xhtml.push_str(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>",
    );
    escape(&mut xhtml, title.as_deref().unwrap_or(""));
    xhtml.push_str("</title>");
    if !stylesheet.trim().is_empty() {
        xhtml.push_str("<style>");
        escape(&mut xhtml, &stylesheet);
        xhtml.push_str("</style>");
    }
    xhtml.push_str("</head><body>");
    for cover in &covers {
        xhtml.push_str("<div class=\"coverpage\"><img src=\"");
        escape(&mut xhtml, cover);
        xhtml.push_str("\" alt=\"\"/></div>");
    }
    xhtml.push_str(&body);
    xhtml.push_str("</body></html>\n");
    Ok((
        Translated {
            xhtml,
            author,
            binaries,
            defects: counts.counts,
        },
        truncated,
    ))
}

/// An FB2 document as the XHTML document the EPUB reader lays out, or `None`
/// when the XML reader cannot begin it (an encoding other than UTF-8 or
/// UTF-16, or a character XML 1.0 §2.2 forbids).
///
/// The translation [`crate::Document::open`] makes of a sniffed FB2, without
/// its pictures: those are `<img src="#id">` here and answered from the
/// document's own `<binary>` elements when it is laid out.
#[must_use]
pub fn to_xhtml(bytes: &[u8]) -> Option<String> {
    translate(bytes, &XmlLimits::DEFAULT)
        .ok()
        .map(|(translated, _)| translated.xhtml)
}

fn collapse(text: &str) -> Option<String> {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!joined.is_empty()).then_some(joined)
}
