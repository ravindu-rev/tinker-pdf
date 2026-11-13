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
///
/// Two values whose thousandths [`thousandths`] decides print alike exactly
/// when those are equal, `-0` with `0`; any other pair is written, each into a
/// buffer of its own, and the text compared. Nothing is allocated.
fn same_number(a: f64, b: f64) -> bool {
    if let (Some(a), Some(b)) = (signed_thousandths(a), signed_thousandths(b)) {
        return a == b;
    }
    let (mut one, mut other) = (OneNumber::new(), OneNumber::new());
    write_number(&mut one, a) == write_number(&mut other, b) && one.written() == other.written()
}

/// Where [`write_number`] writes: the output itself, which grows as it always
/// does, or a [`OneNumber`] on the stack to compare a number in. Neither
/// makes a `String` for a number.
trait Digits: core::fmt::Write {
    /// Everything written so far.
    fn written(&self) -> &[u8];
    /// Drops what was written past `len`, a length this writer had.
    fn cut(&mut self, len: usize);
}

impl Digits for String {
    fn written(&self) -> &[u8] {
        self.as_bytes()
    }

    fn cut(&mut self, len: usize) {
        // Only ever called inside text `write_number` wrote, which is ASCII,
        // so `len` is a character boundary.
        self.truncate(len);
    }
}

/// The longest a finite `f64` writes at three places: the largest has 309
/// digits before the point, and a sign, the point and three places beside.
const NUMBER_BYTES: usize = 320;

/// One number's text, held on the stack.
struct OneNumber {
    bytes: [u8; NUMBER_BYTES],
    len: usize,
}

impl OneNumber {
    fn new() -> OneNumber {
        OneNumber {
            bytes: [0; NUMBER_BYTES],
            len: 0,
        }
    }
}

impl core::fmt::Write for OneNumber {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(core::fmt::Error)?;
        let room = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        room.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

impl Digits for OneNumber {
    fn written(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or_default()
    }

