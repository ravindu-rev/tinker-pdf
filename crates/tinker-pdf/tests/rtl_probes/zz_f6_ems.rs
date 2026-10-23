//! `zz_f6_ems` (f6, round 6 lane F (sweep)): the books of
//! `tools/probes/rtl/probes/pdf/zz_f6_ems.rs`, which is the probe as it ran.

use super::common::{run_book, AR, HE};

fn probe() {
    let mut n = 0usize;
    for (style, name, lang, body) in books() {
        n += 1;
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_book(&style, &name, lang, &body)
        }));
        if r.is_err() {
            emit!("ROW\t{style}\t{name}\tm=0\ttext=0\tsearch=0/1\tltr=-\tcons=0\tlogical=0\tPANIC");
            emit!("PANIC\t{style}\t{name}");
        }
    }
    emit!("books={n}");
}

/// Every book of the family, as its probe built them, in order.
pub(crate) fn run() {
    probe();
}

// Family f6_ems (lane F, round 6 of ROADMAP CD-19): the edges of the fix of
// the class of lane V's V1. A reader's ems on the page (paint::along_the_line)
// are what page_lines' gap between two runs and parted's gap inside a run are
// asked in; the place of a mark of no advance that opens a styled run
// (paint::leading_mark_at) stays in the run's ems; and the run-space drawing
// is taken only where it reads closer on the page (paint::Fonts::reads_closer).
// A targeted sweep, made when the injections of three such rules changed no
// book of f6_transform: tables of two cells whose gap is near half an em,
// inline blocks of about half an em between words, and marks of no advance
// opening a styled run, letter-spaced; each under no transform, a scale of
// one axis and a skew.

const SCALES: [&str; 6] = [
    "none",
    "scaleX(0.5)",
    "scaleX(0.7)",
    "scaleX(2)",
    "scaleX(3)",
    "skewX(-15deg)",
];

/// (style, name, language, body)
fn books() -> Vec<(String, String, &'static str, String)> {
    let mut out = Vec::new();
    // Two cells: a Latin one and a right-to-left one, and two right-to-left
    // ones, the gap between them near half an em.
    for (lang, w) in [("he", HE), ("ar", AR)] {
        for sp in ["4px", "6px", "8px", "10px"] {
            for s in SCALES {
                for (shape, cells) in [
                    (
                        "lat",
                        format!("<td>abc (x).</td><td>{} {}</td>", w[0], w[1]),
                    ),
                    (
                        "rtl",
                        format!(
                            "<td><b>{}</b> {}\u{60C}</td><td>{} <i>(</i>{}<i>)</i>.</td>",
                            w[2], w[3], w[4], w[5]
                        ),
                    ),
                ] {
                    out.push((
                        format!("table {{ border-spacing: {sp}; transform: {s} }} td {{ padding: 0; word-spacing: 1em }}"),
                        format!("t/{lang}/{shape}/{sp}"),
                        lang,
                        format!("<table><tr>{cells}</tr></table>"),
                    ));
                }
            }
        }
    }
    // Inline blocks of about half an em between words.
    for (lang, w) in [("he", HE), ("ar", AR)] {
        for width in ["0.3em", "0.4em", "0.6em", "0.8em"] {
            for s in SCALES {
                for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                    let gap =
                        format!("<span style=\"display: inline-block; width: {width}\"></span>");
                    out.push((
                        format!("p {{ transform: {s}; word-spacing: 0.6em }}"),
                        format!("ib/{lang}/{width}/{d}"),
                        lang,
                        format!(
                            "<p{attr}>{} <b>?!</b>{gap}{} {}{gap}{}.</p>",
                            w[0], w[1], w[2], w[3]
                        ),
                    ));
                }
            }
        }
    }
    // Marks of no advance opening a styled run, letter-spaced.
    for ls in ["0.2em", "0.3em", "0.4em", "0.6em"] {
        for s in SCALES {
            for (name, lang, body) in [
                ("lat", "en", "<p>The word cafe<b>\u{301}</b> means \u{5E9}\u{5DC}\u{5D5}\u{5DD} here.</p>"),
                ("heb", "he", "<p dir=\"rtl\">\u{5E9}\u{5DC}<b>\u{5B8}</b>\u{5D5}\u{5DD} \u{5E2}\u{5D5}\u{5DC}\u{5DD} abc<i>\u{301}</i>def.</p>"),
                ("ltr-in-rtl", "he", "<p dir=\"rtl\">\u{5E9}\u{5DC}\u{5D5}\u{5DD} abc<b>\u{301}d</b>ef \u{5E2}\u{5D5}\u{5DC}\u{5DD}.</p>"),
            ] {
                out.push((
                    format!("p {{ transform: {s}; letter-spacing: {ls} }}"),
                    format!("mk/{name}/{ls}"),
                    lang,
                    body.to_owned(),
                ));
            }
        }
    }
    out
}
