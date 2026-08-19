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
//!
//! **Since gap 31's milestone 2 this file has two halves, and the second is the
//! point of it.** That milestone relaxes `<!DOCTYPE` for XHTML, because 100 %
//! of one producer's EPUB 2 content documents carry one and refusing loses the
//! book. Everything above is asserted again below under
//! [`Doctype::SkipExternalId`], where the four bombs are refused by a *second*
//! name — [`Error::InternalSubset`], since all four of them live in the
//! internal subset — and `external-subset.xml`, which has no subset at all, is
//! skipped and read. A suite that only tested these files under the mode
//! nothing uses would prove nothing about the mode EPUB does.

use tinker_pdf_xml::{Doctype, Error, Limits, Source, Warning};

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

// ---------------------------------------------------------------------------
// The same five files under `Doctype::SkipExternalId`, which is the test that
// says the defence survived the relaxation.
//
// Gap 31's milestone 2 relaxes `<!DOCTYPE` for XHTML, because 100 % of one
// producer's EPUB 2 content documents carry one and refusing loses the book. A
// suite that only tested these files under `Refuse` would prove nothing about
// the mode EPUB actually uses — so every one of them is run again here, and
// the four bombs are asserted by a *second* name: `InternalSubset`, which is
// where all four of them live.
//
// The fifth file is the other direction, and it is the one that must **not**
// refuse. `external-subset.xml` is a declaration with no internal subset at
// all, naming a DTD on a host this engine could not reach; under the relaxed
// mode it is skipped, the document is read, and the identifier is named in a
// warning rather than swallowed. Four refusals with no reading beside them
// would pass on a mode that refused every declaration, which is the mode this
// milestone was written to stop being.
// ---------------------------------------------------------------------------

/// The four gap 30 names. `external-subset.xml` is deliberately not among them:
/// it is the twin for this half of the file, the way `no-doctype.xml` is the
/// twin for the other half.
const FOUR_BOMBS: [(&str, &[u8]); 4] = [
    ("billion-laughs.xml", BILLION_LAUGHS),
    ("quadratic-blowup.xml", QUADRATIC_BLOWUP),
    ("external-entity.xml", EXTERNAL_ENTITY),
    ("parameter-entity.xml", PARAMETER_ENTITY),
];

/// The first refusal under a stated mode, and how far the reader got before it.
fn first_refusal_as(bytes: &[u8], doctype: Doctype) -> (Option<Error>, usize, usize) {
    let source = Source::new(bytes).expect("all six files are UTF-8");
    let mut reader = source.reader_with(&Limits::DEFAULT, doctype);
    let mut produced = 0usize;
    for event in reader.by_ref() {
        match event {
            Ok(_) => produced += 1,
            Err(error) => return (Some(error), produced, reader.offset()),
        }
    }
    (None, produced, reader.offset())
}

/// The exit criterion: each of the four re-asserted **under the new mode**, by
/// the name that mode refuses them with.
#[test]
fn every_bomb_is_refused_as_an_internal_subset_under_the_relaxed_mode() {
    for (name, bytes) in FOUR_BOMBS {
        let (error, produced, _) = first_refusal_as(bytes, Doctype::SkipExternalId);
        assert_eq!(
            error,
            Some(Error::InternalSubset),
            "{name} was not refused as an internal subset in the relaxed mode"
        );
        // Not by a bound, for the reason the other half of this file gives:
        // a cap firing would mean the subset had been parsed and was being
        // expanded.
        assert!(
            !matches!(
                error,
                Some(Error::DepthCap | Error::AttributeCap | Error::NameCap | Error::TokenCap)
            ),
            "{name} was refused by a cap, which means the subset was parsed"
        );
        // And not by the *other* mode's name, which is what says the mode
        // argument was honoured rather than ignored. A `reader_with` that threw
        // its parameter away and always refused would pass every assertion
        // above and fail this one.
        assert_ne!(
            error,
            Some(Error::DoctypeUnsupported),
            "{name} was refused as though the relaxed mode had not been asked for"
        );
        assert_eq!(
            produced, 0,
            "{name} produced {produced} events before the subset was refused"
        );
    }
}

/// And refused **at the bracket**, with nothing inside it read.
///
/// The other half of the criterion, and the half a name alone cannot show: a
/// reader that walked the subset looking for the matching `]` and then refused
/// would answer the same variant from a different place. `billion-laughs.xml`
/// declares ten entities inside its subset and this says none of them was
/// looked at.
#[test]
fn no_bomb_is_read_into_its_internal_subset() {
    for (name, bytes) in FOUR_BOMBS {
        let text = std::str::from_utf8(bytes).expect("UTF-8");
        let bracket = text.find('[').expect("every bomb has an internal subset");
        let (_, _, offset) = first_refusal_as(bytes, Doctype::SkipExternalId);
        assert_eq!(
            offset,
            bracket,
            "{name}: the reader stopped {} bytes past the `[`",
            offset.saturating_sub(bracket)
        );
        // Nothing the subset declares was reached. The first entity name in
        // each of these is past the bracket by construction, so an offset that
        // is the bracket is an offset that read none of them.
        assert!(
            text.get(bracket..)
                .is_some_and(|subset| subset.contains("<!ENTITY")),
            "{name}: this test is asserting nothing, since the subset declares \
             no entity after the bracket"
        );
    }
}

