//! html5lib's tokenizer tests, run against §13.2.5 alone.
//!
//! `data/html5lib-tests/tokenizer/*.test` is the tokenizer half of the suite
//! `tests/html5lib.rs` holds the tree builder to: each test an input, the
//! state the tokenizer starts in, and the tokens it must emit. The tokenizer
//! is not public — nothing outside this crate has a use for tokens without a
//! tree — so the suite runs here, as a unit test, over the vendored files.
//!
//! The files are JSON, and the crate has no dependencies, so the reader below
//! is a JSON reader of the kind a test needs: the six value types, string
//! escapes with surrogate pairs, and nothing it would have to be lenient
//! about. Adjacent character tokens are joined before comparing, as the
//! suite's README says a consumer must, and so are the runs this tokenizer
//! emits. Parse errors are not compared: a test passes on its tokens.
//!
//! **Not attempted, by name:** the four `doubleEscaped` runs whose input or
//! output holds a lone surrogate, which a Rust `str` cannot hold — HTML's
//! tokenizer meets a lone surrogate only through script, which this crate
//! never runs.

use super::tokenizer::{State, Token, Tokenizer};
use crate::Limits;

#[derive(Clone, Debug, PartialEq)]
enum Json {
    Null,
    Bool(bool),
    Number,
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    fn array(&self) -> &[Json] {
        match self {
            Json::Array(items) => items,
            _ => &[],
        }
    }
}

struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while self.text.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        self.space();
        if self.text.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Json {
        self.space();
        match self.text.get(self.at) {
            Some(b'{') => {
                self.at += 1;
                let mut fields = Vec::new();
                if self.eat(b'}') {
                    return Json::Object(fields);
                }
                loop {
                    self.space();
                    let Json::Str(key) = self.value() else {
                        panic!("an object key that is not a string at {}", self.at);
                    };
                    assert!(self.eat(b':'), "a key with no colon at {}", self.at);
                    let value = self.value();
                    fields.push((key, value));
                    if self.eat(b'}') {
                        return Json::Object(fields);
                    }
                    assert!(
                        self.eat(b','),
                        "an object that does not continue at {}",
                        self.at
                    );
                }
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                if self.eat(b']') {
                    return Json::Array(items);
                }
                loop {
                    items.push(self.value());
                    if self.eat(b']') {
                        return Json::Array(items);
                    }
                    assert!(
                        self.eat(b','),
                        "an array that does not continue at {}",
                        self.at
                    );
                }
            }
            Some(b'"') => {
                self.at += 1;
                Json::Str(self.string())
            }
            Some(b't') => {
                self.at += 4;
                Json::Bool(true)
            }
            Some(b'f') => {
                self.at += 5;
                Json::Bool(false)
            }
            Some(b'n') => {
                self.at += 4;
                Json::Null
            }
            _ => {
                while self.text.get(self.at).is_some_and(|b| {
                    b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E')
                }) {
                    self.at += 1;
                }
                Json::Number
            }
        }
    }

    /// A string's body, escapes resolved. A lone surrogate becomes U+FFFF
    /// with a marker the caller looks for, because a `String` cannot hold one.
    fn string(&mut self) -> String {
        let mut units: Vec<u16> = Vec::new();
        loop {
            let byte = *self.text.get(self.at).expect("a string that never closes");
            self.at += 1;
            match byte {
                b'"' => break,
                b'\\' => {
                    let escape = *self.text.get(self.at).expect("an escape");
                    self.at += 1;
                    match escape {
                        b'n' => units.push(0x0A),
                        b't' => units.push(0x09),
                        b'r' => units.push(0x0D),
                        b'b' => units.push(0x08),
                        b'f' => units.push(0x0C),
                        b'u' => {
                            let hex = std::str::from_utf8(&self.text[self.at..self.at + 4])
                                .expect("four hex digits");
                            units.push(u16::from_str_radix(hex, 16).expect("hex"));
                            self.at += 4;
                        }
                        other => units.push(u16::from(other)),
                    }
                }
                _ => {
                    // A run of UTF-8: find its end and re-encode as UTF-16.
                    let start = self.at - 1;
                    let mut end = self.at;
                    while self.text.get(end).is_some_and(|&b| b != b'"' && b != b'\\') {
                        end += 1;
                    }
                    let run = std::str::from_utf8(&self.text[start..end]).expect("UTF-8");
                    units.extend(run.encode_utf16());
                    self.at = end;
                }
            }
        }
        decode_units(&units)
    }
}

