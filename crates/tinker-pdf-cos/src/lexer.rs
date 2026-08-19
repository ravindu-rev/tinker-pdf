//! The token scanner (7.2, 7.3).
//!
//! A hand-written state machine over `&[u8]` with a cursor: no regex, no
//! lookahead buffer, and every token carries the byte range it came from so
//! warnings can name a position. The lexer never fails. At every position it
//! produces a token or a warning-carrying fallback, because failure is a
//! parser-level concept and the repair ladder needs the scanner to keep going
//! through anything.

use crate::limits;
use crate::object::PdfString;
use crate::warn::{WarningKind, WarningSink};

/// 7.2.2 Table 1: the six white-space characters. NUL is one of them.
pub const fn is_whitespace(b: u8) -> bool {
    matches!(b, 0x00 | 0x09 | 0x0a | 0x0c | 0x0d | 0x20)
}

/// 7.2.2 Table 2: the delimiter characters.
pub const fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// 7.2.2: everything that is neither white-space nor a delimiter.
pub const fn is_regular(b: u8) -> bool {
    !is_whitespace(b) && !is_delimiter(b)
}

/// The keywords of the file grammar (7.2.2, 7.3.2, 7.3.9, 7.3.10, 7.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Keyword {
    /// `obj`
    Obj,
    /// `endobj`
    Endobj,
    /// `stream`
    Stream,
    /// `endstream`
    Endstream,
    /// `R`
    R,
    /// `xref`
    Xref,
    /// `trailer`
    Trailer,
    /// `startxref`
    Startxref,
    /// `true`
    True,
    /// `false`
    False,
    /// `null`
    Null,
}

impl Keyword {
    /// The keyword spelled by `bytes`, if it is one.
    pub fn from_bytes(bytes: &[u8]) -> Option<Keyword> {
        Some(match bytes {
            b"obj" => Keyword::Obj,
            b"endobj" => Keyword::Endobj,
            b"stream" => Keyword::Stream,
            b"endstream" => Keyword::Endstream,
            b"R" => Keyword::R,
            b"xref" => Keyword::Xref,
            b"trailer" => Keyword::Trailer,
            b"startxref" => Keyword::Startxref,
            b"true" => Keyword::True,
            b"false" => Keyword::False,
            b"null" => Keyword::Null,
            _ => return None,
        })
    }

    /// How the keyword is spelled.
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            Keyword::Obj => b"obj",
            Keyword::Endobj => b"endobj",
            Keyword::Stream => b"stream",
            Keyword::Endstream => b"endstream",
            Keyword::R => b"R",
            Keyword::Xref => b"xref",
            Keyword::Trailer => b"trailer",
            Keyword::Startxref => b"startxref",
            Keyword::True => b"true",
            Keyword::False => b"false",
            Keyword::Null => b"null",
        }
    }

    /// Whether this keyword delimits file structure rather than an object.
    ///
    /// Meeting one inside an array or dictionary means the container was never
    /// closed, and swallowing it would lose the structure that follows — so
    /// these end the container instead.
    pub fn is_structural(self) -> bool {
        matches!(
            self,
            Keyword::Obj
                | Keyword::Endobj
                | Keyword::Stream
                | Keyword::Endstream
                | Keyword::Xref
                | Keyword::Trailer
                | Keyword::Startxref
        )
    }
}

/// What a token is.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    /// An integer (7.3.3), already clamped to `i64`.
    Int(i64),
    /// A real (7.3.3).
    Real(f64),
    /// A literal or hex string (7.3.4), already unescaped.
    String(PdfString),
    /// A name (7.3.5), already `#`-decoded, without the leading solidus.
    Name(Vec<u8>),
    /// `[`
    ArrayOpen,
    /// `]`
    ArrayClose,
    /// `<<`
    DictOpen,
    /// `>>`
    DictClose,
    /// `{` — not a COS object, but 7.10.5 function streams use it.
    ProcOpen,
    /// `}`
    ProcClose,
    /// One of the file-grammar keywords.
    Keyword(Keyword),
    /// A regular-character run that is no keyword, or a stray delimiter. The
    /// span says which bytes; nothing else about them is knowable here.
    Unknown,
    /// End of buffer. `start` and `end` are both the buffer length.
    Eof,
}

/// A token and the byte range it occupies.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// What was read.
    pub kind: TokenKind,
    /// Offset of the token's first byte.
    pub start: u64,
    /// Offset just past the token's last byte.
    pub end: u64,
}

impl Token {
    /// Whether this is the end-of-buffer token.
    pub fn is_eof(&self) -> bool {
        matches!(self.kind, TokenKind::Eof)
    }
}

