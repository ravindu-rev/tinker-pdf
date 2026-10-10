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
/// under `letter-spacing` of either sign.**
///
/// The two limits `epub.md` named until October 2026, each measured on
/// synthetic books (review of 6d08c6b): a default build's lone point, drawn
/// after its letter at a tie, read `a מֶלֶךְ,` as `a מלֶךְ,ֶ` at a
/// `font-size` of `12.5px` and `14px`; and `letter-spacing`, which is added
/// after a mark as after any character (as layout measures it), moved a
/// point drawn where its letter starts off the letter — `שָׁלוֹם` at `0.5px`
/// read `שׁלָוֹם` and `מֶלֶךְ` at `-0.5px` `מלֶךְֶ` with `bundled-fonts`. A
/// mark drawn inside its letter's box at a fixed inset from the letter's
/// end is read with it whatever the size, and whatever the spacing puts
/// between the letter and the glyph after it.
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
    ];
    // Every half pixel from 9 to 24.5, the sweep the limit was measured on.
    let sizes = (18..=49).map(|half| format!("p {{ font-size: {}px }}", f64::from(half) / 2.0));
    let spacings = ["0.5px", "2px", "-0.5px", "-1px"]
        .into_iter()
        .map(|spacing| format!("p {{ letter-spacing: {spacing} }}"));
    for style in sizes.chain(spacings) {
        for (language, body, expected) in &books {
            let bytes = styled_book(language, &style, body);
            let doc = Document::open(bytes.clone()).expect("the book opens");
            let text = doc.page(0).expect("a page").text().plain_text();
            assert!(
                text.contains(expected.as_str()),
                "under `{style}`, {expected:?} does not read as written: {text:?}"
            );
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
