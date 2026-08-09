//! CMaps: `/ToUnicode` and CID mappings (9.7.5, 9.10.3).
//!
//! A CMap is PostScript-flavoured but only a handful of operators matter, so
//! this is a small scanner rather than an interpreter — the same decision every
//! reader makes, and the reason a malformed CMap degrades to partial coverage
//! instead of taking a document down.
//!
//! Codespace ranges are what make a CMap more than a table: they say how many
//! bytes each code occupies, and a font with mixed one- and two-byte codes is
//! unreadable without them.

use std::collections::HashMap;

/// One codespace range (9.7.6.2): codes of a fixed byte length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodespaceRange {
    /// How many bytes a code in this range occupies, 1 to 4.
    pub bytes: u8,
    /// Inclusive low bound, as a big-endian integer.
    pub low: u32,
    /// Inclusive high bound.
    pub high: u32,
}

/// A parsed CMap.
#[derive(Clone, Debug, Default)]
pub struct CMap {
    codespaces: Vec<CodespaceRange>,
    single: HashMap<u32, Vec<char>>,
    ranges: Vec<(u32, u32, Vec<char>)>,
    cid_single: HashMap<u32, u32>,
    cid_ranges: Vec<(u32, u32, u32)>,
    /// True for `Identity-H` and `Identity-V`, where code equals CID.
    identity: bool,
    /// True for `Identity-V` and other vertical CMaps (9.7.4.3).
    vertical: bool,
    /// True when this stands in for a predefined CMap whose data is not built
    /// in, so the code-to-CID mapping is assumed rather than known.
    approximate: bool,
}

impl CMap {
    /// The identity CMap of `Identity-H` or `Identity-V`: two-byte codes whose
    /// value is the CID.
    #[must_use]
    pub fn identity(vertical: bool) -> CMap {
        CMap {
            codespaces: vec![CodespaceRange {
                bytes: 2,
                low: 0,
                high: 0xFFFF,
            }],
            identity: true,
            vertical,
            ..CMap::default()
        }
    }

    /// One of the predefined CMaps, by name (9.7.5.2).
    ///
    /// Only the identity pair is built in. The CJK collections are large data
    /// files; a font using one still lays out, because its codes are two bytes
    /// and its `/W` array supplies the advances — only the CID mapping is
    /// approximate, and that is reported by [`CMap::is_approximate`].
    #[must_use]
    pub fn predefined(name: &[u8]) -> Option<CMap> {
        if name == b"Identity-H" {
            return Some(CMap::identity(false));
        }
        if name == b"Identity-V" {
            return Some(CMap::identity(true));
        }

        // The CJK collections are recognized by their registry prefixes. Their
        // real code-to-CID tables are large data files that are not built in,
        // so this stands in with the one property they all share: two-byte
        // codes. Advances still come from the font's own `/W` array, so text
        // positions correctly; only the CID mapping is assumed, and
        // `is_approximate` says so.
        const CJK_PREFIXES: [&[u8]; 14] = [
            b"UniJIS", b"UniGB", b"UniCNS", b"UniKS", b"GBK-", b"GB-", b"ETen", b"90ms", b"90pv",
            b"B5pc", b"KSC", b"Add-", b"Ext-", b"RKSJ",
        ];
        let known =
            CJK_PREFIXES.iter().any(|p| name.starts_with(p)) || name == b"H" || name == b"V";
        known.then(|| CMap {
            codespaces: vec![CodespaceRange {
                bytes: 2,
                low: 0,
                high: 0xFFFF,
            }],
            identity: true,
            vertical: name == b"V" || name.ends_with(b"-V"),
            approximate: true,
            ..CMap::default()
        })
    }

    /// Whether this CMap stands in for one whose data is not built in.
    #[must_use]
    pub fn is_approximate(&self) -> bool {
        self.approximate
    }

    /// Whether the writing mode is vertical (9.7.4.3).
    #[must_use]
    pub fn is_vertical(&self) -> bool {
        self.vertical
    }

    /// Splits a string into codes, using the codespace ranges.
    ///
    /// A byte sequence matching no range is consumed one byte at a time, which
    /// is what 9.7.6.3 prescribes and keeps a damaged string from swallowing
    /// the rest of the text.
    #[must_use]
    pub fn decode_codes(&self, bytes: &[u8]) -> Vec<(u32, u8)> {
        let mut out = Vec::new();
        let mut i = 0usize;

        // With no codespace at all, a simple font's one-byte default applies.
        let default_len = if self.codespaces.is_empty() {
            if self.identity {
                2
            } else {
                1
            }
        } else {
            0
        };

        while i < bytes.len() {
            let mut matched = None;
            if default_len == 0 {
                for len in 1..=4usize {
                    let Some(slice) = bytes.get(i..i + len) else {
                        continue;
                    };
                    let value = slice.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b));
                    if self
                        .codespaces
                        .iter()
                        .any(|r| usize::from(r.bytes) == len && (r.low..=r.high).contains(&value))
                    {
                        matched = Some((value, len as u8));
                        break;
                    }
                }
            }

