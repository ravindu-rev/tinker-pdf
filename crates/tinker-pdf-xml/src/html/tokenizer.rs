//! HTML §13.2.5, the tokenization stage: characters in, tokens out.
//!
//! One state per subsection of §13.2.5, named as the standard names it, and
//! each arm of [`Tokenizer::step`] is that subsection's list read in order —
//! so a reader holding the standard beside this file can check it line by
//! line. Three liberties, none of which changes a token:
//!
//! - **Character tokens are runs.** The standard emits one token per
//!   character; this emits a run of them as one [`Token::Characters`], and the
//!   tree builder splits a run where its rules treat white space differently.
//!   A run ends at every `<`, so the tree builder has processed all the text
//!   before a tag before the tokenizer reads the tag — which is what makes
//!   [`Tokenizer::allow_cdata`], a question about the tree, answerable.
//! - **U+0000 in the data state is its own token**, [`Token::Null`], because
//!   the tree builder ignores it in some modes and replaces it in others.
//! - **`<?` is a bogus comment.** The standard's text as fetched on
//!   3 October 2026 had begun to give HTML processing instructions; the
//!   html5lib tree-construction tests this parser is held to were written
//!   before that change and expect the long-standing comment, so that is what
//!   is read.

use std::collections::VecDeque;

use super::entities;
use crate::{Error, Limits};

/// A start or end tag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tag {
    /// Lowercased, as §13.2.5.8 appends it.
    pub(crate) name: String,
    /// In source order, the first of any two with one name kept.
    pub(crate) attributes: Vec<(String, String)>,
    pub(crate) self_closing: bool,
}

impl Tag {
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A DOCTYPE token: §13.2.5's three fields that may be *missing*, which is
/// not the empty string, and the force-quirks flag.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DoctypeToken {
    pub(crate) name: Option<String>,
    pub(crate) public_id: Option<String>,
    pub(crate) system_id: Option<String>,
    pub(crate) force_quirks: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Token {
    Doctype(DoctypeToken),
    StartTag(Tag),
    EndTag(Tag),
    Comment(String),
    Characters(String),
    /// U+0000 from the data or CDATA section state, which the tree builder
    /// decides about.
    Null,
    Eof,
}

/// The states of §13.2.5, by the names it gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
    TagOpen,
    EndTagOpen,
    TagName,
    RcdataLessThan,
    RcdataEndTagOpen,
    RcdataEndTagName,
    RawtextLessThan,
    RawtextEndTagOpen,
    RawtextEndTagName,
    ScriptDataLessThan,
    ScriptDataEndTagOpen,
    ScriptDataEndTagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLessThan,
    ScriptDataEscapedEndTagOpen,
    ScriptDataEscapedEndTagName,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLessThan,
    ScriptDataDoubleEscapeEnd,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLessThan,
    CommentLessThanBang,
    CommentLessThanBangDash,
    CommentLessThanBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    AfterDoctypePublicKeyword,
    BeforeDoctypePublicIdentifier,
    DoctypePublicIdentifierDoubleQuoted,
    DoctypePublicIdentifierSingleQuoted,
    AfterDoctypePublicIdentifier,
    BetweenDoctypePublicAndSystemIdentifiers,
    AfterDoctypeSystemKeyword,
    BeforeDoctypeSystemIdentifier,
    DoctypeSystemIdentifierDoubleQuoted,
    DoctypeSystemIdentifierSingleQuoted,
    AfterDoctypeSystemIdentifier,
    BogusDoctype,
    CdataSection,
    CdataSectionBracket,
    CdataSectionEnd,
    CharacterReference,
    NamedCharacterReference,
    AmbiguousAmpersand,
    NumericCharacterReference,
    HexadecimalCharacterReferenceStart,
    DecimalCharacterReference,
    HexadecimalCharacterReference,
    NumericCharacterReferenceEnd,
}

/// §13.2.5's white space in the tokenizer: tab, line feed, form feed and
/// space. Carriage return never reaches it — §13.2.3.5 normalised it away.
fn is_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0C' | ' ')
}

/// The longest named character reference: `CounterClockwiseContourIntegral;`.
const LONGEST_NAME: usize = 32;

pub(crate) struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
    current: Option<char>,
    reconsume: bool,
    pub(crate) state: State,
    return_state: State,
    buffer: String,
    tag: Tag,
    end_tag: bool,
    attribute: Option<(String, String)>,
    comment: String,
    doctype: DoctypeToken,
    last_start_tag: String,
    code: u32,
    text: String,
    queue: VecDeque<Token>,
    finished: bool,
    /// Whether the adjusted current node is outside the HTML namespace, which
    /// is what decides whether `<![CDATA[` opens a CDATA section (§13.2.5.42).
    /// The tree builder sets it before asking for each token.
    pub(crate) allow_cdata: bool,
    pub(crate) errors: usize,
    limits: Limits,
    pub(crate) stop: Option<Error>,
}

impl<'a> Tokenizer<'a> {
    pub(crate) fn new(input: &'a str, limits: &Limits) -> Self {
        Tokenizer {
            input,
            pos: 0,
            current: None,
            reconsume: false,
            state: State::Data,
            return_state: State::Data,
            buffer: String::new(),
            tag: Tag::default(),
            end_tag: false,
            attribute: None,
            comment: String::new(),
            doctype: DoctypeToken::default(),
            last_start_tag: String::new(),
            code: 0,
            text: String::new(),
            queue: VecDeque::new(),
            finished: false,
            allow_cdata: false,
            errors: 0,
            limits: *limits,
            stop: None,
        }
    }

    /// The tag name an end tag must match to be *appropriate*: html5lib's
    /// `lastStartTag`, for a test that starts in a text state.
    #[cfg(test)]
    pub(crate) fn set_last_start_tag(&mut self, name: &str) {
        self.last_start_tag = name.to_owned();
    }

