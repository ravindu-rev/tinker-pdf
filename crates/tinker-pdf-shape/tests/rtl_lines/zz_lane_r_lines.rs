//! `zz_laneR_lines` (lines, lane R (round 5)): the books of
//! `tools/probes/rtl/probes/shape/zz_laneR_lines.rs`, which is the probe as it ran.
//!
//! Lane R line probe: K-11 finding V1. Wrapped
//! sentences whose lines end in the extension's new endings or open with
//! its marks, in both paragraph directions, typed and mirrored-name pages.
//! One row per line, written to $K11RV_OUT/laneR.tsv.

use std::fmt::Write as _;
use tinker_pdf_shape::bidi::{logical_order, mirror, BaseDirection, Paragraph};

const AR: [&str; 6] = ["عوامل", "عدّة", "وقد", "أصبحنا", "نعرف", "الآن"];

const HE: [&str; 6] = ["שלום", "עולם", "מלחמה", "ספר", "בית", "גדול"];

const ENDS: [&str; 20] = [
    "...",
    "?!",
    "!!",
    "!?",
    ".\"",
    ".\u{BB}",
    ".\u{201D}",
    "\u{2026}",
    ".'",
    "?\u{201D}",
    "!\u{BB}",
    "...\u{201D}",
    "\u{2024}",
    "\u{2025}",
    "\u{FE19}",
    "..",
    "??",
    ".)",
    "\u{201D}",
    ".",
];

const MARKS: [&str; 21] = [
    "\u{2026}",
    "...",
    "..",
    "\u{2024}",
    "\u{2025}",
    "\u{FE19}",
    "?!",
    "!!",
    "!?",
    "??",
    "\".",
    "\u{201D}.",
    "\u{BB}.",
    "'.",
    "\u{201D}\u{2026}",
    "\"...",
    ".",
    "?",
    "\u{60C}",
    "\u{201D}",
    "\"",
];

const SHAPES: [&str; 8] = [
    // ending a sentence after a pair (the fix's shape, dir-less)
    "{0} {1} {2} ({3}) {4}{E} {5} {0} ({1}) {2}{E}",
    "{0} {1} [{2}] {3}{E} {4} {5} {0} {1}{E}",
    "{0} ({1}) {2} {3} ({4}) {5}{E}",
    "Latin {0} {1} ({2}) {3}{E}",
    // a mark opening the paragraph, and after a space
    "{M}({0}) {1} {2} {3}.",
    "{M}[{0}] {1} {2}.",
    "{0} {1} {2} {M}({3}) {4} {5}.",
    "{0} {1} {M}({2}) {3} ({4}) {5}{M}",
];

fn fill(template: &str, words: &[&str; 6], e: &str, m: &str) -> String {
    let mut out = template.to_owned();
    for (i, w) in words.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), w);
    }
    out.replace("{E}", e).replace("{M}", m)
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

fn zz_lane_r_lines() {
    let mut log = String::new();
    let (mut total, mut ok) = (0usize, 0usize);
    for (si, shape) in SHAPES.iter().enumerate() {
        let variants: Vec<(String, String)> = if shape.contains("{E}") {
            ENDS.iter()
                .map(|e| (format!("e{}", e.escape_unicode()), fill(shape, &AR, e, "")))
                .collect()
        } else {
            MARKS
                .iter()
                .map(|m| (format!("m{}", m.escape_unicode()), fill(shape, &AR, "", m)))
                .collect()
        };
        let he: Vec<String> = if shape.contains("{E}") {
            ENDS.iter().map(|e| fill(shape, &HE, e, "")).collect()
        } else {
            MARKS.iter().map(|m| fill(shape, &HE, "", m)).collect()
        };
        for ((tag, ar_text), he_text) in variants.iter().zip(he.iter()) {
            for (script, text) in [("ar", ar_text), ("he", he_text)] {
                let chars: Vec<char> = text.chars().collect();
                for direction in [BaseDirection::LeftToRight, BaseDirection::RightToLeft] {
                    let paragraph = Paragraph::new(text, direction);
                    for width in [1000usize, 48, 40, 33, 28, 24, 20, 16, 13, 10, 8] {
                        for range in wraps(&chars, width) {
                            let line = paragraph.line(range.clone());
                            let levels = line.levels();
                            for mode in ["typed", "mirrored"] {
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
                                let refs: Vec<&str> = visual.iter().map(String::as_str).collect();
                                let order = logical_order(&refs, BaseDirection::Auto);
                                let read: String = order.iter().map(|at| refs[*at]).collect();
                                let want: String = range.clone().map(named).collect();
                                let good = read == want;
                                total += 1;
                                if good {
                                    ok += 1;
                                }
                                let _ = writeln!(
                                    log,
                                    "s{si}{tag}\t{script}\t{direction:?}\tw{width}\t{}..{}\t{mode}\t{}\t{want}\t=>\t{read}",
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
    }
    crate::rtl_lines::ratchet::table("laneR.tsv", log);
    emit!("LANER total={total} ok={ok}");
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    zz_lane_r_lines();
}
