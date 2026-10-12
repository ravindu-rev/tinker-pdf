//! Re-review probe (scratch, never committed): marks under spacing, both orders.

mod epub_support;

use epub_support::book::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::{Document, TextOptions};
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

const KATABA: &str = "\u{643}\u{64E}\u{62A}\u{64E}\u{628}\u{64E}";
const AMALIYYAN: &str = "\u{639}\u{645}\u{644}\u{64A}\u{651}\u{64B}\u{627}\u{60C} \
    \u{62A}\u{62A}\u{623}\u{644}\u{651}\u{641} \u{643}\u{644}\u{651} \
    \u{627}\u{644}\u{645}\u{648}\u{627}\u{62F} \u{645}\u{646} \
    \u{630}\u{631}\u{651}\u{627}\u{62A}.";
const MUDDA_MUALLIM: &str =
    "\u{645}\u{64F}\u{62F}\u{651}\u{64E}\u{629} \u{645}\u{64F}\u{639}\u{644}\u{651}\u{650}\u{645}.";
const ALLAH3: &str = "\u{671}\u{644}\u{644}\u{651}\u{64E}\u{670}\u{647}\u{650}";
const SHALOM: &str = "\u{5E9}\u{5B8}\u{5C1}\u{5DC}\u{5D5}\u{5B9}\u{5DD}";
const MELEKH: &str = "\u{5DE}\u{5B6}\u{5DC}\u{5B6}\u{5DA}\u{5B0}";
const HASHABBAT: &str = "\u{5D4}\u{5B7}\u{5E9}\u{5BC}\u{5C1}\u{5B7}\u{5D1}\u{5BC}\u{5B8}\u{5EA}";
const BERESHIT: &str = "\u{5D1}\u{5BC}\u{5B0}\u{5E8}\u{5B5}\u{5D0}\u{5E9}\u{5C1}\u{5B4}\u{5D9}\u{5EA}";
const HET_VAV: &str = "\u{5D7}\u{5D5}";
const CANTILLATED: &str = "\u{5E9}\u{5BC}\u{5C1}\u{5B8}\u{591}\u{5DC}\u{5B5}\u{5DD}";
const LATIN: &str = "cafe\u{301} na\u{308}ive Nguye\u{302}\u{303}n q\u{307}\u{323} o\u{302}\u{301}";
const GRCY: &str = "\u{3B1}\u{301}\u{3BB}\u{3C6}\u{3B1} \u{430}\u{301}\u{431}";
const VIET: &str = "Nguye\u{302}\u{303}n Tha\u{300}nh ca\u{301}c ba\u{323}n";

