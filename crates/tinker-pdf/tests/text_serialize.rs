//! Structured text serialisation, held to its own output.
//!
//! The JSON is read back by the small parser at the bottom of this file —
//! hand-written, like the writer, because the tree carries no JSON crate and
//! none is wanted — so what is asserted is the *parsed* structure rather than
//! substrings of the text: a writer that forgot a comma or an escape fails to
//! parse here before any value is compared. The XML and HTML are checked for
//! the values the row names, fonts, sizes and boxes, on the same fixture.
//!
//! The fixture is `testdata/simple-text.pdf`: three pages, one line each,
//! Helvetica at 18 points on a 595 × 842 page.

use std::sync::Arc;

use tinker_pdf::{
    Document, PageFrame, Quad, TextBlock, TextChar, TextFormat, TextLine, TextPage, TextWarning,
    TextWriter, WritingMode, TEXT_FORMAT_VERSION,
};

fn fixture() -> Document {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/simple-text.pdf"
    );
    Document::open(std::fs::read(path).expect("the fixture")).expect("it opens")
}

fn whole_document(doc: &Document, format: TextFormat) -> String {
    let mut writer = TextWriter::new(format);
    let mut out = String::new();
    for page in doc.pages() {
        writer.page(&page.text_frame(), &page.text());
        out.push_str(&writer.take());
    }
    out.push_str(&writer.finish());
    out
}

#[test]
fn the_fixture_serialises_with_its_font_sizes_and_boxes() {
    let doc = fixture();
    let json = whole_document(&doc, TextFormat::Json);
    let root = parse(&json).unwrap_or_else(|e| panic!("the JSON does not parse: {e}\n{json}"));

    assert_eq!(
        root.get("format").and_then(Json::str),
        Some("tinker-pdf/text")
    );
    assert_eq!(
        root.get("version").and_then(Json::num),
        Some(f64::from(TEXT_FORMAT_VERSION))
    );
    let pages = root.get("pages").and_then(Json::arr).expect("pages");
    assert_eq!(pages.len(), 3);

    for (i, page) in pages.iter().enumerate() {
        assert_eq!(page.get("index").and_then(Json::num), Some(i as f64));
        assert_eq!(numbers(page.get("box")), vec![0.0, 0.0, 595.0, 842.0]);
        assert_eq!(page.get("rotation").and_then(Json::num), Some(0.0));
        assert_eq!(
            page.get("warnings").and_then(Json::arr).map(<[_]>::len),
            Some(0)
        );

        let blocks = page.get("blocks").and_then(Json::arr).expect("blocks");
        assert_eq!(blocks.len(), 1);
        let lines = blocks[0].get("lines").and_then(Json::arr).expect("lines");
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        let want = format!("Tinker fixture, page {} of 3", i + 1);
        assert_eq!(line.get("text").and_then(Json::str), Some(want.as_str()));
        assert_eq!(line.get("wmode").and_then(Json::str), Some("horizontal"));
        assert_eq!(line.get("rtl"), Some(&Json::Bool(false)));
        assert_eq!(line.get("size").and_then(Json::num), Some(18.0));

        let spans = line.get("spans").and_then(Json::arr).expect("spans");
        assert_eq!(spans.len(), 1, "one font, one size, one span");
        let span = &spans[0];
        assert_eq!(span.get("font").and_then(Json::str), Some("Helvetica"));
        assert_eq!(span.get("size").and_then(Json::num), Some(18.0));
        assert_eq!(span.get("text").and_then(Json::str), Some(want.as_str()));

        // The span's box sits where the content stream put the text — at
        // x = 72 on the baseline y = 742 — and inside the page.
        let bbox = numbers(span.get("bbox"));
        assert_eq!(bbox.len(), 4);
        assert!((bbox[0] - 72.0).abs() < 1e-9, "{bbox:?}");
        assert!(bbox[1] < 742.0 && bbox[3] > 742.0, "{bbox:?}");
        assert!(bbox[2] > bbox[0] && bbox[2] <= 595.0, "{bbox:?}");

        let chars = span.get("chars").and_then(Json::arr).expect("chars");
        assert_eq!(chars.len(), want.chars().count());
        for c in chars {
            assert_eq!(numbers(c.get("quad")).len(), 8);
            assert_eq!(numbers(c.get("origin")).len(), 2);
        }
        let spelled: String = chars
            .iter()
            .filter_map(|c| c.get("c").and_then(Json::str))
            .collect();
        assert_eq!(spelled, want);
    }
}

