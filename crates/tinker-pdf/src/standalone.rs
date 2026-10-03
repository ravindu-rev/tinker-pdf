//! A document that is one file and not a container: a standalone SVG, a bare
//! image, a loose XHTML file (tier 5's formats row).
//!
//! Every reader these need was already in the tree — `tinker-pdf-svg` and
//! `epub::svg` for an SVG, the comic path for a picture, the EPUB cascade,
//! layout and painter for a content document — and each of the three was
//! refused as not-a-PDF because nothing
//! asked whether the bytes were one of them. This module is that question and
//! the routing behind it. It adds no reader of its own: an SVG is a book of one
//! pre-paginated chapter, a loose XHTML file is a book of one reflowable
//! chapter, and a bare image is the comic of its one picture, each built by the
//! code that builds the larger document so the two cannot disagree. For a bare
//! image that is literal: it is paged by `cbz`'s own body, with no archive
//! around its one entry, and `tests/standalone.rs` holds it equal to a
//! one-entry CBZ in every format the comic path reads.
//!
//! # The sniff, and why a PDF always wins it
//!
//! [`sniff`] answers `None` for anything carrying `%PDF-` in its first
//! [`tinker_pdf_cos::limits::MAX_HEADER_SCAN`] bytes, which is exactly where
//! the COS parser looks for a header and opens what follows it as the PDF — so
//! a PDF with junk in front of it, and a polyglot that is a PDF and something
//! else, stay PDFs wherever the parser would have read them as one. (7.5.2
//! puts the header first and names no window; 1 024 bytes is Acrobat's
//! implementation note, and the parser's 4 096 is the leniency this build
//! already extends, so it is the one a sniff in front of it must not undercut.)
//! Past that, an image is told by its magic at offset zero, exactly as
//! [`cbz::image_format`] tells a page, and a markup document by the name of its
//! root element once the prolog is skipped: the byte-order mark, white space,
//! the XML declaration and any processing instruction, comments, and the
//! document type declaration with its internal subset — in UTF-8, or in the
//! UTF-16 `tinker-pdf-xml` decodes (see `narrowed`). The window is
//! [`SNIFF_WINDOW`] bytes and nothing past it is read, so an SVG whose licence
//! comment runs longer than that is not recognised — named in
//! `docs/features/opening.md` rather than searched for, because a sniff that
//! scans is one that finds `<svg` inside a PDF's stream.
//!
//! A BMP is not sniffed. Its signature is two bytes, `BM`, which is also how a
//! text file about a car starts, and [`cbz::image_format`] can afford it only
//! because a comic's entries are already known to be pictures.
//!
//! # What each one becomes
//!
//! - **An SVG** is laid out through [`crate::epub::lay_out_one`] as a
//!   pre-paginated chapter: one page, the size its root states, with the
//!   caller's page box as the viewport a root with no size of its own fills.
//! - **An XHTML or HTML file** is read as XML into the EPUB reader's tree and
//!   laid out as a reflowable chapter at the caller's page box, with
//!   [`crate::epub::PAGE_MARGIN`] inside it. Its `<title>` is the document's
//!   `/Title`. **HTML that does not parse as XML is read as far as it parses**
//!   and [`crate::ArchiveWarning::Markup`] says it stopped: this build has no
//!   HTML5 tree builder, so tag soup is the narrowed half of the roadmap row.
//! - **A bare image** is the comic of that one picture, paged by
//!   [`crate::cbz`]'s own body (`cbz::pages_from_picture`) and not by a copy of
//!   it: one image pixel to one point (8.9.5.2), a JPEG, PNG, TIFF, JPEG 2000,
//!   GIF or WebP drawn, a multi-page TIFF one page per directory that is a page,
//!   and a format recognised and not decoded — AVIF — or bytes that will not
//!   decode one placeholder page naming why, exactly as a one-entry comic
//!   archive holding it.
//! - **An FB2** — a root named `FictionBook`, since tier 5's FB2 row — is
//!   translated by [`crate::fb2`] into an XHTML document and laid out as a
//!   loose XHTML file is, with [`crate::fb2::STYLESHEET`] ahead of the book's
//!   own sheet and its pictures answered from its own `<binary>` elements.
//!   `Document::open` also takes the `.fb2.zip` it is shipped as: a ZIP of one
//!   file whose bytes sniff as FB2.
//!
//! # References, and the one kind a loose file can resolve
//!
//! A file opened from its bytes alone has nothing beside it, so a stylesheet,
//! a picture or a face it names by a relative reference is missing, and each
//! is named as missing by the warning that already exists for it. The
//! exception is RFC 2397's `data:` URL, which carries its own bytes and is how
//! a self-contained HTML file embeds a picture: [`DataUrls`] answers those,
//! and hands everything else to whatever stands behind it.

