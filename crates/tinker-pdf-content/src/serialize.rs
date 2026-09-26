//! A page's text as JSON, XML or HTML, with fonts, sizes and boxes.
//!
//! Hand-written writers, and deliberately so: CONTRIBUTING's first rule leaves
//! no room for a serialisation crate inside the engine, and three formats of
//! one small tree are a few hundred lines, most of them escaping — which is the
//! part worth owning, because it is the part a hostile document attacks.
//!
//! # The model
//!
//! The same for all three formats, one level per [`TextPage`] level plus one:
//!
//! ```text
//! document                      format "tinker-pdf/text", version 1
//! └ page                        index, box, rotation, warnings
//!   └ block                     bbox
//!     └ line                    bbox, wmode, rtl, size, text
//!       └ span                  font, size, bbox, text
//!         └ char                c, quad, origin
//! ```
//!
//! A **span** is a run of consecutive characters on one line that share a
//! font name and a size (to the thousandth of a unit the numbers are written
//! at). It is the level [`TextPage`] does not have and a consumer asking
//! "which font is this word in" needs; the font name rides on it rather than
//! on every character, which is where the row asked for it.
//!
//! Every coordinate is **PDF user space, y upward** — the space the page's own
//! boxes are in, and the one [`crate::Quad`] reports — and is written to three
//! decimal places, a thousandth of a point. `box` is the page's crop box in
//! that space and `rotation` its `/Rotate`, which the coordinates do **not**
//! have applied: a consumer that wants the page as a viewer shows it turns it
//! by that much. A `bbox` is `[x0, y0, x1, y1]`; a character's `quad` is its
//! four corners, upper-left, upper-right, lower-left, lower-right, which is
//! the order [`crate::Quad`] names them in. A number that is not finite —
//! which extraction does not produce, and a writer must not assume — is JSON
//! `null` and an absent XML attribute.
//!
//! `font` is `/BaseFont` as the file writes it, subset tag and all, and
//! absent where the font states none. `wmode` is `horizontal` or `vertical`
//! (9.7.4.3) and `rtl` whether the line is predominantly right-to-left, the
//! two properties [`crate::TextLine`] keeps apart.
//!
//! # Escaping, per format
//!
//! - **JSON** (RFC 8259 §7): `"` and `\` escaped, every control character
//!   below U+0020 escaped — the short forms where the RFC has them, `\u00XX`
//!   otherwise — and U+2028 and U+2029 escaped too, which the RFC does not
//!   require and a JavaScript consumer does. Everything else is written as
//!   UTF-8. A Rust `str` cannot hold a lone surrogate, so the RFC's one
//!   unrepresentable case cannot arise.
//! - **XML** (1.0, §2.2 `Char`, §2.4, §3.3.3): `&`, `<`, `>`, `"` and `'` as
//!   entities; tab, line feed and carriage return as character references,
//!   so that neither end-of-line handling nor attribute-value normalisation
//!   changes them. The characters XML 1.0 cannot carry **in any form** — C0
//!   controls other than those three, U+FFFE and U+FFFF — are written as
//!   U+FFFD, because a character reference to one is not well-formed either.
//!   That is a loss, and it is named here rather than hidden; JSON carries
//!   them exactly.
//! - **HTML**: `&`, `<`, `>`, `"` and `'` as references, and every control
//!   character other than tab and line feed, and every noncharacter, as
//!   U+FFFD — the HTML parser reports each as a parse error, and the HTML
//!   view is a picture of the page rather than an exchange format. The font
//!   name goes into a `data-font` attribute and **never into CSS**, so no
//!   second escaping context exists to get wrong.

use core::fmt::Write as _;
use std::sync::Arc;

use crate::text::{Quad, TextChar, TextLine, TextPage, TextWarning, WritingMode};

/// Which serialisation [`TextWriter`] writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextFormat {
    /// RFC 8259 JSON: one object, a `pages` array inside it, one page per
    /// line.
    Json,
    /// XML 1.0: a `document` element, one `page` element per page.
    Xml,
    /// An HTML document that shows each page's lines where the page puts
    /// them, with the model in `data-` attributes.
    Html,
}

