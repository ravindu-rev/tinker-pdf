//! Verification probe for hunt2's findings (scratch, never committed).
#![allow(missing_docs, clippy::all, unused)]

mod epub_support;

use epub_support::book::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order, logical_text};
use tinker_pdf::{Document, TextOptions};
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

fn esc(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\n' { " | ".to_owned() } else { c.to_string() })
        .collect()
}

fn nsm(c: char) -> bool {
    bidi_class(c) == BidiClass::NSM
}

fn format_char(c: char) -> bool {
    matches!(c, '\u{AD}' | '\u{61C}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}')
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !format_char(*c))
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

const BLOCKS: [&str; 26] = [
    "p", "li", "td", "th", "tr", "figcaption", "div", "br", "h1", "h2", "h3", "h4", "h5", "h6", "dt", "dd",
    "caption", "pre", "blockquote", "figure", "table", "ul", "ol", "dl", "section", "tbody",
];

fn segments(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = body.chars().peekable();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        let n = norm(cur);
        if !n.is_empty() {
            out.push(n);
        }
        cur.clear();
    };
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            while let Some(d) = chars.next() {
                if d == '>' {
                    break;
                }
                tag.push(d);
            }
            let name = tag
                .trim_start_matches('/')
                .split(|c: char| c.is_whitespace() || c == '/')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            if BLOCKS.contains(&name.as_str()) {
                flush(&mut cur, &mut out);
            }
        } else {
            cur.push(c);
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn tokens(segs: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in segs {
        for t in s.split(|c: char| !(c.is_alphanumeric() || nsm(c))) {
            if t.chars().filter(|c| c.is_alphanumeric() && !nsm(*c)).count() >= 2 && !out.iter().any(|o| o == t) {
                out.push(t.to_owned());
            }
        }
    }
    out
}

pub struct V {
    pub cons: bool,
    pub logical: bool,
    pub whole: bool,
    pub search: bool,
    pub missing: Vec<String>,
    pub plain: String,
    pub logical_text: String,
    pub overlap: usize,
    pub vov: usize,
}

pub fn verdict(lang: &str, style: &str, body: &str) -> V {
    let bytes = styled_book(lang, style, body);
    let doc = Document::open(bytes.clone()).expect("opens");
    let mut text = String::new();
    let mut texts = Vec::new();
    let mut overlaps = 0usize;
    let mut vov = 0usize;
    for at in 0..doc.page_count() {
        let page = doc.page(at).expect("a page");
        let t = page.text();
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&t.plain_text());
        texts.push(t);
        let drawn = page.text_with(&TextOptions { content_order: true });
        let mut boxes: Vec<(i64, f64, f64, f64)> = Vec::new();
        for line in drawn.lines() {
            let mut prev: Option<&tinker_pdf::TextChar> = None;
            for c in &line.chars {
                let (x0, _, x1, _) = c.quad.bounds();
                if c.text.chars().next().is_some_and(nsm) {
                    continue;
                }
                if let Some(p) = prev {
                    let (_, _, p1, _) = p.quad.bounds();
                    if x0 < p1 - 0.02 * c.size && x0 > p1 - 3.0 * c.size {
                        overlaps += 1;
                    }
                }
                prev = Some(c);
                if !c.text.trim().is_empty() {
                    boxes.push(((c.origin.1 * 10.0).round() as i64, x0, x1, c.size));
                }
            }
        }
        boxes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        for pair in boxes.windows(2) {
            let (p, n) = (&pair[0], &pair[1]);
            if p.0 == n.0 && n.1 < p.2 - 0.02 * n.3 {
                vov += 1;
            }
        }
    }
    let segs = segments(body);
    let full = norm(&text);
    let whole = segs.iter().all(|s| full.contains(s.as_str()));
    let toks = tokens(&segs);
    let found_in = |t: &str| texts.iter().any(|x| !x.search(t).is_empty());
    let missing: Vec<String> = toks.iter().filter(|t| !found_in(t)).cloned().collect();
    let c = conservation(&bytes, &doc);
    let l = conservation_in_logical_order(&bytes, &doc);
    V {
        cons: c.holds(),
        logical: l.holds(),
        whole,
        search: missing.is_empty(),
        missing,
        plain: esc(&text),
        logical_text: logical_text(&doc).join("|"),
        overlap: overlaps,
        vov,
    }
}

fn show(tag: &str, v: &V) {
    println!(
        "{tag}: cons={} logical={} whole={} search={} ov={}/{} missing={:?} plain={}",
        v.cons, v.logical, v.whole, v.search, v.overlap, v.vov, v.missing, v.plain
    );
}

