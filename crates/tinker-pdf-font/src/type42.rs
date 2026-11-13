//! Type 42: a TrueType face inside a PostScript font dictionary.
//!
//! Adobe Technical Note #5012, *The Type 42 Font Format Specification*, is the
//! format, and `docs/design/type42-and-multiple-master.md` the design: the
//! dictionary of section 2.2 — `/FontType 42`, `/FontMatrix`, `/Encoding`,
//! `/CharStrings` and the `/sfnts` strings — read **structurally**, by the
//! definitions section 7's example makes. The program is PostScript and this
//! is no interpreter, so nothing is run: a definition is found where the
//! format puts it, and anything the reader cannot place is declined by name.
//!
//! The `/sfnts` strings, concatenated, are the face, and it is handed to
//! [`Sfnt::parse`]: what a Type 42 program draws is its TrueType face's
//! `glyf` outlines, as `/FontFile2`'s are drawn. Bytes in, values out; no PDF
//! type in the interface (ruling 8).

use std::collections::BTreeMap;

use crate::encoding::{base_glyph_name, BaseEncoding};
use crate::sfnt::Sfnt;

/// The most `/CharStrings` entries a wrapper is read with: one a glyph index,
/// and a face has no more than 65 536 glyphs.
pub const MAX_CHARSTRINGS: usize = 65_536;

/// How deep the reader follows the procedures, arrays and dictionaries it
/// passes over — a `/CharStrings` entry that is a procedure, the `for` loop an
/// `/Encoding` is cleared with.
pub const MAX_NESTING: usize = 64;

/// The first line TN #5012 section 2.1 requires the start of.
const FIRST_LINE: &[u8] = b"%!PS-TrueTypeFont";

/// `gdir`: a face whose glyphs are in a `/GlyphDirectory` rather than in
/// `glyf` (TN #5012 section 5.5).
const TAG_GDIR: u32 = u32::from_be_bytes(*b"gdir");

/// Why a program was declined, by the structure that refused it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Type42Refusal {
    /// No `/FontType 42` (Table 1): not a Type 42 program.
    NotType42,
    /// A `CIDFontType 2` CIDFont (TN #5012 section 5), with its `/CIDMap`:
    /// refused by name, a non-goal of the design.
    CidKeyed,
    /// `/sfnts` is missing, is no array, does not close, or holds something
    /// other than hexadecimal and binary strings.
    Sfnts,
    /// A string that does not close (`>`, `)`), or a binary string with fewer
    /// bytes than its `RD` declares.
    StringTruncated,
    /// A hexadecimal string holding a character that is neither a hex digit
    /// nor white space.
    HexString,
    /// `/CharStrings` is missing, is no dictionary, does not close, or holds
    /// an entry that is neither a glyph index nor a procedure.
    CharStrings,
    /// `/CharStrings` holds more than [`MAX_CHARSTRINGS`] entries.
    TooManyCharStrings,
    /// `/Encoding` does not close, or holds something no encoding is.
    Encoding,
    /// `/FontMatrix` is not a literal array of six finite numbers.
    FontMatrix,
    /// A procedure, array or dictionary the reader passes over nests deeper
    /// than [`MAX_NESTING`].
    Nesting,
    /// A procedure, array or dictionary the reader passes over does not
    /// close.
    Unclosed,
    /// The face the strings reassemble is no sfnt [`Sfnt::parse`] reads.
    Face,
    /// The face's glyphs are in a `/GlyphDirectory`, a `gdir` table standing
    /// in for `glyf` (TN #5012 section 5.5): refused by name.
    GlyphDirectory,
}

/// A program's built-in encoding: code to glyph name.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Encoding {
    /// `StandardEncoding`.
    Standard,
    /// An array, by code; `None` for `.notdef` or a code it does not set.
    Codes(Vec<Option<Vec<u8>>>),
}

/// A Type 42 program, read.
#[derive(Clone, Debug)]
pub struct Type42 {
    /// The `/sfnts` strings, each without its padding byte, concatenated.
    face: Vec<u8>,
    /// `/CharStrings`, in the order written: glyph name and glyph index.
    charstrings: Vec<(Vec<u8>, u16)>,
    /// The first index each name was given.
    by_name: BTreeMap<Vec<u8>, u16>,
    /// How many `/CharStrings` entries were procedures, passed over.
    procedures: usize,
    encoding: Option<Encoding>,
    font_matrix: Option<[f64; 6]>,
}

impl Type42 {
    /// Whether `data` is a Type 42 program as the design recognises one:
    /// after leading white space it begins with `%!PS-TrueTypeFont` (TN #5012
    /// section 2.1), or its first `/FontType` definition is 42 (Table 1).
    ///
    /// Asked before a program is handed to the Type 1 parser, which finds
    /// `eexec` anywhere in its input — and a binary `/sfnts` string can hold
    /// those five bytes.
    #[must_use]
    pub fn is_type42(data: &[u8]) -> bool {
        let start = data
            .iter()
            .position(|b| !is_white(*b))
            .unwrap_or(data.len());
        if data[start..].starts_with(FIRST_LINE) {
            return true;
        }
        let mut scanner = Scanner::new(data);
        let mut previous_was_font_type = false;
        while let Some(token) = scanner.scan() {
            let Ok(token) = token else {
                return false;
            };
            if previous_was_font_type {
                return matches!(token, Token::Integer(42));
            }
            previous_was_font_type = token == Token::Name(b"FontType");
            if token == Token::Executable(b"eexec") {
                return false;
            }
        }
        false
    }

