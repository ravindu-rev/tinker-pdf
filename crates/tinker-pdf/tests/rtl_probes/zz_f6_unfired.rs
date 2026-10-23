//! `zz_f6_unfired` (f6inj, round 6 lane F (injections)): the books of
//! `tools/probes/rtl/probes/pdf/zz_f6_unfired.rs`, which is the probe as it ran.

use super::common::{rep, run_book, AR, HE};

fn probe() {
    let mut n = 0usize;
    for (style, name, lang, body) in books() {
        n += 1;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_book(&style, &name, lang, &body)
        }));
        if r.is_err() {
            emit!("ROW\t{style}\t{name}\tm=0\ttext=0\tsearch=0/1\tltr=-\tcons=0\tlogical=0\tPANIC");
            emit!("PANIC\t{style}\t{name}");
        }
    }
    emit!("books={n}");
}

/// Every book of the family, as its probe built them, in order.
pub(crate) fn run() {
    probe();
}

// Family f6_unfired (lane F, round 6 of ROADMAP CD-19): F:INJ's scratch probe
// zz_inj_cand.rs, its RA lines printed as the kit's rr3m lines. Books for the
// rules of reading a line back (paint.rs) that a build's suites fired no test
// for when each was put back broken: paint::Fonts::reads_whole and
// paint::Fonts::reads_closer with bundled-fonts, read_as in either build.
// whole/, for reads_whole: dir-less lines with a styled ?! under a scale of
// one axis or a matrix, and styled marks in a paragraph scaled or in a
// scaled block; closer/, for reads_closer: squeezed inline blocks turned a
// quarter, a half and three quarters, and scaled and turned a quarter,
// styled marks turned a quarter and three quarters, and Arabic signs and
// numbers turned a quarter and a half;
// readas/, for read_as: a number with a NO-BREAK SPACE before an ARABIC
// COMMA, letter-spaced, justified and scaled. Hebrew and Arabic.

