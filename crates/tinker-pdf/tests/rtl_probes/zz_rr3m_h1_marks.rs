//! `zz_rr3m_h1_marks` (h1, round-5 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h1_marks.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11/CD-19 round-5 hunt, family marks.

use super::common::{halves, rep, run_book, Case, ARP, HEP};

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

// Family 7: nonspacing marks under spacing, and a mark as the face's
// code-32 letter.

const LATM: [&str; 6] = [
    "cafe\u{301}",
    "na\u{308}ive",
    "Nguye\u{302}\u{303}n",
    "re\u{301}sume\u{301}",
    "a\u{30A}ngstro\u{308}m",
    "pin\u{303}ata",
];

const GRM: [&str; 6] = [
    "\u{3B1}\u{301}\u{3BB}\u{3C6}\u{3B1}",
    "\u{3B2}\u{3B7}\u{301}\u{3C4}\u{3B1}",
    "\u{3BB}\u{3CC}\u{3B3}\u{3BF}\u{3C2}",
    "\u{3BF}\u{301}\u{3BC}\u{3B1}",
    "\u{3B3}\u{3B7}\u{342}",
    "\u{3BD}\u{3B5}\u{301}\u{3BF}",
];

const CYM: [&str; 6] = [
    "\u{430}\u{301}\u{431}\u{432}",
    "\u{43C}\u{438}\u{301}\u{440}",
    "\u{434}\u{43E}\u{301}\u{43C}",
    "\u{441}\u{43B}\u{43E}\u{301}\u{432}\u{43E}",
    "\u{433}\u{43E}\u{301}\u{440}\u{43E}\u{434}",
    "\u{432}\u{43E}\u{434}\u{430}\u{301}",
];

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let hep: Vec<&str> = HEP.to_vec();
    let arp: Vec<&str> = ARP.to_vec();
    let scripts: [(&str, &'static str, Vec<&str>, char); 5] = [
        ("hep", "he", hep, '\u{5B8}'),
        ("arp", "ar", arp, '\u{64E}'),
        ("lat", "en", LATM.to_vec(), '\u{301}'),
        ("grm", "el", GRM.to_vec(), '\u{301}'),
        ("cym", "ru", CYM.to_vec(), '\u{301}'),
    ];
    for (sc, lang, w, mark) in scripts {
        let s = |i: usize| w[i % w.len()];
        let (b0, b1) = halves(s(1));
        let shapes: Vec<(&str, String)> = vec![
            (
                "prose",
                rep(2, |i| {
                    format!(
                        "{} {} {}, {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "styled",
                rep(2, |i| {
                    format!(
                        "{} <b>{}</b> {}, <i>{}</i> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "glued",
                format!(
                    "{} <b>{b0}</b>{b1} {} {}. {} {}.",
                    s(0),
                    s(2),
                    s(3),
                    s(4),
                    s(5)
                ),
            ),
        ];
        let positions: [(&str, String); 3] = [
            ("nat", String::new()),
            ("markfirst", format!("<p>x{mark}</p>")),
            ("out", "<p>\u{2212}</p>".to_owned()),
        ];
        for (pname, preface) in &positions {
            for (name, text) in &shapes {
                for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                    out.push((
                        format!("{sc}/{name}/{pname}/{d}"),
                        lang,
                        format!("{preface}<p{attr}>{text}</p>"),
                    ));
                }
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    [
        "p { word-spacing: -2px; margin: 0 20% }",
        "p { word-spacing: 0.5em; margin: 0 20% }",
        "p { word-spacing: 1em; margin: 0 20% }",
        "p { word-spacing: 2em; margin: 0 10% }",
        "p { text-align: justify; margin: 0 33% }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify; margin: 0 28% }",
        "body { font-family: sans-serif } p { letter-spacing: 0.5em; text-align: justify; margin: 0 25% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
