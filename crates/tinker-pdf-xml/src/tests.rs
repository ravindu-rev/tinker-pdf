//! The parser, against what it must read and what it must refuse.
//!
//! The discipline is `tinker-pdf-zip`'s and the reason is gap 29's milestone 5:
//! truncating PNG's eight-byte signature to four and JPEG's three to two both
//! survived a suite that asserted every *correct* signature is recognised. A
//! positive assertion cannot catch a weakened check. So for every rule below
//! there is a near miss beside it — `<!doctype` in the wrong case, `<![cdata[`
//! in the wrong case, `&#xD800;`, `<a b="1"c="2"/>`, an end tag whose prefix
//! differs from its start tag's — and the near miss is asserted to be refused
//! **by its own name**.
//!
//! Two shapes recur. `read` hands a closure the events while the decoded text
//! is still alive, because a [`Source`] owns the buffer its events borrow from;
//! `refusal` streams rather than collecting, so the test that crosses the
//! million-event cap costs a constant amount of memory rather than a hundred
//! megabytes of it.

use std::borrow::Cow;

use crate::limits::{MAX_XML_ATTRIBUTES, MAX_XML_DEPTH, MAX_XML_NAME_LEN, MAX_XML_TOKENS};
use crate::{Construct, Encoding, Error, Event, Limits, Reader, Source, Warning, XML_NAMESPACE};

