//! `zz_vx_k11rv3_just` (vx, round-5 review (confirm)): the books of
//! `tools/probes/rtl/probes/pdf/zz_vx_k11rv3_just.rs`, which is the probe as it ran.
//!
//! Round-3 review probe: the round-3 shapes in
//! long, justified paragraphs without letter-spacing — opening marks before
//! right-to-left words, styled neutral runs between them, Arabic-Indic
//! digits in brackets, and glosses in English. One ROW per book.

use super::common::{strip, untag, Book, Shape};
use super::vov::styled_book;
use crate::epub_support::conservation::{conservation, conservation_in_logical_order};
use tinker_pdf::Document;

const AR: [&str; 12] = [
    "عوامل",
    "عدّة",
    "وقد",
    "أصبحنا",
    "نعرف",
    "الآن",
    "الموادّ",
    "تكبح",
    "الخلايا",
    "السرطانيّة",
    "تتكاثر",
    "بسرعة",
];

const HE: [&str; 12] = [
    "שָׁלוֹם",
    "עוֹלָם",
    "מִלְחָמָה",
    "סֵפֶר",
    "בַּיִת",
    "גָּדוֹל",
    "אוֹר",
    "יוֹם",
    "בְּרֵאשִׁית",
    "בָּרָא",
    "אֱלֹהִים",
    "הָאָרֶץ",
];

