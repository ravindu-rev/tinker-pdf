//! A book set in one of the standard 14, with more characters outside
//! `WinAnsiEncoding` than a simple font has codes for.
//!
//! The tier-4 row: *"EPUB text set in a standard-14 fallback face loses
//! characters past a simple font's 256 codes — 224 outside `WinAnsiEncoding`
//! — counted as `UnrepresentedCharacters`"*, with a CID-keyed path for
//! fallback text as its exit. That path exists where the build carries a face
//! to key it to: with `bundled-fonts`, a character outside the encoding that
//! the standard face's Liberation stand-in covers is drawn in that face as a
//! composite font, so nothing is lost and nothing is a notdef. Without it, the
//! standard-14 limit stays, and is counted — so this file asserts **both**
//! answers, each in the build that gives it.
//!
//! The text is 234 distinct characters every Liberation face covers and
//! `WinAnsiEncoding` does not: Latin Extended-A less the seven letters
//! cp1252 has, the Greek capitals and small letters, and the Cyrillic basic
//! alphabet. 234 is ten past the 224 an overflow font holds, so the standard
//! build has a number to report and the bundled one has a number to beat.

mod epub_support;

use epub_support::book::{faces_book, styled_book};
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::{ArchiveWarning, Document, TextOptions};

/// Every character of the test text, once each, in code point order.
fn characters() -> Vec<char> {
    // The seven of U+0100..U+017F that `WinAnsiEncoding` already has a code
    // for, and so never reach either path under test.
    let winansi = [
        '\u{152}', '\u{153}', '\u{160}', '\u{161}', '\u{178}', '\u{17D}', '\u{17E}',
    ];
    let latin = (0x100u32..=0x17F)
        .filter_map(char::from_u32)
        .filter(|c| !winansi.contains(c));
    // U+03A2 is unassigned: there is no final capital sigma.
    let greek = (0x391u32..=0x3A9)
        .chain(0x3B1..=0x3C9)
        .filter(|code| *code != 0x3A2)
        .filter_map(char::from_u32);
    let cyrillic = (0x410u32..=0x44F).filter_map(char::from_u32);
    latin.chain(greek).chain(cyrillic).collect()
}

/// The book: one paragraph in the initial `serif` family — Times — with a
/// space after every tenth character so the line breaker has somewhere to
/// break.
fn book() -> Vec<u8> {
    let mut body = String::new();
    for (at, ch) in characters().iter().enumerate() {
        if at > 0 && at % 10 == 0 {
            body.push(' ');
        }
        body.push(*ch);
    }
    faces_book(&[], 16, &body)
}

/// Every page's text, joined.
fn text(doc: &Document) -> String {
    doc.pages()
        .iter()
        .map(|page| page.text().plain_text())
        .collect()
}

fn unrepresented(doc: &Document) -> usize {
    doc.archive()
        .expect("a book has a report")
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::UnrepresentedCharacters { characters } => Some(*characters),
            _ => None,
        })
        .sum()
}

fn uncovered(doc: &Document) -> usize {
    doc.archive()
        .expect("a book has a report")
        .warnings()
        .iter()
        .filter_map(|w| match w {
            ArchiveWarning::UncoveredCharacters { characters } => Some(*characters),
            _ => None,
        })
        .sum()
}

#[test]
fn the_text_is_what_the_row_says_it_is() {
    let chars = characters();
    assert_eq!(chars.len(), 234, "the fixture's own count moved");
    for ch in &chars {
        assert!(
            tinker_pdf::epub::paint::winansi_code(*ch).is_none(),
            "{ch:?} has a WinAnsi code and would test nothing"
        );
    }
}

/// **With a face to key it to, nothing is lost and nothing is a notdef.**
///
/// Every character is drawn in the Liberation Serif stand-in, embedded as a
/// composite font, so neither warning fires and every character is on the
/// page's text in the order it was written.
#[cfg(feature = "bundled-fonts")]
#[test]
fn past_the_simple_font_limit_the_fallback_is_cid_keyed() {
    let doc = Document::open(book()).expect("the book opens");
    assert_eq!(unrepresented(&doc), 0, "characters were lost");
    assert_eq!(uncovered(&doc), 0, "characters were drawn as a notdef");
    let extracted: String = text(&doc).chars().filter(|c| !c.is_whitespace()).collect();
    let expected: String = characters().into_iter().collect();
    assert_eq!(
        extracted, expected,
        "the page's text is not the book's text"
    );
    // And it is a composite font over the stand-in that drew them, beside
    // the simple Times the Latin text needs.
    let fonts: Vec<(String, tinker_pdf::FontKind)> = doc
        .fonts()
        .into_iter()
        .map(|font| (font.base_font, font.kind))
        .collect();
    assert!(
        fonts
            .iter()
            .any(|(name, kind)| name.ends_with("LiberationSerif")
                && *kind == tinker_pdf::FontKind::Type0),
        "no composite Liberation Serif stand-in was embedded: {fonts:?}"
    );
}

