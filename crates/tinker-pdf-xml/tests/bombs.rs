//! The four bombs gap 30 names, as committed inputs, each refused **by that
//! name**.
//!
//! Gap 30's design section says why this file exists rather than a paragraph
//! saying the parser is safe: *"a mitigation nobody named is a mitigation
//! nobody can check"*. The four inputs beside this file are the four attacks
//! ECMA-388 9.3.2's [M2.71] is about, and every one of them is asserted to be
//! [`Error::DoctypeUnsupported`] specifically — never merely "an error", and
//! never a cap being hit.
//!
//! **The difference between those two assertions is the whole of the defence.**
//! A parser that bounded entity expansion would also refuse billion laughs; it
//! would refuse it as a depth or work cap, after having parsed a document type
//! declaration, built an entity table and begun substituting. Asserting the
//! name says the declaration was never parsed at all, which is a structural
//! property rather than a budget — and a budget is what gap 18a's milestone 8
//! found set above its own inputs' ceiling, where it could never fire.
//!
//! `no-doctype.xml` is the twin that must parse. A suite in which every
//! committed input is refused cannot tell a reader that refuses DTDs from one
//! that refuses everything, and the second would pass the first's tests — gap
//! 29 milestone 5's lesson, that a positive assertion cannot catch a weakened
//! check, arriving from the other side.

use tinker_pdf_xml::{Error, Limits, Source};

const BILLION_LAUGHS: &[u8] = include_bytes!("bombs/billion-laughs.xml");
const QUADRATIC_BLOWUP: &[u8] = include_bytes!("bombs/quadratic-blowup.xml");
const EXTERNAL_ENTITY: &[u8] = include_bytes!("bombs/external-entity.xml");
const PARAMETER_ENTITY: &[u8] = include_bytes!("bombs/parameter-entity.xml");
const EXTERNAL_SUBSET: &[u8] = include_bytes!("bombs/external-subset.xml");
const NO_DOCTYPE: &[u8] = include_bytes!("bombs/no-doctype.xml");

const BOMBS: [(&str, &[u8]); 5] = [
    ("billion-laughs.xml", BILLION_LAUGHS),
    ("quadratic-blowup.xml", QUADRATIC_BLOWUP),
    ("external-entity.xml", EXTERNAL_ENTITY),
    ("parameter-entity.xml", PARAMETER_ENTITY),
    ("external-subset.xml", EXTERNAL_SUBSET),
];

/// The first refusal, and how far the reader got before it.
fn first_refusal(bytes: &[u8]) -> (Option<Error>, usize, usize) {
    let source = Source::new(bytes).expect("all six files are UTF-8");
    let mut reader = source.reader(&Limits::DEFAULT);
    let mut produced = 0usize;
    for event in reader.by_ref() {
        match event {
            Ok(_) => produced += 1,
            Err(error) => return (Some(error), produced, reader.offset()),
        }
    }
    (None, produced, reader.offset())
}

/// The exit criterion, verbatim: each asserted to be refused *by that name* and
/// not merely refused.
#[test]
fn every_bomb_is_refused_as_a_doctype_and_not_as_a_cap() {
    for (name, bytes) in BOMBS {
        let (error, produced, _) = first_refusal(bytes);
        assert_eq!(
            error,
            Some(Error::DoctypeUnsupported),
            "{name} was not refused as a document type declaration"
        );
        // If any of these had been refused by a bound instead, the error would
        // be one of the four below and this assertion is what says which
        // defence ran.
        assert!(
            !matches!(
                error,
                Some(Error::DepthCap | Error::AttributeCap | Error::NameCap | Error::TokenCap)
            ),
            "{name} was refused by a cap, which means the declaration was parsed"
        );
        assert_eq!(
            produced, 0,
            "{name} produced {produced} events before the declaration was refused"
        );
    }
}

/// And refused *before one byte after it is read*, which is the other half of
/// the criterion and the half a name alone cannot show.
///
/// The offset the reader stops at is compared against the offset of `<!DOCTYPE`
/// in the file itself, so a reader that consumed the declaration and then
/// refused — which would still be `DoctypeUnsupported` — fails here.
#[test]
fn no_bomb_is_read_past_its_declaration() {
    for (name, bytes) in BOMBS {
        let text = std::str::from_utf8(bytes).expect("UTF-8");
        let declaration = text.find("<!DOCTYPE").expect("every bomb has one");
        let (_, _, offset) = first_refusal(bytes);
        assert_eq!(
            offset,
            declaration,
            "{name}: the reader stopped {} bytes past `<!DOCTYPE`",
            offset.saturating_sub(declaration)
        );
    }
}

/// What each of these would have cost a parser that expanded rather than
/// refused, recorded as a number so the refusal has a size beside it.
#[test]
fn the_expansion_each_bomb_asks_for_is_recorded_rather_than_described() {
    // Ten levels of ten, from a 822-byte file: 10^9 characters.
    assert!(BILLION_LAUGHS.len() < 1024);
    // One kilobyte referenced 2 048 times, from a file of twenty-one:
    // two megabytes, and its *depth* is one, which is why a depth cap would
    // not have seen it.
    assert!(QUADRATIC_BLOWUP.len() < 32 * 1024);
    assert_eq!(
        std::str::from_utf8(QUADRATIC_BLOWUP)
            .expect("UTF-8")
            .matches("&kilobyte;")
            .count(),
        2048,
    );
    // And neither of the other two expands at all. They read a file or open a
    // socket, which this engine could not do — and the point of refusing the
    // declaration is that a parser one refactor away from resolving one never
    // gets the chance.
    let external = std::str::from_utf8(EXTERNAL_ENTITY).expect("UTF-8");
    assert!(external.contains("SYSTEM \"file:///"));
    assert!(external.contains("SYSTEM \"http://"));
    assert!(std::str::from_utf8(PARAMETER_ENTITY)
        .expect("UTF-8")
        .contains("<!ENTITY % "));
}

/// The twin, which must parse — otherwise the five above prove nothing.
#[test]
fn the_bomb_shaped_file_without_a_doctype_parses() {
    let (error, produced, _) = first_refusal(NO_DOCTYPE);
    assert_eq!(error, None, "the twin was refused");
    assert!(
        produced > 8,
        "the twin produced only {produced} events, so it is not a document"
    );
}

/// A DTD is refused wherever it is, including where a hand-written fixture
/// would never put one.
#[test]
fn a_declaration_in_any_of_the_three_places_is_refused_by_the_same_name() {
    for document in [
        b"<!DOCTYPE a><a/>".as_slice(),
        b"<a><!DOCTYPE a></a>".as_slice(),
        b"<a/><!DOCTYPE a>".as_slice(),
        b"\xEF\xBB\xBF<!DOCTYPE a><a/>".as_slice(),
        b"<!-- first --><!DOCTYPE a><a/>".as_slice(),
        b"<?pi v?><!DOCTYPE a><a/>".as_slice(),
    ] {
        let (error, _, _) = first_refusal(document);
        assert_eq!(
            error,
            Some(Error::DoctypeUnsupported),
            "{} was not refused",
            String::from_utf8_lossy(document)
        );
    }
}

/// UTF-16 is the encoding a filter written against bytes misses, and it is a
/// real one: this reader decodes UTF-16, so a declaration written in it has to
/// be refused by the same name rather than sailing past a byte-level check.
#[test]
fn a_declaration_in_utf_16_is_refused_by_the_same_name() {
    let text = std::str::from_utf8(BILLION_LAUGHS).expect("UTF-8");
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let (error, _, _) = first_refusal(&bytes);
    assert_eq!(error, Some(Error::DoctypeUnsupported));
}
