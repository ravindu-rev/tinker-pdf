//! `css-syntax-3` §3 and §4: the preprocessing and the seventeen algorithms.

use crate::tokenizer::{tokenize, HashKind, Token};

fn tokens(source: &str) -> Vec<Token> {
    tokenize(source)
}

/// §3.3, all four replacements, in one source.
///
/// Each is asserted on its own rather than through a joined string, because a
/// build that folded three of the four would pass a test that only counted the
/// tokens: `\r\n` and `\r` and `\f` all produce one whitespace token, and so
/// does a build that treats `\f` as an ordinary delim next to a newline.
#[test]
fn preprocessing_folds_three_newlines_and_the_null() {
    // Inside a string, where the answer is visible: the string's own bytes.
    assert_eq!(tokens("\"a\\\r\nb\""), vec![Token::Str("ab".into())]);
    // A form feed is a newline, so it terminates a string as a bad-string.
    assert_eq!(
        tokens("\"a\u{c}b\"").first(),
        Some(&Token::BadString),
        "a form feed inside a string is a newline and makes it bad"
    );
    // A lone carriage return is a newline too, and by the same test.
    assert_eq!(tokens("\"a\rb\"").first(), Some(&Token::BadString));
    // U+0000 is U+FFFD, and it is a *name* code point, so it lands inside an
    // identifier rather than beside one.
    assert_eq!(tokens("a\0b"), vec![Token::Ident("a\u{fffd}b".into())]);
}

/// A comment between two whitespace runs is one whitespace token.
///
/// `a /* x */ b` is **one** descendant combinator. A tokenizer that emitted
/// whitespace, nothing, whitespace would give the selector parser two
/// combinators for one pair of compounds — and the parser would then have to
/// know that two descendants in a row mean one, which is a rule nothing else
/// would ever state.
#[test]
fn a_comment_between_whitespace_is_one_whitespace_token() {
    assert_eq!(
        tokens("a /* x */ b"),
        vec![
            Token::Ident("a".into()),
            Token::Whitespace,
            Token::Ident("b".into())
        ]
    );
    // And with no whitespace at all around it, a comment is not a token and
    // not a separator: `a/**/b` is two identifiers.
    assert_eq!(
        tokens("a/**/b"),
        vec![Token::Ident("a".into()), Token::Ident("b".into())]
    );
}

/// §4.3.2: an unterminated comment runs to EOF and is a parse error, not a
/// refusal. A stylesheet that ends mid-comment still yields the rules above it.
#[test]
fn an_unterminated_comment_runs_to_the_end() {
    assert_eq!(
        tokens("a /* and then nothing"),
        vec![Token::Ident("a".into()), Token::Whitespace]
    );
}

/// §4.3.5: a newline inside a string is a bad-string **and the newline is
/// reconsumed**.
///
/// Both halves, separately. A build that consumed the newline would swallow
/// the separator, so `"a\n;color:red` would lose the semicolon and take the
/// declaration after it down with it — which is a recovery bug hiding inside a
/// tokenizer bug.
#[test]
fn a_newline_in_a_string_is_bad_and_the_newline_survives() {
    assert_eq!(
        tokens("\"a\nb\""),
        vec![
            Token::BadString,
            Token::Whitespace,
            Token::Ident("b".into()),
            // The closing quote of the source opens a new, unterminated string.
            Token::Str(String::new()),
        ]
    );
    // A backslash *before* the newline is a line continuation and keeps the
    // string whole, which is the opposite answer from the same two characters.
    assert_eq!(tokens("\"a\\\nb\""), vec![Token::Str("ab".into())]);
}

/// §4.3.7's escapes, including the three code points that become U+FFFD.
#[test]
fn escapes_resolve_and_three_become_the_replacement_character() {
    assert_eq!(tokens("\\41"), vec![Token::Ident("A".into())]);
    // One optional whitespace after a hex escape is part of the escape.
    assert_eq!(tokens("\\41 b"), vec![Token::Ident("Ab".into())]);
    // Two spaces: the first ends the escape and the second is a token.
    assert_eq!(
        tokens("\\41  b"),
        vec![
            Token::Ident("A".into()),
            Token::Whitespace,
            Token::Ident("b".into())
        ]
    );
    assert_eq!(tokens("\\0"), vec![Token::Ident("\u{fffd}".into())]);
    assert_eq!(tokens("\\D800"), vec![Token::Ident("\u{fffd}".into())]);
    assert_eq!(tokens("\\110000"), vec![Token::Ident("\u{fffd}".into())]);
}

/// §4.3.4: a hash is an *id* only when what follows would start an identifier.
///
/// It is the rule that decides whether `#0f0` can be an id selector — it
/// cannot, and a build that flagged every hash as an id would accept
/// `#0f0 { }` as a rule for an element with `id="0f0"`.
#[test]
fn a_hash_is_an_id_only_when_it_starts_an_identifier() {
    assert_eq!(
        tokens("#main"),
        vec![Token::Hash("main".into(), HashKind::Id)]
    );
    assert_eq!(
        tokens("#0f0"),
        vec![Token::Hash("0f0".into(), HashKind::Unrestricted)]
    );
    assert_eq!(
        tokens("#-a"),
        vec![Token::Hash("-a".into(), HashKind::Id)],
        "a leading hyphen still starts an identifier"
    );
    assert_eq!(tokens("# "), vec![Token::Delim('#'), Token::Whitespace]);
}

