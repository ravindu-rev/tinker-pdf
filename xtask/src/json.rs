//! Just enough JSON to write a report and read a ratchet back.
//!
//! Hand-rolled for the same reason the lock parser and the manifest reader
//! are: CONTRIBUTING rule 1 says no third-party crates, and a repository whose
//! own checks pulled a dependency in order to check the rules about
//! dependencies would be funny in a way nobody wants to explain.
//!
//! It is small on purpose. It parses the whole of JSON's grammar bar two
//! things it does not need and would only get subtly wrong: `\u` escapes
//! outside the basic plane, and numbers are read as `f64` and offered as
//! `i64` only when they are exactly integral. That last rule is deliberate —
//! the ratchet is integer arithmetic on counts, and a count that arrived as
//! `9.999999` must be refused rather than truncated to nine.

use std::collections::BTreeMap;
use std::fmt::Write as _;

/// A JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    /// Ordered by key, which is what makes a written report byte-stable
    /// between runs and therefore diffable.
    Object(BTreeMap<String, Json>),
}

impl Json {
    /// An object, from pairs.
    pub fn object<K: Into<String>>(pairs: impl IntoIterator<Item = (K, Json)>) -> Json {
        Json::Object(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// A string.
    pub fn string(text: impl Into<String>) -> Json {
        Json::String(text.into())
    }

    /// A count. `u64` rather than `f64` at the call site so that a count
    /// cannot be constructed from something fractional by accident.
    pub fn count(value: u64) -> Json {
        Json::Number(value as f64)
    }

    /// The value at a key, if this is an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(map) => map.get(key),
            _ => None,
        }
    }