/// UTF-16 to a string, a lone surrogate written as the marker `\u{FFFF}`
/// followed by U+FFFE so a test that holds one is recognisable.
fn decode_units(units: &[u16]) -> String {
    char::decode_utf16(units.iter().copied())
        .map(|unit| match unit {
            Ok(c) => c.to_string(),
            Err(_) => LONE.to_owned(),
        })
        .collect()
}

const LONE: &str = "\u{FFFF}\u{FFFE}";

/// `doubleEscaped`'s second pass: `\uXXXX` in the text itself.
fn unescape_twice(text: &str) -> String {
    let mut units: Vec<u16> = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("\\u") {
        units.extend(rest[..at].encode_utf16());
        let hex = rest.get(at + 2..at + 6).unwrap_or("");
        match u16::from_str_radix(hex, 16) {
            Ok(unit) if hex.len() == 4 => {
                units.push(unit);
                rest = &rest[at + 6..];
            }
            _ => {
                units.extend("\\u".encode_utf16());
                rest = &rest[at + 2..];
            }
        }
    }
    units.extend(rest.encode_utf16());
    decode_units(&units)
}

/// A token as the suite writes one.
#[derive(Debug, PartialEq)]
enum Expected {
    Doctype(Option<String>, Option<String>, Option<String>, bool),
    Start(String, Vec<(String, String)>, bool),
    End(String),
    Comment(String),
    Characters(String),
}

fn expected(output: &[Json], double: bool) -> Vec<Expected> {
    let text = |value: &Json| {
        let raw = value.str().unwrap_or("").to_owned();
        if double {
            unescape_twice(&raw)
        } else {
            raw
        }
    };
    let optional = |value: Option<&Json>| match value {
        Some(Json::Null) | None => None,
        Some(v) => Some(text(v)),
    };
    let mut out: Vec<Expected> = Vec::new();
    for token in output {
        let fields = token.array();
        let kind = fields.first().and_then(Json::str).unwrap_or("");
        let made = match kind {
            "DOCTYPE" => Expected::Doctype(
                optional(fields.get(1)),
                optional(fields.get(2)),
                optional(fields.get(3)),
                fields.get(4) == Some(&Json::Bool(true)),
            ),
            "StartTag" => {
                let mut attributes: Vec<(String, String)> = match fields.get(2) {
                    Some(Json::Object(pairs)) => {
                        pairs.iter().map(|(k, v)| (k.clone(), text(v))).collect()
                    }
                    _ => Vec::new(),
                };
                attributes.sort();
                Expected::Start(
                    text(fields.get(1).unwrap_or(&Json::Null)),
                    attributes,
                    fields.get(3) == Some(&Json::Bool(true)),
                )
            }
            "EndTag" => Expected::End(text(fields.get(1).unwrap_or(&Json::Null))),
            "Comment" => Expected::Comment(text(fields.get(1).unwrap_or(&Json::Null))),
            _ => Expected::Characters(text(fields.get(1).unwrap_or(&Json::Null))),
        };
        push_joined(&mut out, made);
    }
    out
}

fn push_joined(out: &mut Vec<Expected>, token: Expected) {
    if let (Some(Expected::Characters(last)), Expected::Characters(more)) = (out.last_mut(), &token)
    {
        last.push_str(more);
        return;
    }
    out.push(token);
}

