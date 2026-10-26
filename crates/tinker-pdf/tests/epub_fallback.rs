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
use epub_support::reading::{check, read, rep, word, Case, Expect, AR, AR8, HE, HEP};
use epub_support::typeface::covering;
use tinker_pdf::{ArchiveWarning, Document, TextOptions};
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

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

/// **A nonspacing mark set in the standard 14 is measured at no advance**, in
/// either build, and the overflow font's `/Widths` agree — which is what lets
/// the painter draw it on its letter rather than after it.
///
/// `Standard14::advance` has no entry for a mark and answers with a space's
/// width, and a mark measured that way was as wide as a letter: a letter's
/// second mark was then read with the glyph drawn next (CI run 38041540464,
/// `a_standard_14_arabic_letter_with_two_marks_keeps_both`). A combining kana
/// voiced sound mark is East Asian `W` to UAX #11, which measures one em, and
/// is a mark first: the order of the two questions is asserted here. HIRAGANA
/// A, beside it, keeps its em, and a Hebrew letter keeps a letter's width.
#[test]
fn a_standard_14_mark_is_measured_at_no_advance() {
    use tinker_pdf::epub::paint::BookMetrics;
    use tinker_pdf_css::property::{FontFamily, FontStyle};
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
    assert!(
        tinker_pdf_layout::unicode::is_east_asian('\u{3099}'),
        "the kana voiced sound mark is no longer East Asian, and the order is untested"
    );
    for mark in [
        '\u{301}', '\u{5B8}', '\u{5C1}', '\u{64B}', '\u{651}', '\u{3099}',
    ] {
        assert_eq!(
            BookMetrics::STANDARD.advance(mark, &request),
            0.0,
            "{mark:?} is measured with an advance of its own"
        );
    }
    assert_eq!(BookMetrics::STANDARD.advance('\u{3042}', &request), 10.0);
    assert!(BookMetrics::STANDARD.advance('\u{5D0}', &request) > 0.0);

    // And the overflow font's `/Widths` say the same: a mark's code is
    // written at zero, so a reader moves no pen for it either.
    let book = styled_book(
        "ar",
        "",
        &format!("<p dir=\"rtl\">{AMALIYYAN}</p><p>a\u{3099}\u{3042}</p>"),
    );
    let doc = Document::open(book).expect("the book opens");
    let page = doc.page(0).expect("a page");
    let drawn = page.text_with(&TextOptions {
        content_order: true,
    });
    for c in drawn.lines().iter().flat_map(|line| line.chars.iter()) {
        let (x0, _, x1, _) = c.quad.bounds();
        let mark = c
            .text
            .chars()
            .next()
            .is_some_and(|first| bidi_class(first) == BidiClass::NSM);
        // A glyph of no advance is read as a box a thousandth of an em wide.
        let thousandth = c.size / 1000.0 * 1.001;
        if mark {
            assert!(
                x1 - x0 <= thousandth,
                "{:?} is written {} wide",
                c.text,
                x1 - x0
            );
        } else if !c.text.trim().is_empty() {
            assert!(
                x1 - x0 > thousandth,
                "{:?} is written with no width",
                c.text
            );
        }
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

/// MELEKH pointed: MEM with SEGOL, LAMED with SEGOL, FINAL KAF with SHEVA —
/// one nonspacing mark after each letter.
const MELEKH: &str = "\u{5DE}\u{5B6}\u{5DC}\u{5B6}\u{5DA}\u{5B0}";

/// SHALOM pointed: SHIN with QAMATS and SHIN DOT, LAMED, VAV with HOLAM,
/// FINAL MEM — a letter with two marks after it.
const SHALOM: &str = "\u{5E9}\u{5B8}\u{5C1}\u{5DC}\u{5D5}\u{5B9}\u{5DD}";

/// SHALOM as both builds draw it, left to right: each letter, then its
/// points as written — inside the letter, since they have no advance.
const SHALOM_DRAWN: &str = "\u{5DD}\u{5D5}\u{5B9}\u{5DC}\u{5E9}\u{5B8}\u{5C1}";

/// KATABA with its harakat: KAF, FATHA, TEH, FATHA, BEH, FATHA.
const KATABA: &str = "\u{643}\u{64E}\u{62A}\u{64E}\u{628}\u{64E}";

/// The same word drawn, left to right, each letter followed by its FATHA.
const KATABA_DRAWN: &str = "\u{628}\u{64E}\u{62A}\u{64E}\u{643}\u{64E}";

/// Page 0's characters in the order the content stream drew them — for a
/// standard-14 run, left to right along the line — whitespace left out.
fn drawn_text(doc: &Document) -> String {
    let page = doc.page(0).expect("a page");
    page.text_with(&TextOptions {
        content_order: true,
    })
    .lines()
    .iter()
    .flat_map(|line| line.chars.iter())
    .flat_map(|c| c.text.chars())
    .filter(|c| !c.is_whitespace())
    .collect()
}

/// That every nonspacing mark on page 0 is drawn inside the box of the glyph
/// it follows in the content stream — its base, for a page this painter drew
/// — as ruling 14's extraction measures boxes: a glyph of no advance is a
/// thousandth of an em wide, and its centre is what is paired.
///
/// Inside its base's box is where `text_order.rs` reads a mark with its base
/// whatever is drawn beside it; past either end of it, rounding or the glyph
/// drawn next decides.
fn marks_ride_on_their_letters(doc: &Document) {
    let page = doc.page(0).expect("a page");
    let drawn = page.text_with(&TextOptions {
        content_order: true,
    });
    for line in drawn.lines() {
        let mut base: Option<&tinker_pdf::TextChar> = None;
        for c in &line.chars {
            let mark = c
                .text
                .chars()
                .next()
                .is_some_and(|first| bidi_class(first) == BidiClass::NSM);
            if !mark {
                base = Some(c);
                continue;
            }
            let base = base.unwrap_or_else(|| panic!("{:?} rides on nothing", c.text));
            let (x0, _, x1, _) = c.quad.bounds();
            let centre = (x0 + x1) / 2.0;
            let (b0, _, b1, _) = base.quad.bounds();
            assert!(
                b0 < centre && centre < b1,
                "{:?} is drawn at {centre}, outside its letter {:?} from {b0} to {b1}: {:?}",
                c.text,
                base.text,
                line.text
            );
        }
    }
}

/// That page 0 of `book` holds `expected`, and that the book conserves in
/// content order and in logical order alike.
fn reads_as_written(book: &[u8], doc: &Document, expected: &str) {
    let text = doc.page(0).expect("a page").text().plain_text();
    assert!(
        text.contains(expected),
        "{expected:?} does not read as written: {text:?}"
    );
    for verdict in [
        conservation(book, doc),
        conservation_in_logical_order(book, doc),
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

/// **A pointed Hebrew word set in the standard 14 is drawn with each point
/// on its own letter, and reads back as written — a letter with two points
/// too, and one with three in a right-to-left paragraph.**
///
/// A nonspacing mark belongs to the letter before it, so the run L2 reverses
/// is reversed letter by letter, each letter keeping its points (UAX #9 rule
/// L3). Reversed a character at a time, every point left its letter: in a
/// default build `שָׁלוֹם, Hebrew.` read back `שָלׁוםֹ, Hebrew.` — the SHIN
/// DOT on the LAMED, the HOLAM on the FINAL MEM — and `מֶלֶךְ` read back
/// `מלֶךְֶ`; with `bundled-fonts` the SHIN's two points came back swapped.
///
/// Nothing shapes a standard-14 run, so where a point stands is the
/// painter's to say, and extraction (`text_order.rs`) pairs a mark with the
/// base whose box holds its centre. Both builds now draw a point after its
/// letter, with no advance, inside the letter's box. Until October 2026 a
/// default build gave a point the overflow font's letter-sized advance, and
/// its second point was read with the glyph drawn next: `שָׁלוֹם, ` read
/// `שָלוֹם,ׁ `, which this test asserted only the drawing of, as a limit.
#[test]
fn a_standard_14_pointed_hebrew_word_keeps_each_point_on_its_letter() {
    let melekh = styled_book("en", "", &format!("<p>{MELEKH}, <i>Hebrew</i>.</p>"));
    let doc = Document::open(melekh.clone()).expect("the book opens");
    reads_as_written(&melekh, &doc, &format!("{MELEKH}, Hebrew."));
    marks_ride_on_their_letters(&doc);

    let shalom = styled_book("en", "", &format!("<p>{SHALOM}, <i>Hebrew</i>.</p>"));
    let doc = Document::open(shalom.clone()).expect("the book opens");
    let drawn = drawn_text(&doc);
    assert!(
        drawn.contains(SHALOM_DRAWN),
        "a point is not drawn after its own letter: {drawn:?}"
    );
    reads_as_written(&shalom, &doc, &format!("{SHALOM}, Hebrew."));
    marks_ride_on_their_letters(&doc);

    let hashabbat = styled_book(
        "he",
        "",
        &format!("<p dir=\"rtl\">{HASHABBAT} {SHALOM}.</p>"),
    );
    let doc = Document::open(hashabbat.clone()).expect("the book opens");
    reads_as_written(&hashabbat, &doc, &format!("{HASHABBAT} {SHALOM}."));
    marks_ride_on_their_letters(&doc);
}

/// HASHABBAT pointed: HE with PATAH, SHIN with DAGESH, SHIN DOT and PATAH —
/// three marks on one letter — BET with DAGESH and QAMATS, TAV.
const HASHABBAT: &str = "\u{5D4}\u{5B7}\u{5E9}\u{5BC}\u{5C1}\u{5B7}\u{5D1}\u{5BC}\u{5B8}\u{5EA}";

/// **An Arabic word with its harakat, in a right-to-left paragraph set in the
/// standard 14, is drawn with each haraka after and on its own letter, and
/// reads back as written.**
///
/// The pointed Hebrew word's rule, in the other script that writes
/// nonspacing marks over its letters: reversed a character at a time, each
/// FATHA was drawn before its letter and `كَتَبَ كتب.` read back
/// `كتَبَ َكتب.`, the first word's last FATHA thrown onto the second. No
/// Liberation face has an Arabic letter, so both builds draw this with the
/// overflow font. Its harakat were as wide as a letter until October 2026,
/// each with its centre exactly as far from its letter as from the glyph
/// drawn next, and read with its letter only where that tie rounded its way;
/// with no advance, each is drawn inside its letter.
#[test]
fn a_standard_14_arabic_word_keeps_each_haraka_on_its_letter() {
    let body = format!("<p dir=\"rtl\">{KATABA} \u{643}\u{62A}\u{628}.</p>");
    let bytes = styled_book("ar", "", &body);
    let doc = Document::open(bytes.clone()).expect("the book opens");

    let drawn = drawn_text(&doc);
    assert!(
        drawn.contains(KATABA_DRAWN),
        "a haraka is not drawn after its own letter: {drawn:?}"
    );
    reads_as_written(&bytes, &doc, &format!("{KATABA} \u{643}\u{62A}\u{628}."));
    marks_ride_on_their_letters(&doc);
}

/// The sentence `sample-regime-anticancer-arabic.epub` stopped conserving
/// at: `عمليًّا، تتألّف كلّ المواد من ذرّات.` — YEH with SHADDA and then
/// FATHATAN, two marks on one letter, then a SHADDA on a letter of each of
/// three words.
const AMALIYYAN: &str = "\u{639}\u{645}\u{644}\u{64A}\u{651}\u{64B}\u{627}\u{60C} \
    \u{62A}\u{62A}\u{623}\u{644}\u{651}\u{641} \u{643}\u{644}\u{651} \
    \u{627}\u{644}\u{645}\u{648}\u{627}\u{62F} \u{645}\u{646} \
    \u{630}\u{631}\u{651}\u{627}\u{62A}.";

/// MUDDA and MUALLIM: a DAL with SHADDA and FATHA, and a LAM with SHADDA and
/// KASRA — the two other stacks a SHADDA most often heads.
const MUDDA_MUALLIM: &str =
    "\u{645}\u{64F}\u{62F}\u{651}\u{64E}\u{629} \u{645}\u{64F}\u{639}\u{644}\u{651}\u{650}\u{645}.";

/// **An Arabic letter carrying two marks keeps both, in a paragraph of
/// either direction** — the shape `sample-regime-anticancer-arabic.epub`
/// stopped conserving at in CI's `epub-corpus` job (run 38041540464, a
/// default build): `Missing at 287: "ي\u{651}\u{64b}ا،تتأل\u{651}ف…"`,
/// `Extra at 287: "\u{64b}ي\u{651}ا،تتأل\u{651}ف…"`, the FATHATAN read before
/// its letter.
///
/// No face covers Arabic, so the overflow font draws it in both builds, and
/// it measured a mark at `Standard14::advance`'s space width: a haraka was a
/// glyph as wide as a letter, drawn after its letter. Ruling 14's extraction
/// pairs a mark with the base its centre lies in or nearest; the first of
/// two marks lay a letter's width past its letter, at a tie, and the second
/// two widths past, nearer the glyph drawn next — the letter before, in a
/// right-to-left line — and was read with it. The book conserved on 26
/// September, before ruling 14 read a line back from where it is drawn.
///
/// A mark has no advance now (`paint::standard_width`), in the layout's
/// measure and the overflow font's `/Widths` alike, and is drawn after its
/// letter, inside the letter's box: in a `dir="rtl"` paragraph, in one with
/// no `dir` whose text is Arabic, and quoted in an English sentence.
#[test]
fn a_standard_14_arabic_letter_with_two_marks_keeps_both() {
    for (language, body, expected) in [
        (
            "ar",
            format!("<p dir=\"rtl\">{AMALIYYAN}</p>"),
            AMALIYYAN.to_owned(),
        ),
        ("ar", format!("<p>{AMALIYYAN}</p>"), AMALIYYAN.to_owned()),
        (
            "en",
            format!("<p>It reads {AMALIYYAN} in the book.</p>"),
            format!("It reads {AMALIYYAN} in the book."),
        ),
        (
            "ar",
            format!("<p dir=\"rtl\">{MUDDA_MUALLIM}</p>"),
            MUDDA_MUALLIM.to_owned(),
        ),
        (
            "ar",
            format!("<p>{MUDDA_MUALLIM}</p>"),
            MUDDA_MUALLIM.to_owned(),
        ),
    ] {
        let bytes = styled_book(language, "", &body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        reads_as_written(&bytes, &doc, &expected);
        marks_ride_on_their_letters(&doc);
    }
}

/// **The overflow font's letter at code 32 takes no word spacing, so nothing
/// after it in its string moves under `word-spacing`, and a mark rides on
/// its letter when that letter is one of its word's** (ROADMAP CD-19).
///
/// `paint::OVERFLOW_FIRST` is 32, so the first character a face meets past
/// `WinAnsiEncoding` is drawn at code 32, and 9.3.3 applies `Tw` to every
/// single-byte code 32, whatever glyph the font draws there. Written at the
/// run's `Tw`, as it was until October 2026, the overflow font drew every
/// glyph after its code 32 in a string a word spacing from where layout put
/// it: `مدة معلم.` at `0.5em` read `مدةم علم.`, present since the overflow
/// font, and where a run ends inside a word the moved glyph was drawn on the
/// next run's first — `x <b>αλ</b>φα аб y.` at `word-spacing: 4px` drew its
/// λ and its φ both at `60pt` in a default build, whose overflow font draws
/// Greek. It is written at `0 Tw` now, as a text object of its own or as a
/// `PieceText::UnspacedCodes` piece of one, so both read as written and draw
/// no glyph over another. Pointed, a mark placed from layout's pen (6d79fa4)
/// left a letter so moved: in a bold run whose alpha is its face's code 32,
/// `αλφα\u{301}` beside a Hebrew word read `αλφαy\u{301}` at a word spacing
/// of `-2px`, where cd407d5, drawing the mark in its letter's string, read
/// it whole (round 3 of the marks fix); it reads whole at either sign.
#[test]
fn a_standard_14_overflow_letter_at_code_32_takes_no_word_spacing() {
    let unpointed = "\u{645}\u{62F}\u{629} \u{645}\u{639}\u{644}\u{645}.";
    let greek = "x \u{3B1}\u{3BB}\u{3C6}\u{3B1} \u{430}\u{431} y.";
    for (language, style, body, expected) in [
        (
            "ar",
            "p { word-spacing: 0.5em }",
            format!("<p dir=\"rtl\">{unpointed}</p>"),
            unpointed,
        ),
        (
            "en",
            "p { word-spacing: 4px }",
            "<p>x <b>\u{3B1}\u{3BB}</b>\u{3C6}\u{3B1} \u{430}\u{431} y.</p>".to_owned(),
            greek,
        ),
    ] {
        let bytes = styled_book(language, style, &body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        assert_eq!(
            doc.page(0).expect("a page").text().plain_text(),
            format!("{expected}\n"),
            "under `{style}` the overflow font's code 32 moves what follows it"
        );
        reads_as_written(&bytes, &doc, expected);
        let over = overprinted(&doc);
        assert!(
            over.is_empty(),
            "under `{style}`, {body:?} draws a glyph over another: {over:?}"
        );
    }

    // A negative word spacing at the run's `Tw` drew every glyph after the
    // code-32 letter a word spacing to the left, onto the letter before it:
    // at `-4px` the λ of a bold `αλφα` whose α is its face's code 32 was drawn
    // at the α's own origin, and the LAM of `معلم` at the origin of the MEEM
    // drawn left of it, the font's code 32 (review of CD-19's fix, 56dfd93
    // in a default build). The font is written at `0 Tw` whatever the sign.
    for spacing in ["-2px", "-4px", "-0.2em"] {
        let style = format!("p {{ word-spacing: {spacing} }}");
        for (language, body, expected) in [
            (
                "en",
                "<p>x <b>\u{3B1}\u{3BB}\u{3C6}\u{3B1}</b> y.</p>".to_owned(),
                "x \u{3B1}\u{3BB}\u{3C6}\u{3B1} y.",
            ),
            ("ar", format!("<p dir=\"rtl\">{unpointed}</p>"), unpointed),
        ] {
            let bytes = styled_book(language, &style, &body);
            let doc = Document::open(bytes.clone()).expect("the book opens");
            reads_as_written(&bytes, &doc, expected);
            let over = overprinted(&doc);
            assert!(
                over.is_empty(),
                "under `{style}`, {body:?} draws a glyph over another: {over:?}"
            );
        }
    }

    for style in ["p { word-spacing: -2px }", "p { word-spacing: 4px }"] {
        let body = format!("<p>{HET_VAV} x <b>\u{3B1}\u{3BB}\u{3C6}\u{3B1}\u{301}</b>y z.</p>");
        let bytes = styled_book("en", style, &body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        reads_as_written(
            &bytes,
            &doc,
            &format!("{HET_VAV} x \u{3B1}\u{3BB}\u{3C6}\u{3B1}\u{301}y z."),
        );
        marks_ride_on_their_letters(&doc);
    }
}

/// Every two glyphs drawn on one baseline of page 0 whose boxes overlap by
/// more than a hundredth of an em, sorted along it: a letter drawn over
/// another. Nonspacing marks, drawn inside their letters, and whitespace are
/// left out.
fn overprinted(doc: &Document) -> Vec<String> {
    let page = doc.page(0).expect("a page");
    let drawn = page.text_with(&TextOptions {
        content_order: true,
    });
    let mut boxes: Vec<(i64, f64, f64, f64, &str)> = drawn
        .lines()
        .iter()
        .flat_map(|line| line.chars.iter())
        .filter(|c| {
            !c.text.trim().is_empty()
                && c.text
                    .chars()
                    .next()
                    .is_none_or(|first| bidi_class(first) != BidiClass::NSM)
        })
        .map(|c| {
            let (x0, _, x1, _) = c.quad.bounds();
            // A hundredth of a point groups a baseline.
            let baseline = (c.origin.1 * 100.0).round() as i64;
            (baseline, x0, x1, c.size, c.text.as_str())
        })
        .collect();
    boxes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    boxes
        .windows(2)
        .filter_map(|pair| match pair {
            [before, after] if before.0 == after.0 && after.1 < before.2 - 0.01 * after.3 => {
                Some(format!(
                    "{:?} from {:.3} to {:.3} under {:?} from {:.3}",
                    before.4, before.1, before.2, after.4, after.1
                ))
            }
            _ => None,
        })
        .collect()
}

/// **No letter of a pointed Arabic paragraph is drawn over another under
/// `word-spacing` or `text-align: justify`, in a paragraph of either
/// direction** (re-review of f529a47, BLOCKING).
///
/// The overflow font's code 32 — `MUDDA`'s MEM, the first character past
/// `WinAnsiEncoding` the face meets — took `Tw` (9.3.3) while the font was
/// written at the run's word spacing, so the glyph after it in its string
/// was drawn a word spacing right of where layout put it. At cd407d5 the
/// rest of the string moved with it and the shift ended in the word space
/// after it, which is whitespace. A mark ends its string now and the glyph
/// after the marked letter is a piece of its own, drawn where layout put it,
/// so the moved LAM was drawn on the AIN after it: `مُدَّةَ مُعلِّم.` at
/// `word-spacing: 4px` drew both at `48pt`, and a justified paragraph of the
/// two words ten to fourteen such pairs a page. The glyph after the code 32
/// was then made a piece of its own too, drawn where layout put it; the
/// overflow font is written at `0 Tw` now (ROADMAP CD-19), so nothing it
/// draws moves and the glyph stays in its string.
#[test]
fn a_standard_14_pointed_arabic_letter_is_not_drawn_over_the_next_under_word_spacing() {
    let long: String = (0..14)
        .map(|_| format!("{MUDDA_MUALLIM} \u{643}\u{62A}\u{628} "))
        .collect();
    for style in [
        "p { word-spacing: 4px }",
        "p { word-spacing: 0.5em }",
        "p { word-spacing: 1em }",
        "p { letter-spacing: 0.25em; word-spacing: 0.25em }",
        "p { text-align: justify }",
        "p { text-align: justify; margin: 0 30% }",
        "p { text-align: justify; letter-spacing: 2px; word-spacing: 6px }",
    ] {
        for body in [
            format!("<p dir=\"rtl\">{MUDDA_MUALLIM}</p>"),
            format!("<p>{MUDDA_MUALLIM}</p>"),
            format!("<p dir=\"rtl\">{long}</p>"),
            format!("<p>{long}</p>"),
            format!("<p dir=\"rtl\">{AMALIYYAN} {AMALIYYAN} {AMALIYYAN}</p>"),
        ] {
            let bytes = styled_book("ar", style, &body);
            let doc = Document::open(bytes).expect("the book opens");
            let over = overprinted(&doc);
            assert!(
                over.is_empty(),
                "under `{style}`, {body:?} draws a letter over another: {over:?}"
            );
            marks_ride_on_their_letters(&doc);
        }
    }
}

/// Latin, Greek and Cyrillic with their accents decomposed: an acute, a
/// diaeresis, a circumflex and an acute stacked on one `o`, a macron on an
/// `ǫ` (a letter `WinAnsiEncoding` lacks, so it and its mark share a font), an
/// acute on an alpha and on a Cyrillic `а`.
const DECOMPOSED: &str = "cafe\u{301} na\u{308}ive o\u{302}\u{301} \u{1EB}\u{304} \
    \u{3B1}\u{301}\u{3BB}\u{3C6}\u{3B1} \u{430}\u{301}\u{431}";

/// **A decomposed accent set in the standard 14 reads with its own letter,
/// on a left-to-right line, beside a right-to-left word, and inside a
/// right-to-left paragraph.**
///
/// A left-to-right slice keeps a mark after its letter, as written. With no
/// advance and drawn where the pen stood after its letter, a mark's box —
/// a thousandth of an em running right — lay in the next glyph's, and on a
/// line ruling 14 reorders (one holding a right-to-left character) it was
/// read with that glyph: `na\u{308}ive` beside `חו` came back `nai\u{308}ve`.
/// A default build gave the overflow font's marks a space's advance instead,
/// which was a tie: `o\u{302}\u{301} ` came back `o\u{302} \u{301}`, the
/// second accent on the space. Drawn inside its letter, every accent here is
/// read with it. A line with nothing right-to-left is read in content order
/// and was right either way; it is here so a fix for the other two cannot
/// cost it.
#[test]
fn a_standard_14_decomposed_accent_keeps_to_its_letter() {
    for (language, body, expected) in [
        (
            "en",
            format!("<p>{DECOMPOSED} end.</p>"),
            format!("{DECOMPOSED} end."),
        ),
        (
            "en",
            format!("<p>{DECOMPOSED} {HET_VAV} {DECOMPOSED}.</p>"),
            format!("{DECOMPOSED} {HET_VAV} {DECOMPOSED}."),
        ),
        (
            "he",
            format!("<p dir=\"rtl\">{HET_VAV} {DECOMPOSED} {VAV_HET}.</p>"),
            format!("{HET_VAV} {DECOMPOSED} {VAV_HET}."),
        ),
    ] {
        let bytes = styled_book(language, "", &body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        reads_as_written(&bytes, &doc, &expected);
        marks_ride_on_their_letters(&doc);
    }
}

/// **A decomposed accent whose letter is drawn by another run reads with
/// its letter, on a line beside a right-to-left word** — styled apart from
/// it, or left to the standard 14 by a face of the book's own that draws
/// its letter and has no glyph for it.
///
/// Such a mark opens a standard-14 slice and rides on nothing in it. Drawn
/// where the pen stood — its letter's end — its box lay in the next glyph's,
/// and `x<b>e</b>\u{301}x` read back `xex\u{301}`; a default build's overflow
/// font, as wide as a letter, was a tie there. In a left-to-right run it is
/// now drawn just inside where its letter ended.
#[test]
fn a_standard_14_accent_drawn_apart_from_its_letter_keeps_to_it() {
    let styled = styled_book(
        "en",
        "",
        &format!("<p>{HET_VAV} x<b>e</b>\u{301}x <i>a</i>\u{308}y.</p>"),
    );
    let latin = covering("Fixture Latin", "abcdefghijklmnopqrstuvwxyz .");
    let own_face = faces_book(
        &[("Fixture Latin", &latin)],
        16,
        &format!("cafe\u{301} {HET_VAV} na\u{308}ive."),
    );
    for (bytes, expected) in [
        (own_face, format!("cafe\u{301} {HET_VAV} na\u{308}ive.")),
        (styled, format!("{HET_VAV} xe\u{301}x a\u{308}y.")),
    ] {
        let doc = Document::open(bytes.clone()).expect("the book opens");
        reads_as_written(&bytes, &doc, &expected);
        marks_ride_on_their_letters(&doc);
    }
}

/// **A mark styled apart from its letter in a right-to-left run is drawn
/// where the pen stands, and read with the glyph drawn before it — the
/// known limit `epub.md`'s `direction` row names, pinned so that ending it
/// is noticed.**
///
/// `<b>ח</b>ָו`: the QAMATS opens the second run and rides on nothing in
/// it. L2 draws it at that run's right end, which is where its letter
/// starts, but ruling 14 pairs a mark only with a neighbour in the content
/// stream, and the stream wrote the bold run before this one, beside its
/// other end: the mark is read with the VAV drawn just before it, as it was
/// before October 2026, in both builds. When this fails because the line
/// reads `א חָו ב.`, the limit is gone: drop it from `epub.md` and make this
/// the test that the line reads as written.
#[test]
fn a_standard_14_right_to_left_mark_styled_apart_rides_on_nothing() {
    let body = "<p dir=\"rtl\">\u{5D0} <b>\u{5D7}</b>\u{5B8}\u{5D5} \u{5D1}.</p>";
    let doc = Document::open(styled_book("he", "", body)).expect("the book opens");
    let text = doc.page(0).expect("a page").text().plain_text();
    assert!(
        text.contains("\u{5D0} \u{5D7}\u{5D5}\u{5B8} \u{5D1}."),
        "the known limit no longer reads the QAMATS with the VAV: {text:?}"
    );
}

/// **A mark set in the standard 14 reads with its letter at every size and
/// under `letter-spacing` of either sign — a letter's three and four marks
/// too, and past half an em.**
///
/// The two limits `epub.md` named until October 2026, each measured on
/// synthetic books (review of 6d08c6b): a default build's lone point, drawn
/// after its letter at a tie, read `a מֶלֶךְ,` as `a מלֶךְ,ֶ` at a
/// `font-size` of `12.5px` and `14px`; and `letter-spacing`, which was added
/// after a mark as after any character, moved a point drawn where its letter
/// starts off the letter — `שָׁלוֹם` at `0.5px` read `שׁלָוֹם` and `מֶלֶךְ` at
/// `-0.5px` `מלֶךְֶ` with `bundled-fonts`. A mark drawn inside its letter's
/// box is read with it whatever the size, and whatever the spacing puts
/// between the letter and the glyph after it.
///
/// **And past a few pixels of spacing, which 6d79fa4 lost** (its review):
/// that commit drew each mark in a text object of its own and spaced it like
/// a letter, so the glyph after a letter with `n` marks started `(n + 1)`
/// spacings and nine thousandths of an em past where a reader's pen stopped,
/// and a reader resumes a line only half an em on — a quarter of an em of
/// spacing cut a word with one mark into a line a letter, a tenth of an em
/// one with four. A slice holding a mark is one text object now, and a mark
/// is not spaced (`css-text-3` §10.2), so a word of any marks reads whole at
/// any spacing. `5px`, `8px`, `0.3em`, `0.6em` and `1em` are the review's.
/// Past half an em a **run** boundary cuts a line, marks or none — a reader
/// resumes a line across an `ET` only half an em on, the millionth after a
/// mark and a stand-in's half-thousandth `/W` rounding aside (`epub.md`), and
/// at half an em exactly the last place of a float decides (`<i>Hebrew</i>.` is cut
/// before its full stop at `8px` with `bundled-fonts`) — so there the text is
/// asserted with its whitespace left out, the order of every character that
/// is not a space: what each cut-off piece reads as on its own, and in what
/// order the pieces come.
///
/// HASHABBAT's SHIN carries three marks; [`CANTILLATED`]'s four. At cd407d5
/// the four-mark SHIN lost a mark to the glyph drawn next at every size and
/// spacing, the overflow font's marks being as wide as a letter, and at
/// 6d79fa4 its line was cut from `1.7px` (`0.1em`) of spacing, five spacings
/// and nine thousandths of an em past the last mark's box.
#[test]
fn a_standard_14_mark_reads_with_its_letter_at_every_size_and_spacing() {
    let books = [
        (
            "en",
            format!("<p>a {MELEKH}, <i>Hebrew is a language</i>.</p>"),
            format!("a {MELEKH}, Hebrew is a language."),
        ),
        (
            "en",
            format!("<p>{SHALOM}, <i>Hebrew</i>.</p>"),
            format!("{SHALOM}, Hebrew."),
        ),
        (
            "ar",
            format!("<p dir=\"rtl\">{AMALIYYAN}</p>"),
            AMALIYYAN.to_owned(),
        ),
        (
            "en",
            format!("<p>{DECOMPOSED} {HET_VAV}.</p>"),
            format!("{DECOMPOSED} {HET_VAV}."),
        ),
        (
            "he",
            format!("<p dir=\"rtl\">{HASHABBAT} {CANTILLATED} {SHALOM}.</p>"),
            format!("{HASHABBAT} {CANTILLATED} {SHALOM}."),
        ),
        (
            "he",
            format!("<p>{CANTILLATED} {HASHABBAT}.</p>"),
            format!("{CANTILLATED} {HASHABBAT}."),
        ),
        (
            "en",
            format!("<p>so {CANTILLATED} and {HASHABBAT}, <i>quoted</i>.</p>"),
            format!("so {CANTILLATED} and {HASHABBAT}, quoted."),
        ),
    ];
    // Every half pixel from 9 to 24.5, the sweep the limit was measured on.
    let sizes = (18..=49).map(|half| {
        (
            format!("p {{ font-size: {}px }}", f64::from(half) / 2.0),
            false,
        )
    });
    // A paragraph's font size is 16px, so `8px` is half an em exactly, where
    // a run boundary is left whole or cut by a floating-point last place.
    let spacings = [
        ("0.5px", false),
        ("2px", false),
        ("-0.5px", false),
        ("-1px", false),
        ("1.7px", false),
        ("0.1em", false),
        ("5px", false),
        ("8px", true),
        ("0.3em", false),
        ("0.6em", true),
        ("1em", true),
    ]
    .into_iter()
    .map(|(spacing, wide)| (format!("p {{ letter-spacing: {spacing} }}"), wide));
    let squeezed = |text: &str| -> String { text.chars().filter(|c| !c.is_whitespace()).collect() };
    for (style, wide) in sizes.chain(spacings) {
        for (language, body, expected) in &books {
            let bytes = styled_book(language, &style, body);
            let doc = Document::open(bytes.clone()).expect("the book opens");
            let text = doc.page(0).expect("a page").text().plain_text();
            if wide {
                assert!(
                    squeezed(&text).contains(&squeezed(expected)),
                    "under `{style}`, {expected:?} does not read in order: {text:?}"
                );
            } else {
                assert!(
                    text.contains(expected.as_str()),
                    "under `{style}`, {expected:?} does not read as written: {text:?}"
                );
            }
            for verdict in [
                conservation(&bytes, &doc),
                conservation_in_logical_order(&bytes, &doc),
            ] {
                assert!(
                    verdict.holds(),
                    "under `{style}`: {} extra, {} missing, {:?}",
                    verdict.extra,
                    verdict.missing,
                    verdict.divergences
                );
            }
            marks_ride_on_their_letters(&doc);
        }
    }
}

/// SHIN with DAGESH, SHIN DOT, QAMATS and ETNAHTA — four marks on one
/// letter, the fourth a cantillation mark — then LAMED with TSERE, and
/// FINAL MEM.
const CANTILLATED: &str = "\u{5E9}\u{5BC}\u{5C1}\u{5B8}\u{591}\u{5DC}\u{5B5}\u{5DD}";

/// **A pointed word set with `letter-spacing` reads whole, on one line** —
/// the two books 6d79fa4's review cut into a line a letter (BLOCKING).
///
/// 6d79fa4 drew each mark of no advance in a text object of its own, inside
/// its letter, and the pen still moved `letter-spacing` after the letter and
/// again after the mark: the glyph after a pointed letter started two
/// spacings and nine thousandths of an em past where the mark's box — a
/// thousandth of an em — had left a reader's pen. `tinker-pdf-content`'s
/// `TextDevice` resumes a line closed by an `ET` only within half an em of
/// that pen, and ruling 14's rejoin allows the same half em, so from a
/// quarter of an em on (`5px` at `16px`) each pointed letter was a line of
/// its own and an RTL word came back letter by letter: `The word ךְ\nלֶ\nמֶ\n,
/// quoted, ends.`, and `بَ.\nتَ\nكَ\nبَ \nتَ\nكَ`. At cd407d5 both read as
/// written, in both builds.
///
/// Now a slice holding a mark is one text object, the mark and what follows
/// it moved to with `Td` (`PageBuilder::text_pieces`), and a mark takes no
/// `letter-spacing` of its own (`css-text-3` §10.2): the comma after the
/// word, a run of its own, starts one spacing past the word, as after a word
/// with no marks.
#[test]
fn a_standard_14_pointed_word_under_letter_spacing_reads_on_one_line() {
    for (language, body, expected) in [
        (
            "en",
            format!("<p>The word {MELEKH}, <i>quoted</i>, ends.</p>"),
            format!("The word {MELEKH}, quoted, ends.\n"),
        ),
        (
            "ar",
            format!("<p dir=\"rtl\">{KATABA} {KATABA}.</p>"),
            format!("{KATABA} {KATABA}.\n"),
        ),
    ] {
        let bytes = styled_book(language, "p { letter-spacing: 5px }", &body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let text = doc.page(0).expect("a page").text().plain_text();
        assert_eq!(text, expected, "the line was cut");
        reads_as_written(&bytes, &doc, &expected);
        marks_ride_on_their_letters(&doc);
    }
}

/// **A decomposed accent is found by search under `letter-spacing`, on a
/// line with nothing right to left in it** (review of 6d79fa4).
///
/// Conservation leaves whitespace out, and a line cut in two reads the same
/// to it. A search does not: at 6d79fa4 a default build read
/// `Nguye\u{302}\u{303}n Tha\u{300}nh` as `Nguye\u{302}\u{303}` on one line
/// and `n Tha\u{300}nh …` on the next from `3px` of spacing on, and
/// `search("Nguye\u{302}\u{303}n")` found nothing — the `n` started three
/// spacings past the second mark's box, in a text object of its own. At
/// cd407d5 the search found its word at no spacing, `3px`, `0.2em` and
/// `5px`, and it does again: the marks and the `n` are one text object.
#[test]
fn a_standard_14_decomposed_accent_is_found_under_letter_spacing() {
    const SENTENCE: &str = "Nguye\u{302}\u{303}n Tha\u{300}nh ca\u{301}c ba\u{323}n.";
    assert!(
        !SENTENCE
            .chars()
            .any(|c| matches!(bidi_class(c), BidiClass::R | BidiClass::AL)),
        "the line is to hold nothing right to left, so ruling 14 leaves it alone"
    );
    for style in [
        "",
        "p { letter-spacing: 3px }",
        "p { letter-spacing: 0.2em }",
        "p { letter-spacing: 5px }",
    ] {
        let bytes = styled_book("vi", style, &format!("<p>{SENTENCE}</p>"));
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let page = doc.page(0).expect("a page");
        let text = page.text();
        assert_eq!(
            text.search("Nguye\u{302}\u{303}n").len(),
            1,
            "under `{style}` the word is not found: {:?}",
            text.plain_text()
        );
        assert_eq!(
            text.plain_text(),
            format!("{SENTENCE}\n"),
            "under `{style}` the line was cut"
        );
        reads_as_written(&bytes, &doc, SENTENCE);
        marks_ride_on_their_letters(&doc);
    }
}

/// **A letter and its marks are spaced once** (`css-text-3` §10.2): a
/// pointed word under `letter-spacing` takes the room the same word
/// unpointed does, in layout and on the page, in either build.
///
/// `letter-spacing` goes between typographic character units, and a letter
/// with the nonspacing marks after it is one. Layout added it after every
/// character, a mark too, and the painter moved its pen to match: at `4px`,
/// `מֶלֶךְ` was twelve pixels wider than `מלך` and a SHIN with four marks
/// sixteen wider than one with none — a gap after each pointed letter that
/// nothing drew in, and the gap a reader cut the line at. Now neither moves
/// the pen for a mark (`Metrics::letter_spaced`), so the `b` after each
/// word starts where it starts after the word unpointed — layout's answer —
/// and so does the word's first letter, drawn last and rightmost, after the
/// letters and marks left of it — the painter's.
#[test]
fn a_standard_14_letter_and_its_marks_are_spaced_once() {
    for (pointed, plain, first) in [
        (MELEKH, "\u{5DE}\u{5DC}\u{5DA}", "\u{5DE}"),
        (CANTILLATED, "\u{5E9}\u{5DC}\u{5DD}", "\u{5E9}"),
        (KATABA, "\u{643}\u{62A}\u{628}", "\u{643}"),
    ] {
        let at = |word: &str| -> (f64, f64) {
            let bytes = styled_book(
                "en",
                "p { letter-spacing: 4px }",
                &format!("<p>a {word} b.</p>"),
            );
            let doc = Document::open(bytes).expect("the book opens");
            (drawn_at(&doc, first), drawn_at(&doc, "b"))
        };
        let ((pointed_first, pointed_b), (plain_first, plain_b)) = (at(pointed), at(plain));
        assert!(
            (pointed_b - plain_b).abs() < 1e-6,
            "{pointed:?} moves what follows it to {pointed_b}, {plain:?} to {plain_b}"
        );
        // A hundredth of a point, where a spacing is three: with
        // `bundled-fonts` the unpointed word is one string, its letters set
        // by the stand-in's `/W`, rounded to a thousandth of an em, and the
        // pointed word's letters are each placed from the pen after a mark.
        assert!(
            (pointed_first - plain_first).abs() < 0.01,
            "{pointed:?} draws its first letter at {pointed_first}, {plain:?} at {plain_first}"
        );
    }
}

/// The text of each line of page 0, in the order the content stream drew
/// it: where a reader's pen was put down and not taken up again, a line
/// ends.
fn drawn_lines(doc: &Document) -> Vec<String> {
    let page = doc.page(0).expect("a page");
    page.text_with(&TextOptions {
        content_order: true,
    })
    .lines()
    .iter()
    .map(|line| line.chars.iter().map(|c| c.text.as_str()).collect())
    .collect()
}

/// **A run that ends on a mark is not cut from what follows it up to half an
/// em of `letter-spacing`, half an em included, in either build, whatever
/// draws its letter — an alpha, a Cyrillic `а`, an eng and a schwa (the
/// Liberation stand-in's with `bundled-fonts`, the overflow font's without)
/// and an `e` — and nor is a mark styled apart from its letter; below half
/// an em each line reads whole, and so does the four-mark SHIN ending a bold
/// run inside a left-to-right line** (review of bf081ca, BLOCKING).
///
/// A text object that ends with a mark leaves a reader's pen where the
/// mark's box, a thousandth of an em, ends, and `TextDevice` resumes the line
/// in the next object only within half an em of it. bf081ca drew a stand-in
/// letter's mark a hundredth of an em inside the letter's end, since the
/// stand-in's `/W` rounds what layout measured to a whole thousandth: the
/// box ended nine thousandths short and the next run, one spacing past the
/// letter, was cut from `0.491em`: `x <b>α\u{301}</b>y z.` read
/// `x α\u{301}\ny z.` at `0.495em` and `8px`, where cd407d5 read it whole.
/// Now a stand-in letter that carries a mark is a piece of its own, drawn
/// where layout put it, and every mark on a letter in its slice is placed as
/// on a simple font's, its box ending a millionth of an em past where layout
/// measured the letter to end (`paint::EXACT_MARK_INSET`): the next run is a
/// spacing less a millionth of an em away, within half an em at half an em.
/// A mark that opens a run, its letter in the run before — styled apart, or
/// a sans-serif letter whose mark only the serif fallback draws — is drawn a
/// hundredth inside where that letter ended until the spacing nears half an
/// em, and then nearer its end (`paint::leading_mark_at`):
/// `x<b>e</b><i>\u{301}</i>x y.` was cut from `0.491em` too, and a
/// sans-serif `Nguye\u{302}\u{303}n` at half an em. At cd407d5 each of these
/// read whole below half an em.
///
/// **At exactly half an em only the boundary after the mark is asserted.**
/// Every other run boundary on these lines — `x ` before the bold run, the
/// SHIN's word before ` b.` — is a gap a spacing wide that has no mark at
/// it, and at a spacing of half an em it is cut or not by the last place of
/// the two sums that put its ends, in either build and at cd407d5, marks or
/// none, where a left-to-right run stands beside it (class A in `epub.md`).
#[test]
fn a_standard_14_run_ending_on_a_mark_keeps_to_what_follows_to_half_an_em() {
    // At `16px`, `7.84px` is `0.49em` and `7.92px` `0.495em`; the last three
    // are half an em exactly.
    let spacings = [
        ("p { letter-spacing: 7.84px }", false),
        ("p { letter-spacing: 7.92px }", false),
        ("p { letter-spacing: 0.4999em }", false),
        ("p { letter-spacing: 8px }", true),
        ("p { letter-spacing: 0.5em }", true),
        ("p { font-size: 6px; letter-spacing: 3px }", true),
    ];
    let mut books: Vec<(String, String, String)> =
        ['e', '\u{3B1}', '\u{430}', '\u{14B}', '\u{259}']
            .iter()
            .map(|letter| {
                (
                    format!("<p>x <b>{letter}\u{301}</b>y z.</p>"),
                    format!("x {letter}\u{301}y z."),
                    format!("{letter}\u{301}y"),
                )
            })
            .collect();
    books.push((
        "<p>x<b>e</b><i>\u{301}</i>x y.</p>".to_owned(),
        "xe\u{301}x y.".to_owned(),
        "e\u{301}x".to_owned(),
    ));
    books.push((
        "<p style=\"font-family: sans-serif\">Nguye\u{302}\u{303}n Tha\u{300}nh.</p>".to_owned(),
        "Nguye\u{302}\u{303}n Tha\u{300}nh.".to_owned(),
        "e\u{302}\u{303}n".to_owned(),
    ));
    for (style, exactly_half) in spacings {
        for (body, expected, across) in &books {
            let bytes = styled_book("en", style, body);
            let doc = Document::open(bytes.clone()).expect("the book opens");
            for verdict in [
                conservation(&bytes, &doc),
                conservation_in_logical_order(&bytes, &doc),
            ] {
                assert!(
                    verdict.holds(),
                    "under `{style}`, {body:?}: {:?}",
                    verdict.divergences
                );
            }
            marks_ride_on_their_letters(&doc);
            let lines = drawn_lines(&doc);
            assert!(
                lines.iter().any(|line| line.contains(across.as_str())),
                "under `{style}` the run ending on a mark is cut from what follows: {lines:?}"
            );
            if !exactly_half {
                assert_eq!(
                    doc.page(0).expect("a page").text().plain_text(),
                    format!("{expected}\n"),
                    "under `{style}`"
                );
            }
        }
        if !exactly_half {
            let shin =
                "<p>a <b>\u{5E9}\u{5BC}\u{5C1}\u{5B8}\u{591}</b>\u{5DC}\u{5B5}\u{5DD} b.</p>";
            let bytes = styled_book("en", style, shin);
            let doc = Document::open(bytes.clone()).expect("the book opens");
            reads_as_written(&bytes, &doc, &format!("a {CANTILLATED} b."));
            marks_ride_on_their_letters(&doc);
        }
    }
}

/// **Past half an em of `letter-spacing` a run that ends on a mark is cut
/// there, as a run that ends on its letter alone is — which, with
/// `bundled-fonts`, cd407d5 read whole up to a thousandth of an em further:
/// a known limit (class A of the marks fix's round 4, the band at half an
/// em), pinned so that ending it is noticed.**
///
/// A mark's box ends a millionth of an em past where layout measured its
/// letter to end (`paint::EXACT_MARK_INSET`), so the next run is a spacing
/// less a millionth from it, and past half an em and a millionth
/// `TextDevice` starts a new line. After the same letter alone the reader's
/// pen is where the letter's drawn box ends: where layout measured it to,
/// for a simple font's letter, whose widths are the numbers layout measured
/// with, and up to half a thousandth of an em either side of that for a
/// letter the stand-in draws, whose `/W` rounds layout's width to a whole
/// thousandth — so with `bundled-fonts` a run ending on a mark and the same
/// run ending on its letter alone can be cut differently by up to that much
/// either side of half an em (`0.5001em`: `x <b>ə</b>y z.` reads
/// `x \nəy z.` and `x <b>ə\u{301}</b>y z.` reads `x \nə\u{301}\ny z.`, the
/// schwa's `/W` rounding layout's width up by a ten-thousandth of an em or
/// more). cd407d5 spaced a mark like a letter and,
/// with `bundled-fonts`, drew the stand-in's mark where the pen stood after
/// its letter, so the mark's thousandth-of-an-em box lay a spacing past the
/// letter and the next run a spacing past that: the line was cut only past
/// half an em **and a thousandth**, less what the stand-in's `/W` rounded
/// off. So from `0.5em` to `0.501em` that run boundary was whole there and is
/// cut now: `x <b>α\u{301}</b>y z.` at `8.01px` (`0.500625em`) read
/// `x \nα\u{301}y z.` and reads `x \nα\u{301}\ny z.`, and `x <b>α</b>y z.`
/// reads `x \nα\ny z.` at both. A default build drew the mark as wide as a
/// space and cut it as now. On the round-4 probe this band holds every
/// plain-text, search and left-to-right-token regression against cd407d5
/// with `bundled-fonts` that is not at half an em exactly.
#[test]
fn a_standard_14_run_ending_on_a_mark_is_cut_past_half_an_em_as_its_letter_is() {
    // `8.01px` is `0.500625em`. At `0.5001em` an `e`, whose width is the
    // standard 14's exactly, is cut as it is unpointed; the stand-in's alpha
    // is not asked there, since its `/W` moves its own end by up to half a
    // thousandth of an em either way.
    for (style, letters) in [
        ("p { letter-spacing: 8.01px }", &['e', '\u{3B1}'][..]),
        ("p { letter-spacing: 0.5001em }", &['e'][..]),
    ] {
        for letter in letters {
            for (body, expected) in [
                (
                    format!("<p>x <b>{letter}\u{301}</b>y z.</p>"),
                    format!("x \n{letter}\u{301}\ny z.\n"),
                ),
                (
                    format!("<p>x <b>{letter}</b>y z.</p>"),
                    format!("x \n{letter}\ny z.\n"),
                ),
            ] {
                let bytes = styled_book("en", style, &body);
                let doc = Document::open(bytes).expect("the book opens");
                assert_eq!(
                    doc.page(0).expect("a page").text().plain_text(),
                    expected,
                    "under `{style}` the line is no longer cut past half an em: \
                     if a mark now keeps it whole, say how far in epub.md"
                );
            }
        }
    }
}

/// KATABA and the unpointed word, alternating, fourteen times: two words of
/// three letters each, so that a line read in another order than written
/// does not conserve.
fn kataba_paragraph(first: &str) -> String {
    let words: String = (0..14)
        .map(|_| format!("{first} \u{643}\u{62A}\u{628} "))
        .collect();
    format!("<p dir=\"rtl\">{words}</p>")
}

/// **A right-to-left line letter-spaced by exactly half an em reads whole,
/// marks or none** — what was class A of the marks fix's round 4, a known
/// limit pinned so that ending it would be noticed, and ended by the review
/// of the K-11 fix.
///
/// `TextDevice` resumes a line across an `ET` only where the next glyph
/// starts within half an em of where the pen stood, and at a
/// `letter-spacing` of half an em every run boundary puts it half an em on:
/// the box's end is the reader's sum (the `Td`, plus the glyph's width) and
/// the next glyph's start is the painter's (its own `Td`), and two sums of
/// one value differ in their last place. In this paragraph, sans-serif at
/// `0.5em` (`6pt`), a word space is drawn at `99.31199999999995` and ends,
/// `3.336` wide, at `102.64799999999995`, and the next word is written at
/// `108.64799999999997`: `6.000000000000014` on, past half an em by the last
/// place of the two sums. Each line was cut so before its last word, and
/// ruling 14 read the cut-off word in the order the pieces were drawn, so the
/// paragraph did not conserve — pointed, and with no mark at all (`بتك` for
/// KATABA: the same three glyph widths, so the same lines and the same sums);
/// at `0.4999em` and at `0.5001em` both conserved. Within right-to-left text
/// the pen is where `Tc` and `Tw` left it now, the next word's own start, so
/// the boundary is no gap at all. A pointed word is as wide as the same word
/// unpointed (`css-text-3` §10.2), so it is drawn at the same places as the
/// mark-free twin. What is left of the band is a boundary beside a
/// left-to-right run, which nothing carries the pen across.
#[test]
fn a_standard_14_right_to_left_line_at_exactly_half_an_em_reads_whole() {
    let read = |style: &str, first: &str| {
        let bytes = styled_book("ar", style, &kataba_paragraph(first));
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let holds = conservation(&bytes, &doc).holds()
            && conservation_in_logical_order(&bytes, &doc).holds();
        let page = doc.page(0).expect("a page");
        let starts: Vec<f64> = page
            .text_with(&TextOptions {
                content_order: true,
            })
            .lines()
            .iter()
            .flat_map(|line| line.chars.iter())
            .filter(|c| {
                c.text
                    .chars()
                    .next()
                    .is_none_or(|first| bidi_class(first) != BidiClass::NSM)
            })
            .map(|c| c.quad.bounds().0)
            .collect();
        (holds, drawn_lines(&doc).len(), starts)
    };
    for spacing in ["0.4999em", "0.5em", "0.5001em"] {
        let style = format!("p {{ font-family: sans-serif; letter-spacing: {spacing} }}");
        let pointed = read(&style, KATABA);
        let plain = read(&style, "\u{628}\u{62A}\u{643}");
        assert!(
            pointed.2.len() == plain.2.len()
                && pointed
                    .2
                    .iter()
                    .zip(&plain.2)
                    .all(|(a, b)| (a - b).abs() < 1e-9),
            "at `{spacing}` the pointed words are not drawn where the unpointed ones are"
        );
        assert_eq!(
            pointed.1, plain.1,
            "at `{spacing}` the pointed paragraph is cut in other places"
        );
        assert!(
            pointed.0 && plain.0,
            "at `{spacing}` the paragraph does not conserve: pointed {}, plain {}",
            pointed.0,
            plain.0
        );
    }
}

/// **A pointed word is as wide as the same word unpointed, so its paragraph
/// breaks its lines where the unpointed paragraph does — and a line break
/// that moved so into one of ruling 14's named limits reads as the
/// unpointed paragraph does: class B of the marks fix's round 4, a known
/// consequence pinned so that a change to it is noticed.**
///
/// `css-text-3` §10.2 spaces a letter and its marks once (bf081ca); at
/// cd407d5 each decomposed accent of `cafe\u{301} na\u{308}ive …` was spaced
/// like a letter and, in a default build, as wide as a space besides, so the
/// pointed words were wider and broke this justified, half-em-spaced
/// right-to-left paragraph elsewhere — and it conserved. Now its lines are
/// the unpointed paragraph's, and its last line, `Nguyen q o חו.`, is a
/// left-to-right run and then, drawn more than three ems left of where it
/// ended, a right-to-left word with its full stop: `TextDevice` cuts them
/// into two lines, and ruling 14 reads the two pieces of a right-to-left
/// paragraph in the order they are drawn (`epub.md`'s `direction` row), the
/// full stop and word first. The unpointed paragraph reads the same, in either build, at
/// cd407d5 and now. On the round-4 probe, 3 of a default build's 15 books
/// that conserve at cd407d5 and do not now are this class: this one, and
/// `CANTILLATED` and `كتب` fourteen times in a right-to-left paragraph at
/// `1em` (sans-serif) and at `0.5001em` (monospace), every word a piece of
/// its own, read in drawn order as `שלם كتب` fourteen times is; none with
/// `bundled-fonts`.
#[test]
fn a_narrower_pointed_word_breaks_its_line_where_the_unpointed_word_does() {
    let style = "p { text-align: justify; letter-spacing: 0.5em; margin: 0 20% }";
    let read = |latin: &str| {
        let body = format!("<p dir=\"rtl\">{HET_VAV} {latin} {HET_VAV}.</p>");
        let bytes = styled_book("he", style, &body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let holds = conservation(&bytes, &doc).holds()
            && conservation_in_logical_order(&bytes, &doc).holds();
        let text: String = doc
            .page(0)
            .expect("a page")
            .text()
            .plain_text()
            .chars()
            .filter(|c| bidi_class(*c) != BidiClass::NSM)
            .collect();
        (holds, text)
    };
    let pointed =
        read("cafe\u{301} na\u{308}ive Nguye\u{302}\u{303}n q\u{307}\u{323} o\u{302}\u{301}");
    let plain = read("cafe naive Nguyen q o");
    assert_eq!(
        pointed.1, plain.1,
        "the pointed paragraph's lines are not the unpointed one's"
    );
    assert_eq!(
        plain.1,
        format!("{HET_VAV}\ncafe naive\n.{HET_VAV} Nguyen q o\n"),
        "ruling 14 now reads the last line as written: drop class B from epub.md"
    );
    assert_eq!((pointed.0, plain.0), (false, false));
}

/// **A letter-spaced book with no mark in it is written byte for byte as at
/// cd407d5** (review of bf081ca).
///
/// bf081ca moved the pen by a character's advance and then by its
/// `letter-spacing`, two additions, where cd407d5 added their sum, and the
/// last place of every later segment's `Td` moved with it: `body {
/// letter-spacing: 0.3em }` on a Latin and Greek paragraph, the Greek in the
/// overflow font, wrote `284.9640000000001 561.204 Td` where cd407d5 wrote
/// `284.96400000000006 561.204 Td` — 97 of the review's 1 891 books with no
/// mark were other bytes, every one of them letter-spaced. The pen moves by
/// the one sum again, and the round-3 probe's 3 042 books with no mark are
/// cd407d5's bytes in either build. The overflow font is a default build's.
#[cfg(not(feature = "bundled-fonts"))]
#[test]
fn a_letter_spaced_book_with_no_mark_is_written_as_at_cd407d5() {
    let body: String = (0..20)
        .map(|_| "cafe naive Nguyen q o \u{3B1}\u{3BB}\u{3C6}\u{3B1} \u{430}\u{431} ")
        .collect();
    let bytes = styled_book(
        "en",
        "body { letter-spacing: 0.3em }",
        &format!("<p>{body}</p>"),
    );
    let doc = Document::open(bytes).expect("the book opens");
    let cos = doc.cos();
    let pages = tinker_pdf_cos::pages::collect(cos);
    let page = pages.first().expect("one page");
    let content =
        String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned();
    assert!(
        content.contains(" 284.96400000000006 561.204 Td "),
        "the overflow segment's Td is not the one cd407d5 wrote: {}",
        content.lines().take(30).collect::<Vec<_>>().join("\n")
    );
}

/// **A line of nothing but neutrals at a right-to-left level is drawn as
/// written, and reads as written.**
///
/// Ruling 14 reads a line that holds no right-to-left character in the order
/// the content stream drew it, so such a line conserves exactly when its
/// standard-14 slices are drawn as typed, whatever level its paragraph puts
/// them at. Reordering every slice whose run was at an odd level — tried on
/// the review of 6d08c6b, for the punctuation run below — read every one of
/// these back reversed: `?!` as `!?`, `(...)` as `)...(` (drawn so too, each
/// bracket's hollow turned away from the dots), `...!` as `!...`, `[*]` as
/// `]*[`, `?! 12` as ` !?12` and a heading's `?!` as `!?`, where a66f692,
/// before that was tried, conserved each.
#[test]
fn a_standard_14_neutral_line_at_a_right_to_left_level_reads_as_written() {
    for (body, expected) in [
        ("<p dir=\"rtl\">?!</p>", "?!"),
        ("<h2 dir=\"rtl\">?!</h2>", "?!"),
        ("<p dir=\"rtl\">(...)</p>", "(...)"),
        ("<p dir=\"rtl\">...!</p>", "...!"),
        ("<p dir=\"rtl\">[*]</p>", "[*]"),
        ("<p dir=\"rtl\">?! 12</p>", "?! 12"),
    ] {
        let bytes = styled_book("he", "", body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        reads_as_written(&bytes, &doc, expected);
    }
}

/// **The same line, first in a paragraph whose second line is Hebrew, reads
/// as written too.**
///
/// The paragraph holds a right-to-left character and its first line does
/// not. Ruling 14 resolves a line alone, so that line is read in content
/// order like any other with nothing right-to-left in it, and has to be
/// drawn as typed: reordered by its level, it was drawn `!?`, and the page
/// read `!?` above `חו`.
#[test]
fn a_standard_14_neutral_line_above_a_right_to_left_line_reads_as_written() {
    let body = format!("<p dir=\"rtl\">?!<br/>{HET_VAV}</p>");
    let bytes = styled_book("he", "", &body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    reads_as_written(&bytes, &doc, &format!("?!\n{HET_VAV}"));
}

/// **A run of nothing but punctuation between two right-to-left words is
/// drawn in its level's order, and reads as written** — the known limit
/// `epub.md`'s `direction` row named until ROADMAP K-11 closed.
///
/// The `.,` between two italic Hebrew words is a run of its own, and N1
/// resolves both marks to the paragraph's level, 1, so L2 draws them `,.`.
/// `coded_order` reordered only a slice that held a right-to-left character,
/// so this one was drawn as typed, `.` left of `,`, and the line, which
/// ruling 14 reads right to left, came back `חו,.וח`. It is drawn `,` left
/// of `.` now, because its **line** holds a right-to-left character
/// (`TextRun::bidi_line_rtl`), and a line with none still draws such a run as
/// typed (the two tests above).
///
/// The same limit drew CI's `sample-regime-anticancer-arabic.epub` wrong:
/// in a sans-serif book the brackets, the full stop and the spaces are
/// Helvetica's and the Arabic the overflow font's, so `). ` and ` (` are
/// face segments of their own inside an Arabic run, and were drawn as
/// typed — the page showed `عدة( النازعة`, and read `للأسيتيل .)وقد`, the
/// job's `Missing ").وقد…"`, `Extra ")وقد…"`.
#[test]
fn a_standard_14_punctuation_run_between_right_to_left_words_reads_as_written() {
    let body = format!("<p dir=\"rtl\"><i>{HET_VAV}</i>.,<i>{VAV_HET}</i></p>");
    let bytes = styled_book("he", "", &body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let (comma, stop) = (drawn_at(&doc, ","), drawn_at(&doc, "."));
    assert!(
        comma < stop,
        "the punctuation is drawn as typed: , at {comma}, . at {stop}"
    );
    reads_as_written(&bytes, &doc, &format!("{HET_VAV}.,{VAV_HET}"));

    let arabic = "عوامل عدّة (النازعة للأسيتيل). وقد أصبحنا نعرف الآن";
    let bytes = styled_book(
        "ar",
        "body { font-family: sans-serif }",
        &format!("<p dir=\"rtl\">{arabic}</p>"),
    );
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let (open, close) = (drawn_at(&doc, "("), drawn_at(&doc, ")"));
    assert!(
        close < open,
        "the pair is drawn as typed, each bracket facing its word: ( at {open}, ) at {close}"
    );
    reads_as_written(&bytes, &doc, arabic);
}

/// **A run of neutrals in an embedding, on a line with no right-to-left
/// letter, is drawn as typed and reads as typed** (review of the K-11 fix).
///
/// `x RLE ?! PDF y`: the `?!` is at level 1, and a line that holds an `RLE`
/// is one ruling 14 reads — but rule X9 removes the `RLE` before anything is
/// resolved, so its reading never sees the embedding, and a run drawn in the
/// embedding's order reads back reversed. As first written, K-11's fix drew
/// it so, because its line held an `RLE`, and the page extracted
/// `x RLE PDF !? y`; a line decides it by a letter of class `R` or `AL` now,
/// or an `RLI`, which stays on the line and is read back with what it
/// isolates — so the isolate's `?!`, which read `!?` before K-11, reads as
/// typed too.
#[test]
fn a_run_of_neutrals_in_an_embedding_with_no_right_to_left_letter_reads_as_typed() {
    for (control, closer) in [
        ("&#x202B;", "&#x202C;"),
        ("&#x202E;", "&#x202C;"),
        ("&#x2067;", "&#x2069;"),
    ] {
        let bytes = styled_book("en", "", &format!("<p>x {control}?!{closer} y</p>"));
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let text = doc.page(0).expect("a page").text().plain_text();
        assert!(text.contains("?!"), "{control}: {text:?}");
        for verdict in [
            conservation(&bytes, &doc),
            conservation_in_logical_order(&bytes, &doc),
        ] {
            assert!(verdict.holds(), "{control}: {text:?} {verdict:?}");
        }
    }
}

/// **A run of neutrals of its own that opens a right-to-left line with
/// nothing left to right on it is drawn as typed, and reads as typed** — the
/// comma tie-break's price, not widened (review of the K-11 fix).
///
/// `<p dir="rtl"><i>?!</i> שלום</p>`: UAX #9 draws the `?!` at the line's
/// right end, `?` rightmost. The line is ruling 14's comma tie-break's shape
/// — a right-to-left letter leftmost, closing punctuation rightmost, nothing
/// left to right — and a left-to-right paragraph draws it too, so it is read
/// as one, the punctuation trailing in the order drawn. Drawn in its level's
/// order, as K-11's fix first drew every such run on a line holding a
/// right-to-left letter, the run read `!?`: 51 probe books that conserved in
/// logical order at 9297d2d did not. The writer asks the reader's own
/// question of the line it draws (`drawn_direction`), and resolves the line
/// alone in the direction the answer gives: read left to right, the `?!` is
/// at level 0 there, not 1, so it is drawn as typed, as before K-11, and the
/// run reads `?!` within its own structure element. The same line in one run
/// is a slice of a right-to-left run, drawn in L2's order, and pays the price
/// the owner accepted, `שלום !?` (`content-and-text.md`).
#[test]
fn a_run_of_neutrals_opening_a_right_to_left_line_is_drawn_as_typed() {
    let body = format!("<p dir=\"rtl\"><i>?!</i> {SHALOM}</p>");
    let bytes = styled_book("he", "", &body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let (question, bang) = (drawn_at(&doc, "?"), drawn_at(&doc, "!"));
    assert!(
        question < bang,
        "{body}: the run is drawn in its level's order: ? at {question}, ! at {bang}"
    );
    let text = doc.page(0).expect("a page").text().plain_text();
    assert!(text.contains(&format!("{SHALOM} ?!")), "{body}: {text:?}");
    let logical = conservation_in_logical_order(&bytes, &doc);
    assert!(logical.holds(), "{body}: {text:?} {logical:?}");

    // In one run: the price.
    let body = format!("<p dir=\"rtl\">?! {SHALOM}</p>");
    let bytes = styled_book("he", "", &body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let (question, bang) = (drawn_at(&doc, "?"), drawn_at(&doc, "!"));
    assert!(
        bang < question,
        "{body}: the run is drawn in its level's order, ? rightmost: ? at {question}, ! at {bang}"
    );
    let text = doc.page(0).expect("a page").text().plain_text();
    assert!(text.contains(&format!("{SHALOM} !?")), "{body}: {text:?}");
}

/// **A run of neutrals whose level order a reader of the page cannot undo is
/// drawn as typed, and reads as written** (review of the K-11 fix).
///
/// K-11's fix draws a run of nothing but neutrals at a right-to-left level
/// in its level's order, for ruling 14 to undo. Two such runs it cannot
/// undo, each in a book that conserved at 9297d2d, where every such run was
/// drawn as typed, and did not with the run in its level's order:
///
/// - **A run its line alone resolves otherwise.** `عوامل عدّة ($1,234.56).
///   وقد أصبحنا نعرف.` at `margin: 0 38%` breaks before the bracket. W2 makes
///   the digits `AN` for the Arabic word on the line above, so the `$` is a
///   neutral at the paragraph's level, drawn right of its number; ruling 14
///   reads the line alone, with no Arabic letter before the digits, and the
///   `$` as a European number's: `(1,234.56$)`. The line alone is resolved in
///   the direction a reader reads it in, not its paragraph's: with no `dir`,
///   a justified sans-serif line `… عدّة (2010). وقد …` has Arabic at both
///   ends and is read from the right, and resolved left to right, as its
///   paragraph is, its `). ` was a run of other levels, drawn as typed, and
///   lost its full stop from its bracket — K-11's own failure, `Missing
///   ").وقد…"`.
/// - **A run of a line letter-spaced by half an em.** A reader resumes a line
///   across a text object's end only within half an em, so such a line is
///   read run by run, and a run of neutrals on its own in the order drawn:
///   `שָׁלוֹם <i>?!</i> עוֹלָם` in monospace at `8px` read its `?!` on a line
///   with the word after it, by the comma tie-break, `עוֹלָם !?`.
#[test]
fn a_run_of_neutrals_a_reader_cannot_undo_is_drawn_as_typed() {
    let money = "<p dir=\"rtl\">عوامل عدّة ($1,234.56). وقد أصبحنا نعرف.</p>".to_owned();
    let bytes = styled_book("ar", "p { margin: 0 38% }", &money);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let text = doc.page(0).expect("a page").text().plain_text();
    for verdict in [
        conservation(&bytes, &doc),
        conservation_in_logical_order(&bytes, &doc),
    ] {
        assert!(verdict.holds(), "{money}: {text:?} {verdict:?}");
    }

    let year = "<p>إنّ الخلايا السرطانيّة تتكاثر بسرعة كبيرة، ويعتمد ذلك على عوامل عدّة \
                (2010). وقد أصبحنا نعرف الآن أنّ بعض الموادّ تكبح الهستونات النازعة \
                للأسيتيل، مخفّفة بذلك من نموّ الأورام.</p>";
    let style = "body { font-family: sans-serif } p { text-align: justify; margin: 0 15% }";
    let bytes = styled_book("ar", style, year);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let text = doc.page(0).expect("a page").text().plain_text();
    for verdict in [
        conservation(&bytes, &doc),
        conservation_in_logical_order(&bytes, &doc),
    ] {
        assert!(verdict.holds(), "{year}: {text:?} {verdict:?}");
    }

    let spaced = format!("<p dir=\"rtl\">{SHALOM} <i>?!</i> \u{5E2}\u{5D5}\u{5DC}\u{5DD}</p>");
    let style = "p { font-family: monospace; letter-spacing: 8px }";
    let bytes = styled_book("he", style, &spaced);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let text = doc.page(0).expect("a page").text().plain_text();
    assert!(text.contains("?!"), "{spaced}: {text:?}");
    let logical = conservation_in_logical_order(&bytes, &doc);
    assert!(logical.holds(), "{spaced}: {text:?} {logical:?}");
}

/// `text` with its Arabic and Hebrew marks taken out.
fn unpointed(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0591}'..='\u{05C7}'))
        .collect()
}

/// **A run of neutrals its word spaces part from the words either side of it
/// is drawn as typed, and reads as written** (third review of the K-11 fix).
///
/// `<i>?!</i>` between two Arabic words is a run of its own at the
/// paragraph's right-to-left level, and K-11's fix draws it in that level's
/// order, `!?`, for ruling 14 to undo with its neighbours. Justified at
/// `letter-spacing: 0.3em`, or at `word-spacing: 1em`, the word space on each
/// side of it is drawn more than half an em from the glyphs beside it, so a
/// reader cuts the line there and reads the run on its own, in the order it
/// was drawn: `!?`. Drawn as typed, as at 9297d2d, it reads `?!`
/// (`paint::neutrals_drawn_as_typed`, which counts the gap a reader sees —
/// the run's `letter-spacing`, and its `word-spacing` after a word space —
/// on each side of the run). Fifteen probe books in a default build, and
/// eighteen with `bundled-fonts`, conserved at 9297d2d and did not with the
/// run in its level's order; `letter-spacing` of half an em was the only gap
/// the writer counted.
#[test]
fn a_run_of_neutrals_its_word_spaces_part_is_drawn_as_typed() {
    let text = "عوامل <i>?!</i> عدّة <b>;</b> وقد <i>…</i> أصبحنا";
    for style in [
        "p { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }",
        "p { word-spacing: 1em; margin: 0 18% }",
    ] {
        for body in [
            format!("<p dir=\"rtl\">{text}</p>"),
            format!("<p>{text}</p>"),
            format!("<div style=\"direction: rtl\"><p>{text}</p></div>"),
        ] {
            for body in [body.clone(), unpointed(&body)] {
                let bytes = styled_book("ar", style, &body);
                let doc = Document::open(bytes.clone()).expect("the book opens");
                let text = doc.page(0).expect("a page").text().plain_text();
                assert!(text.contains("?!"), "{style}, {body}: {text:?}");
                for verdict in [
                    conservation(&bytes, &doc),
                    conservation_in_logical_order(&bytes, &doc),
                ] {
                    assert!(verdict.holds(), "{style}, {body}: {text:?} {verdict:?}");
                }
            }
        }
    }
}

/// **A word-spaced slice whose glyph reads alike either way round is drawn
/// with its word space first** (third review of the K-11 fix).
///
/// In a monospace Arabic paragraph, justified, Courier draws ` #` and the
/// overflow font the Arabic words. The overflow font's code 32 is a letter of
/// the book's, and while the font was written at the run's `Tw` (until
/// October 2026, ROADMAP CD-19) the last glyphs of its string were drawn up
/// to a word spacing right of where layout put them — over what is drawn
/// next. Drawn in its level's order, `# `, the `#` stood there: the NOON of
/// `نعرف` and the `#` overlapped and the line read `أصبحنا ن#عرف`. Drawn as
/// typed, as at 9297d2d, the space stood there, which draws nothing
/// (`paint::opens_with_its_word_space`); the slice's one glyph reads the same
/// either way. Four books in each build conserved at 9297d2d and did not with
/// the slice in its level's order. The font is written at `0 Tw` now and
/// nothing overruns; the book is kept for the slice's other reason, its
/// stretched space inside its own text object.
#[test]
fn a_word_spaced_slice_reading_alike_is_drawn_with_its_space_first() {
    let words = [
        "عوامل",
        "عدّة",
        "وقد",
        "أصبحنا",
        "نعرف",
        "الآن",
        "الموادّ",
        "تكبح",
        "الخلايا",
        "السرطانيّة",
        "تتكاثر",
        "بسرعة",
    ];
    let text: Vec<String> = (0..6)
        .map(|k| {
            let at = |i: usize| words[(3 * k + i) % words.len()];
            format!("{} #{} {} {}.", at(0), at(1), at(2), at(3))
        })
        .collect();
    let text = text.join(" ");
    let style = "body { font-family: monospace } p { text-align: justify; margin: 0 20% }";
    for body in [
        format!("<p dir=\"rtl\">{text}</p>"),
        format!("<p>{text}</p>"),
    ] {
        for body in [body.clone(), unpointed(&body)] {
            let bytes = styled_book("ar", style, &body);
            let doc = Document::open(bytes.clone()).expect("the book opens");
            let text = doc.page(0).expect("a page").text().plain_text();
            for verdict in [
                conservation(&bytes, &doc),
                conservation_in_logical_order(&bytes, &doc),
            ] {
                assert!(verdict.holds(), "{body}: {text:?} {verdict:?}");
            }
        }
    }
}

/// **A word-spaced slice of more than one glyph that does not read alike
/// either way round is drawn in its level's order, its word space and all**
/// (review of the third review's K-11 fix).
///
/// `paint::opens_with_its_word_space` draws a word-spaced slice of neutrals
/// as typed, its word space first, only where its glyphs read the same
/// reversed. The italic ` ?!` glued to `عوامل` here, under `word-spacing:
/// 0.3em` and `letter-spacing: 0.1em`, is a slice whose glyphs do not: drawn
/// as typed it read `عوامل!?  عدّة` and neither content nor logical order
/// held; in its level's order it reads as typed. The one-glyph ` #` of the
/// test above reads alike trivially, so without this book nothing failed
/// when the condition was dropped. 9297d2d failed this book too: it pins
/// the third review's gain, not a book 9297d2d conserved.
#[test]
fn a_word_spaced_slice_not_reading_alike_is_drawn_in_its_levels_order() {
    let body = "<p dir=\"rtl\">عوامل<i> ?!</i> عدّة وقد أصبحنا</p>";
    let style = "p { word-spacing: 0.3em; letter-spacing: 0.1em }";
    let bytes = styled_book("ar", style, body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let text = doc.page(0).expect("a page").text().plain_text();
    for verdict in [
        conservation(&bytes, &doc),
        conservation_in_logical_order(&bytes, &doc),
    ] {
        assert!(verdict.holds(), "{body}: {text:?} {verdict:?}");
    }
}

/// **A number or a word drawn left to right inside word-spaced right-to-left
/// text is not drawn over the bracket before it** (third review of the K-11
/// fix).
///
/// The overflow font's code 32 is a character of the book's, the first past
/// `WinAnsiEncoding` its face met, and 9.3.3 moves every glyph after it in
/// its string by `Tw`: written at the run's word spacing, as it was until
/// October 2026 (ROADMAP CD-19), in a slice with no mark it drew them a word spacing
/// into whatever is drawn on its right. In a slice drawn left to right
/// inside right-to-left text, what is drawn on its right is what a reader
/// reads before it, and a glyph moved past it is read before it too: with
/// `bundled-fonts`, whose overflow font draws a Hebrew book's Arabic-Indic
/// digits and the ONE at code 32, `(١٢٣),` opening a justified line
/// letter-spaced by `0.3em` drew its THREE on its `(` and read `٣(١٢),`.
/// Such a book conserved at 9297d2d only while its lines were read word by
/// word; K-11's carry joins them, and the number was found by no search (3
/// books). A number or a Greek word the book meets first, so that it opens
/// with its face's code 32, did the same under `word-spacing` from the
/// overflow font on. The third review of the K-11 fix drew the glyph after
/// the code 32 in such a slice as a piece of its own, where layout put it;
/// the overflow font is written at `0 Tw` now, so nothing it draws moves, in
/// any slice. (In a default build the justified book breaks a line after its
/// second `(` and does not conserve, at 9297d2d as now: a bracket a line
/// break parted, `content-and-text.md`.)
#[test]
fn a_number_in_word_spaced_right_to_left_text_is_not_drawn_over_its_bracket() {
    let justified = "שלום עולם מלחמה (١٢٣), ספר בית גדול אור. \
                     עולם מלחמה ספר (١٢٣), בית גדול אור יום. \
                     מלחמה ספר בית (١٢٣), גדול אור יום שלום.";
    let mut books = vec![
        (
            "p { word-spacing: 0.5em }",
            "(١٢٣) שלום עולם (١٢٣) מלחמה.",
            "(١٢٣)",
            2,
        ),
        (
            "p { word-spacing: 0.5em }",
            "(\u{3B1}\u{3BB}\u{3C6}\u{3B1}) שלום עולם (\u{3B1}\u{3BB}\u{3C6}\u{3B1}) מלחמה.",
            "(\u{3B1}\u{3BB}\u{3C6}\u{3B1})",
            2,
        ),
    ];
    if cfg!(feature = "bundled-fonts") {
        books.push((
            "p { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }",
            justified,
            "(١٢٣)",
            3,
        ));
    }
    for (style, text, expected, times) in books {
        let body = format!("<p dir=\"rtl\">{text}</p>");
        let bytes = styled_book("he", style, &body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        reads_as_written(&bytes, &doc, expected);
        let page = doc.page(0).expect("a page");
        assert_eq!(
            page.text().search(expected).len(),
            times,
            "under `{style}`, {body:?}: {expected:?} is not found {times} times in {:?}",
            page.text().plain_text()
        );
        let over = overprinted(&doc);
        assert!(
            over.is_empty(),
            "under `{style}`, {body:?} draws a glyph over another: {over:?}"
        );
    }
}

/// **A word-spaced right-to-left table cell after a cell ending in a Latin
/// word reads whole, and both its words are found** (ROADMAP CD-19).
///
/// `<table dir="rtl" style="border-spacing: 0; word-spacing: 1em">` whose
/// last row is `abc | وقد | def | أصبحنا نعرف`: the last cell is drawn just
/// left of `def`, with no white space between. The overflow font draws its
/// Arabic, and the cell's AIN is its code 32. Written at the run's `Tw`, the
/// font drew the NOON after it a word spacing left of the word space's far
/// end, closing the cell's stretched word gap to 3pt, and ruling 14 rejoined
/// the two words across it, the NOON in the wrong one: the cell read
/// `أصبحنان عرف`, and so did the second row's, which follows no Latin word,
/// so `نعرف` was found nowhere in the book (at 9297d2d and 56dfd93 alike, in
/// either build, and in every variant below; `أصبحنا` was found twice).
/// Written at `0 Tw`, the NOON is where layout put it and the gap is the stretched word
/// space's whole em, which `TextDevice` carries a reader's pen across only
/// since the cell's word no longer goes on from `def` (the commit before
/// this one, `a_gap_no_spacing_accounts_for_ends_the_word_before_it`): before
/// it, the cell came back in two pieces read in drawn order, as its Hebrew
/// twin did, and the 24 books a build of the K-11 review's probe that
/// conserved only through the moved NOON would have failed.
#[test]
fn a_word_spaced_right_to_left_cell_after_a_latin_word_reads_whole() {
    let rows: String = [
        "<td>عوامل</td><td>عدّة</td><td>وقد.</td><td>(HDAC)</td>",
        "<td>أصبحنا نعرف</td><td>الآن</td><td>(2010)</td><td>x</td>",
        "<td>«الموادّ»</td><td>تكبح</td><td>[عوامل]</td><td>عدّة</td>",
        "<td>abc</td><td>وقد</td><td>def</td><td>أصبحنا نعرف</td>",
    ]
    .iter()
    .map(|row| format!("<tr>{row}</tr>"))
    .collect();
    for style in ["", "body { font-family: sans-serif }"] {
        for rows in [
            rows.clone(),
            rows.replace("<td>", "<td style=\"padding: 0\">"),
        ] {
            let table = format!(
                "<table dir=\"rtl\" style=\"border-spacing: 0; word-spacing: 1em\">{rows}</table>"
            );
            for body in [table.clone(), unpointed(&table)] {
                let bytes = styled_book("ar", style, &body);
                let doc = Document::open(bytes.clone()).expect("the book opens");
                let page = doc.page(0).expect("a page");
                let text = page.text().plain_text();
                for verdict in [
                    conservation(&bytes, &doc),
                    conservation_in_logical_order(&bytes, &doc),
                ] {
                    assert!(verdict.holds(), "`{style}`, {body}: {text:?} {verdict:?}");
                }
                for word in ["أصبحنا", "نعرف"] {
                    assert_eq!(
                        page.text().search(word).len(),
                        2,
                        "`{style}`, {body}: {word:?} is not found twice in {text:?}"
                    );
                }
            }
        }
    }
}

/// **A styled neutral glued to a word under `word-spacing` keeps the word
/// searchable** (ROADMAP CD-19; H4 of the review of the third review's K-11
/// fix).
///
/// `<p dir="rtl">ا <b>.</b>أصبحنا ن</p>` at `word-spacing: 0.6em`: the
/// overflow font's code 32 is the paragraph's first ALEF, and written at the
/// run's `Tw` the font drew the rest of `أصبحنا`'s string — NOON to
/// ALEF-HAMZA, drawn left to right — a word spacing right of where layout put
/// it, over the bold `.` glued to the word. K-11's carry joins the word to its
/// line, which holds the `.`, and the line read `اأص.بحنا ن`: the word was
/// not found (8 books in each build of the hunt's probes, found at 9297d2d,
/// whose content streams the K-11 work did not change; 16 more in a default
/// build of the fix-verify probe and 3 with `bundled-fonts`, Hebrew too).
/// Written at `0 Tw`, every glyph is where layout put it and the line reads
/// as written: the Arabic dots of the hunt's minimised book, and the Hebrew
/// `?!` and `»,` glued to the word on both sides, `יום<b>?!</b>בראשית`.
#[test]
fn a_styled_neutral_glued_to_a_word_under_word_spacing_keeps_it_searchable() {
    let hebrew = "\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD}<b>{p}</b>\
                  \u{5DE}\u{5DC}\u{5D7}\u{5DE}\u{5D4} \u{5E1}\u{5E4}\u{5E8}. \
                  \u{5D0}\u{5D5}\u{5E8} \u{5D9}\u{5D5}\u{5DD}<b>{p}</b>\
                  \u{5D1}\u{5E8}\u{5D0}\u{5E9}\u{5D9}\u{5EA} \u{5D1}\u{5E8}\u{5D0}.";
    let books = [
        ("ar", "<p dir=\"rtl\">ا <b>.</b>أصبحنا ن</p>", vec!["أصبحنا"]),
        (
            "ar",
            "<p dir=\"rtl\">ال <i>...</i> وقد <b>...</b>أصبحنا نعرف.</p>",
            vec!["أصبحنا", "نعرف"],
        ),
        (
            "he",
            "<p dir=\"rtl\">\u{5D9}\u{5D5}\u{5DD}<b>?!</b>\u{5D1}\u{5E8}\u{5D0}\u{5E9}\u{5D9}\u{5EA}</p>",
            vec!["\u{5D9}\u{5D5}\u{5DD}", "\u{5D1}\u{5E8}\u{5D0}\u{5E9}\u{5D9}\u{5EA}"],
        ),
    ];
    let mut books: Vec<(&str, String, Vec<&str>)> = books
        .into_iter()
        .map(|(language, body, words)| (language, body.to_owned(), words))
        .collect();
    for glued in ["?!", "\u{BB},"] {
        let text = hebrew.replace("{p}", glued);
        for open in ["<p dir=\"rtl\">", "<p>"] {
            books.push((
                "he",
                format!("{open}{text}</p>"),
                vec![
                    "\u{5D1}\u{5E8}\u{5D0}\u{5E9}\u{5D9}\u{5EA}",
                    "\u{5DE}\u{5DC}\u{5D7}\u{5DE}\u{5D4}",
                ],
            ));
        }
    }
    for spacing in ["0.5em", "0.6em", "1em"] {
        let style = format!("p {{ word-spacing: {spacing}; margin: 0 20% }}");
        for (language, body, words) in &books {
            let bytes = styled_book(language, &style, body);
            let doc = Document::open(bytes.clone()).expect("the book opens");
            let page = doc.page(0).expect("a page");
            let text = page.text().plain_text();
            for word in words {
                assert!(
                    !page.text().search(word).is_empty(),
                    "`{style}`, {body}: {word:?} is not found in {text:?}"
                );
            }
            for verdict in [
                conservation(&bytes, &doc),
                conservation_in_logical_order(&bytes, &doc),
            ] {
                assert!(verdict.holds(), "`{style}`, {body}: {text:?} {verdict:?}");
            }
            let over = overprinted(&doc);
            assert!(
                over.is_empty(),
                "`{style}`, {body}: a glyph is drawn over another: {over:?}"
            );
        }
    }
}

/// **A neutral past `WinAnsiEncoding` after a word space reads with its
/// number** (V3 of the review of the third review's K-11 fix).
///
/// `<p>… بيت ٥٪ <b>كبير</b> …</p>`, Arabic with no `dir`, at `word-spacing:
/// 0.5em; margin: 0 20%`: the ARABIC PERCENT SIGN after the Arabic-Indic
/// FIVE is a neutral (`ET`, which W5 makes a European number's only beside
/// one, and W6 makes `ON` here) that N1 puts at level 1 with the word space
/// after it, a slice the
/// painter draws in its level's order for a reader to undo with its line,
/// the space at its left and the sign at its right. The sign is past
/// `WinAnsiEncoding`, so the overflow font draws it and a standard face the
/// space: two resources, and since 8e8e111 two text objects. The reader's
/// pen stopped where the space's box ends, the sign started a stretched word
/// space later, more than half an em on, and `TextDevice` carries a pen
/// across spacing only into a right-to-left letter: the line was cut at the
/// sign, and the sign read after the next word, `٥ \nنعرف٪`. 9297d2d drew
/// the slice as typed and conserved. A slice `coded_order` draws for a
/// reader to undo is one text object now, under spacing that may part its
/// objects (`paint::PARTED_FROM`), the space and the sign pieces of
/// it, so the sign reads as a `WinAnsiEncoding` one does, a glyph of the
/// space's string; it takes no word spacing, as the overflow font takes none
/// anywhere.
#[test]
fn a_non_winansi_neutral_after_a_word_space_reads_with_its_number() {
    let text = "مدرسة الطالب جميل بيت ٥٪ <b>كبير</b> نور بيت كبير نور يوم ٥٪ \
                <b>عوامل</b> وقد يوم عوامل وقد أصبحنا ٥٪ <b>نعرف</b> كتاب";
    for spacing in ["0.5em", "0.6em", "1em"] {
        let style = format!("p {{ word-spacing: {spacing}; margin: 0 20% }}");
        for body in [
            format!("<p>{text}</p>"),
            format!("<p dir=\"rtl\">{text}</p>"),
        ] {
            let bytes = styled_book("ar", &style, &body);
            let doc = Document::open(bytes.clone()).expect("the book opens");
            let page = doc.page(0).expect("a page");
            let read = page.text().plain_text();
            for verdict in [
                conservation(&bytes, &doc),
                conservation_in_logical_order(&bytes, &doc),
            ] {
                assert!(verdict.holds(), "`{style}`, {body}: {read:?} {verdict:?}");
            }
            assert_eq!(
                page.text().search("٥٪").len(),
                3,
                "`{style}`, {body}: the number and its sign are not found three times in {read:?}"
            );
        }
    }
}

/// **The inside of a spaced slice is one text object, so a reader reads its
/// words together** (ROADMAP CD-19; H2 and F5 of the review of CD-19's fix).
///
/// A slice is the stretch of one run, at one level, that one standard face
/// draws, and it needs two font resources wherever it holds a character past
/// `WinAnsiEncoding`: its Greek, Cyrillic, Hebrew or Arabic letters are the
/// overflow font's and its word spaces the face's own, a text object at each
/// change. Under `word-spacing` or `letter-spacing` each object starts a
/// spacing past where the one before it ended, and `TextDevice` carries a
/// reader's pen across spacing only into a right-to-left letter after
/// right-to-left text. Written at the run's `Tw`, the glyphs after the
/// overflow font's code-32 letter were moved over some of those gaps; written
/// at `0 Tw` (CD-19), every gap is where layout put it, and the reader cut the
/// slice at each of them:
///
/// - a Greek or Cyrillic cell of a table with no `dir`, at
///   `word-spacing: 1em`, was cut after each word, and its last word, glued
///   to the right-to-left cell beside it, was read with that cell, right to
///   left by its majority: `αβ | ספרγδ` (79 books of the review's probes in a
///   default build that conserved at 9297d2d and 56dfd93, every one of which
///   conserves again; with `bundled-fonts` the stand-in draws the letters and
///   none was lost);
/// - an Arabic paragraph letter-spaced by `0.55em` and justified was cut
///   before each word space, a text object of its own `0.55em` past its
///   word, and its words were read in the order drawn, `نعرف | أصبحنا`.
///
/// The inside of such a slice, from its first glyph that is not white space
/// to its last, is one text object now, the stretches of each resource
/// pieces of it, and is read as one line, as a `WinAnsiEncoding` slice, one
/// string, always was.
#[test]
fn the_inside_of_a_spaced_slice_is_one_text_object() {
    let greek = "<table><tr><td>\u{3B1}\u{3B2} \u{3B3}\u{3B4}</td>\
                 <td>\u{5E1}\u{5E4}\u{5E8}</td></tr></table>";
    let cyrillic = "<table><tr><td>\u{43F}\u{440}\u{438}\u{432}\u{435}\u{442} \
                    \u{43C}\u{438}\u{440}</td><td>\u{643}\u{62A}\u{627}\u{628}</td></tr></table>";
    let arabic = "<p dir=\"rtl\">\u{639}\u{648}\u{627}\u{645}\u{644} \u{639}\u{62F}\u{651}\u{629} \
                  \u{648}\u{642}\u{62F} <i>?! -- ?!</i> \u{623}\u{635}\u{628}\u{62D}\u{646}\u{627} \
                  \u{646}\u{639}\u{631}\u{641} \u{627}\u{644}\u{622}\u{646} \
                  \u{627}\u{644}\u{645}\u{648}\u{627}\u{62F}\u{651}.</p>";
    let books = [
        (
            "he",
            "td { word-spacing: 1em }",
            greek,
            vec!["\u{3B1}\u{3B2}", "\u{3B3}\u{3B4}", "\u{5E1}\u{5E4}\u{5E8}"],
        ),
        (
            "he",
            "td { word-spacing: 0.6em }",
            greek,
            vec!["\u{3B1}\u{3B2}", "\u{3B3}\u{3B4}", "\u{5E1}\u{5E4}\u{5E8}"],
        ),
        (
            "ar",
            "p, td { word-spacing: 1em }",
            cyrillic,
            vec!["\u{43C}\u{438}\u{440}", "\u{643}\u{62A}\u{627}\u{628}"],
        ),
        (
            "ar",
            "p { letter-spacing: 0.55em; text-align: justify; margin: 0 20% }",
            arabic,
            vec![
                "\u{623}\u{635}\u{628}\u{62D}\u{646}\u{627}",
                "\u{646}\u{639}\u{631}\u{641}",
            ],
        ),
    ];
    for (language, style, body, words) in books {
        let bytes = styled_book(language, style, body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let page = doc.page(0).expect("a page");
        let text = page.text().plain_text();
        for verdict in [
            conservation(&bytes, &doc),
            conservation_in_logical_order(&bytes, &doc),
        ] {
            assert!(verdict.holds(), "`{style}`, {body}: {text:?} {verdict:?}");
        }
        for word in words {
            assert!(
                !page.text().search(word).is_empty(),
                "`{style}`, {body}: {word:?} is not found in {text:?}"
            );
        }
        let over = overprinted(&doc);
        assert!(
            over.is_empty(),
            "`{style}`, {body}: a glyph is drawn over another: {over:?}"
        );
    }
}

/// **A spaced slice is one text object only where its line reads it whole,
/// and a slice no spacing parts is written as before** (ROADMAP CD-19).
///
/// Three kinds of slice keep a text object at each change of resource, each
/// because written whole it read worse than in its pieces, which a reader
/// reads in the order drawn, the order written:
///
/// - **the white space at a slice's ends**: written with the slice, the word
///   space after `عوامل` drew the Arabic word's line up to the URL beside it,
///   and ruling 14, reading the two as one line whose Latin letters are the
///   more, read `https://example.org/ عوامل`;
/// - **a left-to-right slice inside right-to-left text**, at level 2:
///   `Łódź Kraków ő`, whose `Ł`, `ź` and `ő` the overflow font draws, joined
///   the Hebrew either side into such a line, read left to right;
/// - **a slice holding an embedding or override formatting character**:
///   with its `LEFT-TO-RIGHT OVERRIDE` on one line with the Hebrew word
///   drawn before it, `שלום \u{202D}ab\u{202C} עולם` read the word backwards.
///
/// And a run whose spacing opens no gap a reader parts two objects at —
/// under half an em, and with none at all — is written as before: one text
/// object for each change of resource in a slice of neutrals as in any
/// other.
#[test]
fn a_spaced_slice_is_one_text_object_only_where_its_line_reads_it_whole() {
    const HE: [&str; 12] = [
        "\u{5E9}\u{5DC}\u{5D5}\u{5DD}",
        "\u{5E2}\u{5D5}\u{5DC}\u{5DD}",
        "\u{5DE}\u{5DC}\u{5D7}\u{5DE}\u{5D4}",
        "\u{5E1}\u{5E4}\u{5E8}",
        "\u{5D1}\u{5D9}\u{5EA}",
        "\u{5D2}\u{5D3}\u{5D5}\u{5DC}",
        "\u{5D0}\u{5D5}\u{5E8}",
        "\u{5D9}\u{5D5}\u{5DD}",
        "\u{5D1}\u{5E8}\u{5D0}\u{5E9}\u{5D9}\u{5EA}",
        "\u{5D1}\u{5E8}\u{5D0}",
        "\u{5D0}\u{5DC}\u{5D4}\u{5D9}\u{5DD}",
        "\u{5D4}\u{5D0}\u{5E8}\u{5E5}",
    ];
    let s = |i: usize| HE[i % HE.len()];
    let five = |shape: &dyn Fn(usize) -> String| -> String {
        (0..5).map(|k| shape(k * 3)).collect::<Vec<_>>().join(" ")
    };
    let latin = five(&|i| {
        format!(
            "{} {} \u{141}\u{F3}d\u{17A} Krak\u{F3}w \u{151} {}.",
            s(i),
            s(i + 1),
            s(i + 2)
        )
    });
    let url = "\u{639}\u{648}\u{627}\u{645}\u{644} https://example.org/a_(b) \
               \u{639}\u{62F}\u{651}\u{629} \u{648}\u{642}\u{62F}.";
    let mut conserving = vec![(
        "he",
        "p { word-spacing: 0.5em; margin: 0 20% }".to_owned(),
        format!("<p dir=\"rtl\">{latin}</p>"),
    )];
    for style in [
        "p { text-align: justify; margin: 0 30% }",
        "p { word-spacing: 0.6em; margin: 0 20% }",
        "p { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }",
    ] {
        conserving.push(("ar", style.to_owned(), format!("<p dir=\"rtl\">{url}</p>")));
    }
    for (language, style, body) in &conserving {
        let bytes = styled_book(language, style, body);
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let text = doc.page(0).expect("a page").text().plain_text();
        for verdict in [
            conservation(&bytes, &doc),
            conservation_in_logical_order(&bytes, &doc),
        ] {
            assert!(verdict.holds(), "`{style}`, {body}: {text:?} {verdict:?}");
        }
    }

    let overridden = five(&|i| {
        format!(
            "{} \u{202D}ab\u{202C} {} \u{202E}cd\u{202C} {}.",
            s(i),
            s(i + 1),
            s(i + 2)
        )
    });
    let style = "p { word-spacing: 1em; margin: 0 20% }";
    let body = format!("<p dir=\"rtl\">{overridden}</p>");
    let bytes = styled_book("he", style, &body);
    let doc = Document::open(bytes).expect("the book opens");
    let page = doc.page(0).expect("a page");
    let text = page.text().plain_text();
    for word in HE.iter().copied().chain(["ab", "cd"]) {
        assert!(
            !page.text().search(word).is_empty(),
            "`{style}`, {body}: {word:?} is not found in {text:?}"
        );
    }

    // No spacing, and spacing under half an em: every text object holds one
    // resource, as it did before this.
    let neutral = "\u{645}\u{62F}\u{631}\u{633}\u{629} \u{627}\u{644}\u{637}\u{627}\u{644}\u{628} \
                   \u{628}\u{64A}\u{62A} \u{665}\u{66A} <b>\u{643}\u{628}\u{64A}\u{631}</b> \
                   \u{646}\u{648}\u{631}";
    for style in ["", "p { word-spacing: 4px }"] {
        let bytes = styled_book("ar", style, &format!("<p>{neutral}</p>"));
        let doc = Document::open(bytes).expect("the book opens");
        let cos = doc.cos();
        let pages = tinker_pdf_cos::pages::collect(cos);
        let page = pages.first().expect("one page");
        let content =
            String::from_utf8_lossy(&tinker_pdf_cos::pages::content_bytes(cos, page)).into_owned();
        for object in content.split("BT").skip(1) {
            assert!(
                object.matches(" Tf").count() <= 1,
                "under `{style}` a text object holds two resources: BT{object}"
            );
        }
    }
}

/// **A right-to-left cell set touching a left-to-right one is left in
/// pieces** (R1 of the second review of CD-19's fix, ROADMAP CD-19).
///
/// With `border-spacing: 0` and no padding a table's cells touch on one
/// baseline, and a reader of the page reads the row as one line. The inside
/// of a spaced slice, written as one text object since the first review of
/// CD-19's fix, drew the Arabic or Hebrew cell's line up to the `abc (x).`
/// cell beside it, and ruling 14 read the two as one line whose right-to-left
/// letters are the more — `أصبحنا نعرف.abc (x)` — where in pieces, as before,
/// the Latin cell read whole (2 books in a default build and 4 with
/// `bundled-fonts`, read whole at 9297d2d and 56dfd93 and not after). A
/// right-to-left slice of a left-to-right paragraph on a line that holds
/// another paragraph's text is left in pieces, as a left-to-right slice
/// inside right-to-left text is; with `bundled-fonts` the Hebrew cell is set
/// a point and a half from the Latin one, which the reader joins as it does
/// a touching one, and so does the writer's line.
#[test]
fn a_right_to_left_cell_touching_a_left_to_right_one_is_left_in_pieces() {
    for style in [
        "table { border-spacing: 0 } td { padding: 0; word-spacing: 1em }",
        "table { border-spacing: 0 } td { padding: 0; letter-spacing: 0.1em; word-spacing: 0.5em }",
    ] {
        for (language, cell) in [
            (
                "ar",
                "\u{623}\u{635}\u{628}\u{62D}\u{646}\u{627} \u{646}\u{639}\u{631}\u{641}",
            ),
            ("he", "\u{5E1}\u{5E4}\u{5E8} \u{5D1}\u{5D9}\u{5EA}"),
        ] {
            let body = format!(
                "<table><tr><td>abc (x).</td><td>{cell}</td></tr>\
                 <tr><td>{cell}</td><td>abc (x).</td></tr></table>"
            );
            let bytes = styled_book(language, style, &body);
            let doc = Document::open(bytes).expect("the book opens");
            let text = doc.page(0).expect("a page").text().plain_text();
            let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
            for whole in ["abc (x).", cell] {
                assert!(
                    words.contains(whole),
                    "`{style}`, {body}: {whole:?} is not read whole in {text:?}"
                );
            }
        }
    }
}

/// Conservation in content and logical order, and no glyph drawn over
/// another, for one book.
fn conserves_drawn_apart(language: &str, style: &str, body: &str) {
    let bytes = styled_book(language, style, body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let text = doc.page(0).expect("a page").text().plain_text();
    for verdict in [
        conservation(&bytes, &doc),
        conservation_in_logical_order(&bytes, &doc),
    ] {
        assert!(verdict.holds(), "`{style}`, {body}: {text:?} {verdict:?}");
    }
    let over = overprinted(&doc);
    assert!(
        over.is_empty(),
        "`{style}`, {body}: a glyph is drawn over another: {over:?}"
    );
}

/// **Neutrals a reader reads with the word beside them are drawn for that
/// reading** (the second review of CD-19's fix: H2-1, F1, F2, and the price
/// probe's books that lost their structure tree's order through the
/// overflow font at `0 Tw`; ROADMAP CD-19).
///
/// The rules draw a run of neutrals as typed where they model a reader that
/// reads it alone, or reads it in a stretch it reads left to right: a styled
/// `?!` glued to an Arabic word, a word space at `0.6em` after it, was drawn
/// `?!` for a reader that would read the word and the `?!` by themselves —
/// and the reader, the slice after the `?!` one text object, read the whole
/// line from the right, `!?ابا اما.`; an italic `?!` letter-spaced by `0.45em`
/// and justified, half an em from the words either side, was drawn for a
/// reader reading it alone and was read with the bold word before it, which
/// it resumes within three ems. Each line is read back as the page draws it
/// now (`paint::Fonts::set_readings`), and drawn the other way where that
/// reads it right.
#[test]
fn neutrals_a_reader_reads_with_the_word_beside_them_are_drawn_for_that_reading() {
    const AR: [&str; 12] = [
        "\u{643}\u{62A}\u{627}\u{628}",
        "\u{645}\u{62F}\u{631}\u{633}\u{629}",
        "\u{627}\u{644}\u{637}\u{627}\u{644}\u{628}",
        "\u{62C}\u{645}\u{64A}\u{644}",
        "\u{628}\u{64A}\u{62A}",
        "\u{643}\u{628}\u{64A}\u{631}",
        "\u{646}\u{648}\u{631}",
        "\u{64A}\u{648}\u{645}",
        "\u{639}\u{648}\u{627}\u{645}\u{644}",
        "\u{648}\u{642}\u{62F}",
        "\u{623}\u{635}\u{628}\u{62D}\u{646}\u{627}",
        "\u{646}\u{639}\u{631}\u{641}",
    ];
    let s = |i: usize| AR[i % AR.len()];
    let styled = (0..6)
        .map(|k| {
            let i = k * 3;
            format!(
                "{} <b>{}</b> <i>?!</i> {} <b>(</b>{}<b>)</b> {}.",
                s(i),
                s(i + 1),
                s(i + 2),
                s(i + 3),
                s(i + 4)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut books: Vec<(&str, &str, String)> = vec![
        (
            "ar",
            "p { word-spacing: 0.6em }",
            "<p dir=\"rtl\"><b>?!</b>\u{627}\u{628}\u{627} \u{627}\u{645}\u{627}.</p>".to_owned(),
        ),
        (
            "ar",
            "p { word-spacing: 0.6em }",
            "<p dir=\"rtl\"><b>?!</b>\u{627}\u{628}\u{648} \u{627}\u{645}\u{627}.</p>".to_owned(),
        ),
        (
            "he",
            "p { word-spacing: 0.6em }",
            "<p dir=\"rtl\"><b>?!</b>\u{5D0}\u{5D1}\u{5D0} \u{5D0}\u{5DE}\u{5D0}.</p>".to_owned(),
        ),
        (
            "syr",
            "p, li, figcaption { word-spacing: 0.6em }",
            "<div dir=\"rtl\"><figure><figcaption><i>?!</i>\u{710}\u{720}\u{717}\u{710} \
             \u{710}\u{72A}\u{725}\u{710} \u{72B}\u{721}\u{71D}\u{710} \u{721}\u{720}\u{72C}\u{710} \
             \u{72A}\u{712}\u{710}.</figcaption></figure></div>"
                .to_owned(),
        ),
        (
            "ar",
            "p { letter-spacing: 0.45em; text-align: justify; margin: 0 32% }",
            format!("<p dir=\"rtl\">{styled}</p>"),
        ),
        // The price probe's `!?` and `?!` before styled brackets after a
        // `<br/>`, which lost their structure tree's order.
        (
            "ar",
            "p { word-spacing: 0.6em; margin: 0 20% }",
            format!(
                "<p dir=\"rtl\">{} {}<br/>!?<b>(</b>{}<b>)</b> {} {}</p>",
                s(8),
                s(9),
                s(10),
                s(11),
                s(12)
            ),
        ),
        (
            "he",
            "p { word-spacing: 1em; margin: 0 20% }",
            "<p dir=\"rtl\">\u{5D0}\u{5D5}\u{5E8} \u{5D9}\u{5D5}\u{5DD}<br/>?!<b>(</b>\
             \u{5D1}\u{5E8}\u{5D0}\u{5E9}\u{5D9}\u{5EA}<b>)</b> \u{5D1}\u{5E8}\u{5D0} \
             \u{5D0}\u{5DC}\u{5D4}\u{5D9}\u{5DD}</p>"
                .to_owned(),
        ),
        // A run that mixes the stops, `.!`, opening the line after a `<br/>`.
        (
            "ar",
            "p { word-spacing: 0.6em; margin: 0 20% }",
            format!(
                "<p dir=\"rtl\">{} {}<br/>.!<b>(</b>{}<b>)</b> {} {}</p>",
                s(0),
                s(8),
                s(3),
                s(4),
                s(11)
            ),
        ),
        // Read right only drawn two runs otherwise at once: the word before
        // the glued `?!` drawn whole and the `?!` in its level's order, with
        // no `dir` at `word-spacing: 2em` (H2-1; both conserved at 9297d2d).
        (
            "ar",
            "p { word-spacing: 2em }",
            "<p>\u{627}\u{645}\u{627} <b>?!</b>\u{627}\u{628}\u{648} \u{627}\u{645}\u{627}.</p>".to_owned(),
        ),
        (
            "he",
            "p { word-spacing: 2em }",
            "<p>\u{5D0}\u{5DE}\u{5D0} <b>?!</b>\u{5D0}\u{5D1}\u{5D0} \u{5D0}\u{5DE}\u{5D0}.</p>".to_owned(),
        ),
        // Read right only with a word's slice drawn whole, its word spaces
        // inside its own text object: `!!` italic and bold between Arabic
        // words at `word-spacing: 1em`.
        (
            "ar",
            "p { word-spacing: 1em; margin: 0 20% }",
            format!(
                "<p dir=\"rtl\">{}</p>",
                (0..3)
                    .map(|k| {
                        let i = k * 3;
                        format!(
                            "{} <i>!!</i> {} <b>!!</b>{} {}.",
                            s(i + 1),
                            s(i + 2),
                            s(i + 3),
                            s(i + 4)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
        ),
    ];
    // F2, `?!` before styled braces opening a Hebrew line letter-spaced by
    // half an em and justified. With `bundled-fonts` the rules' drawing reads
    // it right, as it did before; read back with the stand-in's widths as
    // layout measured them rather than as its `/W` rounds them, it read
    // wrong, and the drawing chosen for it read worse on the page.
    books.push((
        "he",
        "p { letter-spacing: 0.5em; text-align: justify; margin: 0 25% }",
        "<p dir=\"rtl\">\u{5D0}\u{5D5}\u{5E8} \u{5D9}\u{5D5}\u{5DD}<br/>?!<b>{</b>\
         \u{5D1}\u{5E8}\u{5D0}\u{5E9}\u{5D9}\u{5EA}<b>}</b> \u{5D1}\u{5E8}\u{5D0} \
         \u{5D0}\u{5DC}\u{5D4}\u{5D9}\u{5DD}</p>"
            .to_owned(),
    ));
    for (language, style, body) in &books {
        conserves_drawn_apart(language, style, body);
    }
}

/// **A line a reader cuts where it is spaced apart is drawn so that it
/// reads whole where cut it reads otherwise** (the second review of CD-19's
/// fix: H3, H2-4, H2-5 and the classes since 56dfd93 it named; ROADMAP
/// CD-19).
///
/// - A right-to-left word's word space, the white space at its slice's end,
///   is a text object of its own drawn left of the word, its stretch between
///   them: a reader cut `نور أصبحنا | ٪25 نعرف` at it, and read the number,
///   with no Arabic letter before it on its line, as a European one, which
///   W5 joins its sign to — `٪25`, not `25٪`. Drawn whole, its space the
///   word's, the line reads `نور أصبحنا 25٪ نعرف`.
/// - The space and the ORNATE RIGHT PARENTHESIS of `﴾٢٥﴿ <b>W</b>`, one text
///   object for a reader to undo with its line, drew `﴾٢٥﴿ مدرسة` whole, a
///   line the comma tie-break reads left to right by its closing `﴾`: in
///   pieces, as at 56dfd93, each reads as typed.
/// - A word space after a Greek or Latin word at a run's end is stretched
///   after it, and a reader cut `αλφα | βήτα (ספר) בית` there and read the
///   second piece, with more Hebrew letters than Greek, from the right. Drawn
///   at the end of its stretch, against the word after it, the space parts
///   nothing, and the line reads left to right, as it did while the overflow
///   font's moved letters covered the stretch.
/// - A word space styled apart from the words either side of it, a run of its
///   own after a bold Greek word, stood at the start of its stretch, and a
///   reader cut `αλφα βήτα | ספר בית xyz.` after it and read the second piece
///   from the right, `.xyz ספר בית`. Drawn at the middle of its stretch, the
///   gap either side of it is half the stretch, which the reader resumes the
///   line across (it conserved at 56dfd93).
#[test]
fn a_line_a_reader_cuts_where_it_is_spaced_apart_is_drawn_to_read_whole() {
    let books: [(&str, &str, &str); 12] = [
        (
            "ar",
            "p, td { word-spacing: 1em }",
            "<p dir=\"rtl\">\u{62C}\u{645}\u{64A}\u{644} \u{628}\u{64A}\u{62A} 25\u{66A} \
             <b>\u{643}\u{628}\u{64A}\u{631}</b> \u{646}\u{648}\u{631}.</p>",
        ),
        (
            "ar",
            "p, td { word-spacing: 1em }",
            "<p dir=\"rtl\">\u{646}\u{648}\u{631} \u{623}\u{635}\u{628}\u{62D}\u{646}\u{627} 25\u{66A} \
             <b>\u{646}\u{639}\u{631}\u{641}</b> \u{643}\u{62A}\u{627}\u{628}.</p>",
        ),
        (
            "ar",
            "p, li, td { text-align: justify; letter-spacing: 0.1em; word-spacing: 0.25em; margin: 0 30% }",
            "<div dir=\"rtl\"><ol><li>\u{643}\u{62A}\u{627}\u{628} \u{639}\u{648}\u{627}\u{645}\u{644} \
             25\u{20AA} <b>\u{645}\u{62F}\u{631}\u{633}\u{629}</b> \
             \u{627}\u{644}\u{637}\u{627}\u{644}\u{628}.</li></ol></div>",
        ),
        (
            "ar",
            "p { word-spacing: 0.75em }",
            "<p dir=\"rtl\">\u{FD3E}\u{662}\u{665}\u{FD3F} <b>\u{645}\u{62F}\u{631}\u{633}\u{629}</b> \
             \u{627}\u{644}\u{637}\u{627}\u{644}\u{628}.</p>",
        ),
        (
            "he",
            "p, li, td { word-spacing: 0.75em }",
            "<p dir=\"rtl\">\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5D1}\u{5E8}\u{5D0}\u{5E9}\u{5D9}\u{5EA} \
             25\u{60C} <b><i>\u{5E2}\u{5D5}\u{5DC}\u{5DD}</i></b> \u{5DE}\u{5DC}\u{5D7}\u{5DE}\u{5D4}, \
             \u{5E1}\u{5E4}\u{5E8} 25\u{60C}.</p>",
        ),
        (
            "ar",
            "p, li, td { word-spacing: 16px; letter-spacing: 1px }",
            "<div dir=\"rtl\"><table><tr><td>\u{643}\u{62A}\u{627}\u{628} 25\u{66A} \
             <b>\u{645}\u{62F}\u{631}\u{633}\u{629}</b></td><td>25\u{66A} \
             \u{627}\u{644}\u{637}\u{627}\u{644}\u{628}</td></tr></table></div>",
        ),
        (
            "he",
            "p { word-spacing: 12px }",
            "<p>\u{3B1}\u{3BB}\u{3C6}\u{3B1} <b>\u{3B2}\u{3AE}\u{3C4}\u{3B1}</b> \
             (\u{5E1}\u{5E4}\u{5E8}) \u{5D1}\u{5D9}\u{5EA}</p>",
        ),
        (
            "he",
            "p { word-spacing: 12px }",
            "<p>\u{563}\u{56B}\u{580}\u{584} <b>\u{57F}\u{578}\u{582}\u{576}</b> \
             (\u{5E1}\u{5E4}\u{5E8}) \u{5D1}\u{5D9}\u{5EA}</p>",
        ),
        (
            "he",
            "p { word-spacing: 12px }",
            "<p>abc <b>def</b> (\u{5E1}\u{5E4}\u{5E8}) \u{5D1}\u{5D9}\u{5EA}</p>",
        ),
        (
            "he",
            "p { word-spacing: 0.5001em }",
            "<p>x \u{3B1}\u{3BB}\u{3C6}\u{3B1} \u{5E9}\u{5DC}\u{5D5}\u{5DD} \
             \u{5E2}\u{5D5}\u{5DC}\u{5DD} y.</p>",
        ),
        (
            "he",
            "p { word-spacing: 0.75em }",
            "<p dir=\"rtl\">\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5D1}\u{5E8}\u{5D0}\u{5E9}\u{5D9}\u{5EA}<br/>\
             \u{FD3E}\u{662}\u{665}\u{FD3F} <b>\u{5E2}\u{5D5}\u{5DC}\u{5DD}</b> \
             \u{5DE}\u{5DC}\u{5D7}\u{5DE}\u{5D4} \u{5D1}\u{5E8}\u{5D0}.</p>",
        ),
        (
            "he",
            "p { word-spacing: 12px }",
            "<p>\u{3B1}\u{3BB}\u{3C6}\u{3B1} <b>\u{3B2}\u{3AE}\u{3C4}\u{3B1}</b> \
             \u{5E1}\u{5E4}\u{5E8} \u{5D1}\u{5D9}\u{5EA} xyz.</p>",
        ),
    ];
    for (language, style, body) in books {
        conserves_drawn_apart(language, style, body);
    }
    // With `bundled-fonts` the stand-in's widths cut two more lines where the
    // overflow font's do not, each read whole only in pieces: an ARABIC COMMA
    // after a number in a justified Hebrew paragraph with no `dir`, whose
    // space and comma, one text object, were read with the bold word (it
    // conserved at 56dfd93), and Cyrillic words letter-spaced by half an em in
    // an Arabic paragraph with no `dir`, read from the right whole (it
    // conserved at 9297d2d and 56dfd93).
    if cfg!(feature = "bundled-fonts") {
        for (language, style, body) in [
            (
                "he",
                "p { letter-spacing: 0.1em; word-spacing: 0.25em; text-align: justify; margin: 0 35% }",
                "<p>\u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5D1}\u{5E8}\u{5D0}\u{5E9}\u{5D9}\u{5EA} 25\u{60C} \
                 <b>\u{5E2}\u{5D5}\u{5DC}\u{5DD}</b> \u{5DE}\u{5DC}\u{5D7}\u{5DE}\u{5D4}.</p>",
            ),
            (
                "ar",
                "p { letter-spacing: 0.5em }",
                "<p>\u{643}\u{62A}\u{627}\u{628} \u{43A}\u{43D}\u{438}\u{433}\u{430} \u{434}\u{43E}\u{43C} \
                 \u{441}\u{432}\u{435}\u{442} \u{434}\u{435}\u{43D}\u{44C}, \
                 \u{645}\u{62F}\u{631}\u{633}\u{629}.</p>",
            ),
        ] {
            conserves_drawn_apart(language, style, body);
        }
    }
}

/// **A letter-spaced Arabic line reads its full stop with its word** (review
/// of the K-11 fix).
///
/// `مُدَّة مُعلِّم. مُدَّة مُعلِّم..` in monospace at `letter-spacing: 8px` is one
/// line, with no `dir`: the `. ` between the two Arabic phrases is at level
/// 1, drawn ` .` as UAX #9 draws it, and the next phrase is a text object of
/// its own, written `Tc` past the stop's box — half an em at this size, the
/// slack a line closed by `ET` is resumed within. Read from the box the line
/// was cut after the stop, and the piece before the cut read
/// `مُدَّة مُعلِّم .`, its stop parted from its word. `TextDevice` carries the
/// pen across `Tc` into a right-to-left glyph now, and the line is one.
#[test]
fn a_letter_spaced_arabic_line_reads_its_full_stop_with_its_word() {
    for style in [
        "p { font-family: monospace; letter-spacing: 8px }",
        "p { font-family: monospace; letter-spacing: 0.5em }",
    ] {
        let bytes = styled_book(
            "ar",
            style,
            &format!("<p>{MUDDA_MUALLIM} {MUDDA_MUALLIM}.</p>"),
        );
        let doc = Document::open(bytes.clone()).expect("the book opens");
        let text = doc.page(0).expect("a page").text().plain_text();
        let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            words.contains(&format!("{MUDDA_MUALLIM} {MUDDA_MUALLIM}.")),
            "{style}: {text:?}"
        );
        for verdict in [
            conservation(&bytes, &doc),
            conservation_in_logical_order(&bytes, &doc),
        ] {
            assert!(verdict.holds(), "{style}: {text:?} {verdict:?}");
        }
    }
}

/// **The same word alone on its line — the book's own shape, a table with a
/// cell for the word and one for the language — is drawn right to left and
/// reads as `חו,`.**
///
/// The word: drawn `ו` left of `ח`, and read back with `ח` first, where the
/// page drew it `ח` first and read it back `וח`.
///
/// The comma: the cell is a left-to-right paragraph, so its comma, at level
/// 0, is drawn right of the word — the picture a right-to-left paragraph
/// that *opens* with the comma draws too. The cell's line holds no
/// left-to-right character, so ruling 14's "the two ends decide" read it as
/// right to left and the comma first, `,חו`, until the ruling's comma
/// tie-break (amended 10 October 2026): a strong right-to-left character
/// leftmost and closing punctuation rightmost read as a left-to-right
/// paragraph, where a left-to-right paragraph draws the line.
/// Which way such a line reads is ruling 14's to decide, not this painter's;
/// this asserts what it decided.
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
    assert!(
        text.contains(&format!("{HET_VAV},")) && !text.contains(&format!(",{HET_VAV}")),
        "the cell's comma does not trail its word: {text:?}"
    );
}

// ---- the cases the reviews found -------------------------------------------
//
// Every book a finding of the K-11 and CD-19 reviews named about what the
// writer draws is a row of the test of its mechanism here; the reader's are
// in `epub_conservation.rs`. Read as `epub_support::reading` reads a
// book: a case that was fixed is held to its right
// reading (`Expect::Holds`), a case still open is pinned at what it reads
// today (`Expect::Pinned`) and says which ROADMAP row leaves it, so that it
// fails when the limit closes and the row moves.

const SANS_NARROW: &str = "body { font-family: sans-serif } p { margin: 0 42% }";
const WS06: &str = "p { word-spacing: 0.6em }";
const WS06_20: &str = "p { word-spacing: 0.6em; margin: 0 20% }";
const LS025_JUSTIFIED: &str = "p { letter-spacing: 0.25em; text-align: justify; margin: 0 25% }";
const LS05_JUSTIFIED: &str = "p { letter-spacing: 0.5em; text-align: justify; margin: 0 25% }";
const LS055_JUSTIFIED: &str =
    "p, li, td { letter-spacing: 0.55em; text-align: justify; margin: 0 20% }";
const MONO_JUSTIFIED: &str =
    "body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }";

/// **A neutral beside a number or a styled word is drawn for the reading a
/// reader makes of its line** (`paint::neutrals_drawn_as_typed`, the
/// read-back of 935b077).
///
/// - The K-11 round-4 review's hunt H1, fixed by 56dfd93: a sign after a
///   European number opening a wrapped Arabic or Persian line in
///   sans-serif, `5%` and the sweep's `‰ ° ¢ £`, a range, a decimal and a
///   minus, read with the sign first (W2 makes the number Arabic for the
///   letter on the line above; a reader of the line alone takes it as
///   European). Hebrew, a `<br/>` instead of the wrap, and an Arabic-Indic
///   number guard it. `$5` and `#5`, the sign before the number, read `5$`
///   and `5#` at every tree since 9297d2d, pinned (ROADMAP CD-19, "Read so
///   at every tree").
/// - The fix's verification, V3, fixed by ff6d650: `٥٪ <b>W</b>` at
///   `word-spacing: 0.5em` read the sign after the next word.
/// - The review of CD-19's fix, H3 and H4(1), and the second review's H2-4,
///   H2-5 and H2-7e: C3's one text object for neutrals a reader undoes read
///   `25٪ <b>W</b>` at `1em`, `N، <b>W</b>` justified with `bundled-fonts`,
///   ornate parentheses round Arabic-Indic digits before a bold word, a `₪`
///   in a justified list item, `25، <b><i>W</i></b>` and `25٪ <b>W</b>` in a
///   right-to-left table's cell with the sign before the number; fixed by
///   935b077's read-back, but for `25، <b>W</b>` in a dir-less table's cell
///   (ROADMAP CD-19, Left (4)), pinned.
/// - The confirm review's F3: the witness of the "glyphs read alike either
///   way round" condition, and the books of its survey.
/// - The second review's fix: `5°<i>W</i>.` at `0.5em`, which lost the
///   structure tree's order 9297d2d and 56dfd93 held, outside the Left
///   list's scope, pinned.
#[test]
fn a_neutral_beside_a_number_or_a_styled_word_is_drawn_for_its_lines_reading() {
    let mut cases = Vec::new();
    for sign in ["5%", "5‰", "10–20%", "5°", "5¢", "5£", "۵%", "15.5%", "-5%"] {
        cases.push(Case::both(
            "a sign after a number opening a wrapped Arabic line (hunt H1)",
            "ar",
            SANS_NARROW,
            format!("<p dir=\"rtl\">جميل بيت كبير نور {sign} يوم</p>"),
            Expect::Holds("cons logical found apart"),
        ));
    }
    for sign in ["٥%", "٥٪"] {
        cases.push(Case::both(
            "a sign after an Arabic-Indic number opening a wrapped Arabic line (guard)",
            "ar",
            SANS_NARROW,
            format!("<p dir=\"rtl\">جميل بيت كبير نور {sign} يوم</p>"),
            Expect::Holds("cons logical whole found apart"),
        ));
    }
    for sign in ["$5", "#5"] {
        cases.push(Case::both(
            "a sign before a number opening a wrapped Arabic line (as at every tree)",
            "ar",
            SANS_NARROW,
            format!("<p dir=\"rtl\">جميل بيت كبير نور {sign} يوم</p>"),
            Expect::Pinned("found apart"),
        ));
    }
    cases.extend([
        Case::both("a sign after a number opening a wrapped Persian line (hunt H1)", "fa", SANS_NARROW, "<p dir=\"rtl\">جميل بيت كبير نور 5% يوم</p>", Expect::Holds("cons logical found apart")),
        Case::both("a sign after a number opening a wrapped dir-less line (hunt H1)", "ar", SANS_NARROW, "<p>جميل بيت كبير نور 5% يوم</p>", Expect::Holds("cons logical found apart")),
        Case::both("a sign after a number opening a Hebrew line (guard)", "he", SANS_NARROW, "<p dir=\"rtl\">שלום עולם ספר בית 5% יום</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("a sign after a number after a break (guard)", "ar", SANS_NARROW, "<p dir=\"rtl\">جميل بيت كبير نور<br/>5% يوم</p>", Expect::Holds("cons logical whole found apart")),
        Case::both(
            "٥٪ <b>W</b> three times, dir-less, word-spaced (V3)",
            "ar",
            "p { word-spacing: 0.5em; margin: 0 20% }",
            "<p>مدرسة الطالب جميل بيت ٥٪ <b>كبير</b> نور بيت كبير نور يوم ٥٪ <b>عوامل</b> وقد يوم عوامل وقد أصبحنا ٥٪ <b>نعرف</b> كتاب</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("٥٪ <b>W</b> once, dir-less, word-spaced (V3)", "ar", "p { word-spacing: 0.5em; margin: 0 20% }", "<p>مدرسة الطالب جميل بيت ٥٪ <b>كبير</b> نور</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("25٪ <b>W</b> at 1em (H3)", "ar", "p, td { word-spacing: 1em }", "<p dir=\"rtl\">جميل بيت 25٪ <b>كبير</b> نور.</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("25₪ <b>W</b> at 1em (H3)", "ar", "p, td { word-spacing: 1em }", "<p dir=\"rtl\">جميل بيت 25₪ <b>كبير</b> نور.</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("25٪ <b>W</b> at 1em, dir-less (H3)", "ar", "p { word-spacing: 1em }", "<p>كتاب عوامل 25٪ <b>مدرسة</b> الطالب.</p>", Expect::Holds("cons logical found apart")),
        Case::both("25٪ <b>W</b> at 1em, the word after (H4(1))", "ar", "p, td { word-spacing: 1em }", "<p dir=\"rtl\">نور أصبحنا 25٪ <b>نعرف</b> كتاب.</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("25٪ <b>W</b> at 1em, the word after, dir-less (H4(1))", "ar", "p, td { word-spacing: 1em }", "<p>نور أصبحنا 25٪ <b>نعرف</b> كتاب.</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("﴾٢٥﴿ <b>W</b> at 0.75em (H2-4)", "ar", "p { word-spacing: 0.75em }", "<p dir=\"rtl\">﴾٢٥﴿ <b>مدرسة</b> الطالب.</p>", Expect::Holds("cons logical found apart")),
        Case::both("﴾٢٥﴿ <b>W</b> at 1em (H2-4)", "ar", "p { word-spacing: 1em }", "<p dir=\"rtl\">﴾٢٥﴿ <b>مدرسة</b> الطالب.</p>", Expect::Holds("cons logical found apart")),
        Case::both("﴾٢٥﴿ <b>W</b> after a break, he (H2-4)", "he", "p { word-spacing: 0.75em }", "<p dir=\"rtl\">שלום בראשית<br/>﴾٢٥﴿ <b>עולם</b> מלחמה ברא.</p>", Expect::Holds("cons logical found apart")),
        Case::both("﴾٢٥﴿ <b>W</b> at 0.5em (guard)", "ar", "p { word-spacing: 0.5em }", "<p dir=\"rtl\">﴾٢٥﴿ <b>مدرسة</b> الطالب.</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("﴾٢٥﴿ <b>W</b>. at 0.75em (guard)", "ar", "p { word-spacing: 0.75em }", "<p dir=\"rtl\">﴾٢٥﴿ <b>مدرسة</b>.</p>", Expect::Holds("cons logical whole found apart")),
    ]);
    let list = "p, li, td { text-align: justify; letter-spacing: 0.1em; word-spacing: 0.25em; margin: 0 30% }";
    for sign in ["₪", "٪"] {
        cases.push(Case::both(
            "25₪ <b>W</b> in a justified list item (H2-5)",
            "ar",
            list,
            format!(
                "<div dir=\"rtl\"><ol><li>كتاب عوامل 25{sign} <b>مدرسة</b> الطالب.</li></ol></div>"
            ),
            Expect::Holds("cons logical whole found apart"),
        ));
    }
    cases.push(Case::both(
        "25₪ <b>W</b> in a justified paragraph (H2-5)",
        "ar",
        list,
        "<p dir=\"rtl\">كتاب عوامل 25₪ <b>مدرسة</b> الطالب.</p>",
        Expect::Holds("cons logical whole found apart"),
    ));
    let justified =
        "p { letter-spacing: 0.1em; word-spacing: 0.25em; text-align: justify; margin: 0 35% }";
    let breadth = "p { letter-spacing: 0.1em; word-spacing: 0.25em; text-align: justify; margin: 0 35% } td { letter-spacing: 0.1em; word-spacing: 0.5em }";
    for sign in ["25\u{60C}", "\u{27E8}25\u{27E9}"] {
        for attr in [" dir=\"rtl\"", ""] {
            for style in [justified, breadth] {
                cases.push(Case::each(
                    "25، <b>W</b> justified, he (H3(a))",
                    "he",
                    style,
                    format!("<p{attr}>שלום בראשית {sign} <b>עולם</b> מלחמה.</p>"),
                    Expect::Holds("cons logical whole found apart"),
                    Expect::Holds("cons logical found apart"),
                ));
            }
        }
    }
    cases.extend([
        Case::both(
            "25، <b><i>W</i></b> at 0.75em, he (H2-7e)",
            "he",
            "p, li, td { word-spacing: 0.75em }",
            "<p dir=\"rtl\">שלום בראשית 25، <b><i>עולם</i></b> מלחמה, ספר 25،.</p>", Expect::Holds("cons logical whole found apart")),
        Case::both(
            "25، <b>W</b> in a dir-less table cell, ar (H2-7c of the fix, Left (4))",
            "ar",
            "p, li, td { word-spacing: 16px; letter-spacing: 1px }",
            "<div><table><tr><td>كتاب 25، <b>مدرسة</b></td><td>25، الطالب</td></tr></table></div>", Expect::Pinned("found apart")),
        Case::both(
            "25، <b>W</b> in a dir-less table cell, he (H2-7c of the fix, Left (4))",
            "he",
            "p, li, td { word-spacing: 16px; letter-spacing: 1px }",
            "<div><table><tr><td>שלום 25، <b>עולם</b></td><td>25، מלחמה</td></tr></table></div>", Expect::Pinned("found apart")),
        Case::both(
            "25٪ <b>W</b> in a table cell (H2-7e)",
            "ar",
            "p, li, td { word-spacing: 16px; letter-spacing: 1px }",
            "<div dir=\"rtl\"><table><tr><td>كتاب 25٪ <b>مدرسة</b></td><td>25٪ الطالب</td></tr></table></div>", Expect::Holds("cons logical whole found apart")),
    ]);
    for (style, attr) in [
        (
            "p { word-spacing: 0.3em; letter-spacing: 0.1em }",
            " dir=\"rtl\"",
        ),
        ("p { word-spacing: 0.3em; letter-spacing: 0.1em }", ""),
        ("p { word-spacing: 1em }", " dir=\"rtl\""),
    ] {
        cases.push(Case::both(
            "a word-spaced ` ?!` slice (confirm F3)",
            "ar",
            style,
            format!("<p{attr}>عوامل<i> ?!</i> عدّة وقد أصبحنا</p>"),
            Expect::Holds("cons logical whole found apart"),
        ));
    }
    cases.extend([
        Case::both(
            "a word-spaced ` :-)` slice (confirm F3)",
            "ar",
            "p { word-spacing: 1em }",
            "<p dir=\"rtl\">عوامل<b> :-)</b> عدّة</p>",
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::both(
            "a word-spaced ` #1` slice (confirm F3)",
            "ar",
            "p { word-spacing: 1em }",
            "<p dir=\"rtl\">عوامل<i> #1</i> عدّة</p>",
            Expect::Holds("cons logical found apart"),
        ),
        Case::both(
            "a word-spaced ` ?!` slice, monospace, he (confirm F3)",
            "he",
            "p { word-spacing: 0.5em; font-family: monospace }",
            "<p dir=\"rtl\">שלום<i> ?!</i> עולם מלחמה</p>",
            Expect::Holds("cons logical whole found apart"),
        ),
    ]);
    let s = |i: usize| word(&AR, i);
    let glued = rep(3, |i| {
        format!(
            "{} {} {} {} 5°<i>{}</i>. {}",
            s(i + 4),
            s(i + 5),
            s(i + 6),
            s(i + 7),
            s(i + 8),
            s(i + 9)
        )
    });
    for attr in [" dir=\"rtl\"", ""] {
        cases.push(Case::both(
            "5°<i>W</i>. word-spaced (fv_extra m/ar/n4/glued)",
            "ar",
            "p { word-spacing: 0.5em; margin: 0 20% }",
            format!("<p{attr}>{glued}</p>"),
            Expect::Pinned("found apart"),
        ));
    }
    check(&cases);
}

/// **A styled neutral glued to a word under spacing reads where it was
/// typed** (`paint::Gaps`, and the read-back of 935b077, which reads every
/// line holding a right-to-left character back through `TextDevice` and
/// ruling 14 and draws it otherwise where they read it otherwise).
///
/// - The K-11 round-4 review's hunt H3, fixed by 56dfd93: a styled `?!`
///   glued to the word after it, letter-spaced and justified, read `!?`.
/// - Its hunt H4 and the fix's verification V2: CD-19's code-32 move put a
///   word over the styled neutrals glued to it, and K-11's carry read them
///   interleaved, so search lost the word; the overflow font at 0 Tw
///   (e7ca679) moves nothing. With the overflow font's code 32 another
///   letter, with the stop unstyled, or with no word spacing, the book reads
///   right as well (held).
/// - The review of CD-19's fix, CR1, F1, F2, F4 and F5, and the second
///   review's H2-1 and H2-6: books that read right only because the moved
///   glyph closed or widened a gap, fixed by ce0b3de's one text object for
///   the inside of a spaced slice and 935b077's read-back, the `?!` glued to
///   a word at `0.5em`, which no tree before 935b077 read right, among them;
///   but for `;` glued to a word, which reads whole no more, and an inline
///   `word-spacing` span cutting a word (ROADMAP CD-19, Left (5)), pinned.
///   H2-6, a letter-spaced dir-less Arabic line with Cyrillic words, was
///   `bundled-fonts`' (fixed); a default build reads it, and its Greek twin,
///   from the right at every tree since 9297d2d, `.مدرسة ,книга дом свет
///   день كتاب`, pinned (ROADMAP CD-19, "Read so at every tree").
#[test]
fn a_styled_neutral_glued_to_a_word_under_spacing_reads_where_it_was_typed() {
    let right = Expect::Holds("cons logical whole found apart");
    let ar = |i: usize| word(&AR, i);
    let he = |i: usize| word(&HE, i);
    let mut cases = vec![
        Case::both(
            "a styled ?! glued to the word after it (hunt H3)",
            "ar",
            LS025_JUSTIFIED,
            "<p dir=\"rtl\">كتاب <i>?!</i> مدرسة <b>?!</b>الطالب جميل.</p>",
            Expect::Holds("logical found apart"),
        ),
        Case::both(
            "a styled ?! glued to the word after it, he (hunt H3)",
            "he",
            LS025_JUSTIFIED,
            "<p dir=\"rtl\">שלום <i>?!</i> עולם <b>?!</b>מלחמה ספר.</p>",
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::both(
            "a styled ... glued to the word after it (guard)",
            "ar",
            LS025_JUSTIFIED,
            "<p dir=\"rtl\">كتاب <i>...</i> مدرسة <b>...</b>الطالب جميل.</p>",
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::both(
            "a styled stop glued to a word (hunt H4)",
            "ar",
            WS06,
            "<p dir=\"rtl\">ا <b>.</b>أصبحنا ن</p>",
            Expect::Holds("cons logical found apart"),
        ),
        Case::both(
            "a styled ... glued to a word (hunt H4)",
            "ar",
            WS06,
            "<p dir=\"rtl\">ال <i>...</i> وقد <b>...</b>أصبحنا نعرف.</p>",
            Expect::Holds("cons logical found apart"),
        ),
        Case::both(
            "a styled stop glued to a word, KAF at code 32 (guard)",
            "ar",
            WS06,
            "<p>ك</p><p dir=\"rtl\">ا <b>.</b>أصبحنا ن</p>",
            Expect::Holds("cons logical found apart"),
        ),
        Case::both(
            "an unstyled stop glued to a word (guard)",
            "ar",
            WS06,
            "<p dir=\"rtl\">ا .أصبحنا ن</p>",
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::both(
            "a styled stop glued to a word, no word spacing (guard)",
            "ar",
            "p { word-spacing: 0 }",
            "<p dir=\"rtl\">ا <b>.</b>أصبحنا ن</p>",
            Expect::Holds("cons logical whole found apart"),
        ),
    ];
    let conserves = Expect::Holds("cons logical found apart");
    let in_order = Expect::Holds("logical found apart");
    for (pi, piece, expects) in [
        (1, "!!", [conserves, conserves, conserves, conserves]),
        (2, "...", [conserves, conserves, conserves, right]),
        (14, "?! ?!", [in_order, right, right, in_order]),
    ] {
        let text = rep(3, |i| {
            format!(
                "{} <i>{piece}</i> {} <b>{piece}</b>{} {}.",
                ar(i + pi),
                ar(i + pi + 1),
                ar(i + pi + 2),
                ar(i + pi + 3)
            )
        });
        let styles = [
            "p { word-spacing: 0.51em; margin: 0 20% }",
            WS06_20,
            "p { word-spacing: 1em; margin: 0 20% }",
            MONO_JUSTIFIED,
        ];
        for (style, expect) in styles.into_iter().zip(expects) {
            cases.push(Case::both(
                "styled neutrals glued to a word, word-spaced (hunt gaps, H4 and CR1)",
                "ar",
                style,
                format!("<p dir=\"rtl\">{text}</p>"),
                expect,
            ));
        }
    }
    let both = |s: &dyn Fn(usize) -> &'static str, pi: usize, piece: &str| {
        rep(3, |i| {
            format!(
                "{} {}<b>{piece}</b>{} {}.",
                s(i + pi),
                s(i + pi + 1),
                s(i + pi + 2),
                s(i + pi + 3)
            )
        })
    };
    for (language, text, bundled) in [
        ("he", both(&he, 0, "?!"), conserves),
        ("he", both(&he, 6, "»,"), right),
        ("ar", both(&ar, 2, "!!"), right),
    ] {
        cases.push(Case::each(
            "styled neutrals glued on both sides (V2)",
            language,
            WS06_20,
            format!("<p dir=\"rtl\">{text}</p>"),
            right,
            bundled,
        ));
        cases.push(Case::both(
            "styled neutrals glued on both sides (V2)",
            language,
            "p { word-spacing: 1em; margin: 0 20% }",
            format!("<p dir=\"rtl\">{text}</p>"),
            right,
        ));
    }
    cases.push(Case::both(
        "a styled !! opening a monospace justified paragraph (V2)",
        "ar",
        MONO_JUSTIFIED,
        format!("<p dir=\"rtl\"><b>!!</b>{} {} {}.</p>", ar(2), ar(3), ar(4)),
        Expect::Holds("cons logical whole found apart"),
    ));
    let gap8 = rep(3, |i| {
        format!(
            "{} <i>;</i> {} <b>;</b>{} {}.",
            ar(i + 8),
            ar(i + 9),
            ar(i + 10),
            ar(i + 11)
        )
    });
    cases.push(Case::both(
        "a styled ; glued to a word, letter-spaced (CR1, Left (5))",
        "ar",
        LS025_JUSTIFIED,
        format!("<p dir=\"rtl\">{gap8}</p>"),
        Expect::Pinned("cons logical found apart"),
    ));
    let styled = rep(6, |i| {
        format!(
            "{} <b>{}</b> <i>?!</i> {} <b>(</b>{}<b>)</b> {}.",
            ar(i),
            ar(i + 1),
            ar(i + 2),
            ar(i + 3),
            ar(i + 4)
        )
    });
    for (attr, expect) in [(" dir=\"rtl\"", right), ("", in_order)] {
        cases.push(Case::both(
            "styled neutrals between styled words, letter-spaced and justified (CR1, F1)",
            "ar",
            "p { letter-spacing: 0.45em; text-align: justify; margin: 0 32% }",
            format!("<p{attr}>{styled}</p>"),
            expect,
        ));
    }
    cases.push(Case::each(
        "?! before styled braces after a break, letter-spaced (CR1, F2)",
        "he",
        LS05_JUSTIFIED,
        "<p dir=\"rtl\">אור יום<br/>?!<b>{</b>בראשית<b>}</b> ברא אלהים</p>",
        Expect::Holds("cons logical whole found apart"),
        Expect::Holds("cons logical found apart"),
    ));
    let midws = rep(6, |i| {
        let cut: Vec<char> = ar(i).chars().collect();
        let k = cut.len() / 2;
        format!(
            "{} <span style=\"word-spacing: 1em\">{} {}</span>{} ({}).",
            ar(i + 1),
            ar(i + 3),
            cut[..k].iter().collect::<String>(),
            cut[k..].iter().collect::<String>(),
            ar(i + 2)
        )
    });
    cases.push(Case::both(
        "an inline word-spacing span cutting a word (CR1, Left (5))",
        "ar",
        "",
        format!("<p>{midws}</p>"),
        Expect::Pinned("found apart"),
    ));
    let s8 = |i: usize| word(&AR8, i);
    let long_neutral = format!(
        "{} {} {} <i>?! -- ?!</i> {} {} {} {}.",
        s8(0),
        s8(1),
        s8(2),
        s8(3),
        s8(4),
        s8(5),
        s8(6)
    );
    for (body, expect) in [
        (format!("<p dir=\"rtl\">{long_neutral}</p>"), right),
        (format!("<p>{long_neutral}</p>"), conserves),
        (
            format!("<div style=\"direction: rtl\"><p>{long_neutral}</p></div>"),
            right,
        ),
    ] {
        cases.push(Case::both(
            "a long run of neutrals mid-line, letter-spaced (F5)",
            "ar",
            LS055_JUSTIFIED,
            body,
            expect,
        ));
    }
    for inside in ["[HDAC]", "«HDAC»"] {
        let text: String = (0..3)
            .map(|i| {
                format!(
                    "{} {} {} {inside}, {} {} {} {}. ",
                    s8(i),
                    s8(i + 1),
                    s8(i + 2),
                    s8(i + 3),
                    s8(i + 4),
                    s8(i + 5),
                    s8(i + 6)
                )
            })
            .collect();
        cases.push(Case::both(
            "a bracketed run three times, letter-spaced (F5)",
            "ar",
            LS055_JUSTIFIED,
            format!("<p>{text}</p>"),
            Expect::Holds("cons logical found apart"),
        ));
    }
    let greek: String = (0..3)
        .map(|i| {
            format!(
                "{} {} (αλφα βήτα), {} {} {} {}. ",
                s8(i),
                s8(i + 1),
                s8(i + 3),
                s8(i + 4),
                s8(i + 5),
                s8(i + 6)
            )
        })
        .collect();
    cases.push(Case::both(
        "a bracketed Greek run three times, letter-spaced (F5, code greek-x3)",
        "ar",
        LS055_JUSTIFIED,
        format!("<p>{greek}</p>"),
        Expect::Holds("cons logical found apart"),
    ));
    let h8 = |i: usize| HEP[i % 8];
    let own = (0..5)
        .map(|i| {
            format!(
                "{} <b>.</b>{} <i>?!</i> {} <b>(</b>{}<b>)</b> {};",
                h8(i),
                h8(i + 1),
                h8(i + 2),
                h8(i + 3),
                h8(i + 4)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    cases.push(Case::each(
        "a glued <b>.</b> and an italic ?!, pointed he, monospace (F4)",
        "he",
        MONO_JUSTIFIED,
        format!("<p>{own}</p>"),
        Expect::Holds("cons logical found apart"),
        Expect::Holds("logical found apart"),
    ));
    let soft = rep(2, |i| {
        format!(
            "{} {} {} \u{60C}({}) {} {}.",
            ar(i + 18),
            ar(i + 19),
            ar(i + 20),
            ar(i + 21),
            ar(i + 22),
            ar(i + 23)
        )
    });
    cases.push(Case::both(
        "، before a pair after a space, letter-spaced (F5, soft/ar/acomma)",
        "ar",
        LS05_JUSTIFIED,
        format!("<p dir=\"rtl\">{soft}</p>"),
        Expect::Holds("cons logical found apart"),
    ));
    let three = rep(3, |i| {
        format!(
            "{} {} {} <b>(</b>{}<b>)</b> {}!!",
            ar(i + 2),
            ar(i + 3),
            ar(i + 4),
            ar(i + 5),
            ar(i + 6)
        )
    });
    cases.push(Case::both(
        "(W) W!! three times, dir-less, letter-spaced (F5, end/ar/ee)",
        "ar",
        LS05_JUSTIFIED,
        format!("<p>{three}</p>"),
        Expect::Holds("cons logical whole found apart"),
    ));
    cases.push(Case::both(
        "a Greek word before Hebrew at 0.5001em (F5, mf/greek-he)",
        "en",
        "p { word-spacing: 0.5001em }",
        "<p>x αλφα שלום עולם y.</p>",
        Expect::Holds("cons logical whole found apart"),
    ));
    for (what, language, style, body) in [
        ("a styled ?! glued to a word, ar (H2-1)", "ar", WS06, "<p dir=\"rtl\"><b>?!</b>ابا اما.</p>"),
        ("a styled ?! glued to a word at 2em (H2-1)", "ar", "p { word-spacing: 2em }", "<p dir=\"rtl\"><b>?!</b>ابا اما.</p>"),
        ("an italic ?! glued to a word (H2-1)", "ar", WS06, "<p dir=\"rtl\"><i>?!</i>ابا اما.</p>"),
        ("a styled ?! glued to a word, he (H2-1)", "he", WS06, "<p dir=\"rtl\"><b>?!</b>אבא אמא.</p>"),
        (
            "a styled ?! glued to a word, a Syriac caption (H2-1)",
            "syr",
            "p, li, figcaption { word-spacing: 0.6em }",
            "<div dir=\"rtl\"><figure><figcaption><i>?!</i>ܐܠܗܐ ܐܪܥܐ ܫܡܝܐ ܡܠܬܐ ܪܒܐ.</figcaption></figure></div>",
        ),
        ("a styled ?! glued to a word with its letter once (H2-7c)", "ar", WS06, "<p dir=\"rtl\"><b>?!</b>ابو اما.</p>"),
        ("a styled ? glued to a word (guard)", "ar", WS06, "<p dir=\"rtl\"><b>?</b>ابا اما.</p>"),
        ("a styled ?! glued to a word at 0.45em (guard)", "ar", "p { word-spacing: 0.45em }", "<p dir=\"rtl\"><b>?!</b>ابا اما.</p>"),
        ("a styled ?! glued to a word at 0.5em (H2-1)", "ar", "p { word-spacing: 0.5em }", "<p dir=\"rtl\"><b>?!</b>ابا اما.</p>"),
        ("a letter-spaced dir-less Arabic line with Latin words (guard)", "ar", "p { letter-spacing: 0.5em }", "<p>كتاب book house light day, مدرسة.</p>"),
    ] {
        cases.push(Case::both(what, language, style, body, right));
    }
    for (what, body) in [
        (
            "a letter-spaced dir-less Arabic line with Cyrillic words (H2-6)",
            "<p>كتاب книга дом свет день, مدرسة.</p>",
        ),
        (
            "a letter-spaced dir-less Arabic line with Greek words (as at every tree)",
            "<p>كتاب αλφα βήτα γάμμα δέλτα, مدرسة.</p>",
        ),
    ] {
        cases.push(Case::each(
            what,
            "ar",
            "p { letter-spacing: 0.5em }",
            body,
            Expect::Pinned("found apart"),
            right,
        ));
    }
    check(&cases);
}

/// **A cell or a spaced slice is one text object only where its line reads
/// it whole** (`paint::draw_coded`'s inside of a spaced slice,
/// `paint::sharing_a_line`).
///
/// - H2 of the review of CD-19's fix, fixed by ce0b3de: a Greek or Cyrillic
///   cell before a Hebrew or Arabic one at `word-spacing: 0.6em` or `1em`
///   was cut at each stretched space and its last word read with the next
///   cell (breadth B's `αλφα βήτα | שלום` row, in a default build, by
///   e7ca679's 0 Tw already).
/// - R1-TABLECOST of its verification, fixed by 935b077: a dir-less table's
///   `abc (x).` cell touching a right-to-left one read the row as a
///   right-to-left line. Its cost, a Latin or Greek cell's row whose
///   structure tree read in order only at ce0b3de, through the join that fix
///   took back, and which no tree conserved, is pinned (outside the Left
///   list's scope); in a default build the Greek row letter-spaced by
///   `0.1em` reads in order since c7e9b89 (held).
/// - CR3 of the review of CD-19's fix: the right-to-left table whose
///   `أصبحنا نعرف` cells 9297d2d and 56dfd93 read `أصبحنان عرف` finds both
///   words since e7ca679.
#[test]
fn a_cell_or_a_spaced_slice_is_one_text_object_only_where_its_line_reads_it_whole() {
    let mut cases = vec![
        Case::both(
            "a Greek cell before a Hebrew one at 1em (H2)",
            "he",
            "td { word-spacing: 1em }",
            "<table><tr><td>αβ γδ</td><td>ספר</td></tr></table>",
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::both(
            "a Greek cell before a Hebrew one at 0.6em (H2)",
            "he",
            "td { word-spacing: 0.6em }",
            "<table><tr><td>αβ γδ</td><td>ספר</td></tr></table>",
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::both(
            "a Greek cell before a pointed Hebrew one (H2)",
            "he",
            "td { word-spacing: 1em }",
            "<table><tr><td>αλφα βήτα</td><td>סֵפֶר בַּיִת</td></tr></table>",
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::both(
            "a Cyrillic cell before an Arabic one (H2)",
            "ar",
            "p, td { word-spacing: 1em }",
            "<table><tr><td>привет мир</td><td>كتاب</td></tr></table>",
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::both(
            "a Greek cell before a Hebrew one, breadth B (H2)",
            "he",
            "p, td { word-spacing: 1em }",
            "<table><tr><td>αλφα βήτα</td><td>שלום</td></tr></table>",
            Expect::Holds("cons logical whole found apart"),
        ),
    ];
    let cost = Expect::Pinned("whole found apart");
    for (style, greek) in [
        ("table { border-spacing: 0 } td { padding: 0; word-spacing: 1em }", cost),
        (
            "table { border-spacing: 0 } td { padding: 0; letter-spacing: 0.1em; word-spacing: 0.5em }",
            Expect::Holds("logical whole found apart"),
        ),
    ] {
        for (language, cell) in [("ar", "أصبحنا نعرف"), ("he", "ספר בית")] {
            cases.push(Case::both(
                "an abc (x). cell touching a right-to-left one (R1-TABLECOST)",
                language,
                style,
                format!("<table><tr><td>abc (x).</td><td>{cell}</td></tr><tr><td>{cell}</td><td>abc (x).</td></tr></table>"), Expect::Holds("whole found apart")));
        }
        for (before, default) in [("abc def", cost), ("αλφα βήτα", greek)] {
            cases.push(Case::each(
                "a Latin or Greek cell touching an Arabic one (R1-TABLECOST's cost)",
                "ar",
                style,
                format!("<table><tr><td>{before}</td><td>أصبحنا نعرف</td></tr><tr><td>أصبحنا نعرف</td><td>{before}</td></tr></table>"),
                default,
                cost,
            ));
        }
    }
    let rows: String = [
        "<td>عوامل</td><td>عدّة</td><td>وقد.</td><td>(HDAC)</td>",
        "<td>أصبحنا نعرف</td><td>الآن</td><td>(2010)</td><td>x</td>",
        "<td>«الموادّ»</td><td>تكبح</td><td>[عوامل]</td><td>عدّة</td>",
        "<td>abc</td><td>وقد</td><td>def</td><td>أصبحنا نعرف</td>",
    ]
    .iter()
    .map(|row| format!("<tr>{row}</tr>"))
    .collect();
    for style in ["", "body { font-family: sans-serif }"] {
        for rows in [
            rows.clone(),
            rows.replace("<td>", "<td style=\"padding: 0\">"),
        ] {
            cases.push(Case::both(
                "a right-to-left table's أصبحنا نعرف cells (CR3)",
                "ar",
                style,
                format!("<table dir=\"rtl\" style=\"border-spacing: 0; word-spacing: 1em\">{rows}</table>"), Expect::Holds("cons logical whole found apart")));
        }
    }
    check(&cases);
}

/// **A word of an overflow script an inline block cuts reads as a Latin word
/// so cut does** — ROADMAP CD-19, Left (2), pinned; H2-3 of the second
/// review of CD-19's fix.
///
/// `<p>αλφα<span style="display: inline-block; width: 1em"></span>βήτα</p>`
/// at `word-spacing: 0.6em`: Greek, Cyrillic or Armenian words read whole
/// and were found at 9297d2d and 56dfd93 only because the code-32 letter's
/// move closed the block's gap; at 0 Tw the word reads in two, as a Latin
/// word so cut does at every tree, and a Greek one with no word spacing
/// (pinned with them). With `bundled-fonts` the Greek and Cyrillic words read
/// as at every tree, and the Armenian one as in a default build. A padded
/// inline span, not a block, keeps the word whole (held). These scripts read
/// as Latin does, in two, by the owner's decision (ruling 14), and each case
/// is held at exactly that reading. And a
/// pre-existing one the hunt found: the writer draws no Adlam letter in
/// either build, at any tree (pinned; ROADMAP CD-19, "Read so at every
/// tree").
#[test]
fn a_word_of_an_overflow_script_an_inline_block_cuts_reads_as_a_latin_word_does() {
    let block = "<span style=\"display: inline-block; width: 1em\"></span>";
    check(&[
        Case::each("a Greek word an inline block cuts (H2-3)", "el", WS06, format!("<p>αλφα{block}βήτα</p>"), Expect::Pinned("cons logical apart"), Expect::Holds("cons logical apart")),
        Case::each("a Cyrillic word an inline block cuts (H2-3)", "ru", WS06, format!("<p>дома{block}книга свет.</p>"), Expect::Pinned("cons logical apart"), Expect::Holds("cons logical apart")),
        Case::both("an Armenian word an inline block cuts (H2-3)", "hy", WS06, format!("<p>գիրք{block}տուն մեծ լույս.</p>"), Expect::Pinned("cons logical apart")),
        Case::each(
            "a Greek word a padded inline block cuts (H2-3)",
            "el",
            WS06,
            "<p>αλφα<span style=\"display: inline-block; padding-left: 1em\">βήτα</span> γάμμα.</p>", Expect::Pinned("cons logical apart"), Expect::Holds("cons logical apart")),
        Case::both("a Latin word an inline block cuts (as at every tree)", "en", WS06, format!("<p>abcd{block}efgh</p>"), Expect::Pinned("cons logical apart")),
        Case::both("a Greek word an inline block cuts, no word spacing (as at every tree)", "el", "", format!("<p>αλφα{block}βήτα</p>"), Expect::Pinned("cons logical apart")),
        Case::both("a Greek word a padded inline span cuts (guard)", "el", WS06, "<p>αλφα<span style=\"padding-left: 1em\">βήτα</span> γάμμα.</p>", Expect::Holds("cons logical whole found apart")),
        Case::both("Adlam (hunt, as at every tree)", "ff", "", "<p>𞤀𞤣𞤤𞤢𞤥 𞤆𞤵𞤤𞤢𞤪.</p>", Expect::Pinned("apart")),
    ]);
}

/// The punctuation shape of the round-6 sweeps: a styled `?!` glued to a
/// word, a full stop, a styled bracket pair and a `!`, three times.
fn styled_marks(words: &[&'static str; 12]) -> String {
    rep(3, |i| {
        format!(
            "{} <b>?!</b> {} {}. <i>(</i>{}<i>)</i> {}!",
            word(words, i),
            word(words, i + 1),
            word(words, i + 2),
            word(words, i + 3),
            word(words, i + 4)
        )
    })
}

/// The number shape of the round-6 sweeps: a sign after European digits, a
/// bold word, Arabic-Indic digits and a year with the ARABIC COMMA.
fn signs_and_numbers(sentences: usize) -> String {
    rep(sentences, |i| {
        format!(
            "{} 25\u{66A} <b>{}</b> \u{661}\u{662}\u{663} {} 2010\u{60C} {}.",
            word(&AR, i),
            word(&AR, i + 1),
            word(&AR, i + 2),
            word(&AR, i + 3)
        )
    })
}

/// **A line drawn under a CSS transform is read back on the page it is drawn
/// on** (V1 of round 6's lane V; ROADMAP CD-19).
///
/// The writer reads each line that holds a right-to-left character back as a
/// reader reads it (`paint::Fonts::set_readings`), and two of its rules ask
/// whether a reader parts two text objects of a run, or two runs
/// (`paint::parted`, `paint::page_lines`). A transform writes a `cm`
/// before the text (`paint::OnPage::open`), and a scale of one axis widens or
/// narrows every gap on the page against the glyphs' size, which is what a
/// reader's thresholds are in. Read back under no matrix, a styled `?!` glued
/// to a Hebrew word at `word-spacing: 0.6em` under `scaleX(2)` was read with
/// the word and drawn turned for that reading; on the page, where the gap is
/// twice as wide, it was read alone, and turned: `שלום!? עולם.`, where
/// 9297d2d and 56dfd93 read `שלום?! עולם.`. Each line is read back under the
/// matrix the page draws it under now (`paint::OnPage::matrix`), and both
/// rules ask their question in the page's ems (`paint::along_the_line`):
/// under `scaleX(3)` a `letter-spacing` of `0.3em` parts the line on the page
/// and not in the run. Under `scaleY(2)` the gaps narrow against the glyphs
/// instead, and with `bundled-fonts` the Hebrew book read `!?` at 05cea0b.
///
/// `25٪ <b>W</b>` in Arabic under `scaleX(1.4)`, which conserved at 56dfd93
/// and not since round 5, reads right again. Under a half turn at
/// `word-spacing: 1em` three such sentences conserved at 56dfd93 only because
/// the overflow font's moved glyphs drew the bold word apart around its sign;
/// at `0 Tw` no drawing the read-back tries reads the line right (ROADMAP
/// CD-19, Left (6)), pinned. A line the rules' drawing reads on the page with
/// its letters in order and not its words is searched in its runs' own space
/// too, and what that finds is drawn only where it reads the line's words
/// right on the page (`paint::Fonts::reads_whole`): taken as found, it turned
/// the third `?!` of the sweep's punctuation shape, with no `dir`, under
/// `scaleX(2)` for a reading only that space has, and the book, which
/// conserved at 56dfd93, did not; and a styled `?!` glued to an Arabic word
/// with no `dir`, which read `!?` at 05cea0b in either build, read so again.
/// The controls read as they did: no transform, `scale(2)`, whose gaps keep
/// their size against the glyphs, and a Latin paragraph, which is not read
/// back.
#[test]
fn a_line_drawn_under_a_transform_is_read_back_on_the_page_it_is_drawn_on() {
    let right = Expect::Holds("cons logical whole found apart");
    let he = "<p dir=\"rtl\">שלום <b>?!</b> עולם.</p>";
    let ar = "<p dir=\"rtl\">كتاب <b>?!</b> مدرسة.</p>";
    let prose = |w: &[&'static str; 12]| {
        let text = rep(3, |i| {
            format!(
                "{} {} {}\u{60C} {} {} {}.",
                word(w, i),
                word(w, i + 1),
                word(w, i + 2),
                word(w, i + 3),
                word(w, i + 4),
                word(w, i + 5)
            )
        });
        format!("<p dir=\"rtl\">{text}</p>")
    };
    let mut cases = vec![
        Case::both("a styled ?! glued to a Hebrew word under scaleX(2) at 0.6em (V1)", "he", "p { transform: scaleX(2); word-spacing: 0.6em }", he, right),
        Case::both("the same at 1em (V1)", "he", "p { transform: scaleX(2); word-spacing: 1em }", he, right),
        Case::both("the same under scaleX(1.4) at 1em (V1)", "he", "p { transform: scaleX(1.4); word-spacing: 1em }", he, right),
        Case::both("the same with no dir (V1)", "he", "p { transform: scaleX(2); word-spacing: 0.6em }", "<p>שלום <b>?!</b> עולם.</p>", Expect::Holds("cons logical found apart")),
        Case::both("a styled ?! glued to an Arabic word under scaleX(2) (V1)", "ar", "p { transform: scaleX(2); word-spacing: 0.6em }", ar, right),
        Case::both("the same with no dir (V1's class, in either build)", "ar", "p { transform: scaleX(2); word-spacing: 0.6em }", "<p>كتاب <b>?!</b> مدرسة.</p>", Expect::Holds("cons logical found apart")),
        Case::both(
            "a sign after European digits before a bold word under scaleX(1.4) (V1, since round 5)",
            "ar",
            "p { margin: 0 30%; transform: scaleX(1.4); word-spacing: 0.6em }",
            format!("<p dir=\"rtl\">{}</p>", signs_and_numbers(1)),
            right,
        ),
        Case::both(
            "three such sentences under a half turn at 1em (V1, conserved at 56dfd93 only; Left (6))",
            "ar",
            "p { margin: 0 30%; transform: rotate(180deg); word-spacing: 1em }",
            format!("<p dir=\"rtl\">{}</p>", signs_and_numbers(3)),
            Expect::Pinned("logical found apart"),
        ),
        Case::both("one under a half turn at 1em (guard)", "ar", "p { margin: 0 30%; transform: rotate(180deg); word-spacing: 1em }", format!("<p dir=\"rtl\">{}</p>", signs_and_numbers(1)), right),
        Case::both("Hebrew prose under scaleX(3), letter-spaced by 0.3em (the rules, f6 sweep)", "he", "p { margin: 0 30%; transform: scaleX(3); letter-spacing: 0.3em }", prose(&HE), Expect::Holds("cons logical found")),
        Case::both("Arabic prose under scaleX(3), letter-spaced by 0.3em (the rules, f6 sweep)", "ar", "p { margin: 0 30%; transform: scaleX(3); letter-spacing: 0.3em }", prose(&AR), Expect::Holds("cons logical found")),
        Case::both("no transform (control)", "he", "p { word-spacing: 0.6em }", he, Expect::Holds("cons logical found apart")),
        Case::both("scale(2), both axes alike (control)", "he", "p { transform: scale(2); word-spacing: 0.6em }", he, Expect::Holds("cons logical found apart")),
        Case::both("scaleY(2), the gaps narrower against the glyphs (V1 with bundled-fonts)", "he", "p { transform: scaleY(2); word-spacing: 0.6em }", he, right),
        Case::both("a Latin paragraph under scaleX(2) at 1em, nothing right to left (guard)", "en", "p { transform: scaleX(2); word-spacing: 1em }", "<p>The word <b>?!</b> means <i>peace</i>, (here) 25%.</p>", right),
    ];
    for (style, language, words, expect) in [
        (
            "p { margin: 0 30%; transform: scaleX(2); word-spacing: 0.6em }",
            "he",
            HE,
            right,
        ),
        (
            "p { margin: 0 30%; transform: scaleX(0.5); word-spacing: 0.6em }",
            "ar",
            AR,
            right,
        ),
        // A skew sets each glyph's box over the next one's: not a spacing.
        (
            "p { margin: 0 30%; transform: skewX(-15deg); word-spacing: 0.6em }",
            "he",
            HE,
            Expect::Holds("cons logical whole found"),
        ),
        (
            "p { margin: 0 30%; transform: rotate(12deg); word-spacing: 0.6em }",
            "ar",
            AR,
            right,
        ),
    ] {
        cases.push(Case::both(
            "the punctuation shape under a transform at 0.6em (f6 sweep)",
            language,
            style,
            format!("<p dir=\"rtl\">{}</p>", styled_marks(&words)),
            expect,
        ));
    }
    // With no `dir`, where the rules' drawing reads a line's letters in order
    // on the page and not its words: what a search in the runs' own space
    // finds turned the third `?!` for a reading in which it joined its word,
    // and on the page it read `!?` alone, where the rules' drawing reads it
    // right. They conserved at 56dfd93, and not at 935b077.
    for style in [
        "p { margin: 0 30%; transform: scaleX(2); word-spacing: 0.6em } .w { transform: none }",
        "p { margin: 0 30%; transform: scaleX(3); word-spacing: 0.6em } .w { transform: none }",
        "p { margin: 0 30%; transform: scaleY(0.5); word-spacing: 0.6em } .w { transform: none }",
        "p { margin: 0 30%; transform: matrix(1.6, 0, 0, 0.8, 0, 0); word-spacing: 0.6em } .w { transform: none }",
        "p { margin: 0 30%; transform: none; word-spacing: 0.6em } .w { transform: scaleX(2) }",
    ] {
        cases.push(Case::both(
            "the punctuation shape with no dir under a transform at 0.6em (f6 sweep)",
            "he",
            style,
            format!("<div class=\"w\"><p>{}</p></div>", styled_marks(&HE)),
            Expect::Holds("cons logical found"),
        ));
    }
    check(&cases);
}

/// **A line is read back as the page's fonts give its characters back**
/// (round 6's lane F, the sweep of V1's class).
///
/// The read-back held a reading to the line as typed, and fed `TextDevice`
/// each character as typed. A standard face draws a NO-BREAK SPACE at
/// `WinAnsiEncoding`'s 0xA0, which 9.6.5 reads by its glyph name, `space`, so
/// a reader of the page reads a word space there; read back as typed it was a
/// `CS` between two digits, `1 234` one number to UAX #9, and a drawing that
/// read right only so was taken. Each character is read back now as the face
/// the page draws it in gives it back (`paint::Segment::read_as`,
/// `paint::Fonts::read_as`): the WinAnsi code's character, the overflow
/// font's by its `/Differences` name, the stand-in's own.
///
/// With `bundled-fonts`, a Hebrew line of `1 234،`, the number's halves a
/// NO-BREAK SPACE apart, at `word-spacing: 1em` or letter-spaced and
/// justified, read `234 1،` at 05cea0b and conserves now, in either
/// direction: read back as typed, it reads `234 1،` still. Guards: the other
/// books read as at 05cea0b, and the decoding changes none of their readings.
#[test]
fn a_line_is_read_back_as_the_pages_fonts_give_its_characters_back() {
    let ws = "p { word-spacing: 0.6em }";
    let comma = "שלום 1\u{a0}234\u{60C} <b>עולם</b> מלחמה.";
    check(&[
        Case::each(
            "a dir-less Hebrew line of 1\u{a0}234\u{60C} at 1em (bundled-fonts)",
            "he",
            "p { word-spacing: 1em }",
            format!("<p>{comma}</p>"),
            Expect::Neither,
            Expect::Holds("cons logical found apart"),
        ),
        Case::each(
            "the same, right to left (bundled-fonts)",
            "he",
            "p { word-spacing: 1em }",
            format!("<p dir=\"rtl\">{comma}</p>"),
            Expect::Neither,
            Expect::Holds("cons logical found apart"),
        ),
        Case::each(
            "the same with no dir, letter-spaced and justified (bundled-fonts)",
            "he",
            "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify; margin: 0 30% }",
            format!("<p>{comma}</p>"),
            Expect::Neither,
            Expect::Holds("cons logical found apart"),
        ),
        Case::both(
            "a bold 1\u{a0}234 in a dir-less Arabic line (guard)",
            "ar",
            ws,
            "<p>كتاب مدرسة <b>1\u{a0}234</b> الطالب، جميل بيت.</p>",
            Expect::Holds("cons logical found apart"),
        ),
        Case::both(
            "the same, right to left (guard)",
            "ar",
            ws,
            "<p dir=\"rtl\">كتاب مدرسة <b>1\u{a0}234</b> الطالب، جميل بيت.</p>",
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::both(
            "a bold 1\u{a0}234 in a dir-less Hebrew line (guard)",
            "he",
            ws,
            "<p>שלום עולם <b>1\u{a0}234</b> מלחמה، ספר בית.</p>",
            Expect::Holds("cons logical found apart"),
        ),
        Case::both(
            "the same, right to left (guard)",
            "he",
            ws,
            "<p dir=\"rtl\">שלום עולם <b>1\u{a0}234</b> מלחמה، ספר בית.</p>",
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::both(
            "a SOFT HYPHEN inside a Hebrew word (as at every tree)",
            "he",
            ws,
            "<p dir=\"rtl\">שלום עו\u{ad}לם <b>?!</b> מלחמה.</p>",
            Expect::Pinned("whole found apart"),
        ),
    ]);
}

/// **A styled mark between letters the page gives back as spaces reads as
/// typed** (round 6's lane F, the read-back's decoding, in either build).
///
/// The overflow font names no letter of the supplementary planes in its
/// `/Differences` (`space`, for a character with no glyph name), so a reader
/// of the page reads an Adlam word drawn in it as spaces: the writer draws no
/// Adlam letter, in either build, at any tree (pinned above; ROADMAP CD-19,
/// "Read so at every tree"). Read back as typed, a styled `?!` between two
/// Adlam words was turned for a line of right-to-left letters the page does
/// not have, and read `!?`, as at 05cea0b in either build. Read back as the
/// overflow font gives each character back (`paint::Fonts::read_as`), the
/// line holds no right-to-left letter a reader reads, and the `?!` reads as
/// typed. The letters, spaces on the page, are no part of what is held: the
/// marks are, in the order a reader reads them.
#[test]
fn a_styled_mark_between_letters_the_page_gives_back_as_spaces_reads_as_typed() {
    let adlam = "\u{1E900}\u{1E923}\u{1E924}\u{1E922} <b>?!</b> \u{1E906}\u{1E935}\u{1E924}\u{1E922} 25\u{66A}.";
    let mut wrong = Vec::new();
    for (what, body) in [
        ("right to left", format!("<p dir=\"rtl\">{adlam}</p>")),
        ("with no dir", format!("<p>{adlam}</p>")),
    ] {
        let reading = read("ff", "p { word-spacing: 0.6em }", &body);
        let marks: String = reading
            .text
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if marks != "?!25\u{66A}." {
            wrong.push(format!("{what}: {:?}", reading.text));
        }
    }
    assert!(
        wrong.is_empty(),
        "a styled ?! between two Adlam words read otherwise:\n{}",
        wrong.join("\n")
    );
}

/// **A styled `?!` after a number whose halves a NO-BREAK SPACE holds
/// together reads as typed** (round 6's lane F; the owner declined the text
/// price of reading the page's fonts back, 17 October 2026).
///
/// A standard face draws a NO-BREAK SPACE at `WinAnsiEncoding`'s 0xA0, which
/// a reader of the page reads as a word space (`paint::Segment::read_as`). So
/// read back, `<b>1 234</b>` is two numbers to UAX #9, and a reader of a
/// right-to-left line reads them right to left, `234 1`, in every drawing
/// tried, since no drawing puts a number's halves back together: no drawing
/// read the line's letters in order, nothing was found, and the rules'
/// drawing was kept, which a reader reads with the styled `?!` after the
/// number turned, `!?`. 05cea0b read the line back as typed, the number one,
/// and drew the `?!` in its level's order, which the page reads as typed.
/// Where nothing tried reads a line right as the page's fonts give it back,
/// and they give a character of it back otherwise than typed, the line is
/// searched again read as typed, and what that finds is drawn where it reads
/// closer to the line on the page than the rules' drawing
/// (`paint::Fonts::set_readings`): the page reads `234 1 ?!`.
///
/// At `word-spacing: 0.6em`, `1em` and `12px`, the second sentence's `?!` read
/// `!?` where 05cea0b read it as typed (12 books in a default build, 7 with
/// `bundled-fonts`); under `scaleY(2)` both did, where 05cea0b turned the
/// first. Each row reads every `?!` as typed now, in either build; the rows
/// that read so at b6a622b2 too (5 with `bundled-fonts`) are guards.
///
/// With `bundled-fonts`, the `?!` before the number, `<b>?!</b> W 1 234`,
/// under `scaleY(2)` read `!?` twice, as at 05cea0b, and reads as typed now;
/// in a default build its second `?!` reads `!?`, as at 05cea0b and
/// b6a622b2, pinned.
#[test]
fn a_styled_mark_after_a_number_a_no_break_space_holds_together_reads_as_typed() {
    let scaled = "p { transform: scaleY(2); word-spacing: 0.6em }";
    let mut wrong = Vec::new();
    for (language, words) in [("he", &HE), ("ar", &AR)] {
        let text = rep(2, |i| {
            format!(
                "{} {} <b>1\u{a0}234</b> <i>?!</i> {}.",
                word(words, i),
                word(words, i + 1),
                word(words, i + 2)
            )
        });
        for (what, attr) in [("right to left", " dir=\"rtl\""), ("with no dir", "")] {
            for style in [
                "p { word-spacing: 0.6em }",
                "p { word-spacing: 1em }",
                "p { word-spacing: 12px }",
                scaled,
            ] {
                let reading = read(language, style, &format!("<p{attr}>{text}</p>"));
                if reading.text.matches("?!").count() != 2 {
                    wrong.push(format!("{language} {what} {style}: {:?}", reading.text));
                }
            }
        }
    }
    // The `?!` before the number, under `scaleY(2)`.
    let before = rep(2, |i| {
        format!(
            "{} <b>?!</b> {} 1\u{a0}234 {}.",
            word(&HE, i),
            word(&HE, i + 1),
            word(&HE, i + 2)
        )
    });
    for (what, attr) in [("right to left", " dir=\"rtl\""), ("with no dir", "")] {
        let reading = read("he", scaled, &format!("<p{attr}>{before}</p>"));
        let (typed, turned) = if cfg!(feature = "bundled-fonts") {
            (2, 0)
        } else {
            (1, 1)
        };
        if reading.text.matches("?!").count() != typed
            || reading.text.matches("!?").count() != turned
        {
            wrong.push(format!(
                "he {what} ?! before 1\u{a0}234 {scaled}: {:?}",
                reading.text
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "a styled ?! beside 1\u{a0}234 read otherwise:\n{}",
        wrong.join("\n")
    );
}

/// **What a line read back as typed finds is drawn only where it reads
/// closer to the line on the page** (round 6's lane F, the guards of the
/// search above).
///
/// A drawing that reads a line right as typed is no better than a guess on
/// the page, whose fonts give some of its characters back otherwise, and is
/// drawn only where it reads more of the line's pairs of letters in order on
/// the page than the rules' drawing (`paint::Fonts::reads_closer`). Taken
/// unchecked, `1 234٪` in a right-to-left Arabic line at
/// `word-spacing: 0.6em` read `234 1٪`, 05cea0b's text, where the rules'
/// `234 ٪1` holds the sign after the 4; a styled `?!` between Adlam words
/// with no `dir`, letter-spaced and justified, read `!?`; and with
/// `bundled-fonts` a dir-less Hebrew line of `1 234،` under `scaleX(2)` read
/// `234 1،` and lost the structure tree's order. Each reads as at b6a622b2,
/// in either build.
#[test]
fn what_a_line_read_as_typed_finds_is_drawn_only_where_it_reads_closer_on_the_page() {
    let mut wrong = Vec::new();
    let percent = rep(2, |i| {
        format!(
            "{} 1\u{a0}234\u{66a} <b>{}</b> {}.",
            word(&AR, i),
            word(&AR, i + 1),
            word(&AR, i + 2)
        )
    });
    let reading = read(
        "ar",
        "p { word-spacing: 0.6em }",
        &format!("<p dir=\"rtl\">{percent}</p>"),
    );
    if reading.text.matches("234 \u{66a}1").count() != 2 {
        wrong.push(format!("1\u{a0}234\u{66a} at 0.6em: {:?}", reading.text));
    }
    let adlam = rep(2, |i| {
        format!(
            "{} \u{1e900}\u{1e923}\u{1e924}\u{1e922} <b>?!</b> {}.",
            word(&AR, i),
            word(&AR, i + 1)
        )
    });
    let reading = read(
        "ar",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify; margin: 0 30% }",
        &format!("<p>{adlam}</p>"),
    );
    if reading.text.matches("?!").count() != 2 {
        wrong.push(format!(
            "?! between Adlam words, justified: {:?}",
            reading.text
        ));
    }
    assert!(
        wrong.is_empty(),
        "a line read as typed was drawn as it found it:\n{}",
        wrong.join("\n")
    );
    let comma = rep(2, |i| {
        format!(
            "{} 1\u{a0}234\u{60c} <b>{}</b> {}.",
            word(&HE, i),
            word(&HE, i + 1),
            word(&HE, i + 2)
        )
    });
    check(&[Case::each(
        "a dir-less Hebrew line of 1\u{a0}234\u{60c} under scaleX(2) at 0.6em",
        "he",
        "p { transform: scaleX(2); word-spacing: 0.6em }",
        format!("<p>{comma}</p>"),
        Expect::Holds("found apart"),
        Expect::Holds("logical found apart"),
    )]);
}

/// **A styled `?!` after a number a NO-BREAK SPACE holds together reads as
/// typed beside a letter the page's fonts give back otherwise** (round 6's
/// lane F, the sweep of the search read as typed, in either build).
///
/// Where nothing tried reads a line right as the page's fonts give it back,
/// the line is searched again with each glyph read as typed, and each drawing
/// is held to the line's words as typed (`paint::Fonts::set_readings`), not
/// to its words as the fonts give them back. The two differ beside an Adlam
/// letter, which the overflow font names `space` and a reader of the page
/// reads as a word space. Held to the words as the fonts give them back, a
/// drawing read as typed reads letters the words do not have, so nothing is
/// found, and the styled `?!` after `<b>1 234</b>` is drawn by the rules,
/// which a reader reads `!?`, as at b6a622b2. Held to the words as typed, it
/// reads as typed, as at 05cea0b, and under `scaleY(2)` too, where 05cea0b
/// turned one `?!` or both.
///
/// Rows: an Adlam word before the number, Adlam letters glued to the word
/// before it, an Adlam word after the `?!`, and a SOFT HYPHEN inside the word
/// before it. Each comes in Hebrew and in Arabic, right to left and with no
/// `dir`, at `word-spacing: 0.6em`, `1em` and `12px` and under `scaleY(2)`:
/// 64 books. Each reads both `?!` as typed, in either build. At b6a622b2, 52
/// of them read a `!?` in a default build and 39 with `bundled-fonts`. At
/// 05cea0b, 16 do (20 with `bundled-fonts`). Held to the words as the fonts
/// give them back, 36 do in a default build, every Adlam row but the unscaled
/// ones with the Adlam word after the `?!`, and 28 with `bundled-fonts`. The
/// SOFT HYPHEN rows read as typed either way: they hold the search read as
/// typed itself, which b6a622b2 did not make.
#[test]
fn a_styled_mark_after_a_number_reads_as_typed_beside_a_letter_the_page_gives_back_otherwise() {
    let adlam = "\u{1e900}\u{1e923}\u{1e924}\u{1e922}";
    let mut wrong = Vec::new();
    for (language, words) in [("he", &HE), ("ar", &AR)] {
        let shy = |i: usize| {
            let mut letters = word(words, i).chars();
            let first = letters.next().map(String::from).unwrap_or_default();
            format!("{first}\u{ad}{}", letters.as_str())
        };
        let shapes = [
            (
                "an Adlam word before the number",
                rep(2, |i| {
                    format!(
                        "{} {adlam} <b>1\u{a0}234</b> <i>?!</i> {}.",
                        word(words, i),
                        word(words, i + 1)
                    )
                }),
            ),
            (
                "Adlam letters glued to the word before the number",
                rep(2, |i| {
                    format!(
                        "{} {}{adlam} <b>1\u{a0}234</b> <i>?!</i> {}.",
                        word(words, i),
                        word(words, i + 1),
                        word(words, i + 2)
                    )
                }),
            ),
            (
                "an Adlam word after the ?!",
                rep(2, |i| {
                    format!(
                        "{} {} <b>1\u{a0}234</b> <i>?!</i> {adlam} {}.",
                        word(words, i),
                        word(words, i + 1),
                        word(words, i + 2)
                    )
                }),
            ),
            (
                "a SOFT HYPHEN inside the word before the number",
                rep(2, |i| {
                    format!(
                        "{} {} <b>1\u{a0}234</b> <i>?!</i> {}.",
                        word(words, i),
                        shy(i + 1),
                        word(words, i + 2)
                    )
                }),
            ),
        ];
        for (shape, text) in &shapes {
            for (what, attr) in [("right to left", " dir=\"rtl\""), ("with no dir", "")] {
                for style in [
                    "p { word-spacing: 0.6em }",
                    "p { word-spacing: 1em }",
                    "p { word-spacing: 12px }",
                    "p { transform: scaleY(2); word-spacing: 0.6em }",
                ] {
                    let reading = read(language, style, &format!("<p{attr}>{text}</p>"));
                    if reading.text.matches("?!").count() != 2 {
                        wrong.push(format!(
                            "{language} {what}, {shape}, {style}: {:?}",
                            reading.text
                        ));
                    }
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "a styled ?! after 1\u{a0}234 beside a letter the page gives back otherwise read otherwise:\n{}",
        wrong.join("\n")
    );
}

/// **Each run is written as the read-back planned it, though two runs share
/// a place** (round 6's lane F; the owner declined the time price of reading
/// lines back on the page, 17 October 2026).
///
/// The read-back plans each run of a page once for each drawing a reading
/// tries it in, and the page writes the plan its line was read in
/// (`paint::Fonts::plan_run`). A run of format characters alone — an
/// embedding's POP DIRECTIONAL FORMATTING, a LEFT-TO-RIGHT MARK, a ZERO WIDTH
/// JOINER — is set where the next run starts, and `paint::run_key`, a run's
/// order and place, is the next run's too: plans held by it gave one run the
/// other's text objects, and with `bundled-fonts` 14 books of the round-4
/// review's hunts (isolates and white space) read worse in a first form of
/// the change. Held by each run's place in the page's list of runs
/// (`paint::place_in`), each run is written as its own. Guards: each book reads as at b6a622b2, in either
/// build.
#[test]
fn each_run_is_written_as_the_read_back_planned_it_though_two_share_a_place() {
    let embed = |w: &[&'static str; 12]| {
        rep(5, |i| {
            format!(
                "{} \u{202A}a (b)\u{202C} {} \u{202B}{} !\u{202C} {}.",
                word(w, i),
                word(w, i + 1),
                word(w, i + 2),
                word(w, i + 3)
            )
        })
    };
    let joiners = rep(5, |i| {
        format!(
            "{}\u{200D} \u{200D}{} {}\u{200D}{} {}.",
            word(&HE, i),
            word(&HE, i + 1),
            word(&HE, i + 2),
            word(&HE, i + 3),
            word(&HE, i + 4)
        )
    });
    let marks = rep(5, |i| {
        format!(
            "{} 2010\u{200E} {} 15%\u{200E}. {}",
            word(&AR, i),
            word(&AR, i + 1),
            word(&AR, i + 2)
        )
    });
    check(&[
        Case::both(
            "Hebrew with LRE and RLE embeddings, no dir",
            "he",
            "",
            format!("<p>{}</p>", embed(&HE)),
            Expect::Holds("found apart"),
        ),
        Case::each(
            "the same at word-spacing: 1em",
            "he",
            "p { word-spacing: 1em; margin: 0 20% }",
            format!("<p>{}</p>", embed(&HE)),
            Expect::Holds("cons logical whole found apart"),
            Expect::Holds("found apart"),
        ),
        Case::each(
            "Arabic with LRE and RLE embeddings, no dir, at word-spacing: 1em",
            "ar",
            "p { word-spacing: 1em; margin: 0 20% }",
            format!("<p>{}</p>", embed(&AR)),
            Expect::Holds("cons logical whole found apart"),
            Expect::Holds("found apart"),
        ),
        Case::each(
            "Hebrew with ZERO WIDTH JOINERs at a word space, justified",
            "he",
            "p { text-align: justify; margin: 0 30% }",
            format!("<p>{joiners}</p>"),
            Expect::Holds("cons logical whole apart"),
            Expect::Holds("whole apart"),
        ),
        Case::each(
            "Arabic numbers each closed by a LEFT-TO-RIGHT MARK, right to left",
            "ar",
            "",
            format!("<p dir=\"rtl\">{marks}</p>"),
            Expect::Holds("found apart"),
            Expect::Holds("whole found"),
        ),
    ]);
}

/// **A reading that cuts a word or runs two together is taken for the line's
/// text, as 935b077 took it** (round 6's lane F, the sweep of V1's class),
/// pinned.
///
/// The read-back holds a reading to the line's letters, white space aside,
/// as 935b077 held it: a drawing that a reader reads with a word run into
/// the next, or cut, is taken for the line's text (`paint::Read::Letters`).
/// A styled `?!` glued to a Hebrew word at `word-spacing: 0.6em` conserves
/// and reads `שלום?! עולם.`, its word space gone, in a default build, as at
/// 935b077; a sign after European digits before a bold word at
/// `word-spacing: 1em`, which a reader runs into the word before it on the
/// rules' drawing, reads `كتاب25٪`; and so does an Arabic paragraph of styled
/// stops and brackets at `line-height: 0.6`. Holding each drawing tried to
/// the line's words reads the first and the last whole, and searching on
/// past the rules' drawing where it runs two words together reads the
/// second whole; each costs a justified paragraph of such lines some times
/// its time to open, the owner's to price (ROADMAP CD-19). Under
/// `scaleY(2)`, letter-spaced and justified, three such number sentences
/// read whole (a guard).
#[test]
fn a_reading_that_cuts_or_joins_a_word_is_taken_as_935b077_took_it() {
    let joined = Expect::Pinned("cons logical found apart");
    check(&[
        Case::each(
            "a styled ?! glued to a Hebrew word at 0.6em",
            "he",
            "p { word-spacing: 0.6em }",
            "<p dir=\"rtl\">שלום <b>?!</b> עולם.</p>",
            joined,
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::each(
            "the same under scale(2)",
            "he",
            "p { transform: scale(2); word-spacing: 0.6em }",
            "<p dir=\"rtl\">שלום <b>?!</b> עולם.</p>",
            joined,
            Expect::Holds("cons logical whole found apart"),
        ),
        Case::both("a sign after European digits before a bold word at 1em", "ar", "p { margin: 0 30%; word-spacing: 1em }", format!("<p dir=\"rtl\">{}</p>", signs_and_numbers(1)), joined),
        Case::both("two such sentences at 1em", "ar", "p { margin: 0 30%; word-spacing: 1em }", format!("<p dir=\"rtl\">{}</p>", signs_and_numbers(2)), joined),
        Case::both("the punctuation shape at line-height 0.6", "ar", "p { margin: 0 30%; line-height: 0.6; word-spacing: 0.6em }", format!("<p dir=\"rtl\">{}</p>", styled_marks(&AR)), joined),
        Case::both(
            "three number sentences under scaleY(2), letter-spaced and justified (guard)",
            "ar",
            "p { margin: 0 30%; transform: scaleY(2); letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify }",
            format!("<p dir=\"rtl\">{}</p>", signs_and_numbers(3)),
            Expect::Holds("cons logical whole found"),
        ),
    ]);
}

/// **A line whose right drawing lies past the read-back's budget is drawn by
/// the rules** (V5 of round 6's lane V; ROADMAP CD-19, Left (8)), pinned.
///
/// The read-back tries each run's drawings, then two runs' at a time, and
/// gives up after `paint::READINGS_PER_LINE` readings. Many of a run's
/// drawings write the same text objects as the rules' or as another of its
/// own, and each is read again: on a line of a few spaced runs the pair that
/// reads it right comes after the budget. A right-to-left paragraph after a
/// generated `§ ` at `word-spacing: 0.6em` or `1em` is not read whole, and,
/// with `bundled-fonts`, a `<bdi>` holding a Latin sentence with Hebrew in it,
/// in a Hebrew paragraph at `word-spacing: 1em`, does not conserve; in a
/// default build `TextDevice`'s mend of a cut at a word space's stretch reads
/// that one whole. Reading
/// each drawing once reaches those drawings, and costs an Arabic paragraph
/// at `line-height: 0.4`, whose rows a reader runs into the next ones, its
/// structure tree's order, which it holds at 56dfd93 and 935b077: the
/// read-back reads a line alone, and a drawing it finds right for the line
/// is read with the next on the page. Whether that is a price worth paying
/// is the owner's; both are pinned at what they read.
#[test]
fn a_line_whose_right_drawing_lies_past_the_budget_is_drawn_by_the_rules() {
    let he = "<p dir=\"rtl\" class=\"b\">שלום עולם <b>מלחמה</b> ספר، בית (גדול) אור.</p>";
    let ar = "<p dir=\"rtl\" class=\"b\">كتاب مدرسة <b>الطالب</b> جميل، بيت (كبير) نور.</p>";
    let generated_06 = "p { word-spacing: 0.6em } .b::before { content: \"\u{a7} \" }";
    let generated_1 = "p { word-spacing: 1em } .b::before { content: \"\u{a7} \" }";
    let tight = "p { margin: 0 30%; line-height: 0.4; letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify }";
    check(&[
        Case::both("a Hebrew paragraph after a generated \u{a7} at 0.6em (V5)", "he", generated_06, he, Expect::Pinned("found apart")),
        Case::both("the same at 1em (V5)", "he", generated_1, he, Expect::Pinned("found apart")),
        Case::both("an Arabic paragraph after a generated \u{a7} at 0.6em (V5)", "ar", generated_06, ar, Expect::Pinned("found apart")),
        Case::both("the same at 1em (V5)", "ar", generated_1, ar, Expect::Pinned("found apart")),
        Case::each(
            "a bdi of Latin with Hebrew in it in a Hebrew paragraph at 1em (V5)",
            "he",
            "p { word-spacing: 1em; margin: 0 20% }",
            "<p dir=\"rtl\">יום <bdi>The word שלום means עולם in <i>מלחמה</i>. The word שלום means עולם in <i>מלחמה</i>.</bdi> בראשית.</p>",
            Expect::Pinned("cons logical whole found apart"),
            Expect::Pinned("found apart"),
        ),
        Case::both("a justified Arabic paragraph at line-height 0.4, no dir (what reading each drawing once costs)", "ar", tight, format!("<p>{}</p>", styled_marks(&AR)), Expect::Pinned("logical found apart")),
        Case::both("the same, right to left (what reading each drawing once costs)", "ar", tight, format!("<p dir=\"rtl\">{}</p>", styled_marks(&AR)), Expect::Pinned("logical found apart")),
    ]);
}

/// The inline-block shape of the round-6 sweep: a word, an inline block
/// squeezed by `scaleX(0.5)` holding two words and a styled `?!`, a word,
/// `25٪` and a word, three times.
fn squeezed_blocks(words: &[&'static str; 12]) -> String {
    rep(3, |i| {
        format!(
            "{} <span style=\"display: inline-block; transform: scaleX(0.5)\">{} <b>?!</b> {}</span> {} 25\u{66A} {}.",
            word(words, i),
            word(words, i + 1),
            word(words, i + 2),
            word(words, i + 3),
            word(words, i + 4)
        )
    })
}

/// **A line nothing reads right on the page under a transform is searched
/// again in its runs' own space** (round 6's lane F, the sweep of V1's
/// class), guards and a case.
///
/// An inline block squeezed by `scaleX(0.5)` draws glyphs over the ones
/// beside it at every tree, with no spacing as with any (the sweep's `ib`
/// shape), and on such a line no drawing the read-back tries reads the
/// letters in order on the page. Searched on the page alone, the line was
/// drawn by the rules, and a Hebrew paragraph with no `dir` at
/// `word-spacing: 0.6em` or `1em` lost its structure tree's order, which it
/// holds at 935b077 in a default build: there the read-back, in the runs'
/// space, had found a drawing that reads right in it, and that drawing
/// reads the logical order right on the page as well
/// (`paint::Fonts::set_readings`). What that search finds is drawn only
/// where it reads closer to the line on the page than the rules' drawing
/// (`paint::Fonts::reads_closer`): turned a quarter (`rotate(90deg)`) at
/// `0.6em`, the paragraph held its structure tree's order at 9297d2d and
/// 56dfd93 and lost it at 935b077, which drew what its search found in the
/// runs' space; that drawing reads no closer on the page, and the rules'
/// drawing, kept, holds the order again; and so turned three quarters
/// (`rotate(270deg)`), which lost it at 05cea0b too.
/// With `bundled-fonts` the paragraph at
/// `0.6em` is pinned at what it reads: a word of it is not found by search,
/// as at 56dfd93 and 935b077, where 9297d2d found every one (15 books of the
/// sweep, with no transform and under fourteen; ROADMAP CD-19, Left (7)).
#[test]
fn a_line_nothing_reads_right_on_the_page_is_searched_in_its_runs_space() {
    let at_06 = "p { margin: 0 30%; transform: none; word-spacing: 0.6em } .w { transform: none }";
    let at_1 = "p { margin: 0 30%; transform: none; word-spacing: 1em } .w { transform: none }";
    let body = format!("<div class=\"w\"><p>{}</p></div>", squeezed_blocks(&HE));
    check(&[
        Case::each(
            "a dir-less Hebrew paragraph with inline blocks under scaleX(0.5) at 0.6em (guard; pinned with bundled-fonts)",
            "he",
            at_06,
            body.clone(),
            Expect::Holds("logical"),
            Expect::Pinned("logical"),
        ),
        Case::each(
            "the same at 1em (guard)",
            "he",
            at_1,
            body.clone(),
            Expect::Holds("logical found"),
            Expect::Holds("found"),
        ),
        Case::each(
            "the same at 0.6em turned a quarter (f6 sweep, as at 9297d2d and 56dfd93)",
            "he",
            "p { margin: 0 30%; transform: rotate(90deg); word-spacing: 0.6em } .w { transform: none }",
            body.clone(),
            Expect::Holds("logical"),
            Expect::Holds("found"),
        ),
        Case::each(
            "the same turned three quarters",
            "he",
            "p { margin: 0 30%; transform: rotate(270deg); word-spacing: 0.6em } .w { transform: none }",
            body,
            Expect::Holds("logical"),
            Expect::Holds("found"),
        ),
    ]);
}

/// The review's `narrowmark` shape: twice, a word, an inline block squeezed
/// by `scaleX(sx)` that holds a word and a styled `?!`, two words and a
/// stop.
fn marked_blocks(words: &[&'static str; 12], sx: &str) -> String {
    rep(2, |i| {
        format!(
            "{} <span style=\"display: inline-block; transform: scaleX({sx})\">{} <b>?!</b></span> {} {}.",
            word(words, i),
            word(words, i + 1),
            word(words, i + 2),
            word(words, i + 3)
        )
    })
}

/// **A drawing found in the runs' own space is drawn where it reads more of
/// the line's runs in their own order on the page than the rules' drawing**
/// (round 6's review), cases.
///
/// A styled `?!` in an inline block squeezed by `scaleX(0.6)`, `0.7` or
/// `0.8`, in a Hebrew or Arabic paragraph with no `dir` at
/// `word-spacing: 0.5em` or `0.55em`, under no transform or `scaleX(2)`: no
/// drawing the read-back tries reads the line's letters in order on the
/// page, and in the runs' own space its search finds one
/// (`paint::Fonts::set_readings`). On the page that drawing reads the `?!`
/// in its order with a letter of the next word between them, where the
/// rules' drawing turns it about, and the two keep as many of the text's
/// pairs of letters side by side, or the rules' more
/// (`paint::Words::pairs_in`). Held to the pairs alone, the rules' drawing
/// was kept, and paragraphs whose structure tree's order 935b077 held, by
/// drawing what its search found, lost it. What the search finds is drawn
/// now where it reads more of the line's runs with their own characters in
/// order, as a reader of the structure tree reads each element's text
/// (`paint::Fonts::reads_closer`), and where as many, more pairs. Each of
/// the 24 paragraphs holds its order, in both builds.
#[test]
fn a_styled_mark_in_a_squeezed_block_reads_in_its_order_where_the_runs_space_drawing_holds_it() {
    let mut cases = Vec::new();
    for (lang, words) in [("he", &HE), ("ar", &AR)] {
        for sx in ["0.6", "0.7", "0.8"] {
            for style in [
                "p { margin: 0 30%; transform: none; word-spacing: 0.5em; } .w { transform: none }",
                "p { margin: 0 30%; transform: none; word-spacing: 0.55em; } .w { transform: none }",
                "p { margin: 0 30%; transform: scaleX(2); word-spacing: 0.5em; } .w { transform: none }",
                "p { margin: 0 30%; transform: scaleX(2); word-spacing: 0.55em; } .w { transform: none }",
            ] {
                cases.push(Case::both(
                    "a dir-less paragraph's styled ?! in an inline block squeezed by scaleX(sx)",
                    lang,
                    style,
                    format!("<div class=\"w\"><p>{}</p></div>", marked_blocks(words, sx)),
                    Expect::Holds("logical"),
                ));
            }
        }
    }
    check(&cases);
}

/// The review's `both` shape: twice, a word, an inline block widened by
/// `scaleX(2)` that holds a word and a styled `?!`, a word, an inline block
/// squeezed by `scaleX(sx)` that holds a word, a word and a stop.
fn widened_and_squeezed(words: &[&'static str; 12], sx: &str) -> String {
    rep(2, |i| {
        format!(
            "{} <span style=\"display: inline-block; transform: scaleX(2)\">{} <b>?!</b></span> {} <span style=\"display: inline-block; transform: scaleX({sx})\">{}</span> {}.",
            word(words, i),
            word(words, i + 1),
            word(words, i + 2),
            word(words, i + 3),
            word(words, i + 4)
        )
    })
}

/// **A styled `?!` in an inline block widened by `scaleX(2)` reads in its
/// order, and draws more glyphs over others than at 935b077** (round 6's
/// review, F2; ROADMAP CD-19, Left (10)), pins.
///
/// A dir-less Arabic paragraph that holds, twice, an inline block widened
/// by `scaleX(2)` with a word and a styled `?!` in it, and one squeezed by
/// `scaleX(0.6)`, `0.7` or `0.8`, at `word-spacing: 0.5em` or `0.55em`. At
/// 935b077 the review's hunts read it out of logical order, drawing 11, 6
/// and 2 glyphs over others, and none turned a quarter. Here it reads in
/// logical order and draws 13, 8 and 4, and 1 turned a quarter: the CASE
/// lines print each count. The owner has not said whether that is a price
/// to pay for the order (ROADMAP CD-19, Left (10), first in the order of
/// work), so each paragraph is pinned at exactly what it reads today, the
/// same in both builds: no glyph apart in any, found as well where the
/// block is squeezed least, and conserved turned a quarter.
#[test]
fn a_styled_mark_in_a_widened_block_reads_in_order_and_is_pinned_at_what_it_draws_over() {
    const STYLES: [&str; 6] = [
        "p { margin: 0 30%; transform: none; word-spacing: 0.5em; } .w { transform: none }",
        "p { margin: 0 30%; transform: none; word-spacing: 0.55em; } .w { transform: none }",
        "p { margin: 0 30%; transform: rotate(90deg); word-spacing: 0.5em; } .w { transform: none }",
        "p { margin: 0 30%; transform: scaleX(1.4); word-spacing: 0.5em; } .w { transform: none }",
        "p { margin: 0 30%; transform: scaleX(2); word-spacing: 0.5em; } .w { transform: none }",
        "p { margin: 0 30%; transform: scaleX(2); word-spacing: 0.55em; } .w { transform: none }",
    ];
    let mut cases = Vec::new();
    for (sx, pins) in [
        (
            "0.6",
            [
                "logical",
                "logical",
                "cons logical found",
                "logical",
                "logical",
                "logical",
            ],
        ),
        ("0.7", ["logical"; 6]),
        (
            "0.8",
            [
                "logical",
                "logical found",
                "cons logical found",
                "logical found",
                "logical found",
                "logical found",
            ],
        ),
    ] {
        for (style, pin) in STYLES.into_iter().zip(pins) {
            cases.push(Case::both(
                "a dir-less Arabic paragraph's styled ?! in an inline block widened by scaleX(2) (pinned)",
                "ar",
                style,
                format!("<div class=\"w\"><p>{}</p></div>", widened_and_squeezed(&AR, sx)),
                Expect::Pinned(pin),
            ));
        }
    }
    check(&cases);
}

/// **A line a reader cuts on the page under a transform is searched in its
/// runs' own space too** (round 6's lane F, the sweep of V1's class),
/// guards.
///
/// A reading of the rules' drawing that has the line's letters in order,
/// a word cut where a reader parts the spacing or two run together, is
/// searched no further, as 935b077 searched it no further. But 935b077
/// read a line under a transform in its runs' own space: an Arabic
/// paragraph of numbers, letter-spaced and justified, under a scale of one
/// axis, whose rules' drawing a reader parts inside `25٪` on the page, read
/// wrong in that space, and the drawing 935b077's search found there reads
/// it whole on the page. Kept as the rules drew it, each lost its text; searched in
/// its runs' space as well, and drawn where what that finds reads its
/// words right on the page, each reads whole (`paint::Fonts::set_readings`).
#[test]
fn a_line_a_reader_cuts_on_the_page_under_a_transform_is_searched_in_its_runs_space_too() {
    let body = format!(
        "<div class=\"w\"><p dir=\"rtl\">{}</p></div>",
        signs_and_numbers(3)
    );
    let mut cases = Vec::new();
    for (name, style) in [
        (
            "numbers, letter-spaced and justified, under scaleX(2) (guard)",
            "p { margin: 0 30%; transform: scaleX(2); letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify } .w { transform: none }",
        ),
        (
            "the same under scaleY(0.5) (guard)",
            "p { margin: 0 30%; transform: scaleY(0.5); letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify } .w { transform: none }",
        ),
        (
            "the same under matrix(1.6, 0, 0, 0.8, 0, 0) (guard)",
            "p { margin: 0 30%; transform: matrix(1.6, 0, 0, 0.8, 0, 0); letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify } .w { transform: none }",
        ),
        (
            "the same with scaleX(2) on the block around it (guard)",
            "p { margin: 0 30%; transform: none; letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify } .w { transform: scaleX(2) }",
        ),
    ] {
        cases.push(Case::both(name, "ar", style, body.clone(), Expect::Holds("cons logical whole found")));
    }
    check(&cases);
}

/// **A drawing that sets more glyphs over others than the rules' is not
/// taken** (round 6's lane F, the sweep of V1's class), a guard.
///
/// The same inline blocks in an Arabic paragraph turned by half a turn at
/// `word-spacing: 1em` draw one glyph over another at 9297d2d, 56dfd93 and
/// 935b077. Read back under the page's matrix, a drawing that read the
/// line's letters right in the runs' space drew two, and was taken: a
/// glyph more over another under word spacing. The read-back counts the
/// glyphs each drawing sets over another
/// (`paint::Overprint`) and takes none that sets more than the rules'.
#[test]
fn a_drawing_that_sets_more_glyphs_over_others_than_the_rules_is_not_taken() {
    let body = format!("<div class=\"w\"><p>{}</p></div>", squeezed_blocks(&AR));
    let mut more = Vec::new();
    for style in [
        "p { margin: 0 30%; transform: rotate(180deg); word-spacing: 1em } .w { transform: none }",
        "p { margin: 0 30%; transform: scaleX(2) rotate(180deg); word-spacing: 1em } .w { transform: none }",
    ] {
        let reading = read("ar", style, &body);
        if reading.overprint > 1 {
            more.push(format!("{style}: {} glyphs over another", reading.overprint));
        }
    }
    assert!(
        more.is_empty(),
        "more glyphs drawn over another than at 935b077 (1):\n{}",
        more.join("\n")
    );
}

/// **Two cells are read back as a reader parts them on the page** (the edges
/// of V1's class).
///
/// The read-back reads together the runs a reader reads as one line
/// (`paint::page_lines`), and a gap between two runs parts them where it is
/// wide in a reader's ems on the page (`paint::along_the_line`). Under
/// `scaleX(0.5)` a Latin cell `abc (x).` and a right-to-left one `10px` apart
/// at `word-spacing: 1em` are one line to a reader and two in their runs'
/// ems: read back as two, the right-to-left cell was drawn whole and the
/// reader read the row from the right, `שלום עולם.abc (x)`, as 935b077 drew
/// such a row when its cells touched. Read back as a reader reads it, the
/// row holds two paragraphs' text, and the right-to-left cell is left in
/// pieces (`paint::sharing_a_line`): its two words are not read together
/// before the Latin cell.
///
/// Under `scaleX(2)` or `scaleX(3)` two right-to-left cells `6px` or `8px`
/// apart are parted on the page and not in their runs, and each is read back
/// alone; the first cell's words read on lines of their own, as at 9297d2d,
/// 56dfd93 and 935b077: a reading of the rules' drawing that holds the
/// letters in order is kept (`paint::Read::Letters`), and searching on for
/// one that reads the cell on one line, which finds it, cost a letter-spaced
/// line fifty times its time to open and is the owner's to price (pinned).
#[test]
fn two_cells_are_read_back_as_a_reader_parts_them_on_the_page() {
    let mut wrong = Vec::new();
    for (language, cells, whole) in [
        ("he", "<td>abc (x).</td><td>שלום עולם</td>", "שלום עולם"),
        ("ar", "<td>abc (x).</td><td>كتاب مدرسة</td>", "كتاب مدرسة"),
    ] {
        let style =
            "table { border-spacing: 10px; transform: scaleX(0.5) } td { padding: 0; word-spacing: 1em }";
        let reading = read(language, style, &format!("<table><tr>{cells}</tr></table>"));
        if reading.text.contains(whole) {
            wrong.push(format!("{language}, {style}: {:?}", reading.text));
        }
    }
    for (language, cells, pinned) in [
        (
            "he",
            "<td><b>מלחמה</b> ספר\u{60C}</td><td>בית <i>(</i>גדול<i>)</i>.</td>",
            if cfg!(feature = "bundled-fonts") {
                "מלחמה\n ספר\n\u{60C}\nבית\n(גדול)\n.\n"
            } else {
                "מלחמה\n ספר\n\u{60C}\n.בית(גדול)\n"
            },
        ),
        (
            "ar",
            "<td><b>الطالب</b> جميل\u{60C}</td><td>بيت <i>(</i>كبير<i>)</i>.</td>",
            "الطالب\n جميل\n\u{60C}\n.بيت(كبير)\n",
        ),
    ] {
        let style =
            "table { border-spacing: 6px; transform: scaleX(2) } td { padding: 0; word-spacing: 1em }";
        let reading = read(language, style, &format!("<table><tr>{cells}</tr></table>"));
        if reading.text != pinned {
            wrong.push(format!(
                "{language}, {style} (pinned {pinned:?}): {:?}",
                reading.text
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "cells read otherwise:\n{}",
        wrong.join("\n")
    );
}