            let (value, len) = matched.unwrap_or_else(|| {
                let len = if default_len > 0 { default_len } else { 1 }.min(bytes.len() - i);
                let slice = bytes.get(i..i + len).unwrap_or_default();
                (
                    slice.iter().fold(0u32, |a, &b| (a << 8) | u32::from(b)),
                    len as u8,
                )
            });

            out.push((value, len));
            i += usize::from(len).max(1);
        }

        out
    }

    /// The text a code maps to, if this CMap is a `/ToUnicode`.
    #[must_use]
    pub fn to_unicode(&self, code: u32) -> Option<&[char]> {
        if let Some(chars) = self.single.get(&code) {
            return Some(chars);
        }
        for (low, high, base) in &self.ranges {
            if (*low..=*high).contains(&code) {
                return Some(base);
            }
        }
        None
    }

    /// The text a code maps to, with a range's offset applied.
    ///
    /// `bfrange` with a destination string means consecutive codes take
    /// consecutive characters, which matters for the common case of an entire
    /// alphabet in one entry.
    #[must_use]
    pub fn to_unicode_string(&self, code: u32) -> Option<String> {
        if let Some(chars) = self.single.get(&code) {
            return Some(chars.iter().collect());
        }
        for (low, high, base) in &self.ranges {
            if (*low..=*high).contains(&code) {
                let offset = code - low;
                let mut chars = base.clone();
                // Only the final character advances; the rest is a prefix.
                if let Some(last) = chars.last_mut() {
                    if let Some(shifted) = char::from_u32(u32::from(*last) + offset) {
                        *last = shifted;
                    }
                }
                return Some(chars.into_iter().collect());
            }
        }
        None
    }

    /// The CID a code maps to (9.7.5).
    #[must_use]
    pub fn cid(&self, code: u32) -> Option<u32> {
        if self.identity {
            return Some(code);
        }
        if let Some(cid) = self.cid_single.get(&code) {
            return Some(*cid);
        }
        for (low, high, base) in &self.cid_ranges {
            if (*low..=*high).contains(&code) {
                return Some(base + (code - low));
            }
        }
        None
    }
}

/// Parses an embedded CMap stream.
///
/// The scanner reads only the operators that carry mappings; everything else,
/// including the PostScript prologue every CMap opens with, is skipped. A
/// truncated or malformed section ends that section rather than the parse.
#[must_use]
pub fn parse(bytes: &[u8]) -> CMap {
    let mut cmap = CMap::default();
    let mut tokens = Tokenizer::new(bytes);
    let mut stack: Vec<Token> = Vec::new();

    while let Some(token) = tokens.next_token() {
        match &token {
            Token::Keyword(k) if k == b"begincodespacerange" => {
                read_codespaces(&mut tokens, &mut cmap);
                stack.clear();
            }
            Token::Keyword(k) if k == b"beginbfchar" => {
                read_bfchar(&mut tokens, &mut cmap);
                stack.clear();
            }
            Token::Keyword(k) if k == b"beginbfrange" => {
                read_bfrange(&mut tokens, &mut cmap);
                stack.clear();
            }
            Token::Keyword(k) if k == b"begincidchar" => {
                read_cidchar(&mut tokens, &mut cmap);
                stack.clear();
            }
            Token::Keyword(k) if k == b"begincidrange" => {
                read_cidrange(&mut tokens, &mut cmap);
                stack.clear();
            }
            Token::Keyword(k) if k == b"def" => {
                // `/WMode 1 def` selects vertical writing (9.7.5.1).
                if let (Some(Token::Number(n)), Some(Token::Name(name))) =
                    (stack.last(), stack.get(stack.len().wrapping_sub(2)))
                {
                    if name == b"WMode" && *n == 1 {
                        cmap.vertical = true;
                    }
                }
                stack.clear();
            }
            _ => {
                stack.push(token);
                if stack.len() > 8 {
                    stack.remove(0);
                }
            }
        }
    }

    cmap
}

fn read_codespaces(tokens: &mut Tokenizer, cmap: &mut CMap) {
    while let Some(token) = tokens.next_token() {
        let Token::HexString(low) = token else {
            return; // `endcodespacerange`, or something unexpected.
        };
        let Some(Token::HexString(high)) = tokens.next_token() else {
            return;
        };
        let bytes = low.len().clamp(1, 4) as u8;
        cmap.codespaces.push(CodespaceRange {
            bytes,
            low: be(&low),
            high: be(&high),
        });
    }
}

