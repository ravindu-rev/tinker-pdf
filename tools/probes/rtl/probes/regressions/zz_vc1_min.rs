//! verify-confirm1 (scratch, never committed): one styled_book test per
//! book the reviewer named, asserting what the finding says the book did at
//! the baseline. Each test prints `VC1\t<id>\tcons=\tlogical=\ttext=\tsearch=\t<text>`.
#![allow(missing_docs, clippy::all, unused)]

mod epub_support;

use epub_support::book::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

// The rr3m probes' word lists (hunt, laneR, fv_extra).
const HE: [&str; 12] = ["שלום", "עולם", "מלחמה", "ספר", "בית", "גדול", "אור", "יום", "בראשית", "ברא", "אלהים", "הארץ"];
const HEP: [&str; 12] = ["שָׁלוֹם", "עוֹלָם", "מִלְחָמָה", "סֵפֶר", "בַּיִת", "גָּדוֹל", "אוֹר", "יוֹם", "בְּרֵאשִׁית", "בָּרָא", "אֱלֹהִים", "הָאָרֶץ"];
const AR: [&str; 12] = ["كتاب", "مدرسة", "الطالب", "جميل", "بيت", "كبير", "نور", "يوم", "عوامل", "وقد", "أصبحنا", "نعرف"];
// The own and code probes' Arabic.
const AR8: [&str; 8] = ["عوامل", "عدّة", "وقد", "أصبحنا", "نعرف", "الآن", "الموادّ", "تكبح"];

const LS05: &str = "p { letter-spacing: 0.5em; text-align: justify; margin: 0 25% }";
const S4: &str = "p, li, td { letter-spacing: 0.55em; text-align: justify; margin: 0 20% }";

fn rep(n: usize, f: impl Fn(usize) -> String) -> String {
    (0..n).map(|k| f(k * 3)).collect::<Vec<_>>().join(" ")
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

const BLOCKS: [&str; 7] = ["p", "br", "div", "td", "tr", "table", "li"];

/// The visible text, one string per block or forced break.
fn segments(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c == '<' {
            let mut tag = String::new();
            for d in chars.by_ref() {
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
                let n = norm(&cur);
                if !n.is_empty() {
                    out.push(n);
                }
                cur.clear();
            }
        } else {
            cur.push(c);
        }
    }
    let n = norm(&cur);
    if !n.is_empty() {
        out.push(n);
    }
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

struct Read {
    cons: bool,
    logical: bool,
    text: bool,
    search: bool,
}

fn read(id: &str, lang: &str, style: &str, body: &str) -> Read {
    let bytes = styled_book(lang, style, body);
    let doc = Document::open(bytes.clone()).expect("the book opens");
    let mut text = String::new();
    let mut texts = Vec::new();
    for at in 0..doc.page_count() {
        let t = doc.page(at).expect("a page").text();
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&t.plain_text());
        texts.push(t);
    }
    let segs = segments(body);
    let full = norm(&text);
    let text_ok = segs.iter().all(|s| full.contains(s.as_str()));
    let toks = tokens(&segs);
    let search = toks.iter().all(|t| texts.iter().any(|x| !x.search(t).is_empty()));
    let c = conservation(&bytes, &doc);
    let l = conservation_in_logical_order(&bytes, &doc);
    println!(
        "VC1\t{id}\tcons={}\tlogical={}\ttext={}\tsearch={}\t{}",
        c.holds() as u8,
        l.holds() as u8,
        text_ok as u8,
        search as u8,
        text.replace('\n', " / ")
    );
    Read { cons: c.holds(), logical: l.holds(), text: text_ok, search }
}

