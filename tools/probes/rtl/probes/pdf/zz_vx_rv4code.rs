//! Scratch round-4 code review probe (never committed): the characters the
//! `reads_right_to_left` table takes beyond `R`/`AL` (ornate parentheses,
//! Arabic number punctuation), palindromic neutral slices under word
//! spacing, and numbers/Latin words inside word-spaced right-to-left text.
//! One ROW per book: content, logical, search.
#![allow(missing_docs, clippy::all)]

mod epub_support;


mod zz_vov_shim {
    use tinker_pdf::{Document, TextOptions};
    use tinker_pdf_shape::unicode::{bidi_class, BidiClass};
    fn h(s: &str) -> u64 {
        let mut x: u64 = 0xcbf29ce484222325;
        for b in s.bytes() {
            x ^= u64::from(b);
            x = x.wrapping_mul(0x100000001b3);
        }
        x
    }
    pub fn styled_book(lang: &str, style: &str, body: &str) -> Vec<u8> {
        let bytes = super::epub_support::book::styled_book(lang, style, body);
        let mut vov = 0usize;
        let mut at = String::new();
        if let Ok(doc) = Document::open(bytes.clone()) {
            for p in 0..doc.page_count() {
                let Some(page) = doc.page(p) else { continue };
                let drawn = page.text_with(&TextOptions { content_order: true });
                let mut boxes: Vec<(i64, f64, f64, f64, String)> = Vec::new();
                for line in drawn.lines() {
                    for c in &line.chars {
                        let (x0, _, x1, _) = c.quad.bounds();
                        if c.text.chars().next().is_some_and(|ch| bidi_class(ch) == BidiClass::NSM) {
                            continue;
                        }
                        if !c.text.trim().is_empty() {
                            boxes.push(((c.origin.1 * 10.0).round() as i64, x0, x1, c.size, c.text.clone()));
                        }
                    }
                }
                boxes.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
                for pair in boxes.windows(2) {
                    let (a, b) = (&pair[0], &pair[1]);
                    if a.0 == b.0 && b.1 < a.2 - 0.02 * b.3 {
                        vov += 1;
                        if at.is_empty() {
                            at = format!("{:?}@{:.2}..{:.2}/{:?}@{:.2}", a.4, a.1, a.2, b.4, b.1);
                        }
                    }
                }
            }
        }
        println!("VOVX\t{style}{}\t{lang}\t{:016x}\t{vov}\t{at}", if body.contains("word-spacing") || body.contains("justify") { " INLINE-WS" } else { "" }, h(&format!("{lang}|{body}")));
        bytes
    }
}
use zz_vov_shim::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

fn strip(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0591}'..='\u{05C7}'))
        .collect()
}

