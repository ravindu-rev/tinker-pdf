//! `css-syntax-3` §4: bytes in, tokens out.
//!
//! §3.3's preprocessing happens first and is not optional — a `\r\n` inside a
//! string literal and a `\r` on its own are the same token, and U+0000 is
//! U+FFFD everywhere including inside an identifier. Doing it as a pass over
//! the decoded text rather than as special cases in each consumer is what keeps
//! the seventeen algorithms below readable.
//!
//! The one departure from the spec's letter is that this tokenizer works over
//! `char`s rather than over code points read from a byte stream: §3.2's
//! decoding step is `String::from_utf8_lossy`, which is the same answer for
//! every input this engine will meet — an EPUB's stylesheets are UTF-8 by
//! OCF 3.3 §3.4 — and a replacement character is a `Delim` either way.

/// A hash token's flag (§4.2). `#foo` is an id and `#0f0` is not, and the
/// difference decides whether `#0f0` can be an id selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashKind {
    /// The value is an identifier: `#main`.
    Id,
    /// It is not: `#0f0`, `#123`.
    Unrestricted,
}

/// One `css-syntax-3` §4 token.
///
/// `<bad-string-token>` and `<bad-url-token>` are here rather than folded into
/// an error, because the grammar in §5 treats them as *values* that make the
/// construct holding them malformed — which is how a stylesheet with an
/// unterminated string keeps the rules after it.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    /// `<ident-token>`.
    Ident(String),
    /// `<function-token>`: an identifier immediately followed by `(`.
    Function(String),
    /// `<at-keyword-token>`.
    AtKeyword(String),
    /// `<hash-token>`.
    Hash(String, HashKind),
    /// `<string-token>`, with the quotes removed and escapes resolved.
    Str(String),
    /// `<bad-string-token>`: a newline inside a string.
    BadString,
    /// `<url-token>`: the unquoted `url(...)` form only. `url("x")` is a
    /// function token followed by a string, which is what §4.3.4 says.
    Url(String),
    /// `<bad-url-token>`.
    BadUrl,
    /// `<delim-token>`.
    Delim(char),
    /// `<number-token>`. `integer` is `Some` when §4.3.12's type flag is
    /// "integer", which is what `:nth-child()` and `font-weight` need.
    Number { value: f64, integer: bool },
    /// `<percentage-token>`.
    Percentage(f64),
    /// `<dimension-token>`, unit lower-cased for comparison but kept as
    /// written for a warning to name.
    Dimension { value: f64, unit: String },
    /// `<whitespace-token>`, collapsed to one per run as §4.3.1 allows.
    Whitespace,
    /// `<CDO-token>`, `<!--`.
    Cdo,
    /// `<CDC-token>`, `-->`.
    Cdc,
    /// `<colon-token>`.
    Colon,
    /// `<semicolon-token>`.
    Semicolon,
    /// `<comma-token>`.
    Comma,
    /// `<[-token>`.
    OpenSquare,
    /// `<]-token>`.
    CloseSquare,
    /// `<(-token>`.
    OpenParen,
    /// `<)-token>`.
    CloseParen,
    /// `<{-token>`.
    OpenCurly,
    /// `<}-token>`.
    CloseCurly,
}

impl Token {
    /// The mirror of an opening bracket, for §5.4.7's simple-block rule.
    pub fn closing_for(&self) -> Option<Token> {
        match self {
            Token::OpenSquare => Some(Token::CloseSquare),
            Token::OpenParen | Token::Function(_) => Some(Token::CloseParen),
            Token::OpenCurly => Some(Token::CloseCurly),
            _ => None,
        }
    }
}

/// §3.3's preprocessing, as one pass.
///
/// CR, CRLF and FF all become LF, and U+0000 becomes U+FFFD. A tokenizer that
/// did this per-consumer would get it right in the four places its author
/// thought of; a book only needs the fifth.
fn preprocess(text: &str) -> Vec<char> {
    let mut out = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\u{c}' => out.push('\n'),
            '\0' => out.push('\u{fffd}'),
            other => out.push(other),
        }
    }
    out
}

/// The tokenizer's own cursor over preprocessed text.
pub struct Tokenizer {
    text: Vec<char>,
    at: usize,
}

/// §4.3.7: is this a valid escape — a `\` not followed by a newline?
fn is_valid_escape(a: Option<char>, b: Option<char>) -> bool {
    a == Some('\\') && b != Some('\n')
}