type Case = (String, &'static str, String, Option<String>);

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let mut add = |name: &str, lang: &'static str, body: String, expected: Option<String>| {
        out.push((name.to_owned(), lang, body, expected));
    };
    // Polytonic Greek, decomposed: breathing, accent, iota subscript.
    let greek = "\u{3B5}\u{313}\u{3BD} \u{3B1}\u{313}\u{3C1}\u{3C7}\u{3B7}\u{342}\u{345} \u{3B7}\u{313}\u{342}\u{3BD} \u{3BF}\u{314} \u{3BB}\u{3BF}\u{301}\u{3B3}\u{3BF}\u{3C2}";
    add("poly/alone", "el", format!("<p>{greek}.</p>"), Some(format!("{greek}.")));
    add("poly/bold", "el", format!("<p>x <b>{greek}</b> y.</p>"), Some(format!("x {greek} y.")));
    add("poly/he", "el", format!("<p>{HET_VAV} {greek}.</p>"), Some(format!("{HET_VAV} {greek}.")));
    add("poly/rtl", "he", format!("<p dir=\"rtl\">{HET_VAV} {greek} {HET_VAV}.</p>"), Some(format!("{HET_VAV} {greek} {HET_VAV}.")));
    let long: String = (0..12).map(|_| format!("{greek} ")).collect();
    add("poly/long", "el", format!("<p>{long}</p>"), None);
    let long_he: String = (0..10).map(|_| format!("{greek} {HET_VAV} ")).collect();
    add("poly/long-he", "el", format!("<p>{long_he}</p>"), None);
    // Greek words with no mark before a marked one in the same string.
    let grk = "\u{3BB}\u{3BF}\u{3B3}\u{3BF}\u{3C3}\u{3BB}\u{3BF}\u{3B3}\u{3BF}\u{3C3}\u{3B1}\u{301}";
    add("grk/run", "el", format!("<p>x <b>{grk}</b>y z.</p>"), Some(format!("x {grk}y z.")));
    add("grk/run-he", "el", format!("<p>{HET_VAV} x <b>{grk}</b>y z.</p>"), Some(format!("{HET_VAV} x {grk}y z.")));
    // Cyrillic and IPA.
    let cyr = "\u{438}\u{302}\u{43C}\u{44F} \u{43E}\u{301}\u{43A}\u{43D}\u{43E}";
    add("cyr/alone", "ru", format!("<p>{cyr} {HET_VAV}.</p>"), Some(format!("{cyr} {HET_VAV}.")));
    let ipa = "\u{14B}\u{30A}\u{259}\u{303}\u{254}\u{301}\u{25B}\u{300}";
    add("ipa/alone", "en", format!("<p>[{ipa}] {HET_VAV}.</p>"), Some(format!("[{ipa}] {HET_VAV}.")));
    add("ipa/sans", "en", format!("<p style=\"font-family: sans-serif\">[{ipa}] {HET_VAV}.</p>"), Some(format!("[{ipa}] {HET_VAV}.")));
    // Vietnamese sentence, stacked marks, every face.
    let vi = "Tie\u{302}\u{301}ng Vie\u{323}\u{302}t co\u{301} nhie\u{302}\u{300}u da\u{302}\u{301}u thanh";
    for fam in ["serif", "sans-serif", "monospace"] {
        add(&format!("vi/{fam}"), "vi", format!("<p style=\"font-family: {fam}\">{vi}.</p>"), Some(format!("{vi}.")));
        add(&format!("vi/{fam}-he"), "vi", format!("<p style=\"font-family: {fam}\">{HET_VAV} {vi}.</p>"), Some(format!("{HET_VAV} {vi}.")));
        add(&format!("vi/{fam}-bold"), "vi", format!("<p style=\"font-family: {fam}\">x <b>Vie\u{323}\u{302}</b>t y.</p>"), Some("x Vie\u{323}\u{302}t y.".to_owned()));
    }
    // The sweep's own Hebrew line.
    add("sweep/melekh", "en", format!("<p>a {MELEKH}, <i>Hebrew is a language</i>.</p>"), Some(format!("a {MELEKH}, Hebrew is a language.")));
    add("sweep/shalom", "en", format!("<p>{SHALOM}, <i>Hebrew</i>.</p>"), Some(format!("{SHALOM}, Hebrew.")));
    add("sweep/shalom-plain", "en", "<p>\u{5E9}\u{5DC}\u{5D5}\u{5DD}, <i>Hebrew</i>.</p>".to_owned(), Some("\u{5E9}\u{5DC}\u{5D5}\u{5DD}, Hebrew.".to_owned()));
    // CD-19's unpointed line.
    add("cd19/unpointed", "ar", "<p dir=\"rtl\">\u{645}\u{62F}\u{629} \u{645}\u{639}\u{644}\u{645}.</p>".to_owned(), Some("\u{645}\u{62F}\u{629} \u{645}\u{639}\u{644}\u{645}.".to_owned()));
    let _ = (KATABA, AMALIYYAN, MUDDA_MUALLIM, ALLAH3, HASHABBAT, BERESHIT, CANTILLATED, LATIN, GRCY, VIET);
    out
}

