//! Lane V probe (round 6 of ROADMAP CD-19): family scripts2. The rr3m prefix is
//! zz_rr3m_h2_pages.rs's, byte for byte; the cases and styles below are new.
#![allow(missing_docs, clippy::all, unused)]

mod epub_support;

use epub_support::book::styled_book;
use epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::{Document, TextOptions};
use tinker_pdf_shape::unicode::{bidi_class, BidiClass};

/// (name, language, body)
type Case = (String, &'static str, String);

const HE: [&str; 12] = ["שלום", "עולם", "מלחמה", "ספר", "בית", "גדול", "אור", "יום", "בראשית", "ברא", "אלהים", "הארץ"];
const HEP: [&str; 12] = ["שָׁלוֹם", "עוֹלָם", "מִלְחָמָה", "סֵפֶר", "בַּיִת", "גָּדוֹל", "אוֹר", "יוֹם", "בְּרֵאשִׁית", "בָּרָא", "אֱלֹהִים", "הָאָרֶץ"];
const AR: [&str; 12] = ["كتاب", "مدرسة", "الطالب", "جميل", "بيت", "كبير", "نور", "يوم", "عوامل", "وقد", "أصبحنا", "نعرف"];
const ARP: [&str; 12] = ["كِتَابٌ", "مَدْرَسَةٌ", "الطَّالِبُ", "جَمِيلٌ", "بَيْتٌ", "كَبِيرٌ", "نُورٌ", "يَوْمٌ", "عَوَامِلُ", "وَقَدْ", "أَصْبَحْنَا", "نَعْرِفُ"];

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

fn strip_marks(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0591}'..='\u{05C7}'))
        .collect()
}

const BLOCKS: [&str; 26] = [
    "p", "li", "td", "th", "tr", "figcaption", "div", "br", "h1", "h2", "h3", "h4", "h5", "h6", "dt", "dd",
    "caption", "pre", "blockquote", "figure", "table", "ul", "ol", "dl", "section", "tbody",
];