/// Parses under the shipped bounds and hands the events to `check`.
fn read(bytes: &[u8], check: impl FnOnce(&[Event<'_>])) {
    let source = Source::new(bytes).expect("decodes");
    let events: Vec<Event<'_>> = source
        .reader(&Limits::DEFAULT)
        .collect::<Result<_, _>>()
        .expect("well formed");
    check(&events);
}

/// The same, with the reader afterwards, for the tests that assert warnings.
fn read_with_reader(bytes: &[u8], check: impl FnOnce(&[Event<'_>], &Reader<'_>)) {
    let source = Source::new(bytes).expect("decodes");
    let mut reader = source.reader(&Limits::DEFAULT);
    let events: Vec<Event<'_>> = reader
        .by_ref()
        .collect::<Result<_, _>>()
        .expect("well formed");
    check(&events, &reader);
}

/// The refusal a document produced, streamed rather than collected.
fn refusal(bytes: &[u8]) -> Error {
    refusal_within(bytes, &Limits::DEFAULT)
}

fn refusal_within(bytes: &[u8], limits: &Limits) -> Error {
    match Source::new(bytes) {
        Err(error) => error,
        Ok(source) => {
            let mut produced = 0usize;
            for event in source.reader(limits) {
                match event {
                    Ok(_) => produced += 1,
                    Err(error) => return error,
                }
            }
            panic!("expected a refusal, got {produced} events");
        }
    }
}

/// How many events, under the given bounds, or the refusal.
fn count_within(bytes: &[u8], limits: &Limits) -> Result<usize, Error> {
    let source = Source::new(bytes)?;
    let mut produced = 0usize;
    for event in source.reader(limits) {
        event?;
        produced += 1;
    }
    Ok(produced)
}

fn count(bytes: &[u8]) -> usize {
    count_within(bytes, &Limits::DEFAULT).expect("well formed")
}

fn starts(bytes: &[u8]) -> Vec<String> {
    let source = Source::new(bytes).expect("decodes");
    let mut names = Vec::new();
    for event in source.reader(&Limits::DEFAULT) {
        if let Event::Start(element) = event.expect("well formed") {
            names.push(element.name().qualified().to_string());
        }
    }
    names
}

fn text_of(bytes: &[u8]) -> String {
    let source = Source::new(bytes).expect("decodes");
    let mut out = String::new();
    for event in source.reader(&Limits::DEFAULT) {
        match event.expect("well formed") {
            Event::Text(text) | Event::Cdata(text) => out.push_str(&text),
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// The refusal this whole crate exists for.
// ---------------------------------------------------------------------------

/// The exit criterion, in the form it is written in: refused **by its own
/// name**, and refused *before one byte after it is read*.
///
/// The offset is the assertion that makes the second half checkable. A reader
/// that consumed `<!DOCTYPE` and then refused would leave the cursor nine bytes
/// further on, and every other test in this file would pass either way.
#[test]
fn a_doctype_is_refused_by_name_and_the_cursor_does_not_move_past_it() {
    let bytes = b"<?xml version=\"1.0\"?>\n<!DOCTYPE page SYSTEM \"page.dtd\">\n<page/>";
    let source = Source::new(bytes).expect("decodes");
    let mut reader = source.reader(&Limits::DEFAULT);
    assert_eq!(reader.next(), Some(Err(Error::DoctypeUnsupported)));
    assert_eq!(
        reader.offset(),
        22,
        "the reader read past `<!DOCTYPE` before refusing it"
    );
    assert_eq!(
        source.text().get(reader.offset()..reader.offset() + 9),
        Some("<!DOCTYPE"),
        "the offset is not the start of the declaration"
    );
    // And it is fused: asking again does not try the declaration a second time.
    assert_eq!(reader.next(), None);
}

/// The same refusal after the root, which is the one place a reader that only
/// guarded the prolog would let a declaration through.
#[test]
fn a_doctype_after_the_root_is_refused_by_the_same_name() {
    assert_eq!(
        refusal(b"<page/><!DOCTYPE page [<!ENTITY a \"b\">]>"),
        Error::DoctypeUnsupported
    );
}

/// Inside content, where a reader dispatching on `<!` alone would fall into the
/// CDATA branch.
#[test]
fn a_doctype_inside_content_is_refused_by_the_same_name() {
    assert_eq!(
        refusal(b"<page><!DOCTYPE x></page>"),
        Error::DoctypeUnsupported
    );
}

/// The near miss. `<!doctype` is not `<!DOCTYPE` and XML is case-sensitive, so
/// it must not be *silently* accepted as one: it is a markup declaration nobody
/// may write, which is a different refusal and the honest one.
#[test]
fn a_lower_case_doctype_is_refused_as_a_markup_declaration_not_as_a_doctype() {
    assert_eq!(refusal(b"<!doctype x><page/>"), Error::MarkupDeclaration);
}

#[test]
fn the_four_markup_declarations_are_refused_by_their_own_name() {
    for bytes in [
        b"<!ENTITY a \"b\"><page/>".as_slice(),
        b"<!ELEMENT page EMPTY><page/>".as_slice(),
        b"<!ATTLIST page a CDATA #IMPLIED><page/>".as_slice(),
        b"<!NOTATION gif SYSTEM \"gif\"><page/>".as_slice(),
    ] {
        assert_eq!(refusal(bytes), Error::MarkupDeclaration);
    }
}

// ---------------------------------------------------------------------------
// Entities and character references: exactly one character each, or nothing.
// ---------------------------------------------------------------------------

#[test]
fn the_five_predefined_entities_are_the_five_and_no_others() {
    assert_eq!(
        text_of(b"<a>&lt;&gt;&amp;&apos;&quot;</a>"),
        "<>&'\"",
        "the five predefined entities did not decode"
    );
    assert_eq!(refusal(b"<a>&nbsp;</a>"), Error::UnknownEntity);
    assert_eq!(refusal(b"<a>&foo bar;</a>"), Error::UnknownEntity);
    assert_eq!(
        refusal(b"<a>a & b</a>"),
        Error::Unterminated(Construct::Reference)
    );
}

#[test]
fn numeric_character_references_are_read_in_both_radixes() {
    assert_eq!(text_of(b"<a>&#65;&#x42;&#x63;&#100;</a>"), "ABcd");
    assert_eq!(
        text_of("<a>&#x2014;&#8212;</a>".as_bytes()),
        "\u{2014}\u{2014}"
    );
    assert_eq!(text_of(b"<a>&#x10FFFD;</a>"), "\u{10FFFD}");
    // Both cases of the radix marker, and leading zeroes.
    assert_eq!(text_of(b"<a>&#X41;&#x0041;&#0065;</a>"), "AAA");
}

/// The injection this exists for is a reference check that hands the number to
/// `char::from_u32` and stops there. That catches the surrogates and the
/// out-of-range value and **not** `&#0;` or `&#xFFFE;`, which are perfectly
/// good scalar values and are not characters (§2.2).
#[test]
fn a_character_reference_naming_no_character_is_refused_by_name() {
    for bytes in [
        b"<a>&#xD800;</a>".as_slice(),                // a lone high surrogate
        b"<a>&#xDFFF;</a>".as_slice(),                // a lone low surrogate
        b"<a>&#x110000;</a>".as_slice(),              // past the last code point
        b"<a>&#0;</a>".as_slice(),                    // a scalar value, and not a character
        b"<a>&#xFFFE;</a>".as_slice(),                // likewise
        b"<a>&#x1;</a>".as_slice(),                   // a C0 control that is not tab or newline
        b"<a>&#99999999999999999999;</a>".as_slice(), // wider than a u32
        b"<a>&#;</a>".as_slice(),
        b"<a>&#x;</a>".as_slice(),
        b"<a>&#xZZ;</a>".as_slice(),
        b"<a b=\"&#xD800;\"/>".as_slice(), // and in an attribute value too
    ] {
        assert_eq!(
            refusal(bytes),
            Error::BadCharacterReference,
            "{} was not refused as a bad character reference",
            String::from_utf8_lossy(bytes)
        );
    }
}

/// The property the DTD refusal rests on, asserted rather than described: no
/// reference this reader admits produces more bytes than it spends.
#[test]
fn no_reference_this_reader_admits_can_expand() {
    for reference in [
        "&lt;",
        "&gt;",
        "&amp;",
        "&apos;",
        "&quot;",
        "&#9;",
        "&#x10FFFD;",
        "&#1114109;",
    ] {
        let document = format!("<a>{reference}</a>");
        let decoded = text_of(document.as_bytes());
        assert!(
            decoded.len() <= reference.len(),
            "{reference} decoded to {} bytes from {}",
            decoded.len(),
            reference.len()
        );
        assert_eq!(
            decoded.chars().count(),
            1,
            "{reference} is not exactly one character"
        );
    }
}

// ---------------------------------------------------------------------------
// Encodings.
// ---------------------------------------------------------------------------

#[test]
fn utf_8_is_read_with_a_byte_order_mark_and_without_one() {
    for bytes in [b"\xEF\xBB\xBF<page/>".as_slice(), b"<page/>".as_slice()] {
        let source = Source::new(bytes).expect("decodes");
        assert_eq!(source.encoding(), Encoding::Utf8);
        assert_eq!(source.text(), "<page/>", "the mark reached the text");
        assert!(source.warnings().is_empty());
    }
}

#[test]
fn utf_16_is_read_in_both_byte_orders_from_its_mark() {
    let text = "<page a=\"\u{2014}\"/>";
    let mut little = vec![0xFF, 0xFE];
    let mut big = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        little.extend_from_slice(&unit.to_le_bytes());
        big.extend_from_slice(&unit.to_be_bytes());
    }
    for (bytes, expected) in [
        (little, Encoding::Utf16LittleEndian),
        (big, Encoding::Utf16BigEndian),
    ] {
        let source = Source::new(&bytes).expect("decodes");
        assert_eq!(source.encoding(), expected);
        assert_eq!(source.text(), text);
        assert!(source.warnings().is_empty(), "a marked entity warned");
    }
}

#[test]
fn a_surrogate_pair_decodes_to_one_character() {
    let text = "<a>\u{1F600}</a>";
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let source = Source::new(&bytes).expect("decodes");
    assert_eq!(source.text(), text);
}

#[test]
fn utf_16_without_a_mark_is_read_and_warned_about() {
    let mut bytes = Vec::new();
    for unit in "<page/>".encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let source = Source::new(&bytes).expect("decodes");
    assert_eq!(source.encoding(), Encoding::Utf16LittleEndian);
    assert_eq!(source.warnings(), [Warning::Utf16WithoutByteOrderMark]);
    // And the warning survives into the reader, so a caller reading events
    // never has two lists to merge.
    assert_eq!(
        source.reader(&Limits::DEFAULT).warnings(),
        [Warning::Utf16WithoutByteOrderMark]
    );
}

#[test]
fn a_surrogate_that_is_not_half_of_a_pair_refuses_the_input() {
    // FF FE marks UTF-16LE; D800 with nothing after it is half a character.
    assert_eq!(
        Source::new(&[0xFF, 0xFE, 0x00, 0xD8]).err(),
        Some(Error::NotUtf16)
    );
    // An odd length cannot be UTF-16 at all.
    assert_eq!(
        Source::new(&[0xFF, 0xFE, 0x3C]).err(),
        Some(Error::NotUtf16)
    );
    // A low surrogate with no high one in front of it.
    assert_eq!(
        Source::new(&[0xFE, 0xFF, 0xDC, 0x00, 0x00, 0x3C]).err(),
        Some(Error::NotUtf16)
    );
}

/// A UTF-32 mark begins with the same two bytes as a UTF-16 one, so the near
/// miss here is a reader that tests for UTF-16 first and then decodes NULs.
#[test]
fn a_utf_32_byte_order_mark_is_refused_as_an_encoding_rather_than_read_as_utf_16() {
    assert_eq!(
        Source::new(&[0xFF, 0xFE, 0x00, 0x00, 0x3C, 0x00, 0x00, 0x00]).err(),
        Some(Error::UnsupportedEncoding)
    );
    assert_eq!(
        Source::new(&[0x00, 0x00, 0xFE, 0xFF, 0x00, 0x00, 0x00, 0x3C]).err(),
        Some(Error::UnsupportedEncoding)
    );
}

#[test]
fn an_encoding_this_reader_does_not_decode_is_refused_by_name() {
    assert_eq!(
        refusal(b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><page/>"),
        Error::UnsupportedEncoding
    );
}

#[test]
fn the_encoding_declaration_is_read_case_insensitively_in_both_spellings() {
    // Both appear in one corpus from one vendor: WPF writes `utf-8` and the XPS
    // object model writes `UTF-8`.
    for bytes in [
        b"<?xml version=\"1.0\" encoding=\"utf-8\"?><page/>".as_slice(),
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><page/>".as_slice(),
    ] {
        read_with_reader(bytes, |events, reader| {
            assert_eq!(events.len(), 2);
            assert!(
                reader.warnings().is_empty(),
                "a matching declaration warned"
            );
        });
    }
}

#[test]
fn a_declaration_that_disagrees_with_the_bytes_warns_and_the_bytes_win() {
    let bytes = b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"UTF-16\"?><page/>";
    read_with_reader(bytes, |events, reader| {
        assert_eq!(events.len(), 2, "the document did not parse");
        assert_eq!(reader.warnings(), [Warning::EncodingDeclarationIgnored]);
    });
    assert_eq!(
        Source::new(bytes).expect("decodes").encoding(),
        Encoding::Utf8
    );
}

#[test]
fn an_xml_version_other_than_one_point_zero_is_refused_by_name() {
    assert_eq!(
        refusal(b"<?xml version=\"1.1\"?><page/>"),
        Error::UnsupportedVersion
    );
}

/// Milestone 1 found four spellings of the prolog across eight real files, one
/// of which is no prolog at all — WPF's `.fpage`, `.fdoc` and `.fdseq` begin
/// directly with their root element.
#[test]
fn every_prolog_spelling_the_corpus_holds_is_read() {
    for bytes in [
        b"<page/>".as_slice(),
        b"\xEF\xBB\xBF<page/>".as_slice(),
        b"<?xml version=\"1.0\"?><page/>".as_slice(),
        b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><page/>".as_slice(),
        b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"utf-8\"?><page/>".as_slice(),
        b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><page/>".as_slice(),
    ] {
        assert_eq!(
            starts(bytes),
            ["page"],
            "{} did not parse",
            String::from_utf8_lossy(bytes)
        );
    }
}

#[test]
fn a_malformed_declaration_is_refused_by_name() {
    for bytes in [
        b"<?xml?><page/>".as_slice(),
        b"<?xml version=1.0?><page/>".as_slice(),
        b"<?xml encoding=\"UTF-8\"?><page/>".as_slice(),
        b"<?xml version=\"1.0\" standalone=\"maybe\"?><page/>".as_slice(),
        b"<?xml version=\"1.0\" standalone=\"yes\" encoding=\"UTF-8\"?><page/>".as_slice(),
    ] {
        assert_eq!(
            refusal(bytes),
            Error::MalformedDeclaration,
            "{} was not refused",
            String::from_utf8_lossy(bytes)
        );
    }
    assert_eq!(
        refusal(b"<?xml version=\"1.0\""),
        Error::Unterminated(Construct::Declaration)
    );
}

// ---------------------------------------------------------------------------
// Characters.
// ---------------------------------------------------------------------------

#[test]
fn a_character_xml_does_not_admit_refuses_the_whole_input() {
    // Anywhere: in text, in a name, in an attribute value, in a comment.
    for bytes in [
        b"<a>\x00</a>".as_slice(),
        b"<a\x0Bb=\"1\"/>".as_slice(),
        b"<a b=\"\x0C\"/>".as_slice(),
        b"<a><!-- \x07 --></a>".as_slice(),
        "<a>\u{FFFF}</a>".as_bytes(),
    ] {
        assert_eq!(
            Source::new(bytes).err(),
            Some(Error::IllegalCharacter),
            "{} was admitted",
            String::from_utf8_lossy(bytes)
        );
    }
    // And the three C0 characters that are characters.
    assert_eq!(text_of(b"<a>\t\r\n</a>"), "\t\n");
}

#[test]
fn bytes_that_are_not_utf_8_are_refused_as_such() {
    assert_eq!(Source::new(b"<a>\xFF\xFF</a>").err(), Some(Error::NotUtf8));
}

// ---------------------------------------------------------------------------
// Elements, attributes, and the shapes that are not them.
// ---------------------------------------------------------------------------

#[test]
fn an_empty_element_tag_produces_a_start_and_an_end() {
    read(b"<a><b/></a>", |events| {
        assert_eq!(events.len(), 4);
        match (&events[1], &events[2]) {
            (Event::Start(start), Event::End(end)) => {
                assert_eq!(start.local(), "b");
                assert_eq!(end.local(), "b");
                assert!(start.was_empty(), "the source spelling was lost");
            }
            other => panic!("{other:?}"),
        }
    });
}

#[test]
fn attributes_are_read_in_source_order_with_their_values_decoded() {
    read(b"<a x=\"1\" y='2 &amp; 3' z=\"\"/>", |events| {
        let Event::Start(element) = &events[0] else {
            panic!("not a start")
        };
        let names: Vec<_> = element.attributes().iter().map(|a| a.local()).collect();
        assert_eq!(names, ["x", "y", "z"]);
        assert_eq!(element.attribute(None, "y"), Some("2 & 3"));
        assert_eq!(element.attribute(None, "z"), Some(""));
        assert_eq!(element.attribute(None, "missing"), None);
    });
}

/// §3.3.3: a *literal* tab, newline or carriage return in an attribute value is
/// a space; one that arrived as a character reference is not. Both halves,
/// because a reader that normalised everything would pass a test that only
/// checked the first.
#[test]
fn an_attribute_value_is_normalised_and_a_reference_survives_it() {
    read(
        b"<a x=\"one\ttwo\r\nthree\" y=\"one&#x9;two&#xA;three\"/>",
        |events| {
            let Event::Start(element) = &events[0] else {
                panic!("not a start")
            };
            assert_eq!(element.attribute(None, "x"), Some("one two three"));
            assert_eq!(element.attribute(None, "y"), Some("one\ttwo\nthree"));
        },
    );
}

/// A real fixed page from `wpf-tiled-brush.xps` holds CRLF between elements, so
/// this is not a hypothetical: §2.11's normalisation runs on text too.
#[test]
fn line_ends_are_normalised_in_text_and_a_reference_survives_it() {
    assert_eq!(
        text_of(b"<a>one\r\ntwo\rthree\nfour</a>"),
        "one\ntwo\nthree\nfour"
    );
    assert_eq!(text_of(b"<a>one&#xD;two</a>"), "one\rtwo");
    assert_eq!(text_of(b"<a><![CDATA[one\r\ntwo]]></a>"), "one\ntwo");
}

#[test]
fn a_mismatched_end_tag_is_refused_by_name() {
    assert_eq!(refusal(b"<a><b></a></b>"), Error::MismatchedEndTag);
    assert_eq!(refusal(b"<a></b>"), Error::MismatchedEndTag);
    assert_eq!(refusal(b"</a>"), Error::MismatchedEndTag);
    // XML 1.0 §3 wants the *spelling* repeated, prefix and all, even when both
    // spellings resolve to the same expanded name.
    assert_eq!(
        refusal(b"<p:a xmlns:p=\"u\" xmlns:q=\"u\"></q:a>"),
        Error::MismatchedEndTag
    );
}

#[test]
fn a_duplicate_attribute_is_refused_by_name() {
    assert_eq!(refusal(b"<a x=\"1\" x=\"2\"/>"), Error::DuplicateAttribute);
    assert_eq!(
        refusal(b"<a xmlns:p=\"u\" xmlns:p=\"v\"/>"),
        Error::DuplicateAttribute
    );
}

/// **Namespaces §5.3, which a raw-name comparison misses entirely.** Two
/// prefixes bound to one namespace are one attribute name, and the injection
/// this catches is a duplicate check written against the spelling.
#[test]
fn two_prefixes_bound_to_one_namespace_are_one_attribute_name() {
    assert_eq!(
        refusal(b"<a xmlns:p=\"u\" xmlns:q=\"u\" p:x=\"1\" q:x=\"2\"/>"),
        Error::DuplicateAttribute
    );
    // And the other direction, which says the check is not simply refusing
    // everything: an unprefixed attribute is in **no** namespace, so it never
    // collides with a prefixed one.
    read(
        b"<a xmlns=\"u\" xmlns:p=\"u\" p:x=\"1\" x=\"2\"/>",
        |events| {
            let Event::Start(element) = &events[0] else {
                panic!("not a start")
            };
            assert_eq!(element.attribute(Some("u"), "x"), Some("1"));
            assert_eq!(element.attribute(None, "x"), Some("2"));
        },
    );
}

#[test]
fn an_undeclared_prefix_is_refused_by_name() {
    assert_eq!(refusal(b"<p:a/>"), Error::UndeclaredPrefix);
    assert_eq!(refusal(b"<a p:x=\"1\"/>"), Error::UndeclaredPrefix);
}

/// The scope test, in both directions. A prefix declared on an inner element is
/// gone when that element closes, and a prefix it shadowed is back.
#[test]
fn a_prefix_is_resolved_from_its_own_scope_and_unbound_when_that_scope_closes() {
    read(
        b"<r xmlns:p=\"outer\"><m xmlns:p=\"inner\"><p:a/></m><p:b/></r>",
        |events| {
            let namespaces: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Start(element) => {
                        Some((element.local(), element.namespace().map(str::to_string)))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(
                namespaces,
                [
                    ("r", None),
                    ("m", None),
                    ("a", Some("inner".to_string())),
                    ("b", Some("outer".to_string())),
                ],
                "a prefix resolved from the wrong scope"
            );
        },
    );

    // And once the declaring element closes, the prefix is gone entirely.
    assert_eq!(
        refusal(b"<r><m xmlns:p=\"inner\"><p:a/></m><p:b/></r>"),
        Error::UndeclaredPrefix
    );
}

#[test]
fn the_default_namespace_applies_to_elements_and_never_to_attributes() {
    read(b"<a xmlns=\"u\" x=\"1\"/>", |events| {
        let Event::Start(element) = &events[0] else {
            panic!("not a start")
        };
        assert_eq!(element.namespace(), Some("u"));
        assert_eq!(
            element.attributes()[0].namespace(),
            None,
            "the default namespace was applied to an attribute"
        );
    });
}

#[test]
fn the_default_namespace_can_be_turned_off_without_a_warning() {
    read_with_reader(b"<a xmlns=\"u\"><b xmlns=\"\"/></a>", |events, reader| {
        let namespaces: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Start(element) => Some(element.namespace().map(str::to_string)),
                _ => None,
            })
            .collect();
        assert_eq!(namespaces, [Some("u".to_string()), None]);
        assert!(reader.warnings().is_empty());
    });
}

#[test]
fn undeclaring_a_prefix_is_taken_as_namespaces_one_point_one_meant_it_and_warns() {
    read_with_reader(
        b"<r xmlns:p=\"u\"><m xmlns:p=\"\"><a/></m></r>",
        |events, reader| {
            assert_eq!(events.len(), 6);
            assert_eq!(reader.warnings(), [Warning::PrefixUndeclared]);
        },
    );
    assert_eq!(
        refusal(b"<r xmlns:p=\"u\"><m xmlns:p=\"\"><p:a/></m></r>"),
        Error::UndeclaredPrefix
    );
}

#[test]
fn the_xml_prefix_is_bound_without_a_declaration_and_lang_is_an_ordinary_attribute() {
    read(
        b"<a xml:lang=\"en-us\" xml:space=\"preserve\"/>",
        |events| {
            let Event::Start(element) = &events[0] else {
                panic!("not a start")
            };
            assert_eq!(
                element.attributes().len(),
                2,
                "an xml: attribute was consumed rather than passed through"
            );
            assert_eq!(
                element.attribute(Some(XML_NAMESPACE), "lang"),
                Some("en-us")
            );
            assert_eq!(
                element.attribute(Some(XML_NAMESPACE), "space"),
                Some("preserve"),
                "xml:space was acted on rather than passed through"
            );
        },
    );
    // And acted on nowhere: `xml:space="preserve"` changes no event.
    assert_eq!(count(b"<a xml:space=\"preserve\"> </a>"), 3);
    assert_eq!(count(b"<a xml:space=\"default\"> </a>"), 3);
}

#[test]
fn the_namespaces_xml_reserves_are_refused_by_name() {
    for bytes in [
        b"<a xmlns:xmlns=\"http://www.w3.org/2000/xmlns/\"/>".as_slice(),
        b"<a xmlns:xml=\"http://example.invalid/\"/>".as_slice(),
        b"<a xmlns:p=\"http://www.w3.org/XML/1998/namespace\"/>".as_slice(),
        b"<a xmlns=\"http://www.w3.org/2000/xmlns/\"/>".as_slice(),
        b"<a xmlns:xml=\"\"/>".as_slice(),
        b"<a xmlns:p=\"u\" xmlns:x=\"u\"><xmlns:b/></a>".as_slice(),
    ] {
        assert_eq!(
            refusal(bytes),
            Error::ReservedNamespace,
            "{} was admitted",
            String::from_utf8_lossy(bytes)
        );
    }
    // Declaring `xml` to its own namespace is legal and is not a warning.
    assert_eq!(
        starts(b"<a xmlns:xml=\"http://www.w3.org/XML/1998/namespace\"/>"),
        ["a"]
    );
}

#[test]
fn namespace_declarations_are_not_reported_as_attributes() {
    read(b"<a xmlns=\"u\" xmlns:p=\"v\" x=\"1\"/>", |events| {
        let Event::Start(element) = &events[0] else {
            panic!("not a start")
        };
        assert_eq!(element.attributes().len(), 1);
        assert_eq!(element.attributes()[0].local(), "x");
    });
}

#[test]
fn a_malformed_tag_is_refused_by_the_name_of_what_is_wrong_with_it() {
    // No whitespace between two attributes, which is the shape that otherwise
    // reads as one attribute with a longer name.
    assert_eq!(refusal(b"<a b=\"1\"c=\"2\"/>"), Error::MalformedTag);
    assert_eq!(refusal(b"<a b/>"), Error::ExpectedEquals);
    assert_eq!(refusal(b"<a b=1/>"), Error::UnquotedAttributeValue);
    assert_eq!(
        refusal(b"<a b=\"1\"><c d=\"<e>\"/></a>"),
        Error::LessThanInAttributeValue
    );
    assert_eq!(refusal(b"<1a/>"), Error::MalformedName);
    assert_eq!(refusal(b"<a::b/>"), Error::MalformedName);
    assert_eq!(refusal(b"<:a/>"), Error::MalformedName);
    assert_eq!(refusal(b"<a:/>"), Error::MalformedName);
    assert_eq!(refusal(b"<a></a b>"), Error::MalformedTag);
}

/// A dot is a `NameChar`, and every real XPS package leans on it:
/// `FixedPage.Resources` and `LinearGradientBrush.GradientStops` are element
/// names. A scanner that stopped at the dot would read half of each.
#[test]
fn a_dot_is_part_of_a_name_and_not_the_end_of_one() {
    assert_eq!(
        starts(b"<FixedPage.Resources><LinearGradientBrush.GradientStops/></FixedPage.Resources>"),
        ["FixedPage.Resources", "LinearGradientBrush.GradientStops"]
    );
}

#[test]
fn an_unterminated_construct_says_which_construct() {
    assert_eq!(refusal(b"<a>"), Error::Unterminated(Construct::Element));
    assert_eq!(refusal(b"<a b=\"1\""), Error::Unterminated(Construct::Tag));
    assert_eq!(
        refusal(b"<a b=\"1"),
        Error::Unterminated(Construct::AttributeValue)
    );
    assert_eq!(
        refusal(b"<a><!-- x </a>"),
        Error::Unterminated(Construct::Comment)
    );
    assert_eq!(
        refusal(b"<a><![CDATA[x</a>"),
        Error::Unterminated(Construct::CdataSection)
    );
    assert_eq!(
        refusal(b"<a><?target x</a>"),
        Error::Unterminated(Construct::Instruction)
    );
    assert_eq!(
        refusal(b"<a>&amp</a>"),
        Error::Unterminated(Construct::Reference)
    );
}

#[test]
fn a_document_with_no_root_or_two_is_refused_by_name() {
    assert_eq!(refusal(b""), Error::NoRootElement);
    assert_eq!(
        refusal(b"<?xml version=\"1.0\"?>  <!-- x -->  "),
        Error::NoRootElement
    );
    assert_eq!(refusal(b"<a/><b/>"), Error::ContentAfterRoot);
    assert_eq!(refusal(b"<a/>tail"), Error::ContentAfterRoot);
    assert_eq!(refusal(b"lead<a/>"), Error::TextBeforeRoot);
    assert_eq!(refusal(b"<![CDATA[x]]><a/>"), Error::TextBeforeRoot);
}

// ---------------------------------------------------------------------------
// Comments, processing instructions, CDATA.
// ---------------------------------------------------------------------------

/// Milestone 1 found a comment **inside element content** on the first real
/// file — between `<FixedPage>` and `<FixedPage.Resources>` — rather than in
/// the prolog, where a hand-written fixture would have put it.
#[test]
fn a_comment_is_read_in_the_prolog_in_content_and_after_the_root() {
    read(
        b"<!--before--><a><!--inside--><b/></a><!--after-->",
        |events| {
            let comments: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    Event::Comment(text) => Some(text.as_ref()),
                    _ => None,
                })
                .collect();
            assert_eq!(comments, ["before", "inside", "after"]);
        },
    );
}

#[test]
fn a_double_hyphen_inside_a_comment_warns_and_the_comment_still_ends_where_it_ends() {
    read_with_reader(b"<a><!-- a -- b --><b/></a>", |events, reader| {
        assert_eq!(events.len(), 5);
        assert_eq!(reader.warnings(), [Warning::DoubleHyphenInComment]);
    });
    // And an ordinary comment does not warn, which is the half that says the
    // check is looking at something.
    read_with_reader(b"<a><!-- a b --></a>", |_, reader| {
        assert!(reader.warnings().is_empty());
    });
}

#[test]
fn a_processing_instruction_keeps_its_target_and_its_value() {
    read(b"<?stylesheet href=\"a.css\"?><a><?bare?></a>", |events| {
        let instructions: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Instruction { target, value } => Some((*target, value.as_ref())),
                _ => None,
            })
            .collect();
        assert_eq!(
            instructions,
            [("stylesheet", "href=\"a.css\""), ("bare", "")]
        );
    });
}

#[test]
fn a_processing_instruction_targeting_xml_is_refused_by_name() {
    assert_eq!(
        refusal(b"<a><?xml version=\"1.0\"?></a>"),
        Error::ReservedTarget
    );
    assert_eq!(refusal(b"<a><?XmL x?></a>"), Error::ReservedTarget);
    // And a target that merely begins with those letters is an ordinary one.
    assert_eq!(count(b"<a><?xml-stylesheet x?></a>"), 3);
}

/// The injection this is for is a CDATA terminator matched on `]` or `]]`
/// rather than on the whole of `]]>`, which reads `x]]b` as `x`.
#[test]
fn a_cdata_section_ends_at_its_own_terminator_and_not_before_it() {
    assert_eq!(text_of(b"<a><![CDATA[x]]b]]></a>"), "x]]b");
    assert_eq!(text_of(b"<a><![CDATA[<b/>&amp;]]></a>"), "<b/>&amp;");
    assert_eq!(text_of(b"<a><![CDATA[]]></a>"), "");
    // A CDATA section is its own event, because the source said something
    // different from what plain text would have said.
    read(b"<a>t<![CDATA[c]]></a>", |events| {
        assert!(matches!(events[1], Event::Text(_)));
        assert!(matches!(events[2], Event::Cdata(_)));
    });
}

/// XML is case-sensitive and `<![cdata[` is not a CDATA section. It must not be
/// quietly read as one — the near miss a case-insensitive match lets through.
#[test]
fn a_lower_case_cdata_opening_is_not_a_cdata_section() {
    assert_eq!(refusal(b"<a><![cdata[x]]></a>"), Error::MarkupDeclaration);
}

#[test]
fn a_cdata_close_in_ordinary_text_warns_and_the_text_is_kept() {
    read_with_reader(b"<a>x]]>y</a>", |events, reader| {
        assert_eq!(events.len(), 3);
        assert!(matches!(&events[1], Event::Text(t) if t == "x]]>y"));
        assert_eq!(reader.warnings(), [Warning::CdataCloseInContent]);
    });
}

#[test]
fn inter_element_whitespace_arrives_as_text_rather_than_being_guessed_about() {
    // WPF writes newlines and four-space indentation inside
    // `FixedPage.Resources`, with no `xml:space`, so this is what a real fixed
    // page produces and nothing here decides it is insignificant.
    read(b"<a>\n    <b/>\n</a>", |events| {
        assert_eq!(events.len(), 6);
        assert!(matches!(&events[1], Event::Text(t) if t == "\n    "));
    });
}

// ---------------------------------------------------------------------------
// Borrowing, which is what the pull shape is for.
// ---------------------------------------------------------------------------

#[test]
fn text_that_needs_no_decoding_is_borrowed_and_text_that_does_is_owned() {
    read(b"<a>plain</a>", |events| {
        assert!(matches!(&events[1], Event::Text(Cow::Borrowed(_))));
    });
    read(b"<a>a&amp;b</a>", |events| {
        assert!(matches!(&events[1], Event::Text(Cow::Owned(_))));
    });
}

// ---------------------------------------------------------------------------
// The bounds. Each fires at the shipped constant, by its own refusal.
// ---------------------------------------------------------------------------

/// Proves [`MAX_XML_DEPTH`] fires, by [`Error::DepthCap`] and not by a clock.
///
/// And the other direction in the same test, because the injection that matters
/// is a depth cap counting the wrong thing: 300 *siblings* at depth one must
/// not trip a cap of 256.
#[test]
fn nesting_past_the_depth_cap_is_refused_by_name() {
    let deep = "<a>".repeat(MAX_XML_DEPTH + 1);
    assert_eq!(refusal(deep.as_bytes()), Error::DepthCap);

    let allowed = format!(
        "{}{}",
        "<a>".repeat(MAX_XML_DEPTH),
        "</a>".repeat(MAX_XML_DEPTH)
    );
    assert_eq!(
        count(allowed.as_bytes()),
        MAX_XML_DEPTH * 2,
        "the cap refuses what it says it allows"
    );

    let wide = format!("<r>{}</r>", "<a/>".repeat(MAX_XML_DEPTH + 44));
    assert_eq!(count(wide.as_bytes()), 2 + (MAX_XML_DEPTH + 44) * 2);
}

/// Proves [`MAX_XML_ATTRIBUTES`] fires, by [`Error::AttributeCap`].
#[test]
fn more_attributes_than_the_cap_is_refused_by_name() {
    let mut past = String::from("<a");
    for index in 0..=MAX_XML_ATTRIBUTES {
        past.push_str(&format!(" a{index}=\"1\""));
    }
    past.push_str("/>");
    assert_eq!(refusal(past.as_bytes()), Error::AttributeCap);

    // The cap allows exactly what it says, which is the half that would still
    // pass if the comparison were off by one in the other direction.
    let mut allowed = String::from("<a");
    for index in 0..MAX_XML_ATTRIBUTES {
        allowed.push_str(&format!(" a{index}=\"1\""));
    }
    allowed.push_str("/>");
    read(allowed.as_bytes(), |events| {
        let Event::Start(element) = &events[0] else {
            panic!("not a start")
        };
        assert_eq!(element.attributes().len(), MAX_XML_ATTRIBUTES);
    });
}

/// Proves [`MAX_XML_NAME_LEN`] fires, by [`Error::NameCap`] — and that it
/// **refuses** rather than truncating, which is the whole difference between
/// this cap and `tinker_pdf_zip::limits::MAX_ZIP_NAME_LEN`.
#[test]
fn a_name_past_the_cap_is_refused_rather_than_truncated() {
    let long = "n".repeat(MAX_XML_NAME_LEN + 1);
    assert_eq!(
        refusal(format!("<{long}/>").as_bytes()),
        Error::NameCap,
        "an over-long element name was not refused"
    );
    assert_eq!(
        refusal(format!("<a {long}=\"1\"/>").as_bytes()),
        Error::NameCap,
        "an over-long attribute name was not refused"
    );

    let allowed = "n".repeat(MAX_XML_NAME_LEN);
    assert_eq!(
        starts(format!("<{allowed}/>").as_bytes()),
        [allowed.as_str()],
        "the name came back truncated"
    );
}

/// Proves [`MAX_XML_TOKENS`] fires, by [`Error::TokenCap`], at the shipped
/// constant rather than a lowered one — because a work cap proved only against
/// a lowered limit is a work cap whose shipped number nobody has checked.
#[test]
fn more_events_than_the_token_cap_is_refused_by_name() {
    // `<a/>` is two events, so half the cap of them, plus the root, crosses it.
    let mut document = String::with_capacity(MAX_XML_TOKENS * 2 + 16);
    document.push_str("<r>");
    for _ in 0..MAX_XML_TOKENS / 2 {
        document.push_str("<a/>");
    }
    document.push_str("</r>");
    assert_eq!(refusal(document.as_bytes()), Error::TokenCap);
}

/// And the total is a total: the same event count spread over one element or
/// over thousands costs the same, which is what makes it a work cap rather than
/// a per-element one.
#[test]
fn the_token_cap_is_a_total_and_not_a_per_element_cap() {
    let tight = Limits {
        max_tokens: 8,
        ..Limits::DEFAULT
    };
    // Four elements: eight events exactly, which the cap allows.
    assert_eq!(count_within(b"<r><a/><b/><c/></r>", &tight), Ok(8));
    // A fifth beside them costs two more events, and does not fit.
    assert_eq!(
        refusal_within(b"<r><a/><b/><c/><d/></r>", &tight),
        Error::TokenCap
    );
    // Nor does a fifth *nested* rather than beside them, at a depth the depth
    // cap is nowhere near: depth is not what is being counted.
    assert_eq!(
        refusal_within(b"<r><a><b><c><d/></c></b></a></r>", &tight),
        Error::TokenCap
    );
}

// ---------------------------------------------------------------------------
// Ruling 8, checked rather than asserted in prose.
// ---------------------------------------------------------------------------

/// The public API carries no PDF vocabulary and no XPS vocabulary.
///
/// Prose cannot enforce this and the compiler will not: an `Element` that grew
/// a `fixed_page()` helper would compile perfectly. So every `pub` item in the
/// crate is read back out of its own source and checked against the words that
/// would mean the leaf had learned what it is for.
#[test]
fn no_public_item_names_a_pdf_or_an_xps_concept() {
    const SOURCES: [&str; 4] = [
        include_str!("lib.rs"),
        include_str!("limits.rs"),
        include_str!("scan.rs"),
        include_str!("text.rs"),
    ];
    const FORBIDDEN: [&str; 12] = [
        "Pdf",
        "PDF",
        "Cos",
        "Xps",
        "XPS",
        "Opc",
        "OPC",
        "FixedPage",
        "Page",
        "Glyph",
        "Font",
        "Stream",
    ];
    let mut checked = 0usize;
    for source in SOURCES {
        for line in source.lines() {
            let code = line.trim_start();
            if !code.starts_with("pub ") {
                continue;
            }
            checked += 1;
            for word in FORBIDDEN {
                assert!(
                    !code.contains(word),
                    "a public item names `{word}`, which is a concept a leaf may not \
                     have: {code}"
                );
            }
        }
    }
    // A check that found nothing to check is a check that does not run — the
    // shape gap 20 found where a skipped oracle exits 0 and reads like a pass.
    assert!(checked > 40, "only {checked} public items were checked");
}

// ---------------------------------------------------------------------------
// Ruling 1, cheaply, on every `cargo test` rather than only in a campaign.
// ---------------------------------------------------------------------------

/// A well-formed document, cut at every byte, never panics.
///
/// The fuzz target is the real instrument; this is the tripwire that catches a
/// panic introduced today rather than one found in next month's session, and it
/// is `tinker-pdf-zip`'s pattern verbatim.
#[test]
fn truncating_a_good_document_anywhere_never_panics() {
    let document = b"\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"utf-8\"?>\
        <r xmlns=\"u\" xmlns:p=\"v\"><!-- c --><p:a x=\"1\" xml:lang=\"en\">t&amp;t</p:a>\
        <![CDATA[c]]><?pi v?><b/></r>";
    for end in 0..=document.len() {
        let Ok(source) = Source::new(document.get(..end).unwrap_or_default()) else {
            continue;
        };
        for event in source.reader(&Limits::DEFAULT) {
            if event.is_err() {
                break;
            }
        }
    }
}

#[test]
fn flipping_any_single_byte_never_panics() {
    let original = b"<r xmlns=\"u\"><a b=\"&#65;\"/><![CDATA[x]]></r>";
    for index in 0..original.len() {
        for bit in 0..8 {
            let mut damaged = original.to_vec();
            if let Some(byte) = damaged.get_mut(index) {
                *byte ^= 1 << bit;
            }
            let Ok(source) = Source::new(&damaged) else {
                continue;
            };
            for event in source.reader(&Limits::DEFAULT) {
                if event.is_err() {
                    break;
                }
            }
        }
    }
}

#[test]
fn arbitrary_bytes_never_panic() {
    // A cheap deterministic sweep rather than a corpus: every byte value as a
    // whole document, and every byte value spliced into a well-formed one.
    for byte in 0..=255u8 {
        if let Ok(source) = Source::new(&[byte]) {
            source.reader(&Limits::DEFAULT).count();
        }
        let document = [b"<r a=\"1\">".as_slice(), &[byte], b"</r>".as_slice()].concat();
        if let Ok(source) = Source::new(&document) {
            source.reader(&Limits::DEFAULT).count();
        }
    }
}

/// A **second** byte order mark is text, and the reader refuses it where it
/// stands.
///
/// Found by gap 30 milestone 9's fuzz campaign, which is the first session the
/// `xml` target has ever had. libFuzzer produced thirteen bytes — a control
/// byte and then `FE FF FE FF` and four code units — and the target's own
/// assertion fired: *a byte order mark reached the text*.
///
/// **The crate was right and the assertion was wrong**, which is worth writing
/// down because the opposite is the usual outcome. 4.3.3 has `Source::new`
/// consume *the* mark, one, at offset zero; a `U+FEFF` after it is
/// ZERO WIDTH NO-BREAK SPACE and an ordinary character, so decoding leaves it
/// in the text exactly as it should. What it is not is *legal there*: 2.8's
/// prolog admits whitespace, processing instructions, comments and a doctype,
/// and ZWNBSP is none of those — so the reader answers
/// [`Error::TextBeforeRoot`], which is the right refusal by the right name.
///
/// Both halves are asserted here, because a build that stripped every leading
/// `U+FEFF` would satisfy the first and lose the second: it would silently
/// accept a document XML says is malformed.
#[test]
fn a_second_byte_order_mark_is_text_and_is_refused_where_it_stands() {
    // UTF-16 big-endian: the mark, the mark again, then two characters.
    let bytes = [0xFEu8, 0xFF, 0xFE, 0xFF, 0xD3, 0x3C, 0xFA, 0x3C];
    let source = Source::new(&bytes).expect("it decodes");
    assert_eq!(source.encoding(), Encoding::Utf16BigEndian);

    // The first mark is gone and the second is a character.
    assert_eq!(
        source.text().chars().next(),
        Some('\u{FEFF}'),
        "the second mark is text, not a mark"
    );
    assert_eq!(source.text().chars().count(), 3);

    // And it is not text the prolog admits.
    let mut reader = source.reader(&Limits::DEFAULT);
    assert_eq!(reader.next(), Some(Err(Error::TextBeforeRoot)));
}