fn styles() -> Vec<String> {
    let mut out = vec![String::new()];
    for size in ["9px", "12.5px", "14px", "21.5px", "31px"] {
        out.push(format!("p {{ font-size: {size} }}"));
    }
    for ls in [
        "0.5px", "1.5px", "1.7px", "2px", "3px", "3.5px", "4px", "4.5px", "5px", "0.1em", "0.12em", "0.3em",
        "7.84px", "0.49em", "7.92px", "0.495em", "0.4999em", "8px", "0.5em", "0.5001em", "8.01px", "0.501em",
        "0.502em", "0.51em", "0.55em", "0.6em", "0.75em", "1em", "-1px", "-2px", "-0.1em",
    ] {
        out.push(format!("p {{ letter-spacing: {ls} }}"));
    }
    for (size, ls) in [("6px", "3px"), ("12.5px", "0.5em"), ("13px", "6.5px"), ("21.5px", "0.5em"), ("21.5px", "0.49em"), ("9px", "0.3em")] {
        out.push(format!("p {{ font-size: {size}; letter-spacing: {ls} }}"));
    }
    for ws in ["4px", "0.49em", "0.5em", "8px", "1em", "-2px"] {
        out.push(format!("p {{ word-spacing: {ws} }}"));
    }
    out.push("p { letter-spacing: 0.25em; word-spacing: 0.25em }".to_owned());
    out.push("p { letter-spacing: 0.5em; word-spacing: 0.5em }".to_owned());
    out.push("p { letter-spacing: 3px; word-spacing: 0.5em }".to_owned());
    out.push("p { text-align: justify }".to_owned());
    out.push("p { text-align: justify; margin: 0 30% }".to_owned());
    out.push("p { text-align: justify; letter-spacing: 2px; word-spacing: 6px }".to_owned());
    out.push("p { text-align: justify; letter-spacing: 0.5em; margin: 0 20% }".to_owned());
    for fam in ["sans-serif", "monospace"] {
        out.push(format!("p {{ font-family: {fam} }}"));
        for ls in ["1px", "3px", "0.3em", "0.49em", "0.495em", "8px", "0.5em", "0.5001em", "0.6em", "1em"] {
            out.push(format!("p {{ font-family: {fam}; letter-spacing: {ls} }}"));
        }
    }
    out
}

fn esc(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c.to_string()
            } else if c == '\n' {
                "\\n".to_owned()
            } else {
                format!("\\u{{{:x}}}", c as u32)
            }
        })
        .collect()
}

fn hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn nsm(c: char) -> bool {
    bidi_class(c) == BidiClass::NSM
}

fn rtl(c: char) -> bool {
    matches!(bidi_class(c), BidiClass::R | BidiClass::AL)
}