    /// This value as a whole number, or `None` if it is not one.
    ///
    /// Integral rather than merely numeric: see the module note. A ratchet
    /// entry holding `12.5` is a corrupt ratchet, not a bar of twelve.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Number(n) if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 => Some(*n as u64),
            _ => None,
        }
    }

    /// This value as a string slice.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(text) => Some(text),
            _ => None,
        }
    }

    /// This value as a slice, if it is an array.
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Renders as indented JSON with a trailing newline.
    ///
    /// Pretty rather than compact because `ratchet.json` is committed, and a
    /// committed file whose diff is one very long line tells a reviewer
    /// nothing about which corpus moved.
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out.push('\n');
        out
    }

    fn write(&self, out: &mut String, indent: usize) {
        let pad = |out: &mut String, n: usize| {
            for _ in 0..n {
                out.push_str("  ");
            }
        };
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Number(n) => {
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 9e15 {
                    let _ = write!(out, "{}", *n as i64);
                } else if n.is_finite() {
                    let _ = write!(out, "{n}");
                } else {
                    // JSON has no infinity or NaN, and emitting a bare `NaN`
                    // would produce a file nothing can read back.
                    out.push_str("null");
                }
            }
            Json::String(text) => write_string(out, text),
            Json::Array(items) if items.is_empty() => out.push_str("[]"),
            Json::Array(items) => {
                out.push_str("[\n");
                for (index, item) in items.iter().enumerate() {
                    pad(out, indent + 1);
                    item.write(out, indent + 1);
                    if index + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                pad(out, indent);
                out.push(']');
            }
            Json::Object(map) if map.is_empty() => out.push_str("{}"),
            Json::Object(map) => {
                out.push_str("{\n");
                for (index, (key, value)) in map.iter().enumerate() {
                    pad(out, indent + 1);
                    write_string(out, key);
                    out.push_str(": ");
                    value.write(out, indent + 1);
                    if index + 1 < map.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                pad(out, indent);
                out.push('}');
            }
        }
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parses a JSON document, refusing trailing rubbish.
pub fn parse(text: &str) -> Result<Json, String> {
    let bytes: Vec<char> = text.chars().collect();
    let mut at = 0usize;
    let value = parse_value(&bytes, &mut at, 0)?;
    skip_space(&bytes, &mut at);
    if at != bytes.len() {
        return Err(format!(
            "character {at}: there is more text after the value ends"
        ));
    }
    Ok(value)
}

fn skip_space(text: &[char], at: &mut usize) {
    while text.get(*at).is_some_and(|c| c.is_whitespace()) {
        *at += 1;
    }
}

fn parse_value(text: &[char], at: &mut usize, depth: u32) -> Result<Json, String> {
    // A file of ten thousand `[` would otherwise recurse until the stack ends,
    // and a checker that aborts on its own input file is not a checker.
    if depth > 64 {
        return Err(format!("character {at}: nested more than 64 deep"));
    }
    skip_space(text, at);
    let Some(&ch) = text.get(*at) else {
        return Err("the text ends where a value was expected".to_string());
    };
    match ch {
        'n' => literal(text, at, "null", Json::Null),
        't' => literal(text, at, "true", Json::Bool(true)),
        'f' => literal(text, at, "false", Json::Bool(false)),
        '"' => parse_string(text, at).map(Json::String),
        '[' => {
            *at += 1;
            let mut items = Vec::new();
            skip_space(text, at);
            if text.get(*at) == Some(&']') {
                *at += 1;
                return Ok(Json::Array(items));
            }
            loop {
                items.push(parse_value(text, at, depth + 1)?);
                skip_space(text, at);
                match text.get(*at) {
                    Some(',') => *at += 1,
                    Some(']') => {
                        *at += 1;
                        return Ok(Json::Array(items));
                    }
                    _ => return Err(format!("character {at}: an array wants `,` or `]`")),
                }
            }
        }
        '{' => {
            *at += 1;
            let mut map = BTreeMap::new();
            skip_space(text, at);
            if text.get(*at) == Some(&'}') {
                *at += 1;
                return Ok(Json::Object(map));
            }
            loop {
                skip_space(text, at);
                let key = parse_string(text, at)?;
                skip_space(text, at);
                if text.get(*at) != Some(&':') {
                    return Err(format!("character {at}: a key wants `:` after it"));
                }
                *at += 1;
                let value = parse_value(text, at, depth + 1)?;
                // Refused rather than resolved: two values under one key means
                // the reader and the writer disagree about which one counts,
                // and silently taking either is how a ratchet reads the wrong
                // number.
                if map.insert(key.clone(), value).is_some() {
                    return Err(format!("character {at}: `{key}` is given twice"));
                }
                skip_space(text, at);
                match text.get(*at) {
                    Some(',') => *at += 1,
                    Some('}') => {
                        *at += 1;
                        return Ok(Json::Object(map));
                    }
                    _ => return Err(format!("character {at}: an object wants `,` or `}}`")),
                }
            }
        }
        '-' | '0'..='9' => parse_number(text, at),
        other => Err(format!("character {at}: `{other}` starts no value")),
    }
}

fn literal(text: &[char], at: &mut usize, word: &str, value: Json) -> Result<Json, String> {
    for expected in word.chars() {
        if text.get(*at) != Some(&expected) {
            return Err(format!("character {at}: `{word}` was expected"));
        }
        *at += 1;
    }
    Ok(value)
}

fn parse_string(text: &[char], at: &mut usize) -> Result<String, String> {
    if text.get(*at) != Some(&'"') {
        return Err(format!("character {at}: a string was expected"));
    }
    *at += 1;
    let mut out = String::new();
    loop {
        let Some(&ch) = text.get(*at) else {
            return Err("a string runs to the end of the text".to_string());
        };
        *at += 1;
        match ch {
            '"' => return Ok(out),
            '\\' => {
                let Some(&escape) = text.get(*at) else {
                    return Err("an escape runs to the end of the text".to_string());
                };
                *at += 1;
                match escape {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'u' => {
                        let mut code = 0u32;
                        for _ in 0..4 {
                            let Some(digit) = text.get(*at).and_then(|c| c.to_digit(16)) else {
                                return Err(format!("character {at}: \\u wants four hex digits"));
                            };
                            code = code * 16 + digit;
                            *at += 1;
                        }
                        // A lone surrogate is not a character. It becomes the
                        // replacement rather than an error: nothing this
                        // writes emits one, so meeting one means the file came
                        // from elsewhere and the shape still matters more.
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    }
                    other => return Err(format!("character {at}: `\\{other}` is not an escape")),
                }
            }
            other => out.push(other),
        }
    }
}

fn parse_number(text: &[char], at: &mut usize) -> Result<Json, String> {
    let start = *at;
    if text.get(*at) == Some(&'-') {
        *at += 1;
    }
    while text
        .get(*at)
        .is_some_and(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
    {
        *at += 1;
    }
    let raw: String = text[start..*at].iter().collect();
    raw.parse::<f64>()
        .map(Json::Number)
        .map_err(|_| format!("character {start}: `{raw}` is not a number"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_round_trips() {
        let value = Json::object([
            ("name", Json::string("pdfjs")),
            ("passed", Json::count(960)),
            ("total", Json::count(976)),
            ("complete", Json::Bool(true)),
            ("limits", Json::Array(vec![])),
            ("capabilities", Json::object([("jbig2", Json::count(12))])),
        ]);
        let text = value.to_pretty();
        assert_eq!(parse(&text).expect("it parses"), value);
    }

    /// A count is a count. A ratchet built by dividing floats and rounding is
    /// exactly the bug milestone 3 exists to prevent, so a fractional count
    /// must not read as a whole one anywhere.
    #[test]
    fn a_fractional_number_is_not_a_count() {
        assert_eq!(Json::Number(9.0).as_u64(), Some(9));
        assert_eq!(Json::Number(9.5).as_u64(), None);
        assert_eq!(Json::Number(-1.0).as_u64(), None);
        assert_eq!(Json::Number(f64::NAN).as_u64(), None);
    }

    #[test]
    fn malformations_are_rejected_with_a_position() {
        for text in [
            "{",
            "{\"a\"}",
            "{\"a\": }",
            "[1, ]",
            "nul",
            "{\"a\": 1} trailing",
            "{\"a\": 1, \"a\": 2}",
        ] {
            let error = parse(text).expect_err(&format!("`{text}` must be rejected"));
            assert!(!error.is_empty(), "`{text}` gave an empty message");
        }
    }

    #[test]
    fn strings_survive_the_characters_that_would_break_the_file() {
        let value = Json::string("a \"quoted\" \\ path\nwith a newline\tand a tab");
        let text = value.to_pretty();
        assert_eq!(parse(&text).expect("it parses"), value);
    }

    /// Ordered keys, so that two runs producing the same numbers produce the
    /// same bytes and a committed ratchet's diff is only what moved.
    #[test]
    fn objects_are_written_in_key_order() {
        let value = Json::object([
            ("zebra", Json::count(1)),
            ("apple", Json::count(2)),
            ("mango", Json::count(3)),
        ]);
        let text = value.to_pretty();
        let apple = text.find("apple").expect("apple");
        let mango = text.find("mango").expect("mango");
        let zebra = text.find("zebra").expect("zebra");
        assert!(apple < mango && mango < zebra, "{text}");
    }

    /// A deeply nested document must not take the stack with it.
    #[test]
    fn nesting_is_bounded_rather_than_fatal() {
        let text = "[".repeat(500);
        let error = parse(&text).expect_err("it is refused");
        assert!(error.contains("64 deep"), "{error}");
    }
}
