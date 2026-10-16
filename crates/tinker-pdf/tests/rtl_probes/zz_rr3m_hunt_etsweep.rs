//! `zz_rr3m_hunt_etsweep` (hunt, K-11 round-4 review (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_rr3m_hunt_etsweep.rs`, which is the probe as it ran.
//!
//! Hunt probe: K-11 round-4 review, family etsweep.

use super::common::{rep, run_book, Case, AR, HE};

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
    // A number with a terminator (ET) beside it, at every place a wrapped
    // right-to-left line can put it.
    let mut out: Vec<Case> = Vec::new();
    let nums = [
        "5%", "5‰", "10–20%", "$5", "5°", "#5", "5¢", "5£", "٥%", "٥٪", "۵%", "15.5%", "-5%",
        "5% (x)",
    ];
    for (sc, lang, w) in [
        ("ar", "ar", AR),
        ("he", "he", HE),
        (
            "fa",
            "fa",
            [
                "کتاب",
                "خانه",
                "بزرگ",
                "ایران",
                "دانشگاه",
                "تهران",
                "سال",
                "روز",
                "شهر",
                "کشور",
                "مردم",
                "زبان",
            ],
        ),
    ] {
        let s = |i: usize| w[i % w.len()];
        for (ni, n) in nums.iter().enumerate() {
            let text = rep(4, |i| {
                format!(
                    "{} {} {} {} {n} {}",
                    s(i + ni),
                    s(i + ni + 1),
                    s(i + ni + 2),
                    s(i + ni + 3),
                    s(i + ni + 4)
                )
            });
            out.push((
                format!("{sc}/n{ni}/rtl"),
                lang,
                format!("<p dir=\"rtl\">{text}</p>"),
            ));
        }
    }
    out
}

fn styles() -> Vec<String> {
    let mut out = Vec::new();
    for fam in ["serif", "sans-serif", "monospace"] {
        for m in ["30%", "36%", "42%", "46%"] {
            out.push(format!(
                "body {{ font-family: {fam} }} p {{ margin: 0 {m} }}"
            ));
        }
        out.push(format!(
            "body {{ font-family: {fam} }} p {{ text-align: justify; margin: 0 38% }}"
        ));
    }
    out
}

/// Every book of the family, as its probe's tests built them, in order.
pub(crate) fn run() {
    probe();
}