/// §4.3.9's name-start code point.
fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c as u32 >= 0x80
}

/// §4.3.9's name code point.
fn is_name(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c == '-'
}

/// §4.2's whitespace: newline, tab and space, and nothing else. Notably not
/// U+00A0, which is a name code point and part of an identifier.
fn is_whitespace(c: char) -> bool {
    c == '\n' || c == '\t' || c == ' '
}

impl Tokenizer {
    /// Takes decoded source and preprocesses it.
    pub fn new(text: &str) -> Self {
        Self {
            text: preprocess(text),
            at: 0,
        }
    }

    fn peek(&self, ahead: usize) -> Option<char> {
        self.text.get(self.at + ahead).copied()
    }

    fn next_char(&mut self) -> Option<char> {
        let c = self.peek(0);
        if c.is_some() {
            self.at += 1;
        }
        c
    }

    /// §4.3.1's comment step, which is not a token: comments are consumed
    /// wherever whitespace could be and produce nothing at all.
    ///
    /// An unterminated comment runs to the end of the sheet. §4.3.2 makes that
    /// a parse error and then says to consume to EOF anyway, which is the
    /// forgiving answer and the one that keeps a book whose stylesheet ends
    /// mid-comment.
    ///
    /// Returns whether anything was consumed, which is what the whitespace
    /// step needs: `a /* x */ b` is **one** descendant combinator, so the
    /// whitespace either side of a comment has to collapse across it.
    fn consume_comments(&mut self) -> bool {
        let start = self.at;
        while self.peek(0) == Some('/') && self.peek(1) == Some('*') {
            self.at += 2;
            loop {
                match self.next_char() {
                    None => break,
                    Some('*') if self.peek(0) == Some('/') => {
                        self.at += 1;
                        break;
                    }
                    Some(_) => {}
                }
            }
        }
        self.at != start
    }

    /// §4.3.7: consume an escaped code point, the `\` already consumed.
    fn consume_escape(&mut self) -> char {
        match self.next_char() {
            None => '\u{fffd}',
            Some(c) if c.is_ascii_hexdigit() => {
                let mut value: u32 = c.to_digit(16).unwrap_or(0);
                let mut digits = 1;
                while digits < 6 {
                    match self.peek(0) {
                        Some(d) if d.is_ascii_hexdigit() => {
                            value = value * 16 + d.to_digit(16).unwrap_or(0);
                            self.at += 1;
                            digits += 1;
                        }
                        _ => break,
                    }
                }
                if self.peek(0).map(is_whitespace) == Some(true) {
                    self.at += 1;
                }
                // §4.3.7: zero, a surrogate and anything past the maximum
                // allowed code point are all U+FFFD.
                if value == 0 || (0xd800..=0xdfff).contains(&value) || value > 0x10_ffff {
                    '\u{fffd}'
                } else {
                    char::from_u32(value).unwrap_or('\u{fffd}')
                }
            }
            Some(c) => c,
        }
    }

    /// §4.3.11: consume a name.
    fn consume_name(&mut self) -> String {
        let mut out = String::new();
        loop {
            match self.peek(0) {
                Some(c) if is_name(c) => {
                    self.at += 1;
                    out.push(c);
                }
                Some('\\') if is_valid_escape(self.peek(0), self.peek(1)) => {
                    self.at += 1;
                    out.push(self.consume_escape());
                }
                _ => return out,
            }
        }
    }

    /// §4.3.9: would starting here begin an identifier?
    fn starts_ident(&self, offset: usize) -> bool {
        match self.peek(offset) {
            Some('-') => match self.peek(offset + 1) {
                Some('-') => true,
                Some(c) if is_name_start(c) => true,
                Some('\\') => is_valid_escape(self.peek(offset + 1), self.peek(offset + 2)),
                _ => false,
            },
            Some('\\') => is_valid_escape(self.peek(offset), self.peek(offset + 1)),
            Some(c) => is_name_start(c),
            None => false,
        }
    }

    /// §4.3.10: would starting here begin a number?
    fn starts_number(&self, offset: usize) -> bool {
        match self.peek(offset) {
            Some('+') | Some('-') => match self.peek(offset + 1) {
                Some(c) if c.is_ascii_digit() => true,
                Some('.') => self.peek(offset + 2).is_some_and(|c| c.is_ascii_digit()),
                _ => false,
            },
            Some('.') => self.peek(offset + 1).is_some_and(|c| c.is_ascii_digit()),
            Some(c) => c.is_ascii_digit(),
            None => false,
        }
    }

