//! `zz_v6_lists` (v6, round 6 lane V (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_v6_lists.rs`, which is the probe as it ran.

use super::common::{AR, Case, HE, HEP, run_book};

fn probe() {
    let mut n = 0usize;
    for style in styles() {
        for (name, lang, body) in cases() {
            n += 1;
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_book(&style, &name, lang, &body)
            }));
            if r.is_err() {
                emit!("ROW\t{style}\t{name}\tm=0\ttext=0\tsearch=0/1\tltr=-\tcons=0\tlogical=0\tPANIC");
                emit!("PANIC\t{style}\t{name}");
            }
        }
    }
    emit!("books={n}");
}

/// Every book of the family, as its probe built them, in order.
pub(crate) fn run() {
    probe();
}

// Family lists: ordered and unordered lists (markers outside and inside, two-
// digit and Roman and alphabetic markers, nested), definition lists, figures
// with their caption after and before, generated content before and after a
// paragraph (a Latin sign, an Arabic star, a counter), <q> with the stylesheet's
// quotes, blockquotes and a heading. Every one of these draws a run the
// painter generates, or a box beside the text, on the line a reader reads.

const FA: [&str; 12] = ["کتاب", "خانه", "بزرگ", "ایران", "می\u{200C}خواهم", "دانشگاه", "تهران", "سال", "روز", "شهر", "کشور", "نمی\u{200C}دانم"];

const GEN: &str = " .b::before { content: \"\u{a7} \" } .a::after { content: \" (*)\" } \
    .s::before { content: \"\u{66d} \" } .c { counter-increment: n } .c::before { content: counter(n) \". \" } \
    q { quotes: \"\u{ab}\" \"\u{bb}\" }";

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE), ("hep", "he", HEP), ("fa", "fa", FA)] {
        let s = |i: usize| w[i % w.len()];
        let sent = |i: usize| format!("{} {} <b>{}</b> {}، {} ({}) {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5), s(i + 6));
        let shapes: Vec<(&str, String)> = vec![
            ("ol", format!("<ol{{D}}><li>{}</li><li>{}</li></ol>", sent(0), sent(3))),
            ("ul", format!("<ul{{D}}><li>{}</li><li>{}</li></ul>", sent(1), sent(4))),
            ("ol-inside", format!("<ol{{D}} style=\"list-style-position: inside\"><li>{}</li><li>{}</li></ol>", sent(0), sent(5))),
            ("ol-start", format!("<ol{{D}} start=\"9\"><li>{}</li><li>{}</li></ol>", sent(2), sent(6))),
            ("ol-roman", format!("<ol{{D}} style=\"list-style-type: upper-roman\"><li>{}</li><li>{}</li></ol>", sent(1), sent(7))),
            ("ol-alpha", format!("<ol{{D}} style=\"list-style-type: lower-alpha\"><li>{}</li><li>{}</li></ol>", sent(3), sent(8))),
            ("ol-nested", format!("<ol{{D}}><li>{}<ol><li>{}</li></ol></li></ol>", sent(0), sent(4))),
            ("dl", format!("<dl{{D}}><dt>{} {}</dt><dd>{}</dd></dl>", s(1), s(2), sent(3))),
            ("figure", format!("<figure{{D}}><div style=\"height: 2em; border: 1px solid black\"></div><figcaption>{}</figcaption></figure>", sent(2))),
            ("figure-first", format!("<figure{{D}}><figcaption>{}</figcaption><div style=\"height: 2em; border: 1px solid black\"></div></figure>", sent(5))),
            ("before", format!("<p{{D}} class=\"b\">{}</p>", sent(0))),
            ("after", format!("<p{{D}} class=\"a\">{}</p>", sent(1))),
            ("star", format!("<p{{D}} class=\"s\">{}</p>", sent(2))),
            ("counter", format!("<div{{D}}><p class=\"c\">{}</p><p class=\"c\">{}</p></div>", sent(3), sent(4))),
            ("q", format!("<p{{D}}>{} <q>{} {}</q> {}، {}.</p>", s(0), s(1), s(2), s(3), s(4))),
            ("blockquote", format!("<blockquote{{D}}>{}</blockquote>", sent(6))),
            ("h2", format!("<div{{D}}><h2>{} {}</h2><p>{}</p></div>", s(5), s(6), sent(7))),
        ];
        for (name, text) in &shapes {
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((format!("{sc}/{name}/{d}"), lang, text.replace("{D}", attr)));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    let t = "p, li, dt, dd, figcaption, blockquote, h2";
    [
        format!("{t} {{ margin-right: 15% }}"),
        format!("{t} {{ word-spacing: 0.6em }}"),
        format!("{t} {{ word-spacing: 1em }}"),
        format!("{t} {{ text-align: justify }}"),
        format!("{t} {{ letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify }}"),
        format!("{t} {{ letter-spacing: 0.5em }}"),
    ]
    .iter()
    .map(|s| format!("{s}{GEN}"))
    .collect()
}