/// §4.3.2: `url(x)` is a url token and `url("x")` is a function token.
///
/// The difference is load-bearing for `@import`, which accepts both spellings
/// and would silently resolve neither if the tokenizer collapsed them.
#[test]
fn url_unquoted_is_a_url_token_and_quoted_is_a_function() {
    assert_eq!(tokens("url(a.css)"), vec![Token::Url("a.css".into())]);
    assert_eq!(
        tokens("url(\"a.css\")"),
        vec![
            Token::Function("url".into()),
            Token::Str("a.css".into()),
            Token::CloseParen
        ]
    );
    assert_eq!(
        tokens("url(  a.css  )"),
        vec![Token::Url("a.css".into())],
        "§4.3.6 strips the whitespace either side"
    );
}

/// §4.3.14: a bad url swallows to its closing paren, so the rules after it
/// survive.
#[test]
fn a_bad_url_swallows_to_the_close_paren() {
    assert_eq!(
        tokens("url(a'b) c"),
        vec![Token::BadUrl, Token::Whitespace, Token::Ident("c".into())]
    );
}

/// §4.3.12's type flag, and the three shapes that make a number not an integer.
#[test]
fn the_number_grammar_keeps_its_integer_flag() {
    assert_eq!(
        tokens("400"),
        vec![Token::Number {
            value: 400.0,
            integer: true
        }]
    );
    assert_eq!(
        tokens("1.5"),
        vec![Token::Number {
            value: 1.5,
            integer: false
        }]
    );
    assert_eq!(
        tokens("1e2"),
        vec![Token::Number {
            value: 100.0,
            integer: false
        }]
    );
    assert_eq!(
        tokens("-0.5"),
        vec![Token::Number {
            value: -0.5,
            integer: false
        }]
    );
    // `.5` is a number and `.a` is a delim followed by an identifier, which is
    // what makes `.class` a class selector.
    assert_eq!(
        tokens(".5"),
        vec![Token::Number {
            value: 0.5,
            integer: false
        }]
    );
    assert_eq!(
        tokens(".a"),
        vec![Token::Delim('.'), Token::Ident("a".into())]
    );
}

/// A dimension and a percentage are neither numbers nor identifiers.
#[test]
fn a_dimension_and_a_percentage_are_their_own_tokens() {
    assert_eq!(
        tokens("12px"),
        vec![Token::Dimension {
            value: 12.0,
            unit: "px".into()
        }]
    );
    assert_eq!(tokens("50%"), vec![Token::Percentage(50.0)]);
    assert_eq!(
        tokens("1E"),
        vec![Token::Dimension {
            value: 1.0,
            unit: "E".into()
        }],
        "an `e` with no exponent digits is a unit, not an exponent"
    );
}

/// §4.3.1's CDO and CDC, which exist so a 1996 stylesheet inside an HTML
/// comment still parses.
#[test]
fn cdo_and_cdc_are_tokens() {
    assert_eq!(
        tokens("<!-- a -->"),
        vec![
            Token::Cdo,
            Token::Whitespace,
            Token::Ident("a".into()),
            Token::Whitespace,
            Token::Cdc
        ]
    );
    // And the rule that makes the spaces necessary: `-` is a name code point,
    // so an identifier running up to a `-->` eats both hyphens and the CDC
    // never happens. This was written the other way round first and the
    // tokenizer was right.
    assert_eq!(
        tokens("<!--a-->"),
        vec![Token::Cdo, Token::Ident("a--".into()), Token::Delim('>')]
    );
}

/// `+` and `-` are delims unless they start a number, which is what makes
/// `p + p` a sibling combinator and `+1` a number.
#[test]
fn a_sign_that_does_not_start_a_number_is_a_delim() {
    assert_eq!(
        tokens("p + p"),
        vec![
            Token::Ident("p".into()),
            Token::Whitespace,
            Token::Delim('+'),
            Token::Whitespace,
            Token::Ident("p".into())
        ]
    );
    assert_eq!(
        tokens("+1"),
        vec![Token::Number {
            value: 1.0,
            integer: true
        }]
    );
    assert_eq!(tokens("-a"), vec![Token::Ident("-a".into())]);
    assert_eq!(tokens("--a"), vec![Token::Ident("--a".into())]);
}

/// U+00A0 is a name code point and **not** whitespace, so it belongs inside an
/// identifier. §4.2's whitespace is three characters and no more.
#[test]
fn a_no_break_space_is_part_of_an_identifier() {
    assert_eq!(tokens("a\u{a0}b"), vec![Token::Ident("a\u{a0}b".into())]);
}
