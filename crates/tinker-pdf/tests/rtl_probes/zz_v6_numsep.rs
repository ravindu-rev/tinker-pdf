//! `zz_v6_numsep` (v6, round 6 lane V (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_v6_numsep.rs`, which is the probe as it ran.

use super::common::{rep, run_book, Case, AR, HE};

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

// Family numsep: numbers with every separator no probe has set: thin space,
// NARROW NO-BREAK SPACE and NO-BREAK SPACE between thousands, an apostrophe
// and a MODIFIER LETTER APOSTROPHE, a middle dot, FRACTION SLASH, superscript
// digits, a vulgar fraction, fullwidth digits, circled digits, NUMERO SIGN,
// degrees, multiplication and division signs, ratios, a time range, a version,
// an address, an ISBN, an exponent, a hash, the ARABIC SEMICOLON and comma
// between digits, the European grouping `1.000.000,50`, and a sign either side.
// Each mid-sentence, in bold, and at the line's end.

const NUMS: [(&str, &str); 32] = [
    ("thin", "1\u{2009}234\u{2009}567"),
    ("nnbsp", "1\u{202F}234"),
    ("nbsp", "1\u{A0}234"),
    ("apos", "1'234'567"),
    ("modapos", "1\u{2BC}234"),
    ("middot", "3\u{B7}14"),
    ("fracslash", "1\u{2044}2"),
    ("sup", "10\u{B2}"),
    ("vulgar", "2\u{BD}"),
    ("fullwidth", "\u{FF11}\u{FF12}\u{FF13}"),
    ("circled", "\u{2460}\u{2461}"),
    ("numero", "\u{2116}5"),
    ("degc", "-5\u{B0}C"),
    ("times", "3\u{D7}4"),
    ("divide", "8\u{F7}2"),
    ("ratio", "1:2:3"),
    ("timerange", "10:30\u{2013}11:45"),
    ("version", "v2.0.1"),
    ("ip", "192.168.0.1"),
    ("isbn", "978-3-16-148410-0"),
    ("exp", "1e-3"),
    ("hash", "#12"),
    ("arsemi", "\u{661}\u{61B}\u{662}"),
    ("arcomma", "\u{661}\u{60C}\u{662}"),
    ("euro-group", "1.000.000,50"),
    (
        "an-group",
        "\u{661}\u{66C}\u{660}\u{660}\u{660}\u{66C}\u{660}\u{660}\u{660}",
    ),
    ("sign-after", "\u{665}-"),
    ("sign-before", "-\u{665}"),
    ("pct-space", "50 %"),
    ("shekel-space", "\u{20AA} 100"),
    ("fa-dec", "\u{6F3}\u{66B}\u{6F1}\u{6F4}"),
    ("pm", "\u{B1}0.5"),
];

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        for (nn, n) in NUMS {
            let shapes: Vec<(&str, String)> = vec![
                (
                    "mid",
                    rep(2, |i| {
                        format!("{} {} {n} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                    }),
                ),
                (
                    "bold",
                    format!("{} {} <b>{n}</b> {}، {} {}.", s(0), s(1), s(2), s(3), s(4)),
                ),
                (
                    "end",
                    format!("{} {} {} {} {} {n}", s(5), s(6), s(7), s(8), s(9)),
                ),
            ];
            for (name, text) in shapes {
                for (d, attr) in [
                    ("rtl", " dir=\"rtl\""),
                    ("ltr", " dir=\"ltr\""),
                    ("none", ""),
                ] {
                    out.push((
                        format!("{sc}/{nn}/{name}/{d}"),
                        lang,
                        format!("<p{attr}>{text}</p>"),
                    ));
                }
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
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
