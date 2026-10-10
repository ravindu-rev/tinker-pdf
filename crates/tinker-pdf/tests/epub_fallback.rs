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

/// **A mark rides on its letter under `word-spacing` when its face's code 32
/// is a letter of its word** — and the known limit that code 32 is, ROADMAP
/// CD-19, pinned so that ending it is noticed.
///
/// `paint::OVERFLOW_FIRST` is 32, so the first character a face meets past
/// `WinAnsiEncoding` is drawn at code 32, and 9.3.3 applies `Tw` to every
/// single-byte code 32: in a slice with no mark, every glyph after it in its
/// string is drawn a word spacing from where layout measured it. Unpointed,
/// the word space moves — `مدة معلم.` at `0.5em` extracts `مدةم علم.`,
/// present since the overflow font. Pointed, a mark placed from layout's pen
/// (6d79fa4) left a letter so moved: in a bold run whose alpha is its face's
/// code 32, `αλφα\u{301}` beside a Hebrew word read `αλφαy\u{301}` at a word
/// spacing of `-2px`, where cd407d5, drawing the mark in its letter's string,
/// read it whole (round 3 of the marks fix). In a slice that holds a mark
/// the glyph after the code 32 is a piece of its own now, drawn where layout
/// put it, so nothing in it is moved and every mark is on its letter
/// (`a_standard_14_pointed_arabic_letter_is_not_drawn_over_the_next_under_word_spacing`
/// for what the move did there). Writing the overflow font at `0 Tw` instead, which
/// ends the limit, was measured on that round's probe: 25 more books
/// conserve under word spacing and 8 fewer, 6 of them with no mark — they
/// conserved only because the misplacement closed a word gap past half an
/// em, which otherwise cuts a right-to-left line into pieces ruling 14 reads
/// in the order they are drawn.
#[test]
fn a_standard_14_overflow_letter_at_code_32_takes_word_spacing() {
    let unpointed = "\u{645}\u{62F}\u{629} \u{645}\u{639}\u{644}\u{645}.";
    let bytes = styled_book(
        "ar",
        "p { word-spacing: 0.5em }",
        &format!("<p dir=\"rtl\">{unpointed}</p>"),
    );
    let doc = Document::open(bytes).expect("the book opens");
    assert_eq!(
        doc.page(0).expect("a page").text().plain_text(),
        "\u{645}\u{62F}\u{629}\u{645} \u{639}\u{644}\u{645}.\n",
        "the overflow font's code 32 no longer takes word spacing: \
         drop ROADMAP CD-19 and assert the line as written"
    );

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
/// `WinAnsiEncoding` the face meets — takes `Tw` (9.3.3; ROADMAP CD-19), so
/// the glyph after it in its string is drawn a word spacing right of where
/// layout put it. At cd407d5 the rest of the string moved with it and the
/// shift ended in the word space after it, which is whitespace. A mark ends
/// its string now and the glyph after the marked letter is a piece of its
/// own, drawn where layout put it, so the moved LAM was drawn on the AIN
/// after it: `مُدَّةَ مُعلِّم.` at `word-spacing: 4px` drew both at `48pt`,
/// and a justified paragraph of the two words ten to fourteen such pairs a
/// page. In a slice holding a mark the glyph after the overflow font's code
/// 32 is a piece of its own too, drawn where layout put it.
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
/// Past half an em every **run** boundary cuts a line, marks or none — a
/// reader resumes a line across an `ET` only half an em on, and at half an
/// em exactly the last place of a float decides (`<i>Hebrew</i>.` is cut
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
/// none (`a_standard_14_line_at_exactly_half_an_em_is_cut_where_a_last_place_says`).
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

/// **At a spacing of exactly half an em, a run boundary is cut or not by a
/// floating-point last place, marks or none — class A of the marks fix's
/// round 4, a known limit pinned so that ending it is noticed.**
///
/// `TextDevice` resumes a line across an `ET` only where the next glyph
/// starts within half an em of where the last one's box ended, and at a
/// `letter-spacing` of half an em every run boundary puts it half an em on:
/// the box's end is the reader's sum (the `Td`, plus the glyph's width) and
/// the next glyph's start is the painter's (its own `Td`), and two sums of
/// one value differ in their last place. In this paragraph, sans-serif at
/// `0.5em` (`6pt`), a word space is drawn at `99.31199999999995` and ends,
/// `3.336` wide, at `102.64799999999995`, and the next word is written at
/// `108.64799999999997`: `6.000000000000014` on, past half an em by the last
/// place of the two sums, and cut.
/// Each line is cut so before its last word, and ruling 14 reads the cut-off
/// word in the order the pieces are drawn, so the paragraph does not conserve
/// — pointed, and with no mark at all (`بتك` for KATABA: the same three
/// glyph widths, so the same lines and the same sums); at `0.4999em` and at
/// `0.5001em` both conserve, every boundary decided by a ten-thousandth of
/// an em. A pointed word is as wide as the same word unpointed
/// (`css-text-3` §10.2), so it is drawn at the same places as the
/// mark-free twin. At cd407d5 the pointed words were wider, the last places
/// fell elsewhere and this paragraph conserved: on the round-4 probe, 12 of
/// a default build's 15 books that conserve, read whole or are found by
/// search at cd407d5 and not now, and all 94 with `bundled-fonts` (85 of
/// them the band just past half an em,
/// `a_standard_14_run_ending_on_a_mark_is_cut_past_half_an_em_as_its_letter_is`),
/// are at a spacing within a thousandth of an em of half an em.
#[test]
fn a_standard_14_line_at_exactly_half_an_em_is_cut_where_a_last_place_says() {
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
    for (spacing, conserves) in [("0.4999em", true), ("0.5em", false), ("0.5001em", true)] {
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
        assert_eq!(
            (pointed.0, plain.0),
            (conserves, conserves),
            "at `{spacing}`: if half an em now conserves, drop class A from epub.md"
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
/// its own, read in drawn order as `שלם כתב` fourteen times is; none with
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
/// drawn in the order it was typed, and reads back reversed — the known
/// limit `epub.md`'s `direction` row names, pinned so that ending it is
/// noticed.**
///
/// The `.,` between two italic Hebrew words is a run of its own, and N1
/// resolves both marks to the paragraph's level, 1, so L2 draws them `,.`.
/// `coded_order` reorders only a slice that holds a right-to-left character,
/// and this one holds none, so it is drawn as typed, `.` left of `,`, and
/// the line, which ruling 14 reads right to left, comes back `חו,.וח`.
///
/// Reordering every slice at an odd level instead drew this line right and
/// every line of nothing but neutrals at a right-to-left level backwards
/// (the two tests above), and was taken back. Which of the two a slice is on
/// — whether its **line** holds a right-to-left character — is the line's to
/// say, and `draw_coded` is handed the run and the slice. When this fails
/// because the line reads `חו.,וח`, the limit is gone: drop it from
/// `epub.md` and make this the test that the line reads as written.
#[test]
fn a_standard_14_punctuation_run_between_right_to_left_words_is_drawn_as_typed() {
    let body = format!("<p dir=\"rtl\"><i>{HET_VAV}</i>.,<i>{VAV_HET}</i></p>");
    let bytes = styled_book("he", "", &body);
    let doc = Document::open(bytes.clone()).expect("the book opens");

    let (comma, stop) = (drawn_at(&doc, ","), drawn_at(&doc, "."));
    assert!(
        stop < comma,
        "the punctuation is no longer drawn as typed: , at {comma}, . at {stop}"
    );

    let text = doc.page(0).expect("a page").text().plain_text();
    assert!(
        text.contains(&format!("{HET_VAV},.{VAV_HET}")),
        "the known limit no longer reads the punctuation reversed: {text:?}"
    );
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
