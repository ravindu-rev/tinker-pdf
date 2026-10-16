//! `zz_rr3m_h2_r2gloss` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_r2gloss.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family r2gloss.

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

// Family gloss: breadth of N5, a left-to-right paragraph (dir-less or ltr)
// whose left-to-right words (Latin, Greek, Cyrillic, Armenian) are followed,
// across a stretched word space, by bracketed right-to-left words (Hebrew,
// Arabic), the word before them styled or not, with or without text after.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let ltrs: [(&str, [&str; 3]); 4] = [
        ("lat", ["abc", "def", "xyz"]),
        ("grk", ["αλφα", "βήτα", "γάμμα"]),
        ("cyr", ["книга", "дом", "свет"]),
        ("arm", ["գիրք", "տուն", "մեծ"]),
    ];
    let rtls: [(&str, &'static str, [&str; 2]); 2] =
        [("he", "he", ["ספר", "בית"]), ("ar", "ar", ["كتاب", "بيت"])];
    let brackets: [(&str, &str, &str); 3] =
        [("paren", "(", ")"), ("sq", "[", "]"), ("bare", "", "")];
    for (ln, l) in ltrs {
        for (rn, lang, r) in rtls {
            for (bn, o, c) in brackets {
                for (sn, styled) in [("b", true), ("plain", false)] {
                    for (tn, tail) in [("end", ""), ("tail", " xyz.")] {
                        let w1 = if styled {
                            format!("<b>{}</b>", l[1])
                        } else {
                            l[1].to_owned()
                        };
                        let text = format!("{} {w1} {o}{}{c} {}{tail}", l[0], r[0], r[1]);
                        for (d, attr) in [("ltr", " dir=\"ltr\""), ("none", "")] {
                            out.push((
                                format!("{ln}-{rn}/{bn}/{sn}/{tn}/{d}"),
                                lang,
                                format!("<p{attr}>{text}</p>"),
                            ));
                        }
                    }
                }
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p { word-spacing: 0.3em }",
        "p { word-spacing: 0.5em }",
        "p { word-spacing: 12px }",
        "p { word-spacing: 2em }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em }",
        "p { letter-spacing: 0.5em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
