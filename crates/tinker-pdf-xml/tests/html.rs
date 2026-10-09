//! The HTML parser's bounds and its decoding, each asserted by name.
//!
//! html5lib's suite (`tests/html5lib.rs`) says whether the trees are right.
//! What it cannot say is whether the four [`Limits`] hold — every test in it
//! is a few hundred bytes — or how bytes become text, which it does not test
//! at all. Each cap here is crossed at its **shipped** value, by an input a
//! few kilobytes long, and stops the parse by its own name.

use tinker_pdf_xml::encoding::SingleByte;
use tinker_pdf_xml::html::{self, DecodedAs, Document, NodeData};
use tinker_pdf_xml::limits::{
    MAX_HTML_ACTIVE_FORMATTING, MAX_HTML_CLONE_BYTES, MAX_XML_ATTRIBUTES, MAX_XML_DEPTH,
    MAX_XML_NAME_LEN, MAX_XML_TOKENS,
};
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

fn elements_named<'a>(document: &'a Document, name: &str) -> Vec<&'a html::Element> {
    document
        .nodes()
        .iter()
        .filter_map(|n| n.element())
        .filter(|e| e.name == name)
        .collect()
}

fn attribute_bytes(document: &Document) -> usize {
    document
        .nodes()
        .iter()
        .filter_map(|n| n.element())
        .flat_map(|e| e.attributes.iter())
        .map(|a| a.name.len() + a.value.len())
        .sum()
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

/// **The attribute cap is per element, not only per tag.** A `<body>` or an
/// `<html>` start tag after the first adds each attribute the element does
/// not carry yet, so a file of them — two thousand one-attribute `<html>`s, or
/// two `<body>`s of two hundred each — would hang as many on one element as
/// it liked. The merge that would cross the cap is not made and the parse
/// stops by name.
#[test]
fn attributes_merged_into_html_and_body_are_held_to_the_cap() {
    let many: String = (0..2_000).map(|i| format!("<html a{i}>")).collect();
    let document = html::parse(&many, &Limits::DEFAULT);
    assert_eq!(document.stopped(), Some(Error::AttributeCap));
    let root = elements_named(&document, "html");
    assert_eq!(root.len(), 1);
    assert_eq!(root[0].attributes.len(), MAX_XML_ATTRIBUTES);

    let half = MAX_XML_ATTRIBUTES / 2 + 1;
    let body = |prefix: &str| -> String {
        let attributes: String = (0..half).map(|i| format!(" {prefix}{i}")).collect();
        format!("<body{attributes}>")
    };
    let two = format!("{}<p>one{}<p>two", body("a"), body("b"));
    let document = html::parse(&two, &Limits::DEFAULT);
    assert_eq!(document.stopped(), Some(Error::AttributeCap));
    assert_eq!(elements_named(&document, "body")[0].attributes.len(), half);
    assert_eq!(text_of(&document), "one");

    // Names the element already carries are not counted twice, and a merge
    // up to the cap exactly is kept whole.
    let fits: String = (0..MAX_XML_ATTRIBUTES)
        .map(|i| format!("<html a{i} a0>"))
        .collect();
    let document = html::parse(&fits, &Limits::DEFAULT);
    assert_eq!(document.stopped(), None);
    assert_eq!(
        elements_named(&document, "html")[0].attributes.len(),
        MAX_XML_ATTRIBUTES
    );
}

/// **[`MAX_HTML_CLONE_BYTES`] fires, by [`Error::CloneCap`], at the shipped
/// value.** Every `<p>x</p>` after a `<b>` a `</p>` closed reopens it, and the
/// clone carries a copy of its attributes: one value of a hundred thousand
/// bytes, reopened seven hundred times, asks for seventy megabytes from a
/// hundred and six kilobytes of markup. The clone that would cross the cap is
/// not made, and what the tree holds is the input and the cap at the most.
#[test]
fn attribute_bytes_copied_onto_clones_stop_at_their_cap() {
    let value = 100_000;
    let markup = |reopened: usize| {
        format!(
            "<p><b title=\"{}\"></p>{}",
            "v".repeat(value),
            "<p>x</p>".repeat(reopened)
        )
    };
    let per_clone = "title".len() + value;
    let fit = MAX_HTML_CLONE_BYTES / per_clone;

    let over = markup(700);
    assert!(over.len() < 110 * 1024);
    let document = html::parse(&over, &Limits::DEFAULT);
    assert_eq!(document.stopped(), Some(Error::CloneCap));
    assert_eq!(elements_named(&document, "b").len(), 1 + fit);
    assert!(attribute_bytes(&document) <= over.len() + MAX_HTML_CLONE_BYTES);

    let exactly = html::parse(&markup(fit), &Limits::DEFAULT);
    assert_eq!(exactly.stopped(), None);
    assert_eq!(elements_named(&exactly, "b").len(), 1 + fit);
}

/// **And a clone's attributes are counted, not only its bytes.** A `<b>` of
/// two hundred and fifty-six one-byte-valued attributes reopened by every
/// `<p>x</p>` copies two hundred and fifty-six attributes per eight bytes, so
/// thirty-five kilobytes asked for a million of them with a few thousand
/// nodes spent. Each spends one unit of the token cap as a node does.
#[test]
fn attributes_copied_onto_clones_are_spent_against_the_token_cap() {
    let attributes: String = (0..MAX_XML_ATTRIBUTES).map(|i| format!(" a{i}")).collect();
    let markup = |reopened: usize| format!("<p><b{attributes}></p>{}", "<p>x</p>".repeat(reopened));
    let over = markup(4_100);
    assert!(over.len() < 36 * 1024);
    let document = html::parse(&over, &Limits::DEFAULT);
    assert_eq!(document.stopped(), Some(Error::TokenCap));
    let copied: usize = elements_named(&document, "b")
        .iter()
        .map(|b| b.attributes.len())
        .sum();
    assert!(copied <= MAX_XML_TOKENS);

    assert_eq!(
        html::parse(&markup(3_000), &Limits::DEFAULT).stopped(),
        None
    );
}

/// **[`MAX_HTML_ACTIVE_FORMATTING`] fires, by [`Error::FormattingCap`], at the
/// shipped value, and not as the depth cap.** A table cell is a marker, and
/// the two hundred `<b>`s its `</p>` closes stay in the list behind it; six
/// cells nested one inside another hold 1 206 entries with the stack of open
/// elements never past 227. The lane that wrote the list's bound reported it
/// as [`Error::DepthCap`], which no element on that stack was near.
#[test]
fn the_list_of_active_formatting_elements_stops_at_its_cap() {
    let cells = |levels: usize| {
        let mut markup = String::new();
        for _ in 0..levels {
            markup.push_str("<table><tr><td><p>");
            for i in 0..200 {
                markup.push_str(&format!("<b id={i}>"));
            }
            markup.push_str("</p>");
        }
        markup
    };
    let over = cells(6);
    assert!(over.len() < 16 * 1024);
    const { assert!(6 * 201 > MAX_HTML_ACTIVE_FORMATTING) };
    let document = html::parse(&over, &Limits::DEFAULT);
    assert_eq!(document.stopped(), Some(Error::FormattingCap));

    const { assert!(5 * 201 <= MAX_HTML_ACTIVE_FORMATTING) };
    let fits = html::parse(&cells(5), &Limits::DEFAULT);
    assert_eq!(fits.stopped(), None);
    assert_eq!(elements_named(&fits, "b").len(), 5 * 200);
}

/// **Noah's Ark is linear in a tag's attributes.** Every formatting start tag
/// is compared with each entry after the last marker that has its name, and
/// the comparison looked each of one tag's attributes up in the other's list:
/// two hundred and forty open `<b>`s of two hundred and fifty-six attributes
/// made every further `<b>` sixteen million string comparisons, seventy
/// milliseconds apiece. The tree asserted is the standard's; the bound is the
/// test finishing at all.
#[test]
fn noahs_ark_compares_attribute_lists_in_one_pass() {
    // The same in any order: of four, the earliest goes, so three are
    // reopened after the `</p>` — and four that differ in one value, or an
    // `<i>` among `<b>`s with its attributes, are all reopened.
    let reopened = |markup: &str| {
        let document = html::parse(markup, &Limits::DEFAULT);
        let all = elements_named(&document, "b").len() + elements_named(&document, "i").len();
        all - 4
    };
    assert_eq!(
        reopened("<p><b a=1 c=2><b c=2 a=1><b a=1 c=2><b c=2 a=1></p>x"),
        3
    );
    assert_eq!(reopened("<p><b a=1><b a=2><b a=3><b a=4></p>x"), 4);
    assert_eq!(reopened("<p><i a=1><b a=1><b a=1><b a=1></p>x"), 4);

    let attributes: String = (0..MAX_XML_ATTRIBUTES - 1)
        .map(|i| format!(" a{i}"))
        .collect();
    let mut markup = String::new();
    for i in 0..240 {
        markup.push_str(&format!("<b{attributes} z={i}>"));
    }
    for j in 0..1_000 {
        markup.push_str(&format!("<b{attributes} z=n{j}></b>"));
    }
    let document = html::parse(&markup, &Limits::DEFAULT);
    assert_eq!(document.stopped(), None);
    assert_eq!(elements_named(&document, "b").len(), 1_240);
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
///
/// Text fostered between them is the third case, and the one the lane missed:
/// a run is appended to the text node in front of the table when there is
/// one, and that node was looked for from the front of the list — five
/// seconds for forty thousand runs in a debug build, and minutes for the
/// hundred and fifty thousand here.
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

    let runs = 150_000;
    let fostered_text = html::parse(
        &format!("<table>{}</table>", "<b></b>x".repeat(runs)),
        &Limits::DEFAULT,
    );
    assert_eq!(fostered_text.stopped(), None);
    let body = fostered_text
        .nodes()
        .iter()
        .find(|n| n.element().is_some_and(|e| e.name == "body"))
        .expect("a body");
    assert_eq!(
        body.children.len(),
        2 * runs + 1,
        "every <b> and every run before the table"
    );
    assert!(body.children.iter().skip(1).step_by(2).all(|&c| matches!(
        fostered_text.node(c).map(|n| &n.data),
        Some(NodeData::Text(text)) if text == "x"
    )));
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

/// **The prescan's `<meta>` is §13.2.3.2's, variable for variable.** A
/// `charset` attribute overrides a `content` on its `<meta>` whichever comes
/// first, and one whose label names no encoding is *failure*, so that
/// `<meta>` names none and the bytes are guessed at; and `x-user-defined` is
/// read as windows-1252. The lane read the content's encoding past a bogus
/// `charset`, and set `x-user-defined` aside as not decoded.
#[test]
fn the_prescan_reads_a_meta_as_section_13_2_3_2_does() {
    let guessed = |bytes: &[u8]| {
        let decoding = html::parse_bytes(bytes, &Limits::DEFAULT)
            .encoding()
            .expect("decoded");
        (decoding.encoding, decoding.confident, decoding.not_decoded)
    };
    let latin = (DecodedAs::SingleByte(SingleByte::Windows1252), false, None);
    // \xf0\xd2\xc9 is "При" in KOI8-R and not UTF-8.
    assert_eq!(
        guessed(
            b"<meta http-equiv=content-type content='text/html; charset=koi8-r' \
              charset=bogus><p>\xf0\xd2\xc9"
        ),
        latin,
        "a bogus charset after the content"
    );
    assert_eq!(
        guessed(
            b"<meta charset=bogus http-equiv=content-type \
              content='text/html; charset=koi8-r'><p>\xf0\xd2\xc9"
        ),
        latin,
        "and before it"
    );
    assert_eq!(
        guessed(
            b"<meta http-equiv=content-type content='text/html; charset=koi8-r'>\
              <p>\xf0\xd2\xc9"
        ),
        (DecodedAs::SingleByte(SingleByte::Koi8R), true, None),
        "the content alone, with its pragma"
    );
    assert_eq!(
        guessed(b"<meta content='text/html; charset=koi8-r'><p>\xf0\xd2\xc9"),
        latin,
        "a content with no pragma names nothing"
    );
    assert_eq!(
        guessed(b"<meta charset=x-user-defined><p>\x93"),
        (DecodedAs::SingleByte(SingleByte::Windows1252), true, None),
        "x-user-defined"
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