    /// The next token. After [`Token::Eof`] every call returns another.
    pub(crate) fn next_token(&mut self) -> Token {
        loop {
            if let Some(token) = self.queue.pop_front() {
                return token;
            }
            if self.finished {
                return Token::Eof;
            }
            self.step();
        }
    }

    fn error(&mut self) {
        self.errors = self.errors.saturating_add(1);
    }

    /// Stops at a cap: everything already queued is discarded, because a tag
    /// half built is not a tag, and the next token is the end of the file.
    fn fail(&mut self, error: Error) {
        if self.stop.is_none() {
            self.stop = Some(error);
        }
        self.text.clear();
        self.queue.clear();
        self.queue.push_back(Token::Eof);
        self.finished = true;
    }

    fn next_char(&mut self) -> Option<char> {
        if self.reconsume {
            self.reconsume = false;
            return self.current;
        }
        let c = self
            .input
            .get(self.pos..)
            .and_then(|rest| rest.chars().next());
        if let Some(c) = c {
            self.pos += c.len_utf8();
        }
        self.current = c;
        c
    }

    fn reconsume_in(&mut self, state: State) {
        self.reconsume = true;
        self.state = state;
    }

    fn rest(&self) -> &'a str {
        self.input.get(self.pos..).unwrap_or("")
    }

    fn emit_char(&mut self, c: char) {
        self.text.push(c);
    }

    fn emit_str(&mut self, s: &str) {
        self.text.push_str(s);
    }

    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            let text = std::mem::take(&mut self.text);
            self.queue.push_back(Token::Characters(text));
        }
    }

    fn emit(&mut self, token: Token) {
        self.flush_text();
        self.queue.push_back(token);
    }

    fn emit_eof(&mut self) {
        self.flush_text();
        self.queue.push_back(Token::Eof);
        self.finished = true;
    }

    fn emit_comment(&mut self) {
        let comment = std::mem::take(&mut self.comment);
        self.emit(Token::Comment(comment));
    }

    fn emit_doctype(&mut self) {
        let doctype = std::mem::take(&mut self.doctype);
        self.emit(Token::Doctype(doctype));
    }

    fn new_doctype(&mut self) {
        self.doctype = DoctypeToken::default();
    }

    fn new_tag(&mut self, end: bool) {
        self.tag = Tag::default();
        self.end_tag = end;
        self.attribute = None;
    }

    fn push_tag_name(&mut self, c: char) {
        self.tag.name.push(c);
        if self.tag.name.len() > self.limits.max_name_len {
            self.fail(Error::NameCap);
        }
    }

    /// Opens a new attribute, closing the one before: §13.2.5.33's duplicate
    /// rule is applied as the old one is closed, so the first of two
    /// attributes with one name is the one kept.
    fn start_attribute(&mut self, name: &str) {
        self.finish_attribute();
        self.attribute = Some((name.to_owned(), String::new()));
    }

    fn finish_attribute(&mut self) {
        let Some((name, value)) = self.attribute.take() else {
            return;
        };
        if self.tag.attributes.iter().any(|(n, _)| *n == name) {
            self.error();
            return;
        }
        if self.tag.attributes.len() >= self.limits.max_attributes {
            self.fail(Error::AttributeCap);
            return;
        }
        self.tag.attributes.push((name, value));
    }

    fn push_attribute_name(&mut self, c: char) {
        let too_long = match &mut self.attribute {
            Some((name, _)) => {
                name.push(c);
                name.len() > self.limits.max_name_len
            }
            None => false,
        };
        if too_long {
            self.fail(Error::NameCap);
        }
    }

    fn push_attribute_value(&mut self, c: char) {
        if let Some((_, value)) = &mut self.attribute {
            value.push(c);
        }
    }

    fn emit_tag(&mut self) {
        self.finish_attribute();
        if self.finished {
            return;
        }
        let tag = std::mem::take(&mut self.tag);
        if self.end_tag {
            if !tag.attributes.is_empty() || tag.self_closing {
                self.error();
            }
            self.emit(Token::EndTag(tag));
        } else {
            self.last_start_tag.clone_from(&tag.name);
            self.emit(Token::StartTag(tag));
        }
    }

    fn appropriate(&self) -> bool {
        self.end_tag && !self.last_start_tag.is_empty() && self.tag.name == self.last_start_tag
    }

    fn in_attribute(&self) -> bool {
        matches!(
            self.return_state,
            State::AttributeValueDoubleQuoted
                | State::AttributeValueSingleQuoted
                | State::AttributeValueUnquoted
        )
    }

    /// §13.2.5's *flush code points consumed as a character reference*.
    fn flush_reference(&mut self) {
        let buffer = std::mem::take(&mut self.buffer);
        if self.in_attribute() {
            if let Some((_, value)) = &mut self.attribute {
                value.push_str(&buffer);
            }
        } else {
            self.emit_str(&buffer);
        }
    }

    /// A run of ordinary characters in one of the text states, taken at once:
    /// everything up to the next of `stops`, which the state's own arms read.
    fn run(&mut self, stops: &[char]) {
        if self.reconsume {
            return;
        }
        let rest = self.rest();
        let end = rest
            .find(|c: char| stops.contains(&c))
            .unwrap_or(rest.len());
        if end > 0 {
            if let Some(run) = rest.get(..end) {
                self.text.push_str(run);
            }
            self.pos += end;
        }
    }

    /// The `-ending end tag name` states of RCDATA, RAWTEXT and script data,
    /// which differ only in the state they fall back to.
    fn end_tag_name(&mut self, c: Option<char>, back: State) {
        match c {
            Some(c) if is_space(c) && self.appropriate() => {
                self.state = State::BeforeAttributeName;
                return;
            }
            Some('/') if self.appropriate() => {
                self.state = State::SelfClosingStartTag;
                return;
            }
            Some('>') if self.appropriate() => {
                self.state = State::Data;
                self.emit_tag();
                return;
            }
            Some(c) if c.is_ascii_alphabetic() => {
                self.push_tag_name(c.to_ascii_lowercase());
                self.buffer.push(c);
                return;
            }
            _ => {}
        }
        self.emit_str("</");
        let buffer = std::mem::take(&mut self.buffer);
        self.emit_str(&buffer);
        self.reconsume_in(back);
    }

    fn step(&mut self) {
        if self.state == State::NumericCharacterReferenceEnd {
            self.numeric_end();
            return;
        }
        match self.state {
            State::Data => self.run(&['&', '<', '\0']),
            State::Rcdata => self.run(&['&', '<', '\0']),
            State::Rawtext | State::ScriptData => self.run(&['<', '\0']),
            State::Plaintext => self.run(&['\0']),
            _ => {}
        }
        let c = self.next_char();
        match self.state {
            State::Data => match c {
                Some('&') => {
                    self.return_state = State::Data;
                    self.state = State::CharacterReference;
                }
                Some('<') => {
                    // The tree builder sees the text before the tag first.
                    self.flush_text();
                    self.state = State::TagOpen;
                }
                Some('\0') => {
                    self.error();
                    self.emit(Token::Null);
                }
                None => self.emit_eof(),
                Some(c) => self.emit_char(c),
            },
            State::Rcdata => match c {
                Some('&') => {
                    self.return_state = State::Rcdata;
                    self.state = State::CharacterReference;
                }
                Some('<') => self.state = State::RcdataLessThan,
                Some('\0') => {
                    self.error();
                    self.emit_char('\u{FFFD}');
                }
                None => self.emit_eof(),
                Some(c) => self.emit_char(c),
            },
            State::Rawtext => match c {
                Some('<') => self.state = State::RawtextLessThan,
                Some('\0') => {
                    self.error();
                    self.emit_char('\u{FFFD}');
                }
                None => self.emit_eof(),
                Some(c) => self.emit_char(c),
            },
            State::ScriptData => match c {
                Some('<') => self.state = State::ScriptDataLessThan,
                Some('\0') => {
                    self.error();
                    self.emit_char('\u{FFFD}');
                }
                None => self.emit_eof(),
                Some(c) => self.emit_char(c),
            },
            State::Plaintext => match c {
                Some('\0') => {
                    self.error();
                    self.emit_char('\u{FFFD}');
                }
                None => self.emit_eof(),
                Some(c) => self.emit_char(c),
            },
            State::TagOpen => match c {
                Some('!') => self.state = State::MarkupDeclarationOpen,
                Some('/') => self.state = State::EndTagOpen,
                Some(c) if c.is_ascii_alphabetic() => {
                    self.new_tag(false);
                    self.reconsume_in(State::TagName);
                }
                Some('?') => {
                    self.error();
                    self.comment.clear();
                    self.reconsume_in(State::BogusComment);
                }
                None => {
                    self.error();
                    self.emit_char('<');
                    self.emit_eof();
                }
                Some(_) => {
                    self.error();
                    self.emit_char('<');
                    self.reconsume_in(State::Data);
                }
            },
            State::EndTagOpen => match c {
                Some(c) if c.is_ascii_alphabetic() => {
                    self.new_tag(true);
                    self.reconsume_in(State::TagName);
                }
                Some('>') => {
                    self.error();
                    self.state = State::Data;
                }
                None => {
                    self.error();
                    self.emit_str("</");
                    self.emit_eof();
                }
                Some(_) => {
                    self.error();
                    self.comment.clear();
                    self.reconsume_in(State::BogusComment);
                }
            },
            State::TagName => match c {
                Some(c) if is_space(c) => self.state = State::BeforeAttributeName,
                Some('/') => self.state = State::SelfClosingStartTag,
                Some('>') => {
                    self.state = State::Data;
                    self.emit_tag();
                }
                Some('\0') => {
                    self.error();
                    self.push_tag_name('\u{FFFD}');
                }
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some(c) => self.push_tag_name(c.to_ascii_lowercase()),
            },
            State::RcdataLessThan => match c {
                Some('/') => {
                    self.buffer.clear();
                    self.state = State::RcdataEndTagOpen;
                }
                _ => {
                    self.emit_char('<');
                    self.reconsume_in(State::Rcdata);
                }
            },
            State::RcdataEndTagOpen => match c {
                Some(c) if c.is_ascii_alphabetic() => {
                    self.new_tag(true);
                    self.reconsume_in(State::RcdataEndTagName);
                }
                _ => {
                    self.emit_str("</");
                    self.reconsume_in(State::Rcdata);
                }
            },
            State::RcdataEndTagName => self.end_tag_name(c, State::Rcdata),
            State::RawtextLessThan => match c {
                Some('/') => {
                    self.buffer.clear();
                    self.state = State::RawtextEndTagOpen;
                }
                _ => {
                    self.emit_char('<');
                    self.reconsume_in(State::Rawtext);
                }
            },
            State::RawtextEndTagOpen => match c {
                Some(c) if c.is_ascii_alphabetic() => {
                    self.new_tag(true);
                    self.reconsume_in(State::RawtextEndTagName);
                }
                _ => {
                    self.emit_str("</");
                    self.reconsume_in(State::Rawtext);
                }
            },
            State::RawtextEndTagName => self.end_tag_name(c, State::Rawtext),
            State::ScriptDataLessThan => match c {
                Some('/') => {
                    self.buffer.clear();
                    self.state = State::ScriptDataEndTagOpen;
                }
                Some('!') => {
                    self.state = State::ScriptDataEscapeStart;
                    self.emit_str("<!");
                }
                _ => {
                    self.emit_char('<');
                    self.reconsume_in(State::ScriptData);
                }
            },
            State::ScriptDataEndTagOpen => match c {
                Some(c) if c.is_ascii_alphabetic() => {
                    self.new_tag(true);
                    self.reconsume_in(State::ScriptDataEndTagName);
                }
                _ => {
                    self.emit_str("</");
                    self.reconsume_in(State::ScriptData);
                }
            },
            State::ScriptDataEndTagName => self.end_tag_name(c, State::ScriptData),
            State::ScriptDataEscapeStart => match c {
                Some('-') => {
                    self.state = State::ScriptDataEscapeStartDash;
                    self.emit_char('-');
                }
                _ => self.reconsume_in(State::ScriptData),
            },
            State::ScriptDataEscapeStartDash => match c {
                Some('-') => {
                    self.state = State::ScriptDataEscapedDashDash;
                    self.emit_char('-');
                }
                _ => self.reconsume_in(State::ScriptData),
            },
            State::ScriptDataEscaped => match c {
                Some('-') => {
                    self.state = State::ScriptDataEscapedDash;
                    self.emit_char('-');
                }
                Some('<') => self.state = State::ScriptDataEscapedLessThan,
                Some('\0') => {
                    self.error();
                    self.emit_char('\u{FFFD}');
                }
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some(c) => self.emit_char(c),
            },
            State::ScriptDataEscapedDash => match c {
                Some('-') => {
                    self.state = State::ScriptDataEscapedDashDash;
                    self.emit_char('-');
                }
                Some('<') => self.state = State::ScriptDataEscapedLessThan,
                Some('\0') => {
                    self.error();
                    self.state = State::ScriptDataEscaped;
                    self.emit_char('\u{FFFD}');
                }
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some(c) => {
                    self.state = State::ScriptDataEscaped;
                    self.emit_char(c);
                }
            },
            State::ScriptDataEscapedDashDash => match c {
                Some('-') => self.emit_char('-'),
                Some('<') => self.state = State::ScriptDataEscapedLessThan,
                Some('>') => {
                    self.state = State::ScriptData;
                    self.emit_char('>');
                }
                Some('\0') => {
                    self.error();
                    self.state = State::ScriptDataEscaped;
                    self.emit_char('\u{FFFD}');
                }
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some(c) => {
                    self.state = State::ScriptDataEscaped;
                    self.emit_char(c);
                }
            },
            State::ScriptDataEscapedLessThan => match c {
                Some('/') => {
                    self.buffer.clear();
                    self.state = State::ScriptDataEscapedEndTagOpen;
                }
                Some(c) if c.is_ascii_alphabetic() => {
                    self.buffer.clear();
                    self.emit_char('<');
                    self.reconsume_in(State::ScriptDataDoubleEscapeStart);
                }
                _ => {
                    self.emit_char('<');
                    self.reconsume_in(State::ScriptDataEscaped);
                }
            },
            State::ScriptDataEscapedEndTagOpen => match c {
                Some(c) if c.is_ascii_alphabetic() => {
                    self.new_tag(true);
                    self.reconsume_in(State::ScriptDataEscapedEndTagName);
                }
                _ => {
                    self.emit_str("</");
                    self.reconsume_in(State::ScriptDataEscaped);
                }
            },
            State::ScriptDataEscapedEndTagName => self.end_tag_name(c, State::ScriptDataEscaped),
            State::ScriptDataDoubleEscapeStart => match c {
                Some(c) if is_space(c) || c == '/' || c == '>' => {
                    self.state = if self.buffer == "script" {
                        State::ScriptDataDoubleEscaped
                    } else {
                        State::ScriptDataEscaped
                    };
                    self.emit_char(c);
                }
                Some(c) if c.is_ascii_alphabetic() => {
                    self.buffer.push(c.to_ascii_lowercase());
                    self.emit_char(c);
                }
                _ => self.reconsume_in(State::ScriptDataEscaped),
            },
            State::ScriptDataDoubleEscaped => match c {
                Some('-') => {
                    self.state = State::ScriptDataDoubleEscapedDash;
                    self.emit_char('-');
                }
                Some('<') => {
                    self.state = State::ScriptDataDoubleEscapedLessThan;
                    self.emit_char('<');
                }
                Some('\0') => {
                    self.error();
                    self.emit_char('\u{FFFD}');
                }
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some(c) => self.emit_char(c),
            },
            State::ScriptDataDoubleEscapedDash => match c {
                Some('-') => {
                    self.state = State::ScriptDataDoubleEscapedDashDash;
                    self.emit_char('-');
                }
                Some('<') => {
                    self.state = State::ScriptDataDoubleEscapedLessThan;
                    self.emit_char('<');
                }
                Some('\0') => {
                    self.error();
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emit_char('\u{FFFD}');
                }
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some(c) => {
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emit_char(c);
                }
            },
            State::ScriptDataDoubleEscapedDashDash => match c {
                Some('-') => self.emit_char('-'),
                Some('<') => {
                    self.state = State::ScriptDataDoubleEscapedLessThan;
                    self.emit_char('<');
                }
                Some('>') => {
                    self.state = State::ScriptData;
                    self.emit_char('>');
                }
                Some('\0') => {
                    self.error();
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emit_char('\u{FFFD}');
                }
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some(c) => {
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emit_char(c);
                }
            },
            State::ScriptDataDoubleEscapedLessThan => match c {
                Some('/') => {
                    self.buffer.clear();
                    self.state = State::ScriptDataDoubleEscapeEnd;
                    self.emit_char('/');
                }
                _ => self.reconsume_in(State::ScriptDataDoubleEscaped),
            },
            State::ScriptDataDoubleEscapeEnd => match c {
                Some(c) if is_space(c) || c == '/' || c == '>' => {
                    self.state = if self.buffer == "script" {
                        State::ScriptDataEscaped
                    } else {
                        State::ScriptDataDoubleEscaped
                    };
                    self.emit_char(c);
                }
                Some(c) if c.is_ascii_alphabetic() => {
                    self.buffer.push(c.to_ascii_lowercase());
                    self.emit_char(c);
                }
                _ => self.reconsume_in(State::ScriptDataDoubleEscaped),
            },
            State::BeforeAttributeName => match c {
                Some(c) if is_space(c) => {}
                Some('/' | '>') | None => self.reconsume_in(State::AfterAttributeName),
                Some('=') => {
                    self.error();
                    self.start_attribute("=");
                    self.state = State::AttributeName;
                }
                Some(_) => {
                    self.start_attribute("");
                    self.reconsume_in(State::AttributeName);
                }
            },
            State::AttributeName => match c {
                Some(c) if is_space(c) => self.reconsume_in(State::AfterAttributeName),
                Some('/' | '>') | None => self.reconsume_in(State::AfterAttributeName),
                Some('=') => self.state = State::BeforeAttributeValue,
                Some('\0') => {
                    self.error();
                    self.push_attribute_name('\u{FFFD}');
                }
                Some(c @ ('"' | '\'' | '<')) => {
                    self.error();
                    self.push_attribute_name(c);
                }
                Some(c) => self.push_attribute_name(c.to_ascii_lowercase()),
            },
            State::AfterAttributeName => match c {
                Some(c) if is_space(c) => {}
                Some('/') => self.state = State::SelfClosingStartTag,
                Some('=') => self.state = State::BeforeAttributeValue,
                Some('>') => {
                    self.state = State::Data;
                    self.emit_tag();
                }
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some(_) => {
                    self.start_attribute("");
                    self.reconsume_in(State::AttributeName);
                }
            },
            State::BeforeAttributeValue => match c {
                Some(c) if is_space(c) => {}
                Some('"') => self.state = State::AttributeValueDoubleQuoted,
                Some('\'') => self.state = State::AttributeValueSingleQuoted,
                Some('>') => {
                    self.error();
                    self.state = State::Data;
                    self.emit_tag();
                }
                _ => self.reconsume_in(State::AttributeValueUnquoted),
            },
            State::AttributeValueDoubleQuoted | State::AttributeValueSingleQuoted => {
                let quote = if self.state == State::AttributeValueDoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                match c {
                    Some(q) if q == quote => self.state = State::AfterAttributeValueQuoted,
                    Some('&') => {
                        self.return_state = self.state;
                        self.state = State::CharacterReference;
                    }
                    Some('\0') => {
                        self.error();
                        self.push_attribute_value('\u{FFFD}');
                    }
                    None => {
                        self.error();
                        self.emit_eof();
                    }
                    Some(c) => self.push_attribute_value(c),
                }
            }
            State::AttributeValueUnquoted => match c {
                Some(c) if is_space(c) => self.state = State::BeforeAttributeName,
                Some('&') => {
                    self.return_state = State::AttributeValueUnquoted;
                    self.state = State::CharacterReference;
                }
                Some('>') => {
                    self.state = State::Data;
                    self.emit_tag();
                }
                Some('\0') => {
                    self.error();
                    self.push_attribute_value('\u{FFFD}');
                }
                Some(c @ ('"' | '\'' | '<' | '=' | '`')) => {
                    self.error();
                    self.push_attribute_value(c);
                }
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some(c) => self.push_attribute_value(c),
            },
            State::AfterAttributeValueQuoted => match c {
                Some(c) if is_space(c) => self.state = State::BeforeAttributeName,
                Some('/') => self.state = State::SelfClosingStartTag,
                Some('>') => {
                    self.state = State::Data;
                    self.emit_tag();
                }
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some(_) => {
                    self.error();
                    self.reconsume_in(State::BeforeAttributeName);
                }
            },
            State::SelfClosingStartTag => match c {
                Some('>') => {
                    self.tag.self_closing = true;
                    self.state = State::Data;
                    self.emit_tag();
                }
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some(_) => {
                    self.error();
                    self.reconsume_in(State::BeforeAttributeName);
                }
            },
            State::BogusComment => match c {
                Some('>') => {
                    self.state = State::Data;
                    self.emit_comment();
                }
                None => {
                    self.emit_comment();
                    self.emit_eof();
                }
                Some('\0') => {
                    self.error();
                    self.comment.push('\u{FFFD}');
                }
                Some(c) => self.comment.push(c),
            },
            State::MarkupDeclarationOpen => {
                // Nothing was consumed for this state: what `next_char` took is
                // given back, and the lookahead is over the input itself.
                self.reconsume = false;
                if let Some(c) = c {
                    self.pos -= c.len_utf8();
                }
                let rest = self.rest();
                if rest.starts_with("--") {
                    self.pos += 2;
                    self.comment.clear();
                    self.state = State::CommentStart;
                } else if rest
                    .get(..7)
                    .is_some_and(|w| w.eq_ignore_ascii_case("doctype"))
                {
                    self.pos += 7;
                    self.state = State::Doctype;
                } else if rest.starts_with("[CDATA[") {
                    self.pos += 7;
                    if self.allow_cdata {
                        self.state = State::CdataSection;
                    } else {
                        self.error();
                        self.comment = "[CDATA[".to_owned();
                        self.state = State::BogusComment;
                    }
                } else {
                    self.error();
                    self.comment.clear();
                    self.state = State::BogusComment;
                }
            }
            State::CommentStart => match c {
                Some('-') => self.state = State::CommentStartDash,
                Some('>') => {
                    self.error();
                    self.state = State::Data;
                    self.emit_comment();
                }
                _ => self.reconsume_in(State::Comment),
            },
            State::CommentStartDash => match c {
                Some('-') => self.state = State::CommentEnd,
                Some('>') => {
                    self.error();
                    self.state = State::Data;
                    self.emit_comment();
                }
                None => {
                    self.error();
                    self.emit_comment();
                    self.emit_eof();
                }
                Some(_) => {
                    self.comment.push('-');
                    self.reconsume_in(State::Comment);
                }
            },
            State::Comment => match c {
                Some('<') => {
                    self.comment.push('<');
                    self.state = State::CommentLessThan;
                }
                Some('-') => self.state = State::CommentEndDash,
                Some('\0') => {
                    self.error();
                    self.comment.push('\u{FFFD}');
                }
                None => {
                    self.error();
                    self.emit_comment();
                    self.emit_eof();
                }
                Some(c) => self.comment.push(c),
            },
            State::CommentLessThan => match c {
                Some('!') => {
                    self.comment.push('!');
                    self.state = State::CommentLessThanBang;
                }
                Some('<') => self.comment.push('<'),
                _ => self.reconsume_in(State::Comment),
            },
            State::CommentLessThanBang => match c {
                Some('-') => self.state = State::CommentLessThanBangDash,
                _ => self.reconsume_in(State::Comment),
            },
            State::CommentLessThanBangDash => match c {
                Some('-') => self.state = State::CommentLessThanBangDashDash,
                _ => self.reconsume_in(State::CommentEndDash),
            },
            State::CommentLessThanBangDashDash => match c {
                Some('>') | None => self.reconsume_in(State::CommentEnd),
                Some(_) => {
                    self.error();
                    self.reconsume_in(State::CommentEnd);
                }
            },
            State::CommentEndDash => match c {
                Some('-') => self.state = State::CommentEnd,
                None => {
                    self.error();
                    self.emit_comment();
                    self.emit_eof();
                }
                Some(_) => {
                    self.comment.push('-');
                    self.reconsume_in(State::Comment);
                }
            },
            State::CommentEnd => match c {
                Some('>') => {
                    self.state = State::Data;
                    self.emit_comment();
                }
                Some('!') => self.state = State::CommentEndBang,
                Some('-') => self.comment.push('-'),
                None => {
                    self.error();
                    self.emit_comment();
                    self.emit_eof();
                }
                Some(_) => {
                    self.comment.push_str("--");
                    self.reconsume_in(State::Comment);
                }
            },
            State::CommentEndBang => match c {
                Some('-') => {
                    self.comment.push_str("--!");
                    self.state = State::CommentEndDash;
                }
                Some('>') => {
                    self.error();
                    self.state = State::Data;
                    self.emit_comment();
                }
                None => {
                    self.error();
                    self.emit_comment();
                    self.emit_eof();
                }
                Some(_) => {
                    self.comment.push_str("--!");
                    self.reconsume_in(State::Comment);
                }
            },
            State::Doctype => match c {
                Some(c) if is_space(c) => self.state = State::BeforeDoctypeName,
                Some('>') => self.reconsume_in(State::BeforeDoctypeName),
                None => {
                    self.error();
                    self.new_doctype();
                    self.doctype.force_quirks = true;
                    self.emit_doctype();
                    self.emit_eof();
                }
                Some(_) => {
                    self.error();
                    self.reconsume_in(State::BeforeDoctypeName);
                }
            },
            State::BeforeDoctypeName => match c {
                Some(c) if is_space(c) => {}
                Some('\0') => {
                    self.error();
                    self.new_doctype();
                    self.doctype.name = Some('\u{FFFD}'.to_string());
                    self.state = State::DoctypeName;
                }
                Some('>') => {
                    self.error();
                    self.new_doctype();
                    self.doctype.force_quirks = true;
                    self.state = State::Data;
                    self.emit_doctype();
                }
                None => {
                    self.error();
                    self.new_doctype();
                    self.doctype.force_quirks = true;
                    self.emit_doctype();
                    self.emit_eof();
                }
                Some(c) => {
                    self.new_doctype();
                    self.doctype.name = Some(c.to_ascii_lowercase().to_string());
                    self.state = State::DoctypeName;
                }
            },
            State::DoctypeName => match c {
                Some(c) if is_space(c) => self.state = State::AfterDoctypeName,
                Some('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                Some('\0') => {
                    self.error();
                    self.doctype_name_push('\u{FFFD}');
                }
                None => {
                    self.error();
                    self.doctype.force_quirks = true;
                    self.emit_doctype();
                    self.emit_eof();
                }
                Some(c) => self.doctype_name_push(c.to_ascii_lowercase()),
            },
            State::AfterDoctypeName => match c {
                Some(c) if is_space(c) => {}
                Some('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                None => {
                    self.error();
                    self.doctype.force_quirks = true;
                    self.emit_doctype();
                    self.emit_eof();
                }
                Some(c) => {
                    let start = self.pos - c.len_utf8();
                    let word = self.input.get(start..start + 6);
                    if word.is_some_and(|w| w.eq_ignore_ascii_case("public")) {
                        self.pos = start + 6;
                        self.state = State::AfterDoctypePublicKeyword;
                    } else if word.is_some_and(|w| w.eq_ignore_ascii_case("system")) {
                        self.pos = start + 6;
                        self.state = State::AfterDoctypeSystemKeyword;
                    } else {
                        self.error();
                        self.doctype.force_quirks = true;
                        self.reconsume_in(State::BogusDoctype);
                    }
                }
            },
            State::AfterDoctypePublicKeyword | State::AfterDoctypeSystemKeyword => {
                let public = self.state == State::AfterDoctypePublicKeyword;
                match c {
                    Some(c) if is_space(c) => {
                        self.state = if public {
                            State::BeforeDoctypePublicIdentifier
                        } else {
                            State::BeforeDoctypeSystemIdentifier
                        };
                    }
                    Some(q @ ('"' | '\'')) => {
                        self.error();
                        self.open_identifier(public, q);
                    }
                    Some('>') => {
                        self.error();
                        self.doctype.force_quirks = true;
                        self.state = State::Data;
                        self.emit_doctype();
                    }
                    None => {
                        self.error();
                        self.doctype.force_quirks = true;
                        self.emit_doctype();
                        self.emit_eof();
                    }
                    Some(_) => {
                        self.error();
                        self.doctype.force_quirks = true;
                        self.reconsume_in(State::BogusDoctype);
                    }
                }
            }
            State::BeforeDoctypePublicIdentifier | State::BeforeDoctypeSystemIdentifier => {
                let public = self.state == State::BeforeDoctypePublicIdentifier;
                match c {
                    Some(c) if is_space(c) => {}
                    Some(q @ ('"' | '\'')) => self.open_identifier(public, q),
                    Some('>') => {
                        self.error();
                        self.doctype.force_quirks = true;
                        self.state = State::Data;
                        self.emit_doctype();
                    }
                    None => {
                        self.error();
                        self.doctype.force_quirks = true;
                        self.emit_doctype();
                        self.emit_eof();
                    }
                    Some(_) => {
                        self.error();
                        self.doctype.force_quirks = true;
                        self.reconsume_in(State::BogusDoctype);
                    }
                }
            }
            State::DoctypePublicIdentifierDoubleQuoted
            | State::DoctypePublicIdentifierSingleQuoted
            | State::DoctypeSystemIdentifierDoubleQuoted
            | State::DoctypeSystemIdentifierSingleQuoted => {
                let public = matches!(
                    self.state,
                    State::DoctypePublicIdentifierDoubleQuoted
                        | State::DoctypePublicIdentifierSingleQuoted
                );
                let quote = if matches!(
                    self.state,
                    State::DoctypePublicIdentifierDoubleQuoted
                        | State::DoctypeSystemIdentifierDoubleQuoted
                ) {
                    '"'
                } else {
                    '\''
                };
                match c {
                    Some(q) if q == quote => {
                        self.state = if public {
                            State::AfterDoctypePublicIdentifier
                        } else {
                            State::AfterDoctypeSystemIdentifier
                        };
                    }
                    Some('\0') => {
                        self.error();
                        self.identifier_push(public, '\u{FFFD}');
                    }
                    Some('>') => {
                        self.error();
                        self.doctype.force_quirks = true;
                        self.state = State::Data;
                        self.emit_doctype();
                    }
                    None => {
                        self.error();
                        self.doctype.force_quirks = true;
                        self.emit_doctype();
                        self.emit_eof();
                    }
                    Some(c) => self.identifier_push(public, c),
                }
            }
            State::AfterDoctypePublicIdentifier => match c {
                Some(c) if is_space(c) => {
                    self.state = State::BetweenDoctypePublicAndSystemIdentifiers;
                }
                Some('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                Some(q @ ('"' | '\'')) => {
                    self.error();
                    self.open_identifier(false, q);
                }
                None => {
                    self.error();
                    self.doctype.force_quirks = true;
                    self.emit_doctype();
                    self.emit_eof();
                }
                Some(_) => {
                    self.error();
                    self.doctype.force_quirks = true;
                    self.reconsume_in(State::BogusDoctype);
                }
            },
            State::BetweenDoctypePublicAndSystemIdentifiers => match c {
                Some(c) if is_space(c) => {}
                Some('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                Some(q @ ('"' | '\'')) => self.open_identifier(false, q),
                None => {
                    self.error();
                    self.doctype.force_quirks = true;
                    self.emit_doctype();
                    self.emit_eof();
                }
                Some(_) => {
                    self.error();
                    self.doctype.force_quirks = true;
                    self.reconsume_in(State::BogusDoctype);
                }
            },
            State::AfterDoctypeSystemIdentifier => match c {
                Some(c) if is_space(c) => {}
                Some('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                None => {
                    self.error();
                    self.doctype.force_quirks = true;
                    self.emit_doctype();
                    self.emit_eof();
                }
                Some(_) => {
                    // §13.2.5.66: this one does not set the force-quirks flag.
                    self.error();
                    self.reconsume_in(State::BogusDoctype);
                }
            },
            State::BogusDoctype => match c {
                Some('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                Some('\0') => self.error(),
                None => {
                    self.emit_doctype();
                    self.emit_eof();
                }
                Some(_) => {}
            },
            State::CdataSection => match c {
                Some(']') => self.state = State::CdataSectionBracket,
                None => {
                    self.error();
                    self.emit_eof();
                }
                Some('\0') => self.emit(Token::Null),
                Some(c) => self.emit_char(c),
            },
            State::CdataSectionBracket => match c {
                Some(']') => self.state = State::CdataSectionEnd,
                _ => {
                    self.emit_char(']');
                    self.reconsume_in(State::CdataSection);
                }
            },
            State::CdataSectionEnd => match c {
                Some(']') => self.emit_char(']'),
                Some('>') => self.state = State::Data,
                _ => {
                    self.emit_str("]]");
                    self.reconsume_in(State::CdataSection);
                }
            },
            State::CharacterReference => {
                self.buffer.clear();
                self.buffer.push('&');
                match c {
                    Some(c) if c.is_ascii_alphanumeric() => {
                        self.reconsume_in(State::NamedCharacterReference);
                    }
                    Some('#') => {
                        self.buffer.push('#');
                        self.state = State::NumericCharacterReference;
                    }
                    _ => {
                        self.flush_reference();
                        let back = self.return_state;
                        self.reconsume_in(back);
                    }
                }
            }
            State::NamedCharacterReference => self.named_reference(c),
            State::AmbiguousAmpersand => match c {
                Some(c) if c.is_ascii_alphanumeric() => {
                    if self.in_attribute() {
                        self.push_attribute_value(c);
                    } else {
                        self.emit_char(c);
                    }
                }
                Some(';') => {
                    self.error();
                    let back = self.return_state;
                    self.reconsume_in(back);
                }
                _ => {
                    let back = self.return_state;
                    self.reconsume_in(back);
                }
            },
            State::NumericCharacterReference => {
                self.code = 0;
                match c {
                    Some(x @ ('x' | 'X')) => {
                        self.buffer.push(x);
                        self.state = State::HexadecimalCharacterReferenceStart;
                    }
                    Some(c) if c.is_ascii_digit() => {
                        self.reconsume_in(State::DecimalCharacterReference);
                    }
                    _ => {
                        self.error();
                        self.flush_reference();
                        let back = self.return_state;
                        self.reconsume_in(back);
                    }
                }
            }
            State::HexadecimalCharacterReferenceStart => match c {
                Some(c) if c.is_ascii_hexdigit() => {
                    self.reconsume_in(State::HexadecimalCharacterReference);
                }
                _ => {
                    self.error();
                    self.flush_reference();
                    let back = self.return_state;
                    self.reconsume_in(back);
                }
            },
            State::HexadecimalCharacterReference | State::DecimalCharacterReference => {
                let radix = if self.state == State::HexadecimalCharacterReference {
                    16
                } else {
                    10
                };
                match c {
                    Some(c) if c.is_digit(radix) => {
                        let digit = c.to_digit(radix).unwrap_or(0);
                        // Saturated just past the range, which is all the end
                        // state needs to know: a code this large is U+FFFD.
                        self.code = self
                            .code
                            .saturating_mul(radix)
                            .saturating_add(digit)
                            .min(0x11_0000);
                    }
                    Some(';') => self.state = State::NumericCharacterReferenceEnd,
                    _ => {
                        self.error();
                        self.reconsume_in(State::NumericCharacterReferenceEnd);
                    }
                }
            }
            State::NumericCharacterReferenceEnd => {}
        }
    }

    fn doctype_name_push(&mut self, c: char) {
        let name = self.doctype.name.get_or_insert_with(String::new);
        name.push(c);
        if name.len() > self.limits.max_name_len {
            self.fail(Error::NameCap);
        }
    }

    fn open_identifier(&mut self, public: bool, quote: char) {
        let double = quote == '"';
        if public {
            self.doctype.public_id = Some(String::new());
            self.state = if double {
                State::DoctypePublicIdentifierDoubleQuoted
            } else {
                State::DoctypePublicIdentifierSingleQuoted
            };
        } else {
            self.doctype.system_id = Some(String::new());
            self.state = if double {
                State::DoctypeSystemIdentifierDoubleQuoted
            } else {
                State::DoctypeSystemIdentifierSingleQuoted
            };
        }
    }

    fn identifier_push(&mut self, public: bool, c: char) {
        let slot = if public {
            &mut self.doctype.public_id
        } else {
            &mut self.doctype.system_id
        };
        slot.get_or_insert_with(String::new).push(c);
    }

    /// §13.2.5.73, the named character reference state, at once: the longest
    /// prefix of what follows the `&` that is a name in the table.
    fn named_reference(&mut self, c: Option<char>) {
        // `c` is the alphanumeric the previous state reconsumed; the name
        // starts with it.
        let Some(first) = c else {
            let back = self.return_state;
            self.flush_reference();
            self.reconsume_in(back);
            return;
        };
        let start = self.pos - first.len_utf8();
        let window: &str = {
            let rest = self.input.get(start..).unwrap_or("");
            let end = rest
                .bytes()
                .take(LONGEST_NAME)
                .position(|b| !(b.is_ascii_alphanumeric() || b == b';'))
                .unwrap_or(rest.len().min(LONGEST_NAME));
            rest.get(..end).unwrap_or("")
        };
        let found = (1..=window.len()).rev().find_map(|len| {
            let name = window.get(..len)?;
            entities::lookup(name).map(|value| (name, value))
        });
        match found {
            Some((name, (first_char, second_char))) => {
                self.pos = start + name.len();
                self.buffer.push_str(name);
                let next = self.rest().chars().next();
                if self.in_attribute()
                    && !name.ends_with(';')
                    && next.is_some_and(|n| n == '=' || n.is_ascii_alphanumeric())
                {
                    self.flush_reference();
                    self.state = self.return_state;
                    return;
                }
                if !name.ends_with(';') {
                    self.error();
                }
                self.buffer.clear();
                self.buffer.push(first_char);
                if let Some(second) = second_char {
                    self.buffer.push(second);
                }
                self.flush_reference();
                self.state = self.return_state;
            }
            None => {
                // Nothing matched, so nothing was consumed: the ambiguous
                // ampersand state reads the same characters again.
                self.pos = start;
                self.flush_reference();
                self.state = State::AmbiguousAmpersand;
            }
        }
    }

    /// §13.2.5.80, which consumes nothing.
    fn numeric_end(&mut self) {
        let mut code = self.code;
        // A null, a code past the range and a surrogate: three parse errors
        // the standard names apart, and one answer.
        if code == 0 || code > 0x10_FFFF || (0xD800..=0xDFFF).contains(&code) {
            self.error();
            code = 0xFFFD;
        } else if (0xFDD0..=0xFDEF).contains(&code) || (code & 0xFFFE) == 0xFFFE {
            self.error();
        } else if code == 0x0D
            || ((code < 0x20 || (0x7F..=0x9F).contains(&code))
                && !matches!(code, 0x09 | 0x0A | 0x0C | 0x20))
        {
            self.error();
            if let Some(&(_, replacement)) = C1_REPLACEMENTS.iter().find(|(from, _)| *from == code)
            {
                code = replacement;
            }
        }
        self.buffer.clear();
        self.buffer.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
        self.flush_reference();
        self.state = self.return_state;
    }
}

/// §13.2.5.80's table: a numeric reference to a C1 control is the character
/// windows-1252 puts at that byte.
const C1_REPLACEMENTS: [(u32, u32); 27] = [
    (0x80, 0x20AC),
    (0x82, 0x201A),
    (0x83, 0x0192),
    (0x84, 0x201E),
    (0x85, 0x2026),
    (0x86, 0x2020),
    (0x87, 0x2021),
    (0x88, 0x02C6),
    (0x89, 0x2030),
    (0x8A, 0x0160),
    (0x8B, 0x2039),
    (0x8C, 0x0152),
    (0x8E, 0x017D),
    (0x91, 0x2018),
    (0x92, 0x2019),
    (0x93, 0x201C),
    (0x94, 0x201D),
    (0x95, 0x2022),
    (0x96, 0x2013),
    (0x97, 0x2014),
    (0x98, 0x02DC),
    (0x99, 0x2122),
    (0x9A, 0x0161),
    (0x9B, 0x203A),
    (0x9C, 0x0153),
    (0x9E, 0x017E),
    (0x9F, 0x0178),
];
