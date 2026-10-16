//! `zz_rr3m_h2_r2ibword` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_r2ibword.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family r2ibword.

use super::common::{run_book, Case};

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

// Family ibword: breadth of N1, a word of a script the overflow font draws
// (Greek, Cyrillic, Armenian, IPA) and Latin as a control, cut by a box no
// spacing explains (an empty inline-block, a padded inline-block, a padded
// span, a margin), under word spacing.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    // (script, lang, the word's two halves, more words)
    let words: [(&str, &'static str, &str, &str, &str); 5] = [
        ("grk", "el", "αλφα", "βήτα", "γάμμα δέλτα"),
        ("cyr", "ru", "дома", "книга", "свет день"),
        ("arm", "hy", "գիրք", "տուն", "մեծ լույս"),
        ("ipa", "en", "ʃɪp", "θɪŋk", "ðɪs ʒɑ̃"),
        ("lat", "en", "abca", "defa", "ghi jkl"),
    ];
    let gaps: [(&str, &str, &str); 5] = [
        (
            "ib1",
            "<span style=\"display: inline-block; width: 1em\"></span>",
            "",
        ),
        (
            "ib06",
            "<span style=\"display: inline-block; width: 0.6em\"></span>",
            "",
        ),
        (
            "ib2",
            "<span style=\"display: inline-block; width: 2em\"></span>",
            "",
        ),
        (
            "ibpad",
            "<span style=\"display: inline-block; padding-left: 1em\">",
            "</span>",
        ),
        ("spanpad", "<span style=\"padding-left: 1em\">", "</span>"),
    ];
    for (sc, lang, a, b, more) in words {
        for (gn, open, close) in gaps {
            for (pn, pre) in [("first", ""), ("after", "x ")] {
                let text = format!("{pre}{a}{open}{b}{close} {more}.");
                for (d, attr) in [("none", ""), ("rtl", " dir=\"rtl\"")] {
                    out.push((
                        format!("{sc}/{gn}/{pn}/{d}"),
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
        "p { word-spacing: 0.3em }",
        "p { word-spacing: 0.6em }",
        "p { word-spacing: 1em }",
        "p { text-align: justify; margin: 0 35% }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em }",
        "body { font-family: monospace } p { word-spacing: 0.75em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
