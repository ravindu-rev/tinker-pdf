//! `zz_rr3m_h1_breadth` (h1, round-5 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h1_breadth.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11/CD-19 round-5 hunt, family breadth.

use super::common::{run_book, Case, AR, CY, GR, HE, HEP};

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

// Family 9: breadth of the three classes the hunt found: a paragraph's last
// stop after right-to-left text and a number in a dir-less or ltr paragraph;
// a Greek or Cyrillic cell before a right-to-left one in a dir-less table;
// a number, a sign past WinAnsi and a styled word, justified.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    // A: the last stop.
    for (sc, lang, w, pre) in [("he", "he", HE, "\u{5E9}"), ("ar", "ar", AR, "\u{643}")] {
        let s = |i: usize| w[i % w.len()];
        for (nn, n) in [
            ("an", "\u{666}\u{660}"),
            ("en", "60"),
            ("ean", "\u{6F6}\u{6F0}"),
            ("word", ""),
        ] {
            for (pn, p) in [
                ("dot", "."),
                ("ex", "!"),
                ("colon", ":"),
                ("paren", ")."),
                ("dq", ".\""),
            ] {
                let tail = if n.is_empty() {
                    s(5).to_owned()
                } else {
                    format!("{} {n}", s(5))
                };
                let text = if pn == "paren" {
                    format!("{} {}. {} ({tail}).", s(8), s(9), s(6))
                } else {
                    let p2 = p;
                    format!("{} {}. {} {tail}{p2}", s(8), s(9), s(6))
                };
                for (prn, preface) in [("pref", format!("<p>{pre}</p>")), ("none", String::new())] {
                    for (d, attr) in [("ltr", " dir=\"ltr\""), ("none", "")] {
                        out.push((
                            format!("A/{sc}/{nn}/{pn}/{prn}/{d}"),
                            lang,
                            format!("{preface}<p{attr}>{text}</p>"),
                        ));
                    }
                }
            }
        }
    }
    // B: a cell of left-to-right overflow text before a right-to-left cell.
    for (gn, glang, g) in [("gr", "el", GR), ("cy", "ru", CY)] {
        let gs = |i: usize| g[i % g.len()];
        for (rn, rlang, r) in [("he", "he", HE), ("ar", "ar", AR), ("hep", "he", HEP)] {
            let rs = |i: usize| r[i % r.len()];
            for (k, (a, b)) in [(0usize, 1usize), (2, 3), (4, 11), (10, 7)]
                .iter()
                .enumerate()
            {
                let left = format!("{} {}", gs(*a), gs(*b));
                for (cn, right) in [
                    ("one", rs(*a).to_owned()),
                    ("two", format!("{} {}", rs(*a), rs(*b))),
                ] {
                    let _ = glang;
                    out.push((
                        format!("B/{gn}-{rn}/{k}/{cn}"),
                        rlang,
                        format!("<table><tr><td>{left}</td><td>{right}</td></tr></table>"),
                    ));
                }
            }
        }
    }
    // C: a number, a sign and a styled word.
    for (sc, lang, w) in [("he", "he", HE), ("ar", "ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        for (en, e) in [
            ("acomma", "25\u{60C}"),
            ("angle", "\u{27E8}25\u{27E9}"),
            ("pct", "25\u{66A}"),
            ("shekel", "25\u{20AA}"),
            ("minus", "\u{2212}25"),
            ("ornate", "\u{FD3E}25\u{FD3F}"),
            ("comma", "25,"),
            ("an-acomma", "\u{662}\u{665}\u{60C}"),
        ] {
            for (k, (a, b, c)) in [(0usize, 8usize, 1usize), (3, 4, 5), (6, 10, 11)]
                .iter()
                .enumerate()
            {
                let text = format!("{} {} {e} <b>{}</b> {}.", s(*a), s(*b), s(*c), s(*c + 1));
                for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                    out.push((
                        format!("C/{sc}/{en}/{k}/{d}"),
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
        "p, td { word-spacing: 0.5em }",
        "p, td { word-spacing: 1em }",
        "p { word-spacing: 0.5em; margin: 0 20% } td { word-spacing: 0.6em }",
        "p { letter-spacing: 0.1em; word-spacing: 0.25em; text-align: justify; margin: 0 35% } td { letter-spacing: 0.1em; word-spacing: 0.5em }",
        "p { text-align: justify; margin: 0 30% } table { border-spacing: 0 } td { padding: 0; word-spacing: 2em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