    fn cut(&mut self, len: usize) {
        self.len = self.len.min(len);
    }
}

/// Writes `value` at three decimal places with trailing zeros dropped — the
/// bytes `format!("{value:.3}")` and that trim give — and says whether it was
/// finite; a value that is not writes nothing.
///
/// `-0` is written `0`: a coordinate that rounds to zero from below is not a
/// different place from one that rounds to it from above.
///
/// The digits are written from integer arithmetic wherever it decides the
/// rounding to a thousandth exactly ([`thousandths`]), so a character's ten
/// numbers cost no `String` apiece; a tie at the fourth decimal and a
/// magnitude of 2^52 or more are written by the formatter itself, into `out`,
/// and trimmed there.
fn write_number<D: Digits>(out: &mut D, value: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    match thousandths(value.abs()) {
        Some(0) => {
            let _ = out.write_char('0');
        }
        Some(thousandths) => {
            if value < 0.0 {
                let _ = out.write_char('-');
            }
            let _ = write!(out, "{}", thousandths / 1000);
            let places = thousandths % 1000;
            if places != 0 {
                let digits = [places / 100, places / 10 % 10, places % 10];
                let kept = if digits[2] != 0 {
                    3
                } else if digits[1] != 0 {
                    2
                } else {
                    1
                };
                let _ = out.write_char('.');
                for digit in digits.iter().take(kept) {
                    let _ = out.write_char(char::from(b'0' + *digit as u8));
                }
            }
        }
        None => {
            let start = out.written().len();
            let _ = write!(out, "{value:.3}");
            let text = out.written().get(start..).unwrap_or_default();
            let mut kept = text;
            if kept.contains(&b'.') {
                while let Some(rest) = kept.strip_suffix(b"0") {
                    kept = rest;
                }
                kept = kept.strip_suffix(b".").unwrap_or(kept);
            }
            let (len, negative_zero) = (kept.len(), kept == b"-0");
            out.cut(start + len);
            if negative_zero {
                out.cut(start);
                let _ = out.write_char('0');
            }
        }
    }
    true
}

/// `magnitude`, finite and not negative, in thousandths, rounded to the
/// nearest as `{:.3}` rounds it, wherever that is decided exactly; `None` for
/// a tie at the fourth decimal, which the formatter settles, and for a
/// magnitude of 2^52 or more.
///
/// A double is its significand over a power of two, so a thousand times it,
/// shifted down by that power, is its whole number of thousandths, and the
/// bits shifted out, against half of what the last place is worth, say which
/// way it rounds, with no rounding of its own.
fn thousandths(magnitude: f64) -> Option<u64> {
    let bits = magnitude.to_bits();
    let exponent = (bits >> 52) & 0x7ff;
    let fraction = bits & ((1 << 52) - 1);
    // `magnitude` is `significand / 2^shift`; a shift of 0 or less is a
    // magnitude of 2^52 or more.
    let (significand, shift) = match exponent {
        0 => (fraction, 1074),
        _ => (
            fraction | 1 << 52,
            1075u64.checked_sub(exponent).filter(|shift| *shift > 0)?,
        ),
    };
    // Under 2^53 a thousand times, so under 2^63: it cannot overflow.
    let scaled = significand * 1000;
    if shift >= 64 {
        // Under 2^63 against a half of 2^63 or more: it rounds to nothing.
        return Some(0);
    }
    let whole = scaled >> shift;
    let rest = scaled & ((1 << shift) - 1);
    let half = 1 << (shift - 1);
    match rest.cmp(&half) {
        core::cmp::Ordering::Less => Some(whole),
        core::cmp::Ordering::Greater => Some(whole + 1),
        core::cmp::Ordering::Equal => None,
    }
}

/// `value`'s thousandths with its sign, where [`thousandths`] decides them,
/// `-0` and `0` alike: the one integer the text written for it pictures.
fn signed_thousandths(value: f64) -> Option<i64> {
    if !value.is_finite() {
        return None;
    }
    let magnitude = i64::try_from(thousandths(value.abs())?).ok()?;
    Some(if value < 0.0 { -magnitude } else { magnitude })
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
    json_escape(out, text);
    out.push('"');
}

/// `text` escaped for the inside of a JSON string literal, a character at a
/// time, so the text of several pieces is written by escaping each.
fn json_escape(out: &mut String, text: &str) {
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
}

fn json_number(out: &mut String, value: f64) {
    if !write_number(out, value) {
        out.push_str("null");
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
    for (b, block) in page.blocks().iter().enumerate() {
        if b > 0 {
            out.push(',');
        }
        out.push_str("{\"bbox\":");
        json_bbox(out, block.quad.is_finite().then(|| block.quad.bounds()));
        out.push_str(",\"lines\":[");
        for (l, line) in block.lines.iter().enumerate() {
            if l > 0 {
                out.push(',');
            }
            out.push_str("{\"bbox\":");
            json_bbox(out, line.quad.is_finite().then(|| line.quad.bounds()));
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
                out.push_str(",\"text\":\"");
                for c in span.chars {
                    json_escape(out, &c.text);
                }
                out.push_str("\",\"chars\":[");
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
    // One unwritable number makes the whole list unwritable: a bbox with
    // three coordinates is not a bbox.
    if !values.iter().all(|value| value.is_finite()) {
        return;
    }
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    for (i, value) in values.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        write_number(out, *value);
    }
    out.push('"');
}

fn xml_page(out: &mut String, frame: &PageFrame, page: &TextPage) {
    let (x0, y0, x1, y1) = frame.bounds;
    let _ = write!(out, "<page index=\"{}\"", frame.index);
    xml_numbers(out, "box", &[x0, y0, x1, y1]);
    let _ = writeln!(out, " rotation=\"{}\">", frame.rotation);
    for block in page.blocks() {
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
                for c in span.chars {
                    xml_escape(out, &c.text);
                }
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
fn css_points(out: &mut String, value: f64) {
    if !write_number(out, value) {
        out.push('0');
    }
    out.push_str("pt");
}

fn html_page(out: &mut String, frame: &PageFrame, page: &TextPage) {
    let (x0, y0, x1, y1) = frame.bounds;
    let _ = write!(
        out,
        "<div class=\"page\" data-index=\"{}\" data-rotation=\"{}\" style=\"width:",
        frame.index, frame.rotation,
    );
    css_points(out, x1 - x0);
    out.push_str(";height:");
    css_points(out, y1 - y0);
    out.push_str("\">\n");
    for block in page.blocks() {
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
                "<div class=\"line\" data-wmode=\"{}\" data-rtl=\"{}\" style=\"left:",
                wmode(line.wmode),
                line.rtl,
            );
            css_points(out, lx0 - x0);
            out.push_str(";top:");
            css_points(out, y1 - ly1);
            out.push_str(";font-size:");
            css_points(out, line.size);
            out.push_str("\">");
            for span in spans(line) {
                out.push_str("<span");
                if let Some(font) = span.font {
                    out.push_str(" data-font=\"");
                    html_escape(out, font);
                    out.push('"');
                }
                if span.size.is_finite() {
                    out.push_str(" data-size=\"");
                    write_number(out, span.size);
                    out.push('"');
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

    /// What [`write_number`] writes for `value` into a `String` of its own,
    /// or `None` where it writes nothing.
    fn written(value: f64) -> Option<String> {
        let mut out = String::new();
        write_number(&mut out, value).then_some(out)
    }

    #[test]
    fn numbers_are_three_places_trimmed_and_never_negative_zero() {
        assert_eq!(written(12.0).as_deref(), Some("12"));
        assert_eq!(written(0.1 + 0.2).as_deref(), Some("0.3"));
        assert_eq!(written(-0.0001).as_deref(), Some("0"));
        assert_eq!(written(-2.5).as_deref(), Some("-2.5"));
        assert_eq!(written(1234.5678).as_deref(), Some("1234.568"));
        assert_eq!(written(f64::NAN), None);
        assert_eq!(written(f64::INFINITY), None);
    }

    /// The number this writer replaced, kept to hold [`write_number`] to:
    /// `format!` at three places and a trim, a `String` a number.
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

    /// `value`, finite and not negative, and the two doubles on either side
    /// of it that are finite.
    fn around(value: f64) -> Vec<f64> {
        let bits = value.to_bits();
        [
            bits.checked_sub(2),
            bits.checked_sub(1),
            Some(bits),
            bits.checked_add(1),
            bits.checked_add(2),
        ]
        .into_iter()
        .flatten()
        .map(f64::from_bits)
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect()
    }

    /// **P-02.6**: every number writes the bytes `format!` and the trim wrote
    /// — over every thousandth to 5 and the midpoints between them, the ties at
    /// the fourth decimal (an odd number of sixteenths, the only doubles a
    /// thousand times which ends in exactly a half) near zero and far out,
    /// every power of two from the smallest subnormal to the largest, the
    /// edge of the integer path at 2^52, the largest finite value, and seeded
    /// values, the doubles beside each of them, each signed both ways — and
    /// the ties are the formatter's to write.
    #[test]
    fn numbers_write_what_format_and_trim_wrote() {
        let mut values = Vec::new();
        for k in 0..5_000u32 {
            values.extend(around(f64::from(k) / 1000.0));
            values.extend(around(f64::from(2 * k + 1) / 2000.0));
        }
        let mut ties = Vec::new();
        for odd in (1..2_000u32).step_by(2) {
            ties.push(f64::from(odd) / 16.0);
        }
        for base in [1.0e3, 1.0e6, 1.0e9, 1.0e12, (1u64 << 40) as f64] {
            for odd in [1.0, 3.0, 5.0, 15.0] {
                ties.push(base + odd / 16.0);
            }
        }
        for tie in &ties {
            assert_eq!(
                thousandths(*tie),
                None,
                "{tie} is a tie at the fourth decimal, the formatter's to round"
            );
            values.extend(around(*tie));
        }
        for exponent in 1..2047u64 {
            values.extend(around(f64::from_bits(exponent << 52)));
        }
        for shift in 0..52 {
            values.extend(around(f64::from_bits(1 << shift)));
        }
        for edge in [(1u64 << 52) as f64, (1u64 << 53) as f64, f64::MAX] {
            values.extend(around(edge));
        }
        values.extend([0.0, f64::MIN_POSITIVE, 5e-324, 4.9e-4, 5e-4, 5.1e-4]);
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..50_000 {
            let any = f64::from_bits(next());
            if any.is_finite() {
                values.push(any.abs());
            }
            // A coordinate on a page, to a millionth of a point and past it.
            values.push((next() % 2_000_000_000) as f64 / 1.0e6);
        }
        let mut formatted = 0usize;
        for value in &values {
            for value in [*value, -*value] {
                assert_eq!(
                    written(value),
                    number(value),
                    "{value:e} ({:#018x})",
                    value.to_bits()
                );
                formatted += usize::from(thousandths(value.abs()).is_none());
            }
        }
        assert!(
            formatted > 2 * ties.len(),
            "the ties and the large magnitudes reach the formatter: {formatted}"
        );
        assert_eq!(written(-0.0).as_deref(), Some("0"));
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!((written(value), number(value)), (None, None));
        }
    }

    /// Two numbers are the same where they print the same, whichever way
    /// each is written: a tie the formatter rounds beside the two thousandths
    /// it may round to, `-0` and `0`, magnitudes past the integer path, and
    /// values that are not finite.
    #[test]
    fn same_number_is_what_prints_the_same() {
        let pairs = [
            (-0.0, 0.0, true),
            (-0.0004, 0.0004, true),
            (-0.0006, 0.0006, false),
            (1.0005e15, 1.0005e15, true),
            (1.0e300, 1.0e300, true),
            (1.0e300, 2.0e300, false),
            (12.0, 12.0001, true),
            (12.0, 12.001, false),
            (f64::NAN, f64::INFINITY, true),
            (f64::NAN, 0.0, false),
        ];
        for (a, b, same) in pairs {
            assert_eq!(same_number(a, b), same, "{a} against {b}");
            assert_eq!(same_number(b, a), same, "{b} against {a}");
            assert_eq!(same, number(a) == number(b), "{a} against {b}");
        }
        // A tie prints as one of the thousandths beside it, whichever the
        // formatter rounds it to, and is the same as that one alone.
        let tie = 0.0625;
        let beside = [0.062, 0.063].map(|other| same_number(tie, other));
        assert_eq!(beside.iter().filter(|same| **same).count(), 1, "{beside:?}");
        for other in [0.062, 0.063] {
            assert_eq!(same_number(tie, other), number(tie) == number(other));
        }
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
            stated: None,
            artifact: false,
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
        TextPage::new(
            vec![crate::text::TextBlock {
                quad: quad(0.0, 10.0),
                lines: vec![line(vec![
                    ch(HOSTILE, 0.0, 12.0, Some(&font)),
                    ch("x", 5.0, 12.0, None),
                ])],
            }],
            Default::default(),
            Vec::new(),
            vec![
                TextWarning::UnknownFont {
                    name: "F<1>\"".to_string(),
                },
                TextWarning::UnmappedCode { code: 7 },
            ],
        )
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
