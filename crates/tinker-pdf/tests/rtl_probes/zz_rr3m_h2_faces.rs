//! `zz_rr3m_h2_faces` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_faces.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family faces.

use super::common::{nsm, rep, run_book, Case};

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

// Family faces: scripts the overflow font draws that no earlier probe tried
// (Armenian, Georgian, Thai, Devanagari, Ethiopic, Syriac, Thaana, N'Ko,
// Arabic presentation forms, Persian with ZWNJ, Urdu, Yiddish, Japanese,
// IPA, polytonic Greek, Hebrew mixed with Latin), in other faces (sans,
// mono, bold italic), with nested inline styles, under spacing in mixed
// units, and with the face's code-32 letter either the book's natural first
// or the paragraph's last letter (met first in a preface paragraph).

const SCRIPTS: [(&str, &str, [&str; 8]); 16] = [
    (
        "hy",
        "hy",
        [
            "բարև",
            "աշխարհ",
            "գիրք",
            "տուն",
            "մեծ",
            "լույս",
            "օր",
            "քաղաք",
        ],
    ),
    (
        "ka",
        "ka",
        [
            "გამარჯობა",
            "მსოფლიო",
            "წიგნი",
            "სახლი",
            "დიდი",
            "სინათლე",
            "დღე",
            "ქალაქი",
        ],
    ),
    (
        "th",
        "th",
        ["สวัสดี", "โลก", "หนังสือ", "บ้าน", "ใหญ่", "แสง", "วัน", "เมือง"],
    ),
    (
        "hi",
        "hi",
        ["नमस्ते", "दुनिया", "किताब", "घर", "बड़ा", "रोशनी", "दिन", "शहर"],
    ),
    (
        "am",
        "am",
        ["ሰላም", "ዓለም", "መጽሐፍ", "ቤት", "ትልቅ", "ብርሃን", "ቀን", "ከተማ"],
    ),
    (
        "syr",
        "syr",
        [
            "ܫܠܡܐ",
            "ܥܠܡܐ",
            "ܟܬܒܐ",
            "ܒܝܬܐ",
            "ܪܒܐ",
            "ܢܘܗܪܐ",
            "ܝܘܡܐ",
            "ܡܕܝܢܬܐ",
        ],
    ),
    (
        "dv",
        "dv",
        ["ދިވެހި", "ބަސް", "ފޮތް", "ގެ", "ބޮޑު", "އަލި", "ދުވަސް", "ރަށް"],
    ),
    (
        "nqo",
        "nqo",
        ["ߒߞߏ", "ߞߊ߲", "ߛߓߍ", "ߓߏ߲", "ߓߊ", "ߞߍ", "ߟߊ", "ߘߎ"],
    ),
    (
        "arpf",
        "ar",
        ["ﻛﺘﺎﺏ", "ﺑﻴﺖ", "ﻧﻮﺭ", "ﻳﻮﻡ", "ﺟﻤﻴﻞ", "ﻛﺒﻴﺮ", "ﻋﻮﺍﻣﻞ", "ﻭﻗﺪ"],
    ),
    (
        "fa",
        "fa",
        [
            "می\u{200C}خواهم",
            "کتاب\u{200C}ها",
            "خانه",
            "بزرگ",
            "روز",
            "۱۲۳",
            "نور",
            "دنیا",
        ],
    ),
    (
        "ur",
        "ur",
        ["کتاب", "گھر", "بڑا", "روشنی", "دن", "شہر", "دنیا", "سلام"],
    ),
    (
        "yi",
        "yi",
        [
            "װעלט",
            "ייִדיש",
            "בוך",
            "הויז",
            "גרויס",
            "ליכט",
            "טאָג",
            "שטאָט",
        ],
    ),
    (
        "ja",
        "ja",
        ["こんにちは", "世界", "本", "家", "大きい", "光", "日", "町"],
    ),
    (
        "ipa",
        "en",
        ["ʃɪp", "θɪŋk", "ðɪs", "ŋ̊ə̃", "ʒɑ̃", "ɛɡ", "ʔa", "ɾu"],
    ),
    (
        "grp",
        "grc",
        ["ἀρχῇ", "ἦν", "λόγος", "πρὸς", "θεόν", "οὗτος", "ἐν", "αὐτῷ"],
    ),
    (
        "mix",
        "he",
        [
            "שלום", "hello", "עולם", "world", "ספר", "book", "בית", "home",
        ],
    ),
];

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in SCRIPTS {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<(&str, String)> = vec![
            ("styled", rep(2, |i| format!("{} <b>{}</b> {} <i>{}</i>, ({}) {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5)))),
            ("nested", format!("{} <b><i>{}</i> {}</b> <code>{} {}</code> {}, <span style=\"font-family: sans-serif\">{} {}</span>.", s(0), s(1), s(2), s(3), s(4), s(5), s(6), s(7))),
        ];
        // The letter the paragraph ends on, met first in a preface.
        let last: char = s(7).chars().rfind(|c| !nsm(*c)).unwrap_or('x');
        let positions: [(&str, String); 2] =
            [("nat", String::new()), ("last", format!("<p>{last}</p>"))];
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
    let faces = [
        ("sans", "body { font-family: sans-serif }"),
        ("mono", "body { font-family: monospace }"),
        ("bi", "body { font-weight: bold; font-style: italic }"),
    ];
    let spacing = [
        "p { word-spacing: 9px; margin: 0 20% }",
        "p { word-spacing: 1.5ex; letter-spacing: 1pt; margin: 0 18% }",
        "p { word-spacing: 2rem; margin: 0 12% }",
        "p { text-align: justify; margin: 0 36% }",
        "p { text-align: justify; letter-spacing: 0.05em; word-spacing: 0.3em; margin: 0 2% }",
    ];
    let mut out = Vec::new();
    for (_, f) in faces {
        for s in spacing {
            out.push(format!("{f} {s}"));
        }
    }
    out
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
