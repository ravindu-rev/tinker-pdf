//! `zz_rr3m_hunt_scripts` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_scripts.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family scripts.

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

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let syr: [&str; 6] = ["ܫܠܡܐ", "ܥܠܡܐ", "ܟܬܒܐ", "ܒܝܬܐ", "ܢܘܗܪܐ", "ܝܘܡܐ"];
    let thaana: [&str; 6] = ["ދިވެހި", "ރާއްޖެ", "ފޮތް", "ގެ", "މާލެ", "ދުވަސް"];
    let nko: [&str; 6] = ["ߒߞߏ", "ߞߊ߲", "ߡߊ߲߬ߘߋ߲", "ߛߓߍ", "ߘߎ߰", "ߟߊ"];
    let samaritan: [&str; 6] = ["ࠀࠁࠂ", "ࠃࠄ", "ࠅࠆࠇ", "ࠈࠉ", "ࠊࠋ", "ࠌࠍ"];
    let mandaic: [&str; 6] = ["ࡀࡁࡂ", "ࡃࡄ", "ࡅࡆࡇ", "ࡈࡉ", "ࡊࡋ", "ࡌࡍ"];
    let arsup: [&str; 6] = ["ݐݑݒ", "ݓݔ", "ݕݖݗ", "ݘݙ", "ݚݛ", "ݜݝ"];
    for (sc, lang, w) in [
        ("syr", "syr", syr),
        ("thaana", "dv", thaana),
        ("nko", "nqo", nko),
        ("sam", "smp", samaritan),
        ("mandaic", "mid", mandaic),
        ("arsup", "ar", arsup),
    ] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<(&str, String)> = vec![
            (
                "plain",
                rep(4, |i| {
                    format!("{} {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "brackets",
                rep(4, |i| {
                    format!(
                        "{} ({} {}) [{}] «{}».",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "numbers",
                rep(4, |i| {
                    format!("{} 2010 {} ١٢٣ {} 15%.", s(i), s(i + 1), s(i + 2))
                }),
            ),
            (
                "latin",
                rep(4, |i| {
                    format!("{} HDAC (x) {} {}.", s(i), s(i + 1), s(i + 2))
                }),
            ),
            (
                "punct",
                rep(4, |i| {
                    format!(
                        "{}?! {}... {} — {}؛ {}، {}.",
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
                "styled",
                rep(4, |i| {
                    format!(
                        "{} <b>{}</b> <i>({})</i> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "he-mix",
                rep(4, |i| {
                    format!("{} שלום {} (עולם) {}.", s(i), s(i + 1), s(i + 2))
                }),
            ),
        ];
        for (name, text) in shapes {
            out.push((
                format!("{sc}/{name}/rtl"),
                lang,
                format!("<p dir=\"rtl\">{text}</p>"),
            ));
            out.push((format!("{sc}/{name}/none"), lang, format!("<p>{text}</p>")));
        }
        out.push((
            format!("{sc}/en-gloss"),
            "en",
            format!(
                "<p>{}</p>",
                rep(5, |i| format!(
                    "The word ({}) and «{} {}» here,",
                    s(i),
                    s(i + 1),
                    s(i + 2)
                ))
            ),
        ));
    }
    out
}

fn styles() -> Vec<String> {
    [
        "",
        "p { text-align: justify; margin: 0 30% }",
        "p { letter-spacing: 0.3em; text-align: justify; margin: 0 25% }",
        "p { word-spacing: 1em; margin: 0 20% }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 33% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
