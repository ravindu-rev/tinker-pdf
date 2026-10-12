#![allow(missing_docs)]
//! Verifier's witness for the "glyphs read alike either way round" condition
//! of `paint::opens_with_its_word_space`: prints each book's verdicts, then
//! asserts the witness book conserves.
mod epub_support;
use epub_support::book::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

#[test]
fn zz_vc_pal_survey() {
    let books = [
        ("p { word-spacing: 0.3em; letter-spacing: 0.1em }", "<p dir=\"rtl\">عوامل<i> ?!</i> عدّة وقد أصبحنا</p>"),
        ("p { word-spacing: 0.3em; letter-spacing: 0.1em }", "<p>عوامل<i> ?!</i> عدّة وقد أصبحنا</p>"),
        ("p { word-spacing: 1em }", "<p dir=\"rtl\">عوامل<i> ?!</i> عدّة وقد أصبحنا</p>"),
        ("p { word-spacing: 1em }", "<p dir=\"rtl\">عوامل<b> :-)</b> عدّة</p>"),
        ("p { word-spacing: 1em }", "<p dir=\"rtl\">عوامل<i> #1</i> عدّة</p>"),
        ("p { word-spacing: 0.5em; font-family: monospace }", "<p dir=\"rtl\">שלום<i> ?!</i> עולם מלחמה</p>"),
    ];
    for (style, body) in books {
        let lang = if body.contains('ש') { "he" } else { "ar" };
        let bytes = styled_book(lang, style, body);
        let doc = Document::open(bytes.clone()).expect("opens");
        let plain = doc.page(0).expect("page").text().plain_text();
        let c = conservation(&bytes, &doc).holds();
        let l = conservation_in_logical_order(&bytes, &doc).holds();
        println!("VC\t{style}\t{body}\tcontent={c}\tlogical={l}\t{:?}", plain);
    }
}

/// A word-spaced slice of neutrals whose glyphs do not read alike either way
/// round, ` ?!` here, is drawn in its level's order and reads `?!`; drawn as
/// typed with its word space first it reads `!?` beside the word before it.
#[test]
fn zz_vc_pal_witness() {
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
