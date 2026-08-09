//! The object parser (7.3.6 – 7.3.10).
//!
//! Two entry points, both taking a buffer and an offset because the document
//! layer above always knows where it wants to read and never wants a cursor
//! kept for it. Neither can fail: an object always comes back, with warnings
//! naming every repair, because "no object here" is a decision only the xref
//! and repair layers have the context to make.

use crate::lexer::{Keyword, Lexer, Token, TokenKind};
use crate::limits;
use crate::name::{Name, NameTable};
use crate::object::{Dict, ObjRef, Object, StreamObj};
use crate::warn::{WarningKind, WarningSink};

/// A direct object and where reading it stopped.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedObject {
    /// The object. [`Object::Null`] when nothing sensible was there.
    pub object: Object,
    /// Offset just past the last byte consumed.
    pub end_offset: u64,
}

/// An indirect object (7.3.10), its reference, and where reading it stopped.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedIndirect {
    /// The reference spelled by the `N G obj` header.
    pub reference: ObjRef,
    /// The object body. A stream carries only its claimed data range.
    pub object: Object,
    /// Offset just past the last byte consumed.
    ///
    /// For a stream this is the end of the *claimed* data — `data_start` plus
    /// the declared `/Length`, clamped to the buffer — and equals `data_start`
    /// when the length is not yet known. The parser deliberately does not lex
    /// past stream data: validating the extent against `endstream` needs the
    /// store (an indirect `/Length` must be resolved first) and is the
    /// document layer's job.
    pub end_offset: u64,
}

/// Reads one direct object at `offset`.
///
/// Indirect references are recognized but not resolved: `1 0 R` yields
/// [`Object::Ref`]. An indirect object *header* is not: pointing this at
/// `1 0 obj` yields `Int(1)` and stops, which is what
/// [`parse_indirect_at`] is for.
pub fn parse_object_at(
    buf: &[u8],
    offset: u64,
    names: &NameTable,
    sink: &mut WarningSink,
) -> ParsedObject {
    let mut parser = Parser::new(buf, offset, names);
    let object = parser.parse_object(sink);
    ParsedObject {
        object,
        end_offset: parser.offset(),
    }
}

/// Reads the indirect object whose `N G obj` header is at `offset`.
///
/// Returns `None` when there is no such header, without warning: whether a
/// missing header is a lying xref entry, a probe, or a scan miss is the
/// caller's judgement, and only the caller knows which object it expected.
/// Warnings from inside the object are attributed to its reference.
pub fn parse_indirect_at(
    buf: &[u8],
    offset: u64,
    names: &NameTable,
    sink: &mut WarningSink,
) -> Option<ParsedIndirect> {
    let mut parser = Parser::new(buf, offset, names);

    // The header is lexed into a scratch sink so that probing an offset that
    // turns out not to be a header leaves no trace in the document's warnings.
    let mut header_sink = WarningSink::new();
    let reference = parser.indirect_header(&mut header_sink)?;
    sink.extend(header_sink.take());

    let previous = sink.set_context(Some(reference));
    let object = parser.parse_object(sink);
    let token = parser.next(sink);
    let parsed = match token.kind {
        TokenKind::Keyword(Keyword::Stream) => match object {
            Object::Dict(dict) => {
                let data_start = stream_data_start(buf, token.end, sink);
                let len_hint = stream_len_hint(&dict, token.start, sink);
                let end_offset = match len_hint {
                    Some(len) => data_start.saturating_add(len).min(buf.len() as u64),
                    None => data_start,
                };
                ParsedIndirect {
                    reference,
                    object: Object::Stream(StreamObj {
                        dict,
                        data_start,
                        len_hint,
                    }),
                    end_offset,
                }
            }
            other => {
                sink.warn(token.start, WarningKind::StreamWithoutDict);
                ParsedIndirect {
                    reference,
                    object: other,
                    end_offset: token.end,
                }
            }
        },
        TokenKind::Keyword(Keyword::Endobj) => ParsedIndirect {
            reference,
            object,
            end_offset: token.end,
        },
        _ => {
            parser.push_back(token);
            let end_offset = parser.offset();
            sink.warn(end_offset, WarningKind::MissingEndobj);
            ParsedIndirect {
                reference,
                object,
                end_offset,
            }
        }
    };
    sink.set_context(previous);
    Some(parsed)
}

