//! `zz_rr3m_h1_ofscripts` (h1, round-5 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h1_ofscripts.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11/CD-19 round-5 hunt, family ofscripts.

use super::common::{halves, rep, run_book, Case, AR, CY, GR, HE};

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

// Family 1: every script the overflow font draws, by paragraph direction,
// word spacing, justification and the position of the face's code-32 letter.

const KO: [&str; 12] = [
    "모든",
    "사람은",
    "태어날",
    "때부터",
    "자유로우며",
    "존엄과",
    "권리에",
    "있어",
    "동등하다",
    "인간은",
    "천부적으로",
    "이성과",
];

const ZH: [&str; 12] = [
    "中文", "字符", "测试", "书籍", "世界", "和平", "人民", "国家", "城市", "大学", "学生", "老师",
];

const NB: [&str; 12] = [
    "well\u{2011}known",
    "state\u{2011}of\u{2011}art",
    "x\u{2011}ray",
    "co\u{2011}op",
    "re\u{2011}read",
    "e\u{2011}mail",
    "self\u{2011}aware",
    "two\u{2011}way",
    "up\u{2011}to\u{2011}date",
    "long\u{2011}term",
    "far\u{2011}off",
    "one\u{2011}off",
];

const ARN: [&str; 12] = [
    "كتاب",
    "١٢٣",
    "الطالب",
    "٤٥",
    "بيت",
    "٢٠١٠",
    "نور",
    "٧٨٩",
    "عوامل",
    "٣٫٥",
    "أصبحنا",
    "٦٠",
];

const HEN: [&str; 12] = [
    "שלום",
    "١٢٣",
    "מלחמה",
    "۴۵",
    "בית",
    "٢٠١٠",
    "אור",
    "۷۸۹",
    "בראשית",
    "٣٫٥",
    "אלהים",
    "٦٠",
];

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    // (name, lang, words, a character of the script not in the words)
    let scripts: [(&str, &'static str, [&str; 12], char); 9] = [
        ("ar", "ar", AR, '\u{6A4}'),
        ("he", "he", HE, '\u{5E3}'),
        ("gr", "el", GR, '\u{3C9}'),
        ("cy", "ru", CY, '\u{449}'),
        ("ko", "ko", KO, '\u{D7A3}'),
        ("zh", "zh", ZH, '\u{9F8D}'),
        ("nb", "en", NB, '\u{2212}'),
        ("arn", "ar", ARN, '\u{6A4}'),
        ("hen", "he", HEN, '\u{5E3}'),
    ];
    for (sc, lang, w, outside) in scripts {
        let s = |i: usize| w[i % w.len()];
        let mid: Option<char> = if sc == "nb" {
            None
        } else {
            s(1).chars().nth(1)
        };
        let shapes: Vec<(&str, String)> = vec![
            (
                "prose",
                rep(3, |i| {
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
                rep(3, |i| {
                    format!(
                        "{} <b>{}</b> abc {} 2010 <i>{}</i>, ({}).",
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
                rep(3, |i| {
                    let (a, b) = halves(s(i + 1));
                    format!("{} <b>{a}</b>{b} {} {}.", s(i), s(i + 2), s(i + 4))
                }),
            ),
        ];
        let mut positions: Vec<(&str, String)> =
            vec![("nat", String::new()), ("out", format!("<p>{outside}</p>"))];
        if let Some(m) = mid {
            positions.push(("mid", format!("<p>{m}</p>")));
        }
        for (pname, preface) in &positions {
            for (name, text) in &shapes {
                for (d, attr) in [
                    ("rtl", " dir=\"rtl\""),
                    ("ltr", " dir=\"ltr\""),
                    ("none", ""),
                ] {
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
        "p { word-spacing: 0.25em; margin: 0 20% }",
        "p { word-spacing: 0.5em; margin: 0 20% }",
        "p { word-spacing: 1em; margin: 0 20% }",
        "p { word-spacing: 2em; margin: 0 15% }",
        "p { text-align: justify; margin: 0 30% }",
        "p { letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify; margin: 0 25% }",
        "body { font-family: monospace } p { word-spacing: 0.5em; text-align: justify; margin: 0 25% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
