//! The HTML parser's bounds and its decoding, each asserted by name.
//!
//! html5lib's suite (`tests/html5lib.rs`) says whether the trees are right.
//! What it cannot say is whether the four [`Limits`] hold — every test in it
//! is a few hundred bytes — or how bytes become text, which it does not test
//! at all. Each cap here is crossed at its **shipped** value, by an input a
//! few kilobytes long, and stops the parse by its own name.

use tinker_pdf_xml::encoding::SingleByte;
use tinker_pdf_xml::html::{self, DecodedAs, Document, NodeData};
use tinker_pdf_xml::limits::{MAX_XML_ATTRIBUTES, MAX_XML_DEPTH, MAX_XML_NAME_LEN, MAX_XML_TOKENS};
use tinker_pdf_xml::{Error, Limits};

fn text_of(document: &Document) -> String {
    let mut out = String::new();
    for node in document.nodes() {
        if let NodeData::Text(text) = &node.data {
            out.push_str(text);
        }
    }
    out
}

#[test]
fn nesting_past_the_depth_cap_stops_the_parse_by_name() {
    let deep = "<div>".repeat(MAX_XML_DEPTH);
    let document = html::parse(&deep, &Limits::DEFAULT);
    assert_eq!(document.stopped(), Some(Error::DepthCap));
    // `html` and `body` are two of the stack's entries, so the cap is met
    // two `<div>`s before the input runs out, and the tree up to it is kept.
    let divs = document
        .nodes()
        .iter()
        .filter(|n| n.element().is_some_and(|e| e.name == "div"))
        .count();
    assert_eq!(divs, MAX_XML_DEPTH - 1);
    let shallow = "<div>".repeat(MAX_XML_DEPTH - 3);
    assert_eq!(html::parse(&shallow, &Limits::DEFAULT).stopped(), None);
}

#[test]
fn attributes_past_the_cap_stop_the_parse_by_name() {
    let attributes: String = (0..=MAX_XML_ATTRIBUTES).map(|i| format!(" a{i}")).collect();
    let document = html::parse(&format!("<p{attributes}>after"), &Limits::DEFAULT);
    assert_eq!(document.stopped(), Some(Error::AttributeCap));
    assert!(!text_of(&document).contains("after"));
    let fits: String = (0..MAX_XML_ATTRIBUTES).map(|i| format!(" a{i}")).collect();
    assert_eq!(
        html::parse(&format!("<p{fits}>after"), &Limits::DEFAULT).stopped(),
        None
    );
}

#[test]
fn a_name_past_the_cap_stops_the_parse_rather_than_being_truncated() {
    let long = "a".repeat(MAX_XML_NAME_LEN + 1);
    for markup in [
        format!("<{long}>"),
        format!("<p {long}=1>"),
        format!("<!DOCTYPE {long}>"),
    ] {
        assert_eq!(
            html::parse(&markup, &Limits::DEFAULT).stopped(),
            Some(Error::NameCap),
            "{}",
            &markup[..12]
        );
    }
    let fits = "a".repeat(MAX_XML_NAME_LEN);
    assert_eq!(
        html::parse(&format!("<{fits}>"), &Limits::DEFAULT).stopped(),
        None
    );
}

/// **The token cap counts the nodes the tree builder makes, not only the
/// tokens.** Two hundred formatting elements left open inside a `<p>` are
/// closed by its `</p>` and stay in the list of active formatting elements,
/// so each `<p>x</p>` after them — three tokens, eight bytes — reopens all
/// two hundred. Fifty kilobytes of that is eighteen thousand tokens and over a
/// million elements; counted as tokens alone it would never stop.
#[test]
fn reopened_formatting_elements_are_spent_against_the_token_cap() {
    let mut markup = String::from("<p>");
    for i in 0..200 {
        markup.push_str(&format!("<b id={i}>"));
    }
    markup.push_str("</p>");
    markup.push_str(&"<p>x</p>".repeat(6_000));
    assert!(markup.len() < 64 * 1024);
    let document = html::parse(&markup, &Limits::DEFAULT);
    assert_eq!(document.stopped(), Some(Error::TokenCap));
    assert!(document.nodes().len() <= MAX_XML_TOKENS + 2);
    // A tenth of it fits.
    let mut small = String::from("<p>");
    for i in 0..200 {
        small.push_str(&format!("<b id={i}>"));
    }
    small.push_str("</p>");
    small.push_str(&"<p>x</p>".repeat(500));
    assert_eq!(html::parse(&small, &Limits::DEFAULT).stopped(), None);
}

