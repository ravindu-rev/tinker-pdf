//! A document that is one file and not a container: a standalone SVG, a bare
//! image, a loose XHTML file (tier 5's formats row).
//!
//! Every reader these need was already in the tree — `tinker-pdf-svg` and
//! `epub::svg` for a picture, the PNG, TIFF and JPEG embedders the comic path
//! places its pages with, the EPUB cascade, layout and painter for a content
//! document — and each of the three was refused as not-a-PDF because nothing
//! asked whether the bytes were one of them. This module is that question and
//! the routing behind it. It adds no reader of its own: an SVG is a book of one
//! pre-paginated chapter, a loose XHTML file is a book of one reflowable
//! chapter, and a bare image is a comic of one page, each built by the code that
//! builds the larger document so the two cannot disagree.
//!
//! # The sniff, and why a PDF always wins it
//!
//! [`sniff`] answers `None` for anything carrying `%PDF-` in its first 1 024
//! bytes, which is where 7.5.2's leniency lets a header sit — so a PDF with
//! junk in front of it, and a polyglot that is a PDF and something else, stay
//! PDFs. Past that, an image is told by its magic at offset zero, exactly as
//! [`cbz::image_format`] tells a page, and a markup document by the name of its
//! root element once the prolog is skipped: the byte-order mark, white space,
//! the XML declaration and any processing instruction, comments, and the
//! document type declaration with its internal subset. The window is
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
//! - **A bare image** is one page, one image pixel to one point (8.9.5.2),
//!   built with the embedders the comic path uses. A format recognised and not
//!   decoded here — GIF, WebP, AVIF — or bytes that will not decode are one
//!   placeholder page naming why, which is what a comic archive holding that
//!   one picture has always produced.
//!
//! # References, and the one kind a loose file can resolve
//!
//! A file opened from its bytes alone has nothing beside it, so a stylesheet,
//! a picture or a face it names by a relative reference is missing, and each
//! is named as missing by the warning that already exists for it. The
//! exception is RFC 2397's `data:` URL, which carries its own bytes and is how
//! a self-contained HTML file embeds a picture: [`DataUrls`] answers those,
//! and hands everything else to whatever stands behind it.

use tinker_pdf_cos::{png_image, tiff_image, DocumentBuilder, ImageData};
use tinker_pdf_filters::Limits as FilterLimits;

use crate::cbz::{
    image_format, jpx_image, jpx_space, ArchiveRefusal, ArchiveReport, ArchiveWarning, ImageFormat,
    PageDefect, PageOrigin, PLACEHOLDER_GREY,
};
use crate::epub::read::{Resources, Unavailable};
use crate::epub::{self, BookLayout, Loose};

/// How many bytes [`sniff`] looks at.
///
/// Four kilobytes — the chunk a streamed open reads anyway
/// ([`crate::CHUNK_SIZE`]) — because the root element of a markup document
/// comes after its prolog, and an XML declaration, a generator's comment and a
/// doctype with a small internal subset fit in it where they would not all fit
/// in the 1 024 bytes a PDF header is looked for in.
pub const SNIFF_WINDOW: usize = 4_096;

/// Where 7.5.2's leniency lets a PDF header sit, and so where a PDF is looked
/// for before anything else is.
const PDF_HEADER_WINDOW: usize = 1_024;

/// What a one-file document turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Standalone {
    /// An SVG document: the root element's local name is `svg`.
    Svg,
    /// An XHTML or HTML document: the root element is `html`, in any case, or
    /// the document type declaration names `html`.
    Html,
    /// An image, by its magic at offset zero.
    Image(ImageFormat),
}

/// Whether the bytes are a one-file document this build opens, and which.
///
/// Reads at most [`SNIFF_WINDOW`] bytes, and answers `None` for anything with
/// `%PDF-` in its first 1 024 — see the module comment for why a PDF always
/// wins.
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
    let (doctype, root) = prolog(head);
    let root = root.map(|name| match name.iter().rposition(|&b| b == b':') {
        Some(colon) => name.get(colon + 1..).unwrap_or_default(),
        None => name,
    });
    match root {
        Some(b"svg") => Some(Standalone::Svg),
        Some(name) if name.eq_ignore_ascii_case(b"html") => Some(Standalone::Html),
        // HTML lets a document leave out its `<html>`, and says what it is in
        // its doctype instead. The reader will stop where the markup stops being
        // XML, and the report will say so.
        _ if doctype.is_some_and(|name| name.eq_ignore_ascii_case(b"html")) => {
            Some(Standalone::Html)
        }
        _ => None,
    }
}

/// The document type declaration's name and the root element's qualified
/// name, as far as the window shows them.
///
/// XML 1.0 §2.8's prolog: an optional declaration, then any number of
/// comments, processing instructions and white space, with at most one
/// document type declaration among them. Nothing is decoded and nothing is
/// validated — a construct that does not end inside the window ends the walk.
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
    if let Standalone::Image(format) = kind {
        return image(bytes, format, &limits);
    }
    let mut builder = DocumentBuilder::new();
    let content = match kind {
        Standalone::Svg => Loose::Svg(bytes),
        _ => {
            let dom = epub::read::markup(bytes, &limits.xml);
            if let Some(title) = dom.title() {
                builder.set_info(b"Title", &title);
            }
            Loose::Markup(dom)
        }
    };
    let laid = epub::lay_out_one(
        &mut DataUrls(epub::read::NoResources),
        &mut builder,
        "",
        content,
        "",
        epub::PAGE_MARGIN,
        &limits,
        layout,
    );
    let pdf = builder.finish();
    if pdf.len() > limits.max_synthesised {
        return Err(ArchiveRefusal::TooLarge);
    }
    let synthesised_bytes = pdf.len();
    Ok((
        pdf,
        ArchiveReport::book(
            laid.warnings,
            laid.pages,
            synthesised_bytes,
            *layout,
            laid.cost,
        ),
    ))
}