/// 7.3.8.1: `stream` shall be followed by CRLF or LF. A lone CR, or blanks
/// before the marker, are tolerated.
fn stream_data_start(buf: &[u8], after_keyword: u64, sink: &mut WarningSink) -> u64 {
    let len = buf.len() as u64;
    let start = after_keyword.min(len);
    let at = |i: u64| -> Option<u8> { usize::try_from(i).ok().and_then(|i| buf.get(i).copied()) };

    match at(start) {
        Some(b'\n') => return (start + 1).min(len),
        Some(b'\r') => {
            if at(start + 1) == Some(b'\n') {
                return (start + 2).min(len);
            }
            sink.warn(start, WarningKind::StreamKeywordBadEol);
            return (start + 1).min(len);
        }
        _ => {}
    }
    sink.warn(start, WarningKind::StreamKeywordBadEol);

    // Only blanks are skipped. Stream data routinely begins with 0x0a, so
    // hunting for an end-of-line through arbitrary bytes would silently drop
    // the start of the data.
    let mut i = start;
    let mut skipped = 0usize;
    while skipped < limits::MAX_STREAM_EOL_SKIP && matches!(at(i), Some(0x20 | 0x09 | 0x00 | 0x0c))
    {
        i += 1;
        skipped += 1;
    }
    match at(i) {
        Some(b'\n') => (i + 1).min(len),
        Some(b'\r') => {
            if at(i + 1) == Some(b'\n') {
                (i + 2).min(len)
            } else {
                (i + 1).min(len)
            }
        }
        // No marker at all: the data starts where the keyword ended.
        _ => start,
    }
}

fn stream_len_hint(dict: &Dict, offset: u64, sink: &mut WarningSink) -> Option<u64> {
    match dict.get(Name::LENGTH) {
        Some(Object::Int(n)) if *n >= 0 => u64::try_from(*n).ok(),
        // 7.3.8.2 allows an indirect /Length and real files use it constantly;
        // resolving it needs the store, so it is not a defect here.
        Some(Object::Ref(_)) => None,
        _ => {
            sink.warn(offset, WarningKind::StreamLengthMissing);
            None
        }
    }
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    names: &'a NameTable,
    pushback: Vec<Token>,
    depth: u32,
}

