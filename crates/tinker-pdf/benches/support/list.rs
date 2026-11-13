//! The lists `baseline.json` commits, one a group: each operation's name,
//! its unit and what one iteration processes in it; and the operations
//! `wasm.json` names as ones a wasm target cannot run.
//!
//! The harness defines an operation the first time criterion calls its
//! routine, and criterion takes the figure it reports a rate in when the
//! operation starts, before that. So the harness hands criterion the figure
//! the list commits and holds what the group file declares to it once the
//! operation is defined (`support/mod.rs`). This file holds no criterion
//! type, so a test reaches it by path, as it reaches `inputs.rs`, beside
//! which it is always reached: it reads each committed file where
//! `inputs.rs`' [`super::inputs::in_tree`] says the process finds it, so a
//! wasm leg reads them through its runtime's directory grant.
//!
//! The reader is a whole JSON reader over each file, so what it hands back
//! depends on no key order, unlike `cargo xtask`'s, which cuts
//! `baseline.json` at its first keys and which `cargo xtask baseline` holds
//! the file to.

/// One operation of a group's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// Its name within the group.
    pub name: String,
    /// What an iteration processes: pages, pixels, bytes, outlines or none.
    pub unit: String,
    /// How much of its unit one iteration processes, every call included.
    pub per_iteration: u64,
}

/// The group `group`'s list, as the committed `benches/baseline.json` holds
/// it; a file that cannot be read, or that holds no such list, panics.
pub fn committed(group: &str) -> Vec<Listed> {
    let path = super::inputs::in_tree("benches/baseline.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the committed lists, {}: {error}", path.display()));
    list(&text, group).unwrap_or_else(|problem| panic!("{}: {problem}", path.display()))
}

/// The ids, `<group>/<name>`, of the operations the committed
/// `benches/wasm.json` excludes from the wasm legs, which a wasm build of
/// the harness leaves out; a file that cannot be read, or whose list does
/// not read, panics.
pub fn excluded_on_wasm() -> Vec<String> {
    let path = super::inputs::in_tree("benches/wasm.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("the wasm exclusions, {}: {error}", path.display()));
    excluded(&text).unwrap_or_else(|problem| panic!("{}: {problem}", path.display()))
}

/// The id of each entry of the `excluded` list in `text`, a `wasm.json`, in
/// its order, or what keeps it from being read. Why each is excluded is
/// `cargo xtask baseline`'s to hold; the harness needs the ids alone.
pub fn excluded(text: &str) -> Result<Vec<String>, String> {
    let root = Json::parse(text)?;
    let entries = root
        .get("excluded")
        .and_then(Json::array)
        .ok_or("the file holds no `excluded` list")?;
    entries
        .iter()
        .enumerate()
        .map(|(at, entry)| {
            entry
                .get("id")
                .and_then(Json::text)
                .map(str::to_string)
                .ok_or_else(|| format!("the `excluded` list's entry {at} has no `id` string"))
        })
        .collect()
}

/// The group `group`'s list in `text`, a `baseline.json`, or what keeps it
/// from being read.
pub fn list(text: &str, group: &str) -> Result<Vec<Listed>, String> {
    let root = Json::parse(text)?;
    let groups = root
        .get("groups")
        .and_then(Json::array)
        .ok_or("the file holds no `groups` list")?;
    let entry = groups
        .iter()
        .find(|entry| entry.get("group").and_then(Json::text) == Some(group))
        .ok_or_else(|| format!("the file holds no `{group}` list"))?;
    let operations = entry
        .get("operations")
        .and_then(Json::array)
        .ok_or_else(|| format!("the `{group}` list holds no `operations`"))?;
    operations
        .iter()
        .enumerate()
        .map(|(at, operation)| {
            let field = |key: &str| {
                operation
                    .get(key)
                    .ok_or_else(|| format!("the `{group}` list's entry {at} has no `{key}`"))
            };
            let text = |key: &str| {
                field(key)?.text().map(str::to_string).ok_or_else(|| {
                    format!("the `{group}` list's entry {at} has a `{key}` that is no string")
                })
            };
            Ok(Listed {
                name: text("name")?,
                unit: text("unit")?,
                per_iteration: field("per_iteration")?.count().ok_or_else(|| {
                    format!(
                        "the `{group}` list's entry {at} has a `per_iteration` that is no count"
                    )
                })?,
            })
        })
        .collect()
}

/// A JSON value.
enum Json {
    Null,
    Bool,
    Number(String),
    Text(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// The value `text` holds, whole.
    fn parse(text: &str) -> Result<Json, String> {
        let mut reader = Reader {
            bytes: text.as_bytes(),
            at: 0,
        };
        let value = reader.value()?;
        reader.space();
        if reader.at == reader.bytes.len() {
            Ok(value)
        } else {
            Err(format!(
                "the file goes on past its value, at byte {}",
                reader.at
            ))
        }
    }

    /// An object's value under `key`.
    fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// An array's values.
    fn array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(values) => Some(values),
            _ => None,
        }
    }

    /// A string's text.
    fn text(&self) -> Option<&str> {
        match self {
            Json::Text(text) => Some(text),
            _ => None,
        }
    }

    /// A number that is a whole count, written without sign, fraction or
    /// exponent.
    fn count(&self) -> Option<u64> {
        match self {
            Json::Number(digits) => digits.parse().ok(),
            _ => None,
        }
    }
}