/// Where a page's text sits: what [`TextPage`] does not know about the page
/// it came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageFrame {
    /// The page's zero-based index in its document.
    pub index: u32,
    /// The visible box, `(x0, y0, x1, y1)` in user space — the crop box.
    pub bounds: (f64, f64, f64, f64),
    /// `/Rotate`, which the text's coordinates do not have applied.
    pub rotation: u16,
}

/// The version of the model above. It changes when a field changes meaning or
/// goes away; a new field does not change it.
pub const TEXT_FORMAT_VERSION: u32 = 1;

/// Writes pages of text in one [`TextFormat`], a page at a time.
///
/// A document's worth of pages need not be held at once: [`TextWriter::take`]
/// hands back what has been written so far, and [`TextWriter::finish`] the
/// rest with the closing markup.
#[derive(Debug)]
pub struct TextWriter {
    format: TextFormat,
    out: String,
    pages: usize,
}

impl TextWriter {
    /// A writer with the format's opening markup already written.
    #[must_use]
    pub fn new(format: TextFormat) -> TextWriter {
        let mut out = String::new();
        match format {
            TextFormat::Json => {
                let _ = write!(
                    out,
                    "{{\"format\":\"tinker-pdf/text\",\"version\":{TEXT_FORMAT_VERSION},\"pages\":["
                );
            }
            TextFormat::Xml => {
                let _ = writeln!(
                    out,
                    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                     <document format=\"tinker-pdf/text\" version=\"{TEXT_FORMAT_VERSION}\">"
                );
            }
            TextFormat::Html => {
                out.push_str(
                    "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
                     <title>tinker-pdf text</title>\n<style>\n\
                     .page{position:relative;margin:1em auto;border:1px solid #999;overflow:hidden}\n\
                     .line{position:absolute;white-space:pre;line-height:1}\n\
                     </style>\n</head>\n<body>\n",
                );
                let _ = writeln!(
                    out,
                    "<main data-format=\"tinker-pdf/text\" data-version=\"{TEXT_FORMAT_VERSION}\">"
                );
            }
        }
        TextWriter {
            format,
            out,
            pages: 0,
        }
    }

    /// Writes one page.
    pub fn page(&mut self, frame: &PageFrame, page: &TextPage) {
        match self.format {
            TextFormat::Json => {
                if self.pages > 0 {
                    self.out.push(',');
                }
                self.out.push('\n');
                json_page(&mut self.out, frame, page);
            }
            TextFormat::Xml => xml_page(&mut self.out, frame, page),
            TextFormat::Html => html_page(&mut self.out, frame, page),
        }
        self.pages += 1;
    }

    /// What has been written since the last call, leaving the writer ready
    /// for the next page.
    pub fn take(&mut self) -> String {
        core::mem::take(&mut self.out)
    }

    /// The rest of the document, closing markup included.
    #[must_use]
    pub fn finish(mut self) -> String {
        match self.format {
            TextFormat::Json => self.out.push_str("\n]}\n"),
            TextFormat::Xml => self.out.push_str("</document>\n"),
            TextFormat::Html => self.out.push_str("</main>\n</body>\n</html>\n"),
        }
        self.out
    }
}

impl TextPage {
    /// This page alone, as a whole document in `format`.
    #[must_use]
    pub fn serialize(&self, format: TextFormat, frame: &PageFrame) -> String {
        let mut writer = TextWriter::new(format);
        writer.page(frame, self);
        writer.finish()
    }
}

/// A run of characters on one line sharing a font and a size.
struct Span<'a> {
    font: Option<&'a Arc<str>>,
    size: f64,
    chars: &'a [TextChar],
}

/// A line's spans, in the line's own order.
fn spans(line: &TextLine) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let chars = &line.chars;
    for i in 1..=chars.len() {
        let breaks = match (chars.get(i - 1), chars.get(i)) {
            (Some(a), Some(b)) => a.font != b.font || !same_number(a.size, b.size),
            _ => true,
        };
        if breaks {
            if let (Some(first), Some(run)) = (chars.get(start), chars.get(start..i)) {
                out.push(Span {
                    font: first.font.as_ref(),
                    size: first.size,
                    chars: run,
                });
            }
            start = i;
        }
    }
    out
}