impl<'a> Parser<'a> {
    fn new(buf: &'a [u8], offset: u64, names: &'a NameTable) -> Parser<'a> {
        Parser {
            lexer: Lexer::at(buf, offset),
            names,
            pushback: Vec::new(),
            depth: 0,
        }
    }

    fn next(&mut self, sink: &mut WarningSink) -> Token {
        match self.pushback.pop() {
            Some(token) => token,
            None => self.lexer.next_token(sink),
        }
    }

    fn push_back(&mut self, token: Token) {
        self.pushback.push(token);
    }

    fn offset(&self) -> u64 {
        match self.pushback.last() {
            Some(token) => token.start,
            None => self.lexer.offset(),
        }
    }

    fn indirect_header(&mut self, sink: &mut WarningSink) -> Option<ObjRef> {
        let TokenKind::Int(num) = self.next(sink).kind else {
            return None;
        };
        let TokenKind::Int(gen) = self.next(sink).kind else {
            return None;
        };
        if self.next(sink).kind != TokenKind::Keyword(Keyword::Obj) {
            return None;
        }
        Some(ObjRef::new(
            u32::try_from(num).ok()?,
            u16::try_from(gen).ok()?,
        ))
    }

    fn parse_object(&mut self, sink: &mut WarningSink) -> Object {
        let token = self.next(sink);
        self.object_from(token, sink)
    }

    fn object_from(&mut self, token: Token, sink: &mut WarningSink) -> Object {
        match token.kind {
            TokenKind::Int(value) => self.int_or_ref(value, sink),
            TokenKind::Real(value) => Object::Real(value),
            TokenKind::String(value) => Object::String(value),
            TokenKind::Name(bytes) => Object::Name(self.names.intern(&bytes)),
            TokenKind::ArrayOpen => self.parse_array(token.start, sink),
            TokenKind::DictOpen => self.parse_dict(token.start, sink),
            TokenKind::Keyword(Keyword::True) => Object::Bool(true),
            TokenKind::Keyword(Keyword::False) => Object::Bool(false),
            TokenKind::Keyword(Keyword::Null) => Object::Null,
            TokenKind::Keyword(_) => {
                sink.warn(token.start, WarningKind::UnexpectedKeyword);
                Object::Null
            }
            _ => {
                sink.warn(token.start, WarningKind::UnexpectedToken);
                Object::Null
            }
        }
    }

    /// 7.3.10: an integer is a reference only in the exact form `num gen R`,
    /// which needs two tokens of lookahead and no more.
    fn int_or_ref(&mut self, value: i64, sink: &mut WarningSink) -> Object {
        let second = self.next(sink);
        if let TokenKind::Int(generation) = second.kind {
            let third = self.next(sink);
            if third.kind == TokenKind::Keyword(Keyword::R) {
                if let (Ok(num), Ok(gen)) = (u32::try_from(value), u16::try_from(generation)) {
                    return Object::Ref(ObjRef::new(num, gen));
                }
            }
            self.push_back(third);
        }
        self.push_back(second);
        Object::Int(value)
    }

    fn parse_array(&mut self, start: u64, sink: &mut WarningSink) -> Object {
        if self.depth >= limits::MAX_NEST_DEPTH {
            sink.warn(start, WarningKind::DepthCapHit);
            self.skip_container(sink);
            return Object::Null;
        }
        self.depth += 1;
        let mut items: Vec<Object> = Vec::new();
        let mut capped = false;
        loop {
            let token = self.next(sink);
            match token.kind {
                TokenKind::ArrayClose => break,
                TokenKind::Eof => {
                    sink.warn(start, WarningKind::UnterminatedArray);
                    break;
                }
                // A close that belongs to an enclosing container ends this one
                // without being eaten, so the enclosing one still sees it.
                TokenKind::DictClose | TokenKind::ProcClose => {
                    sink.warn(token.start, WarningKind::MismatchedClose);
                    sink.warn(start, WarningKind::UnterminatedArray);
                    self.push_back(token);
                    break;
                }
                TokenKind::Keyword(k) if k.is_structural() => {
                    sink.warn(start, WarningKind::UnterminatedArray);
                    self.push_back(token);
                    break;
                }
                _ => {
                    let value = self.object_from(token, sink);
                    if items.len() < limits::MAX_ARRAY_LEN {
                        items.push(value);
                    } else if !capped {
                        capped = true;
                        sink.warn(start, WarningKind::ArrayCapHit);
                    }
                }
            }
        }
        self.depth -= 1;
        Object::Array(items)
    }

    fn parse_dict(&mut self, start: u64, sink: &mut WarningSink) -> Object {
        if self.depth >= limits::MAX_NEST_DEPTH {
            sink.warn(start, WarningKind::DepthCapHit);
            self.skip_container(sink);
            return Object::Null;
        }
        self.depth += 1;
        let mut dict = Dict::new();
        let mut capped = false;
        loop {
            let token = self.next(sink);
            match token.kind {
                TokenKind::DictClose => break,
                TokenKind::Eof => {
                    sink.warn(start, WarningKind::UnterminatedDict);
                    break;
                }
                TokenKind::Keyword(k) if k.is_structural() => {
                    sink.warn(start, WarningKind::UnterminatedDict);
                    self.push_back(token);
                    break;
                }
                // A stray close from a container that was never opened: drop
                // it and keep reading keys, which loses less than bailing out.
                TokenKind::ArrayClose | TokenKind::ProcClose => {
                    sink.warn(token.start, WarningKind::MismatchedClose);
                }
                TokenKind::Name(bytes) => {
                    let key = self.names.intern(&bytes);
                    let value_token = self.next(sink);
                    let value = match value_token.kind {
                        // 7.3.7: every key needs a value. A key that runs into
                        // the end of the dictionary keeps its place as null.
                        TokenKind::DictClose | TokenKind::Eof => {
                            sink.warn(value_token.start, WarningKind::MissingDictValue);
                            self.push_back(value_token);
                            Object::Null
                        }
                        TokenKind::Keyword(k) if k.is_structural() => {
                            sink.warn(value_token.start, WarningKind::MissingDictValue);
                            self.push_back(value_token);
                            Object::Null
                        }
                        _ => self.object_from(value_token, sink),
                    };
                    if dict.len() < limits::MAX_DICT_ENTRIES {
                        if dict.insert(key, value) {
                            sink.warn(token.start, WarningKind::DuplicateDictKey);
                        }
                    } else if !capped {
                        capped = true;
                        sink.warn(start, WarningKind::DictCapHit);
                    }
                }
                _ => {
                    sink.warn(token.start, WarningKind::DictKeyNotName);
                    // Consumed as a whole object so a container in key
                    // position cannot desynchronize the rest of the file.
                    let _ = self.object_from(token, sink);
                }
            }
        }
        self.depth -= 1;
        Object::Dict(dict)
    }

    /// Consumes tokens to the end of the container that was just opened,
    /// iteratively — this is what keeps the depth cap from needing a stack.
    fn skip_container(&mut self, sink: &mut WarningSink) {
        let mut depth = 1u32;
        loop {
            let token = self.next(sink);
            match token.kind {
                TokenKind::ArrayOpen | TokenKind::DictOpen => depth = depth.saturating_add(1),
                TokenKind::ArrayClose | TokenKind::DictClose => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                TokenKind::Eof => break,
                TokenKind::Keyword(k) if k.is_structural() => {
                    self.push_back(token);
                    break;
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PdfString;

    struct Fixture {
        names: NameTable,
        sink: WarningSink,
    }

    impl Fixture {
        fn new() -> Fixture {
            Fixture {
                names: NameTable::new(),
                sink: WarningSink::new(),
            }
        }

        fn object(&mut self, input: &[u8]) -> Object {
            parse_object_at(input, 0, &self.names, &mut self.sink).object
        }

        fn kinds(&self) -> Vec<WarningKind> {
            self.sink.warnings().iter().map(|w| w.kind).collect()
        }

        fn name(&self, bytes: &[u8]) -> Name {
            self.names.intern(bytes)
        }
    }

    fn parse(input: &[u8]) -> (Object, Vec<WarningKind>) {
        let mut f = Fixture::new();
        let object = f.object(input);
        let kinds = f.kinds();
        (object, kinds)
    }

    fn int_array(values: &[i64]) -> Object {
        Object::Array(values.iter().copied().map(Object::Int).collect())
    }

    // ---- 7.3 object forms ------------------------------------------------

    #[test]
    fn scalar_objects_parse() {
        assert_eq!(parse(b"true"), (Object::Bool(true), vec![]));
        assert_eq!(parse(b"false"), (Object::Bool(false), vec![]));
        assert_eq!(parse(b"null"), (Object::Null, vec![]));
        assert_eq!(parse(b"123"), (Object::Int(123), vec![]));
        assert_eq!(parse(b"34.5"), (Object::Real(34.5), vec![]));
        assert_eq!(
            parse(b"(Brillig)"),
            (
                Object::String(PdfString::literal(b"Brillig".to_vec())),
                vec![]
            )
        );
    }

    #[test]
    // 7.3.6's own example uses 3.14; it is a lexeme here, not an approximation
    // of pi.
    #[allow(clippy::approx_constant)]
    fn spec_array_example() {
        let mut f = Fixture::new();
        let object = f.object(b"[549 3.14 false (Ralph) /SomeName]");
        assert_eq!(
            object,
            Object::Array(vec![
                Object::Int(549),
                Object::Real(3.14),
                Object::Bool(false),
                Object::String(PdfString::literal(b"Ralph".to_vec())),
                Object::Name(f.name(b"SomeName")),
            ])
        );
        assert!(f.kinds().is_empty());
    }

    #[test]
    fn spec_dictionary_example() {
        let mut f = Fixture::new();
        let object = f.object(
            b"<< /Type /Example
                 /Subtype /DictionaryExample
                 /Version 0.01
                 /IntegerItem 12
                 /StringItem (a string)
                 /Subdictionary << /Item1 0.4
                                   /Item2 true
                                   /LastItem (not!)
                                   /VeryLastItem (OK)
                                >>
              >>",
        );
        assert!(f.kinds().is_empty(), "{:?}", f.kinds());
        let dict = object.as_dict().expect("a dictionary");
        assert_eq!(dict.len(), 6);
        assert_eq!(dict.get_name(Name::TYPE), Some(f.name(b"Example")));
        assert_eq!(dict.get_number(f.name(b"Version")), Some(0.01));
        assert_eq!(dict.get_int(f.name(b"IntegerItem")), Some(12));
        assert_eq!(
            dict.get_string(f.name(b"StringItem"))
                .map(|s| s.bytes.clone()),
            Some(b"a string".to_vec())
        );
        let sub = dict
            .get_dict(f.name(b"Subdictionary"))
            .expect("a subdictionary");
        assert_eq!(sub.len(), 4);
        assert_eq!(sub.get_bool(f.name(b"Item2")), Some(true));
        assert_eq!(
            sub.get_string(f.name(b"VeryLastItem"))
                .map(|s| s.bytes.clone()),
            Some(b"OK".to_vec())
        );
        // File order is preserved.
        let keys: Vec<Name> = dict.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys[0], Name::TYPE);
        assert_eq!(keys[5], f.name(b"Subdictionary"));
    }

    #[test]
    fn empty_containers_parse() {
        assert_eq!(parse(b"[]"), (Object::Array(Vec::new()), vec![]));
        assert_eq!(parse(b"<<>>"), (Object::Dict(Dict::new()), vec![]));
    }

    #[test]
    fn nested_containers_parse() {
        let (object, warnings) = parse(b"[[1 2] [3 [4]]]");
        assert_eq!(
            object,
            Object::Array(vec![
                int_array(&[1, 2]),
                Object::Array(vec![Object::Int(3), int_array(&[4])]),
            ])
        );
        assert!(warnings.is_empty());
    }

    // ---- 7.3.10 indirect references and objects --------------------------

    #[test]
    fn spec_indirect_reference_example() {
        assert_eq!(parse(b"12 0 R"), (Object::Ref(ObjRef::new(12, 0)), vec![]));
    }

    #[test]
    fn integers_that_are_not_references_stay_integers() {
        assert_eq!(parse(b"[1 2 3]"), (int_array(&[1, 2, 3]), vec![]));
        assert_eq!(parse(b"[1 0]"), (int_array(&[1, 0]), vec![]));
        let (object, warnings) = parse(b"[1 0 3 R]");
        assert_eq!(
            object,
            Object::Array(vec![Object::Int(1), Object::Ref(ObjRef::new(0, 3))])
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn reference_lookahead_does_not_lose_tokens() {
        let (object, warnings) = parse(b"[1 2 (x)]");
        assert_eq!(
            object,
            Object::Array(vec![
                Object::Int(1),
                Object::Int(2),
                Object::String(PdfString::literal(b"x".to_vec())),
            ])
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn out_of_range_reference_numbers_stay_integers() {
        let (object, warnings) = parse(b"[-1 0 R]");
        assert_eq!(
            object,
            Object::Array(vec![Object::Int(-1), Object::Int(0), Object::Null])
        );
        assert_eq!(warnings, [WarningKind::UnexpectedKeyword]);

        let (object, warnings) = parse(b"[1 99999 R]");
        assert_eq!(
            object,
            Object::Array(vec![Object::Int(1), Object::Int(99999), Object::Null])
        );
        assert_eq!(warnings, [WarningKind::UnexpectedKeyword]);
    }

    #[test]
    fn spec_indirect_object_example() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"12 0 obj\n    (Brillig)\nendobj\n";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        assert_eq!(parsed.reference, ObjRef::new(12, 0));
        assert_eq!(
            parsed.object,
            Object::String(PdfString::literal(b"Brillig".to_vec()))
        );
        assert_eq!(parsed.end_offset, 29);
        assert!(sink.is_empty());
    }

    #[test]
    fn indirect_header_must_match_the_grammar() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        for input in [
            &b"12 obj (x) endobj"[..],
            b"12 0 xobj (x) endobj",
            b"/Name 0 obj",
            b"trailer << >>",
            b"",
            b"-1 0 obj (x) endobj",
            b"1 99999 obj (x) endobj",
        ] {
            assert!(
                parse_indirect_at(input, 0, &names, &mut sink).is_none(),
                "{:?} should not be a header",
                core::str::from_utf8(input)
            );
        }
        // A rejected header leaves no warnings behind: whether it is a defect
        // is the caller's call.
        assert!(sink.is_empty(), "{:?}", sink.warnings());
    }

    #[test]
    fn indirect_warnings_carry_the_object() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        parse_indirect_at(b"7 1 obj (unterminated", 0, &names, &mut sink).expect("a header");
        assert_eq!(sink.warnings()[0].kind, WarningKind::UnterminatedString);
        assert_eq!(sink.warnings()[0].object, Some(ObjRef::new(7, 1)));
        // The context is restored afterwards.
        assert_eq!(sink.context(), None);
    }

    #[test]
    fn leniency_missing_endobj_is_reported_and_not_consumed() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"1 0 obj (a) 2 0 obj (b) endobj";
        let first = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        assert_eq!(first.reference, ObjRef::new(1, 0));
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::MissingEndobj]
        );
        // The next header is intact at end_offset.
        let second =
            parse_indirect_at(input, first.end_offset, &names, &mut sink).expect("a header");
        assert_eq!(second.reference, ObjRef::new(2, 0));
        assert_eq!(
            second.object,
            Object::String(PdfString::literal(b"b".to_vec()))
        );
    }

    // ---- 7.3.8 streams ---------------------------------------------------

    #[test]
    fn stream_with_direct_length() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"3 0 obj\n<< /Length 5 >>\nstream\nHELLO\nendstream\nendobj\n";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        let stream = parsed.object.as_stream().expect("a stream");
        assert_eq!(stream.len_hint, Some(5));
        let range = stream.data_range();
        assert_eq!(&input[range.start as usize..range.end as usize], b"HELLO");
        assert_eq!(parsed.end_offset, range.end);
        assert!(sink.is_empty(), "{:?}", sink.warnings());
    }

    #[test]
    fn stream_with_crlf_after_the_keyword() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"3 0 obj\n<< /Length 2 >>\r\nstream\r\nHI\r\nendstream\r\nendobj";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        let stream = parsed.object.as_stream().expect("a stream");
        let range = stream.data_range();
        assert_eq!(&input[range.start as usize..range.end as usize], b"HI");
        assert!(sink.is_empty(), "{:?}", sink.warnings());
    }

    #[test]
    fn leniency_stream_keyword_followed_by_lone_cr() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"3 0 obj << /Length 2 >> stream\rHI\rendstream endobj";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        let stream = parsed.object.as_stream().expect("a stream");
        let range = stream.data_range();
        assert_eq!(&input[range.start as usize..range.end as usize], b"HI");
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::StreamKeywordBadEol]
        );
    }

    #[test]
    fn leniency_stream_keyword_followed_by_blanks_then_eol() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"3 0 obj << /Length 2 >> stream  \t\nHI\nendstream endobj";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        let stream = parsed.object.as_stream().expect("a stream");
        let range = stream.data_range();
        assert_eq!(&input[range.start as usize..range.end as usize], b"HI");
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::StreamKeywordBadEol]
        );
    }

    #[test]
    fn leniency_stream_keyword_with_no_eol_at_all() {
        // The keyword still has to be a token, so the data begins with a
        // delimiter here; data that begins with a regular character would make
        // `stream` unrecognizable to any tokenizer.
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"3 0 obj << /Length 4 >> stream(AB) endstream endobj";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        let stream = parsed.object.as_stream().expect("a stream");
        let range = stream.data_range();
        assert_eq!(&input[range.start as usize..range.end as usize], b"(AB)");
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::StreamKeywordBadEol]
        );
    }

    #[test]
    fn stream_data_beginning_with_lf_is_not_skipped() {
        // The blank-skipping recovery must not eat data bytes: only blanks are
        // skipped, so a data byte of 0x0a stays data.
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"3 0 obj << /Length 3 >> stream\n\nAB\nendstream endobj";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        let stream = parsed.object.as_stream().expect("a stream");
        let range = stream.data_range();
        assert_eq!(&input[range.start as usize..range.end as usize], b"\nAB");
        assert!(sink.is_empty(), "{:?}", sink.warnings());
    }

    #[test]
    fn indirect_stream_length_stays_unresolved() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"3 0 obj\n<< /Length 9 0 R >>\nstream\nHELLO\nendstream\nendobj\n";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        let stream = parsed.object.as_stream().expect("a stream");
        assert_eq!(stream.dict.get_ref(Name::LENGTH), Some(ObjRef::new(9, 0)));
        assert_eq!(stream.len_hint, None);
        assert!(stream.data_range().is_empty());
        assert_eq!(stream.data_start, 35);
        assert_eq!(parsed.end_offset, stream.data_start);
        // An indirect /Length is legal; nothing to warn about.
        assert!(sink.is_empty(), "{:?}", sink.warnings());
    }

    #[test]
    fn leniency_missing_stream_length() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"3 0 obj\n<< /Type /X >>\nstream\nHELLO\nendstream\nendobj\n";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        let stream = parsed.object.as_stream().expect("a stream");
        assert_eq!(stream.len_hint, None);
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::StreamLengthMissing]
        );
    }

    #[test]
    fn leniency_negative_stream_length() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"3 0 obj\n<< /Length -4 >>\nstream\nHELLO\nendstream\nendobj\n";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        assert_eq!(parsed.object.as_stream().expect("a stream").len_hint, None);
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::StreamLengthMissing]
        );
    }

    #[test]
    fn overlong_stream_length_clamps_to_the_buffer() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"3 0 obj << /Length 9999 >> stream\nHI\nendstream endobj";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        let stream = parsed.object.as_stream().expect("a stream");
        // The claim is preserved raw; only end_offset is clamped, because the
        // document layer needs the declared value to detect the lie.
        assert_eq!(stream.len_hint, Some(9999));
        assert_eq!(parsed.end_offset, input.len() as u64);
    }

    #[test]
    fn leniency_stream_keyword_after_a_non_dictionary() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"3 0 obj 42 stream\nHI\nendstream endobj";
        let parsed = parse_indirect_at(input, 0, &names, &mut sink).expect("a header");
        assert_eq!(parsed.object, Object::Int(42));
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::StreamWithoutDict]
        );
    }

    // ---- structural leniency ---------------------------------------------

    #[test]
    fn leniency_unterminated_array_at_eof() {
        let (object, warnings) = parse(b"[1 2");
        assert_eq!(object, int_array(&[1, 2]));
        assert_eq!(warnings, [WarningKind::UnterminatedArray]);
    }

    #[test]
    fn leniency_unterminated_dict_at_eof() {
        let mut f = Fixture::new();
        let object = f.object(b"<< /A 1");
        let dict = object.as_dict().expect("a dictionary");
        assert_eq!(dict.get_int(f.name(b"A")), Some(1));
        assert_eq!(f.kinds(), [WarningKind::UnterminatedDict]);
    }

    #[test]
    fn leniency_mismatched_close_ends_the_array_without_eating_it() {
        let mut f = Fixture::new();
        let object = f.object(b"<< /A [1 2 >> /B 3 >>");
        let dict = object.as_dict().expect("a dictionary");
        assert_eq!(
            dict.get_array(f.name(b"A")),
            Some(&[Object::Int(1), Object::Int(2)][..])
        );
        // The >> that ended the array is the one that ends the dictionary, so
        // /B is not in it.
        assert_eq!(dict.len(), 1);
        assert_eq!(
            f.kinds(),
            [WarningKind::MismatchedClose, WarningKind::UnterminatedArray]
        );
    }

    #[test]
    fn leniency_stray_array_close_inside_a_dict_is_dropped() {
        let mut f = Fixture::new();
        let object = f.object(b"<< /A 1 ] /B 2 >>");
        let dict = object.as_dict().expect("a dictionary");
        assert_eq!(dict.get_int(f.name(b"A")), Some(1));
        assert_eq!(dict.get_int(f.name(b"B")), Some(2));
        assert_eq!(f.kinds(), [WarningKind::MismatchedClose]);
    }

    #[test]
    fn leniency_structural_keyword_ends_a_container() {
        let mut f = Fixture::new();
        let object = f.object(b"<< /A 1 endobj");
        let dict = object.as_dict().expect("a dictionary");
        assert_eq!(dict.get_int(f.name(b"A")), Some(1));
        assert_eq!(f.kinds(), [WarningKind::UnterminatedDict]);

        let (object, warnings) = parse(b"[1 2 endstream");
        assert_eq!(object, int_array(&[1, 2]));
        assert_eq!(warnings, [WarningKind::UnterminatedArray]);
    }

    #[test]
    fn leniency_dict_key_is_not_a_name() {
        let mut f = Fixture::new();
        let object = f.object(b"<< 42 /A 1 >>");
        let dict = object.as_dict().expect("a dictionary");
        assert_eq!(dict.len(), 1);
        assert_eq!(dict.get_int(f.name(b"A")), Some(1));
        assert_eq!(f.kinds(), [WarningKind::DictKeyNotName]);
    }

    #[test]
    fn leniency_container_in_key_position_is_consumed_whole() {
        let mut f = Fixture::new();
        let object = f.object(b"<< [/A 1] /B 2 >>");
        let dict = object.as_dict().expect("a dictionary");
        assert_eq!(dict.len(), 1);
        assert_eq!(dict.get_int(f.name(b"B")), Some(2));
        assert_eq!(f.kinds(), [WarningKind::DictKeyNotName]);
    }

    #[test]
    fn leniency_key_without_a_value() {
        let mut f = Fixture::new();
        let object = f.object(b"<< /A 1 /B >>");
        let dict = object.as_dict().expect("a dictionary");
        assert_eq!(dict.len(), 2);
        assert_eq!(dict.get(f.name(b"B")), Some(&Object::Null));
        assert_eq!(f.kinds(), [WarningKind::MissingDictValue]);
    }

    #[test]
    fn leniency_key_without_a_value_at_eof() {
        let mut f = Fixture::new();
        let object = f.object(b"<< /A");
        let dict = object.as_dict().expect("a dictionary");
        assert_eq!(dict.get(f.name(b"A")), Some(&Object::Null));
        assert_eq!(
            f.kinds(),
            [WarningKind::MissingDictValue, WarningKind::UnterminatedDict]
        );
    }

    #[test]
    fn leniency_duplicate_key_keeps_the_later_value_in_place() {
        let mut f = Fixture::new();
        let object = f.object(b"<< /A 1 /B 2 /A 3 >>");
        let dict = object.as_dict().expect("a dictionary");
        assert_eq!(dict.len(), 2);
        assert_eq!(dict.get_int(f.name(b"A")), Some(3));
        let keys: Vec<Name> = dict.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, [f.name(b"A"), f.name(b"B")]);
        assert_eq!(f.kinds(), [WarningKind::DuplicateDictKey]);
    }

    #[test]
    fn leniency_keyword_in_value_position() {
        let mut f = Fixture::new();
        let object = f.object(b"<< /A R /B 1 >>");
        let dict = object.as_dict().expect("a dictionary");
        assert_eq!(dict.get(f.name(b"A")), Some(&Object::Null));
        assert_eq!(dict.get_int(f.name(b"B")), Some(1));
        assert_eq!(f.kinds(), [WarningKind::UnexpectedKeyword]);
    }

    #[test]
    fn leniency_unexpected_token_reads_as_null() {
        assert_eq!(
            parse(b")"),
            (Object::Null, vec![WarningKind::UnexpectedToken])
        );
        assert_eq!(
            parse(b""),
            (Object::Null, vec![WarningKind::UnexpectedToken])
        );
        assert_eq!(
            parse(b"junk"),
            (Object::Null, vec![WarningKind::UnexpectedToken])
        );
    }

    #[test]
    fn leniency_depth_cap_reads_as_null_without_recursing() {
        let depth = (limits::MAX_NEST_DEPTH as usize) + 50;
        let mut input = vec![b'['; depth];
        input.extend(std::iter::repeat_n(b']', depth));
        let (object, warnings) = parse(&input);
        assert_eq!(warnings, [WarningKind::DepthCapHit]);
        // The cap is hit exactly once: skipping is iterative, so the rest of
        // the nest costs no stack and produces no further warnings.
        let mut level = 0;
        let mut node = &object;
        while let Object::Array(items) = node {
            if items.is_empty() {
                break;
            }
            level += 1;
            node = &items[0];
        }
        assert_eq!(level, limits::MAX_NEST_DEPTH as usize);
        assert_eq!(node, &Object::Null);
    }

    #[test]
    fn depth_cap_skips_to_the_right_place() {
        let mut input = vec![b'['; (limits::MAX_NEST_DEPTH as usize) + 1];
        input.extend(std::iter::repeat_n(
            b']',
            (limits::MAX_NEST_DEPTH as usize) + 1,
        ));
        input.extend_from_slice(b" 7");
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let parsed = parse_object_at(&input, 0, &names, &mut sink);
        // Everything after the over-deep container is still reachable.
        let next = parse_object_at(&input, parsed.end_offset, &names, &mut sink);
        assert_eq!(next.object, Object::Int(7));
    }

    #[test]
    fn deeply_nested_dictionaries_do_not_overflow_the_stack() {
        let depth = 5_000;
        let mut input = Vec::new();
        for _ in 0..depth {
            input.extend_from_slice(b"<< /A ");
        }
        for _ in 0..depth {
            input.extend_from_slice(b">> ");
        }
        let (_, warnings) = parse(&input);
        assert_eq!(warnings[0], WarningKind::DepthCapHit);
    }

    #[test]
    fn array_cap_drops_the_excess_and_keeps_parsing() {
        // The cap is documented, not free to raise, so exercise it through a
        // parser configured the same way the real one is.
        let mut input = vec![b'['];
        for _ in 0..limits::MAX_ARRAY_LEN + 3 {
            input.extend_from_slice(b"0 ");
        }
        input.extend_from_slice(b"] 7");
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let parsed = parse_object_at(&input, 0, &names, &mut sink);
        match &parsed.object {
            Object::Array(items) => assert_eq!(items.len(), limits::MAX_ARRAY_LEN),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::ArrayCapHit]
        );
        let next = parse_object_at(&input, parsed.end_offset, &names, &mut sink);
        assert_eq!(next.object, Object::Int(7));
    }

    #[test]
    fn dict_cap_drops_the_excess_and_keeps_parsing() {
        let mut input = vec![b'<', b'<'];
        for i in 0..limits::MAX_DICT_ENTRIES + 3 {
            input.extend_from_slice(format!("/k{i} {i} ").as_bytes());
        }
        input.extend_from_slice(b">> 7");
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let parsed = parse_object_at(&input, 0, &names, &mut sink);
        match &parsed.object {
            Object::Dict(dict) => assert_eq!(dict.len(), limits::MAX_DICT_ENTRIES),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::DictCapHit]
        );
        let next = parse_object_at(&input, parsed.end_offset, &names, &mut sink);
        assert_eq!(next.object, Object::Int(7));
    }

    #[test]
    fn end_offset_stops_after_the_object() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"[1 2] /Next";
        let parsed = parse_object_at(input, 0, &names, &mut sink);
        assert_eq!(parsed.end_offset, 5);
        let next = parse_object_at(input, parsed.end_offset, &names, &mut sink);
        assert_eq!(next.object, Object::Name(names.intern(b"Next")));
        assert!(sink.is_empty());
    }

    #[test]
    fn end_offset_after_reference_lookahead_is_exact() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        // The lookahead reads two tokens past the integer and must give them
        // back, offset included.
        let input = b"1 2 3";
        let parsed = parse_object_at(input, 0, &names, &mut sink);
        assert_eq!(parsed.object, Object::Int(1));
        assert_eq!(parsed.end_offset, 2);
        let next = parse_object_at(input, parsed.end_offset, &names, &mut sink);
        assert_eq!(next.object, Object::Int(2));
        assert_eq!(next.end_offset, 4);
    }

    #[test]
    fn parsing_starts_at_the_given_offset() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let input = b"garbage (wanted)";
        let parsed = parse_object_at(input, 8, &names, &mut sink);
        assert_eq!(
            parsed.object,
            Object::String(PdfString::literal(b"wanted".to_vec()))
        );
        assert!(sink.is_empty());
    }

    #[test]
    fn offset_past_the_end_reads_as_null() {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let parsed = parse_object_at(b"123", u64::MAX, &names, &mut sink);
        assert_eq!(parsed.object, Object::Null);
        assert_eq!(parsed.end_offset, 3);
        assert_eq!(
            sink.warnings().iter().map(|w| w.kind).collect::<Vec<_>>(),
            [WarningKind::UnexpectedToken]
        );
    }

    #[test]
    fn comments_between_tokens_are_ignored() {
        let mut f = Fixture::new();
        let object = f.object(b"<< % a comment\n /A 1 % another\n >>");
        assert_eq!(
            object.as_dict().and_then(|d| d.get_int(f.name(b"A"))),
            Some(1)
        );
        assert!(f.kinds().is_empty());
    }
}