/// **Without one, the standard-14 limit stays, and is counted.**
///
/// The first 224 characters take overflow codes, are drawn as a notdef each —
/// one occurrence apiece, so 224 uncovered — and extract; the ten past them
/// have no code at all and are reported unrepresented.
#[cfg(not(feature = "bundled-fonts"))]
#[test]
fn past_the_simple_font_limit_the_excess_is_counted() {
    let doc = Document::open(book()).expect("the book opens");
    let distinct = characters().len();
    assert_eq!(unrepresented(&doc), distinct - 224);
    assert_eq!(uncovered(&doc), 224);
    let extracted = text(&doc);
    let present = characters()
        .iter()
        .filter(|ch| extracted.contains(**ch))
        .count();
    assert_eq!(present, 224, "the overflow font's characters extract");
}

/// **One path owns the run**: a character drawn in the stand-in is measured in
/// it too, at the stand-in's own `hmtx` advance — read here straight out of
/// the bundled face's bytes, not out of the code under test. A build that
/// drew in Liberation and measured in Times's AFM would set lines whose glyphs
/// do not fit the boxes they were broken into.
#[cfg(feature = "bundled-fonts")]
#[test]
fn a_fallback_character_is_measured_in_the_face_it_is_drawn_in() {
    use tinker_pdf::epub::paint::BookMetrics;
    use tinker_pdf_css::property::{FontFamily, FontStyle};
    use tinker_pdf_font::bundled::{face, Family};
    use tinker_pdf_font::Sfnt;
    use tinker_pdf_layout::metrics::{FontRequest, Metrics};

    let families = vec![FontFamily::Serif];
    let request = FontRequest {
        families: &families,
        weight: 400,
        style: FontStyle::Normal,
        size: 10.0,
        kerning: tinker_pdf_css::property::FontKerning::Auto,
        features: &[],
    };
    let sfnt = Sfnt::parse(face(Family::Serif, false, false)).expect("the bundled face parses");
    for ch in ['\u{416}', '\u{3A9}', '\u{11F}'] {
        let glyph = sfnt.glyph_for_char(ch).expect("Liberation covers it");
        let expected = f64::from(sfnt.advance(glyph).expect("an advance"))
            / f64::from(sfnt.units_per_em)
            * 10.0;
        assert_eq!(
            BookMetrics::STANDARD.advance(ch, &request),
            expected,
            "{ch:?} is measured in a face it is not drawn in"
        );
    }
}

// ---- right-to-left fallback text ---------------------------------------------

/// HET and VAV, the Hebrew word Melville's etymology opens with.
const HET_VAV: &str = "\u{5D7}\u{5D5}";

/// The same two letters as a page that draws them in logical order shows them,
/// read right to left: the word backwards.
const VAV_HET: &str = "\u{5D5}\u{5D7}";

/// Where the first character `text` is drawn starts, along the baseline,
/// read in the order the content stream drew it.
fn drawn_at(doc: &Document, text: &str) -> f64 {
    let page = doc.page(0).expect("a page");
    let drawn = page.text_with(&TextOptions {
        content_order: true,
    });
    drawn
        .lines()
        .iter()
        .flat_map(|line| line.chars.iter())
        .find(|c| c.text == text)
        .map(|c| c.quad.bounds().0)
        .unwrap_or_else(|| panic!("nothing draws {text:?}"))
}

