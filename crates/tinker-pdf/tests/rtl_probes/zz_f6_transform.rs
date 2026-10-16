//! `zz_f6_transform` (f6, round 6 lane F (sweep)): the books of
//! `tools/probes/rtl/probes/pdf/zz_f6_transform.rs`, which is the probe as it ran.

use super::common::{AR, ARP, Case, HE, HEP, rep, run_book};

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

// Family f6_transform (lane F, round 6 of ROADMAP CD-19): the class of lane V's
// V1, a writer that measures a line for a page other than the one it writes.
// The writer reads each line that holds a right-to-left character back as a
// reader of the page does (paint::Fonts::set_readings), and decides which
// runs a reader may part (paint::PARTED_FROM, paint::page_lines); a CSS
// transform puts a `cm` before the text (paint::OnPage::open), and on the
// page a scale of one axis widens or narrows every gap against the glyphs'
// size, which is what a reader's thresholds are in; a turn or a skew moves
// the baseline. So: right-to-left paragraphs, and a Latin one with a Hebrew
// gloss, under a scale of one axis or both, a turn, a skew, a matrix of its
// own and a shift, on the paragraph, on the block around it, on both, and on
// an inline block inside a line, under word spacing, letter spacing and
// justification, and with none. The inline block is narrowed, `scaleX(0.5)`:
// a transform does not move what is laid out beside its box, and widened it
// draws its words over its neighbours' at every tree, which no reading can
// conserve.

fn cases() -> Vec<Case> {
    let mut out: Vec<Case> = Vec::new();
    for (sc, lang, w, p) in [("ar", "ar", AR, ARP), ("he", "he", HE, HEP)] {
        let s = |i: usize| w[i % w.len()];
        let q = |i: usize| p[i % p.len()];
        let shapes: Vec<(&str, Box<dyn Fn(usize) -> String>)> = vec![
            ("prose", Box::new(move |i| format!("{} {} {}\u{60C} {} {} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4), s(i + 5)))),
            ("styled", Box::new(move |i| format!("{} <b>{}</b> {} <i>{}</i>, ({}).", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("punct", Box::new(move |i| format!("{} <b>?!</b> {} {}. <i>(</i>{}<i>)</i> {}!", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
            ("num", Box::new(move |i| format!("{} 25\u{66A} <b>{}</b> \u{661}\u{662}\u{663} {} 2010\u{60C} {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("gloss", Box::new(move |i| format!("{} {} (lingua franca) {} <i>sic</i> {}.", s(i), s(i + 1), s(i + 2), s(i + 3)))),
            ("marks", Box::new(move |i| format!("{} <b>{}</b> {}\u{60C} {} ({}) {}.", q(i), q(i + 1), q(i + 2), q(i + 3), q(i + 4), q(i + 5)))),
            ("ib", Box::new(move |i| format!("{} <span style=\"display: inline-block; transform: scaleX(0.5)\">{} <b>?!</b> {}</span> {} 25\u{66A} {}.", s(i), s(i + 1), s(i + 2), s(i + 3), s(i + 4)))),
        ];
        for (name, shape) in &shapes {
            let text = rep(3, |k| shape(k));
            for (d, attr) in [("rtl", " dir=\"rtl\""), ("none", "")] {
                out.push((format!("{sc}/{name}/{d}"), lang, format!("<div class=\"w\"><p{attr}>{text}</p></div>")));
            }
        }
    }
    let gloss = rep(3, |k| {
        let h = |i: usize| HE[i % HE.len()];
        format!("The word <b>{}</b> means <i>peace</i>, ({}) 25% {}.", h(k), h(k + 1), h(k + 2))
    });
    out.push(("lat/gloss/none".to_owned(), "en", format!("<div class=\"w\"><p>{gloss}</p></div>")));
    out
}

fn styles() -> Vec<String> {
    // The paragraph's transform, and the block's around it.
    let transforms = [
        ("none", "none"),
        ("scaleX(0.5)", "none"),
        ("scaleX(0.7)", "none"),
        ("scaleX(1.4)", "none"),
        ("scaleX(2)", "none"),
        ("scaleX(3)", "none"),
        ("scaleY(0.5)", "none"),
        ("scaleY(2)", "none"),
        ("scale(0.6)", "none"),
        ("scale(2)", "none"),
        ("rotate(180deg)", "none"),
        ("rotate(90deg)", "none"),
        ("rotate(12deg)", "none"),
        ("skewX(-15deg)", "none"),
        ("skewY(8deg)", "none"),
        ("matrix(1.6, 0, 0, 0.8, 0, 0)", "none"),
        ("translateX(12px)", "none"),
        ("none", "scaleX(2)"),
        ("scaleX(0.5)", "scaleX(2)"),
        ("rotate(180deg)", "scaleX(1.4)"),
        ("scaleX(2) rotate(180deg)", "none"),
    ];
    let spacing = [
        "word-spacing: 0",
        "word-spacing: 0.3em",
        "word-spacing: 0.6em",
        "word-spacing: 1em",
        "letter-spacing: 0.3em",
        "letter-spacing: 0.1em; word-spacing: 0.5em; text-align: justify",
    ];
    let mut out = Vec::new();
    for (p, w) in transforms {
        for s in spacing {
            out.push(format!("p {{ margin: 0 30%; transform: {p}; {s} }} .w {{ transform: {w} }}"));
        }
    }
    out
}