/// Whether two numbers print the same at the precision they are written at.
fn same_number(a: f64, b: f64) -> bool {
    number(a) == number(b)
}

/// A number at three decimal places with trailing zeros dropped, or `None`
/// when it is not finite.
///
/// `-0` is written `0`: a coordinate that rounds to zero from below is not a
/// different place from one that rounds to it from above.
fn number(value: f64) -> Option<String> {
    if !value.is_finite() {
        return None;
    }
    let mut text = format!("{value:.3}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        text = "0".to_string();
    }
    Some(text)
}

/// The axis-aligned box around some characters' quads, as `[x0, y0, x1, y1]`.
fn bbox(chars: &[TextChar]) -> Option<(f64, f64, f64, f64)> {
    let mut out: Option<(f64, f64, f64, f64)> = None;
    for c in chars {
        if !c.quad.is_finite() {
            continue;
        }
        let (x0, y0, x1, y1) = c.quad.bounds();
        out = Some(match out {
            None => (x0, y0, x1, y1),
            Some((a0, b0, a1, b1)) => (a0.min(x0), b0.min(y0), a1.max(x1), b1.max(y1)),
        });
    }
    out
}

fn wmode(mode: WritingMode) -> &'static str {
    match mode {
        WritingMode::Horizontal => "horizontal",
        WritingMode::Vertical => "vertical",
    }
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

/// A JSON string literal (RFC 8259 §7).
fn json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if u32::from(c) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_number(out: &mut String, value: f64) {
    match number(value) {
        Some(text) => out.push_str(&text),
        None => out.push_str("null"),
    }
}

fn json_numbers(out: &mut String, values: &[f64]) {
    out.push('[');
    for (i, value) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_number(out, *value);
    }
    out.push(']');
}

fn json_bbox(out: &mut String, bounds: Option<(f64, f64, f64, f64)>) {
    match bounds {
        Some((x0, y0, x1, y1)) => json_numbers(out, &[x0, y0, x1, y1]),
        None => out.push_str("null"),
    }
}

fn json_quad(out: &mut String, quad: &Quad) {
    json_numbers(
        out,
        &[
            quad.ul.0, quad.ul.1, quad.ur.0, quad.ur.1, quad.ll.0, quad.ll.1, quad.lr.0, quad.lr.1,
        ],
    );
}

fn json_page(out: &mut String, frame: &PageFrame, page: &TextPage) {
    let _ = write!(out, "{{\"index\":{},\"box\":", frame.index);
    let (x0, y0, x1, y1) = frame.bounds;
    json_numbers(out, &[x0, y0, x1, y1]);
    let _ = write!(out, ",\"rotation\":{},\"blocks\":[", frame.rotation);
    for (b, block) in page.blocks.iter().enumerate() {
        if b > 0 {
            out.push(',');
        }
        out.push_str("{\"bbox\":");
        json_bbox(
            out,
            Some(block.quad.bounds()).filter(|_| block.quad.is_finite()),
        );
        out.push_str(",\"lines\":[");
        for (l, line) in block.lines.iter().enumerate() {
            if l > 0 {
                out.push(',');
            }
            out.push_str("{\"bbox\":");
            json_bbox(
                out,
                Some(line.quad.bounds()).filter(|_| line.quad.is_finite()),
            );
            let _ = write!(
                out,
                ",\"wmode\":\"{}\",\"rtl\":{},\"size\":",
                wmode(line.wmode),
                line.rtl
            );
            json_number(out, line.size);
            out.push_str(",\"text\":");
            json_string(out, &line.text);
            out.push_str(",\"spans\":[");
            for (s, span) in spans(line).iter().enumerate() {
                if s > 0 {
                    out.push(',');
                }
                out.push_str("{\"font\":");
                match span.font {
                    Some(font) => json_string(out, font),
                    None => out.push_str("null"),
                }
                out.push_str(",\"size\":");
                json_number(out, span.size);
                out.push_str(",\"bbox\":");
                json_bbox(out, bbox(span.chars));
                out.push_str(",\"text\":");
                let text: String = span.chars.iter().map(|c| c.text.as_str()).collect();
                json_string(out, &text);
                out.push_str(",\"chars\":[");
                for (i, c) in span.chars.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str("{\"c\":");
                    json_string(out, &c.text);
                    out.push_str(",\"quad\":");
                    json_quad(out, &c.quad);
                    out.push_str(",\"origin\":");
                    json_numbers(out, &[c.origin.0, c.origin.1]);
                    out.push('}');
                }
                out.push_str("]}");
            }
            out.push_str("]}");
        }
        out.push_str("]}");
    }
    out.push_str("],\"warnings\":[");
    for (i, warning) in page.warnings.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        match warning {
            TextWarning::UnknownFont { name } => {
                out.push_str("{\"kind\":\"unknown-font\",\"name\":");
                json_string(out, name);
                out.push('}');
            }
            TextWarning::UnmappedCode { code } => {
                let _ = write!(out, "{{\"kind\":\"unmapped-code\",\"code\":{code}}}");
            }
        }
    }
    out.push_str("]}");
}

