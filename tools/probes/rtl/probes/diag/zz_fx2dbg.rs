//! Fixer r2 debug probe (scratch, never committed): the books in DBG_FILE
//! (`name<TAB>lang<TAB>style<TAB>body`), each with its verdicts, plain and
//! logical text, and with DBG_LINES its content-order lines and glyph boxes,
//! with DBG_CS its content stream's text operators.
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
        "V\t{tag}\tc{}l{}w{}s{} ov={}/{} miss={:?}\tplain={}\tlog={}",
        v.cons as u8, v.logical as u8, v.whole as u8, v.search as u8, v.overlap, v.vov, v.missing, v.plain, v.logical_text
    );
}

#[test]
fn dbg() {
    let file = std::env::var("DBG_FILE").unwrap_or_default();
    if file.is_empty() {
        return;
    }
    let all = std::fs::read_to_string(&file).expect("DBG_FILE");
    for row in all.lines() {
        let p: Vec<&str> = row.split('\t').collect();
        if p.len() < 4 || row.starts_with('#') {
            continue;
        }
        let (name, lang, style, body) = (p[0], p[1], p[2], p[3]);
        let v = verdict(lang, style, body);
        show(name, &v);
        if std::env::var("DBG_LINES").is_ok() || std::env::var("DBG_CS").is_ok() {
            let bytes = styled_book(lang, style, body);
            let doc = Document::open(bytes.clone()).expect("opens");
            for at in 0..doc.page_count() {
                let page = doc.page(at).expect("page");
                if std::env::var("DBG_LINES").is_ok() {
                    let drawn = page.text_with(&TextOptions { content_order: true });
                    for (i, line) in drawn.lines().iter().enumerate() {
                        let mut s = String::new();
                        for c in &line.chars {
                            let (x0, _, x1, _) = c.quad.bounds();
                            s.push_str(&format!(" [{}]{:.2}-{:.2}", c.text, x0, x1));
                        }
                        println!("LINE{i}@{:.1}\t{s}", line.chars.first().map_or(0.0, |c| c.origin.1));
                    }
                }
            }
            if std::env::var("DBG_CS").is_ok() {
                let cos = doc.cos();
                for page in tinker_pdf_cos::pages::collect(cos) {
                    let b = tinker_pdf_cos::pages::content_bytes(cos, &page);
                    let s = String::from_utf8_lossy(&b);
                    let keep: Vec<&str> = s
                        .lines()
                        .filter(|l| l.contains("Tj") || l.contains("TJ") || l.contains("Tf") || l.contains("BT") || l.contains("ET") || l.contains(" Td"))
                        .collect();
                    println!("CS\n{}", keep.join("\n"));
                }
            }
        }
    }
}