/// The resource name the one image is drawn under, as the comic path's is.
const IMAGE_RESOURCE: &[u8] = b"Im";

/// A bare image as a document of one page.
///
/// The comic path's per-entry decisions, made with the comic path's own
/// helpers — `png_image`, `tiff_image`, `jpeg_shape` and the JPEG 2000 header
/// — for an entry that has no archive around it. The ceiling on a decoded
/// raster is the comic path's too: the largest entry an archive may hand over,
/// which is the most a page's picture may be.
fn image(
    bytes: &[u8],
    format: ImageFormat,
    limits: &epub::Limits,
) -> Result<(Vec<u8>, ArchiveReport), ArchiveRefusal> {
    let ceiling = FilterLimits::new(crate::cbz::zip_limits::MAX_ZIP_ENTRY_BYTES);
    let mut builder = DocumentBuilder::new();
    // `Some((size, degraded))` when the picture was registered, and the
    // defect when it was not.
    let placed: Result<((f64, f64), bool), PageDefect> = match format {
        ImageFormat::Jpeg => match tinker_pdf_cos::jpeg_shape(bytes) {
            Some((width, height, _)) if width > 0 && height > 0 => {
                if builder.add_image(IMAGE_RESOURCE, &ImageData::Jpeg(bytes)) {
                    Ok(((f64::from(width), f64::from(height)), false))
                } else {
                    Err(PageDefect::Undecodable)
                }
            }
            _ => Err(PageDefect::Undecodable),
        },
        ImageFormat::Png => match png_image(bytes, &ceiling) {
            Ok(png) if png.width() > 0 && png.height() > 0 => {
                if builder.add_image(IMAGE_RESOURCE, &png.image()) {
                    Ok((
                        (f64::from(png.width()), f64::from(png.height())),
                        !png.complete(),
                    ))
                } else {
                    Err(PageDefect::Undecodable)
                }
            }
            _ => Err(PageDefect::Undecodable),
        },
        ImageFormat::Tiff => match tiff_image(bytes, &ceiling) {
            Ok(tiff) if tiff.width() > 0 && tiff.height() > 0 => {
                if builder.add_image(IMAGE_RESOURCE, &tiff.image()) {
                    Ok((
                        (f64::from(tiff.width()), f64::from(tiff.height())),
                        !tiff.complete(),
                    ))
                } else {
                    Err(PageDefect::Undecodable)
                }
            }
            _ => Err(PageDefect::Undecodable),
        },
        ImageFormat::Jpeg2000 => match tinker_pdf_filters::jpx_header(bytes, &ceiling) {
            Ok(header) if header.width > 0 && header.height > 0 && jpx_space(&header).is_some() => {
                let image = ImageData::Compressed(jpx_image(bytes, &header));
                if builder.add_image(IMAGE_RESOURCE, &image) {
                    Ok((
                        (f64::from(header.width), f64::from(header.height)),
                        header.opacity,
                    ))
                } else {
                    Err(PageDefect::Undecodable)
                }
            }
            _ => Err(PageDefect::Undecodable),
        },
        other => Err(PageDefect::UnsupportedFormat(other)),
    };

    let mut warnings = Vec::new();
    let defect = match placed {
        Ok(((width, height), degraded)) => {
            builder.add_page(width, height, |page| {
                // 8.9.5.2: an image occupies the unit square, so one image
                // pixel is one point exactly when the transform is the
                // page's own size.
                page.image(IMAGE_RESOURCE, 0.0, 0.0, width, height);
            });
            if degraded {
                warnings.push(ArchiveWarning::DegradedImage { page: 0 });
            }
            None
        }
        Err(defect) => {
            // A placeholder has no size of its own and there is no neighbour
            // to borrow one from, so it is the comic path's answer for an
            // archive that never states one: US Letter.
            let (width, height) = crate::cbz::FALLBACK_PAGE;
            builder.add_page(width, height, |page| {
                page.fill_rect(0.0, 0.0, width, height, PLACEHOLDER_GREY);
            });
            warnings.push(ArchiveWarning::PlaceholderPage { page: 0, defect });
            Some(defect)
        }
    };
    let pdf = builder.finish();
    if pdf.len() > limits.max_synthesised {
        return Err(ArchiveRefusal::TooLarge);
    }
    let synthesised_bytes = pdf.len();
    let pages = vec![PageOrigin {
        name: String::new(),
        defect,
    }];
    Ok((
        pdf,
        ArchiveReport::synthesised(warnings, pages, synthesised_bytes, None, 0),
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
        assert_eq!(kind("<SVG/>"), None, "XML names are case-sensitive");
        assert_eq!(kind("<FixedPage/>"), None, "XML that is none of these");
        assert_eq!(kind("<!-- never closes <svg/>"), None);
        assert_eq!(kind("text <svg/>"), None, "the root is not searched for");
        assert_eq!(kind("this is not a pdf at all"), None);
    }

    #[test]
    fn a_pdf_header_in_the_first_kilobyte_wins() {
        assert_eq!(kind("<svg/>%PDF-1.7"), None);
        let mut far = String::from("<svg>");
        far.push_str(&" ".repeat(PDF_HEADER_WINDOW));
        far.push_str("%PDF-1.7");
        assert_eq!(
            kind(&far),
            Some(Standalone::Svg),
            "past the window 7.5.2 allows, a header is not one"
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
}
