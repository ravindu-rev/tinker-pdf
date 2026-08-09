//! Content-stream tokenizing (7.8.2).
//!
//! A content stream is postfix: operands then an operator. The tokenizer is
//! separate from the interpreter because inline images (8.9.7) need to be
//! scanned rather than parsed, and that scan has to happen at the byte level.

/// One lexical item of a content stream.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    /// An integer or real operand.
    Number(f64),
    /// A literal or hex string operand, decoded to its bytes.
    String(Vec<u8>),
    /// A name operand, without its slash.
    Name(Vec<u8>),
    /// `[`.
    ArrayOpen,
    /// `]`.
    ArrayClose,
    /// `<<`.
    DictOpen,
    /// `>>`.
    DictClose,
    /// `true` or `false`.
    Bool(bool),
    /// `null`.
    Null,
    /// An operator: everything else.
    Operator(Vec<u8>),
}

/// Splits a content stream into tokens.
pub struct Tokenizer<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    /// Starts at the beginning of `data`.
    #[must_use]
    pub fn new(data: &'a [u8]) -> Tokenizer<'a> {
        Tokenizer { data, pos: 0 }
    }

    /// The current byte offset, which inline-image scanning needs.
    #[must_use]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Moves to an absolute offset, clamped to the buffer.
    pub fn seek(&mut self, to: usize) {
        self.pos = to.min(self.data.len());
    }

    /// The remaining bytes, for the inline-image scanner.
    #[must_use]
    pub fn rest(&self) -> &'a [u8] {
        self.data.get(self.pos..).unwrap_or_default()
    }

    fn skip_trivia(&mut self) {
        while let Some(&b) = self.data.get(self.pos) {
            if b.is_ascii_whitespace() || b == 0 {
                self.pos += 1;
            } else if b == b'%' {
                // 7.2.3: a comment runs to the end of the line.
                while self
                    .data
                    .get(self.pos)
                    .is_some_and(|&c| c != b'\n' && c != b'\r')
                {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    /// The next token, or `None` at the end of the stream.
    pub fn next_token(&mut self) -> Option<Token> {
        self.skip_trivia();
        let b = *self.data.get(self.pos)?;

        match b {
            b'(' => Some(Token::String(self.literal_string())),
            b'<' => {
                if self.data.get(self.pos + 1) == Some(&b'<') {
                    self.pos += 2;
                    Some(Token::DictOpen)
                } else {
                    Some(Token::String(self.hex_string()))
                }
            }
            b'>' => {
                // A lone `>` is damage; consume it so scanning progresses.
                self.pos += if self.data.get(self.pos + 1) == Some(&b'>') {
                    2
                } else {
                    1
                };
                Some(Token::DictClose)
            }
            b'[' => {
                self.pos += 1;
                Some(Token::ArrayOpen)
            }
            b']' => {
                self.pos += 1;
                Some(Token::ArrayClose)
            }
            b'/' => {
                self.pos += 1;
                Some(Token::Name(self.regular_run_decoded()))
            }
            b'{' | b'}' => {
                // PostScript calculator braces appear in type 4 functions,
                // never in page content; consume rather than stall.
                self.pos += 1;
                self.next_token()
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => {
                let raw = self.regular_run();
                Some(Token::Number(parse_number(&raw)))
            }
            _ => {
                let raw = self.regular_run();
                if raw.is_empty() {
                    self.pos += 1;
                    return self.next_token();
                }
                match raw.as_slice() {
                    b"true" => Some(Token::Bool(true)),
                    b"false" => Some(Token::Bool(false)),
                    b"null" => Some(Token::Null),
                    _ => Some(Token::Operator(raw)),
                }
            }
        }
    }

    /// A run of regular characters, raw.
    fn regular_run(&mut self) -> Vec<u8> {
        let start = self.pos;
        while self
            .data
            .get(self.pos)
            .is_some_and(|&c| !is_delimiter(c) && !c.is_ascii_whitespace() && c != 0)
        {
            self.pos += 1;
        }
        self.data.get(start..self.pos).unwrap_or_default().to_vec()
    }

    /// A name's run, with `#xx` escapes decoded (7.3.5).
    fn regular_run_decoded(&mut self) -> Vec<u8> {
        let raw = self.regular_run();
        if !raw.contains(&b'#') {
            return raw;
        }
        let mut out = Vec::with_capacity(raw.len());
        let mut i = 0;
        while i < raw.len() {
            if raw.get(i) == Some(&b'#') {
                let hex = raw.get(i + 1..i + 3).and_then(|h| {
                    let text = std::str::from_utf8(h).ok()?;
                    u8::from_str_radix(text, 16).ok()
                });
                if let Some(byte) = hex {
                    out.push(byte);
                    i += 3;
                    continue;
                }
            }
            if let Some(&b) = raw.get(i) {
                out.push(b);
            }
            i += 1;
        }
        out
    }

    /// 7.3.4.2, the same rules the object parser applies.
    fn literal_string(&mut self) -> Vec<u8> {
        self.pos += 1; // the opening paren
        let mut out = Vec::new();
        let mut depth = 1u32;

        while let Some(&b) = self.data.get(self.pos) {
            self.pos += 1;
            match b {
                b'\\' => {
                    let Some(&esc) = self.data.get(self.pos) else {
                        break;
                    };
                    self.pos += 1;
                    match esc {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'(' => out.push(b'('),
                        b')' => out.push(b')'),
                        b'\\' => out.push(b'\\'),
                        b'\r' => {
                            // A line continuation; a following LF joins it.
                            if self.data.get(self.pos) == Some(&b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut value = u32::from(esc - b'0');
                            for _ in 0..2 {
                                match self.data.get(self.pos) {
                                    Some(&d @ b'0'..=b'7') => {
                                        value = value * 8 + u32::from(d - b'0');
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            // 7.3.4.2: values above 255 are taken mod 256.
                            out.push((value % 256) as u8);
                        }
                        other => out.push(other),
                    }
                }
                b'(' => {
                    depth += 1;
                    out.push(b'(');
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(b')');
                }
                b'\r' => {
                    // An unescaped CR or CRLF normalizes to LF.
                    if self.data.get(self.pos) == Some(&b'\n') {
                        self.pos += 1;
                    }
                    out.push(b'\n');
                }
                other => out.push(other),
            }
        }

        out
    }

    /// 7.3.4.3.
    fn hex_string(&mut self) -> Vec<u8> {
        self.pos += 1; // the opening angle
        let mut nibbles = Vec::new();
        while let Some(&b) = self.data.get(self.pos) {
            self.pos += 1;
            if b == b'>' {
                break;
            }
            if let Some(v) = (b as char).to_digit(16) {
                nibbles.push(v as u8);
            }
        }
        if nibbles.len() % 2 == 1 {
            // An odd digit count pads with zero.
            nibbles.push(0);
        }
        nibbles
            .chunks_exact(2)
            .map(|p| (p[0] << 4) | p[1])
            .collect()
    }
}

fn is_delimiter(c: u8) -> bool {
    matches!(
        c,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Parses a content-stream number, leniently.
///
/// Real streams contain `--5`, `.5.3` and bare `-`; each parses to the longest
/// sensible prefix rather than stopping the page.
fn parse_number(raw: &[u8]) -> f64 {
    let text = String::from_utf8_lossy(raw);
    if let Ok(v) = text.parse::<f64>() {
        return if v.is_finite() { v } else { 0.0 };
    }

    let mut chars = text.chars().peekable();

    // Producers emit `--5` and `+-3`; every leading sign is consumed and the
    // first one decides, which is what the object lexer does too.
    let mut negative = false;
    let mut first_sign = true;
    while let Some(c) = chars.peek() {
        match c {
            '-' | '+' => {
                if first_sign {
                    negative = *c == '-';
                    first_sign = false;
                }
                chars.next();
            }
            _ => break,
        }
    }

    let mut digits = String::with_capacity(text.len());
    let mut seen_dot = false;
    for c in chars {
        match c {
            '.' if !seen_dot => {
                seen_dot = true;
                digits.push('.');
            }
            '0'..='9' => digits.push(c),
            _ => break,
        }
    }

    let magnitude = digits
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .unwrap_or(0.0);
    if negative {
        -magnitude
    } else {
        magnitude
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(src: &[u8]) -> Vec<Token> {
        let mut t = Tokenizer::new(src);
        let mut out = Vec::new();
        while let Some(token) = t.next_token() {
            out.push(token);
        }
        out
    }

    #[test]
    fn a_text_showing_sequence_tokenizes() {
        let got = tokens(b"BT /F0 18 Tf 72 742 Td (Hi) Tj ET");
        assert_eq!(
            got,
            vec![
                Token::Operator(b"BT".to_vec()),
                Token::Name(b"F0".to_vec()),
                Token::Number(18.0),
                Token::Operator(b"Tf".to_vec()),
                Token::Number(72.0),
                Token::Number(742.0),
                Token::Operator(b"Td".to_vec()),
                Token::String(b"Hi".to_vec()),
                Token::Operator(b"Tj".to_vec()),
                Token::Operator(b"ET".to_vec()),
            ]
        );
    }

    #[test]
    fn literal_strings_handle_every_escape() {
        assert_eq!(
            tokens(br"(a\(b\)c)"),
            vec![Token::String(b"a(b)c".to_vec())]
        );
        assert_eq!(tokens(br"(\101\102)"), vec![Token::String(b"AB".to_vec())]);
        assert_eq!(
            tokens(br"(nested (parens) ok)")[0],
            Token::String(b"nested (parens) ok".to_vec())
        );
        assert_eq!(
            tokens(b"(line\\\ncontinued)")[0],
            Token::String(b"linecontinued".to_vec())
        );
        assert_eq!(tokens(br"(\n\r\t)")[0], Token::String(vec![10, 13, 9]));
        // An octal escape above 255 wraps.
        assert_eq!(tokens(br"(\400)")[0], Token::String(vec![0]));
    }

    #[test]
    fn hex_strings_pad_an_odd_digit_count() {
        assert_eq!(
            tokens(b"<48656C6C6F>"),
            vec![Token::String(b"Hello".to_vec())]
        );
        assert_eq!(tokens(b"<414>"), vec![Token::String(vec![0x41, 0x40])]);
        assert_eq!(tokens(b"<41 42>"), vec![Token::String(vec![0x41, 0x42])]);
    }

    #[test]
    fn names_decode_their_escapes() {
        assert_eq!(tokens(b"/A#20B"), vec![Token::Name(b"A B".to_vec())]);
        assert_eq!(tokens(b"/Simple"), vec![Token::Name(b"Simple".to_vec())]);
    }

    #[test]
    fn malformed_numbers_take_their_longest_prefix() {
        assert_eq!(parse_number(b"5"), 5.0);
        assert_eq!(parse_number(b"-3.5"), -3.5);
        assert_eq!(parse_number(b"--5"), -5.0);
        assert_eq!(parse_number(b".5"), 0.5);
        assert_eq!(parse_number(b"4."), 4.0);
        assert_eq!(parse_number(b"-"), 0.0);
        assert_eq!(parse_number(b"1.2.3"), 1.2);
        assert_eq!(parse_number(b"nonsense"), 0.0);
    }

    #[test]
    fn comments_and_dictionaries_are_recognized() {
        assert_eq!(
            tokens(b"% a comment\n<< /Type /X >>"),
            vec![
                Token::DictOpen,
                Token::Name(b"Type".to_vec()),
                Token::Name(b"X".to_vec()),
                Token::DictClose,
            ]
        );
    }

    #[test]
    fn arbitrary_bytes_terminate_without_panicking() {
        for src in [
            b"".as_slice(),
            b"(unterminated",
            b"<4",
            b"<<",
            b"/",
            b"\\",
            b")))",
            &[0xFF; 64],
        ] {
            let _ = tokens(src);
        }
    }
}