/// **A right-to-left word set in the standard 14 is drawn right to left, and
/// extracts as it was written** (UAX #9 rule L2; ruling 14).
///
/// The etymology of Project Gutenberg's `pg2701-images.epub` — the word for
/// whale in each language, then the language — with the lines a `<br/>`
/// ends. No face in the book covers Hebrew, so `חו` is fallback text: the
/// overflow font in a default build, the Liberation stand-in with
/// `bundled-fonts`, and `draw_coded` in both. Its line is left to right and
/// the word, at level 1, is reversed by L2, so `ו` is drawn left of `ח`.
///
/// `draw_coded` wrote a standard-14 piece in the order it was typed, `ח`
/// at the left, and nothing noticed while extraction read the content
/// stream's order, which was the typed order too. Ruling 14 (dd50471)
/// reads the line the page **draws**, and read the word back as `וח`: the
/// whole book stopped conserving at its first Hebrew word in
/// `epub_fetched.rs`'s two conservation sweeps, in content order and in the
/// structure tree's order alike.
#[test]
fn a_standard_14_hebrew_word_extracts_as_written() {
    let body = format!(
        "<p>{HET_VAV}, <i>Hebrew</i>.<br/>\u{3F0}\u{3B7}\u{3C4}\u{3BF}\u{3C2}, \
         <i>Greek</i>.<br/>CETUS, <i>Latin</i>.<br/></p>"
    );
    let bytes = styled_book("en", "", &body);
    let doc = Document::open(bytes.clone()).expect("the book opens");

    let (vav, het) = (drawn_at(&doc, "\u{5D5}"), drawn_at(&doc, "\u{5D7}"));
    assert!(
        vav < het,
        "the word is drawn in the order it was typed: \u{5D7} at {het}, \u{5D5} at {vav}"
    );

    let text = doc.page(0).expect("a page").text().plain_text();
    assert!(
        text.contains(&format!("{HET_VAV}, Hebrew.")),
        "the Hebrew word does not read as written: {text:?}"
    );
    for verdict in [
        conservation(&bytes, &doc),
        conservation_in_logical_order(&bytes, &doc),
    ] {
        assert!(
            verdict.holds(),
            "{} extra, {} missing, {:?}",
            verdict.extra,
            verdict.missing,
            verdict.divergences
        );
    }
}

/// **A right-to-left paragraph set in the standard 14 reads as written.**
///
/// `dir="rtl"` makes the paragraph level 1, so the run `draw_coded` is handed
/// is at an odd level with its comma and space at that level too: L2 draws
/// the punctuation left of the words and the words right to left, `ok` at
/// level 2 its own run. Drawn as typed, the line read back ` ,חו וחok.`.
///
/// The first word carries a zero-width joiner, which rule X9 removes from
/// the levels L2 orders: it is still drawn, between its two letters, or the
/// page would be a character short.
#[test]
fn a_standard_14_right_to_left_paragraph_extracts_as_written() {
    let body = format!("<p dir=\"rtl\">\u{5D7}\u{200D}\u{5D5} {VAV_HET}, ok.</p>");
    let bytes = styled_book("he", "", &body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    for verdict in [
        conservation(&bytes, &doc),
        conservation_in_logical_order(&bytes, &doc),
    ] {
        assert!(
            verdict.holds(),
            "{} extra, {} missing, {:?}",
            verdict.extra,
            verdict.missing,
            verdict.divergences
        );
    }
}

/// **The same word alone on its line — the book's own shape, a table with a
/// cell for the word and one for the language — is drawn right to left and
/// reads as `חו`.**
///
/// What is asserted is the word: drawn `ו` left of `ח`, and read back with
/// `ח` first, where the page drew it `ח` first and read it back `וח`.
///
/// What is **not** asserted is the comma's side. The cell is a left-to-right
/// paragraph, so its comma, at level 0, is drawn right of the word — the
/// picture a right-to-left paragraph that *opens* with the comma draws too.
/// The cell's line holds no left-to-right character for ruling 14's "the two
/// ends decide" to find, so it reads the line as right to left, and the
/// comma first: `,חו` — what `text_order.rs` names as "a line is resolved
/// alone", and the same answer a face that covers Hebrew gets. Which way
/// such a line reads is ruling 14's to decide, not this painter's.
#[test]
fn a_standard_14_hebrew_word_alone_on_its_line_is_drawn_right_to_left() {
    let body = format!(
        "<table><tr><td>{HET_VAV},</td><td><i>Hebrew</i>.</td></tr>\
         <tr><td>\u{3F0}\u{3B7}\u{3C4}\u{3BF}\u{3C2},</td><td><i>Greek</i>.</td></tr>\
         <tr><td>CETUS,</td><td><i>Latin</i>.</td></tr></table>"
    );
    let doc = Document::open(styled_book("en", "", &body)).expect("the book opens");

    let (vav, het) = (drawn_at(&doc, "\u{5D5}"), drawn_at(&doc, "\u{5D7}"));
    assert!(
        vav < het,
        "the word is drawn in the order it was typed: \u{5D7} at {het}, \u{5D5} at {vav}"
    );

    let text = doc.page(0).expect("a page").text().plain_text();
    assert!(
        text.contains(HET_VAV) && !text.contains(VAV_HET),
        "the Hebrew word reads backwards: {text:?}"
    );
}