fn read_bfchar(tokens: &mut Tokenizer, cmap: &mut CMap) {
    while let Some(token) = tokens.next_token() {
        let Token::HexString(code) = token else {
            return;
        };
        match tokens.next_token() {
            Some(Token::HexString(dst)) => {
                cmap.single.insert(be(&code), utf16be_chars(&dst));
            }
            Some(Token::Name(name)) => {
                if let Some(c) =
                    crate::encoding::glyph_name_to_char(&String::from_utf8_lossy(&name))
                {
                    cmap.single.insert(be(&code), vec![c]);
                }
            }
            _ => return,
        }
    }
}

fn read_bfrange(tokens: &mut Tokenizer, cmap: &mut CMap) {
    while let Some(token) = tokens.next_token() {
        let Token::HexString(low) = token else {
            return;
        };
        let Some(Token::HexString(high)) = tokens.next_token() else {
            return;
        };
        match tokens.next_token() {
            Some(Token::HexString(dst)) => {
                cmap.ranges.push((be(&low), be(&high), utf16be_chars(&dst)));
            }
            // The array form gives one destination per code (9.10.3).
            Some(Token::ArrayOpen) => {
                let mut code = be(&low);
                while let Some(item) = tokens.next_token() {
                    match item {
                        Token::HexString(dst) => {
                            cmap.single.insert(code, utf16be_chars(&dst));
                            code = code.saturating_add(1);
                        }
                        Token::ArrayClose => break,
                        _ => break,
                    }
                }
            }
            _ => return,
        }
    }
}

fn read_cidchar(tokens: &mut Tokenizer, cmap: &mut CMap) {
    while let Some(token) = tokens.next_token() {
        let Token::HexString(code) = token else {
            return;
        };
        let Some(Token::Number(cid)) = tokens.next_token() else {
            return;
        };
        if let Ok(cid) = u32::try_from(cid) {
            cmap.cid_single.insert(be(&code), cid);
        }
    }
}

fn read_cidrange(tokens: &mut Tokenizer, cmap: &mut CMap) {
    while let Some(token) = tokens.next_token() {
        let Token::HexString(low) = token else {
            return;
        };
        let Some(Token::HexString(high)) = tokens.next_token() else {
            return;
        };
        let Some(Token::Number(cid)) = tokens.next_token() else {
            return;
        };
        if let Ok(cid) = u32::try_from(cid) {
            cmap.cid_ranges.push((be(&low), be(&high), cid));
        }
    }
}

fn be(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .take(4)
        .fold(0u32, |a, &b| (a << 8) | u32::from(b))
}

/// A CMap destination string is UTF-16BE (9.10.3).
fn utf16be_chars(bytes: &[u8]) -> Vec<char> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|p| u16::from_be_bytes([p[0], p[1]]))
        .collect();
    if units.is_empty() {
        // A single byte destination occurs in hand-written CMaps.
        return bytes.iter().map(|&b| char::from(b)).collect();
    }
    char::decode_utf16(units)
        .map(|r| r.unwrap_or('\u{FFFD}'))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    HexString(Vec<u8>),
    Name(Vec<u8>),
    Number(i64),
    Keyword(Vec<u8>),
    ArrayOpen,
    ArrayClose,
}

struct Tokenizer<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(bytes: &'a [u8]) -> Tokenizer<'a> {
        Tokenizer { bytes, pos: 0 }
    }

    fn next_token(&mut self) -> Option<Token> {
        loop {
            let b = *self.bytes.get(self.pos)?;
            if b.is_ascii_whitespace() {
                self.pos += 1;
                continue;
            }
            if b == b'%' {
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|&c| c != b'\n' && c != b'\r')
                {
                    self.pos += 1;
                }
                continue;
            }
            break;
        }

        let b = *self.bytes.get(self.pos)?;
        match b {
            b'<' => {
                self.pos += 1;
                let mut hex = Vec::new();
                let mut nibbles = Vec::new();
                while let Some(&c) = self.bytes.get(self.pos) {
                    self.pos += 1;
                    if c == b'>' {
                        break;
                    }
                    if let Some(v) = (c as char).to_digit(16) {
                        nibbles.push(v as u8);
                    }
                }
                if nibbles.len() % 2 == 1 {
                    nibbles.push(0);
                }
                for pair in nibbles.chunks_exact(2) {
                    hex.push((pair[0] << 4) | pair[1]);
                }
                Some(Token::HexString(hex))
            }
            b'/' => {
                self.pos += 1;
                let start = self.pos;
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|&c| !is_delimiter(c) && !c.is_ascii_whitespace())
                {
                    self.pos += 1;
                }
                Some(Token::Name(
                    self.bytes.get(start..self.pos).unwrap_or_default().to_vec(),
                ))
            }
            b'[' => {
                self.pos += 1;
                Some(Token::ArrayOpen)
            }
            b']' => {
                self.pos += 1;
                Some(Token::ArrayClose)
            }
            b'0'..=b'9' | b'-' | b'+' | b'.' => {
                let start = self.pos;
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|&c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
                {
                    self.pos += 1;
                }
                let text = String::from_utf8_lossy(self.bytes.get(start..self.pos)?);
                Some(Token::Number(
                    text.parse::<f64>().ok().map(|v| v as i64).unwrap_or(0),
                ))
            }
            _ => {
                let start = self.pos;
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|&c| !is_delimiter(c) && !c.is_ascii_whitespace())
                {
                    self.pos += 1;
                }
                if self.pos == start {
                    self.pos += 1;
                }
                Some(Token::Keyword(
                    self.bytes.get(start..self.pos).unwrap_or_default().to_vec(),
                ))
            }
        }
    }
}

