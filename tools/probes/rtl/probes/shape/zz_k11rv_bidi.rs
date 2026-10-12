//! Scratch re-review probe (never committed): ruling 14's reading over
//! BidiCharacterTest.txt with the paragraph direction read off the line
//! (`BaseDirection::Auto`), and over realistic wrapped sentences.
#![allow(missing_docs, clippy::all)]

use std::fmt::Write as _;
use tinker_pdf_shape::bidi::{logical_order, order_units, BaseDirection, Paragraph};
use tinker_pdf_shape::unicode::{bidi_class, bracket};

const CHARACTER_TEST: &str = include_str!("../data/ucd/BidiCharacterTest.txt");

fn out_path(name: &str) -> String {
    let dir = std::env::var("K11RV_OUT").unwrap_or_else(|_| "/tmp".to_owned());
    format!("{dir}/{name}")
}

/// Whether every bracket of `text` pairs as typed (BD16, simplified).
fn balanced(text: &str) -> bool {
    let mut stack: Vec<char> = Vec::new();
    let mut count = 0;
    let mut pairs = 0;
    for c in text.chars() {
        if let Some((paired, kind)) = bracket(c) {
            count += 1;
            match kind {
                tinker_pdf_shape::unicode::BracketKind::Open => stack.push(paired),
                tinker_pdf_shape::unicode::BracketKind::Close => {
                    if let Some(at) = stack.iter().rposition(|want| *want == c) {
                        pairs += 1;
                        stack.truncate(at);
                    }
                }
            }
        }
    }
    pairs * 2 == count
}

#[test]
fn zz_bct_auto() {
    let mut log = String::new();
    let (mut ran, mut typed, mut drawn) = (0usize, 0usize, 0usize);
    for (number, raw) in CHARACTER_TEST.lines().enumerate() {
        let body = match raw.find('#') {
            Some(0) => continue,
            Some(at) => &raw[..at],
            None => raw,
        }
        .trim();
        if body.is_empty() {
            continue;
        }
        let fields: Vec<&str> = body.split(';').collect();
        let chars: Vec<char> = fields[0]
            .split_whitespace()
            .map(|h| char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap())
            .collect();
        if chars.iter().any(|c| bidi_class(*c).is_removed_by_x9()) {
            continue;
        }
        let level = if fields[2].trim() == "0" {
            BaseDirection::LeftToRight
        } else {
            BaseDirection::RightToLeft
        };
        let units: Vec<String> = chars.iter().map(char::to_string).collect();
        let visual: Vec<&str> = fields[4]
            .split_whitespace()
            .map(|t| units[t.parse::<usize>().unwrap()].as_str())
            .collect();
        ran += 1;
        let order = logical_order(&visual, BaseDirection::Auto);
        let read: Vec<&str> = order.iter().map(|at| visual[*at]).collect();
        let text: String = chars.iter().collect();
        let ok = read.concat() == text;
        if ok {
            typed += 1;
        }
        // Does the reading draw the line in either direction?
        let mut draws = false;
        for d in [BaseDirection::LeftToRight, BaseDirection::RightToLeft] {
            let redrawn: Vec<&str> = order_units(&read, d).into_iter().map(|at| read[at]).collect();
            if redrawn == visual {
                draws = true;
            }
        }
        if draws {
            drawn += 1;
        }
        let _ = writeln!(
            log,
            "{}\t{}\t{}\t{}\t{:?}\t{}",
            number + 1,
            if ok { "OK" } else { "NO" },
            if balanced(&text) { "bal" } else { "unbal" },
            if draws { "draws" } else { "undrawn" },
            level,
            read.concat().escape_unicode()
        );
    }
    std::fs::write(out_path("bct-auto.tsv"), log).unwrap();
    println!("BCT-AUTO ran={ran} typed={typed} drawn={drawn}");
}