    /// Reads a Type 42 program: the dictionary TN #5012 section 2.2 lays out
    /// and the face its `/sfnts` strings reassemble.
    ///
    /// # Errors
    ///
    /// The [`Type42Refusal`] naming the first structure that could not be
    /// read; never a panic, and never a byte read past a string.
    pub fn parse(data: &[u8]) -> Result<Type42, Type42Refusal> {
        let tokens = tokens(data)?;

        match value_after(&tokens, b"FontType") {
            Some(Token::Integer(42)) => {}
            _ => return Err(Type42Refusal::NotType42),
        }
        if value_after(&tokens, b"CIDFontType").is_some()
            || value_after(&tokens, b"CIDMap").is_some()
        {
            return Err(Type42Refusal::CidKeyed);
        }

        let font_matrix = font_matrix(&tokens)?;
        let encoding = encoding(&tokens)?;
        let face = sfnts(&tokens)?;
        let (charstrings, procedures) = charstrings(&tokens)?;

        let sfnt = Sfnt::parse(&face).ok_or(Type42Refusal::Face)?;
        if sfnt.table(TAG_GDIR).is_some() {
            return Err(Type42Refusal::GlyphDirectory);
        }

        let mut by_name = BTreeMap::new();
        for (name, glyph) in &charstrings {
            by_name.entry(name.clone()).or_insert(*glyph);
        }
        Ok(Type42 {
            face,
            charstrings,
            by_name,
            procedures,
            encoding,
            font_matrix,
        })
    }

    /// The face the `/sfnts` strings reassemble, each string without the
    /// padding byte TN #5012 section 4.2 appends to an odd one.
    #[must_use]
    pub fn face(&self) -> &[u8] {
        &self.face
    }

    /// The face, parsed. [`Type42::parse`] has already read it once, so this
    /// answers `Some` for every program it returned.
    #[must_use]
    pub fn sfnt(&self) -> Option<Sfnt<'_>> {
        Sfnt::parse(&self.face)
    }

    /// The glyph index `/CharStrings` gives a name; the first entry written
    /// wins where a name is given twice.
    #[must_use]
    pub fn glyph_for_name(&self, name: &[u8]) -> Option<u16> {
        self.by_name.get(name).copied()
    }

    /// Every `/CharStrings` entry that is a glyph index, in the order
    /// written.
    pub fn glyph_names(&self) -> impl Iterator<Item = (&[u8], u16)> {
        self.charstrings
            .iter()
            .map(|(name, glyph)| (name.as_slice(), *glyph))
    }

    /// How many `/CharStrings` entries are glyph indices.
    #[must_use]
    pub fn charstring_count(&self) -> usize {
        self.charstrings.len()
    }

    /// How many `/CharStrings` entries were procedures, which no outline here
    /// reaches (TN #5012 section 4.3), and were passed over.
    #[must_use]
    pub fn procedure_count(&self) -> usize {
        self.procedures
    }

    /// `.notdef`'s glyph, which Table 3 requires `/CharStrings` to name, or
    /// glyph 0 where it names none.
    #[must_use]
    pub fn notdef(&self) -> u16 {
        self.glyph_for_name(b".notdef").unwrap_or(0)
    }

    /// The glyph name the program's built-in `/Encoding` gives a code: `None`
    /// for a program with no `/Encoding` this reader reads, and for a code it
    /// leaves at `.notdef`.
    #[must_use]
    pub fn encoding_name(&self, code: u8) -> Option<&[u8]> {
        match self.encoding.as_ref()? {
            Encoding::Standard => base_glyph_name(BaseEncoding::Standard, code).map(str::as_bytes),
            Encoding::Codes(codes) => codes.get(usize::from(code))?.as_deref(),
        }
    }

    /// The glyph a code chooses through the built-in `/Encoding` and then
    /// `/CharStrings` — TN #5012 section 4.5's route.
    #[must_use]
    pub fn glyph_for_code(&self, code: u8) -> Option<u16> {
        self.glyph_for_name(self.encoding_name(code)?)
    }

    /// `/FontMatrix`, where the program states one. TN #5012 section 2.3
    /// makes it the identity; it does not move the drawing (the design).
    #[must_use]
    pub fn font_matrix(&self) -> Option<[f64; 6]> {
        self.font_matrix
    }
}

/// PostScript's white-space characters.
const fn is_white(b: u8) -> bool {
    matches!(b, b'\0' | b'\t' | b'\n' | b'\x0C' | b'\r' | b' ')
}