use std::borrow::Cow;

use tinker_pdf_cos::DocumentBuilder;

use crate::cbz::{image_format, ArchiveRefusal, ArchiveReport, ArchiveWarning, ImageFormat};
use crate::epub::read::{Resources, Unavailable};
use crate::epub::{self, BookLayout, Loose};

/// How many bytes [`sniff`] looks at.
///
/// Four kilobytes — the chunk a streamed open reads anyway
/// ([`crate::CHUNK_SIZE`]) — because the root element of a markup document
/// comes after its prolog, and an XML declaration, a generator's comment and a
/// doctype with a small internal subset fit in it. It is never less than the
/// window a PDF header is looked for in, the COS parser's
/// [`tinker_pdf_cos::limits::MAX_HEADER_SCAN`], which a streamed open relies
/// on: the window it reads for this sniff is the one that header is looked for
/// in.
pub const SNIFF_WINDOW: usize = 4_096;

/// Where a PDF header is looked for before anything else is: the COS parser's
/// own [`tinker_pdf_cos::limits::MAX_HEADER_SCAN`], because a header it would
/// find is a PDF it would open, and a sniff that looked in fewer bytes turned
/// a PDF behind junk that began like a picture or a markup document into a
/// synthesised one.
const PDF_HEADER_WINDOW: usize = tinker_pdf_cos::limits::MAX_HEADER_SCAN;

// The streamed open reads `SNIFF_WINDOW` bytes for this sniff, so a header past
// them would go unseen streamed and be seen buffered.
const _: () = assert!(SNIFF_WINDOW >= PDF_HEADER_WINDOW);

/// What translating a document written in another language into the EPUB
/// reader's tree had to do (tier 5's Markdown and FB2 rows), reported as
/// [`ArchiveWarning::Translation`] with a count.
///
/// One vocabulary for both translators, because they answer the same
/// question — *what of the source did not arrive as itself* — and a host
/// that reads one reads the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TranslationDefect {
    /// Markdown bytes that are not UTF-8, each malformed sequence read as
    /// U+FFFD.
    NotUtf8,
    /// Raw HTML in Markdown, set as the text it is rather than passed through:
    /// a tag that is not well-formed XML would stop the reader and lose the
    /// rest of the document. Counted per tag or block.
    RawHtmlAsText,
    /// A Markdown block quote or list item that would have opened past
    /// [`crate::markdown::MAX_MARKDOWN_NESTING`], read as text instead; or an
    /// emphasis, strong emphasis or link that would have nested its element
    /// past the 202 elements that cap bounds a document to, set without the
    /// element and with its text, so that the XML reader is never stopped by
    /// depth and nothing after the nest is lost.
    NestingTooDeep,
    /// A Markdown reference link read as the text it is written as, because
    /// the document's references had already copied
    /// [`crate::markdown::MAX_MARKDOWN_REFERENCE_BYTES`] — or the document's
    /// own length, if larger — out of their definitions.
    ReferenceBudgetSpent,
    /// An FB2 element FictionBook 2.1's schema does not define, or one in
    /// another namespace, read as its content: its text reaches the page and
    /// its structure does not.
    UnknownElement,
    /// An FB2 `<binary>` whose base64 would not decode, so the picture that
    /// names it is not drawn.
    BinaryUnreadable,
    /// Bytes of an FB2 the single-byte encoding its declaration names leaves
    /// unmapped, each read as U+FFFD — the Encoding Standard's own
    /// *replacement* error mode. Counted per byte.
    UnmappedByte,
}

