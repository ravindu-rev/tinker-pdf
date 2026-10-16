//! `zz_v6_transform` (v6t, round 6 lane V (hunt)): the books of
//! `tools/probes/rtl/probes/pdf/zz_v6_transform.rs`, which is the probe as it ran.

use super::common::{AR, Case, HE, rep, run_book};

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

// Family transform: spaced right-to-left paragraphs drawn under a CSS
// `transform` (a horizontal or uniform scale, a half turn, a skew, a shift)
// and under a line height so tight that one line's baseline is within half
// an em of the next. The writer's rules and its read-back of each line
// (paint::Fonts::set_readings) measure gaps in the run's own ems with no
// matrix; on the page a `scaleX` widens or narrows every gap against the
// glyphs' height, which is what a reader's thresholds are in. The tight
// line height puts two lines' runs within the half em paint::page_lines
// takes for one line.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w) in [("ar", "ar", AR), ("he", "he", HE)] {
        let s = |i: usize| w[i % w.len()];
        let shapes: Vec<(&str, Box<dyn Fn(usize) -> String>)> = vec![
            ("prose", Box::new(move |i| format!("{} {} {}، {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5)))),
            ("styled", Box::new(move |i| format!("{} <b>{}</b> {} <i>{}</i>, ({}).", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("punct", Box::new(move |i| format!("{} <b>?!</b> {} {}. <i>(</i>{}<i>)</i> {}!", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("num", Box::new(move |i| format!("{} 25\u{66A} <b>{}</b> \u{661}\u{662}\u{663} {} 2010، {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("gloss", Box::new(move |i| format!("{} {} (lingua franca) {} <i>sic</i> {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
        ];
        for (name, shape) in &shapes {
            let text = rep(3, |k| shape(k));
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((format!("{sc}/{name}/{d}"), lang, format!("<p{attr}>{text}</p>")));
            }
        }
    }
    out
}

fn styles() -> Vec<String> {
    let transforms = [
        "none",
        "scaleX(0.5)",
        "scaleX(0.7)",
        "scaleX(1.4)",
        "scaleX(2)",
        "scale(1.5)",
        "scale(0.6)",
        "rotate(180deg)",
        "skewX(-15deg)",
        "translateX(12px)",
    ];
    let spacing = [
        "word-spacing: 0.3em",
        "word-spacing: 0.6em",
        "word-spacing: 1em",
        "letter-spacing: 0.3em",
        "letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify",
    ];
    let mut out = Vec::new();
    for t in transforms {
        for s in spacing {
            out.push(format!("p {{ margin: 0 30%; transform: {t}; {s} }}"));
        }
    }
    for lh in ["0.6", "0.4"] {
        for s in ["word-spacing: 0.6em", "letter-spacing: 0.3em", "letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify"] {
            out.push(format!("p {{ margin: 0 30%; line-height: {lh}; {s} }}"));
        }
    }
    out
}
