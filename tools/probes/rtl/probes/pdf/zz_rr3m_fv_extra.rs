//! Fix-verify probe (scratch, never committed).
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

fn cases() -> Vec<Case> {
    // Fix-verify extra family: books aimed at the fix's three new paths —
    // the stretch-end as-typed rule, the mixed-piece as-typed rule, and the
    // comma tie-break's line-before exception — in shapes the hunt did not.
    let mut out: Vec<Case> = Vec::new();
    // 1. stretch ends: styled neutrals glued after / both sides / at the ends.
    let pieces = ["?!", "...", "!!", "),", "\".", ":-", "»,", "?! ?!"];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        for (pi, p) in pieces.iter().enumerate() {
            let after = rep(3, |i| format!("{} {}<b>{p}</b> {} {}.", s(i + pi), s(i + pi + 1), s(i + pi + 2), s(i + pi + 3)));
            let both = rep(3, |i| format!("{} {}<b>{p}</b>{} {}.", s(i + pi), s(i + pi + 1), s(i + pi + 2), s(i + pi + 3)));
            let end = format!("{} {} {}<b>{p}</b>", s(pi), s(pi + 1), s(pi + 2));
            let start = format!("<b>{p}</b>{} {} {}.", s(pi), s(pi + 1), s(pi + 2));
            for (shape, text) in [("after", after), ("both", both), ("end", end), ("start", start)] {
                out.push((format!("s/{sc}/p{pi}/{shape}/rtl"), lang, format!("<p dir=\"rtl\">{text}</p>")));
                out.push((format!("s/{sc}/p{pi}/{shape}/none"), lang, format!("<p>{text}</p>")));
            }
        }
    }
    // 2. mixed pieces: a number with a sign opening a wrapped line, a styled
    // word after it.
    let nums = ["5%", "٥٪", "3.5%", "+5", "5°", "(5%)", "5%,", "#5"];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        for (ni, n) in nums.iter().enumerate() {
            let a = rep(3, |i| format!("{} {} {} {} {n} <b>{}</b> {}", s(i + ni), s(i + ni + 1), s(i + ni + 2), s(i + ni + 3), s(i + ni + 4), s(i + ni + 5)));
            let b = rep(3, |i| format!("{} {} {} {} {n}<i>{}</i>. {}", s(i + ni), s(i + ni + 1), s(i + ni + 2), s(i + ni + 3), s(i + ni + 4), s(i + ni + 5)));
            for (shape, text) in [("bold", a), ("glued", b)] {
                out.push((format!("m/{sc}/n{ni}/{shape}/rtl"), lang, format!("<p dir=\"rtl\">{text}</p>")));
                out.push((format!("m/{sc}/n{ni}/{shape}/none"), lang, format!("<p>{text}</p>")));
            }
        }
    }
    // 3. the comma tie-break's mark: dir-less lines `(W) W<mark>` and
    // right-to-left lines that open with a mark after a forced break.
    let marks = [".", ",", ":", ";", "!", "?", "،", "…", ".\u{201D}"];
    let brackets = [("(", ")"), ("[", "]"), ("{", "}"), ("«", "»"), ("\u{FD3F}", "\u{FD3E}")];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE), ("hep", "he", HEP)] {
        let s = |i: usize| w[i % w.len()];
        for (mi, m) in marks.iter().enumerate() {
            for (bi, (o, c)) in brackets.iter().enumerate() {
                if sc == "hep" && bi > 1 {
                    continue;
                }
                let apart = rep(2, |i| format!("{} {} {} <b>{o}</b>{}<b>{c}</b> {}{m}", s(i + mi), s(i + mi + 1), s(i + mi + 2), s(i + mi + 3), s(i + mi + 4)));
                let two = rep(2, |i| format!("{} {} <b>{o}</b>{}<b>{c}</b> {o}{}{c} {}{m}", s(i + mi), s(i + mi + 1), s(i + mi + 2), s(i + mi + 3), s(i + mi + 4)));
                let br = format!("{} {}<br/>{m}{o}{}{c} {} {}", s(mi), s(mi + 1), s(mi + 2), s(mi + 3), s(mi + 4));
                out.push((format!("c/{sc}/m{mi}/b{bi}/apart/none"), lang, format!("<p>{apart}</p>")));
                out.push((format!("c/{sc}/m{mi}/b{bi}/two/none"), lang, format!("<p>{two}</p>")));
                out.push((format!("c/{sc}/m{mi}/b{bi}/br/rtl"), lang, format!("<p dir=\"rtl\">{br}</p>")));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    let mut out = Vec::new();
    for ws in ["0.5em", "0.6em", "1em"] {
        out.push(format!("p {{ word-spacing: {ws}; margin: 0 20% }}"));
    }
    for ls in ["0.25em", "0.5em"] {
        out.push(format!("p {{ letter-spacing: {ls}; text-align: justify; margin: 0 25% }}"));
    }
    out.push("p { letter-spacing: 0.2em; word-spacing: 0.4em; margin: 0 30% }".to_owned());
    out.push("body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }".to_owned());
    for m in ["34%", "42%"] {
        out.push(format!("body {{ font-family: sans-serif }} p {{ margin: 0 {m} }}"));
        out.push(format!("p {{ margin: 0 {m} }}"));
    }
    out.push("body { font-family: monospace } p { margin: 0 38% }".to_owned());
    out
}