/// What a one-file document turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Standalone {
    /// An SVG document: the root element's local name is `svg`.
    Svg,
    /// An XHTML or HTML document: the root element is `html`, in any case, or
    /// the document type declaration names `html`.
    Html,
    /// A FictionBook 2 document: the root element is `FictionBook`.
    Fb2,
    /// An image, by its magic at offset zero.
    Image(ImageFormat),
}

/// Whether the bytes are a one-file document this build opens, and which.
///
/// Reads at most [`SNIFF_WINDOW`] bytes, and answers `None` for anything with
/// `%PDF-` where the COS parser looks for a header — see the module comment
/// for why a PDF always wins.
#[must_use]
pub fn sniff(bytes: &[u8]) -> Option<Standalone> {
    let head = bytes.get(..bytes.len().min(SNIFF_WINDOW)).unwrap_or(bytes);
    let pdf_window = head
        .get(..head.len().min(PDF_HEADER_WINDOW))
        .unwrap_or(head);
    if pdf_window.windows(5).any(|w| w == b"%PDF-") {
        return None;
    }
    match image_format(head) {
        // Two bytes of signature; see the module comment.
        Some(ImageFormat::Bmp) => {}
        Some(format) => return Some(Standalone::Image(format)),
        None => {}
    }
    let narrowed = narrowed(head);
    let (doctype, root) = prolog(&narrowed);
    let root = root.map(|name| match name.iter().rposition(|&b| b == b':') {
        Some(colon) => name.get(colon + 1..).unwrap_or_default(),
        None => name,
    });
    match root {
        Some(b"svg") => Some(Standalone::Svg),
        Some(name) if name.eq_ignore_ascii_case(b"html") => Some(Standalone::Html),
        Some(b"FictionBook") => Some(Standalone::Fb2),
        // HTML lets a document leave out its `<html>`, and says what it is in
        // its doctype instead. The reader will stop where the markup stops being
        // XML, and the report will say so.
        _ if doctype.is_some_and(|name| name.eq_ignore_ascii_case(b"html")) => {
            Some(Standalone::Html)
        }
        _ => None,
    }
}

/// The window as the bytes [`prolog`] walks: UTF-8 as it is, and UTF-16 one
/// byte per code unit.
///
/// UTF-16 is recognised exactly as `tinker-pdf-xml` recognises it — either byte
/// order mark, or Appendix F's unmarked `3C 00` / `00 3C` shape — because
/// every reader behind this sniff is built on that crate and decodes it; a
/// sniff that walked only UTF-8 sent a UTF-16 SVG, XHTML file or FB2 to the
/// PDF parser and `NotAPdf`. A unit past ASCII becomes `0x80`, a byte no name
/// this sniff compares against contains, which is where a UTF-8 walk stops a
/// name too. A UTF-32 mark is left alone: that crate refuses UTF-32 by name, so
/// nothing behind this sniff could read one.
fn narrowed(head: &[u8]) -> Cow<'_, [u8]> {
    if head.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) || head.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return Cow::Borrowed(head);
    }
    let (units, big_endian) = if let Some(rest) = head.strip_prefix(&[0xFF, 0xFE]) {
        (rest, false)
    } else if let Some(rest) = head.strip_prefix(&[0xFE, 0xFF]) {
        (rest, true)
    } else {
        match head.get(..2) {
            Some([0x3C, 0x00]) => (head, false),
            Some([0x00, 0x3C]) => (head, true),
            _ => return Cow::Borrowed(head),
        }
    };
    Cow::Owned(
        units
            .chunks_exact(2)
            .map(|pair| {
                let unit = match pair {
                    [a, b] if big_endian => u16::from_be_bytes([*a, *b]),
                    [a, b] => u16::from_le_bytes([*a, *b]),
                    _ => 0x80,
                };
                u8::try_from(unit).ok().filter(u8::is_ascii).unwrap_or(0x80)
            })
            .collect(),
    )
}