/// RFC 8259's grammar over `bytes`, from `at`.
struct Reader<'t> {
    bytes: &'t [u8],
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn fault(&self, what: &str) -> String {
        format!("{what} at byte {}", self.at)
    }

    fn value(&mut self) -> Result<Json, String> {
        self.space();
        match self.bytes.get(self.at) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::Text),
            Some(b't') => self.word("true", Json::Bool),
            Some(b'f') => self.word("false", Json::Bool),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.fault("no value")),
        }
    }

    fn word(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.fault("no value"))
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.at += 1;
        let mut entries = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.space();
            if self.bytes.get(self.at) != Some(&b'"') {
                return Err(self.fault("no key"));
            }
            let key = self.string()?;
            self.space();
            if self.bytes.get(self.at) != Some(&b':') {
                return Err(self.fault("no `:` after a key"));
            }
            self.at += 1;
            entries.push((key, self.value()?));
            self.space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(entries));
                }
                _ => return Err(self.fault("no `,` or `}` in an object")),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.at += 1;
        let mut values = Vec::new();
        self.space();
        if self.bytes.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(Json::Array(values));
        }
        loop {
            values.push(self.value()?);
            self.space();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(values));
                }
                _ => return Err(self.fault("no `,` or `]` in an array")),
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        while matches!(
            self.bytes.get(self.at),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.at += 1;
        }
        let digits = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| self.fault("a number in other than ASCII"))?;
        if digits.parse::<f64>().is_err() {
            return Err(self.fault("no number"));
        }
        Ok(Json::Number(digits.to_string()))
    }

    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let start = self.at;
            while !matches!(self.bytes.get(self.at), None | Some(b'"' | b'\\')) {
                self.at += 1;
            }
            out.push_str(
                std::str::from_utf8(&self.bytes[start..self.at])
                    .map_err(|_| self.fault("a string in other than UTF-8"))?,
            );
            match self.bytes.get(self.at) {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.at += 1;
                    let escaped = match self.bytes.get(self.at) {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => self.unicode()?,
                        _ => return Err(self.fault("no escape after `\\`")),
                    };
                    self.at += 1;
                    out.push(escaped);
                }
                _ => return Err(self.fault("a string with no end")),
            }
        }
    }

    /// A `\u` escape's character, `at` left on its last digit: one code
    /// unit, or a surrogate pair written as two escapes.
    fn unicode(&mut self) -> Result<char, String> {
        let unit = self.hex4()?;
        let code = if (0xD800..0xDC00).contains(&unit) {
            if !self.bytes[self.at + 1..].starts_with(b"\\u") {
                return Err(self.fault("a high surrogate with no low one"));
            }
            self.at += 2;
            let low = self.hex4()?;
            if !(0xDC00..0xE000).contains(&low) {
                return Err(self.fault("a high surrogate with no low one"));
            }
            0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
        } else {
            unit
        };
        char::from_u32(code).ok_or_else(|| self.fault("an escape that is no character"))
    }

    /// The four hexadecimal digits after `at`, `at` left on the last.
    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.at + 1..self.at + 5)
            .and_then(|digits| std::str::from_utf8(digits).ok())
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or_else(|| self.fault("a `\\u` with no four hexadecimal digits"))?;
        self.at += 4;
        Ok(digits)
    }
}
