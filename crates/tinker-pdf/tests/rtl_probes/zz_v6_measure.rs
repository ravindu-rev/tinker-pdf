//! `zz_v6_measure` (v6, round 6 lane V (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_v6_measure.rs`, which is the probe as it ran.

use super::common::{run_book, Case, AR, HE, HEP};

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

// Family measure: measures narrower than a word (breaks inside a word, with
// overflow-wrap and word-break, and soft hyphens), a line wider than the page
// (nowrap), very small and very large type, preserved white space (runs of
// spaces between right-to-left words under word spacing, pre-wrap), and a
// paragraph broken across a page at each of nine places by a spacer before it.

fn soft(w: &str) -> String {
    let v: Vec<char> = w.chars().collect();
    if v.len() < 4 {
        return w.to_owned();
    }
    let k = v.len() / 2;
    format!(
        "{}&shy;{}",
        v[..k].iter().collect::<String>(),
        v[k..].iter().collect::<String>()
    )
}

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE), ("hep", "he", HEP)] {
        let s = |i: usize| w[i % w.len()];
        let sent = |i: usize| {
            format!(
                "{} {} <b>{}</b> {}، {} ({}) {}. {}?!",
                s(i),
                s(i + 1),
                s(i + 2),
                s(i + 3),
                s(i + 4),
                s(i + 5),
                s(i + 6),
                s(i + 7)
            )
        };
        let long = (0..8).map(|k| sent(k * 3)).collect::<Vec<_>>().join(" ");
        let mut shapes: Vec<(String, String)> = vec![
            ("w1em".into(), format!("<div style=\"width: 1em\"><p{{D}}>{}</p></div>", sent(0))),
            ("w2em-anywhere".into(), format!("<div style=\"width: 2.5em\"><p{{D}} style=\"overflow-wrap: anywhere\">{}</p></div>", sent(1))),
            ("w3em-breakall".into(), format!("<div style=\"width: 3em\"><p{{D}} style=\"word-break: break-all\">{}</p></div>", sent(2))),
            ("w4em-shy".into(), format!("<div style=\"width: 4em\"><p{{D}} style=\"hyphens: manual\">{}</p></div>", (0..6).map(|k| soft(s(k))).collect::<Vec<_>>().join(" "))),
            ("nowrap".into(), format!("<p{{D}} style=\"white-space: nowrap\">{}</p>", (0..3).map(|k| sent(k * 3)).collect::<Vec<_>>().join(" "))),
            ("tiny".into(), format!("<p{{D}} style=\"font-size: 4px\">{}</p>", long)),
            ("huge".into(), format!("<p{{D}} style=\"font-size: 40px\">{}</p>", sent(4))),
            ("pre-spaces".into(), format!("<p{{D}} style=\"white-space: pre-wrap\">{}   {}  <b>{}</b>    {}، {}.</p>", s(0), s(1), s(2), s(3), s(4))),
            ("pre".into(), format!("<pre{{D}}>{} {}  {}\n{}   ({}) {}.</pre>", s(0), s(1), s(2), s(3), s(4), s(5))),
        ];
        for h in [560, 590, 610, 625, 640, 655, 670, 690, 710] {
            shapes.push((
                format!("break{h}"),
                format!("<div style=\"height: {h}pt\"></div><p{{D}}>{long}</p>"),
            ));
        }
        for (name, text) in &shapes {
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((format!("{sc}/{name}/{d}"), lang, text.replace("{D}", attr)));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p, pre { margin: 0 }",
        "p, pre { word-spacing: 0.6em }",
        "p, pre { text-align: justify }",
        "p, pre { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify }",
        "p, pre { word-spacing: 1em; orphans: 1; widows: 1 }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