#[test]
fn probe() {
    let filter = std::env::var("RV_FILTER").unwrap_or_default();
    let cos_pages = |doc: &Document| -> u64 {
        let cos = doc.cos();
        let mut all = Vec::new();
        for page in tinker_pdf_cos::pages::collect(cos) {
            all.extend_from_slice(&tinker_pdf_cos::pages::content_bytes(cos, &page));
        }
        hash(&all)
    };
    for style in styles() {
        for (name, lang, body, expected) in cases() {
            if !filter.is_empty() && !format!("{style}|{name}").contains(&filter) {
                continue;
            }
            let bytes = styled_book(lang, &style, &body);
            let doc = Document::open(bytes.clone()).expect("the book opens");
            let marked = body.chars().any(nsm);
            let mut text = String::new();
            let mut lines = 0;
            let mut outside = 0usize;
            let mut overlaps = 0usize;
            let mut maxx = f64::MIN;
            for at in 0..doc.page_count() {
                let page = doc.page(at).expect("a page");
                text.push_str(&page.text().plain_text());
                let drawn = page.text_with(&TextOptions { content_order: true });
                lines += drawn.lines().len();
                for line in drawn.lines() {
                    let mut base: Option<&tinker_pdf::TextChar> = None;
                    let mut prev: Option<&tinker_pdf::TextChar> = None;
                    for c in &line.chars {
                        let (x0, _, x1, _) = c.quad.bounds();
                        maxx = maxx.max(x1);
                        let mark = c.text.chars().next().is_some_and(nsm);
                        if mark {
                            match base {
                                Some(b) => {
                                    let centre = (x0 + x1) / 2.0;
                                    let (b0, _, b1, _) = b.quad.bounds();
                                    if !(b0 < centre && centre < b1) {
                                        outside += 1;
                                    }
                                }
                                None => outside += 1,
                            }
                            continue;
                        }
                        if let Some(p) = prev {
                            let (_, _, p1, _) = p.quad.bounds();
                            // Drawn left to right in content order on a line.
                            if x0 < p1 - 0.02 * c.size && x0 > p1 - 3.0 * c.size {
                                overlaps += 1;
                            }
                        }
                        base = Some(c);
                        prev = Some(c);
                    }
                }
            }
            // Visual overlap: every non-mark glyph on a page, grouped by
            // baseline, sorted along it; a box starting more than a fiftieth
            // of an em inside the one before it overlaps it.
            let mut vov = 0usize;
            let mut vov_at = String::new();
            for at in 0..doc.page_count() {
                let page = doc.page(at).expect("a page");
                let drawn = page.text_with(&TextOptions { content_order: true });
                let mut boxes: Vec<(i64, f64, f64, f64, String)> = Vec::new();
                for line in drawn.lines() {
                    for c in &line.chars {
                        if c.text.chars().next().is_some_and(nsm) || c.text.trim().is_empty() {
                            continue;
                        }
                        let (x0, _, x1, _) = c.quad.bounds();
                        boxes.push(((c.origin.1 * 10.0).round() as i64, x0, x1, c.size, c.text.clone()));
                    }
                }
                boxes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
                for pair in boxes.windows(2) {
                    let (p, n) = (&pair[0], &pair[1]);
                    if p.0 == n.0 && n.1 < p.2 - 0.02 * n.3 {
                        vov += 1;
                        if vov_at.is_empty() {
                            vov_at = format!("{}@{:.2}..{:.2}/{}@{:.2}", esc(&p.4), p.1, p.2, esc(&n.4), n.1);
                        }
                    }
                }
            }
            let page0 = doc.page(0).expect("a page");
            let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
            let text_ok = expected
                .as_ref()
                .map_or("-".to_owned(), |e| (norm(&text).contains(&norm(e)) as u8).to_string());
            let tokens = |e: &str| -> Vec<String> {
                e.split_whitespace()
                    .map(|t| t.trim_matches(|c: char| c == ',' || c == '.').to_owned())
                    .filter(|t| !t.is_empty())
                    .collect()
            };
            let found = expected.as_ref().map_or("-".to_owned(), |e| {
                let needles: Vec<String> = tokens(e).into_iter().filter(|t| t.chars().any(nsm)).collect();
                if needles.is_empty() {
                    return "-".to_owned();
                }
                let hits = needles.iter().filter(|n| !page0.text().search(n).is_empty()).count();
                format!("{hits}/{}", needles.len())
            });
            let ltr = expected.as_ref().map_or("-".to_owned(), |e| {
                let needles: Vec<String> = tokens(e).into_iter().filter(|t| !t.chars().any(rtl)).collect();
                if needles.is_empty() {
                    return "-".to_owned();
                }
                let hits = needles.iter().filter(|n| !page0.text().search(n).is_empty()).count();
                format!("{hits}/{}", needles.len())
            });
            let c = conservation(&bytes, &doc);
            let l = conservation_in_logical_order(&bytes, &doc);
            let first = |v: &epub_support::conservation::Verdict| {
                v.divergences.first().map(|d| esc(&format!("{d:?}"))).unwrap_or_default()
            };
            println!(
                "ROW\t{style}\t{name}\tm={}\ttext={text_ok}\tsearch={found}\tltr={ltr}\tcons={}\tlogical={}\tride={outside}\toverlap={overlaps}/{vov}\tlines={lines}\tmaxx={maxx:.3}\tthash={:016x}\tchash={:016x}\t{}\t{}\t{}",
                marked as u8,
                c.holds() as u8,
                l.holds() as u8,
                hash(text.as_bytes()),
                cos_pages(&doc),
                if text.chars().count() < 200 || std::env::var("RV_FULL").is_ok() { esc(&text) } else { String::from("<long>") },
                first(&c),
                first(&l),
            );
            if vov > 0 {
                println!("VOV\t{style}\t{name}\t{vov}\t{vov_at}");
            }
        }
    }
}