/// **The tree builder's moves are not quadratic in a parent's children.** A
/// misnested `</b>` hands every child of the block after it to a clone — sixty
/// thousand here — and a table that foster-parents sixty thousand elements
/// inserts each in front of itself. Moving a child at a time from the front of
/// the list, or finding the table from the front, made each of these billions
/// of steps; the assertions are the trees the standard builds, and the bound
/// is the test finishing at all.
#[test]
fn moving_many_children_is_linear() {
    let many = 60_000;
    let adopted = html::parse(
        &format!("<b><div>{}</b>", "<i></i>".repeat(many)),
        &Limits::DEFAULT,
    );
    assert_eq!(adopted.stopped(), None);
    // body > b, div > b' > every <i>.
    let clone = adopted
        .nodes()
        .iter()
        .find(|n| n.element().is_some_and(|e| e.name == "b") && n.children.len() == many)
        .expect("the formatting element's clone holds every child the block had");
    assert!(clone
        .children
        .iter()
        .all(|&c| adopted.node(c).and_then(|n| n.parent).is_some()));

    let fostered = html::parse(
        &format!("<table>{}</table>", "<span></span>".repeat(many)),
        &Limits::DEFAULT,
    );
    let body = fostered
        .nodes()
        .iter()
        .find(|n| n.element().is_some_and(|e| e.name == "body"))
        .expect("a body");
    assert_eq!(body.children.len(), many + 1, "every span before the table");
}

/// Tag soup is a document: what the XML reader stops at in its first line is
/// read whole.
#[test]
fn tag_soup_is_a_document() {
    let document = html::parse(
        "<title>Soup</title><p>One<br>two &amp three &nbsp<p class=x>Four<li>five",
        &Limits::DEFAULT,
    );
    assert_eq!(document.stopped(), None);
    assert!(document.errors() > 0, "the soup has parse errors");
    assert_eq!(text_of(&document), "SoupOnetwo & three \u{A0}Fourfive");
    let names: Vec<&str> = document
        .nodes()
        .iter()
        .filter_map(|n| n.element().map(|e| e.name.as_str()))
        .collect();
    assert_eq!(
        names,
        ["html", "head", "title", "body", "p", "br", "p", "li"]
    );
}

#[test]
fn bytes_are_decoded_by_mark_then_meta_then_utf8_then_windows_1252() {
    let utf8 = html::parse_bytes("<p>é".as_bytes(), &Limits::DEFAULT);
    let decoding = utf8.encoding().expect("decoded");
    assert_eq!(
        (decoding.encoding, decoding.confident),
        (DecodedAs::Utf8, false)
    );
    assert_eq!(text_of(&utf8), "é");

    // Not UTF-8 and nothing says what: windows-1252, where 0x93 is a quote.
    let latin = html::parse_bytes(b"<p>\x93caf\xe9\x94", &Limits::DEFAULT);
    assert_eq!(
        latin.encoding().map(|d| d.encoding),
        Some(DecodedAs::SingleByte(SingleByte::Windows1252))
    );
    assert_eq!(text_of(&latin), "\u{201C}café\u{201D}");

    // A `<meta charset>` in the first kilobyte, after a comment holding a
    // decoy and another tag's attributes.
    let russian = html::parse_bytes(
        b"<!-- <meta charset=koi8-r> --><html lang=ru><head>\
          <meta http-equiv=Content-Type content='text/html; charset=windows-1251'>\
          </head><p>\xcf\xf0\xe8\xe2\xe5\xf2",
        &Limits::DEFAULT,
    );
    let decoding = russian.encoding().expect("decoded");
    assert_eq!(
        (decoding.encoding, decoding.confident),
        (DecodedAs::SingleByte(SingleByte::Windows1251), true)
    );
    assert_eq!(text_of(&russian), "Привет");

    let charset = html::parse_bytes(
        b"<meta charset=\"KOI8-R\"><p>\xf0\xd2\xc9",
        &Limits::DEFAULT,
    );
    assert_eq!(text_of(&charset), "При");
    assert_eq!(charset.encoding().and_then(|d| d.not_decoded), None);

    // A multi-byte encoding is named and set aside for the guess.
    let japanese = html::parse_bytes(b"<meta charset=shift_jis><p>\x82\xa0", &Limits::DEFAULT);
    let decoding = japanese.encoding().expect("decoded");
    assert_eq!(decoding.not_decoded, Some("Shift_JIS"));
    assert_eq!(
        (decoding.encoding, decoding.confident),
        (DecodedAs::SingleByte(SingleByte::Windows1252), false)
    );

    // A byte order mark beats a `<meta>`.
    let marked = html::parse_bytes(
        "\u{FEFF}<meta charset=koi8-r><p>é".as_bytes(),
        &Limits::DEFAULT,
    );
    assert_eq!(marked.encoding().map(|d| d.encoding), Some(DecodedAs::Utf8));
    assert_eq!(text_of(&marked), "é");

    let wide: Vec<u8> = [0xFF, 0xFE]
        .into_iter()
        .chain("<p>ok".encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    let utf16 = html::parse_bytes(&wide, &Limits::DEFAULT);
    assert_eq!(
        utf16.encoding().map(|d| d.encoding),
        Some(DecodedAs::Utf16LittleEndian)
    );
    assert_eq!(text_of(&utf16), "ok");
}