fn tokens(input: &str, state: State, last_start_tag: Option<&str>) -> Vec<Expected> {
    let input = super::normalise_newlines(input);
    let mut tokenizer = Tokenizer::new(&input, &Limits::DEFAULT);
    tokenizer.state = state;
    if let Some(name) = last_start_tag {
        tokenizer.set_last_start_tag(name);
    }
    let mut out = Vec::new();
    loop {
        let made = match tokenizer.next_token() {
            Token::Eof => break,
            Token::Doctype(d) => {
                Expected::Doctype(d.name, d.public_id, d.system_id, !d.force_quirks)
            }
            Token::StartTag(tag) => {
                let mut attributes = tag.attributes;
                attributes.sort();
                Expected::Start(tag.name, attributes, tag.self_closing)
            }
            Token::EndTag(tag) => Expected::End(tag.name),
            Token::Comment(text) => Expected::Comment(text),
            Token::Characters(text) => Expected::Characters(text),
            Token::Null => Expected::Characters("\0".to_owned()),
        };
        push_joined(&mut out, made);
    }
    out
}

fn state_named(name: &str) -> Option<State> {
    Some(match name {
        "Data state" => State::Data,
        "PLAINTEXT state" => State::Plaintext,
        "RCDATA state" => State::Rcdata,
        "RAWTEXT state" => State::Rawtext,
        "Script data state" => State::ScriptData,
        "CDATA section state" => State::CdataSection,
        _ => return None,
    })
}

/// Tests the vendored files hold, runs — a test with several initial states
/// is a run per state — and the runs not attempted.
const TESTS: usize = 6_806;
const RUNS: usize = 7_032;
const NOT_ATTEMPTED: usize = 4;

/// **The floor**: runs whose tokens are the suite's, measured 3 October 2026 —
/// every run attempted.
const PASSING: usize = 7_028;

#[test]
fn the_tokenizer_emits_html5libs_tokens() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/html5lib-tests/tokenizer");
    let mut files: Vec<_> = std::fs::read_dir(&directory)
        .expect("the vendored suite is in the tree")
        .map(|e| e.expect("an entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "test"))
        .collect();
    files.sort();
    let (mut tests, mut runs, mut skipped, mut passed) = (0, 0, 0, 0);
    let mut failures = Vec::new();
    for path in files {
        let text = std::fs::read(&path).expect("readable");
        let json = Reader { text: &text, at: 0 }.value();
        let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        for test in json.get("tests").map(Json::array).unwrap_or(&[]) {
            tests += 1;
            let double = test.get("doubleEscaped") == Some(&Json::Bool(true));
            let raw = test.get("input").and_then(Json::str).unwrap_or("");
            let input = if double {
                unescape_twice(raw)
            } else {
                raw.to_owned()
            };
            let want = expected(test.get("output").map(Json::array).unwrap_or(&[]), double);
            let lone = input.contains(LONE)
                || want
                    .iter()
                    .any(|t| format!("{t:?}").contains("\\u{ffff}\\u{fffe}"));
            let states: Vec<&str> = match test.get("initialStates") {
                Some(Json::Array(states)) => states.iter().filter_map(Json::str).collect(),
                _ => vec!["Data state"],
            };
            let last = test.get("lastStartTag").and_then(Json::str);
            for name in states {
                runs += 1;
                if lone {
                    skipped += 1;
                    continue;
                }
                let state = state_named(name).expect("a state the suite names");
                let got = tokens(&input, state, last);
                if got == want {
                    passed += 1;
                } else {
                    failures.push(format!(
                        "{file}: {:?} in {name}: {input:?}\n  want {want:?}\n  got  {got:?}",
                        test.get("description").and_then(Json::str).unwrap_or("")
                    ));
                }
            }
        }
    }
    if std::env::var_os("HTML5LIB_SHOW").is_some() {
        for failure in &failures {
            println!("{failure}");
        }
    }
    println!(
        "html5lib tokenizer: {passed} of {} runs pass ({skipped} not attempted)",
        runs - skipped
    );
    assert_eq!(tests, TESTS);
    assert_eq!(runs, RUNS);
    assert_eq!(skipped, NOT_ATTEMPTED);
    assert_eq!(passed, PASSING, "{} failures", failures.len());
}