    /// §4.3.12: consume a number, returning its value and its type flag.
    fn consume_number(&mut self) -> (f64, bool) {
        let start = self.at;
        let mut integer = true;
        if matches!(self.peek(0), Some('+') | Some('-')) {
            self.at += 1;
        }
        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
            integer = false;
            self.at += 2;
            while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                self.at += 1;
            }
        }
        if matches!(self.peek(0), Some('e') | Some('E')) {
            let sign = matches!(self.peek(1), Some('+') | Some('-'));
            let digit_at = if sign { 2 } else { 1 };
            if self.peek(digit_at).is_some_and(|c| c.is_ascii_digit()) {
                integer = false;
                self.at += digit_at;
                while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                    self.at += 1;
                }
            }
        }
        let text: String = self.text[start..self.at].iter().collect();
        // `str::parse::<f64>` is `strtod`'s correctly-rounded answer, which is
        // ruling 4's requirement: the same digits give the same double on every
        // target this engine builds for.
        (text.parse::<f64>().unwrap_or(0.0), integer)
    }

    /// §4.3.4: consume a URL token, `url(` already consumed.
    fn consume_url(&mut self) -> Token {
        while self.peek(0).is_some_and(is_whitespace) {
            self.at += 1;
        }
        let mut out = String::new();
        loop {
            match self.next_char() {
                None => return Token::Url(out),
                Some(')') => return Token::Url(out),
                Some(c) if is_whitespace(c) => {
                    while self.peek(0).is_some_and(is_whitespace) {
                        self.at += 1;
                    }
                    return match self.next_char() {
                        None | Some(')') => Token::Url(out),
                        Some(_) => {
                            self.consume_bad_url();
                            Token::BadUrl
                        }
                    };
                }
                Some('"') | Some('\'') | Some('(') => {
                    self.consume_bad_url();
                    return Token::BadUrl;
                }
                Some('\\') => {
                    if is_valid_escape(Some('\\'), self.peek(0)) {
                        out.push(self.consume_escape());
                    } else {
                        self.consume_bad_url();
                        return Token::BadUrl;
                    }
                }
                Some(c) => out.push(c),
            }
        }
    }

    /// §4.3.14: consume the remnants of a bad url.
    fn consume_bad_url(&mut self) {
        loop {
            match self.next_char() {
                None | Some(')') => return,
                Some('\\') if is_valid_escape(Some('\\'), self.peek(0)) => {
                    self.consume_escape();
                }
                Some(_) => {}
            }
        }
    }

    /// §4.3.5: consume a string, the opening quote already consumed.
    fn consume_string(&mut self, quote: char) -> Token {
        let mut out = String::new();
        loop {
            match self.next_char() {
                None => return Token::Str(out),
                Some(c) if c == quote => return Token::Str(out),
                // A newline inside a string is a *bad-string*, and the newline
                // is **not** consumed: §4.3.5 reconsumes it, which is what lets
                // the declaration after it be found.
                Some('\n') => {
                    self.at -= 1;
                    return Token::BadString;
                }
                Some('\\') => match self.peek(0) {
                    None => {}
                    Some('\n') => {
                        self.at += 1;
                    }
                    Some(_) => out.push(self.consume_escape()),
                },
                Some(c) => out.push(c),
            }
        }
    }

    /// §4.3.1: the next token, or `None` at the end of the source.
    pub fn next_token(&mut self) -> Option<Token> {
        self.consume_comments();
        let c = self.peek(0)?;
        if is_whitespace(c) {
            // A comment between two whitespace runs is not a token boundary,
            // and a selector `a /* x */ b` is one descendant combinator rather
            // than two — so whitespace and comments collapse together until
            // neither is next.
            loop {
                while self.peek(0).is_some_and(is_whitespace) {
                    self.at += 1;
                }
                if self.peek(0) == Some('/') && self.peek(1) == Some('*') {
                    self.consume_comments();
                    continue;
                }
                break;
            }
            return Some(Token::Whitespace);
        }
        match c {
            '"' | '\'' => {
                self.at += 1;
                Some(self.consume_string(c))
            }
            '#' => {
                if self.peek(1).is_some_and(is_name) || is_valid_escape(self.peek(1), self.peek(2))
                {
                    self.at += 1;
                    let kind = if self.starts_ident(0) {
                        HashKind::Id
                    } else {
                        HashKind::Unrestricted
                    };
                    Some(Token::Hash(self.consume_name(), kind))
                } else {
                    self.at += 1;
                    Some(Token::Delim('#'))
                }
            }
            '(' => {
                self.at += 1;
                Some(Token::OpenParen)
            }
            ')' => {
                self.at += 1;
                Some(Token::CloseParen)
            }
            '[' => {
                self.at += 1;
                Some(Token::OpenSquare)
            }
            ']' => {
                self.at += 1;
                Some(Token::CloseSquare)
            }
            '{' => {
                self.at += 1;
                Some(Token::OpenCurly)
            }
            '}' => {
                self.at += 1;
                Some(Token::CloseCurly)
            }
            ',' => {
                self.at += 1;
                Some(Token::Comma)
            }
            ':' => {
                self.at += 1;
                Some(Token::Colon)
            }
            ';' => {
                self.at += 1;
                Some(Token::Semicolon)
            }
            '+' | '.' => {
                if self.starts_number(0) {
                    Some(self.consume_numeric())
                } else {
                    self.at += 1;
                    Some(Token::Delim(c))
                }
            }
            '-' => {
                if self.starts_number(0) {
                    Some(self.consume_numeric())
                } else if self.peek(1) == Some('-') && self.peek(2) == Some('>') {
                    self.at += 3;
                    Some(Token::Cdc)
                } else if self.starts_ident(0) {
                    Some(self.consume_ident_like())
                } else {
                    self.at += 1;
                    Some(Token::Delim('-'))
                }
            }
            '<' => {
                if self.peek(1) == Some('!')
                    && self.peek(2) == Some('-')
                    && self.peek(3) == Some('-')
                {
                    self.at += 4;
                    Some(Token::Cdo)
                } else {
                    self.at += 1;
                    Some(Token::Delim('<'))
                }
            }
            '@' => {
                if self.starts_ident(1) {
                    self.at += 1;
                    Some(Token::AtKeyword(self.consume_name()))
                } else {
                    self.at += 1;
                    Some(Token::Delim('@'))
                }
            }
            '\\' => {
                if is_valid_escape(self.peek(0), self.peek(1)) {
                    Some(self.consume_ident_like())
                } else {
                    self.at += 1;
                    Some(Token::Delim('\\'))
                }
            }
            c if c.is_ascii_digit() => Some(self.consume_numeric()),
            c if is_name_start(c) => Some(self.consume_ident_like()),
            other => {
                self.at += 1;
                Some(Token::Delim(other))
            }
        }
    }

    /// §4.3.3: consume a numeric token.
    fn consume_numeric(&mut self) -> Token {
        let (value, integer) = self.consume_number();
        if self.starts_ident(0) {
            let unit = self.consume_name();
            Token::Dimension { value, unit }
        } else if self.peek(0) == Some('%') {
            self.at += 1;
            Token::Percentage(value)
        } else {
            Token::Number { value, integer }
        }
    }

    /// §4.3.2: consume an ident-like token.
    fn consume_ident_like(&mut self) -> Token {
        let name = self.consume_name();
        if self.peek(0) == Some('(') {
            self.at += 1;
            if name.eq_ignore_ascii_case("url") {
                // §4.3.2: `url(` followed by a quote is a function token, not a
                // url token, and the string that follows is an ordinary string.
                let mut ahead = 0;
                while self.peek(ahead).is_some_and(is_whitespace) {
                    ahead += 1;
                }
                if matches!(self.peek(ahead), Some('"') | Some('\'')) {
                    return Token::Function(name);
                }
                return self.consume_url();
            }
            return Token::Function(name);
        }
        Token::Ident(name)
    }
}

/// Every token in a source, in order.
///
/// The count is the caller's to charge against [`crate::limits::MAX_CSS_TOKENS`]
/// — this returns the whole vector because §5's grammar needs to look back as
/// well as forward, and a pull interface would mean a second buffer inside the
/// parser.
pub fn tokenize(text: &str) -> Vec<Token> {
    let mut tokenizer = Tokenizer::new(text);
    let mut out = Vec::new();
    while let Some(token) = tokenizer.next_token() {
        out.push(token);
    }
    out
}