#[test]
fn xml_and_html_carry_the_same_font_and_size() {
    let doc = fixture();
    let xml = whole_document(&doc, TextFormat::Xml);
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<document "));
    assert_eq!(xml.matches("<page ").count(), 3);
    assert_eq!(
        xml.matches("<span font=\"Helvetica\" size=\"18\" bbox=\"72 ")
            .count(),
        3,
        "{xml}"
    );
    assert!(xml.contains("<page index=\"2\" box=\"0 0 595 842\" rotation=\"0\">"));
    assert!(xml.trim_end().ends_with("</document>"));

    let html = whole_document(&doc, TextFormat::Html);
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert_eq!(
        html.matches("<span data-font=\"Helvetica\" data-size=\"18\">Tinker fixture, page ")
            .count(),
        3,
        "{html}"
    );
    assert!(html.contains("style=\"width:595pt;height:842pt\""));
    // The line's box tops out at y = 756.4 in user space, which is 85.6
    // points below the top of an 842-point page in HTML's downward y.
    assert_eq!(
        html.matches("style=\"left:72pt;top:85.6pt;font-size:18pt\"")
            .count(),
        3,
        "{html}"
    );
}

/// JSON is the one lossless format, so it is held to that: hostile text and a
/// hostile font name come back from the parser exactly as they went in.
#[test]
fn hostile_text_round_trips_through_json_exactly() {
    let hostile =
        "\"\\/\u{0}\u{1}\u{8}\u{c}\n\r\t\u{1f}\u{7f}\u{2028}\u{2029}<&>'é\u{10FFFF}\u{1F600}";
    let font: Arc<str> = Arc::from("Evil\"}{\\Font\u{0}");
    let quad = Quad {
        ul: (0.0, 8.0),
        ur: (5.0, 8.0),
        ll: (0.0, -2.0),
        lr: (5.0, -2.0),
    };
    let page = TextPage {
        blocks: vec![TextBlock {
            quad,
            lines: vec![TextLine {
                chars: vec![TextChar {
                    text: hostile.to_string(),
                    quad,
                    size: 12.0,
                    origin: (0.0, 0.0),
                    mcid: None,
                    stream: 0,
                    font: Some(font.clone()),
                }],
                text: hostile.to_string(),
                quad,
                wmode: WritingMode::Vertical,
                rtl: true,
                size: 12.0,
            }],
        }],
        mcid_props: Default::default(),
        warnings: vec![TextWarning::UnknownFont {
            name: hostile.to_string(),
        }],
    };
    let frame = PageFrame {
        index: 0,
        bounds: (0.0, 0.0, 10.0, 10.0),
        rotation: 0,
    };
    let json = page.serialize(TextFormat::Json, &frame);
    let root = parse(&json).unwrap_or_else(|e| panic!("the JSON does not parse: {e}\n{json}"));
    let page = &root.get("pages").and_then(Json::arr).expect("pages")[0];
    let line = &page.get("blocks").and_then(Json::arr).expect("blocks")[0]
        .get("lines")
        .and_then(Json::arr)
        .expect("lines")[0];
    assert_eq!(line.get("text").and_then(Json::str), Some(hostile));
    assert_eq!(line.get("wmode").and_then(Json::str), Some("vertical"));
    assert_eq!(line.get("rtl"), Some(&Json::Bool(true)));
    let span = &line.get("spans").and_then(Json::arr).expect("spans")[0];
    assert_eq!(span.get("font").and_then(Json::str), Some(&*font));
    assert_eq!(span.get("text").and_then(Json::str), Some(hostile));
    let warning = &page.get("warnings").and_then(Json::arr).expect("warnings")[0];
    assert_eq!(
        warning.get("kind").and_then(Json::str),
        Some("unknown-font")
    );
    assert_eq!(warning.get("name").and_then(Json::str), Some(hostile));
}

