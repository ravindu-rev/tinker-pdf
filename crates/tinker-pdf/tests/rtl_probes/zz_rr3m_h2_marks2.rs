//! `zz_rr3m_h2_marks2` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_marks2.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family marks2.

use super::common::{rep, run_book, Case};

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

// Family marks2: nonspacing marks of scripts the earlier mark probes did
// not use (Devanagari, Thai, Syriac, Thaana, N'Ko, Hebrew cantillation,
// stacked Arabic harakat, Vietnamese, an enclosing mark), whole, styled
// apart from their letter, and as the overflow font's code-32 letter.

const MW: [(&str, &str, [&str; 6], char); 9] = [
    (
        "hi",
        "hi",
        ["नमस्ते", "दुनिया", "किताब", "बड़ा", "रोशनी", "शहर"],
        '\u{94D}',
    ),
    (
        "th",
        "th",
        ["สวัสดี", "หนังสือ", "บ้าน", "ใหญ่", "เมือง", "น้ำ"],
        '\u{E48}',
    ),
    (
        "syr",
        "syr",
        ["ܫܠܵܡܵܐ", "ܥܵܠܡܵܐ", "ܟܬܵܒܵܐ", "ܒܲܝܬܵܐ", "ܪܲܒܵܐ", "ܝܵܘܡܵܐ"],
        '\u{735}',
    ),
    (
        "dv",
        "dv",
        ["ދިވެހި", "ބަސް", "ފޮތް", "ބޮޑު", "އަލި", "ދުވަސް"],
        '\u{7A6}',
    ),
    (
        "nqo",
        "nqo",
        ["ߒߞߏ߫", "ߞߊ߲", "ߛߓߍ߬", "ߓߏ߲", "ߓߊ߫", "ߘߎ߲"],
        '\u{7EB}',
    ),
    (
        "heca",
        "he",
        ["בְּרֵאשִׁ֖ית", "בָּרָ֣א", "אֱלֹהִ֑ים", "אֵ֥ת", "הַשָּׁמַ֖יִם", "וְאֵ֥ת"],
        '\u{596}',
    ),
    (
        "arst",
        "ar",
        ["شَدَّةٌ", "مُحَمَّدٌ", "قُوَّةٌ", "رَبِّ", "حَقًّا", "سِرٌّ"],
        '\u{651}',
    ),
    (
        "vi",
        "vi",
        [
            "Vie\u{302}\u{323}t",
            "tie\u{302}\u{301}ng",
            "ngu\u{31B}\u{300}o\u{31B}\u{300}i",
            "nhu\u{31B}\u{303}ng",
            "đe\u{302}\u{300}u",
            "co\u{301}",
        ],
        '\u{323}',
    ),
    (
        "encl",
        "en",
        [
            "1\u{20DD}",
            "a\u{20DD}b",
            "x\u{20E3}",
            "\u{5D0}\u{20DD}",
            "ok\u{20DD}",
            "z",
        ],
        '\u{20DD}',
    ),
];

fn halves(w: &str) -> (String, String) {
    // Split after the first letter, so the second half opens with whatever
    // follows it (a mark, often).
    let mut it = w.char_indices();
    let _ = it.next();
    match it.next() {
        Some((k, _)) => (w[..k].to_owned(), w[k..].to_owned()),
        None => (w.to_owned(), String::new()),
    }
}

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w, mark) in MW {
        let s = |i: usize| w[i % w.len()];
        let (b0, b1) = halves(s(1));
        let shapes: Vec<(&str, String)> = vec![
            (
                "prose",
                rep(2, |i| {
                    format!(
                        "{} {} {}, {} {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4),
                        s(i + 5)
                    )
                }),
            ),
            (
                "split",
                format!(
                    "{} <b>{b0}</b>{b1} {} {}. {} <i>{}</i> {}.",
                    s(0),
                    s(2),
                    s(3),
                    s(4),
                    s(5),
                    s(0)
                ),
            ),
            (
                "bare",
                format!("{} {mark}{} {} {}.", s(0), s(1), s(2), s(3)),
            ),
        ];
        let positions: [(&str, String); 2] =
            [("nat", String::new()), ("mark", format!("<p>x{mark}</p>"))];
        for (pname, preface) in &positions {
            for (name, text) in &shapes {
                for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                    out.push((
                        format!("{sc}/{name}/{pname}/{d}"),
                        lang,
                        format!("{preface}<p{attr}>{text}</p>"),
                    ));
                }
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p { word-spacing: 0.6em; margin: 0 20% }",
        "p { word-spacing: 1.2em; letter-spacing: 2px; margin: 0 15% }",
        "p { text-align: justify; margin: 0 33% }",
        "body { font-family: sans-serif } p { letter-spacing: 0.15em; word-spacing: 0.5em; text-align: justify; margin: 0 28% }",
        "p { letter-spacing: -1px; word-spacing: 0.5em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
