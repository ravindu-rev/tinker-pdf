//! `zz_rr3m_h2_r2lvl2` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_r2lvl2.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family r2lvl2.

use super::common::{run_book, Case, AR, HE};

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

// Family lvl2: left-to-right text the overflow font draws (Greek, Cyrillic,
// Armenian, IPA, CJK, Persian digits) at an even level inside right-to-left
// text, which joins_its_inside leaves a text object per piece, in runs of one
// to four words; a span with dir="ltr" holding a right-to-left word (level 3)
// and <bdi>; under spacing that parts text objects.

const GRK: [&str; 4] = ["αλφα", "βήτα", "γάμμα", "δέλτα"];

const CYR: [&str; 4] = ["книга", "дом", "свет", "день"];

const ARM: [&str; 4] = ["գիրք", "տուն", "մեծ", "լույս"];

const IPA: [&str; 4] = ["ʃɪp", "θɪŋk", "ðɪs", "ʒɑ̃"];

const CJK: [&str; 4] = ["中文", "字符", "测试", "书籍"];

const FAD: [&str; 4] = ["۱۲۳", "۴۵", "۲۰۱۰", "۷۸۹"];

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let ltrs: [(&str, [&str; 4]); 6] = [
        ("grk", GRK),
        ("cyr", CYR),
        ("arm", ARM),
        ("ipa", IPA),
        ("cjk", CJK),
        ("fad", FAD),
    ];
    for (sc, lang, w) in [("he", "he", HE), ("ar", "ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        for (ln, l) in ltrs {
            let shapes: Vec<(&str, String)> = vec![
                ("one", format!("{} {} {} {}.", s(0), l[0], s(1), s(2))),
                (
                    "two",
                    format!("{} {} {} {} {}.", s(0), l[0], l[1], s(1), s(2)),
                ),
                (
                    "four",
                    format!("{} {} {} {} {}, {}.", s(0), l[0], l[1], l[2], l[3], s(1)),
                ),
                (
                    "styled",
                    format!(
                        "{} <b>{}</b> {} <i>{} {}</i> {}.",
                        s(0),
                        l[0],
                        l[1],
                        l[2],
                        s(1),
                        s(2)
                    ),
                ),
                ("paren", format!("{} ({} {}) {}.", s(0), l[0], l[1], s(1))),
                (
                    "spanltr",
                    format!(
                        "{} <span dir=\"ltr\">{} {} {}</span> {}.",
                        s(0),
                        l[0],
                        s(3),
                        l[1],
                        s(1)
                    ),
                ),
                (
                    "bdi",
                    format!("{} <bdi>{} {}</bdi> {}.", s(0), l[2], l[3], s(1)),
                ),
            ];
            for (name, text) in shapes {
                for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                    out.push((
                        format!("{sc}/{ln}/{name}/{d}"),
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
        "p { word-spacing: 0.6em }",
        "p { word-spacing: 1em; margin: 0 20% }",
        "p { word-spacing: 2em; margin: 0 10% }",
        "p { text-align: justify; margin: 0 38% }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em }",
        "p { letter-spacing: 0.5em }",
        "body { font-family: monospace } p { word-spacing: 0.75em; text-align: justify; margin: 0 22% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
