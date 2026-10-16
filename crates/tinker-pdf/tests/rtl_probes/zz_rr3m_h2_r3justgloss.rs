//! `zz_rr3m_h2_r3justgloss` (h2, CD-19 fix review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_h2_r3justgloss.rs`, which is the probe as it ran.
//!
//! Hunt probe: CD-19 round-5 hunt 2, family r2justgloss.

use super::common::{run_book, Case};

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

// Family justgloss: N5 in running text, a left-to-right paragraph of some
// forty words (English, Greek or Russian) with right-to-left glosses
// (Hebrew, Arabic) in brackets and other shapes, justified at three
// measures or word-spaced, serif and sans-serif.

const EN: [&str; 16] = [
    "the", "word", "for", "peace", "is", "older", "than", "any", "of", "its", "readers", "and",
    "they", "said", "it", "often",
];

const EL: [&str; 16] = [
    "η",
    "λέξη",
    "για",
    "ειρήνη",
    "είναι",
    "παλαιότερη",
    "από",
    "κάθε",
    "της",
    "αναγνώστη",
    "και",
    "είπαν",
    "αυτό",
    "συχνά",
    "πολύ",
    "εδώ",
];

const RU: [&str; 16] = [
    "слово",
    "для",
    "мира",
    "старше",
    "чем",
    "любой",
    "из",
    "его",
    "читателей",
    "и",
    "они",
    "говорили",
    "это",
    "часто",
    "здесь",
    "там",
];

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    let glosses: [(&str, &'static str, &str); 6] = [
        ("paren-he", "he", "(שלום)"),
        ("paren2-he", "he", "(שלום עולם)"),
        ("ital-he", "he", "<i>(שָׁלוֹם)</i>"),
        ("sq-ar", "ar", "[سلام]"),
        ("bold-ar", "ar", "<b>سلام</b> (كتاب)"),
        ("comma-he", "he", "(שלום, עולם)"),
    ];
    for (ln, l) in [("en", EN), ("el", EL), ("ru", RU)] {
        let s = |i: usize| l[i % l.len()];
        for (gn, lang, g) in glosses {
            let mut words: Vec<String> = Vec::new();
            for i in 0..40 {
                words.push(s(i).to_owned());
                if i == 3 || i == 17 || i == 31 {
                    words.push(g.to_owned());
                }
            }
            let text = format!("{}.", words.join(" "));
            for (d, attr) in [("none", ""), ("ltr", " dir=\"ltr\"")] {
                out.push((
                    format!("{ln}/{gn}/{d}"),
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
        "p { text-align: justify; margin: 0 30% }",
        "p { text-align: justify; margin: 0 38% }",
        "p { text-align: justify; margin: 0 10% }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 33% }",
        "p { word-spacing: 0.5em }",
        "p { word-spacing: 0.75em; margin: 0 20% }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
