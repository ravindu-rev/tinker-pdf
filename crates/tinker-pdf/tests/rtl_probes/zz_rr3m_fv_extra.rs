//! `zz_rr3m_fv_extra` (fv, 56dfd93 verification): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_fv_extra.rs`, which is the probe as it ran.
//!
//! Fix-verify probe.

use super::common::{rep, run_book, Case, AR, HE, HEP};

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

fn cases() -> Vec<Case> {
    // Fix-verify extra family: books aimed at the fix's three new paths —
    // the stretch-end as-typed rule, the mixed-piece as-typed rule, and the
    // comma tie-break's line-before exception — in shapes the hunt did not.
    let mut out: Vec<Case> = Vec::new();
    // 1. stretch ends: styled neutrals glued after / both sides / at the ends.
    let pieces = ["?!", "...", "!!", "),", "\".", ":-", "»,", "?! ?!"];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        for (pi, p) in pieces.iter().enumerate() {
            let after = rep(3, |i| {
                format!(
                    "{} {}<b>{p}</b> {} {}.",
                    s(i + pi),
                    s(i + pi + 1),
                    s(i + pi + 2),
                    s(i + pi + 3)
                )
            });
            let both = rep(3, |i| {
                format!(
                    "{} {}<b>{p}</b>{} {}.",
                    s(i + pi),
                    s(i + pi + 1),
                    s(i + pi + 2),
                    s(i + pi + 3)
                )
            });
            let end = format!("{} {} {}<b>{p}</b>", s(pi), s(pi + 1), s(pi + 2));
            let start = format!("<b>{p}</b>{} {} {}.", s(pi), s(pi + 1), s(pi + 2));
            for (shape, text) in [
                ("after", after),
                ("both", both),
                ("end", end),
                ("start", start),
            ] {
                out.push((
                    format!("s/{sc}/p{pi}/{shape}/rtl"),
                    lang,
                    format!("<p dir=\"rtl\">{text}</p>"),
                ));
                out.push((
                    format!("s/{sc}/p{pi}/{shape}/none"),
                    lang,
                    format!("<p>{text}</p>"),
                ));
            }
        }
    }
    // 2. mixed pieces: a number with a sign opening a wrapped line, a styled
    // word after it.
    let nums = ["5%", "٥٪", "3.5%", "+5", "5°", "(5%)", "5%,", "#5"];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        for (ni, n) in nums.iter().enumerate() {
            let a = rep(3, |i| {
                format!(
                    "{} {} {} {} {n} <b>{}</b> {}",
                    s(i + ni),
                    s(i + ni + 1),
                    s(i + ni + 2),
                    s(i + ni + 3),
                    s(i + ni + 4),
                    s(i + ni + 5)
                )
            });
            let b = rep(3, |i| {
                format!(
                    "{} {} {} {} {n}<i>{}</i>. {}",
                    s(i + ni),
                    s(i + ni + 1),
                    s(i + ni + 2),
                    s(i + ni + 3),
                    s(i + ni + 4),
                    s(i + ni + 5)
                )
            });
            for (shape, text) in [("bold", a), ("glued", b)] {
                out.push((
                    format!("m/{sc}/n{ni}/{shape}/rtl"),
                    lang,
                    format!("<p dir=\"rtl\">{text}</p>"),
                ));
                out.push((
                    format!("m/{sc}/n{ni}/{shape}/none"),
                    lang,
                    format!("<p>{text}</p>"),
                ));
            }
        }
    }
    // 3. the comma tie-break's mark: dir-less lines `(W) W<mark>` and
    // right-to-left lines that open with a mark after a forced break.
    let marks = [".", ",", ":", ";", "!", "?", "،", "…", ".\u{201D}"];
    let brackets = [
        ("(", ")"),
        ("[", "]"),
        ("{", "}"),
        ("«", "»"),
        ("\u{FD3F}", "\u{FD3E}"),
    ];
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE), ("hep", "he", HEP)] {
        let s = |i: usize| w[i % w.len()];
        for (mi, m) in marks.iter().enumerate() {
            for (bi, (o, c)) in brackets.iter().enumerate() {
                if sc == "hep" && bi > 1 {
                    continue;
                }
                let apart = rep(2, |i| {
                    format!(
                        "{} {} {} <b>{o}</b>{}<b>{c}</b> {}{m}",
                        s(i + mi),
                        s(i + mi + 1),
                        s(i + mi + 2),
                        s(i + mi + 3),
                        s(i + mi + 4)
                    )
                });
                let two = rep(2, |i| {
                    format!(
                        "{} {} <b>{o}</b>{}<b>{c}</b> {o}{}{c} {}{m}",
                        s(i + mi),
                        s(i + mi + 1),
                        s(i + mi + 2),
                        s(i + mi + 3),
                        s(i + mi + 4)
                    )
                });
                let br = format!(
                    "{} {}<br/>{m}{o}{}{c} {} {}",
                    s(mi),
                    s(mi + 1),
                    s(mi + 2),
                    s(mi + 3),
                    s(mi + 4)
                );
                out.push((
                    format!("c/{sc}/m{mi}/b{bi}/apart/none"),
                    lang,
                    format!("<p>{apart}</p>"),
                ));
                out.push((
                    format!("c/{sc}/m{mi}/b{bi}/two/none"),
                    lang,
                    format!("<p>{two}</p>"),
                ));
                out.push((
                    format!("c/{sc}/m{mi}/b{bi}/br/rtl"),
                    lang,
                    format!("<p dir=\"rtl\">{br}</p>"),
                ));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    let mut out = Vec::new();
    for ws in ["0.5em", "0.6em", "1em"] {
        out.push(format!("p {{ word-spacing: {ws}; margin: 0 20% }}"));
    }
    for ls in ["0.25em", "0.5em"] {
        out.push(format!(
            "p {{ letter-spacing: {ls}; text-align: justify; margin: 0 25% }}"
        ));
    }
    out.push("p { letter-spacing: 0.2em; word-spacing: 0.4em; margin: 0 30% }".to_owned());
    out.push("body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }".to_owned());
    for m in ["34%", "42%"] {
        out.push(format!(
            "body {{ font-family: sans-serif }} p {{ margin: 0 {m} }}"
        ));
        out.push(format!("p {{ margin: 0 {m} }}"));
    }
    out.push("body { font-family: monospace } p { margin: 0 38% }".to_owned());
    out
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