/// PostScript's delimiters, besides white space.
const fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// One PostScript token, as the reader needs it.
#[derive(Clone, Debug, PartialEq)]
enum Token<'a> {
    /// `/name`.
    Name(&'a [u8]),
    /// An executable name or operator: `def`, `begin`, `StandardEncoding`.
    Executable(&'a [u8]),
    Integer(i64),
    Real(f64),
    /// The bytes between `<` and `>`, still hexadecimal.
    Hex(&'a [u8]),
    /// The bytes after `<n> RD` and its one space.
    Binary(&'a [u8]),
    /// A literal string, `(…)`, which nothing here reads.
    Text,
    /// `[`, `{` or `<<`.
    Open(u8),
    /// `]`, `}` or `>>`.
    Close(u8),
}

/// A PostScript scanner over one program's bytes.
struct Scanner<'a> {
    data: &'a [u8],
    at: usize,
    /// The integer just scanned, which an `RD` or `-|` takes as its count.
    count: Option<i64>,
}

impl<'a> Scanner<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            at: 0,
            count: None,
        }
    }

    /// The next token, or the refusal a malformed one earns; `None` at the
    /// end of the program.
    fn scan(&mut self) -> Option<Result<Token<'a>, Type42Refusal>> {
        loop {
            let b = *self.data.get(self.at)?;
            if is_white(b) {
                self.at += 1;
                continue;
            }
            if b == b'%' {
                while self
                    .data
                    .get(self.at)
                    .is_some_and(|b| *b != b'\n' && *b != b'\r')
                {
                    self.at += 1;
                }
                continue;
            }
            break;
        }
        let token = self.token();
        self.count = match &token {
            Ok(Token::Integer(n)) => Some(*n),
            _ => None,
        };
        Some(token)
    }

    fn token(&mut self) -> Result<Token<'a>, Type42Refusal> {
        let data = self.data;
        // In range: `scan` reads the byte at `at` before it calls this.
        let b = data[self.at];
        match b {
            b'[' | b'{' => {
                self.at += 1;
                Ok(Token::Open(b))
            }
            b']' | b'}' => {
                self.at += 1;
                Ok(Token::Close(b))
            }
            b'<' if data.get(self.at + 1) == Some(&b'<') => {
                self.at += 2;
                Ok(Token::Open(b'<'))
            }
            b'>' if data.get(self.at + 1) == Some(&b'>') => {
                self.at += 2;
                Ok(Token::Close(b'>'))
            }
            b'<' => {
                let start = self.at + 1;
                let end = data[start..]
                    .iter()
                    .position(|b| *b == b'>')
                    .map(|at| start + at)
                    .ok_or(Type42Refusal::StringTruncated)?;
                let body = &data[start..end];
                if !body.iter().all(|b| b.is_ascii_hexdigit() || is_white(*b)) {
                    return Err(Type42Refusal::HexString);
                }
                self.at = end + 1;
                Ok(Token::Hex(body))
            }
            b'>' | b')' => Err(Type42Refusal::StringTruncated),
            b'(' => {
                self.literal_string()?;
                Ok(Token::Text)
            }
            b'/' => {
                let start = self.at + 1;
                let end = self.regular_end(start);
                self.at = end;
                Ok(Token::Name(&data[start..end]))
            }
            _ => {
                let start = self.at;
                let end = self.regular_end(start);
                self.at = end;
                let word = &data[start..end];
                if let Some(number) = number(word) {
                    return Ok(number);
                }
                if word == b"RD" || word == b"-|" {
                    if let Some(count) = self.count {
                        return self.binary(count);
                    }
                }
                Ok(Token::Executable(word))
            }
        }
    }

    /// Where a run of regular characters from `start` ends.
    fn regular_end(&self, start: usize) -> usize {
        self.data[start..]
            .iter()
            .position(|b| is_white(*b) || is_delimiter(*b))
            .map_or(self.data.len(), |at| start + at)
    }

    /// A `(…)` string: balanced parentheses, a backslash escaping the byte
    /// after it.
    fn literal_string(&mut self) -> Result<(), Type42Refusal> {
        let mut depth = 0usize;
        while let Some(b) = self.data.get(self.at) {
            self.at += 1;
            match b {
                b'\\' => self.at += 1,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        Err(Type42Refusal::StringTruncated)
    }

    /// The `count` bytes after `RD` and its one space (TN #5012 section 4.2).
    fn binary(&mut self, count: i64) -> Result<Token<'a>, Type42Refusal> {
        let count = usize::try_from(count).map_err(|_| Type42Refusal::StringTruncated)?;
        let start = self.at + 1;
        let end = start
            .checked_add(count)
            .ok_or(Type42Refusal::StringTruncated)?;
        let bytes = self
            .data
            .get(start..end)
            .ok_or(Type42Refusal::StringTruncated)?;
        self.at = end;
        Ok(Token::Binary(bytes))
    }
}

/// A PostScript number: an integer, a real, or a radix number `base#digits`.
fn number(word: &[u8]) -> Option<Token<'static>> {
    let text = core::str::from_utf8(word).ok()?;
    if let Some((base, digits)) = text.split_once('#') {
        let base: u32 = base.parse().ok()?;
        if !(2..=36).contains(&base) || digits.is_empty() {
            return None;
        }
        return i64::from_str_radix(digits, base).ok().map(Token::Integer);
    }
    let body = text.strip_prefix(['+', '-']).unwrap_or(text);
    if body.is_empty()
        || !body
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        || !body
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_digit() || b == b'.')
    {
        return None;
    }
    if body.bytes().all(|b| b.is_ascii_digit()) {
        return text.parse().ok().map(Token::Integer);
    }
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(Token::Real)
}

/// Every token of a program, or the refusal the first malformed one earns.
fn tokens(data: &[u8]) -> Result<Vec<Token<'_>>, Type42Refusal> {
    let mut scanner = Scanner::new(data);
    let mut out = Vec::new();
    while let Some(token) = scanner.scan() {
        out.push(token?);
    }
    Ok(out)
}

/// The token after the first `/key`, where there is one.
fn value_after<'t, 'a>(tokens: &'t [Token<'a>], key: &[u8]) -> Option<&'t Token<'a>> {
    let at = tokens.iter().position(|t| *t == Token::Name(key))?;
    tokens.get(at + 1)
}