/// The fifth file, which is the other direction: a declaration with **no**
/// internal subset is skipped, the document is read, and the identifier it
/// named is reported rather than swallowed.
///
/// Without this, the four assertions above would pass on a mode that refused
/// every document type declaration — which is the mode this milestone exists to
/// replace. `http://attacker.invalid/data.dtd` is never opened: this engine
/// performs no I/O and the literal is a string that happens to look like a URL.
#[test]
fn the_declaration_with_no_subset_is_skipped_and_its_identifier_is_named() {
    let source = Source::new(EXTERNAL_SUBSET).expect("UTF-8");
    let mut reader = source.reader_with(&Limits::DEFAULT, Doctype::SkipExternalId);
    let events: Vec<_> = reader
        .by_ref()
        .collect::<Result<Vec<_>, _>>()
        .expect("the external subset form is read, not refused");
    assert!(
        events.len() >= 3,
        "the document did not survive the skip: {} events",
        events.len()
    );
    assert_eq!(
        reader.warnings(),
        [Warning::ExternalIdentifierNotAllowed],
        "the identifier was discarded silently"
    );
    let identifier = reader
        .external_identifier()
        .expect("the declaration named one");
    assert_eq!(identifier.public(), None);
    assert_eq!(identifier.system(), "http://attacker.invalid/data.dtd");
    assert!(!identifier.is_allowed());
    // And under the shipped mode the same file is still refused by its own
    // name, which the array at the top of this file already says.
    assert_eq!(
        first_refusal_as(EXTERNAL_SUBSET, Doctype::Refuse).0,
        Some(Error::DoctypeUnsupported)
    );
}

/// The twin parses in both modes, and produces the same events in each.
///
/// The relaxation is a change to one construct or it is a change to the parser,
/// and nothing but this says which.
#[test]
fn the_bomb_shaped_file_without_a_doctype_parses_identically_in_both_modes() {
    let source = Source::new(NO_DOCTYPE).expect("UTF-8");
    let under_refuse: Vec<String> = source
        .reader(&Limits::DEFAULT)
        .map(|event| format!("{:?}", event.expect("well formed")))
        .collect();
    let under_skip: Vec<String> = source
        .reader_with(&Limits::DEFAULT, Doctype::SkipExternalId)
        .map(|event| format!("{:?}", event.expect("well formed")))
        .collect();
    assert!(under_refuse.len() > 8);
    assert_eq!(under_refuse, under_skip);
}

/// UTF-16 again, on the other side of the mode.
///
/// The relaxed mode works over decoded characters like the refusal does, so a
/// bomb written in UTF-16 reaches the same bracket and stops at it.
#[test]
fn a_bomb_in_utf_16_is_refused_as_an_internal_subset_too() {
    let text = std::str::from_utf8(BILLION_LAUGHS).expect("UTF-8");
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let (error, produced, _) = first_refusal_as(&bytes, Doctype::SkipExternalId);
    assert_eq!(error, Some(Error::InternalSubset));
    assert_eq!(produced, 0);
}

/// A declaration in the two places §2.8 has no room for one is refused in the
/// relaxed mode as well, by a name that says *where* rather than *what*.
///
/// The four bombs all sit in the prolog, so a mode that only guarded the prolog
/// would pass every assertion above. This is the same sweep the other half of
/// this file makes, run again under the mode that is allowed to skip one.
#[test]
fn a_declaration_outside_the_prolog_is_still_refused_in_the_relaxed_mode() {
    for document in [
        b"<a><!DOCTYPE a SYSTEM \"s\"></a>".as_slice(),
        b"<a/><!DOCTYPE a SYSTEM \"s\">".as_slice(),
        b"<!DOCTYPE a><!DOCTYPE a><a/>".as_slice(),
    ] {
        let (error, _, _) = first_refusal_as(document, Doctype::SkipExternalId);
        assert_eq!(
            error,
            Some(Error::MisplacedDoctype),
            "{} was not refused",
            String::from_utf8_lossy(document)
        );
    }
    // And the one place there is room for one is not refused, so the three
    // above are about position.
    assert_eq!(
        first_refusal_as(b"<!DOCTYPE a SYSTEM \"s\"><a/>", Doctype::SkipExternalId).0,
        None
    );
}
