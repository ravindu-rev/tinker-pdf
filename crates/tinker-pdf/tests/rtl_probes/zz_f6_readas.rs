//! `zz_f6_readas` (f6inj, round 6 lane F (injections)): the books of
//! `tools/probes/rtl/probes/pdf/zz_f6_readas.rs`, which is the probe as it ran.

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

// Family f6_readas (lane F, round 6 of ROADMAP CD-19): F:INJ's scratch probe
// zz_inj_readas.rs, the NO-BREAK SPACE grid, its RA lines printed as the kit's
// rr3m lines. Books where the page's fonts read a character otherwise than it
// was typed, so that reading a line back (paint::Fonts::read_as and
// paint::Segment::read_as) can decide its drawing: a NO-BREAK SPACE, which a
// reader reads as a space, inside a number, before a sign or a comma, after a
// word and before a styled ?!; a SOFT HYPHEN, inside a word, a number and
// before a styled ?!; and letters of the supplementary planes (Adlam, Hanifi
// Rohingya) beside a styled ?! or a bracketed word. Hebrew, Arabic and Adlam,
// in right-to-left and dir-less paragraphs, under word spacing, letter
// spacing, justification and a scale of one axis. Made when injecting
// page/read-as-typed fired no test in either build.

const STYLES: [&str; 8] = [
    "p { word-spacing: 0.6em }",
    "p { word-spacing: 1em }",
    "p { word-spacing: 0.3em }",
    "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify; margin: 0 30% }",
    "p { transform: scaleX(2); word-spacing: 0.6em }",
    "p { word-spacing: 12px }",
    "p { letter-spacing: 0.15em; word-spacing: 0.3em }",
    "p { transform: scaleY(2); word-spacing: 0.6em }",
];

/// The `i`th word of a list, round and round.
fn word(list: &[&'static str], i: usize) -> &'static str {
    list[i % list.len()]
}

/// A word with a SOFT HYPHEN after its first letter.
fn soft(w: &str) -> String {
    let mut chars = w.chars();
    let first = chars.next().unwrap();
    format!("{first}\u{ad}{}", chars.as_str())
}

/// A group's shapes: each a name and a paragraph's text.
fn shapes(group: &str, w: &[&'static str; 12]) -> Vec<(&'static str, String)> {
    match group {
        "nbsp" => vec![
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
                "qb",
                rep(2, |i| {
                    format!(
                        "{} {} <b>1\u{a0}234</b> <i>?!</i> {}.",
                        word(w, i),
                        word(w, i + 1),
                        word(w, i + 2)
                    )
                }),
            ),
            (
                "pct",
                rep(2, |i| {
                    format!(
                        "{} 1\u{a0}234\u{66a} <b>{}</b> {}.",
                        word(w, i),
                        word(w, i + 1),
                        word(w, i + 2)
                    )
                }),
            ),
            (
                "paren",
                rep(2, |i| {
                    format!(
                        "{} (1\u{a0}234) <b>{}</b>, {}.",
                        word(w, i),
                        word(w, i + 1),
                        word(w, i + 2)
                    )
                }),
            ),
            (
                "comma",
                rep(2, |i| {
                    format!(
                        "{} 1\u{a0}234\u{60c} <b>{}</b> {}.",
                        word(w, i),
                        word(w, i + 1),
                        word(w, i + 2)
                    )
                }),
            ),
            (
                "lat",
                rep(2, |i| {
                    format!(
                        "{} abc\u{a0}def <b>{}</b> {}!",
                        word(w, i),
                        word(w, i + 1),
                        word(w, i + 2)
                    )
                }),
            ),
            (
                "nb2",
                rep(2, |i| {
                    format!(
                        "{}\u{a0}<b>?!</b> {} {}.",
                        word(w, i),
                        word(w, i + 1),
                        word(w, i + 2)
                    )
                }),
            ),
            (
                "nb3",
                rep(2, |i| {
                    format!(
                        "{} <b>{}</b>\u{a0}{} 25\u{66a}.",
                        word(w, i),
                        word(w, i + 1),
                        word(w, i + 2)
                    )
                }),
            ),
        ],
        "shy" => vec![
            (
                "shy",
                rep(2, |i| {
                    format!(
                        "{} {} <b>?!</b> {}.",
                        word(w, i),
                        soft(word(w, i + 1)),
                        word(w, i + 2)
                    )
                }),
            ),
            (
                "shyn",
                rep(2, |i| {
                    format!(
                        "{} 1\u{ad}234 <b>{}</b> {}.",
                        word(w, i),
                        word(w, i + 1),
                        word(w, i + 2)
                    )
                }),
            ),
            (
                "shyq",
                rep(2, |i| {
                    format!(
                        "{} {}\u{ad} <b>?!</b> {} 25\u{66a}.",
                        word(w, i),
                        word(w, i + 1),
                        word(w, i + 2)
                    )
                }),
            ),
        ],
        _ => vec![
            (
                "adlam",
                rep(2, |i| {
                    format!(
                        "{} \u{1e900}\u{1e923}\u{1e924}\u{1e922} <b>?!</b> {}.",
                        word(w, i),
                        word(w, i + 1)
                    )
                }),
            ),
            (
                "adlamn",
                rep(2, |i| {
                    format!(
                        "{} \u{1e900}\u{1e923}\u{1e924}\u{1e922} 25\u{66a} <b>{}</b>.",
                        word(w, i),
                        word(w, i + 1)
                    )
                }),
            ),
            (
                "rohg",
                rep(2, |i| {
                    format!(
                        "{} \u{10d00}\u{10d01}\u{10d02} (<i>{}</i>) {}.",
                        word(w, i),
                        word(w, i + 1),
                        word(w, i + 2)
                    )
                }),
            ),
        ],
    }
}

/// (style, name, language, body)
fn books() -> Vec<(String, String, &'static str, String)> {
    let mut out = Vec::new();
    for group in ["nbsp", "shy", "smp"] {
        for lang in ["he", "ar"] {
            let w = if lang == "ar" { &AR } else { &HE };
            for (name, text) in shapes(group, w) {
                for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                    for style in STYLES {
                        out.push((
                            style.to_owned(),
                            format!("{lang}/{name}/{d}"),
                            lang,
                            format!("<p{attr}>{text}</p>"),
                        ));
                    }
                }
            }
        }
    }
    // A Fulani paragraph in Adlam, whose every letter is past the Basic
    // Multilingual Plane.
    for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
        for style in STYLES {
            out.push((
                style.to_owned(),
                format!("ff/adlam/{d}"),
                "ff",
                format!("<p{attr}>\u{1e900}\u{1e923}\u{1e924}\u{1e922} <b>?!</b> \u{1e906}\u{1e935}\u{1e924}\u{1e922} 25\u{66a}.</p>"),
            ));
        }
    }
    out
}
