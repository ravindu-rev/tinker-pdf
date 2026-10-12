//! Scratch round-3 review probe (never committed): the round-3 shapes in
//! long, justified paragraphs without letter-spacing — opening marks before
//! right-to-left words, styled neutral runs between them, Arabic-Indic
//! digits in brackets, and glosses in English. One ROW per book.
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

const AR: [&str; 12] = ["عوامل", "عدّة", "وقد", "أصبحنا", "نعرف", "الآن", "الموادّ", "تكبح", "الخلايا", "السرطانيّة", "تتكاثر", "بسرعة"];
const HE: [&str; 12] = ["שָׁלוֹם", "עוֹלָם", "מִלְחָמָה", "סֵפֶר", "בַּיִת", "גָּדוֹל", "אוֹר", "יוֹם", "בְּרֵאשִׁית", "בָּרָא", "אֱלֹהִים", "הָאָרֶץ"];

type Book = (String, &'static str, String);

fn books() -> Vec<Book> {
    let mut out: Vec<Book> = Vec::new();
    for (script, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        // A sentence of `n` words starting at `i`, with `mark(k)` spliced in
        // before the k-th word.
        let shapes: Vec<(&str, Box<dyn Fn(usize) -> String>)> = vec![
            ("guil", Box::new(move |i| format!("{} {} «{} {}» {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5)))),
            ("guil-i", Box::new(move |i| format!("{} {} <i>«</i>{} {}<i>»</i> {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5)))),
            ("curly", Box::new(move |i| format!("{} {} “{}” {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("straight", Box::new(move |i| format!("{} \"{} {}\" {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("num", Box::new(move |i| format!("(1) {} {} (2) {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("dash", Box::new(move |i| format!("{} — {} {} — {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("dash-b", Box::new(move |i| format!("{} <b>—</b> {} {} <b>—</b> {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("sq", Box::new(move |i| format!("{} [{}] {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("paren-rtl", Box::new(move |i| format!("{} ({} {}) {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("qx-i", Box::new(move |i| format!("{} <i>?!</i> {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("semi-b", Box::new(move |i| format!("{} {}<b>;</b> {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("ellipsis-i", Box::new(move |i| format!("{} <i>…</i> {} {}.", s(i), s(i + 1), s(i + 2)))),
            ("colon-b", Box::new(move |i| format!("{}<b>:</b> {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("an-paren", Box::new(move |i| format!("{} {} (١٢٣) {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("an-bare", Box::new(move |i| format!("{} ١٢٣ {} ٤٥ {}.", s(i), s(i + 1), s(i + 2)))),
            ("en-paren", Box::new(move |i| format!("{} {} (2010) {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("hash", Box::new(move |i| format!("{} #{} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("star", Box::new(move |i| format!("* {} {} * {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("pct", Box::new(move |i| format!("{} 15% {} %{} {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
        ];
        for (name, shape) in &shapes {
            let text: String = (0..6).map(|k| shape(k * 3)).collect::<Vec<_>>().join(" ");
            out.push((format!("{script}/rtl/{name}"), lang, format!("<p dir=\"rtl\">{text}</p>")));
            out.push((format!("{script}/none/{name}"), lang, format!("<p>{text}</p>")));
        }
        // English paragraphs quoting right-to-left words.
        let en: Vec<(&str, String)> = vec![
            ("gloss", (0..5).map(|k| format!("The word for peace ({} {}) and for war ({}) appear here,", s(3 * k), s(3 * k + 1), s(3 * k + 2))).collect::<Vec<_>>().join(" ")),
            ("quote", (0..5).map(|k| format!("He said «{} {}» and then “{}” before [{}] was read,", s(3 * k), s(3 * k + 1), s(3 * k + 2), s(3 * k + 3))).collect::<Vec<_>>().join(" ")),
            ("dash", (0..5).map(|k| format!("A list — {} {} — and {} - {} in the line,", s(3 * k), s(3 * k + 1), s(3 * k + 2), s(3 * k + 3))).collect::<Vec<_>>().join(" ")),
            ("styled", (0..5).map(|k| format!("Words <b>{}</b> <i>{}</i> (<b>{}</b>) and more English text,", s(3 * k), s(3 * k + 1), s(3 * k + 2))).collect::<Vec<_>>().join(" ")),
        ];
        for (name, text) in en {
            out.push((format!("{script}/en/{name}"), "en", format!("<p>{text}.</p>")));
        }
    }
    out
}

#[test]
fn probe() {
    let only = std::env::var("RV3_FILTER").unwrap_or_default();
    let styles = [
        "p { text-align: justify }",
        "p { text-align: justify; margin: 0 15% }",
        "p { text-align: justify; margin: 0 25% }",
        "p { text-align: justify; margin: 0 35% }",
        "p { text-align: justify; margin: 0 40% }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 30% }",
        "body { font-family: monospace } p { text-align: justify; margin: 0 20% }",
        "p { text-align: justify; font-size: 1.3em; margin: 0 10% }",
        "p { word-spacing: 0.3em; margin: 0 20% }",
        "p { letter-spacing: 0.1em; text-align: justify; margin: 0 25% }",
    ];
    let (mut rows, mut fails) = (0, 0);
    for (si, style) in styles.iter().enumerate() {
        for (name, lang, body) in books() {
            for (label, body) in [("pointed", body.clone()), ("twin", strip(&body))] {
                let key = format!("j{si}\t{name}\t{label}");
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
                let mark = |c: char| matches!(c, '\u{0591}'..='\u{05C7}' | '\u{064B}'..='\u{065F}' | '\u{0670}');
                let words: Vec<String> = untag(&body)
                    .split_whitespace()
                    .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && !mark(c)).to_owned())
                    .filter(|w| w.chars().any(char::is_alphanumeric) && w.chars().all(|c| c.is_alphanumeric() || mark(c)))
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