/// The document type declaration's name and the root element's qualified
/// name, as far as the window shows them.
///
/// XML 1.0 §2.8's prolog: an optional declaration, then any number of
/// comments, processing instructions and white space, with at most one
/// document type declaration among them. Nothing is validated — a construct
/// that does not end inside the window ends the walk — and nothing is decoded
/// here: [`narrowed`] has already made UTF-16 the bytes this reads.
fn prolog(head: &[u8]) -> (Option<&[u8]>, Option<&[u8]>) {
    let mut at = if head.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };
    let mut doctype = None;
    loop {
        while head
            .get(at)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        {
            at += 1;
        }
        let rest = head.get(at..).unwrap_or_default();
        if rest.starts_with(b"<?") {
            let Some(end) = find(rest, b"?>") else {
                return (doctype, None);
            };
            at += end + 2;
        } else if rest.starts_with(b"<!--") {
            let Some(end) = find(rest.get(4..).unwrap_or_default(), b"-->") else {
                return (doctype, None);
            };
            at += 4 + end + 3;
        } else if rest
            .get(..9)
            .is_some_and(|open| open.eq_ignore_ascii_case(b"<!DOCTYPE"))
        {
            let name_at = rest
                .iter()
                .skip(9)
                .position(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
                .map_or(rest.len(), |p| p + 9);
            doctype = Some(name(rest.get(name_at..).unwrap_or_default()));
            let Some(end) = doctype_end(rest) else {
                return (doctype, None);
            };
            at += end + 1;
        } else if rest.first() == Some(&b'<') {
            let root = name(rest.get(1..).unwrap_or_default());
            return (doctype, (!root.is_empty()).then_some(root));
        } else {
            return (doctype, None);
        }
    }
}

/// The leading run of name characters: XML §2.3's `NameChar`, restricted to
/// ASCII, which is every name this sniff compares against.
fn name(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .position(|&b| !(b.is_ascii_alphanumeric() || matches!(b, b':' | b'_' | b'-' | b'.')))
        .unwrap_or(bytes.len());
    bytes.get(..end).unwrap_or_default()
}