fn books() -> Vec<Book> {
    let mut out: Vec<Book> = Vec::new();
    for (script, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        // A sentence of `n` words starting at `i`, with `mark(k)` spliced in
        // before the k-th word.
        let shapes: Vec<Shape<'_>> = vec![
            (
                "guil",
                Box::new(move |i| {
                    format!(
                        "{} {} «{} {}» {} {}.",
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
                "guil-i",
                Box::new(move |i| {
                    format!(
                        "{} {} <i>«</i>{} {}<i>»</i> {} {}.",
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
                "curly",
                Box::new(move |i| {
                    format!(
                        "{} {} “{}” {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "straight",
                Box::new(move |i| {
                    format!(
                        "{} \"{} {}\" {} {}.",
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
                    format!("(1) {} {} (2) {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "dash",
                Box::new(move |i| {
                    format!(
                        "{} — {} {} — {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "dash-b",
                Box::new(move |i| {
                    format!(
                        "{} <b>—</b> {} {} <b>—</b> {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "sq",
                Box::new(move |i| format!("{} [{}] {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))),
            ),
            (
                "paren-rtl",
                Box::new(move |i| {
                    format!(
                        "{} ({} {}) {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "qx-i",
                Box::new(move |i| {
                    format!("{} <i>?!</i> {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "semi-b",
                Box::new(move |i| {
                    format!("{} {}<b>;</b> {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "ellipsis-i",
                Box::new(move |i| format!("{} <i>…</i> {} {}.", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "colon-b",
                Box::new(move |i| {
                    format!("{}<b>:</b> {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "an-paren",
                Box::new(move |i| {
                    format!("{} {} (١٢٣) {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "an-bare",
                Box::new(move |i| format!("{} ١٢٣ {} ٤٥ {}.", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "en-paren",
                Box::new(move |i| {
                    format!("{} {} (2010) {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "hash",
                Box::new(move |i| format!("{} #{} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))),
            ),
            (
                "star",
                Box::new(move |i| format!("* {} {} * {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))),
            ),
            (
                "pct",
                Box::new(move |i| format!("{} 15% {} %{} {}.", s(i), s(i + 1), s(i + 2), s(i + 3))),
            ),
        ];
        for (name, shape) in &shapes {
            let text: String = (0..6).map(|k| shape(k * 3)).collect::<Vec<_>>().join(" ");
            out.push((
                format!("{script}/rtl/{name}"),
                lang,
                format!("<p dir=\"rtl\">{text}</p>"),
            ));
            out.push((
                format!("{script}/none/{name}"),
                lang,
                format!("<p>{text}</p>"),
            ));
        }
        // English paragraphs quoting right-to-left words.
        let en: Vec<(&str, String)> = vec![
            (
                "gloss",
                (0..5)
                    .map(|k| {
                        format!(
                            "The word for peace ({} {}) and for war ({}) appear here,",
                            s(3 * k),
                            s(3 * k + 1),
                            s(3 * k + 2)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            (
                "quote",
                (0..5)
                    .map(|k| {
                        format!(
                            "He said «{} {}» and then “{}” before [{}] was read,",
                            s(3 * k),
                            s(3 * k + 1),
                            s(3 * k + 2),
                            s(3 * k + 3)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            (
                "dash",
                (0..5)
                    .map(|k| {
                        format!(
                            "A list — {} {} — and {} - {} in the line,",
                            s(3 * k),
                            s(3 * k + 1),
                            s(3 * k + 2),
                            s(3 * k + 3)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            (
                "styled",
                (0..5)
                    .map(|k| {
                        format!(
                            "Words <b>{}</b> <i>{}</i> (<b>{}</b>) and more English text,",
                            s(3 * k),
                            s(3 * k + 1),
                            s(3 * k + 2)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
        ];
        for (name, text) in en {
            out.push((
                format!("{script}/en/{name}"),
                "en",
                format!("<p>{text}.</p>"),
            ));
        }
    }
    out
}

fn probe() {
    let styles = [
        "p { text-align: justify }",
        "p { text-align: justify; margin: 0 15% }",
        "p { text-align: justify; margin: 0 25% }",
        "p { text-align: justify; margin: 0 35% }",
        "p { text-align: justify; margin: 0 40% }",
        "body { font-family: sans-serif } p { text-align: justify; margin: 0 30% }",
        "body { font-family: monospace } p { text-align: justify; margin: 0 20% }",
        "p { text-align: justify; font-size: 1.3em; margin: 0 10% }",
        "p { word-spacing: 0.3em; margin: 0 20% }",
        "p { letter-spacing: 0.1em; text-align: justify; margin: 0 25% }",
    ];
    let (mut rows, mut fails) = (0, 0);
    for (si, style) in styles.iter().enumerate() {
        for (name, lang, body) in books() {
            for (label, body) in [("pointed", body.clone()), ("twin", strip(&body))] {
                let key = format!("j{si}\t{name}\t{label}");
                let bytes = styled_book(lang, style, &body);
                let doc = match Document::open(bytes.clone()) {
                    Ok(doc) => doc,
                    Err(e) => {
                        emit!("ROW\t{key}\tOPEN-ERR {e:?}");
                        continue;
                    }
                };
                let c = conservation(&bytes, &doc);
                let l = conservation_in_logical_order(&bytes, &doc);
                rows += 1;
                if !c.holds() || !l.holds() {
                    fails += 1;
                }
                let pages: Vec<_> = (0..doc.page_count()).filter_map(|p| doc.page(p)).collect();
                let texts: Vec<_> = pages.iter().map(|p| p.text()).collect();
                let mark = |c: char| matches!(c, '\u{0591}'..='\u{05C7}' | '\u{064B}'..='\u{065F}' | '\u{0670}');
                let words: Vec<String> = untag(&body)
                    .split_whitespace()
                    .map(|w| {
                        w.trim_matches(|c: char| !c.is_alphanumeric() && !mark(c))
                            .to_owned()
                    })
                    .filter(|w| {
                        w.chars().any(char::is_alphanumeric)
                            && w.chars().all(|c| c.is_alphanumeric() || mark(c))
                    })
                    .collect();
                let found = words
                    .iter()
                    .filter(|w| texts.iter().any(|t| !t.search(w).is_empty()))
                    .count();
                let text: String = texts
                    .iter()
                    .map(|t| t.plain_text())
                    .collect::<Vec<_>>()
                    .join("|")
                    .replace('\n', "|");
                emit!(
                    "ROW\t{key}\tcontent={}\tlogical={}\tsearch={}\tfound={found}/{}\t{:?}\t{}",
                    c.holds(),
                    l.holds(),
                    found == words.len(),
                    words.len(),
                    l.divergences.first().or(c.divergences.first()),
                    text
                );
            }
        }
    }
    emit!("rows={rows} fails={fails}");
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
