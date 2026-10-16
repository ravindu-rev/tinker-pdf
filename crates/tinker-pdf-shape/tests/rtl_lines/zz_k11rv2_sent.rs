//! `zz_k11rv2_sent` (lines, K-11 review): the books of
//! `tools/probes/rtl/probes/shape/zz_k11rv2_sent.rs`, which is the probe as it ran.
//!
//! Round-2 review probe: ruling 14's reading of
//! wrapped sentences, typed-name and mirrored-name pages, both paragraph
//! directions. One row per line, written to $K11RV_OUT/sent.tsv.

use std::fmt::Write as _;
use tinker_pdf_shape::bidi::{logical_order, mirror, BaseDirection, Paragraph};

const AR: [&str; 6] = ["عوامل", "عدّة", "وقد", "أصبحنا", "نعرف", "الآن"];

const HE: [&str; 6] = ["שלום", "עולם", "מלחמה", "ספר", "בית", "גדול"];

const TEMPLATES: &[&str] = &[
    // Left-to-right-minded.
    "Intervals [0, 1) and (2, 3] hold {0} here.",
    "See f(x) = {0}(y) for details.",
    "The list: (a) {0}, (b) {1}, (c) {2}.",
    "The list: a) {0} b) {1} c) {2}.",
    "Item i) {0} ii) {1} (note) iii) end.",
    "Smile :-) and {0} ;-) then (wink) {1}.",
    "He said ({0}) :) and left (quickly).",
    "Wrapped gloss ({0} {1} {2} {3} {4}) ends here (see p. 3).",
    "Version 2.0) {0} (beta",
    "A {0}) B ({1} C",
    "Note ( {0} ) spaced (x).",
    "Code: if (x) {{ y(); }} and {0} (z).",
    "Chemistry (CH3)2 {0} (aq).",
    "Math (a+b)(c+d) = {0}.",
    "URL https://en.wikipedia.org/wiki/{0}_(disambiguation) here.",
    "Email {0} <a@b.c> (work) and <c@d.e> (home).",
    "Quotes \"{0}\" and '{1}' with ({2}).",
    "Smith (2010) and {0} (2011) say [12], [13].",
    "The word {0}) means peace (in Hebrew).",
    "1) {0} 2) {1} 3) {2}",
    "(1) {0} (2) {1} (3) {2}",
    "({0}) ({1}) ({2})",
    "{0} (x) {1} (y) {2}",
    "{0} [{1}] {2}",
    "Text with {0} (Fig. 2a) and {1} (Fig. 2b).",
    "He wrote: {0} ({1} {2}) and then ({3}).",
    "x){0}(y and a){1}(b",
    "See {0}(1) and {1}(2).",
    // Right-to-left-minded.
    "{0} {1} (Smith, 2010). {2} {3}.",
    "{0} (p. 23) {1} (pp. 4–5) {2}.",
    "{0} (e.g., HDAC1 and HDAC2) {1} {2}.",
    "{0} (٢٠١٠) {1}.",
    "{0} ((HDAC)) {1}.",
    "{0} {{HDAC}} {1}.",
    "{0} ⟨HDAC⟩ {1}.",
    "{0} 〈HDAC〉 {1}.",
    "{0} 〈HDAC〉 {1}.",
    "א) {0} ב) {1} ג) {2}.",
    "{0} :) {1} (: {2}",
    "{0} (HDAC) (SIRT) {1}.",
    "{0} (HDAC)(SIRT) {1}.",
    "{0} (HDAC [class I]) {1}.",
    "{0} [(HDAC)] {1}.",
    "{0} (HDAC) {1} (2010) {2} (see above) {3}.",
    "(HDAC) {0} {1}.",
    "{0} {1} (HDAC)",
    "{0} (HDAC. {1}",
    "HDAC) {0} {1}",
    "{0} (histone deacetylase inhibitors such as vorinostat) {1}.",
    "{0} \"(HDAC)\" {1}.",
    "{0} «(HDAC)» {1}.",
    "{0} (5%) {1} (-3.5) {2}.",
    "{0} ($1,234.56) {1}.",
    "{0} (2010–2015) {1}.",
    "{0} (HDAC). (SIRT). {1}.",
    "{0} HDAC) {1} (SIRT {2}.",
    "{0} (HDAC {1}) {2}.",
    "{0} ({1} HDAC) {2}.",
    "{0} (a) {1} (b) {2}.",
    "{0} f(x) {1}.",
    "{0} (x)(y) {1}.",
    "{0}(HDAC){1}",
    "{0} ( HDAC ) {1}.",
    "{0} {1} (HDAC). {2} {3} {4}. {5} (2010). {0} {1}.",
    "{0} (see [HDAC] here). {1} {2}. {3} (see [SIRT] there). {4} {5}.",
    "{0} {1} [12] {2} [13, 14].",
    "{0} - (HDAC) - {1}.",
    "{0}: (HDAC) {1}; (SIRT) {2}.",
];

fn fill(template: &str, words: &[&str; 6]) -> String {
    let mut out = template.to_owned();
    for (i, w) in words.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), w);
    }
    out.replace("{{", "{").replace("}}", "}")
}

fn wraps(text: &[char], width: usize) -> Vec<std::ops::Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    while start < text.len() {
        let mut end = (start + width).min(text.len());
        if end < text.len() {
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

fn zz_sentences() {
    let mut log = String::new();
    let (mut total, mut ok) = (0usize, 0usize);
    for (ti, template) in TEMPLATES.iter().enumerate() {
        for (script, words) in [("ar", &AR), ("he", &HE)] {
            let text = fill(template, words);
            let chars: Vec<char> = text.chars().collect();
            for direction in [BaseDirection::LeftToRight, BaseDirection::RightToLeft] {
                let paragraph = Paragraph::new(&text, direction);
                for width in [1000usize, 48, 40, 33, 28, 24, 20, 16, 13, 10, 8] {
                    for range in wraps(&chars, width) {
                        let line = paragraph.line(range.clone());
                        let levels = line.levels();
                        for mode in ["typed", "mirrored"] {
                            // The unit each drawn position holds, as a producer names it.
                            let named = |at: usize| -> String {
                                let c = chars[at];
                                if mode == "mirrored" {
                                    let level = levels[at - range.start];
                                    if let Some(m) = mirror(c, level) {
                                        return m.to_string();
                                    }
                                }
                                c.to_string()
                            };
                            let visual: Vec<String> =
                                line.visual_order().iter().map(|at| named(*at)).collect();
                            let visual_refs: Vec<&str> =
                                visual.iter().map(String::as_str).collect();
                            let order = logical_order(&visual_refs, BaseDirection::Auto);
                            let read: String = order.iter().map(|at| visual_refs[*at]).collect();
                            // What a reader that swaps nothing should return: the
                            // logical text as the producer named each glyph.
                            let want: String = range.clone().map(named).collect();
                            let good = read == want;
                            total += 1;
                            if good {
                                ok += 1;
                            }
                            let _ = writeln!(
                                log,
                                "t{ti}\t{script}\t{direction:?}\tw{width}\t{}..{}\t{mode}\t{}\t{want}\t=>\t{read}",
                                range.start,
                                range.end,
                                if good { "OK" } else { "NO" }
                            );
                        }
                    }
                }
            }
        }
    }
    crate::rtl_lines::ratchet::table("sent.tsv", log);
    emit!("SENT total={total} ok={ok}");
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    zz_sentences();
}