fn is_delimiter(c: u8) -> bool {
    matches!(
        c,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bfchar_entries_map_single_codes() {
        let src = b"
            /CIDInit /ProcSet findresource begin
            1 begincodespacerange <00> <FF> endcodespacerange
            2 beginbfchar
            <41> <0041>
            <42> <00420043>
            endbfchar
        ";
        let cmap = parse(src);
        assert_eq!(cmap.to_unicode_string(0x41).as_deref(), Some("A"));
        assert_eq!(
            cmap.to_unicode_string(0x42).as_deref(),
            Some("BC"),
            "a destination may be more than one character"
        );
        assert_eq!(cmap.to_unicode_string(0x43), None);
    }

    #[test]
    fn bfrange_advances_the_last_character() {
        let src = b"
            1 begincodespacerange <0000> <FFFF> endcodespacerange
            1 beginbfrange
            <0041> <0043> <0061>
            endbfrange
        ";
        let cmap = parse(src);
        assert_eq!(cmap.to_unicode_string(0x41).as_deref(), Some("a"));
        assert_eq!(cmap.to_unicode_string(0x42).as_deref(), Some("b"));
        assert_eq!(cmap.to_unicode_string(0x43).as_deref(), Some("c"));
        assert_eq!(cmap.to_unicode_string(0x44), None, "past the range");
    }

    #[test]
    fn bfrange_array_form_gives_one_destination_each() {
        let src = b"
            1 beginbfrange
            <0001> <0003> [<0058> <0059> <005A>]
            endbfrange
        ";
        let cmap = parse(src);
        assert_eq!(cmap.to_unicode_string(1).as_deref(), Some("X"));
        assert_eq!(cmap.to_unicode_string(2).as_deref(), Some("Y"));
        assert_eq!(cmap.to_unicode_string(3).as_deref(), Some("Z"));
    }

    #[test]
    fn codespace_ranges_decide_code_length() {
        let src = b"
            2 begincodespacerange
            <00> <80>
            <8140> <9FFC>
            endcodespacerange
        ";
        let cmap = parse(src);
        // A one-byte code, then a two-byte one.
        let codes = cmap.decode_codes(&[0x41, 0x81, 0x50]);
        assert_eq!(codes, vec![(0x41, 1), (0x8150, 2)]);
    }

    #[test]
    fn identity_maps_two_byte_codes_to_themselves() {
        let cmap = CMap::identity(false);
        assert_eq!(cmap.decode_codes(&[0x00, 0x41]), vec![(0x41, 2)]);
        assert_eq!(cmap.cid(0x41), Some(0x41));
        assert!(!cmap.is_vertical());
        assert!(CMap::identity(true).is_vertical());
    }

    #[test]
    fn cid_ranges_offset_from_their_base() {
        let src = b"
            1 begincidrange
            <0020> <007E> 1
            endcidrange
        ";
        let cmap = parse(src);
        assert_eq!(cmap.cid(0x20), Some(1));
        assert_eq!(cmap.cid(0x21), Some(2));
        assert_eq!(cmap.cid(0x7E), Some(95));
        assert_eq!(cmap.cid(0x7F), None);
    }

    #[test]
    fn vertical_writing_is_read_from_wmode() {
        assert!(parse(b"/WMode 1 def").is_vertical());
        assert!(!parse(b"/WMode 0 def").is_vertical());
    }

    #[test]
    fn malformed_input_ends_a_section_not_the_parse() {
        // A truncated bfchar must not lose the codespace before it.
        let cmap = parse(b"1 begincodespacerange <00> <FF> endcodespacerange 5 beginbfchar <41>");
        assert_eq!(cmap.decode_codes(&[0x41]), vec![(0x41, 1)]);
        for bytes in [b"".as_slice(), b"garbage", b"<<<<", b"beginbfrange"] {
            let _ = parse(bytes);
        }
    }
}
