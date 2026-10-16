//! `zz_v6_scripts2` (v6, round 6 lane V (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_v6_scripts2.rs`, which is the probe as it ran.

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

/// Every book of the family, as its probe built them, in order.
pub(crate) fn run() {
    probe();
}

// Family scripts2: right-to-left scripts no probe has set, among them the
// supplementary planes' (four bytes in UTF-8, a surrogate pair in UTF-16):
// Adlam, Hanifi Rohingya, Phoenician, Imperial Aramaic, Mende Kikakui; and
// the Basic Multilingual Plane's forms a reader of old PDFs meets: Arabic and
// Hebrew presentation forms, Urdu's letters, Persian with ZERO WIDTH NON-JOINER,
// a kashida (tatweel) inside a word, Yiddish ligatures.

const ADLAM: [&str; 8] = [
    "\u{1E900}\u{1E901}\u{1E902}",
    "\u{1E903}\u{1E904}",
    "\u{1E905}\u{1E906}\u{1E907}\u{1E908}",
    "\u{1E909}\u{1E90A}",
    "\u{1E90B}\u{1E90C}\u{1E90D}",
    "\u{1E90E}\u{1E90F}",
    "\u{1E910}\u{1E911}\u{1E912}",
    "\u{1E913}\u{1E914}",
];
const ROHG: [&str; 8] = [
    "\u{10D00}\u{10D01}\u{10D02}",
    "\u{10D03}\u{10D04}",
    "\u{10D05}\u{10D06}\u{10D07}",
    "\u{10D08}\u{10D09}",
    "\u{10D0A}\u{10D0B}\u{10D0C}",
    "\u{10D0D}\u{10D0E}",
    "\u{10D0F}\u{10D10}\u{10D11}",
    "\u{10D12}\u{10D13}",
];
const PHNX: [&str; 8] = [
    "\u{10900}\u{10901}\u{10902}",
    "\u{10903}\u{10904}",
    "\u{10905}\u{10906}\u{10907}",
    "\u{10908}\u{10909}",
    "\u{1090A}\u{1090B}\u{1090C}",
    "\u{1090D}\u{1090E}",
    "\u{1090F}\u{10910}\u{10911}",
    "\u{10912}\u{10913}",
];
const ARMI: [&str; 8] = [
    "\u{10840}\u{10841}\u{10842}",
    "\u{10843}\u{10844}",
    "\u{10845}\u{10846}\u{10847}",
    "\u{10848}\u{10849}",
    "\u{1084A}\u{1084B}\u{1084C}",
    "\u{1084D}\u{1084E}",
    "\u{1084F}\u{10850}\u{10851}",
    "\u{10852}\u{10853}",
];
const MEND: [&str; 8] = [
    "\u{1E800}\u{1E801}\u{1E802}",
    "\u{1E803}\u{1E804}",
    "\u{1E805}\u{1E806}\u{1E807}",
    "\u{1E808}\u{1E809}",
    "\u{1E80A}\u{1E80B}\u{1E80C}",
    "\u{1E80D}\u{1E80E}",
    "\u{1E80F}\u{1E810}\u{1E811}",
    "\u{1E812}\u{1E813}",
];
// Arabic presentation forms B: كتاب, بيت, نور, يوم... as their positional forms.
const ARPF: [&str; 8] = [
    "\u{FEDB}\u{FE98}\u{FE8E}\u{FE8F}",
    "\u{FE91}\u{FEF4}\u{FE96}",
    "\u{FEE7}\u{FEEE}\u{FEAD}",
    "\u{FEF3}\u{FEEE}\u{FEE1}",
    "\u{FEDF}\u{FEE4}\u{FE94}",
    "\u{FEB3}\u{FEE0}\u{FE8E}\u{FEE1}",
    "\u{FEE3}\u{FEA4}\u{FEE4}\u{FEAA}",
    "\u{FEFB}",
];
// Hebrew presentation forms: shin with dagesh and dots, alef with patah, wide letters.
const HEPF: [&str; 8] = [
    "\u{FB2C}\u{5DC}\u{5D5}\u{5DD}",
    "\u{FB2E}\u{5D5}\u{5E8}",
    "\u{FB31}\u{5D9}\u{5EA}",
    "\u{FB4B}\u{5DC}\u{5DD}",
    "\u{5E1}\u{FB44}\u{5E8}",
    "\u{FB21}\u{5D3}\u{5DD}",
    "\u{FB4F}",
    "\u{5D9}\u{FB35}\u{5DD}",
];
const URDU: [&str; 8] = [
    "\u{67E}\u{627}\u{6A9}\u{633}\u{62A}\u{627}\u{646}",
    "\u{679}\u{6CC}\u{644}\u{6CC}",
    "\u{688}\u{627}\u{6A9}",
    "\u{6AF}\u{691}\u{6BE}\u{6CC}",
    "\u{6BA}\u{6C1}\u{6D2}",
    "\u{6A9}\u{62A}\u{627}\u{628}",
    "\u{6CC}\u{6C1}\u{627}\u{6BA}",
    "\u{648}\u{6C1}",
];
const FAZ: [&str; 8] = [
    "\u{645}\u{6CC}\u{200C}\u{62E}\u{648}\u{627}\u{647}\u{645}",
    "\u{646}\u{645}\u{6CC}\u{200C}\u{62F}\u{627}\u{646}\u{645}",
    "\u{6A9}\u{62A}\u{627}\u{628}\u{200C}\u{647}\u{627}",
    "\u{62E}\u{627}\u{646}\u{647}",
    "\u{628}\u{632}\u{631}\u{6AF}\u{200C}\u{062A}\u{631}",
    "\u{631}\u{648}\u{632}",
    "\u{647}\u{645}\u{200C}\u{627}\u{6CC}\u{646}",
    "\u{633}\u{627}\u{644}",
];
const KASH: [&str; 8] = [
    "\u{643}\u{62A}\u{640}\u{640}\u{640}\u{627}\u{628}",
    "\u{628}\u{64A}\u{640}\u{62A}",
    "\u{646}\u{640}\u{648}\u{631}",
    "\u{64A}\u{648}\u{645}",
    "\u{62C}\u{645}\u{640}\u{640}\u{64A}\u{644}",
    "\u{643}\u{628}\u{64A}\u{631}",
    "\u{639}\u{640}\u{648}\u{627}\u{645}\u{644}",
    "\u{648}\u{642}\u{62F}",
];
const YI: [&str; 8] = [
    "\u{5F2}\u{5D9}\u{5D3}\u{5D9}\u{5E9}",
    "\u{5F0}\u{5D0}\u{5B7}\u{5E1}\u{5E2}\u{5E8}",
    "\u{5D1}\u{5F1}",
    "\u{5E9}\u{5D8}\u{5D5}\u{5D1}",
    "\u{5D4}\u{5F2}\u{5D6}",
    "\u{5D6}\u{5E2}\u{5DC}\u{5D8}",
    "\u{5DC}\u{5F2}\u{5B7}\u{5D8}",
    "\u{5D2}\u{5D5}\u{5D8}",
];

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let scripts: [(&str, &'static str, [&str; 8]); 11] = [
        ("adlam", "ff", ADLAM),
        ("rohg", "rhg", ROHG),
        ("phnx", "phn", PHNX),
        ("armi", "arc", ARMI),
        ("mend", "men", MEND),
        ("arpf", "ar", ARPF),
        ("hepf", "he", HEPF),
        ("urdu", "ur", URDU),
        ("faz", "fa", FAZ),
        ("kash", "ar", KASH),
        ("yi", "yi", YI),
    ];
    for (sc, lang, w) in scripts {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<(&str, Box<dyn Fn(usize) -> String>)> = vec![
            (
                "prose",
                Box::new(move |i| {
                    format!(
                        "{} {} {}, {} {} {}.",
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
                Box::new(move |i| {
                    format!(
                        "{} <b>{}</b> {} <i>{}</i> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "punct",
                Box::new(move |i| {
                    format!(
                        "{} ({}) {}?! {} \u{ab}{}\u{bb}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "num",
                Box::new(move |i| {
                    format!(
                        "{} 2010 {} 12.5 {} (3) {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "gloss",
                Box::new(move |i| {
                    format!(
                        "{} {} (abc def) {} <i>xyz</i> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
        ];
        for (name, shape) in &shapes {
            let text = rep(3, |k| shape(k));
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
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
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