/// The cursor over a document buffer.
#[derive(Clone, Debug)]
pub struct Lexer<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    /// A lexer positioned at the start of `buf`.
    pub fn new(buf: &'a [u8]) -> Lexer<'a> {
        Lexer { buf, pos: 0 }
    }

    /// A lexer positioned at `offset`, clamped to the end of the buffer.
    pub fn at(buf: &'a [u8], offset: u64) -> Lexer<'a> {
        let mut lexer = Lexer::new(buf);
        lexer.seek(offset);
        lexer
    }

    /// The buffer being scanned.
    pub fn buffer(&self) -> &'a [u8] {
        self.buf
    }

    /// The current offset.
    pub fn offset(&self) -> u64 {
        self.pos as u64
    }

    /// Moves the cursor, clamping past-the-end offsets to the buffer length.
    pub fn seek(&mut self, offset: u64) {
        let len = self.buf.len() as u64;
        // Clamping first makes the narrowing cast exact on 32-bit targets.
        self.pos = offset.min(len) as usize;
    }

    /// Whether the cursor is at the end of the buffer.
    pub fn at_eof(&self) -> bool {
        self.pos >= self.buf.len()
    }

    fn byte(&self, at: usize) -> Option<u8> {
        self.buf.get(at).copied()
    }

    fn peek(&self) -> Option<u8> {
        self.byte(self.pos)
    }

    /// Skips white space and comments (7.2.2, 7.2.3).
    pub fn skip_space(&mut self) {
        loop {
            match self.peek() {
                Some(b) if is_whitespace(b) => self.pos += 1,
                // 7.2.3: a comment runs to the end of the line and is
                // equivalent to a single white-space character.
                Some(b'%') => {
                    self.pos += 1;
                    while let Some(b) = self.peek() {
                        if b == b'\r' || b == b'\n' {
                            break;
                        }
                        self.pos += 1;
                    }
                }
                _ => return,
            }
        }
    }

    /// Reads the next token, recording any leniency it needed into `sink`.
    pub fn next_token(&mut self, sink: &mut WarningSink) -> Token {
        self.skip_space();
        let start = self.pos;
        let Some(b) = self.peek() else {
            return Token {
                kind: TokenKind::Eof,
                start: start as u64,
                end: start as u64,
            };
        };
        let kind = match b {
            b'[' => {
                self.pos += 1;
                TokenKind::ArrayOpen
            }
            b']' => {
                self.pos += 1;
                TokenKind::ArrayClose
            }
            b'{' => {
                self.pos += 1;
                TokenKind::ProcOpen
            }
            b'}' => {
                self.pos += 1;
                TokenKind::ProcClose
            }
            b'(' => {
                self.pos += 1;
                self.lex_literal_string(start, sink)
            }
            b'<' => {
                if self.byte(self.pos + 1) == Some(b'<') {
                    self.pos += 2;
                    TokenKind::DictOpen
                } else {
                    self.pos += 1;
                    self.lex_hex_string(start, sink)
                }
            }
            b'>' => {
                if self.byte(self.pos + 1) == Some(b'>') {
                    self.pos += 2;
                    TokenKind::DictClose
                } else {
                    self.pos += 1;
                    TokenKind::Unknown
                }
            }
            b')' => {
                self.pos += 1;
                TokenKind::Unknown
            }
            b'/' => {
                self.pos += 1;
                self.lex_name(start, sink)
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.lex_number(sink),
            _ => self.lex_keyword(),
        };
        Token {
            kind,
            start: start as u64,
            end: self.pos as u64,
        }
    }

    fn take_regular_run(&mut self) -> &'a [u8] {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if !is_regular(b) {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            // Only reachable if a delimiter fell through the dispatch above;
            // advancing keeps the scanner making progress regardless.
            self.pos += 1;
        }
        self.buf.get(start..self.pos).unwrap_or(&[])
    }

    fn lex_keyword(&mut self) -> TokenKind {
        let run = self.take_regular_run();
        match Keyword::from_bytes(run) {
            Some(k) => TokenKind::Keyword(k),
            None => TokenKind::Unknown,
        }
    }

    fn lex_number(&mut self, sink: &mut WarningSink) -> TokenKind {
        let start = self.pos as u64;
        let run = self.take_regular_run();
        parse_number(run, start, sink)
    }

    /// 7.3.5. `#xx` decodes to a raw byte; anything else after `#` is taken
    /// literally, because rejecting the name would lose the dictionary key it
    /// is spelling.
    fn lex_name(&mut self, start: usize, sink: &mut WarningSink) -> TokenKind {
        let mut out: Vec<u8> = Vec::new();
        let mut truncated = false;
        let mut bad_escape = false;
        while let Some(b) = self.peek() {
            if !is_regular(b) {
                break;
            }
            self.pos += 1;
            let byte = if b == b'#' {
                match (
                    self.byte(self.pos).and_then(hex_val),
                    self.byte(self.pos + 1).and_then(hex_val),
                ) {
                    (Some(hi), Some(lo)) => {
                        self.pos += 2;
                        (hi << 4) | lo
                    }
                    _ => {
                        if !bad_escape {
                            bad_escape = true;
                            sink.warn(self.pos as u64 - 1, WarningKind::BadNameEscape);
                        }
                        b'#'
                    }
                }
            } else {
                b
            };
            if out.len() < limits::MAX_NAME_LEN {
                out.push(byte);
            } else {
                truncated = true;
            }
        }
        if truncated {
            sink.warn(start as u64, WarningKind::NameTruncated);
        }
        TokenKind::Name(out)
    }

    /// 7.3.4.2. The cursor is just past the opening parenthesis.
    fn lex_literal_string(&mut self, start: usize, sink: &mut WarningSink) -> TokenKind {
        let mut out: Vec<u8> = Vec::new();
        let mut depth: u64 = 1;
        let mut terminated = false;
        while let Some(b) = self.peek() {
            self.pos += 1;
            match b {
                b'(' => {
                    depth = depth.saturating_add(1);
                    out.push(b'(');
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        terminated = true;
                        break;
                    }
                    out.push(b')');
                }
                b'\\' => self.literal_escape(&mut out),
                // 7.3.4.2: an end-of-line marker inside a literal string is a
                // byte value of 0x0A whichever way it is spelled.
                b'\r' => {
                    if self.peek() == Some(b'\n') {
                        self.pos += 1;
                    }
                    out.push(b'\n');
                }
                _ => out.push(b),
            }
        }
        if !terminated {
            sink.warn(start as u64, WarningKind::UnterminatedString);
        }
        TokenKind::String(PdfString::literal(out))
    }

    fn literal_escape(&mut self, out: &mut Vec<u8>) {
        let Some(e) = self.peek() else {
            // A reverse solidus at end of input has nothing to escape.
            return;
        };
        self.pos += 1;
        match e {
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'(' => out.push(b'('),
            b')' => out.push(b')'),
            b'\\' => out.push(b'\\'),
            // 7.3.4.2: a reverse solidus before an end-of-line marker is a
            // line continuation; both are dropped.
            b'\r' => {
                if self.peek() == Some(b'\n') {
                    self.pos += 1;
                }
            }
            b'\n' => {}
            b'0'..=b'7' => {
                // 7.3.4.2: one to three octal digits, high-order overflow
                // ignored.
                let mut value: u32 = u32::from(e - b'0');
                for _ in 0..2 {
                    match self.peek() {
                        Some(d @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(d - b'0');
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                out.push((value & 0xff) as u8);
            }
            // 7.3.4.2: before any other character the reverse solidus is
            // ignored.
            other => out.push(other),
        }
    }

    /// 7.3.4.3. The cursor is just past the opening angle bracket.
    fn lex_hex_string(&mut self, start: usize, sink: &mut WarningSink) -> TokenKind {
        let mut out: Vec<u8> = Vec::new();
        let mut high: Option<u8> = None;
        let mut terminated = false;
        let mut warned_non_hex = false;
        while let Some(b) = self.peek() {
            self.pos += 1;
            if b == b'>' {
                terminated = true;
                break;
            }
            if is_whitespace(b) {
                continue;
            }
            match hex_val(b) {
                Some(v) => match high {
                    None => high = Some(v),
                    Some(hi) => {
                        out.push((hi << 4) | v);
                        high = None;
                    }
                },
                // Real files carry stray bytes here; skipping one is cheaper
                // than losing the whole string.
                None => {
                    if !warned_non_hex {
                        warned_non_hex = true;
                        sink.warn(self.pos as u64 - 1, WarningKind::NonHexInHexString);
                    }
                }
            }
        }
        // 7.3.4.3: an odd digit count means the final digit is 0.
        if let Some(hi) = high {
            out.push(hi << 4);
        }
        if !terminated {
            sink.warn(start as u64, WarningKind::UnterminatedHexString);
        }
        TokenKind::String(PdfString::hex(out))
    }
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn digit_val(b: u8) -> Option<u64> {
    let d = b.wrapping_sub(b'0');
    if d <= 9 {
        Some(u64::from(d))
    } else {
        None
    }
}

/// 7.3.3, plus the numeric leniency the corpus demands: a doubled sign reads
/// as negative, exponent notation is evaluated, and anything else reads as its
/// longest valid prefix or as zero.
fn parse_number(run: &[u8], offset: u64, sink: &mut WarningSink) -> TokenKind {
    let mut i = 0usize;
    let mut negative = false;
    let mut signs = 0u32;
    while let Some(&b) = run.get(i) {
        match b {
            b'-' => {
                negative = true;
                signs += 1;
                i += 1;
            }
            b'+' => {
                signs += 1;
                i += 1;
            }
            _ => break,
        }
    }
    if signs > 1 {
        sink.warn(offset, WarningKind::DoubledSign);
    }

    let int_start = i;
    while matches!(run.get(i), Some(b) if b.is_ascii_digit()) {
        i += 1;
    }
    let int_digits = run.get(int_start..i).unwrap_or(&[]);

    let mut frac_digits: &[u8] = &[];
    let mut has_dot = false;
    if run.get(i) == Some(&b'.') {
        has_dot = true;
        i += 1;
        let frac_start = i;
        while matches!(run.get(i), Some(b) if b.is_ascii_digit()) {
            i += 1;
        }
        frac_digits = run.get(frac_start..i).unwrap_or(&[]);
    }

    let mut exponent = 0i32;
    let mut has_exponent = false;
    if matches!(run.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        let mut exp_negative = false;
        match run.get(j) {
            Some(b'+') => j += 1,
            Some(b'-') => {
                exp_negative = true;
                j += 1;
            }
            _ => {}
        }
        let digits_start = j;
        let mut value = 0i32;
        while let Some(d) = run.get(j).copied().and_then(digit_val) {
            value = value.saturating_mul(10).saturating_add(d as i32);
            j += 1;
        }
        if j > digits_start {
            has_exponent = true;
            exponent = if exp_negative { -value } else { value };
            i = j;
        }
    }

    let no_digits = int_digits.is_empty() && frac_digits.is_empty();
    if no_digits || i < run.len() {
        sink.warn(offset, WarningKind::MalformedNumberPrefix);
    }
    if has_exponent {
        // 7.3.3 has no exponent form. Producers emit it anyway, so evaluate it.
        sink.warn(offset, WarningKind::ExponentNumber);
    }

    if has_dot || has_exponent {
        TokenKind::Real(build_real(
            int_digits,
            frac_digits,
            exponent,
            negative,
            offset,
            sink,
        ))
    } else {
        TokenKind::Int(build_int(int_digits, negative, offset, sink))
    }
}

fn build_int(digits: &[u8], negative: bool, offset: u64, sink: &mut WarningSink) -> i64 {
    let mut magnitude = 0u64;
    let mut overflowed = false;
    for &b in digits {
        let Some(d) = digit_val(b) else { continue };
        match magnitude.checked_mul(10).and_then(|m| m.checked_add(d)) {
            Some(m) => magnitude = m,
            None => {
                overflowed = true;
                break;
            }
        }
    }
    let signed = if negative {
        -(i128::from(magnitude))
    } else {
        i128::from(magnitude)
    };
    if overflowed || signed > i128::from(i64::MAX) || signed < i128::from(i64::MIN) {
        sink.warn(offset, WarningKind::IntOverflowClamped);
        return if negative { i64::MIN } else { i64::MAX };
    }
    signed as i64
}

/// The exactly representable powers of ten: 10^22 is the last one f64 holds
/// without rounding, which is what makes the fast path below exact.
const POW10: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

/// 2^53: above this an f64 no longer holds every integer, so the single-
/// rounding argument for the fast path stops holding.
const MAX_EXACT_MANTISSA: f64 = 9_007_199_254_740_992.0;

/// The most significant decimal digits an f64 can distinguish; more than this
/// only moves the decimal point.
const MAX_SIGNIFICANT_DIGITS: u32 = 19;

fn build_real(
    int_digits: &[u8],
    frac_digits: &[u8],
    exponent: i32,
    negative: bool,
    offset: u64,
    sink: &mut WarningSink,
) -> f64 {
    let mut mantissa = 0u64;
    let mut significant = 0u32;
    let mut scale = 0i32;
    for &b in int_digits {
        let Some(d) = digit_val(b) else { continue };
        if mantissa == 0 && d == 0 {
            continue;
        }
        if significant < MAX_SIGNIFICANT_DIGITS {
            mantissa = mantissa.saturating_mul(10).saturating_add(d);
            significant += 1;
        } else {
            scale = scale.saturating_add(1);
        }
    }
    for &b in frac_digits {
        let Some(d) = digit_val(b) else { continue };
        if mantissa == 0 && d == 0 {
            scale = scale.saturating_sub(1);
            continue;
        }
        if significant < MAX_SIGNIFICANT_DIGITS {
            mantissa = mantissa.saturating_mul(10).saturating_add(d);
            significant += 1;
            scale = scale.saturating_sub(1);
        }
    }

    let mut value = mantissa as f64;
    if value != 0.0 {
        value = scale_pow10(value, scale.saturating_add(exponent));
    }
    if !value.is_finite() {
        sink.warn(offset, WarningKind::RealOverflowClamped);
        value = f64::MAX;
    }
    if negative {
        -value
    } else {
        value
    }
}

fn scale_pow10(mut value: f64, mut exponent: i32) -> f64 {
    // The saturating guards come **first**, and that ordering is the whole of
    // this fix rather than a tidying.
    //
    // Gap 24's first real campaign panicked here on `0.0002E-7700000000000000`
    // in 1.5 million runs. The exponent's own accumulation saturates at
    // `i32::MAX`, so a negative one is `i32::MIN + 1` — safe by itself — and
    // then the caller's `scale.saturating_add(exponent)` takes it the last step
    // to `i32::MIN` whenever the number also has a fraction. `-i32::MIN` has no
    // `i32`, so the negation below panicked in any build with overflow checks
    // and **wrapped to `i32::MIN` in release**, where `POW10.get` misses and the
    // wrong branch is taken silently. A panic in debug and a wrong number in
    // release is the worst pair available.
    //
    // Clamping first makes both impossible: past ±400 the answer is infinity or
    // zero whatever the mantissa is, because `f64` has no exponent range left —
    // and those two returns were already here, three lines too late.
    if exponent > 400 {
        return f64::INFINITY;
    }
    if exponent < -400 {
        return 0.0;
    }
    // Fast path: mantissa and power are both exact, so the result carries a
    // single rounding — the same value a correctly rounded parse produces.
    if value < MAX_EXACT_MANTISSA {
        if let Some(p) = usize::try_from(exponent).ok().and_then(|e| POW10.get(e)) {
            return value * p;
        }
        if let Some(p) = usize::try_from(-exponent).ok().and_then(|e| POW10.get(e)) {
            return value / p;
        }
    }
    while exponent > 0 {
        let step = exponent.min(22);
        if let Some(p) = POW10.get(step as usize) {
            value *= p;
        }
        exponent -= step;
    }
    while exponent < 0 {
        let step = (-exponent).min(22);
        if let Some(p) = POW10.get(step as usize) {
            value /= p;
        }
        exponent += step;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex_all(input: &[u8]) -> (Vec<TokenKind>, Vec<WarningKind>) {
        let mut sink = WarningSink::new();
        let mut lexer = Lexer::new(input);
        let mut kinds = Vec::new();
        loop {
            let token = lexer.next_token(&mut sink);
            if token.is_eof() {
                break;
            }
            assert!(token.end > token.start, "token made no progress: {token:?}");
            kinds.push(token.kind);
        }
        (kinds, sink.warnings().iter().map(|w| w.kind).collect())
    }

    fn lex_one(input: &[u8]) -> (TokenKind, Vec<WarningKind>) {
        let (mut kinds, warnings) = lex_all(input);
        assert_eq!(kinds.len(), 1, "expected one token, got {kinds:?}");
        (kinds.remove(0), warnings)
    }

    fn string_bytes(input: &[u8]) -> Vec<u8> {
        match lex_one(input).0 {
            TokenKind::String(s) => s.bytes,
            other => panic!("expected a string, got {other:?}"),
        }
    }

    fn name_bytes(input: &[u8]) -> Vec<u8> {
        match lex_one(input).0 {
            TokenKind::Name(n) => n,
            other => panic!("expected a name, got {other:?}"),
        }
    }

    // ---- 7.2: white space, comments, delimiters --------------------------

    #[test]
    fn byte_classes_follow_tables_1_and_2() {
        for b in [0x00u8, 0x09, 0x0a, 0x0c, 0x0d, 0x20] {
            assert!(is_whitespace(b));
            assert!(!is_regular(b));
        }
        for b in *b"()<>[]{}/%" {
            assert!(is_delimiter(b));
            assert!(!is_regular(b));
        }
        for b in *b"aZ0#+-." {
            assert!(is_regular(b));
        }
        assert!(!is_whitespace(0x0b));
    }

    #[test]
    fn comments_are_white_space() {
        let (kinds, warnings) = lex_all(b"12 %comment (not a string\n34% to eof");
        assert_eq!(kinds, [TokenKind::Int(12), TokenKind::Int(34)]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn nul_separates_tokens() {
        let (kinds, _) = lex_all(b"1\x002");
        assert_eq!(kinds, [TokenKind::Int(1), TokenKind::Int(2)]);
    }

    #[test]
    fn tokens_carry_spans() {
        let mut sink = WarningSink::new();
        let mut lexer = Lexer::new(b"  /Name1 (s) ");
        let name = lexer.next_token(&mut sink);
        assert_eq!((name.start, name.end), (2, 8));
        let string = lexer.next_token(&mut sink);
        assert_eq!((string.start, string.end), (9, 12));
        let eof = lexer.next_token(&mut sink);
        assert!(eof.is_eof());
        assert_eq!((eof.start, eof.end), (13, 13));
        assert!(sink.is_empty());
    }

    #[test]
    fn seek_clamps_past_the_end() {
        let mut lexer = Lexer::new(b"123");
        lexer.seek(u64::MAX);
        assert_eq!(lexer.offset(), 3);
        assert!(lexer.at_eof());
        let mut sink = WarningSink::new();
        assert!(lexer.next_token(&mut sink).is_eof());
    }

    // ---- 7.3.2 booleans, 7.3.9 null, keywords ----------------------------

    #[test]
    fn keywords_lex_as_keywords() {
        let (kinds, warnings) =
            lex_all(b"true false null obj endobj stream endstream R xref trailer startxref");
        assert_eq!(
            kinds,
            [
                TokenKind::Keyword(Keyword::True),
                TokenKind::Keyword(Keyword::False),
                TokenKind::Keyword(Keyword::Null),
                TokenKind::Keyword(Keyword::Obj),
                TokenKind::Keyword(Keyword::Endobj),
                TokenKind::Keyword(Keyword::Stream),
                TokenKind::Keyword(Keyword::Endstream),
                TokenKind::Keyword(Keyword::R),
                TokenKind::Keyword(Keyword::Xref),
                TokenKind::Keyword(Keyword::Trailer),
                TokenKind::Keyword(Keyword::Startxref),
            ]
        );
        assert!(warnings.is_empty());
        for k in [Keyword::Obj, Keyword::R, Keyword::True] {
            assert_eq!(Keyword::from_bytes(k.as_bytes()), Some(k));
        }
    }

    #[test]
    fn unknown_runs_lex_as_unknown() {
        let (kinds, warnings) = lex_all(b"TRUE nul");
        assert_eq!(kinds, [TokenKind::Unknown, TokenKind::Unknown]);
        assert!(warnings.is_empty());
    }

    // ---- 7.3.3 numbers ---------------------------------------------------

    #[test]
    fn spec_integer_examples() {
        let (kinds, warnings) = lex_all(b"123 43445 +17 -98 0");
        assert_eq!(
            kinds,
            [
                TokenKind::Int(123),
                TokenKind::Int(43445),
                TokenKind::Int(17),
                TokenKind::Int(-98),
                TokenKind::Int(0),
            ]
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn spec_real_examples() {
        let (kinds, warnings) = lex_all(b"34.5 -3.62 +123.6 4. -.002 0.0");
        assert_eq!(
            kinds,
            [
                TokenKind::Real(34.5),
                TokenKind::Real(-3.62),
                TokenKind::Real(123.6),
                TokenKind::Real(4.0),
                TokenKind::Real(-0.002),
                TokenKind::Real(0.0),
            ]
        );
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    fn real_value(lexeme: &str) -> f64 {
        match lex_one(lexeme.as_bytes()).0 {
            TokenKind::Real(v) => v,
            other => panic!("{lexeme}: {other:?}"),
        }
    }

    #[test]
    fn real_values_are_exact_in_the_range_pdf_uses() {
        // Up to 19 significant digits with a decimal exponent within +-22, the
        // mantissa and the power of ten are both exact, so the result carries
        // one rounding and equals a correctly rounded parse.
        for lexeme in [
            "0.1",
            "3.14159",
            "2.718281828459045",
            "612.0",
            "0.00001",
            "123456.789",
            "595.2755905511811",
            "0.0000000000000000001",
        ] {
            assert_eq!(
                real_value(lexeme),
                lexeme.parse::<f64>().unwrap(),
                "{lexeme}"
            );
        }
    }

    #[test]
    fn real_values_outside_that_range_stay_within_an_ulp() {
        // Past the exact path the value is scaled in two steps, which costs at
        // most a second rounding. Accepted deliberately: no PDF coordinate
        // lives out here.
        for lexeme in ["0.000000000000000000001234", "1234567890123456789012.5"] {
            let got = real_value(lexeme);
            let want: f64 = lexeme.parse().unwrap();
            let ulps = ((got - want) / want).abs();
            assert!(ulps <= 1e-15, "{lexeme}: got {got}, want {want}");
        }
    }

    #[test]
    fn leniency_integer_overflow_clamps() {
        assert_eq!(
            lex_one(b"99999999999999999999"),
            (
                TokenKind::Int(i64::MAX),
                vec![WarningKind::IntOverflowClamped]
            )
        );
        assert_eq!(
            lex_one(b"-99999999999999999999"),
            (
                TokenKind::Int(i64::MIN),
                vec![WarningKind::IntOverflowClamped]
            )
        );
    }

    #[test]
    fn integer_bounds_do_not_warn() {
        assert_eq!(
            lex_one(b"9223372036854775807"),
            (TokenKind::Int(i64::MAX), vec![])
        );
        assert_eq!(
            lex_one(b"-9223372036854775808"),
            (TokenKind::Int(i64::MIN), vec![])
        );
    }

    #[test]
    fn leniency_leading_plus_is_accepted() {
        assert_eq!(lex_one(b"+17"), (TokenKind::Int(17), vec![]));
    }

    #[test]
    fn leniency_doubled_sign_reads_as_negative() {
        assert_eq!(
            lex_one(b"--5"),
            (TokenKind::Int(-5), vec![WarningKind::DoubledSign])
        );
        assert_eq!(
            lex_one(b"+-5"),
            (TokenKind::Int(-5), vec![WarningKind::DoubledSign])
        );
        assert_eq!(
            lex_one(b"---5"),
            (TokenKind::Int(-5), vec![WarningKind::DoubledSign])
        );
    }

    /// An exponent large enough to reach `i32::MIN` is zero or infinity, and does
    /// not panic on the way.
    ///
    /// Found by gap 24 milestone 5's campaign — the first session `cos_object`
    /// has ever had — in 1.5 million runs, on `0.0002E-7700000000000000`.
    ///
    /// The arithmetic is worth writing down because every step of it looked
    /// safe. The exponent's own accumulation *saturates*, so a huge negative
    /// exponent arrives as `i32::MIN + 1` and negating that is fine. Then
    /// `build_real` adds the fraction's scale with `saturating_add`, which is
    /// also fine, and lands on exactly `i32::MIN` — for which there is no
    /// `i32` negation. Two saturating operations, each correct, composing into
    /// the one value the third could not take.
    ///
    /// It panicked in any build with overflow checks and **wrapped in
    /// release**, where the negation yields `i32::MIN` again, `POW10.get`
    /// misses, and the slow path runs on a number nothing clamped. A panic in
    /// debug and a wrong answer in release.
    ///
    /// Both crash inputs are asserted, and so is the boundary either side of
    /// the ±400 clamp, because moving those two returns above the fast path is
    /// the fix and a test that only covered the extreme would not see them move
    /// back.
    #[test]
    fn an_exponent_that_saturates_is_clamped_rather_than_negated() {
        let real = |bytes: &[u8]| match lex_one(bytes).0 {
            TokenKind::Real(v) => v,
            other => panic!("{other:?}"),
        };

        // The two inputs libFuzzer produced, verbatim.
        assert_eq!(real(b"0.0002E-7700000000000000"), 0.0);
        assert_eq!(real(b"0.00177777777777771e-2029777777777777"), 0.0);

        // The same shape upward. Not infinity: `build_real` clamps a
        // non-finite result to `f64::MAX` and warns, because 7.3.3's reals are
        // finite — so the clamp this fix moved and the clamp above it are two
        // different guards, and the assertion is on what a caller actually
        // gets.
        assert_eq!(real(b"0.0002E7700000000000000"), f64::MAX);

        // And the clamp's own edges, so the reordering above cannot silently
        // come undone: inside it the value is computed, outside it is not.
        assert_eq!(real(b"1e-400"), 0.0);
        assert!(real(b"1e-300") > 0.0);
        assert_eq!(real(b"1e401"), f64::MAX);
        assert!(real(b"1e300").is_finite() && real(b"1e300") < f64::MAX);

        // Ordinary numbers are untouched by the reordering.
        assert_eq!(real(b"1.5"), 1.5);
        assert_eq!(real(b"2e3"), 2000.0);
        assert_eq!(real(b"-0.25e-2"), -0.0025);
    }

    #[test]
    fn leniency_exponent_is_evaluated() {
        assert_eq!(
            lex_one(b"6.02e23"),
            (TokenKind::Real(6.02e23), vec![WarningKind::ExponentNumber])
        );
        assert_eq!(
            lex_one(b"1e-3"),
            (TokenKind::Real(1e-3), vec![WarningKind::ExponentNumber])
        );
        assert_eq!(
            lex_one(b"5E+2"),
            (TokenKind::Real(500.0), vec![WarningKind::ExponentNumber])
        );
    }

    #[test]
    fn leniency_exponent_overflow_clamps() {
        assert_eq!(
            lex_one(b"1e999"),
            (
                TokenKind::Real(f64::MAX),
                vec![
                    WarningKind::ExponentNumber,
                    WarningKind::RealOverflowClamped
                ]
            )
        );
        assert_eq!(
            lex_one(b"-1e99999999999999999999"),
            (
                TokenKind::Real(-f64::MAX),
                vec![
                    WarningKind::ExponentNumber,
                    WarningKind::RealOverflowClamped
                ]
            )
        );
        assert_eq!(
            lex_one(b"1e-999"),
            (TokenKind::Real(0.0), vec![WarningKind::ExponentNumber])
        );
    }

    #[test]
    fn leniency_malformed_reads_longest_valid_prefix() {
        assert_eq!(
            lex_one(b"1.2.3"),
            (
                TokenKind::Real(1.2),
                vec![WarningKind::MalformedNumberPrefix]
            )
        );
        assert_eq!(
            lex_one(b"12abc"),
            (TokenKind::Int(12), vec![WarningKind::MalformedNumberPrefix])
        );
        assert_eq!(
            lex_one(b"123e"),
            (
                TokenKind::Int(123),
                vec![WarningKind::MalformedNumberPrefix]
            )
        );
        assert_eq!(
            lex_one(b"-1-2"),
            (TokenKind::Int(-1), vec![WarningKind::MalformedNumberPrefix])
        );
    }

    #[test]
    fn leniency_numeric_with_no_digits_reads_as_zero() {
        assert_eq!(
            lex_one(b"-"),
            (TokenKind::Int(0), vec![WarningKind::MalformedNumberPrefix])
        );
        assert_eq!(
            lex_one(b"."),
            (
                TokenKind::Real(0.0),
                vec![WarningKind::MalformedNumberPrefix]
            )
        );
        assert_eq!(
            lex_one(b"--"),
            (
                TokenKind::Int(0),
                vec![WarningKind::DoubledSign, WarningKind::MalformedNumberPrefix]
            )
        );
    }

    #[test]
    fn numbers_end_at_delimiters() {
        let (kinds, warnings) = lex_all(b"[1 2.5]");
        assert_eq!(
            kinds,
            [
                TokenKind::ArrayOpen,
                TokenKind::Int(1),
                TokenKind::Real(2.5),
                TokenKind::ArrayClose
            ]
        );
        assert!(warnings.is_empty());
    }

    // ---- 7.3.4.2 literal strings ----------------------------------------

    #[test]
    fn spec_literal_string_examples() {
        assert_eq!(string_bytes(b"(This is a string)"), b"This is a string");
        assert_eq!(
            string_bytes(b"(Strings may contain newlines\nand such.)"),
            b"Strings may contain newlines\nand such."
        );
        assert_eq!(
            string_bytes(b"(Strings may contain balanced parentheses ( ) and special characters (*!&}^% and so on).)"),
            &b"Strings may contain balanced parentheses ( ) and special characters (*!&}^% and so on)."[..]
        );
        assert_eq!(string_bytes(b"()"), b"");
        assert_eq!(
            string_bytes(b"(It has zero (0) length.)"),
            b"It has zero (0) length."
        );
    }

    #[test]
    fn spec_escape_examples() {
        assert_eq!(
            string_bytes(b"(These \\\ntwo strings are the same.)"),
            b"These two strings are the same."
        );
        assert_eq!(
            string_bytes(b"(This string has an end-of-line at the end of it.\\n)"),
            b"This string has an end-of-line at the end of it.\n"
        );
        assert_eq!(
            string_bytes(b"(This string contains \\245two octal characters\\307.)"),
            b"This string contains \xa5two octal characters\xc7."
        );
        assert_eq!(string_bytes(b"(\\0053)"), b"\x053");
        assert_eq!(string_bytes(b"(\\053)"), b"+");
        assert_eq!(string_bytes(b"(\\53)"), b"+");
    }

    #[test]
    fn all_named_escapes_decode() {
        assert_eq!(
            string_bytes(b"(\\n\\r\\t\\b\\f\\(\\)\\\\)"),
            b"\n\r\t\x08\x0c()\\"
        );
    }

    #[test]
    fn leniency_octal_overflow_is_taken_mod_256() {
        // 7.3.4.2: high-order overflow is ignored, so \777 is 0o777 & 0xff.
        assert_eq!(string_bytes(b"(\\777)"), b"\xff");
        assert_eq!(string_bytes(b"(\\400)"), b"\x00");
        // Only three digits are consumed; the fourth is literal.
        assert_eq!(string_bytes(b"(\\1011)"), b"A1");
    }

    #[test]
    fn leniency_backslash_before_eol_continues_the_line() {
        assert_eq!(string_bytes(b"(a\\\nb)"), b"ab");
        assert_eq!(string_bytes(b"(a\\\r\nb)"), b"ab");
        assert_eq!(string_bytes(b"(a\\\rb)"), b"ab");
    }

    #[test]
    fn leniency_lone_backslash_is_dropped() {
        assert_eq!(string_bytes(b"(a\\qb)"), b"aqb");
        assert_eq!(string_bytes(b"(a\\\x00b)"), b"a\x00b");
    }

    #[test]
    fn leniency_backslash_at_eof_is_dropped() {
        let (kind, warnings) = lex_one(b"(abc\\");
        assert_eq!(kind, TokenKind::String(PdfString::literal(b"abc".to_vec())));
        assert_eq!(warnings, [WarningKind::UnterminatedString]);
    }

    #[test]
    fn leniency_raw_eol_normalizes_to_lf() {
        // 7.3.4.2: CR and CRLF inside a literal string are one 0x0a byte.
        assert_eq!(string_bytes(b"(a\rb)"), b"a\nb");
        assert_eq!(string_bytes(b"(a\r\nb)"), b"a\nb");
        assert_eq!(string_bytes(b"(a\nb)"), b"a\nb");
    }

    #[test]
    fn nested_parens_balance() {
        assert_eq!(string_bytes(b"(a(b(c)d)e)"), b"a(b(c)d)e");
        // An escaped parenthesis does not change the nesting.
        assert_eq!(string_bytes(b"(a\\(b)"), b"a(b");
        let (kinds, _) = lex_all(b"(a\\)b)");
        assert_eq!(
            kinds,
            [TokenKind::String(PdfString::literal(b"a)b".to_vec()))]
        );
    }

    #[test]
    fn leniency_unterminated_string_closes_at_eof() {
        let (kind, warnings) = lex_one(b"(abc");
        assert_eq!(kind, TokenKind::String(PdfString::literal(b"abc".to_vec())));
        assert_eq!(warnings, [WarningKind::UnterminatedString]);

        let (kind, warnings) = lex_one(b"(a(b");
        assert_eq!(kind, TokenKind::String(PdfString::literal(b"a(b".to_vec())));
        assert_eq!(warnings, [WarningKind::UnterminatedString]);
    }

    #[test]
    fn strings_are_bytes_not_text() {
        assert_eq!(string_bytes(b"(\xff\xfe\x00A)"), b"\xff\xfe\x00A");
    }

    // ---- 7.3.4.3 hex strings --------------------------------------------

    #[test]
    fn spec_hex_string_examples() {
        assert_eq!(
            string_bytes(b"<4E6F762073686D6F7A206B6120706F702E>"),
            b"Nov shmoz ka pop."
        );
        assert_eq!(string_bytes(b"<901FA3>"), b"\x90\x1f\xa3");
        // 7.3.4.3: the missing final digit is assumed to be 0.
        assert_eq!(string_bytes(b"<901FA>"), b"\x90\x1f\xa0");
    }

    #[test]
    fn hex_strings_record_their_origin() {
        match lex_one(b"<41>").0 {
            TokenKind::String(s) => {
                assert!(s.hex);
                assert_eq!(s.bytes, b"A");
            }
            other => panic!("{other:?}"),
        }
        match lex_one(b"(A)").0 {
            TokenKind::String(s) => assert!(!s.hex),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn hex_strings_ignore_white_space() {
        assert_eq!(string_bytes(b"<4 1\n4\r2\t43>"), b"ABC");
        assert_eq!(string_bytes(b"<>"), b"");
    }

    #[test]
    fn odd_hex_padding_is_spec_behaviour_and_does_not_warn() {
        let (kind, warnings) = lex_one(b"<4>");
        assert_eq!(kind, TokenKind::String(PdfString::hex(vec![0x40])));
        assert!(warnings.is_empty());
    }

    #[test]
    fn leniency_non_hex_byte_is_skipped() {
        // The byte is dropped from the digit stream, not treated as a
        // separator: the digits either side pair up across it.
        let (kind, warnings) = lex_one(b"<4G8>");
        assert_eq!(kind, TokenKind::String(PdfString::hex(b"H".to_vec())));
        assert_eq!(warnings, [WarningKind::NonHexInHexString]);

        let (kind, warnings) = lex_one(b"<48 4G 49>");
        assert_eq!(
            kind,
            TokenKind::String(PdfString::hex(vec![0x48, 0x44, 0x90]))
        );
        assert_eq!(warnings, [WarningKind::NonHexInHexString]);
    }

    #[test]
    fn leniency_unterminated_hex_string_closes_at_eof() {
        let (kind, warnings) = lex_one(b"<4869");
        assert_eq!(kind, TokenKind::String(PdfString::hex(b"Hi".to_vec())));
        assert_eq!(warnings, [WarningKind::UnterminatedHexString]);
    }

    #[test]
    fn dict_open_beats_hex_string() {
        let (kinds, warnings) = lex_all(b"<</A 1>>");
        assert_eq!(
            kinds,
            [
                TokenKind::DictOpen,
                TokenKind::Name(b"A".to_vec()),
                TokenKind::Int(1),
                TokenKind::DictClose,
            ]
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn stray_close_delimiters_lex_as_unknown() {
        let (kinds, warnings) = lex_all(b"> )");
        assert_eq!(kinds, [TokenKind::Unknown, TokenKind::Unknown]);
        assert!(warnings.is_empty());
    }

    // ---- 7.3.5 names -----------------------------------------------------

    #[test]
    fn spec_name_examples() {
        assert_eq!(name_bytes(b"/Name1"), b"Name1");
        assert_eq!(name_bytes(b"/ASomewhatLongerName"), b"ASomewhatLongerName");
        assert_eq!(
            name_bytes(b"/A;Name_With-Various***Characters?"),
            &b"A;Name_With-Various***Characters?"[..]
        );
        assert_eq!(name_bytes(b"/1.2"), b"1.2");
        assert_eq!(name_bytes(b"/$$"), b"$$");
        assert_eq!(name_bytes(b"/@pattern"), b"@pattern");
        assert_eq!(name_bytes(b"/.notdef"), b".notdef");
        assert_eq!(name_bytes(b"/Lime#20Green"), b"Lime Green");
        assert_eq!(
            name_bytes(b"/paired#28#29parentheses"),
            b"paired()parentheses"
        );
        assert_eq!(
            name_bytes(b"/The_Key_of_F#23_Minor"),
            b"The_Key_of_F#_Minor"
        );
        assert_eq!(name_bytes(b"/A#42"), b"AB");
    }

    #[test]
    fn empty_name_lexes() {
        let (kinds, warnings) = lex_all(b"/ 1");
        assert_eq!(kinds, [TokenKind::Name(Vec::new()), TokenKind::Int(1)]);
        assert!(warnings.is_empty());
    }

    #[test]
    fn names_end_at_delimiters() {
        let (kinds, _) = lex_all(b"/A/B");
        assert_eq!(
            kinds,
            [
                TokenKind::Name(b"A".to_vec()),
                TokenKind::Name(b"B".to_vec())
            ]
        );
    }

    #[test]
    fn leniency_bad_name_escape_is_literal() {
        let (kind, warnings) = lex_one(b"/A#ZZ");
        assert_eq!(kind, TokenKind::Name(b"A#ZZ".to_vec()));
        assert_eq!(warnings, [WarningKind::BadNameEscape]);

        let (kind, warnings) = lex_one(b"/A#4");
        assert_eq!(kind, TokenKind::Name(b"A#4".to_vec()));
        assert_eq!(warnings, [WarningKind::BadNameEscape]);

        let (kind, warnings) = lex_one(b"/#");
        assert_eq!(kind, TokenKind::Name(b"#".to_vec()));
        assert_eq!(warnings, [WarningKind::BadNameEscape]);
    }

    #[test]
    fn bad_name_escape_warns_once_per_name() {
        let (_, warnings) = lex_one(b"/A#Z#Y#X");
        assert_eq!(warnings, [WarningKind::BadNameEscape]);
    }

    #[test]
    fn names_decode_to_raw_bytes_not_utf8() {
        assert_eq!(name_bytes(b"/A#ff#00B"), b"A\xff\x00B");
    }

    #[test]
    fn leniency_overlong_name_is_truncated() {
        let mut input = vec![b'/'];
        input.extend(std::iter::repeat_n(b'x', limits::MAX_NAME_LEN + 10));
        input.extend_from_slice(b" 1");
        let mut sink = WarningSink::new();
        let mut lexer = Lexer::new(&input);
        let token = lexer.next_token(&mut sink);
        match token.kind {
            TokenKind::Name(n) => assert_eq!(n.len(), limits::MAX_NAME_LEN),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::NameTruncated]
        );
        // The whole name is still consumed, so the next token is not garbage.
        assert_eq!(lexer.next_token(&mut sink).kind, TokenKind::Int(1));
    }

    // ---- structural ------------------------------------------------------

    #[test]
    fn lexer_always_makes_progress() {
        for byte in 0u8..=255 {
            let input = [byte];
            let mut sink = WarningSink::new();
            let mut lexer = Lexer::new(&input);
            let token = lexer.next_token(&mut sink);
            if !token.is_eof() {
                assert!(lexer.offset() > 0, "byte {byte:#04x} did not advance");
            }
        }
    }

    #[test]
    fn deeply_nested_parens_do_not_recurse() {
        // Nesting is a counter, not a call stack, so depth costs nothing.
        let input = vec![b'('; 200_000];
        let (kind, warnings) = lex_one(&input);
        match kind {
            TokenKind::String(s) => assert_eq!(s.bytes.len(), input.len() - 1),
            other => panic!("{other:?}"),
        }
        assert_eq!(warnings, [WarningKind::UnterminatedString]);
    }
}