const AR: [&str; 6] = ["عوامل", "عدّة", "وقد", "أصبحنا", "نعرف", "الآن"];
const HE: [&str; 6] = ["שלום", "עולם", "מלחמה", "ספר", "בית", "גדול"];

fn templates() -> Vec<(&'static str, BaseDirection, String)> {
    let a = |i: usize| AR[i % AR.len()];
    let h = |i: usize| HE[i % HE.len()];
    let mut t: Vec<(&'static str, BaseDirection, String)> = Vec::new();
    use BaseDirection::{LeftToRight as L, RightToLeft as R};
    for (script, w) in [("ar", &a as &dyn Fn(usize) -> &'static str), ("he", &h)] {
        let w0 = w(0);
        let w1 = w(1);
        let w2 = w(2);
        let w3 = w(3);
        let w4 = w(4);
        let w5 = w(5);
        let _ = script;
        // Right-to-left paragraphs.
        t.push(("rtl-latin-acronym", R, format!("{w0} {w1} (HDAC). {w2} {w3} {w4} {w5}.")));
        t.push(("rtl-latin-words", R, format!("{w0} {w1} (histone deacetylase). {w2} {w3} {w4}.")));
        t.push(("rtl-year", R, format!("{w0} {w1} (2010). {w2} {w3}.")));
        t.push(("rtl-rtl-inside", R, format!("{w0} ({w1} {w2}). {w3} {w4}.")));
        t.push(("rtl-nested", R, format!("{w0} ({w1} [HDAC] {w2}). {w3}.")));
        t.push(("rtl-nested-latin", R, format!("{w0} (see [HDAC] here). {w3}.")));
        t.push(("rtl-quote-latin", R, format!("{w0} \"HDAC\" {w1}. {w2}.")));
        t.push(("rtl-guillemets", R, format!("{w0} «HDAC» {w1}. {w2}.")));
        t.push(("rtl-curly", R, format!("{w0} “histone deacetylase” {w1}. {w2}.")));
        t.push(("rtl-number-sep", R, format!("{w0} (1,234.56) {w1} {w2}.")));
        t.push(("rtl-number-sep-latin", R, format!("{w0} (USD 1,234.56) {w1} {w2}.")));
        t.push(("rtl-bracket-ends-para", R, format!("{w0} {w1} {w2} (HDAC)")));
        t.push(("rtl-bracket-opens-para", R, format!("(HDAC) {w0} {w1} {w2}.")));
        t.push(("rtl-two-pairs", R, format!("{w0} (HDAC) {w1} (SIRT) {w2}.")));
        t.push(("rtl-list", R, format!("1) {w0} {w1} 2) {w2} (HDAC) {w3}.")));
        t.push(("rtl-list-letter", R, format!("{w0} a) HDAC b) SIRT {w1}.")));
        t.push(("rtl-smiley", R, format!("{w0} :) {w1} (HDAC) {w2}.")));
        t.push(("rtl-latin-long", R, format!("{w0} (the histone deacetylase family of enzymes) {w1} {w2}.")));
        t.push(("rtl-latin-paren-in-latin", R, format!("{w0} HDAC (class I) inhibitors {w1} {w2}.")));
        t.push(("rtl-brackets-square", R, format!("{w0} [HDAC] {w1} {w2}.")));
        t.push(("rtl-ref", R, format!("{w0} {w1} [12] {w2} [13, 14].")));
        t.push(("rtl-ref-latin", R, format!("{w0} HDAC [12] {w2} [13].")));
        // Left-to-right paragraphs quoting right-to-left text.
        t.push(("ltr-quote-rtl", L, format!("The word ({w0}) means peace.")));
        t.push(("ltr-two-quotes", L, format!("Peace ({w0}) and war ({w1}) here.")));
        t.push(("ltr-rtl-bracketed-run", L, format!("He wrote \"{w0} ({w1} {w2})\" yesterday.")));
        t.push(("ltr-rtl-bracketed-latin", L, format!("He wrote {w0} {w1} (HDAC). {w2} {w3} today.")));
        t.push(("ltr-list", L, format!("Step 1) open the file ({w0}) and 2) close it.")));
        t.push(("ltr-list-rtl", L, format!("a) {w0} {w1} b) {w2} (see {w3}) c) end.")));
        t.push(("ltr-smiley", L, format!("Great :) {w0} (yes) {w1}.")));
        t.push(("ltr-ref", L, format!("As {w0} said [12], {w1} [13].")));
        t.push(("ltr-in-arabic", L, format!("The term (in Arabic: {w0} {w1}) means peace (see below).")));
        t.push(("ltr-nested", L, format!("Text ({w0} [{w1}] {w2}) more.")));
        t.push(("ltr-rtl-ends", L, format!("{w0} {w1} (HDAC). {w2} {w3}")));
        t.push(("ltr-rtl-start-latin-end", L, format!("{w0} (HDAC) then more text.")));
        t.push(("ltr-guillemets", L, format!("He said «{w0} ({w1})» then.")));
        t.push(("ltr-number", L, format!("Price ({w0}: 1,234.56) and ({w1}: 7.5).")));
        t.push(("ltr-two-glosses-multi", L, format!("Peace ({w0} {w1}) and war ({w2} {w3} {w4}) are words in the text.")));
        t.push(("ltr-two-glosses-multi-b", L, format!("The words peace ({w0} {w1} {w2}) and war ({w3} {w4} {w5}) are glossed here.")));
        t.push(("ltr-gloss-then-ref", L, format!("See the word ({w0}) and the note (page {w1} {w2}) below.")));
        t.push(("ltr-three-glosses", L, format!("A ({w0}) B ({w1}) C ({w2} {w3}) D.")));
        t.push(("rtl-two-latin-multi", R, format!("{w0} (histone deacetylase) {w1} (sirtuin family) {w2}.")));
    }
    t
}

/// Every way of breaking `text` into lines of at most `width` characters at
/// spaces, for a handful of widths; each line's character range.
fn wraps(text: &[char], width: usize) -> Vec<std::ops::Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    while start < text.len() {
        let mut end = (start + width).min(text.len());
        if end < text.len() {
            // Back up to a space.
            if let Some(space) = (start + 1..end).rev().find(|at| text[*at] == ' ') {
                end = space;
            }
        }
        lines.push(start..end);
        start = end;
        while start < text.len() && text[start] == ' ' {
            start += 1;
        }
    }
    lines
}

#[test]
fn zz_wrapped_sentences() {
    let mut log = String::new();
    let (mut total, mut ok) = (0usize, 0usize);
    for (name, direction, text) in templates() {
        let chars: Vec<char> = text.chars().collect();
        let paragraph = Paragraph::new(&text, direction);
        for width in [1000usize, 40, 32, 26, 20, 16, 12, 9, 7] {
            for range in wraps(&chars, width) {
                let line = paragraph.line(range.clone());
                let units: Vec<String> = chars.iter().map(char::to_string).collect();
                let visual: Vec<&str> = line
                    .visual_order()
                    .iter()
                    .map(|at| units[*at].as_str())
                    .collect();
                let order = logical_order(&visual, BaseDirection::Auto);
                let read: String = order.iter().map(|at| visual[*at]).collect();
                let typed: String = chars[range.clone()].iter().collect();
                let good = read == typed;
                total += 1;
                if good {
                    ok += 1;
                }
                let _ = writeln!(
                    log,
                    "{name}\t{direction:?}\tw{width}\t{}..{}\t{}\t{typed}\t=>\t{read}",
                    range.start,
                    range.end,
                    if good { "OK" } else { "NO" }
                );
            }
        }
    }
    std::fs::write(out_path("wrapped.tsv"), log).unwrap();
    println!("WRAPPED total={total} ok={ok}");
}
