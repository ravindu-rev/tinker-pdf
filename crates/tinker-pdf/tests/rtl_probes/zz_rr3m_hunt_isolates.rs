//! `zz_rr3m_hunt_isolates` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_isolates.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family isolates.

use super::common::{rep, run_book, Case, Shape, AR, HE};

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
    for (sc, lang, w) in [("he", "he", HE), ("ar", "ar", AR)] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<Shape<'_>> = vec![
            (
                "lrm-num",
                Box::new(move |i| {
                    format!(
                        "{} 2010\u{200E} {} 15%\u{200E}. {}",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "rlm-punct",
                Box::new(move |i| {
                    format!(
                        "{} HDAC!\u{200F} {} (x)\u{200F} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "alm",
                Box::new(move |i| {
                    format!(
                        "{} ١٢\u{061C}-٣ {} \u{061C}(٤) {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "lrm-wrap",
                Box::new(move |i| {
                    format!(
                        "{} \u{200E}HDAC (x)\u{200E} {} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "bdi-latin",
                Box::new(move |i| {
                    format!("{} <bdi>HDAC (x)!</bdi> {} {}.", s(i), s(i + 1), s(i + 2))
                }),
            ),
            (
                "bdi-rtl",
                Box::new(move |i| format!("{} <bdi>{} (1)</bdi> {}.", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "span-auto",
                Box::new(move |i| {
                    format!(
                        "{} <span dir=\"auto\">HDAC 2!</span> {} <span dir=\"auto\">{}?</span> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "span-ltr",
                Box::new(move |i| {
                    format!("{} <span dir=\"ltr\">a (b) c!</span> {}.", s(i), s(i + 1))
                }),
            ),
            (
                "css-isolate",
                Box::new(move |i| {
                    format!("{} <span style=\"unicode-bidi: isolate; direction: ltr\">x [y] 3.</span> {}.", s(i), s(i + 1))
                }),
            ),
            (
                "css-embed",
                Box::new(move |i| {
                    format!("{} <span style=\"unicode-bidi: embed; direction: ltr\">x [y] 3.</span> {}.", s(i), s(i + 1))
                }),
            ),
            (
                "css-plaintext",
                Box::new(move |i| {
                    format!(
                        "{} <span style=\"unicode-bidi: plaintext\">x (y) {}</span> {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
            (
                "bdo",
                Box::new(move |i| {
                    format!("{} <bdo dir=\"ltr\">{}</bdo> {}.", s(i), s(i + 1), s(i + 2))
                }),
            ),
            (
                "chars-embed",
                Box::new(move |i| {
                    format!(
                        "{} \u{202A}a (b)\u{202C} {} \u{202B}{} !\u{202C} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "chars-isolate",
                Box::new(move |i| {
                    format!(
                        "{} \u{2066}a (b)\u{2069} {} \u{2067}{} !\u{2069} \u{2068}x?\u{2069} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "chars-override",
                Box::new(move |i| {
                    format!(
                        "{} \u{202D}ab\u{202C} {} \u{202E}cd\u{202C} {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2)
                    )
                }),
            ),
        ];
        for (name, shape) in &shapes {
            let text = rep(5, shape);
            out.push((
                format!("{sc}/{name}/rtl"),
                lang,
                format!("<p dir=\"rtl\">{text}</p>"),
            ));
            out.push((format!("{sc}/{name}/none"), lang, format!("<p>{text}</p>")));
        }
        // Left-to-right paragraphs holding isolates of right-to-left text.
        let en: Vec<(&str, String)> = vec![
            (
                "en-bdi",
                rep(5, |i| {
                    format!("The <bdi>{} {}</bdi> (x) word and", s(i), s(i + 1))
                }),
            ),
            (
                "en-rlm",
                rep(5, |i| format!("The {}\u{200F} (x) word and", s(i))),
            ),
            (
                "en-span-rtl",
                rep(5, |i| {
                    format!("The <span dir=\"rtl\">{} (1)!</span> word and", s(i))
                }),
            ),
            (
                "en-rli",
                rep(5, |i| format!("The \u{2067}{} 2!\u{2069} word and", s(i))),
            ),
        ];
        for (name, text) in en {
            out.push((format!("{sc}/{name}"), "en", format!("<p>{text}</p>")));
        }
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
        "body { font-family: monospace } p { margin: 0 26%; text-align: justify }",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