/// The index just past the procedure, array or dictionary that opens at
/// `at`, or `at + 1` for any other token.
fn skip(tokens: &[Token<'_>], at: usize) -> Result<usize, Type42Refusal> {
    let Some(Token::Open(_)) = tokens.get(at) else {
        return Ok(at + 1);
    };
    let mut depth = 0usize;
    let mut i = at;
    while let Some(token) = tokens.get(i) {
        match token {
            Token::Open(_) => {
                depth += 1;
                if depth > MAX_NESTING {
                    return Err(Type42Refusal::Nesting);
                }
            }
            Token::Close(_) => {
                depth -= 1;
                if depth == 0 {
                    return Ok(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(Type42Refusal::Unclosed)
}

/// `/FontMatrix [a b c d e f]`: the first definition that is a literal
/// array.
fn font_matrix(tokens: &[Token<'_>]) -> Result<Option<[f64; 6]>, Type42Refusal> {
    let Some(at) = tokens.iter().position(|t| *t == Token::Name(b"FontMatrix")) else {
        return Ok(None);
    };
    if tokens.get(at + 1) != Some(&Token::Open(b'[')) {
        return Err(Type42Refusal::FontMatrix);
    }
    let mut values = [0.0f64; 6];
    for (index, value) in values.iter_mut().enumerate() {
        *value = match tokens.get(at + 2 + index) {
            Some(Token::Integer(n)) => *n as f64,
            Some(Token::Real(r)) => *r,
            _ => return Err(Type42Refusal::FontMatrix),
        };
    }
    if tokens.get(at + 8) != Some(&Token::Close(b']')) {
        return Err(Type42Refusal::FontMatrix);
    }
    Ok(Some(values))
}

/// `/Encoding`: `StandardEncoding`, an array built by `dup <code> /<name>
/// put`, or a literal array of names.
fn encoding(tokens: &[Token<'_>]) -> Result<Option<Encoding>, Type42Refusal> {
    let Some(at) = tokens.iter().position(|t| *t == Token::Name(b"Encoding")) else {
        return Ok(None);
    };
    match tokens.get(at + 1) {
        Some(Token::Executable(b"StandardEncoding")) => Ok(Some(Encoding::Standard)),
        Some(Token::Open(b'[')) => {
            let mut codes: Vec<Option<Vec<u8>>> = Vec::new();
            let mut i = at + 2;
            loop {
                match tokens.get(i) {
                    Some(Token::Close(b']')) => break,
                    Some(Token::Name(name)) if codes.len() < 256 => {
                        codes.push((*name != b".notdef").then(|| name.to_vec()));
                    }
                    _ => return Err(Type42Refusal::Encoding),
                }
                i += 1;
            }
            codes.resize(256, None);
            Ok(Some(Encoding::Codes(codes)))
        }
        Some(Token::Integer(_)) => {
            if tokens.get(at + 2) != Some(&Token::Executable(b"array")) {
                return Err(Type42Refusal::Encoding);
            }
            let mut codes: Vec<Option<Vec<u8>>> = vec![None; 256];
            let mut i = at + 3;
            loop {
                match tokens.get(i) {
                    // The end of the definition.
                    Some(Token::Executable(b"def" | b"readonly")) => break,
                    Some(Token::Executable(b"dup")) => {
                        let (Some(Token::Integer(code)), Some(Token::Name(name)), Some(put)) =
                            (tokens.get(i + 1), tokens.get(i + 2), tokens.get(i + 3))
                        else {
                            return Err(Type42Refusal::Encoding);
                        };
                        if *put != Token::Executable(b"put") {
                            return Err(Type42Refusal::Encoding);
                        }
                        if let Some(slot) = usize::try_from(*code)
                            .ok()
                            .and_then(|code| codes.get_mut(code))
                        {
                            *slot = (*name != b".notdef").then(|| name.to_vec());
                        }
                        i += 4;
                    }
                    // The `0 1 255 {1 index exch /.notdef put} for` that
                    // section 7's example clears the array with, and any
                    // other step that sets no code.
                    Some(Token::Open(_)) => i = skip(tokens, i)?,
                    Some(_) => i += 1,
                    None => return Err(Type42Refusal::Encoding),
                }
            }
            Ok(Some(Encoding::Codes(codes)))
        }
        // Another named encoding — `ISOLatin1Encoding` — is one this crate
        // carries no table for: the program then has no built-in encoding
        // here, which is not damage.
        Some(Token::Executable(_)) => Ok(None),
        _ => Err(Type42Refusal::Encoding),
    }
}

/// `/sfnts [<…> <…>]`: each string decoded, its padding byte dropped, and
/// all of them concatenated.
fn sfnts(tokens: &[Token<'_>]) -> Result<Vec<u8>, Type42Refusal> {
    let at = tokens
        .iter()
        .position(|t| *t == Token::Name(b"sfnts"))
        .ok_or(Type42Refusal::Sfnts)?;
    if tokens.get(at + 1) != Some(&Token::Open(b'[')) {
        return Err(Type42Refusal::Sfnts);
    }
    let mut face = Vec::new();
    let mut i = at + 2;
    loop {
        let string = match tokens.get(i) {
            Some(Token::Close(b']')) => break,
            Some(Token::Hex(hex)) => unhex(hex),
            Some(Token::Binary(bytes)) => bytes.to_vec(),
            // A binary string is followed by whatever defines it, as in
            // section 4.2's `{noaccess def} executeonly def`; inside the
            // array there is nothing of that kind to pass over.
            _ => return Err(Type42Refusal::Sfnts),
        };
        // Section 4.2: an odd string's last byte is padding, not face.
        let keep = if string.len() % 2 == 1 {
            string.len() - 1
        } else {
            string.len()
        };
        face.extend_from_slice(&string[..keep]);
        i += 1;
    }
    Ok(face)
}

/// The bytes a hexadecimal string spells; an odd last digit is followed by
/// 0, as PostScript reads one.
fn unhex(hex: &[u8]) -> Vec<u8> {
    let digits: Vec<u8> = hex
        .iter()
        .filter_map(|b| (*b as char).to_digit(16).map(|d| d as u8))
        .collect();
    digits
        .chunks(2)
        .map(|pair| (pair[0] << 4) | pair.get(1).copied().unwrap_or(0))
        .collect()
}

/// `/CharStrings`: `<n> dict dup begin … end`, or `<< … >>`, each entry a
/// name and a glyph index (`def` after it in the first form), or a name and
/// a procedure, which is passed over and counted.
fn charstrings(tokens: &[Token<'_>]) -> Result<(Vec<(Vec<u8>, u16)>, usize), Type42Refusal> {
    let at = tokens
        .iter()
        .position(|t| *t == Token::Name(b"CharStrings"))
        .ok_or(Type42Refusal::CharStrings)?;
    let (mut i, close): (usize, Token<'_>) = match tokens.get(at + 1) {
        Some(Token::Open(b'<')) => (at + 2, Token::Close(b'>')),
        Some(Token::Integer(_)) => {
            // `<n> dict dup begin`, the `dup` optional.
            let mut j = at + 2;
            if tokens.get(j) != Some(&Token::Executable(b"dict")) {
                return Err(Type42Refusal::CharStrings);
            }
            j += 1;
            if tokens.get(j) == Some(&Token::Executable(b"dup")) {
                j += 1;
            }
            if tokens.get(j) != Some(&Token::Executable(b"begin")) {
                return Err(Type42Refusal::CharStrings);
            }
            (j + 1, Token::Executable(b"end"))
        }
        _ => return Err(Type42Refusal::CharStrings),
    };

    let mut out: Vec<(Vec<u8>, u16)> = Vec::new();
    let mut procedures = 0usize;
    loop {
        match tokens.get(i) {
            Some(token) if *token == close => break,
            Some(Token::Name(name)) => {
                if out.len() + procedures >= MAX_CHARSTRINGS {
                    return Err(Type42Refusal::TooManyCharStrings);
                }
                match tokens.get(i + 1) {
                    Some(Token::Integer(glyph)) => {
                        let glyph =
                            u16::try_from(*glyph).map_err(|_| Type42Refusal::CharStrings)?;
                        out.push((name.to_vec(), glyph));
                        i += 2;
                    }
                    Some(Token::Open(b'{')) => {
                        procedures += 1;
                        i = skip(tokens, i + 1)?;
                        // `bind`, `executeonly` and the like before `def`.
                        while let Some(Token::Executable(word)) = tokens.get(i) {
                            if *word == b"def" {
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ => return Err(Type42Refusal::CharStrings),
                }
                if close == Token::Executable(b"end") {
                    if tokens.get(i) != Some(&Token::Executable(b"def")) {
                        return Err(Type42Refusal::CharStrings);
                    }
                    i += 1;
                }
            }
            _ => return Err(Type42Refusal::CharStrings),
        }
    }
    Ok((out, procedures))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table directory around `tables`, each table padded to four bytes, as
    /// a font tool lays one out.
    fn face_of(tables: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let count = tables.len() as u16;
        let mut out = Vec::new();
        out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        out.extend_from_slice(&count.to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let mut offset = 12 + 16 * tables.len();
        let mut bodies = Vec::new();
        for (tag, body) in tables {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&0u32.to_be_bytes());
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            let mut padded = body.clone();
            while padded.len() % 4 != 0 {
                padded.push(0);
            }
            offset += padded.len();
            bodies.push(padded);
        }
        for body in bodies {
            out.extend_from_slice(&body);
        }
        out
    }

    /// A face of three glyphs: `head` with 1000 units per em and short
    /// offsets, `maxp`, and a `glyf` of twenty-two bytes and its `loca`, every
    /// table four-aligned, so a split inside a table and one at its boundary
    /// are both at even offsets.
    fn face() -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&1000u16.to_be_bytes());
        let mut maxp = vec![0u8; 6];
        maxp[0..4].copy_from_slice(&0x0000_5000u32.to_be_bytes());
        maxp[4..6].copy_from_slice(&3u16.to_be_bytes());
        // A ten-unit square, then two empty glyphs: one contour of four
        // on-curve points, `(0, 0)`, `(10, 0)`, `(10, 10)` and `(0, 10)`, each
        // coordinate a short delta or the same as the last, and a byte of
        // padding so the short `loca` can halve the length.
        let glyf: Vec<u8> = vec![
            0, 1, 0, 0, 0, 0, 0, 10, 0, 10, 0, 3, 0, 0, 0x31, 0x33, 0x35, 0x23, 10, 10, 10, 0,
        ];
        let loca: Vec<u8> = vec![0, 0, 0, 11, 0, 11, 0, 11];
        face_of(&[
            (b"glyf", glyf),
            (b"head", head),
            (b"loca", loca),
            (b"maxp", maxp),
        ])
    }

    /// The hex form of `bytes`, wrapped at 32 digits a line as section 4.2's
    /// strings are.
    fn hex(bytes: &[u8]) -> String {
        let mut out = String::new();
        for (index, b) in bytes.iter().enumerate() {
            if index > 0 && index % 16 == 0 {
                out.push('\n');
            }
            out.push_str(&format!("{b:02X}"));
        }
        out
    }

    /// The face split at `cuts`, each piece one string padded to an odd
    /// length with a zero byte, in hex.
    fn hex_strings(face: &[u8], cuts: &[usize]) -> String {
        let mut out = String::new();
        let mut from = 0;
        for &to in cuts.iter().chain(core::iter::once(&face.len())) {
            let mut piece = face[from..to].to_vec();
            piece.push(0);
            out.push_str(&format!("<{}>\n", hex(&piece)));
            from = to;
        }
        out
    }

    /// The same as binary strings, `<n> RD` and one space and the bytes.
    fn binary_strings(face: &[u8], cuts: &[usize], token: &str) -> Vec<u8> {
        let mut out = Vec::new();
        let mut from = 0;
        for &to in cuts.iter().chain(core::iter::once(&face.len())) {
            let mut piece = face[from..to].to_vec();
            piece.push(0);
            out.extend_from_slice(format!("{} {token} ", piece.len()).as_bytes());
            out.extend_from_slice(&piece);
            out.push(b'\n');
            from = to;
        }
        out
    }

    /// A whole Type 42 program, as TN #5012 section 7 writes one, around the
    /// `/sfnts` array `strings`; its `/CharStrings` dictionary comes last, so
    /// its `end` is the last thing a reader needs.
    fn program(strings: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(
            b"%!PS-TrueTypeFont-65536-65536-1\n\
              11 dict begin\n\
              /FontName /Test42 def\n\
              /Encoding 256 array\n\
              0 1 255{1 index exch/.notdef put}for\n\
              dup 65 /A put\n\
              dup 66 /B put\n\
              readonly def\n\
              /PaintType 0 def\n\
              /FontMatrix [1 0 0 1 0 0] def\n\
              /FontBBox[0 0 1000 1000] def\n\
              /FontType 42 def\n\
              /FontInfo 2 dict dup begin /Notice (A (nested) test face) def end def\n\
              /sfnts[",
        );
        out.extend_from_slice(strings);
        out.extend_from_slice(
            b"]def\n\
              /CharStrings 4 dict dup begin\n\
              /.notdef 0 def/A 1 def\n\
              /B 2 def /space 2 def\n\
              end",
        );
        out
    }

    /// The bytes the hex wrapper writes for `face` cut at `cuts`.
    fn hex_program(face: &[u8], cuts: &[usize]) -> Vec<u8> {
        program(hex_strings(face, cuts).as_bytes())
    }

    /// The offset of a table's data in [`face`].
    fn table_offset(face: &[u8], tag: &[u8; 4]) -> usize {
        let count = usize::from(u16::from_be_bytes([face[4], face[5]]));
        (0..count)
            .find_map(|i| {
                let record = &face[12 + 16 * i..28 + 16 * i];
                (&record[0..4] == tag).then(|| {
                    u32::from_be_bytes([record[8], record[9], record[10], record[11]]) as usize
                })
            })
            .expect("the table is in the face")
    }

    #[test]
    fn the_sfnts_strings_reassemble_the_face_they_wrap() {
        let face = face();
        let boundary = table_offset(&face, b"head");
        let inside = table_offset(&face, b"glyf") + 8;
        assert_eq!(boundary % 2, 0);
        assert_eq!(inside % 2, 0);
        for cuts in [vec![], vec![boundary], vec![inside], vec![inside, boundary]] {
            let read = Type42::parse(&hex_program(&face, &cuts)).expect("the hex wrapper reads");
            assert_eq!(read.face(), face.as_slice(), "hex, cut at {cuts:?}");
            assert!(read.sfnt().is_some());
            for token in ["RD", "-|"] {
                let read = Type42::parse(&program(&binary_strings(&face, &cuts, token)))
                    .expect("the binary wrapper reads");
                assert_eq!(
                    read.face(),
                    face.as_slice(),
                    "binary {token}, cut at {cuts:?}"
                );
            }
        }

        // A string of even length is all face, with no padding to drop.
        let even = program(format!("<{}>", hex(&face)).as_bytes());
        assert_eq!(
            Type42::parse(&even).expect("it reads").face(),
            face.as_slice()
        );

        // Hex and binary strings may be mixed, white space anywhere in a hex
        // string ignored.
        let mut mixed = Vec::new();
        let mut first = face[..boundary].to_vec();
        first.push(0);
        mixed.extend_from_slice(format!("< {} >\n", hex(&first).replace('\n', "\n  ")).as_bytes());
        let mut second = face[boundary..].to_vec();
        second.push(0);
        mixed.extend_from_slice(format!("{} RD ", second.len()).as_bytes());
        mixed.extend_from_slice(&second);
        assert_eq!(
            Type42::parse(&program(&mixed)).expect("it reads").face(),
            face.as_slice()
        );
    }

    /// A hexadecimal string with an odd number of digits: the last is followed
    /// by 0, as PostScript reads one.
    #[test]
    fn an_odd_hex_digit_is_followed_by_zero() {
        assert_eq!(unhex(b"0A1"), vec![0x0A, 0x10]);
        assert_eq!(unhex(b" 0 a\n1B "), vec![0x0A, 0x1B]);
        assert_eq!(unhex(b""), Vec::<u8>::new());
    }

    #[test]
    fn charstrings_map_each_name_to_its_glyph() {
        let face = face();
        let read = Type42::parse(&hex_program(&face, &[])).expect("it reads");
        assert_eq!(read.glyph_for_name(b".notdef"), Some(0));
        assert_eq!(read.glyph_for_name(b"A"), Some(1));
        assert_eq!(read.glyph_for_name(b"B"), Some(2));
        assert_eq!(read.glyph_for_name(b"space"), Some(2));
        assert_eq!(read.glyph_for_name(b"C"), None);
        assert_eq!(read.notdef(), 0);
        assert_eq!(read.charstring_count(), 4);
        assert_eq!(
            read.glyph_names().collect::<Vec<_>>(),
            vec![
                (b".notdef".as_slice(), 0),
                (b"A".as_slice(), 1),
                (b"B".as_slice(), 2),
                (b"space".as_slice(), 2),
            ]
        );

        // Through the built-in encoding.
        assert_eq!(read.encoding_name(65), Some(b"A".as_slice()));
        assert_eq!(read.glyph_for_code(65), Some(1));
        assert_eq!(read.glyph_for_code(66), Some(2));
        assert_eq!(read.glyph_for_code(67), None, "a code left at .notdef");
        assert_eq!(read.font_matrix(), Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]));

        // The first index a name is given wins; a procedure is passed over
        // and counted; the `<< >>` form reads as the `dict` form does.
        let strings = hex_strings(&face, &[]);
        let twice = String::from_utf8(program(strings.as_bytes()))
            .expect("ASCII")
            .replace(
                "/B 2 def /space 2 def",
                "/B 2 def /A 7 def /C {pop} bind def",
            );
        let read = Type42::parse(twice.as_bytes()).expect("it reads");
        assert_eq!(read.glyph_for_name(b"A"), Some(1));
        assert_eq!(read.glyph_for_name(b"C"), None);
        assert_eq!(read.procedure_count(), 1);
        let angle = String::from_utf8(program(strings.as_bytes()))
            .expect("ASCII")
            .replace(
                "/CharStrings 4 dict dup begin\n/.notdef 0 def/A 1 def\n/B 2 def /space 2 def\nend",
                "/CharStrings << /.notdef 0 /A 1 /B 2 >>",
            );
        let read = Type42::parse(angle.as_bytes()).expect("it reads");
        assert_eq!(read.glyph_for_name(b"B"), Some(2));
        assert_eq!(read.charstring_count(), 3);

        // `StandardEncoding` and a literal array of names.
        let standard = String::from_utf8(program(strings.as_bytes()))
            .expect("ASCII")
            .replace(
                "/Encoding 256 array\n0 1 255{1 index exch/.notdef put}for\ndup 65 /A put\ndup 66 /B put\nreadonly def",
                "/Encoding StandardEncoding def",
            );
        let read = Type42::parse(standard.as_bytes()).expect("it reads");
        assert_eq!(read.encoding_name(65), Some(b"A".as_slice()));
        assert_eq!(
            read.glyph_for_code(32),
            Some(2),
            "space, through StandardEncoding"
        );
        let literal = standard.replace(
            "/Encoding StandardEncoding def",
            "/Encoding [/.notdef /A /B] def",
        );
        let read = Type42::parse(literal.as_bytes()).expect("it reads");
        assert_eq!(read.glyph_for_code(1), Some(1));
        assert_eq!(read.glyph_for_code(2), Some(2));
        assert_eq!(read.glyph_for_code(0), None);
        assert_eq!(read.glyph_for_code(200), None);
        let other = standard.replace(
            "/Encoding StandardEncoding def",
            "/Encoding ISOLatin1Encoding def",
        );
        assert_eq!(
            Type42::parse(other.as_bytes())
                .expect("it reads")
                .encoding_name(65),
            None
        );
    }

    /// Every prefix of a program is declined, except those that already hold
    /// everything a reader needs, which read the whole program.
    #[test]
    fn every_truncation_is_declined() {
        let face = face();
        let cuts = [table_offset(&face, b"head")];
        for full in [
            hex_program(&face, &cuts),
            program(&binary_strings(&face, &cuts, "RD")),
        ] {
            let whole = Type42::parse(&full).expect("the whole program reads");
            for cut in 0..full.len() {
                if let Ok(read) = Type42::parse(&full[..cut]) {
                    panic!(
                        "a program cut to {cut} of {} bytes read, with a face of {} bytes",
                        full.len(),
                        read.face().len()
                    );
                }
            }
            assert_eq!(whole.face(), face.as_slice());
        }
    }

    #[test]
    fn what_is_not_a_type42_program_is_declined_by_name() {
        let face = face();
        let good = String::from_utf8(hex_program(&face, &[])).expect("ASCII");
        let refused = |text: &str| Type42::parse(text.as_bytes()).err();

        assert_eq!(refused(""), Some(Type42Refusal::NotType42));
        assert_eq!(refused("not a font"), Some(Type42Refusal::NotType42));
        assert_eq!(
            refused(&good.replace("/FontType 42 def", "/FontType 1 def")),
            Some(Type42Refusal::NotType42)
        );
        assert_eq!(
            refused(&good.replace("/FontType 42 def", "/FontType 42 def /CIDFontType 2 def")),
            Some(Type42Refusal::CidKeyed)
        );
        assert_eq!(
            refused(&good.replace("/sfnts[", "/sfnts (")),
            Some(Type42Refusal::StringTruncated)
        );
        assert_eq!(
            refused(&good.replace("/sfnts[", "/sfnts 7 [")),
            Some(Type42Refusal::Sfnts)
        );
        assert_eq!(
            refused(&good.replace("/sfnts[<", "/sfnts[/x <")),
            Some(Type42Refusal::Sfnts)
        );
        assert_eq!(
            refused(&good.replace("/sfnts[<", "/sfnts[<QQ")),
            Some(Type42Refusal::HexString)
        );
        assert_eq!(
            refused(&good.replace("/FontMatrix [1 0 0 1 0 0]", "/FontMatrix [1 0 0 1 0]")),
            Some(Type42Refusal::FontMatrix)
        );
        assert_eq!(
            refused(&good.replace("/FontMatrix [1 0 0 1 0 0]", "/FontMatrix matrix")),
            Some(Type42Refusal::FontMatrix)
        );
        assert_eq!(
            refused(&good.replace("dup 66 /B put", "dup 66 put")),
            Some(Type42Refusal::Encoding)
        );
        assert_eq!(
            refused(&good.replace("/A 1 def", "/A (one) def")),
            Some(Type42Refusal::CharStrings)
        );
        assert_eq!(
            refused(&good.replace("/A 1 def", "/A 70000 def")),
            Some(Type42Refusal::CharStrings)
        );
        assert_eq!(
            refused(&good.replace("/CharStrings 4 dict dup begin", "/CharStrings 4 dict")),
            Some(Type42Refusal::CharStrings)
        );
        assert_eq!(
            refused(&good.replace("/CharStrings", "/Charstrings")),
            Some(Type42Refusal::CharStrings)
        );
        // A face the strings spell that is no sfnt.
        assert_eq!(
            refused(
                &program(b"<0102030400>")
                    .iter()
                    .map(|b| *b as char)
                    .collect::<String>()
            ),
            Some(Type42Refusal::Face)
        );
        // A binary string with fewer bytes than its RD declares.
        let mut short = program(b"");
        let at = short
            .windows(7)
            .position(|w| w == b"/sfnts[")
            .expect("sfnts")
            + 7;
        short.splice(at..at, b"9999 RD ab".iter().copied());
        assert_eq!(
            Type42::parse(&short).err(),
            Some(Type42Refusal::StringTruncated)
        );
        // A glyph directory in place of `glyf`.
        let gdir = face_of(&[(b"gdir", Vec::new()), (b"head", vec![0; 54])]);
        assert_eq!(
            Type42::parse(&hex_program(&gdir, &[])).err(),
            Some(Type42Refusal::GlyphDirectory)
        );
    }

    #[test]
    fn nesting_and_counts_past_their_caps_are_refused_by_name() {
        let face = face();
        let good = String::from_utf8(hex_program(&face, &[])).expect("ASCII");
        let deep = format!(
            "{}{}",
            "{".repeat(MAX_NESTING + 1),
            "}".repeat(MAX_NESTING + 1)
        );
        assert_eq!(
            Type42::parse(
                good.replace("/A 1 def", &format!("/A {deep} def"))
                    .as_bytes()
            )
            .err(),
            Some(Type42Refusal::Nesting)
        );
        let shallow = format!("{}{}", "{".repeat(MAX_NESTING), "}".repeat(MAX_NESTING));
        let read = Type42::parse(
            good.replace("/A 1 def", &format!("/A {shallow} def"))
                .as_bytes(),
        )
        .expect("a procedure at the cap is passed over");
        assert_eq!(read.procedure_count(), 1);

        let mut many = String::new();
        for index in 0..=MAX_CHARSTRINGS {
            many.push_str(&format!("/g{index} 0 def "));
        }
        assert_eq!(
            Type42::parse(good.replace("/B 2 def /space 2 def", &many).as_bytes()).err(),
            Some(Type42Refusal::TooManyCharStrings)
        );
    }

    #[test]
    fn a_program_is_recognised_by_its_first_line_or_its_font_type() {
        let face = face();
        let program = hex_program(&face, &[]);
        assert!(Type42::is_type42(&program));
        let mut indented = b"\n  ".to_vec();
        indented.extend_from_slice(&program);
        assert!(Type42::is_type42(&indented));

        let text = String::from_utf8(program).expect("ASCII");
        let unheaded = text.replace("%!PS-TrueTypeFont-65536-65536-1\n", "");
        assert!(Type42::is_type42(unheaded.as_bytes()), "by /FontType 42");
        assert!(
            Type42::parse(unheaded.as_bytes()).is_ok(),
            "the comment is not required"
        );

        assert!(!Type42::is_type42(
            unheaded.replace("/FontType 42", "/FontType 1").as_bytes()
        ));
        assert!(!Type42::is_type42(
            b"%!PS-AdobeFont-1.0: Test\n/FontType 1 def currentfile eexec"
        ));
        assert!(!Type42::is_type42(b"\0\x01\0\0"));
        assert!(!Type42::is_type42(b""));
        // The first `/FontType` with a number after it decides, and a Type 1
        // program's `eexec` ends the search.
        assert!(!Type42::is_type42(b"currentfile eexec /FontType 42"));
    }

    #[test]
    fn numbers_read_as_postscript_writes_them() {
        assert_eq!(number(b"42"), Some(Token::Integer(42)));
        assert_eq!(number(b"-7"), Some(Token::Integer(-7)));
        assert_eq!(number(b"+3"), Some(Token::Integer(3)));
        assert_eq!(number(b"16#FF"), Some(Token::Integer(255)));
        assert_eq!(number(b"8#777"), Some(Token::Integer(511)));
        assert_eq!(number(b".5"), Some(Token::Real(0.5)));
        assert_eq!(number(b"-1.25e2"), Some(Token::Real(-125.0)));
        assert_eq!(number(b"1e400"), None, "not finite");
        assert_eq!(number(b"inf"), None);
        assert_eq!(number(b"NaN"), None);
        assert_eq!(number(b"RD"), None);
        assert_eq!(number(b"-|"), None);
        assert_eq!(number(b"37#1"), None);
        assert_eq!(number(b"-"), None);
    }

    /// Arbitrary bytes are declined without a panic, every single-byte change
    /// of a good program included.
    #[test]
    fn damaged_programs_are_declined_without_panicking() {
        let face = face();
        let good = program(&binary_strings(
            &face,
            &[table_offset(&face, b"head")],
            "RD",
        ));
        for at in 0..good.len() {
            for byte in [
                0u8, b'(', b')', b'<', b'>', b'[', b']', b'{', b'}', b'/', 0xFF,
            ] {
                let mut damaged = good.clone();
                damaged[at] = byte;
                let _ = Type42::is_type42(&damaged);
                if let Ok(read) = Type42::parse(&damaged) {
                    let _ = read.sfnt();
                    let _ = read.glyph_for_code(65);
                }
            }
        }
    }

    /// The fuzz target's seeds, written by this file's own wrapper rather than
    /// fetched: run by name, `cargo test -p tinker-pdf-font --lib --
    /// --ignored type42::tests::write_the_fuzz_seeds --exact`, and its output
    /// committed under `fuzz/corpus/type42`.
    #[test]
    #[ignore = "writes fuzz/corpus/type42; run it when the wrapper the seeds come from changes"]
    fn write_the_fuzz_seeds() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/type42");
        std::fs::create_dir_all(&dir).expect("the corpus directory");
        let face = face();
        let boundary = table_offset(&face, b"head");
        let inside = table_offset(&face, b"glyf") + 8;
        let hex_split = hex_program(&face, &[inside, boundary]);
        let seeds: Vec<(&str, Vec<u8>)> = vec![
            ("hex-one-string.t42", hex_program(&face, &[])),
            ("hex-split.t42", hex_split.clone()),
            (
                "binary-split.t42",
                program(&binary_strings(&face, &[inside, boundary], "RD")),
            ),
            (
                "binary-dash-bar.t42",
                program(&binary_strings(&face, &[boundary], "-|")),
            ),
            (
                "truncated-in-sfnts.t42",
                hex_split[..hex_split.len() / 2].to_vec(),
            ),
        ];
        for (name, bytes) in seeds {
            std::fs::write(dir.join(name), bytes).expect("the seed is written");
        }
    }
}