// ---------------------------------------------------------------------------
// XML
// ---------------------------------------------------------------------------

/// Whether XML 1.0 can carry a character at all (§2.2, production 2).
fn xml_char(c: char) -> bool {
    matches!(u32::from(c),
        0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x1_0000..=0x10_FFFF)
}

/// Text escaped for XML content or a double-quoted attribute value.
fn xml_escape(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // §2.11's end-of-line handling and §3.3.3's attribute-value
            // normalisation would both rewrite these if they were written raw.
            '\t' => out.push_str("&#x9;"),
            '\n' => out.push_str("&#xA;"),
            '\r' => out.push_str("&#xD;"),
            c if !xml_char(c) => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
}

fn xml_numbers(out: &mut String, name: &str, values: &[f64]) {
    let mut written = Vec::with_capacity(values.len());
    for value in values {
        match number(*value) {
            Some(text) => written.push(text),
            // One unwritable number makes the whole list unwritable: a bbox
            // with three coordinates is not a bbox.
            None => return,
        }
    }
    let _ = write!(out, " {name}=\"{}\"", written.join(" "));
}

fn xml_page(out: &mut String, frame: &PageFrame, page: &TextPage) {
    let (x0, y0, x1, y1) = frame.bounds;
    let _ = write!(out, "<page index=\"{}\"", frame.index);
    xml_numbers(out, "box", &[x0, y0, x1, y1]);
    let _ = writeln!(out, " rotation=\"{}\">", frame.rotation);
    for block in &page.blocks {
        out.push_str("<block");
        if block.quad.is_finite() {
            let (a, b, c, d) = block.quad.bounds();
            xml_numbers(out, "bbox", &[a, b, c, d]);
        }
        out.push_str(">\n");
        for line in &block.lines {
            out.push_str("<line");
            if line.quad.is_finite() {
                let (a, b, c, d) = line.quad.bounds();
                xml_numbers(out, "bbox", &[a, b, c, d]);
            }
            let _ = write!(out, " wmode=\"{}\" rtl=\"{}\"", wmode(line.wmode), line.rtl);
            xml_numbers(out, "size", &[line.size]);
            out.push_str(" text=\"");
            xml_escape(out, &line.text);
            out.push_str("\">\n");
            for span in spans(line) {
                out.push_str("<span");
                if let Some(font) = span.font {
                    out.push_str(" font=\"");
                    xml_escape(out, font);
                    out.push('"');
                }
                xml_numbers(out, "size", &[span.size]);
                if let Some((a, b, c, d)) = bbox(span.chars) {
                    xml_numbers(out, "bbox", &[a, b, c, d]);
                }
                out.push_str(" text=\"");
                let text: String = span.chars.iter().map(|c| c.text.as_str()).collect();
                xml_escape(out, &text);
                out.push_str("\">\n");
                for c in span.chars {
                    out.push_str("<char c=\"");
                    xml_escape(out, &c.text);
                    out.push('"');
                    let q = &c.quad;
                    xml_numbers(
                        out,
                        "quad",
                        &[
                            q.ul.0, q.ul.1, q.ur.0, q.ur.1, q.ll.0, q.ll.1, q.lr.0, q.lr.1,
                        ],
                    );
                    xml_numbers(out, "origin", &[c.origin.0, c.origin.1]);
                    out.push_str("/>\n");
                }
                out.push_str("</span>\n");
            }
            out.push_str("</line>\n");
        }
        out.push_str("</block>\n");
    }
    for warning in &page.warnings {
        match warning {
            TextWarning::UnknownFont { name } => {
                out.push_str("<warning kind=\"unknown-font\" name=\"");
                xml_escape(out, name);
                out.push_str("\"/>\n");
            }
            TextWarning::UnmappedCode { code } => {
                let _ = writeln!(out, "<warning kind=\"unmapped-code\" code=\"{code}\"/>");
            }
        }
    }
    out.push_str("</page>\n");
}

