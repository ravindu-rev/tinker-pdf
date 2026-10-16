//! `zz_v6_inline` (v6, round 6 lane V (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_v6_inline.rs`, which is the probe as it ran.

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

/// Every book of the family, as its probe built them, in order.
pub(crate) fn run() {
    probe();
}

// Family inline: nested inline styles three deep, a style that opens inside a
// styled run, superscript and subscript (footnote calls, chemical formulae),
// vertical-align, smaller and larger type, monospace code, text shadows (one
// offset only along the baseline, so a copy is drawn on the line itself),
// spans that change only the colour of half a word, links, and a span that
// sets its own letter or word spacing inside a spaced paragraph.

const FA: [&str; 12] = [
    "کتاب",
    "خانه",
    "بزرگ",
    "ایران",
    "می\u{200C}خواهم",
    "دانشگاه",
    "تهران",
    "سال",
    "روز",
    "شهر",
    "کشور",
    "نمی\u{200C}دانم",
];

fn halves(w: &str) -> (String, String) {
    let v: Vec<char> = w.chars().collect();
    let k = (v.len() / 2).max(1).min(v.len());
    (v[..k].iter().collect(), v[k..].iter().collect())
}

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [
        ("ar", "ar", AR),
        ("he", "he", HE),
        ("hep", "he", HEP),
        ("fa", "fa", FA),
    ] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<(&str, Box<dyn Fn(usize) -> String>)> = vec![
            (
                "nest3",
                Box::new(move |i| {
                    format!(
                        "{} <b><i><u>{}</u></i></b> {}، {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "nest-mid",
                Box::new(move |i| {
                    format!(
                        "{} <b>{} <i>{}</i> {}</b> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "nest-punct",
                Box::new(move |i| {
                    format!(
                        "{} <b><i>{}</i>،</b> <i>({})</i>. {}",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "sup",
                Box::new(move |i| {
                    format!(
                        "{} {}<sup>1</sup> {} {}<sup>[2]</sup>. {}",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "sup-an",
                Box::new(move |i| {
                    format!(
                        "{} {}<sup>\u{661}\u{662}</sup>، {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "sub",
                Box::new(move |i| {
                    format!(
                        "{} H<sub>2</sub>O {} {}<sub>3</sub> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "valign",
                Box::new(move |i| {
                    format!(
                        "{} <span style=\"vertical-align: super; font-size: 70%\">{}</span> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "small",
                Box::new(move |i| {
                    format!(
                        "{} <small>{} {}</small> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "big",
                Box::new(move |i| {
                    format!(
                        "{} <span style=\"font-size: 160%\">{}</span> {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "code",
                Box::new(move |i| {
                    format!("{} <code>x = 1;</code> {} {}.", s(i), s(i + 1), s(i + 2))
                }),
            ),
            (
                "shadow",
                Box::new(move |i| {
                    format!(
                        "{} <span style=\"text-shadow: 1px 1px 0 #888\">{} {}</span> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "shadow-x",
                Box::new(move |i| {
                    format!(
                        "{} <span style=\"text-shadow: 2px 0 0 #888\">{}</span> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "colour-half",
                Box::new(move |i| {
                    let (a, b) = halves(s(i + 1));
                    format!("{} <span style=\"color: #a00\">{a}</span><span style=\"color: #00a\">{b}</span> {}.", s(i), s(i + 2))
                }),
            ),
            (
                "link",
                Box::new(move |i| {
                    format!(
                        "{} <a href=\"#n{i}\">{} {}</a> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "ls-span",
                Box::new(move |i| {
                    format!(
                        "{} <span style=\"letter-spacing: 0.3em\">{} {}</span> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "ws-span",
                Box::new(move |i| {
                    format!(
                        "{} <span style=\"word-spacing: 2em\">{} {} {}</span> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "ws0-span",
                Box::new(move |i| {
                    format!(
                        "{} <span style=\"word-spacing: 0; letter-spacing: 0\">{} {}</span> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
        ];
        for (name, shape) in &shapes {
            let text = rep(2, |k| shape(k));
            for (d, attr) in [
                ("rtl", " dir=\"rtl\""),
                ("ltr", " dir=\"ltr\""),
                ("none", ""),
            ] {
                out.push((
                    format!("{sc}/{name}/{d}"),
                    lang,
                    format!("<p{attr}>{text}</p>"),
                ));
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
        "p { letter-spacing: 0.5em }",
        "p { letter-spacing: 0.2em; word-spacing: 0.3em }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