/// The book's visible text, one string per block (and per preserved line).
fn segments(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = body.chars().peekable();
    let mut flush = |cur: &mut String, out: &mut Vec<String>| {
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
        } else if c == '&' {
            let mut ent = String::new();
            while let Some(d) = chars.next() {
                if d == ';' {
                    break;
                }
                ent.push(d);
            }
            let ch = match ent.as_str() {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "nbsp" => Some('\u{A0}'),
                e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
                e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(ch) = ch {
                cur.push(ch);
            }
        } else if c == '\n' {
            flush(&mut cur, &mut out);
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

fn run_book(style: &str, name: &str, lang: &str, body: &str) {
    let bytes = styled_book(lang, style, body);
    let doc = match Document::open(bytes.clone()) {
        Ok(doc) => doc,
        Err(e) => {
            println!("ROW\t{style}\t{name}\tm=0\ttext=0\tsearch=0/1\tltr=-\tcons=0\tlogical=0\tOPEN-ERR {}", esc(&format!("{e:?}")));
            return;
        }
    };
    let cos_pages = |doc: &Document| -> u64 {
        let cos = doc.cos();
        let mut all = Vec::new();
        for page in tinker_pdf_cos::pages::collect(cos) {
            all.extend_from_slice(&tinker_pdf_cos::pages::content_bytes(cos, &page));
        }
        hash(&all)
    };
    let marked = body.chars().any(nsm);
    let mut text = String::new();
    let mut lines = 0;
    let mut outside = 0usize;
    let mut overlaps = 0usize;
    let mut maxx = f64::MIN;
    let mut vov = 0usize;
    let mut vov_at = String::new();
    let mut texts = Vec::new();
    for at in 0..doc.page_count() {
        let page = doc.page(at).expect("a page");
        let t = page.text();
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&t.plain_text());
        texts.push(t);
        let drawn = page.text_with(&TextOptions { content_order: true });
        lines += drawn.lines().len();
        let mut boxes: Vec<(i64, f64, f64, f64, String)> = Vec::new();
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
                    if x0 < p1 - 0.02 * c.size && x0 > p1 - 3.0 * c.size {
                        overlaps += 1;
                    }
                }
                base = Some(c);
                prev = Some(c);
                if !c.text.trim().is_empty() {
                    boxes.push(((c.origin.1 * 10.0).round() as i64, x0, x1, c.size, c.text.clone()));
                }
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
    let segs = segments(body);
    let full = norm(&text);
    let text_ok = if segs.is_empty() { "-".to_owned() } else { (segs.iter().all(|s| full.contains(s.as_str())) as u8).to_string() };
    let toks = tokens(&segs);
    let found_in = |t: &str| texts.iter().any(|x| !x.search(t).is_empty());
    let found = if toks.is_empty() {
        "-".to_owned()
    } else {
        format!("{}/{}", toks.iter().filter(|t| found_in(t)).count(), toks.len())
    };
    let ltr_toks: Vec<&String> = toks.iter().filter(|t| !t.chars().any(rtl)).collect();
    let ltr = if ltr_toks.is_empty() {
        "-".to_owned()
    } else {
        format!("{}/{}", ltr_toks.iter().filter(|t| found_in(t)).count(), ltr_toks.len())
    };
    let c = conservation(&bytes, &doc);
    let l = conservation_in_logical_order(&bytes, &doc);
    let first = |v: &epub_support::conservation::Verdict| v.divergences.first().map(|d| esc(&format!("{d:?}"))).unwrap_or_default();
    let missing: Vec<&String> = toks.iter().filter(|t| !found_in(t)).take(4).collect();
    println!(
        "ROW\t{style}\t{name}\tm={}\ttext={text_ok}\tsearch={found}\tltr={ltr}\tcons={}\tlogical={}\tride={outside}\toverlap={overlaps}/{vov}\tlines={lines}\tmaxx={maxx:.3}\tthash={:016x}\tchash={:016x}\t{}\t{}\t{}\tnf={}",
        marked as u8,
        c.holds() as u8,
        l.holds() as u8,
        hash(text.as_bytes()),
        cos_pages(&doc),
        if text.chars().count() < 1200 || std::env::var("RV_FULL").is_ok() { esc(&text) } else { String::from("<long>") },
        first(&c),
        first(&l),
        esc(&missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")),
    );
    if vov > 0 {
        println!("VOV\t{style}\t{name}\t{vov}\t{vov_at}");
    }
}

fn para3(out: &mut Vec<Case>, name: &str, lang: &'static str, text: &str) {
    out.push((format!("{name}/rtl"), lang, format!("<p dir=\"rtl\">{text}</p>")));
    out.push((format!("{name}/none"), lang, format!("<p>{text}</p>")));
    out.push((format!("{name}/css"), lang, format!("<div style=\"direction: rtl\"><p>{text}</p></div>")));
}

fn rep(n: usize, f: impl Fn(usize) -> String) -> String {
    (0..n).map(|k| f(k * 3)).collect::<Vec<_>>().join(" ")
}

#[test]
fn probe() {
    let filter = std::env::var("RV_FILTER").unwrap_or_default();
    let mut n = 0usize;
    for style in styles() {
        for (name, lang, body) in cases() {
            if !filter.is_empty() && !format!("{style}|{name}").contains(&filter) {
                continue;
            }
            n += 1;
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_book(&style, &name, lang, &body)));
            if r.is_err() {
                println!("ROW\t{style}\t{name}\tm=0\ttext=0\tsearch=0/1\tltr=-\tcons=0\tlogical=0\tPANIC");
                println!("PANIC\t{style}\t{name}");
            }
        }
    }
    println!("books={n}");
}



// Family scripts2: right-to-left scripts no probe has set, among them the
// supplementary planes' (four bytes in UTF-8, a surrogate pair in UTF-16):
// Adlam, Hanifi Rohingya, Phoenician, Imperial Aramaic, Mende Kikakui; and
// the Basic Multilingual Plane's forms a reader of old PDFs meets: Arabic and
// Hebrew presentation forms, Urdu's letters, Persian with ZERO WIDTH NON-JOINER,
// a kashida (tatweel) inside a word, Yiddish ligatures.

const ADLAM: [&str; 8] = ["\u{1E900}\u{1E901}\u{1E902}", "\u{1E903}\u{1E904}", "\u{1E905}\u{1E906}\u{1E907}\u{1E908}", "\u{1E909}\u{1E90A}", "\u{1E90B}\u{1E90C}\u{1E90D}", "\u{1E90E}\u{1E90F}", "\u{1E910}\u{1E911}\u{1E912}", "\u{1E913}\u{1E914}"];
const ROHG: [&str; 8] = ["\u{10D00}\u{10D01}\u{10D02}", "\u{10D03}\u{10D04}", "\u{10D05}\u{10D06}\u{10D07}", "\u{10D08}\u{10D09}", "\u{10D0A}\u{10D0B}\u{10D0C}", "\u{10D0D}\u{10D0E}", "\u{10D0F}\u{10D10}\u{10D11}", "\u{10D12}\u{10D13}"];
const PHNX: [&str; 8] = ["\u{10900}\u{10901}\u{10902}", "\u{10903}\u{10904}", "\u{10905}\u{10906}\u{10907}", "\u{10908}\u{10909}", "\u{1090A}\u{1090B}\u{1090C}", "\u{1090D}\u{1090E}", "\u{1090F}\u{10910}\u{10911}", "\u{10912}\u{10913}"];
const ARMI: [&str; 8] = ["\u{10840}\u{10841}\u{10842}", "\u{10843}\u{10844}", "\u{10845}\u{10846}\u{10847}", "\u{10848}\u{10849}", "\u{1084A}\u{1084B}\u{1084C}", "\u{1084D}\u{1084E}", "\u{1084F}\u{10850}\u{10851}", "\u{10852}\u{10853}"];
const MEND: [&str; 8] = ["\u{1E800}\u{1E801}\u{1E802}", "\u{1E803}\u{1E804}", "\u{1E805}\u{1E806}\u{1E807}", "\u{1E808}\u{1E809}", "\u{1E80A}\u{1E80B}\u{1E80C}", "\u{1E80D}\u{1E80E}", "\u{1E80F}\u{1E810}\u{1E811}", "\u{1E812}\u{1E813}"];
// Arabic presentation forms B: كتاب, بيت, نور, يوم... as their positional forms.
const ARPF: [&str; 8] = ["\u{FEDB}\u{FE98}\u{FE8E}\u{FE8F}", "\u{FE91}\u{FEF4}\u{FE96}", "\u{FEE7}\u{FEEE}\u{FEAD}", "\u{FEF3}\u{FEEE}\u{FEE1}", "\u{FEDF}\u{FEE4}\u{FE94}", "\u{FEB3}\u{FEE0}\u{FE8E}\u{FEE1}", "\u{FEE3}\u{FEA4}\u{FEE4}\u{FEAA}", "\u{FEFB}"];
// Hebrew presentation forms: shin with dagesh and dots, alef with patah, wide letters.
const HEPF: [&str; 8] = ["\u{FB2C}\u{5DC}\u{5D5}\u{5DD}", "\u{FB2E}\u{5D5}\u{5E8}", "\u{FB31}\u{5D9}\u{5EA}", "\u{FB4B}\u{5DC}\u{5DD}", "\u{5E1}\u{FB44}\u{5E8}", "\u{FB21}\u{5D3}\u{5DD}", "\u{FB4F}", "\u{5D9}\u{FB35}\u{5DD}"];
const URDU: [&str; 8] = ["\u{67E}\u{627}\u{6A9}\u{633}\u{62A}\u{627}\u{646}", "\u{679}\u{6CC}\u{644}\u{6CC}", "\u{688}\u{627}\u{6A9}", "\u{6AF}\u{691}\u{6BE}\u{6CC}", "\u{6BA}\u{6C1}\u{6D2}", "\u{6A9}\u{62A}\u{627}\u{628}", "\u{6CC}\u{6C1}\u{627}\u{6BA}", "\u{648}\u{6C1}"];
const FAZ: [&str; 8] = ["\u{645}\u{6CC}\u{200C}\u{62E}\u{648}\u{627}\u{647}\u{645}", "\u{646}\u{645}\u{6CC}\u{200C}\u{62F}\u{627}\u{646}\u{645}", "\u{6A9}\u{62A}\u{627}\u{628}\u{200C}\u{647}\u{627}", "\u{62E}\u{627}\u{646}\u{647}", "\u{628}\u{632}\u{631}\u{6AF}\u{200C}\u{062A}\u{631}", "\u{631}\u{648}\u{632}", "\u{647}\u{645}\u{200C}\u{627}\u{6CC}\u{646}", "\u{633}\u{627}\u{644}"];
const KASH: [&str; 8] = ["\u{643}\u{62A}\u{640}\u{640}\u{640}\u{627}\u{628}", "\u{628}\u{64A}\u{640}\u{62A}", "\u{646}\u{640}\u{648}\u{631}", "\u{64A}\u{648}\u{645}", "\u{62C}\u{645}\u{640}\u{640}\u{64A}\u{644}", "\u{643}\u{628}\u{64A}\u{631}", "\u{639}\u{640}\u{648}\u{627}\u{645}\u{644}", "\u{648}\u{642}\u{62F}"];
const YI: [&str; 8] = ["\u{5F2}\u{5D9}\u{5D3}\u{5D9}\u{5E9}", "\u{5F0}\u{5D0}\u{5B7}\u{5E1}\u{5E2}\u{5E8}", "\u{5D1}\u{5F1}", "\u{5E9}\u{5D8}\u{5D5}\u{5D1}", "\u{5D4}\u{5F2}\u{5D6}", "\u{5D6}\u{5E2}\u{5DC}\u{5D8}", "\u{5DC}\u{5F2}\u{5B7}\u{5D8}", "\u{5D2}\u{5D5}\u{5D8}"];

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let scripts: [(&str, &'static str, [&str; 8]); 11] = [
        ("adlam", "ff", ADLAM),
        ("rohg", "rhg", ROHG),
        ("phnx", "phn", PHNX),
        ("armi", "arc", ARMI),
        ("mend", "men", MEND),
        ("arpf", "ar", ARPF),
        ("hepf", "he", HEPF),
        ("urdu", "ur", URDU),
        ("faz", "fa", FAZ),
        ("kash", "ar", KASH),
        ("yi", "yi", YI),
    ];
    for (sc, lang, w) in scripts {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<(&str, Box<dyn Fn(usize) -> String>)> = vec![
            ("prose", Box::new(move |i| format!("{} {} {}, {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5)))),
            ("styled", Box::new(move |i| format!("{} <b>{}</b> {} <i>{}</i> {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("punct", Box::new(move |i| format!("{} ({}) {}?! {} \u{ab}{}\u{bb}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("num", Box::new(move |i| format!("{} 2010 {} 12.5 {} (3) {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("gloss", Box::new(move |i| format!("{} {} (abc def) {} <i>xyz</i> {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
        ];
        for (name, shape) in &shapes {
            let text = rep(3, |k| shape(k));
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((format!("{sc}/{name}/{d}"), lang, format!("<p{attr}>{text}</p>")));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p { margin: 0 20% }",
        "p { word-spacing: 0.6em }",
        "p { word-spacing: 1em; margin: 0 20% }",
        "p { text-align: justify; margin: 0 25% }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify }",
        "p { letter-spacing: 0.5em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