fn untag(s: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in s.chars() {
        match c {
            '<' => inside = true,
            '>' => {
                inside = false;
                out.push(' ');
            }
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

const AR: [&str; 8] = ["عوامل", "عدّة", "وقد", "أصبحنا", "نعرف", "الآن", "الموادّ", "تكبح"];
const HE: [&str; 8] = ["שָׁלוֹם", "עוֹלָם", "מִלְחָמָה", "סֵפֶר", "בַּיִת", "גָּדוֹל", "אוֹר", "יוֹם"];

type Book = (String, &'static str, String);

const OPEN: char = '\u{FD3F}';
const CLOSE: char = '\u{FD3E}';

fn books() -> Vec<Book> {
    let mut out: Vec<Book> = Vec::new();
    for (script, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        let para = |name: &str, text: String, out: &mut Vec<Book>| {
            out.push((format!("{script}/rtl/{name}"), lang, format!("<p dir=\"rtl\">{text}</p>")));
            out.push((format!("{script}/none/{name}"), lang, format!("<p>{text}</p>")));
        };
        // Ornate parentheses, FD3F opening and FD3E closing in RTL use, and swapped.
        for (tag, o, c) in [("orn", OPEN, CLOSE), ("orn-sw", CLOSE, OPEN)] {
            para(&format!("{tag}-i"), format!("<i>{o}</i>{} {}<i>{c}</i>", s(0), s(1)), &mut out);
            para(&format!("{tag}-b-mid"), format!("{} {} <b>{o}</b>{} {}<b>{c}</b> {} {}.", s(2), s(3), s(0), s(1), s(4), s(5)), &mut out);
            para(&format!("{tag}-plain"), format!("{} {} {o}{} {}{c} {} {}.", s(2), s(3), s(0), s(1), s(4), s(5)), &mut out);
            para(
                &format!("{tag}-x3"),
                (0..3)
                    .map(|i| format!("{} {} {o}{} {}{c} {} {} {}. ", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5), s(i + 6)))
                    .collect(),
                &mut out,
            );
            para(&format!("{tag}-latin"), format!("{} {o}HDAC{c} {} {}.", s(0), s(1), s(2)), &mut out);
            out.push((
                format!("{script}/en/{tag}"),
                "en",
                format!("<p>The verse {o}{} {}{c} and the words {o}<i>{}</i>{c} are quoted in this English sentence here.</p>", s(0), s(1), s(2)),
            ));
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((
                    format!("{script}/table-{tag}/{d}"),
                    lang,
                    format!(
                        "<table{attr} style=\"border-spacing: 0\"><tr><td>{}</td><td>{o}{}{c}</td><td>{}</td></tr><tr><td>{o}{}{c}</td><td>{}</td><td>x</td></tr></table>",
                        s(0), s(1), s(2), s(3), s(4)
                    ),
                ));
            }
        }
        // Arabic number punctuation: thousands and decimal separators, percent, per mille.
        para("an-sep", format!("{} {} (١٬٢٣٤) {} ٣٫١٤ {} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        para("an-pct", format!("{} ٥٠٪ {} <i>٪</i> {} ؉ {}.", s(0), s(1), s(2), s(3)), &mut out);
        para("an-sep-i", format!("{} <i>٬</i>{} <b>٫</b> {}.", s(0), s(1), s(2)), &mut out);
        para(
            "an-sep-x3",
            (0..3)
                .map(|i| format!("{} {} {} (١٬٢٣٤), {} {} {} {}. ", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5), s(i + 6)))
                .collect(),
            &mut out,
        );
        for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
            out.push((
                format!("{script}/table-an/{d}"),
                lang,
                format!(
                    "<table{attr} style=\"border-spacing: 0\"><tr><td>{}</td><td>٪</td><td>{}</td></tr><tr><td>١٬٢٣٤</td><td>{}</td><td>٫</td></tr></table>",
                    s(0), s(1), s(2)
                ),
            ));
        }
        // Palindromic neutral slices after a word space.
        para("pal-paren2", format!("{} <i>( (</i>{} {}.", s(0), s(1), s(2)), &mut out);
        para("pal-dash", format!("{} -- {} .. {} ## {}.", s(0), s(1), s(2), s(3)), &mut out);
        para("pal-hash", format!("{} #{} {} ##{} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        para("pal-mixed", format!("{} (#{}) {} [#]{} {}.", s(0), s(1), s(2), s(3), s(4)), &mut out);
        // Numbers and Latin/Greek words inside right-to-left text.
        para("num-paren", format!("(١٢٣) {} {} (١٢٣) {}.", s(0), s(1), s(2)), &mut out);
        para("num-en", format!("{} (2010) {} 15 {} 1,234.5 {}.", s(0), s(1), s(2), s(3)), &mut out);
        para("greek", format!("{} (αλφα βήτα) {} αλφα {}.", s(0), s(1), s(2)), &mut out);
        para("latin-2w", format!("{} (histone deacetylase) {} HDAC SIRT {}.", s(0), s(1), s(2)), &mut out);
        para(
            "num-x3",
            (0..3)
                .map(|i| format!("{} {} {} (١٢٣), {} {} {} {}. ", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5), s(i + 6)))
                .collect(),
            &mut out,
        );
        para(
            "greek-x3",
            (0..3)
                .map(|i| format!("{} {} (αλφα βήτα), {} {} {} {}. ", s(i), s(i + 1), s(i + 3), s(i + 4), s(i + 5), s(i + 6)))
                .collect(),
            &mut out,
        );
    }
    out
}

#[test]
fn probe() {
    let only = std::env::var("RV4C_FILTER").unwrap_or_default();
    let styles = [
        "",
        "p, li { text-align: justify; margin: 0 30% }",
        "p, li { text-align: justify; margin: 0 22% }",
        "p, li, td { letter-spacing: 0.5em }",
        "p, li, td { letter-spacing: 0.55em; text-align: justify; margin: 0 20% }",
        "p, li, td { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }",
        "p, li { word-spacing: 1em; margin: 0 18% }",
        "p, li { word-spacing: 0.5em }",
        "body { font-family: monospace } p, li { text-align: justify; margin: 0 20% }",
        "body { font-family: sans-serif } p, li { text-align: justify; margin: 0 15% }",
        "td { padding: 0; letter-spacing: 0.6em } table { border-spacing: 0 }",
        "td { padding: 0; word-spacing: 1em; letter-spacing: 0.3em } table { border-spacing: 0 }",
    ];
    let (mut rows, mut fails) = (0, 0);
    for (si, style) in styles.iter().enumerate() {
        for (name, lang, body) in books() {
            for (label, body) in [("pointed", body.clone()), ("twin", strip(&body))] {
                if label == "twin" && body == strip(&body) && false {
                    continue;
                }
                let key = format!("s{si}\t{name}\t{label}");
                if !only.is_empty() && !key.contains(&only) {
                    continue;
                }
                let bytes = styled_book(lang, style, &body);
                let doc = match Document::open(bytes.clone()) {
                    Ok(doc) => doc,
                    Err(e) => {
                        println!("ROW\t{key}\tOPEN-ERR {e:?}");
                        continue;
                    }
                };
                let c = conservation(&bytes, &doc);
                let l = conservation_in_logical_order(&bytes, &doc);
                rows += 1;
                if !c.holds() || !l.holds() {
                    fails += 1;
                }
                let pages: Vec<_> = (0..doc.page_count()).filter_map(|p| doc.page(p)).collect();
                let texts: Vec<_> = pages.iter().map(|p| p.text()).collect();
                let keep = |c: char| c.is_alphanumeric() || matches!(c, '\u{0591}'..='\u{05C7}' | '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{066B}' | '\u{066C}');
                let words: Vec<String> = untag(&body)
                    .split_whitespace()
                    .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && !matches!(c, '\u{0591}'..='\u{05C7}' | '\u{064B}'..='\u{065F}' | '\u{0670}')).to_owned())
                    .filter(|w| w.chars().any(char::is_alphanumeric) && w.chars().all(keep))
                    .collect();
                let found = words
                    .iter()
                    .filter(|w| texts.iter().any(|t| !t.search(w).is_empty()))
                    .count();
                let text: String = texts
                    .iter()
                    .map(|t| t.plain_text())
                    .collect::<Vec<_>>()
                    .join("|")
                    .replace('\n', "|");
                println!(
                    "ROW\t{key}\tcontent={}\tlogical={}\tsearch={}\tfound={found}/{}\t{:?}\t{}",
                    c.holds(),
                    l.holds(),
                    found == words.len(),
                    words.len(),
                    l.divergences.first().or(c.divergences.first()),
                    text
                );
            }
        }
    }
    println!("rows={rows} fails={fails}");
}