/// The `i`th word of a list, round and round.
fn word(list: &[&'static str], i: usize) -> &'static str {
    list[i % list.len()]
}

/// The list of a language's words.
fn words(lang: &str) -> &'static [&'static str; 12] {
    if lang == "ar" {
        &AR
    } else {
        &HE
    }
}

fn styled_marks(words: &[&'static str; 12]) -> String {
    rep(3, |i| {
        format!(
            "{} <b>?!</b> {} {}. <i>(</i>{}<i>)</i> {}!",
            word(words, i),
            word(words, i + 1),
            word(words, i + 2),
            word(words, i + 3),
            word(words, i + 4)
        )
    })
}

fn signs_and_numbers(sentences: usize) -> String {
    rep(sentences, |i| {
        format!(
            "{} 25\u{66A} <b>{}</b> \u{661}\u{662}\u{663} {} 2010\u{60C} {}.",
            word(&AR, i),
            word(&AR, i + 1),
            word(&AR, i + 2),
            word(&AR, i + 3)
        )
    })
}

fn squeezed_blocks(words: &[&'static str; 12]) -> String {
    rep(3, |i| {
        format!(
            "{} <span style=\"display: inline-block; transform: scaleX(0.5)\">{} <b>?!</b> {}</span> {} 25\u{66A} {}.",
            word(words, i),
            word(words, i + 1),
            word(words, i + 2),
            word(words, i + 3),
            word(words, i + 4)
        )
    })
}

/// Dir-less lines under a scale of one axis: `reads_whole`'s.
fn whole(out: &mut Vec<(String, String, &'static str, String)>, lang: &'static str) {
    let w = words(lang);
    let shapes: Vec<(&str, String)> = vec![
        ("v1", format!("{} <b>?!</b> {}.", word(w, 0), word(w, 1))),
        (
            "q",
            rep(2, |i| {
                format!(
                    "{} <b>?!</b> {} 1\u{a0}234 {}.",
                    word(w, i),
                    word(w, i + 1),
                    word(w, i + 2)
                )
            }),
        ),
        (
            "q-digits",
            rep(2, |i| {
                format!(
                    "{} <b>?!</b> {} 1234 {}.",
                    word(w, i),
                    word(w, i + 1),
                    word(w, i + 2)
                )
            }),
        ),
        (
            "q-words",
            rep(2, |i| {
                format!(
                    "{} <b>?!</b> {} {}.",
                    word(w, i),
                    word(w, i + 1),
                    word(w, i + 2)
                )
            }),
        ),
        (
            "q-one",
            format!(
                "{} <b>?!</b> {} 1\u{a0}234 {}.",
                word(w, 0),
                word(w, 1),
                word(w, 2)
            ),
        ),
    ];
    let styles = [
        "p { transform: scaleX(2); word-spacing: 0.6em }",
        "p { transform: scaleX(3); word-spacing: 0.6em }",
        "p { transform: scaleY(0.5); word-spacing: 0.6em }",
        "p { transform: matrix(1.6, 0, 0, 0.8, 0, 0); word-spacing: 0.6em }",
        "p { transform: scaleX(2); word-spacing: 1em }",
        "p { transform: scaleX(1.4); word-spacing: 1em }",
    ];
    for (name, text) in &shapes {
        for style in styles {
            out.push((
                style.to_owned(),
                format!("whole/{lang}/{name}"),
                lang,
                format!("<p>{text}</p>"),
            ));
        }
    }
    for style in [
        "p { margin: 0 30%; transform: scaleX(2); word-spacing: 0.6em } .w { transform: none }",
        "p { margin: 0 30%; transform: scaleX(3); word-spacing: 0.6em } .w { transform: none }",
        "p { margin: 0 30%; transform: scaleY(0.5); word-spacing: 0.6em } .w { transform: none }",
        "p { margin: 0 30%; transform: matrix(1.6, 0, 0, 0.8, 0, 0); word-spacing: 0.6em } .w { transform: none }",
        "p { margin: 0 30%; transform: none; word-spacing: 0.6em } .w { transform: scaleX(2) }",
    ] {
        out.push((
            style.to_owned(),
            format!("whole/{lang}/marks"),
            lang,
            format!("<div class=\"w\"><p>{}</p></div>", styled_marks(w)),
        ));
    }
}

/// Lines nothing reads right on the page under a turn: `reads_closer`'s.
fn closer(out: &mut Vec<(String, String, &'static str, String)>, lang: &'static str) {
    let w = words(lang);
    for (d, attr) in [("none", ""), ("rtl", " dir=\"rtl\"")] {
        for turn in [
            "rotate(90deg)",
            "rotate(180deg)",
            "rotate(270deg)",
            "scaleX(2) rotate(90deg)",
        ] {
            for ws in ["0.6em", "1em"] {
                out.push((
                    format!("p {{ margin: 0 30%; transform: {turn}; word-spacing: {ws} }} .w {{ transform: none }}"),
                    format!("closer/{lang}/ib/{d}"),
                    lang,
                    format!("<div class=\"w\"><p{attr}>{}</p></div>", squeezed_blocks(w)),
                ));
            }
        }
        for turn in ["rotate(90deg)", "rotate(270deg)"] {
            for ws in ["0.6em", "1em"] {
                out.push((
                    format!("p {{ margin: 0 30%; transform: {turn}; word-spacing: {ws} }} .w {{ transform: none }}"),
                    format!("closer/{lang}/marks/{d}"),
                    lang,
                    format!("<div class=\"w\"><p{attr}>{}</p></div>", styled_marks(w)),
                ));
            }
        }
        if lang == "ar" {
            for turn in ["rotate(90deg)", "rotate(180deg)"] {
                out.push((
                    format!("p {{ margin: 0 30%; transform: {turn}; word-spacing: 1em }}"),
                    format!("closer/{lang}/num/{d}"),
                    lang,
                    format!("<p{attr}>{}</p>", signs_and_numbers(3)),
                ));
            }
        }
    }
}

/// A number with a NO-BREAK SPACE before an ARABIC COMMA: `read_as`'s.
fn readas(out: &mut Vec<(String, String, &'static str, String)>, lang: &'static str) {
    let w = words(lang);
    let styles = [
        "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify; margin: 0 30% }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em; margin: 0 30% }",
        "p { transform: scaleX(2); word-spacing: 0.6em }",
        "p { word-spacing: 0.6em }",
        "p { word-spacing: 1em }",
        "p { letter-spacing: 0.15em; word-spacing: 0.3em }",
    ];
    for (n, sentences) in [("comma2", 2), ("comma1", 1)] {
        let text = rep(sentences, |i| {
            format!(
                "{} 1\u{a0}234\u{60c} <b>{}</b> {}.",
                word(w, i),
                word(w, i + 1),
                word(w, i + 2)
            )
        });
        for (d, attr) in [("none", ""), ("rtl", " dir=\"rtl\"")] {
            for style in styles {
                out.push((
                    style.to_owned(),
                    format!("readas/{lang}/{n}/{d}"),
                    lang,
                    format!("<p{attr}>{text}</p>"),
                ));
            }
        }
    }
}

/// (style, name, language, body)
fn books() -> Vec<(String, String, &'static str, String)> {
    let mut out = Vec::new();
    for lang in ["he", "ar"] {
        whole(&mut out, lang);
        closer(&mut out, lang);
        readas(&mut out, lang);
    }
    out
}
