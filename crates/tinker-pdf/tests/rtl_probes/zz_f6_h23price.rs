//! `zz_f6_h23price` (f6pdf, round 6 lane F (owner options)): the books of
//! `tools/probes/rtl/probes/pdf/zz_f6_h23price.rs`, which is the probe as it
//! ran.
//!
//! Plain synthetic PDFs, not EPUB: two words of one script, each its own text
//! object, on one baseline a gap apart with no space glyph between them (two
//! words a producer positions apart, two table cells with a rule between
//! them), at gaps from 0.3em to 2em, in Greek, Cyrillic, Armenian and Latin.
//! What a reader reads there is the price of answering H2-3 (ROADMAP CD-19's
//! Left (2)) in the reader rather than the writer.

use super::common::{esc, hash};
use std::fmt::Write as _;
use tinker_pdf::Document;
use tinker_pdf_cos::build::DocumentBuilder;

fn names_of(words: &[&str]) -> Vec<char> {
    let mut out: Vec<char> = Vec::new();
    for w in words {
        for c in w.chars() {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every book of the family, as its probe built them, in order.
pub(crate) fn run() {
    let scripts: [(&str, [&str; 2]); 4] = [
        (
            "grk",
            [
                "\u{3CC}\u{3BD}\u{3BF}\u{3BC}\u{3B1}",
                "\u{3B5}\u{3C0}\u{3CE}\u{3BD}\u{3C5}\u{3BC}\u{3BF}",
            ],
        ),
        (
            "cyr",
            [
                "\u{43C}\u{43E}\u{441}\u{43A}\u{432}\u{430}",
                "\u{440}\u{43E}\u{441}\u{441}\u{438}\u{44F}",
            ],
        ),
        (
            "arm",
            [
                "\u{563}\u{56B}\u{580}\u{584}",
                "\u{57F}\u{578}\u{582}\u{576}",
            ],
        ),
        ("lat", ["name", "surname"]),
    ];
    let size = 10.0f64;
    let width = 500u16;
    let mut n = 0;
    for (script, words) in scripts {
        let chars = names_of(&words);
        let names: Vec<String> = chars
            .iter()
            .map(|c| format!("uni{:04X}", *c as u32))
            .collect();
        let names_ref: Vec<&str> = names.iter().map(String::as_str).collect();
        let widths: Vec<u16> = chars.iter().map(|_| width).collect();
        let code = |c: char| 32 + chars.iter().position(|x| *x == c).unwrap() as u8;
        let string = |w: &str| {
            let mut s = String::new();
            for c in w.chars() {
                let b = code(c);
                match b {
                    b'(' | b')' | b'\\' => {
                        s.push('\\');
                        s.push(b as char);
                    }
                    32..=126 => s.push(b as char),
                    _ => {
                        let _ = write!(s, "\\{b:03o}");
                    }
                }
            }
            s
        };
        for gap in [0.3f64, 0.5, 0.6, 0.8, 1.0, 1.2, 1.3, 2.0] {
            for shape in ["objects", "cells-tm"] {
                let x0 = 72.0;
                let w0 = words[0].chars().count() as f64 * size * f64::from(width) / 1000.0;
                let x1 = x0 + w0 + gap * size;
                let mut c = String::new();
                match shape {
                    // Two text objects, each word at its own place.
                    "objects" => {
                        let _ = writeln!(
                            c,
                            "BT /F0 {size} Tf {x0} 700 Td ({}) Tj ET",
                            string(words[0])
                        );
                        let _ = writeln!(
                            c,
                            "BT /F0 {size} Tf {x1:.3} 700 Td ({}) Tj ET",
                            string(words[1])
                        );
                    }
                    // Two cells of a table row: a rule between them, each cell's text its own object.
                    _ => {
                        let _ = writeln!(
                            c,
                            "BT /F0 {size} Tf 1 0 0 1 {x0} 700 Tm ({}) Tj ET",
                            string(words[0])
                        );
                        let mid = x0 + w0 + gap * size / 2.0;
                        let _ = writeln!(c, "{mid:.3} 695 m {mid:.3} 712 l S");
                        let _ = writeln!(
                            c,
                            "BT /F0 {size} Tf 1 0 0 1 {x1:.3} 700 Tm ({}) Tj ET",
                            string(words[1])
                        );
                    }
                }
                let mut builder = DocumentBuilder::new();
                assert!(builder.add_named_font(b"F0", b"Helvetica", 32, &names_ref, &widths));
                builder.add_page(612.0, 792.0, |page| page.raw(c.as_bytes()));
                let doc = Document::open(builder.finish()).expect("opens");
                let page = doc.page(0).expect("a page");
                let text = page.text().plain_text();
                let t = page.text();
                let found = words.iter().filter(|w| !t.search(w).is_empty()).count();
                let glued = format!("{}{}", words[0], words[1]);
                let read = if text.contains(&glued) {
                    "glued"
                } else if found == 2 {
                    "apart"
                } else {
                    "other"
                };
                let want = format!("{} {}", words[0], words[1]);
                let got = norm(&text);
                let whole = got.contains(&want) as u8;
                let same = (got == want) as u8;
                emit!(
                    "ROW\t{shape}@{gap}em\t{script}\tm=0\ttext={whole}\tsearch={found}/2\tcons={same}\tlogical={same}\tread={read}\tthash={:016x}\t{}",
                    hash(text.as_bytes()),
                    esc(&text)
                );
                n += 1;
            }
        }
    }
    emit!("books={n}");
}