fn s12(w: [&'static str; 12]) -> impl Fn(usize) -> &'static str {
    move |i| w[i % 12]
}

fn s8(i: usize) -> &'static str {
    AR8[i % 8]
}

// ---- F1: hunt spacing ar/styled/rtl, 0.45em justified ----------------------

fn hunt_styled() -> String {
    let s = s12(AR);
    rep(6, |i| format!("{} <b>{}</b> <i>?!</i> {} <b>(</b>{}<b>)</b> {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))
}

#[test]
fn f1_hunt_spacing_ar_styled_rtl() {
    let body = format!("<p dir=\"rtl\">{}</p>", hunt_styled());
    let r = read("F1 spacing ar/styled/rtl", "ar", "p { letter-spacing: 0.45em; text-align: justify; margin: 0 32% }", &body);
    assert!(r.cons && r.logical && r.text && r.search);
}

#[test]
fn f1_hunt_spacing_ar_styled_none_logical() {
    let body = format!("<p>{}</p>", hunt_styled());
    let r = read("F1b spacing ar/styled/none", "ar", "p { letter-spacing: 0.45em; text-align: justify; margin: 0 32% }", &body);
    assert!(r.logical);
}

// ---- F2: price/he/qe/b2/brb/rtl ---------------------------------------------

#[test]
fn f2_price_he_qe_braces_rtl() {
    let body = "<p dir=\"rtl\">אור יום<br/>?!<b>{</b>בראשית<b>}</b> ברא אלהים</p>";
    let r = read("F2 price/he/qe/b2/brb/rtl", "he", LS05, body);
    assert!(r.cons && r.logical);
}

// ---- F3: fv_extra c/hep/m6/b{0,1}/two/none ----------------------------------

fn fv_two(o: &str, c: &str) -> String {
    let s = s12(HEP);
    let mi = 6;
    let m = "،";
    let two = rep(2, |i| format!("{} {} <b>{o}</b>{}<b>{c}</b> {o}{}{c} {}{m}", s(i + mi), s(i + mi + 1), s(i + mi + 2), s(i + mi + 3), s(i + mi + 4)));
    format!("<p>{two}</p>")
}

#[test]
fn f3_fv_extra_hep_m6_b0_two_none() {
    let r = read("F3 c/hep/m6/b0/two/none", "he", LS05, &fv_two("(", ")"));
    assert!(r.cons && r.logical);
}

#[test]
fn f3_fv_extra_hep_m6_b1_two_none() {
    let r = read("F3 c/hep/m6/b1/two/none", "he", LS05, &fv_two("[", "]"));
    assert!(r.cons && r.logical);
}

// ---- F4: the reviewer's own pointed Hebrew book -------------------------------

#[test]
fn f4_own_he_glued_stop_italic_qe() {
    let body = "<p>שָׁלוֹם <b>.</b>עוֹלָם <i>?!</i> מִלְחָמָה <b>(</b>סֵפֶר<b>)</b> בַּיִת; עוֹלָם <b>.</b>מִלְחָמָה <i>?!</i> סֵפֶר <b>(</b>בַּיִת<b>)</b> גָּדוֹל; מִלְחָמָה <b>.</b>סֵפֶר <i>?!</i> בַּיִת <b>(</b>גָּדוֹל<b>)</b> אוֹר; סֵפֶר <b>.</b>בַּיִת <i>?!</i> גָּדוֹל <b>(</b>אוֹר<b>)</b> יוֹם; בַּיִת <b>.</b>גָּדוֹל <i>?!</i> אוֹר <b>(</b>יוֹם<b>)</b> שָׁלוֹם;</p>";
    let r = read("F4 own he", "he", "body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }", body);
    assert!(r.cons && r.logical);
}

// ---- F5: books conserving at 56dfd93 --------------------------------------------

fn own_long_neutral_mid() -> String {
    format!("{} {} {} <i>?! -- ?!</i> {} {} {} {}.", s8(0), s8(1), s8(2), s8(3), s8(4), s8(5), s8(6))
}

#[test]
fn f5_own_s4_ar_rtl_long_neutral_mid() {
    let r = read("F5 own s4 ar/rtl/long-neutral-mid", "ar", S4, &format!("<p dir=\"rtl\">{}</p>", own_long_neutral_mid()));
    assert!(r.cons && r.logical);
}

#[test]
fn f5_own_s4_ar_none_long_neutral_mid() {
    let r = read("F5 own s4 ar/none/long-neutral-mid", "ar", S4, &format!("<p>{}</p>", own_long_neutral_mid()));
    assert!(r.cons && r.logical);
}

#[test]
fn f5_own_s4_ar_css_long_neutral_mid() {
    let r = read(
        "F5 own s4 ar/css/long-neutral-mid",
        "ar",
        S4,
        &format!("<div style=\"direction: rtl\"><p>{}</p></div>", own_long_neutral_mid()),
    );
    assert!(r.cons && r.logical);
}

fn own_in_x3(inside: &str) -> String {
    let t: String = (0..3)
        .map(|i| format!("{} {} {} {}, {} {} {} {}. ", s8(i), s8(i + 1), s8(i + 2), inside, s8(i + 3), s8(i + 4), s8(i + 5), s8(i + 6)))
        .collect();
    format!("<p>{t}</p>")
}

#[test]
fn f5_own_s4_ar_none_in_sq_x3() {
    let r = read("F5 own s4 ar/none/in-sq-x3", "ar", S4, &own_in_x3("[HDAC]"));
    assert!(r.cons && r.logical);
}

#[test]
fn f5_own_s4_ar_none_in_gu_x3() {
    let r = read("F5 own s4 ar/none/in-gu-x3", "ar", S4, &own_in_x3("«HDAC»"));
    assert!(r.cons && r.logical);
}

#[test]
fn f5_tie_greek_he() {
    let r = read("F5 tie mf/greek-he", "en", "p { word-spacing: 0.5001em }", "<p>x αλφα שלום עולם y.</p>");
    assert!(r.cons && r.logical);
}

#[test]
fn f5_code_s4_ar_none_greek_x3() {
    let t: String = (0..3)
        .map(|i| format!("{} {} (αλφα βήτα), {} {} {} {}. ", s8(i), s8(i + 1), s8(i + 3), s8(i + 4), s8(i + 5), s8(i + 6)))
        .collect();
    let r = read("F5 code s4 ar/none/greek-x3", "ar", S4, &format!("<p>{t}</p>"));
    assert!(r.cons && r.logical);
}

fn gaps(pi: usize, p: &str) -> String {
    let s = s12(AR);
    let text = rep(3, |i| format!("{} <i>{p}</i> {} <b>{p}</b>{} {}.", s(i + pi), s(i + pi + 1), s(i + pi + 2), s(i + pi + 3)));
    format!("<p dir=\"rtl\">{text}</p>")
}

#[test]
fn f5_hunt_gaps_ar_p1_rtl_ws06() {
    let r = read("F5 gaps ws0.6 ar/p1/rtl", "ar", "p { word-spacing: 0.6em; margin: 0 20% }", &gaps(1, "!!"));
    assert!(r.cons && r.logical);
}

#[test]
fn f5_hunt_gaps_ar_p8_rtl_ls025_read_whole() {
    let r = read("F5 gaps ls0.25 ar/p8/rtl", "ar", "p { letter-spacing: 0.25em; text-align: justify; margin: 0 25% }", &gaps(8, ";"));
    assert!(r.text);
}

#[test]
fn f5_hunt_punct_ar_mid_ws_none() {
    let s = s12(AR);
    let text = rep(6, |i| {
        let x = s(i);
        let c: Vec<char> = x.chars().collect();
        let k = c.len() / 2;
        format!(
            "{} <span style=\"word-spacing: 1em\">{} {}</span>{} ({}).",
            s(i + 1),
            s(i + 3),
            c[..k].iter().collect::<String>(),
            c[k..].iter().collect::<String>(),
            s(i + 2)
        )
    });
    let r = read("F5 punct '' ar/mid-ws/none", "ar", "", &format!("<p>{text}</p>"));
    assert!(r.cons && r.logical);
}

#[test]
fn f5_price_soft_ar_acomma_rtl() {
    let s = s12(AR);
    let mi = 18;
    let (m, o, c) = ("\u{60C}", "(", ")");
    let soft = rep(2, |i| format!("{} {} {} {m}{o}{}{c} {} {}.", s(i + mi), s(i + mi + 1), s(i + mi + 2), s(i + mi + 3), s(i + mi + 4), s(i + mi + 5)));
    let r = read("F5 soft/ar/acomma/b0/rtl", "ar", LS05, &format!("<p dir=\"rtl\">{soft}</p>"));
    assert!(r.cons && r.logical);
}

#[test]
fn f5_price_end_ar_ee_b_paren_three() {
    let s = s12(AR);
    let ei = 2;
    let e = "!!";
    let shape = |i: usize| format!("{} {} {} <b>(</b>{}<b>)</b> {}{e}", s(i + ei), s(i + ei + 1), s(i + ei + 2), s(i + ei + 3), s(i + ei + 4));
    let r = read("F5 end/ar/ee/b-paren/three", "ar", LS05, &format!("<p>{}</p>", rep(3, |k| shape(k))));
    assert!(r.cons && r.logical);
}

// ---- F6: the rule's price outside the ellipsis --------------------------------

#[test]
fn f6_price_ar_ee_br_rtl() {
    let r = read("F6 price/ar/ee/b0/br/rtl", "ar", "p { margin: 0 30% }", "<p dir=\"rtl\">يوم عوامل<br/>!!(وقد) أصبحنا نعرف</p>");
    assert!(r.cons && r.logical);
}

#[test]
fn f6_price_he_quot_stop_br_rtl() {
    let r = read("F6 price/he/quot-stop/b0/br/rtl", "he", "p { margin: 0 30% }", "<p dir=\"rtl\">אלהים הארץ<br/>\".(שלום) עולם מלחמה</p>");
    assert!(r.cons && r.logical);
}

/// Control: the owner-accepted ellipsis price, for contrast (expected to
/// fail at the tip and at 9297d2d, conserve at 56dfd93).
#[test]
fn f6_control_price_ar_ell_br_rtl() {
    let r = read("F6c price/ar/ell/b0/br/rtl", "ar", "p { margin: 0 30% }", "<p dir=\"rtl\">كتاب مدرسة<br/>\u{2026}(الطالب) جميل بيت</p>");
    assert!(r.cons && r.logical);
}