// H2-1: a styled `?!` glued to a right-to-left word under word spacing.
#[test]
fn h2_1_styled_pair_glued_to_an_arabic_word_conserves() {
    let v = verdict("ar", "p { word-spacing: 0.6em }", r#"<p dir="rtl"><b>?!</b>ابا اما.</p>"#);
    show("h2_1", &v);
    assert!(v.cons && v.logical);
}

#[test]
fn h2_1_syriac_figcaption_conserves() {
    let v = verdict(
        "syr",
        "p, li, figcaption { word-spacing: 0.6em }",
        r#"<div dir="rtl"><figure><figcaption><i>?!</i>ܐܠܗܐ ܐܪܥܐ ܫܡܝܐ ܡܠܬܐ ܪܒܐ.</figcaption></figure></div>"#,
    );
    show("h2_1_syr", &v);
    assert!(v.cons && v.logical);
}

// H2-2: a dir-less gloss after a styled overflow-script word.
#[test]
fn h2_2_gloss_after_a_bold_greek_word_conserves_default() {
    let v = verdict("he", "p { word-spacing: 12px }", r#"<p>αλφα <b>βήτα</b> (ספר) בית</p>"#);
    show("h2_2_gr", &v);
    assert!(v.cons && v.logical);
}

#[test]
fn h2_2_gloss_after_a_bold_armenian_word_conserves() {
    let v = verdict("he", "p { word-spacing: 12px }", r#"<p>գիրք <b>տուն</b> (ספר) בית</p>"#);
    show("h2_2_hy", &v);
    assert!(v.cons && v.logical);
}

// H2-3: an empty inline-block cutting an overflow-script word.
#[test]
fn h2_3_greek_word_cut_by_an_inline_block_is_found_default() {
    let v = verdict(
        "el",
        "p { word-spacing: 0.6em }",
        r#"<p>αλφα<span style="display: inline-block; width: 1em"></span>βήτα</p>"#,
    );
    show("h2_3_gr", &v);
    assert!(v.whole && v.search);
}

#[test]
fn h2_3_armenian_word_cut_by_an_inline_block_is_found() {
    let v = verdict(
        "hy",
        "p { word-spacing: 0.6em }",
        r#"<p>գիրք<span style="display: inline-block; width: 1em"></span>տուն մեծ լույս.</p>"#,
    );
    show("h2_3_hy", &v);
    assert!(v.whole && v.search);
}

// H2-4: ornate parentheses round Arabic-Indic digits before a bold word.
#[test]
fn h2_4_ornate_parentheses_before_a_bold_word_conserve() {
    let v = verdict("ar", "p { word-spacing: 0.75em }", r#"<p dir="rtl">﴾٢٥﴿ <b>مدرسة</b> الطالب.</p>"#);
    show("h2_4", &v);
    assert!(v.cons && v.logical);
}

// H2-5: a shekel sign before a bold word in a justified, letter-spaced list item.
#[test]
fn h2_5_shekel_before_a_bold_word_in_a_justified_list_item_conserves() {
    let v = verdict(
        "ar",
        "p, li, td { text-align: justify; letter-spacing: 0.1em; word-spacing: 0.25em; margin: 0 30% }",
        r#"<div dir="rtl"><ol><li>كتاب عوامل 25₪ <b>مدرسة</b> الطالب.</li></ol></div>"#,
    );
    show("h2_5", &v);
    assert!(v.cons && v.logical);
}

// H2-6: a letter-spaced dir-less Arabic line with Cyrillic words (bundled-fonts).
#[test]
fn h2_6_letter_spaced_arabic_line_with_cyrillic_conserves_bundled() {
    let v = verdict("ar", "p { letter-spacing: 0.5em }", r#"<p>كتاب книга дом свет день, مدرسة.</p>"#);
    show("h2_6", &v);
    assert!(v.cons && v.whole);
}

// H2-7: since 56dfd93.
#[test]
fn h2_7a_latin_gloss_conserves() {
    let v = verdict("he", "p { word-spacing: 12px }", r#"<p>abc <b>def</b> (ספר) בית</p>"#);
    show("h2_7a", &v);
    assert!(v.cons && v.whole);
}

#[test]
fn h2_7a_unstyled_latin_gloss_conserves() {
    let v = verdict("ar", "p { word-spacing: 0.5em }", r#"<p>abc def (كتاب) بيت</p>"#);
    show("h2_7a2", &v);
    assert!(v.cons && v.whole);
}

#[test]
fn h2_7b_latin_cell_before_a_gloss_cell_conserves() {
    let v = verdict(
        "he",
        "td { word-spacing: 12px }",
        r#"<table><tr><td>abc —</td><td>(ספר) בית</td></tr></table>"#,
    );
    show("h2_7b", &v);
    assert!(v.cons && v.whole);
}

#[test]
fn h2_7c_styled_pair_glued_to_a_word_with_its_letter_once_conserves() {
    let v = verdict("ar", "p { word-spacing: 0.6em }", r#"<p dir="rtl"><b>?!</b>ابو اما.</p>"#);
    show("h2_7c", &v);
    assert!(v.cons && v.logical);
}

#[test]
fn h2_7d_arabic_question_mark_after_a_bracket_pair_conserves() {
    let v = verdict("he", "p { word-spacing: 0.6em }", r#"<p>(abc ברא) אלהים؟</p>"#);
    show("h2_7d", &v);
    assert!(v.cons && v.whole);
}

#[test]
fn h2_7e_arabic_comma_before_bold_italic_conserves_default() {
    let v = verdict(
        "he",
        "p, li, td { word-spacing: 0.75em }",
        r#"<p dir="rtl">שלום בראשית 25، <b><i>עולם</i></b> מלחמה, ספר 25،.</p>"#,
    );
    show("h2_7e", &v);
    assert!(v.cons && v.logical);
}

#[test]
fn h2_7e_percent_in_a_table_cell_conserves() {
    let v = verdict(
        "ar",
        "p, li, td { word-spacing: 16px; letter-spacing: 1px }",
        r#"<div dir="rtl"><table><tr><td>كتاب 25٪ <b>مدرسة</b></td><td>25٪ الطالب</td></tr></table></div>"#,
    );
    show("h2_7e_td", &v);
    assert!(v.cons && v.logical);
}
