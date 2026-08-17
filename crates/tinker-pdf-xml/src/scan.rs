//! A cursor over decoded text, and the four lexical shapes every construct is
//! built from.
//!
//! The cursor works in byte offsets over `&str` and every advance lands on a
//! character boundary, because every advance is either a delimiter this
//! grammar spells in ASCII or the length of a character it just read. Nothing
//! indexes; `get` is used throughout, so a malformed input runs off the end
//! rather than off a slice.

use crate::text::{is_name_char, is_name_start, is_space};
use crate::{Construct, Error};

/// Where the parser is, in bytes from the start of the decoded text.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Cursor<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Cursor { text, at: 0 }
    }

    pub(crate) fn offset(&self) -> usize {
        self.at
    }

    pub(crate) fn rest(&self) -> &'a str {
        self.text.get(self.at..).unwrap_or("")
    }

    pub(crate) fn at_end(&self) -> bool {
        self.rest().is_empty()
    }

    pub(crate) fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    pub(crate) fn starts_with(&self, what: &str) -> bool {
        self.rest().starts_with(what)
    }

    /// Consumes `what` if it is there, and says whether it was.
    pub(crate) fn eat(&mut self, what: &str) -> bool {
        if self.starts_with(what) {
            self.at += what.len();
            true
        } else {
            false
        }
    }

    /// Consumes `S` (§2.3) and says whether there was any.
    ///
    /// The answer matters: `<a b="1"c="2"/>` is not well formed, and the only
    /// thing that distinguishes it from `<a b="1" c="2"/>` is that no
    /// whitespace was consumed between the two attributes.
    pub(crate) fn skip_space(&mut self) -> bool {
        let before = self.at;
        while let Some(c) = self.peek() {
            if !is_space(c) {
                break;
            }
            self.at += c.len_utf8();
        }
        self.at != before
    }

    /// A `Name` (§2.3), as the source spells it.
    pub(crate) fn name(&mut self, max_len: usize) -> Result<&'a str, Error> {
        let start = self.at;
        match self.peek() {
            Some(c) if is_name_start(c) => self.at += c.len_utf8(),
            _ => return Err(Error::MalformedName),
        }
        while let Some(c) = self.peek() {
            if !is_name_char(c) {
                break;
            }
            self.at += c.len_utf8();
        }
        let name = self.text.get(start..self.at).unwrap_or("");
        if name.len() > max_len {
            // Refused, never truncated: `FixedPage` and `FixedPag` are
            // different tags and nothing downstream could tell which it had.
            return Err(Error::NameCap);
        }
        Ok(name)
    }

    /// Everything up to `terminator`, and the terminator consumed.
    ///
    /// The match is on the whole delimiter rather than on its first byte, which
    /// is the difference between reading `<![CDATA[a]]b]]>` as `a]]b` and
    /// reading it as `a`. `find` searches the rest of the input, so a construct
    /// that never terminates is refused as unterminated rather than closed at
    /// the end of the file.
    pub(crate) fn until(
        &mut self,
        terminator: &str,
        construct: Construct,
    ) -> Result<&'a str, Error> {
        let rest = self.rest();
        let Some(end) = rest.find(terminator) else {
            self.at = self.text.len();
            return Err(Error::Unterminated(construct));
        };
        let body = rest.get(..end).unwrap_or("");
        self.at += end + terminator.len();
        Ok(body)
    }

    /// A quoted attribute value, verbatim, with the quotes consumed.
    ///
    /// §3.1's `AttValue` forbids a literal `<` inside one, and that rule is
    /// worth keeping rather than waving through: an unterminated start tag —
    /// `<a b="1><c/>` — is otherwise read as an attribute whose value runs to
    /// the next quote somewhere down the page, which produces a plausible
    /// element with the wrong content instead of a refusal.
    pub(crate) fn attribute_value(&mut self) -> Result<&'a str, Error> {
        let quote = match self.peek() {
            Some(q @ ('"' | '\'')) => q,
            _ => return Err(Error::UnquotedAttributeValue),
        };
        self.at += quote.len_utf8();
        let rest = self.rest();
        let Some(end) = rest.find(quote) else {
            self.at = self.text.len();
            return Err(Error::Unterminated(Construct::AttributeValue));
        };
        let body = rest.get(..end).unwrap_or("");
        if body.contains('<') {
            return Err(Error::LessThanInAttributeValue);
        }
        self.at += end + quote.len_utf8();
        Ok(body)
    }
}
