//! `zz_rr3m_hunt_numbers` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_numbers.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family numbers.

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
    for (sc, lang, w) in [
        ("he", "he", HE),
        ("ar", "ar", AR),
        (
            "fa",
            "fa",
            [
                "کتاب",
                "خانه",
                "بزرگ",
                "ایران",
                "می‌خواهم",
                "دانشگاه",
                "تهران",
                "سال",
                "روز",
                "شهر",
                "کشور",
                "مردم",
            ],
        ),
    ] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<Shape<'_>> = vec![
            (
                "an-sep",
                Box::new(move |i| format!("{} ١٬٢٣٤٫٥٦ {} ٧٨٩ {}.", s(i), s(i + 1), s(i + 2))),
            ),
            (
                "ean",
                Box::new(move |i| {
                    format!(
                        "{} ۱۲۳۴ {} ۵٫۶ {} ۷۸/۹ {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "en-sep",
                Box::new(move |i| {
                    format!(
                        "{} 1,234.56 {} 7.5 {} 1 000 {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "pct",
                Box::new(move |i| {
                    format!(
                        "{} 50% {} ٥٠٪ {} %25 {} ٪٣ {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "permille",
                Box::new(move |i| {
                    format!("{} 5‰ {} ٥؉ {} 3 ‱ {}.", s(i), s(i + 1), s(i + 2), s(i + 3))
                }),
            ),
            (
                "currency",
                Box::new(move |i| {
                    format!(
                        "{} $12.50 {} 12€ {} ₪100 {} 100 ﷼ {} £3 {}.",
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
                "signs",
                Box::new(move |i| {
                    format!(
                        "{} -5 {} +3.2 {} −7 {} ±2 {} ٣- {}.",
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
                "date",
                Box::new(move |i| {
                    format!(
                        "{} 2010/05/03 {} ٢٠١٠/٠٥/٠٣ {} 03.05.2010 {} 3-5-2010 {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "time",
                Box::new(move |i| {
                    format!(
                        "{} 10:30 {} ١٠:٣٠ {} 23:59:59 {} 9:05 AM {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "phone",
                Box::new(move |i| {
                    format!(
                        "{} +972-3-1234567 {} (03) 123-4567 {} ٠٥٠-١٢٣٤ {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "range",
                Box::new(move |i| {
                    format!(
                        "{} 1–2 {} ١–٢ {} 10-20 {} 10–20% {} pp. 3—7 {}.",
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
                "bracketed",
                Box::new(move |i| {
                    format!(
                        "{} (12) {} [٣٤] {} (1.5%) {} (٢٠١٠–٢٠١٢) {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "line-end",
                Box::new(move |i| {
                    format!(
                        "{} {} 2010. {} {} ٢٠١٠. {} {} (5)",
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
                "line-start",
                Box::new(move |i| {
                    format!(
                        "2010 {} {}. ١٢٣ {} {}. (4) {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "ordinal",
                Box::new(move |i| {
                    format!(
                        "1. {} 2. {} 3) {} ٤) {} ٥. {}",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "num-letter",
                Box::new(move |i| {
                    format!(
                        "{} ٣أ {} 3a {} 2ב {} H2O {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
            (
                "styled-num",
                Box::new(move |i| {
                    format!(
                        "{} <b>12</b>,<i>5</i> {} <b>٣٤</b>٫٥ {} <i>-</i>7 {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3)
                    )
                }),
            ),
            (
                "mixed-sep",
                Box::new(move |i| {
                    format!(
                        "{} 1,٢٣ {} ١.٥ {} 3٫5 {} ١/2 {}.",
                        s(i),
                        s(i + 1),
                        s(i + 2),
                        s(i + 3),
                        s(i + 4)
                    )
                }),
            ),
        ];
        for (name, shape) in &shapes {
            if sc == "fa"
                && ![
                    "ean",
                    "pct",
                    "date",
                    "range",
                    "bracketed",
                    "line-end",
                    "mixed-sep",
                ]
                .contains(name)
            {
                continue;
            }
            let text = rep(5, shape);
            out.push((
                format!("{sc}/{name}/rtl"),
                lang,
                format!("<p dir=\"rtl\">{text}</p>"),
            ));
            out.push((format!("{sc}/{name}/none"), lang, format!("<p>{text}</p>")));
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