/// Where a document type declaration's closing `>` is, past any internal
/// subset in `[` `]` and any quoted literal, or `None` when it does not close
/// inside the window.
fn doctype_end(rest: &[u8]) -> Option<usize> {
    let mut quote: Option<u8> = None;
    let mut subset = false;
    for (at, &b) in rest.iter().enumerate() {
        match (quote, b) {
            (Some(q), _) if b == q => quote = None,
            (Some(_), _) => {}
            (None, b'"' | b'\'') => quote = Some(b),
            (None, b'[') => subset = true,
            (None, b']') => subset = false,
            (None, b'>') if !subset => return Some(at),
            _ => {}
        }
    }
    None
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Opens one sniffed document as a synthesised PDF and its report.
///
/// `layout` reaches the two markup kinds only: a bare image's page is its own
/// pixels, which is the comic path's rule and is not the caller's number.
///
/// # Errors
/// [`ArchiveRefusal::TooLarge`] when the synthesised document is past
/// [`crate::cbz::MAX_SYNTHESISED_PDF`]. Nothing else refuses: a document this
/// build recognises and cannot read is a page saying so.
pub(crate) fn synthesise(
    kind: Standalone,
    bytes: &[u8],
    layout: &BookLayout,
) -> Result<(Vec<u8>, ArchiveReport), ArchiveRefusal> {
    let limits = epub::Limits::DEFAULT;
    match kind {
        // The comic path's own body, with no archive around its one entry. Its
        // one refusal that is not a bound, `NoImages`, is for bytes
        // `image_format` does not recognise, and the sniff said it did.
        Standalone::Image(_) => crate::cbz::pages_from_picture(bytes, &crate::cbz::Limits::DEFAULT),
        Standalone::Svg => laid_out(
            Loose::Svg(bytes),
            Vec::new(),
            &mut DataUrls(epub::read::NoResources),
            Page::plain(layout),
        ),
        Standalone::Fb2 => fb2(bytes, layout),
        Standalone::Html => laid_out(
            Loose::Markup(epub::read::markup(bytes, &limits.xml)),
            Vec::new(),
            &mut DataUrls(epub::read::NoResources),
            Page::plain(layout),
        ),
    }
}

/// A Markdown document as a synthesised PDF (tier 5's Markdown row): the
/// bytes read as UTF-8, translated by [`crate::markdown`] into an XHTML
/// document, and laid out as a loose XHTML file is.
///
/// # Errors
/// [`synthesise`]'s.
pub(crate) fn markdown(
    bytes: &[u8],
    layout: &BookLayout,
) -> Result<(Vec<u8>, ArchiveReport), ArchiveRefusal> {
    let limits = epub::Limits::DEFAULT;
    let (text, malformed) = lossy_utf8(bytes);
    let (xhtml, defects) = crate::markdown::to_xhtml(&text);
    let mut before = Vec::new();
    if malformed > 0 {
        before.push(ArchiveWarning::Translation {
            item: String::new(),
            defect: TranslationDefect::NotUtf8,
            count: malformed,
        });
    }
    for (defect, count) in defects {
        before.push(ArchiveWarning::Translation {
            item: String::new(),
            defect,
            count,
        });
    }
    laid_out(
        Loose::Markup(epub::read::markup(xhtml.as_bytes(), &limits.xml)),
        before,
        &mut DataUrls(epub::read::NoResources),
        Page::plain(layout),
    )
}

/// A FictionBook 2 document as a synthesised PDF (tier 5's FB2 row):
/// translated by [`crate::fb2`] into an XHTML document whose pictures are its
/// own `<binary>` elements, and laid out with the format's reading-system sheet
/// ahead of the book's own.
fn fb2(bytes: &[u8], layout: &BookLayout) -> Result<(Vec<u8>, ArchiveReport), ArchiveRefusal> {
    let limits = epub::Limits::DEFAULT;
    let mut before = Vec::new();
    let translated = match crate::fb2::translate(bytes, &limits.xml) {
        Ok((translated, stopped)) => {
            if stopped {
                before.push(ArchiveWarning::Markup {
                    item: String::new(),
                    defect: epub::xhtml::MarkupDefect::Truncated,
                });
            }
            Some(translated)
        }
        // An encoding the XML reader does not decode, or a character it may
        // not read: no tree at all, which a loose XHTML file says the same way.
        Err(_) => {
            before.push(ArchiveWarning::Markup {
                item: String::new(),
                defect: epub::xhtml::MarkupDefect::Truncated,
            });
            None
        }
    };
    let Some(translated) = translated else {
        return laid_out(
            Loose::Markup(epub::xhtml::Dom::default()),
            before,
            &mut DataUrls(epub::read::NoResources),
            Page::plain(layout),
        );
    };
    for (defect, count) in &translated.defects {
        before.push(ArchiveWarning::Translation {
            item: String::new(),
            defect: *defect,
            count: *count,
        });
    }
    let dom = epub::read::markup(translated.xhtml.as_bytes(), &limits.xml);
    let mut binaries = translated.binaries;
    laid_out(
        Loose::Markup(dom),
        before,
        &mut DataUrls(&mut binaries),
        Page {
            layout,
            sheet: crate::fb2::STYLESHEET,
            author: translated.author.as_deref(),
        },
    )
}

/// The bytes as UTF-8, each malformed sequence read as U+FFFD, and how many
/// there were.
pub(crate) fn lossy_utf8(bytes: &[u8]) -> (String, usize) {
    let mut text = String::with_capacity(bytes.len());
    let mut malformed = 0;
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            text.push('\u{FFFD}');
            malformed += 1;
        }
    }
    (text, malformed)
}

/// What a loose document is laid out with besides its own markup.
#[derive(Clone, Copy)]
struct Page<'a> {
    /// The page box and base font size.
    layout: &'a BookLayout,
    /// A reading system's sheet for the format, ahead of the document's own.
    sheet: &'a str,
    /// `/Author`, where the format names one outside its markup.
    author: Option<&'a str>,
}

