//! Verification of review r5/code1 (scratch, never committed).
#![allow(missing_docs, clippy::all, unused)]

mod epub_support;

use epub_support::book::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::{Document, TextOptions};
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

const AR12: [&str; 12] = ["كتاب", "مدرسة", "الطالب", "جميل", "بيت", "كبير", "نور", "يوم", "عوامل", "وقد", "أصبحنا", "نعرف"];

fn rep(n: usize, f: impl Fn(usize) -> String) -> String {
    (0..n).map(|k| f(k * 3)).collect::<Vec<_>>().join(" ")
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Same measure as epub_fallback.rs's `overprinted` at 68734c5.
fn overprinted(doc: &Document) -> Vec<String> {
    let page = doc.page(0).expect("a page");
    let drawn = page.text_with(&TextOptions { content_order: true });
    let mut boxes: Vec<(i64, f64, f64, f64, String)> = drawn
        .lines()
        .iter()
        .flat_map(|line| line.chars.iter())
        .filter(|c| {
            !c.text.trim().is_empty()
                && c.text.chars().next().map_or(true, |first| bidi_class(first) != BidiClass::NSM)
        })
        .map(|c| {
            let (x0, _, x1, _) = c.quad.bounds();
            let baseline = (c.origin.1 * 100.0).round() as i64;
            (baseline, x0, x1, c.size, c.text.clone())
        })
        .collect();
    boxes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    boxes
        .windows(2)
        .filter_map(|pair| {
            let (before, after) = (&pair[0], &pair[1]);
            if before.0 == after.0 && after.1 < before.2 - 0.01 * after.3 {
                Some(format!("{:?} {:.3}-{:.3} under {:?} at {:.3}", before.4, before.1, before.2, after.4, after.1))
            } else {
                None
            }
        })
        .collect()
}

struct Seen {
    text: String,
    cons: bool,
    logical: bool,
}

fn see(tag: &str, lang: &str, style: &str, body: &str) -> Seen {
    let bytes = styled_book(lang, style, body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let mut text = String::new();
    for p in 0..doc.page_count() {
        text.push_str(&doc.page(p).expect("page").text().plain_text());
    }
    let cons = conservation(&bytes, &doc).holds();
    let logical = conservation_in_logical_order(&bytes, &doc).holds();
    let over = overprinted(&doc);
    println!("VC1\t{tag}\tcons={cons}\tlogical={logical}\tover={}\t{:?}", over.len(), text);
    Seen { text, cons, logical }
}

fn whole(seen: &Seen, plain: &str) -> bool {
    norm(&seen.text).contains(&norm(plain))
}

// ---- CR1: CD-19's 0 Tw loses books ----------------------------------------

/// hunt zz_rr3m_hunt_spacing `ar/styled/rtl`: conserved and read whole at
/// 9297d2d and 56dfd93, both builds.
#[test]
fn cr1_hunt_styled_rtl_letter_spaced_justified() {
    let s = |i: usize| AR12[i % 12];
    let text = rep(6, |i| format!("{} <b>{}</b> <i>?!</i> {} <b>(</b>{}<b>)</b> {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)));
    let plain = rep(6, |i| format!("{} {} ?! {} ({}) {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)));
    let style = "p { letter-spacing: 0.45em; text-align: justify; margin: 0 32% }";
    let seen = see("cr1-hunt-styled", "ar", style, &format!("<p dir=\"rtl\">{text}</p>"));
    assert!(seen.cons && seen.logical && whole(&seen, &plain), "{:?}", seen.text);
}

/// price `price/he/qe/b2/brb/rtl`, default build: conserved at 9297d2d and 56dfd93.
#[test]
fn cr1_price_he_qe_braces_letter_spaced() {
    let body = "<p dir=\"rtl\">אור יום<br/>?!<b>{</b>בראשית<b>}</b> ברא אלהים</p>";
    let style = "p { letter-spacing: 0.5em; text-align: justify; margin: 0 25% }";
    let seen = see("cr1-price-he-qe", "he", style, body);
    assert!(seen.cons && seen.logical, "{:?}", seen.text);
}

/// hunt gaps `ar/p1/rtl` (`!!` glued) at word-spacing 0.6em: conserved at 56dfd93.
#[test]
fn cr1_hunt_gaps_bangs_word_spaced() {
    let s = |i: usize| AR12[i % 12];
    let p = "!!";
    let pi = 1;
    let text = rep(3, |i| format!("{} <i>{p}</i> {} <b>{p}</b>{} {}.", s(i + pi), s(i + pi + 1), s(i + pi + 2), s(i + pi + 3)));
    let seen = see("cr1-gaps-p1", "ar", "p { word-spacing: 0.6em; margin: 0 20% }", &format!("<p dir=\"rtl\">{text}</p>"));
    assert!(seen.cons && seen.logical, "{:?}", seen.text);
}

/// hunt gaps `ar/p8/rtl` (`;` glued) letter-spaced: read whole at 56dfd93.
#[test]
fn cr1_hunt_gaps_semicolon_reads_whole() {
    let s = |i: usize| AR12[i % 12];
    let p = ";";
    let pi = 8;
    let text = rep(3, |i| format!("{} <i>{p}</i> {} <b>{p}</b>{} {}.", s(i + pi), s(i + pi + 1), s(i + pi + 2), s(i + pi + 3)));
    let plain = rep(3, |i| format!("{} {p} {} {p}{} {}.", s(i + pi), s(i + pi + 1), s(i + pi + 2), s(i + pi + 3)));
    let style = "p { letter-spacing: 0.25em; text-align: justify; margin: 0 25% }";
    let seen = see("cr1-gaps-p8", "ar", style, &format!("<p dir=\"rtl\">{text}</p>"));
    assert!(whole(&seen, &plain), "{:?}", seen.text);
}

/// kit zz_rr3m_tie `mf/greek-he` at word-spacing 0.5001em, default build: conserved at 56dfd93.
#[test]
fn cr1_kit_greek_he_word_spaced() {
    let body = "<p>x \u{3B1}\u{3BB}\u{3C6}\u{3B1} \u{5E9}\u{5DC}\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD} y.</p>";
    let seen = see("cr1-greek-he", "en", "p { word-spacing: 0.5001em }", body);
    assert!(seen.cons && seen.logical, "{:?}", seen.text);
}

/// kit zz_k11rv3_own s4 `ar/rtl/long-neutral-mid` pointed: conserved at 56dfd93.
#[test]
fn cr1_kit_own_long_neutral_mid() {
    let body = "<p dir=\"rtl\">عوامل عدّة وقد <i>?! -- ?!</i> أصبحنا نعرف الآن الموادّ.</p>";
    let style = "p, li, td { letter-spacing: 0.55em; text-align: justify; margin: 0 20% }";
    let seen = see("cr1-own-lnm", "ar", style, body);
    assert!(seen.cons && seen.logical, "{:?}", seen.text);
}

// ---- CR2: V1's price outside the ellipsis --------------------------------

/// price/ar/qe/b0/br/rtl at margin 30%: conserved and read whole at 56dfd93 (not at 9297d2d).
#[test]
fn cr2_price_ar_qe_after_br() {
    let seen = see("cr2-ar-qe", "ar", "p { margin: 0 30% }", "<p dir=\"rtl\">نور يوم<br/>?!(عوامل) وقد أصبحنا</p>");
    assert!(seen.cons && seen.logical && whole(&seen, "?!(عوامل) وقد أصبحنا"), "{:?}", seen.text);
}

/// price/ar/ee/b0/br/rtl at margin 30%.
#[test]
fn cr2_price_ar_ee_after_br() {
    let seen = see("cr2-ar-ee", "ar", "p { margin: 0 30% }", "<p dir=\"rtl\">يوم عوامل<br/>!!(وقد) أصبحنا نعرف</p>");
    assert!(seen.cons && seen.logical && whole(&seen, "!!(وقد) أصبحنا نعرف"), "{:?}", seen.text);
}

/// price/he/quot-stop/b0/br/rtl at margin 30%.
#[test]
fn cr2_price_he_quot_stop_after_br() {
    let seen = see("cr2-he-quot-stop", "he", "p { margin: 0 30% }", "<p dir=\"rtl\">אלהים הארץ<br/>\".(שלום) עולם מלחמה</p>");
    assert!(seen.cons && seen.logical && whole(&seen, "\".(שלום) עולם מלחמה"), "{:?}", seen.text);
}

/// price/he/apos-stop/b0/brb/rtl at margin 30%: structure-tree order held at 9297d2d and 56dfd93.
#[test]
fn cr2_price_he_apos_stop_styled_logical() {
    let seen = see("cr2-he-apos-brb", "he", "p { margin: 0 30% }", "<p dir=\"rtl\">עולם מלחמה<br/>'.<b>(</b>ספר<b>)</b> בית גדול</p>");
    assert!(seen.logical, "{:?}", seen.text);
}

/// soft/he/quot-stop/b0/rtl at margin 42%: no <br/>, a soft wrap opens the line with `".`.
#[test]
fn cr2_soft_he_quot_stop() {
    let body = "<p dir=\"rtl\">אלהים הארץ שלום \".(עולם) מלחמה ספר. עולם מלחמה ספר \".(בית) גדול אור.</p>";
    let seen = see("cr2-soft-he-quot-stop", "he", "p { margin: 0 42% }", body);
    assert!(seen.cons && seen.logical, "{:?}", seen.text);
}

// ---- CR3: the table test's doc --------------------------------------------

#[test]
fn cr3_table_diag() {
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
        for (pad, rows) in [("pad", rows.clone()), ("pad0", rows.replace("<td>", "<td style=\"padding: 0\">"))] {
            let table = format!("<table dir=\"rtl\" style=\"border-spacing: 0; word-spacing: 1em\">{rows}</table>");
            let bytes = styled_book("ar", style, &table);
            let doc = Document::open(bytes.clone()).expect("opens");
            let page = doc.page(0).expect("page");
            let text = page.text().plain_text();
            let cells: Vec<&str> = text.lines().filter(|l| l.contains("أصبحن")).collect();
            println!(
                "VC1T\t{style:?}\t{pad}\tcons={}\tlogical={}\tasbahna={}\tnarifu={}\t{:?}",
                conservation(&bytes, &doc).holds(),
                conservation_in_logical_order(&bytes, &doc).holds(),
                page.text().search("أصبحنا").len(),
                page.text().search("نعرف").len(),
                cells
            );
        }
    }
}

// ---- CR4: negative word spacing ------------------------------------------

#[test]
fn cr4_negative_word_spacing_overprint() {
    let mut total = 0;
    for ws in ["-2px", "-4px", "-0.2em"] {
        for (lang, body) in [("en", "<p>x <b>\u{3B1}\u{3BB}\u{3C6}\u{3B1}</b> y.</p>"), ("ar", "<p dir=\"rtl\">\u{645}\u{62F}\u{629} \u{645}\u{639}\u{644}\u{645}.</p>")] {
            let style = format!("p {{ word-spacing: {ws} }}");
            let bytes = styled_book(lang, &style, body);
            let doc = Document::open(bytes.clone()).expect("opens");
            let over = overprinted(&doc);
            println!("VC1N\t{ws}\t{lang}\tover={}\t{:?}\t{:?}", over.len(), over, doc.page(0).unwrap().text().plain_text());
            total += over.len();
        }
    }
    assert_eq!(total, 0, "negative word spacing draws a glyph over another");
}