/// A page whose `/F0` is Helvetica invoking a form whose own `/F0` is a font
/// with a hostile name: each glyph reports the font of the scope it was shown
/// in — which is why the name travels with the glyph rather than being looked
/// up afterwards from a resource name — and the name survives the trip.
#[test]
fn a_forms_own_font_is_the_one_its_text_reports() {
    let page_stream = "BT /F0 10 Tf 10 150 Td (page) Tj ET /X0 Do";
    let form_stream = "BT /F0 12 Tf 10 100 Td (form) Tj ET";
    let bytes = format!(
        "%PDF-1.7\n\
         1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
         2 0 obj\n<< /Type /Pages /Count 1 /Kids [3 0 R] >>\nendobj\n\
         3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] \
         /Resources << /Font << /F0 4 0 R >> /XObject << /X0 6 0 R >> >> /Contents 5 0 R >>\nendobj\n\
         4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n\
         5 0 obj\n<< /Length {} >>\nstream\n{page_stream}\nendstream\nendobj\n\
         6 0 obj\n<< /Type /XObject /Subtype /Form /BBox [0 0 200 200] \
         /Resources << /Font << /F0 7 0 R >> >> /Length {} >>\nstream\n{form_stream}\nendstream\nendobj\n\
         7 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Cour#22ier#3C#3E >>\nendobj\n\
         trailer\n<< /Size 8 /Root 1 0 R >>\n%%EOF\n",
        page_stream.len(),
        form_stream.len()
    );
    let doc = Document::open(bytes.into_bytes()).expect("it opens");
    let page = doc.page(0).expect("a page");
    let text = page.text();

    let fonts: Vec<(String, Option<String>)> = text
        .lines()
        .iter()
        .map(|l| {
            (
                l.text.clone(),
                l.chars
                    .first()
                    .and_then(|c| c.font.as_deref().map(str::to_string)),
            )
        })
        .collect();
    assert_eq!(
        fonts,
        vec![
            ("page".to_string(), Some("Helvetica".to_string())),
            ("form".to_string(), Some("Cour\"ier<>".to_string())),
        ]
    );

    let json = text.serialize(TextFormat::Json, &page.text_frame());
    let root = parse(&json).unwrap_or_else(|e| panic!("the JSON does not parse: {e}\n{json}"));
    let spans: Vec<(Option<String>, Option<f64>)> =
        root.get("pages").and_then(Json::arr).expect("pages")[0]
            .get("blocks")
            .and_then(Json::arr)
            .expect("blocks")
            .iter()
            .flat_map(|b| b.get("lines").and_then(Json::arr).unwrap_or_default())
            .flat_map(|l| l.get("spans").and_then(Json::arr).unwrap_or_default())
            .map(|s| {
                (
                    s.get("font").and_then(Json::str).map(str::to_string),
                    s.get("size").and_then(Json::num),
                )
            })
            .collect();
    assert_eq!(
        spans,
        vec![
            (Some("Helvetica".to_string()), Some(10.0)),
            (Some("Cour\"ier<>".to_string()), Some(12.0)),
        ]
    );
}