impl<'a> Page<'a> {
    fn plain(layout: &'a BookLayout) -> Page<'a> {
        Page {
            layout,
            sheet: "",
            author: None,
        }
    }
}

/// One loose content document laid out as a book of one chapter, written and
/// reported; `before` is what was tolerated on the way to the tree.
fn laid_out<R: Resources>(
    content: Loose<'_>,
    before: Vec<ArchiveWarning>,
    resources: &mut R,
    page: Page<'_>,
) -> Result<(Vec<u8>, ArchiveReport), ArchiveRefusal> {
    let Page {
        layout,
        sheet,
        author,
    } = page;
    let limits = epub::Limits::DEFAULT;
    let mut builder = DocumentBuilder::new();
    if let Loose::Markup(dom) = &content {
        if let Some(title) = dom.title() {
            builder.set_info(b"Title", &title);
        }
    }
    if let Some(author) = author {
        builder.set_info(b"Author", author);
    }
    let laid = epub::lay_out_one(
        resources,
        &mut builder,
        "",
        content,
        sheet,
        epub::PAGE_MARGIN,
        &limits,
        layout,
    );
    let pdf = builder.finish();
    if pdf.len() > limits.max_synthesised {
        return Err(ArchiveRefusal::TooLarge);
    }
    let synthesised_bytes = pdf.len();
    let mut warnings = before;
    warnings.extend(laid.warnings);
    Ok((
        pdf,
        ArchiveReport::book(warnings, laid.pages, synthesised_bytes, *layout, laid.cost),
    ))
}

// ---- data: URLs --------------------------------------------------------------

/// RFC 2397's `data:` URLs, answered from their own bytes; every other
/// reference handed to the provider behind.
///
/// The path a `data:` URL resolves to is the URL itself, because two of them
/// are one resource exactly when they are one string — which is what
/// [`crate::epub::typeface::load`] deduplicates faces on — and because nothing
/// a `data:` resource refers to can be resolved against it: a relative
/// reference has no base inside one, and the provider behind refuses a base
/// with a scheme.
pub struct DataUrls<R>(pub R);

impl<R: Resources> Resources for DataUrls<R> {
    fn fetch(
        &mut self,
        referring: &str,
        reference: &str,
        limits: &epub::Limits,
    ) -> Result<(String, Vec<u8>), Unavailable> {
        let trimmed = reference.trim();
        match trimmed.get(..5) {
            Some(scheme) if scheme.eq_ignore_ascii_case("data:") => data_url(trimmed)
                .map(|bytes| (trimmed.to_owned(), bytes))
                .ok_or(Unavailable::Unreadable),
            _ => self.0.fetch(referring, reference, limits),
        }
    }
}

/// The bytes a `data:` URL carries, or `None` when it is not one RFC 2397's
/// grammar admits.
///
/// `data:[<mediatype>][;base64],<data>`: everything before the first comma is
/// the media type and its parameters, and a final `;base64` parameter says the
/// data is RFC 4648 base64. The data is percent-decoded first in both cases,
/// because RFC 2397 §2 writes it as URL characters and a `+` or `/` escaped as
/// `%2B` or `%2F` is still that character.
#[must_use]
pub fn data_url(url: &str) -> Option<Vec<u8>> {
    let rest = url.get(5..)?;
    let (header, data) = rest.split_once(',')?;
    let base64 = header
        .rsplit(';')
        .next()
        .is_some_and(|last| last.trim().eq_ignore_ascii_case("base64"));
    let data = percent_decode(data.as_bytes())?;
    if base64 {
        base64_decode(&data)
    } else {
        Some(data)
    }
}

/// RFC 3986 §2.1's percent-encoding, undone; `None` for a `%` that is not
/// followed by two hexadecimal digits.
fn percent_decode(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&b) = bytes.get(at) {
        if b == b'%' {
            let high = hex(*bytes.get(at + 1)?)?;
            let low = hex(*bytes.get(at + 2)?)?;
            out.push((high << 4) | low);
            at += 3;
        } else {
            out.push(b);
            at += 1;
        }
    }
    Some(out)
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// RFC 4648 §4's base64, decoded.
///
/// White space is skipped wherever it falls, because the two places this build
/// meets base64 — a `data:` URL wrapped by a producer and an FB2 `<binary>`
/// element, which every producer breaks into lines — both put it there, and
/// RFC 4648 §3.3 lets a specification that refers to it say so. Padding is
/// optional and, where present, ends the data; any other character outside the
/// alphabet, or a final group of one character, is `None`.
#[must_use]
pub fn base64_decode(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3 + 3);
    let mut group: u32 = 0;
    let mut held = 0u8;
    let mut padded = false;
    for &b in text {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b' ' | b'\t' | b'\r' | b'\n' | b'\x0C' => continue,
            b'=' => {
                padded = true;
                continue;
            }
            _ => return None,
        };
        // Data after padding is not base64 any more.
        if padded {
            return None;
        }
        group = (group << 6) | u32::from(value);
        held += 1;
        if held == 4 {
            out.extend_from_slice(&[(group >> 16) as u8, (group >> 8) as u8, group as u8]);
            group = 0;
            held = 0;
        }
    }
    match held {
        0 => {}
        // One character carries six bits, which is not a byte.
        1 => return None,
        2 => out.push((group >> 4) as u8),
        _ => out.extend_from_slice(&[(group >> 10) as u8, (group >> 2) as u8]),
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4648 §10's test vectors, every one, and the same with the padding
    /// left off.
    #[test]
    fn base64_is_rfc_4648_s_own_vectors() {
        for (encoded, decoded) in [
            ("", ""),
            ("Zg==", "f"),
            ("Zm8=", "fo"),
            ("Zm9v", "foo"),
            ("Zm9vYg==", "foob"),
            ("Zm9vYmE=", "fooba"),
            ("Zm9vYmFy", "foobar"),
        ] {
            assert_eq!(
                base64_decode(encoded.as_bytes()).as_deref(),
                Some(decoded.as_bytes()),
                "{encoded}"
            );
            let bare = encoded.trim_end_matches('=');
            assert_eq!(
                base64_decode(bare.as_bytes()).as_deref(),
                Some(decoded.as_bytes()),
                "{bare} without its padding"
            );
        }
        assert_eq!(
            base64_decode(b"Zm9v\r\n YmFy").as_deref(),
            Some(b"foobar".as_slice()),
            "white space is skipped"
        );
        assert_eq!(base64_decode(b"Zm9v!"), None, "outside the alphabet");
        assert_eq!(base64_decode(b"Zm9vY"), None, "one character is no byte");
        assert_eq!(base64_decode(b"Zg==Zg=="), None, "data after the padding");
    }

    /// RFC 2397 §4's own examples, with base64 and without.
    ///
    /// The second is printed in the RFC as `%be%fg%be`, and `%fg` is not an
    /// escape under RFC 3986 §2.1 — so the example as printed is refused, and
    /// the same URL with a well-formed middle escape is decoded.
    #[test]
    fn a_data_url_is_rfc_2397_s_grammar() {
        assert_eq!(
            data_url("data:,A%20brief%20note").as_deref(),
            Some(b"A brief note".as_slice())
        );
        assert_eq!(
            data_url("data:text/plain;charset=iso-8859-7,%be%fg%be"),
            None,
            "%fg is not an escape"
        );
        assert_eq!(
            data_url("data:text/plain;charset=iso-8859-7,%be%d3%be").as_deref(),
            Some([0xBE, 0xD3, 0xBE].as_slice())
        );
        assert_eq!(
            data_url("data:image/gif;base64,R0lGODdh").as_deref(),
            Some(b"GIF87a".as_slice())
        );
        assert_eq!(data_url("data:no comma"), None);
    }

    fn kind(text: &str) -> Option<Standalone> {
        sniff(text.as_bytes())
    }

    #[test]
    fn a_markup_document_is_told_by_its_root_past_the_prolog() {
        assert_eq!(kind("<svg/>"), Some(Standalone::Svg));
        assert_eq!(
            kind(concat!(
                "\u{FEFF}<?xml version=\"1.0\"?>\n",
                "<!-- Created with a drawing program -->\n",
                "<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" ",
                "\"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\" [\n",
                "  <!ENTITY ns_svg \"http://www.w3.org/2000/svg\">\n",
                "  <!ENTITY gt \">\">\n",
                "]>\n<?xml-stylesheet href=\"a.css\"?>",
                "<svg:svg xmlns:svg=\"http://www.w3.org/2000/svg\"/>"
            )),
            Some(Standalone::Svg),
            "a declaration, a comment, a doctype with an internal subset holding \
             a `>`, a processing instruction and a prefixed root"
        );
        assert_eq!(
            kind("<html xmlns=\"http://www.w3.org/1999/xhtml\"/>"),
            Some(Standalone::Html)
        );
        assert_eq!(
            kind("<!DOCTYPE html>\n<HTML><body>"),
            Some(Standalone::Html)
        );
        assert_eq!(
            kind("<!doctype html><title>no html element</title>"),
            Some(Standalone::Html),
            "HTML may leave its root out, and the doctype says what it is"
        );
        assert_eq!(
            kind("<?xml version=\"1.0\"?>\n<FictionBook xmlns=\"x\"><body/></FictionBook>"),
            Some(Standalone::Fb2)
        );
        assert_eq!(
            kind("<fictionbook/>"),
            None,
            "an FB2 root is case-sensitive"
        );
        assert_eq!(kind("<SVG/>"), None, "XML names are case-sensitive");
        assert_eq!(kind("<FixedPage/>"), None, "XML that is none of these");
        assert_eq!(kind("<!-- never closes <svg/>"), None);
        assert_eq!(kind("text <svg/>"), None, "the root is not searched for");
        assert_eq!(kind("this is not a pdf at all"), None);
    }

    #[test]
    fn a_pdf_header_where_the_parser_looks_for_one_wins() {
        assert_eq!(kind("<svg/>%PDF-1.7"), None);
        // The last place a header still ends inside the parser's window.
        let mut edge = String::from("<svg>");
        edge.push_str(&" ".repeat(PDF_HEADER_WINDOW - 5 - "<svg>".len()));
        edge.push_str("%PDF-1.7");
        assert_eq!(kind(&edge), None, "a header the parser would find");
        let mut far = String::from("<svg>");
        far.push_str(&" ".repeat(PDF_HEADER_WINDOW - 4 - "<svg>".len()));
        far.push_str("%PDF-1.7");
        assert_eq!(
            kind(&far),
            Some(Standalone::Svg),
            "a header that does not end inside the parser's window is not one"
        );
        assert_eq!(sniff(b"\xFF\xD8\xFF%PDF-1.4"), None, "a JPEG polyglot");
    }

    #[test]
    fn an_image_is_told_by_its_magic_and_a_bmp_is_not_sniffed() {
        assert_eq!(
            sniff(b"\x89PNG\r\n\x1A\n...."),
            Some(Standalone::Image(ImageFormat::Png))
        );
        assert_eq!(sniff(b"GIF89a"), Some(Standalone::Image(ImageFormat::Gif)));
        assert_eq!(sniff(b"BMW reports a record year, and so on"), None);
    }

    #[test]
    fn utf_16_is_walked_and_utf_32_is_not() {
        let wide = |mark: &[u8], width: usize| -> Vec<u8> {
            let mut out = mark.to_vec();
            for b in "<!-- é --><svg/>".bytes() {
                out.push(b);
                out.extend(std::iter::repeat_n(0, width - 1));
            }
            out
        };
        assert_eq!(sniff(&wide(&[0xFF, 0xFE], 2)), Some(Standalone::Svg));
        assert_eq!(sniff(&wide(&[], 2)), Some(Standalone::Svg), "unmarked");
        // Not decoded by `tinker-pdf-xml`, so not a document anything here reads.
        assert_eq!(sniff(&wide(&[0xFF, 0xFE, 0, 0], 4)), None);
    }
}
