//! Standing properties for the scanner and the object parser: escaping round
//! trips, and nothing panics on arbitrary bytes (ruling 1).

use proptest::prelude::*;

use tinker_pdf_cos::lexer::is_regular;
use tinker_pdf_cos::{
    parse_indirect_at, parse_object_at, Lexer, NameTable, Object, TokenKind, WarningSink,
};

/// Writes `bytes` as a literal string (7.3.4.2), escaping exactly what has to
/// be escaped: the parentheses and the reverse solidus because they are
/// syntax, and CR because a raw one inside a string means LF.
fn escape_literal(bytes: &[u8]) -> Vec<u8> {
    let mut out = vec![b'('];
    for &b in bytes {
        match b {
            b'(' => out.extend_from_slice(b"\\("),
            b')' => out.extend_from_slice(b"\\)"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\r' => out.extend_from_slice(b"\\r"),
            _ => out.push(b),
        }
    }
    out.push(b')');
    out
}

/// Writes `bytes` as a name (7.3.5), `#`-escaping everything outside the
/// printable regular characters, plus `#` itself.
fn escape_name(bytes: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = vec![b'/'];
    for &b in bytes {
        if b == b'#' || !is_regular(b) || !(0x21..=0x7e).contains(&b) {
            out.push(b'#');
            out.push(HEX[usize::from(b >> 4)]);
            out.push(HEX[usize::from(b & 0x0f)]);
        } else {
            out.push(b);
        }
    }
    out
}

fn lex_all(input: &[u8]) -> Vec<(TokenKind, u64, u64)> {
    let mut sink = WarningSink::new();
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next_token(&mut sink);
        let eof = token.is_eof();
        tokens.push((token.kind, token.start, token.end));
        if eof {
            return tokens;
        }
    }
}

proptest! {
    #[test]
    fn literal_string_escaping_round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let mut sink = WarningSink::new();
        let encoded = escape_literal(&bytes);
        let token = Lexer::new(&encoded).next_token(&mut sink);
        prop_assert_eq!(token.start, 0);
        prop_assert_eq!(token.end, encoded.len() as u64);
        prop_assert!(sink.is_empty(), "{:?}", sink.warnings());
        match token.kind {
            TokenKind::String(s) => {
                prop_assert!(!s.hex);
                prop_assert_eq!(s.bytes, bytes);
            }
            other => prop_assert!(false, "expected a string, got {:?}", other),
        }
    }

    #[test]
    fn name_escaping_round_trips(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let mut sink = WarningSink::new();
        let encoded = escape_name(&bytes);
        let token = Lexer::new(&encoded).next_token(&mut sink);
        prop_assert_eq!(token.end, encoded.len() as u64);
        prop_assert!(sink.is_empty(), "{:?}", sink.warnings());
        match token.kind {
            TokenKind::Name(decoded) => prop_assert_eq!(decoded, bytes),
            other => prop_assert!(false, "expected a name, got {:?}", other),
        }
    }

    #[test]
    fn interned_names_round_trip_through_the_table(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let encoded = escape_name(&bytes);
        let parsed = parse_object_at(&encoded, 0, &names, &mut sink);
        match parsed.object {
            Object::Name(name) => {
                let interned = names.bytes(name);
                prop_assert_eq!(interned.as_deref(), Some(bytes.as_slice()));
            }
            other => prop_assert!(false, "expected a name, got {:?}", other),
        }
    }

    #[test]
    fn lexer_never_panics_on_arbitrary_bytes(input in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let tokens = lex_all(&input);
        let mut previous_end = 0u64;
        for (kind, start, end) in tokens {
            prop_assert!(start >= previous_end, "tokens overlap at {}", start);
            prop_assert!(end <= input.len() as u64, "token runs past the buffer");
            if !matches!(kind, TokenKind::Eof) {
                prop_assert!(end > start, "token made no progress at {}", start);
            }
            previous_end = end;
        }
    }

    #[test]
    fn parser_never_panics_on_arbitrary_bytes(input in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let parsed = parse_object_at(&input, 0, &names, &mut sink);
        prop_assert!(parsed.end_offset <= input.len() as u64);
        let _ = parse_indirect_at(&input, 0, &names, &mut sink);
    }

    #[test]
    fn parser_never_panics_at_any_offset(
        input in proptest::collection::vec(any::<u8>(), 0..512),
        offset in any::<u64>(),
    ) {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let parsed = parse_object_at(&input, offset, &names, &mut sink);
        prop_assert!(parsed.end_offset <= input.len() as u64);
        let _ = parse_indirect_at(&input, offset, &names, &mut sink);
    }

    #[test]
    fn arbitrary_bytes_wrapped_as_an_indirect_object_keep_their_reference(
        num in 0u32..100_000,
        gen in 0u16..1000,
        body in proptest::collection::vec(any::<u8>(), 0..512),
    ) {
        let names = NameTable::new();
        let mut sink = WarningSink::new();
        let mut input = format!("{num} {gen} obj\n").into_bytes();
        input.extend_from_slice(&body);
        input.extend_from_slice(b"\nendobj\n");
        let parsed = parse_indirect_at(&input, 0, &names, &mut sink).expect("a header");
        prop_assert_eq!(parsed.reference.num, num);
        prop_assert_eq!(parsed.reference.gen, gen);
        prop_assert!(parsed.end_offset <= input.len() as u64);
    }
}