fn numbers(value: Option<&Json>) -> Vec<f64> {
    value
        .and_then(Json::arr)
        .map(|items| items.iter().filter_map(Json::num).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// A JSON parser (RFC 8259), small and strict, for reading the writer back.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }
    fn num(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }
    fn arr(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }
}

fn parse(text: &str) -> Result<Json, String> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = parser.value(0)?;
    parser.space();
    if parser.at != parser.bytes.len() {
        return Err(format!("trailing bytes at {}", parser.at));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("expected {:?} at {}", char::from(byte), self.at))
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes.get(self.at..self.at + word.len()) == Some(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(format!("bad literal at {}", self.at))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > 64 {
            return Err("nested too deep".to_string());
        }
        self.space();
        match self.peek() {
            Some(b'{') => {
                self.at += 1;
                let mut members = Vec::new();
                self.space();
                if self.peek() == Some(b'}') {
                    self.at += 1;
                    return Ok(Json::Object(members));
                }
                loop {
                    self.space();
                    let key = self.string()?;
                    self.space();
                    self.expect(b':')?;
                    let value = self.value(depth + 1)?;
                    members.push((key, value));
                    self.space();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {
                            self.at += 1;
                            return Ok(Json::Object(members));
                        }
                        _ => return Err(format!("expected , or }} at {}", self.at)),
                    }
                }
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.peek() == Some(b']') {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.space();
                    match self.peek() {
                        Some(b',') => self.at += 1,
                        Some(b']') => {
                            self.at += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => return Err(format!("expected , or ] at {}", self.at)),
                    }
                }
            }
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(format!("unexpected byte at {}", self.at)),
        }
    }

    /// RFC 8259 §6's grammar, checked, then read by the standard library.
    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        let digits = |p: &mut Parser<'_>| {
            let from = p.at;
            while matches!(p.peek(), Some(b'0'..=b'9')) {
                p.at += 1;
            }
            p.at - from
        };
        let int_start = self.at;
        let int = digits(self);
        if int == 0 || (int > 1 && self.bytes.get(int_start) == Some(&b'0')) {
            return Err(format!("bad integer part at {start}"));
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            if digits(self) == 0 {
                return Err(format!("bad fraction at {start}"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if digits(self) == 0 {
                return Err(format!("bad exponent at {start}"));
            }
        }
        let lexeme = std::str::from_utf8(&self.bytes[start..self.at]).map_err(|e| e.to_string())?;
        lexeme
            .parse::<f64>()
            .map(Json::Number)
            .map_err(|e| e.to_string())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.at..self.at + 4)
            .and_then(|d| std::str::from_utf8(d).ok())
            .ok_or_else(|| format!("short \\u escape at {}", self.at))?;
        let value = u32::from_str_radix(digits, 16).map_err(|e| e.to_string())?;
        self.at += 4;
        Ok(value)
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err("unterminated string".to_string());
            };
            match byte {
                b'"' => {
                    self.at += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.at += 1;
                    let escape = self.peek().ok_or("unterminated escape")?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                self.expect(b'\\')?;
                                self.expect(b'u')?;
                                let second = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return Err("unpaired surrogate".to_string());
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            out.push(char::from_u32(code).ok_or("lone surrogate")?);
                        }
                        other => return Err(format!("bad escape \\{}", char::from(other))),
                    }
                }
                // §7: every control character must be escaped.
                0x00..=0x1F => return Err(format!("raw control byte at {}", self.at)),
                _ => {
                    // One whole UTF-8 sequence; the input is a `str`, so it is
                    // well formed and its length is in its first byte.
                    let len = match byte {
                        0x00..=0x7F => 1,
                        0xC0..=0xDF => 2,
                        0xE0..=0xEF => 3,
                        _ => 4,
                    };
                    let piece = self
                        .bytes
                        .get(self.at..self.at + len)
                        .and_then(|p| std::str::from_utf8(p).ok())
                        .ok_or_else(|| format!("bad UTF-8 at {}", self.at))?;
                    out.push_str(piece);
                    self.at += len;
                }
            }
        }
    }
}

/// The parser rejects what RFC 8259 rejects, so that "it parsed" means
/// something about the writer.
#[test]
fn the_parser_is_strict_where_the_writer_could_go_wrong() {
    for bad in [
        "{\"a\":1,}",
        "[1 2]",
        "\"raw\u{1}control\"",
        "\"bad \\x escape\"",
        "01",
        "1.",
        "\"\\ud800\"",
        "{\"a\":1} x",
        "NaN",
    ] {
        assert!(parse(bad).is_err(), "{bad:?} parsed");
    }
    assert_eq!(
        parse("{\"a\":[1,-2.5,true,null,\"\\u00e9\\ud83d\\ude00\"]}"),
        Ok(Json::Object(vec![(
            "a".to_string(),
            Json::Array(vec![
                Json::Number(1.0),
                Json::Number(-2.5),
                Json::Bool(true),
                Json::Null,
                Json::String("é\u{1F600}".to_string()),
            ])
        )]))
    );
}