// ---------------------------------------------------------------------------
// HTML
// ---------------------------------------------------------------------------

/// Whether the HTML parser takes a character without a parse error: not a
/// control other than tab and line feed, and not a noncharacter.
fn html_char(c: char) -> bool {
    let code = u32::from(c);
    let noncharacter = (0xFDD0..=0xFDEF).contains(&code) || (code & 0xFFFE) == 0xFFFE;
    (!c.is_control() || c == '\t' || c == '\n') && !noncharacter
}

/// Text escaped for HTML content or a double-quoted attribute value.
fn html_escape(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if !html_char(c) => out.push('\u{FFFD}'),
            c => out.push(c),
        }
    }
}

/// A length in points for a `style` attribute: digits, a point and a sign
/// only, so it needs no escaping in either of the contexts it sits in.
fn css_points(value: f64) -> String {
    format!("{}pt", number(value).unwrap_or_else(|| "0".to_string()))
}

fn html_page(out: &mut String, frame: &PageFrame, page: &TextPage) {
    let (x0, y0, x1, y1) = frame.bounds;
    let _ = writeln!(
        out,
        "<div class=\"page\" data-index=\"{}\" data-rotation=\"{}\" style=\"width:{};height:{}\">",
        frame.index,
        frame.rotation,
        css_points(x1 - x0),
        css_points(y1 - y0)
    );
    for block in &page.blocks {
        out.push_str("<div class=\"block\">\n");
        for line in &block.lines {
            let (lx0, _, _, ly1) = if line.quad.is_finite() {
                line.quad.bounds()
            } else {
                (x0, y1, x0, y1)
            };
            // User space is y upward from the box's corner; a page in HTML is
            // y downward from its top-left.
            let _ = write!(
                out,
                "<div class=\"line\" data-wmode=\"{}\" data-rtl=\"{}\" \
                 style=\"left:{};top:{};font-size:{}\">",
                wmode(line.wmode),
                line.rtl,
                css_points(lx0 - x0),
                css_points(y1 - ly1),
                css_points(line.size)
            );
            for span in spans(line) {
                out.push_str("<span");
                if let Some(font) = span.font {
                    out.push_str(" data-font=\"");
                    html_escape(out, font);
                    out.push('"');
                }
                if let Some(size) = number(span.size) {
                    let _ = write!(out, " data-size=\"{size}\"");
                }
                out.push('>');
                for c in span.chars {
                    html_escape(out, &c.text);
                }
                out.push_str("</span>");
            }
            out.push_str("</div>\n");
        }
        out.push_str("</div>\n");
    }
    out.push_str("</div>\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOSTILE: &str = "q\"b\\s/\u{0}\u{1}\u{8}\u{c}\n\r\t\u{1f}\u{7f}\u{85}\u{2028}\u{2029}<&>'\u{fffe}\u{ffff}\u{fdd0}é\u{1F600}";

    fn escaped(f: fn(&mut String, &str), text: &str) -> String {
        let mut out = String::new();
        f(&mut out, text);
        out
    }

    #[test]
    fn json_escapes_what_rfc_8259_requires() {
        assert_eq!(
            escaped(json_string, HOSTILE),
            "\"q\\\"b\\\\s/\\u0000\\u0001\\b\\f\\n\\r\\t\\u001f\u{7f}\u{85}\\u2028\\u2029<&>'\u{fffe}\u{ffff}\u{fdd0}é\u{1F600}\""
        );
        // Nothing below U+0020 survives unescaped, whatever it is.
        let all: String = (0u8..0x20).map(char::from).collect();
        let out = escaped(json_string, &all);
        assert!(out.chars().all(|c| u32::from(c) >= 0x20), "{out:?}");
    }

    #[test]
    fn xml_escapes_markup_and_replaces_what_it_cannot_carry() {
        assert_eq!(
            escaped(xml_escape, HOSTILE),
            "q&quot;b\\s/\u{fffd}\u{fffd}\u{fffd}\u{fffd}&#xA;&#xD;&#x9;\u{fffd}\u{7f}\u{85}\u{2028}\u{2029}&lt;&amp;&gt;&apos;\u{fffd}\u{fffd}\u{fdd0}é\u{1F600}"
        );
        // Every character the output holds is one XML 1.0 can carry, and no
        // markup character is left raw.
        let out = escaped(xml_escape, HOSTILE);
        assert!(out.chars().all(xml_char));
        assert!(!out.contains(['<', '>', '"', '\'']));
    }

    #[test]
    fn html_escapes_markup_and_replaces_controls() {
        assert_eq!(
            escaped(html_escape, "<script>alert('x')</script>&"),
            "&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;&amp;"
        );
        let out = escaped(html_escape, HOSTILE);
        assert!(!out.contains(['<', '>', '"', '\'']));
        assert!(out.chars().all(html_char), "{out:?}");
        assert!(
            out.contains("\n\u{fffd}\t"),
            "tab and line feed are kept, and the carriage return between them is not"
        );
    }

    #[test]
    fn numbers_are_three_places_trimmed_and_never_negative_zero() {
        assert_eq!(number(12.0).as_deref(), Some("12"));
        assert_eq!(number(0.1 + 0.2).as_deref(), Some("0.3"));
        assert_eq!(number(-0.0001).as_deref(), Some("0"));
        assert_eq!(number(-2.5).as_deref(), Some("-2.5"));
        assert_eq!(number(1234.5678).as_deref(), Some("1234.568"));
        assert_eq!(number(f64::NAN), None);
        assert_eq!(number(f64::INFINITY), None);
    }

    fn quad(x: f64, width: f64) -> Quad {
        Quad {
            ul: (x, 8.0),
            ur: (x + width, 8.0),
            ll: (x, -2.0),
            lr: (x + width, -2.0),
        }
    }

    fn ch(text: &str, x: f64, size: f64, font: Option<&Arc<str>>) -> TextChar {
        TextChar {
            text: text.to_string(),
            quad: quad(x, 5.0),
            size,
            origin: (x, 0.0),
            mcid: None,
            stream: 0,
            font: font.cloned(),
        }
    }

    fn line(chars: Vec<TextChar>) -> TextLine {
        TextLine {
            text: chars.iter().map(|c| c.text.as_str()).collect(),
            quad: quad(0.0, chars.len() as f64 * 5.0),
            chars,
            wmode: WritingMode::Horizontal,
            rtl: false,
            size: 12.0,
        }
    }

    /// A span changes where the font or the size does, and not otherwise.
    #[test]
    fn spans_split_at_a_font_or_a_size_change() {
        let regular: Arc<str> = Arc::from("Times-Roman");
        let bold: Arc<str> = Arc::from("Times-Bold");
        let l = line(vec![
            ch("a", 0.0, 12.0, Some(&regular)),
            ch("b", 5.0, 12.0 + 1e-9, Some(&regular)),
            ch("c", 10.0, 12.0, Some(&bold)),
            ch("d", 15.0, 9.0, Some(&bold)),
            ch("e", 20.0, 9.0, None),
        ]);
        let found: Vec<(Option<&str>, String)> = spans(&l)
            .iter()
            .map(|s| {
                (
                    s.font.map(|f| &**f),
                    s.chars.iter().map(|c| c.text.as_str()).collect(),
                )
            })
            .collect();
        assert_eq!(
            found,
            vec![
                (Some("Times-Roman"), "ab".to_string()),
                (Some("Times-Bold"), "c".to_string()),
                (Some("Times-Bold"), "d".to_string()),
                (None, "e".to_string()),
            ]
        );
    }

    fn hostile_page() -> TextPage {
        let font: Arc<str> = Arc::from("Evil\"<Font>&\u{1}");
        TextPage {
            blocks: vec![crate::text::TextBlock {
                quad: quad(0.0, 10.0),
                lines: vec![line(vec![
                    ch(HOSTILE, 0.0, 12.0, Some(&font)),
                    ch("x", 5.0, 12.0, None),
                ])],
            }],
            mcid_props: Default::default(),
            warnings: vec![
                TextWarning::UnknownFont {
                    name: "F<1>\"".to_string(),
                },
                TextWarning::UnmappedCode { code: 7 },
            ],
        }
    }

    const FRAME: PageFrame = PageFrame {
        index: 3,
        bounds: (0.0, 0.0, 612.0, 792.0),
        rotation: 90,
    };

    /// Whole documents in each format, over a page whose text and font name
    /// are hostile: nothing raw escapes into the markup.
    #[test]
    fn a_hostile_page_stays_inside_its_quotes_in_every_format() {
        let page = hostile_page();

        let json = page.serialize(TextFormat::Json, &FRAME);
        assert!(json.starts_with("{\"format\":\"tinker-pdf/text\",\"version\":1,\"pages\":["));
        assert!(
            json.contains("\"font\":\"Evil\\\"<Font>&\\u0001\""),
            "{json}"
        );
        assert!(json.contains("{\"kind\":\"unmapped-code\",\"code\":7}"));
        assert!(!json.chars().any(|c| u32::from(c) < 0x20 && c != '\n'));

        let xml = page.serialize(TextFormat::Xml, &FRAME);
        assert!(
            xml.contains(" font=\"Evil&quot;&lt;Font&gt;&amp;\u{fffd}\""),
            "{xml}"
        );
        assert!(xml.contains("<page index=\"3\" box=\"0 0 612 792\" rotation=\"90\">"));
        assert!(xml.chars().all(xml_char));
        assert!(xml.ends_with("</document>\n"));

        let html = page.serialize(TextFormat::Html, &FRAME);
        assert!(
            html.contains(" data-font=\"Evil&quot;&lt;Font&gt;&amp;\u{fffd}\""),
            "{html}"
        );
        assert!(!html.contains("<Font>"));
        assert!(html.contains("style=\"width:612pt;height:792pt\""));
    }

    #[test]
    fn a_writer_streams_pages_and_closes_once() {
        let page = hostile_page();
        let mut writer = TextWriter::new(TextFormat::Json);
        writer.page(&FRAME, &page);
        let first = writer.take();
        writer.page(&PageFrame { index: 4, ..FRAME }, &page);
        let rest = writer.finish();
        let whole = format!("{first}{rest}");
        assert_eq!(whole.matches("\"index\":").count(), 2);
        assert!(rest.starts_with(",\n{\"index\":4"), "{rest}");
        assert!(whole.ends_with("\n]}\n"));
    }

    #[test]
    fn an_empty_page_is_a_page_with_nothing_in_it() {
        let empty = TextPage::default();
        assert_eq!(
            empty.serialize(TextFormat::Json, &FRAME),
            "{\"format\":\"tinker-pdf/text\",\"version\":1,\"pages\":[\n\
             {\"index\":3,\"box\":[0,0,612,792],\"rotation\":90,\"blocks\":[],\"warnings\":[]}\n]}\n"
        );
    }
}
